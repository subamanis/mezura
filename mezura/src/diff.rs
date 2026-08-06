use std::collections::HashMap;

use mezura_core::{RunResult, Stats, render};

use super::config_manager::SortCriterion;
use super::json_reader::{DocumentError, DocumentWarning, Scope};

// The half of a document's warnings that says the numbers themselves may be wrong, as the document
// spells it
const COUNTS_AFFECTED : &str = "counts";

// What the column of the run being made right now is called. Not a date, because it is the one
// reading whose date says nothing: it is this one.
const THIS_RUN_NAME : &str = "this run";

// What a reading is, which its consumers on a screen never ask and a consumer of a saved comparison
// cannot ask anything else: with the file gone from the disk and 'HEAD' pointing somewhere new,
// these fields are the identity, and each source's identity has its own shape.
pub enum Source {
    Run,
    Document { path: String },
    // Both halves, because neither derives from the other: the hash is what the comparison really
    // measured, 'asked_for' is what makes it readable six months later, 'v2.0.1' over '030e6e72a1'.
    Revision { commit: String, asked_for: String }
}

// One reading, whole: where it came from, when it was taken, which mezura counted it, under what
// settings, what that run said about its own counts, and the counts. Every source fills the same
// six; a comparison is two of these and the only difference between its sides is which one was
// written first in the command.
pub struct Reading {
    pub source: Source,
    // As the document writes it, so both sides are read by one function: the file's 'generated_at',
    // the commit's own date, the clock for this run
    pub taken: String,
    pub version: String,
    pub scope: Scope,
    // Empty for a reading counted by this very run, whose warnings were printed as they happened
    pub warnings: Vec<DocumentWarning>,
    pub result: RunResult
}

impl Reading {
    // A revision is counted by this build, over a checkout of it, under the settings of this run,
    // so everything but the counts and the commit's own two facts is this run's own.
    pub fn of_revision(asked_for: &str, commit: String, taken: String, result: RunResult,
            engine: &mezura_core::EngineConfig) -> Self {
        Reading {
            source: Source::Revision { commit, asked_for: asked_for.to_owned() },
            taken,
            version: super::config_manager::VERSION_ID.trim_start_matches('v').to_owned(),
            scope: scope_of(engine),
            warnings: Vec::new(),
            result
        }
    }

    // The one clone in the whole feature: the run's result is still being presented and logged
    // around the comparison, so the reading takes a copy rather than the thing itself.
    pub fn of_this_run(result: &RunResult, taken: &chrono::DateTime<chrono::Local>,
            engine: &mezura_core::EngineConfig) -> Self {
        Reading {
            source: Source::Run,
            taken: taken.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
            version: super::config_manager::VERSION_ID.trim_start_matches('v').to_owned(),
            scope: scope_of(engine),
            warnings: Vec::new(),
            result: result.clone()
        }
    }

    // The identity as a person reads it in a heading or a warning, derived at the moment of
    // printing: the identity itself is the source, and a label is a way of showing one.
    pub fn display_name(&self) -> String {
        match &self.source {
            Source::Run => THIS_RUN_NAME.to_owned(),
            Source::Document { path } => std::path::Path::new(path).file_name()
                    .map_or_else(|| path.clone(), |x| x.to_string_lossy().into_owned()),
            Source::Revision { asked_for, .. } => asked_for.clone()
        }
    }
}

// Every one of these stops the run before a single file is counted, so that a mistake in the
// baseline is not paid for by a scan of the whole tree first.
#[derive(Debug)]
pub enum LoadError {
    Unreadable { path: String, error: std::io::Error },
    NotADocument { path: String, error: DocumentError },
    // A document written with '--top' holds some of its languages and all of its total, so the ones
    // it left out would read as languages that were deleted since.
    Incomplete { path: String, missing: usize }
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable { path, error } => write!(f, "'{path}' could not be read: {error}."),
            // A key that is absent gets its own sentence, because it has a likelier story than the
            // other three: nothing needs to have gone wrong, an older mezura simply had not met it
            Self::NotADocument { path, error: DocumentError::Missing(at) } => write!(f, "'{path}' is incomplete \
and will not be parsed. Maybe it was written by an older version of mezura, or it has been modified. \
It is missing '{at}'."),
            Self::NotADocument { path, error } => write!(f, "'{path}' could not be read as a mezura document. {error}"),
            Self::Incomplete { path, missing } => write!(f, "'{path}' was written with '--top' and is missing {missing} of \
its languages, so comparing against it would report every one of them as deleted. Write it again without '--top'.")
        }
    }
}

impl std::error::Error for LoadError {
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

pub fn load(path: &str) -> Result<Reading, LoadError> {
    let contents = std::fs::read_to_string(path)
            .map_err(|error| LoadError::Unreadable { path: path.to_owned(), error })?;
    let document = super::json_reader::parse(&contents)
            .map_err(|error| LoadError::NotADocument { path: path.to_owned(), error })?;
    if document.languages_hidden > 0 {
        return Err(LoadError::Incomplete { path: path.to_owned(), missing: document.languages_hidden });
    }

    Ok(Reading { source: Source::Document { path: path.to_owned() }, taken: document.generated_at,
            version: document.mezura_version, scope: document.scope, warnings: document.warnings,
            result: document.result })
}

pub fn change_of(before: usize, now: usize) -> Change {
    match (before, now) {
        (0, 0) => Change::Percent(0.0),
        (0, _) => Change::Appeared,
        (_, 0) => Change::Gone,
        (before, now) => Change::Percent(render::relative_change(before, now))
    }
}

// Every language of either reading, sorted the way the report would have been. 'top' is the screen's
// cut and is not applied when a document is being written, which holds every row the same way the
// run's own document does.
pub fn comparison_rows(baseline: &HashMap<String, Stats>, now: &HashMap<String, Stats>,
        sort_by: SortCriterion, top: Option<usize>) -> Vec<Row>
{
    // Held at what each is now, so one that disappeared sorts to the bottom where a zero belongs
    let mut merged = now.clone();
    for name in baseline.keys() {
        merged.entry(name.clone()).or_default();
    }

    let names = super::result_printer::get_sorted_language_names(&merged, sort_by);
    let shown = top.map_or(names.len(), |x| x.min(names.len()));

    names[..shown].iter().map(|name| Row {
        before: baseline.get(name).cloned().unwrap_or_default(),
        now: now.get(name).cloned().unwrap_or_default(),
        name: name.clone()
    }).collect()
}

// The settings of a run in the shape a document records them, so that a comparison asks the same
// question of both its sides whatever each of them came from.
//
// The gitignore flag is turned around here and nowhere else: a document records whether the file was
// obeyed, and the command line records whether it was not.
pub fn scope_of(engine: &mezura_core::EngineConfig) -> Scope {
    Scope {
        exclude: engine.exclude_dirs.clone(),
        languages: engine.languages_of_interest.clone(),
        excluded_languages: engine.excluded_languages.clone(),
        forced_languages: engine.forced_languages.clone(),
        braces_as_code: engine.braces_as_code,
        search_in_dotted: engine.should_search_in_dotted,
        gitignore: !engine.no_gitignore
    }
}

// A difference here is not a change in the code, and every one of these can move a count on its own.
pub fn settings_that_differ(baseline: &Scope, subject: &Scope) -> Vec<&'static str> {
    let same = |a: &[String], b: &[String]| {
        let (mut a, mut b) = (a.to_vec(), b.to_vec());
        a.sort();
        b.sort();
        a == b
    };

    let mut differ = Vec::new();
    if !same(&baseline.exclude, &subject.exclude) {differ.push("--exclude")}
    if !same(&baseline.languages, &subject.languages) {differ.push("--languages")}
    if !same(&baseline.excluded_languages, &subject.excluded_languages) {differ.push("--exclude-languages")}
    if baseline.forced_languages != subject.forced_languages {differ.push("--force-lang")}
    if baseline.braces_as_code != subject.braces_as_code {differ.push("--braces-as-code")}
    if baseline.search_in_dotted != subject.search_in_dotted {differ.push("--search-in-dotted")}
    if baseline.gitignore != subject.gitignore {differ.push("--no-gitignore")}

    differ
}

// What the run that wrote the baseline said about its own counts. It said it on the error output of
// a run nobody is looking at any more, and a reading taken under a doubt is not something the next
// one can be measured against: an unreadable language file leaves a whole language at zero, which
// this run would report as a language that appeared out of nowhere.
pub fn doubts_about(warnings: &[DocumentWarning]) -> Vec<String> {
    warnings.iter().filter(|x| x.affects == COUNTS_AFFECTED)
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
        let before = hashmap!["Rust".to_owned() => stats(100, 70, 2), "Java".to_owned() => stats(40, 30, 1)];
        let now = hashmap!["Rust".to_owned() => stats(150, 100, 3), "Go".to_owned() => stats(60, 50, 1)];

        let rows = comparison_rows(&before, &now, SortCriterion::Lines, None);
        assert_eq!(vec!["Rust".to_owned(), "Go".to_owned(), "Java".to_owned()],
                rows.iter().map(|x| x.name.clone()).collect::<Vec<_>>());
        // the one that is gone sorts last, holding the zero it is now, and keeps every figure it had
        assert_eq!(40, rows[2].before.lines);
        assert_eq!(30, rows[2].before.code_lines);
        assert_eq!(0, rows[2].now.lines);
        // and the one that appeared has a whole empty reading behind it rather than a missing one
        assert_eq!(0, rows[1].before.lines);
        assert_eq!(60, rows[1].now.lines);

        // '--top' cuts these rows the way it cuts the report, and a document asks for no cut
        assert_eq!(2, comparison_rows(&before, &now, SortCriterion::Lines, Some(2)).len());
        assert_eq!(3, comparison_rows(&before, &now, SortCriterion::Lines, None).len());

        // and '--sort' orders them, as it does everywhere else
        assert_eq!(vec!["Go".to_owned(), "Java".to_owned(), "Rust".to_owned()],
                comparison_rows(&before, &now, SortCriterion::Name, None).iter()
                        .map(|x| x.name.clone()).collect::<Vec<_>>());
    }

    // Two readings taken under different rules are two measurements, and the difference between them
    // is not a change in the code. Only what can move a count is asked about.
    #[test]
    fn the_settings_the_two_readings_were_taken_under_are_compared() {
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let per_language = hashmap!["Rust".to_owned() => stats(100, 70, 2)];
        let result = mezura_core::RunResult {
            total: Stats::total_of(&per_language), per_language, modules: Vec::new(),
            faulty_files: Vec::new(), targets: Vec::new(), unreadable_dirs: Vec::new(),
            files_present: mezura_core::FilesPresent {total_files: 2, relevant_files: 2, excluded_files: 0},
            performance: mezura_core::Performance {duration_millis: 0, threads: mezura_core::Threads::new(1, 1)}
        };
        let document = crate::json_reader::parse(&crate::json_printer::document(&result,
                &chrono::Local::now(), &crate::config_manager::Configuration::new(vec!["./src".to_owned()]))).unwrap();
        assert!(settings_that_differ(&document.scope, &scope_of(&config.engine)).is_empty());

        // the order they were written in is not a difference
        config.engine.exclude_dirs = vec!["target".to_owned()];
        assert_eq!(vec!["--exclude"], settings_that_differ(&document.scope, &scope_of(&config.engine)));

        // It decides which language a file is counted as, so a run that forced one and a run that
        // did not measured different things and the difference is not code that changed
        config.engine.exclude_dirs = Vec::new();
        config.engine.forced_languages = hashmap!["m".to_owned() => "matlab".to_owned()];
        assert_eq!(vec!["--force-lang"], settings_that_differ(&document.scope, &scope_of(&config.engine)));

        config.engine.forced_languages = HashMap::new();
        config.engine.braces_as_code = true;
        config.engine.no_gitignore = true;
        assert_eq!(vec!["--braces-as-code", "--no-gitignore"], settings_that_differ(&document.scope, &scope_of(&config.engine)));

        // and hiding the keywords is not among them, since it moves no count that is compared here
        config.engine.braces_as_code = false;
        config.engine.no_gitignore = false;
        config.engine.count_keywords = false;
        assert!(settings_that_differ(&document.scope, &scope_of(&config.engine)).is_empty());
    }
}
