use std::{sync::atomic::Ordering, thread, time::Duration};

use crossbeam_deque::Steal;

use crate::*;

pub fn start_parser_thread(id: usize, files_injector: Arc<Injector<ParsableFile>>, faulty_files_ref: FaultyFilesListMut, finish_condition_ref: Arc<AtomicBool>,
        languages_content_info_ref: ContentInfoMapMut, language_map: Arc<HashMap<String,Language>>, config: Arc<Configuration>) -> JoinHandle<()>
{
    thread::Builder::new().name(id.to_string()).spawn(move || {
        start_parsing_files(id, files_injector, faulty_files_ref, finish_condition_ref, languages_content_info_ref, language_map, config);
    }).unwrap()
}

pub fn start_parsing_files(_id: usize, files_injector: Arc<Injector<ParsableFile>>, faulty_files_ref: FaultyFilesListMut, finish_condition_ref: Arc<AtomicBool>,
    languages_content_info_ref: ContentInfoMapMut, language_map: Arc<HashMap<String,Language>>, config: Arc<Configuration>) 
{
    let mut buf = String::with_capacity(150);
    // let mut share = 0;
    loop {
        if let Steal::Success(parsable_file) = &files_injector.steal() 
        {
            match file_parser::parse_file(&parsable_file.path, &parsable_file.language_name, &mut buf, language_map.clone(), &config) {
                Ok(x) => languages_content_info_ref.lock().unwrap().get_mut(&parsable_file.language_name).unwrap().add_file_stats(x),
                Err(x) => faulty_files_ref.lock().unwrap().push(FaultyFileDetails::new(
                        parsable_file.path.to_str().unwrap().to_owned(),x,parsable_file.path.metadata().map_or(0, |m| m.len())))
            }
        } else {
            if finish_condition_ref.load(Ordering::Relaxed) {
                break;
            } 

            thread::sleep(Duration::from_millis(2));
        }
    }
    // println!("Thread {} finished, having done {} files.",_id,share);
}
