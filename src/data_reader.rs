use std::{{ffi::OsString, path::Path}, cmp::max, collections::{HashMap as HashMap}, env, fs::{self, File, ReadDir}, io::{BufRead, BufReader, BufWriter, Write}, path::PathBuf};

use colored::*;
use lazy_static::lazy_static;

use crate::{Configuration, config_manager, domain::*};

lazy_static! {
    pub static ref DATA_DIR : Option<String> = try_find_data_dir();
}

const DEFAULT_CONFIG_FILE_NAME : &str = "default_config";
const CONFIG_DIR_NAME : &str = "/config";
const EXTENSIONS_DIR_NAME : &str = "/extensions";

#[derive(Debug)]
pub enum ParseExtensionsError {
    NoFilesFound,
    NoFilesFormattedProperly,
    ExtensionsOfInterestNotFound
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
    pub extensions_of_interest   : Option<Vec<String>>,
    pub threads                  : Option<usize>,
    pub braces_as_code           : Option<bool>,
    pub should_search_in_dotted  : Option<bool>,
    pub should_show_faulty_files : Option<bool>
}


pub fn parse_supported_extensions_to_map(extensions_of_interest: &[String])
        -> Result<(HashMap<String,Extension>, Vec<OsString>), ParseExtensionsError> 
{
    let dirs = fs::read_dir(DATA_DIR.clone().unwrap() + EXTENSIONS_DIR_NAME).unwrap();
    
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
         mut extensions_of_interest, mut show_faulty_files) = (None,None,None,None,None,None,None);
    let mut buf = String::with_capacity(150); 
    let mut has_formatting_errors = false;

    while let Ok(size) = reader.read_line(&mut buf) {
        if size == 0 {break};
        if buf.trim().starts_with("===>") {
            let id = buf.split(' ').nth(1).unwrap_or("").trim();

            if id == "path" {
                buf.clear();
                reader.read_line(&mut buf);
                let buf = buf.trim();
                if buf.is_empty() {
                    has_formatting_errors = true;
                    continue;
                }
                path = Some(buf.to_owned());
            } else if id == "exclude" {
                exclude_dirs = read_vec_value(&mut reader, &mut buf, Box::new(|x| x));
            } else if id == "extensions" {
                extensions_of_interest = read_vec_value(&mut reader, &mut buf, Box::new(remove_dot_prefix));
            } else if id == "threads" {
                threads = read_usize_value(&mut reader, &mut buf);
            }else if id == "braces-as-code" {
                braces_as_code = read_bool_value(&mut reader, &mut buf);
            } else if id == "show-faulty-files" {
                show_faulty_files = read_bool_value(&mut reader, &mut buf);
            } else if id == "search-in-dotted" {
                search_in_dotted = read_bool_value(&mut reader, &mut buf);
            }

        }
        buf.clear();
    }

    Ok((PersistentOptions::new(path,exclude_dirs, extensions_of_interest, threads, braces_as_code,
             search_in_dotted, show_faulty_files), has_formatting_errors))
}

pub fn save_config_to_file(config_name: &str, config: &Configuration) -> std::io::Result<()> {
    let file_name = DATA_DIR.clone().unwrap() + CONFIG_DIR_NAME + "/" + config_name + ".txt"; 
    let mut writer = BufWriter::new(std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(file_name)?);

    writer.write(b"Generated config file.");

    writer.write(b"\n\n===> path\n");
    writer.write(config.path.as_bytes());
    writer.write(b"\n\n===> exclude\n");
    writer.write(config.exclude_dirs.join(" ").as_bytes());
    writer.write(b"\n\n===> extensions\n");
    writer.write(config.extensions_of_interest.join(" ").as_bytes());
    writer.write(b"\n\n===> threads\n");
    writer.write(config.threads.to_string().as_bytes());
    writer.write(b"\n\n===> braces-as-code\n");
    writer.write(if config.braces_as_code {b"yes"} else {b"no"});
    writer.write(b"\n\n===> search-in-dotted\n");
    writer.write(if config.should_search_in_dotted {b"yes"} else {b"no"});
    writer.write(b"\n\n===> show-faulty-files\n");
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
        return Some(true);
    } else {
        return Some(false);
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

fn read_vec_value(reader: &mut BufReader<File>, mut buf: &mut String, mut transformation: Box<dyn FnMut(&str) -> &str>) 
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
    Some(lines.split(' ').filter_map(|x| get_if_not_empty(transformation(x.trim()))).collect::<Vec<String>>())
}

fn get_if_not_empty(str: &str) -> Option<String> {
    if str.is_empty() {None}
    else {Some(str.to_owned())}
}

fn remove_dot_prefix(str: &str) -> &str {
    if let Some(stripped) = str.strip_prefix('.') {
        stripped
    } else {
        str
    }
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
    None
}

impl ParseExtensionsError {
    pub fn formatted(&self) -> String {
        match self {
            Self::NoFilesFound => "Error: No extension files found in directory.".red().to_string(),
            Self::NoFilesFormattedProperly => "Error: No extension file is formatted properly, so none could be parsed.".red().to_string(),
            Self::ExtensionsOfInterestNotFound => "Error: None of the provided extensions exists in the extensions directory".red().to_string()
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
    pub fn new(path: Option<String>, exclude_dirs: Option<Vec<String>>, extensions_of_interest: Option<Vec<String>>,
        threads: Option<usize>, braces_as_code: Option<bool>, should_search_in_dotted: Option<bool>, should_show_faulty_files: Option<bool>) 
    -> PersistentOptions {
        PersistentOptions {
            path,
            exclude_dirs,
            extensions_of_interest,
            threads,
            braces_as_code,
            should_search_in_dotted,
            should_show_faulty_files
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
