use std::{sync::atomic::Ordering, thread, time::Duration};

use crossbeam_deque::Steal;

use crate::*;

pub fn start_parser_thread(id: usize, files_injector: Arc<Injector<ParsableFile>>, faulty_files: FaultyFilesListMut, finish_condition: Arc<AtomicBool>,
        languages_content_info: ContentInfoMapMut, languages_metadata_map: MetadataMapMut, language_map: Arc<HashMap<String,Language>>,
        config: Arc<Configuration>) -> JoinHandle<()>
{
    thread::Builder::new().name(id.to_string()).spawn(move || {
        start_parsing_files(id, files_injector, faulty_files, finish_condition, languages_content_info, languages_metadata_map, language_map, config);
    }).unwrap()
}

pub fn start_parsing_files(_id: usize, files_injector: Arc<Injector<ParsableFile>>, faulty_files: FaultyFilesListMut, finish_condition: Arc<AtomicBool>,
    languages_content_info: ContentInfoMapMut, languages_metadata_map: MetadataMapMut, language_map: Arc<HashMap<String,Language>>,
    config: Arc<Configuration>) 
{
    let mut buf = String::with_capacity(150);
    let mut parse_buffers = file_parser::ParseBuffers::default();
    let mut idle_iterations = 0u32;
    let mut keyword_matchers: HashMap<String, Option<file_parser::KeywordMatcher>> = HashMap::new();
    // One entry per language holding both halves, so a file still costs a single lookup
    let mut local_content_info: HashMap<String, (LanguageContentInfo, LanguageMetadata)> = HashMap::new();
    // let mut share = 0;
    loop {
        match files_injector.steal() {
            Steal::Success(parsable_file) => {
                idle_iterations = 0;
                let lang_name = parsable_file.language_name.as_ref();
                if !keyword_matchers.contains_key(lang_name) {
                    // Hidden keywords are not counted either. Nothing else in the program reads
                    // the counts, not even the log, so the work would be thrown away
                    let built = if config.hidden.keywords {
                        None
                    } else {
                        file_parser::KeywordMatcher::build(language_map.get(lang_name).unwrap())
                    };
                    keyword_matchers.insert(lang_name.to_owned(), built);
                }
                let keyword_matcher = keyword_matchers.get(lang_name).unwrap().as_ref();
                match file_parser::parse_file(&parsable_file.path, lang_name, &mut buf, &mut parse_buffers, &language_map, keyword_matcher, &config) {
                    Ok(x) => {
                        let keywords = &language_map.get(lang_name).unwrap().keywords;
                        let bytes = buf.len();
                        match local_content_info.get_mut(lang_name) {
                            Some((info, meta)) => { info.add_file_stats(x, keywords); meta.add_file_meta(bytes); },
                            None => { local_content_info.insert(lang_name.to_owned(),
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

    if !local_content_info.is_empty() {
        {
            let mut global_content_info_guard = languages_content_info.lock().unwrap();
            for (lang_name, (info, _)) in local_content_info.iter() {
                global_content_info_guard.get_mut(lang_name).unwrap().add_content_info(info);
            }
        }
        let mut global_metadata_guard = languages_metadata_map.lock().unwrap();
        for (lang_name, (_, meta)) in local_content_info.iter() {
            global_metadata_guard.get_mut(lang_name).unwrap().add_metadata(meta);
        }
    }
    // println!("Thread {} finished, having done {} files.",_id,share);
}
