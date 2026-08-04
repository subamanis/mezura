use std::collections::HashMap;

use colored::{ColoredString, Colorize};

use super::formatted::Formatted;
use super::{message_printer, suggestions, theme::Theme};
use mezura_core::{EngineConfig, Target, Targets, Threads};
use mezura_core::engine::config::{MAX_CONSUMERS_VALUE, MAX_PRODUCERS_VALUE, MIN_CONSUMERS_VALUE, MIN_PRODUCERS_VALUE};
#[cfg(test)]
use colored::Color;

// Application version, to be displayed at startup and with --help command
pub const VERSION_ID : &str = "v3.0.0";

// command flags
pub const DIRS               :&str   = "dirs";
pub const EXCLUDE            :&str   = "exclude";
pub const LANGUAGES          :&str   = "languages";
pub const EXCLUDE_LANGUAGES  :&str   = "exclude-languages";
pub const FORCE_LANG         :&str   = "force-lang";
pub const THREADS            :&str   = "threads";
pub const BRACES_AS_CODE     :&str   = "braces-as-code";
pub const SEARCH_IN_DOTTED   :&str   = "search-in-dotted";
pub const SHOW_FAULTY_FILES  :&str   = "show-faulty-files";
pub const HIDE               :&str   = "hide";
pub const NO_GITIGNORE       :&str   = "no-gitignore";
pub const THEME              :&str   = "theme";
pub const STYLE              :&str   = "style";
pub const BAR_THICKNESS      :&str   = "bar-thickness";
pub const LAYOUT             :&str   = "layout";
pub const OUTPUT             :&str   = "output";
pub const NUMBER_SEPARATOR   :&str   = "number-separator";
pub const DECIMAL_SEPARATOR  :&str   = "decimal-separator";
pub const SORT               :&str   = "sort";
pub const TOP                :&str   = "top";
pub const LOG                :&str   = "log";
pub const COMPRARE_LEVEL     :&str   = "compare";
pub const SAVE               :&str   = "save";
pub const SAVE_THEME         :&str   = "save-theme";
pub const LOAD               :&str   = "load";
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

// What the always-loaded configuration is called in a message about it
const DEFAULT_CONFIG_LABEL  : &str    = "default";







// Two halves and not one flat struct, because the two are asked different questions. The engine is
// handed only what can change a number; the presentation is handed everything, since echoing what
// the counting was done with is part of its job.
//
// The command line and the configuration file stay flat, because the distinction is ours and not the
// user's. Only 'build' knows that one flag can answer both questions, which is what '--hide keywords'
// does: it is 'count_keywords' to the engine and 'hidden' to the view.
#[derive(Debug,PartialEq,Clone,Default)]
pub struct Configuration {
    pub engine: EngineConfig,
    pub view: ViewConfig
}


// Everything that decides how the answer is shown, saved and logged. The engine never sees it.
#[derive(Debug,PartialEq,Clone)]
pub struct ViewConfig {
    pub version: &'static str,
    pub should_show_faulty_files: bool,
    pub hidden: Hidden,
    pub log: LogOption,
    pub compare_level: usize,
    pub config_name_to_save: Option<String>,
    pub config_name_to_load: Option<String>,
    pub theme_name_to_save: Option<String>,
    pub bar_thickness: BarThickness,
    pub layout: Layout,
    pub output: OutputFormat,
    pub number_separator: NumberSeparator,
    pub decimal_separator: DecimalSeparator,
    pub sort_by: SortCriterion,
    pub top_n: Option<usize>,
    pub theme: Theme
}

// A hide list and not a show list: a show list would have to be re-enumerated every time a section
// is added, and a configuration saved today would silently keep hiding it.
// The list mixes whole sections with parts of them on purpose, because the user is pointing at what
// they see and not at how the program is structured.
#[derive(Debug,PartialEq,Eq,Clone,Copy,Default)]
pub struct Hidden {
    pub version: bool,
    pub directory_info: bool,
    pub parsing_info: bool,
    pub keywords: bool,
    pub overview: bool,
    pub bar: bool,
    pub progress: bool,
    pub timing: bool
}

impl Hidden {
    fn pairs(self) -> [(&'static str, bool); 8] {
        [("version", self.version), ("directory-info", self.directory_info), ("parsing-info", self.parsing_info),
         ("keywords", self.keywords), ("overview", self.overview), ("bar", self.bar),
         ("progress", self.progress), ("timing", self.timing)]
    }

    // Returns the unrecognised name, so that the error can say which one it was
    pub fn parse(value: &str) -> Result<Hidden, String> {
        let mut hidden = Hidden::default();
        for entry in value.split([',', ' ', '\t']).map(str::trim).filter(|x| !x.is_empty()) {
            match entry.to_lowercase().as_str() {
                "version" => hidden.version = true,
                "directory-info" => hidden.directory_info = true,
                "parsing-info" => hidden.parsing_info = true,
                "keywords" => hidden.keywords = true,
                "overview" => hidden.overview = true,
                "bar" => hidden.bar = true,
                "progress" => hidden.progress = true,
                "timing" => hidden.timing = true,
                _ => return Err(entry.to_owned())
            }
        }

        Ok(hidden)
    }

    pub fn to_list_string(self) -> String {
        self.pairs().iter().filter(|(_,is_hidden)| *is_hidden).map(|(name,_)| *name).collect::<Vec<_>>().join(",")
    }

    pub fn names() -> String {
        Hidden::default().pairs().iter().map(|(name,_)| *name).collect::<Vec<_>>().join(", ")
    }
}

#[derive(Debug,PartialEq,Eq,Clone,Copy,Default)]
pub enum SortCriterion {
    Files,
    #[default]
    Lines,
    Code,
    Size,
    Name
}

impl SortCriterion {
    pub fn parse(value: &str) -> Option<SortCriterion> {
        match value.trim().to_lowercase().as_str() {
            "files" => Some(Self::Files),
            "lines" => Some(Self::Lines),
            "code" => Some(Self::Code),
            "size" => Some(Self::Size),
            "name" => Some(Self::Name),
            _ => None
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Lines => "lines",
            Self::Code => "code",
            Self::Size => "size",
            Self::Name => "name"
        }
    }
}

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
    pub fn character(&self) -> &'static str {
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

// 'compact' was specified once and dropped: it was "one line per language with no blank lines",
// which is what 'table' already is, and aligned as well.
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
    pub fn character(&self) -> Option<char> {
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
    pub fn character(&self) -> char {
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


#[derive(Debug, PartialEq)]
pub enum ArgParsingError {
    UnparsableWorkingDir,
    InvalidPath(String),
    InvalidPathInConfig(String,String),
    DoublePath,
    UnrecognisedCommand(String),
    IncorrectCommandArgs(String),
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
    ContestedTarget(String, String, String)
}

// Empty line argument is not supposed to be allowed, since this check is being performed in main
pub fn create_config_from_args(line: &str) -> Result<Configuration, ArgParsingError> {
    let config = create_config_builder_from_args(line)?.build();

    // Written from the resolved theme and therefore after it is built, which is also why this does
    // not sit next to '--save': what the file has to hold is the look, not the pieces it came from
    if let Some(name) = &config.view.theme_name_to_save {
        if config.view.theme == Theme::default() {
            eprintln!("\n{}", format!("Nothing to save in theme '{name}': every style is at its default.").yellow());
        } else {
            match super::theme_files::save_theme_to_file(&crate::paths::PERSISTENT_APP_PATHS.themes_dir, name, &config.view.theme) {
                Err(_) => eprintln!("\n{}","Error while trying to save the theme.".yellow()),
                Ok(_) => eprintln!("\nTheme '{name}' saved successfully.")
            }
        }
    }

    Ok(config)
}

// The form that reads back as this exact target, which is the syntax 'parse_dirs' below accepts. The
// quotes go around the path and not around the whole thing, because the name is taken from before the
// first '=' and a leading quote would end up inside it.
pub fn declared_form(target: &Target) -> String {
    let path = if target.path.contains(char::is_whitespace) {format!("\"{}\"", target.path)} else {target.path.clone()};
    match &target.module {
        Some(name) => format!("{name}={path}"),
        None => path
    }
}

// Targets on one line, for the log entry that decides whether two runs are comparable.
//
// The separator is a comma while nothing is named, which is the only thing this ever wrote and what
// keeps a run after an upgrade from reporting 'modified: dirs' over a difference in punctuation. The
// moment a module exists it has to be whitespace: inside a comma list a name carries on to the paths
// after it, so 'frontend=./web,./ui' is one module of two directories, and an unnamed target written
// after a named one with a comma between them would be read back as part of it.
pub fn targets_to_string(targets: &[Target]) -> String {
    if targets.iter().all(|x| x.module.is_none()) {
        targets.iter().map(|x| x.path.clone()).collect::<Vec<_>>().join(",")
    } else {
        targets.iter().map(declared_form).collect::<Vec<_>>().join(" ")
    }
}

pub fn create_config_builder_from_args(line: &str) -> Result<ConfigurationBuilder, ArgParsingError> {
    let mut dirs = None;
    let mut options = super::args::split_into_command_segments(line).into_iter();

    if line.trim().starts_with("--") {
        //ignoring the empty first element that is caused by splitting
        options.next();
    } else {
        match parse_dirs(options.next().unwrap()) {
            Ok(x) => {
                if !x.is_empty() {
                    dirs = Some(x);
                }
            },
            Err(x) => {
                return Err(x);
            }
        }
    }

    let mut custom_config = None;
    let (mut exclude_dirs, mut languages_of_interest, mut excluded_languages, mut forced_languages, mut threads, mut braces_as_code,
         mut search_in_dotted, mut show_faulty_files, mut config_name_to_save, mut hidden, mut log,
         mut compare_level, mut config_name_to_load, mut no_gitignore, mut theme_name, mut theme_name_to_save, mut styles, mut bar_thickness,
         mut number_separator, mut decimal_separator, mut layout, mut output, mut sort_by, mut top_n)
         = (None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None);
    for command in options {
        let (command_name, arguments) = match command.find(" ") {
            Some(index) => command.split_at(index),
            None => (command.trim(), "")
        };
        if command_name == DIRS {
            if dirs.is_some() {
                return Err(ArgParsingError::DoublePath);
            }

            let parse_result = parse_dirs(arguments);
            if let Ok(x) = parse_result {
                if x.is_empty() {
                    message_printer::print_help_message_for_command(DIRS);
                    return Err(ArgParsingError::IncorrectCommandArgs(DIRS.to_owned()));
                }
                dirs = Some(x)
            } else {
                return Err(parse_result.err().unwrap());
            }
        } else if command_name == EXCLUDE {
            let vec = super::args::parse_paths_to_vec(arguments);
            if vec.is_empty() || mezura_core::engine::targets::validate_exclude_patterns(&vec).is_err() {
                message_printer::print_help_message_for_command(EXCLUDE);
                return Err(ArgParsingError::IncorrectCommandArgs(EXCLUDE.to_owned()));
            }
            exclude_dirs = Some(vec);
        } else if command_name == LANGUAGES {
            let vec = super::args::parse_languages_to_vec(arguments);
            if vec.is_empty() {
                message_printer::print_help_message_for_command(LANGUAGES);
                return Err(ArgParsingError::IncorrectCommandArgs(LANGUAGES.to_owned()));
            }
            languages_of_interest = Some(vec);
        } else if command_name == EXCLUDE_LANGUAGES {
            let vec = super::args::parse_languages_to_vec(arguments);
            if vec.is_empty() {
                message_printer::print_help_message_for_command(EXCLUDE_LANGUAGES);
                return Err(ArgParsingError::IncorrectCommandArgs(EXCLUDE_LANGUAGES.to_owned()));
            }
            excluded_languages = Some(vec);
        } else if command_name == FORCE_LANG {
            let Some(map) = super::args::parse_forced_languages(arguments) else {
                message_printer::print_help_message_for_command(FORCE_LANG);
                return Err(ArgParsingError::IncorrectCommandArgs(FORCE_LANG.to_owned()));
            };
            forced_languages = Some(map);
        } else if command_name == THREADS {
            let threads_values = super::args::parse_two_usize_values(arguments,
                    MIN_PRODUCERS_VALUE, MAX_PRODUCERS_VALUE, MIN_CONSUMERS_VALUE, MAX_CONSUMERS_VALUE);
            if let Some(_threads) = threads_values {
                threads = Some(Threads::from(_threads));
            } else {
                message_printer::print_help_message_for_command(THREADS);
                return Err(ArgParsingError::IncorrectCommandArgs(THREADS.to_owned()))
            }
        } else if command_name == BRACES_AS_CODE {
            if has_any_args(command) {
                message_printer::print_help_message_for_command(BRACES_AS_CODE);
                return Err(ArgParsingError::UnexpectedCommandArgs(BRACES_AS_CODE.to_owned()))
            }
            braces_as_code = Some(true)
        } else if command_name == SEARCH_IN_DOTTED {
            if has_any_args(command) {
                message_printer::print_help_message_for_command(SEARCH_IN_DOTTED);
                return Err(ArgParsingError::UnexpectedCommandArgs(SEARCH_IN_DOTTED.to_owned()))
            }
            search_in_dotted = Some(true)
        } else if command_name == SHOW_FAULTY_FILES {
            if has_any_args(command) {
                message_printer::print_help_message_for_command(SHOW_FAULTY_FILES);
                return Err(ArgParsingError::UnexpectedCommandArgs(SHOW_FAULTY_FILES.to_owned()))
            }
            show_faulty_files = Some(true);
        } else if command_name == HIDE {
            if arguments.trim().is_empty() {
                message_printer::print_help_message_for_command(HIDE);
                return Err(ArgParsingError::IncorrectCommandArgs(HIDE.to_owned()))
            }
            match Hidden::parse(arguments) {
                Ok(x) => hidden = Some(x),
                Err(x) => {
                    message_printer::print_help_message_for_command(HIDE);
                    return Err(ArgParsingError::InvalidHideTarget(x))
                }
            }
        } else if command_name == NO_GITIGNORE {
            if has_any_args(command) {
                message_printer::print_help_message_for_command(NO_GITIGNORE);
                return Err(ArgParsingError::UnexpectedCommandArgs(NO_GITIGNORE.to_owned()))
            }
            no_gitignore = Some(true);
        } else if command_name == THEME {
            let name = arguments.trim();
            if name.is_empty() {
                message_printer::print_help_message_for_command(THEME);
                return Err(ArgParsingError::IncorrectCommandArgs(THEME.to_owned()))
            }
            if super::theme_files::load_theme(name, &crate::paths::PERSISTENT_APP_PATHS.themes_dir).is_none() {
                return Err(ArgParsingError::NonExistantTheme(name.to_owned()))
            }
            theme_name = Some(name.to_owned());
        } else if command_name == STYLE {
            match super::theme::parse_overrides(arguments) {
                Ok(x) => styles = Some(x),
                Err(x) => {
                    message_printer::print_help_message_for_command(STYLE);
                    return Err(ArgParsingError::InvalidStyle(x.formatted()))
                }
            }
        } else if command_name == TOP {
            match super::args::parse_usize_value(arguments, 1, usize::MAX) {
                Some(x) => top_n = Some(x),
                None => {
                    message_printer::print_help_message_for_command(TOP);
                    return Err(ArgParsingError::IncorrectCommandArgs(TOP.to_owned()))
                }
            }
        } else if command_name == SORT {
            match SortCriterion::parse(arguments) {
                Some(x) => sort_by = Some(x),
                None => {
                    message_printer::print_help_message_for_command(SORT);
                    return Err(ArgParsingError::IncorrectCommandArgs(SORT.to_owned()))
                }
            }
        } else if command_name == BAR_THICKNESS {
            match BarThickness::parse(arguments) {
                Some(x) => bar_thickness = Some(x),
                None => {
                    message_printer::print_help_message_for_command(BAR_THICKNESS);
                    return Err(ArgParsingError::IncorrectCommandArgs(BAR_THICKNESS.to_owned()))
                }
            }
        } else if command_name == LAYOUT {
            match Layout::parse(arguments) {
                Some(x) => layout = Some(x),
                None => {
                    message_printer::print_help_message_for_command(LAYOUT);
                    return Err(ArgParsingError::IncorrectCommandArgs(LAYOUT.to_owned()))
                }
            }
        } else if command_name == OUTPUT {
            match OutputFormat::parse(arguments) {
                Some(x) => output = Some(x),
                None => {
                    message_printer::print_help_message_for_command(OUTPUT);
                    return Err(ArgParsingError::IncorrectCommandArgs(OUTPUT.to_owned()))
                }
            }
        } else if command_name == NUMBER_SEPARATOR {
            match NumberSeparator::parse(arguments) {
                Some(x) => number_separator = Some(x),
                None => {
                    message_printer::print_help_message_for_command(NUMBER_SEPARATOR);
                    return Err(ArgParsingError::IncorrectCommandArgs(NUMBER_SEPARATOR.to_owned()))
                }
            }
        } else if command_name == DECIMAL_SEPARATOR {
            match DecimalSeparator::parse(arguments) {
                Some(x) => decimal_separator = Some(x),
                None => {
                    message_printer::print_help_message_for_command(DECIMAL_SEPARATOR);
                    return Err(ArgParsingError::IncorrectCommandArgs(DECIMAL_SEPARATOR.to_owned()))
                }
            }
        } else if command_name == LOG {
            let value = arguments.trim();
            if value.is_empty() {
                log = Some(LogOption::new(None));
            } else {
                log = Some(LogOption::new(Some(value.to_owned())));
            }
        } else if command_name == COMPRARE_LEVEL {
            let compare_num = super::args::parse_usize_value(arguments, MIN_COMPARE_LEVEL, MAX_COMPARE_LEVEL);
            if compare_num.is_none() {
                message_printer::print_help_message_for_command(COMPRARE_LEVEL);
                return Err(ArgParsingError::IncorrectCommandArgs(COMPRARE_LEVEL.to_owned()))
            } else {
                compare_level = compare_num
            }
        } else if command_name == LOAD {
            let config_name = arguments.trim();
            if config_name.is_empty() {
                message_printer::print_help_message_for_command(LOAD);
                return Err(ArgParsingError::IncorrectCommandArgs(LOAD.to_owned()));
            }

            match super::config_files::parse_config_file(Some(config_name), None) {
                Ok((options, issues)) => {
                    custom_config = Some((options, issues));
                    config_name_to_load = Some(config_name.to_owned());
                },
                // The file is there, it just cannot be read whole; calling it missing sends the
                // user looking for a typo in the name instead of at the file's encoding
                Err(super::config_files::ConfigFileParseError::UnreadableLine(file, line, cause)) =>
                    return Err(ArgParsingError::UnreadableConfig(file, line, cause)),
                Err(_) => return Err(ArgParsingError::NonExistantConfig(config_name.to_owned()))
            }
        } else if command_name == SAVE {
            let name = arguments.trim();
            if name.is_empty() {
                message_printer::print_help_message_for_command(SAVE);
                return Err(ArgParsingError::IncorrectCommandArgs(SAVE.to_owned()))
            }
            config_name_to_save = Some(name.to_owned());
        } else if command_name == SAVE_THEME {
            let name = arguments.trim();
            if name.is_empty() {
                message_printer::print_help_message_for_command(SAVE_THEME);
                return Err(ArgParsingError::IncorrectCommandArgs(SAVE_THEME.to_owned()))
            }
            theme_name_to_save = Some(name.to_owned());
        } else {
            return Err(ArgParsingError::UnrecognisedCommand(command_name.to_owned()));
        }
    }

    print_warnings_for_commands_that_need_a_loaded_configuration(&config_name_to_save, &config_name_to_load, &log, &compare_level);

    let mut config_builder = ConfigurationBuilder {
        dirs, resolved_dirs: None, exclude_dirs, languages_of_interest, excluded_languages, forced_languages, threads, braces_as_code,
        should_search_in_dotted: search_in_dotted, should_show_faulty_files: show_faulty_files,
        hidden, no_gitignore, theme_name, theme_name_to_save, log, compare_level,
        config_name_to_save, config_name_to_load, styles, bar_thickness, number_separator, decimal_separator, layout, output, sort_by, top_n,
        config_styles: None, theme_styles: None
    };

    let mut dirs_config_source = None;
    if let Some((custom, issues)) = custom_config {
        let config_name = config_builder.config_name_to_load.clone().unwrap_or_default();
        print_config_file_warnings(&issues.warnings, &config_name);
        resolve_invalid_config_fields(&config_builder, &issues.invalid_fields, &config_name)?;
        let dirs_were_missing = config_builder.dirs.is_none();
        config_builder.add_missing_fields(custom);
        if dirs_were_missing && config_builder.dirs.is_some() {
            dirs_config_source = Some(config_name);
        }
    }

    if let Some(name) = &config_builder.config_name_to_save {
        if config_builder.dirs.is_none() {
            config_builder.dirs = Some(working_dir_as_targets()?.to_vec());
        }

        match super::config_files::save_existing_commands_from_config_builder_to_file(None, name, &config_builder) {
            Err(_) => eprintln!("\n{}","Error while trying to save config.".yellow()),
            Ok(_) => eprintln!("\nConfiguration '{name}' saved successfully.")
        }
    }

    if config_builder.has_missing_fields() {
        match super::config_files::parse_config_file(None, None) {
            Ok((default_config, issues)) => {
                print_config_file_warnings(&issues.warnings, DEFAULT_CONFIG_LABEL);
                resolve_invalid_config_fields(&config_builder, &issues.invalid_fields, DEFAULT_CONFIG_LABEL)?;
                let dirs_were_missing = config_builder.dirs.is_none();
                config_builder.add_missing_fields(default_config);
                if dirs_were_missing && config_builder.dirs.is_some() {
                    dirs_config_source = Some(DEFAULT_CONFIG_LABEL.to_owned());
                }
            },
            // An absent default configuration is an ordinary machine. A half-readable one is not,
            // and skipping it in silence would run with whatever defaults it no longer supplies.
            Err(super::config_files::ConfigFileParseError::UnreadableLine(file, line, cause)) =>
                return Err(ArgParsingError::UnreadableConfig(file, line, cause)),
            Err(_) => {}
        }
    }

    // One resolution, after every source has spoken. The two settings that decide which of a
    // pattern's matches survive are read from the merged configuration, so the walk and the
    // resolution cannot answer to different flags. Resolving per source is the bug this replaced:
    // the flags were scanned off the raw command line before anything else was known, so a
    // configuration declaring 'no-gitignore' beside its own glob died over matches the walk was
    // about to count. A mistake in a configuration's dirs still names the configuration, and a
    // 'dirs' block sitting unused in one is still never resolved at all, because it never enters
    // the builder.
    if let Some(dirs) = &config_builder.dirs {
        let engine_defaults = EngineConfig::default();
        let respect_gitignore = !config_builder.no_gitignore.unwrap_or(engine_defaults.no_gitignore);
        let dotted_are_targetable = config_builder.should_search_in_dotted
                .unwrap_or(engine_defaults.should_search_in_dotted);
        match reparse_targets(dirs, respect_gitignore, dotted_are_targetable) {
            Ok(x) => config_builder.resolved_dirs = Some(x),
            Err(x) => return Err(match (x, dirs_config_source) {
                (ArgParsingError::InvalidPath(p), Some(name)) | (ArgParsingError::InvalidGlobPattern(p), Some(name))
                | (ArgParsingError::NoGlobMatches(p), Some(name)) | (ArgParsingError::AllGlobMatchesIgnored(p), Some(name)) =>
                        ArgParsingError::InvalidPathInConfig(p, name),
                (other, _) => other
            })
        }
    }

    if let Some(name) = &config_builder.theme_name {
        match super::theme_files::load_theme(name, &crate::paths::PERSISTENT_APP_PATHS.themes_dir) {
            Some((styles, errors)) => {
                for error in &errors {
                    super::warnings::emit(mezura_core::warnings::Warning::new(mezura_core::warnings::CONFIG_STYLE_INVALID, mezura_core::warnings::Affects::Settings, name,
                            format!("In theme '{name}': {}", error.formatted())));
                }
                config_builder.theme_styles = Some(styles);
            },
            None => super::warnings::emit(mezura_core::warnings::Warning::new(mezura_core::warnings::THEME_UNAVAILABLE, mezura_core::warnings::Affects::Settings, name,
                    format!("Theme '{name}' could not be loaded, the default styles will be used.")))
        }
    }

    if config_builder.resolved_dirs.is_none() {
        config_builder.resolved_dirs = Some(working_dir_as_targets()?);
    }

    Ok(config_builder)
}



fn print_config_file_warnings(issues: &[(&'static str, String)], config_name: &str) {
    for (code, warning) in issues {
        super::warnings::emit(mezura_core::warnings::Warning::new(code, mezura_core::warnings::Affects::Settings, config_name,
                format!("In config '{config_name}': {warning}")));
    }
}

// Every command that can end up in 'invalid_fields' belongs here. One that is missing is treated as
// never overridden, so giving it correctly on the command line would still not rescue the run.
fn resolve_invalid_config_fields(config_builder: &ConfigurationBuilder, invalid_fields: &[&str], config_name: &str) -> Result<(), ArgParsingError> {
    // Destructured with no '..' on purpose. This function decides whether a bad value in a config
    // only warns or kills the run, by asking whether the command line already set the same field,
    // and the match below ends in '_ => false', so a command missing from it counts as never
    // overridden and giving it correctly on the command line does not rescue the run. 'sort', 'top'
    // and 'bar-thickness' were all missing until v3.0.0 with nothing to point at the omission.
    //
    // Every command a configuration file can carry is a field here, so a new one stops the build in
    // this spot until somebody decides whether it belongs in the match. Everything bound to '_'
    // below is a written decision, with the reason next to it.
    let ConfigurationBuilder {
            dirs, exclude_dirs, forced_languages, threads, braces_as_code, should_search_in_dotted,
            should_show_faulty_files, hidden, no_gitignore, theme_name, compare_level, bar_thickness,
            number_separator, decimal_separator, layout, sort_by, top_n,
            // these three accept whatever they are given, so a config can hold no invalid value for
            // them and they never reach 'invalid_fields'
            languages_of_interest: _, excluded_languages: _, log: _,
            // not carried by a configuration file at all
            config_name_to_save: _, config_name_to_load: _, theme_name_to_save: _, output: _, resolved_dirs: _,
            // a style that does not parse is reported per line and skipped, and the rest of the file
            // still applies, so these warn instead of reaching here
            styles: _, config_styles: _, theme_styles: _ } = config_builder;

    for field in invalid_fields {
        let is_overridden = match *field {
            DIRS => dirs.is_some(),
            THREADS => threads.is_some(),
            COMPRARE_LEVEL => compare_level.is_some(),
            BRACES_AS_CODE => braces_as_code.is_some(),
            SEARCH_IN_DOTTED => should_search_in_dotted.is_some(),
            SHOW_FAULTY_FILES => should_show_faulty_files.is_some(),
            HIDE => hidden.is_some(),
            NO_GITIGNORE => no_gitignore.is_some(),
            EXCLUDE => exclude_dirs.is_some(),
            FORCE_LANG => forced_languages.is_some(),
            THEME => theme_name.is_some(),
            SORT => sort_by.is_some(),
            TOP => top_n.is_some(),
            BAR_THICKNESS => bar_thickness.is_some(),
            NUMBER_SEPARATOR => number_separator.is_some(),
            DECIMAL_SEPARATOR => decimal_separator.is_some(),
            LAYOUT => layout.is_some(),
            _ => false
        };

        if is_overridden {
            super::warnings::emit(mezura_core::warnings::Warning::new(mezura_core::warnings::CONFIG_VALUE_IGNORED, mezura_core::warnings::Affects::Settings, field,
                    format!("Invalid value for the command '--{field}', in config '{config_name}'. The value will be ignored.")));
        } else {
            message_printer::print_help_message_for_command(field);
            return Err(ArgParsingError::InvalidValueInConfig(field.to_string(), config_name.to_owned()));
        }
    }

    Ok(())
}

fn print_warnings_for_commands_that_need_a_loaded_configuration(config_name_to_save: &Option<String>, config_name_to_load: &Option<String>,
        log: &Option<LogOption>, compare_level: &Option<usize>)
{
    if config_name_to_load.is_none() {
        if let Some(log) = log && config_name_to_save.is_none() && log.should_log {
            eprintln!("\n{}","'--log' command will be ignored, since no config file was specified.".yellow());
        }

        if compare_level.is_some() {
            eprintln!("\n{}","'--compare' command will be ignored, since no config file was specified for loading.".yellow());
        }
    }
}

fn has_any_args(command: &str) -> bool {
    command.split(' ').skip(1).filter_map(super::args::get_trimmed_if_not_empty).count() != 0
}

// Only the half of resolution that no setting can change: patterns are expanded once, at the end of
// 'create_config_builder_from_args', when every source of the run's settings has been merged.
fn parse_dirs(s: &str) -> Result<Vec<Target>, ArgParsingError> {
    let declared = super::args::parse_targets(s).map_err(ArgParsingError::MalformedTarget)?;
    mezura_core::engine::targets::prepare(&declared).map_err(map_target_error)
}

// A target that a configuration file declared, or that a run has already resolved once, is read back
// through the same parser as a command line, so that the name and the path cannot be taken apart in
// two different ways
fn reparse_targets(targets: &[Target], respect_gitignore: bool, search_in_dotted: bool)
-> Result<Targets, ArgParsingError>
{
    let declared = targets.iter().map(|x| (x.module.clone(), x.path.clone())).collect::<Vec<_>>();
    mezura_core::engine::targets::resolve(&declared, respect_gitignore, search_in_dotted).map_err(map_target_error)
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


// The working directory is not something anybody typed, so it goes through 'literal' and never
// through the parser that takes typed text apart. It used to, and a working directory whose path
// contains a space was then split into two targets, neither of which existed, which stopped a bare
// 'mezura' from running at all in a place like 'C:/Users/John Smith/project'.
fn working_dir_as_targets() -> Result<Targets, ArgParsingError> {
    if let Ok(path_buf) = std::env::current_dir()
        && let Some(path_str) = path_buf.to_str()
        && let Ok(targets) = Targets::literal(&[path_str]) {
        return Ok(targets);
    }

    Err(ArgParsingError::UnparsableWorkingDir)
}



#[derive(Debug, PartialEq, Default)]
pub struct ConfigurationBuilder {
    pub dirs:                     Option<Vec<Target>>,
    // What the run is actually given: the declared list above, put through the one resolution in
    // 'create_config_builder_from_args'. Deliberately absent from 'add_missing_fields', because it
    // exists only after every source has been merged.
    pub resolved_dirs:            Option<Targets>,
    pub exclude_dirs:             Option<Vec<String>>,
    pub languages_of_interest:    Option<Vec<String>>,
    pub excluded_languages:       Option<Vec<String>>,
    pub forced_languages:         Option<HashMap<String,String>>,
    pub threads:                  Option<Threads>,
    pub braces_as_code:           Option<bool>,
    pub should_search_in_dotted:  Option<bool>,
    pub should_show_faulty_files: Option<bool>,
    pub hidden:                   Option<Hidden>,
    pub no_gitignore:             Option<bool>,
    pub theme_name:               Option<String>,
    pub log:                      Option<LogOption>,
    pub compare_level:            Option<usize>,
    pub config_name_to_save:      Option<String>,
    pub config_name_to_load:      Option<String>,
    pub theme_name_to_save:       Option<String>,
    pub bar_thickness:            Option<BarThickness>,
    pub number_separator:         Option<NumberSeparator>,
    pub decimal_separator:        Option<DecimalSeparator>,
    pub layout:                   Option<Layout>,
    // Absent from 'add_missing_fields' and 'has_missing_fields' on purpose, like the save and load
    // names: those two functions exist for what a configuration file can supply, and this is not it
    pub output:                   Option<OutputFormat>,
    pub sort_by:                  Option<SortCriterion>,
    pub top_n:                    Option<usize>,
    pub styles:                   Option<Vec<(String,String)>>,
    pub config_styles:            Option<Vec<(String,String)>>,
    pub theme_styles:             Option<Vec<(String,String)>>
}

impl ConfigurationBuilder {
    pub fn add_missing_fields(&mut self, config: Self) -> &mut Self {
        if self.dirs.is_none() {self.dirs = config.dirs};
        if self.exclude_dirs.is_none() {self.exclude_dirs = config.exclude_dirs};
        if self.languages_of_interest.is_none() {self.languages_of_interest = config.languages_of_interest};
        if self.excluded_languages.is_none() {self.excluded_languages = config.excluded_languages};
        if self.forced_languages.is_none() {self.forced_languages = config.forced_languages};
        if self.threads.is_none() {self.threads = config.threads};
        if self.braces_as_code.is_none() {self.braces_as_code = config.braces_as_code};
        if self.should_search_in_dotted.is_none() {self.should_search_in_dotted = config.should_search_in_dotted};
        if self.should_show_faulty_files.is_none() {self.should_show_faulty_files = config.should_show_faulty_files};
        if self.hidden.is_none() {self.hidden = config.hidden};
        if self.no_gitignore.is_none() {self.no_gitignore = config.no_gitignore};
        if self.theme_name.is_none() {self.theme_name = config.theme_name};
        if self.compare_level.is_none() {self.compare_level = config.compare_level};
        if self.log.is_none() {self.log = config.log};
        if self.config_styles.is_none() {self.config_styles = config.config_styles};
        if self.bar_thickness.is_none() {self.bar_thickness = config.bar_thickness};
        if self.number_separator.is_none() {self.number_separator = config.number_separator};
        if self.decimal_separator.is_none() {self.decimal_separator = config.decimal_separator};
        if self.layout.is_none() {self.layout = config.layout};
        if self.sort_by.is_none() {self.sort_by = config.sort_by};
        if self.top_n.is_none() {self.top_n = config.top_n};
        self
    }

    pub fn has_missing_fields(&self) -> bool {
        self.exclude_dirs.is_none() || self.languages_of_interest.is_none() || self.forced_languages.is_none() ||
        self.threads.is_none() || self.braces_as_code.is_none() || self.should_search_in_dotted.is_none() ||
        self.should_show_faulty_files.is_none() || self.hidden.is_none() || self.no_gitignore.is_none() ||
        self.theme_name.is_none() || self.log.is_none() || self.compare_level.is_none() ||
        self.config_styles.is_none() || self.bar_thickness.is_none() || self.number_separator.is_none() || self.decimal_separator.is_none() || self.layout.is_none() || self.sort_by.is_none()
    }

    // The only place that knows the flat form maps onto two halves. Everything above this stays one
    // list, matching the command line and the configuration file.
    pub fn build(&self) -> Configuration {
        let hidden = self.hidden.unwrap_or_default();
        // Asked of the engine rather than read from constants of its own, so the help text and the
        // behaviour cannot answer differently. The literal below stays exhaustive on purpose: a new
        // field of EngineConfig has to be decided here and not inherited silently.
        let engine_defaults = EngineConfig::default();

        Configuration {
            engine: EngineConfig {
                dirs: self.resolved_dirs.clone().unwrap(),
                exclude_dirs: (self.exclude_dirs).clone().unwrap_or_default(),
                languages_of_interest: (self.languages_of_interest).clone().unwrap_or_default(),
                excluded_languages: (self.excluded_languages).clone().unwrap_or_default(),
                forced_languages: (self.forced_languages).clone().unwrap_or_default(),
                threads: self.threads.clone().unwrap_or_default(),
                braces_as_code: self.braces_as_code.unwrap_or(engine_defaults.braces_as_code),
                should_search_in_dotted: self.should_search_in_dotted.unwrap_or(engine_defaults.should_search_in_dotted),
                no_gitignore: self.no_gitignore.unwrap_or(engine_defaults.no_gitignore),
                // The one flag that answers both questions
                count_keywords: !hidden.keywords
            },
            view: ViewConfig {
                version: VERSION_ID,
                should_show_faulty_files: self.should_show_faulty_files.unwrap_or(DEF_SHOW_FAULTY_FILES),
                hidden,
                log: self.log.clone().unwrap_or_default(),
                compare_level: self.compare_level.unwrap_or(DEF_COMPARE_LEVEL),
                config_name_to_save: self.config_name_to_save.clone(),
                config_name_to_load: self.config_name_to_load.clone(),
                theme_name_to_save: self.theme_name_to_save.clone(),
                bar_thickness: self.bar_thickness.unwrap_or_default(),
                layout: self.layout.unwrap_or_default(),
                output: self.output.unwrap_or_default(),
                number_separator: self.number_separator.unwrap_or_default(),
                decimal_separator: self.decimal_separator.unwrap_or_default(),
                sort_by: self.sort_by.unwrap_or_default(),
                top_n: self.top_n,
                theme: super::theme::resolve(self.theme_styles.as_deref().unwrap_or_default(),
                        self.config_styles.as_deref().unwrap_or_default(), self.styles.as_deref().unwrap_or_default())
            }
        }
    }
}

impl Default for ViewConfig {
    fn default() -> Self {
        ViewConfig {
            version: VERSION_ID,
            should_show_faulty_files: DEF_SHOW_FAULTY_FILES,
            hidden: Hidden::default(),
            log: LogOption::default(),
            compare_level: DEF_COMPARE_LEVEL,
            config_name_to_save: None,
            config_name_to_load: None,
            theme_name_to_save: None,
            bar_thickness: BarThickness::default(),
            layout: Layout::default(),
            output: OutputFormat::default(),
            number_separator: NumberSeparator::default(),
            decimal_separator: DecimalSeparator::default(),
            sort_by: SortCriterion::default(),
            top_n: None,
            theme: Theme::default()
        }
    }
}


impl ViewConfig {
    // Everything that is not the document itself stays off stdout when the output is machine
    // readable, so that a single stray line cannot make it unparseable
    pub fn prints_text(&self) -> bool {
        self.output == OutputFormat::Text
    }

    // Used only by the tests of this crate, which is what marks them
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

impl Configuration {
    #[cfg(test)]
    pub fn new(dirs: Vec<String>) -> Self {
        let paths = dirs.iter().map(String::as_str).collect::<Vec<_>>();
        Configuration { engine: EngineConfig::new(Targets::of(&paths).unwrap()), view: ViewConfig::default() }
    }

    // One flag answering two questions, so the two halves are set together and never one without
    // the other
    #[cfg(test)]
    pub fn set_hidden(&mut self, hidden: Hidden) -> &mut Self {
        self.engine.count_keywords = !hidden.keywords;
        self.view.hidden = hidden;
        self
    }
}

impl LogOption {
    pub fn new(log_name: Option<String>) -> Self {
        LogOption {
            should_log: true,
            name: log_name,
        }
    }
}

impl Formatted for ArgParsingError {
    fn formatted(&self) -> ColoredString {
        match self {
            Self::UnparsableWorkingDir => "The current working dir could not be parsed as target dir, try inputing it manually.".red(),
            Self::InvalidPath(p) => format!("Path provided is not a valid directory or file:\n'{p}'.").red(),
            Self::InvalidPathInConfig(dir,name) => format!("Specified path '{dir}', in config '{name}', doesn't exist anymore.").red(),
            Self::DoublePath => "Directories already provided as first argument, but --dirs command also found.".red(),
            // Only the mistake is red. What to do about it is not an error, it is the way out.
            Self::UnrecognisedCommand(p) => {
                let tail = suggestions::formatted_suggestion(p, &message_printer::command_names())
                        .unwrap_or_else(|| format!("Run '--{HELP}' to see every command."));
                let error = format!("--{p} is not recognised as a command.").red();
                ColoredString::from(format!("{error}\n\n{tail}").as_str())
            },
            Self::IncorrectCommandArgs(p) => format!("Incorrect arguments provided for the command '--{p}'.").red(),
            Self::UnreadableConfig(name, line, super::config_files::UnreadableCause::NotUtf8) => format!("Configuration '{name}' stops being readable at line {line}, so none of it was used: the file is not saved as UTF-8.").red(),
            Self::UnreadableConfig(name, line, super::config_files::UnreadableCause::Io(error)) => format!("Configuration '{name}' could not be read past line {line}, so none of it was used: {error}").red(),
            Self::UnexpectedCommandArgs(p) => format!("Command '--{p}' does not expect any arguments.").red(),
            Self::NonExistantConfig(p) => {
                let names = super::config_files::names_in_dir(&crate::paths::PERSISTENT_APP_PATHS.config_dir);
                let tail = suggestions::formatted_suggestion(p, &names.iter().map(String::as_str).collect::<Vec<_>>())
                        .unwrap_or_else(|| format!("Run '--{SHOW_CONFIGS}' to see the ones you have."));
                let error = format!("Configuration '{p}' does not exist.").red();
                ColoredString::from(format!("{error}\n\n{tail}").as_str())
            },
            Self::NonExistantTheme(p) => {
                let names = super::config_files::names_in_dir(&crate::paths::PERSISTENT_APP_PATHS.themes_dir);
                let tail = suggestions::formatted_suggestion(p, &names.iter().map(String::as_str).collect::<Vec<_>>())
                        .unwrap_or_else(|| format!("Run '--{SHOW_THEMES}' to see the ones you have."));
                let error = format!("Theme '{p}' was not found, or could not be read.").red();
                ColoredString::from(format!("{error}\n\n{tail}").as_str())
            },
            Self::InvalidStyle(p) => p.clone().red(),
            Self::InvalidHideTarget(p) => format!("'{p}' is not something that can be hidden.\nThe options are: {}.", Hidden::names()).red(),
            Self::InvalidValueInConfig(cmd,conf) => format!("Invalid value for the command '--{cmd}', in config '{conf}'.\nFix the value in the config file, or override it by providing a valid '--{cmd}' argument.").red(),
            Self::InvalidGlobPattern(p) => format!("'{p}' is not a valid glob pattern.").red(),
            Self::NoGlobMatches(p) => format!("The pattern '{p}' did not match any existing directory or file.").red(),
            Self::AllGlobMatchesIgnored(p) => format!("Everything that the pattern '{p}' matched is skipped, because a .gitignore file ignores it, because it is a dotted path, or because it is a link.\nUse the '--no-gitignore' or '--search-in-dotted' commands to include it, or provide the paths explicitly.").red(),
            Self::MalformedTarget(p) => format!("'{p}' names a module with no path after it.\nA target is written as '<module>=<path>', and its paths are separated by commas: 'tests=./api/tests,./web/tests'.").red(),
            Self::ContestedTarget(path, first, second) => format!("'{path}' is declared both as '{first}' and as '{second}'.\nEvery file belongs to exactly one module, and there is no more specific of the two to decide it.").red()
        }
    }
}


#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::ops::Add;

    
    use super::*;
    use super::super::theme::Style;

    // Rendered back into the form they were declared in, so that a test reads the same way whether
    // the target was named or not
    fn parse_dirs(s: &str) -> Result<Vec<String>, ArgParsingError> {
        parse_dirs_with(s, true, false)
    }

    // Preparation and expansion composed the way 'create_config_builder_from_args' composes them:
    // the flags reach the expansion at the end now, not the parse
    fn parse_dirs_with(s: &str, respect_gitignore: bool, search_in_dotted: bool) -> Result<Vec<String>, ArgParsingError> {
        super::parse_dirs(s)
                .and_then(|targets| super::reparse_targets(&targets, respect_gitignore, search_in_dotted))
                .map(|targets| targets.iter().map(Target::to_string).collect())
    }

    fn new_conf(dir: &str) -> Configuration {
        let dirs = vec![Target::of(mezura_core::engine::targets::convert_to_absolute(dir))];
        let mut builder = ConfigurationBuilder { resolved_dirs: Some(super::reparse_targets(&dirs, true, false).unwrap()),
                dirs: Some(dirs), ..Default::default() };
        if let Ok((default_config, _)) = super::super::config_files::parse_config_file(None, None) {
            builder.add_missing_fields(default_config);
        }
        builder.build()
    }

    // The same, with one flag set by hand. The closure has to name the half it lands in, which is
    // the thing worth stating in a test of what the parsing produces.
    fn conf(dir: &str, edit: impl FnOnce(&mut Configuration)) -> Configuration {
        let mut config = new_conf(dir);
        edit(&mut config);
        config
    }

    // A command given twice keeps the value it was last given, which is what a reader of the line
    // assumes and what every shell tool does
    #[test]
    fn test_a_repeated_command_keeps_its_last_value() {
        assert_eq!(Threads::new(3, 11), create_config_from_args("./ --threads 2 10 --threads 3 11").unwrap().engine.threads);
        assert_eq!(Some(4), create_config_from_args("./ --top 9 --top 4").unwrap().view.top_n);
        assert_eq!(SortCriterion::Name, create_config_from_args("./ --sort size --sort name").unwrap().view.sort_by);
        assert_eq!(Layout::Boxed, create_config_from_args("./ --layout list --layout boxed").unwrap().view.layout);
    }

    #[test]
    fn test_cmd_arg_parsing() {
        assert_eq!(Err(ArgParsingError::InvalidPath("random".to_owned())), create_config_from_args("random"));
        assert_eq!(Err(ArgParsingError::InvalidPath("./ random".to_owned())), create_config_from_args("./ random"));
        assert_eq!(Err(ArgParsingError::InvalidPath("./ -show-faulty-files".to_owned())), create_config_from_args("--dirs ./ -show-faulty-files"));
        assert_eq!(Err(ArgParsingError::UnrecognisedCommand("random".to_owned())), create_config_from_args("--random"));
        assert_eq!(Err(ArgParsingError::UnrecognisedCommand("random".to_owned())), create_config_from_args("--dirs ./ --random"));
        assert_eq!(Err(ArgParsingError::DoublePath), create_config_from_args("./ --dirs ./"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("dirs".to_owned())), create_config_from_args("--dirs"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("dirs".to_owned())), create_config_from_args("--dirs   "));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned())), create_config_from_args("./ --threads"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned())), create_config_from_args("./ --threads 33 10"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned())), create_config_from_args("./ --threads 2 129"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned())), create_config_from_args("./ --threads 33"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned())), create_config_from_args("./ --threads A"));
        assert_eq!(Err(ArgParsingError::UnexpectedCommandArgs("show-faulty-files".to_owned())), create_config_from_args("./ --threads 1 1 --show-faulty-files 1"));
        assert_eq!(Err(ArgParsingError::UnexpectedCommandArgs("show-faulty-files".to_owned())), create_config_from_args("./ --threads 1 1 --show-faulty-files a"));
        assert_eq!(Err(ArgParsingError::UnexpectedCommandArgs("search-in-dotted".to_owned())), create_config_from_args("./ --threads 1 1 --search-in-dotted a"));
        assert_eq!(Err(ArgParsingError::UnexpectedCommandArgs("braces-as-code".to_owned())), create_config_from_args("./ --braces-as-code a"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("exclude".to_owned())), create_config_from_args("./ --exclude"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("exclude".to_owned())), create_config_from_args("./ --exclude   --threads 4"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("exclude".to_owned())), create_config_from_args("./ --exclude [invalid"));
        assert_eq!(Err(ArgParsingError::UnexpectedCommandArgs("no-gitignore".to_owned())), create_config_from_args("./ --no-gitignore a"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("load".to_owned())), create_config_from_args("./ --load"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("load".to_owned())), create_config_from_args("./ --load   "));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("save".to_owned())), create_config_from_args("./ --save"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("save".to_owned())), create_config_from_args("./ --save   "));

        assert_ne!(new_conf("../"), create_config_from_args(std::env::current_dir().unwrap().to_str().unwrap()).unwrap());
        assert_eq!(new_conf("./"), create_config_from_args(std::env::current_dir().unwrap().to_str().unwrap()).unwrap());

        assert_eq!(new_conf("./"), create_config_from_args("./").unwrap());
        assert_eq!(new_conf("./"), create_config_from_args("--dirs ./").unwrap());
        assert_eq!(conf("./", |c| {c.engine.set_threads(1,1);}), create_config_from_args("./ --threads 1 1").unwrap());
        assert_eq!(conf("./", |c| {c.engine.set_threads(1,1);}), create_config_from_args("./ --threads   1   1 ").unwrap());
        assert_eq!(conf("./", |c| {c.engine.set_threads(1,1).set_braces_as_code(true);}),
                create_config_from_args("./ --threads 1 1 --braces-as-code").unwrap());
        assert_eq!(conf("./", |c| {c.engine.set_should_search_in_dotted(true);}),
                create_config_from_args("./ --search-in-dotted").unwrap());
        assert_eq!(conf("./", |c| {c.engine.set_no_gitignore(true);}),
                create_config_from_args("./ --no-gitignore").unwrap());
        assert_eq!(conf("./", |c| {c.view.set_should_show_faulty_files(true);}),
                create_config_from_args("./ --show-faulty-files").unwrap());
        assert_eq!(conf("./", |c| {c.engine.set_exclude_dirs(vec!["a".to_owned(),"b".to_owned(),"c".to_owned()]);}),
                create_config_from_args("./ --exclude a,b ,  c ").unwrap());
        assert_eq!(conf("./", |c| {c.engine.set_exclude_dirs(vec!["a/path".to_owned(),"b/path".to_owned()]);}),
                create_config_from_args("./ --exclude \"a\\path\", \"b\\path\"").unwrap());
        assert_eq!(conf("./", |c| {c.engine.set_languages_of_interest(vec!["a".to_owned(),"b".to_owned(),"c".to_owned()]);}),
                create_config_from_args("./ --languages a,b,c").unwrap());
        assert_eq!(conf("./", |c| {c.engine.set_languages_of_interest(vec!["a".to_owned()]);}),
                create_config_from_args("./ --languages a, ").unwrap());
        assert_eq!(conf("./", |c| {c.view.set_log_option(LogOption::new(Some("this is a test".to_owned())));}),
                create_config_from_args("./ --log   this is a test ").unwrap());
        assert_eq!(conf("./", |c| {c.view.set_log_option(LogOption::new(None));}),
                create_config_from_args("./ --log  ").unwrap());
    }

    #[test]
    fn test_hide_arg_parsing() {
        let hidden = |command: &str| create_config_from_args(command).unwrap().view.hidden;

        assert_eq!(Hidden::default(), hidden("./"));
        assert_eq!(Hidden {keywords: true, ..Default::default()}, hidden("./ --hide keywords"));
        // Commas and spaces both separate, so the Powershell comma escaping is never needed
        let expected = Hidden {parsing_info: true, bar: true, timing: true, ..Default::default()};
        assert_eq!(expected, hidden("./ --hide parsing-info,bar,timing"));
        assert_eq!(expected, hidden("./ --hide parsing-info bar timing"));
        assert_eq!(expected, hidden("./ --hide  PARSING-INFO , bar,  Timing "));

        // The error names the entry that was not understood, instead of the whole command
        assert_eq!(Err(ArgParsingError::InvalidHideTarget("detials".to_owned())),
                create_config_from_args("./ --hide keywords,detials"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs(HIDE.to_owned())), create_config_from_args("./ --hide"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs(HIDE.to_owned())), create_config_from_args("./ --hide   "));

        // What is written to a config file is what the command line accepts
        assert_eq!("parsing-info,bar,timing", expected.to_list_string());
        assert_eq!(Ok(expected), Hidden::parse(&expected.to_list_string()));
        assert_eq!(Ok(Hidden::default()), Hidden::parse(""));
    }

    #[test]
    fn test_has_any_args() {
        assert!(has_any_args("cmnd a"));
        assert!(has_any_args("cmnd    a"));
        assert!(has_any_args("cmnd    a   "));
        assert!(has_any_args("cmnd a a"));

        assert!(!has_any_args("cmnd"));
        assert!(!has_any_args("cmnd    "));
    }

    #[test]
    fn test_absolute_conversion() {
        let path = "./";
        let abs = mezura_core::engine::targets::convert_to_absolute(path);
        assert!(Path::new(path).is_relative());
        assert!(Path::new(&abs).is_absolute());

        let path = "./src";
        let abs = mezura_core::engine::targets::convert_to_absolute(path);
        assert!(Path::new(path).is_relative());
        assert!(Path::new(&abs).is_absolute());

        let path = "./src/../src";
        let abs = mezura_core::engine::targets::convert_to_absolute(path);
        assert!(Path::new(path).is_relative());
        assert!(Path::new(&abs).is_absolute());

        let path = "src";
        let abs = mezura_core::engine::targets::convert_to_absolute(path);
        assert!(Path::new(path).is_relative());
        assert!(Path::new(&abs).is_absolute());

        let path = "src/main.rs";
        let abs = mezura_core::engine::targets::convert_to_absolute(path);
        assert!(Path::new(path).is_relative());
        assert!(Path::new(&abs).is_absolute());
    }

    #[test]
    fn test_parse_dirs() {
        assert_eq!(Err(ArgParsingError::InvalidPath("a".to_owned())), parse_dirs("a"));
        assert_eq!(Err(ArgParsingError::InvalidPath("a b c".to_owned())), parse_dirs("a b c"));

        assert_eq!(vec![mezura_core::engine::targets::convert_to_absolute("./")], parse_dirs("./").unwrap());
        assert_eq!(vec![mezura_core::engine::targets::convert_to_absolute("./src")], parse_dirs("\"./src\"").unwrap());

        // Targets that contain other targets swallow them, so that no file is counted twice
        assert_eq!(vec![mezura_core::engine::targets::convert_to_absolute(".././")], parse_dirs("./, .././").unwrap());
        assert_eq!(vec![mezura_core::engine::targets::convert_to_absolute(".././")], parse_dirs("./, \".././\"").unwrap());
        assert_eq!(vec![mezura_core::engine::targets::convert_to_absolute("./")], parse_dirs("./src, ./, ./tests").unwrap());
        assert_eq!(vec![mezura_core::engine::targets::convert_to_absolute("./src")], parse_dirs("./src, ./src/main.rs").unwrap());

        // Unrelated targets are all kept, in the order they were written
        assert_eq!(vec![mezura_core::engine::targets::convert_to_absolute("./tests"), mezura_core::engine::targets::convert_to_absolute("./src")],
                parse_dirs("./tests, ./src").unwrap());

        // A space is not a separator while nothing is named, so a path is allowed to contain one.
        // It cannot be: by the time a command line reaches here the shell has split it and taken
        // the quotes off, so a space inside a path and a space between two paths look the same.
        assert_eq!(Err(ArgParsingError::InvalidPath("./tests ./src".to_owned())), parse_dirs("./tests ./src"));
        assert_eq!(vec![mezura_core::engine::targets::convert_to_absolute("./")], parse_dirs(&std::env::current_dir().unwrap().to_string_lossy()).unwrap());
    }

    #[test]
    fn a_target_can_be_declared_under_a_module_name() {
        let src = mezura_core::engine::targets::convert_to_absolute("./src");
        let tests = mezura_core::engine::targets::convert_to_absolute("./tests");

        assert_eq!(vec![format!("code={src}"), format!("suite={tests}")], parse_dirs("code=./src suite=./tests").unwrap());
        // The name holds for the whole comma list it opened, and one module is allowed to be several
        // directories
        assert_eq!(vec![format!("code={src}"), format!("code={tests}")], parse_dirs("code=./src,./tests").unwrap());
        // and a name inside the list still starts a new one, which is what lets a saved
        // configuration write the whole thing back on one line and read it as what it was
        assert_eq!(vec![format!("code={src}"), format!("suite={tests}")], parse_dirs("code=./src,suite=./tests").unwrap());
        // A comma with a space around it is still a comma
        assert_eq!(vec![format!("code={src}"), format!("code={tests}")], parse_dirs("code=./src, ./tests").unwrap());
        // but a space alone ends the list, so nothing written after a named target joins it. That
        // only holds once a name is present, which is what this whole rule is conditional on.
        assert_eq!(vec![format!("code={src}"), tests.clone()], parse_dirs("code=./src ./tests").unwrap());
        // Two spellings of one name are one module
        assert_eq!(vec![format!("code={src}"), format!("code={tests}")], parse_dirs("code=./src CODE=./tests").unwrap());

        // An '=' is a legal character in a path, so anything that looks like one is read as one
        assert_eq!(vec![src.clone()], parse_dirs("./src").unwrap());
        assert_eq!(Err(ArgParsingError::MalformedTarget("code=".to_owned())), parse_dirs("code="));
        assert_eq!(Err(ArgParsingError::InvalidPath("nope".to_owned())), parse_dirs("code=nope"));
    }

    // The targets a configuration declares are text like any other and go through the same
    // resolution the command line does, once, at the end, after every source of the run's settings
    // has been merged. A mistake in them still names the configuration it came from, and a contest
    // the command line refuses is refused from a file too.
    #[test]
    fn the_targets_of_a_configuration_are_resolved_before_they_are_used() {
        std::fs::create_dir_all(&crate::paths::PERSISTENT_APP_PATHS.config_dir).unwrap();
        let write_config = |name: &str, dirs: &str| {
            let path = crate::paths::PERSISTENT_APP_PATHS.config_dir.clone() + name + ".txt";
            std::fs::write(&path, format!("===> dirs\n{dirs}\n")).unwrap();
            path
        };

        let path = write_config("a2resolve1", "./does-not-exist-a2");
        let result = create_config_from_args("--load a2resolve1").map(|_| ());
        std::fs::remove_file(&path).unwrap();
        assert_eq!(Err(ArgParsingError::InvalidPathInConfig("./does-not-exist-a2".to_owned(), "a2resolve1".to_owned())),
                result);

        let path = write_config("a2resolve2", "frontend=./src\nbackend=./src");
        let result = create_config_from_args("--load a2resolve2").map(|_| ());
        std::fs::remove_file(&path).unwrap();
        assert_eq!(Err(ArgParsingError::ContestedTarget(mezura_core::engine::targets::convert_to_absolute("./src"),
                "frontend".to_owned(), "backend".to_owned())), result);

        let path = write_config("a2resolve3", "code=./src");
        let result = create_config_from_args("--load a2resolve3").unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(vec![Target::named("code", mezura_core::engine::targets::convert_to_absolute("./src"))],
                result.engine.dirs.to_vec());
    }

    // There is no more specific one of the two to decide it, and whichever won would take the
    // other's files away without a word
    #[test]
    fn one_path_under_two_names_is_refused() {
        let src = mezura_core::engine::targets::convert_to_absolute("./src");
        assert_eq!(Err(ArgParsingError::ContestedTarget(src.clone(), "code".to_owned(), "other".to_owned())),
                parse_dirs("code=./src other=./src"));
        assert_eq!(Err(ArgParsingError::ContestedTarget(src.clone(), "code".to_owned(), "(unnamed)".to_owned())),
                parse_dirs("code=./src ./src"));

        // The same name twice over one path is a repetition and not a contest
        assert_eq!(vec![format!("code={src}")], parse_dirs("code=./src code=./src").unwrap());
    }

    #[test]
    fn test_parse_dirs_with_glob_patterns() {
        let root = std::env::temp_dir().join("mezura_glob_test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("a").join("src")).unwrap();
        std::fs::create_dir_all(root.join("b").join("src")).unwrap();
        std::fs::create_dir_all(root.join("c")).unwrap();
        std::fs::write(root.join("a").join("src").join("one.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("b").join("src").join("two.rs"), "fn main() {}").unwrap();
        let root = root.to_str().unwrap().replace('\\', "/");
        let abs = |x: &str| mezura_core::engine::targets::convert_to_absolute(&format!("{root}/{x}"));

        assert_eq!(vec![abs("a/src"), abs("b/src")], parse_dirs(&format!("{root}/*/src")).unwrap());
        assert_eq!(vec![abs("a/src/one.rs")], parse_dirs(&format!("{root}/a/src/*.rs")).unwrap());
        assert_eq!(vec![abs("a"), abs("b"), abs("c")], parse_dirs(&format!("{root}/*")).unwrap());

        // A pattern can be mixed with literal paths, and the overlaps of both are collapsed
        assert_eq!(vec![abs("a"), abs("b"), abs("c")],
                parse_dirs(&format!("{root}/*, {root}/*/src, {root}/a/src/one.rs")).unwrap());

        assert_eq!(Err(ArgParsingError::NoGlobMatches(format!("{root}/*/nope"))),
                parse_dirs(&format!("{root}/*/nope")));
        // Named in its prepared form, joined to the working directory: that is what a saved
        // configuration holds, so the error names what is actually written wherever it lives,
        // and for a typed one it also says where the expansion looked
        assert_eq!(Err(ArgParsingError::InvalidGlobPattern(
                mezura_core::engine::targets::convert_to_absolute("./").trim_end_matches('/').to_owned() + "/a[")),
                parse_dirs("a["));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn test_glob_matches_respect_gitignore_but_literal_paths_do_not() {
        let root = std::env::temp_dir().join("mezura_glob_gitignore_test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("kept")).unwrap();
        std::fs::create_dir_all(root.join("build").join("deep")).unwrap();
        std::fs::write(root.join(".gitignore"), "build/\nignored.rs\n").unwrap();
        std::fs::write(root.join("kept").join("one.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("kept").join("ignored.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("build").join("deep").join("generated.rs"), "fn main() {}").unwrap();
        let root = root.to_str().unwrap().replace('\\', "/");
        let abs = |x: &str| mezura_core::engine::targets::convert_to_absolute(&format!("{root}/{x}"));

        // The ignored dir and the ignored file are dropped from the matches
        assert_eq!(vec![abs("kept")], parse_dirs(&format!("{root}/*")).unwrap());
        assert_eq!(vec![abs("kept/one.rs")], parse_dirs(&format!("{root}/**/*.rs")).unwrap());

        // Unless the gitignore support is turned off
        assert_eq!(vec![abs("build"), abs("kept")], parse_dirs_with(&format!("{root}/*"), false, false).unwrap());
        assert_eq!(vec![abs("build/deep/generated.rs"), abs("kept/ignored.rs"), abs("kept/one.rs")],
                parse_dirs_with(&format!("{root}/**/*.rs"), false, false).unwrap());

        // Explicitly named paths are always used, even when they are ignored
        assert_eq!(vec![abs("build")], parse_dirs(&format!("{root}/build")).unwrap());
        assert_eq!(vec![abs("kept/ignored.rs")], parse_dirs(&format!("{root}/kept/ignored.rs")).unwrap());

        assert_eq!(Err(ArgParsingError::AllGlobMatchesIgnored(format!("{root}/build/*"))),
                parse_dirs(&format!("{root}/build/*")));

        std::fs::remove_dir_all(&root).unwrap();
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

    // A configuration that declared 'no-gitignore' beside its own dirs had the two halves honoured
    // differently: the walk respected the flag, while the resolution of the dirs written next to it
    // read the flags from the raw command line only. A glob whose matches are all gitignored then
    // died with "doesn't exist anymore", when the identical command line counted them.
    #[test]
    fn a_configs_own_flags_apply_when_its_own_dirs_are_resolved() {
        std::fs::create_dir_all(&crate::paths::PERSISTENT_APP_PATHS.config_dir).unwrap();
        let (corpus, corpus_str) = a2_corpus("mezura-a2-config-corpus");

        let config_path = crate::paths::PERSISTENT_APP_PATHS.config_dir.clone() + "a2gitignore.txt";
        std::fs::write(&config_path, format!(
                "===> dirs\n{corpus_str}/target/*\n\n===> no-gitignore\nyes\n")).unwrap();

        let config = create_config_from_args("--load a2gitignore");
        std::fs::remove_file(&config_path).unwrap();
        std::fs::remove_dir_all(&corpus).unwrap();

        let config = config.expect("the config's own no-gitignore was not applied to its own dirs");
        assert!(config.engine.no_gitignore);
        assert_eq!(1, config.engine.dirs.len());
        assert!(config.engine.dirs[0].path.ends_with("target/lib.rs"), "resolved to {:?}", config.engine.dirs);
    }

    // The merged settings drive the walk, so they drive the resolution of every target too,
    // whichever source the target came from: a glob typed on the command line is expanded under
    // the 'no-gitignore' that a loaded configuration declared.
    #[test]
    fn a_loaded_configs_flags_apply_to_a_command_line_glob() {
        std::fs::create_dir_all(&crate::paths::PERSISTENT_APP_PATHS.config_dir).unwrap();
        let (corpus, corpus_str) = a2_corpus("mezura-a2-cli-corpus");

        let config_path = crate::paths::PERSISTENT_APP_PATHS.config_dir.clone() + "a2cliflag.txt";
        std::fs::write(&config_path, "===> no-gitignore\nyes\n").unwrap();

        let config = create_config_from_args(&format!("{corpus_str}/target/* --load a2cliflag"));
        std::fs::remove_file(&config_path).unwrap();
        std::fs::remove_dir_all(&corpus).unwrap();

        let config = config.expect("the loaded config's no-gitignore was not applied to the command line glob");
        assert!(config.engine.no_gitignore);
        assert_eq!(1, config.engine.dirs.len());
        assert!(config.engine.dirs[0].path.ends_with("target/lib.rs"), "resolved to {:?}", config.engine.dirs);
    }

    // '--save' used to write the matches a pattern had expanded to at the moment of saving, so the
    // pattern was lost and the configuration was a snapshot pretending to be a rule. The saved file
    // carries the pattern itself now, absolute, and every load expands it fresh.
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

        // and a later load expands the pattern to whatever exists by then
        let loaded = create_config_from_args("--load a2save");
        std::fs::remove_file(&config_path).unwrap();
        std::fs::remove_dir_all(&corpus).unwrap();

        assert!(saved.contains(&format!("{corpus_str}/sub*")),
                "the pattern is not in the saved file:\n{saved}");
        assert!(!saved.contains("sub1"), "the saved file holds the expansion, not the pattern:\n{saved}");
        assert_eq!(2, loaded.unwrap().engine.dirs.len());
    }

    // Through '--load', the same file must not be reported as non-existent: it exists, it just
    // cannot be read whole, and telling the user it is not there sends them looking for a typo in
    // the name instead of at the file's encoding.
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

    // A '--' inside a word belongs to the word. The old substring split cut any path carrying one
    // into a target that does not exist and a command that does not parse: tools that encode a
    // hierarchy into a single folder name produce exactly such paths, and this program's own
    // scratchpad directory was one. A command still begins wherever '--' follows whitespace or
    // starts the line, which is the only way anybody writes one.
    #[test]
    fn a_double_dash_inside_a_path_is_not_the_start_of_a_command() {
        let root = std::env::temp_dir().join("mezura--double--dash");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.rs"), "fn a() {}\n").unwrap();
        let root_str = root.to_str().unwrap().replace('\\', "/");

        let bare = create_config_from_args(&root_str).unwrap();
        let with_flag = create_config_from_args(&format!("{root_str} --threads 2 3")).unwrap();
        let through_dirs = create_config_from_args(&format!("--dirs {root_str} --threads 2 3")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();

        assert_eq!(vec![Target::of(root_str.clone())], bare.engine.dirs.to_vec());
        assert_eq!(vec![Target::of(root_str.clone())], with_flag.engine.dirs.to_vec());
        assert_eq!(Threads::new(2, 3), with_flag.engine.threads);
        assert_eq!(vec![Target::of(root_str)], through_dirs.engine.dirs.to_vec());
        assert_eq!(Threads::new(2, 3), through_dirs.engine.threads);
    }

    #[test]
    fn test_save_load_configs() {
        // The saving and loading of configs always goes through the persistent config dir, which doesn't
        // exist yet on a machine where the program has never been executed.
        std::fs::create_dir_all(&crate::paths::PERSISTENT_APP_PATHS.config_dir).unwrap();
        let test_file_path = &crate::paths::PERSISTENT_APP_PATHS.config_dir.clone().add("/test000.txt");
        assert!(!Path::new(test_file_path).exists());

        let mut saved_config = create_config_builder_from_args("--threads 1 5 --languages lang1, lang2 --save test000").unwrap();
        assert!(Path::new(test_file_path).exists());
        assert_eq!(saved_config.dirs.clone().unwrap()[0], Target::of(mezura_core::engine::targets::convert_to_absolute("./")));
        assert_eq!(saved_config.threads.clone().unwrap(), Threads::new(1, 5));
        assert_eq!(saved_config.languages_of_interest.clone().unwrap(), vec!["lang1", "lang2"]);

        let mut loaded_config = create_config_builder_from_args("--load test000").unwrap();
        saved_config.config_name_to_save = None;
        loaded_config.config_name_to_load = None;
        assert_eq!(saved_config, loaded_config);

        loaded_config = create_config_builder_from_args("--load test000 --threads 1 4 --dirs ./").unwrap();
        assert_eq!(saved_config.dirs, loaded_config.dirs);
        assert_ne!(saved_config.threads, loaded_config.threads);

        saved_config = create_config_builder_from_args("--load test000 --threads 1 4 --dirs ./ --save test000").unwrap();
        saved_config.config_name_to_save = None;
        assert_eq!(saved_config, loaded_config);

        std::fs::remove_file(test_file_path).unwrap();
    }

    #[test]
    fn test_theme_arg_parsing() {
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

        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("theme".to_owned())),
                create_config_from_args("./ --theme"));

        std::fs::remove_file(test_theme_path).unwrap();
    }

    #[test]
    fn test_load_config_with_invalid_value() {
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

    // Every command that 'resolve_invalid_config_fields' does not know about is treated as never
    // overridden, so giving it correctly on the command line would still kill the run
    #[test]
    fn force_lang_takes_pairs_of_an_extension_and_a_language_and_refuses_anything_else() {
        let forced = |args: &str| create_config_from_args(&format!("./ --force-lang {args}")).map(|x| x.engine.forced_languages);

        assert_eq!(Ok(hashmap!("m".to_owned() => "matlab".to_owned())), forced("m=matlab"));
        // A leading dot is accepted the way '--languages' accepts it, and the extension is lowercased
        // here so that it is keyed the same way the lookup will ask for it. The language name is kept
        // as it was typed, and compared without case later.
        assert_eq!(Ok(hashmap!("m".to_owned() => "MATLAB".to_owned(), "pl".to_owned() => "perl".to_owned())),
                forced(".M=MATLAB, pl = perl"));

        for wrong in ["", "matlab", "m=", "=matlab", "m=matlab,perl"] {
            assert!(forced(wrong).is_err(), "'--force-lang {wrong}' was accepted");
        }
    }

    #[test]
    fn test_a_command_line_value_rescues_every_invalid_field_of_a_config() {
        std::fs::create_dir_all(&crate::paths::PERSISTENT_APP_PATHS.config_dir).unwrap();
        let test_file_path = &crate::paths::PERSISTENT_APP_PATHS.config_dir.clone().add("/test002.txt");
        let _ = std::fs::remove_file(test_file_path);
        std::fs::write(test_file_path, "===> dirs\nfrontend=\n\n===> sort\nnope\n\n===> top\nnope\n\n===> bar-thickness\nnope\n\n\
                ===> number-separator\nnope\n\n===> decimal-separator\nnope\n\n===> force-lang\nnope\n").unwrap();

        // A target that does not parse is a target whose files would not be counted, so with no
        // target on the command line to take its place the run stops instead of counting less
        assert_eq!(Err(ArgParsingError::InvalidValueInConfig("dirs".to_owned(), "test002".to_owned())),
                create_config_from_args("--load test002"));

        assert_eq!(Err(ArgParsingError::InvalidValueInConfig("sort".to_owned(), "test002".to_owned())),
                create_config_from_args("./ --load test002"));

        let rescued = create_config_from_args(
                "./ --load test002 --sort name --top 3 --bar-thickness fat --number-separator dot --decimal-separator comma --force-lang m=matlab").unwrap();
        assert_eq!(vec![Target::of(mezura_core::engine::targets::convert_to_absolute("./"))], rescued.engine.dirs.to_vec());
        assert_eq!(SortCriterion::Name, rescued.view.sort_by);
        assert_eq!(Some(3), rescued.view.top_n);
        assert_eq!(BarThickness::Fat, rescued.view.bar_thickness);
        assert_eq!(NumberSeparator::Dot, rescued.view.number_separator);
        assert_eq!(DecimalSeparator::Comma, rescued.view.decimal_separator);
        assert_eq!(hashmap!("m".to_owned() => "matlab".to_owned()), rescued.engine.forced_languages);

        std::fs::remove_file(test_file_path).unwrap();
    }
}
