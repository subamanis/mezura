use std::collections::HashMap;

use mezura_core::{Stats, render};

use super::config_manager::Configuration;
use super::json_reader::{Document, DocumentError};

// The half of a document's warnings that says the numbers themselves may be wrong, as the document
// spells it
const COUNTS_AFFECTED : &str = "counts";

// A reading to compare this run against, and the name to call it by, which is the file's own.
pub struct Baseline {
    pub name: String,
    pub document: Document
}

// Every one of these stops the run before a single file is counted, so that a mistake in the
// baseline is not paid for by a scan of the whole tree first.
#[derive(Debug)]
pub enum BaselineError {
    Unreadable { path: String, error: std::io::Error },
    NotADocument { path: String, error: DocumentError },
    // A document written with '--top' holds some of its languages and all of its total, so the ones
    // it left out would read as languages that were deleted since.
    Incomplete { path: String, missing: usize }
}

impl std::fmt::Display for BaselineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable { path, error } => write!(f, "'{path}' could not be read: {error}."),
            Self::NotADocument { path, error } => write!(f, "'{path}' is not a document mezura wrote. {error}"),
            Self::Incomplete { path, missing } => write!(f, "'{path}' was written with '--top' and is missing {missing} of \
its languages, so comparing against it would report every one of them as deleted. Write it again without '--top'.")
        }
    }
}

impl std::error::Error for BaselineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unreadable { error, .. } => Some(error),
            Self::NotADocument { error, .. } => Some(error),
            Self::Incomplete { .. } => None
        }
    }
}

// What one language came to in the two readings. Both sides are the whole of what was counted, so
// that every figure of the report has a change to put beside it and not only the one being sorted by.
pub struct Row {
    pub name: String,
    pub before: Stats,
    pub now: Stats
}

// A jump out of nothing is not a percentage and neither is a fall to nothing, and both have to be
// told apart from standing still: 'relative_change' answers 0.0 when there was nothing to grow from,
// which would print as "no change" for a figure that was not there at all.
#[derive(Debug,PartialEq)]
pub enum Change {
    Appeared,
    Gone,
    Percent(f64)
}

// Splits '--diff a.json..b.json' into the two readings it names, and answers None for the second
// when only one was given, which is the form whose second reading is this run.
//
// The trap is that '..' is a separator here and a directory in every filesystem, so '--diff
// ../old.json' must not come apart into an empty name and '/old.json'. The rule is git's own: what
// was written is taken whole if it names something that exists, and only split if it does not.
pub fn split_operand(value: &str) -> Result<(&str, Option<&str>), String> {
    // What was written wins if it names something that is really there, which is what tells
    // 'a/../b.json', a path that climbs on its way to a file beside 'a', apart from a pair
    if std::path::Path::new(value).exists() {
        return Ok((value, None));
    }
    // Beyond one, there is no telling which is the separator and which is a climb, and guessing
    // would mean asking the disk about every way of cutting it, then asking git about every way once
    // revisions are allowed here. Refused instead, with the way out in the message.
    if value.matches("..").count() > 1 {
        return Err(format!("'{value}' has more than one '..' in it, and only one of them can be the \
separator between the two readings. Write the paths out without the '..' that climbs."));
    }

    match value.split_once("..") {
        Some((before, after)) if !before.is_empty() && !after.is_empty() => Ok((before, Some(after))),
        // A separator with nothing after it is a line that was left half written, and saying so is
        // worth more than the "no such file" that reading it whole would produce
        Some((before, _)) if !before.is_empty() => Err(format!("'{value}' names a reading before the \
'..' and none after it. Write the second one, or drop the '..' to compare '{before}' against this run.")),
        // Nothing before it is an ordinary path climbing a directory, and a missing file is reported
        // as a missing file
        _ => Ok((value, None))
    }
}

pub fn load(path: &str) -> Result<Baseline, BaselineError> {
    let contents = std::fs::read_to_string(path)
            .map_err(|error| BaselineError::Unreadable { path: path.to_owned(), error })?;
    let document = super::json_reader::parse(&contents)
            .map_err(|error| BaselineError::NotADocument { path: path.to_owned(), error })?;
    if document.languages_hidden > 0 {
        return Err(BaselineError::Incomplete { path: path.to_owned(), missing: document.languages_hidden });
    }

    let name = std::path::Path::new(path).file_name()
            .map_or_else(|| path.to_owned(), |x| x.to_string_lossy().into_owned());

    Ok(Baseline { name, document })
}

pub fn change_of(before: usize, now: usize) -> Change {
    match (before, now) {
        (0, 0) => Change::Percent(0.0),
        (0, _) => Change::Appeared,
        (_, 0) => Change::Gone,
        (before, now) => Change::Percent(render::relative_change(before, now))
    }
}

// Every language of either reading, sorted and cut the way the report would have been, so that a
// comparison holds the rows a plain run of the same command would have held.
pub fn comparison_rows(baseline: &HashMap<String, Stats>, now: &HashMap<String, Stats>,
        config: &Configuration) -> Vec<Row>
{
    // Held at what each is now, so one that disappeared sorts to the bottom where a zero belongs
    let mut merged = now.clone();
    for name in baseline.keys() {
        merged.entry(name.clone()).or_default();
    }

    let names = super::result_printer::get_sorted_language_names(&merged, config.view.sort_by);
    let shown = config.view.top_n.map_or(names.len(), |top| top.min(names.len()));

    names[..shown].iter().map(|name| Row {
        before: baseline.get(name).cloned().unwrap_or_default(),
        now: now.get(name).cloned().unwrap_or_default(),
        name: name.clone()
    }).collect()
}

// The settings that decided what got counted, as the baseline had them against as this run has them.
// A difference here is not a change in the code, and every one of these can move a count on its own.
pub fn settings_that_differ(baseline: &Document, config: &Configuration) -> Vec<&'static str> {
    let (scope, engine) = (&baseline.scope, &config.engine);
    let same = |a: &[String], b: &[String]| {
        let (mut a, mut b) = (a.to_vec(), b.to_vec());
        a.sort();
        b.sort();
        a == b
    };

    let mut differ = Vec::new();
    if !same(&scope.exclude, &engine.exclude_dirs) {differ.push("--exclude")}
    if !same(&scope.languages, &engine.languages_of_interest) {differ.push("--languages")}
    if !same(&scope.excluded_languages, &engine.excluded_languages) {differ.push("--exclude-languages")}
    if scope.braces_as_code != engine.braces_as_code {differ.push("--braces-as-code")}
    if scope.search_in_dotted != engine.should_search_in_dotted {differ.push("--search-in-dotted")}
    // The document records whether the file was obeyed, and the flag records whether it was not
    if scope.gitignore == engine.no_gitignore {differ.push("--no-gitignore")}

    differ
}

// What the run that wrote the baseline said about its own counts. It said it on the error output of
// a run nobody is looking at any more, and a reading taken under a doubt is not something the next
// one can be measured against: an unreadable language file leaves a whole language at zero, which
// this run would report as a language that appeared out of nowhere.
pub fn doubts_about(baseline: &Document) -> Vec<String> {
    baseline.warnings.iter().filter(|x| x.affects == COUNTS_AFFECTED)
            .map(|x| format!("{} ({})", x.message, x.code)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_manager::SortCriterion;

    fn stats(lines: usize, code: usize, files: usize) -> Stats {
        Stats::new(files, lines * 30, lines, code, 0, HashMap::new())
    }

    // '..' separates the two readings and is also a directory on every filesystem there is, so the
    // two meanings have to be told apart before anything else happens.
    #[test]
    fn a_pair_of_readings_is_told_apart_from_a_path_that_climbs() {
        let split = |x| split_operand(x).unwrap();
        assert_eq!(("a.json", Some("b.json")), split("a.json..b.json"));
        assert_eq!(("main", Some("HEAD")), split("main..HEAD"));
        assert_eq!(("old.json", None), split("old.json"));

        // A file that is really there is taken whole, whatever is in the way of it. Cargo runs these
        // from the package root, so this one climbs to the repository's own README.
        assert_eq!(("../README.md", None), split("../README.md"));
        // and a climb that leads nowhere still holds together, because it leaves nothing on its
        // left to be a reading
        assert_eq!(("../gone.json", None), split("../gone.json"));

        // A separator with nothing after it is a half written line and says so, while nothing before
        // it is just a path climbing, whose missing file is reported as a missing file
        let half = split_operand("a.json..").unwrap_err();
        assert!(half.contains("none after it") && half.contains("'a.json'"), "{half}");
        assert_eq!(("..b.json", None), split("..b.json"));
        assert_eq!(("..", None), split(".."));

        // Two climbs and a separator cannot be told apart by anything short of asking the disk about
        // every way of cutting the line, so it is refused and the message says what to write instead
        let refused = split_operand("a/../b.json..c/../d.json").unwrap_err();
        assert!(refused.contains("more than one '..'") && refused.contains("climbs"), "{refused}");
        // and a file that really is there is still taken whole, however many climbs are in it
        assert_eq!(("../mezura/../README.md", None), split("../mezura/../README.md"));
    }

    // The one figure a comparison cannot express as a percentage, and the reason it needs a word
    // instead: 'relative_change' answers 0.0 when there was nothing to grow from, which is the same
    // answer it gives a figure that did not move at all.
    #[test]
    fn a_figure_that_only_one_of_the_two_readings_has_is_named_and_not_given_a_percentage() {
        assert_eq!(Change::Appeared, change_of(0, 500));
        assert_eq!(Change::Gone, change_of(500, 0));
        assert_eq!(Change::Percent(0.0), change_of(0, 0));
        assert_eq!(Change::Percent(100.0), change_of(100, 200));
        assert_eq!(Change::Percent(-50.0), change_of(100, 50));
    }

    // The rows are the union of the two readings, because a language that was deleted has to have a
    // row saying so, and one added has no row in the baseline to be found under.
    #[test]
    fn every_language_of_either_reading_gets_a_row_in_the_order_the_report_uses() {
        let mut config = Configuration::new(vec!["./src".to_owned()]);
        let before = hashmap!["Rust".to_owned() => stats(100, 70, 2), "Java".to_owned() => stats(40, 30, 1)];
        let now = hashmap!["Rust".to_owned() => stats(150, 100, 3), "Go".to_owned() => stats(60, 50, 1)];

        let rows = comparison_rows(&before, &now, &config);
        assert_eq!(vec!["Rust".to_owned(), "Go".to_owned(), "Java".to_owned()],
                rows.iter().map(|x| x.name.clone()).collect::<Vec<_>>());
        // the one that is gone sorts last, holding the zero it is now, and keeps every figure it had
        assert_eq!(40, rows[2].before.lines);
        assert_eq!(30, rows[2].before.code_lines);
        assert_eq!(0, rows[2].now.lines);
        // and the one that appeared has a whole empty reading behind it rather than a missing one
        assert_eq!(0, rows[1].before.lines);
        assert_eq!(60, rows[1].now.lines);

        // '--top' cuts these rows the way it cuts the report
        config.view.top_n = Some(2);
        assert_eq!(2, comparison_rows(&before, &now, &config).len());

        // and '--sort' orders them, as it does everywhere else
        config.view.top_n = None;
        config.view.sort_by = SortCriterion::Name;
        assert_eq!(vec!["Go".to_owned(), "Java".to_owned(), "Rust".to_owned()],
                comparison_rows(&before, &now, &config).iter().map(|x| x.name.clone()).collect::<Vec<_>>());
    }

    // Two readings taken under different rules are two measurements, and the difference between them
    // is not a change in the code. Only what can move a count is asked about.
    #[test]
    fn the_settings_the_two_readings_were_taken_under_are_compared() {
        let mut config = Configuration::new(vec!["./src".to_owned()]);
        let per_language = hashmap!["Rust".to_owned() => stats(100, 70, 2)];
        let result = mezura_core::RunResult {
            total: Stats::total_of(&per_language), per_language, modules: Vec::new(),
            faulty_files: Vec::new(), targets: Vec::new(), unreadable_dirs: Vec::new(),
            files_present: mezura_core::FilesPresent {total_files: 2, relevant_files: 2, excluded_files: 0},
            performance: mezura_core::Performance {duration_millis: 0, threads: mezura_core::Threads::new(1, 1)}
        };
        let document = crate::json_reader::parse(&crate::json_printer::document(&result,
                &chrono::Local::now(), &Configuration::new(vec!["./src".to_owned()]))).unwrap();
        assert!(settings_that_differ(&document, &config).is_empty());

        // the order they were written in is not a difference
        config.engine.exclude_dirs = vec!["target".to_owned()];
        assert_eq!(vec!["--exclude"], settings_that_differ(&document, &config));

        config.engine.exclude_dirs = Vec::new();
        config.engine.braces_as_code = true;
        config.engine.no_gitignore = true;
        assert_eq!(vec!["--braces-as-code", "--no-gitignore"], settings_that_differ(&document, &config));

        // and hiding the keywords is not among them, since it moves no count that is compared here
        config.engine.braces_as_code = false;
        config.engine.no_gitignore = false;
        config.engine.count_keywords = false;
        assert!(settings_that_differ(&document, &config).is_empty());
    }
}
