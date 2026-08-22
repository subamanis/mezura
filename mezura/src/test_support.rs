use std::collections::HashMap;

use mezura_core::{LineClasses, Stats};

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
