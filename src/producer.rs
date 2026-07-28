use std::{fs::ReadDir, thread};

use crossbeam_deque::Steal;

use crate::*;


pub fn start_producer_thread(id: usize, files_injector: Arc<Injector<ParsableFile>>, dirs_injector: Arc<Injector<TraversedDir>>, worker: Worker<TraversedDir>,
        languages_metadata_map: MetadataMapMut, idle_producers: Arc<AtomicUsize>, extension_lang_map: ExtensionLangMap, exclude_matcher: Arc<globset::GlobSet>,
        config: Arc<Configuration>, files_stats: Arc<Mutex<FilesPresent>>)
-> JoinHandle<()>
{
    thread::Builder::new().name(id.to_string()).spawn(move || {
        let (total_files, relevant_files, excluded_files) =
                search_for_files(id, files_injector, dirs_injector, worker, idle_producers, extension_lang_map, exclude_matcher, languages_metadata_map, config);
        let mut file_stats_guard = files_stats.lock().unwrap(); 
        file_stats_guard.total_files += total_files;
        file_stats_guard.relevant_files += relevant_files;
        file_stats_guard.excluded_files += excluded_files;

    }).unwrap()
}

pub fn search_for_files(_id: usize, files_injector: Arc<Injector<ParsableFile>>, dirs_injector: Arc<Injector<TraversedDir>>, worker: Worker<TraversedDir>, idle_producers: Arc<AtomicUsize>,
        extension_lang_map: ExtensionLangMap, exclude_matcher: Arc<globset::GlobSet>, languages_metadata_map: MetadataMapMut, config: Arc<Configuration>)
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
                idle_producers.fetch_sub(1, Ordering::SeqCst);
            }

            if let Ok(entries) = fs::read_dir(&dir.path) {
                let gitignore_stack = if config.no_gitignore {
                    None
                } else {
                    GitignoreStack::extended(&dir.path, dir.gitignore_stack.clone())
                };
                traverse_dir(&files_injector, entries, &dirs_injector, &extension_lang_map, &exclude_matcher, &gitignore_stack,
                        &config, &mut local_metadata, &mut total_files, &mut relevant_files, &mut excluded_files)
            }
        } else {
            if !should_terminate {
                should_terminate = true;
                idle_producers.fetch_add(1, Ordering::SeqCst);
            }
            if idle_producers.load(Ordering::SeqCst) == config.threads.producers {
                break;
            }

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

fn traverse_dir(files_injector: &Arc<Injector<ParsableFile>>, entries: ReadDir, dirs_injector: &Arc<Injector<TraversedDir>>,
        extension_lang_map: &HashMap<String, Arc<str>>, exclude_matcher: &globset::GlobSet, gitignore_stack: &Option<Arc<GitignoreStack>>,
        config: &Configuration, local_metadata: &mut HashMap<String, LanguageMetadata>,
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
                let Some(extension_name) = path_buf.extension().and_then(|x| x.to_str()) else { continue };
                if let Some(lang_name) = find_language_of_extension(extension_lang_map, extension_name) {
                    if !exclude_matcher.is_empty() && exclude_matcher.is_match(&path_buf) {
                        local_excluded_files += 1;
                        continue;
                    }
                    if let Some(stack) = gitignore_stack && stack.is_ignored(&path_buf, false) {
                        local_excluded_files += 1;
                        continue;
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
                let Some(dir_name) = file_name.to_str() else { continue };
                if !config.should_search_in_dotted && dir_name.starts_with('.') { continue; }

                let pathbuf = e.path();
                if !exclude_matcher.is_empty() && exclude_matcher.is_match(&pathbuf) {
                    continue;
                }
                if let Some(stack) = gitignore_stack && stack.is_ignored(&pathbuf, true) {
                    continue;
                }
                dirs_injector.push(TraversedDir::new(pathbuf, gitignore_stack.clone()));
            }
        }
    }

    *total_files += local_total_files;
    *relevant_files += local_relevant_files;
    *excluded_files += local_excluded_files;
}
