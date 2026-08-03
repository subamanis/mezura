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
    pub scan_plan : OnceLock<crate::engine::file_parser::ScanPlan>
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

#[derive(Debug,PartialEq,Clone)]
pub struct LanguageContentInfo {
    pub lines : usize,
    pub code_lines : usize,
    pub comment_lines : usize,
    pub keyword_occurences : HashMap<String,usize>
}

#[derive(Debug,PartialEq,Default,Clone)]
pub struct LanguageMetadata {
    pub files: usize,
    pub bytes: usize
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

impl Language {
    pub fn new(name: String, extensions: Vec<String>, string_symbols: Vec<String>, comment_symbols: Vec<String>,
        multiline_comment_start_symbol: Option<String>, multiline_comment_end_symbol: Option<String>,
        keywords: Vec<Keyword>) -> Self
    {
        Language {
            name,
            extensions,
            string_symbols,
            comment_symbols,
            multiline_comment_start_symbol,
            multiline_comment_end_symbol,
            keywords,
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

impl LanguageContentInfo {
    pub fn new(lines: usize, code_lines: usize, comment_lines: usize, keyword_occurences: HashMap<String,usize>) -> Self {
        LanguageContentInfo {
            lines,
            code_lines,
            comment_lines,
            keyword_occurences
        }
    }

    pub fn dummy(lines: usize) -> LanguageContentInfo {
        LanguageContentInfo {
            lines,
            code_lines: 0,
            comment_lines: 0,
            keyword_occurences: HashMap::new()
        }
    }

    pub fn add_file_stats(&mut self, other: FileStats, keywords: &[Keyword]) {
        self.lines += other.lines;
        self.code_lines += other.code_lines;
        self.comment_lines += other.comment_lines;
        for (keyword_index, occurrences) in other.keyword_occurences.iter().enumerate() {
            if *occurrences > 0 {
                *self.keyword_occurences.get_mut(&keywords[keyword_index].descriptive_name).unwrap() += *occurrences;
            }
        }
    }

    pub fn from_file_stats(stats: FileStats, keywords: &[Keyword]) -> LanguageContentInfo {
        let mut keyword_occurences = HashMap::<String,usize>::new();
        for (keyword_index, occurrences) in stats.keyword_occurences.iter().enumerate() {
            keyword_occurences.insert(keywords[keyword_index].descriptive_name.clone(), *occurrences);
        }
        LanguageContentInfo {
            lines : stats.lines,
            code_lines : stats.code_lines,
            comment_lines : stats.comment_lines,
            keyword_occurences
        }
    }

    pub fn add_content_info(&mut self, other: &LanguageContentInfo) {
        self.lines += other.lines;
        self.code_lines += other.code_lines;
        self.comment_lines += other.comment_lines;
        for (k,v) in other.keyword_occurences.iter() {
            *self.keyword_occurences.get_mut(k).unwrap() += *v;
        }
    }
}

impl From<&Language> for LanguageContentInfo {
    fn from(ext: &Language) -> Self {
        LanguageContentInfo {
            lines : 0,
            code_lines : 0,
            comment_lines : 0,
            keyword_occurences : get_keyword_stats_map(ext)
        }
    }
}

impl LanguageMetadata {
    pub fn new(files: usize, bytes: usize) ->  Self {
        LanguageMetadata {
            files,
            bytes
        }
    }

    pub fn add_file_meta(&mut self, bytes: usize) {
        self.files += 1;
        self.bytes += bytes;
    }

    pub fn add_metadata(&mut self, other_metadata: &LanguageMetadata) {
        self.files += other_metadata.files;
        self.bytes += other_metadata.bytes;
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

fn get_keyword_stats_map(extension: &Language) -> HashMap<String,usize> {
    let mut map = HashMap::<String,usize>::new();
    for k in &extension.keywords {
        map.insert(k.descriptive_name.to_owned(), 0);
    }
    map
}
