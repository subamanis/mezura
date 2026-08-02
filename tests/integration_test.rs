use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use crossbeam_deque::{Injector, Worker};
use mezura::*;
use mezura::config_manager::{Target, Threads};

#[test]
fn test_whole_workflow () {
    let current_dir = env!("CARGO_MANIFEST_DIR").replace("\\", "/");
    let config = config_manager::create_config_from_args(&format!("{current_dir}/src,{current_dir}/tests --threads 1 3 ")).unwrap();
    let language_map = io_handler::parse_supported_languages_to_map(&LOCAL_APP_PATHS.languages_dir).unwrap().0;
    let language_map_len = language_map.len(); 

    assert_eq!(Threads::new(1,3), config.threads);
    assert_eq!(vec![Target::of(format!("{current_dir}/src")), Target::of(format!("{}/tests", current_dir))], config.dirs);
    assert!(!language_map.is_empty());

    let config = Arc::new(config);
    let mut files_present = FilesPresent::default();
    let faulty_files_ref : FaultyFilesListMut  = Arc::new(Mutex::new(Vec::new()));
    let finish_condition_ref = Arc::new(AtomicBool::new(false));
    let language_map = Arc::new(language_map);
    let modules = Arc::new(Modules::of(&config.dirs));
    let languages_content_info_ref = Arc::new(Mutex::new(make_language_stats(language_map.clone(), modules.count())));
    let files_injector = Arc::new(Injector::new());
    let dirs_injector = Arc::new(Injector::new());
    let idle_producers = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let languages_metadata_map = Arc::new(Mutex::new(make_language_metadata(&language_map, modules.count())));

    assert!(languages_metadata_map.lock().unwrap()[0].len() == language_map_len);

    let extension_lang_map: ExtensionLangMap = Arc::new(make_extension_language_map(&language_map, &HashMap::new(), &HashMap::new()).0);
    calculate_single_file_stats_or_add_to_injector(&config, &dirs_injector, &files_injector, &mut files_present, &extension_lang_map, &modules);

    let exclude_matcher = Arc::new(build_exclude_matcher(&config.exclude_dirs).unwrap());
    let (total_files_num, relevant_files_num, _) = producer::search_for_files(0, files_injector.clone(), dirs_injector.clone(),
         Worker::new_fifo(), idle_producers, extension_lang_map, exclude_matcher, config.clone(), modules);

    finish_condition_ref.store(true, Ordering::Relaxed);
    consumer::start_parsing_files(0, files_injector, faulty_files_ref.clone(), finish_condition_ref, languages_content_info_ref.clone(), languages_metadata_map.clone(),
         language_map.clone(), config);

    let mut content_info_map_guard = languages_content_info_ref.lock();
    let content_info_map = &mut content_info_map_guard.as_deref_mut().unwrap()[0];

    let mut languages_metadata_map_guard = languages_metadata_map.lock();
    let languages_metadata_map = &mut languages_metadata_map_guard.as_deref_mut().unwrap()[0];

    remove_languages_with_0_files(content_info_map, languages_metadata_map);
    
    assert!(relevant_files_num != 0 && total_files_num != 0);
    let first_lang_metadata = languages_metadata_map.iter().next().unwrap().1;
    assert!(first_lang_metadata.files != 0 && first_lang_metadata.bytes != 0);
    assert!(faulty_files_ref.clone().lock().unwrap().is_empty());

    let mut keyword_num = 0;
    for content_info in content_info_map.iter() {
        content_info.1.keyword_occurences.iter().for_each(|x| keyword_num += x.1);
    }
    assert!(keyword_num != 0);
}

fn count_files_of(target: &str, extra_args: &str) -> (usize, usize, usize, Vec<String>) {
    let (total, relevant, excluded, found) = walk(target, extra_args);
    (total, relevant, excluded, found.into_iter().map(|(name, _)| name).collect())
}

// Every file the traversal queued, with the module it was queued under, which is the only place the
// attribution can be seen before the counting folds it into a bucket
fn walk(target: &str, extra_args: &str) -> (usize, usize, usize, Vec<(String, Option<String>)>) {
    let config = Arc::new(config_manager::create_config_from_args(&format!("{target} {extra_args} --threads 1 1")).unwrap());
    let language_map = Arc::new(io_handler::parse_supported_languages_to_map(&LOCAL_APP_PATHS.languages_dir).unwrap().0);
    let files_injector = Arc::new(Injector::new());
    let dirs_injector = Arc::new(Injector::new());
    let idle_producers = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let extension_lang_map: ExtensionLangMap = Arc::new(make_extension_language_map(&language_map, &HashMap::new(), &HashMap::new()).0);
    let modules = Arc::new(Modules::of(&config.dirs));
    let mut files_present = FilesPresent::default();
    calculate_single_file_stats_or_add_to_injector(&config, &dirs_injector, &files_injector, &mut files_present, &extension_lang_map, &modules);

    let exclude_matcher = Arc::new(build_exclude_matcher(&config.exclude_dirs).unwrap());
    let (total, relevant, excluded) = producer::search_for_files(0, files_injector.clone(), dirs_injector,
         Worker::new_fifo(), idle_producers, extension_lang_map, exclude_matcher, config, modules.clone());

    let mut found_files = Vec::new();
    while let crossbeam_deque::Steal::Success(f) = files_injector.steal() {
        found_files.push((f.path.file_name().unwrap().to_str().unwrap().to_owned(),
                modules.name_of(f.module).map(str::to_owned)));
    }
    found_files.sort();

    (total, relevant, excluded, found_files)
}

#[test]
fn test_overlapping_and_globbed_targets_count_every_file_once() {
    let root = std::env::temp_dir().join("mezura_overlap_test");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("sub").join("deep")).unwrap();
    std::fs::write(root.join("a.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.join("sub").join("b.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.join("sub").join("deep").join("c.rs"), "fn main() {}\n").unwrap();
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

    std::fs::remove_dir_all(&root).unwrap();
}

// The attribution happens on the way down and never per file, so what this checks is that the module
// of a directory reaches every file below it, and that a target nested inside another one takes its
// own files back from it however the two were written down.
#[test]
fn every_file_is_attributed_to_exactly_one_module() {
    let root = std::env::temp_dir().join("mezura_modules_test");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("api").join("tests").join("deep")).unwrap();
    std::fs::create_dir_all(root.join("web")).unwrap();
    std::fs::create_dir_all(root.join("loose")).unwrap();
    std::fs::write(root.join("api").join("server.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.join("api").join("tests").join("api_test.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.join("api").join("tests").join("deep").join("nested_test.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.join("web").join("app.js"), "let x = 1;\n").unwrap();
    std::fs::write(root.join("loose").join("script.py"), "x = 1\n").unwrap();
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

    std::fs::remove_dir_all(&root).unwrap();
}

// A junction needs no privilege, where a real symbolic link on Windows does, so it is what the test
// makes there. It is also the harder of the two: it answers no to both 'is_file' and 'is_dir'.
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

// The walk used to follow a link and count everything under it a second time, because the arm that
// handles a directory is reached by everything that is not a file. A link the run was pointed at on
// purpose is a different thing and is still followed: what must not happen is the walk doubling back
// on its own discoveries.
#[test]
fn a_link_found_during_the_walk_is_not_followed_but_one_that_was_asked_for_is() {
    let root = std::env::temp_dir().join("mezura_symlink_test");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("real")).unwrap();
    std::fs::write(root.join("real").join("a.rs"), "fn main() {}\nlet x = 1;\n").unwrap();
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

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn test_gitignore_traversal() {
    let root = std::env::temp_dir().join("mezura_gitignore_test");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::create_dir_all(root.join("ignored_dir")).unwrap();
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join(".gitignore"), "*.py\nignored_dir/\n!keep.py\n").unwrap();
    std::fs::write(root.join("a.py"), "x = 1\n").unwrap();
    std::fs::write(root.join("keep.py"), "x = 1\n").unwrap();
    std::fs::write(root.join("b.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.join("ignored_dir").join("c.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.join("sub").join(".gitignore"), "*.rs\n").unwrap();
    std::fs::write(root.join("sub").join("d.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.join("sub").join("e.py"), "x = 1\n").unwrap();

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

    std::fs::remove_dir_all(&root).unwrap();
}

