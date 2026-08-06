// The words the problem is described in, and the bottom of the dependency graph: nothing here knows
// about threads, settings or printing. These exist before anything has been counted, which is what
// separates this file from 'result.rs' beside it.
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
    // Worked out from the symbols above and reused for every file of this language.
    pub(crate) scan_plan : OnceLock<crate::engine::file_parser::ScanPlan>
}

impl Language {
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

// Hand written to leave 'scan_plan' out: it is a cache, filled when a language parses its first
// file, so comparing it would make one language stop equalling its own untouched copy.
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

// 'descriptive_name' is what the report shows, 'aliases' are the spellings that count towards it, so
// 'classes' can be counted from both 'class' and 'record'.
#[derive(Debug,PartialEq)]
pub struct Keyword {
    pub descriptive_name : String,
    pub aliases : Vec<String>
}

impl Keyword {
    pub fn new(descriptive_name: impl AsRef<str>, aliases: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        Keyword {
            descriptive_name : descriptive_name.as_ref().to_owned(),
            aliases : owned_strings(aliases)
        }
    }
}

impl Clone for Keyword {
    fn clone(&self) -> Self {
        Keyword {
            descriptive_name : self.descriptive_name.to_owned(),
            aliases : self.aliases.to_owned()
        }
    }
}

// 'extra_lines' and the average size are methods rather than fields, so a stored copy cannot drift
// from the numbers it comes from.
#[derive(Debug,PartialEq,Default,Clone)]
pub struct Stats {
    pub files : usize,
    pub bytes : usize,
    pub lines : usize,
    pub code_lines : usize,
    pub comment_lines : usize,
    // Added up over a run these are every keyword any language declared, which answers "how many
    // classes are in this project" across the several languages that have such a thing.
    pub keyword_occurences : HashMap<String,usize>
}

impl Stats {
    pub fn new(files: usize, bytes: usize, lines: usize, code_lines: usize, comment_lines: usize,
            keyword_occurences: HashMap<String,usize>) -> Self
    {
        Stats { files, bytes, lines, code_lines, comment_lines, keyword_occurences }
    }

    // The blank lines and the ones carrying no content.
    //
    // Saturating because the fields are public and this is also built from numbers read off a log
    // file: three counts that do not add up are the caller's arithmetic, not a reason to panic.
    pub fn extra_lines(&self) -> usize {
        self.lines.saturating_sub(self.code_lines).saturating_sub(self.comment_lines)
    }

    // Rounded to whole bytes, and zero rather than a division by zero when nothing was counted
    pub fn average_size(&self) -> usize {
        self.bytes.checked_div(self.files).unwrap_or(0)
    }

    // Not public, because its argument is not: the one thing outside this crate could do with it is
    // hand-build a file's counts and get the keyword indices to line up with a slice it also has to
    // supply. The way in from outside is 'run'.
    pub(crate) fn add_file(&mut self, stats: FileStats, bytes: usize, keywords: &[Keyword]) {
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

// What a run starts each language from: every keyword it declares, at zero. The merge that ends a
// counting thread reaches for a keyword by name, and one with no slot waiting would make that
// language count nothing.
impl From<&Language> for Stats {
    fn from(language: &Language) -> Self {
        Stats { keyword_occurences: keyword_slots(language), ..Default::default() }
    }
}

// What one file came to, on its way into a 'Stats'. Kept apart from it for one reason: the parser
// identifies a keyword by its position in the language's list, because that is what the matcher
// hands back, so counting into a vector slot costs no hashing and no string copying in the innermost
// loop of the parse. The names are attached once per file, in 'add_file'.
#[derive(Debug,PartialEq,Default)]
pub(crate) struct FileStats {
    pub lines : usize,
    pub code_lines : usize,
    pub comment_lines : usize,
    pub keyword_occurences : Vec<usize>
}

impl FileStats {
    pub(crate) fn with_keywords(keywords: &[Keyword]) -> Self {
        FileStats {
            lines : 0,
            code_lines : 0,
            comment_lines : 0,
            keyword_occurences : vec![0; keywords.len()]
        }
    }
}

// 'AsRef<str>' and not 'Into<String>': everything ends up owned anyway, and the borrowed form also
// takes a Cow off a path and a reference to any of them.
pub(crate) fn owned_strings(items: impl IntoIterator<Item = impl AsRef<str>>) -> Vec<String> {
    items.into_iter().map(|x| x.as_ref().to_owned()).collect()
}

fn keyword_slots(language: &Language) -> HashMap<String,usize> {
    let mut map = HashMap::<String,usize>::new();
    for keyword in &language.keywords {
        map.insert(keyword.descriptive_name.to_owned(), 0);
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    // The parser cannot produce these: a line is counted once and falls into exactly one of the
    // three. They come off a log file whose head was lost.
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
