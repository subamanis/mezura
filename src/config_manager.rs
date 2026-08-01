use std::{collections::HashMap, path::Path};

use colored::{ColoredString, Colorize};

use crate::{Formatted, GitignoreStack, io_handler, message_printer, suggestions, theme::{self, Theme}, utils};
#[cfg(test)]
use crate::Color;

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


pub const MAX_PRODUCERS_VALUE : usize = 32;
pub const MIN_PRODUCERS_VALUE : usize = 1;
pub const MAX_CONSUMERS_VALUE : usize = 128;
pub const MIN_CONSUMERS_VALUE : usize = 1;
pub const MIN_COMPARE_LEVEL   : usize = 0;
pub const MAX_COMPARE_LEVEL   : usize = 10;

// default config values
const DEF_BRACES_AS_CODE    : bool    = false;
const DEF_SEARCH_IN_DOTTED  : bool    = false;
const DEF_SHOW_FAULTY_FILES : bool    = false;
const DEF_NO_GITIGNORE      : bool    = false;
const DEF_COMPARE_LEVEL     : usize   = 1;


#[derive(Debug,PartialEq,Clone)]
pub struct Configuration {
    pub version: &'static str,
    pub dirs: Vec<String>,
    pub exclude_dirs: Vec<String>,
    pub languages_of_interest: Vec<String>,
    pub excluded_languages: Vec<String>,
    pub forced_languages: HashMap<String,String>,
    pub threads: Threads,
    pub braces_as_code: bool,
    pub should_search_in_dotted: bool,
    pub should_show_faulty_files: bool,
    pub hidden: Hidden,
    pub no_gitignore: bool,
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
    Boxed
}

impl Layout {
    pub fn parse(value: &str) -> Option<Layout> {
        match value.trim().to_lowercase().as_str() {
            "list" => Some(Self::List),
            "table" => Some(Self::Table),
            "boxed" => Some(Self::Boxed),
            _ => None
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Table => "table",
            Self::Boxed => "boxed"
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

#[derive(Debug,PartialEq,Clone)]
pub struct Threads {
    pub producers: usize,
    pub consumers: usize
}

#[derive(Debug, PartialEq)]
pub enum ArgParsingError {
    NoArgsProvided,
    UnparsableWorkingDir,
    MissingTargetDirs,
    InvalidPath(String),
    InvalidPathInConfig(String,String),
    DoublePath,
    UnrecognisedCommand(String),
    IncorrectCommandArgs(String),
    UnexpectedCommandArgs(String),
    NonExistantConfig(String),
    NonExistantTheme(String),
    InvalidStyle(String),
    InvalidHideTarget(String),
    InvalidValueInConfig(String,String),
    InvalidGlobPattern(String),
    NoGlobMatches(String),
    AllGlobMatchesIgnored(String)
}

// Empty line argument is not supposed to be allowed, since this check is being performed in main
pub fn create_config_from_args(line: &str) -> Result<Configuration, ArgParsingError> {
    let config = create_config_builder_from_args(line)?.build();

    // Written from the resolved theme and therefore after it is built, which is also why this does
    // not sit next to '--save': what the file has to hold is the look, not the pieces it came from
    if let Some(name) = &config.theme_name_to_save {
        if config.theme == Theme::default() {
            eprintln!("\n{}", format!("Nothing to save in theme '{name}': every style is at its default.").yellow());
        } else {
            match io_handler::save_theme_to_file(&crate::PERSISTENT_APP_PATHS.themes_dir, name, &config.theme) {
                Err(_) => eprintln!("\n{}","Error while trying to save the theme.".yellow()),
                Ok(_) => eprintln!("\nTheme '{name}' saved successfully.")
            }
        }
    }

    Ok(config)
}

pub fn create_config_builder_from_args(line: &str) -> Result<ConfigurationBuilder, ArgParsingError> {
    let mut dirs = None;
    let mut options = line.split("--");
    // The target paths can be given before these flags are parsed, so they are detected up front
    let respect_gitignore = !line.contains(&(String::from("--") + NO_GITIGNORE));
    let dotted_are_targetable = line.contains(&(String::from("--") + SEARCH_IN_DOTTED));

    if line.trim().starts_with("--") {
        //ignoring the empty first element that is caused by splitting
        options.next();
    } else {
        match parse_dirs(options.next().unwrap(), respect_gitignore, dotted_are_targetable) {
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

            let parse_result = parse_dirs(arguments, respect_gitignore, dotted_are_targetable);
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
            let vec = utils::parse_paths_to_vec(arguments);
            if vec.is_empty() || utils::build_exclude_matcher(&vec).is_err() {
                message_printer::print_help_message_for_command(EXCLUDE);
                return Err(ArgParsingError::IncorrectCommandArgs(EXCLUDE.to_owned()));
            }
            exclude_dirs = Some(vec);
        } else if command_name == LANGUAGES {
            let vec = utils::parse_languages_to_vec(arguments);
            if vec.is_empty() {
                message_printer::print_help_message_for_command(LANGUAGES);
                return Err(ArgParsingError::IncorrectCommandArgs(LANGUAGES.to_owned()));
            }
            languages_of_interest = Some(vec);
        } else if command_name == EXCLUDE_LANGUAGES {
            let vec = utils::parse_languages_to_vec(arguments);
            if vec.is_empty() {
                message_printer::print_help_message_for_command(EXCLUDE_LANGUAGES);
                return Err(ArgParsingError::IncorrectCommandArgs(EXCLUDE_LANGUAGES.to_owned()));
            }
            excluded_languages = Some(vec);
        } else if command_name == FORCE_LANG {
            let Some(map) = utils::parse_forced_languages(arguments) else {
                message_printer::print_help_message_for_command(FORCE_LANG);
                return Err(ArgParsingError::IncorrectCommandArgs(FORCE_LANG.to_owned()));
            };
            forced_languages = Some(map);
        } else if command_name == THREADS {
            let threads_values = utils::parse_two_usize_values(arguments,
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
            if io_handler::load_theme(name, &crate::PERSISTENT_APP_PATHS.themes_dir).is_none() {
                return Err(ArgParsingError::NonExistantTheme(name.to_owned()))
            }
            theme_name = Some(name.to_owned());
        } else if command_name == STYLE {
            match theme::parse_overrides(arguments) {
                Ok(x) => styles = Some(x),
                Err(x) => {
                    message_printer::print_help_message_for_command(STYLE);
                    return Err(ArgParsingError::InvalidStyle(x.formatted()))
                }
            }
        } else if command_name == TOP {
            match utils::parse_usize_value(arguments, 1, usize::MAX) {
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
            let compare_num = utils::parse_usize_value(arguments, MIN_COMPARE_LEVEL, MAX_COMPARE_LEVEL);
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

            if let Ok((mut options, issues)) = io_handler::parse_config_file(Some(config_name), None) {
                if let Some(dirs) = &options.dirs {
                    match resolve_target_paths(dirs, respect_gitignore, dotted_are_targetable) {
                        Ok(x) => options.dirs = Some(x),
                        Err(ArgParsingError::InvalidPath(p)) | Err(ArgParsingError::InvalidGlobPattern(p))
                                | Err(ArgParsingError::NoGlobMatches(p)) | Err(ArgParsingError::AllGlobMatchesIgnored(p)) =>
                                return Err(ArgParsingError::InvalidPathInConfig(p, config_name.to_owned())),
                        Err(x) => return Err(x)
                    }
                }
                custom_config = Some((options, issues));
                config_name_to_load = Some(config_name.to_owned());
            } else {
                return Err(ArgParsingError::NonExistantConfig(config_name.to_owned()))
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
        dirs, exclude_dirs, languages_of_interest, excluded_languages, forced_languages, threads, braces_as_code,
        should_search_in_dotted: search_in_dotted, should_show_faulty_files: show_faulty_files,
        hidden, no_gitignore, theme_name, theme_name_to_save, log, compare_level,
        config_name_to_save, config_name_to_load, styles, bar_thickness, number_separator, decimal_separator, layout, output, sort_by, top_n,
        config_styles: None, theme_styles: None
    };

    if let Some((custom, issues)) = custom_config {
        let config_name = config_builder.config_name_to_load.clone().unwrap_or_default();
        print_config_file_warnings(&issues.warnings, &config_name);
        resolve_invalid_config_fields(&config_builder, &issues.invalid_fields, &config_name)?;
        config_builder.add_missing_fields(custom);
    }

    if let Some(name) = &config_builder.config_name_to_save {
        if config_builder.dirs.is_none() {
            config_builder.dirs = Some(parse_working_dir_as_target_dir()?);
        }

        match io_handler::save_existing_commands_from_config_builder_to_file(None, name, &config_builder) {
            Err(_) => eprintln!("\n{}","Error while trying to save config.".yellow()),
            Ok(_) => eprintln!("\nConfiguration '{name}' saved successfully.")
        }
    }

    if config_builder.has_missing_fields()
        && let Ok((default_config, issues)) = io_handler::parse_config_file(None, None) {
        print_config_file_warnings(&issues.warnings, "default");
        resolve_invalid_config_fields(&config_builder, &issues.invalid_fields, "default")?;
        config_builder.add_missing_fields(default_config);
    }

    if let Some(name) = &config_builder.theme_name {
        match io_handler::load_theme(name, &crate::PERSISTENT_APP_PATHS.themes_dir) {
            Some((styles, errors)) => {
                for error in &errors {
                    eprintln!("\n{}", format!("In theme '{name}': {}", error.formatted()).yellow());
                }
                config_builder.theme_styles = Some(styles);
            },
            None => eprintln!("\n{}", format!("Theme '{name}' could not be loaded, the default styles will be used.").yellow())
        }
    }

    if config_builder.dirs.is_none() {
        config_builder.dirs = Some(parse_working_dir_as_target_dir()?);
    }

    Ok(config_builder)
}


fn print_config_file_warnings(warnings: &[String], config_name: &str) {
    for warning in warnings {
        eprintln!("\n{}", format!("In config '{config_name}': {warning}").yellow());
    }
}

// Every command that can end up in 'invalid_fields' belongs here. One that is missing is treated as
// never overridden, so giving it correctly on the command line would still not rescue the run.
fn resolve_invalid_config_fields(config_builder: &ConfigurationBuilder, invalid_fields: &[&str], config_name: &str) -> Result<(), ArgParsingError> {
    for field in invalid_fields {
        let is_overridden = match *field {
            THREADS => config_builder.threads.is_some(),
            COMPRARE_LEVEL => config_builder.compare_level.is_some(),
            BRACES_AS_CODE => config_builder.braces_as_code.is_some(),
            SEARCH_IN_DOTTED => config_builder.should_search_in_dotted.is_some(),
            SHOW_FAULTY_FILES => config_builder.should_show_faulty_files.is_some(),
            HIDE => config_builder.hidden.is_some(),
            NO_GITIGNORE => config_builder.no_gitignore.is_some(),
            EXCLUDE => config_builder.exclude_dirs.is_some(),
            FORCE_LANG => config_builder.forced_languages.is_some(),
            THEME => config_builder.theme_name.is_some(),
            SORT => config_builder.sort_by.is_some(),
            TOP => config_builder.top_n.is_some(),
            BAR_THICKNESS => config_builder.bar_thickness.is_some(),
            NUMBER_SEPARATOR => config_builder.number_separator.is_some(),
            DECIMAL_SEPARATOR => config_builder.decimal_separator.is_some(),
            LAYOUT => config_builder.layout.is_some(),
            _ => false
        };

        if is_overridden {
            eprintln!("\n{}", format!("Invalid value for the command '--{field}', in config '{config_name}'. The value will be ignored.").yellow());
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
    command.split(' ').skip(1).filter_map(utils::get_trimmed_if_not_empty).count() != 0
}

fn parse_dirs(s: &str, respect_gitignore: bool, search_in_dotted: bool) -> Result<Vec<String>, ArgParsingError> {
    resolve_target_paths(&utils::parse_paths_to_vec(s), respect_gitignore, search_in_dotted)
}

// Literal paths must exist and are always used, even if they are ignored or dotted, since the user
// named them explicitly. Glob patterns are expanded to the existing paths they match, and those
// matches are discovered by the program, so they are subject to the same rules as every other
// discovered path. Finally, targets contained in other targets are dropped, so that no file
// is counted twice.
fn resolve_target_paths(entries: &[String], respect_gitignore: bool, search_in_dotted: bool)
-> Result<Vec<String>, ArgParsingError>
{
    fn is_dotted(path: &Path) -> bool {
        path.file_name().and_then(|x| x.to_str()).is_some_and(|x| x.starts_with('.'))
    }

    let mut resolved = Vec::with_capacity(entries.len());
    for entry in entries {
        let trimmed = entry.trim();
        if utils::has_glob_metacharacters(trimmed) {
            let paths = match glob::glob(&trimmed.replace('\\', "/")) {
                Ok(x) => x,
                Err(_) => return Err(ArgParsingError::InvalidGlobPattern(trimmed.to_owned()))
            };
            let matches = paths.flatten().filter(|x| x.is_dir() || x.is_file()).collect::<Vec<_>>();
            if matches.is_empty() {
                return Err(ArgParsingError::NoGlobMatches(trimmed.to_owned()));
            }

            let relevant = matches.iter()
                    .filter(|x| search_in_dotted || !is_dotted(x))
                    .filter(|x| !respect_gitignore || !GitignoreStack::is_path_ignored(x))
                    .filter_map(|x| x.to_str().map(convert_to_absolute)).collect::<Vec<_>>();
            if relevant.is_empty() {
                return Err(ArgParsingError::AllGlobMatchesIgnored(trimmed.to_owned()));
            }
            resolved.extend(relevant);
        } else if utils::is_valid_path(trimmed) {
            resolved.push(convert_to_absolute(trimmed));
        } else {
            return Err(ArgParsingError::InvalidPath(trimmed.to_owned()));
        }
    }

    Ok(utils::remove_overlapping_paths(resolved))
}

fn parse_working_dir_as_target_dir() -> Result<Vec<String>, ArgParsingError> {
    if let Ok(path_buf) = std::env::current_dir()
        && let Some(path_str) = path_buf.to_str()
        && let Ok(x) = parse_dirs(path_str, true, false) {
        return Ok(x);
    }

    Err(ArgParsingError::UnparsableWorkingDir)
}

// The "canonicalize" function from the std that this function uses, (at least on window) seems to put the weird prefix
// "\\?\" before the path and it also puts forward slashes that we want to convert for compatibility.
fn convert_to_absolute(s: &str) -> String {
    let p = Path::new(s);
    if p.is_absolute() {
        return s.replace("\\", "/");
    }

    if let Ok(buf) = std::fs::canonicalize(p) {
        let str_path = buf.to_str().unwrap();
        str_path.strip_prefix(r"\\?\").unwrap_or(str_path).replace("\\", "/")
    } else {
        s.replace("\\", "/")
    }
}


#[derive(Debug, PartialEq, Default)]
pub struct ConfigurationBuilder {
    pub dirs:                     Option<Vec<String>>,
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

    pub fn build(&self) -> Configuration {
        Configuration {
            version: VERSION_ID,
            dirs: self.dirs.clone().unwrap(),
            exclude_dirs: (self.exclude_dirs).clone().unwrap_or_default(),
            languages_of_interest: (self.languages_of_interest).clone().unwrap_or_default(),
            excluded_languages: (self.excluded_languages).clone().unwrap_or_default(),
            forced_languages: (self.forced_languages).clone().unwrap_or_default(),
            threads: self.threads.clone().unwrap_or_default(),
            braces_as_code: self.braces_as_code.unwrap_or(DEF_BRACES_AS_CODE),
            should_search_in_dotted: self.should_search_in_dotted.unwrap_or(DEF_SEARCH_IN_DOTTED),
            should_show_faulty_files: self.should_show_faulty_files.unwrap_or(DEF_SHOW_FAULTY_FILES),
            hidden: self.hidden.unwrap_or_default(),
            no_gitignore: self.no_gitignore.unwrap_or(DEF_NO_GITIGNORE),
            log: self.log.clone().unwrap_or_default(),
            compare_level: self.compare_level.unwrap_or(DEF_COMPARE_LEVEL),
            config_name_to_save: self.config_name_to_save.clone(),
            config_name_to_load: self.config_name_to_load.clone(),
            theme_name_to_save: self.theme_name_to_save.clone(),
            bar_thickness: self.bar_thickness.unwrap_or_default(),
            number_separator: self.number_separator.unwrap_or_default(),
            decimal_separator: self.decimal_separator.unwrap_or_default(),
            layout: self.layout.unwrap_or_default(),
            output: self.output.unwrap_or_default(),
            sort_by: self.sort_by.unwrap_or_default(),
            top_n: self.top_n,
            theme: theme::resolve(self.theme_styles.as_deref().unwrap_or_default(),
                    self.config_styles.as_deref().unwrap_or_default(), self.styles.as_deref().unwrap_or_default())
        }
    }
}

impl Configuration {
    pub fn new(dirs: Vec<String>) -> Self {
        Configuration {
            version: VERSION_ID,
            dirs,
            exclude_dirs: Vec::new(),
            languages_of_interest: Vec::new(),
            excluded_languages: Vec::new(),
            forced_languages: HashMap::new(),
            threads: Threads::default(),
            braces_as_code: DEF_BRACES_AS_CODE,
            should_search_in_dotted: DEF_SEARCH_IN_DOTTED,
            should_show_faulty_files: DEF_SHOW_FAULTY_FILES,
            hidden: Hidden::default(),
            no_gitignore: DEF_NO_GITIGNORE,
            log: LogOption::default(),
            compare_level: DEF_COMPARE_LEVEL,
            config_name_to_save: None,
            config_name_to_load: None,
            theme_name_to_save: None,
            bar_thickness: BarThickness::default(),
            number_separator: NumberSeparator::default(),
            decimal_separator: DecimalSeparator::default(),
            layout: Layout::default(),
            output: OutputFormat::default(),
            sort_by: SortCriterion::default(),
            top_n: None,
            theme: Theme::default()
        }
    }

    // Everything that is not the document itself stays off stdout when the output is machine
    // readable, so that a single stray line cannot make it unparseable
    pub fn prints_text(&self) -> bool {
        self.output == OutputFormat::Text
    }

    //Setters used mainly in tests, for the ability to chain many config changes

    pub fn set_config_names_to_save_and_load(&mut self, to_save: Option<String>, to_load: Option<String>) -> &mut Self {
        self.config_name_to_save = to_save;
        self.config_name_to_load = to_load;
        self
    }

    pub fn set_exclude_dirs(&mut self, exclude_dirs: Vec<String>) -> &mut Self {
        self.exclude_dirs = exclude_dirs;
        self
    }

    pub fn set_languages_of_interest(&mut self, languages_of_interest: Vec<String>) -> &mut Self {
        self.languages_of_interest = languages_of_interest;
        self
    }

    pub fn set_threads(&mut self, producers: usize, consumers: usize) -> &mut Self {
        self.threads = Threads::new(producers, consumers);
        self
    }

    pub fn set_braces_as_code(&mut self, braces_as_code: bool) -> &mut Self {
        self.braces_as_code = braces_as_code;
        self
    }

    pub fn set_should_search_in_dotted(&mut self, should_search_in_dotted: bool) -> &mut Self {
        self.should_search_in_dotted = should_search_in_dotted;
        self
    }

    pub fn set_should_show_faulty_files(&mut self, should_show_faulty_files: bool) -> &mut Self {
        self.should_show_faulty_files = should_show_faulty_files;
        self
    }

    pub fn set_hidden(&mut self, hidden: Hidden) -> &mut Self {
        self.hidden = hidden;
        self
    }

    pub fn set_no_gitignore(&mut self, no_gitignore: bool) -> &mut Self {
        self.no_gitignore = no_gitignore;
        self
    }

    pub fn set_theme(&mut self, theme: Theme) -> &mut Self {
        self.theme = theme;
        self
    }

    pub fn set_log_option(&mut self, log: LogOption) -> &mut Self {
        self.log = log;
        self
    }
}

impl Threads {
    pub fn new(producers: usize, consumers: usize) -> Self {
        Threads {
            producers,
            consumers
        }
    }

    pub fn from(threads: (usize,usize)) -> Self {
        Threads {
            producers: threads.0,
            consumers: threads.1
        }
    }
}

impl Default for Threads {
    fn default() -> Self {
        let threads = num_cpus::get();
        // Consumers are oversubscribed hard, because what they wait on is a blocking file open and
        // the number that matters is how many reads are in flight, not how many cores exist. On one
        // machine, going from 22 consumers to 96 cost nothing measurable on a fast disk with a warm
        // cache, won 1.20x on a slow disk, and won 1.97x from cold. The asymmetry is the whole
        // argument: it is free where it does not help.
        if threads <= 4 {
            Threads {
                producers: 2,
                consumers: (threads * 4).clamp(8, MAX_CONSUMERS_VALUE)
            }
        } else {
            Threads {
                producers: (threads / 2).clamp(2, MAX_PRODUCERS_VALUE),
                consumers: (threads * 4).clamp(8, MAX_CONSUMERS_VALUE)
            }
        }
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
            Self::NoArgsProvided => "No arguments provided.".red(),
            Self::UnparsableWorkingDir => "The current working dir could not be parsed as target dir, try inputing it manually.".red(),
            Self::MissingTargetDirs => "The target directories (--dirs) are not specified.".red(),
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
            Self::UnexpectedCommandArgs(p) => format!("Command '--{p}' does not expect any arguments.").red(),
            Self::NonExistantConfig(p) => {
                let names = io_handler::names_in_dir(&crate::PERSISTENT_APP_PATHS.config_dir);
                let tail = suggestions::formatted_suggestion(p, &names.iter().map(String::as_str).collect::<Vec<_>>())
                        .unwrap_or_else(|| format!("Run '--{SHOW_CONFIGS}' to see the ones you have."));
                let error = format!("Configuration '{p}' does not exist.").red();
                ColoredString::from(format!("{error}\n\n{tail}").as_str())
            },
            Self::NonExistantTheme(p) => {
                let names = io_handler::names_in_dir(&crate::PERSISTENT_APP_PATHS.themes_dir);
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
            Self::AllGlobMatchesIgnored(p) => format!("Everything that the pattern '{p}' matched is skipped, either because a .gitignore file ignores it, or because it is a dotted path.\nUse the '--no-gitignore' or '--search-in-dotted' commands to include it, or provide the paths explicitly.").red()
        }
    }
}


#[cfg(test)]
mod tests {
    use std::ops::Add;

    use crate::PERSISTENT_APP_PATHS;

    use super::*;
    use crate::theme::Style;

    fn parse_dirs(s: &str) -> Result<Vec<String>, ArgParsingError> {
        super::parse_dirs(s, true, false)
    }

    fn new_conf(dir: &str) -> Configuration {
        let mut builder = ConfigurationBuilder { dirs: Some(vec![convert_to_absolute(dir)]), ..Default::default() };
        if let Ok((default_config, _)) = io_handler::parse_config_file(None, None) {
            builder.add_missing_fields(default_config);
        }
        builder.build()
    }

    // A command given twice keeps the value it was last given, which is what a reader of the line
    // assumes and what every shell tool does
    #[test]
    fn test_a_repeated_command_keeps_its_last_value() {
        assert_eq!(Threads::new(3, 11), create_config_from_args("./ --threads 2 10 --threads 3 11").unwrap().threads);
        assert_eq!(Some(4), create_config_from_args("./ --top 9 --top 4").unwrap().top_n);
        assert_eq!(SortCriterion::Name, create_config_from_args("./ --sort size --sort name").unwrap().sort_by);
        assert_eq!(Layout::Boxed, create_config_from_args("./ --layout list --layout boxed").unwrap().layout);
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
        assert_eq!(*new_conf("./").set_threads(1,1), create_config_from_args("./ --threads 1 1").unwrap());
        assert_eq!(*new_conf("./").set_threads(1,1), create_config_from_args("./ --threads   1   1 ").unwrap());
        assert_eq!(*new_conf("./").set_threads(1,1).set_braces_as_code(true),
                create_config_from_args("./ --threads 1 1 --braces-as-code").unwrap());
        assert_eq!(*new_conf("./").set_should_search_in_dotted(true),
                create_config_from_args("./ --search-in-dotted").unwrap());
        assert_eq!(*new_conf("./").set_no_gitignore(true),
                create_config_from_args("./ --no-gitignore").unwrap());
        assert_eq!(*new_conf("./").set_should_show_faulty_files(true),
                create_config_from_args("./ --show-faulty-files").unwrap());
        assert_eq!(*new_conf("./").set_exclude_dirs(vec!["a".to_owned(),"b".to_owned(),"c".to_owned()]),
                create_config_from_args("./ --exclude a,b ,  c ").unwrap());
        assert_eq!(*new_conf("./").set_exclude_dirs(vec!["a/path".to_owned(),"b/path".to_owned()]),
                create_config_from_args("./ --exclude \"a\\path\", \"b\\path\"").unwrap());
        assert_eq!(*new_conf("./").set_languages_of_interest(vec!["a".to_owned(),"b".to_owned(),"c".to_owned()]),
                create_config_from_args("./ --languages a,b,c").unwrap());
        assert_eq!(*new_conf("./").set_languages_of_interest(vec!["a".to_owned()]),
                create_config_from_args("./ --languages a, ").unwrap());
        assert_eq!(*new_conf("./").set_log_option(LogOption::new(Some("this is a test".to_owned()))),
                create_config_from_args("./ --log   this is a test ").unwrap());
        assert_eq!(*new_conf("./").set_log_option(LogOption::new(None)),
                create_config_from_args("./ --log  ").unwrap());
    }

    #[test]
    fn test_hide_arg_parsing() {
        let hidden = |command: &str| create_config_from_args(command).unwrap().hidden;

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
        let abs = convert_to_absolute(path);
        assert!(Path::new(path).is_relative());
        assert!(Path::new(&abs).is_absolute());

        let path = "./src";
        let abs = convert_to_absolute(path);
        assert!(Path::new(path).is_relative());
        assert!(Path::new(&abs).is_absolute());

        let path = "./src/../src";
        let abs = convert_to_absolute(path);
        assert!(Path::new(path).is_relative());
        assert!(Path::new(&abs).is_absolute());

        let path = "src";
        let abs = convert_to_absolute(path);
        assert!(Path::new(path).is_relative());
        assert!(Path::new(&abs).is_absolute());

        let path = "src/utils.rs";
        let abs = convert_to_absolute(path);
        assert!(Path::new(path).is_relative());
        assert!(Path::new(&abs).is_absolute());
    }

    #[test]
    fn test_parse_dirs() {
        assert_eq!(Err(ArgParsingError::InvalidPath("a".to_owned())), parse_dirs("a"));
        assert_eq!(Err(ArgParsingError::InvalidPath("a b c".to_owned())), parse_dirs("a b c"));

        assert_eq!(vec![convert_to_absolute("./")], parse_dirs("./").unwrap());
        assert_eq!(vec![convert_to_absolute("./src")], parse_dirs("\"./src\"").unwrap());

        // Targets that contain other targets swallow them, so that no file is counted twice
        assert_eq!(vec![convert_to_absolute(".././")], parse_dirs("./, .././").unwrap());
        assert_eq!(vec![convert_to_absolute(".././")], parse_dirs("./, \".././\"").unwrap());
        assert_eq!(vec![convert_to_absolute("./")], parse_dirs("./src, ./, ./tests").unwrap());
        assert_eq!(vec![convert_to_absolute("./src")], parse_dirs("./src, ./src/utils.rs").unwrap());

        // Unrelated targets are all kept
        assert_eq!(vec![convert_to_absolute("./src"), convert_to_absolute("./tests")],
                parse_dirs("./tests, ./src").unwrap());
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
        let abs = |x: &str| convert_to_absolute(&format!("{root}/{x}"));

        assert_eq!(vec![abs("a/src"), abs("b/src")], parse_dirs(&format!("{root}/*/src")).unwrap());
        assert_eq!(vec![abs("a/src/one.rs")], parse_dirs(&format!("{root}/a/src/*.rs")).unwrap());
        assert_eq!(vec![abs("a"), abs("b"), abs("c")], parse_dirs(&format!("{root}/*")).unwrap());

        // A pattern can be mixed with literal paths, and the overlaps of both are collapsed
        assert_eq!(vec![abs("a"), abs("b"), abs("c")],
                parse_dirs(&format!("{root}/*, {root}/*/src, {root}/a/src/one.rs")).unwrap());

        assert_eq!(Err(ArgParsingError::NoGlobMatches(format!("{root}/*/nope"))),
                parse_dirs(&format!("{root}/*/nope")));
        assert_eq!(Err(ArgParsingError::InvalidGlobPattern("a[".to_owned())), parse_dirs("a["));

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
        let abs = |x: &str| convert_to_absolute(&format!("{root}/{x}"));

        // The ignored dir and the ignored file are dropped from the matches
        assert_eq!(vec![abs("kept")], parse_dirs(&format!("{root}/*")).unwrap());
        assert_eq!(vec![abs("kept/one.rs")], parse_dirs(&format!("{root}/**/*.rs")).unwrap());

        // Unless the gitignore support is turned off
        assert_eq!(vec![abs("build"), abs("kept")], super::parse_dirs(&format!("{root}/*"), false, false).unwrap());
        assert_eq!(vec![abs("build/deep/generated.rs"), abs("kept/ignored.rs"), abs("kept/one.rs")],
                super::parse_dirs(&format!("{root}/**/*.rs"), false, false).unwrap());

        // Explicitly named paths are always used, even when they are ignored
        assert_eq!(vec![abs("build")], parse_dirs(&format!("{root}/build")).unwrap());
        assert_eq!(vec![abs("kept/ignored.rs")], parse_dirs(&format!("{root}/kept/ignored.rs")).unwrap());

        assert_eq!(Err(ArgParsingError::AllGlobMatchesIgnored(format!("{root}/build/*"))),
                parse_dirs(&format!("{root}/build/*")));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn test_save_load_configs() {
        // The saving and loading of configs always goes through the persistent config dir, which doesn't
        // exist yet on a machine where the program has never been executed.
        std::fs::create_dir_all(&PERSISTENT_APP_PATHS.config_dir).unwrap();
        let test_file_path = &PERSISTENT_APP_PATHS.config_dir.clone().add("/test000.txt");
        assert!(!Path::new(test_file_path).exists());

        let mut saved_config = create_config_builder_from_args("--threads 1 5 --languages lang1, lang2 --save test000").unwrap();
        assert!(Path::new(test_file_path).exists());
        assert_eq!(saved_config.dirs.clone().unwrap()[0], convert_to_absolute("./"));
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
        std::fs::create_dir_all(&PERSISTENT_APP_PATHS.themes_dir).unwrap();
        let test_theme_path = &PERSISTENT_APP_PATHS.themes_dir.clone().add("test-theme000.txt");
        // Cleaning up front instead of asserting absence, so that a failed run does not leave
        // behind a file that makes every later run fail during setup
        let _ = std::fs::remove_file(test_theme_path);
        std::fs::write(test_theme_path, "language-1 = cyan\nlanguage-2 = ff0080\ncode-number = bright-black dim\n").unwrap();

        let config = create_config_from_args("./ --theme Test-Theme000").unwrap();
        assert_eq!(Style::of(Color::Cyan), config.theme.language_1);
        assert_eq!(Style::of(Color::TrueColor{r:255,g:0,b:128}), config.theme.language_2);
        assert_eq!(Style::of(Color::BrightBlack).dim(), config.theme.code_number);

        // --style wins over what the theme declared, and the tokens it does not name survive
        let restyled = create_config_from_args("./ --theme test-theme000 --style code-number=cyan,heading=bold").unwrap();
        assert_eq!(Style::of(Color::Cyan), restyled.theme.code_number);
        assert_eq!(Style::plain().bold(), restyled.theme.heading);
        assert_eq!(Style::of(Color::Cyan), restyled.theme.language_1);

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
        std::fs::create_dir_all(&PERSISTENT_APP_PATHS.config_dir).unwrap();
        let test_file_path = &PERSISTENT_APP_PATHS.config_dir.clone().add("/test001.txt");
        assert!(!Path::new(test_file_path).exists());
        std::fs::write(test_file_path, "===> threads\n3343 45534\n").unwrap();

        assert_eq!(Err(ArgParsingError::InvalidValueInConfig("threads".to_owned(), "test001".to_owned())),
                create_config_from_args("./ --load test001"));

        let overridden = create_config_from_args("./ --load test001 --threads 1 2").unwrap();
        assert_eq!(overridden.threads, Threads::new(1, 2));

        std::fs::remove_file(test_file_path).unwrap();
    }

    // Every command that 'resolve_invalid_config_fields' does not know about is treated as never
    // overridden, so giving it correctly on the command line would still kill the run
    #[test]
    fn force_lang_takes_pairs_of_an_extension_and_a_language_and_refuses_anything_else() {
        let forced = |args: &str| create_config_from_args(&format!("./ --force-lang {args}")).map(|x| x.forced_languages);

        assert_eq!(Ok(crate::hashmap!("m".to_owned() => "matlab".to_owned())), forced("m=matlab"));
        // A leading dot is accepted the way '--languages' accepts it, and the extension is lowercased
        // here so that it is keyed the same way the lookup will ask for it. The language name is kept
        // as it was typed, and compared without case later.
        assert_eq!(Ok(crate::hashmap!("m".to_owned() => "MATLAB".to_owned(), "pl".to_owned() => "perl".to_owned())),
                forced(".M=MATLAB, pl = perl"));

        for wrong in ["", "matlab", "m=", "=matlab", "m=matlab,perl"] {
            assert!(forced(wrong).is_err(), "'--force-lang {wrong}' was accepted");
        }
    }

    #[test]
    fn test_a_command_line_value_rescues_every_invalid_field_of_a_config() {
        std::fs::create_dir_all(&PERSISTENT_APP_PATHS.config_dir).unwrap();
        let test_file_path = &PERSISTENT_APP_PATHS.config_dir.clone().add("/test002.txt");
        let _ = std::fs::remove_file(test_file_path);
        std::fs::write(test_file_path, "===> sort\nnope\n\n===> top\nnope\n\n===> bar-thickness\nnope\n\n\
                ===> number-separator\nnope\n\n===> decimal-separator\nnope\n\n===> force-lang\nnope\n").unwrap();

        assert_eq!(Err(ArgParsingError::InvalidValueInConfig("sort".to_owned(), "test002".to_owned())),
                create_config_from_args("./ --load test002"));

        let rescued = create_config_from_args(
                "./ --load test002 --sort name --top 3 --bar-thickness fat --number-separator dot --decimal-separator comma --force-lang m=matlab").unwrap();
        assert_eq!(SortCriterion::Name, rescued.sort_by);
        assert_eq!(Some(3), rescued.top_n);
        assert_eq!(BarThickness::Fat, rescued.bar_thickness);
        assert_eq!(NumberSeparator::Dot, rescued.number_separator);
        assert_eq!(DecimalSeparator::Comma, rescued.decimal_separator);
        assert_eq!(crate::hashmap!("m".to_owned() => "matlab".to_owned()), rescued.forced_languages);

        std::fs::remove_file(test_file_path).unwrap();
    }
}
