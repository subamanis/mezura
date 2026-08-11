// The log a configuration keeps: one JSON entry per line, the newest first. Writing and reading
// both live here, so the shape cannot drift between two files.
use std::{fs::File, io::{self, BufWriter, Read, Write}, path::Path};

use chrono::{DateTime, Local, SecondsFormat};
use serde_json::Value;

use mezura_core::{RunResult, Stats, Target, UNNAMED_MODULE_NAME};

use super::config_manager::Configuration;
use super::json_printer::escape;
use super::json_reader::{DocumentError, Scope};
use super::json_reader::{parse_scope, parse_stats, read_list, read_nested, read_number,
        read_object, read_optional_name, read_text};

// The log's own lineage, separate from the run document's: it moves when a key of an entry is
// removed or changes meaning, and an entry of a newer format is skipped rather than misread
const LOG_FORMAT_VERSION : usize = 1;

const LOG_ENTRY_KIND : &str = "log-entry";

pub struct LogEntry {
    pub name: Option<String>,
    pub datetime: DateTime<Local>,
    pub scope: Scope,
    pub targets: Vec<Target>,
    pub total: Stats,
    pub modules: Vec<ModuleEntry>
}

// Only what a module line of the history section prints. Files and Extra are on the total and not
// repeated per module.
pub struct ModuleEntry {
    pub name: String,
    pub lines: usize,
    pub code_lines: usize,
    pub comment_lines: usize
}

// A log is the one output that cannot be recomputed: everything else is a fresh measurement of a
// tree still on disk, this is the record of runs that are gone. So it is never truncated in place.
pub fn log_stats(path: &str, contents: &Option<String>, result: &RunResult, datetime_now: &DateTime<Local>, config: &Configuration) -> io::Result<()> {
    // A file that exists and is not empty has history in it whatever stopped it being read this
    // time: a lock, an editor holding it, bytes that are not UTF-8. Writing this run alone over it
    // would leave one entry where there were twenty.
    if contents.is_none() && std::fs::metadata(path).is_ok_and(|file| file.len() > 0) {
        return Err(io::Error::other(format!(
                "'{path}' could not be read, so it was left as it is rather than replaced with this run alone")));
    }

    // Written whole beside the old one and moved over it only when it is complete, so a failure part
    // way through costs this entry and not the file.
    //
    // Named after the process, because with a fixed name two runs against the same log share one
    // file: the second truncates the first mid-write, both flush at their own offsets, and both move
    // the spliced result over the real log. That is not last-writer-wins, it is one run's header
    // over another's body.
    let being_written = format!("{path}.writing.{}", std::process::id());
    let outcome = write_whole_log(&being_written, contents, result, datetime_now, config)
            .and_then(|()| std::fs::rename(&being_written, path));
    if outcome.is_err() {
        let _ = std::fs::remove_file(&being_written);
    }

    outcome
}

// 'None' means one thing: there is no history here to keep. The file is absent, or it is there and
// could not be read, and 'log_stats' tells those two apart by asking the filesystem for its size.
//
// Whitespace is not a third meaning. A log emptied with 'echo. >' holds two bytes, and answering
// 'None' for it would make the refusal above see a file with bytes in it that came back unreadable,
// and decline to write on that run and every run after. Whether whitespace is worth comparing
// against is the printer's question, and the printer asks it.
pub fn extract_file_contents(file_path: &str) -> Option<String> {
    if Path::new(&file_path).is_file() {
        let mut contents = String::with_capacity(700);
        File::open(file_path).ok()?.read_to_string(&mut contents).ok()?;
        Some(contents)
    } else {
        None
    }
}

// The newest first, as the file keeps them. A line that does not read as an entry is skipped rather
// than costing the ones under it: the history behind one broken line is still history.
pub fn read_last_entries(contents: &str, count: usize) -> Vec<LogEntry> {
    contents.lines().map(str::trim).filter(|line| !line.is_empty())
            .filter_map(|line| parse_entry(line).ok())
            .take(count).collect()
}

fn write_whole_log(path: &str, contents: &Option<String>, result: &RunResult,
        datetime_now: &DateTime<Local>, config: &Configuration) -> io::Result<()>
{
    let mut writer = BufWriter::new(File::create(path)?);

    writer.write_all(format_entry_line(config, datetime_now, result).as_bytes())?;
    writer.write_all(b"\n")?;

    if let Some(contents) = contents {
        writer.write_all(contents.as_bytes())?;
    }

    writer.flush()
}

// One line, holding everything the history section reads back: the identity, the scope the run
// obeyed, the totals and the module totals. No derived figure is written: 'extra' and the average
// size are worked out from the counts beside them, and a stored copy is the one thing that can
// disagree with them.
fn format_entry_line(config: &Configuration, datetime_now: &DateTime<Local>, result: &RunResult) -> String {
    let name = config.view.log.name.as_ref()
            .map_or("null".to_owned(), |name| format!("\"{}\"", escape(name)));

    format!("{{\"format\":{LOG_FORMAT_VERSION},\"kind\":\"{LOG_ENTRY_KIND}\",\"name\":{name},\
\"taken_at\":\"{}\",\"scope\":{},\"total\":{},\"modules\":{}}}",
            datetime_now.to_rfc3339_opts(SecondsFormat::Secs, false),
            format_scope(config, &result.targets),
            format_stats(&result.total),
            format_modules(result))
}

// The same keys as the run document's scope, read back through the same 'parse_scope', so the two
// kinds of file cannot drift apart in what they record
fn format_scope(config: &Configuration, targets: &[Target]) -> String {
    let engine = &config.engine;
    format!("{{\"dirs\":{},\"exclude\":{},\"languages\":{},\"excluded_languages\":{},\
\"forced_languages\":{},\"braces_as_code\":{},\"search_in_dotted\":{},\"gitignore\":{},\"keywords_counted\":{}}}",
            format_targets(targets),
            format_strings(&engine.exclude_dirs),
            format_strings(&engine.languages_of_interest),
            format_strings(&engine.excluded_languages),
            format_forced_languages(&engine.forced_languages),
            engine.braces_as_code,
            engine.should_search_in_dotted,
            !engine.no_gitignore,
            engine.count_keywords)
}

fn format_stats(total: &Stats) -> String {
    format!("{{\"files\":{},\"bytes\":{},\"lines\":{},\"code\":{},\"comments\":{}}}",
            total.files, total.bytes, total.lines, total.code_lines, total.comment_lines)
}

fn format_modules(result: &RunResult) -> String {
    if !result.has_modules() {
        return String::from("[]");
    }

    let entries = result.modules.iter().map(|module| {
        let name = module.name.as_ref().map_or("null".to_owned(), |name| format!("\"{}\"", escape(name)));
        format!("{{\"name\":{name},\"lines\":{},\"code\":{},\"comments\":{}}}",
                module.total.lines, module.total.code_lines, module.total.comment_lines)
    }).collect::<Vec<_>>();

    format!("[{}]", entries.join(","))
}

fn format_targets(targets: &[Target]) -> String {
    let entries = targets.iter().map(|target| {
        let module = target.module.as_ref().map_or("null".to_owned(), |x| format!("\"{}\"", escape(x)));
        format!("{{\"module\":{module},\"path\":\"{}\"}}", escape(&target.path))
    }).collect::<Vec<_>>();

    format!("[{}]", entries.join(","))
}

fn format_strings(values: &[String]) -> String {
    format!("[{}]", values.iter().map(|x| format!("\"{}\"", escape(x))).collect::<Vec<_>>().join(","))
}

// Sorted, so that two runs with the same settings produce the same bytes
fn format_forced_languages(forced: &std::collections::HashMap<String, String>) -> String {
    let mut sorted = forced.iter().collect::<Vec<_>>();
    sorted.sort_unstable_by_key(|(extension, _)| extension.as_str());
    let members = sorted.into_iter()
            .map(|(extension, language)| format!("\"{}\":\"{}\"", escape(extension), escape(language)))
            .collect::<Vec<_>>();

    format!("{{{}}}", members.join(","))
}

fn parse_entry(line: &str) -> Result<LogEntry, DocumentError> {
    let value = serde_json::from_str::<Value>(line).map_err(DocumentError::NotJson)?;
    let Value::Object(root) = &value else {
        return Err(DocumentError::WrongType { at: "the entry".to_owned(), wanted: "an object" });
    };

    let format = read_number(root, "format", "")?;
    if format > LOG_FORMAT_VERSION {
        return Err(DocumentError::FormatTooNew { found: format });
    }
    if read_text(root, "kind", "")? != LOG_ENTRY_KIND {
        return Err(DocumentError::WrongType { at: "kind".to_owned(), wanted: "a log entry" });
    }

    let (scope, targets) = parse_scope(read_nested(root, "scope", "")?)?;
    let taken = read_text(root, "taken_at", "")?;
    let datetime = DateTime::parse_from_rfc3339(&taken)
            .map_err(|_| DocumentError::WrongType { at: "taken_at".to_owned(), wanted: "an rfc3339 date" })?
            .with_timezone(&Local);

    Ok(LogEntry {
        name: read_optional_name(root, "name", "")?,
        datetime,
        scope,
        targets,
        total: parse_stats(read_nested(root, "total", "")?, "total")?,
        modules: read_list(root, "modules", "")?.iter().enumerate().map(|(i, entry)| {
            let at = format!("modules[{i}]");
            let entry = read_object(entry, &at)?;
            Ok(ModuleEntry {
                name: read_optional_name(entry, "name", &at)?
                        .unwrap_or_else(|| UNNAMED_MODULE_NAME.to_owned()),
                lines: read_number(entry, "lines", &at)?,
                code_lines: read_number(entry, "code", &at)?,
                comment_lines: read_number(entry, "comments", &at)?
            })
        }).collect::<Result<Vec<_>, _>>()?
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::str::FromStr;

    use mezura_core::{FilesPresent, ModuleResult, Performance, Threads};

    use crate::config_manager::LogOption;
    use crate::paths::test_paths::SCRATCH_LOG_DIR;

    use super::*;

    fn result_of(total: Stats, modules: Vec<ModuleResult>) -> RunResult {
        RunResult { per_language: HashMap::new(), modules, embedded: Default::default(), total,
                faulty_files: Vec::new(),
                files_present: FilesPresent::default(), targets: Vec::new(), unreadable_dirs: Vec::new(),
                performance: Performance { duration_millis: 0, threads: Threads::new(1, 1) } }
    }

    // Written through the same writer the program uses, and read back through the same reader: the
    // identity, the scope, every figure and every module. The scope goes through 'parse_scope', the
    // run document's own reader, so a key written here under another name fails this rather than
    // being silently dropped.
    #[test]
    fn an_entry_round_trips_through_its_own_line() {
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.view.set_log_option(LogOption::new(Some("with \"quotes\" in it".to_owned())));
        config.engine.braces_as_code = true;
        config.engine.exclude_dirs = vec!["node_modules".to_owned()];
        config.engine.forced_languages = hashmap!["m".to_owned() => "matlab".to_owned()];

        let module_of = |name: Option<&str>, lines: usize, code: usize, comments: usize| ModuleResult {
            name: name.map(str::to_owned), per_language: HashMap::new(), embedded: Default::default(),
            total: Stats::new(1, 10, lines, code, comments, HashMap::new()) };
        let mut result = result_of(Stats::new(10, 5000, 1000, 700, 200, HashMap::new()),
                vec![module_of(Some("frontend"), 600, 400, 150), module_of(None, 400, 300, 50)]);
        result.targets = vec![Target::named("frontend", "./web"), Target::of("./src")];

        let now: DateTime<Local> = DateTime::from_str("2021-09-12 04:00:00 +03:00").unwrap();
        let entry = parse_entry(&format_entry_line(&config, &now, &result)).unwrap();

        assert_eq!(Some("with \"quotes\" in it".to_owned()), entry.name);
        assert_eq!(now, entry.datetime);
        assert_eq!((10, 5000, 1000, 700, 200), (entry.total.files, entry.total.bytes,
                entry.total.lines, entry.total.code_lines, entry.total.comment_lines));
        assert_eq!(result.targets, entry.targets);
        assert_eq!(vec!["node_modules".to_owned()], entry.scope.exclude);
        assert_eq!(hashmap!["m".to_owned() => "matlab".to_owned()], entry.scope.forced_languages);
        assert!(entry.scope.braces_as_code && entry.scope.gitignore && entry.scope.keywords_counted);
        assert_eq!(vec!["frontend".to_owned(), UNNAMED_MODULE_NAME.to_owned()],
                entry.modules.iter().map(|x| x.name.clone()).collect::<Vec<_>>());
        assert_eq!((600, 400, 150), (entry.modules[0].lines, entry.modules[0].code_lines,
                entry.modules[0].comment_lines));

        // and a run with no name and no modules reads back as exactly that
        config.view.set_log_option(LogOption::new(None));
        let plain = parse_entry(&format_entry_line(&config, &now,
                &result_of(Stats::new(1, 1, 1, 1, 0, HashMap::new()), Vec::new()))).unwrap();
        assert_eq!(None, plain.name);
        assert!(plain.modules.is_empty());
    }

    // The history behind one broken line is still history, and an entry from a build of tomorrow is
    // one this build must not half-read
    #[test]
    fn a_broken_line_does_not_discard_the_lines_around_it() {
        let config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);
        let now: DateTime<Local> = DateTime::from_str("2021-09-12 04:00:00 +03:00").unwrap();
        let good = format_entry_line(&config, &now, &result_of(Stats::new(2, 2, 2, 2, 0, HashMap::new()), Vec::new()));

        let contents = format!("{good}\nnot json at all\n{}\n{good}\n",
                good.replace("\"format\":1", "\"format\":99"));
        let entries = read_last_entries(&contents, 10);
        assert_eq!(2, entries.len(), "a broken or newer line took its neighbours with it");

        // and the count still means entries, not lines
        assert_eq!(1, read_last_entries(&contents, 1).len());
    }

    #[test]
    fn test_log_creation_and_reading() -> io::Result<()> {
        std::fs::create_dir_all(SCRATCH_LOG_DIR)?;
        let path = SCRATCH_LOG_DIR.to_owned() + "test2.jsonl";
        if Path::new(&path).exists() {
            std::fs::remove_file(&path).unwrap();
        }

        let mut config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);
        config.view.set_log_option(LogOption::new(Some("test name".to_owned())));
        let result = result_of(Stats::new(10, 100, 1000, 100, 0, HashMap::new()), Vec::new());

        log_stats(&path, &None, &result, &DateTime::from_str("2021-09-12 04:00:00 +03:00").unwrap(), &config).unwrap();

        let entries = read_last_entries(&extract_file_contents(&path).unwrap(), 1);
        assert_eq!(10, entries[0].total.files);
        assert_eq!(1000, entries[0].total.lines);
        assert_eq!(100, entries[0].total.code_lines);
        assert_eq!(900, entries[0].total.calculate_extra_lines());
        assert_eq!(100, entries[0].total.bytes);
        assert_eq!(10, entries[0].total.calculate_average_size());
        assert_eq!(Some("test name".to_owned()), entries[0].name);
        assert!(entries[0].modules.is_empty());

        Ok(())
    }

    // The block reaches the entry it belongs to and its figures stay out of the ones above and
    // below it, and the newest entry is the first line
    #[test]
    fn the_modules_of_an_entry_are_read_back_and_never_reach_another_one() {
        std::fs::create_dir_all(SCRATCH_LOG_DIR).unwrap();
        let path = SCRATCH_LOG_DIR.to_owned() + "test_modules.jsonl";
        if Path::new(&path).exists() {
            std::fs::remove_file(&path).unwrap();
        }

        let mut config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);
        config.view.set_log_option(LogOption::new(None));
        let module_of = |name: Option<&str>, lines: usize, code: usize, comments: usize| ModuleResult {
            name: name.map(str::to_owned), per_language: HashMap::new(), embedded: Default::default(),
            total: Stats::new(1, 10, lines, code, comments, HashMap::new()) };

        let older = result_of(Stats::new(4, 4000, 400, 300, 0, HashMap::new()), Vec::new());
        log_stats(&path, &None, &older, &DateTime::from_str("2021-09-12 04:00:00 +03:00").unwrap(), &config).unwrap();

        let with_modules = result_of(Stats::new(10, 5000, 1000, 700, 200, HashMap::new()),
                vec![module_of(Some("frontend"), 600, 400, 150), module_of(None, 400, 300, 50)]);
        let history = extract_file_contents(&path);
        log_stats(&path, &history, &with_modules,
                &DateTime::from_str("2021-09-13 04:00:00 +03:00").unwrap(), &config).unwrap();

        let entries = read_last_entries(&extract_file_contents(&path).unwrap(), 2);
        assert_eq!(2, entries.len());

        assert_eq!(1000, entries[0].total.lines);
        assert_eq!(200, entries[0].total.comment_lines);
        assert_eq!(vec!["frontend".to_owned(), UNNAMED_MODULE_NAME.to_owned()],
                entries[0].modules.iter().map(|x| x.name.clone()).collect::<Vec<_>>());
        assert_eq!((600, 400, 150), (entries[0].modules[0].lines, entries[0].modules[0].code_lines, entries[0].modules[0].comment_lines));
        assert_eq!((400, 300, 50), (entries[0].modules[1].lines, entries[0].modules[1].code_lines, entries[0].modules[1].comment_lines));

        assert_eq!(400, entries[1].total.lines);
        assert!(entries[1].modules.is_empty());

        // and the entry that carries the block is still complete when only one was asked for
        let only_one = read_last_entries(&extract_file_contents(&path).unwrap(), 1);
        assert_eq!(1, only_one.len());
        assert_eq!(2, only_one[0].modules.len());

        std::fs::remove_file(&path).unwrap();
    }

    // The log is the one output that cannot be measured again: the trees those runs counted have
    // moved on. 'extract_file_contents' answers None both to "there is nothing" and to "I could not
    // read it", which are opposite instructions, so the refusal is what this holds.
    #[test]
    fn a_log_that_could_not_be_read_is_kept_rather_than_replaced_by_the_run() {
        std::fs::create_dir_all(SCRATCH_LOG_DIR).unwrap();
        let path = SCRATCH_LOG_DIR.to_owned() + "test_unreadable.jsonl";
        let config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);
        let result = result_of(Stats::new(10, 100, 1000, 100, 0, HashMap::new()), Vec::new());
        let now = DateTime::from_str("2021-09-12 04:00:00 +03:00").unwrap();

        // What a run finds when the history is there and readable: the new entry, then all of it
        std::fs::write(&path, "AN ENTRY FROM BEFORE\n").unwrap();
        let history = extract_file_contents(&path);
        assert!(history.is_some());
        log_stats(&path, &history, &result, &now, &config).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("AN ENTRY FROM BEFORE"), "the history was dropped:\n{written}");

        // And what it finds when the same file cannot be read. The bytes below are not UTF-8, which
        // is one way in; a lock or an antivirus holding the file is the same answer through the
        // same door, and on this platform far likelier.
        std::fs::write(&path, [b"AN ENTRY FROM BEFORE\n".to_vec(), vec![0xFF, 0xFE, 0x80]].concat()).unwrap();
        let unreadable = extract_file_contents(&path);
        assert!(unreadable.is_none(), "the probe no longer reproduces an unreadable log");

        let refused = log_stats(&path, &unreadable, &result, &now, &config);
        assert!(refused.is_err(), "a log that could not be read was overwritten anyway");
        let after = std::fs::read(&path).unwrap();
        assert!(String::from_utf8_lossy(&after).contains("AN ENTRY FROM BEFORE"),
                "the entries were destroyed by a run that could not read them");
        // and nothing half written is left lying beside it, under the name this process would use
        assert!(!Path::new(&format!("{path}.writing.{}", std::process::id())).exists());

        std::fs::remove_file(&path).unwrap();
    }

    // The other side of the same guard. Emptying a log is an ordinary thing to want, and every
    // ordinary way of doing it on this platform leaves a newline behind rather than nothing at all,
    // which the refusal above must not read as a file it failed to parse.
    #[test]
    fn a_log_emptied_by_hand_is_written_again_rather_than_refused_forever() {
        std::fs::create_dir_all(SCRATCH_LOG_DIR).unwrap();
        let path = SCRATCH_LOG_DIR.to_owned() + "test_emptied.jsonl";
        let config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);
        let result = result_of(Stats::new(10, 100, 1000, 100, 0, HashMap::new()), Vec::new());
        let now = DateTime::from_str("2021-09-12 04:00:00 +03:00").unwrap();

        for emptied in ["", "\r\n", "\n\n   \n", "   "] {
            std::fs::write(&path, emptied).unwrap();
            let history = extract_file_contents(&path);
            let written = log_stats(&path, &history, &result, &now, &config);
            assert!(written.is_ok(), "a log holding {emptied:?} was refused: {:?}", written.err());
            assert_eq!(1, read_last_entries(&std::fs::read_to_string(&path).unwrap(), 5).len(),
                    "a log holding {emptied:?} was left without the entry of this run");
        }

        std::fs::remove_file(&path).unwrap();
    }
}
