// Reading and writing the configuration files, which are the command line written down.
use std::{fs::{self, File}, io::{BufRead, BufReader, BufWriter, Write}};

use colored::{ColoredString, Colorize};

use super::formatted::Formatted;
use crate::paths::PERSISTENT_APP_PATHS;
use crate::paths::DEFAULT_CONFIG_NAME;
use super::theme_files;
use super::config_manager::{self, ConfigurationBuilder, LogOption, MAX_COMPARE_LEVEL, MIN_COMPARE_LEVEL};
use mezura::engine::config::{MAX_CONSUMERS_VALUE, MAX_PRODUCERS_VALUE, MIN_CONSUMERS_VALUE, MIN_PRODUCERS_VALUE};
use mezura::engine::config::{Target, Threads};

#[derive(Debug)]
pub enum ConfigFileParseError {
    FileNotFound(String),
    // The file and the number of the first line the reader could not deliver. An error and not a
    // warning, because everything after that line was never seen, and blocks that decide what gets
    // counted may be among what was lost: a half-applied configuration is a wrong answer wearing a
    // valid one's clothes.
    UnreadableLine(String, usize, UnreadableCause)
}

// The two things that can actually stop 'read_line', and they deserve different sentences: bytes
// that are not UTF-8 are a permanent property of the file, there every time until it is re-saved,
// while an I/O failure is an event of this run, a network drive dropping or a disk failing, and has
// nothing to do with the line's content. Blaming the encoding for the second would send somebody
// re-saving a file that was never the problem.
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
    // The code travels with the message from where the kind is known. Recovering it later by
    // looking for a phrase inside the English would tie the machine readable half of a warning to
    // the wording of the human one, which is the exact coupling the pair exists to avoid.
    pub warnings: Vec<(&'static str, String)>
}

pub fn parse_config_file(file_name: Option<&str>, config_dir_path: Option<String>) -> Result<(ConfigurationBuilder, ConfigFileIssues),ConfigFileParseError> {
    let config_path = if let Some(dir) = config_dir_path {dir} else {PERSISTENT_APP_PATHS.config_dir.clone()};
    let file_name = if let Some(x) = file_name {x} else {DEFAULT_CONFIG_NAME.trim_end_matches(".txt")};
    let file_path = (config_path + file_name + ".txt").replace("\\", "/");
    let mut reader = CountingReader { line: 0, failed_at: None, reader: BufReader::new(match fs::File::open(file_path){
        Ok(f) => f,
        Err(_) => return Err(ConfigFileParseError::FileNotFound(file_name.to_owned()))
    })};

    let (mut dirs, mut braces_as_code, mut should_search_in_dotted, mut threads, mut exclude_dirs,
         mut languages_of_interest, mut excluded_languages, mut forced_languages, mut should_show_faulty_files, mut hidden,
         mut no_gitignore, mut theme_name, mut log, mut compare_level, mut config_styles, mut bar_thickness,
         mut number_separator, mut decimal_separator, mut layout, mut sort_by, mut top_n) = (None,None,None,None,None,None,None,None,None,None,None,None,None,None,None,None,None,None,None,None,None);
    let mut issues = ConfigFileIssues::default();
    let mut buf = String::with_capacity(150);

    loop {
        let size = reader.read_line(&mut buf);
        if size == 0 {break};
        if buf.trim().starts_with("===>") {
            let id = buf.trim().trim_start_matches("===>").split_whitespace().next().unwrap_or("");

            if id == config_manager::DIRS {
                // The line is what ends a target here, and a space never does, so a path with one in
                // it needs no quoting and a configuration written by any earlier version still
                // reads. A target that does not parse is a target that would silently not be
                // counted, so it stops the run rather than warning.
                let declared = read_lines_from_file_to_vec(&mut reader, &mut buf, |line| vec![line.trim().to_owned()]);
                match super::args::parse_targets_in_block(&declared.join("\n")) {
                    Ok(targets) if !targets.is_empty() => dirs = Some(targets.into_iter()
                            .map(|(module, path)| Target { module, path }).collect()),
                    Ok(_) => {},
                    Err(_) => issues.invalid_fields.push(config_manager::DIRS)
                }
            } else if id == config_manager::EXCLUDE {
                let paths = read_lines_from_file_to_vec(&mut reader, &mut buf, super::args::parse_paths_to_vec);
                if mezura::engine::targets::validate_exclude_patterns(&paths).is_err() {
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
            } else if id == config_manager::FORCE_LANG {
                // Read as a block and not as a single line, like the other lists: a value written
                // across two lines was otherwise cut down to its first line in silence, since the
                // remainder does not begin with '===>' and the outer loop simply skips it.
                let declared = read_lines_from_file_to_vec(&mut reader, &mut buf, |line| vec![line.trim().to_owned()]).join(",");
                // An empty value is the command left in the file without being used, which is not a
                // mistake. Anything else that does not parse is one.
                if declared.split(',').any(|pair| !pair.trim().is_empty()) {
                    match super::args::parse_forced_languages(&declared) {
                        Some(x) => forced_languages = Some(x),
                        None => issues.invalid_fields.push(config_manager::FORCE_LANG)
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
            }else if id == config_manager::BRACES_AS_CODE {
                match read_bool_value_from_file(&mut reader, &mut buf) {
                    Ok(x) => braces_as_code = x,
                    Err(()) => issues.invalid_fields.push(config_manager::BRACES_AS_CODE)
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
            } else if id == config_manager::BAR_THICKNESS {
                buf.clear();
                let _ = reader.read_line(&mut buf);
                match config_manager::BarThickness::parse(&buf) {
                    Some(x) => bar_thickness = Some(x),
                    None => issues.invalid_fields.push(config_manager::BAR_THICKNESS)
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
                issues.warnings.extend(errors.iter().map(|x| (mezura::warnings::CONFIG_STYLE_INVALID, x.formatted())));
                if !declared.is_empty() {
                    config_styles = Some(declared);
                }
            } else if id == config_manager::LOG {
                buf.clear();
                let _ = reader.read_line(&mut buf);
                let name = &buf.trim().to_lowercase();
                if name == "yes" || name == "true" {
                    log = Some(LogOption::new(None));
                } else if name != "no" && name != "false"{
                    log = Some(LogOption::new(Some(name.to_owned())));
                }
            } else if id == config_manager::COMPRARE_LEVEL {
                buf.clear();
                let _ = reader.read_line(&mut buf);
                match super::args::parse_usize_value(&buf,MIN_COMPARE_LEVEL, MAX_COMPARE_LEVEL) {
                    Some(x) => compare_level = Some(x),
                    None => issues.invalid_fields.push(config_manager::COMPRARE_LEVEL)
                }
            } else {
                issues.warnings.push((mezura::warnings::CONFIG_SECTION_UNKNOWN,
                        format!("'{id}' is not a command, the section is ignored.")));
            }
        }
        buf.clear();
    }

    let builder = ConfigurationBuilder {
        dirs, exclude_dirs, languages_of_interest, excluded_languages, forced_languages, threads, braces_as_code, should_search_in_dotted,
        should_show_faulty_files, hidden, no_gitignore, theme_name, log, compare_level, config_styles, bar_thickness,
        number_separator, decimal_separator, layout, sort_by, top_n,
        ..Default::default()
    };

    // After the whole walk of the file, so that the number reported is the first failure and not
    // whichever one a nested block happened to meet
    if let Some((line, error)) = reader.failed_at {
        let cause = if error.kind() == std::io::ErrorKind::InvalidData {UnreadableCause::NotUtf8}
                else {UnreadableCause::Io(error.to_string())};
        return Err(ConfigFileParseError::UnreadableLine(file_name.to_owned(), line, cause));
    }

    Ok((builder, issues))
}

// Dirs must be specified (is checked before calling this function)
pub fn save_existing_commands_from_config_builder_to_file(config_path: Option<String>, config_name: &str, config_builder: &ConfigurationBuilder) 
-> std::io::Result<()> 
{
    let config_dir = if let Some(dir) = config_path {dir} else {PERSISTENT_APP_PATHS.config_dir.clone()};
    let file_name = config_dir + config_name + ".txt";

    let mut writer = BufWriter::new(std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(file_name)?);

    writer.write_all(b"Auto-generated config file.")?;

    // One target per line, which the block reader joins back with the whitespace that separates one
    // from the next. It is the readable form and the unambiguous one at the same time: a name only
    // ever reaches the paths written after it with a comma between them.
    writer.write_all(&[b"\n\n===> ",config_manager::DIRS.as_bytes(),b"\n"].concat())?;
    writer.write_all(config_builder.dirs.as_ref().unwrap().iter().map(config_manager::declared_form)
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
        writer.write_all(&[b"\n\n===> ",config_manager::FORCE_LANG.as_bytes(),b"\n"].concat())?;
        writer.write_all(super::args::forced_languages_to_string(forced_languages).as_bytes())?;
    }
    if let Some(threads) = &config_builder.threads {
        writer.write_all(&[b"\n\n===> ",config_manager::THREADS.as_bytes(),b"\n"].concat())?;
        writer.write_all((threads.producers().to_string() + " " + &threads.consumers().to_string()).as_bytes())?;
    }
    if let Some(braces_as_code) = &config_builder.braces_as_code {
        writer.write_all(&[b"\n\n===> ",config_manager::BRACES_AS_CODE.as_bytes(),b"\n"].concat())?;
        writer.write_all(if *braces_as_code {b"yes"} else {b"no"})?;
    }
    if let Some(should_search_in_dotted) = &config_builder.should_search_in_dotted {
        writer.write_all(&[b"\n\n===> ",config_manager::SEARCH_IN_DOTTED.as_bytes(),b"\n"].concat())?;
        writer.write_all(if *should_search_in_dotted {b"yes"} else {b"no"})?;
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

    if let Some(bar_thickness) = &config_builder.bar_thickness {
        writer.write_all(&[b"

===> ",config_manager::BAR_THICKNESS.as_bytes(),b"
"].concat())?;
        writer.write_all(bar_thickness.name().as_bytes())?;
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

    // The two style layers of a configuration collapse into its one block, in the order they were
    // applied, so that reloading the file reproduces what the run looked like. When --save-theme is
    // writing a theme in the same run, they are already inside it and would only be said twice.
    let styles = if config_builder.theme_name_to_save.is_some() {Vec::new()}
            else {config_builder.config_styles.iter().chain(config_builder.styles.iter()).flatten().collect::<Vec<_>>()};
    if !styles.is_empty() {
        // One pair per line, the same shape a theme file uses, so a long list stays readable
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
        writer.write_all(&[b"\n\n===> ",config_manager::COMPRARE_LEVEL.as_bytes(),b"\n"].concat())?;
        writer.write_all(compare_level.to_string().as_bytes())?;
    }

    writer.write_all(b"\n")?;
    writer.flush()?;

    Ok(())
}


// The names a directory offers, for the close-match suggestions of one that was not found
pub fn names_in_dir(dir: &str) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut names = entries.flatten().filter(|x| x.path().is_file())
            .filter_map(|x| x.path().file_stem().and_then(|x| x.to_str()).map(str::to_owned)).collect::<Vec<_>>();
    names.sort_by_key(|x| x.to_lowercase());
    names
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

//Keep parsing new lines as relevant, until an empty one appears.
// The reader 'parse_config_file' hands around: it counts lines, and one it cannot deliver is
// remembered instead of vanishing. From then on it reads as an ended file, which every caller
// already treats as the end of its block, so nothing after the bad line is applied and the file is
// refused as a whole by the single check at the end. The old shape ended whichever loop met the
// error and carried on, so a config saved in the wrong encoding kept its first blocks and silently
// lost the rest.
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

impl Formatted for ConfigFileParseError {
    fn formatted(&self) -> ColoredString {
        match self {
            Self::FileNotFound(x) => format!("'{x}' config file not found, defaults will be used.").yellow(),
            Self::UnreadableLine(file, line, UnreadableCause::NotUtf8) => format!("Configuration '{file}' stops being readable at line {line}, so none of it was used: the file is not saved as UTF-8.").red(),
            Self::UnreadableLine(file, line, UnreadableCause::Io(error)) => format!("Configuration '{file}' could not be read past line {line}, so none of it was used: {error}").red(),
        }
    }
}


#[cfg(test)]
mod tests {
    use super::super::config_manager::Configuration;
    use super::*;
    use mezura::Target;
    use crate::paths::test_paths::CONFIG_DIR;
    use super::super::config_manager::ConfigurationBuilder;
    // A line the reader could not deliver used to end the loop as if the file ended there, and the
    // half that had been read was applied without a word: a config saved in the wrong encoding kept
    // its first blocks and silently lost the rest. The realistic cause is not a failing disk, it is
    // an editor writing a path with non-ASCII characters as something other than UTF-8. A mistake
    // that decides what gets counted stops the run, so this is an error naming the file and the
    // line, not a warning.
    #[test]
    fn a_config_that_stops_being_readable_mid_file_is_an_error_not_a_half_applied_config() {
        let dir = std::env::temp_dir().join("mezura_unreadable_config_test");
        std::fs::create_dir_all(&dir).unwrap();
        let dir_str = dir.to_str().unwrap().to_owned() + "/";

        let mut contents = b"===> threads\n2 8\n\n===> dirs\n".to_vec();
        contents.extend([0xCF, 0xE1, 0xE8, 0xFF, b'\n']);
        contents.extend(b"\n===> braces-as-code\nyes\n");
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
    fn test_save_config_file_and_then_parse_it() -> std::io::Result<()> {
        let command = "./ --exclude a,b,c.txt,d.txt, --braces-as-code --threads 1 1 --hide keywords,timing \
                --force-lang m=matlab,.pl=Perl \
                --style code-number=green,comments-label=magenta bold,arrow=default dim".to_string();
        let config_builder = config_manager::create_config_builder_from_args(&command).unwrap();

        let test_config_dir = Some(CONFIG_DIR.to_owned());
        super::super::config_files::save_existing_commands_from_config_builder_to_file(test_config_dir, "auto-generated", &config_builder)?;

        let (options, issues) = super::super::config_files::parse_config_file(Some("auto-generated"), Some(CONFIG_DIR.to_owned())).unwrap();
        assert!(issues.invalid_fields.is_empty() && issues.warnings.is_empty());
        assert_eq!(config_builder.dirs, options.dirs);
        assert_eq!(config_builder.exclude_dirs, options.exclude_dirs);
        assert_eq!(config_builder.threads, options.threads);
        assert_eq!(config_builder.braces_as_code, options.braces_as_code);
        assert_eq!(config_builder.should_show_faulty_files, options.should_show_faulty_files);
        assert_eq!(config_builder.should_search_in_dotted, options.should_search_in_dotted);
        assert_eq!(config_builder.hidden, options.hidden);
        // A project that answers a contested extension its own way answers it once, in its config
        assert_eq!(config_builder.forced_languages, options.forced_languages);
        assert_eq!(Some(hashmap!("m".to_owned() => "matlab".to_owned(), "pl".to_owned() => "Perl".to_owned())),
                options.forced_languages);
        // Written one pair per line and read back as a group, so a saved look survives a reload
        assert_eq!(config_builder.styles, options.config_styles);
        assert_eq!(3, options.config_styles.as_ref().unwrap().len());

        Ok(())
    }

    #[test]
    fn a_force_lang_value_written_across_lines_is_read_whole() -> std::io::Result<()> {
        let dir = CONFIG_DIR.to_owned();
        std::fs::create_dir_all(&dir)?;
        let path = dir.clone() + "force-lang-block.txt";
        std::fs::write(&path, "===> dirs\n./\n\n===> force-lang\nm=matlab,\npl=perl\n")?;

        let (options, issues) = super::super::config_files::parse_config_file(Some("force-lang-block"), Some(dir)).unwrap();
        assert!(issues.invalid_fields.is_empty());
        assert_eq!(Some(hashmap!("m".to_owned() => "matlab".to_owned(), "pl".to_owned() => "perl".to_owned())),
                options.forced_languages);

        std::fs::remove_file(&path)
    }

    // The whole point of putting the name inside the target rather than in a field of its own: what
    // '--save' writes has to be what a load reads, or the definitions would drift away from the
    // paths they belong to on the first round trip.
    #[test]
    fn the_modules_of_a_saved_configuration_survive_being_read_back() -> std::io::Result<()> {
        let dir = CONFIG_DIR.to_owned();
        std::fs::create_dir_all(&dir)?;
        let path = dir.clone() + "modules-round-trip.txt";

        // The last two are the ones that break: an unnamed target after a named one, and a path
        // with a space in it now that whitespace is what separates one target from the next
        let declared = vec![Target::named("frontend", "D:/x/web".to_owned()),
                Target::named("frontend", "D:/x/ui".to_owned()),
                Target::named("backend", "D:/x/my api".to_owned()),
                Target::of("D:/x/loose".to_owned())];
        // The one line form that the log entry carries. Whitespace and not commas, or the unnamed
        // target at the end would be read back as one more directory of 'backend'.
        assert_eq!("frontend=D:/x/web frontend=D:/x/ui backend=\"D:/x/my api\" D:/x/loose",
                config_manager::targets_to_string(&declared));
        // and while nothing is named it is what it always was, so an entry logged by an older
        // version is not reported as having had its targets changed
        assert_eq!("D:/x/web,D:/x/api", config_manager::targets_to_string(
                &[Target::of("D:/x/web".to_owned()), Target::of("D:/x/api".to_owned())]));

        let builder = ConfigurationBuilder { dirs: Some(declared.clone()), ..Default::default() };
        save_existing_commands_from_config_builder_to_file(Some(dir.clone()), "modules-round-trip", &builder)?;

        let (options, issues) = parse_config_file(Some("modules-round-trip"), Some(dir)).unwrap();
        assert!(issues.invalid_fields.is_empty());
        assert_eq!(Some(declared), options.dirs);

        std::fs::remove_file(&path)
    }

    // A block is read as a whole, so a module written on a line of its own means what the same
    // module written next to the others means
    #[test]
    fn the_dirs_block_reads_a_module_across_lines_and_refuses_one_with_no_path() -> std::io::Result<()> {
        let dir = CONFIG_DIR.to_owned();
        std::fs::create_dir_all(&dir)?;
        let path = dir.clone() + "dirs-block.txt";
        std::fs::write(&path, "===> dirs\ntests=D:/x/api/tests\ntests=D:/x/web/tests\nbackend=D:/x/api\n")?;

        let (options, issues) = parse_config_file(Some("dirs-block"), Some(dir.clone())).unwrap();
        assert!(issues.invalid_fields.is_empty());
        assert_eq!(Some(vec![Target::named("tests", "D:/x/api/tests".to_owned()),
                Target::named("tests", "D:/x/web/tests".to_owned()),
                Target::named("backend", "D:/x/api".to_owned())]), options.dirs);

        // and a trailing comma still continues the list over the line break
        std::fs::write(&path, "===> dirs\ntests=D:/x/api/tests,\nD:/x/web/tests\n")?;
        let (options, _) = parse_config_file(Some("dirs-block"), Some(dir.clone())).unwrap();
        assert_eq!(Some(vec![Target::named("tests", "D:/x/api/tests".to_owned()),
                Target::named("tests", "D:/x/web/tests".to_owned())]), options.dirs);

        std::fs::write(&path, "===> dirs\nfrontend=\n")?;
        let (options, issues) = parse_config_file(Some("dirs-block"), Some(dir)).unwrap();
        assert_eq!(vec![config_manager::DIRS], issues.invalid_fields);
        assert_eq!(None, options.dirs);

        std::fs::remove_file(&path)
    }

    // It used to unwrap its way through the file, which was safe while the only files it ever read
    // were ours. The migration now asks it whether the user's copy still means what our copy means,
    // so a file edited into something unrecognisable has to come back as None and not take the run
    // with it. Every one of these was a panic before.
    #[test]
    fn test_read_config_file() -> std::io::Result<()> {
        let mut config = Configuration::new(vec!["C:/Some/Path/a".to_owned(),"C:/Some/Path/b".to_owned(),"C:/Some/Path/c".to_owned(),"C:/Some/Path/d".to_owned()]);
        config.engine
            .set_exclude_dirs(vec!["a".to_owned(), "b".to_owned(), "c.txt".to_owned(), "d.txt".to_owned()])
            .set_threads(1,1)
            .set_braces_as_code(true);
        config
            .set_hidden(config_manager::Hidden {bar: true, timing: true, ..Default::default()});


        let (options, issues) = super::super::config_files::parse_config_file(Some("test"), Some(CONFIG_DIR.to_owned())).unwrap();
        assert!(issues.invalid_fields.is_empty() && issues.warnings.is_empty());
        assert_eq!(config.engine.dirs, options.dirs.unwrap());
        assert_eq!(config.engine.exclude_dirs, options.exclude_dirs.unwrap());
        assert_eq!(config.engine.threads, options.threads.unwrap());
        assert_eq!(config.engine.braces_as_code, options.braces_as_code.unwrap());
        assert_eq!(config.view.should_show_faulty_files, options.should_show_faulty_files.unwrap());
        assert_eq!(config.engine.should_search_in_dotted, options.should_search_in_dotted.unwrap());
        assert_eq!(config.view.hidden, options.hidden.unwrap());

        Ok(())
    }
    #[test]
    fn test_default_config_file_is_found_and_parsed() {
        let dir = std::env::temp_dir().join("mezura_default_config_test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("default.txt"), "===> exclude-languages\nSQL\n").unwrap();

        let (options, issues) = super::super::config_files::parse_config_file(None, Some(dir.to_str().unwrap().to_owned() + "/")).unwrap();
        assert!(issues.invalid_fields.is_empty() && issues.warnings.is_empty());
        assert_eq!(Some(vec!["sql".to_owned()]), options.excluded_languages);

        std::fs::remove_dir_all(&dir).unwrap();
    }
    #[test]
    fn test_parse_config_file_reports_invalid_values() {
        let dir = std::env::temp_dir().join("mezura_invalid_config_test");
        std::fs::create_dir_all(&dir).unwrap();
        let dir_str = dir.to_str().unwrap().to_owned() + "/";

        std::fs::write(dir.join("badcfg.txt"),
                "===> threads\n3343 45534\n\n===> braces-as-code\nmitsos\n\n===> compare\n99\n\n===> hide\nkeywords\n\n===> sort\nnope\n").unwrap();

        let (options, issues) = super::super::config_files::parse_config_file(Some("badcfg"), Some(dir_str)).unwrap();
        assert_eq!(issues.invalid_fields, vec![config_manager::THREADS, config_manager::BRACES_AS_CODE,
                config_manager::COMPRARE_LEVEL, config_manager::SORT]);
        assert!(issues.warnings.is_empty());
        assert_eq!(options.threads, None);
        assert_eq!(options.braces_as_code, None);
        assert_eq!(options.compare_level, None);
        assert_eq!(options.sort_by, None);
        assert_eq!(options.hidden, Some(config_manager::Hidden {keywords: true, ..Default::default()}));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // A section nobody knows and a style line that does not parse are both about how the result
    // looks, so they are said out loud and the rest of the file still applies
    #[test]
    fn test_parse_config_file_warns_instead_of_failing_for_unknown_sections_and_styles() {
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

