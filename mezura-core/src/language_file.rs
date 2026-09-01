//! The format of a language file, which is the one thing under this roof that decides a number:
//! what a language is called, which extensions it claims and which symbols open a comment.
//!
//! Reading a definition is this crate's business. Installing, replacing and migrating the files
//! that hold them belongs to whoever owns the directory they live in.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::{Keyword, Language, LeveledPair, LineContinuation, NestedLanguage, StringRules};
use crate::engine::identity::IdentifiedBy;

/// What the language conflicts file decides. It names who wins an extension or a file name that
/// more than one language claims, and it lists the literals that mark a file of an extension as
/// not code at all.
///
/// The contest maps list languages in the order they get the identity, the first one present
/// taking it. The not-code maps list marker literals, not languages. Separate maps because they
/// are separate questions. A rule for the extension `m` says nothing about a file called `m`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ConflictRules {
    /// Keyed by extension, without the dot.
    pub by_extension : HashMap<String, Vec<String>>,
    /// Keyed by whole file name.
    pub by_filename : HashMap<String, Vec<String>>,
    /// Literals that mark a file of this extension as not code when a line begins with one.
    pub not_code_line_starts : HashMap<String, Vec<String>>,
    /// The same, for a literal found anywhere in a line.
    pub not_code_line_contains : HashMap<String, Vec<String>>
}

impl ConflictRules {
    /// The rules under one block of the file.
    pub fn get_of_block(&self, block: ConflictBlock) -> &HashMap<String, Vec<String>> {
        match block {
            ConflictBlock::ContestedExtensions => &self.by_extension,
            ConflictBlock::ContestedFilenames => &self.by_filename,
            ConflictBlock::NotCodeLineStarts => &self.not_code_line_starts,
            ConflictBlock::NotCodeLineContains => &self.not_code_line_contains
        }
    }

    fn get_of_block_mut(&mut self, block: ConflictBlock) -> &mut HashMap<String, Vec<String>> {
        match block {
            ConflictBlock::ContestedExtensions => &mut self.by_extension,
            ConflictBlock::ContestedFilenames => &mut self.by_filename,
            ConflictBlock::NotCodeLineStarts => &mut self.not_code_line_starts,
            ConflictBlock::NotCodeLineContains => &mut self.not_code_line_contains
        }
    }
}

/// One rule block of the language conflicts file, named by the marker that opens it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictBlock {
    /// `===> contested-extensions`, filling [`ConflictRules::by_extension`].
    ContestedExtensions,
    /// `===> contested-filenames`, filling [`ConflictRules::by_filename`].
    ContestedFilenames,
    /// `===> not-code-when-a-line-starts`, filling [`ConflictRules::not_code_line_starts`].
    NotCodeLineStarts,
    /// `===> not-code-when-a-line-contains`, filling [`ConflictRules::not_code_line_contains`].
    NotCodeLineContains
}

impl ConflictBlock {
    /// Every block, in the order the shipped file writes them.
    pub const ALL: [ConflictBlock; 4] = [ConflictBlock::ContestedExtensions,
            ConflictBlock::ContestedFilenames, ConflictBlock::NotCodeLineStarts,
            ConflictBlock::NotCodeLineContains];

    /// The key under which this block files a rule's first word, folded the way the parser folds it.
    pub fn key_of(self, claimed: &str) -> String {
        match self {
            ConflictBlock::ContestedExtensions | ConflictBlock::NotCodeLineStarts
                    | ConflictBlock::NotCodeLineContains => IdentifiedBy::Extension.key_of(claimed),
            ConflictBlock::ContestedFilenames => IdentifiedBy::Filename.key_of(claimed)
        }
    }

    /// The marker that opens the block in the file, `contested-extensions` and the other three.
    pub fn name(self) -> &'static str {
        match self {
            ConflictBlock::ContestedExtensions => CONTESTED_EXTENSIONS,
            ConflictBlock::ContestedFilenames => CONTESTED_FILENAMES,
            ConflictBlock::NotCodeLineStarts => NOT_CODE_LINE_STARTS,
            ConflictBlock::NotCodeLineContains => NOT_CODE_LINE_CONTAINS
        }
    }
}

// The headers a language file is written with, spelled as they appear in it
const LANGUAGE                 : &str = "Language";
const EXTENSIONS               : &str = "Extensions";
const FILENAMES                : &str = "Filenames";
const SHEBANGS                 : &str = "Shebangs";
const IDENTIFYING_LINE_STARTS   : &str = "Identifying line starts";
const IDENTIFYING_LINE_CONTAINS : &str = "Identifying line contains";
const STRING_SYMBOLS           : &str = "String symbols";
const MULTILINE_STRINGS        : &str = "Multi line string symbols";
const MULTILINE_RAW_STRINGS    : &str = "Multi line raw string symbols";
const PAIRED_STRING_OPENERS    : &str = "Paired string openers";
const PAIRED_STRING_CLOSERS    : &str = "Paired string closers";
const CHARACTER_LITERALS       : &str = "Character literal symbols";
const ESCAPE_CHARACTER         : &str = "Escape character";
const ESCAPES_NOTHING          : &str = "none";
const LINE_CONTINUATION        : &str = "Line continuation";
const CONTINUES                : &str = "Continues";
const CONTINUES_STRINGS        : &str = "strings";
const CONTINUES_COMMENTS       : &str = "comments";
const COMMENT_SYMBOLS          : &str = "Comment symbols";
const MULTILINE_COMMENT_START  : &str = "Multi line comment start";
const MULTILINE_COMMENT_END    : &str = "Multi line comment end";
const SELF_NESTING_COMMENT_START : &str = "Self-nesting comment start";
const SELF_NESTING_COMMENT_END   : &str = "Self-nesting comment end";
const NESTED_LANGUAGE_START    : &str = "Nested language start";
const NESTED_LANGUAGE_END      : &str = "Nested language end";
const NESTED_LANGUAGE_DEFAULT  : &str = "Nested language default";
const KEYWORD                  : &str = "Keyword";
const KEYWORD_NAME             : &str = "NAME";
const KEYWORD_ALIASES          : &str = "ALIASES";

// The markers that open the rule blocks of the language conflicts file
const CONTESTED_EXTENSIONS     : &str = "contested-extensions";
const CONTESTED_FILENAMES      : &str = "contested-filenames";
const NOT_CODE_LINE_STARTS     : &str = "not-code-when-a-line-starts";
const NOT_CODE_LINE_CONTAINS   : &str = "not-code-when-a-line-contains";

// No command is named here: this crate does not know the command line's. Whoever prints it adds the
// way out, the way 'warning_collector' does for the language warnings. And nothing here promises a
// repair on the next run: the command line's migration pass runs before this parse, so all three of
// the errors below are only ever reached after it has already run and not fixed them.
const REGENERATE_LANGUAGES_HINT : &str =
        "The copies this build ships can be written over them.";

/// Why a whole directory of language files gave nothing usable.
#[derive(Debug)]
#[non_exhaustive]
pub enum LanguageDirParseError {
    /// The directory opened and holds no files.
    NoFilesFound,
    /// It holds files and not one of them parses.
    NoFilesFormattedProperly,
    /// The directory itself could not be opened, quoted here.
    PathMissing(String)
}

impl std::fmt::Display for LanguageDirParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoFilesFound => write!(f, "No language files found in directory.
{REGENERATE_LANGUAGES_HINT}"),
            Self::NoFilesFormattedProperly => write!(f, "No language file is formatted properly, so none could be parsed.
{REGENERATE_LANGUAGES_HINT}"),
            Self::PathMissing(path) => write!(f, "The languages directory ({path}) is not there.
{REGENERATE_LANGUAGES_HINT}")
        }
    }
}

impl std::error::Error for LanguageDirParseError {}

/// Why one language file gave nothing.
///
/// Two different mistakes, kept apart: a path that is not there is the caller's, text that does not
/// parse is the file's. Collapsed into one answer, somebody hunts for a formatting problem in a
/// file they misspelled the name of.
#[derive(Debug)]
#[non_exhaustive]
pub enum LanguageFileError {
    /// The file could not be opened or read.
    Unreadable(std::io::Error),
    /// The line the reading stopped at, counted from 1.
    Malformed(usize)
}

impl std::fmt::Display for LanguageFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable(error) => write!(f, "the language file could not be read: {error}"),
            Self::Malformed(line) => write!(f, "line {line} is not what the format expects there")
        }
    }
}

impl std::error::Error for LanguageFileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unreadable(error) => Some(error),
            Self::Malformed(_) => None
        }
    }
}

/// One language file that gave nothing, which is one whole language missing from the count.
#[derive(Debug)]
pub struct FaultyLanguageFile {
    /// As it is spelled on disk, so that somebody can go and open it.
    pub file_name: String,
    /// What went wrong with it.
    pub error: LanguageFileError
}

/// Reads every language file in a directory, and the ones that failed beside them.
// A list and not a map keyed by name, since the name lives in the language itself and a second copy
// of it is a second thing to disagree.
pub fn parse_languages_in_dir(target_path: impl AsRef<Path>)
-> Result<(Vec<Language>, Vec<FaultyLanguageFile>), LanguageDirParseError> {
    let target_path = target_path.as_ref();
    let Ok(entries) = fs::read_dir(target_path) else {
        return Err(LanguageDirParseError::PathMissing(target_path.display().to_string()));
    };

    let mut languages = Vec::with_capacity(30);
    let mut faulty_files : Vec<FaultyLanguageFile> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {continue;}

        match parse_language_file(&path) {
            Ok(language) => languages.push(language),
            // The name as it is spelled on disk: the point of the list is that somebody can go and
            // open the file it names, and lowercasing it names a file that does not exist wherever
            // the filesystem has a case.
            Err(error) => if let Ok(file_name) = entry.file_name().into_string()
                    && !file_name.is_empty() {
                faulty_files.push(FaultyLanguageFile { file_name, error });
            }
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

/// Reads one language file, naming the line it stopped at if it does not parse.
pub fn parse_language_file(path: impl AsRef<Path>) -> Result<Language, LanguageFileError> {
    let contents = fs::read_to_string(path).map_err(LanguageFileError::Unreadable)?;
    parse_language_or_faulty_line(&contents).map_err(LanguageFileError::Malformed)
}

/// Reads the text of a language file. A file on disk and the bytes baked into this crate go through
/// this same parser, so there is no second one to drift from it.
///
/// A value sits on the line straight after its header and is taken as it is even when empty, since
/// a language with only block comments has an empty `Comment symbols` value and that empty line is
/// the value, not a separator. Blank lines are skipped only before a header. And every line has to
/// be accounted for: a header left over at the end is one this did not understand and the file is
/// refused whole, or `Multiline comment start` written for `Multi line comment start` passes as a
/// language with no block comments and no keywords, which counts its comments as code.
///
/// `None` rather than a panic on anything unrecognised, so that a file edited into nonsense can be
/// answered for rather than taking the run down.
pub fn parse_language(contents: &str) -> Option<Language> {
    parse_language_or_faulty_line(contents).ok()
}

// The same reading, with the line the parser stopped at when it refuses. The blocks are read in one
// fixed order and an optional one that arrives late is refused whole, so "the format is wrong" on
// its own leaves somebody comparing their file against the documentation.
pub(crate) fn parse_language_or_faulty_line(contents: &str) -> Result<Language, usize> {
    let contents = strip_byte_order_mark(contents);
    let mut reader = LineReader::of(contents);
    match read_language(&mut reader) {
        Some(language) => Ok(language),
        None => Err(reader.read.max(1))
    }
}

fn read_language(lines: &mut LineReader) -> Option<Language> {
    if read_next_header(lines)? != LANGUAGE {return None;}
    let lang_name = read_value_line(lines)?;
    // 'read_value_line' trims whitespace and nothing else, so a control character on the name line
    // reaches the map key the command line prints. This is the only value that is displayed, and a
    // file carrying one is damaged rather than unusual.
    if lang_name.is_empty() || lang_name.chars().any(char::is_control) {return None;}

    if read_next_header(lines)? != EXTENSIONS {return None;}
    let extensions = split_line_on_whitespace(&read_value_line(lines)?);

    // Whole names, for the files an extension cannot describe. Optional, and the value may be
    // empty for a language that is nothing but names, which is what Make and Dockerfile are.
    let mut filenames = Vec::new();
    let mut next = read_next_header(lines)?;
    if next == FILENAMES {
        filenames = split_line_on_whitespace(&read_value_line(lines)?);
        next = read_next_header(lines)?;
    }
    // Interpreter names as a '#!' line spells them, for the scripts whose name says nothing.
    // Optional; declaring the block with nothing under it is a mistake and refuses the file.
    let mut shebangs = Vec::new();
    if next == SHEBANGS {
        shebangs = split_line_on_whitespace(&read_value_line(lines)?);
        if shebangs.is_empty() {return None;}
        next = read_next_header(lines)?;
    }
    if extensions.is_empty() && filenames.is_empty() && shebangs.is_empty() {return None;}

    let mut identifying_line_starts = Vec::new();
    if next == IDENTIFYING_LINE_STARTS {
        identifying_line_starts = split_line_on_commas(&read_value_line(lines)?);
        if identifying_line_starts.is_empty() {return None;}
        next = read_next_header(lines)?;
    }
    let mut identifying_line_contains = Vec::new();
    if next == IDENTIFYING_LINE_CONTAINS {
        identifying_line_contains = split_line_on_commas(&read_value_line(lines)?);
        if identifying_line_contains.is_empty() {return None;}
        next = read_next_header(lines)?;
    }

    if next != STRING_SYMBOLS {return None;}
    let string_symbols = split_line_on_whitespace(&read_value_line(lines)?);

    // The symbol of a character literal, which exists only paired on its own line: a lone one is
    // not a literal at all. Its own block, because declaring Rust's ' as a string would be a lie
    // the format cannot explain.
    let mut char_literals = Vec::new();
    let mut header = read_next_header(lines)?;
    if header == CHARACTER_LITERALS {
        char_literals = split_line_on_whitespace(&read_value_line(lines)?);
        if char_literals.is_empty() {return None;}
        header = read_next_header(lines)?;
    }

    // A symbol belongs to exactly one of the lists, the way a comment symbol does: a string that
    // ends with its line goes above, one that crosses lines goes here. Declaring the same symbol
    // twice would leave the two answers to argue, so it refuses the file.
    let mut multiline_strings = Vec::new();
    if header == MULTILINE_STRINGS {
        multiline_strings = split_line_on_whitespace(&read_value_line(lines)?);
        if multiline_strings.is_empty() {return None;}
        header = read_next_header(lines)?;
    }

    // The same, for a form where a backslash in front of the closer is an ordinary byte: Go's and
    // Odin's backtick, Kotlin's and C#'s '"""'. Its own block because nothing about a symbol says
    // which of the two it is, and the block above holds languages that write it identically.
    let mut raw_multiline_strings = Vec::new();
    if header == MULTILINE_RAW_STRINGS {
        raw_multiline_strings = split_line_on_whitespace(&read_value_line(lines)?);
        if raw_multiline_strings.is_empty() {return None;}
        header = read_next_header(lines)?;
    }

    // Strings that open with one symbol and close with another, 'r#"' with '"#'. The two value
    // lines are lists paired by position, the shape the multiline comment block also has. Nothing
    // escapes inside one, which is the reason such a form has a distinct opener at all.
    let mut string_pairs = Vec::new();
    if header == PAIRED_STRING_OPENERS {
        let openers = split_line_on_whitespace(&read_value_line(lines)?);
        if openers.is_empty() || read_next_header(lines)?.as_str() != PAIRED_STRING_CLOSERS {return None;}
        let closers = split_line_on_whitespace(&read_value_line(lines)?);
        if closers.len() != openers.len() {return None;}
        string_pairs = openers.into_iter().zip(closers).collect::<Vec<_>>();
        header = read_next_header(lines)?;
    }

    // What cancels a string symbol, which is a fact about the language and not about the symbol.
    // Required of any language declaring a string of any kind, so that a file states it instead of
    // inheriting whatever the parser last happened to do; 'none' is how a language says nothing
    // escapes.
    let mut escape_character = None;
    let declares_a_string = !string_symbols.is_empty() || !char_literals.is_empty()
            || !multiline_strings.is_empty() || !raw_multiline_strings.is_empty()
            || !string_pairs.is_empty();
    if header == ESCAPE_CHARACTER {
        let value = read_value_line(lines)?;
        if value != ESCAPES_NOTHING {
            // One byte, because the test walks backwards from the symbol counting how many of these
            // stand in front of it, and every language that has one spells it in ASCII
            let [byte] = value.as_bytes() else { return None };
            if !byte.is_ascii() { return None; }
            escape_character = Some(*byte);
        }
        header = read_next_header(lines)?;
    } else if declares_a_string {
        return None;
    }

    // The symbol that joins a line to the next, and what the joining reaches. Read before the
    // comment symbols because it is a property of the line and not of either kind of delimiter.
    let mut line_continuation = None;
    if header == LINE_CONTINUATION {
        let symbol = read_value_line(lines)?;
        if symbol.is_empty() || read_next_header(lines)?.as_str() != CONTINUES {return None;}
        let reaches = split_line_on_whitespace(&read_value_line(lines)?);
        let (in_strings, in_comments) = (reaches.iter().any(|x| x == CONTINUES_STRINGS),
                reaches.iter().any(|x| x == CONTINUES_COMMENTS));
        // A named thing it reaches nothing of is a mistake somebody made, not a declaration
        if reaches.len() != usize::from(in_strings) + usize::from(in_comments) || reaches.is_empty() {
            return None;
        }
        line_continuation = Some(LineContinuation { symbol, in_strings, in_comments });
        header = read_next_header(lines)?;
    }

    // Declaring no string at all is allowed and is what HTML needs: its quotes delimit attributes
    // rather than strings, and the free text between its tags is full of apostrophes.
    let crossing_openers = multiline_strings.iter().chain(raw_multiline_strings.iter())
            .chain(string_pairs.iter().map(|(open, _)| open)).collect::<Vec<&String>>();
    let opens_a_crossing_string = |symbol: &String| crossing_openers.contains(&symbol);
    if string_symbols.iter().any(opens_a_crossing_string) {return None;}
    if char_literals.iter().any(|literal| string_symbols.contains(literal)
            || opens_a_crossing_string(literal)) {return None;}
    // One symbol in two of the three blocks would leave them to argue about whether it escapes
    if crossing_openers.iter().enumerate()
            .any(|(at, open)| crossing_openers[at + 1..].contains(open)) {return None;}

    if header != COMMENT_SYMBOLS {return None;}
    // Deliberately allowed to be empty: a language whose only comments are multiline has no line
    // comment symbol.
    let comment_symbols = split_line_on_whitespace(&read_value_line(lines)?);

    // The two value lines are lists paired by position: the first start closes with the first end.
    // Unequal counts leave some symbol with no other half, and the file is refused rather than
    // guessed at.
    let mut multiline_comments = Vec::new();
    let mut header = read_next_header(lines);
    if header.as_deref() == Some(MULTILINE_COMMENT_START) {
        let starts = split_line_on_whitespace(&read_value_line(lines)?);
        if starts.is_empty() || read_next_header(lines)?.as_str() != MULTILINE_COMMENT_END {return None;}
        let ends = split_line_on_whitespace(&read_value_line(lines)?);
        if ends.len() != starts.len() {return None;}
        multiline_comments = starts.into_iter().zip(ends).collect();
        header = read_next_header(lines);
    }

    // The pairs that nest inside themselves, so a closer only ends the block once it has closed
    // as many as were opened. Same zip shape; a pair belongs to one block or the other, since
    // whether '/* /* */' is still open is exactly what the two declarations disagree about.
    let mut nesting_comments : Vec<(String, String)> = Vec::new();
    if header.as_deref() == Some(SELF_NESTING_COMMENT_START) {
        let starts = split_line_on_whitespace(&read_value_line(lines)?);
        if starts.is_empty() || read_next_header(lines)?.as_str() != SELF_NESTING_COMMENT_END {return None;}
        let ends = split_line_on_whitespace(&read_value_line(lines)?);
        if ends.len() != starts.len() {return None;}
        nesting_comments = starts.into_iter().zip(ends).collect();
        header = read_next_header(lines);
    }
    if multiline_comments.iter().any(|(start, _)| nesting_comments.iter().any(|(other, _)| start == other)) {
        return None;
    }

    // A pair written with '=*' is Lua's long bracket: the run of '=' is counted at the opener and
    // only an end with the same count closes. It is declared in the multiline block, since the
    // counting is what such a pair has instead of nesting. Half a marker is a typo, refused.
    let mut leveled_comments = Vec::new();
    let mut half_a_marker = false;
    multiline_comments.retain(|(start, end)| {
        if !start.contains("=*") && !end.contains("=*") {
            return true;
        }
        match LeveledPair::of(start, end) {
            Some(pair) => leveled_comments.push(pair),
            None => half_a_marker = true
        }
        false
    });
    if half_a_marker {
        return None;
    }

    // Sections of another language inside a file, HTML's script and style tags. Three lists paired
    // by position: the opener, its closer, and the language the section falls to when the tag
    // names none, written as an extension so it resolves the way a 'lang' attribute does.
    let mut nested_languages = Vec::new();
    if header.as_deref() == Some(NESTED_LANGUAGE_START) {
        let starts = split_line_on_whitespace(&read_value_line(lines)?);
        if starts.is_empty() || read_next_header(lines)?.as_str() != NESTED_LANGUAGE_END {return None;}
        let ends = split_line_on_whitespace(&read_value_line(lines)?);
        if read_next_header(lines)?.as_str() != NESTED_LANGUAGE_DEFAULT {return None;}
        let defaults = split_line_on_whitespace(&read_value_line(lines)?);
        if ends.len() != starts.len() || defaults.len() != starts.len() {return None;}
        // A section is looked for where a tag begins and nowhere else, so an opener that is not one
        // could never match. Refused rather than carried as a declaration that does nothing.
        if starts.iter().chain(&ends).any(|symbol| !symbol.starts_with('<')) {return None;}
        nested_languages = starts.iter().zip(&ends).zip(&defaults)
                .map(|((start, end), default)| NestedLanguage::of(start, end, default))
                .collect();
        header = read_next_header(lines);
    }

    let mut keywords = Vec::new();
    while header.as_deref() == Some(KEYWORD) {
        if read_next_header(lines)?.as_str() != KEYWORD_NAME {return None;}
        let name = read_value_line(lines)?;
        if read_next_header(lines)?.as_str() != KEYWORD_ALIASES {return None;}
        let aliases = split_line_on_whitespace(&read_value_line(lines)?);
        if name.is_empty() || aliases.is_empty() {return None;}

        keywords.push(Keyword{descriptive_name: name, aliases});
        header = read_next_header(lines);
    }

    // Anything still standing here is a header this parser has no block for, and the lines under it
    // were never read.
    if header.is_some() {return None;}

    // In the order the file declares them, which is the order the scan numbers them in
    let strings = match escape_character {
        Some(byte) => StringRules::escaping_with(byte),
        None => StringRules::escaping_nothing()
    }
            .with_symbols(string_symbols)
            .with_char_literals(char_literals)
            .with_multiline_strings(multiline_strings)
            .with_raw_multiline_strings(raw_multiline_strings)
            .with_string_pairs(&string_pairs);

    let mut language = Language::new(lang_name, extensions, strings, comment_symbols,
            &multiline_comments.iter().map(|(start, end): &(String, String)| (start.as_str(), end.as_str()))
                    .collect::<Vec<_>>(), keywords);
    language.line_continuation = line_continuation;

    Some(language
            .with_nesting_comments(&nesting_comments)
            .with_leveled_comments(&leveled_comments)
            .with_nested_languages(&nested_languages)
            .with_filenames(&filenames)
            .with_shebangs(&shebangs)
            .with_identification(&identifying_line_starts, &identifying_line_contains))
}

/// Reads the file that settles contested extensions, and the lines of it that did not parse.
///
/// A missing file is not a mistake and comes back as no rules at all: the only consequence is that
/// a contested extension falls back to the alphabetical tiebreak, which announces itself anyway.
pub fn parse_conflict_rules_file(path: impl AsRef<Path>) -> (ConflictRules, Vec<(ConflictBlock, String)>) {
    match fs::read_to_string(path) {
        Ok(contents) => parse_conflict_rules(&contents),
        Err(_) => (ConflictRules::default(), Vec::new())
    }
}

/// The same reading, from text, giving back the rules and the lines that did not parse, each with
/// the block it sat under.
///
/// A line that does not parse is reported and skipped while the rest of the file applies. Under a
/// contest block the extension it failed to settle falls through to the tiebreak, which says so by
/// name, and under a not-code block the files of its extension simply keep being counted.
pub fn parse_conflict_rules(contents: &str) -> (ConflictRules, Vec<(ConflictBlock, String)>) {
    let mut rules = ConflictRules::default();
    let mut faulty_lines = Vec::new();
    let mut block = None;

    for line in strip_byte_order_mark(contents).lines() {
        let line = line.trim();
        if line.is_empty() {continue;}
        // The '===>' of the configuration files, since this one explains itself above its rules the
        // way a configuration does. A marker also ends the block, so a section added later is
        // skipped rather than read as a rule for an extension named '===>', which would be neither
        // applied nor reported.
        if line.starts_with("===>") {
            block = find_block_of_marker(line);
            continue;
        }
        let Some(block) = block else { continue };

        let Some((claimed, names)) = split_rule_line(line) else {
            faulty_lines.push((block, line.to_owned()));
            continue;
        };

        // The first declaration is the one that counts, so that a second one cannot silently undo a
        // decision sitting a few lines above it in the same file. Keyed the way a language's own
        // declaration is keyed, dot and case alike, or a rule written '.m' would settle nothing and
        // never say why.
        match rules.get_of_block_mut(block).entry(block.key_of(claimed)) {
            std::collections::hash_map::Entry::Occupied(_) => faulty_lines.push((block, line.to_owned())),
            std::collections::hash_map::Entry::Vacant(slot) => { slot.insert(names); }
        }
    }

    (rules, faulty_lines)
}

/// Which block a `===>` marker line opens, read exactly the way [`parse_conflict_rules`] reads it,
/// or None for a line that is no marker or a marker this build does not know.
pub fn find_block_of_marker(line: &str) -> Option<ConflictBlock> {
    let line = line.trim();
    if !line.starts_with("===>") {
        return None;
    }
    let name = line.trim_start_matches("===>").split_whitespace().next().unwrap_or_default();
    ConflictBlock::ALL.into_iter().find(|block| name.eq_ignore_ascii_case(block.name()))
}

/// The key under which [`parse_conflict_rules`] would file this rule line of the given block, or
/// None for a line it would reject.
pub fn find_key_of_rule(block: ConflictBlock, line: &str) -> Option<String> {
    split_rule_line(line.trim()).map(|(claimed, _)| block.key_of(claimed))
}

// A byte order mark is three bytes that mean "this is UTF-8" and carry no text, and 'trim' does not
// remove them because they are not whitespace. PowerShell's 'Set-Content' and older Notepad both
// write one, so it arrives through the ordinary way of editing one of these files on Windows.
//
// Every parser of a text format in this crate calls it: a mark in front of the '===>' of the
// conflicts file stops that line matching, and every rule in the file is then skipped without so
// much as a faulty line to report it, a line never read being a line never rejected.
fn strip_byte_order_mark(contents: &str) -> &str {
    contents.trim_start_matches('\u{feff}')
}

// The next line that carries a header, with the blank lines between blocks skipped. Trimmed, so an
// indented sub-header like the 'NAME' of a keyword block is recognised.
fn read_next_header(lines: &mut LineReader) -> Option<String> {
    lines.by_ref().map(str::trim).find(|line| !line.is_empty()).map(str::to_owned)
}

// The value that belongs to the header just read: the very next line, trimmed, empty or not. Never
// skips a blank, because for the comment symbols the blank line is the value.
fn read_value_line(lines: &mut LineReader) -> Option<String> {
    lines.next().map(|line| line.trim().to_owned())
}

// Counts what it hands out, so a file that is refused can name the line the parser stopped at:
// the blocks have to arrive in one order, and "somewhere in this file" does not say which one is
// out of place.
struct LineReader<'a> {
    lines: std::str::Lines<'a>,
    read: usize
}

impl<'a> LineReader<'a> {
    fn of(contents: &'a str) -> Self {
        LineReader { lines: contents.lines(), read: 0 }
    }
}

impl<'a> Iterator for LineReader<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        let line = self.lines.next();
        self.read += usize::from(line.is_some());
        line
    }
}

fn split_line_on_whitespace(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::trim).filter(|x| !x.is_empty()).map(str::to_owned).collect()
}

fn split_line_on_commas(line: &str) -> Vec<String> {
    line.split(',').map(str::trim).filter(|x| !x.is_empty()).map(str::to_owned).collect()
}

fn split_rule_line(line: &str) -> Option<(&str, Vec<String>)> {
    let (claimed, values) = line.split_once(char::is_whitespace)?;
    let values = split_line_on_commas(values);
    if values.is_empty() {None} else {Some((claimed, values))}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MultilineString;
    use crate::test_paths::{DATA_DIR, FIXTURES_DIR, LANGUAGES_DIR};

    // The real shipped files and not a hand-written string that copies the format, which would be a
    // second source of truth that rots.
    #[test]
    fn the_parser_reads_the_shipped_files_and_a_blank_line_never_costs_a_block() {
        let (languages, _) = parse_languages_in_dir(LANGUAGES_DIR).unwrap();
        let named = |wanted: &str| languages.iter().find(|x| x.name == wanted)
                .unwrap_or_else(|| panic!("{wanted} is shipped"));

        // the keywords sit below a blank line
        let rust = named("Rust");
        assert!(rust.keywords.iter().any(|k| k.descriptive_name == "structs"),
                "Rust lost its keywords: {:?}", rust.keywords);

        // A language whose only comments are multiline has an empty 'Comment symbols' value, and
        // that empty line is the value and not a separator to skip.
        let css = named("CSS");
        assert!(css.comment_symbols.is_empty() && !css.multiline_comments.is_empty(),
                "CSS was the empty-comment-symbols case and its shape changed");

        // The one thing no shipped file can show, since a stray blank line in one would be tidied
        // away: an extra blank line between blocks does not derail the parse.
        let padded = "Language\nJava\n\n\nExtensions\njava\n\n\n\nString symbols\n\"\n\n\
Escape character\n\\\n\n\
Comment symbols\n//\n\n\nKeyword\n    NAME\n    classes\n    ALIASES\n    class\n";
        let java = parse_language(padded).expect("an extra blank line broke the parse");
        assert_eq!(vec!["classes"], java.keywords.iter().map(|k| k.descriptive_name.clone()).collect::<Vec<_>>());
    }
    #[test]
    fn every_contest_between_the_shipped_languages_is_settled_by_the_shipped_conflicts_file() {
        let (languages, _) = parse_languages_in_dir(LANGUAGES_DIR).unwrap();
        let (conflicts, faulty) = parse_conflict_rules_file(
                DATA_DIR.to_owned() + crate::LANGUAGE_CONFLICTS_FILE_NAME);
        assert!(faulty.is_empty(), "the shipped conflicts file has lines that do not parse: {faulty:?}");

        // Both maps, since a filename two languages both claim is settled in the same file and
        // would otherwise be the one contest nothing here notices
        let by_name = crate::languages::keyed_by_name(languages);
        let mut unsettled = Vec::new();
        for (identified_by, rules) in [(IdentifiedBy::Extension, &conflicts.by_extension),
                (IdentifiedBy::Filename, &conflicts.by_filename)] {
            let (_, report) = crate::engine::identity::build_language_map_by(identified_by, &by_name, rules, &HashMap::new());
            unsettled.extend(report.contested.iter()
                    .filter(|x| x.resolved_by == crate::engine::identity::ResolvedBy::AlphabeticalFallback)
                    .map(|x| format!("the {} '{}' between {} and {}", x.identified_by.name(), x.identity, x.winner,
                            x.losers.join(", "))));
        }

        assert!(unsettled.is_empty(),
                "these contests are left to the alphabetical tiebreak, so a clean installation is \
                 warned about them on every run. Declare each one in '{}':\n{}",
                crate::LANGUAGE_CONFLICTS_FILE_NAME, unsettled.join("\n"));
    }

    #[test]
    fn the_conflicts_file_reads_only_what_is_under_its_headers() {
        let (rules, faulty) = parse_conflict_rules(
"Anything up here is explanation, and this looks exactly like a rule:
    m       Objective-C, MATLAB

===> contested-extensions
M        Objective-C , MATLAB
pl       Perl

===> contested-filenames
Makefile   Make, Automake

===> not-code-when-a-line-starts
.PRO     -keep, -dontwarn

===> not-code-when-a-line-contains
d       .o:, .rlib:
");
        assert!(faulty.is_empty());
        assert_eq!(2, rules.by_extension.len());
        assert_eq!(Some(&vec!["Objective-C".to_owned(), "MATLAB".to_owned()]), rules.by_extension.get("m"));
        assert_eq!(Some(&vec!["Perl".to_owned()]), rules.by_extension.get("pl"));

        // a name lands in its own map and is not answered by a rule about extensions
        assert_eq!(Some(&vec!["Make".to_owned(), "Automake".to_owned()]), rules.by_filename.get("makefile"));
        assert!(!rules.by_extension.contains_key("makefile"));

        assert_eq!(Some(&vec!["-keep".to_owned(), "-dontwarn".to_owned()]), rules.not_code_line_starts.get("pro"));
        assert_eq!(Some(&vec![".o:".to_owned(), ".rlib:".to_owned()]), rules.not_code_line_contains.get("d"));
        assert!(!rules.by_extension.contains_key("pro") && !rules.by_extension.contains_key("d"));
    }
    #[test]
    fn a_marker_is_recognised_with_or_without_spacing_and_a_rule_is_keyed_by_its_block() {
        assert_eq!(Some(ConflictBlock::ContestedExtensions), find_block_of_marker("===>contested-extensions"));
        assert_eq!(Some(ConflictBlock::ContestedExtensions), find_block_of_marker("  ===> Contested-Extensions and a note"));
        assert_eq!(Some(ConflictBlock::NotCodeLineContains), find_block_of_marker("===> not-code-when-a-line-contains"));
        assert_eq!(None, find_block_of_marker("===> some-section-added-later"));
        assert_eq!(None, find_block_of_marker("not a marker at all"));

        assert_eq!(Some("pro".to_owned()),
                find_key_of_rule(ConflictBlock::NotCodeLineStarts, ".PRO  -keep, -dontwarn"));
        assert_eq!(Some("makefile.am".to_owned()),
                find_key_of_rule(ConflictBlock::ContestedFilenames, "Makefile.am  Make, Automake"));
        assert_eq!(None, find_key_of_rule(ConflictBlock::ContestedExtensions, "justoneword"));
        assert_eq!(None, find_key_of_rule(ConflictBlock::ContestedExtensions, "v  ,  ,"));
    }

    #[test]
    fn a_line_of_the_conflicts_file_that_does_not_parse_is_skipped_and_the_rest_applies() {
        let (rules, faulty) = parse_conflict_rules(
"===> contested-extensions
m       Objective-C, MATLAB
justoneword
m       Prolog
v       ,  ,
===> some-section-added-later
pl      Perl, Prolog
");
        assert!(!rules.by_extension.contains_key("===>") && !rules.by_extension.contains_key("pl"));
        assert_eq!(1, rules.by_extension.len());
        // the second declaration is the one that loses, and the decision above it stands
        assert_eq!(Some(&vec!["Objective-C".to_owned(), "MATLAB".to_owned()]), rules.by_extension.get("m"));
        assert_eq!(vec![(ConflictBlock::ContestedExtensions, "justoneword".to_owned()),
                (ConflictBlock::ContestedExtensions, "m       Prolog".to_owned()),
                (ConflictBlock::ContestedExtensions, "v       ,  ,".to_owned())], faulty);
    }

    #[test]
    fn a_language_that_does_not_parse_comes_back_as_none() {
        let good = "Language\nLua\n\nExtensions\nlua\n\nString symbols\n\" '\n\n\
Escape character\n\\\n\nComment symbols\n--\n";
        assert!(parse_language(good).is_some());
        // and the carriage returns of a windows checkout change nothing about it
        assert_eq!(parse_language(good),
                parse_language(&good.replace('\n', "\r\n")));

        let broken = [
            String::new(),
            "Language\n".to_owned(),
            good.replace("Extensions", "Extension"),
            // no name and no extensions, each on its own
            good.replace("Lua\n", "\n"),
            good.replace("lua\n\n", "\n\n")
        ];
        for contents in broken {
            assert!(parse_language(&contents).is_none(), "accepted:\n{contents}");
        }

        // Declaring no string symbol is not a mistake, it is what HTML does.
        assert!(parse_language(&good.replace("\" '\n", "\n")).is_some());

        // a stray blank line between blocks does not cost a language its definition
        assert!(parse_language(&good.replace("lua\n", "lua\n\n")).is_some());
    }
    // Every case below is a plausible typo in a file somebody edited by hand, and each one, kept
    // rather than refused, is a language with no multiline comments and no keywords, which counts a
    // block comment as code.
    #[test]
    fn a_header_the_parser_does_not_know_refuses_the_file_instead_of_truncating_it() {
        // A shipped file and not a copy of the format written here, so the shape being mutated
        // below is whatever the program actually reads today.
        let good = std::fs::read_to_string(LANGUAGES_DIR.to_owned() + "Java.txt").unwrap();
        let parsed = parse_language(&good).expect("the control file must parse");
        assert!(!parsed.multiline_comments.is_empty() && !parsed.keywords.is_empty(),
                "Java.txt no longer declares the two blocks this test truncates, so pick another file");

        // Line by line and never by replacing a run of text, because how many blank lines sit
        // between two blocks is the file's business and not this test's: a replacement that matches
        // nothing tests the good file again and says so nowhere. Each mutation is asserted to have
        // changed something for the same reason.
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
            // one word of the header run together
            ("a header run together", with_line_replaced(start, "Multiline comment start")),
            ("a header that means nothing", with_line_replaced(start, "Totally Bogus")),
            ("a heading added over the keyword blocks", with_a_heading_over_the_keywords.join("\n")),
            ("a closing symbol with nothing to close", without_the_start_block.join("\n"))
        ];
        for (name, contents) in truncating {
            assert_ne!(good.trim(), contents.trim(), "the '{name}' mutation changed nothing");
            assert!(parse_language(&contents).is_none(),
                    "'{name}' was accepted, and everything under it dropped:\n{contents}");
        }
    }

    // Declaring the block with nothing under it is a mistake rather than an empty list.
    #[test]
    fn the_shebangs_block_parses_where_it_belongs_and_nowhere_else() {
        let good = std::fs::read_to_string(LANGUAGES_DIR.to_owned() + "Shell.txt").unwrap();
        let parsed = parse_language(&good).expect("the control file must parse");
        assert_eq!(vec!["sh", "bash", "zsh", "ksh", "dash"],
                parsed.shebangs.iter().map(String::as_str).collect::<Vec<_>>());

        let lines = good.lines().collect::<Vec<_>>();
        let at = lines.iter().position(|x| x.trim() == SHEBANGS).unwrap();

        let mut blanked = lines.clone();
        blanked[at + 1] = "";
        assert!(parse_language(&blanked.join("\n")).is_none(),
                "a 'Shebangs' block with nothing under it was accepted");

        // moved below the string symbols, which is the late arrival the fixed order refuses
        let mut moved = lines.clone();
        let block = moved.drain(at..at + 2).collect::<Vec<_>>();
        let after_strings = moved.iter().position(|x| x.trim() == STRING_SYMBOLS).unwrap() + 2;
        for (offset, line) in block.into_iter().enumerate() {
            moved.insert(after_strings + offset, line);
        }
        assert!(parse_language(&moved.join("\n")).is_none(),
                "a 'Shebangs' block after the string symbols was accepted");

        let mut without = lines.clone();
        without.drain(at..at + 2);
        let parsed = parse_language(&without.join("\n"))
                .expect("a file without the optional block must still parse");
        assert!(parsed.shebangs.is_empty());

        // a file whose only claim is its Shebangs block parses: the block is a third way to claim
        // files and not a decoration on the other two
        let extensions_value = lines.iter().position(|x| x.trim() == EXTENSIONS).unwrap() + 1;
        let mut shebang_only = lines.clone();
        shebang_only[extensions_value] = "";
        let parsed = parse_language(&shebang_only.join("\n"))
                .expect("a shebang-only language file was refused");
        assert!(parsed.extensions.is_empty());
        assert_eq!(vec!["sh", "bash", "zsh", "ksh", "dash"],
                parsed.shebangs.iter().map(String::as_str).collect::<Vec<_>>());

        let mut claimless = shebang_only.clone();
        claimless.drain(at..at + 2);
        assert!(parse_language(&claimless.join("\n")).is_none(),
                "a language claiming nothing at all was accepted");
    }
    #[test]
    fn a_path_that_is_not_there_and_a_file_that_does_not_parse_are_different_answers() {
        let missing = parse_language_file(
                LANGUAGES_DIR.to_owned() + "no-such-language-file.txt").unwrap_err();
        assert!(matches!(missing, LanguageFileError::Unreadable(_)), "got: {missing:?}");
        // the io error travels with it, so a caller can say why rather than only that
        assert!(std::error::Error::source(&missing).is_some(), "the reason was dropped");
        assert!(missing.to_string().contains("could not be read"), "{missing}");

        let garbage = std::env::temp_dir().join("mezura-not-a-language-file.txt");
        std::fs::write(&garbage, "this is not a language file at all\n").unwrap();
        let malformed = parse_language_file(&garbage).unwrap_err();
        std::fs::remove_file(&garbage).unwrap();
        // and it names the line it stopped at
        assert!(matches!(malformed, LanguageFileError::Malformed(1)), "got: {malformed:?}");
        assert!(malformed.to_string().contains("line 1"), "{malformed}");
        assert!(std::error::Error::source(&malformed).is_none());

        assert!(parse_language_file(LANGUAGES_DIR.to_owned() + "Rust.txt").is_ok());
    }

    // The distinction above survives the walk of a directory, which is the only place any caller
    // meets it. Announcing "formatting problems" over both sends the owner of a file saved in
    // UTF-16, which is what PowerShell writes with '-Encoding Unicode', looking for a typo.
    #[test]
    fn the_walk_of_a_directory_keeps_the_reason_each_file_failed() {
        let dir = std::env::temp_dir().join("mezura-faulty-language-dir");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(LANGUAGES_DIR.to_owned() + "Rust.txt", dir.join("Rust.txt")).unwrap();
        std::fs::write(dir.join("Garbage.txt"), "this is not a language file at all\n").unwrap();
        // UTF-16, so 'read_to_string' refuses it before any parsing is attempted
        std::fs::write(dir.join("Utf16.txt"), [0xFFu8, 0xFE, 0x4C, 0x00, 0x61, 0x00]).unwrap();

        let (languages, mut faulty) = parse_languages_in_dir(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        faulty.sort_by(|a, b| a.file_name.cmp(&b.file_name));

        assert_eq!(1, languages.len());
        assert_eq!(vec!["Garbage.txt", "Utf16.txt"],
                faulty.iter().map(|x| x.file_name.as_str()).collect::<Vec<_>>());
        assert!(matches!(faulty[0].error, LanguageFileError::Malformed(_)), "got: {:?}", faulty[0].error);
        assert!(matches!(faulty[1].error, LanguageFileError::Unreadable(_)), "got: {:?}", faulty[1].error);
    }

    #[test]
    fn a_language_file_saved_with_a_byte_order_mark_still_reads() {
        let good = std::fs::read_to_string(LANGUAGES_DIR.to_owned() + "Rust.txt").unwrap();
        // the mark is not whitespace, so nothing on the way in was ever going to remove it
        assert!(!'\u{feff}'.is_whitespace());

        let with_mark = "\u{feff}".to_owned() + &good;
        assert_eq!(parse_language(&good), parse_language(&with_mark),
                "the same definition read differently depending on how the editor saved it");
        assert!(parse_language(&with_mark).is_some());
    }

    // Here the failure is silent: nothing lands among the faulty lines, since a line that is never
    // read is never rejected, and the only trace is the tiebreak warnings the file exists to
    // silence. The shipped copy opens with explanatory text, so the mark sits on a line that is
    // skipped anyway; a user who trims those lines away is the one who pays.
    #[test]
    fn a_conflicts_file_saved_with_a_byte_order_mark_still_reads() {
        let good = "===> contested-extensions\nm    Objective-C, MATLAB\n";
        let with_mark = "\u{feff}".to_owned() + good;

        let (rules, faulty) = parse_conflict_rules(good);
        let (rules_with_mark, faulty_with_mark) = parse_conflict_rules(&with_mark);

        assert_eq!(rules, rules_with_mark, "the same rules read differently depending on how the editor saved it");
        assert_eq!(faulty, faulty_with_mark);
        assert_eq!(1, rules_with_mark.by_extension.len(), "the rules of the file were dropped, and in silence");
    }

    #[test]
    fn multiline_comment_pairs_zip_by_position_and_unequal_counts_refuse_the_file() {
        let two_pairs = "Language\nPascalish\n\nExtensions\npax\n\nString symbols\n'\n\n\
Escape character\nnone\n\n\
Comment symbols\n//\n\nMulti line comment start\n{ (*\nMulti line comment end\n} *)\n";
        let parsed = parse_language(two_pairs).expect("two pairs must parse");
        assert_eq!(vec![("{".to_owned(), "}".to_owned()), ("(*".to_owned(), "*)".to_owned())],
                parsed.multiline_comments);

        let one_end = two_pairs.replace("} *)", "}");
        assert!(parse_language(&one_end).is_none(), "one end for two starts was accepted");
        let one_start = two_pairs.replace("{ (*", "{");
        assert!(parse_language(&one_start).is_none(), "one start for two ends was accepted");

        // and the shipped files that need a second pair declare it; D's second pair is the nesting
        // one, which is the case that makes the distinction per pair
        for name in ["Pascal.txt", "Delphi.txt"] {
            let language = parse_language_file(LANGUAGES_DIR.to_owned() + name).unwrap();
            assert_eq!(2, language.multiline_comments.len(), "{name} no longer declares both of its pairs");
        }
        let d = parse_language_file(LANGUAGES_DIR.to_owned() + "D.txt").unwrap();
        assert_eq!((1, 1), (d.multiline_comments.len(), d.nesting_comments.len()),
                "D.txt no longer declares its plain pair beside its nesting one");
    }

    #[test]
    fn a_string_symbol_is_declared_in_one_list_and_the_crossing_ones_are_numbered_last() {
        let good = "Language\nPylike\n\nExtensions\npyl\n\nString symbols\n\" '\n\n\
Multi line string symbols\n\"\"\"\n\nEscape character\n\\\n\nComment symbols\n#\n";
        let parsed = parse_language(good).expect("the declaration must parse");
        assert_eq!(vec!["\"".to_owned(), "'".to_owned()], parsed.strings.get_symbols());
        assert_eq!(vec![MultilineString::escaping("\"\"\"")], parsed.strings.get_multiline_strings());

        let twice = good.replace("String symbols\n\" '", "String symbols\n\" ' \"\"\"");
        assert!(parse_language(&twice).is_none(),
                "a symbol declared in both lists was accepted, leaving two answers to argue");
        let empty = good.replace("Multi line string symbols\n\"\"\"", "Multi line string symbols\n");
        assert!(parse_language(&empty).is_none());

        // a language that writes no string at all is allowed, which is what HTML needs
        let stringless = good.replace("String symbols\n\" '", "String symbols\n")
                .replace("Multi line string symbols\n\"\"\"\n\n", "");
        let parsed = parse_language(&stringless).expect("a language with no strings must parse");
        assert!(parsed.strings.get_symbols().is_empty() && parsed.strings.get_multiline_strings().is_empty());

        // and the shipped files that declare crossing strings still do
        for name in ["Python.txt", "JavaScript.txt", "Java.txt", "Rust.txt", "C#.txt", "Go.txt"] {
            let language = parse_language_file(LANGUAGES_DIR.to_owned() + name).unwrap();
            assert!(!language.strings.get_multiline_strings().is_empty(), "{name} lost its crossing string declaration");
        }
    }

    // '"""' is raw in Kotlin and escaping in Java, which is why the shape of a symbol cannot answer
    // this and each block does.
    #[test]
    fn a_crossing_string_declares_whether_a_backslash_cancels_its_closer() {
        let good = "Language\nGolike\n\nExtensions\ngol\n\nString symbols\n\"\n\n\
Multi line raw string symbols\n`\n\nEscape character\n\\\n\nComment symbols\n//\n";
        let parsed = parse_language(good).expect("the declaration must parse");
        assert_eq!(vec![MultilineString::raw("`")], parsed.strings.get_multiline_strings());

        // both blocks at once, numbered in the order the file declares them
        let both = good.replace("Multi line raw string symbols\n`",
                "Multi line string symbols\n\"\"\"\n\nMulti line raw string symbols\n`");
        let parsed = parse_language(&both).expect("both blocks must parse");
        assert_eq!(vec![MultilineString::escaping("\"\"\""), MultilineString::raw("`")],
                parsed.strings.get_multiline_strings());

        let twice = good.replace("Multi line raw string symbols\n`",
                "Multi line string symbols\n`\n\nMulti line raw string symbols\n`");
        assert!(parse_language(&twice).is_none(),
                "a symbol declared raw and escaping at once was accepted");
        let with_pair = good.replace("Multi line raw string symbols\n`",
                "Multi line raw string symbols\n`\n\nPaired string openers\n`\nPaired string closers\n'");
        assert!(parse_language(&with_pair).is_none(),
                "a symbol declared raw and as a pair opener at once was accepted");
        let empty = good.replace("Multi line raw string symbols\n`", "Multi line raw string symbols\n");
        assert!(parse_language(&empty).is_none());

        // and the shipped files whose crossing form escapes nothing say so
        for name in ["Go.txt", "Odin.txt", "D.txt", "Kotlin.txt", "Shell.txt", "PowerShell.txt"] {
            let language = parse_language_file(LANGUAGES_DIR.to_owned() + name).unwrap();
            assert!(language.strings.get_multiline_strings().iter().any(|crossing| !crossing.escapes),
                    "{name} lost its raw crossing string declaration");
        }
    }

    #[test]
    fn identification_literals_split_on_commas_so_one_may_hold_a_space() {
        let good = "Language\nPerlish\n\nExtensions\npx\n\n\
Identifying line starts\nuse strict, my $, =head\n\nIdentifying line contains\n:-, std::\n\n\
String symbols\n\"\n\nEscape character\n\\\n\nComment symbols\n#\n";
        let parsed = parse_language(good).expect("the declaration must parse");
        assert_eq!(vec!["use strict".to_owned(), "my $".to_owned(), "=head".to_owned()],
                parsed.identifying_line_starts);
        assert_eq!(vec![":-".to_owned(), "std::".to_owned()], parsed.identifying_line_contains);

        let alone = good.replace("Identifying line starts\nuse strict, my $, =head\n\n", "");
        assert_eq!(vec![":-".to_owned(), "std::".to_owned()],
                parse_language(&alone).expect("one block alone must parse").identifying_line_contains);
        let empty = good.replace("use strict, my $, =head", " ,, ");
        assert!(parse_language(&empty).is_none(), "a declared block holding nothing was accepted");
        let without = good.replace("Identifying line starts\nuse strict, my $, =head\n\n", "")
                .replace("Identifying line contains\n:-, std::\n\n", "");
        assert!(parse_language(&without).expect("optional blocks may be absent")
                .identifying_line_starts.is_empty());
    }

    #[test]
    fn a_nested_language_declares_its_tags_and_where_an_unnamed_section_falls() {
        let good = "Language\nWeblike\n\nExtensions\nwbl\n\nString symbols\n\n\nComment symbols\n\n\
Multi line comment start\n<!--\nMulti line comment end\n-->\n\n\
Nested language start\n<script <style\nNested language end\n</script> </style>\nNested language default\njs css\n";
        let parsed = parse_language(good).expect("the declaration must parse");
        assert_eq!(vec![NestedLanguage::of("<script", "</script>", "js"),
                NestedLanguage::of("<style", "</style>", "css")], parsed.nested_languages);

        let short = good.replace("Nested language default\njs css", "Nested language default\njs");
        assert!(parse_language(&short).is_none(),
                "a region without its default was accepted");
        let no_ends = good.replace("Nested language end\n</script> </style>\n", "");
        assert!(parse_language(&no_ends).is_none());
        let empty = good.replace("<script <style", "");
        assert!(parse_language(&empty).is_none());

        // A section is looked for where a tag begins, so an opener that is not one could never match
        let fenced = good.replace("<script <style", "```py <style").replace("</script> </style>", "``` </style>");
        assert!(parse_language(&fenced).is_none(),
                "an opener that is not a tag was accepted");
        let fenced_end = good.replace("</script> </style>", "``` </style>");
        assert!(parse_language(&fenced_end).is_none(),
                "a closer that is not a tag was accepted");

        // out of place it refuses the file whole, like every other block
        let misplaced = "Language\nWeblike\n\nExtensions\nwbl\n\n\
Nested language start\n<script\nNested language end\n</script>\nNested language default\njs\n\n\
String symbols\n\n\nComment symbols\n\n";
        assert!(parse_language(misplaced).is_none());
    }

    #[test]
    fn a_character_literal_symbol_has_its_own_block_and_shares_no_list() {
        let good = "Language\nRustlike\n\nExtensions\nrsl\n\nString symbols\n\n\n\
Character literal symbols\n'\n\nMulti line string symbols\n\"\n\n\
Escape character\n\\\n\nComment symbols\n//\n";
        let parsed = parse_language(good).expect("the declaration must parse");
        assert_eq!(vec!["'".to_owned()], parsed.strings.get_char_literals());
        assert_eq!(vec![MultilineString::escaping("\"")], parsed.strings.get_multiline_strings());

        // declared in two lists it refuses the file, empty it refuses the file
        let twice = good.replace("String symbols\n\n", "String symbols\n'\n");
        assert!(parse_language(&twice).is_none(),
                "a symbol that is both a string and a character literal was accepted");
        let empty = good.replace("Character literal symbols\n'\n\n", "Character literal symbols\n\n\n");
        assert!(parse_language(&empty).is_none());

        // the shipped declarations that use the block
        for name in ["Rust.txt", "D.txt"] {
            let language = parse_language_file(LANGUAGES_DIR.to_owned() + name).unwrap();
            assert_eq!(vec!["'".to_owned()], language.strings.get_char_literals(),
                    "{name} lost its character literal declaration");
        }
    }

    #[test]
    fn a_language_that_declares_a_string_has_to_say_what_escapes_it() {
        let good = "Language\nEsclike\n\nExtensions\nesc\n\nString symbols\n\"\n\n\
Escape character\n\\\n\nComment symbols\n//\n";
        assert_eq!(Some(b'\\'), parse_language(good).expect("the declaration must parse").strings.get_escape());

        let backtick = good.replace("Escape character\n\\", "Escape character\n`");
        assert_eq!(Some(b'`'), parse_language(&backtick).unwrap().strings.get_escape());
        let nothing = good.replace("Escape character\n\\", "Escape character\nnone");
        assert_eq!(None, parse_language(&nothing).unwrap().strings.get_escape());

        let missing = good.replace("Escape character\n\\\n\n", "");
        assert!(parse_language(&missing).is_none(),
                "a language declaring a string was accepted without saying what escapes it");
        // more than one byte is not a character, and the test walks backwards a byte at a time
        let two = good.replace("Escape character\n\\", "Escape character\n\\\\");
        assert!(parse_language(&two).is_none());

        let stringless = good.replace("String symbols\n\"", "String symbols\n")
                .replace("Escape character\n\\\n\n", "");
        assert!(parse_language(&stringless).is_some(), "a language with no strings was refused");

        // and the shipped files that declare each of the three answers
        for (name, escape) in [("Shell.txt", Some(b'\\')), ("PowerShell.txt", Some(b'`')),
                ("SQL.txt", None), ("Pascal.txt", None), ("C.txt", Some(b'\\'))] {
            let language = parse_language_file(LANGUAGES_DIR.to_owned() + name).unwrap();
            assert_eq!(escape, language.strings.get_escape(), "{name} declares the wrong escape");
        }
    }

    // One declaration covers '--[[', '--[=[' and every level above.
    #[test]
    fn a_pair_written_with_the_counted_marker_is_leveled() {
        let good = "Language\nLualike\n\nExtensions\nlux\n\nString symbols\n\" '\n\n\
Escape character\n\\\n\n\
Comment symbols\n--\nMulti line comment start\n--[=*[\nMulti line comment end\n]=*]\n";
        let parsed = parse_language(good).expect("the leveled declaration must parse");
        assert!(parsed.multiline_comments.is_empty());
        assert_eq!(1, parsed.leveled_comments.len());
        assert_eq!(("--[", b'['), (parsed.leveled_comments[0].start_prefix.as_str(), parsed.leveled_comments[0].start_suffix));
        assert_eq!(("]", b']'), (parsed.leveled_comments[0].end_prefix.as_str(), parsed.leveled_comments[0].end_suffix));

        let half = good.replace("]=*]", "]]");
        assert!(parse_language(&half).is_none(), "one leveled half was accepted");

        let lua = parse_language_file(LANGUAGES_DIR.to_owned() + "Lua.txt").unwrap();
        assert_eq!(1, lua.leveled_comments.len(), "Lua.txt no longer declares its long bracket");
    }

    #[test]
    fn a_missing_conflicts_file_is_not_a_mistake() {
        let (rules, faulty) = parse_conflict_rules_file("a/path/that/is/not/there.txt");
        assert_eq!((ConflictRules::default(), Vec::<(ConflictBlock, String)>::new()), (rules, faulty));
    }
    // The C++ fixture is a definition that is correct everywhere except for one stray line under the
    // language name, which is the mistake somebody editing a file by hand actually makes. Its
    // neighbours in the directory have to come through untouched.
    #[test]
    fn a_stray_line_in_one_definition_costs_that_language_and_no_other() {
        let (languages, faulty) = parse_languages_in_dir(
                FIXTURES_DIR.to_owned() + "definitions/").unwrap();

        let mut names = languages.iter().map(|x| x.name.as_str()).collect::<Vec<_>>();
        names.sort_unstable();
        assert_eq!(vec!["Java", "Rust"], names);
        assert_eq!(vec!["C++.txt"], faulty.iter().map(|x| x.file_name.as_str()).collect::<Vec<_>>());
        assert!(matches!(faulty[0].error, LanguageFileError::Malformed(_)), "got: {:?}", faulty[0].error);
    }

    // A shipped file that does not parse costs the whole language, and the only signal during a run
    // is one line saying "formatting problems".
    #[test]
    fn every_shipped_language_file_parses() {
        let dir = LANGUAGES_DIR;
        let (languages, faulty) = parse_languages_in_dir(dir)
                .unwrap_or_else(|e| panic!("the shipped languages dir did not parse at all: {e:?}"));

        assert!(faulty.is_empty(), "these shipped language files do not parse: {faulty:?}");

        let on_disk = std::fs::read_dir(dir).unwrap()
                .flatten()
                .filter(|e| e.path().is_file())
                .count();
        assert_eq!(on_disk, languages.len(),
                "{} language files on disk but {} parsed", on_disk, languages.len());

        // Two files declaring one name is a language silently lost, since resolving keys by name
        // and the second declaration wins.
        let mut names = languages.iter().map(|x| x.name.as_str()).collect::<Vec<_>>();
        names.sort_unstable();
        let duplicates = names.windows(2).filter(|pair| pair[0] == pair[1])
                .map(|pair| pair[0]).collect::<Vec<_>>();
        assert!(duplicates.is_empty(), "these names are declared by more than one shipped file: {duplicates:?}");

        // A language with no string symbol at all is markup and only markup: HTML, and the shells
        // of Vue and Svelte, whose code lives in sections that carry their own languages' strings.
        // Naming them keeps a symbol lost from any other file loud instead of allowed.
        for language in &languages {
            let name = &language.name;
            assert!(!language.extensions.is_empty() || !language.filenames.is_empty(),
                    "{name} declares neither an extension nor a filename");
            assert!(!language.strings.get_symbols().is_empty() || !language.strings.get_multiline_strings().is_empty()
                    || name == "HTML" || !language.nested_languages.is_empty(),
                    "{name} declares no string symbol");
        }

        // An interpreter two shipped files claim would be settled by the alphabetical tiebreak,
        // since the priority file has no block for it: refused here instead, until a real contest
        // earns that block.
        let mut interpreters = languages.iter()
                .flat_map(|language| language.shebangs.iter()
                        .map(move |shebang| (shebang.to_ascii_lowercase(), language.name.as_str())))
                .collect::<Vec<_>>();
        interpreters.sort_unstable();
        let contested = interpreters.windows(2).filter(|pair| pair[0].0 == pair[1].0)
                .map(|pair| format!("'{}' ({}, {})", pair[0].0, pair[0].1, pair[1].1))
                .collect::<Vec<_>>();
        assert!(contested.is_empty(), "these interpreters are claimed by more than one shipped file: {contested:?}");
    }
}
