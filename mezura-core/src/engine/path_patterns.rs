//! One reading of a glob pattern list for every command that takes one. It tells paths from names,
//! finds the root a name is read from, and lets the last written match win.

use std::path::{Component, Path};

use globset::{Candidate, GlobBuilder, GlobSet, GlobSetBuilder};

use crate::engine::targets::{absolutize_pattern, convert_to_absolute, normalise_separators};

const NEGATION : char = '!';
const WILDCARDS : [char; 4] = ['*', '?', '[', '{'];
const ANY_DEPTH : &str = "**/";
const EVERYTHING_BELOW : &str = "/**";

/// Why a pattern could not be read, with the pattern as it was written.
#[derive(Debug, PartialEq, Eq, Clone)]
#[non_exhaustive]
pub enum PatternError {
    /// Does not parse as a glob.
    InvalidGlob(String),
    /// Holds nothing once its `!` and its trailing `/` are read.
    Empty(String)
}

impl std::error::Error for PatternError {}

impl std::fmt::Display for PatternError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidGlob(x) => write!(f, "'{x}' is not a valid glob pattern."),
            Self::Empty(x) => write!(f, "'{x}' names nothing once its '!' and its trailing '/' are read.")
        }
    }
}

/// Whether every pattern of `--tests` reads, for refusing a bad one at the moment somebody typed it.
pub fn validate_test_patterns(patterns: &[String]) -> Result<(), PatternError> {
    PathPatternMatcher::compile(patterns).map(|_| ())
}

/// Whether a pattern names a place, which is resolved from where the pattern was written.
pub fn is_a_path_pattern(pattern: &str) -> bool {
    let text = normalise_separators(pattern.trim().trim_start_matches(NEGATION));
    matches!(Path::new(text.as_ref()).components().next(),
            Some(Component::Prefix(_) | Component::RootDir | Component::CurDir | Component::ParentDir))
}

// Each glob remembers where in the written list it came from, since the last one written decides
pub(crate) struct PathPatternMatcher {
    names: GlobSet,
    paths: GlobSet,
    names_written_at: Vec<usize>,
    paths_written_at: Vec<usize>,
    rules: Vec<Rule>
}

impl PathPatternMatcher {
    pub(crate) fn compile(patterns: &[String]) -> Result<Self, PatternError> {
        let mut names = GlobSetBuilder::new();
        let mut paths = GlobSetBuilder::new();
        let mut names_written_at = Vec::new();
        let mut paths_written_at = Vec::new();
        let mut rules = Vec::with_capacity(patterns.len());
        for (written_at, pattern) in patterns.iter().enumerate() {
            let text = pattern.trim();
            let (negated, text) = match text.strip_prefix(NEGATION) {
                Some(rest) => (true, rest.trim()),
                None => (false, text)
            };
            let text = normalise_separators(text);
            let (folders_only, text) = strip_folder_suffix(&text);
            if text.is_empty() {
                return Err(PatternError::Empty(pattern.clone()));
            }
            rules.push(Rule { negated, folders_only });
            let invalid = |_| PatternError::InvalidGlob(pattern.clone());
            if is_a_path_pattern(text) {
                let place = resolve_path_pattern(text);
                let case_follows_the_filesystem = cfg!(any(windows, target_os = "macos"));
                for glob in [place.clone(), format!("{place}{EVERYTHING_BELOW}")] {
                    paths.add(GlobBuilder::new(&glob).literal_separator(true)
                            .case_insensitive(case_follows_the_filesystem).build().map_err(invalid)?);
                    paths_written_at.push(written_at);
                }
            } else {
                let anchored = match text.starts_with(ANY_DEPTH) || text == "**" {
                    true => text.to_owned(),
                    false => format!("{ANY_DEPTH}{text}")
                };
                names.add(GlobBuilder::new(&anchored).literal_separator(true).build().map_err(invalid)?);
                names_written_at.push(written_at);
            }
        }
        let invalid_set = |_| PatternError::InvalidGlob(String::new());
        Ok(PathPatternMatcher { names: names.build().map_err(invalid_set)?, paths: paths.build().map_err(invalid_set)?,
                names_written_at, paths_written_at, rules })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    // The names read 'absolute' from 'names_root' on, an offset that sits right after a separator
    pub(crate) fn find_last_match(&self, absolute: &str, names_root: usize, is_folder: bool, found: &mut Vec<usize>)
    -> Option<PatternMatch>
    {
        let mut best = None;
        if !self.names.is_empty() && names_root < absolute.len() {
            self.take_best(&self.names, &self.names_written_at, Candidate::new(&absolute[names_root..]), is_folder,
                    found, &mut best);
        }
        if !self.paths.is_empty() {
            self.take_best(&self.paths, &self.paths_written_at, Candidate::new(absolute), is_folder, found, &mut best);
        }
        best
    }

    // Every folder from the root down is read before the target itself, so a later pattern wins
    // across folders here as it does in the walk
    pub(crate) fn find_last_match_along(&self, absolute: &str, names_root: usize, ends_with_a_file: bool, found: &mut Vec<usize>)
    -> Option<PatternMatch>
    {
        let mut best: Option<PatternMatch> = None;
        let names_root = names_root.min(absolute.len());
        let separators = absolute[names_root..].char_indices()
                .filter(|(_, character)| *character == '/').map(|(at, _)| names_root + at);
        let ends = separators.chain(std::iter::once(absolute.len())).filter(|end| *end > 0);
        for end in ends {
            let is_folder = end < absolute.len() || !ends_with_a_file;
            if let Some(found) = self.find_last_match(&absolute[..end], names_root, is_folder, found)
                    && best.is_none_or(|earlier| found.written_at > earlier.written_at) {
                best = Some(found);
            }
        }
        best
    }

    fn take_best(&self, set: &GlobSet, written_at: &[usize], candidate: Candidate<'_>, is_folder: bool,
            found: &mut Vec<usize>, best: &mut Option<PatternMatch>)
    {
        set.matches_candidate_into(&candidate, found);
        for &in_set in found.iter() {
            let at = written_at[in_set];
            let rule = self.rules[at];
            if rule.folders_only && !is_folder {
                continue;
            }
            if best.is_none_or(|earlier| at > earlier.written_at) {
                *best = Some(PatternMatch { written_at: at, negated: rule.negated });
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PatternMatch {
    pub written_at: usize,
    pub negated: bool
}

#[derive(Debug, Clone, Copy)]
struct Rule {
    negated: bool,
    folders_only: bool
}

fn strip_folder_suffix(text: &str) -> (bool, &str) {
    if let Some(folder) = text.strip_suffix(EVERYTHING_BELOW) {
        return (true, folder.trim_end_matches('/'));
    }
    match text.strip_suffix('/') {
        Some(folder) => (true, folder.trim_end_matches('/')),
        None => (false, text)
    }
}

// What exists is taken whole, so a folder named 'br [v2]' is a name and no character class.
// Otherwise the place ends at the last separator before the first wildcard.
fn resolve_path_pattern(text: &str) -> String {
    let (place, rest) = match Path::new(text).exists() {
        true => (text, ""),
        false => match text.find(WILDCARDS).and_then(|wildcard| text[..wildcard].rfind('/')) {
            Some(slash) => (&text[..=slash], &text[slash + 1..]),
            None => (text, "")
        }
    };
    let absolute = convert_to_absolute(place);
    let absolute = match Path::new(&absolute).is_absolute() {
        true => absolute,
        false => remove_parent_steps(&absolutize_pattern(&absolute))
    };
    let escaped = globset::escape(&absolute);
    match rest.is_empty() {
        true => escaped,
        false => format!("{}/{rest}", escaped.trim_end_matches('/'))
    }
}

// A '..' left in the glob matches no path the walk builds, and a root or a drive is never stepped over
fn remove_parent_steps(path: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    for component in path.split('/') {
        match component {
            "." => {},
            ".." => match kept.last() {
                Some(last) if !last.is_empty() && !last.ends_with(':') && *last != ".." => { kept.pop(); },
                _ => kept.push(component)
            },
            _ => kept.push(component)
        }
    }
    kept.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matcher(patterns: &[&str]) -> PathPatternMatcher {
        PathPatternMatcher::compile(&patterns.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>()).unwrap()
    }

    fn decide(matcher: &PathPatternMatcher, absolute: &str, names_root: usize, is_folder: bool) -> Option<(usize, bool)> {
        matcher.find_last_match(absolute, names_root, is_folder, &mut Vec::new())
                .map(|found| (found.written_at, found.negated))
    }

    #[test]
    fn a_pattern_is_a_path_when_it_starts_from_a_place_and_a_name_otherwise() {
        assert!(is_a_path_pattern("./gen"));
        assert!(is_a_path_pattern("../gen"));
        assert!(is_a_path_pattern("!./gen"));
        assert!(is_a_path_pattern(" ./gen "));
        assert!(is_a_path_pattern("."));
        assert!(!is_a_path_pattern("gen"));
        assert!(!is_a_path_pattern("src/gen"));
        assert!(!is_a_path_pattern("*.min.js"));
        assert!(!is_a_path_pattern(".hidden"));
        assert!(!is_a_path_pattern("**/tests"));
        if cfg!(windows) {
            assert!(is_a_path_pattern("D:/x"));
            assert!(is_a_path_pattern("D:x"));
            assert!(is_a_path_pattern(r"\x"));
            assert!(is_a_path_pattern(r".\x"));
            assert!(is_a_path_pattern(r"\\server\share\x"));
        } else {
            assert!(is_a_path_pattern("/x"));
            assert!(!is_a_path_pattern("D:/x"));
        }
    }

    #[test]
    fn a_name_matches_at_any_depth_below_the_root_and_never_above_it() {
        let found = matcher(&["tests", "src/generated", "*.min.js"]);
        let root = "D:/dev/".len();

        assert_eq!(Some((0, false)), decide(&found, "D:/dev/proj/tests", root, true));
        assert_eq!(Some((0, false)), decide(&found, "D:/dev/proj/a/b/tests", root, true));
        assert_eq!(Some((0, false)), decide(&found, "D:/dev/proj/tests", root, false), "a name with no slash matches a file too");
        assert_eq!(None, decide(&found, "D:/dev/proj/tests2", root, true));
        assert_eq!(None, decide(&found, "D:/dev/proj/my_tests", root, true));
        assert_eq!(Some((1, false)), decide(&found, "D:/dev/proj/src/generated", root, true));
        assert_eq!(None, decide(&found, "D:/dev/proj/xsrc/generated", root, true));
        assert_eq!(Some((2, false)), decide(&found, "D:/dev/proj/app/bundle.min.js", root, false));
        assert_eq!(None, decide(&found, "D:/dev/proj/app/bundle.js", root, false));

        let above = matcher(&["dev", "dev/proj"]);
        assert_eq!(None, decide(&above, "D:/dev/proj/src", root, true), "a folder above the root took part in a match");
        assert_eq!(None, decide(&above, "D:/dev/proj", root, true));
        let own_name = matcher(&["proj", "proj/src"]);
        assert_eq!(Some((0, false)), decide(&own_name, "D:/dev/proj", root, true), "a name sees the target's own name");
        assert_eq!(Some((1, false)), decide(&own_name, "D:/dev/proj/src", root, true));
    }

    #[test]
    fn a_trailing_slash_or_a_double_star_means_a_folder_and_the_target_is_seen_whole_through_two_stars() {
        let found = matcher(&["tests/", "spec/**", "**"]);
        let root = "D:/dev/".len();

        assert_eq!(Some((2, false)), decide(&found, "D:/dev/proj/tests", root, true));
        assert_eq!(Some((2, false)), decide(&found, "D:/dev/proj/tests", root, false), "'**' matches a file too");
        let folders = matcher(&["tests/", "spec/**"]);
        assert_eq!(Some((0, false)), decide(&folders, "D:/dev/proj/tests", root, true));
        assert_eq!(None, decide(&folders, "D:/dev/proj/tests", root, false), "a shell script named 'tests' was taken as a folder");
        assert_eq!(Some((1, false)), decide(&folders, "D:/dev/proj/spec", root, true));
        assert_eq!(None, decide(&folders, "D:/dev/proj/spec/a.rb", root, false), "'spec/**' is the folder, and a file in it inherits");
        assert_eq!(None, decide(&folders, "D:/dev/proj/spec", root, false));
    }

    #[test]
    fn the_last_written_pattern_wins_and_a_negated_one_says_so() {
        let found = matcher(&["tests/", "!tests/fixtures/", "!*_test.go", "vendor/tests/"]);
        let root = "D:/dev/".len();

        assert_eq!(Some((0, false)), decide(&found, "D:/dev/proj/tests", root, true));
        assert_eq!(Some((1, true)), decide(&found, "D:/dev/proj/tests/fixtures", root, true));
        assert_eq!(Some((2, true)), decide(&found, "D:/dev/proj/src/parser_test.go", root, false));
        assert_eq!(None, decide(&found, "D:/dev/proj/src/parser.go", root, false));
        assert_eq!(Some((3, false)), decide(&found, "D:/dev/proj/vendor/tests", root, true), "two patterns match and the later one decides");

        let along = |absolute: &str, ends_with_a_file: bool| found.find_last_match_along(absolute, root, ends_with_a_file, &mut Vec::new())
                .map(|x| (x.written_at, x.negated));
        assert_eq!(Some((1, true)), along("D:/dev/proj/tests/fixtures/a.rs", true), "the folders above the file were not read");
        assert_eq!(Some((0, false)), along("D:/dev/proj/tests/unit/a.rs", true));
        assert_eq!(Some((0, false)), along("D:/dev/proj/tests", false));
        assert_eq!(None, along("D:/dev/proj/src/a.rs", true));
        assert_eq!(Some((2, true)), along("D:/dev/proj/tests/parser_test.go", true), "the later pattern wins over the folder above");
        assert_eq!(None, along("D:/dev/proj", false));
    }

    #[test]
    fn a_name_keeps_its_case_everywhere_and_a_path_follows_the_filesystem() {
        let root = std::env::temp_dir().join("mezura-pattern-case");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Proj").join("Gen")).unwrap();
        let root_str = root.to_str().unwrap().replace('\\', "/");
        let names_root = root_str.len() + 1;

        let names = matcher(&["gen"]);
        assert_eq!(Some((0, false)), decide(&names, &format!("{root_str}/Proj/gen"), names_root, true));
        assert_eq!(None, decide(&names, &format!("{root_str}/Proj/Gen"), names_root, true), "'gen' matched 'Gen' by name");

        let paths = matcher(&[&format!("{root_str}/proj/gen")]);
        let cased = if cfg!(any(windows, target_os = "macos")) { Some((0, false)) } else { None };
        assert_eq!(cased, decide(&paths, &format!("{root_str}/Proj/Gen"), names_root, true));
        assert_eq!(cased, decide(&paths, &format!("{root_str}/Proj/Gen/deep/a.rs"), names_root, false), "a path names everything below it");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_path_pattern_is_resolved_from_where_it_was_written_and_its_brackets_are_a_name() {
        let root = std::env::temp_dir().join("mezura-pattern-place");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("br [v2]").join("gen")).unwrap();
        std::fs::create_dir_all(root.join("a").join("gen")).unwrap();
        std::fs::create_dir_all(root.join("b").join("gen")).unwrap();
        let root_str = root.to_str().unwrap().replace('\\', "/");
        let names_root = root_str.len() + 1;

        let bracketed = matcher(&[&format!("{root_str}/br [v2]/gen"), &format!("{root_str}/br [v2]/gen/**")]);
        assert_eq!(Some((1, false)), decide(&bracketed, &format!("{root_str}/br [v2]/gen"), names_root, true));
        assert_eq!(Some((0, false)), decide(&bracketed, &format!("{root_str}/br [v2]/gen/out.rs"), names_root, false),
                "the folder-only pattern was read for a file, or the bracket was read as a class");
        assert_eq!(None, decide(&bracketed, &format!("{root_str}/br v/gen"), names_root, true));

        let wildcard = matcher(&[&format!("{root_str}/*/gen")]);
        assert_eq!(Some((0, false)), decide(&wildcard, &format!("{root_str}/a/gen"), names_root, true));
        assert_eq!(Some((0, false)), decide(&wildcard, &format!("{root_str}/b/gen/x.rs"), names_root, false));
        assert_eq!(None, decide(&wildcard, &format!("{root_str}/a/b/gen"), names_root, true), "'*' crossed a separator");

        let cwd = std::env::current_dir().unwrap().to_str().unwrap().replace('\\', "/");
        let relative = matcher(&["./src", "../elsewhere/gen"]);
        assert_eq!(Some((0, false)), decide(&relative, &format!("{cwd}/src"), 0, true));
        assert_eq!(Some((0, false)), decide(&relative, &format!("{cwd}/src/lib.rs"), 0, false));
        assert_eq!(None, decide(&relative, &format!("{cwd}/other/src"), 0, true), "'./src' matched at any depth");
        let parent = std::path::Path::new(&cwd).parent().unwrap().to_str().unwrap().to_owned();
        assert_eq!(Some((1, false)), decide(&relative, &format!("{parent}/elsewhere/gen"), 0, true));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_parent_step_in_a_place_that_is_not_on_disk_is_taken_lexically() {
        assert_eq!("D:/dev/elsewhere/gen", remove_parent_steps("D:/dev/proj/../elsewhere/gen"));
        assert_eq!("D:/x", remove_parent_steps("D:/a/b/../../x"));
        assert_eq!("D:/../x", remove_parent_steps("D:/../x"), "a drive was stepped over");
        assert_eq!("/x", remove_parent_steps("/a/./../x"));
        assert_eq!("/../x", remove_parent_steps("/../x"));
        assert_eq!("//server/share/x", remove_parent_steps("//server/share/a/../x"));
        assert_eq!("D:/a", remove_parent_steps("D:/a/b/.."));
    }

    #[test]
    fn a_pattern_that_names_nothing_or_does_not_parse_is_refused_as_written() {
        assert_eq!(Err(PatternError::Empty("!".to_owned())), PathPatternMatcher::compile(&["!".to_owned()]).map(|_| ()));
        assert_eq!(Err(PatternError::Empty("/".to_owned())), PathPatternMatcher::compile(&["/".to_owned()]).map(|_| ()));
        assert_eq!(Err(PatternError::Empty(" ! / ".to_owned())), PathPatternMatcher::compile(&[" ! / ".to_owned()]).map(|_| ()));
        assert_eq!(Err(PatternError::InvalidGlob("[bad".to_owned())),
                PathPatternMatcher::compile(&["fine".to_owned(), "[bad".to_owned()]).map(|_| ()));
        assert!(validate_test_patterns(&["tests/".to_owned(), "!**/*_test.go".to_owned()]).is_ok());
        assert!(matcher(&[]).is_empty());
        assert!(!matcher(&["x"]).is_empty());
    }
}
