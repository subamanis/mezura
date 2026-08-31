// The words the problem is described in, and the bottom of the dependency graph: nothing here knows
// about threads, settings or printing. These exist before anything has been counted, which is what
// separates this file from 'result.rs' beside it.
use std::{collections::HashMap, sync::OnceLock};

/// One language: what it is called, which files belong to it, and the symbols that decide what
/// each of their lines is.
///
/// Built with [`Language::new`] and the `with_` methods, never with a struct literal: one field is
/// a cache this crate fills as it counts.
#[derive(Debug, Clone)]
pub struct Language {
    /// What the report calls it, and the name `--languages` answers to.
    pub name: String,
    /// Without the dot, and matched without regard to case.
    pub extensions : Vec<String>,
    /// Whole names, for the files that carry no extension worth reading: `Makefile`, `Dockerfile`,
    /// and `CMakeLists.txt`, whose extension says text and means nothing.
    pub filenames : Vec<String>,
    /// Interpreter names as a `#!` line spells them, `sh` or `python`, for the scripts whose name
    /// says nothing at all. Only a file with no extension and an unclaimed name is ever probed.
    pub shebangs : Vec<String>,
    /// Which quotes open a string, and what escapes one.
    pub strings : StringRules,
    /// The symbols that make the rest of the line a comment.
    pub comment_symbols : Vec<String>,
    /// Block comments, opener and closer, ending at the first closer.
    pub multiline_comments : Vec<(String, String)>,
    /// The pairs that nest inside themselves, so a closer only ends the block when it has closed
    /// as many as were opened: OCaml's whole comment syntax, D's `/+ +/` beside its plain `/* */`.
    pub nesting_comments : Vec<(String, String)>,
    /// Lua's long brackets, `--[=*[` with `]=*]`: the run of `=` is counted at the opener and only
    /// an end carrying the same count closes, so a `]]` inside a `--[==[` block is text.
    pub leveled_comments : Vec<LeveledPair>,
    /// The symbol that joins a line to the one after it when it is the last thing on it, and what
    /// it joins. C splices anything, including a line comment; JavaScript and Python only continue
    /// a string literal; Java, Go and C# have no such thing at all.
    pub line_continuation : Option<LineContinuation>,
    /// Sections of the file that belong to another language, HTML's `<script>` and `<style>`. The
    /// lines between the tags are counted with that language's own symbols and reported under it.
    pub nested_languages : Vec<NestedLanguage>,
    /// The words this language is worth counting, `classes` and `structs` and the like.
    pub keywords : Vec<Keyword>,
    /// Literals identifying this language when a line begins with one, leading whitespace aside.
    pub identifying_line_starts : Vec<String>,
    /// The same, for a literal found anywhere in a line.
    pub identifying_line_contains : Vec<String>,
    // Worked out from the symbols above and reused for every file of this language.
    pub(crate) scan_plan : OnceLock<crate::engine::file_parser::ScanPlan>
}

impl Language {
    /// A language with nothing but line and block comments. Everything else is added with the
    /// `with_` methods below.
    pub fn new(name: impl AsRef<str>,
        extensions: impl IntoIterator<Item = impl AsRef<str>>,
        strings: StringRules,
        comment_symbols: impl IntoIterator<Item = impl AsRef<str>>,
        multiline_comments: &[(&str, &str)],
        keywords: impl IntoIterator<Item = Keyword>) -> Self
    {
        Language {
            name : name.as_ref().to_owned(),
            extensions : owned_strings(extensions),
            filenames : Vec::new(),
            shebangs : Vec::new(),
            strings,
            comment_symbols : owned_strings(comment_symbols),
            multiline_comments : multiline_comments.iter()
                    .map(|(start, end)| ((*start).to_owned(), (*end).to_owned())).collect(),
            nesting_comments : Vec::new(),
            leveled_comments : Vec::new(),
            line_continuation : None,
            nested_languages : Vec::new(),
            keywords : keywords.into_iter().collect(),
            identifying_line_starts : Vec::new(),
            identifying_line_contains : Vec::new(),
            scan_plan : OnceLock::new()
        }
    }

    /// Adds the content evidence that identifies this language when its extension is contested.
    /// An empty literal identifies nothing and is dropped.
    pub fn with_identification(mut self, line_starts: impl IntoIterator<Item = impl AsRef<str>>,
        line_contains: impl IntoIterator<Item = impl AsRef<str>>) -> Self
    {
        let kept = |x: Vec<String>| x.into_iter().filter(|literal| !literal.is_empty()).collect();
        self.identifying_line_starts = kept(owned_strings(line_starts));
        self.identifying_line_contains = kept(owned_strings(line_contains));
        self
    }

    /// Adds long-bracket comment pairs. Takes the parsed pair rather than its text, so the one
    /// place that decides whether a pair is written correctly is [`LeveledPair::of`].
    pub fn with_leveled_comments(mut self, pairs: &[LeveledPair]) -> Self {
        self.leveled_comments.extend(pairs.iter().cloned());
        self
    }

    /// Adds whole file names this language claims.
    pub fn with_filenames(mut self, names: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.filenames.extend(owned_strings(names));
        self
    }

    /// Adds the interpreter names a `#!` line may carry.
    pub fn with_shebangs(mut self, interpreters: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.shebangs.extend(owned_strings(interpreters));
        self
    }

    /// Sets the symbol that joins a line to the one after it, and whether the join also happens
    /// inside a string and inside a comment.
    ///
    /// An empty symbol leaves the language with no line continuation at all, since every line ends
    /// in one and the whole file would join into a single line.
    pub fn with_line_continuation(mut self, symbol: &str, in_strings: bool, in_comments: bool) -> Self {
        if symbol.is_empty() {
            return self;
        }
        self.line_continuation = Some(LineContinuation {
            symbol: symbol.to_owned(), in_strings, in_comments });
        self
    }

    /// Adds sections of the file that another language is counted inside.
    pub fn with_nested_languages(mut self, regions: &[NestedLanguage]) -> Self {
        self.nested_languages.extend(regions.iter().cloned());
        self
    }

    /// Adds comment pairs that nest inside themselves.
    pub fn with_nesting_comments(mut self, pairs: &[(impl AsRef<str>, impl AsRef<str>)]) -> Self {
        self.nesting_comments.extend(pairs.iter()
                .map(|(start, end)| (start.as_ref().to_owned(), end.as_ref().to_owned())));
        self
    }

    /// Whether it has any kind of block comment: plain, nesting or long bracket.
    pub fn supports_multiline_comments(&self) -> bool {
        !self.multiline_comments.is_empty() || !self.nesting_comments.is_empty()
                || !self.leveled_comments.is_empty()
    }

    pub(crate) fn declares_identification(&self) -> bool {
        !self.identifying_line_starts.is_empty() || !self.identifying_line_contains.is_empty()
    }

    // The scan numbers every string symbol of a language in one sequence, the single line ones
    // first, the character literals after them and the crossing ones last, which is the order the
    // plan is built in.
    pub(crate) fn get_string_pair_of(&self, symbol: u8) -> (&str, &str) {
        let (symbols, literals) = (self.strings.get_symbols(), self.strings.get_char_literals());
        let symbol = symbol as usize;
        if let Some(single) = symbols.get(symbol) {
            return (single, single);
        }
        match literals.get(symbol - symbols.len()) {
            Some(literal) => (literal, literal),
            None => {
                let crossing = &self.strings.get_multiline_strings()[symbol - symbols.len() - literals.len()];
                (&crossing.open, &crossing.close)
            }
        }
    }

    pub(crate) fn string_crosses_lines(&self, symbol: u8) -> bool {
        symbol as usize >= self.strings.get_symbols().len() + self.strings.get_char_literals().len()
    }

    // The one place that knows the numbering: a pair kind added later is one match arm here and not
    // seven pieces of arithmetic spread over two files.
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

// Hand written to leave 'scan_plan' out: it is a cache, filled when a language parses its first
// file, so comparing it would make one language stop equalling its own untouched copy.
impl PartialEq for Language {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.extensions == other.extensions
            && self.filenames == other.filenames
            && self.shebangs == other.shebangs
            && self.strings == other.strings
            && self.comment_symbols == other.comment_symbols
            && self.multiline_comments == other.multiline_comments
            && self.nesting_comments == other.nesting_comments
            && self.leveled_comments == other.leveled_comments
            && self.line_continuation == other.line_continuation
            && self.nested_languages == other.nested_languages
            && self.keywords == other.keywords
    }
}

// Plain ends at its first closer, nesting counts its own openers, leveled closes only at an end
// carrying the count its opener did.
pub(crate) enum CommentPair<'a> {
    Plain { start: &'a str, end: &'a str },
    Nesting { start: &'a str, end: &'a str },
    Leveled(&'a LeveledPair)
}

/// The symbols that open a string, and the one byte that cancels the symbol standing after it: the
/// backslash in most languages, the backtick in PowerShell, and nothing in the family that escapes
/// a quote by doubling it, which is what Pascal, Ada, Fortran, COBOL and standard SQL need.
///
/// Escaping is two questions, and the wrong answer to either runs a string past the quote that
/// should have closed it. The byte here says which one escapes; [`MultilineString::escapes`] says
/// whether anything escapes inside one form that crosses lines, and PowerShell answers the two
/// differently per form.
#[derive(Debug, Clone, PartialEq)]
pub struct StringRules {
    escape : Option<u8>,
    symbols : Vec<String>,
    char_literals : Vec<String>,
    multiline : Vec<MultilineString>
}

impl StringRules {
    /// Rules whose strings are escaped by the given byte, usually a backslash.
    pub fn escaping_with(escape: u8) -> StringRules {
        StringRules { escape: Some(escape), symbols: Vec::new(), char_literals: Vec::new(),
                multiline: Vec::new() }
    }

    /// Rules for the languages that escape a quote by doubling it and have no escape byte at all.
    pub fn escaping_nothing() -> StringRules {
        StringRules { escape: None, symbols: Vec::new(), char_literals: Vec::new(), multiline: Vec::new() }
    }

    /// Adds quotes that open a string ending with its own line.
    pub fn with_symbols(mut self, symbols: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.symbols.extend(owned_strings(symbols));
        self
    }

    /// Adds the symbol of a character literal, Rust's and D's `'`. One that does not close on its
    /// own line is not a literal at all, so a lifetime's lone `'` opens nothing while `'"'` still
    /// shields its quote.
    pub fn with_char_literals(mut self, symbols: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.char_literals.extend(owned_strings(symbols));
        self
    }

    /// Adds symbols that open a string running past the end of its line, escapes obeyed inside it.
    pub fn with_multiline_strings(mut self, symbols: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.multiline.extend(symbols.into_iter().map(|x| MultilineString::escaping(x.as_ref())));
        self
    }

    /// The same, for the forms where nothing escapes and only the closing symbol ends the string.
    pub fn with_raw_multiline_strings(mut self, symbols: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.multiline.extend(symbols.into_iter().map(|x| MultilineString::raw(x.as_ref())));
        self
    }

    /// The same again, for the forms opened and closed by different text, such as `r#"` with `"#`.
    pub fn with_string_pairs(mut self, pairs: &[(impl AsRef<str>, impl AsRef<str>)]) -> Self {
        self.multiline.extend(pairs.iter().map(|(open, close)| MultilineString::of(open.as_ref(), close.as_ref())));
        self
    }

    /// The byte that cancels the symbol after it, if this language has one.
    pub fn get_escape(&self) -> Option<u8> {
        self.escape
    }

    /// The quotes whose string ends with its line.
    pub fn get_symbols(&self) -> &[String] {
        &self.symbols
    }

    /// The quotes that open a character literal.
    pub fn get_char_literals(&self) -> &[String] {
        &self.char_literals
    }

    /// The forms whose string may run past the end of its line.
    pub fn get_multiline_strings(&self) -> &[MultilineString] {
        &self.multiline
    }
}

/// A string that crosses lines: the same symbol twice for Python's `"""`, two different ones for a
/// raw form like `r#"` with `"#`.
///
/// Whether it escapes is the one thing the shape cannot answer, since a backtick escapes nothing in
/// Go, Odin and D and does escape in a JavaScript template literal, and `"""` splits the same way
/// between Kotlin and Java. A form written with two different symbols is raw by construction, which
/// is why [`MultilineString::of`] takes no flag.
#[derive(Debug, Clone, PartialEq)]
pub struct MultilineString {
    /// The text that opens it.
    pub open : String,
    /// The text that closes it, the same as the opener for a symmetrical form.
    pub close : String,
    /// Whether the language's escape byte works inside it.
    pub escapes : bool
}

impl MultilineString {
    /// One symbol at both ends, escapes obeyed inside.
    pub fn escaping(symbol: &str) -> MultilineString {
        MultilineString { open: symbol.to_owned(), close: symbol.to_owned(), escapes: true }
    }

    /// One symbol at both ends, nothing escaping inside.
    pub fn raw(symbol: &str) -> MultilineString {
        MultilineString { open: symbol.to_owned(), close: symbol.to_owned(), escapes: false }
    }

    /// Different text at each end, which is raw by construction.
    pub fn of(open: &str, close: &str) -> MultilineString {
        MultilineString { open: open.to_owned(), close: close.to_owned(), escapes: false }
    }
}

/// One section of another language inside a file: everything between the two tags is counted with
/// that language's own symbols.
///
/// Which language it is comes off the opening tag's `lang` or `type` attribute through the
/// extension lookup, and the default answers when the tag names none. The tags match without
/// regard to case, the way HTML reads them.
#[derive(Debug, Clone, PartialEq)]
pub struct NestedLanguage {
    /// The tag that opens the section, `<script`.
    pub start : String,
    /// The tag that closes it, `</script>`.
    pub end : String,
    /// What the section is written in when the tag says nothing. An extension and not a language
    /// name, so that both paths resolve the same way.
    pub default : String
}

impl NestedLanguage {
    /// A section between the two tags, falling back to the named extension.
    pub fn of(start: &str, end: &str, default: &str) -> NestedLanguage {
        NestedLanguage { start: start.to_owned(), end: end.to_owned(), default: default.to_owned() }
    }
}

/// A line ending in this symbol is joined to the one after it, before anything is decided about
/// either.
#[derive(Debug, Clone, PartialEq)]
pub struct LineContinuation {
    /// The text the line has to end in, usually a backslash.
    pub symbol : String,
    /// Whether the join also happens inside a string.
    pub in_strings : bool,
    /// Whether it also happens inside a line comment. This is what separates C, where the join
    /// happens whatever the line held, from JavaScript and Python, where a line comment ends at
    /// the newline whatever follows it.
    pub in_comments : bool
}

/// A long-bracket comment pair, split around the run of `=` that gives it its level: `--[=*[` is
/// the prefix `--[` and the suffix `[`.
#[derive(Debug, Clone, PartialEq)]
pub struct LeveledPair {
    /// The fixed bytes before the run of `=` in the opener.
    pub start_prefix : String,
    /// The single byte that closes the opener after that run.
    pub start_suffix : u8,
    /// The fixed bytes before the run of `=` in the closer.
    pub end_prefix : String,
    /// The single byte that closes the closer after that run.
    pub end_suffix : u8,
}

impl LeveledPair {
    /// Splits both halves around their `=*`, and answers `None` if either is not written that way.
    pub fn of(start: &str, end: &str) -> Option<LeveledPair> {
        let (start_prefix, start_suffix) = split_leveled_half(start)?;
        let (end_prefix, end_suffix) = split_leveled_half(end)?;
        Some(LeveledPair { start_prefix, start_suffix, end_prefix, end_suffix })
    }
}

/// A word worth counting occurrences of, under a name of its own.
#[derive(Debug,PartialEq,Eq,Clone)]
pub struct Keyword {
    /// What the report calls the count, `classes`.
    pub descriptive_name : String,
    /// The spellings that count towards it, so `classes` can be raised by both `class` and
    /// `record`.
    pub aliases : Vec<String>
}

impl Keyword {
    /// A count under the given name, raised by any of the given spellings.
    pub fn new(descriptive_name: impl AsRef<str>, aliases: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        Keyword {
            descriptive_name : descriptive_name.as_ref().to_owned(),
            aliases : owned_strings(aliases)
        }
    }
}

/// Where each line of a file landed, one slot per line, so the nine always add up to the lines.
///
/// This is what the counting produces. What a report's code and comment columns show is these nine
/// folded into three, which is [`CountingModel`] and is chosen when the figures are read, not when
/// they are taken.
///
/// "Words" means word bytes: a letter, a digit, or anything above ASCII. A line with none anywhere
/// is punctuation some grammar required, or blank.
#[derive(Debug,PartialEq,Default,Clone)]
pub struct LineClasses {
    /// Lines carrying words outside any string or comment.
    pub words_in_code : usize,
    /// Lines whose only content is the inside of a string literal, the middle of a string running
    /// over several lines being the plain case. String content is data, and both models count it
    /// as code.
    pub string_content : usize,
    /// Words only inside a comment, on a line that also carries code punctuation: `} // words`.
    /// The two models part here, which is why it is a class of its own: content reads the words
    /// and says comment, region reads the `}` and says code.
    pub comment_words_beside_code : usize,
    /// Lines whose words are all inside a comment, with no code on them at all.
    pub words_in_comment : usize,
    /// Lines of code with no words on them, a lone `});`.
    pub punctuation_in_code : usize,
    /// The same inside a comment, a line of a drawn box or a row of dashes.
    pub punctuation_in_comment : usize,
    /// Empty lines outside everything.
    pub blank : usize,
    /// Empty lines inside a block comment.
    pub blank_in_comment : usize,
    /// Empty lines inside a string that runs over several lines.
    pub blank_in_string : usize
}

impl LineClasses {
    /// The nine names, in the order [`LineClasses::to_array`] and [`LineClasses::of_array`] use.
    pub const NAMES : [&'static str; 9] = ["words_in_code", "string_content",
            "comment_words_beside_code", "words_in_comment", "punctuation_in_code",
            "punctuation_in_comment", "blank", "blank_in_comment", "blank_in_string"];

    /// The nine counts in the order of [`LineClasses::NAMES`].
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

    /// The same nine, back out in that order.
    pub fn to_array(&self) -> [usize; 9] {
        [self.words_in_code, self.string_content, self.comment_words_beside_code,
         self.words_in_comment, self.punctuation_in_code, self.punctuation_in_comment,
         self.blank, self.blank_in_comment, self.blank_in_string]
    }

    /// How many lines were sorted into all nine.
    pub fn calculate_lines(&self) -> usize {
        self.to_array().iter().sum()
    }

    /// Counts one more line of that class.
    pub fn bump(&mut self, class: LineClass) {
        match class {
            LineClass::WordsInCode => self.words_in_code += 1,
            LineClass::StringContent => self.string_content += 1,
            LineClass::CommentWordsBesideCode => self.comment_words_beside_code += 1,
            LineClass::WordsInComment => self.words_in_comment += 1,
            LineClass::PunctuationInCode => self.punctuation_in_code += 1,
            LineClass::PunctuationInComment => self.punctuation_in_comment += 1,
            LineClass::Blank => self.blank += 1,
            LineClass::BlankInComment => self.blank_in_comment += 1,
            LineClass::BlankInString => self.blank_in_string += 1
        }
    }

    pub(crate) fn add(&mut self, other: &LineClasses) {
        *self = combine_classes(self, other, |mine, theirs| mine + theirs);
    }

    /// Takes one set of counts off another, class by class.
    ///
    /// Saturating, because the numbers taken out may come off a document or a log, where nothing
    /// promises they stay inside what they are taken from.
    pub fn subtract(&mut self, other: &LineClasses) {
        *self = combine_classes(self, other, usize::saturating_sub);
    }
}

/// Which of the nine a single line was sorted into. One variant per field of [`LineClasses`],
/// named after it and described there.
#[allow(missing_docs)]
#[derive(Debug,PartialEq,Eq,Clone,Copy)]
pub enum LineClass {
    WordsInCode,
    StringContent,
    CommentWordsBesideCode,
    WordsInComment,
    PunctuationInCode,
    PunctuationInComment,
    Blank,
    BlankInComment,
    BlankInString
}

impl LineClass {
    /// All nine, in the order of [`LineClasses::NAMES`].
    pub const ALL : [LineClass; 9] = [LineClass::WordsInCode, LineClass::StringContent,
            LineClass::CommentWordsBesideCode, LineClass::WordsInComment, LineClass::PunctuationInCode,
            LineClass::PunctuationInComment, LineClass::Blank, LineClass::BlankInComment,
            LineClass::BlankInString];

    /// The name of the [`LineClasses`] field this class counts into.
    pub fn name(self) -> &'static str {
        LineClasses::NAMES[self as usize]
    }
}

/// One of the three columns a report shows, which is what a [`CountingModel`] folds the nine
/// classes into.
#[derive(Debug,PartialEq,Eq,Clone,Copy)]
pub enum Bucket {
    /// The code column.
    Code,
    /// The comments column.
    Comments,
    /// Whatever the model counts as neither, whose name it decides:
    /// [`CountingModel::get_third_quantity_name`].
    Third
}

/// One stretch of a line, as [`crate::explain_file`] reports it: which bytes sit inside a string,
/// which inside a comment, and which outside both.
///
/// The symbols that open and close a thing belong to its own stretch.
#[derive(Debug,PartialEq,Eq,Clone,Copy)]
pub struct Span {
    /// Byte offset into the line as the file spells it, the stretch's first byte.
    pub from: usize,
    /// Byte offset one past its last.
    pub to: usize,
    /// What the bytes between them are.
    pub kind: SpanKind
}

/// What a [`Span`] holds.
#[derive(Debug,PartialEq,Eq,Clone,Copy)]
pub enum SpanKind {
    /// Outside any string or comment.
    Code,
    /// Inside a string literal, its quotes included.
    String,
    /// Inside a comment, its symbols included.
    Comment
}

impl SpanKind {
    /// `code`, `string` or `comment`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::String => "string",
            Self::Comment => "comment"
        }
    }
}

/// Where the code and comment columns come from.
///
/// The counting only ever fills a [`LineClasses`]. What a column shows is this fold of those nine
/// into three, chosen when the figures are read, so one run answers both models and switching
/// costs no recounting.
#[derive(Debug,PartialEq,Eq,Clone,Copy,Default)]
pub enum CountingModel {
    /// What a line says: words in code make it code, words only in a comment make it a comment,
    /// and punctuation and blank lines are neither. The third column is called `extra`.
    #[default]
    Content,
    /// Where a line sits, which is how cloc, tokei and scc count: any code on the line makes it
    /// code, a line inside a comment belongs to the comment whatever it holds, and only a blank
    /// outside everything is blank. The third column is called `blanks`.
    Region
}

impl CountingModel {
    /// Reads `content` or `region`, trimmed and in any case.
    pub fn parse(value: &str) -> Option<CountingModel> {
        match value.trim().to_lowercase().as_str() {
            "content" => Some(Self::Content),
            "region" => Some(Self::Region),
            _ => None
        }
    }

    /// The spelling [`CountingModel::parse`] reads back.
    pub fn name(self) -> &'static str {
        match self {
            Self::Content => "content",
            Self::Region => "region"
        }
    }

    /// The other one of the two.
    pub fn get_other(self) -> CountingModel {
        match self {
            Self::Content => Self::Region,
            Self::Region => Self::Content
        }
    }

    /// `extra` under [`CountingModel::Content`], `blanks` under [`CountingModel::Region`].
    pub fn get_third_quantity_name(self) -> &'static str {
        match self {
            Self::Content => "extra",
            Self::Region => "blanks"
        }
    }

    /// What this model heads that column with.
    pub fn get_bucket_name(self, bucket: Bucket) -> &'static str {
        match bucket {
            Bucket::Code => "code",
            Bucket::Comments => "comments",
            Bucket::Third => self.get_third_quantity_name()
        }
    }

    /// Which column a line of that class lands in.
    ///
    /// The column sums below and the per-line answer of [`crate::explain_file`] both go through
    /// this, so the two cannot disagree.
    pub fn fold(self, class: LineClass) -> Bucket {
        match self {
            Self::Content => match class {
                LineClass::WordsInCode | LineClass::StringContent => Bucket::Code,
                LineClass::CommentWordsBesideCode | LineClass::WordsInComment => Bucket::Comments,
                LineClass::PunctuationInCode | LineClass::PunctuationInComment | LineClass::Blank
                | LineClass::BlankInComment | LineClass::BlankInString => Bucket::Third
            },
            Self::Region => match class {
                LineClass::WordsInCode | LineClass::StringContent | LineClass::CommentWordsBesideCode
                | LineClass::PunctuationInCode | LineClass::BlankInString => Bucket::Code,
                LineClass::WordsInComment | LineClass::PunctuationInComment
                | LineClass::BlankInComment => Bucket::Comments,
                LineClass::Blank => Bucket::Third
            }
        }
    }

    /// Adds up every class this model calls code.
    pub fn calculate_code_lines(self, classes: &LineClasses) -> usize {
        self.sum_the_classes_folding_to(Bucket::Code, classes)
    }

    /// Adds up every class this model calls a comment.
    pub fn calculate_comment_lines(self, classes: &LineClasses) -> usize {
        self.sum_the_classes_folding_to(Bucket::Comments, classes)
    }

    fn sum_the_classes_folding_to(self, bucket: Bucket, classes: &LineClasses) -> usize {
        LineClass::ALL.iter().zip(classes.to_array())
                .filter(|(class, _)| self.fold(**class) == bucket)
                .map(|(_, count)| count).sum()
    }
}

/// What was counted, for one language, one module or a whole run.
///
/// The code and comment columns and the average size are methods rather than fields, so a stored
/// copy cannot drift from the numbers it came from.
#[derive(Debug,PartialEq,Default,Clone)]
pub struct Stats {
    /// How many files went into these figures.
    pub files : usize,
    /// Their size on disk, in bytes.
    pub bytes : usize,
    /// Every line of them, which is also what the nine classes add up to.
    pub lines : usize,
    /// Where each of those lines landed.
    pub classes : LineClasses,
    /// How often each keyword was found, under the name its language gave the count. Added up over
    /// a run these are every keyword every language declared, which is what answers "how many
    /// classes are in this project" across the several languages that have such a thing.
    pub keyword_occurences : HashMap<String,usize>
}

impl Stats {
    /// Figures the caller already has, from a log file or a document.
    pub fn new(files: usize, bytes: usize, lines: usize, classes: LineClasses,
            keyword_occurences: HashMap<String,usize>) -> Self
    {
        Stats { files, bytes, lines, classes, keyword_occurences }
    }

    /// The code column under that model.
    pub fn calculate_code_lines(&self, model: CountingModel) -> usize {
        model.calculate_code_lines(&self.classes)
    }

    /// The comments column under that model.
    pub fn calculate_comment_lines(&self, model: CountingModel) -> usize {
        model.calculate_comment_lines(&self.classes)
    }

    /// The third column: `extra` under [`CountingModel::Content`], `blanks` under
    /// [`CountingModel::Region`].
    ///
    /// Saturating, because the fields are public and these figures are also built from numbers read
    /// off a log file: counts that do not add up are the caller's arithmetic, not a reason to panic.
    pub fn calculate_extra_lines(&self, model: CountingModel) -> usize {
        self.lines.saturating_sub(self.calculate_code_lines(model))
                .saturating_sub(self.calculate_comment_lines(model))
    }

    pub(crate) fn add_file(&mut self, stats: &FileStats, bytes: usize, keywords: &[Keyword]) {
        // The walk counts a line and then sorts it into exactly one class, so the two have to
        // agree, and this is the only door a counted file comes through. Not on 'add' below, which
        // also takes counts parsed out of a document, where nothing promises anything.
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

    /// Adds another set of figures into this one, keyword counts included.
    pub fn add(&mut self, other: &Stats) {
        self.files += other.files;
        self.bytes += other.bytes;
        self.lines += other.lines;
        self.classes.add(&other.classes);
        for (keyword, occurrences) in other.keyword_occurences.iter() {
            *self.keyword_occurences.entry(keyword.clone()).or_default() += *occurrences;
        }
    }

    /// Every language added together, which is what the last row of a report holds.
    pub fn total_of(languages: &HashMap<String, Stats>) -> Self {
        let mut total = Stats::default();
        for stats in languages.values() {
            total.add(stats);
        }
        total
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

// What one file came to, on its way into a 'Stats'. Kept apart from it because the parser
// identifies a keyword by its position in the language's list, which is what the matcher hands
// back, so counting into a vector slot costs no hashing and no string copying in the innermost loop
// of the parse. The names are attached once per file, in 'add_file'.
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

// The half before '=*' and the one byte after it; anything else is not a leveled symbol
fn split_leveled_half(pattern: &str) -> Option<(String, u8)> {
    let (prefix, rest) = pattern.split_once("=*")?;
    if prefix.is_empty() || rest.len() != 1 || rest.contains("=*") {
        return None;
    }
    Some((prefix.to_owned(), rest.as_bytes()[0]))
}

fn combine_classes(one: &LineClasses, other: &LineClasses,
    of_each: impl Fn(usize, usize) -> usize) -> LineClasses
{
    let (mine, theirs) = (one.to_array(), other.to_array());
    LineClasses::of_array(std::array::from_fn(|at| of_each(mine[at], theirs[at])))
}

fn create_keyword_slots(language: &Language) -> HashMap<String,usize> {
    language.keywords.iter().map(|keyword| (keyword.descriptive_name.clone(), 0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats_of(lines: usize, code: usize, comments: usize) -> Stats {
        let classes = LineClasses { words_in_code: code, words_in_comment: comments, ..Default::default() };
        Stats::new(1, 0, lines, classes, HashMap::new())
    }

    // The parser cannot produce these; they come off a log file whose head was lost.
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

    // 'get_name' indexes NAMES by the variant's own discriminant, so this is not the names being
    // right, it is 'ALL' listing the variants in the order the names are written in.
    #[test]
    fn every_class_is_listed_in_the_order_its_names_are_written_in() {
        for (i, class) in LineClass::ALL.iter().enumerate() {
            assert_eq!(LineClasses::NAMES[i], class.name());
        }
    }

    #[test]
    fn a_model_names_its_buckets() {
        assert_eq!("code", CountingModel::Content.get_bucket_name(Bucket::Code));
        assert_eq!("comments", CountingModel::Region.get_bucket_name(Bucket::Comments));
        assert_eq!("extra", CountingModel::Content.get_bucket_name(Bucket::Third));
        assert_eq!("blanks", CountingModel::Region.get_bucket_name(Bucket::Third));
    }

    // Every line ends in an empty symbol, so a language carrying one joins its whole file into one
    // line and reports counts that look ordinary.
    #[test]
    fn an_empty_continuation_symbol_leaves_the_language_without_one() {
        let language = |symbol| Language::new("L", ["l"], StringRules::escaping_nothing(), [""], &[], [])
                .with_line_continuation(symbol, true, true);

        assert_eq!(None, language("").line_continuation);
        assert_eq!(Some("\\".to_owned()), language("\\").line_continuation.map(|x| x.symbol));
    }

}
