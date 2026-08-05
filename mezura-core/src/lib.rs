#![forbid(unsafe_code)]

#![allow(non_snake_case)]

// Test scaffolding, and every call site in this crate is in a test module. Exported it was published
// API: '#[macro_export]' is unconditional and puts the macro at the root of whoever depends on us.
#[cfg(test)]
macro_rules! hashmap {
    ($( $key: expr => $val: expr ),*) => {{
        #[allow(unused_mut)]
        let mut map = ::std::collections::HashMap::new();
        $( map.insert($key, $val); )*
        map
    }}
}

mod domain;
mod result;
mod phase_timing;
// The arithmetic of showing a result, which the counting does not need and a caller drawing its own
// view does. Optional in the sense the layout of this crate uses: one caller wants it, another has
// its own way of showing things and never looks.
pub mod render;
// Still open, and each is its own decision: see B0c and B0d in RESTRUCTURE.md section 12.
pub mod engine;
pub mod languages;
pub mod language_file;
// The codes are what one caller wants and another does not, so they stay behind the module. The two
// types every caller meets are re-exported below.
pub mod warnings;


pub use engine::config::{EngineConfig, Target, Threads};
pub use engine::targets::TargetError;
pub use languages::Languages;
pub use domain::{FileStats, Keyword, Language, Stats};
pub use result::{FaultyFileDetails, FilesPresent, ModuleResult, Performance, RunError, RunResult,
        SortCriterion, UnreadableDirDetails};
pub use warnings::{Affects, Warning};

pub(crate) type FaultyFilesListMut = Arc<Mutex<Vec<FaultyFileDetails>>>;
pub(crate) type ExtensionLangMap = Arc<HashMap<String, Arc<str>>>;
// One bucket per module, and a run that declared none has exactly one, so that nothing downstream
// has two shapes to handle
pub(crate) type StatsMapMut = Arc<Mutex<Vec<HashMap<String,Stats>>>>;

use engine::extensions::find_language_of_extension;
use engine::modules::{ModuleId, Modules};

use crossbeam_deque::{Worker,Injector};
use std::{collections::HashMap, path::{Path, PathBuf}, sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering}, time::Instant};
use std::sync::{Arc, Mutex};


// Named here and not with the rest of the file layout, which belongs to the command line, because a
// warning this crate emits tells the reader to declare the contested extension in it. Whoever acts
// on that warning needs the name as much as whoever writes the file.
pub const EXTENSION_PRIORITY_FILE_NAME : &str = "extension_priority.txt";
// Marked rather than named, because naming it after its directory would be a lie: with
// './project tests=./project/tests' a row called 'project' is everything in it except the tests.
// Being one marked row is also what settles two unnamed targets ending in the same folder name.
// It says what the row is and not what is left in it, which is the only wording that holds in both
// shapes: with './project tests=./project/tests' the row really is the rest of the project, but
// with 'frontend=./web ./docs' the './docs' did not survive anything, it was simply never named.
pub const UNNAMED_MODULE_NAME : &str = "(unnamed)";

// What the tests read: the repository's own 'data/', which the program itself never reads (it reads
// the persistent directory, and that one belongs to the command line), and the checked-in inputs
// under 'tests/fixtures'. Nothing here is ever written to. Both are anchored on the manifest rather
// than on the working directory, which cargo happens to set to the package root: a test that leans
// on that passes from cargo and nowhere else.
#[cfg(test)]
pub(crate) mod test_paths {
    pub const DATA_DIR      : &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/");
    pub const LANGUAGES_DIR : &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/languages/");
    pub const FIXTURES_DIR  : &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
}


// What a panic left behind, as text. 'panic!' with a literal carries a '&str', everything formatted
// carries a 'String', and anything else is somebody's typed payload, which has no text to give.
pub(crate) fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&'static str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "a worker died with a panic payload that is not text".to_owned()
    }
}

// 'languages' must have been resolved with the same 'config' handed here: the narrowing by name and
// the forced extensions are applied during resolution, so a different config would count one set of
// languages while the settings claim another.
//
// 'on_traversal_done' is called with what the walk found, the moment the walk ends and while the
// counting of what it queued is still going on. That instant is the only thing a caller cannot
// reach around the call, and it is why the callback exists rather than the same figures being read
// off the result, where they also are. A caller with nothing to say at that moment passes '|_| {}'
// and the compiler removes it.
//
// **Exactly once on every run that returns 'Ok'.** A walk that found nothing is still announced,
// because what is being reported is the walk and not the haul. The one case it does not fire is a
// walk whose own thread died, and that run returns 'Err(IncompleteRun)': the figures such a walk
// left behind are short of what is on disk, since a producer that died never merged its share, and
// announcing them puts a number on the screen that the error a moment later contradicts. Measured on
// a tree of 60 files with one of two producers dying, it announced 30.
//
// So there is no silent case and nothing for a caller to guard: not firing is always accompanied by
// the error, which the caller has to handle anyway, and firing is a promise that the figures are
// final. What is deliberately not offered is the third shape, firing with a marker saying the walk
// was partial, which hands the caller the same wrong number and one more thing to remember.
//
// The time it spends is its own and is charged to nobody. 'Performance.duration_millis' is measured
// by the consumers rather than by this thread precisely so that it cannot contain any of it.
pub fn run(config: &EngineConfig, languages: Languages,
        on_traversal_done: impl FnOnce(FilesPresent)) -> Result<RunResult, RunError>
{
    if config.dirs.is_empty() {
        return Err(RunError::NoTargets);
    }
    // Before anything is walked, because the two arguments describing different runs is not a
    // failure that shows up in the answer: it shows up as a perfectly ordinary answer to a question
    // nobody asked.
    if !languages.describe_the_same_selection_as(config) {
        return Err(RunError::LanguagesFromAnotherConfig);
    }
    // The declared targets become places to walk here, with the settings of the same configuration
    // the walk itself obeys, so the two can never disagree. Resolution is existence-first and
    // idempotent, so a caller that resolved early for its own reasons loses nothing by this pass.
    let dirs = engine::targets::resolve(&config.dirs, !config.no_gitignore, config.should_search_in_dotted)
            .map_err(RunError::InvalidTargets)?;
    let config = Arc::new(config.clone());
    let faulty_files_ref : FaultyFilesListMut  = Arc::new(Mutex::new(Vec::with_capacity(10)));
    let finish_condition_ref = Arc::new(AtomicBool::new(false));
    // Already narrowed and already resolved, by whoever built it. Nothing about which languages
    // exist is decided in here, so nothing in here has anything to complain about.
    let (by_name, extension_map) = languages.into_parts();
    let language_map_ref = Arc::new(by_name);
    let extension_lang_map: ExtensionLangMap = Arc::new(extension_map);
    // Pre-built for every module and language pair, and not only for every language: the merge that
    // ends a consumer reaches straight into this map and unwraps, so a pair that was never foreseen
    // would kill the thread rather than miscount
    let modules = Arc::new(Modules::of(&dirs));
    let stats_per_module : StatsMapMut =
            Arc::new(Mutex::new(make_language_stats(&language_map_ref, modules.count())));

    let mut files_present = FilesPresent::default();
    let idle_producers = Arc::new(AtomicUsize::new(0));
    let files_injector = Arc::new(Injector::<ParsableFile>::new());
    let dirs_injector = Arc::new(Injector::<TraversedDir>::new());
    let exclude_matcher = Arc::new(engine::targets::build_exclude_matcher(&config.exclude_dirs)
            .map_err(|_| {
                // The builder's own error names the anchored form, which the caller never wrote,
                // so the culprit is found by asking about each pattern on its own
                let culprit = config.exclude_dirs.iter()
                        .find(|x| engine::targets::build_exclude_matcher(std::slice::from_ref(x)).is_err())
                        .cloned().unwrap_or_default();
                RunError::InvalidExcludePattern(culprit)
            })?);
    calculate_single_file_stats_or_add_to_injector(&config, &dirs, &dirs_injector, &files_injector, &mut files_present,
            &extension_lang_map, &modules);

    let files_stats = Arc::new(Mutex::new(files_present));
    let unreadable_dirs = Arc::new(Mutex::new(Vec::new()));

    let mut producer_handles = Vec::with_capacity(config.threads.producers());
    let mut consumer_handles = Vec::with_capacity(config.threads.consumers());
    // Producers terminate when the idle count reaches this, so it must hold the number that
    // actually started, and it must be fixed before any producer can finish: comparing against a
    // count that is still growing lets an early finisher see itself as the last one standing.
    // Until the spawns are done it holds a value the idle count cannot reach.
    let producers_total = Arc::new(AtomicUsize::new(usize::MAX));
    let worker_panics: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    // A thread the operating system refuses is a slower run, not a different answer, so the run
    // carries on with what it was given. Zero of either side is the exception: nothing would be
    // discovered, or nothing counted, and the result would dress the refusal up as an empty answer.
    let parsing_started_instant = Instant::now();
    let mut last_refusal = None;
    for i in 0..config.threads.producers() {
        match engine::producer::start_producer_thread(i, files_injector.clone(), dirs_injector.clone(), Worker::new_fifo(),
                idle_producers.clone(), extension_lang_map.clone(), exclude_matcher.clone(),
                config.clone(), files_stats.clone(), modules.clone(), unreadable_dirs.clone(),
                producers_total.clone(), worker_panics.clone()) {
            Ok(handle) => producer_handles.push(handle),
            Err(x) => last_refusal = Some(x)
        }
    }
    if producer_handles.is_empty() {
        return Err(RunError::NoThreadsAvailable { side: "producer", error: last_refusal.unwrap() });
    }
    producers_total.store(producer_handles.len(), Ordering::SeqCst);

    // Written by whichever consumer stops last and read once they have all been joined. It is the
    // measurement itself and not a correction applied to one: see the comment where it is read.
    let counting_ended = Arc::new(AtomicU64::new(0));
    for i in 0..config.threads.consumers() {
        match engine::consumer::start_parser_thread(i, files_injector.clone(), faulty_files_ref.clone(), finish_condition_ref.clone(),
                stats_per_module.clone(), language_map_ref.clone(), config.clone(),
                parsing_started_instant, counting_ended.clone()) {
            Ok(handle) => consumer_handles.push(handle),
            Err(x) => last_refusal = Some(x)
        }
    }
    if consumer_handles.is_empty() {
        // The producers are already walking and will finish on their own; they are collected so
        // that no thread outlives the call that started it
        for handle in producer_handles {
            let _ = handle.join();
        }
        return Err(RunError::NoThreadsAvailable { side: "consumer", error: last_refusal.unwrap() });
    }

    let threads_used = Threads::new(producer_handles.len(), consumer_handles.len());
    for handle in producer_handles {
        let _ = handle.join();
    }
    let producers_done_millis = parsing_started_instant.elapsed().as_millis();

    let queued_at_producer_exit = files_injector.len();

    finish_condition_ref.store(true,Ordering::Relaxed);

    // Here and not at the end, which is the only moment a caller cannot reach on its own: the walk
    // is over, so its counts are final and nothing writes to them again, while the consumers are
    // still draining what it queued. Announced before the run has an answer, since that is the whole
    // of what it is worth. Unconditionally too, so a walk that found nothing still says so.
    //
    // **Below the two lines above and never above them.** This is caller code and it may panic, and
    // a panic here unwinds past the joins with the consumers still running: they leave their loop
    // only on the flag that is now already raised, so they finish instead of spinning on one nobody
    // will ever raise. Measured, with 16 consumers over this crate's own 'src': raised first, the
    // process is back to 4 threads and no CPU; raised after, 20 threads were still alive and burning
    // ten seconds later. The queue size just above is a diagnostic and is read before the callback
    // for a smaller reason of the same kind: it is a measurement of the run, not of the printing.
    //
    // Both lists are read the way the panic list is read further down and for the same reason: this
    // is above the guard that turns a dead worker into an error, so a poisoned lock here would panic
    // in the caller's thread with a message about a mutex instead of letting that error be returned.
    //
    // A producer that died never merged its share of the walk, so what is in these counters is short
    // of what is out there, and the run is about to refuse them anyway. Announcing them would put a
    // count on the screen that the error two steps down contradicts: measured on a tree of 60 files
    // with one of two producers dying, it announced 30.
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
    // What the counting cost, taken from the consumers themselves rather than from the clock at the
    // join above. That clock is not a measurement of the counting: the callback runs on this thread
    // while the consumers are still draining, so it holds whichever of the two finished last, and a
    // caller doing something slow in there was charged to the parser. Measured, 50 files really
    // counted in 9 ms were reported as 41 files per second, and a rate line that only appears for
    // runs over a second was fabricated out of the caller's own wait.
    //
    // Subtracting the callback's elapsed time was the first answer to that and it is only right when
    // the callback finished last. When the consumers did, the callback delayed nothing and the
    // subtraction took real counting off the figure: 3,100 ms of counting under a 500 ms callback
    // came back as 2,600 ms, and the files per second went up by the same sixth. There is no
    // correction to apply from here, because this thread cannot see when the consumers stopped. They
    // can, so they say so.
    //
    // The floor is kept because the debug timing line below subtracts the two, and because a run
    // whose consumers all died records nothing at all: that is an error two steps down, and it
    // should stay one rather than becoming an underflow here.
    let parsing_duration_millis = u128::from(counting_ended.load(Ordering::Relaxed)).max(producers_done_millis);

    if *phase_timing::ENABLED {
        eprintln!("[phase] producers alive: {} ms | drain after producers: {} ms | queue size at producer exit: {}",
            producers_done_millis, parsing_duration_millis - producers_done_millis, queued_at_producer_exit);
        eprintln!("{}", phase_timing::report(threads_used.consumers(), parsing_duration_millis));
    }

    // Before anything shared is read: a worker that died may have poisoned whichever lock it held,
    // and the locks below would then panic in the caller's thread with a message about a mutex,
    // three calls away from what actually happened. Nothing after this line runs unless every
    // worker finished whole, which is also what keeps those locks clean by construction.
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

    let mut per_language = merged_over_modules(per_module);
    // Before the total, and the order matters now that a total carries keywords. Every language the
    // run selected was given a bucket with its own keyword names set to zero, so summing first put
    // the union of every declared name into the total: a Rust-only tree came back reporting nought
    // classes and nought interfaces, from languages that had not appeared at all. The module totals
    // are summed after their own filtering and always were, so the two disagreed about which
    // keywords existed within one result. The numbers are the same either way, since an empty
    // language adds nothing to any of them.
    remove_languages_with_0_files(&mut per_language);
    let total = Stats::total_of(&per_language);

    let modules_result = per_module.iter_mut().enumerate().map(|(id, bucket)| {
        let mut of_this_module = std::mem::take(bucket);
        remove_languages_with_0_files(&mut of_this_module);
        // A module that found nothing still gets its row, since it was asked for by name and its
        // absence from the report would read as a mistake in the report
        ModuleResult {
            name: modules.name_of(id as ModuleId).map(str::to_owned),
            total: Stats::total_of(&of_this_module),
            per_language: of_this_module
        }
    }).collect::<Vec<_>>();

    Ok(RunResult {
        per_language,
        total,
        modules: modules_result,
        faulty_files: std::mem::take(&mut faulty_files_ref.lock().unwrap()),
        files_present,
        performance: Performance { duration_millis: parsing_duration_millis, threads: threads_used },
        targets: dirs.to_vec(),
        unreadable_dirs: std::mem::take(&mut unreadable_dirs.lock().unwrap())
    })
}

// The totals across every module, which is what the overview, the sum and the document's own
// language list are about: those questions are asked of the whole run and not of one part of it
fn merged_over_modules(per_module: &[HashMap<String,Stats>]) -> HashMap<String,Stats> {
    let mut merged = per_module[0].clone();
    for of_a_module in &per_module[1..] {
        for (name, stats) in of_a_module {
            merged.entry(name.clone()).or_default().add(stats);
        }
    }

    merged
}


// The roots and not every target: a target that lies inside another is reached by the walk of the
// one around it, and walking it again would count its files twice. Its module is not lost with it,
// it is what the boundary table hands back on the way down. 'dirs' is the resolved list the run
// built at its entry, never the declared one off the configuration.
pub(crate) fn calculate_single_file_stats_or_add_to_injector(config: &EngineConfig, dirs: &engine::targets::Targets,
        dirs_injector: &Arc<Injector<TraversedDir>>, files_injector: &Arc<Injector<ParsableFile>>,
        files_present: &mut FilesPresent, extension_lang_map: &HashMap<String, Arc<str>>, modules: &Modules)
{
    crate::engine::targets::topmost_targets(dirs).iter().for_each(|target| {
        let dir_path = Path::new(&target.path);
        let module = modules.of_target(target);
        if dir_path.is_file() {
            if let Some(x) = dir_path.extension()
                && let Some(extension) = x.to_str()
                && let Some(lang_name) = find_language_of_extension(extension_lang_map, extension) {
                files_injector.push(ParsableFile::new(dir_path.to_path_buf(), lang_name, module));
                files_present.total_files += 1;
                files_present.relevant_files += 1;
            }
        } else if dir_path.is_dir() {
            let gitignore_stack = if config.no_gitignore { None } else { GitignoreStack::for_root_dir(dir_path) };
            dirs_injector.push(TraversedDir::new(dir_path.to_path_buf(), gitignore_stack, module));
        }
    })
}

// A language nobody wrote a file in adds nothing to any total and would take a row in every report.
// Removed after the totals are worked out, since the empty ones contribute nothing to them and
// dropping them first would only make the same sum out of fewer entries.
pub(crate) fn remove_languages_with_0_files(languages: &mut HashMap<String,Stats>) {
    languages.retain(|_, stats| stats.files > 0);
}

// One bucket per language in every module, and not only per language: the merge that ends a
// consumer reaches into this map by name, so a language that was never given a slot would kill the
// thread rather than miscount.
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

#[derive(Debug,Clone)]
pub(crate) struct TraversedDir {
    pub path: PathBuf,
    pub gitignore_stack: Option<Arc<GitignoreStack>>,
    pub module: ModuleId
}

#[derive(Debug)]
pub(crate) struct GitignoreStack {
    matcher: ignore::gitignore::Gitignore,
    parent: Option<Arc<GitignoreStack>>
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

impl TraversedDir {
    pub fn new(path: PathBuf, gitignore_stack: Option<Arc<GitignoreStack>>, module: ModuleId) -> Self {
        TraversedDir {
            path,
            gitignore_stack,
            module
        }
    }
}

impl GitignoreStack {
    pub fn extended(dir: &Path, parent: Option<Arc<GitignoreStack>>) -> Option<Arc<GitignoreStack>> {
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
            stack = Self::extended(ancestor, stack);
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

        let stack = Self::extended(parent, Self::of_ancestors(parent));
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


// Shared by the tests of three modules, so it cannot live inside any one of their 'mod tests'
#[cfg(test)]
pub(crate) fn languages_claiming(claims: &[(&str, &[&str])]) -> HashMap<String, Language> {
    languages::keyed_by_name(claims.iter().map(|(name, extensions)|
            Language::new(name, *extensions, ["\""], ["//"], None, [])))
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
        assert_eq!(800, total.extra_lines());
        assert_eq!(5000, total.average_size());
        // 'classes' exists in two of the three, which is the question the totals could not answer
        // at all before: they carried no keywords.
        assert_eq!(Some(&9), total.keyword_occurences.get("classes"));
        assert_eq!(Some(&5), total.keyword_occurences.get("structs"));

        // and nothing counted is a zero rather than a division by zero
        assert_eq!(0, Stats::default().average_size());
        assert_eq!(0, Stats::total_of(&HashMap::new()).files);
    }
}


// What 'run' owes its caller when a worker thread dies: an error, never a number it knows is short.
// A worker merges its counters at the end, so one that dies mid-run takes its share of the counting
// with it, and the old 'let _ = handle.join()' threw the only evidence away. The two hooks that
// cause the deaths fire on the corpus names used here and on nothing else.
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
        Languages::resolve(config, languages, &std::collections::HashMap::new()).0
    }

    #[test]
    fn a_dead_consumer_is_an_error_and_not_a_short_count() {
        let (root, config) = corpus("mezura-dead-consumer");

        let err = run(&config, languages_for(&config), |_| {});
        std::fs::remove_dir_all(&root).unwrap();
        let (clean_root, clean_config) = corpus("mezura-alive-consumer");
        let clean = run(&clean_config, languages_for(&clean_config), |_| {});
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

        let err = run(&config, languages_for(&config), |_| {});
        std::fs::remove_dir_all(&root).unwrap();
        let (clean_root, clean_config) = corpus("mezura-alive-producer");
        let clean = run(&clean_config, languages_for(&clean_config), |_| {});
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
        let outcome = run(&config, languages_for(&config), |scan| announced.push(scan));
        std::fs::remove_dir_all(&root).unwrap();

        // the hook answers to 'mezura-dead-producer' as a prefix, so this corpus dies the same way
        assert!(matches!(&outcome, Err(RunError::IncompleteRun { .. })), "got: {outcome:?}");
        assert!(announced.is_empty(), "a walk that lost a thread was announced anyway: {announced:?}");

        // and the same run with every thread intact does announce, so the guard above is the
        // difference and not some other reason nothing was said
        let (clean_root, clean_config) = corpus("mezura-alive-producer-announce");
        let mut announced = Vec::new();
        let clean = run(&clean_config, languages_for(&clean_config), |scan| announced.push(scan));
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

        let counted = run(&config, languages_for(&config), |_| std::thread::sleep(callback_holds)).unwrap();
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
