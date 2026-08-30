use std::collections::HashMap;

use mezura_core::{FaultyFileDetails, FileEntry, FilesPresent, LineClasses, ModuleResult, Performance,
        RunResult, Stats, Target, Threads, UnreadableDirDetails};
use serde_json::{Map, Value};

use super::json_printer::FORMAT_VERSION;

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
    // only when '--show-faulty-files' asked for them, so their length reads zero for a run that had
    // failures and did not detail them, while these two hold the real number.
    pub faulty_files_count: usize,
    pub unreadable_dirs_count: usize,
    // Whether the run wrote file rows at all, so a document taken without '--by-file' is told apart
    // from one whose files simply did not change. A run that counted nothing had nothing to record,
    // so it counts as recorded.
    pub files_recorded: bool,
    // How many file rows a capped '--by-file' left out of the document, summed over its modules
    pub files_hidden: usize,
    pub result: RunResult
}

// The settings that can change a number, as the run that wrote the document had them: two documents
// that disagree here were not measuring the same thing.
#[derive(Debug,Clone)]
pub struct Scope {
    pub exclude: Vec<String>,
    pub languages: Vec<String>,
    pub excluded_languages: Vec<String>,
    // The extension is the key, as the run is asked about it: 'm' to 'matlab'
    pub forced_languages: HashMap<String, String>,
    // Kept as the word the document holds rather than a 'CountingModel': a document written by a
    // later version can name a model this build has never heard of, and the comparison still has
    // to say that the two readings were not taken the same way
    pub counting: String,
    pub search_in_dotted: bool,
    pub gitignore: bool,
    // The '.ignore' and '.rgignore' files, asked separately from the '.gitignore' above
    pub ignore_files: bool,
    // Whether the keywords were counted at all: '--hide keywords' stops the counting, and a map
    // that is empty because nothing measured it must not read as a count of zero
    pub keywords_counted: bool,
    pub count_minified: bool,
    pub count_generated: bool
}

// Kept as text rather than as the library's own warning: a document written by a later version can
// carry a code this build has never heard of. The subject is read and thrown away, since every
// message mezura writes names its own subject inside the sentence.
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
    // A valid document of another kind. Without this check a comparison, which has no 'scan' block,
    // would be reported as "missing 'scan'" and send the reader hunting for damage in a file with
    // nothing wrong with it.
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

// Reads what '--output json' wrote. A key this build does not know is ignored, because adding one
// is not a change of format. What comes back is what the document carries and nothing more: written
// with '--hide timing' it has no timing, so 'result.performance' comes back holding zeros that were
// never measured.
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
    // The file rows a cap left out are counted where the rows themselves live, which is why the
    // top level's own figure is read only when there are no modules to read it from.
    let (modules, files_hidden) = match root.get("modules") {
        Some(x) => parse_modules(read_array(x, "modules")?)?,
        None => (vec![ModuleResult { name: None, per_language: per_language.clone(), total: total.clone(),
                nested_languages: nested_languages.clone(),
                files: parse_files(read_list(root, "languages", "")?, "languages")? }],
                read_optional_number(root, "files_hidden", "")?)
    };

    let (scope, targets) = parse_scope(scope)?;
    Ok(Document {
        mezura_version: read_text(root, "mezura_version", "")?,
        generated_at: read_text(root, "generated_at", "")?,
        languages_hidden: read_number(root, "languages_hidden", "")?,
        warnings: parse_warnings(read_list(root, "warnings", "")?)?,
        faulty_files_count: read_number(scan, "files_faulty", "scan")?,
        unreadable_dirs_count: read_number(scan, "dirs_unreadable", "scan")?,
        // Decided off what was actually parsed, so it can never claim rows the modules do not hold
        files_recorded: modules.iter().any(|x| !x.files.is_empty()) || total.files == 0,
        files_hidden,
        scope,
        result: RunResult {
            per_language,
            total,
            modules,
            nested_languages,
            // An absent list means the paths were not detailed, never that nothing went wrong: how
            // many there were is in 'scan' and is read either way.
            faulty_files: match root.get("faulty_files") {
                Some(x) => parse_faulty_files(read_array(x, "faulty_files")?)?,
                None => Vec::new()
            },
            // Absent from a document of the first builds, which counted every file they read
            minified_files: read_optional_number(scan, "files_minified", "scan")?,
            generated_files: read_optional_number(scan, "files_generated", "scan")?,
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

// A log entry records the same scope, so both kinds of file read it through here and a key cannot
// be added to one of them alone
pub(crate) fn parse_scope(scope: &Map<String, Value>) -> Result<(Scope, Vec<Target>), DocumentError> {
    Ok((Scope {
        exclude: read_strings(scope, "exclude", "scope")?,
        languages: read_strings(scope, "languages", "scope")?,
        excluded_languages: read_strings(scope, "excluded_languages", "scope")?,
        forced_languages: parse_forced_languages(read_nested(scope, "forced_languages", "scope")?)?,
        counting: read_text(scope, "counting", "scope")?,
        search_in_dotted: read_flag(scope, "search_in_dotted", "scope")?,
        gitignore: read_flag(scope, "gitignore", "scope")?,
        // Absent from a document of the builds that read no such file, which is what its absence
        // therefore means
        ignore_files: read_optional_flag(scope, "ignore_files", "scope", false)?,
        // Absent from a document of the first builds, which all counted them
        keywords_counted: read_optional_flag(scope, "keywords_counted", "scope", true)?,
        // Absent for the same reason, and those builds counted every file they could read
        count_minified: read_optional_flag(scope, "count_minified", "scope", true)?,
        count_generated: read_optional_flag(scope, "count_generated", "scope", true)?
    }, parse_targets(read_list(scope, "targets", "scope")?)?))
}

// 'code', 'comments' and 'extra' are written and not read: all three are worked out from the counts
// beside them, and the first two are the writer's counting model showing, while this run folds the
// classes with its own.
pub(crate) fn parse_stats(entry: &Map<String, Value>, at: &str) -> Result<Stats, DocumentError> {
    let keywords = match entry.get("keywords") {
        Some(x) => parse_keywords(read_object(x, &join_location(at, "keywords"))?, &join_location(at, "keywords"))?,
        None => HashMap::new()
    };

    Ok(Stats::new(
        read_number(entry, "files", at)?,
        read_number(entry, "bytes", at)?,
        read_number(entry, "lines", at)?,
        parse_classes(entry, at)?,
        keywords))
}

// Where every line of the counted files landed, which is what both counting models are folds of
pub(crate) fn parse_classes(entry: &Map<String, Value>, at: &str) -> Result<LineClasses, DocumentError> {
    let classes = read_nested(entry, "classes", at)?;
    let at = join_location(at, "classes");
    let mut counts = [0usize; LineClasses::NAMES.len()];
    for (slot, name) in counts.iter_mut().zip(LineClasses::NAMES) {
        *slot = read_number(classes, name, &at)?;
    }

    Ok(LineClasses::of_array(counts))
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

// A document written before nested languages existed simply has none, so it compares as a run whose
// containers held nothing rather than failing to read
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

// The modules and, beside them, how many file rows a capped '--by-file' left out of all of them
fn parse_modules(entries: &[Value]) -> Result<(Vec<ModuleResult>, usize), DocumentError> {
    let mut hidden = 0;
    let modules = entries.iter().enumerate().map(|(i, entry)| {
        let at = format!("modules[{i}]");
        let entry = read_object(entry, &at)?;
        hidden += read_optional_number(entry, "files_hidden", &at)?;

        Ok(ModuleResult {
            name: read_optional_name(entry, "name", &at)?,
            total: parse_stats(read_nested(entry, "total", &at)?, &join_location(&at, "total"))?,
            per_language: parse_languages(read_list(entry, "languages", &at)?, &join_location(&at, "languages"))?,
            nested_languages: parse_nested_languages(read_list(entry, "languages", &at)?,
                    &join_location(&at, "languages"))?,
            files: parse_files(read_list(entry, "languages", &at)?, &join_location(&at, "languages"))?
        })
    }).collect::<Result<Vec<_>, DocumentError>>()?;

    Ok((modules, hidden))
}

// Its own reader rather than 'parse_stats': a file row writes no 'files' count and no keywords, so
// the row is one file with an empty keyword map.
fn parse_files(entries: &[Value], at: &str) -> Result<HashMap<String, Vec<FileEntry>>, DocumentError> {
    let mut found = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        let at = format!("{at}[{i}]");
        let entry = read_object(entry, &at)?;
        let Some(rows) = entry.get("by_file") else { continue };
        let name = read_text(entry, "name", &at)?;

        let at = join_location(&at, "by_file");
        let rows = read_array(rows, &at)?;
        let files = rows.iter().enumerate().map(|(i, row)| {
            let at = format!("{at}[{i}]");
            let row = read_object(row, &at)?;
            Ok(FileEntry {
                path: read_text(row, "path", &at)?,
                stats: Stats::new(1, read_number(row, "bytes", &at)?, read_number(row, "lines", &at)?,
                        parse_classes(row, &at)?, HashMap::new()),
                nested_languages: HashMap::new()
            })
        }).collect::<Result<Vec<_>, DocumentError>>()?;
        found.insert(name, files);
    }

    Ok(found)
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
        let at = format!("scope.targets[{i}]");
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

// Zero when the key is absent, which is what a document of a build that never wrote it means
fn read_optional_number(parent: &Map<String, Value>, key: &str, at: &str) -> Result<usize, DocumentError> {
    match parent.get(key) {
        Some(_) => read_number(parent, key, at),
        None => Ok(0)
    }
}

// The given fallback when the key is absent, for the flags older documents never wrote
fn read_optional_flag(parent: &Map<String, Value>, key: &str, at: &str, absent: bool) -> Result<bool, DocumentError> {
    match parent.get(key) {
        Some(_) => read_flag(parent, key, at),
        None => Ok(absent)
    }
}

pub(crate) fn read_text(parent: &Map<String, Value>, key: &str, at: &str) -> Result<String, DocumentError> {
    read_member(parent, key, at)?.as_str().map(str::to_owned)
            .ok_or_else(|| DocumentError::WrongType { at: join_location(at, key), wanted: "a string" })
}

// A module that was never given a name. 'null' is the only place a member of a document may be
// empty.
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

    fn stats(files: usize, bytes: usize, lines: usize, code: usize, comments: usize,
            keywords: HashMap<String, usize>) -> Stats {
        crate::test_support::plain_stats_of(files, bytes, lines, code, comments, keywords)
    }

    // Everything the printer can put in a document: two languages, one of them with keywords and
    // one without, a named module beside the leftovers, and paths carrying the backslashes and
    // quotation marks that have to survive the escaping on the way out and the way back in.
    fn populated() -> (RunResult, Configuration) {
        let rust = stats(2, 5000, 100, 70, 10, hashmap!["structs".to_owned() => 3, "enums".to_owned() => 0]);
        let html = stats(1, 900, 40, 30, 0, HashMap::new());
        let per_language = hashmap!["Rust".to_owned() => rust.clone(), "HTML".to_owned() => html.clone()];

        let result = RunResult {
            total: Stats::total_of(&per_language),
            modules: vec![
                ModuleResult { name: Some("backend".to_owned()), per_language: hashmap!["Rust".to_owned() => rust], total: Stats::total_of(&hashmap!["Rust".to_owned() => stats(2, 5000, 100, 70, 10, hashmap!["structs".to_owned() => 3, "enums".to_owned() => 0])]), nested_languages: HashMap::new(), files: HashMap::new() },
                ModuleResult { name: None, per_language: hashmap!["HTML".to_owned() => html.clone()], total: Stats::total_of(&hashmap!["HTML".to_owned() => html]), nested_languages: HashMap::new(), files: HashMap::new() }],
            per_language,
            nested_languages: HashMap::new(),
            faulty_files: vec![FaultyFileDetails::new("D:\\dev\\a \"b\".rs".to_owned(), "stream did not contain valid UTF-8".to_owned(), 412)],
            minified_files: 0,
            generated_files: 0,
            files_present: FilesPresent { total_files: 9, relevant_files: 3, excluded_files: 4 },
            performance: Performance { duration_millis: 1180, threads: Threads::new(2, 8) },
            targets: vec![Target::named("backend", "D:/dev/api"), Target::named("backend", "D:/dev/api-v2"),
                    Target::of("D:/dev/web")],
            unreadable_dirs: vec![UnreadableDirDetails::new("D:/dev/locked".to_owned(), "Access is denied. (os error 5)".to_owned())]
        };

        let mut config = Configuration::new(vec!["./src".to_owned()]);
        config.engine.exclude_dirs = vec!["target".to_owned(), "*.min.js".to_owned()];
        config.engine.languages_of_interest = vec!["rust".to_owned()].into();
        config.engine.excluded_languages = vec!["json".to_owned()].into();
        config.view.counting = mezura_core::CountingModel::Region;
        config.engine.should_search_in_dotted = true;
        config.engine.no_gitignore = true;
        config.view.sort_by = SortCriterion::Name;
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
        assert_eq!(3, read.targets.len());
        assert_eq!(written.performance.threads, read.performance.threads);

        assert_eq!(written.modules.len(), read.modules.len());
        for (written, read) in written.modules.iter().zip(&read.modules) {
            assert_eq!(written.name, read.name);
            assert_eq!(written.total, read.total);
            assert_same_stats(&written.per_language, &read.per_language);
        }

        assert_eq!(1, read.faulty_files.len());
        assert_eq!("D:\\dev\\a \"b\".rs", read.faulty_files[0].path);
        assert_eq!("stream did not contain valid UTF-8", read.faulty_files[0].error_msg);
        assert_eq!(412, read.faulty_files[0].size);
        assert_eq!(1, read.unreadable_dirs.len());
        assert_eq!("D:/dev/locked", read.unreadable_dirs[0].path);
        assert_eq!("Access is denied. (os error 5)", read.unreadable_dirs[0].error_msg);
    }

    #[test]
    fn the_settings_the_counting_obeyed_come_back_with_it() {
        let (result, config) = populated();
        let read = parse(&create_document(&result, &Local::now(), &config)).unwrap();

        assert_eq!(crate::config_manager::VERSION_ID.trim_start_matches('v'), read.mezura_version);
        assert_eq!(vec!["target".to_owned(), "*.min.js".to_owned()], read.scope.exclude);
        assert_eq!(vec!["rust".to_owned()], read.scope.languages);
        assert_eq!(vec!["json".to_owned()], read.scope.excluded_languages);
        assert_eq!("region", read.scope.counting);
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

    #[test]
    fn what_a_document_leaves_out_reads_back_as_what_its_absence_means() {
        let (result, mut config) = populated();

        config.view.hidden.timing = true;
        let read = parse(&create_document(&result, &Local::now(), &config)).unwrap();
        assert_eq!(0, read.result.performance.duration_millis);

        // '--hide keywords' stops the counting as well as the printing, so the map comes back empty
        // rather than as a set of zeros
        config.view.hidden.timing = false;
        config.view.hidden.keywords = true;
        let read = parse(&create_document(&result, &Local::now(), &config)).unwrap();
        assert!(read.result.per_language["Rust"].keyword_occurences.is_empty());
        assert!(read.result.total.keyword_occurences.is_empty());
        assert!(!read.scope.keywords_counted);

        let mut short = populated().0;
        short.faulty_files = vec![mezura_core::FaultyFileDetails::new("a.rs".to_owned(), "no".to_owned(), 1)];
        short.unreadable_dirs = vec![mezura_core::UnreadableDirDetails::new("D:/locked".to_owned(), "no".to_owned())];
        config.view.set_should_show_faulty_files(false);
        let undetailed = parse(&create_document(&short, &Local::now(), &config)).unwrap();
        assert!(undetailed.result.faulty_files.is_empty() && undetailed.result.unreadable_dirs.is_empty());
        assert_eq!((1, 1), (undetailed.faulty_files_count, undetailed.unreadable_dirs_count));

        // A document without the key was written by a build that always counted them, so its
        // absence must not read as keywords that were hidden
        config.view.hidden.keywords = false;
        let older = create_document(&result, &Local::now(), &config).replace(",\"keywords_counted\":true", "");
        assert!(!older.contains("keywords_counted"));
        assert!(parse(&older).unwrap().scope.keywords_counted);

        let (mut plain, config) = populated();
        plain.modules = vec![ModuleResult { name: None, per_language: plain.per_language.clone(), total: plain.total.clone(),
                nested_languages: HashMap::new(), files: HashMap::new() }];
        let written = create_document(&plain, &Local::now(), &config);
        assert!(!written.contains("\"modules\""));

        let read = parse(&written).unwrap().result;
        assert_eq!(1, read.modules.len());
        assert_eq!(None, read.modules[0].name);
        assert_eq!(plain.total, read.modules[0].total);
        assert_same_stats(&plain.per_language, &read.modules[0].per_language);
    }

    #[test]
    fn the_file_rows_come_back_and_their_absence_reads_as_a_run_that_kept_none() {
        let (mut result, mut config) = populated();
        config.view.by_file = Some(crate::config_manager::ByFile::All);
        let written = FileEntry { path: "D:/dev/api/main.rs".to_owned(),
                stats: stats(1, 3000, 60, 40, 10, HashMap::new()),
                nested_languages: HashMap::new() };
        result.modules[0].files = hashmap!["Rust".to_owned() => vec![written.clone()]];

        let read = parse(&create_document(&result, &Local::now(), &config)).unwrap();
        assert!(read.files_recorded);
        let files = &read.result.modules[0].files["Rust"];
        assert_eq!(1, files.len());
        assert_eq!("D:/dev/api/main.rs", files[0].path);
        assert_eq!(written.stats, files[0].stats);

        // A capped run says how many rows its document is missing, wherever the cuts landed
        config.view.by_file = Some(crate::config_manager::ByFile::Capped(1));
        result.modules[0].files.get_mut("Rust").unwrap().push(FileEntry {
                path: "D:/dev/api/lib.rs".to_owned(),
                stats: stats(1, 500, 10, 8, 1, HashMap::new()),
                nested_languages: HashMap::new() });
        let capped = create_document(&result, &Local::now(), &config);
        let read = parse(&capped).unwrap();
        assert!(read.files_recorded);
        assert_eq!(1, read.files_hidden);

        // A document whose top level repeats what its modules already count, as the builds that
        // wrote the rows twice did, is not counted twice over
        let repeated = capped.replace("\"files_hidden\":0,\"faulty_files\"", "\"files_hidden\":1,\"faulty_files\"");
        assert_ne!(capped, repeated);
        assert_eq!(1, parse(&repeated).unwrap().files_hidden);

        // Written without '--by-file', the same run reads back as one that recorded nothing
        config.view.by_file = None;
        let read = parse(&create_document(&result, &Local::now(), &config)).unwrap();
        assert!(!read.files_recorded);
        assert_eq!(0, read.files_hidden);
        assert!(read.result.modules.iter().all(|x| x.files.is_empty()));
    }

    #[test]
    fn a_document_that_was_cut_by_top_says_how_many_languages_it_is_short() {
        let (result, mut config) = populated();
        config.view.top_n = Some(1);
        let read = parse(&create_document(&result, &Local::now(), &config)).unwrap();

        assert_eq!(1, read.languages_hidden);
        assert_eq!(1, read.result.per_language.len());
        assert_eq!(result.total, read.result.total);
    }

    // Written out by hand and not through the printer, because the warning collector belongs to the
    // whole process and every other test of this binary adds to it.
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

        assert!(error("\"warnings\": [{\"code\": 7}]").contains("'warnings[0].code' is not a string"));
        assert!(error("\"warnings\": \"none\"").contains("'warnings' is not an array"));
        assert!(error("\"total\": 5").contains("'total' is not an object"));
        assert!(error("\"modules\": [{\"name\": 7}]").contains("'modules[0].name' is not a string or null"));

        let scope_with = |targets: &str| format!("\"scope\": {{\"targets\": {targets}, \"exclude\": [], \
                \"languages\": [], \"excluded_languages\": [], \"forced_languages\": {{}}, \
                \"counting\": \"content\", \"search_in_dotted\": false, \"gitignore\": true, \
                \"keywords_counted\": true}}");
        assert!(error(&scope_with("[{\"module\": null}]")).contains("'scope.targets[0].path'"));
        assert!(error(&scope_with("[{\"module\": 7, \"path\": \"x\"}]")).contains("'scope.targets[0].module' is not a string or null"));

        let too_new = FORMAT_VERSION + 1;
        let newer = minimal_document("\"warnings\": []")
                .replace("\"format\": 1", &format!("\"format\": {too_new}"));
        let refused = parse(&newer).unwrap_err().to_string();
        assert!(refused.contains(&format!("format {too_new}")) && refused.contains("Update mezura"), "{refused}");
    }

    fn empty_classes() -> String {
        format!("{{{}}}", LineClasses::NAMES.map(|name| format!("\"{name}\": 0")).join(", "))
    }

    // Written by hand rather than by the printer, so that a test can leave a member out or spell it
    // wrong. 'body' is added to it, and replaces any member of the same name, since the same key
    // twice is valid JSON and the last one wins.
    fn minimal_document(body: &str) -> String {
        let document = format!("{{\"format\": 1, \"mezura_version\": \"3.0.0\", \
            \"generated_at\": \"2026-07-30T14:22:07+03:00\", \
            \"scope\": {{\"targets\": [], \"exclude\": [], \"languages\": [], \"excluded_languages\": [], \
                \"forced_languages\": {{}}, \"counting\": \"content\", \"search_in_dotted\": false, \
                \"gitignore\": true, \"keywords_counted\": true}}, \
            \"scan\": {{\"files_found\": 0, \"files_of_interest\": 0, \"files_excluded\": 0, \
                \"files_faulty\": 0, \"dirs_unreadable\": 0}}, \
            \"total\": {{\"files\": 0, \"lines\": 0, \"code\": 0, \"comments\": 0, \"bytes\": 0, \
                \"classes\": {}}}, \
            \"languages\": [], \"languages_hidden\": 0, \"faulty_files\": [], \"unreadable_dirs\": [], \
            \"warnings\": [], {body}}}", empty_classes());
        assert!(serde_json::from_str::<Value>(&document).is_ok(), "the test's own document is not JSON:\n{document}");

        document
    }
}
