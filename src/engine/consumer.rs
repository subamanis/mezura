use std::{collections::HashMap, sync::{Arc, atomic::{AtomicBool, Ordering}}, thread, thread::JoinHandle, time::Duration};

use crossbeam_deque::{Injector, Steal, Worker};

use crate::{EngineConfig, ContentInfoMapMut, FaultyFileDetails, FaultyFilesListMut, Language, LanguageContentInfo,
        LanguageMetadata, MetadataMapMut, ParsableFile, phase_timing};
use crate::engine::file_parser;

pub fn start_parser_thread(id: usize, files_injector: Arc<Injector<ParsableFile>>, faulty_files: FaultyFilesListMut, finish_condition: Arc<AtomicBool>,
        languages_content_info: ContentInfoMapMut, languages_metadata_map: MetadataMapMut, language_map: Arc<HashMap<String,Language>>,
        config: Arc<EngineConfig>) -> JoinHandle<()>
{
    thread::Builder::new().name(format!("consumer-{id}")).spawn(move || {
        start_parsing_files(id, files_injector, faulty_files, finish_condition, languages_content_info, languages_metadata_map, language_map, config);
    }).unwrap()
}

pub fn start_parsing_files(_id: usize, files_injector: Arc<Injector<ParsableFile>>, faulty_files: FaultyFilesListMut, finish_condition: Arc<AtomicBool>,
    languages_content_info: ContentInfoMapMut, languages_metadata_map: MetadataMapMut, language_map: Arc<HashMap<String,Language>>,
    config: Arc<EngineConfig>) 
{
    let mut buf = String::with_capacity(150);
    let mut parse_buffers = file_parser::ParseBuffers::default();
    let mut idle_iterations = 0u32;
    let mut keyword_matchers: HashMap<String, Option<file_parser::KeywordMatcher>> = HashMap::new();
    // One entry per language holding both halves, so a file still costs a single lookup. The module
    // is an index into the outer vector and never part of the key: a composite one would be an
    // allocation on every file, and a run without modules simply has a vector of one.
    let modules = languages_content_info.lock().unwrap().len();
    let mut local_content_info: Vec<HashMap<String, (LanguageContentInfo, LanguageMetadata)>> =
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
                        let bucket = &mut local_content_info[parsable_file.module as usize];
                        match bucket.get_mut(lang_name) {
                            Some((info, meta)) => { info.add_file_stats(x, keywords); meta.add_file_meta(bytes); },
                            None => { bucket.insert(lang_name.to_owned(),
                                    (LanguageContentInfo::from_file_stats(x, keywords), LanguageMetadata::new(1, bytes))); }
                        }
                    },
                    Err(x) => faulty_files.lock().unwrap().push(FaultyFileDetails::new(
                            parsable_file.path.to_str().unwrap().to_owned(),x,parsable_file.path.metadata().map_or(0, |m| m.len())))
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

    if local_content_info.iter().any(|bucket| !bucket.is_empty()) {
        {
            let mut global_content_info_guard = languages_content_info.lock().unwrap();
            for (module, bucket) in local_content_info.iter().enumerate() {
                for (lang_name, (info, _)) in bucket.iter() {
                    global_content_info_guard[module].get_mut(lang_name).unwrap().add_content_info(info);
                }
            }
        }
        let mut global_metadata_guard = languages_metadata_map.lock().unwrap();
        for (module, bucket) in local_content_info.iter().enumerate() {
            for (lang_name, (_, meta)) in bucket.iter() {
                global_metadata_guard[module].get_mut(lang_name).unwrap().add_metadata(meta);
            }
        }
    }
    // println!("Thread {} finished, having done {} files.",_id,share);
}
