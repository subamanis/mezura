use std::{path::{Path, PathBuf}, process};

use colored::Colorize;

use crate::{io_handler::{self, ParseConfigFileError, PersistentOptions},utils};

// command flags
pub const PATH               :&str   = "path";
pub const EXCLUDE            :&str   = "exclude";
pub const LANGUAGES          :&str   = "languages";
pub const THREADS            :&str   = "threads";
pub const BRACES_AS_CODE     :&str   = "braces-as-code";
pub const SEARCH_IN_DOTTED   :&str   = "search-in-dotted";
pub const SHOW_FAULTY_FILES  :&str   = "show-faulty-files";
pub const NO_VISUAL          :&str   = "no-visual";
pub const SAVE               :&str   = "save";
pub const LOAD               :&str   = "load";

// default config values
const DEF_BRACES_AS_CODE    : bool    = false;
const DEF_SEARCH_IN_DOTTED  : bool    = false;
const DEF_SHOW_FAULTY_FILES : bool    = false;
const DEF_NO_VISUAL         : bool    = false;
const DEF_THREADS           : usize   = 4;


#[derive(Debug,PartialEq,Clone)]
pub struct Configuration {
    pub path: String,
    pub exclude_dirs: Vec<String>,
    pub languages_of_interest: Vec<String>,
    pub threads: usize,
    pub braces_as_code: bool,
    pub should_search_in_dotted: bool,
    pub should_show_faulty_files: bool,
    pub no_visual: bool
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

fn create_config_from_args(line: &str) -> Result<Configuration, ArgParsingError> {
    let mut path = None;
    let options = line.split("--").collect::<Vec<_>>();

    if !line.trim().starts_with("--") {
        let path_str = options[0].trim().to_owned();
        if !is_valid_path(&path_str) {
            return Err(ArgParsingError::InvalidPath);
        }

        path = Some(convert_to_absolute(&path_str));
    }

    let mut custom_config = None;
    let (mut exclude_dirs, mut languages_of_interest, mut threads, mut braces_as_code,
         mut search_in_dotted, mut show_faulty_files, mut config_name_for_save, mut no_visual) 
         = (None, None, None, None, None, None, None, None);
    for command in options.into_iter().skip(1) {
        if command.starts_with(EXCLUDE) {
            let vec = command.split(' ').skip(1).filter_map(|x| get_if_not_empty(&x.trim().replace("\\", "/"))).collect::<Vec<_>>();
            if vec.is_empty() {
                return Err(ArgParsingError::IncorrectCommandArgs(EXCLUDE.to_owned()));
            }
            exclude_dirs = Some(vec);
        } else if command.starts_with(LANGUAGES){
            let vec = command.split(' ').skip(1).filter_map(|x| get_if_not_empty(&remove_dot_prefix(x.trim()).to_lowercase())).collect::<Vec<_>>();
            if vec.is_empty() {
                return Err(ArgParsingError::IncorrectCommandArgs(LANGUAGES.to_owned()));
            }    
            languages_of_interest = Some(vec);
        } else if command.starts_with(THREADS) {
            match parse_usize_command(command) {
                Ok(x) => threads = x,
                Err(_) => return Err(ArgParsingError::IncorrectCommandArgs(THREADS.to_owned()))
            }
        } else if let Some(config_name) = command.strip_prefix(LOAD) {
            match parse_load_command(config_name) {
                Ok(x) => custom_config = x,
                Err(_) => return Err(ArgParsingError::IncorrectCommandArgs(LOAD.to_owned()))
            }
        } else if let Some(name) = command.strip_prefix(SAVE) {
            let name = name.trim();
            if name.is_empty() {
                return Err(ArgParsingError::IncorrectCommandArgs(SAVE.to_owned()))
            }
            config_name_for_save = Some(name.to_owned());
        } else if let Some(path_str) = command.strip_prefix(PATH) {
            if path.is_some() {
                return Err(ArgParsingError::DoublePath);
            }
            let path_str = path_str.trim();
            if path_str.is_empty() || !is_valid_path(path_str) {
                return Err(ArgParsingError::IncorrectCommandArgs(PATH.to_owned()))
            }

            path = Some(convert_to_absolute(path_str));
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
        } else {
            return Err(ArgParsingError::UnrecognisedParameter(command.to_owned()));
        }
    }

    let args_builder = combine_specified_config_options(custom_config, path, exclude_dirs,
            languages_of_interest, threads, braces_as_code, search_in_dotted, show_faulty_files, no_visual);

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
        languages_of_interest: Option<Vec<String>>, threads: Option<usize>, braces_as_code: Option<bool>, search_in_dotted: Option<bool>,
        show_faulty_files: Option<bool>, no_visual: Option<bool>) 
-> ConfigurationBuilder 
{
    let mut args_builder = ConfigurationBuilder::new(path, exclude_dirs, languages_of_interest,
            threads, braces_as_code, search_in_dotted, show_faulty_files, no_visual);
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

fn is_valid_path(s: &str) -> bool {
    let path_str = s.trim();

    let p = Path::new(path_str);
    p.is_dir() || p.is_file()
}

fn convert_to_absolute(s: &str) -> String {
    let path_str = s.trim();

    let p = Path::new(path_str);
    if p.is_absolute() {
        return path_str.to_owned();
    }

    if let Ok(buf) = std::fs::canonicalize(p) {
        return buf.to_str().unwrap().to_owned();
    } else {
        return path_str.to_owned();
    }
}


fn print_help_message_and_exit() {
    println!("
    Format of arguments: <path_here> --optional_command1 --optional_commandN

    COMMANDS:

    --path
        The path to a directory or a single file, in this form: '--path <path_here>'
        It can either be surrounded by quotes: \"path\" or not, even if the path has whitespace.

        The path can also be given implicitly (in which case this command is not needed) with 2 ways:
        1) as the first argument of the program directly
        2) if it is present in a configuration file (see '--save' and '--load' commands).

    --exclude 
        1..n arguments separated with whitespace, can be a folder name or a file name (including extension).

        The program will ignore these dirs.
    
    --languages 
        1..n arguments separated with whitespace, case-insensitive

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

        If we plan to run the program many times for a project, it can be bothersome to specify,
        all the flags every time, especially if they contain a lot of exclude dirs for example.
        That's why you can specify all the flags once, and add this command to save them
        as a configuration file. If you specify a '--path' command, it will save the absolute
        version of the specified path, otherwise, no path will be specified.

        Doing so, will run the program and also create a .txt configuration file,
        inside 'data/config/' with the specified name, that can later be loaded with the --load command.

    --load
        One argument as the file name (whitespace allowed, without an extension, case-insensitive)
        
        Assosiated with the '--save' command, this comman is used to load the flags of 
        an existing configuration file from the 'data/config/' directory. 

        There is already a configuration file named 'default.txt' that contains the default of the program,
        and gets automatically loaded with each program run. You can modify it to add common flags
        so you dont have to create the same configurations for different projects.

        If you provide in the cmd a flag that exists also in the provided config file,
        then the value of the cmd is used. The priority is cmd> custom config> default config. 
        You can combine the '--load' and '--save' commands to modify a configuration file.
    ");
    
    utils::wait_for_input();
    process::exit(0);
}


#[derive(Debug)]
struct ConfigurationBuilder {
    pub path: Option<String>,
    pub exclude_dirs: Option<Vec<String>>,
    pub languages_of_interest: Option<Vec<String>>,
    pub threads: Option<usize>,
    pub braces_as_code: Option<bool>,
    pub should_search_in_dotted: Option<bool>,
    pub should_show_faulty_files: Option<bool>,
    pub no_visual: Option<bool>
}

impl ConfigurationBuilder {
    pub fn new(path: Option<String>, exclude_dirs: Option<Vec<String>>, languages_of_interest: Option<Vec<String>>,
            threads: Option<usize>, braces_as_code: Option<bool>, should_search_in_dotted: Option<bool>,
            should_show_faulty_files: Option<bool>, no_visual: Option<bool>) 
    -> ConfigurationBuilder 
    {
        ConfigurationBuilder {
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

    pub fn add_missing_fields(&mut self, config: PersistentOptions) -> &mut ConfigurationBuilder {
        if self.path.is_none() {self.path = config.path};
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
            path : self.path.clone().unwrap(),
            exclude_dirs: (self.exclude_dirs).clone().unwrap_or_default(),
            languages_of_interest: (self.languages_of_interest).clone().unwrap_or_default(),
            threads: self.threads.unwrap_or(DEF_THREADS),
            braces_as_code: self.braces_as_code.unwrap_or(DEF_BRACES_AS_CODE),
            should_search_in_dotted: self.should_search_in_dotted.unwrap_or(DEF_SEARCH_IN_DOTTED),
            should_show_faulty_files: self.should_show_faulty_files.unwrap_or(DEF_SHOW_FAULTY_FILES),
            no_visual: self.no_visual.unwrap_or(DEF_NO_VISUAL)
        }
    }
}

impl Configuration {
    pub fn new(path: String) -> Configuration {
        Configuration {
            path,
            exclude_dirs: Vec::new(),
            languages_of_interest: Vec::new(),
            threads: DEF_THREADS,
            braces_as_code: DEF_BRACES_AS_CODE,
            should_search_in_dotted: DEF_SEARCH_IN_DOTTED,
            should_show_faulty_files: DEF_SHOW_FAULTY_FILES,
            no_visual: DEF_NO_VISUAL
        }
    }

    pub fn set_exclude_dirs(&mut self, exclude_dirs: Vec<String>) -> &mut Configuration {
        self.exclude_dirs = exclude_dirs;
        self
    }

    pub fn set_languages_of_interest(&mut self, languages_of_interest: Vec<String>) -> &mut Configuration {
        self.languages_of_interest = languages_of_interest;
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
            Self::IncorrectCommandArgs(p) => format!("Incorrect arguments provided for the command '--{}'. Type '--help'",p).red().to_string()
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_arg_parsing() {
        assert!(create_config_from_args("path --exclude   --languages .java    .cs").is_err());
        assert!(create_config_from_args("path --exclude--languages .java    .cs").is_err());
        assert!(create_config_from_args("path --languages .java .cs --exclude").is_err());
        assert!(create_config_from_args("path --something").is_err());
        assert!(create_config_from_args("path --threads 3 --something --exclude e").is_err());

        assert_eq!(Configuration::new("./".to_owned()),create_config_from_args("./").unwrap());

        assert_eq!(*Configuration::new("./".to_owned())
            .set_exclude_dirs(vec!["ex1".to_owned(),"ex2".to_owned()])
            .set_languages_of_interest(vec!["java".to_owned(),"cs".to_owned()])
            .set_threads(4),
            create_config_from_args("./ --threads 4 --exclude ex1 ex2 --languages java cs").unwrap()
        );

        assert_eq!(*Configuration::new("./".to_owned())
            .set_exclude_dirs(vec!["ex2".to_owned()]) 
            .set_languages_of_interest(vec!["java".to_owned(),"cs".to_owned()]),
            create_config_from_args("./   --exclude ex2  --languages java    cs").unwrap()
        );

        assert_eq!(*Configuration::new("./".to_owned())
            .set_exclude_dirs(vec!["ex2".to_owned()]) 
            .set_languages_of_interest(vec!["java".to_owned(),"cs".to_owned()]),
            create_config_from_args("./   --exclude ex2  --languages .java    .cs").unwrap()
        );
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

        let path = "src/putils";
        let abs = convert_to_absolute(path);
        assert!(Path::new(path).is_relative());
        assert!(Path::new(&abs).is_absolute());
    }
}

