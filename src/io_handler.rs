use std::{borrow::Cow, collections::HashMap, fs::{self, DirEntry, File}, io::{self, BufRead, BufReader, BufWriter, Write}, path::Path};

use chrono::{DateTime, Local};
use colored::*;

use crate::{Configuration, DEFAULT_CONFIG_NAME, FinalStats, Formatted, PERSISTENT_APP_PATHS, config_manager::{self, ConfigurationBuilder, LogOption,
     MAX_COMPARE_LEVEL, MAX_CONSUMERS_VALUE, MAX_PRODUCERS_VALUE, MIN_COMPARE_LEVEL, MIN_CONSUMERS_VALUE, MIN_PRODUCERS_VALUE, Threads}, domain::*, split_line_on_whitespace, theme, utils};


const LANGUAGE                 : &str = "Language";     
const EXTENSIONS               : &str = "Extensions";     
const STRING_SYMBOLS           : &str = "String symbols";     
const COMMENT_SYMBOLS          : &str = "Comment symbols";     
const MULTILINE_COMMENT_START  : &str = "Multi line comment start";     
const MULTILINE_COMMENT_END    : &str = "Multi line comment end";     
const KEYWORD                  : &str = "Keyword";     
const KEYWORD_NAME             : &str = "NAME";     
const KEYWORD_ALIASES          : &str = "ALIASES";     


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
        if !reader.read_line_exists(buffer) {return Err(())}
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
        finders : std::sync::OnceLock::new()
    })
}

pub fn parse_string_to_language(contents: Cow<str>) -> Language {
    let mut lines = contents.lines();
    let (mut mult_start, mut mult_end) = (None, None);

    lines.next();
    let lang_name = lines.next().unwrap().trim().to_owned();
    lines.next();
    lines.next();
    let extensions = split_line_on_whitespace(lines.next().unwrap());
    lines.next();
    lines.next();
    let string_symbols = split_line_on_whitespace(lines.next().unwrap());
    lines.next();
    lines.next();
    let comment_symbols = split_line_on_whitespace(lines.next().unwrap());
    let next_line = lines.next();
    if let Some(line) = next_line && line == MULTILINE_COMMENT_START {
        mult_start = Some(lines.next().unwrap().trim().to_owned());
        lines.next();
        mult_end = Some(lines.next().unwrap().trim().to_owned());
        lines.next();
    }

    let mut keywords = Vec::new();
    while let Some(x) = lines.next() {
        if x != KEYWORD {break;} 

        lines.next();
        let k_name = lines.next().unwrap().trim().to_owned();
        lines.next();
        let k_aliases = split_line_on_whitespace(lines.next().unwrap());
        keywords.push(Keyword{
            descriptive_name: k_name,
            aliases: k_aliases
        });
    }

    Language::new(lang_name, extensions, string_symbols, comment_symbols, mult_start, mult_end, keywords)
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


// ------------------------------ Palette handling ------------------------------

pub fn load_palette(name: &str, palettes_dir: &str) -> Option<theme::Palette> {
    let entries = fs::read_dir(palettes_dir).ok()?;
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
        return theme::Palette::parse(&contents).ok();
    }

    None
}

// Palettes are the one data file whose format changed in v3.0.0. The old shape is unambiguous (its
// first meaningful line has no '='), and the conversion loses nothing, so installed palettes are
// rewritten in place rather than starting to fail for everyone who already had them.
pub fn migrate_legacy_palettes(palettes_dir: &str) -> io::Result<Vec<String>> {
    let mut migrated = Vec::new();
    for entry in fs::read_dir(palettes_dir)?.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let Ok(contents) = fs::read_to_string(&path) else { continue };
        if !theme::Palette::is_in_legacy_format(&contents) {
            continue;
        }

        let Some(palette) = theme::Palette::from_legacy_format(&contents) else { continue };
        fs::write(&path, palette.to_file_contents())?;
        if let Some(name) = path.file_stem().and_then(|x| x.to_str()) {
            migrated.push(name.to_owned());
        }
    }

    Ok(migrated)
}


pub fn generate_palette_tuner_page() -> io::Result<String> {
    fn js_escape(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"").replace('<', "\\u003c")
    }

    let template = include_str!("../docs/palette-tuner/index.html");

    let mut entries: Vec<(String, Vec<String>)> = Vec::new();
    for entry in fs::read_dir(&PERSISTENT_APP_PATHS.palettes_dir)?.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|x| x.to_str()) else { continue };
        let Ok(contents) = fs::read_to_string(&path) else { continue };
        // The page edits the language slots only, so a palette without them has nothing to show
        let Ok(palette) = theme::Palette::parse(&contents) else { continue };
        let Some(colors) = palette.languages else { continue };
        entries.push((stem.to_owned(), colors.iter().map(utils::color_to_config_string).collect()));
    }
    entries.sort_by_key(|x| x.0.to_lowercase());

    let palettes_js = entries.iter().map(|(name, tokens)| {
        format!("{{name:\"{}\",tokens:[{}]}}", js_escape(name),
            tokens.iter().map(|t| format!("\"{}\"", js_escape(t))).collect::<Vec<_>>().join(","))
    }).collect::<Vec<_>>().join(",");

    let page = template.replace("/*MEZURA_SYSTEM_PALETTES*/", &format!("SYSTEM_PALETTES = [{palettes_js}];"));

    let out_path = PERSISTENT_APP_PATHS.data_dir.clone() + "palette-tuner.html";
    fs::write(&out_path, page)?;

    Ok(out_path)
}


// ------------------------------ Config handling ------------------------------

pub fn parse_config_file(file_name: Option<&str>, config_dir_path: Option<String>) -> Result<(ConfigurationBuilder, Vec<&'static str>),ConfigFileParseError> {
    let config_path = if let Some(dir) = config_dir_path {dir} else {PERSISTENT_APP_PATHS.config_dir.clone()};
    let file_name = if let Some(x) = file_name {x} else {DEFAULT_CONFIG_NAME.trim_end_matches(".txt")};
    let file_path = (config_path + file_name + ".txt").replace("\\", "/");
    let mut reader = BufReader::new(match fs::File::open(file_path){
        Ok(f) => f,
        Err(_) => return Err(ConfigFileParseError::FileNotFound(file_name.to_owned()))
    });

    let (mut dirs, mut braces_as_code, mut should_search_in_dotted, mut threads, mut exclude_dirs,
         mut languages_of_interest, mut excluded_languages, mut should_show_faulty_files, mut no_keywords, mut no_visual,
         mut no_gitignore, mut colors, mut color_palette, mut log, mut compare_level, mut styles) = (None,None,None,None,None,None,None,None,None,None,None,None,None,None,None,None);
    let mut invalid_fields: Vec<&'static str> = Vec::new();
    let mut buf = String::with_capacity(150);

    while let Ok(size) = reader.read_line(&mut buf) {
        if size == 0 {break};
        if buf.trim().starts_with("===>") {
            let id = buf.split(' ').nth(1).unwrap_or("").trim();

            if id == config_manager::DIRS {
                let paths = read_lines_from_file_to_vec(&mut reader, &mut buf, utils::parse_paths_to_vec);
                if !paths.is_empty() {
                    dirs = Some(paths);
                }
            } else if id == config_manager::EXCLUDE {
                let paths = read_lines_from_file_to_vec(&mut reader, &mut buf, utils::parse_paths_to_vec);
                if utils::build_exclude_matcher(&paths).is_err() {
                    invalid_fields.push(config_manager::EXCLUDE);
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
            } else if id == config_manager::THREADS {
                buf.clear();
                let _ = reader.read_line(&mut buf);
                match utils::parse_two_usize_values(&buf,MIN_PRODUCERS_VALUE, MAX_PRODUCERS_VALUE,
                        MIN_CONSUMERS_VALUE, MAX_CONSUMERS_VALUE) {
                    Some(x) => threads = Some(Threads::from(x)),
                    None => invalid_fields.push(config_manager::THREADS)
                }
            }else if id == config_manager::BRACES_AS_CODE {
                match read_bool_value_from_file(&mut reader, &mut buf) {
                    Ok(x) => braces_as_code = x,
                    Err(()) => invalid_fields.push(config_manager::BRACES_AS_CODE)
                }
            } else if id == config_manager::SHOW_FAULTY_FILES {
                match read_bool_value_from_file(&mut reader, &mut buf) {
                    Ok(x) => should_show_faulty_files = x,
                    Err(()) => invalid_fields.push(config_manager::SHOW_FAULTY_FILES)
                }
            } else if id == config_manager::SEARCH_IN_DOTTED {
                match read_bool_value_from_file(&mut reader, &mut buf) {
                    Ok(x) => should_search_in_dotted = x,
                    Err(()) => invalid_fields.push(config_manager::SEARCH_IN_DOTTED)
                }
            } else if id == config_manager::NO_KEYWORDS {
                match read_bool_value_from_file(&mut reader, &mut buf) {
                    Ok(x) => no_keywords = x,
                    Err(()) => invalid_fields.push(config_manager::NO_KEYWORDS)
                }
            } else if id == config_manager::NO_VISUAL {
                match read_bool_value_from_file(&mut reader, &mut buf) {
                    Ok(x) => no_visual = x,
                    Err(()) => invalid_fields.push(config_manager::NO_VISUAL)
                }
            } else if id == config_manager::NO_GITIGNORE {
                match read_bool_value_from_file(&mut reader, &mut buf) {
                    Ok(x) => no_gitignore = x,
                    Err(()) => invalid_fields.push(config_manager::NO_GITIGNORE)
                }
            } else if id == config_manager::COLORS {
                buf.clear();
                let _ = reader.read_line(&mut buf);
                match utils::parse_colors_to_vec(&buf) {
                    Some(x) => colors = Some(x),
                    None => invalid_fields.push(config_manager::COLORS)
                }
            } else if id == config_manager::COLOR_PALETTE {
                buf.clear();
                let _ = reader.read_line(&mut buf);
                let name = buf.trim();
                if name.is_empty() || load_palette(name, &PERSISTENT_APP_PATHS.palettes_dir).is_none() {
                    invalid_fields.push(config_manager::COLOR_PALETTE);
                } else {
                    color_palette = Some(name.to_owned());
                }
            } else if id == config_manager::STYLE {
                buf.clear();
                let _ = reader.read_line(&mut buf);
                match theme::parse_overrides(&buf) {
                    Ok(x) => styles = Some(x),
                    Err(_) => invalid_fields.push(config_manager::STYLE)
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
                    None => invalid_fields.push(config_manager::COMPRARE_LEVEL)
                }
            }
        }
        buf.clear();
    }

    let builder = ConfigurationBuilder {
        dirs, exclude_dirs, languages_of_interest, excluded_languages, threads, braces_as_code, should_search_in_dotted,
        should_show_faulty_files, no_keywords, no_visual, no_gitignore, colors, color_palette, log, compare_level, styles,
        ..Default::default()
    };

    Ok((builder, invalid_fields))
}

// Dirs must be specified (is checked before calling this function)
pub fn save_existing_commands_from_config_builder_to_file(config_path: Option<String>, config_name: &str, config_builder: &ConfigurationBuilder) 
-> std::io::Result<()> 
{
    let config_dir = if let Some(dir) = config_path {dir} else {PERSISTENT_APP_PATHS.config_dir.clone()};
    let file_name = config_dir + config_name + ".txt";

    let mut writer = BufWriter::new(std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(file_name)?);

    writer.write_all(b"Auto-generated config file.")?;

    writer.write_all(&[b"\n\n===> ",config_manager::DIRS.as_bytes(),b"\n"].concat())?;
    writer.write_all(config_builder.dirs.as_ref().unwrap().join(",").as_bytes())?;

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
    if let Some(no_keywords) = &config_builder.no_keywords {
        writer.write_all(&[b"\n\n===> ",config_manager::NO_KEYWORDS.as_bytes(),b"\n"].concat())?;
        writer.write_all(if *no_keywords {b"yes"} else {b"no"})?;
    }
    if let Some(no_visual) = &config_builder.no_visual {
        writer.write_all(&[b"\n\n===> ",config_manager::NO_VISUAL.as_bytes(),b"\n"].concat())?;
        writer.write_all(if *no_visual {b"yes"} else {b"no"})?;
    }
    if let Some(no_gitignore) = &config_builder.no_gitignore {
        writer.write_all(&[b"\n\n===> ",config_manager::NO_GITIGNORE.as_bytes(),b"\n"].concat())?;
        writer.write_all(if *no_gitignore {b"yes"} else {b"no"})?;
    }
    if let Some(colors) = &config_builder.colors {
        writer.write_all(&[b"\n\n===> ",config_manager::COLORS.as_bytes(),b"\n"].concat())?;
        writer.write_all(colors.iter().map(utils::color_to_config_string)
                .collect::<Vec<_>>().join(" ").as_bytes())?;
    }
    if let Some(styles) = &config_builder.styles {
        let rendered = styles.iter().map(|(token, style)| format!("{token}={style}")).collect::<Vec<_>>().join(",");
        writer.write_all(&[b"\n\n===> ",config_manager::STYLE.as_bytes(),b"\n"].concat())?;
        writer.write_all(rendered.as_bytes())?;
    }

    if let Some(color_palette) = &config_builder.color_palette {
        writer.write_all(&[b"\n\n===> ",config_manager::COLOR_PALETTE.as_bytes(),b"\n"].concat())?;
        writer.write_all(color_palette.as_bytes())?;
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

pub fn log_stats(path: &str, contents: &Option<String>, final_stats: &FinalStats, datetime_now: &DateTime<Local>, config: &Configuration) -> io::Result<()> {
    let mut writer = std::io::BufWriter::new(std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(path)?);

    write_current_log(&mut writer, config, datetime_now, final_stats)?;

    if let Some(contents) = contents {
        writer.write_all(contents.as_bytes())?;
    }
    writer.flush()?;

    Ok(())
}

fn write_current_log(writer: &mut BufWriter<File>, config: &Configuration, datetime_now: &DateTime<Local>, final_stats: &FinalStats) -> io::Result<()> {
    writer.write_all(format!("===>{}\n",config.log.name.clone().unwrap_or_default()).as_bytes())?;
    writer.write_all(datetime_now.format("%Y-%m-%d %H:%M:%S %z").to_string().as_bytes())?;
    writer.write_all(b"\n")?;
    writer.write_all(b"Configuration:\n")?;
    writer.write_all(format!("    dirs: {}\n",config.dirs.join(",")).as_bytes())?;
    writer.write_all(format!("    exclude: {}\n",config.exclude_dirs.join(",")).as_bytes())?;
    writer.write_all(format!("    languages: {}\n",config.languages_of_interest.join(",")).as_bytes())?;
    writer.write_all(format!("    excluded-languages: {}\n",config.excluded_languages.join(",")).as_bytes())?;
    writer.write_all(format!("    braces-as-code: {}\n",if config.braces_as_code{"yes"} else {"no"}).as_bytes())?;
    writer.write_all(format!("    search-in-dotted: {}\n",if config.should_search_in_dotted{"yes"} else {"no"}).as_bytes())?;
    writer.write_all(b"Stats:\n")?;
    writer.write_all(format!("    Files: {}\n",final_stats.files).as_bytes())?;
    writer.write_all(format!("    Lines: {}\n",final_stats.lines).as_bytes())?;
    writer.write_all(format!("        Code: {}\n",final_stats.code_lines).as_bytes())?;
    writer.write_all(format!("        Extra: {}\n",final_stats.extra_lines).as_bytes())?;
    writer.write_all(format!("    Total Size: {}\n",final_stats.bytes_size).as_bytes())?;
    writer.write_all(format!("        Average Size: {}\n\n\n",final_stats.bytes_average_size).as_bytes())?;
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
fn read_lines_from_file_to_vec(reader: &mut BufReader<File>, buf: &mut String, parser_func: fn(&str) -> Vec<String>) -> Vec<String> {
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

    #[test]
    fn test_save_config_file_and_then_parse_it() -> std::io::Result<()> {
        let command = "./ --exclude a,b,c.txt,d.txt, --braces-as-code --threads 1 1".to_string();
        let config_builder = config_manager::create_config_builder_from_args(&command).unwrap();

        let test_config_dir = Some(LOCAL_APP_PATHS.test_config_dir.clone());
        io_handler::save_existing_commands_from_config_builder_to_file(test_config_dir, "auto-generated", &config_builder)?;

        let (options, invalid_fields) = io_handler::parse_config_file(Some("auto-generated"), Some(LOCAL_APP_PATHS.test_config_dir.clone())).unwrap();
        assert!(invalid_fields.is_empty());
        assert_eq!(config_builder.dirs, options.dirs);
        assert_eq!(config_builder.exclude_dirs, options.exclude_dirs);
        assert_eq!(config_builder.threads, options.threads);
        assert_eq!(config_builder.braces_as_code, options.braces_as_code);
        assert_eq!(config_builder.should_show_faulty_files, options.should_show_faulty_files);
        assert_eq!(config_builder.should_search_in_dotted, options.should_search_in_dotted);
        assert_eq!(config_builder.no_visual, options.no_visual);

        Ok(())
    }

    #[test]
    fn test_read_config_file() -> std::io::Result<()> {
        let mut config = Configuration::new(vec!["C:/Some/Path/a".to_owned(),"C:/Some/Path/b".to_owned(),"C:/Some/Path/c".to_owned(),"C:/Some/Path/d".to_owned()]);
        config
            .set_exclude_dirs(vec!["a".to_owned(), "b".to_owned(), "c.txt".to_owned(), "d.txt".to_owned()])
            .set_threads(1,1)
            .set_braces_as_code(true);


        let (options, invalid_fields) = io_handler::parse_config_file(Some("test"), Some(LOCAL_APP_PATHS.test_config_dir.clone())).unwrap();
        assert!(invalid_fields.is_empty());
        assert_eq!(config.dirs, options.dirs.unwrap());
        assert_eq!(config.exclude_dirs, options.exclude_dirs.unwrap());
        assert_eq!(config.threads, options.threads.unwrap());
        assert_eq!(config.braces_as_code, options.braces_as_code.unwrap());
        assert_eq!(config.should_show_faulty_files, options.should_show_faulty_files.unwrap());
        assert_eq!(config.should_search_in_dotted, options.should_search_in_dotted.unwrap());
        assert_eq!(config.no_visual, options.no_visual.unwrap());

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
    fn test_load_palette() {
        let dir = std::env::temp_dir().join("mezura_palette_test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Mypalette.txt"), "languages = cyan bright-magenta ff0080\nlabel = bright-yellow italic\n").unwrap();
        std::fs::write(dir.join("Broken.txt"), "languages = kaka\n").unwrap();
        std::fs::write(dir.join("Legacy.txt"), "cyan bright-magenta ff0080\n").unwrap();
        let dir_str = dir.to_str().unwrap();

        let expected_colors = Some(vec![Color::Cyan, Color::BrightMagenta, Color::TrueColor{r:255,g:0,b:128}]);
        let loaded = io_handler::load_palette("mypalette", dir_str).unwrap();
        assert_eq!(expected_colors, loaded.languages);
        assert_eq!(vec![("label".to_owned(), "bright-yellow italic".to_owned())], loaded.styles);
        assert_eq!(expected_colors, io_handler::load_palette("MYPALETTE", dir_str).unwrap().languages);
        assert!(io_handler::load_palette("nonexistant", dir_str).is_none());
        assert!(io_handler::load_palette("broken", dir_str).is_none());

        // A palette left in the pre-v3 format is rewritten in place and loads normally afterwards
        assert!(io_handler::load_palette("legacy", dir_str).is_none());
        assert_eq!(vec!["Legacy".to_owned()], io_handler::migrate_legacy_palettes(dir_str).unwrap());
        assert_eq!(expected_colors, io_handler::load_palette("legacy", dir_str).unwrap().languages);
        assert!(io_handler::migrate_legacy_palettes(dir_str).unwrap().is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_default_config_file_is_found_and_parsed() {
        let dir = std::env::temp_dir().join("mezura_default_config_test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("default.txt"), "===> exclude-languages\nSQL\n").unwrap();

        let (options, invalid_fields) = io_handler::parse_config_file(None, Some(dir.to_str().unwrap().to_owned() + "/")).unwrap();
        assert!(invalid_fields.is_empty());
        assert_eq!(Some(vec!["sql".to_owned()]), options.excluded_languages);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_config_file_reports_invalid_values() {
        let dir = std::env::temp_dir().join("mezura_invalid_config_test");
        std::fs::create_dir_all(&dir).unwrap();
        let dir_str = dir.to_str().unwrap().to_owned() + "/";

        std::fs::write(dir.join("badcfg.txt"),
                "===> threads\n3343 45534\n\n===> braces-as-code\nmitsos\n\n===> compare\n99\n\n===> no-visual\nyes\n").unwrap();

        let (options, invalid_fields) = io_handler::parse_config_file(Some("badcfg"), Some(dir_str)).unwrap();
        assert_eq!(invalid_fields, vec![config_manager::THREADS, config_manager::BRACES_AS_CODE, config_manager::COMPRARE_LEVEL]);
        assert_eq!(options.threads, None);
        assert_eq!(options.braces_as_code, None);
        assert_eq!(options.compare_level, None);
        assert_eq!(options.no_visual, Some(true));

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
}
