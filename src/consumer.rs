use std::{io, sync::atomic::Ordering, thread, time::Duration};

use crate::*;

pub fn start_parser_thread(id: usize, files_list: Arc<Mutex<Vec<String>>>, faulty_files_ref: FaultyFilesRef, finish_condition_ref: BoolRef,
        language_content_info_ref: ContentInfoMapRef, language_map: LanguageMapRef, config: Arc<Configuration>) -> JoinHandle<()>
{
    thread::Builder::new().name(id.to_string()).spawn(move || {
        start_parsing_files(files_list, faulty_files_ref, finish_condition_ref, language_content_info_ref, language_map, config);
    }).unwrap()
}

pub fn start_parsing_files(files_list: Arc<Mutex<Vec<String>>>, faulty_files_ref: FaultyFilesRef, finish_condition_ref: BoolRef,
    languages_content_info_ref: ContentInfoMapRef, languages_map_ref: LanguageMapRef, config: Arc<Configuration>) 
{
    let mut buf = String::with_capacity(150);
    loop {
        let mut files_list_guard = files_list.lock().unwrap();
        if let Some(file_path) = files_list_guard.pop() {
            drop(files_list_guard);
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

            match file_parser::parse_file(&file_path, &lang_name, &mut buf, languages_map_ref.clone(), &config) {
                Ok(x) => languages_content_info_ref.lock().unwrap().get_mut(&lang_name).unwrap().add_file_stats(x),
                Err(x) => faulty_files_ref.lock().unwrap().push(
                        FaultyFileDetails::new(file_path.clone(),x,path.metadata().map_or(0, |m| m.len())))
            }
        } else {
            if finish_condition_ref.load(Ordering::Relaxed) {
                break;
            } 
            
            thread::sleep(Duration::from_millis(3));
        }
    }
}
