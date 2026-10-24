// Which lines of a file are test code. The markers are found once over the whole buffer before the
// line loop. Each one's extent is followed inside the loop over the code ranges of every line, so a
// brace inside a string or a comment is absent by construction.
use memchr::memmem;

use crate::{Language, LineClass};
use crate::engine::file_parser::BYTE_ORDER_MARK;

use super::{TestsByPath, attribute, begins_with, find_next_brace, find_word_end, is_word_byte, is_word_start, skip_whitespace};

const ELSE : &[u8] = b"else";
const IF : &[u8] = b"if";

pub(crate) struct TestWalk<'a> {
    contents: &'a [u8],
    markers: &'a [String],
    file_markers: &'a [String],
    // The index counts the markers first and the file markers after them
    candidates: Vec<(usize, u16)>,
    next: usize,
    state: State,
    // The offset the scan of the open extent resumes from, which can sit past the current line
    cursor: usize,
    held: Vec<(LineClass, usize)>,
    // The test code of the line last observed, as offsets into the file. One still open at the end of
    // the line runs to the largest offset.
    test_spans: Vec<(usize, usize)>,
    // An 'if' under an attribute owns its 'else' arms. After a word such as D's 'unittest', or
    // after a cfg_if's 'if #[cfg(test)]', the 'else' is the branch built without it.
    else_continues: bool,
    // The offset and the index of the first file marker found in code
    file_marker: Option<(usize, usize)>,
    whole_file_attribute: Option<(usize, usize)>,
    looks_ahead: bool,
    next_brace: usize,
}

impl<'a> TestWalk<'a> {
    pub(crate) fn of(language: &'a Language, contents: &'a str, enabled: bool, by_path: TestsByPath) -> Option<TestWalk<'a>> {
        if !enabled || by_path == TestsByPath::WholeFile { return None; }
        let markers = language.test_markers.as_slice();
        let file_markers = match by_path {
            TestsByPath::TakenBack => &[][..],
            _ => language.test_file_markers.as_slice()
        };
        if markers.is_empty() && file_markers.is_empty() { return None; }

        let bytes = contents.as_bytes();
        let mut candidates = Vec::new();
        find_candidates(bytes, markers, 0, &mut candidates);
        find_candidates(bytes, file_markers, markers.len(), &mut candidates);
        if candidates.is_empty() { return None; }
        candidates.sort_unstable();

        let mut walk = TestWalk { contents: bytes, markers, file_markers, candidates: Vec::new(), next: 0, state: State::Idle,
                cursor: 0, held: Vec::new(), test_spans: Vec::new(), else_continues: false, file_marker: None,
                whole_file_attribute: None, looks_ahead: language.nested_languages.is_empty(), next_brace: 0 };
        candidates.retain(|&(at, index)| walk.could_be_counted(at, index as usize));
        walk.candidates = candidates;
        Some(walk)
    }

    fn could_be_counted(&self, at: usize, index: usize) -> bool {
        match index.checked_sub(self.markers.len()) {
            // A file marker names an annotation, an attribute or a type, so a member access after it,
            // '[Fact.Create(a)]' in a collection expression, is none
            Some(in_file_markers) => {
                let width = self.file_markers[in_file_markers].len();
                self.stands_as_a_word(at, width) && self.contents.get(at + width) != Some(&b'.')
            },
            None if attribute::is_opener(&self.markers[index]) => attribute::read(self.contents, at).is_some(),
            None => self.stands_as_a_word(at, self.markers[index].len())
        }
    }

    // Whether the line just classified is test code. The ranges are offsets into the line with its
    // whitespace trimmed at both ends, and they are read only where the line held code. A yes may
    // also claim the lines held back since a closing brace, which 'take_held' hands out.
    pub(crate) fn observe_line(&mut self, line_start: usize, raw_line: &str, has_code: bool, lead: usize,
        ranges: &[(usize, usize)], class: LineClass, bytes: usize) -> bool
    {
        self.test_spans.clear();
        let line_end = line_start + raw_line.len();
        if self.state == State::Idle {
            while self.candidates.get(self.next).is_some_and(|(at, _)| *at < line_start) {
                self.next += 1;
            }
            if self.candidates.get(self.next).is_none_or(|(at, _)| *at >= line_end) {
                return false;
            }
        }

        let base = line_start + lead;
        let (line, ranges): (&[u8], &[(usize, usize)]) = if has_code { (&raw_line.as_bytes()[lead..], ranges) } else { (&[], &[]) };

        let mut is_test = matches!(self.state, State::Seeking { .. } | State::Inside { .. });
        if is_test { self.test_spans.push((line_start, usize::MAX)); }
        loop {
            match self.state {
                State::Idle => {
                    let floor = self.cursor.max(base);
                    while self.candidates.get(self.next).is_some_and(|(at, _)| *at < floor) {
                        self.next += 1;
                    }
                    let Some(&(at, index)) = self.candidates.get(self.next) else { return is_test };
                    if at >= line_end { return is_test; }
                    self.next += 1;
                    let index = index as usize;
                    if let Some(in_file_markers) = index.checked_sub(self.markers.len()) {
                        if self.file_marker.is_none() && self.is_file_marker_at(at, &self.file_markers[in_file_markers], base, ranges) {
                            self.file_marker = Some((at, in_file_markers));
                        }
                        continue;
                    }
                    let Some(found) = self.read_marker(at, &self.markers[index], base, ranges) else { continue };
                    is_test = true;
                    self.test_spans.push((at, usize::MAX));
                    let under_an_attribute = attribute::is_opener(&self.markers[index]);
                    self.else_continues = under_an_attribute;
                    match found {
                        // A depth of one is the block around the marker, so a file with no such
                        // block runs to its end
                        Marker::RestOfScope { after } => {
                            if under_an_attribute && is_before_every_item(self.contents, at) {
                                self.whole_file_attribute = Some((at, after));
                            }
                            self.cursor = after;
                            self.state = State::Inside { depth: 1 };
                        },
                        Marker::Extent { after } => {
                            self.cursor = after;
                            let ends = if under_an_attribute {Terminator::Undecided} else {Terminator::BraceOrSemicolon};
                            self.state = State::Seeking { depth: 0, ends };
                        }
                    }
                },
                State::Seeking { .. } => if !self.seek_opener(base, line, ranges) { return is_test; },
                State::Inside { .. } => if !self.seek_closer(base, line, ranges, line_end) { return is_test; },
                State::AfterClose => match self.look_past_closer(base, line, ranges) {
                    Look::Exhausted => {
                        if !is_test { self.held.push((class, bytes)); }
                        return is_test;
                    },
                    Look::Else => {
                        is_test = true;
                        self.test_spans.push((self.cursor - ELSE.len(), usize::MAX));
                        self.state = State::Seeking { depth: 0, ends: Terminator::BraceOrSemicolon };
                    },
                    Look::Other => {
                        self.held.clear();
                        self.state = State::Idle;
                    }
                }
            }
        }
    }

    #[inline(always)]
    pub(crate) fn quiet_until(&self) -> usize {
        match self.state {
            State::Idle => self.candidates.get(self.next).map_or(usize::MAX, |(at, _)| *at),
            State::Inside { .. } => self.next_brace,
            _ => 0
        }
    }

    #[inline(always)]
    pub(crate) fn skips_lines_inside(&self) -> bool {
        self.looks_ahead && matches!(self.state, State::Inside { .. })
    }

    pub(crate) fn take_held(&mut self) -> std::vec::Drain<'_, (LineClass, usize)> {
        self.held.drain(..)
    }

    pub(crate) fn get_file_marker(&self) -> Option<(usize, usize)> {
        self.file_marker
    }

    pub(crate) fn get_whole_file_attribute(&self) -> Option<(usize, usize)> {
        self.whole_file_attribute
    }

    pub(crate) fn get_test_spans(&self) -> &[(usize, usize)] {
        &self.test_spans
    }

    fn read_marker(&self, at: usize, marker: &str, base: usize, ranges: &[(usize, usize)]) -> Option<Marker> {
        if !self.lies_in_code(at, marker.len(), base, ranges) { return None; }
        if attribute::is_opener(marker) {
            return attribute::read(self.contents, at);
        }
        let end = at + marker.len();
        Some(match find_scope_colon(self.contents, end) {
            Some(past_colon) => Marker::RestOfScope { after: past_colon },
            None => Marker::Extent { after: end }
        })
    }

    fn is_file_marker_at(&self, at: usize, marker: &str, base: usize, ranges: &[(usize, usize)]) -> bool {
        self.lies_in_code(at, marker.len(), base, ranges)
                && (!marker.starts_with('[') || self.has_only_attributes_in_front(at, base, ranges))
    }

    fn lies_in_code(&self, at: usize, width: usize, base: usize, ranges: &[(usize, usize)]) -> bool {
        let in_line = at - base;
        ranges.iter().any(|&(from, to)| from <= in_line && in_line + width <= to)
    }

    // The last byte of a byte order mark is above ASCII, which would read as a letter glued to a
    // marker opening the file
    fn stands_as_a_word(&self, at: usize, width: usize) -> bool {
        let opens_the_file = at == 0 || (at == BYTE_ORDER_MARK.len() && self.contents.starts_with(BYTE_ORDER_MARK));
        let before = (!opens_the_file).then(|| self.contents[at - 1]);
        let after = self.contents.get(at + width).copied();
        !before.is_some_and(is_word_byte) && !after.is_some_and(is_word_byte)
    }

    fn has_only_attributes_in_front(&self, at: usize, base: usize, ranges: &[(usize, usize)]) -> bool {
        let skipped = if base == 0 && self.contents.starts_with(BYTE_ORDER_MARK) { BYTE_ORDER_MARK.len() } else { 0 };
        let mut depth = 0u32;
        for &(from, to) in ranges {
            let (from, to) = (from.max(skipped), to.min(at - base));
            if from >= to { continue; }
            for &byte in &self.contents[base + from..base + to] {
                match byte {
                    b'[' => depth += 1,
                    b']' if depth > 0 => depth -= 1,
                    _ if depth == 0 && !byte.is_ascii_whitespace() => return false,
                    _ => ()
                }
            }
        }
        depth == 0
    }

    // Every bracket moves the depth, and an opener or a terminator counts only at depth zero. The
    // words met at depth zero decide which of the two ends the item. A closing brace at depth zero
    // is the block around the marked thing ending, so a marker on a field, a variant or an arm
    // stops there.
    fn seek_opener(&mut self, base: usize, line: &[u8], ranges: &[(usize, usize)]) -> bool {
        let State::Seeking { mut depth, mut ends } = self.state else { return false };
        for &(from, to) in ranges {
            let mut at = self.cursor.saturating_sub(base).max(from);
            while at < to {
                let byte = line[at];
                match byte {
                    b'(' | b'[' => depth += 1,
                    b')' | b']' => depth = depth.saturating_sub(1),
                    b'}' => {
                        if depth == 0 {
                            self.state = State::Idle;
                            self.cursor = base + at + 1;
                            self.end_test_span();
                            return true;
                        }
                        depth -= 1;
                    },
                    b'{' => {
                        if depth == 0 && ends != Terminator::Semicolon {
                            if ends == Terminator::Undecided { self.else_continues = false; }
                            self.state = State::Inside { depth: 1 };
                            self.cursor = base + at + 1;
                            return true;
                        }
                        depth += 1;
                    },
                    b';' if depth == 0 => {
                        self.state = State::Idle;
                        self.cursor = base + at + 1;
                        self.end_test_span();
                        return true;
                    },
                    _ if depth == 0 && ends.is_open() && is_word_start(byte) => {
                        let end = find_word_end(line, at);
                        if ends == Terminator::Undecided { self.else_continues &= &line[at..end] == IF; }
                        ends = ends.read_word(&line[at..end]);
                        at = end;
                        continue;
                    },
                    _ => ()
                }
                at += 1;
            }
        }
        self.state = State::Seeking { depth, ends };
        false
    }

    fn seek_closer(&mut self, base: usize, line: &[u8], ranges: &[(usize, usize)], line_end: usize) -> bool {
        let State::Inside { mut depth } = self.state else { return false };
        for &(from, to) in ranges {
            let start = self.cursor.saturating_sub(base).max(from);
            if start >= to { continue; }
            for at in memchr::memchr2_iter(b'{', b'}', &line[start..to]) {
                if line[start + at] == b'{' {
                    depth += 1;
                    continue;
                }
                depth -= 1;
                if depth == 0 {
                    self.state = State::AfterClose;
                    self.cursor = base + start + at + 1;
                    self.end_test_span();
                    return true;
                }
            }
        }
        self.state = State::Inside { depth };
        if self.looks_ahead {
            self.next_brace = find_next_brace(self.contents, line_end.max(self.cursor));
        }
        false
    }

    fn look_past_closer(&mut self, base: usize, line: &[u8], ranges: &[(usize, usize)]) -> Look {
        for &(from, to) in ranges {
            let at = skip_whitespace(&line[..to], self.cursor.saturating_sub(base).max(from));
            if at >= to { continue; }
            if self.else_continues && line[at..to].starts_with(ELSE)
                    && !line.get(at + ELSE.len()).is_some_and(|byte| is_word_byte(*byte)) {
                self.cursor = base + at + ELSE.len();
                return Look::Else;
            }
            self.cursor = base + at;
            return Look::Other;
        }
        Look::Exhausted
    }

    fn end_test_span(&mut self) {
        if let Some(span) = self.test_spans.last_mut() { span.1 = self.cursor; }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Marker {
    RestOfScope { after: usize },
    Extent { after: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Seeking { depth: u32, ends: Terminator },
    Inside { depth: u32 },
    AfterClose,
}

// What ends the item under an attribute. A binding, an import, a type alias, a static, a const and
// an extern crate end at the next semicolon at depth zero and their braces only nest. Everything
// else, and every item under another marker, ends at whichever of brace and semicolon comes first.
// A const fn and an extern fn are the two that need a second word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Terminator {
    Undecided,
    AfterConst,
    AfterExtern,
    Semicolon,
    BraceOrSemicolon,
}

impl Terminator {
    fn is_open(self) -> bool {
        matches!(self, Terminator::Undecided | Terminator::AfterConst | Terminator::AfterExtern)
    }

    fn read_word(self, word: &[u8]) -> Terminator {
        match (self, word) {
            (Terminator::Undecided, b"pub" | b"unsafe" | b"async" | b"default") => Terminator::Undecided,
            (Terminator::Undecided, b"let" | b"use" | b"type" | b"static") => Terminator::Semicolon,
            (Terminator::Undecided, b"const") => Terminator::AfterConst,
            (Terminator::Undecided, b"extern") => Terminator::AfterExtern,
            (Terminator::AfterConst, b"unsafe" | b"async" | b"extern") => Terminator::AfterConst,
            (Terminator::AfterConst, b"fn") => Terminator::BraceOrSemicolon,
            (Terminator::AfterConst, _) => Terminator::Semicolon,
            (Terminator::AfterExtern, b"crate") => Terminator::Semicolon,
            _ => Terminator::BraceOrSemicolon
        }
    }
}

enum Look {
    Exhausted,
    Else,
    Other,
}

// The caller sorts, since the walk reads the candidates in file order
fn find_candidates(bytes: &[u8], markers: &[String], first_index: usize, into: &mut Vec<(usize, u16)>) {
    for (index, marker) in markers.iter().enumerate() {
        let Some(&first) = marker.as_bytes().first() else { continue };
        let shares_the_first_byte = |other: &String| other.as_bytes().first() == Some(&first);
        if markers[..index].iter().any(shares_the_first_byte) { continue; }
        if markers[index + 1..].iter().any(shares_the_first_byte) {
            let mut second_bytes = [false; 256];
            for candidate in markers.iter().filter(|candidate| shares_the_first_byte(candidate)) {
                match candidate.as_bytes().get(1) {
                    Some(&second) => second_bytes[second as usize] = true,
                    None => second_bytes = [true; 256]
                }
            }
            for at in memchr::memchr_iter(first, bytes) {
                if bytes.get(at + 1).is_some_and(|second| !second_bytes[*second as usize]) { continue; }
                for (other, candidate) in markers.iter().enumerate() {
                    if shares_the_first_byte(candidate) && begins_with(&bytes[at..], candidate.as_bytes()) {
                        into.push((at, (first_index + other) as u16));
                    }
                }
            }
        } else {
            into.extend(memmem::find_iter(bytes, marker.as_bytes()).map(|at| (at, (first_index + index) as u16)));
        }
    }
}

// An inner attribute is allowed before the items of a module alone, so one with nothing but other
// inner attributes, comments and a shebang in front of it covers the whole file
fn is_before_every_item(bytes: &[u8], at: usize) -> bool {
    let mut pos = if bytes.starts_with(BYTE_ORDER_MARK) { BYTE_ORDER_MARK.len() } else { 0 };
    if bytes[pos..].starts_with(b"#!") && !bytes[pos..].starts_with(b"#![") {
        pos = memchr::memchr(b'\n', &bytes[pos..]).map_or(bytes.len(), |end| pos + end);
    }
    loop {
        pos = attribute::skip_whitespace_and_comments(bytes, pos);
        if pos >= at { return pos == at; }
        if !bytes[pos..].starts_with(b"#![") { return false; }
        let Some(close) = attribute::find_matching_bracket(bytes, pos + 2, b']') else { return false };
        pos = close + 1;
    }
}

// D's 'version(unittest)' with a colon after it, which covers the rest of its scope
fn find_scope_colon(bytes: &[u8], from: usize) -> Option<usize> {
    let at = skip_whitespace(bytes, from);
    if bytes.get(at) != Some(&b')') { return None; }
    let at = skip_whitespace(bytes, at + 1);
    (bytes.get(at) == Some(&b':')).then_some(at + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_after_the_attribute_decide_what_ends_the_item() {
        let decide = |words: &[&[u8]]| words.iter().fold(Terminator::Undecided, |ends, word| ends.read_word(word));
        assert_eq!(decide(&[b"fn"]), Terminator::BraceOrSemicolon);
        assert_eq!(decide(&[b"pub", b"fn"]), Terminator::BraceOrSemicolon);
        assert_eq!(decide(&[b"mod"]), Terminator::BraceOrSemicolon);
        assert_eq!(decide(&[b"if"]), Terminator::BraceOrSemicolon);
        assert_eq!(decide(&[b"let"]), Terminator::Semicolon);
        assert_eq!(decide(&[b"pub", b"use"]), Terminator::Semicolon);
        assert_eq!(decide(&[b"static"]), Terminator::Semicolon);
        assert_eq!(decide(&[b"type"]), Terminator::Semicolon);
        assert_eq!(decide(&[b"const", b"X"]), Terminator::Semicolon);
        assert_eq!(decide(&[b"pub", b"const", b"fn"]), Terminator::BraceOrSemicolon);
        assert_eq!(decide(&[b"const", b"unsafe", b"fn"]), Terminator::BraceOrSemicolon);
        assert_eq!(decide(&[b"extern", b"crate"]), Terminator::Semicolon);
        assert_eq!(decide(&[b"extern", b"fn"]), Terminator::BraceOrSemicolon);
        assert_eq!(decide(&[b"unsafe"]), Terminator::Undecided);
        assert!(!decide(&[b"let"]).is_open());
    }

    #[test]
    fn a_word_marker_stands_as_its_own_word() {
        let language = Language::new("D", ["d"], crate::StringRules::escaping_nothing(), ["//"], &[], [])
                .with_test_markers(["unittest"]);
        let source = "unittests { } version(unittest) { } my_unittest";
        let walk = TestWalk::of(&language, source, true, TestsByPath::Nothing).unwrap();
        let ranges = [(0, source.len())];
        assert!(!walk.could_be_counted(0, 0));
        assert!(matches!(walk.read_marker(22, "unittest", 0, &ranges), Some(Marker::Extent { after: 30 })));
        assert!(!walk.could_be_counted(39, 0));
        assert!(walk.read_marker(22, "unittest", 0, &[(0, 10)]).is_none());

        let read_first = |source: &str| {
            let walk = TestWalk::of(&language, source, true, TestsByPath::Nothing).unwrap();
            walk.could_be_counted(3, 0).then(|| walk.read_marker(3, "unittest", 0, &[(0, source.len())])).flatten()
        };
        assert_eq!(Some(Marker::Extent { after: 11 }), read_first("\u{feff}unittest { }"), "a byte order mark hid the marker");
        assert_eq!(None, read_first("€unittest { }"));
    }

    #[test]
    fn a_word_marker_closed_by_a_paren_and_a_colon_covers_the_rest_of_its_scope() {
        let language = Language::new("D", ["d"], crate::StringRules::escaping_nothing(), ["//"], &[], [])
                .with_test_markers(["unittest"]);
        let source = "version(unittest) : int a; version(unittest) { }";
        let walk = TestWalk::of(&language, source, true, TestsByPath::Nothing).unwrap();
        let ranges = [(0, source.len())];
        assert_eq!(walk.read_marker(8, "unittest", 0, &ranges), Some(Marker::RestOfScope { after: 19 }));
        assert_eq!(walk.read_marker(35, "unittest", 0, &ranges), Some(Marker::Extent { after: 43 }));
    }

    #[test]
    fn markers_sharing_a_first_byte_are_found_at_the_same_places_as_markers_searched_alone() {
        let source = "#[test] x #![cfg(test)] #[ unittest #[ #![ u";
        let owned = |markers: &[&str]| markers.iter().map(|marker| (*marker).to_owned()).collect::<Vec<_>>();
        let one_by_one = |markers: &[String]| {
            let mut found = markers.iter().enumerate()
                    .flat_map(|(index, marker)| memmem::find_iter(source.as_bytes(), marker.as_bytes()).map(move |at| (at, index as u16)))
                    .collect::<Vec<_>>();
            found.sort_unstable();
            found
        };
        let grouped = |markers: &[String], first_index: usize| {
            let mut found = Vec::new();
            find_candidates(source.as_bytes(), markers, first_index, &mut found);
            found.sort_unstable();
            found
        };
        for markers in [owned(&["#[", "#!["]), owned(&["#[", "unittest", "#!["]), owned(&["unittest"]), owned(&["#![", "#["])] {
            assert_eq!(one_by_one(&markers), grouped(&markers, 0), "{markers:?}");
        }
        assert_eq!(vec![(0, 0), (10, 1), (24, 0), (36, 0), (39, 1)], grouped(&owned(&["#[", "#!["]), 0));
        assert_eq!(vec![(0, 3), (10, 4), (24, 3), (36, 3), (39, 4)], grouped(&owned(&["#[", "#!["]), 3));
        let mut none = Vec::new();
        find_candidates(b"int x;", &owned(&["#[", "#!["]), 0, &mut none);
        assert!(none.is_empty());
    }

    #[test]
    fn a_file_marker_in_code_standing_as_a_word_is_recorded_once_and_a_file_taken_back_is_not_scanned_for_one() {
        let language = Language::new("Annotated", ["an"], crate::StringRules::escaping_nothing(), ["//"], &[], [])
                .with_test_file_markers(["@Test", "[Fact"]);
        let marker_of = |source: &str, ranges: &[(usize, usize)]| {
            let mut walk = TestWalk::of(&language, source, true, TestsByPath::Nothing)?;
            walk.observe_line(0, source, true, 0, ranges, LineClass::WordsInCode, source.len());
            walk.get_file_marker()
        };
        assert_eq!(Some((0, 0)), marker_of("@Test void t() {}", &[(0, 17)]));
        assert_eq!(Some((0, 1)), marker_of("[Fact] [Fact]", &[(0, 13)]), "the second find was recorded over the first");
        assert_eq!(Some((16, 1)), marker_of("[Category(\"x\")] [Fact]", &[(0, 22)]));
        assert_eq!(Some((8, 1)), marker_of("/* c */ [Fact]", &[(7, 14)]), "a comment in front was taken for code");
        assert_eq!(None, marker_of("grid[0][Fact]", &[(0, 13)]), "an index after an index was taken for an attribute");
        assert_eq!(None, marker_of("args is [Fact]", &[(0, 14)]));
        assert_eq!(None, marker_of("new() { [Fact] = 1 }", &[(0, 20)]));
        assert_eq!(None, marker_of("x = [Fact] [Fact]", &[(0, 17)]));
        assert_eq!(Some((10, 1)), marker_of("[Factory] [Fact(Skip = \"x\")]", &[(0, 28)]));
        assert_eq!(None, marker_of("@TestOnly void t() {}", &[(0, 21)]));
        assert_eq!(None, marker_of("x@Test", &[(0, 6)]));
        assert_eq!(Some((3, 0)), marker_of("\u{feff}@Test void t() {}", &[(0, 20)]), "a byte order mark hid the marker");
        assert_eq!(None, marker_of("€@Test", &[(0, 8)]));
        assert_eq!(None, marker_of("list[Fact]", &[(0, 10)]), "an indexer was taken for an attribute");
        assert_eq!(None, marker_of("[Fact.Create(a)]", &[(0, 16)]), "a member access was taken for an attribute");
        assert_eq!(None, marker_of("@Test in a comment", &[(6, 18)]), "a marker outside the code ranges was taken");
        assert!(TestWalk::of(&language, "@Test", true, TestsByPath::TakenBack).is_none(), "a file a '!' took back was scanned");
        assert!(TestWalk::of(&language, "@Test", true, TestsByPath::WholeFile).is_none());
        assert!(TestWalk::of(&language, "int x;", true, TestsByPath::Nothing).is_none());
    }

    #[test]
    fn a_file_marker_inside_the_test_code_a_marker_opened_belongs_to_that_test() {
        let language = Language::new("Both", ["bo"], crate::StringRules::escaping_nothing(), ["//"], &[], [])
                .with_test_markers(["unittest"]).with_test_file_markers(["@Test"]);
        let marker_of = |source: &str| {
            let mut walk = TestWalk::of(&language, source, true, TestsByPath::Nothing)?;
            let mut line_start = 0;
            for line in source.split_inclusive('\n') {
                let raw_line = line.trim_end_matches('\n');
                let code = [(0, raw_line.trim_ascii().len())];
                let lead = raw_line.len() - raw_line.trim_ascii_start().len();
                walk.observe_line(line_start, raw_line, true, lead, &code, LineClass::WordsInCode, raw_line.len());
                line_start += line.len();
            }
            walk.get_file_marker()
        };
        assert_eq!(None, marker_of("int x;\nunittest { @Test }\nint y;\n"), "an annotation inside a test block made the file whole");
        assert_eq!(Some((26, 0)), marker_of("unittest { @Test }\nint y; @Test void t();\n"));
    }

    #[test]
    fn the_test_code_of_a_line_runs_from_its_marker_to_the_end_of_the_extent() {
        let language = Language::new("Rust", ["rs"], crate::StringRules::escaping_nothing(), ["//"], &[], [])
                .with_test_markers(["#[", "#!["]);
        let spans_of = |source: &str| {
            let mut walk = TestWalk::of(&language, source, true, TestsByPath::Nothing).unwrap();
            let mut line_start = 0;
            let mut spans = Vec::new();
            for line in source.split_inclusive('\n') {
                let raw_line = line.trim_end_matches('\n');
                let code = [(0, raw_line.trim_ascii().len())];
                let lead = raw_line.len() - raw_line.trim_ascii_start().len();
                walk.observe_line(line_start, raw_line, true, lead, &code, LineClass::WordsInCode, raw_line.len());
                spans.push(walk.get_test_spans().to_vec());
                line_start += line.len();
            }
            spans
        };
        assert_eq!(vec![vec![(0, 19)]], spans_of("#[cfg(test)] mod t; mod x;"));
        assert_eq!(vec![vec![(7, 26)]], spans_of("mod a; #[cfg(test)] mod t;"));
        assert_eq!(vec![vec![(0, 29)]], spans_of("#[cfg(test)] mod t { mod x; } mod y;"));
        assert_eq!(vec![vec![(8, 27)]], spans_of("mod a { #[cfg(test)] mod t; mod y; }"));
        assert_eq!(vec![vec![(0, 19), (27, 46)]], spans_of("#[cfg(test)] mod a; mod b; #[cfg(test)] mod c;"));
        assert_eq!(vec![vec![(20, 39)]], spans_of("#[allow(dead_code)] #[cfg(test)] mod t; mod x;"));
        assert_eq!(vec![vec![(0, 17)]], spans_of("#[test] fn t() {} mod x;"));
        assert_eq!(vec![vec![(0, 21), (22, 30)]], spans_of("#[cfg(test)] if a { } else { } x;"));
        assert_eq!(vec![vec![(0, usize::MAX)], vec![(13, usize::MAX)], vec![(25, usize::MAX)], vec![(40, 41)], vec![]],
                spans_of("#[cfg(test)]\nmod tests {\n    mod cases;\n}\nmod y;\n"));
    }

    #[test]
    fn an_inner_cfg_test_before_every_item_covers_the_whole_file_and_one_inside_a_module_its_module() {
        let language = Language::new("Rust", ["rs"], crate::StringRules::escaping_nothing(), ["//"], &[], [])
                .with_test_markers(["#[", "#!["]);
        let whole_file_attribute_of = |source: &str| {
            let mut walk = TestWalk::of(&language, source, true, TestsByPath::Nothing).unwrap();
            let mut line_start = 0;
            for line in source.split_inclusive('\n') {
                let raw_line = line.trim_end_matches('\n');
                let trimmed = raw_line.trim_ascii();
                let lead = raw_line.len() - raw_line.trim_ascii_start().len();
                let is_comment = trimmed.starts_with("//");
                let code = if is_comment || trimmed.is_empty() { Vec::new() } else { vec![(0, trimmed.len())] };
                walk.observe_line(line_start, raw_line, !code.is_empty(), lead, &code,
                        if is_comment { LineClass::WordsInComment } else { LineClass::WordsInCode }, raw_line.len());
                line_start += line.len();
            }
            walk.get_whole_file_attribute()
        };
        assert_eq!(Some((0, 13)), whole_file_attribute_of("#![cfg(test)]\nuse a;\n"));
        assert_eq!(Some((39, 52)), whole_file_attribute_of("// Copyright\n//! The crate\n\n#![no_std]\n#![cfg(test)]\nuse a;\n"));
        assert_eq!(Some((19, 32)), whole_file_attribute_of("#!/usr/bin/env run\n#![cfg(test)]\nfn main() {}\n"));
        assert_eq!(Some((3, 16)), whole_file_attribute_of("\u{feff}#![cfg(test)]\nuse a;\n"));
        assert_eq!(None, whole_file_attribute_of("mod checks {\n    #![cfg(test)]\n    use a;\n}\n"), "a module's inner attribute took the file");
        assert_eq!(None, whole_file_attribute_of("use a;\n#![cfg(test)]\nuse b;\n"));
        assert_eq!(None, whole_file_attribute_of("#![cfg(not(test))]\nuse a;\n"));
        assert_eq!(None, whole_file_attribute_of("#[cfg(test)]\nmod tests {}\n"), "an outer attribute is no inner one");
    }

    #[test]
    fn a_language_with_no_markers_or_a_file_with_none_gets_no_walk() {
        let d = Language::new("D", ["d"], crate::StringRules::escaping_nothing(), ["//"], &[], [])
                .with_test_markers(["unittest"]);
        let c = Language::new("C", ["c"], crate::StringRules::escaping_nothing(), ["//"], &[], []);
        assert!(TestWalk::of(&c, "unittest { }", true, TestsByPath::Nothing).is_none());
        assert!(TestWalk::of(&d, "int x;", true, TestsByPath::Nothing).is_none());
        assert!(TestWalk::of(&d, "unittest { }", false, TestsByPath::Nothing).is_none());
        assert!(TestWalk::of(&d, "unittest { }", true, TestsByPath::Nothing).is_some());
        assert!(TestWalk::of(&d, "unittest { }", true, TestsByPath::TakenBack).is_some(), "a '!' switched the markers that open lines off");
    }
}
