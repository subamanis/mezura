// Turning a result into something a person reads. Kept apart from the run itself, so that counting
// is a function of its inputs and a caller which wants the numbers and not the report never comes
// near any of this.
use crate::paths::PERSISTENT_APP_PATHS;
use mezura::{FaultyFileDetails, RunResult};
use super::config_manager::Configuration;

// Everything that turns a result into something a person reads, kept out of 'run' so that the run
// itself is a function of its inputs. A caller that wants the numbers and not the report never calls
// this, and one that wants both gets the same result twice, since presenting reads and never writes.
pub fn present(result: &RunResult, config: &Configuration) {
    let datetime_now = chrono::Local::now();
    // Before anything else, because a scan can come back empty precisely because the directories
    // could not be opened, and the report would otherwise say "no relevant files" with a straight face
    print_unreadable_dirs(&result.unreadable_dirs, config);

    if result.files_present.relevant_files == 0 {
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

    // Every file failing to parse is presented as the failure it is, and the report of zeros the
    // ordinary path would print under it is not printed: a table of nothing under a real failure
    // reads as an answer. The document is still written whole, faulty files and warnings included,
    // for the same reason the empty scan writes one. Returning here also keeps this run out of the
    // log, where a row of zeros would make the next comparison report a collapse and a recovery.
    if result.all_relevant_files_were_faulty() {
        print_faulty_files_or_ok(&result.faulty_files, config);
        if config.view.prints_text() {
            eprintln!("{}", super::theme::active().warning.paint("None of the files were able to be parsed"));
        } else {
            super::json_printer::print_as_json(result, &datetime_now, config);
        }
        return;
    }

    print_faulty_files_or_ok(&result.faulty_files, config);

    if !config.view.prints_text() {
        super::json_printer::print_as_json(result, &datetime_now, config);
        return;
    }

    let log_file_path = get_specified_config_file_path(config);
    let existing_log_contents = log_file_path.as_ref().and_then(|path| super::log::extract_file_contents(path));
    super::result_printer::format_and_print_results(result, &existing_log_contents, &datetime_now, config);

    if config.view.log.should_log && let Some(path) = log_file_path
        && super::log::log_stats(&path, &existing_log_contents, result, &datetime_now, config).is_err() {
        eprintln!("\n{}",super::theme::active().warning.paint("Error while trying to save the log."));
    }
}

// The same kind of problem as a file that will not parse, and told in the same shape: how many, and
// the same command for the detail. In the error colour and not a milder one because it is the worse
// of the two, since a faulty file is at least counted among the files that were found while
// everything under one of these appears in no total at all.
fn print_unreadable_dirs(unreadable_dirs: &[String], config: &Configuration) {
    if unreadable_dirs.is_empty() {return;}

    let error = &super::theme::active().error;
    let (count, subject, pronoun) = (unreadable_dirs.len(),
            if unreadable_dirs.len() == 1 {"directory"} else {"directories"},
            if unreadable_dirs.len() == 1 {"it"} else {"them"});
    eprintln!("{} {}", error.paint(&count.to_string()),
            error.paint(&format!("{subject} could not be read. Nothing inside {pronoun} was counted.")));
    if config.view.should_show_faulty_files {
        for dir in unreadable_dirs {
            eprintln!("-- Could not be read:
   {dir}
");
        }
    } else {
        eprintln!("Run with command '--{}' to get detailed info.", super::config_manager::SHOW_FAULTY_FILES);
    }
    eprintln!();
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
        } else {
            eprintln!("Run with command '--{}' to get detailed info.",super::config_manager::SHOW_FAULTY_FILES)
        }
        eprintln!();
    }
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
