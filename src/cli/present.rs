// Turning a result into something a person reads. Kept apart from the run itself, so that counting
// is a function of its inputs and a caller which wants the numbers and not the report never comes
// near any of this.
use crate::paths::PERSISTENT_APP_PATHS;
use mezura::{FaultyFileDetails, ParseFilesError, RunResult};
use super::config_manager::Configuration;
use super::formatted::Formatted;

// Everything that turns a result into something a person reads, kept out of 'run' so that the run
// itself is a function of its inputs. A caller that wants the numbers and not the report never calls
// this, and one that wants both gets the same result twice, since presenting reads and never writes.
pub fn present(result: &RunResult, config: &Configuration) {
    let datetime_now = chrono::Local::now();

    if result.files_present.relevant_files == 0 {
        // A machine consumer must not have to tell "no output" apart from "no code found", so the
        // document is written even here, whole and with everything zeroed
        if config.view.prints_text() {
            eprintln!("{}", ParseFilesError::NoRelevantFiles(get_activated_languages_as_str(config)).formatted());
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
        eprintln!("{} {}",error.paint(&faulty_files.len().to_string()), error.paint("faulty files detected. They will be ignored in stat calculation."));
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
