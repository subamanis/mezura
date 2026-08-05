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

pub fn parse_languages_in_dir(target_path: &str) -> Result<(HashMap<String, Language>, Vec<String>), LanguageDirParseError> {
    fn add_file_name_to_faulty_files(entry: &DirEntry, faulty_files: &mut Vec<String>) {
        let file_name = entry.file_name().to_str().map_or(String::new(), |x| x.to_owned());
        if !file_name.is_empty() {faulty_files.push(file_name.to_lowercase())}
    }

    let mut language_map = HashMap::with_capacity(30);
    let mut faulty_files : Vec<String> = Vec::new();

    let entries = fs::read_dir(target_path);
    if entries.is_err() {
        return Err(LanguageDirParseError::PathMissing(target_path.to_owned()));
    }
    for entry in entries.unwrap() {
        let Ok(entry) = entry else { continue };

        let path = entry.path();
        if !Path::new(&path).is_file() {continue;}

        // Read whole and parsed as a string, the one way the format is parsed: a definition on
        // disk and one baked into the binary go through the same 'parse_language'.
        match fs::read_to_string(&path).ok().and_then(|contents| parse_language(&contents)) {
            Some(language) => { language_map.insert(language.name.to_owned(), language); },
            None => add_file_name_to_faulty_files(&entry, &mut faulty_files)
        }
    }

    if language_map.is_empty() && faulty_files.is_empty() {
        Err(LanguageDirParseError::NoFilesFound)
    } else if language_map.is_empty() {
        Err(LanguageDirParseError::NoFilesFormattedProperly)
    } else {
        Ok((language_map, faulty_files))
    }
}

// The one parser of the language file format, over a string. 'parse_languages_in_dir' reads each
// file into one and calls this, so a file on disk and the baked-in bytes go through identical code:
// there is no second parser to drift from, which is what let one read every keyword and one none.
//
// Two rules keep it robust. A value sits on the line right after its header, taken as it is even
// when empty, because a language with only multiline comments has an empty 'Comment symbols' value
// and that empty line is the value, not a separator. Blank lines are skipped only before a header,
// between blocks, so an extra one, or the one that always separates the keyword blocks from the
// symbols above them, never derails the parse.
//
// Returns None on anything it does not recognise rather than panicking, because the migration reads
// what is on the user's disk through it to ask whether their copy still means what ours means, and a
// file edited into something unparseable must come back as "not the same" and not take the run down.
pub fn parse_language(contents: &str) -> Option<Language> {
    let mut lines = contents.lines();

    if next_header(&mut lines)? != LANGUAGE {return None;}
    let lang_name = value_line(&mut lines)?;
    if lang_name.is_empty() {return None;}

    if next_header(&mut lines)? != EXTENSIONS {return None;}
    let extensions = split_line_on_whitespace(&value_line(&mut lines)?);
    if extensions.is_empty() {return None;}

    if next_header(&mut lines)? != STRING_SYMBOLS {return None;}
    let string_symbols = split_line_on_whitespace(&value_line(&mut lines)?);
    if string_symbols.is_empty() {return None;}

    if next_header(&mut lines)? != COMMENT_SYMBOLS {return None;}
    // Deliberately allowed to be empty: a language whose only comments are multiline has no line
    // comment symbol, and the value here is the empty line that says so.
    let comment_symbols = split_line_on_whitespace(&value_line(&mut lines)?);

    let (mut mult_start, mut mult_end) = (None, None);
    let mut header = next_header(&mut lines);
    if header.as_deref() == Some(MULTILINE_COMMENT_START) {
        let start = value_line(&mut lines)?;
        if start.is_empty() || next_header(&mut lines)?.as_str() != MULTILINE_COMMENT_END {return None;}
        let end = value_line(&mut lines)?;
        if end.is_empty() {return None;}
        (mult_start, mult_end) = (Some(start), Some(end));
        header = next_header(&mut lines);
    }

    let mut keywords = Vec::new();
    while header.as_deref() == Some(KEYWORD) {
        if next_header(&mut lines)?.as_str() != KEYWORD_NAME {return None;}
        let name = value_line(&mut lines)?;
        if next_header(&mut lines)?.as_str() != KEYWORD_ALIASES {return None;}
        let aliases = split_line_on_whitespace(&value_line(&mut lines)?);
        if name.is_empty() || aliases.is_empty() {return None;}

        keywords.push(Keyword{descriptive_name: name, aliases});
        header = next_header(&mut lines);
    }

    Some(Language::new(lang_name, extensions, string_symbols, comment_symbols, mult_start, mult_end, keywords))
}

// The next line that carries a header, with the blank lines between blocks skipped. Trimmed, so an
// indented sub-header like the 'NAME' of a keyword block is recognised.
fn next_header(lines: &mut std::str::Lines) -> Option<String> {
    lines.by_ref().map(str::trim).find(|line| !line.is_empty()).map(str::to_owned)
}

// The value that belongs to the header just read: the very next line, trimmed, empty or not. Never
// skips a blank, because for the comment symbols the blank line is the value.
fn value_line(lines: &mut std::str::Lines) -> Option<String> {
    lines.next().map(|line| line.trim().to_owned())
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


#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_paths::{DATA_DIR, FIXTURES_DIR, LANGUAGES_DIR};

    // The parser reads the real shipped files correctly, which is where the weight belongs: a
    // hand-written string that copies the format is a second source of truth that rots. There is
    // one parser, 'parse_language'; 'parse_languages_in_dir' is not a second one, it reads each file and calls
    // it, so reading the languages directory is reading the files through it.
    #[test]
    fn the_parser_reads_the_shipped_files_and_a_blank_line_never_costs_a_block() {
        let (languages, faulty) = parse_languages_in_dir(LANGUAGES_DIR).unwrap();
        assert!(faulty.is_empty(), "shipped files that did not parse: {faulty:?}");

        // The bug this fixed: the keywords sit below a blank line, and every one of them was dropped.
        let rust = languages.get("Rust").expect("Rust is shipped");
        assert!(rust.keywords.iter().any(|k| k.descriptive_name == "structs"),
                "Rust lost its keywords: {:?}", rust.keywords);

        // A language whose only comments are multiline has an empty 'Comment symbols' value, and
        // that empty line is the value and not a separator to skip, or the symbols one line down
        // would read as it.
        let css = languages.get("CSS").expect("CSS is shipped");
        assert!(css.comment_symbols.is_empty() && css.multiline_comment_start_symbol.is_some(),
                "CSS was the empty-comment-symbols case and its shape changed");

        // The one thing no shipped file can show, because a stray blank line in one would be tidied
        // away as a mistake: an extra blank line between blocks does not derail the parse. Fed as a
        // string, which is what 'parse_language' takes and what 'parse_languages_in_dir' hands it per file.
        let padded = "Language\nJava\n\n\nExtensions\njava\n\n\n\nString symbols\n\"\n\n\
Comment symbols\n//\n\n\nKeyword\n    NAME\n    classes\n    ALIASES\n    class\n";
        let java = parse_language(padded).expect("an extra blank line broke the parse");
        assert_eq!(vec!["classes"], java.keywords.iter().map(|k| k.descriptive_name.clone()).collect::<Vec<_>>());
    }
    #[test]
    fn every_contest_between_the_shipped_languages_is_settled_by_the_shipped_priority_file() {
        let (languages, _) = crate::language_file::parse_languages_in_dir(LANGUAGES_DIR).unwrap();
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
        assert!(crate::language_file::parse_language(good).is_some());
        // and the carriage returns of a windows checkout change nothing about it
        assert_eq!(crate::language_file::parse_language(good),
                crate::language_file::parse_language(&good.replace('\n', "\r\n")));

        let broken = [
            String::new(),
            "Language\n".to_owned(),
            good.replace("Extensions", "Extension"),
            // no name, no extensions, and no string symbols, each on its own
            good.replace("Lua\n", "\n"),
            good.replace("lua\n\n", "\n\n"),
            good.replace("\" '\n", "\n")
        ];
        for contents in broken {
            assert!(crate::language_file::parse_language(&contents).is_none(), "accepted:\n{contents}");
        }

        // An extra blank line between blocks is no longer a mistake: the parser skips blanks before
        // a header, so a stray one does not cost a language its whole definition.
        assert!(crate::language_file::parse_language(&good.replace("lua\n", "lua\n\n")).is_some());
    }
    #[test]
    fn a_missing_priority_file_is_not_a_mistake() {
        let (rules, faulty) = crate::language_file::parse_priority_file("a/path/that/is/not/there.txt");
        assert!(rules.is_empty() && faulty.is_empty());
    }
    #[test]
    fn test_parse_languages_in_dir() {
        let (lang_map, faulty_files) = crate::language_file::parse_languages_in_dir(
                &(FIXTURES_DIR.to_owned() + "definitions/")).unwrap();
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
        let (languages, faulty) = crate::language_file::parse_languages_in_dir(dir)
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
