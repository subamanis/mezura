use std::{collections::HashMap, sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}}, thread, thread::JoinHandle, time::{Duration, Instant}};

use crossbeam_deque::{Injector, Steal, Worker};

use crate::{FileEntry, FilesPerModuleMut, NestedLanguageMapMut, EngineConfig, FaultyFileDetails,
        FaultyFilesListMut, Language, ParsableFile, ScanProgress, Stats, StatsMapMut, phase_timing};
use crate::engine::file_parser;
use crate::languages::NestedLanguageDefinitions;

pub fn start_parser_thread(id: usize, files_injector: Arc<Injector<ParsableFile>>, faulty_files: FaultyFilesListMut, finish_condition: Arc<AtomicBool>,
        stats_per_module: StatsMapMut, nested_per_module: NestedLanguageMapMut, files_per_module: FilesPerModuleMut,
        language_map: Arc<HashMap<String,Language>>, nested_definitions: Arc<NestedLanguageDefinitions>,
        config: Arc<EngineConfig>, started: Instant, counting_ended: Arc<AtomicU64>,
        progress: Arc<ScanProgress>) -> std::io::Result<JoinHandle<()>>
{
    thread::Builder::new().name(format!("consumer-{id}")).spawn(move || {
        start_parsing_files(id, files_injector, faulty_files, finish_condition, stats_per_module,
                nested_per_module, files_per_module, language_map, nested_definitions, config, &progress);
        // The last thing this thread does, and the only honest answer to how long the counting took:
        // 'run' joins these threads after calling the caller's callback, so its own clock cannot tell
        // the two apart.
        counting_ended.fetch_max(started.elapsed().as_millis() as u64, Ordering::Relaxed);
    })
}

pub fn start_parsing_files(_id: usize, files_injector: Arc<Injector<ParsableFile>>, faulty_files: FaultyFilesListMut, finish_condition: Arc<AtomicBool>,
    stats_per_module: StatsMapMut, nested_per_module: NestedLanguageMapMut, files_per_module: FilesPerModuleMut,
    language_map: Arc<HashMap<String,Language>>, nested_definitions: Arc<NestedLanguageDefinitions>,
    config: Arc<EngineConfig>, progress: &ScanProgress)
{
    let mut buf = String::with_capacity(150);
    let mut parse_buffers = file_parser::ParseBuffers::default();
    let mut idle_iterations = 0u32;
    let mut local_faulty: Vec<FaultyFileDetails> = Vec::new();
    let mut keyword_matchers = file_parser::KeywordMatchers::default();
    // The module is an index into the outer vector and never part of the key: a composite one would
    // be an allocation on every file, and a run without modules simply has a vector of one.
    let modules = stats_per_module.lock().unwrap().len();
    let mut local_stats: Vec<HashMap<String, Stats>> =
            vec![HashMap::new(); modules];
    let mut local_nested: Vec<HashMap<String, HashMap<String, Stats>>> =
            vec![HashMap::new(); modules];
    let mut local_files: Vec<HashMap<String, Vec<FileEntry>>> = vec![HashMap::new(); modules];
    // A batch and not one file at a time. With four of these threads per core they all reach for the
    // same queue head between files, and the cost is not the atomic but the losing side: a contended
    // steal comes back as Retry, which the arm below answers by yielding, buying a whole scheduling
    // round per file. The batch halves what is left when the queue runs low, so the last files still
    // spread out rather than queueing behind one thread.
    let worker = Worker::new_fifo();
    loop {
        let next = match worker.pop() {
            Some(parsable_file) => Steal::Success(parsable_file),
            None => files_injector.steal_batch_and_pop(&worker)
        };
        match next {
            Steal::Success(parsable_file) => {
                // Nothing public can kill one of these threads, so the test for what 'run' does about
                // a dead one causes it here. Keyed on a corpus name and not on shared state, since
                // tests run in parallel and a flag either could trip is a race.
                #[cfg(test)]
                if parsable_file.path.to_string_lossy().contains("mezura-dead-consumer") {
                    panic!("test-induced consumer panic");
                }
                // The same door for the opposite question: checking the reported duration needs
                // counting that visibly outlasts the caller's callback, and a corpus big enough to
                // take a known number of milliseconds is a corpus whose timing depends on the
                // machine. This makes it slow by an amount the test chose.
                #[cfg(test)]
                if parsable_file.path.to_string_lossy().contains("mezura-slow-consumer") {
                    thread::sleep(Duration::from_millis(40));
                }
                idle_iterations = 0;
                let lang_name = parsable_file.language_name.as_ref();
                let lookup = file_parser::NestedLanguageLookup { languages: &language_map,
                        extension_to_name: &nested_definitions.extension_to_name,
                        set_aside: &nested_definitions.set_aside };
                match file_parser::parse_file(&parsable_file.path, lang_name, &mut buf, &mut parse_buffers, &lookup, &mut keyword_matchers, &config) {
                    Ok(report) => {
                        progress.record_file_parsed(report.total_lines());
                        let keywords = &language_map.get(lang_name).unwrap().keywords;
                        let bytes = buf.len();
                        let module = parsable_file.module as usize;
                        let mut of_this_file = config.collect_files.then(HashMap::<String, Stats>::new);
                        // The sections are the breakdown of this file, booked beside the row and
                        // never into it; what they weigh is already inside the whole below. A region
                        // that held nothing and one written in the container's own language get no
                        // row: zero lines say nothing, and a container is not a breakdown of itself.
                        for section in report.sections.iter().filter(|section|
                                section.stats.lines > 0 && section.language != lang_name) {
                            let section_keywords = lookup.find_by_name(&section.language)
                                    .map(|inner| inner.keywords.as_slice()).unwrap_or(&[]);
                            local_nested[module].entry(lang_name.to_owned()).or_default()
                                    .entry(section.language.clone()).or_default()
                                    .add_file(&section.stats, section.bytes, section_keywords);
                            if let Some(sections) = &mut of_this_file {
                                sections.entry(section.language.clone()).or_default()
                                        .add(&Stats::new(1, section.bytes, section.stats.lines,
                                                section.stats.classes.clone(), HashMap::new()));
                            }
                        }
                        // The whole file weighs on its own language's row: a container file is one
                        // file of its language, all of its lines included
                        let whole = report.into_whole();
                        // Without its keywords: a map per file is what would cost real memory over a
                        // large tree, and no report shows them per file
                        if let Some(nested_languages) = of_this_file {
                            let entry = FileEntry {
                                    path: spell_out(&parsable_file.path),
                                    stats: Stats::new(1, bytes, whole.lines, whole.classes.clone(),
                                            HashMap::new()),
                                    nested_languages };
                            // Not 'entry(lang_name.to_owned())', which allocates the name per file
                            match local_files[module].get_mut(lang_name) {
                                Some(bucket) => bucket.push(entry),
                                None => { local_files[module].insert(lang_name.to_owned(), vec![entry]); }
                            }
                        }
                        local_stats[module].entry(lang_name.to_owned())
                                .or_default().add_file(&whole, bytes, keywords);
                    },
                    Err(x) => {
                        progress.record_file_parsed(0);
                        local_faulty.push(FaultyFileDetails::new(spell_out(&parsable_file.path), x,
                                parsable_file.path.metadata().map_or(0, |m| m.len())))
                    }
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

    if !local_faulty.is_empty() {
        faulty_files.lock().unwrap().extend(local_faulty);
    }

    if local_stats.iter().any(|bucket| !bucket.is_empty()) {
        let mut global = stats_per_module.lock().unwrap();
        for (module, bucket) in local_stats.iter().enumerate() {
            for (lang_name, stats) in bucket.iter() {
                global[module].entry(lang_name.clone()).or_default().add(stats);
            }
        }
    }
    if local_nested.iter().any(|bucket| !bucket.is_empty()) {
        let mut global = nested_per_module.lock().unwrap();
        for (module, bucket) in local_nested.into_iter().enumerate() {
            for (shell_name, sections) in bucket {
                let shell_entry = global[module].entry(shell_name).or_default();
                for (inner_name, stats) in sections {
                    shell_entry.entry(inner_name).or_default().add(&stats);
                }
            }
        }
    }
    if local_files.iter().any(|bucket| !bucket.is_empty()) {
        let mut global = files_per_module.lock().unwrap();
        for (module, bucket) in local_files.into_iter().enumerate() {
            for (language, files) in bucket {
                global[module].entry(language).or_default().extend(files);
            }
        }
    }
    // println!("Thread {} finished, having done {} files.",_id,share);
}

fn spell_out(path: &std::path::Path) -> String {
    crate::engine::targets::normalise_separators(&path.to_string_lossy()).into_owned()
}
