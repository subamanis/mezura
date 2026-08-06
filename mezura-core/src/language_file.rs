// The format of a language file, which is the one thing under this roof that decides a number: what
// a language is called, which extensions it claims and which symbols open a comment. Reading a
// definition is this crate's business; installing, replacing and migrating the files that hold them
// is the command line's, and lives there.
use std::{collections::HashMap, fs::{self, DirEntry}, path::Path};

use crate::{Keyword, Language};

// The headers a language file is written with, spelled as they appear in it
const LANGUAGE                 : &str = "Language";
const EXTENSIONS               : &str = "Extensions";
const STRING_SYMBOLS           : &str = "String symbols";
const COMMENT_SYMBOLS          : &str = "Comment symbols";
const MULTILINE_COMMENT_START  : &str = "Multi line comment start";
const MULTILINE_COMMENT_END    : &str = "Multi line comment end";
const KEYWORD                  : &str = "Keyword";
const KEYWORD_NAME             : &str = "NAME";
const KEYWORD_ALIASES          : &str = "ALIASES";

// The marker that opens the rules block of the extension priority file
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

// Two different mistakes: a path that is not there is the caller's, text that does not parse is the
// file's. Collapsed into one answer, somebody hunts for a formatting problem in a file they
// misspelled the name of.
#[derive(Debug)]
pub enum LanguageFileError {
    Unreadable(std::io::Error),
    Malformed
}

impl std::fmt::Display for LanguageFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable(error) => write!(f, "the language file could not be read: {error}"),
            Self::Malformed => write!(f, "the language file is not written in the format mezura reads")
        }
    }
}

impl std::error::Error for LanguageFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unreadable(error) => Some(error),
            Self::Malformed => None
        }
    }
}

#[derive(Debug)]
pub struct FaultyLanguageFile {
    pub file_name: String,
    pub error: LanguageFileError
}

// A list and not a map keyed by name, since the name lives in the language itself and a second copy
// of it is a second thing to disagree. The files that failed come back beside it: each is a whole
// language missing from the count.
pub fn parse_languages_in_dir(target_path: impl AsRef<Path>)
-> Result<(Vec<Language>, Vec<FaultyLanguageFile>), LanguageDirParseError> {
    // As it is spelled on disk. Lowercasing it named a file that does not exist anywhere the
    // filesystem is case sensitive, and the whole point of the list is that somebody can go and
    // open the file it names.
    fn add_to_faulty_files(entry: &DirEntry, error: LanguageFileError, faulty_files: &mut Vec<FaultyLanguageFile>) {
        let file_name = entry.file_name().to_str().map_or(String::new(), |x| x.to_owned());
        if !file_name.is_empty() {faulty_files.push(FaultyLanguageFile { file_name, error })}
    }

    let target_path = target_path.as_ref();
    let mut languages = Vec::with_capacity(30);
    let mut faulty_files : Vec<FaultyLanguageFile> = Vec::new();

    let entries = fs::read_dir(target_path);
    if entries.is_err() {
        return Err(LanguageDirParseError::PathMissing(target_path.display().to_string()));
    }
    for entry in entries.unwrap() {
        let Ok(entry) = entry else { continue };

        let path = entry.path();
        if !path.is_file() {continue;}

        match parse_language_file(&path) {
            Ok(language) => languages.push(language),
            Err(error) => add_to_faulty_files(&entry, error, &mut faulty_files)
        }
    }

    if languages.is_empty() && faulty_files.is_empty() {
        Err(LanguageDirParseError::NoFilesFound)
    } else if languages.is_empty() {
        Err(LanguageDirParseError::NoFilesFormattedProperly)
    } else {
        Ok((languages, faulty_files))
    }
}

pub fn parse_language_file(path: impl AsRef<Path>) -> Result<Language, LanguageFileError> {
    let contents = fs::read_to_string(path).map_err(LanguageFileError::Unreadable)?;
    parse_language(&contents).ok_or(LanguageFileError::Malformed)
}

// The one parser of the language file format. A file on disk and the bytes baked into this crate go
// through it alike, so there is no second parser to drift from.
//
// Three rules. A value sits on the line straight after its header and is taken as it is even when
// empty, because a language with only multiline comments has an empty 'Comment symbols' value and
// that empty line is the value, not a separator. Blank lines are skipped only before a header, so a
// spare one never derails the parse.
//
// And **every line has to be accounted for**, which is the rule that decides whether a mistake is
// loud or silent: a header left over at the end is one this did not understand, and the file is
// refused rather than half kept. Without it, 'Multiline comment start' written for 'Multi line
// comment start' was accepted with no multiline comments and no keywords at all, and that language
// then counted its block comments as code.
//
// Returns None rather than panicking on anything unrecognised: the version migration reads what is on
// the user's disk through this to ask whether their copy still means what ours does, and a file
// edited into nonsense has to come back as "not the same" and not take the run down.
pub fn parse_language(contents: &str) -> Option<Language> {
    let contents = without_byte_order_mark(contents);
    let mut lines = contents.lines();

    if next_header(&mut lines)? != LANGUAGE {return None;}
    let lang_name = value_line(&mut lines)?;
    // 'value_line' trims whitespace and nothing else, so an escape sequence on the name line came
    // through whole and ended up as a key of the result, which the command line then prints. A file
    // carrying one is damaged rather than unusual, and this is the only value that is displayed.
    if lang_name.is_empty() || lang_name.chars().any(char::is_control) {return None;}

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

    let mut multiline_comments = None;
    let mut header = next_header(&mut lines);
    if header.as_deref() == Some(MULTILINE_COMMENT_START) {
        let start = value_line(&mut lines)?;
        if start.is_empty() || next_header(&mut lines)?.as_str() != MULTILINE_COMMENT_END {return None;}
        let end = value_line(&mut lines)?;
        if end.is_empty() {return None;}
        multiline_comments = Some((start, end));
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

    // Anything still standing here is a header this parser has no block for, and the lines under it
    // were never read. Keeping the half it recognised is what made a typo silently change a count.
    if header.is_some() {return None;}

    Some(Language::new(lang_name, extensions, string_symbols, comment_symbols,
            multiline_comments.as_ref().map(|(start, end)| (start.as_str(), end.as_str())), keywords))
}

// A missing file is not a mistake: an installation made by an earlier version has none, and the
// only consequence is that contested extensions fall back to the alphabetical tiebreak, which
// announces itself anyway.
pub fn parse_priority_file(path: impl AsRef<Path>) -> (HashMap<String,Vec<String>>, Vec<String>) {
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

    for line in without_byte_order_mark(contents).lines() {
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
        // Keyed the way a language's own declaration is keyed, dot and case alike, or a rule written
        // '.m' would settle nothing and never say why
        match rules.entry(crate::engine::extensions::extension_key(extension)) {
            std::collections::hash_map::Entry::Occupied(_) => faulty_lines.push(line.to_owned()),
            std::collections::hash_map::Entry::Vacant(slot) => { slot.insert(names); }
        }
    }

    (rules, faulty_lines)
}

// A byte order mark is three bytes that mean "this is UTF-8" and carry no text, and 'trim' does not
// remove them because they are not whitespace. PowerShell's 'Set-Content' and older Notepad both
// write one, so it arrives through the ordinary way of editing one of these files on Windows.
//
// Every parser of a text format in this crate calls it, because leaving it to each one is how it
// goes wrong: a mark in front of the '===>' of the priority file stops that line matching, and every
// rule in the file is then skipped without so much as a faulty line to report it, a line never read
// being a line never rejected.
fn without_byte_order_mark(contents: &str) -> &str {
    contents.trim_start_matches('\u{feff}')
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
        let named = |wanted: &str| languages.iter().find(|x| x.name == wanted)
                .unwrap_or_else(|| panic!("{wanted} is shipped"));

        // The bug this fixed: the keywords sit below a blank line, and every one of them was dropped.
        let rust = named("Rust");
        assert!(rust.keywords.iter().any(|k| k.descriptive_name == "structs"),
                "Rust lost its keywords: {:?}", rust.keywords);

        // A language whose only comments are multiline has an empty 'Comment symbols' value, and
        // that empty line is the value and not a separator to skip, or the symbols one line down
        // would read as it.
        let css = named("CSS");
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
                DATA_DIR.to_owned() + crate::EXTENSION_PRIORITY_FILE_NAME);
        assert!(faulty.is_empty(), "the shipped priority file has lines that do not parse: {faulty:?}");

        let (_, report) = crate::engine::extensions::make_extension_language_map(
                &crate::languages::keyed_by_name(languages), &priority, &HashMap::new());
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
    // The parser used to keep whatever it recognised and drop the rest, so a header it did not know
    // ended the definition early and in silence. Every case below is a plausible typo in a file
    // somebody edited by hand, and every one of them used to be ACCEPTED as a language with no
    // multiline comments and no keywords, which counts a block comment as code.
    #[test]
    fn a_header_the_parser_does_not_know_refuses_the_file_instead_of_truncating_it() {
        // A shipped file and not a copy of the format written here, so that the shape being mutated
        // below is whatever the program actually reads today and cannot drift away from it.
        let good = std::fs::read_to_string(LANGUAGES_DIR.to_owned() + "Rust.txt").unwrap();
        let parsed = crate::language_file::parse_language(&good).expect("the control file must parse");
        assert!(parsed.multiline_comment_start_symbol.is_some() && !parsed.keywords.is_empty(),
                "Rust.txt no longer declares the two blocks this test truncates, so pick another file");

        // Line by line and never by replacing a run of text, because how many blank lines sit
        // between two blocks is the file's business and not this test's: written the other way, the
        // last case below silently matched nothing and the test passed by testing the good file
        // four times. Each mutation is asserted to have changed something for the same reason.
        let lines = good.lines().collect::<Vec<_>>();
        let line_of = |header: &str| lines.iter().position(|x| x.trim() == header)
                .unwrap_or_else(|| panic!("the control file has no '{header}' line"));
        let with_line_replaced = |at: usize, text: &str| {
            let mut kept = lines.clone();
            kept[at] = text;
            kept.join("\n")
        };

        let start = line_of(MULTILINE_COMMENT_START);
        let mut without_the_start_block = lines.clone();
        // the header and the symbol under it, so the end is left with nothing to close
        without_the_start_block.drain(start..start + 2);
        let mut with_a_heading_over_the_keywords = lines.clone();
        with_a_heading_over_the_keywords.insert(line_of(KEYWORD), "Keywords");

        let truncating = [
            // one word of the header run together, which is the mistake that found this
            ("a header run together", with_line_replaced(start, "Multiline comment start")),
            ("a header that means nothing", with_line_replaced(start, "Totally Bogus")),
            ("a heading added over the keyword blocks", with_a_heading_over_the_keywords.join("\n")),
            ("a closing symbol with nothing to close", without_the_start_block.join("\n"))
        ];
        for (name, contents) in truncating {
            assert_ne!(good.trim(), contents.trim(), "the '{name}' mutation changed nothing");
            assert!(crate::language_file::parse_language(&contents).is_none(),
                    "'{name}' was accepted, and everything under it dropped:\n{contents}");
        }
    }
    // Why a file could not become a language is two different answers and the caller is meant to be
    // able to tell them apart, so both have to be reachable. Collapsing them into one passed every
    // test there was, which is what a distinction with nothing asserting it is worth.
    #[test]
    fn a_path_that_is_not_there_and_a_file_that_does_not_parse_are_different_answers() {
        let missing = crate::language_file::parse_language_file(
                LANGUAGES_DIR.to_owned() + "no-such-language-file.txt").unwrap_err();
        assert!(matches!(missing, LanguageFileError::Unreadable(_)), "got: {missing:?}");
        // the io error travels with it, so a caller can say why rather than only that
        assert!(std::error::Error::source(&missing).is_some(), "the reason was dropped");
        assert!(missing.to_string().contains("could not be read"), "{missing}");

        let garbage = std::env::temp_dir().join("mezura-not-a-language-file.txt");
        std::fs::write(&garbage, "this is not a language file at all\n").unwrap();
        let malformed = crate::language_file::parse_language_file(&garbage).unwrap_err();
        std::fs::remove_file(&garbage).unwrap();
        assert!(matches!(malformed, LanguageFileError::Malformed), "got: {malformed:?}");
        assert!(std::error::Error::source(&malformed).is_none());

        // and a real one still comes back as a language
        assert!(crate::language_file::parse_language_file(LANGUAGES_DIR.to_owned() + "Rust.txt").is_ok());
    }

    // The distinction above survives the walk of a directory, which is the only place any caller
    // meets it. It used to be thrown away there, in an 'Err(_)' that pushed a bare name onto one
    // list, so the sole caller announced "Formatting problems detected in language files" over both:
    // a file saved in UTF-16, which is what PowerShell writes with '-Encoding Unicode', sent its
    // owner looking for a typo in a file whose format was never the problem.
    #[test]
    fn the_walk_of_a_directory_keeps_the_reason_each_file_failed() {
        let dir = std::env::temp_dir().join("mezura-faulty-language-dir");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(LANGUAGES_DIR.to_owned() + "Rust.txt", dir.join("Rust.txt")).unwrap();
        std::fs::write(dir.join("Garbage.txt"), "this is not a language file at all\n").unwrap();
        // UTF-16, so 'read_to_string' refuses it before any parsing is attempted
        std::fs::write(dir.join("Utf16.txt"), [0xFFu8, 0xFE, 0x4C, 0x00, 0x61, 0x00]).unwrap();

        let (languages, mut faulty) = crate::language_file::parse_languages_in_dir(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        faulty.sort_by(|a, b| a.file_name.cmp(&b.file_name));

        assert_eq!(1, languages.len());
        assert_eq!(vec!["Garbage.txt", "Utf16.txt"],
                faulty.iter().map(|x| x.file_name.as_str()).collect::<Vec<_>>());
        assert!(matches!(faulty[0].error, LanguageFileError::Malformed), "got: {:?}", faulty[0].error);
        assert!(matches!(faulty[1].error, LanguageFileError::Unreadable(_)), "got: {:?}", faulty[1].error);
    }

    // A byte order mark is what PowerShell's 'Set-Content' and older Notepad put at the front of a
    // file they save, so this is the ordinary way of editing a language file on Windows, and it
    // used to make the file unreadable and the language vanish from the count.
    #[test]
    fn a_language_file_saved_with_a_byte_order_mark_still_reads() {
        let good = std::fs::read_to_string(LANGUAGES_DIR.to_owned() + "Rust.txt").unwrap();
        // the mark is not whitespace, so nothing on the way in was ever going to remove it
        assert!(!'\u{feff}'.is_whitespace());

        let with_mark = "\u{feff}".to_owned() + &good;
        assert_eq!(crate::language_file::parse_language(&good), crate::language_file::parse_language(&with_mark),
                "the same definition read differently depending on how the editor saved it");
        assert!(crate::language_file::parse_language(&with_mark).is_some());
    }

    // The same editor saves this file too, and here the failure was silent rather than loud: the
    // mark stopped the '===>' of the first line from matching, so the block was never entered, every
    // rule in it was skipped, and nothing landed among the faulty lines either, since a line that is
    // never read is never rejected. The only trace was the tiebreak warnings the file exists to
    // silence. It survived unnoticed because the shipped copy opens with prose, and the mark sits on
    // a line that is skipped anyway; a user who trims that prose away pays for it.
    #[test]
    fn a_priority_file_saved_with_a_byte_order_mark_still_reads() {
        let good = "===> contested-extensions\nm    Objective-C, MATLAB\n";
        let with_mark = "\u{feff}".to_owned() + good;

        let (rules, faulty) = crate::language_file::parse_priority(good);
        let (rules_with_mark, faulty_with_mark) = crate::language_file::parse_priority(&with_mark);

        assert_eq!(rules, rules_with_mark, "the same rules read differently depending on how the editor saved it");
        assert_eq!(faulty, faulty_with_mark);
        assert_eq!(1, rules_with_mark.len(), "the rules of the file were dropped, and in silence");
    }

    #[test]
    fn a_missing_priority_file_is_not_a_mistake() {
        let (rules, faulty) = crate::language_file::parse_priority_file("a/path/that/is/not/there.txt");
        assert!(rules.is_empty() && faulty.is_empty());
    }
    #[test]
    fn test_parse_languages_in_dir() {
        let (languages, faulty_files) = crate::language_file::parse_languages_in_dir(
                FIXTURES_DIR.to_owned() + "definitions/").unwrap();
        assert!(languages.len() == 2);
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

        // Two files declaring one name is a language silently lost, since resolving keys by name and
        // the second declaration wins. The count above used to catch it on its own, back when the
        // parse came back as a map and the two collapsed into one entry.
        let mut names = languages.iter().map(|x| x.name.as_str()).collect::<Vec<_>>();
        names.sort_unstable();
        let duplicates = names.windows(2).filter(|pair| pair[0] == pair[1])
                .map(|pair| pair[0]).collect::<Vec<_>>();
        assert!(duplicates.is_empty(), "these names are declared by more than one shipped file: {duplicates:?}");

        // and each one has to describe something countable. The two halves of a multiline comment
        // are not checked here any more: 'Language::new' takes them as one pair, so a language
        // holding one without the other cannot be built at all, and an assertion that cannot fail
        // reads as cover that is not there.
        for language in &languages {
            let name = &language.name;
            assert!(!language.extensions.is_empty(), "{name} declares no extension");
            assert!(!language.string_symbols.is_empty(), "{name} declares no string symbol");
        }
    }
}
