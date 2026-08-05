// What one run produced, as opposed to the vocabulary it produced it in, which is 'domain.rs'. A
// 'Language' exists before anything has been counted; a 'Stats' of it does not.
use std::collections::HashMap;

use crate::Stats;
use crate::engine::config::{Target, Threads};
use crate::engine::modules::{ModuleId, Modules};

// What one run produces, and the only thing 'run' returns. Presentation is a separate call, so the
// same result can be printed, written as JSON, compared with another one, or read by a caller that
// wants none of those.
#[derive(Debug)]
pub struct RunResult {
    // What each language came to, across every module. A run that named none has exactly one module
    // holding the same numbers, and reading these is what every question about the whole run goes
    // through. 'total' is the same measurement summed, in the same type, so the last row of a report
    // is built the way its other rows are.
    pub per_language: HashMap<String, Stats>,
    pub total: Stats,
    pub modules: Vec<ModuleResult>,
    pub faulty_files: Vec<FaultyFileDetails>,
    pub files_present: FilesPresent,
    pub performance: Performance,
    // The places the run actually walked: the declared targets, resolved, with every pattern
    // expanded to what it matched at the moment of the run. This is what a log or a document that
    // wants to say "these two runs measured the same thing" has to record, because the declared
    // form answers a different question: the same './src' declared over two different trees is two
    // different measurements, and a pattern's matches change while its text does not.
    pub targets: Vec<Target>,
    // Directories the walk found and could not open, so everything under them is missing from every
    // number above. Empty on an ordinary run, and the one thing that says the counts are short.
    pub unreadable_dirs: Vec<UnreadableDirDetails>
}

// One part of the run, counted on its own. 'name' is None for the leftovers of the named ones, which
// is also the single unnamed one of a run that declared no modules at all.
#[derive(Debug)]
pub struct ModuleResult {
    pub name: Option<String>,
    pub per_language: HashMap<String, Stats>,
    pub total: Stats
}

impl ModuleResult {
    // The same order as the run's own, asked of this one part of it
    pub fn languages_sorted_by(&self, criterion: SortCriterion) -> Vec<(&str, &Stats)> {
        sorted_by(&self.per_language, criterion)
    }
}

// Shared by the whole run and by one module of it, which are the same question asked of two maps
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
        // The name breaks every tie, folded, so that two languages of equal size cannot swap places
        // between two runs of the same command because a map happened to iterate differently
        rows.sort_by(|a, b| value_of(b.1).cmp(&value_of(a.1))
                .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
    }

    rows
}

// How long the counting took and what it had to work with, which is the pair that makes either
// number worth anything: a duration says nothing without the threads behind it. The threads are
// what the run actually used and not what was asked for, since the operating system is allowed to
// grant fewer and the run carries on with what it was given.
//
// The rates a report shows are arithmetic on these two and are worked out by whoever shows them,
// rather than living here as an Option that reads as "could not be measured" when what it meant was
// "your run was quick".
#[derive(Debug)]
pub struct Performance {
    pub duration_millis: u128,
    pub threads: Threads
}

// Which number decides the order of a report's rows. Here rather than in whatever draws one,
// because every consumer of a result sorts it and there is only one sensible way to break a tie:
// by name, folded, so that two languages of equal size never swap places between two runs of the
// same command. The command line's '--sort' parses into this.
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
    // The spelling a person types and the one a configuration stores, which are the same word. Here
    // with the enum rather than beside the argument parser, so that the name a run was sorted by can
    // be written into a log or a document without the enum's own vocabulary being copied out.
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

impl RunResult {
    // The languages in the order a report shows them, largest first by the chosen figure and ties
    // broken by name. Offered here because the alternative is what every caller was writing: pull
    // the map into a vector of pairs, sort it, and then map it back to whichever number was wanted.
    pub fn languages_sorted_by(&self, criterion: SortCriterion) -> Vec<(&str, &Stats)> {
        sorted_by(&self.per_language, criterion)
    }

    // Nothing of interest was found, which is an answer and not a failure: the counts are zero and
    // the file numbers still say how many were looked at and how many were excluded.
    //
    // The modules are built here for the same reason the ordinary path builds them: one that found
    // nothing was still asked for by name, and its absence reads as a mistake in the report rather
    // than as an empty part. Leaving them out also made the two answers disagree, since 'has_modules'
    // then said no and the whole block vanished from the document exactly when the scan was empty.
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

    // Whether files were found and every one of them failed to parse, which is a different answer
    // from the empty scan: there nothing failed, because there was nothing. Offered here because the
    // obvious comparison is wrong exactly there, the two counts being equal when both are zero.
    pub fn all_relevant_files_were_faulty(&self) -> bool {
        !self.faulty_files.is_empty() && self.faulty_files.len() == self.files_present.relevant_files
    }

    // Whether the zero this result reports is a count of anything: a scan that found no relevant
    // files after failing to open a directory is not "no code here", it is "I could not look".
    // An empty but readable tree with one unreadable corner also answers yes, deliberately: the
    // corner that could not be opened may have held everything, so the zero is suspect either way.
    pub fn nothing_could_be_read(&self) -> bool {
        self.files_present.relevant_files == 0 && !self.unreadable_dirs.is_empty()
    }

    // Whether the report has a second axis at all. One name is enough for the column to appear, and
    // without one there is nothing to group by and the output is what it always was.
    pub fn has_modules(&self) -> bool {
        self.modules.iter().any(|x| x.name.is_some())
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub struct FaultyFileDetails {
    pub path: String,
    pub error_msg: String,
    pub size: u64
}

// A place the walk could not open, and why. The reason used to be discarded at the one line that
// records these, so every one of them was reported with the same sentence whether it was a
// permission, a path that had gone away between being queued and being opened, or a name the
// filesystem refused. On a whole drive that is hundreds of directories under one word.
//
// The same shape as the faulty files above and for the same reason: what a reader needs is the place
// and the reason, in one row, without a second list to cross-reference.
#[derive(Debug)]
#[non_exhaustive]
pub struct UnreadableDirDetails {
    pub path: String,
    pub error_msg: String
}

// A mistake in the configuration itself, as opposed to something the counting found. What can be
// judged with no setting known, a typed path that names nothing, the command line still refuses
// where it was typed; what depends on the merged settings, which is every pattern and every target
// a configuration file declared, is the run's to judge and arrives here. Finding nothing is a
// result, and every file failing to parse is a result with the reasons attached, so neither lives
// here.
#[derive(Debug)]
#[non_exhaustive]
pub enum RunError {
    // No places to look is a malformed question and not an empty answer. The command line can
    // never ask it, because a bare run falls back to the working directory; a library caller who
    // built a configuration without dirs almost certainly forgot them, and an Ok full of zeros
    // would dress the mistake up as a measurement.
    NoTargets,
    // The languages were resolved against a configuration that names a different set from the one
    // handed here. Refused rather than counted, because the answer would be about a question nobody
    // asked and would look exactly like the answer to the one they did: resolving with a
    // configuration naming Rust and running with one naming Python counted Rust and said nothing.
    // Only the three fields resolution reads are compared, and case and order are folded, so
    // resolving once and then counting several directories is untouched.
    LanguagesFromAnotherConfig,
    // The declared targets could not be turned into places to walk: a path that names nothing, a
    // pattern that does not parse or matches nothing, or one place declared under two names. Found
    // at the run's entry, where the targets are resolved with the same configuration the walk obeys.
    InvalidTargets(crate::engine::targets::TargetError),
    // The pattern as the caller wrote it, not the anchored form the matcher builds from it
    InvalidExcludePattern(String),
    // The operating system refused every thread of one side. A refusal of some but not all is not
    // an error at all: fewer threads is the same answer arriving slower, so the run degrades and
    // carries on. Zero is different, because zero producers discover nothing and zero consumers
    // count nothing, and either would dress a non-answer up as an empty one.
    NoThreadsAvailable { side: &'static str, error: std::io::Error },
    // A worker died mid-run. It merges its share of the counting at the end, so whatever it had
    // done is lost with it, and a number known to be short is never returned as an answer. The
    // message of the panic travels here; its location has already reached the error output through
    // the panic hook, at the moment it happened.
    IncompleteRun { worker_panic: String }
}

#[derive(Debug,Default,Clone,Copy,PartialEq,Eq)]
pub struct FilesPresent {
    pub total_files: usize,
    pub relevant_files: usize,
    pub excluded_files: usize
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

impl Stats {
    // Every language added together, which is what the last row of a report holds. The keywords add
    // up with the rest now: 'classes' exists in several languages, so the question "how many in this
    // project" has an answer, where before the totals carried no keywords at all.
    pub fn total_of(languages: &HashMap<String, Stats>) -> Self {
        let mut total = Stats::default();
        for stats in languages.values() {
            total.add(stats);
        }
        total
    }
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

// The constructor exists for the same reason the one above does: the struct is non-exhaustive, so a
// crate outside this one cannot build it field by field, and the command line's own tests do.
impl UnreadableDirDetails {
    pub fn new(path: String, error_msg: String) -> Self {
        UnreadableDirDetails {
            path,
            error_msg
        }
    }
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

    // The truth table of the two zero-answers: an empty readable tree is an answer, an empty scan
    // that failed to open something is not, and finding files makes the question moot
    #[test]
    fn an_empty_scan_is_suspect_only_when_something_was_unreadable() {
        assert!(!result_with(0, &[]).nothing_could_be_read());
        assert!(result_with(0, &["D:/gone"]).nothing_could_be_read());
        assert!(!result_with(3, &["D:/gone"]).nothing_could_be_read());
        assert!(!result_with(3, &[]).nothing_could_be_read());
    }
}
