pub mod cmd_arg_parser;
pub mod my_reader;
pub mod file_scraper;
pub mod utils;
mod conc_test;

use std::{collections::HashMap, fmt::{self, Display}, fs::{self, File}, io::{self, BufRead, BufReader}, path::Path, time::{Duration, SystemTime}};
use std::{sync::{Arc, Mutex}, thread::JoinHandle};
use std::thread;

use colored::Colorize;
use num_cpus;

use domain::{Extension,ExtensionStats, FileStats};
use fmt::{Formatter};
use cmd_arg_parser::ProgramArguments;

pub type VecRef    = Arc<Mutex<Vec<String>>>;
pub type BoolRef   = Arc<Mutex<bool>>;
pub type StatsRef  = Arc<Mutex<HashMap<String,ExtensionStats>>>;
pub type ExtMapRef = Arc<HashMap<String,Extension>>;

pub fn run(args :ProgramArguments, extensions_map :HashMap<String, Extension>) -> Result<(), ParseFilesError> {
    let start = SystemTime::now();
    let thread_num = num_cpus::get();

    let files_ref : VecRef = Arc::new(Mutex::new(Vec::new()));
    let faulty_files_ref : VecRef = Arc::new(Mutex::new(Vec::new()));
    let finish_condition_ref : BoolRef = Arc::new(Mutex::new(false));
    let extensions_map_ref : ExtMapRef = Arc::new(extensions_map);
    let extensions_stats_ref = Arc::new(Mutex::new(make_extension_stats(extensions_map_ref.clone())));
    let mut handles = Vec::new(); 

    println!("\nParsing files...");

    for i in 0..thread_num-5 {
        handles.push(start_consumer_thread(
            i, files_ref.clone(), faulty_files_ref.clone(), finish_condition_ref.clone(), extensions_stats_ref.clone(), extensions_map_ref.clone())
        .unwrap());
    }

    let files_num = add_relevant_files(files_ref, finish_condition_ref, &args.path, extensions_map_ref, &args.exclude_dirs);
    if files_num == 0 {
        return Err(ParseFilesError::NoRelevantFiles);
    }

    for h in handles {
        h.join().unwrap();
    }

    println!("Result: {:?}",extensions_stats_ref);
    println!("Exec time: {:?}",SystemTime::now().duration_since(start).unwrap());

    // println!("Found {} files, {} of interest.\n",files_num, relevant_files.len());


    // match calculate_stats(relevant_files, &extensions_map, &mut extensions_stats) {
    //     Ok(_) => println!("success"),
    //     Err(x) => return Err(x)
    // };

    Ok(())
}

fn start_consumer_thread
    (id: usize, files_ref: VecRef, faulty_files_ref: VecRef, finish_condition_ref: BoolRef,
     extension_stats_ref: StatsRef, extension_map: ExtMapRef) 
    -> Result<JoinHandle<()>,io::Error> 
{
    thread::Builder::new().name(id.to_string()).spawn(move || {
        let mut buf = String::with_capacity(150);
        let mut files_parsed = 0;
        loop {
            let mut files_guard = files_ref.lock().unwrap();
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

            let file_name = files_guard.remove(0);
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
            drop(files_guard);

            match parse_file_t(&file_name, &mut buf, extension_map.clone()) {
                Ok(x) => extension_stats_ref.lock().unwrap().get_mut(&file_extension).unwrap().add_stats(x),
                Err(_) => faulty_files_ref.lock().unwrap().push(file_name)
            }
        }
        println!("Thread {} finished. Parsed {} files.",id,files_parsed);
    })
}

fn parse_file_t(file_name: &String, buf: &mut String, extension_map: ExtMapRef) -> Result<FileStats,ParseFilesError> {
    let extension_str = match Path::new(&file_name).extension() {
        Some(x) => match x.to_str() {
            Some(y) => y,
            None => return Err(ParseFilesError::FaultyFile)
        },
        None => return Err(ParseFilesError::FaultyFile)
    };
    let extension = extension_map.get(extension_str).unwrap();
    let mut file_stats = FileStats::default(&extension.keywords);

    let mut reader = BufReader::new(match File::open(file_name){
        Ok(f) => f,
        Err(_) => return Err(ParseFilesError::FaultyFile)
    });

    loop {
        match reader.read_line(buf) {
            Ok(u) => if u == 0 {return Ok(file_stats)},
            Err(_) => return Err(ParseFilesError::FaultyFile)
        }
        file_stats.incr_lines();
    }
}

pub fn add_relevant_files(files_vec :VecRef, finish_condition: BoolRef, dir: &String, extensions: ExtMapRef, exclude_dirs: &Option<Vec<String>>) -> usize {
    let path = Path::new(&dir); 
    if path.is_file() {
        if let Some(x) = path.extension(){
            if let Some(y) = x.to_str() {
                if extensions.contains_key(y) {
                    files_vec.lock().unwrap().push(dir.clone());
                    *finish_condition.lock().unwrap() = true;
                    return 1;
                }
            }
        }
        *finish_condition.lock().unwrap() = true;
        return 0;
    } else {
        let mut total_files : usize = 0;
        add_files_recursively(files_vec, dir, extensions, exclude_dirs, &mut total_files);
        *finish_condition.lock().unwrap() = true;
        return total_files;
    }
} 

fn add_files_recursively(files_vec: VecRef, dir: &String, extensions: ExtMapRef, exclude_dirs: &Option<Vec<String>>, total_files: &mut usize) {
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
        if Path::new(&path).is_file() {
            *total_files += 1;
            let extension = match path.extension() {
                Some(x) => match x.to_str() {
                    Some(y) => y,
                    None => continue
                },
                None => continue
            };
            if extensions.contains_key(extension) {
                files_vec.lock().unwrap().push(path_str);
            }
        } else {
            if let Some(x) = exclude_dirs {
                if !x.contains(&path_str){
                    add_files_recursively(files_vec.clone(), &path_str, extensions.clone(), exclude_dirs, total_files);
                }
            } else {
                add_files_recursively(files_vec.clone(), &path_str, extensions.clone(), exclude_dirs, total_files);
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
        pub fn default( keywords: &Vec<Keyword>) -> FileStats {
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

        pub fn incr_keyword(&mut self, keyword_name:&String) {
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

    fn get_stats_map(keywords: &Vec<Keyword>) -> HashMap<String,usize> {
        let mut map = HashMap::<String,usize>::new();
        for k in keywords {
            map.insert(k.descriptive_name.to_owned(), 0);
        }
        map
    }
}


// lazy_static! {
//     static ref J_CLASS : Keyword = Keyword {
//         descriptive_name : "classes".to_owned(),
//         aliases : vec!["class".to_owned(),"record".to_owned()]
//     };

//     static ref CLASS : Keyword = Keyword {
//         descriptive_name : "classes".to_owned(),
//         aliases : vec!["class".to_owned()]
//     };

//     static ref INTERFACE : Keyword = Keyword {
//         descriptive_name : "interfaces".to_owned(),
//         aliases : vec!["interface".to_owned()]
//     };

//     static ref JAVA : Extension = Extension {
//         name : "java".to_owned(),
//         string_symbols : vec!["\"".to_owned()],
//         comment_symbol : "//".to_owned(),
//         mutliline_comment_start_symbol : Some("/*".to_owned()),
//         mutliline_comment_end_symbol : Some("*/".to_owned()),
//         keywords : vec![J_CLASS.clone(),INTERFACE.clone()]
//     };

//     static ref PYTHON : Extension = Extension {
//         name : "py".to_owned(),
//         string_symbols : vec!["\"".to_owned(),"'".to_owned()],
//         comment_symbol : "#".to_owned(),
//         mutliline_comment_start_symbol : None,
//         mutliline_comment_end_symbol : None,
//         keywords : vec![CLASS.clone()]
//     };
// }

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