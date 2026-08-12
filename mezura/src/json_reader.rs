use std::collections::HashMap;

use mezura_core::{FaultyFileDetails, FilesPresent, ModuleResult, Performance, RunResult, Stats,
        Target, Threads, UnreadableDirDetails};
use serde_json::{Map, Value};

use super::json_printer::FORMAT_VERSION;

// One parsed document: the result of the run that wrote it, and the things a result does not carry,
// which are which mezura wrote it, when, the settings the counting obeyed, and what it warned about.
#[derive(Debug)]
pub struct Document {
    pub mezura_version: String,
    pub generated_at: String,
    pub scope: Scope,
    // How many languages '--top' left out of 'result.per_language'. The total is the whole run's
    // either way, so with anything but zero here the languages do not add up to it.
    pub languages_hidden: usize,
    pub warnings: Vec<DocumentWarning>,
    // The counts from the scan block, kept apart from the lists in 'result': the lists are written
    // only when '--show-faulty-files' asked for them, so their length reads zero for a run that
    // had failures and did not detail them, and these two do not.
    pub faulty_files_count: usize,
    pub unreadable_dirs_count: usize,
    pub result: RunResult
}

// The settings that can change a number, as the run that wrote the document had them: two documents
// that disagree here were not measuring the same thing. The directories are not among them, they
// are the result's own targets.
#[derive(Debug,Clone)]
pub struct Scope {
    pub exclude: Vec<String>,
    pub languages: Vec<String>,
    pub excluded_languages: Vec<String>,
    // The extension is the key, as the run is asked about it: 'm' to 'matlab'
    pub forced_languages: HashMap<String, String>,
    pub braces_as_code: bool,
    pub search_in_dotted: bool,
    pub gitignore: bool,
    // Whether the keywords were counted at all: '--hide keywords' stops the counting, and a map
    // that is empty because nothing measured it must not read as a count of zero
    pub keywords_counted: bool
}

// Kept as text rather than as the library's own warning, whose code is a '&'static str': a document
// written by a later version can carry a code this build has never heard of.
//
// The subject is not among them, and is the one member of a warning that is read and thrown away:
// every message mezura writes names its own subject inside the sentence, so carrying it separately
// would only put the same word on the screen twice.
#[derive(Debug)]
pub struct DocumentWarning {
    pub code: String,
    pub affects: String,
    pub message: String
}

#[derive(Debug)]
pub enum DocumentError {
    NotJson(serde_json::Error),
    // Not "unsupported": the format is only bumped when a key is removed or changes meaning, so a
    // higher one may be missing something read here or may spell it differently.
    FormatTooNew { found: usize },
    // A valid document of another kind, which without this check would be reported as a broken one
    // of this kind: a comparison has no 'scan', and "missing 'scan'" sends the reader hunting for
    // damage in a file with nothing wrong with it.
    NotARun { kind: String },
    // Both carry the path of the offending member, as 'total.lines' or 'languages[2].name'
    Missing(String),
    WrongType { at: String, wanted: &'static str }
}

impl std::fmt::Display for DocumentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotJson(x) => write!(f, "The file is not valid JSON: {x}."),
            Self::FormatTooNew { found } => write!(f, "The document is written in format {found} and this mezura reads up to format {FORMAT_VERSION}, so a newer version wrote it. Update mezura to read it."),
            Self::NotARun { kind } => write!(f, "The document holds a {kind}, not the counts of a run, so there is nothing in it to compare against."),
            Self::Missing(at) => write!(f, "The document has no '{at}', so it cannot be read."),
            Self::WrongType { at, wanted } => write!(f, "'{at}' is not {wanted}.")
        }
    }
}

impl std::error::Error for DocumentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotJson(x) => Some(x),
            _ => None
        }
    }
}

// Reads what '--output json' wrote. A key this build does not know is ignored, because adding one is
// not a change of format.
//
// What comes back is what the document carries and nothing more: a document written with
// '--hide timing' has no timing in it, so 'result.performance' comes back holding a zero duration
// and one thread of each kind, which is not something that was measured.
pub fn parse(contents: &str) -> Result<Document, DocumentError> {
    let value = serde_json::from_str::<Value>(crate::config_files::strip_byte_order_mark(contents))
            .map_err(DocumentError::NotJson)?;
    let Value::Object(root) = &value else {
        return Err(DocumentError::WrongType { at: "the document".to_owned(), wanted: "an object" });
    };

    // Before anything is read out of it, so that a document this build cannot understand is refused
    // rather than half read
    let format = read_number(root, "format", "")?;
    if format > FORMAT_VERSION {
        return Err(DocumentError::FormatTooNew { found: format });
    }
    // Absent from a document of the first builds, which held nothing but runs, so only a kind that
    // is present and says something else refuses
    if let Some(kind) = root.get("kind").and_then(Value::as_str) && kind != "run" {
        return Err(DocumentError::NotARun { kind: kind.to_owned() });
    }

    let scope = read_nested(root, "scope", "")?;
    let scan = read_nested(root, "scan", "")?;
    let total = parse_stats(read_nested(root, "total", "")?, "total")?;
    let per_language = parse_languages(read_list(root, "languages", "")?, "languages")?;
    let nested_languages = parse_nested_languages(read_list(root, "languages", "")?, "languages")?;

    let performance = match root.get("performance") {
        Some(x) => parse_performance(read_object(x, "performance")?)?,
        None => Performance { duration_millis: 0, threads: Threads::new(0, 0) }
    };
    // A document that named no module leaves the block out, and a run that named none has exactly
    // one module holding everything it counted, which is what the two blocks above already are.
    let modules = match root.get("modules") {
        Some(x) => parse_modules(read_array(x, "modules")?)?,
        None => vec![ModuleResult { name: None, per_language: per_language.clone(), total: total.clone(),
                nested_languages: nested_languages.clone() }]
    };

    let (scope, targets) = parse_scope(scope)?;
    Ok(Document {
        mezura_version: read_text(root, "mezura_version", "")?,
        generated_at: read_text(root, "generated_at", "")?,
        languages_hidden: read_number(root, "languages_hidden", "")?,
        warnings: parse_warnings(read_list(root, "warnings", "")?)?,
        faulty_files_count: read_number(scan, "files_faulty", "scan")?,
        unreadable_dirs_count: read_number(scan, "dirs_unreadable", "scan")?,
        scope,
        result: RunResult {
            per_language,
            total,
            modules,
            nested_languages,
            // The paths are written only when '--show-faulty-files' asked for them, so an absent
            // list means they were not detailed, never that nothing went wrong: how many there
            // were is in 'scan' and is read either way.
            faulty_files: match root.get("faulty_files") {
                Some(x) => parse_faulty_files(read_array(x, "faulty_files")?)?,
                None => Vec::new()
            },
            files_present: FilesPresent {
                total_files: read_number(scan, "files_found", "scan")?,
                relevant_files: read_number(scan, "files_of_interest", "scan")?,
                excluded_files: read_number(scan, "files_excluded", "scan")?
            },
            performance,
            targets,
            unreadable_dirs: match root.get("unreadable_dirs") {
                Some(x) => parse_unreadable_dirs(read_array(x, "unreadable_dirs")?)?,
                None => Vec::new()
            }
        }
    })
}

// The one block that travels beyond the run document: a log entry records the same scope, so both
// kinds read it through here and a key cannot be added to one of them alone
pub(crate) fn parse_scope(scope: &Map<String, Value>) -> Result<(Scope, Vec<Target>), DocumentError> {
    Ok((Scope {
        exclude: read_strings(scope, "exclude", "scope")?,
        languages: read_strings(scope, "languages", "scope")?,
        excluded_languages: read_strings(scope, "excluded_languages", "scope")?,
        forced_languages: parse_forced_languages(read_nested(scope, "forced_languages", "scope")?)?,
        braces_as_code: read_flag(scope, "braces_as_code", "scope")?,
        search_in_dotted: read_flag(scope, "search_in_dotted", "scope")?,
        gitignore: read_flag(scope, "gitignore", "scope")?,
        // Absent from a document of the first builds, which all counted them
        keywords_counted: match scope.get("keywords_counted") {
            Some(x) => x.as_bool().ok_or(DocumentError::WrongType {
                    at: "scope.keywords_counted".to_owned(), wanted: "true or false" })?,
            None => true
        }
    }, parse_targets(read_list(scope, "dirs", "scope")?)?))
}

// 'extra' and 'average_bytes' are not read: both are worked out from the counts beside them, and a
// stored copy is the one thing that can disagree with them.
pub(crate) fn parse_stats(entry: &Map<String, Value>, at: &str) -> Result<Stats, DocumentError> {
    let keywords = match entry.get("keywords") {
        Some(x) => parse_keywords(read_object(x, &join_location(at, "keywords"))?, &join_location(at, "keywords"))?,
        None => HashMap::new()
    };

    Ok(Stats::new(
        read_number(entry, "files", at)?,
        read_number(entry, "bytes", at)?,
        read_number(entry, "lines", at)?,
        read_number(entry, "code", at)?,
        read_number(entry, "comments", at)?,
        keywords))
}

fn parse_forced_languages(entry: &Map<String, Value>) -> Result<HashMap<String, String>, DocumentError> {
    entry.keys().map(|extension| Ok((extension.clone(), read_text(entry, extension, "scope.forced_languages")?)))
            .collect()
}

fn parse_keywords(entry: &Map<String, Value>, at: &str) -> Result<HashMap<String, usize>, DocumentError> {
    entry.keys().map(|name| Ok((name.clone(), read_number(entry, name, at)?))).collect()
}

fn parse_languages(entries: &[Value], at: &str) -> Result<HashMap<String, Stats>, DocumentError> {
    entries.iter().enumerate().map(|(i, entry)| {
        let at = format!("{at}[{i}]");
        let entry = read_object(entry, &at)?;
        Ok((read_text(entry, "name", &at)?, parse_stats(entry, &at)?))
    }).collect()
}

// A document written before sections existed simply has none, so an older baseline compares as a
// run whose containers held nothing rather than failing to read
fn parse_nested_languages(entries: &[Value], at: &str)
        -> Result<HashMap<String, HashMap<String, Stats>>, DocumentError>
{
    let mut found = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        let at = format!("{at}[{i}]");
        let entry = read_object(entry, &at)?;
        let Some(sections) = entry.get("nested_languages") else { continue };

        let at = join_location(&at, "nested_languages");
        let sections = sections.as_array().ok_or_else(|| DocumentError::WrongType {
                at: at.clone(), wanted: "a list" })?;
        found.insert(read_text(entry, "name", &at)?, parse_languages(sections, &at)?);
    }

    Ok(found)
}

fn parse_modules(entries: &[Value]) -> Result<Vec<ModuleResult>, DocumentError> {
    entries.iter().enumerate().map(|(i, entry)| {
        let at = format!("modules[{i}]");
        let entry = read_object(entry, &at)?;

        Ok(ModuleResult {
            name: read_optional_name(entry, "name", &at)?,
            total: parse_stats(read_nested(entry, "total", &at)?, &join_location(&at, "total"))?,
            per_language: parse_languages(read_list(entry, "languages", &at)?, &join_location(&at, "languages"))?,
            nested_languages: parse_nested_languages(read_list(entry, "languages", &at)?,
                    &join_location(&at, "languages"))?
        })
    }).collect()
}

fn parse_performance(entry: &Map<String, Value>) -> Result<Performance, DocumentError> {
    let threads = read_nested(entry, "threads", "performance")?;

    Ok(Performance {
        duration_millis: read_number(entry, "scan_ms", "performance")? as u128,
        threads: Threads::new(read_number(threads, "producers", "performance.threads")?,
                read_number(threads, "consumers", "performance.threads")?)
    })
}

fn parse_warnings(entries: &[Value]) -> Result<Vec<DocumentWarning>, DocumentError> {
    entries.iter().enumerate().map(|(i, entry)| {
        let at = format!("warnings[{i}]");
        let entry = read_object(entry, &at)?;

        Ok(DocumentWarning {
            code: read_text(entry, "code", &at)?,
            affects: read_text(entry, "affects", &at)?,
            message: read_text(entry, "message", &at)?
        })
    }).collect()
}

fn parse_faulty_files(entries: &[Value]) -> Result<Vec<FaultyFileDetails>, DocumentError> {
    entries.iter().enumerate().map(|(i, entry)| {
        let at = format!("faulty_files[{i}]");
        let entry = read_object(entry, &at)?;

        Ok(FaultyFileDetails::new(read_text(entry, "path", &at)?, read_text(entry, "error", &at)?,
                read_number(entry, "bytes", &at)? as u64))
    }).collect()
}

fn parse_unreadable_dirs(entries: &[Value]) -> Result<Vec<UnreadableDirDetails>, DocumentError> {
    entries.iter().enumerate().map(|(i, entry)| {
        let at = format!("unreadable_dirs[{i}]");
        let entry = read_object(entry, &at)?;

        Ok(UnreadableDirDetails::new(read_text(entry, "path", &at)?, read_text(entry, "error", &at)?))
    }).collect()
}

fn parse_targets(entries: &[Value]) -> Result<Vec<Target>, DocumentError> {
    entries.iter().enumerate().map(|(i, entry)| {
        let at = format!("scope.dirs[{i}]");
        let entry = read_object(entry, &at)?;
        let path = read_text(entry, "path", &at)?;

        Ok(match read_optional_name(entry, "module", &at)? {
            Some(module) => Target::named(module, path),
            None => Target::of(path)
        })
    }).collect()
}

fn read_member<'a>(parent: &'a Map<String, Value>, key: &str, at: &str) -> Result<&'a Value, DocumentError> {
    parent.get(key).ok_or_else(|| DocumentError::Missing(join_location(at, key)))
}

pub(crate) fn read_number(parent: &Map<String, Value>, key: &str, at: &str) -> Result<usize, DocumentError> {
    read_member(parent, key, at)?.as_u64().and_then(|x| usize::try_from(x).ok())
            .ok_or_else(|| DocumentError::WrongType { at: join_location(at, key), wanted: "a whole number" })
}

pub(crate) fn read_text(parent: &Map<String, Value>, key: &str, at: &str) -> Result<String, DocumentError> {
    read_member(parent, key, at)?.as_str().map(str::to_owned)
            .ok_or_else(|| DocumentError::WrongType { at: join_location(at, key), wanted: "a string" })
}

// A module that was never given a name, which is what the leftovers of the named ones are called and
// what an ordinary target has. 'null' is the only place a member of a document may be empty.
pub(crate) fn read_optional_name(parent: &Map<String, Value>, key: &str, at: &str) -> Result<Option<String>, DocumentError> {
    match read_member(parent, key, at)? {
        Value::Null => Ok(None),
        Value::String(x) => Ok(Some(x.clone())),
        _ => Err(DocumentError::WrongType { at: join_location(at, key), wanted: "a string or null" })
    }
}

fn read_flag(parent: &Map<String, Value>, key: &str, at: &str) -> Result<bool, DocumentError> {
    read_member(parent, key, at)?.as_bool()
            .ok_or_else(|| DocumentError::WrongType { at: join_location(at, key), wanted: "true or false" })
}

fn read_strings(parent: &Map<String, Value>, key: &str, at: &str) -> Result<Vec<String>, DocumentError> {
    read_list(parent, key, at)?.iter().enumerate().map(|(i, value)| {
        value.as_str().map(str::to_owned).ok_or_else(|| DocumentError::WrongType {
                at: format!("{}[{i}]", join_location(at, key)), wanted: "a string" })
    }).collect()
}

pub(crate) fn read_nested<'a>(parent: &'a Map<String, Value>, key: &str, at: &str) -> Result<&'a Map<String, Value>, DocumentError> {
    read_object(read_member(parent, key, at)?, &join_location(at, key))
}

pub(crate) fn read_list<'a>(parent: &'a Map<String, Value>, key: &str, at: &str) -> Result<&'a Vec<Value>, DocumentError> {
    read_array(read_member(parent, key, at)?, &join_location(at, key))
}

pub(crate) fn read_object<'a>(value: &'a Value, at: &str) -> Result<&'a Map<String, Value>, DocumentError> {
    value.as_object().ok_or_else(|| DocumentError::WrongType { at: at.to_owned(), wanted: "an object" })
}

fn read_array<'a>(value: &'a Value, at: &str) -> Result<&'a Vec<Value>, DocumentError> {
    value.as_array().ok_or_else(|| DocumentError::WrongType { at: at.to_owned(), wanted: "an array" })
}

fn join_location(at: &str, key: &str) -> String {
    if at.is_empty() {
        key.to_owned()
    } else {
        format!("{at}.{key}")
    }
}

#[cfg(test)]
mod tests {
    use chrono::Local;
    use mezura_core::SortCriterion;

    use crate::config_manager::Configuration;
    use crate::json_printer::create_document;

    use super::*;

    fn parse_stats(files: usize, bytes: usize, lines: usize, code: usize, comments: usize,
            keywords: HashMap<String, usize>) -> Stats {
        Stats::new(files, bytes, lines, code, comments, keywords)
    }

    // Everything the printer can put in a document, so that nothing is left to a shape no test
    // reaches: two languages, one of them with keywords and one without, a named module beside the
    // leftovers, and paths carrying the backslashes and quotation marks that have to survive the
    // escaping on the way out and the way back in.
    fn populated() -> (RunResult, Configuration) {
        let rust = parse_stats(2, 5000, 100, 70, 10, hashmap!["structs".to_owned() => 3, "enums".to_owned() => 0]);
        let html = parse_stats(1, 900, 40, 30, 0, HashMap::new());
        let per_language = hashmap!["Rust".to_owned() => rust.clone(), "HTML".to_owned() => html.clone()];

        let result = RunResult {
            total: Stats::total_of(&per_language),
            modules: vec![
                ModuleResult { name: Some("backend".to_owned()), per_language: hashmap!["Rust".to_owned() => rust], total: Stats::total_of(&hashmap!["Rust".to_owned() => parse_stats(2, 5000, 100, 70, 10, hashmap!["structs".to_owned() => 3, "enums".to_owned() => 0])]), nested_languages: Default::default() },
                ModuleResult { name: None, per_language: hashmap!["HTML".to_owned() => html.clone()], total: Stats::total_of(&hashmap!["HTML".to_owned() => html]), nested_languages: Default::default() }],
            per_language,
            nested_languages: Default::default(),
            faulty_files: vec![FaultyFileDetails::new("D:\\dev\\a \"b\".rs".to_owned(), "stream did not contain valid UTF-8".to_owned(), 412)],
            files_present: FilesPresent { total_files: 9, relevant_files: 3, excluded_files: 4 },
            performance: Performance { duration_millis: 1180, threads: Threads::new(2, 8) },
            targets: vec![Target::named("backend", "D:/dev/api"), Target::named("backend", "D:/dev/api-v2"),
                    Target::of("D:/dev/web")],
            unreadable_dirs: vec![UnreadableDirDetails::new("D:/dev/locked".to_owned(), "Access is denied. (os error 5)".to_owned())]
        };

        let mut config = Configuration::new(vec!["./src".to_owned()]);
        config.engine.exclude_dirs = vec!["target".to_owned(), "*.min.js".to_owned()];
        config.engine.languages_of_interest = vec!["rust".to_owned()];
        config.engine.excluded_languages = vec!["json".to_owned()];
        config.engine.braces_as_code = true;
        config.engine.should_search_in_dotted = true;
        config.engine.no_gitignore = true;
        config.view.sort_by = SortCriterion::Name;
        // The two lists of paths are written only when they are asked for, and this asks
        config.view.set_should_show_faulty_files(true);

        (result, config)
    }

    fn assert_same_stats(expected: &HashMap<String, Stats>, read: &HashMap<String, Stats>) {
        assert_eq!(expected.len(), read.len(), "expected {expected:?}, read {read:?}");
        for (name, stats) in expected {
            assert_eq!(Some(stats), read.get(name), "'{name}' did not survive the round trip");
        }
    }

    #[test]
    fn a_document_reads_back_into_the_result_that_wrote_it() {
        let (written, config) = populated();
        let read = parse(&create_document(&written, &Local::now(), &config)).unwrap().result;

        assert_same_stats(&written.per_language, &read.per_language);
        assert_eq!(written.total, read.total);
        assert_eq!(written.files_present, read.files_present);
        assert_eq!(written.targets, read.targets);
        assert_eq!(written.performance.duration_millis, read.performance.duration_millis);
        // one entry per target, so the two paths of 'backend' come back as two, in the order they
        // were declared and each still knowing which module claimed it
        assert_eq!(3, read.targets.len());
        assert_eq!(written.performance.threads, read.performance.threads);

        assert_eq!(written.modules.len(), read.modules.len());
        for (written, read) in written.modules.iter().zip(&read.modules) {
            assert_eq!(written.name, read.name);
            assert_eq!(written.total, read.total);
            assert_same_stats(&written.per_language, &read.per_language);
        }

        // The two lists carry the strings that the escaping on the way out has to undo exactly: a
        // Windows path is backslashes, and one of them holds a quotation mark as well
        assert_eq!(1, read.faulty_files.len());
        assert_eq!("D:\\dev\\a \"b\".rs", read.faulty_files[0].path);
        assert_eq!("stream did not contain valid UTF-8", read.faulty_files[0].error_msg);
        assert_eq!(412, read.faulty_files[0].size);
        assert_eq!(1, read.unreadable_dirs.len());
        assert_eq!("D:/dev/locked", read.unreadable_dirs[0].path);
        assert_eq!("Access is denied. (os error 5)", read.unreadable_dirs[0].error_msg);
    }

    // The settings are the half of a document that says whether two of them are comparable at all,
    // and none of them can be worked out from the counts.
    #[test]
    fn the_settings_the_counting_obeyed_come_back_with_it() {
        let (result, config) = populated();
        let read = parse(&create_document(&result, &Local::now(), &config)).unwrap();

        assert_eq!(crate::config_manager::VERSION_ID.trim_start_matches('v'), read.mezura_version);
        assert_eq!(vec!["target".to_owned(), "*.min.js".to_owned()], read.scope.exclude);
        assert_eq!(vec!["rust".to_owned()], read.scope.languages);
        assert_eq!(vec!["json".to_owned()], read.scope.excluded_languages);
        assert!(read.scope.braces_as_code);
        assert!(read.scope.search_in_dotted);
        // written as whether the file is obeyed, which is the opposite of the flag that turns it off
        assert!(!read.scope.gitignore);
        assert!(read.scope.keywords_counted);
        assert_eq!(0, read.languages_hidden);
        assert_eq!(1180, read.result.performance.duration_millis);

        let timestamp = chrono::DateTime::parse_from_rfc3339("2026-07-30T14:22:07+03:00").unwrap().with_timezone(&Local);
        assert_eq!(timestamp.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
                parse(&create_document(&result, &timestamp, &config)).unwrap().generated_at);
    }

    // Three blocks are written only when there is something to say, so their absence is what carries
    // the meaning and none of them may read as a measurement of zero.
    #[test]
    fn what_a_document_leaves_out_reads_back_as_what_its_absence_means() {
        let (result, mut config) = populated();

        // Nothing measured it, so what comes back is a zero and not a measurement of zero, which is
        // the one thing about a document that reading it cannot make honest
        config.view.hidden.timing = true;
        let read = parse(&create_document(&result, &Local::now(), &config)).unwrap();
        assert_eq!(0, read.result.performance.duration_millis);

        // '--hide keywords' stops the counting as well as the printing, so the map comes back empty
        // rather than as a set of zeros, and the scope says why it is empty
        config.view.hidden.timing = false;
        config.view.hidden.keywords = true;
        let read = parse(&create_document(&result, &Local::now(), &config)).unwrap();
        assert!(read.result.per_language["Rust"].keyword_occurences.is_empty());
        assert!(read.result.total.keyword_occurences.is_empty());
        assert!(!read.scope.keywords_counted);

        // The scan counts come back even when the lists were not asked for, because the lists are
        // '--show-faulty-files' detail while the counts say whether the numbers are short
        let mut short = populated().0;
        short.faulty_files = vec![mezura_core::FaultyFileDetails::new("a.rs".to_owned(), "no".to_owned(), 1)];
        short.unreadable_dirs = vec![mezura_core::UnreadableDirDetails::new("D:/locked".to_owned(), "no".to_owned())];
        config.view.set_should_show_faulty_files(false);
        let undetailed = parse(&create_document(&short, &Local::now(), &config)).unwrap();
        assert!(undetailed.result.faulty_files.is_empty() && undetailed.result.unreadable_dirs.is_empty());
        assert_eq!((1, 1), (undetailed.faulty_files_count, undetailed.unreadable_dirs_count));

        // A document from a build that had not met the key counted them, all of those builds did,
        // so its absence must not read as a refusal or as keywords that were hidden
        config.view.hidden.keywords = false;
        let older = create_document(&result, &Local::now(), &config).replace(",\n    \"keywords_counted\": true", "");
        assert!(!older.contains("keywords_counted"));
        assert!(parse(&older).unwrap().scope.keywords_counted);

        // and a run that named no module still has the one holding everything, so what a document
        // without the block reads back as has to be that and not an absence of modules
        let (mut plain, config) = populated();
        plain.modules = vec![ModuleResult { name: None, per_language: plain.per_language.clone(), total: plain.total.clone(),
                nested_languages: Default::default() }];
        let written = create_document(&plain, &Local::now(), &config);
        assert!(!written.contains("\"modules\""));

        let read = parse(&written).unwrap().result;
        assert_eq!(1, read.modules.len());
        assert_eq!(None, read.modules[0].name);
        assert_eq!(plain.total, read.modules[0].total);
        assert_same_stats(&plain.per_language, &read.modules[0].per_language);
    }

    // '--top' cuts the languages and leaves the total whole, so a reading taken off such a document
    // is not the run, and the one thing that says so is a number nothing else can be derived from.
    #[test]
    fn a_document_that_was_cut_by_top_says_how_many_languages_it_is_short() {
        let (result, mut config) = populated();
        config.view.top_n = Some(1);
        let read = parse(&create_document(&result, &Local::now(), &config)).unwrap();

        assert_eq!(1, read.languages_hidden);
        assert_eq!(1, read.result.per_language.len());
        assert_eq!(result.total, read.result.total);
    }

    // Everything the run said on the error output, which is the half of a document that says whether
    // the counts can be trusted at all. Written out by hand and not through the printer, because the
    // collector those come from belongs to the process and every other test of this binary adds to it.
    #[test]
    fn the_warnings_come_back_as_text_and_a_key_this_build_never_heard_of_is_ignored() {
        let read = parse(&minimal_document(
            "\"warnings\": [{\"code\": \"extension-tiebreak\", \"affects\": \"counts\", \
             \"subject\": \"m\", \"message\": \"'m' is claimed by two languages.\"}], \
             \"a_key_from_a_later_version\": {\"nested\": [1, 2]}")).unwrap();

        assert_eq!(1, read.warnings.len());
        assert_eq!("extension-tiebreak", read.warnings[0].code);
        assert_eq!("counts", read.warnings[0].affects);
        assert_eq!("'m' is claimed by two languages.", read.warnings[0].message);
    }

    // Every one of these is a file somebody was handed and passed on, so each has to say which part
    // of it is wrong rather than that it is not a document.
    #[test]
    fn a_file_that_is_not_a_document_says_which_part_of_it_is_wrong() {
        let error = |body: &str| parse(&minimal_document(body)).unwrap_err().to_string();

        assert!(matches!(parse("{ not json"), Err(DocumentError::NotJson(_))));
        assert!(matches!(parse("[1, 2, 3]"),
                Err(DocumentError::WrongType { ref at, .. }) if at == "the document"));

        // a key that is not there, at every depth, named by the path a person can look up
        assert!(parse("{}").unwrap_err().to_string().contains("'format'"));
        assert!(error("\"total\": {\"files\": 0, \"bytes\": 0}").contains("'total.lines'"));
        assert!(error("\"scope\": {}").contains("'scope.exclude'"));
        assert!(error("\"languages\": [{\"name\": \"Rust\"}]").contains("'languages[0].files'"));
        assert!(error("\"modules\": [{\"name\": null, \"total\": {}}]").contains("'modules[0].total.files'"));

        // and one that is there holding something else
        assert!(error("\"warnings\": [{\"code\": 7}]").contains("'warnings[0].code' is not a string"));
        assert!(error("\"warnings\": \"none\"").contains("'warnings' is not an array"));
        assert!(error("\"total\": 5").contains("'total' is not an object"));
        assert!(error("\"modules\": [{\"name\": 7}]").contains("'modules[0].name' is not a string or null"));

        // a target is an object of two members, and either of them being wrong names that target
        let scope_with = |dirs: &str| format!("\"scope\": {{\"dirs\": {dirs}, \"exclude\": [], \
                \"languages\": [], \"excluded_languages\": [], \"forced_languages\": {{}}, \
                \"braces_as_code\": false, \"search_in_dotted\": false, \"gitignore\": true, \
                \"keywords_counted\": true}}");
        assert!(error(&scope_with("[{\"module\": null}]")).contains("'scope.dirs[0].path'"));
        assert!(error(&scope_with("[{\"module\": 7, \"path\": \"x\"}]")).contains("'scope.dirs[0].module' is not a string or null"));

        // a later format may have removed a key or changed what one means, so it is refused whole
        // rather than read as far as it happens to match
        let newer = minimal_document("\"warnings\": []").replace("\"format\": 1", "\"format\": 2");
        let refused = parse(&newer).unwrap_err().to_string();
        assert!(refused.contains("format 2") && refused.contains("Update mezura"), "{refused}");
    }

    // Written by hand rather than by the printer, so that a test can leave a member out or spell it
    // wrong. 'body' is added to it, and replaces any member of the same name.
    fn minimal_document(body: &str) -> String {
        let document = format!("{{\"format\": 1, \"mezura_version\": \"3.0.0\", \
            \"generated_at\": \"2026-07-30T14:22:07+03:00\", \
            \"scope\": {{\"dirs\": [], \"exclude\": [], \"languages\": [], \"excluded_languages\": [], \
                \"forced_languages\": {{}}, \"braces_as_code\": false, \"search_in_dotted\": false, \
                \"gitignore\": true, \"keywords_counted\": true}}, \
            \"scan\": {{\"files_found\": 0, \"files_of_interest\": 0, \"files_excluded\": 0, \
                \"files_faulty\": 0, \"dirs_unreadable\": 0}}, \
            \"total\": {{\"files\": 0, \"lines\": 0, \"code\": 0, \"comments\": 0, \"bytes\": 0}}, \
            \"languages\": [], \"languages_hidden\": 0, \"faulty_files\": [], \"unreadable_dirs\": [], \
            \"warnings\": [], {body}}}");
        // the same key twice is valid JSON and the last one wins, which is what lets 'body' replace
        // one of the members above instead of only adding to them
        assert!(serde_json::from_str::<Value>(&document).is_ok(), "the test's own document is not JSON:\n{document}");

        document
    }
}
