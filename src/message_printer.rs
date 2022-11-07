use std::{collections::HashMap, fs};

use colored::Colorize;

use crate::{CHANGELOG_BYTES, Language, PERSISTENT_APP_PATHS, config_manager::*};

// These constants need to be maintained along with the readme's commands
pub const DIRS_HELP  :  &str = 
"--dirs
    The paths to the directories or files, seperated by commas if more than 1,
    in this form: '--dirs <path1>, <path2>'
    If you are using Windows Powershell, you will need to escape the commas with a backtick: ` 
    or surround all the arguments with quatation marks:
    <path1>`, <path2>`, <path3>   or   \"<path1>, <path2>, <path3>\"

    The target directories can also be given implicitly (in which case this command is not needed) with 2 ways:
    1) as the first arguments of the program directly
    2) if they are present in a configuration file (see '--save' and '--load' commands).

"; 
pub const EXCLUDE_HELP  :  &str = 
"--exclude 
    1..n arguments separated by commas, can be a folder name, a file name (including extension), 
    or a full path to a folder or file.
    If you are using Windows Powershell, you will need to escape the commas with a backtick: ` 
    or surround all the arguments with quatation marks:
    <arg1>`, <arg2>`, <arg3>   or   \"<arg1>, <arg2>, <arg3>\"

    The program will ignore these dirs.

"; 
pub const LANGUAGES_HELP  :  &str = 
"--languages 
    1..n arguments separated by commas, case-insensitive

    The given language names must exist in any of the files in the 'data/languages/' dir as the
    parameter of the field 'Language'.

    Only the languages specified here will be taken into account for the stats.

"; 
pub const THREADS_HELP  :  &str = 
"--threads
    2 numbers: the first between 1 and 4 and the seconds between 1 and 12. 

    This represents the number of the producers (threads that will traverse the given directories),
    and consumers (threads that will parse whatever files the producers found).

    If this command is not provided, the numbers will be chosen based on the available threads
    on your machine. Generally, a good ratio of producers-consumers is 1:3
    
"; 
pub const BRACES_AS_CODE_HELP  :  &str = 
"--braces-as-code
    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable, or 'no'
    to disable. Default: no 

    Specifies whether lines that only contain braces ( {{ or }} ), should be considered as code lines or not.

    The default behaviour is to not count them as code, since it is silly for code of the same content
    and substance to be counted differently, according to the programer's code style.
    This helps to keep the stats clean when using code lines as a complexity and productivity metric.

"; 
pub const SEARCH_IN_DOTTED_HELP  :  &str = 
"--search-in-dotted
    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
    or 'no' to disable. Default: no 

    Specifies whether the program should traverse directories that are prefixed with a dot,
    like .vscode or .git.

"; 
pub const SHOW_FAULTY_FILES_HELP  :  &str = 
"--show-faulty-files
    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
    or 'no' to disable. Default: no 

    Sometimes it happens that an error occurs when trying to parse a file, either while opening it,
    or while reading it's contents. The default behavior when this happens is to count all of
    the faulty files and display their count.

    This flag specifies that their path, along with information about the exact error is displayed too.
    The most common reason for this error is if a file contains non UTF-8 characters. 

"; 
pub const NO_VISUAL_HELP  :  &str = 
"--no-visual
    No arguments in the cmd, but if specified in a configuration file use 'true' or 'yes' to enable,
    or 'no' to disable. Default: no 

    Disables the colors in the \"overview\" section of the results, and disables the visualization with 
    the vertical lines that reprisent the percentages.

"; 
pub const LOG_HELP  :  &str = 
"--log 
    Can take 0..n words as arguments in the cmd.
    If specified in a configuration file use 'true' or 'yes' to enable,
    or 'no' to disable. Default: no 

    This flag only works if a configuration file is loaded. Specifies that a new log entry should be made
    with the stats of this program execution, inside the appropriate file in the 'data/logs' directory.
    If not log file exists for this configuration, one is created.
    All the provided arguments are used as a description of the log entry.

"; 
pub const COMPRARE_LEVEL_HELP  :  &str = 
"--compare
    1 argument: a number between 0 and 10. Default: 1

    This flag only works if a configuration file is loaded. Specifies with how many previous logs this
    program execution should be compared to (see '--save' and '--load' commands).

    Providing 0 as argument will disable the progress report (comparison).

"; 
pub const SAVE_HELP  :  &str = 
"--save
    One argument as the file name (whitespace allowed, without an extension, case-insensitive)

    Doing so, will run the program and also create a .txt configuration file,
    inside 'data/config/' with the specified name, that can later be loaded with the --load command.

"; 
pub const LOAD_HELP  :  &str = 
"--load
    One argument as the file name (whitespace allowed, without an extension, case-insensitive)

    Assosiated with the '--save' command, this command is used to load the flags of 
    an existing configuration file from the 'data/config/' directory. 

    You can combine the '--load' and '--save' commands to modify a configuration file.

"; 
pub const CHANGELOG_HELP  :  &str =
"--changelog
    No arguments.

    Overrides normal program execution and just prints a summary of the changes
    of every previous version of the program
    
";
pub const SHOW_LANGUAGES_HELP  :  &str =
"--show-languages
    No arguments.

    Overrides normal program execution and just prints a sorted list with the names of
    all the supported languages that were detected in the persistent data path
    of the application, where you can add more. 
    
";
pub const SHOW_CONFIGS_HELP  :  &str =
"--show-configs
    No arguments.

    Overrides normal program execution and just prints a sorted list with the names of
    all the configuration files that were detected in the persistent data path
    of the application. 
    
";


pub fn print_whole_help_message() {
    let mut msg = get_data_dir_str();
    msg += "Format of arguments: <path_here> --optional_command1 --optional_commandN\n\nCOMMANDS:\n\n";

    msg += CHANGELOG_HELP;
    msg += SHOW_LANGUAGES_HELP;
    msg += SHOW_CONFIGS_HELP;
    msg += DIRS_HELP;
    msg += EXCLUDE_HELP;
    msg += LANGUAGES_HELP;
    msg += THREADS_HELP;
    msg += BRACES_AS_CODE_HELP;
    msg += SEARCH_IN_DOTTED_HELP;
    msg += SHOW_FAULTY_FILES_HELP;
    msg += NO_VISUAL_HELP;
    msg += LOG_HELP;
    msg += COMPRARE_LEVEL_HELP;
    msg += SAVE_HELP;
    msg += LOAD_HELP;

    println!("{}",msg);
}

pub fn print_help_message_for_given_args(args_line: &str) {
    let options = args_line.split("--").skip(1).collect::<Vec<_>>();
    if options.len() == 1 {
        print_whole_help_message();
        return;
    }

    let mut msg = get_data_dir_str();

    for option in options {
        if option.trim().is_empty() {continue;}
        let sliced = option.split_whitespace().collect::<Vec<_>>();

        if let Some(x) = get_help_msg_of_command(sliced[0]) {
            msg += x;
        } else {
            if sliced[0].trim() != HELP {
                msg += &format!("'--{}' is not recognised as a command\n\n",sliced[0]);
            }
        }
    }

    if msg.is_empty() {
        print_whole_help_message();
    } else {
        println!("{}",msg);
    }
}

pub fn print_help_message_for_command(arg: &str) {
    if let Some(x) = get_help_msg_of_command(arg) {
        println!("\n{}",x);
    } 
}

pub fn print_changelog() {
    println!("\n{}\n", String::from_utf8_lossy(&CHANGELOG_BYTES));
}

pub fn print_supported_languages(languages_map: &HashMap<String,Language>) {
    let mut lang_names = languages_map.keys().map(|x| x.to_owned()).collect::<Vec<_>>();
    lang_names.sort();
    let prefix = get_data_dir_str();
    println!("{}The supported languages found are:\n  {}\n",prefix,lang_names.join("\n  "));
}

pub fn print_existing_configs() {
    let mut config_names = Vec::with_capacity(10);

    let config_dir = match fs::read_dir(&PERSISTENT_APP_PATHS.config_dir) {
        Ok(x) => x,
        Err(_) => {
            println!("{}","Could not read the config dir".yellow());
            return;
        }
    };
    for path in config_dir {
        if let Ok(x) = path {
            if let Ok(f) = x.file_type() {
                if f.is_file() {
                    config_names.push(x.file_name())
                }
            }
        }
    }
    let mut config_names = config_names.iter().filter_map(|x| {
        let str = x.to_str().unwrap();
        if str != "default.txt" {
            Some(str)
        } else {
            None
        }
    }).collect::<Vec<_>>();
    config_names.sort_unstable();
    let prefix = get_data_dir_str();
    println!("{}Found these configurations:\n  {}\n",prefix,config_names.join("\n  "));
}


fn get_data_dir_str() -> String {
    format!("\nData dir path: {}\n\n", PERSISTENT_APP_PATHS.data_dir)
}

fn get_help_msg_of_command(command: &str) -> Option<&str> {
    if command == DIRS {
        Some(DIRS_HELP)
    } else if command == EXCLUDE {
        Some(EXCLUDE_HELP)
    } else if command == LANGUAGES {
        Some(LANGUAGES_HELP)
    } else if command == THREADS {
        Some(THREADS_HELP)
    } else if command == BRACES_AS_CODE {
        Some(BRACES_AS_CODE_HELP)
    } else if command == SEARCH_IN_DOTTED {
        Some(SEARCH_IN_DOTTED_HELP)
    } else if command == SHOW_FAULTY_FILES {
        Some(SHOW_FAULTY_FILES_HELP)
    } else if command == NO_VISUAL {
        Some(NO_VISUAL_HELP)
    } else if command == LOG {
        Some(LOG_HELP)
    } else if command == COMPRARE_LEVEL {
        Some(COMPRARE_LEVEL_HELP)
    } else if command == SAVE {
        Some(SAVE_HELP)
    } else if command == LOAD {
        Some(LOAD_HELP)
    } else if command == CHANGELOG {
        Some(CHANGELOG_HELP)
    } else if command == SHOW_LANGUAGES {
        Some(SHOW_LANGUAGES_HELP)
    } else if command == SHOW_CONFIGS {
        Some(SHOW_CONFIGS_HELP)
    } else {
        None
    }
}