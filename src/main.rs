use std::{collections::HashMap, time::Instant};

use colored::*;
use include_dir::include_dir;

use mezura::{*, self, config_manager::{self, CHANGELOG, HELP, SHOW_CONFIGS, SHOW_LANGUAGES, SHOW_PALETTES, TUNE_PALETTES, VERSION_ID}, io_handler, theme};


fn main() {
    // Only on windows, it is required to enable a virtual terminal environment, so that the colors will display correctly
    #[cfg(target_os = "windows")]
    control::set_virtual_terminal(true).unwrap();

    let mut language_map: HashMap<String, Language>;

    if !PERSISTENT_APP_PATHS.are_initialized {
        // If it is the first execution, use the baked-in language folder of the executable to initialize the language map
        // and save the baked-in info, to a persistent path for future uses and user modification.
        language_map = read_baked_in_languages_dir();
        if let Err(x) = init_persistent_paths(&language_map, read_baked_in_default_config_contents()) {
            // Whatever was created stays on disk. It is not considered a valid installation anyway,
            // so the next execution will detect that and try to complete it again.
            println!("{}",format!("\nUnable to initialize persistent directories: {x}\n").yellow());
        }
    } else {
        match io_handler::parse_supported_languages_to_map(&PERSISTENT_APP_PATHS.languages_dir) {
            Ok((_language_map, faulty_files)) => {
                if !faulty_files.is_empty() {
                    let mut warn_msg = String::from("\nFormatting problems detected in language files: ");
                    warn_msg.push_str(&faulty_files.join(", "));
                    warn_msg.push_str(".\nThese files will not be taken into consideration.");
                    println!("{}",warn_msg.yellow());
                }

                language_map = _language_map;
            },
            Err(x) => {
                println!("\n{}", x.formatted());
                return;
            }
        }
    }

    if PERSISTENT_APP_PATHS.are_initialized && !dir_contains_entries(&PERSISTENT_APP_PATHS.palettes_dir)
        && let Err(x) = write_baked_in_palettes() {
        println!("{}",format!("\nUnable to initialize the color palettes directory: {x}\n").yellow());
    }

    if PERSISTENT_APP_PATHS.are_initialized {
        match io_handler::migrate_legacy_palettes(&PERSISTENT_APP_PATHS.palettes_dir) {
            Ok(migrated) if !migrated.is_empty() =>
                println!("{}", format!("\nUpdated {} color palette(s) to the new format: {}\n", migrated.len(), migrated.join(", ")).yellow()),
            Err(x) => println!("{}", format!("\nUnable to update the color palettes to the new format: {x}\n").yellow()),
            _ => ()
        }
    }

    let args_str = match read_args_as_str() {
        Some(args) => {
            args
        },
        None => {
            String::from("./")
        }
    };

    if handle_message_only_command(&args_str, &language_map) {
        return;
    }

    let config = match config_manager::create_config_from_args(&args_str) {
        Ok(config) => config,
        Err(x) => {
            println!("\n{}\n",x.formatted());
            return;
        }
    };
    theme::set_active(config.theme.clone());

    // Printed here and not at the very start, so that '--hide version' can be declared in a
    // configuration file and not only on the command line
    if !config.hidden.version {
        // The status block opens with a blank line of its own, so the separation below the
        // version is only missing when that block is not printed
        let separator = if config.hidden.status {"\n"} else {""};
        println!("\n{}{separator}", theme::active().version.paint(VERSION_ID));
    }

    if !config.languages_of_interest.is_empty() &&
     config.languages_of_interest.iter().all(|lang| config.excluded_languages.contains(lang)) {
        println!("\n{}\n",theme::active().error.paint("Included and excluded languages are mutually exclusive."));
        return;
    }

    if !config.languages_of_interest.is_empty() {
        match retain_only_languages_of_interest(&mut language_map, &config.languages_of_interest) {
            Ok(x) => {
                if let Some(msg) = x {
                    println!("\n {msg}");
                }
            },
            Err(_) => {
                println!("\n{}\n",theme::active().error.paint("Error: None of the provided language names map to valid supported languages"));
                return;
            }
        }
    }

    if !config.excluded_languages.is_empty() {
        config.excluded_languages.iter().for_each(|x| {
            language_map.retain(|k, _| {
                k.to_lowercase() != x.to_lowercase()
            });
        });
    }

    let instant = Instant::now();
    let hide_timing = config.hidden.timing;
    match mezura::run(config, language_map) {
        Ok(x) => {
            if !hide_timing {
                let perf = format!("Exec time: {:.2} secs ", instant.elapsed().as_secs_f32());
                let metrics = match x {
                    Some(x) => format!("(Parsing {} files/s | {} lines/s)", with_seperators(x.files_per_sec), with_seperators(x.lines_per_sec)),
                    None => String::new()
                };
                println!("\n{}",theme::active().footer.paint(&(perf + &metrics)));
            }
        },
        Err(x) => println!("{}",x.formatted())
    }
}


fn retain_only_languages_of_interest(language_map: &mut HashMap<String, Language>, languages_of_interest: &[String]) -> Result<Option<ColoredString>,()> 
{
    language_map.retain(|s, _| languages_of_interest.iter().any(|x| x.to_lowercase() == s.to_lowercase()));

    if language_map.is_empty() {
        return Err(());
    }

    let mut non_existant_lang_names = String::with_capacity(60);
    let mut has_any_relevant_languages = false;
    languages_of_interest.iter().for_each(|x| {
        if !language_map.iter().any(|(s,_)| s.to_lowercase() == x.to_lowercase()) {
            non_existant_lang_names.push_str(&(x.clone() + " , "));
        } else {
            has_any_relevant_languages = true;
        }
    });

    if !non_existant_lang_names.is_empty() {
        Ok(Some(format!("\nThese languages don't exist as language files:\n {non_existant_lang_names}").yellow()))
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
    // create_dir_all, so that an incomplete dir left behind by a previous failed attempt gets completed
    std::fs::create_dir_all(&PERSISTENT_APP_PATHS.languages_dir)?;
    std::fs::create_dir_all(&PERSISTENT_APP_PATHS.config_dir)?;
    std::fs::create_dir_all(&PERSISTENT_APP_PATHS.logs_dir)?;

    for language in languages.values() {
        io_handler::serialize_language(language, &PERSISTENT_APP_PATHS.languages_dir)?;
    }

    io_handler::write_default_config(default_config_contents)?;
    write_baked_in_palettes()?;

    Ok(())
}

fn open_in_browser(path: &str) {
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("cmd").args(["/C", "start", "", path]).spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = std::process::Command::new("xdg-open").arg(path).spawn();

    if result.is_err() {
        println!("(the page could not be opened in a browser automatically)");
    }
}

fn write_baked_in_palettes() -> Result<(),std::io::Error> {
    std::fs::create_dir_all(&PERSISTENT_APP_PATHS.palettes_dir)?;
    for file in include_dir!("data/palettes").files.iter() {
        let file_name = std::path::Path::new(file.path).file_name().and_then(|x| x.to_str()).unwrap_or(file.path);
        std::fs::write(PERSISTENT_APP_PATHS.palettes_dir.clone() + file_name, file.contents)?;
    }

    Ok(())
}

fn read_args_as_str() -> Option<String> {
    let args = std::env::args().skip(1)
            .filter_map(|arg| get_trimmed_if_not_empty(&arg))
            .collect::<Vec<String>>();
    if args.is_empty() {
        None
    } else {
        Some(args.join(" ").trim().to_owned())
    }
}

// These commands take no configuration, so there is nothing that could hide the version line
// from them, and they are the only place where the version of an installed binary can be read
fn handle_message_only_command(args_str: &str, language_map: &HashMap<String,Language>) -> bool {
    let is_present = |command: &str| args_str.contains(&(String::from("--") + command));
    if ![HELP, CHANGELOG, SHOW_LANGUAGES, SHOW_CONFIGS, SHOW_PALETTES, TUNE_PALETTES].iter().any(|x| is_present(x)) {
        return false;
    }
    println!("\n{}", theme::active().version.paint(VERSION_ID));

    if args_str.contains(&(String::from("--") + HELP)) {
        message_printer::print_help_message_for_given_args(args_str);
        return true; 
    } else if let Some(pos) = args_str.find(&(String::from("--") + CHANGELOG)) {
        match args_str[pos + CHANGELOG.len() + 2..].split_whitespace().next() {
            Some("full") => message_printer::print_changelog(true),
            Some(arg) if !arg.starts_with("--") => {
                println!("\n{}", config_manager::ArgParsingError::IncorrectCommandArgs(CHANGELOG.to_owned()).formatted());
                message_printer::print_help_message_for_command(CHANGELOG);
            },
            _ => message_printer::print_changelog(false),
        }
        return true;
    } else if args_str.contains(&(String::from("--") + SHOW_LANGUAGES)) {
        message_printer::print_supported_languages(language_map);
        return true;
    } else if args_str.contains(&(String::from("--") + SHOW_CONFIGS)) {
        message_printer::print_existing_configs();
        return true;
    } else if args_str.contains(&(String::from("--") + TUNE_PALETTES)) {
        match io_handler::generate_palette_tuner_page() {
            Ok(path) => {
                println!("\nPalette tuner page generated at:\n{path}");
                open_in_browser(&path);
            },
            Err(x) => println!("\n{}", format!("Unable to generate the palette tuner page: {x}").red())
        }
        return true;
    } else if let Some(pos) = args_str.find(&(String::from("--") + SHOW_PALETTES)) {
        match args_str[pos + SHOW_PALETTES.len() + 2..].split_whitespace().next() {
            Some(arg) if !arg.starts_with("--") => match config_manager::BarThickness::parse(arg) {
                Some(thickness) => message_printer::print_existing_palettes(thickness),
                None => {
                    println!("\n{}", config_manager::ArgParsingError::IncorrectCommandArgs(SHOW_PALETTES.to_owned()).formatted());
                    message_printer::print_help_message_for_command(SHOW_PALETTES);
                }
            },
            _ => message_printer::print_existing_palettes(config_manager::BarThickness::default())
        }
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
                "Java".to_owned() => Language::new("Java".to_owned(),vec![],vec![],vec!["\"".to_owned()],None,None,vec![]),
                "C#".to_owned() => Language::new("C#".to_owned(),vec![],vec![],vec!["\"".to_owned()],None,None,vec![])];

        let result = retain_only_languages_of_interest(&mut language_map, &languages_of_interest);
        assert!(result.unwrap().is_none());
        assert!(language_map.len() == 1);
        
        let languages_of_interest = vec!["java".to_owned(),"c++".to_owned(),"Rust".to_owned()];
        let mut language_map = hashmap![
                "Java".to_owned() => Language::new("Java".to_owned(),vec![],vec![],vec!["\"".to_owned()],None,None,vec![]),
                "C#".to_owned() => Language::new("C#".to_owned(),vec![],vec![],vec!["\"".to_owned()],None,None,vec![])];

        let result = retain_only_languages_of_interest(&mut language_map, &languages_of_interest);
        assert!(result.unwrap().is_some());
        assert!(language_map.len() == 1);
        
        let languages_of_interest = vec!["c++".to_owned(),"Rust".to_owned()];
        let mut language_map = hashmap![
                "Java".to_owned() => Language::new("Java".to_owned(),vec![],vec![],vec!["\"".to_owned()],None,None,vec![]),
                "C#".to_owned() => Language::new("C#".to_owned(),vec![],vec![],vec!["\"".to_owned()],None,None,vec![])];

        let result = retain_only_languages_of_interest(&mut language_map, &languages_of_interest);
        assert!(result.is_err());
        assert!(language_map.is_empty());
    }
}