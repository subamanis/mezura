// Which paths the walk is actually given, once the ones that lie inside other ones have been taken
// out, and which of the things it finds are excluded. Everything here decides what gets counted.

use std::path::Path;

use crate::GitignoreStack;
use crate::engine::config::Target;


// The directories the traversal starts from, which is the same list with the nesting gone whatever
// the names are. A nested target is never walked on its own: the walk of the one that contains it
// reaches those files anyway, and the module they belong to is decided on the way down.
pub(crate) fn topmost_targets(targets: &[Target]) -> Vec<Target> {
    keep_topmost(targets.to_vec(), |_, _| true)
}

// Targets that are contained in other targets would have their files counted twice, so only the
// topmost of every overlapping group is kept. A nested one that names a different module is not a
// repetition of its parent, it is the boundary that takes those files away from it, so it stays:
// dropping it is what would quietly count the tests of 'backend=./api tests=./api/tests' as backend.
pub(crate) fn remove_overlapping_targets(targets: Vec<Target>) -> Vec<Target> {
    keep_topmost(targets, |enclosing, target| enclosing.module == target.module)
}

// Whether every pattern parses, which is all a caller can act on. The matcher itself stays inside,
// because its type belongs to a dependency and putting it in the signature would make a release of
// globset a breaking change of ours.
pub fn validate_exclude_patterns(exclude_patterns: &[String]) -> Result<(), TargetError> {
    build_exclude_matcher(exclude_patterns).map(|_| ())
            .map_err(|x| TargetError::InvalidGlob(x.glob().unwrap_or("").to_owned()))
}

pub(crate) fn build_exclude_matcher(exclude_patterns: &[String]) -> Result<globset::GlobSet, globset::Error> {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in exclude_patterns {
        let normalized = pattern.trim().replace('\\', "/");
        let normalized = normalized.trim_end_matches('/');
        let anchored = if normalized.starts_with("**/") {
            normalized.to_owned()
        } else {
            format!("**/{normalized}")
        };
        builder.add(globset::GlobBuilder::new(&anchored).literal_separator(true).build()?);
    }
    builder.build()
}

// The key that answers "are these two the same place". Case-insensitive on Windows, where the file
// system is, and without a trailing separator, because 'D:/a' and 'D:/a/' are one directory. The
// second half was missing and it counted every file under such a pair twice: the deduplication
// compares these keys, and the containment test asks for a path strictly longer than its ancestor
// plus a separator, which 'D:/a/' is not against 'D:/a'.
pub(crate) fn path_comparison_key(path: &str) -> String {
    let path = path.trim_end_matches('/');
    if cfg!(windows) {path.to_lowercase()} else {path.to_owned()}
}

// Sorted by path and with the duplicates gone, so that the nearest enclosing target of any entry is
// the last one kept before it. 'covered' decides what "enclosing" is allowed to remove.
//
// The sort belongs to the algorithm and not to the answer, so the order the targets were written in
// is carried through it and restored at the end. It used to come out sorted by path, which is a
// third order that is neither what was asked for nor anything a reader could act on: declaring
// 'zeta=... alpha=...' produced a report whose first column was alpha.
fn keep_topmost(targets: Vec<Target>, covered: impl Fn(&Target, &Target) -> bool) -> Vec<Target> {
    let mut sorted = targets.into_iter().enumerate()
            .map(|(declared_at, x)| (path_comparison_key(&x.path), declared_at, x)).collect::<Vec<_>>();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    // A stable sort leaves the earliest declaration of a repeated path first, and this keeps that one
    sorted.dedup_by(|a, b| a.0 == b.0);

    let mut kept : Vec<(String,usize,Target)> = Vec::with_capacity(sorted.len());
    for (key, declared_at, target) in sorted {
        let enclosing = kept.iter().rev().find(|(kept_key,_,_)| is_ancestor_of(kept_key, &key));
        if !enclosing.is_some_and(|(_, _, enclosing)| covered(enclosing, &target)) {
            kept.push((key, declared_at, target));
        }
    }

    kept.sort_by_key(|(_, declared_at, _)| *declared_at);
    kept.into_iter().map(|(_,_,target)| target).collect()
}

fn is_ancestor_of(ancestor: &str, path: &str) -> bool {
    let ancestor = ancestor.trim_end_matches('/');
    path.len() > ancestor.len() + 1 && path.starts_with(ancestor)
            && path.as_bytes()[ancestor.len()] == b'/'
}


// What went wrong while working out which paths to walk. Reported rather than printed, and the
// command line turns it into its own error with the wording a person reads.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TargetError {
    InvalidPath(String),
    InvalidGlob(String),
    NoGlobMatches(String),
    AllGlobMatchesIgnored(String),
    Contested(String, String, String)
}

// Literal paths must exist and are always used, even if they are ignored or dotted, since the user
// named them explicitly. Glob patterns are expanded to the existing paths they match, and those
// matches are discovered by the program, so they are subject to the same rules as every other
// discovered path. Finally, targets contained in other targets are dropped, so that no file
// is counted twice, unless the nested one carries a module of its own and is therefore the boundary
// that takes those files off the one around it.
pub fn resolve(entries: &[(Option<String>, String)], respect_gitignore: bool, search_in_dotted: bool)
-> Result<Vec<Target>, TargetError>
{
    fn is_dotted(path: &Path) -> bool {
        path.file_name().and_then(|x| x.to_str()).is_some_and(|x| x.starts_with('.'))
    }

    let mut resolved = Vec::with_capacity(entries.len());
    for (module, entry) in entries {
        let trimmed = entry.trim();
        // Two spellings of one name are one module, the way two spellings of one extension are one
        // extension. The first one seen is the one the report prints.
        let module = module.as_ref().map(|name| resolved.iter()
                .find_map(|x: &Target| x.module.clone().filter(|seen| seen.to_lowercase() == name.to_lowercase()))
                .unwrap_or_else(|| name.clone()));
        if has_glob_metacharacters(trimmed) {
            let paths = match glob::glob(&trimmed.replace('\\', "/")) {
                Ok(x) => x,
                Err(_) => return Err(TargetError::InvalidGlob(trimmed.to_owned()))
            };
            let matches = paths.flatten().filter(|x| x.is_dir() || x.is_file()).collect::<Vec<_>>();
            if matches.is_empty() {
                return Err(TargetError::NoGlobMatches(trimmed.to_owned()));
            }

            let relevant = matches.iter()
                    .filter(|x| search_in_dotted || !is_dotted(x))
                    .filter(|x| !respect_gitignore || !GitignoreStack::is_path_ignored(x))
                    // A pattern is not a name: what it matched was found by the program, and a link
                    // it found is a link the walk would have skipped for counting twice whatever it
                    // points at. Named on its own it is still followed, as any target is.
                    .filter(|x| !x.is_symlink())
                    .filter_map(|x| x.to_str().map(convert_to_absolute))
                    .map(|path| Target { module: module.clone(), path }).collect::<Vec<_>>();
            if relevant.is_empty() {
                return Err(TargetError::AllGlobMatchesIgnored(trimmed.to_owned()));
            }
            resolved.extend(relevant);
        } else if is_valid_path(trimmed) {
            resolved.push(Target { module: module.clone(), path: convert_to_absolute(trimmed) });
        } else {
            return Err(TargetError::InvalidPath(trimmed.to_owned()));
        }
    }

    // Two names over one path is not something a rule can settle: there is no more specific one of
    // the two, and whichever won would take the other's files away without a word
    for (position, target) in resolved.iter().enumerate() {
        let key = path_comparison_key(&target.path);
        if let Some(other) = resolved[position + 1..].iter()
                .find(|x| x.module != target.module && path_comparison_key(&x.path) == key) {
            return Err(TargetError::Contested(target.path.clone(),
                    name_or_rest(&target.module), name_or_rest(&other.module)));
        }
    }

    Ok(remove_overlapping_targets(resolved))
}

fn name_or_rest(module: &Option<String>) -> String {
    module.clone().unwrap_or_else(|| crate::UNNAMED_MODULE_NAME.to_owned())
}

// The opener is the signal, which is what the help text and the README both describe as
// '(* ? [..] {..})'. A closing bracket on its own is an ordinary character in a path, and treating
// it as a pattern sends the target down the glob branch, where being gitignored, dotted or a symlink
// silently removes it: a literal path the user named is meant to survive all three.
fn has_glob_metacharacters(s: &str) -> bool {
    s.contains(['*', '?', '[', '{'])
}

fn is_valid_path(s: &str) -> bool {
    let p = Path::new(s.trim());
    p.is_dir() || p.is_file()
}


// The one way to produce the form 'Target.path' demands. The type says 'absolute and resolved, never
// what was typed' and a caller building targets by hand has no other way to satisfy it.
pub // The "canonicalize" function from the std that this function uses, (at least on window) seems to put the weird prefix
// "\\?\" before the path and it also puts forward slashes that we want to convert for compatibility.
fn convert_to_absolute(s: &str) -> String {
    let p = Path::new(s);
    if p.is_absolute() {
        return s.replace("\\", "/");
    }

    // The canonical form of a path that was typed as valid UTF-8 need not be valid UTF-8 itself,
    // since canonicalizing resolves links and the target's real name is whatever the file system
    // holds. Falling back to what was typed keeps a string that still names the place, which
    // 'to_string_lossy' would not: this one is handed back to 'is_dir' and 'is_file' further down.
    match std::fs::canonicalize(p).ok().and_then(|buf| buf.to_str().map(str::to_owned)) {
        Some(str_path) => str_path.strip_prefix(r"\\?\").unwrap_or(&str_path).replace("\\", "/"),
        None => s.replace("\\", "/")
    }
}


#[cfg(test)]
mod target_path_tests {
    use super::*;

    #[test]
    fn test_has_glob_metacharacters() {
        assert!(has_glob_metacharacters("src/*"));
        assert!(has_glob_metacharacters("a?b"));
        assert!(has_glob_metacharacters("[abc]"));
        assert!(has_glob_metacharacters("{a,b}"));
        assert!(has_glob_metacharacters("D:/dev/**/src"));

        assert!(!has_glob_metacharacters("src"));
        assert!(!has_glob_metacharacters("D:/dev/Rusty/mezura"));
        assert!(!has_glob_metacharacters("../a b/c-d.rs"));
        // A closing bracket with no opener is a character in a name, not a pattern
        assert!(!has_glob_metacharacters("./build}"));
        assert!(!has_glob_metacharacters("./cache]"));
    }

    fn dedupe(paths: &[&str]) -> Vec<String> {
        kept_paths(remove_overlapping_targets(paths.iter().map(|x| Target::of((*x).to_owned())).collect()))
    }

    // 'name path' declares the module, a bare path declares none
    fn dedupe_named(entries: &[&str]) -> Vec<String> {
        let targets = entries.iter().map(|entry| match entry.split_once(' ') {
            Some((name, path)) => Target::named(name, path.to_owned()),
            None => Target::of((*entry).to_owned())
        }).collect();
        kept_paths(remove_overlapping_targets(targets))
    }

    fn kept_paths(targets: Vec<Target>) -> Vec<String> {
        targets.into_iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn test_remove_overlapping_paths_keeps_unrelated() {
        assert_eq!(Vec::<String>::new(), dedupe(&[]));
        assert_eq!(vec!["D:/a"], dedupe(&["D:/a"]));
        // The order they were written in, not the order they sort in
        assert_eq!(vec!["D:/b", "D:/a"], dedupe(&["D:/b", "D:/a"]));
        assert_eq!(vec!["D:/a", "E:/a"], dedupe(&["D:/a", "E:/a"]));
    }

    #[test]
    fn test_remove_overlapping_paths_drops_identical() {
        assert_eq!(vec!["D:/a"], dedupe(&["D:/a", "D:/a"]));
        assert_eq!(vec!["D:/b", "D:/a"], dedupe(&["D:/b", "D:/a", "D:/b", "D:/a"]));
    }

    #[test]
    fn test_remove_overlapping_paths_drops_nested() {
        assert_eq!(vec!["D:/a"], dedupe(&["D:/a", "D:/a/b"]));
        assert_eq!(vec!["D:/a"], dedupe(&["D:/a/b", "D:/a"]));
        assert_eq!(vec!["D:/a"], dedupe(&["D:/a", "D:/a/b/c/d", "D:/a/b"]));
        assert_eq!(vec!["D:/a"], dedupe(&["D:/a", "D:/a/file.rs"]));
        assert_eq!(vec!["D:/a", "D:/b"], dedupe(&["D:/a", "D:/a/x", "D:/b", "D:/b/y/z"]));
    }

    #[test]
    fn test_remove_overlapping_paths_respects_component_boundaries() {
        // 'D:/ab' is not inside 'D:/a', despite the string prefix
        assert_eq!(vec!["D:/a", "D:/ab"], dedupe(&["D:/a", "D:/ab"]));
        assert_eq!(vec!["D:/a", "D:/a-b"], dedupe(&["D:/a", "D:/a-b"]));
        // the '-' sorts before the '/', so a naive scan against only the previous kept path
        // would let 'D:/a/b' through, even though it is inside 'D:/a'
        assert_eq!(vec!["D:/a", "D:/a-b"], dedupe(&["D:/a", "D:/a-b", "D:/a/b"]));
        assert_eq!(vec!["D:/a-b", "D:/a", "D:/a!b"], dedupe(&["D:/a/deep/one", "D:/a-b", "D:/a", "D:/a!b"]));
    }

    #[test]
    fn test_remove_overlapping_paths_handles_trailing_slashes_and_case() {
        assert_eq!(vec!["D:/a/"], dedupe(&["D:/a/", "D:/a/b"]));
        // The same place written twice, once with the slash. Only the byte-identical duplicate was
        // ever tested, so this pair survived the pruning and every file under it was counted twice.
        assert_eq!(vec!["D:/a"], dedupe(&["D:/a", "D:/a/"]));
        assert_eq!(vec!["D:/a/"], dedupe(&["D:/a/", "D:/a"]));
        assert_eq!(vec!["D:/a"], dedupe(&["D:/a", "D:/a/", "D:/a/b"]));

        let result = dedupe(&["D:/Dev", "D:/dev/sub"]);
        if cfg!(windows) {
            assert_eq!(vec!["D:/Dev"], result);
        } else {
            assert_eq!(vec!["D:/Dev", "D:/dev/sub"], result);
        }
    }

    // Dropping the nested one is what would quietly count the tests as backend, which is the exact
    // opposite of what was asked for
    #[test]
    fn a_nested_target_survives_the_pruning_when_it_names_another_module() {
        assert_eq!(vec!["backend=D:/api", "tests=D:/api/tests"],
                dedupe_named(&["backend D:/api", "tests D:/api/tests"]));
        assert_eq!(vec!["D:/api", "tests=D:/api/tests"], dedupe_named(&["D:/api", "tests D:/api/tests"]));

        // and it is dropped when it would have been counted the same way anyway
        assert_eq!(vec!["tests=D:/api"], dedupe_named(&["tests D:/api", "tests D:/api/deep"]));
    }

    // The nearest enclosing target decides, not the outermost one: below a boundary that changed the
    // module, a target that changes it back is a boundary of its own and has to stay
    #[test]
    fn a_target_that_reverts_the_module_of_the_one_above_it_is_kept() {
        assert_eq!(vec!["D:/a", "tests=D:/a/b", "D:/a/b/c"],
                dedupe_named(&["D:/a", "tests D:/a/b", "D:/a/b/c"]));
    }

    #[test]
    fn the_roots_of_the_traversal_never_contain_one_another() {
        let targets = vec![Target::named("backend", "D:/api".to_owned()),
                Target::named("tests", "D:/api/tests".to_owned())];
        assert_eq!(vec!["backend=D:/api"], kept_paths(topmost_targets(&targets)));
    }
}

#[cfg(test)]
mod exclude_matcher_tests {
    use super::*;

    #[test]
    fn test_name_patterns_match_at_any_depth() {
        let matcher = build_exclude_matcher(&["node_modules".to_owned(), "*.min.js".to_owned()]).unwrap();

        assert!(matcher.is_match("node_modules"));
        assert!(matcher.is_match("D:/proj/node_modules"));
        assert!(!matcher.is_match("D:/proj/node_modules_2"));
        assert!(matcher.is_match("D:/proj/app/bundle.min.js"));
        assert!(!matcher.is_match("D:/proj/app/bundle.js"));
        assert!(!matcher.is_match("D:/proj/appbundle.min.js/other.js"));
    }

    #[test]
    fn test_path_patterns_are_component_anchored() {
        let matcher = build_exclude_matcher(&["Rusty/mezura".to_owned(), "D:/dev/bench".to_owned()]).unwrap();

        assert!(matcher.is_match("D:/dev/Rusty/mezura"));
        assert!(!matcher.is_match("D:/dev/aRusty/mezura"));
        assert!(matcher.is_match("D:/dev/bench"));
        assert!(!matcher.is_match("D:/dev/benchx"));
    }

    #[test]
    fn test_backslashes_and_trailing_slashes_are_normalized() {
        let matcher = build_exclude_matcher(&["Rusty\\mezura\\bench".to_owned(), "target/".to_owned()]).unwrap();

        assert!(matcher.is_match("D:/dev/Rusty/mezura/bench"));
        assert!(matcher.is_match("D:/dev/proj/target"));
    }

    #[test]
    fn test_invalid_glob_is_rejected() {
        assert!(build_exclude_matcher(&["[invalid".to_owned()]).is_err());
        assert!(build_exclude_matcher(&["valid".to_owned(), "[invalid".to_owned()]).is_err());
    }
}
