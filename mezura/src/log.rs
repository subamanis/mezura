// The log entry a run leaves behind, and the list that decides whether two runs measured the same
// thing at all.
use std::{fs::File, io::{self, BufWriter, Read, Write}, path::Path};

use chrono::{DateTime, Local};

use super::config_manager::Configuration;
use super::config_manager;


// Everything that can change a number, and nothing that only changes how it looks, written into
// every log entry so that a later run can say whether the two are comparable at all. The same list
// is what the progress section reads back, so the writing and the comparison cannot drift into
// formatting the same setting two different ways.
// What decides whether two runs measured the same thing, which is a narrower question than what the
// engine needs to run: 4 threads and 16 threads produce identical numbers.
//
// Destructured with no '..' on purpose. A new field on 'EngineConfig' stops the build right here
// until somebody decides which of the two questions it answers, and the fixed size of the array is
// the second half of the same guard. 'threads: _' and the rest below are written decisions and not
// omissions. The sibling guard in 'resolve_invalid_config_fields' does the same for the
// commands a configuration file can carry.
pub fn counting_settings(config: &mezura_core::engine::config::EngineConfig) -> [(&'static str, String); 8] {
    let mezura_core::engine::config::EngineConfig { dirs, exclude_dirs, languages_of_interest, excluded_languages,
            forced_languages, braces_as_code, should_search_in_dotted, no_gitignore,
            // changes no number: the same tree counted by more threads is the same tree
            threads: _,
            // changes only the keyword counts, which the log does not record
            count_keywords: _ } = config;

    let yes_no = |value: bool| if value {"yes"} else {"no"}.to_owned();

    // Every key is the name of the command that sets it, so that the 'modified:' tag of the progress
    // section names something the reader can look up with '--help'. That is why this one is the
    // double negative 'no-gitignore' and not the 'gitignore' that would have read better.
    // Sorted here and nowhere else. The report shows the targets in the order they were declared,
    // because that order is the user's own arrangement of the columns, but reordering them changes
    // no number, and this list exists to say whether two runs counted the same thing.
    let mut targets = dirs.to_vec();
    targets.sort();

    [(config_manager::DIRS, config_manager::targets_to_string(&targets)),
     (config_manager::EXCLUDE, exclude_dirs.join(",")),
     (config_manager::LANGUAGES, languages_of_interest.join(",")),
     (config_manager::EXCLUDE_LANGUAGES, excluded_languages.join(",")),
     (config_manager::FORCE_LANG, super::args::forced_languages_to_string(forced_languages)),
     (config_manager::BRACES_AS_CODE, yes_no(*braces_as_code)),
     (config_manager::SEARCH_IN_DOTTED, yes_no(*should_search_in_dotted)),
     (config_manager::NO_GITIGNORE, yes_no(*no_gitignore))]
}

pub fn log_stats(path: &str, contents: &Option<String>, result: &mezura_core::RunResult, datetime_now: &DateTime<Local>, config: &Configuration) -> io::Result<()> {
    let mut writer = std::io::BufWriter::new(std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(path)?);

    write_current_log(&mut writer, config, datetime_now, result)?;

    if let Some(contents) = contents {
        writer.write_all(contents.as_bytes())?;
    }
    writer.flush()?;

    Ok(())
}

pub fn extract_file_contents(file_path: &str) -> Option<String> {
    if Path::new(&file_path).is_file() {
        let mut contents = String::with_capacity(700);
        File::open(file_path).ok()?.read_to_string(&mut contents).ok()?;
        if contents.trim().is_empty() {
            None
        } else {
            Some(contents)
        }
    } else {
        None
    }
}

// The totals stay where they were and the modules are a block under them, so an entry written before
// any of this existed reads exactly as it always did and a run that named none writes no block at
// all. Nothing on disk needs converting, which is the same reason an entry from v2 with no
// 'Comments' line is still read without complaint.
fn write_current_log(writer: &mut BufWriter<File>, config: &Configuration, datetime_now: &DateTime<Local>, result: &mezura_core::RunResult) -> io::Result<()> {
    let final_stats = &result.final_stats;
    writer.write_all(format!("===>{}\n",config.view.log.name.clone().unwrap_or_default()).as_bytes())?;
    writer.write_all(datetime_now.format("%Y-%m-%d %H:%M:%S %z").to_string().as_bytes())?;
    writer.write_all(b"\n")?;
    writer.write_all(b"Configuration:\n")?;
    for (key, value) in counting_settings(&config.engine) {
        writer.write_all(format!("    {key}: {value}\n").as_bytes())?;
    }
    writer.write_all(b"Stats:\n")?;
    writer.write_all(format!("    Files: {}\n",final_stats.files).as_bytes())?;
    writer.write_all(format!("    Lines: {}\n",final_stats.lines).as_bytes())?;
    writer.write_all(format!("        Code: {}\n",final_stats.code_lines).as_bytes())?;
    writer.write_all(format!("        Comments: {}\n",final_stats.comment_lines).as_bytes())?;
    writer.write_all(format!("        Extra: {}\n",final_stats.extra_lines).as_bytes())?;
    writer.write_all(format!("    Total Size: {}\n",final_stats.bytes_size).as_bytes())?;
    writer.write_all(format!("        Average Size: {}\n",final_stats.bytes_average_size).as_bytes())?;
    if result.has_modules() {
        writer.write_all(b"    Modules:\n")?;
        for module in &result.modules {
            let stats = &module.final_stats;
            writer.write_all(format!("        {}:\n", module.name.as_deref().unwrap_or(mezura_core::UNNAMED_MODULE_NAME)).as_bytes())?;
            writer.write_all(format!("            Files: {}\n", stats.files).as_bytes())?;
            writer.write_all(format!("            Lines: {}\n", stats.lines).as_bytes())?;
            writer.write_all(format!("                Code: {}\n", stats.code_lines).as_bytes())?;
            writer.write_all(format!("                Comments: {}\n", stats.comment_lines).as_bytes())?;
            writer.write_all(format!("                Extra: {}\n", stats.extra_lines).as_bytes())?;
        }
    }
    writer.write_all(b"\n\n")?;
    writer.write_all(b"--------------------------------------------------------------------------------------------\n\n\n")?;

    Ok(())
}
