// Reading and writing the configuration files, which are the command line written down.
use std::{fs::{self, File}, io::{BufRead, BufReader, BufWriter, Write}};

use colored::{ColoredString, Colorize};
use mezura_core::engine::config::{Target, Threads};
use mezura_core::engine::config::{MAX_CONSUMERS_VALUE, MAX_PRODUCERS_VALUE, MIN_CONSUMERS_VALUE,
        MIN_PRODUCERS_VALUE};

use super::config_manager::{self, ConfigurationBuilder};
use super::config_manager::{MAX_COMPARE_LEVEL, MIN_COMPARE_LEVEL};
use super::message_printer::{Formatted, wrap_message};
use super::theme_files;
use crate::paths::{DEFAULT_CONFIG_NAME, PERSISTENT_APP_PATHS};

#[derive(Debug)]
pub enum ConfigFileParseError {
    FileNotFound(String),
    // An error and not a warning, because everything after that line was never seen and the blocks
    // deciding what gets counted may be among them.
    UnreadableLine(String, usize, UnreadableCause)
}

impl Formatted for ConfigFileParseError {
    fn format(&self) -> ColoredString {
        match self {
            Self::FileNotFound(x) => wrap_message(&format!("'{x}' config file not found, defaults will be used.")).yellow(),
            Self::UnreadableLine(file, line, UnreadableCause::NotUtf8) => wrap_message(&format!("Configuration '{file}' stops being readable at line {line}, so none of it was used: the file is not saved as UTF-8.")).red(),
            Self::UnreadableLine(file, line, UnreadableCause::Io(error)) => wrap_message(&format!("Configuration '{file}' could not be read past line {line}, so none of it was used: {error}")).red(),
        }
    }
}

// The two need different sentences: bytes that are not UTF-8 are a property of the file and will be
// there every time until it is re-saved, while an I/O failure belongs to this run. Blaming the
// encoding for the second sends somebody re-saving a file that was never the problem.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum UnreadableCause {
    NotUtf8,
    Io(String)
}

// 'invalid_fields' are the ones whose value decides what the program does, so a bad one stops the
// run unless the command line already overrode it. 'warnings' are the ones that only decide how the
// result looks, plus the sections nobody asked for, and they are always just said out loud.
#[derive(Debug, Default)]
pub struct ConfigFileIssues {
    pub invalid_fields: Vec<&'static str>,
    // The code travels with the message from where the kind is known, so nothing downstream has to
    // recover it by looking for a phrase inside the English.
    pub warnings: Vec<(mezura_core::warnings::Code, String)>
}

pub fn parse_config_file(file_name: Option<&str>, config_dir_path: Option<String>) -> Result<(ConfigurationBuilder, ConfigFileIssues),ConfigFileParseError> {
    let config_path = if let Some(dir) = config_dir_path {dir} else {PERSISTENT_APP_PATHS.config_dir.clone()};
    let file_name = if let Some(x) = file_name {x} else {DEFAULT_CONFIG_NAME.trim_end_matches(".txt")};
    let file_path = super::paths::normalise_separators(&(config_path + file_name + ".txt")).into_owned();
    let mut reader = CountingReader { line: 0, failed_at: None, reader: BufReader::new(match fs::File::open(file_path){
        Ok(f) => f,
        Err(_) => return Err(ConfigFileParseError::FileNotFound(file_name.to_owned()))
    })};

    let (mut targets, mut counting, mut should_search_in_dotted, mut count_minified, mut count_generated, mut threads, mut exclude_dirs,
         mut languages_of_interest, mut excluded_languages, mut forced_languages, mut should_show_faulty_files, mut hidden,
         mut no_gitignore, mut theme_name, mut compare_level, mut config_styles, mut bar_thickness,
         mut progress_bar, mut number_separator, mut decimal_separator, mut layout, mut sort_by, mut top_n,
         mut by_file) = (None,None,None,None,None,None,None,None,None,None,None,None,None,None,None,None,None,None,None,None,None,None,None,None);
    let mut issues = ConfigFileIssues::default();
    let mut buf = String::with_capacity(150);

    loop {
        let size = reader.read_line(&mut buf);
        if size == 0 {break};
        // Only the first line of the file can carry the mark, but asking on every line costs one
        // comparison and spares the loop a special case
        let line = strip_byte_order_mark(buf.trim());
        if line.starts_with("===>") {
            let id = line.trim_start_matches("===>").split_whitespace().next().unwrap_or("");

            if id == config_manager::TARGETS {
                // The line ends a target here and a space never does, so a path with one in it needs
                // no quoting. A target that does not parse would silently not be counted, so it
                // stops the run rather than warning.
                let declared = read_lines_from_file_to_vec(&mut reader, &mut buf, |line| vec![line.trim().to_owned()]);
                match super::args::parse_targets_in_block(&declared.join("\n")) {
                    Ok(parsed) if !parsed.is_empty() => targets = Some(parsed.into_iter()
                            .map(|(module, path)| Target { module, path }).collect()),
                    Ok(_) => {},
                    Err(_) => issues.invalid_fields.push(config_manager::TARGETS)
                }
            } else if id == config_manager::EXCLUDE {
                let paths = read_lines_from_file_to_vec(&mut reader, &mut buf, super::args::parse_paths_to_vec);
                if mezura_core::engine::targets::validate_exclude_patterns(&paths).is_err() {
                    issues.invalid_fields.push(config_manager::EXCLUDE);
                } else if !paths.is_empty() {
                    exclude_dirs = Some(paths);
                }
            } else if id == config_manager::LANGUAGES {
                let langs = read_lines_from_file_to_vec(&mut reader, &mut buf, super::args::parse_languages_to_vec);
                if !langs.is_empty() {
                    languages_of_interest = Some(langs);
                }
            } else if id == config_manager::EXCLUDE_LANGUAGES {
                let langs = read_lines_from_file_to_vec(&mut reader, &mut buf, super::args::parse_languages_to_vec);
                if !langs.is_empty() {
                    excluded_languages = Some(langs);
                }
            } else if id == config_manager::FORCE_LANGUAGE {
                // Read as a block like the other lists: a value written across two lines would
                // otherwise be cut to its first in silence, since the rest does not begin with
                // '===>' and the outer loop skips it.
                let declared = read_lines_from_file_to_vec(&mut reader, &mut buf, |line| vec![line.trim().to_owned()]).join(",");
                // An empty value is the command left in the file without being used, which is not a
                // mistake. Anything else that does not parse is one.
                if declared.split(',').any(|pair| !pair.trim().is_empty()) {
                    match super::args::parse_forced_languages(&declared) {
                        Some(x) => forced_languages = Some(x),
                        None => issues.invalid_fields.push(config_manager::FORCE_LANGUAGE)
                    }
                }
            } else if id == config_manager::THREADS {
                buf.clear();
                let _ = reader.read_line(&mut buf);
                match super::args::parse_two_usize_values(&buf,MIN_PRODUCERS_VALUE, MAX_PRODUCERS_VALUE,
                        MIN_CONSUMERS_VALUE, MAX_CONSUMERS_VALUE) {
                    Some(x) => threads = Some(Threads::from(x)),
                    None => issues.invalid_fields.push(config_manager::THREADS)
                }
            } else if id == config_manager::COUNTING {
                buf.clear();
                let _ = reader.read_line(&mut buf);
                match mezura_core::CountingModel::parse(&buf) {
                    Some(x) => counting = Some(x),
                    None => issues.invalid_fields.push(config_manager::COUNTING)
                }
            } else if id == config_manager::SHOW_FAULTY_FILES {
                match read_bool_value_from_file(&mut reader, &mut buf) {
                    Ok(x) => should_show_faulty_files = x,
                    Err(()) => issues.invalid_fields.push(config_manager::SHOW_FAULTY_FILES)
                }
            } else if id == config_manager::SEARCH_IN_DOTTED {
                match read_bool_value_from_file(&mut reader, &mut buf) {
                    Ok(x) => should_search_in_dotted = x,
                    Err(()) => issues.invalid_fields.push(config_manager::SEARCH_IN_DOTTED)
                }
            } else if id == config_manager::COUNT_MINIFIED {
                match read_bool_value_from_file(&mut reader, &mut buf) {
                    Ok(x) => count_minified = x,
                    Err(()) => issues.invalid_fields.push(config_manager::COUNT_MINIFIED)
                }
            } else if id == config_manager::COUNT_GENERATED {
                match read_bool_value_from_file(&mut reader, &mut buf) {
                    Ok(x) => count_generated = x,
                    Err(()) => issues.invalid_fields.push(config_manager::COUNT_GENERATED)
                }
            } else if id == config_manager::HIDE {
                buf.clear();
                let _ = reader.read_line(&mut buf);
                match config_manager::Hidden::parse(&buf) {
                    Ok(x) => hidden = Some(x),
                    Err(_) => issues.invalid_fields.push(config_manager::HIDE)
                }
            } else if id == config_manager::NO_GITIGNORE {
                match read_bool_value_from_file(&mut reader, &mut buf) {
                    Ok(x) => no_gitignore = x,
                    Err(()) => issues.invalid_fields.push(config_manager::NO_GITIGNORE)
                }
            } else if id == config_manager::THEME {
                buf.clear();
                let _ = reader.read_line(&mut buf);
                let name = buf.trim();
                if name.is_empty() || theme_files::load_theme(name, &PERSISTENT_APP_PATHS.themes_dir).is_none() {
                    issues.invalid_fields.push(config_manager::THEME);
                } else {
                    theme_name = Some(name.to_owned());
                }
            } else if id == config_manager::SORT {
                buf.clear();
                let _ = reader.read_line(&mut buf);
                match config_manager::SortCriterion::parse(&buf) {
                    Some(x) => sort_by = Some(x),
                    None => issues.invalid_fields.push(config_manager::SORT)
                }
            } else if id == config_manager::TOP {
                buf.clear();
                let _ = reader.read_line(&mut buf);
                match super::args::parse_usize_value(&buf, 1, usize::MAX) {
                    Some(x) => top_n = Some(x),
                    None => issues.invalid_fields.push(config_manager::TOP)
                }
            } else if id == config_manager::BY_FILE {
                buf.clear();
                let _ = reader.read_line(&mut buf);
                match config_manager::ByFile::parse(&buf) {
                    Some(x) => by_file = Some(x),
                    None => issues.invalid_fields.push(config_manager::BY_FILE)
                }
            } else if id == config_manager::BAR_THICKNESS {
                buf.clear();
                let _ = reader.read_line(&mut buf);
                match config_manager::BarThickness::parse(&buf) {
                    Some(x) => bar_thickness = Some(x),
                    None => issues.invalid_fields.push(config_manager::BAR_THICKNESS)
                }
            } else if id == config_manager::PROGRESS_BAR {
                buf.clear();
                let _ = reader.read_line(&mut buf);
                match config_manager::ProgressBarStyle::parse(&buf) {
                    Some(x) => progress_bar = Some(x),
                    None => issues.invalid_fields.push(config_manager::PROGRESS_BAR)
                }
            } else if id == config_manager::NUMBER_SEPARATOR {
                buf.clear();
                let _ = reader.read_line(&mut buf);
                match config_manager::NumberSeparator::parse(&buf) {
                    Some(x) => number_separator = Some(x),
                    None => issues.invalid_fields.push(config_manager::NUMBER_SEPARATOR)
                }
            } else if id == config_manager::DECIMAL_SEPARATOR {
                buf.clear();
                let _ = reader.read_line(&mut buf);
                match config_manager::DecimalSeparator::parse(&buf) {
                    Some(x) => decimal_separator = Some(x),
                    None => issues.invalid_fields.push(config_manager::DECIMAL_SEPARATOR)
                }
            } else if id == config_manager::LAYOUT {
                buf.clear();
                let _ = reader.read_line(&mut buf);
                match config_manager::Layout::parse(&buf) {
                    Some(x) => layout = Some(x),
                    None => issues.invalid_fields.push(config_manager::LAYOUT)
                }
            } else if id == config_manager::STYLE {
                let declared = read_lines_from_file_to_vec(&mut reader, &mut buf, |line| vec![line.trim().to_owned()]);
                let (declared, errors) = super::theme::parse_overrides_leniently(&declared.join("\n"));
                issues.warnings.extend(errors.iter().map(|x| (mezura_core::warnings::Code::ConfigStyleInvalid, x.format())));
                if !declared.is_empty() {
                    config_styles = Some(declared);
                }
            } else if id == config_manager::COMPARE_LEVEL {
                buf.clear();
                let _ = reader.read_line(&mut buf);
                match super::args::parse_usize_value(&buf,MIN_COMPARE_LEVEL, MAX_COMPARE_LEVEL) {
                    Some(x) => compare_level = Some(x),
                    None => issues.invalid_fields.push(config_manager::COMPARE_LEVEL)
                }
            } else {
                issues.warnings.push((mezura_core::warnings::Code::ConfigSectionUnknown,
                        format!("'{id}' is not something a configuration file can carry, the section is ignored.")));
            }
        }
        buf.clear();
    }

    let builder = ConfigurationBuilder {
        targets, exclude_dirs, languages_of_interest, excluded_languages, forced_languages, threads, counting, should_search_in_dotted,
        count_minified, count_generated, should_show_faulty_files, hidden, no_gitignore, theme_name, compare_level, config_styles, bar_thickness,
        progress_bar, number_separator, decimal_separator, layout, sort_by, top_n, by_file,
        ..Default::default()
    };

    // After the whole file has been walked, so that the line reported is the first failure and not
    // whichever one a block inside happened to meet
    if let Some((line, error)) = reader.failed_at {
        let cause = if error.kind() == std::io::ErrorKind::InvalidData {UnreadableCause::NotUtf8}
                else {UnreadableCause::Io(error.to_string())};
        return Err(ConfigFileParseError::UnreadableLine(file_name.to_owned(), line, cause));
    }

    Ok((builder, issues))
}

// Three bytes that mean "this is UTF-8" and carry no text. 'trim' leaves them where they are, since
// they are not whitespace, so a header on the first line of a file stops matching the moment
// somebody re-saves it with PowerShell's 'Set-Content' or an older Notepad. Every parser of a text
// format in this crate calls it.
pub fn strip_byte_order_mark(contents: &str) -> &str {
    contents.trim_start_matches('\u{feff}')
}

// The targets must be set before this is called: they are unwrapped below.
pub fn save_existing_commands_from_config_builder_to_file(config_path: Option<String>, config_name: &str, config_builder: &ConfigurationBuilder)
-> std::io::Result<()> 
{
    let config_dir = if let Some(dir) = config_path {dir} else {PERSISTENT_APP_PATHS.config_dir.clone()};
    let file_name = config_dir + config_name + ".txt";

    let mut writer = BufWriter::new(std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(file_name)?);

    writer.write_all(b"Auto-generated config file.")?;

    // One target per line, which is what the block reader expects: a module name only reaches the
    // paths written after it when a comma joins them.
    writer.write_all(&[b"\n\n===> ",config_manager::TARGETS.as_bytes(),b"\n"].concat())?;
    writer.write_all(config_builder.targets.as_ref().unwrap().iter().map(config_manager::format_declared_form)
            .collect::<Vec<_>>().join("\n").as_bytes())?;

    if let Some(exclude_dirs) = &config_builder.exclude_dirs {
        writer.write_all(&[b"\n\n===> ",config_manager::EXCLUDE.as_bytes(),b"\n"].concat())?;
        writer.write_all(exclude_dirs.join(",").as_bytes())?;
    }
    if let Some(languages_of_interest) = &config_builder.languages_of_interest {
        writer.write_all(&[b"\n\n===> ",config_manager::LANGUAGES.as_bytes(),b"\n"].concat())?;
        writer.write_all(languages_of_interest.join(",").as_bytes())?;
    }
    if let Some(exclude_languages) = &config_builder.excluded_languages {
        writer.write_all(&[b"\n\n===> ",config_manager::EXCLUDE_LANGUAGES.as_bytes(),b"\n"].concat())?;
        writer.write_all(exclude_languages.join(",").as_bytes())?;
    }
    if let Some(forced_languages) = &config_builder.forced_languages {
        writer.write_all(&[b"\n\n===> ",config_manager::FORCE_LANGUAGE.as_bytes(),b"\n"].concat())?;
        writer.write_all(super::args::forced_languages_to_string(forced_languages).as_bytes())?;
    }
    if let Some(threads) = &config_builder.threads {
        writer.write_all(&[b"\n\n===> ",config_manager::THREADS.as_bytes(),b"\n"].concat())?;
        writer.write_all((threads.producers().to_string() + " " + &threads.consumers().to_string()).as_bytes())?;
    }
    if let Some(counting) = &config_builder.counting {
        writer.write_all(&[b"\n\n===> ",config_manager::COUNTING.as_bytes(),b"\n"].concat())?;
        writer.write_all(counting.name().as_bytes())?;
    }
    if let Some(should_search_in_dotted) = &config_builder.should_search_in_dotted {
        writer.write_all(&[b"\n\n===> ",config_manager::SEARCH_IN_DOTTED.as_bytes(),b"\n"].concat())?;
        writer.write_all(if *should_search_in_dotted {b"yes"} else {b"no"})?;
    }
    if let Some(count_minified) = &config_builder.count_minified {
        writer.write_all(&[b"\n\n===> ",config_manager::COUNT_MINIFIED.as_bytes(),b"\n"].concat())?;
        writer.write_all(if *count_minified {b"yes"} else {b"no"})?;
    }
    if let Some(count_generated) = &config_builder.count_generated {
        writer.write_all(&[b"\n\n===> ",config_manager::COUNT_GENERATED.as_bytes(),b"\n"].concat())?;
        writer.write_all(if *count_generated {b"yes"} else {b"no"})?;
    }
    if let Some(should_show_faulty_files) = &config_builder.should_show_faulty_files {
        writer.write_all(&[b"\n\n===> ",config_manager::SHOW_FAULTY_FILES.as_bytes(),b"\n"].concat())?;
        writer.write_all(if *should_show_faulty_files {b"yes"} else {b"no"})?;
    }
    if let Some(hidden) = &config_builder.hidden {
        writer.write_all(&[b"\n\n===> ",config_manager::HIDE.as_bytes(),b"\n"].concat())?;
        writer.write_all(hidden.to_list_string().as_bytes())?;
    }
    if let Some(no_gitignore) = &config_builder.no_gitignore {
        writer.write_all(&[b"\n\n===> ",config_manager::NO_GITIGNORE.as_bytes(),b"\n"].concat())?;
        writer.write_all(if *no_gitignore {b"yes"} else {b"no"})?;
    }
    if let Some(sort_by) = &config_builder.sort_by {
        writer.write_all(&[b"

===> ",config_manager::SORT.as_bytes(),b"
"].concat())?;
        writer.write_all(sort_by.name().as_bytes())?;
    }

    if let Some(top_n) = &config_builder.top_n {
        writer.write_all(&[b"

===> ",config_manager::TOP.as_bytes(),b"
"].concat())?;
        writer.write_all(top_n.to_string().as_bytes())?;
    }

    if let Some(by_file) = &config_builder.by_file {
        writer.write_all(&[b"\n\n===> ",config_manager::BY_FILE.as_bytes(),b"\n"].concat())?;
        writer.write_all(by_file.to_text().as_bytes())?;
    }

    if let Some(bar_thickness) = &config_builder.bar_thickness {
        writer.write_all(&[b"

===> ",config_manager::BAR_THICKNESS.as_bytes(),b"
"].concat())?;
        writer.write_all(bar_thickness.name().as_bytes())?;
    }

    if let Some(progress_bar) = &config_builder.progress_bar {
        writer.write_all(&[b"

===> ",config_manager::PROGRESS_BAR.as_bytes(),b"
"].concat())?;
        writer.write_all(progress_bar.name().as_bytes())?;
    }

    if let Some(number_separator) = &config_builder.number_separator {
        writer.write_all(&[b"\n\n===> ",config_manager::NUMBER_SEPARATOR.as_bytes(),b"\n"].concat())?;
        writer.write_all(number_separator.name().as_bytes())?;
    }

    if let Some(decimal_separator) = &config_builder.decimal_separator {
        writer.write_all(&[b"\n\n===> ",config_manager::DECIMAL_SEPARATOR.as_bytes(),b"\n"].concat())?;
        writer.write_all(decimal_separator.name().as_bytes())?;
    }

    if let Some(layout) = &config_builder.layout {
        writer.write_all(&[b"\n\n===> ",config_manager::LAYOUT.as_bytes(),b"\n"].concat())?;
        writer.write_all(layout.name().as_bytes())?;
    }

    // The two style layers collapse into one block, in the order they were applied, so that
    // reloading the file reproduces what the run looked like. When '--save-theme' is writing a theme
    // in the same run they are already inside it and would only be said twice.
    let styles = if config_builder.theme_name_to_save.is_some() {Vec::new()}
            else {config_builder.config_styles.iter().chain(config_builder.styles.iter()).flatten().collect::<Vec<_>>()};
    if !styles.is_empty() {
        let rendered = styles.iter().map(|(token, style)| format!("{token} = {style}")).collect::<Vec<_>>().join("\n");
        writer.write_all(&[b"\n\n===> ",config_manager::STYLE.as_bytes(),b"\n"].concat())?;
        writer.write_all(rendered.as_bytes())?;
    }

    // A theme that --save-theme is writing in the same run is the one this config should point at
    if let Some(theme_name) = config_builder.theme_name_to_save.as_ref().or(config_builder.theme_name.as_ref()) {
        writer.write_all(&[b"\n\n===> ",config_manager::THEME.as_bytes(),b"\n"].concat())?;
        writer.write_all(theme_name.as_bytes())?;
    }
    if let Some(compare_level) = &config_builder.compare_level {
        writer.write_all(&[b"\n\n===> ",config_manager::COMPARE_LEVEL.as_bytes(),b"\n"].concat())?;
        writer.write_all(compare_level.to_string().as_bytes())?;
    }

    writer.write_all(b"\n")?;
    writer.flush()?;

    Ok(())
}

pub fn read_names_in_dir(dir: &str) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut names = entries.flatten().filter(|x| x.path().is_file())
            .filter_map(|x| x.path().file_stem().and_then(|x| x.to_str()).map(str::to_owned)).collect::<Vec<_>>();
    names.sort_by_key(|x| x.to_lowercase());
    names
}

// Counts lines, and remembers a line it could not deliver instead of losing it. From then on it
// reads as an ended file, which every caller already treats as the end of its block, so nothing
// after the bad line is applied and the file is refused whole by the single check at the end.
struct CountingReader {
    reader: BufReader<File>,
    line: usize,
    failed_at: Option<(usize, std::io::Error)>
}

impl CountingReader {
    fn read_line(&mut self, buf: &mut String) -> usize {
        if self.failed_at.is_some() {
            return 0;
        }
        self.line += 1;
        match self.reader.read_line(buf) {
            Ok(0) => { self.line -= 1; 0 },
            Ok(size) => size,
            Err(error) => { self.failed_at = Some((self.line, error)); 0 }
        }
    }
}

fn read_bool_value_from_file(reader: &mut CountingReader, buf: &mut String) -> Result<Option<bool>, ()> {
    buf.clear();
    let _ = reader.read_line(buf);
    let buf = buf.trim();
    if buf.is_empty() {
        return Ok(None);
    }
    let buf = buf.to_ascii_lowercase();
    if buf == "yes" || buf ==  "true" {
        Ok(Some(true))
    } else if buf == "no" || buf == "false" {
        Ok(Some(false))
    } else {
        Err(())
    }
}

fn read_lines_from_file_to_vec<T>(reader: &mut CountingReader, buf: &mut String, parser_func: fn(&str) -> Vec<T>) -> Vec<T> {
    let mut vec = Vec::new();
    loop {
        buf.clear();
        let _ = reader.read_line(buf);
        if buf.trim().is_empty() {
            break;
        }
        let new_vec = parser_func(buf);
        vec.extend(new_vec);
    }
    vec
}

#[cfg(test)]
mod tests {
    use mezura_core::Target;

    use super::super::config_manager::Configuration;
    use super::*;
    use crate::paths::test_paths::{FIXTURES_DIR, SCRATCH_CONFIG_DIR};
    use super::super::config_manager::ConfigurationBuilder;
    // The realistic cause is not a failing disk, it is an editor writing a path with non-ASCII
    // characters as something other than UTF-8.
    #[test]
    fn a_config_that_stops_being_readable_mid_file_is_an_error_not_a_half_applied_config() {
        let dir = std::env::temp_dir().join("mezura_unreadable_config_test");
        std::fs::create_dir_all(&dir).unwrap();
        let dir_str = dir.to_str().unwrap().to_owned() + "/";

        let mut contents = b"===> threads\n2 8\n\n===> targets\n".to_vec();
        contents.extend([0xCF, 0xE1, 0xE8, 0xFF, b'\n']);
        contents.extend(b"\n===> counting\nregion\n");
        std::fs::write(dir.join("halfway.txt"), contents).unwrap();

        let result = super::super::config_files::parse_config_file(Some("halfway"), Some(dir_str));
        std::fs::remove_dir_all(&dir).unwrap();

        match result {
            Err(ConfigFileParseError::UnreadableLine(file, line, cause)) => {
                assert_eq!(("halfway", 5), (file.as_str(), line));
                assert_eq!(UnreadableCause::NotUtf8, cause);
            },
            other => panic!("the half-readable config was not refused: {other:?}")
        }
    }

    #[test]
    fn a_command_line_written_to_a_configuration_reads_back_as_the_same_command_line() -> std::io::Result<()> {
        let command = "./ --exclude a,b,c.txt,d.txt, --counting region --threads 1 1 --hide keywords,timing \
                --force-language m=matlab,.pl=Perl --by-file 12 --count-minified --count-generated \
                --style code-number=green,comments-label=magenta bold,arrow=default dim".to_string();
        let config_builder = config_manager::create_config_builder_from_args(&command).unwrap();

        std::fs::create_dir_all(SCRATCH_CONFIG_DIR)?;
        let test_config_dir = Some(SCRATCH_CONFIG_DIR.to_owned());
        super::super::config_files::save_existing_commands_from_config_builder_to_file(test_config_dir, "auto-generated", &config_builder)?;

        let (options, issues) = super::super::config_files::parse_config_file(Some("auto-generated"), Some(SCRATCH_CONFIG_DIR.to_owned())).unwrap();
        assert!(issues.invalid_fields.is_empty() && issues.warnings.is_empty());
        assert_eq!(config_builder.targets, options.targets);
        assert_eq!(config_builder.exclude_dirs, options.exclude_dirs);
        assert_eq!(config_builder.threads, options.threads);
        assert_eq!(config_builder.counting, options.counting);
        assert_eq!(config_builder.should_show_faulty_files, options.should_show_faulty_files);
        assert_eq!(config_builder.should_search_in_dotted, options.should_search_in_dotted);
        assert_eq!(config_builder.count_minified, options.count_minified);
        assert_eq!(config_builder.count_generated, options.count_generated);
        assert_eq!(config_builder.hidden, options.hidden);
        assert_eq!(config_builder.by_file, options.by_file);
        assert_eq!(config_builder.forced_languages, options.forced_languages);
        // '.pl' keeps its dot through the round trip: an extension drops the dot when it is keyed,
        // a whole filename keeps it.
        assert_eq!(Some(hashmap!("m".to_owned() => "matlab".to_owned(), ".pl".to_owned() => "Perl".to_owned())),
                options.forced_languages);
        assert_eq!(config_builder.styles, options.config_styles);
        assert_eq!(3, options.config_styles.as_ref().unwrap().len());

        Ok(())
    }

    // A configuration that carried its own log would write an entry on every run that loads it, so
    // the section is refused like any other a file cannot carry, and said out loud.
    #[test]
    fn a_log_section_written_into_a_config_file_never_takes_effect() -> std::io::Result<()> {
        let dir = SCRATCH_CONFIG_DIR.to_owned();
        std::fs::create_dir_all(&dir)?;
        let path = dir.clone() + "carries-a-log.txt";
        std::fs::write(&path, "===> targets\n./\n\n===> log\nyes\n\n===> counting\nregion\n")?;

        let (options, issues) = super::super::config_files::parse_config_file(Some("carries-a-log"), Some(dir)).unwrap();
        assert_eq!(None, options.log);
        // the rest of the file still applies
        assert_eq!(Some(mezura_core::CountingModel::Region), options.counting);
        assert!(issues.warnings.iter().any(|(code, message)|
                *code == mezura_core::warnings::Code::ConfigSectionUnknown && message.contains("'log'")),
                "the ignored section was not named: {:?}", issues.warnings);

        let mut loaded = ConfigurationBuilder::default();
        loaded.add_missing_fields(options);
        assert!(!loaded.build().view.log.should_log);

        std::fs::remove_file(&path)
    }

    #[test]
    fn a_force_lang_value_written_across_lines_is_read_whole() -> std::io::Result<()> {
        let dir = SCRATCH_CONFIG_DIR.to_owned();
        std::fs::create_dir_all(&dir)?;
        let path = dir.clone() + "force-language-block.txt";
        std::fs::write(&path, "===> targets\n./\n\n===> force-language\nm=matlab,\npl=perl\n")?;

        let (options, issues) = super::super::config_files::parse_config_file(Some("force-language-block"), Some(dir)).unwrap();
        assert!(issues.invalid_fields.is_empty());
        assert_eq!(Some(hashmap!("m".to_owned() => "matlab".to_owned(), "pl".to_owned() => "perl".to_owned())),
                options.forced_languages);

        std::fs::remove_file(&path)
    }

    // Both files are written because only the first line can carry the mark, and a mark in front of
    // the first '===>' would make that block no block at all: its settings would be read as loose
    // text and dropped, leaving the run on the defaults without a word.
    #[test]
    fn a_configuration_saved_with_a_byte_order_mark_still_reads() -> std::io::Result<()> {
        let dir = SCRATCH_CONFIG_DIR.to_owned();
        std::fs::create_dir_all(&dir)?;
        let body = "===> targets\n./\n\n===> exclude-languages\njava\n";

        let plain = dir.clone() + "no-mark.txt";
        let marked = dir.clone() + "with-mark.txt";
        std::fs::write(&plain, body)?;
        std::fs::write(&marked, "\u{feff}".to_owned() + body)?;

        let (without, _) = super::super::config_files::parse_config_file(Some("no-mark"), Some(dir.clone())).unwrap();
        let (with, issues) = super::super::config_files::parse_config_file(Some("with-mark"), Some(dir)).unwrap();

        assert!(issues.invalid_fields.is_empty());
        assert_eq!(without.targets, with.targets, "the targets of the file were dropped, and in silence");
        assert_eq!(without.excluded_languages, with.excluded_languages);
        assert_eq!(Some(vec!["java".to_owned()]), with.excluded_languages);

        std::fs::remove_file(&plain).and(std::fs::remove_file(&marked))
    }

    #[test]
    fn the_modules_of_a_saved_configuration_survive_being_read_back() -> std::io::Result<()> {
        let dir = SCRATCH_CONFIG_DIR.to_owned();
        std::fs::create_dir_all(&dir)?;
        let path = dir.clone() + "modules-round-trip.txt";

        // The last two are the ones that break: an unnamed target after a named one, and a path
        // with a space in it, whitespace being what separates one target from the next
        let declared = vec![Target::named("frontend", "D:/x/web"),
                Target::named("frontend", "D:/x/ui"),
                Target::named("backend", "D:/x/my api"),
                Target::of("D:/x/loose")];
        let builder = ConfigurationBuilder { targets: Some(declared.clone()), ..Default::default() };
        save_existing_commands_from_config_builder_to_file(Some(dir.clone()), "modules-round-trip", &builder)?;

        let (options, issues) = parse_config_file(Some("modules-round-trip"), Some(dir)).unwrap();
        assert!(issues.invalid_fields.is_empty());
        assert_eq!(Some(declared), options.targets);

        std::fs::remove_file(&path)
    }

    #[test]
    fn the_targets_block_reads_a_module_across_lines_and_refuses_one_with_no_path() -> std::io::Result<()> {
        let dir = SCRATCH_CONFIG_DIR.to_owned();
        std::fs::create_dir_all(&dir)?;
        let path = dir.clone() + "targets-block.txt";
        std::fs::write(&path, "===> targets\ntests=D:/x/api/tests\ntests=D:/x/web/tests\nbackend=D:/x/api\n")?;

        let (options, issues) = parse_config_file(Some("targets-block"), Some(dir.clone())).unwrap();
        assert!(issues.invalid_fields.is_empty());
        assert_eq!(Some(vec![Target::named("tests", "D:/x/api/tests"),
                Target::named("tests", "D:/x/web/tests"),
                Target::named("backend", "D:/x/api")]), options.targets);

        std::fs::write(&path, "===> targets\ntests=D:/x/api/tests,\nD:/x/web/tests\n")?;
        let (options, _) = parse_config_file(Some("targets-block"), Some(dir.clone())).unwrap();
        assert_eq!(Some(vec![Target::named("tests", "D:/x/api/tests"),
                Target::named("tests", "D:/x/web/tests")]), options.targets);

        std::fs::write(&path, "===> targets\nfrontend=\n")?;
        let (options, issues) = parse_config_file(Some("targets-block"), Some(dir)).unwrap();
        assert_eq!(vec![config_manager::TARGETS], issues.invalid_fields);
        assert_eq!(None, options.targets);

        std::fs::remove_file(&path)
    }

    #[test]
    fn every_block_of_a_configuration_file_reaches_the_setting_it_names() -> std::io::Result<()> {
        let mut config = Configuration::new(vec![]);
        let declared_targets = vec![Target::of("C:/Some/Path/a"), Target::of("C:/Some/Path/b"),
                Target::of("C:/Some/Path/c"), Target::of("C:/Some/Path/d")];
        config.engine.exclude_dirs = vec!["a".to_owned(), "b".to_owned(), "c.txt".to_owned(), "d.txt".to_owned()];
        config.engine.threads = mezura_core::Threads::new(1, 1);
        config.view.counting = mezura_core::CountingModel::Region;
        config
            .set_hidden(config_manager::Hidden {bar: true, timing: true, ..Default::default()});

        let (options, issues) = super::super::config_files::parse_config_file(Some("test"), Some(FIXTURES_DIR.to_owned() + "config/")).unwrap();
        assert!(issues.invalid_fields.is_empty() && issues.warnings.is_empty());
        assert_eq!(declared_targets, options.targets.unwrap());
        assert_eq!(config.engine.exclude_dirs, options.exclude_dirs.unwrap());
        assert_eq!(config.engine.threads, options.threads.unwrap());
        assert_eq!(config.view.counting, options.counting.unwrap());
        // Against the value the file holds and not against a default-built configuration: both of
        // these default to false, so comparing them that way proves the block was seen and nothing
        // about where its value went
        assert_eq!(Some(true), options.should_show_faulty_files);
        assert_eq!(Some(true), options.should_search_in_dotted);
        assert_eq!(config.view.hidden, options.hidden.unwrap());

        Ok(())
    }
    #[test]
    fn the_default_configuration_is_found_by_its_name_and_parsed() {
        let dir = std::env::temp_dir().join("mezura_default_config_test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("default.txt"), "===> exclude-languages\nSQL\n").unwrap();

        let (options, issues) = super::super::config_files::parse_config_file(None, Some(dir.to_str().unwrap().to_owned() + "/")).unwrap();
        assert!(issues.invalid_fields.is_empty() && issues.warnings.is_empty());
        assert_eq!(Some(vec!["sql".to_owned()]), options.excluded_languages);

        std::fs::remove_dir_all(&dir).unwrap();
    }
    #[test]
    fn a_value_a_configuration_cannot_carry_is_named_rather_than_taken() {
        let dir = std::env::temp_dir().join("mezura_invalid_config_test");
        std::fs::create_dir_all(&dir).unwrap();
        let dir_str = dir.to_str().unwrap().to_owned() + "/";

        std::fs::write(dir.join("badcfg.txt"),
                "===> threads\n3343 45534\n\n===> counting\nmitsos\n\n===> compare\n99\n\n===> hide\nkeywords\n\n===> sort\nnope\n").unwrap();

        let (options, issues) = super::super::config_files::parse_config_file(Some("badcfg"), Some(dir_str)).unwrap();
        assert_eq!(issues.invalid_fields, vec![config_manager::THREADS, config_manager::COUNTING,
                config_manager::COMPARE_LEVEL, config_manager::SORT]);
        assert!(issues.warnings.is_empty());
        assert_eq!(options.threads, None);
        assert_eq!(options.counting, None);
        assert_eq!(options.compare_level, None);
        assert_eq!(options.sort_by, None);
        assert_eq!(options.hidden, Some(config_manager::Hidden {keywords: true, ..Default::default()}));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_unknown_section_or_a_broken_style_warns_and_the_rest_of_the_file_still_applies() {
        let dir = std::env::temp_dir().join("mezura_warning_config_test");
        std::fs::create_dir_all(&dir).unwrap();
        let dir_str = dir.to_str().unwrap().to_owned() + "/";

        std::fs::write(dir.join("warncfg.txt"),
                "===> mpampis\nwhatever\n\n===> style\ncode-number = green\nlabell = cyan\nheading = nope\narrow = dim\n\n===> sort\nname\n").unwrap();

        let (options, issues) = super::super::config_files::parse_config_file(Some("warncfg"), Some(dir_str)).unwrap();
        assert!(issues.invalid_fields.is_empty());
        assert_eq!(3, issues.warnings.len());
        assert!(issues.warnings[0].1.contains("mpampis"));
        assert!(issues.warnings[1].1.contains("labell"));
        assert!(issues.warnings[2].1.contains("heading"));

        assert_eq!(Some(vec![("code-number".to_owned(), "green".to_owned()), ("arrow".to_owned(), "dim".to_owned())]), options.config_styles);
        assert_eq!(Some(config_manager::SortCriterion::Name), options.sort_by);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}

