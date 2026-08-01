use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crossbeam_deque::{Injector, Worker};
use mezura::config_manager::Configuration;
use mezura::*;

const CONSUMER_THREADS: usize = 4;
const UPDATE_ENV_VAR: &str = "MEZURA_UPDATE_GOLDEN";

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("lang")
}

fn golden_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("stats.golden")
}

// Byte sizes are deliberately absent from the report: they are the one figure that differs between a
// CRLF and an LF checkout of the same fixtures, which would break the golden on the CI matrix.
fn render_report(content_info: &HashMap<String, LanguageContentInfo>, metadata: &HashMap<String, LanguageMetadata>) -> String {
    let mut names = content_info.keys().cloned().collect::<Vec<_>>();
    names.sort();

    let (mut total_files, mut total_lines, mut total_code, mut total_comments) = (0, 0, 0, 0);
    let mut report = String::with_capacity(500);
    for name in &names {
        let info = content_info.get(name).unwrap();
        let meta = metadata.get(name).unwrap();
        total_files += meta.files;
        total_lines += info.lines;
        total_code += info.code_lines;
        total_comments += info.comment_lines;

        report.push_str(&format!("{name}\n  files={} lines={} code={} comments={}\n",
                meta.files, info.lines, info.code_lines, info.comment_lines));

        let mut keywords = info.keyword_occurences.iter().collect::<Vec<_>>();
        keywords.sort_by_key(|(name, _)| name.as_str());
        if !keywords.is_empty() {
            let rendered = keywords.iter().map(|(name, count)| format!("{name}={count}")).collect::<Vec<_>>();
            report.push_str(&format!("  {}\n", rendered.join(" ")));
        }
    }

    format!("files={total_files} lines={total_lines} code={total_code} comments={total_comments}\n\n{report}")
}

// One producer fills the queue, then several consumers drain it in parallel, which is where the
// per-thread stats merging happens and where the historical nondeterminism bugs lived.
fn collect_stats() -> String {
    let mut config = Configuration::new(vec![fixtures_root().to_str().unwrap().replace('\\', "/")]);
    // The producer count must match the number of producers actually started, since traversal only
    // terminates once that many of them report idle at the same time
    config.set_threads(1, CONSUMER_THREADS).set_should_show_faulty_files(true);
    let config = Arc::new(config);

    let language_map = Arc::new(io_handler::parse_supported_languages_to_map(&LOCAL_APP_PATHS.languages_dir).unwrap().0);
    let extension_lang_map: ExtensionLangMap = Arc::new(make_extension_language_map(&language_map, &HashMap::new(), &HashMap::new()).0);
    let content_info_ref: ContentInfoMapMut = Arc::new(Mutex::new(make_language_stats(language_map.clone())));
    let metadata_ref = Arc::new(Mutex::new(make_language_metadata(&language_map)));
    let faulty_files_ref: FaultyFilesListMut = Arc::new(Mutex::new(Vec::new()));
    let finish_condition_ref = Arc::new(AtomicBool::new(false));
    let files_injector = Arc::new(Injector::new());
    let dirs_injector = Arc::new(Injector::new());

    let mut files_present = FilesPresent::default();
    calculate_single_file_stats_or_add_to_injector(&config, &dirs_injector, &files_injector, &mut files_present, &extension_lang_map);

    let exclude_matcher = Arc::new(build_exclude_matcher(&config.exclude_dirs).unwrap());
    let (_, relevant_files, _) = producer::search_for_files(0, files_injector.clone(), dirs_injector, Worker::new_fifo(),
            Arc::new(AtomicUsize::new(0)), extension_lang_map, exclude_matcher, config.clone());
    assert!(relevant_files > 0, "the fixture corpus produced no relevant files");

    finish_condition_ref.store(true, Ordering::Relaxed);
    let handles = (0..CONSUMER_THREADS).map(|id| {
        let (files_injector, faulty_files_ref) = (files_injector.clone(), faulty_files_ref.clone());
        let (finish_condition_ref, content_info_ref) = (finish_condition_ref.clone(), content_info_ref.clone());
        let (language_map, config) = (language_map.clone(), config.clone());
        let metadata_ref = metadata_ref.clone();
        std::thread::spawn(move || {
            consumer::start_parsing_files(id, files_injector, faulty_files_ref, finish_condition_ref, content_info_ref, metadata_ref, language_map, config);
        })
    }).collect::<Vec<_>>();
    handles.into_iter().for_each(|handle| handle.join().unwrap());

    let faulty_files = faulty_files_ref.lock().unwrap();
    assert!(faulty_files.is_empty(), "{} fixture(s) failed to parse", faulty_files.len());
    drop(faulty_files);

    let mut content_info_guard = content_info_ref.lock();
    let content_info = content_info_guard.as_deref_mut().unwrap();
    let mut metadata_guard = metadata_ref.lock();
    let metadata = metadata_guard.as_deref_mut().unwrap();
    remove_languages_with_0_files(content_info, metadata);

    render_report(content_info, metadata)
}

#[test]
fn stats_of_the_fixture_corpus_match_the_golden_byte_for_byte() {
    let report = collect_stats();
    let golden = golden_path();

    if std::env::var(UPDATE_ENV_VAR).is_ok() {
        std::fs::write(&golden, &report).unwrap();
        println!("{} was set, rewrote {}", UPDATE_ENV_VAR, golden.display());
        return;
    }

    let expected = std::fs::read_to_string(&golden).unwrap_or_else(|x|
            panic!("cannot read {}: {x}\nRun with {UPDATE_ENV_VAR}=1 to create it.", golden.display()));

    if expected != report {
        let mut differences = Vec::new();
        let (expected_lines, actual_lines) = (expected.lines().collect::<Vec<_>>(), report.lines().collect::<Vec<_>>());
        for i in 0..expected_lines.len().max(actual_lines.len()) {
            let (before, after) = (expected_lines.get(i).unwrap_or(&"<missing>"), actual_lines.get(i).unwrap_or(&"<missing>"));
            if before != after {
                differences.push(format!("line {}: expected \"{before}\", got \"{after}\"", i + 1));
            }
        }
        panic!("\nthe fixture corpus no longer produces the recorded stats:\n  {}\n\nIf the change is intended, rerun with {UPDATE_ENV_VAR}=1 and review the diff of {}.\n",
                differences.join("\n  "), golden.display());
    }
}

#[test]
fn two_runs_over_the_same_corpus_produce_identical_stats() {
    assert_eq!(collect_stats(), collect_stats(), "the same corpus produced different stats across two runs in the same process");
}
