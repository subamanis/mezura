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
// Still open, and each is its own decision: see B0c and B0d in RESTRUCTURE.md section 12.
pub mod engine;
pub mod languages;
pub mod language_file;
// The codes are what one caller wants and another does not, so they stay behind the module. The two
// types every caller meets are re-exported below.
pub mod warnings;


pub use engine::config::{EngineConfig, Target, Threads};
pub use languages::Languages;
pub use domain::{Language, LanguageContentInfo, LanguageMetadata, FileStats, Keyword};
pub use result::{FaultyFileDetails, FilesPresent, FinalStats, Metrics, ModuleResult, RunError, RunResult};
pub use warnings::{Affects, Warning};

pub(crate) type FaultyFilesListMut = Arc<Mutex<Vec<FaultyFileDetails>>>;
pub(crate) type ExtensionLangMap = Arc<HashMap<String, Arc<str>>>;
// One bucket per module, and a run that declared none has exactly one, so that nothing downstream
// has two shapes to handle
pub(crate) type ContentInfoMapMut  = Arc<Mutex<Vec<HashMap<String,LanguageContentInfo>>>>;
pub(crate) type MetadataMapMut     = Arc<Mutex<Vec<HashMap<String,LanguageMetadata>>>>;

use engine::extensions::find_language_of_extension;
use engine::modules::{ModuleId, Modules};

use crossbeam_deque::{Worker,Injector};
use std::{collections::HashMap, path::{Path, PathBuf}, sync::atomic::{AtomicBool, AtomicUsize, Ordering}, time::Instant};
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

// The repository's own 'data/', which only tests ever read: the program itself reads the persistent
// directory, and that one belongs to the command line. Anchored on the manifest rather than on the
// executable, so it does not depend on where cargo put the test binary or on the working directory.
#[cfg(test)]
pub(crate) mod test_paths {
    pub const DATA_DIR      : &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/");
    pub const LANGUAGES_DIR : &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/languages/");
    pub const TEST_DIR      : &str = concat!(env!("CARGO_MANIFEST_DIR"), "/test_dir/");
}


// 'on_traversal_done' is called exactly once, with what the walk found, at the only moment a caller
// cannot reach on its own: the two phases overlap, so the counts are known part way through and not
// before or after. Everything else a caller wants to say it can say around the call. A caller with
// nothing to say passes '|_| {}' and the compiler removes it.
pub fn run(config: &EngineConfig, languages: Languages,
        on_traversal_done: impl FnOnce(FilesPresent)) -> Result<RunResult, RunError>
{
    let config = Arc::new(config.clone());
    let faulty_files_ref : FaultyFilesListMut  = Arc::new(Mutex::new(Vec::with_capacity(10)));
    let finish_condition_ref = Arc::new(AtomicBool::new(false));
    // Already narrowed and already resolved, by whoever built it. Nothing about which languages
    // exist is decided in here, so nothing in here has anything to complain about.
    let (definitions, extension_map) = languages.into_parts();
    let language_map_ref = Arc::new(definitions);
    let extension_lang_map: ExtensionLangMap = Arc::new(extension_map);
    // Pre-built for every module and language pair, and not only for every language: the merge that
    // ends a consumer reaches straight into this map and unwraps, so a pair that was never foreseen
    // would kill the thread rather than miscount
    let modules = Arc::new(Modules::of(&config.dirs));
    let languages_content_info_ref : ContentInfoMapMut =
            Arc::new(Mutex::new(make_language_stats(language_map_ref.clone(), modules.count())));
    let global_languages_metadata_map = Arc::new(Mutex::new(make_language_metadata(&language_map_ref, modules.count())));

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
    calculate_single_file_stats_or_add_to_injector(&config, &dirs_injector, &files_injector, &mut files_present,
            &extension_lang_map, &modules);

    let files_stats = Arc::new(Mutex::new(files_present));
    let unreadable_dirs = Arc::new(Mutex::new(Vec::new()));

    let mut producer_handles = Vec::with_capacity(config.threads.producers);
    let mut consumer_handles = Vec::with_capacity(config.threads.consumers);

    let parsing_started_instant = Instant::now();
    for i in 0..config.threads.producers {
        producer_handles.push(engine::producer::start_producer_thread(i, files_injector.clone(), dirs_injector.clone(), Worker::new_fifo(),
            idle_producers.clone(), extension_lang_map.clone(), exclude_matcher.clone(),
            config.clone(), files_stats.clone(), modules.clone(), unreadable_dirs.clone()));
    }
    for i in 0..config.threads.consumers {
        consumer_handles.push(engine::consumer::start_parser_thread(i, files_injector.clone(), faulty_files_ref.clone(), finish_condition_ref.clone(),
        languages_content_info_ref.clone(), global_languages_metadata_map.clone(), language_map_ref.clone(), config.clone()));
    }

    for handle in producer_handles {
        let _ = handle.join();
    }
    let producers_done_millis = parsing_started_instant.elapsed().as_millis();

    let queued_at_producer_exit = files_injector.len();

    finish_condition_ref.store(true,Ordering::Relaxed);
    for handle in consumer_handles {
        let _ = handle.join();
    }
    let parsing_duration_millis = parsing_started_instant.elapsed().as_millis();

    if *phase_timing::ENABLED {
        eprintln!("[phase] producers alive: {} ms | drain after producers: {} ms | queue size at producer exit: {}",
            producers_done_millis, parsing_duration_millis - producers_done_millis, queued_at_producer_exit);
        eprintln!("{}", phase_timing::report());
    }

    let files_present = *files_stats.lock().unwrap();
    let relevant_files_num = files_present.relevant_files;
    // Unconditionally, and before the answer below, so that a caller is told the walk finished even
    // when it found nothing. Whether that is worth printing is the caller's question and not ours.
    on_traversal_done(files_present);
    if relevant_files_num == 0 {
        return Ok(RunResult::of_nothing(files_present, parsing_duration_millis, &modules,
                std::mem::take(&mut unreadable_dirs.lock().unwrap())));
    }

    let mut global_languages_metadata_map_guard = global_languages_metadata_map.lock();
    let per_module_metadata = global_languages_metadata_map_guard.as_deref_mut().unwrap();

    let mut content_info_map_guard = languages_content_info_ref.lock();
    let per_module_content_info = content_info_map_guard.as_deref_mut().unwrap();

    let (content_info_map, languages_metadata_map) = merged_over_modules(per_module_content_info, per_module_metadata);
    let metrics = generate_metrics_if_parsing_took_more_than_one_sec(parsing_duration_millis, relevant_files_num, &content_info_map);
    let final_stats = FinalStats::calculate(&content_info_map, &languages_metadata_map);

    let mut modules_result = Vec::with_capacity(modules.count());
    for id in 0..modules.count() {
        let (mut content_info, mut metadata) = (std::mem::take(&mut per_module_content_info[id]),
                std::mem::take(&mut per_module_metadata[id]));
        remove_languages_with_0_files(&mut content_info, &mut metadata);
        // A module that found nothing still gets its row, since it was asked for by name and its
        // absence from the report would read as a mistake in the report
        let final_stats = if metadata.is_empty() {FinalStats::new_extended(0, 0, 0, 0, 0, 0, 0)}
                else {FinalStats::calculate(&content_info, &metadata)};
        modules_result.push(ModuleResult {
            name: modules.name_of(id as ModuleId).map(str::to_owned),
            content_info_map: content_info,
            languages_metadata_map: metadata,
            final_stats
        });
    }

    let (mut content_info_map, mut languages_metadata_map) = (content_info_map, languages_metadata_map);
    // After the total has been calculated from them, since the empty ones add nothing to it and
    // dropping them first would only make the same sum out of fewer entries
    remove_languages_with_0_files(&mut content_info_map, &mut languages_metadata_map);

    Ok(RunResult {
        content_info_map,
        languages_metadata_map,
        modules: modules_result,
        final_stats,
        faulty_files: std::mem::take(&mut faulty_files_ref.lock().unwrap()),
        files_present,
        scan_duration_millis: parsing_duration_millis,
        metrics,
        unreadable_dirs: std::mem::take(&mut unreadable_dirs.lock().unwrap())
    })
}

// The totals across every module, which is what the overview, the sum and the document's own
// language list are about: those questions are asked of the whole run and not of one part of it
fn merged_over_modules(per_module_content_info: &[HashMap<String,LanguageContentInfo>],
        per_module_metadata: &[HashMap<String,LanguageMetadata>])
-> (HashMap<String,LanguageContentInfo>, HashMap<String,LanguageMetadata>)
{
    let mut content_info = per_module_content_info[0].clone();
    let mut metadata = per_module_metadata[0].clone();
    for id in 1..per_module_content_info.len() {
        for (name, info) in &per_module_content_info[id] {
            content_info.get_mut(name).unwrap().add_content_info(info);
        }
        for (name, meta) in &per_module_metadata[id] {
            metadata.get_mut(name).unwrap().add_metadata(meta);
        }
    }

    (content_info, metadata)
}


// The roots and not every target: a target that lies inside another is reached by the walk of the
// one around it, and walking it again would count its files twice. Its module is not lost with it,
// it is what the boundary table hands back on the way down.
pub(crate) fn calculate_single_file_stats_or_add_to_injector(config: &EngineConfig, dirs_injector: &Arc<Injector<TraversedDir>>, files_injector: &Arc<Injector<ParsableFile>>,
        files_present: &mut FilesPresent, extension_lang_map: &HashMap<String, Arc<str>>, modules: &Modules)
{
    crate::engine::targets::topmost_targets(&config.dirs).iter().for_each(|target| {
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

pub(crate) fn remove_languages_with_0_files(content_info_map: &mut HashMap<String,LanguageContentInfo>,
    languages_metadata_map: &mut HashMap<String, LanguageMetadata>)
{
   let mut empty_languages = Vec::new();
   for element in languages_metadata_map.iter() {
       if element.1.files == 0 {
           empty_languages.push(element.0.to_owned());
       }
   }

   for ext in empty_languages {
       languages_metadata_map.remove(&ext);
       content_info_map.remove(&ext);
   }
}









fn generate_metrics_if_parsing_took_more_than_one_sec(parsing_duration_millis: u128, relevant_files: usize,
        content_info_map: &HashMap<String, LanguageContentInfo>) -> Option<Metrics>
{
    if parsing_duration_millis <= 1000 {
        return None;
    }

    let duration_secs = parsing_duration_millis as f32/ 1000f32;
    let mut total_lines = 0;
    content_info_map.iter().for_each(|x| total_lines += x.1.lines);
    let lines_per_sec = (total_lines as f32 / duration_secs) as usize;
    let files_per_sec = (relevant_files as f32 / duration_secs) as usize;

    Some(
        Metrics {
            files_per_sec,
            lines_per_sec
        }
    )
}





pub(crate) fn make_language_stats(languages_map: Arc<HashMap<String,Language>>, modules: usize) -> Vec<HashMap<String,LanguageContentInfo>> {
    let mut map = HashMap::<String,LanguageContentInfo>::new();
    for (key, value) in languages_map.iter() {
        map.insert(key.to_owned(), LanguageContentInfo::from(value));
    }
    vec![map; modules]
}

pub(crate) fn make_language_metadata(language_map: &Arc<HashMap<String,Language>>, modules: usize) -> Vec<HashMap<String, LanguageMetadata>> {
    let mut map = HashMap::<String,LanguageMetadata>::new();
    for name in language_map.keys() {
        map.insert(name.to_owned(), LanguageMetadata::default());
    }
    vec![map; modules]
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
    claims.iter().map(|(name, extensions)| ((*name).to_owned(), Language::new((*name).to_owned(),
            extensions.iter().map(|x| (*x).to_owned()).collect(),
            vec!["\"".to_owned()], vec!["//".to_owned()], None, None, Vec::new()))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;













    // The merge that ends a consumer reaches into these maps and unwraps, so a language or a module
    // that was never given an entry would kill the thread rather than miscount. Asserted here and
    // not through a run, since a result has had the empty languages removed from it by then.
    #[test]
    fn every_language_gets_a_bucket_in_every_module() {
        let languages = Arc::new(languages_claiming(&[("Rust", &["rs"]), ("Go", &["go"]), ("Zig", &["zig"])]));
        let modules = Modules::of(&[crate::engine::config::Target::named("backend", "./api".to_owned()),
                crate::engine::config::Target::named("frontend", "./web".to_owned()),
                crate::engine::config::Target::of("./docs".to_owned())]);
        assert_eq!(3, modules.count());

        let stats = make_language_stats(languages.clone(), modules.count());
        let metadata = make_language_metadata(&languages, modules.count());
        assert_eq!(modules.count(), stats.len());
        assert_eq!(modules.count(), metadata.len());

        for id in 0..modules.count() {
            for name in languages.keys() {
                assert!(stats[id].contains_key(name), "'{name}' has no content bucket in module {id}");
                assert!(metadata[id].contains_key(name), "'{name}' has no metadata bucket in module {id}");
            }
        }
    }

    #[test]
    fn test_FinalStats_creation() {
        let content_info_map = hashmap![
            "a".to_owned() => LanguageContentInfo::new(2000, 1400, 0, hashmap![]),
            "b".to_owned() => LanguageContentInfo::new(1000, 800, 0, hashmap![]),
            "c".to_owned() => LanguageContentInfo::new(1000, 800, 0, hashmap![])
        ];
        let languages_metadata_map = hashmap![
            "a".to_owned() => LanguageMetadata::new(20, 100000),
            "b".to_owned() => LanguageMetadata::new(10, 50000),
            "c".to_owned() => LanguageMetadata::new(10, 50000)
        ];
        let f = FinalStats::new(40, 4000, 3000, 0, 200000);
        let ef = FinalStats::new_extended(40, 4000, 3000, 0, 1000, 200000, 5000);
        let cf = FinalStats::calculate(&content_info_map, &languages_metadata_map);
        let customf = FinalStats {
            files: 40,
            lines: 4000,
            code_lines: 3000,
            comment_lines: 0,
            extra_lines: 1000,
            bytes_size: 200000,
            bytes_average_size: 5000,
            size: 200.0,
            size_measurement: "KBs".to_owned(),
            average_size: 5.0,
            average_size_measurement: "KBs".to_owned()
        };
        assert_eq!(customf, f);
        assert_eq!(customf, ef);
        assert_eq!(customf, cf);


        let content_info_map = hashmap![
            "a".to_owned() => LanguageContentInfo::new(2000, 1400, 0, hashmap![]),
            "b".to_owned() => LanguageContentInfo::new(1000, 800, 0, hashmap![]),
            "c".to_owned() => LanguageContentInfo::new(1000, 800, 0, hashmap![])
        ];
        let languages_metadata_map = hashmap![
            "a".to_owned() => LanguageMetadata::new(25, 1417403),
            "b".to_owned() => LanguageMetadata::new(12, 500000),
            "c".to_owned() => LanguageMetadata::new(12, 500000)
        ];
        let f = FinalStats::new(49, 4000, 3000, 0, 2417403);
        let ef = FinalStats::new_extended(49, 4000, 3000, 0, 1000, 2417403, 49334);
        let cf = FinalStats::calculate(&content_info_map, &languages_metadata_map);
        let customf = FinalStats {
            files: 49,
            lines: 4000,
            code_lines: 3000,
            comment_lines: 0,
            extra_lines: 1000,
            bytes_size: 2417403,
            bytes_average_size: 49334,
            size: 2.4,
            size_measurement: "MBs".to_owned(),
            average_size: 49.3,
            average_size_measurement: "KBs".to_owned()
        };
        assert_eq!(customf, f);
        assert_eq!(customf, ef);
        assert_eq!(customf, cf);
    }
}
