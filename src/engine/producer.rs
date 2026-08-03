use std::{collections::HashMap, fs, fs::ReadDir, sync::{Arc, Mutex, atomic::{AtomicUsize, Ordering}},
        thread, thread::JoinHandle, time::Duration};

use crossbeam_deque::{Injector, Steal, Worker};

use crate::{EngineConfig, ExtensionLangMap, FilesPresent, GitignoreStack, ModuleId, Modules,
        ParsableFile, TraversedDir, find_language_of_extension};


pub fn start_producer_thread(id: usize, files_injector: Arc<Injector<ParsableFile>>, dirs_injector: Arc<Injector<TraversedDir>>, worker: Worker<TraversedDir>,
        idle_producers: Arc<AtomicUsize>, extension_lang_map: ExtensionLangMap, exclude_matcher: Arc<globset::GlobSet>,
        config: Arc<EngineConfig>, files_stats: Arc<Mutex<FilesPresent>>, modules: Arc<Modules>)
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
        extension_lang_map: ExtensionLangMap, exclude_matcher: Arc<globset::GlobSet>, config: Arc<EngineConfig>, modules: Arc<Modules>)
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
        config: &EngineConfig, modules: &Modules, module: ModuleId,
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


// These drive the traversal directly instead of going through 'run', because what they are about is
// what the walk queued and under which module, and a result has folded that into buckets by the time
// it is returned.
#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::engine::targets::build_exclude_matcher;
    use crate::{LOCAL_APP_PATHS, calculate_single_file_stats_or_add_to_injector,
            make_extension_language_map};

    fn count_files_of(target: &str, extra_args: &str) -> (usize, usize, usize, Vec<String>) {
        let (total, relevant, excluded, found) = walk(target, extra_args);
        (total, relevant, excluded, found.into_iter().map(|(name, _)| name).collect())
    }

    // Every file the traversal queued, with the module it was queued under, which is the only place
    // the attribution can be seen before the counting folds it into a bucket
    fn walk(target: &str, extra_args: &str) -> (usize, usize, usize, Vec<(String, Option<String>)>) {
        // The rule the real parser applies: whitespace separates one target from the next only once a
        // module has been named, so a path with a space in it survives while nothing is named. These
        // fixtures live under the temporary directory, whose path carries the account name.
        let declares_a_module = target.contains('=');
        let pieces = if declares_a_module {target.split_whitespace().collect::<Vec<_>>()}
                else {target.split(',').collect::<Vec<_>>()};
        let entries = pieces.into_iter().map(str::trim).filter(|x| !x.is_empty())
                .map(|piece| match piece.split_once('=').filter(|_| declares_a_module) {
                    Some((name, path)) => (Some(name.trim().to_owned()), path.trim().to_owned()),
                    None => (None, piece.to_owned())
                }).collect::<Vec<_>>();
        let mut config = EngineConfig::new(Vec::new());
        config.dirs = crate::engine::targets::resolve(&entries, !extra_args.contains("--no-gitignore"),
                extra_args.contains("--search-in-dotted")).unwrap();
        config.set_threads(1, 1)
                .set_no_gitignore(extra_args.contains("--no-gitignore"))
                .set_should_search_in_dotted(extra_args.contains("--search-in-dotted"));
        let config = Arc::new(config);
        let language_map = Arc::new(crate::languages::parse_supported_languages_to_map(&LOCAL_APP_PATHS.languages_dir).unwrap().0);
        let files_injector = Arc::new(Injector::new());
        let dirs_injector = Arc::new(Injector::new());
        let idle_producers = Arc::new(AtomicUsize::new(0));
        let extension_lang_map: ExtensionLangMap = Arc::new(make_extension_language_map(&language_map, &HashMap::new(), &HashMap::new()).0);
        let modules = Arc::new(Modules::of(&config.dirs));
        let mut files_present = FilesPresent::default();
        calculate_single_file_stats_or_add_to_injector(&config, &dirs_injector, &files_injector, &mut files_present, &extension_lang_map, &modules);

        let exclude_matcher = Arc::new(build_exclude_matcher(&config.exclude_dirs).unwrap());
        let (total, relevant, excluded) = search_for_files(0, files_injector.clone(), dirs_injector,
                Worker::new_fifo(), idle_producers, extension_lang_map, exclude_matcher, config, modules.clone());

        let mut found_files = Vec::new();
        while let Steal::Success(f) = files_injector.steal() {
            found_files.push((f.path.file_name().unwrap().to_str().unwrap().to_owned(),
                    modules.name_of(f.module).map(str::to_owned)));
        }
        found_files.sort();

        (total, relevant, excluded, found_files)
    }

    #[test]
    fn test_overlapping_and_globbed_targets_count_every_file_once() {
        let root = std::env::temp_dir().join("mezura_overlap_test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("sub").join("deep")).unwrap();
        fs::write(root.join("a.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("sub").join("b.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("sub").join("deep").join("c.rs"), "fn main() {}\n").unwrap();
        let root = root.to_str().unwrap().replace('\\', "/");

        let (_, _, _, found_files) = count_files_of(&root, "");
        assert_eq!(vec!["a.rs", "b.rs", "c.rs"], found_files);

        // Without pruning, the files of the nested targets would appear multiple times
        let (_, _, _, found_files) = count_files_of(&format!("{root},{root}/sub,{root}/sub/deep"), "");
        assert_eq!(vec!["a.rs", "b.rs", "c.rs"], found_files);

        let (_, _, _, found_files) = count_files_of(&format!("{root}/sub/deep,{root}/sub"), "");
        assert_eq!(vec!["b.rs", "c.rs"], found_files);

        // Glob matches go through the same pruning
        let (_, _, _, found_files) = count_files_of(&format!("{root}/*,{root}/**/*.rs"), "");
        assert_eq!(vec!["a.rs", "b.rs", "c.rs"], found_files);

        fs::remove_dir_all(&root).unwrap();
    }

    // The attribution happens on the way down and never per file, so what this checks is that the
    // module of a directory reaches every file below it, and that a target nested inside another one
    // takes its own files back from it however the two were written down.
    #[test]
    fn every_file_is_attributed_to_exactly_one_module() {
        let root = std::env::temp_dir().join("mezura_modules_test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("api").join("tests").join("deep")).unwrap();
        fs::create_dir_all(root.join("web")).unwrap();
        fs::create_dir_all(root.join("loose")).unwrap();
        fs::write(root.join("api").join("server.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("api").join("tests").join("api_test.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("api").join("tests").join("deep").join("nested_test.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("web").join("app.js"), "let x = 1;\n").unwrap();
        fs::write(root.join("loose").join("script.py"), "x = 1\n").unwrap();
        let root = root.to_str().unwrap().replace('\\', "/");

        let of = |name: &str| Some(name.to_owned());
        let (_, _, _, found) = walk(&format!("backend={root}/api frontend={root}/web"), "");
        assert_eq!(vec![("api_test.rs".to_owned(), of("backend")), ("app.js".to_owned(), of("frontend")),
                        ("nested_test.rs".to_owned(), of("backend")), ("server.rs".to_owned(), of("backend"))], found);

        // The nested target is not walked a second time, and its files still leave the module around it
        let (_, _, _, found) = walk(&format!("backend={root}/api tests={root}/api/tests"), "");
        assert_eq!(vec![("api_test.rs".to_owned(), of("tests")), ("nested_test.rs".to_owned(), of("tests")),
                        ("server.rs".to_owned(), of("backend"))], found);

        // An unnamed target next to a named one keeps everything it holds, minus what the named one took
        let (_, _, _, found) = walk(&format!("{root} tests={root}/api/tests"), "");
        assert_eq!(vec![("api_test.rs".to_owned(), of("tests")), ("app.js".to_owned(), None),
                        ("nested_test.rs".to_owned(), of("tests")), ("script.py".to_owned(), None),
                        ("server.rs".to_owned(), None)], found);

        // A file named on its own is a boundary like any other
        let (_, _, _, found) = walk(&format!("{root}/api entry={root}/api/server.rs"), "");
        assert_eq!(vec![("api_test.rs".to_owned(), None), ("nested_test.rs".to_owned(), None),
                        ("server.rs".to_owned(), of("entry"))], found);

        // and the order the two were written in changes nothing, since the more specific path wins
        let (_, _, _, found) = walk(&format!("tests={root}/api/tests backend={root}/api"), "");
        assert_eq!(vec![("api_test.rs".to_owned(), of("tests")), ("nested_test.rs".to_owned(), of("tests")),
                        ("server.rs".to_owned(), of("backend"))], found);

        fs::remove_dir_all(&root).unwrap();
    }

    // A junction needs no privilege, where a real symbolic link on Windows does, so it is what the
    // test makes there. It is also the harder of the two: it answers no to both 'is_file' and 'is_dir'.
    #[cfg(windows)]
    fn link_dir(original: &Path, link: &Path) {
        let output = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J", &link.to_string_lossy(), &original.to_string_lossy()])
                .output().expect("mklink is part of the shell on every Windows");
        assert!(link.exists(), "could not create a junction: {}", String::from_utf8_lossy(&output.stderr));
    }

    #[cfg(unix)]
    fn link_dir(original: &Path, link: &Path) {
        std::os::unix::fs::symlink(original, link).unwrap();
    }

    // The walk used to follow a link and count everything under it a second time, because the arm
    // that handles a directory is reached by everything that is not a file. A link the run was
    // pointed at on purpose is a different thing and is still followed: what must not happen is the
    // walk doubling back on its own discoveries.
    #[test]
    fn a_link_found_during_the_walk_is_not_followed_but_one_that_was_asked_for_is() {
        let root = std::env::temp_dir().join("mezura_symlink_test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("real")).unwrap();
        fs::write(root.join("real").join("a.rs"), "fn main() {}\nlet x = 1;\n").unwrap();
        link_dir(&root.join("real"), &root.join("linked"));

        let root_str = root.to_str().unwrap().replace('\\', "/");
        let (_, relevant, _, found) = count_files_of(&root_str, "");
        assert_eq!(1, relevant, "the file under the link was counted a second time");
        assert_eq!(vec!["a.rs"], found);

        // and the link named as the target is walked, since that is what was asked for
        let (_, relevant, _, found) = count_files_of(&format!("{root_str}/linked"), "");
        assert_eq!(1, relevant);
        assert_eq!(vec!["a.rs"], found);

        // A pattern is not a name. What it matched was found by the program, the same way the walk
        // finds things, which is already why a match that a .gitignore ignores is dropped.
        let (_, relevant, _, found) = count_files_of(&format!("{root_str}/*"), "");
        assert_eq!(1, relevant, "the link was reached through a pattern and counted a second time");
        assert_eq!(vec!["a.rs"], found);

        // A link to a single file used to take the directory arm too, fail to open and disappear
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("real").join("a.rs"), root.join("real").join("b.rs")).unwrap();
            let (_, relevant, _, found) = count_files_of(&root_str, "");
            assert_eq!(1, relevant);
            assert_eq!(vec!["a.rs"], found);
        }

        fs::remove_dir_all(&root).unwrap();
    }

    // The fixtures sit under the temporary directory, whose path carries the account name, so an
    // account with a space in it used to be split into two targets that do not exist. Naming a
    // module changes the rule and needs the path quoted, which is the parser's job and is tested
    // where the parser lives.
    #[test]
    fn a_target_whose_path_contains_a_space_is_one_target() {
        let root = std::env::temp_dir().join("mezura space test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("a.rs"), "fn main() {}
").unwrap();
        fs::write(root.join("sub").join("b.rs"), "fn main() {}
").unwrap();
        let root = root.to_str().unwrap().replace('\\', "/");

        let (_, relevant, _, found) = count_files_of(&root, "");
        assert_eq!(2, relevant, "a path with a space in it was read as two targets");
        assert_eq!(vec!["a.rs", "b.rs"], found);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn test_gitignore_traversal() {
        let root = std::env::temp_dir().join("mezura_gitignore_test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join("ignored_dir")).unwrap();
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join(".gitignore"), "*.py\nignored_dir/\n!keep.py\n").unwrap();
        fs::write(root.join("a.py"), "x = 1\n").unwrap();
        fs::write(root.join("keep.py"), "x = 1\n").unwrap();
        fs::write(root.join("b.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("ignored_dir").join("c.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("sub").join(".gitignore"), "*.rs\n").unwrap();
        fs::write(root.join("sub").join("d.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("sub").join("e.py"), "x = 1\n").unwrap();

        let root_str = root.to_str().unwrap().replace('\\', "/");

        let (total, relevant, excluded, found_files) = count_files_of(&root_str, "");
        assert_eq!((7, 2, 3), (total, relevant, excluded));
        assert_eq!(vec!["b.rs", "keep.py"], found_files);

        let (total, relevant, excluded, found_files) = count_files_of(&root_str, "--no-gitignore");
        assert_eq!((8, 6, 0), (total, relevant, excluded));
        assert_eq!(vec!["a.py", "b.rs", "c.rs", "d.rs", "e.py", "keep.py"], found_files);

        let (_, relevant, _, found_files) = count_files_of(&format!("{root_str}/ignored_dir"), "");
        assert_eq!(1, relevant);
        assert_eq!(vec!["c.rs"], found_files);

        fs::remove_dir_all(&root).unwrap();
    }
}
