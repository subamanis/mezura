#![allow(warnings)] 
// #![allow(unused_must_use)]

pub mod cmd_arg_parser;
pub mod extension_reader;
pub mod file_parser;
pub mod putils;

use std::{collections::{HashMap, LinkedList}, fmt::{self, Display}, fs::{self, File}, io::{self, BufRead, BufReader}, path::Path, time::{Duration, SystemTime}};
use std::{sync::{Arc, Mutex}, thread::JoinHandle};
use std::thread;

use colored::Colorize;
use num_cpus;
pub use lazy_static::lazy_static;

pub use putils::*;
use domain::{Extension,Keyword,ExtensionStats, FileStats};
use fmt::{Formatter};
use cmd_arg_parser::ProgramArguments;

pub type LLRef     = Arc<Mutex<LinkedList<String>>>;
pub type VecRef    = Arc<Mutex<Vec<String>>>;
pub type BoolRef   = Arc<Mutex<bool>>;
pub type StatsRef  = Arc<Mutex<HashMap<String,ExtensionStats>>>;
pub type ExtMapRef = Arc<HashMap<String,Extension>>;

pub fn run(args :ProgramArguments, extensions_map :HashMap<String, Extension>) -> Result<(), ParseFilesError> {
    let start = SystemTime::now();
    let thread_num = num_cpus::get();

    let files_ref : LLRef = Arc::new(Mutex::new(LinkedList::new()));
    let faulty_files_ref : VecRef  = Arc::new(Mutex::new(Vec::new()));
    let finish_condition_ref : BoolRef = Arc::new(Mutex::new(false));
    let extensions_map_ref : ExtMapRef = Arc::new(extensions_map);
    let extensions_stats_ref = Arc::new(Mutex::new(make_extension_stats(extensions_map_ref.clone())));
    let mut handles = Vec::new(); 

    println!("\nParsing files...");

    for i in 0..thread_num-4 {
        handles.push(start_consumer_thread(
            i, files_ref.clone(), faulty_files_ref.clone(), finish_condition_ref.clone(), extensions_stats_ref.clone(), extensions_map_ref.clone())
        .unwrap());
    }

    let (total_files_num, relevant_files) = add_relevant_files(files_ref, finish_condition_ref, &args.path, extensions_map_ref, &args.exclude_dirs);
    if relevant_files == 0 {
        return Err(ParseFilesError::NoRelevantFiles);
    }
    println!("{} files found. {} of interest.",total_files_num, relevant_files);

    for h in handles {
        h.join().unwrap();
    }

    println!("Result: {:?}",extensions_stats_ref);
    println!("Exec time: {:?}",SystemTime::now().duration_since(start).unwrap());

    Ok(())
}

fn start_consumer_thread
    (id: usize, files_ref: LLRef, faulty_files_ref: VecRef, finish_condition_ref: BoolRef,
     extension_stats_ref: StatsRef, extension_map: ExtMapRef) 
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
                    thread::sleep(Duration::from_millis(4));
                    continue;
                }
            }
            files_parsed += 1;

            let file_name = files_guard.pop_front().unwrap();
            drop(files_guard);
            let file_extension = match Path::new(&file_name).extension() {
                Some(x) => match x.to_str() {
                    Some(y) => y.to_owned(),
                    None => {
                        faulty_files_ref.lock().unwrap().push(file_name);
                        continue;
                    }
                },
                None => {
                    faulty_files_ref.lock().unwrap().push(file_name);
                    continue;
                }
            };

            match file_parser::parse_file(&file_name, &mut buf, extension_map.clone()) {
                Ok(x) => extension_stats_ref.lock().unwrap().get_mut(&file_extension).unwrap().add_stats(x),
                Err(_) => faulty_files_ref.lock().unwrap().push(file_name)
            }
        }
        println!("Thread {} finished. Parsed {} files.",id,files_parsed);
    })
}

pub fn add_relevant_files(files_list :LLRef, finish_condition: BoolRef, dir: &str, extensions: ExtMapRef, exclude_dirs: &Option<Vec<String>>) -> (usize,usize) {
    let path = Path::new(&dir); 
    if path.is_file() {
        if let Some(x) = path.extension() {
            if let Some(y) = x.to_str() {
                if extensions.contains_key(y) {
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
        add_files_recursively(files_list, dir, extensions, exclude_dirs, &mut total_files, &mut relevant_files);
        *finish_condition.lock().unwrap() = true;
        (total_files,relevant_files)
    }
} 

fn add_files_recursively(files_list: LLRef, dir: &str, extensions: ExtMapRef, exclude_dirs: &Option<Vec<String>>, total_files: &mut usize, relevant_files: &mut usize) {
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
            let extension = match utils::get_file_extension(path) {
                Some(x) => x,
                None => continue
            };
            if extensions.contains_key(extension) {
                *relevant_files += 1;
                files_list.lock().unwrap().push_front(path_str);
            }
        } else {
            if let Some(x) = exclude_dirs {
                let dir_name = match utils::get_file_name(path) {
                    Some(x) => x.to_owned(),
                    None => continue
                };

                if !x.contains(&dir_name){
                    add_files_recursively(files_list.clone(), &path_str, extensions.clone(), exclude_dirs, total_files, relevant_files);
                }
            } else {
                add_files_recursively(files_list.clone(), &path_str, extensions.clone(), exclude_dirs, total_files, relevant_files);
            }
        }
    }
}

fn make_extension_stats(extensions_map: ExtMapRef) -> HashMap<String,ExtensionStats> {
    let mut map = HashMap::<String,ExtensionStats>::new();
    for (key, value) in extensions_map.iter() {
        map.insert(key.to_owned(), ExtensionStats::new(value));
    }

    map
}

pub enum ParseFilesError {
    NoRelevantFiles,
    PlaceholderError,
    FaultyFile
} 

impl ParseFilesError {
    pub fn formatted(&self) -> String {
        match self {
            Self::NoRelevantFiles => "\nNo relevant files found in the given directory.".yellow().to_string(),
            Self::PlaceholderError => "\nPlaceholder error".yellow().to_string(),
            Self::FaultyFile => "\nFaulty file".yellow().to_string()
        }
    }
}


pub mod domain {
    use std::usize;

    use super::*;
    
    #[derive(Debug)]
    pub struct Extension{
        pub name : String,
        pub string_symbols : Vec<String>,
        pub comment_symbol : String,
        pub mutliline_comment_start_symbol : Option<String>,
        pub mutliline_comment_end_symbol : Option<String>,
        pub keywords : Vec<Keyword>
    }
    
    #[derive(Debug)]
    pub struct Keyword{
        pub descriptive_name : String,
        pub aliases : Vec<String>
    }
    
    #[derive(Debug)]
    pub struct ExtensionStats {
        pub extension_name : String,
        pub files : usize,
        pub lines : usize,
        pub code_lines : usize,
        pub keyword_occurences : HashMap<String,usize>
    }

    #[derive(Debug)]
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

    impl PartialEq for Keyword {
        fn eq(&self, other: &Self) -> bool {
            self.descriptive_name == other.descriptive_name &&
            self.aliases == other.aliases
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
    
    impl PartialEq for Extension { 
        fn eq(&self, other: &Self) -> bool {
            self.name == other.name &&
            self.string_symbols == other.string_symbols &&
            self.comment_symbol == other.comment_symbol &&
            self.mutliline_comment_start_symbol == other.mutliline_comment_start_symbol &&
            self.mutliline_comment_end_symbol == other.mutliline_comment_end_symbol &&
            self.keywords == other.keywords
        }
    }

    impl Display for ExtensionStats {
        fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
            write!(f, "\t\t-{} ({} files): {} , {}",self.extension_name, self.files, self.lines, self.code_lines)
        }
    }
    
    impl PartialEq for ExtensionStats {
        fn eq(&self, other: &Self) -> bool {
            self.extension_name == other.extension_name &&
            self.files == other.files &&
            self.code_lines == other.code_lines &&
            self.keyword_occurences == other.keyword_occurences
        }
    }
    
    impl ExtensionStats {
        pub fn new(extension: &Extension) -> ExtensionStats {
            ExtensionStats {
                extension_name : extension.name.to_owned(),
                files : 0,
                lines : 0,
                code_lines : 0,
                keyword_occurences : get_keyword_stats_map(extension)
            }
        }

        pub fn add_stats(&mut self, other: FileStats) {
            self.files += 0;
            self.lines += other.lines;
            self.code_lines += other.code_lines;
            self.keyword_occurences.extend(other.keyword_occurences);
        }
    }

    impl FileStats {
        pub fn default( keywords: &[Keyword]) -> FileStats {
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


lazy_static! {
    pub static ref J_CLASS : Keyword = Keyword {
        descriptive_name : "classes".to_owned(),
        aliases : vec!["class".to_owned(),"record".to_owned()]
    };

    static ref CLASS : Keyword = Keyword {
        descriptive_name : "classes".to_owned(),
        aliases : vec!["class".to_owned()]
    };

    static ref INTERFACE : Keyword = Keyword {
        descriptive_name : "interfaces".to_owned(),
        aliases : vec!["interface".to_owned()]
    };

    static ref JAVA : Extension = Extension {
        name : "java".to_owned(),
        string_symbols : vec!["\"".to_owned()],
        comment_symbol : "//".to_owned(),
        mutliline_comment_start_symbol : Some("/*".to_owned()),
        mutliline_comment_end_symbol : Some("*/".to_owned()),
        keywords : vec![J_CLASS.clone(),INTERFACE.clone()]
    };

    static ref PYTHON : Extension = Extension {
        name : "py".to_owned(),
        string_symbols : vec!["\"".to_owned(),"'".to_owned()],
        comment_symbol : "#".to_owned(),
        mutliline_comment_start_symbol : None,
        mutliline_comment_end_symbol : None,
        keywords : vec![CLASS.clone()]
    };
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn extensions_workflow() -> Result<(), String>{
//         let extensions_map = match file_scraper::parse_supported_extensions_to_map() {
//             Ok(it) => it.0,
//             _ => return Err("Cannot parse extensions to map".to_string())
//         };
//         let mut extensions_map_custom = HashMap::new();
//         extensions_map_custom.insert("java".to_owned(), JAVA.clone());
//         extensions_map_custom.insert("py".to_owned(), PYTHON.clone());
//         assert_eq!(extensions_map[&"java".to_owned()] , extensions_map_custom[&"java".to_owned()]);
//         assert_eq!(extensions_map[&"py".to_owned()] , extensions_map_custom[&"py".to_owned()]);
        
//         //----- Getting relevant files from test directory test
//         let relevant_files_result = file_scraper::get_relevant_files(&"test_dir".to_owned(), &extensions_map, &Some(vec!["dirb".to_owned()]));
//         //@TODO: fix this
//         assert_eq!((3,vec!["test_dir\\a.java".to_owned(),"test_dir\\b.py".to_owned(),"test_dir\\c.py".to_owned()]), relevant_files_result);
        
//         //----- Extensions stats testing
//         let mut java_keyword_occur_map  = HashMap::new();
//         java_keyword_occur_map.insert("classes".to_owned(), 0);
//         java_keyword_occur_map.insert("interfaces".to_owned(), 0);
//         let mut python_keyword_occur_map  = HashMap::new();
//         python_keyword_occur_map.insert("classes".to_owned(), 0);
        
//         let java_extension_stats = ExtensionStats::new(&*JAVA);
//         let python_extension_stats = ExtensionStats::new(&*PYTHON);
//         let java_extension_stats_custom = ExtensionStats {
//             extension_name : "java".to_owned(),
//             files : 0,
//             lines : 0,
//             code_lines : 0,
//             keyword_occurences : java_keyword_occur_map
//         }; 
//         let python_extension_stats_custom = ExtensionStats {
//             extension_name : "py".to_owned(),
//             files : 0,
//             lines : 0,
//             code_lines : 0,
//             keyword_occurences : python_keyword_occur_map
//         }; 
//         assert_eq!(java_extension_stats,java_extension_stats_custom);
//         assert_eq!(python_extension_stats,python_extension_stats_custom);
        
//         let extension_stats_map = make_extension_stats(&extensions_map);
//         let mut extension_stats_map_custom = HashMap::new();
//         extension_stats_map_custom.insert("java".to_owned(), java_extension_stats_custom);
//         extension_stats_map_custom.insert("py".to_owned(), python_extension_stats_custom);
//         //@TODO: add other extensions
//         //assert_eq!(extension_stats_map, extension_stats_map_custom);

//         Ok(())
//     }
// }