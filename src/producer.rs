use std::{fs::ReadDir, thread};

use crossbeam_deque::Steal;

use crate::*;


pub fn start_producer_thread(id: usize, files_injector: Arc<Injector<ParsableFile>>, dirs_injector: Arc<Injector<TraversedDir>>, worker: Worker<TraversedDir>,
        idle_producers: Arc<AtomicUsize>, extension_lang_map: ExtensionLangMap, exclude_matcher: Arc<globset::GlobSet>,
        config: Arc<Configuration>, files_stats: Arc<Mutex<FilesPresent>>, modules: Arc<Modules>)
-> JoinHandle<()>
{
    thread::Builder::new().name(id.to_string()).spawn(move || {
        let (total_files, relevant_files, excluded_files) =
                search_for_files(id, files_injector, dirs_injector, worker, idle_producers, extension_lang_map, exclude_matcher, config, modules);
        let mut file_stats_guard = files_stats.lock().unwrap(); 
        file_stats_guard.total_files += total_files;
        file_stats_guard.relevant_files += relevant_files;
        file_stats_guard.excluded_files += excluded_files;

    }).unwrap()
}

pub fn search_for_files(_id: usize, files_injector: Arc<Injector<ParsableFile>>, dirs_injector: Arc<Injector<TraversedDir>>, worker: Worker<TraversedDir>, idle_producers: Arc<AtomicUsize>,
        extension_lang_map: ExtensionLangMap, exclude_matcher: Arc<globset::GlobSet>, config: Arc<Configuration>, modules: Arc<Modules>)
-> (usize,usize,usize)
{
    let mut total_files = 0;
    let mut relevant_files = 0;
    let mut excluded_files = 0;
    let mut should_terminate = false;
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
                        &config, &modules, dir.module, &mut total_files, &mut relevant_files, &mut excluded_files)
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

    (total_files,relevant_files,excluded_files)
}

// 'module' is the one this directory belongs to, decided when it was queued. Its entries inherit it,
// and the two lookups below only happen in a run that declared a target inside another target, which
// is the only way a child can belong somewhere other than where its parent does.
fn traverse_dir(files_injector: &Arc<Injector<ParsableFile>>, entries: ReadDir, dirs_injector: &Arc<Injector<TraversedDir>>,
        extension_lang_map: &HashMap<String, Arc<str>>, exclude_matcher: &globset::GlobSet, gitignore_stack: &Option<Arc<GitignoreStack>>,
        config: &Configuration, modules: &Modules, module: ModuleId,
        total_files: &mut usize, relevant_files: &mut usize, excluded_files: &mut usize)
{
    let mut local_total_files = 0;
    let mut local_relevant_files = 0;
    let mut local_excluded_files = 0;
    let (dir_boundaries, file_boundaries) = (modules.has_dir_boundaries(), modules.has_file_boundaries());
    for e in entries.flatten(){
        if let Ok(ft) = e.file_type() {
            // A link is where the files already counted somewhere else would be counted again. It
            // has to be tested before the two arms below and not inside them, because the second
            // one is reached by everything that is not a file: on Windows a junction answers no to
            // both 'is_file' and 'is_dir', so it landed there and was walked as a directory, and a
            // link to a single file landed there too, failed to open and vanished without a word.
            // A target named explicitly is a different matter and is still followed: that one was
            // asked for, and it is the walk's own discoveries that must not double back.
            if ft.is_symlink() {
                continue;
            }
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
                    let module = if file_boundaries {modules.at_file(&path_buf, module)} else {module};
                    // The size is not asked for here any more: the consumer reads the whole file
                    // into a buffer anyway, so its length is the same number for free
                    files_injector.push(ParsableFile::new(path_buf, lang_name, module));
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
                let module = if dir_boundaries {modules.at_dir(&pathbuf, module)} else {module};
                dirs_injector.push(TraversedDir::new(pathbuf, gitignore_stack.clone(), module));
            }
        }
    }

    *total_files += local_total_files;
    *relevant_files += local_relevant_files;
    *excluded_files += local_excluded_files;
}
