use std::collections::HashMap;

use mezura_core::{EngineConfig, Language, Languages, ModuleResult, RunResult, Stats, UNNAMED_MODULE_NAME, render};
use mezura_core::language_file::PriorityRules;

use super::config_manager::{Configuration, Layout, SortCriterion};
use super::config_manager::{BRACES_AS_CODE, EXCLUDE, EXCLUDE_LANGUAGES, FORCE_LANGUAGE, LANGUAGES,
        NO_GITIGNORE, SEARCH_IN_DOTTED};
use super::json_reader::{DocumentError, DocumentWarning, Scope};
use super::sources::RevisionSide;

// The half of a document's warnings that says the numbers themselves may be wrong, as the document
// spells it
const COUNTS_AFFECTED : &str = "counts";

// The one compared setting that is not a config key of its own: it is the 'keywords' value of
// '--hide', and the log's comparison filters it out because the log holds no keyword counts
pub const HIDE_KEYWORDS : &str = "hide keywords";

// What the column of the run being made right now is called. Not a date, because it is the one
// reading whose date says nothing: it is this one.
const THIS_RUN_NAME : &str = "this run";

pub enum Source {
    Run,
    Document { path: String },
    // Both halves, because neither derives from the other: the hash is what the comparison really
    // measured, 'asked_for' is what makes it readable six months later, 'v2.0.1' over '030e6e72a1'.
    GitRevision { commit: String, asked_for: String }
}

pub struct Reading {
    pub source: Source,
    // In the document's own format, whichever source filled it
    pub taken: String,
    pub version: String,
    pub scope: Scope,
    // Empty for a reading counted by this very run, whose warnings were printed as they happened
    pub warnings: Vec<DocumentWarning>,
    // Counts, not lists, because a document details the failures only when '--show-faulty-files'
    // asked it to while its scan block counts them either way: the lists in 'result' can read
    // empty for a side whose counts are short.
    pub faulty_files_count: usize,
    pub unreadable_dirs_count: usize,
    pub result: RunResult
}

impl Reading {
    // Everything but the counts and the commit's own two facts is this run's own
    pub fn of_git_revision(asked_for: &str, commit: String, taken: String, result: RunResult,
            engine: &mezura_core::EngineConfig) -> Self {
        Reading {
            source: Source::GitRevision { commit, asked_for: asked_for.to_owned() },
            taken,
            version: super::config_manager::VERSION_ID.trim_start_matches('v').to_owned(),
            scope: scope_of(engine),
            warnings: Vec::new(),
            faulty_files_count: result.faulty_files.len(),
            unreadable_dirs_count: result.unreadable_dirs.len(),
            result
        }
    }

    // A copy, because the result is still being presented around the comparison
    pub fn of_this_run(result: &RunResult, taken: &chrono::DateTime<chrono::Local>,
            engine: &mezura_core::EngineConfig) -> Self {
        Reading {
            source: Source::Run,
            taken: taken.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
            version: super::config_manager::VERSION_ID.trim_start_matches('v').to_owned(),
            scope: scope_of(engine),
            warnings: Vec::new(),
            faulty_files_count: result.faulty_files.len(),
            unreadable_dirs_count: result.unreadable_dirs.len(),
            result: result.clone()
        }
    }

    pub fn determine_display_name(&self) -> String {
        match &self.source {
            Source::Run => THIS_RUN_NAME.to_owned(),
            Source::Document { path } => std::path::Path::new(path).file_name()
                    .map_or_else(|| path.clone(), |x| x.to_string_lossy().into_owned()),
            Source::GitRevision { asked_for, .. } => asked_for.clone()
        }
    }
}

pub struct Comparison {
    pub baseline: Reading,
    pub subject: Reading,
    pub notes: Vec<Note>
}

impl Comparison {
    // 'notes_so_far' is the notes the acquisition already gathered, and they read first
    pub fn of(baseline: Reading, subject: Reading, config: &Configuration, notes_so_far: Vec<Note>) -> Self {
        let unpaired = pair_modules(&baseline.result, &subject.result).is_none();
        let notes = determine_comparison_notes(&baseline, &subject, config, notes_so_far, unpaired);

        Comparison { baseline, subject, notes }
    }

    // Computed rather than stored, because the pairs borrow the readings this struct owns
    pub fn module_pairs(&self) -> Option<Vec<ModulePair<'_>>> {
        pair_modules(&self.baseline.result, &self.subject.result)
    }
}

// The two phases exist because the order is load-bearing: a document's settings reach the language
// resolution and the counting that follow, and a baseline that is not one must cost no scan.
pub enum DiffRequest {
    BetweenTwoReadings(BothSidesNamed),
    // The subject is this very run, which arrives after the scan
    AgainstThisRun(BaselineOnly)
}

impl DiffRequest {
    pub fn of(config: &mut Configuration, available: &[Language]) -> Result<Option<Self>, String> {
        let Some(operand) = config.view.diff_against.clone() else {
            return Ok(None);
        };
        let (baseline_name, subject_name) = split_operand(&operand)?;
        let baseline = DiffSide::from_name(baseline_name)?;
        // None when only one side was named: the subject is then this run, which has no name and
        // has not been counted yet
        let subject = subject_name.map(DiffSide::from_name).transpose()?;

        // With two documents there is nothing being counted for the settings to reach, and with none
        // there is nothing to take them from; both sides then run as declared, and what differs is
        // reported above the table
        let settings_source = match (&baseline, &subject) {
            (DiffSide::Document(x), None) => Some(x),
            (DiffSide::Document(x), Some(other)) | (other, Some(DiffSide::Document(x)))
                    if other.needs_counting() => Some(x),
            _ => None
        };
        let notes_so_far = settings_source.map(|document| adopt_settings_from(document, config))
                .unwrap_or_default().into_iter().collect();

        let languages = available.to_vec();
        Ok(Some(match subject {
            Some(subject) => DiffRequest::BetweenTwoReadings(
                    BothSidesNamed { baseline, subject, notes_so_far, languages }),
            None => DiffRequest::AgainstThisRun(BaselineOnly { baseline, notes_so_far, languages })
        }))
    }
}

// The languages travel with the request because 'Languages::resolve' and 'run' both consume what
// they are handed, so every side that is counted needs a list of its own. Held here rather than
// juggled by the caller, which had to know which kinds of source need counting in order to know
// whether to copy at all.
pub struct BothSidesNamed {
    baseline: DiffSide,
    subject: DiffSide,
    notes_so_far: Vec<Note>,
    languages: Vec<Language>
}

impl BothSidesNamed {
    // The language complaints are reported here, this being the one form where no run of this
    // program's own will report them
    pub fn into_comparison(self, config: &Configuration,
            extension_priority: &PriorityRules) -> Result<Comparison, String>
    {
        let (_, reported) = Languages::resolve(&config.engine, self.languages.clone(), extension_priority);
        super::warning_collector::report_language_resolution_warnings(reported);

        let [baseline, subject] = <[PreparedSide; 2]>::try_from(
                prepare_sides(vec![self.baseline, self.subject], &config.engine)?)
                .ok().expect("two sides in, two sides out");

        let (baseline, notes) = baseline.into_reading(config, self.languages.clone(), extension_priority)?;
        let mut notes_so_far = self.notes_so_far;
        notes_so_far.extend(notes);
        let (subject, notes) = subject.into_reading(config, self.languages, extension_priority)?;
        notes_so_far.extend(notes);

        Ok(Comparison::of(baseline, subject, config, notes_so_far))
    }
}

pub struct BaselineOnly {
    baseline: DiffSide,
    notes_so_far: Vec<Note>,
    languages: Vec<Language>
}

impl BaselineOnly {
    pub fn count_baseline(self, config: &Configuration,
            extension_priority: &PriorityRules) -> Result<CountedBaseline, String>
    {
        let [baseline] = <[PreparedSide; 1]>::try_from(prepare_sides(vec![self.baseline], &config.engine)?)
                .ok().expect("one side in, one side out");
        let (baseline, notes) = baseline.into_reading(config, self.languages, extension_priority)?;
        let mut notes_so_far = self.notes_so_far;
        notes_so_far.extend(notes);

        Ok(CountedBaseline { baseline, notes_so_far })
    }
}

pub struct CountedBaseline {
    baseline: Reading,
    notes_so_far: Vec<Note>
}

impl CountedBaseline {
    pub fn with_subject(self, subject: Reading, config: &Configuration) -> Comparison {
        Comparison::of(self.baseline, subject, config, self.notes_so_far)
    }
}

// Boxed for the size difference between the variants
enum DiffSide {
    Document(Box<Reading>),
    GitRevision(String)
}

impl DiffSide {
    // Which of the two a name is gets decided by what exists on disk: a file is read here and now as
    // a document, and anything else goes to git exactly as it was written.
    fn from_name(name: &str) -> Result<Self, String> {
        match super::sources::read_document(name) {
            Some(document) => document.map(|x| DiffSide::Document(Box::new(x))),
            None => Ok(DiffSide::GitRevision(name.to_owned()))
        }
    }

    // Asked instead of naming the variants that need it, so that a new kind of side joins the
    // decision by existing rather than by being remembered
    fn needs_counting(&self) -> bool {
        !matches!(self, DiffSide::Document(_))
    }

    fn find_revision_name(&self) -> Option<&str> {
        match self {
            DiffSide::GitRevision(name) => Some(name),
            DiffSide::Document(_) => None
        }
    }
}

// The same sides after preparation, as their own type so that an unresolved side cannot reach the
// counting: one enum with all three shapes would give 'into_reading' an arm for the unprepared one
// that could only panic or quietly resolve on the spot. A revision owns its resolution and the
// write started ahead; an error path just drops the unread side, which cleans up after itself.
enum PreparedSide {
    Document(Box<Reading>),
    Revision(RevisionSide)
}

// Every side that is a revision resolves before anything is written out, which is where two
// spellings of one commit are refused and where a typo in the second side fails at once
fn prepare_sides(sides: Vec<DiffSide>, engine: &EngineConfig) -> Result<Vec<PreparedSide>, String> {
    let names = sides.iter().filter_map(DiffSide::find_revision_name).collect::<Vec<_>>();
    let resolved = super::sources::prepare_revisions(&names, engine).map_err(|x| x.to_string())?;
    let mut acquiring = super::sources::start_acquiring_revisions(resolved).into_iter();

    Ok(sides.into_iter().map(|side| match side {
        DiffSide::Document(reading) => PreparedSide::Document(reading),
        DiffSide::GitRevision(_) => PreparedSide::Revision(acquiring.next()
                .expect("a resolution for every revision side"))
    }).collect())
}

impl PreparedSide {
    fn into_reading(self, config: &Configuration, languages: Vec<Language>,
            extension_priority: &PriorityRules) -> Result<(Reading, Vec<Note>), String>
    {
        match self {
            PreparedSide::Document(reading) => Ok((*reading, Vec::new())),
            PreparedSide::Revision(side) => super::sources::count_git_revision(side, config, languages,
                    extension_priority).map_err(|x| x.to_string())
        }
    }
}

fn adopt_settings_from(document: &Reading, config: &mut Configuration) -> Option<Note> {
    let settings = resolve_settings(&document.scope, config);
    (!settings.is_empty()).then(|| Note::SettingsAdopted { from: document.determine_display_name(), settings })
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

pub struct LanguageStatsChange {
    pub name: String,
    pub baseline: Stats,
    pub subject: Stats
}

// In the order the later reading declared them, which is the order its own report would use
pub struct ModulePair<'a> {
    pub name: Option<&'a str>,
    pub before: &'a ModuleResult,
    pub now: &'a ModuleResult
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

// Rendered by the screen and the document each in its own shape, from this one list
#[derive(Debug,PartialEq)]
pub enum Note {
    SettingsAdopted { from: String, settings: Vec<&'static str> },
    SettingsDiffer { baseline: String, subject: String, settings: Vec<&'static str> },
    VersionsDiffer { baseline: String, baseline_version: String, subject: String, subject_version: String },
    CountsInDoubt { about: String, doubts: Vec<String> },
    // A side whose scan found nothing at all, which the table can only show as zeros
    NothingCounted { about: String },
    // None is a side that declared no modules at all
    ModulesDiffer { baseline: String, subject: String, baseline_modules: Option<String>, subject_modules: Option<String> },
    LayoutFallback { layout: &'static str },
    NoGitignoreInCheckout { git_revision: String },
    MissingInRevision { git_revision: String, targets: Vec<String> }
}

// Splits '--diff a.json..b.json' into the two readings it names, and answers None for the second
// when only one was given, which is the form whose second reading is this run.
//
// The trap is that '..' is a separator here and a directory in every filesystem, so '--diff
// ../old.json' must not come apart into an empty name and '/old.json'. The rule is git's own: what
// was written is taken whole if it names something that exists, and only split if it does not.
fn split_operand(value: &str) -> Result<(&str, Option<&str>), String> {
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
            faulty_files_count: document.faulty_files_count, unreadable_dirs_count: document.unreadable_dirs_count,
            result: document.result })
}

pub fn change_of(before: usize, now: usize) -> Change {
    match (before, now) {
        (0, 0) => Change::Percent(0.0),
        (0, _) => Change::Appeared,
        (_, 0) => Change::Gone,
        (before, now) => Change::Percent(render::calculate_relative_change(before, now))
    }
}

// Every language of either reading, sorted the way the report would have been, and how many
// languages that union holds: rows and union come from one place, so 'union - rows.len()' is what
// '--top' hid and can never underflow. 'top' is the screen's cut and is not applied when a document
// is being written, which holds every row the same way the run's own document does.
pub fn create_comparison_rows(baseline: &HashMap<String, Stats>, subject: &HashMap<String, Stats>,
        sort_by: SortCriterion, top: Option<usize>) -> (Vec<LanguageStatsChange>, usize)
{
    // Held at what the subject has, so one that disappeared sorts to the bottom where a zero belongs
    let mut merged = subject.clone();
    for name in baseline.keys() {
        merged.entry(name.clone()).or_default();
    }

    let names = super::result_printer::get_sorted_language_names(&merged, sort_by);
    let shown = top.map_or(names.len(), |x| x.min(names.len()));

    (names[..shown].iter().map(|name| LanguageStatsChange {
        baseline: baseline.get(name).cloned().unwrap_or_default(),
        subject: subject.get(name).cloned().unwrap_or_default(),
        name: name.clone()
    }).collect(), merged.len())
}

// The modules of the two readings matched up by name, and None when there is nothing to show: the
// sets differ, or nothing was ever named and the only pair is the one module holding everything.
//
// Nothing short of the same set can be shown. A module only one of them has would be compared
// against nothing, and every language in it would read as written from scratch or deleted whole.
// This repository is that case: one tree became 'mezura-core' and 'mezura', and a comparison across
// that commit would report both as infinite growth and the leftovers as infinite loss, for files
// that only moved.
pub fn pair_modules<'a>(baseline: &'a RunResult, subject: &'a RunResult) -> Option<Vec<ModulePair<'a>>> {
    if baseline.modules.len() != subject.modules.len() || !subject.has_modules() {
        return None;
    }

    // A name is claimed once, so finding every one of the second reading's in the first is the sets
    // being equal, and collecting into an Option is the one that is missing answering for all of them
    subject.modules.iter().map(|now| baseline.modules.iter().find(|x| x.name == now.name)
            .map(|before| ModulePair { name: now.name.as_deref(), before, now })).collect()
}

// The modules of one reading as the message names them, each in its own quotes, and None when
// nothing was declared, which the sentence says in words. The leftovers are in the list: their
// being on one side and not the other is one of the ways the two sets differ, and a message that
// left them out would name two lists that look identical.
pub fn format_module_names(result: &RunResult) -> Option<String> {
    result.has_modules().then(|| result.modules.iter()
            .map(|x| format!("'{}'", x.name.as_deref().unwrap_or(UNNAMED_MODULE_NAME)))
            .collect::<Vec<_>>().join(", "))
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
        gitignore: !engine.no_gitignore,
        keywords_counted: engine.count_keywords
    }
}

// A difference here is not a change in the code, and every one of these can move a count on its own.
pub fn find_settings_that_differ(baseline: &Scope, subject: &Scope) -> Vec<&'static str> {
    let same = |a: &[String], b: &[String]| {
        let (mut a, mut b) = (a.to_vec(), b.to_vec());
        a.sort();
        b.sort();
        a == b
    };

    let mut differ = Vec::new();
    if !same(&baseline.exclude, &subject.exclude) {differ.push(EXCLUDE)}
    if !same(&baseline.languages, &subject.languages) {differ.push(LANGUAGES)}
    if !same(&baseline.excluded_languages, &subject.excluded_languages) {differ.push(EXCLUDE_LANGUAGES)}
    if baseline.forced_languages != subject.forced_languages {differ.push(FORCE_LANGUAGE)}
    if baseline.braces_as_code != subject.braces_as_code {differ.push(BRACES_AS_CODE)}
    if baseline.search_in_dotted != subject.search_in_dotted {differ.push(SEARCH_IN_DOTTED)}
    if baseline.gitignore != subject.gitignore {differ.push(NO_GITIGNORE)}
    if baseline.keywords_counted != subject.keywords_counted {differ.push(HIDE_KEYWORDS)}

    differ
}

// Takes the document's settings for everything this run's own command line did not explicitly set,
// so that the two readings measure the same thing
pub fn resolve_settings(document: &Scope, config: &mut super::config_manager::Configuration) -> Vec<&'static str> {
    let different = |a: &[String], b: &[String]| {
        let (mut a, mut b) = (a.to_vec(), b.to_vec());
        a.sort();
        b.sort();
        a != b
    };

    let typed = config.typed_explicitly;
    let mut adopted = Vec::new();
    if !typed.exclude && different(&document.exclude, &config.engine.exclude_dirs) {
        config.engine.exclude_dirs = document.exclude.clone();
        adopted.push(EXCLUDE);
    }
    if !typed.languages && different(&document.languages, &config.engine.languages_of_interest) {
        config.engine.languages_of_interest = document.languages.clone();
        adopted.push(LANGUAGES);
    }
    if !typed.excluded_languages && different(&document.excluded_languages, &config.engine.excluded_languages) {
        config.engine.excluded_languages = document.excluded_languages.clone();
        adopted.push(EXCLUDE_LANGUAGES);
    }
    if !typed.forced_languages && document.forced_languages != config.engine.forced_languages {
        config.engine.forced_languages = document.forced_languages.clone();
        adopted.push(FORCE_LANGUAGE);
    }
    if !typed.braces_as_code && document.braces_as_code != config.engine.braces_as_code {
        config.engine.braces_as_code = document.braces_as_code;
        adopted.push(BRACES_AS_CODE);
    }
    if !typed.search_in_dotted && document.search_in_dotted != config.engine.should_search_in_dotted {
        config.engine.should_search_in_dotted = document.search_in_dotted;
        adopted.push(SEARCH_IN_DOTTED);
    }
    // The document records whether the file was obeyed, the flag says the opposite
    if !typed.no_gitignore && document.gitignore == config.engine.no_gitignore {
        config.engine.no_gitignore = !document.gitignore;
        adopted.push(NO_GITIGNORE);
    }
    // Both halves of the one flag, or the counting and the printing would disagree: a standing
    // '--hide keywords' from a configuration yields here like any other supplied value
    if !typed.hide_keywords && document.keywords_counted != config.engine.count_keywords {
        config.engine.count_keywords = document.keywords_counted;
        config.view.hidden.keywords = !document.keywords_counted;
        adopted.push(HIDE_KEYWORDS);
    }

    adopted
}

// In the order they are read above the table
fn determine_comparison_notes(baseline: &Reading, subject: &Reading, config: &Configuration,
        notes_so_far: Vec<Note>, modules_unpaired: bool) -> Vec<Note>
{
    let mut notes = notes_so_far;

    let differing = find_settings_that_differ(&baseline.scope, &subject.scope);
    if !differing.is_empty() {
        notes.push(Note::SettingsDiffer { baseline: baseline.determine_display_name(),
                subject: subject.determine_display_name(), settings: differing });
    }
    // A build whose language files were corrected counts the same tree differently, and the
    // Changelog is full of exactly that
    if baseline.version != subject.version {
        notes.push(Note::VersionsDiffer { baseline: baseline.determine_display_name(),
                baseline_version: baseline.version.clone(), subject: subject.determine_display_name(),
                subject_version: subject.version.clone() });
    }
    // How each side's own scan went, said here because no other voice says it: the run's side is
    // announced by 'present' as it happens, but a document or a revision counted on the spot is
    // otherwise silent, and a side that failed to parse half its files reads as code that shrank.
    for reading in [baseline, subject] {
        let this_run = matches!(reading.source, Source::Run);
        if !this_run && reading.result.files_present.relevant_files == 0 {
            notes.push(Note::NothingCounted { about: reading.determine_display_name() });
        }

        let mut doubts = Vec::new();
        if !this_run && reading.faulty_files_count > 0 {
            doubts.push(format!("{} of its files could not be parsed, so its counts are short by that",
                    reading.faulty_files_count));
        }
        if !this_run && reading.unreadable_dirs_count > 0 {
            let (n, dirs) = (reading.unreadable_dirs_count,
                    if reading.unreadable_dirs_count == 1 {"directory"} else {"directories"});
            doubts.push(format!("{n} {dirs} could not be read, so nothing inside was counted"));
        }
        // What the run that wrote a reading said about its own counts: an unreadable language file
        // leaves a whole language at zero, which this run would report as a language that appeared
        // out of nowhere
        doubts.extend(reading.warnings.iter().filter(|x| x.affects == COUNTS_AFFECTED)
                .map(|x| format!("{} ({})", x.message, x.code)));
        if !doubts.is_empty() {
            notes.push(Note::CountsInDoubt { about: reading.determine_display_name(), doubts });
        }
    }
    if modules_unpaired && (baseline.result.has_modules() || subject.result.has_modules()) {
        notes.push(Note::ModulesDiffer { baseline: baseline.determine_display_name(),
                subject: subject.determine_display_name(), baseline_modules: format_module_names(&baseline.result),
                subject_modules: format_module_names(&subject.result) });
    }
    // The other two layouts have nothing to show for a comparison, and are told so rather than
    // being ignored, the way a matrix with nothing to cross is
    if matches!(config.view.layout, Layout::List | Layout::Matrix) {
        notes.push(Note::LayoutFallback { layout: config.view.layout.name() });
    }

    notes
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
        let baseline = hashmap!["Rust".to_owned() => stats(100, 70, 2), "Java".to_owned() => stats(40, 30, 1)];
        let subject = hashmap!["Rust".to_owned() => stats(150, 100, 3), "Go".to_owned() => stats(60, 50, 1)];

        let (rows, union) = create_comparison_rows(&baseline, &subject, SortCriterion::Lines, None);
        assert_eq!(3, union);
        assert_eq!(vec!["Rust".to_owned(), "Go".to_owned(), "Java".to_owned()],
                rows.iter().map(|x| x.name.clone()).collect::<Vec<_>>());
        // the one that is gone sorts last, holding the zero it is now, and keeps every figure it had
        assert_eq!(40, rows[2].baseline.lines);
        assert_eq!(30, rows[2].baseline.code_lines);
        assert_eq!(0, rows[2].subject.lines);
        // and the one that appeared has a whole empty reading behind it rather than a missing one
        assert_eq!(0, rows[1].baseline.lines);
        assert_eq!(60, rows[1].subject.lines);

        // '--top' cuts these rows the way it cuts the report, and never the union, so what it hid
        // is always the difference of the two. A document asks for no cut.
        let (cut, union) = create_comparison_rows(&baseline, &subject, SortCriterion::Lines, Some(2));
        assert_eq!((2, 3), (cut.len(), union));
        assert_eq!(3, create_comparison_rows(&baseline, &subject, SortCriterion::Lines, None).0.len());

        // and '--sort' orders them, as it does everywhere else
        assert_eq!(vec!["Go".to_owned(), "Java".to_owned(), "Rust".to_owned()],
                create_comparison_rows(&baseline, &subject, SortCriterion::Name, None).0.iter()
                        .map(|x| x.name.clone()).collect::<Vec<_>>());
    }

    // A module can only be compared against the same module, so the whole block is shown or none of
    // it is. The interesting cases are all data, so they are all here.
    #[test]
    fn the_modules_are_paired_by_name_and_a_set_that_is_not_the_same_is_not_paired_at_all() {
        let module = |name: Option<&str>, lines: usize| {
            let per_language = hashmap!["Rust".to_owned() => stats(lines, lines, 1)];
            ModuleResult {name: name.map(str::to_owned), total: Stats::total_of(&per_language), per_language,
                    nested_languages: Default::default()}
        };
        let result = |modules: Vec<ModuleResult>| {
            let per_language = hashmap!["Rust".to_owned() => stats(100, 70, 2)];
            mezura_core::RunResult {
                total: Stats::total_of(&per_language), per_language, modules, nested_languages: Default::default(),
                faulty_files: Vec::new(), targets: Vec::new(), unreadable_dirs: Vec::new(),
                files_present: mezura_core::FilesPresent {total_files: 2, relevant_files: 2, excluded_files: 0},
                performance: mezura_core::Performance {duration_millis: 0, threads: mezura_core::Threads::new(1, 1)}
            }
        };

        // The order they were declared in is the reader's choice and not a difference, so the pairs
        // follow the second reading and find the first's wherever it put them
        let before = result(vec![module(Some("cli"), 40), module(Some("core"), 100), module(None, 7)]);
        let now = result(vec![module(Some("core"), 120), module(Some("cli"), 44), module(None, 9)]);
        let pairs = pair_modules(&before, &now).unwrap();
        assert_eq!(vec![Some("core"), Some("cli"), None], pairs.iter().map(|x| x.name).collect::<Vec<_>>());
        assert_eq!(100, pairs[0].before.total.lines);
        assert_eq!(120, pairs[0].now.total.lines);
        assert_eq!(7, pairs[2].before.total.lines);

        // One name that is not on both sides takes the whole block with it, whichever side has it,
        // and so does the leftover, which is a member like any other
        assert!(pair_modules(&before, &result(vec![module(Some("core"), 120), module(Some("web"), 44),
                module(None, 9)])).is_none());
        assert!(pair_modules(&before, &result(vec![module(Some("core"), 120), module(Some("cli"), 44)])).is_none());
        assert!(pair_modules(&result(vec![module(None, 100)]), &now).is_none());

        // A run that named nothing has the one module holding everything, and a pair of those has
        // nothing to show: both callers asked exactly this and answered it identically, so the
        // question moved in here
        let plain = result(vec![module(None, 100)]);
        assert!(pair_modules(&plain, &plain).is_none());
        assert!(!plain.has_modules());

        // and the sentence that names them says which they were, the leftovers included, while a
        // run that declared none answers with the absence itself rather than a word for it
        assert_eq!(Some("'core', 'cli', '(unnamed)'".to_owned()), format_module_names(&now));
        assert_eq!(None, format_module_names(&plain));
    }

    // The one list both surfaces render, so what it holds and in which order is the whole of what
    // either can show. Every kind at once, so the order is pinned where it is decided.
    #[test]
    fn the_notes_carry_every_fact_the_reader_is_owed_in_reading_order() {
        let reading = |name: &str, version: &str, modules: Vec<ModuleResult>| {
            let per_language = hashmap!["Rust".to_owned() => stats(100, 70, 2)];
            Reading {
                source: Source::Document { path: name.to_owned() },
                taken: "2026-08-07T10:00:00+03:00".to_owned(),
                version: version.to_owned(),
                scope: scope_of(&mezura_core::EngineConfig::default()),
                warnings: Vec::new(),
                faulty_files_count: 0,
                unreadable_dirs_count: 0,
                result: RunResult {
                    total: Stats::total_of(&per_language), per_language, modules, nested_languages: Default::default(),
                    faulty_files: Vec::new(), targets: Vec::new(), unreadable_dirs: Vec::new(),
                    files_present: mezura_core::FilesPresent {total_files: 2, relevant_files: 2, excluded_files: 0},
                    performance: mezura_core::Performance {duration_millis: 0, threads: mezura_core::Threads::new(1, 1)}
                }
            }
        };
        let module = |name: &str| {
            let per_language = hashmap!["Rust".to_owned() => stats(50, 40, 1)];
            ModuleResult { name: Some(name.to_owned()), total: Stats::total_of(&per_language), per_language,
                    nested_languages: Default::default() }
        };

        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.view.layout = crate::config_manager::Layout::List;
        let adopted = Note::SettingsAdopted { from: "old.json".to_owned(), settings: vec!["exclude"] };

        let mut baseline = reading("old.json", "2.9.0", vec![module("api")]);
        baseline.scope.braces_as_code = true;
        // The scan facts come first among the doubts, then what that run had said itself
        baseline.faulty_files_count = 2;
        baseline.unreadable_dirs_count = 1;
        baseline.warnings.push(DocumentWarning { code: "language-file-unreadable".to_owned(),
                affects: "counts".to_owned(), message: "'Lua.txt' could not be used.".to_owned() });
        let subject = reading("new.json", "3.0.0", vec![module("web")]);

        let notes = determine_comparison_notes(&baseline, &subject, &config, vec![adopted], true);
        assert_eq!(vec![
            Note::SettingsAdopted { from: "old.json".to_owned(), settings: vec!["exclude"] },
            Note::SettingsDiffer { baseline: "old.json".to_owned(), subject: "new.json".to_owned(),
                    settings: vec!["braces-as-code"] },
            Note::VersionsDiffer { baseline: "old.json".to_owned(), baseline_version: "2.9.0".to_owned(),
                    subject: "new.json".to_owned(), subject_version: "3.0.0".to_owned() },
            Note::CountsInDoubt { about: "old.json".to_owned(), doubts: vec![
                    "2 of its files could not be parsed, so its counts are short by that".to_owned(),
                    "1 directory could not be read, so nothing inside was counted".to_owned(),
                    "'Lua.txt' could not be used. (language-file-unreadable)".to_owned()] },
            Note::ModulesDiffer { baseline: "old.json".to_owned(), subject: "new.json".to_owned(),
                    baseline_modules: Some("'api'".to_owned()), subject_modules: Some("'web'".to_owned()) },
            Note::LayoutFallback { layout: "list" }
        ], notes);

        // and two readings with nothing between them owe the reader nothing
        config.view.layout = crate::config_manager::Layout::Table;
        let plain = reading("a.json", "3.0.0", Vec::new());
        assert!(determine_comparison_notes(&plain, &reading("b.json", "3.0.0", Vec::new()), &config, Vec::new(), true).is_empty());

        // a doubt about the settings alone is not a doubt about the counts
        let mut settled = reading("a.json", "3.0.0", Vec::new());
        settled.warnings.push(DocumentWarning { code: "config-value-ignored".to_owned(),
                affects: "settings".to_owned(), message: "ignored.".to_owned() });
        assert!(determine_comparison_notes(&settled, &reading("b.json", "3.0.0", Vec::new()), &config, Vec::new(), true).is_empty());

        // A side whose scan found nothing is said, or the zeros under it read as everything
        // deleted. The run's own side is not: 'present' already announced that scan as it happened.
        let mut empty = reading("a.json", "3.0.0", Vec::new());
        empty.result.files_present.relevant_files = 0;
        assert_eq!(vec![Note::NothingCounted { about: "a.json".to_owned() }],
                determine_comparison_notes(&empty, &reading("b.json", "3.0.0", Vec::new()), &config, Vec::new(), true));

        let mut this_run = reading("ignored-name", "3.0.0", Vec::new());
        this_run.source = Source::Run;
        this_run.result.files_present.relevant_files = 0;
        this_run.faulty_files_count = 3;
        assert!(determine_comparison_notes(&reading("a.json", "3.0.0", Vec::new()), &this_run, &config, Vec::new(), true).is_empty());
    }

    // Settings reach a side that is about to be counted and never a document, whose numbers are
    // already fixed, so which combination of sources was named decides whether anything is adopted
    // at all. Written out in full because the answer is a guard over an or-pattern, where the
    // both-documents case is the one that quietly goes wrong.
    #[test]
    fn a_documents_settings_reach_a_side_that_is_counted_and_no_other() {
        let dir = crate::paths::test_paths::SCRATCH_DIR.to_owned() + "diff-plan-sources/";
        std::fs::create_dir_all(&dir).unwrap();
        let document = |name: &str, braces: bool| {
            let path = format!("{dir}{name}.json");
            let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
            config.engine.braces_as_code = braces;
            let per_language = hashmap!["Rust".to_owned() => stats(100, 70, 2)];
            let result = mezura_core::RunResult {
                total: Stats::total_of(&per_language), per_language, modules: Vec::new(), nested_languages: Default::default(),
                faulty_files: Vec::new(), targets: Vec::new(), unreadable_dirs: Vec::new(),
                files_present: mezura_core::FilesPresent {total_files: 2, relevant_files: 2, excluded_files: 0},
                performance: mezura_core::Performance {duration_millis: 0, threads: mezura_core::Threads::new(1, 1)}
            };
            std::fs::write(&path, crate::json_printer::create_document(&result, &chrono::Local::now(), &config)).unwrap();
            path
        };
        let with_braces = document("with-braces", true);
        let without = document("without-braces", false);

        // 'braces_as_code' is false on a fresh configuration, so a document that recorded it true
        // is a real difference and every case below is asking whether that difference travels
        let adopted_by = |operand: &str| {
            let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
            config.view.diff_against = Some(operand.to_owned());
            let request = DiffRequest::of(&mut config, &[]).unwrap().unwrap();
            let notes = match request {
                DiffRequest::BetweenTwoReadings(x) => x.notes_so_far,
                DiffRequest::AgainstThisRun(x) => x.notes_so_far
            };
            (notes, config.engine.braces_as_code)
        };

        // one document beside something that will be counted, from either side and in the single
        // form: the counting takes the document's value
        for operand in [with_braces.clone(), format!("{with_braces}..HEAD"), format!("HEAD..{with_braces}")] {
            let (notes, braces) = adopted_by(&operand);
            assert_eq!(vec![Note::SettingsAdopted { from: "with-braces.json".to_owned(),
                    settings: vec!["braces-as-code"] }], notes, "nothing was adopted for '{operand}'");
            assert!(braces, "the value was reported as adopted and not applied, for '{operand}'");
        }

        // Two documents: both sets of numbers are already fixed, so there is nothing to reach and
        // the difference is reported instead. Both orders, because only the one whose first side
        // carries the difference can tell a working guard from a missing one.
        for operand in [format!("{with_braces}..{without}"), format!("{without}..{with_braces}")] {
            let (notes, braces) = adopted_by(&operand);
            assert!(notes.is_empty(), "a document was overridden by another document, for '{operand}': {notes:?}");
            assert!(!braces);
        }

        // and two revisions have no document to take anything from
        let (notes, braces) = adopted_by("HEAD~1..HEAD");
        assert!(notes.is_empty(), "{notes:?}");
        assert!(!braces);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // Two readings taken under different rules are two measurements, and the difference between them
    // is not a change in the code. Only what can move a count is asked about.
    #[test]
    fn the_settings_the_two_readings_were_taken_under_are_compared() {
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let per_language = hashmap!["Rust".to_owned() => stats(100, 70, 2)];
        let result = mezura_core::RunResult {
            total: Stats::total_of(&per_language), per_language, modules: Vec::new(), nested_languages: Default::default(),
            faulty_files: Vec::new(), targets: Vec::new(), unreadable_dirs: Vec::new(),
            files_present: mezura_core::FilesPresent {total_files: 2, relevant_files: 2, excluded_files: 0},
            performance: mezura_core::Performance {duration_millis: 0, threads: mezura_core::Threads::new(1, 1)}
        };
        let document = crate::json_reader::parse(&crate::json_printer::create_document(&result,
                &chrono::Local::now(), &crate::config_manager::Configuration::new(vec!["./src".to_owned()]))).unwrap();
        assert!(find_settings_that_differ(&document.scope, &scope_of(&config.engine)).is_empty());

        // the order they were written in is not a difference
        config.engine.exclude_dirs = vec!["target".to_owned()];
        assert_eq!(vec!["exclude"], find_settings_that_differ(&document.scope, &scope_of(&config.engine)));

        // It decides which language a file is counted as, so a run that forced one and a run that
        // did not measured different things and the difference is not code that changed
        config.engine.exclude_dirs = Vec::new();
        config.engine.forced_languages = hashmap!["m".to_owned() => "matlab".to_owned()];
        assert_eq!(vec!["force-language"], find_settings_that_differ(&document.scope, &scope_of(&config.engine)));

        config.engine.forced_languages = HashMap::new();
        config.engine.braces_as_code = true;
        config.engine.no_gitignore = true;
        assert_eq!(vec!["braces-as-code", "no-gitignore"], find_settings_that_differ(&document.scope, &scope_of(&config.engine)));

        // and hiding the keywords is among them: it moves no line or code count, but a side that
        // did not count keywords would read as every keyword written since
        config.engine.braces_as_code = false;
        config.engine.no_gitignore = false;
        config.engine.count_keywords = false;
        assert_eq!(vec!["hide keywords"], find_settings_that_differ(&document.scope, &scope_of(&config.engine)));
    }

    // The settings of a document reach whatever is counted against it, unless this run's own
    // command line spoke: the mask decides per setting, the values move with their spelling
    // corrected ('gitignore' recorded as obeyed, the flag saying the opposite), and what was
    // already agreed on is not reported as taken.
    #[test]
    fn a_documents_settings_are_taken_unless_the_command_line_set_its_own() {
        let document = Scope {
            exclude: vec!["target".to_owned()],
            languages: Vec::new(),
            excluded_languages: Vec::new(),
            forced_languages: HashMap::new(),
            braces_as_code: true,
            search_in_dotted: false,
            gitignore: false,
            keywords_counted: true
        };

        // Nothing typed: what differs is taken, what agrees is not reported
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let adopted = resolve_settings(&document, &mut config);
        assert_eq!(vec!["exclude", "braces-as-code", "no-gitignore"], adopted);
        assert_eq!(vec!["target".to_owned()], config.engine.exclude_dirs);
        assert!(config.engine.braces_as_code);
        // recorded as "the file was not obeyed", so the flag turns on
        assert!(config.engine.no_gitignore);
        // and a second pass finds nothing left to take
        assert!(resolve_settings(&document, &mut config).is_empty());

        // The same difference with the value typed stays as typed, and is not reported as taken
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.typed_explicitly.braces_as_code = true;
        config.typed_explicitly.exclude = true;
        assert_eq!(vec!["no-gitignore"], resolve_settings(&document, &mut config));
        assert!(!config.engine.braces_as_code);
        assert!(config.engine.exclude_dirs.is_empty());

        // The order two lists were written in is not a difference
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.engine.exclude_dirs = vec!["target".to_owned()];
        config.engine.no_gitignore = true;
        config.engine.braces_as_code = true;
        assert!(resolve_settings(&document, &mut config).is_empty());

        // The keyword flag moves both halves, or the counting and the printing would disagree
        let without_keywords = Scope { keywords_counted: false, gitignore: true,
                braces_as_code: false, exclude: Vec::new(), ..document };
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        assert_eq!(vec!["hide keywords"], resolve_settings(&without_keywords, &mut config));
        assert!(!config.engine.count_keywords);
        assert!(config.view.hidden.keywords);

        // and in the other direction a standing '--hide keywords' from a configuration yields,
        // while a typed one is kept
        let with_keywords = Scope { keywords_counted: true, ..without_keywords.clone() };
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.engine.count_keywords = false;
        config.view.hidden.keywords = true;
        assert_eq!(vec!["hide keywords"], resolve_settings(&with_keywords, &mut config));
        assert!(config.engine.count_keywords && !config.view.hidden.keywords);

        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.engine.count_keywords = false;
        config.view.hidden.keywords = true;
        config.typed_explicitly.hide_keywords = true;
        assert!(resolve_settings(&with_keywords, &mut config).is_empty());
        assert!(!config.engine.count_keywords && config.view.hidden.keywords);
    }
}
