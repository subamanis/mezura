use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Local};
use colored::{Color, ColoredString, Colorize};
use mezura_core::{CountingModel, RunResult, Stats, UNNAMED_MODULE_NAME, render};

use super::config_manager::{self, ByFile, Configuration, Layout, SortCriterion};
use super::number_formatter::format_with_separators;
use super::theme::Theme;

type ColorFunc = Box<dyn Fn(&str) -> String>;

const TOTAL_NAME : &str = "Total";

// What a comparison writes where a figure did not move
const NO_CHANGE : &str = "-";

// How far a language sits under the module it belongs to, in either table
const GROUP_INDENT : &str = "  ";
const SHELL_SUFFIX : &str = "itself";
// A tree cannot survive a frame drawn between every two rows, so the boxed layout marks instead
const BOXED_MARKER : char = '\u{203a}';
const BOXED_FILE_MARKER : char = '\u{25ab}';
// Under the language's second letter, so that two languages of different lengths line their children
// up in the same column
const BRANCH_INDENT : &str = " ";
const SHOWN_PATH_WIDTH : usize = 45;
const ELIDED : &str = "...";
const SEPARATOR_LINE : &str = "\u{2500}";
// Down for the figures, which come biggest first, and up for the name, which comes A to Z
const SORTED_DESCENDING : char = '\u{2304}';
const SORTED_ASCENDING : char = '\u{2303}';

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
    let global_names = get_sorted_language_names(per_language, config.view.sort_by, config.view.counting);
    let matrix_hidden = config.view.top_n.map_or(0, |top| global_names.len().saturating_sub(top));
    let matrix_names = global_names[..global_names.len() - matrix_hidden].to_vec();

    // The list is cut, but the total below it still counts everything, so the reader is told what
    // is missing rather than left to wonder why the rows do not add up
    let hidden_languages = if config.view.layout == Layout::Matrix {matrix_hidden}
            else {groups.iter().map(|x| x.hidden).sum::<usize>()};

    let theme = super::theme::get_active();
    let columns = Columns::of(&groups, total, config.view.hidden, config.view.counting);
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
    // The matrix crosses languages with modules and has no third direction for a file to hang in,
    // so the rows are not printed there and the count of what is missing would be about nothing
    let files_are_shown = layout != Layout::Matrix;
    if config.view.by_file.is_some() && !files_are_shown {
        eprintln!("\n{}", super::theme::get_active().warning.paint("'--by-file' prints nothing under the 'matrix' layout, whose rows are \
languages crossed with modules. Use any other layout to see the files."));
    }
    let hidden_files = if files_are_shown {count_hidden_files(&groups)} else {0};
    let is_table = layout != Layout::List;
    // With modules there is a sum of the module rows to be shown even when one language made all of
    // them, and without them a single language would only be repeated by a total under it
    let print_total = per_language.len() > 1 || groups.len() > 1;

    // What the rows do not add up to the total for. The two tables take them as rows of their own,
    // where they sit above the total that does not match them; the other two have no row to be, and
    // the list prints its total after this point anyway.
    let notes = create_hidden_notes(hidden_languages, hidden_files, config);
    let of_the_table = if is_table && layout != Layout::Matrix {notes.as_slice()} else {&[]};

    let view = ViewSettings::of(config);
    match layout {
        Layout::Matrix => print_as_matrix(theme, &groups, &matrix_names, total, print_total,
                should_print_keywords, config.view.counting),
        Layout::Boxed => print_as_boxed_table(theme, &groups, total, print_total, should_print_keywords,
                of_the_table, view),
        Layout::Table => print_as_table(theme, &groups, total, print_total, should_print_keywords,
                of_the_table, view),
        Layout::List => print_individually(theme, &groups, &columns, block_width, should_print_keywords)
    }

    if of_the_table.is_empty() {
        for note in &notes {
            println!("\n{}", theme.note.paint(note));
        }
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
        print_comparison_to_previous_runs(result, &groups, content, config, datetime_now);
    }
}

// The theme listing runs before a configuration exists, so it cannot go through
// 'super::theme::get_active()'. It asks for the real rows of one made-up language instead, built by the
// same functions a run uses, so that the preview cannot drift from what will actually be printed,
// and it follows the layout and the counting model in effect for the same reason: the third column
// is labelled by the model, so a preview under the other one names a column the run will not print.
// The figures are constants, so that every theme is judged against the same row.
pub fn create_theme_sample_rows(theme: &Theme, layout: Layout, model: CountingModel) -> Vec<String> {
    const NAME    : &str   = "Rust";
    const FILES   : usize  = 1_284;
    const BYTES   : usize  = 3_412_500;

    // A whole line for every class, so that the sample obeys both counting models the way a counted
    // file does. Given as classes and not as three columns because the columns are folds of these,
    // and a hand written pair that disagreed would print a third column no class of it accounts for.
    let classes = mezura_core::LineClasses {
        words_in_code: 68_004, string_content: 2_800, comment_words_beside_code: 200,
        words_in_comment: 12_638, punctuation_in_code: 9_100, punctuation_in_comment: 190,
        blank: 3_310, blank_in_comment: 130, blank_in_string: 140
    };
    let (lines, code, comments) = (classes.calculate_lines(),
            model.calculate_code_lines(&classes), model.calculate_comment_lines(&classes));

    let keywords = hashmap!("structs".to_owned() => 284usize, "traits".to_owned() => 31);
    let per_language = hashmap!(NAME.to_owned() => Stats::new(FILES, BYTES, lines, classes, keywords.clone()));
    let total = Stats::total_of(&per_language);
    let groups = vec![Group {name: None, languages: vec![NAME.to_owned()], hidden: 0,
            per_language: &per_language, nested: &NO_NESTED, files: HashMap::new(),
            total: &total, baseline: None}];

    // The two tables keep their keywords in a block of their own, so the sample has to ask for it or
    // the keyword tokens would go unshown in the two layouts that are now the common ones. One
    // language leaves nothing for a total to add up: it would only repeat the row above it.
    let with_keywords = |mut lines: Vec<String>| {
        lines.push(String::new());
        lines.extend(format_keyword_block_lines(theme, &groups));
        lines
    };
    let no_hides = config_manager::Hidden::default();
    let sample_view = ViewSettings { sort_by: SortCriterion::Lines, hidden: no_hides, model };
    match layout {
        Layout::Table => with_keywords(format_table_lines(theme, &groups, &total, false, &[], sample_view)),
        Layout::Boxed => with_keywords(format_boxed_lines(theme, &groups, &total, false, &[], sample_view)),
        // The matrix has no second axis to show for one made-up language of one unnamed module, and
        // the tokens it paints are the ones the table already previews
        Layout::Matrix => with_keywords(format_table_lines(theme, &groups, &total, false, &[], sample_view)),
        // Through the same 'Columns' a run builds, off the same counts the tables were handed, or
        // the one function whose whole purpose is that the preview cannot drift would be two
        // previews of two different languages
        Layout::List => {
            let columns = Columns::of(&groups, &total, no_hides, model);
            let width = columns.width(theme);
            vec![columns.format_files_row(theme, FILES, &format_size(theme, BYTES, BYTES / FILES), width),
                 columns.format_breakdown_row(theme, &theme.details_language_name.paint(NAME).to_string(),
                        NAME.len(), lines, code, comments),
                 get_keywords_as_str(theme, &keywords, None, columns.calculate_words_start(), width)]
        }
    }
}

// Ties are broken by name rather than left to the iteration order of the maps, which would make
// the printed order differ between runs on the very projects where languages are evenly matched
pub(crate) fn get_sorted_language_names(per_language: &HashMap<String, Stats>, criterion: SortCriterion,
    model: CountingModel) -> Vec<String>
{
    let value_of = |name: &String| per_language.get(name).map_or(0, |x| criterion.get_value_of(x, model));

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
    nested: &'a HashMap<String, HashMap<String, Stats>>,
    // Empty unless '--by-file' asked for them
    files: FileRowsOfModule<'a>,
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

// The three settings every row of one report has to answer to alike: the order the rows are in,
// the columns that are drawn, and the fold the counted ones go through. One value rather than three
// arguments, so that a block cannot be given one of them and miss another, which is the same reason
// 'Columns' carries the last two for the list layout.
#[derive(Clone,Copy)]
struct ViewSettings {
    sort_by: SortCriterion,
    hidden: config_manager::Hidden,
    model: CountingModel
}

impl ViewSettings {
    fn of(config: &Configuration) -> Self {
        ViewSettings { sort_by: config.view.sort_by, hidden: config.view.hidden,
                model: config.view.counting }
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
    let mut groups = result.modules.iter().map(|module| {
        let languages = get_sorted_language_names(&module.per_language, config.view.sort_by, config.view.counting);
        let hidden = config.view.top_n.map_or(0, |top| languages.len().saturating_sub(top));
        Group {
            name: module.name.as_deref(),
            languages: languages[..languages.len() - hidden].to_vec(),
            hidden,
            per_language: &module.per_language,
            // Emptied here and not at each layout, or the next layout forgets to obey the flag
            nested: if config.view.hidden.nested_languages {&NO_NESTED} else {&module.nested_languages},
            files: HashMap::new(),
            total: &module.total,
            baseline: None
        }
    }).collect::<Vec<_>>();

    for (group, files) in groups.iter_mut().zip(find_files_to_show(result, config)) {
        group.files = files;
    }

    groups
}

fn count_hidden_files(groups: &[Group]) -> usize {
    groups.iter().flat_map(|group| group.files.values()).map(|rows| rows.hidden).sum()
}

// The files a '--top' hid are behind the first sentence and are not counted again in the second
fn create_hidden_notes(languages: usize, files: usize, config: &Configuration) -> Vec<String> {
    let mut notes = Vec::new();
    if let Some(top) = config.view.top_n.filter(|_| languages > 0) {
        let plural = if languages == 1 {"language"} else {"languages"};
        notes.push(format!("(+{languages} more {plural} hidden by --{} {top})", config_manager::TOP));
    }
    if let Some(ByFile::Capped(rows)) = config.view.by_file.filter(|_| files > 0) {
        let plural = if files == 1 {"file"} else {"files"};
        notes.push(format!("(+{} more {plural} hidden by --{} {rows})",
                format_with_separators(files), config_manager::BY_FILE));
    }

    notes
}

// 'hidden' keeps the tree honest: a list that continues must not be drawn as one that ended
pub(crate) struct FileRows<'a> {
    pub shown: Vec<(Cow<'a, str>, &'a mezura_core::FileEntry)>,
    pub hidden: usize
}

pub(crate) type FileRowsOfModule<'a> = HashMap<&'a str, FileRows<'a>>;

// Cut inside each language of each module: over a whole report, the one part holding the biggest
// files would leave every other part with none
pub(crate) fn find_files_to_show<'a>(result: &'a RunResult, config: &Configuration) -> Vec<FileRowsOfModule<'a>> {
    let Some(by_file) = config.view.by_file else {
        return result.modules.iter().map(|_| HashMap::new()).collect();
    };

    let common_directory = find_common_directory_of(&result.targets);
    result.modules.iter().map(|module| {
        let names = get_sorted_language_names(&module.per_language, config.view.sort_by, config.view.counting);
        let hidden = config.view.top_n.map_or(0, |top| names.len().saturating_sub(top));

        names[..names.len() - hidden].iter().filter_map(|name| {
            let (language, entries) = module.files.get_key_value(name.as_str())?;
            let mut files = entries.iter().collect::<Vec<_>>();
            files.sort_by(|one, other| compare_files_by(one, other, config.view.sort_by, config.view.counting));
            let shown = by_file.shown_out_of(files.len());
            Some((language.as_str(), FileRows {
                shown: files[..shown].iter()
                        .map(|file| (shorten_path(&file.path, &result.targets, common_directory), *file)).collect(),
                hidden: files.len() - shown
            }))
        }).collect()
    }).collect()
}

// The path breaks every tie, so two files of equal size cannot swap places between two runs
pub(crate) fn compare_files_by(one: &mezura_core::FileEntry, other: &mezura_core::FileEntry,
        sort_by: SortCriterion, model: CountingModel) -> std::cmp::Ordering
{
    sort_by.get_value_of(&other.stats, model).cmp(&sort_by.get_value_of(&one.stats, model))
            .then_with(|| one.path.cmp(&other.path))
}

// A glob is one target per file it matched, so without this every row of such a run would be a bare
// name and two files called 'mod.rs' would print as the same row twice
fn find_common_directory_of(targets: &[mezura_core::Target]) -> &str {
    let Some(first) = targets.first() else { return "" };

    let mut common = first.path.as_str();
    for target in &targets[1..] {
        while !is_inside(&target.path, common) {
            match common.rsplit_once('/') {
                Some((shorter, _)) => common = shorter,
                None => return ""
            }
        }
    }

    common
}

// Compared by whole components, or 'D:/repository' counts as being inside 'D:/repo'
fn is_inside(path: &str, directory: &str) -> bool {
    path == directory || path.strip_prefix(directory).is_some_and(|rest| rest.starts_with('/'))
}

// What is too wide loses whole components out of its middle and never a piece of one: the file's own
// name is what tells one row from another, so a name too wide on its own is the floor
fn shorten_path<'a>(path: &'a str, targets: &[mezura_core::Target], common_directory: &str) -> Cow<'a, str> {
    let relative = targets.iter().filter_map(|target| path.strip_prefix(&target.path))
            .map(|rest| rest.trim_start_matches('/'))
            .filter(|rest| !rest.is_empty())
            .min_by_key(|rest| rest.len())
            .or_else(|| path.strip_prefix(common_directory).filter(|rest| rest.starts_with('/'))
                    .map(|rest| rest.trim_start_matches('/')))
            .unwrap_or_else(|| path.rsplit('/').next().unwrap_or(path));
    if calculate_widest_visible_line(relative) <= SHOWN_PATH_WIDTH {
        return Cow::Borrowed(relative);
    }

    let parts = relative.split('/').collect::<Vec<_>>();
    for kept in (1..parts.len().saturating_sub(1)).rev() {
        let shortened = format!("{}/{ELIDED}/{}", parts[0], parts[parts.len() - kept..].join("/"));
        if calculate_widest_visible_line(&shortened) <= SHOWN_PATH_WIDTH {
            return Cow::Owned(shortened);
        }
    }

    let name_alone = format!("{ELIDED}/{}", parts[parts.len() - 1]);
    if calculate_widest_visible_line(&name_alone) < calculate_widest_visible_line(relative) {
        return Cow::Owned(name_alone);
    }

    Cow::Borrowed(relative)
}

#[derive(PartialEq,Eq,Clone,Copy)]
enum RowKind {
    Module,
    Language,
    Total,
    Nested,
    File,
    Note
}

// Declared beside each table's own headers, so a sub-row is painted by role and not by position.
// 'Change' resolves to no paint at all: those cells arrive painted by their direction.
#[derive(PartialEq,Eq,Clone,Copy)]
enum ColumnKind {
    Name,
    Files,
    Lines,
    Code,
    Comments,
    Extra,
    Size,
    Percent,
    Change,
    // The percentage of a change, which only a comparison has. '--hide percentages' takes it and
    // leaves the absolute move, since that is still a change and not a share.
    ChangePercent
}

// Which of a table's columns '--hide' leaves standing. Everything that describes a figure follows
// that figure out, so hiding 'files' never leaves a bare share or a bare change behind.
fn create_shown_mask(columns: &[ColumnKind], hidden: config_manager::Hidden) -> Vec<bool> {
    let survives = |kind: ColumnKind| match kind {
        ColumnKind::Files => !hidden.files,
        ColumnKind::Comments => !hidden.comments,
        ColumnKind::Extra => !hidden.extra,
        ColumnKind::Size => !hidden.size,
        ColumnKind::Percent | ColumnKind::ChangePercent => !hidden.percentages,
        ColumnKind::Name | ColumnKind::Lines | ColumnKind::Code | ColumnKind::Change => true
    };
    let mut mask = Vec::with_capacity(columns.len());
    let mut figure_shown = true;
    for kind in columns {
        match kind {
            ColumnKind::Percent | ColumnKind::ChangePercent | ColumnKind::Change =>
                    mask.push(figure_shown && survives(*kind)),
            _ => {
                figure_shown = survives(*kind);
                mask.push(figure_shown);
            }
        }
    }

    mask
}

fn keep_shown<T>(cells: Vec<T>, mask: &[bool]) -> Vec<T> {
    cells.into_iter().zip(mask).filter(|(_, kept)| **kept).map(|(cell, _)| cell).collect()
}

static NO_NESTED : std::sync::LazyLock<HashMap<String, HashMap<String, Stats>>> =
        std::sync::LazyLock::new(HashMap::new);

// A nested language and a file hang off the same branch and each has its own set of tokens
struct SubRowStyles<'a> {
    name: &'a super::theme::Style,
    branch: &'a super::theme::Style,
    percent: &'a super::theme::Style,
    files: &'a super::theme::Style,
    lines: &'a super::theme::Style,
    code: &'a super::theme::Style,
    comments: &'a super::theme::Style,
    extra: &'a super::theme::Style,
    size: &'a super::theme::Style,
    size_unit: &'a super::theme::Style
}

impl<'a> SubRowStyles<'a> {
    fn of(theme: &'a Theme, kind: RowKind) -> Self {
        if kind == RowKind::File {
            SubRowStyles { name: &theme.file_name, branch: &theme.file_branch, percent: &theme.file_percent,
                    files: &theme.file_files, lines: &theme.file_lines, code: &theme.file_code,
                    comments: &theme.file_comments, extra: &theme.file_extra, size: &theme.file_size,
                    size_unit: &theme.file_size_unit }
        } else {
            SubRowStyles { name: &theme.nested_name, branch: &theme.nested_branch, percent: &theme.nested_percent,
                    files: &theme.nested_files, lines: &theme.nested_lines, code: &theme.nested_code,
                    comments: &theme.nested_comments, extra: &theme.nested_extra, size: &theme.nested_size,
                    size_unit: &theme.nested_size_unit }
        }
    }

    fn find_style_of(&self, column: ColumnKind) -> &'a super::theme::Style {
        match column {
            ColumnKind::Name => self.name,
            ColumnKind::Files => self.files,
            ColumnKind::Lines => self.lines,
            ColumnKind::Code => self.code,
            ColumnKind::Comments => self.comments,
            ColumnKind::Extra => self.extra,
            ColumnKind::Size => self.size,
            ColumnKind::Percent => self.percent,
            ColumnKind::Change | ColumnKind::ChangePercent => &UNPAINTED
        }
    }

    fn take_columns(&self, columns: &[ColumnKind]) -> Vec<&'a super::theme::Style> {
        columns.iter().map(|column| self.find_style_of(*column)).collect()
    }
}

// Saturating per class because these numbers arrive from a document as readily as from a run,
// where nothing promises the sections stay inside the whole. The lines are then what the classes
// left over add up to, rather than a subtraction of their own: a document whose sections do not
// fit would otherwise leave a shell holding more code than it has lines, and every layout works
// the third column out by taking code and comments off them.
fn take_out(shell: &mut Stats, sections: &Stats) {
    shell.bytes = shell.bytes.saturating_sub(sections.bytes);
    shell.classes.subtract(&sections.classes);
    shell.lines = shell.classes.calculate_lines();
}

// Biggest first, with the shell's own share inserted ahead of them
fn find_sections_of(group: &Group, language: &str, whole: &Stats) -> Vec<(String, Stats)> {
    let Some(sections) = group.nested.get(language) else { return Vec::new() };

    let mut counted = Stats::default();
    let mut rows = sections.iter().map(|(name, stats)| {
        counted.add(stats);
        (name.clone(), stats.clone())
    }).collect::<Vec<_>>();

    let mut shell = whole.clone();
    take_out(&mut shell, &counted);
    rows.sort_by(|one, other| other.1.lines.cmp(&one.1.lines).then(one.0.cmp(&other.0)));
    rows.insert(0, (format!("{language} {SHELL_SUFFIX}"), shell));

    rows
}

// 'stats' is carried rather than looked up by the name in the cell: with '--by-file 0' over a large
// tree, one search per row is a search through every file for every file
struct NamedRow<'a> {
    cell: String,
    kind: RowKind,
    group: &'a Group<'a>,
    // The language whose row this is, or whose row it sits under
    language: Option<&'a String>,
    stats: Option<Cow<'a, Stats>>
}

// The notes go last, above the total, since they say why the rows above do not add up to it
fn create_named_rows<'a>(groups: &'a [Group], print_total: bool, notes: &[String]) -> Vec<NamedRow<'a>> {
    let grouped = is_grouped(groups);
    let mut rows = Vec::with_capacity(groups.len() * 4);
    for group in groups {
        if grouped {
            rows.push(NamedRow { cell: group.get_displayed_name().to_owned(), kind: RowKind::Module,
                    group, language: None, stats: None });
        }
        for name in &group.languages {
            let cell = if grouped {GROUP_INDENT.to_owned() + name} else {name.clone()};
            rows.push(NamedRow { cell, kind: RowKind::Language, group, language: Some(name), stats: None });
            rows.extend(create_nested_rows_under(group, name, grouped));
        }
    }
    if groups.is_empty() {
        return rows;
    }
    for note in notes {
        rows.push(NamedRow { cell: note.clone(), kind: RowKind::Note, group: &groups[0],
                language: None, stats: None });
    }
    if print_total {
        rows.push(NamedRow { cell: TOTAL_NAME.to_owned(), kind: RowKind::Total, group: &groups[0],
                language: None, stats: None });
    }

    rows
}

fn create_nested_rows_under<'a>(group: &'a Group<'a>, name: &'a String, grouped: bool) -> Vec<NamedRow<'a>> {
    let indent = if grouped {GROUP_INDENT.to_owned() + BRANCH_INDENT} else {BRANCH_INDENT.to_owned()};
    let whole = group.per_language.get(name).unwrap();

    let sections = find_sections_of(group, name, whole);
    let files = group.files.get(name.as_str());
    let shown = files.map(|rows| rows.shown.as_slice()).unwrap_or_default();
    let mut rows = Vec::with_capacity(sections.len() + shown.len());
    let section_count = sections.len();
    for (at, (section, stats)) in sections.into_iter().enumerate() {
        let last = at + 1 == section_count;
        rows.push(NamedRow { cell: format!("{indent}{}{section}", find_branch_marker(last)),
                kind: RowKind::Nested, group, language: Some(name), stats: Some(Cow::Owned(stats)) });
    }
    // A language whose files were cut ends on a branch that hangs open, so that a tree drawn shut is
    // always the whole of what there is
    let complete = files.is_none_or(|rows| rows.hidden == 0);
    for (at, (shown_path, file)) in shown.iter().enumerate() {
        let last = at + 1 == shown.len() && complete;
        rows.push(NamedRow { cell: format!("{indent}{}{shown_path}", find_file_branch_marker(last)),
                kind: RowKind::File, group, language: Some(name), stats: Some(Cow::Borrowed(&file.stats)) });
    }

    rows
}

// The figures one row of a table draws, and the two the percentages beside them are shares of.
// The two tables lay them out differently and choose them alike, so the choosing is here and each
// of them is left with only its own cells.
struct RowFigures {
    files: usize,
    lines: usize,
    code: usize,
    comments: usize,
    bytes: usize,
    against_files: usize,
    against_lines: usize
}

// A module's share is of the whole run, a language's of the module it sits in, and a section's or a
// file's of the language it came out of: a module reading 100% of itself would say nothing, which
// is the whole point of the two levels. A note is a sentence about the rows and has no figures.
fn find_row_figures(row: &NamedRow, total: &Stats, model: CountingModel) -> Option<RowFigures> {
    let shown_against = |stats: &Stats, against: &Stats| RowFigures {
        files: stats.files,
        lines: stats.lines,
        code: stats.calculate_code_lines(model),
        comments: stats.calculate_comment_lines(model),
        bytes: stats.bytes,
        against_files: against.files,
        against_lines: against.lines
    };
    let group = row.group;
    let of_language = |name| group.per_language.get(name).unwrap();

    match row.kind {
        RowKind::Module => Some(shown_against(group.total, total)),
        RowKind::Total => Some(shown_against(total, total)),
        RowKind::Language => Some(shown_against(of_language(row.language.unwrap()), group.total)),
        RowKind::Nested | RowKind::File => Some(shown_against(row.stats.as_deref().unwrap(),
                of_language(row.language.unwrap()))),
        RowKind::Note => None
    }
}

fn find_branch_marker(last: bool) -> &'static str {
    if last {"\u{2514}\u{2500} "} else {"\u{251c}\u{2500} "}
}

// Longer than a nested language's, so the two lists under one language are told apart at a glance
fn find_file_branch_marker(last: bool) -> &'static str {
    if last {"\u{2514}\u{2500}\u{2500}\u{25ab} "} else {"\u{251c}\u{2500}\u{2500}\u{25ab} "}
}

fn find_section_name_in(cell: &str) -> &str {
    cell.trim_start_matches([' ', '\u{251c}', '\u{2514}', '\u{2500}', '\u{25ab}', BOXED_MARKER]).trim()
}

// '--sort' can come from a configuration file, and then nothing else on the page says it. The cell
// comes back painted and its style is replaced with one that adds nothing: a style laid over the
// whole of it would put the header's italics on the marker, which is a glyph and not a word.
fn mark_sorted_column<'a>(theme: &'a Theme, headers: &mut [String],
        styles: &mut [&'a super::theme::Style], sort_by: SortCriterion)
{
    let names_the_order = |header: &String| match sort_by {
        SortCriterion::Files => header == "Files",
        SortCriterion::Lines => header == "Lines",
        SortCriterion::Code => header == "Code",
        SortCriterion::Comments => header == "Comments",
        SortCriterion::Extra | SortCriterion::Blanks => header == "Extra" || header == "Blanks",
        SortCriterion::Size => header == "Size",
        SortCriterion::Name => header == "Language" || header == "Module"
    };
    let Some(at) = headers.iter().position(names_the_order) else { return };

    let marker = if sort_by == SortCriterion::Name {SORTED_ASCENDING} else {SORTED_DESCENDING};
    headers[at] = format!("{} {}", theme.sort_marker.paint(&marker.to_string()),
            styles[at].paint(&headers[at]));
    styles[at] = &UNPAINTED;
}

static UNPAINTED : std::sync::LazyLock<super::theme::Style> =
        std::sync::LazyLock::new(super::theme::Style::plain);

// The column holds modules and languages alike, and the indentation says which is which. Without
// the change of heading the reader of an uncolored paste is told that 'backend' is a language.
fn determine_name_header(groups: &[Group]) -> &'static str {
    if is_grouped(groups) {"Module"} else {"Language"}
}

// The third column is what is left after code and comments, and each model leaves something else:
// content leaves the lines that say nothing, region only the blanks outside everything. This is the
// heading over the cells, and the model itself holds the word the cells use.
fn get_third_column_header(model: CountingModel) -> &'static str {
    match model {
        CountingModel::Content => "Extra",
        CountingModel::Region => "Blanks"
    }
}

// One aligned row per language, no borders: whitespace alignment survives being pasted into a
// README or a ticket. The header cells reuse the label token of the quantity underneath them and
// the body cells its number token, so the table needs no styling of its own.
fn print_as_table(theme: &Theme, groups: &[Group], total: &Stats, print_total: bool,
        should_print_keywords: bool, notes: &[String], view: ViewSettings)
{
    println!("{}.\n", theme.heading.paint("Details"));
    for line in format_table_lines(theme, groups, total, print_total, notes, view) {
        println!("{line}");
    }

    if should_print_keywords {
        print_keyword_block(theme, groups);
    }

    // The 'list' layout closes with a blank line of its own, this one has to say so
    println!();
}

fn format_table_lines(theme: &Theme, groups: &[Group], total: &Stats, print_total: bool,
        notes: &[String], view: ViewSettings) -> Vec<String>
{
    let ViewSettings { sort_by, hidden, model } = view;
    // Every counted column carries its own percentage. The two that compare languages ('Files' and
    // 'Lines') take a share of the total, the two that describe one ('Code' and 'Comments') take a
    // share of that language's own lines.
    let headers : [&str; 11] = ["Language", "Files", "%", "Lines", "%", "Code", "%", "Comments", "%",
            get_third_column_header(model), "Size"];
    const COLUMNS : [ColumnKind; 11] = [ColumnKind::Name, ColumnKind::Files, ColumnKind::Percent,
            ColumnKind::Lines, ColumnKind::Percent, ColumnKind::Code, ColumnKind::Percent,
            ColumnKind::Comments, ColumnKind::Percent, ColumnKind::Extra, ColumnKind::Size];

    fn format_row_of(theme: &Theme, name: &str, figures: &RowFigures) -> Vec<String>
    {
        fn format_percent_cell(value: f64) -> String {
            format_percent_text(value) + "%"
        }

        fn format_share(part: usize, whole: usize) -> String {
            format_percent_cell(if whole == 0 {0.0} else {part as f64 / whole as f64 * 100.0})
        }

        let RowFigures { files, lines, code, comments, bytes, against_files, against_lines } = *figures;
        let (size, unit) = super::number_formatter::get_active().size_with_unit(bytes);
        let (code_percentage, comment_percentage) = calculate_code_and_comment_percentages(lines,code, comments);
        vec![name.to_owned(),
         format_with_separators(files), format_share(files, against_files),
         format_with_separators(lines), format_share(lines, against_lines),
         format_with_separators(code), format_percent_cell(code_percentage),
         format_with_separators(comments), format_percent_cell(comment_percentage),
         format_with_separators(lines - code - comments),
         size + " " + &theme.size_unit.paint(unit).to_string()]
    }

    let described = create_named_rows(groups, print_total, notes);
    let rows = described.iter().map(|row| {
        match find_row_figures(row, total, model) {
            Some(figures) => {
                let mut cells = format_row_of(theme, &row.cell, &figures);
                // A branch under a language wears the branch's own size unit, so the tree reads as
                // one drawing rather than as rows that happen to be indented
                if matches!(row.kind, RowKind::Nested | RowKind::File) {
                    let (size, unit) = super::number_formatter::get_active().size_with_unit(figures.bytes);
                    cells[10] = size + " " + &SubRowStyles::of(theme, row.kind).size_unit.paint(unit).to_string();
                }
                cells
            },
            None => {
                let mut cells = vec![String::new(); headers.len()];
                cells[0] = row.cell.clone();
                cells
            }
        }}).collect::<Vec<_>>();

    let mask = create_shown_mask(&COLUMNS, hidden);
    let columns = keep_shown(COLUMNS.to_vec(), &mask);
    // A percentage is kept against its number by a gap of its own
    let tight_after = (0..columns.len()).filter(|at| columns.get(at + 1) == Some(&ColumnKind::Percent))
            .collect::<Vec<_>>();
    let rows = rows.into_iter().map(|row| keep_shown(row, &mask)).collect::<Vec<_>>();

    let mut headers = keep_shown(headers.map(str::to_owned).to_vec(), &mask);
    headers[0] = determine_name_header(groups).to_owned();
    let mut header_styles = keep_shown(vec![&theme.details_language_header, &theme.files_label, &theme.percent,
            &theme.lines_label, &theme.percent, &theme.code_label, &theme.percent, &theme.comments_label,
            &theme.percent, &theme.extra_label, &theme.total_size_label], &mask);
    mark_sorted_column(theme, &mut headers, &mut header_styles, sort_by);
    let body_styles = keep_shown(vec![&theme.details_language_name, &theme.files_number, &theme.percent,
            &theme.lines_number, &theme.percent, &theme.code_number, &theme.percent, &theme.comments_number,
            &theme.percent, &theme.extra_number, &theme.total_size_number], &mask);

    draw_aligned_table(theme, &headers, &rows, &described.iter().map(|row| row.kind).collect::<Vec<_>>(),
            &columns, &tight_after, &header_styles, &body_styles, is_grouped(groups))
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
    let view = ViewSettings::of(config);
    let lines = match config.view.layout {
        Layout::Boxed => format_boxed_comparison_lines(theme, &rows, view),
        _ => format_comparison_lines(theme, &rows, view)
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
    let languages_of = |baseline_languages: &HashMap<String, Stats>, subject_languages: &HashMap<String, Stats>,
            baseline_nested: &HashMap<String, HashMap<String, Stats>>,
            subject_nested: &HashMap<String, HashMap<String, Stats>>, indent: &str| {
        super::diff::create_comparison_rows(baseline_languages, subject_languages, config.view.sort_by,
                config.view.top_n, config.view.counting)
                .0.into_iter()
                .flat_map(|change| {
                    let mut rows = vec![ComparedRow { name: indent.to_owned() + &change.name,
                            kind: RowKind::Language, baseline: change.baseline.clone(), subject: change.subject.clone() }];
                    if !config.view.hidden.nested_languages {
                        rows.extend(create_compared_sections(&change, baseline_nested, subject_nested, indent));
                    }
                    rows
                })
                .collect::<Vec<_>>()
    };

    let mut rows = Vec::new();
    match pairs {
        Some(pairs) => for pair in pairs {
            rows.push(ComparedRow { name: pair.name.unwrap_or(UNNAMED_MODULE_NAME).to_owned(),
                    kind: RowKind::Module, baseline: pair.before.total.clone(), subject: pair.now.total.clone() });
            rows.extend(languages_of(&pair.before.per_language, &pair.now.per_language,
                    &pair.before.nested_languages, &pair.now.nested_languages, GROUP_INDENT));
        },
        None => rows.extend(languages_of(&baseline.per_language, &subject.per_language,
                &baseline.nested_languages, &subject.nested_languages, ""))
    }
    rows.push(ComparedRow { name: TOTAL_NAME.to_owned(), kind: RowKind::Total,
            baseline: baseline.total.clone(), subject: subject.total.clone() });

    rows
}

// A section only one reading holds still gets a row, so one that was added or taken out is visible
fn create_compared_sections(change: &super::diff::LanguageStatsChange,
        baseline_nested: &HashMap<String, HashMap<String, Stats>>,
        subject_nested: &HashMap<String, HashMap<String, Stats>>, indent: &str) -> Vec<ComparedRow>
{
    let (before, now) = (baseline_nested.get(&change.name), subject_nested.get(&change.name));
    if before.is_none() && now.is_none() {
        return Vec::new();
    }

    let shell_of = |whole: &Stats, sections: Option<&HashMap<String, Stats>>| {
        let mut shell = whole.clone();
        let mut counted = Stats::default();
        for stats in sections.into_iter().flat_map(HashMap::values) {
            counted.add(stats);
        }
        take_out(&mut shell, &counted);
        shell
    };
    let mut names = before.into_iter().chain(now).flat_map(HashMap::keys).cloned()
            .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names.sort_by_key(|name| std::cmp::Reverse(now.and_then(|x| x.get(name)).map_or(0, |x| x.lines)));

    let of = |sections: Option<&HashMap<String, Stats>>, name: &str|
            sections.and_then(|x| x.get(name)).cloned().unwrap_or_default();
    let rows = std::iter::once((format!("{} {SHELL_SUFFIX}", change.name),
                    shell_of(&change.baseline, before), shell_of(&change.subject, now)))
            .chain(names.into_iter().map(|name| {
                let (was, is) = (of(before, &name), of(now, &name));
                (name, was, is)
            }))
            .collect::<Vec<_>>();

    rows.iter().enumerate().map(|(at, (name, baseline, subject))| ComparedRow {
            name: format!("{indent}{BRANCH_INDENT}{}{name}", find_branch_marker(at + 1 == rows.len())),
            kind: RowKind::Nested, baseline: baseline.clone(), subject: subject.clone() })
            .collect()
}

// One part of a comparison as the keyword block reads it: the languages of the rows the table
// printed, so one cut governs both and the block cannot name a language with no row above it, and
// what the earlier reading had of each. A language that is gone keeps its row in the table and has
// no keywords now, so it leaves the list here.
fn create_group_with_baseline<'a>(name: Option<&'a str>, baseline: &'a HashMap<String, Stats>,
        subject: &'a HashMap<String, Stats>, total: &'a Stats, config: &Configuration) -> Group<'a>
{
    let (rows, union) = super::diff::create_comparison_rows(baseline, subject, config.view.sort_by,
            config.view.top_n, config.view.counting);
    let hidden = union - rows.len();
    let languages = rows.into_iter().map(|row| row.name)
            .filter(|language| subject.contains_key(language)).collect();

    Group { name, languages, hidden, per_language: subject, nested: &NO_NESTED,
            files: HashMap::new(), total, baseline: Some(baseline) }
}

// What '--top' left out of the rows, counted where they were cut: inside each module when the
// modules are shown, over everything at once otherwise. Both readings' languages are in it, since a
// row exists for one that only the earlier had.
fn count_languages_hidden_by_top(pairs: Option<&[super::diff::ModulePair]>, baseline: &RunResult,
        subject: &RunResult, top: Option<usize>) -> usize
{
    let Some(top) = top else { return 0 };
    // A row per name either side has, which is what 'create_comparison_rows' builds and cuts, so
    // the two agree on how many it left out without it having to build any of them
    let cut = |before: &HashMap<String, Stats>, now: &HashMap<String, Stats>| {
        let union = now.keys().chain(before.keys()).collect::<HashSet<_>>().len();
        union - top.min(union)
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
fn format_boxed_comparison_lines(theme: &Theme, rows: &[ComparedRow], view: ViewSettings) -> Vec<String>
{
    let ViewSettings { sort_by, hidden, model } = view;
    const HEADERS : [&str; 6] = ["Language", "Files", "Lines", "Code", "Comments", "Size"];
    const COLUMNS : [ColumnKind; 6] = [ColumnKind::Name, ColumnKind::Files, ColumnKind::Lines,
            ColumnKind::Code, ColumnKind::Comments, ColumnKind::Size];

    let cells = |before: &Stats, now: &Stats| {
        // The absolute move and its percentage share one slot here, the borders doing the grouping
        // that the tight gaps do on the table
        let counted = |was: usize, is: usize| BoxedCell {
            number: format_with_separators(is),
            slot: if was == is {paint_change(theme, was, is, NO_CHANGE)}
                    else if hidden.percentages {paint_change(theme, was, is, &format_signed_difference(was, is))}
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
             counted(before.calculate_code_lines(model), now.calculate_code_lines(model)),
             counted(before.calculate_comment_lines(model), now.calculate_comment_lines(model)),
             BoxedCell { number: size + " " + &theme.size_unit.paint(unit).to_string(),
                         slot: paint_change(theme, before.bytes, now.bytes, &format_signed_size(theme, before.bytes, now.bytes)) }]
    };

    let drawn = rows.iter().map(|row| (row.name.clone(), cells(&row.baseline, &row.subject))).collect::<Vec<_>>();
    let kinds = rows.iter().map(|row| row.kind).collect::<Vec<_>>();

    let mask = create_shown_mask(&COLUMNS, hidden);
    let columns = keep_shown(COLUMNS.to_vec(), &mask);
    let drawn = drawn.into_iter()
            .map(|(name, cells)| (name, keep_shown(cells, &mask[1..]))).collect::<Vec<_>>();
    let number_styles = keep_shown(vec![&theme.files_number, &theme.lines_number, &theme.code_number,
            &theme.comments_number, &theme.total_size_number], &mask[1..]);

    let mut headers = keep_shown(HEADERS.map(str::to_owned).to_vec(), &mask);
    headers[0] = determine_name_header_for(&kinds).to_owned();
    let mut header_styles = keep_shown(vec![&theme.details_language_header, &theme.files_label,
            &theme.lines_label, &theme.code_label, &theme.comments_label, &theme.total_size_label], &mask);
    mark_sorted_column(theme, &mut headers, &mut header_styles, sort_by);

    draw_boxed_table(theme, &headers[0], &headers.iter().map(String::as_str).collect::<Vec<_>>(),
            &drawn, &kinds, &columns, ColumnKind::Change, &header_styles, &number_styles)
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
fn format_comparison_lines(theme: &Theme, rows: &[ComparedRow], view: ViewSettings) -> Vec<String>
{
    let ViewSettings { sort_by, hidden, model } = view;
    // The change columns are left unnamed: their values are two to five characters and the word would
    // widen the table for nothing, while every one of them carries a sign that says what it is.
    // The size carries no percentage of its own: it tracks the lines it is made of, so its share of
    // the change is the one beside them, and a second copy of it costs eight columns
    // The file count carries no percentage either: it is a handful of whole things, and "two more
    // files" is the answer where "+5.26%" is the same fact with a decimal point put on it
    const HEADERS : [&str; 14] = ["Language", "Files", "", "Lines", "", "%", "Code", "", "%",
            "Comments", "", "%", "Size", ""];
    const COLUMNS : [ColumnKind; 14] = [ColumnKind::Name, ColumnKind::Files, ColumnKind::Change,
            ColumnKind::Lines, ColumnKind::Change, ColumnKind::ChangePercent, ColumnKind::Code,
            ColumnKind::Change, ColumnKind::ChangePercent, ColumnKind::Comments, ColumnKind::Change,
            ColumnKind::ChangePercent, ColumnKind::Size, ColumnKind::Change];

    let plain = super::theme::Style::plain();
    // The direction is a property of the value and not of the column, so these two cells arrive
    // already painted and their column adds nothing
    let cells = |name: String, before: &Stats, now: &Stats| {
        let mut row = vec![name, format_with_separators(now.files),
                paint_change(theme, before.files, now.files, &format_signed_difference(before.files, now.files))];
        for (was, is) in [(before.lines, now.lines),
                (before.calculate_code_lines(model), now.calculate_code_lines(model)),
                (before.calculate_comment_lines(model), now.calculate_comment_lines(model))] {
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

    let mask = create_shown_mask(&COLUMNS, hidden);
    let columns = keep_shown(COLUMNS.to_vec(), &mask);
    // Both the change and its percentage belong to the number before them
    let tight_after = (0..columns.len()).filter(|at|
            matches!(columns.get(at + 1), Some(ColumnKind::Change | ColumnKind::ChangePercent)))
            .collect::<Vec<_>>();
    let drawn = drawn.into_iter().map(|row| keep_shown(row, &mask)).collect::<Vec<_>>();

    let mut headers = keep_shown(HEADERS.map(str::to_owned).to_vec(), &mask);
    let mut header_styles = keep_shown(vec![&theme.details_language_header, &theme.files_label, &plain,
            &theme.lines_label, &plain, &theme.percent, &theme.code_label, &plain, &theme.percent,
            &theme.comments_label, &plain, &theme.percent, &theme.total_size_label, &plain], &mask);
    let body_styles = keep_shown(vec![&theme.details_language_name, &theme.files_number, &plain,
            &theme.lines_number, &plain, &plain, &theme.code_number, &plain, &plain,
            &theme.comments_number, &plain, &plain, &theme.total_size_number, &plain], &mask);
    headers[0] = determine_name_header_for(&kinds).to_owned();
    mark_sorted_column(theme, &mut headers, &mut header_styles, sort_by);

    draw_aligned_table(theme, &headers, &drawn, &kinds, &columns, &tight_after, &header_styles,
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
        columns: &[ColumnKind], tight_after: &[usize], header_styles: &[&super::theme::Style],
        body_styles: &[&super::theme::Style], grouped: bool) -> Vec<String>
{
    const GAP : usize = 4;
    const TIGHT_GAP : usize = 2;

    let widths = (0..headers.len()).map(|i|
            rows.iter().zip(kinds.iter()).filter(|(_, kind)| **kind != RowKind::Note)
                    .map(|(row, _)| calculate_widest_visible_line(&row[i]))
                    .max().unwrap_or(0).max(calculate_widest_visible_line(&headers[i]))).collect::<Vec<_>>();

    // The language name and the percentages are not right aligned: a percentage sits a fixed two
    // spaces after the number it belongs to, and padding it on the left would push it away on exactly
    // the rows where its column is wider.
    let render = |cells: &[String], styles: &[&super::theme::Style]| {
        let mut line = String::with_capacity(140);
        if cells[1..].iter().all(String::is_empty) {
            return styles[0].paint(&cells[0]).to_string();
        }
        for (i, cell) in cells.iter().enumerate() {
            let padding = " ".repeat(widths[i].saturating_sub(calculate_widest_visible_line(cell)));
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

    let of_a_nested = SubRowStyles::of(theme, RowKind::Nested).take_columns(columns);
    let of_a_file = SubRowStyles::of(theme, RowKind::File).take_columns(columns);
    let rendered = std::iter::once(render(headers, header_styles))
            .chain(rows.iter().zip(kinds.iter()).map(|(row, kind)| match kind {
                RowKind::Nested => render(row, &of_a_nested),
                RowKind::File => render(row, &of_a_file),
                RowKind::Note => render(row, &vec![&theme.note; row.len()]),
                RowKind::Module | RowKind::Language | RowKind::Total => {
                    let mut styles = body_styles.to_vec();
                    styles[0] = match kind {
                        RowKind::Module => &theme.details_module,
                        RowKind::Total => &theme.details_total,
                        _ => &theme.details_language_name
                    };
                    render(row, &styles)
                }
            })).collect::<Vec<_>>();

    // Measured off the rows themselves rather than added up from the widths, which is the only way
    // it stays right: a row ends where its last cell ends, and a table whose last column is a tight
    // one has the padding of that column trimmed off the end of every row.
    let table_width = rendered.iter().map(|x| calculate_widest_visible_line(x)).max().unwrap_or(0);

    let mut lines = Vec::with_capacity(rendered.len() + kinds.len());
    let mut rendered = rendered.into_iter();
    lines.push(rendered.next().unwrap_or_default());
    lines.push(theme.separator_header.paint(&SEPARATOR_LINE.repeat(table_width)).to_string());
    // A blank line closes each module, so the sections are read apart at a glance instead of being
    // told apart only by the indentation of every second row. Once anything hangs under a language
    // the same blank closes each language, or one language's tree runs into the name of the next;
    // a module's first language is not held away from the name it belongs to.
    let has_sub_rows = kinds.iter().any(|kind| *kind == RowKind::Nested || *kind == RowKind::File);
    let mut previous = None;
    for (position, (line, kind)) in rendered.zip(kinds.iter()).enumerate() {
        let gap_above = match kind {
            RowKind::Nested | RowKind::File => false,
            RowKind::Language => has_sub_rows && previous != Some(RowKind::Module),
            // Two notes are one paragraph, so only the first opens a gap
            RowKind::Note => previous != Some(RowKind::Note),
            RowKind::Module | RowKind::Total => grouped && previous != Some(RowKind::Note)
        };
        if position > 0 && gap_above {
            lines.push(String::new());
        }
        if *kind == RowKind::Total {
            lines.push(theme.separator_total.paint(&SEPARATOR_LINE.repeat(table_width)).to_string());
        }
        lines.push(line);
        previous = Some(*kind);
    }

    lines
}

// Languages down, modules across. The nested table answers "what is inside the backend", read down
// a section; this one answers "how do the modules compare on the same language", read along a row.
fn print_as_matrix(theme: &Theme, groups: &[Group], languages: &[String], total: &Stats,
        print_total: bool, should_print_keywords: bool, model: CountingModel)
{
    println!("{}.\n", theme.heading.paint("Details"));
    for line in format_matrix_lines(theme, groups, languages, total, print_total, model) {
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
            nested: group.nested,
            files: HashMap::new(),
            total: group.total,
            baseline: group.baseline
        }).collect::<Vec<_>>();
        print_keyword_block(theme, &shown);
    }
    println!();
}

fn format_matrix_lines<'a>(theme: &'a Theme, groups: &[Group], languages: &[String], total: &Stats,
        print_total: bool, model: CountingModel) -> Vec<String>
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
            _ => content_info.calculate_code_lines(model)
        })
    };
    let of_stats = |stats: &Stats, metric: usize| match metric {
        0 => stats.files,
        1 => stats.lines,
        _ => stats.calculate_code_lines(model)
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
            .map(|(row,_)| calculate_widest_visible_line(&row[i])).max().unwrap_or(0)
            .max(calculate_widest_visible_line(&headers[i]))).collect::<Vec<_>>();

    // The name and its labels are left aligned like the labels they are, and every figure is right
    // aligned, so that a column can be compared down and a language across
    let render = |cells: &[String], styles: &[&super::theme::Style]| {
        let mut line = String::with_capacity(140);
        for (i, cell) in cells.iter().enumerate() {
            let padding = " ".repeat(widths[i] - calculate_widest_visible_line(cell));
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
        lines.push(theme.separator_total.paint(&SEPARATOR_LINE.repeat(table_width)).to_string());
        for (row, metric) in &totals {
            lines.push(render(row, &styles_for(&theme.details_total, *metric)));
        }
    }

    lines
}

// The same figures as the borderless table, in a drawn frame. Each number and its percentage share
// one cell here, since the borders already do the grouping that the tight gap does over there, and
// that brings the whole thing down from eleven columns to seven.
fn print_as_boxed_table(theme: &Theme, groups: &[Group], total: &Stats, print_total: bool,
        should_print_keywords: bool, notes: &[String], view: ViewSettings)
{
    println!("{}.
", theme.heading.paint("Details"));
    for line in format_boxed_lines(theme, groups, total, print_total, notes, view) {
        println!("{line}");
    }

    if should_print_keywords {
        print_keyword_block(theme, groups);
    }
    println!();
}

fn format_boxed_lines(theme: &Theme, groups: &[Group], total: &Stats, print_total: bool,
        notes: &[String], view: ViewSettings) -> Vec<String>
{
    let ViewSettings { sort_by, hidden, model } = view;
    let headers : [&str; 7] = ["Language", "Files", "Lines", "Code", "Comments",
            get_third_column_header(model), "Size"];
    const COLUMNS : [ColumnKind; 7] = [ColumnKind::Name, ColumnKind::Files, ColumnKind::Lines,
            ColumnKind::Code, ColumnKind::Comments, ColumnKind::Extra, ColumnKind::Size];

    fn format_row_of(theme: &Theme, name: &str, figures: &RowFigures) -> (String, Vec<BoxedCell>)
    {
        fn format_share(part: usize, whole: usize) -> String {
            format_percent_text(if whole == 0 {0.0} else {part as f64 / whole as f64 * 100.0}) + "%"
        }
        fn create_cell(number: String, slot: String) -> BoxedCell {
            BoxedCell { number, slot }
        }

        let RowFigures { files, lines, code, comments, bytes, against_files, against_lines } = *figures;
        let (size, unit) = super::number_formatter::get_active().size_with_unit(bytes);
        let (code_percentage, comment_percentage) = calculate_code_and_comment_percentages(lines,code, comments);
        (name.to_owned(), vec![
            create_cell(format_with_separators(files), format_share(files, against_files)),
            create_cell(format_with_separators(lines), format_share(lines, against_lines)),
            create_cell(format_with_separators(code), format_percent_text(code_percentage) + "%"),
            create_cell(format_with_separators(comments), format_percent_text(comment_percentage) + "%"),
            create_cell(format_with_separators(lines - code - comments), String::new()),
            create_cell(size + " " + &theme.size_unit.paint(unit).to_string(), String::new())])
    }

    let described = create_named_rows(groups, print_total, notes);
    let rows = described.iter().map(|row| match find_row_figures(row, total, model) {
        Some(figures) => format_row_of(theme, &row.cell, &figures),
        // A sentence about the rows and not a row of figures, so it takes the name cell and
        // leaves the columns empty
        None => (row.cell.clone(),
                (0..headers.len() - 1).map(|_| BoxedCell { number: String::new(), slot: String::new() }).collect())
    }).collect::<Vec<_>>();
    let kinds = described.iter().map(|row| row.kind).collect::<Vec<_>>();

    // The percentages live in the slots beside the numbers, and a column whose slots are all empty
    // is drawn without one
    let mask = create_shown_mask(&COLUMNS, hidden);
    let columns = keep_shown(COLUMNS.to_vec(), &mask);
    let mut rows = rows.into_iter()
            .map(|(name, cells)| (name, keep_shown(cells, &mask[1..]))).collect::<Vec<_>>();
    if hidden.percentages {
        for (_, cells) in &mut rows {
            for cell in cells {
                cell.slot.clear();
            }
        }
    }

    let header_styles = [&theme.files_label, &theme.lines_label, &theme.code_label,
            &theme.comments_label, &theme.extra_label, &theme.total_size_label];
    let number_styles = keep_shown(vec![&theme.files_number, &theme.lines_number, &theme.code_number,
            &theme.comments_number, &theme.extra_number, &theme.total_size_number], &mask[1..]);

    let mut headers = keep_shown(headers.map(str::to_owned).to_vec(), &mask);
    headers[0] = determine_name_header(groups).to_owned();
    let mut header_styles = keep_shown(std::iter::once(&theme.details_language_header)
            .chain(header_styles).collect::<Vec<_>>(), &mask);
    mark_sorted_column(theme, &mut headers, &mut header_styles, sort_by);

    draw_boxed_table(theme, &headers[0], &headers.iter().map(String::as_str).collect::<Vec<_>>(),
            &rows, &kinds, &columns, ColumnKind::Percent, &header_styles, &number_styles)
}

// The joints are as much text as the cells, so the width is the columns plus every joint
fn span_across(theme: &Theme, text: &str, inner_widths: &[usize], pad: usize, border: Color) -> String {
    let width = inner_widths.iter().sum::<usize>() + (inner_widths.len() - 1) * (2 * pad + 1);
    let bar = "\u{2502}".color(border).to_string();
    let padding = " ".repeat(pad);
    format!("{bar}{padding}{}{}{padding}{bar}", theme.note.paint(text),
            " ".repeat(width.saturating_sub(calculate_widest_visible_line(text))))
}

// One cell of the boxed frame: the count, and the slot beside it that a run fills with a share and a
// comparison with a change. A column whose slots are all empty is drawn without one.
struct BoxedCell { number: String, slot: String }

// The frame the boxed layout draws, for whatever columns it is handed: the details fill it with one
// reading, the comparison with two.
// 'header_styles' covers the name column too, at its front, since the one carrying the sort marker
// arrives painted and must not be painted a second time.
fn draw_boxed_table(theme: &Theme, name_title: &str, headers: &[&str], rows: &[(String, Vec<BoxedCell>)],
        kinds: &[RowKind], columns: &[ColumnKind], slot: ColumnKind,
        header_styles: &[&super::theme::Style], number_styles: &[&super::theme::Style]) -> Vec<String>
{
    const SLOT_GAP : usize = 2;
    // One space of air between a border and the text it holds
    const PAD : usize = 1;

    let slot_style : &super::theme::Style = if slot == ColumnKind::Change {&UNPAINTED} else {&theme.percent};

    // The name is drawn on its own, so everything indexed below counts from the column after it
    let with_figures = headers.len() - 1;
    // A note spans the whole frame, so it is not what any one column has to be wide enough for
    let name_width = rows.iter().zip(kinds.iter()).filter(|(_, kind)| **kind != RowKind::Note)
            .map(|((name, _), _)| calculate_widest_visible_line(name)).max().unwrap_or(0)
            .max(calculate_widest_visible_line(name_title));
    // Measured with the escape sequences skipped, since the size cell colors its own unit and a
    // comparison's change cells arrive painted by their direction
    let number_widths = (0..with_figures).map(|i| rows.iter().map(|(_,cells)| calculate_widest_visible_line(&cells[i].number)).max().unwrap_or(0))
            .collect::<Vec<_>>();
    let slot_widths = (0..with_figures).map(|i| rows.iter().map(|(_,cells)| calculate_widest_visible_line(&cells[i].slot)).max().unwrap_or(0))
            .collect::<Vec<_>>();

    // A column is as wide as its content needs, or as its header, whichever is more
    let inner_widths = std::iter::once(name_width).chain((0..with_figures).map(|i| {
            let content = number_widths[i] + if slot_widths[i] > 0 {SLOT_GAP + slot_widths[i]} else {0};
            content.max(calculate_widest_visible_line(headers[i + 1]))
        })).collect::<Vec<_>>();

    // Not theme tokens, since they would mean nothing in the other layouts. That needs FEAT-17.
    const BORDER_OUTER : Color = Color::TrueColor { r: 160, g: 160, b: 160 };
    const BORDER_INNER : Color = Color::TrueColor { r: 65, g: 65, b: 65 };

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

    let longest_section = rows.iter().zip(kinds.iter())
            .filter(|(_, kind)| **kind == RowKind::Nested || **kind == RowKind::File)
            .map(|((name, _), _)| calculate_widest_visible_line(find_section_name_in(name)))
            .max().unwrap_or(0);
    let mut lines = vec![frame("┌", "┬", "┐", "─", BORDER_OUTER, false)];

    // The titles sit over columns of mixed width, so they are centred rather than pinned to one
    // side of a cell that is often wider than the word in it
    let gap = inner_widths[0].saturating_sub(calculate_widest_visible_line(name_title));
    let centred = " ".repeat(gap / 2) + name_title;
    let mut header_cells = vec![header_styles[0].paint(&centred).to_string() + &" ".repeat(gap - gap / 2)];
    for (i, style) in header_styles[1..].iter().enumerate() {
        let text = headers[i + 1];
        // Measured with the escape sequences skipped: the header carrying the sort marker arrives
        // painted, and its bytes are several times what it draws
        let gap = inner_widths[i + 1].saturating_sub(calculate_widest_visible_line(text));
        header_cells.push(format!("{}{}{}", " ".repeat(gap / 2), style.paint(text), " ".repeat(gap - gap / 2)));
    }
    lines.push(content_row(header_cells, true));

    // The lines that bound the body, and the line that opens a module's section, belong to the
    // frame: solid and in its shade. Everything inside it is dashed and dim, the notes about what
    // was left out included, since those are read with the rows and not with the total.
    for (position, (name, cells)) in rows.iter().enumerate() {
        let kind = kinds[position];
        let is_body = match kind {
            RowKind::Module | RowKind::Total => false,
            RowKind::Language | RowKind::Nested | RowKind::File | RowKind::Note => true
        };
        // A module's name row is drawn in the frame's own shade, so it is closed on both sides
        // rather than only above: without the second rule the vertical of the table changes
        // brightness in the middle of itself and reads as a rendering fault.
        let after_module = position > 0 && kinds[position - 1] == RowKind::Module;
        let separator = if position == 0 || !is_body || after_module {
            frame("├", "┼", "┤", "─", BORDER_OUTER, false)
        } else {
            frame("├", "┼", "┤", "╌", BORDER_INNER, true)
        };
        lines.push(separator);

        // Cut into cells it would be one long name and five empty boxes, and the name column would
        // have to be as wide as a sentence
        if kind == RowKind::Note {
            lines.push(span_across(theme, name, &inner_widths, PAD, BORDER_OUTER));
            continue;
        }

        let of_a_sub_row = SubRowStyles::of(theme, kind);
        let name_style = match kind {
            RowKind::Module => &theme.details_module,
            RowKind::Total => &theme.details_total,
            RowKind::Language => &theme.details_language_name,
            RowKind::Note => &theme.note,
            RowKind::Nested | RowKind::File => of_a_sub_row.name
        };
        // The block as a whole is pushed right by its longest name. Aligning each name on its own
        // right edge puts a three letter one further in than a ten letter one, which reads as depth.
        // The sections stop halfway, so that the two lists under one language are not one block.
        let name = if kind == RowKind::Nested || kind == RowKind::File {
            let marker = if kind == RowKind::File {BOXED_FILE_MARKER} else {BOXED_MARKER};
            let text = format!("{marker} {}", find_section_name_in(name));
            let indent = inner_widths[0].saturating_sub(2 + longest_section);
            " ".repeat(if kind == RowKind::Nested {indent / 2} else {indent}) + &text
        } else {
            name.clone()
        };
        let padding = " ".repeat(inner_widths[0].saturating_sub(calculate_widest_visible_line(&name)));
        let mut painted = vec![format!("{}{padding}", name_style.paint(&name))];
        for (i, cell) in cells.iter().enumerate() {
            let (number_style, slot_style) = match kind {
                RowKind::Nested | RowKind::File =>
                        (of_a_sub_row.find_style_of(columns[i + 1]), of_a_sub_row.find_style_of(slot)),
                _ => (number_styles[i], slot_style)
            };
            let number = format!("{}{}", " ".repeat(number_widths[i] - calculate_widest_visible_line(&cell.number)),
                    number_style.paint(&cell.number));
            // A column with no slots holds bare numbers, and those sit on the right edge rather
            // than huddling on the left of a centred title
            if slot_widths[i] == 0 {
                painted.push(format!("{}{number}", " ".repeat(inner_widths[i + 1] - number_widths[i])));
                continue;
            }
            let body = format!("{number}{}{}{}", " ".repeat(SLOT_GAP), slot_style.paint(&cell.slot),
                    " ".repeat(slot_widths[i] - calculate_widest_visible_line(&cell.slot)));
            let used = number_widths[i] + SLOT_GAP + slot_widths[i];
            painted.push(format!("{body}{}", " ".repeat(inner_widths[i + 1] - used)));
        }
        lines.push(content_row(painted, !is_body));
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
            if columns.prints_files_row() {
                lines.push(columns.format_files_row(theme, stats.files,
                        &format_size(theme, stats.bytes, stats.calculate_average_size()), block_width));
            }
            lines.push(columns.format_breakdown_row(theme, &theme.details_module.paint(name).to_string(),
                    calculate_widest_visible_line(name), stats.lines,
                    stats.calculate_code_lines(columns.model), stats.calculate_comment_lines(columns.model)));
        }

        for (i, lang_name) in group.languages.iter().enumerate() {
            let content_info = group.per_language.get(lang_name).unwrap();
            if grouped || i > 0 {
                lines.push(String::new());
            }

            if columns.prints_files_row() {
                lines.push(columns.format_files_row(theme, content_info.files,
                        &format_size(theme, content_info.bytes, content_info.calculate_average_size()), block_width));
            }
            lines.push(columns.format_breakdown_row(theme, &(indent.to_owned() + &theme.details_language_name.paint(lang_name).to_string()),
                    calculate_widest_visible_line(lang_name) + indent.len(), content_info.lines,
                    content_info.calculate_code_lines(columns.model),
                    content_info.calculate_comment_lines(columns.model)));
            // No files row: the count and the average size above describe whole files
            let sections = find_sections_of(group, lang_name, content_info);
            let of_language = group.files.get(lang_name.as_str());
            let files = of_language.map(|rows| rows.shown.as_slice()).unwrap_or_default();
            let complete = of_language.is_none_or(|rows| rows.hidden == 0);
            let (sections_end, files_end) = (sections.len(), sections.len() + files.len());
            for (at, (branch_name, stats)) in sections.iter().map(|(section, stats)| (section.as_str(), stats))
                    .chain(files.iter().map(|(path, file)| (path.as_ref(), &file.stats))).enumerate() {
                let last = at + 1 == sections_end || (at + 1 == files_end && complete);
                let of_a_file = at >= sections_end;
                let branch = if of_a_file {find_file_branch_marker(last)} else {find_branch_marker(last)};
                let styles = SubRowStyles::of(theme, if of_a_file {RowKind::File} else {RowKind::Nested});
                let name = format!("{indent}{BRANCH_INDENT}{}{}", styles.branch.paint(branch),
                        styles.name.paint(branch_name));
                lines.push(columns.format_nested_row(&styles, &name,
                        indent.len() + BRANCH_INDENT.len() + calculate_widest_visible_line(branch)
                                + calculate_widest_visible_line(branch_name),
                        stats.lines, stats.calculate_code_lines(columns.model),
                        stats.calculate_comment_lines(columns.model)));
            }
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
    extra: usize,
    // Carried here so that every row a run renders through these functions obeys the same '--hide'
    // and the same fold
    hidden: config_manager::Hidden,
    model: CountingModel
}

impl Columns {
    fn of(groups: &[Group], total: &Stats, hidden: config_manager::Hidden, model: CountingModel) -> Self
    {
        let grouped = is_grouped(groups);
        let indent = if grouped {LIST_INDENT.len()} else {0};
        let len_of = |value: usize| format_with_separators(value).len();
        let mut columns = Columns {
            name: TOTAL_NAME.len(),
            headline: len_of(total.files).max(len_of(total.lines)),
            code: len_of(total.calculate_code_lines(model)),
            comments: len_of(total.calculate_comment_lines(model)),
            extra: len_of(total.calculate_extra_lines(model)),
            hidden,
            model
        };

        // The total holds the largest of every column, except when --top hid the language that made
        // it so, which is why the shown ones are measured too instead of assumed smaller
        for group in groups {
            // The leftovers print their name too, so it has to be measured too: a name wider than
            // the column it sits in makes the padding of its row a subtraction below zero.
            if grouped {
                columns.name = columns.name.max(calculate_widest_visible_line(group.get_displayed_name()));
            }
            for name in &group.languages {
                let content_info = group.per_language.get(name).unwrap();
                columns.name = columns.name.max(calculate_widest_visible_line(name) + indent);
                columns.headline = columns.headline.max(len_of(group.per_language.get(name).unwrap().files))
                        .max(len_of(content_info.lines));
                columns.code = columns.code.max(len_of(content_info.calculate_code_lines(model)));
                columns.comments = columns.comments.max(len_of(content_info.calculate_comment_lines(model)));
                columns.extra = columns.extra.max(len_of(content_info.calculate_extra_lines(model)));
                // The markers are asked for rather than assumed, so changing one cannot leave this
                // column a character short of what gets drawn in it
                let under = indent + BRANCH_INDENT.len();
                let branch = under + calculate_widest_visible_line(find_branch_marker(false));
                for (nested, _) in find_sections_of(group, name, content_info) {
                    columns.name = columns.name.max(calculate_widest_visible_line(&nested) + branch);
                }
                let file_branch = under + calculate_widest_visible_line(find_file_branch_marker(false));
                for (path, _) in group.files.get(name.as_str()).map(|rows| rows.shown.as_slice()).unwrap_or_default() {
                    columns.name = columns.name.max(calculate_widest_visible_line(path) + file_branch);
                }
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
        let percent = |value: f64| if self.hidden.percentages {String::new()}
                else {format!(" ({})", paint_percent(theme, value))};
        let mut terms = vec![format!("{:>code_w$} {}{}",
                theme.code_number.paint(&format_with_separators(code_lines)), theme.code_label.paint("code"),
                percent(code_percentage), code_w = self.code)];
        if !self.hidden.comments {
            terms.push(format!("{:>comments_w$} {}{}",
                    theme.comments_number.paint(&format_with_separators(comment_lines)),
                    theme.comments_label.paint("comments"), percent(comment_percentage), comments_w = self.comments));
        }
        if !self.hidden.extra {
            terms.push(format!("{:>extra_w$} {}",
                    theme.extra_number.paint(&format_with_separators(lines - code_lines - comment_lines)),
                    theme.extra_label.paint(self.model.get_third_quantity_name()), extra_w = self.extra));
        }
        format!("{}{}{}{}{:>headline_w$} {} {{ {} }}",
                painted_name, " ".repeat(self.name - name_len + NAME_GAP), theme.arrow.paint("->"), " ".repeat(NAME_GAP),
                theme.lines_number.paint(&format_with_separators(lines)), theme.lines_label.paint("lines"),
                terms.join("  +  "), headline_w = self.headline)
    }

    fn format_nested_row(&self, styles: &SubRowStyles, painted_name: &str, name_len: usize, lines: usize,
            code_lines: usize, comment_lines: usize) -> String
    {
        let (code_percentage, comment_percentage) = calculate_code_and_comment_percentages(lines, code_lines, comment_lines);
        let percent = |value: f64| if self.hidden.percentages {String::new()}
                else {format!(" ({})", styles.percent.paint(&(format_percent_text(value) + "%")))};
        let mut terms = vec![format!("{:>code_w$} {}{}",
                styles.code.paint(&format_with_separators(code_lines)), styles.code.paint("code"),
                percent(code_percentage), code_w = self.code)];
        if !self.hidden.comments {
            terms.push(format!("{:>comments_w$} {}{}",
                    styles.comments.paint(&format_with_separators(comment_lines)),
                    styles.comments.paint("comments"), percent(comment_percentage), comments_w = self.comments));
        }
        if !self.hidden.extra {
            terms.push(format!("{:>extra_w$} {}",
                    styles.extra.paint(&format_with_separators(lines - code_lines - comment_lines)),
                    styles.extra.paint(self.model.get_third_quantity_name()), extra_w = self.extra));
        }
        format!("{}{}{}{}{:>headline_w$} {} {{ {} }}",
                painted_name, " ".repeat(self.name - name_len + NAME_GAP), styles.branch.paint("->"), " ".repeat(NAME_GAP),
                styles.lines.paint(&format_with_separators(lines)), styles.lines.paint("lines"),
                terms.join("  +  "), headline_w = self.headline)
    }

    // The whole row is skipped when both halves are hidden, which is the caller's question to ask
    fn prints_files_row(&self) -> bool {
        !(self.hidden.files && self.hidden.size)
    }

    // The size text ends where the row below it does
    fn format_files_row(&self, theme: &Theme, files: usize, size_text: &str, width: usize) -> String {
        let left = if self.hidden.files {String::new()} else {
            format!("{}{:>headline_w$} {}", " ".repeat(self.calculate_headline_end() - self.headline),
                    theme.files_number.paint(&format_with_separators(files)), theme.files_label.paint("files"),
                    headline_w = self.headline)
        };
        if self.hidden.size {
            return left;
        }
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
    let mut lines = vec![format!("{} ",theme.separator_total.paint(&SEPARATOR_LINE.repeat(block_width)))];
    if columns.prints_files_row() {
        lines.push(columns.format_files_row(theme, total.files,
                &format_size(theme, total.bytes, total.calculate_average_size()), block_width));
    }
    lines.push(columns.format_breakdown_row(theme, &theme.details_total.paint(TOTAL_NAME).to_string(),
            TOTAL_NAME.len(), total.lines, total.calculate_code_lines(columns.model),
            total.calculate_comment_lines(columns.model)));

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
            .map(|(name,_)| calculate_widest_visible_line(name)).max().unwrap();
    let mut lines = Vec::with_capacity(rows.len() * 3);
    for (group, keyword_rows) in rows {
        if grouped {
            lines.push(theme.details_module.paint(group.get_displayed_name()).to_string());
        }
        lines.extend(keyword_rows.into_iter().map(|(name, keywords)| format!("{}{}{}{}", " ".repeat(indent),
                theme.details_language_name.paint(name),
                " ".repeat(language_width - calculate_widest_visible_line(name) + GAP), keywords)));
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

    // Asked of the entries and not of the map: a language whose every keyword was found nowhere has
    // a map full of zeros and nothing to print, and the caller drops an empty line and not a blank one
    let entries = create_keyword_entries(keyword_occurencies);
    if entries.is_empty() {
        return String::new();
    }

    let mut keyword_info = " ".repeat(indent);
    let mut used = indent;
    for (position, (name, count)) in entries.into_iter().enumerate() {
        let moved = baseline.map(|baseline| (baseline.get(&name).copied().unwrap_or(0), keyword_occurencies[&name]))
                .filter(|(was, is)| was != is)
                .map(|(was, is)| format!(" ({})", paint_change(theme, was, is, &format_signed_difference(was, is))));
        let change = moved.unwrap_or_default();
        let entry_len = calculate_widest_visible_line(&name) + 2 + calculate_widest_visible_line(&count)
                + calculate_widest_visible_line(&change);
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
// A keyword a language declares and no file used says nothing, so it gets no cell. The JSON keeps
// its zeros: a machine format whose fields appear and disappear per run is worse than a stable one.
fn create_keyword_entries(keyword_occurencies: &HashMap<String,usize>) -> Vec<(String, String)> {
    let mut sorted_keywords = keyword_occurencies.iter().filter(|(_, count)| **count > 0).collect::<Vec<_>>();
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
    let mut others = total.clone();
    let shown = Stats::total_of(&per_language);
    others.files -= shown.files;
    others.bytes -= shown.bytes;
    others.lines -= shown.lines;
    // The classes go with them, or the row would hold lines that no class of it accounts for and
    // whichever column is folded from them would read zero for everything that was folded away
    others.classes.subtract(&shown.classes);
    others.keyword_occurences.clear();
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
    // The scope cannot carry the targets, so they are compared beside it: the same './src'
    // declared over two different trees is two different measurements
    if sort(&entry.targets) != sort(targets) {
        changed.push(config_manager::TARGETS);
    }
    // The log holds no keyword counts, so a run that only stopped counting them changed nothing
    // the log records
    changed.extend(super::diff::find_settings_that_differ(&entry.scope,
            &super::diff::scope_of(&config.engine, config.view.counting))
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
fn format_module_comparison_lines(entry: &super::log::LogEntry, groups: &[Group],
        model: CountingModel) -> String {
    let theme = super::theme::get_active();
    let names = groups.iter().map(|x| x.get_displayed_name().to_owned())
            .chain(entry.modules.iter().map(|x| x.name.clone())
                    .filter(|name| !groups.iter().any(|x| x.get_displayed_name() == name)))
            .collect::<Vec<_>>();
    let width = names.iter().map(|x| calculate_widest_visible_line(x)).max().unwrap_or(0);

    // Right aligned down the entry, since the whole reason these are three narrow columns and not
    // the full breakdown is that they are meant to be read down rather than across
    let compared = names.iter().filter_map(|name| entry.modules.iter().find(|x| &x.name == name)).collect::<Vec<_>>();
    let number_width = |value: &dyn Fn(&super::log::ModuleEntry) -> usize|
            compared.iter().map(|x| format_with_separators(value(x)).len()).max().unwrap_or(0);
    let (lines_width, code_width, comments_width) = (number_width(&|x| x.lines),
            number_width(&|x| model.calculate_code_lines(&x.classes)),
            number_width(&|x| model.calculate_comment_lines(&x.classes)));

    let mut rendered = String::with_capacity(names.len() * 80);
    for name in &names {
        let padded = format!("       {}{}   ", theme.details_module.paint(name),
                " ".repeat(width - calculate_widest_visible_line(name)));
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
                        cell(&theme.code_number, now.calculate_code_lines(model),
                                model.calculate_code_lines(&then.classes), code_width),
                        cell(&theme.comments_number, now.calculate_comment_lines(model),
                                model.calculate_comment_lines(&then.classes), comments_width))
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

fn print_comparison_to_previous_runs(result: &RunResult, groups: &[Group], log_content: &str,
        config: &Configuration, datetime_now: &DateTime<Local>)
{
    println!("\n{}.\n", super::theme::get_active().heading.paint("History"));

    let total = &result.total;
    let log_entries = super::log::read_last_entries(log_content, config.view.compare_level);

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
        let model = config.view.counting;
        comparison_str.push_str(&format!("     Files: {}({}%) Lines: {}({}%) {{Code: {}({}%), Comments: {}({}%), {}: {}({}%)}}\n",
                super::theme::get_active().files_number.paint(&format_with_separators(entry.total.files)), paint_percentage(&format_signed_percentage_difference(entry.total.files, total.files)),
                super::theme::get_active().lines_number.paint(&format_with_separators(entry.total.lines)), paint_percentage(&format_signed_percentage_difference(entry.total.lines, total.lines)),
                super::theme::get_active().code_number.paint(&format_with_separators(entry.total.calculate_code_lines(model))), paint_percentage(&format_signed_percentage_difference(entry.total.calculate_code_lines(model), total.calculate_code_lines(model))),
                super::theme::get_active().comments_number.paint(&format_with_separators(entry.total.calculate_comment_lines(model))), paint_percentage(&format_signed_percentage_difference(entry.total.calculate_comment_lines(model), total.calculate_comment_lines(model))),
                get_third_column_header(model),
                super::theme::get_active().extra_number.paint(&format_with_separators(entry.total.calculate_extra_lines(model))), paint_percentage(&format_signed_percentage_difference(entry.total.calculate_extra_lines(model), total.calculate_extra_lines(model)))));
        // A run that named no module says nothing about them here either; the 'modified: targets'
        // tag is what already reports that the targets are not the ones they were
        if result.has_modules() {
            comparison_str.push_str(&format_module_comparison_lines(entry, groups, model));
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
    text.lines().map(crate::theme::measure_columns).max().unwrap_or(0)
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
    // keyword row long enough to wrap, a language with no keywords at all, a keyword that fired
    // nowhere, and five languages, one more than the overview shows without folding into "others".
    fn sample_data() -> (Vec<String>, HashMap<String, Stats>, Stats) {
        let per_language = hashmap![
            "Rust".to_owned() => crate::test_support::plain_stats_of(13, 416800, 9008, 6122, 505,
                    hashmap!["enums".to_owned() => 11, "structs".to_owned() => 29, "traits".to_owned() => 1]),
            "JavaScript".to_owned() => crate::test_support::plain_stats_of(4, 40000, 1200, 900, 120,
                    hashmap!["classes".to_owned() => 805, "functions".to_owned() => 1204, "generators".to_owned() => 17,
                             "promises".to_owned() => 96, "imports".to_owned() => 342]),
            // Every keyword it declares was found nowhere, so it prints no keyword line at all and
            // not a line of the indent alone
            "HTML".to_owned() => crate::test_support::plain_stats_of(2, 18800, 396, 361, 0, hashmap!["tags".to_owned() => 0]),
            // 'decorators' is declared and never used, so no layout may print a cell for it
            "Python".to_owned() => crate::test_support::plain_stats_of(3, 9000, 250, 200, 20,
                    hashmap!["classes".to_owned() => 2, "decorators".to_owned() => 0]),
            "Java".to_owned() => crate::test_support::plain_stats_of(1, 900, 80, 60, 5,
                    hashmap!["classes".to_owned() => 2, "interfaces".to_owned() => 1])];
        let total = Stats::total_of(&per_language);
        let sorted = get_sorted_language_names(&per_language, SortCriterion::Lines, CountingModel::Content);

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
            ModuleResult {name: name.map(str::to_owned), per_language, total,
                    nested_languages: HashMap::new(), files: HashMap::new()}
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
                    nested_languages: HashMap::new(), files: HashMap::new()}
        };

        vec![of(Some("backend"), vec![
                ("Rust", crate::test_support::plain_stats_of(11, 380000, 8104, 5510, 470,
                        hashmap!["enums".to_owned() => 9, "structs".to_owned() => 24, "traits".to_owned() => 1]))]),
             of(Some("frontend"), vec![
                ("JavaScript", crate::test_support::plain_stats_of(5, 52000, 1500, 1150, 140,
                        hashmap!["classes".to_owned() => 900, "functions".to_owned() => 1204, "generators".to_owned() => 17,
                                 "promises".to_owned() => 96, "imports".to_owned() => 400])),
                ("HTML", crate::test_support::plain_stats_of(2, 18800, 396, 361, 0, hashmap![]))]),
             of(None, vec![
                ("Go", crate::test_support::plain_stats_of(2, 7000, 210, 170, 12, hashmap!["structs".to_owned() => 4])),
                ("Java", crate::test_support::plain_stats_of(1, 900, 80, 60, 5,
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

        vec![ModuleResult {name: None, total: Stats::total_of(&per_language), per_language,
                nested_languages: HashMap::new(), files: HashMap::new()}]
    }

    fn reading_of(name: &str, taken: &str, modules: Vec<ModuleResult>) -> crate::diff::Reading {
        let per_language = merged(&modules);
        let total = Stats::total_of(&per_language);
        let files_present = FilesPresent {total_files: total.files, relevant_files: total.files, excluded_files: 0};

        crate::diff::Reading {
            source: crate::diff::Source::Document {path: name.to_owned()},
            taken: taken.to_owned(),
            version: "3.0.0".to_owned(),
            scope: crate::diff::scope_of(&mezura_core::EngineConfig::default(), CountingModel::Content),
            warnings: Vec::new(),
            faulty_files_count: 0,
            unreadable_dirs_count: 0,
            result: RunResult {total, per_language, modules, nested_languages: HashMap::new(),
                    faulty_files: Vec::new(), minified_files: 0, generated_files: 0, files_present, targets: Vec::new(),
                    unreadable_dirs: Vec::new(),
                    performance: mezura_core::Performance {duration_millis: 0, threads: mezura_core::Threads::new(1, 1)}}
        }
    }

    fn groups_from<'a>(modules: &'a [ModuleResult], config: &crate::config_manager::Configuration) -> Vec<Group<'a>> {
        let result = RunResult {per_language: HashMap::new(),
                modules: Vec::new(), nested_languages: HashMap::new(), total: Stats::default(), faulty_files: Vec::new(),
                minified_files: 0, generated_files: 0, files_present: FilesPresent::default(), targets: Vec::new(), unreadable_dirs: Vec::new(), performance: mezura_core::Performance { duration_millis: 0, threads: mezura_core::Threads::new(1, 1) }};
        let mut result = result;
        result.modules = modules.iter().map(|x| ModuleResult {
            name: x.name.clone(),
            per_language: x.per_language.clone(),
            total: Stats::total_of(&x.per_language),
            nested_languages: HashMap::new(),
            files: HashMap::new()
        }).collect();
        // The borrow has to outlive the temporary, so the groups are built against the caller's slice
        let order = create_groups_of(&result, config).into_iter().map(|x| (x.name.map(str::to_owned), x.languages, x.hidden))
                .collect::<Vec<_>>();

        order.into_iter().map(|(name, languages, hidden)| {
            let module = modules.iter().find(|x| x.name == name).unwrap();
            Group {name: module.name.as_deref(), languages, hidden, per_language: &module.per_language,
                    nested: &module.nested_languages, files: HashMap::new(),
                    total: &module.total, baseline: None}
        }).collect()
    }

    // Two of the HTML files and all three of the Python ones, adding up to exactly what those two
    // rows of 'sample_data' say: a file row is a share of its language, and a golden showing one
    // that is larger than the whole reads as a bug in whoever looks at it next.
    fn sample_files() -> HashMap<String, Vec<mezura_core::FileEntry>> {
        let entry = |path: &str, lines, code, comments, bytes| mezura_core::FileEntry {
            path: format!("D:/x/{path}"),
            stats: crate::test_support::plain_stats_of(1, bytes, lines, code, comments, hashmap![]),
            nested_languages: HashMap::new()
        };
        hashmap![
            "HTML".to_owned() => vec![
                entry("src/components/Views/Learn.html", 300, 275, 0, 14200),
                entry("index.html", 96, 86, 0, 4600)],
            "Python".to_owned() => vec![
                entry("app/models/repository.py", 120, 95, 10, 4400),
                entry("app/main.py", 80, 65, 6, 3000),
                entry("setup.py", 50, 40, 4, 1600)]]
    }

    fn sample_sections() -> HashMap<String, HashMap<String, Stats>> {
        hashmap!["HTML".to_owned() => hashmap![
                "JavaScript".to_owned() => crate::test_support::plain_stats_of(2, 4000, 200, 182, 0, hashmap![]),
                "Python".to_owned() => crate::test_support::plain_stats_of(1, 2000, 100, 91, 0, hashmap![])]]
    }

    fn render_every_layout() -> String {
        // Not left to the absence of a terminal: CLICOLOR_FORCE overrides that, and the verification
        // protocol in CLAUDE.md tells the reader to export it, so the same shell that ran a manual
        // comparison would otherwise fail this test with a wall of escape codes
        colored::control::set_override(false);

        let (sorted, content_info, total) = sample_data();
        let theme = &Theme::default();
        let no_hides = crate::config_manager::Hidden::default();
        let content = CountingModel::Content;
        let by_lines = |model| ViewSettings { sort_by: SortCriterion::Lines, hidden: no_hides, model };
        let shown = by_lines(content);
        let mut config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);
        let plain = vec![Group {name: None, languages: sorted.clone(), hidden: 0, per_language: &content_info,
                nested: &NO_NESTED, files: HashMap::new(), total: &total, baseline: None}];
        let columns = Columns::of(&plain, &total, no_hides, content);
        let width = columns.width(theme);

        // HTML holds the two sections, so every layout is rendered once without a container in it
        // and once with one. Its section names are longer than it is, since the name column is
        // measured over them too.
        let sections = sample_sections();
        let with_nested = vec![Group {name: None, languages: sorted.clone(), hidden: 0,
                per_language: &content_info, nested: &sections, files: HashMap::new(),
                total: &total, baseline: None}];
        let nested_columns = Columns::of(&with_nested, &total, no_hides, content);
        let nested_width = nested_columns.width(theme);

        let mut cases: Vec<(String, Vec<String>)> = Vec::new();
        let mut list = format_individual_lines(theme, &plain, &columns, width, true);
        list.extend(format_sum_lines(theme, &content_info, &total, &columns, width, true));
        cases.push(("list".to_owned(), list));
        cases.push(("list, keywords hidden".to_owned(),
                format_individual_lines(theme, &plain, &columns, width, false)));
        cases.push(("list, with nested languages".to_owned(),
                format_individual_lines(theme, &with_nested, &nested_columns, nested_width, false)));

        let mut table = format_table_lines(theme, &plain, &total, true, &[], shown);
        table.extend(format_keyword_block_lines(theme, &plain));
        cases.push(("table".to_owned(), table));
        cases.push(("table, with nested languages".to_owned(),
                format_table_lines(theme, &with_nested, &total, true, &[], shown)));

        let mut boxed = format_boxed_lines(theme, &plain, &total, true, &[], shown);
        boxed.extend(format_keyword_block_lines(theme, &plain));
        cases.push(("boxed".to_owned(), boxed));
        cases.push(("boxed, with nested languages".to_owned(),
                format_boxed_lines(theme, &with_nested, &total, true, &[], shown)));

        // HTML has sections and files under it, Python has files alone, and the other three have
        // neither. Python's list is cut, so its branch has to hang open where HTML's is drawn shut.
        let entries = sample_files();
        let files_of = |language: &str, hidden| FileRows {
            shown: entries[language].iter()
                    .map(|file| (Cow::Borrowed(file.path.trim_start_matches("D:/x/")), file)).collect(),
            hidden
        };
        let with_files = vec![Group {name: None, languages: sorted.clone(), hidden: 0,
                per_language: &content_info, nested: &sections,
                files: hashmap!["HTML" => files_of("HTML", 0), "Python" => files_of("Python", 4)],
                total: &total, baseline: None}];
        let file_columns = Columns::of(&with_files, &total, no_hides, content);
        let file_width = file_columns.width(theme);
        cases.push(("list, with files".to_owned(),
                format_individual_lines(theme, &with_files, &file_columns, file_width, false)));
        let a_note = vec!["(+4 more files hidden by --by-file 1)".to_owned()];
        cases.push(("table, with files".to_owned(),
                format_table_lines(theme, &with_files, &total, true, &a_note, shown)));
        cases.push(("boxed, with files".to_owned(),
                format_boxed_lines(theme, &with_files, &total, true, &a_note, shown)));

        // Two sentences are one paragraph: a blank line above the first, none between them
        let both_cuts = vec![Group {name: None, languages: sorted[..4].to_vec(), hidden: 1,
                per_language: &content_info, nested: &sections,
                files: hashmap!["HTML" => files_of("HTML", 0), "Python" => files_of("Python", 4)],
                total: &total, baseline: None}];
        let both_notes = vec!["(+1 more language hidden by --top 4)".to_owned(),
                "(+4 more files hidden by --by-file 1)".to_owned()];
        cases.push(("table, both notes".to_owned(),
                format_table_lines(theme, &both_cuts, &total, true, &both_notes, shown)));

        // Hidden columns: the numbers stay, their shares go, and two whole columns are gone
        let trimmed = crate::config_manager::Hidden { size: true, extra: true, percentages: true,
                ..crate::config_manager::Hidden::default() };
        cases.push(("table, columns hidden".to_owned(),
                format_table_lines(theme, &with_nested, &total, true, &[], ViewSettings { hidden: trimmed, ..shown })));
        cases.push(("boxed, columns hidden".to_owned(),
                format_boxed_lines(theme, &with_nested, &total, true, &[], ViewSettings { hidden: trimmed, ..shown })));
        let trimmed_columns = Columns::of(&with_nested, &total, trimmed, content);
        cases.push(("list, columns hidden".to_owned(),
                format_individual_lines(theme, &with_nested, &trimmed_columns, trimmed_columns.width(theme), false)));

        // The files row goes whole when both of its halves are hidden
        let no_files_row = crate::config_manager::Hidden { files: true, size: true,
                ..crate::config_manager::Hidden::default() };
        let bare_columns = Columns::of(&plain, &total, no_files_row, content);
        let mut bare = format_individual_lines(theme, &plain, &bare_columns, bare_columns.width(theme), false);
        bare.extend(format_sum_lines(theme, &content_info, &total, &bare_columns, bare_columns.width(theme), false));
        cases.push(("list, no files row".to_owned(), bare));

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
        let columns = Columns::of(&groups, &total, no_hides, content);
        let width = columns.width(theme);

        let mut list = format_individual_lines(theme, &groups, &columns, width, true);
        list.extend(format_sum_lines(theme, &content_info, &total, &columns, width, true));
        cases.push(("modules, list".to_owned(), list));

        let mut table = format_table_lines(theme, &groups, &total, true, &[], shown);
        table.extend(format_keyword_block_lines(theme, &groups));
        cases.push(("modules, table".to_owned(), table));

        let mut boxed = format_boxed_lines(theme, &groups, &total, true, &[], shown);
        boxed.extend(format_keyword_block_lines(theme, &groups));
        cases.push(("modules, boxed".to_owned(), boxed));

        // '--top' is per module, so it cuts inside each one and not across the report. A cut here
        // means a note, which is where the blank line a module opens and the one a note opens meet.
        config.view.top_n = Some(1);
        let groups = groups_from(&modules, &config);
        let note = vec!["(+3 more languages hidden by --top 1)".to_owned()];
        cases.push(("modules, table, top 1".to_owned(),
                format_table_lines(theme, &groups, &total, true, &note, shown)));

        config.view.top_n = None;
        config.view.sort_by = SortCriterion::Name;
        let groups = groups_from(&modules, &config);
        cases.push(("modules, table, sorted by name".to_owned(),
                format_table_lines(theme, &groups, &total, true, &[],
                        ViewSettings { sort_by: SortCriterion::Name, ..shown })));

        // The rows of the matrix are the languages of the whole run, and each of them is three
        // physical rows, so the second case is the one where a module does not have the language
        // at all and only the middle of the three carries a dash
        config.view.sort_by = SortCriterion::Lines;
        let groups = groups_from(&modules, &config);
        cases.push(("modules, matrix".to_owned(),
                format_matrix_lines(theme, &groups, &sorted, &total, true, content)));
        cases.push(("modules, matrix, top 2".to_owned(),
                format_matrix_lines(theme, &groups, &sorted[..2], &total, true, content)));
        cases.push(("modules, matrix, no total".to_owned(),
                format_matrix_lines(theme, &groups, &sorted[..1], &total, false, content)));

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
        let mut comparison = format_comparison_lines(theme, &rows, ViewSettings::of(&config));
        comparison.extend(format_keyword_block_lines(theme, &[create_group_with_baseline(None, &before.result.per_language,
                &now.result.per_language, &now.result.total, &config)]));
        cases.push(("comparison".to_owned(), headed(comparison, &before, &now)));
        cases.push(("comparison, boxed".to_owned(),
                headed(format_boxed_comparison_lines(theme, &rows, ViewSettings::of(&config)), &before, &now)));

        // A comparison obeys the column names too. Its '%' cells are percentages of the change, so
        // 'percentages' takes them and leaves the absolute move, and every change follows its own
        // figure out rather than being left beside a column that is gone.
        let trimmed = crate::config_manager::Hidden { files: true, size: true, percentages: true,
                ..crate::config_manager::Hidden::default() };
        cases.push(("comparison, columns hidden".to_owned(),
                headed(format_comparison_lines(theme, &rows,
                        ViewSettings { hidden: trimmed, ..ViewSettings::of(&config) }), &before, &now)));
        cases.push(("comparison, boxed, columns hidden".to_owned(),
                headed(format_boxed_comparison_lines(theme, &rows,
                        ViewSettings { hidden: trimmed, ..ViewSettings::of(&config) }), &before, &now)));

        // The same two readings with a second axis through them, which is shown because they named
        // the same modules
        let (before, now) = (reading_of("older.json", EARLIER, earlier),
                reading_of("newer.json", LATER, sample_modules()));
        let pairs = crate::diff::pair_modules(&before.result, &now.result).unwrap();
        let grouped_keywords = |config: &crate::config_manager::Configuration| pairs.iter()
                .map(|pair| create_group_with_baseline(pair.name, &pair.before.per_language, &pair.now.per_language,
                        &pair.now.total, config)).collect::<Vec<_>>();

        let rows = create_compared_rows(Some(&pairs), &before.result, &now.result, &config);
        let mut comparison = format_comparison_lines(theme, &rows, ViewSettings::of(&config));
        comparison.extend(format_keyword_block_lines(theme, &grouped_keywords(&config)));
        cases.push(("comparison, modules".to_owned(), headed(comparison, &before, &now)));

        let mut comparison = format_boxed_comparison_lines(theme, &rows, ViewSettings::of(&config));
        comparison.extend(format_keyword_block_lines(theme, &grouped_keywords(&config)));
        cases.push(("comparison, modules, boxed".to_owned(), headed(comparison, &before, &now)));

        // '--top' cuts inside each module here as it does everywhere else
        config.view.top_n = Some(1);
        cases.push(("comparison, modules, top 1".to_owned(), headed(format_comparison_lines(theme,
                &create_compared_rows(Some(&pairs), &before.result, &now.result, &config),
                ViewSettings::of(&config)), &before, &now)));

        // The two models, over data that tells them apart. Every one of the nine classes is in
        // play, so each model answers differently in every column and a block handed the wrong one,
        // or ignoring it and folding the default, cannot pass. The dataset is hand counted and the
        // arithmetic is written out beside it, since these are the only cases here whose numbers
        // are the subject rather than the alignment.
        //
        // Rust: 100 + 7 + 9 + 40 + 20 + 3 + 25 + 2 + 1 = 207 lines.
        //   content  107 code (100 + 7), 49 comments (9 + 40), 51 extra (20 + 3 + 25 + 2 + 1)
        //   region   137 code (100 + 7 + 9 + 20 + 1), 45 comments (40 + 3 + 2), 25 blanks
        // Lua: 60 + 3 + 2 + 15 + 8 + 1 + 10 + 4 + 2 = 105 lines.
        //   content  63 code, 17 comments, 25 extra
        //   region   75 code (60 + 3 + 2 + 8 + 2), 20 comments (15 + 1 + 4), 10 blanks
        let every_class = hashmap![
            "Rust".to_owned() => crate::test_support::stats_of(9, 8000, mezura_core::LineClasses {
                    words_in_code: 100, string_content: 7, comment_words_beside_code: 9,
                    words_in_comment: 40, punctuation_in_code: 20, punctuation_in_comment: 3,
                    blank: 25, blank_in_comment: 2, blank_in_string: 1 },
                    hashmap!["structs".to_owned() => 12]),
            "Lua".to_owned() => crate::test_support::stats_of(4, 3000, mezura_core::LineClasses {
                    words_in_code: 60, string_content: 3, comment_words_beside_code: 2,
                    words_in_comment: 15, punctuation_in_code: 8, punctuation_in_comment: 1,
                    blank: 10, blank_in_comment: 4, blank_in_string: 2 }, hashmap![])];
        let every_class_total = Stats::total_of(&every_class);
        let both_models = vec![Group { name: None, hidden: 0,
                languages: get_sorted_language_names(&every_class, SortCriterion::Lines, content),
                per_language: &every_class, nested: &NO_NESTED, files: HashMap::new(),
                total: &every_class_total, baseline: None }];

        for (label, model) in [("content", content), ("region", CountingModel::Region)] {
            let columns = Columns::of(&both_models, &every_class_total, no_hides, model);
            cases.push((format!("every class, {label}, list"),
                    format_individual_lines(theme, &both_models, &columns, columns.width(theme), false)));
            cases.push((format!("every class, {label}, table"), format_table_lines(theme, &both_models,
                    &every_class_total, true, &[], by_lines(model))));
            cases.push((format!("every class, {label}, boxed"), format_boxed_lines(theme, &both_models,
                    &every_class_total, true, &[], by_lines(model))));
        }

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

        let content_info = hashmap!["D".to_owned() => crate::test_support::plain_stats_of(1, 24, 2, 2, 0, hashmap![])];
        let total = Stats::total_of(&content_info);
        fn group<'a>(name: Option<&'a str>, content_info: &'a HashMap<String, Stats>,
                total: &'a Stats) -> Group<'a> {
            Group {name, languages: vec!["D".to_owned()], hidden: 0, per_language: content_info,
                    nested: &NO_NESTED, files: HashMap::new(), total, baseline: None}
        }
        let groups = vec![group(Some("a"), &content_info, &total),
                group(None, &content_info, &total)];

        let theme = &Theme::default();
        let columns = Columns::of(&groups, &total, crate::config_manager::Hidden::default(),
                CountingModel::Content);
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
                    crate::test_support::plain_stats_of(2, 4000, 100, 70, 10, hashmap!["structs".to_owned() => structs])];
            ModuleResult {name: Some(name.to_owned()), total: Stats::total_of(&per_language), per_language,
                    nested_languages: HashMap::new(), files: HashMap::new()}
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
            per_language: content_info.clone(), modules, nested_languages: HashMap::new(),
            total: crate::test_support::plain_stats_of(23, 485500, 10934, 7643, 650, hashmap![]),
            faulty_files: Vec::new(), minified_files: 0, generated_files: 0, files_present: FilesPresent::default(), targets: Vec::new(), unreadable_dirs: Vec::new(), performance: mezura_core::Performance { duration_millis: 0, threads: mezura_core::Threads::new(1, 1) }};
        let single = || vec![ModuleResult {name: None, per_language: content_info.clone(),
                total: Stats::total_of(&content_info), nested_languages: HashMap::new(), files: HashMap::new()}];

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

    // The golden hands the blocks a map of files it built itself, so it says nothing about which
    // files the real entry point puts in it. That is what this holds: the cut happens inside each
    // module, and a language '--top' hid takes its files with it.
    #[test]
    fn the_file_rows_are_cut_inside_each_module_and_survive_every_layout() {
        colored::control::set_override(false);

        let (_, content_info, total) = sample_data();
        let entries = sample_files();
        let of_module = |name: Option<&str>, languages: &[&str]| {
            let per_language = languages.iter().map(|x| ((*x).to_owned(), content_info[*x].clone()))
                    .collect::<HashMap<_,_>>();
            ModuleResult { name: name.map(str::to_owned), total: Stats::total_of(&per_language),
                    per_language, nested_languages: HashMap::new(),
                    files: entries.iter().filter(|(language, _)| languages.contains(&language.as_str()))
                            .map(|(language, files)| (language.clone(), files.clone())).collect() }
        };
        let of_modules = |modules: Vec<ModuleResult>| RunResult {
            per_language: content_info.clone(), modules, nested_languages: HashMap::new(),
            total: total.clone(), faulty_files: Vec::new(), minified_files: 0, generated_files: 0, files_present: FilesPresent::default(),
            targets: vec![mezura_core::Target::of("D:/x")], unreadable_dirs: Vec::new(),
            performance: mezura_core::Performance { duration_millis: 0, threads: mezura_core::Threads::new(1, 1) }};

        let split = || vec![of_module(Some("web"), &["HTML"]), of_module(None, &["Python", "Rust"])];
        let config_of = |by_file, top_n| {
            let mut config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);
            config.view.by_file = by_file;
            config.view.top_n = top_n;
            config
        };

        // Every language keeps its own biggest two, whatever the languages beside it hold: the two
        // HTML files are both shown and the three Python ones lose their smallest
        let one_module = || vec![of_module(None, &["HTML", "Python", "Rust"])];
        let together = of_modules(one_module());
        let two = find_files_to_show(&together, &config_of(Some(config_manager::ByFile::Capped(2)), None));
        assert_eq!(vec![("HTML", 2, 0), ("Python", 2, 1)], sorted_rows(&two[0]));

        // Biggest first inside each language
        assert_eq!(vec!["Learn.html", "index.html"], names_of(&two[0], "HTML"));
        assert_eq!(vec!["repository.py", "main.py"], names_of(&two[0], "Python"));

        // A language that '--top' left out has no row for its files to sit under, so they are not
        // candidates at all rather than rows with no parent
        let all_of = |result: &RunResult, config: &Configuration| find_files_to_show(result, config)
                .iter().flat_map(HashMap::values).map(|rows| rows.shown.len()).sum::<usize>();
        assert_eq!(5, all_of(&of_modules(one_module()), &config_of(Some(config_manager::ByFile::All), None)));
        assert_eq!(0, all_of(&of_modules(one_module()), &config_of(Some(config_manager::ByFile::All), Some(1))));
        assert_eq!(0, all_of(&of_modules(one_module()), &config_of(None, None)));

        // and the cut is inside a module as well as inside a language, so a module holding smaller
        // files than its neighbour still shows its own
        let across_two_modules = of_modules(split());
        let split_two = find_files_to_show(&across_two_modules, &config_of(Some(config_manager::ByFile::Capped(2)), None));
        assert_eq!(vec![("HTML", 2, 0)], sorted_rows(&split_two[0]));
        assert_eq!(vec![("Python", 2, 1)], sorted_rows(&split_two[1]));

        for layout in [Layout::List, Layout::Table, Layout::Boxed, Layout::Matrix] {
            for by_file in [None, Some(config_manager::ByFile::Capped(1)), Some(config_manager::ByFile::All)] {
                for top in [None, Some(1)] {
                    let mut config = config_of(by_file, top);
                    config.view.layout = layout;
                    format_and_print_results(&of_modules(one_module()), &None, &Local::now(), &config);
                    format_and_print_results(&of_modules(split()), &None, &Local::now(), &config);
                }
            }
        }
    }

    // What each language of one module shows and what it left out, by language name
    fn sorted_rows<'a>(of_module: &FileRowsOfModule<'a>) -> Vec<(&'a str, usize, usize)> {
        let mut rows = of_module.iter().map(|(language, files)|
                (*language, files.shown.len(), files.hidden)).collect::<Vec<_>>();
        rows.sort();
        rows
    }

    fn names_of(of_module: &FileRowsOfModule, language: &str) -> Vec<String> {
        of_module[language].shown.iter()
                .map(|(path, _)| path.rsplit('/').next().unwrap().to_owned()).collect()
    }

    // The header that carries the sort marker arrives already painted, so its bytes are several
    // times what it draws. Every width of a frame has to be measured with the escapes skipped, and
    // the one subtraction that was not asked the allocator for nine exabytes. The golden cannot see
    // this: it turns colour off, and without escapes the two counts differ by two rather than by
    // thirty. The escapes are written by hand here for the same reason, so that nothing has to touch
    // the colour override that every other test in this file depends on.
    #[test]
    fn a_header_that_arrives_painted_is_measured_by_what_it_draws() {
        let theme = Theme::default();
        let painted = format!("\u{1b}[38;2;181;169;138m{SORTED_DESCENDING}\u{1b}[0m Lines");
        let headers = ["Language", "Files", painted.as_str()];
        let rows = vec![("Rust".to_owned(), vec![
                BoxedCell { number: "18".to_owned(), slot: "100.00%".to_owned() },
                BoxedCell { number: "9,355".to_owned(), slot: "100.00%".to_owned() }])];
        let styles = [&theme.details_language_header, &theme.files_label, &theme.lines_label];
        let numbers = [&theme.files_number, &theme.lines_number];

        let columns = [ColumnKind::Name, ColumnKind::Files, ColumnKind::Lines];
        let lines = draw_boxed_table(&theme, "Language", &headers, &rows, &[RowKind::Language],
                &columns, ColumnKind::Percent, &styles, &numbers);

        let widths = lines.iter().map(|line| calculate_widest_visible_line(line)).collect::<Vec<_>>();
        assert!(widths.windows(2).all(|pair| pair[0] == pair[1]),
                "the frame is not square once a header carries escapes: {widths:?}");
    }

    // The row shows what is left of the path under the target it was found in, since the target is
    // the one part of it the reader already knows
    #[test]
    fn a_file_row_drops_the_target_it_was_found_under() {
        let shortened = |path: &str, targets: &[mezura_core::Target]|
                shorten_path(path, targets, find_common_directory_of(targets)).into_owned();
        let targets = [mezura_core::Target::of("D:/x/api"), mezura_core::Target::of("D:/x")];

        assert_eq!("src/main.rs", shortened("D:/x/api/src/main.rs", &targets));
        assert_eq!("web/index.html", shortened("D:/x/web/index.html", &targets));
        // A target that is the file itself leaves nothing behind, so the directory it sits in goes
        assert_eq!("build.rs", shortened("D:/x/api/build.rs", &[mezura_core::Target::of("D:/x/api/build.rs")]));
        // And a path nothing matches at all keeps its name, which is all that is certainly its own
        assert_eq!("elsewhere.rs", shortened("D:/other/elsewhere.rs", &targets));

        // A glob is one target per file it matched, so without the directory they share every row
        // would read 'main.rs' and two of them would be the same row twice
        let matched = [mezura_core::Target::of("D:/x/api/main.rs"), mezura_core::Target::of("D:/x/web/main.rs")];
        assert_eq!("api/main.rs", shortened("D:/x/api/main.rs", &matched));
        assert_eq!("web/main.rs", shortened("D:/x/web/main.rs", &matched));

        // The shared directory is measured in whole components, or 'D:/xyz' counts as being in 'D:/x'
        assert_eq!("stray.rs", shortened("D:/xyz/stray.rs", &matched));
    }

    // A column as wide as the deepest path in a large repository is a column nothing else fits in,
    // so what is too wide loses whole directories out of its middle
    #[test]
    fn a_path_too_wide_for_its_column_loses_whole_directories_and_never_a_name() {
        let targets = [mezura_core::Target::of("D:/x")];
        let shortened = |path: &str| shorten_path(path, &targets, find_common_directory_of(&targets)).into_owned();

        // Untouched while it fits, and the last directories are the ones kept when it does not
        assert_eq!("src/components/Views/Learn.vue", shortened("D:/x/src/components/Views/Learn.vue"));
        assert_eq!("src/.../Reusable/Shapes/ShapesSelector.vue",
                shortened("D:/x/src/components/Reusable/Shapes/ShapesSelector.vue"));
        assert_eq!("src/.../PatternRecognitionEditorWrapper.vue",
                shortened("D:/x/src/components/Reusable/BoardsSpecialized/PatternRecognitionEditorWrapper.vue"));

        // Nothing is ever shown wider than it started, and a name too wide on its own is the floor:
        // half a name reads as a different file
        let huge = "AnEntirelyUnreasonableNameForOneSingleSourceFileIndeed.vue";
        assert_eq!(format!(".../{huge}"), shortened(&format!("D:/x/src/components/deep/{huge}")));
        assert_eq!(huge, shortened(&format!("D:/x/{huge}")));

        for path in ["D:/x/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/t/u/v/w/x/y/z/end.rs",
                     &format!("D:/x/{huge}"), &format!("D:/x/one/{huge}")] {
            assert!(shortened(path).chars().count() <= path.trim_start_matches("D:/x/").chars().count(),
                    "'{path}' came out wider than it went in: '{}'", shortened(path));
        }
    }

    // The one block whose row count is not the language count
    #[test]
    fn the_nested_languages_of_a_container_survive_every_layout() {
        colored::control::set_override(false);

        let (_, content_info, total) = sample_data();
        let nested = hashmap!["HTML".to_owned() => hashmap![
                "JavaScript".to_owned() => crate::test_support::plain_stats_of(2, 4000, 200, 182, 0, hashmap![]),
                "Python".to_owned() => crate::test_support::plain_stats_of(1, 2000, 100, 91, 0, hashmap![])]];
        let a_run = || RunResult {
            per_language: content_info.clone(),
            modules: vec![ModuleResult { name: None, per_language: content_info.clone(),
                    total: Stats::total_of(&content_info), nested_languages: nested.clone(), files: HashMap::new() }],
            nested_languages: nested.clone(), total: total.clone(), faulty_files: Vec::new(),
            minified_files: 0, generated_files: 0, files_present: FilesPresent::default(), targets: Vec::new(), unreadable_dirs: Vec::new(),
            performance: mezura_core::Performance { duration_millis: 0, threads: mezura_core::Threads::new(1, 1) }};

        for layout in [Layout::List, Layout::Table, Layout::Boxed, Layout::Matrix] {
            for top in [None, Some(1), Some(3), Some(6)] {
                for hidden in [false, true] {
                    let mut config = crate::config_manager::Configuration::new(vec!["./".to_owned()]);
                    config.view.layout = layout;
                    config.view.top_n = top;
                    config.view.hidden.nested_languages = hidden;
                    format_and_print_results(&a_run(), &None, &Local::now(), &config);
                }
            }
        }

        let group = Group { name: None, languages: vec!["HTML".to_owned()], hidden: 0,
                per_language: &content_info, nested: &nested, files: HashMap::new(),
                total: &total, baseline: None };
        let whole = content_info.get("HTML").unwrap();
        let sections = find_sections_of(&group, "HTML", whole);
        assert_eq!(format!("HTML {SHELL_SUFFIX}"), sections[0].0, "the shell is not the first row");
        assert_eq!(vec!["JavaScript".to_owned(), "Python".to_owned()],
                sections[1..].iter().map(|(name, _)| name.clone()).collect::<Vec<_>>(),
                "the sections are not ordered by lines");
        let code_of = |stats: &Stats| stats.calculate_code_lines(CountingModel::Content);
        assert_eq!(whole.lines - 300, sections[0].1.lines);
        assert_eq!(code_of(whole) - 273, code_of(&sections[0].1));
        // Every column answers "of the container", so the file count of a section is how many of its
        // files hold that section, and the shell is in all of them
        assert_eq!(whole.files, sections[0].1.files);

        // A document holding sections larger than their container must not take the run down.
        // Built by hand and not through the helper, whose whole job is to refuse counts that no
        // file could have produced: this one is exactly such a document. Its lines are the
        // container's and its classes are nothing, which is the shape that tells a shell taking
        // the two apart from one keeping them together: subtracting them separately leaves every
        // class of the container behind a line count of zero.
        let broken = hashmap!["HTML".to_owned() => hashmap![
                "JavaScript".to_owned() => Stats::new(9, 99999, 9999, mezura_core::LineClasses::default(),
                        hashmap![])]];
        let group = Group { name: None, languages: vec!["HTML".to_owned()], hidden: 0,
                per_language: &content_info, nested: &broken, files: HashMap::new(),
                total: &total, baseline: None };
        let shell = &find_sections_of(&group, "HTML", whole)[0].1;
        for model in [CountingModel::Content, CountingModel::Region] {
            assert!(shell.calculate_code_lines(model) + shell.calculate_comment_lines(model) <= shell.lines,
                    "under {} the shell holds more code and comments than it has lines: {shell:?}",
                    model.name());
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
            "A".to_owned() => crate::test_support::plain_stats_of(1, 60, 30, 0, 0, hashmap![]),
            "B".to_owned() => crate::test_support::plain_stats_of(2, 30, 10, 0, 0, hashmap![]),
            "C".to_owned() => crate::test_support::plain_stats_of(7, 10, 60, 0, 0, hashmap![]));
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
            "Zig".to_owned() => crate::test_support::plain_stats_of(9, 10, 100, 50, 0, HashMap::new()),
            "Ada".to_owned() => crate::test_support::plain_stats_of(1, 900, 100, 90, 0, HashMap::new()),
            "Rust".to_owned() => crate::test_support::plain_stats_of(5, 50, 300, 10, 0, HashMap::new())];

        let sorted = |criterion| get_sorted_language_names(&content, criterion, CountingModel::Content);
        assert_eq!(vec!["Rust","Ada","Zig"], sorted(SortCriterion::Lines));
        assert_eq!(vec!["Zig","Rust","Ada"], sorted(SortCriterion::Files));
        assert_eq!(vec!["Ada","Rust","Zig"], sorted(SortCriterion::Size));
        assert_eq!(vec!["Ada","Zig","Rust"], sorted(SortCriterion::Code));
        assert_eq!(vec!["Ada","Rust","Zig"], sorted(SortCriterion::Name));

        // Ada and Zig both have 100 lines, so the name decides, not the iteration order of the map
        assert_eq!(vec!["Rust","Ada","Zig"], sorted(SortCriterion::Lines));
    }

    #[test]
    fn test_retain_most_relevant_and_add_others_field_for_rest() {
        let sorted_language_names = vec!["a".to_owned(), "b".to_owned(), "c".to_owned(), "d".to_owned(), "e".to_owned()];
        let per_language = hashmap![
            "a".to_owned() => crate::test_support::plain_stats_of(10, 60000, 1000, 800, 0, hashmap![]),
            "b".to_owned() => crate::test_support::plain_stats_of(9, 50000, 900, 700, 0, hashmap![]),
            "c".to_owned() => crate::test_support::plain_stats_of(8, 40000, 800, 600, 0, hashmap![]),
            "d".to_owned() => crate::test_support::plain_stats_of(7, 30000, 700, 500, 0, hashmap![]),
            "e".to_owned() => crate::test_support::plain_stats_of(6, 20000, 600, 400, 0, hashmap![])
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
            "a".to_owned() => crate::test_support::plain_stats_of(10, 60000, 1000, 800, 0, hashmap![]),
            "b".to_owned() => crate::test_support::plain_stats_of(9, 50000, 900, 700, 0, hashmap![]),
            "c".to_owned() => crate::test_support::plain_stats_of(8, 40000, 800, 600, 0, hashmap![]),
            // The leftovers carry the files, the bytes and the classes of what was folded away as
            // well as the lines, so the overview's shares are shares of the whole run in all three
            // of its bars, and the row's own columns are the ones its languages had: 'd' and 'e'
            // held 500 and 400 lines of code between their 1300
            "others".to_owned() => crate::test_support::plain_stats_of(13, 50000, 1300, 900, 0, hashmap![])
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
                    scope: crate::diff::scope_of(&then.engine, then.view.counting), targets: Vec::new(),
                    total: crate::test_support::plain_stats_of(1, 1, 1, 1, 0, hashmap![]), modules: Vec::new() }
        };

        // Taken with the same settings: nothing to say
        let same = entry_of(|_| {});
        assert!(find_settings_changed_since(&same, &config, &[]).is_empty());
        assert!(format_modified_tag(&find_settings_changed_since(&same, &config, &[])).is_empty());

        // One that differs is named, and the names are the ones the reader can look up. The
        // counting model is not one of them: the entry records every class of line, so it is
        // folded by the model on screen whichever one it was written under.
        config.view.counting = CountingModel::Region;
        config.engine.no_gitignore = true;
        let changed = find_settings_changed_since(&same, &config, &[]);
        assert_eq!(vec!["no-gitignore"], changed);
        assert!(format_modified_tag(&changed).contains("no-gitignore"));

        // The targets are compared as a set: the same list reordered is the same measurement
        let entry_with_targets = crate::log::LogEntry {
                targets: vec![mezura_core::Target::of("./b"), mezura_core::Target::of("./a")],
                ..entry_of(|c| {c.engine.no_gitignore = true;}) };
        assert!(find_settings_changed_since(&entry_with_targets, &config,
                &[mezura_core::Target::of("./a"), mezura_core::Target::of("./b")]).is_empty());
        assert_eq!(vec!["targets"], find_settings_changed_since(&entry_with_targets, &config,
                &[mezura_core::Target::of("./c")]));

        // and a run that only stopped counting keywords changed nothing the log holds
        config.engine.count_keywords = false;
        assert!(find_settings_changed_since(&entry_with_targets, &config,
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
