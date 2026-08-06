// Turning a result into something a person reads. Kept apart from the run itself, so that counting
// is a function of its inputs and a caller which wants the numbers and not the report never comes
// near any of this.
use mezura_core::{FaultyFileDetails, RunResult, UnreadableDirDetails};

use super::config_manager::Configuration;
use crate::paths::PERSISTENT_APP_PATHS;

// Reads and never writes, so a caller wanting both the numbers and the report can have the same
// result twice.
pub fn present(result: &RunResult, config: &Configuration) {
    let datetime_now = chrono::Local::now();
    // Before anything else, because a scan can come back empty precisely because the directories
    // could not be opened, and the report would otherwise say "no relevant files" with a straight face
    print_unreadable_dirs(&result.unreadable_dirs, config);

    if result.files_present.relevant_files == 0 {
        // Nothing was parsed, so the unreadable directories are the whole of what there is to offer
        // detail on, and this is the one path where the hint has to come before the sentence below
        print_detail_hint_if_anything_was_hidden(result, config);
        // A machine consumer must not have to tell "no output" apart from "no code found", so the
        // document is written even here, whole and with everything zeroed
        if config.view.prints_text() {
            // Worded here and not in the library, because 'run' answers this case with a result and
            // not an error: the sentence is presentation, like every other sentence
            let activated = get_activated_languages_as_str(config);
            let message = if activated.is_empty() {"No relevant files found in the given directory.".to_owned()}
                    else {format!("No relevant files found in the given directory. {activated}")};
            eprintln!("{}", super::theme::active().warning.paint(&message));
        } else {
            super::json_printer::print_as_json(result, &datetime_now, config);
        }
        return;
    }

    // No table of zeros under a real failure, because a table reads as an answer. The document is
    // still written whole, for the same reason the empty scan writes one. Returning here also keeps
    // the run out of the log, where a row of zeros makes the next comparison report a collapse and
    // then a recovery.
    if result.all_relevant_files_were_faulty() {
        print_faulty_files_or_ok(&result.faulty_files, config);
        print_detail_hint_if_anything_was_hidden(result, config);
        if config.view.prints_text() {
            eprintln!("{}", super::theme::active().warning.paint("None of the files were able to be parsed"));
        } else {
            super::json_printer::print_as_json(result, &datetime_now, config);
        }
        return;
    }

    print_faulty_files_or_ok(&result.faulty_files, config);
    print_detail_hint_if_anything_was_hidden(result, config);

    if !config.view.prints_text() {
        super::json_printer::print_as_json(result, &datetime_now, config);
        return;
    }

    let log_file_path = get_specified_config_file_path(config);
    let existing_log_contents = log_file_path.as_ref().and_then(|path| super::log::extract_file_contents(path));
    super::result_printer::format_and_print_results(result, &existing_log_contents, &datetime_now, config);

    // The reason travels with the warning, because the two that can happen are opposite news: one
    // says this run was not recorded, the other says this run was not recorded and everything
    // already in there was kept rather than replaced by it.
    if config.view.log.should_log && let Some(path) = log_file_path
        && let Err(reason) = super::log::log_stats(&path, &existing_log_contents, result, &datetime_now, config) {
        eprintln!("\n{}",super::theme::active().warning.paint(&format!("Error while trying to save the log: {reason}")));
    }
}

// Hiding the status never hides a parsing failure: that would show wrong numbers with nothing
// to indicate it
pub fn print_faulty_files_or_ok(faulty_files: &[FaultyFileDetails], config: &Configuration) {
    if faulty_files.is_empty() {
        if !config.view.hidden.parsing_info && config.view.prints_text() {
            println!("{}\n",super::theme::active().success.paint("ok"));
        }
    } else {
        // A JSON run reports them inside the document as well, but they are a mistake and belong on
        // the error output in every case, where '--hide' can never suppress them
        let error = &super::theme::active().error;
        let (count, subject, pronoun) = (faulty_files.len(),
                if faulty_files.len() == 1 {"faulty file"} else {"faulty files"},
                if faulty_files.len() == 1 {"It"} else {"They"});
        eprintln!("{} {}", error.paint(&count.to_string()),
                error.paint(&format!("{subject} detected. {pronoun} will be ignored in stat calculation.")));
        if config.view.should_show_faulty_files {
            for f in faulty_files {
                eprintln!("-- Error: {} \n   for file: {}\n",f.error_msg,f.path);
            }
        }
        eprintln!();
    }
}

// In the error colour and not a milder one, because it is the worse of the two problems: a faulty
// file is at least counted among the files that were found, while everything under one of these
// appears in no total at all.
fn print_unreadable_dirs(unreadable_dirs: &[UnreadableDirDetails], config: &Configuration) {
    if unreadable_dirs.is_empty() {return;}

    let error = &super::theme::active().error;
    let (count, subject, pronoun) = (unreadable_dirs.len(),
            if unreadable_dirs.len() == 1 {"directory"} else {"directories"},
            if unreadable_dirs.len() == 1 {"it"} else {"them"});
    eprintln!("{} {}", error.paint(&count.to_string()),
            error.paint(&format!("{subject} could not be read. Nothing inside {pronoun} was counted.")));
    // The reason beside the path and not on a line of its own, because on a whole drive this list
    // runs to hundreds and most of them say the same thing
    if config.view.should_show_faulty_files {
        for dir in unreadable_dirs {
            eprintln!("-- Could not be read ({}):
   {}
", dir.error_msg, dir.path);
        }
    }
    eprintln!();
}

fn print_detail_hint_if_anything_was_hidden(result: &RunResult, config: &Configuration) {
    if let Some(hint) = detail_hint(result, config) {
        eprintln!("{hint}");
    }
}

// Once for the run and not once per kind of problem: it is the same flag either way, and a scan that
// meets both would otherwise tell the reader twice to do one thing.
//
// Split from the printing so the decision can be asserted, which is the one thing worth asserting
// here: three paths through 'present' reach it with a different pair of lists behind them.
fn detail_hint(result: &RunResult, config: &Configuration) -> Option<String> {
    if config.view.should_show_faulty_files
        || (result.unreadable_dirs.is_empty() && result.faulty_files.is_empty()) {
        return None;
    }

    Some(format!("Run with command '--{}' to get detailed info.\n", super::config_manager::SHOW_FAULTY_FILES))
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

fn get_specified_config_file_path(config: &Configuration) -> Option<String> {
    if let Some(name) = &config.view.config_name_to_save {
        Some(PERSISTENT_APP_PATHS.logs_dir.clone() + name)
    } else { config.view.config_name_to_load.as_ref().map(|name|PERSISTENT_APP_PATHS.logs_dir.clone() + name) }
}

#[cfg(test)]
mod tests {
    use mezura_core::{FilesPresent, Performance, Stats, Threads};

    use super::*;

    fn result_with(unreadable: usize, faulty: usize) -> RunResult {
        RunResult {
            per_language: std::collections::HashMap::new(), total: Stats::default(),
            modules: Vec::new(), targets: Vec::new(),
            files_present: FilesPresent {total_files: 1, relevant_files: 1, excluded_files: 0},
            performance: Performance {duration_millis: 0, threads: Threads::new(1, 1)},
            faulty_files: (0..faulty).map(|i| mezura_core::FaultyFileDetails::new(
                    format!("a{i}.rs"), "no".to_owned(), 1)).collect(),
            unreadable_dirs: (0..unreadable).map(|i| UnreadableDirDetails::new(
                    format!("D:/d{i}"), "Access is denied. (os error 5)".to_owned())).collect()
        }
    }

    // A scan that meets both kinds of problem prints two counts, and each of them used to name the
    // same command underneath it, so the reader was told twice to run one flag. The hint belongs to
    // the run and not to either list, which is what the first case holds.
    #[test]
    fn the_offer_of_detail_is_made_once_however_many_kinds_of_problem_there_were() {
        let mut config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);

        let both = detail_hint(&result_with(3, 5), &config).expect("a run with both kinds said nothing");
        assert_eq!(1, both.matches("--show-faulty-files").count(), "the same flag was named twice:\n{both}");

        // and it is offered for either kind on its own, since the one flag shows both
        assert!(detail_hint(&result_with(3, 0), &config).is_some(), "only unreadable directories said nothing");
        assert!(detail_hint(&result_with(0, 5), &config).is_some(), "only faulty files said nothing");

        // Nothing to show, nothing offered. A clean run must not carry a line telling the reader to
        // go and look at problems it did not have.
        assert!(detail_hint(&result_with(0, 0), &config).is_none(), "a clean run offered detail on nothing");

        // and the offer is gone once it has been taken up, or it would point at the output above it
        config.view.should_show_faulty_files = true;
        assert!(detail_hint(&result_with(3, 5), &config).is_none(),
                "the detail was printed and the reader was still told to ask for it");
    }
}
