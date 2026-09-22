// The Rust-shaped attribute, read the same way for every language that declares '#['. Yes for a
// name holding test, for a cfg holding test under no not, and for a cfg_attr applying the former
// under the latter.
use memchr::memmem;

use super::{Marker, find_word_end, is_word_byte, is_word_start};

// Past this many bytes an unclosed '#[' is a stray bracket
const MOST_ATTRIBUTE_BYTES : usize = 4_096;
const OPENERS : [&str; 2] = ["#[", "#!["];
const CFG : &[u8] = b"cfg";
const CFG_ATTR : &[u8] = b"cfg_attr";
const TEST : &[u8] = b"test";
const BENCH : &[u8] = b"bench";
const NOT : &[u8] = b"not";
const LINE_COMMENT : &[u8] = b"//";
const BLOCK_COMMENT_START : &[u8] = b"/*";
const BLOCK_COMMENT_END : &[u8] = b"*/";

pub(super) fn is_opener(marker: &str) -> bool {
    OPENERS.contains(&marker)
}

// The arguments of a named attribute stay unread, so a path attribute naming a tests directory says nothing
pub(super) fn read(contents: &[u8], at: usize) -> Option<Marker> {
    let inner = contents.get(at + 1) == Some(&b'!');
    let open = at + if inner { 2 } else { 1 };
    let close = find_matching_bracket(contents, open, b']')?;
    let body = &contents[open + 1..close];
    let (name, rest) = split_name(body);
    let is_test = if name == CFG { find_arguments(rest).is_some_and(predicate_says_test) }
            else if name == CFG_ATTR { find_arguments(rest).is_some_and(applies_a_test_attribute) }
            else { holds_test(name) || name == BENCH };
    if !is_test { return None; }
    let after = close + 1;
    Some(if inner { Marker::WholeFile { after } } else { Marker::Extent { after } })
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
        if let Some(past) = skip_comment(bytes, at, limit) {
            at = past;
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

// Rust's own comments, since this is Rust's attribute
fn skip_comment(bytes: &[u8], at: usize, limit: usize) -> Option<usize> {
    let rest = &bytes[at..limit];
    if rest.starts_with(LINE_COMMENT) {
        return Some(memchr::memchr(b'\n', rest).map_or(limit, |newline| at + newline));
    }
    if rest.starts_with(BLOCK_COMMENT_START) {
        return Some(memmem::find(&rest[BLOCK_COMMENT_START.len()..], BLOCK_COMMENT_END)
                .map_or(limit, |end| at + BLOCK_COMMENT_START.len() + end + BLOCK_COMMENT_END.len()));
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
        } else if let Some(past) = skip_comment(predicate, at, predicate.len()) {
            at = past;
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
            if let Some(past) = skip_comment(bytes, at, bytes.len()) {
                at = past;
                continue;
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn read_marker_of(attribute: &str) -> Option<(bool, usize)> {
        read(attribute.as_bytes(), 0).map(|marker| match marker {
            Marker::WholeFile { after } => (true, after),
            Marker::Extent { after } => (false, after)
        })
    }

    #[test]
    fn an_attribute_whose_name_holds_test_is_a_marker_whatever_its_arguments() {
        for attribute in ["#[test]", "#[tokio::test]", "#[rstest]", "#[test_case(1, 2)]",
                "#[wasm_bindgen_test]", "#[std::prelude::v1::test]", "#[bench]", "#[ test ]"] {
            assert_eq!(read_marker_of(attribute), Some((false, attribute.len())), "{attribute}");
        }
        for attribute in ["#[should_panic]", "#[derive(Debug)]", "#[path = \"tests/x.rs\"]",
                "#![feature(test)]", "#[grammar = \"tests/grammar.pest\"]", "#[inline]", "#[cfg]"] {
            assert_eq!(read_marker_of(attribute), None, "{attribute}");
        }
    }

    #[test]
    fn a_cfg_predicate_is_read_for_test_under_no_not() {
        for attribute in ["#[cfg(test)]", "#[cfg(any(test, testlib))]", "#[cfg(all(feature = \"std\", test))]",
                "#[cfg(any(not(feature = \"x\"), test))]", "#[cfg( test )]", "#[cfg(\n    test\n)]"] {
            assert_eq!(read_marker_of(attribute), Some((false, attribute.len())), "{attribute}");
        }
        for attribute in ["#[cfg(not(test))]", "#[cfg(not(any(test, testlib)))]", "#[cfg(all(not(test), feature = \"x\"))]",
                "#[cfg(testlib)]", "#[cfg(feature = \"test-util\")]", "#[cfg(target_os = \"linux\")]",
                "#[cfg(tests)]", "#[cfg(test_harness)]"] {
            assert_eq!(read_marker_of(attribute), None, "{attribute}");
        }
    }

    #[test]
    fn a_cfg_attr_is_a_marker_only_where_a_test_attribute_is_applied_under_test() {
        for attribute in ["#[cfg_attr(test, test)]", "#[cfg_attr(test, tokio::test, ignore)]",
                "#[cfg_attr(any(test, feature = \"x\"), rstest)]"] {
            assert_eq!(read_marker_of(attribute), Some((false, attribute.len())), "{attribute}");
        }
        for attribute in ["#[cfg_attr(test, derive(Debug))]", "#![cfg_attr(test, allow(deref_nullptr))]",
                "#[cfg_attr(not(test), test)]", "#[cfg_attr(feature = \"x\", test)]", "#[cfg_attr(test)]"] {
            assert_eq!(read_marker_of(attribute), None, "{attribute}");
        }
    }

    #[test]
    fn the_inner_attribute_is_told_apart() {
        assert_eq!(read_marker_of("#![cfg(test)]"), Some((true, 13)));
        assert_eq!(read_marker_of("#![cfg(not(test))]"), None);
    }

    #[test]
    fn a_bracket_inside_a_comment_of_the_attribute_does_not_close_it() {
        let over_lines = "#[cfg(\n    // see [u8]\n    test\n)]";
        assert_eq!(read_marker_of(over_lines), Some((false, over_lines.len())));
        let block = "#[cfg(/* ] */ test)]";
        assert_eq!(read_marker_of(block), Some((false, block.len())));
        let negated_in_a_comment = "#[cfg(/* not( */ test)]";
        assert_eq!(read_marker_of(negated_in_a_comment), Some((false, negated_in_a_comment.len())));
        let applied_in_a_comment = "#[cfg_attr(test, /* test, */ derive(Debug))]";
        assert_eq!(read_marker_of(applied_in_a_comment), None);
        assert_eq!(read_marker_of("#[cfg(// ]\n"), None);
    }

    #[test]
    fn a_bracket_inside_a_string_of_the_attribute_does_not_close_it() {
        assert_eq!(read_marker_of("#[doc = \"] #[test]\"]"), None);
        assert_eq!(read_marker_of("#[test_case(\"]\")]"), Some((false, 17)));
        assert_eq!(read_marker_of("#[test_case(r#\"]\"#)]"), Some((false, 20)));
        assert_eq!(read_marker_of("#[test_case(\"\\\"]\")]"), Some((false, 19)));
        assert_eq!(read_marker_of("#[test"), None);
        assert!(is_opener("#[") && is_opener("#![") && !is_opener("#") && !is_opener("unittest"));
    }
}
