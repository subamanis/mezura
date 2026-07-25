use std::{sync::atomic::Ordering, thread, time::Duration};

use crossbeam_deque::Steal;

use crate::*;

pub fn start_parser_thread(id: usize, files_injector: Arc<Injector<ParsableFile>>, faulty_files: FaultyFilesListMut, finish_condition: Arc<AtomicBool>,
        languages_content_info: ContentInfoMapMut, language_map: Arc<HashMap<String,Language>>, config: Arc<Configuration>) -> JoinHandle<()>
{
    thread::Builder::new().name(id.to_string()).spawn(move || {
        start_parsing_files(id, files_injector, faulty_files, finish_condition, languages_content_info, language_map, config);
    }).unwrap()
}

pub fn start_parsing_files(_id: usize, files_injector: Arc<Injector<ParsableFile>>, faulty_files: FaultyFilesListMut, finish_condition: Arc<AtomicBool>,
    languages_content_info: ContentInfoMapMut, language_map: Arc<HashMap<String,Language>>, config: Arc<Configuration>) 
{
    let mut buf = String::with_capacity(150);
    let mut idle_iterations = 0u32;
    let mut keyword_matchers: HashMap<String, Option<file_parser::KeywordMatcher>> = HashMap::new();
    // let mut share = 0;
    loop {
        if let Steal::Success(parsable_file) = &files_injector.steal()
        {
            idle_iterations = 0;
            let lang_name = parsable_file.language_name.as_ref();
            if !keyword_matchers.contains_key(lang_name) {
                let built = if config.no_keywords {
                    None
                } else {
                    file_parser::KeywordMatcher::build(language_map.get(lang_name).unwrap())
                };
                keyword_matchers.insert(lang_name.to_owned(), built);
            }
            let keyword_matcher = keyword_matchers.get(lang_name).unwrap().as_ref();
            match file_parser::parse_file(&parsable_file.path, lang_name, &mut buf, language_map.clone(), keyword_matcher, &config) {
                Ok(x) => languages_content_info.lock().unwrap().get_mut(parsable_file.language_name.as_ref()).unwrap().add_file_stats(x),
                Err(x) => faulty_files.lock().unwrap().push(FaultyFileDetails::new(
                        parsable_file.path.to_str().unwrap().to_owned(),x,parsable_file.path.metadata().map_or(0, |m| m.len())))
            }
        } else {
            if finish_condition.load(Ordering::Relaxed) {
                break;
            }

            idle_iterations += 1;
            if idle_iterations < 10 {
                thread::yield_now();
            } else {
                thread::sleep(Duration::from_millis(1));
            }
        }
    }
    // println!("Thread {} finished, having done {} files.",_id,share);
}
