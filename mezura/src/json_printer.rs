use std::collections::HashMap;

use chrono::{DateTime, Local, SecondsFormat};
use mezura_core::{CountingModel, FaultyFileDetails, LineClasses, RunResult, Stats};

use super::config_manager::Configuration;
use super::result_printer;

// Bumped only when a key is removed or changes meaning. Adding one is not a bump, so a consumer can
// check this and not the version of the binary, which moves for reasons that do not concern it.
pub const FORMAT_VERSION : usize = 1;

// The file rows of one language, in the order the report shows them, beside the path each row prints
// No shortened path: a document carries the whole one, which is the only form a consumer can open
type FilesByLanguage<'a> = HashMap<&'a str, Vec<&'a mezura_core::FileEntry>>;

// The document is a designed shape and not a serialization of the structs the program happens to
// have. It carries every number that was measured, in its raw unit, and nothing the printer computed
// in order to look right: no sizes in KB, no separators, no percentages, no bar.
pub fn print_as_json(result: &RunResult, datetime_now: &DateTime<Local>, config: &Configuration) {
    println!("{}", create_document(result, datetime_now, config));
}

pub fn create_document(result: &RunResult, datetime_now: &DateTime<Local>, config: &Configuration) -> String {
    let RunResult {per_language, total, faulty_files, unreadable_dirs, nested_languages, ..} = result;
    let names = result_printer::get_sorted_language_names(per_language, config.view.sort_by, config.view.counting);
    let hidden = config.view.top_n.map_or(0, |top| names.len().saturating_sub(top));
    let shown = &names[..names.len() - hidden];
    let file_rows = result_printer::find_files_to_show(result, config);
    let files = merge_file_rows(&file_rows, config.view.sort_by, config.view.counting);
    let files_hidden = file_rows.iter().flat_map(HashMap::values).map(|rows| rows.hidden).sum::<usize>();

    let mut members = vec![
        format!("  \"format\": {FORMAT_VERSION}"),
        // What the document holds, so a consumer handed a file can tell a run from a comparison
        // without guessing from which keys exist
        String::from("  \"kind\": \"run\""),
        format!("  \"mezura_version\": \"{}\"", escape(config.view.version.trim_start_matches('v'))),
        format!("  \"generated_at\": \"{}\"", datetime_now.to_rfc3339_opts(SecondsFormat::Secs, false)),
        format!("  \"scope\": {}", create_scope_object(config,&result.targets)),
        format!("  \"scan\": {}", create_scan_object(4, &result.files_present,
                result.faulty_files.len(), result.unreadable_dirs.len())),
        format!("  \"total\": {}", create_total_object(total, !config.view.hidden.keywords, config.view.counting)),
        format!("  \"languages\": {}", create_languages_array(shown, per_language, nested_languages, &files, config)),
        format!("  \"languages_hidden\": {hidden}"),
        format!("  \"files_hidden\": {files_hidden}"),
        // The paths of the two, which '--show-faulty-files' asks for here as it does on the screen.
        // How many there were is in 'scan' either way, so their absence is never a claim that
        // nothing went wrong.
        format!("  \"faulty_files\": {}", create_faulty_files_array(faulty_files, config.view.should_show_faulty_files)),
        format!("  \"unreadable_dirs\": {}", create_unreadable_dirs_array(unreadable_dirs, config.view.should_show_faulty_files)),
        format!("  \"warnings\": {}", create_warnings_array()),
    ];
    // Absent from a run that named no module, the same way the section is absent from the printed
    // report: a consumer that never asked for a second axis is not handed one holding everything
    if result.has_modules() {
        members.push(format!("  \"modules\": {}", create_modules_array(result, &file_rows, config)));
    }
    // The only volatile block apart from the timestamp, so hiding the timing is also what makes the
    // document repeatable enough to hash or to compare against a stored one
    if !config.view.hidden.timing {
        members.push(format!("  \"performance\": {}", create_performance_object(&result.performance)));
    }

    format!("{{\n{}\n}}", members.join(",\n"))
}

pub fn print_comparison_as_json(comparison: &super::diff::Comparison,
        datetime_now: &DateTime<Local>, config: &Configuration) {
    println!("{}", create_comparison_document(comparison, datetime_now, config));
}

// The comparison as a document: the same vocabulary as a run's document, with every count a triad of
// 'from', 'to' and 'change'. The sides carry identity and nothing else, since their counts are the
// halves of the triads; '--top' is not applied, being a decision about a screen, so this document
// always holds every language of either reading.
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

    let mut members = vec![
        format!("  \"format\": {FORMAT_VERSION}"),
        String::from("  \"kind\": \"comparison\""),
        format!("  \"mezura_version\": \"{}\"", escape(config.view.version.trim_start_matches('v'))),
        format!("  \"generated_at\": \"{}\"", datetime_now.to_rfc3339_opts(SecondsFormat::Secs, false)),
        format!("  \"from\": {}", create_side_object(baseline)),
        format!("  \"to\": {}", create_side_object(subject)),
        format!("  \"total\": {}", create_compared_total_object(4, &baseline.result.total, &subject.result.total,
                keywords_counted, config.view.counting)),
        format!("  \"languages\": {}", create_compared_languages_array(4, &rows, keywords_counted,
                &nested_of(&baseline.result, config), &nested_of(&subject.result, config), config.view.counting)),
        format!("  \"warnings\": {}", create_comparison_warnings_array(&comparison.notes)),
    ];
    if let Some(pairs) = &pairs {
        members.push(format!("  \"modules\": {}", create_comparison_modules_array(pairs, config, keywords_counted)));
    }

    format!("{{\n{}\n}}", members.join(",\n"))
}

fn nested_of(result: &RunResult, config: &Configuration) -> HashMap<String, HashMap<String, Stats>> {
    match config.view.hidden.nested_languages {
        true => HashMap::new(),
        false => result.nested_languages.clone()
    }
}

fn nested_of_module(module: &mezura_core::ModuleResult, config: &Configuration)
        -> HashMap<String, HashMap<String, Stats>>
{
    match config.view.hidden.nested_languages {
        true => HashMap::new(),
        false => module.nested_languages.clone()
    }
}

// The same shape as a run document's modules, with every count a triad. '--top' does not cut these,
// the way it does not cut the languages above.
fn create_comparison_modules_array(pairs: &[super::diff::ModulePair], config: &Configuration,
        keywords_counted: bool) -> String
{
    let entries = pairs.iter().map(|pair| {
        let (rows, _) = super::diff::create_comparison_rows(&pair.before.per_language, &pair.now.per_language,
                config.view.sort_by, None, config.view.counting);
        let name = pair.name.map_or("null".to_owned(), |x| format!("\"{}\"", escape(x)));
        let members = [
            format!("      \"name\": {name}"),
            format!("      \"total\": {}", create_compared_total_object(8, &pair.before.total, &pair.now.total,
                    keywords_counted, config.view.counting)),
            format!("      \"languages\": {}", create_compared_languages_array(8, &rows, keywords_counted,
                    &nested_of_module(pair.before, config), &nested_of_module(pair.now, config),
                    config.view.counting)),
        ];
        format!("    {{\n{}\n    }}", members.join(",\n"))
    }).collect::<Vec<_>>();

    format!("[\n{}\n  ]", entries.join(",\n"))
}

// 'brace' is the column each entry's opening brace sits at, so that the same array can be written at
// the top level and under a module, which are at two depths.
fn create_compared_languages_array(brace_indent: usize, changes: &[super::diff::LanguageStatsChange],
        keywords_counted: bool, baseline_nested: &HashMap<String, HashMap<String, Stats>>,
        subject_nested: &HashMap<String, HashMap<String, Stats>>, model: CountingModel) -> String
{
    if changes.is_empty() {
        return String::from("[]");
    }

    let entries = changes.iter().map(|change| create_compared_language_object(brace_indent, &change.name,
            &change.baseline, &change.subject, keywords_counted,
            baseline_nested.get(&change.name), subject_nested.get(&change.name), model)).collect::<Vec<_>>();

    format!("[\n{}\n{}]", entries.join(",\n"), " ".repeat(brace_indent - 2))
}

fn create_compared_language_object(brace_indent: usize, name: &str, baseline: &Stats, subject: &Stats,
        keywords_counted: bool, baseline_nested: Option<&HashMap<String, Stats>>,
        subject_nested: Option<&HashMap<String, Stats>>, model: CountingModel) -> String
{
    let pad = " ".repeat(brace_indent + 2);
    let mut members = vec![format!("{pad}\"name\": \"{}\"", escape(name))];
    members.extend(create_triad_members(&pad, baseline, subject, model));
    if keywords_counted {
        members.push(format!("{pad}\"keywords\": {}", create_keyword_triads(&baseline.keyword_occurences,
                &subject.keyword_occurences, brace_indent + 4)));
    }
    if baseline_nested.is_some() || subject_nested.is_some() {
        members.push(format!("{pad}\"nested_languages\": {}", create_compared_nested_array(brace_indent + 2,
                baseline_nested, subject_nested, keywords_counted, model)));
    }

    format!("{}{{\n{}\n{}}}", " ".repeat(brace_indent), members.join(",\n"), " ".repeat(brace_indent))
}

// A section only one reading holds is written with the other side at zero, so a '<style>' block
// that was added or taken out is a triad and not an absence
fn create_compared_nested_array(brace_indent: usize, baseline: Option<&HashMap<String, Stats>>,
        subject: Option<&HashMap<String, Stats>>, keywords_counted: bool, model: CountingModel) -> String
{
    let mut names = baseline.into_iter().chain(subject).flat_map(HashMap::keys).cloned().collect::<Vec<_>>();
    names.sort_unstable();
    names.dedup();
    if names.is_empty() {
        return String::from("[]");
    }

    let of = |side: Option<&HashMap<String, Stats>>, name: &str|
            side.and_then(|x| x.get(name)).cloned().unwrap_or_default();
    let entries = names.iter().map(|name| create_compared_language_object(brace_indent + 2, name,
            &of(baseline, name), &of(subject, name), keywords_counted, None, None, model)).collect::<Vec<_>>();

    format!("[\n{}\n{}]", entries.join(",\n"), " ".repeat(brace_indent))
}

// 'member_indent' is the column its members sit at, and its closing brace goes two to the left
fn create_compared_total_object(member_indent: usize, baseline: &Stats, subject: &Stats,
        keywords_counted: bool, model: CountingModel) -> String {
    let pad = " ".repeat(member_indent);
    let mut lines = create_triad_members(&pad, baseline, subject, model);
    if keywords_counted {
        lines.push(format!("{pad}\"keywords\": {}", create_keyword_triads(&baseline.keyword_occurences,
                &subject.keyword_occurences, member_indent + 2)));
    }

    format!("{{\n{}\n{}}}", lines.join(",\n"), " ".repeat(member_indent - 2))
}

// Identity, and the identity of each source has its own shape: 'source' is the discriminator, so a
// consumer never guesses from which keys exist. The counts are not here, being the triads' halves.
fn create_side_object(reading: &super::diff::Reading) -> String {
    let mut members = vec![match &reading.source {
        super::diff::Source::Run => String::from("    \"source\": \"run\""),
        super::diff::Source::Document { path } =>
            format!("    \"source\": \"document\",\n    \"path\": \"{}\"", escape(path)),
        // Both halves, because neither derives from the other: the hash is what was measured, and
        // what was asked for is what a person recognises later
        super::diff::Source::GitRevision { commit, asked_for } =>
            format!("    \"source\": \"revision\",\n    \"commit\": \"{}\",\n    \"asked_for\": \"{}\"",
                    escape(commit), escape(asked_for))
    }];
    members.push(format!("    \"taken_at\": \"{}\"", escape(&reading.taken)));
    members.push(format!("    \"mezura_version\": \"{}\"", escape(reading.version.trim_start_matches('v'))));
    // How the scan of this side went, and only how: without it a side that failed to read half its
    // files looks like a side that shrank, and the triads above cannot say which. Which files those
    // were is a question for a run over that side, not for a comparison of two.
    members.push(format!("    \"scan\": {}", create_scan_object(6, &reading.result.files_present,
            reading.faulty_files_count, reading.unreadable_dirs_count)));
    members.push(format!("    \"scope\": {}", indent(&create_scope_object_of(&reading.scope, &reading.result.targets))));
    // Only what the side's own document recorded: what this very process warned about belongs to
    // the comparison and sits at its top level, wherever the run appears in it
    members.push(format!("    \"warnings\": {}", indent(&create_document_warnings_array(&reading.warnings))));

    format!("{{\n{}\n  }}", members.join(",\n"))
}

// The scope in the shape the run document writes it, from wherever the reading carried it. The
// targets are in it for the same reason they are in the run's: two sides that measured
// different trees would otherwise read as code that changed.
fn create_scope_object_of(scope: &super::json_reader::Scope, targets: &[mezura_core::Target]) -> String {
    let members = [
        format!("    \"targets\": {}", create_targets_array(targets)),
        format!("    \"exclude\": {}", create_string_array(&scope.exclude)),
        format!("    \"languages\": {}", create_string_array(&scope.languages)),
        format!("    \"excluded_languages\": {}", create_string_array(&scope.excluded_languages)),
        format!("    \"forced_languages\": {}", create_forced_languages_object(&scope.forced_languages)),
        format!("    \"counting\": \"{}\"", escape(&scope.counting)),
        format!("    \"search_in_dotted\": {}", scope.search_in_dotted),
        format!("    \"gitignore\": {}", scope.gitignore),
        format!("    \"keywords_counted\": {}", scope.keywords_counted),
    ];

    format!("{{\n{}\n  }}", members.join(",\n"))
}

fn create_document_warnings_array(warnings: &[super::json_reader::DocumentWarning]) -> String {
    if warnings.is_empty() {
        return String::from("[]");
    }

    let entries = warnings.iter().map(|warning| {
        let members = [
            format!("      \"code\": \"{}\"", escape(&warning.code)),
            format!("      \"affects\": \"{}\"", escape(&warning.affects)),
            format!("      \"message\": \"{}\"", escape(&warning.message)),
        ];
        format!("    {{\n{}\n    }}", members.join(",\n"))
    }).collect::<Vec<_>>();

    format!("[\n{}\n  ]", entries.join(",\n"))
}

struct WarningEntry {
    code: String,
    affects: &'static str,
    subject: String,
    message: String
}

// What makes the two readings two measurements rather than two moments of one: the same facts the
// screen says above the table, as entries a program can key on.
fn create_comparison_warnings_array(notes: &[super::diff::Note]) -> String {
    // What this very process warned about comes first, having been said first: it belongs to the
    // comparison and not to either side, since a side that is a revision was counted by this
    // process too and a document's own warnings are already inside it
    let mut entries = super::warning_collector::get_collected_warnings().into_iter()
            .map(|x| WarningEntry {
                code: x.code.name().to_owned(), affects: x.affects().name(),
                subject: x.subject.clone(), message: x.message.clone()
            })
            .collect::<Vec<_>>();
    for note in notes {
        entries.extend(create_note_entries(note));
    }
    if entries.is_empty() {
        return String::from("[]");
    }

    let rendered = entries.into_iter().map(|entry| {
        let members = [
            format!("      \"code\": \"{}\"", entry.code),
            format!("      \"affects\": \"{}\"", entry.affects),
            format!("      \"subject\": \"{}\"", escape(&entry.subject)),
            format!("      \"message\": \"{}\"", escape(&entry.message)),
        ];
        format!("    {{\n{}\n    }}", members.join(",\n"))
    }).collect::<Vec<_>>();

    format!("[\n{}\n  ]", rendered.join(",\n"))
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
        // to agree. Without it both scopes simply read alike and nothing tells a value the command
        // line gave apart from one it borrowed.
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
        Note::CountsInDoubt { .. } | Note::NothingCounted { .. } | Note::LayoutFallback { .. }
        | Note::NoGitignoreInCheckout { .. } | Note::MissingInRevision { .. } => Vec::new()
    }
}

// The five figures a comparison compares, each as '{"from": a, "to": b, "change": b - a}'. 'change'
// is derived and written anyway, because the comparison is the product: handing back two numbers
// and leaving the subtraction to the reader is handing back the input.
fn create_triad_members(indent: &str, before: &Stats, now: &Stats, model: CountingModel) -> Vec<String> {
    [("files", before.files, now.files), ("lines", before.lines, now.lines),
     ("code", before.calculate_code_lines(model), now.calculate_code_lines(model)),
     ("comments", before.calculate_comment_lines(model), now.calculate_comment_lines(model)),
     ("bytes", before.bytes, now.bytes)]
            .into_iter().map(|(name, from, to)| format!("{indent}\"{name}\": {}", create_triad(from, to)))
            .collect()
}

fn create_triad(from: usize, to: usize) -> String {
    format!("{{\"from\": {from}, \"to\": {to}, \"change\": {}}}", to as i128 - from as i128)
}

// The union of both sides' keywords, without the ones that are zero on both: a slot every selected
// language declares and nothing ever used is a row about nothing.
fn create_keyword_triads(before: &HashMap<String, usize>, now: &HashMap<String, usize>, indent: usize) -> String {
    let mut names = before.keys().chain(now.keys()).cloned().collect::<Vec<_>>();
    names.sort_unstable();
    names.dedup();
    names.retain(|name| before.get(name).copied().unwrap_or(0) > 0 || now.get(name).copied().unwrap_or(0) > 0);
    if names.is_empty() {
        return String::from("{}");
    }

    let members = names.into_iter().map(|name| {
        let (from, to) = (before.get(&name).copied().unwrap_or(0), now.get(&name).copied().unwrap_or(0));
        format!("{}\"{}\": {}", " ".repeat(indent), escape(&name), create_triad(from, to))
    }).collect::<Vec<_>>();

    format!("{{\n{}\n{}}}", members.join(",\n"), " ".repeat(indent - 2))
}

// Only what can change a number: no theme, no layout, no separators. Without it, two documents that
// differ by an '--exclude' look like a code change.
fn create_scope_object(config: &Configuration, targets: &[mezura_core::Target]) -> String {
    let members = [
        // The resolved list off the result, not the declared one off the configuration: the same
        // './src' over two different trees is two different measurements
        format!("    \"targets\": {}", create_targets_array(targets)),
        format!("    \"exclude\": {}", create_string_array(&config.engine.exclude_dirs)),
        format!("    \"languages\": {}", create_string_array(&config.engine.languages_of_interest)),
        format!("    \"excluded_languages\": {}", create_string_array(&config.engine.excluded_languages)),
        // '--force-language m=matlab' decides which language a file is counted as, so it moves numbers
        // the same way an exclusion does, and two runs that disagree about it are not comparable
        format!("    \"forced_languages\": {}", create_forced_languages_object(&config.engine.forced_languages)),
        format!("    \"counting\": \"{}\"", config.view.counting.name()),
        format!("    \"search_in_dotted\": {}", config.engine.should_search_in_dotted),
        format!("    \"gitignore\": {}", !config.engine.no_gitignore),
        format!("    \"keywords_counted\": {}", !config.view.hidden.keywords),
    ];

    format!("{{\n{}\n  }}", members.join(",\n"))
}

// 'files_of_interest' is what the status line calls it, and it is not the same as the file count of
// the total below: the faulty ones were found and are of interest, but nothing of them was counted.
// Everything the scan itself has to say, and the one block that answers whether the counts beside
// it are complete. Written per side of a comparison too, which is why 'indent' exists: the same
// shape at two depths, so a consumer learns it once.
//
// The names inside are '<what>_<state>', where the lists that carry the paths are natural English:
// 'files_faulty' is the count, 'faulty_files' is the array.
// The counts arrive apart from the lists in the result, because for a side read from a document the
// lists are only there when that run detailed them, while its scan block counted either way
fn create_scan_object(indent: usize, files_present: &mezura_core::FilesPresent,
        faulty_files_count: usize, unreadable_dirs_count: usize) -> String
{
    let pad = " ".repeat(indent);
    let members = [
        format!("{pad}\"files_found\": {}", files_present.total_files),
        format!("{pad}\"files_of_interest\": {}", files_present.relevant_files),
        format!("{pad}\"files_excluded\": {}", files_present.excluded_files),
        format!("{pad}\"files_faulty\": {faulty_files_count}"),
        format!("{pad}\"dirs_unreadable\": {unreadable_dirs_count}"),
    ];

    format!("{{\n{}\n{}}}", members.join(",\n"), " ".repeat(indent - 2))
}

fn create_total_object(total: &Stats, keywords_counted: bool, model: CountingModel) -> String {
    let mut members = vec![
        format!("    \"files\": {}", total.files),
        format!("    \"lines\": {}", total.lines),
        format!("    \"code\": {}", total.calculate_code_lines(model)),
        format!("    \"comments\": {}", total.calculate_comment_lines(model)),
        format!("    \"extra\": {}", total.calculate_extra_lines(model)),
        format!("    \"bytes\": {}", total.bytes),
        format!("    \"average_bytes\": {}", total.calculate_average_size()),
        format!("    \"classes\": {}", create_classes_object(&total.classes, 6)),
    ];
    // Every language of the run added up, which is the only place the figure survives '--top': the
    // languages are cut there and the ones left cannot be added back up to it.
    if keywords_counted {
        members.push(format!("    \"keywords\": {}", create_keywords_object(&total.keyword_occurences, 6)));
    }

    format!("{{\n{}\n  }}", members.join(",\n"))
}

// The leftovers of the named modules carry 'null' and not the '(unnamed)' the report prints: a marker
// spelled as a name is one a real module could be called, and a machine consumer grouping by that
// key would silently merge the two.
fn create_modules_array(result: &RunResult, file_rows: &[result_printer::FileRowsOfModule],
        config: &Configuration) -> String
{
    let entries = result.modules.iter().zip(file_rows).map(|(module, files)| {
        let names = result_printer::get_sorted_language_names(&module.per_language, config.view.sort_by,
                config.view.counting);
        let hidden = config.view.top_n.map_or(0, |top| names.len().saturating_sub(top));
        let shown = &names[..names.len() - hidden];
        let files_hidden = files.values().map(|rows| rows.hidden).sum::<usize>();
        let files = find_shown_files(files);
        let name = module.name.as_ref().map_or("null".to_owned(), |x| format!("\"{}\"", escape(x)));
        let members = [
            format!("      \"name\": {name}"),
            format!("      \"total\": {}", indent(&create_total_object(&module.total, !config.view.hidden.keywords,
                    config.view.counting))),
            format!("      \"languages\": {}", indent(&create_languages_array(shown, &module.per_language,
                    &module.nested_languages, &files, config))),
            format!("      \"languages_hidden\": {hidden}"),
            format!("      \"files_hidden\": {files_hidden}"),
        ];
        format!("    {{\n{}\n    }}", members.join(",\n"))
    }).collect::<Vec<_>>();

    format!("[\n{}\n  ]", entries.join(",\n"))
}

fn find_shown_files<'a>(of_module: &'a result_printer::FileRowsOfModule<'a>) -> FilesByLanguage<'a> {
    of_module.iter().map(|(language, rows)|
            (*language, rows.shown.iter().map(|(_, file)| *file).collect())).collect()
}

// The document's own 'languages' array is the run and not one part of it
fn merge_file_rows<'a>(per_module: &'a [result_printer::FileRowsOfModule<'a>],
        sort_by: mezura_core::SortCriterion, model: CountingModel) -> FilesByLanguage<'a>
{
    let mut merged: FilesByLanguage<'a> = HashMap::new();
    for of_module in per_module {
        for (language, rows) in of_module {
            merged.entry(language).or_default().extend(rows.shown.iter().map(|(_, file)| *file));
        }
    }
    if per_module.len() > 1 {
        for files in merged.values_mut() {
            files.sort_by(|one, other| result_printer::compare_files_by(one, other, sort_by, model));
        }
    }

    merged
}

// The two blocks are shared with the top level, where they sit one level higher, so their closing
// braces and their members are pushed in rather than written twice
fn indent(block: &str) -> String {
    block.replace('\n', "\n    ")
}

// An array and not an object keyed by language name, so that the order '--sort' chose survives and
// so that no language can collide with a key of the document.
fn create_languages_array(shown: &[String], per_language: &HashMap<String, Stats>,
        nested_languages: &HashMap<String, HashMap<String, Stats>>,
        files: &FilesByLanguage, config: &Configuration) -> String
{
    if shown.is_empty() {
        return String::from("[]");
    }

    let entries = shown.iter().filter_map(|name| {
        Some(create_language_object(name, per_language.get(name)?, !config.view.hidden.keywords,
                !config.view.hidden.nested_languages, nested_languages.get(name),
                files.get(name.as_str()).map(Vec::as_slice).unwrap_or_default(), config.view.counting))
    }).collect::<Vec<_>>();

    format!("[\n{}\n  ]", entries.join(",\n"))
}

fn create_files_array(files: &[&mezura_core::FileEntry], nested_shown: bool, model: CountingModel) -> String {
    let entries = files.iter().map(|file| {
        let stats = &file.stats;
        let mut members = vec![
            format!("          \"path\": \"{}\"", escape(&file.path)),
            format!("          \"lines\": {}", stats.lines),
            format!("          \"code\": {}", stats.calculate_code_lines(model)),
            format!("          \"comments\": {}", stats.calculate_comment_lines(model)),
            format!("          \"extra\": {}", stats.calculate_extra_lines(model)),
            format!("          \"bytes\": {}", stats.bytes),
            format!("          \"classes\": {}", create_classes_object(&stats.classes, 12)),
        ];
        if nested_shown && !file.nested_languages.is_empty() {
            members.push(format!("          \"nested_languages\": {}",
                    create_nested_languages_array(&file.nested_languages, model).replace('\n', "\n    ")));
        }
        format!("        {{\n{}\n        }}", members.join(",\n"))
    }).collect::<Vec<_>>();

    format!("[\n{}\n      ]", entries.join(",\n"))
}

// Absent for a language that holds nothing, so an older document and this one read alike
fn create_nested_languages_array(sections: &HashMap<String, Stats>, model: CountingModel) -> String {
    let mut sorted = sections.iter().collect::<Vec<_>>();
    sorted.sort_unstable_by_key(|(name, _)| name.as_str());

    let entries = sorted.into_iter().map(|(name, info)| format!(
"        {{\n          \"name\": \"{}\",\n          \"files\": {},\n          \"lines\": {},\n          \
\"code\": {},\n          \"comments\": {},\n          \"extra\": {},\n          \"bytes\": {},\n          \
\"classes\": {}\n        }}",
            escape(name), info.files, info.lines, info.calculate_code_lines(model),
            info.calculate_comment_lines(model), info.calculate_extra_lines(model), info.bytes,
            create_classes_object(&info.classes, 12))).collect::<Vec<_>>();

    format!("[\n{}\n      ]", entries.join(",\n"))
}

// 'nested_shown' reaches the language's own breakdown and the one inside each of its files alike
fn create_language_object(name: &str, info: &Stats, keywords_counted: bool, nested_shown: bool,
        sections: Option<&HashMap<String, Stats>>, files: &[&mezura_core::FileEntry],
        model: CountingModel) -> String
{
    let mut members = vec![
        format!("      \"name\": \"{}\"", escape(name)),
        format!("      \"files\": {}", info.files),
        format!("      \"lines\": {}", info.lines),
        format!("      \"code\": {}", info.calculate_code_lines(model)),
        format!("      \"comments\": {}", info.calculate_comment_lines(model)),
        format!("      \"extra\": {}", info.calculate_extra_lines(model)),
        format!("      \"bytes\": {}", info.bytes),
        format!("      \"average_bytes\": {}", info.calculate_average_size()),
        format!("      \"classes\": {}", create_classes_object(&info.classes, 8)),
    ];
    // Absent when they were not counted, since '--hide keywords' also stops the counting. An empty
    // object means the opposite: they were counted and the language declares none.
    if keywords_counted {
        members.push(format!("      \"keywords\": {}", create_keywords_object(&info.keyword_occurences, 8)));
    }
    if let Some(sections) = sections.filter(|x| nested_shown && !x.is_empty()) {
        members.push(format!("      \"nested_languages\": {}", create_nested_languages_array(sections, model)));
    }
    // Named after the command and not 'files', which this object already uses for how many there are
    if !files.is_empty() {
        members.push(format!("      \"by_file\": {}", create_files_array(files, nested_shown, model)));
    }

    format!("    {{\n{}\n    }}", members.join(",\n"))
}

// The raw counts behind the folded columns, one member per class, so a consumer can fold them
// under either model whatever the scope's own was
fn create_classes_object(classes: &LineClasses, indent: usize) -> String {
    let pad = " ".repeat(indent);
    let members = [
        format!("{pad}\"words_in_code\": {}", classes.words_in_code),
        format!("{pad}\"string_content\": {}", classes.string_content),
        format!("{pad}\"comment_words_beside_code\": {}", classes.comment_words_beside_code),
        format!("{pad}\"words_in_comment\": {}", classes.words_in_comment),
        format!("{pad}\"punctuation_in_code\": {}", classes.punctuation_in_code),
        format!("{pad}\"punctuation_in_comment\": {}", classes.punctuation_in_comment),
        format!("{pad}\"blank\": {}", classes.blank),
        format!("{pad}\"blank_in_comment\": {}", classes.blank_in_comment),
        format!("{pad}\"blank_in_string\": {}", classes.blank_in_string),
    ];

    format!("{{\n{}\n{}}}", members.join(",\n"), " ".repeat(indent - 2))
}

// 'indent' is the column its members sit at, and its closing brace goes two to the left of them, so
// that the same object can be written under a language and under a total, which are at two depths.
fn create_keywords_object(occurences: &HashMap<String, usize>, indent: usize) -> String {
    if occurences.is_empty() {
        return String::from("{}");
    }

    let mut sorted = occurences.iter().collect::<Vec<_>>();
    sorted.sort_unstable_by_key(|(name, _)| name.as_str());
    let members = sorted.into_iter()
            .map(|(name, count)| format!("{}\"{}\": {count}", " ".repeat(indent), escape(name)))
            .collect::<Vec<_>>();

    format!("{{\n{}\n{}}}", members.join(",\n"), " ".repeat(indent - 2))
}

// Everything the run said on the error output, which a machine consumer never sees. Always present,
// empty array included, so that a consumer can read it without asking whether the key is there.
//
// 'code' is the half that is safe to branch on and 'message' the half that is safe to show, and
// 'affects' is what lets a consumer written today keep working when a later version adds a code it
// has never heard of: the question is whether the counts can be trusted, not which of the codes are
// the serious ones. In emission order, which is the order they were printed in.
fn create_warnings_array() -> String {
    let warnings = super::warning_collector::get_collected_warnings();
    if warnings.is_empty() {
        return String::from("[]");
    }

    let entries = warnings.iter().map(|warning| {
        let members = [
            format!("      \"code\": \"{}\"", escape(warning.code.name())),
            format!("      \"affects\": \"{}\"", warning.affects().name()),
            format!("      \"subject\": \"{}\"", escape(&warning.subject)),
            format!("      \"message\": \"{}\"", escape(&warning.message)),
        ];
        format!("    {{\n{}\n    }}", members.join(",\n"))
    }).collect::<Vec<_>>();

    format!("[\n{}\n  ]", entries.join(",\n"))
}

// Sorted by path, because the faulty files are collected by whichever thread hit them and their
// order would otherwise change between two runs over the same tree
fn create_faulty_files_array(faulty_files: &[FaultyFileDetails], asked_for: bool) -> String {
    if !asked_for || faulty_files.is_empty() {
        return String::from("[]");
    }

    let mut sorted = faulty_files.iter().collect::<Vec<_>>();
    sorted.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    let entries = sorted.into_iter().map(|file| {
        let members = [
            format!("      \"path\": \"{}\"", escape(&file.path)),
            format!("      \"bytes\": {}", file.size),
            format!("      \"error\": \"{}\"", escape(&file.error_msg)),
        ];
        format!("    {{\n{}\n    }}", members.join(",\n"))
    }).collect::<Vec<_>>();

    format!("[\n{}\n  ]", entries.join(",\n"))
}

// Objects and not bare paths, and sorted for the same reason as the faulty files above. A consumer
// that wants only the paths reads one key of each; one that wants to tell a permission apart from a
// directory that went away mid-walk could not do it at all while this was an array of strings.
fn create_unreadable_dirs_array(unreadable_dirs: &[mezura_core::UnreadableDirDetails], asked_for: bool) -> String {
    if !asked_for || unreadable_dirs.is_empty() {
        return String::from("[]");
    }

    let mut sorted = unreadable_dirs.iter().collect::<Vec<_>>();
    sorted.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    let entries = sorted.into_iter().map(|dir| {
        let members = [
            format!("      \"path\": \"{}\"", escape(&dir.path)),
            format!("      \"error\": \"{}\"", escape(&dir.error_msg)),
        ];
        format!("    {{\n{}\n    }}", members.join(",\n"))
    }).collect::<Vec<_>>();

    format!("[\n{}\n  ]", entries.join(",\n"))
}

// 'scan_ms' and not the 'Exec time' of the footer: what is measured here is the interval that starts
// before the producers and ends when the consumers are done, which is the phase 'scan' describes.
// The total is not known yet at this point, and the shell can measure it honestly anyway.
// The thread counts come from the result and not from the configuration, because they sit beside
// the measurement they exist to interpret: the configuration holds what was asked for, and the
// operating system is allowed to grant fewer. A document stating the requested counts next to
// 'scan_ms' would be lying about the conditions of its own timing.
fn create_performance_object(performance: &mezura_core::Performance) -> String {
    let threads = format!("{{\n      \"producers\": {},\n      \"consumers\": {}\n    }}",
            performance.threads.producers(), performance.threads.consumers());

    format!("{{\n    \"scan_ms\": {},\n    \"threads\": {threads}\n  }}", performance.duration_millis)
}

// One entry per target and not one per module: a module given several paths is several targets that
// share a name, and grouping them would lose the order they were declared in, which the columns of
// the report follow. The unnamed ones carry 'null' for the same reason the modules do.
fn create_targets_array(targets: &[mezura_core::Target]) -> String {
    if targets.is_empty() {
        return String::from("[]");
    }

    let entries = targets.iter().map(|target| {
        let module = target.module.as_ref().map_or("null".to_owned(), |x| format!("\"{}\"", escape(x)));
        format!("      {{\"module\": {module}, \"path\": \"{}\"}}", escape(&target.path))
    }).collect::<Vec<_>>();

    format!("[\n{}\n    ]", entries.join(",\n"))
}

// The extension is the key, since that is what a run is asked about and what can only be claimed
// once. Sorted, so that two runs over the same tree produce the same bytes.
fn create_forced_languages_object(forced: &HashMap<String, String>) -> String {
    if forced.is_empty() {
        return String::from("{}");
    }

    let mut sorted = forced.iter().collect::<Vec<_>>();
    sorted.sort_unstable_by_key(|(extension, _)| extension.as_str());
    let members = sorted.into_iter()
            .map(|(extension, language)| format!("      \"{}\": \"{}\"", escape(extension), escape(language)))
            .collect::<Vec<_>>();

    format!("{{\n{}\n    }}", members.join(",\n"))
}

fn create_string_array(values: &[String]) -> String {
    if values.is_empty() {
        return String::from("[]");
    }

    format!("[{}]", values.iter().map(|x| format!("\"{}\"", escape(x))).collect::<Vec<_>>().join(", "))
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
                files_present, targets: Vec::new(), unreadable_dirs: Vec::new(),
                performance: mezura_core::Performance { duration_millis: 1180, threads: mezura_core::Threads::new(2, 8) }}
    }

    fn document_of(config: &crate::config_manager::Configuration) -> String {
        // Summed rather than written out, so that the document is one a run could have produced and
        // the keywords of the total are the keywords of the languages under it
        let per_language = hashmap![
            "Rust".to_owned() => stats_of(2, 5000, 100, 70, 10, hashmap!["structs".to_owned() => 3, "enums".to_owned() => 1]),
            "HTML".to_owned() => stats_of(1, 900, 40, 30, 0, HashMap::new())];
        let result = result_of(per_language.clone(), Stats::total_of(&per_language), Vec::new(),
            FilesPresent {total_files: 5, relevant_files: 3, excluded_files: 2});
        let datetime = DateTime::parse_from_rfc3339("2026-07-30T14:22:07+03:00").unwrap().with_timezone(&Local);

        create_document(&result, &datetime, config)
    }

    #[test]
    fn every_string_that_json_cannot_carry_raw_is_escape() {
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

        assert!(document.contains(&format!("\"format\": {FORMAT_VERSION}")));
        assert!(document.contains("\"mezura_version\": \"3.0.0\""));
        assert!(document.contains("\"generated_at\": \"2026-07-30T14:22:07+03:00\""));
        assert!(document.contains("\"lines\": 140"));
        assert!(document.contains("\"average_bytes\": 2500"));
        assert!(document.contains("\"scan_ms\": 1180"));
        // Nothing that the printed output adds: no separators in the four digit numbers, no size
        // measurement, no percentage, and no layout or theme in the echo of the settings
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
        assert!(document.contains("\"languages_hidden\": 1"));
        assert!(document.contains("\"lines\": 140"));
    }

    #[test]
    fn hiding_the_keywords_removes_the_key_while_a_language_without_any_gets_an_empty_one() {
        let config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let document = document_of(&config);
        assert!(document.contains("\"keywords\": {}"));
        assert!(document.contains("\"structs\": 3"));
        assert!(document.contains("\"keywords_counted\": true"));
        // Sorted by name, so that two runs over the same tree produce the same bytes
        assert!(document.find("\"enums\"").unwrap() < document.find("\"structs\"").unwrap());

        // The total carries them too, which is the only figure that survives a '--top' that cuts
        // the languages they would otherwise have to be added back up from
        let at = document.find("\"total\"").unwrap();
        let total_block = &document[at..at + document[at..].find("\"languages\"").unwrap()];
        assert!(total_block.contains("\"keywords\": {"), "{total_block}");
        assert!(total_block.contains("\"structs\": 3") && total_block.contains("\"enums\": 1"), "{total_block}");

        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.view.hidden.keywords = true;
        let document = document_of(&config);
        assert!(!document.contains("\"keywords\""));
        assert!(document.contains("\"keywords_counted\": false"));
    }

    #[test]
    fn hiding_the_timing_removes_the_only_block_that_changes_between_two_identical_runs() {
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.view.hidden.timing = true;
        let document = document_of(&config);

        assert!(!document.contains("\"performance\""));
        assert!(!document.contains("\"scan_ms\""));
    }

    // The key is absent from a run that named nothing, and the leftovers carry 'null': a marker
    // spelled '(unnamed)' is a name a real module could be given, and a consumer grouping by that key
    // would merge the two without noticing
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
        assert!(rendered.contains("\"name\": \"backend\""));
        assert!(rendered.contains("\"name\": null"));
        // Each module carries the same 'total' and 'languages' blocks the document carries for the
        // whole run, so a consumer reads one shape and not two, and the two of them add up to it
        let block = &rendered[rendered.find("\"modules\"").unwrap()..];
        assert_eq!(2, block.matches("\"total\":").count());
        assert_eq!(2, block.matches("\"languages\":").count());
        assert!(block.contains("\"lines\": 100") && block.contains("\"lines\": 40"));
        assert!(rendered.contains("\"lines\": 140"));
        assert!(rendered.contains("\"languages_hidden\": 0"));

        // '--top' is per module there too, so a module with one language is not cut by '--top 1'
        // while the report as a whole has two
        config.view.top_n = Some(1);
        let cut = create_document(&result, &Local::now(), &config);
        assert_eq!(2, cut.matches("\"languages_hidden\": 0").count());
        assert!(cut.contains("\"languages_hidden\": 1"));
    }

    // Everything a run says on the error output is invisible to whoever asked for the document, and
    // some of it means the counts cannot be trusted. The collector is shared by the whole process,
    // so this asserts on its own entry rather than on the whole array.
    #[test]
    fn a_warning_reaches_the_document_with_both_of_its_halves() {
        let config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        // The key is always written, so a consumer never has to test for it. Whether the array is
        // empty cannot be asserted: the collector belongs to the process and every other test of
        // this binary adds to it.
        assert!(document_of(&config).contains("\"warnings\": ["));

        super::super::warning_collector::keep(mezura_core::warnings::Warning::new(
                mezura_core::warnings::Code::LanguageTiebreak, "a-subject-only-this-test-uses",
                "quoted \"text\" and a \\ backslash".to_owned()));

        let rendered = create_warnings_array();
        assert!(rendered.contains("\"subject\": \"a-subject-only-this-test-uses\""));
        assert!(rendered.contains("\"code\": \"language-tiebreak\""));
        assert!(rendered.contains("\"affects\": \"counts\""));
        // The message is written for a person, so it goes through the same escaping as every other
        // string here or a quotation mark in it would break the document
        assert!(rendered.contains("quoted \\\"text\\\" and a \\\\ backslash"));
    }

    // One entry per target, so a module given several paths is several entries carrying one name,
    // and a consumer never has to split a string to find out where a run looked.
    // It decides which language a file is counted as, so it moves numbers exactly the way an
    // exclusion does. The log has recorded it among the settings from the start and the document did
    // not, so two runs that disagreed about it compared silently.
    #[test]
    fn the_extensions_that_were_forced_to_a_language_are_among_the_settings() {
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        assert!(document_of(&config).contains("\"forced_languages\": {}"));

        config.engine.forced_languages = hashmap!["m".to_owned() => "matlab".to_owned(),
                "h".to_owned() => "objective-c".to_owned()];
        let document = document_of(&config);
        assert!(document.contains("\"m\": \"matlab\""), "{document}");
        // by extension, which is the thing a run is asked about and can only be claimed once, and
        // sorted so that two runs over the same tree produce the same bytes
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

        assert!(written.contains("{\"module\": \"tests\", \"path\": \"D:/api/tests\"}"), "{written}");
        assert_eq!(2, written.matches("\"module\": \"tests\"").count());
        // the unnamed one carries 'null' rather than a name a real module could be given, and a
        // Windows path is escaped here as it is everywhere else
        assert!(written.contains("{\"module\": null, \"path\": \"D:\\\\web\"}"), "{written}");
        // in the order they were declared, which the report's columns follow, and not sorted
        assert!(written.find("api/tests").unwrap() < written.find("web/tests").unwrap());

        // and a run over the working directory alone still writes the key, empty
        result.targets = Vec::new();
        assert!(create_document(&result, &Local::now(), &config).contains("\"targets\": []"));
    }

    fn reading_of(source: crate::diff::Source, per_language: HashMap<String, Stats>) -> crate::diff::Reading {
        crate::diff::Reading {
            source,
            taken: "2026-08-05T21:14:03+03:00".to_owned(),
            version: "3.0.0".to_owned(),
            scope: crate::diff::scope_of(&mezura_core::EngineConfig::default(), mezura_core::CountingModel::Content),
            warnings: Vec::new(),
            faulty_files_count: 0,
            unreadable_dirs_count: 0,
            result: result_of(per_language.clone(), Stats::total_of(&per_language), Vec::new(),
                    FilesPresent {total_files: 2, relevant_files: 2, excluded_files: 0})
        }
    }

    // Every count is a triad, the sides carry identity and never counts, and each source's identity
    // has its own shape behind the 'source' discriminator.
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
        assert!(document.contains("\"kind\": \"comparison\""));
        assert!(document.contains("\"source\": \"revision\""), "{document}");
        assert!(document.contains("\"commit\": \"030e6e72a1b4c9d8e7f6a5b4c3d2e1f0a9b8c7d6\""));
        assert!(document.contains("\"asked_for\": \"v2.0.1\""));
        assert!(document.contains("\"source\": \"run\""));
        // each side says what it measured, or two sides over different trees would read as change
        assert!(document.contains("{\"module\": \"core\", \"path\": \"D:/proj/src\"}"), "{document}");

        // every figure is the pair and the journey, so nothing has to be subtracted by the reader
        assert!(document.contains("\"lines\": {\"from\": 140, \"to\": 210, \"change\": 70}"), "{document}");
        // a language of only one side has a whole zero side, and the change can be negative
        assert!(document.contains("\"lines\": {\"from\": 40, \"to\": 0, \"change\": -40}"), "{document}");
        assert!(document.contains("\"structs\": {\"from\": 3, \"to\": 5, \"change\": 2}"), "{document}");

        // and it is a document, not a description of one
        assert!(serde_json::from_str::<serde_json::Value>(&document).is_ok(), "{document}");
    }

    // The second axis of a comparison, which is there only when both readings named the same
    // modules: one that only one of them has would be compared against nothing, and the key being
    // absent has to be told apart from a run that named none, which is what the warning is for.
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
        assert!(document.contains("\"modules\": ["), "{document}");
        assert!(document.contains("\"name\": \"backend\"") && document.contains("\"name\": null"));
        // every figure of a module is the same triad as the figures above it, keywords included, and
        // the leftovers that did not move are in it as a triad of no change
        let block = &document[document.find("\"modules\"").unwrap()..];
        assert!(block.contains("\"lines\": {\"from\": 100, \"to\": 150, \"change\": 50}"), "{block}");
        assert!(block.contains("\"structs\": {\"from\": 3, \"to\": 5, \"change\": 2}"), "{block}");
        assert!(block.contains("\"lines\": {\"from\": 40, \"to\": 40, \"change\": 0}"), "{block}");
        assert!(serde_json::from_str::<serde_json::Value>(&document).is_ok(), "{document}");

        // A module only one of them has takes the whole key with it, and the reader is told why
        // rather than left to read the absence as a run that named nothing
        let renamed = with_modules(crate::diff::Source::Run,
                vec![module(Some("api"), "Rust", 150, 5), module(None, "HTML", 40, 0)]);
        let document = create_comparison_document(&crate::diff::Comparison::of(from(), renamed, &config, Vec::new()), &datetime, &config);
        assert!(!document.contains("\"modules\""), "{document}");
        assert!(document.contains("\"code\": \"modules-differ\""), "{document}");
        assert!(document.contains("\"affects\": \"settings\""));
        assert!(document.contains("\"subject\": \"'backend', '(unnamed)' -> 'api', '(unnamed)'\""), "{document}");

        // and two readings that named nothing at all have no second axis and nothing to report
        let plain = create_comparison_document(&crate::diff::Comparison::of(
                reading_of(crate::diff::Source::Run, HashMap::new()),
                reading_of(crate::diff::Source::Run, HashMap::new()), &config, Vec::new()), &datetime, &config);
        assert!(!plain.contains("modules"), "{plain}");
    }

    // A side that failed to read half its files reports smaller counts, and the triads alone cannot
    // say whether that is code that went away or a scan that came back short. The block is the run
    // document's own, so a consumer learns one shape, and it stops at the counts: which files those
    // were is a question for a run over that side.
    #[test]
    fn each_side_of_a_comparison_says_how_its_own_scan_went() {
        let config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let with_trouble = |source| {
            let mut reading = reading_of(source, hashmap!["Rust".to_owned() => stats_of(1, 30, 10, 5, 0, HashMap::new())]);
            reading.result.files_present = FilesPresent {total_files: 9, relevant_files: 4, excluded_files: 5};
            // The counts and not the lists, which is what a document written without
            // '--show-faulty-files' carries: the block has to be right for that side too
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
        // and the clean side says so rather than leaving the key out, so nothing has to be guessed
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

        // and no list of paths anywhere, on either side: a comparison records how each scan went
        // and never sets out to compare what went wrong in them
        assert!(!document.contains("faulty_files"), "{document}");
        assert!(!document.contains("unreadable_dirs"), "{document}");
    }

    // The same facts the screen says above the table, as entries a program can key on: the sides'
    // own warnings stay inside the sides, and what differs between them is the comparison's own.
    #[test]
    fn a_comparison_says_what_makes_its_sides_two_measurements() {
        let config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let datetime = Local::now();
        let from = || reading_of(crate::diff::Source::Document { path: "D:/old.json".to_owned() }, HashMap::new());
        let mut to = reading_of(crate::diff::Source::Run, HashMap::new());
        to.version = "3.1.0".to_owned();
        to.scope.search_in_dotted = true;

        // A value the two readings were made to agree on is as much a fact about the numbers as one
        // they disagreed on, and after the adoption the two scopes read alike, so without this
        // nothing tells a value the command line gave apart from one it borrowed
        let adopted = crate::diff::Note::SettingsAdopted { from: "old.json".to_owned(), settings: vec!["exclude"] };
        let borrowed = create_comparison_document(&crate::diff::Comparison::of(from(),
                reading_of(crate::diff::Source::Run, HashMap::new()), &config, vec![adopted]), &datetime, &config);
        assert!(borrowed.contains("\"code\": \"setting-adopted\""), "{borrowed}");
        assert!(borrowed.contains("\"subject\": \"exclude\""));
        assert!(borrowed.contains("was taken from 'old.json'"), "{borrowed}");

        let document = create_comparison_document(&crate::diff::Comparison::of(from(), to, &config, Vec::new()), &datetime, &config);
        assert!(document.contains("\"code\": \"setting-differs\""), "{document}");
        assert!(document.contains("\"subject\": \"search-in-dotted\""));
        assert!(document.contains("\"code\": \"versions-differ\""));
        assert!(document.contains("\"subject\": \"3.0.0 -> 3.1.0\""));
        assert!(document.contains("\"path\": \"D:/old.json\""));

        // nothing differing writes the key empty rather than leaving the reader to ask for it
        let same = create_comparison_document(&crate::diff::Comparison::of(from(),
                reading_of(crate::diff::Source::Run, HashMap::new()), &config, Vec::new()), &datetime, &config);
        assert!(same.contains("\"warnings\": []"), "{same}");
    }

    #[test]
    fn a_run_with_nothing_to_count_is_still_a_whole_document() {
        let config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let result = result_of(HashMap::new(), Stats::default(),
                Vec::new(), FilesPresent {total_files: 12, relevant_files: 0, excluded_files: 12});
        let document = create_document(&result, &Local::now(), &config);

        assert!(document.contains("\"languages\": []"));
        assert!(document.contains("\"files\": 0"));
        assert!(document.contains("\"files_found\": 12"));
        assert!(document.contains("\"faulty_files\": []"));
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

        // How many is always said, so a consumer never has to ask for the detail to learn that
        // something went wrong; the paths are the detail, and are asked for
        let counted_only = create_document(&result, &Local::now(), &config);
        assert!(counted_only.contains("\"files_faulty\": 2"));
        assert!(!counted_only.contains("a.rs"), "{counted_only}");

        config.view.set_should_show_faulty_files(true);
        let document = create_document(&result, &Local::now(), &config);
        assert!(document.contains("\"files_faulty\": 2"));
        assert!(document.contains("\"path\": \"src\\\\a.rs\""));
        assert!(document.find("a.rs").unwrap() < document.find("z.rs").unwrap());
    }

    // Objects and not bare paths, so that a consumer can tell a permission apart from a directory
    // that went away between being queued and being opened. As strings there was one sentence for
    // every reason, and on a whole drive that is hundreds of rows saying the same word.
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

        assert!(written.contains("\"path\": \"D:/a\""), "{written}");
        assert!(written.contains("\"error\": \"Access is denied. (os error 5)\""), "{written}");
        assert!(written.contains("\"error\": \"The system cannot find the path specified. (os error 3)\""), "{written}");
        // sorted by path, since the walk collects these in whichever order its threads hit them
        assert!(written.find("D:/a").unwrap() < written.find("D:/z").unwrap(), "{written}");

        // and a run that opened everything still writes the key, empty
        let clean = result_of(HashMap::new(), Stats::default(), Vec::new(),
                FilesPresent {total_files: 0, relevant_files: 0, excluded_files: 0});
        assert!(create_document(&clean, &Local::now(), &config).contains("\"unreadable_dirs\": []"));
    }
}
