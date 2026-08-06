use std::collections::HashMap;

use chrono::{DateTime, Local};
use colored::{Color, ColoredString, Colorize};
use mezura_core::{RunResult, Stats, UNNAMED_MODULE_NAME, render};

use super::config_manager::{self, Configuration, Layout, SortCriterion};
use super::format::with_seperators;
use super::theme::Theme;

type ColorFunc = Box<dyn Fn(&str) -> String>;

const TOTAL_NAME : &str = "Total";

// How far a language sits under the module it belongs to, in either table
const GROUP_INDENT : &str = "  ";

// The list layout indents further: its rows are far wider and already carry a blank line between them
const LIST_INDENT : &str = "    ";

const MATRIX_METRICS : [&str; 3] = ["files", "lines", "code"];

// The row of MATRIX_METRICS that carries the language name, and the only one a module that lacks
// the language marks with a dash. Blanking the other two keeps a sparse matrix free of punctuation.
const MATRIX_LINES_ROW : usize = 1;

// Kept on both sides of the arrow, so the longest language name still has room around it
const NAME_GAP : usize = 3;

// The number of cells the overview's bar is drawn out of
const NUM_OF_VERTICALS : usize = 50;

// How many languages the overview names before folding the rest into "others"
const OVERVIEW_LANGUAGES : usize = 3;

const OTHERS_NAME : &str = "others";

// Has to be every key 'super::log::counting_settings' writes, or the missing one is read and
// dropped and a run whose settings changed shows moved numbers with no 'modified:' tag to say why.
// 'the_settings_written_to_a_log_are_the_settings_read_back' holds the two lists together.
const SETTING_KEYS : [&str; 8] = [config_manager::DIRS, config_manager::EXCLUDE, config_manager::LANGUAGES,
        config_manager::EXCLUDE_LANGUAGES, config_manager::FORCE_LANG, config_manager::BRACES_AS_CODE,
        config_manager::SEARCH_IN_DOTTED, config_manager::NO_GITIGNORE];

// The keys of the stats block of a log entry, as the log writes them
const FILES         : &str  = "Files:";
const LINES         : &str  = "Lines:";
const CODE          : &str  = "Code:";
const COMMENTS      : &str  = "Comments:";
const EXTRA         : &str  = "Extra:";
const TOTAL_SIZE    : &str  = "Total Size:";
const AVERAGE_SIZE  : &str  = "Average Size:";
const MODULES       : &str  = "Modules:";

pub fn format_and_print_results(result: &RunResult, existing_log_content: &Option<String>,
        datetime_now: &DateTime<Local>, config: &Configuration)
{
    let RunResult {per_language, total, ..} = result;
    let groups = groups_of(result, config);

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

    let theme = super::theme::active();
    let columns = Columns::of(&groups, total);
    let block_width = columns.width(theme);
    let should_print_keywords = !config.view.hidden.keywords;
    // Nothing to cross when no module was named, so the table is printed instead of a grid of one
    // column. Not an error, since killing a run over how its numbers would be shown costs the
    // numbers, and not silent either: the reader asked for one layout and is getting another.
    let mut layout = config.view.layout;
    if layout == Layout::Matrix && !is_grouped(&groups) {
        layout = Layout::Table;
        eprintln!("\n{}", super::theme::active().warning.paint("'--layout matrix' has nothing to cross, since no target was given a name, \
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
    if !config.view.hidden.progress && let Some(content) = existing_log_content
        && !content.trim().is_empty() && config.view.compare_level != 0 {
        print_comparison_to_previous_runs(result, content, config, datetime_now);
    }
}

// The theme listing runs before a configuration exists, so it cannot go through
// 'super::theme::active()'. It asks for the real rows of one made-up language instead, built by the
// same functions a run uses, so that the preview cannot drift from what will actually be printed,
// and it follows the layout in effect for the same reason. The figures are constants, so that every
// theme is judged against the same row.
pub fn theme_sample_rows(theme: &Theme, layout: Layout) -> Vec<String> {
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
            per_language: &per_language, total: &total}];

    // The two tables keep their keywords in a block of their own, so the sample has to ask for it or
    // the keyword tokens would go unshown in the two layouts that are now the common ones. One
    // language leaves nothing for a total to add up: it would only repeat the row above it.
    let with_keywords = |mut lines: Vec<String>| {
        lines.push(String::new());
        lines.extend(keyword_block_lines(theme, &groups));
        lines
    };
    match layout {
        Layout::Table => with_keywords(table_lines(theme, &groups, &total, false)),
        Layout::Boxed => with_keywords(boxed_lines(theme, &groups, &total, false)),
        // The matrix has no second axis to show for one made-up language of one unnamed module, and
        // the tokens it paints are the ones the table already previews
        Layout::Matrix => with_keywords(table_lines(theme, &groups, &total, false)),
        Layout::List => {
            let len_of = |value: usize| with_seperators(value).len();
            let columns = Columns {
                name: NAME.len().max(TOTAL_NAME.len()),
                headline: len_of(FILES).max(len_of(LINES)),
                code: len_of(CODE),
                comments: len_of(COMMENTS),
                extra: len_of(LINES - CODE - COMMENTS)
            };
            let width = columns.width(theme);
            vec![columns.files_row(theme, FILES, &size_text(theme, BYTES, BYTES / FILES), width),
                 columns.breakdown_row(theme, &theme.details_language_name.paint(NAME).to_string(), NAME.len(), LINES, CODE, COMMENTS),
                 get_keywords_as_str(theme, &keywords, columns.words_start(), width)]
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
    total: &'a Stats
}

impl Group<'_> {
    fn displayed_name(&self) -> &str {
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
fn groups_of<'a>(result: &'a RunResult, config: &Configuration) -> Vec<Group<'a>> {
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
            total: &module.total
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
fn named_rows<'a>(groups: &'a [Group], print_total: bool) -> Vec<(String, RowKind, &'a Group<'a>, Option<&'a String>)> {
    let grouped = is_grouped(groups);
    let mut rows = Vec::with_capacity(groups.len() * 4);
    for group in groups {
        if grouped {
            rows.push((group.displayed_name().to_owned(), RowKind::Module, group, None));
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
// the change of heading the reader of an uncoloured paste is told that 'backend' is a language.
fn name_header(groups: &[Group]) -> &'static str {
    if is_grouped(groups) {"Module"} else {"Language"}
}

// One aligned row per language, no borders: whitespace alignment survives being pasted into a
// README or a ticket. The header cells reuse the label token of the quantity underneath them and
// the body cells its number token, so the table needs no styling of its own.
fn print_as_table(theme: &Theme, groups: &[Group], total: &Stats,
        print_total: bool, should_print_keywords: bool)
{
    println!("{}.\n", theme.heading.paint("Details"));
    for line in table_lines(theme, groups, total, print_total) {
        println!("{line}");
    }

    if should_print_keywords {
        print_keyword_block(theme, groups);
    }

    // The 'list' layout closes with a blank line of its own, this one has to say so
    println!();
}

fn table_lines(theme: &Theme, groups: &[Group], total: &Stats, print_total: bool) -> Vec<String>
{
    // Every counted column carries its own percentage. The two that compare languages ('Files' and
    // 'Lines') take a share of the total, the two that describe one ('Code' and 'Comments') take a
    // share of that language's own lines.
    const HEADERS : [&str; 11] = ["Language", "Files", "%", "Lines", "%", "Code", "%", "Comments", "%", "Extra", "Size"];
    const GAP : usize = 4;
    // The columns a percentage belongs to, kept against their number by a gap of their own
    const TIGHT_AFTER : [usize; 4] = [1, 3, 5, 7];
    const TIGHT_GAP : usize = 2;

    fn row_of(theme: &Theme, name: &str, files: usize, lines: usize, code: usize, comments: usize, bytes: usize,
            total_files: usize, total_lines: usize) -> [String; 11]
    {
        fn percent_cell(value: f64) -> String {
            percent_text(value) + "%"
        }

        fn share(part: usize, whole: usize) -> String {
            percent_cell(if whole == 0 {0.0} else {part as f64 / whole as f64 * 100.0})
        }

        let (size, unit) = super::format::active().size_with_unit(bytes);
        let (code_percentage, comment_percentage) = percentages(lines, code, comments);
        [name.to_owned(),
         with_seperators(files), share(files, total_files),
         with_seperators(lines), share(lines, total_lines),
         with_seperators(code), percent_cell(code_percentage),
         with_seperators(comments), percent_cell(comment_percentage),
         with_seperators(lines - code - comments),
         size + " " + &theme.size_unit.paint(unit).to_string()]
    }

    let described = named_rows(groups, print_total);
    let rows = described.iter().map(|(cell, kind, group, language)| match kind {
            // A module's share is of the whole, a language's is of the module it is in: a module
            // reading 100% of itself would say nothing, which is the whole point of the two levels
            RowKind::Module => row_of(theme, cell, group.total.files, group.total.lines,
                    group.total.code_lines, group.total.comment_lines, group.total.bytes,
                    total.files, total.lines),
            RowKind::Total => row_of(theme, cell, total.files, total.lines, total.code_lines,
                    total.comment_lines, total.bytes, total.files, total.lines),
            RowKind::Language => {
                let name = language.unwrap();
                let content_info = group.per_language.get(name).unwrap();
                row_of(theme, cell, content_info.files, content_info.lines, content_info.code_lines, content_info.comment_lines,
                        content_info.bytes, group.total.files, group.total.lines)
            }
        }).collect::<Vec<_>>();

    let mut headers = HEADERS.map(str::to_owned);
    headers[0] = name_header(groups).to_owned();

    // The size cell carries its own colour for the unit, so its width has to be measured with the
    // escape sequences skipped rather than counted as characters
    let widths = (0..headers.len()).map(|i|
            rows.iter().map(|row| widest_visible_line(&row[i])).max().unwrap_or(0).max(headers[i].len())).collect::<Vec<_>>();

    let header_styles = [&theme.details_language_header, &theme.files_label, &theme.percent, &theme.lines_label, &theme.percent,
            &theme.code_label, &theme.percent, &theme.comments_label, &theme.percent, &theme.extra_label, &theme.total_size_label];
    let body_styles = [&theme.details_language_name, &theme.files_number, &theme.percent, &theme.lines_number, &theme.percent,
            &theme.code_number, &theme.percent, &theme.comments_number, &theme.percent, &theme.extra_number, &theme.total_size_number];

    // The figures are right aligned so they can be compared down a column. The language name and the
    // percentages are not: a percentage sits a fixed two spaces after the number it belongs to, and
    // padding it on the left would push it away on exactly the rows where its column is wider.
    let render = |cells: &[String], styles: &[&super::theme::Style; 11]| {
        let mut line = String::with_capacity(140);
        for (i, cell) in cells.iter().enumerate() {
            let padding = " ".repeat(widths[i] - widest_visible_line(cell));
            if i == 0 {
                line.push_str(&format!("{}{}", styles[i].paint(cell), padding));
            } else if TIGHT_AFTER.contains(&(i - 1)) {
                line.push_str(&format!("{}{}{}", " ".repeat(TIGHT_GAP), styles[i].paint(cell), padding));
            } else {
                line.push_str(&format!("{}{}{}", " ".repeat(GAP), padding, styles[i].paint(cell)));
            }
        }
        line
    };

    let table_width = widths.iter().sum::<usize>()
            + GAP * (HEADERS.len() - 1 - TIGHT_AFTER.len()) + TIGHT_GAP * TIGHT_AFTER.len();

    let mut lines = vec![render(&headers, &header_styles)];

    // A blank line closes each module, so the sections are read apart at a glance instead of being
    // told apart only by the indentation of every second row. Without grouping there are no
    // sections and nothing changes.
    let grouped = is_grouped(groups);
    for (position, (row, (_, kind, _, _))) in rows.iter().zip(described.iter()).enumerate() {
        if grouped && position > 0 && *kind != RowKind::Language {
            lines.push(String::new());
        }
        if *kind == RowKind::Total {
            lines.push(theme.separator.paint(&"-".repeat(table_width)).to_string());
        }
        let mut styles = body_styles;
        styles[0] = match kind {
            RowKind::Module => &theme.details_module,
            RowKind::Total => &theme.details_total,
            RowKind::Language => &theme.details_language_name
        };
        lines.push(render(row, &styles));
    }

    lines
}

// Languages down, modules across. The nested table answers "what is inside the backend", read down
// a section; this one answers "how do the modules compare on the same language", read along a row.
fn print_as_matrix(theme: &Theme, groups: &[Group], languages: &[String], total: &Stats,
        print_total: bool, should_print_keywords: bool)
{
    println!("{}.\n", theme.heading.paint("Details"));
    for line in matrix_lines(theme, groups, languages, total, print_total) {
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
            total: group.total
        }).collect::<Vec<_>>();
        print_keyword_block(theme, &shown);
    }
    println!();
}

fn matrix_lines<'a>(theme: &'a Theme, groups: &[Group], languages: &[String], total: &Stats,
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
        Some(value) => with_seperators(value),
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
            .chain(groups.iter().map(|group| group.displayed_name().to_owned()))
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
    for line in boxed_lines(theme, groups, total, print_total) {
        println!("{line}");
    }

    if should_print_keywords {
        print_keyword_block(theme, groups);
    }
    println!();
}

fn boxed_lines(theme: &Theme, groups: &[Group], total: &Stats, print_total: bool) -> Vec<String>
{
    const HEADERS : [&str; 7] = ["Language", "Files", "Lines", "Code", "Comments", "Extra", "Size"];
    // The columns that carry a percentage next to their number
    const WITH_PERCENT : usize = 4;
    const PERCENT_GAP : usize = 2;
    // One space of air between a border and the text it holds
    const PAD : usize = 1;

    struct Cell { number: String, percent: String }

    fn row_of(theme: &Theme, name: &str, files: usize, lines: usize, code: usize, comments: usize, bytes: usize,
            total_files: usize, total_lines: usize) -> (String, [Cell; 6])
    {
        fn share(part: usize, whole: usize) -> String {
            percent_text(if whole == 0 {0.0} else {part as f64 / whole as f64 * 100.0}) + "%"
        }
        fn cell(number: String, percent: String) -> Cell {
            Cell { number, percent }
        }

        let (size, unit) = super::format::active().size_with_unit(bytes);
        let (code_percentage, comment_percentage) = percentages(lines, code, comments);
        (name.to_owned(), [
            cell(with_seperators(files), share(files, total_files)),
            cell(with_seperators(lines), share(lines, total_lines)),
            cell(with_seperators(code), percent_text(code_percentage) + "%"),
            cell(with_seperators(comments), percent_text(comment_percentage) + "%"),
            cell(with_seperators(lines - code - comments), String::new()),
            cell(size + " " + &theme.size_unit.paint(unit).to_string(), String::new())])
    }

    let described = named_rows(groups, print_total);
    let rows = described.iter().map(|(cell, kind, group, language)| match kind {
            RowKind::Module => row_of(theme, cell, group.total.files, group.total.lines,
                    group.total.code_lines, group.total.comment_lines, group.total.bytes,
                    total.files, total.lines),
            RowKind::Total => row_of(theme, cell, total.files, total.lines, total.code_lines,
                    total.comment_lines, total.bytes, total.files, total.lines),
            RowKind::Language => {
                let name = language.unwrap();
                let content_info = group.per_language.get(name).unwrap();
                row_of(theme, cell, content_info.files, content_info.lines, content_info.code_lines, content_info.comment_lines,
                        content_info.bytes, group.total.files, group.total.lines)
            }
        }).collect::<Vec<_>>();

    let name_title = name_header(groups);
    let name_width = rows.iter().map(|(name,_)| name.chars().count()).max().unwrap_or(0).max(name_title.len());
    // Measured with the escape sequences skipped, since the size cell colours its own unit
    let number_widths = (0..6).map(|i| rows.iter().map(|(_,cells)| widest_visible_line(&cells[i].number)).max().unwrap_or(0))
            .collect::<Vec<_>>();
    let percent_widths = (0..6).map(|i| rows.iter().map(|(_,cells)| cells[i].percent.chars().count()).max().unwrap_or(0))
            .collect::<Vec<_>>();

    // A column is as wide as its content needs, or as its header, whichever is more
    let inner_widths = std::iter::once(name_width).chain((0..6).map(|i| {
            let content = number_widths[i] + if i < WITH_PERCENT {PERCENT_GAP + percent_widths[i]} else {0};
            content.max(HEADERS[i + 1].len())
        })).collect::<Vec<_>>();

    let header_styles = [&theme.files_label, &theme.lines_label, &theme.code_label,
            &theme.comments_label, &theme.extra_label, &theme.total_size_label];
    let number_styles = [&theme.files_number, &theme.lines_number, &theme.code_number,
            &theme.comments_number, &theme.extra_number, &theme.total_size_number];

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
        let text = HEADERS[i + 1];
        let left = (width - text.len()) / 2;
        header_cells.push(format!("{}{}{}", " ".repeat(left), style.paint(text), " ".repeat(width - text.len() - left)));
    }
    lines.push(content_row(header_cells, true));

    // The lines that bound the body, and the line that opens a module's section, belong to the
    // frame: solid and in its shade. Only the lines between two languages are dashed, and those
    // alternate between two shades.
    for (position, (name, cells)) in rows.iter().enumerate() {
        let kind = described[position].1;
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
            let number = format!("{}{}", " ".repeat(number_widths[i] - widest_visible_line(&cell.number)),
                    number_styles[i].paint(&cell.number));
            let body = if i < WITH_PERCENT {
                format!("{number}{}{}{}", " ".repeat(PERCENT_GAP), theme.percent.paint(&cell.percent),
                        " ".repeat(percent_widths[i] - cell.percent.chars().count()))
            } else {
                number
            };
            let used = number_widths[i] + if i < WITH_PERCENT {PERCENT_GAP + percent_widths[i]} else {0};
            painted.push(format!("{body}{}", " ".repeat(inner_widths[i + 1] - used)));
        }
        lines.push(content_row(painted, kind != RowKind::Language));
    }

    lines.push(frame("└", "┴", "┘", "─", BORDER_OUTER, false));
    lines
}

fn print_individually(theme: &Theme, groups: &[Group], columns: &Columns, block_width: usize, should_print_keywords: bool)
{
    print_lines(&individual_lines(theme, groups, columns, block_width, should_print_keywords));
}

fn individual_lines(theme: &Theme, groups: &[Group], columns: &Columns, block_width: usize,
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
            let name = group.displayed_name();
            let stats = group.total;
            lines.push(columns.files_row(theme, stats.files,
                    &size_text(theme, stats.bytes, stats.average_size()), block_width));
            lines.push(columns.breakdown_row(theme, &theme.details_module.paint(name).to_string(),
                    name.chars().count(), stats.lines, stats.code_lines, stats.comment_lines));
        }

        for (i, lang_name) in group.languages.iter().enumerate() {
            let content_info = group.per_language.get(lang_name).unwrap();
            if grouped || i > 0 {
                lines.push(String::new());
            }

            lines.push(columns.files_row(theme, content_info.files,
                    &size_text(theme, content_info.bytes, content_info.average_size()), block_width));
            lines.push(columns.breakdown_row(theme, &(indent.to_owned() + &theme.details_language_name.paint(lang_name).to_string()),
                    lang_name.chars().count() + indent.len(), content_info.lines, content_info.code_lines, content_info.comment_lines));
            if should_print_keywords {
                let keywords = get_keywords_as_str(theme, &content_info.keyword_occurences, columns.words_start(), block_width);
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
        let len_of = |value: usize| with_seperators(value).len();
        let mut columns = Columns {
            name: TOTAL_NAME.len(),
            headline: len_of(total.files).max(len_of(total.lines)),
            code: len_of(total.code_lines),
            comments: len_of(total.comment_lines),
            extra: len_of(total.extra_lines())
        };

        // The total holds the largest of every column, except when --top hid the language that made
        // it so, which is why the shown ones are measured too instead of assumed smaller
        for group in groups {
            // The leftovers print their name too, so it has to be measured too: a name wider than
            // the column it sits in makes the padding of its row a subtraction below zero.
            if grouped {
                columns.name = columns.name.max(group.displayed_name().chars().count());
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
    fn headline_end(&self) -> usize {
        self.name + 2 * NAME_GAP + 2 + self.headline
    }

    // Where the words 'files' and 'lines' start, and with them the keywords row
    fn words_start(&self) -> usize {
        self.headline_end() + 1
    }

    // The theme arrives as an argument rather than being read from 'super::theme::active()', because
    // '--show-themes' renders one sample per theme it found, in a single run, and it renders it
    // through these same functions so that the preview cannot drift from the real output
    fn breakdown_row(&self, theme: &Theme, painted_name: &str, name_len: usize, lines: usize, code_lines: usize, comment_lines: usize) -> String {
        let (code_percentage, comment_percentage) = percentages(lines, code_lines, comment_lines);
        format!("{}{}{}{}{:>headline_w$} {} {{ {:>code_w$} {} ({})  +  {:>comments_w$} {} ({})  +  {:>extra_w$} {} }}",
                painted_name, " ".repeat(self.name - name_len + NAME_GAP), theme.arrow.paint("->"), " ".repeat(NAME_GAP),
                theme.lines_number.paint(&with_seperators(lines)), theme.lines_label.paint("lines"),
                theme.code_number.paint(&with_seperators(code_lines)), theme.code_label.paint("code"), percent(theme, code_percentage),
                theme.comments_number.paint(&with_seperators(comment_lines)), theme.comments_label.paint("comments"), percent(theme, comment_percentage),
                theme.extra_number.paint(&with_seperators(lines - code_lines - comment_lines)), theme.extra_label.paint("extra"),
                headline_w = self.headline, code_w = self.code, comments_w = self.comments, extra_w = self.extra)
    }

    // The size text ends where the row below it does
    fn files_row(&self, theme: &Theme, files: usize, size_text: &str, width: usize) -> String {
        let left = format!("{}{:>headline_w$} {}", " ".repeat(self.headline_end() - self.headline),
                theme.files_number.paint(&with_seperators(files)), theme.files_label.paint("files"), headline_w = self.headline);
        let used = widest_visible_line(&left) + widest_visible_line(size_text);

        left + &" ".repeat(width.saturating_sub(used).max(2)) + size_text
    }

    // Rendered once to be measured and again to be printed: once per run costs nothing, and the
    // width cannot fall behind the row the way a formula would
    fn width(&self, theme: &Theme) -> usize {
        widest_visible_line(&self.breakdown_row(theme, "", 0, 0, 0, 0))
    }
}

fn print_sum(theme: &Theme, per_language: &HashMap<String,Stats>, total: &Stats, columns: &Columns,
        block_width: usize, should_print_keywords: bool)
{
    print_lines(&sum_lines(theme, per_language, total, columns, block_width, should_print_keywords));
}

fn sum_lines(theme: &Theme, per_language: &HashMap<String,Stats>, total: &Stats, columns: &Columns,
        block_width: usize, should_print_keywords: bool) -> Vec<String>
{
    // The separator spans the block, which every row of the details section already fits exactly
    let mut lines = vec![
        format!("{} ",theme.separator.paint(&"-".repeat(block_width))),
        columns.files_row(theme, total.files,
                &size_text(theme, total.bytes, total.average_size()), block_width),
        columns.breakdown_row(theme, &theme.details_total.paint(TOTAL_NAME).to_string(),
                TOTAL_NAME.len(), total.lines, total.code_lines, total.comment_lines)];

    if should_print_keywords {
        let keywords_line = get_keywords_as_str(theme, &create_keyword_sum_map(per_language), columns.words_start(), block_width);
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
    let lines = keyword_block_lines(theme, groups);
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
fn keyword_block_lines(theme: &Theme, groups: &[Group]) -> Vec<String> {
    const GAP : usize = 3;

    let grouped = is_grouped(groups);
    let rows = groups.iter().map(|group| (group, group.languages.iter().filter_map(|name| {
            let keywords = get_keywords_as_str(theme, &group.per_language.get(name).unwrap().keyword_occurences, 0, usize::MAX);
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
            lines.push(theme.details_module.paint(group.displayed_name()).to_string());
        }
        lines.extend(keyword_rows.into_iter().map(|(name, keywords)| format!("{}{}{}{}", " ".repeat(indent),
                theme.details_language_name.paint(name),
                " ".repeat(language_width - name.chars().count() + GAP), keywords)));
    }

    lines
}

// Indented to where the word 'lines' starts on the row above, and wrapped to the width of the block
// so that a language with many keywords cannot push the section wider than every other row
fn get_keywords_as_str(theme: &Theme, keyword_occurencies: &HashMap<String,usize>, indent: usize, width: usize) -> String {
    const SEPARATOR : &str = ", ";

    if keyword_occurencies.is_empty() {
        return String::new();
    }

    let mut keyword_info = " ".repeat(indent);
    let mut used = indent;
    for (position, (name, count)) in keyword_entries(keyword_occurencies).into_iter().enumerate() {
        let entry_len = name.chars().count() + 2 + count.chars().count();
        let entry = format!("{}: {}", theme.keyword_label.paint(&name), theme.keyword_number.paint(&count));

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
fn keyword_entries(keyword_occurencies: &HashMap<String,usize>) -> Vec<(String, String)> {
    let mut sorted_keywords = keyword_occurencies.iter().collect::<Vec<_>>();
    sorted_keywords.sort_unstable_by_key(|(name,_)| name.as_str());

    sorted_keywords.into_iter().map(|(name, occurancies)| (name.to_owned(), with_seperators(*occurancies))).collect()
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
    print_lines(&overview_lines(sorted_language_names, per_language, total, config));
}

fn overview_lines(sorted_language_names: &[String], per_language: &HashMap<String, Stats>, total: &Stats, config: &Configuration) -> Vec<String>
{
    // The function itself decides whether there is anything to fold
    let (sorted_language_vec, per_language) =
            most_relevant_with_others_for_rest(sorted_language_names, per_language, total, config.view.top_n);
    let (sorted_language_vec, per_language) =
            (&sorted_language_vec, &per_language);

    // 'others' takes its style by identity and not by position, because --top moves it: with
    // --top 2 it sits third and used to steal the slot meant for the third language.
    let slots = super::theme::active().language_slots();
    let styles = sorted_language_vec.iter().enumerate()
            .map(|(i, name)| if name == OTHERS_NAME {slots[slots.len()-1]} else {slots[i.min(slots.len()-2)]}.clone())
            .collect::<Vec<_>>();
    let color_func_vec : Vec<ColorFunc> = styles.iter().cloned()
            .map(|style| Box::new(move |s: &str| style.paint(s).to_string()) as ColorFunc).collect();
    // A bar cell takes the color of the slot and none of its attributes: bold or underline on a
    // block character is not something a terminal shows usefully
    let bar_func_vec : Vec<ColorFunc> = styles.iter().map(|style| {
            let color = style.color;
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
        percent_text(files_percentages[i]).len().max(percent_text(lines_percentages[i]).len())
                .max(percent_text(sizes_percentages[i]).len())
    }).collect::<Vec<_>>();

    let files_line = create_overview_line("Files:", &files_percentages, &files_verticals,
            sorted_language_vec, &color_func_vec, &bar_func_vec, &percent_widths, config);
    let lines_line = create_overview_line("Lines:", &lines_percentages, &lines_verticals,
            sorted_language_vec, &color_func_vec, &bar_func_vec, &percent_widths, config);
    let size_line = create_overview_line("Size :", &sizes_percentages, &size_verticals,
            sorted_language_vec, &color_func_vec, &bar_func_vec, &percent_widths, config);

    vec![format!("{}.", super::theme::active().heading.paint("Overview")), String::new(),
         files_line, String::new(), lines_line, String::new(), size_line, String::new()]
}

fn create_overview_line(prefix: &str, percentages: &[f64], verticals: &[usize], languages_name: &[String],
        color_func_vec: &[ColorFunc], bar_func_vec: &[ColorFunc], percent_widths: &[usize], config: &Configuration) -> String
{
    let theme = super::theme::active();
    let mut line = String::with_capacity(150);
    line.push_str(&format!("{}   ", theme.overview_label.paint(prefix)));
    for (i,percentage) in percentages.iter().enumerate() {
        let str_perc = percent_text(*percentage);
        line.push_str(&format!("{}{} ", " ".repeat(percent_widths[i].saturating_sub(str_perc.len())), overview_percent(theme, *percentage)));
        line.push_str(&color_func_vec[i](&languages_name[i]));
        if i < percentages.len() - 1{
            line.push_str(" - ")
        }
    }

    if !config.view.hidden.bar {
        add_verticals_str(&mut line, verticals, bar_func_vec, config.view.bar_thickness.character());
    }

    line
}

fn add_verticals_str(line: &mut String, files_verticals: &[usize], color_func_vec: &[ColorFunc], character: &str) {
    let theme = super::theme::active();
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
fn most_relevant_with_others_for_rest(sorted_language_names: &[String],
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

    render::percentages(&language_files)
}

fn get_lines_percentages(per_language: &HashMap<String,Stats>, languages_name: &[String]) -> Vec<f64> {
    let mut language_lines = [0].repeat(per_language.len());
    per_language.iter().for_each(|e| {
        let pos = languages_name.iter().position(|name| name == e.0).unwrap();
        language_lines[pos] = e.1.lines;
    });

    render::percentages(&language_lines)
}

fn get_sizes_percentages(per_language: &HashMap<String,Stats>, languages_name: &[String]) -> Vec<f64> {
    let mut language_size = [0].repeat(per_language.len());
    per_language.iter().for_each(|e| {
        let pos = languages_name.iter().position(|name| name == e.0).unwrap();
        language_size[pos] = e.1.bytes;
    });

    render::percentages(&language_size)
}

// The settings this run counted with, against the ones the entry recorded. A setting the entry never
// wrote is left alone rather than reported as changed, which is what keeps entries from older
// versions from being accused of a difference nobody can know about.
fn settings_changed_since(entry: &LogEntry, config: &Configuration, targets: &[mezura_core::Target]) -> Vec<&'static str> {
    super::log::counting_settings(&config.engine, targets).into_iter()
            .filter(|(key, value)| entry.settings.iter().any(|(k, v)| k == key && v != value))
            .map(|(key, _)| key)
            .collect()
}

// At the end of the line of the entry it belongs to, since it is a statement about that entry and
// not about the run. Nothing is printed when nothing changed, or the reader stops seeing it.
fn modified_tag(changed: &[&'static str]) -> String {
    if changed.is_empty() {
        return String::new();
    }

    let theme = super::theme::active();
    format!("   {} {}", theme.progress_modified.paint("modified:"),
            theme.progress_modified_field.paint(&changed.join(", ")))
}

// One line per module under the line of the entry, and narrower than it: Files and Extra stay on
// the total, since what is asked of a module is which part of it moved. With every column repeated,
// one entry is five wide lines and '--compare 3' stops being readable.
fn module_comparison_lines(entry: &LogEntry, groups: &[Group]) -> String {
    let theme = super::theme::active();
    let names = groups.iter().map(|x| x.displayed_name().to_owned())
            .chain(entry.modules.iter().map(|x| x.name.clone())
                    .filter(|name| !groups.iter().any(|x| x.displayed_name() == name)))
            .collect::<Vec<_>>();
    let width = names.iter().map(|x| x.chars().count()).max().unwrap_or(0);

    // Right aligned down the entry, since the whole reason these are three narrow columns and not
    // the full breakdown is that they are meant to be read down rather than across
    let compared = names.iter().filter_map(|name| entry.modules.iter().find(|x| &x.name == name)).collect::<Vec<_>>();
    let number_width = |value: fn(&ModuleEntry) -> usize|
            compared.iter().map(|x| with_seperators(value(x)).len()).max().unwrap_or(0);
    let (lines_width, code_width, comments_width) =
            (number_width(|x| x.lines), number_width(|x| x.code_lines), number_width(|x| x.comment_lines));

    let mut rendered = String::with_capacity(names.len() * 80);
    for name in &names {
        let padded = format!("       {}{}   ", theme.details_module.paint(name),
                " ".repeat(width - name.chars().count()));
        let now = groups.iter().find(|x| x.displayed_name() == name).map(|x| x.total);
        let then = entry.modules.iter().find(|x| &x.name == name);
        // A module compared against nothing would read '+100%', which is false: it did not grow, it
        // started being counted on its own. The ones that are not in both are named as what they are.
        let tail = match (now, then) {
            (Some(now), Some(then)) => {
                let cell = |style: &super::theme::Style, value: usize, then: usize, width: usize| {
                    let text = with_seperators(then);
                    format!("{}{}({}%)", " ".repeat(width - text.len()), style.paint(&text),
                            color_percentage(&difference_as_signed_percentage_str_of_usize(then, value)))
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

fn color_percentage(percentage: &str) -> ColoredString {
    let theme = super::theme::active();
    if percentage.starts_with('+') {
        theme.progress_up.paint(percentage)
    } else if percentage.starts_with('-') {
        theme.progress_down.paint(percentage)
    } else {
        theme.progress_same.paint(percentage)
    }
}

fn print_comparison_to_previous_runs(result: &RunResult, log_content: &str, config: &Configuration, datetime_now: &DateTime<Local>) {
    println!("\n{}.\n", super::theme::active().heading.paint("Progress"));

    let total = &result.total;
    let log_entries = parse_N_previous_entries(log_content, config.view.compare_level);
    // Silent until used: a run that named no module says nothing about them here either, and the
    // 'modified: dirs' tag is what already reports that the targets are not the ones they were
    let groups = if result.has_modules() {groups_of(result, config)} else {Vec::new()};

    let mut comparison_str = String::with_capacity(200);
    for entry in log_entries.iter() {
        let duration = datetime_now.signed_duration_since(entry.datetime);
        let (days, hours, minutes) = split_minutes_to_D_H_M(duration.num_minutes());
        let arrow = super::theme::active().progress_entry.paint("->");
        let tag = modified_tag(&settings_changed_since(entry, config, &result.targets));
        if let Some(name) = &entry.name {
            comparison_str.push_str(&format!("{} \"{}\" ({} days, {} hours and {} minutes ago){}\n",
                    arrow, name, days, hours, minutes, tag));
        } else {
            let then_str = entry.datetime.naive_local().to_string();
            comparison_str.push_str(&format!("{} {} ({} days, {} hours and {} minutes ago){}\n",
                    arrow, then_str, days, hours, minutes, tag));
        }
        // An entry from before the comments were split off has an 'extra' that meant something
        // else, so it is named and left uncompared instead of being reported as a collapse
        let tail = if entry.splits_comments {
            format!("Comments: {}({}%), Extra: {}({}%)",
                super::theme::active().comments_number.paint(&with_seperators(entry.stats.comment_lines)), color_percentage(&difference_as_signed_percentage_str_of_usize(entry.stats.comment_lines, total.comment_lines)),
                super::theme::active().extra_number.paint(&with_seperators(entry.stats.extra_lines())), color_percentage(&difference_as_signed_percentage_str_of_usize(entry.stats.extra_lines(), total.extra_lines())))
        } else {
            format!("Non-code: {} (logged before comments were counted separately)",
                super::theme::active().extra_number.paint(&with_seperators(entry.stats.extra_lines())))
        };
        comparison_str.push_str(&format!("     Files: {}({}%) Lines: {}({}%) {{Code: {}({}%), {}}}\n",
                super::theme::active().files_number.paint(&with_seperators(entry.stats.files)), color_percentage(&difference_as_signed_percentage_str_of_usize(entry.stats.files, total.files)),
                super::theme::active().lines_number.paint(&with_seperators(entry.stats.lines)), color_percentage(&difference_as_signed_percentage_str_of_usize(entry.stats.lines, total.lines)),
                super::theme::active().code_number.paint(&with_seperators(entry.stats.code_lines)), color_percentage(&difference_as_signed_percentage_str_of_usize(entry.stats.code_lines, total.code_lines)),
                tail));
        if !groups.is_empty() {
            comparison_str.push_str(&module_comparison_lines(entry, &groups));
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

fn difference_as_signed_percentage_str_of_usize(older: usize, newer: usize) -> String {
    super::format::active().signed_percent(render::relative_change(older, newer))
}

// Only what a module line prints. Files and Extra are on the total and not repeated per module.
#[derive(Debug,Default,Clone)]
struct ModuleEntry {
    name: String,
    lines: usize,
    code_lines: usize,
    comment_lines: usize
}

#[derive(Debug)]
struct LogEntry {
    name: Option<String>,
    stats: Stats,
    // Empty for every entry written before modules existed, and for every run that named none
    modules: Vec<ModuleEntry>,
    datetime: DateTime<Local>,
    // The counting settings as that run recorded them. A setting the entry does not mention is one
    // that version did not write, and an absent setting can never be reported as changed: the older
    // entries here have no 'excluded-languages' line at all, and none of them has 'gitignore'
    settings: Vec<(String, String)>,
    // Entries written before v3.0.0 have no 'Comments' key, and their 'Extra' counted the comments
    // in as well, so comparing it against an extra that no longer does would report a drop that
    // never happened
    splits_comments: bool
}

fn parse_N_previous_entries(log_content: &str, n: usize) -> Vec<LogEntry> {
    let mut log_entries : Vec<LogEntry> = Vec::with_capacity(15);
    let (mut files, mut lines, mut code_lines, mut comment_lines, mut bytes_size) = (0, 0, 0, 0, 0);
    let mut splits_comments = false;
    let mut counter = 0;
    let mut is_expecting_date = false;
    // The block sits below the stats of the entry it belongs to, which has been pushed by the time
    // it is read. Its own 'Lines' and 'Comments' lines are dispatched here and never through the
    // branches below: left to them, a 'Comments' of a module would make the entry that follows,
    // written by a version that had none, look like one that counted them.
    let mut is_reading_modules = false;
    let mut entry_name = None;
    let mut datetime = chrono::Local::now();
    let mut settings : Vec<(String, String)> = Vec::with_capacity(7);

    for line in log_content.lines() {
        let line = line.trim_start();

        if let Some(entry) = line.strip_prefix("===>") {
            // The count is checked here and not where an entry is pushed, so that the block under
            // the last one asked for is still read before the walk stops
            if counter == n {return log_entries}
            (is_expecting_date, is_reading_modules) = (true, false);
            settings.clear();
            let _entry = entry.trim();
            entry_name = if _entry.is_empty() {None} else {Some(_entry.to_owned())};
            continue;
        }

        if is_expecting_date {
            let fixed_datetime = chrono::DateTime::parse_from_str(line, "%Y-%m-%d %H:%M:%S %z").unwrap();
            datetime = fixed_datetime.with_timezone(&Local);
            is_expecting_date = false;
            continue;
        }

        if is_reading_modules {
            read_module_line(line, log_entries.last_mut());
            continue;
        }

        if let Some((key, value)) = line.split_once(": ").or_else(|| line.strip_suffix(':').map(|key| (key, "")))
            && SETTING_KEYS.contains(&key) {
            settings.push((key.to_owned(), value.trim().to_owned()));
            continue;
        }

        if line == MODULES {
            is_reading_modules = true;
        } else if let Some(value) = line.strip_prefix(FILES) {
            files = value.trim().parse::<usize>().unwrap();
        } else if let Some(value) = line.strip_prefix(LINES) {
            lines = value.trim().parse::<usize>().unwrap();
        } else if let Some(value) = line.strip_prefix(CODE) {
            code_lines = value.trim().parse::<usize>().unwrap();
        } else if let Some(value) = line.strip_prefix(COMMENTS) {
            comment_lines = value.trim().parse::<usize>().unwrap();
            splits_comments = true;
        } else if let Some(value) = line.strip_prefix(EXTRA) {
            // Read back from the three it comes from and not from the line the entry stores, which
            // is the same number: an entry from before the comments were counted has none of them,
            // so 'lines - code - 0' is exactly the 'extra' it wrote.
            let _ = value.trim().parse::<usize>().unwrap();
        } else if let Some(value) = line.strip_prefix(TOTAL_SIZE) {
            bytes_size = value.trim().parse::<usize>().unwrap();
        } else if let Some(value) = line.strip_prefix(AVERAGE_SIZE) {
            let _ = value.trim().parse::<usize>().unwrap();
            let stats = Stats::new(files, bytes_size, lines, code_lines, comment_lines, HashMap::new());
            log_entries.push(LogEntry{name: entry_name.clone(), stats, modules: Vec::new(), datetime,
                    settings: settings.clone(), splits_comments});
            // All six. A complete entry overwrites them all and the reset changes nothing, but an
            // entry missing a line would otherwise take that figure from the entry above it.
            (files, lines, code_lines, comment_lines, bytes_size, splits_comments) = (0, 0, 0, 0, 0, false);
            counter += 1;
        }
    }

    log_entries
}

// A name is a line that ends in a colon with nothing after it, which is what tells it apart from the
// figures under it however a module happens to be called
fn read_module_line(line: &str, entry: Option<&mut LogEntry>) {
    let Some(entry) = entry else { return };

    if let Some(name) = line.strip_suffix(':') {
        entry.modules.push(ModuleEntry {name: name.to_owned(), ..Default::default()});
        return;
    }

    let Some(module) = entry.modules.last_mut() else { return };
    if let Some(value) = line.strip_prefix(LINES) {
        module.lines = value.trim().parse::<usize>().unwrap_or(0);
    } else if let Some(value) = line.strip_prefix(CODE) {
        module.code_lines = value.trim().parse::<usize>().unwrap_or(0);
    } else if let Some(value) = line.strip_prefix(COMMENTS) {
        module.comment_lines = value.trim().parse::<usize>().unwrap_or(0);
    }
}

fn print_lines(lines: &[String]) {
    for line in lines {
        println!("{line}");
    }
}

// The text is already coloured by the time it gets measured, so the escape sequences have to be
// skipped: their bytes are in the string but not on the screen.
fn widest_visible_line(text: &str) -> usize {
    fn visible_len(line: &str) -> usize {
        let mut len = 0;
        let mut chars = line.chars();
        while let Some(character) = chars.next() {
            if character == '\x1b' {
                for terminator in chars.by_ref() {
                    if terminator == 'm' {break}
                }
            } else {
                len += 1;
            }
        }
        len
    }

    text.lines().map(visible_len).max().unwrap_or(0)
}

fn size_text(theme: &Theme, total_bytes: usize, average_bytes: usize) -> String {
    let (total, total_unit) = super::format::active().size_with_unit(total_bytes);
    let (average, average_unit) = super::format::active().size_with_unit(average_bytes);
    format!("{} {} {} - {} {} {}",
            theme.total_size_number.paint(&total),
            theme.size_unit.paint(total_unit), theme.total_size_label.paint("total"),
            theme.avg_size_number.paint(&average),
            theme.size_unit.paint(average_unit), theme.avg_size_label.paint("average"))
}

fn percent_text(value: f64) -> String {
    super::format::active().percent(value)
}

// The '%' is painted with the number, or it keeps the default colour while the digits fade
fn percent(theme: &Theme, value: f64) -> ColoredString {
    theme.percent.paint(&(percent_text(value) + "%"))
}

// The overview's percentages are the datum of that section rather than an annotation on a count,
// so they take a token of their own
fn overview_percent(theme: &Theme, value: f64) -> ColoredString {
    theme.overview_percent.paint(&(percent_text(value) + "%"))
}

fn percentages(lines: usize, code_lines: usize, comment_lines: usize) -> (f64, f64) {
    if lines == 0 {
        return (0f64, 0f64);
    }
    (code_lines as f64 / lines as f64 * 100f64, comment_lines as f64 / lines as f64 * 100f64)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::str::FromStr;

    use mezura_core::{FilesPresent, ModuleResult};

    use crate::config_manager::LogOption;
    use crate::paths::test_paths::{FIXTURES_DIR, SCRATCH_LOG_DIR};
    use super::super::log::log_stats;
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
            ModuleResult {name: name.map(str::to_owned), per_language, total}
        };

        vec![of(Some("frontend"), &["JavaScript", "HTML"]), of(Some("backend"), &["Rust"]),
             of(None, &["Python", "Java"])]
    }

    fn groups_from<'a>(modules: &'a [ModuleResult], config: &crate::config_manager::Configuration) -> Vec<Group<'a>> {
        let result = RunResult {per_language: HashMap::new(),
                modules: Vec::new(), total: Stats::default(), faulty_files: Vec::new(),
                files_present: FilesPresent::default(), targets: Vec::new(), unreadable_dirs: Vec::new(), performance: mezura_core::Performance { duration_millis: 0, threads: mezura_core::Threads::new(1, 1) }};
        let mut result = result;
        result.modules = modules.iter().map(|x| ModuleResult {
            name: x.name.clone(),
            per_language: x.per_language.clone(),
            total: Stats::total_of(&x.per_language)
        }).collect();
        // The borrow has to outlive the temporary, so the groups are built against the caller's slice
        let order = groups_of(&result, config).into_iter().map(|x| (x.name.map(str::to_owned), x.languages, x.hidden))
                .collect::<Vec<_>>();

        order.into_iter().map(|(name, languages, hidden)| {
            let module = modules.iter().find(|x| x.name == name).unwrap();
            Group {name: module.name.as_deref(), languages, hidden, per_language: &module.per_language, total: &module.total}
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
        let plain = vec![Group {name: None, languages: sorted.clone(), hidden: 0, per_language: &content_info, total: &total}];
        let columns = Columns::of(&plain, &total);
        let width = columns.width(theme);

        let mut cases: Vec<(String, Vec<String>)> = Vec::new();
        let mut list = individual_lines(theme, &plain, &columns, width, true);
        list.extend(sum_lines(theme, &content_info, &total, &columns, width, true));
        cases.push(("list".to_owned(), list));
        cases.push(("list, keywords hidden".to_owned(),
                individual_lines(theme, &plain, &columns, width, false)));

        let mut table = table_lines(theme, &plain, &total, true);
        table.extend(keyword_block_lines(theme, &plain));
        cases.push(("table".to_owned(), table));

        let mut boxed = boxed_lines(theme, &plain, &total, true);
        boxed.extend(keyword_block_lines(theme, &plain));
        cases.push(("boxed".to_owned(), boxed));

        cases.push(("overview".to_owned(), overview_lines(&sorted, &content_info, &total, &config)));

        config.view.top_n = Some(2);
        cases.push(("overview, top 2".to_owned(), overview_lines(&sorted, &content_info, &total, &config)));

        config.view.top_n = None;
        config.view.hidden.bar = true;
        cases.push(("overview, bar hidden".to_owned(), overview_lines(&sorted, &content_info, &total, &config)));

        // The same data with a second axis through it. Every layout groups, and the total under them
        // is the same total, which is what makes the two halves of this file comparable by eye.
        let mut config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);
        let modules = sample_modules();
        let groups = groups_from(&modules, &config);
        let columns = Columns::of(&groups, &total);
        let width = columns.width(theme);

        let mut list = individual_lines(theme, &groups, &columns, width, true);
        list.extend(sum_lines(theme, &content_info, &total, &columns, width, true));
        cases.push(("modules, list".to_owned(), list));

        let mut table = table_lines(theme, &groups, &total, true);
        table.extend(keyword_block_lines(theme, &groups));
        cases.push(("modules, table".to_owned(), table));

        let mut boxed = boxed_lines(theme, &groups, &total, true);
        boxed.extend(keyword_block_lines(theme, &groups));
        cases.push(("modules, boxed".to_owned(), boxed));

        // '--top' is per module, so it cuts inside each one and not across the report
        config.view.top_n = Some(1);
        let groups = groups_from(&modules, &config);
        cases.push(("modules, table, top 1".to_owned(), table_lines(theme, &groups, &total, true)));

        config.view.top_n = None;
        config.view.sort_by = SortCriterion::Name;
        let groups = groups_from(&modules, &config);
        cases.push(("modules, table, sorted by name".to_owned(), table_lines(theme, &groups, &total, true)));

        // The rows of the matrix are the languages of the whole run, and each of them is three
        // physical rows, so the second case is the one where a module does not have the language
        // at all and only the middle of the three carries a dash
        config.view.sort_by = SortCriterion::Lines;
        let groups = groups_from(&modules, &config);
        cases.push(("modules, matrix".to_owned(),
                matrix_lines(theme, &groups, &sorted, &total, true)));
        cases.push(("modules, matrix, top 2".to_owned(),
                matrix_lines(theme, &groups, &sorted[..2], &total, true)));
        cases.push(("modules, matrix, no total".to_owned(),
                matrix_lines(theme, &groups, &sorted[..1], &total, false)));

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
            Group {name, languages: vec!["D".to_owned()], hidden: 0, per_language: content_info, total}
        }
        let groups = vec![group(Some("a"), &content_info, &total),
                group(None, &content_info, &total)];

        let theme = &Theme::default();
        let columns = Columns::of(&groups, &total);
        assert!(columns.name >= UNNAMED_MODULE_NAME.len());

        let lines = individual_lines(theme, &groups, &columns, columns.width(theme), false);
        // and the arrow of every row still lands in the same column
        let arrow_at = |needle: &str| lines.iter().find(|x| x.starts_with(needle)).map(|x| x.find("->").unwrap());
        assert_eq!(arrow_at("a "), arrow_at(UNNAMED_MODULE_NAME));
        assert_eq!(arrow_at("a "), arrow_at(&(LIST_INDENT.to_owned() + "D")));
    }

    // The golden calls each block function directly, so it says nothing about which list a block is
    // handed. That is what this holds, through the real entry point: the overview needs the uncut
    // list, since it folds the remainder into 'others' itself.
    #[test]
    fn every_layout_survives_a_top_that_hides_languages() {
        colored::control::set_override(false);

        let (_, content_info, _) = sample_data();
        let of_modules = |modules: Vec<ModuleResult>| RunResult {
            per_language: content_info.clone(), modules,
            total: Stats::new(23, 485500, 10934, 7643, 650, hashmap![]),
            faulty_files: Vec::new(), files_present: FilesPresent::default(), targets: Vec::new(), unreadable_dirs: Vec::new(), performance: mezura_core::Performance { duration_millis: 0, threads: mezura_core::Threads::new(1, 1) }};
        let single = || vec![ModuleResult {name: None, per_language: content_info.clone(),
                total: Stats::total_of(&content_info)}];

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

    // The counting has its own golden in tests/stats_golden.rs; this one is the presentation. What
    // is locked is the shape: alignment, widths, the wrapping of the keyword rows, the folding into
    // "others" and the apportionment of the bar. Colour is not, being turned off above.
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
    #[test]
    fn test_get_lines_percentages() {
        let ext_names = ["py".to_string(),"java".to_string(),"cs".to_string()];

        let per_language = hashmap!("cs".to_string() => Stats { lines: 100, ..Default::default() },
            "java".to_string() => Stats { lines: 100, ..Default::default() }, "py".to_string() => Stats { lines: 0, ..Default::default() });
        assert_eq!(vec![0f64,50f64,50f64], get_lines_percentages(&per_language, &ext_names));
        let per_language = hashmap!("cs".to_string() => Stats { lines: 0, ..Default::default() },
        "java".to_string() => Stats { lines: 0, ..Default::default() }, "py".to_string() => Stats { lines: 1, ..Default::default() });
        assert_eq!(vec![100f64,0f64,0f64], get_lines_percentages(&per_language, &ext_names));
        let per_language = hashmap!("cs".to_string() => Stats { lines: 20, ..Default::default() },
        "java".to_string() => Stats { lines: 20, ..Default::default() }, "py".to_string() => Stats { lines: 20, ..Default::default() });
        assert_eq!(vec![33.33f64,33.33f64,33.34f64], get_lines_percentages(&per_language, &ext_names));
        
        let ext_names = ["py".to_string(),"java".to_string(),"cs".to_string(),"rs".to_string()];

        let per_language = hashmap!("cs".to_string() => Stats { lines: 100, ..Default::default() },
            "java".to_string() => Stats { lines: 100, ..Default::default() }, "py".to_string() => Stats { lines: 0, ..Default::default() },
            "rs".to_string() => Stats { lines: 0, ..Default::default() });
        assert_eq!(vec![0f64,50f64,50f64,0f64], get_lines_percentages(&per_language, &ext_names));
        let per_language = hashmap!("cs".to_string() => Stats { lines: 100, ..Default::default() },
            "java".to_string() => Stats { lines: 100, ..Default::default() }, "py".to_string() => Stats { lines: 100, ..Default::default() },
            "rs".to_string() => Stats { lines: 0, ..Default::default() });
        assert_eq!(vec![33.33,33.33,33.33,0.01], get_lines_percentages(&per_language, &ext_names));
        let per_language = hashmap!("cs".to_string() => Stats { lines: 201, ..Default::default() },
            "java".to_string() => Stats { lines: 200, ..Default::default() }, "py".to_string() => Stats { lines: 200, ..Default::default() },
            "rs".to_string() => Stats { lines: 0, ..Default::default() });
        assert_eq!(vec![33.28,33.28,33.44,0.0], get_lines_percentages(&per_language, &ext_names));

        let ext_names = ["py".to_string(),"java".to_string(),"cs".to_string(),"rs".to_string(),"cpp".to_string()];

        let per_language = hashmap!("cs".to_string() => Stats { lines: 100, ..Default::default() },
            "java".to_string() => Stats { lines: 100, ..Default::default() }, "py".to_string() => Stats { lines: 0, ..Default::default() },
            "rs".to_string() => Stats { lines: 0, ..Default::default() }, "cpp".to_string() => Stats { lines: 0, ..Default::default() });
        assert_eq!(vec![0.0,50f64,50f64,0f64,0f64], get_lines_percentages(&per_language, &ext_names));
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

        let (folded_names, folded_per_language) = most_relevant_with_others_for_rest(
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

    // The rule that keeps every old entry from being accused of a change nobody recorded: a setting
    // the entry never wrote is unknown, not different.
    #[test]
    fn a_setting_an_entry_never_recorded_is_never_reported_as_changed() {
        let entry_of = |settings: Vec<(&str, &str)>| LogEntry {
            name: None, stats: Stats::new(1, 1, 1, 1, 0, hashmap![]), modules: Vec::new(), datetime: Local::now(),
            settings: settings.into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect(),
            splits_comments: true};
        let mut config = crate::config_manager::Configuration::new(vec!["./src".to_owned()]);
        config.engine.braces_as_code = true;

        // Everything the entry knows about matches, and it knows nothing of the rest
        let entry = entry_of(vec![("braces-as-code", "yes")]);
        assert!(settings_changed_since(&entry, &config, &[]).is_empty());
        assert!(modified_tag(&settings_changed_since(&entry, &config, &[])).is_empty());

        let entry = entry_of(vec![("braces-as-code", "no"), ("no-gitignore", "no")]);
        assert_eq!(vec!["braces-as-code"], settings_changed_since(&entry, &config, &[]));

        config.engine.no_gitignore = true;
        let changed = settings_changed_since(&entry, &config, &[]);
        assert_eq!(vec!["braces-as-code", "no-gitignore"], changed);
        assert!(modified_tag(&changed).contains("braces-as-code, no-gitignore"));

        // An entry with no settings block at all, which is every entry written before this existed
        assert!(settings_changed_since(&entry_of(vec![]), &config, &[]).is_empty());
    }

    // Two lists that have to hold the same names, kept in two files. Written as a comparison of the
    // lists rather than as a case per key, so that the next setting cannot be added to one of them
    // alone: a key the reader drops makes the progress section report no change beside numbers that
    // moved by everything.
    #[test]
    fn the_settings_written_to_a_log_are_the_settings_read_back() {
        let config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);
        let mut written = super::super::log::counting_settings(&config.engine, &[])
                .into_iter().map(|(key, _)| key).collect::<Vec<_>>();
        let mut accepted = SETTING_KEYS.to_vec();
        written.sort();
        accepted.sort();

        assert_eq!(written, accepted,
                "the log writes settings the reader drops, or accepts names nothing ever writes");
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

    #[test]
    fn test_difference_as_percentages() {
        assert_eq!("0",difference_as_signed_percentage_str_of_usize(100, 100));
        assert_eq!("-10",difference_as_signed_percentage_str_of_usize(100, 90));
        assert_eq!("+100",difference_as_signed_percentage_str_of_usize(100, 200));
        assert_eq!("+ <0.01",difference_as_signed_percentage_str_of_usize(22819, 22820));
        // A first run to compare against has nothing to have grown from, and 'inf%' is not a
        // reading of that
        assert_eq!("0",difference_as_signed_percentage_str_of_usize(0, 500));
    }

    // A whole entry writes all six figures and hides the question, so the fixture is a log cut in
    // the middle of a write: the one shape where a missing line could take its number from the
    // entry above it.
    #[test]
    fn a_figure_missing_from_an_entry_is_not_taken_from_the_entry_before_it() {
        let contents = super::super::log::extract_file_contents(&(FIXTURES_DIR.to_owned()+"logs/truncated")).unwrap();
        let log_entries = parse_N_previous_entries(&contents, 2);

        assert_eq!(2, log_entries.len());
        assert_eq!((10, 1000), (log_entries[0].stats.files, log_entries[0].stats.lines));

        assert_eq!(0, log_entries[1].stats.files, "the file count of the entry above it was reused");
        assert_eq!(0, log_entries[1].stats.lines, "the line count of the entry above it was reused");
        // And the arithmetic over what is left says nothing rather than a number the size of the
        // address space, which is what the same entry printed before 'extra_lines' saturated
        assert_eq!(0, log_entries[1].stats.extra_lines());
    }

    #[test]
    fn test_parse_N_previous_entries() {
        let contents = super::super::log::extract_file_contents(&(FIXTURES_DIR.to_owned()+"logs/test1")).unwrap();
        let log_entries = parse_N_previous_entries(&contents, 3);

        assert_eq!(10, log_entries[0].stats.files);
        assert_eq!(1000, log_entries[0].stats.lines);
        assert_eq!(100, log_entries[0].stats.code_lines);
        assert_eq!(900, log_entries[0].stats.extra_lines());
        assert_eq!(100000, log_entries[0].stats.bytes);
        assert_eq!(10000, log_entries[0].stats.average_size());
        let datetime: DateTime<Local> = chrono::DateTime::from_str("2021-09-12 16:42:00 +0300").unwrap();
        assert_eq!(datetime, log_entries[0].datetime);
        assert_eq!(Some("entry one".to_owned()),log_entries[0].name);

        assert_eq!(11, log_entries[1].stats.files);
        assert_eq!(1111, log_entries[1].stats.lines);
        assert_eq!(111, log_entries[1].stats.code_lines);
        assert_eq!(1000, log_entries[1].stats.extra_lines());
        assert_eq!(111100, log_entries[1].stats.bytes);
        assert_eq!(10100, log_entries[1].stats.average_size());
        let datetime: DateTime<Local> = chrono::DateTime::from_str("2021-09-12 16:23:50 +03:00").unwrap();
        assert_eq!(datetime, log_entries[1].datetime);
        assert_eq!(None,log_entries[1].name);

        assert_eq!(12, log_entries[2].stats.files);
        assert_eq!(1222, log_entries[2].stats.lines);
        assert_eq!(122, log_entries[2].stats.code_lines);
        assert_eq!(1100, log_entries[2].stats.extra_lines());
        assert_eq!(122200, log_entries[2].stats.bytes);
        assert_eq!(10183, log_entries[2].stats.average_size());
        let datetime: DateTime<Local> = chrono::DateTime::from_str("2021-09-12 04:01:56 +03:00").unwrap();
        assert_eq!(datetime, log_entries[2].datetime);
        assert_eq!(Some("entry three".to_owned()),log_entries[2].name);
    }

    fn result_of(total: Stats, modules: Vec<ModuleResult>) -> RunResult {
        RunResult {per_language: HashMap::new(), modules,
                total, faulty_files: Vec::new(), files_present: FilesPresent::default(),
                targets: Vec::new(), unreadable_dirs: Vec::new(), performance: mezura_core::Performance { duration_millis: 0, threads: mezura_core::Threads::new(1, 1) }}
    }

    #[test]
    fn test_log_creation_and_reading() -> std::io::Result<()> {
        std::fs::create_dir_all(SCRATCH_LOG_DIR)?;
        let test_log_dir = SCRATCH_LOG_DIR.to_owned() + "test2";
        if Path::new(&test_log_dir).exists() {
            std::fs::remove_file(&test_log_dir).unwrap();
        }

        let mut config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);
        config.view.set_log_option(LogOption::new(Some("test name".to_owned())));
        let result = result_of(Stats::new(10, 100, 1000, 100, 0, HashMap::new()), Vec::new());

        log_stats(&test_log_dir, &None, &result, &chrono::DateTime::from_str("2021-09-12 04:00:00 +03:00").unwrap(), &config).unwrap();

        let contents = super::super::log::extract_file_contents(&test_log_dir).unwrap();
        let log_entries = parse_N_previous_entries(&contents, 1);

        assert_eq!(10, log_entries[0].stats.files);
        assert_eq!(1000, log_entries[0].stats.lines);
        assert_eq!(100, log_entries[0].stats.code_lines);
        assert_eq!(900, log_entries[0].stats.extra_lines());
        assert_eq!(100, log_entries[0].stats.bytes);
        assert_eq!(10, log_entries[0].stats.average_size());
        assert_eq!(Some("test name".to_owned()),log_entries[0].name);
        assert!(log_entries[0].modules.is_empty());

        Ok(())
    }

    // The log is the one output that cannot be measured again: the trees those runs counted have
    // moved on. 'extract_file_contents' answers None both to "there is nothing" and to "I could not
    // read it", which are opposite instructions, so the refusal is what this holds.
    #[test]
    fn a_log_that_could_not_be_read_is_kept_rather_than_replaced_by_the_run() {
        std::fs::create_dir_all(SCRATCH_LOG_DIR).unwrap();
        let path = SCRATCH_LOG_DIR.to_owned() + "test_unreadable";
        let config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);
        let result = result_of(Stats::new(10, 100, 1000, 100, 0, HashMap::new()), Vec::new());
        let now = chrono::DateTime::from_str("2021-09-12 04:00:00 +03:00").unwrap();

        // What a run finds when the history is there and readable: the new entry, then all of it
        std::fs::write(&path, "===>\nAN ENTRY FROM BEFORE\n").unwrap();
        let history = super::super::log::extract_file_contents(&path);
        assert!(history.is_some());
        log_stats(&path, &history, &result, &now, &config).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("AN ENTRY FROM BEFORE"), "the history was dropped:\n{written}");

        // And what it finds when the same file cannot be read. The bytes below are not UTF-8, which
        // is one way in; a lock or an antivirus holding the file is the same answer through the
        // same door, and on this platform far likelier.
        std::fs::write(&path, [b"===>\nAN ENTRY FROM BEFORE\n".to_vec(), vec![0xFF, 0xFE, 0x80]].concat()).unwrap();
        let unreadable = super::super::log::extract_file_contents(&path);
        assert!(unreadable.is_none(), "the probe no longer reproduces an unreadable log");

        let refused = log_stats(&path, &unreadable, &result, &now, &config);
        assert!(refused.is_err(), "a log that could not be read was overwritten anyway");
        let after = std::fs::read(&path).unwrap();
        assert!(String::from_utf8_lossy(&after).contains("AN ENTRY FROM BEFORE"),
                "the entries were destroyed by a run that could not read them");
        // and nothing half written is left lying beside it, under the name this process would use
        assert!(!Path::new(&format!("{path}.writing.{}", std::process::id())).exists());

        std::fs::remove_file(&path).unwrap();
    }

    // The other side of the same guard. Emptying a log is an ordinary thing to want, and every
    // ordinary way of doing it on this platform leaves a newline behind rather than nothing at all,
    // which the refusal above must not read as a file it failed to parse.
    #[test]
    fn a_log_emptied_by_hand_is_written_again_rather_than_refused_forever() {
        std::fs::create_dir_all(SCRATCH_LOG_DIR).unwrap();
        let path = SCRATCH_LOG_DIR.to_owned() + "test_emptied";
        let config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);
        let result = result_of(Stats::new(10, 100, 1000, 100, 0, HashMap::new()), Vec::new());
        let now = chrono::DateTime::from_str("2021-09-12 04:00:00 +03:00").unwrap();

        for emptied in ["", "\r\n", "\n\n   \n", "   "] {
            std::fs::write(&path, emptied).unwrap();
            let history = super::super::log::extract_file_contents(&path);
            let written = log_stats(&path, &history, &result, &now, &config);
            assert!(written.is_ok(), "a log holding {emptied:?} was refused: {:?}", written.err());
            assert!(std::fs::read_to_string(&path).unwrap().contains("==="),
                    "a log holding {emptied:?} was left without the entry of this run");
        }

        std::fs::remove_file(&path).unwrap();
    }

    // The block is written under the totals of the entry it belongs to, which is already complete by
    // then, so what this holds is that it reaches the right entry and that its own figures stay out
    // of the ones above and below it
    #[test]
    fn the_modules_of_an_entry_are_read_back_and_never_reach_another_one() {
        std::fs::create_dir_all(SCRATCH_LOG_DIR).unwrap();
        let test_log_dir = SCRATCH_LOG_DIR.to_owned() + "test_modules";
        if Path::new(&test_log_dir).exists() {
            std::fs::remove_file(&test_log_dir).unwrap();
        }

        // An entry from before any of this existed, with no 'Comments' line of its own
        let older = "===>\n2021-09-12 04:00:00 +0300\nStats:\n    Files: 4\n    Lines: 400\n        Code: 300\n        \
Extra: 100\n    Total Size: 4000\n        Average Size: 1000\n\n\n";
        let module_of = |name: Option<&str>, lines: usize, code: usize, comments: usize| ModuleResult {
            name: name.map(str::to_owned), per_language: HashMap::new(),
            total: Stats::new(1, 10, lines, code, comments, HashMap::new())};
        let result = result_of(Stats::new(10, 5000, 1000, 700, 200, hashmap![]),
                vec![module_of(Some("frontend"), 600, 400, 150), module_of(None, 400, 300, 50)]);

        let mut config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);
        config.view.set_log_option(LogOption::new(None));
        log_stats(&test_log_dir, &Some(older.to_owned()), &result,
                &chrono::DateTime::from_str("2021-09-13 04:00:00 +03:00").unwrap(), &config).unwrap();

        let contents = super::super::log::extract_file_contents(&test_log_dir).unwrap();
        let entries = parse_N_previous_entries(&contents, 2);
        assert_eq!(2, entries.len());

        assert_eq!(1000, entries[0].stats.lines);
        assert_eq!(200, entries[0].stats.comment_lines);
        assert_eq!(vec!["frontend".to_owned(), UNNAMED_MODULE_NAME.to_owned()],
                entries[0].modules.iter().map(|x| x.name.clone()).collect::<Vec<_>>());
        assert_eq!((600, 400, 150), (entries[0].modules[0].lines, entries[0].modules[0].code_lines, entries[0].modules[0].comment_lines));
        assert_eq!((400, 300, 50), (entries[0].modules[1].lines, entries[0].modules[1].code_lines, entries[0].modules[1].comment_lines));

        // Nothing of the block above leaked into the entry below it, which is the one that would
        // otherwise be reported as having lost every comment it never counted
        assert_eq!(400, entries[1].stats.lines);
        assert!(entries[1].modules.is_empty());
        assert!(!entries[1].splits_comments);
        assert!(entries[0].splits_comments);

        // and the entry that carries the block is still complete when only one was asked for
        let only_one = parse_N_previous_entries(&contents, 1);
        assert_eq!(1, only_one.len());
        assert_eq!(2, only_one[0].modules.len());

        std::fs::remove_file(&test_log_dir).unwrap();
    }
}