use std::sync::atomic::Ordering;

use crate::*;

pub fn add_relevant_files(files_list :LinkedListRef, extensions_metadata_map: &mut HashMap<String,ExtensionMetadata>, finish_condition: BoolRef, 
    extensions: ExtensionsMapRef, config: &Configuration) -> (usize,usize) 
{
   let path = Path::new(&config.path); 
   if path.is_file() {
       if let Some(x) = path.extension() {
           if let Some(y) = x.to_str() {
               if extensions.contains_key(y) {
                   extensions_metadata_map.get_mut(y).unwrap().add_file_meta(path.metadata().map_or(0, |m| m.len() as usize));
                   files_list.lock().unwrap().push_front(config.path.to_string());
                   finish_condition.store(true, Ordering::Relaxed);
                   return (1,1);
               }
           }
       }
       finish_condition.store(true, Ordering::Relaxed);
       (0,0)
   } else {
       let (total_files, relevant_files) = search_dir_and_add_files_to_list(&files_list, extensions_metadata_map, &extensions, config);
       finish_condition.store(true, Ordering::Relaxed);
       (total_files,relevant_files)
   }
} 

fn search_dir_and_add_files_to_list(files_list: &LinkedListRef, extensions_metadata_map: &mut HashMap<String,ExtensionMetadata>,
   extensions: &ExtensionsMapRef, config: &Configuration) -> (usize,usize) 
{
    let mut total_files = 0;
    let mut relevant_files = 0;
    let mut dirs: LinkedList<PathBuf> = LinkedList::new();
    dirs.push_front(Path::new(&config.path).to_path_buf());
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
                        if extensions.contains_key(&extension_name) {
                            if !config.exclude_dirs.is_empty() {
                                let full_path = &path_buf.to_str().unwrap_or("").replace('\\', "/");
                                if config.exclude_dirs.iter().any(|x| full_path.ends_with(x) || x == full_path) {continue;}
                            }

                            relevant_files += 1;
                            let bytes = match path_buf.metadata() {
                                Ok(x) => x.len() as usize,
                                Err(_) => 0
                            };
                            extensions_metadata_map.get_mut(&extension_name).unwrap().add_file_meta(bytes);
                            
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