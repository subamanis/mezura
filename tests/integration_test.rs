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
