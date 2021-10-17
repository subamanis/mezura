use std::{borrow::Cow, collections::HashMap, fs::{self, DirEntry, File}, io::{self, BufRead, BufReader, BufWriter, Write}, path::Path};

use chrono::{DateTime, Local};
use colored::*;

use crate::{Configuration, DEFAULT_CONFIG_NAME, FinalStats, Formatted, PERSISTENT_APP_PATHS, config_manager::{self, ConfigurationBuilder, LogOption,
     MAX_COMPARE_LEVEL, MAX_CONSUMERS_VALUE, MAX_PRODUCERS_VALUE, MIN_COMPARE_LEVEL, MIN_CONSUMERS_VALUE, MIN_PRODUCERS_VALUE, Threads}, domain::*, split_line_on_whitespace, utils};


const LANGUAGE                 : &str = "Language";     
const EXTENSIONS               : &str = "Extensions";     
const STRING_SYMBOLS           : &str = "String symbols";     
const COMMENT_SYMBOLS          : &str = "Comment symbols";     
const MULTILINE_COMMENT_START  : &str = "Multi line comment start";     
const MULTILINE_COMMENT_END    : &str = "Multi line comment end";     
const KEYWORD                  : &str = "Keyword";     
const KEYWORD_NAME             : &str = "NAME";     
const KEYWORD_ALIASES          : &str = "ALIASES";     


#[derive(Debug)]
pub struct LanguageDirParseInfo {
    pub language_map: HashMap<String,Language>,
    pub faulty_files: Vec<String>,
    pub non_existant_languages:  Vec<String>
}

#[derive(Debug)]
pub enum LanguageDirParseError {
    NoFilesFound,
    NoFilesFormattedProperly,
}

#[derive(Debug)]
pub enum ConfigFileParseError {
    FileNotFound(String),
    IOError
}


// --------------------- Languages handling -------------------------

pub fn parse_supported_languages_to_map(target_path: &str) -> Result<(HashMap<String, Language>, Vec<String>), LanguageDirParseError> {
    fn add_file_name_to_faulty_files(entry: &DirEntry, faulty_files: &mut Vec<String>) {
        let file_name = entry.file_name().to_str().map_or(String::new(), |x| x.to_owned());
        if !file_name.is_empty() {faulty_files.push(file_name.to_lowercase())}
    }

    let mut language_map = HashMap::with_capacity(30);
    let mut faulty_files : Vec<String> = Vec::new();
    let mut buffer = String::with_capacity(200);
    
    let entries = fs::read_dir(target_path).unwrap();
    for entry in entries {
        let entry = match entry {
            Ok(x) => x,
            Err(_) => continue
        };

        let path = entry.path();
        if !Path::new(&path).is_file() {continue;}
        
        let reader = match my_reader::BufReader::open(path) {
            Ok(x) => x,
            Err(_) => {
                add_file_name_to_faulty_files(&entry, &mut faulty_files);
                continue;
            }
        } ;
        
        let language = match parse_file_to_language(reader, &mut buffer) {
            Ok(x) => x,
            Err(_) => {
                add_file_name_to_faulty_files(&entry, &mut faulty_files);
                continue;
            }
        };

        language_map.insert(language.name.to_owned(), language);
    }

    if language_map.is_empty() && faulty_files.is_empty() {
        Err(LanguageDirParseError::NoFilesFound)
    } else if language_map.is_empty() {
        Err(LanguageDirParseError::NoFilesFormattedProperly)
    } else {
        Ok((language_map, faulty_files))
    }
}

fn parse_file_to_language(mut reader :my_reader::BufReader, buffer :&mut String) -> Result<Language,()> {
    if !reader.read_line_and_compare(buffer, LANGUAGE) {return Err(());}
    if !reader.read_line_exists(buffer) {return Err(());}
    let lang_name = buffer.trim_end().to_owned();
    if !reader.read_line_exists(buffer) {return Err(());}

    if !reader.read_line_and_compare(buffer, EXTENSIONS) {return Err(());}
    let identifiers = match reader.get_line_sliced(buffer) {
        Ok(x) => x,
        Err(_) => return Err(())
    };
    if !reader.read_line_exists(buffer) {return Err(());}

    if !reader.read_line_and_compare(buffer, STRING_SYMBOLS) {return Err(());}
    let string_symbols = match reader.get_line_sliced(buffer) {
        Ok(x) => x,
        Err(_) => return Err(())
    };
    if string_symbols.is_empty() {return Err(());}

    if !reader.read_line_exists(buffer) {return Err(());}
    if !reader.read_line_and_compare(buffer, COMMENT_SYMBOLS) {return Err(());} 
    let comment_symbols = match reader.get_line_sliced(buffer) {
        Ok(x) => x,
        Err(_) => return Err(())
    };
    
    let mut multi_start :Option<String> = None;
    let mut multi_end :Option<String> = None;
    if reader.read_line_and_compare(buffer, MULTILINE_COMMENT_START) {
        if !reader.read_line_exists(buffer) {return Err(());}
        let symbol = buffer.trim_end().to_owned();
        if symbol.is_empty() {return Err(());}
        multi_start = Some(symbol);
        if !reader.read_line_and_compare(buffer, MULTILINE_COMMENT_END) {return Err(());}
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
        comment_symbols,
        multiline_comment_start_symbol : multi_start,
        multiline_comment_end_symbol : multi_end,
        keywords
    })
}

pub fn parse_string_to_language(contents: Cow<str>) -> Language {
    let mut lines = (&contents).lines();
    let (mut mult_start, mut mult_end) = (None, None);

    lines.next();
    let lang_name = lines.next().unwrap().trim().to_owned();
    lines.next();
    lines.next();
    let extensions = split_line_on_whitespace(lines.next().unwrap());
    lines.next();
    lines.next();
    let string_symbols = split_line_on_whitespace(lines.next().unwrap());
    lines.next();
    lines.next();
    let comment_symbols = split_line_on_whitespace(lines.next().unwrap());
    let next_line = lines.next();
    if let Some(line) = next_line {
        if line == MULTILINE_COMMENT_START {
            mult_start = Some(lines.next().unwrap().trim().to_owned());
            lines.next();
            mult_end = Some(lines.next().unwrap().trim().to_owned());
            lines.next();
        }
    }

    let mut keywords = Vec::new();
    while let Some(x) = lines.next() {
        if x != KEYWORD {break;} 

        lines.next();
        let k_name = lines.next().unwrap().trim().to_owned();
        lines.next();
        let k_aliases = split_line_on_whitespace(lines.next().unwrap());
        keywords.push(Keyword{
            descriptive_name: k_name,
            aliases: k_aliases
        });
    }

    Language::new(lang_name, extensions, string_symbols, comment_symbols, mult_start, mult_end, keywords)
}

pub fn serialize_language(lang: &Language, path: &str) -> Result<(), io::Error> {
    let file_path = path.to_string() + "/" + &lang.name + ".txt";
    let mut writer = BufWriter::new(std::fs::OpenOptions::new().write(true).create(true).open(file_path)?);

    writer.write(format!("{}\n",LANGUAGE).as_bytes());
    writer.write(lang.name.as_bytes());
    writer.write(b"\n\n");

    writer.write(format!("{}\n",EXTENSIONS).as_bytes());
    writer.write(lang.extensions.join(" ").as_bytes());
    writer.write(b"\n\n");

    writer.write(format!("{}\n",STRING_SYMBOLS).as_bytes());
    writer.write(lang.string_symbols.join(" ").as_bytes());
    writer.write(b"\n\n");

    writer.write(format!("{}\n",COMMENT_SYMBOLS).as_bytes());
    writer.write(lang.comment_symbols.join(" ").as_bytes());
    writer.write(b"\n");
    
    if let Some(symbol) = &lang.multiline_comment_start_symbol {
        writer.write(format!("{}\n",MULTILINE_COMMENT_START).as_bytes());
        writer.write(symbol.as_bytes());
        writer.write(b"\n");
        writer.write(format!("{}\n",MULTILINE_COMMENT_END).as_bytes());
        writer.write(lang.multiline_comment_end_symbol.as_ref().unwrap().as_bytes());
        writer.write(b"\n");
    }
    writer.write(b"\n");
    
    for keyword in lang.keywords.iter() {
        writer.write(format!("{}\n",KEYWORD).as_bytes());
        writer.write(format!("{}\n",KEYWORD_NAME).as_bytes());
        writer.write(keyword.descriptive_name.as_bytes());
        writer.write(b"\n");
        writer.write(format!("{}\n",KEYWORD_ALIASES).as_bytes());
        writer.write(keyword.aliases.join(" ").as_bytes());
        writer.write(b"\n");
    }

    Ok(())
}


// ------------------------------ Config handling ------------------------------

pub fn parse_config_file(file_name: Option<&str>, config_dir_path: Option<String>) -> Result<ConfigurationBuilder,ConfigFileParseError> {
    let config_path = if let Some(dir) = config_dir_path {dir} else {PERSISTENT_APP_PATHS.config_dir.clone()};
    let file_name = if let Some(x) = file_name {x} else {DEFAULT_CONFIG_NAME};
    let file_path = (config_path + file_name + ".txt").replace("\\", "/");
    let mut reader = BufReader::new(match fs::File::open(file_path){
        Ok(f) => f,
        Err(_) => return Err(ConfigFileParseError::FileNotFound(file_name.to_owned()))
    });

    let (mut dirs, mut braces_as_code, mut should_search_in_dotted, mut threads, mut exclude_dirs,
         mut languages_of_interest, mut should_show_faulty_files, mut no_keywords, mut no_visual,
         mut log, mut compare_level) = (None,None,None,None,None,None,None,None,None,None,None);
    let mut buf = String::with_capacity(150); 

    while let Ok(size) = reader.read_line(&mut buf) {
        if size == 0 {break};
        if buf.trim().starts_with("===>") {
            let id = buf.split(' ').nth(1).unwrap_or("").trim();

            if id == config_manager::DIRS {
                let paths = read_lines_from_file_to_vec(&mut reader, &mut buf, utils::parse_paths_to_vec);
                if !paths.is_empty() {
                    dirs = Some(paths);
                }
            } else if id == config_manager::EXCLUDE {
                let paths = read_lines_from_file_to_vec(&mut reader, &mut buf, utils::parse_paths_to_vec);
                if !paths.is_empty() {
                    exclude_dirs = Some(paths);
                }
            } else if id == config_manager::LANGUAGES {
                let langs = read_lines_from_file_to_vec(&mut reader, &mut buf, utils::parse_languages_to_vec);
                if !langs.is_empty() {
                    languages_of_interest = Some(langs);
                }
            } else if id == config_manager::THREADS {
                buf.clear();
                reader.read_line(&mut buf);
                threads = Some(Threads::from(utils::parse_two_usize_values(&buf,MIN_PRODUCERS_VALUE, MAX_PRODUCERS_VALUE,
                        MIN_CONSUMERS_VALUE, MAX_CONSUMERS_VALUE).unwrap()));
            }else if id == config_manager::BRACES_AS_CODE {
                braces_as_code = read_bool_value_from_file(&mut reader, &mut buf);
            } else if id == config_manager::SHOW_FAULTY_FILES {
                should_show_faulty_files = read_bool_value_from_file(&mut reader, &mut buf);
            } else if id == config_manager::SEARCH_IN_DOTTED {
                should_search_in_dotted = read_bool_value_from_file(&mut reader, &mut buf);
            } else if id == config_manager::NO_KEYWORDS {
                no_keywords = read_bool_value_from_file(&mut reader, &mut buf);
            } else if id == config_manager::NO_VISUAL {
                no_visual = read_bool_value_from_file(&mut reader, &mut buf);
            } else if id == config_manager::LOG {
                buf.clear();
                reader.read_line(&mut buf);
                let name = &buf.trim().to_lowercase();
                if name == "yes" || name == "true" {
                    log = Some(LogOption::new(None));
                } else if name != "no" && name != "false"{
                    log = Some(LogOption::new(Some(name.to_owned())));
                }
            } else if id == config_manager::COMPRARE_LEVEL {
                buf.clear();
                reader.read_line(&mut buf);
                compare_level = utils::parse_usize_value(&buf,MIN_COMPARE_LEVEL, MAX_COMPARE_LEVEL);
            }
        }
        buf.clear();
    }

    Ok(ConfigurationBuilder::new(dirs,exclude_dirs, languages_of_interest, threads, braces_as_code,should_search_in_dotted,
             should_show_faulty_files, no_keywords, no_visual, log, compare_level, None, None))
}

// Dirs must be specified (is checked before calling this function)
pub fn save_existing_commands_from_config_builder_to_file(config_path: Option<String>, config_name: &str, config_builder: &ConfigurationBuilder) 
-> std::io::Result<()> 
{
    let config_dir = if let Some(dir) = config_path {dir} else {PERSISTENT_APP_PATHS.config_dir.clone()};
    let file_name = config_dir + config_name + ".txt";

    let mut writer = BufWriter::new(std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(file_name)?);

    writer.write(b"Auto-generated config file.");

    writer.write(&[b"\n\n===> ",config_manager::DIRS.as_bytes(),b"\n"].concat());
    writer.write(config_builder.dirs.as_ref().unwrap().join(",").as_bytes());

    if let Some(exclude_dirs) = &config_builder.exclude_dirs {
        writer.write(&[b"\n\n===> ",config_manager::EXCLUDE.as_bytes(),b"\n"].concat());
        writer.write(exclude_dirs.join(",").as_bytes());
    }
    if let Some(languages_of_interest) = &config_builder.languages_of_interest {
        writer.write(&[b"\n\n===> ",config_manager::LANGUAGES.as_bytes(),b"\n"].concat());
        writer.write(languages_of_interest.join(",").as_bytes());
    }
    if let Some(threads) = &config_builder.threads {
        writer.write(&[b"\n\n===> ",config_manager::THREADS.as_bytes(),b"\n"].concat());
        writer.write((threads.producers.to_string() + " " + &threads.consumers.to_string()).as_bytes());
    }
    if let Some(braces_as_code) = &config_builder.braces_as_code {
        writer.write(&[b"\n\n===> ",config_manager::BRACES_AS_CODE.as_bytes(),b"\n"].concat());
        writer.write(if *braces_as_code {b"yes"} else {b"no"});
    }
    if let Some(should_search_in_dotted) = &config_builder.should_search_in_dotted {
        writer.write(&[b"\n\n===> ",config_manager::SEARCH_IN_DOTTED.as_bytes(),b"\n"].concat());
        writer.write(if *should_search_in_dotted {b"yes"} else {b"no"});
    }
    if let Some(should_show_faulty_files) = &config_builder.should_show_faulty_files {
        writer.write(&[b"\n\n===> ",config_manager::SHOW_FAULTY_FILES.as_bytes(),b"\n"].concat());
        writer.write(if *should_show_faulty_files {b"yes"} else {b"no"});
    }
    if let Some(no_keywords) = &config_builder.no_keywords {
        writer.write(&[b"\n\n===> ",config_manager::NO_KEYWORDS.as_bytes(),b"\n"].concat());
        writer.write(if *no_keywords {b"yes"} else {b"no"});
    }
    if let Some(no_visual) = &config_builder.no_visual {
        writer.write(&[b"\n\n===> ",config_manager::NO_VISUAL.as_bytes(),b"\n"].concat());
        writer.write(if *no_visual {b"yes"} else {b"no"});
    }
    if let Some(log) = &config_builder.log {
        writer.write(&[b"\n\n===> ",config_manager::LOG.as_bytes(),b"\n"].concat());
        if log.should_log {
            if let Some(name) = &log.name {
                writer.write(name.as_bytes());
            } else {
                writer.write(b"yes");
            }
        } else {
            writer.write(b"no");
        }
    }
    if let Some(compare_level) = &config_builder.compare_level {
        writer.write(&[b"\n\n===> ",config_manager::COMPRARE_LEVEL.as_bytes(),b"\n"].concat());
        writer.write(compare_level.to_string().as_bytes());
    }

    writer.write(b"\n");    
    writer.flush();

    Ok(())
}

pub fn write_default_config(contents: String) -> Result<(), io::Error> {
    let file_path = PERSISTENT_APP_PATHS.config_dir.clone() + DEFAULT_CONFIG_NAME;
    let mut writer = BufWriter::new(std::fs::OpenOptions::new().write(true).create(true).open(file_path)?);
    writer.write_all(contents.as_bytes());

    Ok(())
}


// ----------------------------------- Log handling ------------------------------------------

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
    writer.write(format!("===>{}\n",config.log.name.clone().unwrap_or(String::new())).as_bytes());
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
    writer.write(format!("    Total Size: {}\n",final_stats.bytes_size.to_string()).as_bytes());
    writer.write(format!("        Average Size: {}\n\n\n",final_stats.bytes_average_size.to_string()).as_bytes());
    writer.write(b"--------------------------------------------------------------------------------------------\n\n\n");
}


fn read_bool_value_from_file(reader: &mut BufReader<File>, mut buf: &mut String) -> Option<bool> {
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

//Keep parsing new lines as relevant, until an empty one appears.
fn read_lines_from_file_to_vec(reader: &mut BufReader<File>, mut buf: &mut String, parser_func: fn(&str) -> Vec<String>) -> Vec<String> {
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


impl LanguageDirParseInfo {
    pub fn new(language_map: HashMap<String, Language>, faulty_files: Vec<String>, non_existant_languages: Vec<String>) -> Self {
        LanguageDirParseInfo {
            language_map,
            faulty_files,
            non_existant_languages
        }
    }
}

impl Formatted for LanguageDirParseError {
    fn formatted(&self) -> ColoredString {
        match self {
            Self::NoFilesFound => "Error: No language files found in directory.".red(),
            Self::NoFilesFormattedProperly => "Error: No language file is formatted properly, so none could be parsed.".red(),
        }
    }
}

impl Formatted for ConfigFileParseError {
    fn formatted(&self) -> ColoredString {
        match self {
            Self::FileNotFound(x) => format!("'{}' config file not found, defaults will be used.", x).yellow(),
            Self::IOError => "Unexpected IO error while reading, defaults will be used".yellow()
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
    use crate::*;

    #[test]
    fn test_save_config_file_and_then_parse_it() -> std::io::Result<()> {
        let command = format!("./ --exclude a,b,c.txt,d.txt, --braces-as-code --threads 1 1");
        let config_builder = config_manager::create_config_builder_from_args(&command).unwrap();

        let test_config_dir = Some(LOCAL_APP_PATHS.test_config_dir.clone());
        io_handler::save_existing_commands_from_config_builder_to_file(test_config_dir, "auto-generated", &config_builder);

        let options = io_handler::parse_config_file(Some("auto-generated"), Some(LOCAL_APP_PATHS.test_config_dir.clone())).unwrap();
        assert_eq!(config_builder.dirs, options.dirs);
        assert_eq!(config_builder.exclude_dirs, options.exclude_dirs);
        assert_eq!(config_builder.threads, options.threads);
        assert_eq!(config_builder.braces_as_code, options.braces_as_code);
        assert_eq!(config_builder.should_show_faulty_files, options.should_show_faulty_files);
        assert_eq!(config_builder.should_search_in_dotted, options.should_search_in_dotted);
        assert_eq!(config_builder.no_visual, options.no_visual);

        Ok(())
    }

    #[test]
    fn test_read_config_file() -> std::io::Result<()> {
        let mut config = Configuration::new(vec!["C:/Some/Path/a".to_owned(),"C:/Some/Path/b".to_owned(),"C:/Some/Path/c".to_owned(),"C:/Some/Path/d".to_owned()]);
        config
            .set_exclude_dirs(vec!["a".to_owned(), "b".to_owned(), "c.txt".to_owned(), "d.txt".to_owned()])
            .set_threads(1,1)
            .set_braces_as_code(true);


        let options = io_handler::parse_config_file(Some("test"), Some(LOCAL_APP_PATHS.test_config_dir.clone())).unwrap();
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
    fn test_parse_supported_languages_to_map() {
        let (lang_map, faulty_files) = io_handler::parse_supported_languages_to_map(
                &(LOCAL_APP_PATHS.test_dir.clone() + "languages/")).unwrap();
        assert!(lang_map.len() == 2);
        assert!(faulty_files.len() == 1);
    }
}
