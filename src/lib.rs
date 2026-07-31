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

mod result_printer;
mod json_printer;

pub use colored::{Color,Colorize,ColoredString};
pub use config_manager::{Configuration, SortCriterion};
pub use utils::*;
pub use domain::{Language, LanguageContentInfo, LanguageMetadata, FileStats, Keyword};

pub type FaultyFilesListMut = Arc<Mutex<Vec<FaultyFileDetails>>>;
pub type ExtensionLangMap = Arc<HashMap<String, Arc<str>>>;
pub type ContentInfoMapMut  = Arc<Mutex<HashMap<String,LanguageContentInfo>>>;
pub type MetadataMapMut     = Arc<Mutex<HashMap<String,LanguageMetadata>>>;

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

pub static PERSISTENT_APP_PATHS : LazyLock<PersistentAppPaths> = LazyLock::new(PersistentAppPaths::get);
pub static LOCAL_APP_PATHS : LazyLock<LocalAppPaths> = LazyLock::new(LocalAppPaths::get);
pub static CHANGELOG_BYTES : &[u8] = include_bytes!("../Changelog");


pub fn run(config: &Configuration, language_map: HashMap<String, Language>) -> Result<RunResult, ParseFilesError> {
    let config = Arc::new(config.clone());
    let faulty_files_ref : FaultyFilesListMut  = Arc::new(Mutex::new(Vec::with_capacity(10)));
    let finish_condition_ref = Arc::new(AtomicBool::new(false));
    let language_map_ref = Arc::new(language_map);
    let extension_lang_map: ExtensionLangMap = Arc::new(make_extension_language_map(&language_map_ref));
    let languages_content_info_ref : ContentInfoMapMut = Arc::new(Mutex::new(make_language_stats(language_map_ref.clone())));
    let global_languages_metadata_map = Arc::new(Mutex::new(make_language_metadata(&language_map_ref)));

    let mut files_present = FilesPresent::default();
    let idle_producers = Arc::new(AtomicUsize::new(0));
    let files_injector = Arc::new(Injector::<ParsableFile>::new());
    let dirs_injector = Arc::new(Injector::<TraversedDir>::new());
    let exclude_matcher = Arc::new(build_exclude_matcher(&config.exclude_dirs)
            .expect("exclude patterns are validated during argument parsing"));
    calculate_single_file_stats_or_add_to_injector(&config, &dirs_injector, &files_injector, &mut files_present,
            &extension_lang_map, &global_languages_metadata_map);

    let files_stats = Arc::new(Mutex::new(files_present));

    let mut producer_handles = Vec::with_capacity(config.threads.producers);
    let mut consumer_handles = Vec::with_capacity(config.threads.consumers);

    if !config.hidden.directory_info && config.prints_text() {
        println!("\n{}...",theme::active().heading.paint("Analyzing directories"));
    }

    let parsing_started_instant = Instant::now();
    for i in 0..config.threads.producers {
        producer_handles.push(producer::start_producer_thread(i, files_injector.clone(), dirs_injector.clone(), Worker::new_fifo(),
            global_languages_metadata_map.clone(), idle_producers.clone(), extension_lang_map.clone(), exclude_matcher.clone(),
            config.clone(), files_stats.clone()));
    }
    for i in 0..config.threads.consumers {
        consumer_handles.push(consumer::start_parser_thread(i, files_injector.clone(), faulty_files_ref.clone(), finish_condition_ref.clone(),
        languages_content_info_ref.clone(), language_map_ref.clone(), config.clone()));
    }

    for handle in producer_handles {
        let _ = handle.join();
    }
    let producers_done_millis = parsing_started_instant.elapsed().as_millis();

    //If there are a lot of files remaining after producers finish, it makes sense to start another consumer.
    let len = files_injector.len();
    if len > 1200 {
        consumer_handles.push(consumer::start_parser_thread(config.threads.consumers, files_injector, faulty_files_ref.clone(), finish_condition_ref.clone(),
        languages_content_info_ref.clone(), language_map_ref.clone(), config.clone()));
    }

    finish_condition_ref.store(true,Ordering::Relaxed);
    for handle in consumer_handles {
        let _ = handle.join();
    }
    let parsing_duration_millis = parsing_started_instant.elapsed().as_millis();

    if *phase_timing::ENABLED {
        eprintln!("[phase] producers alive: {} ms | drain after producers: {} ms | queue size at producer exit: {}",
            producers_done_millis, parsing_duration_millis - producers_done_millis, len);
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
    let languages_metadata_map = global_languages_metadata_map_guard.as_deref_mut().unwrap();

    remove_faulty_files_stats(&faulty_files_ref, languages_metadata_map, &extension_lang_map);

    let mut content_info_map_guard = languages_content_info_ref.lock();
    let content_info_map = content_info_map_guard.as_deref_mut().unwrap();

    let metrics = generate_metrics_if_parsing_took_more_than_one_sec(parsing_duration_millis, relevant_files_num, content_info_map);
    let final_stats = FinalStats::calculate(content_info_map, languages_metadata_map);
    remove_languages_with_0_files(content_info_map, languages_metadata_map);

    Ok(RunResult {
        content_info_map: std::mem::take(content_info_map),
        languages_metadata_map: std::mem::take(languages_metadata_map),
        final_stats,
        faulty_files: std::mem::take(&mut faulty_files_ref.lock().unwrap()),
        files_present,
        scan_duration_millis: parsing_duration_millis,
        metrics
    })
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
    result_printer::format_and_print_results(&result.content_info_map, &result.languages_metadata_map,
            &result.final_stats, &existing_log_contents, &datetime_now, config);

    if config.log.should_log && let Some(path) = log_file_path
        && io_handler::log_stats(&path, &existing_log_contents, &result.final_stats, &datetime_now, config).is_err() {
        eprintln!("\n{}",theme::active().warning.paint("Error while trying to save the log."));
    }
}

//pub for integration tests
pub fn calculate_single_file_stats_or_add_to_injector(config: &Configuration, dirs_injector: &Arc<Injector<TraversedDir>>, files_injector: &Arc<Injector<ParsableFile>>,
        files_present: &mut FilesPresent, extension_lang_map: &HashMap<String, Arc<str>>, languages_metadata_map: &MetadataMapMut)
{
    config.dirs.iter().for_each(|dir| {
        let dir_path = Path::new(dir);
        if dir_path.is_file() {
            if let Some(x) = dir_path.extension()
                && let Some(extension) = x.to_str()
                && let Some(lang_name) = find_language_of_extension(extension_lang_map, extension) {
                languages_metadata_map.lock().unwrap().get_mut(lang_name.as_ref()).unwrap().add_file_meta(
                        dir_path.metadata().map_or(0, |m| m.len() as usize));
                files_injector.push(ParsableFile::new(dir_path.to_path_buf(),lang_name));
                files_present.total_files += 1;
                files_present.relevant_files += 1;
            }
        } else if dir_path.is_dir() {
            let gitignore_stack = if config.no_gitignore { None } else { GitignoreStack::for_root_dir(dir_path) };
            dirs_injector.push(TraversedDir::new(dir_path.to_path_buf(), gitignore_stack));
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

pub fn make_extension_language_map(languages: &HashMap<String,Language>) -> HashMap<String, Arc<str>> {
    let mut names = languages.keys().collect::<Vec<_>>();
    names.sort_unstable();
    let mut map: HashMap<String, Arc<str>> = HashMap::new();
    for name in names {
        let shared_name: Arc<str> = Arc::from(name.as_str());
        for extension in &languages[name].extensions {
            map.entry(extension.clone()).or_insert_with(|| shared_name.clone());
        }
    }
    map
}

pub fn find_language_of_extension(extension_lang_map: &HashMap<String, Arc<str>>, extension: &str) -> Option<Arc<str>> {
    extension_lang_map.get(extension).cloned()
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

fn remove_faulty_files_stats(faulty_files_ref: &FaultyFilesListMut, languages_metadata_map: &mut HashMap<String,LanguageMetadata>,
        extension_lang_map: &HashMap<String, Arc<str>>) {
    let faulty_files = &*faulty_files_ref.as_ref().lock().unwrap();
    for file in faulty_files {
        let extension = utils::get_file_extension(Path::new(&file.path));
        if let Some(x) = extension {
            let lang_name = find_language_of_extension(extension_lang_map, x).unwrap();
            let language_metadata = languages_metadata_map.get_mut(lang_name.as_ref()).unwrap();
            language_metadata.files -= 1;
            language_metadata.bytes -= file.size as usize;
        }
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

pub fn make_language_stats(languages_map: Arc<HashMap<String,Language>>) -> HashMap<String,LanguageContentInfo> {
    let mut map = HashMap::<String,LanguageContentInfo>::new();
    for (key, value) in languages_map.iter() {
        map.insert(key.to_owned(), LanguageContentInfo::from(value));
    }
    map
}

pub fn make_language_metadata(language_map: &Arc<HashMap<String,Language>>) -> HashMap<String, LanguageMetadata> {
    let mut map = HashMap::<String,LanguageMetadata>::new();
    for name in language_map.keys() {
        map.insert(name.to_owned(), LanguageMetadata::default());
    }
    map
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
    pub content_info_map: HashMap<String, LanguageContentInfo>,
    pub languages_metadata_map: HashMap<String, LanguageMetadata>,
    pub final_stats: FinalStats,
    pub faulty_files: Vec<FaultyFileDetails>,
    pub files_present: FilesPresent,
    pub scan_duration_millis: u128,
    pub metrics: Option<Metrics>
}

impl RunResult {
    // Nothing of interest was found, which is an answer and not a failure: the counts are zero and
    // the file numbers still say how many were looked at and how many were excluded.
    fn of_nothing(files_present: FilesPresent, scan_duration_millis: u128) -> Self {
        RunResult {
            content_info_map: HashMap::new(),
            languages_metadata_map: HashMap::new(),
            final_stats: FinalStats::new_extended(0, 0, 0, 0, 0, 0, 0),
            faulty_files: Vec::new(),
            files_present,
            scan_duration_millis,
            metrics: None
        }
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
    pub language_name: Arc<str>
}

#[derive(Debug,Clone)]
pub struct TraversedDir {
    pub path: PathBuf,
    pub gitignore_stack: Option<Arc<GitignoreStack>>
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
        let data_dir = proj_dirs.data_dir().to_str().unwrap().to_owned() + "/";
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
    pub fn new(path: PathBuf, language_name: Arc<str>) -> Self {
        ParsableFile {
            path,
            language_name
        }
    }
}

impl TraversedDir {
    pub fn new(path: PathBuf, gitignore_stack: Option<Arc<GitignoreStack>>) -> Self {
        TraversedDir {
            path,
            gitignore_stack
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
