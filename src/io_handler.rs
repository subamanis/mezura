use std::{{ffi::OsString, path::Path}, cmp::max, collections::{HashMap as HashMap}, env, fs::{self, File, ReadDir}, io::{self, BufRead, BufReader, BufWriter, Write}, path::PathBuf};

use chrono::{DateTime, Local};
use colored::*;
use lazy_static::lazy_static;

use crate::{Configuration, FinalStats, config_manager::{self, ConfigurationBuilder,BRACES_AS_CODE, EXCLUDE, LANGUAGES, DIRS, SEARCH_IN_DOTTED,
     SHOW_FAULTY_FILES, THREADS, MIN_THREADS_VALUE, MAX_THREADS_VALUE, MIN_COMPARE_LEVEL, MAX_COMPARE_LEVEL}, domain::*, utils};

lazy_static! {
    pub static ref DATA_DIR : Option<String> = try_find_data_dir();
    pub static ref LOG_DIR  : String = find_or_create_log_dir(); 
}

const DEFAULT_CONFIG_FILE_NAME : &str = "default";
const CONFIG_DIR : &str = "/config/";
const LANGUAGE_DIR : &str = "/languages/";
const TEST_CONFIG_DIR : &str = "/test_dir/config/";

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


pub fn parse_supported_languages_to_map(languages_of_interest: &mut Vec<String>)
        -> Result<(HashMap<String,Language>, Vec<String>, Vec<String>), ParseLanguageError> 
{
    let dirs = fs::read_dir(DATA_DIR.clone().unwrap() + LANGUAGE_DIR).unwrap();
    
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

pub fn parse_config_file(file_name: Option<&str>) -> Result<ConfigurationBuilder,ParseConfigFileError> {
    let dir_path = {
        if cfg!(test) {
            env!("CARGO_MANIFEST_DIR").to_owned() + TEST_CONFIG_DIR
        } else {
            DATA_DIR.clone().unwrap() + CONFIG_DIR
        }
    };

    if !Path::new(&dir_path).is_dir() {
        return Err(ParseConfigFileError::DirNotFound);
    }

    let file_name = if let Some(x) = file_name {x} else {DEFAULT_CONFIG_FILE_NAME};
    let file_path = (dir_path + file_name + ".txt").replace("\\", "/");
    let mut reader = BufReader::new(match fs::File::open(file_path){
        Ok(f) => f,
        Err(_) => return Err(ParseConfigFileError::FileNotFound(file_name.to_owned()))
    });

    let (mut dirs, mut braces_as_code, mut should_search_in_dotted, mut threads, mut exclude_dirs, mut languages_of_interest,
         mut should_show_faulty_files, mut no_visual, mut log, mut compare_level) = (None,None,None,None,None,None,None,None,None,None);
    let mut buf = String::with_capacity(150); 

    while let Ok(size) = reader.read_line(&mut buf) {
        if size == 0 {break};
        if buf.trim().starts_with("===>") {
            let id = buf.split(' ').nth(1).unwrap_or("").trim();

            if id == config_manager::DIRS {
                let paths = read_lines_to_vec(&mut reader, &mut buf, utils::parse_paths_to_vec);
                if !paths.is_empty() {
                    dirs = Some(paths);
                }
            } else if id == config_manager::EXCLUDE {
                let paths = read_lines_to_vec(&mut reader, &mut buf, utils::parse_paths_to_vec);
                if !paths.is_empty() {
                    exclude_dirs = Some(paths);
                }
            } else if id == config_manager::LANGUAGES {
                let langs = read_lines_to_vec(&mut reader, &mut buf, utils::parse_languages_to_vec);
                if !langs.is_empty() {
                    languages_of_interest = Some(langs);
                }
            } else if id == config_manager::THREADS {
                buf.clear();
                reader.read_line(&mut buf);
                threads = utils::parse_usize_value(&buf,MIN_THREADS_VALUE, MAX_THREADS_VALUE);
            }else if id == config_manager::BRACES_AS_CODE {
                braces_as_code = read_bool_value(&mut reader, &mut buf);
            } else if id == config_manager::SHOW_FAULTY_FILES {
                should_show_faulty_files = read_bool_value(&mut reader, &mut buf);
            } else if id == config_manager::SEARCH_IN_DOTTED {
                should_search_in_dotted = read_bool_value(&mut reader, &mut buf);
            } else if id == config_manager::NO_VISUAL {
                no_visual = read_bool_value(&mut reader, &mut buf);
            } else if id == config_manager::LOG {
                log = read_bool_value(&mut reader, &mut buf);
            } else if id == config_manager::COMPRARE_LEVEL {
                buf.clear();
                reader.read_line(&mut buf);
                compare_level = utils::parse_usize_value(&buf,MIN_COMPARE_LEVEL, MAX_COMPARE_LEVEL);
            }
        }
        buf.clear();
    }

    Ok(ConfigurationBuilder::new(dirs,exclude_dirs, languages_of_interest, threads, braces_as_code,should_search_in_dotted,
             should_show_faulty_files, no_visual, log, compare_level))
}

pub fn save_config_to_file(config_name: &str, config: &Configuration) -> std::io::Result<()> {
    let file_name = {
        if cfg!(test) {
            (env!("CARGO_MANIFEST_DIR").to_owned() + TEST_CONFIG_DIR + config_name + ".txt").replace("\\", "/")
        } else {
            (DATA_DIR.clone().unwrap() + CONFIG_DIR  + config_name + ".txt").replace("\\", "/")
        }
    };
    let mut writer = BufWriter::new(std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(file_name)?);

    writer.write(b"Auto-generated config file.");

    if !config.dirs.is_empty() {
        writer.write(&[b"\n\n===> ",config_manager::DIRS.as_bytes(),b"\n"].concat());
        writer.write(config.dirs.join(",").as_bytes());
    }
    if !config.exclude_dirs.is_empty() {
        writer.write(&[b"\n\n===> ",config_manager::EXCLUDE.as_bytes(),b"\n"].concat());
        writer.write(config.exclude_dirs.join(",").as_bytes());
    }
    if !config.languages_of_interest.is_empty() {
        writer.write(&[b"\n\n===> ",config_manager::LANGUAGES.as_bytes(),b"\n"].concat());
        writer.write(config.languages_of_interest.join(",").as_bytes());
    }
    writer.write(&[b"\n\n===> ",config_manager::THREADS.as_bytes(),b"\n"].concat());
    writer.write(config.threads.to_string().as_bytes());
    writer.write(&[b"\n\n===> ",config_manager::BRACES_AS_CODE.as_bytes(),b"\n"].concat());
    writer.write(if config.braces_as_code {b"yes"} else {b"no"});
    writer.write(&[b"\n\n===> ",config_manager::SEARCH_IN_DOTTED.as_bytes(),b"\n"].concat());
    writer.write(if config.should_search_in_dotted {b"yes"} else {b"no"});
    writer.write(&[b"\n\n===> ",config_manager::SHOW_FAULTY_FILES.as_bytes(),b"\n"].concat());
    writer.write(if config.should_show_faulty_files {b"yes"} else {b"no"});
    writer.write(&[b"\n\n===> ",config_manager::NO_VISUAL.as_bytes(),b"\n"].concat());
    writer.write(if config.no_visual {b"yes"} else {b"no"});
    writer.write(&[b"\n\n===> ",config_manager::LOG.as_bytes(),b"\n"].concat());
    writer.write(if config.log {b"yes"} else {b"no"});
    writer.write(&[b"\n\n===> ",config_manager::COMPRARE_LEVEL.as_bytes(),b"\n"].concat());
    writer.write(config.compare_level.to_string().as_bytes());

    writer.write(b"\n");    
    writer.flush();

    Ok(())
}

pub fn log_stats(path: &str, contents: &Option<String>, final_stats: &FinalStats, datetime_now: &DateTime<Local>, config: &Configuration) -> io::Result<()> {
    let mut writer = std::io::BufWriter::new(std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(path)?);

    write_current_log(&mut writer, config, datetime_now, final_stats);

    if let Some(contents) = contents {
        writer.write(contents.as_bytes());
    }
    writer.flush();

    Ok(())
}


fn write_current_log(writer: &mut BufWriter<File>, config: &Configuration, datetime_now: &DateTime<Local>, final_stats: &FinalStats) {
    writer.write(b"===>Entry\n");
    writer.write(datetime_now.format("%Y-%m-%d %H:%M:%S %z").to_string().as_bytes());
    writer.write(b"\n");
    writer.write(b"Configuration:\n");
    writer.write(format!("    dirs: {}\n",config.dirs.join(",")).as_bytes());
    writer.write(format!("    exclude: {}\n",config.exclude_dirs.join(",")).as_bytes());
    writer.write(format!("    languages: {}\n",config.languages_of_interest.join(",")).as_bytes());
    writer.write(format!("    braces-as-code: {}\n",if config.braces_as_code{"yes"} else {"no"}).as_bytes());
    writer.write(format!("    search-in-dotted: {}\n",if config.should_search_in_dotted{"yes"} else {"no"}).as_bytes());
    writer.write(b"Stats:\n");
    writer.write(format!("    Files: {}\n",final_stats.files).as_bytes());
    writer.write(format!("    Lines: {}\n",final_stats.lines).as_bytes());
    writer.write(format!("        Code: {}\n",final_stats.code_lines).as_bytes());
    writer.write(format!("        Extra: {}\n",final_stats.extra_lines).as_bytes());
    writer.write(format!("    Total Size: {} {}\n",final_stats.size, final_stats.size_measurement).as_bytes());
    writer.write(format!("        Average Size: {} {}\n\n\n",final_stats.average_size, final_stats.average_size_measurement).as_bytes());
    writer.write(b"--------------------------------------------------------------------------------------------\n\n\n");
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

fn read_lines_to_vec(reader: &mut BufReader<File>, mut buf: &mut String, parser_func: fn(&str) -> Vec<String>) -> Vec<String> {
    let mut vec = Vec::new();
    loop {
        buf.clear();
        reader.read_line(&mut buf);
        if buf.trim().is_empty() {
            break;
        }
        let new_vec = parser_func(buf);
        vec.extend(new_vec);
    }
    vec
}

fn try_find_data_dir() -> Option<String> {
    if cfg!(test) {
        Some(env!("CARGO_MANIFEST_DIR").to_owned() + "\\data")
    } else {
        let abs_path = try_get_folder_of_exe().unwrap_or_else(|| "".to_owned());
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

        if let Some(last_backslash) = str_path.match_indices('\\').last() {
            return Some(str_path[..last_backslash.0].to_owned());
        } 
    }

    None
}

fn find_or_create_log_dir() -> String {
    let path = DATA_DIR.clone().unwrap() + "/logs/";
    if !Path::new(&path).exists() {
        fs::create_dir(&path).unwrap();
    }
    path
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

#[cfg(test)]
mod tests {
    use crate::{Configuration, config_manager, io_handler::{parse_config_file, save_config_to_file}};

    #[test]
    fn test_save_config_file_and_then_parse_it() -> std::io::Result<()> {
        let working_dir = env!("CARGO_MANIFEST_DIR").to_owned() + "/data";
        let command = format!("{}/config, {}/languages --exclude a,b,c.txt,d.txt, --braces-as-code --threads 4", working_dir, working_dir);
        let config = config_manager::create_config_from_args(&command).unwrap();

        save_config_to_file("auto-generated", &config);

        let options = parse_config_file(Some("auto-generated")).unwrap();
        assert_eq!(config.dirs, options.dirs.unwrap());
        assert_eq!(config.exclude_dirs, options.exclude_dirs.unwrap());
        assert_eq!(config.threads, options.threads.unwrap());
        assert_eq!(config.braces_as_code, options.braces_as_code.unwrap());
        assert_eq!(config.should_show_faulty_files, options.should_show_faulty_files.unwrap());
        assert_eq!(config.should_search_in_dotted, options.should_search_in_dotted.unwrap());
        assert_eq!(config.no_visual, options.no_visual.unwrap());

        Ok(())
    }

    #[test]
    fn test_read_config_file() -> std::io::Result<()> {
        let mut config = Configuration::new(vec!["C:/Some/Path/a".to_owned(),"C:/Some/Path/b".to_owned(),"C:/Some/Path/c".to_owned(),"C:/Some/Path/d".to_owned()]);
        config
            .set_exclude_dirs(vec!["a".to_owned(), "b".to_owned(), "c.txt".to_owned(), "d.txt".to_owned()])
            .set_threads(4)
            .set_braces_as_code(true);


        let options = parse_config_file(Some("test")).unwrap();
        assert_eq!(config.dirs, options.dirs.unwrap());
        assert_eq!(config.exclude_dirs, options.exclude_dirs.unwrap());
        assert_eq!(config.threads, options.threads.unwrap());
        assert_eq!(config.braces_as_code, options.braces_as_code.unwrap());
        assert_eq!(config.should_show_faulty_files, options.should_show_faulty_files.unwrap());
        assert_eq!(config.should_search_in_dotted, options.should_search_in_dotted.unwrap());
        assert_eq!(config.no_visual, options.no_visual.unwrap());

        Ok(())
    }
}
