use std::{path::Path};

use colored::{ColoredString, Colorize};

use crate::{Color, Formatted, GitignoreStack, io_handler, message_printer, theme::{self, Theme}, utils};

// Application version, to be displayed at startup and with --help command
pub const VERSION_ID : &str = "v3.0.0";

// command flags
pub const DIRS               :&str   = "dirs";
pub const EXCLUDE            :&str   = "exclude";
pub const LANGUAGES          :&str   = "languages";
pub const EXCLUDE_LANGUAGES  :&str   = "exclude-languages";
pub const THREADS            :&str   = "threads";
pub const BRACES_AS_CODE     :&str   = "braces-as-code";
pub const SEARCH_IN_DOTTED   :&str   = "search-in-dotted";
pub const SHOW_FAULTY_FILES  :&str   = "show-faulty-files";
pub const HIDE               :&str   = "hide";
pub const NO_GITIGNORE       :&str   = "no-gitignore";
pub const COLORS             :&str   = "colors";
pub const COLOR_PALETTE      :&str   = "color-palette";
pub const STYLE              :&str   = "style";
pub const BAR_THICKNESS      :&str   = "bar-thickness";
pub const SORT               :&str   = "sort";
pub const TOP                :&str   = "top";
pub const LOG                :&str   = "log";
pub const COMPRARE_LEVEL     :&str   = "compare";
pub const SAVE               :&str   = "save";
pub const LOAD               :&str   = "load";
pub const HELP               :&str   = "help";
pub const CHANGELOG          :&str   = "changelog";
pub const SHOW_LANGUAGES     :&str   = "show-languages";
pub const SHOW_CONFIGS       :&str   = "show-configs";
pub const SHOW_PALETTES      :&str   = "show-palettes";
pub const TUNE_PALETTES      :&str   = "tune-palettes";

pub const MAX_PRODUCERS_VALUE : usize = 8;
pub const MIN_PRODUCERS_VALUE : usize = 1;
pub const MAX_CONSUMERS_VALUE : usize = 30;
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
    pub threads: Threads,
    pub braces_as_code: bool,
    pub should_search_in_dotted: bool,
    pub should_show_faulty_files: bool,
    pub hidden: Hidden,
    pub no_gitignore: bool,
    pub colors: Vec<Color>,
    pub log: LogOption,
    pub compare_level: usize,
    pub config_name_to_save: Option<String>,
    pub config_name_to_load: Option<String>,
    pub bar_thickness: BarThickness,
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
    pub status: bool,
    pub details: bool,
    pub keywords: bool,
    pub overview: bool,
    pub bar: bool,
    pub progress: bool,
    pub timing: bool
}

impl Hidden {
    fn pairs(self) -> [(&'static str, bool); 8] {
        [("version", self.version), ("status", self.status), ("details", self.details),
         ("keywords", self.keywords), ("overview", self.overview), ("bar", self.bar),
         ("progress", self.progress), ("timing", self.timing)]
    }

    // Returns the unrecognised name, so that the error can say which one it was
    pub fn parse(value: &str) -> Result<Hidden, String> {
        let mut hidden = Hidden::default();
        for entry in value.split([',', ' ', '\t']).map(str::trim).filter(|x| !x.is_empty()) {
            match entry.to_lowercase().as_str() {
                "version" => hidden.version = true,
                "status" => hidden.status = true,
                "details" => hidden.details = true,
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
    #[default]
    Slim,
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
    NonExistantPalette(String),
    InvalidStyle(String),
    InvalidHideTarget(String),
    InvalidValueInConfig(String,String),
    InvalidGlobPattern(String),
    NoGlobMatches(String),
    AllGlobMatchesIgnored(String)
}

// Empty line argument is not supposed to be allowed, since this check is being performed in main
pub fn create_config_from_args(line: &str) -> Result<Configuration, ArgParsingError> {
    match create_config_builder_from_args(line) {
        Ok(config_builder) => Ok(config_builder.build()),
        Err(x) => Err(x)
    }
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
    let (mut exclude_dirs, mut languages_of_interest, mut excluded_languages, mut threads, mut braces_as_code,
         mut search_in_dotted, mut show_faulty_files, mut config_name_to_save, mut hidden, mut log,
         mut compare_level, mut config_name_to_load, mut no_gitignore, mut colors, mut color_palette, mut styles, mut bar_thickness, mut sort_by, mut top_n)
         = (None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None, None);
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
        } else if command_name == COLORS {
            match utils::parse_colors_to_vec(arguments) {
                Some(x) => colors = Some(x),
                None => {
                    message_printer::print_help_message_for_command(COLORS);
                    return Err(ArgParsingError::IncorrectCommandArgs(COLORS.to_owned()))
                }
            }
        } else if command_name == COLOR_PALETTE {
            let name = arguments.trim();
            if name.is_empty() {
                message_printer::print_help_message_for_command(COLOR_PALETTE);
                return Err(ArgParsingError::IncorrectCommandArgs(COLOR_PALETTE.to_owned()))
            }
            if io_handler::load_palette(name, &crate::PERSISTENT_APP_PATHS.palettes_dir).is_none() {
                return Err(ArgParsingError::NonExistantPalette(name.to_owned()))
            }
            color_palette = Some(name.to_owned());
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

            if let Ok((mut options, invalid_fields)) = io_handler::parse_config_file(Some(config_name), None) {
                if let Some(dirs) = &options.dirs {
                    match resolve_target_paths(dirs, respect_gitignore, dotted_are_targetable) {
                        Ok(x) => options.dirs = Some(x),
                        Err(ArgParsingError::InvalidPath(p)) | Err(ArgParsingError::InvalidGlobPattern(p))
                                | Err(ArgParsingError::NoGlobMatches(p)) | Err(ArgParsingError::AllGlobMatchesIgnored(p)) =>
                                return Err(ArgParsingError::InvalidPathInConfig(p, config_name.to_owned())),
                        Err(x) => return Err(x)
                    }
                }
                custom_config = Some((options, invalid_fields));
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
        } else {
            return Err(ArgParsingError::UnrecognisedCommand(command.to_owned()));
        }
    }

    print_warnings_for_commands_that_need_a_loaded_configuration(&config_name_to_save, &config_name_to_load, &log, &compare_level);

    let mut config_builder = ConfigurationBuilder {
        dirs, exclude_dirs, languages_of_interest, excluded_languages, threads, braces_as_code,
        should_search_in_dotted: search_in_dotted, should_show_faulty_files: show_faulty_files,
        hidden, no_gitignore, colors, color_palette, log, compare_level,
        config_name_to_save, config_name_to_load, styles, bar_thickness, sort_by, top_n, palette_styles: None
    };

    if let Some((custom, invalid_fields)) = custom_config {
        let config_name = config_builder.config_name_to_load.clone().unwrap_or_default();
        resolve_invalid_config_fields(&config_builder, &invalid_fields, &config_name)?;
        config_builder.add_missing_fields(custom);
    }

    if let Some(name) = &config_builder.config_name_to_save {
        if config_builder.dirs.is_none() {
            config_builder.dirs = Some(parse_working_dir_as_target_dir()?);
        }

        match io_handler::save_existing_commands_from_config_builder_to_file(None, name, &config_builder) {
            Err(_) => println!("\n{}","Error while trying to save config.".yellow()),
            Ok(_) => println!("\nConfiguration '{name}' saved successfully.")
        }
    }

    if config_builder.has_missing_fields()
        && let Ok((default_config, invalid_fields)) = io_handler::parse_config_file(None, None) {
        resolve_invalid_config_fields(&config_builder, &invalid_fields, "default")?;
        config_builder.add_missing_fields(default_config);
    }

    // A palette contributes its language colors only when they are not overridden, but its style
    // tokens always apply, since --colors speaks about the overview alone
    if let Some(name) = &config_builder.color_palette {
        match io_handler::load_palette(name, &crate::PERSISTENT_APP_PATHS.palettes_dir) {
            Some(palette) => {
                if config_builder.colors.is_none() {
                    config_builder.colors = palette.languages;
                }
                config_builder.palette_styles = Some(palette.styles);
            },
            None => println!("\n{}", format!("Color palette '{name}' could not be loaded, the default colors will be used.").yellow())
        }
    }

    if config_builder.dirs.is_none() {
        config_builder.dirs = Some(parse_working_dir_as_target_dir()?);
    }

    Ok(config_builder)
}


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
            COLORS => config_builder.colors.is_some(),
            COLOR_PALETTE => config_builder.color_palette.is_some(),
            _ => false
        };

        if is_overridden {
            println!("\n{}", format!("Invalid value for the command '--{field}', in config '{config_name}'. The value will be ignored.").yellow());
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
            println!("\n{}","'--log' command will be ignored, since no config file was specified.".yellow());
        }

        if compare_level.is_some() {
            println!("\n{}","'--compare' command will be ignored, since no config file was specified for loading.".yellow());
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
    pub threads:                  Option<Threads>,
    pub braces_as_code:           Option<bool>,
    pub should_search_in_dotted:  Option<bool>,
    pub should_show_faulty_files: Option<bool>,
    pub hidden:                   Option<Hidden>,
    pub no_gitignore:             Option<bool>,
    pub colors:                   Option<Vec<Color>>,
    pub color_palette:            Option<String>,
    pub log:                      Option<LogOption>,
    pub compare_level:            Option<usize>,
    pub config_name_to_save:      Option<String>,
    pub config_name_to_load:      Option<String>,
    pub bar_thickness:            Option<BarThickness>,
    pub sort_by:                  Option<SortCriterion>,
    pub top_n:                    Option<usize>,
    pub styles:                   Option<Vec<(String,String)>>,
    pub palette_styles:           Option<Vec<(String,String)>>
}

impl ConfigurationBuilder {
    pub fn add_missing_fields(&mut self, config: Self) -> &mut Self {
        if self.dirs.is_none() {self.dirs = config.dirs};
        if self.exclude_dirs.is_none() {self.exclude_dirs = config.exclude_dirs};
        if self.languages_of_interest.is_none() {self.languages_of_interest = config.languages_of_interest};
        if self.excluded_languages.is_none() {self.excluded_languages = config.excluded_languages};
        if self.threads.is_none() {self.threads = config.threads};
        if self.braces_as_code.is_none() {self.braces_as_code = config.braces_as_code};
        if self.should_search_in_dotted.is_none() {self.should_search_in_dotted = config.should_search_in_dotted};
        if self.should_show_faulty_files.is_none() {self.should_show_faulty_files = config.should_show_faulty_files};
        if self.hidden.is_none() {self.hidden = config.hidden};
        if self.no_gitignore.is_none() {self.no_gitignore = config.no_gitignore};
        if self.colors.is_none() {self.colors = config.colors};
        if self.color_palette.is_none() {self.color_palette = config.color_palette};
        if self.compare_level.is_none() {self.compare_level = config.compare_level};
        if self.log.is_none() {self.log = config.log};
        if self.styles.is_none() {self.styles = config.styles};
        if self.bar_thickness.is_none() {self.bar_thickness = config.bar_thickness};
        if self.sort_by.is_none() {self.sort_by = config.sort_by};
        if self.top_n.is_none() {self.top_n = config.top_n};
        self
    }

    pub fn has_missing_fields(&self) -> bool {
        self.exclude_dirs.is_none() || self.languages_of_interest.is_none() ||
        self.threads.is_none() || self.braces_as_code.is_none() || self.should_search_in_dotted.is_none() ||
        self.should_show_faulty_files.is_none() || self.hidden.is_none() || self.no_gitignore.is_none() ||
        self.colors.is_none() || self.color_palette.is_none() || self.log.is_none() || self.compare_level.is_none() ||
        self.styles.is_none() || self.bar_thickness.is_none() || self.sort_by.is_none()
    }

    pub fn build(&self) -> Configuration {
        Configuration {
            version: VERSION_ID,
            dirs: self.dirs.clone().unwrap(),
            exclude_dirs: (self.exclude_dirs).clone().unwrap_or_default(),
            languages_of_interest: (self.languages_of_interest).clone().unwrap_or_default(),
            excluded_languages: (self.excluded_languages).clone().unwrap_or_default(),
            threads: self.threads.clone().unwrap_or_default(),
            braces_as_code: self.braces_as_code.unwrap_or(DEF_BRACES_AS_CODE),
            should_search_in_dotted: self.should_search_in_dotted.unwrap_or(DEF_SEARCH_IN_DOTTED),
            should_show_faulty_files: self.should_show_faulty_files.unwrap_or(DEF_SHOW_FAULTY_FILES),
            hidden: self.hidden.unwrap_or_default(),
            no_gitignore: self.no_gitignore.unwrap_or(DEF_NO_GITIGNORE),
            colors: self.colors.clone().unwrap_or_default(),
            log: self.log.clone().unwrap_or_default(),
            compare_level: self.compare_level.unwrap_or(DEF_COMPARE_LEVEL),
            config_name_to_save: self.config_name_to_save.clone(),
            config_name_to_load: self.config_name_to_load.clone(),
            bar_thickness: self.bar_thickness.unwrap_or_default(),
            sort_by: self.sort_by.unwrap_or_default(),
            top_n: self.top_n,
            theme: theme::resolve(self.palette_styles.as_deref().unwrap_or_default(), self.styles.as_deref().unwrap_or_default())
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
            threads: Threads::default(),
            braces_as_code: DEF_BRACES_AS_CODE,
            should_search_in_dotted: DEF_SEARCH_IN_DOTTED,
            should_show_faulty_files: DEF_SHOW_FAULTY_FILES,
            hidden: Hidden::default(),
            no_gitignore: DEF_NO_GITIGNORE,
            colors: Vec::new(),
            log: LogOption::default(),
            compare_level: DEF_COMPARE_LEVEL,
            config_name_to_save: None,
            config_name_to_load: None,
            bar_thickness: BarThickness::default(),
            sort_by: SortCriterion::default(),
            top_n: None,
            theme: Theme::default()
        }
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

    pub fn set_colors(&mut self, colors: Vec<Color>) -> &mut Self {
        self.colors = colors;
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
        // Consumers are deliberately oversubscribed relative to the core count, so that
        // blocking file opens overlap instead of idling cores.
        if threads <= 4 {
            Threads {
                producers: 2,
                consumers: (threads * 2).clamp(3, MAX_CONSUMERS_VALUE)
            }
        } else {
            Threads {
                producers: (threads / 2).clamp(2, MAX_PRODUCERS_VALUE),
                consumers: (threads * 2).clamp(3, MAX_CONSUMERS_VALUE)
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
            Self::UnrecognisedCommand(p) => format!("--{p} is not recognised as a command.").red(),
            Self::IncorrectCommandArgs(p) => format!("Incorrect arguments provided for the command '--{p}'.").red(),
            Self::UnexpectedCommandArgs(p) => format!("Command '--{p}' does not expect any arguments.").red(),
            Self::NonExistantConfig(p) => format!("Configuration '{p}' does not exist.").red(),
            Self::NonExistantPalette(p) => format!("Color palette '{p}' was not found, or could not be read.").red(),
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
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned())), create_config_from_args("./ --threads 9 10"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned())), create_config_from_args("./ --threads 2 31"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned())), create_config_from_args("./ --threads 9"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("threads".to_owned())), create_config_from_args("./ --threads A"));
        assert_eq!(Err(ArgParsingError::UnexpectedCommandArgs("show-faulty-files".to_owned())), create_config_from_args("./ --threads 1 1 --show-faulty-files 1"));
        assert_eq!(Err(ArgParsingError::UnexpectedCommandArgs("show-faulty-files".to_owned())), create_config_from_args("./ --threads 1 1 --show-faulty-files a"));
        assert_eq!(Err(ArgParsingError::UnexpectedCommandArgs("search-in-dotted".to_owned())), create_config_from_args("./ --threads 1 1 --search-in-dotted a"));
        assert_eq!(Err(ArgParsingError::UnexpectedCommandArgs("braces-as-code".to_owned())), create_config_from_args("./ --braces-as-code a"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("exclude".to_owned())), create_config_from_args("./ --exclude"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("exclude".to_owned())), create_config_from_args("./ --exclude   --threads 4"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("exclude".to_owned())), create_config_from_args("./ --exclude [invalid"));
        assert_eq!(Err(ArgParsingError::UnexpectedCommandArgs("no-gitignore".to_owned())), create_config_from_args("./ --no-gitignore a"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("colors".to_owned())), create_config_from_args("./ --colors"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("colors".to_owned())), create_config_from_args("./ --colors kaka"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("colors".to_owned())), create_config_from_args("./ --colors ff0000 ff0000 ff0000 ff0000 ff0000 ff0000"));
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
        assert_eq!(*new_conf("./").set_colors(vec![Color::TrueColor{r:255,g:136,b:0}, Color::BrightCyan]),
                create_config_from_args("./ --colors ff8800 bright-cyan").unwrap());
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
        let expected = Hidden {status: true, bar: true, timing: true, ..Default::default()};
        assert_eq!(expected, hidden("./ --hide status,bar,timing"));
        assert_eq!(expected, hidden("./ --hide status bar timing"));
        assert_eq!(expected, hidden("./ --hide  STATUS , bar,  Timing "));

        // The error names the entry that was not understood, instead of the whole command
        assert_eq!(Err(ArgParsingError::InvalidHideTarget("detials".to_owned())),
                create_config_from_args("./ --hide details,detials"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs(HIDE.to_owned())), create_config_from_args("./ --hide"));
        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs(HIDE.to_owned())), create_config_from_args("./ --hide   "));

        // What is written to a config file is what the command line accepts
        assert_eq!("status,bar,timing", expected.to_list_string());
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
    fn test_color_palette_arg_parsing() {
        std::fs::create_dir_all(&PERSISTENT_APP_PATHS.palettes_dir).unwrap();
        let test_palette_path = &PERSISTENT_APP_PATHS.palettes_dir.clone().add("test-palette000.txt");
        // Cleaning up front instead of asserting absence, so that a failed run does not leave
        // behind a file that makes every later run fail during setup
        let _ = std::fs::remove_file(test_palette_path);
        std::fs::write(test_palette_path, "languages = cyan ff0080\ncode-number = bright-black dim\n").unwrap();

        let config = create_config_from_args("./ --color-palette Test-Palette000").unwrap();
        assert_eq!(vec![Color::Cyan, Color::TrueColor{r:255,g:0,b:128}], config.colors);
        assert_eq!(Style::of(Color::BrightBlack).dim(), config.theme.code_number);

        // --colors speaks about the overview alone, so the palette's style tokens still apply
        let overridden = create_config_from_args("./ --color-palette test-palette000 --colors bright-blue").unwrap();
        assert_eq!(vec![Color::BrightBlue], overridden.colors);
        assert_eq!(Style::of(Color::BrightBlack).dim(), overridden.theme.code_number);

        // and --style wins over what the palette declared
        let restyled = create_config_from_args("./ --color-palette test-palette000 --style code-number=cyan,heading=bold").unwrap();
        assert_eq!(Style::of(Color::Cyan), restyled.theme.code_number);
        assert_eq!(Style::plain().bold(), restyled.theme.heading);

        // The error names what is actually wrong, instead of a generic "incorrect arguments"
        assert!(matches!(create_config_from_args("./ --style"), Err(ArgParsingError::InvalidStyle(_))));
        assert!(matches!(create_config_from_args("./ --style nonsense"), Err(ArgParsingError::InvalidStyle(_))));
        assert_eq!(Err(ArgParsingError::InvalidStyle("'code-numberr' is not a style token.".to_owned())),
                create_config_from_args("./ --style code-numberr=cyan"));
        assert!(matches!(create_config_from_args("./ --style code-number=notacolor"), Err(ArgParsingError::InvalidStyle(_))));
        assert_eq!(Err(ArgParsingError::NonExistantPalette("definitely-not-a-palette000".to_owned())),
                create_config_from_args("./ --color-palette definitely-not-a-palette000"));

        assert_eq!(Err(ArgParsingError::IncorrectCommandArgs("color-palette".to_owned())),
                create_config_from_args("./ --color-palette"));

        std::fs::remove_file(test_palette_path).unwrap();
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
}
