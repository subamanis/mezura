use std::{path::{Path, PathBuf}, process};

use colored::Colorize;

use crate::{io_handler::{self, ParseConfigFileError},utils};

// command flags
pub const DIRS               :&str   = "dirs";
pub const EXCLUDE            :&str   = "exclude";
pub const LANGUAGES          :&str   = "languages";
pub const THREADS            :&str   = "threads";
pub const BRACES_AS_CODE     :&str   = "braces-as-code";
pub const SEARCH_IN_DOTTED   :&str   = "search-in-dotted";
pub const SHOW_FAULTY_FILES  :&str   = "show-faulty-files";
pub const NO_VISUAL          :&str   = "no-visual";
pub const COMPRARE_LEVEL     :&str   = "compare";
pub const SAVE               :&str   = "save";
pub const LOAD               :&str   = "load";

pub const MAX_THREADS_VALUE : usize = 8;
pub const MIN_THREADS_VALUE : usize = 1;
pub const MIN_COMPARE_LEVEL : usize = 0;
pub const MAX_COMPARE_LEVEL : usize = 10;

// default config values
const DEF_BRACES_AS_CODE    : bool    = false;
const DEF_SEARCH_IN_DOTTED  : bool    = false;
const DEF_SHOW_FAULTY_FILES : bool    = false;
const DEF_NO_VISUAL         : bool    = false;
const DEF_THREADS           : usize   = 4;
const DEF_COMPARE_LEVEL     : usize   = 1;


#[derive(Debug,PartialEq,Clone)]
pub struct Configuration {
    pub dirs: Vec<String>,
    pub exclude_dirs: Vec<String>,
    pub languages_of_interest: Vec<String>,
    pub threads: usize,
    pub braces_as_code: bool,
    pub should_search_in_dotted: bool,
    pub should_show_faulty_files: bool,
    pub no_visual: bool,
    pub compare_level: usize,
    pub config_name_to_save: Option<String>,
    pub config_name_to_load: Option<String>
}

#[derive(Debug, PartialEq)]
pub enum ArgParsingError {
    NoArgsProvided,
    MissingTargetDirs,
    InvalidPath(String),
    InvalidPathInConfig(String,String),
    DoublePath,
    UnrecognisedCommand(String),
    IncorrectCommandArgs(String),
}

pub fn read_args_cmd() -> Result<Configuration,ArgParsingError> {
    let args  = std::env::args().skip(1).collect::<Vec<String>>();
    if args.is_empty() {return Err(ArgParsingError::NoArgsProvided)}
    let line = args.join(" ");

    create_config_from_args(&line)
}

pub fn read_args_console() -> Result<Configuration,ArgParsingError> {
    let mut line = String::with_capacity(30);
    std::io::stdin().read_line(&mut line).unwrap();

    create_config_from_args(&line)
}


pub fn create_config_from_args(line: &str) -> Result<Configuration, ArgParsingError> {
    let line = line.trim();
    if line.is_empty() {
        return Err(ArgParsingError::NoArgsProvided)
    }

    if line == "--help" {
        print_help_message_and_exit()
    }

    let mut dirs = None;
    let options = line.split("--").collect::<Vec<_>>();

    if !line.trim().starts_with("--") {
        let parse_result = parse_dirs(options[0]);
        if let Ok(x) = parse_result {
            if x.is_empty() {
                return Err(ArgParsingError::IncorrectCommandArgs(DIRS.to_owned()));
            }
            dirs = Some(x)
        } else {
            return Err(parse_result.err().unwrap());
        }
    }

    let mut custom_config = None;
    let (mut exclude_dirs, mut languages_of_interest, mut threads, mut braces_as_code,
         mut search_in_dotted, mut show_faulty_files, mut config_name_to_save, mut no_visual,
         mut compare_level, mut config_name_to_load) 
         = (None, None, None, None, None, None, None, None, None, None);
    for command in options.into_iter().skip(1) {
         if let Some(_dirs) = command.strip_prefix(DIRS) {
            if dirs.is_some() {
                return Err(ArgParsingError::DoublePath);
            }

            let parse_result = parse_dirs(_dirs);
            if let Ok(x) = parse_result {
                if x.is_empty() {
                    return Err(ArgParsingError::IncorrectCommandArgs(DIRS.to_owned()));
                }
                dirs = Some(x)
            } else {
                return Err(parse_result.err().unwrap());
            }
        } else if let Some(excluded) = command.strip_prefix(EXCLUDE) {
            let vec = utils::parse_paths_to_vec(excluded);
            if vec.is_empty() {
                return Err(ArgParsingError::IncorrectCommandArgs(EXCLUDE.to_owned()));
            }
            exclude_dirs = Some(vec);
        } else if let Some(langs) = command.strip_prefix(LANGUAGES) {
            let vec = utils::parse_languages_to_vec(langs);
            if vec.is_empty() {
                return Err(ArgParsingError::IncorrectCommandArgs(LANGUAGES.to_owned()));
            }    
            languages_of_interest = Some(vec);
        } else if let Some(value) = command.strip_prefix(THREADS) {
            let threads_value = utils::parse_usize_value(value, MIN_THREADS_VALUE, MAX_THREADS_VALUE);
            if threads_value.is_none() {
                return Err(ArgParsingError::IncorrectCommandArgs(THREADS.to_owned()))
            } else {
                threads = threads_value
            }
        } else if let Some(config_name) = command.strip_prefix(LOAD) {
            let config_name = config_name.trim();
            if config_name.is_empty() {
                return Err(ArgParsingError::IncorrectCommandArgs(LOAD.to_owned()));
            }

            if let Some(options) = parse_load_command(config_name) {
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
                return Err(ArgParsingError::IncorrectCommandArgs(LOAD.to_owned()));
            }
        } else if let Some(name) = command.strip_prefix(SAVE) {
            let name = name.trim();
            if name.is_empty() {
                return Err(ArgParsingError::IncorrectCommandArgs(SAVE.to_owned()))
            }
            config_name_to_save = Some(name.to_owned());
        } else if command.starts_with(BRACES_AS_CODE) {
            if has_any_args(command) {
                return Err(ArgParsingError::IncorrectCommandArgs(BRACES_AS_CODE.to_owned()))
            }
            braces_as_code = Some(true)
        } else if command.starts_with(SEARCH_IN_DOTTED) {
            if has_any_args(command) {
                return Err(ArgParsingError::IncorrectCommandArgs(SEARCH_IN_DOTTED.to_owned()))
            }
            search_in_dotted = Some(true)
        } else if command.starts_with(SHOW_FAULTY_FILES) {
            if has_any_args(command) {
                return Err(ArgParsingError::IncorrectCommandArgs(SHOW_FAULTY_FILES.to_owned()))
            }
            show_faulty_files = Some(true);
        } else if command.starts_with(NO_VISUAL) {
            if has_any_args(command) {
                return Err(ArgParsingError::IncorrectCommandArgs(NO_VISUAL.to_owned()))
            }
            no_visual = Some(true);
        } else if let Some(value) = command.strip_prefix(COMPRARE_LEVEL) {
            let compare_num = utils::parse_usize_value(value, MIN_COMPARE_LEVEL, MAX_COMPARE_LEVEL);
            if compare_num.is_none() {
                return Err(ArgParsingError::IncorrectCommandArgs(COMPRARE_LEVEL.to_owned()))
            } else {
                compare_level = compare_num
            }
        } else {
            return Err(ArgParsingError::UnrecognisedCommand(command.to_owned()));
        }
    }

    let args_builder = combine_specified_config_options(custom_config, dirs, exclude_dirs, languages_of_interest,
         threads, braces_as_code, search_in_dotted, show_faulty_files, no_visual, compare_level);

    if args_builder.dirs.is_none() {
        return Err(ArgParsingError::MissingTargetDirs);
    }

    let mut config = args_builder.build();
    config.set_config_names_to_save_and_load(config_name_to_save.clone(), config_name_to_load);

    if let Some(x) = config_name_to_save {
        match io_handler::save_config_to_file(&x, &config) {
            Err(_) => println!("\n{}","Error while trying to save config.".yellow()),
            Ok(_) => println!("\nConfiguration '{}' saved successfully.",x)
        }
    }

    Ok(config)
}

fn has_any_args(command: &str) -> bool {
    command.split(' ').skip(1).filter_map(|x| utils::get_if_not_empty(x.trim())).count() != 0
}

fn parse_load_command(config_name: &str) -> Option<ConfigurationBuilder> {
    let config_name = config_name.trim();
    if config_name.is_empty() {
        return None;
    }
    match io_handler::parse_config_file(Some(config_name)) {
        Ok(x) => {
            Some(x)
        },
        Err(x) => {
            println!("\n{}",x.formatted());
            None
        }
    }
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

// The "canonicalize" function from the std that this function uses, seems to put the weird prefix
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

// Fill the missing arguments that the user didn't specify when he run the program with 
// 1) The arguments saved in the given config (if he gave any)
// 2) The default config file
// 3) Default values
// In this order of importance.
fn combine_specified_config_options(custom_config: Option<ConfigurationBuilder>, dirs: Option<Vec<String>>, exclude_dirs: Option<Vec<String>>,
        languages_of_interest: Option<Vec<String>>, threads: Option<usize>, braces_as_code: Option<bool>, search_in_dotted: Option<bool>,
        show_faulty_files: Option<bool>, no_visual: Option<bool>, compare_level: Option<usize>) 
-> ConfigurationBuilder 
{
    let mut args_builder = ConfigurationBuilder::new(dirs, exclude_dirs, languages_of_interest, threads,
            braces_as_code, search_in_dotted, show_faulty_files, no_visual, compare_level);
    if let Some(x) = custom_config {
        args_builder.add_missing_fields(x);
    }
    if args_builder.has_missing_fields() {
        let default_config = io_handler::parse_config_file(None);
        if let Ok(x) = default_config {
            args_builder.add_missing_fields(x);
        }
    }
    args_builder
}

// This needs to maintained along the README, I am not sure how else to make a help message without duplication
fn print_help_message_and_exit() {
    println!("
    Format of arguments: <path_here> --optional_command1 --optional_commandN

    COMMANDS:

    --dirs
        The paths to the directories or files, seperated by commas if more than 1, in this form: '--dirs <path1, path2>'
        They can either be surrounded by quotes: \"path\" or not, even if the paths have whitespace.

        The target directories can also be given implicitly (in which case this command is not needed) with 2 ways:
        1) as the first arguments of the program directly
        2) if they are present in a configuration file (see '--save' and '--load' commands).

    --exclude 
        1..n arguments separated by commas, can be a folder name, a file name (including extension), 
        or a full path to a folder or file. The paths can be surrounded by quotes or not, even if they have whitespace.

        The program will ignore these dirs.
    
    --languages 
        1..n arguments separated by commas, case-insensitive

        The given language names must exist in any of the files in the 'data/languages/' dir as the
        parameter of the field 'Language'.

        Only the languages specified here will be taken into account for the stats.

    --threads
        1 argument: a number between 1 and 8. Default: 4 

        This reprisents the number of the consumer threads that parse files,
        there is also always one producer thread that is traversing the given dir.

        Increasing the number of consumers can help performance a bit in a situation where
        there are a lot of big files, concentrated in a shallow directory structure.
        
    --braces-as-code
        No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
        or anything else to disable. Default: disabled

        Specifies whether lines that only contain braces ( {{ or }} ), should be considered as code lines or not.

        The default behaviour is to not count them as code, since it is silly for code of the same content
        and substance to be counted differently, according to the programer's code style.
        This helps to keep the stats clean when using code lines as a complexity and productivity metric.

    --search-in-dotted
        No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
        or anything else to disable. Default: disabled

        Specifies whether the program should traverse directories that are prefixed with a dot,
        like .vscode or .git.

    --show-faulty-files
        No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
        or anything else to disable. Default: disabled

        Sometimes it happens that an error occurs when trying to parse a file, either while opening it,
        or while reading it's contents. The default behavior when this happens is to count all of
        the faulty files and display their count.

        This flag specifies that their path, along with information about the exact error is displayed too.
        The most common reason for this error is if a file contains non UTF-8 characters. 

    --no-visual
        No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
        or anything else to disable. Default: disabled

        Disables the colors in the \"overview\" section of the results, and disables the visualization with 
        the vertical lines that reprisent the percentages.

    --save
        One argument as the file name (whitespace allowed, without an extension, case-insensitive)

        Doing so, will run the program and also create a .txt configuration file,
        inside 'data/config/' with the specified name, that can later be loaded with the --load command.

    --load
        One argument as the file name (whitespace allowed, without an extension, case-insensitive)

        Assosiated with the '--save' command, this command is used to load the flags of 
        an existing configuration file from the 'data/config/' directory. 

        You can combine the '--load' and '--save' commands to modify a configuration file.
    ");
    
    utils::wait_for_input();
    process::exit(0);
}


#[derive(Debug)]
pub struct ConfigurationBuilder {
    pub dirs:                     Option<Vec<String>>,
    pub exclude_dirs:             Option<Vec<String>>,
    pub languages_of_interest:    Option<Vec<String>>,
    pub threads:                  Option<usize>,
    pub braces_as_code:           Option<bool>,
    pub should_search_in_dotted:  Option<bool>,
    pub should_show_faulty_files: Option<bool>,
    pub no_visual:                Option<bool>,
    pub compare_level:            Option<usize>,
}

impl ConfigurationBuilder {
    pub fn new(dirs: Option<Vec<String>>, exclude_dirs: Option<Vec<String>>, languages_of_interest: Option<Vec<String>>,
            threads: Option<usize>, braces_as_code: Option<bool>, should_search_in_dotted: Option<bool>,
            should_show_faulty_files: Option<bool>, no_visual: Option<bool>, compare_level: Option<usize>) 
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
            no_visual,
            compare_level
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
        if self.no_visual.is_none() {self.no_visual = config.no_visual};
        self
    }

    pub fn has_missing_fields(&self) -> bool {
        self.exclude_dirs.is_none() || self.languages_of_interest.is_none() ||
        self.threads.is_none() || self.braces_as_code.is_none() || self.should_search_in_dotted.is_none()
    } 

    pub fn build(&self) -> Configuration {
        Configuration {
            dirs : self.dirs.clone().unwrap(),
            exclude_dirs: (self.exclude_dirs).clone().unwrap_or_default(),
            languages_of_interest: (self.languages_of_interest).clone().unwrap_or_default(),
            threads: self.threads.unwrap_or(DEF_THREADS),
            braces_as_code: self.braces_as_code.unwrap_or(DEF_BRACES_AS_CODE),
            should_search_in_dotted: self.should_search_in_dotted.unwrap_or(DEF_SEARCH_IN_DOTTED),
            should_show_faulty_files: self.should_show_faulty_files.unwrap_or(DEF_SHOW_FAULTY_FILES),
            no_visual: self.no_visual.unwrap_or(DEF_NO_VISUAL),
            compare_level: self.compare_level.unwrap_or(DEF_COMPARE_LEVEL),
            config_name_to_save: None,
            config_name_to_load: None
        }
    }
}

impl Configuration {
    pub fn new(dirs: Vec<String>) -> Self {
        Configuration {
            dirs,
            exclude_dirs: Vec::new(),
            languages_of_interest: Vec::new(),
            threads: DEF_THREADS,
            braces_as_code: DEF_BRACES_AS_CODE,
            should_search_in_dotted: DEF_SEARCH_IN_DOTTED,
            should_show_faulty_files: DEF_SHOW_FAULTY_FILES,
            no_visual: DEF_NO_VISUAL,
            compare_level: DEF_COMPARE_LEVEL,
            config_name_to_save: None,
            config_name_to_load: None
        }
    }

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

    pub fn set_threads(&mut self, threads: usize) -> &mut Self {
        self.threads = threads;
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

    pub fn set_should_enable_visuals(&mut self, should_enable_visuals: bool) -> &mut Self {
        self.no_visual = should_enable_visuals;
        self
    }
}

impl ArgParsingError {
    pub fn formatted(&self) -> String {
        match self {
            Self::NoArgsProvided => "No arguments provided.".red().to_string(),
            Self::MissingTargetDirs => "The target directories (--dirs) are not specified.".red().to_string(),
            Self::InvalidPath(p) => format!("Path provided is not a valid directory or file:\n'{}'.",p).red().to_string(),
            Self::InvalidPathInConfig(dir,name) => format!("Specified path '{}', in config '{}', doesn't exist anymore.",dir,name).red().to_string(),
            Self::DoublePath => "Directories already provided as first argument, but --dirs command also found.".red().to_string(),
            Self::UnrecognisedCommand(p) => format!("--{} is not recognised as a command.",p).red().to_string(),
            Self::IncorrectCommandArgs(p) => format!("Incorrect arguments provided for the command '--{}'. Type '--help'",p).red().to_string()
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_arg_parsing() {
        assert_eq!(Err(ArgParsingError::NoArgsProvided), create_config_from_args(""));
        assert_eq!(Err(ArgParsingError::NoArgsProvided), create_config_from_args("   "));
        assert_eq!(Err(ArgParsingError::InvalidPath("random".to_owned())), create_config_from_args("random"));
        assert_eq!(Err(ArgParsingError::InvalidPath("./ random".to_owned())), create_config_from_args("./ random"));
        assert_eq!(Err(ArgParsingError::InvalidPath("./ -show-faulty-files".to_owned())), create_config_from_args("--dirs ./ -show-faulty-files"));
        assert_eq!(Err(ArgParsingError::UnrecognisedCommand("random".to_owned())), create_config_from_args("--random"));
        assert_eq!(Err(ArgParsingError::UnrecognisedCommand("random".to_owned())), create_config_from_args("--dirs ./ --random"));
        assert_eq!(Err(ArgParsingError::DoublePath), create_config_from_args("./ --dirs ./"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("dirs".to_owned())), create_config_from_args("--dirs"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("dirs".to_owned())), create_config_from_args("--dirs   "));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned())), create_config_from_args("./ --threads"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned())), create_config_from_args("./ --threads 0"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned())), create_config_from_args("./ --threads 9"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned())), create_config_from_args("./ --threads 2 2"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned())), create_config_from_args("./ --threads A"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("show-faulty-files".to_owned())), create_config_from_args("./ --threads 1 --show-faulty-files 1"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("show-faulty-files".to_owned())), create_config_from_args("./ --threads 1 --show-faulty-files a"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("search-in-dotted".to_owned())), create_config_from_args("./ --threads 1 --search-in-dotted a"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("no-visual".to_owned())), create_config_from_args("./ --threads 1 --no-visual a"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("braces-as-code".to_owned())), create_config_from_args("./ --threads 1 --braces-as-code a"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("exclude".to_owned())), create_config_from_args("./ --exclude"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("exclude".to_owned())), create_config_from_args("./ --exclude   --threads 4"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("load".to_owned())), create_config_from_args("./ --load"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("load".to_owned())), create_config_from_args("./ --load   "));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("save".to_owned())), create_config_from_args("./ --save"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("save".to_owned())), create_config_from_args("./ --save   "));

        assert_eq!(Configuration::new(vec![convert_to_absolute("./")]), create_config_from_args("./").unwrap());
        assert_eq!(Configuration::new(vec![convert_to_absolute("./")]), create_config_from_args("--dirs ./").unwrap());
        assert_eq!(*Configuration::new(vec![convert_to_absolute("./")]).set_threads(1), create_config_from_args("./ --threads 1").unwrap());
        assert_eq!(*Configuration::new(vec![convert_to_absolute("./")]).set_threads(1), create_config_from_args("./ --threads   1   ").unwrap());
        assert_eq!(*Configuration::new(vec![convert_to_absolute("./")]).set_threads(1).set_braces_as_code(true),
                create_config_from_args("./ --threads 1 --braces-as-code").unwrap());
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
    fn test_parse_threads_command() {
        assert_eq!(Some(1), utils::parse_usize_value("1", MIN_THREADS_VALUE, MAX_THREADS_VALUE));
        assert_eq!(Some(1), utils::parse_usize_value("   1", MIN_THREADS_VALUE, MAX_THREADS_VALUE));
        assert_eq!(Some(1), utils::parse_usize_value("   1   ", MIN_THREADS_VALUE, MAX_THREADS_VALUE));

        assert_eq!(None, utils::parse_usize_value("1 3 3", MIN_THREADS_VALUE, MAX_THREADS_VALUE));
        assert_eq!(None, utils::parse_usize_value("-1 0", MIN_THREADS_VALUE, MAX_THREADS_VALUE));
        assert_eq!(None, utils::parse_usize_value("cmnd", MIN_THREADS_VALUE, MAX_THREADS_VALUE));
        assert_eq!(None, utils::parse_usize_value("   ", MIN_THREADS_VALUE, MAX_THREADS_VALUE));
        assert_eq!(None, utils::parse_usize_value("A", MIN_THREADS_VALUE, MAX_THREADS_VALUE));
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
}

