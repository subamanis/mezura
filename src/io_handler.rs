use std::{collections::HashMap, fs::{self, DirEntry, File}, io::{self, BufRead, BufReader, BufWriter, Write}, path::Path};

use chrono::{DateTime, Local};
use colored::*;

use crate::{Configuration, DEFAULT_CONFIG_NAME, Formatted, PERSISTENT_APP_PATHS, config_manager::{self, ConfigurationBuilder, LogOption,
     MAX_COMPARE_LEVEL, MAX_CONSUMERS_VALUE, MAX_PRODUCERS_VALUE, MIN_COMPARE_LEVEL, MIN_CONSUMERS_VALUE, MIN_PRODUCERS_VALUE, Target, Threads}, domain::*, split_line_on_whitespace, theme, utils};


const LANGUAGE                 : &str = "Language";     
const EXTENSIONS               : &str = "Extensions";     
const STRING_SYMBOLS           : &str = "String symbols";     
const COMMENT_SYMBOLS          : &str = "Comment symbols";     
const MULTILINE_COMMENT_START  : &str = "Multi line comment start";     
const MULTILINE_COMMENT_END    : &str = "Multi line comment end";     
const KEYWORD                  : &str = "Keyword";
const KEYWORD_NAME             : &str = "NAME";
const KEYWORD_ALIASES          : &str = "ALIASES";
const CONTESTED_EXTENSIONS     : &str = "contested-extensions";


#[derive(Debug)]
pub enum LanguageDirParseError {
    NoFilesFound,
    NoFilesFormattedProperly,
    PathMissing(String)
}

#[derive(Debug)]
pub enum ConfigFileParseError {
    FileNotFound(String),
    IOError
}


// --------------------- Languages handling -------------------------

pub fn parse_supported_languages_to_map(target_path: &str) -> Result<(HashMap<String, Language>, Vec<String>), LanguageDirParseError> {
    fn add_file_name_to_faulty_files(entry: &DirEntry, faulty_files: &mut Vec<String>) {
        let file_name = entry.file_name().to_str().map_or(String::new(), |x| x.to_owned());
        if !file_name.is_empty() {faulty_files.push(file_name.to_lowercase())}
    }

    let mut language_map = HashMap::with_capacity(30);
    let mut faulty_files : Vec<String> = Vec::new();
    let mut buffer = String::with_capacity(200);
    
    let entries = fs::read_dir(target_path);
    if entries.is_err() {
        return Err(LanguageDirParseError::PathMissing(target_path.to_owned()));
    }
    for entry in entries.unwrap() {
        let Ok(entry) = entry else { continue };

        let path = entry.path();
        if !Path::new(&path).is_file() {continue;}

        let Ok(reader) = my_reader::BufReader::open(path) else {
            add_file_name_to_faulty_files(&entry, &mut faulty_files);
            continue;
        };

        let Ok(language) = parse_file_to_language(reader, &mut buffer) else {
            add_file_name_to_faulty_files(&entry, &mut faulty_files);
            continue;
        };

        language_map.insert(language.name.to_owned(), language);
    }

    if language_map.is_empty() && faulty_files.is_empty() {
        Err(LanguageDirParseError::NoFilesFound)
    } else if language_map.is_empty() {
        Err(LanguageDirParseError::NoFilesFormattedProperly)
    } else {
        Ok((language_map, faulty_files))
    }
}

fn parse_file_to_language(mut reader :my_reader::BufReader, buffer :&mut String) -> Result<Language,()> {
    if !reader.read_line_and_compare(buffer, LANGUAGE) {return Err(());}
    if !reader.read_line_exists(buffer) {return Err(());}
    let lang_name = buffer.trim_end().to_owned();
    if !reader.read_line_exists(buffer) {return Err(());}

    if !reader.read_line_and_compare(buffer, EXTENSIONS) {return Err(());}
    let Ok(identifiers) = reader.get_line_sliced(buffer) else { return Err(()) };
    if !reader.read_line_exists(buffer) {return Err(());}

    if !reader.read_line_and_compare(buffer, STRING_SYMBOLS) {return Err(());}
    let Ok(string_symbols) = reader.get_line_sliced(buffer) else { return Err(()) };
    if string_symbols.is_empty() {return Err(());}

    if !reader.read_line_exists(buffer) {return Err(());}
    if !reader.read_line_and_compare(buffer, COMMENT_SYMBOLS) {return Err(());}
    let Ok(comment_symbols) = reader.get_line_sliced(buffer) else { return Err(()) };
    
    let mut multi_start :Option<String> = None;
    let mut multi_end :Option<String> = None;
    if reader.read_line_and_compare(buffer, MULTILINE_COMMENT_START) {
        if !reader.read_line_exists(buffer) {return Err(());}
        let symbol = buffer.trim_end().to_owned();
        if symbol.is_empty() {return Err(());}
        multi_start = Some(symbol);
        if !reader.read_line_and_compare(buffer, MULTILINE_COMMENT_END) {return Err(());}
        if !reader.read_line_exists(buffer) {return Err(());}
        let symbol = buffer.trim_end().to_owned();
        if symbol.is_empty() {return Err(());}
        multi_end = Some(symbol);
        // The blank line that separates the multiline symbols from the keyword blocks. A language
        // that declares no keywords ends here instead, and that is not a formatting mistake: CSS,
        // HTML and SCSS were rejected outright for it, on a clean installation as much as an old one.
        reader.read_line_exists(buffer);
    }
    
    let mut keywords = Vec::new();
    while reader.read_line_exists(buffer) {
        if !reader.read_lines_exist(2, buffer) {return Err(());}
        let name = buffer.trim().to_string().clone();
        if name.is_empty() {return Err(());}
        if !reader.read_line_exists(buffer) {return Err(());}
        let Ok(aliases) = reader.get_line_sliced(buffer) else { return Err(()) };
        if aliases.is_empty() {return Err(());}
        
        let keyword = Keyword {
            descriptive_name : name,
            aliases
        };
        keywords.push(keyword);
    }
    
    Ok(Language {
        name: lang_name,
        extensions: identifiers,
        string_symbols,
        comment_symbols,
        multiline_comment_start_symbol : multi_start,
        multiline_comment_end_symbol : multi_end,
        keywords,
        scan_plan : std::sync::OnceLock::new()
    })
}

// Returns None instead of panicking on anything it does not recognise, because it is no longer only
// read over the baked-in files, which are ours and are correct by construction. The migration reads
// what is on the user's disk through it, to ask whether their copy of a file still means what our
// copy means, and a file that somebody edited into something unparseable must come back as "not the
// same" rather than take the run down with it.
pub fn parse_string_to_language(contents: &str) -> Option<Language> {
    let mut lines = contents.lines().map(str::trim_end);

    if lines.next()? != LANGUAGE {return None;}
    let lang_name = lines.next()?.trim().to_owned();
    if lang_name.is_empty() {return None;}
    lines.next()?;

    if lines.next()? != EXTENSIONS {return None;}
    let extensions = split_line_on_whitespace(lines.next()?);
    if extensions.is_empty() {return None;}
    lines.next()?;

    if lines.next()? != STRING_SYMBOLS {return None;}
    let string_symbols = split_line_on_whitespace(lines.next()?);
    if string_symbols.is_empty() {return None;}
    lines.next()?;

    if lines.next()? != COMMENT_SYMBOLS {return None;}
    let comment_symbols = split_line_on_whitespace(lines.next()?);

    let (mut mult_start, mut mult_end) = (None, None);
    let mut next_line = lines.next();
    if next_line == Some(MULTILINE_COMMENT_START) {
        let start = lines.next()?.trim().to_owned();
        if start.is_empty() || lines.next()? != MULTILINE_COMMENT_END {return None;}
        let end = lines.next()?.trim().to_owned();
        if end.is_empty() {return None;}
        (mult_start, mult_end) = (Some(start), Some(end));
        // The blank line that separates the multiline symbols from the keyword blocks, and which a
        // language declaring no keywords does not have at all
        next_line = lines.next();
    }

    let mut keywords = Vec::new();
    while let Some(line) = next_line {
        if line != KEYWORD {break;}
        if lines.next()? != KEYWORD_NAME {return None;}
        let name = lines.next()?.trim().to_owned();
        if lines.next()? != KEYWORD_ALIASES {return None;}
        let aliases = split_line_on_whitespace(lines.next()?);
        if name.is_empty() || aliases.is_empty() {return None;}

        keywords.push(Keyword{descriptive_name: name, aliases});
        next_line = lines.next();
    }

    Some(Language::new(lang_name, extensions, string_symbols, comment_symbols, mult_start, mult_end, keywords))
}

pub fn serialize_language(lang: &Language, path: &str) -> Result<(), io::Error> {
    let file_path = path.to_string() + "/" + &lang.name + ".txt";
    let mut writer = BufWriter::new(std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(file_path)?);

    writer.write_all(format!("{LANGUAGE}\n").as_bytes())?;
    writer.write_all(lang.name.as_bytes())?;
    writer.write_all(b"\n\n")?;

    writer.write_all(format!("{EXTENSIONS}\n").as_bytes())?;
    writer.write_all(lang.extensions.join(" ").as_bytes())?;
    writer.write_all(b"\n\n")?;

    writer.write_all(format!("{STRING_SYMBOLS}\n").as_bytes())?;
    writer.write_all(lang.string_symbols.join(" ").as_bytes())?;
    writer.write_all(b"\n\n")?;

    writer.write_all(format!("{COMMENT_SYMBOLS}\n").as_bytes())?;
    writer.write_all(lang.comment_symbols.join(" ").as_bytes())?;
    writer.write_all(b"\n")?;

    if let Some(symbol) = &lang.multiline_comment_start_symbol {
        writer.write_all(format!("{MULTILINE_COMMENT_START}\n").as_bytes())?;
        writer.write_all(symbol.as_bytes())?;
        writer.write_all(b"\n")?;
        writer.write_all(format!("{MULTILINE_COMMENT_END}\n").as_bytes())?;
        writer.write_all(lang.multiline_comment_end_symbol.as_ref().unwrap().as_bytes())?;
        writer.write_all(b"\n")?;
    }
    writer.write_all(b"\n")?;

    for keyword in lang.keywords.iter() {
        writer.write_all(format!("{KEYWORD}\n").as_bytes())?;
        writer.write_all(format!("{KEYWORD_NAME}\n").as_bytes())?;
        writer.write_all(keyword.descriptive_name.as_bytes())?;
        writer.write_all(b"\n")?;
        writer.write_all(format!("{KEYWORD_ALIASES}\n").as_bytes())?;
        writer.write_all(keyword.aliases.join(" ").as_bytes())?;
        writer.write_all(b"\n")?;
    }

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


// A missing file is not a mistake: an installation made by an earlier version has none, and the
// only consequence is that contested extensions fall back to the alphabetical tiebreak, which
// announces itself anyway.
pub fn parse_extension_priority_file(path: &str) -> (HashMap<String,Vec<String>>, Vec<String>) {
    match fs::read_to_string(path) {
        Ok(contents) => parse_extension_priority(&contents),
        Err(_) => (HashMap::new(), Vec::new())
    }
}

// A line that does not parse is reported and skipped while the rest of the file applies, because a
// mistake here cannot produce a wrong number in silence: the extension it failed to settle falls
// through to the tiebreak, which says so by name.
pub fn parse_extension_priority(contents: &str) -> (HashMap<String,Vec<String>>, Vec<String>) {
    let mut rules : HashMap<String,Vec<String>> = HashMap::new();
    let mut faulty_lines = Vec::new();
    let mut inside_the_rules = false;

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() {continue;}
        // The '===>' of the configuration files and not the bare headers of the language files: a
        // language file has no prose in it and needs nothing to separate the two, while this one
        // explains itself above its rules exactly as a configuration does. A marker also ends the
        // block, so that a section added later is skipped rather than read as a rule for an
        // extension named '===>', which is neither applied nor reported.
        if line.starts_with("===>") {
            inside_the_rules = line.trim_start_matches("===>").split_whitespace().next()
                    .is_some_and(|id| id.eq_ignore_ascii_case(CONTESTED_EXTENSIONS));
            continue;
        }
        if !inside_the_rules {continue;}

        let Some((extension, claimants)) = line.split_once(char::is_whitespace) else {
            faulty_lines.push(line.to_owned());
            continue;
        };
        let names = claimants.split(',').filter_map(utils::get_trimmed_if_not_empty).collect::<Vec<_>>();
        if names.is_empty() {
            faulty_lines.push(line.to_owned());
            continue;
        }

        // The first declaration of an extension is the one that counts, so that a second one cannot
        // silently undo a decision that is sitting a few lines above it in the same file
        match rules.entry(extension.to_ascii_lowercase()) {
            std::collections::hash_map::Entry::Occupied(_) => faulty_lines.push(line.to_owned()),
            std::collections::hash_map::Entry::Vacant(slot) => { slot.insert(names); }
        }
    }

    (rules, faulty_lines)
}


// ------------------------------ Theme handling ------------------------------

// None means the theme is not there at all, which is a mistake in the name and not in the file.
// A theme that exists always loads, carrying whatever its parser could not read.
pub fn load_theme(name: &str, themes_dir: &str) -> Option<theme::ThemeFile> {
    let entries = fs::read_dir(themes_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|x| x.to_str()) else { continue };
        if !stem.eq_ignore_ascii_case(name.trim()) {
            continue;
        }

        let contents = fs::read_to_string(&path).ok()?;
        return Some(theme::parse_theme_file(&contents));
    }

    None
}

// Flattened on purpose: the reason a theme file exists is that it can be handed to someone else, so
// it carries values and not a reference to whatever it was built on top of.
pub fn save_theme_to_file(themes_dir: &str, name: &str, theme: &theme::Theme) -> io::Result<()> {
    let styles = theme.non_default_tokens().into_iter().map(|(token, value)| (token.to_owned(), value)).collect::<Vec<_>>();
    fs::create_dir_all(themes_dir)?;
    fs::write(themes_dir.to_owned() + name + ".txt", theme::theme_file_contents(&styles))
}

pub fn generate_theme_editor_page() -> io::Result<String> {
    fn js_escape(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"").replace('<', "\\u003c")
    }

    let template = include_str!("../docs/theme-editor/index.html");

    let mut entries: Vec<(String, Vec<String>)> = Vec::new();
    for entry in fs::read_dir(&PERSISTENT_APP_PATHS.themes_dir)?.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|x| x.to_str()) else { continue };
        let Ok(contents) = fs::read_to_string(&path) else { continue };
        // The page edits the language slots only, so the rest of the theme is resolved and dropped
        let resolved = theme::resolve(&theme::parse_theme_file(&contents).0, &[], &[]);
        entries.push((stem.to_owned(), resolved.language_colors().iter().map(utils::color_to_config_string).collect()));
    }
    entries.sort_by_key(|x| x.0.to_lowercase());

    let themes_js = entries.iter().map(|(name, tokens)| {
        format!("{{name:\"{}\",tokens:[{}]}}", js_escape(name),
            tokens.iter().map(|t| format!("\"{}\"", js_escape(t))).collect::<Vec<_>>().join(","))
    }).collect::<Vec<_>>().join(",");

    let page = template.replace("/*MEZURA_SYSTEM_THEMES*/", &format!("SYSTEM_THEMES = [{themes_js}];"));

    let out_path = PERSISTENT_APP_PATHS.data_dir.clone() + "theme-editor.html";
    fs::write(&out_path, page)?;

    Ok(out_path)
}


// ------------------------------ Config handling ------------------------------

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
    let mut reader = BufReader::new(match fs::File::open(file_path){
        Ok(f) => f,
        Err(_) => return Err(ConfigFileParseError::FileNotFound(file_name.to_owned()))
    });

    let (mut dirs, mut braces_as_code, mut should_search_in_dotted, mut threads, mut exclude_dirs,
         mut languages_of_interest, mut excluded_languages, mut forced_languages, mut should_show_faulty_files, mut hidden,
         mut no_gitignore, mut theme_name, mut log, mut compare_level, mut config_styles, mut bar_thickness,
         mut number_separator, mut decimal_separator, mut layout, mut sort_by, mut top_n) = (None,None,None,None,None,None,None,None,None,None,None,None,None,None,None,None,None,None,None,None,None);
    let mut issues = ConfigFileIssues::default();
    let mut buf = String::with_capacity(150);

    while let Ok(size) = reader.read_line(&mut buf) {
        if size == 0 {break};
        if buf.trim().starts_with("===>") {
            let id = buf.trim().trim_start_matches("===>").split_whitespace().next().unwrap_or("");

            if id == config_manager::DIRS {
                // The line is what ends a target here, and a space never does, so a path with one in
                // it needs no quoting and a configuration written by any earlier version still
                // reads. A target that does not parse is a target that would silently not be
                // counted, so it stops the run rather than warning.
                let declared = read_lines_from_file_to_vec(&mut reader, &mut buf, |line| vec![line.trim().to_owned()]);
                match utils::parse_targets_in_block(&declared.join("\n")) {
                    Ok(targets) if !targets.is_empty() => dirs = Some(targets.into_iter()
                            .map(|(module, path)| Target { module, path }).collect()),
                    Ok(_) => {},
                    Err(_) => issues.invalid_fields.push(config_manager::DIRS)
                }
            } else if id == config_manager::EXCLUDE {
                let paths = read_lines_from_file_to_vec(&mut reader, &mut buf, utils::parse_paths_to_vec);
                if utils::build_exclude_matcher(&paths).is_err() {
                    issues.invalid_fields.push(config_manager::EXCLUDE);
                } else if !paths.is_empty() {
                    exclude_dirs = Some(paths);
                }
            } else if id == config_manager::LANGUAGES {
                let langs = read_lines_from_file_to_vec(&mut reader, &mut buf, utils::parse_languages_to_vec);
                if !langs.is_empty() {
                    languages_of_interest = Some(langs);
                }
            } else if id == config_manager::EXCLUDE_LANGUAGES {
                let langs = read_lines_from_file_to_vec(&mut reader, &mut buf, utils::parse_languages_to_vec);
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
                    match utils::parse_forced_languages(&declared) {
                        Some(x) => forced_languages = Some(x),
                        None => issues.invalid_fields.push(config_manager::FORCE_LANG)
                    }
                }
            } else if id == config_manager::THREADS {
                buf.clear();
                let _ = reader.read_line(&mut buf);
                match utils::parse_two_usize_values(&buf,MIN_PRODUCERS_VALUE, MAX_PRODUCERS_VALUE,
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
                if name.is_empty() || load_theme(name, &PERSISTENT_APP_PATHS.themes_dir).is_none() {
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
                match utils::parse_usize_value(&buf, 1, usize::MAX) {
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
                let (declared, errors) = theme::parse_overrides_leniently(&declared.join("\n"));
                issues.warnings.extend(errors.iter().map(|x| (crate::warnings::CONFIG_STYLE_INVALID, x.formatted())));
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
                match utils::parse_usize_value(&buf,MIN_COMPARE_LEVEL, MAX_COMPARE_LEVEL) {
                    Some(x) => compare_level = Some(x),
                    None => issues.invalid_fields.push(config_manager::COMPRARE_LEVEL)
                }
            } else {
                issues.warnings.push((crate::warnings::CONFIG_SECTION_UNKNOWN,
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
    writer.write_all(config_builder.dirs.as_ref().unwrap().iter().map(config_manager::Target::declared_form)
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
        writer.write_all(utils::forced_languages_to_string(forced_languages).as_bytes())?;
    }
    if let Some(threads) = &config_builder.threads {
        writer.write_all(&[b"\n\n===> ",config_manager::THREADS.as_bytes(),b"\n"].concat())?;
        writer.write_all((threads.producers.to_string() + " " + &threads.consumers.to_string()).as_bytes())?;
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

pub fn write_default_config(contents: String) -> Result<(), io::Error> {
    let file_path = PERSISTENT_APP_PATHS.config_dir.clone() + DEFAULT_CONFIG_NAME;
    let mut writer = BufWriter::new(std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(file_path)?);
    writer.write_all(contents.as_bytes())?;
    writer.flush()?;

    Ok(())
}


// ----------------------------------- Log handling ------------------------------------------

// Everything that can change a number, and nothing that only changes how it looks, written into
// every log entry so that a later run can say whether the two are comparable at all. The same list
// is what the progress section reads back, so the writing and the comparison cannot drift into
// formatting the same setting two different ways.
pub fn counting_settings(config: &Configuration) -> [(&'static str, String); 8] {
    let yes_no = |value: bool| if value {"yes"} else {"no"}.to_owned();

    // Every key is the name of the command that sets it, so that the 'modified:' tag of the progress
    // section names something the reader can look up with '--help'. That is why this one is the
    // double negative 'no-gitignore' and not the 'gitignore' that would have read better.
    // Sorted here and nowhere else. The report shows the targets in the order they were declared,
    // because that order is the user's own arrangement of the columns, but reordering them changes
    // no number, and this list exists to say whether two runs counted the same thing.
    let mut targets = config.dirs.clone();
    targets.sort_by_key(|x| utils::path_comparison_key(&x.to_string()));

    [(config_manager::DIRS, config_manager::targets_to_string(&targets)),
     (config_manager::EXCLUDE, config.exclude_dirs.join(",")),
     (config_manager::LANGUAGES, config.languages_of_interest.join(",")),
     (config_manager::EXCLUDE_LANGUAGES, config.excluded_languages.join(",")),
     (config_manager::FORCE_LANG, utils::forced_languages_to_string(&config.forced_languages)),
     (config_manager::BRACES_AS_CODE, yes_no(config.braces_as_code)),
     (config_manager::SEARCH_IN_DOTTED, yes_no(config.should_search_in_dotted)),
     (config_manager::NO_GITIGNORE, yes_no(config.no_gitignore))]
}

pub fn log_stats(path: &str, contents: &Option<String>, result: &crate::RunResult, datetime_now: &DateTime<Local>, config: &Configuration) -> io::Result<()> {
    let mut writer = std::io::BufWriter::new(std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(path)?);

    write_current_log(&mut writer, config, datetime_now, result)?;

    if let Some(contents) = contents {
        writer.write_all(contents.as_bytes())?;
    }
    writer.flush()?;

    Ok(())
}

// The totals stay where they were and the modules are a block under them, so an entry written before
// any of this existed reads exactly as it always did and a run that named none writes no block at
// all. Nothing on disk needs converting, which is the same reason an entry from v2 with no
// 'Comments' line is still read without complaint.
fn write_current_log(writer: &mut BufWriter<File>, config: &Configuration, datetime_now: &DateTime<Local>, result: &crate::RunResult) -> io::Result<()> {
    let final_stats = &result.final_stats;
    writer.write_all(format!("===>{}\n",config.log.name.clone().unwrap_or_default()).as_bytes())?;
    writer.write_all(datetime_now.format("%Y-%m-%d %H:%M:%S %z").to_string().as_bytes())?;
    writer.write_all(b"\n")?;
    writer.write_all(b"Configuration:\n")?;
    for (key, value) in counting_settings(config) {
        writer.write_all(format!("    {key}: {value}\n").as_bytes())?;
    }
    writer.write_all(b"Stats:\n")?;
    writer.write_all(format!("    Files: {}\n",final_stats.files).as_bytes())?;
    writer.write_all(format!("    Lines: {}\n",final_stats.lines).as_bytes())?;
    writer.write_all(format!("        Code: {}\n",final_stats.code_lines).as_bytes())?;
    writer.write_all(format!("        Comments: {}\n",final_stats.comment_lines).as_bytes())?;
    writer.write_all(format!("        Extra: {}\n",final_stats.extra_lines).as_bytes())?;
    writer.write_all(format!("    Total Size: {}\n",final_stats.bytes_size).as_bytes())?;
    writer.write_all(format!("        Average Size: {}\n",final_stats.bytes_average_size).as_bytes())?;
    if result.has_modules() {
        writer.write_all(b"    Modules:\n")?;
        for module in &result.modules {
            let stats = &module.final_stats;
            writer.write_all(format!("        {}:\n", module.name.as_deref().unwrap_or(crate::UNNAMED_MODULE_NAME)).as_bytes())?;
            writer.write_all(format!("            Files: {}\n", stats.files).as_bytes())?;
            writer.write_all(format!("            Lines: {}\n", stats.lines).as_bytes())?;
            writer.write_all(format!("                Code: {}\n", stats.code_lines).as_bytes())?;
            writer.write_all(format!("                Comments: {}\n", stats.comment_lines).as_bytes())?;
            writer.write_all(format!("                Extra: {}\n", stats.extra_lines).as_bytes())?;
        }
    }
    writer.write_all(b"\n\n")?;
    writer.write_all(b"--------------------------------------------------------------------------------------------\n\n\n")?;

    Ok(())
}


fn read_bool_value_from_file(reader: &mut BufReader<File>, buf: &mut String) -> Result<Option<bool>, ()> {
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
fn read_lines_from_file_to_vec<T>(reader: &mut BufReader<File>, buf: &mut String, parser_func: fn(&str) -> Vec<T>) -> Vec<T> {
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


const REGENERATE_LANGUAGES_HINT : &str =
        "Delete the \"languages\" folder and it will be generated again on the next execution.\nThe \"config\" and \"logs\" folders will not be affected.";

impl Formatted for LanguageDirParseError {
    fn formatted(&self) -> ColoredString {
        match self {
            Self::NoFilesFound => format!("Error: No language files found in directory.\n{REGENERATE_LANGUAGES_HINT}").red(),
            Self::NoFilesFormattedProperly => format!("Error: No language file is formatted properly, so none could be parsed.\n{REGENERATE_LANGUAGES_HINT}").red(),
            Self::PathMissing(path) => format!("Error: It seems that the language dir ({path}) has been deleted.\n{REGENERATE_LANGUAGES_HINT}").red(),
        }
    }
}

impl Formatted for ConfigFileParseError {
    fn formatted(&self) -> ColoredString {
        match self {
            Self::FileNotFound(x) => format!("'{x}' config file not found, defaults will be used.").yellow(),
            Self::IOError => "Unexpected IO error while reading, defaults will be used".yellow()
        }
    }
}


mod my_reader {
    use std::{fs::File, io::{self, prelude::*}};

    pub struct BufReader {
        reader: io::BufReader<File>,
    }

    impl BufReader {
        pub fn open(path: impl AsRef<std::path::Path>) -> io::Result<Self> {
            let file = File::open(path)?;
            let reader = io::BufReader::new(file);

            Ok(Self { reader })
        }

        pub fn read_line_exists(&mut self, buffer: &mut String) -> bool {
            match self.read_line(buffer) {
                Err(_) => false,
                Ok(x) => {
                    x != 0 
                }
            }
        }

        pub fn read_line_and_compare(&mut self, buffer: &mut String, other : &str) -> bool {
            match self.read_line(buffer) {
                Ok(_) => {
                    buffer.trim_end() == other
                },
                Err(_) => false
            }
        }

        pub fn read_line(&mut self, buffer: &mut String) -> Result<usize, io::Error> {
            buffer.clear();
            self.reader.read_line(buffer)
        }

        pub fn read_lines_exist(&mut self, num :usize, buffer: &mut String) -> bool {
            for _ in 0..num {
                if !self.read_line_exists(buffer) {return false;}
            }
            
            true
        }

        pub fn get_line_sliced(&mut self, buffer: &mut String) -> Result<Vec<String>, ()> {
            if self.read_line_exists(buffer) {
                let buffer = buffer.trim_end();
                let mut vec = buffer.split_whitespace().filter_map(|s| if s.is_empty() {None} else {Some(s.to_string())})
                    .collect::<Vec<String>>();
                if vec.is_empty() {return Ok(vec![String::new()]);}
                let last_index = vec.len()-1;
                vec[last_index] = vec[last_index].trim_end().to_owned();
                Ok(vec) 
            } else {
                Err(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    use crate::config_manager::{ConfigurationBuilder, Target};
    use super::{parse_config_file, save_existing_commands_from_config_builder_to_file};

    #[test]
    fn test_save_config_file_and_then_parse_it() -> std::io::Result<()> {
        let command = "./ --exclude a,b,c.txt,d.txt, --braces-as-code --threads 1 1 --hide keywords,timing \
                --force-lang m=matlab,.pl=Perl \
                --style code-number=green,comments-label=magenta bold,arrow=default dim".to_string();
        let config_builder = config_manager::create_config_builder_from_args(&command).unwrap();

        let test_config_dir = Some(LOCAL_APP_PATHS.test_config_dir.clone());
        io_handler::save_existing_commands_from_config_builder_to_file(test_config_dir, "auto-generated", &config_builder)?;

        let (options, issues) = io_handler::parse_config_file(Some("auto-generated"), Some(LOCAL_APP_PATHS.test_config_dir.clone())).unwrap();
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
        assert_eq!(Some(crate::hashmap!("m".to_owned() => "matlab".to_owned(), "pl".to_owned() => "Perl".to_owned())),
                options.forced_languages);
        // Written one pair per line and read back as a group, so a saved look survives a reload
        assert_eq!(config_builder.styles, options.config_styles);
        assert_eq!(3, options.config_styles.as_ref().unwrap().len());

        Ok(())
    }

    // The warning about a contested extension is worth having only if a clean installation never
    // sees it, so the shipped priority file has to answer every contest the shipped languages have
    // between them. Adding a language that takes an extension another one already claims is
    // therefore two files and not one.
    #[test]
    fn every_contest_between_the_shipped_languages_is_settled_by_the_shipped_priority_file() {
        let (languages, _) = io_handler::parse_supported_languages_to_map(&LOCAL_APP_PATHS.languages_dir).unwrap();
        let (priority, faulty) = io_handler::parse_extension_priority_file(
                &(LOCAL_APP_PATHS.data_dir.clone() + crate::EXTENSION_PRIORITY_FILE_NAME));
        assert!(faulty.is_empty(), "the shipped priority file has lines that do not parse: {faulty:?}");

        let (_, report) = crate::make_extension_language_map(&languages, &priority, &HashMap::new());
        let unsettled = report.collisions.iter()
                .filter(|x| x.resolved_by == crate::ResolvedBy::AlphabeticalFallback)
                .map(|x| format!("'{}' between {} and {}", x.extension, x.winner, x.losers.join(", ")))
                .collect::<Vec<_>>();

        assert!(unsettled.is_empty(),
                "these contests are left to the alphabetical tiebreak, so a clean installation is \
                 warned about them on every run. Declare each one in '{}':\n{}",
                crate::EXTENSION_PRIORITY_FILE_NAME, unsettled.join("\n"));
    }

    // Everything above the header is explanation and has to stay explanation, including an example
    // written in the very shape of a rule
    #[test]
    fn the_priority_file_reads_only_what_is_under_its_header() {
        let (rules, faulty) = io_handler::parse_extension_priority(
"Anything up here is prose, and this looks exactly like a rule:
    m       Objective-C, MATLAB

===> contested-extensions
M        Objective-C , MATLAB
pl       Perl
");
        assert!(faulty.is_empty());
        assert_eq!(2, rules.len());
        assert_eq!(Some(&vec!["Objective-C".to_owned(), "MATLAB".to_owned()]), rules.get("m"));
        assert_eq!(Some(&vec!["Perl".to_owned()]), rules.get("pl"));
    }

    #[test]
    fn a_line_of_the_priority_file_that_does_not_parse_is_skipped_and_the_rest_applies() {
        let (rules, faulty) = io_handler::parse_extension_priority(
"===> contested-extensions
m       Objective-C, MATLAB
justoneword
m       Prolog
v       ,  ,
===> some-section-added-later
pl      Perl, Prolog
");
        // A marker ends the block, so the section after it is skipped whole instead of becoming a
        // rule for an extension called '===>'
        assert!(!rules.contains_key("===>") && !rules.contains_key("pl"));
        assert_eq!(1, rules.len());
        // the second declaration is the one that loses, and the decision above it stands
        assert_eq!(Some(&vec!["Objective-C".to_owned(), "MATLAB".to_owned()]), rules.get("m"));
        assert_eq!(vec!["justoneword".to_owned(), "m       Prolog".to_owned(), "v       ,  ,".to_owned()], faulty);
    }

    // The value used to be read as a single line, and a second line of it was dropped in silence:
    // it does not begin with '===>', so the loop that looks for commands simply skipped it, and
    // nothing was reported as invalid because what was read did parse.
    #[test]
    fn a_force_lang_value_written_across_lines_is_read_whole() -> std::io::Result<()> {
        let dir = LOCAL_APP_PATHS.test_config_dir.clone();
        std::fs::create_dir_all(&dir)?;
        let path = dir.clone() + "force-lang-block.txt";
        std::fs::write(&path, "===> dirs\n./\n\n===> force-lang\nm=matlab,\npl=perl\n")?;

        let (options, issues) = io_handler::parse_config_file(Some("force-lang-block"), Some(dir)).unwrap();
        assert!(issues.invalid_fields.is_empty());
        assert_eq!(Some(crate::hashmap!("m".to_owned() => "matlab".to_owned(), "pl".to_owned() => "perl".to_owned())),
                options.forced_languages);

        std::fs::remove_file(&path)
    }

    // The whole point of putting the name inside the target rather than in a field of its own: what
    // '--save' writes has to be what a load reads, or the definitions would drift away from the
    // paths they belong to on the first round trip.
    #[test]
    fn the_modules_of_a_saved_configuration_survive_being_read_back() -> std::io::Result<()> {
        let dir = LOCAL_APP_PATHS.test_config_dir.clone();
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
        let dir = LOCAL_APP_PATHS.test_config_dir.clone();
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
    fn a_language_that_does_not_parse_comes_back_as_none() {
        let good = "Language\nLua\n\nExtensions\nlua\n\nString symbols\n\" '\n\nComment symbols\n--\n";
        assert!(io_handler::parse_string_to_language(good).is_some());
        // and the carriage returns of a windows checkout change nothing about it
        assert_eq!(io_handler::parse_string_to_language(good),
                io_handler::parse_string_to_language(&good.replace('\n', "\r\n")));

        let broken = [
            String::new(),
            "Language\n".to_owned(),
            good.replace("Extensions", "Extension"),
            // an extra blank line, which the loader itself rejects just as flatly
            good.replace("lua\n", "lua\n\n"),
            // no name, no extensions, and no string symbols, each on its own
            good.replace("Lua\n", "\n"),
            good.replace("lua\n\n", "\n\n"),
            good.replace("\" '\n", "\n")
        ];
        for contents in broken {
            assert!(io_handler::parse_string_to_language(&contents).is_none(), "accepted:\n{contents}");
        }
    }

    #[test]
    fn a_missing_priority_file_is_not_a_mistake() {
        let (rules, faulty) = io_handler::parse_extension_priority_file("a/path/that/is/not/there.txt");
        assert!(rules.is_empty() && faulty.is_empty());
    }

    #[test]
    fn test_read_config_file() -> std::io::Result<()> {
        let mut config = Configuration::new(vec!["C:/Some/Path/a".to_owned(),"C:/Some/Path/b".to_owned(),"C:/Some/Path/c".to_owned(),"C:/Some/Path/d".to_owned()]);
        config
            .set_exclude_dirs(vec!["a".to_owned(), "b".to_owned(), "c.txt".to_owned(), "d.txt".to_owned()])
            .set_threads(1,1)
            .set_braces_as_code(true)
            .set_hidden(config_manager::Hidden {bar: true, timing: true, ..Default::default()});


        let (options, issues) = io_handler::parse_config_file(Some("test"), Some(LOCAL_APP_PATHS.test_config_dir.clone())).unwrap();
        assert!(issues.invalid_fields.is_empty() && issues.warnings.is_empty());
        assert_eq!(config.dirs, options.dirs.unwrap());
        assert_eq!(config.exclude_dirs, options.exclude_dirs.unwrap());
        assert_eq!(config.threads, options.threads.unwrap());
        assert_eq!(config.braces_as_code, options.braces_as_code.unwrap());
        assert_eq!(config.should_show_faulty_files, options.should_show_faulty_files.unwrap());
        assert_eq!(config.should_search_in_dotted, options.should_search_in_dotted.unwrap());
        assert_eq!(config.hidden, options.hidden.unwrap());

        Ok(())
    }

    #[test]
    fn test_parse_supported_languages_to_map() {
        let (lang_map, faulty_files) = io_handler::parse_supported_languages_to_map(
                &(LOCAL_APP_PATHS.test_dir.clone() + "languages/")).unwrap();
        assert!(lang_map.len() == 2);
        assert!(faulty_files.len() == 1);
    }

    #[test]
    fn test_load_theme() {
        let dir = std::env::temp_dir().join("mezura_theme_test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Mytheme.txt"), "language-1 = cyan\nlanguage-2 = bright-magenta\ncode-label = bright-yellow italic\n").unwrap();
        std::fs::write(dir.join("Broken.txt"), "language-1 = kaka\nheading = white bold\n").unwrap();
        let dir_str = dir.to_str().unwrap();

        let expected = vec![("language-1".to_owned(), "cyan".to_owned()), ("language-2".to_owned(), "bright-magenta".to_owned()),
                ("code-label".to_owned(), "bright-yellow italic".to_owned())];
        let (loaded, errors) = io_handler::load_theme("mytheme", dir_str).unwrap();
        assert!(errors.is_empty());
        assert_eq!(expected, loaded);
        assert_eq!(expected, io_handler::load_theme("MYTHEME", dir_str).unwrap().0);
        assert!(io_handler::load_theme("nonexistant", dir_str).is_none());

        // A theme that is there always loads, carrying what could not be read. Only a name that
        // points at no file at all is a failure, since only that one is a mistake in the command.
        let (broken, errors) = io_handler::load_theme("broken", dir_str).unwrap();
        assert_eq!(vec![("heading".to_owned(), "white bold".to_owned())], broken);
        assert_eq!(vec![theme::ThemeParseError::InvalidValue("language-1".to_owned(), "kaka".to_owned())], errors);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // The file --save-theme writes has to reproduce the look on its own, which is the whole reason
    // it is flattened instead of pointing at whatever it was built on top of
    #[test]
    fn test_a_saved_theme_reloads_into_the_same_theme() {
        let dir = std::env::temp_dir().join("mezura_theme_save_test");
        let _ = std::fs::remove_dir_all(&dir);
        let dir_str = dir.to_str().unwrap().to_owned() + "/";

        let original = theme::resolve(&[("language-1".to_owned(), "cyan".to_owned())],
                &[("heading".to_owned(), "ff0080 reverse".to_owned())], &[("code-number".to_owned(), "dim".to_owned())]);
        io_handler::save_theme_to_file(&dir_str, "written", &original).unwrap();

        let (styles, errors) = io_handler::load_theme("written", &dir_str).unwrap();
        assert!(errors.is_empty());
        assert_eq!(original, theme::resolve(&styles, &[], &[]));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_default_config_file_is_found_and_parsed() {
        let dir = std::env::temp_dir().join("mezura_default_config_test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("default.txt"), "===> exclude-languages\nSQL\n").unwrap();

        let (options, issues) = io_handler::parse_config_file(None, Some(dir.to_str().unwrap().to_owned() + "/")).unwrap();
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

        let (options, issues) = io_handler::parse_config_file(Some("badcfg"), Some(dir_str)).unwrap();
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

        let (options, issues) = io_handler::parse_config_file(Some("warncfg"), Some(dir_str)).unwrap();
        assert!(issues.invalid_fields.is_empty());
        assert_eq!(3, issues.warnings.len());
        assert!(issues.warnings[0].1.contains("mpampis"));
        assert!(issues.warnings[1].1.contains("labell"));
        assert!(issues.warnings[2].1.contains("heading"));

        assert_eq!(Some(vec![("code-number".to_owned(), "green".to_owned()), ("arrow".to_owned(), "dim".to_owned())]), options.config_styles);
        assert_eq!(Some(config_manager::SortCriterion::Name), options.sort_by);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_serialize_language_overwrites_longer_existing_file() {
        let dir = std::env::temp_dir().join("mezura_serialize_truncate_test");
        std::fs::create_dir_all(&dir).unwrap();
        let dir_str = dir.to_str().unwrap().to_owned();

        let keyword = |name: &str| Keyword {
            descriptive_name: name.to_owned(),
            aliases: vec![name.to_owned()]
        };

        let long_lang = Language::new("Truncatetest".to_owned(), vec!["trnc".to_owned()], vec!["\"".to_owned()],
                vec!["//".to_owned()], Some("/*".to_owned()), Some("*/".to_owned()),
                (0..20).map(|i| keyword(&format!("keyword{i}"))).collect());
        io_handler::serialize_language(&long_lang, &dir_str).unwrap();

        let short_lang = Language::new("Truncatetest".to_owned(), vec!["trnc".to_owned()], vec!["\"".to_owned()],
                vec!["//".to_owned()], Some("/*".to_owned()), Some("*/".to_owned()), vec![keyword("keyword0")]);
        io_handler::serialize_language(&short_lang, &dir_str).unwrap();

        let (lang_map, faulty_files) = io_handler::parse_supported_languages_to_map(&dir_str).unwrap();
        assert!(faulty_files.is_empty());
        assert_eq!(lang_map.get("Truncatetest").unwrap(), &short_lang);

        std::fs::remove_dir_all(&dir).unwrap();
    }
    // Every language file that ships has to parse. CSS, HTML and SCSS were silently rejected for
    // months because the parser demanded a blank line after the multiline comment symbols, which a
    // language with no keywords has no reason to have, and nothing pointed at it: the run simply
    // said "formatting problems" and carried on without them.
    #[test]
    fn every_shipped_language_file_parses() {
        let dir = &LOCAL_APP_PATHS.languages_dir;
        let (languages, faulty) = io_handler::parse_supported_languages_to_map(dir)
                .unwrap_or_else(|e| panic!("the shipped languages dir did not parse at all: {e:?}"));

        assert!(faulty.is_empty(), "these shipped language files do not parse: {faulty:?}");

        let on_disk = std::fs::read_dir(dir).unwrap()
                .flatten()
                .filter(|e| e.path().is_file())
                .count();
        assert_eq!(on_disk, languages.len(),
                "{} language files on disk but {} parsed", on_disk, languages.len());

        // and each one has to describe something countable
        for (name, language) in languages.iter() {
            assert!(!language.extensions.is_empty(), "{name} declares no extension");
            assert!(!language.string_symbols.is_empty(), "{name} declares no string symbol");
            assert_eq!(language.multiline_comment_start_symbol.is_some(),
                    language.multiline_comment_end_symbol.is_some(),
                    "{name} declares only one half of its multiline comment");
        }
    }

}
