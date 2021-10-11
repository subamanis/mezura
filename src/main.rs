use std::{collections::HashMap, process, time::{Instant}};

use colored::*;
#[macro_use]
extern crate include_dir;

use mezura::{*, self, config_manager::{self, ArgParsingError}, io_handler::{self, LanguageDirParseError}};


fn main() {
    // Only on windows, it is required to enable a virtual terminal environment, so that the colors will display correctly
    #[cfg(target_os = "windows")]
    control::set_virtual_terminal(true).unwrap();

    let mut languages_map: HashMap<String, Language> = hashmap![];

    //if it is the first execution, use the baked-in language folder of the executable to initialize the language map
    //and save the baked-in info, to a persistent path for future uses and user modification.
    if !PERSISTENT_APP_PATHS.are_initialized {
        languages_map = read_baked_in_languages_dir();
        if let Err(x) = init_persistent_paths(&languages_map, read_baked_in_default_config_contents()) {
            println!("{}",format!("Unable to initialize persistent directories:{}\n",x.to_string()).yellow());
            std::fs::remove_dir_all(&PERSISTENT_APP_PATHS.project_path).unwrap();
        }
    } 
    

    let args_str = read_args_as_str();
    if let Err(x) = args_str {
        println!("\n{}",x.formatted());
        process::exit(1);
    }

    let args_str = args_str.unwrap();
    if args_str.contains("--help") {
        help_message_printer::print_appropriate_help_message(&args_str);
        return;
    }

    
    let mut config = match config_manager::create_config_from_args(&args_str) {
        Ok(config) => config,
        Err(x) => {
            println!("\n{}",x.formatted());
            process::exit(1);
        } 
    };

    //if it is not the first execution, initialize the language map from the persistent path, that will contain any
    //potential user modifications to the languages.
    if PERSISTENT_APP_PATHS.are_initialized {
        match read_language_map_from_persistent_path(&mut config) {
            Ok(x) => languages_map = x,
            Err(x) => {
                println!("\n{}", x.formatted());
                process::exit(1);
            }
        } 
    }

    let instant = Instant::now();
    match mezura::run(config, languages_map) {
        Ok(x) => {
            let perf = format!("\nExec time: {:.2} secs ", instant.elapsed().as_secs_f32());
            let metrics = match x {
                Some(x) => format!("(Parsing {} files/s | {} lines/s)", with_seperators(x.files_per_sec), with_seperators(x.lines_per_sec)),
                None => String::new()
            };
            println!("{}",perf + &metrics);
        },
        Err(x) => println!("{}",x.formatted())
    }
}


fn read_baked_in_languages_dir() -> HashMap<String, Language> {
    let mut lang_files = HashMap::with_capacity(20);
    for file in include_dir!("data/languages").files.iter() {
        let language = io_handler::parse_string_to_language(String::from_utf8_lossy(file.contents));
        lang_files.insert(language.name.to_owned(), language);
    }

    lang_files
}

fn read_baked_in_default_config_contents() -> String {
    String::from_utf8_lossy(include_bytes!("../data/config/default.txt")).to_string()
}

fn init_persistent_paths(languages: &HashMap<String,Language>, default_config_contents: String) -> Result<(),std::io::Error> {
    std::fs::create_dir(&PERSISTENT_APP_PATHS.languages_dir).unwrap();
    std::fs::create_dir(&PERSISTENT_APP_PATHS.config_dir).unwrap();
    std::fs::create_dir(&PERSISTENT_APP_PATHS.logs_dir).unwrap();

    for language in languages.values() {
        io_handler::serialize_language(language, &PERSISTENT_APP_PATHS.languages_dir)?;
    }

    io_handler::write_default_config(default_config_contents)?;

    Ok(())
}

fn read_language_map_from_persistent_path(config: &mut Configuration) -> Result<HashMap<String,Language>,LanguageDirParseError> {
    match io_handler::parse_supported_languages_to_map(&PERSISTENT_APP_PATHS.languages_dir, &mut config.languages_of_interest) {
        Ok(x) =>  {
            if !x.faulty_files.is_empty() {
                let mut warn_msg = String::from("\nFormatting problems detected in language files: ");

                warn_msg.push_str(&x.faulty_files.join(", "));
                warn_msg.push_str(". These files will not be taken into consideration.");
                println!("{}",warn_msg.yellow());
            }
            
            if !x.non_existant_languages.is_empty() {
                let relevant = x.non_existant_languages.iter().filter_map(|s| if !x.faulty_files.contains(&(s.to_owned()+".txt")){Some(s.to_owned())} else {None}).collect::<Vec<_>>();
                if !relevant.is_empty() {
                    let warn_msg = format!("\nThese languages don't exist as language files: {}",relevant.join(", "));
                    println!("{}",warn_msg.yellow());
                }
            }

            Ok(x.language_map)
        }, 
        Err(x) => Err(x)
    }
}

fn read_args_as_str() -> Result<String,ArgParsingError> {
    let args = std::env::args().skip(1)
            .filter_map(|arg| get_trimmed_if_not_empty(&arg))
            .collect::<Vec<String>>();
    if args.is_empty() {return Err(ArgParsingError::NoArgsProvided)}
    Ok(args.join(" ").trim().to_owned())
}