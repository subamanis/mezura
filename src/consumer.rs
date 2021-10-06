use std::{sync::atomic::Ordering, thread, time::Duration};

use crossbeam_deque::Steal;

use crate::*;

pub fn start_parser_thread(id: usize, injector: Arc<Injector<String>>, faulty_files_ref: FaultyFilesListMut, finish_condition_ref: Arc<AtomicBool>,
        languages_content_info_ref: ContentInfoMapMut, language_map: Arc<HashMap<String,Language>>, config: Arc<Configuration>) -> JoinHandle<()>
{
    thread::Builder::new().name(id.to_string()).spawn(move || {
        start_parsing_files(id, injector, faulty_files_ref, finish_condition_ref, languages_content_info_ref, language_map, config);
    }).unwrap()
}

pub fn start_parsing_files(_id: usize, injector: Arc<Injector<String>>, faulty_files_ref: FaultyFilesListMut, finish_condition_ref: Arc<AtomicBool>,
    languages_content_info_ref: ContentInfoMapMut, languages_map_ref: Arc<HashMap<String,Language>>, config: Arc<Configuration>) 
{
    let mut buf = String::with_capacity(150);
    // let mut share = 0;
    loop {
        if let Steal::Success(file_path) = &injector.steal() {
            let path = Path::new(&file_path);
            let file_extension = match path.extension() {
                Some(x) => match x.to_str() {
                    Some(y) => y.to_owned(),
                    None => {
                        faulty_files_ref.lock().unwrap().push(FaultyFileDetails::new(file_path.clone().clone(),
                                "could not get the file's extension".to_owned(), path.metadata().map_or(0, |m| m.len()))); 
                        continue;
                    }
                },
                None => {
                    faulty_files_ref.lock().unwrap().push(FaultyFileDetails::new(file_path.clone(),
                        "could not get the file's extension".to_owned(), path.metadata().map_or(0, |m| m.len())));   
                    continue;
                }
            };
            let lang_name = find_lang_with_this_identifier(&languages_map_ref, &file_extension).unwrap();

            // share += 1;
            match file_parser::parse_file(file_path, &lang_name, &mut buf, languages_map_ref.clone(), &config) {
                Ok(x) => languages_content_info_ref.lock().unwrap().get_mut(&lang_name).unwrap().add_file_stats(x),
                Err(x) => faulty_files_ref.lock().unwrap().push(
                        FaultyFileDetails::new(file_path.clone(),x,path.metadata().map_or(0, |m| m.len())))
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
