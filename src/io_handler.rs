use std::{{ffi::OsString, path::Path}, cmp::max, collections::{HashMap as HashMap}, env, fs::{self, File, ReadDir}, io::{BufRead, BufReader, BufWriter, Write}, path::PathBuf};

use colored::*;
use lazy_static::lazy_static;

use crate::{Configuration, config_manager::{self, BRACES_AS_CODE, EXCLUDE, LANGUAGES, PATH, SEARCH_IN_DOTTED, SHOW_FAULTY_FILES, THREADS}, domain::*};

lazy_static! {
    pub static ref DATA_DIR : Option<String> = try_find_data_dir();
}

const DEFAULT_CONFIG_FILE_NAME : &str = "default";
const CONFIG_DIR_NAME : &str = "/config";
const LANGUAGE_DIR_NAME : &str = "/languages";

#[derive(Debug)]
pub enum ParseLanguageError {
    NoFilesFound,
    NoFilesFormattedProperly,
    LanguagesOfInterestNotFound
}

#[derive(Debug)]
pub enum ParseConfigFileError {
    DirNotFound,
    FileNotFound(String),
    IOError
}

#[derive(Debug)]
pub struct PersistentOptions {
    pub path                     : Option<String>,
    pub exclude_dirs             : Option<Vec<String>>,
    pub languages_of_interest    : Option<Vec<String>>,
    pub threads                  : Option<usize>,
    pub braces_as_code           : Option<bool>,
    pub should_search_in_dotted  : Option<bool>,
    pub should_show_faulty_files : Option<bool>,
    pub no_visual                : Option<bool>
}


pub fn parse_supported_languages_to_map(languages_of_interest: &mut Vec<String>)
        -> Result<(HashMap<String,Language>, Vec<String>, Vec<String>), ParseLanguageError> 
{
    let dirs = fs::read_dir(DATA_DIR.clone().unwrap() + LANGUAGE_DIR_NAME).unwrap();
    
    let mut languages_of_interest_appearance = HashMap::<String,bool>::new();
    for lang_name in languages_of_interest.iter() {
        languages_of_interest_appearance.insert(lang_name.to_owned(), false);
    }

    let mut language_map = HashMap::new();
    let mut num_of_entries = 0;
    let mut parsed_any_successfully = false;
    let mut faulty_files : Vec<String> = Vec::new();
    let mut buffer = String::with_capacity(200);
    for entry in dirs {
        let entry = match entry {
            Ok(x) => x,
            Err(_) => continue
        };

        let path = entry.path();
        if !Path::new(&path).is_file() {continue;}
        
        num_of_entries += 1;
        
        let reader = match my_reader::BufReader::open(path) {
            Ok(x) => x,
            Err(_) => {
                let file_name = entry.file_name().to_str().map_or(String::new(), |x| x.to_owned());
                if !file_name.is_empty() {faulty_files.push(file_name.to_lowercase())}
                continue;
            }
        } ;
        
        let language = match parse_file_to_language(reader, &mut buffer) {
            Ok(x) => x,
            Err(_) => {
                let file_name = entry.file_name().to_str().map_or(String::new(), |x| x.to_owned());
                if !file_name.is_empty() {faulty_files.push(file_name.to_lowercase())}
                continue;
            }
        };

        parsed_any_successfully = true;

        if !languages_of_interest.is_empty() {
            if !languages_of_interest.contains(&language.name.to_lowercase()) {
                continue;
            }

            *languages_of_interest_appearance.get_mut(&language.name.to_lowercase()).unwrap() = true;
        }

        language_map.insert(language.name.to_owned(), language);
    }
    
    if num_of_entries == 0 {
        return Err(ParseLanguageError::NoFilesFound);
    } 
    
    let mut non_existant_languages_of_interest = Vec::new();
    if !languages_of_interest.is_empty() {
        languages_of_interest_appearance.iter().for_each(|x| 
            if !x.1 && !faulty_files.contains(&(x.0.to_owned() + ".txt")) {
                non_existant_languages_of_interest.push(x.0.to_owned())
            });
    }

    if !parsed_any_successfully {
        return Err(ParseLanguageError::NoFilesFormattedProperly);
    }

    if !languages_of_interest.is_empty() && non_existant_languages_of_interest.len() == languages_of_interest.len() {
        return Err(ParseLanguageError::LanguagesOfInterestNotFound);
    }

    languages_of_interest.retain(|x| !non_existant_languages_of_interest.contains(x) && !faulty_files.contains(&(x.to_owned()+".txt")));

    Ok((language_map, faulty_files, non_existant_languages_of_interest))
}

pub fn parse_config_file(file_name: Option<&str>) -> Result<(PersistentOptions,bool),ParseConfigFileError> {
    let dir_path = DATA_DIR.clone().unwrap() + CONFIG_DIR_NAME;

    if !Path::new(&dir_path).is_dir() {
        return Err(ParseConfigFileError::DirNotFound);
    }

    let file_name = if let Some(x) = file_name {x} else {DEFAULT_CONFIG_FILE_NAME};
    let file_path = dir_path + "/" + file_name + ".txt";
    let mut reader = BufReader::new(match fs::File::open(file_path){
        Ok(f) => f,
        Err(_) => return Err(ParseConfigFileError::FileNotFound(file_name.to_owned()))
    });

    let (mut path, mut braces_as_code, mut search_in_dotted, mut threads, mut exclude_dirs,
         mut languages_of_interest, mut show_faulty_files, mut no_visual) = (None,None,None,None,None,None,None,None);
    let mut buf = String::with_capacity(150); 
    let mut has_formatting_errors = false;

    while let Ok(size) = reader.read_line(&mut buf) {
        if size == 0 {break};
        if buf.trim().starts_with("===>") {
            let id = buf.split(' ').nth(1).unwrap_or("").trim();

            if id == config_manager::PATH {
                buf.clear();
                reader.read_line(&mut buf);
                let buf = buf.trim();
                if buf.is_empty() {
                    has_formatting_errors = true;
                    continue;
                }
                path = Some(buf.to_owned());
            } else if id == config_manager::EXCLUDE {
                exclude_dirs = read_vec_value(&mut reader, &mut buf, Box::new(|x| x.replace("\\", "/")));
            } else if id == config_manager::LANGUAGES {
                languages_of_interest = read_vec_value(&mut reader, &mut buf, Box::new(remove_dot_prefix));
            } else if id == config_manager::THREADS {
                threads = read_usize_value(&mut reader, &mut buf);
            }else if id == config_manager::BRACES_AS_CODE {
                braces_as_code = read_bool_value(&mut reader, &mut buf);
            } else if id == config_manager::SHOW_FAULTY_FILES {
                show_faulty_files = read_bool_value(&mut reader, &mut buf);
            } else if id == config_manager::SEARCH_IN_DOTTED {
                search_in_dotted = read_bool_value(&mut reader, &mut buf);
            } else if id == config_manager::NO_VISUAL {
                no_visual = read_bool_value(&mut reader, &mut buf);
            }

        }
        buf.clear();
    }

    Ok((PersistentOptions::new(path,exclude_dirs, languages_of_interest, threads, braces_as_code,
             search_in_dotted, show_faulty_files, no_visual), has_formatting_errors))
}

pub fn save_config_to_file(config_name: &str, config: &Configuration) -> std::io::Result<()> {
    let file_name = DATA_DIR.clone().unwrap() + CONFIG_DIR_NAME + "/" + config_name + ".txt"; 
    let mut writer = BufWriter::new(std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(file_name)?);

    writer.write(b"Generated config file.");

    writer.write(&[b"\n\n===> ",config_manager::PATH.as_bytes(),b"\n"].concat());
    writer.write(config.path.as_bytes());
    writer.write(&[b"\n\n===> ",config_manager::EXCLUDE.as_bytes(),b"\n"].concat());
    writer.write(config.exclude_dirs.join(" ").as_bytes());
    writer.write(&[b"\n\n===> ",config_manager::LANGUAGES.as_bytes(),b"\n"].concat());
    writer.write(config.languages_of_interest.join(" ").as_bytes());
    writer.write(&[b"\n\n===> ",config_manager::THREADS.as_bytes(),b"\n"].concat());
    writer.write(config.threads.to_string().as_bytes());
    writer.write(&[b"\n\n===> ",config_manager::BRACES_AS_CODE.as_bytes(),b"\n"].concat());
    writer.write(if config.braces_as_code {b"yes"} else {b"no"});
    writer.write(&[b"\n\n===> ",config_manager::SEARCH_IN_DOTTED.as_bytes(),b"\n"].concat());
    writer.write(if config.should_search_in_dotted {b"yes"} else {b"no"});
    writer.write(&[b"\n\n===> ",config_manager::SHOW_FAULTY_FILES.as_bytes(),b"\n"].concat());
    writer.write(if config.should_show_faulty_files {b"yes"} else {b"no"});

    writer.write(b"\n");    
    writer.flush();

    Ok(())
}

fn read_bool_value(reader: &mut BufReader<File>, mut buf: &mut String) -> Option<bool> {
    buf.clear();
    reader.read_line(&mut buf);
    let buf = buf.trim();
    if buf.is_empty() {
        return None;
    }
    let buf = buf.to_ascii_lowercase();
    if buf == "yes" || buf ==  "true" {
        Some(true)
    } else {
        Some(false)
    }
}

fn read_usize_value(reader: &mut BufReader<File>, mut buf: &mut String) -> Option<usize> {
    buf.clear();
    reader.read_line(&mut buf);
    let buf = buf.trim();
    if buf.is_empty() {
        return None;
    }

    if let Ok(num) = buf.parse::<usize>() {
        if num <= 8 && num >= 1 {
            Some(num)
        } else {
            None
        }
    } else {
        None
    }
}

fn read_vec_value(reader: &mut BufReader<File>, mut buf: &mut String, mut transformation: Box<dyn FnMut(&str) -> String>) 
        -> Option<Vec<String>> 
{
    buf.clear();
    reader.read_line(&mut buf);
    if buf.trim().is_empty() {
        return None;
    }
    let mut lines = buf.clone();
    loop {
        reader.read_line(&mut buf);
        if let Some(new_line) = buf.strip_prefix('+') {
            if new_line.len() > 1 {
                lines += new_line;
            }
        } else {
            break;
        }
    }
    Some(lines.split(' ').filter_map(|x| get_if_not_empty(&transformation(x.trim()))).collect::<Vec<String>>())
}

fn get_if_not_empty(str: &str) -> Option<String> {
    if str.is_empty() {None}
    else {Some(str.to_owned())}
}

fn remove_dot_prefix(str: &str) -> String {
    if let Some(stripped) = str.strip_prefix('.') {
        stripped.to_owned()
    } else {
        str.to_owned()
    }
}

fn parse_file_to_language(mut reader :my_reader::BufReader, buffer :&mut String) -> Result<Language,()> {
    if !reader.read_line_and_compare(buffer, "Language") {return Err(());}
    if !reader.read_line_exists(buffer) {return Err(());}
    let lang_name = buffer.trim_end().to_owned();
    if !reader.read_line_exists(buffer) {return Err(());}

    if !reader.read_line_and_compare(buffer, "Extensions") {return Err(());}
    let identifiers = match reader.get_line_sliced(buffer) {
        Ok(x) => x,
        Err(_) => return Err(())
    };
    if !reader.read_line_exists(buffer) {return Err(());}

    if !reader.read_line_and_compare(buffer, "String symbols") {return Err(());}
    let string_symbols = match reader.get_line_sliced(buffer) {
        Ok(x) => x,
        Err(_) => return Err(())
    };
    if string_symbols.is_empty() {return Err(());}

    if !reader.read_line_exists(buffer) {return Err(());}
    if !reader.read_line_and_compare(buffer, "Comment symbol") {return Err(());} 
    if !reader.read_line_exists(buffer) {return Err(());}
    let comment_symbol = buffer.trim_end().to_owned();
    if comment_symbol.is_empty() {return Err(());}
    
    let mut multi_start :Option<String> = None;
    let mut multi_end :Option<String> = None;
    if reader.read_line_and_compare(buffer, "Multi line comment start") {
        if !reader.read_line_exists(buffer) {return Err(());}
        let symbol = buffer.trim_end().to_owned();
        if symbol.is_empty() {return Err(());}
        multi_start = Some(symbol);
        if !reader.read_line_and_compare(buffer, "Multi line comment end") {return Err(());}
        if !reader.read_line_exists(buffer) {return Err(());}
        let symbol = buffer.trim_end().to_owned();
        if symbol.is_empty() {return Err(());}
        multi_end = Some(symbol);
        if !reader.read_line_exists(buffer) {return Err(())}
    }
    
    let mut keywords = Vec::new();
    while reader.read_line_exists(buffer) {
        if !reader.read_lines_exist(2, buffer) {return Err(());}
        let name = buffer.trim().to_string().clone();
        if name.is_empty() {return Err(());}
        if !reader.read_line_exists(buffer) {return Err(());}
        let aliases = match reader.get_line_sliced(buffer) {
            Ok(x) => x,
            Err(_) => return Err(())
        };
        if aliases.is_empty() {return Err(());}
        
        let keyword = Keyword {
            descriptive_name : name,
            aliases
        };
        keywords.push(keyword);
    }
    
    Ok(Language {
        name: lang_name,
        extensions: identifiers,
        string_symbols,
        comment_symbol,
        mutliline_comment_start_symbol : multi_start,
        mutliline_comment_end_symbol : multi_end,
        keywords
    })
}

fn try_find_data_dir() -> Option<String> {
    if cfg!(test) {
        return Some(env!("CARGO_MANIFEST_DIR").to_owned() + "\\data")
    } else {
        let abs_path = try_get_folder_of_exe().clone().unwrap_or("".to_owned());
        let data_in_current = &(abs_path.clone() + "\\data");
        let data_in_parent = &(abs_path + "\\..\\..\\data");
        if Path::new(data_in_current).is_dir() {return Some(data_in_current.to_owned())}
        if Path::new(data_in_parent).is_dir() {return Some(data_in_parent.to_owned())}
        None
    }
}

fn try_get_folder_of_exe() -> Option<String> {
    if let Ok(path) = std::env::current_exe() {
        let str_path = path.to_str().map_or("", |x| x);
        if str_path.is_empty() {
            return None;
        }

        if let Some(last_backslash) = str_path.match_indices("\\").last() {
            return Some(str_path[..last_backslash.0].to_owned());
        } 
    }

    None
}

impl ParseLanguageError {
    pub fn formatted(&self) -> String {
        match self {
            Self::NoFilesFound => "Error: No language files found in directory.".red().to_string(),
            Self::NoFilesFormattedProperly => "Error: No language file is formatted properly, so none could be parsed.".red().to_string(),
            Self::LanguagesOfInterestNotFound => "Error: None of the provided languages exists in the languages directory".red().to_string()
        }
    }
}

impl ParseConfigFileError {
    pub fn formatted(&self) -> String {
        match self {
            Self::DirNotFound => "No 'config' dir found, defaults will be used.".yellow().to_string(),
            Self::FileNotFound(x) => format!("'{}' config file not found, defaults will be used.", x).yellow().to_string(),
            Self::IOError => "Unexpected IO error while reading, defaults will be used".yellow().to_string()
        }
    }
}

impl PersistentOptions {
    pub fn new(path: Option<String>, exclude_dirs: Option<Vec<String>>, languages_of_interest: Option<Vec<String>>,
            threads: Option<usize>, braces_as_code: Option<bool>, should_search_in_dotted: Option<bool>,
            should_show_faulty_files: Option<bool>, no_visual: Option<bool>) 
    -> PersistentOptions 
    {
        PersistentOptions {
            path,
            exclude_dirs,
            languages_of_interest,
            threads,
            braces_as_code,
            should_search_in_dotted,
            should_show_faulty_files,
            no_visual
        }
    }
}

mod my_reader {
    use std::{fs::File, io::{self, prelude::*}};

    pub struct BufReader {
        reader: io::BufReader<File>,
    }

    impl BufReader {
        pub fn open(path: impl AsRef<std::path::Path>) -> io::Result<Self> {
            let file = File::open(path)?;
            let reader = io::BufReader::new(file);

            Ok(Self { reader })
        }

        pub fn read_line_exists(&mut self, buffer: &mut String) -> bool {
            match self.read_line(buffer) {
                Err(_) => false,
                Ok(x) => {
                    x != 0 
                }
            }
        }

        pub fn read_line_and_compare(&mut self, buffer: &mut String, other : &str) -> bool {
            match self.read_line(buffer) {
                Ok(_) => {
                    buffer.trim_end() == other
                },
                Err(_) => false
            }
        }

        pub fn read_line(&mut self, buffer: &mut String) -> Result<usize, io::Error> {
            buffer.clear();
            self.reader.read_line(buffer)
        }

        pub fn read_lines_exist(&mut self, num :usize, buffer: &mut String) -> bool {
            for _ in 0..num {
                if !self.read_line_exists(buffer) {return false;}
            }
            
            true
        }

        pub fn get_line_sliced(&mut self, buffer: &mut String) -> Result<Vec<String>, ()> {
            if self.read_line_exists(buffer) {
                let buffer = buffer.trim_end();
                let mut vec = buffer.split_whitespace().filter_map(|s| if s.is_empty() {None} else {Some(s.to_string())})
                    .collect::<Vec<String>>();
                if vec.is_empty() {return Ok(vec![String::new()]);}
                let last_index = vec.len()-1;
                vec[last_index] = vec[last_index].trim_end().to_owned();
                Ok(vec) 
            } else {
                Err(())
            }
        }
    }
}
