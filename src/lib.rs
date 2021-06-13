#![allow(unused_must_use)]
#![allow(dead_code)]
#![allow(non_snake_case)]
#![allow(unused_imports)]

pub mod config_manager;
pub mod data_reader;
pub mod putils;

mod file_parser;
mod result_printer;
mod consumer;
mod producer;

use std::{collections::{HashMap, LinkedList}, fs::{self, File}, io::{self, BufRead, BufReader}, path::{Path, PathBuf}, sync::atomic::{AtomicBool, Ordering}, time::{Duration}};
use std::{sync::{Arc, Mutex}, thread::JoinHandle};
use std::thread;

pub use colored::{Colorize,ColoredString};
pub use config_manager::Configuration;
pub use putils::*;
pub use domain::{Extension, ExtensionContentInfo, ExtensionMetadata, FileStats, Keyword};

pub type LinkedListRef      = Arc<Mutex<LinkedList<String>>>;
pub type FaultyFilesRef     = Arc<Mutex<Vec<(String,u64)>>>;
pub type BoolRef            = Arc<AtomicBool>;
pub type ContentInfoMapRef  = Arc<Mutex<HashMap<String,ExtensionContentInfo>>>;
pub type ExtensionsMapRef   = Arc<HashMap<String,Extension>>;

pub fn run(config: Configuration, extensions_map: HashMap<String, Extension>) -> Result<(), ParseFilesError> {
    let files_ref : LinkedListRef = Arc::new(Mutex::new(LinkedList::new()));
    let faulty_files_ref : FaultyFilesRef  = Arc::new(Mutex::new(Vec::new()));
    let finish_condition_ref : BoolRef = Arc::new(AtomicBool::new(false));
    let extensions_map_ref : ExtensionsMapRef = Arc::new(extensions_map);
    let mut extensions_content_info_ref = Arc::new(Mutex::new(make_extension_stats(extensions_map_ref.clone())));
    let mut extensions_metadata = make_extension_metadata(extensions_map_ref.clone());
    let mut handles = Vec::new(); 

    println!("\n{}...","Analyzing directory".underline().bold());
    for i in 0..config.threads {
        handles.push(
            consumer::start_parser_thread(
                i, files_ref.clone(), faulty_files_ref.clone(), finish_condition_ref.clone(),
                extensions_content_info_ref.clone(), extensions_map_ref.clone(), config.clone())
            .unwrap()
        );
    }

    let (total_files_num, relevant_files_num) = 
            producer::add_relevant_files(files_ref, &mut extensions_metadata, finish_condition_ref, extensions_map_ref, &config);
    if relevant_files_num == 0 {
        return Err(ParseFilesError::NoRelevantFiles);
    }
    println!("{} files found. {} of interest.\n",with_seperators(total_files_num), with_seperators(relevant_files_num));

    println!("{}...","Parsing files".underline().bold());
    for h in handles {
        h.join().unwrap();
    }
    print_faulty_files_or_ok(&faulty_files_ref, &config);
    
    remove_faulty_files_stats(&faulty_files_ref, &mut extensions_metadata);

    result_printer::format_and_print_results(&mut extensions_content_info_ref, &mut extensions_metadata);
    
    Ok(())
}


fn print_faulty_files_or_ok(faulty_files_ref: &FaultyFilesRef, config: &Configuration) {
    let faulty_files = &*faulty_files_ref.as_ref().lock().unwrap();
    if faulty_files.is_empty() {
        println!("{}\n","ok".bright_green());
    } else {
        println!("{} {}",format!("{}",faulty_files.len()).red(), "faulty files detected. Lines and keywords will not be counted for them.".red());
        if config.should_show_faulty_files {
            for f in faulty_files {
                println!("-- {}",f.0);
            }
        } else {
            println!("Run with command '--{}' to get the paths.",config_manager::SHOW_FAULTY_FILES)
        }
        println!();
    }
}

fn remove_faulty_files_stats(faulty_files_ref: &FaultyFilesRef, extensions_metadata_map: &mut HashMap<String,ExtensionMetadata>) {
    let faulty_files = &*faulty_files_ref.as_ref().lock().unwrap();
    for file in faulty_files {
        let extension = utils::get_file_extension(Path::new(&file.0));
        if let Some(x) = extension {
            let extension_metadata = extensions_metadata_map.get_mut(x).unwrap();
            extension_metadata.files -= 1;
            extension_metadata.bytes -= file.1 as usize;
        }
    }
}

fn make_extension_stats(extensions_map: ExtensionsMapRef) -> HashMap<String,ExtensionContentInfo> {
    let mut map = HashMap::<String,ExtensionContentInfo>::new();
    for (key, value) in extensions_map.iter() {
        map.insert(key.to_owned(), ExtensionContentInfo::from(value));
    }
    map
}

fn make_extension_metadata(extension_map: ExtensionsMapRef) -> HashMap<String, ExtensionMetadata> {
    let mut map = HashMap::<String,ExtensionMetadata>::new();
    for (name,_) in extension_map.iter() {
        map.insert(name.to_owned(), ExtensionMetadata::default());
    }
    map
}

#[derive(Debug)]
pub enum ParseFilesError {
    NoRelevantFiles,
    FaultyFile
} 

impl ParseFilesError {
    pub fn formatted(&self) -> String {
        match self {
            Self::NoRelevantFiles => "\nNo relevant files found in the given directory.".yellow().to_string(),
            Self::FaultyFile => "\nFaulty file".yellow().to_string()
        }
    }
}


pub mod domain {
    use super::*;
    
    #[derive(Debug,PartialEq)]
    pub struct Extension{
        pub name : String,
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
    
    //Used during the file parsing, it needs to be synchronized 
    #[derive(Debug,PartialEq)]
    pub struct ExtensionContentInfo {
        pub lines : usize,
        pub code_lines : usize,
        pub keyword_occurences : HashMap<String,usize>
    }

    //Used in the file searching, doesn't need to be shared between threads.
    #[derive(Debug,PartialEq,Default)]
    pub struct ExtensionMetadata {
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

    impl Extension {
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
    
    impl Clone for Extension {
        fn clone(&self) -> Self {
            Extension {
                name : self.name.to_owned(),
                string_symbols : self.string_symbols.to_owned(),
                comment_symbol : self.comment_symbol.to_owned(),
                mutliline_comment_start_symbol : self.mutliline_comment_start_symbol.to_owned(),
                mutliline_comment_end_symbol : self.mutliline_comment_end_symbol.to_owned(),
                keywords : self.keywords.to_owned()
            }
        }
    }

    impl ExtensionContentInfo {
        pub fn new(lines: usize, code_lines: usize, keyword_occurences: HashMap<String,usize>) -> ExtensionContentInfo {
            ExtensionContentInfo {
                lines,
                code_lines,
                keyword_occurences
            }
        }

        pub fn dummy(lines: usize) -> ExtensionContentInfo {
            ExtensionContentInfo {
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
        
        pub fn add_content_info(&mut self, other: &ExtensionContentInfo) {
            self.lines += other.lines;
            self.code_lines += other.code_lines;
            for (k,v) in other.keyword_occurences.iter() {
                *self.keyword_occurences.get_mut(k).unwrap() += *v;
            }
        }
    }

    impl From<&Extension> for ExtensionContentInfo {
        fn from(ext: &Extension) -> Self {
            ExtensionContentInfo {
                lines : 0,
                code_lines : 0,
                keyword_occurences : get_keyword_stats_map(ext)
            }
        }
    }

    impl From<FileStats> for ExtensionContentInfo {
        fn from(stats: FileStats) -> Self {
            ExtensionContentInfo {
                lines : stats.lines,
                code_lines : stats.code_lines,
                keyword_occurences : stats.keyword_occurences
            }
        }
    }

    impl ExtensionMetadata {
        pub fn new(files: usize, bytes: usize) ->  ExtensionMetadata {
            ExtensionMetadata {
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
    
    fn get_keyword_stats_map(extension: &Extension) -> HashMap<String,usize> {
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