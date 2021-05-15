pub mod cmd_arg_parser;
pub mod my_reader;
pub mod file_scraper;
pub mod utils;
mod conc_test;

use std::{cmp::{max}, collections::HashMap, fmt::{self, Display}, fs::File, io::{self, BufRead, BufReader}, path::Path, time::{Duration, SystemTime}};

use domain::FileStats;
use lazy_static::lazy_static;
use colored::Colorize;
use num_cpus;

pub use domain::{Extension,Keyword,ExtensionStats};
pub use fmt::{Formatter};
use cmd_arg_parser::ProgramArguments;
 
pub fn run(args :ProgramArguments, extensions_map :HashMap<String, Extension>) -> Result<(), ParseFilesError> {
    let thread_num = num_cpus::get();
    // let start = SystemTime::now();
    println!("\nSearching for files...");
    let relevant_files_result = file_scraper::get_relevant_files(&args.path, &extensions_map, &args.exclude_dirs);
    if relevant_files_result.0 == 0 {
        return Err(ParseFilesError::NoRelevantFiles);
    }
    let relevant_files = relevant_files_result.1;
    // println!("{:?}",SystemTime::now().duration_since(start));

    println!("Found {} files, {} of interest.\n",relevant_files_result.0, relevant_files.len());

    let mut extensions_stats = make_extension_stats(&extensions_map);

    match calculate_stats(relevant_files, &extensions_map, &mut extensions_stats) {
        Ok(_) => println!("success"),
        Err(x) => return Err(x)
    };

    Ok(())
}

fn calculate_stats(relevant_files: Vec<String>, extensions_map :&HashMap<String, Extension>, extensions_stats: &mut HashMap<String,ExtensionStats>) -> Result<(), ParseFilesError> {
    let mut faulty_files :Vec<String> = Vec::new();
    let print_interval = max(relevant_files.len() / 400, 1);
    let mut buf = String::with_capacity(200);
    let mut file_counter = 0;
    let files_count = relevant_files.len();

    println!("Parsing files...");
    for file in relevant_files {
        file_counter += 1;
        let extension_str = match Path::new(&file).extension() {
            Some(x) => match x.to_str() {
                Some(y) => y,
                None => continue
            },
            None => continue
        };
        let extension = match extensions_map.get(extension_str){
            Some(x) => x,
            None => continue
        };
        match parse_file(&file, &mut buf, extension) {
            Ok(x) => extensions_stats.get_mut(extension_str).unwrap().append(x),
            Err(_) => faulty_files.push(file)
        }

        if file_counter % 400 == 0 {
            println!("\t     ... ({}/{}) done",file_counter, files_count);
        }
    }

    if file_counter % 400 != 0 {
        println!("\t     ... ({}/{}) done\n",file_counter, files_count);
    }

    Err(ParseFilesError::PlaceholderError)
}

fn parse_file(file: &String, buf: &mut String, extension: &Extension) -> io::Result<FileStats> {
    let mut file_stats = FileStats::default(&extension.keywords);
    let mut reader = BufReader::new(File::open(file)?);
    while reader.read_line(buf)? != 0 {
        file_stats.incr_lines();

        
    }

    Ok(file_stats)
}

fn make_extension_stats(extensions_map: &HashMap<String, Extension>) -> HashMap<String,ExtensionStats> {
    let mut map = HashMap::<String,ExtensionStats>::new();
    for (key, value) in extensions_map.iter() {
        map.insert(key.to_owned(), ExtensionStats::new(value));
    }

    map
}

pub enum ParseFilesError {
    NoRelevantFiles,
    PlaceholderError
} 

impl ParseFilesError {
    pub fn formatted(&self) -> String {
        match self {
            Self::NoRelevantFiles => "\nNo relevant files found in the given directory.".yellow().to_string(),
            Self::PlaceholderError => "\nPlaceholder error".yellow().to_string()
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

        pub fn append(&mut self, other: FileStats) {
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


lazy_static! {
    static ref J_CLASS : Keyword = Keyword {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_workflow() -> Result<(), String>{
        let extensions_map = match file_scraper::parse_supported_extensions_to_map() {
            Ok(it) => it.0,
            _ => return Err("Cannot parse extensions to map".to_string())
        };
        let mut extensions_map_custom = HashMap::new();
        extensions_map_custom.insert("java".to_owned(), JAVA.clone());
        extensions_map_custom.insert("py".to_owned(), PYTHON.clone());
        assert_eq!(extensions_map[&"java".to_owned()] , extensions_map_custom[&"java".to_owned()]);
        assert_eq!(extensions_map[&"py".to_owned()] , extensions_map_custom[&"py".to_owned()]);
        
        //----- Getting relevant files from test directory test
        let relevant_files_result = file_scraper::get_relevant_files(&"test_dir".to_owned(), &extensions_map, &Some(vec!["dirb".to_owned()]));
        //@TODO: fix this
        assert_eq!((3,vec!["test_dir\\a.java".to_owned(),"test_dir\\b.py".to_owned(),"test_dir\\c.py".to_owned()]), relevant_files_result);
        
        //----- Extensions stats testing
        let mut java_keyword_occur_map  = HashMap::new();
        java_keyword_occur_map.insert("classes".to_owned(), 0);
        java_keyword_occur_map.insert("interfaces".to_owned(), 0);
        let mut python_keyword_occur_map  = HashMap::new();
        python_keyword_occur_map.insert("classes".to_owned(), 0);
        
        let java_extension_stats = ExtensionStats::new(&*JAVA);
        let python_extension_stats = ExtensionStats::new(&*PYTHON);
        let java_extension_stats_custom = ExtensionStats {
            extension_name : "java".to_owned(),
            files : 0,
            lines : 0,
            code_lines : 0,
            keyword_occurences : java_keyword_occur_map
        }; 
        let python_extension_stats_custom = ExtensionStats {
            extension_name : "py".to_owned(),
            files : 0,
            lines : 0,
            code_lines : 0,
            keyword_occurences : python_keyword_occur_map
        }; 
        assert_eq!(java_extension_stats,java_extension_stats_custom);
        assert_eq!(python_extension_stats,python_extension_stats_custom);
        
        let extension_stats_map = make_extension_stats(&extensions_map);
        let mut extension_stats_map_custom = HashMap::new();
        extension_stats_map_custom.insert("java".to_owned(), java_extension_stats_custom);
        extension_stats_map_custom.insert("py".to_owned(), python_extension_stats_custom);
        //@TODO: add other extensions
        //assert_eq!(extension_stats_map, extension_stats_map_custom);

        Ok(())
    }
}