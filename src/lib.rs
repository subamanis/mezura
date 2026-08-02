#![forbid(unsafe_code)]

#![allow(non_snake_case)]

pub mod config_manager;
pub mod io_handler;
pub mod utils;
pub mod theme;
pub mod suggestions;
pub mod consumer;
pub mod producer;
pub mod message_printer;
pub mod file_parser;
pub mod phase_timing;
pub mod warnings;

mod result_printer;
mod json_printer;

pub use colored::{Color,Colorize,ColoredString};
pub use config_manager::{Configuration, SortCriterion};
pub use utils::*;
pub use domain::{Language, LanguageContentInfo, LanguageMetadata, FileStats, Keyword};

pub type FaultyFilesListMut = Arc<Mutex<Vec<FaultyFileDetails>>>;
pub type ExtensionLangMap = Arc<HashMap<String, Arc<str>>>;
// One bucket per module, and a run that declared none has exactly one, so that nothing downstream
// has two shapes to handle
pub type ContentInfoMapMut  = Arc<Mutex<Vec<HashMap<String,LanguageContentInfo>>>>;
pub type MetadataMapMut     = Arc<Mutex<Vec<HashMap<String,LanguageMetadata>>>>;

use directories::{BaseDirs,ProjectDirs};
use crossbeam_deque::{Worker,Injector};
use chrono::{DateTime, Local};
use std::{collections::HashMap, fs::{self, File}, io::Read, path::{Path, PathBuf}, sync::atomic::{AtomicBool, AtomicUsize, Ordering}, time::{Duration, Instant}};
use std::{sync::{Arc, LazyLock, Mutex, OnceLock}, thread::JoinHandle};


pub const APP_NAME : &str = "mezura";
pub const LANGUAGES_DIR_NAME : &str = "languages";
pub const THEMES_DIR_NAME : &str = "themes";
pub const CONFIG_DIR_NAME : &str = "config";
pub const LOGS_DIR_NAME : &str = "logs";
pub const TEST_DIR_NAME : &str = "test_dir";
pub const DEFAULT_CONFIG_NAME : &str = "default.txt";
pub const EXTENSION_PRIORITY_FILE_NAME : &str = "extension_priority.txt";
pub const MANIFEST_FILE_NAME : &str = "installed.txt";
pub const REPLACED_DIR_NAME : &str = "replaced";
// Marked rather than named, because naming it after its directory would be a lie: with
// './project tests=./project/tests' a row called 'project' is everything in it except the tests.
// Being one marked row is also what settles two unnamed targets ending in the same folder name.
// It says what the row is and not what is left in it, which is the only wording that holds in both
// shapes: with './project tests=./project/tests' the row really is the rest of the project, but
// with 'frontend=./web ./docs' the './docs' did not survive anything, it was simply never named.
pub const UNNAMED_MODULE_NAME : &str = "(unnamed)";

pub static PERSISTENT_APP_PATHS : LazyLock<PersistentAppPaths> = LazyLock::new(PersistentAppPaths::get);
pub static LOCAL_APP_PATHS : LazyLock<LocalAppPaths> = LazyLock::new(LocalAppPaths::get);
pub static CHANGELOG_BYTES : &[u8] = include_bytes!("../Changelog");


pub fn run(config: &Configuration, language_map: HashMap<String, Language>,
        extension_priority: &HashMap<String,Vec<String>>) -> Result<RunResult, ParseFilesError>
{
    let config = Arc::new(config.clone());
    let faulty_files_ref : FaultyFilesListMut  = Arc::new(Mutex::new(Vec::with_capacity(10)));
    let finish_condition_ref = Arc::new(AtomicBool::new(false));
    let language_map_ref = Arc::new(language_map);
    let (extension_map, extension_report) =
            make_extension_language_map(&language_map_ref, extension_priority, &config.forced_languages);
    extension_report.warnings().into_iter().for_each(warnings::emit);
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
    let exclude_matcher = Arc::new(build_exclude_matcher(&config.exclude_dirs)
            .expect("exclude patterns are validated during argument parsing"));
    calculate_single_file_stats_or_add_to_injector(&config, &dirs_injector, &files_injector, &mut files_present,
            &extension_lang_map, &modules);

    let files_stats = Arc::new(Mutex::new(files_present));

    let mut producer_handles = Vec::with_capacity(config.threads.producers);
    let mut consumer_handles = Vec::with_capacity(config.threads.consumers);

    if !config.hidden.directory_info && config.prints_text() {
        println!("\n{}...",theme::active().heading.paint("Analyzing directories"));
    }

    let parsing_started_instant = Instant::now();
    for i in 0..config.threads.producers {
        producer_handles.push(producer::start_producer_thread(i, files_injector.clone(), dirs_injector.clone(), Worker::new_fifo(),
            idle_producers.clone(), extension_lang_map.clone(), exclude_matcher.clone(),
            config.clone(), files_stats.clone(), modules.clone()));
    }
    for i in 0..config.threads.consumers {
        consumer_handles.push(consumer::start_parser_thread(i, files_injector.clone(), faulty_files_ref.clone(), finish_condition_ref.clone(),
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
    if relevant_files_num == 0 {
        return Ok(RunResult::of_nothing(files_present, parsing_duration_millis));
    }
    if !config.hidden.directory_info && config.prints_text() {
        println!("{}\n",theme::active().summary.paint(&format!("{} files found. {} of interest. {} excluded.",
                with_seperators(files_present.total_files), with_seperators(relevant_files_num),
                with_seperators(files_present.excluded_files))));
    }
    if !config.hidden.parsing_info && config.prints_text() {
        println!("{}...",theme::active().heading.paint("Parsing files"));
    }

    if faulty_files_ref.lock().unwrap().len() == relevant_files_num {
        return Err(ParseFilesError::AllAreFaultyFiles(std::mem::take(&mut faulty_files_ref.lock().unwrap())));
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
        metrics
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

// Everything that turns a result into something a person reads, kept out of 'run' so that the run
// itself is a function of its inputs. A caller that wants the numbers and not the report never calls
// this, and one that wants both gets the same result twice, since presenting reads and never writes.
pub fn present(result: &RunResult, config: &Configuration) {
    let datetime_now = chrono::Local::now();

    if result.files_present.relevant_files == 0 {
        // A machine consumer must not have to tell "no output" apart from "no code found", so the
        // document is written even here, whole and with everything zeroed
        if config.prints_text() {
            eprintln!("{}", ParseFilesError::NoRelevantFiles(get_activated_languages_as_str(config)).formatted());
        } else {
            json_printer::print_as_json(result, &datetime_now, config);
        }
        return;
    }

    print_faulty_files_or_ok(&result.faulty_files, config);

    if !config.prints_text() {
        json_printer::print_as_json(result, &datetime_now, config);
        return;
    }

    let log_file_path = get_specified_config_file_path(config);
    let existing_log_contents = log_file_path.as_ref().and_then(|path| extract_file_contents(path));
    result_printer::format_and_print_results(result, &existing_log_contents, &datetime_now, config);

    if config.log.should_log && let Some(path) = log_file_path
        && io_handler::log_stats(&path, &existing_log_contents, result, &datetime_now, config).is_err() {
        eprintln!("\n{}",theme::active().warning.paint("Error while trying to save the log."));
    }
}

//pub for integration tests
// The roots and not every target: a target that lies inside another is reached by the walk of the
// one around it, and walking it again would count its files twice. Its module is not lost with it,
// it is what the boundary table hands back on the way down.
pub fn calculate_single_file_stats_or_add_to_injector(config: &Configuration, dirs_injector: &Arc<Injector<TraversedDir>>, files_injector: &Arc<Injector<ParsableFile>>,
        files_present: &mut FilesPresent, extension_lang_map: &HashMap<String, Arc<str>>, modules: &Modules)
{
    utils::topmost_targets(&config.dirs).iter().for_each(|target| {
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

//pub for integration tests
pub fn remove_languages_with_0_files(content_info_map: &mut HashMap<String,LanguageContentInfo>,
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

// The module a file was counted under, carried through the queue as an index and never as a name.
// A composite string key would be an allocation on every single file, which is what the whole
// performance work of v3 was spent removing.
pub type ModuleId = u16;

// The names, in the order they were declared, and the places where the walk changes its mind about
// which module it is in.
//
// The module of a directory is decided once, on the way in, and its children inherit it, so a run
// that nests nothing looks up nothing at all: every root carries its own module and the walk never
// asks again. Only a target that lies inside another target can change the answer part way down,
// and those are the only paths this table holds.
#[derive(Debug,Default)]
pub struct Modules {
    // Empty when nothing was named, and then everything belongs to the single bucket 0
    names: Vec<Option<String>>,
    dir_boundaries: HashMap<String, ModuleId>,
    file_boundaries: HashMap<String, ModuleId>
}

impl Modules {
    // The boundaries are built from the resolved paths and never from what was typed, because
    // 'starts_with' and an equality on a path are case sensitive on every platform: on Windows a
    // 'frontend=./Web' over a real './web' would match nothing, the module would come out empty and
    // every file would fall into '(unnamed)' with nothing printed to say why.
    pub fn of(targets: &[config_manager::Target]) -> Self {
        if targets.iter().all(|x| x.module.is_none()) {
            return Modules::default();
        }

        let mut names : Vec<Option<String>> = Vec::new();
        for target in targets {
            if !names.contains(&target.module) {
                names.push(target.module.clone());
            }
        }
        // The unnamed one is a row like any other, and it is last because it is the leftover
        if let Some(position) = names.iter().position(Option::is_none) {
            let unnamed = names.remove(position);
            names.push(unnamed);
        }

        let roots = utils::topmost_targets(targets);
        let mut modules = Modules { names, ..Default::default() };
        for target in targets {
            if roots.contains(target) {
                continue;
            }
            let id = modules.id_of(&target.module);
            let key = utils::path_comparison_key(target.path.trim_end_matches('/'));
            if Path::new(&target.path).is_dir() {
                modules.dir_boundaries.insert(key, id);
            } else {
                modules.file_boundaries.insert(key, id);
            }
        }

        modules
    }

    fn id_of(&self, module: &Option<String>) -> ModuleId {
        self.names.iter().position(|x| x == module).unwrap_or(0) as ModuleId
    }

    pub fn count(&self) -> usize {
        self.names.len().max(1)
    }

    pub fn is_used(&self) -> bool {
        !self.names.is_empty()
    }

    pub fn name_of(&self, id: ModuleId) -> Option<&str> {
        self.names.get(id as usize).and_then(|x| x.as_deref())
    }

    // The module of a target, for the roots the traversal is handed
    pub fn of_target(&self, target: &config_manager::Target) -> ModuleId {
        if self.is_used() {self.id_of(&target.module)} else {0}
    }

    pub fn has_dir_boundaries(&self) -> bool {
        !self.dir_boundaries.is_empty()
    }

    pub fn has_file_boundaries(&self) -> bool {
        !self.file_boundaries.is_empty()
    }

    // Called only when the run declared a target inside another one, which is the only way a child
    // can belong somewhere other than where its parent does
    pub fn at_dir(&self, path: &Path, inherited: ModuleId) -> ModuleId {
        self.at(&self.dir_boundaries, path, inherited)
    }

    pub fn at_file(&self, path: &Path, inherited: ModuleId) -> ModuleId {
        self.at(&self.file_boundaries, path, inherited)
    }

    fn at(&self, boundaries: &HashMap<String, ModuleId>, path: &Path, inherited: ModuleId) -> ModuleId {
        let Some(path) = path.to_str() else { return inherited };
        boundaries.get(&utils::path_comparison_key(&path.replace('\\', "/"))).copied().unwrap_or(inherited)
    }
}

// An extension claimed by more than one language, and how that was settled. The three outcomes are
// not equally trustworthy and must never read alike: the first two are decisions somebody took, the
// third is a tiebreak nobody asked for, and it is the one that can put a language's comments into
// another language's 'code'.
#[derive(Debug,PartialEq,Eq,Clone,Copy)]
pub enum ResolvedBy {
    ForceLang,
    PriorityFile,
    AlphabeticalFallback
}

#[derive(Debug,PartialEq,Eq,Clone)]
pub struct ExtensionCollision {
    pub extension: String,
    pub winner: String,
    pub losers: Vec<String>,
    pub resolved_by: ResolvedBy
}

#[derive(Debug,PartialEq,Eq,Clone,Default)]
pub struct ExtensionReport {
    pub collisions: Vec<ExtensionCollision>,
    pub unknown_forced_languages: Vec<(String,String)>
}

impl ExtensionReport {
    // Only the tiebreak is reported. A collision that the priority file or '--force-lang' settled is
    // a decision somebody took on purpose, and printing it on every run would turn the whole notice
    // into noise that hides the one line that matters.
    //
    // One warning per contested extension rather than one for the lot, because each names a
    // different extension and that is what a reader of the document wants to key on. What reaches
    // the terminal is unchanged: the blocks were joined by a blank line, and separate lines each
    // carrying a leading one produce the same text.
    //
    // Returned as values and emitted by the caller, so that what a report is worth can be tested
    // without going through the collector that the whole process shares.
    pub fn warnings(&self) -> Vec<warnings::Warning> {
        let mut reported = Vec::new();
        for collision in self.collisions.iter().filter(|x| x.resolved_by == ResolvedBy::AlphabeticalFallback) {
            reported.push(warnings::Warning::new(warnings::EXTENSION_TIEBREAK, warnings::Affects::Counts, &collision.extension,
                    format!("The extension '{}' is claimed by {} and {}. It was given to {} only because that name comes first \
alphabetically, so the files of the rest are counted with the wrong comment and string symbols.\nDeclare it in '{}', or run with '--force-lang {}=<language>'.",
                    collision.extension, collision.winner, collision.losers.join(", "), collision.winner,
                    EXTENSION_PRIORITY_FILE_NAME, collision.extension)));
        }

        for (extension, wanted) in &self.unknown_forced_languages {
            reported.push(warnings::Warning::new(warnings::UNKNOWN_FORCED_LANGUAGE, warnings::Affects::Settings, extension,
                    format!("'--force-lang {extension}={wanted}' names a language that is not available, so the extension was left as it was.")));
        }

        reported
    }
}

// Longer than any extension that exists, and the buffer that keeps the case-insensitive lookup from
// allocating once per file
const MAX_EXTENSION_LEN : usize = 24;

// Extensions are matched without regard to case, so the keys are lowercased here, once, and the
// lookup lowercases what it is given. This has to happen before the claimants are counted: with the
// declarations left as they were written, 'cs' and 'CS' would look like two different extensions,
// would never be found to collide, and would each win silently in different files.
pub fn make_extension_language_map(languages: &HashMap<String,Language>, priority: &HashMap<String,Vec<String>>,
        forced: &HashMap<String,String>) -> (HashMap<String, Arc<str>>, ExtensionReport)
{
    let mut names = languages.keys().collect::<Vec<_>>();
    names.sort_unstable();

    let shared_names : HashMap<&str, Arc<str>> = names.iter()
            .map(|name| (name.as_str(), Arc::from(name.as_str())))
            .collect();

    // Normalised once, so that the two places that consult it cannot disagree about the shape of a
    // key. A caller of the library sets this field directly and is under no obligation to lowercase
    // it, and when only one of the two lookups did, the mapping was applied while the run also
    // warned that the extension had been left to the alphabetical tiebreak.
    let forced : HashMap<String, &str> = forced.iter()
            .map(|(extension, language)| (extension.to_ascii_lowercase(), language.as_str()))
            .collect();
    // Searched in the sorted order the names already have, and not through the keys of a map, whose
    // iteration order is arbitrary: two languages whose names differ only in case would otherwise
    // resolve to a different one of the two between runs of the same command.
    let language_named = |wanted: &str| names.iter().find(|name| name.eq_ignore_ascii_case(wanted)).map(|x| x.as_str());

    let mut claimants : HashMap<String, Vec<&str>> = HashMap::with_capacity(languages.len() * 2);
    for name in &names {
        for extension in &languages[*name].extensions {
            claimants.entry(extension.to_ascii_lowercase()).or_default().push(name.as_str());
        }
    }

    let mut map : HashMap<String, Arc<str>> = HashMap::with_capacity(claimants.len());
    let mut report = ExtensionReport::default();

    for (extension, claimants) in claimants {
        let forced_winner = forced.get(&extension).and_then(|wanted| language_named(wanted));
        let priority_winner = priority.get(&extension)
                .and_then(|order| order.iter()
                        .find_map(|wanted| claimants.iter().find(|name| name.eq_ignore_ascii_case(wanted)))
                        .copied());

        // The winner and the mechanism that chose it are decided in one place, because deriving the
        // second from "is there a rule for this extension" is not the same question. A rule naming a
        // language that does not claim the extension, because it was renamed, removed or misspelled,
        // settles nothing: the tiebreak decides, and reporting it as settled hides exactly the case
        // this whole mechanism exists to announce.
        // The claimants were pushed in the order the sorted names were walked, so the first of them
        // is the alphabetical winner this has always fallen back to.
        let (winner, resolved_by) = match (forced_winner, priority_winner) {
            (Some(x), _) => (x, ResolvedBy::ForceLang),
            (_, Some(x)) => (x, ResolvedBy::PriorityFile),
            _ => (claimants[0], ResolvedBy::AlphabeticalFallback)
        };

        if claimants.len() > 1 {
            report.collisions.push(ExtensionCollision {
                extension: extension.clone(),
                winner: winner.to_owned(),
                losers: claimants.iter().filter(|name| **name != winner).map(|name| (*name).to_owned()).collect(),
                resolved_by
            });
        }

        map.insert(extension, shared_names[winner].clone());
    }

    // '--force-lang txt=python' is meant to work whether or not anything else claims the extension,
    // so a forced entry that no language claims is added rather than ignored
    for (extension, wanted) in &forced {
        match language_named(wanted) {
            Some(name) => { map.insert(extension.clone(), shared_names[name].clone()); },
            None => report.unknown_forced_languages.push((extension.clone(), (*wanted).to_owned()))
        }
    }

    report.collisions.sort_by(|a, b| a.extension.cmp(&b.extension));
    report.unknown_forced_languages.sort();
    (map, report)
}

pub fn find_language_of_extension(extension_lang_map: &HashMap<String, Arc<str>>, extension: &str) -> Option<Arc<str>> {
    if let Some(x) = extension_lang_map.get(extension) {
        return Some(x.clone());
    }

    // Every key is already lowercase, so anything that is too, has simply not been found
    if !extension.bytes().any(|b| b.is_ascii_uppercase()) || extension.len() > MAX_EXTENSION_LEN {
        return None;
    }

    let mut buffer = [0u8; MAX_EXTENSION_LEN];
    let length = extension.len();
    buffer[..length].copy_from_slice(extension.as_bytes());
    buffer[..length].make_ascii_lowercase();
    std::str::from_utf8(&buffer[..length]).ok()
            .and_then(|lowercased| extension_lang_map.get(lowercased))
            .cloned()
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


// Hiding the status never hides a parsing failure: that would show wrong numbers with nothing
// to indicate it
pub fn print_faulty_files_or_ok(faulty_files: &[FaultyFileDetails], config: &Configuration) {
    if faulty_files.is_empty() {
        if !config.hidden.parsing_info && config.prints_text() {
            println!("{}\n",theme::active().success.paint("ok"));
        }
    } else {
        // A JSON run reports them inside the document as well, but they are a mistake and belong on
        // the error output in every case, where '--hide' can never suppress them
        let error = &theme::active().error;
        eprintln!("{} {}",error.paint(&faulty_files.len().to_string()), error.paint("faulty files detected. They will be ignored in stat calculation."));
        if config.should_show_faulty_files {
            for f in faulty_files {
                eprintln!("-- Error: {} \n   for file: {}\n",f.error_msg,f.path);
            }
        } else {
            eprintln!("Run with command '--{}' to get detailed info.",config_manager::SHOW_FAULTY_FILES)
        }
        eprintln!();
    }
}


fn get_activated_languages_as_str(config: &Configuration) -> String {
    let mut msg = if config.languages_of_interest.is_empty() {
        String::new()
    } else {
        String::from("\n(Activated languages: ") + &config.languages_of_interest.join(", ") + ")"
    }
    ;
    let other = if config.excluded_languages.is_empty() {
        String::new()
    } else {
        String::from("\n(Excluded languages: ") + &config.excluded_languages.join(", ") + ")"
    };

    msg += &other;
    msg
}

pub fn make_language_stats(languages_map: Arc<HashMap<String,Language>>, modules: usize) -> Vec<HashMap<String,LanguageContentInfo>> {
    let mut map = HashMap::<String,LanguageContentInfo>::new();
    for (key, value) in languages_map.iter() {
        map.insert(key.to_owned(), LanguageContentInfo::from(value));
    }
    vec![map; modules]
}

pub fn make_language_metadata(language_map: &Arc<HashMap<String,Language>>, modules: usize) -> Vec<HashMap<String, LanguageMetadata>> {
    let mut map = HashMap::<String,LanguageMetadata>::new();
    for name in language_map.keys() {
        map.insert(name.to_owned(), LanguageMetadata::default());
    }
    vec![map; modules]
}

fn get_specified_config_file_path(config: &Configuration) -> Option<String> {
    if let Some(name) = &config.config_name_to_save {
        Some(PERSISTENT_APP_PATHS.logs_dir.clone() + name)
    } else { config.config_name_to_load.as_ref().map(|name| PERSISTENT_APP_PATHS.logs_dir.clone() + name) }
}

// Used to display colorful errors and warnings, by implementing it on Error enums.
pub trait Formatted {
    fn formatted(&self) -> ColoredString;
}

#[derive(Debug)]
pub struct PersistentAppPaths {
    pub project_path: String,
    pub data_dir: String,
    pub languages_dir: String,
    pub themes_dir: String,
    pub config_dir: String,
    pub logs_dir: String,
    pub are_initialized: bool
}

#[derive(Debug)]
pub struct LocalAppPaths {
    pub data_dir: String,
    pub languages_dir: String,
    pub config_dir: String,
    pub test_dir: String,
    pub test_config_dir: String,
    pub test_log_dir: String,
}

#[derive(Debug)]
pub struct Metrics {
    pub files_per_sec: usize,
    pub lines_per_sec: usize
}

// What one run produces, and the only thing 'run' returns. Presentation is a separate call, so the
// same result can be printed, written as JSON, compared with another one, or read by a caller that
// wants none of those.
#[derive(Debug)]
pub struct RunResult {
    // The totals across every module. A run that named none has exactly one module holding the same
    // numbers, and reading these is what every question about the whole run goes through.
    pub content_info_map: HashMap<String, LanguageContentInfo>,
    pub languages_metadata_map: HashMap<String, LanguageMetadata>,
    pub modules: Vec<ModuleResult>,
    pub final_stats: FinalStats,
    pub faulty_files: Vec<FaultyFileDetails>,
    pub files_present: FilesPresent,
    pub scan_duration_millis: u128,
    pub metrics: Option<Metrics>
}

// One part of the run, counted on its own. 'name' is None for the leftovers of the named ones, which
// is also the single unnamed one of a run that declared no modules at all.
#[derive(Debug)]
pub struct ModuleResult {
    pub name: Option<String>,
    pub content_info_map: HashMap<String, LanguageContentInfo>,
    pub languages_metadata_map: HashMap<String, LanguageMetadata>,
    pub final_stats: FinalStats
}

impl RunResult {
    // Nothing of interest was found, which is an answer and not a failure: the counts are zero and
    // the file numbers still say how many were looked at and how many were excluded.
    fn of_nothing(files_present: FilesPresent, scan_duration_millis: u128) -> Self {
        RunResult {
            content_info_map: HashMap::new(),
            languages_metadata_map: HashMap::new(),
            modules: Vec::new(),
            final_stats: FinalStats::new_extended(0, 0, 0, 0, 0, 0, 0),
            faulty_files: Vec::new(),
            files_present,
            scan_duration_millis,
            metrics: None
        }
    }

    // Whether the report has a second axis at all. One name is enough for the column to appear, and
    // without one there is nothing to group by and the output is what it always was.
    pub fn has_modules(&self) -> bool {
        self.modules.iter().any(|x| x.name.is_some())
    }
}

// 'extra_lines' is what is left after the code and the comments: blank lines, and lines that the
// language required but that say nothing, like a closing brace. The three add up to 'lines'.
#[derive(Debug, PartialEq)]
pub struct FinalStats {
    files: usize,
    lines: usize,
    code_lines: usize,
    comment_lines: usize,
    extra_lines: usize,
    bytes_size: usize,
    bytes_average_size: usize,
    size: f64,
    size_measurement: String,
    average_size: f64,
    average_size_measurement: String
}

#[derive(Debug)]
pub struct FaultyFileDetails {
    path: String,
    error_msg: String,
    size: u64
}

// The failure carries the faulty files with it, so that the report of what went wrong is printed by
// whoever is doing the printing, and 'run' does not have to print on its way out
#[derive(Debug)]
pub enum ParseFilesError {
    NoRelevantFiles(String),
    AllAreFaultyFiles(Vec<FaultyFileDetails>)
}

#[derive(Debug,Default,Clone,Copy)]
pub struct FilesPresent {
    pub total_files: usize,
    pub relevant_files: usize,
    pub excluded_files: usize
}

#[derive(Debug,Clone)]
pub struct ParsableFile {
    pub path: PathBuf,
    pub language_name: Arc<str>,
    pub module: ModuleId
}

#[derive(Debug,Clone)]
pub struct TraversedDir {
    pub path: PathBuf,
    pub gitignore_stack: Option<Arc<GitignoreStack>>,
    pub module: ModuleId
}

#[derive(Debug)]
pub struct GitignoreStack {
    matcher: ignore::gitignore::Gitignore,
    parent: Option<Arc<GitignoreStack>>
}


// Returns false both when the dir doesn't exist and when it exists but is empty.
pub fn dir_contains_entries(path: &str) -> bool {
    fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_some())
}

impl PersistentAppPaths {
    //Persistent paths:
    // Windows:  C:/Users/<user_name>/AppData/Roaming/mezura
    // Linux:    /home/<user_name>/.local/share/mezura
    // MacOs:    /Users/<user_name>/Library/Application Support/mezura
    pub fn get() -> Self {
        let proj_dirs = ProjectDirs::from("", "",  APP_NAME).unwrap();
        let project_path = BaseDirs::new().unwrap().data_dir().to_str().unwrap().to_owned() + "/" + APP_NAME;
        // A test writes real configuration and theme files through these paths, and one that is
        // interrupted before its cleanup leaves them behind. In the real directory that is not
        // litter: the leftovers are loadable configurations that '--show-configs' lists, and
        // 'test_save_load_configs' begins by demanding that its own file is absent, so a single
        // interrupted run makes it fail on every run after it until the file is deleted by hand.
        // Pointing the whole thing at a temporary directory also stops the machine's own default
        // configuration from taking part in the tests, which is what made them differ per machine.
        let data_dir = if cfg!(test) {
            std::env::temp_dir().join(APP_NAME.to_owned() + "-test").to_string_lossy().into_owned() + "/"
        } else {
            proj_dirs.data_dir().to_str().unwrap().to_owned() + "/"
        };
        let languages_dir = data_dir.clone() + LANGUAGES_DIR_NAME + "/";
        let config_dir = data_dir.clone() + CONFIG_DIR_NAME + "/";
        let logs_dir = data_dir.clone() + LOGS_DIR_NAME + "/";
        // The existence of the project dir alone means nothing, since any part of the program (or the test
        // suite) that touches these paths can create it. The baked-in data must actually be present, otherwise
        // a half-created dir would be mistaken for a valid installation and every run would fail.
        let are_initialized = dir_contains_entries(&languages_dir) && Path::new(&config_dir).exists()
                && Path::new(&logs_dir).exists();

        PersistentAppPaths {
            project_path,
            themes_dir: data_dir.clone() + THEMES_DIR_NAME + "/",
            data_dir,
            config_dir,
            languages_dir,
            logs_dir,
            are_initialized
        }
    }
}

impl LocalAppPaths {
    // Paths that exist inside the repository folder
    pub fn get() -> Self {
        let mut working_dir = String::from(std::env::current_exe().expect("Failed to find executable path.")
            .parent().expect("Failed to get parent directory of the executable.").to_str().unwrap());
        if working_dir.contains("target/") || working_dir.contains("target\\"){
            working_dir = String::from(".");
        }

        let data_dir =  working_dir + "/data/";

        LocalAppPaths {
            data_dir: data_dir.clone(),
            languages_dir: data_dir.clone() + LANGUAGES_DIR_NAME + "/",
            config_dir: data_dir.clone() + CONFIG_DIR_NAME + "/",
            test_dir: data_dir.clone() + "../" + TEST_DIR_NAME + "/",
            test_config_dir: data_dir.clone() + "../" + TEST_DIR_NAME + "/config/",
            test_log_dir: data_dir + "../" + TEST_DIR_NAME + "/logs/"
        }
    }
}

impl Formatted for ParseFilesError {
    fn formatted(&self) -> ColoredString {
        match self {
            Self::NoRelevantFiles(x) => theme::active().warning.paint(&format!("{} {}","No relevant files found in the given directory.", x)),
            Self::AllAreFaultyFiles(_) => theme::active().warning.paint("None of the files were able to be parsed")
        }
    }
}

impl FinalStats {
    pub fn new(files: usize, lines: usize, code_lines: usize, comment_lines: usize, bytes_size: usize) -> Self
    {
        let bytes_average_size = bytes_size / files;
        let (size, size_measurement) = FinalStats::get_formatted_size_and_measurement(bytes_size);
        let size = round_1(size);
        let (average_size, average_size_measurement) = Self::get_formatted_size_and_measurement(bytes_average_size);
        let average_size = round_1(average_size);
        FinalStats {
            files,
            lines,
            code_lines,
            comment_lines,
            extra_lines: lines - code_lines - comment_lines,
            bytes_size,
            bytes_average_size,
            size,
            size_measurement,
            average_size,
            average_size_measurement,
        }
    }

    pub fn new_extended(files: usize, lines: usize, code_lines: usize, comment_lines: usize, extra_lines: usize,
            bytes_size: usize, bytes_average_size: usize) -> Self {
        let (size, size_measurement) = FinalStats::get_formatted_size_and_measurement(bytes_size);
        let size = round_1(size);
        let (average_size, average_size_measurement) = Self::get_formatted_size_and_measurement(bytes_average_size);
        let average_size = round_1(average_size);

        FinalStats {
            files,
            lines,
            code_lines,
            comment_lines,
            extra_lines,
            bytes_size,
            bytes_average_size,
            size,
            size_measurement,
            average_size,
            average_size_measurement,
        }
    }

    pub fn calculate(content_info_map: &HashMap<String,LanguageContentInfo>, languages_metadata_map: &HashMap<String,LanguageMetadata>) -> Self {
        let (mut total_files, mut total_lines, mut total_code_lines, mut total_comment_lines, mut total_bytes) = (0, 0, 0, 0, 0);
        languages_metadata_map.values().for_each(|e| {total_files += e.files; total_bytes += e.bytes});
        content_info_map.values().for_each(|c| {total_lines += c.lines; total_code_lines += c.code_lines;
                total_comment_lines += c.comment_lines});
        let bytes_size = total_bytes;
        let bytes_average_size = total_bytes / total_files;
        let (total_size, size_measurement) = Self::get_formatted_size_and_measurement(total_bytes);
        let (average_size, average_size_measurement) = Self::get_formatted_size_and_measurement(bytes_average_size);
        let total_size = round_1(total_size);
        let average_size = round_1(average_size);


        FinalStats {
            files: total_files,
            lines: total_lines,
            code_lines: total_code_lines,
            comment_lines: total_comment_lines,
            extra_lines: total_lines - total_code_lines - total_comment_lines,
            bytes_size,
            bytes_average_size,
            size: total_size,
            size_measurement,
            average_size,
            average_size_measurement
        }
    }

    fn get_formatted_size_and_measurement(value: usize) -> (f64, String) {
        if value >= 1000000000 {(value as f64 / 1000000000f64, "GBs".to_owned())}
        else if value >= 1000000 {(value as f64 / 1000000f64, "MBs".to_owned())}
        else if value >= 1000 {(value as f64 / 1000f64, "KBs".to_owned())}
        else {(value as f64, "Bytes".to_owned())}
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


pub mod domain {
    use super::*;

    #[derive(Debug, Clone)]
    pub struct Language {
        pub name: String,
        pub extensions : Vec<String>,
        pub string_symbols : Vec<String>,
        pub comment_symbols : Vec<String>,
        pub multiline_comment_start_symbol : Option<String>,
        pub multiline_comment_end_symbol : Option<String>,
        pub keywords : Vec<Keyword>,
        pub scan_plan : OnceLock<crate::file_parser::ScanPlan>
    }

    impl PartialEq for Language {
        fn eq(&self, other: &Self) -> bool {
            self.name == other.name
                && self.extensions == other.extensions
                && self.string_symbols == other.string_symbols
                && self.comment_symbols == other.comment_symbols
                && self.multiline_comment_start_symbol == other.multiline_comment_start_symbol
                && self.multiline_comment_end_symbol == other.multiline_comment_end_symbol
                && self.keywords == other.keywords
        }
    }

    #[derive(Debug,PartialEq)]
    pub struct Keyword{
        pub descriptive_name : String,
        pub aliases : Vec<String>
    }

    #[derive(Debug,PartialEq,Clone)]
    pub struct LanguageContentInfo {
        pub lines : usize,
        pub code_lines : usize,
        pub comment_lines : usize,
        pub keyword_occurences : HashMap<String,usize>
    }

    #[derive(Debug,PartialEq,Default,Clone)]
    pub struct LanguageMetadata {
        pub files: usize,
        pub bytes: usize
    }

    #[derive(Debug,PartialEq,Default)]
    pub struct FileStats {
        pub lines : usize,
        pub code_lines : usize,
        pub comment_lines : usize,
        pub keyword_occurences : Vec<usize>
    }

    impl Clone for Keyword {
        fn clone(&self) -> Self {
            Keyword {
                descriptive_name : self.descriptive_name.to_owned(),
                aliases : self.aliases.to_owned()
            }
        }
    }

    impl Language {
        pub fn new(name: String, extensions: Vec<String>, string_symbols: Vec<String>, comment_symbols: Vec<String>,
            multiline_comment_start_symbol: Option<String>, multiline_comment_end_symbol: Option<String>,
            keywords: Vec<Keyword>) -> Self
        {
            Language {
                name,
                extensions,
                string_symbols,
                comment_symbols,
                multiline_comment_start_symbol,
                multiline_comment_end_symbol,
                keywords,
                scan_plan : OnceLock::new()
            }
        }

        pub fn multiline_start_len(&self) -> usize {
            if let Some(x) = &self.multiline_comment_start_symbol {
                x.len()
            } else {
                0
            }
        }

        pub fn multiline_end_len(&self) -> usize {
            if let Some(x) = &self.multiline_comment_end_symbol {
                x.len()
            } else {
                0
            }
        }

        pub fn supports_multiline_comments(&self) -> bool {
            self.multiline_comment_start_symbol.is_some()
        }
    }

    impl LanguageContentInfo {
        pub fn new(lines: usize, code_lines: usize, comment_lines: usize, keyword_occurences: HashMap<String,usize>) -> Self {
            LanguageContentInfo {
                lines,
                code_lines,
                comment_lines,
                keyword_occurences
            }
        }

        pub fn dummy(lines: usize) -> LanguageContentInfo {
            LanguageContentInfo {
                lines,
                code_lines: 0,
                comment_lines: 0,
                keyword_occurences: HashMap::new()
            }
        }

        pub fn add_file_stats(&mut self, other: FileStats, keywords: &[Keyword]) {
            self.lines += other.lines;
            self.code_lines += other.code_lines;
            self.comment_lines += other.comment_lines;
            for (keyword_index, occurrences) in other.keyword_occurences.iter().enumerate() {
                if *occurrences > 0 {
                    *self.keyword_occurences.get_mut(&keywords[keyword_index].descriptive_name).unwrap() += *occurrences;
                }
            }
        }

        pub fn from_file_stats(stats: FileStats, keywords: &[Keyword]) -> LanguageContentInfo {
            let mut keyword_occurences = HashMap::<String,usize>::new();
            for (keyword_index, occurrences) in stats.keyword_occurences.iter().enumerate() {
                keyword_occurences.insert(keywords[keyword_index].descriptive_name.clone(), *occurrences);
            }
            LanguageContentInfo {
                lines : stats.lines,
                code_lines : stats.code_lines,
                comment_lines : stats.comment_lines,
                keyword_occurences
            }
        }

        pub fn add_content_info(&mut self, other: &LanguageContentInfo) {
            self.lines += other.lines;
            self.code_lines += other.code_lines;
            self.comment_lines += other.comment_lines;
            for (k,v) in other.keyword_occurences.iter() {
                *self.keyword_occurences.get_mut(k).unwrap() += *v;
            }
        }
    }

    impl From<&Language> for LanguageContentInfo {
        fn from(ext: &Language) -> Self {
            LanguageContentInfo {
                lines : 0,
                code_lines : 0,
                comment_lines : 0,
                keyword_occurences : get_keyword_stats_map(ext)
            }
        }
    }

    impl LanguageMetadata {
        pub fn new(files: usize, bytes: usize) ->  Self {
            LanguageMetadata {
                files,
                bytes
            }
        }

        pub fn add_file_meta(&mut self, bytes: usize) {
            self.files += 1;
            self.bytes += bytes;
        }

        pub fn add_metadata(&mut self, other_metadata: &LanguageMetadata) {
            self.files += other_metadata.files;
            self.bytes += other_metadata.bytes;
        }
    }

    impl FileStats {
        pub fn with_keywords(keywords: &[Keyword]) -> Self {
            FileStats {
                lines : 0,
                code_lines : 0,
                comment_lines : 0,
                keyword_occurences : vec![0; keywords.len()]
            }
        }

        pub fn incr_lines(&mut self) {
            self.lines += 1;
        }

        pub fn incr_code_lines(&mut self) {
            self.code_lines += 1;
        }

        pub fn incr_comment_lines(&mut self) {
            self.comment_lines += 1;
        }

        pub fn incr_keyword(&mut self, keyword_index: usize) {
            self.keyword_occurences[keyword_index] += 1;
        }
    }

    fn get_keyword_stats_map(extension: &Language) -> HashMap<String,usize> {
        let mut map = HashMap::<String,usize>::new();
        for k in &extension.keywords {
            map.insert(k.descriptive_name.to_owned(), 0);
        }
        map
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn languages_claiming(claims: &[(&str, &[&str])]) -> HashMap<String, Language> {
        claims.iter().map(|(name, extensions)| ((*name).to_owned(), Language::new((*name).to_owned(),
                extensions.iter().map(|x| (*x).to_owned()).collect(),
                vec!["\"".to_owned()], vec!["//".to_owned()], None, None, Vec::new()))).collect()
    }

    fn priority(rules: &[(&str, &[&str])]) -> HashMap<String,Vec<String>> {
        rules.iter().map(|(extension, order)| ((*extension).to_owned(),
                order.iter().map(|x| (*x).to_owned()).collect())).collect()
    }

    fn winner_of(map: &HashMap<String, Arc<str>>, extension: &str) -> String {
        find_language_of_extension(map, extension).map(|x| x.to_string()).unwrap_or_default()
    }

    #[test]
    fn an_extension_that_only_one_language_claims_is_never_reported() {
        let languages = languages_claiming(&[("Rust", &["rs"]), ("Go", &["go"])]);
        let (map, report) = make_extension_language_map(&languages, &HashMap::new(), &HashMap::new());

        assert_eq!("Rust", winner_of(&map, "rs"));
        assert_eq!("Go", winner_of(&map, "go"));
        assert_eq!(ExtensionReport::default(), report);
        assert!(report.warnings().is_empty());
    }

    // The tiebreak is the outcome nobody chose, and the only one that is announced
    #[test]
    fn a_contested_extension_falls_back_to_the_first_name_alphabetically_and_says_so() {
        let languages = languages_claiming(&[("Objective-C", &["m", "mm"]), ("MATLAB", &["m"])]);
        let (map, report) = make_extension_language_map(&languages, &HashMap::new(), &HashMap::new());

        assert_eq!("MATLAB", winner_of(&map, "m"));
        assert_eq!("Objective-C", winner_of(&map, "mm"));
        assert_eq!(vec![ExtensionCollision {
            extension: "m".to_owned(),
            winner: "MATLAB".to_owned(),
            losers: vec!["Objective-C".to_owned()],
            resolved_by: ResolvedBy::AlphabeticalFallback
        }], report.collisions);
        assert_eq!(vec![(warnings::EXTENSION_TIEBREAK, "counts")],
                report.warnings().iter().map(|x| (x.code, x.affects.name())).collect::<Vec<_>>());
    }

    #[test]
    fn the_priority_file_decides_it_and_force_lang_overrules_the_priority_file() {
        let languages = languages_claiming(&[("Objective-C", &["m"]), ("MATLAB", &["m"])]);

        let (map, report) = make_extension_language_map(&languages, &priority(&[("m", &["Objective-C", "MATLAB"])]), &HashMap::new());
        assert_eq!("Objective-C", winner_of(&map, "m"));
        assert_eq!(ResolvedBy::PriorityFile, report.collisions[0].resolved_by);
        assert_eq!(vec!["MATLAB".to_owned()], report.collisions[0].losers);

        let forced = hashmap!("m".to_owned() => "matlab".to_owned());
        let (map, report) = make_extension_language_map(&languages, &priority(&[("m", &["Objective-C", "MATLAB"])]), &forced);
        assert_eq!("MATLAB", winner_of(&map, "m"));
        assert_eq!(ResolvedBy::ForceLang, report.collisions[0].resolved_by);

        // and neither of them is the tiebreak, so neither is announced
        assert!(report.warnings().is_empty());
    }

    // A rule whose every name has been renamed away, removed or misspelled settles nothing, and the
    // tiebreak is what decides. Reporting it as settled left the user believing their rule was in
    // force while the extension quietly went elsewhere, with nothing printed.
    #[test]
    fn a_priority_rule_that_names_no_claimant_falls_through_to_the_tiebreak_and_says_so() {
        let languages = languages_claiming(&[("MATLAB", &["m"]), ("Objective-C", &["m"])]);
        let (map, report) = make_extension_language_map(&languages, &priority(&[("m", &["ObjC"])]), &HashMap::new());

        assert_eq!("MATLAB", winner_of(&map, "m"));
        assert_eq!(ResolvedBy::AlphabeticalFallback, report.collisions[0].resolved_by);
        let reported = report.warnings();
        assert_eq!(warnings::EXTENSION_TIEBREAK, reported[0].code);
        assert_eq!("m", reported[0].subject);
        assert!(reported[0].message.contains("only because"));
    }

    // A name in the priority file that no longer exists is skipped rather than left to win nothing
    #[test]
    fn the_priority_file_moves_on_to_the_next_name_when_the_first_is_not_there() {
        let languages = languages_claiming(&[("Prolog", &["pl"]), ("Raku", &["pl"])]);
        let (map, _) = make_extension_language_map(&languages, &priority(&[("pl", &["Perl", "Raku", "Prolog"])]), &HashMap::new());

        assert_eq!("Raku", winner_of(&map, "pl"));
    }

    #[test]
    fn a_forced_extension_is_taken_even_when_no_language_claims_it() {
        let languages = languages_claiming(&[("Python", &["py"])]);
        let forced = hashmap!("txt".to_owned() => "python".to_owned());
        let (map, report) = make_extension_language_map(&languages, &HashMap::new(), &forced);

        assert_eq!("Python", winner_of(&map, "txt"));
        assert_eq!("Python", winner_of(&map, "py"));
        // nothing was contested, so there is nothing to report
        assert!(report.collisions.is_empty());
    }

    // A caller of the library sets the field directly and is under no obligation to lowercase its
    // keys. When only the second of the two lookups normalised, the mapping was applied and the run
    // warned in the same breath that the extension had been left to the alphabetical tiebreak.
    #[test]
    fn a_forced_extension_is_normalised_before_it_is_looked_up() {
        let languages = languages_claiming(&[("MATLAB", &["m"]), ("Objective-C", &["m"])]);
        let forced = hashmap!("M".to_owned() => "MatLab".to_owned());
        let (map, report) = make_extension_language_map(&languages, &HashMap::new(), &forced);

        assert_eq!("MATLAB", winner_of(&map, "m"));
        assert_eq!(ResolvedBy::ForceLang, report.collisions[0].resolved_by);
        assert!(report.warnings().is_empty());
    }

    #[test]
    fn a_forced_language_that_is_not_available_is_reported_and_changes_nothing() {
        let languages = languages_claiming(&[("Python", &["py"])]);
        let forced = hashmap!("py".to_owned() => "cobol".to_owned());
        let (map, report) = make_extension_language_map(&languages, &HashMap::new(), &forced);

        assert_eq!("Python", winner_of(&map, "py"));
        assert_eq!(vec![("py".to_owned(), "cobol".to_owned())], report.unknown_forced_languages);
        let reported = report.warnings();
        assert_eq!(warnings::UNKNOWN_FORCED_LANGUAGE, reported[0].code);
        // a mapping that did not apply leaves the counts alone, it is the settings that were not honoured
        assert_eq!("settings", reported[0].affects.name());
        assert_eq!("py", reported[0].subject);
        assert!(reported[0].message.contains("not available"));
    }

    // Two spellings of one extension are one extension, and they have to collide as one. Left as
    // they were written they would look like two, would never be found to contest anything, and
    // would each quietly win in the files that happened to be spelled their way.
    #[test]
    fn extensions_are_matched_without_case_and_contest_each_other_across_it() {
        let languages = languages_claiming(&[("Zig", &["ZIG"]), ("Ziggy", &["zig"])]);
        let (map, report) = make_extension_language_map(&languages, &HashMap::new(), &HashMap::new());

        assert_eq!(1, report.collisions.len());
        assert_eq!("zig", report.collisions[0].extension);
        assert_eq!("Zig", winner_of(&map, "zig"));
        assert_eq!("Zig", winner_of(&map, "ZIG"));
        assert_eq!("Zig", winner_of(&map, "Zig"));
        assert_eq!("", winner_of(&map, "zigg"));
    }

    // 'name path' declares the module, a bare path declares none. The paths are the repository's
    // own, because a boundary is only a boundary if it is on disk: the table has to know whether a
    // nested target is a directory or a file to decide which of the two lookups will find it.
    fn modules_of(entries: &[&str]) -> Modules {
        let targets = entries.iter().map(|entry| match entry.split_once(' ') {
            Some((name, path)) => config_manager::Target::named(name, path.to_owned()),
            None => config_manager::Target::of((*entry).to_owned())
        }).collect::<Vec<_>>();

        Modules::of(&targets)
    }

    #[test]
    fn a_run_that_names_nothing_has_one_bucket_and_no_lookups() {
        let modules = modules_of(&["./src", "./tests"]);

        assert!(!modules.is_used());
        assert_eq!(1, modules.count());
        assert_eq!(None, modules.name_of(0));
        assert!(!modules.has_dir_boundaries() && !modules.has_file_boundaries());
    }

    // The order is the order they were declared in, except that the leftovers are last because they
    // are the leftovers. What the report shows is decided by '--sort' and not by this.
    #[test]
    fn the_leftovers_are_a_bucket_of_their_own_and_come_last() {
        let modules = modules_of(&["./src", "code ./src/utils.rs", "docs ./data"]);

        assert!(modules.is_used());
        assert_eq!(3, modules.count());
        assert_eq!(Some("code"), modules.name_of(0));
        assert_eq!(Some("docs"), modules.name_of(1));
        assert_eq!(None, modules.name_of(2));

        // One name and there is a second axis, with nothing left over to put in an unnamed row
        let modules = modules_of(&["code ./src"]);
        assert!(modules.is_used());
        assert_eq!(1, modules.count());
        assert_eq!(Some("code"), modules.name_of(0));
    }

    // The lookup that a walk pays for is the one that can find something. A nested file target must
    // not make every directory pay, and a nested directory must not make every file pay.
    #[test]
    fn only_a_target_inside_another_target_is_a_boundary() {
        let unrelated = modules_of(&["code ./src", "suite ./tests"]);
        assert!(!unrelated.has_dir_boundaries() && !unrelated.has_file_boundaries());

        let nested_dir = modules_of(&["./", "fixtures ./tests/fixtures"]);
        assert!(nested_dir.has_dir_boundaries() && !nested_dir.has_file_boundaries());

        let nested_file = modules_of(&["./src", "entry ./src/main.rs"]);
        assert!(!nested_file.has_dir_boundaries() && nested_file.has_file_boundaries());
    }

    // A path that does not match falls through to what the parent was, and the match is made on the
    // resolved path with the platform's own idea of case, or a boundary declared with a different
    // capitalisation would find nothing and its module would come out empty with nothing printed
    #[test]
    fn a_boundary_answers_for_its_own_path_and_leaves_the_rest_inherited() {
        let modules = modules_of(&["./", "fixtures ./tests/fixtures"]);
        let fixtures = modules.id_of(&Some("fixtures".to_owned()));

        assert_eq!(fixtures, modules.at_dir(Path::new("./tests/fixtures"), 7));
        assert_eq!(7, modules.at_dir(Path::new("./tests"), 7));
        assert_eq!(7, modules.at_dir(Path::new("./tests/fixtures/lang"), 7));
        // the same path as the platform hands it over during a walk
        assert_eq!(fixtures, modules.at_dir(Path::new(".\\tests\\fixtures"), 7));
        if cfg!(windows) {
            assert_eq!(fixtures, modules.at_dir(Path::new("./TESTS/Fixtures"), 7));
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
