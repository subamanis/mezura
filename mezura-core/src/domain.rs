// The vocabulary of the problem, and the bottom of the dependency graph: it knows nothing about
// threads, configuration or printing, and both halves of the program speak it. A 'Language' and a
// 'Keyword' exist before anything has been counted, which is what separates this file from
// 'result.rs' next to it.
use std::{collections::HashMap, sync::OnceLock};

#[derive(Debug, Clone)]
pub struct Language {
    pub name: String,
    pub extensions : Vec<String>,
    pub string_symbols : Vec<String>,
    pub comment_symbols : Vec<String>,
    pub multiline_comment_start_symbol : Option<String>,
    pub multiline_comment_end_symbol : Option<String>,
    pub keywords : Vec<Keyword>,
    // What the parser works out once from the symbols above and then reuses for every file of this
    // language. A cache and not part of the vocabulary: it is filled on the first file parsed, its
    // type belongs to the engine, and 'Language::new' is how anyone builds one of these.
    pub(crate) scan_plan : OnceLock<crate::engine::file_parser::ScanPlan>
}

impl PartialEq for Language {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.extensions == other.extensions
            && self.string_symbols == other.string_symbols
            && self.comment_symbols == other.comment_symbols
            && self.multiline_comment_start_symbol == other.multiline_comment_start_symbol
            && self.multiline_comment_end_symbol == other.multiline_comment_end_symbol
            && self.keywords == other.keywords
    }
}

#[derive(Debug,PartialEq)]
pub struct Keyword{
    pub descriptive_name : String,
    pub aliases : Vec<String>
}

// What was counted, of one language or of a whole run: the total is the same measurement added up,
// so it is the same type and not a second one that has to be kept in step with this.
//
// It used to be three: the lines in one struct, the files and bytes in another, and the totals in a
// third that carried neither the keywords nor the same field names. A caller wanting a row of a
// report had to look a language up in two maps by the same key and unwrap the second, which the
// types gave it no reason to believe would be there.
//
// 'extra_lines' and the average size are methods and not fields, because both are arithmetic on
// what is already here and a stored copy is a second answer waiting to disagree.
#[derive(Debug,PartialEq,Default,Clone)]
pub struct Stats {
    pub files : usize,
    pub bytes : usize,
    pub lines : usize,
    pub code_lines : usize,
    pub comment_lines : usize,
    // Per language these are its own keywords; summed over a run they are every keyword that any
    // language declared, which is what answers "how many classes in this project" across the
    // several languages that have such a thing.
    pub keyword_occurences : HashMap<String,usize>
}

#[derive(Debug,PartialEq,Default)]
pub struct FileStats {
    pub lines : usize,
    pub code_lines : usize,
    pub comment_lines : usize,
    pub keyword_occurences : Vec<usize>
}

impl Clone for Keyword {
    fn clone(&self) -> Self {
        Keyword {
            descriptive_name : self.descriptive_name.to_owned(),
            aliases : self.aliases.to_owned()
        }
    }
}

impl Keyword {
    pub fn new(descriptive_name: impl AsRef<str>, aliases: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        Keyword {
            descriptive_name : descriptive_name.as_ref().to_owned(),
            aliases : owned_strings(aliases)
        }
    }
}

impl Language {
    // The multiline comment is the pair or it is nothing, and never one half of it. Two separate
    // options let a caller declare an opener with no closer, which opens a comment that is never
    // closed and hands the rest of every file of that language to it.
    pub fn new(name: impl AsRef<str>,
        extensions: impl IntoIterator<Item = impl AsRef<str>>,
        string_symbols: impl IntoIterator<Item = impl AsRef<str>>,
        comment_symbols: impl IntoIterator<Item = impl AsRef<str>>,
        multiline_comments: Option<(&str, &str)>,
        keywords: impl IntoIterator<Item = Keyword>) -> Self
    {
        let (start, end) = multiline_comments.unzip();
        Language {
            name : name.as_ref().to_owned(),
            extensions : owned_strings(extensions),
            string_symbols : owned_strings(string_symbols),
            comment_symbols : owned_strings(comment_symbols),
            multiline_comment_start_symbol : start.map(str::to_owned),
            multiline_comment_end_symbol : end.map(str::to_owned),
            keywords : keywords.into_iter().collect(),
            scan_plan : OnceLock::new()
        }
    }

    pub fn multiline_start_len(&self) -> usize {
        if let Some(x) = &self.multiline_comment_start_symbol {
            x.len()
        } else {
            0
        }
    }

    pub fn multiline_end_len(&self) -> usize {
        if let Some(x) = &self.multiline_comment_end_symbol {
            x.len()
        } else {
            0
        }
    }

    pub fn supports_multiline_comments(&self) -> bool {
        self.multiline_comment_start_symbol.is_some()
    }
}

impl Stats {
    pub fn new(files: usize, bytes: usize, lines: usize, code_lines: usize, comment_lines: usize,
            keyword_occurences: HashMap<String,usize>) -> Self
    {
        Stats { files, bytes, lines, code_lines, comment_lines, keyword_occurences }
    }

    // Everything on a line that is neither code nor a comment: the blank ones and the ones that
    // carry no content. Derived rather than stored, so it cannot drift from the three it comes from.
    //
    // Saturating for the same reason the division below is checked: the fields are public and this
    // type is built from numbers read off a log file as well as from the parser, so three counts
    // that do not add up are the caller's arithmetic and not a reason to take the process down.
    pub fn extra_lines(&self) -> usize {
        self.lines.saturating_sub(self.code_lines).saturating_sub(self.comment_lines)
    }

    // Rounded to whole bytes, and zero rather than a division by zero when nothing was counted
    pub fn average_size(&self) -> usize {
        self.bytes.checked_div(self.files).unwrap_or(0)
    }

    pub fn add_file(&mut self, stats: FileStats, bytes: usize, keywords: &[Keyword]) {
        self.files += 1;
        self.bytes += bytes;
        self.lines += stats.lines;
        self.code_lines += stats.code_lines;
        self.comment_lines += stats.comment_lines;
        for (keyword_index, occurrences) in stats.keyword_occurences.iter().enumerate() {
            if *occurrences > 0 {
                *self.keyword_occurences.entry(keywords[keyword_index].descriptive_name.clone())
                        .or_default() += *occurrences;
            }
        }
    }

    pub fn add(&mut self, other: &Stats) {
        self.files += other.files;
        self.bytes += other.bytes;
        self.lines += other.lines;
        self.code_lines += other.code_lines;
        self.comment_lines += other.comment_lines;
        for (keyword, occurrences) in other.keyword_occurences.iter() {
            *self.keyword_occurences.entry(keyword.clone()).or_default() += *occurrences;
        }
    }
}

// A language with every keyword it declares set to zero, which is what a run starts from: the merge
// that ends a consumer reaches for a keyword by name, and one that was never given a slot would
// make it a language that counted nothing.
impl From<&Language> for Stats {
    fn from(language: &Language) -> Self {
        Stats { keyword_occurences: get_keyword_stats_map(language), ..Default::default() }
    }
}

impl FileStats {
    pub fn with_keywords(keywords: &[Keyword]) -> Self {
        FileStats {
            lines : 0,
            code_lines : 0,
            comment_lines : 0,
            keyword_occurences : vec![0; keywords.len()]
        }
    }

    pub fn incr_lines(&mut self) {
        self.lines += 1;
    }

    pub fn incr_code_lines(&mut self) {
        self.code_lines += 1;
    }

    pub fn incr_comment_lines(&mut self) {
        self.comment_lines += 1;
    }

    pub fn incr_keyword(&mut self, keyword_index: usize) {
        self.keyword_occurences[keyword_index] += 1;
    }
}

// What every constructor of this crate does with the text it is handed. Taken as 'AsRef<str>' and
// not 'Into<String>', because everything ends up owned anyway and the borrowed form accepts more:
// a literal, a String, a Cow off a path, and a reference to any of them.
pub(crate) fn owned_strings(items: impl IntoIterator<Item = impl AsRef<str>>) -> Vec<String> {
    items.into_iter().map(|x| x.as_ref().to_owned()).collect()
}

fn get_keyword_stats_map(extension: &Language) -> HashMap<String,usize> {
    let mut map = HashMap::<String,usize>::new();
    for k in &extension.keywords {
        map.insert(k.descriptive_name.to_owned(), 0);
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    // The engine cannot produce these: a line is counted once and then falls into exactly one of the
    // three. They arrive from outside, off a log file whose head was lost, and the arithmetic runs
    // before anybody has looked at them. Under 'cargo test' the plain '-' panics here rather than
    // wrapping, which is why this asserts the value and not merely that it returned.
    #[test]
    fn three_counts_that_do_not_add_up_give_no_extra_lines_rather_than_a_panic() {
        assert_eq!(0, Stats::new(1, 0, 0, 0, 900, HashMap::new()).extra_lines());
        assert_eq!(0, Stats::new(1, 0, 40, 900, 50, HashMap::new()).extra_lines());
        assert_eq!(0, Stats::new(1, 0, 100, 60, 40, HashMap::new()).extra_lines());
        assert_eq!(10, Stats::new(1, 0, 100, 60, 30, HashMap::new()).extra_lines());
    }

    #[test]
    fn an_average_size_over_no_files_is_zero_rather_than_a_division_by_zero() {
        assert_eq!(0, Stats::default().average_size());
        assert_eq!(250, Stats::new(4, 1000, 0, 0, 0, HashMap::new()).average_size());
    }
}
