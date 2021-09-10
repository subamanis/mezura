use std::sync::atomic::Ordering;

use crate::*;

pub fn add_relevant_files(files_list :LinkedListRef, languages_metadata_map: &mut HashMap<String,LanguageMetadata>, finish_condition: BoolRef, 
    languages: &LanguageMapRef, config: &Configuration) -> (usize,usize) 
{
    let (mut total_files_sum, mut relevant_files_sum) = (0,0);
    for path in config.dirs.iter() {
        let path_buf = Path::new(path); 
        if path_buf.is_file() {
            if let Some(x) = path_buf.extension() {
                if let Some(y) = x.to_str() {
                    if let Some(lang_name) = find_lang_with_this_identifier(languages, y) {
                        languages_metadata_map.get_mut(&lang_name).unwrap().add_file_meta(path_buf.metadata().map_or(0, |m| m.len() as usize));
                        files_list.lock().unwrap().push_front(path.to_owned());
                        total_files_sum += 1;
                        relevant_files_sum += 1;
                    }
                }
            }
        } else {
            let (total_files, relevant_files) = search_dir_and_add_files_to_list(path, &files_list, languages_metadata_map, &languages, config);
            total_files_sum += total_files;
            relevant_files_sum += relevant_files;
        }
    }

    finish_condition.store(true, Ordering::Relaxed);
    (total_files_sum, relevant_files_sum)
} 

fn search_dir_and_add_files_to_list(current_dir: &str, files_list: &LinkedListRef, languages_metadata_map: &mut HashMap<String,LanguageMetadata>,
   languages: &LanguageMapRef, config: &Configuration) -> (usize,usize) 
{
    let mut total_files = 0;
    let mut relevant_files = 0;
    let mut dirs: LinkedList<PathBuf> = LinkedList::new();
    dirs.push_front(Path::new(current_dir).to_path_buf());
    while let Some(dir) = dirs.pop_front() {
        if let Ok(entries) = fs::read_dir(&dir) {
            for e in entries.flatten(){
                if let Ok(ft) = e.file_type() {
                    if ft.is_file() { 
                        total_files += 1;
                        let path_buf = e.path();
                        let extension_name = match path_buf.extension() {
                            Some(x) => {
                                match x.to_str() {
                                        Some(x) => x.to_owned(),
                                        None => continue
                                    }
                                },
                                None => continue
                            };
                        if let Some(lang_name) = find_lang_with_this_identifier(languages, &extension_name) {
                            if !config.exclude_dirs.is_empty() {
                                let full_path = &path_buf.to_str().unwrap_or("").replace('\\', "/");
                                if config.exclude_dirs.iter().any(|x| full_path.ends_with(x) || x == full_path) {continue;}
                            }

                            relevant_files += 1;
                            let bytes = match path_buf.metadata() {
                                Ok(x) => x.len() as usize,
                                Err(_) => 0
                            };
                            languages_metadata_map.get_mut(&lang_name).unwrap().add_file_meta(bytes);
                            
                            let str_path = match path_buf.to_str() {
                                Some(y) => y.to_owned(),
                                None => continue
                            };
                            files_list.lock().unwrap().push_front(str_path);
                        }
                    } else { //is directory
                        let file_name = e.file_name();
                        let dir_name = match file_name.to_str() {
                            Some(x) => {
                                if !config.should_search_in_dotted && x.starts_with('.') {continue;}
                                else {x}
                            },
                            None => continue
                        };

                        let full_path = &e.path().to_str().unwrap_or("").replace('\\', "/");
                
                        if !config.exclude_dirs.iter().any(|x| x == dir_name || x == full_path) {
                            dirs.push_front(e.path());
                        }
                    }
                }
            }
        }
    }
    (total_files,relevant_files)
}