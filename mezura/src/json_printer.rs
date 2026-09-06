use std::collections::HashMap;

use chrono::{DateTime, Local, SecondsFormat};
use mezura_core::{CountingModel, FaultyFileDetails, LineClasses, RunResult, Stats};

use super::config_manager::Configuration;
use super::result_printer;

// Bumped only when a key is removed or changes meaning. Adding one is not a bump, so a consumer can
// check this and not the version of the binary, which moves for reasons that do not concern it.
pub const FORMAT_VERSION : usize = 1;

// The file rows of one language, in the order the report shows them. The paths are never shortened
// the way the report shortens them: a document carries the only form a consumer can open.
type FilesByLanguage<'a> = HashMap<&'a str, Vec<&'a mezura_core::FileEntry>>;

// The document is a designed shape and not a serialization of the structs the program happens to
// have. It carries every number that was measured, in its raw unit, and nothing the printer computed
// in order to look right: no sizes in KB, no separators, no percentages, no bar.
pub fn print_as_json(result: &RunResult, datetime_now: &DateTime<Local>, config: &Configuration) {
    println!("{}", create_document(result, datetime_now, config));
}

pub fn create_document(result: &RunResult, datetime_now: &DateTime<Local>, config: &Configuration) -> String {
    let RunResult {per_language, total, faulty_files, unreadable_dirs, nested_languages, ..} = result;
    let (shown, hidden) = result_printer::find_shown_language_names(per_language, config);
    let file_rows = result_printer::find_files_to_show(result, config);
    // With modules the rows are written once, inside each module's own languages, and so are the
    // counts of what the cap hid: every file belongs to exactly one module
    let (files, files_hidden) = match result.has_modules() {
        true => (HashMap::new(), 0),
        false => (file_rows.first().map(find_shown_files).unwrap_or_default(),
                file_rows.iter().flat_map(HashMap::values).map(|rows| rows.hidden).sum::<usize>())
    };

    let mut members = vec![
        format!("\"format\":{FORMAT_VERSION}"),
        // What the document holds, so a consumer handed a file can tell a run from a comparison
        // without guessing from which keys exist
        String::from("\"kind\":\"run\""),
        format!("\"mezura_version\":\"{}\"", escape(config.view.version.trim_start_matches('v'))),
        format!("\"generated_at\":\"{}\"", datetime_now.to_rfc3339_opts(SecondsFormat::Secs, false)),
        format!("\"scope\":{}", create_scope_object(config,&result.targets)),
        format!("\"scan\":{}", create_scan_object(&result.files_present, result.faulty_files.len(),
                &super::json_reader::SkippedCounts::of(&result.skipped_files),
                result.unreadable_dirs.len())),
        format!("\"total\":{}", create_total_object(total, !config.view.hidden.keywords, config.view.counting)),
        format!("\"languages\":{}", create_languages_array(&shown, per_language, nested_languages, &files, config)),
        format!("\"languages_hidden\":{hidden}"),
        format!("\"files_hidden\":{files_hidden}"),
        // The paths, which '--show-faulty-files' asks for here as it does on the screen. How many
        // there were is in 'scan' either way, so an empty list never claims nothing went wrong.
        format!("\"faulty_files\":{}", create_faulty_files_array(faulty_files, config.view.should_show_faulty_files)),
        format!("\"skipped_files\":{}", create_skipped_files_object(&result.skipped_files,
                config.view.should_show_skipped_files)),
        format!("\"unreadable_dirs\":{}", create_unreadable_dirs_array(unreadable_dirs, config.view.should_show_faulty_files)),
        format!("\"warnings\":{}", create_warnings_array()),
    ];
    // Absent from a run that named no module, the same way the block is absent from the printed
    // report: a consumer that never asked for a second axis is not handed one holding everything
    if result.has_modules() {
        members.push(format!("\"modules\":{}", create_modules_array(result, &file_rows, config)));
    }
    // The only volatile block apart from the timestamp, so hiding the timing is also what makes the
    // document repeatable enough to hash or to compare against a stored one
    if !config.view.hidden.timing {
        members.push(format!("\"performance\":{}", create_performance_object(&result.performance)));
    }

    create_object(members)
}

pub fn print_comparison_as_json(comparison: &super::diff::Comparison,
        datetime_now: &DateTime<Local>, config: &Configuration) {
    println!("{}", create_comparison_document(comparison, datetime_now, config));
}

// The comparison as a document: the same vocabulary as a run's, with every count a triad of 'from',
// 'to' and 'change'. The sides carry identity and nothing else, their counts being the halves of the
// triads, and '--top' is not applied, so every language of either reading is here.
fn create_comparison_document(comparison: &super::diff::Comparison, datetime_now: &DateTime<Local>,
        config: &Configuration) -> String
{
    let (baseline, subject) = (&comparison.baseline, &comparison.subject);
    let (rows, _) = super::diff::create_comparison_rows(&baseline.result.per_language, &subject.result.per_language,
            config.view.sort_by, None, config.view.counting);
    let keywords_counted = !config.view.hidden.keywords;
    // Written only when both readings named the same modules, since a module only one of them has
    // has nothing to be compared against
    let pairs = comparison.module_pairs();

    // The changed files under each language, the same gate and the same single placement the run
    // document has: inside the modules when they are written, at the top level otherwise
    let by_file = comparison.resolve_by_file(config);
    let bases = by_file.map(|_| super::diff::determine_file_bases(&baseline.result, &subject.result))
            .unwrap_or_default();
    let (files, top_level_hidden) = match by_file.filter(|_| pairs.is_none()) {
        Some(by_file) => {
            let (files, hidden) = compare_files_per_language(&baseline.result.modules,
                    &subject.result.modules, &bases, by_file, config);
            (Some(files), hidden)
        },
        None => (None, 0)
    };
    let modules = pairs.as_ref().map(|pairs|
            create_comparison_modules_array(pairs, config, keywords_counted, by_file, &bases));

    let mut members = vec![
        format!("\"format\":{FORMAT_VERSION}"),
        String::from("\"kind\":\"comparison\""),
        format!("\"mezura_version\":\"{}\"", escape(config.view.version.trim_start_matches('v'))),
        format!("\"generated_at\":\"{}\"", datetime_now.to_rfc3339_opts(SecondsFormat::Secs, false)),
        format!("\"from\":{}", create_side_object(baseline)),
        format!("\"to\":{}", create_side_object(subject)),
        format!("\"total\":{}", create_compared_total_object(&baseline.result.total, &subject.result.total,
                keywords_counted, config.view.counting)),
        format!("\"languages\":{}", create_compared_languages_array(&rows, keywords_counted,
                find_nested(&baseline.result.nested_languages, config),
                find_nested(&subject.result.nested_languages, config),
                files.as_ref(), config.view.counting)),
        // Counts the cuts of this level's own rows, the way a run document's does: with modules
        // the rows and their cuts live inside each module
        format!("\"files_hidden\":{top_level_hidden}"),
        format!("\"warnings\":{}", create_comparison_warnings_array(&comparison.notes)),
    ];
    if let Some(rendered) = modules {
        members.push(format!("\"modules\":{rendered}"));
    }

    create_object(members)
}

fn find_nested<'a>(nested: &'a HashMap<String, HashMap<String, Stats>>, config: &Configuration)
-> Option<&'a HashMap<String, HashMap<String, Stats>>>
{
    (!config.view.hidden.nested_languages).then_some(nested)
}

// The same shape as a run document's modules, with every count a triad. '--top' does not cut these,
// the way it does not cut the languages above.
fn create_comparison_modules_array(pairs: &[super::diff::ModulePair], config: &Configuration,
        keywords_counted: bool, by_file: Option<super::config_manager::ByFile>,
        bases: &(String, String)) -> String
{
    create_array(pairs.iter().map(|pair| {
        let (rows, _) = super::diff::create_comparison_rows(&pair.before.per_language, &pair.now.per_language,
                config.view.sort_by, None, config.view.counting);
        let mut files_hidden = 0;
        let files = by_file.map(|by_file| {
            let (files, hidden) = compare_files_per_language(std::slice::from_ref(pair.before),
                    std::slice::from_ref(pair.now), bases, by_file, config);
            files_hidden = hidden;
            files
        });
        let name = pair.name.map_or("null".to_owned(), |x| format!("\"{}\"", escape(x)));
        let members = [
            format!("\"name\":{name}"),
            format!("\"total\":{}", create_compared_total_object(&pair.before.total, &pair.now.total,
                    keywords_counted, config.view.counting)),
            format!("\"languages\":{}", create_compared_languages_array(&rows, keywords_counted,
                    find_nested(&pair.before.nested_languages, config),
                    find_nested(&pair.now.nested_languages, config),
                    files.as_ref(), config.view.counting)),
            format!("\"files_hidden\":{files_hidden}"),
        ];
        create_object(members)
    }))
}

fn create_compared_languages_array(changes: &[super::diff::LanguageStatsChange],
        keywords_counted: bool, baseline_nested: Option<&HashMap<String, HashMap<String, Stats>>>,
        subject_nested: Option<&HashMap<String, HashMap<String, Stats>>>,
        files: Option<&HashMap<String, Vec<super::diff::FileStatsChange>>>,
        model: CountingModel) -> String
{
    create_array(changes.iter().map(|change| create_compared_language_object(&change.name,
            &change.baseline, &change.subject, keywords_counted,
            baseline_nested.and_then(|x| x.get(&change.name)), subject_nested.and_then(|x| x.get(&change.name)),
            files.and_then(|x| x.get(&change.name)).map(Vec::as_slice), model)))
}

fn create_compared_language_object(name: &str, baseline: &Stats, subject: &Stats,
        keywords_counted: bool, baseline_nested: Option<&HashMap<String, Stats>>,
        subject_nested: Option<&HashMap<String, Stats>>,
        files: Option<&[super::diff::FileStatsChange]>, model: CountingModel) -> String
{
    let mut members = vec![format!("\"name\":\"{}\"", escape(name))];
    members.extend(create_triad_members(baseline, subject, model));
    if keywords_counted {
        members.push(format!("\"keywords\":{}", create_keyword_triads(&baseline.keyword_occurences,
                &subject.keyword_occurences)));
    }
    if baseline_nested.is_some() || subject_nested.is_some() {
        members.push(format!("\"nested_languages\":{}", create_compared_nested_array(
                baseline_nested, subject_nested, keywords_counted, model)));
    }
    if let Some(files) = files.filter(|x| !x.is_empty()) {
        members.push(format!("\"by_file\":{}", create_compared_files_array(files, model)));
    }

    create_object(members)
}

fn compare_files_per_language(baseline_modules: &[mezura_core::ModuleResult],
        subject_modules: &[mezura_core::ModuleResult], bases: &(String, String),
        by_file: super::config_manager::ByFile, config: &Configuration)
-> (HashMap<String, Vec<super::diff::FileStatsChange>>, usize)
{
    let baseline = super::diff::collect_files_per_language(baseline_modules);
    let subject = super::diff::collect_files_per_language(subject_modules);
    let mut names = baseline.keys().chain(subject.keys()).copied().collect::<Vec<_>>();
    names.sort_unstable();
    names.dedup();

    let empty = Vec::new();
    let mut hidden = 0;
    let mut per_language = HashMap::new();
    for name in names {
        let (rows, cut) = super::diff::create_file_comparison_rows(
                baseline.get(name).unwrap_or(&empty), subject.get(name).unwrap_or(&empty),
                bases, by_file, config.view.sort_by, config.view.counting);
        hidden += cut;
        if !rows.is_empty() {
            per_language.insert(name.to_owned(), rows);
        }
    }

    (per_language, hidden)
}

// The 'files' triad is written too: an empty file that appeared or went away moves no other
// figure, and without it the row would read as a file that did not change.
fn create_compared_files_array(files: &[super::diff::FileStatsChange], model: CountingModel) -> String {
    create_array(files.iter().map(|file| {
        let mut members = vec![format!("\"path\":\"{}\"", escape(&file.path))];
        members.extend(create_triad_members(&file.baseline, &file.subject, model));
        create_object(members)
    }))
}

// A section only one reading holds is written with the other side at zero, so a '<style>' block
// that was added or taken out is a triad and not an absence
fn create_compared_nested_array(baseline: Option<&HashMap<String, Stats>>,
        subject: Option<&HashMap<String, Stats>>, keywords_counted: bool, model: CountingModel) -> String
{
    let mut names = baseline.into_iter().chain(subject).flat_map(HashMap::keys).cloned().collect::<Vec<_>>();
    names.sort_unstable();
    names.dedup();

    let of = |side: Option<&HashMap<String, Stats>>, name: &str|
            side.and_then(|x| x.get(name)).cloned().unwrap_or_default();
    create_array(names.iter().map(|name| create_compared_language_object(name,
            &of(baseline, name), &of(subject, name), keywords_counted, None, None, None, model)))
}

fn create_compared_total_object(baseline: &Stats, subject: &Stats,
        keywords_counted: bool, model: CountingModel) -> String {
    let mut lines = create_triad_members(baseline, subject, model);
    if keywords_counted {
        lines.push(format!("\"keywords\":{}", create_keyword_triads(&baseline.keyword_occurences,
                &subject.keyword_occurences)));
    }

    create_object(lines)
}

// Each kind of source has its own shape, behind the 'source' discriminator, so a consumer never
// guesses from which keys exist. The counts are not here, being the halves of the triads.
fn create_side_object(reading: &super::diff::Reading) -> String {
    let mut members = vec![match &reading.source {
        super::diff::Source::Run => String::from("\"source\":\"run\""),
        super::diff::Source::Document { path } =>
            format!("\"source\":\"document\",\"path\":\"{}\"", escape(path)),
        super::diff::Source::GitRevision { commit, asked_for } =>
            format!("\"source\":\"revision\",\"commit\":\"{}\",\"asked_for\":\"{}\"",
                    escape(commit), escape(asked_for))
    }];
    members.push(format!("\"taken_at\":\"{}\"", escape(&reading.taken)));
    members.push(format!("\"mezura_version\":\"{}\"", escape(reading.version.trim_start_matches('v'))));
    // How the scan of this side went, and only how: without it a side that failed to read half its
    // files looks like a side that shrank. Which files those were is a question for a run over that
    // side, not for a comparison of two.
    members.push(format!("\"scan\":{}", create_scan_object(&reading.result.files_present,
            reading.faulty_files_count, &reading.skipped_counts, reading.unreadable_dirs_count)));
    members.push(format!("\"scope\":{}", create_scope_object_of(&reading.scope, &reading.result.targets)));
    // Only what the side's own document recorded: what this very process warned about belongs to
    // the comparison and sits at its top level, wherever the run appears in it
    members.push(format!("\"warnings\":{}", create_document_warnings_array(&reading.warnings)));

    create_object(members)
}

// The scope in the shape the run document writes it, from wherever the reading carried it. The
// targets are in it because two sides that measured different trees would otherwise read as code
// that changed.
fn create_scope_object_of(scope: &super::json_reader::Scope, targets: &[mezura_core::Target]) -> String {
    let members = [
        format!("\"targets\":{}", create_targets_array(targets)),
        format!("\"exclude\":{}", create_string_array(&scope.exclude)),
        format!("\"languages\":{}", create_string_array(&scope.languages)),
        format!("\"excluded_languages\":{}", create_string_array(&scope.excluded_languages)),
        format!("\"forced_languages\":{}", create_forced_languages_object(&scope.forced_languages)),
        format!("\"counting\":\"{}\"", escape(&scope.counting)),
        format!("\"search_in_dotted\":{}", scope.search_in_dotted),
        format!("\"gitignore\":{}", scope.gitignore),
        format!("\"ignore_files\":{}", scope.ignore_files),
        format!("\"keywords_counted\":{}", scope.keywords_counted),
        format!("\"count_minified\":{}", scope.count_minified),
        format!("\"count_generated\":{}", scope.count_generated),
        format!("\"count_not_code\":{}", scope.count_not_code),
        format!("\"use_heuristics\":{}", scope.use_heuristics),
    ];

    create_object(members)
}

fn create_document_warnings_array(warnings: &[super::json_reader::DocumentWarning]) -> String {
    create_array(warnings.iter().map(|warning| create_object([
        format!("\"code\":\"{}\"", escape(&warning.code)),
        format!("\"affects\":\"{}\"", escape(&warning.affects)),
        format!("\"message\":\"{}\"", escape(&warning.message)),
    ])))
}

struct WarningEntry {
    code: String,
    affects: &'static str,
    subject: String,
    message: String
}

// The same facts the screen says above the table, as entries a program can key on
fn create_comparison_warnings_array(notes: &[super::diff::Note]) -> String {
    // What this very process warned about comes first, having been said first, and belongs to the
    // comparison rather than to either side: a document's own warnings are already inside it
    let mut entries = super::warning_collector::get_collected_warnings().into_iter()
            .map(|x| WarningEntry {
                code: x.code.name().to_owned(), affects: x.affects().name(),
                subject: x.subject.clone(), message: x.message.clone()
            })
            .collect::<Vec<_>>();
    for note in notes {
        entries.extend(create_note_entries(note));
    }

    create_array(entries.into_iter().map(|entry| create_object([
        format!("\"code\":\"{}\"", entry.code),
        format!("\"affects\":\"{}\"", entry.affects),
        format!("\"subject\":\"{}\"", escape(&entry.subject)),
        format!("\"message\":\"{}\"", escape(&entry.message)),
    ])))
}

// Nothing for the notes that only make sense on a screen: each side already carries its own
// doubts, and a layout is not a warning.
fn create_note_entries(note: &super::diff::Note) -> Vec<WarningEntry> {
    use super::diff::Note;
    use mezura_core::warnings::Affects;

    let entry = |code: &str, affects: Affects, subject: String, message: String| WarningEntry {
        code: code.to_owned(), affects: affects.name(), subject, message
    };
    match note {
        // The other half of 'setting-differs': one says the two disagreed, this says they were made
        // to agree. Without it both scopes read alike and nothing tells a value the command line
        // gave apart from one it borrowed.
        Note::SettingsAdopted { from, settings } => settings.iter()
                .map(|setting| entry("setting-adopted", Affects::Settings, (*setting).to_owned(),
                    format!("'{setting}' was taken from '{from}', which this run had not set itself, so both readings are counted the same way.")))
                .collect(),
        Note::SettingsDiffer { settings, .. } => settings.iter()
                .map(|setting| entry("setting-differs", Affects::Counts, (*setting).to_owned(),
                    format!("The two readings were not taken with the same '{setting}', so part of the difference is that setting and not code that changed.")))
                .collect(),
        Note::VersionsDiffer { baseline_version, subject_version, .. } => vec![
            entry("versions-differ", Affects::Counts, format!("{baseline_version} -> {subject_version}"),
                format!("The readings were counted by mezura {baseline_version} and {subject_version}, so part of the difference may be a language counted better since."))],
        // 'settings' and not 'counts': the counts are sound, the 'modules' key is simply absent,
        // and its absence would otherwise read as a run that never named a module
        Note::ModulesDiffer { baseline_modules, subject_modules, .. } => {
            let names = |modules: &Option<String>| modules.clone().unwrap_or_else(|| "none".to_owned());
            vec![entry("modules-differ", Affects::Settings,
                format!("{} -> {}", names(baseline_modules), names(subject_modules)),
                String::from("Module declarations must match between the two readings for the modules to take effect, so this document has no 'modules'."))]
        },
        Note::FilesNotRecorded { about } => vec![
            entry("files-not-recorded", Affects::Settings, about.clone(),
                format!("'{about}' was written without '--by-file', so it holds no file rows and this document carries no 'by_file'."))],
        Note::FilesCut { about, hidden } => vec![
            entry("files-cut", Affects::Settings, about.clone(),
                format!("'{about}' was written with a capped '--by-file' and is missing {hidden} of its file rows, so this document carries no 'by_file'."))],
        Note::CountsInDoubt { .. } | Note::NothingCounted { .. } | Note::LayoutFallback { .. }
        | Note::NoGitignoreInCheckout { .. } | Note::MissingInRevision { .. } => Vec::new()
    }
}

// The five figures a comparison compares, each as '{"from": a, "to": b, "change": b - a}'. 'change'
// is derived and written anyway, the difference being what was asked for.
fn create_triad_members(before: &Stats, now: &Stats, model: CountingModel) -> Vec<String> {
    [("files", before.files, now.files),
     ("lines", before.lines, now.lines),
     ("code", before.calculate_code_lines(model), now.calculate_code_lines(model)),
     ("comments", before.calculate_comment_lines(model), now.calculate_comment_lines(model)),
     ("bytes", before.bytes, now.bytes)]
            .into_iter().map(|(name, from, to)| format!("\"{name}\":{}", create_triad(from, to)))
            .collect()
}

fn create_triad(from: usize, to: usize) -> String {
    format!("{{\"from\":{from},\"to\":{to},\"change\":{}}}", to as i128 - from as i128)
}

// The union of both sides' keywords, without the ones that are zero on both: a slot every selected
// language declares and nothing ever used is a row about nothing.
fn create_keyword_triads(before: &HashMap<String, usize>, now: &HashMap<String, usize>) -> String {
    let mut names = before.keys().chain(now.keys()).cloned().collect::<Vec<_>>();
    names.sort_unstable();
    names.dedup();
    names.retain(|name| before.get(name).copied().unwrap_or(0) > 0 || now.get(name).copied().unwrap_or(0) > 0);

    create_object(names.into_iter().map(|name| {
        let (from, to) = (before.get(&name).copied().unwrap_or(0), now.get(&name).copied().unwrap_or(0));
        format!("\"{}\":{}", escape(&name), create_triad(from, to))
    }))
}

// Only what can change a number: no theme, no layout, no separators. Without it, two documents that
// differ by an '--exclude' look like a code change.
fn create_scope_object(config: &Configuration, targets: &[mezura_core::Target]) -> String {
    let members = [
        // The resolved list off the result, not the declared one off the configuration: the same
        // './src' over two different trees is two different measurements
        format!("\"targets\":{}", create_targets_array(targets)),
        format!("\"exclude\":{}", create_string_array(&config.engine.exclude_dirs)),
        format!("\"languages\":{}", create_string_array(&config.engine.languages_of_interest.to_written_form())),
        format!("\"excluded_languages\":{}", create_string_array(&config.engine.excluded_languages.to_written_form())),
        // '--force-language m=matlab' decides which language a file is counted as, so it moves
        // numbers the same way an exclusion does
        format!("\"forced_languages\":{}",
                create_forced_languages_object(&config.engine.forced_languages.to_written_form())),
        format!("\"counting\":\"{}\"", config.view.counting.name()),
        format!("\"search_in_dotted\":{}", config.engine.should_search_in_dotted),
        format!("\"gitignore\":{}", !config.engine.no_gitignore),
        format!("\"ignore_files\":{}", !config.engine.no_ignore_files),
        format!("\"keywords_counted\":{}", !config.view.hidden.keywords),
        format!("\"count_minified\":{}", config.engine.count_minified),
        format!("\"count_generated\":{}", config.engine.count_generated),
        format!("\"count_not_code\":{}", config.engine.count_not_code),
        format!("\"use_heuristics\":{}", config.engine.use_heuristics),
    ];

    create_object(members)
}

// Whether the counts beside it are complete. 'files_of_interest' is not the file count of the total
// below: a faulty file was found and is of interest, and nothing of it was counted. The counts
// arrive apart from the lists in the result, because for a side read from a document the lists are
// only there when that run detailed them while its scan block counted either way.
fn create_scan_object(files_present: &mezura_core::FilesPresent, faulty_files_count: usize,
        skipped: &super::json_reader::SkippedCounts, unreadable_dirs_count: usize) -> String
{
    let members = [
        format!("\"files_found\":{}", files_present.total_files),
        format!("\"files_of_interest\":{}", files_present.relevant_files),
        format!("\"files_excluded\":{}", files_present.excluded_files),
        format!("\"files_faulty\":{faulty_files_count}"),
        format!("\"files_minified\":{}", skipped.minified),
        format!("\"files_generated\":{}", skipped.generated),
        format!("\"files_not_code\":{}", skipped.not_code),
        format!("\"dirs_unreadable\":{unreadable_dirs_count}"),
    ];

    create_object(members)
}

fn create_total_object(total: &Stats, keywords_counted: bool, model: CountingModel) -> String {
    let mut members = vec![
        format!("\"files\":{}", total.files),
        format!("\"lines\":{}", total.lines),
        format!("\"code\":{}", total.calculate_code_lines(model)),
        format!("\"comments\":{}", total.calculate_comment_lines(model)),
        format!("\"{}\":{}", model.get_third_quantity_name(), total.calculate_extra_lines(model)),
        format!("\"bytes\":{}", total.bytes),
        format!("\"classes\":{}", create_classes_object(&total.classes)),
    ];
    // Every language of the run added up, which is the only place the figure survives '--top': the
    // languages are cut there and the ones left cannot be added back up to it.
    if keywords_counted {
        members.push(format!("\"keywords\":{}", create_keywords_object(&total.keyword_occurences)));
    }

    create_object(members)
}

// The leftovers of the named modules carry 'null' and not the '(unnamed)' the report prints: a
// marker spelled as a name is one a real module could be given, and a consumer grouping by that key
// would silently merge the two.
fn create_modules_array(result: &RunResult, file_rows: &[result_printer::FileRowsOfModule],
        config: &Configuration) -> String
{
    create_array(result.modules.iter().zip(file_rows).map(|(module, files)| {
        let (shown, hidden) = result_printer::find_shown_language_names(&module.per_language, config);
        let files_hidden = files.values().map(|rows| rows.hidden).sum::<usize>();
        let files = find_shown_files(files);
        let name = module.name.as_ref().map_or("null".to_owned(), |x| format!("\"{}\"", escape(x)));
        let members = [
            format!("\"name\":{name}"),
            format!("\"total\":{}", create_total_object(&module.total, !config.view.hidden.keywords,
                    config.view.counting)),
            format!("\"languages\":{}", create_languages_array(&shown, &module.per_language,
                    &module.nested_languages, &files, config)),
            format!("\"languages_hidden\":{hidden}"),
            format!("\"files_hidden\":{files_hidden}"),
        ];
        create_object(members)
    }))
}

fn find_shown_files<'a>(of_module: &'a result_printer::FileRowsOfModule<'a>) -> FilesByLanguage<'a> {
    of_module.iter().map(|(language, rows)|
            (*language, rows.shown.iter().map(|(_, file)| *file).collect())).collect()
}

// An array and not an object keyed by language name, so that the order '--sort' chose survives and
// so that no language can collide with a key of the document.
fn create_languages_array(shown: &[String], per_language: &HashMap<String, Stats>,
        nested_languages: &HashMap<String, HashMap<String, Stats>>,
        files: &FilesByLanguage, config: &Configuration) -> String
{
    create_array(shown.iter().filter_map(|name| {
        Some(create_language_object(name, per_language.get(name)?, !config.view.hidden.keywords,
                !config.view.hidden.nested_languages, nested_languages.get(name),
                files.get(name.as_str()).map(Vec::as_slice).unwrap_or_default(), config.view.counting))
    }))
}

fn create_files_array(files: &[&mezura_core::FileEntry], nested_shown: bool, model: CountingModel) -> String {
    create_array(files.iter().map(|file| {
        let stats = &file.stats;
        let mut members = vec![
            format!("\"path\":\"{}\"", escape(&file.path)),
            format!("\"lines\":{}", stats.lines),
            format!("\"code\":{}", stats.calculate_code_lines(model)),
            format!("\"comments\":{}", stats.calculate_comment_lines(model)),
            format!("\"{}\":{}", model.get_third_quantity_name(),
                    stats.calculate_extra_lines(model)),
            format!("\"bytes\":{}", stats.bytes),
            format!("\"classes\":{}", create_classes_object(&stats.classes)),
        ];
        if nested_shown && !file.nested_languages.is_empty() {
            members.push(format!("\"nested_languages\":{}",
                    create_nested_languages_array(&file.nested_languages, model)));
        }
        create_object(members)
    }))
}

fn create_nested_languages_array(sections: &HashMap<String, Stats>, model: CountingModel) -> String {
    let mut sorted = sections.iter().collect::<Vec<_>>();
    sorted.sort_unstable_by_key(|(name, _)| name.as_str());

    create_array(sorted.into_iter().map(|(name, info)| format!(
"{{\"name\":\"{}\",\"files\":{},\"lines\":{},\"code\":{},\"comments\":{},\"{}\":{},\
\"bytes\":{},\"classes\":{}}}",
            escape(name), info.files, info.lines, info.calculate_code_lines(model),
            info.calculate_comment_lines(model), model.get_third_quantity_name(),
            info.calculate_extra_lines(model), info.bytes,
            create_classes_object(&info.classes))))
}

// 'nested_shown' reaches the language's own breakdown and the one inside each of its files alike
fn create_language_object(name: &str, info: &Stats, keywords_counted: bool, nested_shown: bool,
        sections: Option<&HashMap<String, Stats>>, files: &[&mezura_core::FileEntry],
        model: CountingModel) -> String
{
    let mut members = vec![
        format!("\"name\":\"{}\"", escape(name)),
        format!("\"files\":{}", info.files),
        format!("\"lines\":{}", info.lines),
        format!("\"code\":{}", info.calculate_code_lines(model)),
        format!("\"comments\":{}", info.calculate_comment_lines(model)),
        format!("\"{}\":{}", model.get_third_quantity_name(), info.calculate_extra_lines(model)),
        format!("\"bytes\":{}", info.bytes),
        format!("\"classes\":{}", create_classes_object(&info.classes)),
    ];
    // Absent when they were not counted, since '--hide keywords' also stops the counting. An empty
    // object means the opposite: they were counted and the language declares none.
    if keywords_counted {
        members.push(format!("\"keywords\":{}", create_keywords_object(&info.keyword_occurences)));
    }
    if let Some(sections) = sections.filter(|x| nested_shown && !x.is_empty()) {
        members.push(format!("\"nested_languages\":{}", create_nested_languages_array(sections, model)));
    }
    // Named after the command and not 'files', which this object already uses for how many there are
    if !files.is_empty() {
        members.push(format!("\"by_file\":{}", create_files_array(files, nested_shown, model)));
    }

    create_object(members)
}

// The raw counts behind the folded columns, one member per class, so a consumer can fold them
// under either model whatever the scope's own was
fn create_classes_object(classes: &LineClasses) -> String {
    create_object(LineClasses::NAMES.iter().zip(classes.to_array())
            .map(|(name, count)| format!("\"{name}\":{count}")))
}

fn create_keywords_object(occurences: &HashMap<String, usize>) -> String {
    let mut sorted = occurences.iter().collect::<Vec<_>>();
    sorted.sort_unstable_by_key(|(name, _)| name.as_str());
    create_object(sorted.into_iter()
            .map(|(name, count)| format!("\"{}\":{count}", escape(name))))
}

// Everything the run said on the error output, which a machine consumer never sees. Always present,
// empty array included. 'code' is the half that is safe to branch on, 'message' the half that is
// safe to show, and 'affects' is what lets a consumer written today keep working when a later
// version adds a code it has never heard of. In the order they were printed.
fn create_warnings_array() -> String {
    create_array(super::warning_collector::get_collected_warnings().iter().map(|warning| create_object([
        format!("\"code\":\"{}\"", escape(warning.code.name())),
        format!("\"affects\":\"{}\"", warning.affects().name()),
        format!("\"subject\":\"{}\"", escape(&warning.subject)),
        format!("\"message\":\"{}\"", escape(&warning.message)),
    ])))
}

fn create_skipped_files_object(skipped: &mezura_core::SkippedFiles, asked_for: bool) -> String {
    let paths = |list: &[String]| {
        if !asked_for {
            return String::from("[]");
        }
        create_array(list.iter().map(|path| format!("\"{}\"", escape(path))))
    };

    create_object(mezura_core::ScanSkip::ALL.map(|kind|
            format!("\"{}\":{}", kind.name(), paths(skipped.get_of_kind(kind)))))
}

// Sorted by path, because the faulty files are collected by whichever thread hit them and their
// order would otherwise change between two runs over the same tree
fn create_faulty_files_array(faulty_files: &[FaultyFileDetails], asked_for: bool) -> String {
    if !asked_for {
        return String::from("[]");
    }

    let mut sorted = faulty_files.iter().collect::<Vec<_>>();
    sorted.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    create_array(sorted.into_iter().map(|file| create_object([
        format!("\"path\":\"{}\"", escape(&file.path)),
        format!("\"bytes\":{}", file.size),
        format!("\"error\":\"{}\"", escape(&file.error_msg)),
    ])))
}

// Objects and not bare paths, and sorted for the same reason as the faulty files above: a consumer
// has to be able to tell a refused permission apart from a directory that went away mid-walk.
fn create_unreadable_dirs_array(unreadable_dirs: &[mezura_core::UnreadableDirDetails], asked_for: bool) -> String {
    if !asked_for {
        return String::from("[]");
    }

    let mut sorted = unreadable_dirs.iter().collect::<Vec<_>>();
    sorted.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    create_array(sorted.into_iter().map(|dir| create_object([
        format!("\"path\":\"{}\"", escape(&dir.path)),
        format!("\"error\":\"{}\"", escape(&dir.error_msg)),
    ])))
}

// 'scan_ms' and not the 'Exec time' of the footer: what is measured here starts before the producers
// and ends when the consumers are done, which is the phase 'scan' describes. The thread counts come
// from the result and not from the configuration, which holds what was asked for while the operating
// system is allowed to grant fewer.
fn create_performance_object(performance: &mezura_core::Performance) -> String {
    let threads = format!("{{\"producers\":{},\"consumers\":{}}}",
            performance.threads.producers(), performance.threads.consumers());

    format!("{{\"scan_ms\":{},\"threads\":{threads}}}", performance.duration_millis)
}

// One entry per target and not one per module: a module given several paths is several targets that
// share a name, and grouping them would lose the order they were declared in, which the columns of
// the report follow.
fn create_targets_array(targets: &[mezura_core::Target]) -> String {
    create_array(targets.iter().map(|target| {
        let module = target.module.as_ref().map_or("null".to_owned(), |x| format!("\"{}\"", escape(x)));
        format!("{{\"module\":{module},\"path\":\"{}\"}}", escape(&target.path))
    }))
}

// The extension is the key, since that is what a run is asked about and what can only be claimed
// once. Sorted, so that two runs over the same tree produce the same bytes.
fn create_forced_languages_object(forced: &HashMap<String, String>) -> String {
    let mut sorted = forced.iter().collect::<Vec<_>>();
    sorted.sort_unstable_by_key(|(extension, _)| extension.as_str());
    create_object(sorted.into_iter()
            .map(|(extension, language)| format!("\"{}\":\"{}\"", escape(extension), escape(language))))
}

fn create_string_array(values: &[String]) -> String {
    create_array(values.iter().map(|x| format!("\"{}\"", escape(x))))
}

// Paths are the reason this has to be right: on Windows they arrive with backslashes in them, so
// every single document would be invalid JSON without the escape.
pub(crate) fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '"'  => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            x if (x as u32) < 0x20 => escaped.push_str(&format!("\\u{:04x}", x as u32)),
            x => escaped.push(x)
        }
    }

    escaped
}

pub(crate) fn create_object(members: impl IntoIterator<Item = String>) -> String {
    format!("{{{}}}", members.into_iter().collect::<Vec<_>>().join(","))
}

pub(crate) fn create_array(entries: impl IntoIterator<Item = String>) -> String {
    format!("[{}]", entries.into_iter().collect::<Vec<_>>().join(","))
}

#[cfg(test)]
mod tests {
    use mezura_core::FilesPresent;

    use crate::config_manager::{Layout, SortCriterion};

    use super::*;

    fn stats_of(files: usize, bytes: usize, lines: usize, code: usize, comments: usize, keywords: HashMap<String,usize>) -> Stats {
        crate::test_support::plain_stats_of(files, bytes, lines, code, comments, keywords)
    }

    fn result_of(per_language: HashMap<String, Stats>, total: Stats,
            faulty_files: Vec<FaultyFileDetails>, files_present: FilesPresent) -> RunResult
    {
        RunResult {per_language, modules: Vec::new(), nested_languages: HashMap::new(), total, faulty_files,
                skipped_files: mezura_core::SkippedFiles::default(), files_present, targets: Vec::new(), unreadable_dirs: Vec::new(),
                performance: mezura_core::Performance { duration_millis: 1180, threads: mezura_core::Threads::new(2, 8) }}
    }

    // Through the local zone, since the document prints local time and the runner's zone is not ours
    fn generated_at() -> DateTime<Local> {
        DateTime::parse_from_rfc3339("2026-07-30T14:22:07+03:00").unwrap().with_timezone(&Local)
    }

    fn document_of(config: &crate::config_manager::Configuration) -> String {
        // Summed rather than written out, so the document is one a run could have produced
        let per_language = hashmap![
            "Rust".to_owned() => stats_of(2, 5000, 100, 70, 10, hashmap!["structs".to_owned() => 3, "enums".to_owned() => 1]),
            "HTML".to_owned() => stats_of(1, 900, 40, 30, 0, HashMap::new())];
        let result = result_of(per_language.clone(), Stats::total_of(&per_language), Vec::new(),
            FilesPresent {total_files: 5, relevant_files: 3, excluded_files: 2});
        create_document(&result, &generated_at(), config)
    }

    #[test]
    fn every_character_that_json_cannot_carry_raw_is_escaped() {
        assert_eq!("a\\\\b", escape("a\\b"));
        assert_eq!("D:\\\\dev\\\\a \\\"b\\\".rs", escape("D:\\dev\\a \"b\".rs"));
        assert_eq!("one\\ntwo\\tthree", escape("one\ntwo\tthree"));
        assert_eq!("\\u0007", escape("\u{7}"));
        assert_eq!("Δ ok", escape("Δ ok"));
    }

    #[test]
    fn the_document_carries_the_raw_counts_and_none_of_the_presentation() {
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.view.layout = Layout::Boxed;
        let document = document_of(&config);

        assert!(document.contains(&format!("\"format\":{FORMAT_VERSION}")));
        assert!(document.contains(&format!("\"mezura_version\":\"{}\"",
                crate::config_manager::VERSION_ID.trim_start_matches('v'))));
        assert!(document.contains(&format!("\"generated_at\":\"{}\"",
                generated_at().to_rfc3339_opts(SecondsFormat::Secs, false))));
        assert!(document.contains("\"lines\":140"));
        assert!(document.contains("\"scan_ms\":1180"));
        // Nothing the printed output adds: no separators in the numbers, no size measurement, no
        // percentage, and no layout or theme among the settings
        assert!(!document.contains("5,900"));
        assert!(!document.contains("KB"));
        assert!(!document.contains('%'));
        assert!(!document.contains("boxed"));
    }

    #[test]
    fn sort_orders_the_languages_and_top_cuts_them_while_the_total_stays_whole() {
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.view.sort_by = SortCriterion::Name;
        let document = document_of(&config);
        assert!(document.find("\"HTML\"").unwrap() < document.find("\"Rust\"").unwrap());

        config.view.sort_by = SortCriterion::Lines;
        let document = document_of(&config);
        assert!(document.find("\"Rust\"").unwrap() < document.find("\"HTML\"").unwrap());

        config.view.top_n = Some(1);
        let document = document_of(&config);
        assert!(document.contains("\"Rust\""));
        assert!(!document.contains("\"HTML\""));
        assert!(document.contains("\"languages_hidden\":1"));
        assert!(document.contains("\"lines\":140"));
    }

    #[test]
    fn hiding_the_keywords_removes_the_key_while_a_language_without_any_gets_an_empty_one() {
        let config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let document = document_of(&config);
        assert!(document.contains("\"keywords\":{}"));
        assert!(document.contains("\"structs\":3"));
        assert!(document.contains("\"keywords_counted\":true"));
        // Sorted by name, so that two runs over the same tree produce the same bytes
        assert!(document.find("\"enums\"").unwrap() < document.find("\"structs\"").unwrap());

        // The total carries them too, being the only figure that survives a '--top' cutting the
        // languages they would otherwise have to be added back up from
        let at = document.find("\"total\"").unwrap();
        let total_block = &document[at..at + document[at..].find("\"languages\"").unwrap()];
        assert!(total_block.contains("\"keywords\":{"), "{total_block}");
        assert!(total_block.contains("\"structs\":3") && total_block.contains("\"enums\":1"), "{total_block}");

        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.view.hidden.keywords = true;
        let document = document_of(&config);
        assert!(!document.contains("\"keywords\""));
        assert!(document.contains("\"keywords_counted\":false"));
    }

    #[test]
    fn hiding_the_timing_removes_the_only_block_that_changes_between_two_identical_runs() {
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.view.hidden.timing = true;
        let document = document_of(&config);

        assert!(!document.contains("\"performance\""));
        assert!(!document.contains("\"scan_ms\""));
    }

    #[test]
    fn the_modules_appear_only_when_one_was_named_and_the_leftovers_have_no_name() {
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        assert!(!document_of(&config).contains("\"modules\""));

        let module_of = |name: Option<&str>, language: &str, lines: usize, files: usize| {
            let per_language = hashmap![language.to_owned() => stats_of(files, lines * 10, lines, lines, 0, HashMap::new())];
            let total = Stats::total_of(&per_language);
            mezura_core::ModuleResult {name: name.map(str::to_owned), per_language, total,
                    nested_languages: HashMap::new(), files: HashMap::new()}
        };
        let mut result = result_of(
            hashmap!["Rust".to_owned() => stats_of(2, 1000, 100, 100, 0, HashMap::new()),
                     "HTML".to_owned() => stats_of(1, 400, 40, 40, 0, HashMap::new())],
            stats_of(3, 1400, 140, 140, 0, HashMap::new()), Vec::new(),
            FilesPresent {total_files: 3, relevant_files: 3, excluded_files: 0});
        result.modules = vec![module_of(Some("backend"), "Rust", 100, 2), module_of(None, "HTML", 40, 1)];

        config.view.hidden.timing = true;
        let rendered = create_document(&result, &Local::now(), &config);
        assert!(rendered.contains("\"name\":\"backend\""));
        assert!(rendered.contains("\"name\":null"));
        let block = &rendered[rendered.find("\"modules\"").unwrap()..];
        assert_eq!(2, block.matches("\"total\":").count());
        assert_eq!(2, block.matches("\"languages\":").count());
        assert!(block.contains("\"lines\":100") && block.contains("\"lines\":40"));
        assert!(rendered.contains("\"lines\":140"));
        assert!(rendered.contains("\"languages_hidden\":0"));

        // '--top' is per module here, so one with a single language is not cut by '--top 1'
        config.view.top_n = Some(1);
        let cut = create_document(&result, &Local::now(), &config);
        assert_eq!(2, cut.matches("\"languages_hidden\":0").count());
        assert!(cut.contains("\"languages_hidden\":1"));
    }

    #[test]
    fn a_warning_reaches_the_document_with_both_of_its_halves() {
        let config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        // Whether the array is empty cannot be asserted: the collector belongs to the whole process
        // and every other test of this binary adds to it.
        assert!(document_of(&config).contains("\"warnings\":["));

        super::super::warning_collector::keep(mezura_core::warnings::Warning::new(
                mezura_core::warnings::Code::LanguageTiebreak, "a-subject-only-this-test-uses",
                "quoted \"text\" and a \\ backslash".to_owned()));

        let rendered = create_warnings_array();
        assert!(rendered.contains("\"subject\":\"a-subject-only-this-test-uses\""));
        assert!(rendered.contains("\"code\":\"language-tiebreak\""));
        assert!(rendered.contains("\"affects\":\"counts\""));
        assert!(rendered.contains("quoted \\\"text\\\" and a \\\\ backslash"));
    }

    #[test]
    fn the_extensions_that_were_forced_to_a_language_are_among_the_settings() {
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        assert!(document_of(&config).contains("\"forced_languages\":{}"));

        config.engine.forced_languages = hashmap!["m".to_owned() => "matlab".to_owned(),
                "h".to_owned() => "objective-c".to_owned()].into();
        let document = document_of(&config);
        assert!(document.contains("\"m\":\"matlab\""), "{document}");
        // sorted, so that two runs over the same tree produce the same bytes
        assert!(document.find("\"h\":").unwrap() < document.find("\"m\":").unwrap());
    }

    #[test]
    fn the_targets_are_written_one_by_one_with_the_module_that_claimed_each() {
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.view.hidden.timing = true;
        let mut result = result_of(HashMap::new(), Stats::default(), Vec::new(),
                FilesPresent {total_files: 0, relevant_files: 0, excluded_files: 0});
        result.targets = vec![mezura_core::Target::named("tests", "D:/api/tests"),
                mezura_core::Target::named("tests", "D:/web/tests"),
                mezura_core::Target::of("D:\\web")];
        let written = create_document(&result, &Local::now(), &config);

        assert!(written.contains("{\"module\":\"tests\",\"path\":\"D:/api/tests\"}"), "{written}");
        assert_eq!(2, written.matches("\"module\":\"tests\"").count());
        assert!(written.contains("{\"module\":null,\"path\":\"D:\\\\web\"}"), "{written}");
        // in the order they were declared, which the report's columns follow, and not sorted
        assert!(written.find("api/tests").unwrap() < written.find("web/tests").unwrap());

        result.targets = Vec::new();
        assert!(create_document(&result, &Local::now(), &config).contains("\"targets\":[]"));
    }

    fn file_entry(path: &str, lines: usize, code: usize, bytes: usize) -> mezura_core::FileEntry {
        mezura_core::FileEntry { path: path.to_owned(),
                stats: stats_of(1, bytes, lines, code, 0, HashMap::new()),
                nested_languages: HashMap::new() }
    }

    #[test]
    fn the_file_rows_of_a_run_with_modules_are_written_once_inside_them() {
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.view.by_file = Some(crate::config_manager::ByFile::All);
        config.view.hidden.timing = true;

        let module_of = |name: Option<&str>, language: &str, path: &str| {
            let per_language = hashmap![language.to_owned() => stats_of(1, 900, 30, 24, 0, HashMap::new())];
            let total = Stats::total_of(&per_language);
            mezura_core::ModuleResult {name: name.map(str::to_owned), per_language, total,
                    nested_languages: HashMap::new(),
                    files: hashmap![language.to_owned() => vec![file_entry(path, 30, 24, 900)]]}
        };
        let mut result = result_of(
            hashmap!["Rust".to_owned() => stats_of(1, 900, 30, 24, 0, HashMap::new()),
                     "HTML".to_owned() => stats_of(1, 900, 30, 24, 0, HashMap::new())],
            stats_of(2, 1800, 60, 48, 0, HashMap::new()), Vec::new(),
            FilesPresent {total_files: 2, relevant_files: 2, excluded_files: 0});
        result.modules = vec![module_of(Some("backend"), "Rust", "D:/x/api/a.rs"),
                module_of(None, "HTML", "D:/x/web/i.html")];

        let written = create_document(&result, &Local::now(), &config);
        let modules_at = written.find("\"modules\"").unwrap();
        assert_eq!(2, written.matches("\"by_file\"").count(), "{written}");
        assert!(!written[..modules_at].contains("\"by_file\""), "{written}");

        // and a run that named no module keeps them at the top level
        result.modules = vec![mezura_core::ModuleResult {name: None, per_language: result.per_language.clone(),
                total: result.total.clone(), nested_languages: HashMap::new(),
                files: hashmap!["Rust".to_owned() => vec![file_entry("D:/x/api/a.rs", 30, 24, 900)]]}];
        let written = create_document(&result, &Local::now(), &config);
        assert!(!written.contains("\"modules\""));
        assert_eq!(1, written.matches("\"by_file\"").count(), "{written}");
        assert!(written.contains("\"path\":\"D:/x/api/a.rs\""), "{written}");
    }

    fn reading_of(source: crate::diff::Source, per_language: HashMap<String, Stats>) -> crate::diff::Reading {
        crate::diff::Reading {
            source,
            taken: "2026-08-05T21:14:03+03:00".to_owned(),
            version: "3.0.0".to_owned(),
            scope: crate::diff::scope_of(&mezura_core::EngineConfig::default(), mezura_core::CountingModel::Content),
            warnings: Vec::new(),
            faulty_files_count: 0,
            skipped_counts: crate::json_reader::SkippedCounts::default(),
            unreadable_dirs_count: 0,
            files_recorded: true,
            files_hidden: 0,
            result: result_of(per_language.clone(), Stats::total_of(&per_language), Vec::new(),
                    FilesPresent {total_files: 2, relevant_files: 2, excluded_files: 0})
        }
    }

    #[test]
    fn a_comparison_document_holds_both_sides_of_every_figure_and_who_the_sides_were() {
        let config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let datetime = DateTime::parse_from_rfc3339("2026-08-06T15:00:00+03:00").unwrap().with_timezone(&Local);
        let mut from = reading_of(crate::diff::Source::GitRevision {
                commit: "030e6e72a1b4c9d8e7f6a5b4c3d2e1f0a9b8c7d6".to_owned(), asked_for: "v2.0.1".to_owned() },
                hashmap!["Rust".to_owned() => stats_of(2, 3000, 100, 70, 10, hashmap!["structs".to_owned() => 3]),
                         "Java".to_owned() => stats_of(1, 400, 40, 30, 0, HashMap::new())]);
        from.result.targets = vec![mezura_core::Target::named("core", "D:/proj/src")];
        let to = reading_of(crate::diff::Source::Run,
                hashmap!["Rust".to_owned() => stats_of(3, 4500, 150, 100, 20, hashmap!["structs".to_owned() => 5]),
                         "Go".to_owned() => stats_of(1, 600, 60, 50, 0, HashMap::new())]);

        let document = create_comparison_document(&crate::diff::Comparison::of(from, to, &config, Vec::new()), &datetime, &config);
        assert!(document.contains("\"kind\":\"comparison\""));
        assert!(document.contains("\"source\":\"revision\""), "{document}");
        assert!(document.contains("\"commit\":\"030e6e72a1b4c9d8e7f6a5b4c3d2e1f0a9b8c7d6\""));
        assert!(document.contains("\"asked_for\":\"v2.0.1\""));
        assert!(document.contains("\"source\":\"run\""));
        // each side says what it measured, or two sides over different trees would read as change
        assert!(document.contains("{\"module\":\"core\",\"path\":\"D:/proj/src\"}"), "{document}");

        assert!(document.contains("\"lines\":{\"from\":140,\"to\":210,\"change\":70}"), "{document}");
        // a language of only one side has a whole zero side, and the change can be negative
        assert!(document.contains("\"lines\":{\"from\":40,\"to\":0,\"change\":-40}"), "{document}");
        assert!(document.contains("\"structs\":{\"from\":3,\"to\":5,\"change\":2}"), "{document}");

        assert!(serde_json::from_str::<serde_json::Value>(&document).is_ok(), "{document}");
    }

    #[test]
    fn the_modules_of_a_comparison_are_written_only_when_both_readings_named_the_same_ones() {
        let config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let datetime = Local::now();
        let module = |name: Option<&str>, language: &str, lines: usize, structs: usize| {
            let per_language = hashmap![language.to_owned() =>
                    stats_of(1, lines * 10, lines, lines, 0, hashmap!["structs".to_owned() => structs])];
            mezura_core::ModuleResult {name: name.map(str::to_owned), total: Stats::total_of(&per_language), per_language,
                    nested_languages: HashMap::new(), files: HashMap::new()}
        };
        let with_modules = |source, modules: Vec<mezura_core::ModuleResult>| {
            let mut reading = reading_of(source, HashMap::new());
            reading.result.modules = modules;
            reading
        };

        let from = || with_modules(crate::diff::Source::Document {path: "D:/old.json".to_owned()},
                vec![module(Some("backend"), "Rust", 100, 3), module(None, "HTML", 40, 0)]);
        let to = with_modules(crate::diff::Source::Run,
                vec![module(Some("backend"), "Rust", 150, 5), module(None, "HTML", 40, 0)]);

        let document = create_comparison_document(&crate::diff::Comparison::of(from(), to, &config, Vec::new()), &datetime, &config);
        assert!(document.contains("\"modules\":["), "{document}");
        assert!(document.contains("\"name\":\"backend\"") && document.contains("\"name\":null"));
        let block = &document[document.find("\"modules\"").unwrap()..];
        assert!(block.contains("\"lines\":{\"from\":100,\"to\":150,\"change\":50}"), "{block}");
        assert!(block.contains("\"structs\":{\"from\":3,\"to\":5,\"change\":2}"), "{block}");
        assert!(block.contains("\"lines\":{\"from\":40,\"to\":40,\"change\":0}"), "{block}");
        assert!(serde_json::from_str::<serde_json::Value>(&document).is_ok(), "{document}");

        // A module only one of them has takes the whole key with it, and the reader is told why
        let renamed = with_modules(crate::diff::Source::Run,
                vec![module(Some("api"), "Rust", 150, 5), module(None, "HTML", 40, 0)]);
        let document = create_comparison_document(&crate::diff::Comparison::of(from(), renamed, &config, Vec::new()), &datetime, &config);
        assert!(!document.contains("\"modules\""), "{document}");
        assert!(document.contains("\"code\":\"modules-differ\""), "{document}");
        assert!(document.contains("\"affects\":\"settings\""));
        assert!(document.contains("\"subject\":\"'backend', '(unnamed)' -> 'api', '(unnamed)'\""), "{document}");

        let plain = create_comparison_document(&crate::diff::Comparison::of(
                reading_of(crate::diff::Source::Run, HashMap::new()),
                reading_of(crate::diff::Source::Run, HashMap::new()), &config, Vec::new()), &datetime, &config);
        assert!(!plain.contains("modules"), "{plain}");
    }

    #[test]
    fn a_comparison_carries_the_changed_files_when_both_sides_recorded_them() {
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.view.by_file = Some(crate::config_manager::ByFile::All);
        let side_with = |source, files: Vec<mezura_core::FileEntry>| {
            let per_language = hashmap!["Rust".to_owned() => stats_of(2, 5000, 100, 70, 10, HashMap::new())];
            let mut reading = reading_of(source, per_language.clone());
            reading.result.modules = vec![mezura_core::ModuleResult { name: None,
                    total: Stats::total_of(&per_language), per_language, nested_languages: HashMap::new(),
                    files: hashmap!["Rust".to_owned() => files] }];
            reading
        };
        let from = || side_with(crate::diff::Source::Document { path: "D:/old.json".to_owned() },
                vec![file_entry("D:/proj/a.rs", 100, 70, 3000), file_entry("D:/proj/b.rs", 50, 40, 1500),
                     file_entry("D:/proj/gone.rs", 10, 5, 300)]);
        let to = || side_with(crate::diff::Source::Run,
                vec![file_entry("D:/proj/a.rs", 130, 90, 3900), file_entry("D:/proj/b.rs", 50, 40, 1500)]);

        let document = create_comparison_document(&crate::diff::Comparison::of(from(), to(), &config, Vec::new()),
                &Local::now(), &config);
        assert!(document.contains("\"by_file\":["), "{document}");
        assert!(document.contains("\"path\":\"D:/proj/a.rs\""), "{document}");
        assert!(document.contains("\"lines\":{\"from\":100,\"to\":130,\"change\":30}"), "{document}");
        assert!(document.contains("\"path\":\"D:/proj/gone.rs\""), "{document}");
        assert!(document.contains("\"lines\":{\"from\":10,\"to\":0,\"change\":-10}"), "{document}");
        assert!(document.contains("\"files_hidden\":0"), "{document}");
        // the unchanged file has no row, and the files triad is written so that an empty file
        // that appeared or went away still says so
        assert!(!document.contains("\"path\":\"D:/proj/b.rs\""), "{document}");
        assert!(document.contains("\"files\":{\"from\":1,\"to\":1,\"change\":0}"), "{document}");
        assert!(document.contains("\"files\":{\"from\":1,\"to\":0,\"change\":-1}"), "{document}");
        assert!(serde_json::from_str::<serde_json::Value>(&document).is_ok(), "{document}");

        // the cap keeps the biggest mover and the document says how many it left out
        config.view.by_file = Some(crate::config_manager::ByFile::Capped(1));
        let document = create_comparison_document(&crate::diff::Comparison::of(from(), to(), &config, Vec::new()),
                &Local::now(), &config);
        assert!(document.contains("\"path\":\"D:/proj/a.rs\"") && !document.contains("gone.rs"), "{document}");
        assert!(document.contains("\"files_hidden\":1"), "{document}");

        // a baseline that recorded no rows takes the whole key with it, and the reader is told why
        config.view.by_file = Some(crate::config_manager::ByFile::All);
        let mut unrecorded = from();
        unrecorded.files_recorded = false;
        unrecorded.result.modules[0].files = HashMap::new();
        let document = create_comparison_document(&crate::diff::Comparison::of(unrecorded, to(), &config, Vec::new()),
                &Local::now(), &config);
        assert!(!document.contains("\"by_file\""), "{document}");
        assert!(document.contains("\"code\":\"files-not-recorded\""), "{document}");
        assert!(document.contains("\"files_hidden\":0"), "{document}");
    }

    #[test]
    fn each_side_of_a_comparison_says_how_its_own_scan_went() {
        let config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let with_trouble = |source| {
            let mut reading = reading_of(source, hashmap!["Rust".to_owned() => stats_of(1, 30, 10, 5, 0, HashMap::new())]);
            reading.result.files_present = FilesPresent {total_files: 9, relevant_files: 4, excluded_files: 5};
            // The counts and not the lists, which is what a document written without
            // '--show-faulty-files' carries
            reading.faulty_files_count = 2;
            reading.unreadable_dirs_count = 1;
            reading
        };
        let document = create_comparison_document(&crate::diff::Comparison::of(
                with_trouble(crate::diff::Source::Document {path: "D:/old.json".to_owned()}),
                reading_of(crate::diff::Source::Run, HashMap::new()), &config, Vec::new()), &Local::now(), &config);

        let read = serde_json::from_str::<serde_json::Value>(&document).unwrap();
        let (from, to) = (&read["from"]["scan"], &read["to"]["scan"]);
        assert_eq!(2, from["files_faulty"], "{document}");
        assert_eq!(1, from["dirs_unreadable"]);
        assert_eq!(9, from["files_found"]);
        assert_eq!(5, from["files_excluded"]);
        // the clean side says so rather than leaving the key out
        assert_eq!(0, to["files_faulty"]);
        assert_eq!(0, to["dirs_unreadable"]);

        // the same block a run document writes, so one shape is learned and not two
        let run = serde_json::from_str::<serde_json::Value>(&document_of(&config)).unwrap();
        let keys = |value: &serde_json::Value| {
            let mut names = value.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
            names.sort();
            names
        };
        assert_eq!(keys(&run["scan"]), keys(from));

        // and no list of paths on either side: a comparison records how each scan went and never
        // compares what went wrong in them
        assert!(!document.contains("faulty_files"), "{document}");
        assert!(!document.contains("unreadable_dirs"), "{document}");
    }

    #[test]
    fn a_comparison_says_what_makes_its_sides_two_measurements() {
        let config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let datetime = Local::now();
        let from = || reading_of(crate::diff::Source::Document { path: "D:/old.json".to_owned() }, HashMap::new());
        let mut to = reading_of(crate::diff::Source::Run, HashMap::new());
        to.version = "3.1.0".to_owned();
        to.scope.search_in_dotted = true;

        // After an adoption the two scopes read alike, so without this entry nothing tells a value
        // the command line gave apart from one it borrowed
        let adopted = crate::diff::Note::SettingsAdopted { from: "old.json".to_owned(), settings: vec!["exclude"] };
        let borrowed = create_comparison_document(&crate::diff::Comparison::of(from(),
                reading_of(crate::diff::Source::Run, HashMap::new()), &config, vec![adopted]), &datetime, &config);
        assert!(borrowed.contains("\"code\":\"setting-adopted\""), "{borrowed}");
        assert!(borrowed.contains("\"subject\":\"exclude\""));
        assert!(borrowed.contains("was taken from 'old.json'"), "{borrowed}");

        let document = create_comparison_document(&crate::diff::Comparison::of(from(), to, &config, Vec::new()), &datetime, &config);
        assert!(document.contains("\"code\":\"setting-differs\""), "{document}");
        assert!(document.contains("\"subject\":\"search-in-dotted\""));
        assert!(document.contains("\"code\":\"versions-differ\""));
        assert!(document.contains("\"subject\":\"3.0.0 -> 3.1.0\""));
        assert!(document.contains("\"path\":\"D:/old.json\""));

        let same = create_comparison_document(&crate::diff::Comparison::of(from(),
                reading_of(crate::diff::Source::Run, HashMap::new()), &config, Vec::new()), &datetime, &config);
        assert!(same.contains("\"warnings\":[]"), "{same}");
    }

    // A comparison document is read back as a baseline, and every setting its scope leaves out is
    // filled in by the reader with what an old build would have done. So a side that ran without
    // one of these comes back claiming it had it, and no later comparison can see the difference.
    #[test]
    fn each_side_of_a_comparison_carries_every_setting_a_run_document_carries() {
        let config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let datetime = Local::now();
        let document = create_comparison_document(&crate::diff::Comparison::of(
                reading_of(crate::diff::Source::Document { path: "D:/old.json".to_owned() }, HashMap::new()),
                reading_of(crate::diff::Source::Run, HashMap::new()), &config, Vec::new()), &datetime, &config);

        // Read off the run document rather than typed out here, or the direction that will actually
        // happen slips through: a setting added to one writer and forgotten on the other adds a name
        // a hand-written list never asks about.
        let run: serde_json::Value = serde_json::from_str(&document_of(&config)).unwrap();
        let settings = run["scope"].as_object().unwrap();
        assert!(settings.len() >= 11, "the run document's scope block was not found: {settings:?}");

        let compared: serde_json::Value = serde_json::from_str(&document).unwrap();
        for side in ["from", "to"] {
            let scope = compared[side]["scope"].as_object()
                    .unwrap_or_else(|| panic!("'{side}' carries no scope at all: {document}"));
            for setting in settings.keys() {
                assert!(scope.contains_key(setting),
                        "'{setting}' is missing from the '{side}' side of the comparison: {document}");
            }
        }
    }

    #[test]
    fn the_skipped_lists_are_written_only_when_asked_for_and_the_counts_either_way() {
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let mut result = result_of(HashMap::new(), Stats::default(),
                Vec::new(), FilesPresent {total_files: 3, relevant_files: 3, excluded_files: 0});
        result.skipped_files = mezura_core::SkippedFiles {
            minified: vec!["D:/x/bundle.js".to_owned()],
            generated: Vec::new(),
            not_code: vec!["D:/x/a.d".to_owned(), "D:/x/b.d".to_owned()]
        };

        let document = create_document(&result, &Local::now(), &config);
        assert!(document.contains("\"files_minified\":1"), "{document}");
        assert!(document.contains("\"files_not_code\":2"), "{document}");
        assert!(document.contains("\"skipped_files\":{\"minified\":[],\"generated\":[],\"not_code\":[]}"),
                "the lists were written without '--show-skipped': {document}");

        config.view.should_show_skipped_files = true;
        let detailed = create_document(&result, &Local::now(), &config);
        assert!(detailed.contains("\"not_code\":[\"D:/x/a.d\",\"D:/x/b.d\"]"), "{detailed}");
        assert!(detailed.contains("\"minified\":[\"D:/x/bundle.js\"]"), "{detailed}");

        let read_back = crate::json_reader::parse(&detailed).unwrap();
        assert_eq!(2, read_back.skipped_counts.not_code);
        assert_eq!(vec!["D:/x/a.d".to_owned(), "D:/x/b.d".to_owned()], read_back.result.skipped_files.not_code);
    }

    #[test]
    fn a_run_with_nothing_to_count_is_still_a_whole_document() {
        let config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let result = result_of(HashMap::new(), Stats::default(),
                Vec::new(), FilesPresent {total_files: 12, relevant_files: 0, excluded_files: 12});
        let document = create_document(&result, &Local::now(), &config);

        assert!(document.contains("\"languages\":[]"));
        assert!(document.contains("\"files\":0"));
        assert!(document.contains("\"files_found\":12"));
        assert!(document.contains("\"faulty_files\":[]"));
    }

    #[test]
    fn the_faulty_files_are_reported_with_their_reason_in_a_stable_order() {
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let result = result_of(
            hashmap!["Rust".to_owned() => stats_of(1, 30, 10, 5, 0, HashMap::new())],
            stats_of(1, 30, 10, 5, 0, HashMap::new()),
            vec![FaultyFileDetails::new("src\\z.rs".to_owned(), "no".to_owned(), 20),
                 FaultyFileDetails::new("src\\a.rs".to_owned(), "nope".to_owned(), 10)],
            FilesPresent {total_files: 3, relevant_files: 3, excluded_files: 0});

        // How many is always said; the paths are the detail, and are asked for
        let counted_only = create_document(&result, &Local::now(), &config);
        assert!(counted_only.contains("\"files_faulty\":2"));
        assert!(!counted_only.contains("a.rs"), "{counted_only}");

        config.view.set_should_show_faulty_files(true);
        let document = create_document(&result, &Local::now(), &config);
        assert!(document.contains("\"files_faulty\":2"));
        assert!(document.contains("\"path\":\"src\\\\a.rs\""));
        assert!(document.find("a.rs").unwrap() < document.find("z.rs").unwrap());
    }

    #[test]
    fn the_unreadable_directories_carry_their_reason_in_a_stable_order() {
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.view.set_should_show_faulty_files(true);
        let mut result = result_of(HashMap::new(), Stats::default(), Vec::new(),
                FilesPresent {total_files: 0, relevant_files: 0, excluded_files: 0});
        result.unreadable_dirs = vec![
            mezura_core::UnreadableDirDetails::new("D:/z".to_owned(),
                    "Access is denied. (os error 5)".to_owned()),
            mezura_core::UnreadableDirDetails::new("D:/a".to_owned(),
                    "The system cannot find the path specified. (os error 3)".to_owned())];
        let written = create_document(&result, &Local::now(), &config);

        assert!(written.contains("\"path\":\"D:/a\""), "{written}");
        assert!(written.contains("\"error\":\"Access is denied. (os error 5)\""), "{written}");
        assert!(written.contains("\"error\":\"The system cannot find the path specified. (os error 3)\""), "{written}");
        // sorted by path, since the walk collects these in whichever order its threads hit them
        assert!(written.find("D:/a").unwrap() < written.find("D:/z").unwrap(), "{written}");

        let clean = result_of(HashMap::new(), Stats::default(), Vec::new(),
                FilesPresent {total_files: 0, relevant_files: 0, excluded_files: 0});
        assert!(create_document(&clean, &Local::now(), &config).contains("\"unreadable_dirs\":[]"));
    }
}
