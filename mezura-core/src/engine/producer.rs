use std::{fs, fs::ReadDir, sync::{Arc, Mutex, atomic::{AtomicUsize, Ordering}},
        thread, thread::JoinHandle, time::Duration};

use crossbeam_deque::{Injector, Steal, Worker};

use crate::{EngineConfig, FilesPresent, GitignoreStack, ParsableFile, ScanProgress,
        SharedModuleLookups, TraversedDir, UnreadableDirDetails};
use crate::engine::identity::ModuleLookups;
use crate::engine::modules::{ModuleId, Modules};

// A panic is caught here rather than read back from 'join', because these threads stop by counting
// how many of them have gone idle against how many started: one that dies without ever going idle
// makes that count unreachable and the rest wait on it forever. The catch marks the dead one idle on
// its way out and records what killed it, which 'run' turns into an error after the joins.
pub(crate) fn start_producer_thread(id: usize, files_injector: Arc<Injector<ParsableFile>>, dirs_injector: Arc<Injector<TraversedDir>>, worker: Worker<TraversedDir>,
        idle_producers: Arc<AtomicUsize>, language_lookups: SharedModuleLookups, exclude_matcher: Arc<globset::GlobSet>,
        config: Arc<EngineConfig>, files_stats: Arc<Mutex<FilesPresent>>, modules: Arc<Modules>,
        unreadable_dirs: Arc<Mutex<Vec<UnreadableDirDetails>>>, producers_total: Arc<AtomicUsize>,
        worker_panics: Arc<Mutex<Vec<String>>>, progress: Arc<ScanProgress>)
-> std::io::Result<JoinHandle<()>>
{
    thread::Builder::new().name(format!("producer-{id}")).spawn(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
                search_for_files(files_injector, dirs_injector, worker, idle_producers.clone(),
                        language_lookups, exclude_matcher, config, modules, &producers_total, &progress)));
        match outcome {
            Ok((total_files, relevant_files, excluded_files, unreadable)) => {
                if !unreadable.is_empty() {
                    unreadable_dirs.lock().unwrap().extend(unreadable);
                }
                let mut file_stats_guard = files_stats.lock().unwrap();
                file_stats_guard.total_files += total_files;
                file_stats_guard.relevant_files += relevant_files;
                file_stats_guard.excluded_files += excluded_files;
            },
            Err(payload) => {
                worker_panics.lock().unwrap().push(crate::panic_message(payload.as_ref()));
                idle_producers.fetch_add(1, Ordering::SeqCst);
            }
        }
    })
}

fn search_for_files(files_injector: Arc<Injector<ParsableFile>>, dirs_injector: Arc<Injector<TraversedDir>>, worker: Worker<TraversedDir>, idle_producers: Arc<AtomicUsize>,
        language_lookups: SharedModuleLookups, exclude_matcher: Arc<globset::GlobSet>, config: Arc<EngineConfig>, modules: Arc<Modules>,
        producers_total: &AtomicUsize, progress: &ScanProgress)
-> (usize,usize,usize,Vec<UnreadableDirDetails>)
{
    let mut total_files = 0;
    let mut relevant_files = 0;
    let mut excluded_files = 0;
    let mut unreadable_dirs = Vec::new();
    let mut should_terminate = false;

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
            // The twin of the hook in the consumer, keyed on a name for the same reason: the walk
            // tests below drive this loop directly and in parallel, so shared state would be a race.
            // Below the reset above, or a thread that had already counted itself idle is counted
            // again by the handler that catches this, and the survivors see one dead thread as two.
            #[cfg(test)]
            if dir.path.to_string_lossy().contains("mezura-dead-producer") {
                panic!("test-induced producer panic");
            }

            match fs::read_dir(&dir.path) {
                Ok(entries) => {
                    let gitignore_stack = GitignoreStack::extend_with_dir(&dir.path,
                            dir.gitignore_stack.clone(), crate::ObeyedIgnoreFiles::of(&config));
                    traverse_dir(&files_injector, entries, &dirs_injector, &language_lookups, &exclude_matcher, &gitignore_stack,
                            &config, &modules, dir.module, &mut total_files, &mut relevant_files, &mut excluded_files, progress)
                },
                // Everything under it goes uncounted and reaches no total, not even the number of
                // files looked at, and nothing else would say so. The reason travels with the path,
                // or a permission and a directory that went away arrive as the same sentence.
                Err(error) => unreadable_dirs.push(UnreadableDirDetails {
                    path: crate::engine::targets::normalise_separators(&dir.path.to_string_lossy()).into_owned(),
                    error_msg: error.to_string()
                })
            }
        } else {
            if !should_terminate {
                should_terminate = true;
                idle_producers.fetch_add(1, Ordering::SeqCst);
            }
            if idle_producers.load(Ordering::SeqCst) == producers_total.load(Ordering::SeqCst) {
                break;
            }

            thread::sleep(Duration::from_micros(50));
        }
    }

    (total_files,relevant_files,excluded_files,unreadable_dirs)
}

// 'module' is decided when the directory is queued and its entries inherit it. The two lookups below
// only happen in a run with a target inside another target.
fn traverse_dir(files_injector: &Injector<ParsableFile>, entries: ReadDir, dirs_injector: &Injector<TraversedDir>,
        language_lookups: &ModuleLookups, exclude_matcher: &globset::GlobSet, gitignore_stack: &Option<Arc<GitignoreStack>>,
        config: &EngineConfig, modules: &Modules, module: ModuleId,
        total_files: &mut usize, relevant_files: &mut usize, excluded_files: &mut usize, progress: &ScanProgress)
{
    let mut local_total_files = 0;
    let mut local_relevant_files = 0;
    let mut local_excluded_files = 0;
    let (dir_boundaries, file_boundaries) = (modules.has_dir_boundaries(), modules.has_file_boundaries());
    for e in entries.flatten(){
        if let Ok(ft) = e.file_type() {
            // A link is where files already counted elsewhere get counted again. Tested before the
            // two arms below and not inside them, because the second is reached by everything that
            // is not a file: on Windows a junction answers no to both 'is_file' and 'is_dir', and a
            // link to one file lands there too, fails to open and vanishes without a word. A target
            // named explicitly is still followed; only what the scan finds by itself must not
            // double back.
            if ft.is_symlink() {
                continue;
            }
            if ft.is_file() {
                local_total_files += 1;
                let path_buf = e.path();
                // Which module the file is in is settled before its language is named, and not
                // after, because a module can be given rules of its own: a file that is a target
                // itself, sitting inside a directory target of another module, would otherwise be
                // identified by the rules of the module it is only passing through.
                let module = if file_boundaries {modules.at_file(&path_buf, module)} else {module};
                let language_lookup = language_lookups.get_of_module(module);
                let claimed = language_lookup.of_path(&path_buf);
                if claimed.is_none() && !language_lookup.needs_a_shebang_probe(&path_buf) {
                    continue;
                }
                // The ignore checks sit between the name lookup and the probe, so a covered file
                // is never opened. Only a claimed file counts as excluded; an unclaimed candidate
                // was never identified, so it stays in the uncounted remainder.
                if (!exclude_matcher.is_empty() && exclude_matcher.is_match(&path_buf))
                        || gitignore_stack.as_ref().is_some_and(|stack| stack.is_ignored(&path_buf, false)) {
                    if claimed.is_some() {
                        local_excluded_files += 1;
                    }
                    continue;
                }
                let Some(lang_name) = claimed.or_else(|| language_lookup.of_shebang(&path_buf)) else {
                    continue;
                };
                local_relevant_files += 1;
                // The size is not asked for: the counting thread reads the file into a buffer
                // anyway, so its length is the same number for free.
                files_injector.push(ParsableFile::new(path_buf, lang_name, module));
                progress.record_file_found();
            } else {
                // Read lossily, and only to ask whether it is dotted, which a lossy reading answers
                // correctly since a leading '.' is ASCII and survives any replacement. Demanding
                // valid UTF-8 skips the whole directory over a name used for nothing else.
                let file_name = e.file_name();
                let dir_name = file_name.to_string_lossy();
                // '--search-in-dotted' opens the directories somebody made, and git's object database
                // is not one: nothing in it is source. Tested by name at every depth, so a submodule
                // or a nested clone is covered too.
                if dir_name == ".git" { continue; }
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

// These drive the traversal directly instead of going through 'run': what they are about is what the
// walk queued and under which module, which a result has already folded into buckets.
#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::engine::config::Target;
    use crate::engine::targets::build_exclude_matcher;
    use crate::calculate_single_file_stats_or_add_to_injector;
    use crate::test_paths::LANGUAGES_DIR;
    use crate::engine::identity::{IdentifiedBy, LanguageLookup, ModuleLookups, build_extension_language_map,
            build_language_map_by};

    fn count_files_of(target: &str, extra_args: &str) -> (usize, usize, usize, Vec<String>) {
        let (total, relevant, excluded, found) = walk(target, extra_args);
        (total, relevant, excluded, found.into_iter().map(|(name, _)| name).collect())
    }

    fn walk(target: &str, extra_args: &str) -> (usize, usize, usize, Vec<(String, Option<String>)>) {
        // The rule the real parser applies: whitespace separates one target from the next only once a
        // module has been named, so a path with a space in it survives while nothing is named.
        let declares_a_module = target.contains('=');
        let pieces = if declares_a_module {target.split_whitespace().collect::<Vec<_>>()}
                else {target.split(',').collect::<Vec<_>>()};
        let declared = pieces.into_iter().map(str::trim).filter(|x| !x.is_empty())
                .map(|piece| match piece.split_once('=').filter(|_| declares_a_module) {
                    Some((name, path)) => Target::named(name.trim(), path.trim()),
                    None => Target::of(piece)
                }).collect::<Vec<_>>();
        let config = EngineConfig {
            targets: declared,
            threads: crate::Threads::new(1, 1),
            no_gitignore: extra_args.contains("--no-gitignore"),
            no_ignore_files: extra_args.contains("--no-ignore-files"),
            should_search_in_dotted: extra_args.contains("--search-in-dotted"),
            ..Default::default()
        };
        // The same first step 'run' takes, with the flags the walk is about to obey
        let targets = crate::engine::targets::resolve(&config.targets, crate::ObeyedIgnoreFiles::of(&config),
                config.should_search_in_dotted).unwrap();
        let config = Arc::new(config);
        let language_map = Arc::new(crate::languages::keyed_by_name(
                crate::language_file::parse_languages_in_dir(LANGUAGES_DIR).unwrap().0));
        let files_injector = Arc::new(Injector::new());
        let dirs_injector = Arc::new(Injector::new());
        let idle_producers = Arc::new(AtomicUsize::new(0));
        let language_lookups: SharedModuleLookups = Arc::new(ModuleLookups::OfTheWholeRun(LanguageLookup {
                        by_extension: build_extension_language_map(&language_map, &Default::default(), &Default::default()).0,
                        by_shebang: build_language_map_by(IdentifiedBy::Shebang, &language_map, &Default::default(), &Default::default()).0,
                        ..Default::default() }));
        let modules = Arc::new(Modules::of(&targets));
        let mut files_present = FilesPresent::default();
        calculate_single_file_stats_or_add_to_injector(&config, &targets, &dirs_injector, &files_injector, &mut files_present, &language_lookups, &modules,
                &ScanProgress::default());

        let exclude_matcher = Arc::new(build_exclude_matcher(&config.exclude_dirs).unwrap());
        let (total, relevant, excluded, _) = search_for_files(files_injector.clone(), dirs_injector,
                Worker::new_fifo(), idle_producers, language_lookups, exclude_matcher, config, modules.clone(),
                &AtomicUsize::new(1), &ScanProgress::default());

        let mut found_files = Vec::new();
        while let Steal::Success(f) = files_injector.steal() {
            found_files.push((f.path.file_name().unwrap().to_str().unwrap().to_owned(),
                    modules.name_of(f.module).map(str::to_owned)));
        }
        found_files.sort();

        (total, relevant, excluded, found_files)
    }

    // Reproduced with a queued path that does not exist, which fails in 'read_dir' the same way a
    // directory deleted or made unreadable between being queued and being opened does.
    #[test]
    fn a_directory_that_cannot_be_read_is_reported_and_not_silently_dropped() {
        let root = std::env::temp_dir().join("mezura_unreadable_dir_test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.rs"), "fn main() {}\n").unwrap();
        let root_str = root.to_str().unwrap().replace('\\', "/");
        let vanished = format!("{root_str}/gone");

        let config = EngineConfig { threads: crate::Threads::new(1, 1), ..EngineConfig::new([&root_str]) };
        let targets = crate::engine::targets::resolve(&config.targets, crate::ObeyedIgnoreFiles::of(&config),
                config.should_search_in_dotted).unwrap();
        let config = Arc::new(config);
        let language_map = Arc::new(crate::languages::keyed_by_name(
                crate::language_file::parse_languages_in_dir(LANGUAGES_DIR).unwrap().0));
        let language_lookups: SharedModuleLookups = Arc::new(ModuleLookups::OfTheWholeRun(
                LanguageLookup { by_extension: build_extension_language_map(&language_map, &Default::default(), &Default::default()).0,
                        ..Default::default() }));
        let modules = Arc::new(Modules::of(&targets));
        let (files_injector, dirs_injector) = (Arc::new(Injector::new()), Arc::new(Injector::new()));
        let mut files_present = FilesPresent::default();
        calculate_single_file_stats_or_add_to_injector(&config, &targets, &dirs_injector, &files_injector,
                &mut files_present, &language_lookups, &modules, &ScanProgress::default());
        dirs_injector.push(TraversedDir::new(std::path::PathBuf::from(&vanished), None, 0));

        let exclude_matcher = Arc::new(build_exclude_matcher(&config.exclude_dirs).unwrap());
        let (total, relevant, _, unreadable) = search_for_files(files_injector, dirs_injector,
                Worker::new_fifo(), Arc::new(AtomicUsize::new(0)), language_lookups, exclude_matcher,
                config, modules, &AtomicUsize::new(1), &ScanProgress::default());

        fs::remove_dir_all(&root).unwrap();

        assert_eq!((1, 1), (total, relevant));
        assert_eq!(vec![vanished], unreadable.iter().map(|x| x.path.clone()).collect::<Vec<_>>(),
                "the directory that could not be read went unreported");
        assert!(!unreadable[0].error_msg.is_empty(), "the reason it could not be read was dropped");
    }

    #[test]
    fn the_git_directory_is_never_walked_even_when_dotted_ones_are() {
        let root = std::env::temp_dir().join("mezura_git_skip_test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".git").join("hooks")).unwrap();
        fs::create_dir_all(root.join("sub").join(".git")).unwrap();
        fs::create_dir_all(root.join(".github")).unwrap();
        fs::write(root.join("a.rs"), "fn main() {}
").unwrap();
        fs::write(root.join(".git").join("hooks").join("pre.rs"), "fn main() {}
").unwrap();
        fs::write(root.join("sub").join(".git").join("nested.rs"), "fn main() {}
").unwrap();
        fs::write(root.join(".github").join("deploy.rs"), "fn main() {}
").unwrap();
        let root_str = root.to_str().unwrap().replace('\\', "/");

        let (_, _, _, found) = count_files_of(&root_str, "");
        assert_eq!(vec!["a.rs"], found, "the dotted rule alone should have kept all three out");

        // The flag opens the ones somebody created, at either depth, and still not the one git keeps
        let (_, _, _, found) = count_files_of(&root_str, "--search-in-dotted");
        assert_eq!(vec!["a.rs", "deploy.rs"], found, "the object database was walked");

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn overlapping_and_globbed_targets_count_every_file_once() {
        let root = std::env::temp_dir().join("mezura_overlap_test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("sub").join("deep")).unwrap();
        fs::write(root.join("a.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("sub").join("b.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("sub").join("deep").join("c.rs"), "fn main() {}\n").unwrap();
        let root = root.to_str().unwrap().replace('\\', "/");

        let (_, _, _, found_files) = count_files_of(&root, "");
        assert_eq!(vec!["a.rs", "b.rs", "c.rs"], found_files);

        let (_, _, _, found_files) = count_files_of(&format!("{root},{root}/sub,{root}/sub/deep"), "");
        assert_eq!(vec!["a.rs", "b.rs", "c.rs"], found_files);

        let (_, _, _, found_files) = count_files_of(&format!("{root}/sub/deep,{root}/sub"), "");
        assert_eq!(vec!["b.rs", "c.rs"], found_files);

        let (_, _, _, found_files) = count_files_of(&format!("{root}/*,{root}/**/*.rs"), "");
        assert_eq!(vec!["a.rs", "b.rs", "c.rs"], found_files);

        fs::remove_dir_all(&root).unwrap();
    }

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

        // A pattern is not a name: what it matched was found by the program, which is already why a
        // match that a .gitignore ignores is dropped.
        let (_, relevant, _, found) = count_files_of(&format!("{root_str}/*"), "");
        assert_eq!(1, relevant, "the link was reached through a pattern and counted a second time");
        assert_eq!(vec!["a.rs"], found);

        // A link to a single file would otherwise take the directory arm, fail to open and disappear
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
    // account with a space in it is the ordinary way in. Naming a module changes the rule and needs
    // the path quoted, which is the parser's job and is tested where the parser lives.
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
    fn an_extensionless_script_is_claimed_through_its_first_line() {
        let root = std::env::temp_dir().join("mezura_shebang_walk_test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".gitignore"), "ignored-deploy\n").unwrap();
        fs::write(root.join("deploy"), "#!/usr/bin/env bash\necho hi\n").unwrap();
        fs::write(root.join("ignored-deploy"), "#!/bin/sh\necho hi\n").unwrap();
        fs::write(root.join("LICENSE"), "MIT License\n").unwrap();
        fs::write(root.join("script.xyz"), "#!/bin/sh\necho hi\n").unwrap();
        fs::write(root.join("a.rs"), "fn main() {}\n").unwrap();
        let root_str = root.to_str().unwrap().replace('\\', "/");

        let (total, relevant, excluded, found) = count_files_of(&root_str, "");
        assert_eq!(vec!["a.rs", "deploy"], found, "the shebang did not claim the script");
        // 'LICENSE', '.gitignore', the '.xyz' carrying a shebang and the ignored script all stay in
        // the remainder: an extension keeps a file out of the probe whatever its first line says,
        // and a file the ignore checks cover was never identified, so it is not 'excluded' either
        assert_eq!((6, 2, 0), (total, relevant, excluded));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn what_a_gitignore_names_is_left_out_of_the_walk() {
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

    // A '.ignore' is what somebody writes to hide a vendored dependency from their search tools
    // while git keeps it, so obeying only the '.gitignore' counts the whole of it. The two flags are
    // separate because the two files answer different questions, and the case that proves they are
    // not one flag is the file each one alone brings back.
    #[test]
    fn the_ignore_files_git_does_not_read_are_obeyed_and_are_turned_off_on_their_own() {
        let root = std::env::temp_dir().join("mezura_ignore_files_test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join("vendor")).unwrap();
        fs::write(root.join(".gitignore"), "built.rs\n").unwrap();
        fs::write(root.join(".ignore"), "vendor/\nbundle.rs\n").unwrap();
        // Last word to the narrowest file: '.rgignore' brings the bundle back over the '.ignore'
        fs::write(root.join(".rgignore"), "!bundle.rs\n").unwrap();
        fs::write(root.join("mine.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("built.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("bundle.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("vendor").join("dep.rs"), "fn main() {}\n").unwrap();

        let root_str = root.to_str().unwrap().replace('\\', "/");

        let (_, _, _, found_files) = count_files_of(&root_str, "");
        assert_eq!(vec!["bundle.rs", "mine.rs"], found_files);

        // Only the search tools' files off: what the '.gitignore' hides is still hidden
        let (_, _, _, found_files) = count_files_of(&root_str, "--no-ignore-files");
        assert_eq!(vec!["bundle.rs", "dep.rs", "mine.rs"], found_files);

        // Only the '.gitignore' off: what the '.ignore' hides is still hidden
        let (_, _, _, found_files) = count_files_of(&root_str, "--no-gitignore");
        assert_eq!(vec!["built.rs", "bundle.rs", "mine.rs"], found_files);

        let (_, _, _, found_files) = count_files_of(&root_str, "--no-gitignore --no-ignore-files");
        assert_eq!(vec!["built.rs", "bundle.rs", "dep.rs", "mine.rs"], found_files);

        fs::remove_dir_all(&root).unwrap();
    }
}
