use std::{collections::HashMap, sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}}, thread, thread::JoinHandle, time::{Duration, Instant}};

use crossbeam_deque::{Injector, Steal, Worker};

use crate::{EngineConfig, FaultyFileDetails, FaultyFilesListMut, Language, ParsableFile, Stats,
        StatsMapMut, phase_timing};
use crate::engine::file_parser;

pub fn start_parser_thread(id: usize, files_injector: Arc<Injector<ParsableFile>>, faulty_files: FaultyFilesListMut, finish_condition: Arc<AtomicBool>,
        stats_per_module: StatsMapMut, language_map: Arc<HashMap<String,Language>>,
        config: Arc<EngineConfig>, started: Instant, counting_ended: Arc<AtomicU64>) -> std::io::Result<JoinHandle<()>>
{
    thread::Builder::new().name(format!("consumer-{id}")).spawn(move || {
        start_parsing_files(id, files_injector, faulty_files, finish_condition, stats_per_module, language_map, config);
        // The last thing this thread does, and the only honest answer to how long the counting took.
        // 'run' cannot ask: it joins these threads after calling the caller's callback, so the clock
        // it reads there holds the callback as well whenever the callback finished last, and holds
        // nothing of it whenever the consumers did. Subtracting the callback's own elapsed time was
        // right in the first case and wrong in the second, which is every long run.
        counting_ended.fetch_max(started.elapsed().as_millis() as u64, Ordering::Relaxed);
    })
}

pub fn start_parsing_files(_id: usize, files_injector: Arc<Injector<ParsableFile>>, faulty_files: FaultyFilesListMut, finish_condition: Arc<AtomicBool>,
    stats_per_module: StatsMapMut, language_map: Arc<HashMap<String,Language>>,
    config: Arc<EngineConfig>)
{
    let mut buf = String::with_capacity(150);
    let mut parse_buffers = file_parser::ParseBuffers::default();
    let mut idle_iterations = 0u32;
    let mut keyword_matchers: HashMap<String, Option<file_parser::KeywordMatcher>> = HashMap::new();
    // The module is an index into the outer vector and never part of the key: a composite one would
    // be an allocation on every file, and a run without modules simply has a vector of one.
    let modules = stats_per_module.lock().unwrap().len();
    let mut local_stats: Vec<HashMap<String, Stats>> =
            vec![HashMap::new(); modules];
    // A batch and not a file at a time, because the consumers outnumber the cores four to one and
    // every one of them was reaching for the same injector head between one file and the next. The
    // cost is not the atomic, it is the losing side: a contended steal returns Retry, and the arm
    // below answers it by yielding, which on an oversubscribed machine buys a scheduling round for
    // every file. The batch halves whatever is left when the queue runs low, so the last files
    // still spread out instead of ending up behind one thread.
    let worker = Worker::new_fifo();
    // let mut share = 0;
    loop {
        let next = match worker.pop() {
            Some(parsable_file) => Steal::Success(parsable_file),
            None => files_injector.steal_batch_and_pop(&worker)
        };
        match next {
            Steal::Success(parsable_file) => {
                // A worker that dies mid-run is unreachable from the public surface on purpose,
                // so the test for what 'run' does about one causes it here. Triggered by the name
                // of the file and not by shared state, because tests run in parallel and other
                // ones drive this loop directly: a flag either of them could trip is a race, a
                // name only one corpus carries is not. Compiled out of every real build.
                #[cfg(test)]
                if parsable_file.path.to_string_lossy().contains("mezura-dead-consumer") {
                    panic!("test-induced consumer panic");
                }
                // The same door, for the opposite question. What a run reports as its duration can
                // only be checked against counting that visibly outlasts the caller's callback, and
                // a corpus large enough to take a known number of milliseconds is a corpus whose
                // timing depends on the machine. This makes the counting slow by an amount the test
                // chose, so the assertion is about arithmetic and not about how fast the disk is.
                #[cfg(test)]
                if parsable_file.path.to_string_lossy().contains("mezura-slow-consumer") {
                    thread::sleep(Duration::from_millis(40));
                }
                idle_iterations = 0;
                let lang_name = parsable_file.language_name.as_ref();
                if !keyword_matchers.contains_key(lang_name) {
                    // Hidden keywords are not counted either. Nothing else in the program reads
                    // the counts, not even the log, so the work would be thrown away
                    let built = if config.count_keywords {
                        file_parser::KeywordMatcher::build(language_map.get(lang_name).unwrap())
                    } else {
                        None
                    };
                    keyword_matchers.insert(lang_name.to_owned(), built);
                }
                let keyword_matcher = keyword_matchers.get(lang_name).unwrap().as_ref();
                match file_parser::parse_file(&parsable_file.path, lang_name, &mut buf, &mut parse_buffers, &language_map, keyword_matcher, &config) {
                    Ok(x) => {
                        let keywords = &language_map.get(lang_name).unwrap().keywords;
                        let bytes = buf.len();
                        local_stats[parsable_file.module as usize].entry(lang_name.to_owned())
                                .or_default().add_file(x, bytes, keywords);
                    },
                    // Lossily and with the separators normalised: the walk joins its components
                    // with the platform's own, while the target it started from was resolved to
                    // forward slashes, so the two halves of one path disagreed in every report and
                    // inside the JSON document. Lossy because a path is not required to be UTF-8 on
                    // every platform, and this string is only ever shown: 'to_str().unwrap()' here
                    // panicked on such a name, while holding the lock it had just taken.
                    Err(x) => faulty_files.lock().unwrap().push(FaultyFileDetails::new(
                            parsable_file.path.to_string_lossy().replace('\\', "/"), x,
                            parsable_file.path.metadata().map_or(0, |m| m.len())))
                }
                // Shrinking belongs to whoever owns the buffer, and it has to happen after its
                // length has been read as the file's size
                if buf.capacity() > file_parser::MAX_RETAINED_FILE_BUFFER_BYTES {
                    buf = String::with_capacity(150);
                }
            },
            Steal::Retry => {
                thread::yield_now();
            },
            Steal::Empty => {
                if finish_condition.load(Ordering::Relaxed) {
                    break;
                }

                // An empty queue while the producers are still running is the consumer waiting for
                // work that has not been discovered yet, which is the only real starvation here
                let waited_from = phase_timing::ENABLED.then(phase_timing::now);
                idle_iterations += 1;
                if idle_iterations < 10 {
                    thread::yield_now();
                } else {
                    thread::sleep(Duration::from_millis(2));
                }
                if let Some(from) = waited_from {
                    parse_buffers.timing.starved += 1;
                    parse_buffers.timing.starved_nanos += phase_timing::nanos_since(from);
                }
            }
        }
    }

    if *phase_timing::ENABLED {
        parse_buffers.timing.publish();
    }

    // One lock and one pass, where the two halves used to need two of each
    if local_stats.iter().any(|bucket| !bucket.is_empty()) {
        let mut global = stats_per_module.lock().unwrap();
        for (module, bucket) in local_stats.iter().enumerate() {
            for (lang_name, stats) in bucket.iter() {
                global[module].entry(lang_name.clone()).or_default().add(stats);
            }
        }
    }
    // println!("Thread {} finished, having done {} files.",_id,share);
}
