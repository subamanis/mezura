// What one run produced, as opposed to the words it is described in, which are in 'domain.rs': a
// 'Language' exists before anything has been counted, a 'Stats' of it does not.
use std::collections::HashMap;

use crate::Stats;
use crate::engine::config::{Target, Threads};
use crate::engine::modules::{ModuleId, Modules};

#[derive(Debug,Clone)]
pub struct RunResult {
    // Across every module. A run where nothing was named has one module holding these same numbers.
    pub per_language: HashMap<String, Stats>,
    pub total: Stats,
    pub modules: Vec<ModuleResult>,
    pub faulty_files: Vec<FaultyFileDetails>,
    pub files_present: FilesPresent,
    pub performance: Performance,
    // The places actually visited: the targets as given, resolved, with every pattern expanded to
    // what it matched at that moment. What a log or a document records to say "these two runs
    // measured the same thing", since the same './src' over two different trees is two different
    // measurements and a pattern's matches change while its text does not.
    pub targets: Vec<Target>,
    // Directories that could not be opened, so everything inside them is missing from every number
    // above. Empty on an ordinary run, and the one thing that says the counts are short.
    pub unreadable_dirs: Vec<UnreadableDirDetails>
}

impl RunResult {
    // Largest first by the chosen figure, ties broken by name.
    pub fn languages_sorted_by(&self, criterion: SortCriterion) -> Vec<(&str, &Stats)> {
        sorted_by(&self.per_language, criterion)
    }

    // The emptiness check is not redundant: without it the two counts are equal when both are zero,
    // and a scan that found nothing would answer yes.
    pub fn all_relevant_files_were_faulty(&self) -> bool {
        !self.faulty_files.is_empty() && self.faulty_files.len() == self.files_present.relevant_files
    }

    // Finding no files after failing to open a directory is not "no code here", it is "I could not
    // look". An otherwise empty tree with one unreadable corner answers yes as well, on purpose:
    // that corner may have held everything.
    pub fn nothing_could_be_read(&self) -> bool {
        self.files_present.relevant_files == 0 && !self.unreadable_dirs.is_empty()
    }

    // One name is enough for the second column to appear.
    pub fn has_modules(&self) -> bool {
        self.modules.iter().any(|x| x.name.is_some())
    }

    // Nothing of interest was found, which is an answer and not a failure: the counts are zero and
    // the file numbers still say how many were looked at and how many were excluded.
    //
    // The modules are built even here. One that found nothing was still asked for by name, and
    // leaving it out would make 'has_modules' say no and take the whole block out of the document
    // exactly when the scan came back empty.
    pub(crate) fn of_nothing(files_present: FilesPresent, performance: Performance, modules: &Modules,
            targets: Vec<Target>, unreadable_dirs: Vec<UnreadableDirDetails>) -> Self {
        RunResult {
            per_language: HashMap::new(),
            total: Stats::default(),
            modules: (0..modules.count()).map(|id| ModuleResult {
                name: modules.name_of(id as ModuleId).map(str::to_owned),
                per_language: HashMap::new(),
                total: Stats::default()
            }).collect(),
            faulty_files: Vec::new(),
            files_present,
            performance,
            targets,
            unreadable_dirs
        }
    }
}

// 'name' is None for whatever the named parts left over, which is also the single part of a run
// where nothing was named at all.
#[derive(Debug,Clone)]
pub struct ModuleResult {
    pub name: Option<String>,
    pub per_language: HashMap<String, Stats>,
    pub total: Stats
}

impl ModuleResult {
    pub fn languages_sorted_by(&self, criterion: SortCriterion) -> Vec<(&str, &Stats)> {
        sorted_by(&self.per_language, criterion)
    }
}

// 'threads' is what the run actually got, not what was asked for: the operating system may grant
// fewer and the run carries on with those.
#[derive(Debug,Clone)]
pub struct Performance {
    pub duration_millis: u128,
    pub threads: Threads
}

#[derive(Debug,Default,Clone,Copy,PartialEq,Eq)]
pub struct FilesPresent {
    pub total_files: usize,
    pub relevant_files: usize,
    pub excluded_files: usize
}

// Which figure decides the order of a report's rows. Here rather than in whatever draws one, because
// every consumer of a result sorts it and there is one sensible way to break a tie.
#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub enum SortCriterion {
    Files,
    #[default]
    Lines,
    Code,
    Size,
    Name
}

impl SortCriterion {
    // The spelling a person types and the one a configuration file stores are the same word. Kept
    // with the enum so that the name a run was sorted by can be written into a log or a document
    // without copying the vocabulary out.
    pub fn parse(value: &str) -> Option<SortCriterion> {
        match value.trim().to_lowercase().as_str() {
            "files" => Some(Self::Files),
            "lines" => Some(Self::Lines),
            "code" => Some(Self::Code),
            "size" => Some(Self::Size),
            "name" => Some(Self::Name),
            _ => None
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Lines => "lines",
            Self::Code => "code",
            Self::Size => "size",
            Self::Name => "name"
        }
    }
}

// Its lines are in no total, but it is counted among the files that were seen.
#[derive(Debug,Clone)]
#[non_exhaustive]
pub struct FaultyFileDetails {
    pub path: String,
    pub error_msg: String,
    pub size: u64
}

impl FaultyFileDetails {
    pub fn new(path: String, error_msg: String, size: u64) -> Self {
        FaultyFileDetails {
            path,
            error_msg,
            size
        }
    }
}

// The reason belongs beside the path because a permission and a directory deleted mid-scan send the
// reader to different places, and on a whole drive there are hundreds of these.
#[derive(Debug,Clone)]
#[non_exhaustive]
pub struct UnreadableDirDetails {
    pub path: String,
    pub error_msg: String
}

impl UnreadableDirDetails {
    // Both types here are non-exhaustive, so a crate outside this one cannot build them field by
    // field and needs a constructor.
    pub fn new(path: String, error_msg: String) -> Self {
        UnreadableDirDetails {
            path,
            error_msg
        }
    }
}

// A run that produced nothing at all, as opposed to a run that produced zeros. Finding no code is a
// result, and every file failing to parse is a result with the reasons attached, so neither is here.
#[derive(Debug)]
#[non_exhaustive]
pub enum RunError {
    // Nowhere to look is a malformed question, not an empty answer. The command line cannot ask it,
    // since a bare run falls back to the working directory; a caller that built a configuration with
    // no directories almost certainly forgot them, and an Ok full of zeros would dress that up as a
    // measurement.
    NoTargets,
    // The languages were resolved against a configuration that selects a different set from the one
    // handed to the run. Refused rather than counted, because the answer would look exactly like the
    // answer to the question that was asked: resolving with a configuration naming Rust and then
    // running with one naming Python counted Rust and said nothing. Only the three fields resolution
    // reads are compared, with case and order folded, so resolving once and counting several
    // directories is untouched.
    LanguagesFromAnotherConfig,
    // The targets could not be turned into places to visit: a path that names nothing, a pattern
    // that does not parse or matches nothing, or one place given under two names.
    InvalidTargets(crate::engine::targets::TargetError),
    // The pattern as the caller wrote it, not the longer form the matcher builds from it.
    InvalidExcludePattern(String),
    // The operating system refused every thread of one side. Refusing some but not all is not an
    // error: fewer threads is the same answer arriving slower. Zero is different, because zero
    // scanning threads find nothing and zero counting threads count nothing, and either would dress
    // a non-answer up as an empty one.
    NoThreadsAvailable { side: &'static str, error: std::io::Error },
    // A worker died mid-run. Each merges its share of the counting at the end, so whatever it had
    // done is lost with it, and a number known to be short is never returned as an answer. The
    // panic's message travels here; its location already reached the error output through the panic
    // hook, at the moment it happened.
    IncompleteRun { worker_panic: String }
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoTargets => write!(f, "The configuration names no directories or files, so there is nothing to count."),
            Self::LanguagesFromAnotherConfig => write!(f, "The languages were resolved against a configuration that selects a different set of them than the one this run was given, so the counts would not be the ones the settings describe. Resolve them against the same configuration you are counting with."),
            Self::InvalidTargets(x) => write!(f, "{x} Nothing was counted."),
            Self::InvalidExcludePattern(x) => write!(f, "'{x}' is not a valid exclude pattern, so nothing was counted."),
            Self::NoThreadsAvailable { side, error } => write!(f, "The operating system refused every {side} thread, so the run could not start: {error}"),
            Self::IncompleteRun { worker_panic } => write!(f, "A worker thread died mid-run, so the counts would have been incomplete and were discarded: {worker_panic}")
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidTargets(x) => Some(x),
            _ => None
        }
    }
}

// On the type from 'domain.rs' but about results, which is why it is here: adding languages together
// is what the last row of a report holds. The keywords add up with the rest, so "how many classes are
// in this project" has an answer across the several languages that have them.
impl Stats {
    pub fn total_of(languages: &HashMap<String, Stats>) -> Self {
        let mut total = Stats::default();
        for stats in languages.values() {
            total.add(stats);
        }
        total
    }
}

// Shared by the whole run and by one module of it, which are the same question asked of two maps.
fn sorted_by(per_language: &HashMap<String, Stats>, criterion: SortCriterion) -> Vec<(&str, &Stats)> {
    let value_of = |stats: &Stats| match criterion {
        SortCriterion::Files => stats.files,
        SortCriterion::Size => stats.bytes,
        SortCriterion::Lines => stats.lines,
        SortCriterion::Code => stats.code_lines,
        SortCriterion::Name => 0
    };

    let mut rows = per_language.iter().map(|(name, stats)| (name.as_str(), stats)).collect::<Vec<_>>();
    if criterion == SortCriterion::Name {
        rows.sort_by_key(|(name, _)| name.to_lowercase());
    } else {
        // The name breaks every tie, with case folded, so two languages of equal size cannot swap
        // places between two runs of the same command because a map iterated differently.
        rows.sort_by(|a, b| value_of(b.1).cmp(&value_of(a.1))
                .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    }

    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::modules::Modules;

    fn result_with(relevant: usize, unreadable: &[&str]) -> RunResult {
        let unreadable = unreadable.iter().map(|path| UnreadableDirDetails {
            path: (*path).to_owned(), error_msg: "Access is denied. (os error 5)".to_owned()
        }).collect();
        let mut result = RunResult::of_nothing(
                FilesPresent { total_files: relevant, relevant_files: relevant, excluded_files: 0 },
                Performance { duration_millis: 0, threads: Threads::new(1, 1) },
                &Modules::of(&[]), Vec::new(), unreadable);
        result.files_present.relevant_files = relevant;
        result
    }

    // An empty readable tree is an answer, an empty scan that failed to open something is not, and
    // finding files makes the question moot.
    #[test]
    fn an_empty_scan_is_suspect_only_when_something_was_unreadable() {
        assert!(!result_with(0, &[]).nothing_could_be_read());
        assert!(result_with(0, &["D:/gone"]).nothing_could_be_read());
        assert!(!result_with(3, &["D:/gone"]).nothing_could_be_read());
        assert!(!result_with(3, &[]).nothing_could_be_read());
    }
}
