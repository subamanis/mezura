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
pub fn counting_settings(config: &mezura_core::engine::config::EngineConfig, targets: &[mezura_core::Target])
-> [(&'static str, String); 8] {
    let mezura_core::engine::config::EngineConfig { exclude_dirs, languages_of_interest, excluded_languages,
            forced_languages, braces_as_code, should_search_in_dotted, no_gitignore,
            // recorded from the result instead, which holds the resolved list the run walked: the
            // declared form answers a different question, since the same './src' declared over two
            // different trees is two different measurements
            dirs: _,
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
    let mut targets = targets.to_vec();
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

// A log is the one thing here that cannot be recomputed: every other output is a fresh measurement
// of a tree that is still on disk, and this is the record of runs that are gone. So it is never
// truncated in place. Two things used to be able to empty it, and both were silent.
pub fn log_stats(path: &str, contents: &Option<String>, result: &mezura_core::RunResult, datetime_now: &DateTime<Local>, config: &Configuration) -> io::Result<()> {
    // 'contents' is None both when there is nothing to keep and when the file could not be read,
    // and those are opposite instructions. A file that exists and is not empty has history in it,
    // whatever the reason it could not be read this time: a lock, an editor holding it, bytes that
    // are not UTF-8. Writing this run alone over it would leave one entry where there were twenty.
    if contents.is_none() && std::fs::metadata(path).is_ok_and(|file| file.len() > 0) {
        return Err(io::Error::other(format!(
                "'{path}' could not be read, so it was left as it is rather than replaced with this run alone")));
    }

    // Written whole beside the old one and then moved over it, so that a failure part way through
    // costs this entry and not the file. It used to truncate first and write the history back out
    // of memory afterwards, which is the widest window there is.
    //
    // Named after the process, because a fixed name gives two runs against the same log one file to
    // write into: the second truncates the first mid-write, both flush at their own offsets, and
    // both then move the spliced result over the real log. That is not the last writer winning, it
    // is a file with one run's header over another's body, which the reader takes apart with
    // 'unwrap' on every figure. With a name of its own per process the worst case is the one this
    // whole shape was chosen for, a lost entry rather than a broken file.
    let being_written = format!("{path}.writing.{}", std::process::id());
    let outcome = write_whole_log(&being_written, contents, result, datetime_now, config)
            .and_then(|()| std::fs::rename(&being_written, path));
    if outcome.is_err() {
        let _ = std::fs::remove_file(&being_written);
    }

    outcome
}

fn write_whole_log(path: &str, contents: &Option<String>, result: &mezura_core::RunResult,
        datetime_now: &DateTime<Local>, config: &Configuration) -> io::Result<()>
{
    let mut writer = BufWriter::new(File::create(path)?);

    write_current_log(&mut writer, config, datetime_now, result)?;

    if let Some(contents) = contents {
        writer.write_all(contents.as_bytes())?;
    }

    writer.flush()
}

// 'None' means one thing and one thing only: there is no history here to be kept. The file is
// absent, or it is there and could not be read, and 'log_stats' tells those two apart by asking the
// filesystem for its size.
//
// It used to answer 'None' for a file of nothing but whitespace as well, which is a third meaning
// and the one that does not belong: a log emptied with 'echo. >' holds two bytes, so the refusal
// below saw a file with bytes in it that had come back unreadable and declined to write, on that run
// and on every run after it. Whether whitespace is worth comparing against is the printer's
// question, and the printer asks it.
pub fn extract_file_contents(file_path: &str) -> Option<String> {
    if Path::new(&file_path).is_file() {
        let mut contents = String::with_capacity(700);
        File::open(file_path).ok()?.read_to_string(&mut contents).ok()?;
        Some(contents)
    } else {
        None
    }
}

// The totals stay where they were and the modules are a block under them, so an entry written before
// any of this existed reads exactly as it always did and a run that named none writes no block at
// all. Nothing on disk needs converting, which is the same reason an entry from v2 with no
// 'Comments' line is still read without complaint.
fn write_current_log(writer: &mut BufWriter<File>, config: &Configuration, datetime_now: &DateTime<Local>, result: &mezura_core::RunResult) -> io::Result<()> {
    let total = &result.total;
    writer.write_all(format!("===>{}\n",config.view.log.name.clone().unwrap_or_default()).as_bytes())?;
    writer.write_all(datetime_now.format("%Y-%m-%d %H:%M:%S %z").to_string().as_bytes())?;
    writer.write_all(b"\n")?;
    writer.write_all(b"Configuration:\n")?;
    for (key, value) in counting_settings(&config.engine, &result.targets) {
        writer.write_all(format!("    {key}: {value}\n").as_bytes())?;
    }
    writer.write_all(b"Stats:\n")?;
    writer.write_all(format!("    Files: {}\n",total.files).as_bytes())?;
    writer.write_all(format!("    Lines: {}\n",total.lines).as_bytes())?;
    writer.write_all(format!("        Code: {}\n",total.code_lines).as_bytes())?;
    writer.write_all(format!("        Comments: {}\n",total.comment_lines).as_bytes())?;
    writer.write_all(format!("        Extra: {}\n",total.extra_lines()).as_bytes())?;
    writer.write_all(format!("    Total Size: {}\n",total.bytes).as_bytes())?;
    writer.write_all(format!("        Average Size: {}\n",total.average_size()).as_bytes())?;
    if result.has_modules() {
        writer.write_all(b"    Modules:\n")?;
        for module in &result.modules {
            let stats = &module.total;
            writer.write_all(format!("        {}:\n", module.name.as_deref().unwrap_or(mezura_core::UNNAMED_MODULE_NAME)).as_bytes())?;
            writer.write_all(format!("            Files: {}\n", stats.files).as_bytes())?;
            writer.write_all(format!("            Lines: {}\n", stats.lines).as_bytes())?;
            writer.write_all(format!("                Code: {}\n", stats.code_lines).as_bytes())?;
            writer.write_all(format!("                Comments: {}\n", stats.comment_lines).as_bytes())?;
            writer.write_all(format!("                Extra: {}\n", stats.extra_lines()).as_bytes())?;
        }
    }
    writer.write_all(b"\n\n")?;
    writer.write_all(b"--------------------------------------------------------------------------------------------\n\n\n")?;

    Ok(())
}
