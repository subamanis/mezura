use std::{path::Path};

use colored::{ColoredString, Colorize};

use crate::{Formatted, io_handler, message_printer, utils};

// Application version, to be displayed at startup and with --help command
pub const VERSION_ID : &str = "v1.0.0"; 

// command flags
pub const DIRS               :&str   = "dirs";
pub const EXCLUDE            :&str   = "exclude";
pub const LANGUAGES          :&str   = "languages";
pub const THREADS            :&str   = "threads";
pub const BRACES_AS_CODE     :&str   = "braces-as-code";
pub const SEARCH_IN_DOTTED   :&str   = "search-in-dotted";
pub const SHOW_FAULTY_FILES  :&str   = "show-faulty-files";
pub const NO_KEYWORDS        :&str   = "no-keywords";
pub const NO_VISUAL          :&str   = "no-visual";
pub const LOG                :&str   = "log";
pub const COMPRARE_LEVEL     :&str   = "compare";
pub const SAVE               :&str   = "save";
pub const LOAD               :&str   = "load";
pub const HELP               :&str   = "help";
pub const CHANGELOG          :&str   = "changelog";
pub const SHOW_LANGUAGES     :&str   = "show-languages";
pub const SHOW_CONFIGS       :&str   = "show-configs";

pub const MAX_PRODUCERS_VALUE : usize = 4;
pub const MIN_PRODUCERS_VALUE : usize = 1;
pub const MAX_CONSUMERS_VALUE : usize = 12;
pub const MIN_CONSUMERS_VALUE : usize = 1;
pub const MIN_COMPARE_LEVEL   : usize = 0;
pub const MAX_COMPARE_LEVEL   : usize = 10;

// default config values
const DEF_BRACES_AS_CODE    : bool    = false;
const DEF_SEARCH_IN_DOTTED  : bool    = false;
const DEF_SHOW_FAULTY_FILES : bool    = false;
const DEF_NO_VISUAL         : bool    = false;
const DEF_NO_KEYWORDS       : bool    = false;
const DEF_COMPARE_LEVEL     : usize   = 1;


#[derive(Debug,PartialEq,Clone)]
pub struct Configuration {
    pub version: &'static str,
    pub dirs: Vec<String>,
    pub exclude_dirs: Vec<String>,
    pub languages_of_interest: Vec<String>,
    pub threads: Threads,
    pub braces_as_code: bool,
    pub should_search_in_dotted: bool,
    pub should_show_faulty_files: bool,
    pub no_keywords: bool,
    pub no_visual: bool,
    pub log: LogOption,
    pub compare_level: usize,
    pub config_name_to_save: Option<String>,
    pub config_name_to_load: Option<String>
}

#[derive(Debug,PartialEq,Clone)]
pub struct LogOption {
    pub should_log: bool,
    pub name: Option<String>
}

#[derive(Debug,PartialEq,Clone)]
pub struct Threads {
    pub producers: usize,
    pub consumers: usize
}

#[derive(Debug, PartialEq)]
pub enum ArgParsingError {
    NoArgsProvided,
    UnparsableWorkingDir,
    MissingTargetDirs,
    InvalidPath(String),
    InvalidPathInConfig(String,String),
    DoublePath,
    UnrecognisedCommand(String),
    IncorrectCommandArgs(String),
    UnexpectedCommandArgs(String),
    NonExistantConfig(String)
}

// Empty line argument is not supposed to be allowed, since this check is being performed in main
pub fn create_config_from_args(line: &str) -> Result<Configuration, ArgParsingError> {
    match create_config_builder_from_args(line) {
        Ok(config_builder) => Ok(config_builder.build()),
        Err(x) => Err(x)
    }
}

pub fn create_config_builder_from_args(line: &str) -> Result<ConfigurationBuilder, ArgParsingError> {
    let mut dirs = None;
    let mut options = line.split("--");

    if line.trim().starts_with("--") {
        //ignoring the empty first element that is caused by splitting
        options.next();
    } else {
        match parse_dirs(options.next().unwrap()) {
            Ok(x) => {
                if !x.is_empty() {
                    dirs = Some(x);
                }
            },
            Err(x) => {
                return Err(x);
            }
        }
    }

    let mut custom_config = None;
    let (mut exclude_dirs, mut languages_of_interest, mut threads, mut braces_as_code,
         mut search_in_dotted, mut show_faulty_files, mut config_name_to_save, mut no_visual,
         mut log, mut compare_level, mut config_name_to_load, mut no_keywords) 
         = (None, None, None, None, None, None, None, None, None, None, None, None);
    for command in options {
         if let Some(_dirs) = command.strip_prefix(DIRS) {
            if dirs.is_some() {
                return Err(ArgParsingError::DoublePath);
            }

            let parse_result = parse_dirs(_dirs);
            if let Ok(x) = parse_result {
                if x.is_empty() {
                    message_printer::print_help_message_for_command(DIRS);
                    return Err(ArgParsingError::IncorrectCommandArgs(DIRS.to_owned()));
                }
                dirs = Some(x)
            } else {
                return Err(parse_result.err().unwrap());
            }
        } else if let Some(excluded) = command.strip_prefix(EXCLUDE) {
            let vec = utils::parse_paths_to_vec(excluded);
            if vec.is_empty() {
                message_printer::print_help_message_for_command(EXCLUDE);
                return Err(ArgParsingError::IncorrectCommandArgs(EXCLUDE.to_owned()));
            }
            exclude_dirs = Some(vec);
        } else if let Some(langs) = command.strip_prefix(LANGUAGES) {
            let vec = utils::parse_languages_to_vec(langs);
            if vec.is_empty() {
                message_printer::print_help_message_for_command(LANGUAGES);
                return Err(ArgParsingError::IncorrectCommandArgs(LANGUAGES.to_owned()));
            }    
            languages_of_interest = Some(vec);
        } else if let Some(value) = command.strip_prefix(THREADS) {
            let threads_values = utils::parse_two_usize_values(value,
                    MIN_PRODUCERS_VALUE, MAX_PRODUCERS_VALUE, MIN_CONSUMERS_VALUE, MAX_CONSUMERS_VALUE);
            if let Some(_threads) = threads_values {
                threads = Some(Threads::from(_threads));
            } else {
                message_printer::print_help_message_for_command(THREADS);
                return Err(ArgParsingError::IncorrectCommandArgs(THREADS.to_owned()))
            }
        } else if command.starts_with(BRACES_AS_CODE) {
            if has_any_args(command) {
                message_printer::print_help_message_for_command(BRACES_AS_CODE);
                return Err(ArgParsingError::UnexpectedCommandArgs(BRACES_AS_CODE.to_owned()))
            }
            braces_as_code = Some(true)
        } else if command.starts_with(SEARCH_IN_DOTTED) {
            if has_any_args(command) {
                message_printer::print_help_message_for_command(SEARCH_IN_DOTTED);
                return Err(ArgParsingError::UnexpectedCommandArgs(SEARCH_IN_DOTTED.to_owned()))
            }
            search_in_dotted = Some(true)
        } else if command.starts_with(SHOW_FAULTY_FILES) {
            if has_any_args(command) {
                message_printer::print_help_message_for_command(SHOW_FAULTY_FILES);
                return Err(ArgParsingError::UnexpectedCommandArgs(SHOW_FAULTY_FILES.to_owned()))
            }
            show_faulty_files = Some(true);
        } else if command.starts_with(NO_KEYWORDS) {
            if has_any_args(command) {
                message_printer::print_help_message_for_command(NO_VISUAL);
                return Err(ArgParsingError::UnexpectedCommandArgs(NO_KEYWORDS.to_owned()))
            }
            no_keywords = Some(true);
        } else if command.starts_with(NO_VISUAL) {
            if has_any_args(command) {
                message_printer::print_help_message_for_command(NO_VISUAL);
                return Err(ArgParsingError::UnexpectedCommandArgs(NO_VISUAL.to_owned()))
            }
            no_visual = Some(true);
        } else if let Some(value) = command.strip_prefix(LOG) {
            let value = value.trim();
            if value.is_empty() {
                log = Some(LogOption::new(None));
            } else {
                log = Some(LogOption::new(Some(value.to_owned())));
            }
        } else if let Some(value) = command.strip_prefix(COMPRARE_LEVEL) {
            let compare_num = utils::parse_usize_value(value, MIN_COMPARE_LEVEL, MAX_COMPARE_LEVEL);
            if compare_num.is_none() {
                message_printer::print_help_message_for_command(COMPRARE_LEVEL);
                return Err(ArgParsingError::IncorrectCommandArgs(COMPRARE_LEVEL.to_owned()))
            } else {
                compare_level = compare_num
            }
        } else if let Some(config_name) = command.strip_prefix(LOAD) {
            let config_name = config_name.trim();
            if config_name.is_empty() {
                message_printer::print_help_message_for_command(LOAD);
                return Err(ArgParsingError::IncorrectCommandArgs(LOAD.to_owned()));
            }

            if let Ok(options) = io_handler::parse_config_file(Some(config_name), None) {
                if let Some(dirs) = &options.dirs {
                    for dir in dirs.iter() {
                        if !utils::is_valid_path(dir) {
                            return Err(ArgParsingError::InvalidPathInConfig(dir.to_owned(), config_name.to_owned()));
                        }
                    }
                }
                custom_config = Some(options);
                config_name_to_load = Some(config_name.to_owned());
            } else {
                return Err(ArgParsingError::NonExistantConfig(config_name.to_owned()))
            }
        } else if let Some(name) = command.strip_prefix(SAVE) {
            let name = name.trim();
            if name.is_empty() {
                message_printer::print_help_message_for_command(SAVE);
                return Err(ArgParsingError::IncorrectCommandArgs(SAVE.to_owned()))
            }
            config_name_to_save = Some(name.to_owned());
        } else {
            return Err(ArgParsingError::UnrecognisedCommand(command.to_owned()));
        }
    }

    print_warnings_for_commands_that_need_a_loaded_configuration(&config_name_to_save, &config_name_to_load, &log, &compare_level);
    
    let mut config_builder = ConfigurationBuilder::new(dirs, exclude_dirs, languages_of_interest, threads, braces_as_code,
        search_in_dotted, show_faulty_files, no_keywords, no_visual, log, compare_level,
        config_name_to_save, config_name_to_load);

    if let Some(x) = custom_config {
        config_builder.add_missing_fields(x);
    }

    if let Some(name) = &config_builder.config_name_to_save {
        if config_builder.dirs.is_none() {
            match parse_working_dir_as_target_dir() {
                Ok(x) => {config_builder.dirs = Some(x)},
                Err(x) => {return Err(x)}
            }
        }

        match io_handler::save_existing_commands_from_config_builder_to_file(None, name, &config_builder) {
            Err(_) => println!("\n{}","Error while trying to save config.".yellow()),
            Ok(_) => println!("\nConfiguration '{}' saved successfully.",name)
        }
    }

    if config_builder.has_missing_fields() {
        let default_config = io_handler::parse_config_file(None, None);
        if let Ok(x) = default_config {
            config_builder.add_missing_fields(x);
        }
    }

    if config_builder.dirs.is_none() {
        match parse_working_dir_as_target_dir() {
            Ok(x) => {config_builder.dirs = Some(x)},
            Err(x) => {return Err(x)}
        }
    }

    Ok(config_builder)
}


fn print_warnings_for_commands_that_need_a_loaded_configuration(config_name_to_save: &Option<String>, config_name_to_load: &Option<String>,
        log: &Option<LogOption>, compare_level: &Option<usize>) 
{
    if config_name_to_load.is_none() {
        if let Some(log) = log {
            if config_name_to_save.is_none() && log.should_log {
                println!("\n{}","'--log' command will be ignored, since no config file was specified.".yellow());
            }
        }

        if compare_level.is_some() {
            println!("\n{}","'--compare' command will be ignored, since no config file was specified for loading.".yellow());
        }
    }
}

fn has_any_args(command: &str) -> bool {
    command.split(' ').skip(1).filter_map(|x| utils::get_trimmed_if_not_empty(x)).count() != 0
}

fn parse_dirs(s: &str) -> Result<Vec<String>, ArgParsingError> {
    let mut _dirs = utils::parse_paths_to_vec(s);

    for dir in _dirs.iter_mut() {
        let trimmed_dir =  dir.trim();
        if !utils::is_valid_path(dir) {
            return Err(ArgParsingError::InvalidPath(trimmed_dir.to_owned()))
        } else {
            *dir = convert_to_absolute(trimmed_dir);
        }
    }

    Ok(_dirs)
}

fn parse_working_dir_as_target_dir() -> Result<Vec<String>, ArgParsingError> {
    if let Ok(path_buf) = std::env::current_dir() {
        if let Some(path_str) = path_buf.to_str() {
            if let Ok(x) = parse_dirs(path_str) {
                return Ok(x);
            }
        }
    }

    Err(ArgParsingError::UnparsableWorkingDir)
}

// The "canonicalize" function from the std that this function uses, (at least on window) seems to put the weird prefix
// "\\?\" before the path and it also puts forward slashes that we want to convert for compatibility.  
fn convert_to_absolute(s: &str) -> String {
    let p = Path::new(s);
    if p.is_absolute() {
        return s.replace("\\", "/");
    }

    if let Ok(buf) = std::fs::canonicalize(p) {
        let str_path = buf.to_str().unwrap();
        str_path.strip_prefix(r"\\?\").unwrap_or(str_path).replace("\\", "/")
    } else {
        s.replace("\\", "/")
    }
}


#[derive(Debug, PartialEq)]
pub struct ConfigurationBuilder {
    pub dirs:                     Option<Vec<String>>,
    pub exclude_dirs:             Option<Vec<String>>,
    pub languages_of_interest:    Option<Vec<String>>,
    pub threads:                  Option<Threads>,
    pub braces_as_code:           Option<bool>,
    pub should_search_in_dotted:  Option<bool>,
    pub should_show_faulty_files: Option<bool>,
    pub no_keywords:              Option<bool>,
    pub no_visual:                Option<bool>,
    pub log:                      Option<LogOption>,
    pub compare_level:            Option<usize>,
    pub config_name_to_save:      Option<String>,
    pub config_name_to_load:      Option<String>
}

impl ConfigurationBuilder {
    pub fn new(dirs: Option<Vec<String>>, exclude_dirs: Option<Vec<String>>, languages_of_interest: Option<Vec<String>>, threads: Option<Threads>,
             braces_as_code: Option<bool>, should_search_in_dotted: Option<bool>, should_show_faulty_files: Option<bool>, no_keywords: Option<bool>,
             no_visual: Option<bool>, log: Option<LogOption>, compare_level: Option<usize>, config_name_to_save: Option<String>, config_name_to_load: Option<String>) 
    -> ConfigurationBuilder 
    {
        ConfigurationBuilder {
            dirs,
            exclude_dirs,
            languages_of_interest,
            threads,
            braces_as_code,
            should_search_in_dotted,
            should_show_faulty_files,
            no_keywords,
            no_visual,
            log,
            compare_level,
            config_name_to_save,
            config_name_to_load
        }
    }

    pub fn add_missing_fields(&mut self, config: Self) -> &mut Self {
        if self.dirs.is_none() {self.dirs = config.dirs};
        if self.exclude_dirs.is_none() {self.exclude_dirs = config.exclude_dirs};
        if self.languages_of_interest.is_none() {self.languages_of_interest = config.languages_of_interest};
        if self.threads.is_none() {self.threads = config.threads};
        if self.braces_as_code.is_none() {self.braces_as_code = config.braces_as_code};
        if self.should_search_in_dotted.is_none() {self.should_search_in_dotted = config.should_search_in_dotted};
        if self.should_show_faulty_files.is_none() {self.should_show_faulty_files = config.should_show_faulty_files};
        if self.no_keywords.is_none() {self.no_keywords = config.no_keywords};
        if self.no_visual.is_none() {self.no_visual = config.no_visual};
        if self.compare_level.is_none() {self.compare_level = config.compare_level};
        if self.log.is_none() {self.log = config.log};
        self
    }

    pub fn has_missing_fields(&self) -> bool {
        self.exclude_dirs.is_none() || self.languages_of_interest.is_none() ||
        self.threads.is_none() || self.braces_as_code.is_none() || self.should_search_in_dotted.is_none() ||
        self.should_show_faulty_files.is_none() || self.no_visual.is_none() || self.log.is_none() || self.compare_level.is_none()
    } 

    pub fn build(&self) -> Configuration {
        Configuration {
            version: VERSION_ID,
            dirs: self.dirs.clone().unwrap(),
            exclude_dirs: (self.exclude_dirs).clone().unwrap_or_default(),
            languages_of_interest: (self.languages_of_interest).clone().unwrap_or_default(),
            threads: self.threads.clone().unwrap_or_else(Threads::default),
            braces_as_code: self.braces_as_code.unwrap_or(DEF_BRACES_AS_CODE),
            should_search_in_dotted: self.should_search_in_dotted.unwrap_or(DEF_SEARCH_IN_DOTTED),
            should_show_faulty_files: self.should_show_faulty_files.unwrap_or(DEF_SHOW_FAULTY_FILES),
            no_keywords: self.no_keywords.unwrap_or(DEF_NO_KEYWORDS),
            no_visual: self.no_visual.unwrap_or(DEF_NO_VISUAL),
            log: self.log.clone().unwrap_or_else(LogOption::default),
            compare_level: self.compare_level.unwrap_or(DEF_COMPARE_LEVEL),
            config_name_to_save: self.config_name_to_save.clone(),
            config_name_to_load: self.config_name_to_load.clone()
        }
    }
}

impl Configuration {
    pub fn new(dirs: Vec<String>) -> Self {
        Configuration {
            version: VERSION_ID,
            dirs,
            exclude_dirs: Vec::new(),
            languages_of_interest: Vec::new(),
            threads: Threads::default(),
            braces_as_code: DEF_BRACES_AS_CODE,
            should_search_in_dotted: DEF_SEARCH_IN_DOTTED,
            should_show_faulty_files: DEF_SHOW_FAULTY_FILES,
            no_keywords: DEF_NO_KEYWORDS,
            no_visual: DEF_NO_VISUAL,
            log: LogOption::default(),
            compare_level: DEF_COMPARE_LEVEL,
            config_name_to_save: None,
            config_name_to_load: None
        }
    }

    //Setters used mainly in tests, for the ability to chain many config changes

    pub fn set_config_names_to_save_and_load(&mut self, to_save: Option<String>, to_load: Option<String>) -> &mut Self {
        self.config_name_to_save = to_save;
        self.config_name_to_load = to_load;
        self
    }

    pub fn set_exclude_dirs(&mut self, exclude_dirs: Vec<String>) -> &mut Self {
        self.exclude_dirs = exclude_dirs;
        self
    }

    pub fn set_languages_of_interest(&mut self, languages_of_interest: Vec<String>) -> &mut Self {
        self.languages_of_interest = languages_of_interest;
        self
    }

    pub fn set_threads(&mut self, producers: usize, consumers: usize) -> &mut Self {
        self.threads = Threads::new(producers, consumers);
        self
    }

    pub fn set_braces_as_code(&mut self, braces_as_code: bool) -> &mut Self {
        self.braces_as_code = braces_as_code;
        self
    }

    pub fn set_should_search_in_dotted(&mut self, should_search_in_dotted: bool) -> &mut Self {
        self.should_search_in_dotted = should_search_in_dotted;
        self
    }
    
    pub fn set_should_show_faulty_files(&mut self, should_show_faulty_files: bool) -> &mut Self {
        self.should_show_faulty_files = should_show_faulty_files;
        self
    }

    pub fn set_should_not_count_keywords(&mut self, should_count_keywords: bool) -> &mut Self {
        self.no_keywords = should_count_keywords;
        self
    }

    pub fn set_should_enable_visuals(&mut self, should_enable_visuals: bool) -> &mut Self {
        self.no_visual = should_enable_visuals;
        self
    }

    pub fn set_log_option(&mut self, log: LogOption) -> &mut Self {
        self.log = log;
        self
    }
}

impl Threads {
    pub fn new(producers: usize, consumers: usize) -> Self {
        Threads {
            producers,
            consumers
        }
    }

    pub fn from(threads: (usize,usize)) -> Self {
        Threads {
            producers: threads.0,
            consumers: threads.1
        }
    }

    pub fn default() -> Self {
        let threads = num_cpus::get();
        // We may actually use one more thread than the available ones, it seems to help a bit
        if threads <= 4 {
            Threads {
                producers: 2,
                consumers: 3
            }
        } else if threads <= 8 {
            Threads {
                producers: 3,
                consumers: 6
            }
        } else {
            Threads {
                producers: 3,
                consumers: 8
            }
        }
    }
}

impl LogOption {
    pub fn new(log_name: Option<String>) -> Self {
        LogOption {
            should_log: true,
            name: log_name,
        }
    }

    pub fn default() -> Self {
        LogOption {
            should_log: false,
            name: None
        }
    }
}

impl Formatted for ArgParsingError {
    fn formatted(&self) -> ColoredString {
        match self {
            Self::NoArgsProvided => "No arguments provided.".red(),
            Self::UnparsableWorkingDir => "The current working dir could not be parsed as target dir, try inputing it manually.".red(),
            Self::MissingTargetDirs => "The target directories (--dirs) are not specified.".red(),
            Self::InvalidPath(p) => format!("Path provided is not a valid directory or file:\n'{}'.",p).red(),
            Self::InvalidPathInConfig(dir,name) => format!("Specified path '{}', in config '{}', doesn't exist anymore.",dir,name).red(),
            Self::DoublePath => "Directories already provided as first argument, but --dirs command also found.".red(),
            Self::UnrecognisedCommand(p) => format!("--{} is not recognised as a command.",p).red(),
            Self::IncorrectCommandArgs(p) => format!("Incorrect arguments provided for the command '--{}'.",p).red(),
            Self::UnexpectedCommandArgs(p) => format!("Command '--{}' does not expect any arguments.",p).red(),
            Self::NonExistantConfig(p) => format!("Configuration '{}' does not exist.",p).red()
        }
    }
}


#[cfg(test)]
mod tests {
    use std::ops::Add;

    use crate::PERSISTENT_APP_PATHS;

    use super::*;

    #[test]
    fn test_cmd_arg_parsing() {
        assert_eq!(Err(ArgParsingError::InvalidPath("random".to_owned())), create_config_from_args("random"));
        assert_eq!(Err(ArgParsingError::InvalidPath("./ random".to_owned())), create_config_from_args("./ random"));
        assert_eq!(Err(ArgParsingError::InvalidPath("./ -show-faulty-files".to_owned())), create_config_from_args("--dirs ./ -show-faulty-files"));
        assert_eq!(Err(ArgParsingError::UnrecognisedCommand("random".to_owned())), create_config_from_args("--random"));
        assert_eq!(Err(ArgParsingError::UnrecognisedCommand("random".to_owned())), create_config_from_args("--dirs ./ --random"));
        assert_eq!(Err(ArgParsingError::DoublePath), create_config_from_args("./ --dirs ./"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("dirs".to_owned())), create_config_from_args("--dirs"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("dirs".to_owned())), create_config_from_args("--dirs   "));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned())), create_config_from_args("./ --threads"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned())), create_config_from_args("./ --threads 5 10"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned())), create_config_from_args("./ --threads 2 13"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned())), create_config_from_args("./ --threads 9"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned())), create_config_from_args("./ --threads A"));
        assert_eq!(Err(ArgParsingError::UnexpectedCommandArgs("show-faulty-files".to_owned())), create_config_from_args("./ --threads 1 1 --show-faulty-files 1"));
        assert_eq!(Err(ArgParsingError::UnexpectedCommandArgs("show-faulty-files".to_owned())), create_config_from_args("./ --threads 1 1 --show-faulty-files a"));
        assert_eq!(Err(ArgParsingError::UnexpectedCommandArgs("search-in-dotted".to_owned())), create_config_from_args("./ --threads 1 1 --search-in-dotted a"));
        assert_eq!(Err(ArgParsingError::UnexpectedCommandArgs("no-visual".to_owned())), create_config_from_args("./ --no-visual a"));
        assert_eq!(Err(ArgParsingError::UnexpectedCommandArgs("braces-as-code".to_owned())), create_config_from_args("./ --braces-as-code a"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("exclude".to_owned())), create_config_from_args("./ --exclude"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("exclude".to_owned())), create_config_from_args("./ --exclude   --threads 4"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("load".to_owned())), create_config_from_args("./ --load"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("load".to_owned())), create_config_from_args("./ --load   "));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("save".to_owned())), create_config_from_args("./ --save"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("save".to_owned())), create_config_from_args("./ --save   "));

        assert_ne!(Configuration::new(vec![convert_to_absolute("../")]), create_config_from_args(std::env::current_dir().unwrap().to_str().unwrap()).unwrap());
        assert_eq!(Configuration::new(vec![convert_to_absolute("./")]), create_config_from_args(std::env::current_dir().unwrap().to_str().unwrap()).unwrap());

        assert_eq!(Configuration::new(vec![convert_to_absolute("./")]), create_config_from_args("./").unwrap());
        assert_eq!(Configuration::new(vec![convert_to_absolute("./")]), create_config_from_args("--dirs ./").unwrap());
        assert_eq!(*Configuration::new(vec![convert_to_absolute("./")]).set_threads(1,1), create_config_from_args("./ --threads 1 1").unwrap());
        assert_eq!(*Configuration::new(vec![convert_to_absolute("./")]).set_threads(1,1), create_config_from_args("./ --threads   1   1 ").unwrap());
        assert_eq!(*Configuration::new(vec![convert_to_absolute("./")]).set_threads(1,1).set_braces_as_code(true),
                create_config_from_args("./ --threads 1 1 --braces-as-code").unwrap());
        assert_eq!(*Configuration::new(vec![convert_to_absolute("./")]).set_should_search_in_dotted(true),
                create_config_from_args("./ --search-in-dotted").unwrap());
        assert_eq!(*Configuration::new(vec![convert_to_absolute("./")]).set_should_enable_visuals(true),
                create_config_from_args("./ --no-visual").unwrap());
        assert_eq!(*Configuration::new(vec![convert_to_absolute("./")]).set_should_show_faulty_files(true),
                create_config_from_args("./ --show-faulty-files").unwrap());
        assert_eq!(*Configuration::new(vec![convert_to_absolute("./")]).set_exclude_dirs(vec!["a".to_owned(),"b".to_owned(),"c".to_owned()]),
                create_config_from_args("./ --exclude a,b ,  c ").unwrap());
        assert_eq!(*Configuration::new(vec![convert_to_absolute("./")]).set_exclude_dirs(vec!["a/path".to_owned(),"b/path".to_owned()]),
                create_config_from_args("./ --exclude \"a\\path\", \"b\\path\"").unwrap());
        assert_eq!(*Configuration::new(vec![convert_to_absolute("./")]).set_languages_of_interest(vec!["a".to_owned(),"b".to_owned(),"c".to_owned()]),
                create_config_from_args("./ --languages a,b,c").unwrap());
        assert_eq!(*Configuration::new(vec![convert_to_absolute("./")]).set_languages_of_interest(vec!["a".to_owned()]),
                create_config_from_args("./ --languages a, ").unwrap());
        assert_eq!(*Configuration::new(vec![convert_to_absolute("./")]).set_log_option(LogOption::new(Some("this is a test".to_owned()))),
                create_config_from_args("./ --log   this is a test ").unwrap());
        assert_eq!(*Configuration::new(vec![convert_to_absolute("./")]).set_log_option(LogOption::new(None)),
                create_config_from_args("./ --log  ").unwrap());
    }

    #[test]
    fn test_has_any_args() {
        assert!(has_any_args("cmnd a"));
        assert!(has_any_args("cmnd    a"));
        assert!(has_any_args("cmnd    a   "));
        assert!(has_any_args("cmnd a a"));

        assert!(!has_any_args("cmnd"));
        assert!(!has_any_args("cmnd    "));
    }

    #[test]
    fn test_absolute_conversion() {
        let path = "./";
        let abs = convert_to_absolute(path);
        assert!(Path::new(path).is_relative());
        assert!(Path::new(&abs).is_absolute());

        let path = "./src";
        let abs = convert_to_absolute(path);
        assert!(Path::new(path).is_relative());
        assert!(Path::new(&abs).is_absolute());

        let path = "./src/../src";
        let abs = convert_to_absolute(path);
        assert!(Path::new(path).is_relative());
        assert!(Path::new(&abs).is_absolute());

        let path = "src";
        let abs = convert_to_absolute(path);
        assert!(Path::new(path).is_relative());
        assert!(Path::new(&abs).is_absolute());

        let path = "src/utils.rs";
        let abs = convert_to_absolute(path);
        assert!(Path::new(path).is_relative());
        assert!(Path::new(&abs).is_absolute());
    }

    #[test]
    fn test_parse_dirs() {
        assert!(parse_dirs("a").is_err());
        assert!(parse_dirs("a b c").is_err());

        assert_eq!(vec![convert_to_absolute("./"), convert_to_absolute(".././")], parse_dirs("./, .././").unwrap());
        assert_eq!(vec![convert_to_absolute("./"), convert_to_absolute(".././")], parse_dirs("./, \".././\"").unwrap());
    }
    
    #[test]
    fn test_save_load_configs() {
        let test_file_path = &PERSISTENT_APP_PATHS.config_dir.clone().add("/test000.txt");
        assert!(!Path::new(test_file_path).exists());

        let mut saved_config = create_config_builder_from_args("--threads 1 5 --languages lang1, lang2 --save test000").unwrap();
        assert!(Path::new(test_file_path).exists());
        assert_eq!(saved_config.dirs.clone().unwrap()[0], convert_to_absolute("./"));
        assert_eq!(saved_config.threads.clone().unwrap(), Threads::new(1, 5));
        assert_eq!(saved_config.languages_of_interest.clone().unwrap(), vec!["lang1", "lang2"]);

        let mut loaded_config = create_config_builder_from_args("--load test000").unwrap();
        saved_config.config_name_to_save = None;
        loaded_config.config_name_to_load = None;
        assert_eq!(saved_config, loaded_config);
        
        loaded_config = create_config_builder_from_args("--load test000 --threads 1 4 --dirs ./").unwrap();
        assert_eq!(saved_config.dirs, loaded_config.dirs);
        assert_ne!(saved_config.threads, loaded_config.threads);

        saved_config = create_config_builder_from_args("--load test000 --threads 1 4 --dirs ./ --save test000").unwrap();
        saved_config.config_name_to_save = None;
        assert_eq!(saved_config, loaded_config);

        std::fs::remove_file(test_file_path).unwrap();
    }
}

