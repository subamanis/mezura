use std::collections::HashMap;

use chrono::{DateTime, Local};
use colored::{Color, ColoredString, Colorize};
use mezura_core::{RunResult, Stats, UNNAMED_MODULE_NAME, render};

use super::config_manager::{self, Configuration, Layout, SortCriterion};
use super::number_formatter::format_with_separators;
use super::theme::Theme;

type ColorFunc = Box<dyn Fn(&str) -> String>;

const TOTAL_NAME : &str = "Total";

// What a comparison writes where a figure did not move
const NO_CHANGE : &str = "-";

// How far a language sits under the module it belongs to, in either table
const GROUP_INDENT : &str = "  ";

// The same, in the list layout, whose rows are far wider and already carry a blank line between them
const LIST_INDENT : &str = "    ";

const MATRIX_METRICS : [&str; 3] = ["files", "lines", "code"];

// The row of MATRIX_METRICS that carries the language name, and the only one a module that lacks
// the language marks with a dash. Blanking the other two keeps a sparse matrix free of punctuation.
const MATRIX_LINES_ROW : usize = 1;

// Kept on both sides of the arrow, so the longest language name still has room around it
const NAME_GAP : usize = 3;

// The total number of cells the overview's bar is drawn out of, shared between the languages in it
const NUM_OF_VERTICALS : usize = 50;

// How many languages the overview names before folding the rest into OTHERS_NAME
const OVERVIEW_LANGUAGES : usize = 3;

const OTHERS_NAME : &str = "others";

pub fn format_and_print_results(result: &RunResult, existing_log_content: &Option<String>,
        datetime_now: &DateTime<Local>, config: &Configuration)
{

    let RunResult {per_language, total, ..} = result;
    let groups = create_groups_of(result, config);

    // The rows of the matrix are the languages of the whole run and not of one module, so that is
    // also what '--top' cuts there. Everywhere else it cuts inside each module.
    //
    // Two lists and not one: the overview is handed the uncut one, since it folds everything past
    // its own limit into 'others' itself and cannot do that without seeing what it is folding.
    let global_names = get_sorted_language_names(per_language, config.view.sort_by);
    let matrix_hidden = config.view.top_n.map_or(0, |top| global_names.len().saturating_sub(top));
    let matrix_names = global_names[..global_names.len() - matrix_hidden].to_vec();

    // The list is cut, but the total below it still counts everything, so the reader is told what
    // is missing rather than left to wonder why the rows do not add up
    let hidden_languages = if config.view.layout == Layout::Matrix {matrix_hidden}
            else {groups.iter().map(|x| x.hidden).sum::<usize>()};

    let theme = super::theme::get_active();
    let columns = Columns::of(&groups, total);
    let block_width = columns.width(theme);
    let should_print_keywords = !config.view.hidden.keywords;
    // Nothing to cross when no module was named, so the table is printed instead of a grid of one
    // column. Not an error, since killing a run over how its numbers would be shown costs the
    // numbers, and not silent either: the reader asked for one layout and is getting another.
    let mut layout = config.view.layout;
    if layout == Layout::Matrix && !is_grouped(&groups) {
        layout = Layout::Table;
        eprintln!("\n{}", super::theme::get_active().warning.paint("'--layout matrix' has nothing to cross, since no target was given a name, \
so the 'table' layout was printed. Use the modules feature to get a matrix: 'mezura frontend=./web backend=./api'."));
    }
    let is_table = layout != Layout::List;
    // With modules there is a sum of the module rows to be shown even when one language made all of
    // them, and without them a single language would only be repeated by a total under it
    let print_total = per_language.len() > 1 || groups.len() > 1;

    match layout {
        Layout::Matrix => print_as_matrix(theme, &groups, &matrix_names, total, print_total, should_print_keywords),
        Layout::Boxed => print_as_boxed_table(theme, &groups, total, print_total, should_print_keywords),
        Layout::Table => print_as_table(theme, &groups, total, print_total, should_print_keywords),
        Layout::List => print_individually(theme, &groups, &columns, block_width, should_print_keywords)
    }

    if hidden_languages > 0 {
        let plural = if hidden_languages == 1 {"language"} else {"languages"};
        println!("\n{}", theme.note.paint(&format!("(+{hidden_languages} more {plural} hidden by --top {})", config.view.top_n.unwrap())));
    }

    if print_total {
        if !is_table {
            print_sum(theme, per_language, total, &columns, block_width, should_print_keywords);
        }
        // The overview is the overview: it stays global however the details were grouped
        if !config.view.hidden.overview {
            print_visual_overview(&global_names, per_language, total, config);
        }
    }

    // A log of nothing but whitespace has nothing to compare against, and the section would be a
    // heading with no rows under it. The same file is history the log itself must not destroy, so
    // the emptiness is asked about here rather than by whoever read it.
    if !config.view.hidden.history && let Some(content) = existing_log_content
        && !content.trim().is_empty() && config.view.compare_level != 0 {
        print_comparison_to_previous_runs(result, content, config, datetime_now);
    }
}

// The theme listing runs before a configuration exists, so it cannot go through
// 'super::theme::get_active()'. It asks for the real rows of one made-up language instead, built by the
// same functions a run uses, so that the preview cannot drift from what will actually be printed,
// and it follows the layout in effect for the same reason. The figures are constants, so that every
// theme is judged against the same row.
pub fn create_theme_sample_rows(theme: &Theme, layout: Layout) -> Vec<String> {
    const NAME    : &str   = "Rust";
    const FILES   : usize  = 1_284;
    const LINES   : usize  = 96_512;
    const CODE    : usize  = 71_004;
    const COMMENTS: usize  = 12_838;
    const BYTES   : usize  = 3_412_500;

    let keywords = hashmap!("structs".to_owned() => 284usize, "traits".to_owned() => 31);
    let per_language = hashmap!(NAME.to_owned() => Stats::new(FILES, BYTES, LINES, CODE, COMMENTS, keywords.clone()));
    let total = Stats::total_of(&per_language);
    let groups = vec![Group {name: None, languages: vec![NAME.to_owned()], hidden: 0,
            per_language: &per_language, total: &total, baseline: None}];

    // The two tables keep their keywords in a block of their own, so the sample has to ask for it or
    // the keyword tokens would go unshown in the two layouts that are now the common ones. One
    // language leaves nothing for a total to add up: it would only repeat the row above it.
    let with_keywords = |mut lines: Vec<String>| {
        lines.push(String::new());
        lines.extend(format_keyword_block_lines(theme, &groups));
        lines
    };
    match layout {
        Layout::Table => with_keywords(format_table_lines(theme, &groups, &total, false)),
        Layout::Boxed => with_keywords(format_boxed_lines(theme, &groups, &total, false)),
        // The matrix has no second axis to show for one made-up language of one unnamed module, and
        // the tokens it paints are the ones the table already previews
        Layout::Matrix => with_keywords(format_table_lines(theme, &groups, &total, false)),
        Layout::List => {
            let len_of = |value: usize| format_with_separators(value).len();
            let columns = Columns {
                name: NAME.len().max(TOTAL_NAME.len()),
                headline: len_of(FILES).max(len_of(LINES)),
                code: len_of(CODE),
                comments: len_of(COMMENTS),
                extra: len_of(LINES - CODE - COMMENTS)
            };
            let width = columns.width(theme);
            vec![columns.format_files_row(theme, FILES, &format_size(theme, BYTES, BYTES / FILES), width),
                 columns.format_breakdown_row(theme, &theme.details_language_name.paint(NAME).to_string(), NAME.len(), LINES, CODE, COMMENTS),
                 get_keywords_as_str(theme, &keywords, None, columns.calculate_words_start(), width)]
        }
    }
}

// Ties are broken by name rather than left to the iteration order of the maps, which would make
// the printed order differ between runs on the very projects where languages are evenly matched
pub(crate) fn get_sorted_language_names(per_language: &HashMap<String, Stats>, criterion: SortCriterion) -> Vec<String>
{
    let value_of = |name: &String| match criterion {
        SortCriterion::Files => per_language.get(name).map_or(0, |x| x.files),
        SortCriterion::Size => per_language.get(name).map_or(0, |x| x.bytes),
        SortCriterion::Lines => per_language.get(name).map_or(0, |x| x.lines),
        SortCriterion::Code => per_language.get(name).map_or(0, |x| x.code_lines),
        SortCriterion::Name => 0
    };

    let mut names = per_language.keys().cloned().collect::<Vec<_>>();
    if criterion == SortCriterion::Name {
        names.sort_by_key(|x| x.to_lowercase());
    } else {
        names.sort_by(|a, b| value_of(b).cmp(&value_of(a)).then_with(|| a.to_lowercase().cmp(&b.to_lowercase())));
    }

    names
}

// One part of the run and the languages inside it, in the order '--sort' put them. A run that named
// no module has exactly one of these, with no name.
struct Group<'a> {
    name: Option<&'a str>,
    languages: Vec<String>,
    hidden: usize,
    per_language: &'a HashMap<String, Stats>,
    total: &'a Stats,
    // The same part as an earlier reading counted it, under '--diff' and nowhere else, which is what
    // turns every keyword that moved into 'structs: 60 (+5)'. It belongs to the part rather than to
    // the block drawn out of them: with modules there is one of these per module, and a block handed
    // a single map would measure a module's keywords against every module's.
    baseline: Option<&'a HashMap<String, Stats>>
}

impl Group<'_> {
    fn get_displayed_name(&self) -> &str {
        self.name.unwrap_or(UNNAMED_MODULE_NAME)
    }
}

// One name is enough for the second axis to appear. It cannot stay invisible once other rows exist,
// or the files of everything unnamed would vanish from between the rows and the total.
fn is_grouped(groups: &[Group]) -> bool {
    groups.iter().any(|x| x.name.is_some())
}

// '--sort' applies at both levels with the same criterion, and '--top' is per module, since the
// question it asks is about the rows the reader is looking at
fn create_groups_of<'a>(result: &'a RunResult, config: &Configuration) -> Vec<Group<'a>> {
    // The modules keep the order they were written in and only the languages inside them are
    // sorted: the order the user named their targets in is already a choice, and it is the only way
    // of arranging the columns of a matrix they have. The leftovers are last, never having been
    // declared at all.
    result.modules.iter().map(|module| {
        let languages = get_sorted_language_names(&module.per_language, config.view.sort_by);
        let hidden = config.view.top_n.map_or(0, |top| languages.len().saturating_sub(top));
        Group {
            name: module.name.as_deref(),
            languages: languages[..languages.len() - hidden].to_vec(),
            hidden,
            per_language: &module.per_language,
            total: &module.total,
            baseline: None
        }
    }).collect()
}

#[derive(PartialEq,Eq,Clone,Copy)]
enum RowKind {
    Module,
    Language,
    Total
}

// The name cell of every row that is going to be printed, and what it is
fn create_named_rows<'a>(groups: &'a [Group], print_total: bool) -> Vec<(String, RowKind, &'a Group<'a>, Option<&'a String>)> {
    let grouped = is_grouped(groups);
    let mut rows = Vec::with_capacity(groups.len() * 4);
    for group in groups {
        if grouped {
            rows.push((group.get_displayed_name().to_owned(), RowKind::Module, group, None));
        }
        for name in &group.languages {
            let cell = if grouped {GROUP_INDENT.to_owned() + name} else {name.clone()};
            rows.push((cell, RowKind::Language, group, Some(name)));
        }
    }
    if print_total && !groups.is_empty() {
        rows.push((TOTAL_NAME.to_owned(), RowKind::Total, &groups[0], None));
    }

    rows
}

// The column holds modules and languages alike, and the indentation says which is which. Without
// the change of heading the reader of an uncolored paste is told that 'backend' is a language.
fn determine_name_header(groups: &[Group]) -> &'static str {
    if is_grouped(groups) {"Module"} else {"Language"}
}

// One aligned row per language, no borders: whitespace alignment survives being pasted into a
// README or a ticket. The header cells reuse the label token of the quantity underneath them and
// the body cells its number token, so the table needs no styling of its own.
fn print_as_table(theme: &Theme, groups: &[Group], total: &Stats,
        print_total: bool, should_print_keywords: bool)
{
    println!("{}.\n", theme.heading.paint("Details"));
    for line in format_table_lines(theme, groups, total, print_total) {
        println!("{line}");
    }

    if should_print_keywords {
        print_keyword_block(theme, groups);
    }

    // The 'list' layout closes with a blank line of its own, this one has to say so
    println!();
}

fn format_table_lines(theme: &Theme, groups: &[Group], total: &Stats, print_total: bool) -> Vec<String>
{
    // Every counted column carries its own percentage. The two that compare languages ('Files' and
    // 'Lines') take a share of the total, the two that describe one ('Code' and 'Comments') take a
    // share of that language's own lines.
    const HEADERS : [&str; 11] = ["Language", "Files", "%", "Lines", "%", "Code", "%", "Comments", "%", "Extra", "Size"];
    // The columns a percentage belongs to, kept against their number by a gap of their own
    const TIGHT_AFTER : [usize; 4] = [1, 3, 5, 7];

    fn format_row_of(theme: &Theme, name: &str, files: usize, lines: usize, code: usize, comments: usize, bytes: usize,
            total_files: usize, total_lines: usize) -> Vec<String>
    {
        fn format_percent_cell(value: f64) -> String {
            format_percent_text(value) + "%"
        }

        fn format_share(part: usize, whole: usize) -> String {
            format_percent_cell(if whole == 0 {0.0} else {part as f64 / whole as f64 * 100.0})
        }

        let (size, unit) = super::number_formatter::get_active().size_with_unit(bytes);
        let (code_percentage, comment_percentage) = calculate_code_and_comment_percentages(lines,code, comments);
        vec![name.to_owned(),
         format_with_separators(files), format_share(files, total_files),
         format_with_separators(lines), format_share(lines, total_lines),
         format_with_separators(code), format_percent_cell(code_percentage),
         format_with_separators(comments), format_percent_cell(comment_percentage),
         format_with_separators(lines - code - comments),
         size + " " + &theme.size_unit.paint(unit).to_string()]
    }

    let described = create_named_rows(groups, print_total);
    let rows = described.iter().map(|(cell, kind, group, language)| match kind {
            // A module's share is of the whole, a language's is of the module it is in: a module
            // reading 100% of itself would say nothing, which is the whole point of the two levels
            RowKind::Module => format_row_of(theme, cell, group.total.files, group.total.lines,
                    group.total.code_lines, group.total.comment_lines, group.total.bytes,
                    total.files, total.lines),
            RowKind::Total => format_row_of(theme, cell, total.files, total.lines, total.code_lines,
                    total.comment_lines, total.bytes, total.files, total.lines),
            RowKind::Language => {
                let name = language.unwrap();
                let content_info = group.per_language.get(name).unwrap();
                format_row_of(theme, cell, content_info.files, content_info.lines, content_info.code_lines, content_info.comment_lines,
                        content_info.bytes, group.total.files, group.total.lines)
            }
        }).collect::<Vec<_>>();

    let mut headers = HEADERS.map(str::to_owned).to_vec();
    headers[0] = determine_name_header(groups).to_owned();

    let header_styles = [&theme.details_language_header, &theme.files_label, &theme.percent, &theme.lines_label, &theme.percent,
            &theme.code_label, &theme.percent, &theme.comments_label, &theme.percent, &theme.extra_label, &theme.total_size_label];
    let body_styles = [&theme.details_language_name, &theme.files_number, &theme.percent, &theme.lines_number, &theme.percent,
            &theme.code_number, &theme.percent, &theme.comments_number, &theme.percent, &theme.extra_number, &theme.total_size_number];

    draw_aligned_table(theme, &headers, &rows, &described.iter().map(|(_, kind, _, _)| *kind).collect::<Vec<_>>(),
            &TIGHT_AFTER, &header_styles, &body_styles, is_grouped(groups))
}

// The whole of what '--diff' prints, and the only thing printed at all when both readings were
// given, since then nothing was counted and there is no report for this to take the place of.
pub fn print_comparison(comparison: &super::diff::Comparison, config: &Configuration) {
    let theme = super::theme::get_active();
    let (baseline, subject) = (&comparison.baseline, &comparison.subject);
    let pairs = comparison.module_pairs();

    println!("{}.\n", theme.heading.paint("Details"));
    println!("{}", format_comparison_heading(theme, baseline, subject));
    // Between the heading of the table and its rows, because every one of them is about the figures
    // directly underneath: what this run borrowed to make them comparable, what makes them two
    // measurements anyway, and what the table is not showing.
    for note in &comparison.notes {
        eprintln!("\n{}", format_note_sentence(theme, note));
    }
    println!();

    let rows = create_compared_rows(pairs.as_deref(), &baseline.result, &subject.result, config);
    let lines = match config.view.layout {
        Layout::Boxed => format_boxed_comparison_lines(theme,&rows),
        _ => format_comparison_lines(theme,&rows)
    };
    for line in lines {
        println!("{line}");
    }

    // The total under the rows counts every language whatever '--top' shows, so the reader is told
    // what is missing rather than left to wonder why the rows do not add up, as the report says it
    let hidden = count_languages_hidden_by_top(pairs.as_deref(), &baseline.result, &subject.result, config.view.top_n);
    if hidden > 0 {
        let plural = if hidden == 1 {"language"} else {"languages"};
        println!("\n{}", theme.note.paint(&format!("(+{hidden} more {plural} hidden by --top {})", config.view.top_n.unwrap())));
    }

    if !config.view.hidden.keywords {
        let groups = match pairs.as_deref() {
            Some(pairs) => pairs.iter().map(|pair| create_group_with_baseline(pair.name, &pair.before.per_language,
                    &pair.now.per_language, &pair.now.total, config)).collect::<Vec<_>>(),
            None => vec![create_group_with_baseline(None, &baseline.result.per_language, &subject.result.per_language,
                    &subject.result.total, config)]
        };
        print_keyword_block(theme, &groups);
    }
    println!();
}

// One printed row of a comparison: its name cell, indent and all, and the two readings behind it.
struct ComparedRow {
    name: String,
    kind: RowKind,
    baseline: Stats,
    subject: Stats
}

// What 'named_rows' is to a report, which has one reading to draw. A module gets a row of its own
// with its languages indented under it, exactly as a grouped report does.
fn create_compared_rows(pairs: Option<&[super::diff::ModulePair]>, baseline: &RunResult, subject: &RunResult,
        config: &Configuration) -> Vec<ComparedRow>
{
    let languages_of = |baseline_languages: &HashMap<String, Stats>, subject_languages: &HashMap<String, Stats>, indent: &str| {
        super::diff::create_comparison_rows(baseline_languages, subject_languages, config.view.sort_by, config.view.top_n)
                .0.into_iter()
                .map(|change| ComparedRow { name: indent.to_owned() + &change.name,
                        kind: RowKind::Language, baseline: change.baseline, subject: change.subject })
                .collect::<Vec<_>>()
    };

    let mut rows = Vec::new();
    match pairs {
        Some(pairs) => for pair in pairs {
            rows.push(ComparedRow { name: pair.name.unwrap_or(UNNAMED_MODULE_NAME).to_owned(),
                    kind: RowKind::Module, baseline: pair.before.total.clone(), subject: pair.now.total.clone() });
            rows.extend(languages_of(&pair.before.per_language, &pair.now.per_language, GROUP_INDENT));
        },
        None => rows.extend(languages_of(&baseline.per_language, &subject.per_language, ""))
    }
    rows.push(ComparedRow { name: TOTAL_NAME.to_owned(), kind: RowKind::Total,
            baseline: baseline.total.clone(), subject: subject.total.clone() });

    rows
}

// One part of a comparison as the keyword block reads it: the languages of the rows the table
// printed, so one cut governs both and the block cannot name a language with no row above it, and
// what the earlier reading had of each. A language that is gone keeps its row in the table and has
// no keywords now, so it leaves the list here.
fn create_group_with_baseline<'a>(name: Option<&'a str>, baseline: &'a HashMap<String, Stats>,
        subject: &'a HashMap<String, Stats>, total: &'a Stats, config: &Configuration) -> Group<'a>
{
    let (rows, union) = super::diff::create_comparison_rows(baseline, subject, config.view.sort_by, config.view.top_n);
    let hidden = union - rows.len();
    let languages = rows.into_iter().map(|row| row.name)
            .filter(|language| subject.contains_key(language)).collect();

    Group { name, languages, hidden, per_language: subject, total, baseline: Some(baseline) }
}

// What '--top' left out of the rows, counted where they were cut: inside each module when the
// modules are shown, over everything at once otherwise. Both readings' languages are in it, since a
// row exists for one that only the earlier had.
fn count_languages_hidden_by_top(pairs: Option<&[super::diff::ModulePair]>, baseline: &RunResult,
        subject: &RunResult, top: Option<usize>) -> usize
{
    if top.is_none() {
        return 0;
    }
    let cut = |before: &HashMap<String, Stats>, now: &HashMap<String, Stats>| {
        let (rows, union) = super::diff::create_comparison_rows(before, now, SortCriterion::Lines, top);
        union - rows.len()
    };

    match pairs {
        Some(pairs) => pairs.iter().map(|x| cut(&x.before.per_language, &x.now.per_language)).sum(),
        None => cut(&baseline.per_language, &subject.per_language)
    }
}

fn format_note_sentence(theme: &Theme, note: &super::diff::Note) -> String {
    use super::diff::Note;
    match note {
        Note::SettingsAdopted { from, settings } => {
            let one = settings.len() == 1;
            let (was, value, it) = if one {("has", "value", "it")} else {("have", "values", "them")};
            theme.warning.paint(&format!("'{}' {was} been overridden by the {value} recorded \
in '{from}', so both readings are counted the same way. Provide {it} explicitly in the command line to \
keep your own.", settings.join("', '"))).to_string()
        },
        Note::SettingsDiffer { baseline, subject, settings } => theme.warning.paint(&format!(
                "'{baseline}' and '{subject}' were not taken with the same {}, so part of the difference below is \
those settings and not code that changed.", settings.join(", "))).to_string(),
        Note::VersionsDiffer { baseline, baseline_version, subject, subject_version } => theme.warning.paint(&format!(
                "'{baseline}' was counted by mezura {baseline_version} and '{subject}' by {subject_version}, \
so part of the difference below may be a language counted better since, and not code that changed.")).to_string(),
        Note::CountsInDoubt { about, doubts } => format!("{}\n{}", theme.warning.paint(&format!(
                "The run that took '{about}' was not sure of its own counts:")),
                doubts.iter().map(|x| format!("-- {x}")).collect::<Vec<_>>().join("\n")),
        Note::NothingCounted { about } => theme.warning.paint(&format!(
                "'{about}' found no relevant files, so its side of every figure is zero.")).to_string(),
        Note::ModulesDiffer { baseline, subject, baseline_modules, subject_modules } => {
            // The word 'modules' is said once, by the first side, and the second reads on from it
            let first = match baseline_modules {
                Some(names) => format!("'{baseline}' declared modules {names}"),
                None => format!("'{baseline}' didn't declare any modules")
            };
            let second = match subject_modules {
                Some(names) => format!("'{subject}' declared {names}"),
                None => format!("'{subject}' didn't declare any")
            };
            theme.warning.paint(&format!("{first}, whereas {second}. Module declarations \
must match between the two sources for the modules to take effect in the comparison. Defaulting to \
the normal comparison view.")).to_string()
        },
        Note::LayoutFallback { layout } => theme.warning.paint(&format!(
                "'--{} {layout}' has nothing to show for a comparison, so the 'table' layout was printed.",
                config_manager::LAYOUT)).to_string(),
        Note::NoGitignoreInCheckout { git_revision } => theme.warning.paint(&format!(
                "'--no-gitignore' cannot reach '{git_revision}': a checkout holds only what git tracks, \
so anything a .gitignore ignores is counted on one side alone.")).to_string(),
        Note::MissingInRevision { git_revision, targets } => {
            let named = targets.iter().map(|x| format!("'{x}'")).collect::<Vec<_>>().join(", no ");
            theme.warning.paint(&format!("'{git_revision}' has no {named}, so it counts as nothing there.")).to_string()
        }
    }
}

// The comparison in the boxed frame: the same triads as the table, with each figure's change in the
// slot its share occupies on a plain run, and 'Extra' gone the same way. The change cells arrive
// painted by their direction, so the frame's own slot style is plain.
fn format_boxed_comparison_lines(theme: &Theme, rows: &[ComparedRow]) -> Vec<String>
{
    const HEADERS : [&str; 6] = ["Language", "Files", "Lines", "Code", "Comments", "Size"];

    let cells = |before: &Stats, now: &Stats| {
        // The absolute move and its percentage share one slot here, the borders doing the grouping
        // that the tight gaps do on the table
        let counted = |was: usize, is: usize| BoxedCell {
            number: format_with_separators(is),
            slot: if was == is {paint_change(theme, was, is, NO_CHANGE)}
                    else {paint_change(theme, was, is, &format!("{}  {}", format_signed_difference(was, is), format_change(was, is)))}
        };
        let (size, unit) = super::number_formatter::get_active().size_with_unit(now.bytes);
        // The file count moves in single whole things, so its slot carries the move and no
        // percentage, exactly as on the table
        let files = BoxedCell {
            number: format_with_separators(now.files),
            slot: paint_change(theme, before.files, now.files, &format_signed_difference(before.files, now.files))
        };
        vec![files, counted(before.lines, now.lines),
             counted(before.code_lines, now.code_lines), counted(before.comment_lines, now.comment_lines),
             BoxedCell { number: size + " " + &theme.size_unit.paint(unit).to_string(),
                         slot: paint_change(theme, before.bytes, now.bytes, &format_signed_size(theme, before.bytes, now.bytes)) }]
    };

    let drawn = rows.iter().map(|row| (row.name.clone(), cells(&row.baseline, &row.subject))).collect::<Vec<_>>();
    let kinds = rows.iter().map(|row| row.kind).collect::<Vec<_>>();

    let plain = super::theme::Style::plain();
    let header_styles = [&theme.files_label, &theme.lines_label, &theme.code_label,
            &theme.comments_label, &theme.total_size_label];
    let number_styles = [&theme.files_number, &theme.lines_number, &theme.code_number,
            &theme.comments_number, &theme.total_size_number];

    draw_boxed_table(theme, determine_name_header_for(&kinds), &HEADERS, &drawn, &kinds,
            &header_styles, &number_styles, &plain)
}

// 'From A to B' and not 'compared A to B': the columns hold B's counts and the signs are the
// journey, so a sentence that puts A first as its subject says the opposite of the table.
fn format_comparison_heading(theme: &Theme, baseline: &super::diff::Reading, subject: &super::diff::Reading) -> String {
    format!("{} '{}' ({}) {} '{}' ({})", theme.history_entry.paint("From"),
            baseline.determine_display_name(), format_readable_time(&baseline.taken), theme.history_entry.paint("to"),
            subject.determine_display_name(), format_readable_time(&subject.taken))
}

// What '--diff' prints in place of the details, and why it is the details table with columns taken
// out rather than a block of its own: every figure has room for the change beside it only because the
// share percentages, 'Extra' and 'Size' are gone. The shares are what the change replaces, 'Extra' is
// the three columns left over subtracted from the lines, and the size is the one figure genuinely
// dropped.
fn format_comparison_lines(theme: &Theme, rows: &[ComparedRow]) -> Vec<String>
{
    // The change columns are left unnamed: their values are two to five characters and the word would
    // widen the table for nothing, while every one of them carries a sign that says what it is.
    // The size carries no percentage of its own: it tracks the lines it is made of, so its share of
    // the change is the one beside them, and a second copy of it costs eight columns
    // The file count carries no percentage either: it is a handful of whole things, and "two more
    // files" is the answer where "+5.26%" is the same fact with a decimal point put on it
    const HEADERS : [&str; 14] = ["Language", "Files", "", "Lines", "", "%", "Code", "", "%",
            "Comments", "", "%", "Size", ""];
    // Both the change and the percentage belong to the number before them
    const TIGHT_AFTER : [usize; 8] = [1, 3, 4, 6, 7, 9, 10, 12];

    let plain = super::theme::Style::plain();
    // The direction is a property of the value and not of the column, so these two cells arrive
    // already painted and their column adds nothing
    let cells = |name: String, before: &Stats, now: &Stats| {
        let mut row = vec![name, format_with_separators(now.files),
                paint_change(theme, before.files, now.files, &format_signed_difference(before.files, now.files))];
        for (was, is) in [(before.lines, now.lines), (before.code_lines, now.code_lines),
                (before.comment_lines, now.comment_lines)] {
            row.push(format_with_separators(is));
            row.push(paint_change(theme, was, is, &format_signed_difference(was, is)));
            row.push(paint_change(theme, was, is, &format_change(was, is)));
        }
        let (size, unit) = super::number_formatter::get_active().size_with_unit(now.bytes);
        row.push(size + " " + &theme.size_unit.paint(unit).to_string());
        row.push(paint_change(theme, before.bytes, now.bytes, &format_signed_size(theme, before.bytes, now.bytes)));
        row
    };

    let drawn = rows.iter().map(|row| cells(row.name.clone(), &row.baseline, &row.subject)).collect::<Vec<_>>();
    let kinds = rows.iter().map(|row| row.kind).collect::<Vec<_>>();

    let mut headers = HEADERS.map(str::to_owned).to_vec();
    let header_styles = [&theme.details_language_header, &theme.files_label, &plain,
            &theme.lines_label, &plain, &theme.percent, &theme.code_label, &plain, &theme.percent,
            &theme.comments_label, &plain, &theme.percent, &theme.total_size_label, &plain];
    let body_styles = [&theme.details_language_name, &theme.files_number, &plain,
            &theme.lines_number, &plain, &plain, &theme.code_number, &plain, &plain,
            &theme.comments_number, &plain, &plain, &theme.total_size_number, &plain];
    headers[0] = determine_name_header_for(&kinds).to_owned();

    draw_aligned_table(theme, &headers, &drawn, &kinds, &TIGHT_AFTER, &header_styles,
            &body_styles, kinds.contains(&RowKind::Module))
}

// The counterpart of 'name_header' for a comparison, whose rows are already built: the column holds
// modules and languages alike, and without the change of heading the reader of an uncolored paste
// is told that 'backend' is a language.
fn determine_name_header_for(kinds: &[RowKind]) -> &'static str {
    if kinds.contains(&RowKind::Module) {"Module"} else {"Language"}
}

// A dash and not a zero, and nothing at all where the percentage would go: a column of dashes is
// read past, while a column of zeros has to be read to find the rows that are not one.
fn format_signed_difference(before: usize, now: usize) -> String {
    if before == now {
        return NO_CHANGE.to_owned();
    }
    let sign = if now > before {"+"} else {"-"};

    format!("{sign}{}", format_with_separators(now.abs_diff(before)))
}

fn format_signed_size(theme: &Theme, before: usize, now: usize) -> String {
    if before == now {
        return NO_CHANGE.to_owned();
    }
    let (text, unit) = super::number_formatter::get_active().size_with_unit(now.abs_diff(before));
    let sign = if now > before {"+"} else {"-"};

    format!("{sign}{text} {}", theme.size_unit.paint(unit))
}

fn format_change(before: usize, now: usize) -> String {
    if before == now {
        return NO_CHANGE.to_owned();
    }
    match super::diff::change_of(before, now) {
        super::diff::Change::Appeared => "new".to_owned(),
        super::diff::Change::Gone => "gone".to_owned(),
        super::diff::Change::Percent(x) => super::number_formatter::get_active().signed_percent(x) + "%"
    }
}

// The three cases already have tokens of their own, since the history section colors the same
// three. What is added here is only that a figure which did not move is dimmed as well: most rows of
// a comparison are that, and they are there to be read past rather than read.
fn paint_change(theme: &Theme, before: usize, now: usize, text: &str) -> String {
    match now.cmp(&before) {
        std::cmp::Ordering::Greater => theme.change_up.paint(text).to_string(),
        std::cmp::Ordering::Less => theme.change_down.paint(text).to_string(),
        std::cmp::Ordering::Equal => theme.change_same.clone().dim().paint(text).to_string()
    }
}

// The moment the baseline was written, as a person reads a date. Left as it stands when it does not
// parse, since a document somebody edited by hand is still worth comparing against.
fn format_readable_time(generated_at: &str) -> String {
    match DateTime::parse_from_rfc3339(generated_at) {
        Ok(x) => x.format("%Y-%m-%d %H:%M").to_string(),
        Err(_) => generated_at.to_owned()
    }
}

// The alignment the details table and the comparison that replaces it under '--diff' both use: every
// column as wide as its widest cell, the figures right aligned so that a column can be read down, and
// the columns named in 'tight_after' sitting two spaces behind the one before them because they
// belong to it rather than standing on their own.
//
// Widths are measured with the escape sequences skipped rather than counted, since a cell is allowed
// to carry a color of its own, which the size cell does for its unit.
fn draw_aligned_table(theme: &Theme, headers: &[String], rows: &[Vec<String>], kinds: &[RowKind],
        tight_after: &[usize], header_styles: &[&super::theme::Style], body_styles: &[&super::theme::Style],
        grouped: bool) -> Vec<String>
{
    const GAP : usize = 4;
    const TIGHT_GAP : usize = 2;

    let widths = (0..headers.len()).map(|i|
            rows.iter().map(|row| calculate_widest_visible_line(&row[i])).max().unwrap_or(0).max(headers[i].len())).collect::<Vec<_>>();

    // The language name and the percentages are not right aligned: a percentage sits a fixed two
    // spaces after the number it belongs to, and padding it on the left would push it away on exactly
    // the rows where its column is wider.
    let render = |cells: &[String], styles: &[&super::theme::Style]| {
        let mut line = String::with_capacity(140);
        for (i, cell) in cells.iter().enumerate() {
            let padding = " ".repeat(widths[i] - calculate_widest_visible_line(cell));
            if i == 0 {
                line.push_str(&format!("{}{}", styles[i].paint(cell), padding));
            } else if tight_after.contains(&(i - 1)) {
                line.push_str(&format!("{}{}{}", " ".repeat(TIGHT_GAP), styles[i].paint(cell), padding));
            } else {
                line.push_str(&format!("{}{}{}", " ".repeat(GAP), padding, styles[i].paint(cell)));
            }
        }
        // A tight column pads on its right, so a table whose last column is one of those ends every
        // row in whitespace, which survives being pasted anywhere
        line.trim_end().to_owned()
    };

    let rendered = std::iter::once(render(headers, header_styles))
            .chain(rows.iter().zip(kinds.iter()).map(|(row, kind)| {
                let mut styles = body_styles.to_vec();
                styles[0] = match kind {
                    RowKind::Module => &theme.details_module,
                    RowKind::Total => &theme.details_total,
                    RowKind::Language => &theme.details_language_name
                };
                render(row, &styles)
            })).collect::<Vec<_>>();

    // Measured off the rows themselves rather than added up from the widths, which is the only way
    // it stays right: a row ends where its last cell ends, and a table whose last column is a tight
    // one has the padding of that column trimmed off the end of every row.
    let table_width = rendered.iter().map(|x| calculate_widest_visible_line(x)).max().unwrap_or(0);

    let mut lines = Vec::with_capacity(rendered.len() + kinds.len());
    let mut rendered = rendered.into_iter();
    lines.push(rendered.next().unwrap_or_default());
    // A blank line closes each module, so the sections are read apart at a glance instead of being
    // told apart only by the indentation of every second row. Without grouping there are no
    // sections and nothing changes.
    for (position, (line, kind)) in rendered.zip(kinds.iter()).enumerate() {
        if grouped && position > 0 && *kind != RowKind::Language {
            lines.push(String::new());
        }
        if *kind == RowKind::Total {
            lines.push(theme.separator.paint(&"-".repeat(table_width)).to_string());
        }
        lines.push(line);
    }

    lines
}

// Languages down, modules across. The nested table answers "what is inside the backend", read down
// a section; this one answers "how do the modules compare on the same language", read along a row.
fn print_as_matrix(theme: &Theme, groups: &[Group], languages: &[String], total: &Stats,
        print_total: bool, should_print_keywords: bool)
{
    println!("{}.\n", theme.heading.paint("Details"));
    for line in format_matrix_lines(theme, groups, languages, total, print_total) {
        println!("{line}");
    }

    if should_print_keywords {
        // The rows of the matrix are cut globally by '--top' while every other layout cuts per
        // module, so the block has to follow these rows or it names a language with no row above it
        let shown = groups.iter().map(|group| Group {
            name: group.name,
            languages: languages.iter().filter(|x| group.per_language.contains_key(*x)).cloned().collect(),
            hidden: group.hidden,
            per_language: group.per_language,
            total: group.total,
            baseline: group.baseline
        }).collect::<Vec<_>>();
        print_keyword_block(theme, &shown);
    }
    println!();
}

fn format_matrix_lines<'a>(theme: &'a Theme, groups: &[Group], languages: &[String], total: &Stats,
        print_total: bool) -> Vec<String>
{
    const GAP : usize = 4;
    const TOTAL_HEADER : &str = "Total";

    // 'None' is a module that does not have the language at all, which is not the same as one that
    // has it and counts zero
    let of_group = |group: &Group, language: &str, metric: usize| -> Option<usize> {
        let content_info = group.per_language.get(language)?;
        Some(match metric {
            0 => group.per_language.get(language).map_or(0, |x| x.files),
            1 => content_info.lines,
            _ => content_info.code_lines
        })
    };
    let of_stats = |stats: &Stats, metric: usize| match metric {
        0 => stats.files,
        1 => stats.lines,
        _ => stats.code_lines
    };
    let cell_of = |value: Option<usize>, metric: usize| match value {
        Some(value) => format_with_separators(value),
        None if metric == MATRIX_LINES_ROW => "-".to_owned(),
        None => String::new()
    };

    // One language is three rows with its name on the first of them, so it reads as the heading of
    // its own block. The labels are written once, against the total when there is one and against
    // the languages when there is not, and the blocks above take their meaning from the same order.
    let mut rows = Vec::with_capacity(languages.len() * MATRIX_METRICS.len() + MATRIX_METRICS.len());
    for language in languages {
        for (metric, label) in MATRIX_METRICS.iter().enumerate() {
            let label = if print_total {String::new()} else {(*label).to_owned()};
            let mut cells = vec![if metric == 0 {language.clone()} else {String::new()}, label];
            cells.extend(groups.iter().map(|group| cell_of(of_group(group, language, metric), metric)));
            let total = groups.iter().filter_map(|group| of_group(group, language, metric)).sum::<usize>();
            cells.push(cell_of(Some(total), metric));
            rows.push((cells, metric));
        }
    }
    // The total counts every language, including the ones '--top' left out of the rows above
    let totals = MATRIX_METRICS.iter().enumerate().map(|(metric, label)| {
        let mut cells = vec![if metric == 0 {TOTAL_HEADER.to_owned()} else {String::new()}, (*label).to_owned()];
        cells.extend(groups.iter().map(|group| cell_of(Some(of_stats(group.total, metric)), metric)));
        cells.push(cell_of(Some(of_stats(total, metric)), metric));
        (cells, metric)
    }).collect::<Vec<_>>();

    let headers = [String::from("Language"), String::new()].into_iter()
            .chain(groups.iter().map(|group| group.get_displayed_name().to_owned()))
            .chain(std::iter::once(TOTAL_HEADER.to_owned())).collect::<Vec<_>>();
    let widths = (0..headers.len()).map(|i| rows.iter().chain(totals.iter())
            .map(|(row,_)| row[i].chars().count()).max().unwrap_or(0).max(headers[i].chars().count()))
            .collect::<Vec<_>>();

    // The name and its labels are left aligned like the labels they are, and every figure is right
    // aligned, so that a column can be compared down and a language across
    let render = |cells: &[String], styles: &[&super::theme::Style]| {
        let mut line = String::with_capacity(140);
        for (i, cell) in cells.iter().enumerate() {
            let padding = " ".repeat(widths[i] - cell.chars().count());
            if i == 0 {
                line.push_str(&format!("{}{padding}", styles[i].paint(cell)));
            } else if i == 1 {
                line.push_str(&format!("{}{}{padding}", " ".repeat(GAP), styles[i].paint(cell)));
            } else {
                line.push_str(&format!("{}{padding}{}", " ".repeat(GAP), styles[i].paint(cell)));
            }
        }
        line.trim_end().to_owned()
    };

    // Each of the three rows takes the tokens of the quantity it carries, so the matrix is themed by
    // what a reader already set for the other layouts and needs nothing of its own
    let number_style = |metric: usize| match metric {
        0 => &theme.files_number,
        1 => &theme.lines_number,
        _ => &theme.code_number
    };
    let label_style = |metric: usize| match metric {
        0 => &theme.files_label,
        1 => &theme.lines_label,
        _ => &theme.code_label
    };
    let mut header_styles = vec![&theme.details_language_header, &theme.details_language_header];
    header_styles.extend(groups.iter().map(|_| &theme.details_module));
    header_styles.push(&theme.details_total);

    let styles_for = |name_style: &'a super::theme::Style, metric: usize| {
        [name_style, label_style(metric)].into_iter()
                .chain((0..headers.len() - 2).map(|_| number_style(metric))).collect::<Vec<_>>()
    };

    let table_width = widths.iter().sum::<usize>() + GAP * (headers.len() - 1);
    let mut lines = vec![render(&headers, &header_styles)];
    for (position, (row, metric)) in rows.iter().enumerate() {
        // A language is three physical rows, so they need to be told apart by something other than
        // the name sitting on the first of them
        if position > 0 && position % MATRIX_METRICS.len() == 0 {
            lines.push(String::new());
        }
        lines.push(render(row, &styles_for(&theme.details_language_name, *metric)));
    }
    // One module and one language leaves nothing for a total to add up, and here it would repeat
    // the single row twice over, since the matrix already carries a Total column.
    if print_total {
        lines.push(theme.separator.paint(&"-".repeat(table_width)).to_string());
        for (row, metric) in &totals {
            lines.push(render(row, &styles_for(&theme.details_total, *metric)));
        }
    }

    lines
}

// The same figures as the borderless table, in a drawn frame. Each number and its percentage share
// one cell here, since the borders already do the grouping that the tight gap does over there, and
// that brings the whole thing down from eleven columns to seven.
fn print_as_boxed_table(theme: &Theme, groups: &[Group], total: &Stats,
        print_total: bool, should_print_keywords: bool)
{
    println!("{}.
", theme.heading.paint("Details"));
    for line in format_boxed_lines(theme, groups, total, print_total) {
        println!("{line}");
    }

    if should_print_keywords {
        print_keyword_block(theme, groups);
    }
    println!();
}

fn format_boxed_lines(theme: &Theme, groups: &[Group], total: &Stats, print_total: bool) -> Vec<String>
{
    const HEADERS : [&str; 7] = ["Language", "Files", "Lines", "Code", "Comments", "Extra", "Size"];

    fn format_row_of(theme: &Theme, name: &str, files: usize, lines: usize, code: usize, comments: usize, bytes: usize,
            total_files: usize, total_lines: usize) -> (String, Vec<BoxedCell>)
    {
        fn format_share(part: usize, whole: usize) -> String {
            format_percent_text(if whole == 0 {0.0} else {part as f64 / whole as f64 * 100.0}) + "%"
        }
        fn create_cell(number: String, slot: String) -> BoxedCell {
            BoxedCell { number, slot }
        }

        let (size, unit) = super::number_formatter::get_active().size_with_unit(bytes);
        let (code_percentage, comment_percentage) = calculate_code_and_comment_percentages(lines,code, comments);
        (name.to_owned(), vec![
            create_cell(format_with_separators(files), format_share(files, total_files)),
            create_cell(format_with_separators(lines), format_share(lines, total_lines)),
            create_cell(format_with_separators(code), format_percent_text(code_percentage) + "%"),
            create_cell(format_with_separators(comments), format_percent_text(comment_percentage) + "%"),
            create_cell(format_with_separators(lines - code - comments), String::new()),
            create_cell(size + " " + &theme.size_unit.paint(unit).to_string(), String::new())])
    }

    let described = create_named_rows(groups, print_total);
    let rows = described.iter().map(|(cell, kind, group, language)| match kind {
            RowKind::Module => format_row_of(theme, cell, group.total.files, group.total.lines,
                    group.total.code_lines, group.total.comment_lines, group.total.bytes,
                    total.files, total.lines),
            RowKind::Total => format_row_of(theme, cell, total.files, total.lines, total.code_lines,
                    total.comment_lines, total.bytes, total.files, total.lines),
            RowKind::Language => {
                let name = language.unwrap();
                let content_info = group.per_language.get(name).unwrap();
                format_row_of(theme, cell, content_info.files, content_info.lines, content_info.code_lines, content_info.comment_lines,
                        content_info.bytes, group.total.files, group.total.lines)
            }
        }).collect::<Vec<_>>();
    let kinds = described.iter().map(|(_, kind, _, _)| *kind).collect::<Vec<_>>();

    let header_styles = [&theme.files_label, &theme.lines_label, &theme.code_label,
            &theme.comments_label, &theme.extra_label, &theme.total_size_label];
    let number_styles = [&theme.files_number, &theme.lines_number, &theme.code_number,
            &theme.comments_number, &theme.extra_number, &theme.total_size_number];

    draw_boxed_table(theme, determine_name_header(groups), &HEADERS, &rows, &kinds, &header_styles, &number_styles,
            &theme.percent)
}

// One cell of the boxed frame: the count, and the slot beside it that a run fills with a share and a
// comparison with a change. A column whose slots are all empty is drawn without one.
struct BoxedCell { number: String, slot: String }

// The frame the boxed layout draws, for whatever columns it is handed: the details fill it with one
// reading, the comparison with two.
fn draw_boxed_table(theme: &Theme, name_title: &str, headers: &[&str], rows: &[(String, Vec<BoxedCell>)],
        kinds: &[RowKind], header_styles: &[&super::theme::Style], number_styles: &[&super::theme::Style],
        slot_style: &super::theme::Style) -> Vec<String>
{
    const SLOT_GAP : usize = 2;
    // One space of air between a border and the text it holds
    const PAD : usize = 1;

    let columns = headers.len() - 1;
    let name_width = rows.iter().map(|(name,_)| name.chars().count()).max().unwrap_or(0).max(name_title.len());
    // Measured with the escape sequences skipped, since the size cell colors its own unit and a
    // comparison's change cells arrive painted by their direction
    let number_widths = (0..columns).map(|i| rows.iter().map(|(_,cells)| calculate_widest_visible_line(&cells[i].number)).max().unwrap_or(0))
            .collect::<Vec<_>>();
    let slot_widths = (0..columns).map(|i| rows.iter().map(|(_,cells)| calculate_widest_visible_line(&cells[i].slot)).max().unwrap_or(0))
            .collect::<Vec<_>>();

    // A column is as wide as its content needs, or as its header, whichever is more
    let inner_widths = std::iter::once(name_width).chain((0..columns).map(|i| {
            let content = number_widths[i] + if slot_widths[i] > 0 {SLOT_GAP + slot_widths[i]} else {0};
            content.max(headers[i + 1].len())
        })).collect::<Vec<_>>();

    // Not theme tokens, since they would mean nothing in the other layouts. That needs FEAT-17.
    const BORDER_OUTER : Color = Color::TrueColor { r: 160, g: 160, b: 160 };
    const BORDER_INNER : Color = Color::TrueColor { r: 65, g: 65, b: 65 };
    const BORDER_INNER_ALT : Color = Color::TrueColor { r: 140, g: 140, b: 140 };

    // The two ends always belong to the outer frame. A crossing takes the shade of its own line when
    // that line is solid, and the interior shade when it is dashed, so that it never cuts the
    // vertical it sits on with a brighter bead.
    let frame = |left: &str, joint: &str, right: &str, fill: &str, shade: Color, dashed: bool| {
        let painted_joint = joint.color(if dashed {BORDER_INNER} else {shade}).to_string();
        let runs = inner_widths.iter().map(|width| fill.repeat(width + 2 * PAD).color(shade).to_string())
                .collect::<Vec<_>>();
        format!("{}{}{}", left.color(BORDER_OUTER), runs.join(&painted_joint), right.color(BORDER_OUTER))
    };

    // The header and the total sit between two solid lines, so their verticals are solid too. Only
    // the language rows, the ones the dashed lines separate, get the dim ones.
    let content_row = |cells: Vec<String>, bright: bool| {
        let padding = " ".repeat(PAD);
        let bar = "│".color(BORDER_OUTER).to_string();
        let inner_bar = "│".color(if bright {BORDER_OUTER} else {BORDER_INNER}).to_string();
        format!("{bar}{padding}{}{padding}{bar}", cells.join(&format!("{padding}{inner_bar}{padding}")))
    };

    let mut lines = vec![frame("┌", "┬", "┐", "─", BORDER_OUTER, false)];

    // The titles sit over columns of mixed width, so they are centred rather than pinned to one
    // side of a cell that is often wider than the word in it
    let centred = format!("{:^width$}", name_title, width = inner_widths[0]);
    let mut header_cells = vec![theme.details_language_header.paint(centred.trim_end()).to_string()
            + &" ".repeat(centred.len() - centred.trim_end().len())];
    for (i, style) in header_styles.iter().enumerate() {
        let width = inner_widths[i + 1];
        let text = headers[i + 1];
        let left = (width - text.len()) / 2;
        header_cells.push(format!("{}{}{}", " ".repeat(left), style.paint(text), " ".repeat(width - text.len() - left)));
    }
    lines.push(content_row(header_cells, true));

    // The lines that bound the body, and the line that opens a module's section, belong to the
    // frame: solid and in its shade. Only the lines between two languages are dashed, and those
    // alternate between two shades.
    for (position, (name, cells)) in rows.iter().enumerate() {
        let kind = kinds[position];
        let separator = if position == 0 || kind != RowKind::Language {
            frame("├", "┼", "┤", "─", BORDER_OUTER, false)
        } else {
            frame("├", "┼", "┤", "╌", if position % 2 == 1 {BORDER_INNER_ALT} else {BORDER_INNER}, true)
        };
        lines.push(separator);

        let name_style = match kind {
            RowKind::Module => &theme.details_module,
            RowKind::Total => &theme.details_total,
            RowKind::Language => &theme.details_language_name
        };
        let mut painted = vec![format!("{}{}", name_style.paint(name), " ".repeat(inner_widths[0] - name.chars().count()))];
        for (i, cell) in cells.iter().enumerate() {
            let number = format!("{}{}", " ".repeat(number_widths[i] - calculate_widest_visible_line(&cell.number)),
                    number_styles[i].paint(&cell.number));
            let body = if slot_widths[i] > 0 {
                format!("{number}{}{}{}", " ".repeat(SLOT_GAP), slot_style.paint(&cell.slot),
                        " ".repeat(slot_widths[i] - calculate_widest_visible_line(&cell.slot)))
            } else {
                number
            };
            let used = number_widths[i] + if slot_widths[i] > 0 {SLOT_GAP + slot_widths[i]} else {0};
            painted.push(format!("{body}{}", " ".repeat(inner_widths[i + 1] - used)));
        }
        lines.push(content_row(painted, kind != RowKind::Language));
    }

    lines.push(frame("└", "┴", "┘", "─", BORDER_OUTER, false));
    lines
}

fn print_individually(theme: &Theme, groups: &[Group], columns: &Columns, block_width: usize, should_print_keywords: bool)
{
    print_lines(&format_individual_lines(theme, groups, columns, block_width, should_print_keywords));
}

fn format_individual_lines(theme: &Theme, groups: &[Group], columns: &Columns, block_width: usize,
     should_print_keywords: bool) -> Vec<String>
{
    let grouped = is_grouped(groups);
    let indent = if grouped {LIST_INDENT} else {""};
    let mut lines = vec![format!("{}.", theme.heading.paint("Details")), String::new()];

    for (position, group) in groups.iter().enumerate() {
        if position > 0 {
            lines.push(String::new());
        }
        if grouped {
            let name = group.get_displayed_name();
            let stats = group.total;
            lines.push(columns.format_files_row(theme, stats.files,
                    &format_size(theme, stats.bytes, stats.calculate_average_size()), block_width));
            lines.push(columns.format_breakdown_row(theme, &theme.details_module.paint(name).to_string(),
                    name.chars().count(), stats.lines, stats.code_lines, stats.comment_lines));
        }

        for (i, lang_name) in group.languages.iter().enumerate() {
            let content_info = group.per_language.get(lang_name).unwrap();
            if grouped || i > 0 {
                lines.push(String::new());
            }

            lines.push(columns.format_files_row(theme, content_info.files,
                    &format_size(theme, content_info.bytes, content_info.calculate_average_size()), block_width));
            lines.push(columns.format_breakdown_row(theme, &(indent.to_owned() + &theme.details_language_name.paint(lang_name).to_string()),
                    lang_name.chars().count() + indent.len(), content_info.lines, content_info.code_lines, content_info.comment_lines));
            if should_print_keywords {
                let keywords = get_keywords_as_str(theme, &content_info.keyword_occurences, None, columns.calculate_words_start(), block_width);
                if !keywords.is_empty() {
                    lines.push(keywords);
                }
            }
        }
    }

    lines
}

// Every column is right aligned to a shared edge. The file count and the line count end at the same
// place, so the line count, being the longer of the two and the criterion the list is sorted by,
// reaches further left and is the first thing the eye lands on.
struct Columns {
    name: usize,
    headline: usize,
    code: usize,
    comments: usize,
    extra: usize
}

impl Columns {
    fn of(groups: &[Group], total: &Stats) -> Self
    {
        let grouped = is_grouped(groups);
        let indent = if grouped {LIST_INDENT.len()} else {0};
        let len_of = |value: usize| format_with_separators(value).len();
        let mut columns = Columns {
            name: TOTAL_NAME.len(),
            headline: len_of(total.files).max(len_of(total.lines)),
            code: len_of(total.code_lines),
            comments: len_of(total.comment_lines),
            extra: len_of(total.calculate_extra_lines())
        };

        // The total holds the largest of every column, except when --top hid the language that made
        // it so, which is why the shown ones are measured too instead of assumed smaller
        for group in groups {
            // The leftovers print their name too, so it has to be measured too: a name wider than
            // the column it sits in makes the padding of its row a subtraction below zero.
            if grouped {
                columns.name = columns.name.max(group.get_displayed_name().chars().count());
            }
            for name in &group.languages {
                let content_info = group.per_language.get(name).unwrap();
                columns.name = columns.name.max(name.chars().count() + indent);
                columns.headline = columns.headline.max(len_of(group.per_language.get(name).unwrap().files))
                        .max(len_of(content_info.lines));
                columns.code = columns.code.max(len_of(content_info.code_lines));
                columns.comments = columns.comments.max(len_of(content_info.comment_lines));
                columns.extra = columns.extra.max(len_of(content_info.lines - content_info.code_lines - content_info.comment_lines));
            }
        }

        columns
    }

    // Where the file count and the line count both end
    fn calculate_headline_end(&self) -> usize {
        self.name + 2 * NAME_GAP + 2 + self.headline
    }

    // Where the words 'files' and 'lines' start, and with them the keywords row
    fn calculate_words_start(&self) -> usize {
        self.calculate_headline_end() + 1
    }

    // The theme arrives as an argument rather than being read from 'super::theme::get_active()', because
    // '--show-themes' renders one sample per theme it found, in a single run, and it renders it
    // through these same functions so that the preview cannot drift from the real output
    fn format_breakdown_row(&self, theme: &Theme, painted_name: &str, name_len: usize, lines: usize, code_lines: usize, comment_lines: usize) -> String {
        let (code_percentage, comment_percentage) = calculate_code_and_comment_percentages(lines,code_lines, comment_lines);
        format!("{}{}{}{}{:>headline_w$} {} {{ {:>code_w$} {} ({})  +  {:>comments_w$} {} ({})  +  {:>extra_w$} {} }}",
                painted_name, " ".repeat(self.name - name_len + NAME_GAP), theme.arrow.paint("->"), " ".repeat(NAME_GAP),
                theme.lines_number.paint(&format_with_separators(lines)), theme.lines_label.paint("lines"),
                theme.code_number.paint(&format_with_separators(code_lines)), theme.code_label.paint("code"), paint_percent(theme,code_percentage),
                theme.comments_number.paint(&format_with_separators(comment_lines)), theme.comments_label.paint("comments"), paint_percent(theme,comment_percentage),
                theme.extra_number.paint(&format_with_separators(lines - code_lines - comment_lines)), theme.extra_label.paint("extra"),
                headline_w = self.headline, code_w = self.code, comments_w = self.comments, extra_w = self.extra)
    }

    // The size text ends where the row below it does
    fn format_files_row(&self, theme: &Theme, files: usize, size_text: &str, width: usize) -> String {
        let left = format!("{}{:>headline_w$} {}", " ".repeat(self.calculate_headline_end() - self.headline),
                theme.files_number.paint(&format_with_separators(files)), theme.files_label.paint("files"), headline_w = self.headline);
        let used = calculate_widest_visible_line(&left) + calculate_widest_visible_line(size_text);

        left + &" ".repeat(width.saturating_sub(used).max(2)) + size_text
    }

    // Rendered once to be measured and again to be printed: once per run costs nothing, and the
    // width cannot fall behind the row the way a formula would
    fn width(&self, theme: &Theme) -> usize {
        calculate_widest_visible_line(&self.format_breakdown_row(theme, "", 0, 0, 0, 0))
    }
}

fn print_sum(theme: &Theme, per_language: &HashMap<String,Stats>, total: &Stats, columns: &Columns,
        block_width: usize, should_print_keywords: bool)
{
    print_lines(&format_sum_lines(theme, per_language, total, columns, block_width, should_print_keywords));
}

fn format_sum_lines(theme: &Theme, per_language: &HashMap<String,Stats>, total: &Stats, columns: &Columns,
        block_width: usize, should_print_keywords: bool) -> Vec<String>
{
    // The separator spans the block, which every row of the details section already fits exactly
    let mut lines = vec![
        format!("{} ",theme.separator.paint(&"-".repeat(block_width))),
        columns.format_files_row(theme, total.files,
                &format_size(theme, total.bytes, total.calculate_average_size()), block_width),
        columns.format_breakdown_row(theme, &theme.details_total.paint(TOTAL_NAME).to_string(),
                TOTAL_NAME.len(), total.lines, total.code_lines, total.comment_lines)];

    if should_print_keywords {
        let keywords_line = get_keywords_as_str(theme, &create_keyword_sum_map(per_language), None, columns.calculate_words_start(), block_width);
        if !keywords_line.is_empty() {
            lines.push(keywords_line);
        }
    }
    lines.push(String::new());

    lines
}

// Their own block instead of a trailing column, whose width varies by nature and would destroy the
// alignment a table exists for. Not aligned by position either: a column of the table means one
// thing all the way down, while the first keyword of one language and the first of the next are
// unrelated, so aligning them promises a comparison that does not exist.
fn print_keyword_block(theme: &Theme, groups: &[Group]) {
    let lines = format_keyword_block_lines(theme, groups);
    if !lines.is_empty() {
        println!("
{}.
", theme.heading.paint("Keywords"));
        for line in lines {
            println!("{line}");
        }
    }
}

// Nested the way the table is, because ungrouped keywords under a grouped table cannot be read:
// 'Rust structs: 210' with no way to tell whose they are. A language appears only under the modules
// it is in, which keeps the block from growing by the product of the two.
fn format_keyword_block_lines(theme: &Theme, groups: &[Group]) -> Vec<String> {
    const GAP : usize = 3;

    let grouped = is_grouped(groups);
    let rows = groups.iter().map(|group| (group, group.languages.iter().filter_map(|name| {
            // A language that only the baseline had has no row here at all, having no keywords now
            let was = group.baseline.and_then(|x| x.get(name)).map(|x| &x.keyword_occurences);
            let occurrences = &group.per_language.get(name).unwrap().keyword_occurences;
            // Nothing but zeros says only that the language declares keywords this code never
            // wrote. A zero that moved keeps its row, the movement being the whole point of it.
            if occurrences.values().all(|x| *x == 0) && was.is_none_or(|x| x.values().all(|y| *y == 0)) {
                return None;
            }
            let keywords = get_keywords_as_str(theme, occurrences, was, 0, usize::MAX);
            if keywords.is_empty() {None} else {Some((name, keywords))}
        }).collect::<Vec<_>>())).filter(|(_, rows)| !rows.is_empty()).collect::<Vec<_>>();

    if rows.is_empty() {
        return Vec::new();
    }

    let indent = if grouped {GROUP_INDENT.len()} else {0};
    let language_width = rows.iter().flat_map(|(_, rows)| rows.iter())
            .map(|(name,_)| name.chars().count()).max().unwrap();
    let mut lines = Vec::with_capacity(rows.len() * 3);
    for (group, keyword_rows) in rows {
        if grouped {
            lines.push(theme.details_module.paint(group.get_displayed_name()).to_string());
        }
        lines.extend(keyword_rows.into_iter().map(|(name, keywords)| format!("{}{}{}{}", " ".repeat(indent),
                theme.details_language_name.paint(name),
                " ".repeat(language_width - name.chars().count() + GAP), keywords)));
    }

    lines
}

// Indented to where the word 'lines' starts on the row above, and wrapped to the width of the block
// so that a language with many keywords cannot push the section wider than every other row
// 'baseline' is the same keywords as an earlier reading counted them, and turns every entry that
// moved into 'structs: 60 (+5)'. Only the ones that moved are marked: a comma separated sentence with
// a dash on every entry is harder to read than the sentence, which is the opposite of the table
// above, where a column of dashes is what lets the eye skip to the rows that are not one.
fn get_keywords_as_str(theme: &Theme, keyword_occurencies: &HashMap<String,usize>,
        baseline: Option<&HashMap<String,usize>>, indent: usize, width: usize) -> String
{
    const SEPARATOR : &str = ", ";

    if keyword_occurencies.is_empty() {
        return String::new();
    }

    let mut keyword_info = " ".repeat(indent);
    let mut used = indent;
    for (position, (name, count)) in create_keyword_entries(keyword_occurencies).into_iter().enumerate() {
        let moved = baseline.map(|baseline| (baseline.get(&name).copied().unwrap_or(0), keyword_occurencies[&name]))
                .filter(|(was, is)| was != is)
                .map(|(was, is)| format!(" ({})", paint_change(theme, was, is, &format_signed_difference(was, is))));
        let change = moved.unwrap_or_default();
        let entry_len = name.chars().count() + 2 + count.chars().count() + calculate_widest_visible_line(&change);
        let entry = format!("{}: {}{change}", theme.keyword_label.paint(&name), theme.keyword_number.paint(&count));

        if position > 0 {
            // The comma stays on the line it ends, the way a sentence breaks
            if used + SEPARATOR.len() + entry_len > width {
                keyword_info.push_str(",\n");
                keyword_info.push_str(&" ".repeat(indent));
                used = indent;
            } else {
                keyword_info.push_str(SEPARATOR);
                used += SEPARATOR.len();
            }
        }

        keyword_info.push_str(&entry);
        used += entry_len;
    }

    keyword_info
}

// Ordered by name, so that a keyword stays in the same place down a report and between two runs
fn create_keyword_entries(keyword_occurencies: &HashMap<String,usize>) -> Vec<(String, String)> {
    let mut sorted_keywords = keyword_occurencies.iter().collect::<Vec<_>>();
    sorted_keywords.sort_unstable_by_key(|(name,_)| name.as_str());

    sorted_keywords.into_iter().map(|(name, occurancies)| (name.to_owned(), format_with_separators(*occurancies))).collect()
}

fn create_keyword_sum_map(per_language: &HashMap<String,Stats>) -> HashMap<String,usize> {
    let mut collective_keywords_map : HashMap<String,usize> = HashMap::new();
    for content_info in per_language.values() {
        for keyword in &content_info.keyword_occurences {
            if *keyword.1 == 0 {continue;}
            if let Some(x) = collective_keywords_map.get_mut(keyword.0) {
                *x += *keyword.1;
            } else {
                collective_keywords_map.insert(keyword.0.to_owned(), *keyword.1);
            }
        }
    }

    collective_keywords_map
}

//                                    OVERVIEW
//
// Files:    47% java - 32% cs - 21% py        [-||||||||||||||||||||||||||||||||||||||||||||||||||]
//
// Lines: ...
//
// Size : ...
fn print_visual_overview(sorted_language_names: &[String], per_language: &HashMap<String, Stats>, total: &Stats, config: &Configuration)
{
    print_lines(&format_overview_lines(sorted_language_names, per_language, total, config));
}

fn format_overview_lines(sorted_language_names: &[String], per_language: &HashMap<String, Stats>, total: &Stats, config: &Configuration) -> Vec<String>
{
    // The function itself decides whether there is anything to fold
    let (sorted_language_vec, per_language) =
            fold_rest_into_others(sorted_language_names, per_language, total, config.view.top_n);
    let (sorted_language_vec, per_language) =
            (&sorted_language_vec, &per_language);

    // 'others' takes its style by identity and not by position, because --top moves it: with
    // --top 2 it sits third and used to steal the slot meant for the third language.
    let slots = super::theme::get_active().get_language_slots();
    let styles = sorted_language_vec.iter().enumerate()
            .map(|(i, name)| if name == OTHERS_NAME {slots[slots.len()-1]} else {slots[i.min(slots.len()-2)]}.clone())
            .collect::<Vec<_>>();
    let color_func_vec : Vec<ColorFunc> = styles.iter().cloned()
            .map(|style| Box::new(move |s: &str| style.paint(s).to_string()) as ColorFunc).collect();
    // A bar cell takes the color of the slot and none of its attributes: bold or underline on a
    // block character is not something a terminal shows usefully
    let bar_func_vec : Vec<ColorFunc> = styles.iter().map(|style| {
            let color = style.get_color();
            Box::new(move |s: &str| match color {
                Some(color) => s.color(color).to_string(),
                None => s.to_owned()
            }) as ColorFunc
        }).collect();

    let files_percentages = get_files_percentages(per_language, sorted_language_vec);
    let lines_percentages = get_lines_percentages(per_language, sorted_language_vec);
    let sizes_percentages = get_sizes_percentages(per_language, sorted_language_vec);

    let files_verticals = if config.view.hidden.bar {vec![]} else{render::apportion(&files_percentages, NUM_OF_VERTICALS)};
    let lines_verticals = if config.view.hidden.bar {vec![]} else{render::apportion(&lines_percentages, NUM_OF_VERTICALS)};
    let size_verticals = if config.view.hidden.bar {vec![]} else{render::apportion(&sizes_percentages, NUM_OF_VERTICALS)};

    // Each percentage is padded to the widest of the three rows in its own position, so the same
    // language stays in the same column down the section without paying for a gap nobody needs
    let percent_widths = (0..sorted_language_vec.len()).map(|i| {
        format_percent_text(files_percentages[i]).len().max(format_percent_text(lines_percentages[i]).len())
                .max(format_percent_text(sizes_percentages[i]).len())
    }).collect::<Vec<_>>();

    let files_line = create_overview_line("Files:", &files_percentages, &files_verticals,
            sorted_language_vec, &color_func_vec, &bar_func_vec, &percent_widths, config);
    let lines_line = create_overview_line("Lines:", &lines_percentages, &lines_verticals,
            sorted_language_vec, &color_func_vec, &bar_func_vec, &percent_widths, config);
    let size_line = create_overview_line("Size :", &sizes_percentages, &size_verticals,
            sorted_language_vec, &color_func_vec, &bar_func_vec, &percent_widths, config);

    vec![format!("{}.", super::theme::get_active().heading.paint("Overview")), String::new(),
         files_line, String::new(), lines_line, String::new(), size_line, String::new()]
}

fn create_overview_line(prefix: &str, percentages: &[f64], verticals: &[usize], languages_name: &[String],
        color_func_vec: &[ColorFunc], bar_func_vec: &[ColorFunc], percent_widths: &[usize], config: &Configuration) -> String
{
    let theme = super::theme::get_active();
    let mut line = String::with_capacity(150);
    line.push_str(&format!("{}   ", theme.overview_label.paint(prefix)));
    for (i,percentage) in percentages.iter().enumerate() {
        let str_perc = format_percent_text(*percentage);
        line.push_str(&format!("{}{} ", " ".repeat(percent_widths[i].saturating_sub(str_perc.len())), paint_overview_percent(theme,*percentage)));
        line.push_str(&color_func_vec[i](&languages_name[i]));
        if i < percentages.len() - 1{
            line.push_str(" - ")
        }
    }

    if !config.view.hidden.bar {
        add_verticals_str(&mut line, verticals, bar_func_vec, config.view.bar_thickness.get_character());
    }

    line
}

fn add_verticals_str(line: &mut String, files_verticals: &[usize], color_func_vec: &[ColorFunc], character: &str) {
    let theme = super::theme::get_active();
    line.push_str("   ");
    line.push_str(&theme.bar_frame.paint("[-").to_string());
    for (i,verticals) in files_verticals.iter().enumerate() {
        line.push_str(&color_func_vec[i](character).repeat(*verticals));
    }
    line.push_str(&theme.bar_frame.paint("-]").to_string());
}

// Returns its own view of the data rather than folding the caller's maps in place: "others" is a
// creature of the overview and of nothing else, and a result that has been printed once has to
// still be the result.
fn fold_rest_into_others(sorted_language_names: &[String],
        per_language: &HashMap<String, Stats>, total: &Stats, top_n: Option<usize>)
-> (Vec<String>, HashMap<String, Stats>)
{
    // --top never widens the overview past its own cap, it only narrows it, so that asking for the
    // top 2 does not leave three languages sitting in the bar
    let to_keep = OVERVIEW_LANGUAGES.min(top_n.unwrap_or(OVERVIEW_LANGUAGES));
    if sorted_language_names.len() <= to_keep + 1 {
        return (sorted_language_names.to_vec(), per_language.clone());
    }

    let mut sorted_language_names = sorted_language_names[..to_keep].to_vec();
    sorted_language_names.push(OTHERS_NAME.to_owned());
    let mut per_language = per_language.clone();
    per_language.retain(|name, _| sorted_language_names.contains(name));

    // Whatever the kept ones do not account for, which is what keeps the shares of the overview
    // shares of the whole run and not of the few it draws
    let mut others = Stats::default();
    let shown = Stats::total_of(&per_language);
    others.files = total.files - shown.files;
    others.bytes = total.bytes - shown.bytes;
    others.lines = total.lines - shown.lines;
    per_language.insert(OTHERS_NAME.to_string(), others);

    (sorted_language_names, per_language)
}

fn get_files_percentages(per_language: &HashMap<String,Stats>, sorted_language_names: &[String]) -> Vec<f64> {
    let mut language_files = [0].repeat(per_language.len());
    per_language.iter().for_each(|e| {
        let pos = sorted_language_names.iter().position(|name| name == e.0).unwrap();
        language_files[pos] = e.1.files;
    });

    render::calculate_percentages_of_their_own_sum(&language_files)
}

fn get_lines_percentages(per_language: &HashMap<String,Stats>, languages_name: &[String]) -> Vec<f64> {
    let mut language_lines = [0].repeat(per_language.len());
    per_language.iter().for_each(|e| {
        let pos = languages_name.iter().position(|name| name == e.0).unwrap();
        language_lines[pos] = e.1.lines;
    });

    render::calculate_percentages_of_their_own_sum(&language_lines)
}

fn get_sizes_percentages(per_language: &HashMap<String,Stats>, languages_name: &[String]) -> Vec<f64> {
    let mut language_size = [0].repeat(per_language.len());
    per_language.iter().for_each(|e| {
        let pos = languages_name.iter().position(|name| name == e.0).unwrap();
        language_size[pos] = e.1.bytes;
    });

    render::calculate_percentages_of_their_own_sum(&language_size)
}

// The settings this run counted with, against the ones the entry recorded. A setting the entry never
// wrote is left alone rather than reported as changed, which is what keeps entries from older
// versions from being accused of a difference nobody can know about.
fn find_settings_changed_since(entry: &super::log::LogEntry, config: &Configuration,
        targets: &[mezura_core::Target]) -> Vec<&'static str>
{
    let sort = |targets: &[mezura_core::Target]| {
        let mut sorted = targets.to_vec();
        sorted.sort();
        sorted
    };

    let mut changed = Vec::new();
    // The scope cannot carry the directories, so they are compared beside it: the same './src'
    // declared over two different trees is two different measurements
    if sort(&entry.targets) != sort(targets) {
        changed.push(config_manager::DIRS);
    }
    // The log holds no keyword counts, so a run that only stopped counting them changed nothing
    // the log records
    changed.extend(super::diff::find_settings_that_differ(&entry.scope, &super::diff::scope_of(&config.engine))
            .into_iter().filter(|setting| *setting != super::diff::HIDE_KEYWORDS));

    changed
}

// At the end of the line of the entry it belongs to, since it is a statement about that entry and
// not about the run. Nothing is printed when nothing changed, or the reader stops seeing it.
fn format_modified_tag(changed: &[&'static str]) -> String {
    if changed.is_empty() {
        return String::new();
    }

    let theme = super::theme::get_active();
    format!("   {} {}", theme.history_modified.paint("modified:"),
            theme.history_modified_field.paint(&changed.join(", ")))
}

// One line per module under the line of the entry, and narrower than it: Files and Extra stay on
// the total, since what is asked of a module is which part of it moved. With every column repeated,
// one entry is five wide lines and '--compare 3' stops being readable.
fn format_module_comparison_lines(entry: &super::log::LogEntry, groups: &[Group]) -> String {
    let theme = super::theme::get_active();
    let names = groups.iter().map(|x| x.get_displayed_name().to_owned())
            .chain(entry.modules.iter().map(|x| x.name.clone())
                    .filter(|name| !groups.iter().any(|x| x.get_displayed_name() == name)))
            .collect::<Vec<_>>();
    let width = names.iter().map(|x| x.chars().count()).max().unwrap_or(0);

    // Right aligned down the entry, since the whole reason these are three narrow columns and not
    // the full breakdown is that they are meant to be read down rather than across
    let compared = names.iter().filter_map(|name| entry.modules.iter().find(|x| &x.name == name)).collect::<Vec<_>>();
    let number_width = |value: fn(&super::log::ModuleEntry) -> usize|
            compared.iter().map(|x| format_with_separators(value(x)).len()).max().unwrap_or(0);
    let (lines_width, code_width, comments_width) =
            (number_width(|x| x.lines), number_width(|x| x.code_lines), number_width(|x| x.comment_lines));

    let mut rendered = String::with_capacity(names.len() * 80);
    for name in &names {
        let padded = format!("       {}{}   ", theme.details_module.paint(name),
                " ".repeat(width - name.chars().count()));
        let now = groups.iter().find(|x| x.get_displayed_name() == name).map(|x| x.total);
        let then = entry.modules.iter().find(|x| &x.name == name);
        // A module compared against nothing would read '+100%', which is false: it did not grow, it
        // started being counted on its own. The ones that are not in both are named as what they are.
        let tail = match (now, then) {
            (Some(now), Some(then)) => {
                let cell = |style: &super::theme::Style, value: usize, then: usize, width: usize| {
                    let text = format_with_separators(then);
                    format!("{}{}({}%)", " ".repeat(width - text.len()), style.paint(&text),
                            paint_percentage(&format_signed_percentage_difference(then, value)))
                };
                format!("Lines: {}   Code: {}   Comments: {}",
                        cell(&theme.lines_number, now.lines, then.lines, lines_width),
                        cell(&theme.code_number, now.code_lines, then.code_lines, code_width),
                        cell(&theme.comments_number, now.comment_lines, then.comment_lines, comments_width))
            },
            (Some(_), None) => theme.note.paint("declared in this run, nothing to compare against").to_string(),
            _ => theme.note.paint("not counted any more").to_string()
        };
        rendered.push_str(&format!("{padded}{tail}\n"));
    }

    rendered
}

fn paint_percentage(percentage: &str) -> ColoredString {
    let theme = super::theme::get_active();
    if percentage.starts_with('+') {
        theme.change_up.paint(percentage)
    } else if percentage.starts_with('-') {
        theme.change_down.paint(percentage)
    } else {
        theme.change_same.paint(percentage)
    }
}

fn print_comparison_to_previous_runs(result: &RunResult, log_content: &str, config: &Configuration, datetime_now: &DateTime<Local>) {
    println!("\n{}.\n", super::theme::get_active().heading.paint("History"));

    let total = &result.total;
    let log_entries = super::log::read_last_entries(log_content, config.view.compare_level);
    // Silent until used: a run that named no module says nothing about them here either, and the
    // 'modified: dirs' tag is what already reports that the targets are not the ones they were
    let groups = if result.has_modules() {create_groups_of(result, config)} else {Vec::new()};

    let mut comparison_str = String::with_capacity(200);
    for entry in log_entries.iter() {
        let duration = datetime_now.signed_duration_since(entry.datetime);
        let (days, hours, minutes) = split_minutes_to_D_H_M(duration.num_minutes());
        let arrow = super::theme::get_active().history_entry.paint("->");
        let tag = format_modified_tag(&find_settings_changed_since(entry, config, &result.targets));
        if let Some(name) = &entry.name {
            comparison_str.push_str(&format!("{} \"{}\" ({} days, {} hours and {} minutes ago){}\n",
                    arrow, name, days, hours, minutes, tag));
        } else {
            let then_str = entry.datetime.naive_local().to_string();
            comparison_str.push_str(&format!("{} {} ({} days, {} hours and {} minutes ago){}\n",
                    arrow, then_str, days, hours, minutes, tag));
        }
        comparison_str.push_str(&format!("     Files: {}({}%) Lines: {}({}%) {{Code: {}({}%), Comments: {}({}%), Extra: {}({}%)}}\n",
                super::theme::get_active().files_number.paint(&format_with_separators(entry.total.files)), paint_percentage(&format_signed_percentage_difference(entry.total.files, total.files)),
                super::theme::get_active().lines_number.paint(&format_with_separators(entry.total.lines)), paint_percentage(&format_signed_percentage_difference(entry.total.lines, total.lines)),
                super::theme::get_active().code_number.paint(&format_with_separators(entry.total.code_lines)), paint_percentage(&format_signed_percentage_difference(entry.total.code_lines, total.code_lines)),
                super::theme::get_active().comments_number.paint(&format_with_separators(entry.total.comment_lines)), paint_percentage(&format_signed_percentage_difference(entry.total.comment_lines, total.comment_lines)),
                super::theme::get_active().extra_number.paint(&format_with_separators(entry.total.calculate_extra_lines())), paint_percentage(&format_signed_percentage_difference(entry.total.calculate_extra_lines(), total.calculate_extra_lines()))));
        if !groups.is_empty() {
            comparison_str.push_str(&format_module_comparison_lines(entry, &groups));
        }
        comparison_str.push('\n');
    }
    print!("{comparison_str}");
}

fn split_minutes_to_D_H_M(mut minutes: i64) -> (i64, i64, i64) {
    let minutes_in_day = 60 * 24;
    let minutes_in_hour = 60;
    let days = minutes / minutes_in_day;
    minutes -= days * minutes_in_day;
    let hours = minutes / minutes_in_hour;
    minutes -= hours * minutes_in_hour;

    (days, hours, minutes)
}

fn format_signed_percentage_difference(older: usize, newer: usize) -> String {
    super::number_formatter::get_active().signed_percent(render::calculate_relative_change(older, newer))
}

fn print_lines(lines: &[String]) {
    for line in lines {
        println!("{line}");
    }
}

fn calculate_widest_visible_line(text: &str) -> usize {
    text.lines().map(crate::theme::calculate_visible_len).max().unwrap_or(0)
}

fn format_size(theme: &Theme, total_bytes: usize, average_bytes: usize) -> String {
    let (total, total_unit) = super::number_formatter::get_active().size_with_unit(total_bytes);
    let (average, average_unit) = super::number_formatter::get_active().size_with_unit(average_bytes);
    format!("{} {} {} - {} {} {}",
            theme.total_size_number.paint(&total),
            theme.size_unit.paint(total_unit), theme.total_size_label.paint("total"),
            theme.avg_size_number.paint(&average),
            theme.size_unit.paint(average_unit), theme.avg_size_label.paint("average"))
}

fn format_percent_text(value: f64) -> String {
    super::number_formatter::get_active().percent(value)
}

// The '%' is painted with the number, or it keeps the default color while the digits fade
fn paint_percent(theme: &Theme, value: f64) -> ColoredString {
    theme.percent.paint(&(format_percent_text(value) + "%"))
}

// The overview's percentages are the datum of that section rather than an annotation on a count,
// so they take a token of their own
fn paint_overview_percent(theme: &Theme, value: f64) -> ColoredString {
    theme.overview_percent.paint(&(format_percent_text(value) + "%"))
}

fn calculate_code_and_comment_percentages(lines: usize, code_lines: usize, comment_lines: usize) -> (f64, f64) {
    if lines == 0 {
        return (0f64, 0f64);
    }
    (code_lines as f64 / lines as f64 * 100f64, comment_lines as f64 / lines as f64 * 100f64)
}

#[cfg(test)]
mod tests {
    use mezura_core::{FilesPresent, ModuleResult};

    use super::*;

    // One dataset for every layout, chosen so that the things that break are all present at once: a
    // long language name next to a short one, figures wide enough to move the shared right edge, a
    // keyword row long enough to wrap, a language with no keywords at all, and five languages, which
    // is one more than the overview can show without folding into "others".
    fn sample_data() -> (Vec<String>, HashMap<String, Stats>, Stats) {
        let per_language = hashmap![
            "Rust".to_owned() => Stats::new(13, 416800, 9008, 6122, 505,
                    hashmap!["enums".to_owned() => 11, "structs".to_owned() => 29, "traits".to_owned() => 1]),
            "JavaScript".to_owned() => Stats::new(4, 40000, 1200, 900, 120,
                    hashmap!["classes".to_owned() => 805, "functions".to_owned() => 1204, "generators".to_owned() => 17,
                             "promises".to_owned() => 96, "imports".to_owned() => 342]),
            "HTML".to_owned() => Stats::new(2, 18800, 396, 361, 0, hashmap![]),
            "Python".to_owned() => Stats::new(3, 9000, 250, 200, 20, hashmap!["classes".to_owned() => 2]),
            "Java".to_owned() => Stats::new(1, 900, 80, 60, 5,
                    hashmap!["classes".to_owned() => 2, "interfaces".to_owned() => 1])];
        let total = Stats::total_of(&per_language);
        let sorted = get_sorted_language_names(&per_language, SortCriterion::Lines);

        (sorted, per_language, total)
    }

    // The same five languages, split into the shape a run with modules produces: two named ones and
    // the leftovers. The totals are unchanged by construction, so any difference between the grouped
    // cases and the ungrouped ones below is the grouping and nothing else.
    fn sample_modules() -> Vec<ModuleResult> {
        let (_, content_info, _) = sample_data();
        let of = |name: Option<&str>, languages: &[&str]| {
            let per_language = languages.iter().map(|x| ((*x).to_owned(), content_info[*x].clone())).collect::<HashMap<_,_>>();
            let total = Stats::total_of(&per_language);
            ModuleResult {name: name.map(str::to_owned), per_language, total, embedded: Default::default()}
        };

        vec![of(Some("frontend"), &["JavaScript", "HTML"]), of(Some("backend"), &["Rust"]),
             of(None, &["Python", "Java"])]
    }

    // The same five languages as an earlier reading counted them, split the same way, so that every
    // shape a comparison has to draw is in one dataset: JavaScript shrank, Rust grew, HTML did not
    // move at all, Python is only there now and Go is only there before. Declared in another order
    // than the later reading, since the order of the rows is the later one's.
    fn earlier_modules() -> Vec<ModuleResult> {
        let of = |name: Option<&str>, languages: Vec<(&str, Stats)>| {
            let per_language = languages.into_iter().map(|(x, stats)| (x.to_owned(), stats)).collect::<HashMap<_,_>>();
            ModuleResult {name: name.map(str::to_owned), total: Stats::total_of(&per_language), per_language,
                    embedded: Default::default()}
        };

        vec![of(Some("backend"), vec![
                ("Rust", Stats::new(11, 380000, 8104, 5510, 470,
                        hashmap!["enums".to_owned() => 9, "structs".to_owned() => 24, "traits".to_owned() => 1]))]),
             of(Some("frontend"), vec![
                ("JavaScript", Stats::new(5, 52000, 1500, 1150, 140,
                        hashmap!["classes".to_owned() => 900, "functions".to_owned() => 1204, "generators".to_owned() => 17,
                                 "promises".to_owned() => 96, "imports".to_owned() => 400])),
                ("HTML", Stats::new(2, 18800, 396, 361, 0, hashmap![]))]),
             of(None, vec![
                ("Go", Stats::new(2, 7000, 210, 170, 12, hashmap!["structs".to_owned() => 4])),
                ("Java", Stats::new(1, 900, 80, 60, 5,
                        hashmap!["classes".to_owned() => 2, "interfaces".to_owned() => 1]))])]
    }

    fn merged(modules: &[ModuleResult]) -> HashMap<String, Stats> {
        let mut per_language : HashMap<String, Stats> = HashMap::new();
        for module in modules {
            for (language, stats) in &module.per_language {
                per_language.entry(language.clone()).or_default().add(stats);
            }
        }

        per_language
    }

    // The same counts as a run that named nothing produces: one module holding everything
    fn without_modules(modules: &[ModuleResult]) -> Vec<ModuleResult> {
        let per_language = merged(modules);

        vec![ModuleResult {name: None, total: Stats::total_of(&per_language), per_language, embedded: Default::default()}]
    }

    fn reading_of(name: &str, taken: &str, modules: Vec<ModuleResult>) -> crate::diff::Reading {
        let per_language = merged(&modules);
        let total = Stats::total_of(&per_language);
        let files_present = FilesPresent {total_files: total.files, relevant_files: total.files, excluded_files: 0};

        crate::diff::Reading {
            source: crate::diff::Source::Document {path: name.to_owned()},
            taken: taken.to_owned(),
            version: "3.0.0".to_owned(),
            scope: crate::diff::scope_of(&mezura_core::EngineConfig::default()),
            warnings: Vec::new(),
            faulty_files_count: 0,
            unreadable_dirs_count: 0,
            result: RunResult {total, per_language, modules, embedded: Default::default(),
                    faulty_files: Vec::new(), files_present, targets: Vec::new(),
                    unreadable_dirs: Vec::new(),
                    performance: mezura_core::Performance {duration_millis: 0, threads: mezura_core::Threads::new(1, 1)}}
        }
    }

    fn groups_from<'a>(modules: &'a [ModuleResult], config: &crate::config_manager::Configuration) -> Vec<Group<'a>> {
        let result = RunResult {per_language: HashMap::new(),
                modules: Vec::new(), embedded: Default::default(), total: Stats::default(), faulty_files: Vec::new(),
                files_present: FilesPresent::default(), targets: Vec::new(), unreadable_dirs: Vec::new(), performance: mezura_core::Performance { duration_millis: 0, threads: mezura_core::Threads::new(1, 1) }};
        let mut result = result;
        result.modules = modules.iter().map(|x| ModuleResult {
            name: x.name.clone(),
            per_language: x.per_language.clone(),
            total: Stats::total_of(&x.per_language),
            embedded: Default::default()
        }).collect();
        // The borrow has to outlive the temporary, so the groups are built against the caller's slice
        let order = create_groups_of(&result, config).into_iter().map(|x| (x.name.map(str::to_owned), x.languages, x.hidden))
                .collect::<Vec<_>>();

        order.into_iter().map(|(name, languages, hidden)| {
            let module = modules.iter().find(|x| x.name == name).unwrap();
            Group {name: module.name.as_deref(), languages, hidden, per_language: &module.per_language, total: &module.total, baseline: None}
        }).collect()
    }

    fn render_every_layout() -> String {
        // Not left to the absence of a terminal: CLICOLOR_FORCE overrides that, and the verification
        // protocol in CLAUDE.md tells the reader to export it, so the same shell that ran a manual
        // comparison would otherwise fail this test with a wall of escape codes
        colored::control::set_override(false);

        let (sorted, content_info, total) = sample_data();
        let theme = &Theme::default();
        let mut config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);
        let plain = vec![Group {name: None, languages: sorted.clone(), hidden: 0, per_language: &content_info, total: &total, baseline: None}];
        let columns = Columns::of(&plain, &total);
        let width = columns.width(theme);

        let mut cases: Vec<(String, Vec<String>)> = Vec::new();
        let mut list = format_individual_lines(theme, &plain, &columns, width, true);
        list.extend(format_sum_lines(theme, &content_info, &total, &columns, width, true));
        cases.push(("list".to_owned(), list));
        cases.push(("list, keywords hidden".to_owned(),
                format_individual_lines(theme, &plain, &columns, width, false)));

        let mut table = format_table_lines(theme, &plain, &total, true);
        table.extend(format_keyword_block_lines(theme, &plain));
        cases.push(("table".to_owned(), table));

        let mut boxed = format_boxed_lines(theme, &plain, &total, true);
        boxed.extend(format_keyword_block_lines(theme, &plain));
        cases.push(("boxed".to_owned(), boxed));

        cases.push(("overview".to_owned(), format_overview_lines(&sorted, &content_info, &total, &config)));

        config.view.top_n = Some(2);
        cases.push(("overview, top 2".to_owned(), format_overview_lines(&sorted, &content_info, &total, &config)));

        config.view.top_n = None;
        config.view.hidden.bar = true;
        cases.push(("overview, bar hidden".to_owned(), format_overview_lines(&sorted, &content_info, &total, &config)));

        // The same data with a second axis through it. Every layout groups, and the total under them
        // is the same total, which is what makes the two halves of this file comparable by eye.
        let mut config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);
        let modules = sample_modules();
        let groups = groups_from(&modules, &config);
        let columns = Columns::of(&groups, &total);
        let width = columns.width(theme);

        let mut list = format_individual_lines(theme, &groups, &columns, width, true);
        list.extend(format_sum_lines(theme, &content_info, &total, &columns, width, true));
        cases.push(("modules, list".to_owned(), list));

        let mut table = format_table_lines(theme, &groups, &total, true);
        table.extend(format_keyword_block_lines(theme, &groups));
        cases.push(("modules, table".to_owned(), table));

        let mut boxed = format_boxed_lines(theme, &groups, &total, true);
        boxed.extend(format_keyword_block_lines(theme, &groups));
        cases.push(("modules, boxed".to_owned(), boxed));

        // '--top' is per module, so it cuts inside each one and not across the report
        config.view.top_n = Some(1);
        let groups = groups_from(&modules, &config);
        cases.push(("modules, table, top 1".to_owned(), format_table_lines(theme, &groups, &total, true)));

        config.view.top_n = None;
        config.view.sort_by = SortCriterion::Name;
        let groups = groups_from(&modules, &config);
        cases.push(("modules, table, sorted by name".to_owned(), format_table_lines(theme, &groups, &total, true)));

        // The rows of the matrix are the languages of the whole run, and each of them is three
        // physical rows, so the second case is the one where a module does not have the language
        // at all and only the middle of the three carries a dash
        config.view.sort_by = SortCriterion::Lines;
        let groups = groups_from(&modules, &config);
        cases.push(("modules, matrix".to_owned(),
                format_matrix_lines(theme, &groups, &sorted, &total, true)));
        cases.push(("modules, matrix, top 2".to_owned(),
                format_matrix_lines(theme, &groups, &sorted[..2], &total, true)));
        cases.push(("modules, matrix, no total".to_owned(),
                format_matrix_lines(theme, &groups, &sorted[..1], &total, false)));

        // What '--diff' prints in place of everything above. The dates are fixed, being the one part
        // of the heading that a clock would otherwise write.
        const EARLIER : &str = "2026-07-30T14:22:07+03:00";
        const LATER   : &str = "2026-08-06T09:41:00+03:00";

        let mut config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);
        let earlier = earlier_modules();
        let (before, now) = (reading_of("older.json", EARLIER, without_modules(&earlier)),
                reading_of("newer.json", LATER, without_modules(&modules)));
        // The two lines 'print_comparison' puts above the rows, so that the heading is covered here
        // and not only where it is called from
        let headed = |lines: Vec<String>, before: &crate::diff::Reading, now: &crate::diff::Reading| {
            let mut headed = vec![format_comparison_heading(theme, before, now), String::new()];
            headed.extend(lines);
            headed
        };

        let rows = create_compared_rows(None, &before.result, &now.result, &config);
        let mut comparison = format_comparison_lines(theme,&rows);
        comparison.extend(format_keyword_block_lines(theme, &[create_group_with_baseline(None, &before.result.per_language,
                &now.result.per_language, &now.result.total, &config)]));
        cases.push(("comparison".to_owned(), headed(comparison, &before, &now)));
        cases.push(("comparison, boxed".to_owned(), headed(format_boxed_comparison_lines(theme,&rows), &before, &now)));

        // The same two readings with a second axis through them, which is shown because they named
        // the same modules
        let (before, now) = (reading_of("older.json", EARLIER, earlier),
                reading_of("newer.json", LATER, sample_modules()));
        let pairs = crate::diff::pair_modules(&before.result, &now.result).unwrap();
        let grouped_keywords = |config: &crate::config_manager::Configuration| pairs.iter()
                .map(|pair| create_group_with_baseline(pair.name, &pair.before.per_language, &pair.now.per_language,
                        &pair.now.total, config)).collect::<Vec<_>>();

        let rows = create_compared_rows(Some(&pairs), &before.result, &now.result, &config);
        let mut comparison = format_comparison_lines(theme,&rows);
        comparison.extend(format_keyword_block_lines(theme, &grouped_keywords(&config)));
        cases.push(("comparison, modules".to_owned(), headed(comparison, &before, &now)));

        let mut comparison = format_boxed_comparison_lines(theme,&rows);
        comparison.extend(format_keyword_block_lines(theme, &grouped_keywords(&config)));
        cases.push(("comparison, modules, boxed".to_owned(), headed(comparison, &before, &now)));

        // '--top' cuts inside each module here as it does everywhere else
        config.view.top_n = Some(1);
        cases.push(("comparison, modules, top 1".to_owned(), headed(format_comparison_lines(theme,
                &create_compared_rows(Some(&pairs), &before.result, &now.result, &config)), &before, &now)));

        let mut rendered = String::with_capacity(4000);
        for (name, lines) in cases {
            rendered.push_str(&format!("===> {name}\n"));
            for line in lines {
                rendered.push_str(line.trim_end());
                rendered.push('\n');
            }
            rendered.push('\n');
        }

        rendered
    }

    // The unnamed row is nine characters and 'Total', the width the column starts from, is five, so
    // a report whose names are all shorter leaves the row wider than the column it sits in. The
    // padding is then a subtraction below zero: a panic in debug and a broken line in release.
    #[test]
    fn the_leftovers_row_fits_the_column_even_when_every_other_name_is_shorter() {
        colored::control::set_override(false);

        let content_info = hashmap!["D".to_owned() => Stats::new(1, 24, 2, 2, 0, hashmap![])];
        let total = Stats::total_of(&content_info);
        fn group<'a>(name: Option<&'a str>, content_info: &'a HashMap<String, Stats>,
                total: &'a Stats) -> Group<'a> {
            Group {name, languages: vec!["D".to_owned()], hidden: 0, per_language: content_info, total, baseline: None}
        }
        let groups = vec![group(Some("a"), &content_info, &total),
                group(None, &content_info, &total)];

        let theme = &Theme::default();
        let columns = Columns::of(&groups, &total);
        assert!(columns.name >= UNNAMED_MODULE_NAME.len());

        let lines = format_individual_lines(theme, &groups, &columns, columns.width(theme), false);
        // and the arrow of every row still lands in the same column
        let arrow_at = |needle: &str| lines.iter().find(|x| x.starts_with(needle)).map(|x| x.find("->").unwrap());
        assert_eq!(arrow_at("a "), arrow_at(UNNAMED_MODULE_NAME));
        assert_eq!(arrow_at("a "), arrow_at(&(LIST_INDENT.to_owned() + "D")));
    }

    // Every language of the golden's data is in one module, so its keyword marks would come out the
    // same whether each module is measured against its own earlier counts or against every module's
    // added up. This is the case that tells the two apart: 'api' grew by ten and 'web' did not move,
    // and against the sum of thirty two neither would read as either.
    #[test]
    fn a_keyword_under_a_comparison_is_marked_against_the_module_it_is_in() {
        colored::control::set_override(false);

        let of = |name: &str, structs: usize| {
            let per_language = hashmap!["Rust".to_owned() =>
                    Stats::new(2, 4000, 100, 70, 10, hashmap!["structs".to_owned() => structs])];
            ModuleResult {name: Some(name.to_owned()), total: Stats::total_of(&per_language), per_language,
                    embedded: Default::default()}
        };
        let (before, now) = (reading_of("older.json", "2026-07-30T14:22:07+03:00", vec![of("api", 20), of("web", 12)]),
                reading_of("newer.json", "2026-08-06T09:41:00+03:00", vec![of("api", 30), of("web", 12)]));

        let config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);
        let pairs = crate::diff::pair_modules(&before.result, &now.result).unwrap();
        let groups = pairs.iter().map(|pair| create_group_with_baseline(pair.name, &pair.before.per_language,
                &pair.now.per_language, &pair.now.total, &config)).collect::<Vec<_>>();
        let lines = format_keyword_block_lines(&Theme::default(), &groups);

        assert_eq!(vec!["api", "  Rust   structs: 30 (+10)", "web", "  Rust   structs: 12"], lines);
    }

    // The golden calls each block function directly, so it says nothing about which list a block is
    // handed. That is what this holds, through the real entry point: the overview needs the uncut
    // list, since it folds the remainder into 'others' itself.
    #[test]
    fn every_layout_survives_a_top_that_hides_languages() {
        colored::control::set_override(false);

        let (_, content_info, _) = sample_data();
        let of_modules = |modules: Vec<ModuleResult>| RunResult {
            per_language: content_info.clone(), modules, embedded: Default::default(),
            total: Stats::new(23, 485500, 10934, 7643, 650, hashmap![]),
            faulty_files: Vec::new(), files_present: FilesPresent::default(), targets: Vec::new(), unreadable_dirs: Vec::new(), performance: mezura_core::Performance { duration_millis: 0, threads: mezura_core::Threads::new(1, 1) }};
        let single = || vec![ModuleResult {name: None, per_language: content_info.clone(),
                total: Stats::total_of(&content_info), embedded: Default::default()}];

        for layout in [Layout::List, Layout::Table, Layout::Boxed, Layout::Matrix] {
            // One past the five languages of the sample, so the boundary where nothing is hidden is
            // walked as well as the ones where almost everything is
            for top in 1..=6 {
                let mut config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);
                config.view.layout = layout;
                config.view.top_n = Some(top);

                format_and_print_results(&of_modules(single()), &None, &Local::now(), &config);
                format_and_print_results(&of_modules(sample_modules()), &None, &Local::now(), &config);
            }
        }
    }

    // The golden hands the blocks rows it built itself, so it says nothing about which rows the real
    // entry point decides to build. That is what this holds: whether the modules are shown at all is
    // decided when the comparison is assembled, and both answers have to survive every layout and
    // every '--top'.
    #[test]
    fn a_comparison_survives_every_layout_whether_or_not_the_modules_agree() {
        colored::control::set_override(false);

        let earlier = earlier_modules();
        for layout in [Layout::List, Layout::Table, Layout::Boxed, Layout::Matrix] {
            // One past the five languages of the sample, so the boundary where nothing is hidden is
            // walked as well as the ones where almost everything is
            for top in 1..=6 {
                let mut config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);
                config.view.layout = layout;
                config.view.top_n = Some(top);

                let now = || reading_of("newer.json", "2026-08-06T09:41:00+03:00", sample_modules());
                let agreeing = crate::diff::Comparison::of(
                        reading_of("older.json", "2026-07-30T14:22:07+03:00", earlier_modules()), now(), &config, Vec::new());
                let differing = crate::diff::Comparison::of(
                        reading_of("older.json", "2026-07-30T14:22:07+03:00", without_modules(&earlier)), now(), &config, Vec::new());
                // Through the real entry point, which is where the routing decision now lives
                crate::present::present(&agreeing.subject.result.clone(), Some(&agreeing), &config);
                crate::present::present(&differing.subject.result.clone(), Some(&differing), &config);
            }
        }
    }

    // The counting has its own golden in tests/stats_golden.rs; this one is the presentation. What
    // is locked is the shape: alignment, widths, the wrapping of the keyword rows, the folding into
    // "others" and the apportionment of the bar. Color is not, being turned off above.
    #[test]
    fn every_layout_matches_the_golden_file() {
        let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("layouts.golden");
        let rendered = render_every_layout();

        if std::env::var_os("MEZURA_UPDATE_GOLDEN").is_some() {
            std::fs::write(&golden, rendered.as_bytes()).unwrap();
            return;
        }

        let expected = std::fs::read_to_string(&golden).expect("run with MEZURA_UPDATE_GOLDEN=1 to create it");
        assert_eq!(expected.replace("\r\n", "\n"), rendered,
                "the printed layouts changed. Read the diff, and if every difference is intended, \
                 regenerate with MEZURA_UPDATE_GOLDEN=1 cargo test");
    }
    // The three read a different field of one map into the slot its language name occupies, and
    // that is the whole of what they do: the arithmetic on the figures is 'render::percentages' and
    // is asserted there. Each language is given three figures that would rank it differently, so a
    // function reading the wrong field, or filling by the map's own order, cannot pass.
    #[test]
    fn each_overview_row_reads_its_own_field_into_the_slot_of_its_language() {
        let content = hashmap!(
            "A".to_owned() => Stats::new(1, 60, 30, 0, 0, hashmap![]),
            "B".to_owned() => Stats::new(2, 30, 10, 0, 0, hashmap![]),
            "C".to_owned() => Stats::new(7, 10, 60, 0, 0, hashmap![]));
        let names = ["A".to_owned(), "B".to_owned(), "C".to_owned()];

        assert_eq!(vec![10.0, 20.0, 70.0], get_files_percentages(&content, &names));
        assert_eq!(vec![30.0, 10.0, 60.0], get_lines_percentages(&content, &names));
        assert_eq!(vec![60.0, 30.0, 10.0], get_sizes_percentages(&content, &names));

        // and the slot follows the name, so the sorted order the overview was given is the order it
        // draws in
        let reversed = ["C".to_owned(), "B".to_owned(), "A".to_owned()];
        assert_eq!(vec![70.0, 20.0, 10.0], get_files_percentages(&content, &reversed));
        assert_eq!(vec![60.0, 10.0, 30.0], get_lines_percentages(&content, &reversed));
    }

    #[test]
    fn sorting_uses_the_chosen_criterion_and_breaks_ties_by_name() {
        let content = hashmap![
            "Zig".to_owned() => Stats::new(9, 10, 100, 50, 0, HashMap::new()),
            "Ada".to_owned() => Stats::new(1, 900, 100, 90, 0, HashMap::new()),
            "Rust".to_owned() => Stats::new(5, 50, 300, 10, 0, HashMap::new())];

        assert_eq!(vec!["Rust","Ada","Zig"], get_sorted_language_names(&content, SortCriterion::Lines));
        assert_eq!(vec!["Zig","Rust","Ada"], get_sorted_language_names(&content, SortCriterion::Files));
        assert_eq!(vec!["Ada","Rust","Zig"], get_sorted_language_names(&content, SortCriterion::Size));
        assert_eq!(vec!["Ada","Zig","Rust"], get_sorted_language_names(&content, SortCriterion::Code));
        assert_eq!(vec!["Ada","Rust","Zig"], get_sorted_language_names(&content, SortCriterion::Name));

        // Ada and Zig both have 100 lines, so the name decides, not the iteration order of the map
        assert_eq!(vec!["Rust","Ada","Zig"], get_sorted_language_names(&content, SortCriterion::Lines));
    }

    #[test]
    fn test_retain_most_relevant_and_add_others_field_for_rest() {
        let sorted_language_names = vec!["a".to_owned(), "b".to_owned(), "c".to_owned(), "d".to_owned(), "e".to_owned()];
        let per_language = hashmap![
            "a".to_owned() => Stats::new(10, 60000, 1000, 800, 0, hashmap![]),
            "b".to_owned() => Stats::new(9, 50000, 900, 700, 0, hashmap![]),
            "c".to_owned() => Stats::new(8, 40000, 800, 600, 0, hashmap![]),
            "d".to_owned() => Stats::new(7, 30000, 700, 500, 0, hashmap![]),
            "e".to_owned() => Stats::new(6, 20000, 600, 400, 0, hashmap![])
        ];
        let total = Stats::total_of(&per_language);

        let (folded_names, folded_per_language) = fold_rest_into_others(
                &sorted_language_names, &per_language, &total, None);

        // The caller's own data is untouched, so the same result can be printed again or handed to
        // a second consumer. Folding into "others" produces a separate view and nothing more.
        assert_eq!(5, sorted_language_names.len());
        assert_eq!(5, per_language.len());
        assert_eq!(vec!["a".to_owned(), "b".to_owned(), "c".to_owned(), "others".to_owned()], folded_names);

        assert_eq!(hashmap![
            "a".to_owned() => Stats::new(10, 60000, 1000, 800, 0, hashmap![]),
            "b".to_owned() => Stats::new(9, 50000, 900, 700, 0, hashmap![]),
            "c".to_owned() => Stats::new(8, 40000, 800, 600, 0, hashmap![]),
            // The leftovers carry the files and the bytes of what was folded away as well as the
            // lines, so the overview's shares are shares of the whole run in all three of its bars
            "others".to_owned() => Stats::new(13, 50000, 1300, 0, 0, hashmap![])
            ], folded_per_language);

        // and what was folded plus what was kept is still the whole
        assert_eq!(total.lines, Stats::total_of(&folded_per_language).lines);
        assert_eq!(total.files, Stats::total_of(&folded_per_language).files);
        assert_eq!(total.bytes, Stats::total_of(&folded_per_language).bytes);
    }

    // What the 'modified:' tag reports: the directories beside the scope, and never the keywords,
    // which move nothing the log records
    #[test]
    fn a_changed_setting_is_tagged_and_a_keyword_setting_never_is() {
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        let entry_of = |edit: fn(&mut crate::config_manager::Configuration)| {
            let mut then = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
            edit(&mut then);
            crate::log::LogEntry { name: None, datetime: Local::now(),
                    scope: crate::diff::scope_of(&then.engine), targets: Vec::new(),
                    total: Stats::new(1, 1, 1, 1, 0, hashmap![]), modules: Vec::new() }
        };

        // Taken with the same settings: nothing to say
        let same = entry_of(|_| {});
        assert!(find_settings_changed_since(&same, &config, &[]).is_empty());
        assert!(format_modified_tag(&find_settings_changed_since(&same, &config, &[])).is_empty());

        // One that differs is named, and the names are the ones the reader can look up
        config.engine.braces_as_code = true;
        config.engine.no_gitignore = true;
        let changed = find_settings_changed_since(&same, &config, &[]);
        assert_eq!(vec!["braces-as-code", "no-gitignore"], changed);
        assert!(format_modified_tag(&changed).contains("braces-as-code, no-gitignore"));

        // The targets are compared as a set: the same list reordered is the same measurement
        let entry_with_dirs = crate::log::LogEntry {
                targets: vec![mezura_core::Target::of("./b"), mezura_core::Target::of("./a")],
                ..entry_of(|c| {c.engine.braces_as_code = true; c.engine.no_gitignore = true;}) };
        assert!(find_settings_changed_since(&entry_with_dirs, &config,
                &[mezura_core::Target::of("./a"), mezura_core::Target::of("./b")]).is_empty());
        assert_eq!(vec!["dirs"], find_settings_changed_since(&entry_with_dirs, &config,
                &[mezura_core::Target::of("./c")]));

        // and a run that only stopped counting keywords changed nothing the log holds
        config.engine.count_keywords = false;
        assert!(find_settings_changed_since(&entry_with_dirs, &config,
                &[mezura_core::Target::of("./a"), mezura_core::Target::of("./b")]).is_empty());
    }

    #[test]
    fn test_time_split_from_minutes() {
        assert_eq!((0,0,0),split_minutes_to_D_H_M(0));
        assert_eq!((0,0,59),split_minutes_to_D_H_M(59));
        assert_eq!((0,1,0),split_minutes_to_D_H_M(60));
        assert_eq!((0,1,1),split_minutes_to_D_H_M(61));
        assert_eq!((1,0,0),split_minutes_to_D_H_M(1440));
        assert_eq!((1,0,1),split_minutes_to_D_H_M(1441));
        assert_eq!((1,1,1),split_minutes_to_D_H_M(1501));
    }

}
