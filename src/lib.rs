#![allow(warnings)] 
// #![allow(unused_must_use)]

pub mod cmd_arg_parser;
pub mod extension_reader;
pub mod file_parser;
pub mod result_printer;
pub mod putils;

use std::{collections::{HashMap, LinkedList}, fs::{self, File}, io::{self, BufRead, BufReader}, path::Path, time::{Duration}};
use std::{sync::{Arc, Mutex}, thread::JoinHandle};
use std::thread;

use colored::{Colorize,ColoredString};
use num_cpus;
pub use lazy_static::lazy_static;

pub use putils::*;
use domain::{Extension, ExtensionContentInfo, ExtensionMetadata, FileStats, Keyword};
use cmd_arg_parser::ProgramArguments;

pub type LLRef          = Arc<Mutex<LinkedList<String>>>;
pub type VecRef         = Arc<Mutex<Vec<String>>>;
pub type BoolRef        = Arc<Mutex<bool>>;
pub type ContentInfoRef = Arc<Mutex<HashMap<String,ExtensionContentInfo>>>;
pub type ExtMapRef      = Arc<HashMap<String,Extension>>;

pub fn run(args :ProgramArguments, extensions_map :HashMap<String, Extension>) -> Result<(), ParseFilesError> {
    let thread_num = num_cpus::get();

    let files_ref : LLRef = Arc::new(Mutex::new(LinkedList::new()));
    let faulty_files_ref : VecRef  = Arc::new(Mutex::new(Vec::new()));
    let finish_condition_ref : BoolRef = Arc::new(Mutex::new(false));
    let extensions_map_ref : ExtMapRef = Arc::new(extensions_map);
    let mut extensions_content_info_ref = Arc::new(Mutex::new(make_extension_stats(extensions_map_ref.clone())));
    let mut extensions_metadata = make_extension_metadata(extensions_map_ref.clone());
    let mut handles = Vec::new(); 

    println!("\n{}...","Analyzing directory".underline().bold());
    for i in 0..5 {
        handles.push(
            start_consumer_thread(
                i, files_ref.clone(), faulty_files_ref.clone(), finish_condition_ref.clone(), extensions_content_info_ref.clone(), extensions_map_ref.clone())
            .unwrap()
        );
    }

    let (total_files_num, relevant_files) = add_relevant_files(files_ref, &mut extensions_metadata, finish_condition_ref, &args.path, extensions_map_ref, &args.exclude_dirs);
    if relevant_files == 0 {
        return Err(ParseFilesError::NoRelevantFiles);
    }
    println!("{} files found. {} of interest.",with_seperators(total_files_num), with_seperators(relevant_files));

    println!("\n{}...","Parsing files".underline().bold());
    for h in handles {
        h.join().unwrap();
    }
    println!("done.\n\n");

    result_printer::format_and_print_results(&mut extensions_content_info_ref, &mut extensions_metadata);

    Ok(())
}

fn start_consumer_thread
    (id: usize, files_ref: LLRef, faulty_files_ref: VecRef, finish_condition_ref: BoolRef,
     extension_stats_ref: ContentInfoRef, extension_map: ExtMapRef) 
    -> Result<JoinHandle<()>,io::Error> 
{
    thread::Builder::new().name(id.to_string()).spawn(move || {
        let mut buf = String::with_capacity(150);
        let mut files_parsed = 0;
        loop {
            let mut files_guard = files_ref.lock().unwrap();
            // println!("Thread {} , remaining: {}",id,files_guard.len());
            if files_guard.is_empty() {
                if *finish_condition_ref.lock().unwrap() {
                    break;
                } else {
                    drop(files_guard);
                    // println!("Thread {} Seeping...",id);
                    thread::sleep(Duration::from_millis(4));
                    continue;
                }
            }
            files_parsed += 1;
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

            match file_parser::parse_file(&file_path, &file_extension, &mut buf, extension_map.clone()) {
                Ok(x) => {
                    //@TODO: add them to local and after finishing all the jobs add them to global
                    extension_stats_ref.lock().unwrap().get_mut(&file_extension).unwrap().add_stats(x);
                },
                Err(_) => faulty_files_ref.lock().unwrap().push(file_path)
            }
        }
        // println!("Thread {} finished. Parsed {} files.",id,files_parsed);
    })
}

fn add_relevant_files(files_list :LLRef, extensions_metadata_map: &mut HashMap<String,ExtensionMetadata>, finish_condition: BoolRef, dir: &str, extensions: ExtMapRef, exclude_dirs: &Option<Vec<String>>) -> (usize,usize) {
    let path = Path::new(&dir); 
    if path.is_file() {
        if let Some(x) = path.extension() {
            if let Some(y) = x.to_str() {
                if extensions.contains_key(y) {
                    let kilobytes = path.metadata().map_or(0, |m|m.len() / 1000);
                    extensions_metadata_map.get_mut(y).unwrap().add_file_meta(kilobytes);
                    files_list.lock().unwrap().push_front(dir.to_string());
                    *finish_condition.lock().unwrap() = true;
                    return (1,1);
                }
            }
        }
        *finish_condition.lock().unwrap() = true;
        (0,0)
    } else {
        let mut total_files : usize = 0;
        let mut relevant_files : usize = 0;
        add_files_recursively(files_list, extensions_metadata_map, dir, extensions, exclude_dirs, &mut total_files, &mut relevant_files);
        *finish_condition.lock().unwrap() = true;
        (total_files,relevant_files)
    }
} 

fn add_files_recursively(files_list: LLRef, extensions_metadata_map: &mut HashMap<String,ExtensionMetadata>, dir: &str, extensions: ExtMapRef, exclude_dirs: &Option<Vec<String>>, total_files: &mut usize, relevant_files: &mut usize) {
    let dirs = match fs::read_dir(dir) {
        Err(_) => return,
        Ok(x) => x
    };
    
    for entry in dirs {
        let entry = match entry {
            Ok(x) => x,
            Err(_) => continue
        };
    
        let path = entry.path();
        let path_str = match path.to_str() {
            Some(x) => x.to_owned(),
            None => continue
        };
        let path = Path::new(&path);

        if path.is_file() {
            *total_files += 1;
            let extension_name = match utils::get_file_extension(path) {
                Some(x) => x,
                None => continue
            };
            if extensions.contains_key(extension_name) {
                *relevant_files += 1;
                let kilobytes = path.metadata().map_or(0, |m|m.len() / 1000);
                extensions_metadata_map.get_mut(extension_name).unwrap().add_file_meta(kilobytes);

                // println!("Producer trying to push");
                files_list.lock().unwrap().push_front(path_str);
                // println!("Producer just pushed!");
            }
        } else {
            if let Some(x) = exclude_dirs {
                let dir_name = match utils::get_file_name(path) {
                    Some(x) => x.to_owned(),
                    None => continue
                };

                if !x.contains(&dir_name){
                    add_files_recursively(files_list.clone(), extensions_metadata_map, &path_str, extensions.clone(), exclude_dirs, total_files, relevant_files);
                }
            } else {
                add_files_recursively(files_list.clone(), extensions_metadata_map, &path_str, extensions.clone(), exclude_dirs, total_files, relevant_files);
            }
        }
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
        pub kilobytes: u64
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
        
        pub fn add_stats(&mut self, other: FileStats) {
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
        pub fn new(files: usize, kilobytes: u64) ->  ExtensionMetadata {
            ExtensionMetadata {
                files,
                kilobytes
            }
        }

        pub fn add_file_meta(&mut self, kilobytes: u64) {
            self.files += 1;
            self.kilobytes += kilobytes;
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