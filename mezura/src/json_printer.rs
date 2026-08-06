use std::collections::HashMap;

use chrono::{DateTime, Local, SecondsFormat};
use mezura_core::{FaultyFileDetails, FilesPresent, RunResult, Stats};

use super::config_manager::Configuration;
use super::result_printer;

// Bumped only when a key is removed or changes meaning. Adding one is not a bump, so a consumer can
// check this and not the version of the binary, which moves for reasons that do not concern it.
const FORMAT_VERSION : usize = 1;

// The document is a designed shape and not a serialization of the structs the program happens to
// have. It carries every number that was measured, in its raw unit, and nothing the printer computed
// in order to look right: no sizes in KB, no separators, no percentages, no bar.
pub fn print_as_json(result: &RunResult, datetime_now: &DateTime<Local>, config: &Configuration) {
    println!("{}", document(result, datetime_now, config));
}

fn document(result: &RunResult, datetime_now: &DateTime<Local>, config: &Configuration) -> String {
    let RunResult {per_language, total, faulty_files, files_present, unreadable_dirs, ..} = result;
    let names = result_printer::get_sorted_language_names(per_language, config.view.sort_by);
    let hidden = config.view.top_n.map_or(0, |top| names.len().saturating_sub(top));
    let shown = &names[..names.len() - hidden];

    let mut members = vec![
        format!("  \"format\": {FORMAT_VERSION}"),
        format!("  \"mezura_version\": \"{}\"", escaped(config.view.version.trim_start_matches('v'))),
        format!("  \"generated_at\": \"{}\"", datetime_now.to_rfc3339_opts(SecondsFormat::Secs, false)),
        format!("  \"scope\": {}", scope_object(config, &result.targets)),
        format!("  \"scan\": {}", scan_object(files_present, faulty_files.len())),
        format!("  \"total\": {}", total_object(total)),
        format!("  \"languages\": {}", languages_array(shown, per_language, config)),
        format!("  \"languages_hidden\": {hidden}"),
        format!("  \"faulty_files\": {}", faulty_files_array(faulty_files)),
        format!("  \"unreadable_dirs\": {}", unreadable_dirs_array(unreadable_dirs)),
        format!("  \"warnings\": {}", warnings_array()),
    ];
    // Absent from a run that named no module, the same way the section is absent from the printed
    // report: a consumer that never asked for a second axis is not handed one holding everything
    if result.has_modules() {
        members.push(format!("  \"modules\": {}", modules_array(result, config)));
    }
    // The only volatile block apart from the timestamp, so hiding the timing is also what makes the
    // document repeatable enough to hash or to compare against a stored one
    if !config.view.hidden.timing {
        members.push(format!("  \"performance\": {}", performance_object(&result.performance)));
    }

    format!("{{\n{}\n}}", members.join(",\n"))
}

// Only what can change a number: no theme, no layout, no separators. Without it, two documents that
// differ by an '--exclude' look like a code change.
fn scope_object(config: &Configuration, targets: &[mezura_core::Target]) -> String {
    let members = [
        // The resolved list off the result, not the declared one off the configuration: the same
        // './src' over two different trees is two different measurements
        format!("    \"dirs\": {}", string_array(&targets.iter().map(|x| x.to_string()).collect::<Vec<_>>())),
        format!("    \"exclude\": {}", string_array(&config.engine.exclude_dirs)),
        format!("    \"languages\": {}", string_array(&config.engine.languages_of_interest)),
        format!("    \"excluded_languages\": {}", string_array(&config.engine.excluded_languages)),
        format!("    \"braces_as_code\": {}", config.engine.braces_as_code),
        format!("    \"search_in_dotted\": {}", config.engine.should_search_in_dotted),
        format!("    \"gitignore\": {}", !config.engine.no_gitignore),
        format!("    \"keywords_counted\": {}", !config.view.hidden.keywords),
    ];

    format!("{{\n{}\n  }}", members.join(",\n"))
}

// 'files_of_interest' is what the status line calls it, and it is not the same as the file count of
// the total below: the faulty ones were found and are of interest, but nothing of them was counted.
fn scan_object(files: &FilesPresent, faulty: usize) -> String {
    let members = [
        format!("    \"files_found\": {}", files.total_files),
        format!("    \"files_of_interest\": {}", files.relevant_files),
        format!("    \"files_excluded\": {}", files.excluded_files),
        format!("    \"files_faulty\": {faulty}"),
    ];

    format!("{{\n{}\n  }}", members.join(",\n"))
}

fn total_object(total: &Stats) -> String {
    let members = [
        format!("    \"files\": {}", total.files),
        format!("    \"lines\": {}", total.lines),
        format!("    \"code\": {}", total.code_lines),
        format!("    \"comments\": {}", total.comment_lines),
        format!("    \"extra\": {}", total.extra_lines()),
        format!("    \"bytes\": {}", total.bytes),
        format!("    \"average_bytes\": {}", total.average_size()),
    ];

    format!("{{\n{}\n  }}", members.join(",\n"))
}

// The leftovers of the named modules carry 'null' and not the '(unnamed)' the report prints: a marker
// spelled as a name is one a real module could be called, and a machine consumer grouping by that
// key would silently merge the two.
fn modules_array(result: &RunResult, config: &Configuration) -> String {
    let entries = result.modules.iter().map(|module| {
        let names = result_printer::get_sorted_language_names(&module.per_language, config.view.sort_by);
        let hidden = config.view.top_n.map_or(0, |top| names.len().saturating_sub(top));
        let shown = &names[..names.len() - hidden];
        let name = module.name.as_ref().map_or("null".to_owned(), |x| format!("\"{}\"", escaped(x)));
        let members = [
            format!("      \"name\": {name}"),
            format!("      \"total\": {}", indented(&total_object(&module.total))),
            format!("      \"languages\": {}", indented(&languages_array(shown, &module.per_language, config))),
            format!("      \"languages_hidden\": {hidden}"),
        ];
        format!("    {{\n{}\n    }}", members.join(",\n"))
    }).collect::<Vec<_>>();

    format!("[\n{}\n  ]", entries.join(",\n"))
}

// The two blocks are shared with the top level, where they sit one level higher, so their closing
// braces and their members are pushed in rather than written twice
fn indented(block: &str) -> String {
    block.replace('\n', "\n    ")
}

// An array and not an object keyed by language name, so that the order '--sort' chose survives and
// so that no language can collide with a key of the document.
fn languages_array(shown: &[String], per_language: &HashMap<String, Stats>, config: &Configuration) -> String
{
    if shown.is_empty() {
        return String::from("[]");
    }

    let entries = shown.iter().filter_map(|name| {
        Some(language_object(name, per_language.get(name)?, !config.view.hidden.keywords))
    }).collect::<Vec<_>>();

    format!("[\n{}\n  ]", entries.join(",\n"))
}

fn language_object(name: &str, info: &Stats, keywords_counted: bool) -> String {
    let mut members = vec![
        format!("      \"name\": \"{}\"", escaped(name)),
        format!("      \"files\": {}", info.files),
        format!("      \"lines\": {}", info.lines),
        format!("      \"code\": {}", info.code_lines),
        format!("      \"comments\": {}", info.comment_lines),
        format!("      \"extra\": {}", info.extra_lines()),
        format!("      \"bytes\": {}", info.bytes),
        format!("      \"average_bytes\": {}", info.average_size()),
    ];
    // Absent when they were not counted, since '--hide keywords' also stops the counting. An empty
    // object means the opposite: they were counted and the language declares none.
    if keywords_counted {
        members.push(format!("      \"keywords\": {}", keywords_object(&info.keyword_occurences)));
    }

    format!("    {{\n{}\n    }}", members.join(",\n"))
}

fn keywords_object(occurences: &HashMap<String, usize>) -> String {
    if occurences.is_empty() {
        return String::from("{}");
    }

    let mut sorted = occurences.iter().collect::<Vec<_>>();
    sorted.sort_unstable_by_key(|(name, _)| name.as_str());
    let members = sorted.into_iter()
            .map(|(name, count)| format!("        \"{}\": {count}", escaped(name)))
            .collect::<Vec<_>>();

    format!("{{\n{}\n      }}", members.join(",\n"))
}

// Everything the run said on the error output, which a machine consumer never sees. Always present,
// empty array included, so that a consumer can read it without asking whether the key is there.
//
// 'code' is the half that is safe to branch on and 'message' the half that is safe to show, and
// 'affects' is what lets a consumer written today keep working when a later version adds a code it
// has never heard of: the question is whether the counts can be trusted, not which of the codes are
// the serious ones. In emission order, which is the order they were printed in.
fn warnings_array() -> String {
    let warnings = super::warnings::collected();
    if warnings.is_empty() {
        return String::from("[]");
    }

    let entries = warnings.iter().map(|warning| {
        let members = [
            format!("      \"code\": \"{}\"", escaped(warning.code)),
            format!("      \"affects\": \"{}\"", warning.affects.name()),
            format!("      \"subject\": \"{}\"", escaped(&warning.subject)),
            format!("      \"message\": \"{}\"", escaped(&warning.message)),
        ];
        format!("    {{\n{}\n    }}", members.join(",\n"))
    }).collect::<Vec<_>>();

    format!("[\n{}\n  ]", entries.join(",\n"))
}

// Sorted by path, because the faulty files are collected by whichever thread hit them and their
// order would otherwise change between two runs over the same tree
fn faulty_files_array(faulty_files: &[FaultyFileDetails]) -> String {
    if faulty_files.is_empty() {
        return String::from("[]");
    }

    let mut sorted = faulty_files.iter().collect::<Vec<_>>();
    sorted.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    let entries = sorted.into_iter().map(|file| {
        let members = [
            format!("      \"path\": \"{}\"", escaped(&file.path)),
            format!("      \"bytes\": {}", file.size),
            format!("      \"error\": \"{}\"", escaped(&file.error_msg)),
        ];
        format!("    {{\n{}\n    }}", members.join(",\n"))
    }).collect::<Vec<_>>();

    format!("[\n{}\n  ]", entries.join(",\n"))
}

// Objects and not bare paths, and sorted for the same reason as the faulty files above. A consumer
// that wants only the paths reads one key of each; one that wants to tell a permission apart from a
// directory that went away mid-walk could not do it at all while this was an array of strings.
fn unreadable_dirs_array(unreadable_dirs: &[mezura_core::UnreadableDirDetails]) -> String {
    if unreadable_dirs.is_empty() {
        return String::from("[]");
    }

    let mut sorted = unreadable_dirs.iter().collect::<Vec<_>>();
    sorted.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    let entries = sorted.into_iter().map(|dir| {
        let members = [
            format!("      \"path\": \"{}\"", escaped(&dir.path)),
            format!("      \"error\": \"{}\"", escaped(&dir.error_msg)),
        ];
        format!("    {{\n{}\n    }}", members.join(",\n"))
    }).collect::<Vec<_>>();

    format!("[\n{}\n  ]", entries.join(",\n"))
}

// 'scan_ms' and not the 'Exec time' of the footer: what is measured here is the interval that starts
// before the producers and ends when the consumers are done, which is the phase 'scan' describes.
// The total is not known yet at this point, and the shell can measure it honestly anyway.
// The thread counts come from the result and not from the configuration, because they sit beside
// the measurement they exist to interpret: the configuration holds what was asked for, and the
// operating system is allowed to grant fewer. A document stating the requested counts next to
// 'scan_ms' would be lying about the conditions of its own timing.
fn performance_object(performance: &mezura_core::Performance) -> String {
    let threads = format!("{{\n      \"producers\": {},\n      \"consumers\": {}\n    }}",
            performance.threads.producers(), performance.threads.consumers());

    format!("{{\n    \"scan_ms\": {},\n    \"threads\": {threads}\n  }}", performance.duration_millis)
}

fn string_array(values: &[String]) -> String {
    if values.is_empty() {
        return String::from("[]");
    }

    format!("[{}]", values.iter().map(|x| format!("\"{}\"", escaped(x))).collect::<Vec<_>>().join(", "))
}

// Paths are the reason this has to be right: on Windows they arrive with backslashes in them, so
// every single document would be invalid JSON without the escape.
fn escaped(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '"'  => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            x if (x as u32) < 0x20 => escaped.push_str(&format!("\\u{:04x}", x as u32)),
            x => escaped.push(x)
        }
    }

    escaped
}

#[cfg(test)]
mod tests {
    use crate::config_manager::{Layout, SortCriterion};

    use super::*;

    fn stats_of(files: usize, bytes: usize, lines: usize, code: usize, comments: usize, keywords: HashMap<String,usize>) -> Stats {
        Stats::new(files, bytes, lines, code, comments, keywords)
    }

    fn result_of(per_language: HashMap<String, Stats>, total: Stats,
            faulty_files: Vec<FaultyFileDetails>, files_present: FilesPresent) -> RunResult
    {
        RunResult {per_language, modules: Vec::new(), total, faulty_files,
                files_present, targets: Vec::new(), unreadable_dirs: Vec::new(),
                performance: mezura_core::Performance { duration_millis: 1180, threads: mezura_core::Threads::new(2, 8) }}
    }

    fn document_of(config: &crate::config_manager::Configuration) -> String {
        let result = result_of(
            hashmap![
                "Rust".to_owned() => stats_of(2, 5000, 100, 70, 10, hashmap!["structs".to_owned() => 3, "enums".to_owned() => 1]),
                "HTML".to_owned() => stats_of(1, 900, 40, 30, 0, HashMap::new())],
            Stats::new(3, 5900, 140, 100, 10, HashMap::new()), Vec::new(),
            FilesPresent {total_files: 5, relevant_files: 3, excluded_files: 2});
        let datetime = DateTime::parse_from_rfc3339("2026-07-30T14:22:07+03:00").unwrap().with_timezone(&Local);

        document(&result, &datetime, config)
    }

    #[test]
    fn every_string_that_json_cannot_carry_raw_is_escaped() {
        assert_eq!("a\\\\b", escaped("a\\b"));
        assert_eq!("D:\\\\dev\\\\a \\\"b\\\".rs", escaped("D:\\dev\\a \"b\".rs"));
        assert_eq!("one\\ntwo\\tthree", escaped("one\ntwo\tthree"));
        assert_eq!("\\u0007", escaped("\u{7}"));
        assert_eq!("Δ ok", escaped("Δ ok"));
    }

    #[test]
    fn the_document_carries_the_raw_counts_and_none_of_the_presentation() {
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.view.layout = Layout::Boxed;
        let document = document_of(&config);

        assert!(document.contains("\"format\": 1"));
        assert!(document.contains("\"mezura_version\": \"3.0.0\""));
        assert!(document.contains("\"generated_at\": \"2026-07-30T14:22:07+03:00\""));
        assert!(document.contains("\"lines\": 140"));
        assert!(document.contains("\"average_bytes\": 2500"));
        assert!(document.contains("\"scan_ms\": 1180"));
        // Nothing that the printed output adds: no separators in the four digit numbers, no size
        // measurement, no percentage, and no layout or theme in the echo of the settings
        assert!(!document.contains("5,900"));
        assert!(!document.contains("KB"));
        assert!(!document.contains('%'));
        assert!(!document.contains("boxed"));
    }

    #[test]
    fn sort_orders_the_languages_and_top_cuts_them_while_the_total_stays_whole() {
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.view.sort_by = SortCriterion::Name;
        let document = document_of(&config);
        assert!(document.find("\"HTML\"").unwrap() < document.find("\"Rust\"").unwrap());

        config.view.sort_by = SortCriterion::Lines;
        let document = document_of(&config);
        assert!(document.find("\"Rust\"").unwrap() < document.find("\"HTML\"").unwrap());

        config.view.top_n = Some(1);
        let document = document_of(&config);
        assert!(document.contains("\"Rust\""));
        assert!(!document.contains("\"HTML\""));
        assert!(document.contains("\"languages_hidden\": 1"));
        assert!(document.contains("\"lines\": 140"));
    }

    #[test]
    fn hiding_the_keywords_removes_the_key_while_a_language_without_any_gets_an_empty_one() {
        let config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let document = document_of(&config);
        assert!(document.contains("\"keywords\": {}"));
        assert!(document.contains("\"structs\": 3"));
        assert!(document.contains("\"keywords_counted\": true"));
        // Sorted by name, so that two runs over the same tree produce the same bytes
        assert!(document.find("\"enums\"").unwrap() < document.find("\"structs\"").unwrap());

        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.view.hidden.keywords = true;
        let document = document_of(&config);
        assert!(!document.contains("\"keywords\""));
        assert!(document.contains("\"keywords_counted\": false"));
    }

    #[test]
    fn hiding_the_timing_removes_the_only_block_that_changes_between_two_identical_runs() {
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.view.hidden.timing = true;
        let document = document_of(&config);

        assert!(!document.contains("\"performance\""));
        assert!(!document.contains("\"scan_ms\""));
    }

    // The key is absent from a run that named nothing, and the leftovers carry 'null': a marker
    // spelled '(unnamed)' is a name a real module could be given, and a consumer grouping by that key
    // would merge the two without noticing
    #[test]
    fn the_modules_appear_only_when_one_was_named_and_the_leftovers_have_no_name() {
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        assert!(!document_of(&config).contains("\"modules\""));

        let module_of = |name: Option<&str>, language: &str, lines: usize, files: usize| {
            let per_language = hashmap![language.to_owned() => stats_of(files, lines * 10, lines, lines, 0, HashMap::new())];
            let total = Stats::total_of(&per_language);
            mezura_core::ModuleResult {name: name.map(str::to_owned), per_language, total}
        };
        let mut result = result_of(
            hashmap!["Rust".to_owned() => stats_of(2, 1000, 100, 100, 0, HashMap::new()),
                     "HTML".to_owned() => stats_of(1, 400, 40, 40, 0, HashMap::new())],
            Stats::new(3, 1400, 140, 140, 0, HashMap::new()), Vec::new(),
            FilesPresent {total_files: 3, relevant_files: 3, excluded_files: 0});
        result.modules = vec![module_of(Some("backend"), "Rust", 100, 2), module_of(None, "HTML", 40, 1)];

        config.view.hidden.timing = true;
        let rendered = document(&result, &Local::now(), &config);
        assert!(rendered.contains("\"name\": \"backend\""));
        assert!(rendered.contains("\"name\": null"));
        // Each module carries the same 'total' and 'languages' blocks the document carries for the
        // whole run, so a consumer reads one shape and not two, and the two of them add up to it
        let block = &rendered[rendered.find("\"modules\"").unwrap()..];
        assert_eq!(2, block.matches("\"total\":").count());
        assert_eq!(2, block.matches("\"languages\":").count());
        assert!(block.contains("\"lines\": 100") && block.contains("\"lines\": 40"));
        assert!(rendered.contains("\"lines\": 140"));
        assert!(rendered.contains("\"languages_hidden\": 0"));

        // '--top' is per module there too, so a module with one language is not cut by '--top 1'
        // while the report as a whole has two
        config.view.top_n = Some(1);
        let cut = document(&result, &Local::now(), &config);
        assert_eq!(2, cut.matches("\"languages_hidden\": 0").count());
        assert!(cut.contains("\"languages_hidden\": 1"));
    }

    // Everything a run says on the error output is invisible to whoever asked for the document, and
    // some of it means the counts cannot be trusted. The collector is shared by the whole process,
    // so this asserts on its own entry rather than on the whole array.
    #[test]
    fn a_warning_reaches_the_document_with_both_of_its_halves() {
        let config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        // Present even when there is nothing to say, so a consumer never has to test for the key
        assert!(document_of(&config).contains("\"warnings\": []") || document_of(&config).contains("\"warnings\": ["));

        super::super::warnings::keep(mezura_core::warnings::Warning::new(mezura_core::warnings::EXTENSION_TIEBREAK,
                mezura_core::warnings::Affects::Counts, "a-subject-only-this-test-uses",
                "quoted \"text\" and a \\ backslash".to_owned()));

        let rendered = warnings_array();
        assert!(rendered.contains("\"subject\": \"a-subject-only-this-test-uses\""));
        assert!(rendered.contains("\"code\": \"extension-tiebreak\""));
        assert!(rendered.contains("\"affects\": \"counts\""));
        // The message is prose written for a person, so it goes through the same escaping as every
        // other string here or a quotation mark in it would break the document
        assert!(rendered.contains("quoted \\\"text\\\" and a \\\\ backslash"));
    }

    #[test]
    fn a_run_with_nothing_to_count_is_still_a_whole_document() {
        let config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let result = result_of(HashMap::new(), Stats::default(),
                Vec::new(), FilesPresent {total_files: 12, relevant_files: 0, excluded_files: 12});
        let document = document(&result, &Local::now(), &config);

        assert!(document.contains("\"languages\": []"));
        assert!(document.contains("\"files\": 0"));
        assert!(document.contains("\"files_found\": 12"));
        assert!(document.contains("\"faulty_files\": []"));
    }

    #[test]
    fn the_faulty_files_are_reported_with_their_reason_in_a_stable_order() {
        let config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let result = result_of(
            hashmap!["Rust".to_owned() => stats_of(1, 30, 10, 5, 0, HashMap::new())],
            Stats::new(1, 30, 10, 5, 0, HashMap::new()),
            vec![FaultyFileDetails::new("src\\z.rs".to_owned(), "no".to_owned(), 20),
                 FaultyFileDetails::new("src\\a.rs".to_owned(), "nope".to_owned(), 10)],
            FilesPresent {total_files: 3, relevant_files: 3, excluded_files: 0});
        let document = document(&result, &Local::now(), &config);

        assert!(document.contains("\"files_faulty\": 2"));
        assert!(document.contains("\"path\": \"src\\\\a.rs\""));
        assert!(document.find("a.rs").unwrap() < document.find("z.rs").unwrap());
    }

    // Objects and not bare paths, so that a consumer can tell a permission apart from a directory
    // that went away between being queued and being opened. As strings there was one sentence for
    // every reason, and on a whole drive that is hundreds of rows saying the same word.
    #[test]
    fn the_unreadable_directories_carry_their_reason_in_a_stable_order() {
        let config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let mut result = result_of(HashMap::new(), Stats::default(), Vec::new(),
                FilesPresent {total_files: 0, relevant_files: 0, excluded_files: 0});
        result.unreadable_dirs = vec![
            mezura_core::UnreadableDirDetails::new("D:/z".to_owned(),
                    "Access is denied. (os error 5)".to_owned()),
            mezura_core::UnreadableDirDetails::new("D:/a".to_owned(),
                    "The system cannot find the path specified. (os error 3)".to_owned())];
        let written = document(&result, &Local::now(), &config);

        assert!(written.contains("\"path\": \"D:/a\""), "{written}");
        assert!(written.contains("\"error\": \"Access is denied. (os error 5)\""), "{written}");
        assert!(written.contains("\"error\": \"The system cannot find the path specified. (os error 3)\""), "{written}");
        // sorted by path, since the walk collects these in whichever order its threads hit them
        assert!(written.find("D:/a").unwrap() < written.find("D:/z").unwrap(), "{written}");

        // and a run that opened everything still writes the key, empty
        let clean = result_of(HashMap::new(), Stats::default(), Vec::new(),
                FilesPresent {total_files: 0, relevant_files: 0, excluded_files: 0});
        assert!(document(&clean, &Local::now(), &config).contains("\"unreadable_dirs\": []"));
    }
}
