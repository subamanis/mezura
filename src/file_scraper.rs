use std::{{ffi::OsString, path::Path}, collections::{HashMap as HashMap, btree_map::Keys}, env, ffi::OsStr, fs, vec};

use colored::*;

use crate::{VecRef,BoolRef, my_reader, domain::*};

pub enum ParseExtensionsError {
    UnavailableDirectory(String),
    NoFilesFound,
    NoFilesFormattedProperly,
}

impl ParseExtensionsError {
    pub fn formatted(&self) -> String {
        match self {
            Self::UnavailableDirectory(x) => format!("\nError while trying to open the extensions` path (./extensions): {}",x).red().to_string(),
            Self::NoFilesFormattedProperly => "\nError: No extension file is formatted properly, so none could be parsed.".red().to_string(),
            Self::NoFilesFound => "\nError: No extension files found in directory.".red().to_string()
        }
    }
}


pub fn parse_supported_extensions_to_map() -> Result<(HashMap<String,Extension>, Vec<OsString>), ParseExtensionsError> {
    let mut extensions_map = HashMap::new();
    let dirs = match fs::read_dir(get_extensions_path()) {
        Err(x) => return Err(ParseExtensionsError::UnavailableDirectory(x.to_string())),
        Ok(x) => x
    };
    
    let mut num_of_entries = 0;
    let mut faulty_files : Vec<OsString> = Vec::new();
    let mut buffer = String::with_capacity(200);
    for entry in dirs {
        let entry = match entry {
            Ok(x) => x,
            Err(_) => continue
        };
        num_of_entries += 1;
        let path = entry.path();
        if !Path::new(&path).is_file() {continue;}
        
        //@TODO: helper func that returns opt?
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
        
        extensions_map.insert(extension.name.to_owned(), extension);
    }
    
    if num_of_entries == 0 {
        return Err(ParseExtensionsError::NoFilesFound);
    }
    
    if extensions_map.is_empty() {
        Err(ParseExtensionsError::NoFilesFormattedProperly)
    } else {
        Ok((extensions_map, faulty_files))
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

fn get_extensions_path() -> String {
    let mut curr : String = match env::current_dir() {
        Ok(x) => {
            match x.to_str() {
                Some(s) => s.to_owned(),
                None => String::new()
            }
        },
        Err(_) => String::new()
    };

    curr.push_str("\\extensions\\");

    curr
}
