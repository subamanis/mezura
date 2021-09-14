#![forbid(unsafe_code)]

#![allow(unused_must_use)]
#![allow(dead_code)]
#![allow(non_snake_case)]
#![allow(unused_imports)]

pub mod config_manager;
pub mod io_handler;
pub mod utils;

mod file_parser;
mod result_printer;
mod consumer;
mod producer;

pub use colored::{Colorize,ColoredString};
pub use config_manager::Configuration;
pub use utils::*;
pub use domain::{Language, LanguageContentInfo, LanguageMetadata, FileStats, Keyword};

pub type LinkedListRef      = Arc<Mutex<LinkedList<String>>>;
pub type FaultyFilesRef     = Arc<Mutex<Vec<FaultyFileDetails>>>;
pub type BoolRef            = Arc<AtomicBool>;
pub type ContentInfoMapRef  = Arc<Mutex<HashMap<String,LanguageContentInfo>>>;
pub type LanguageMapRef     = Arc<HashMap<String,Language>>;

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, Offset};
use std::{borrow::Borrow, collections::{HashMap, LinkedList}, error::Error, fs::{self, File}, io::{self, BufRead, BufReader, BufWriter, Read, Write}, os::raw, path::{Path, PathBuf}, sync::atomic::{AtomicBool, Ordering}, time::{Duration, Instant}};
use std::{sync::{Arc, Mutex}, thread::JoinHandle};


pub struct Metrics {
    pub files_per_sec: usize,
    pub lines_per_sec: usize
}

#[derive(Debug, PartialEq)]
pub struct FinalStats {
    files: usize,
    lines: usize,
    code_lines: usize,
    extra_lines: usize,
    bytes_size: usize,
    bytes_average_size: usize,
    size: f64,
    size_measurement: String, 
    average_size: f64,
    average_size_measurement: String
}

pub struct FaultyFileDetails {
    path: String,
    error_msg: String,
    size: u64
}

#[derive(Debug)]
pub enum ParseFilesError {
    NoRelevantFiles(String),
    AllAreFaultyFiles
} 

pub fn run(config: Configuration, language_map: HashMap<String, Language>) -> Result<Option<Metrics>, ParseFilesError> {
    let files_ref : LinkedListRef = Arc::new(Mutex::new(LinkedList::new()));
    let faulty_files_ref : FaultyFilesRef  = Arc::new(Mutex::new(Vec::new()));
    let finish_condition_ref : BoolRef = Arc::new(AtomicBool::new(false));
    let language_map_ref : LanguageMapRef = Arc::new(language_map);
    let languages_content_info_ref = Arc::new(Mutex::new(make_language_stats(language_map_ref.clone())));
    let mut languages_metadata = make_language_metadata(language_map_ref.clone());
    let mut handles = Vec::new(); 

    println!("\n{}...","Analyzing directory".underline().bold());
    for i in 0..config.threads {
        handles.push(
            consumer::start_parser_thread(
                i, files_ref.clone(), faulty_files_ref.clone(), finish_condition_ref.clone(),
                languages_content_info_ref.clone(), language_map_ref.clone(), config.clone())
            .unwrap()
        );
    }

    let instant = Instant::now();

    let (total_files_num, relevant_files_num) = producer::add_relevant_files(
            files_ref, &mut languages_metadata, finish_condition_ref, &language_map_ref, &config);
    if relevant_files_num == 0 {
        return Err(ParseFilesError::NoRelevantFiles(get_activated_languages_as_str(&config)));
    }
    println!("{} files found. {} of interest.\n",with_seperators(total_files_num), with_seperators(relevant_files_num));

    println!("{}...","Parsing files".underline().bold());
    for h in handles {
        h.join().unwrap();
    }

    let parsing_duration_millis = instant.elapsed().as_millis();

    print_faulty_files_or_ok(&faulty_files_ref, &config);

    if faulty_files_ref.lock().unwrap().len() == relevant_files_num {
        return Err(ParseFilesError::AllAreFaultyFiles);
    }
    
    remove_faulty_files_stats(&faulty_files_ref, &mut languages_metadata, &language_map_ref);

    let mut content_info_map_guard = languages_content_info_ref.lock();
    let mut content_info_map = content_info_map_guard.as_deref_mut().unwrap();

    let metrics = generate_metrics_if_parsing_took_more_than_one_sec(
            parsing_duration_millis, relevant_files_num, content_info_map);

    let final_stats = FinalStats::calculate(&content_info_map, &languages_metadata);
    let log_file_path = get_specified_config_file_path(&config);
    let existing_log_contents = {
        if let Some(path) = &log_file_path {
            extract_file_contents(path)
        } else {
            None
        }
    };
    let datetime_now = chrono::Local::now();

    result_printer::format_and_print_results(&mut content_info_map, &mut languages_metadata, &final_stats, 
        &existing_log_contents, &datetime_now, &config);

    if config.log {
        if let Some(path) = log_file_path {
            io_handler::log_stats(&path, &existing_log_contents, &final_stats, &datetime_now, &config);
        }
    }

    Ok(metrics)
}

pub fn find_lang_with_this_identifier(languages: &LanguageMapRef, wanted_identifier: &str) -> Option<String> {
    for lang in languages.iter() {
        if lang.1.extensions.iter().any(|x| x == wanted_identifier) {
            return Some(lang.0.to_owned());
        }
    }
    None
}


fn generate_metrics_if_parsing_took_more_than_one_sec(parsing_duration_millis: u128, relevant_files: usize,
        content_info_map: &HashMap<String, LanguageContentInfo>) -> Option<Metrics> 
{
    if parsing_duration_millis <= 1000 {
        return None;
    }

    let duration_secs = parsing_duration_millis as f32/ 1000f32;
    let mut total_lines = 0;
    content_info_map.iter().for_each(|x| total_lines += x.1.lines);
    let lines_per_sec = (total_lines as f32 / duration_secs) as usize;
    let files_per_sec = (relevant_files as f32 / duration_secs) as usize;

    Some(
        Metrics {
            files_per_sec,
            lines_per_sec
        }
    )
}


fn print_faulty_files_or_ok(faulty_files_ref: &FaultyFilesRef, config: &Configuration) {
    let faulty_files = &*faulty_files_ref.as_ref().lock().unwrap();
    if faulty_files.is_empty() {
        println!("{}\n","ok".bright_green());
    } else {
        println!("{} {}",format!("{}",faulty_files.len()).red(), "faulty files detected. They will be ignored in stat calculation.".red());
        if config.should_show_faulty_files {
            for f in faulty_files {
                println!("-- Error: {} \n   for file: {}\n",f.error_msg,f.path);
            }
        } else {
            println!("Run with command '--{}' to get detailed info.",config_manager::SHOW_FAULTY_FILES)
        }
        println!();
    }
}

fn remove_faulty_files_stats(faulty_files_ref: &FaultyFilesRef, languages_metadata_map: &mut HashMap<String,LanguageMetadata>,
        language_map: &LanguageMapRef) {
    let faulty_files = &*faulty_files_ref.as_ref().lock().unwrap();
    for file in faulty_files {
        let extension = utils::get_file_extension(Path::new(&file.path));
        if let Some(x) = extension {
            let lang_name = find_lang_with_this_identifier(&language_map, x).unwrap();
            let language_metadata = languages_metadata_map.get_mut(&lang_name).unwrap();
            language_metadata.files -= 1;
            language_metadata.bytes -= file.size as usize;
        }
    }
}

fn get_activated_languages_as_str(config: &Configuration) -> String {
    if config.languages_of_interest.is_empty() {
        String::new()
    } else {
        String::from("\n(Activated languages: ") + &config.languages_of_interest.join(", ") + ")"
    }
}

fn make_language_stats(languages_map: LanguageMapRef) -> HashMap<String,LanguageContentInfo> {
    let mut map = HashMap::<String,LanguageContentInfo>::new();
    for (key, value) in languages_map.iter() {
        map.insert(key.to_owned(), LanguageContentInfo::from(value));
    }
    map
}

fn make_language_metadata(language_map: LanguageMapRef) -> HashMap<String, LanguageMetadata> {
    let mut map = HashMap::<String,LanguageMetadata>::new();
    for (name,_) in language_map.iter() {
        map.insert(name.to_owned(), LanguageMetadata::default());
    }
    map
}

fn get_specified_config_file_path(config: &Configuration) -> Option<String> {
    if let Some(name) = &config.config_name_to_save {
        Some(io_handler::LOG_DIR.clone() + name)
    } else if let Some(name) = &config.config_name_to_load {
        Some(io_handler::LOG_DIR.clone() + name)
    } else {
        None
    }
}


impl ParseFilesError {
    pub fn formatted(&self) -> String {
        match self {
            Self::NoRelevantFiles(x) => "No relevant files found in the given directory.".yellow().to_string() + x,
            Self::AllAreFaultyFiles => "None of the files were able to be parsed".yellow().to_string()
        }
    }
}

impl FinalStats {
    pub fn new(files: usize, lines: usize, code_lines: usize, bytes_size: usize) -> Self
    {
        let bytes_average_size = bytes_size / files;
        let (size, size_measurement) = FinalStats::get_formatted_size_and_measurement(bytes_size);
        let size = round_1(size);
        let (average_size, average_size_measurement) = Self::get_formatted_size_and_measurement(bytes_average_size);
        let average_size = round_1(average_size);
        FinalStats {
            files,
            lines,
            code_lines,
            extra_lines: lines - code_lines,
            bytes_size,
            bytes_average_size,
            size,
            size_measurement,
            average_size,
            average_size_measurement,
        }
    }

    pub fn new_extended(files: usize, lines: usize, code_lines: usize, extra_lines: usize, bytes_size: usize, bytes_average_size: usize) -> Self {
        let (size, size_measurement) = FinalStats::get_formatted_size_and_measurement(bytes_size);
        let size = round_1(size);
        let (average_size, average_size_measurement) = Self::get_formatted_size_and_measurement(bytes_average_size);
        let average_size = round_1(average_size);

        FinalStats {
            files,
            lines,
            code_lines,
            extra_lines,
            bytes_size,
            bytes_average_size,
            size,
            size_measurement,
            average_size,
            average_size_measurement,
        }
    }

    pub fn calculate(content_info_map: &HashMap<String,LanguageContentInfo>, languages_metadata_map: &HashMap<String,LanguageMetadata>) -> Self {
        let (mut total_files, mut total_lines, mut total_code_lines, mut total_bytes) = (0, 0, 0,0);
        languages_metadata_map.values().for_each(|e| {total_files += e.files; total_bytes += e.bytes});
        content_info_map.values().for_each(|c| {total_lines += c.lines; total_code_lines += c.code_lines});
        let bytes_size = total_bytes;
        let bytes_average_size = total_bytes / total_files;
        let (total_size, size_measurement) = Self::get_formatted_size_and_measurement(total_bytes);
        let (average_size, average_size_measurement) = Self::get_formatted_size_and_measurement(bytes_average_size);
        let total_size = round_1(total_size);
        let average_size = round_1(average_size);


        FinalStats {
            files: total_files,
            lines: total_lines,
            code_lines: total_code_lines,
            extra_lines: total_lines - total_code_lines,
            bytes_size: bytes_size,
            bytes_average_size: bytes_average_size,
            size: total_size,
            size_measurement,
            average_size,
            average_size_measurement
        }
    }

    fn get_formatted_size_and_measurement(value: usize) -> (f64, String) {
        if value >= 1000000 {(value as f64 / 1000000f64, "MBs".to_owned())}
        else if value >= 1000 {(value as f64 / 1000f64, "KBs".to_owned())}
        else {(value as f64, "Bs".to_owned())}
    }
}

impl FaultyFileDetails {
    pub fn new(path: String, error_msg: String, size: u64) -> Self {
        FaultyFileDetails {
            path,
            error_msg,
            size
        }
    }
}


pub mod domain {
    use super::*;
    
    #[derive(Debug,PartialEq, Clone)]
    pub struct Language {
        pub name: String,
        pub extensions : Vec<String>,
        pub string_symbols : Vec<String>,
        pub comment_symbol : String,
        pub mutliline_comment_start_symbol : Option<String>,
        pub mutliline_comment_end_symbol : Option<String>,
        pub keywords : Vec<Keyword>
    }
    
    #[derive(Debug,PartialEq)]
    pub struct Keyword{
        pub descriptive_name : String,
        pub aliases : Vec<String>
    }
    
    #[derive(Debug,PartialEq)]
    pub struct LanguageContentInfo {
        pub lines : usize,
        pub code_lines : usize,
        pub keyword_occurences : HashMap<String,usize>
    }

    #[derive(Debug,PartialEq,Default)]
    pub struct LanguageMetadata {
        pub files: usize,
        pub bytes: usize
    }

    #[derive(Debug,PartialEq)]
    pub struct FileStats {
        pub lines : usize,
        pub code_lines : usize,
        pub keyword_occurences : HashMap<String,usize> 
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
        pub fn multiline_len(&self) -> usize {
            if let Some(x) = &self.mutliline_comment_start_symbol {
                x.len()
            } else {
                0
            }
        }

        pub fn supports_multiline_comments(&self) -> bool {
            self.mutliline_comment_start_symbol.is_some()
        }
    }

    impl LanguageContentInfo {
        pub fn new(lines: usize, code_lines: usize, keyword_occurences: HashMap<String,usize>) -> LanguageContentInfo {
            LanguageContentInfo {
                lines,
                code_lines,
                keyword_occurences
            }
        }

        pub fn dummy(lines: usize) -> LanguageContentInfo {
            LanguageContentInfo {
                lines,
                code_lines: 0,
                keyword_occurences: HashMap::new()
            }
        }
        
        pub fn add_file_stats(&mut self, other: FileStats) {
            self.lines += other.lines;
            self.code_lines += other.code_lines;
            for (k,v) in other.keyword_occurences.iter() {
                *self.keyword_occurences.get_mut(k).unwrap() += *v;
            }
        }
        
        pub fn add_content_info(&mut self, other: &LanguageContentInfo) {
            self.lines += other.lines;
            self.code_lines += other.code_lines;
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
                keyword_occurences : get_keyword_stats_map(ext)
            }
        }
    }

    impl From<FileStats> for LanguageContentInfo {
        fn from(stats: FileStats) -> Self {
            LanguageContentInfo {
                lines : stats.lines,
                code_lines : stats.code_lines,
                keyword_occurences : stats.keyword_occurences
            }
        }
    }

    impl LanguageMetadata {
        pub fn new(files: usize, bytes: usize) ->  LanguageMetadata {
            LanguageMetadata {
                files,
                bytes
            }
        }

        pub fn add_file_meta(&mut self, bytes: usize) {
            self.files += 1;
            self.bytes += bytes;
        }
    }

    impl FileStats {
        pub fn default(keywords: &[Keyword]) -> FileStats {
            FileStats {
                lines : 0,
                code_lines : 0,
                keyword_occurences : get_stats_map(&keywords)
            }
        }

        pub fn incr_lines(&mut self) {
            self.lines += 1;
        }

        pub fn incr_code_lines(&mut self) {
            self.code_lines += 1;
        }

        pub fn incr_keyword(&mut self, keyword_name:&str) {
            *self.keyword_occurences.get_mut(keyword_name).unwrap() += 1;
        }
    }
    
    fn get_keyword_stats_map(extension: &Language) -> HashMap<String,usize> {
        let mut map = HashMap::<String,usize>::new();
        for k in &extension.keywords {
            map.insert(k.descriptive_name.to_owned(), 0);
        }
        map
    }

    fn get_stats_map(keywords: &[Keyword]) -> HashMap<String,usize> {
        let mut map = HashMap::<String,usize>::new();
        for k in keywords {
            map.insert(k.descriptive_name.to_owned(), 0);
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_FinalStats_creation() {
        let content_info_map = hashmap![
            "a".to_owned() => LanguageContentInfo::new(2000, 1400, hashmap![]),
            "b".to_owned() => LanguageContentInfo::new(1000, 800, hashmap![]),
            "c".to_owned() => LanguageContentInfo::new(1000, 800, hashmap![])
        ];
        let languages_metadata_map = hashmap![
            "a".to_owned() => LanguageMetadata::new(20, 100000),
            "b".to_owned() => LanguageMetadata::new(10, 50000),
            "c".to_owned() => LanguageMetadata::new(10, 50000)
        ];
        let f = FinalStats::new(40, 4000, 3000, 200000);
        let ef = FinalStats::new_extended(40, 4000, 3000, 1000, 200000, 5000);
        let cf = FinalStats::calculate(&content_info_map, &languages_metadata_map);
        let customf = FinalStats {
            files: 40,
            lines: 4000,
            code_lines: 3000,
            extra_lines: 1000,
            bytes_size: 200000,
            bytes_average_size: 5000,
            size: 200.0,
            size_measurement: "KBs".to_owned(),
            average_size: 5.0,
            average_size_measurement: "KBs".to_owned()
        };
        assert_eq!(customf, f);
        assert_eq!(customf, ef);
        assert_eq!(customf, cf);


        let content_info_map = hashmap![
            "a".to_owned() => LanguageContentInfo::new(2000, 1400, hashmap![]),
            "b".to_owned() => LanguageContentInfo::new(1000, 800, hashmap![]),
            "c".to_owned() => LanguageContentInfo::new(1000, 800, hashmap![])
        ];
        let languages_metadata_map = hashmap![
            "a".to_owned() => LanguageMetadata::new(25, 1417403),
            "b".to_owned() => LanguageMetadata::new(12, 500000),
            "c".to_owned() => LanguageMetadata::new(12, 500000)
        ];
        let f = FinalStats::new(49, 4000, 3000, 2417403);
        let ef = FinalStats::new_extended(49, 4000, 3000, 1000, 2417403, 49334);
        let cf = FinalStats::calculate(&content_info_map, &languages_metadata_map);
        let customf = FinalStats {
            files: 49,
            lines: 4000,
            code_lines: 3000,
            extra_lines: 1000,
            bytes_size: 2417403,
            bytes_average_size: 49334,
            size: 2.4,
            size_measurement: "MBs".to_owned(),
            average_size: 49.3,
            average_size_measurement: "KBs".to_owned()
        };
        assert_eq!(customf, f);
        assert_eq!(customf, ef);
        assert_eq!(customf, cf);
    }
}