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
use mezura_core::{CountingModel, FilesPresent, LANGUAGE_CONFLICTS_FILE_NAME, Language};
use mezura_core::language_file::ConflictRules;

use crate::config_manager::{Configuration, OutputFormat};
use crate::config_manager::{CHANGELOG, COUNTING, HELP, LAYOUT, OUTPUT, RESTORE, SHOW_CONFIGS,
        SHOW_LANGUAGES, SHOW_THEMES, THEME_EDITOR, VERSION, VERSION_ID};
use crate::message_printer::Formatted;
use crate::migration::{MigrationOutcome, migrate_data_files};

fn main() -> ExitCode {
    // Dropped last of all: a '--diff' removes its temporary checkouts on background threads, and
    // exiting before they finish would leave a half-deleted tree in the temp directory
    let _removals = crate::animated_display::RemovalsGuard;

    // Started here and not at the scan, so the footer answers for the whole command: a '--diff' pays
    // for checkouts and baseline counting long before any scan of this run begins
    let instant = Instant::now();

    // Windows needs a virtual terminal enabled before it shows colors at all
    #[cfg(target_os = "windows")]
    control::set_virtual_terminal(true).unwrap();

    // Empty when nothing was typed, and never './': a target invented here would be a typed target,
    // and a typed target beats the one a configuration names, so no configuration could supply the
    // targets of a run that names none. The working directory answers for that run inside
    // 'create_config_builder_from_args', after every configuration has had its say.
    let args_str = read_args_as_str().unwrap_or_default();

    // Before the languages are read, or the run that performs it counts with the old files and the
    // change takes two runs to arrive. Skipped for '--restore', which performs this same pass
    // itself and exists in order to report it.
    let restore_was_asked_for = crate::args::find_command(&args_str, RESTORE).is_some();
    let outcome = if restore_was_asked_for {MigrationOutcome::default()}
            else {migrate_data_files(&crate::paths::PERSISTENT_APP_PATHS.data_dir, false)};
    for message in [outcome.format_restored(), outcome.format_replaced(), outcome.format_updated(),
            outcome.format_withdrawn(), outcome.format_merged(), outcome.format_failures()].into_iter().flatten() {
        eprintln!("{message}");
    }

    // The pass above just wrote the directory and is the only thing that knows whether it is whole,
    // so nothing here asks the same question a second time
    let languages_available = if outcome.every_language_file_is_in_place() {
        match mezura_core::language_file::parse_languages_in_dir(&crate::paths::PERSISTENT_APP_PATHS.languages_dir) {
            // A directory that cannot be read is not a reason to refuse to run, and refusing would
            // be worst of all for the command that repairs it: '--restore' does not perform the
            // startup pass, so a deleted 'languages' folder would kill the run right here.
            Err(x) => {
                // Not offered to somebody who is already running it, who is two lines away from the
                // report of the restore this same run is about to perform
                let way_out = if restore_was_asked_for {String::new()}
                        else {format!("\nRun with '--{RESTORE}' to write them now. \
Your configurations, themes and logs are left alone.")};
                eprintln!("\n{}\n", crate::message_printer::wrap_message(&format!(
                        "{}\nCounting with the copies inside the program until it is there again.{way_out}",
                        x.format())).yellow());
                mezura_core::languages::parse_shipped_languages()
            },
            Ok((parsed, faulty_files)) => {
                if !faulty_files.is_empty() {
                    eprintln!("{}", crate::message_printer::wrap_message(
                            &crate::message_printer::format_faulty_language_files_message(&faulty_files)).yellow());
                    // One warning per file and not one for the list, since each is a whole language
                    // whose files went uncounted and the document has to name which
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
    // understand colors and keeps them, the bare 'dumb' cannot show those either. The removals guard
    // built at the top of main reads the answer from where the setter leaves it, since there was no
    // configuration to ask when it was made.
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

    // Also never silently, and to the error output because it is about this machine and not about
    // the count: a variable set weeks ago and forgotten hides every saved configuration, theme and
    // log, and the run otherwise looks like a fresh installation with no explanation on screen.
    if crate::paths::PERSISTENT_APP_PATHS.named_by_the_environment {
        eprintln!("\n{}", crate::theme::get_active().note.paint(&crate::message_printer::wrap_message(
                &format!("{} names the data directory, so this run reads its languages, themes and \
configurations from '{}' and not from the usual place.", crate::paths::DATA_DIR_VARIABLE,
                crate::paths::PERSISTENT_APP_PATHS.data_dir))));
    }

    // Never silently. A folder in some directory above the one being counted decided what this run
    // measures, and without this line the only way to find out is to go looking for it.
    if let Some(local) = &config.view.local_dir
        && local.configuration_applied && !config.view.hidden.directory_info && config.view.prints_text() {
        let opening = if config.view.hidden.version {"\n"} else {""};
        println!("{opening}{}", crate::theme::get_active().note.paint(
                &format!("Using the settings of this project, from '{}'.", local.get_config_path())));
    }

    // The two lists of the run itself, and not those of a module: one module counting nothing is a
    // row of zeroes in the report, where the run counting nothing is a run worth stopping.
    let wanted = config.engine.languages_of_interest.get_of_the_whole_run();
    if !wanted.is_empty()
            && wanted.iter().all(|lang| config.engine.excluded_languages.get_of_the_whole_run().contains(lang)) {
        eprintln!("\n{}\n",crate::theme::get_active().error.paint(
                "Every language named in '--languages' is also named in '--exclude-languages', so nothing would be left to count."));
        return ExitCode::FAILURE;
    }

    if !config.engine.languages_of_interest.is_empty() {
        match report_unknown_languages(&languages_available, &config.engine.languages_of_interest.get_all_names()) {
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

    let (conflict_rules, faulty_conflict_lines) = read_conflict_rules();
    if !faulty_conflict_lines.is_empty() {
        eprintln!("\n{}", crate::message_printer::wrap_message(
                &format!("Lines that could not be read in '{LANGUAGE_CONFLICTS_FILE_NAME}', and were skipped:\n{}",
                faulty_conflict_lines.join("\n"))).yellow());
        for line in &faulty_conflict_lines {
            crate::warning_collector::keep(mezura_core::warnings::Warning::new(mezura_core::warnings::Code::ConflictLineSkipped, line,
                    format!("'{line}' could not be read in '{LANGUAGE_CONFLICTS_FILE_NAME}' and was skipped, so any contest it was meant to settle was left to the tiebreak.")));
        }
    }

    // Above the language resolution, whose selection a document's adopted settings can change, and
    // below the conflict rules, which a side that is a revision is counted with
    let baseline_only = match crate::diff::DiffRequest::of(&mut config, &languages_available) {
        Ok(Some(crate::diff::DiffRequest::BetweenTwoReadings(both))) => {
            return match both.into_comparison(&config, &conflict_rules) {
                Ok(comparison) => {
                    if config.view.prints_text() {
                        println!();
                    }
                    crate::present::print_comparison_as_text_or_json(&comparison, &chrono::Local::now(), &config);
                    // The exec time alone, without the parsing figures: those describe one scan, and
                    // a comparison had up to two
                    if config.view.prints_text() && !config.view.hidden.timing {
                        println!("\n{}", crate::theme::get_active().footer.paint(&format_exec_time(&instant)));
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

    let (languages, reported) = mezura_core::Languages::resolve(&config.engine, languages_available, &conflict_rules);
    crate::warning_collector::report_language_resolution_warnings(reported);

    // Its own answer entirely: no scan, no report, no log.
    if config.view.explain.is_some() {
        return crate::explain::run_explain(&config, languages);
    }

    // Above the run, so that a baseline which turns out not to be one costs no scan of the tree
    let counted_baseline = match baseline_only.map(|x| x.count_baseline(&config, &conflict_rules)) {
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
    let outcome = mezura_core::run_watched(&config.engine, languages, Some(progress.clone()), |scan| {
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
            // Already presented above as the failures they are. The exit code keeps its meaning:
            // 1 is a run that did not happen, every file unparseable or every place unopenable.
            if result.all_relevant_files_were_faulty() || result.nothing_could_be_read() {
                return ExitCode::FAILURE;
            }
            // The document has its own 'scan_ms' measured inside the run; this is the only place
            // that knows what the whole command took. A run that found nothing prints no timing.
            if !config.view.hidden.timing && config.view.prints_text() && result.files_present.relevant_files > 0 {
                let perf = format_exec_time(&instant);
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
        // A bad target surfaces here, because the run is what resolves the declared ones. The
        // wording is this crate's own, so that a configuration file which supplied the targets is
        // named as the culprit the reader cannot see failing.
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

// The two lines between the phases. They cannot be printed around the call, because the scanning
// and the counting overlap and these figures are known part way through.
fn announce_traversal(config: &Configuration, scan: FilesPresent) {
    if scan.relevant_files == 0 {
        return;
    }
    if !config.view.hidden.directory_info && config.view.prints_text() {
        let word = if scan.total_files == 1 {"file"} else {"files"};
        println!("{}\n",crate::theme::get_active().summary.paint(&format!("{} {word} found. {} of interest. {} excluded.",
                crate::number_formatter::format_with_separators(scan.total_files), crate::number_formatter::format_with_separators(scan.relevant_files),
                crate::number_formatter::format_with_separators(scan.excluded_files))));
    }
    if !config.view.hidden.parsing_info && config.view.prints_text() {
        println!("{}...",crate::theme::get_active().heading.paint("Parsing files"));
    }
}

// One name at a time, each with the names it is closest to: one line for the whole list leaves
// nothing to attach a suggestion to. The filtering itself belongs to the run, so what is left here
// is the color and the correction.
fn report_unknown_languages(languages_available: &[Language], languages_of_interest: &[String])
        -> Result<Option<String>, String>
{
    let unknown = mezura_core::languages::find_unknown_language_names(languages_available, languages_of_interest);

    // An installation holding two files that declare one name is one language however many files
    // describe it, and offering the same correction twice reads as two things to try.
    let mut all_names = languages_available.iter().map(|x| x.name.clone()).collect::<Vec<_>>();
    all_names.sort_by_key(|x| x.to_lowercase());
    all_names.dedup();
    let candidates = all_names.iter().map(String::as_str).collect::<Vec<_>>();

    let mut report = String::with_capacity(100);
    for name in &unknown {
        report.push_str(&format!("\n{}", crate::theme::get_active().warning.paint(
                &format!("'{name}' is not a language this installation knows."))));
        if let Some(x) = crate::suggestions::formatted_suggestion(name, &candidates) {
            report.push_str(&format!("\n{x}\n"));
        }
    }

    if unknown.len() == languages_of_interest.len() {
        let headline = crate::theme::get_active().error.paint(
                "None of the languages you named are ones this installation knows, so there would be nothing to count.");
        return Err(format!("{headline}\n{report}"));
    }

    Ok(if report.is_empty() {None} else {Some(report)})
}

// Only the file on disk is read, never the baked-in copy: that file is the one the user is meant to
// edit, and reading a different one would make their edits look like they had no effect. Until the
// migration pass has written it, every contested extension simply announces its tiebreak.
fn read_conflict_rules() -> (ConflictRules, Vec<String>) {
    mezura_core::language_file::parse_conflict_rules_file(
            &(crate::paths::PERSISTENT_APP_PATHS.data_dir.clone() + LANGUAGE_CONFLICTS_FILE_NAME))
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

// These commands take no configuration, so nothing could hide the version line from them, and they
// are the only place the version of an installed binary can be read. 'None' means the arguments are
// not one of them and not that nothing went wrong: three of the branches below report a mistake and
// stop the run with a failure.
fn handle_message_only_command(args_str: &str, languages_available: &[Language]) -> Option<ExitCode> {
    let is_present = |command: &str| crate::args::find_command(args_str, command).is_some();
    let message_command = [HELP, VERSION, CHANGELOG, SHOW_LANGUAGES, SHOW_CONFIGS, SHOW_THEMES,
            THEME_EDITOR, RESTORE].into_iter().find(|x| is_present(x))?;

    // Refused rather than answered, and before the banner below, which would otherwise be the first
    // thing written: 'mezura --output json --help > stats.json' leaves a file named for a document
    // that holds a help text, and nothing says it is not one until something tries to parse it.
    if asks_for_a_json_document(args_str) {
        eprintln!("\n{}\n", crate::theme::get_active().error.paint(&crate::message_printer::wrap_message(&format!(
                "'--{message_command}' prints a message to read and '--output json' writes a document for a \
program to read, and both of them go to the output, so only one of the two can be asked for at a time."))));
        return Some(ExitCode::FAILURE);
    }

    // '--version' prints the line itself, with the release date next to it, so it is answered before
    // the plain banner every other message-only command opens with. With '--help' beside it, the
    // question is about the command and belongs to the help.
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
                println!("\n{}", config_manager::ArgParsingError::IncorrectCommandArgs(CHANGELOG.to_owned(), arg.to_owned()).format());
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
        // collapse into a single entry. Reported here because this command returns before the run
        // that would otherwise say it.
        for warning in mezura_core::languages::find_duplicate_names(languages_available) {
            crate::warning_collector::emit(warning);
        }
        return Some(ExitCode::SUCCESS);
    } else if is_present(SHOW_CONFIGS) {
        crate::message_printer::print_existing_configs();
        return Some(ExitCode::SUCCESS);
    } else if is_present(RESTORE) {
        // The same pass a changed binary performs, asked for by hand, for when something was damaged
        // while the binary stayed the same. This is the only run that does not perform it on the way
        // in, which is what leaves this report something to describe.
        let outcome = migrate_data_files(&crate::paths::PERSISTENT_APP_PATHS.data_dir, true);

        if outcome.did_nothing() {
            println!("\nEverything that ships with mezura is in place.");
        }
        for message in [outcome.format_restored(), outcome.format_replaced(), outcome.format_withdrawn(),
                outcome.format_merged(), outcome.format_failures()].into_iter().flatten() {
            println!("{message}");
        }
        for (heading, files) in [("Written for the first time", &outcome.added),
                ("Brought up to date for this version", &outcome.updated)] {
            if !files.is_empty() {
                println!("\n{}", crate::message_printer::wrap_message(&format!(
                        "{heading}:\n{}", files.join(", "))));
            }
        }

        return Some(if outcome.failed.is_empty() {ExitCode::SUCCESS} else {ExitCode::FAILURE});
    } else if is_present(THEME_EDITOR) {
        return match crate::theme_files::generate_theme_editor_page(
                &crate::paths::PERSISTENT_APP_PATHS.themes_dir, &crate::paths::PERSISTENT_APP_PATHS.data_dir) {
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
                    println!("\n{}", config_manager::ArgParsingError::IncorrectCommandArgs(SHOW_THEMES.to_owned(), arg.to_owned()).format());
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

fn asks_for_a_json_document(args_str: &str) -> bool {
    crate::args::find_command(args_str, OUTPUT)
            .and_then(|at| args_str[at + OUTPUT.len() + 2..].split_whitespace().next())
            .and_then(OutputFormat::parse) == Some(OutputFormat::Json)
}

#[cfg(test)]
mod tests {
    use mezura_core::{Language, StringRules};
    use crate::report_unknown_languages;

    #[test]
    fn a_message_and_a_json_document_are_never_asked_for_together() {
        assert!(crate::asks_for_a_json_document("./src --output json"));
        assert!(crate::asks_for_a_json_document("--output JSON --help"));
        assert!(!crate::asks_for_a_json_document("./src --output text --help"));
        assert!(!crate::asks_for_a_json_document("./src --help"));
        assert!(!crate::asks_for_a_json_document("./src --output jsonn --help"));
        // '--output' written last of all, with nothing to read after it
        assert!(!crate::asks_for_a_json_document("./src --output"));
    }

    fn java_and_csharp() -> Vec<Language> {
        vec![Language::new("Java", [""; 0], StringRules::escaping_nothing(), [""; 0], &[], []),
             Language::new("C#", [""; 0], StringRules::escaping_nothing(), [""; 0], &[], [])]
    }

    #[test]
    fn a_language_name_nothing_claims_is_reported_and_only_all_of_them_stops_the_run() {
        let available = java_and_csharp();

        assert!(report_unknown_languages(&available, &["java".to_owned()]).unwrap().is_none());

        let some_unknown = ["java".to_owned(), "c++".to_owned(), "Rust".to_owned()];
        assert!(report_unknown_languages(&available, &some_unknown).unwrap().is_some());

        // none of the names is real, and that stops the run
        let all_unknown = ["c++".to_owned(), "Rust".to_owned()];
        assert!(report_unknown_languages(&available, &all_unknown).is_err());
    }

    #[test]
    fn a_language_declared_by_two_files_is_suggested_once() {
        let mut available = java_and_csharp();
        available.push(Language::new("Java", [""; 0], StringRules::escaping_nothing(), [""; 0], &[], []));

        let report = report_unknown_languages(&available, &["jaava".to_owned(), "C#".to_owned()])
                .unwrap().expect("a misspelling was not reported at all");
        assert_eq!(1, report.matches("Java").count(), "'Java' was offered more than once:\n{report}");
    }
}
