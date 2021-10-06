use std::{fs::ReadDir, thread};

use crossbeam_deque::Steal;

use crate::*;


pub fn start_producer_thread(id: usize, files_injector: Arc<Injector<String>>, dirs_injector: Arc<Injector<PathBuf>>, worker: Worker<PathBuf>,
        languages_metadata_map: MetadataMapMut, termination_states: Arc<Mutex<Vec<bool>>>, languages: Arc<HashMap<String,Language>>, config: Arc<Configuration>,
        files_stats: Arc<Mutex<FilesPresent>>)
-> JoinHandle<()>
{
    thread::Builder::new().name(id.to_string()).spawn(move || {
        let product = produce(id, files_injector, dirs_injector, worker, termination_states, languages, languages_metadata_map, config);
        let mut file_stats_guard = files_stats.lock().unwrap(); 
        file_stats_guard.total_files += product.0;
        file_stats_guard.relevant_files += product.1;

    }).unwrap()
}

pub fn produce(id: usize, files_injector: Arc<Injector<String>>, dirs_injector: Arc<Injector<PathBuf>>, worker: Worker<PathBuf>, termination_states: Arc<Mutex<Vec<bool>>>,
        languages: Arc<HashMap<String,Language>>, languages_metadata_map: MetadataMapMut, config: Arc<Configuration>) 
-> (usize,usize) 
{
    let mut total_files = 0;
    let mut relevant_files = 0;
    let mut should_terminate = false;
    // let mut times_slept = 0;

    loop {
        let next_dir  = {
            if worker.is_empty() {
                match dirs_injector.steal_batch_and_pop(&worker) {
                    Steal::Success(path) => Some(path),
                    _ => None
                }
            } else {
                worker.pop()
            }
        };

        if let Some(dir) = &next_dir {
           if should_terminate {
                should_terminate = false;
                termination_states.lock().unwrap()[id] = false;
            }

            if let Ok(entries) = fs::read_dir(&dir) {
                traverse_dir(&files_injector, entries, &dirs_injector, &languages, &config, &languages_metadata_map,
                        &mut total_files, &mut relevant_files)
            }
        } else {
            should_terminate = true;
            let mut termination_states_guard = termination_states.lock().unwrap();
            termination_states_guard[id] = true;
            if termination_states_guard.iter().all(|x| *x) {
                break;
            }
            drop(termination_states_guard);

            thread::sleep(Duration::from_micros(50));
            // times_slept += 1;
        }
    }

    // print_thread_colored_msg(id, format!("Thread {} |  Exits with findings: {:?}",id,(total_files,relevant_files)));
    // print_thread_colored_msg(id, format!("Thread {} |  Slept {} times. ",id,times_slept));

    (total_files,relevant_files)
}

fn traverse_dir(files_injector: &Arc<Injector<String>>, entries: ReadDir, dirs_injector: &Arc<Injector<PathBuf>>,
        languages: &Arc<HashMap<String,Language>>, config: &Configuration, languages_metadata_map: &MetadataMapMut,
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
                    languages_metadata_map.lock().unwrap().get_mut(&lang_name).unwrap().add_file_meta(bytes);
                    // local_languages_metadata_map.get_mut(&lang_name).unwrap().add_file_meta(bytes);
                    
                    let str_path = match path_buf.to_str() {
                        Some(y) => y.to_owned(),
                        None => continue
                    };

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

#[cfg(debug_assertions)]
fn print_thread_colored_msg(id: usize, msg: String) {
    if id == 0 {
        println!("{}",msg.truecolor(51, 167, 255));
    } else if id == 1 {
        println!("{}",msg.truecolor(255, 179, 71));
    } else {
        println!("{}",msg.truecolor(47, 171, 44));
    }
}