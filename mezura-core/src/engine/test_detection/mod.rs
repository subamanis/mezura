mod attribute;
mod module_graph;
mod path_rules;
mod walk;

pub(crate) use module_graph::{Declaration, FilesOnDisk, ModuleDeclarations, ModuleRow, find_declaring_chain,
        promote_declared_test_modules};
pub(crate) use path_rules::{BuildFilesSeen, DirectoryScope, TestScope, TestsByPath, find_test_directory_along};
pub(crate) use walk::TestWalk;

pub(super) const fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

pub(super) fn is_word_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

#[inline(always)]
pub(super) fn begins_with(bytes: &[u8], word: &[u8]) -> bool {
    bytes.len() >= word.len() && bytes.iter().zip(word).all(|(byte, wanted)| byte == wanted)
}

pub(super) fn find_word_end(bytes: &[u8], from: usize) -> usize {
    let mut at = from;
    while at < bytes.len() && is_word_byte(bytes[at]) { at += 1; }
    at
}

#[inline(always)]
pub(super) fn find_next_brace(bytes: &[u8], from: usize) -> usize {
    memchr::memchr2(b'{', b'}', &bytes[from..]).map_or(usize::MAX, |at| from + at)
}

pub(super) fn skip_whitespace(bytes: &[u8], from: usize) -> usize {
    let mut at = from;
    while bytes.get(at).is_some_and(u8::is_ascii_whitespace) { at += 1; }
    at
}
