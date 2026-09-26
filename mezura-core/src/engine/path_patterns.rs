//! One reading of a glob pattern list for every command that takes one. It tells paths from names,
//! finds the root a name is read from, and lets the last written match win.

use std::path::{Component, Path};
use std::sync::atomic::{AtomicBool, Ordering};

use globset::{Candidate, GlobBuilder, GlobSet, GlobSetBuilder};

use crate::engine::targets::{absolutize_pattern, convert_to_absolute, is_inside_or_at, normalise_separators};

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
    Empty(String),
    /// Starts with `!` in a list where nothing can be taken back.
    TakesBack(String)
}

impl std::error::Error for PatternError {}

impl std::fmt::Display for PatternError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidGlob(x) => write!(f, "'{x}' is not a valid glob pattern."),
            Self::Empty(x) => write!(f, "'{x}' names nothing once its '!' and its trailing '/' are read."),
            Self::TakesBack(x) => write!(f, "'{x}' starts with '!', which takes an earlier pattern back, but what an \
                    exclude pattern leaves out cannot be taken back, since a directory left out is never entered.")
        }
    }
}

/// Whether every pattern of `--tests` reads, for refusing a bad one at the moment somebody typed it.
pub fn validate_test_patterns(patterns: &[String]) -> Result<(), PatternError> {
    PathPatternMatcher::compile(patterns, TakingBack::Allowed).map(|_| ())
}

/// Whether every exclude pattern reads, a `!` among them refused, for refusing a bad one when typed.
pub fn validate_exclude_patterns(patterns: &[String]) -> Result<(), PatternError> {
    PathPatternMatcher::compile(patterns, TakingBack::Refused).map(|_| ())
}

/// Whether a pattern names a place, which is resolved from where the pattern was written.
pub fn is_a_path_pattern(pattern: &str) -> bool {
    let text = normalise_separators(pattern.trim().trim_start_matches(NEGATION));
    matches!(Path::new(text.as_ref()).components().next(),
            Some(Component::Prefix(_) | Component::RootDir | Component::CurDir | Component::ParentDir))
}

/// The pattern with the place it names made absolute, the way a target is stored. A relative place
/// is joined to the working directory and its `..` steps are taken. A name comes back as written.
pub fn absolutize_path_pattern(pattern: &str) -> String {
    let written = pattern.trim();
    if !is_a_path_pattern(written) {
        return written.to_owned();
    }
    let (negation, text) = match written.strip_prefix(NEGATION) {
        Some(rest) => ("!", rest.trim()),
        None => ("", written)
    };
    let text = normalise_separators(text);
    let (_, without_suffix) = strip_folder_suffix(&text);
    let suffix = &text[without_suffix.len()..];
    let (place, rest, _) = split_place(without_suffix);
    format!("{negation}{}{suffix}", join_place_and_rest(&absolutize_place(place), rest))
}

/// Whether the place a path pattern names, up to its first wildcard, is on disk.
pub fn is_on_disk(pattern: &str) -> bool {
    let text = normalise_separators(pattern.trim().trim_start_matches(NEGATION).trim());
    let (_, without_suffix) = strip_folder_suffix(&text);
    let (place, rest, whole_on_disk) = split_place(without_suffix);
    whole_on_disk || !rest.is_empty() && Path::new(place).exists()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TakingBack {
    Allowed,
    Refused
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnusedPattern {
    pub written: String,
    pub reason: Unused
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unused {
    NotOnDisk,
    OutsideEveryTarget,
    MatchedNothing
}

// Each glob remembers where in the written list it came from, since the last one written decides
pub(crate) struct PathPatternMatcher {
    names: GlobSet,
    paths: GlobSet,
    names_written_at: Vec<usize>,
    paths_written_at: Vec<usize>,
    rules: Vec<Rule>,
    matched: Vec<AtomicBool>
}

impl PathPatternMatcher {
    pub(crate) fn compile(patterns: &[String], taking_back: TakingBack) -> Result<Self, PatternError> {
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
            if negated && taking_back == TakingBack::Refused {
                return Err(PatternError::TakesBack(pattern.clone()));
            }
            let text = normalise_separators(text);
            let (folders_only, text) = strip_folder_suffix(&text);
            if text.is_empty() {
                return Err(PatternError::Empty(pattern.clone()));
            }
            let invalid = |_| PatternError::InvalidGlob(pattern.clone());
            let kind = if is_a_path_pattern(text) {
                let (place, on_disk, glob) = resolve_path_pattern(text);
                let case_follows_the_filesystem = cfg!(any(windows, target_os = "macos"));
                for glob in [glob.clone(), format!("{glob}{EVERYTHING_BELOW}")] {
                    paths.add(GlobBuilder::new(&glob).literal_separator(true)
                            .case_insensitive(case_follows_the_filesystem).build().map_err(invalid)?);
                    paths_written_at.push(written_at);
                }
                Kind::Path { place, on_disk }
            } else {
                let anchored = match text.starts_with(ANY_DEPTH) || text == "**" {
                    true => text.to_owned(),
                    false => format!("{ANY_DEPTH}{text}")
                };
                names.add(GlobBuilder::new(&anchored).literal_separator(true).build().map_err(invalid)?);
                names_written_at.push(written_at);
                Kind::Name { reaches_into_a_path: text.trim_start_matches(ANY_DEPTH).contains('/') }
            };
            rules.push(Rule { written: pattern.clone(), negated, folders_only, kind });
        }
        let invalid_set = |_| PatternError::InvalidGlob(String::new());
        let matched = (0..rules.len()).map(|_| AtomicBool::new(false)).collect();
        Ok(PathPatternMatcher { names: names.build().map_err(invalid_set)?, paths: paths.build().map_err(invalid_set)?,
                names_written_at, paths_written_at, rules, matched })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    // Any match leaves an entry out, since a list that refuses '!' has nothing to take back
    pub(crate) fn matches(&self, path: &Path, names_root: usize, is_folder: bool, found: &mut Vec<usize>) -> bool {
        !self.is_empty() && path.to_str()
                .is_some_and(|absolute| self.find_last_match(absolute, names_root, is_folder, found).is_some())
    }

    pub(crate) fn find_places_out_of_reach(&self, roots: &[String]) -> Vec<UnusedPattern> {
        self.rules.iter().filter_map(|rule| {
            let Kind::Path { place, on_disk } = &rule.kind else {
                return None;
            };
            let reason = if !on_disk {
                Unused::NotOnDisk
            } else if roots.iter().any(|root| is_inside_or_at(place, root) || is_inside_or_at(root, place)) {
                return None;
            } else {
                Unused::OutsideEveryTarget
            };
            Some(UnusedPattern { written: rule.written.clone(), reason })
        }).collect()
    }

    // A plain name is left alone, since a list shared between projects names folders some lack
    pub(crate) fn find_names_that_matched_nothing(&self) -> Vec<UnusedPattern> {
        self.rules.iter().zip(&self.matched).filter_map(|(rule, matched)| {
            match rule.kind {
                Kind::Name { reaches_into_a_path: true } if !matched.load(Ordering::Relaxed) =>
                        Some(UnusedPattern { written: rule.written.clone(), reason: Unused::MatchedNothing }),
                _ => None
            }
        }).collect()
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
            let rule = &self.rules[at];
            if rule.folders_only && !is_folder {
                continue;
            }
            self.matched[at].store(true, Ordering::Relaxed);
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

#[derive(Debug, Clone)]
struct Rule {
    written: String,
    negated: bool,
    folders_only: bool,
    kind: Kind
}

#[derive(Debug, Clone)]
enum Kind {
    Name { reaches_into_a_path: bool },
    Path { place: String, on_disk: bool }
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

// The place is escaped in the glob, so that a folder named 'br [v2]' is a name and no character class
fn resolve_path_pattern(text: &str) -> (String, bool, String) {
    let (place, rest, whole_on_disk) = split_place(text);
    let on_disk = whole_on_disk || !rest.is_empty() && Path::new(place).exists();
    let absolute = absolutize_place(place);
    let glob = join_place_and_rest(&globset::escape(&absolute), rest);
    (absolute, on_disk, glob)
}

// What exists is taken whole, and otherwise the place ends at the last separator before the first wildcard
fn split_place(text: &str) -> (&str, &str, bool) {
    if Path::new(text).exists() {
        return (text, "", true);
    }
    match text.find(WILDCARDS).and_then(|wildcard| text[..wildcard].rfind('/')) {
        Some(slash) => (&text[..=slash], &text[slash + 1..], false),
        None => (text, "", false)
    }
}

fn absolutize_place(place: &str) -> String {
    let absolute = convert_to_absolute(place);
    let absolute = match Path::new(&absolute).is_absolute() {
        true => absolute,
        false => absolutize_pattern(&absolute)
    };
    remove_parent_steps(&absolute)
}

fn join_place_and_rest(place: &str, rest: &str) -> String {
    match rest.is_empty() {
        true => place.to_owned(),
        false => format!("{}/{rest}", place.trim_end_matches('/'))
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
        PathPatternMatcher::compile(&owned(patterns), TakingBack::Allowed).unwrap()
    }

    fn owned(patterns: &[&str]) -> Vec<String> {
        patterns.iter().map(|x| (*x).to_owned()).collect()
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
        let compile = |patterns: &[&str]| PathPatternMatcher::compile(&owned(patterns), TakingBack::Allowed).map(|_| ());
        assert_eq!(Err(PatternError::Empty("!".to_owned())), compile(&["!"]));
        assert_eq!(Err(PatternError::Empty("/".to_owned())), compile(&["/"]));
        assert_eq!(Err(PatternError::Empty(" ! / ".to_owned())), compile(&[" ! / "]));
        assert_eq!(Err(PatternError::InvalidGlob("[bad".to_owned())), compile(&["fine", "[bad"]));
        assert!(validate_test_patterns(&owned(&["tests/", "!**/*_test.go"])).is_ok());
        assert!(matcher(&[]).is_empty());
        assert!(!matcher(&["x"]).is_empty());
    }

    #[test]
    fn an_exclusion_cannot_be_taken_back_and_is_matched_or_not() {
        assert_eq!(Err(PatternError::TakesBack("!vendor/".to_owned())),
                validate_exclude_patterns(&owned(&["build", "!vendor/"])));
        assert_eq!(Err(PatternError::TakesBack(" ! ./gen".to_owned())), validate_exclude_patterns(&owned(&[" ! ./gen"])));
        assert_eq!(Err(PatternError::InvalidGlob("[bad".to_owned())), validate_exclude_patterns(&owned(&["[bad"])));
        assert!(validate_exclude_patterns(&owned(&["node_modules", "*.min.js", "src/generated", "./target"])).is_ok());

        let root = "D:/dev/".len();
        let excluded = PathPatternMatcher::compile(&owned(&["node_modules", "*.min.js", "build/"]), TakingBack::Refused).unwrap();
        let path = |x: &str| std::path::PathBuf::from(x);
        assert!(excluded.matches(&path("D:/dev/proj/node_modules"), root, true, &mut Vec::new()));
        assert!(excluded.matches(&path("D:/dev/proj/app/x.min.js"), root, false, &mut Vec::new()));
        assert!(!excluded.matches(&path("D:/dev/proj/app/x.js"), root, false, &mut Vec::new()));
        assert!(!excluded.matches(&path("D:/dev/proj/build"), root, false, &mut Vec::new()), "a script named 'build' was left out");
        assert!(!matcher(&[]).matches(&path("D:/dev/proj/anything"), root, true, &mut Vec::new()));
    }

    #[test]
    fn a_path_pattern_is_stored_absolute_and_a_name_as_written() {
        let cwd = std::env::current_dir().unwrap().to_str().unwrap().replace('\\', "/");
        assert_eq!(format!("{cwd}/src"), absolutize_path_pattern("./src"));
        assert_eq!(format!("{cwd}/src/"), absolutize_path_pattern(" ./src/ "));
        assert_eq!(format!("!{cwd}/gen/**"), absolutize_path_pattern("!./gen/**"));
        assert_eq!(format!("{cwd}/nowhere/*/gen"), absolutize_path_pattern("./nowhere/*/gen"), "the wildcards after the place were lost");
        let parent = Path::new(&cwd).parent().unwrap().to_str().unwrap().to_owned();
        assert_eq!(format!("{parent}/elsewhere/gen"), absolutize_path_pattern("../elsewhere/gen"), "the '..' was kept");
        assert_eq!("src/gen", absolutize_path_pattern(" src/gen "));
        assert_eq!("!*.min.js", absolutize_path_pattern("!*.min.js"));
        assert_eq!("D:/x/gen", absolutize_path_pattern(if cfg!(windows) {"D:\\x\\gen"} else {"D:/x/gen"}));

        assert!(is_on_disk("./src"));
        assert!(is_on_disk("!./src/"));
        assert!(is_on_disk("./*/nowhere"), "the place before the wildcard is the working directory");
        assert!(!is_on_disk("./nowhere/at/all"));
        assert!(!is_on_disk(&format!("{cwd}/nowhere/*/gen")));
    }

    #[test]
    fn a_pattern_that_changes_nothing_is_named_with_its_reason() {
        let root = std::env::temp_dir().join("mezura-pattern-unused");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("proj").join("src").join("gen")).unwrap();
        std::fs::create_dir_all(root.join("elsewhere")).unwrap();
        let root_str = root.to_str().unwrap().replace('\\', "/");
        let target = format!("{root_str}/proj");
        let targets = vec![target.clone()];

        let placed = matcher(&[&format!("{root_str}/proj/src/gen"), &format!("{root_str}/proj/*/gen"), &format!("{root_str}/nowhere"),
                &format!("{root_str}/elsewhere"), &format!("{root_str}/nowhere/*/gen"), &root_str, "src/gen"]);
        let unused = |written: &str, reason: Unused| UnusedPattern { written: written.to_owned(), reason };
        assert_eq!(vec![unused(&format!("{root_str}/nowhere"), Unused::NotOnDisk),
                unused(&format!("{root_str}/elsewhere"), Unused::OutsideEveryTarget),
                unused(&format!("{root_str}/nowhere/*/gen"), Unused::NotOnDisk)], placed.find_places_out_of_reach(&targets),
                "a place above the target, or a wildcard pattern below it, was reported");

        let names = matcher(&["src/gen", "tests", "**/spec/helpers", "vendor/lib/", "!src/gen/out"]);
        let names_root = root_str.len() + 1;
        let mut found = Vec::new();
        names.find_last_match(&format!("{target}/src/gen"), names_root, true, &mut found);
        names.find_last_match(&format!("{target}/vendor/lib"), names_root, false, &mut found);
        assert_eq!(vec![unused("**/spec/helpers", Unused::MatchedNothing), unused("vendor/lib/", Unused::MatchedNothing),
                unused("!src/gen/out", Unused::MatchedNothing)], names.find_names_that_matched_nothing(),
                "a plain name was reported, or a folder-only match on a file counted");
        assert!(placed.find_names_that_matched_nothing().len() == 1, "{:?}", placed.find_names_that_matched_nothing());

        std::fs::remove_dir_all(&root).unwrap();
    }
}
