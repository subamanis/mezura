use std::borrow::Cow;

pub const UNRELEASED : &str = "unreleased";
pub const VERSION_ID : &str = concat!("v", env!("CARGO_PKG_VERSION"));
static CHANGELOG_BYTES : &[u8] = include_bytes!("../Changelog");

pub fn get_changelog() -> Cow<'static, str> {
    String::from_utf8_lossy(CHANGELOG_BYTES)
}

// The release date lives in the Changelog's first line, 'v3.0.0 - unreleased', and nowhere else. A
// test in this module keeps that line and VERSION_ID together.
pub fn find_release_date() -> Option<String> {
    let changelog = get_changelog();
    let (_, date) = changelog.lines().next()?.split_once(" - ")?;
    let date = date.trim();
    (date != UNRELEASED).then(|| date.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{VERSION_ID, get_changelog};

    // '--version' reads the release date from the first line of the Changelog, so without this the
    // two drift apart silently and '--version' quotes the date of a release that is not running.
    #[test]
    fn the_changelog_opens_with_the_version_this_binary_reports() {
        let changelog = get_changelog();
        let first = changelog.lines().next().unwrap();
        assert!(first.starts_with(&format!("{VERSION_ID} - ")),
                "the Changelog opens with '{first}', which does not start with '{VERSION_ID} - '");
    }
}
