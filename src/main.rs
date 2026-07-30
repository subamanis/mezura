use std::{collections::HashMap, process::ExitCode, time::Instant};

use colored::*;
use include_dir::include_dir;

use mezura::{*, self, config_manager::{self, CHANGELOG, HELP, LAYOUT, SHOW_CONFIGS, SHOW_LANGUAGES, SHOW_THEMES, THEME_EDITOR, VERSION, VERSION_ID}, io_handler, theme};


// A failure code is owed to whoever runs mezura from a script: everything that prints an error and
// stops is a run that did not happen, and until now all of them were indistinguishable from success.
fn main() -> ExitCode {
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
            eprintln!("{}",format!("\nUnable to initialize persistent directories: {x}\n").yellow());
        }
    } else {
        match io_handler::parse_supported_languages_to_map(&PERSISTENT_APP_PATHS.languages_dir) {
            Ok((_language_map, faulty_files)) => {
                if !faulty_files.is_empty() {
                    let mut warn_msg = String::from("\nFormatting problems detected in language files: ");
                    warn_msg.push_str(&faulty_files.join(", "));
                    warn_msg.push_str(".\nThese files will not be taken into consideration.");
                    eprintln!("{}",warn_msg.yellow());
                }

                language_map = _language_map;
            },
            Err(x) => {
                eprintln!("\n{}", x.formatted());
                return ExitCode::FAILURE;
            }
        }
    }

    if PERSISTENT_APP_PATHS.are_initialized && !dir_contains_entries(&PERSISTENT_APP_PATHS.themes_dir)
        && let Err(x) = write_baked_in_themes() {
        eprintln!("{}",format!("\nUnable to initialize the themes directory: {x}\n").yellow());
    }

    let args_str = match read_args_as_str() {
        Some(args) => {
            args
        },
        None => {
            String::from("./")
        }
    };

    if let Some(code) = handle_message_only_command(&args_str, &language_map) {
        return code;
    }

    let config = match config_manager::create_config_from_args(&args_str) {
        Ok(config) => config,
        Err(x) => {
            eprintln!("\n{}\n",x.formatted());
            return ExitCode::FAILURE;
        }
    };
    theme::set_active(config.theme.clone());
    utils::set_number_separator(config.number_separator);
    utils::set_decimal_separator(config.decimal_separator);

    // A pipe already strips the escape codes, but CLICOLOR_FORCE overrides that and would put them
    // inside the strings of the document, so the machine format turns them off itself
    if !config.prints_text() {
        control::set_override(false);
    }

    // Printed here and not at the very start, so that '--hide version' can be declared in a
    // configuration file and not only on the command line
    if !config.hidden.version && config.prints_text() {
        // The status block opens with a blank line of its own, so the separation below the
        // version is only missing when that block is not printed
        let separator = if config.hidden.directory_info {"\n"} else {""};
        println!("\n{}{separator}", theme::active().version.paint(VERSION_ID));
    }

    if !config.languages_of_interest.is_empty() &&
     config.languages_of_interest.iter().all(|lang| config.excluded_languages.contains(lang)) {
        eprintln!("\n{}\n",theme::active().error.paint("Included and excluded languages are mutually exclusive."));
        return ExitCode::FAILURE;
    }

    if !config.languages_of_interest.is_empty() {
        match retain_only_languages_of_interest(&mut language_map, &config.languages_of_interest) {
            Ok(x) => {
                if let Some(msg) = x {
                    eprintln!("\n {msg}");
                }
            },
            Err(x) => {
                eprintln!("\n{x}\n");
                return ExitCode::FAILURE;
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
    // The footer is text and the document carries its own 'scan_ms', so the format has to be read
    // before the configuration is handed over to the run
    let hide_timing = config.hidden.timing || !config.prints_text();
    match mezura::run(config, language_map) {
        Ok(x) => {
            if !hide_timing {
                let perf = format!("Exec time: {} secs ", utils::with_decimal_separator(format!("{:.2}", instant.elapsed().as_secs_f32())));
                let metrics = match x {
                    Some(x) => format!("(Parsing {} files/s | {} lines/s)", with_seperators(x.files_per_sec), with_seperators(x.lines_per_sec)),
                    None => String::new()
                };
                println!("\n{}",theme::active().footer.paint(&(perf + &metrics)));
            }
            ExitCode::SUCCESS
        },
        Err(x) => {
            eprintln!("{}",x.formatted());
            match x {
                // Finding no code is an answer and not a failure, while every file failing to be
                // parsed means a real error behind each one of them
                ParseFilesError::NoRelevantFiles(_) => ExitCode::SUCCESS,
                ParseFilesError::AllAreFaultyFiles => ExitCode::FAILURE
            }
        }
    }
}


// Every name that did not match is reported with the names it is closest to, one at a time. With
// one line for the whole list there was nothing to attach a suggestion to, and the number of
// language files is only going to grow.
fn retain_only_languages_of_interest(language_map: &mut HashMap<String, Language>, languages_of_interest: &[String])
        -> Result<Option<String>, String>
{
    let mut all_names = language_map.keys().cloned().collect::<Vec<_>>();
    all_names.sort_by_key(|x| x.to_lowercase());
    let candidates = all_names.iter().map(String::as_str).collect::<Vec<_>>();

    // Only the mistake is coloured. What to do about it is not an error, it is the way out.
    let mut report = String::with_capacity(100);
    for name in languages_of_interest {
        if all_names.iter().any(|x| x.eq_ignore_ascii_case(name)) {
            continue;
        }
        report.push_str(&format!("\n{}", theme::active().warning.paint(&format!("'{name}' does not exist as a language file."))));
        if let Some(x) = suggestions::formatted_suggestion(name, &candidates) {
            report.push_str(&format!("\n{x}\n"));
        }
    }

    language_map.retain(|s, _| languages_of_interest.iter().any(|x| x.eq_ignore_ascii_case(s)));
    if language_map.is_empty() {
        let headline = theme::active().error.paint("None of the provided language names map to valid supported languages.");
        return Err(format!("{headline}\n{report}"));
    }

    Ok(if report.is_empty() {None} else {Some(report)})
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
    write_baked_in_themes()?;

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

fn write_baked_in_themes() -> Result<(),std::io::Error> {
    std::fs::create_dir_all(&PERSISTENT_APP_PATHS.themes_dir)?;
    for file in include_dir!("data/themes").files.iter() {
        let file_name = std::path::Path::new(file.path).file_name().and_then(|x| x.to_str()).unwrap_or(file.path);
        std::fs::write(PERSISTENT_APP_PATHS.themes_dir.clone() + file_name, file.contents)?;
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
// from them, and they are the only place where the version of an installed binary can be read.
// 'None' means the args are not one of them, and not that nothing went wrong: three of the branches
// below report a mistake, and a bool could not tell the caller to stop with a failure instead of
// handing arguments it has already rejected to the configuration parser.
fn handle_message_only_command(args_str: &str, language_map: &HashMap<String,Language>) -> Option<ExitCode> {
    let is_present = |command: &str| args_str.contains(&(String::from("--") + command));
    if ![HELP, VERSION, CHANGELOG, SHOW_LANGUAGES, SHOW_CONFIGS, SHOW_THEMES, THEME_EDITOR].iter().any(|x| is_present(x)) {
        return None;
    }

    // '--version' prints the line itself, with the release date next to it, so it is answered before
    // the plain banner that every other message-only command opens with. With '--help' next to it,
    // the question is about the command and belongs to the help.
    if is_present(VERSION) && !is_present(HELP) {
        message_printer::print_version();
        return Some(ExitCode::SUCCESS);
    }
    println!("\n{}", theme::active().version.paint(VERSION_ID));

    if args_str.contains(&(String::from("--") + HELP)) {
        message_printer::print_help_message_for_given_args(args_str);
        return Some(ExitCode::SUCCESS);
    } else if let Some(pos) = args_str.find(&(String::from("--") + CHANGELOG)) {
        return match args_str[pos + CHANGELOG.len() + 2..].split_whitespace().next() {
            Some("full") => {
                message_printer::print_changelog(true);
                Some(ExitCode::SUCCESS)
            },
            Some(arg) if !arg.starts_with("--") => {
                println!("\n{}", config_manager::ArgParsingError::IncorrectCommandArgs(CHANGELOG.to_owned()).formatted());
                message_printer::print_help_message_for_command(CHANGELOG);
                Some(ExitCode::FAILURE)
            },
            _ => {
                message_printer::print_changelog(false);
                Some(ExitCode::SUCCESS)
            },
        };
    } else if args_str.contains(&(String::from("--") + SHOW_LANGUAGES)) {
        message_printer::print_supported_languages(language_map);
        return Some(ExitCode::SUCCESS);
    } else if args_str.contains(&(String::from("--") + SHOW_CONFIGS)) {
        message_printer::print_existing_configs();
        return Some(ExitCode::SUCCESS);
    } else if args_str.contains(&(String::from("--") + THEME_EDITOR)) {
        return match io_handler::generate_theme_editor_page() {
            Ok(path) => {
                println!("\nTheme editor page generated at:\n{path}");
                open_in_browser(&path);
                Some(ExitCode::SUCCESS)
            },
            Err(x) => {
                println!("\n{}", format!("Unable to generate the theme editor page: {x}").red());
                Some(ExitCode::FAILURE)
            }
        };
    } else if let Some(pos) = args_str.find(&(String::from("--") + SHOW_THEMES)) {
        // The preview follows '--layout', so that what it shows is what a run would print. Read here
        // by hand, because a message-only command runs before there is a configuration to ask.
        let layout = args_str.find(&(String::from("--") + LAYOUT))
                .and_then(|at| args_str[at + LAYOUT.len() + 2..].split_whitespace().next())
                .and_then(config_manager::Layout::parse)
                .unwrap_or_default();

        return match args_str[pos + SHOW_THEMES.len() + 2..].split_whitespace().next() {
            Some(arg) if !arg.starts_with("--") => match config_manager::BarThickness::parse(arg) {
                Some(thickness) => {
                    message_printer::print_existing_themes(thickness, layout);
                    Some(ExitCode::SUCCESS)
                },
                None => {
                    println!("\n{}", config_manager::ArgParsingError::IncorrectCommandArgs(SHOW_THEMES.to_owned()).formatted());
                    message_printer::print_help_message_for_command(SHOW_THEMES);
                    Some(ExitCode::FAILURE)
                }
            },
            _ => {
                message_printer::print_existing_themes(config_manager::BarThickness::default(), layout);
                Some(ExitCode::SUCCESS)
            }
        };
    }

    None
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