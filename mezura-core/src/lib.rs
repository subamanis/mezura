//! Counts the lines of a codebase: which language every file is written in, and how many of its
//! lines are code, comments and neither.
//!
//! A run takes two things. [`EngineConfig`] says what to count, and [`Languages`] says what the
//! symbols of each language are. The second is built against the first and refuses to be used with
//! any other, since counting Rust with settings that name Python would give figures that look
//! perfectly normal and describe something else.
//!
//! ```no_run
//! use mezura_core::{CountingModel, EngineConfig, Languages, run};
//!
//! let config = EngineConfig::new(["./src", "./tests"]);
//! let (languages, warnings) = Languages::shipped(&config);
//! for warning in &warnings {
//!     eprintln!("{}", warning.message);
//! }
//!
//! let result = run(&config, languages)?;
//! for (name, stats) in result.sort_languages_by(Default::default(), CountingModel::Content) {
//!     println!("{name}: {} code", stats.calculate_code_lines(CountingModel::Content));
//! }
//! # Ok::<(), mezura_core::RunError>(())
//! ```
//!
//! The counting never decides what a comment column shows. It sorts every line into one of the
//! nine [`LineClasses`], and a [`CountingModel`] folds those nine into the three columns of a
//! report when the figures are read, so one run answers both models.
//!
//! [`run_watched`] is the same run for a caller that needs real time feedback while it happens, and
//! [`explain_file`] reads a single file line by line and says why each line was counted the way it
//! was.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(unreachable_pub)]
#![allow(non_snake_case)]

#[cfg(test)]
#[macro_use]
mod test_support;

mod domain;
mod explain;
mod phase_timing;
mod progress;
mod result;

pub mod engine;
pub mod language_file;
pub mod languages;
pub mod render;
pub mod warnings;

pub use domain::{Bucket, CountingModel, Keyword, Language, LeveledPair, LineClass, LineClasses,
        LineContinuation, MultilineString, NestedLanguage, Span, SpanKind, Stats, StringRules};
pub use engine::config::{EngineConfig, ForcedLanguages, LanguageNames, ScopedByModule, Target,
        Threads, format_module_scope, split_off_module_scope};
pub use engine::targets::TargetError;
pub use explain::{Carried, ExplainError, ExplainedLine, FileExplanation, explain_file};
pub use languages::Languages;
pub use progress::ScanProgress;
pub use result::{FaultyFileDetails, FileEntry, FilesPresent, ModuleResult, Performance, RunError,
        RunResult, SortCriterion, UnreadableDirDetails};
pub use warnings::{Affects, Code, Warning};

#[cfg(test)]
pub(crate) use test_support::{languages_claiming, test_paths};

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use crossbeam_deque::{Injector, Worker};

use engine::modules::{ModuleId, Modules};

/// The name of the file that decides which language gets an extension or a file name two of them
/// claim.
///
/// Nothing in this crate reads or writes it: a caller keeps that file wherever it keeps the rest,
/// parses it with [`language_file::parse_conflict_rules_file`] and hands the rules to
/// [`Languages::resolve`]. The name is here because the warning about an unsettled extension is
/// written here and points the reader at the file.
pub const LANGUAGE_CONFLICTS_FILE_NAME : &str = "language_conflicts.txt";
/// The name of the report row holding everything no target was given a name for.
pub const UNNAMED_MODULE_NAME : &str = "(unnamed)";

pub(crate) type FaultyFilesListMut = Arc<Mutex<Vec<FaultyFileDetails>>>;
pub(crate) type SharedModuleLookups = Arc<engine::identity::ModuleLookups>;
// One bucket per module. A run where the user named no modules at all has exactly one bucket, so
// nothing further down has two shapes to handle.
pub(crate) type StatsMapMut = Arc<Mutex<Vec<HashMap<String,Stats>>>>;
pub(crate) type NestedLanguageMapMut = Arc<Mutex<Vec<HashMap<String,HashMap<String,Stats>>>>>;
pub(crate) type FilesPerModuleMut = Arc<Mutex<Vec<HashMap<String, Vec<FileEntry>>>>>;

/// Counts the directories and files the configuration names, and gives back the figures.
///
/// The languages must have been resolved against this same configuration, and the run refuses the
/// pair otherwise: resolving is what applies the chosen and excluded languages and the forced
/// extensions, so an ill-matched pair would count one set of languages while the settings describe
/// another.
///
/// Blocks until everything has been counted. Failing to read some of the files is not an error, and
/// comes back in [`RunResult::faulty_files`]; the cases that are one are [`RunError`].
pub fn run(config: &EngineConfig, languages: Languages) -> Result<RunResult, RunError> {
    run_watched(config, languages, None, |_| {})
}

/// The same run, for a caller that needs real time feedback while it happens.
///
/// The progress counters move as files are found and parsed, so a thread of the caller's can read
/// them while this one blocks.
///
/// `on_traversal_done` is called once, as soon as the directories have been scanned, with the files
/// that were found; the counting of those files is still going on at that point. It is called on
/// every run that returns `Ok`, including one that found nothing, and never on a run whose scanning
/// thread died, because the figures such a run leaves behind are lower than what is really on disk.
pub fn run_watched(config: &EngineConfig, languages: Languages, progress: Option<Arc<ScanProgress>>,
        on_traversal_done: impl FnOnce(FilesPresent)) -> Result<RunResult, RunError>
{
    let progress = progress.unwrap_or_default();
    // Guarded rather than raised on each return: 'run' refuses in six places before the walk ever
    // starts, and a watcher of the public flag must see it rise on every one of them.
    let _walk_ends = WalkDoneGuard(progress.clone());
    if config.targets.is_empty() {
        return Err(RunError::NoTargets);
    }
    // Checked before anything is read from disk.
    if !languages.describe_the_same_selection_as(config) {
        return Err(RunError::LanguagesFromAnotherConfig);
    }
    // Idempotent, so a caller that resolved its own targets earlier loses nothing here.
    let targets = engine::targets::resolve(&config.targets, ObeyedIgnoreFiles::of(config),
            config.should_search_in_dotted).map_err(RunError::InvalidTargets)?;
    let config = Arc::new(config.clone());
    let faulty_files_ref : FaultyFilesListMut  = Arc::new(Mutex::new(Vec::with_capacity(10)));
    let finish_condition_ref = Arc::new(AtomicBool::new(false));
    let (by_name, lookups, nested_definitions) = languages.into_parts();
    let language_map_ref = Arc::new(by_name);
    let nested_definitions = Arc::new(nested_definitions);
    let modules = Arc::new(Modules::of(&targets));
    // Only here can the lookups be put in the order the walk wants them: which number a module was
    // given is decided by the targets, and the languages were resolved before they were seen.
    let language_lookups: SharedModuleLookups = Arc::new(lookups.into_lookups_per_module(&modules));
    let stats_per_module : StatsMapMut =
            Arc::new(Mutex::new(make_language_stats(&language_map_ref, modules.count())));
    let nested_per_module : NestedLanguageMapMut =
            Arc::new(Mutex::new(vec![HashMap::new(); modules.count()]));
    let files_per_module : FilesPerModuleMut =
            Arc::new(Mutex::new(vec![HashMap::new(); modules.count()]));

    let mut files_present = FilesPresent::default();
    let idle_producers = Arc::new(AtomicUsize::new(0));
    let files_injector = Arc::new(Injector::<ParsableFile>::new());
    let dirs_injector = Arc::new(Injector::<TraversedDir>::new());
    let exclude_matcher = Arc::new(engine::targets::build_exclude_matcher(&config.exclude_dirs)
            .map_err(|_| {
                // The builder rewrites every pattern into a longer form before compiling it, and its
                // error quotes that rewritten text, which the user never typed. Trying them one at a
                // time finds the broken one, so the error can quote it as it was written.
                let culprit = config.exclude_dirs.iter()
                        .find(|x| engine::targets::build_exclude_matcher(std::slice::from_ref(x)).is_err())
                        .cloned().unwrap_or_default();
                RunError::InvalidExcludePattern(culprit)
            })?);
    queue_the_targets(&config, &targets, &dirs_injector, &files_injector, &mut files_present,
            &language_lookups, &modules, &progress);

    let files_stats = Arc::new(Mutex::new(files_present));
    let unreadable_dirs = Arc::new(Mutex::new(Vec::new()));

    let mut producer_handles = Vec::with_capacity(config.threads.producers());
    let mut consumer_handles = Vec::with_capacity(config.threads.consumers());
    // Producers stop when the idle count reaches this, so until every spawn is done it holds a value
    // that count can never reach: against a total still growing, the first producer to go idle would
    // see itself as the last one standing.
    let producers_total = Arc::new(AtomicUsize::new(usize::MAX));
    let worker_panics: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    // A thread the operating system refuses is a slower run and not a different answer, so the run
    // carries on with what it was given. Zero of either side is the exception, below.
    let parsing_started_instant = Instant::now();
    let mut last_refusal = None;
    for i in 0..config.threads.producers() {
        match engine::producer::start_producer_thread(i, files_injector.clone(), dirs_injector.clone(), Worker::new_fifo(),
                idle_producers.clone(), language_lookups.clone(), exclude_matcher.clone(),
                config.clone(), files_stats.clone(), modules.clone(), unreadable_dirs.clone(),
                producers_total.clone(), worker_panics.clone(), progress.clone()) {
            Ok(handle) => producer_handles.push(handle),
            Err(x) => last_refusal = Some(x)
        }
    }
    if producer_handles.is_empty() {
        return Err(RunError::NoThreadsAvailable { side: "producer", error: last_refusal.unwrap() });
    }
    producers_total.store(producer_handles.len(), Ordering::SeqCst);

    // Written by whichever consumer stops last, read once they have all been joined.
    let counting_ended = Arc::new(AtomicU64::new(0));
    // Decided by the counting and not by the walk, so they are read after the joins below
    let minified_files = Arc::new(AtomicUsize::new(0));
    let generated_files = Arc::new(AtomicUsize::new(0));
    for i in 0..config.threads.consumers() {
        match engine::consumer::start_parser_thread(i, files_injector.clone(), faulty_files_ref.clone(), finish_condition_ref.clone(),
                stats_per_module.clone(), nested_per_module.clone(), files_per_module.clone(),
                language_map_ref.clone(), nested_definitions.clone(), config.clone(),
                parsing_started_instant, counting_ended.clone(), minified_files.clone(),
                generated_files.clone(), progress.clone()) {
            Ok(handle) => consumer_handles.push(handle),
            Err(x) => last_refusal = Some(x)
        }
    }
    if consumer_handles.is_empty() {
        // Joined so that no thread outlives the call that started it.
        for handle in producer_handles {
            let _ = handle.join();
        }
        return Err(RunError::NoThreadsAvailable { side: "consumer", error: last_refusal.unwrap() });
    }

    let threads_used = Threads::new(producer_handles.len(), consumer_handles.len());
    for handle in producer_handles {
        let _ = handle.join();
    }
    // After the join and not before it, which reads as the more accurate place: a watcher starts
    // timing the counting the moment this flag rises, and only here is nothing else left competing
    // for the cores. Raised earlier, the pace it measures comes out low.
    progress.mark_walk_done();
    let producers_done_millis = parsing_started_instant.elapsed().as_millis();

    let queued_at_producer_exit = files_injector.len();

    finish_condition_ref.store(true,Ordering::Relaxed);

    // The callback goes below the flag above and never above it. It is the caller's code, it may
    // panic, and a panic here unwinds past the joins with the consumers still running: they leave
    // their loop only on that flag, so raising it first is what lets them finish instead of spinning
    // forever.
    //
    // A producer that died merged none of its share, so these counters are short of what is on disk
    // and the run is about to refuse them anyway. Announcing them would put a number on screen that
    // the error two steps down contradicts.
    //
    // Poisoning is tolerated on both locks because this sits above the guard that turns a dead worker
    // into an error: panicking here would report a mutex instead of what actually happened.
    let walk_was_whole = worker_panics.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_empty();
    let files_present = *files_stats.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if walk_was_whole {
        on_traversal_done(files_present);
    }

    for handle in consumer_handles {
        if let Err(payload) = handle.join() {
            worker_panics.lock().unwrap().push(panic_message(payload.as_ref()));
        }
    }
    // From the consumers and not from the clock here, which cannot tell the counting apart from the
    // callback: both run on their own side of this thread, and it holds whichever finished last.
    //
    // The floor matters for a run whose consumers all died and recorded nothing. That is an error
    // two steps down and should stay one rather than becoming an underflow in the line below.
    let parsing_duration_millis = u128::from(counting_ended.load(Ordering::Relaxed)).max(producers_done_millis);

    if *phase_timing::ENABLED {
        eprintln!("[phase] producers alive: {} ms | drain after producers: {} ms | queue size at producer exit: {}",
            producers_done_millis, parsing_duration_millis - producers_done_millis, queued_at_producer_exit);
        eprintln!("{}", phase_timing::report(threads_used.consumers(), parsing_duration_millis));
    }

    // Ahead of every lock below, so that a dead worker is reported as itself rather than as whichever
    // mutex it poisoned. Nothing past this line runs unless every worker finished whole, which is
    // what leaves those locks clean.
    let worker_panics = std::mem::take(&mut *worker_panics.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
    if !worker_panics.is_empty() {
        return Err(RunError::IncompleteRun { worker_panic: worker_panics.join(" | ") });
    }

    let minified_files = minified_files.load(Ordering::Relaxed);
    let generated_files = generated_files.load(Ordering::Relaxed);
    let relevant_files_num = files_present.relevant_files;
    if relevant_files_num == 0 {
        return Ok(RunResult::of_nothing(files_present,
                Performance { duration_millis: parsing_duration_millis, threads: threads_used }, &modules,
                targets.to_vec(), std::mem::take(&mut unreadable_dirs.lock().unwrap())));
    }

    let mut stats_guard = stats_per_module.lock();
    let per_module = stats_guard.as_deref_mut().unwrap();
    let mut nested_guard = nested_per_module.lock();
    let nested_by_module = nested_guard.as_deref_mut().unwrap();
    let mut files_guard = files_per_module.lock();
    let files_by_module = files_guard.as_deref_mut().unwrap();

    let mut per_language = merge_over_modules(per_module);
    // Dropped before the total is summed, or the total's keyword map would name the keywords of
    // every language the run selected, including the ones no file was written in. The figures are
    // the same either way, since an empty language adds nothing.
    remove_languages_with_0_files(&mut per_language);
    let total = Stats::total_of(&per_language);

    let nested_languages = merge_nested_over_modules(nested_by_module);

    let modules_result = per_module.iter_mut().enumerate().map(|(id, bucket)| {
        let mut of_this_module = std::mem::take(bucket);
        remove_languages_with_0_files(&mut of_this_module);
        // A module that found nothing still gets its row: it was asked for by name, and its absence
        // would read as a mistake in the report.
        ModuleResult {
            name: modules.name_of(id as ModuleId).map(str::to_owned),
            total: Stats::total_of(&of_this_module),
            per_language: of_this_module,
            nested_languages: std::mem::take(&mut nested_by_module[id]),
            files: std::mem::take(&mut files_by_module[id])
        }
    }).collect::<Vec<_>>();

    Ok(RunResult {
        per_language,
        total,
        nested_languages,
        modules: modules_result,
        faulty_files: std::mem::take(&mut faulty_files_ref.lock().unwrap()),
        minified_files,
        generated_files,
        files_present,
        performance: Performance { duration_millis: parsing_duration_millis, threads: threads_used },
        targets: targets.to_vec(),
        unreadable_dirs: std::mem::take(&mut unreadable_dirs.lock().unwrap())
    })
}

/// Whether this run will print a report of where its time went to the error output, which the
/// `MEZURA_PHASE_TIMING` environment variable asks for.
///
/// Worth asking before drawing live lines of your own on the error output, so the two do not land
/// on top of each other.
pub fn prints_phase_timing() -> bool {
    *phase_timing::ENABLED
}

struct WalkDoneGuard(Arc<ScanProgress>);

impl Drop for WalkDoneGuard {
    fn drop(&mut self) {
        self.0.mark_walk_done();
    }
}

// Fills the two queues the threads work from: a target that is a single file goes straight onto the
// file queue, a directory is put in the queue for a scanning thread to descend into.
//
// Only the outermost targets are queued. One that sits inside another is reached by the scan of the
// one around it, and queueing both would count its files twice; the name it was given is not lost
// with it, the module table still hands it back on the way down.
pub(crate) fn queue_the_targets(config: &EngineConfig, targets: &engine::targets::Targets,
        dirs_injector: &Arc<Injector<TraversedDir>>, files_injector: &Arc<Injector<ParsableFile>>,
        files_present: &mut FilesPresent, language_lookups: &engine::identity::ModuleLookups, modules: &Modules,
        progress: &ScanProgress)
{
    for target in crate::engine::targets::topmost_targets(targets) {
        let dir_path = Path::new(&target.path);
        let module = modules.of_target(&target);
        if dir_path.is_file() {
            let Some(lang_name) = language_lookups.get_of_module(module).of_path_or_shebang(dir_path) else {
                continue;
            };
            let queued = match targets.was_written_by_hand(dir_path) {
                true => ParsableFile::written_by_hand(dir_path.to_path_buf(), lang_name, module),
                false => ParsableFile::new(dir_path.to_path_buf(), lang_name, module)
            };
            files_injector.push(queued);
            files_present.total_files += 1;
            files_present.relevant_files += 1;
            progress.record_file_found();
        } else if dir_path.is_dir() {
            let gitignore_stack = GitignoreStack::for_root_dir(dir_path, ObeyedIgnoreFiles::of(config));
            dirs_injector.push(TraversedDir::new(dir_path.to_path_buf(), gitignore_stack, module));
        }
    }
}

// A language nobody wrote a file in would take a row in every report and add nothing to any figure.
pub(crate) fn remove_languages_with_0_files(languages: &mut HashMap<String,Stats>) {
    languages.retain(|_, stats| stats.files > 0);
}

// A bucket for every language in every module, built up front: the merge that ends a consumer
// reaches into this map by name, and a pair with no slot would kill the thread rather than miscount.
pub(crate) fn make_language_stats(languages: &HashMap<String,Language>, modules: usize) -> Vec<HashMap<String,Stats>> {
    let of_one_module = languages.iter().map(|(name, language)| (name.to_owned(), Stats::from(language)))
            .collect::<HashMap<_,_>>();
    vec![of_one_module; modules]
}

#[derive(Debug,Clone)]
pub(crate) struct ParsableFile {
    pub path: PathBuf,
    pub language_name: Arc<str>,
    pub module: ModuleId,
    // Named as a target rather than found by the walk, which is what exempts it from every rule
    // that skips a file: the ignore files, the dotted names, and being minified or generated
    pub written_by_hand: bool
}

impl ParsableFile {
    pub(crate) fn new(path: PathBuf, language_name: Arc<str>, module: ModuleId) -> Self {
        ParsableFile {
            path,
            language_name,
            module,
            written_by_hand: false
        }
    }

    pub(crate) fn written_by_hand(path: PathBuf, language_name: Arc<str>, module: ModuleId) -> Self {
        ParsableFile { written_by_hand: true, ..ParsableFile::new(path, language_name, module) }
    }
}

#[derive(Debug,Clone)]
pub(crate) struct TraversedDir {
    pub path: PathBuf,
    pub gitignore_stack: Option<Arc<GitignoreStack>>,
    pub module: ModuleId
}

impl TraversedDir {
    pub(crate) fn new(path: PathBuf, gitignore_stack: Option<Arc<GitignoreStack>>, module: ModuleId) -> Self {
        TraversedDir {
            path,
            gitignore_stack,
            module
        }
    }
}

// Which of the ignore files a walk obeys. Two answers rather than one, because a '.gitignore' is
// the repository's decision and a '.ignore' is the decision of whoever set up their search tools,
// and a vendored dependency is routinely kept by the first and hidden by the second.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ObeyedIgnoreFiles {
    pub gitignore: bool,
    pub search_tools: bool
}

impl ObeyedIgnoreFiles {
    pub(crate) fn of(config: &EngineConfig) -> ObeyedIgnoreFiles {
        ObeyedIgnoreFiles { gitignore: !config.no_gitignore, search_tools: !config.no_ignore_files }
    }

    pub(crate) fn obeys_nothing(self) -> bool {
        !self.gitignore && !self.search_tools
    }

    // In the order they overrule each other, which is the order they are read in: the last rule
    // that matches is the one that answers, so a '!keep' in '.rgignore' stands against an entry in
    // '.gitignore' however the two files are written. That is the order ripgrep reads them in.
    fn get_file_names(self) -> impl Iterator<Item = &'static str> {
        [(".gitignore", self.gitignore), (".ignore", self.search_tools), (".rgignore", self.search_tools)]
                .into_iter().filter_map(|(name, obeyed)| obeyed.then_some(name))
    }
}

// The ignore files that apply at one depth, innermost first, each linked to the one above it. The
// walk extends the chain as it descends so no directory reparses its parents' rules. One matcher
// per directory holds all of that directory's files together, which is what gives them their order.
#[derive(Debug)]
pub(crate) struct GitignoreStack {
    matcher: ignore::gitignore::Gitignore,
    parent: Option<Arc<GitignoreStack>>
}

impl GitignoreStack {
    pub(crate) fn extend_with_dir(dir: &Path, parent: Option<Arc<GitignoreStack>>, obeyed: ObeyedIgnoreFiles)
    -> Option<Arc<GitignoreStack>>
    {
        let mut builder = ignore::gitignore::GitignoreBuilder::new(dir);
        let mut found_one = false;
        for name in obeyed.get_file_names() {
            let path = dir.join(name);
            if path.is_file() {
                // Ignored the way 'Gitignore::new' ignores it: a file that could not be read, or a
                // pattern that does not parse, costs that one rule and not the whole walk
                let _ = builder.add(&path);
                found_one = true;
            }
        }
        if !found_one {
            return parent;
        }

        match builder.build() {
            Ok(matcher) if !matcher.is_empty() => Some(Arc::new(GitignoreStack { matcher, parent })),
            _ => parent
        }
    }

    // The ignore files of every dir between the repository root and the given dir, excluding it
    fn of_ancestors(dir: &Path, obeyed: ObeyedIgnoreFiles) -> Option<Arc<GitignoreStack>> {
        if dir.join(".git").exists() {
            return None;
        }

        let mut relevant_ancestors: Vec<&Path> = Vec::new();
        for ancestor in dir.ancestors().skip(1) {
            relevant_ancestors.push(ancestor);
            if ancestor.join(".git").exists() {
                break;
            }
        }

        let mut stack = None;
        for ancestor in relevant_ancestors.iter().rev() {
            stack = Self::extend_with_dir(ancestor, stack, obeyed);
        }
        stack
    }

    // Explicitly given target dirs are traversed even if an ignore file of their ancestors ignores them
    pub(crate) fn for_root_dir(dir: &Path, obeyed: ObeyedIgnoreFiles) -> Option<Arc<GitignoreStack>> {
        if obeyed.obeys_nothing() {
            return None;
        }
        let stack = Self::of_ancestors(dir, obeyed);
        if let Some(s) = &stack && s.is_ignored(dir, true) {
            return None;
        }
        stack
    }

    // Used for paths that the program discovered on its own, like the matches of a glob pattern
    pub(crate) fn is_path_ignored(path: &Path, obeyed: ObeyedIgnoreFiles) -> bool {
        if obeyed.obeys_nothing() {
            return false;
        }
        let is_dir = path.is_dir();
        let Some(parent) = path.parent() else { return false };

        let stack = Self::extend_with_dir(parent, Self::of_ancestors(parent, obeyed), obeyed);
        match stack {
            Some(x) => x.is_ignored_with_ancestor_dirs(path, is_dir),
            None => false
        }
    }

    // Unlike the traversal, which prunes ignored dirs as it descends and therefore only has to
    // check the entry itself, a standalone path has to be checked against its parent dirs too
    fn is_ignored_with_ancestor_dirs(&self, path: &Path, is_dir: bool) -> bool {
        let mut node = Some(self);
        while let Some(stack) = node {
            match stack.matcher.matched_path_or_any_parents(path, is_dir) {
                ignore::Match::Ignore(_) => return true,
                ignore::Match::Whitelist(_) => return false,
                ignore::Match::None => {}
            }
            node = stack.parent.as_deref();
        }

        false
    }

    pub(crate) fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        let mut node = Some(self);
        while let Some(stack) = node {
            match stack.matcher.matched(path, is_dir) {
                ignore::Match::Ignore(_) => return true,
                ignore::Match::Whitelist(_) => return false,
                ignore::Match::None => {}
            }
            node = stack.parent.as_deref();
        }

        false
    }
}

fn merge_over_modules(per_module: &[HashMap<String,Stats>]) -> HashMap<String,Stats> {
    let mut merged = per_module[0].clone();
    for of_a_module in &per_module[1..] {
        for (name, stats) in of_a_module {
            merged.entry(name.clone()).or_default().add(stats);
        }
    }

    merged
}

fn merge_nested_over_modules(nested_by_module: &[HashMap<String, HashMap<String, Stats>>])
-> HashMap<String, HashMap<String, Stats>> {
    let mut merged: HashMap<String, HashMap<String, Stats>> = HashMap::new();
    for bucket in nested_by_module {
        for (shell_name, sections) in bucket {
            let shell_entry = merged.entry(shell_name.clone()).or_default();
            for (inner_name, stats) in sections {
                shell_entry.entry(inner_name.clone()).or_default().add(stats);
            }
        }
    }

    merged
}

// A panic's payload as text. 'panic!' with a literal carries a '&str' and everything formatted
// carries a 'String'; anything else is somebody's own type and has no text to give.
pub(crate) fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&'static str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "a worker died with a panic payload that is not text".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Asserted here and not through a run, since a result has had the empty languages removed from
    // it by then.
    #[test]
    fn every_language_gets_a_bucket_in_every_module() {
        let languages = languages_claiming(&[("Rust", &["rs"]), ("Go", &["go"]), ("Zig", &["zig"])]);
        let modules = Modules::of(&[crate::engine::config::Target::named("backend", "./api"),
                crate::engine::config::Target::named("frontend", "./web"),
                crate::engine::config::Target::of("./docs")]);
        assert_eq!(3, modules.count());

        let stats = make_language_stats(&languages, modules.count());
        assert_eq!(modules.count(), stats.len());

        for (id, of_a_module) in stats.iter().enumerate() {
            for name in languages.keys() {
                assert!(of_a_module.contains_key(name), "'{name}' has no bucket in module {id}");
            }
        }
    }

    #[test]
    fn the_total_is_the_languages_added_together() {
        let counted = |code, comments| LineClasses {
                words_in_code: code, words_in_comment: comments, ..Default::default() };
        let languages = hashmap![
            "a".to_owned() => Stats::new(20, 100_000, 2000, counted(1400, 100), hashmap!["classes".to_owned() => 7]),
            "b".to_owned() => Stats::new(10, 50_000, 1000, counted(800, 50), hashmap!["classes".to_owned() => 2]),
            "c".to_owned() => Stats::new(10, 50_000, 1000, counted(800, 50), hashmap!["structs".to_owned() => 5])
        ];
        let total = Stats::total_of(&languages);

        assert_eq!(40, total.files);
        assert_eq!(200_000, total.bytes);
        assert_eq!(4000, total.lines);
        assert_eq!(3000, total.calculate_code_lines(CountingModel::Content));
        assert_eq!(200, total.calculate_comment_lines(CountingModel::Content));
        assert_eq!(800, total.calculate_extra_lines(CountingModel::Content));
        assert_eq!(5000, total.calculate_average_size());
        // 'classes' is declared by two of the three, so its total is 7 + 2
        assert_eq!(Some(&9), total.keyword_occurences.get("classes"));
        assert_eq!(Some(&5), total.keyword_occurences.get("structs"));

        // nothing to add up is a total of nothing; 'average_size' over no files is asserted in 'domain'
        assert_eq!(0, Stats::total_of(&HashMap::new()).files);
    }
}

// What 'run' owes its caller when a worker thread dies: an error, never a number it knows is short.
// The two hooks that cause the deaths fire on the corpus names used here and on nothing else.
#[cfg(test)]
mod worker_death_tests {
    use crate::{EngineConfig, Languages, RunError, run, run_watched};

    fn corpus(name: &str) -> (std::path::PathBuf, EngineConfig) {
        let root = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.rs"), "fn a() { let x = 1; }\n").unwrap();
        let config = EngineConfig {
            threads: crate::Threads::new(2, 2),
            ..EngineConfig::new([root.to_string_lossy().replace('\\', "/")])
        };
        (root, config)
    }

    fn languages_for(config: &EngineConfig) -> Languages {
        let languages = crate::language_file::parse_languages_in_dir(crate::test_paths::LANGUAGES_DIR).unwrap().0;
        Languages::resolve(config, languages, &Default::default()).0
    }

    #[test]
    fn a_dead_consumer_is_an_error_and_not_a_short_count() {
        let (root, config) = corpus("mezura-dead-consumer");

        let err = run(&config, languages_for(&config));
        std::fs::remove_dir_all(&root).unwrap();
        let (clean_root, clean_config) = corpus("mezura-alive-consumer");
        let clean = run(&clean_config, languages_for(&clean_config));
        std::fs::remove_dir_all(&clean_root).unwrap();

        let err = err.expect_err("a consumer died and run returned a result anyway");
        assert!(matches!(&err, RunError::IncompleteRun { worker_panic } if worker_panic.contains("test-induced consumer panic")),
                "got: {err:?}");
        // and the hook answers to that corpus name alone, so an ordinary run is untouched
        assert_eq!(1, clean.unwrap().total.files);
    }

    #[test]
    fn a_dead_producer_is_an_error_and_the_run_still_terminates() {
        let (root, config) = corpus("mezura-dead-producer");

        let err = run(&config, languages_for(&config));
        std::fs::remove_dir_all(&root).unwrap();
        let (clean_root, clean_config) = corpus("mezura-alive-producer");
        let clean = run(&clean_config, languages_for(&clean_config));
        std::fs::remove_dir_all(&clean_root).unwrap();

        let err = err.expect_err("a producer died and run returned a result anyway");
        assert!(matches!(&err, RunError::IncompleteRun { worker_panic } if worker_panic.contains("test-induced producer panic")),
                "got: {err:?}");
        assert_eq!(1, clean.unwrap().total.files);
    }

    // A dead producer takes its share of the walk with it and merges nothing, so the counters left
    // behind are short. The announcement fires before the guard that turns the death into an error,
    // so it is the one thing that could put a wrong number on the screen a moment before the run
    // refuses it.
    #[test]
    fn a_walk_whose_own_thread_died_is_never_announced() {
        let (root, config) = corpus("mezura-dead-producer-announce");
        let mut announced = Vec::new();
        let outcome = run_watched(&config, languages_for(&config), None, |scan| announced.push(scan));
        std::fs::remove_dir_all(&root).unwrap();

        // the hook answers to 'mezura-dead-producer' as a prefix, so this corpus dies the same way
        assert!(matches!(&outcome, Err(RunError::IncompleteRun { .. })), "got: {outcome:?}");
        assert!(announced.is_empty(), "a walk that lost a thread was announced anyway: {announced:?}");

        // the same run with every thread intact does announce, so the guard is the difference
        let (clean_root, clean_config) = corpus("mezura-alive-producer-announce");
        let mut announced = Vec::new();
        let clean = run_watched(&clean_config, languages_for(&clean_config), None, |scan| announced.push(scan));
        std::fs::remove_dir_all(&clean_root).unwrap();
        assert_eq!(1, clean.unwrap().total.files);
        assert_eq!(1, announced.len(), "an intact walk was not announced");
    }

    // The integration test beside this one only reaches the case where the callback is the last
    // thing running. Here the counting outlasts it, which is what every run over a real tree looks
    // like, so nothing should come off the figure: ten files at forty milliseconds through one
    // consumer is four hundred milliseconds of counting under a callback that sleeps a hundred and
    // fifty.
    #[test]
    fn a_callback_that_finishes_before_the_counting_takes_nothing_off_the_duration() {
        const SLEPT_PER_FILE : u128 = 40;
        const FILES : u128 = 10;
        let callback_holds = std::time::Duration::from_millis(150);

        let root = std::env::temp_dir().join("mezura-slow-consumer-clock");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for i in 0..FILES {
            std::fs::write(root.join(format!("f{i}.rs")), "fn a() { let x = 1; }\n").unwrap();
        }
        // One consumer, so the sleeps add up instead of overlapping and the expected floor is
        // arithmetic rather than a guess about how many cores are free
        let config = EngineConfig {
            threads: crate::Threads::new(1, 1),
            ..EngineConfig::new([root.to_string_lossy().replace('\\', "/")])
        };

        let counted = run_watched(&config, languages_for(&config), None,
                |_| std::thread::sleep(callback_holds)).unwrap();
        std::fs::remove_dir_all(&root).unwrap();

        assert_eq!(FILES as usize, counted.total.files);
        let counting_took = SLEPT_PER_FILE * FILES;
        // Only the sleeps are asserted on, never the parsing around them, so a slow machine can only
        // push the figure up and the floor holds wherever this runs
        assert!(counted.performance.duration_millis >= counting_took,
                "{} ms of counting under a callback that held {} ms was reported as {} ms, so the \
                 callback was taken off a run it never delayed", counting_took,
                callback_holds.as_millis(), counted.performance.duration_millis);
    }
}
