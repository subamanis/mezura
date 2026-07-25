use std::{fs::ReadDir, thread};

use crossbeam_deque::Steal;

use crate::*;


pub fn start_producer_thread(id: usize, files_injector: Arc<Injector<ParsableFile>>, dirs_injector: Arc<Injector<PathBuf>>, worker: Worker<PathBuf>,
        languages_metadata_map: MetadataMapMut, termination_states: Arc<Mutex<Vec<bool>>>, extension_lang_map: ExtensionLangMap, config: Arc<Configuration>,
        files_stats: Arc<Mutex<FilesPresent>>)
-> JoinHandle<()>
{
    thread::Builder::new().name(id.to_string()).spawn(move || {
        let (total_files, relevant_files, excluded_files) =
                search_for_files(id, files_injector, dirs_injector, worker, termination_states, extension_lang_map, languages_metadata_map, config);
        let mut file_stats_guard = files_stats.lock().unwrap(); 
        file_stats_guard.total_files += total_files;
        file_stats_guard.relevant_files += relevant_files;
        file_stats_guard.excluded_files += excluded_files;

    }).unwrap()
}

pub fn search_for_files(id: usize, files_injector: Arc<Injector<ParsableFile>>, dirs_injector: Arc<Injector<PathBuf>>, worker: Worker<PathBuf>, termination_states: Arc<Mutex<Vec<bool>>>,
        extension_lang_map: ExtensionLangMap, languages_metadata_map: MetadataMapMut, config: Arc<Configuration>)
-> (usize,usize,usize)
{
    let mut total_files = 0;
    let mut relevant_files = 0;
    let mut excluded_files = 0;
    let mut should_terminate = false;
    let mut local_metadata: HashMap<String, LanguageMetadata> = HashMap::new();
    // let mut times_slept = 0;

    loop {
        let next_dir  = {
            if worker.is_empty() {
                match dirs_injector.steal_batch_and_pop(&worker) {
                    Steal::Success(path) => Some(path),
                    Steal::Retry => {
                        thread::yield_now();
                        continue;
                    },
                    Steal::Empty => None
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
                traverse_dir(&files_injector, entries, &dirs_injector, &extension_lang_map, &config, &mut local_metadata,
                        &mut total_files, &mut relevant_files, &mut excluded_files)
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

    if !local_metadata.is_empty() {
        let mut global_metadata_guard = languages_metadata_map.lock().unwrap();
        for (lang_name, metadata) in local_metadata.iter() {
            global_metadata_guard.get_mut(lang_name).unwrap().add_metadata(metadata);
        }
    }

    (total_files,relevant_files,excluded_files)
}

fn traverse_dir(files_injector: &Arc<Injector<ParsableFile>>, entries: ReadDir, dirs_injector: &Arc<Injector<PathBuf>>,
        extension_lang_map: &HashMap<String, Arc<str>>, config: &Configuration, local_metadata: &mut HashMap<String, LanguageMetadata>,
        total_files: &mut usize, relevant_files: &mut usize, excluded_files: &mut usize)
{
    let mut local_total_files = 0;
    let mut local_relevant_files = 0;
    let mut local_excluded_files = 0;
    for e in entries.flatten(){
        if let Ok(ft) = e.file_type() {
            if ft.is_file() { 
                local_total_files += 1;
                let path_buf = e.path();
                let extension_name = match path_buf.extension() {
                    Some(x) => {
                        match x.to_str() {
                                Some(x) => x,
                                None => continue
                            }
                        },
                        None => continue
                };
                if let Some(lang_name) = find_language_of_extension(extension_lang_map, extension_name) {
                    if !config.exclude_dirs.is_empty() {
                        let full_path = &path_buf.to_str().unwrap_or("").replace('\\', "/");
                        if config.exclude_dirs.iter().any(|x| full_path.ends_with(x) || x == full_path) {
                            local_excluded_files += 1;
                            continue;
                        }
                    }

                    local_relevant_files += 1;
                    let bytes = match e.metadata() {
                        Ok(x) => x.len() as usize,
                        Err(_) => 0
                    };

                    match local_metadata.get_mut(lang_name.as_ref()) {
                        Some(metadata) => metadata.add_file_meta(bytes),
                        None => { local_metadata.insert(lang_name.as_ref().to_owned(), LanguageMetadata::new(1, bytes)); }
                    }
                    
                    files_injector.push(ParsableFile::new(path_buf, lang_name));
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
                let is_excluded = !config.exclude_dirs.is_empty() && {
                    let full_path = pathbuf.to_str().unwrap_or("").replace('\\', "/");
                    config.exclude_dirs.iter().any(|x| x == dir_name || *x == full_path)
                };
                if !is_excluded {
                    dirs_injector.push(pathbuf);
                }
            }
        }
    }

    *total_files += local_total_files;
    *relevant_files += local_relevant_files;
    *excluded_files += local_excluded_files;
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