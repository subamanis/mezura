use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_deque::{Injector, Steal, Worker};

use crate::{EngineConfig, FaultyFileDetails, FaultyFilesListMut, FileEntry, FilesPerModuleMut,
        Language, NestedLanguageMapMut, ParsableFile, ScanProgress, ScanSkip, SkippedFiles, Stats,
        StatsMapMut, phase_timing};
use crate::engine::file_parser;
use crate::languages::NestedLanguageDefinitions;

const INITIAL_FILE_BUFFER_BYTES : usize = 150;

pub(crate) fn start_parser_thread(id: usize, files_injector: Arc<Injector<ParsableFile>>, faulty_files: FaultyFilesListMut, finish_condition: Arc<AtomicBool>,
        stats_per_module: StatsMapMut, nested_per_module: NestedLanguageMapMut, files_per_module: FilesPerModuleMut,
        language_map: Arc<HashMap<String,Language>>, nested_definitions: Arc<NestedLanguageDefinitions>,
        language_lookups: crate::SharedModuleLookups,
        config: Arc<EngineConfig>, started: Instant, counting_ended: Arc<AtomicU64>,
        skipped_files: Arc<Mutex<SkippedFiles>>,
        progress: Arc<ScanProgress>) -> std::io::Result<JoinHandle<()>>
{
    thread::Builder::new().name(format!("consumer-{id}")).spawn(move || {
        start_parsing_files(files_injector, faulty_files, finish_condition, stats_per_module,
                nested_per_module, files_per_module, language_map, nested_definitions, language_lookups,
                config, &skipped_files, &progress);
        // The last thing this thread does, and the only honest answer to how long the counting took:
        // 'run' joins these threads after calling the caller's callback, so its own clock cannot tell
        // the two apart.
        counting_ended.fetch_max(started.elapsed().as_millis() as u64, Ordering::Relaxed);
    })
}

fn start_parsing_files(files_injector: Arc<Injector<ParsableFile>>, faulty_files: FaultyFilesListMut, finish_condition: Arc<AtomicBool>,
    stats_per_module: StatsMapMut, nested_per_module: NestedLanguageMapMut, files_per_module: FilesPerModuleMut,
    language_map: Arc<HashMap<String,Language>>, nested_definitions: Arc<NestedLanguageDefinitions>,
    language_lookups: crate::SharedModuleLookups,
    config: Arc<EngineConfig>, skipped_files: &Mutex<SkippedFiles>, progress: &ScanProgress)
{
    let mut buf = String::with_capacity(INITIAL_FILE_BUFFER_BYTES);
    let mut parse_buffers = file_parser::ParseBuffers::default();
    let mut idle_iterations = 0u32;
    let mut local_faulty: Vec<FaultyFileDetails> = Vec::new();
    let mut local_skipped = SkippedFiles::default();
    let mut keyword_matchers = file_parser::KeywordMatchers::default();
    let mut identification_matchers = file_parser::IdentificationMatchers::default();
    // The module is an index into the outer vector and never part of the key: a composite one would
    // be an allocation on every file, and a run without modules simply has a vector of one.
    let modules = stats_per_module.lock().unwrap().len();
    let mut local_stats: Vec<HashMap<String, Stats>> =
            vec![HashMap::new(); modules];
    let mut local_nested: Vec<HashMap<String, HashMap<String, Stats>>> =
            vec![HashMap::new(); modules];
    let mut local_files: Vec<HashMap<String, Vec<FileEntry>>> = vec![HashMap::new(); modules];
    // A batch and not one file at a time. With four of these threads per core they all reach for the
    // same queue head between files, and a contended steal comes back as Retry, which the arm below
    // answers by yielding: a whole scheduling round per file. A batch is half of what is left, so
    // the last files still spread out rather than queueing behind one thread.
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
                // The same for the opposite question: checking the reported duration needs counting
                // that visibly outlasts the caller's callback, and a corpus big enough to take a
                // known number of milliseconds is one whose timing depends on the machine.
                #[cfg(test)]
                if parsable_file.path.to_string_lossy().contains("mezura-slow-consumer") {
                    thread::sleep(Duration::from_millis(40));
                }
                idle_iterations = 0;
                let lang_name = parsable_file.language_name.as_ref();
                let lookup = file_parser::NestedLanguageLookup { languages: &language_map,
                        extension_to_name: &nested_definitions.extension_to_name,
                        set_aside: &nested_definitions.set_aside };
                let shebang_map = &language_lookups.get_of_module(parsable_file.module).by_shebang;
                match file_parser::parse_file(&parsable_file.path, lang_name, &mut buf, &mut parse_buffers,
                        &lookup, &mut keyword_matchers, &mut identification_matchers, &config,
                        parsable_file.written_by_hand, parsable_file.extension_rules.as_deref(),
                        shebang_map) {
                    Ok(file_parser::FileOutcome::Counted(report, resolved)) => {
                        let lang_name = resolved.as_deref().unwrap_or(lang_name);
                        progress.record_file_parsed(report.total_lines());
                        let keywords = &language_map.get(lang_name).unwrap().keywords;
                        let bytes = buf.len();
                        let module = parsable_file.module as usize;
                        let mut of_this_file = config.collect_files.then(HashMap::<String, Stats>::new);
                        // A nested section is booked beside the file's own row and never into it:
                        // its lines are already inside the whole below. A section that held nothing,
                        // and one written in the container's own language, get no row at all.
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
                        // The whole file weighs on its own language's row, its nested lines included
                        let whole = report.into_whole();
                        // No keywords per file: a map each would cost real memory over a large tree,
                        // and no report shows them
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
                        match local_stats[module].get_mut(lang_name) {
                            Some(stats) => stats.add_file(&whole, bytes, keywords),
                            None => { local_stats[module].entry(lang_name.to_owned())
                                    .or_default().add_file(&whole, bytes, keywords); }
                        }
                    },
                    Ok(file_parser::FileOutcome::Skipped(kind)) => {
                        progress.record_file_parsed(0);
                        local_skipped.get_of_kind_mut(kind).push(spell_out(&parsable_file.path));
                    },
                    Err(x) => {
                        progress.record_file_parsed(0);
                        local_faulty.push(FaultyFileDetails::new(spell_out(&parsable_file.path), x,
                                parsable_file.path.metadata().map_or(0, |m| m.len())))
                    }
                }
                // Only after the buffer's length has been read as the file's size, never before
                if buf.capacity() > file_parser::MAX_RETAINED_FILE_BUFFER_BYTES {
                    buf = String::with_capacity(INITIAL_FILE_BUFFER_BYTES);
                }
            },
            Steal::Retry => {
                thread::yield_now();
            },
            Steal::Empty => {
                if finish_condition.load(Ordering::Relaxed) {
                    break;
                }

                // Timed only while the producers are still running: a consumer waiting for work that
                // has not been discovered yet is the only real starvation here
                let waited_from = phase_timing::ENABLED.then(Instant::now);
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
    if local_skipped.calculate_files() > 0 {
        let mut global = skipped_files.lock().unwrap();
        for kind in ScanSkip::ALL {
            global.get_of_kind_mut(kind).append(local_skipped.get_of_kind_mut(kind));
        }
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
}

fn spell_out(path: &std::path::Path) -> String {
    crate::engine::targets::normalise_separators(&path.to_string_lossy()).into_owned()
}
