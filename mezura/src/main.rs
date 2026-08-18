#![forbid(unsafe_code)]
#![allow(non_snake_case)]

macro_rules! hashmap {
    ($( $key: expr => $val: expr ),*) => {{
        #[allow(unused_mut)]
        let mut map = ::std::collections::HashMap::new();
        $( map.insert($key, $val); )*
        map
    }}
}

mod animated_display;
mod args;
mod config_files;
mod config_manager;
mod diff;
mod explain;
mod git;
mod json_printer;
mod json_reader;
mod log;
mod message_printer;
mod migration;
mod number_formatter;
mod paths;
mod present;
mod result_printer;
mod sources;
mod suggestions;
#[cfg(test)]
mod test_support;
mod theme;
mod theme_files;
mod warning_collector;

use std::{process::ExitCode, sync::Arc, time::Instant};

use colored::*;
use mezura_core::{CountingModel, EXTENSION_PRIORITY_FILE_NAME, FilesPresent, Language};
use mezura_core::language_file::PriorityRules;

use crate::config_manager::{Configuration, OutputFormat};
use crate::config_manager::{CHANGELOG, COUNTING, HELP, LAYOUT, OUTPUT, RESTORE, SHOW_CONFIGS,
        SHOW_LANGUAGES, SHOW_THEMES, THEME_EDITOR, VERSION, VERSION_ID};
use crate::message_printer::Formatted;
use crate::migration::{MigrationOutcome, migrate_data_files};

fn main() -> ExitCode {
    // Dropped last of all: a '--diff' removes its temporary checkouts on background threads, and
    // exiting before they finish would leave a half-deleted tree in the temp directory
    let _removals = crate::animated_display::RemovalsGuard;

    // Started here and not at the scan, so the footer answers for the whole command: a '--diff'
    // pays for checkouts and baseline counting long before any scan of this run begins
    let instant = Instant::now();

    // Windows needs a virtual terminal enabled before it shows colors at all
    #[cfg(target_os = "windows")]
    control::set_virtual_terminal(true).unwrap();

    let args_str = read_args_as_str().unwrap_or_else(|| String::from("./"));

    // Before the languages are read, or the run that performs it counts with the old files and the
    // change takes two runs to arrive. Skipped for '--restore', which performs this same pass
    // itself and exists in order to report it.
    let outcome = match crate::args::find_command(&args_str, RESTORE) {
        Some(_) => MigrationOutcome::default(),
        None => migrate_data_files(&crate::paths::PERSISTENT_APP_PATHS.data_dir, false)
    };
    for message in [outcome.format_restored(), outcome.format_replaced(), outcome.format_updated(),
            outcome.format_withdrawn(), outcome.format_merged(), outcome.format_failures()].into_iter().flatten() {
        eprintln!("{message}");
    }

    // The pass above just wrote the directory and is the only thing that knows whether it is whole,
    // so nothing here asks a second, looser version of the same question
    let languages_available = if outcome.every_language_file_is_in_place() {
        match mezura_core::language_file::parse_languages_in_dir(&crate::paths::PERSISTENT_APP_PATHS.languages_dir) {
            // A directory that cannot be read is not a reason to refuse to run, and refusing is
            // worst of all for the one command that repairs it: '--restore' does not perform the
            // startup pass, so a deleted 'languages' folder would kill the run here and print
            // advice to delete the folder it had just been asked to put back.
            Err(x) => {
                eprintln!("\n{}\n", crate::message_printer::wrap_message(&format!(
                        "{}\nCounting with the copies inside the program until it is there again.",
                        x.format())).yellow());
                mezura_core::languages::parse_shipped_languages()
            },
            Ok((parsed, faulty_files)) => {
                if !faulty_files.is_empty() {
                    eprintln!("{}", crate::message_printer::wrap_message(
                            &crate::message_printer::format_faulty_language_files_message(&faulty_files)).yellow());
                    // One warning per file and not one for the list, since each is a whole language
                    // whose files went uncounted and a reader of the document wants to know which
                    for faulty in &faulty_files {
                        let (file, reason) = (&faulty.file_name, &faulty.error);
                        crate::warning_collector::keep(mezura_core::warnings::Warning::new(mezura_core::warnings::Code::LanguageFileUnreadable, file,
                                format!("'{file}' could not be used as a language file, so the files of that language were not counted: {reason}.")));
                    }
                }

                parsed
            }
        }
    } else {
        mezura_core::languages::parse_shipped_languages()
    };

    if let Some(code) = handle_message_only_command(&args_str, &languages_available) {
        return code;
    }

    let mut config = match config_manager::create_config_from_args(&args_str) {
        Ok(config) => config,
        Err(x) => {
            eprintln!("\n{}\n",x.format());
            return ExitCode::FAILURE;
        }
    };
    crate::theme::set_active(config.view.theme.clone());
    crate::number_formatter::set_number_separator(config.view.number_separator);
    crate::number_formatter::set_decimal_separator(config.view.decimal_separator);
    // A terminal that names itself dumb (an Emacs shell buffer, some CI shells) is a tty with no
    // cursor addressing, so nothing moving can be drawn on it; the 'dumb-emacs-ansi' form does
    // understand colors and keeps them, the bare 'dumb' cannot show those either. The removals
    // guard above was built before any of this existed, so it reads the answer from where the
    // setter leaves it rather than from the configuration.
    let terminal_kind = std::env::var("TERM").unwrap_or_default();
    crate::animated_display::set_animations_hidden(
            config.view.hidden.animations || terminal_kind.starts_with("dumb"));
    if terminal_kind == "dumb" {
        control::set_override(false);
    }

    // A pipe already strips the escape codes, but CLICOLOR_FORCE overrides that and would put them
    // inside the strings of the document, so the machine format turns them off itself
    if !config.view.prints_text() {
        control::set_override(false);
    }

    // Printed here and not at the very start, so that '--hide version' can be declared in a
    // configuration file and not only on the command line
    if !config.view.hidden.version && config.view.prints_text() {
        // The status block opens with a blank line of its own, so the separation below the
        // version is only missing when that block is not printed
        let separator = if config.view.hidden.directory_info {"\n"} else {""};
        println!("\n{}{separator}", crate::theme::get_active().version.paint(VERSION_ID));
    }

    if !config.engine.languages_of_interest.is_empty() &&
     config.engine.languages_of_interest.iter().all(|lang| config.engine.excluded_languages.contains(lang)) {
        eprintln!("\n{}\n",crate::theme::get_active().error.paint("Included and excluded languages are mutually exclusive."));
        return ExitCode::FAILURE;
    }

    if !config.engine.languages_of_interest.is_empty() {
        match report_unknown_languages(&languages_available, &config.engine.languages_of_interest) {
            Ok(x) => {
                if let Some(msg) = x {
                    eprintln!("\n {}", crate::message_printer::wrap_message(&msg));
                }
            },
            Err(x) => {
                eprintln!("\n{}\n", crate::message_printer::wrap_message(&x));
                return ExitCode::FAILURE;
            }
        }
    }

    let (extension_priority, faulty_priority_lines) = read_extension_priority();
    if !faulty_priority_lines.is_empty() {
        eprintln!("\n{}", crate::message_printer::wrap_message(
                &format!("Lines that could not be read in '{EXTENSION_PRIORITY_FILE_NAME}', and were skipped:\n{}",
                faulty_priority_lines.join("\n"))).yellow());
        for line in &faulty_priority_lines {
            crate::warning_collector::keep(mezura_core::warnings::Warning::new(mezura_core::warnings::Code::PriorityLineSkipped, line,
                    format!("'{line}' could not be read in '{EXTENSION_PRIORITY_FILE_NAME}' and was skipped, so any contest it was meant to settle was left to the tiebreak.")));
        }
    }

    // Above the language resolution, whose selection a document's adopted settings can change, and
    // below the extension priority, which a side that is a revision is counted with
    let baseline_only = match crate::diff::DiffRequest::of(&mut config, &languages_available) {
        Ok(Some(crate::diff::DiffRequest::BetweenTwoReadings(both))) => {
            return match both.into_comparison(&config, &extension_priority) {
                Ok(comparison) => {
                    if config.view.prints_text() {
                        println!();
                        crate::result_printer::print_comparison(&comparison, &config);
                        // The exec time alone, without the parsing figures: those describe one
                        // scan, and a comparison had up to two
                        if !config.view.hidden.timing {
                            println!("\n{}", crate::theme::get_active().footer.paint(&format_exec_time(&instant)));
                        }
                    } else {
                        crate::json_printer::print_comparison_as_json(&comparison, &chrono::Local::now(), &config);
                    }
                    ExitCode::SUCCESS
                },
                Err(complaint) => {
                    eprintln!("\n{}\n", crate::theme::get_active().error.paint(&crate::message_printer::wrap_message(&complaint)));
                    ExitCode::FAILURE
                }
            };
        },
        Ok(Some(crate::diff::DiffRequest::AgainstThisRun(baseline_only))) => Some(baseline_only),
        Ok(None) => None,
        Err(complaint) => {
            eprintln!("\n{}\n", crate::theme::get_active().error.paint(&crate::message_printer::wrap_message(&complaint)));
            return ExitCode::FAILURE;
        }
    };

    let (languages, reported) = mezura_core::Languages::resolve(&config.engine, languages_available, &extension_priority);
    crate::warning_collector::report_language_resolution_warnings(reported);

    // Its own answer entirely: no scan, no report, no log. The commands that shape a report were
    // refused beside it when the arguments were read.
    if config.view.explain {
        return crate::explain::run_explain(&config, languages);
    }

    // Above the run, so that a baseline which turns out not to be one costs no scan of the tree
    let counted_baseline = match baseline_only.map(|x| x.count_baseline(&config, &extension_priority)) {
        Some(Ok(x)) => Some(x),
        Some(Err(complaint)) => {
            eprintln!("\n{}\n", crate::theme::get_active().error.paint(&crate::message_printer::wrap_message(&complaint)));
            return ExitCode::FAILURE;
        },
        None => None
    };

    let progress = Arc::new(mezura_core::ScanProgress::default());
    let live = crate::animated_display::start_walk_display(&config, progress.clone());

    let mut parsing_live = None;
    let outcome = mezura_core::run(&config.engine, languages, Some(progress.clone()), |scan| {
        live.finish();
        announce_traversal(&config, scan);
        parsing_live = Some(crate::animated_display::start_parsing_display(&config, progress));
    });
    if let Some(x) = &parsing_live {
        x.finish();
    }
    live.finish();
    match outcome {
        Ok(result) => {
            let comparison = counted_baseline.map(|baseline| baseline.with_subject(
                    crate::diff::Reading::of_this_run(&result, &chrono::Local::now(), &config), &config));
            crate::present::present(&result, comparison.as_ref(), &config);
            // Already presented above as the failures they are, and the exit code keeps its meaning:
            // 1 is a run that did not happen. Every file unparseable, or every place unopenable.
            if result.all_relevant_files_were_faulty() || result.nothing_could_be_read() {
                return ExitCode::FAILURE;
            }
            // The document has its own 'scan_ms' measured inside the run; this is the only place
            // that knows what the whole command took. A run that found nothing prints no timing.
            if !config.view.hidden.timing && config.view.prints_text() && result.files_present.relevant_files > 0 {
                let perf = format_exec_time(&instant);
                // Worked out here and not carried on the result: these are arithmetic on the
                // duration and the counts, and the one second rule is a decision about the report.
                let millis = result.performance.duration_millis;
                let metrics = if millis > 1000 {
                    let seconds = millis as f32 / 1000f32;
                    format!("(Parsing {} files/s | {} lines/s)",
                            crate::number_formatter::format_with_separators((result.files_present.relevant_files as f32 / seconds) as usize),
                            crate::number_formatter::format_with_separators((result.total.lines as f32 / seconds) as usize))
                } else {
                    String::new()
                };
                println!("\n{}",crate::theme::get_active().footer.paint(&(perf + &metrics)));
            }
            ExitCode::SUCCESS
        },
        // Mistakes in the configuration surface here, because the run is what resolves the
        // declared targets: the wording is this crate's own, and a configuration file that
        // supplied the targets is named as the culprit the reader cannot see failing
        Err(mezura_core::RunError::InvalidTargets(inner)) => {
            eprintln!("{}", crate::config_manager::attribute_targets_error(inner, &config.view.targets_source).format());
            ExitCode::FAILURE
        },
        Err(x) => {
            eprintln!("{}",x.format());
            ExitCode::FAILURE
        }
    }
}

fn format_exec_time(instant: &Instant) -> String {
    format!("Exec time: {} secs ", crate::number_formatter::format_with_decimal_separator(format!("{:.2}", instant.elapsed().as_secs_f32())))
}

// The two lines between the phases. They cannot be printed around the call, because the scanning and
// the counting overlap and these figures are known part way through.
fn announce_traversal(config: &Configuration, scan: FilesPresent) {
    if scan.relevant_files == 0 {
        return;
    }
    if !config.view.hidden.directory_info && config.view.prints_text() {
        println!("{}\n",crate::theme::get_active().summary.paint(&format!("{} files found. {} of interest. {} excluded.",
                crate::number_formatter::format_with_separators(scan.total_files), crate::number_formatter::format_with_separators(scan.relevant_files),
                crate::number_formatter::format_with_separators(scan.excluded_files))));
    }
    if !config.view.hidden.parsing_info && config.view.prints_text() {
        println!("{}...",crate::theme::get_active().heading.paint("Parsing files"));
    }
}

// One at a time, each with the names it is closest to: one line for the whole list leaves nothing to
// attach a suggestion to. The filtering itself belongs to the run, so a caller that is not this
// binary gets the same selection; what is left here is the color and the correction.
fn report_unknown_languages(languages_available: &[Language], languages_of_interest: &[String])
        -> Result<Option<String>, String>
{
    let unknown = mezura_core::languages::find_unknown_language_names(languages_available, languages_of_interest);

    // Deduplicated for the same reason the list of supported languages is: an installation holding
    // two files that declare one name is one language however many files describe it, and offering
    // the same correction twice reads as two different things to try.
    let mut all_names = languages_available.iter().map(|x| x.name.clone()).collect::<Vec<_>>();
    all_names.sort_by_key(|x| x.to_lowercase());
    all_names.dedup();
    let candidates = all_names.iter().map(String::as_str).collect::<Vec<_>>();

    // Only the mistake is colored. What to do about it is not an error, it is the way out.
    let mut report = String::with_capacity(100);
    for name in &unknown {
        report.push_str(&format!("\n{}", crate::theme::get_active().warning.paint(&format!("'{name}' does not exist as a language file."))));
        if let Some(x) = crate::suggestions::formatted_suggestion(name, &candidates) {
            report.push_str(&format!("\n{x}\n"));
        }
    }

    if unknown.len() == languages_of_interest.len() {
        let headline = crate::theme::get_active().error.paint("None of the provided language names map to valid supported languages.");
        return Err(format!("{headline}\n{report}"));
    }

    Ok(if report.is_empty() {None} else {Some(report)})
}

// An installation made by an earlier version has no such file, and the baked-in copy is not used as
// a substitute: the user is meant to edit the one on disk, and reading a different one would make
// their edits look like they had no effect. It is written by the same pass that writes everything
// else, and until it is there every contested extension simply announces its tiebreak.
fn read_extension_priority() -> (PriorityRules, Vec<String>) {
    mezura_core::language_file::parse_priority_file(&(crate::paths::PERSISTENT_APP_PATHS.data_dir.clone() + EXTENSION_PRIORITY_FILE_NAME))
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

fn read_args_as_str() -> Option<String> {
    let args = std::env::args().skip(1)
            .filter_map(|arg| crate::args::get_trimmed_if_not_empty(&arg))
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
fn handle_message_only_command(args_str: &str, languages_available: &[Language]) -> Option<ExitCode> {
    let is_present = |command: &str| crate::args::find_command(args_str, command).is_some();
    let message_command = [HELP, VERSION, CHANGELOG, SHOW_LANGUAGES, SHOW_CONFIGS, SHOW_THEMES,
            THEME_EDITOR, RESTORE].into_iter().find(|x| is_present(x))?;

    // Refused rather than answered, and before the banner below, which would otherwise be the first
    // thing written. The redirection that makes '--output json' worth asking for is what makes this
    // dangerous: 'mezura --output json --help > stats.json' leaves a file named for a document that
    // holds a help text, and nothing says it is not one until something tries to parse it. The two
    // can only have been typed together, since a configuration file may not declare '--output'.
    if asks_for_a_json_document(args_str) {
        eprintln!("\n{}\n", crate::theme::get_active().error.paint(&crate::message_printer::wrap_message(&format!(
                "'--{message_command}' prints a message to read and '--output json' writes a document for a \
program to read, and both of them go to the output, so only one of the two can be asked for at a time."))));
        return Some(ExitCode::FAILURE);
    }

    // '--version' prints the line itself, with the release date next to it, so it is answered before
    // the plain banner that every other message-only command opens with. With '--help' next to it,
    // the question is about the command and belongs to the help.
    if is_present(VERSION) && !is_present(HELP) {
        crate::message_printer::print_version();
        return Some(ExitCode::SUCCESS);
    }
    println!("\n{}", crate::theme::get_active().version.paint(VERSION_ID));

    if is_present(HELP) {
        crate::message_printer::print_help_message_for_given_args(args_str);
        return Some(ExitCode::SUCCESS);
    } else if let Some(pos) = crate::args::find_command(args_str, CHANGELOG) {
        return match args_str[pos + CHANGELOG.len() + 2..].split_whitespace().next() {
            Some("full") => {
                crate::message_printer::print_changelog(true);
                Some(ExitCode::SUCCESS)
            },
            Some(arg) if !arg.starts_with("--") => {
                println!("\n{}", config_manager::ArgParsingError::IncorrectCommandArgs(CHANGELOG.to_owned()).format());
                crate::message_printer::print_help_message_for_command(CHANGELOG);
                Some(ExitCode::FAILURE)
            },
            _ => {
                crate::message_printer::print_changelog(false);
                Some(ExitCode::SUCCESS)
            },
        };
    } else if is_present(SHOW_LANGUAGES) {
        crate::message_printer::print_supported_languages(languages_available);
        // The list is one line per language and not one per file, so two files declaring one name
        // collapse into a single entry. This command is where somebody looks to find that out, and
        // it returns before the run that would otherwise report it.
        for warning in mezura_core::languages::find_duplicate_names(languages_available) {
            crate::warning_collector::emit(warning);
        }
        return Some(ExitCode::SUCCESS);
    } else if is_present(SHOW_CONFIGS) {
        crate::message_printer::print_existing_configs();
        return Some(ExitCode::SUCCESS);
    } else if is_present(RESTORE) {
        // The same pass a changed binary performs, asked for by hand: useful when something was
        // damaged while the binary stayed the same, where nothing would otherwise trigger it. This
        // is the only run that does not perform it on the way in, which is what leaves this report
        // something to describe.
        let outcome = migrate_data_files(&crate::paths::PERSISTENT_APP_PATHS.data_dir, true);

        if outcome.did_nothing() {
            println!("\nEverything that ships with mezura is in place.");
        }
        for message in [outcome.format_restored(), outcome.format_replaced(), outcome.format_withdrawn(),
                outcome.format_merged(), outcome.format_failures()].into_iter().flatten() {
            println!("{message}");
        }
        // Named here rather than counted, because this is the one command whose whole purpose is to
        // say what the state of the installation was
        for (heading, files) in [("Written for the first time", &outcome.added),
                ("Brought up to date for this version", &outcome.updated)] {
            if !files.is_empty() {
                println!("\n{}", crate::message_printer::wrap_message(&format!(
                        "{heading}:\n{}", files.join(", "))));
            }
        }

        return Some(if outcome.failed.is_empty() {ExitCode::SUCCESS} else {ExitCode::FAILURE});
    } else if is_present(THEME_EDITOR) {
        return match crate::theme_files::generate_theme_editor_page() {
            Ok(path) => {
                println!("\nTheme editor page generated at:\n{path}");
                open_in_browser(&path);
                Some(ExitCode::SUCCESS)
            },
            Err(x) => {
                println!("\n{}", crate::message_printer::wrap_message(
                        &format!("Unable to generate the theme editor page: {x}")).red());
                Some(ExitCode::FAILURE)
            }
        };
    } else if let Some(pos) = crate::args::find_command(args_str, SHOW_THEMES) {
        // The preview follows '--layout' and '--counting', so that what it shows is what a run would
        // print. Read here by hand, because a message-only command runs before there is a
        // configuration to ask.
        let argument_of = |command: &str| crate::args::find_command(args_str, command)
                .and_then(|at| args_str[at + command.len() + 2..].split_whitespace().next());
        let layout = argument_of(LAYOUT).and_then(config_manager::Layout::parse).unwrap_or_default();
        let counting = argument_of(COUNTING).and_then(CountingModel::parse).unwrap_or_default();

        return match args_str[pos + SHOW_THEMES.len() + 2..].split_whitespace().next() {
            Some(arg) if !arg.starts_with("--") => match config_manager::BarThickness::parse(arg) {
                Some(thickness) => {
                    crate::message_printer::print_existing_themes(thickness, layout, counting);
                    Some(ExitCode::SUCCESS)
                },
                None => {
                    println!("\n{}", config_manager::ArgParsingError::IncorrectCommandArgs(SHOW_THEMES.to_owned()).format());
                    crate::message_printer::print_help_message_for_command(SHOW_THEMES);
                    Some(ExitCode::FAILURE)
                }
            },
            _ => {
                crate::message_printer::print_existing_themes(config_manager::BarThickness::default(),
                        layout, counting);
                Some(ExitCode::SUCCESS)
            }
        };
    }

    None
}

// Read off the arguments by hand, the way '--show-themes' reads '--layout' above: a message-only
// command runs before there is a configuration to ask.
fn asks_for_a_json_document(args_str: &str) -> bool {
    crate::args::find_command(args_str, OUTPUT)
            .and_then(|at| args_str[at + OUTPUT.len() + 2..].split_whitespace().next())
            .and_then(OutputFormat::parse) == Some(OutputFormat::Json)
}

#[cfg(test)]
mod tests {
    use mezura_core::Language;
    use crate::report_unknown_languages;

    // What decides the refusal above. A message and a document both want the output, and the whole
    // point of asking for a document is to redirect it, so the message would land in the file.
    #[test]
    fn a_message_and_a_json_document_are_never_asked_for_together() {
        assert!(crate::asks_for_a_json_document("./src --output json"));
        // the value is read the same way the configuration reads it, so the spelling is the same one
        assert!(crate::asks_for_a_json_document("--output JSON --help"));
        assert!(!crate::asks_for_a_json_document("./src --output text --help"));
        assert!(!crate::asks_for_a_json_document("./src --help"));
        // a value that is not one of the two is the configuration's mistake to report, not this one
        assert!(!crate::asks_for_a_json_document("./src --output jsonn --help"));
        // and '--output' written last of all is read without falling off the end of the line
        assert!(!crate::asks_for_a_json_document("./src --output"));
    }

    fn java_and_csharp() -> Vec<Language> {
        vec![Language::new("Java", [""; 0], ["\""], [""; 0], &[], []),
             Language::new("C#", [""; 0], ["\""], [""; 0], &[], [])]
    }

    // The counterpart of this, that the list really is narrowed, is asserted next to the run, which
    // is where the narrowing happens now. What is left here is the part a person reads.
    #[test]
    fn test_report_unknown_languages() {
        let available = java_and_csharp();

        // every name exists, so there is nothing to say
        assert!(report_unknown_languages(&available, &["java".to_owned()]).unwrap().is_none());

        // one of the three is real, so the other two are reported and the run goes on
        let some_unknown = ["java".to_owned(), "c++".to_owned(), "Rust".to_owned()];
        assert!(report_unknown_languages(&available, &some_unknown).unwrap().is_some());

        // none of them is, and that stops the run
        let all_unknown = ["c++".to_owned(), "Rust".to_owned()];
        assert!(report_unknown_languages(&available, &all_unknown).is_err());
    }

    // An installation that has been given a second file declaring a name it already had is one
    // language described twice, and the correction offered for a misspelling of it has to say so
    // once. The list used to arrive as a map, which deduplicated it without anybody deciding to.
    #[test]
    fn a_language_declared_by_two_files_is_suggested_once() {
        let mut available = java_and_csharp();
        available.push(Language::new("Java", [""; 0], ["\""], [""; 0], &[], []));

        let report = report_unknown_languages(&available, &["jaava".to_owned(), "C#".to_owned()])
                .unwrap().expect("a misspelling was not reported at all");
        assert_eq!(1, report.matches("Java").count(), "'Java' was offered more than once:\n{report}");
    }
}
