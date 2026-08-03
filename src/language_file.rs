// The format of a language file, which is the one thing under this roof that decides a number: what
// a language is called, which extensions it claims and which symbols open a comment. Reading a
// definition is this crate's business; installing, replacing and migrating the files that hold them
// is the command line's, and lives there.
use std::{collections::HashMap, fs::{self, DirEntry}, path::Path};

use crate::{Keyword, Language};

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

const REGENERATE_LANGUAGES_HINT : &str =
        "Delete the \"languages\" folder and it will be generated again on the next execution.\nThe \"config\" and \"logs\" folders will not be affected.";

#[derive(Debug)]
pub enum LanguageDirParseError {
    NoFilesFound,
    NoFilesFormattedProperly,
    PathMissing(String)
}

impl std::fmt::Display for LanguageDirParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoFilesFound => write!(f, "No language files found in directory.
{REGENERATE_LANGUAGES_HINT}"),
            Self::NoFilesFormattedProperly => write!(f, "No language file is formatted properly, so none could be parsed.
{REGENERATE_LANGUAGES_HINT}"),
            Self::PathMissing(path) => write!(f, "It seems that the language dir ({path}) has been deleted.
{REGENERATE_LANGUAGES_HINT}")
        }
    }
}

impl std::error::Error for LanguageDirParseError {}

pub fn parse_dir(target_path: &str) -> Result<(HashMap<String, Language>, Vec<String>), LanguageDirParseError> {
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

        let Ok(reader) = line_reader::LineReader::open(path) else {
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

// Returns None instead of panicking on anything it does not recognise, because it is no longer only
// read over the baked-in files, which are ours and are correct by construction. The migration reads
// what is on the user's disk through it, to ask whether their copy of a file still means what our
// copy means, and a file that somebody edited into something unparseable must come back as "not the
// same" rather than take the run down with it.
pub fn parse_definition(contents: &str) -> Option<Language> {
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

// A missing file is not a mistake: an installation made by an earlier version has none, and the
// only consequence is that contested extensions fall back to the alphabetical tiebreak, which
// announces itself anyway.
pub fn parse_priority_file(path: &str) -> (HashMap<String,Vec<String>>, Vec<String>) {
    match fs::read_to_string(path) {
        Ok(contents) => parse_priority(&contents),
        Err(_) => (HashMap::new(), Vec::new())
    }
}

// A line that does not parse is reported and skipped while the rest of the file applies, because a
// mistake here cannot produce a wrong number in silence: the extension it failed to settle falls
// through to the tiebreak, which says so by name.
pub fn parse_priority(contents: &str) -> (HashMap<String,Vec<String>>, Vec<String>) {
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
        let names = claimants.split(',').map(str::trim).filter(|x| !x.is_empty())
                .map(str::to_owned).collect::<Vec<_>>();
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

fn split_line_on_whitespace(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::trim).filter(|x| !x.is_empty()).map(str::to_owned).collect()
}

fn parse_file_to_language(mut reader :line_reader::LineReader, buffer :&mut String) -> Result<Language,()> {
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

// A line at a time over a file, into a buffer the caller owns and this clears, which is how the
// language file format is read: every rule in it is one line.
mod line_reader {
    use std::{fs::File, io::{self, prelude::*}};

    pub struct LineReader {
        reader: io::BufReader<File>,
    }

    impl LineReader {
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
    use super::*;
    use crate::test_paths::{DATA_DIR, LANGUAGES_DIR, TEST_DIR};
    #[test]
    fn every_contest_between_the_shipped_languages_is_settled_by_the_shipped_priority_file() {
        let (languages, _) = crate::language_file::parse_dir(LANGUAGES_DIR).unwrap();
        let (priority, faulty) = crate::language_file::parse_priority_file(
                &(DATA_DIR.to_owned() + crate::EXTENSION_PRIORITY_FILE_NAME));
        assert!(faulty.is_empty(), "the shipped priority file has lines that do not parse: {faulty:?}");

        let (_, report) = crate::engine::extensions::make_extension_language_map(&languages, &priority, &HashMap::new());
        let unsettled = report.collisions.iter()
                .filter(|x| x.resolved_by == crate::engine::extensions::ResolvedBy::AlphabeticalFallback)
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
        let (rules, faulty) = crate::language_file::parse_priority(
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
        let (rules, faulty) = crate::language_file::parse_priority(
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

    #[test]
    fn a_language_that_does_not_parse_comes_back_as_none() {
        let good = "Language\nLua\n\nExtensions\nlua\n\nString symbols\n\" '\n\nComment symbols\n--\n";
        assert!(crate::language_file::parse_definition(good).is_some());
        // and the carriage returns of a windows checkout change nothing about it
        assert_eq!(crate::language_file::parse_definition(good),
                crate::language_file::parse_definition(&good.replace('\n', "\r\n")));

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
            assert!(crate::language_file::parse_definition(&contents).is_none(), "accepted:\n{contents}");
        }
    }
    #[test]
    fn a_missing_priority_file_is_not_a_mistake() {
        let (rules, faulty) = crate::language_file::parse_priority_file("a/path/that/is/not/there.txt");
        assert!(rules.is_empty() && faulty.is_empty());
    }
    #[test]
    fn test_parse_dir() {
        let (lang_map, faulty_files) = crate::language_file::parse_dir(
                &(TEST_DIR.to_owned() + "languages/")).unwrap();
        assert!(lang_map.len() == 2);
        assert!(faulty_files.len() == 1);
    }

    // Every language file that ships has to parse. CSS, HTML and SCSS were silently rejected for
    // months because the parser demanded a blank line after the multiline comment symbols, which a
    // language with no keywords has no reason to have, and nothing pointed at it: the run simply
    // said "formatting problems" and carried on without them.
    #[test]
    fn every_shipped_language_file_parses() {
        let dir = LANGUAGES_DIR;
        let (languages, faulty) = crate::language_file::parse_dir(dir)
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
