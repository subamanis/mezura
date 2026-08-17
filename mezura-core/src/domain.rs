// The words the problem is described in, and the bottom of the dependency graph: nothing here knows
// about threads, settings or printing. These exist before anything has been counted, which is what
// separates this file from 'result.rs' beside it.
use std::{collections::HashMap, sync::OnceLock};

// Both kinds of symbol are declared the same way: a plain list of symbols that end at the end of
// the line, and beside it the ones that cross lines, which come in pairs. So 'string_symbols' is
// to 'multiline_strings' what 'comment_symbols' is to 'multiline_comments'.
#[derive(Debug, Clone)]
pub struct Language {
    pub name: String,
    pub extensions : Vec<String>,
    // Whole names, for the files that carry no extension worth reading: 'Makefile', 'Dockerfile',
    // and 'CMakeLists.txt', whose extension says text and means nothing
    pub filenames : Vec<String>,
    pub string_symbols : Vec<String>,
    // The symbol of a character literal, Rust's and D's '. One that does not close on its own line
    // is not a literal at all, so a lifetime's lone ' opens nothing, while a '"' shields its quote
    pub char_literal_symbols : Vec<String>,
    pub multiline_strings : Vec<MultilineString>,
    pub comment_symbols : Vec<String>,
    pub multiline_comments : Vec<(String, String)>,
    // The pairs that nest inside themselves, so a closer only ends the block when it has closed
    // as many as were opened: OCaml's whole comment syntax, D's '/+ +/' beside its plain '/* */'
    pub nesting_comments : Vec<(String, String)>,
    // Lua's long brackets: a pair written with '=*' in a language file, '--[=*[' with ']=*]',
    // where the run of '=' is counted at the opener and only an end carrying the same count
    // closes, so a ']]' inside a '--[==[' block is text
    pub leveled_comments : Vec<LeveledPair>,
    // The symbol that joins a line to the next one when it is the last thing on it, and what it
    // joins. C splices anything, including a line comment; JavaScript and Python only continue a
    // string literal; Java, Go and C# have no such thing at all.
    pub line_continuation : Option<LineContinuation>,
    // What cancels a string symbol standing after it: the backslash in most languages, the backtick
    // in PowerShell, and nothing in the family that escapes a quote by doubling it. None means a
    // quote closes whatever stands in front of it, which is what Pascal, Ada, Fortran, COBOL and
    // standard SQL need.
    pub escape_character : Option<u8>,
    // Sections of the file that belong to another language, HTML's '<script>' and '<style>'. The
    // lines between the tags are counted with that language's own symbols and reported under it.
    pub nested_languages : Vec<NestedLanguage>,
    pub keywords : Vec<Keyword>,
    // Worked out from the symbols above and reused for every file of this language.
    pub(crate) scan_plan : OnceLock<crate::engine::file_parser::ScanPlan>
}

impl Language {
    pub fn new(name: impl AsRef<str>,
        extensions: impl IntoIterator<Item = impl AsRef<str>>,
        string_symbols: impl IntoIterator<Item = impl AsRef<str>>,
        comment_symbols: impl IntoIterator<Item = impl AsRef<str>>,
        multiline_comments: &[(&str, &str)],
        keywords: impl IntoIterator<Item = Keyword>) -> Self
    {
        Language {
            name : name.as_ref().to_owned(),
            extensions : owned_strings(extensions),
            filenames : Vec::new(),
            string_symbols : owned_strings(string_symbols),
            char_literal_symbols : Vec::new(),
            multiline_strings : Vec::new(),
            comment_symbols : owned_strings(comment_symbols),
            multiline_comments : multiline_comments.iter()
                    .map(|(start, end)| ((*start).to_owned(), (*end).to_owned())).collect(),
            nesting_comments : Vec::new(),
            leveled_comments : Vec::new(),
            line_continuation : None,
            // The backslash, since every language the constructor is reached from in a test
            // escapes with it; a language file has to say so itself
            escape_character : Some(b'\\'),
            nested_languages : Vec::new(),
            keywords : keywords.into_iter().collect(),
            scan_plan : OnceLock::new()
        }
    }

    // Takes the parsed pair rather than its text, so that the one place that decides whether a pair
    // is written correctly is 'LeveledPair::of' and a caller cannot reach a state this has to refuse.
    pub fn with_leveled_comments(mut self, pairs: &[LeveledPair]) -> Self {
        self.leveled_comments.extend(pairs.iter().cloned());
        self
    }

    pub fn with_filenames(mut self, names: &[&str]) -> Self {
        self.filenames.extend(names.iter().map(|x| (*x).to_owned()));
        self
    }

    pub fn with_line_continuation(mut self, symbol: &str, in_strings: bool, in_comments: bool) -> Self {
        self.line_continuation = Some(LineContinuation {
            symbol: symbol.to_owned(), in_strings, in_comments });
        self
    }

    pub fn with_nested_languages(mut self, regions: &[NestedLanguage]) -> Self {
        self.nested_languages.extend(regions.iter().cloned());
        self
    }

    pub fn with_char_literals(mut self, symbols: &[&str]) -> Self {
        self.char_literal_symbols.extend(symbols.iter().map(|x| (*x).to_owned()));
        self
    }

    pub fn with_multiline_strings(mut self, symbols: &[&str]) -> Self {
        self.multiline_strings.extend(symbols.iter().map(|x| MultilineString::escaping(x)));
        self
    }

    pub fn with_raw_multiline_strings(mut self, symbols: &[&str]) -> Self {
        self.multiline_strings.extend(symbols.iter().map(|x| MultilineString::raw(x)));
        self
    }

    pub fn with_string_pairs(mut self, pairs: &[(&str, &str)]) -> Self {
        self.multiline_strings.extend(pairs.iter().map(|(open, close)| MultilineString::of(open, close)));
        self
    }

    pub fn with_nesting_comments(mut self, pairs: &[(&str, &str)]) -> Self {
        self.nesting_comments.extend(pairs.iter()
                .map(|(start, end)| ((*start).to_owned(), (*end).to_owned())));
        self
    }

    pub fn supports_multiline_comments(&self) -> bool {
        !self.multiline_comments.is_empty() || !self.nesting_comments.is_empty()
                || !self.leveled_comments.is_empty()
    }

    // The scan numbers every string symbol of a language in one sequence, the single line ones
    // first, the character literals after them and the crossing ones last, which is the order the
    // plan is built in.
    pub(crate) fn get_string_pair_of(&self, symbol: u8) -> (&str, &str) {
        let symbol = symbol as usize;
        if let Some(single) = self.string_symbols.get(symbol) {
            return (single, single);
        }
        match self.char_literal_symbols.get(symbol - self.string_symbols.len()) {
            Some(literal) => (literal, literal),
            None => {
                let crossing = &self.multiline_strings[
                        symbol - self.string_symbols.len() - self.char_literal_symbols.len()];
                (&crossing.open, &crossing.close)
            }
        }
    }

    pub(crate) fn string_crosses_lines(&self, symbol: u8) -> bool {
        symbol as usize >= self.string_symbols.len() + self.char_literal_symbols.len()
    }

    // The one place that knows the numbering, so that a pair kind added later is one match arm and
    // not seven pieces of arithmetic spread over two files.
    pub(crate) fn get_comment_pair_of(&self, symbol: u8) -> CommentPair<'_> {
        let symbol = symbol as usize;
        if let Some((start, end)) = self.multiline_comments.get(symbol) {
            return CommentPair::Plain { start, end };
        }
        match self.nesting_comments.get(symbol - self.multiline_comments.len()) {
            Some((start, end)) => CommentPair::Nesting { start, end },
            None => CommentPair::Leveled(
                    &self.leveled_comments[symbol - self.multiline_comments.len() - self.nesting_comments.len()])
        }
    }

    // In the order the numbering runs, which is what the scan plan assigns its numbers from.
    pub(crate) fn comment_pairs(&self) -> impl Iterator<Item = CommentPair<'_>> {
        self.multiline_comments.iter().map(|(start, end)| CommentPair::Plain { start, end })
                .chain(self.nesting_comments.iter().map(|(start, end)| CommentPair::Nesting { start, end }))
                .chain(self.leveled_comments.iter().map(CommentPair::Leveled))
    }

    pub(crate) fn comment_nests(&self, symbol: u8) -> bool {
        matches!(self.get_comment_pair_of(symbol), CommentPair::Nesting { .. })
    }

    pub(crate) fn comment_is_leveled(&self, symbol: u8) -> bool {
        matches!(self.get_comment_pair_of(symbol), CommentPair::Leveled(_))
    }

    // The whole width of one occurrence, which for a leveled pair depends on how many '=' it
    // carried; a plain or nesting pair ignores the level
    pub(crate) fn comment_start_len(&self, symbol: u8, level: u8) -> usize {
        match self.get_comment_pair_of(symbol) {
            CommentPair::Leveled(pair) => pair.start_prefix.len() + level as usize + 1,
            CommentPair::Plain { start, .. } | CommentPair::Nesting { start, .. } => start.len()
        }
    }

    pub(crate) fn comment_end_len(&self, symbol: u8, level: u8) -> usize {
        match self.get_comment_pair_of(symbol) {
            CommentPair::Leveled(pair) => pair.end_prefix.len() + level as usize + 1,
            CommentPair::Plain { end, .. } | CommentPair::Nesting { end, .. } => end.len()
        }
    }
}

// A comment pair as the parser has to treat it, which is what the three lists of a language mean
// once they are numbered in one sequence. Plain ends at its first closer, nesting counts its own
// openers, leveled closes only at an end carrying the count its opener did.
pub(crate) enum CommentPair<'a> {
    Plain { start: &'a str, end: &'a str },
    Nesting { start: &'a str, end: &'a str },
    Leveled(&'a LeveledPair)
}

// A string that crosses lines: the same symbol twice for Python's '"""', two different ones for a
// raw form like 'r#"' with '"#'.
//
// 'escapes' is the one thing the shape cannot answer. A backtick escapes nothing in Go, Odin and D
// and does escape in a JavaScript template literal, and '"""' splits the same way between Kotlin
// and Java, so whether a backslash in front of the closer cancels it belongs to the symbol and not
// to whether its two halves differ. A form written with two different symbols is raw by
// construction, which is why 'of' takes no flag.
#[derive(Debug, Clone, PartialEq)]
pub struct MultilineString {
    pub open : String,
    pub close : String,
    pub escapes : bool
}

impl MultilineString {
    pub fn escaping(symbol: &str) -> MultilineString {
        MultilineString { open: symbol.to_owned(), close: symbol.to_owned(), escapes: true }
    }

    pub fn raw(symbol: &str) -> MultilineString {
        MultilineString { open: symbol.to_owned(), close: symbol.to_owned(), escapes: false }
    }

    pub fn of(open: &str, close: &str) -> MultilineString {
        MultilineString { open: open.to_owned(), close: close.to_owned(), escapes: false }
    }
}

// One section of another language inside a file: everything between 'start' and 'end' is counted
// with that language's own symbols. Which language is read off the opener tag's 'lang' or 'type'
// attribute through the extension lookup, and 'default' answers when the tag names none: an
// extension, not a language name, so both paths resolve the same way. The tags match without
// regard to case, the way HTML reads them.
#[derive(Debug, Clone, PartialEq)]
pub struct NestedLanguage {
    pub start : String,
    pub end : String,
    pub default : String
}

impl NestedLanguage {
    pub fn of(start: &str, end: &str, default: &str) -> NestedLanguage {
        NestedLanguage { start: start.to_owned(), end: end.to_owned(), default: default.to_owned() }
    }
}

// A line ending in this symbol is joined to the one after it, before anything is decided about
// either. 'in_comments' is what separates C, where the join happens whatever the line held, from
// JavaScript and Python, where a line comment ends at the newline whatever follows it.
#[derive(Debug, Clone, PartialEq)]
pub struct LineContinuation {
    pub symbol : String,
    pub in_strings : bool,
    pub in_comments : bool
}

// One half of a long-bracket pair: the fixed bytes before the counted run of '=', and the single
// byte that closes the opener after it. '--[=*[' is prefix "--[", suffix '['.
#[derive(Debug, Clone, PartialEq)]
pub struct LeveledPair {
    pub start_prefix : String,
    pub start_suffix : u8,
    pub end_prefix : String,
    pub end_suffix : u8,
}

impl LeveledPair {
    pub fn of(start: &str, end: &str) -> Option<LeveledPair> {
        let (start_prefix, start_suffix) = split_leveled_half(start)?;
        let (end_prefix, end_suffix) = split_leveled_half(end)?;
        Some(LeveledPair { start_prefix, start_suffix, end_prefix, end_suffix })
    }
}

// The half before '=*' and the one byte after it; anything else is not a leveled symbol
fn split_leveled_half(pattern: &str) -> Option<(String, u8)> {
    let (prefix, rest) = pattern.split_once("=*")?;
    if prefix.is_empty() || rest.len() != 1 || rest.contains("=*") {
        return None;
    }
    Some((prefix.to_owned(), rest.as_bytes()[0]))
}

// Hand written to leave 'scan_plan' out: it is a cache, filled when a language parses its first
// file, so comparing it would make one language stop equalling its own untouched copy.
impl PartialEq for Language {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.extensions == other.extensions
            && self.filenames == other.filenames
            && self.string_symbols == other.string_symbols
            && self.char_literal_symbols == other.char_literal_symbols
            && self.multiline_strings == other.multiline_strings
            && self.comment_symbols == other.comment_symbols
            && self.multiline_comments == other.multiline_comments
            && self.nesting_comments == other.nesting_comments
            && self.leveled_comments == other.leveled_comments
            && self.line_continuation == other.line_continuation
            && self.escape_character == other.escape_character
            && self.nested_languages == other.nested_languages
            && self.keywords == other.keywords
    }
}

// 'descriptive_name' is what the report shows, 'aliases' are the spellings that count towards it, so
// 'classes' can be counted from both 'class' and 'record'.
#[derive(Debug,PartialEq)]
pub struct Keyword {
    pub descriptive_name : String,
    pub aliases : Vec<String>
}

impl Keyword {
    pub fn new(descriptive_name: impl AsRef<str>, aliases: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        Keyword {
            descriptive_name : descriptive_name.as_ref().to_owned(),
            aliases : owned_strings(aliases)
        }
    }
}

impl Clone for Keyword {
    fn clone(&self) -> Self {
        Keyword {
            descriptive_name : self.descriptive_name.to_owned(),
            aliases : self.aliases.to_owned()
        }
    }
}

// Where each line of a file landed, one slot per line, so the nine add up to the lines. "Words"
// are word bytes: a letter, a digit, or anything above ASCII. A line with none anywhere is
// punctuation some grammar required, or blank.
#[derive(Debug,PartialEq,Default,Clone)]
pub struct LineClasses {
    pub words_in_code : usize,
    // Lines whose only content is the inside of a string literal, the middle of a multiline
    // string being the plain case. String content is data and both models count it as code.
    pub string_content : usize,
    // Words only inside a comment, on a line that also carries code punctuation: '} // words'.
    // The two models part here, which is why it cannot sit in either neighbour: content reads
    // the words and says comment, region reads the '}' and says code.
    pub comment_words_beside_code : usize,
    pub words_in_comment : usize,
    pub punctuation_in_code : usize,
    pub punctuation_in_comment : usize,
    pub blank : usize,
    pub blank_in_comment : usize,
    pub blank_in_string : usize
}

impl LineClasses {
    // Everything that walks all nine goes through 'to_array' and 'of_array', and everything that
    // writes them out or reads them back pairs those with these names, so a tenth class is two
    // lines here and a compiler error at each of the two conversions rather than a count that is
    // silently dropped from a document, a log and the leftovers row of the overview.
    pub const NAMES : [&'static str; 9] = ["words_in_code", "string_content",
            "comment_words_beside_code", "words_in_comment", "punctuation_in_code",
            "punctuation_in_comment", "blank", "blank_in_comment", "blank_in_string"];

    pub fn of_array(counts: [usize; 9]) -> Self {
        LineClasses {
            words_in_code: counts[0],
            string_content: counts[1],
            comment_words_beside_code: counts[2],
            words_in_comment: counts[3],
            punctuation_in_code: counts[4],
            punctuation_in_comment: counts[5],
            blank: counts[6],
            blank_in_comment: counts[7],
            blank_in_string: counts[8]
        }
    }

    pub fn to_array(&self) -> [usize; 9] {
        [self.words_in_code, self.string_content, self.comment_words_beside_code,
         self.words_in_comment, self.punctuation_in_code, self.punctuation_in_comment,
         self.blank, self.blank_in_comment, self.blank_in_string]
    }

    pub fn calculate_lines(&self) -> usize {
        self.to_array().iter().sum()
    }

    pub(crate) fn add(&mut self, other: &LineClasses) {
        *self = combine_classes(self, other, |mine, theirs| mine + theirs);
    }

    // Saturating because the numbers taken out may come off a document or a log, where nothing
    // promises they stay inside what they are taken from
    pub fn subtract(&mut self, other: &LineClasses) {
        *self = combine_classes(self, other, usize::saturating_sub);
    }
}

// Where the code and comment columns come from. The engine only ever fills 'LineClasses'; what a
// column shows is this fold, chosen at presentation time, so the same run answers both models.
//
// 'Content' asks what a line says: words in code make it code, words only in a comment make it a
// comment, and punctuation and blanks are neither. 'Region' asks where a line sits, which is how
// cloc, tokei and scc count: any code on the line makes it code, a line inside a comment belongs
// to the comment whatever it holds, and only a blank outside everything is blank.
#[derive(Debug,PartialEq,Eq,Clone,Copy,Default)]
pub enum CountingModel {
    #[default]
    Content,
    Region
}

impl CountingModel {
    pub fn parse(value: &str) -> Option<CountingModel> {
        match value.trim().to_lowercase().as_str() {
            "content" => Some(Self::Content),
            "region" => Some(Self::Region),
            _ => None
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Content => "content",
            Self::Region => "region"
        }
    }

    pub fn get_other(&self) -> CountingModel {
        match self {
            Self::Content => Self::Region,
            Self::Region => Self::Content
        }
    }

    // What the third quantity is called under this model. Every face of a report asks here, the
    // column the table heads and the key the document writes, or one of them would name it with a
    // word the other does not use.
    pub fn get_third_quantity_name(&self) -> &'static str {
        match self {
            Self::Content => "extra",
            Self::Region => "blanks"
        }
    }

    pub fn calculate_code_lines(&self, classes: &LineClasses) -> usize {
        match self {
            Self::Content => classes.words_in_code + classes.string_content,
            Self::Region => classes.words_in_code + classes.string_content
                    + classes.comment_words_beside_code + classes.punctuation_in_code
                    + classes.blank_in_string
        }
    }

    pub fn calculate_comment_lines(&self, classes: &LineClasses) -> usize {
        match self {
            Self::Content => classes.comment_words_beside_code + classes.words_in_comment,
            Self::Region => classes.words_in_comment + classes.punctuation_in_comment
                    + classes.blank_in_comment
        }
    }
}

// The folded columns and the average size are methods rather than fields, so a stored copy cannot
// drift from the numbers it comes from.
#[derive(Debug,PartialEq,Default,Clone)]
pub struct Stats {
    pub files : usize,
    pub bytes : usize,
    pub lines : usize,
    pub classes : LineClasses,
    // Added up over a run these are every keyword any language declared, which answers "how many
    // classes are in this project" across the several languages that have such a thing.
    pub keyword_occurences : HashMap<String,usize>
}

impl Stats {
    pub fn new(files: usize, bytes: usize, lines: usize, classes: LineClasses,
            keyword_occurences: HashMap<String,usize>) -> Self
    {
        Stats { files, bytes, lines, classes, keyword_occurences }
    }

    pub fn calculate_code_lines(&self, model: CountingModel) -> usize {
        model.calculate_code_lines(&self.classes)
    }

    pub fn calculate_comment_lines(&self, model: CountingModel) -> usize {
        model.calculate_comment_lines(&self.classes)
    }

    // The third column: extra under 'Content', blanks under 'Region'.
    //
    // Saturating because the fields are public and this is also built from numbers read off a log
    // file: counts that do not add up are the caller's arithmetic, not a reason to panic.
    pub fn calculate_extra_lines(&self, model: CountingModel) -> usize {
        self.lines.saturating_sub(self.calculate_code_lines(model))
                .saturating_sub(self.calculate_comment_lines(model))
    }

    // Rounded to whole bytes, and zero rather than a division by zero when nothing was counted
    pub fn calculate_average_size(&self) -> usize {
        self.bytes.checked_div(self.files).unwrap_or(0)
    }

    // Not public, because its argument is not: the one thing outside this crate could do with it is
    // hand-build a file's counts and get the keyword indices to line up with a slice it also has to
    // supply. The way in from outside is 'run'.
    pub(crate) fn add_file(&mut self, stats: &FileStats, bytes: usize, keywords: &[Keyword]) {
        // The walk counts a line and then sorts it into exactly one class, so the two are a
        // measurement and its itemisation and have to agree. This is the only door a counted file
        // comes through, which makes it the place a classification that lost a line shows up. Not
        // on 'add' below: that one also takes counts parsed out of a document, where nothing
        // promises anything.
        debug_assert_eq!(stats.lines, stats.classes.calculate_lines(),
                "a counted file has {} lines and {} of them landed in a class",
                stats.lines, stats.classes.calculate_lines());

        self.files += 1;
        self.bytes += bytes;
        self.lines += stats.lines;
        self.classes.add(&stats.classes);
        for (keyword_index, occurrences) in stats.keyword_occurences.iter().enumerate() {
            if *occurrences > 0 {
                *self.keyword_occurences.entry(keywords[keyword_index].descriptive_name.clone())
                        .or_default() += *occurrences;
            }
        }
    }

    pub fn add(&mut self, other: &Stats) {
        self.files += other.files;
        self.bytes += other.bytes;
        self.lines += other.lines;
        self.classes.add(&other.classes);
        for (keyword, occurrences) in other.keyword_occurences.iter() {
            *self.keyword_occurences.entry(keyword.clone()).or_default() += *occurrences;
        }
    }
}

// What a run starts each language from: every keyword it declares, at zero. The merge that ends a
// counting thread reaches for a keyword by name, and one with no slot waiting would make that
// language count nothing.
impl From<&Language> for Stats {
    fn from(language: &Language) -> Self {
        Stats { keyword_occurences: create_keyword_slots(language), ..Default::default() }
    }
}

// What one file came to, on its way into a 'Stats'. Kept apart from it for one reason: the parser
// identifies a keyword by its position in the language's list, because that is what the matcher
// hands back, so counting into a vector slot costs no hashing and no string copying in the innermost
// loop of the parse. The names are attached once per file, in 'add_file'.
#[derive(Debug,PartialEq,Default)]
pub(crate) struct FileStats {
    pub lines : usize,
    pub classes : LineClasses,
    pub keyword_occurences : Vec<usize>
}

impl FileStats {
    pub(crate) fn with_keywords(keywords: &[Keyword]) -> Self {
        FileStats {
            lines : 0,
            classes : LineClasses::default(),
            keyword_occurences : vec![0; keywords.len()]
        }
    }
}

// 'AsRef<str>' and not 'Into<String>': everything ends up owned anyway, and the borrowed form also
// takes a Cow off a path and a reference to any of them.
pub(crate) fn owned_strings(items: impl IntoIterator<Item = impl AsRef<str>>) -> Vec<String> {
    items.into_iter().map(|x| x.as_ref().to_owned()).collect()
}

fn combine_classes(one: &LineClasses, other: &LineClasses,
    of_each: impl Fn(usize, usize) -> usize) -> LineClasses
{
    let (mine, theirs) = (one.to_array(), other.to_array());
    LineClasses::of_array(std::array::from_fn(|at| of_each(mine[at], theirs[at])))
}

fn create_keyword_slots(language: &Language) -> HashMap<String,usize> {
    let mut map = HashMap::<String,usize>::new();
    for keyword in &language.keywords {
        map.insert(keyword.descriptive_name.to_owned(), 0);
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats_of(lines: usize, code: usize, comments: usize) -> Stats {
        let classes = LineClasses { words_in_code: code, words_in_comment: comments, ..Default::default() };
        Stats::new(1, 0, lines, classes, HashMap::new())
    }

    // The parser cannot produce these: a line is counted once and falls into exactly one of the
    // classes. They come off a log file whose head was lost.
    #[test]
    fn counts_that_do_not_add_up_give_no_extra_lines_rather_than_a_panic() {
        assert_eq!(0, stats_of(0, 0, 900).calculate_extra_lines(CountingModel::Content));
        assert_eq!(0, stats_of(40, 900, 50).calculate_extra_lines(CountingModel::Content));
        assert_eq!(0, stats_of(100, 60, 40).calculate_extra_lines(CountingModel::Content));
        assert_eq!(10, stats_of(100, 60, 30).calculate_extra_lines(CountingModel::Content));
    }

    #[test]
    fn each_model_folds_the_classes_into_its_own_columns() {
        let classes = LineClasses {
            words_in_code: 10, string_content: 5, comment_words_beside_code: 4, words_in_comment: 8,
            punctuation_in_code: 3, punctuation_in_comment: 2, blank: 6, blank_in_comment: 1,
            blank_in_string: 7
        };
        let stats = Stats::new(1, 0, 46, classes, HashMap::new());

        assert_eq!(15, stats.calculate_code_lines(CountingModel::Content));
        assert_eq!(12, stats.calculate_comment_lines(CountingModel::Content));
        assert_eq!(19, stats.calculate_extra_lines(CountingModel::Content));

        assert_eq!(29, stats.calculate_code_lines(CountingModel::Region));
        assert_eq!(11, stats.calculate_comment_lines(CountingModel::Region));
        assert_eq!(6, stats.calculate_extra_lines(CountingModel::Region));
    }

    #[test]
    fn an_average_size_over_no_files_is_zero_rather_than_a_division_by_zero() {
        assert_eq!(0, Stats::default().calculate_average_size());
        assert_eq!(250, Stats::new(4, 1000, 0, LineClasses::default(), HashMap::new()).calculate_average_size());
    }
}
