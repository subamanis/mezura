use std::{collections::HashMap, process::ExitCode, time::Instant};

use colored::*;
use include_dir::{File, include_dir};

use mezura::{*, self, config_manager::{self, CHANGELOG, HELP, LAYOUT, RESTORE, SHOW_CONFIGS, SHOW_LANGUAGES, SHOW_THEMES, THEME_EDITOR, VERSION, VERSION_ID}, io_handler, theme};


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
        // The same writer as '--restore', so it only creates what is missing: this branch is entered
        // by a deleted languages or config directory just as much as by a first run, and overwriting
        // there would cost the user their themes and their default configuration.
        language_map = read_baked_in_languages_dir();
        if let Err(x) = restore_missing_baked_in_files() {
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

    // The themes directory is not part of what makes an installation valid, so it is checked on its
    // own. The check is one directory read and not a stat per file, which is why it is here on every
    // run while the rest is left to '--restore'
    if PERSISTENT_APP_PATHS.are_initialized && !dir_contains_entries(&PERSISTENT_APP_PATHS.themes_dir)
        && let Err(x) = restore_missing_baked_in_files() {
        eprintln!("{}",format!("\nUnable to initialize the themes directory: {x}\n").yellow());
    }

    // An installation made before this file existed would otherwise never receive it, and the only
    // sign would be that a decision the user thought they had made is not being applied. One stat
    // per run buys that, the same trade the themes directory above makes.
    if PERSISTENT_APP_PATHS.are_initialized
        && !std::path::Path::new(&(PERSISTENT_APP_PATHS.data_dir.clone() + EXTENSION_PRIORITY_FILE_NAME)).exists()
        && let Err(x) = restore_missing_baked_in_files() {
        eprintln!("{}",format!("\nUnable to create the '{EXTENSION_PRIORITY_FILE_NAME}' file: {x}\n").yellow());
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

    let (extension_priority, faulty_priority_lines) = read_extension_priority();
    if !faulty_priority_lines.is_empty() {
        eprintln!("{}", format!("\nLines that could not be read in '{EXTENSION_PRIORITY_FILE_NAME}', and were skipped:\n{}",
                faulty_priority_lines.join("\n")).yellow());
    }

    let instant = Instant::now();
    match mezura::run(&config, language_map, &extension_priority) {
        Ok(result) => {
            mezura::present(&result, &config);
            // The document carries its own 'scan_ms', measured inside the run, and this is the only
            // place that knows what the whole thing took. A run that found nothing to count says so
            // and stops there, with no timing under it
            if !config.hidden.timing && config.prints_text() && result.files_present.relevant_files > 0 {
                let perf = format!("Exec time: {} secs ", utils::with_decimal_separator(format!("{:.2}", instant.elapsed().as_secs_f32())));
                let metrics = match result.metrics {
                    Some(x) => format!("(Parsing {} files/s | {} lines/s)", with_seperators(x.files_per_sec), with_seperators(x.lines_per_sec)),
                    None => String::new()
                };
                println!("\n{}",theme::active().footer.paint(&(perf + &metrics)));
            }
            ExitCode::SUCCESS
        },
        // Finding no code is an answer and not a failure, so it comes back as a result with nothing
        // in it. Only every single file failing to be parsed reaches this, and there is a real
        // error behind each one of them
        Err(x) => {
            if let ParseFilesError::AllAreFaultyFiles(faulty_files) = &x {
                mezura::print_faulty_files_or_ok(faulty_files, &config);
            }
            eprintln!("{}",x.formatted());
            ExitCode::FAILURE
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

fn read_baked_in_extension_priority_contents() -> String {
    String::from_utf8_lossy(include_bytes!("../data/extension_priority.txt")).to_string()
}

// An installation made by an earlier version has no such file, and the baked-in copy is not used as
// a substitute: the user is meant to edit the one on disk, and reading a different one would make
// their edits look like they had no effect. It is written by the same restore that writes everything
// else, and until it is there every contested extension simply announces its tiebreak.
fn read_extension_priority() -> (HashMap<String,Vec<String>>, Vec<String>) {
    io_handler::parse_extension_priority_file(&(PERSISTENT_APP_PATHS.data_dir.clone() + EXTENSION_PRIORITY_FILE_NAME))
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

// Only ever creates. Whatever is on disk stays as it is, which is what makes this safe to run at any
// time: a language, theme or configuration of your own is never a file that ships with the program,
// and one that ships and was edited is not overwritten either.
// A file that exists but is damaged is not repaired, because from here it cannot be told apart from
// one that was emptied deliberately.
fn restore_missing_baked_in_files() -> Result<Vec<String>, std::io::Error> {
    let mut created = Vec::new();
    write_missing_files(include_dir!("data/languages").files, &PERSISTENT_APP_PATHS.languages_dir, &mut created)?;
    write_missing_files(include_dir!("data/themes").files, &PERSISTENT_APP_PATHS.themes_dir, &mut created)?;

    // The logs directory holds nothing that ships, but without it a run with '--log' has nowhere to write
    std::fs::create_dir_all(&PERSISTENT_APP_PATHS.logs_dir)?;
    std::fs::create_dir_all(&PERSISTENT_APP_PATHS.config_dir)?;
    if !std::path::Path::new(&(PERSISTENT_APP_PATHS.config_dir.clone() + DEFAULT_CONFIG_NAME)).exists() {
        io_handler::write_default_config(read_baked_in_default_config_contents())?;
        created.push(DEFAULT_CONFIG_NAME.to_owned());
    }

    let priority_path = PERSISTENT_APP_PATHS.data_dir.clone() + EXTENSION_PRIORITY_FILE_NAME;
    if !std::path::Path::new(&priority_path).exists() {
        std::fs::write(&priority_path, read_baked_in_extension_priority_contents())?;
        created.push(EXTENSION_PRIORITY_FILE_NAME.to_owned());
    }

    Ok(created)
}

fn write_missing_files(files: &[File], target_dir: &str, created: &mut Vec<String>) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(target_dir)?;
    for file in files {
        let name = std::path::Path::new(file.path).file_name().and_then(|x| x.to_str()).unwrap_or(file.path);
        let path = target_dir.to_owned() + name;
        if !std::path::Path::new(&path).exists() {
            std::fs::write(&path, file.contents)?;
            created.push(name.to_owned());
        }
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
    if ![HELP, VERSION, CHANGELOG, SHOW_LANGUAGES, SHOW_CONFIGS, SHOW_THEMES, THEME_EDITOR, RESTORE].iter().any(|x| is_present(x)) {
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
    } else if args_str.contains(&(String::from("--") + RESTORE)) {
        return match restore_missing_baked_in_files() {
            Ok(created) => {
                if created.is_empty() {
                    println!("\nNothing to restore, every file that ships with mezura is in place.");
                } else {
                    let plural = if created.len() == 1 {"file"} else {"files"};
                    println!("\nRestored {} {plural}:\n{}", created.len(), created.join(", "));
                }
                Some(ExitCode::SUCCESS)
            },
            Err(x) => {
                println!("\n{}", format!("Unable to restore the missing files: {x}").red());
                Some(ExitCode::FAILURE)
            }
        };
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
    use include_dir::include_dir;
    use mezura::{LOCAL_APP_PATHS, Language, hashmap};

    use crate::{retain_only_languages_of_interest, write_missing_files};

    #[test]
    fn restoring_creates_what_is_missing_and_touches_nothing_else() {
        let dir = LOCAL_APP_PATHS.test_dir.clone() + "restore-test/";
        let _ = std::fs::remove_dir_all(&dir);
        let files = include_dir!("data/themes").files;

        let mut created = Vec::new();
        write_missing_files(files, &dir, &mut created).unwrap();
        assert_eq!(files.len(), created.len());

        let mut nothing_missing = Vec::new();
        write_missing_files(files, &dir, &mut nothing_missing).unwrap();
        assert!(nothing_missing.is_empty());

        // The edited one has to survive, since a restore that undid your own changes would be worse
        // than the missing file it was meant to fix
        let edited = dir.clone() + &created[0];
        std::fs::write(&edited, b"mine").unwrap();
        std::fs::remove_file(dir.clone() + &created[1]).unwrap();

        let mut second_round = Vec::new();
        write_missing_files(files, &dir, &mut second_round).unwrap();
        assert_eq!(vec![created[1].clone()], second_round);
        assert_eq!("mine", std::fs::read_to_string(&edited).unwrap());

        std::fs::remove_dir_all(&dir).unwrap();
    }

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