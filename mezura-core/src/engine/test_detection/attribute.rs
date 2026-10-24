// The Rust-shaped attribute, read the same way for every language that declares '#['. Yes for a
// name holding test, for a cfg holding test under no not, and for a cfg_attr applying the former
// under the latter.
use super::{begins_with, build_byte_table, find_word_end, is_word_byte, is_word_start, skip_whitespace};
use super::walk::Marker;

pub(super) const RAW_IDENTIFIER : &[u8] = b"r#";
// Past this many bytes an unclosed '#[' is a stray bracket
const MOST_ATTRIBUTE_BYTES : usize = 4_096;
const OPENERS : [&str; 2] = ["#[", "#!["];
const CFG : &[u8] = b"cfg";
const CFG_ATTR : &[u8] = b"cfg_attr";
const TEST : &[u8] = b"test";
const BENCH : &[u8] = b"bench";
const NOT : &[u8] = b"not";
const ANY : &[u8] = b"any";
const LINE_COMMENT : &[u8] = b"//";
const BLOCK_COMMENT_START : &[u8] = b"/*";
const BLOCK_COMMENT_END : &[u8] = b"*/";
const SCAN_STOPS : [bool; 256] = build_byte_table(b"\"'r/()[]");
const NAME_BYTES : [bool; 256] = {
    let mut table = [false; 256];
    let mut byte = 0;
    while byte < 256 {
        table[byte] = is_word_byte(byte as u8) || byte == b':' as usize;
        byte += 1;
    }
    table
};

pub(super) fn is_opener(marker: &str) -> bool {
    OPENERS.iter().any(|opener| opener.len() == marker.len() && begins_with(marker.as_bytes(), opener.as_bytes()))
}

// The arguments of a named attribute stay unread, so a path attribute naming a tests directory says nothing
pub(super) fn read(contents: &[u8], at: usize) -> Option<Marker> {
    let inner = contents.get(at + 1) == Some(&b'!');
    let open = at + if inner { 2 } else { 1 };
    let (name, rest) = split_name(&contents[open + 1..]);
    let arguments_close = if name == CFG || name == CFG_ATTR {
        let (from, to) = find_arguments(rest)?;
        let arguments = &rest[from..to];
        let says_test = holds_test(arguments)
                && if name == CFG { predicate_says_test(arguments) } else { applies_a_test_attribute(arguments) };
        if !says_test { return None; }
        Some(contents.len() - rest.len() + to)
    } else if holds_test(name) || name == BENCH { None } else { return None };
    let close = find_matching_bracket(contents, open, b']')?;
    if arguments_close.is_some_and(|at| at > close) { return None; }
    let after = close + 1;
    Some(if inner { Marker::RestOfScope { after } } else { Marker::Extent { after } })
}

pub(super) fn find_matching_bracket(bytes: &[u8], open: usize, closer: u8) -> Option<usize> {
    let opener = bytes[open];
    let limit = bytes.len().min(open + MOST_ATTRIBUTE_BYTES);
    let mut depth = 0usize;
    let mut at = open;
    while at < limit {
        let byte = bytes[at];
        if !SCAN_STOPS[byte as usize] {
            at += 1;
            continue;
        }
        if byte == b'"' || (byte == b'r' && starts_a_raw_string(bytes, at)) {
            at = skip_string(bytes, at, limit)?;
            continue;
        }
        if byte == b'/' && let Some(past) = skip_comment(bytes, at, limit) {
            at = past;
            continue;
        }
        if byte == b'\'' {
            at = skip_character_literal(bytes, at);
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

// Rust's comments, which nest. The pairs are read left to right as the lexer reads them, so '/*/'
// opens and '*/*' closes.
pub(super) fn skip_comment(bytes: &[u8], at: usize, limit: usize) -> Option<usize> {
    let rest = &bytes[at..limit];
    if rest.starts_with(LINE_COMMENT) {
        return Some(memchr::memchr(b'\n', rest).map_or(limit, |newline| at + newline));
    }
    if !rest.starts_with(BLOCK_COMMENT_START) { return None; }
    let mut depth = 0usize;
    let mut from = at;
    while let Some(found) = memchr::memchr2(b'/', b'*', &bytes[from..limit]) {
        let pair = from + found;
        if pair + 1 == limit { break; }
        from = match &bytes[pair..pair + 2] {
            BLOCK_COMMENT_START => {
                depth += 1;
                pair + 2
            },
            BLOCK_COMMENT_END => {
                depth -= 1;
                if depth == 0 { return Some(pair + 2); }
                pair + 2
            },
            _ => pair + 1
        };
    }
    Some(limit)
}

pub(super) fn skip_whitespace_and_comments(bytes: &[u8], from: usize) -> usize {
    let mut at = skip_whitespace(bytes, from);
    while bytes.get(at) == Some(&b'/') && let Some(past) = skip_comment(bytes, at, bytes.len()) {
        at = skip_whitespace(bytes, past);
    }
    at
}

pub(super) fn skip_string(bytes: &[u8], at: usize, limit: usize) -> Option<usize> {
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

// A quote that closes nothing two characters on is a lifetime or a label, 'a and 'outer
pub(super) fn skip_character_literal(bytes: &[u8], quote: usize) -> usize {
    if bytes.get(quote + 1) == Some(&b'\\') {
        let from = (quote + 3).min(bytes.len());
        return match memchr::memchr2(b'\'', b'\n', &bytes[from..]) {
            Some(found) if bytes[from + found] == b'\'' => from + found + 1,
            _ => quote + 1
        };
    }
    let width = match bytes.get(quote + 1) {
        None => return quote + 1,
        Some(&byte) if byte < 0x80 => 1,
        Some(&byte) if byte >= 0xF0 => 4,
        Some(&byte) if byte >= 0xE0 => 3,
        Some(_) => 2
    };
    match bytes.get(quote + 1 + width) == Some(&b'\'') {
        true => quote + 2 + width,
        false => quote + 1
    }
}

// Under any predicate, since one that holds in some build is enough
pub(super) fn is_applied_through_cfg_attr(body: &[u8], name: &[u8]) -> bool {
    let (attribute_name, rest) = split_name(body);
    if attribute_name != CFG_ATTR { return false; }
    let Some((from, to)) = find_arguments(rest) else { return false };
    let mut items = split_at_top_level_commas(&rest[from..to]);
    items.next();
    items.any(|item| {
        let start = skip_whitespace_and_comments(item, 0);
        let item = item[start..].strip_prefix(RAW_IDENTIFIER).unwrap_or(&item[start..]);
        split_name(item).0 == name || is_applied_through_cfg_attr(item, name)
    })
}

// 'r#type' is a name, so the hashes have to end in a quote
pub(super) fn starts_a_raw_string(bytes: &[u8], at: usize) -> bool {
    let prefix = if at > 0 && matches!(bytes[at - 1], b'b' | b'c') { at - 1 } else { at };
    let preceded_by_a_word = prefix > 0 && is_word_byte(bytes[prefix - 1]);
    let mut quote = at + 1;
    while bytes.get(quote) == Some(&b'#') { quote += 1; }
    !preceded_by_a_word && bytes.get(quote) == Some(&b'"')
}

fn split_name(body: &[u8]) -> (&[u8], &[u8]) {
    let start = skip_whitespace_and_comments(body, 0);
    let mut at = start;
    while at < body.len() && NAME_BYTES[body[at] as usize] { at += 1; }
    (&body[start..at], &body[at..])
}

fn find_arguments(rest: &[u8]) -> Option<(usize, usize)> {
    let at = skip_whitespace_and_comments(rest, 0);
    if rest.get(at) != Some(&b'(') { return None; }
    let close = find_matching_bracket(rest, at, b')')?;
    Some((at + 1, close))
}

fn holds_test(name: &[u8]) -> bool {
    let test = u32::from_le_bytes(*b"test");
    name.windows(4).any(|window| u32::from_le_bytes([window[0], window[1], window[2], window[3]]) == test)
}

// The word test as a term of its own, at a depth where no enclosing group is a not or an any. An
// any is met whenever another of its arms holds, so what sits under it ships in a build with that
// feature or target on. A string is skipped whole, so a feature named with the word says nothing.
fn predicate_says_test(predicate: &[u8]) -> bool {
    let mut not_or_any_groups = 0u64;
    let mut depth = 0u32;
    let mut last_word: &[u8] = &[];
    let mut at = 0;
    while at < predicate.len() {
        let byte = predicate[at];
        if byte == b'"' {
            at = skip_string(predicate, at, predicate.len()).unwrap_or(predicate.len());
            last_word = &[];
        } else if byte == b'/' && let Some(past) = skip_comment(predicate, at, predicate.len()) {
            at = past;
        } else if byte == b'(' {
            if (last_word == NOT || last_word == ANY) && depth < 64 { not_or_any_groups |= 1 << depth; }
            depth += 1;
            last_word = &[];
            at += 1;
        } else if byte == b')' {
            depth = depth.saturating_sub(1);
            if depth < 64 { not_or_any_groups &= !(1 << depth); }
            last_word = &[];
            at += 1;
        } else if is_word_start(byte) {
            let end = find_word_end(predicate, at);
            let word = &predicate[at..end];
            if word == TEST && ends_a_term(predicate, end) && not_or_any_groups == 0 { return true; }
            last_word = word;
            at = end;
        } else {
            at += 1;
        }
    }
    false
}

fn ends_a_term(predicate: &[u8], from: usize) -> bool {
    matches!(predicate.get(skip_whitespace_and_comments(predicate, from)), None | Some(b',') | Some(b')'))
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
            if bytes[at] == b'/' && let Some(past) = skip_comment(bytes, at, bytes.len()) {
                at = past;
                continue;
            }
            match bytes[at] {
                b'"' => at = skip_string(bytes, at, bytes.len()).unwrap_or(bytes.len()),
                b'r' if starts_a_raw_string(bytes, at) => at = skip_string(bytes, at, bytes.len()).unwrap_or(bytes.len()),
                b'\'' => at = skip_character_literal(bytes, at),
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
            Marker::RestOfScope { after } => (true, after),
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
    fn a_cfg_predicate_is_read_for_test_under_no_not_and_no_any() {
        for attribute in ["#[cfg(test)]", "#[cfg(all(feature = \"std\", test))]", "#[cfg(all(test, any(unix, windows)))]",
                "#[cfg( test )]", "#[cfg(\n    test\n)]"] {
            assert_eq!(read_marker_of(attribute), Some((false, attribute.len())), "{attribute}");
        }
        for attribute in ["#[cfg(not(test))]", "#[cfg(not(any(test, testlib)))]", "#[cfg(all(not(test), feature = \"x\"))]",
                "#[cfg(any(test, testlib))]", "#[cfg(any(feature = \"std\", test))]", "#[cfg(any(not(feature = \"x\"), test))]",
                "#[cfg(any(all(test, unix), feature = \"x\"))]", "#[cfg(all(any(test, miri), unix))]",
                "#[cfg(testlib)]", "#[cfg(feature = \"test-util\")]", "#[cfg(target_os = \"linux\")]",
                "#[cfg(tests)]", "#[cfg(test_harness)]"] {
            assert_eq!(read_marker_of(attribute), None, "{attribute}");
        }
    }

    #[test]
    fn a_cfg_attr_is_a_marker_only_where_a_test_attribute_is_applied_under_test() {
        for attribute in ["#[cfg_attr(test, test)]", "#[cfg_attr(test, tokio::test, ignore)]",
                "#[cfg_attr(all(test, feature = \"x\"), rstest)]"] {
            assert_eq!(read_marker_of(attribute), Some((false, attribute.len())), "{attribute}");
        }
        for attribute in ["#[cfg_attr(test, derive(Debug))]", "#![cfg_attr(test, allow(deref_nullptr))]",
                "#[cfg_attr(not(test), test)]", "#[cfg_attr(feature = \"x\", test)]", "#[cfg_attr(test)]",
                "#[cfg_attr(any(test, feature = \"x\"), rstest)]"] {
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
        let nested = "#[cfg(/* a /* ] */ ] */ test)]";
        assert_eq!(read_marker_of(nested), Some((false, nested.len())));
        let negated_in_a_nested_comment = "#[cfg(/* a /* b */ not( */ test)]";
        assert_eq!(read_marker_of(negated_in_a_nested_comment), Some((false, negated_in_a_nested_comment.len())));
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
        assert_eq!(read_marker_of("#[test_case(r#type)]"), Some((false, 20)));
        assert_eq!(read_marker_of("#[test_case(\"\\\"]\")]"), Some((false, 19)));
        assert_eq!(read_marker_of("#[test"), None);
        assert!(is_opener("#[") && is_opener("#![") && !is_opener("#") && !is_opener("unittest"));
    }

    #[test]
    fn a_comment_before_a_name_or_its_arguments_hides_nothing() {
        for attribute in ["#[/* a note */ test]", "#[cfg /* a note */ (test)]", "#[cfg_attr(test, /* a note */ test)]",
                "#[cfg_attr(test,\n    // a note\n    test)]", "#[cfg(test /* a note */)]", "#[cfg(test // a note\n)]",
                "#[cfg(all(test /* a note */, unix))]", "#[cfg_attr(test /* a note */, test)]"] {
            assert_eq!(read_marker_of(attribute), Some((false, attribute.len())), "{attribute}");
        }
        assert_eq!(read_marker_of("#[cfg(testing /* test */)]"), None);
    }

    #[test]
    fn a_comma_inside_a_character_or_a_raw_string_of_a_cfg_attr_splits_nothing() {
        assert_eq!(read_marker_of("#[cfg_attr(test, rustfmt::skip(')', test_x))]"), None);
        let raw = "#[cfg_attr(test, doc = r\"\\\", test_case)]";
        assert_eq!(read_marker_of(raw), Some((false, raw.len())));
    }

    #[test]
    fn a_bracket_inside_a_character_of_the_attribute_does_not_close_it() {
        for attribute in ["#[test_case(']')]", "#[test_case('[')]", "#[test_case('\\u{10_FFFF}', '\"')]",
                "#[test_case(br\"\\\", ']')]"] {
            assert_eq!(read_marker_of(attribute), Some((false, attribute.len())), "{attribute}");
        }
    }
}
