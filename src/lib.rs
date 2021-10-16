#![forbid(unsafe_code)]

#![allow(unused_must_use)]
#![allow(dead_code)]
#![allow(non_snake_case)]

pub mod config_manager;
pub mod io_handler;
pub mod utils;
pub mod consumer;
pub mod producer;
pub mod message_printer;
pub mod file_parser;

mod result_printer;

pub use colored::{Colorize,ColoredString};
pub use config_manager::Configuration;
pub use utils::*;
pub use domain::{Language, LanguageContentInfo, LanguageMetadata, FileStats, Keyword};

pub type FaultyFilesListMut = Arc<Mutex<Vec<FaultyFileDetails>>>;
pub type ContentInfoMapMut  = Arc<Mutex<HashMap<String,LanguageContentInfo>>>;
pub type MetadataMapMut     = Arc<Mutex<HashMap<String,LanguageMetadata>>>;

use lazy_static::lazy_static;
use directories::{BaseDirs,ProjectDirs};
use crossbeam_deque::{Worker,Injector};
use chrono::{DateTime, Local};
use std::{collections::HashMap, fs::{self, File}, io::Read, path::{Path, PathBuf}, sync::atomic::{AtomicBool, Ordering}, time::{Duration, Instant}};
use std::{sync::{Arc, Mutex}, thread::JoinHandle};


pub const APP_NAME : &str = "mezura";
pub const LANGUAGES_DIR_NAME : &str = "languages";
pub const CONFIG_DIR_NAME : &str = "config";
pub const LOGS_DIR_NAME : &str = "logs";
pub const TEST_DIR_NAME : &str = "test_dir";
pub const DEFAULT_CONFIG_NAME : &str = "default.txt";

lazy_static! {
    pub static ref PERSISTENT_APP_PATHS : PersistentAppPaths = PersistentAppPaths::get();
    pub static ref LOCAL_APP_PATHS : LocalAppPaths = LocalAppPaths::get();
    pub static ref CHANGELOG_BYTES : &'static [u8] = include_bytes!("../Changelog");
}


pub fn run(config: Configuration, language_map: HashMap<String, Language>) -> Result<Option<Metrics>, ParseFilesError> {
    let config = Arc::new(config);
    let faulty_files_ref : FaultyFilesListMut  = Arc::new(Mutex::new(Vec::with_capacity(10)));
    let finish_condition_ref = Arc::new(AtomicBool::new(false));
    let language_map_ref = Arc::new(language_map);
    let languages_content_info_ref : ContentInfoMapMut = Arc::new(Mutex::new(make_language_stats(language_map_ref.clone())));
    let global_languages_metadata_map = Arc::new(Mutex::new(make_language_metadata(&language_map_ref)));
    
    let mut files_present = FilesPresent::default();
    let producer_termination_states = Arc::new(Mutex::new(vec![false; config.threads.producers]));
    let files_injector = Arc::new(Injector::<ParsableFile>::new());
    let dirs_injector = Arc::new(Injector::<PathBuf>::new());
    calculate_single_file_stats_or_add_to_injector(&config, &dirs_injector, &files_injector, &mut files_present, 
            &language_map_ref, &global_languages_metadata_map);

    let files_stats = Arc::new(Mutex::new(files_present));

    let mut producer_handles = Vec::with_capacity(config.threads.producers);
    let mut consumer_handles = Vec::with_capacity(config.threads.consumers);

    println!("\n{}...","Analyzing directories".underline().bold());

    let parsing_started_instant = Instant::now();
    for i in 0..config.threads.producers {
        producer_handles.push(producer::start_producer_thread(i, files_injector.clone(), dirs_injector.clone(), Worker::new_fifo(),
            global_languages_metadata_map.clone(), producer_termination_states.clone(),language_map_ref.clone(), config.clone(), files_stats.clone()));
    }
    for i in 0..config.threads.consumers {
        consumer_handles.push(consumer::start_parser_thread(i, files_injector.clone(), faulty_files_ref.clone(), finish_condition_ref.clone(),
        languages_content_info_ref.clone(), language_map_ref.clone(), config.clone()));
    }

    for handle in producer_handles {
        handle.join();
    }

    //If there are a lot of files remaining after producers finish, it makes sense to start another consumer.
    let len = files_injector.len();
    if len > 1200 {
        consumer_handles.push(consumer::start_parser_thread(config.threads.consumers, files_injector, faulty_files_ref.clone(), finish_condition_ref.clone(),
        languages_content_info_ref.clone(), language_map_ref.clone(), config.clone()));
    }

    finish_condition_ref.store(true,Ordering::Relaxed);
    for handle in consumer_handles {
        handle.join();
    }
    let parsing_duration_millis = parsing_started_instant.elapsed().as_millis();

    let file_stats_guard = files_stats.lock().unwrap();
    let (total_files_num, relevant_files_num, excluded_files_num) = 
            (file_stats_guard.total_files, file_stats_guard.relevant_files, file_stats_guard.excluded_files);
    if relevant_files_num == 0 {
        return Err(ParseFilesError::NoRelevantFiles(get_activated_languages_as_str(&config)));
    }
    println!("{} files found. {} of interest. {} excluded.\n",with_seperators(total_files_num), with_seperators(relevant_files_num), with_seperators(excluded_files_num));

    println!("{}...","Parsing files".underline().bold());

    print_faulty_files_or_ok(&faulty_files_ref, &config);
    if faulty_files_ref.lock().unwrap().len() == relevant_files_num {
        return Err(ParseFilesError::AllAreFaultyFiles);
    }

    let mut global_languages_metadata_map_guard = global_languages_metadata_map.lock();
    let mut languages_metadata_map = global_languages_metadata_map_guard.as_deref_mut().unwrap();
    
    remove_faulty_files_stats(&faulty_files_ref, &mut languages_metadata_map, &language_map_ref);

    let mut content_info_map_guard = languages_content_info_ref.lock();
    let mut content_info_map = content_info_map_guard.as_deref_mut().unwrap();

    let metrics = generate_metrics_if_parsing_took_more_than_one_sec(parsing_duration_millis, relevant_files_num, content_info_map);

    let final_stats = FinalStats::calculate(content_info_map, languages_metadata_map);
    let log_file_path = get_specified_config_file_path(&config);
    let existing_log_contents = {
        if let Some(path) = &log_file_path {
            extract_file_contents(path)
        } else {
            None
        }
    };
    let datetime_now = chrono::Local::now();

    remove_languages_with_0_files(content_info_map, languages_metadata_map);
    result_printer::format_and_print_results(&mut content_info_map, &mut languages_metadata_map, &final_stats, 
        &existing_log_contents, &datetime_now, &config);

    if config.log.should_log {
        if let Some(path) = log_file_path {
            io_handler::log_stats(&path, &existing_log_contents, &final_stats, &datetime_now, &config);
        }
    }

    Ok(metrics)
}

//pub for integration tests
pub fn calculate_single_file_stats_or_add_to_injector(config: &Configuration, dirs_injector: &Arc<Injector<PathBuf>>, files_injector: &Arc<Injector<ParsableFile>>,
        files_present: &mut FilesPresent, languages: &Arc<HashMap<String,Language>>, languages_metadata_map: &MetadataMapMut)
{
    config.dirs.iter().for_each(|dir| {
        let dir_path = Path::new(dir);
        if dir_path.is_file() {
            if let Some(x) = dir_path.extension() {
                if let Some(extension) = x.to_str() {
                    if let Some(lang_name) = find_lang_with_this_identifier(languages, extension) {
                        languages_metadata_map.lock().unwrap().get_mut(&lang_name).unwrap().add_file_meta(
                                dir_path.metadata().map_or(0, |m| m.len() as usize));
                        files_injector.push(ParsableFile::new(dir_path.to_path_buf(),lang_name));
                        files_present.total_files += 1;
                        files_present.relevant_files += 1;
                    }
                }
            }
        } else if dir_path.is_dir() {
            dirs_injector.push(dir_path.to_path_buf());
        }
    })
}

//pub for integration tests
pub fn remove_languages_with_0_files(content_info_map: &mut HashMap<String,LanguageContentInfo>,
    languages_metadata_map: &mut HashMap<String, LanguageMetadata>) 
{
   let mut empty_languages = Vec::new();
   for element in languages_metadata_map.iter() {
       if element.1.files == 0 {
           empty_languages.push(element.0.to_owned());
       }
   }

   for ext in empty_languages {
       languages_metadata_map.remove(&ext);
       content_info_map.remove(&ext);
   }
}

pub fn find_lang_with_this_identifier(languages: &Arc<HashMap<String,Language>>, wanted_identifier: &str) -> Option<String> {
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


fn print_faulty_files_or_ok(faulty_files_ref: &FaultyFilesListMut, config: &Configuration) {
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

fn remove_faulty_files_stats(faulty_files_ref: &FaultyFilesListMut, languages_metadata_map: &mut HashMap<String,LanguageMetadata>,
        language_map: &Arc<HashMap<String,Language>>) {
    let faulty_files = &*faulty_files_ref.as_ref().lock().unwrap();
    for file in faulty_files {
        let extension = utils::get_file_extension(Path::new(&file.path));
        if let Some(x) = extension {
            let lang_name = find_lang_with_this_identifier(language_map, x).unwrap();
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

pub fn make_language_stats(languages_map: Arc<HashMap<String,Language>>) -> HashMap<String,LanguageContentInfo> {
    let mut map = HashMap::<String,LanguageContentInfo>::new();
    for (key, value) in languages_map.iter() {
        map.insert(key.to_owned(), LanguageContentInfo::from(value));
    }
    map
}

pub fn make_language_metadata(language_map: &Arc<HashMap<String,Language>>) -> HashMap<String, LanguageMetadata> {
    let mut map = HashMap::<String,LanguageMetadata>::new();
    for (name,_) in language_map.iter() {
        map.insert(name.to_owned(), LanguageMetadata::default());
    }
    map
}

fn get_specified_config_file_path(config: &Configuration) -> Option<String> {
    if let Some(name) = &config.config_name_to_save {
        Some(PERSISTENT_APP_PATHS.logs_dir.clone() + name)
    } else if let Some(name) = &config.config_name_to_load {
        Some(PERSISTENT_APP_PATHS.logs_dir.clone() + name)
    } else {
        None
    }
}

// Used to display colorful errors and warnings, by implementing it on Error enums.
pub trait Formatted {
    fn formatted(&self) -> ColoredString;
} 

#[derive(Debug)]
pub struct PersistentAppPaths {
    pub project_path: String,
    pub data_dir: String,
    pub languages_dir: String,
    pub config_dir: String,
    pub logs_dir: String,
    pub are_initialized: bool
}

#[derive(Debug)]
pub struct LocalAppPaths {
    pub data_dir: String,
    pub languages_dir: String,
    pub config_dir: String,
    pub test_dir: String,
    pub test_config_dir: String,
    pub test_log_dir: String,
}

#[derive(Debug)]
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

#[derive(Debug)]
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

#[derive(Debug,Default,Clone)]
pub struct FilesPresent {
    pub total_files: usize,
    pub relevant_files: usize,
    pub excluded_files: usize
}

#[derive(Debug,Clone)]
pub struct ParsableFile {
    pub path: PathBuf,
    pub language_name: String 
}


impl PersistentAppPaths {
    //Persistent paths: 
    // Windows:  C:/Users/<user_name>/AppData/Roaming/mezura
    // Linux:    /home/<user_name>/.local/share/mezura
    // MacOs:    /Users/<user_name>/Library/Application Support/mezura
    pub fn get() -> Self {
        let mut are_initialized = true;
        let proj_dirs = ProjectDirs::from("", "",  APP_NAME).unwrap();
        let project_path_str = BaseDirs::new().unwrap().data_dir().to_str().unwrap().to_owned() + "/" + APP_NAME;
        let project_path = Path::new(&project_path_str);
        let data_dir = proj_dirs.data_dir().to_str().unwrap().to_owned() + "/";
        if !project_path.exists() {
            are_initialized = false;
            std::fs::create_dir_all(&data_dir).unwrap();
        }
        return PersistentAppPaths {
            project_path: project_path.to_str().unwrap().to_owned(),
            data_dir: data_dir.clone(),
            config_dir: data_dir.clone() + CONFIG_DIR_NAME +"/",
            languages_dir: data_dir.clone() + LANGUAGES_DIR_NAME + "/",
            logs_dir: data_dir + LOGS_DIR_NAME + "/",
            are_initialized
        }
    }
}

impl LocalAppPaths {
    // Paths that exist inside the repository folder
    pub fn get() -> Self {
        let mut working_dir = String::from(std::env::current_exe().expect("Failed to find executable path.")
            .parent().expect("Failed to get parent directory of the executable.").to_str().unwrap());
        if working_dir.contains("target/") || working_dir.contains("target\\"){
            working_dir = String::from(".");
        }
        
        let data_dir =  working_dir + "/data/";

        LocalAppPaths {
            data_dir: data_dir.clone(),
            languages_dir: data_dir.clone() + LANGUAGES_DIR_NAME + "/",
            config_dir: data_dir.clone() + CONFIG_DIR_NAME + "/",
            test_dir: data_dir.clone() + "../" + TEST_DIR_NAME + "/",
            test_config_dir: data_dir.clone() + "../" + TEST_DIR_NAME + "/config/",
            test_log_dir: data_dir + "../" + TEST_DIR_NAME + "/logs/"
        }
    }
}

impl Formatted for ParseFilesError {
    fn formatted(&self) -> ColoredString {
        match self {
            Self::NoRelevantFiles(x) => format!("{} {}","No relevant files found in the given directory.", x).yellow(),
            Self::AllAreFaultyFiles => "None of the files were able to be parsed".yellow()
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
            bytes_size,
            bytes_average_size,
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

impl FilesPresent {
    pub fn new(total_files: usize, relevant_files: usize, excluded_files: usize) -> Self {
        FilesPresent {
            total_files,
            relevant_files,
            excluded_files
        }
    }
}

impl ParsableFile {
    pub fn new(path: PathBuf, language_name: String) -> Self {
        ParsableFile {
            path,
            language_name
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
        pub comment_symbols : Vec<String>,
        pub multiline_comment_start_symbol : Option<String>,
        pub multiline_comment_end_symbol : Option<String>,
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

    #[derive(Debug,PartialEq,Default,Clone)]
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
                keywords 
            }
        }

        pub fn multiline_len(&self) -> usize {
            if let Some(x) = &self.multiline_comment_start_symbol {
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
        pub fn new(lines: usize, code_lines: usize, keyword_occurences: HashMap<String,usize>) -> Self {
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
        pub fn default() -> Self {
            FileStats {
                lines : 0,
                code_lines : 0,
                keyword_occurences : hashmap![]
            }
        }

        pub fn with_keywords(keywords: &[Keyword]) -> Self {
            FileStats {
                lines : 0,
                code_lines : 0,
                keyword_occurences : get_stats_map(keywords)
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