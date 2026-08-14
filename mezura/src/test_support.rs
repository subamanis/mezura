use std::collections::HashMap;

use mezura_core::{LineClasses, Stats};

// Every line of a counted file lands in exactly one class, so the lines are the classes added up
// and are never given separately: a hand written pair that disagreed would print a third column
// holding a number no class of it accounts for.
pub fn stats_of(files: usize, bytes: usize, classes: LineClasses,
    keyword_occurences: HashMap<String, usize>) -> Stats
{
    let lines = classes.words_in_code + classes.string_content + classes.comment_words_beside_code
            + classes.words_in_comment + classes.punctuation_in_code + classes.punctuation_in_comment
            + classes.blank + classes.blank_in_comment + classes.blank_in_string;

    Stats::new(files, bytes, lines, classes, keyword_occurences)
}

// A file holding nothing that the two counting models read differently: words in code, words in a
// comment, and blank lines outside both, which is what everything left over has to be. For the
// tests whose subject is the layout, the document or the log, where what a line is has no bearing
// on what is asserted; the two models answer alike for such a file, and that is not an accident
// being papered over, it is true of the file.
pub fn plain_stats_of(files: usize, bytes: usize, lines: usize, code: usize, comments: usize,
    keyword_occurences: HashMap<String, usize>) -> Stats
{
    assert!(code + comments <= lines, "{code} code and {comments} comments do not fit in {lines} lines");
    let classes = LineClasses { words_in_code: code, words_in_comment: comments,
            blank: lines - code - comments, ..Default::default() };

    stats_of(files, bytes, classes, keyword_occurences)
}
