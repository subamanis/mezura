use std::collections::HashSet;

use colored::{ColoredString, Colorize};
#[cfg(test)]
use colored::Color;
use mezura_core::{CountingModel, EngineConfig, ForcedLanguages, LanguageNames, Target, Threads};
use mezura_core::engine::config::{MAX_CONSUMERS_VALUE, MAX_PRODUCERS_VALUE, MIN_CONSUMERS_VALUE, MIN_PRODUCERS_VALUE};

use super::message_printer::{Formatted, wrap_message};
use super::paths::LocalDir;
use super::{message_printer, suggestions, theme::Theme};

// Printed at startup and by '--version'. Also in mezura/Cargo.toml, and the two move together.
pub const VERSION_ID : &str = "v3.0.0";

// command flags
pub const TARGETS            :&str   = "targets";
pub const EXCLUDE            :&str   = "exclude";
pub const LANGUAGES          :&str   = "languages";
pub const EXCLUDE_LANGUAGES  :&str   = "exclude-languages";
pub const FORCE_LANGUAGE     :&str   = "force-language";
pub const THREADS            :&str   = "threads";
pub const COUNTING           :&str   = "counting";
pub const SEARCH_IN_DOTTED   :&str   = "search-in-dotted";
pub const COUNT_MINIFIED     :&str   = "count-minified";
pub const COUNT_GENERATED    :&str   = "count-generated";
pub const COUNT_NOT_CODE     :&str   = "count-not-code";
pub const SHOW_FAULTY_FILES  :&str   = "show-faulty-files";
pub const SHOW_SKIPPED       :&str   = "show-skipped";
pub const HIDE               :&str   = "hide";
pub const NO_GITIGNORE       :&str   = "no-gitignore";
pub const NO_IGNORE_FILES    :&str   = "no-ignore-files";
pub const NO_HEURISTICS      :&str   = "no-heuristics";
pub const THEME              :&str   = "theme";
pub const STYLE              :&str   = "style";
pub const BAR_THICKNESS      :&str   = "bar-thickness";
pub const PROGRESS_BAR       :&str   = "progress-bar";
pub const LAYOUT             :&str   = "layout";
pub const OUTPUT             :&str   = "output";
pub const EXPLAIN            :&str   = "explain";
pub const DIFF               :&str   = "diff";
pub const NUMBER_SEPARATOR   :&str   = "number-separator";
pub const DECIMAL_SEPARATOR  :&str   = "decimal-separator";
pub const SORT               :&str   = "sort";
pub const TOP                :&str   = "top";
pub const BY_FILE            :&str   = "by-file";
pub const LOG                :&str   = "log";
pub const COMPARE_LEVEL     :&str   = "compare";
pub const SAVE               :&str   = "save";
pub const SAVE_THEME         :&str   = "save-theme";
pub const SAVE_LOCAL         :&str   = "save-local";
pub const LOAD               :&str   = "load";
pub const NO_LOCAL           :&str   = "no-local";
pub const HELP               :&str   = "help";
pub const VERSION            :&str   = "version";
pub const CHANGELOG          :&str   = "changelog";
pub const SHOW_LANGUAGES     :&str   = "show-languages";
pub const SHOW_CONFIGS       :&str   = "show-configs";
pub const SHOW_THEMES        :&str   = "show-themes";
pub const THEME_EDITOR       :&str   = "theme-editor";
pub const RESTORE            :&str   = "restore";

pub const MIN_COMPARE_LEVEL   : usize = 0;
pub const MAX_COMPARE_LEVEL   : usize = 10;

// default config values
const DEF_SHOW_FAULTY_FILES : bool    = false;
const DEF_COMPARE_LEVEL     : usize   = 1;

// What the always-loaded configuration is called in a message about it, not a file name
const DEFAULT_CONFIG_LABEL  : &str    = "default";

// The commands whose value decides what is counted, as against how the count is shown. A project's
// own configuration is answered for these by the program's defaults and never by this machine's
// saved ones, and a value of theirs this build cannot read stops the run rather than warning.
const CHANGES_THE_NUMBERS   : [&str; 13] = [TARGETS, EXCLUDE, LANGUAGES, EXCLUDE_LANGUAGES,
        FORCE_LANGUAGE, COUNTING, SEARCH_IN_DOTTED, COUNT_MINIFIED, COUNT_GENERATED, COUNT_NOT_CODE,
        NO_GITIGNORE, NO_IGNORE_FILES, NO_HEURISTICS];

// Two halves: the engine is handed only what can change a number, the presentation everything,
// since echoing what the counting was done with is part of its job. The command line and the
// configuration file stay flat, and only 'build' knows that '--hide keywords' answers both.
#[derive(Debug,PartialEq,Clone,Default)]
pub struct Configuration {
    pub engine: EngineConfig,
    pub view: ViewConfig,
    pub typed_explicitly: TypedExplicitlyOnCommandLine
}

impl Configuration {
    #[cfg(test)]
    pub fn new(targets: Vec<String>) -> Self {
        Configuration { engine: EngineConfig::new(targets), view: ViewConfig::default(),
                typed_explicitly: TypedExplicitlyOnCommandLine::default() }
    }

    // One flag answering two questions, so both halves are set together and never one alone
    #[cfg(test)]
    pub fn set_hidden(&mut self, hidden: Hidden) -> &mut Self {
        self.engine.count_keywords = !hidden.keywords;
        self.view.hidden = hidden;
        self
    }
}

// Everything that decides how the answer is shown, saved and logged. The engine never sees it.
#[derive(Debug,PartialEq,Clone)]
pub struct ViewConfig {
    pub version: &'static str,
    // Which configuration file supplied the targets, when one did, so a run refusing them can name
    // the file the reader cannot see failing
    pub targets_source: Option<String>,
    pub should_show_faulty_files: bool,
    pub should_show_skipped_files: bool,
    pub hidden: Hidden,
    pub log: LogOption,
    pub compare_level: usize,
    pub config_name_to_save: Option<String>,
    pub config_name_to_load: Option<String>,
    pub theme_name_to_save: Option<String>,
    // The project's own folder, when the run is inside one and was not told to ignore it. It
    // decides where a log with no configuration named goes, and it says whether the project's
    // settings were the ones this run counted with.
    pub local_dir: Option<LocalDir>,
    pub bar_thickness: BarThickness,
    pub progress_bar: ProgressBarStyle,
    pub layout: Layout,
    pub output: OutputFormat,
    // One file explained line by line instead of a report, and which of its lines to print.
    // Command line only, like '--output'.
    pub explain: Option<ExplainedLines>,
    // The document this run is compared against, as the path was typed. Read after the settings are
    // built and before anything is counted, so a baseline that is not one costs no scan.
    pub diff_against: Option<String>,
    pub number_separator: NumberSeparator,
    pub decimal_separator: DecimalSeparator,
    pub sort_by: SortCriterion,
    pub top_n: Option<usize>,
    pub by_file: Option<ByFile>,
    // Which fold of the classes every shown number goes through. In the view and not the engine,
    // because the engine only ever fills the classes and both models are answered by one run.
    pub counting: CountingModel,
    pub theme: Theme
}

impl ViewConfig {
    // Everything that is not the document itself stays off stdout when the output is machine
    // readable, so that a single stray line cannot make it unparseable
    pub fn prints_text(&self) -> bool {
        self.output == OutputFormat::Text
    }

    // The project whose log this run writes and reads, when the log is a project's own. A run
    // naming a configuration keeps that configuration's log wherever it was typed, so it has none.
    // The one place that decides it: the entries of a project's log record their targets relative
    // to what this answers, and the history section compares them against the same.
    pub fn find_project_of_the_log(&self) -> Option<&LocalDir> {
        if self.config_name_to_save.is_some() || self.config_name_to_load.is_some() {
            return None;
        }

        self.local_dir.as_ref()
    }

    #[cfg(test)]
    pub fn set_should_show_faulty_files(&mut self, should_show_faulty_files: bool) -> &mut Self {
        self.should_show_faulty_files = should_show_faulty_files;
        self
    }

    #[cfg(test)]
    pub fn set_log_option(&mut self, log: LogOption) -> &mut Self {
        self.log = log;
        self
    }
}

impl Default for ViewConfig {
    fn default() -> Self {
        ViewConfig {
            version: VERSION_ID,
            targets_source: None,
            should_show_faulty_files: DEF_SHOW_FAULTY_FILES,
            should_show_skipped_files: false,
            hidden: Hidden::default(),
            log: LogOption::default(),
            compare_level: DEF_COMPARE_LEVEL,
            config_name_to_save: None,
            config_name_to_load: None,
            theme_name_to_save: None,
            local_dir: None,
            bar_thickness: BarThickness::default(),
            progress_bar: ProgressBarStyle::default(),
            layout: Layout::default(),
            output: OutputFormat::default(),
            explain: None,
            diff_against: None,
            number_separator: NumberSeparator::default(),
            decimal_separator: DecimalSeparator::default(),
            sort_by: SortCriterion::default(),
            top_n: None,
            by_file: None,
            counting: CountingModel::default(),
            theme: Theme::default()
        }
    }
}

// A hide list and not a show list: a show list would have to be written out again every time a
// section is added, and a configuration saved today would silently keep hiding the new one. Whole
// sections and parts of them are mixed on purpose, since the user points at what they see.
#[derive(Debug,PartialEq,Eq,Clone,Copy,Default)]
pub struct Hidden {
    pub version: bool,
    pub directory_info: bool,
    pub parsing_info: bool,
    pub progress_bar: bool,
    pub animations: bool,
    pub keywords: bool,
    pub nested_languages: bool,
    pub files: bool,
    pub comments: bool,
    pub extra: bool,
    pub blanks: bool,
    pub size: bool,
    pub percentages: bool,
    pub overview: bool,
    pub bar: bool,
    pub history: bool,
    pub timing: bool
}

impl Hidden {
    fn get_pairs(self) -> [(&'static str, bool); 17] {
        [("version", self.version), ("directory-info", self.directory_info), ("parsing-info", self.parsing_info),
         ("progress-bar", self.progress_bar), ("animations", self.animations), ("keywords", self.keywords),
         ("nested-languages", self.nested_languages), ("files", self.files), ("comments", self.comments),
         ("extra", self.extra), ("blanks", self.blanks), ("size", self.size), ("percentages", self.percentages),
         ("overview", self.overview), ("bar", self.bar), ("history", self.history), ("timing", self.timing)]
    }

    // Whether '--sort' was asked to order by a column this run does not draw
    pub fn hides_column_of(&self, criterion: SortCriterion) -> bool {
        match criterion {
            SortCriterion::Files => self.files,
            SortCriterion::Comments => self.comments,
            // Whichever word was asked for, 'build' has already left the answer in one flag
            SortCriterion::Extra | SortCriterion::Blanks => self.extra,
            SortCriterion::Size => self.size,
            // 'SortCriterion' is non_exhaustive, so a criterion added later orders by a column
            // this cannot know is hidden, rather than failing to compile in a released version
            _ => false
        }
    }

    // Returns the unrecognised name, so that the error can say which one it was
    pub fn parse(value: &str) -> Result<Hidden, String> {
        let mut hidden = Hidden::default();
        for entry in value.split([',', ' ', '\t']).map(str::trim).filter(|x| !x.is_empty()) {
            match entry.to_lowercase().as_str() {
                "version" => hidden.version = true,
                "directory-info" => hidden.directory_info = true,
                "parsing-info" => hidden.parsing_info = true,
                "progress-bar" => hidden.progress_bar = true,
                "animations" => hidden.animations = true,
                "keywords" => hidden.keywords = true,
                "nested-languages" => hidden.nested_languages = true,
                "files" => hidden.files = true,
                "comments" => hidden.comments = true,
                "extra" => hidden.extra = true,
                "blanks" => hidden.blanks = true,
                "size" => hidden.size = true,
                "percentages" => hidden.percentages = true,
                "overview" => hidden.overview = true,
                "bar" => hidden.bar = true,
                "history" => hidden.history = true,
                "timing" => hidden.timing = true,
                _ => return Err(entry.to_owned())
            }
        }

        Ok(hidden)
    }

    pub fn to_list_string(self) -> String {
        self.get_pairs().iter().filter(|(_,is_hidden)| *is_hidden).map(|(name,_)| *name).collect::<Vec<_>>().join(",")
    }

    pub fn get_names() -> Vec<&'static str> {
        Hidden::default().get_pairs().iter().map(|(name,_)| *name).collect()
    }
}

pub use mezura_core::SortCriterion;

// Only Slim is ASCII, so it is the one guaranteed to render on every terminal
#[derive(Debug,PartialEq,Eq,Clone,Copy,Default)]
pub enum BarThickness {
    Slim,
    #[default]
    Medium,
    Fat,
    Low
}

impl BarThickness {
    pub fn get_character(&self) ->&'static str {
        match self {
            Self::Slim => "|",
            Self::Medium => "┃",
            Self::Fat => "█",
            Self::Low => "▄"
        }
    }

    pub fn parse(value: &str) -> Option<BarThickness> {
        match value.trim().to_lowercase().as_str() {
            "slim" => Some(Self::Slim),
            "medium" => Some(Self::Medium),
            "fat" => Some(Self::Fat),
            "low" => Some(Self::Low),
            _ => None
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Slim => "slim",
            Self::Medium => "medium",
            Self::Fat => "fat",
            Self::Low => "low"
        }
    }
}

// Only Hash is ASCII, for a terminal that draws the block characters wrongly or not at all
#[derive(Debug,PartialEq,Eq,Clone,Copy,Default)]
pub enum ProgressBarStyle {
    #[default]
    Smooth,
    Blocky,
    Hash
}

impl ProgressBarStyle {
    // Each set runs from its faintest step to the one that fills a cell, and a cell is filled
    // through them in order. The gaps in 'blocky' come from the glyphs: a box is drawn narrower
    // than the cell it sits in.
    pub fn get_charset(&self) -> &'static str {
        match self {
            Self::Smooth => "▏▎▍▌▋▊▉█",
            Self::Blocky => "▪▮",
            Self::Hash => ".:#"
        }
    }

    pub fn parse(value: &str) -> Option<ProgressBarStyle> {
        match value.trim().to_lowercase().as_str() {
            "smooth" => Some(Self::Smooth),
            "blocky" => Some(Self::Blocky),
            "hash" => Some(Self::Hash),
            _ => None
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Smooth => "smooth",
            Self::Blocky => "blocky",
            Self::Hash => "hash"
        }
    }
}

#[derive(Debug,PartialEq,Eq,Clone,Copy,Default)]
pub enum Layout {
    List,
    #[default]
    Table,
    Boxed,
    Matrix
}

impl Layout {
    pub fn parse(value: &str) -> Option<Layout> {
        match value.trim().to_lowercase().as_str() {
            "list" => Some(Self::List),
            "table" => Some(Self::Table),
            "boxed" => Some(Self::Boxed),
            "matrix" => Some(Self::Matrix),
            _ => None
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Table => "table",
            Self::Boxed => "boxed",
            Self::Matrix => "matrix"
        }
    }
}

// Decides what is counted as well as what is shown: a run with none of this keeps no file rows
#[derive(Debug,PartialEq,Eq,Clone,Copy)]
pub enum ByFile {
    Capped(usize),
    All
}

impl ByFile {
    // Zero is the uncapped form, so a configuration file can carry this as a plain number
    pub fn parse(value: &str) -> Option<ByFile> {
        match super::args::parse_usize_value(value, 0, usize::MAX)? {
            0 => Some(Self::All),
            rows => Some(Self::Capped(rows))
        }
    }

    pub fn to_text(self) -> String {
        match self {
            Self::Capped(rows) => rows.to_string(),
            Self::All => String::from("0")
        }
    }

    pub fn shown_out_of(&self, files: usize) -> usize {
        match self {
            Self::Capped(rows) => files.min(*rows),
            Self::All => files
        }
    }
}

// Not a layout: a layout is the shape of a printed block, while this replaces the whole output,
// overview and status lines included. It is also the one display setting a configuration file may
// not carry, since a config that silently turns the output into JSON cannot be seen in the output.
#[derive(Debug,PartialEq,Eq,Clone,Copy,Default)]
pub enum OutputFormat {
    #[default]
    Text,
    Json
}

impl OutputFormat {
    pub fn parse(value: &str) -> Option<OutputFormat> {
        match value.trim().to_lowercase().as_str() {
            "text" => Some(Self::Text),
            "json" => Some(Self::Json),
            _ => None
        }
    }
}

// Which lines of the file '--explain' prints. The file is always read whole, since a comment that
// opened above the range decides every line in it; only the printing is narrowed.
#[derive(Debug,PartialEq,Eq,Clone,Copy)]
pub struct ExplainedLines {
    pub first: usize,
    pub last: usize
}

impl ExplainedLines {
    pub const WHOLE_FILE : ExplainedLines = ExplainedLines {first: 1, last: usize::MAX};

    pub fn holds(&self, line_number: usize) -> bool {
        line_number >= self.first && line_number <= self.last
    }

    pub fn is_the_whole_file(&self, lines_in_the_file: usize) -> bool {
        self.first <= 1 && self.last >= lines_in_the_file
    }
}

// The keyword rows list several figures side by side, so a grouping character that is also the
// list's own separator makes one long number out of two short ones
#[derive(Debug,PartialEq,Eq,Clone,Copy,Default)]
pub enum NumberSeparator {
    #[default]
    Comma,
    Underscore,
    Dot,
    None
}

impl NumberSeparator {
    pub fn get_character(&self) ->Option<char> {
        match self {
            Self::Comma => Some(','),
            Self::Underscore => Some('_'),
            Self::Dot => Some('.'),
            Self::None => None
        }
    }

    pub fn parse(value: &str) -> Option<NumberSeparator> {
        match value.trim().to_lowercase().as_str() {
            "comma" | "," => Some(Self::Comma),
            "underscore" | "_" => Some(Self::Underscore),
            "dot" | "." => Some(Self::Dot),
            "none" => Some(Self::None),
            _ => None
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Comma => "comma",
            Self::Underscore => "underscore",
            Self::Dot => "dot",
            Self::None => "none"
        }
    }
}

// Free to combine with any grouping character, including the same one. Both '1.559.486 / 365.2' and
// '1,559,486 / 365,2' are what some readers expect, so neither is refused.
#[derive(Debug,PartialEq,Eq,Clone,Copy,Default)]
pub enum DecimalSeparator {
    #[default]
    Dot,
    Comma
}

impl DecimalSeparator {
    pub fn get_character(&self) ->char {
        match self {
            Self::Dot => '.',
            Self::Comma => ','
        }
    }

    pub fn parse(value: &str) -> Option<DecimalSeparator> {
        match value.trim().to_lowercase().as_str() {
            "dot" | "." => Some(Self::Dot),
            "comma" | "," => Some(Self::Comma),
            _ => None
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Dot => "dot",
            Self::Comma => "comma"
        }
    }
}

#[derive(Debug,PartialEq,Clone,Default)]
pub struct LogOption {
    pub should_log: bool,
    pub name: Option<String>
}

impl LogOption {
    pub fn new(log_name: Option<String>) -> Self {
        LogOption {
            should_log: true,
            name: log_name,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum ArgParsingError {
    UnparsableWorkingDir,
    InvalidPath(String),
    // The failure as it would have been reported for a typed target, plus the configuration that
    // named it. Wrapped rather than replaced by one sentence of its own: a pattern that matched only
    // ignored files and a path that is not there are two different things to go and do.
    InvalidTargetInConfig(Box<ArgParsingError>,String),
    DoublePath,
    RepeatedCommand(String),
    UnrecognisedCommand(String),
    // The command, and what was written after it, empty when nothing was
    IncorrectCommandArgs(String, String),
    UnexpectedCommandArgs(String),
    NonExistantConfig(String),
    UnreadableConfig(String, usize, super::config_files::UnreadableCause),
    NonExistantTheme(String),
    InvalidStyle(String),
    InvalidHideTarget(String),
    InvalidValueInConfig(String,String),
    InvalidGlobPattern(String),
    NoGlobMatches(String),
    AllGlobMatchesIgnored(String),
    MalformedTarget(String),
    ContestedTarget(String, String, String),
    MeaninglessWithExplain(String),
    ContradictoryCommands(String, String)
}

impl Formatted for ArgParsingError {
    fn format(&self) -> ColoredString {
        match self {
            Self::UnparsableWorkingDir => wrap_message("The current working directory could not be read, so there is no target to count. Write the path out: mezura ./some/path").red(),
            Self::InvalidPath(p) => wrap_message(&format!("'{p}' does not exist as a directory or file.")).red(),
            Self::InvalidTargetInConfig(inner,name) => {
                let attribution = wrap_message(&format!("That target was named in config '{name}'.")).red();
                ColoredString::from(format!("{}\n{attribution}", inner.format()).as_str())
            },
            Self::DoublePath => wrap_message("Targets already provided as first argument, but --targets command also found.").red(),
            Self::RepeatedCommand(c) => wrap_message(&format!("'--{c}' appears more than once in the \
command line. Give it once; a command that takes several values takes them together, like '--hide \
overview,keywords'.")).red(),
            Self::UnrecognisedCommand(p) => {
                let tail = suggestions::format_suggestion(p, &message_printer::get_command_names())
                        .unwrap_or_else(|| format!("Run '--{HELP}' to see every command."));
                let error = format!("--{p} is not recognised as a command.").red();
                ColoredString::from(format!("{error}\n\n{tail}").as_str())
            },
            Self::IncorrectCommandArgs(command, given) if given.is_empty() =>
                    wrap_message(&format!("'--{command}' needs a value.")).red(),
            Self::IncorrectCommandArgs(command, given) =>
                    wrap_message(&format!("'--{command}' cannot take '{given}'.")).red(),
            Self::UnreadableConfig(name, line, super::config_files::UnreadableCause::NotUtf8) => wrap_message(&format!("Configuration '{name}' stops being readable at line {line}, so none of it was used: the file is not saved as UTF-8.")).red(),
            Self::UnreadableConfig(name, line, super::config_files::UnreadableCause::Io(error)) => wrap_message(&format!("Configuration '{name}' could not be read past line {line}, so none of it was used: {error}")).red(),
            Self::UnexpectedCommandArgs(p) => wrap_message(&format!("Command '--{p}' does not expect any arguments.")).red(),
            Self::NonExistantConfig(p) => {
                let names = super::config_files::read_names_in_dir(&crate::paths::PERSISTENT_APP_PATHS.config_dir);
                let tail = suggestions::format_suggestion(p, &names.iter().map(String::as_str).collect::<Vec<_>>())
                        .unwrap_or_else(|| format!("Run '--{SHOW_CONFIGS}' to see the ones you have."));
                let error = format!("Configuration '{p}' does not exist.").red();
                ColoredString::from(format!("{error}\n\n{tail}").as_str())
            },
            Self::NonExistantTheme(p) => {
                let names = super::config_files::read_names_in_dir(&crate::paths::PERSISTENT_APP_PATHS.themes_dir);
                let tail = suggestions::format_suggestion(p, &names.iter().map(String::as_str).collect::<Vec<_>>())
                        .unwrap_or_else(|| format!("Run '--{SHOW_THEMES}' to see the ones you have."));
                let error = format!("Theme '{p}' was not found, or could not be read.").red();
                ColoredString::from(format!("{error}\n\n{tail}").as_str())
            },
            Self::InvalidStyle(p) => wrap_message(p).red(),
            Self::InvalidHideTarget(p) => {
                let names = Hidden::get_names();
                let tail = suggestions::format_suggestion(p, &names)
                        .unwrap_or_else(|| format!("The options are: {}.", names.join(", ")));
                let error = format!("'{p}' is not something that can be hidden.").red();
                ColoredString::from(wrap_message(&format!("{error}\n\n{tail}")).to_string().as_str())
            },
            Self::InvalidValueInConfig(cmd,conf) => wrap_message(&format!("Invalid value for the command '--{cmd}', in config '{conf}'.\nFix the value in the config file, or override it by providing a valid '--{cmd}' argument.")).red(),
            Self::InvalidGlobPattern(p) => wrap_message(&format!("'{p}' is not a valid glob pattern.")).red(),
            Self::NoGlobMatches(p) => wrap_message(&format!("The pattern '{p}' did not match any existing directory or file.")).red(),
            Self::AllGlobMatchesIgnored(p) => wrap_message(&format!("Everything that the pattern '{p}' matched is skipped, because a .gitignore file ignores it, because it is a dotted path, or because it is a link.\nUse the '--no-gitignore' or '--search-in-dotted' commands to include it, or provide the paths explicitly.")).red(),
            Self::MalformedTarget(p) => wrap_message(&format!("'{p}' names a module with no path after it.\nA target is written as '<module>=<path>', and its paths are separated by commas: 'tests=./api/tests,./web/tests'.")).red(),
            Self::ContestedTarget(path, first, second) => wrap_message(&format!("'{path}' is declared both as '{first}' and as '{second}'.\nEvery file belongs to exactly one module, and there is no more specific of the two to decide it.")).red(),
            Self::MeaninglessWithExplain(command) => wrap_message(&format!("'--{EXPLAIN}' explains one file line by line and '--{command}' belongs to a report over a whole scan, so the two cannot be asked for together.")).red(),
            Self::ContradictoryCommands(one, other) => wrap_message(&format!("'--{one}' and '--{other}' ask for opposite things, so they cannot be given together.")).red()
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TypedExplicitlyOnCommandLine {
    pub exclude: bool,
    pub languages: bool,
    pub excluded_languages: bool,
    pub forced_languages: bool,
    pub counting: bool,
    pub search_in_dotted: bool,
    pub count_minified: bool,
    pub count_generated: bool,
    pub count_not_code: bool,
    pub no_gitignore: bool,
    pub no_ignore_files: bool,
    pub no_heuristics: bool,
    pub hide_keywords: bool
}

impl TypedExplicitlyOnCommandLine {
    // Exhaustive on purpose: a new field of the builder has to be decided here, in or out, before
    // this compiles again.
    fn of(builder: &ConfigurationBuilder) -> Self {
        let ConfigurationBuilder { exclude_dirs, languages_of_interest, excluded_languages,
            forced_languages, counting, should_search_in_dotted, count_minified, count_generated,
            count_not_code, no_gitignore, no_ignore_files, no_heuristics, hidden,
            targets: _, targets_source: _, threads: _, should_show_faulty_files: _,
            should_show_skipped_files: _, theme_name: _,
            log: _, compare_level: _, config_name_to_save: _, config_name_to_load: _,
            theme_name_to_save: _, local_dir: _, bar_thickness: _, progress_bar: _, number_separator: _, decimal_separator: _,
            layout: _, output: _, explain: _, diff_against: _, sort_by: _, top_n: _, by_file: _, styles: _,
            config_styles: _, theme_styles: _, typed_explicitly: _ } = builder;

        TypedExplicitlyOnCommandLine {
            exclude: exclude_dirs.is_some(),
            languages: languages_of_interest.is_some(),
            excluded_languages: excluded_languages.is_some(),
            forced_languages: forced_languages.is_some(),
            counting: counting.is_some(),
            search_in_dotted: should_search_in_dotted.is_some(),
            count_minified: count_minified.is_some(),
            count_generated: count_generated.is_some(),
            count_not_code: count_not_code.is_some(),
            no_gitignore: no_gitignore.is_some(),
            no_ignore_files: no_ignore_files.is_some(),
            no_heuristics: no_heuristics.is_some(),
            hide_keywords: hidden.as_ref().is_some_and(|x| x.keywords)
        }
    }
}

// One optional field per command, flat like the command line and the configuration file that fill
// it, and merged from both before 'build' turns it into the two halves the program runs on.
#[derive(Debug, PartialEq, Default)]
pub struct ConfigurationBuilder {
    pub targets:                  Option<Vec<Target>>,
    // Which configuration file supplied the targets, when one did: the run resolves them, and its
    // error has to name the file the reader cannot see failing. Absent from 'add_missing_fields',
    // being bookkeeping about the merge and not a merged value.
    pub targets_source:           Option<String>,
    pub exclude_dirs:             Option<Vec<String>>,
    pub languages_of_interest:    Option<Vec<String>>,
    pub excluded_languages:       Option<Vec<String>>,
    pub forced_languages:         Option<ForcedLanguages>,
    pub threads:                  Option<Threads>,
    pub counting:                 Option<CountingModel>,
    pub should_search_in_dotted:  Option<bool>,
    pub count_minified:           Option<bool>,
    pub count_generated:          Option<bool>,
    pub count_not_code:           Option<bool>,
    pub should_show_faulty_files: Option<bool>,
    pub should_show_skipped_files: Option<bool>,
    pub hidden:                   Option<Hidden>,
    pub no_gitignore:             Option<bool>,
    pub no_ignore_files:          Option<bool>,
    pub no_heuristics:            Option<bool>,
    pub theme_name:               Option<String>,
    // Only the command line switches it on. A configuration that carried its own log would write an
    // entry on every run that loads it, so it stays a per-run request and is absent from
    // 'add_missing_fields', 'has_missing_fields' and the file parser.
    pub log:                      Option<LogOption>,
    pub compare_level:            Option<usize>,
    pub config_name_to_save:      Option<String>,
    pub config_name_to_load:      Option<String>,
    pub theme_name_to_save:       Option<String>,
    // Found by looking around the targets rather than filled by any command, so it is absent from
    // 'add_missing_fields' and 'has_missing_fields' along with the two names above it
    pub local_dir:                Option<LocalDir>,
    pub bar_thickness:            Option<BarThickness>,
    pub progress_bar:             Option<ProgressBarStyle>,
    pub number_separator:         Option<NumberSeparator>,
    pub decimal_separator:        Option<DecimalSeparator>,
    pub layout:                   Option<Layout>,
    // Absent from 'add_missing_fields' and 'has_missing_fields' on purpose, like the save and load
    // names: those two functions exist for what a configuration file can supply, and this is not it
    pub output:                   Option<OutputFormat>,
    // Absent from the same two for the same reason: a per-run diagnostic, never a saved setting
    pub explain:                  Option<ExplainedLines>,
    // Absent from those same two, and for the same reason: a configuration that silently turned
    // every run into a comparison against a file saved months ago is not a setting anybody wants
    pub diff_against:             Option<String>,
    pub sort_by:                  Option<SortCriterion>,
    pub top_n:                    Option<usize>,
    pub by_file:                  Option<ByFile>,
    pub styles:                   Option<Vec<(String,String)>>,
    pub config_styles:            Option<Vec<(String,String)>>,
    pub theme_styles:             Option<Vec<(String,String)>>,
    // Not an Option: it is a fact about the command line, not a value a file can supply
    pub typed_explicitly:         TypedExplicitlyOnCommandLine
}

impl ConfigurationBuilder {
    pub fn add_missing_fields(&mut self, config: Self) -> &mut Self {
        if self.targets.is_none() {self.targets = config.targets};
        if self.exclude_dirs.is_none() {self.exclude_dirs = config.exclude_dirs};
        if self.languages_of_interest.is_none() {self.languages_of_interest = config.languages_of_interest};
        if self.excluded_languages.is_none() {self.excluded_languages = config.excluded_languages};
        if self.forced_languages.is_none() {self.forced_languages = config.forced_languages};
        if self.threads.is_none() {self.threads = config.threads};
        if self.counting.is_none() {self.counting = config.counting};
        if self.should_search_in_dotted.is_none() {self.should_search_in_dotted = config.should_search_in_dotted};
        if self.count_minified.is_none() {self.count_minified = config.count_minified};
        if self.count_generated.is_none() {self.count_generated = config.count_generated};
        if self.count_not_code.is_none() {self.count_not_code = config.count_not_code};
        if self.should_show_faulty_files.is_none() {self.should_show_faulty_files = config.should_show_faulty_files};
        if self.should_show_skipped_files.is_none() {self.should_show_skipped_files = config.should_show_skipped_files};
        if self.hidden.is_none() {self.hidden = config.hidden};
        if self.no_gitignore.is_none() {self.no_gitignore = config.no_gitignore};
        if self.no_ignore_files.is_none() {self.no_ignore_files = config.no_ignore_files};
        if self.no_heuristics.is_none() {self.no_heuristics = config.no_heuristics};
        if self.theme_name.is_none() {self.theme_name = config.theme_name};
        if self.compare_level.is_none() {self.compare_level = config.compare_level};
        if self.config_styles.is_none() {self.config_styles = config.config_styles};
        if self.bar_thickness.is_none() {self.bar_thickness = config.bar_thickness};
        if self.progress_bar.is_none() {self.progress_bar = config.progress_bar};
        if self.number_separator.is_none() {self.number_separator = config.number_separator};
        if self.decimal_separator.is_none() {self.decimal_separator = config.decimal_separator};
        if self.layout.is_none() {self.layout = config.layout};
        if self.sort_by.is_none() {self.sort_by = config.sort_by};
        if self.top_n.is_none() {self.top_n = config.top_n};
        if self.by_file.is_none() {self.by_file = config.by_file};
        self
    }

    // What is left of a configuration once everything that decides a number is taken out of it
    pub fn forget_what_changes_the_numbers(mut self) -> Self {
        // Exhaustive on purpose: a new field of the builder has to be decided here, in or out,
        // before this compiles again.
        let ConfigurationBuilder { targets, exclude_dirs, languages_of_interest, excluded_languages,
            forced_languages, counting, should_search_in_dotted, count_minified, count_generated,
            count_not_code, no_gitignore, no_ignore_files, no_heuristics,
            threads: _, should_show_faulty_files: _, should_show_skipped_files: _, hidden: _,
            theme_name: _, compare_level: _,
            bar_thickness: _, progress_bar: _, number_separator: _, decimal_separator: _, layout: _,
            sort_by: _, top_n: _, by_file: _, config_styles: _,
            // never carried by a configuration file, so never merged out of one either
            targets_source: _, log: _, config_name_to_save: _, config_name_to_load: _,
            theme_name_to_save: _, local_dir: _, output: _, explain: _, diff_against: _,
            styles: _, theme_styles: _, typed_explicitly: _ } = &mut self;

        *targets = None;
        *exclude_dirs = None;
        *languages_of_interest = None;
        *excluded_languages = None;
        *forced_languages = None;
        *counting = None;
        *should_search_in_dotted = None;
        *count_minified = None;
        *count_generated = None;
        *count_not_code = None;
        *no_gitignore = None;
        *no_ignore_files = None;
        *no_heuristics = None;

        self
    }

    // Every field 'add_missing_fields' merges has to be asked about here, or the answer is "nothing
    // to fill in" while that one field is still empty, the default configuration is never read at
    // all, and the value it holds for that field is dropped without a word.
    pub fn has_missing_fields(&self) -> bool {
        self.targets.is_none() || self.exclude_dirs.is_none() || self.languages_of_interest.is_none() ||
        self.excluded_languages.is_none() || self.forced_languages.is_none() ||
        self.threads.is_none() || self.counting.is_none() || self.should_search_in_dotted.is_none() ||
        self.count_minified.is_none() || self.count_generated.is_none() ||
        self.count_not_code.is_none() || self.should_show_faulty_files.is_none() ||
        self.should_show_skipped_files.is_none() || self.hidden.is_none() || self.no_gitignore.is_none() ||
        self.no_ignore_files.is_none() || self.no_heuristics.is_none() ||
        self.theme_name.is_none() || self.compare_level.is_none() ||
        self.config_styles.is_none() || self.bar_thickness.is_none() || self.progress_bar.is_none() ||
        self.number_separator.is_none() || self.decimal_separator.is_none() || self.layout.is_none() ||
        self.sort_by.is_none() || self.top_n.is_none() || self.by_file.is_none()
    }

    // Names the model this run counts with and the word that quantity has there, so that nobody
    // hunts for a column that was never going to be drawn.
    fn report_a_word_of_the_other_model(command: &str, counting: CountingModel, result: &str) {
        let message = format!("'--{command} {}' names the third column of the other way of counting. \
This run counts by {}, where that column is '{}', so {result}.",
                counting.get_other().get_third_quantity_name(), counting.name(),
                counting.get_third_quantity_name());
        eprintln!("\n{}", wrap_message(&message).yellow());
        super::warning_collector::keep(mezura_core::warnings::Warning::new(
                mezura_core::warnings::Code::CommandIgnored, command, message));
    }

    // The only place that knows the flat form maps onto two halves. Everything above this stays one
    // list, matching the command line and the configuration file.
    pub fn build(&self) -> Configuration {
        let counting = self.counting.unwrap_or_default();
        let mut hidden = self.hidden.unwrap_or_default();
        let mut sort_by = self.sort_by.unwrap_or_default();
        // The third quantity is called 'extra' where a line is measured by what it says and
        // 'blanks' where it is measured by where it sits, so the other model's word names nothing
        // this run draws and is dropped. From here down 'hidden.extra' is the one flag that says
        // the column is out.
        let (mine, of_the_other_model) = match counting {
            CountingModel::Content => (hidden.extra, hidden.blanks),
            CountingModel::Region => (hidden.blanks, hidden.extra)
        };
        hidden.extra = mine;
        hidden.blanks = false;
        if of_the_other_model {
            Self::report_a_word_of_the_other_model(HIDE, counting, "nothing was hidden");
        }
        if sort_by == SortCriterion::Extra && counting == CountingModel::Region
                || sort_by == SortCriterion::Blanks && counting == CountingModel::Content {
            Self::report_a_word_of_the_other_model(SORT, counting, "the report is sorted by lines");
            sort_by = SortCriterion::default();
        }
        // Decided here, after a configuration file has had its say on both halves. A JSON document
        // carries every figure whatever is hidden, so there the order stands as asked.
        if hidden.hides_column_of(sort_by) && self.output.unwrap_or_default() == OutputFormat::Text {
            let message = format!("'--{SORT} {}' orders by a column '--{HIDE} {0}' takes out, so the report \
is sorted by lines.", sort_by.name());
            eprintln!("\n{}", wrap_message(&message).yellow());
            super::warning_collector::keep(mezura_core::warnings::Warning::new(
                    mezura_core::warnings::Code::CommandIgnored, SORT, message));
            sort_by = SortCriterion::default();
        }
        // Asked of the engine rather than kept as constants here, so that the help text and the
        // behaviour cannot answer differently
        let engine_defaults = EngineConfig::default();

        Configuration {
            typed_explicitly: self.typed_explicitly,
            engine: EngineConfig {
                targets: self.targets.clone().unwrap_or_default(),
                exclude_dirs: self.exclude_dirs.clone().unwrap_or_default(),
                languages_of_interest: LanguageNames::of_written_form(
                        &self.languages_of_interest.clone().unwrap_or_default()),
                excluded_languages: LanguageNames::of_written_form(
                        &self.excluded_languages.clone().unwrap_or_default()),
                forced_languages: self.forced_languages.clone().unwrap_or_default(),
                threads: self.threads.unwrap_or_default(),
                should_search_in_dotted: self.should_search_in_dotted.unwrap_or(engine_defaults.should_search_in_dotted),
                count_minified: self.count_minified.unwrap_or(engine_defaults.count_minified),
                count_generated: self.count_generated.unwrap_or(engine_defaults.count_generated),
                count_not_code: self.count_not_code.unwrap_or(engine_defaults.count_not_code),
                no_gitignore: self.no_gitignore.unwrap_or(engine_defaults.no_gitignore),
                no_ignore_files: self.no_ignore_files.unwrap_or(engine_defaults.no_ignore_files),
                use_heuristics: !self.no_heuristics.unwrap_or(!engine_defaults.use_heuristics),
                // The two flags that answer both questions: what is counted and what is shown
                count_keywords: !hidden.keywords,
                collect_files: self.by_file.is_some()
            },
            view: ViewConfig {
                version: VERSION_ID,
                targets_source: self.targets_source.clone(),
                should_show_faulty_files: self.should_show_faulty_files.unwrap_or(DEF_SHOW_FAULTY_FILES),
                should_show_skipped_files: self.should_show_skipped_files.unwrap_or(false),
                hidden,
                log: self.log.clone().unwrap_or_default(),
                compare_level: self.compare_level.unwrap_or(DEF_COMPARE_LEVEL),
                config_name_to_save: self.config_name_to_save.clone(),
                config_name_to_load: self.config_name_to_load.clone(),
                theme_name_to_save: self.theme_name_to_save.clone(),
                local_dir: self.local_dir.clone(),
                bar_thickness: self.bar_thickness.unwrap_or_default(),
                progress_bar: self.progress_bar.unwrap_or_default(),
                layout: self.layout.unwrap_or_default(),
                output: self.output.unwrap_or_default(),
                explain: self.explain,
                diff_against: self.diff_against.clone(),
                number_separator: self.number_separator.unwrap_or_default(),
                decimal_separator: self.decimal_separator.unwrap_or_default(),
                sort_by,
                top_n: self.top_n,
                by_file: self.by_file,
                counting: self.counting.unwrap_or_default(),
                theme: super::theme::resolve(self.theme_styles.as_deref().unwrap_or_default(),
                        self.config_styles.as_deref().unwrap_or_default(), self.styles.as_deref().unwrap_or_default())
            }
        }
    }
}

// An empty line is a run that typed nothing at all, which names no target and no command.
pub fn create_config_from_args(line: &str) -> Result<Configuration, ArgParsingError> {
    let config = create_config_builder_from_args(line)?.build();

    // Written from the resolved theme and therefore after it is built, which is also why this does
    // not sit next to '--save': what the file has to hold is the look, not the pieces it came from
    if let Some(name) = &config.view.theme_name_to_save {
        if config.view.theme == Theme::default() {
            eprintln!("\n{}", wrap_message(&format!("Nothing to save in theme '{name}': every style is at its default.")).yellow());
        } else {
            match super::theme_files::save_theme_to_file(&crate::paths::PERSISTENT_APP_PATHS.themes_dir, name, &config.view.theme) {
                Err(error) => eprintln!("\n{}", wrap_message(&format!(
                        "Theme '{name}' could not be written to '{}': {error}",
                        crate::paths::PERSISTENT_APP_PATHS.themes_dir)).yellow()),
                Ok(_) => eprintln!("\nTheme '{name}' saved. Apply it with '--{THEME} {name}'.")
            }
        }
    }

    Ok(config)
}

// The form that reads back as this exact target. The quotes go around the path and not around the
// whole thing, because the name is taken from before the first '=' and a leading quote would end up
// inside it.
pub fn format_declared_form(target: &Target) -> String {
    let path = if target.path.contains(char::is_whitespace) {format!("\"{}\"", target.path)} else {target.path.clone()};
    match &target.module {
        Some(name) => format!("{name}={path}"),
        None => path
    }
}

// The same form, written the way a project's own configuration reads it back
pub fn format_declared_form_relative_to(project_dir: &str, target: &Target) -> String {
    format_declared_form(&Target { module: target.module.clone(),
            path: format_path_inside(project_dir, &target.path) })
}

// How a project spells a place inside it: relative to the project, which is what a configuration
// found there joins its own relative paths to. Written that way by everything a project keeps, its
// configuration and its log alike, so a folder shared with the code says the same thing on every
// disk it is cloned to. A path outside the project has no such spelling and is left as it stands.
pub fn format_path_inside(project_dir: &str, path: &str) -> String {
    match std::path::Path::new(path).strip_prefix(project_dir) {
        Ok(inside) => format!("./{}", crate::paths::normalise_separators(&inside.to_string_lossy())),
        Err(_) => path.to_owned()
    }
}

// The run refused the declared targets. A configuration file that supplied them is named as the
// culprit: otherwise a 'targets' block nobody can see failing sends the reader hunting through the
// command they typed.
pub fn attribute_targets_error(error: mezura_core::TargetError, targets_source: &Option<String>) -> ArgParsingError {
    match (map_target_error(error), targets_source) {
        (inner @ (ArgParsingError::InvalidPath(_) | ArgParsingError::InvalidGlobPattern(_)
                | ArgParsingError::NoGlobMatches(_) | ArgParsingError::AllGlobMatchesIgnored(_)), Some(name)) =>
                ArgParsingError::InvalidTargetInConfig(Box::new(inner), name.clone()),
        (other, _) => other
    }
}

pub fn create_config_builder_from_args(line: &str) -> Result<ConfigurationBuilder, ArgParsingError> {
    let mut config_builder = ConfigurationBuilder::default();
    let mut options = super::args::split_into_command_segments(line).into_iter();

    // The first segment holds the targets when they were written without '--targets'. It is empty
    // for a line that opens with a command, and for a line that is empty because nothing at all was
    // typed, and both of those leave the targets for a configuration file to name.
    if line.trim().starts_with("--") {
        options.next();
    } else {
        let parsed = parse_targets(options.next().unwrap_or_default())?;
        if !parsed.is_empty() {
            config_builder.targets = Some(parsed);
        }
    }

    let mut custom_config = None;
    let (mut save_local, mut no_local) = (false, false);
    let mut seen_commands = HashSet::new();
    for command in options {
        let (command_name, arguments) = match command.find(" ") {
            Some(index) => command.split_at(index),
            None => (command.trim(), "")
        };
        if !seen_commands.insert(command_name.to_owned()) {
            return Err(ArgParsingError::RepeatedCommand(command_name.to_owned()));
        }
        match command_name {
            TARGETS => {
                if config_builder.targets.is_some() {
                    return Err(ArgParsingError::DoublePath);
                }

                let parsed = parse_targets(arguments)?;
                if parsed.is_empty() {
                    message_printer::print_help_message_for_command(TARGETS);
                    return Err(refuse_argument(arguments, TARGETS.to_owned()));
                }
                config_builder.targets = Some(parsed)
            },
            EXCLUDE => config_builder.exclude_dirs = Some(parse_or_refuse(EXCLUDE, arguments,
                    |x| Some(super::args::parse_paths_to_vec(x)).filter(|vec| !vec.is_empty()
                            && mezura_core::engine::targets::validate_exclude_patterns(vec).is_ok()))?),
            LANGUAGES => config_builder.languages_of_interest = Some(parse_or_refuse(LANGUAGES, arguments,
                    |x| Some(super::args::parse_languages_to_vec(x)).filter(|vec| !vec.is_empty()))?),
            EXCLUDE_LANGUAGES => config_builder.excluded_languages = Some(parse_or_refuse(EXCLUDE_LANGUAGES, arguments,
                    |x| Some(super::args::parse_languages_to_vec(x)).filter(|vec| !vec.is_empty()))?),
            FORCE_LANGUAGE => config_builder.forced_languages =
                    Some(parse_or_refuse(FORCE_LANGUAGE, arguments, super::args::parse_forced_languages)?),
            THREADS => config_builder.threads = Some(parse_or_refuse(THREADS, arguments,
                    |x| super::args::parse_two_usize_values(x, MIN_PRODUCERS_VALUE, MAX_PRODUCERS_VALUE,
                            MIN_CONSUMERS_VALUE, MAX_CONSUMERS_VALUE).map(Threads::from))?),
            COUNTING => config_builder.counting = Some(parse_or_refuse(COUNTING, arguments, CountingModel::parse)?),
            SEARCH_IN_DOTTED => config_builder.should_search_in_dotted = Some(take_flag(command, SEARCH_IN_DOTTED)?),
            COUNT_MINIFIED => config_builder.count_minified = Some(take_flag(command, COUNT_MINIFIED)?),
            COUNT_GENERATED => config_builder.count_generated = Some(take_flag(command, COUNT_GENERATED)?),
            COUNT_NOT_CODE => config_builder.count_not_code = Some(take_flag(command, COUNT_NOT_CODE)?),
            SHOW_FAULTY_FILES => config_builder.should_show_faulty_files = Some(take_flag(command, SHOW_FAULTY_FILES)?),
            SHOW_SKIPPED => config_builder.should_show_skipped_files = Some(take_flag(command, SHOW_SKIPPED)?),
            HIDE => {
                if arguments.trim().is_empty() {
                    message_printer::print_help_message_for_command(HIDE);
                    return Err(refuse_argument(arguments, HIDE.to_owned()))
                }
                match Hidden::parse(arguments) {
                    Ok(x) => config_builder.hidden = Some(x),
                    Err(x) => {
                        message_printer::print_help_message_for_command(HIDE);
                        return Err(ArgParsingError::InvalidHideTarget(x))
                    }
                }
            },
            NO_GITIGNORE => config_builder.no_gitignore = Some(take_flag(command, NO_GITIGNORE)?),
            NO_IGNORE_FILES => config_builder.no_ignore_files = Some(take_flag(command, NO_IGNORE_FILES)?),
            NO_HEURISTICS => config_builder.no_heuristics = Some(take_flag(command, NO_HEURISTICS)?),
            THEME => {
                let name = take_name(THEME, arguments)?;
                if super::theme_files::load_theme(&name, &crate::paths::PERSISTENT_APP_PATHS.themes_dir).is_none() {
                    return Err(ArgParsingError::NonExistantTheme(name))
                }
                config_builder.theme_name = Some(name);
            },
            STYLE => match super::theme::parse_overrides(arguments) {
                Ok(x) => config_builder.styles = Some(x),
                Err(x) => {
                    message_printer::print_help_message_for_command(STYLE);
                    return Err(ArgParsingError::InvalidStyle(x.format()))
                }
            },
            TOP => config_builder.top_n = Some(parse_or_refuse(TOP, arguments,
                    |x| super::args::parse_usize_value(x, 1, usize::MAX))?),
            // The only command whose argument is optional. Without one it hides nothing, the way
            // '--top' shows every language until a number says otherwise.
            BY_FILE => config_builder.by_file = Some(if arguments.trim().is_empty() {ByFile::All}
                    else {parse_or_refuse(BY_FILE, arguments, ByFile::parse)?}),
            SORT => config_builder.sort_by = Some(parse_or_refuse(SORT, arguments, SortCriterion::parse)?),
            BAR_THICKNESS => config_builder.bar_thickness =
                    Some(parse_or_refuse(BAR_THICKNESS, arguments, BarThickness::parse)?),
            PROGRESS_BAR => config_builder.progress_bar =
                    Some(parse_or_refuse(PROGRESS_BAR, arguments, ProgressBarStyle::parse)?),
            LAYOUT => config_builder.layout = Some(parse_or_refuse(LAYOUT, arguments, Layout::parse)?),
            OUTPUT => config_builder.output = Some(parse_or_refuse(OUTPUT, arguments, OutputFormat::parse)?),
            EXPLAIN => config_builder.explain =
                    Some(parse_or_refuse(EXPLAIN, arguments, crate::args::parse_explained_lines)?),
            DIFF => config_builder.diff_against = Some(take_name(DIFF, arguments)?),
            NUMBER_SEPARATOR => config_builder.number_separator =
                    Some(parse_or_refuse(NUMBER_SEPARATOR, arguments, NumberSeparator::parse)?),
            DECIMAL_SEPARATOR => config_builder.decimal_separator =
                    Some(parse_or_refuse(DECIMAL_SEPARATOR, arguments, DecimalSeparator::parse)?),
            LOG => config_builder.log = Some(LogOption::new(super::args::get_trimmed_if_not_empty(arguments))),
            COMPARE_LEVEL => config_builder.compare_level = Some(parse_or_refuse(COMPARE_LEVEL, arguments,
                    |x| super::args::parse_usize_value(x, MIN_COMPARE_LEVEL, MAX_COMPARE_LEVEL))?),
            LOAD => {
                let config_name = take_name(LOAD, arguments)?;
                match super::config_files::parse_config_file(Some(&config_name), None) {
                    Ok((options, issues)) => {
                        custom_config = Some((options, issues));
                        config_builder.config_name_to_load = Some(config_name);
                    },
                    // The file is there, it just cannot be read whole; calling it missing sends the
                    // user looking for a typo in the name instead of at the file's encoding
                    Err(super::config_files::ConfigFileParseError::UnreadableLine(file, line, cause)) =>
                        return Err(ArgParsingError::UnreadableConfig(file, line, cause)),
                    Err(_) => return Err(ArgParsingError::NonExistantConfig(config_name))
                }
            },
            SAVE => config_builder.config_name_to_save = Some(take_name(SAVE, arguments)?),
            SAVE_THEME => config_builder.theme_name_to_save = Some(take_name(SAVE_THEME, arguments)?),
            SAVE_LOCAL => save_local = take_flag(command, SAVE_LOCAL)?,
            NO_LOCAL => no_local = take_flag(command, NO_LOCAL)?,
            _ => return Err(ArgParsingError::UnrecognisedCommand(command_name.to_owned()))
        }
    }

    if save_local && no_local {
        return Err(ArgParsingError::ContradictoryCommands(SAVE_LOCAL.to_owned(), NO_LOCAL.to_owned()));
    }

    // Looked for around the targets as they were typed, and never around ones a configuration
    // supplies: the folder belongs to the code this command names, and what its own settings then
    // say about targets is decided after it has been found.
    let typed_paths = config_builder.targets.iter().flatten().map(|target| target.path.clone()).collect::<Vec<_>>();
    config_builder.local_dir = if no_local {None} else {crate::paths::find_local_dir(&typed_paths)};

    // '--save-local' counts as a folder of its own, since a run that writes one has somewhere to
    // keep a log by the time it would be written
    print_warnings_for_commands_that_need_a_loaded_configuration(&config_builder,
            config_builder.local_dir.is_some() || save_local);

    // Before the configuration files below fill anything in, which is what makes the answer the
    // command line's own
    config_builder.typed_explicitly = TypedExplicitlyOnCommandLine::of(&config_builder);

    // Checked here, while every field is still the command line's own: a value one of the files
    // below merges in must not kill an explain run, only a command actually typed beside it.
    if config_builder.explain.is_some() {
        let typed_beside_it = [(DIFF, config_builder.diff_against.is_some()),
                (LOG, config_builder.log.is_some()),
                (COMPARE_LEVEL, config_builder.compare_level.is_some()),
                (SORT, config_builder.sort_by.is_some()),
                (TOP, config_builder.top_n.is_some()),
                (BY_FILE, config_builder.by_file.is_some())];
        if let Some((command, _)) = typed_beside_it.iter().find(|(_, typed)| *typed) {
            return Err(ArgParsingError::MeaninglessWithExplain((*command).to_owned()));
        }
    }

    let mut targets_config_source = None;
    if let Some((custom, issues)) = custom_config {
        let config_name = config_builder.config_name_to_load.clone().unwrap_or_default();
        print_config_file_warnings(&issues.warnings, &config_name);
        resolve_invalid_config_fields(&config_builder, &issues.invalid_fields, &config_name)?;
        let targets_were_missing = config_builder.targets.is_none();
        config_builder.add_missing_fields(custom);
        if targets_were_missing && config_builder.targets.is_some() {
            targets_config_source = Some(config_name);
        }
    }

    // Naming a configuration is asking for that one and no other, so the project's settings are not
    // merged underneath it. What is left, a run that named none, is where the project answers.
    if let Some(local) = config_builder.local_dir.clone()
        && config_builder.config_name_to_load.is_none() && config_builder.config_name_to_save.is_none() {
        let targets_were_missing = config_builder.targets.is_none();
        if apply_local_configuration(&mut config_builder, &local)? {
            if let Some(found) = &mut config_builder.local_dir {
                found.configuration_applied = true;
            }
            if targets_were_missing && config_builder.targets.is_some() {
                targets_config_source = Some(local.get_config_path());
            }
        }
    }

    // Above the default configuration and below the project's own, so that what is written is the
    // command line over the project's settings and nothing of this machine's
    if save_local {
        save_the_local_configuration(&mut config_builder, &typed_paths)?;
    }

    if let Some(name) = &config_builder.config_name_to_save {
        if config_builder.targets.is_none() {
            config_builder.targets = Some(create_targets_from_working_dir()?);
        }

        match super::config_files::save_existing_commands_from_config_builder_to_file(None, name, None, &config_builder) {
            Err(error) => eprintln!("\n{}", wrap_message(&format!(
                    "Configuration '{name}' could not be written to '{}': {error}",
                    crate::paths::PERSISTENT_APP_PATHS.config_dir)).yellow()),
            Ok(_) => eprintln!("\nConfiguration '{name}' saved. Load it with '--{LOAD} {name}'.")
        }
    }

    if config_builder.has_missing_fields() {
        match super::config_files::parse_config_file(None, None) {
            Ok((default_config, issues)) => {
                print_config_file_warnings(&issues.warnings, DEFAULT_CONFIG_LABEL);
                // Under a project's own configuration this machine's saved defaults answer for the
                // look of the report and for nothing that decides a number. What the project left
                // unlocked has to mean the program's default, which is the same for everybody, or
                // two people counting one tree still get two answers and the file that was supposed
                // to end that argument never touches the fields the argument is about.
                let under_a_project = config_builder.local_dir.as_ref().is_some_and(|x| x.configuration_applied);
                let (default_config, invalid_fields) = if under_a_project {
                    (default_config.forget_what_changes_the_numbers(),
                            issues.invalid_fields.iter().copied()
                                    .filter(|field| !CHANGES_THE_NUMBERS.contains(field)).collect())
                } else {
                    (default_config, issues.invalid_fields)
                };
                resolve_invalid_config_fields(&config_builder, &invalid_fields, DEFAULT_CONFIG_LABEL)?;
                let targets_were_missing = config_builder.targets.is_none();
                config_builder.add_missing_fields(default_config);
                if targets_were_missing && config_builder.targets.is_some() {
                    targets_config_source = Some(DEFAULT_CONFIG_LABEL.to_owned());
                }
            },
            // An absent default configuration is an ordinary machine. A half-readable one is not,
            // and skipping it in silence would run with whatever defaults it no longer supplies.
            Err(super::config_files::ConfigFileParseError::UnreadableLine(file, line, cause)) =>
                return Err(ArgParsingError::UnreadableConfig(file, line, cause)),
            Err(_) => {}
        }
    }

    // No pattern is expanded here, or anywhere in this crate: the run resolves the declared targets
    // at its entry, under the flags of the same configuration the walk obeys. Only the name of the
    // file that supplied them is kept, so that the run's refusal can say it.
    config_builder.targets_source = targets_config_source;

    if let Some(name) = &config_builder.theme_name {
        match super::theme_files::load_theme(name, &crate::paths::PERSISTENT_APP_PATHS.themes_dir) {
            Some((styles, errors)) => {
                for error in &errors {
                    super::warning_collector::emit(mezura_core::warnings::Warning::new(mezura_core::warnings::Code::ConfigStyleInvalid, name,
                            format!("In theme '{name}': {}", error.format())));
                }
                config_builder.theme_styles = Some(styles);
            },
            None => super::warning_collector::emit(mezura_core::warnings::Warning::new(mezura_core::warnings::Code::ThemeUnavailable, name,
                    format!("Theme '{name}' could not be loaded, the default styles will be used.")))
        }
    }

    if config_builder.targets.is_none() {
        config_builder.targets = Some(create_targets_from_working_dir()?);
    }

    Ok(config_builder)
}

// Carries what was typed into the error, so the message can name it instead of saying that
// something was wrong
fn refuse_argument(given: &str, command: String) -> ArgParsingError {
    ArgParsingError::IncorrectCommandArgs(command, given.trim().to_owned())
}

// The three shapes every branch of the command loop is one of: a value that parses or is refused
// with that command's help, a flag that takes no argument, and a name that must not be empty.
fn parse_or_refuse<T>(command: &str, arguments: &str,
        parse: impl FnOnce(&str) -> Option<T>) -> Result<T, ArgParsingError>
{
    parse(arguments).ok_or_else(|| {
        message_printer::print_help_message_for_command(command);
        refuse_argument(arguments, command.to_owned())
    })
}

fn take_flag(segment: &str, command: &str) -> Result<bool, ArgParsingError> {
    if has_any_args(segment) {
        message_printer::print_help_message_for_command(command);
        return Err(ArgParsingError::UnexpectedCommandArgs(command.to_owned()));
    }

    Ok(true)
}

fn take_name(command: &str, arguments: &str) -> Result<String, ArgParsingError> {
    let name = arguments.trim();
    if name.is_empty() {
        message_printer::print_help_message_for_command(command);
        return Err(refuse_argument(arguments, command.to_owned()));
    }

    Ok(name.to_owned())
}

// The project's own configuration, merged under the command line. Answers whether the folder held
// one at all, since it may hold nothing but a log.
fn apply_local_configuration(config_builder: &mut ConfigurationBuilder, local: &LocalDir)
-> Result<bool, ArgParsingError>
{
    let file_name = crate::paths::LOCAL_CONFIG_FILE_NAME.trim_end_matches(".txt");
    let (mut local_config, issues) = match super::config_files::parse_config_file(Some(file_name), Some(local.get_dir_path())) {
        Ok(x) => x,
        Err(super::config_files::ConfigFileParseError::FileNotFound(_)) => return Ok(false),
        Err(super::config_files::ConfigFileParseError::UnreadableLine(_, line, cause)) =>
                return Err(ArgParsingError::UnreadableConfig(local.get_config_path(), line, cause))
    };

    let label = local.get_config_path();
    print_config_file_warnings(&issues.warnings, &label);

    // This file travels with the code to machines and versions it has never met, so a value of its
    // own that only decides how the report looks is reported and skipped rather than killing
    // somebody else's run. One that decides what gets counted still stops it, because counting on
    // with a default is the disagreement between two people's numbers that the file exists to end.
    let (changes_the_numbers, presentation) : (Vec<&str>, Vec<&str>) = issues.invalid_fields.iter()
            .partition(|field| CHANGES_THE_NUMBERS.contains(field));
    for field in presentation {
        report_ignored_config_value(field, &label);
    }
    resolve_invalid_config_fields(config_builder, &changes_the_numbers, &label)?;

    local_config.targets = local_config.targets.map(|declared| rebase_targets_on(&local.project_dir, declared));
    config_builder.add_missing_fields(local_config);

    Ok(true)
}

// Written where the next run will look for it: into the folder this one found, from wherever inside
// the project the command was typed, and otherwise into a new folder at the directory holding the
// targets.
fn save_the_local_configuration(config_builder: &mut ConfigurationBuilder, typed_paths: &[String])
-> Result<(), ArgParsingError>
{
    let Some(local) = config_builder.local_dir.clone()
            .or_else(|| crate::paths::choose_place_for_a_local_dir(typed_paths)) else {
        eprintln!("\n{}", wrap_message(&format!("'--{SAVE_LOCAL}' has nowhere to write: the targets of this run \
have no directory holding all of them, so there is no one project for these settings to belong to.")).yellow());
        return Ok(());
    };

    if config_builder.targets.is_none() {
        config_builder.targets = Some(create_targets_from_working_dir()?);
    }

    let written = std::fs::create_dir_all(local.get_dir_path()).and_then(|()|
            super::config_files::save_existing_commands_from_config_builder_to_file(Some(local.get_dir_path()),
                    crate::paths::LOCAL_CONFIG_FILE_NAME.trim_end_matches(".txt"), Some(&local.project_dir), config_builder));

    match written {
        Err(error) => eprintln!("\n{}", wrap_message(&format!(
                "The settings of this project could not be written to '{}': {error}", local.get_config_path())).yellow()),
        Ok(()) => {
            // A folder this run made is a folder this run has, so a '--log' beside it writes into
            // the project rather than being told there is nowhere to write
            config_builder.local_dir = Some(local.clone());
            eprintln!("\nSaved as the settings of this project, in '{}'.", local.get_config_path());
        }
    }

    Ok(())
}

// A relative target of a project's configuration names a place inside the project, wherever the
// command happened to be typed from. Joined here, because the run resolves what is still relative
// against the working directory.
fn rebase_targets_on(project_dir: &str, targets: Vec<Target>) -> Vec<Target> {
    targets.into_iter().map(|target| {
        let declared = std::path::Path::new(&target.path);
        if declared.is_absolute() || declared.has_root() {
            target
        } else {
            let inside = target.path.trim_start_matches("./");
            let path = if inside.is_empty() {project_dir.to_owned()} else {format!("{project_dir}/{inside}")};
            Target { module: target.module, path }
        }
    }).collect()
}

fn print_config_file_warnings(issues: &[(mezura_core::warnings::Code, String)], config_name: &str) {
    for (code, warning) in issues {
        super::warning_collector::emit(mezura_core::warnings::Warning::new(*code, config_name,
                format!("In config '{config_name}': {warning}")));
    }
}

// Every command that can end up in 'invalid_fields' belongs here. One that is missing is treated as
// never overridden, so giving it correctly on the command line would still not rescue the run.
fn resolve_invalid_config_fields(config_builder: &ConfigurationBuilder, invalid_fields: &[&str], config_name: &str) -> Result<(), ArgParsingError> {
    // Destructured with no '..', so a new field of the builder stops the build here until somebody
    // decides whether it belongs in the match below.
    let ConfigurationBuilder {
            targets, exclude_dirs, forced_languages, threads, counting, should_search_in_dotted,
            count_minified, count_generated, count_not_code, should_show_faulty_files,
            should_show_skipped_files, hidden,
            no_gitignore, no_ignore_files, no_heuristics, theme_name, compare_level, bar_thickness,
            progress_bar, number_separator, decimal_separator, layout, sort_by, top_n, by_file,
            // these two accept whatever they are given, so a config can hold no invalid value for
            // them and they never reach 'invalid_fields'
            languages_of_interest: _, excluded_languages: _,
            // not carried by a configuration file at all
            config_name_to_save: _, config_name_to_load: _, theme_name_to_save: _, output: _,
            explain: _, diff_against: _, log: _, targets_source: _, local_dir: _, typed_explicitly: _,
            // a style that does not parse is reported per line and skipped, and the rest of the file
            // still applies, so these warn instead of reaching here
            styles: _, config_styles: _, theme_styles: _ } = config_builder;

    for field in invalid_fields {
        let is_overridden = match *field {
            TARGETS => targets.is_some(),
            THREADS => threads.is_some(),
            COMPARE_LEVEL => compare_level.is_some(),
            COUNTING => counting.is_some(),
            SEARCH_IN_DOTTED => should_search_in_dotted.is_some(),
            COUNT_MINIFIED => count_minified.is_some(),
            COUNT_GENERATED => count_generated.is_some(),
            COUNT_NOT_CODE => count_not_code.is_some(),
            SHOW_FAULTY_FILES => should_show_faulty_files.is_some(),
            SHOW_SKIPPED => should_show_skipped_files.is_some(),
            HIDE => hidden.is_some(),
            NO_GITIGNORE => no_gitignore.is_some(),
            NO_IGNORE_FILES => no_ignore_files.is_some(),
            NO_HEURISTICS => no_heuristics.is_some(),
            EXCLUDE => exclude_dirs.is_some(),
            FORCE_LANGUAGE => forced_languages.is_some(),
            THEME => theme_name.is_some(),
            SORT => sort_by.is_some(),
            TOP => top_n.is_some(),
            BY_FILE => by_file.is_some(),
            BAR_THICKNESS => bar_thickness.is_some(),
            PROGRESS_BAR => progress_bar.is_some(),
            NUMBER_SEPARATOR => number_separator.is_some(),
            DECIMAL_SEPARATOR => decimal_separator.is_some(),
            LAYOUT => layout.is_some(),
            _ => false
        };

        if is_overridden {
            report_ignored_config_value(field, config_name);
        } else {
            message_printer::print_help_message_for_command(field);
            return Err(ArgParsingError::InvalidValueInConfig(field.to_string(), config_name.to_owned()));
        }
    }

    Ok(())
}

fn report_ignored_config_value(field: &str, config_name: &str) {
    super::warning_collector::emit(mezura_core::warnings::Warning::new(mezura_core::warnings::Code::ConfigValueIgnored, field,
            format!("Invalid value for the command '--{field}', in config '{config_name}'. The value will be ignored.")));
}

fn print_warnings_for_commands_that_need_a_loaded_configuration(builder: &ConfigurationBuilder,
        a_local_dir_was_found: bool)
{
    // Printed here rather than kept for later, since this runs before the theme is resolved, and
    // kept as well, so that a machine consumer learns a command it gave was dropped
    let ignored = |command: &str, message: String| {
        eprintln!("\n{}", wrap_message(&message).yellow());
        super::warning_collector::keep(mezura_core::warnings::Warning::new(
                mezura_core::warnings::Code::CommandIgnored, command, message));
    };

    // A comparison is never logged, and saying so wins over the no-config sentence below: one
    // reason the entry will not be written is enough.
    if let Some(log) = &builder.log && log.should_log && builder.diff_against.is_some() {
        ignored(LOG, "'--log' command will be ignored: a comparison is not logged.".to_owned());
    }

    // A project's folder is a configuration without a name: it has a log of its own, so these two
    // have somewhere to write to and somewhere to read from even when nothing was named.
    if builder.config_name_to_load.is_none() && !a_local_dir_was_found {
        if let Some(log) = &builder.log && builder.config_name_to_save.is_none()
                && log.should_log && builder.diff_against.is_none() {
            ignored(LOG, "'--log' command will be ignored, since no config file was specified.".to_owned());
        }

        if builder.compare_level.is_some() {
            ignored(COMPARE_LEVEL, "'--compare' command will be ignored, since no config file was specified for loading.".to_owned());
        }
    }
}

fn has_any_args(command: &str) -> bool {
    command.split(' ').skip(1).filter_map(super::args::get_trimmed_if_not_empty).count() != 0
}

// Only the half of resolution that no setting can change, so a typed path that names nothing is
// refused the moment it was typed. Patterns are expanded by the run itself, under the flags of the
// merged configuration it is handed.
fn parse_targets(s: &str) -> Result<Vec<Target>, ArgParsingError> {
    let declared = super::args::parse_targets(s).map_err(ArgParsingError::MalformedTarget)?
            .into_iter().map(|(module, path)| Target { module, path }).collect::<Vec<_>>();
    mezura_core::engine::targets::validate_and_absolutize(&declared).map_err(map_target_error)
}

// The engine decides which paths are walkable; this turns what it says into the wording a person
// reads on the command line.
fn map_target_error(x: mezura_core::engine::targets::TargetError) -> ArgParsingError {
    use mezura_core::engine::targets::TargetError;
    match x {
        TargetError::InvalidPath(p) => ArgParsingError::InvalidPath(p),
        TargetError::InvalidGlob(p) => ArgParsingError::InvalidGlobPattern(p),
        TargetError::NoGlobMatches(p) => ArgParsingError::NoGlobMatches(p),
        TargetError::AllGlobMatchesIgnored(p) => ArgParsingError::AllGlobMatchesIgnored(p),
        TargetError::Contested(path, a, b) => ArgParsingError::ContestedTarget(path, a, b),
        // 'TargetError' is non_exhaustive, so a variant added later stops here rather than
        // in the middle of a run
        other => ArgParsingError::InvalidPath(format!("{other:?}"))
    }
}

// The working directory is not something anybody typed, so it skips the parser that takes typed
// text apart: one containing a space would be split into two targets, neither of which exists.
fn create_targets_from_working_dir() -> Result<Vec<Target>, ArgParsingError> {
    if let Ok(path_buf) = std::env::current_dir()
        && let Some(path_str) = path_buf.to_str() {
        return Ok(vec![Target::of(mezura_core::engine::targets::convert_to_absolute(path_str))]);
    }

    Err(ArgParsingError::UnparsableWorkingDir)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::ops::Add;
    use std::path::Path;

    use super::super::theme::Style;
    use super::*;

    // Rendered back into the form they were declared in, so that a test reads the same way whether
    // the target was named or not. Nothing is expanded: that belongs to the run and is asserted in
    // the engine's own tests.
    fn parse_targets(s: &str) -> Result<Vec<String>, ArgParsingError> {
        super::parse_targets(s).map(|targets| targets.iter().map(Target::to_string).collect())
    }

    // The counting driven the way 'main' drives it, with the language files read from the checkout
    fn counted(config: &Configuration) -> mezura_core::RunResult {
        let languages_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../mezura-core/data/languages/");
        let parsed = mezura_core::language_file::parse_languages_in_dir(languages_dir).unwrap().0;
        let (languages, _) = mezura_core::Languages::resolve(&config.engine, parsed, &Default::default());
        mezura_core::run(&config.engine, languages).unwrap()
    }

    fn new_conf(dir: &str) -> Configuration {
        let targets = vec![Target::of(mezura_core::engine::targets::convert_to_absolute(dir))];
        let mut builder = ConfigurationBuilder { targets: Some(targets), ..Default::default() };
        if let Ok((default_config, _)) = super::super::config_files::parse_config_file(None, None) {
            builder.add_missing_fields(default_config);
        }
        builder.build()
    }

    fn conf(dir: &str, edit: impl FnOnce(&mut Configuration)) -> Configuration {
        let mut config = new_conf(dir);
        edit(&mut config);
        config
    }

    #[test]
    fn a_repeated_command_is_refused_instead_of_keeping_either_value() {
        assert_eq!(Err(ArgParsingError::RepeatedCommand("threads".to_owned())),
                create_config_from_args("./ --threads 2 10 --threads 3 11"));
        assert_eq!(Err(ArgParsingError::RepeatedCommand("hide".to_owned())),
                create_config_from_args("./ --hide overview --hide keywords"));
        assert_eq!(Err(ArgParsingError::RepeatedCommand("search-in-dotted".to_owned())),
                create_config_from_args("./ --search-in-dotted --search-in-dotted"));
    }

    #[test]
    fn every_way_a_command_line_can_be_wrong_is_reported_as_its_own_mistake() {
        assert_eq!(Err(ArgParsingError::InvalidPath("random".to_owned())), create_config_from_args("random"));
        assert_eq!(Err(ArgParsingError::InvalidPath("./ random".to_owned())), create_config_from_args("./ random"));
        assert_eq!(Err(ArgParsingError::InvalidPath("./ -show-faulty-files".to_owned())), create_config_from_args("--targets ./ -show-faulty-files"));
        assert_eq!(Err(ArgParsingError::UnrecognisedCommand("random".to_owned())), create_config_from_args("--random"));
        assert_eq!(Err(ArgParsingError::UnrecognisedCommand("random".to_owned())), create_config_from_args("--targets ./ --random"));
        assert_eq!(Err(ArgParsingError::DoublePath), create_config_from_args("./ --targets ./"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("targets".to_owned(), String::new())), create_config_from_args("--targets"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("targets".to_owned(), String::new())), create_config_from_args("--targets   "));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned(), String::new())), create_config_from_args("./ --threads"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned(), "33 10".to_owned())), create_config_from_args("./ --threads 33 10"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned(), "2 129".to_owned())), create_config_from_args("./ --threads 2 129"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned(), "33".to_owned())), create_config_from_args("./ --threads 33"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned(), "A".to_owned())), create_config_from_args("./ --threads A"));
        assert_eq!(Err(ArgParsingError::UnexpectedCommandArgs("show-faulty-files".to_owned())), create_config_from_args("./ --threads 1 1 --show-faulty-files 1"));
        assert_eq!(Err(ArgParsingError::UnexpectedCommandArgs("show-faulty-files".to_owned())), create_config_from_args("./ --threads 1 1 --show-faulty-files a"));
        assert_eq!(Err(ArgParsingError::UnexpectedCommandArgs("search-in-dotted".to_owned())), create_config_from_args("./ --threads 1 1 --search-in-dotted a"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("counting".to_owned(), String::new())), create_config_from_args("./ --counting"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("counting".to_owned(), "braces".to_owned())), create_config_from_args("./ --counting braces"));
        assert_eq!(Err(ArgParsingError::UnrecognisedCommand("braces-as-code".to_owned())), create_config_from_args("./ --braces-as-code"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("exclude".to_owned(), String::new())), create_config_from_args("./ --exclude"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("exclude".to_owned(), String::new())), create_config_from_args("./ --exclude   --threads 4"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("exclude".to_owned(), "[invalid".to_owned())), create_config_from_args("./ --exclude [invalid"));
        assert_eq!(Err(ArgParsingError::UnexpectedCommandArgs("no-gitignore".to_owned())), create_config_from_args("./ --no-gitignore a"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("load".to_owned(), String::new())), create_config_from_args("./ --load"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("load".to_owned(), String::new())), create_config_from_args("./ --load   "));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("save".to_owned(), String::new())), create_config_from_args("./ --save"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("save".to_owned(), String::new())), create_config_from_args("./ --save   "));
    }

    // A command's refusal of a bad argument stays beside its acceptance of a good one, so the few
    // errors below are here and not in the test above
    #[test]
    fn every_command_reaches_the_setting_it_names_and_records_that_it_was_typed() {
        assert_ne!(new_conf("../"), create_config_from_args(std::env::current_dir().unwrap().to_str().unwrap()).unwrap());
        assert_eq!(new_conf("./"), create_config_from_args(std::env::current_dir().unwrap().to_str().unwrap()).unwrap());

        assert_eq!(new_conf("./"), create_config_from_args("./").unwrap());
        assert_eq!(new_conf("./"), create_config_from_args("--targets ./").unwrap());
        assert_eq!(conf("./", |c| {c.engine.threads = Threads::new(1,1);}), create_config_from_args("./ --threads 1 1").unwrap());
        assert_eq!(conf("./", |c| {c.engine.threads = Threads::new(1,1);}), create_config_from_args("./ --threads   1   1 ").unwrap());
        assert_eq!(conf("./", |c| {c.engine.threads = Threads::new(1,1); c.view.counting = CountingModel::Region; c.typed_explicitly.counting = true;}),
                create_config_from_args("./ --threads 1 1 --counting region").unwrap());
        assert_eq!(conf("./", |c| {c.view.counting = CountingModel::Content; c.typed_explicitly.counting = true;}),
                create_config_from_args("./ --counting content").unwrap());
        assert_eq!(conf("./", |c| {c.engine.should_search_in_dotted = true; c.typed_explicitly.search_in_dotted = true;}),
                create_config_from_args("./ --search-in-dotted").unwrap());
        assert_eq!(conf("./", |c| {c.engine.count_minified = true; c.typed_explicitly.count_minified = true;}),
                create_config_from_args("./ --count-minified").unwrap());
        assert_eq!(Err(ArgParsingError::UnexpectedCommandArgs("count-minified".to_owned())),
                create_config_from_args("./ --count-minified yes"));
        assert_eq!(conf("./", |c| {c.engine.count_generated = true; c.typed_explicitly.count_generated = true;}),
                create_config_from_args("./ --count-generated").unwrap());
        assert_eq!(conf("./", |c| {c.engine.count_not_code = true; c.typed_explicitly.count_not_code = true;}),
                create_config_from_args("./ --count-not-code").unwrap());
        assert_eq!(conf("./", |c| {c.view.should_show_skipped_files = true;}),
                create_config_from_args("./ --show-skipped").unwrap());
        assert_eq!(conf("./", |c| {c.engine.no_gitignore = true; c.typed_explicitly.no_gitignore = true;}),
                create_config_from_args("./ --no-gitignore").unwrap());
        assert_eq!(conf("./", |c| {c.view.set_should_show_faulty_files(true);}),
                create_config_from_args("./ --show-faulty-files").unwrap());
        assert_eq!(conf("./", |c| {c.engine.exclude_dirs = vec!["a".to_owned(),"b".to_owned(),"c".to_owned()]; c.typed_explicitly.exclude = true;}),
                create_config_from_args("./ --exclude a,b ,  c ").unwrap());
        assert_eq!(conf("./", |c| {c.engine.exclude_dirs = vec!["a/path".to_owned(),"b/path".to_owned()]; c.typed_explicitly.exclude = true;}),
                create_config_from_args("./ --exclude \"a/path\", \"b/path\"").unwrap());
        assert_eq!(conf("./", |c| {c.engine.languages_of_interest = vec!["a".to_owned(),"b".to_owned(),"c".to_owned()].into(); c.typed_explicitly.languages = true;}),
                create_config_from_args("./ --languages a,b,c").unwrap());
        assert_eq!(conf("./", |c| {c.engine.languages_of_interest = vec!["a".to_owned()].into(); c.typed_explicitly.languages = true;}),
                create_config_from_args("./ --languages a, ").unwrap());
        assert_eq!(conf("./", |c| {c.view.set_log_option(LogOption::new(Some("this is a test".to_owned())));}),
                create_config_from_args("./ --log   this is a test ").unwrap());
        assert_eq!(conf("./", |c| {c.view.set_log_option(LogOption::new(None));}),
                create_config_from_args("./ --log  ").unwrap());
    }

    #[test]
    fn by_file_takes_a_number_or_nothing_and_reads_zero_as_every_file() {
        let by_file = |command: &str| create_config_from_args(command).unwrap().view.by_file;

        assert_eq!(None, by_file("./"));
        assert_eq!(Some(ByFile::All), by_file("./ --by-file"));
        assert_eq!(Some(ByFile::All), by_file("./ --by-file   "));
        assert_eq!(Some(ByFile::Capped(20)), by_file("./ --by-file 20"));
        assert_eq!(Some(ByFile::All), by_file("./ --by-file 0"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs(BY_FILE.to_owned(), "nope".to_owned())),
                create_config_from_args("./ --by-file nope"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs(BY_FILE.to_owned(), "-3".to_owned())),
                create_config_from_args("./ --by-file -3"));

        // Asking for the rows is what makes the counting keep them, so the two halves cannot drift
        assert!(!create_config_from_args("./").unwrap().engine.collect_files);
        assert!(create_config_from_args("./ --by-file").unwrap().engine.collect_files);

        // A comparison collects them too, since its subject side is this very run
        let compared = create_config_from_args("./ --by-file 8 --diff old.json").unwrap();
        assert!(compared.engine.collect_files);
        assert_eq!(Some(ByFile::Capped(8)), compared.view.by_file);

        // What a configuration file stores is what the command line accepts back
        assert_eq!(Some(ByFile::All), ByFile::parse(&ByFile::All.to_text()));
        assert_eq!(Some(ByFile::Capped(7)), ByFile::parse(&ByFile::Capped(7).to_text()));
    }

    #[test]
    fn every_name_hide_accepts_switches_off_its_own_part_and_nothing_else() {
        let hidden = |command: &str| create_config_from_args(command).unwrap().view.hidden;

        assert_eq!(Hidden::default(), hidden("./"));
        assert_eq!(Hidden {keywords: true, ..Default::default()}, hidden("./ --hide keywords"));
        assert_eq!(Hidden {animations: true, ..Default::default()}, hidden("./ --hide animations"));
        // Commas and spaces both separate, so the Powershell comma escaping is never needed
        let expected = Hidden {parsing_info: true, bar: true, timing: true, ..Default::default()};
        assert_eq!(expected, hidden("./ --hide parsing-info,bar,timing"));
        assert_eq!(expected, hidden("./ --hide parsing-info bar timing"));
        assert_eq!(expected, hidden("./ --hide  PARSING-INFO , bar,  Timing "));

        // The error names the entry that was not understood, instead of the whole command
        assert_eq!(Err(ArgParsingError::InvalidHideTarget("detials".to_owned())),
                create_config_from_args("./ --hide keywords,detials"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs(HIDE.to_owned(), String::new())), create_config_from_args("./ --hide"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs(HIDE.to_owned(), String::new())), create_config_from_args("./ --hide   "));

        // What is written to a config file is what the command line accepts
        assert_eq!("parsing-info,bar,timing", expected.to_list_string());
        assert_eq!(Ok(expected), Hidden::parse(&expected.to_list_string()));
        assert_eq!(Ok(Hidden::default()), Hidden::parse(""));

        // and every name the struct knows survives that round trip, so a new field cannot be
        // written by '--save' and refused on the way back
        let every_name = Hidden::get_names().join(",");
        assert_eq!(every_name, Hidden::parse(&every_name).unwrap().to_list_string());

        // One at a time as well, since 'parse' and 'get_pairs' are two lists written by hand and
        // all seventeen at once cannot tell two names wired to each other's field apart
        for name in Hidden::get_names() {
            assert_eq!(name, Hidden::parse(name).unwrap().to_list_string(),
                    "'--hide {name}' switches off another part of the report");
        }

        // The mask asks whether keywords were hidden, not whether '--hide' was typed at all: a
        // '--hide timing' says nothing about them
        assert!(create_config_from_args("./ --hide keywords,timing").unwrap().typed_explicitly.hide_keywords);
        assert!(!create_config_from_args("./ --hide timing").unwrap().typed_explicitly.hide_keywords);
        assert!(!create_config_from_args("./").unwrap().typed_explicitly.hide_keywords);
    }

    // A JSON document carries every figure whatever is hidden, so there the order stands as asked
    #[test]
    fn sorting_by_a_hidden_column_falls_back_to_lines() {
        let sorted = |command: &str| create_config_from_args(command).unwrap().view.sort_by;

        assert_eq!(SortCriterion::Lines, sorted("./ --hide size --sort size"));
        assert_eq!(SortCriterion::Lines, sorted("./ --hide size --sort size --diff old.json"));
        assert_eq!(SortCriterion::Size, sorted("./ --hide extra --sort size"));
        assert_eq!(SortCriterion::Size, sorted("./ --hide size --sort size --output json"));
        assert_eq!(SortCriterion::Code, sorted("./ --hide files,comments,extra,size,percentages --sort code"));
    }

    #[test]
    fn the_word_of_the_other_counting_model_orders_nothing_and_hides_nothing() {
        let view = |command: &str| create_config_from_args(command).unwrap().view;

        assert_eq!(SortCriterion::Lines, view("./ --counting region --sort extra").sort_by);
        assert_eq!(SortCriterion::Blanks, view("./ --counting region --sort blanks").sort_by);
        assert_eq!(SortCriterion::Extra, view("./ --sort extra").sort_by);
        assert_eq!(SortCriterion::Lines, view("./ --sort blanks").sort_by);

        assert!(!view("./ --counting region --hide extra").hidden.extra);
        assert!(view("./ --counting region --hide blanks").hidden.extra);
        assert!(view("./ --hide extra").hidden.extra);
        assert!(!view("./ --hide blanks").hidden.extra);
    }

    #[test]
    fn a_command_followed_by_nothing_but_spaces_was_given_no_arguments() {
        assert!(has_any_args("cmnd a"));
        assert!(has_any_args("cmnd    a"));
        assert!(has_any_args("cmnd    a   "));
        assert!(has_any_args("cmnd a a"));

        assert!(!has_any_args("cmnd"));
        assert!(!has_any_args("cmnd    "));
    }

    #[test]
    fn a_target_is_absolutized_and_one_that_is_not_there_is_refused() {
        assert_eq!(Err(ArgParsingError::InvalidPath("a".to_owned())), parse_targets("a"));
        assert_eq!(Err(ArgParsingError::InvalidPath("a b c".to_owned())), parse_targets("a b c"));

        assert_eq!(vec![mezura_core::engine::targets::convert_to_absolute("./")], parse_targets("./").unwrap());
        assert_eq!(vec![mezura_core::engine::targets::convert_to_absolute("./src")], parse_targets("\"./src\"").unwrap());

        // Declared as written: a target inside another survives the parse, because the swallowing
        // of overlaps happens with the expansion, inside the run
        assert_eq!(vec![mezura_core::engine::targets::convert_to_absolute("./"), mezura_core::engine::targets::convert_to_absolute(".././")],
                parse_targets("./, .././").unwrap());

        // A space is not a separator while no module is named, so a path is allowed to contain one
        assert_eq!(Err(ArgParsingError::InvalidPath("./tests ./src".to_owned())), parse_targets("./tests ./src"));
        assert_eq!(vec![mezura_core::engine::targets::convert_to_absolute("./")], parse_targets(&std::env::current_dir().unwrap().to_string_lossy()).unwrap());
    }

    #[test]
    fn a_target_can_be_declared_under_a_module_name() {
        let src = mezura_core::engine::targets::convert_to_absolute("./src");
        let tests = mezura_core::engine::targets::convert_to_absolute("./tests");

        // Only the command line's own half: a declared name reaches a Target with its path made
        // absolute, and the errors are worded here. The grammar itself belongs to 'args'.
        assert_eq!(vec![format!("code={src}"), format!("suite={tests}")], parse_targets("code=./src suite=./tests").unwrap());
        assert_eq!(vec![format!("code={src}"), tests.clone()], parse_targets("code=./src ./tests").unwrap());

        // An '=' is a legal character in a path, so anything that looks like one is read as one
        assert_eq!(vec![src.clone()], parse_targets("./src").unwrap());
        assert_eq!(Err(ArgParsingError::MalformedTarget("code=".to_owned())), parse_targets("code="));
        assert_eq!(Err(ArgParsingError::InvalidPath("nope".to_owned())), parse_targets("code=nope"));
    }

    #[test]
    fn a_mistake_in_a_configs_targets_still_names_the_configuration() {
        std::fs::create_dir_all(&crate::paths::PERSISTENT_APP_PATHS.config_dir).unwrap();
        let write_config = |name: &str, targets: &str| {
            let path = crate::paths::PERSISTENT_APP_PATHS.config_dir.clone() + name + ".txt";
            std::fs::write(&path, format!("===> targets\n{targets}\n")).unwrap();
            path
        };

        let path = write_config("a2resolve1", "./does-not-exist-a2");
        let config = create_config_from_args("--load a2resolve1").unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(vec![Target::of("./does-not-exist-a2")], config.engine.targets);
        assert_eq!(Some("a2resolve1".to_owned()), config.view.targets_source);

        // the error a real run returns, through the same join 'main' prints it with
        let languages_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../mezura-core/data/languages/");
        let parsed = mezura_core::language_file::parse_languages_in_dir(languages_dir).unwrap().0;
        let (languages, _) = mezura_core::Languages::resolve(&config.engine, parsed, &Default::default());
        let mezura_core::RunError::InvalidTargets(inner) = mezura_core::run(&config.engine, languages).unwrap_err()
                else { panic!("the run did not refuse the config's targets") };
        let attributed = attribute_targets_error(inner, &config.view.targets_source);
        assert_eq!(ArgParsingError::InvalidTargetInConfig(
                Box::new(ArgParsingError::InvalidPath("./does-not-exist-a2".to_owned())), "a2resolve1".to_owned()),
                attributed);
        // The cause keeps its own sentence, so a pattern that matched only ignored files is not
        // reported as a path that is gone
        let ignored = attribute_targets_error(mezura_core::TargetError::AllGlobMatchesIgnored("s/*/x".to_owned()),
                &Some("a2resolve1".to_owned()));
        assert!(ignored.format().to_string().contains("--no-gitignore"), "{}", ignored.format());
        assert!(ignored.format().to_string().contains("named in config 'a2resolve1'"), "{}", ignored.format());

        // typed on the command line there is no configuration to name, and a contest never gets
        // one, since naming a file would hide that both declarations are the user's own
        assert_eq!(ArgParsingError::InvalidPath("./gone".to_owned()),
                attribute_targets_error(mezura_core::TargetError::InvalidPath("./gone".to_owned()), &None));
        assert_eq!(ArgParsingError::ContestedTarget("./src".to_owned(), "frontend".to_owned(), "backend".to_owned()),
                attribute_targets_error(mezura_core::TargetError::Contested("./src".to_owned(),
                        "frontend".to_owned(), "backend".to_owned()), &config.view.targets_source));

        let path = write_config("a2resolve3", "code=./src");
        let result = create_config_from_args("--load a2resolve3").unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(vec![Target::named("code", "./src")], result.engine.targets);
    }

    fn a2_corpus(name: &str) -> (std::path::PathBuf, String) {
        let corpus = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&corpus);
        std::fs::create_dir_all(corpus.join("target")).unwrap();
        // A bare directory named '.git' bounds the gitignore stack, so nothing above the corpus
        // takes part in the test
        std::fs::create_dir_all(corpus.join(".git")).unwrap();
        std::fs::write(corpus.join(".gitignore"), "target/\n").unwrap();
        std::fs::write(corpus.join("target").join("lib.rs"), "fn hidden() {}\n").unwrap();
        let corpus_str = corpus.to_str().unwrap().replace('\\', "/");
        (corpus, corpus_str)
    }

    // The run resolves the targets under the flags of the configuration it was handed, so a glob
    // whose matches are all gitignored counts them when the flag beside it says to.
    #[test]
    fn a_configs_own_flags_apply_when_its_own_targets_are_resolved() {
        std::fs::create_dir_all(&crate::paths::PERSISTENT_APP_PATHS.config_dir).unwrap();
        let (corpus, corpus_str) = a2_corpus("mezura-a2-config-corpus");

        let config_path = crate::paths::PERSISTENT_APP_PATHS.config_dir.clone() + "a2gitignore.txt";
        std::fs::write(&config_path, format!(
                "===> targets\n{corpus_str}/target/*\n\n===> no-gitignore\nyes\n")).unwrap();

        let config = create_config_from_args("--load a2gitignore");
        std::fs::remove_file(&config_path).unwrap();

        let config = config.expect("the configuration did not load");
        assert!(config.engine.no_gitignore);
        assert_eq!(vec![Target::of(format!("{corpus_str}/target/*"))], config.engine.targets);
        let result = counted(&config);
        std::fs::remove_dir_all(&corpus).unwrap();
        assert_eq!(1, result.total.files, "the gitignored match was not counted");
    }

    #[test]
    fn a_loaded_configs_flags_apply_to_a_command_line_glob() {
        std::fs::create_dir_all(&crate::paths::PERSISTENT_APP_PATHS.config_dir).unwrap();
        let (corpus, corpus_str) = a2_corpus("mezura-a2-cli-corpus");

        let config_path = crate::paths::PERSISTENT_APP_PATHS.config_dir.clone() + "a2cliflag.txt";
        std::fs::write(&config_path, "===> no-gitignore\nyes\n").unwrap();

        let config = create_config_from_args(&format!("{corpus_str}/target/* --load a2cliflag"));
        std::fs::remove_file(&config_path).unwrap();

        let config = config.expect("the configuration did not load");
        assert!(config.engine.no_gitignore);
        assert_eq!(vec![Target::of(format!("{corpus_str}/target/*"))], config.engine.targets);
        let result = counted(&config);
        std::fs::remove_dir_all(&corpus).unwrap();
        assert_eq!(1, result.total.files, "the gitignored match was not counted");
    }

    // Writing the matches instead would make the configuration a snapshot pretending to be a rule
    #[test]
    fn saving_a_glob_saves_the_pattern_and_not_todays_matches() {
        std::fs::create_dir_all(&crate::paths::PERSISTENT_APP_PATHS.config_dir).unwrap();
        let corpus = std::env::temp_dir().join("mezura-a2-save-corpus");
        let _ = std::fs::remove_dir_all(&corpus);
        std::fs::create_dir_all(corpus.join("sub1")).unwrap();
        std::fs::create_dir_all(corpus.join("sub2")).unwrap();
        std::fs::write(corpus.join("sub1").join("a.rs"), "fn a() {}\n").unwrap();
        std::fs::write(corpus.join("sub2").join("b.rs"), "fn b() {}\n").unwrap();
        let corpus_str = corpus.to_str().unwrap().replace('\\', "/");

        let config_path = crate::paths::PERSISTENT_APP_PATHS.config_dir.clone() + "a2save.txt";
        create_config_from_args(&format!("{corpus_str}/sub* --save a2save")).unwrap();
        let saved = std::fs::read_to_string(&config_path).unwrap();

        let loaded = create_config_from_args("--load a2save").unwrap();
        std::fs::remove_file(&config_path).unwrap();

        assert!(saved.contains(&format!("{corpus_str}/sub*")),
                "the pattern is not in the saved file:\n{saved}");
        assert!(!saved.contains("sub1"), "the saved file holds the expansion, not the pattern:\n{saved}");
        assert_eq!(vec![Target::of(format!("{corpus_str}/sub*"))], loaded.engine.targets);
        let result = counted(&loaded);
        std::fs::remove_dir_all(&corpus).unwrap();
        assert_eq!(2, result.total.files);
    }

    #[test]
    fn an_unreadable_config_is_not_reported_as_a_missing_one() {
        std::fs::create_dir_all(&crate::paths::PERSISTENT_APP_PATHS.config_dir).unwrap();
        let config_path = crate::paths::PERSISTENT_APP_PATHS.config_dir.clone() + "a3halfway.txt";
        let mut contents = b"===> threads\n2 8\n".to_vec();
        contents.extend([0xCF, 0xE1, b'\n']);
        std::fs::write(&config_path, contents).unwrap();

        let result = create_config_from_args("./ --load a3halfway").map(|_| ());
        std::fs::remove_file(&config_path).unwrap();

        assert_eq!(Err(ArgParsingError::UnreadableConfig("a3halfway".to_owned(), 3,
                super::super::config_files::UnreadableCause::NotUtf8)), result);
    }

    // Tools that encode a hierarchy into a single folder name produce such paths, and splitting on
    // the substring cuts them into a target that does not exist and a command that does not parse.
    #[test]
    fn a_double_dash_inside_a_path_is_not_the_start_of_a_command() {
        let root = std::env::temp_dir().join("mezura--double--dash");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.rs"), "fn a() {}\n").unwrap();
        let root_str = root.to_str().unwrap().replace('\\', "/");

        let bare = create_config_from_args(&root_str).unwrap();
        let with_flag = create_config_from_args(&format!("{root_str} --threads 2 3")).unwrap();
        let through_targets = create_config_from_args(&format!("--targets {root_str} --threads 2 3")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();

        assert_eq!(vec![Target::of(root_str.clone())], bare.engine.targets);
        assert_eq!(vec![Target::of(root_str.clone())], with_flag.engine.targets);
        assert_eq!(Threads::new(2, 3), with_flag.engine.threads);
        assert_eq!(vec![Target::of(root_str)], through_targets.engine.targets);
        assert_eq!(Threads::new(2, 3), through_targets.engine.threads);
    }

    #[test]
    fn a_saved_configuration_loads_back_into_the_run_that_saved_it() {
        // The saving and loading of configs always goes through the persistent config dir, which doesn't
        // exist yet on a machine where the program has never been executed.
        std::fs::create_dir_all(&crate::paths::PERSISTENT_APP_PATHS.config_dir).unwrap();
        let test_file_path = &crate::paths::PERSISTENT_APP_PATHS.config_dir.clone().add("/test000.txt");
        assert!(!Path::new(test_file_path).exists());

        let mut saved_config = create_config_builder_from_args("--threads 1 5 --languages lang1, lang2 --save test000").unwrap();
        assert!(Path::new(test_file_path).exists());
        assert_eq!(saved_config.targets.clone().unwrap()[0], Target::of(mezura_core::engine::targets::convert_to_absolute("./")));
        assert_eq!(saved_config.threads.unwrap(), Threads::new(1, 5));
        assert_eq!(saved_config.languages_of_interest.clone().unwrap(), vec!["lang1", "lang2"]);

        let mut loaded_config = create_config_builder_from_args("--load test000").unwrap();
        saved_config.config_name_to_save = None;
        loaded_config.config_name_to_load = None;
        // Bookkeeping about where the targets came from, not a value that was saved
        loaded_config.targets_source = None;
        // A fact about each command line and not a value that was saved: the first typed its
        // languages, the second loaded them
        assert!(saved_config.typed_explicitly.languages && !loaded_config.typed_explicitly.languages);
        saved_config.typed_explicitly = TypedExplicitlyOnCommandLine::default();
        assert_eq!(saved_config, loaded_config);

        loaded_config = create_config_builder_from_args("--load test000 --threads 1 4 --targets ./").unwrap();
        assert_eq!(saved_config.targets, loaded_config.targets);
        assert_ne!(saved_config.threads, loaded_config.threads);

        saved_config = create_config_builder_from_args("--load test000 --threads 1 4 --targets ./ --save test000").unwrap();
        saved_config.config_name_to_save = None;
        assert_eq!(saved_config, loaded_config);

        std::fs::remove_file(test_file_path).unwrap();
    }

    #[test]
    fn a_theme_named_on_the_command_line_is_loaded_and_an_unknown_one_stops_the_run() {
        std::fs::create_dir_all(&crate::paths::PERSISTENT_APP_PATHS.themes_dir).unwrap();
        let test_theme_path = &crate::paths::PERSISTENT_APP_PATHS.themes_dir.clone().add("test-theme000.txt");
        // Cleaning up front instead of asserting absence, so that a failed run does not leave
        // behind a file that makes every later run fail during setup
        let _ = std::fs::remove_file(test_theme_path);
        std::fs::write(test_theme_path, "language-1 = cyan\nlanguage-2 = ff0080\ncode-number = bright-black dim\n").unwrap();

        let config = create_config_from_args("./ --theme Test-Theme000").unwrap();
        assert_eq!(Style::of(Color::Cyan), config.view.theme.language_1);
        assert_eq!(Style::of(Color::TrueColor{r:255,g:0,b:128}), config.view.theme.language_2);
        assert_eq!(Style::of(Color::BrightBlack).dim(), config.view.theme.code_number);

        // --style wins over what the theme declared, and the tokens it does not name survive
        let restyled = create_config_from_args("./ --theme test-theme000 --style code-number=cyan,heading=bold").unwrap();
        assert_eq!(Style::of(Color::Cyan), restyled.view.theme.code_number);
        assert_eq!(Style::plain().bold(), restyled.view.theme.heading);
        assert_eq!(Style::of(Color::Cyan), restyled.view.theme.language_1);

        // The error names what is actually wrong, instead of a generic "incorrect arguments"
        assert!(matches!(create_config_from_args("./ --style"), Err(ArgParsingError::InvalidStyle(_))));
        assert!(matches!(create_config_from_args("./ --style nonsense"), Err(ArgParsingError::InvalidStyle(_))));
        assert_eq!(Err(ArgParsingError::InvalidStyle("'code-numberr' is not a style token.".to_owned())),
                create_config_from_args("./ --style code-numberr=cyan"));
        assert!(matches!(create_config_from_args("./ --style code-number=notacolor"), Err(ArgParsingError::InvalidStyle(_))));
        assert_eq!(Err(ArgParsingError::NonExistantTheme("definitely-not-a-theme000".to_owned())),
                create_config_from_args("./ --theme definitely-not-a-theme000"));

        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("theme".to_owned(), String::new())),
                create_config_from_args("./ --theme"));

        std::fs::remove_file(test_theme_path).unwrap();
    }

    #[test]
    fn a_configuration_holding_a_value_the_run_cannot_use_stops_the_run_and_names_it() {
        std::fs::create_dir_all(&crate::paths::PERSISTENT_APP_PATHS.config_dir).unwrap();
        let test_file_path = &crate::paths::PERSISTENT_APP_PATHS.config_dir.clone().add("/test001.txt");
        assert!(!Path::new(test_file_path).exists());
        std::fs::write(test_file_path, "===> threads\n3343 45534\n").unwrap();

        assert_eq!(Err(ArgParsingError::InvalidValueInConfig("threads".to_owned(), "test001".to_owned())),
                create_config_from_args("./ --load test001"));

        let overridden = create_config_from_args("./ --load test001 --threads 1 2").unwrap();
        assert_eq!(overridden.engine.threads, Threads::new(1, 2));

        std::fs::remove_file(test_file_path).unwrap();
    }

    #[test]
    fn force_lang_takes_pairs_of_an_extension_and_a_language_and_refuses_anything_else() {
        let forced = |args: &str| create_config_from_args(&format!("./ --force-language {args}"))
                .map(|x| x.engine.forced_languages.to_written_form());
        let scoped = |written: &str| written.split(',').map(|pair| pair.split_once('=').unwrap())
                .map(|(claimed, language)| (claimed.to_owned(), language.to_owned())).collect::<HashMap<_,_>>();

        assert_eq!(Ok(scoped("m=matlab")), forced("m=matlab"));
        // Lowercased for the lookup, while the language name is kept as it was typed
        assert_eq!(Ok(scoped(".m=MATLAB,pl=perl")), forced(".M=MATLAB, pl = perl"));
        // The dot survives, or '.gitignore' could not be named at all
        assert_eq!(Ok(scoped(".gitignore=Ini")), forced(".gitignore=Ini"));
        // A module in front of the extension keeps its own capitalisation, being matched against a
        // target name and not against a language
        assert_eq!(Ok(scoped("iOS/m=objective-c,pl=perl")), forced("iOS/M=objective-c,pl=perl"));

        for wrong in ["", "matlab", "m=", "=matlab", "m=matlab,perl", "ios/=matlab", "/m=matlab",
                "ios/m/x=matlab"] {
            assert!(forced(wrong).is_err(), "'--force-language {wrong}' was accepted");
        }
    }

    // The scoped and the plain form of one setting reach the run as the same value however they were
    // written, or a project's saved settings and the command line that saved them would disagree.
    #[test]
    fn a_scoped_setting_survives_being_written_out_and_read_back() {
        let typed = "./ --force-language ios/m=objective-c,pl=perl --languages rust,web/js \
--exclude-languages json,web/xml";
        let config = create_config_from_args(typed).unwrap();

        assert_eq!(hashmap!("ios/m".to_owned() => "objective-c".to_owned(),
                "pl".to_owned() => "perl".to_owned()), config.engine.forced_languages.to_written_form());
        assert_eq!(vec!["rust".to_owned(), "web/js".to_owned()],
                config.engine.languages_of_interest.to_written_form());
        assert_eq!(vec!["json".to_owned(), "web/xml".to_owned()],
                config.engine.excluded_languages.to_written_form());

        let written = super::super::args::forced_languages_to_string(&config.engine.forced_languages);
        assert_eq!("ios/m=objective-c,pl=perl", written);
        assert_eq!(config.engine.forced_languages, super::super::args::parse_forced_languages(&written).unwrap());
    }

    // Every command that 'resolve_invalid_config_fields' does not know about is treated as never
    // overridden, so giving it correctly on the command line would still kill the run
    #[test]
    fn a_command_line_value_rescues_every_invalid_field_of_a_config() {
        std::fs::create_dir_all(&crate::paths::PERSISTENT_APP_PATHS.config_dir).unwrap();
        let test_file_path = &crate::paths::PERSISTENT_APP_PATHS.config_dir.clone().add("/test002.txt");
        let _ = std::fs::remove_file(test_file_path);
        std::fs::write(test_file_path, "===> targets\nfrontend=\n\n===> sort\nnope\n\n===> top\nnope\n\n===> bar-thickness\nnope\n\n\
                ===> progress-bar\nnope\n\n===> number-separator\nnope\n\n===> decimal-separator\nnope\n\n===> force-language\nnope\n\n\
                ===> by-file\nnope\n\n===> counting\nnope\n\n===> count-minified\nnope\n\n\
                ===> count-generated\nnope\n\n===> count-not-code\nnope\n\n===> no-heuristics\nnope\n").unwrap();

        // With no target on the command line to take its place, the run stops instead of counting
        // less than it was asked to
        assert_eq!(Err(ArgParsingError::InvalidValueInConfig("targets".to_owned(), "test002".to_owned())),
                create_config_from_args("--load test002"));

        assert_eq!(Err(ArgParsingError::InvalidValueInConfig("sort".to_owned(), "test002".to_owned())),
                create_config_from_args("./ --load test002"));

        let rescued = create_config_from_args(
                "./ --load test002 --sort name --top 3 --bar-thickness fat --progress-bar hash --number-separator dot --decimal-separator comma --force-language m=matlab --by-file 8 --counting region --count-minified --count-generated --count-not-code --no-heuristics").unwrap();
        assert!(rescued.engine.count_minified && rescued.engine.count_generated && rescued.engine.count_not_code);
        assert!(!rescued.engine.use_heuristics);
        assert_eq!(Some(ByFile::Capped(8)), rescued.view.by_file);
        assert_eq!(vec![Target::of(mezura_core::engine::targets::convert_to_absolute("./"))], rescued.engine.targets);
        assert_eq!(CountingModel::Region, rescued.view.counting);
        assert_eq!(SortCriterion::Name, rescued.view.sort_by);
        assert_eq!(Some(3), rescued.view.top_n);
        assert_eq!(BarThickness::Fat, rescued.view.bar_thickness);
        assert_eq!(ProgressBarStyle::Hash, rescued.view.progress_bar);
        assert_eq!(NumberSeparator::Dot, rescued.view.number_separator);
        assert_eq!(DecimalSeparator::Comma, rescued.view.decimal_separator);
        assert_eq!(hashmap!("m".to_owned() => "matlab".to_owned()), rescued.engine.forced_languages.to_written_form());

        std::fs::remove_file(test_file_path).unwrap();
    }

    // The two have to name the same fields. A field only 'add_missing_fields' knows about is one
    // whose value in the default configuration is dropped whenever the command line happens to
    // supply everything else, and nothing says so; a field only 'has_missing_fields' knows about
    // sends the run to read a configuration it has no use for.
    #[test]
    fn a_field_a_configuration_can_fill_is_a_field_the_builder_asks_about() {
        let donor = || {
            let mut donor = ConfigurationBuilder::default();
            donor.add_missing_fields(create_config_builder_from_args(
                    "./ --exclude a --languages rust --exclude-languages java --force-language m=matlab \
                    --threads 1 1 --counting region --search-in-dotted --count-minified --count-generated \
                    --count-not-code --show-faulty-files --show-skipped --hide bar --no-gitignore --no-ignore-files --no-heuristics \
                    --compare 3 --bar-thickness fat \
                    --progress-bar hash --number-separator dot --decimal-separator comma --layout table \
                    --sort name --top 3 --by-file 8").unwrap());
            // Neither can come off a command line here: a theme is looked up in the data directory,
            // and a style block only ever arrives from inside a configuration file
            donor.theme_name = Some("Mezura".to_owned());
            donor.config_styles = Some(Vec::new());
            donor
        };

        let mut probe = ConfigurationBuilder::default();
        assert!(probe.has_missing_fields(), "an empty builder needs everything");
        probe.add_missing_fields(donor());
        assert!(!probe.has_missing_fields(),
                "a field is asked about that no configuration can fill, so the default one is read for nothing");

        for (name, clear) in [("excluded_languages", (|x: &mut ConfigurationBuilder| x.excluded_languages = None) as fn(&mut ConfigurationBuilder)),
                ("targets", |x| x.targets = None), ("top_n", |x| x.top_n = None), ("by_file", |x| x.by_file = None)] {
            let mut one_short = donor();
            clear(&mut one_short);
            assert!(one_short.has_missing_fields(),
                    "'{name}' is merged from a configuration and never asked about, so its value is dropped");
        }

        // Everything that decides a number is gone and everything that decides the look is kept,
        // which is what this machine's saved defaults are allowed to answer for under a project's
        // own configuration
        let kept = donor().forget_what_changes_the_numbers();
        assert_eq!((None, None, None, None, None), (kept.targets, kept.exclude_dirs,
                kept.languages_of_interest, kept.excluded_languages, kept.forced_languages));
        assert_eq!((None, None, None, None, None, None, None), (kept.counting, kept.should_search_in_dotted,
                kept.count_minified, kept.count_generated, kept.count_not_code, kept.no_gitignore,
                kept.no_ignore_files));
        assert_eq!((Some(3), Some(Layout::Table), Some(SortCriterion::Name), Some(ByFile::Capped(8))),
                (kept.top_n, kept.layout, kept.sort_by, kept.by_file));
        assert_eq!((Some(BarThickness::Fat), Some(ProgressBarStyle::Hash), Some(NumberSeparator::Dot),
                Some(DecimalSeparator::Comma), Some("Mezura".to_owned())),
                (kept.bar_thickness, kept.progress_bar, kept.number_separator, kept.decimal_separator, kept.theme_name));
        assert_eq!((Some(Threads::from((1, 1))), Some(true), Some(3)),
                (kept.threads, kept.should_show_faulty_files, kept.compare_level));
        assert!(kept.hidden.is_some_and(|x| x.bar) && kept.config_styles.is_some());
    }

    // A project of its own, under the temporary directory, which is the only place a test is
    // allowed to find one in
    fn build_test_project(test_name: &str, configuration: &str) -> String {
        let root = std::env::temp_dir().join("mezura-project-".to_owned() + test_name);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        if !configuration.is_empty() {
            std::fs::create_dir_all(root.join(crate::paths::LOCAL_DIR_NAME)).unwrap();
            std::fs::write(root.join(crate::paths::LOCAL_DIR_NAME).join(crate::paths::LOCAL_CONFIG_FILE_NAME),
                    configuration).unwrap();
        }

        crate::paths::normalise_separators(&root.to_string_lossy()).into_owned()
    }

    #[test]
    fn the_settings_of_a_project_are_found_from_inside_it_and_the_command_line_still_wins() {
        let project = build_test_project("found-and-overridden",
                "===> exclude\nnode_modules\n\n===> languages\nrust,java\n\n===> top\n7\n");

        let found = create_config_from_args(&format!("{project}/src")).unwrap();
        assert_eq!(vec!["node_modules".to_owned()], found.engine.exclude_dirs);
        assert_eq!(vec!["rust".to_owned(), "java".to_owned()], found.engine.languages_of_interest.to_written_form());
        assert_eq!(Some(7), found.view.top_n);
        assert_eq!(Some(project.clone() + "/.mezura/config.txt"),
                found.view.local_dir.as_ref().map(LocalDir::get_config_path));
        assert!(found.view.local_dir.is_some_and(|x| x.configuration_applied));

        let overridden = create_config_from_args(&format!("{project}/src --languages python --top 2")).unwrap();
        assert_eq!(vec!["python".to_owned()], overridden.engine.languages_of_interest.to_written_form());
        assert_eq!(Some(2), overridden.view.top_n);
        assert_eq!(vec!["node_modules".to_owned()], overridden.engine.exclude_dirs,
                "what the command line did not mention was dropped along with what it did");

        let ignored = create_config_from_args(&format!("{project}/src --{NO_LOCAL}")).unwrap();
        assert!(ignored.engine.exclude_dirs.is_empty() && ignored.view.top_n.is_none());
        assert_eq!(None, ignored.view.local_dir);

        std::fs::remove_dir_all(&project).unwrap();
    }

    // Not through a command line, which would have to name a target of its own to be inside the
    // project at all, and a target it names is the one that wins
    #[test]
    fn a_relative_target_of_a_project_names_a_place_inside_that_project() {
        let declared = vec![Target::named("web", "./src"), Target::of("api/tests"),
                Target::of("./"), Target::of("/somewhere/else")];

        assert_eq!(vec![Target::named("web", "/work/portal/src"), Target::of("/work/portal/api/tests"),
                Target::of("/work/portal"), Target::of("/somewhere/else")],
                rebase_targets_on("/work/portal", declared));
    }

    // The file travels to machines and versions it has never met, so the two halves of it are not
    // read with the same severity
    #[test]
    fn a_value_a_project_holds_that_this_build_cannot_read_stops_a_count_and_not_a_report() {
        let looks = build_test_project("presentation-mistake", "===> top\nnope\n\n===> exclude\nbuild\n");
        let counts = build_test_project("counting-mistake", "===> counting\nnope\n");

        let survived = create_config_from_args(&format!("{looks}/src")).unwrap();
        assert_eq!(None, survived.view.top_n);
        assert_eq!(vec!["build".to_owned()], survived.engine.exclude_dirs,
                "one unreadable value took the rest of the file with it");

        let stopped = create_config_from_args(&format!("{counts}/src"));
        assert_eq!(Err(ArgParsingError::InvalidValueInConfig(COUNTING.to_owned(),
                counts.clone() + "/.mezura/config.txt")), stopped);
        assert_eq!(CountingModel::Region,
                create_config_from_args(&format!("{counts}/src --{COUNTING} region")).unwrap().view.counting);

        std::fs::remove_dir_all(&looks).unwrap();
        std::fs::remove_dir_all(&counts).unwrap();
    }

    #[test]
    fn a_configuration_asked_for_by_name_leaves_the_project_out_of_the_run() {
        let project = build_test_project("named-wins", "===> exclude\nnode_modules\n");
        std::fs::create_dir_all(&crate::paths::PERSISTENT_APP_PATHS.config_dir).unwrap();
        let named = crate::paths::PERSISTENT_APP_PATHS.config_dir.clone().add("zz-named-wins.txt");
        std::fs::write(&named, "===> top\n5\n").unwrap();

        let loaded = create_config_from_args(&format!("{project}/src --{LOAD} zz-named-wins")).unwrap();
        assert_eq!(Some(5), loaded.view.top_n);
        assert!(loaded.engine.exclude_dirs.is_empty(), "the project's settings were merged under a named configuration");
        // Found all the same, since '--save-local' writes to the folder this run found whatever
        // else was asked for, and only what it holds was left unused
        assert!(loaded.view.local_dir.is_some_and(|x| !x.configuration_applied));

        std::fs::remove_file(&named).unwrap();
        std::fs::remove_dir_all(&project).unwrap();
    }

    // The folder does not exist when the run starts, so this is the first command anybody types in
    // a project: it has to leave the run holding the folder it just made, or the '--log' beside it
    // is refused for a configuration that now exists.
    // Typing nothing and typing one command that is not a target are the same run as far as the
    // targets go, and both have to reach the merge without one of their own: a target invented for
    // the first of them would beat every configuration, so a 'targets' block could never apply to
    // the commonest way of all to run this, which is to stand in a project and type the name.
    #[test]
    fn a_command_line_that_names_nothing_leaves_the_targets_to_a_configuration() {
        let typed_nothing = create_config_from_args("").unwrap();

        assert_eq!(create_config_from_args("--top 3").unwrap().engine.targets, typed_nothing.engine.targets);
        assert_eq!(vec![Target::of(mezura_core::engine::targets::convert_to_absolute("./"))],
                typed_nothing.engine.targets, "a run that named nothing stopped counting the working directory");
    }

    #[test]
    fn a_run_that_writes_the_folder_of_a_project_has_somewhere_to_log_to() {
        let project = build_test_project("save-local-and-log", "");

        let wrote = create_config_from_args(&format!("{project} --{SAVE_LOCAL} --{LOG}")).unwrap();
        assert_eq!(Some(project.clone()), wrote.view.find_project_of_the_log().map(|x| x.project_dir.clone()));
        // Nothing was counted with those settings, so nothing announces them
        assert!(wrote.view.local_dir.is_some_and(|x| !x.configuration_applied));

        std::fs::remove_dir_all(&project).unwrap();
    }

    #[test]
    fn what_is_saved_for_a_project_is_what_the_next_run_inside_it_reads() {
        let project = build_test_project("save-local", "");
        let written = format!("{project}/{}/{}", crate::paths::LOCAL_DIR_NAME, crate::paths::LOCAL_CONFIG_FILE_NAME);

        create_config_from_args(&format!("{project} --exclude build --top 4 --{SAVE_LOCAL}")).unwrap();
        assert!(std::path::Path::new(&written).exists(), "no configuration was written for the project");
        // Relative, or the file names directories that exist on one machine
        let contents = std::fs::read_to_string(&written).unwrap();
        assert!(!contents.contains(&project), "the project was written into its own configuration as an absolute path:\n{contents}");

        let read_back = create_config_from_args(&format!("{project}/src")).unwrap();
        assert_eq!(vec!["build".to_owned()], read_back.engine.exclude_dirs);
        assert_eq!(Some(4), read_back.view.top_n);

        // Saved again from a run that mentions neither, and both survive: what is saved is the
        // command line over what the project already held
        create_config_from_args(&format!("{project} --languages rust --{SAVE_LOCAL}")).unwrap();
        let after = create_config_from_args(&format!("{project}/src")).unwrap();
        assert_eq!(vec!["rust".to_owned()], after.engine.languages_of_interest.to_written_form());
        assert_eq!(vec!["build".to_owned()], after.engine.exclude_dirs);
        assert_eq!(Some(4), after.view.top_n);

        std::fs::remove_dir_all(&project).unwrap();
    }
}
