use std::collections::HashMap;

use mezura_core::{FilesPresent, LineClasses, ModuleResult, Performance, RunResult, Stats, Target, Threads};

// The lines are the classes added up and are never passed in: a hand written pair that disagreed
// would print a third column holding a number no class accounts for.
pub fn stats_of(files: usize, bytes: usize, classes: LineClasses,
    keyword_occurences: HashMap<String, usize>) -> Stats
{
    let lines = classes.calculate_lines();

    Stats::new(files, bytes, lines, classes, keyword_occurences)
}

pub fn plain_stats_of(files: usize, bytes: usize, lines: usize, code: usize, comments: usize,
    keyword_occurences: HashMap<String, usize>) -> Stats
{
    assert!(code + comments <= lines, "{code} code and {comments} comments do not fit in {lines} lines");
    let classes = LineClasses { words_in_code: code, words_in_comment: comments,
            blank: lines - code - comments, ..Default::default() };

    stats_of(files, bytes, classes, keyword_occurences)
}

// Only what a test varies is passed in; everything else is the emptiest value a run can produce.
pub fn plain_result_of(per_language: HashMap<String, Stats>, modules: Vec<ModuleResult>,
    targets: Vec<Target>) -> RunResult
{
    RunResult {
        total: Stats::total_of(&per_language), per_language, modules, targets,
        nested_languages: HashMap::new(), faulty_files: Vec::new(), minified_files: 0,
        generated_files: 0, unreadable_dirs: Vec::new(),
        files_present: FilesPresent { total_files: 2, relevant_files: 2, excluded_files: 0 },
        performance: Performance { duration_millis: 0, threads: Threads::new(1, 1) }
    }
}
