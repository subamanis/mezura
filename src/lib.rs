#![allow(unused_must_use)]
#![allow(dead_code)]
#![allow(non_snake_case)]
#![allow(unused_imports)]

pub mod config_manager;
pub mod data_reader;
pub mod file_parser;
pub mod result_printer;
pub mod putils;
pub mod test;

use std::{collections::{HashMap, LinkedList}, fs::{self, File}, io::{self, BufRead, BufReader}, path::{Path, PathBuf}, sync::atomic::{AtomicBool, Ordering}, time::{Duration}};
use std::{sync::{Arc, Mutex}, thread::JoinHandle};
use std::thread;

use colored::{Colorize,ColoredString};

pub use config_manager::Configuration;
pub use putils::*;
use domain::{Extension, ExtensionContentInfo, ExtensionMetadata, FileStats, Keyword};

pub type LinkedListRef  = Arc<Mutex<LinkedList<String>>>;
pub type VecRef         = Arc<Mutex<Vec<String>>>;
pub type BoolRef        = Arc<AtomicBool>;
pub type ContentInfoRef = Arc<Mutex<HashMap<String,ExtensionContentInfo>>>;
pub type ExtMapRef      = Arc<HashMap<String,Extension>>;

pub fn run(config: Configuration, extensions_map: HashMap<String, Extension>) -> Result<(), ParseFilesError> {
    let files_ref : LinkedListRef = Arc::new(Mutex::new(LinkedList::new()));
    let faulty_files_ref : VecRef  = Arc::new(Mutex::new(Vec::new()));
    let finish_condition_ref : BoolRef = Arc::new(AtomicBool::new(false));
    let extensions_map_ref : ExtMapRef = Arc::new(extensions_map);
    let mut extensions_content_info_ref = Arc::new(Mutex::new(make_extension_stats(extensions_map_ref.clone())));
    let mut extensions_metadata = make_extension_metadata(extensions_map_ref.clone());
    let mut handles = Vec::new(); 

    println!("\n{}...","Analyzing directory".underline().bold());
    for i in 0..config.threads {
        handles.push(
            start_consumer_thread(
                i, files_ref.clone(), faulty_files_ref.clone(), finish_condition_ref.clone(),
                extensions_content_info_ref.clone(), extensions_map_ref.clone(), config.clone())
            .unwrap()
        );
    }

    let (total_files_num, relevant_files) = add_relevant_files(files_ref, &mut extensions_metadata, finish_condition_ref, extensions_map_ref, &config);
    if relevant_files == 0 {
        return Err(ParseFilesError::NoRelevantFiles);
    }
    println!("{} files found. {} of interest.\n",with_seperators(total_files_num), with_seperators(relevant_files));

    println!("{}...","Parsing files".underline().bold());
    for h in handles {
        h.join().unwrap();
    }
    print_faulty_files_or_ok(faulty_files_ref, &config);

    result_printer::format_and_print_results(&mut extensions_content_info_ref, &mut extensions_metadata);
    
    Ok(())
}

fn start_consumer_thread
    (id: usize, files_ref: LinkedListRef, faulty_files_ref: VecRef, finish_condition_ref: BoolRef,
     extension_content_info_ref: ContentInfoRef, extension_map: ExtMapRef, config: Configuration) 
    -> Result<JoinHandle<()>,io::Error> 
{
    thread::Builder::new().name(id.to_string()).spawn(move || {
        let mut buf = String::with_capacity(150);
        loop {
            let mut files_guard = files_ref.lock().unwrap();
            // println!("Thread {} , remaining: {}",id,files_guard.len());
            if files_guard.is_empty() {
                if finish_condition_ref.load(Ordering::Relaxed) {
                    break;
                } else {
                    drop(files_guard);
                    //waiting for the list with the paths to be filled until trying again to pop a path.
                    thread::sleep(Duration::from_millis(3));
                    continue;
                }
            }
            let file_path = files_guard.pop_front().unwrap();
            drop(files_guard);

            let file_extension = match Path::new(&file_path).extension() {
                Some(x) => match x.to_str() {
                    Some(y) => y.to_owned(),
                    None => {
                        faulty_files_ref.lock().unwrap().push(file_path); 
                        continue;
                    }
                },
                None => {
                    faulty_files_ref.lock().unwrap().push(file_path);
                    continue;
                }
            };

            match file_parser::parse_file(&file_path, &file_extension, &mut buf, extension_map.clone(), &config) {
                Ok(x) => {
                    extension_content_info_ref.lock().unwrap().get_mut(&file_extension).unwrap().add_file_stats(x);
                },
                Err(_) => faulty_files_ref.lock().unwrap().push(file_path)
            }
        }
    })
}

fn add_relevant_files(files_list :LinkedListRef, extensions_metadata_map: &mut HashMap<String,ExtensionMetadata>, finish_condition: BoolRef, 
     extensions: ExtMapRef, config: &Configuration) -> (usize,usize) 
{
    let path = Path::new(&config.path); 
    if path.is_file() {
        if let Some(x) = path.extension() {
            if let Some(y) = x.to_str() {
                if extensions.contains_key(y) {
                    extensions_metadata_map.get_mut(y).unwrap().add_file_meta(path.metadata().map_or(0, |m| m.len() as usize));
                    files_list.lock().unwrap().push_front(config.path.to_string());
                    finish_condition.store(true, Ordering::Relaxed);
                    return (1,1);
                }
            }
        }
        finish_condition.store(true, Ordering::Relaxed);
        (0,0)
    } else {
        let (total_files, relevant_files) = search_dir_and_add_files_to_list(&files_list, extensions_metadata_map, &extensions, config);
        finish_condition.store(true, Ordering::Relaxed);
        (total_files,relevant_files)
    }
} 

fn search_dir_and_add_files_to_list(files_list: &LinkedListRef, extensions_metadata_map: &mut HashMap<String,ExtensionMetadata>,
    extensions: &ExtMapRef, config: &Configuration) -> (usize,usize) 
{
    let mut total_files = 0;
    let mut relevant_files = 0;
    let mut dirs: LinkedList<PathBuf> = LinkedList::new();
    dirs.push_front(Path::new(&config.path).to_path_buf());
    while let Some(dir) = dirs.pop_front() {
        if let Ok(entries) = fs::read_dir(&dir) {
            for e in entries.flatten(){
                if let Ok(ft) = e.file_type() {
                    if ft.is_file() { 
                        total_files += 1;
                        let path_buf = e.path();
                        let extension_name = match path_buf.extension() {
                            Some(x) => {
                                match x.to_str() {
                                        Some(x) => x.to_owned(),
                                        None => continue
                                    }
                                },
                                None => continue
                            };
                        if extensions.contains_key(&extension_name) {
                            relevant_files += 1;
                            let bytes = match path_buf.metadata() {
                                Ok(x) => x.len() as usize,
                                Err(_) => 0
                            };
                            extensions_metadata_map.get_mut(&extension_name).unwrap().add_file_meta(bytes);
                            
                            let str_path = match path_buf.to_str() {
                                Some(y) => y.to_owned(),
                                None => continue
                            };
                            files_list.lock().unwrap().push_front(str_path);
                        }
                    } else { //is directory
                        let dir_name = match e.file_name().to_str() {
                            Some(x) => {
                                if !config.should_search_in_dotted && x.starts_with('.') {continue;}
                                else {x.to_owned()}
                            },
                            None => continue
                        };
                
                        if !config.exclude_dirs.is_empty() {
                            if !config.exclude_dirs.contains(&dir_name){
                                dirs.push_front(e.path());
                            }
                        } else {
                            dirs.push_front(e.path());
                        }
                    }
                }
            }
        }
    }
    (total_files,relevant_files)
}

fn print_faulty_files_or_ok(faulty_files_ref: VecRef, config: &Configuration) {
    let faulty_files = &mut *faulty_files_ref.as_ref().lock().unwrap();
    if faulty_files.is_empty() {
        println!("{}\n","ok".bright_green());
    } else {
        println!("{} {}",format!("{}",faulty_files.len()).red(), "faulty files detected. Lines and keywords will not be counted for them.".red());
        if config.should_show_faulty_files {
            for f in faulty_files {
                println!("-- {}",f);
            }
        } else {
            println!("Run with command '--show-faulty-files' to get the paths.")
        }
        println!();
    }
    
}

pub fn make_extension_stats(extensions_map: ExtMapRef) -> HashMap<String,ExtensionContentInfo> {
    let mut map = HashMap::<String,ExtensionContentInfo>::new();
    for (key, value) in extensions_map.iter() {
        map.insert(key.to_owned(), ExtensionContentInfo::from(value));
    }
    map
}

pub fn make_extension_metadata(extension_map: ExtMapRef) -> HashMap<String, ExtensionMetadata> {
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