// Which lines of a file are test code. The markers are found once over the whole buffer before the
// line loop. Each one's extent is followed inside the loop over the code ranges of every line, so a
// brace inside a string or a comment is absent by construction.
use std::sync::LazyLock;

use memchr::memmem;

use crate::Language;

// Past this many bytes an unclosed '#[' is a stray bracket
const MOST_ATTRIBUTE_BYTES : usize = 4_096;
const ATTRIBUTE_OPENERS : [&str; 2] = ["#[", "#!["];
const D_MARKERS : [&str; 1] = ["unittest"];
const CFG : &[u8] = b"cfg";
const CFG_ATTR : &[u8] = b"cfg_attr";
const TEST : &[u8] = b"test";
const BENCH : &[u8] = b"bench";
const NOT : &[u8] = b"not";
const ELSE : &[u8] = b"else";

pub(crate) static PRINT_TEST_LINES : LazyLock<bool> =
        LazyLock::new(|| std::env::var_os("MEZURA_TEST_RANGES").is_some_and(|value| !value.is_empty()));

pub(crate) struct TestWalk<'a> {
    contents: &'a [u8],
    markers: &'static [&'static str],
    candidates: Vec<(usize, u8)>,
    next: usize,
    state: State,
    // The offset the scan of the open extent resumes from, which can sit past the current line
    cursor: usize,
}

impl<'a> TestWalk<'a> {
    pub(crate) fn of(language: &Language, contents: &'a str, enabled: bool) -> Option<TestWalk<'a>> {
        if !enabled { return None; }
        let markers = get_markers_of(language);
        if markers.is_empty() { return None; }

        let bytes = contents.as_bytes();
        let mut candidates = Vec::new();
        for (index, marker) in markers.iter().enumerate() {
            candidates.extend(memmem::find_iter(bytes, marker.as_bytes()).map(|at| (at, index as u8)));
        }
        if candidates.is_empty() { return None; }
        candidates.sort_unstable();

        Some(TestWalk { contents: bytes, markers, candidates, next: 0, state: State::Idle, cursor: 0 })
    }

    // Whether the line just classified is test code. The ranges are offsets into the line with its
    // whitespace trimmed at both ends, and they are read only where the line held code.
    pub(crate) fn observe_line(&mut self, line_start: usize, raw_line: &str, has_code: bool,
        ranges: &[(usize, usize)]) -> bool
    {
        let from_start = raw_line.trim_ascii_start();
        let base = line_start + raw_line.len() - from_start.len();
        let line = from_start.trim_ascii_end().as_bytes();
        let line_end = line_start + raw_line.len();
        let ranges: &[(usize, usize)] = if has_code { ranges } else { &[] };

        let mut is_test = matches!(self.state, State::Seeking { .. } | State::Inside { .. } | State::WholeFile);
        loop {
            match self.state {
                State::WholeFile => return true,
                State::Idle => {
                    let floor = self.cursor.max(base);
                    while self.candidates.get(self.next).is_some_and(|(at, _)| *at < floor) {
                        self.next += 1;
                    }
                    let Some(&(at, marker)) = self.candidates.get(self.next) else { return is_test };
                    if at >= line_end { return is_test; }
                    self.next += 1;
                    let Some(found) = self.read_marker(at, self.markers[marker as usize], base, ranges) else { continue };
                    is_test = true;
                    match found {
                        Marker::WholeFile => {
                            self.state = State::WholeFile;
                            return true;
                        },
                        Marker::Extent { after } => {
                            self.cursor = after;
                            self.state = State::Seeking { depth: 0, ends: Terminator::Undecided };
                        }
                    }
                },
                State::Seeking { .. } => if !self.seek_opener(base, line, ranges) { return is_test; },
                State::Inside { .. } => if !self.seek_closer(base, line, ranges) { return is_test; },
                State::AfterClose => match self.look_past_closer(base, line, ranges) {
                    Look::Exhausted => return is_test,
                    Look::Else => {
                        is_test = true;
                        self.state = State::Seeking { depth: 0, ends: Terminator::BraceOrSemicolon };
                    },
                    Look::Other => self.state = State::Idle
                }
            }
        }
    }

    fn read_marker(&self, at: usize, marker: &str, base: usize, ranges: &[(usize, usize)]) -> Option<Marker> {
        let in_line = at - base;
        let width = marker.len();
        if !ranges.iter().any(|&(from, to)| from <= in_line && in_line + width <= to) {
            return None;
        }
        if marker.starts_with('#') {
            return read_attribute(self.contents, at);
        }
        let before = at.checked_sub(1).map(|i| self.contents[i]);
        let after = self.contents.get(at + width).copied();
        if before.is_some_and(is_word_byte) || after.is_some_and(is_word_byte) {
            return None;
        }
        Some(Marker::Extent { after: at + width })
    }

    // Every bracket moves the depth, and an opener or a terminator counts only at depth zero. The
    // words met at depth zero decide which of the two ends the item.
    fn seek_opener(&mut self, base: usize, line: &[u8], ranges: &[(usize, usize)]) -> bool {
        let State::Seeking { mut depth, mut ends } = self.state else { return false };
        for &(from, to) in ranges {
            let mut at = self.cursor.saturating_sub(base).max(from);
            while at < to {
                let byte = line[at];
                match byte {
                    b'(' | b'[' => depth += 1,
                    b')' | b']' | b'}' => depth = depth.saturating_sub(1),
                    b'{' => {
                        if depth == 0 && ends != Terminator::Semicolon {
                            self.state = State::Inside { depth: 1 };
                            self.cursor = base + at + 1;
                            return true;
                        }
                        depth += 1;
                    },
                    b';' if depth == 0 => {
                        self.state = State::Idle;
                        self.cursor = base + at + 1;
                        return true;
                    },
                    _ if depth == 0 && ends.is_open() && is_word_start(byte) => {
                        let end = find_word_end(line, at);
                        ends = ends.after(&line[at..end]);
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

    fn seek_closer(&mut self, base: usize, line: &[u8], ranges: &[(usize, usize)]) -> bool {
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
                    return true;
                }
            }
        }
        self.state = State::Inside { depth };
        false
    }

    fn look_past_closer(&mut self, base: usize, line: &[u8], ranges: &[(usize, usize)]) -> Look {
        for &(from, to) in ranges {
            let mut at = self.cursor.saturating_sub(base).max(from);
            while at < to && line[at].is_ascii_whitespace() { at += 1; }
            if at >= to { continue; }
            if line[at..to].starts_with(ELSE) && !line.get(at + ELSE.len()).is_some_and(|byte| is_word_byte(*byte)) {
                self.cursor = base + at + ELSE.len();
                return Look::Else;
            }
            self.cursor = base + at;
            return Look::Other;
        }
        Look::Exhausted
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Seeking { depth: u32, ends: Terminator },
    Inside { depth: u32 },
    AfterClose,
    WholeFile,
}

// What ends the item under a marker. A binding, an import, a type alias, a static, a const and an
// extern crate end at the next semicolon at depth zero and their braces only nest. Everything else
// ends at whichever of brace and semicolon comes first. A const fn and an extern fn are the two
// that need a second word.
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

    fn after(self, word: &[u8]) -> Terminator {
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

enum Marker {
    WholeFile,
    Extent { after: usize },
}

enum Look {
    Exhausted,
    Else,
    Other,
}

// Written in here until the language file declares them
fn get_markers_of(language: &Language) -> &'static [&'static str] {
    if language.name.eq_ignore_ascii_case("rust") { &ATTRIBUTE_OPENERS }
    else if language.name.eq_ignore_ascii_case("d") { &D_MARKERS }
    else { &[] }
}

// Yes for a cfg whose predicate holds the word test under no not, for a cfg_attr whose predicate
// holds and which applies an attribute with test in its name, and for any other attribute with test
// in its name. The arguments of those are left unread, which keeps a path attribute naming a tests
// directory out.
fn read_attribute(contents: &[u8], at: usize) -> Option<Marker> {
    let inner = contents.get(at + 1) == Some(&b'!');
    let open = at + if inner { 2 } else { 1 };
    let close = find_matching_bracket(contents, open, b']')?;
    let body = &contents[open + 1..close];
    let (name, rest) = split_name(body);
    let is_test = if name == CFG { find_arguments(rest).is_some_and(predicate_says_test) }
            else if name == CFG_ATTR { find_arguments(rest).is_some_and(applies_a_test_attribute) }
            else { holds_test(name) || name == BENCH };
    if !is_test { return None; }
    Some(if inner { Marker::WholeFile } else { Marker::Extent { after: close + 1 } })
}

fn find_matching_bracket(bytes: &[u8], open: usize, closer: u8) -> Option<usize> {
    let opener = bytes[open];
    let limit = bytes.len().min(open + MOST_ATTRIBUTE_BYTES);
    let mut depth = 0usize;
    let mut at = open;
    while at < limit {
        let byte = bytes[at];
        if byte == b'"' || (byte == b'r' && starts_a_raw_string(bytes, at)) {
            at = skip_string(bytes, at, limit)?;
            continue;
        }
        if byte == opener {
            depth += 1;
        } else if byte == closer {
            depth -= 1;
            if depth == 0 { return Some(at); }
        }
        at += 1;
    }
    None
}

fn starts_a_raw_string(bytes: &[u8], at: usize) -> bool {
    let preceded_by_a_word = at > 0 && is_word_byte(bytes[at - 1]);
    !preceded_by_a_word && matches!(bytes.get(at + 1), Some(b'"') | Some(b'#'))
}

fn skip_string(bytes: &[u8], at: usize, limit: usize) -> Option<usize> {
    if bytes[at] == b'"' {
        let mut i = at + 1;
        while i < limit {
            match bytes[i] {
                b'\\' => i += 2,
                b'"' => return Some(i + 1),
                _ => i += 1
            }
        }
        return None;
    }
    let mut hashes = 0;
    while bytes.get(at + 1 + hashes) == Some(&b'#') { hashes += 1; }
    if bytes.get(at + 1 + hashes) != Some(&b'"') { return None; }
    let mut i = at + 2 + hashes;
    while i < limit {
        if bytes[i] == b'"' && bytes[i + 1..].iter().take(hashes).filter(|byte| **byte == b'#').count() == hashes {
            return Some(i + 1 + hashes);
        }
        i += 1;
    }
    None
}

fn split_name(body: &[u8]) -> (&[u8], &[u8]) {
    let mut at = 0;
    while at < body.len() && body[at].is_ascii_whitespace() { at += 1; }
    let start = at;
    while at < body.len() && (is_word_byte(body[at]) || body[at] == b':') { at += 1; }
    (&body[start..at], &body[at..])
}

fn find_arguments(rest: &[u8]) -> Option<&[u8]> {
    let mut at = 0;
    while at < rest.len() && rest[at].is_ascii_whitespace() { at += 1; }
    if rest.get(at) != Some(&b'(') { return None; }
    let close = find_matching_bracket(rest, at, b')')?;
    Some(&rest[at + 1..close])
}

fn holds_test(name: &[u8]) -> bool {
    memmem::find(name, TEST).is_some()
}

// The word test as a term of its own, at a depth where no enclosing group is a not. A string is
// skipped whole, so a feature named with the word says nothing.
fn predicate_says_test(predicate: &[u8]) -> bool {
    let mut not_groups = 0u64;
    let mut depth = 0u32;
    let mut last_word: &[u8] = &[];
    let mut at = 0;
    while at < predicate.len() {
        let byte = predicate[at];
        if byte == b'"' {
            at = skip_string(predicate, at, predicate.len()).unwrap_or(predicate.len());
            last_word = &[];
        } else if byte == b'(' {
            if last_word == NOT && depth < 64 { not_groups |= 1 << depth; }
            depth += 1;
            last_word = &[];
            at += 1;
        } else if byte == b')' {
            depth = depth.saturating_sub(1);
            if depth < 64 { not_groups &= !(1 << depth); }
            last_word = &[];
            at += 1;
        } else if is_word_start(byte) {
            let end = find_word_end(predicate, at);
            let word = &predicate[at..end];
            if word == TEST && ends_a_term(predicate, end) && not_groups == 0 { return true; }
            last_word = word;
            at = end;
        } else {
            at += 1;
        }
    }
    false
}

fn ends_a_term(predicate: &[u8], from: usize) -> bool {
    let mut at = from;
    while at < predicate.len() && predicate[at].is_ascii_whitespace() { at += 1; }
    matches!(predicate.get(at), None | Some(b',') | Some(b')'))
}

fn applies_a_test_attribute(arguments: &[u8]) -> bool {
    let mut items = split_at_top_level_commas(arguments);
    let Some(predicate) = items.next() else { return false };
    predicate_says_test(predicate) && items.any(|item| holds_test(split_name(item).0))
}

fn split_at_top_level_commas(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut depth = 0usize;
    let mut start = 0;
    let mut at = 0;
    let mut done = false;
    std::iter::from_fn(move || {
        if done { return None; }
        while at < bytes.len() {
            match bytes[at] {
                b'"' => at = skip_string(bytes, at, bytes.len()).unwrap_or(bytes.len()),
                b'(' | b'[' => { depth += 1; at += 1; },
                b')' | b']' => { depth = depth.saturating_sub(1); at += 1; },
                b',' if depth == 0 => {
                    let item = &bytes[start..at];
                    at += 1;
                    start = at;
                    return Some(item);
                },
                _ => at += 1
            }
        }
        done = true;
        Some(&bytes[start..])
    })
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

fn is_word_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

fn find_word_end(bytes: &[u8], from: usize) -> usize {
    let mut at = from;
    while at < bytes.len() && is_word_byte(bytes[at]) { at += 1; }
    at
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(attribute: &str) -> Option<(bool, usize)> {
        read_attribute(attribute.as_bytes(), 0).map(|marker| match marker {
            Marker::WholeFile => (true, 0),
            Marker::Extent { after } => (false, after)
        })
    }

    #[test]
    fn an_attribute_whose_name_holds_test_is_a_marker_whatever_its_arguments() {
        for attribute in ["#[test]", "#[tokio::test]", "#[rstest]", "#[test_case(1, 2)]",
                "#[wasm_bindgen_test]", "#[std::prelude::v1::test]", "#[bench]", "#[ test ]"] {
            assert_eq!(read(attribute), Some((false, attribute.len())), "{attribute}");
        }
        for attribute in ["#[should_panic]", "#[derive(Debug)]", "#[path = \"tests/x.rs\"]",
                "#![feature(test)]", "#[grammar = \"tests/grammar.pest\"]", "#[inline]", "#[cfg]"] {
            assert_eq!(read(attribute), None, "{attribute}");
        }
    }

    #[test]
    fn a_cfg_predicate_is_read_for_test_under_no_not() {
        for attribute in ["#[cfg(test)]", "#[cfg(any(test, testlib))]", "#[cfg(all(feature = \"std\", test))]",
                "#[cfg(any(not(feature = \"x\"), test))]", "#[cfg( test )]", "#[cfg(\n    test\n)]"] {
            assert_eq!(read(attribute), Some((false, attribute.len())), "{attribute}");
        }
        for attribute in ["#[cfg(not(test))]", "#[cfg(not(any(test, testlib)))]", "#[cfg(all(not(test), feature = \"x\"))]",
                "#[cfg(testlib)]", "#[cfg(feature = \"test-util\")]", "#[cfg(target_os = \"linux\")]",
                "#[cfg(tests)]", "#[cfg(test_harness)]"] {
            assert_eq!(read(attribute), None, "{attribute}");
        }
    }

    #[test]
    fn a_cfg_attr_is_a_marker_only_where_a_test_attribute_is_applied_under_test() {
        for attribute in ["#[cfg_attr(test, test)]", "#[cfg_attr(test, tokio::test, ignore)]",
                "#[cfg_attr(any(test, feature = \"x\"), rstest)]"] {
            assert_eq!(read(attribute), Some((false, attribute.len())), "{attribute}");
        }
        for attribute in ["#[cfg_attr(test, derive(Debug))]", "#![cfg_attr(test, allow(deref_nullptr))]",
                "#[cfg_attr(not(test), test)]", "#[cfg_attr(feature = \"x\", test)]", "#[cfg_attr(test)]"] {
            assert_eq!(read(attribute), None, "{attribute}");
        }
    }

    #[test]
    fn the_inner_attribute_covers_the_whole_file() {
        assert_eq!(read("#![cfg(test)]"), Some((true, 0)));
        assert_eq!(read("#![cfg(not(test))]"), None);
    }

    #[test]
    fn a_bracket_inside_a_string_of_the_attribute_does_not_close_it() {
        assert_eq!(read("#[doc = \"] #[test]\"]"), None);
        assert_eq!(read("#[test_case(\"]\")]"), Some((false, 17)));
        assert_eq!(read("#[test_case(r#\"]\"#)]"), Some((false, 20)));
        assert_eq!(read("#[test_case(\"\\\"]\")]"), Some((false, 19)));
        assert_eq!(read("#[test"), None);
    }

    #[test]
    fn the_words_after_the_attribute_decide_what_ends_the_item() {
        let decide = |words: &[&[u8]]| words.iter().fold(Terminator::Undecided, |ends, word| ends.after(word));
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
        let language = Language::new("D", ["d"], crate::StringRules::escaping_nothing(), ["//"], &[], []);
        let source = "unittests { } version(unittest) { } my_unittest";
        let walk = TestWalk::of(&language, source, true).unwrap();
        let ranges = [(0, source.len())];
        assert!(walk.read_marker(0, "unittest", 0, &ranges).is_none());
        assert!(matches!(walk.read_marker(22, "unittest", 0, &ranges), Some(Marker::Extent { after: 30 })));
        assert!(walk.read_marker(39, "unittest", 0, &ranges).is_none());
        assert!(walk.read_marker(22, "unittest", 0, &[(0, 10)]).is_none());
    }

    #[test]
    fn a_language_with_no_markers_or_a_file_with_none_gets_no_walk() {
        let d = Language::new("D", ["d"], crate::StringRules::escaping_nothing(), ["//"], &[], []);
        let c = Language::new("C", ["c"], crate::StringRules::escaping_nothing(), ["//"], &[], []);
        assert!(TestWalk::of(&c, "unittest { }", true).is_none());
        assert!(TestWalk::of(&d, "int x;", true).is_none());
        assert!(TestWalk::of(&d, "unittest { }", false).is_none());
        assert!(TestWalk::of(&d, "unittest { }", true).is_some());
    }
}
