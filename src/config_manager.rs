use std::{path::Path, process};

use colored::Colorize;

use crate::{io_handler::{self, ParseConfigFileError, PersistentOptions},utils};

// command flags
pub const PATH               :&str   = "path";
pub const EXCLUDE            :&str   = "exclude";
pub const EXTENSIONS         :&str   = "extensions";
pub const THREADS            :&str   = "threads";
pub const BRACES_AS_CODE     :&str   = "braces-as-code";
pub const SEARCH_IN_DOTTED   :&str   = "search-in-dotted";
pub const SHOW_FAULTY_FILES  :&str   = "show-faulty-files";
pub const SAVE               :&str   = "save";
pub const LOAD               :&str   = "load";

// default config values
const DEF_BRACES_AS_CODE    : bool    = false;
const DEF_SEARCH_IN_DOTTED  : bool    = false;
const DEF_SHOW_FAULTY_FILES : bool    = false;
const DEF_THREADS           : usize   = 4;


#[derive(Debug,PartialEq,Clone)]
pub struct Configuration {
    pub path: String,
    pub exclude_dirs: Vec<String>,
    pub extensions_of_interest: Vec<String>,
    pub threads: usize,
    pub braces_as_code: bool,
    pub should_search_in_dotted: bool,
    pub should_show_faulty_files: bool
}

#[derive(Debug)]
pub enum ArgParsingError {
    NoArgsProvided,
    MissingTargetPath,
    InvalidPath,
    DoublePath,
    UnrecognisedParameter(String),
    IncorrectCommandArgs(String),
}

pub fn read_args_cmd() -> Result<Configuration,ArgParsingError> {
    let args  = std::env::args().skip(1).collect::<Vec<String>>();
    if args.is_empty() {return Err(ArgParsingError::NoArgsProvided)}
    let line = args.join(" ");
    let line = line.trim();
    if line.is_empty() {return Err(ArgParsingError::NoArgsProvided)}

    if line == "--help" {print_help_message_and_exit()}

    create_config_from_args(line)
}

pub fn read_args_console() -> Result<Configuration,ArgParsingError> {
    let mut line = String::with_capacity(30);
    std::io::stdin().read_line(&mut line).unwrap();
    line = line.trim().to_owned();
    if line.is_empty() {
        Err(ArgParsingError::NoArgsProvided)
    } else {
        if line == "--help" {print_help_message_and_exit()} 

        create_config_from_args(&line)
    }
}

fn print_help_message_and_exit() {
    println!("\nHELP\n");
    utils::wait_for_input();
    process::exit(0);
}

fn create_config_from_args(line: &str) -> Result<Configuration, ArgParsingError> {
    let mut path = None;
    let options = line.split("--").collect::<Vec<_>>();

    if !line.trim().starts_with("--") {
        let path_str = options[0].trim().to_owned();
        if !is_valid_path(&path_str) {
            return Err(ArgParsingError::InvalidPath);
        }

        path = Some(path_str);
    }

    let mut custom_config = None;
    let (mut exclude_dirs, mut extensions_of_interest, mut threads, mut braces_as_code,
         mut search_in_dotted, mut show_faulty_files, mut config_name_for_save) = (None, None, None, None, None, None, None);
    for command in options.into_iter().skip(1) {
        if command.starts_with(EXCLUDE) {
            let vec = command.split(' ').skip(1).filter_map(|x| get_if_not_empty(x.trim())).collect::<Vec<_>>();
            if vec.is_empty() {
                return Err(ArgParsingError::IncorrectCommandArgs("--".to_owned() + EXCLUDE));
            }
            exclude_dirs = Some(vec);
        } else if command.starts_with(EXTENSIONS){
            let vec = command.split(' ').skip(1).filter_map(|x| get_if_not_empty(remove_dot_prefix(x.trim()))).collect::<Vec<_>>();
            if vec.is_empty() {
                return Err(ArgParsingError::IncorrectCommandArgs("--".to_owned() + EXTENSIONS));
            }    
            extensions_of_interest = Some(vec);
        } else if command.starts_with(THREADS) {
            match parse_usize_command(command) {
                Ok(x) => threads = x,
                Err(_) => return Err(ArgParsingError::IncorrectCommandArgs("--".to_owned() + THREADS))
            }
        } else if let Some(config_name) = command.strip_prefix(LOAD) {
            match parse_load_command(config_name) {
                Ok(x) => custom_config = x,
                Err(_) => return Err(ArgParsingError::IncorrectCommandArgs("--".to_owned() + LOAD))
            }
        } else if let Some(name) = command.strip_prefix(SAVE) {
            let name = name.trim();
            if name.is_empty() {
                return Err(ArgParsingError::IncorrectCommandArgs("--".to_owned() + SAVE))
            }
            config_name_for_save = Some(name.to_owned());
        } else if let Some(path_str) = command.strip_prefix(PATH) {
            if path.is_some() {
                return Err(ArgParsingError::DoublePath);
            }
            let path_str = path_str.trim();
            if path_str.is_empty() || !is_valid_path(path_str) {
                return Err(ArgParsingError::IncorrectCommandArgs("--".to_owned() + PATH ))
            }
            path = Some(path_str.to_owned());
        } else if command.starts_with(BRACES_AS_CODE) {
            if has_any_args(command) {
                return Err(ArgParsingError::IncorrectCommandArgs("--".to_owned() + BRACES_AS_CODE))
            }
            braces_as_code = Some(true)
        } else if command.starts_with(SEARCH_IN_DOTTED) {
            if has_any_args(command) {
                return Err(ArgParsingError::IncorrectCommandArgs("--".to_owned() + SEARCH_IN_DOTTED))
            }
            search_in_dotted = Some(true)
        } else if command.starts_with(SHOW_FAULTY_FILES) {
            if has_any_args(command) {
                return Err(ArgParsingError::IncorrectCommandArgs("--".to_owned() + SHOW_FAULTY_FILES))
            }
            show_faulty_files = Some(true);
        } else {
            return Err(ArgParsingError::UnrecognisedParameter(command.to_owned()));
        }
    }

    let args_builder = combine_specified_config_options(
            custom_config, path, exclude_dirs, extensions_of_interest, threads, braces_as_code, search_in_dotted, show_faulty_files);

    if args_builder.path.is_none() {
        return Err(ArgParsingError::MissingTargetPath);
    }


    let config = args_builder.build();
    if let Some(x) = config_name_for_save {
        match io_handler::save_config_to_file(&x, &config) {
            Err(_) => println!("\n{}","Error while trying to save config.".yellow()),
            Ok(_) => println!("\nConfiguration '{}' saved successfully.",x)
        }
    }

    Ok(args_builder.build())
}

fn has_any_args(command: &str) -> bool {
    command.split(' ').skip(1).filter_map(|x| get_if_not_empty(x.trim())).count() != 0
}

fn parse_usize_command(command: &str) -> Result<Option<usize>,()> {
    let vec = command.split(' ').skip(1).filter_map(|x| get_if_not_empty(x.trim())).collect::<Vec<_>>();
    if vec.len() != 1 {
        return Err(());
    }
    if let Ok(x) = vec[0].parse::<usize>() {
        if x >= 1 && x <= 8 {
            Ok(Some(x))
        } else {
            Err(())
        }
    } else {
        Err(())
    }
}

fn parse_load_command(config_name: &str) -> Result<Option<PersistentOptions>,()> {
    let config_name = config_name.trim();
    if config_name.is_empty() {
        return Err(());
    }
    let result = match io_handler::parse_config_file(Some(config_name)) {
        Ok(x) => {
            if x.1 {
                println!("{}",format!("Formatting problems detected in config file '{}', some default values may be used.",config_name).yellow());
            }
            Some(x.0)
        },
        Err(x) => {
            println!("\n{}",x.formatted());
            None
        }
    };

    Ok(result)
}

fn combine_specified_config_options(custom_config: Option<PersistentOptions>, path: Option<String>, exclude_dirs: Option<Vec<String>>,
        extensions_of_interest: Option<Vec<String>>, threads: Option<usize>, braces_as_code: Option<bool>, search_in_dotted: Option<bool>,
        show_faulty_files: Option<bool>) 
-> ConfigurationBuilder 
{
    let mut args_builder = ConfigurationBuilder::new(
            path, exclude_dirs, extensions_of_interest, threads, braces_as_code, search_in_dotted, show_faulty_files);
    if let Some(x) = custom_config {
        args_builder.add_missing_fields(x);
    }
    if args_builder.has_missing_fields() {
        let default_config = io_handler::parse_config_file(None);
        if let Ok(x) = default_config {
            if x.1 {
                println!("{}","Formatting problems detected in the default config file, some default values may be used.".yellow());
            }
            args_builder.add_missing_fields(x.0);
        }
    }
    args_builder
}

fn get_distinct_arguments(line: String) -> Vec<String> {
    if let Some(dirs_pos) = line.find("--exclude") {
        let parts = line.split_at(dirs_pos);
        let mut args = vec![parts.0.trim().to_owned()];
        for dir in parts.1.split_whitespace() {
            if dir != "--dirs"{
                args.push(dir.to_owned());
            }
        }
        args
    } else {
        vec![line.trim().to_owned()]
    }
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

fn is_valid_path(str: &str) -> bool {
    let path_str = str.trim();

    let p = Path::new(path_str);
    p.is_dir() || p.is_file()
}


#[derive(Debug)]
struct ConfigurationBuilder {
    pub path: Option<String>,
    pub exclude_dirs: Option<Vec<String>>,
    pub extensions_of_interest: Option<Vec<String>>,
    pub threads: Option<usize>,
    pub braces_as_code: Option<bool>,
    pub should_search_in_dotted: Option<bool>,
    pub should_show_faulty_files: Option<bool>
}

impl ConfigurationBuilder {
    pub fn new(path: Option<String>, exclude_dirs: Option<Vec<String>>, extensions_of_interest: Option<Vec<String>>,
            threads: Option<usize>, braces_as_code: Option<bool>, should_search_in_dotted: Option<bool>,
            should_show_faulty_files: Option<bool>) 
    -> ConfigurationBuilder 
    {
        ConfigurationBuilder {
            path,
            exclude_dirs,
            extensions_of_interest,
            threads,
            braces_as_code,
            should_search_in_dotted,
            should_show_faulty_files
        }
    }

    pub fn add_missing_fields(&mut self, config: PersistentOptions) -> &mut ConfigurationBuilder {
        if self.path.is_none() {self.path = config.path};
        if self.exclude_dirs.is_none() {self.exclude_dirs = config.exclude_dirs};
        if self.extensions_of_interest.is_none() {self.extensions_of_interest = config.extensions_of_interest};
        if self.threads.is_none() {self.threads = config.threads};
        if self.braces_as_code.is_none() {self.braces_as_code = config.braces_as_code};
        if self.should_search_in_dotted.is_none() {self.should_search_in_dotted = config.should_search_in_dotted};
        if self.should_show_faulty_files.is_none() {self.should_show_faulty_files = config.should_show_faulty_files};
        self
    }

    pub fn has_missing_fields(&self) -> bool {
        self.exclude_dirs.is_none() || self.extensions_of_interest.is_none() ||
        self.threads.is_none() || self.braces_as_code.is_none() || self.should_search_in_dotted.is_none()
    } 

    pub fn build(&self) -> Configuration {
        Configuration {
            path : self.path.clone().unwrap(),
            exclude_dirs: (self.exclude_dirs).clone().unwrap_or_default(),
            extensions_of_interest: (self.extensions_of_interest).clone().unwrap_or_default(),
            threads: self.threads.unwrap_or(DEF_THREADS),
            braces_as_code: self.braces_as_code.unwrap_or(DEF_BRACES_AS_CODE),
            should_search_in_dotted: self.should_search_in_dotted.unwrap_or(DEF_SEARCH_IN_DOTTED),
            should_show_faulty_files: self.should_show_faulty_files.unwrap_or(DEF_SHOW_FAULTY_FILES)
        }
    }
}

impl Configuration {
    pub fn new(path: String) -> Configuration {
        Configuration {
            path,
            exclude_dirs: Vec::new(),
            extensions_of_interest: Vec::new(),
            threads: 4,
            braces_as_code: false,
            should_search_in_dotted: false,
            should_show_faulty_files: false
        }
    }

    pub fn set_exclude_dirs(&mut self, exclude_dirs: Vec<String>) -> & mut Configuration {
        self.exclude_dirs = exclude_dirs;
        self
    }

    pub fn set_extensions_of_interest(&mut self, extensions_of_interest: Vec<String>) -> & mut Configuration {
        self.extensions_of_interest = extensions_of_interest;
        self
    }

    pub fn set_threads(&mut self, threads: usize) -> &mut Configuration {
        self.threads = threads;
        self
    }

    pub fn set_braces_as_code(&mut self, braces_as_code: bool) -> &mut Configuration {
        self.braces_as_code = braces_as_code;
        self
    }

    pub fn set_should_search_in_dotted(&mut self, should_search_in_dotted: bool) -> &mut Configuration {
        self.should_search_in_dotted = should_search_in_dotted;
        self
    }
}

impl ArgParsingError {
    pub fn formatted(&self) -> String {
        match self {
            Self::NoArgsProvided => "No arguments provided.".red().to_string(),
            Self::MissingTargetPath => "The target directory (--path) is not specified.".red().to_string(),
            Self::InvalidPath => "Path provided is not a valid directory or file.".red().to_string(),
            Self::DoublePath => "Path already provided as first argument but --path command also found.".red().to_string(),
            Self::UnrecognisedParameter(p) => format!("--{} is not recognised as a command.",p).red().to_string(),
            Self::IncorrectCommandArgs(p) => format!("Incorrect arguments provided for the command '--{}'.",p).red().to_string()
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_arg_parsing() {
        assert!(create_config_from_args("path --exclude   --extensions .java    .cs").is_err());
        assert!(create_config_from_args("path --exclude--extensions .java    .cs").is_err());
        assert!(create_config_from_args("path --extensions .java .cs --exclude").is_err());
        assert!(create_config_from_args("path --something").is_err());
        assert!(create_config_from_args("path --threads 3 --something --exclude e").is_err());

        assert_eq!(Configuration::new("./".to_owned()),create_config_from_args("./").unwrap());

        assert_eq!(*Configuration::new("./".to_owned())
            .set_exclude_dirs(vec!["ex1".to_owned(),"ex2".to_owned()])
            .set_extensions_of_interest(vec!["java".to_owned(),"cs".to_owned()])
            .set_threads(4),
            create_config_from_args("./ --threads 4 --exclude ex1 ex2 --extensions java cs").unwrap()
        );

        assert_eq!(*Configuration::new("./".to_owned())
            .set_exclude_dirs(vec!["ex2".to_owned()]) 
            .set_extensions_of_interest(vec!["java".to_owned(),"cs".to_owned()]),
            create_config_from_args("./   --exclude ex2  --extensions java    cs").unwrap()
        );

        assert_eq!(*Configuration::new("./".to_owned())
            .set_exclude_dirs(vec!["ex2".to_owned()]) 
            .set_extensions_of_interest(vec!["java".to_owned(),"cs".to_owned()]),
            create_config_from_args("./   --exclude ex2  --extensions .java    .cs").unwrap()
        );
    }
}

