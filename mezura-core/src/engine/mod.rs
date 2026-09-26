//! The machinery that turns a set of targets into a result.

pub mod config;
pub mod targets;

pub(crate) mod consumer;
pub(crate) mod file_parser;
pub(crate) mod identity;
pub(crate) mod masks;
pub(crate) mod modules;
pub(crate) mod producer;
pub(crate) mod test_detection;

// Compared without regard to case wherever the filesystem ignores it, since the name a listing
// spells is the same file there.
pub(crate) fn is_the_same_name(listed: &[u8], wanted: &[u8]) -> bool {
    match cfg!(any(windows, target_os = "macos")) {
        true => listed.eq_ignore_ascii_case(wanted),
        false => listed == wanted
    }
}
