use std::collections::HashMap;

use mezura::{EngineConfig, Languages, Target, Threads, language_file, run};

const LANGUAGES_DIR : &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/languages/");

// Two directories, one producer and three consumers, through the published surface and nothing else.
// What a command line would have parsed into an EngineConfig is asserted where the parsing lives.
#[test]
fn a_run_over_two_directories_counts_files_lines_and_keywords() {
    let current_dir = env!("CARGO_MANIFEST_DIR").replace("\\", "/");
    let mut config = EngineConfig::new(vec![format!("{current_dir}/src"), format!("{current_dir}/tests")]);
    config.set_threads(1, 3);

    assert_eq!(Threads::new(1, 3), config.threads);
    assert_eq!(vec![Target::of(format!("{current_dir}/src")), Target::of(format!("{current_dir}/tests"))], config.dirs);

    let language_map = language_file::parse_dir(LANGUAGES_DIR).unwrap().0;
    assert!(!language_map.is_empty());

    let (languages, _) = Languages::resolve(language_map, &HashMap::new(), &config);
    let result = run(&config, languages, |_| {}).unwrap();

    assert!(result.files_present.total_files != 0 && result.files_present.relevant_files != 0);
    assert!(result.faulty_files.is_empty());

    let first_lang_metadata = result.languages_metadata_map.values().next().unwrap();
    assert!(first_lang_metadata.files != 0 && first_lang_metadata.bytes != 0);

    // Readable from outside at all, which they were not before the surface was chosen
    assert_eq!(result.final_stats.lines, result.final_stats.code_lines
            + result.final_stats.comment_lines + result.final_stats.extra_lines);

    let keyword_num = result.content_info_map.values()
            .flat_map(|info| info.keyword_occurences.values()).copied().sum::<usize>();
    assert!(keyword_num != 0);
}

// A run that named its modules and found nothing in them still has to say which modules it was
// asked about. The empty answer went through a shortcut that hardcoded an empty module list, so the
// document dropped the whole block and a reader could not tell "these two parts are empty" apart
// from "no parts were named".
#[test]
fn a_run_that_names_modules_and_finds_nothing_still_reports_them() {
    let root = std::env::temp_dir().join("mezura-empty-modules-test");
    let (api, web) = (root.join("api"), root.join("web"));
    std::fs::create_dir_all(&api).unwrap();
    std::fs::create_dir_all(&web).unwrap();
    let path_of = |p: &std::path::Path| p.to_str().unwrap().replace('\\', "/");

    let mut config = EngineConfig {
        dirs: vec![Target::named("api", path_of(&api)), Target::named("web", path_of(&web))],
        ..Default::default()
    };
    config.set_threads(1, 2);

    let language_map = language_file::parse_dir(LANGUAGES_DIR).unwrap().0;
    let (languages, _) = Languages::resolve(language_map, &HashMap::new(), &config);
    let result = run(&config, languages, |_| {}).unwrap();

    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(0, result.files_present.relevant_files);
    let mut names = result.modules.iter().filter_map(|x| x.name.as_deref()).collect::<Vec<_>>();
    names.sort();
    assert_eq!(vec!["api", "web"], names, "the modules that were asked about are missing from the result");
    assert!(result.modules.iter().all(|x| x.final_stats.lines == 0));
}

// A caller's own exclude pattern that does not parse used to bring the process down through an
// 'expect' whose message blamed an argument parsing that never ran: only the command line validates
// these, and this call never went through it. A mistake in the configuration comes back on the
// Result like every other mistake in the configuration.
#[test]
fn an_exclude_pattern_that_does_not_parse_is_an_error_not_a_panic() {
    let current_dir = env!("CARGO_MANIFEST_DIR").replace("\\", "/");
    let mut config = EngineConfig::new(vec![format!("{current_dir}/src")]);
    config.set_exclude_dirs(vec!["target".to_owned(), "[invalid".to_owned()]);
    config.set_threads(1, 1);

    let language_map = language_file::parse_dir(LANGUAGES_DIR).unwrap().0;
    let (languages, _) = Languages::resolve(language_map, &HashMap::new(), &config);

    let err = run(&config, languages, |_| {}).unwrap_err();
    // Named as the caller wrote it, not in the anchored form the matcher builds internally
    assert!(matches!(&err, mezura::RunError::InvalidExcludePattern(p) if p == "[invalid"),
            "expected InvalidExcludePattern carrying the pattern as written, got: {err:?}");
}

// Files that were all found and all failed to parse are an answer about the corpus, not a failure
// of the run: the counting worked, the counts are zero, and the faulty list says why. Returning it
// as an error is what left '--output json' printing nothing at all in exactly this case, because
// the error arm never reaches the printer.
#[test]
fn a_run_where_every_file_fails_to_parse_is_an_answer_not_an_error() {
    let root = std::env::temp_dir().join("mezura-all-faulty-test");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.rs"), [0xFF, 0xFE, 0x80, b'\n']).unwrap();
    std::fs::write(root.join("b.rs"), [0xFF, 0xFE, 0x80, b'\n']).unwrap();
    let root_str = root.to_str().unwrap().replace('\\', "/");

    let mut config = EngineConfig::new(vec![root_str]);
    config.set_threads(1, 2);
    let language_map = language_file::parse_dir(LANGUAGES_DIR).unwrap().0;
    let (languages, _) = Languages::resolve(language_map, &HashMap::new(), &config);

    let result = run(&config, languages, |_| {}).unwrap();
    std::fs::remove_dir_all(&root).unwrap();

    assert_eq!(2, result.faulty_files.len());
    assert_eq!(2, result.files_present.relevant_files);
    assert!(result.all_relevant_files_were_faulty());
    assert!(result.final_stats.lines == 0 && result.content_info_map.is_empty());

    // The empty scan answers the same question with a no: nothing failed, there was nothing
    let empty = std::env::temp_dir().join("mezura-all-faulty-empty");
    let _ = std::fs::remove_dir_all(&empty);
    std::fs::create_dir_all(&empty).unwrap();
    let mut config = EngineConfig::new(vec![empty.to_str().unwrap().replace('\\', "/")]);
    config.set_threads(1, 1);
    let language_map = language_file::parse_dir(LANGUAGES_DIR).unwrap().0;
    let (languages, _) = Languages::resolve(language_map, &HashMap::new(), &config);
    let result = run(&config, languages, |_| {}).unwrap();
    std::fs::remove_dir_all(&empty).unwrap();

    assert!(!result.all_relevant_files_were_faulty());
}

// Nobody can ask for a thread count the run cannot work with. Zero producers left every directory
// sitting in the queue with nothing to drain it, so a scan of a real tree came back saying it found
// nothing at all; zero consumers was worse, returning a result that claimed relevant files and zero
// of everything in the same breath. Both are silent wrong answers, which is the one thing a counter
// must never produce, and the command line never saw either because it validates its own input.
#[test]
fn a_thread_count_outside_the_supported_range_cannot_reach_the_run() {
    let current_dir = env!("CARGO_MANIFEST_DIR").replace("\\", "/");
    let count_with = |producers: usize, consumers: usize| {
        let mut config = EngineConfig::new(vec![format!("{current_dir}/src")]);
        config.set_threads(producers, consumers);
        let language_map = language_file::parse_dir(LANGUAGES_DIR).unwrap().0;
        let (languages, _) = Languages::resolve(language_map, &HashMap::new(), &config);
        let result = run(&config, languages, |_| {}).unwrap();
        (result.files_present.relevant_files, result.final_stats.lines)
    };

    let sane = count_with(1, 2);
    assert!(sane.0 > 0 && sane.1 > 0, "the control run counted nothing");

    assert_eq!(sane, count_with(0, 0), "zero of both");
    assert_eq!(sane, count_with(0, 4), "zero producers");
    assert_eq!(sane, count_with(2, 0), "zero consumers");
    // Far above the cap, which used to reach Vec::with_capacity and the spawn loop as written
    assert_eq!(sane, count_with(usize::MAX, usize::MAX), "absurdly many");

    // And what was actually used is readable, rather than the number that was asked for
    let mut config = EngineConfig::new(vec![format!("{current_dir}/src")]);
    config.set_threads(0, 100_000);
    assert_eq!((1, 128), (config.threads.producers(), config.threads.consumers()));
}
