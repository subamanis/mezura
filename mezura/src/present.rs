use std::collections::BTreeMap;

use mezura_core::{FaultyFileDetails, RunResult, ScanSkip, Target, UnreadableDirDetails};

use super::config_manager::{Configuration, HIDE, TESTS};
use crate::message_printer::MESSAGE_WIDTH;
use crate::paths::PERSISTENT_APP_PATHS;
use crate::test_presets::PRESET_SIGIL;

// A file that says which ecosystem a project is in without saying where its tests are, the presets
// that ecosystem's test tools have, and the languages those tests are written in
const GENERIC_PROJECT_FILES : [(&str, &[&str], &[&str]); 4] = [
    ("package.json", &["jest", "vitest", "mocha"], &["JavaScript", "TypeScript", "JSX", "TSX"]),
    ("pyproject.toml", &["pytest", "unittest"], &["Python"]),
    ("Gemfile", &["rspec", "minitest"], &["Ruby"]),
    ("composer.json", &["phpunit"], &["PHP"]),
];

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
                outln!();
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
                outln!();
            }
        }
        print_comparison_or_empty_document(result, comparison, &datetime_now, config);
        return;
    }

    print_faulty_files_or_ok(&result.faulty_files, config);
    print_detail_hint_if_anything_was_hidden(result, config);
    print_files_left_out(result, config);

    // Every file found was left out, so no table and no log entry, for the reason the faulty branch
    // above returns.
    if result.nothing_of_interest_was_counted() {
        if config.view.prints_text() {
            eprintln!("{}", super::theme::get_active().warning.paint("Nothing was left to count."));
            if comparison.is_some() {
                outln!();
            }
        }
        print_comparison_or_empty_document(result, comparison, &datetime_now, config);
        return;
    }

    // A comparison takes the report's place whole: no report, no history section, no log entry
    if comparison.is_some() || !config.view.prints_a_report() {
        print_comparison_or_empty_document(result, comparison, &datetime_now, config);
        return;
    }

    let log_file_path = determine_log_file_path(config);
    let existing_log_contents = log_file_path.as_ref().and_then(|path| super::log::extract_file_contents(path));
    super::result_printer::format_and_print_results(result, &existing_log_contents, &datetime_now, config);
    print_test_preset_notes(result, config);

    if config.view.log.should_log && let Some(path) = log_file_path
        && let Err(reason) = super::log::log_stats(&path, existing_log_contents.as_deref(), result, &datetime_now, config) {
        eprintln!("\n{}",super::theme::get_active().warning.paint(&format!("Error while trying to save the log: {reason}")));
    }
}

// '--hide' never hides a parsing failure: the numbers would be lower than the tree with nothing
// to say why. A JSON run has them in the document as well, and still on the error output.
pub fn print_faulty_files_or_ok(faulty_files: &[FaultyFileDetails], config: &Configuration) {
    if faulty_files.is_empty() {
        if !config.view.hidden.parsing_info && config.view.prints_text() {
            outln!("{}\n",super::theme::get_active().success.paint("ok"));
        }
    } else {
        let error = &super::theme::get_active().error;
        let (count, subject, pronoun) = (faulty_files.len(),
                if faulty_files.len() == 1 {"file"} else {"files"},
                if faulty_files.len() == 1 {"it is"} else {"they are"});
        eprintln!("{} {}", error.paint(&count.to_string()),
                error.paint(&format!("{subject} could not be parsed, so {pronoun} in no figure below.")));
        if config.view.should_show_faulty_files {
            for f in sort_by_path(faulty_files, |x| x.path.as_str()) {
                eprintln!("-- Error: {} \n   for file: {}\n",f.error_msg,f.path);
            }
        }
        eprintln!();
    }
}

// The one place that decides which of the two a comparison is printed as. A run that scanned reaches
// it from here and a run that only read documents reaches it from main, and a third format would
// otherwise have to be remembered in both.
pub fn print_comparison_as_text_or_json(comparison: &super::diff::Comparison,
        datetime_now: &chrono::DateTime<chrono::Local>, config: &Configuration)
{
    if config.view.prints_a_report() {
        super::result_printer::print_comparison(comparison, config);
    } else {
        super::json_printer::print_comparison_as_json(comparison, datetime_now, config);
    }
}

// Whichever thread met the file or the directory is the one that added it, so without this the same
// tree prints its failures in a different order every run.
pub(crate) fn sort_by_path<T>(items: &[T], get_path: impl Fn(&T) -> &str) -> Vec<&T> {
    let mut sorted = items.iter().collect::<Vec<_>>();
    sorted.sort_unstable_by(|a, b| get_path(a).cmp(get_path(b)));

    sorted
}

// On the error output and never hidden, for the reason a faulty file is: the figures are lower than
// the tree and nothing else would say why. A JSON run has it under 'scan'.
fn print_files_left_out(result: &RunResult, config: &Configuration) {
    if !config.view.prints_text() {
        return;
    }
    let lines = format_files_left_out(result, config.view.should_show_skipped_files);
    for line in &lines {
        if line.starts_with("-- ") {
            eprintln!("{line}");
        } else {
            eprintln!("{}", super::theme::get_active().note.paint(line));
        }
    }
    if !lines.is_empty() {
        eprintln!();
    }
}

fn format_files_left_out(result: &RunResult, show_skipped: bool) -> Vec<String> {
    let mut lines = Vec::new();
    for kind in ScanSkip::ALL {
        let paths = result.skipped_files.get_of_kind(kind);
        if paths.is_empty() {
            continue;
        }
        let word = match kind {
            ScanSkip::Minified => "minified",
            ScanSkip::Generated => "generated",
            ScanSkip::NotCode => "non-code"
        };
        let subject = if paths.len() == 1 {"file was"} else {"files were"};
        lines.push(format!("{} {word} {subject} left out of the counts. Run with '--{}' to include them.",
                crate::number_formatter::format_with_separators(paths.len()),
                crate::config_manager::get_command_that_counts(kind)));
        if show_skipped {
            lines.extend(paths.iter().map(|path| format!("-- {path}")));
        }
    }
    if !lines.is_empty() && !show_skipped {
        lines.push(format!("'--{}' lists them.", crate::config_manager::SHOW_SKIPPED));
    }
    lines
}

// Nothing once the project's own configuration declares its test patterns. The rows keep to the
// width themselves, so nothing wraps them.
fn print_test_preset_notes(result: &RunResult, config: &Configuration) {
    let decided = config.view.local_dir.as_ref().is_some_and(|x| x.configuration_applied && x.declares_test_patterns);
    if !config.view.prints_text() || !config.engine.detect_tests || decided {
        return;
    }
    let lines = format_applied_presets(result).into_iter()
            .chain(find_presets_to_suggest(result).iter().map(|names| format_preset_suggestion(names)))
            .collect::<Vec<_>>();
    if !lines.is_empty() {
        eprintln!();
    }
    for line in lines {
        eprintln!("{}", super::theme::get_active().note.paint(&line));
    }
}

fn format_applied_presets(result: &RunResult) -> Vec<String> {
    let mut rows : BTreeMap<&str, (Vec<String>, Vec<&str>)> = BTreeMap::new();
    for preset in result.applied_presets.iter().filter(|preset| preset.matched) {
        let (directories, trigger_files) = rows.entry(preset.name.as_str()).or_default();
        directories.push(format_preset_directory(&preset.directory, &result.targets));
        if !trigger_files.contains(&preset.trigger_file.as_str()) {
            trigger_files.push(&preset.trigger_file);
        }
    }
    if rows.is_empty() {
        return Vec::new();
    }

    let name_width = rows.keys().map(|name| name.chars().count() + 4).max().unwrap_or(0);
    let mut lines = vec!["Test presets applied:".to_owned()];
    for (name, (directories, trigger_files)) in &rows {
        let opening = format!("  {:<name_width$}under ", format!("{PRESET_SIGIL}{name}"));
        let closing = format!(" ({})", trigger_files.join(", "));
        let room = MESSAGE_WIDTH.saturating_sub(opening.chars().count() + closing.chars().count());
        let shown = (1..=directories.len()).rev()
                .find(|&shown| format_directory_list(directories, shown).chars().count() <= room).unwrap_or(1);
        lines.push(format!("{opening}{}{closing}", format_directory_list(directories, shown)));
    }
    lines.push(format!("To disable a test preset: '--{TESTS} !{PRESET_SIGIL}<name>'. To disable test detection: \
            '--{HIDE} tests'."));

    lines
}

fn format_directory_list(directories: &[String], shown: usize) -> String {
    let listed = directories[..shown].join(", ");
    match directories.len() - shown {
        0 => listed,
        more => format!("{listed} and {more} more")
    }
}

fn format_preset_directory(directory: &str, targets: &[Target]) -> String {
    let relative = targets.iter().filter(|target| super::result_printer::is_inside(directory, &target.path))
            .max_by_key(|target| target.path.len())
            .map(|target| format!("./{}", directory[target.path.len()..].trim_start_matches('/')));

    super::result_printer::elide_long_path(relative.as_deref().unwrap_or(directory)).into_owned()
}

fn find_presets_to_suggest(result: &RunResult) -> Vec<&'static [&'static str]> {
    GENERIC_PROJECT_FILES.iter().filter(|(file, presets, languages)| {
        let in_a_target = result.targets.iter().any(|target| std::path::Path::new(&target.path).join(file).is_file());
        let counted = languages.iter().any(|language| result.per_language.contains_key(*language));
        let tests_found = languages.iter().any(|language| result.tests.get(*language).is_some_and(|x| x.stats.lines > 0))
                || result.applied_presets.iter().any(|preset| preset.matched && presets.contains(&preset.name.as_str()));
        in_a_target && counted && !tests_found
    }).map(|(_, presets, _)| *presets).collect()
}

fn format_preset_suggestion(names: &[&str]) -> String {
    let declared = names.iter().enumerate().map(|(at, name)| match at {
        0 => format!("'--{TESTS} {PRESET_SIGIL}{name}'"),
        _ => format!("'{PRESET_SIGIL}{name}'")
    }).collect::<Vec<_>>();

    format!("{} tests are declared with {}.", join_with_or(names), join_with_or(&declared))
}

fn join_with_or(items: &[impl AsRef<str>]) -> String {
    match items {
        [] => String::new(),
        [only] => only.as_ref().to_owned(),
        [first @ .., last] => format!("{} or {}", first.iter().map(|item| item.as_ref()).collect::<Vec<&str>>().join(", "),
                last.as_ref())
    }
}

// In the error color, since everything under an unreadable directory appears in no total at all,
// where a faulty file is at least counted among the files that were found.
fn print_unreadable_dirs(unreadable_dirs: &[UnreadableDirDetails], config: &Configuration) {
    if unreadable_dirs.is_empty() {return;}

    let error = &super::theme::get_active().error;
    let (count, subject, pronoun) = (unreadable_dirs.len(),
            if unreadable_dirs.len() == 1 {"directory"} else {"directories"},
            if unreadable_dirs.len() == 1 {"it"} else {"them"});
    eprintln!("{} {}", error.paint(&count.to_string()),
            error.paint(&format!("{subject} could not be read. Nothing inside {pronoun} was counted.")));
    if config.view.should_show_faulty_files {
        for dir in sort_by_path(unreadable_dirs, |x| x.path.as_str()) {
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
        Some(comparison) => print_comparison_as_text_or_json(comparison, datetime_now, config),
        None if config.view.prints_a_report() => (),
        None => super::json_printer::print_as_json(result, datetime_now, config)
    }
}

fn print_detail_hint_if_anything_was_hidden(result: &RunResult, config: &Configuration) {
    if let Some(hint) = determine_detail_hint(result, config) {
        eprintln!("{}\n", super::theme::get_active().note.paint(&hint));
    }
}

// Once for the run and not once per kind of problem: it is the same flag either way, and a scan
// that meets both would otherwise tell the reader twice to do one thing.
fn determine_detail_hint(result: &RunResult, config: &Configuration) -> Option<String> {
    if config.view.should_show_faulty_files
        || (result.unreadable_dirs.is_empty() && result.faulty_files.is_empty()) {
        return None;
    }

    Some(format!("Run with '--{}' for the paths and the reasons.", super::config_manager::SHOW_FAULTY_FILES))
}

fn get_activated_languages_as_str(config: &Configuration) -> String {
    let mut msg = if config.engine.languages_of_interest.is_empty() {
        String::new()
    } else {
        String::from("\n(Activated languages: ") + &config.engine.languages_of_interest.to_written_form().join(", ") + ")"
    }
    ;
    let other = if config.engine.excluded_languages.is_empty() {
        String::new()
    } else {
        String::from("\n(Excluded languages: ") + &config.engine.excluded_languages.to_written_form().join(", ") + ")"
    };

    msg += &other;
    msg
}

// A named configuration keeps its history where every named one's history is kept. A run that named
// none and found a project's own folder writes into that folder instead, so the history of the
// project sits beside its code.
fn determine_log_file_path(config: &Configuration) -> Option<String> {
    match config.view.config_name_to_save.as_ref().or(config.view.config_name_to_load.as_ref()) {
        Some(name) => Some(PERSISTENT_APP_PATHS.logs_dir.clone() + name + ".jsonl"),
        None => config.view.find_project_of_the_log().map(crate::paths::LocalDir::get_log_path)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use mezura_core::{AppliedPreset, FilesPresent, Performance, Threads};

    use super::*;

    #[test]
    fn the_left_out_notes_name_each_kind_and_the_paths_only_when_asked() {
        let mut result = result_with(0, 0);
        result.skipped_files = mezura_core::SkippedFiles {
            minified: vec!["D:/x/bundle.js".to_owned()],
            generated: Vec::new(),
            not_code: vec!["D:/x/a.d".to_owned(), "D:/x/b.d".to_owned()]
        };

        let quiet = format_files_left_out(&result, false);
        assert_eq!(3, quiet.len(), "{quiet:?}");
        assert!(quiet[0].starts_with("1 minified file was left out"), "{quiet:?}");
        assert!(quiet[1].starts_with("2 non-code files were left out"), "{quiet:?}");
        assert_eq!("'--show-skipped' lists them.", quiet[2]);

        let listed = format_files_left_out(&result, true);
        assert_eq!(vec!["-- D:/x/bundle.js".to_owned()], listed[1..2].to_vec());
        assert!(listed.contains(&"-- D:/x/a.d".to_owned()) && listed.contains(&"-- D:/x/b.d".to_owned()));
        assert!(!listed.iter().any(|line| line.contains("lists them")), "{listed:?}");

        assert!(format_files_left_out(&result_with(0, 0), false).is_empty(),
                "a run with nothing skipped still said something");
    }

    // One file is always counted beside whatever went wrong: a run where nothing at all was counted
    // takes a branch of its own, and this fixture must not stand in both.
    fn result_with(unreadable: usize, faulty: usize) -> RunResult {
        let counted = crate::test_support::plain_stats_of(1, 40, 4, 3, 1, HashMap::new());
        RunResult {
            per_language: HashMap::from([("Rust".to_owned(), counted.clone())]), total: counted,
            modules: Vec::new(), nested_languages: HashMap::new(), tests: HashMap::new(), targets: Vec::new(),
            files_present: FilesPresent {total_files: 2 + faulty, relevant_files: 1 + faulty, excluded_files: 0},
            performance: Performance {duration_millis: 0, threads: Threads::new(1, 1)},
            faulty_files: (0..faulty).map(|i| mezura_core::FaultyFileDetails::new(
                    format!("a{i}.rs"), "no".to_owned(), 1)).collect(),
            skipped_files: mezura_core::SkippedFiles::default(),
            unreadable_dirs: (0..unreadable).map(|i| UnreadableDirDetails::new(
                    format!("D:/d{i}"), "Access is denied. (os error 5)".to_owned())).collect(),
            warnings: Vec::new(),
            applied_presets: Vec::new()
        }
    }

    #[test]
    fn the_presets_list_gathers_the_folders_of_a_preset_to_the_width_and_leaves_out_one_that_decided_nothing() {
        let applied = |name: &str, directory: String, trigger_file: &str, matched| AppliedPreset::new(name.to_owned(),
                directory, trigger_file.to_owned(), matched);
        let mut result = result_with(0, 0);
        result.targets = vec![Target::of("D:/proj")];
        result.applied_presets = vec![applied("jest", "D:/proj".to_owned(), "jest.config.js", true),
                applied("pytest", "D:/proj/tools".to_owned(), "conftest.py", false), applied("jest", "D:/".to_owned(), "jest.config.ts", true)];
        result.applied_presets.extend((0..20).map(|i| applied("jest", format!("D:/proj/packages/package-{i:02}"), "jest.config.js", true)));

        let lines = format_applied_presets(&result);
        assert_eq!(3, lines.len(), "a preset that decided nothing got a row: {lines:#?}");
        assert_eq!("  @jest   under ./, D:/, ./packages/package-00 and 19 more (jest.config.js, jest.config.ts)", lines[1]);
        result.applied_presets.truncate(4);
        assert_eq!("  @jest   under ./, D:/, ./packages/package-00 (jest.config.js, jest.config.ts)", format_applied_presets(&result)[1]);
    }

    #[test]
    fn a_generic_project_file_names_the_presets_of_its_ecosystem_until_a_test_of_it_is_found() {
        let root = std::env::temp_dir().join("mezura-preset-suggestion");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("package.json"), "{}").unwrap();
        let mut result = result_with(0, 0);
        result.targets = vec![Target::of(crate::paths::normalise_separators(&root.to_string_lossy()))];
        result.per_language.insert("TypeScript".to_owned(), result.total.clone());

        let suggested = find_presets_to_suggest(&result).iter().map(|names| format_preset_suggestion(names)).collect::<Vec<_>>();
        assert_eq!(vec!["jest, vitest or mocha tests are declared with '--tests @jest', '@vitest' or '@mocha'.".to_owned()], suggested);
        result.applied_presets = vec![AppliedPreset::new("vitest".to_owned(), "D:/".to_owned(), "vitest.config.ts".to_owned(), true)];
        assert!(find_presets_to_suggest(&result).is_empty(), "a preset that found the tests was suggested again");

        std::fs::remove_dir_all(&root).unwrap();
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
            skipped_counts: crate::json_reader::SkippedCounts::default(),
            unreadable_dirs_count: 0,
            files_recorded: true,
            files_hidden: 0,
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

    #[test]
    fn a_project_keeps_its_history_beside_its_code_and_a_named_configuration_keeps_its_own() {
        let mut config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);
        config.view.local_dir = Some(crate::paths::LocalDir::of("D:/work/portal"));

        assert_eq!(Some("D:/work/portal/.mezura/log.jsonl".to_owned()), determine_log_file_path(&config));

        // The folder is still the one this run found, and the name still decides: a configuration
        // somebody asked for by name keeps the history of that configuration and not of the tree
        config.view.config_name_to_load = Some("portal".to_owned());
        assert_eq!(Some("portal.jsonl".to_owned()), determine_log_file_path(&config)
                .map(|path| std::path::Path::new(&path).file_name().unwrap().to_string_lossy().into_owned()));
    }
}
