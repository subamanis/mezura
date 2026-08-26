// What one run produced, as opposed to the words it is described in, which are in 'domain.rs': a
// 'Language' exists before anything has been counted, a 'Stats' of it does not.
use std::collections::HashMap;

use crate::{CountingModel, Stats};
use crate::engine::config::{Target, Threads};
use crate::engine::modules::{ModuleId, Modules};

/// Everything one run of [`crate::run`] produced.
#[derive(Debug,Clone)]
pub struct RunResult {
    /// Across every module. A run where nothing was named has one module holding these same
    /// numbers.
    pub per_language: HashMap<String, Stats>,
    /// Every language added together.
    pub total: Stats,
    /// The sections other languages held inside container files, the container's language first
    /// and the section's language inside it.
    ///
    /// What is in here is already counted in `per_language` under the container, so it breaks
    /// those rows down and is never added to them. The `files` figure inside it is how many
    /// container files the section language appeared in.
    pub nested_languages: HashMap<String, HashMap<String, Stats>>,
    /// The same figures once per named part of the run. Always at least one, see [`ModuleResult`].
    pub modules: Vec<ModuleResult>,
    /// The files that could not be read or parsed, and why.
    pub faulty_files: Vec<FaultyFileDetails>,
    /// How many bundled files were left out. Counted among `files_present.relevant_files` and in
    /// none of the figures above, the way a faulty file is.
    pub minified_files: usize,
    /// The same, for the files whose head says a tool wrote them.
    pub generated_files: usize,
    /// How many files the scan saw, and how many of them belonged to a language it counts.
    pub files_present: FilesPresent,
    /// How long it took and on how many threads.
    pub performance: Performance,
    /// The places actually visited: the targets as given, resolved, with every pattern expanded to
    /// what it matched at that moment. A pattern's matches change while its text does not, which is
    /// what a log or a document needs in order to say that two runs measured the same thing.
    pub targets: Vec<Target>,
    /// Directories that could not be opened, so everything inside them is missing from every number
    /// above.
    pub unreadable_dirs: Vec<UnreadableDirDetails>
}

impl RunResult {
    /// The languages largest first by the chosen figure, ties broken by name.
    pub fn sort_languages_by(&self, criterion: SortCriterion, model: CountingModel) -> Vec<(&str, &Stats)> {
        sort_languages_by(&self.per_language, criterion, model)
    }

    /// Files belonging to a counted language were found and every one of them failed to be read.
    pub fn all_relevant_files_were_faulty(&self) -> bool {
        !self.faulty_files.is_empty() && self.faulty_files.len() == self.files_present.relevant_files
    }

    /// Such files were found and no row came out of them, by any mixture of failing to parse and
    /// being left out as minified or generated.
    pub fn nothing_of_interest_was_counted(&self) -> bool {
        self.files_present.relevant_files > 0 && self.total.files == 0
    }

    /// Nothing was found and a directory could not be opened, so this is "I could not look" rather
    /// than "there is no code here".
    ///
    /// An otherwise empty tree with one unreadable corner answers yes as well, on purpose: that
    /// corner may have held everything.
    pub fn nothing_could_be_read(&self) -> bool {
        self.files_present.relevant_files == 0 && !self.unreadable_dirs.is_empty()
    }

    /// Whether any part of the run was given a name of its own.
    pub fn has_modules(&self) -> bool {
        self.modules.iter().any(|x| x.name.is_some())
    }

    // The modules are built even here. One that found nothing was still asked for by name, and
    // leaving it out would make 'has_modules' say no and take the whole block out of the document
    // exactly when the scan came back empty.
    pub(crate) fn of_nothing(files_present: FilesPresent, performance: Performance, modules: &Modules,
            targets: Vec<Target>, unreadable_dirs: Vec<UnreadableDirDetails>) -> Self {
        RunResult {
            per_language: HashMap::new(),
            total: Stats::default(),
            nested_languages: HashMap::new(),
            modules: (0..modules.count()).map(|id| ModuleResult {
                name: modules.name_of(id as ModuleId).map(str::to_owned),
                per_language: HashMap::new(),
                nested_languages: HashMap::new(),
                files: HashMap::new(),
                total: Stats::default()
            }).collect(),
            faulty_files: Vec::new(),
            minified_files: 0,
            generated_files: 0,
            files_present,
            performance,
            targets,
            unreadable_dirs
        }
    }
}

/// One named part of a run, and its own figures.
#[derive(Debug,Clone)]
pub struct ModuleResult {
    /// `None` for whatever the named parts left over, which is also the single part of a run where
    /// nothing was named at all.
    pub name: Option<String>,
    /// This part's languages.
    pub per_language: HashMap<String, Stats>,
    /// The same breakdown [`RunResult::nested_languages`] holds, for this part's files alone.
    pub nested_languages: HashMap<String, HashMap<String, Stats>>,
    /// One entry per file, keyed by language. Empty unless [`crate::EngineConfig::collect_files`]
    /// asked for it.
    pub files: HashMap<String, Vec<FileEntry>>,
    /// This part's languages added together.
    pub total: Stats
}

impl ModuleResult {
    /// The languages largest first by the chosen figure, ties broken by name.
    pub fn sort_languages_by(&self, criterion: SortCriterion, model: CountingModel) -> Vec<(&str, &Stats)> {
        sort_languages_by(&self.per_language, criterion, model)
    }
}

/// One counted file.
#[derive(Debug,Clone)]
pub struct FileEntry {
    /// Absolute, with forward slashes whichever way the platform writes them.
    pub path: String,
    /// Its figures, the sections of other languages inside it included, the way a container
    /// language's row holds all of its files' lines.
    pub stats: Stats,
    /// What those sections weigh on their own.
    pub nested_languages: HashMap<String, Stats>
}

/// What the run cost.
#[derive(Debug,Clone)]
pub struct Performance {
    /// From the moment the threads started to the moment the last file was counted.
    pub duration_millis: u128,
    /// What the run actually got, not what was asked for: the operating system may grant fewer and
    /// the run carries on with those.
    pub threads: Threads
}

/// How much of what the scan saw it had reason to count.
#[derive(Debug,Default,Clone,Copy,PartialEq,Eq)]
pub struct FilesPresent {
    /// Every file the scan reached, whatever it was. Links are not followed and are not in here.
    pub total_files: usize,
    /// Those belonging to a language the run counts, which are the ones it went on to read.
    pub relevant_files: usize,
    /// Those a language claims that an exclude pattern or an ignore file then took out. A file no
    /// language claims is not in here, since it was never identified in the first place.
    pub excluded_files: usize
}

/// Which figure decides the order of a report's rows.
#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub enum SortCriterion {
    /// How many files the language has.
    Files,
    /// Every line of them.
    #[default]
    Lines,
    /// The code column under the model being shown.
    Code,
    /// The comments column under it.
    Comments,
    /// The third column, and only under [`CountingModel::Content`], which is the model that calls
    /// it `extra`.
    Extra,
    /// The third column, and only under [`CountingModel::Region`], which is the model that calls
    /// it `blanks`.
    Blanks,
    /// Total size on disk.
    Size,
    /// Alphabetical, ignoring case.
    Name
}

impl SortCriterion {
    /// Reads the same word a person types and a configuration file stores, trimmed and in any case.
    pub fn parse(value: &str) -> Option<SortCriterion> {
        match value.trim().to_lowercase().as_str() {
            "files" => Some(Self::Files),
            "lines" => Some(Self::Lines),
            "code" => Some(Self::Code),
            "comments" => Some(Self::Comments),
            "extra" => Some(Self::Extra),
            "blanks" => Some(Self::Blanks),
            "size" => Some(Self::Size),
            "name" => Some(Self::Name),
            _ => None
        }
    }

    /// The spelling [`SortCriterion::parse`] reads back.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Lines => "lines",
            Self::Code => "code",
            Self::Comments => "comments",
            Self::Extra => "extra",
            Self::Blanks => "blanks",
            Self::Size => "size",
            Self::Name => "name"
        }
    }

    /// The figure this criterion orders by. [`SortCriterion::Name`] has none and every row answers
    /// 0, leaving the order to the caller's tiebreak.
    pub fn get_value_of(&self, stats: &Stats, model: CountingModel) -> usize {
        match self {
            Self::Files => stats.files,
            Self::Size => stats.bytes,
            Self::Lines => stats.lines,
            Self::Code => stats.calculate_code_lines(model),
            Self::Comments => stats.calculate_comment_lines(model),
            Self::Extra | Self::Blanks => stats.calculate_extra_lines(model),
            Self::Name => 0
        }
    }
}

/// A file that could not be read or parsed. Its lines are in no total, but it is counted among the
/// files that were seen.
#[derive(Debug,Clone)]
#[non_exhaustive]
pub struct FaultyFileDetails {
    /// Absolute, with forward slashes.
    pub path: String,
    /// What went wrong, in words.
    pub error_msg: String,
    /// Its size on disk in bytes, which is in no total either.
    pub size: u64
}

impl FaultyFileDetails {
    /// The type is non-exhaustive, so this is how a crate outside this one builds it.
    pub fn new(path: String, error_msg: String, size: u64) -> Self {
        FaultyFileDetails {
            path,
            error_msg,
            size
        }
    }
}

/// A directory that could not be opened, so nothing inside it reached any figure.
#[derive(Debug,Clone)]
#[non_exhaustive]
pub struct UnreadableDirDetails {
    /// Absolute, with forward slashes.
    pub path: String,
    /// What went wrong, in words.
    pub error_msg: String
}

impl UnreadableDirDetails {
    /// The type is non-exhaustive, so this is how a crate outside this one builds it.
    pub fn new(path: String, error_msg: String) -> Self {
        UnreadableDirDetails {
            path,
            error_msg
        }
    }
}

/// A run that produced nothing at all, as opposed to a run that produced zeros.
///
/// Finding no code is a result, and every file failing to parse is a result with the reasons
/// attached, so neither of those is here.
#[derive(Debug)]
#[non_exhaustive]
pub enum RunError {
    /// The configuration names no place to look. That is a malformed question rather than an empty
    /// answer, and an `Ok` full of zeros would dress it up as a measurement.
    NoTargets,
    /// The languages were resolved against a configuration that selects a different set from the
    /// one handed to the run, so the counts would not be the ones the settings describe.
    ///
    /// Only the three settings that resolution reads are compared, with case and order folded, so
    /// resolving once and counting several directories with it is untouched.
    LanguagesFromAnotherConfig,
    /// The targets could not be turned into places to visit: a path that names nothing, a pattern
    /// that does not parse or matches nothing, or one place given under two names.
    InvalidTargets(crate::engine::targets::TargetError),
    /// An exclude pattern does not parse, quoted as the caller wrote it.
    InvalidExcludePattern(String),
    /// The operating system refused every thread of one side. Refusing some but not all is not an
    /// error: fewer threads is the same answer arriving slower.
    NoThreadsAvailable {
        /// `producer` for the threads that scan directories, `consumer` for the ones that count.
        side: &'static str,
        /// What the operating system said.
        error: std::io::Error
    },
    /// A worker died mid-run. Each of them merges its share of the counting at the end, so whatever
    /// it had done is lost with it and the figures left behind are short.
    IncompleteRun {
        /// The panic's message. Its location already reached the error output through the panic
        /// hook, at the moment it happened.
        worker_panic: String
    }
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
// is what the last row of a report holds.
impl Stats {
    /// Every language added together, which is what the last row of a report holds.
    pub fn total_of(languages: &HashMap<String, Stats>) -> Self {
        let mut total = Stats::default();
        for stats in languages.values() {
            total.add(stats);
        }
        total
    }
}

fn sort_languages_by(per_language: &HashMap<String, Stats>, criterion: SortCriterion,
    model: CountingModel) -> Vec<(&str, &Stats)>
{
    let mut rows = per_language.iter().map(|(name, stats)| (name.as_str(), stats)).collect::<Vec<_>>();
    if criterion == SortCriterion::Name {
        rows.sort_by_key(|(name, _)| name.to_lowercase());
    } else {
        // The name breaks every tie, with case folded, so two languages of equal size cannot swap
        // places between two runs of the same command because a map iterated differently.
        rows.sort_by(|a, b| criterion.get_value_of(b.1, model).cmp(&criterion.get_value_of(a.1, model))
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
        RunResult::of_nothing(
                FilesPresent { total_files: relevant, relevant_files: relevant, excluded_files: 0 },
                Performance { duration_millis: 0, threads: Threads::new(1, 1) },
                &Modules::of(&[]), Vec::new(), unreadable)
    }

    #[test]
    fn an_empty_scan_is_suspect_only_when_something_was_unreadable() {
        assert!(!result_with(0, &[]).nothing_could_be_read());
        assert!(result_with(0, &["D:/gone"]).nothing_could_be_read());
        assert!(!result_with(3, &["D:/gone"]).nothing_could_be_read());
        assert!(!result_with(3, &[]).nothing_could_be_read());
    }
}
