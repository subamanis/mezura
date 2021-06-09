use std::{{ffi::OsString, path::Path}, cmp::max, collections::{HashMap as HashMap}, env, fs::{self, ReadDir}, io::{BufRead, BufReader}, path::PathBuf};

use colored::*;
use lazy_static::lazy_static;

use crate::{config_manager, domain::*};

const DEFAULT_CONFIG_FILE_NAME : &'static str = "default_config.txt";

#[derive(Debug)]
pub enum ParseExtensionsError {
    DataDirNotFound,
    DirNotFound,
    NoFilesFound,
    NoFilesFormattedProperly,
    ExtensionsOfInterestNotFound
}

#[derive(Debug)]
pub enum ParseConfigFileError {
    DataDirNotFound,
    DirNotFound,
    FileNotFound(String)
}

#[derive(Debug)]
pub enum ParseConfigFileWarning {
    DataDirNotFound,
    DirNotFound,
    FileNotFound(String)
}

#[derive(Debug)]
pub struct PersistentOptions {
    pub exclude_dirs             : Option<Vec<String>>,
    pub extensions_of_interest   : Option<Vec<String>>,
    pub threads                  : Option<usize>,
    pub braces_as_code           : Option<bool>,
    pub should_search_in_dotted  : Option<bool>
}

lazy_static! {
    static ref DATA_DIR : Option<String> = try_find_data_dir();
}

pub fn parse_supported_extensions_to_map(extensions_of_interest: &Vec<String>)
        -> Result<(HashMap<String,Extension>, Vec<OsString>), ParseExtensionsError> 
{
    let data_dir = match DATA_DIR.clone() {
        Some(x) => x,
        None => return Err(ParseExtensionsError::DataDirNotFound)
    };
    
    let dirs = match fs::read_dir(data_dir + "/extensions") {
        Ok(x) => x,
        Err(_) => return Err(ParseExtensionsError::DirNotFound)
    };
    
    let mut extensions_map = HashMap::new();
    let mut num_of_entries = 0;
    let mut parsed_any_successfully = false;
    let mut faulty_files : Vec<OsString> = Vec::new();
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
                faulty_files.push(entry.file_name());
                continue;
            }
        } ;
        
        let extension = match parse_file_to_extension(reader, &mut buffer) {
            Ok(x) => x,
            Err(_) => {
                faulty_files.push(entry.file_name());
                continue;
            }
        };

        parsed_any_successfully = true;
        
        if !extensions_of_interest.is_empty() && !extensions_of_interest.contains(&extension.name) {
            continue;
        }

        extensions_map.insert(extension.name.to_owned(), extension);
    }
    
    if num_of_entries == 0 {
        return Err(ParseExtensionsError::NoFilesFound);
    }    

    if !parsed_any_successfully {
        return Err(ParseExtensionsError::NoFilesFormattedProperly);
    } else if extensions_map.is_empty() {
        return Err(ParseExtensionsError::ExtensionsOfInterestNotFound);
    } 

    Ok((extensions_map, faulty_files))
}

pub fn parse_config_file(file_name: Option<&str>) -> Result<(PersistentOptions,bool),ParseConfigFileError> {
    let data_dir = match DATA_DIR.clone() {
        Some(x) => x,
        None => return Err(ParseConfigFileError::DataDirNotFound)
    };

    let dir_path = data_dir + "/config";
    if !Path::new(&dir_path).is_dir() {
        return Err(ParseConfigFileError::DirNotFound);
    }

    let file_name = if let Some(x) = file_name {x} else {DEFAULT_CONFIG_FILE_NAME};
    let file_path = dir_path + file_name;
    let mut reader = BufReader::new(match fs::File::open(file_path){
        Ok(f) => f,
        Err(_) => return Err(ParseConfigFileError::FileNotFound(file_name.to_owned()))
    });

    let (mut braces_as_code, mut search_in_dotted, mut threads, mut exclude_dirs,
         mut extensions_of_interest) = (None,None,None,None,None);
    let mut buf = String::with_capacity(150); 
    let mut has_formatting_errors = false;

    while let Ok(size) = reader.read_line(&mut buf) {
        if size == 0 {break};

        if buf.starts_with("===>") {
            let id = buf.split(" ").skip(1).next().unwrap_or("").trim();

            if id == "braces_as_code" {
                reader.read_line(&mut buf);
                let buf = buf.trim();
                if buf.is_empty() {
                    has_formatting_errors = true;
                    continue;
                }
                if buf == "yes" || buf ==  "true" {
                    braces_as_code = Some(true);
                } else {
                    braces_as_code = Some(false);
                }
            } else if id == "search_in_dotted" {
                reader.read_line(&mut buf);
                let buf = buf.trim();
                if buf.is_empty() {
                    has_formatting_errors = true;
                    continue;
                }
                if buf == "yes" || buf ==  "true" {
                    search_in_dotted = Some(true);
                } else {
                    search_in_dotted = Some(false);
                }
            } else if id == "threads" {
                reader.read_line(&mut buf);
                let buf = buf.trim();
                if buf.is_empty() {
                    has_formatting_errors = true;
                    continue;
                }
                if let Ok(num) = buf.parse::<usize>() {
                    if num <= 8 && num >= 1 {
                        threads = Some(num);
                    } else {
                        has_formatting_errors = true;
                    }
                }
            } else if id == "exclude" {
                reader.read_line(&mut buf);
                let buf = buf.trim();
                if buf.is_empty() {
                    has_formatting_errors = true;
                    continue;
                }
                exclude_dirs = Some(buf.split(" ").map(|x| x.trim().to_owned()).collect::<Vec<String>>());
            } else if id == "extensions" {
                reader.read_line(&mut buf);
                let buf = buf.trim();
                if buf.is_empty() {
                    has_formatting_errors = true;
                    continue;
                }
                extensions_of_interest = Some(buf.split(" ").map(|x| x.trim().to_owned()).collect::<Vec<String>>());
            }
        }
    }

    Ok((PersistentOptions::new(exclude_dirs, extensions_of_interest, threads, braces_as_code, search_in_dotted), has_formatting_errors))
}

fn parse_file_to_extension(mut reader :my_reader::BufReader, buffer :&mut String) -> Result<Extension,()> {
    if !reader.read_line_and_compare(buffer, "Extension") {return Err(());}
    if !reader.read_line_exists(buffer) {return Err(());}
    let extension_name = buffer.trim_end().to_owned();
    if !reader.read_line_exists(buffer) {return Err(());}
    if !reader.read_line_and_compare(buffer, "String symbols") {return Err(());}
    let string_symbols = match reader.get_line_sliced(buffer) {
        Ok(x) => x,
        Err(_) => return Err(())
    };
    if !reader.read_line_exists(buffer) {return Err(());}
    if !reader.read_line_and_compare(buffer, "Comment symbol") {return Err(());} 
    if !reader.read_line_exists(buffer) {return Err(());}
    let comment_symbol = buffer.trim_end().to_owned();
    
    let mut multi_start :Option<String> = None;
    let mut multi_end :Option<String> = None;
    if reader.read_line_and_compare(buffer, "Multi line comment start") {
        if !reader.read_line_exists(buffer) {return Err(());}
        multi_start = Some(buffer.trim_end().to_owned());
        if !reader.read_line_and_compare(buffer, "Multi line comment end") {return Err(());}
        if !reader.read_line_exists(buffer) {return Err(());}
        multi_end = Some(buffer.trim_end().to_owned());
        if !reader.read_line_exists(buffer) {return Err(())}
    }
    
    let mut keywords = Vec::new();
    while reader.read_line_exists(buffer) {
        if !reader.read_lines_exist(2, buffer) {return Err(());}
        let name = buffer.trim().to_string().clone();
        if !reader.read_line_exists(buffer) {return Err(());}
        let aliases = match reader.get_line_sliced(buffer) {
            Ok(x) => x,
            Err(_) => return Err(())
        };
        
        let keyword = Keyword {
            descriptive_name : name,
            aliases
        };
        keywords.push(keyword);
    }
    
    Ok(Extension {
        name : extension_name,
        string_symbols,
        comment_symbol,
        mutliline_comment_start_symbol : multi_start,
        mutliline_comment_end_symbol : multi_end,
        keywords
    })
}

fn try_find_data_dir() -> Option<String> {
    if Path::new("data").is_dir() {return Some("data".to_string())}
    if Path::new("../../data").is_dir() {return Some("../../data".to_string())}
    return None
}

impl ParseExtensionsError {
    pub fn formatted(&self) -> String {
        match self {
            Self::DataDirNotFound => "Data dir not found in any known location.".red().to_string(),
            Self::DirNotFound => "Extensions dir not present.".red().to_string(),
            Self::NoFilesFound => "Error: No extension files found in directory.".red().to_string(),
            Self::NoFilesFormattedProperly => "Error: No extension file is formatted properly, so none could be parsed.".red().to_string(),
            Self::ExtensionsOfInterestNotFound => "Error: None of the provided extensions exists in the extensions directory".red().to_string()
        }
    }
}

impl ParseConfigFileError {
    pub fn formatted(&self) -> String {
        match self {
            Self::DataDirNotFound => "Data dir not found in any known location.".red().to_string(),
            Self::DirNotFound => "No 'config' dir found, defaults will be used.".yellow().to_string(),
            Self::FileNotFound(x) => format!("'{}' config file not found, defaults will be used.", x).yellow().to_string()
        }
    }
}

impl PersistentOptions {
    pub fn new(exclude_dirs: Option<Vec<String>>, extensions_of_interest: Option<Vec<String>>,
        threads: Option<usize>, braces_as_code: Option<bool>, search_in_dotted: Option<bool>) -> PersistentOptions {
        PersistentOptions {
            exclude_dirs,
            extensions_of_interest,
            braces_as_code,
            should_search_in_dotted: search_in_dotted,
            threads,
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
                let mut vec = buffer.split_whitespace().map(|s| s.to_string()).collect::<Vec<String>>();
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
