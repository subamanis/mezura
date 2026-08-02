use crate::*;
use crate::config_manager::Layout;
use crate::theme::Theme;

type ColorFunc = Box<dyn Fn(&str) -> String>;

//the total number of vertical lines ( | ) that appear in the [-|||...|-] in the overview section
const NUM_OF_VERTICALS : usize = 50;

//the number of languages the overview shows before folding the rest into "others"
const OVERVIEW_LANGUAGES : usize = 3;

const OTHERS_NAME : &str = "others";

const BYTES_UNIT : &str = "Bytes";

// The keys of the settings block of a log entry, which are the command names that
// 'io_handler::counting_settings' writes. Kept as a list so that a line of the stats block can never
// be mistaken for one of them.
const SETTING_KEYS : [&str; 7] = [config_manager::DIRS, config_manager::EXCLUDE, config_manager::LANGUAGES,
        config_manager::EXCLUDE_LANGUAGES, config_manager::BRACES_AS_CODE, config_manager::SEARCH_IN_DOTTED,
        config_manager::NO_GITIGNORE];

//a language that is present but whose share rounds away to zero: shown as "<0.01", given no cell
const PRESENT_BUT_TINY : f64 = 0.001;

const TOTAL_NAME : &str = "Total";

//log file keys
const FILES         : &str  = "Files:";
const LINES         : &str  = "Lines:";
const CODE          : &str  = "Code:";
const COMMENTS      : &str  = "Comments:";
const EXTRA         : &str  = "Extra:";
const TOTAL_SIZE    : &str  = "Total Size:";
const AVERAGE_SIZE  : &str  = "Average Size:";
const MODULES       : &str  = "Modules:";

// One part of the run and the languages inside it, in the order '--sort' put them. A run that named
// no module has exactly one of these, with no name, and everything below prints what it always did.
struct Group<'a> {
    name: Option<&'a str>,
    languages: Vec<String>,
    hidden: usize,
    content_info_map: &'a HashMap<String, LanguageContentInfo>,
    languages_metadata_map: &'a HashMap<String, LanguageMetadata>,
    final_stats: &'a FinalStats
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
    let value_of = |module: &ModuleResult| match config.sort_by {
        SortCriterion::Files => module.final_stats.files,
        SortCriterion::Size => module.final_stats.bytes_size,
        SortCriterion::Lines => module.final_stats.lines,
        SortCriterion::Code => module.final_stats.code_lines,
        SortCriterion::Name => 0
    };
    let name_of = |module: &ModuleResult| module.name.clone().unwrap_or_else(|| UNNAMED_MODULE_NAME.to_owned()).to_lowercase();

    let mut modules = result.modules.iter().collect::<Vec<_>>();
    modules.sort_by(|a, b| value_of(b).cmp(&value_of(a)).then_with(|| name_of(a).cmp(&name_of(b))));

    modules.into_iter().map(|module| {
        let languages = get_sorted_language_names(&module.content_info_map, &module.languages_metadata_map, config.sort_by);
        let hidden = config.top_n.map_or(0, |top| languages.len().saturating_sub(top));
        Group {
            name: module.name.as_deref(),
            languages: languages[..languages.len() - hidden].to_vec(),
            hidden,
            content_info_map: &module.content_info_map,
            languages_metadata_map: &module.languages_metadata_map,
            final_stats: &module.final_stats
        }
    }).collect()
}

pub fn format_and_print_results(result: &RunResult, existing_log_content: &Option<String>,
        datetime_now: &DateTime<Local>, config: &Configuration)
{
    let RunResult {content_info_map, languages_metadata_map, final_stats, ..} = result;
    let groups = groups_of(result, config);

    // The rows of the matrix are the languages of the whole run and not of one module, so that is
    // also what '--top' cuts there. Everywhere else it cuts inside each module.
    //
    // Two lists and not one: the overview is handed the **uncut** one, because it folds everything
    // past its own limit into 'others' itself and needs to see what it is folding. Handing it the
    // cut one made it return early with a short name list next to the full maps, and the first
    // language it could not find in that list took the run down with it.
    let global_names = get_sorted_language_names(content_info_map, languages_metadata_map, config.sort_by);
    let matrix_hidden = config.top_n.map_or(0, |top| global_names.len().saturating_sub(top));
    let matrix_names = global_names[..global_names.len() - matrix_hidden].to_vec();

    // The list is cut, but the total below it still counts everything, so the reader is told what
    // is missing rather than left to wonder why the rows do not add up
    let hidden_languages = if config.layout == Layout::Matrix {matrix_hidden}
            else {groups.iter().map(|x| x.hidden).sum::<usize>()};

    let theme = theme::active();
    let columns = Columns::of(&groups, final_stats);
    let block_width = columns.width(theme);
    let should_print_keywords = !config.hidden.keywords;
    // The matrix has nothing to cross when no module was named, so it prints the table instead of a
    // grid of one column. It is not an error: the layout is presentation, and killing a run over how
    // its numbers would be shown costs the numbers. It is not silent either, which is what '--log'
    // and '--compare' already do when they are given with no configuration to work on: the reader
    // asked for one thing and is getting another, and has to be told why by something other than
    // their own guess.
    let mut layout = config.layout;
    if layout == Layout::Matrix && !is_grouped(&groups) {
        layout = Layout::Table;
        eprintln!("\n{}", theme::active().warning.paint("'--layout matrix' has nothing to cross, since no target was given a name, \
so the 'table' layout was printed. Use the modules feature to get a matrix: 'mezura frontend=./web backend=./api'."));
    }
    let is_table = layout != Layout::List;
    // With modules there is a sum of the module rows to be shown even when one language made all of
    // them, and without them a single language would only be repeated by a total under it
    let print_total = languages_metadata_map.len() > 1 || groups.len() > 1;

    match layout {
        Layout::Matrix => print_as_matrix(theme, &groups, &matrix_names, final_stats, config.sort_by, print_total, should_print_keywords),
        Layout::Boxed => print_as_boxed_table(theme, &groups, final_stats, print_total, should_print_keywords),
        Layout::Table => print_as_table(theme, &groups, final_stats, print_total, should_print_keywords),
        Layout::List => print_individually(theme, &groups, &columns, block_width, should_print_keywords)
    }

    if hidden_languages > 0 {
        let plural = if hidden_languages == 1 {"language"} else {"languages"};
        println!("\n{}", theme.note.paint(&format!("(+{hidden_languages} more {plural} hidden by --top {})", config.top_n.unwrap())));
    }

    if print_total {
        if !is_table {
            print_sum(theme, content_info_map, final_stats, &columns, block_width, should_print_keywords);
        }
        // The overview is the overview: it stays global however the details were grouped
        if !config.hidden.overview {
            print_visual_overview(&global_names, content_info_map, languages_metadata_map, final_stats, config);
        }
    }

    if !config.hidden.progress && let Some(content) = existing_log_content && config.compare_level != 0 {
        print_comparison_to_previous_runs(result, content, config, datetime_now);
    }
}


// One aligned row per language, no borders: whitespace alignment survives being pasted into a
// README or a ticket, which is what tokei, scc and cloc print for the same reason.
// The header cells reuse the label token of the quantity underneath them and the body cells its
// number token, so the table needs no styling of its own.
fn print_as_table(theme: &Theme, groups: &[Group], final_stats: &FinalStats,
        print_total: bool, should_print_keywords: bool)
{
    println!("{}.\n", theme.heading.paint("Details"));
    for line in table_lines(theme, groups, final_stats, print_total) {
        println!("{line}");
    }

    if should_print_keywords {
        print_keyword_block(theme, groups);
    }

    // The 'list' layout closes with a blank line of its own, this one has to say so
    println!();
}

// A module is a row of the same table and not a table of its own: comparing two of them by scrolling
// between two tables is exactly what the second axis exists to avoid. Its own figures are its share
// of the whole, and the languages indented under it take their share of it, so the two levels answer
// the two different questions that nesting them was for.
const GROUP_INDENT : &str = "  ";

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

// The column holds both, and the indentation says which is which. Without the change of heading the
// reader of an uncoloured paste is told that 'backend' is a language.
fn name_header(groups: &[Group]) -> &'static str {
    if is_grouped(groups) {"Module"} else {"Language"}
}

#[derive(PartialEq,Eq,Clone,Copy)]
enum RowKind {
    Module,
    Language,
    Total
}

fn table_lines(theme: &Theme, groups: &[Group], final_stats: &FinalStats, print_total: bool) -> Vec<String>
{
    // Every counted column carries its own percentage instead of one lonely 'Code%' column. The two
    // that compare languages ('Files' and 'Lines') take a share of the total, the two that describe
    // a language ('Code' and 'Comments') take a share of that language's own lines, which is what
    // the same numbers mean in the default layout.
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

        let (size, unit) = scaled(bytes);
        let (code_percentage, comment_percentage) = percentages(lines, code, comments);
        [name.to_owned(),
         with_seperators(files), share(files, total_files),
         with_seperators(lines), share(lines, total_lines),
         with_seperators(code), percent_cell(code_percentage),
         with_seperators(comments), percent_cell(comment_percentage),
         with_seperators(lines - code - comments),
         size_figure(size, unit) + " " + &theme.size_unit.paint(unit).to_string()]
    }

    let described = named_rows(groups, print_total);
    let rows = described.iter().map(|(cell, kind, group, language)| match kind {
            // A module's share is of the whole, a language's is of the module it is in: a module
            // reading 100% of itself would say nothing, which is the whole point of the two levels
            RowKind::Module => row_of(theme, cell, group.final_stats.files, group.final_stats.lines,
                    group.final_stats.code_lines, group.final_stats.comment_lines, group.final_stats.bytes_size,
                    final_stats.files, final_stats.lines),
            RowKind::Total => row_of(theme, cell, final_stats.files, final_stats.lines, final_stats.code_lines,
                    final_stats.comment_lines, final_stats.bytes_size, final_stats.files, final_stats.lines),
            RowKind::Language => {
                let name = language.unwrap();
                let content_info = group.content_info_map.get(name).unwrap();
                let metadata = group.languages_metadata_map.get(name).unwrap();
                row_of(theme, cell, metadata.files, content_info.lines, content_info.code_lines, content_info.comment_lines,
                        metadata.bytes, group.final_stats.files, group.final_stats.lines)
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
    // padding it on the left would push it further away on exactly the rows where its column happens
    // to be wider, which is what the total's '100.00%' did to every language row above it.
    let render = |cells: &[String], styles: &[&theme::Style; 11]| {
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

    for (row, (_, kind, _, _)) in rows.iter().zip(described.iter()) {
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


// Languages down, modules across, one number per cell. The nested table answers "what is inside the
// backend", read down a section; this one answers "how do the modules compare on the same language",
// read along a row, which is what you want when the subfolders are several answers to one problem
// rather than several parts of one thing.
//
// One number per cell is the whole constraint, so the cells carry whatever '--sort' is already
// ordering by: the axis you are comparing on is the one you chose, and a line under the heading says
// which it is rather than leaving the reader to guess.
fn print_as_matrix(theme: &Theme, groups: &[Group], languages: &[String], final_stats: &FinalStats,
        criterion: SortCriterion, print_total: bool, should_print_keywords: bool)
{
    println!("{}.\n", theme.heading.paint("Details"));
    for line in matrix_lines(theme, groups, languages, final_stats, criterion, print_total) {
        println!("{line}");
    }

    if should_print_keywords {
        // The keywords follow the rows and not each module's own cut of '--top'. Those are two
        // different cuts, global here and per module everywhere else, and left alone the block
        // named a language that had no row above it: the two halves of one report disagreeing.
        let shown = groups.iter().map(|group| Group {
            name: group.name,
            languages: languages.iter().filter(|x| group.content_info_map.contains_key(*x)).cloned().collect(),
            hidden: group.hidden,
            content_info_map: group.content_info_map,
            languages_metadata_map: group.languages_metadata_map,
            final_stats: group.final_stats
        }).collect::<Vec<_>>();
        print_keyword_block(theme, &shown);
    }
    println!();
}

// What a cell holds, and everything that depends on the criterion, in one place so that adding a
// sort criterion cannot leave the matrix showing something else than it is ordered by
fn measured_by(criterion: SortCriterion) -> (&'static str, fn(&FinalStats) -> usize) {
    match criterion {
        SortCriterion::Files => ("files", |stats| stats.files),
        SortCriterion::Size => ("size", |stats| stats.bytes_size),
        SortCriterion::Code => ("code lines", |stats| stats.code_lines),
        // Sorting by name says nothing about what to measure, so the cells stay on the quantity the
        // rest of the report leads with
        SortCriterion::Lines | SortCriterion::Name => ("lines", |stats| stats.lines)
    }
}

fn matrix_lines(theme: &Theme, groups: &[Group], languages: &[String], final_stats: &FinalStats,
        criterion: SortCriterion, print_total: bool) -> Vec<String>
{
    const GAP : usize = 4;
    const TOTAL_HEADER : &str = "Total";

    let (measured, of_stats) = measured_by(criterion);
    let value_of = |group: &Group, language: &str| match criterion {
        SortCriterion::Files => group.languages_metadata_map.get(language).map_or(0, |x| x.files),
        SortCriterion::Size => group.languages_metadata_map.get(language).map_or(0, |x| x.bytes),
        SortCriterion::Code => group.content_info_map.get(language).map_or(0, |x| x.code_lines),
        _ => group.content_info_map.get(language).map_or(0, |x| x.lines)
    };
    // A size is scaled the way it is everywhere else, or the cells would carry raw byte counts that
    // nothing else in the report prints
    let text_of = |value: usize| {
        if value == 0 {
            return None;
        }
        Some(if criterion == SortCriterion::Size {
            let (size, unit) = scaled(value);
            size_figure(size, unit) + " " + unit
        } else {
            with_seperators(value)
        })
    };

    // A zero is a language the module does not have at all, and in a matrix that is most of the
    // cells. Written out as '0' they crowd out the numbers that are the point of the layout.
    let cell_of = |value: usize| text_of(value).unwrap_or_else(|| "-".to_owned());

    let mut rows = Vec::with_capacity(languages.len() + 1);
    for language in languages {
        let mut cells = vec![language.clone()];
        cells.extend(groups.iter().map(|group| cell_of(value_of(group, language))));
        cells.push(cell_of(groups.iter().map(|group| value_of(group, language)).sum()));
        rows.push(cells);
    }
    // The total counts every language, including the ones '--top' left out of the rows above
    let mut totals = vec![TOTAL_HEADER.to_owned()];
    totals.extend(groups.iter().map(|group| cell_of(of_stats(group.final_stats))));
    totals.push(cell_of(of_stats(final_stats)));

    let headers = std::iter::once("Language".to_owned())
            .chain(groups.iter().map(|group| group.displayed_name().to_owned()))
            .chain(std::iter::once(TOTAL_HEADER.to_owned())).collect::<Vec<_>>();
    let widths = (0..headers.len()).map(|i| rows.iter().chain(std::iter::once(&totals))
            .map(|row| row[i].chars().count()).max().unwrap_or(0).max(headers[i].chars().count()))
            .collect::<Vec<_>>();

    // The name column is left aligned like a label and every figure is right aligned, so that a
    // column can be compared down and a language across
    let render = |cells: &[String], styles: &[&theme::Style]| {
        let mut line = String::with_capacity(120);
        for (i, cell) in cells.iter().enumerate() {
            let padding = " ".repeat(widths[i] - cell.chars().count());
            if i == 0 {
                line.push_str(&format!("{}{padding}", styles[i].paint(cell)));
            } else {
                line.push_str(&format!("{}{padding}{}", " ".repeat(GAP), styles[i].paint(cell)));
            }
        }
        line
    };

    let number_style = match criterion {
        SortCriterion::Files => &theme.files_number,
        SortCriterion::Size => &theme.total_size_number,
        SortCriterion::Code => &theme.code_number,
        _ => &theme.lines_number
    };
    let mut header_styles = vec![&theme.details_language_header];
    header_styles.extend(groups.iter().map(|_| &theme.details_module));
    header_styles.push(&theme.details_total);
    let body_styles = std::iter::once(&theme.details_language_name)
            .chain((0..headers.len() - 1).map(|_| number_style)).collect::<Vec<_>>();
    let total_styles = std::iter::once(&theme.details_total)
            .chain((0..headers.len() - 1).map(|_| number_style)).collect::<Vec<_>>();

    let table_width = widths.iter().sum::<usize>() + GAP * (headers.len() - 1);
    let mut lines = vec![theme.note.paint(&format!("every cell is {measured}")).to_string(), String::new(),
            render(&headers, &header_styles)];
    lines.extend(rows.iter().map(|row| render(row, &body_styles)));
    // Suppressed on the same terms as everywhere else. One module and one language leaves nothing
    // for it to add up, and here it would repeat the single row twice over, since the matrix
    // already carries a Total column next to it.
    if print_total {
        lines.push(theme.separator.paint(&"-".repeat(table_width)).to_string());
        lines.push(render(&totals, &total_styles));
    }

    lines
}


// Their own block instead of a trailing column, whose width varies by nature and would destroy the
// alignment that is the whole point of a table. Not aligned by position, though: a column of the
// table means one thing all the way down, while the first keyword of one language and the first of
// the next are unrelated, so aligning them promises a comparison that does not exist.
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

// Nested the same way the table is, not out of symmetry but because ungrouped keywords under a
// grouped table cannot be read: 'Rust structs: 210' with no way to tell whose they are. A language
// appears only under the modules it is in, which is what keeps the block from growing by the product
// of the two, and '--hide keywords' was already the way out of it.
fn keyword_block_lines(theme: &Theme, groups: &[Group]) -> Vec<String> {
    const GAP : usize = 3;

    let grouped = is_grouped(groups);
    let rows = groups.iter().map(|group| (group, group.languages.iter().filter_map(|name| {
            let keywords = get_keywords_as_str(theme, &group.content_info_map.get(name).unwrap().keyword_occurences, 0, usize::MAX);
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


// The same figures as the borderless table, in a drawn frame. Each number and its percentage share
// one cell here, since the borders already do the grouping that the tight gap does over there, and
// that brings the whole thing down from eleven columns to seven.
fn print_as_boxed_table(theme: &Theme, groups: &[Group], final_stats: &FinalStats,
        print_total: bool, should_print_keywords: bool)
{
    println!("{}.
", theme.heading.paint("Details"));
    for line in boxed_lines(theme, groups, final_stats, print_total) {
        println!("{line}");
    }

    if should_print_keywords {
        print_keyword_block(theme, groups);
    }
    println!();
}

fn boxed_lines(theme: &Theme, groups: &[Group], final_stats: &FinalStats, print_total: bool) -> Vec<String>
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

        let (size, unit) = scaled(bytes);
        let (code_percentage, comment_percentage) = percentages(lines, code, comments);
        (name.to_owned(), [
            cell(with_seperators(files), share(files, total_files)),
            cell(with_seperators(lines), share(lines, total_lines)),
            cell(with_seperators(code), percent_text(code_percentage) + "%"),
            cell(with_seperators(comments), percent_text(comment_percentage) + "%"),
            cell(with_seperators(lines - code - comments), String::new()),
            cell(size_figure(size, unit) + " " + &theme.size_unit.paint(unit).to_string(), String::new())])
    }

    let described = named_rows(groups, print_total);
    let rows = described.iter().map(|(cell, kind, group, language)| match kind {
            RowKind::Module => row_of(theme, cell, group.final_stats.files, group.final_stats.lines,
                    group.final_stats.code_lines, group.final_stats.comment_lines, group.final_stats.bytes_size,
                    final_stats.files, final_stats.lines),
            RowKind::Total => row_of(theme, cell, final_stats.files, final_stats.lines, final_stats.code_lines,
                    final_stats.comment_lines, final_stats.bytes_size, final_stats.files, final_stats.lines),
            RowKind::Language => {
                let name = language.unwrap();
                let content_info = group.content_info_map.get(name).unwrap();
                let metadata = group.languages_metadata_map.get(name).unwrap();
                row_of(theme, cell, metadata.files, content_info.lines, content_info.code_lines, content_info.comment_lines,
                        metadata.bytes, group.final_stats.files, group.final_stats.lines)
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

    // Hardcoded while this layout is being looked at. If it stays, the three want their own tokens,
    // which needs FEAT-17 first: they mean nothing in the other two layouts.
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

    // The two lines that bound the body, under the header and above the total, belong to the frame
    // rather than to the striping: they are solid and take its shade, which is also what makes the
    // first and the last crossing of every column bright. Only the lines between two languages are
    // dashed, and those alternate.
    // A module opens a section, so the line above it is solid like the two that bound the body: the
    // dashed ones are what separate the rows inside one section from each other
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


// The theme listing runs before a configuration exists, so it cannot go through 'theme::active()'.
// It asks for the real rows of one made-up language instead, built by the same functions a run uses,
// so that the preview cannot drift from what will actually be printed. It follows the layout in
// effect for the same reason, and so that the listing stays one size however many layouts exist.
// The figures are constants, so that every theme is judged against the same row.
pub fn theme_sample_rows(theme: &Theme, layout: Layout) -> Vec<String> {
    const NAME    : &str   = "Rust";
    const FILES   : usize  = 1_284;
    const LINES   : usize  = 96_512;
    const CODE    : usize  = 71_004;
    const COMMENTS: usize  = 12_838;
    const BYTES   : usize  = 3_412_500;

    let keywords = hashmap!("structs".to_owned() => 284usize, "traits".to_owned() => 31);
    let content_info_map = hashmap!(NAME.to_owned() => LanguageContentInfo::new(LINES, CODE, COMMENTS, keywords.clone()));
    let metadata_map = hashmap!(NAME.to_owned() => LanguageMetadata::new(FILES, BYTES));
    let final_stats = FinalStats::calculate(&content_info_map, &metadata_map);
    let groups = vec![Group {name: None, languages: vec![NAME.to_owned()], hidden: 0,
            content_info_map: &content_info_map, languages_metadata_map: &metadata_map, final_stats: &final_stats}];

    // The two tables keep their keywords in a block of their own, so the sample has to ask for it or
    // the keyword tokens would go unshown in exactly the two layouts that are now the common ones.
    // One language, so there is nothing for a total to add up: it would only repeat the row above it.
    let with_keywords = |mut lines: Vec<String>| {
        lines.push(String::new());
        lines.extend(keyword_block_lines(theme, &groups));
        lines
    };
    match layout {
        Layout::Table => with_keywords(table_lines(theme, &groups, &final_stats, false)),
        Layout::Boxed => with_keywords(boxed_lines(theme, &groups, &final_stats, false)),
        // The matrix has no second axis to show for one made-up language of one unnamed module, and
        // the tokens it paints are the ones the table already previews
        Layout::Matrix => with_keywords(table_lines(theme, &groups, &final_stats, false)),
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


fn print_individually(theme: &Theme, groups: &[Group], columns: &Columns, block_width: usize, should_print_keywords: bool)
{
    print_lines(&individual_lines(theme, groups, columns, block_width, should_print_keywords));
}

fn individual_lines(theme: &Theme, groups: &[Group], columns: &Columns, block_width: usize,
     should_print_keywords: bool) -> Vec<String>
{
    let grouped = is_grouped(groups);
    let indent = if grouped {GROUP_INDENT} else {""};
    let mut lines = vec![format!("{}.", theme.heading.paint("Details")), String::new()];

    for (position, group) in groups.iter().enumerate() {
        if position > 0 {
            lines.push(String::new());
        }
        if grouped {
            let name = group.displayed_name();
            let stats = group.final_stats;
            lines.push(columns.files_row(theme, stats.files,
                    &size_text(theme, stats.bytes_size, stats.bytes_average_size), block_width));
            lines.push(columns.breakdown_row(theme, &theme.details_module.paint(name).to_string(),
                    name.chars().count(), stats.lines, stats.code_lines, stats.comment_lines));
        }

        for (i, lang_name) in group.languages.iter().enumerate() {
            let content_info = group.content_info_map.get(lang_name).unwrap();
            let metadata = group.languages_metadata_map.get(lang_name).unwrap();
            if grouped || i > 0 {
                lines.push(String::new());
            }

            lines.push(columns.files_row(theme, metadata.files,
                    &size_text(theme, metadata.bytes, metadata.bytes / metadata.files), block_width));
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

// Kept on both sides of the arrow, so the longest language name still has room around it
const NAME_GAP : usize = 3;

impl Columns {
    fn of(groups: &[Group], final_stats: &FinalStats) -> Self
    {
        let grouped = is_grouped(groups);
        let indent = if grouped {GROUP_INDENT.len()} else {0};
        let len_of = |value: usize| with_seperators(value).len();
        let mut columns = Columns {
            name: TOTAL_NAME.len(),
            headline: len_of(final_stats.files).max(len_of(final_stats.lines)),
            code: len_of(final_stats.code_lines),
            comments: len_of(final_stats.comment_lines),
            extra: len_of(final_stats.extra_lines)
        };

        // The total holds the largest of every column, except when --top hid the language that made
        // it so, which is why the shown ones are measured too instead of assumed smaller
        for group in groups {
            // Every group prints its name once there is grouping, and that includes the leftovers.
            // Measuring only the named ones left '(unnamed)' wider than the column that has to hold it,
            // and the padding of its row is a subtraction that then goes below zero.
            if grouped {
                columns.name = columns.name.max(group.displayed_name().chars().count());
            }
            for name in &group.languages {
                let content_info = group.content_info_map.get(name).unwrap();
                columns.name = columns.name.max(name.chars().count() + indent);
                columns.headline = columns.headline.max(len_of(group.languages_metadata_map.get(name).unwrap().files))
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

    // The theme arrives as an argument rather than being read from 'theme::active()', because
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

    // Rendered once to be measured and again to be printed, which costs nothing once per run and
    // keeps the width honest instead of derived from a formula that can fall behind
    fn width(&self, theme: &Theme) -> usize {
        widest_visible_line(&self.breakdown_row(theme, "", 0, 0, 0, 0))
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


fn print_sum(theme: &Theme, content_info_map: &HashMap<String,LanguageContentInfo>, final_stats: &FinalStats, columns: &Columns,
        block_width: usize, should_print_keywords: bool)
{
    print_lines(&sum_lines(theme, content_info_map, final_stats, columns, block_width, should_print_keywords));
}

fn sum_lines(theme: &Theme, content_info_map: &HashMap<String,LanguageContentInfo>, final_stats: &FinalStats, columns: &Columns,
        block_width: usize, should_print_keywords: bool) -> Vec<String>
{
    // The separator spans the block, which every row of the details section already fits exactly
    let mut lines = vec![
        format!("{} ",theme.separator.paint(&"-".repeat(block_width))),
        columns.files_row(theme, final_stats.files,
                &size_text(theme, final_stats.bytes_size, final_stats.bytes_average_size), block_width),
        columns.breakdown_row(theme, &theme.details_total.paint(TOTAL_NAME).to_string(),
                TOTAL_NAME.len(), final_stats.lines, final_stats.code_lines, final_stats.comment_lines)];

    if should_print_keywords {
        let keywords_line = get_keywords_as_str(theme, &create_keyword_sum_map(content_info_map), columns.words_start(), block_width);
        if !keywords_line.is_empty() {
            lines.push(keywords_line);
        }
    }
    lines.push(String::new());

    lines
}

//                                    OVERVIEW
//
// Files:    47% java - 32% cs - 21% py        [-||||||||||||||||||||||||||||||||||||||||||||||||||] 
//
// Lines: ...
//
// Size : ...
fn print_visual_overview(sorted_language_names: &[String], content_info_map: &HashMap<String, LanguageContentInfo>,
        languages_metadata_map: &HashMap<String, LanguageMetadata>, final_stats: &FinalStats, config: &Configuration)
{
    print_lines(&overview_lines(sorted_language_names, content_info_map, languages_metadata_map, final_stats, config));
}

fn overview_lines(sorted_language_names: &[String], content_info_map: &HashMap<String, LanguageContentInfo>,
        languages_metadata_map: &HashMap<String, LanguageMetadata>, final_stats: &FinalStats, config: &Configuration) -> Vec<String>
{
    // The function itself decides whether there is anything to fold
    let (sorted_language_vec, content_info_map, languages_metadata_map) =
            most_relevant_with_others_for_rest(sorted_language_names, content_info_map, languages_metadata_map, final_stats, config.top_n);
    let (sorted_language_vec, content_info_map, languages_metadata_map) =
            (&sorted_language_vec, &content_info_map, &languages_metadata_map);

    // 'others' takes its style by identity and not by position, because --top moves it: with
    // --top 2 it sits third and used to steal the slot meant for the third language.
    let slots = theme::active().language_slots();
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

    let files_percentages = get_files_percentages(languages_metadata_map, sorted_language_vec);
    let lines_percentages = get_lines_percentages(content_info_map, sorted_language_vec);
    let sizes_percentages = get_sizes_percentages(languages_metadata_map, sorted_language_vec);

    let files_verticals = if config.hidden.bar {vec![]} else{get_num_of_verticals(&files_percentages, NUM_OF_VERTICALS)};
    let lines_verticals = if config.hidden.bar {vec![]} else{get_num_of_verticals(&lines_percentages, NUM_OF_VERTICALS)};
    let size_verticals = if config.hidden.bar {vec![]} else{get_num_of_verticals(&sizes_percentages, NUM_OF_VERTICALS)};

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

    vec![format!("{}.", theme::active().heading.paint("Overview")), String::new(),
         files_line, String::new(), lines_line, String::new(), size_line, String::new()]
}

fn print_lines(lines: &[String]) {
    for line in lines {
        println!("{line}");
    }
}

// The settings this run counted with, against the ones the entry recorded. A setting the entry never
// wrote is left alone rather than reported as changed, which is what keeps entries from older
// versions from being accused of a difference nobody can know about.
fn settings_changed_since(entry: &LogEntry, config: &Configuration) -> Vec<&'static str> {
    io_handler::counting_settings(config).into_iter()
            .filter(|(key, value)| entry.settings.iter().any(|(k, v)| k == key && v != value))
            .map(|(key, _)| key)
            .collect()
}

// Placed at the end of the line of the entry it belongs to, because it is a statement about that
// entry and not about the run. Nothing is printed when nothing changed: a mark on every line in the
// ordinary case is noise, and the reader would stop seeing it exactly when it appears.
fn modified_tag(changed: &[&'static str]) -> String {
    if changed.is_empty() {
        return String::new();
    }

    let theme = theme::active();
    format!("   {} {}", theme.progress_modified.paint("modified:"),
            theme.progress_modified_field.paint(&changed.join(", ")))
}

// One line per module under the line of the entry, and deliberately narrower than it: Files and
// Extra stay on the total, where the full breakdown belongs, because what is asked of a module is
// which part of it moved. With every column repeated, one entry is five wide lines and '--compare 3'
// stops being readable.
fn module_comparison_lines(entry: &LogEntry, groups: &[Group]) -> String {
    let theme = theme::active();
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
        let now = groups.iter().find(|x| x.displayed_name() == name).map(|x| x.final_stats);
        let then = entry.modules.iter().find(|x| &x.name == name);
        // A module compared against nothing would read '+100%', which is false: it did not grow, it
        // started being counted on its own. The ones that are not in both are named as what they are.
        let tail = match (now, then) {
            (Some(now), Some(then)) => {
                let cell = |style: &theme::Style, value: usize, then: usize, width: usize| {
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
    let theme = theme::active();
    if percentage.starts_with('+') {
        theme.progress_up.paint(percentage)
    } else if percentage.starts_with('-') {
        theme.progress_down.paint(percentage)
    } else {
        theme.progress_same.paint(percentage)
    }
}

fn print_comparison_to_previous_runs(result: &RunResult, log_content: &str, config: &Configuration, datetime_now: &DateTime<Local>) {
    println!("\n{}.\n", theme::active().heading.paint("Progress"));

    let final_stats = &result.final_stats;
    let log_entries = parse_N_previous_entries(log_content, config.compare_level);
    // Silent until used: a run that named no module says nothing about them here either, and the
    // 'modified: dirs' tag is what already reports that the targets are not the ones they were
    let groups = if result.has_modules() {groups_of(result, config)} else {Vec::new()};

    let mut comparison_str = String::with_capacity(200);
    for entry in log_entries.iter() {
        let duration = datetime_now.signed_duration_since(entry.datetime);
        let (days, hours, minutes) = split_minutes_to_D_H_M(duration.num_minutes());
        let arrow = theme::active().progress_entry.paint("->");
        let tag = modified_tag(&settings_changed_since(entry, config));
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
                theme::active().comments_number.paint(&with_seperators(entry.stats.comment_lines)), color_percentage(&difference_as_signed_percentage_str_of_usize(entry.stats.comment_lines, final_stats.comment_lines)),
                theme::active().extra_number.paint(&with_seperators(entry.stats.extra_lines)), color_percentage(&difference_as_signed_percentage_str_of_usize(entry.stats.extra_lines, final_stats.extra_lines)))
        } else {
            format!("Non-code: {} (logged before comments were counted separately)",
                theme::active().extra_number.paint(&with_seperators(entry.stats.extra_lines)))
        };
        comparison_str.push_str(&format!("     Files: {}({}%) Lines: {}({}%) {{Code: {}({}%), {}}}\n",
                theme::active().files_number.paint(&with_seperators(entry.stats.files)), color_percentage(&difference_as_signed_percentage_str_of_usize(entry.stats.files, final_stats.files)),
                theme::active().lines_number.paint(&with_seperators(entry.stats.lines)), color_percentage(&difference_as_signed_percentage_str_of_usize(entry.stats.lines, final_stats.lines)),
                theme::active().code_number.paint(&with_seperators(entry.stats.code_lines)), color_percentage(&difference_as_signed_percentage_str_of_usize(entry.stats.code_lines, final_stats.code_lines)),
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
    let (difference, sign) = if newer > older {(newer-older, "+".to_owned())} else if older > newer {(older-newer, "-".to_owned())} else {(0,String::new())};
    let mut percentage = (difference as f64 / older as f64) * 100.0;
    let mut prefix_symbol = "";
    if percentage > 0.0 && percentage < 0.01 {
        prefix_symbol = " <";
        percentage = 0.01;
    }
    
    sign + prefix_symbol + &with_decimal_separator(round_2(percentage).to_string())
}


#[cfg(test)]
fn difference_as_signed_percentage_str_of_f64(older: f64, newer: f64) -> String {
    let (difference, sign) = if newer > older {(newer-older, "+".to_owned())} else if older > newer {(older-newer, "-".to_owned())} else {(0.0,String::new())};
    let mut percentage = (difference / older) * 100.0;
    if percentage > 0.0 && percentage < 0.01 {
        percentage = 0.01;
    }

    sign + &round_2(percentage).to_string()
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
    stats: FinalStats,
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
    let (mut files, mut lines, mut code_lines, mut comment_lines, mut extra_lines, mut bytes_size) = (0, 0, 0, 0, 0, 0);
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
            extra_lines = value.trim().parse::<usize>().unwrap();
        } else if let Some(value) = line.strip_prefix(TOTAL_SIZE) {
            bytes_size = value.trim().parse::<usize>().unwrap();
        } else if let Some(value) = line.strip_prefix(AVERAGE_SIZE) {
            let bytes_average_size = value.trim().parse::<usize>().unwrap();
            let stats = FinalStats::new_extended(files, lines, code_lines, comment_lines, extra_lines, bytes_size, bytes_average_size);
            log_entries.push(LogEntry{name: entry_name.clone(), stats, modules: Vec::new(), datetime,
                    settings: settings.clone(), splits_comments});
            (comment_lines, splits_comments) = (0, false);
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

// Both the plain text and the painted text of every entry, in one place, because the two layouts
// need different things from it: one measures to wrap, the other measures to align by column
// The name and the count apart, because the two layouts put different things between them: one
// writes 'structs: 12' as it comes, the other pads the name so the colons of a column line up
fn keyword_entries(keyword_occurencies: &HashMap<String,usize>) -> Vec<(String, String)> {
    let mut sorted_keywords = keyword_occurencies.iter().collect::<Vec<_>>();
    sorted_keywords.sort_unstable_by_key(|(name,_)| name.as_str());

    sorted_keywords.into_iter().map(|(name, occurancies)| (name.to_owned(), with_seperators(*occurancies))).collect()
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

fn create_keyword_sum_map(content_info_map: &HashMap<String,LanguageContentInfo>) -> HashMap<String,usize> {
    let mut collective_keywords_map : HashMap<String,usize> = HashMap::new();
    for content_info in content_info_map.values() {
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

// The language rows and the total go through the same formatting, so a value on the border of a
// unit cannot be reported as MBs on one line and KBs on another
fn scaled(value: usize) -> (f64, &'static str) {
    if value > 1_000_000_000 {(value as f64 / 1_000_000_000f64, "GBs")}
    else if value > 1_000_000 {(value as f64 / 1_000_000f64, "MBs")}
    else if value > 1000 {(value as f64 / 1000f64, "KBs")}
    else {(value as f64, BYTES_UNIT)}
}

// A count of bytes is a whole number, so '430.0 Bytes' would be claiming a precision the figure does
// not have. Only a scaled one is divided, and only a divided one has a decimal to show.
fn size_figure(value: f64, unit: &str) -> String {
    if unit == BYTES_UNIT {
        with_seperators(value as usize)
    } else {
        with_decimal_separator(format!("{value:.1}"))
    }
}

fn size_text(theme: &Theme, total_bytes: usize, average_bytes: usize) -> String {
    let (total, total_unit) = scaled(total_bytes);
    let (average, average_unit) = scaled(average_bytes);
    format!("{} {} {} - {} {} {}",
            theme.total_size_number.paint(&size_figure(total, total_unit)),
            theme.size_unit.paint(total_unit), theme.total_size_label.paint("total"),
            theme.avg_size_number.paint(&size_figure(average, average_unit)),
            theme.size_unit.paint(average_unit), theme.avg_size_label.paint("average"))
}

// A language that is present but rounds to 0.00 would read as absent, while the bar still shows a
// cell for it because of the minimum-one rule. '<0.01' is the same convention the progress section
// already uses for tiny differences. Comparing the formatted text rather than the number keeps this
// independent of how the formatter rounds a halfway value.
fn percent_text(value: f64) -> String {
    let text = format!("{value:.2}");
    with_decimal_separator(if value > 0.0 && text == "0.00" { "<0.01".to_owned() } else { text })
}

// The '%' is painted with the number: leaving it outside made it keep the default colour while
// the digits next to it were faded
fn percent(theme: &Theme, value: f64) -> ColoredString {
    theme.percent.paint(&(percent_text(value) + "%"))
}

// The overview's percentages are the datum of that section rather than an annotation on a count,
// so they are not the ones that were faded
fn overview_percent(theme: &Theme, value: f64) -> ColoredString {
    theme.overview_percent.paint(&(percent_text(value) + "%"))
}

fn percentages(lines: usize, code_lines: usize, comment_lines: usize) -> (f64, f64) {
    if lines == 0 {
        return (0f64, 0f64);
    }
    (code_lines as f64 / lines as f64 * 100f64, comment_lines as f64 / lines as f64 * 100f64)
}



// Ties are broken by name rather than left to the iteration order of the maps, which would make
// the printed order differ between runs on the very projects where languages are evenly matched
pub(crate) fn get_sorted_language_names(content_info_map: &HashMap<String, LanguageContentInfo>,
        languages_metadata_map: &HashMap<String, LanguageMetadata>, criterion: SortCriterion) -> Vec<String>
{
    let value_of = |name: &String| match criterion {
        SortCriterion::Files => languages_metadata_map.get(name).map_or(0, |x| x.files),
        SortCriterion::Size => languages_metadata_map.get(name).map_or(0, |x| x.bytes),
        SortCriterion::Lines => content_info_map.get(name).map_or(0, |x| x.lines),
        SortCriterion::Code => content_info_map.get(name).map_or(0, |x| x.code_lines),
        SortCriterion::Name => 0
    };

    let mut names = languages_metadata_map.keys().cloned().collect::<Vec<_>>();
    if criterion == SortCriterion::Name {
        names.sort_by_key(|x| x.to_lowercase());
    } else {
        names.sort_by(|a, b| value_of(b).cmp(&value_of(a)).then_with(|| a.to_lowercase().cmp(&b.to_lowercase())));
    }

    names
}

// Largest remainder apportionment. Every language takes the whole part of its exact share, a
// language with any presence at all keeps at least one cell so that it cannot vanish from the bar,
// and the remaining cells go one at a time to whichever language sits furthest from its exact
// share. Exact by construction, in both directions: the minimum-one rule can push the total over
// the target (97/1/1/1 wants 51 cells), and that is corrected without ever emptying a language.
fn get_num_of_verticals(percentages: &[f64], width: usize) -> Vec<usize> {
    let exact = percentages.iter().map(|x| x * width as f64 / 100.0).collect::<Vec<_>>();
    let mut verticals = percentages.iter().zip(exact.iter())
            .map(|(percentage, exact)| if *percentage < 0.01 {0} else {(*exact as usize).max(1)})
            .collect::<Vec<_>>();

    let mut sum = verticals.iter().sum::<usize>();

    while sum < width {
        let distance_below = |i: &usize| exact[*i] - verticals[*i] as f64;
        let furthest_below = (0..verticals.len()).filter(|i| percentages[*i] >= 0.01)
                .max_by(|a, b| distance_below(a).total_cmp(&distance_below(b)));
        match furthest_below {
            Some(i) => verticals[i] += 1,
            None => break
        }
        sum += 1;
    }

    // The cell comes off whoever holds the most, not off whoever is closest to its exact share.
    // What matters in a bar is relative fidelity: one cell missing from a language holding 96 is
    // invisible, while the same cell taken from a language holding 3 understates it by a third.
    while sum > width {
        let largest = (0..verticals.len()).filter(|i| verticals[*i] > 1)
                .max_by(|a, b| verticals[*a].cmp(&verticals[*b]).then(exact[*a].total_cmp(&exact[*b])));
        match largest {
            Some(i) => verticals[i] -= 1,
            None => break
        }
        sum -= 1;
    }

    verticals
}

fn create_overview_line(prefix: &str, percentages: &[f64], verticals: &[usize], languages_name: &[String],
        color_func_vec: &[ColorFunc], bar_func_vec: &[ColorFunc], percent_widths: &[usize], config: &Configuration) -> String
{
    let theme = theme::active();
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

    if !config.hidden.bar {
        add_verticals_str(&mut line, verticals, bar_func_vec, config.bar_thickness.character());
    }

    line
}

fn add_verticals_str(line: &mut String, files_verticals: &[usize], color_func_vec: &[ColorFunc], character: &str) {
    let theme = theme::active();
    line.push_str("   ");
    line.push_str(&theme.bar_frame.paint("[-").to_string());
    for (i,verticals) in files_verticals.iter().enumerate() {
        line.push_str(&color_func_vec[i](character).repeat(*verticals));
    }
    line.push_str(&theme.bar_frame.paint("-]").to_string());
}

// Returns its own view of the data instead of folding the caller's maps in place. "others" is a
// creature of the overview and of nothing else, and a result that has been printed once has to still
// be the result: the in-place version left the caller holding three languages and a fiction.
fn most_relevant_with_others_for_rest(sorted_language_names: &[String],
        content_info_map: &HashMap<String, LanguageContentInfo>,
        languages_metadata_map: &HashMap<String, LanguageMetadata>,
        final_stats: &FinalStats, top_n: Option<usize>)
-> (Vec<String>, HashMap<String, LanguageContentInfo>, HashMap<String, LanguageMetadata>)
{
    fn get_files_lines_size(content_info_map: &HashMap<String, LanguageContentInfo>,
        languages_metadata_map: &HashMap<String, LanguageMetadata>) -> (usize,usize,usize) 
   {
       let (mut files, mut lines, mut size) = (0,0,0);
       content_info_map.iter().for_each(|x| lines += x.1.lines);
       languages_metadata_map.iter().for_each(|x| {files += x.1.files; size += x.1.bytes});
       (files, lines, size) 
   }

    // --top never widens the overview past its own cap, it only narrows it, so that asking for the
    // top 2 does not leave three languages sitting in the bar
    let to_keep = OVERVIEW_LANGUAGES.min(top_n.unwrap_or(OVERVIEW_LANGUAGES));
    if sorted_language_names.len() <= to_keep + 1 {
        return (sorted_language_names.to_vec(), content_info_map.clone(), languages_metadata_map.clone());
    }

    let mut sorted_language_names = sorted_language_names[..to_keep].to_vec();
    sorted_language_names.push(OTHERS_NAME.to_owned());
    let mut content_info_map = content_info_map.clone();
    let mut languages_metadata_map = languages_metadata_map.clone();
    content_info_map.retain(|x,_| sorted_language_names.contains(x));
    languages_metadata_map.retain(|x,_| sorted_language_names.contains(x));

    let (relevant_files, relevant_lines, relevant_size) = get_files_lines_size(&content_info_map, &languages_metadata_map);
    let (other_files, other_lines, other_size) =
        (final_stats.files - relevant_files, final_stats.lines - relevant_lines,
         final_stats.bytes_size - relevant_size);

    //We only care about the total lines of code for the "others" field, this is the only field involved with calculations
    content_info_map.insert(OTHERS_NAME.to_string(), LanguageContentInfo::dummy(other_lines));
    languages_metadata_map.insert(OTHERS_NAME.to_string(), LanguageMetadata::new(other_files, other_size));

    (sorted_language_names, content_info_map, languages_metadata_map)
}


fn get_files_percentages(languages_metadata_map: &HashMap<String,LanguageMetadata>, sorted_language_names: &[String]) -> Vec<f64> {
    let mut language_files = [0].repeat(languages_metadata_map.len());
    languages_metadata_map.iter().for_each(|e| {
        let pos = sorted_language_names.iter().position(|name| name == e.0).unwrap();
        language_files[pos] = e.1.files;
    });
    
    get_percentages(&language_files)
}

fn get_lines_percentages(content_info_map: &HashMap<String,LanguageContentInfo>, languages_name: &[String]) -> Vec<f64> {
    let mut language_lines = [0].repeat(content_info_map.len());
    content_info_map.iter().for_each(|e| {
        let pos = languages_name.iter().position(|name| name == e.0).unwrap();
        language_lines[pos] = e.1.lines;
    });

    get_percentages(&language_lines)
}

fn get_sizes_percentages(languages_metadata_map: &HashMap<String,LanguageMetadata>, languages_name: &[String]) -> Vec<f64> {
    let mut language_size = [0].repeat(languages_metadata_map.len());
    languages_metadata_map.iter().for_each(|e| {
        let pos = languages_name.iter().position(|name| name == e.0).unwrap();
        language_size[pos] = e.1.bytes;
    });
    
    get_percentages(&language_size)
}

fn get_percentages(numbers: &[usize]) -> Vec<f64> {
    let total_files :usize = numbers.iter().sum();
    let mut language_percentages = Vec::with_capacity(4);
    let mut sum = 0.0;
    for (counter,files) in numbers.iter().enumerate() {
        if counter == numbers.len() - 1 {
            let remainder = if sum > 99.99 {0.0} else {((100f64 - sum) * 100f64).round() / 100f64};
            // The last entry is the one that absorbs the rounding, and it is usually 'others', so
            // it needs the same marker as the rest when it is present but too small to print
            language_percentages.push(if remainder == 0.0 && *files > 0 {PRESENT_BUT_TINY} else {remainder});
        } else {
            let percentage = *files as f64/total_files as f64;
            let canonicalized = (percentage * 10000f64).round() / 100f64;
            // A language that exists but rounds away to zero keeps a value just above zero, so
            // that the printed text can say "<0.01" instead of claiming it is absent. The running
            // sum still takes the rounded value, so the arithmetic of the last entry is untouched,
            // and the marker stays below the threshold that earns a cell in the bar.
            let canonicalized = if canonicalized == 0.0 && *files > 0 {PRESENT_BUT_TINY} else {canonicalized};
            sum += (canonicalized * 100f64).round() / 100f64;
            language_percentages.push(canonicalized);
        }
    }
    language_percentages
}



#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::{config_manager::LogOption, io_handler::log_stats};

    use super::*;

    // One dataset for every layout, chosen so that the things that break are all present at once: a
    // long language name next to a short one, figures wide enough to move the shared right edge, a
    // keyword row long enough to wrap, a language with no keywords at all, and five languages, which
    // is one more than the overview can show without folding into "others".
    fn sample_data() -> (Vec<String>, HashMap<String, LanguageContentInfo>, HashMap<String, LanguageMetadata>, FinalStats) {
        let content_info_map = hashmap![
            "Rust".to_owned() => LanguageContentInfo::new(9008, 6122, 505,
                    hashmap!["enums".to_owned() => 11, "structs".to_owned() => 29, "traits".to_owned() => 1]),
            "JavaScript".to_owned() => LanguageContentInfo::new(1200, 900, 120,
                    hashmap!["classes".to_owned() => 805, "functions".to_owned() => 1204, "generators".to_owned() => 17,
                             "promises".to_owned() => 96, "imports".to_owned() => 342]),
            "HTML".to_owned() => LanguageContentInfo::new(396, 361, 0, hashmap![]),
            "Python".to_owned() => LanguageContentInfo::new(250, 200, 20, hashmap!["classes".to_owned() => 2]),
            "Java".to_owned() => LanguageContentInfo::new(80, 60, 5,
                    hashmap!["classes".to_owned() => 2, "interfaces".to_owned() => 1])];
        let languages_metadata_map = hashmap![
            "Rust".to_owned() => LanguageMetadata::new(13, 416800),
            "JavaScript".to_owned() => LanguageMetadata::new(4, 40000),
            "HTML".to_owned() => LanguageMetadata::new(2, 18800),
            "Python".to_owned() => LanguageMetadata::new(3, 9000),
            "Java".to_owned() => LanguageMetadata::new(1, 900)];
        let final_stats = FinalStats::new_extended(23, 10934, 7643, 650, 2641, 485500, 21108);
        let sorted = get_sorted_language_names(&content_info_map, &languages_metadata_map, SortCriterion::Lines);

        (sorted, content_info_map, languages_metadata_map, final_stats)
    }

    // The same five languages, split into the shape a run with modules produces: two named ones and
    // the leftovers. The totals are unchanged by construction, so any difference between the grouped
    // cases and the ungrouped ones below is the grouping and nothing else.
    fn sample_modules() -> Vec<ModuleResult> {
        let (_, content_info, metadata, _) = sample_data();
        let of = |name: Option<&str>, languages: &[&str]| {
            let content_info_map = languages.iter().map(|x| ((*x).to_owned(), content_info[*x].clone())).collect::<HashMap<_,_>>();
            let languages_metadata_map = languages.iter().map(|x| ((*x).to_owned(), metadata[*x].clone())).collect::<HashMap<_,_>>();
            let final_stats = FinalStats::calculate(&content_info_map, &languages_metadata_map);
            ModuleResult {name: name.map(str::to_owned), content_info_map, languages_metadata_map, final_stats}
        };

        vec![of(Some("frontend"), &["JavaScript", "HTML"]), of(Some("backend"), &["Rust"]),
             of(None, &["Python", "Java"])]
    }

    fn groups_from<'a>(modules: &'a [ModuleResult], config: &Configuration) -> Vec<Group<'a>> {
        let result = RunResult {content_info_map: HashMap::new(), languages_metadata_map: HashMap::new(),
                modules: Vec::new(), final_stats: FinalStats::new_extended(0,0,0,0,0,0,0), faulty_files: Vec::new(),
                files_present: FilesPresent::default(), scan_duration_millis: 0, metrics: None};
        let mut result = result;
        result.modules = modules.iter().map(|x| ModuleResult {
            name: x.name.clone(),
            content_info_map: x.content_info_map.clone(),
            languages_metadata_map: x.languages_metadata_map.clone(),
            final_stats: FinalStats::calculate(&x.content_info_map, &x.languages_metadata_map)
        }).collect();
        // The borrow has to outlive the temporary, so the groups are built against the caller's slice
        let order = groups_of(&result, config).into_iter().map(|x| (x.name.map(str::to_owned), x.languages, x.hidden))
                .collect::<Vec<_>>();

        order.into_iter().map(|(name, languages, hidden)| {
            let module = modules.iter().find(|x| x.name == name).unwrap();
            Group {name: module.name.as_deref(), languages, hidden, content_info_map: &module.content_info_map,
                    languages_metadata_map: &module.languages_metadata_map, final_stats: &module.final_stats}
        }).collect()
    }

    fn render_every_layout() -> String {
        // Not left to the absence of a terminal: CLICOLOR_FORCE overrides that, and the verification
        // protocol in CLAUDE.md tells the reader to export it, so the same shell that ran a manual
        // comparison would otherwise fail this test with a wall of escape codes
        colored::control::set_override(false);

        let (sorted, content_info, metadata, final_stats) = sample_data();
        let theme = &Theme::default();
        let mut config = Configuration::new(vec!["./".to_owned()]);
        let plain = vec![Group {name: None, languages: sorted.clone(), hidden: 0, content_info_map: &content_info,
                languages_metadata_map: &metadata, final_stats: &final_stats}];
        let columns = Columns::of(&plain, &final_stats);
        let width = columns.width(theme);

        let mut cases: Vec<(String, Vec<String>)> = Vec::new();
        let mut list = individual_lines(theme, &plain, &columns, width, true);
        list.extend(sum_lines(theme, &content_info, &final_stats, &columns, width, true));
        cases.push(("list".to_owned(), list));
        cases.push(("list, keywords hidden".to_owned(),
                individual_lines(theme, &plain, &columns, width, false)));

        let mut table = table_lines(theme, &plain, &final_stats, true);
        table.extend(keyword_block_lines(theme, &plain));
        cases.push(("table".to_owned(), table));

        let mut boxed = boxed_lines(theme, &plain, &final_stats, true);
        boxed.extend(keyword_block_lines(theme, &plain));
        cases.push(("boxed".to_owned(), boxed));

        cases.push(("overview".to_owned(), overview_lines(&sorted, &content_info, &metadata, &final_stats, &config)));

        config.top_n = Some(2);
        cases.push(("overview, top 2".to_owned(), overview_lines(&sorted, &content_info, &metadata, &final_stats, &config)));

        config.top_n = None;
        config.hidden.bar = true;
        cases.push(("overview, bar hidden".to_owned(), overview_lines(&sorted, &content_info, &metadata, &final_stats, &config)));

        // The same data with a second axis through it. Every layout groups, and the total under them
        // is the same total, which is what makes the two halves of this file comparable by eye.
        let mut config = Configuration::new(vec!["./".to_owned()]);
        let modules = sample_modules();
        let groups = groups_from(&modules, &config);
        let columns = Columns::of(&groups, &final_stats);
        let width = columns.width(theme);

        let mut list = individual_lines(theme, &groups, &columns, width, true);
        list.extend(sum_lines(theme, &content_info, &final_stats, &columns, width, true));
        cases.push(("modules, list".to_owned(), list));

        let mut table = table_lines(theme, &groups, &final_stats, true);
        table.extend(keyword_block_lines(theme, &groups));
        cases.push(("modules, table".to_owned(), table));

        let mut boxed = boxed_lines(theme, &groups, &final_stats, true);
        boxed.extend(keyword_block_lines(theme, &groups));
        cases.push(("modules, boxed".to_owned(), boxed));

        // '--top' is per module, so it cuts inside each one and not across the report
        config.top_n = Some(1);
        let groups = groups_from(&modules, &config);
        cases.push(("modules, table, top 1".to_owned(), table_lines(theme, &groups, &final_stats, true)));

        config.top_n = None;
        config.sort_by = SortCriterion::Name;
        let groups = groups_from(&modules, &config);
        cases.push(("modules, table, sorted by name".to_owned(), table_lines(theme, &groups, &final_stats, true)));

        // The rows of the matrix are the languages of the whole run, so what fills the cells is the
        // criterion and the criterion is what the second case changes
        config.sort_by = SortCriterion::Lines;
        let groups = groups_from(&modules, &config);
        cases.push(("modules, matrix".to_owned(),
                matrix_lines(theme, &groups, &sorted, &final_stats, SortCriterion::Lines, true)));
        cases.push(("modules, matrix, by size".to_owned(),
                matrix_lines(theme, &groups, &sorted, &final_stats, SortCriterion::Size, true)));
        cases.push(("modules, matrix, top 2".to_owned(),
                matrix_lines(theme, &groups, &sorted[..2], &final_stats, SortCriterion::Lines, true)));

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

    // The unnamed row is nine characters and 'Total', the width the column starts from, is five, so a report
    // whose module and language names are all shorter than that leaves the row wider than the column
    // it sits in. The padding is 'self.name - name_len', which then goes below zero: a panic in a
    // debug build, and in release only an accident of wrapping arithmetic away from a broken line.
    #[test]
    fn the_leftovers_row_fits_the_column_even_when_every_other_name_is_shorter() {
        colored::control::set_override(false);

        let content_info = hashmap!["D".to_owned() => LanguageContentInfo::new(2, 2, 0, hashmap![])];
        let metadata = hashmap!["D".to_owned() => LanguageMetadata::new(1, 24)];
        let final_stats = FinalStats::calculate(&content_info, &metadata);
        fn group<'a>(name: Option<&'a str>, content_info: &'a HashMap<String, LanguageContentInfo>,
                metadata: &'a HashMap<String, LanguageMetadata>, final_stats: &'a FinalStats) -> Group<'a> {
            Group {name, languages: vec!["D".to_owned()], hidden: 0, content_info_map: content_info,
                    languages_metadata_map: metadata, final_stats}
        }
        let groups = vec![group(Some("a"), &content_info, &metadata, &final_stats),
                group(None, &content_info, &metadata, &final_stats)];

        let theme = &Theme::default();
        let columns = Columns::of(&groups, &final_stats);
        assert!(columns.name >= UNNAMED_MODULE_NAME.len());

        let lines = individual_lines(theme, &groups, &columns, columns.width(theme), false);
        // and the arrow of every row still lands in the same column
        let arrow_at = |needle: &str| lines.iter().find(|x| x.starts_with(needle)).map(|x| x.find("->").unwrap());
        assert_eq!(arrow_at("a "), arrow_at(UNNAMED_MODULE_NAME));
        assert_eq!(arrow_at("a "), arrow_at("  D"));
    }

    // Everything above renders one block at a time, and the wiring that hands each block its data
    // is the part that broke: `--top` was cutting the list before the overview saw it, and the
    // overview needs the whole of it because it folds the remainder into 'others' itself. Every
    // value of `--top` from 1 to 4 took the run down. This goes through the real entry point, which
    // is the only thing that could have caught it: the golden calls the block functions directly.
    #[test]
    fn every_layout_survives_a_top_that_hides_languages() {
        colored::control::set_override(false);

        let (_, content_info, metadata, _) = sample_data();
        let of_modules = |modules: Vec<ModuleResult>| RunResult {
            content_info_map: content_info.clone(), languages_metadata_map: metadata.clone(), modules,
            final_stats: FinalStats::new_extended(23, 10934, 7643, 650, 2641, 485500, 21108),
            faulty_files: Vec::new(), files_present: FilesPresent::default(), scan_duration_millis: 0, metrics: None};
        let single = || vec![ModuleResult {name: None, content_info_map: content_info.clone(),
                languages_metadata_map: metadata.clone(),
                final_stats: FinalStats::calculate(&content_info, &metadata)}];

        for layout in [Layout::List, Layout::Table, Layout::Boxed, Layout::Matrix] {
            // One past the five languages of the sample, so the boundary where nothing is hidden is
            // walked as well as the ones where almost everything is
            for top in 1..=6 {
                let mut config = Configuration::new(vec!["./".to_owned()]);
                config.layout = layout;
                config.top_n = Some(top);

                format_and_print_results(&of_modules(single()), &None, &Local::now(), &config);
                format_and_print_results(&of_modules(sample_modules()), &None, &Local::now(), &config);
            }
        }
    }

    // The counting has its own golden in tests/stats_golden.rs; this one is the presentation, which
    // until now was verified by looking at it. Colours are absent by construction, since a test
    // binary is not a terminal, so what is locked here is the shape: alignment, widths, the wrapping
    // of the keyword rows, the folding into "others" and the apportionment of the bar.
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

        let content_info_map = hashmap!("cs".to_string() => LanguageContentInfo::dummy(100),
            "java".to_string() => LanguageContentInfo::dummy(100), "py".to_string() => LanguageContentInfo::dummy(0));
        assert_eq!(vec![0f64,50f64,50f64], get_lines_percentages(&content_info_map, &ext_names));
        let content_info_map = hashmap!("cs".to_string() => LanguageContentInfo::dummy(0),
        "java".to_string() => LanguageContentInfo::dummy(0), "py".to_string() => LanguageContentInfo::dummy(1));
        assert_eq!(vec![100f64,0f64,0f64], get_lines_percentages(&content_info_map, &ext_names));
        let content_info_map = hashmap!("cs".to_string() => LanguageContentInfo::dummy(20),
        "java".to_string() => LanguageContentInfo::dummy(20), "py".to_string() => LanguageContentInfo::dummy(20));
        assert_eq!(vec![33.33f64,33.33f64,33.34f64], get_lines_percentages(&content_info_map, &ext_names));
        
        let ext_names = ["py".to_string(),"java".to_string(),"cs".to_string(),"rs".to_string()];

        let content_info_map = hashmap!("cs".to_string() => LanguageContentInfo::dummy(100),
            "java".to_string() => LanguageContentInfo::dummy(100), "py".to_string() => LanguageContentInfo::dummy(0),
            "rs".to_string() => LanguageContentInfo::dummy(0));
        assert_eq!(vec![0f64,50f64,50f64,0f64], get_lines_percentages(&content_info_map, &ext_names));
        let content_info_map = hashmap!("cs".to_string() => LanguageContentInfo::dummy(100),
            "java".to_string() => LanguageContentInfo::dummy(100), "py".to_string() => LanguageContentInfo::dummy(100),
            "rs".to_string() => LanguageContentInfo::dummy(0));
        assert_eq!(vec![33.33,33.33,33.33,0.01], get_lines_percentages(&content_info_map, &ext_names));
        let content_info_map = hashmap!("cs".to_string() => LanguageContentInfo::dummy(201),
            "java".to_string() => LanguageContentInfo::dummy(200), "py".to_string() => LanguageContentInfo::dummy(200),
            "rs".to_string() => LanguageContentInfo::dummy(0));
        assert_eq!(vec![33.28,33.28,33.44,0.0], get_lines_percentages(&content_info_map, &ext_names));

        let ext_names = ["py".to_string(),"java".to_string(),"cs".to_string(),"rs".to_string(),"cpp".to_string()];

        let content_info_map = hashmap!("cs".to_string() => LanguageContentInfo::dummy(100),
            "java".to_string() => LanguageContentInfo::dummy(100), "py".to_string() => LanguageContentInfo::dummy(0),
            "rs".to_string() => LanguageContentInfo::dummy(0), "cpp".to_string() => LanguageContentInfo::dummy(0));
        assert_eq!(vec![0.0,50f64,50f64,0f64,0f64], get_lines_percentages(&content_info_map, &ext_names));
    }

    #[test]
    fn test_get_num_of_verticals() {
        let percentages = vec![49.6,50.4];
        let verticals = get_num_of_verticals(&percentages, NUM_OF_VERTICALS);
        assert!(verticals.iter().sum::<usize>() == 50);
        assert_eq!(vec![25,25], verticals);

        let percentages = vec![0.0,100.0];
        let verticals = get_num_of_verticals(&percentages, NUM_OF_VERTICALS);
        assert!(verticals.iter().sum::<usize>() == 50);
        assert_eq!(vec![0,50], verticals);


        let percentages = vec![33.33,33.33,33.34];
        assert_eq!(vec![16,17,17], get_num_of_verticals(&percentages, NUM_OF_VERTICALS));

        let percentages = vec![0.3,65.67,34.3];
        let verticals = get_num_of_verticals(&percentages, NUM_OF_VERTICALS);
        assert!(verticals.iter().sum::<usize>() == 50);
        assert_eq!(vec![1,32,17], verticals);
        
        let percentages = vec![0.0,0.0,100.0];
        let verticals = get_num_of_verticals(&percentages, NUM_OF_VERTICALS);
        assert!(verticals.iter().sum::<usize>() == 50);
        assert_eq!(vec![0,0,50], verticals);

        let percentages = vec![0.2,49.9,49.9];
        let verticals = get_num_of_verticals(&percentages, NUM_OF_VERTICALS);
        assert!(verticals.iter().sum::<usize>() == 50);
        assert_eq!(vec![1,24,25], verticals);


        let percentages = vec![12.5,50.0,25.0,12.5];
        let verticals = get_num_of_verticals(&percentages, NUM_OF_VERTICALS);
        assert!(verticals.iter().sum::<usize>() == 50);
        assert_eq!(vec![6,25,13,6], verticals);

        let percentages = vec![0.1,0.1,49.9,49.9];
        let verticals = get_num_of_verticals(&percentages, NUM_OF_VERTICALS);
        assert!(verticals.iter().sum::<usize>() == 50);
        assert_eq!(vec![1,1,24,24], verticals);

        // The minimum-one rule wants 48+1+1+1 here, which is one cell over the target. The excess
        // has to come off the largest share rather than emptying one of the small ones.
        let verticals = get_num_of_verticals(&[97.0,1.0,1.0,1.0], NUM_OF_VERTICALS);
        assert_eq!(vec![47,1,1,1], verticals);

        // Every protected minimum is paid for by the only entry that has cells to spare
        assert_eq!(vec![47,1,1,1], get_num_of_verticals(&[99.4,0.2,0.2,0.2], NUM_OF_VERTICALS));

        let verticals = get_num_of_verticals(&[99.7,0.1,0.1,0.1], NUM_OF_VERTICALS);
        assert_eq!(50, verticals.iter().sum::<usize>());
        assert!(verticals.iter().all(|x| *x >= 1), "a language that is present must never lose its last cell");
    }

    #[test]
    fn a_language_that_rounds_away_is_shown_as_less_than_a_hundredth_and_gets_no_cell() {
        // 3 files out of 800000 is 0.000375%, which used to print as a flat 0.00. Checked in the
        // middle and in the last position, which are computed by different branches
        for numbers in [vec![500_000, 3, 299_997], vec![500_000, 299_997, 3]] {
            let percentages = get_percentages(&numbers);
            let tiny = numbers.iter().position(|x| *x == 3).unwrap();
            assert_eq!("<0.01", percent_text(percentages[tiny]), "for {numbers:?}");
            let verticals = get_num_of_verticals(&percentages, NUM_OF_VERTICALS);
            assert_eq!(0, verticals[tiny], "a share too small to be printed must not claim a cell either");
            assert_eq!(NUM_OF_VERTICALS, verticals.iter().sum::<usize>());
        }

        // A language that really is absent stays at a flat zero and keeps no cell
        let percentages = get_percentages(&[500_000, 299_997, 0]);
        assert_eq!("0.00", percent_text(percentages[2]));
        assert_eq!(0, get_num_of_verticals(&percentages, NUM_OF_VERTICALS)[2]);

        // A genuine zero stays a zero, and anything printable is left alone
        assert_eq!("0.00", percent_text(0.0));
        assert_eq!("0.01", percent_text(0.01));
        assert_eq!("12.35", percent_text(12.345));
        assert_eq!("100.00", percent_text(100.0));

        // The overview pads every percentage into a 5 column field, so the marker has to fit in it.
        // 100.00 is the one value that does not, which is why that padding saturates.
        for value in [0.0, PRESENT_BUT_TINY, 0.01, 9.9, 99.99] {
            assert!(percent_text(value).len() <= 5, "'{}' does not fit the column", percent_text(value));
        }
        assert_eq!(6, percent_text(100.0).len());
    }

    #[test]
    fn sorting_uses_the_chosen_criterion_and_breaks_ties_by_name() {
        let content = hashmap![
            "Zig".to_owned() => LanguageContentInfo::new(100, 50, 0, HashMap::new()),
            "Ada".to_owned() => LanguageContentInfo::new(100, 90, 0, HashMap::new()),
            "Rust".to_owned() => LanguageContentInfo::new(300, 10, 0, HashMap::new())];
        let meta = hashmap![
            "Zig".to_owned() => LanguageMetadata::new(9, 10),
            "Ada".to_owned() => LanguageMetadata::new(1, 900),
            "Rust".to_owned() => LanguageMetadata::new(5, 50)];

        assert_eq!(vec!["Rust","Ada","Zig"], get_sorted_language_names(&content, &meta, SortCriterion::Lines));
        assert_eq!(vec!["Zig","Rust","Ada"], get_sorted_language_names(&content, &meta, SortCriterion::Files));
        assert_eq!(vec!["Ada","Rust","Zig"], get_sorted_language_names(&content, &meta, SortCriterion::Size));
        assert_eq!(vec!["Ada","Zig","Rust"], get_sorted_language_names(&content, &meta, SortCriterion::Code));
        assert_eq!(vec!["Ada","Rust","Zig"], get_sorted_language_names(&content, &meta, SortCriterion::Name));

        // Ada and Zig both have 100 lines, so the name decides, not the iteration order of the map
        assert_eq!(vec!["Rust","Ada","Zig"], get_sorted_language_names(&content, &meta, SortCriterion::Lines));
    }

    #[test]
    fn the_cell_that_a_protected_minimum_costs_comes_off_the_largest_share() {
        // Six entries at 100 cells: the second language deserves 3 and must keep them, because
        // losing one understates it by a third while the first barely notices
        let percentages = vec![96.96, 3.0, 0.01, 0.01, 0.01, 0.01];
        assert_eq!(vec![93,3,1,1,1,1], get_num_of_verticals(&percentages, 100));
        assert_eq!(vec![45,1,1,1,1,1], get_num_of_verticals(&percentages, NUM_OF_VERTICALS));

        // The width is a parameter, so the same shares scale to any bar
        assert_eq!(25, get_num_of_verticals(&[50.0,50.0], 50)[0]);
        assert_eq!(50, get_num_of_verticals(&[50.0,50.0], 100)[0]);
        assert_eq!(10, get_num_of_verticals(&[50.0,50.0], 20)[0]);
    }

    #[test]
    fn verticals_always_sum_to_the_bar_width_and_keep_present_languages_visible() {
        let cases: Vec<Vec<f64>> = vec![
            vec![100.0], vec![50.0,50.0], vec![0.01,99.99], vec![0.0,0.0,0.0,100.0],
            vec![25.0,25.0,25.0,25.0], vec![70.0,10.0,10.0,10.0], vec![1.0,1.0,1.0,97.0],
            vec![0.04,0.04,0.04,99.88], vec![33.34,33.33,33.33], vec![60.5,39.5],
            vec![98.0,2.0], vec![2.0,98.0], vec![0.0,100.0,0.0]
        ];

        for percentages in cases {
            let verticals = get_num_of_verticals(&percentages, NUM_OF_VERTICALS);
            assert_eq!(NUM_OF_VERTICALS, verticals.iter().sum::<usize>(), "wrong total for {percentages:?}");
            for (i, percentage) in percentages.iter().enumerate() {
                if *percentage > 0.0 {
                    assert!(verticals[i] >= 1, "{percentages:?} made a present language disappear");
                } else {
                    assert_eq!(0, verticals[i], "{percentages:?} gave cells to an absent language");
                }
            }
        }
    }

    #[test]
    fn test_retain_most_relevant_and_add_others_field_for_rest() {
        let sorted_language_names = vec!["a".to_owned(), "b".to_owned(), "c".to_owned(), "d".to_owned(), "e".to_owned()];
        let content_info_map = hashmap![
            "a".to_owned() => LanguageContentInfo::new(1000, 800, 0, hashmap![]),
            "b".to_owned() => LanguageContentInfo::new(900, 700, 0, hashmap![]),
            "c".to_owned() => LanguageContentInfo::new(800, 600, 0, hashmap![]),
            "d".to_owned() => LanguageContentInfo::new(700, 500, 0, hashmap![]),
            "e".to_owned() => LanguageContentInfo::new(600, 400, 0, hashmap![])
        ];
        let languages_metadata_map = hashmap![
            "a".to_owned() => LanguageMetadata::new(10, 60000),
            "b".to_owned() => LanguageMetadata::new(9, 50000),
            "c".to_owned() => LanguageMetadata::new(8, 40000),
            "d".to_owned() => LanguageMetadata::new(7, 30000),
            "e".to_owned() => LanguageMetadata::new(6, 20000)
        ];
        let final_stats = FinalStats::new(40, 4000, 3000, 0, 200000);

        let (folded_names, folded_content_info_map, folded_languages_metadata_map) = most_relevant_with_others_for_rest(
                &sorted_language_names, &content_info_map, &languages_metadata_map, &final_stats, None);

        // The caller's own data is untouched, so the same result can be printed again or handed to
        // a second consumer. Folding into "others" produces a separate view and nothing more.
        assert_eq!(5, sorted_language_names.len());
        assert_eq!(5, content_info_map.len());
        assert_eq!(5, languages_metadata_map.len());
        assert_eq!(vec!["a".to_owned(), "b".to_owned(), "c".to_owned(), "others".to_owned()], folded_names);

        let (content_info_map, languages_metadata_map) = (folded_content_info_map, folded_languages_metadata_map);
        assert_eq!(hashmap![
            "a".to_owned() => LanguageContentInfo::new(1000, 800, 0, hashmap![]),
            "b".to_owned() => LanguageContentInfo::new(900, 700, 0, hashmap![]),
            "c".to_owned() => LanguageContentInfo::new(800, 600, 0, hashmap![]),
            "others".to_owned() => LanguageContentInfo::new(1300, 0, 0, hashmap![])
            ], content_info_map);
        
        assert_eq!(hashmap![
            "a".to_owned() => LanguageMetadata::new(10, 60000),
            "b".to_owned() => LanguageMetadata::new(9, 50000),
            "c".to_owned() => LanguageMetadata::new(8, 40000),
            "others".to_owned() => LanguageMetadata::new(13, 50000)
            ], languages_metadata_map);
    }

    // The rule that keeps every old entry from being accused of a change nobody recorded: a setting
    // the entry never wrote is unknown, not different.
    #[test]
    fn a_setting_an_entry_never_recorded_is_never_reported_as_changed() {
        let entry_of = |settings: Vec<(&str, &str)>| LogEntry {
            name: None, stats: FinalStats::new_extended(1, 1, 1, 0, 0, 1, 1), modules: Vec::new(), datetime: Local::now(),
            settings: settings.into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect(),
            splits_comments: true};
        let mut config = Configuration::new(vec!["./src".to_owned()]);
        config.braces_as_code = true;

        // Everything the entry knows about matches, and it knows nothing of the rest
        let entry = entry_of(vec![("braces-as-code", "yes")]);
        assert!(settings_changed_since(&entry, &config).is_empty());
        assert!(modified_tag(&settings_changed_since(&entry, &config)).is_empty());

        let entry = entry_of(vec![("braces-as-code", "no"), ("no-gitignore", "no")]);
        assert_eq!(vec!["braces-as-code"], settings_changed_since(&entry, &config));

        config.no_gitignore = true;
        let changed = settings_changed_since(&entry, &config);
        assert_eq!(vec!["braces-as-code", "no-gitignore"], changed);
        assert!(modified_tag(&changed).contains("braces-as-code, no-gitignore"));

        // An entry with no settings block at all, which is every entry written before this existed
        assert!(settings_changed_since(&entry_of(vec![]), &config).is_empty());
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
        
        assert_eq!("0",difference_as_signed_percentage_str_of_f64(100.0, 100.0));
        assert_eq!("-10",difference_as_signed_percentage_str_of_f64(100.0, 90.0));
        assert_eq!("+100",difference_as_signed_percentage_str_of_f64(100.0, 200.0));
        assert_eq!("+0.01",difference_as_signed_percentage_str_of_f64(22819.0, 22820.0));
    }

    #[test]
    fn test_parse_N_previous_entries() {
        let contents = utils::extract_file_contents(&(LOCAL_APP_PATHS.test_dir.clone()+"logs/test1")).unwrap();
        let log_entries = parse_N_previous_entries(&contents, 3);

        assert_eq!(10, log_entries[0].stats.files);
        assert_eq!(1000, log_entries[0].stats.lines);
        assert_eq!(100, log_entries[0].stats.code_lines);
        assert_eq!(100, log_entries[0].stats.extra_lines);
        assert_eq!(100000, log_entries[0].stats.bytes_size);
        assert_eq!(100.0, log_entries[0].stats.size);
        assert_eq!("KBs".to_owned(), log_entries[0].stats.size_measurement);
        assert_eq!(10000, log_entries[0].stats.bytes_average_size);
        assert_eq!(10.0, log_entries[0].stats.average_size);
        assert_eq!("KBs".to_owned(), log_entries[0].stats.average_size_measurement);
        let datetime: DateTime<Local> = chrono::DateTime::from_str("2021-09-12 16:42:00 +0300").unwrap();
        assert_eq!(datetime, log_entries[0].datetime);
        assert_eq!(Some("entry one".to_owned()),log_entries[0].name);

        assert_eq!(11, log_entries[1].stats.files);
        assert_eq!(1111, log_entries[1].stats.lines);
        assert_eq!(111, log_entries[1].stats.code_lines);
        assert_eq!(111, log_entries[1].stats.extra_lines);
        assert_eq!(111100, log_entries[1].stats.bytes_size);
        assert_eq!(111.1, log_entries[1].stats.size);
        assert_eq!("KBs".to_owned(), log_entries[1].stats.size_measurement);
        assert_eq!(11100, log_entries[1].stats.bytes_average_size);
        assert_eq!(11.1, log_entries[1].stats.average_size);
        assert_eq!("KBs".to_owned(), log_entries[1].stats.average_size_measurement);
        let datetime: DateTime<Local> = chrono::DateTime::from_str("2021-09-12 16:23:50 +03:00").unwrap();
        assert_eq!(datetime, log_entries[1].datetime);
        assert_eq!(None,log_entries[1].name);

        assert_eq!(12, log_entries[2].stats.files);
        assert_eq!(1222, log_entries[2].stats.lines);
        assert_eq!(122, log_entries[2].stats.code_lines);
        assert_eq!(122, log_entries[2].stats.extra_lines);
        assert_eq!(122200, log_entries[2].stats.bytes_size);
        assert_eq!(122.2, log_entries[2].stats.size);
        assert_eq!("KBs".to_owned(), log_entries[2].stats.size_measurement);
        assert_eq!(12200, log_entries[2].stats.bytes_average_size);
        assert_eq!(12.2, log_entries[2].stats.average_size);
        assert_eq!("KBs".to_owned(), log_entries[2].stats.average_size_measurement);
        let datetime: DateTime<Local> = chrono::DateTime::from_str("2021-09-12 04:01:56 +03:00").unwrap();
        assert_eq!(datetime, log_entries[2].datetime);
        assert_eq!(Some("entry three".to_owned()),log_entries[2].name);
    }

    fn result_of(final_stats: FinalStats, modules: Vec<ModuleResult>) -> RunResult {
        RunResult {content_info_map: HashMap::new(), languages_metadata_map: HashMap::new(), modules,
                final_stats, faulty_files: Vec::new(), files_present: FilesPresent::default(),
                scan_duration_millis: 0, metrics: None}
    }

    #[test]
    fn test_log_creation_and_reading() -> std::io::Result<()> {
        let test_log_dir = LOCAL_APP_PATHS.test_log_dir.clone() + "test2";
        if Path::new(&test_log_dir).exists() {
            std::fs::remove_file(&test_log_dir).unwrap();
        }

        let mut config = Configuration::new(vec!["./".to_owned()]);
        config.set_log_option(LogOption::new(Some("test name".to_owned())));
        let result = result_of(FinalStats::new(10, 1000, 100, 0, 100), Vec::new());

        log_stats(&test_log_dir, &None, &result, &chrono::DateTime::from_str("2021-09-12 04:00:00 +03:00").unwrap(), &config).unwrap();

        let contents = utils::extract_file_contents(&test_log_dir).unwrap();
        let log_entries = parse_N_previous_entries(&contents, 1);

        assert_eq!(10, log_entries[0].stats.files);
        assert_eq!(1000, log_entries[0].stats.lines);
        assert_eq!(100, log_entries[0].stats.code_lines);
        assert_eq!(900, log_entries[0].stats.extra_lines);
        assert_eq!(100, log_entries[0].stats.bytes_size);
        assert_eq!(100.0, log_entries[0].stats.size);
        assert_eq!("Bytes".to_owned(), log_entries[0].stats.size_measurement);
        assert_eq!(10, log_entries[0].stats.bytes_average_size);
        assert_eq!(10.0, log_entries[0].stats.average_size);
        assert_eq!("Bytes".to_owned(), log_entries[0].stats.average_size_measurement);
        assert_eq!(Some("test name".to_owned()),log_entries[0].name);
        assert!(log_entries[0].modules.is_empty());

        Ok(())
    }

    // The block is written under the totals of the entry it belongs to, which is already complete by
    // then, so what this holds is that it reaches the right entry and that its own figures stay out
    // of the ones above and below it
    #[test]
    fn the_modules_of_an_entry_are_read_back_and_never_reach_another_one() {
        let test_log_dir = LOCAL_APP_PATHS.test_log_dir.clone() + "test_modules";
        if Path::new(&test_log_dir).exists() {
            std::fs::remove_file(&test_log_dir).unwrap();
        }

        // An entry from before any of this existed, with no 'Comments' line of its own
        let older = "===>\n2021-09-12 04:00:00 +0300\nStats:\n    Files: 4\n    Lines: 400\n        Code: 300\n        \
Extra: 100\n    Total Size: 4000\n        Average Size: 1000\n\n\n";
        let module_of = |name: Option<&str>, lines: usize, code: usize, comments: usize| ModuleResult {
            name: name.map(str::to_owned), content_info_map: HashMap::new(), languages_metadata_map: HashMap::new(),
            final_stats: FinalStats::new_extended(1, lines, code, comments, lines - code - comments, 10, 10)};
        let result = result_of(FinalStats::new_extended(10, 1000, 700, 200, 100, 5000, 500),
                vec![module_of(Some("frontend"), 600, 400, 150), module_of(None, 400, 300, 50)]);

        let mut config = Configuration::new(vec!["./".to_owned()]);
        config.set_log_option(LogOption::new(None));
        log_stats(&test_log_dir, &Some(older.to_owned()), &result,
                &chrono::DateTime::from_str("2021-09-13 04:00:00 +03:00").unwrap(), &config).unwrap();

        let contents = utils::extract_file_contents(&test_log_dir).unwrap();
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