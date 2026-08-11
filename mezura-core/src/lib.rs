#![forbid(unsafe_code)]
#![allow(non_snake_case)]

#[cfg(test)]
#[macro_use]
mod test_support;

mod domain;
mod phase_timing;
mod progress;
mod result;

pub mod engine;
pub mod language_file;
pub mod languages;
pub mod render;
pub mod warnings;

pub use domain::{EmbeddedRegion, Keyword, Language, LeveledPair, MultilineString, Stats};
pub use engine::config::{EngineConfig, Target, Threads};
pub use engine::targets::TargetError;
pub use languages::Languages;
pub use progress::ScanProgress;
pub use result::{FaultyFileDetails, FilesPresent, ModuleResult, Performance, RunError, RunResult,
        SortCriterion, UnreadableDirDetails};
pub use warnings::{Affects, Warning};

#[cfg(test)]
pub(crate) use test_support::{languages_claiming, test_paths};

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use crossbeam_deque::{Injector, Worker};

use engine::modules::{ModuleId, Modules};

// The file that decides which language gets an extension that two of them claim. Nothing here reads
// or writes it: the command line creates it in the user's data directory and parses it, and hands
// the rules in as a plain map. The name lives here because the warning about an unsettled extension
// is written here and points the reader at the file.
pub const EXTENSION_PRIORITY_FILE_NAME : &str = "extension_priority.txt";
// The name of the report row holding everything no target was given a name for. Not the directory's
// own name, which would claim files that a named target has already taken out of it.
pub const UNNAMED_MODULE_NAME : &str = "(unnamed)";

pub(crate) type FaultyFilesListMut = Arc<Mutex<Vec<FaultyFileDetails>>>;
pub(crate) type SharedLanguageLookup = Arc<engine::identity::LanguageLookup>;
// One bucket per module. A run where the user named no modules at all has exactly one bucket, so
// nothing further down has two shapes to handle.
pub(crate) type StatsMapMut = Arc<Mutex<Vec<HashMap<String,Stats>>>>;
pub(crate) type EmbeddedMapMut = Arc<Mutex<Vec<HashMap<String,HashMap<String,Stats>>>>>;

// Counts the directories and files named in 'config' and returns the figures.
//
// 'languages' has to have been resolved against this same 'config'. Resolving is what applies the
// chosen and excluded languages and the forced extensions, so an ill-matched pair would count one
// set of languages while the settings say another: no error, just wrong numbers. The run refuses
// such a pair instead of answering it.
//
// 'progress' is watched while the call blocks: the run moves its counters as files are found and
// parsed, so a thread of the caller's can draw them. Pass 'None' when nobody is watching.
//
// 'on_traversal_done' is called once, as soon as the directories have been scanned, and is told how
// many files were found. The counting of those files is still going on at that point, which is why
// this is a callback: afterwards there is no way to know what was found before the counting ended.
// It is called on every run that returns 'Ok', including one that found nothing to count. It is not
// called when one of the scanning threads died, because the figures such a run leaves behind are
// lower than what is really on disk. Pass '|_| {}' to ignore it.
pub fn run(config: &EngineConfig, languages: Languages, progress: Option<Arc<ScanProgress>>,
        on_traversal_done: impl FnOnce(FilesPresent)) -> Result<RunResult, RunError>
{
    let progress = progress.unwrap_or_default();
    // Guarded rather than called on each return: 'run' refuses in six places before the walk ever
    // starts, and a watcher of the public flag must see it rise on every one of them, including
    // the seventh that will be added without remembering this.
    let _walk_ends = WalkDoneGuard(progress.clone());
    if config.dirs.is_empty() {
        return Err(RunError::NoTargets);
    }
    // Checked before anything is read from disk. Left to run, this pair would produce counts that
    // look perfectly normal and are for a different set of languages than the settings describe.
    if !languages.describe_the_same_selection_as(config) {
        return Err(RunError::LanguagesFromAnotherConfig);
    }
    // Idempotent, so a caller that resolved its own targets earlier loses nothing here.
    let dirs = engine::targets::resolve(&config.dirs, !config.no_gitignore, config.should_search_in_dotted)
            .map_err(RunError::InvalidTargets)?;
    let config = Arc::new(config.clone());
    let faulty_files_ref : FaultyFilesListMut  = Arc::new(Mutex::new(Vec::with_capacity(10)));
    let finish_condition_ref = Arc::new(AtomicBool::new(false));
    let (by_name, lookup, embedded_definitions) = languages.into_parts();
    let language_map_ref = Arc::new(by_name);
    let language_lookup: SharedLanguageLookup = Arc::new(lookup);
    let embedded_definitions = Arc::new(embedded_definitions);
    let modules = Arc::new(Modules::of(&dirs));
    let stats_per_module : StatsMapMut =
            Arc::new(Mutex::new(make_language_stats(&language_map_ref, modules.count())));
    let embedded_per_module : EmbeddedMapMut =
            Arc::new(Mutex::new(vec![HashMap::new(); modules.count()]));

    let mut files_present = FilesPresent::default();
    let idle_producers = Arc::new(AtomicUsize::new(0));
    let files_injector = Arc::new(Injector::<ParsableFile>::new());
    let dirs_injector = Arc::new(Injector::<TraversedDir>::new());
    let exclude_matcher = Arc::new(engine::targets::build_exclude_matcher(&config.exclude_dirs)
            .map_err(|_| {
                // The builder rewrites every pattern into a longer form before compiling it, and its
                // error quotes that rewritten text, which the user never typed. Trying the patterns
                // one at a time finds which of them is the broken one, so the error can quote it as
                // it was written.
                let culprit = config.exclude_dirs.iter()
                        .find(|x| engine::targets::build_exclude_matcher(std::slice::from_ref(x)).is_err())
                        .cloned().unwrap_or_default();
                RunError::InvalidExcludePattern(culprit)
            })?);
    calculate_single_file_stats_or_add_to_injector(&config, &dirs, &dirs_injector, &files_injector, &mut files_present,
            &language_lookup, &modules, &progress);

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
                idle_producers.clone(), language_lookup.clone(), exclude_matcher.clone(),
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
    for i in 0..config.threads.consumers() {
        match engine::consumer::start_parser_thread(i, files_injector.clone(), faulty_files_ref.clone(), finish_condition_ref.clone(),
                stats_per_module.clone(), embedded_per_module.clone(), language_map_ref.clone(),
                embedded_definitions.clone(), config.clone(),
                parsing_started_instant, counting_ended.clone(), progress.clone()) {
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
    progress.mark_walk_done();
    let producers_done_millis = parsing_started_instant.elapsed().as_millis();

    let queued_at_producer_exit = files_injector.len();

    finish_condition_ref.store(true,Ordering::Relaxed);

    // **The callback goes below the flag above and never above it.** It is the caller's code, it may
    // panic, and a panic here unwinds past the joins with the consumers still running: they leave
    // their loop only on that flag, so raising it first is what lets them finish instead of spinning
    // forever. Measured with 16 consumers: raised first, the process is idle a moment later; raised
    // after, 20 threads were still burning ten seconds on.
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

    let relevant_files_num = files_present.relevant_files;
    if relevant_files_num == 0 {
        return Ok(RunResult::of_nothing(files_present,
                Performance { duration_millis: parsing_duration_millis, threads: threads_used }, &modules,
                dirs.to_vec(), std::mem::take(&mut unreadable_dirs.lock().unwrap())));
    }

    let mut stats_guard = stats_per_module.lock();
    let per_module = stats_guard.as_deref_mut().unwrap();
    let mut embedded_guard = embedded_per_module.lock();
    let embedded_by_module = embedded_guard.as_deref_mut().unwrap();

    let mut per_language = merge_over_modules(per_module);
    // Dropped before the total is summed, or the total's keyword map would name the keywords of
    // every language the run selected, including the ones no file was written in. The figures are
    // the same either way, since an empty language adds nothing.
    remove_languages_with_0_files(&mut per_language);
    let total = Stats::total_of(&per_language);

    // The decomposition across every module, summed the same way the rows are
    let mut embedded: HashMap<String, HashMap<String, Stats>> = HashMap::new();
    for bucket in embedded_by_module.iter() {
        for (shell_name, sections) in bucket {
            let shell_entry = embedded.entry(shell_name.clone()).or_default();
            for (inner_name, stats) in sections {
                shell_entry.entry(inner_name.clone()).or_default().add(stats);
            }
        }
    }

    let modules_result = per_module.iter_mut().enumerate().map(|(id, bucket)| {
        let mut of_this_module = std::mem::take(bucket);
        remove_languages_with_0_files(&mut of_this_module);
        // A module that found nothing still gets its row: it was asked for by name, and its absence
        // would read as a mistake in the report.
        ModuleResult {
            name: modules.name_of(id as ModuleId).map(str::to_owned),
            total: Stats::total_of(&of_this_module),
            per_language: of_this_module,
            embedded: std::mem::take(&mut embedded_by_module[id])
        }
    }).collect::<Vec<_>>();

    Ok(RunResult {
        per_language,
        total,
        embedded,
        modules: modules_result,
        faulty_files: std::mem::take(&mut faulty_files_ref.lock().unwrap()),
        files_present,
        performance: Performance { duration_millis: parsing_duration_millis, threads: threads_used },
        targets: dirs.to_vec(),
        unreadable_dirs: std::mem::take(&mut unreadable_dirs.lock().unwrap())
    })
}

// Whether this run will print its phase report to the error output, as MEZURA_PHASE_TIMING asks.
// Public so a caller drawing live lines of its own on stderr can keep them out of the report's way.
pub fn prints_phase_timing() -> bool {
    *phase_timing::ENABLED
}

struct WalkDoneGuard(Arc<ScanProgress>);

impl Drop for WalkDoneGuard {
    fn drop(&mut self) {
        self.0.mark_walk_done();
    }
}

// Fills the two queues the threads work from: a target that is a single file is counted here and
// now, a directory is put in the queue for a scanning thread to descend into.
//
// Only the outermost targets are queued. One that sits inside another is reached by the scan of the
// one around it, and queueing both would count its files twice; the name it was given is not lost
// with it, the module table still hands it back on the way down.
pub(crate) fn calculate_single_file_stats_or_add_to_injector(config: &EngineConfig, dirs: &engine::targets::Targets,
        dirs_injector: &Arc<Injector<TraversedDir>>, files_injector: &Arc<Injector<ParsableFile>>,
        files_present: &mut FilesPresent, language_lookup: &engine::identity::LanguageLookup, modules: &Modules,
        progress: &ScanProgress)
{
    crate::engine::targets::topmost_targets(dirs).iter().for_each(|target| {
        let dir_path = Path::new(&target.path);
        let module = modules.of_target(target);
        if dir_path.is_file() {
            if let Some(lang_name) = language_lookup.of_path(dir_path) {
                files_injector.push(ParsableFile::new(dir_path.to_path_buf(), lang_name, module));
                files_present.total_files += 1;
                files_present.relevant_files += 1;
                progress.record_file_found();
            }
        } else if dir_path.is_dir() {
            let gitignore_stack = if config.no_gitignore { None } else { GitignoreStack::for_root_dir(dir_path) };
            dirs_injector.push(TraversedDir::new(dir_path.to_path_buf(), gitignore_stack, module));
        }
    })
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
    pub module: ModuleId
}

impl ParsableFile {
    pub fn new(path: PathBuf, language_name: Arc<str>, module: ModuleId) -> Self {
        ParsableFile {
            path,
            language_name,
            module
        }
    }
}

#[derive(Debug,Clone)]
pub(crate) struct TraversedDir {
    pub path: PathBuf,
    pub gitignore_stack: Option<Arc<GitignoreStack>>,
    pub module: ModuleId
}

impl TraversedDir {
    pub fn new(path: PathBuf, gitignore_stack: Option<Arc<GitignoreStack>>, module: ModuleId) -> Self {
        TraversedDir {
            path,
            gitignore_stack,
            module
        }
    }
}

// The '.gitignore' files that apply at one depth, innermost first, each linked to the one above it.
// The walk extends the chain as it descends so no directory reparses its parents' rules.
#[derive(Debug)]
pub(crate) struct GitignoreStack {
    matcher: ignore::gitignore::Gitignore,
    parent: Option<Arc<GitignoreStack>>
}

impl GitignoreStack {
    pub fn extend_with_dir(dir: &Path, parent: Option<Arc<GitignoreStack>>) -> Option<Arc<GitignoreStack>> {
        let gitignore_path = dir.join(".gitignore");
        if !gitignore_path.is_file() {
            return parent;
        }

        let (matcher, _) = ignore::gitignore::Gitignore::new(&gitignore_path);
        if matcher.is_empty() {
            return parent;
        }

        Some(Arc::new(GitignoreStack { matcher, parent }))
    }

    // The .gitignore files of every dir between the repository root and the given dir, excluding it
    fn of_ancestors(dir: &Path) -> Option<Arc<GitignoreStack>> {
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
            stack = Self::extend_with_dir(ancestor, stack);
        }
        stack
    }

    // Explicitly given target dirs are traversed even if a .gitignore of their ancestors ignores them
    pub fn for_root_dir(dir: &Path) -> Option<Arc<GitignoreStack>> {
        let stack = Self::of_ancestors(dir);
        if let Some(s) = &stack && s.is_ignored(dir, true) {
            return None;
        }
        stack
    }

    // Used for paths that the program discovered on its own, like the matches of a glob pattern
    pub fn is_path_ignored(path: &Path) -> bool {
        let is_dir = path.is_dir();
        let Some(parent) = path.parent() else { return false };

        let stack = Self::extend_with_dir(parent, Self::of_ancestors(parent));
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

    pub fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
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

// The per-language figures of every module added together, which is what a question about the whole
// run reads.
fn merge_over_modules(per_module: &[HashMap<String,Stats>]) -> HashMap<String,Stats> {
    let mut merged = per_module[0].clone();
    for of_a_module in &per_module[1..] {
        for (name, stats) in of_a_module {
            merged.entry(name.clone()).or_default().add(stats);
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

    // The merge that ends a consumer reaches into these maps and unwraps, so a language or a module
    // that was never given an entry would kill the thread rather than miscount. Asserted here and
    // not through a run, since a result has had the empty languages removed from it by then.
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

    // The total is the same measurement summed, in the same type, which is the whole point of there
    // being one. The two derived figures are methods and not stored, so they cannot drift from what
    // they are derived from, and the keywords add up now where the totals used to carry none.
    #[test]
    fn the_total_is_the_languages_added_together() {
        let languages = hashmap![
            "a".to_owned() => Stats::new(20, 100_000, 2000, 1400, 100, hashmap!["classes".to_owned() => 7]),
            "b".to_owned() => Stats::new(10, 50_000, 1000, 800, 50, hashmap!["classes".to_owned() => 2]),
            "c".to_owned() => Stats::new(10, 50_000, 1000, 800, 50, hashmap!["structs".to_owned() => 5])
        ];
        let total = Stats::total_of(&languages);

        assert_eq!(40, total.files);
        assert_eq!(200_000, total.bytes);
        assert_eq!(4000, total.lines);
        assert_eq!(3000, total.code_lines);
        assert_eq!(200, total.comment_lines);
        // what is neither code nor comment, worked out and not stored
        assert_eq!(800, total.calculate_extra_lines());
        assert_eq!(5000, total.calculate_average_size());
        // 'classes' exists in two of the three, which is the question the totals could not answer
        // at all before: they carried no keywords.
        assert_eq!(Some(&9), total.keyword_occurences.get("classes"));
        assert_eq!(Some(&5), total.keyword_occurences.get("structs"));

        // and nothing to add up is a total of nothing. 'average_size' over no files is 'domain's
        // own question and is asserted there.
        assert_eq!(0, Stats::total_of(&HashMap::new()).files);
    }
}

// What 'run' owes its caller when a worker thread dies: an error, never a number it knows is short.
// A worker merges its counters at the end, so one that dies mid-run takes its share of the counting
// with it. The two hooks that cause the deaths fire on the corpus names used here and on nothing else.
#[cfg(test)]
mod worker_death_tests {
    use crate::{EngineConfig, Languages, RunError, run};

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

        let err = run(&config, languages_for(&config), None, |_| {});
        std::fs::remove_dir_all(&root).unwrap();
        let (clean_root, clean_config) = corpus("mezura-alive-consumer");
        let clean = run(&clean_config, languages_for(&clean_config), None, |_| {});
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

        let err = run(&config, languages_for(&config), None, |_| {});
        std::fs::remove_dir_all(&root).unwrap();
        let (clean_root, clean_config) = corpus("mezura-alive-producer");
        let clean = run(&clean_config, languages_for(&clean_config), None, |_| {});
        std::fs::remove_dir_all(&clean_root).unwrap();

        let err = err.expect_err("a producer died and run returned a result anyway");
        assert!(matches!(&err, RunError::IncompleteRun { worker_panic } if worker_panic.contains("test-induced producer panic")),
                "got: {err:?}");
        assert_eq!(1, clean.unwrap().total.files);
    }

    // A dead producer takes its share of the walk with it and merges nothing, so the counters left
    // behind are short. The announcement fires before the guard that turns the death into an error,
    // which is what it is for, so it is the one thing that could put a wrong number on the screen a
    // moment before the run refuses it. Measured on a tree of 60 with one of two producers dying: it
    // announced 30 and then errored.
    #[test]
    fn a_walk_whose_own_thread_died_is_never_announced() {
        let (root, config) = corpus("mezura-dead-producer-announce");
        let mut announced = Vec::new();
        let outcome = run(&config, languages_for(&config), None, |scan| announced.push(scan));
        std::fs::remove_dir_all(&root).unwrap();

        // the hook answers to 'mezura-dead-producer' as a prefix, so this corpus dies the same way
        assert!(matches!(&outcome, Err(RunError::IncompleteRun { .. })), "got: {outcome:?}");
        assert!(announced.is_empty(), "a walk that lost a thread was announced anyway: {announced:?}");

        // and the same run with every thread intact does announce, so the guard above is the
        // difference and not some other reason nothing was said
        let (clean_root, clean_config) = corpus("mezura-alive-producer-announce");
        let mut announced = Vec::new();
        let clean = run(&clean_config, languages_for(&clean_config), None, |scan| announced.push(scan));
        std::fs::remove_dir_all(&clean_root).unwrap();
        assert_eq!(1, clean.unwrap().total.files);
        assert_eq!(1, announced.len(), "an intact walk was not announced");
    }

    // The other half of what a run reports as its duration, and the half that was wrong. The
    // integration test beside this one holds that a slow callback is not charged to the counting,
    // and it can only see the case where the callback is the last thing running: twenty tiny files
    // are consumed long before its sleep is over. The fix that answered it subtracted the callback's
    // elapsed time from the clock, which is right in exactly that case and wrong in the other one.
    //
    // Here the counting outlasts the callback, which is what every run over a real tree looks like.
    // The callback delayed nothing, so nothing should come off the figure. Ten files at forty
    // milliseconds through one consumer is four hundred milliseconds of counting under a callback
    // that sleeps a hundred and fifty: subtracting gives two hundred and fifty, and the rate the
    // command line prints from it is a third too high.
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

        let counted = run(&config, languages_for(&config), None, |_| std::thread::sleep(callback_holds)).unwrap();
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
