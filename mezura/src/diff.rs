use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use mezura_core::{EngineConfig, FileEntry, ForcedLanguages, Language, LanguageNames, Languages,
        ModuleResult, RunResult, Stats, UNNAMED_MODULE_NAME, render};
use mezura_core::language_file::ConflictRules;

use super::config_manager::{ByFile, Configuration, Layout, SortCriterion};
use super::config_manager::{COUNTING, COUNT_GENERATED, COUNT_MINIFIED, EXCLUDE, EXCLUDE_LANGUAGES,
        FORCE_LANGUAGE, LANGUAGES, NO_GITIGNORE, NO_IGNORE_FILES, SEARCH_IN_DOTTED};
use super::json_reader::{DocumentError, DocumentWarning, Scope};
use super::sources::RevisionSide;

// The half of a document's warnings that says the numbers themselves may be wrong, as the document
// spells it
const COUNTS_AFFECTED : &str = "counts";

// The one compared setting that is not a config key of its own: it is the 'keywords' value of
// '--hide', and the log's comparison filters it out because the log holds no keyword counts
pub const HIDE_KEYWORDS : &str = "hide keywords";

const THIS_RUN_NAME : &str = "this run";

pub enum Source {
    Run,
    Document { path: String },
    // Both halves, because neither derives from the other: the hash is what was really measured,
    // 'asked_for' is what makes it readable six months later, 'v2.0.1' over '030e6e72a1'.
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
    // Counts and not lists: a document details the failures only when '--show-faulty-files' asked
    // it to, so the lists in 'result' can read empty for a side whose counts are short.
    pub faulty_files_count: usize,
    pub unreadable_dirs_count: usize,
    // False only for a document written without '--by-file', whose empty file lists must not read
    // as a tree where every file is new; a side that counted nothing had nothing to record
    pub files_recorded: bool,
    // How many rows a capped '--by-file' left out of a document, which would read the same way
    pub files_hidden: usize,
    pub result: RunResult
}

impl Reading {
    pub fn of_git_revision(asked_for: &str, commit: String, taken: String, result: RunResult,
            config: &Configuration) -> Self {
        Reading {
            source: Source::GitRevision { commit, asked_for: asked_for.to_owned() },
            taken,
            version: super::config_manager::VERSION_ID.trim_start_matches('v').to_owned(),
            scope: scope_of(&config.engine, config.view.counting),
            warnings: Vec::new(),
            faulty_files_count: result.faulty_files.len(),
            unreadable_dirs_count: result.unreadable_dirs.len(),
            files_recorded: true,
            files_hidden: 0,
            result
        }
    }

    // A copy, because the result is still being presented around the comparison
    pub fn of_this_run(result: &RunResult, taken: &chrono::DateTime<chrono::Local>,
            config: &Configuration) -> Self {
        Reading {
            source: Source::Run,
            taken: taken.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
            version: super::config_manager::VERSION_ID.trim_start_matches('v').to_owned(),
            scope: scope_of(&config.engine, config.view.counting),
            warnings: Vec::new(),
            faulty_files_count: result.faulty_files.len(),
            unreadable_dirs_count: result.unreadable_dirs.len(),
            files_recorded: true,
            files_hidden: 0,
            result: result.clone()
        }
    }

    // Whether a file comparison against this side answers honestly: rows a document never held, or
    // that a cap left out of it, would all read as files written since
    pub fn can_pair_files(&self) -> bool {
        self.files_recorded && self.files_hidden == 0
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

    // None unless both sides hold every row there was to pair: against a document written without
    // '--by-file', or capped by it, every file it is missing would read as new
    pub fn resolve_by_file(&self, config: &Configuration) -> Option<ByFile> {
        config.view.by_file.filter(|_| self.baseline.can_pair_files() && self.subject.can_pair_files())
    }
}

// Two phases because the order matters: a document's settings have to reach the language resolution
// and the counting that follow, and a baseline that turns out not to be one must cost no scan.
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
        // there is nothing to take them from: both sides then run as declared, and what differs is
        // reported above the table
        let settings_source = match (&baseline, &subject) {
            (DiffSide::Document(x), None) => Some(x),
            (DiffSide::Document(x), Some(other)) | (other, Some(DiffSide::Document(x)))
                    if other.needs_counting() => Some(x),
            _ => None
        };
        let notes_so_far = settings_source.map(|document| adopt_settings_from(document, config))
                .unwrap_or_default().into_iter().collect();

        // Known before the scan: a document that cannot pair files closes the gate at print time,
        // so the run should not pay for collecting an entry per file
        let cannot_pair = |side: &DiffSide| matches!(side,
                DiffSide::Document(x) if !x.can_pair_files());
        if cannot_pair(&baseline) || subject.as_ref().is_some_and(cannot_pair) {
            config.engine.collect_files = false;
        }

        let languages = available.to_vec();
        Ok(Some(match subject {
            Some(subject) => DiffRequest::BetweenTwoReadings(
                    BothSidesNamed { baseline, subject, notes_so_far, languages }),
            None => DiffRequest::AgainstThisRun(BaselineOnly { baseline, notes_so_far, languages })
        }))
    }
}

// The languages travel with the request because 'Languages::resolve' and 'run' both consume what
// they are handed, so every side that is counted needs a list of its own.
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
            conflicts: &ConflictRules) -> Result<Comparison, String>
    {
        let (_, reported) = Languages::resolve(&config.engine, self.languages.clone(), conflicts);
        super::warning_collector::report_language_resolution_warnings(reported);

        let [baseline, subject] = <[PreparedSide; 2]>::try_from(
                prepare_sides(vec![self.baseline, self.subject], &config.engine)?)
                .ok().expect("two sides in, two sides out");

        let (baseline, notes) = baseline.into_reading(config, self.languages.clone(), conflicts)?;
        let mut notes_so_far = self.notes_so_far;
        notes_so_far.extend(notes);
        let (subject, notes) = subject.into_reading(config, self.languages, conflicts)?;
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
            conflicts: &ConflictRules) -> Result<CountedBaseline, String>
    {
        let [baseline] = <[PreparedSide; 1]>::try_from(prepare_sides(vec![self.baseline], &config.engine)?)
                .ok().expect("one side in, one side out");
        let (baseline, notes) = baseline.into_reading(config, self.languages, conflicts)?;
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
// that could only panic or quietly resolve on the spot.
enum PreparedSide {
    Document(Box<Reading>),
    Revision(RevisionSide)
}

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
            conflicts: &ConflictRules) -> Result<(Reading, Vec<Note>), String>
    {
        match self {
            PreparedSide::Document(reading) => Ok((*reading, Vec::new())),
            PreparedSide::Revision(side) => super::sources::count_git_revision(side, config, languages,
                    conflicts).map_err(|x| x.to_string())
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
            // A key that is absent gets its own sentence: nothing needs to have gone wrong, an older
            // mezura simply had not met it
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

pub struct FileStatsChange {
    // In the relative form the two sides were paired by
    pub path: String,
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
// told apart from standing still: 'relative_change' answers 0.0 when there was nothing to grow
// from, which would print as "no change" for a figure that was not there at all.
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
    // A document taken without '--by-file', so the file rows this run asked for have nothing to be
    // paired against
    FilesNotRecorded { about: String },
    // A document whose file rows were cut by a '--by-file' number, so the missing ones would read
    // as new
    FilesCut { about: String, hidden: usize },
    // None is a side that declared no modules at all
    ModulesDiffer { baseline: String, subject: String, baseline_modules: Option<String>, subject_modules: Option<String> },
    LayoutFallback { layout: &'static str },
    NoGitignoreInCheckout { git_revision: String },
    MissingInRevision { git_revision: String, targets: Vec<String> }
}

// Splits '--diff a.json..b.json' into the two readings it names, and answers None for the second
// when only one was given, whose second reading is this run. The trap is that '..' is a separator
// here and a directory in every filesystem, so '--diff ../old.json' must not come apart into an
// empty name and '/old.json': what was written is taken whole if it names something that exists.
fn split_operand(value: &str) -> Result<(&str, Option<&str>), String> {
    if std::path::Path::new(value).exists() {
        return Ok((value, None));
    }
    // Beyond one there is no telling which is the separator and which is a climb, and guessing would
    // mean asking the disk about every way of cutting it. Refused instead.
    if value.matches("..").count() > 1 {
        return Err(format!("'{value}' has more than one '..' in it, and only one of them can be the \
separator between the two readings. Write the paths out without the '..' that climbs."));
    }

    match value.split_once("..") {
        Some((before, after)) if !before.is_empty() && !after.is_empty() => Ok((before, Some(after))),
        // A separator with nothing after it is a line left half written, and saying so is worth more
        // than the "no such file" that reading it whole would produce
        Some((before, _)) if !before.is_empty() => Err(format!("'{value}' names a reading before the \
'..' and none after it. Write the second one, or drop the '..' to compare '{before}' against this run.")),
        // Nothing before it is an ordinary path climbing a directory
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
            files_recorded: document.files_recorded, files_hidden: document.files_hidden,
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
// is being written.
pub fn create_comparison_rows(baseline: &HashMap<String, Stats>, subject: &HashMap<String, Stats>,
        sort_by: SortCriterion, top: Option<usize>, model: mezura_core::CountingModel)
-> (Vec<LanguageStatsChange>, usize)
{
    // Held at what the subject has, so one that disappeared sorts to the bottom where a zero belongs
    let mut merged = subject.clone();
    for name in baseline.keys() {
        merged.entry(name.clone()).or_default();
    }

    let names = super::result_printer::get_sorted_language_names(&merged, sort_by, model);
    let shown = top.map_or(names.len(), |x| x.min(names.len()));

    (names[..shown].iter().map(|name| LanguageStatsChange {
        baseline: baseline.get(name).cloned().unwrap_or_default(),
        subject: subject.get(name).cloned().unwrap_or_default(),
        name: name.clone()
    }).collect(), merged.len())
}

// The files of one language on both sides, paired by the relative names 'determine_file_bases'
// chose, and only the ones whose counts moved: in a tree of five thousand files where thirty
// changed, the rows are those thirty, with a file only one side has among them as 'new' or 'gone'.
// 'by_file' then keeps the biggest moves, not the biggest files.
pub fn create_file_comparison_rows(baseline: &[&FileEntry], subject: &[&FileEntry],
        bases: &(String, String), by_file: ByFile, sort_by: SortCriterion,
        model: mezura_core::CountingModel) -> (Vec<FileStatsChange>, usize)
{
    let mut merged: HashMap<String, (Option<&Stats>, Option<&Stats>)> =
            HashMap::with_capacity(baseline.len() + subject.len());
    for file in baseline {
        merged.insert(relativise(&file.path, &bases.0), (Some(&file.stats), None));
    }
    for file in subject {
        merged.entry(relativise(&file.path, &bases.1)).or_default().1 = Some(&file.stats);
    }

    // Filtered on the references, so only the rows that survive are built: raw counts that are
    // equal are equal under either model
    let mut rows = merged.into_iter()
            .filter(|(_, (before, now))| before != now)
            .map(|(path, (before, now))| FileStatsChange {
                baseline: before.cloned().unwrap_or_default(),
                subject: now.cloned().unwrap_or_default(),
                path
            }).collect::<Vec<_>>();

    // Every row is one file, so a sort by files would measure every move at zero: lines stand in
    // for it there
    let criterion = if sort_by == SortCriterion::Files {SortCriterion::Lines} else {sort_by};
    if criterion == SortCriterion::Name {
        rows.sort_by(|one, other| one.path.cmp(&other.path));
    } else {
        rows.sort_by_cached_key(|row| (std::cmp::Reverse(
                criterion.get_value_of(&row.baseline, model)
                        .abs_diff(criterion.get_value_of(&row.subject, model))),
                row.path.clone()));
    }
    let shown = by_file.shown_out_of(rows.len());
    let hidden = rows.len() - shown;
    rows.truncate(shown);

    (rows, hidden)
}

pub fn collect_files_per_language(modules: &[ModuleResult]) -> HashMap<&str, Vec<&FileEntry>> {
    let mut merged: HashMap<&str, Vec<&FileEntry>> = HashMap::new();
    for module in modules {
        for (language, files) in &module.files {
            merged.entry(language.as_str()).or_default().extend(files.iter());
        }
    }

    merged
}

// Each side's rows are named relative to the common directory of its own targets, so two checkouts
// of one project pair on the same names. When one side's targets are a subset of the other's, the
// wider side's directory serves both: a revision that lacks one of the targets would otherwise
// relativise deeper than the run it is compared against, and nothing would pair. Two sides whose
// targets merely nest keep their own, since a worktree inside the repository is another tree.
pub fn determine_file_bases(baseline: &RunResult, subject: &RunResult) -> (String, String) {
    let of_baseline = super::result_printer::find_common_directory_of(&baseline.targets);
    let of_subject = super::result_printer::find_common_directory_of(&subject.targets);

    let paths_of = |result: &RunResult| result.targets.iter()
            .map(|x| fold_path(&x.path).into_owned()).collect::<HashSet<_>>();
    let (baseline_targets, subject_targets) = (paths_of(baseline), paths_of(subject));
    let one_holds_the_other = !baseline_targets.is_empty() && !subject_targets.is_empty()
            && (baseline_targets.is_subset(&subject_targets) || subject_targets.is_subset(&baseline_targets));

    if one_holds_the_other {
        let outer = if super::result_printer::is_inside(&fold_path(of_subject), &fold_path(of_baseline))
                {of_baseline} else {of_subject};
        (outer.to_owned(), outer.to_owned())
    } else {
        (of_baseline.to_owned(), of_subject.to_owned())
    }
}

// The modules of the two readings matched up by name, and None when there is nothing to show: the
// sets differ, or nothing was ever named and the only pair is the one module holding everything.
// Nothing short of the same set can be shown, because a module only one side has would be compared
// against nothing and every language in it would read as written from scratch or deleted whole.
pub fn pair_modules<'a>(baseline: &'a RunResult, subject: &'a RunResult) -> Option<Vec<ModulePair<'a>>> {
    if baseline.modules.len() != subject.modules.len() || !subject.has_modules() {
        return None;
    }

    // A name is claimed once, so finding every one of the second reading's in the first means the
    // sets are equal, and collecting into an Option lets one missing name answer for all of them
    subject.modules.iter().map(|now| baseline.modules.iter().find(|x| x.name == now.name)
            .map(|before| ModulePair { name: now.name.as_deref(), before, now })).collect()
}

// The modules of one reading as the message names them, and None when nothing was declared. The
// leftovers are in the list: being on one side and not the other is one of the ways two sets
// differ, and a message that left them out would name two lists that look identical.
pub fn format_module_names(result: &RunResult) -> Option<String> {
    result.has_modules().then(|| result.modules.iter()
            .map(|x| format!("'{}'", x.name.as_deref().unwrap_or(UNNAMED_MODULE_NAME)))
            .collect::<Vec<_>>().join(", "))
}

// The settings of a run in the shape a document records them, so that a comparison asks the same
// question of both its sides. The gitignore flag is turned around here and nowhere else: a document
// records whether the file was obeyed, the command line records whether it was not.
pub fn scope_of(engine: &mezura_core::EngineConfig, counting: mezura_core::CountingModel) -> Scope {
    Scope {
        exclude: engine.exclude_dirs.clone(),
        languages: engine.languages_of_interest.to_written_form(),
        excluded_languages: engine.excluded_languages.to_written_form(),
        forced_languages: engine.forced_languages.to_written_form(),
        counting: counting.name().to_owned(),
        search_in_dotted: engine.should_search_in_dotted,
        gitignore: !engine.no_gitignore,
        ignore_files: !engine.no_ignore_files,
        keywords_counted: engine.count_keywords,
        count_minified: engine.count_minified,
        count_generated: engine.count_generated
    }
}

// A difference here is not a change in the code, and every one of these can move a count on its own.
// 'counting' is deliberately not among them: a document and a log carry the count of every class of
// line, and both sides are folded by the model this run is showing, so two readings taken under
// different models are still compared exactly.
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
    if baseline.search_in_dotted != subject.search_in_dotted {differ.push(SEARCH_IN_DOTTED)}
    if baseline.gitignore != subject.gitignore {differ.push(NO_GITIGNORE)}
    if baseline.ignore_files != subject.ignore_files {differ.push(NO_IGNORE_FILES)}
    if baseline.keywords_counted != subject.keywords_counted {differ.push(HIDE_KEYWORDS)}
    if baseline.count_minified != subject.count_minified {differ.push(COUNT_MINIFIED)}
    if baseline.count_generated != subject.count_generated {differ.push(COUNT_GENERATED)}

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
    if !typed.languages && different(&document.languages, &config.engine.languages_of_interest.to_written_form()) {
        config.engine.languages_of_interest = LanguageNames::of_written_form(&document.languages);
        adopted.push(LANGUAGES);
    }
    if !typed.excluded_languages
            && different(&document.excluded_languages, &config.engine.excluded_languages.to_written_form()) {
        config.engine.excluded_languages = LanguageNames::of_written_form(&document.excluded_languages);
        adopted.push(EXCLUDE_LANGUAGES);
    }
    if !typed.forced_languages && document.forced_languages != config.engine.forced_languages.to_written_form() {
        config.engine.forced_languages = ForcedLanguages::of_written_form(&document.forced_languages);
        adopted.push(FORCE_LANGUAGE);
    }
    // Only a model this build has can be adopted: a word it does not know names a fold it cannot
    // perform, so it is left to the settings-differ note rather than half-imitated
    if !typed.counting && document.counting != config.view.counting.name()
            && let Some(model) = mezura_core::CountingModel::parse(&document.counting) {
        config.view.counting = model;
        adopted.push(COUNTING);
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
    if !typed.no_ignore_files && document.ignore_files == config.engine.no_ignore_files {
        config.engine.no_ignore_files = !document.ignore_files;
        adopted.push(NO_IGNORE_FILES);
    }
    // Both halves of the one flag, or the counting and the printing would disagree
    if !typed.hide_keywords && document.keywords_counted != config.engine.count_keywords {
        config.engine.count_keywords = document.keywords_counted;
        config.view.hidden.keywords = !document.keywords_counted;
        adopted.push(HIDE_KEYWORDS);
    }
    if !typed.count_minified && document.count_minified != config.engine.count_minified {
        config.engine.count_minified = document.count_minified;
        adopted.push(COUNT_MINIFIED);
    }
    if !typed.count_generated && document.count_generated != config.engine.count_generated {
        config.engine.count_generated = document.count_generated;
        adopted.push(COUNT_GENERATED);
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
    // A build whose language files were corrected counts the same tree differently
    if baseline.version != subject.version {
        notes.push(Note::VersionsDiffer { baseline: baseline.determine_display_name(),
                baseline_version: baseline.version.clone(), subject: subject.determine_display_name(),
                subject_version: subject.version.clone() });
    }
    // How each side's own scan went. Nothing else says it for a document or a revision, and a side
    // that failed to parse half its files reads as code that shrank.
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
        // leaves a whole language at zero, which reads here as a language that appeared out of
        // nowhere
        doubts.extend(reading.warnings.iter().filter(|x| x.affects == COUNTS_AFFECTED)
                .map(|x| format!("{} ({})", x.message, x.code)));
        if !doubts.is_empty() {
            notes.push(Note::CountsInDoubt { about: reading.determine_display_name(), doubts });
        }
        if config.view.by_file.is_some() {
            if !reading.files_recorded {
                notes.push(Note::FilesNotRecorded { about: reading.determine_display_name() });
            } else if reading.files_hidden > 0 {
                notes.push(Note::FilesCut { about: reading.determine_display_name(),
                        hidden: reading.files_hidden });
            }
        }
    }
    if modules_unpaired && (baseline.result.has_modules() || subject.result.has_modules()) {
        notes.push(Note::ModulesDiffer { baseline: baseline.determine_display_name(),
                subject: subject.determine_display_name(), baseline_modules: format_module_names(&baseline.result),
                subject_modules: format_module_names(&subject.result) });
    }
    // The other two layouts have nothing to show for a comparison, and the reader is told so rather
    // than left wondering why the layout was ignored
    if matches!(config.view.layout, Layout::List | Layout::Matrix) {
        notes.push(Note::LayoutFallback { layout: config.view.layout.name() });
    }

    notes
}

// A path the base does not hold keeps its absolute form, which pairs with nothing and is honest
// about it. Windows spells one path in several cases, so the matching folds the way
// 'move_excludes_into_checkout' does; the ASCII fold never moves a byte, so the cut lands on the
// unfolded original.
fn relativise(path: &str, base: &str) -> String {
    if base.is_empty() {
        return path.to_owned();
    }
    let (folded_path, folded_base) = (fold_path(path), fold_path(base));
    if folded_path == folded_base {
        return path.rsplit('/').next().unwrap_or(path).to_owned();
    }
    match folded_path.strip_prefix(folded_base.as_ref()) {
        Some(rest) if rest.starts_with('/') => path[base.len() + 1..].to_owned(),
        _ => path.to_owned()
    }
}

fn fold_path(path: &str) -> Cow<'_, str> {
    if cfg!(windows) {Cow::Owned(path.to_ascii_lowercase())} else {Cow::Borrowed(path)}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_manager::SortCriterion;

    fn stats(lines: usize, code: usize, files: usize) -> Stats {
        crate::test_support::plain_stats_of(files, lines * 30, lines, code, 0, HashMap::new())
    }

    #[test]
    fn a_pair_of_readings_is_told_apart_from_a_path_that_climbs() {
        let split = |x| split_operand(x).unwrap();
        assert_eq!(("a.json", Some("b.json")), split("a.json..b.json"));
        assert_eq!(("main", Some("HEAD")), split("main..HEAD"));
        assert_eq!(("old.json", None), split("old.json"));

        // Cargo runs these from the package root, so this one climbs to the repository's own README,
        // which really is there and is therefore taken whole
        assert_eq!(("../README.md", None), split("../README.md"));
        assert_eq!(("../gone.json", None), split("../gone.json"));

        let half = split_operand("a.json..").unwrap_err();
        assert!(half.contains("none after it") && half.contains("'a.json'"), "{half}");
        assert_eq!(("..b.json", None), split("..b.json"));
        assert_eq!(("..", None), split(".."));

        let refused = split_operand("a/../b.json..c/../d.json").unwrap_err();
        assert!(refused.contains("more than one '..'") && refused.contains("climbs"), "{refused}");
        assert_eq!(("../mezura/../README.md", None), split("../mezura/../README.md"));
    }

    #[test]
    fn a_figure_that_only_one_of_the_two_readings_has_is_named_and_not_given_a_percentage() {
        assert_eq!(Change::Appeared, change_of(0, 500));
        assert_eq!(Change::Gone, change_of(500, 0));
        assert_eq!(Change::Percent(0.0), change_of(0, 0));
        assert_eq!(Change::Percent(100.0), change_of(100, 200));
        assert_eq!(Change::Percent(-50.0), change_of(100, 50));
    }

    #[test]
    fn every_language_of_either_reading_gets_a_row_in_the_order_the_report_uses() {
        let baseline = hashmap!["Rust".to_owned() => stats(100, 70, 2), "Java".to_owned() => stats(40, 30, 1)];
        let subject = hashmap!["Rust".to_owned() => stats(150, 100, 3), "Go".to_owned() => stats(60, 50, 1)];

        let model = mezura_core::CountingModel::Content;
        let (rows, union) = create_comparison_rows(&baseline, &subject, SortCriterion::Lines, None, model);
        assert_eq!(3, union);
        assert_eq!(vec!["Rust".to_owned(), "Go".to_owned(), "Java".to_owned()],
                rows.iter().map(|x| x.name.clone()).collect::<Vec<_>>());
        // the one that is gone sorts last, holding the zero it is now, and keeps every figure it had
        assert_eq!(40, rows[2].baseline.lines);
        assert_eq!(30, rows[2].baseline.calculate_code_lines(model));
        assert_eq!(0, rows[2].subject.lines);
        assert_eq!(0, rows[1].baseline.lines);
        assert_eq!(60, rows[1].subject.lines);

        // '--top' cuts the rows and never the union, and a document asks for no cut at all
        let (cut, union) = create_comparison_rows(&baseline, &subject, SortCriterion::Lines, Some(2), model);
        assert_eq!((2, 3), (cut.len(), union));
        assert_eq!(3, create_comparison_rows(&baseline, &subject, SortCriterion::Lines, None, model).0.len());

        assert_eq!(vec!["Go".to_owned(), "Java".to_owned(), "Rust".to_owned()],
                create_comparison_rows(&baseline, &subject, SortCriterion::Name, None, model).0.iter()
                        .map(|x| x.name.clone()).collect::<Vec<_>>());
    }

    fn entry(path: &str, lines: usize, code: usize) -> FileEntry {
        FileEntry { path: path.to_owned(), stats: stats(lines, code, 1), nested_languages: HashMap::new() }
    }

    #[test]
    fn only_the_files_that_moved_get_rows_and_the_cap_keeps_the_biggest_moves() {
        let before = [entry("D:/proj/src/a.rs", 100, 70), entry("D:/proj/src/b.rs", 50, 40),
                entry("D:/proj/src/gone.rs", 10, 5)];
        let after = [entry("C:/other/proj/src/a.rs", 130, 90), entry("C:/other/proj/src/b.rs", 50, 40),
                entry("C:/other/proj/src/added.rs", 8, 6)];
        let (before, after) = (before.iter().collect::<Vec<_>>(), after.iter().collect::<Vec<_>>());
        let bases = ("D:/proj".to_owned(), "C:/other/proj".to_owned());
        let model = mezura_core::CountingModel::Content;

        // The unchanged file has no row; the biggest move first, whichever side has the file
        let (rows, hidden) = create_file_comparison_rows(&before, &after, &bases, ByFile::All,
                SortCriterion::Lines, model);
        assert_eq!(0, hidden);
        assert_eq!(vec!["src/a.rs", "src/gone.rs", "src/added.rs"],
                rows.iter().map(|x| x.path.as_str()).collect::<Vec<_>>());
        assert_eq!((100, 130), (rows[0].baseline.lines, rows[0].subject.lines));
        assert_eq!((10, 0), (rows[1].baseline.lines, rows[1].subject.lines));
        assert_eq!((0, 8), (rows[2].baseline.lines, rows[2].subject.lines));

        // The cap keeps the biggest moves of what changed, and says how many it left out
        let (cut, hidden) = create_file_comparison_rows(&before, &after, &bases, ByFile::Capped(1),
                SortCriterion::Lines, model);
        assert_eq!((1, 2), (cut.len(), hidden));
        assert_eq!("src/a.rs", cut[0].path);

        // Under a sort by name the paths themselves order the rows
        let (named, _) = create_file_comparison_rows(&before, &after, &bases, ByFile::All,
                SortCriterion::Name, model);
        assert_eq!(vec!["src/a.rs", "src/added.rs", "src/gone.rs"],
                named.iter().map(|x| x.path.as_str()).collect::<Vec<_>>());

        // Every row is one file, so a sort by files measures the move in lines instead
        let (by_files, _) = create_file_comparison_rows(&before, &after, &bases, ByFile::All,
                SortCriterion::Files, model);
        assert_eq!(vec!["src/a.rs", "src/gone.rs", "src/added.rs"],
                by_files.iter().map(|x| x.path.as_str()).collect::<Vec<_>>());
    }

    #[test]
    fn a_path_is_relativised_only_on_a_whole_component_boundary() {
        assert_eq!("src/a.rs", relativise("D:/proj/src/a.rs", "D:/proj"));
        assert_eq!("a.rs", relativise("D:/proj/a.rs", "D:/proj"));
        assert_eq!("main.rs", relativise("D:/proj/main.rs", "D:/proj/main.rs"));
        assert_eq!("D:/project-x/a.rs", relativise("D:/project-x/a.rs", "D:/proj"));
        assert_eq!("D:/elsewhere/a.rs", relativise("D:/elsewhere/a.rs", "D:/proj"));
        assert_eq!("D:/proj/a.rs", relativise("D:/proj/a.rs", ""));

        // Windows spells one path in several cases, and the cut still lands on the original
        if cfg!(windows) {
            assert_eq!("src/A.rs", relativise("d:/Proj/src/A.rs", "D:/proj"));
        }
    }

    #[test]
    fn the_file_bases_agree_only_when_one_sides_targets_are_a_subset_of_the_others() {
        let with_targets = |paths: &[&str]| {
            let per_language = hashmap!["Rust".to_owned() => stats(100, 70, 2)];
            RunResult {
                total: Stats::total_of(&per_language), per_language, modules: Vec::new(), nested_languages: HashMap::new(),
                faulty_files: Vec::new(), minified_files: 0, generated_files: 0, unreadable_dirs: Vec::new(),
                targets: paths.iter().map(|x| mezura_core::Target::of(*x)).collect(),
                files_present: mezura_core::FilesPresent {total_files: 2, relevant_files: 2, excluded_files: 0},
                performance: mezura_core::Performance {duration_millis: 0, threads: mezura_core::Threads::new(1, 1)}
            }
        };

        // A revision that lacks one of the targets relativises where the run does
        assert_eq!(("D:/p".to_owned(), "D:/p".to_owned()), determine_file_bases(
                &with_targets(&["D:/p/src"]), &with_targets(&["D:/p/src", "D:/p/tests"])));
        assert_eq!(("D:/p".to_owned(), "D:/p".to_owned()), determine_file_bases(
                &with_targets(&["D:/p/src", "D:/p/tests"]), &with_targets(&["D:/p/src"])));

        // Two checkouts of one project each keep their own root, and pair on the names inside it
        assert_eq!(("D:/p/src".to_owned(), "C:/w/proj/src".to_owned()), determine_file_bases(
                &with_targets(&["D:/p/src"]), &with_targets(&["C:/w/proj/src"])));

        // A worktree inside the repository is another tree, not a subset of its targets
        assert_eq!(("D:/repo".to_owned(), "D:/repo/wt/feature".to_owned()), determine_file_bases(
                &with_targets(&["D:/repo"]), &with_targets(&["D:/repo/wt/feature"])));

        assert_eq!(("D:/p/src".to_owned(), String::new()), determine_file_bases(
                &with_targets(&["D:/p/src"]), &with_targets(&[])));
    }

    #[test]
    fn the_modules_are_paired_by_name_and_a_set_that_is_not_the_same_is_not_paired_at_all() {
        let module = |name: Option<&str>, lines: usize| {
            let per_language = hashmap!["Rust".to_owned() => stats(lines, lines, 1)];
            ModuleResult {name: name.map(str::to_owned), total: Stats::total_of(&per_language), per_language,
                    nested_languages: HashMap::new(), files: HashMap::new()}
        };
        let result = |modules: Vec<ModuleResult>| {
            let per_language = hashmap!["Rust".to_owned() => stats(100, 70, 2)];
            mezura_core::RunResult {
                total: Stats::total_of(&per_language), per_language, modules, nested_languages: HashMap::new(),
                faulty_files: Vec::new(), minified_files: 0, generated_files: 0, targets: Vec::new(), unreadable_dirs: Vec::new(),
                files_present: mezura_core::FilesPresent {total_files: 2, relevant_files: 2, excluded_files: 0},
                performance: mezura_core::Performance {duration_millis: 0, threads: mezura_core::Threads::new(1, 1)}
            }
        };

        // The order they were declared in is not a difference: the pairs follow the second reading
        // and find the first's wherever it put them
        let before = result(vec![module(Some("cli"), 40), module(Some("core"), 100), module(None, 7)]);
        let now = result(vec![module(Some("core"), 120), module(Some("cli"), 44), module(None, 9)]);
        let pairs = pair_modules(&before, &now).unwrap();
        assert_eq!(vec![Some("core"), Some("cli"), None], pairs.iter().map(|x| x.name).collect::<Vec<_>>());
        assert_eq!(100, pairs[0].before.total.lines);
        assert_eq!(120, pairs[0].now.total.lines);
        assert_eq!(7, pairs[2].before.total.lines);

        // One name that is not on both sides takes the whole block with it, the leftover included
        assert!(pair_modules(&before, &result(vec![module(Some("core"), 120), module(Some("web"), 44),
                module(None, 9)])).is_none());
        assert!(pair_modules(&before, &result(vec![module(Some("core"), 120), module(Some("cli"), 44)])).is_none());
        assert!(pair_modules(&result(vec![module(None, 100)]), &now).is_none());

        // A run that named nothing has the one module holding everything, and a pair of those has
        // nothing to show
        let plain = result(vec![module(None, 100)]);
        assert!(pair_modules(&plain, &plain).is_none());
        assert!(!plain.has_modules());

        assert_eq!(Some("'core', 'cli', '(unnamed)'".to_owned()), format_module_names(&now));
        assert_eq!(None, format_module_names(&plain));
    }

    // Every kind of note at once, so that the order they are read in is pinned here
    #[test]
    fn the_notes_carry_every_fact_the_reader_is_owed_in_reading_order() {
        let reading = |name: &str, version: &str, modules: Vec<ModuleResult>| {
            let per_language = hashmap!["Rust".to_owned() => stats(100, 70, 2)];
            Reading {
                source: Source::Document { path: name.to_owned() },
                taken: "2026-08-07T10:00:00+03:00".to_owned(),
                version: version.to_owned(),
                scope: scope_of(&mezura_core::EngineConfig::default(), mezura_core::CountingModel::Content),
                warnings: Vec::new(),
                faulty_files_count: 0,
                unreadable_dirs_count: 0,
                files_recorded: true,
                files_hidden: 0,
                result: RunResult {
                    total: Stats::total_of(&per_language), per_language, modules, nested_languages: HashMap::new(),
                    faulty_files: Vec::new(), minified_files: 0, generated_files: 0, targets: Vec::new(), unreadable_dirs: Vec::new(),
                    files_present: mezura_core::FilesPresent {total_files: 2, relevant_files: 2, excluded_files: 0},
                    performance: mezura_core::Performance {duration_millis: 0, threads: mezura_core::Threads::new(1, 1)}
                }
            }
        };
        let module = |name: &str| {
            let per_language = hashmap!["Rust".to_owned() => stats(50, 40, 1)];
            ModuleResult { name: Some(name.to_owned()), total: Stats::total_of(&per_language), per_language,
                    nested_languages: HashMap::new(), files: HashMap::new() }
        };

        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.view.layout = crate::config_manager::Layout::List;
        let adopted = Note::SettingsAdopted { from: "old.json".to_owned(), settings: vec!["exclude"] };

        let mut baseline = reading("old.json", "2.9.0", vec![module("api")]);
        baseline.scope.search_in_dotted = true;
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
                    settings: vec!["search-in-dotted"] },
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

        config.view.layout = crate::config_manager::Layout::Table;
        let plain = reading("a.json", "3.0.0", Vec::new());
        assert!(determine_comparison_notes(&plain, &reading("b.json", "3.0.0", Vec::new()), &config, Vec::new(), true).is_empty());

        // a doubt about the settings alone is not a doubt about the counts
        let mut settled = reading("a.json", "3.0.0", Vec::new());
        settled.warnings.push(DocumentWarning { code: "config-value-ignored".to_owned(),
                affects: "settings".to_owned(), message: "ignored.".to_owned() });
        assert!(determine_comparison_notes(&settled, &reading("b.json", "3.0.0", Vec::new()), &config, Vec::new(), true).is_empty());

        // The run's own side is not said, since 'present' already announced that scan as it happened
        let mut empty = reading("a.json", "3.0.0", Vec::new());
        empty.result.files_present.relevant_files = 0;
        assert_eq!(vec![Note::NothingCounted { about: "a.json".to_owned() }],
                determine_comparison_notes(&empty, &reading("b.json", "3.0.0", Vec::new()), &config, Vec::new(), true));

        let mut this_run = reading("ignored-name", "3.0.0", Vec::new());
        this_run.source = Source::Run;
        this_run.result.files_present.relevant_files = 0;
        this_run.faulty_files_count = 3;
        assert!(determine_comparison_notes(&reading("a.json", "3.0.0", Vec::new()), &this_run, &config, Vec::new(), true).is_empty());

        // A document that never recorded file rows is said, and one whose rows a cap cut is said
        // with the count, both only when this run asked for file rows
        config.view.by_file = Some(ByFile::All);
        let mut unrecorded = reading("a.json", "3.0.0", Vec::new());
        unrecorded.files_recorded = false;
        assert_eq!(vec![Note::FilesNotRecorded { about: "a.json".to_owned() }],
                determine_comparison_notes(&unrecorded, &reading("b.json", "3.0.0", Vec::new()), &config, Vec::new(), true));
        assert!(!unrecorded.can_pair_files());
        let mut capped = reading("a.json", "3.0.0", Vec::new());
        capped.files_hidden = 7;
        assert_eq!(vec![Note::FilesCut { about: "a.json".to_owned(), hidden: 7 }],
                determine_comparison_notes(&capped, &reading("b.json", "3.0.0", Vec::new()), &config, Vec::new(), true));
        assert!(!capped.can_pair_files());
        config.view.by_file = None;
        let mut unrecorded = reading("a.json", "3.0.0", Vec::new());
        unrecorded.files_recorded = false;
        assert!(determine_comparison_notes(&unrecorded, &reading("b.json", "3.0.0", Vec::new()), &config, Vec::new(), true).is_empty());
    }

    // Every combination is written out because the answer is a guard over an or-pattern, where the
    // case of two documents is the one that quietly goes wrong.
    #[test]
    fn a_documents_settings_reach_a_side_that_is_counted_and_no_other() {
        let dir = crate::paths::test_paths::SCRATCH_DIR.to_owned() + "diff-plan-sources/";
        std::fs::create_dir_all(&dir).unwrap();
        let document = |name: &str, model: mezura_core::CountingModel| {
            let path = format!("{dir}{name}.json");
            let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
            config.view.counting = model;
            let per_language = hashmap!["Rust".to_owned() => stats(100, 70, 2)];
            let result = mezura_core::RunResult {
                total: Stats::total_of(&per_language), per_language, modules: Vec::new(), nested_languages: HashMap::new(),
                faulty_files: Vec::new(), minified_files: 0, generated_files: 0, targets: Vec::new(), unreadable_dirs: Vec::new(),
                files_present: mezura_core::FilesPresent {total_files: 2, relevant_files: 2, excluded_files: 0},
                performance: mezura_core::Performance {duration_millis: 0, threads: mezura_core::Threads::new(1, 1)}
            };
            std::fs::write(&path, crate::json_printer::create_document(&result, &chrono::Local::now(), &config)).unwrap();
            path
        };
        let with_region = document("with-region", mezura_core::CountingModel::Region);
        let without = document("without-region", mezura_core::CountingModel::Content);

        // 'counting' is content on a fresh configuration, so a document that recorded region is a
        // real difference, and every case below asks whether that difference travels
        let adopted_by = |operand: &str| {
            let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
            config.view.diff_against = Some(operand.to_owned());
            let request = DiffRequest::of(&mut config, &[]).unwrap().unwrap();
            let notes = match request {
                DiffRequest::BetweenTwoReadings(x) => x.notes_so_far,
                DiffRequest::AgainstThisRun(x) => x.notes_so_far
            };
            (notes, config.view.counting)
        };

        for operand in [with_region.clone(), format!("{with_region}..HEAD"), format!("HEAD..{with_region}")] {
            let (notes, counting) = adopted_by(&operand);
            assert_eq!(vec![Note::SettingsAdopted { from: "with-region.json".to_owned(),
                    settings: vec!["counting"] }], notes, "nothing was adopted for '{operand}'");
            assert_eq!(mezura_core::CountingModel::Region, counting,
                    "the value was reported as adopted and not applied, for '{operand}'");
        }

        // Two documents: both sets of numbers are already fixed, so nothing is adopted. Both orders,
        // because only the one whose first side carries the difference can tell a working guard
        // from a missing one.
        for operand in [format!("{with_region}..{without}"), format!("{without}..{with_region}")] {
            let (notes, counting) = adopted_by(&operand);
            assert!(notes.is_empty(), "a document was overridden by another document, for '{operand}': {notes:?}");
            assert_eq!(mezura_core::CountingModel::Content, counting);
        }

        let (notes, counting) = adopted_by("HEAD~1..HEAD");
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(mezura_core::CountingModel::Content, counting);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_baseline_that_cannot_pair_files_turns_the_collecting_off_before_the_scan() {
        let dir = crate::paths::test_paths::SCRATCH_DIR.to_owned() + "diff-collect-files/";
        std::fs::create_dir_all(&dir).unwrap();
        let per_language = hashmap!["Rust".to_owned() => stats(100, 70, 2)];
        let mut result = RunResult {
            total: Stats::total_of(&per_language), per_language: per_language.clone(), modules: Vec::new(),
            nested_languages: HashMap::new(), faulty_files: Vec::new(), minified_files: 0, generated_files: 0,
            targets: Vec::new(), unreadable_dirs: Vec::new(),
            files_present: mezura_core::FilesPresent {total_files: 2, relevant_files: 2, excluded_files: 0},
            performance: mezura_core::Performance {duration_millis: 0, threads: mezura_core::Threads::new(1, 1)}
        };
        let write = |name: &str, result: &RunResult, config: &crate::config_manager::Configuration| {
            let path = format!("{dir}{name}.json");
            std::fs::write(&path, crate::json_printer::create_document(result, &chrono::Local::now(), config)).unwrap();
            path
        };
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let without_rows = write("without-rows", &result, &config);

        config.view.by_file = Some(ByFile::All);
        result.modules = vec![ModuleResult { name: None, per_language: per_language.clone(),
                total: Stats::total_of(&per_language), nested_languages: HashMap::new(),
                files: hashmap!["Rust".to_owned() => vec![entry("D:/p/a.rs", 100, 70)]] }];
        let with_rows = write("with-rows", &result, &config);

        let collects_against = |baseline: &str| {
            let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
            config.view.by_file = Some(ByFile::All);
            config.engine.collect_files = true;
            config.view.diff_against = Some(baseline.to_owned());
            DiffRequest::of(&mut config, &[]).unwrap().unwrap();
            config.engine.collect_files
        };
        assert!(!collects_against(&without_rows), "the run still collects what the gate will discard");
        assert!(collects_against(&with_rows));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_settings_the_two_readings_were_taken_under_are_compared() {
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let per_language = hashmap!["Rust".to_owned() => stats(100, 70, 2)];
        let result = mezura_core::RunResult {
            total: Stats::total_of(&per_language), per_language, modules: Vec::new(), nested_languages: HashMap::new(),
            faulty_files: Vec::new(), minified_files: 0, generated_files: 0, targets: Vec::new(), unreadable_dirs: Vec::new(),
            files_present: mezura_core::FilesPresent {total_files: 2, relevant_files: 2, excluded_files: 0},
            performance: mezura_core::Performance {duration_millis: 0, threads: mezura_core::Threads::new(1, 1)}
        };
        let document = crate::json_reader::parse(&crate::json_printer::create_document(&result,
                &chrono::Local::now(), &crate::config_manager::Configuration::new(vec!["./src".to_owned()]))).unwrap();
        let content = mezura_core::CountingModel::Content;
        assert!(find_settings_that_differ(&document.scope, &scope_of(&config.engine, content)).is_empty());

        config.engine.exclude_dirs = vec!["target".to_owned()];
        assert_eq!(vec!["exclude"], find_settings_that_differ(&document.scope, &scope_of(&config.engine, content)));

        // A forced language decides which language a file is counted as, so a run that forced one
        // and a run that did not measured different things
        config.engine.exclude_dirs = Vec::new();
        config.engine.forced_languages = hashmap!["m".to_owned() => "matlab".to_owned()].into();
        assert_eq!(vec!["force-language"], find_settings_that_differ(&document.scope, &scope_of(&config.engine, content)));

        config.engine.forced_languages = ForcedLanguages::default();
        config.engine.no_gitignore = true;
        assert_eq!(vec!["no-gitignore"], find_settings_that_differ(&document.scope,
                &scope_of(&config.engine, content)));

        // The counting model is not among them: both sides are folded by the model on screen
        config.engine.no_gitignore = false;
        assert!(find_settings_that_differ(&document.scope,
                &scope_of(&config.engine, mezura_core::CountingModel::Region)).is_empty());

        // Hiding the keywords is among them: it moves no line or code count, but a side that did
        // not count keywords would read as every keyword having been written since
        config.engine.count_keywords = false;
        assert_eq!(vec!["hide keywords"], find_settings_that_differ(&document.scope, &scope_of(&config.engine, content)));
    }

    #[test]
    fn a_documents_settings_are_taken_unless_the_command_line_set_its_own() {
        let document = Scope {
            exclude: vec!["target".to_owned()],
            languages: Vec::new(),
            excluded_languages: Vec::new(),
            forced_languages: HashMap::new(),
            counting: "region".to_owned(),
            search_in_dotted: false,
            gitignore: false,
            ignore_files: true,
            keywords_counted: true,
            count_minified: false,
            count_generated: false
        };

        // Nothing typed: what differs is taken, what agrees is not reported
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let adopted = resolve_settings(&document, &mut config);
        assert_eq!(vec!["exclude", "counting", "no-gitignore"], adopted);
        assert_eq!(vec!["target".to_owned()], config.engine.exclude_dirs);
        assert_eq!(mezura_core::CountingModel::Region, config.view.counting);
        // recorded as "the file was not obeyed", so the flag turns on
        assert!(config.engine.no_gitignore);
        assert!(resolve_settings(&document, &mut config).is_empty());

        // The same difference with the value typed stays as typed, and is not reported as taken
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.typed_explicitly.counting = true;
        config.typed_explicitly.exclude = true;
        assert_eq!(vec!["no-gitignore"], resolve_settings(&document, &mut config));
        assert_eq!(mezura_core::CountingModel::Content, config.view.counting);
        assert!(config.engine.exclude_dirs.is_empty());

        // A model this build does not have, which a document of a later version can name, cannot be
        // imitated, so this run keeps the one it has
        let unknown = Scope { counting: "some-later-model".to_owned(), gitignore: true,
                exclude: Vec::new(), ..document.clone() };
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        assert!(resolve_settings(&unknown, &mut config).is_empty());
        assert_eq!(mezura_core::CountingModel::Content, config.view.counting);

        // The order two lists were written in is not a difference
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.engine.exclude_dirs = vec!["target".to_owned()];
        config.engine.no_gitignore = true;
        config.view.counting = mezura_core::CountingModel::Region;
        assert!(resolve_settings(&document, &mut config).is_empty());

        let without_keywords = Scope { keywords_counted: false, gitignore: true,
                counting: "content".to_owned(), exclude: Vec::new(), ..document };
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
