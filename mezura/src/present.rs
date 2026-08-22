use mezura_core::{FaultyFileDetails, RunResult, UnreadableDirDetails};

use super::config_manager::Configuration;
use crate::paths::PERSISTENT_APP_PATHS;

pub fn present(result: &RunResult, comparison: Option<&super::diff::Comparison>, config: &Configuration) {
    let datetime_now = chrono::Local::now();
    // Before anything else: a scan can come back empty precisely because the directories could not
    // be opened, and the report would otherwise say "no relevant files" with a straight face
    print_unreadable_dirs(&result.unreadable_dirs, config);

    if result.files_present.relevant_files == 0 {
        // The one path where the hint comes before the sentence below rather than after it
        print_detail_hint_if_anything_was_hidden(result, config);
        // The document is written even here, whole and zeroed: a machine consumer must not have to
        // tell "no output" apart from "no code found"
        if config.view.prints_text() {
            let activated = get_activated_languages_as_str(config);
            let message = if activated.is_empty() {"No relevant files found in the given directory.".to_owned()}
                    else {format!("No relevant files found in the given directory. {activated}")};
            eprintln!("{}", super::theme::get_active().warning.paint(&message));
            if comparison.is_some() {
                println!();
            }
        }
        print_comparison_or_empty_document(result, comparison, &datetime_now, config);
        return;
    }

    // No table of zeros under a real failure, and no log entry either: a logged row of zeros makes
    // the next comparison report a collapse and then a recovery.
    if result.all_relevant_files_were_faulty() {
        print_faulty_files_or_ok(&result.faulty_files, config);
        print_detail_hint_if_anything_was_hidden(result, config);
        if config.view.prints_text() {
            eprintln!("{}", super::theme::get_active().warning.paint("None of the files could be parsed."));
            if comparison.is_some() {
                println!();
            }
        }
        print_comparison_or_empty_document(result, comparison, &datetime_now, config);
        return;
    }

    print_faulty_files_or_ok(&result.faulty_files, config);
    print_files_left_out(result, config);
    print_detail_hint_if_anything_was_hidden(result, config);

    // Every file found was left out, so no table and no log entry, for the reason the faulty branch
    // above returns.
    if result.nothing_of_interest_was_counted() {
        if config.view.prints_text() {
            eprintln!("{}", super::theme::get_active().warning.paint("Nothing was left to count."));
            if comparison.is_some() {
                println!();
            }
        }
        print_comparison_or_empty_document(result, comparison, &datetime_now, config);
        return;
    }

    // A comparison takes the report's place whole: no report, no history section, no log entry
    if comparison.is_some() || !config.view.prints_text() {
        print_comparison_or_empty_document(result, comparison, &datetime_now, config);
        return;
    }

    let log_file_path = determine_log_file_path(config);
    let existing_log_contents = log_file_path.as_ref().and_then(|path| super::log::extract_file_contents(path));
    super::result_printer::format_and_print_results(result, &existing_log_contents, &datetime_now, config);

    if config.view.log.should_log && let Some(path) = log_file_path
        && let Err(reason) = super::log::log_stats(&path, &existing_log_contents, result, &datetime_now, config) {
        eprintln!("\n{}",super::theme::get_active().warning.paint(&format!("Error while trying to save the log: {reason}")));
    }
}

// '--hide' never hides a parsing failure: the numbers would be lower than the tree with nothing
// to say why. A JSON run has them in the document as well, and still on the error output.
pub fn print_faulty_files_or_ok(faulty_files: &[FaultyFileDetails], config: &Configuration) {
    if faulty_files.is_empty() {
        if !config.view.hidden.parsing_info && config.view.prints_text() {
            println!("{}\n",super::theme::get_active().success.paint("ok"));
        }
    } else {
        let error = &super::theme::get_active().error;
        let (count, subject, pronoun) = (faulty_files.len(),
                if faulty_files.len() == 1 {"file"} else {"files"},
                if faulty_files.len() == 1 {"it is"} else {"they are"});
        eprintln!("{} {}", error.paint(&count.to_string()),
                error.paint(&format!("{subject} could not be parsed, so {pronoun} in no figure below.")));
        if config.view.should_show_faulty_files {
            for f in faulty_files {
                eprintln!("-- Error: {} \n   for file: {}\n",f.error_msg,f.path);
            }
        }
        eprintln!();
    }
}

// On the error output and never hidden, for the reason a faulty file is: the figures are lower than
// the tree and nothing else would say why. A JSON run has it under 'scan'.
fn print_files_left_out(result: &RunResult, config: &Configuration) {
    if !config.view.prints_text() {
        return;
    }
    let mut said_something = false;
    for (count, kind, command) in [(result.minified_files, "minified", crate::config_manager::COUNT_MINIFIED),
            (result.generated_files, "generated", crate::config_manager::COUNT_GENERATED)] {
        if count == 0 {
            continue;
        }
        let subject = if count == 1 {"file was"} else {"files were"};
        eprintln!("{}", super::theme::get_active().summary.paint(&format!(
                "{} {kind} {subject} left out of the counts. Run with '--{command}' to include them.",
                crate::number_formatter::format_with_separators(count))));
        said_something = true;
    }
    if said_something {
        eprintln!();
    }
}

// In the error color and not a milder one: everything under an unreadable directory appears in no
// total at all, where a faulty file is at least counted among the files that were found.
fn print_unreadable_dirs(unreadable_dirs: &[UnreadableDirDetails], config: &Configuration) {
    if unreadable_dirs.is_empty() {return;}

    let error = &super::theme::get_active().error;
    let (count, subject, pronoun) = (unreadable_dirs.len(),
            if unreadable_dirs.len() == 1 {"directory"} else {"directories"},
            if unreadable_dirs.len() == 1 {"it"} else {"them"});
    eprintln!("{} {}", error.paint(&count.to_string()),
            error.paint(&format!("{subject} could not be read. Nothing inside {pronoun} was counted.")));
    if config.view.should_show_faulty_files {
        for dir in unreadable_dirs {
            eprintln!("-- Could not be read ({}):
   {}
", dir.error_msg, dir.path);
        }
    }
    eprintln!();
}

// A scan that found nothing still owes the comparison that was asked for, rather than a plain
// document of zeros.
fn print_comparison_or_empty_document(result: &RunResult, comparison: Option<&super::diff::Comparison>,
        datetime_now: &chrono::DateTime<chrono::Local>, config: &Configuration)
{
    match comparison {
        // The blank line above it is the caller's: only the caller knows what sits there
        Some(comparison) if config.view.prints_text() => super::result_printer::print_comparison(comparison, config),
        Some(comparison) => super::json_printer::print_comparison_as_json(comparison, datetime_now, config),
        None if config.view.prints_text() => (),
        None => super::json_printer::print_as_json(result, datetime_now, config)
    }
}

fn print_detail_hint_if_anything_was_hidden(result: &RunResult, config: &Configuration) {
    if let Some(hint) = determine_detail_hint(result, config) {
        eprintln!("{hint}");
    }
}

// Once for the run and not once per kind of problem: it is the same flag either way, and a scan
// that meets both would otherwise tell the reader twice to do one thing.
fn determine_detail_hint(result: &RunResult, config: &Configuration) -> Option<String> {
    if config.view.should_show_faulty_files
        || (result.unreadable_dirs.is_empty() && result.faulty_files.is_empty()) {
        return None;
    }

    Some(format!("Run with '--{}' for the paths and the reasons.\n", super::config_manager::SHOW_FAULTY_FILES))
}

fn get_activated_languages_as_str(config: &Configuration) -> String {
    let mut msg = if config.engine.languages_of_interest.is_empty() {
        String::new()
    } else {
        String::from("\n(Activated languages: ") + &config.engine.languages_of_interest.join(", ") + ")"
    }
    ;
    let other = if config.engine.excluded_languages.is_empty() {
        String::new()
    } else {
        String::from("\n(Excluded languages: ") + &config.engine.excluded_languages.join(", ") + ")"
    };

    msg += &other;
    msg
}

fn determine_log_file_path(config: &Configuration) -> Option<String> {
    let name = config.view.config_name_to_save.as_ref()
            .or(config.view.config_name_to_load.as_ref())?;

    Some(PERSISTENT_APP_PATHS.logs_dir.clone() + name + ".jsonl")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use mezura_core::{FilesPresent, Performance, Threads};

    use super::*;

    // One file is always counted beside whatever went wrong: a run where nothing at all was counted
    // takes a branch of its own, and this fixture must not stand in both.
    fn result_with(unreadable: usize, faulty: usize) -> RunResult {
        let counted = crate::test_support::plain_stats_of(1, 40, 4, 3, 1, HashMap::new());
        RunResult {
            per_language: HashMap::from([("Rust".to_owned(), counted.clone())]), total: counted,
            modules: Vec::new(), nested_languages: HashMap::new(), targets: Vec::new(),
            files_present: FilesPresent {total_files: 2 + faulty, relevant_files: 1 + faulty, excluded_files: 0},
            performance: Performance {duration_millis: 0, threads: Threads::new(1, 1)},
            faulty_files: (0..faulty).map(|i| mezura_core::FaultyFileDetails::new(
                    format!("a{i}.rs"), "no".to_owned(), 1)).collect(),
            minified_files: 0, generated_files: 0,
            unreadable_dirs: (0..unreadable).map(|i| UnreadableDirDetails::new(
                    format!("D:/d{i}"), "Access is denied. (os error 5)".to_owned())).collect()
        }
    }

    #[test]
    fn the_offer_of_detail_is_made_once_however_many_kinds_of_problem_there_were() {
        let mut config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);

        let both = determine_detail_hint(&result_with(3, 5), &config).expect("a run with both kinds said nothing");
        assert_eq!(1, both.matches("--show-faulty-files").count(), "the same flag was named twice:\n{both}");

        assert!(determine_detail_hint(&result_with(3, 0), &config).is_some(), "only unreadable directories said nothing");
        assert!(determine_detail_hint(&result_with(0, 5), &config).is_some(), "only faulty files said nothing");

        assert!(determine_detail_hint(&result_with(0, 0), &config).is_none(), "a clean run offered detail on nothing");

        config.view.should_show_faulty_files = true;
        assert!(determine_detail_hint(&result_with(3, 5), &config).is_none(),
                "the detail was printed and the reader was still told to ask for it");
    }

    // Asserted through the real 'present', because that is where the write happens or does not.
    #[test]
    fn a_run_that_compares_writes_no_log_entry() {
        let name = "zz-a-run-that-compares";
        std::fs::create_dir_all(&PERSISTENT_APP_PATHS.logs_dir).unwrap();
        let path = std::path::Path::new(&PERSISTENT_APP_PATHS.logs_dir).join(name.to_owned() + ".jsonl");
        let _ = std::fs::remove_file(&path);

        let mut config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);
        config.view.config_name_to_load = Some(name.to_owned());
        config.view.log = crate::config_manager::LogOption::new(None);
        let result = result_with(0, 0);
        let baseline = crate::diff::Reading {
            source: crate::diff::Source::Document { path: "old.json".to_owned() },
            taken: "2026-08-06T10:00:00+03:00".to_owned(),
            version: "3.0.0".to_owned(),
            scope: crate::diff::scope_of(&mezura_core::EngineConfig::default(), mezura_core::CountingModel::Content),
            warnings: Vec::new(),
            faulty_files_count: 0,
            unreadable_dirs_count: 0,
            result: result_with(0, 0)
        };
        let comparison = crate::diff::Comparison::of(baseline,
                crate::diff::Reading::of_this_run(&result, &chrono::Local::now(), &config), &config, Vec::new());

        present(&result, Some(&comparison), &config);
        assert!(!path.exists(), "the comparison run wrote a log entry");

        present(&result, None, &config);
        assert!(path.exists(), "the ordinary run stopped logging");
        std::fs::remove_file(&path).unwrap();
    }

    // Only the file name is asserted: the separators around it differ by platform.
    #[test]
    fn a_logs_file_name_is_the_configuration_name_with_jsonl_on_it() {
        let file_name = |config: &Configuration| determine_log_file_path(config)
                .map(|path| std::path::Path::new(&path).file_name().unwrap().to_string_lossy().into_owned());

        let mut config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);
        assert_eq!(None, file_name(&config), "a run naming no configuration asked for a log file");

        config.view.config_name_to_load = Some("portal".to_owned());
        assert_eq!(Some("portal.jsonl".to_owned()), file_name(&config));

        config.view.config_name_to_save = Some("saved".to_owned());
        assert_eq!(Some("saved.jsonl".to_owned()), file_name(&config),
                "the name being saved did not win over the name being loaded");
    }
}
