#![allow(unreachable_code)]

use std::{cell::RefCell, cmp::max, fs::ReadDir, sync::atomic::Ordering, thread};

use crossbeam_deque::Steal;

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


//doesnt work for single file, what if some are dirs other files in dirs of config??
pub fn start_producer_thread(id: usize, files_injector: Arc<Injector<String>>, dirs_injector: Arc<Injector<PathBuf>>, worker: Worker<PathBuf>, global_languages_metadata_map: Arc<Mutex<HashMap<String, LanguageMetadata>>>,
        local_languages_metadata_map: HashMap<String,LanguageMetadata>,termination_states: Arc<Mutex<Vec<bool>>>, languages: LanguageMapRef,
        config: Arc<Configuration>, files_stats: Arc<Mutex<ProduceResults>>)
-> JoinHandle<()>
{
    thread::Builder::new().name(id.to_string()).spawn(move || {
        let product = produce(id, files_injector, dirs_injector, worker, termination_states, languages, global_languages_metadata_map, local_languages_metadata_map, config);
        let mut file_stats_guard = files_stats.lock().unwrap(); 
        file_stats_guard.0 += product.0;
        file_stats_guard.1 += product.1;

    }).unwrap()
}

fn print_thread_colored_msg(id: usize, msg: String) {
    if id == 0 {
        println!("{}",msg.truecolor(51, 167, 255));
    } else if id == 1 {
        println!("{}",msg.truecolor(255, 179, 71));
    } else {
        println!("{}",msg.truecolor(47, 171, 44));
    }
}

pub fn produce(id: usize, injector: Arc<Injector<String>>,dirs_injector: Arc<Injector<PathBuf>>,  worker: Worker<PathBuf>, termination_states: Arc<Mutex<Vec<bool>>>, languages: LanguageMapRef,
        global_languages_metadata_map: Arc<Mutex<HashMap<String, LanguageMetadata>>>, mut local_languages_metadata_map: HashMap<String,LanguageMetadata>, config: Arc<Configuration>) -> (usize,usize)
{
    let mut total_files = 0;
    let mut relevant_files = 0;
    let mut should_terminate = false;
    let mut times_slept = 0;

    loop {
        let a  = {
            if worker.is_empty() {
                match dirs_injector.steal_batch_and_pop(&worker) {
                    Steal::Success(path) => Some(path),
                    _ => None
                }
            } else {
                worker.pop()
            }
        };

        if let Some(dir) = &a {
           if should_terminate {
                should_terminate = false;
                termination_states.lock().unwrap()[id] = false;
            }

            if let Ok(entries) = fs::read_dir(&dir) {
                traverse_dir(id, &injector, entries, &dirs_injector, &languages, &config, &mut local_languages_metadata_map,
                        &mut total_files, &mut relevant_files)
            }
        } else {
            should_terminate = true;
            let mut termination_states_guard = termination_states.lock().unwrap();
            termination_states_guard[id] = true;
            if termination_states_guard.iter().all(|x| *x==true) {
                break;
            }
            drop(termination_states_guard);

            thread::sleep(Duration::from_micros(50));
            times_slept += 1;
        }
    }

    print_thread_colored_msg(id, format!("Thread {} |  Exits with findings: {:?}",id,(total_files,relevant_files)));

    let mut global_guard = global_languages_metadata_map.lock().unwrap();
    global_guard.iter_mut().for_each(|(s, metadata)| metadata.add_metadata(local_languages_metadata_map.get(s).unwrap()));

    print_thread_colored_msg(id, format!("Thread {} |  Slept {} times. ",id,times_slept));
    (total_files,relevant_files)
}

fn traverse_dir(id: usize, files_injector: &Arc<Injector<String>>, entries: ReadDir, dirs_injector: &Arc<Injector<PathBuf>>,
        languages: &LanguageMapRef, config: &Configuration, languages_metadata_map: &mut HashMap<String,LanguageMetadata>,
        total_files: &mut usize, relevant_files: &mut usize)  
{
    let mut local_total_files = 0;
    let mut local_relevant_files = 0;
    for e in entries.flatten(){
        if let Ok(ft) = e.file_type() {
            if ft.is_file() { 
                local_total_files += 1;
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

                    local_relevant_files += 1;
                    let bytes = match path_buf.metadata() {
                        Ok(x) => x.len() as usize,
                        Err(_) => 0
                    };
                    languages_metadata_map.get_mut(&lang_name).unwrap().add_file_meta(bytes);
                    
                    let str_path = match path_buf.to_str() {
                        Some(y) => y.to_owned(),
                        None => continue
                    };
                    if cfg!(debug_assertions) {
                        print_thread_colored_msg(id, format!("Thread {} |  Adding the file '{}' in the files list",id,str_path));
                    }
                    // files_list.lock().unwrap().push(str_path);
                    files_injector.push(str_path);
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

                let pathbuf = e.path();
                let full_path = &pathbuf.to_str().unwrap_or("").replace('\\', "/");
        
                if !config.exclude_dirs.iter().any(|x| x == dir_name || x == full_path) {
                    dirs_injector.push(pathbuf);
                }
            }
        }
    }

    *total_files += local_total_files;
    *relevant_files += local_relevant_files;
}