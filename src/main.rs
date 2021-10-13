use std::{collections::HashMap, process, time::Instant};

use colored::*;
#[macro_use]
extern crate include_dir;

use mezura::{*, self, config_manager::{self, ArgParsingError, CHANGELOG, HELP, SHOW_LANGUAGES, VERSION_ID}, io_handler::{self, LanguageDirParseError}};


fn main() {
    // Only on windows, it is required to enable a virtual terminal environment, so that the colors will display correctly
    #[cfg(target_os = "windows")]
    control::set_virtual_terminal(true).unwrap();

    println!("\n{}",VERSION_ID);

    let mut language_map: HashMap<String, Language>;

    if !PERSISTENT_APP_PATHS.are_initialized {
        // If it is the first execution, use the baked-in language folder of the executable to initialize the language map
        // and save the baked-in info, to a persistent path for future uses and user modification.
        language_map = read_baked_in_languages_dir();
        if let Err(x) = init_persistent_paths(&language_map, read_baked_in_default_config_contents()) {
            println!("{}",format!("Unable to initialize persistent directories:{}\n",x.to_string()).yellow());
            std::fs::remove_dir_all(&PERSISTENT_APP_PATHS.project_path).unwrap();
        }
    } else {
        match io_handler::parse_supported_languages_to_map(&PERSISTENT_APP_PATHS.languages_dir) {
            Ok((_language_map, faulty_files)) => {
                if !faulty_files.is_empty() {
                    let mut warn_msg = String::from("\nFormatting problems detected in language files: ");
                    warn_msg.push_str(&faulty_files.join(", "));
                    warn_msg.push_str(". These files will not be taken into consideration.");
                    println!("{}",warn_msg.yellow());
                }

                language_map = _language_map;
            },
            Err(x) => {
                println!("\n{}", x.formatted());
                process::exit(1);
            }
        }
    }

    let args_str = read_args_as_str();
    if let Err(x) = args_str {
        println!("\n{}",x.formatted());
        process::exit(1);
    }
    let args_str = args_str.unwrap();

    if handle_message_only_command(&args_str, &language_map) {
        return;
    }
    
    let mut config = match config_manager::create_config_from_args(&args_str) {
        Ok(config) => config,
        Err(x) => {
            println!("\n{}\n",x.formatted());
            process::exit(1);
        } 
    };

    if !config.languages_of_interest.is_empty() {
        match retain_only_languages_of_interest(&mut language_map, &mut config.languages_of_interest) {
            Ok(x) => {
                if let Some(msg) = x {
                    println!("\n {}",msg);
                }
            },
            Err(x) => {
                println!("\n{}",x.formatted());
                process::exit(1);
            }
        }
    }

    let instant = Instant::now();
    match mezura::run(config, language_map) {
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


fn retain_only_languages_of_interest(language_map: &mut HashMap<String, Language>, languages_of_interest: &[String])
-> Result<Option<ColoredString>,LanguageDirParseError> 
{
    language_map.retain(|s, _| languages_of_interest.iter().any(|x| x.to_lowercase() == s.to_lowercase()));

    if language_map.is_empty() {
        return Err(LanguageDirParseError::LanguagesOfInterestNotFound);
    }

    let mut non_existant_lang_names = String::with_capacity(60);// "\nThese languages don't exist as language files:\n".to_owned();
    let mut has_any_relevant_languages = false;
    languages_of_interest.iter().for_each(|x| {
        if !language_map.iter().any(|(s,_)| s.to_lowercase() == x.to_lowercase()) {
            non_existant_lang_names.push_str(&(x.clone() + " , "));
        } else {
            has_any_relevant_languages = true;
        }
    });

    if !non_existant_lang_names.is_empty() {
        Ok(Some(format!("\nThese languages don't exist as language files:\n {}",non_existant_lang_names).yellow()))
    } else {
        Ok(None)
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

fn read_args_as_str() -> Result<String,ArgParsingError> {
    let args = std::env::args().skip(1)
            .filter_map(|arg| get_trimmed_if_not_empty(&arg))
            .collect::<Vec<String>>();
    if args.is_empty() {return Err(ArgParsingError::NoArgsProvided)}
    Ok(args.join(" ").trim().to_owned())
}

fn handle_message_only_command(args_str: &str, language_map: &HashMap<String,Language>) -> bool {
    let prefix = "--".to_owned();
    if args_str.contains(&(prefix.clone() + HELP)) {
        message_printer::print_help_message_for_given_args(&args_str);
        return true; 
    } else if args_str.contains(&(prefix.clone() + CHANGELOG)) {
        message_printer::print_changelog();
        return true;
    } else if args_str.contains(&(prefix.clone() + SHOW_LANGUAGES)) {
        message_printer::print_supported_languages(language_map);
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use mezura::{Language, hashmap};

    use crate::retain_only_languages_of_interest;

    #[test]
    fn test_retain_only_languages_of_interest() {
        let languages_of_interest = vec!["java".to_owned()];
        let mut language_map = hashmap![
                "Java".to_owned() => Language::new("Java".to_owned(),vec![],vec![],"\"".to_owned(),None,None,vec![]),
                "C#".to_owned() => Language::new("C#".to_owned(),vec![],vec![],"\"".to_owned(),None,None,vec![])];

        let result = retain_only_languages_of_interest(&mut language_map, &languages_of_interest);
        assert!(result.unwrap().is_none());
        assert!(language_map.len() == 1);
        
        let languages_of_interest = vec!["java".to_owned(),"c++".to_owned(),"Rust".to_owned()];
        let mut language_map = hashmap![
                "Java".to_owned() => Language::new("Java".to_owned(),vec![],vec![],"\"".to_owned(),None,None,vec![]),
                "C#".to_owned() => Language::new("C#".to_owned(),vec![],vec![],"\"".to_owned(),None,None,vec![])];

        let result = retain_only_languages_of_interest(&mut language_map, &languages_of_interest);
        assert!(result.unwrap().is_some());
        assert!(language_map.len() == 1);
        
        let languages_of_interest = vec!["c++".to_owned(),"Rust".to_owned()];
        let mut language_map = hashmap![
                "Java".to_owned() => Language::new("Java".to_owned(),vec![],vec![],"\"".to_owned(),None,None,vec![]),
                "C#".to_owned() => Language::new("C#".to_owned(),vec![],vec![],"\"".to_owned(),None,None,vec![])];

        let result = retain_only_languages_of_interest(&mut language_map, &languages_of_interest);
        assert!(result.is_err());
        assert!(language_map.len() == 0);
    }
}