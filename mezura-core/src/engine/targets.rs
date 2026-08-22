// Which paths the walk is actually given, once the ones that lie inside other ones have been taken
// out, and which of the things it finds are excluded.
use std::borrow::Cow;
use std::path::Path;

use crate::GitignoreStack;
use crate::engine::config::Target;

// Proof inside this crate that resolution has already happened, so everything downstream can take
// absolute validated paths for granted.
#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) struct Targets {
    resolved: Vec<Target>,
    // The paths somebody wrote out, which a pattern's matches are not: a file named by hand is
    // counted whatever it holds, and one the program found obeys every rule the walk obeys.
    written_by_hand: std::collections::HashSet<String>
}

impl Targets {
    pub(crate) fn was_written_by_hand(&self, path: &Path) -> bool {
        path.to_str().is_some_and(|path| self.written_by_hand.contains(path))
    }
}

impl std::ops::Deref for Targets {
    type Target = [Target];
    fn deref(&self) -> &[Target] {
        &self.resolved
    }
}

// The command line rewords these; the Display below is what a library caller prints.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TargetError {
    InvalidPath(String),
    InvalidGlob(String),
    NoGlobMatches(String),
    AllGlobMatchesIgnored(String),
    Contested(String, String, String)
}

impl std::error::Error for TargetError {}

impl std::fmt::Display for TargetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPath(x) => write!(f, "'{x}' does not exist as a directory or file."),
            Self::InvalidGlob(x) => write!(f, "'{x}' is not a valid glob pattern."),
            Self::NoGlobMatches(x) => write!(f, "'{x}' does not match any existing directory or file."),
            Self::AllGlobMatchesIgnored(x) => write!(f, "Everything '{x}' matches is ignored, dotted or a link."),
            Self::Contested(path, first, second) => write!(f, "'{path}' is declared under two names, '{first}' and '{second}'.")
        }
    }
}

// A nested target is never scanned on its own: the scan of the one containing it reaches those files
// anyway, and which module they belong to is decided on the way down.
pub(crate) fn topmost_targets(targets: &[Target]) -> Vec<Target> {
    keep_topmost(targets.to_vec(), |_, _| true)
}

// Only the outermost of every overlapping group is kept, or its files would be counted twice. One
// that names a different module stays: it is not a repetition of its parent but the boundary that
// takes those files off it, and dropping it counts the tests of
// 'backend=./api tests=./api/tests' as backend.
pub(crate) fn remove_overlapping_targets(targets: Vec<Target>) -> Vec<Target> {
    keep_topmost(targets, |enclosing, target| enclosing.module == target.module)
}

// The matcher itself stays inside: its type belongs to a dependency, and putting it in the
// signature would make a release of globset a breaking change of ours.
pub fn validate_exclude_patterns(exclude_patterns: &[String]) -> Result<(), TargetError> {
    build_exclude_matcher(exclude_patterns).map(|_| ())
            .map_err(|x| TargetError::InvalidGlob(x.glob().unwrap_or("").to_owned()))
}

pub(crate) fn build_exclude_matcher(exclude_patterns: &[String]) -> Result<globset::GlobSet, globset::Error> {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in exclude_patterns {
        let normalized = normalise_separators(pattern.trim());
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

// "Are these two the same place". Case-insensitive on Windows where the filesystem is, and with the
// trailing separator gone, since 'D:/a' and 'D:/a/' are one directory: the containment test wants a
// path strictly longer than its ancestor plus a separator, which 'D:/a/' is not against 'D:/a', so
// the pair reads as two places and everything under it is counted twice.
pub(crate) fn path_comparison_key(path: &str) -> String {
    let path = path.trim_end_matches('/');
    if cfg!(windows) {path.to_lowercase()} else {path.to_owned()}
}

// A literal path must exist and is always used, even if it is ignored or dotted, because somebody
// named it. A glob's matches were found by the program, so they obey every rule the walk obeys, and
// the nested ones are then dropped unless they carry a module of their own.
//
// Idempotent, because all of it is existence-first: what one pass resolved, the next takes literally.
pub(crate) fn resolve(declared: &[Target], respect_gitignore: bool, search_in_dotted: bool)
-> Result<Targets, TargetError>
{
    let prepared = validate_and_absolutize(declared)?;
    // Taken before the expansion, which is what turns one pattern into paths nobody typed
    let written_by_hand = prepared.iter().filter(|target| is_valid_path(&target.path))
            .map(|target| target.path.clone()).collect();
    Ok(Targets { resolved: expand_patterns(prepared, respect_gitignore, search_in_dotted)?, written_by_hand })
}

// A relative path or pattern is joined to the working directory, so a saved configuration still
// names the same places when it is loaded from somewhere else.
//
// 'run' does this as its first step anyway, so call it only to refuse a bad path at the moment
// somebody typed it. It does not expand patterns, because which of a pattern's matches survive
// depends on settings this cannot see.
pub fn validate_and_absolutize(declared: &[Target]) -> Result<Vec<Target>, TargetError> {
    let mut prepared: Vec<Target> = Vec::with_capacity(declared.len());
    for target in declared {
        let (module, trimmed) = (&target.module, target.path.trim());
        // Two spellings of one name are one module, and the first one seen is the one the report
        // prints
        let module = module.as_ref().map(|name| prepared.iter()
                .find_map(|x: &Target| x.module.clone().filter(|seen| seen.to_lowercase() == name.to_lowercase()))
                .unwrap_or_else(|| name.clone()));
        // What exists wins over what the text looks like: a path naming something on disk is taken
        // literally whatever characters it has, and pattern syntax is only read in text that names
        // nothing. The other way round, the brackets in a real directory's name, or in the working
        // directory a relative path was joined to, are read as a character class and a place that
        // exists is refused.
        if !is_valid_path(trimmed) && has_glob_metacharacters(trimmed) {
            prepared.push(Target { module, path: absolutize_pattern(trimmed) });
        } else if is_valid_path(trimmed) {
            prepared.push(Target { module, path: convert_to_absolute(trimmed) });
        } else {
            return Err(TargetError::InvalidPath(trimmed.to_owned()));
        }
    }
    // Here as well as after the patterns expand, and not only there: a contest between two literal
    // paths is decidable the moment they are typed, and under '--diff' the later check runs against
    // a checkout, so it names a temporary directory nobody wrote.
    find_contested_target(&prepared)?;

    Ok(prepared)
}

// Two names over one path is not something a rule can settle: there is no more specific one of the
// two, and whichever won would take the other's files away without a word.
fn find_contested_target(targets: &[Target]) -> Result<(), TargetError> {
    for (position, target) in targets.iter().enumerate() {
        let key = path_comparison_key(&target.path);
        if let Some(other) = targets[position + 1..].iter()
                .find(|x| x.module != target.module && path_comparison_key(&x.path) == key) {
            return Err(TargetError::Contested(target.path.clone(),
                    name_or_rest(&target.module), name_or_rest(&other.module)));
        }
    }

    Ok(())
}

// The spelling every resolved path carries: absolute, forward slashes, no trailing separator, and
// without the '\\?\' prefix that std's 'canonicalize' puts on Windows.
pub fn convert_to_absolute(s: &str) -> String {
    let p = Path::new(s);
    if p.is_absolute() {
        return trim_trailing_slash(&normalise_separators(s)).to_owned();
    }

    // The canonical form of a path that was typed as valid UTF-8 need not be valid UTF-8 itself,
    // since canonicalizing resolves links and the target's real name is whatever the file system
    // holds. Falling back to what was typed keeps a string that still names the place, which
    // 'to_string_lossy' would not: this one is handed to 'is_dir' and 'is_file' further down.
    match std::fs::canonicalize(p).ok().and_then(|buf| buf.to_str().map(str::to_owned)) {
        Some(str_path) => trim_trailing_slash(
                &normalise_separators(str_path.strip_prefix(r"\\?\").unwrap_or(&str_path))).to_owned(),
        None => trim_trailing_slash(&normalise_separators(s)).to_owned()
    }
}

pub(crate) fn normalise_separators(path: &str) -> Cow<'_, str> {
    if cfg!(windows) {Cow::Owned(path.replace('\\', "/"))} else {Cow::Borrowed(path)}
}

// Sorted by path with the duplicates gone, so the nearest enclosing target of any entry is the last
// one kept before it. 'covered' decides what "enclosing" is allowed to remove.
//
// The sort belongs to the algorithm and not to the answer, so the order the targets were written in
// is carried through and restored at the end: a report that starts with alpha when the user declared
// 'zeta=... alpha=...' is in a third order that answers nobody's question.
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

fn expand_patterns(targets: Vec<Target>, respect_gitignore: bool, search_in_dotted: bool)
-> Result<Vec<Target>, TargetError>
{
    fn is_dotted(path: &Path) -> bool {
        path.file_name().and_then(|x| x.to_str()).is_some_and(|x| x.starts_with('.'))
    }

    let mut resolved = Vec::with_capacity(targets.len());
    for target in targets {
        // Existence decides here too: prepared targets come back through this on the second
        // resolution pass, and an absolutized literal may carry pattern characters it never typed
        if !is_valid_path(&target.path) && has_glob_metacharacters(&target.path) {
            let Ok(paths) = glob::glob(&target.path) else {
                return Err(TargetError::InvalidGlob(target.path));
            };
            let matches = paths.flatten().filter(|x| x.is_dir() || x.is_file()).collect::<Vec<_>>();
            if matches.is_empty() {
                return Err(TargetError::NoGlobMatches(target.path));
            }

            let relevant = matches.iter()
                    .filter(|x| search_in_dotted || !is_dotted(x))
                    .filter(|x| !respect_gitignore || !GitignoreStack::is_path_ignored(x))
                    // A pattern is not a name: what it matched was found by the program, and a link
                    // the walk finds is skipped, since it counts whatever it points at a second
                    // time. Named on its own it is still followed, as any target is.
                    .filter(|x| !x.is_symlink())
                    .filter_map(|x| x.to_str().map(convert_to_absolute))
                    .map(|path| Target { module: target.module.clone(), path }).collect::<Vec<_>>();
            if relevant.is_empty() {
                return Err(TargetError::AllGlobMatchesIgnored(target.path));
            }
            resolved.extend(relevant);
        } else {
            resolved.push(target);
        }
    }

    // Again here, because a pattern can expand onto a path another target names and that is knowable
    // only now
    find_contested_target(&resolved)?;

    Ok(remove_overlapping_targets(resolved))
}

// 'convert_to_absolute' cannot do this one: it asks the filesystem, and a pattern is not a path that
// exists. Joining the working directory changes nothing about what the pattern matches, since a
// relative one is expanded against that directory anyway; what it buys is that the pattern still
// means the same thing written into a configuration and read back from somewhere else.
fn absolutize_pattern(pattern: &str) -> String {
    let normalized = normalise_separators(pattern);
    if Path::new(normalized.as_ref()).is_absolute() {
        return normalized.into_owned();
    }
    match std::env::current_dir() {
        Ok(cwd) => format!("{}/{}", normalise_separators(&cwd.to_string_lossy()).trim_end_matches('/'),
                normalized.trim_start_matches("./")),
        Err(_) => normalized.into_owned()
    }
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

// One place, one spelling: 'D:/x/' and 'D:/x' name the same directory, and two runs over it have to
// record the same string or a comparison between them reports a change nobody made. Not taken off a
// root, where the separator belongs to the name: 'D:/' is the root of the drive while 'D:' is the
// current directory on it, and '/' is the root of the file system.
fn trim_trailing_slash(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() || trimmed.ends_with(':') {path} else {trimmed}
}

#[cfg(test)]
mod target_path_tests {
    use super::*;

    #[test]
    fn a_path_carrying_a_glob_character_is_recognised_as_a_pattern() {
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
        kept_paths(remove_overlapping_targets(paths.iter().map(|x| Target::of(*x)).collect()))
    }

    // 'name path' declares the module, a bare path declares none
    fn dedupe_named(entries: &[&str]) -> Vec<String> {
        let targets = entries.iter().map(|entry| match entry.split_once(' ') {
            Some((name, path)) => Target::named(name, path),
            None => Target::of(*entry)
        }).collect();
        kept_paths(remove_overlapping_targets(targets))
    }

    fn kept_paths(targets: Vec<Target>) -> Vec<String> {
        targets.into_iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn unrelated_targets_are_all_kept_in_the_order_they_were_written() {
        assert_eq!(Vec::<String>::new(), dedupe(&[]));
        assert_eq!(vec!["D:/a"], dedupe(&["D:/a"]));
        // The order they were written in, not the order they sort in
        assert_eq!(vec!["D:/b", "D:/a"], dedupe(&["D:/b", "D:/a"]));
        assert_eq!(vec!["D:/a", "E:/a"], dedupe(&["D:/a", "E:/a"]));
    }

    #[test]
    fn the_same_target_written_twice_is_kept_once() {
        assert_eq!(vec!["D:/a"], dedupe(&["D:/a", "D:/a"]));
        assert_eq!(vec!["D:/b", "D:/a"], dedupe(&["D:/b", "D:/a", "D:/b", "D:/a"]));
    }

    #[test]
    fn a_target_inside_another_target_is_dropped() {
        assert_eq!(vec!["D:/a"], dedupe(&["D:/a", "D:/a/b"]));
        assert_eq!(vec!["D:/a"], dedupe(&["D:/a/b", "D:/a"]));
        assert_eq!(vec!["D:/a"], dedupe(&["D:/a", "D:/a/b/c/d", "D:/a/b"]));
        assert_eq!(vec!["D:/a"], dedupe(&["D:/a", "D:/a/file.rs"]));
        assert_eq!(vec!["D:/a", "D:/b"], dedupe(&["D:/a", "D:/a/x", "D:/b", "D:/b/y/z"]));
    }

    #[test]
    fn a_shared_prefix_that_is_not_a_whole_component_is_not_an_overlap() {
        // 'D:/ab' is not inside 'D:/a', despite the string prefix
        assert_eq!(vec!["D:/a", "D:/ab"], dedupe(&["D:/a", "D:/ab"]));
        assert_eq!(vec!["D:/a", "D:/a-b"], dedupe(&["D:/a", "D:/a-b"]));
        // the '-' sorts before the '/', so a naive scan against only the previous kept path
        // would let 'D:/a/b' through, even though it is inside 'D:/a'
        assert_eq!(vec!["D:/a", "D:/a-b"], dedupe(&["D:/a", "D:/a-b", "D:/a/b"]));
        assert_eq!(vec!["D:/a-b", "D:/a", "D:/a!b"], dedupe(&["D:/a/deep/one", "D:/a-b", "D:/a", "D:/a!b"]));
    }

    #[test]
    fn a_trailing_slash_or_a_different_case_still_names_the_same_place() {
        assert_eq!(vec!["D:/a/"], dedupe(&["D:/a/", "D:/a/b"]));
        // The same place written twice and not a byte-identical duplicate: it still folds into one
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

    // Dropping the nested one would quietly count the tests as backend, the opposite of what was
    // asked for
    #[test]
    fn a_nested_target_survives_the_pruning_when_it_names_another_module() {
        assert_eq!(vec!["backend=D:/api", "tests=D:/api/tests"],
                dedupe_named(&["backend D:/api", "tests D:/api/tests"]));
        assert_eq!(vec!["D:/api", "tests=D:/api/tests"], dedupe_named(&["D:/api", "tests D:/api/tests"]));

        // and it is dropped when it would have been counted the same way anyway
        assert_eq!(vec!["tests=D:/api"], dedupe_named(&["tests D:/api", "tests D:/api/deep"]));
    }

    // The nearest enclosing target decides, not the outermost one
    #[test]
    fn a_target_that_reverts_the_module_of_the_one_above_it_is_kept() {
        assert_eq!(vec!["D:/a", "tests=D:/a/b", "D:/a/b/c"],
                dedupe_named(&["D:/a", "tests D:/a/b", "D:/a/b/c"]));
    }

    // A saved configuration holds its targets in absolute form, so with syntax deciding first, one
    // saved under a directory with a bracket in its name could never be loaded again.
    #[test]
    fn an_existing_path_is_a_literal_target_even_when_it_looks_like_a_pattern() {
        let root = std::env::temp_dir().join("mezura-existing-bracket");
        let _ = std::fs::remove_dir_all(&root);
        let bracketed = root.join("a[b");
        std::fs::create_dir_all(&bracketed).unwrap();
        let bracketed_str = bracketed.to_str().unwrap().replace('\\', "/");

        let resolved = resolve(&[Target::of(bracketed_str)], true, false);
        std::fs::remove_dir_all(&root).unwrap();

        let resolved = resolved.unwrap();
        assert_eq!(1, resolved.len());
        assert!(resolved[0].path.ends_with("a[b"), "not kept as itself: {resolved:?}");
    }

    fn resolved_paths(declared: Vec<Target>, respect_gitignore: bool) -> Result<Vec<String>, TargetError> {
        resolve(&declared, respect_gitignore, false).map(|x| x.iter().map(Target::to_string).collect())
    }

    #[test]
    fn one_path_under_two_names_is_refused() {
        let root = std::env::temp_dir().join("mezura-contested");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let root_str = root.to_str().unwrap().replace('\\', "/");

        let contested = resolve(&[Target::named("code", root_str.clone()), Target::named("other", root_str.clone())], true, false);
        let unnamed = resolve(&[Target::named("code", root_str.clone()), Target::of(root_str.clone())], true, false);
        let repeated = resolve(&[Target::named("code", root_str.clone()), Target::named("code", root_str)], true, false);
        std::fs::remove_dir_all(&root).unwrap();

        assert!(matches!(contested, Err(TargetError::Contested(_, ref a, ref b)) if a == "code" && b == "other"));
        assert!(matches!(unnamed, Err(TargetError::Contested(_, ref a, ref b)) if a == "code" && b == "(unnamed)"));
        // The same name twice over one path is a repetition and not a contest
        assert_eq!(1, repeated.unwrap().len());
    }

    // Under '--diff' the later check runs against a checkout, so a whole repository is written out
    // before the message names a temporary directory nobody wrote.
    #[test]
    fn two_names_over_one_typed_path_are_refused_before_anything_is_counted() {
        let root = std::env::temp_dir().join("mezura-contested-early");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let (slashes, backslashes) = (root.to_str().unwrap().replace('\\', "/"), root.to_str().unwrap().to_owned());

        // The two spellings are one path once absolutized, which is what makes it a contest
        let contested = validate_and_absolutize(&[Target::named("one", slashes.clone()),
                Target::named("two", backslashes)]);
        let apart = validate_and_absolutize(&[Target::named("one", slashes.clone()), Target::of(slashes)]);
        std::fs::remove_dir_all(&root).unwrap();

        assert!(matches!(contested, Err(TargetError::Contested(_, ref a, ref b)) if a == "one" && b == "two"),
                "{contested:?}");
        assert!(apart.is_err(), "a name against the leftovers is a contest too");
    }

    #[test]
    fn a_pattern_expands_to_its_existing_matches_and_overlaps_collapse() {
        let root = std::env::temp_dir().join("mezura_glob_test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("a").join("src")).unwrap();
        std::fs::create_dir_all(root.join("b").join("src")).unwrap();
        std::fs::create_dir_all(root.join("c")).unwrap();
        std::fs::write(root.join("a").join("src").join("one.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("b").join("src").join("two.rs"), "fn main() {}").unwrap();
        let root_str = root.to_str().unwrap().replace('\\', "/");
        let abs = |x: &str| convert_to_absolute(&format!("{root_str}/{x}"));

        assert_eq!(vec![abs("a/src"), abs("b/src")], resolved_paths(vec![Target::of(format!("{root_str}/*/src"))], true).unwrap());
        assert_eq!(vec![abs("a/src/one.rs")], resolved_paths(vec![Target::of(format!("{root_str}/a/src/*.rs"))], true).unwrap());
        assert_eq!(vec![abs("a"), abs("b"), abs("c")], resolved_paths(vec![Target::of(format!("{root_str}/*"))], true).unwrap());

        // A pattern can be mixed with literal paths, and the overlaps of both are collapsed
        assert_eq!(vec![abs("a"), abs("b"), abs("c")], resolved_paths(vec![Target::of(format!("{root_str}/*")),
                Target::of(format!("{root_str}/*/src")), Target::of(format!("{root_str}/a/src/one.rs"))], true).unwrap());

        assert!(matches!(resolved_paths(vec![Target::of(format!("{root_str}/*/nope"))], true),
                Err(TargetError::NoGlobMatches(_))));
        // Named in its prepared form, joined to the working directory: that is what a saved
        // configuration holds, so the error names what is actually written wherever it lives
        assert!(matches!(resolved_paths(vec![Target::of("a[")], true),
                Err(TargetError::InvalidGlob(p)) if p == convert_to_absolute("./").trim_end_matches('/').to_owned() + "/a["));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn glob_matches_respect_gitignore_but_literal_paths_do_not() {
        let root = std::env::temp_dir().join("mezura_glob_gitignore_test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("kept")).unwrap();
        std::fs::create_dir_all(root.join("build").join("deep")).unwrap();
        std::fs::write(root.join(".gitignore"), "build/\nignored.rs\n").unwrap();
        std::fs::write(root.join("kept").join("one.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("kept").join("ignored.rs"), "fn main() {}").unwrap();
        std::fs::write(root.join("build").join("deep").join("generated.rs"), "fn main() {}").unwrap();
        let root_str = root.to_str().unwrap().replace('\\', "/");
        let abs = |x: &str| convert_to_absolute(&format!("{root_str}/{x}"));

        // The ignored dir and the ignored file are dropped from the matches
        assert_eq!(vec![abs("kept")], resolved_paths(vec![Target::of(format!("{root_str}/*"))], true).unwrap());
        assert_eq!(vec![abs("kept/one.rs")], resolved_paths(vec![Target::of(format!("{root_str}/**/*.rs"))], true).unwrap());

        // Unless the gitignore support is turned off
        assert_eq!(vec![abs("build"), abs("kept")], resolved_paths(vec![Target::of(format!("{root_str}/*"))], false).unwrap());
        assert_eq!(vec![abs("build/deep/generated.rs"), abs("kept/ignored.rs"), abs("kept/one.rs")],
                resolved_paths(vec![Target::of(format!("{root_str}/**/*.rs"))], false).unwrap());

        // Explicitly named paths are always used, even when they are ignored
        assert_eq!(vec![abs("build")], resolved_paths(vec![Target::of(format!("{root_str}/build"))], true).unwrap());
        assert_eq!(vec![abs("kept/ignored.rs")], resolved_paths(vec![Target::of(format!("{root_str}/kept/ignored.rs"))], true).unwrap());

        assert!(matches!(resolved_paths(vec![Target::of(format!("{root_str}/build/*"))], true),
                Err(TargetError::AllGlobMatchesIgnored(_))));

        std::fs::remove_dir_all(&root).unwrap();
    }

    // The log compares these strings to decide whether two runs measured the same tree, so 'D:/x/'
    // against 'D:/x' reports a change nobody made. The pruning inside one run treats them as one
    // place either way, which is what keeps the disagreement out of sight.
    #[test]
    fn a_trailing_separator_is_not_a_second_spelling_of_one_place() {
        let root = std::env::temp_dir().join("mezura-trailing-slash");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let root_str = root.to_str().unwrap().replace('\\', "/");

        let bare = convert_to_absolute(&root_str);
        let slashed = convert_to_absolute(&(root_str + "/"));
        std::fs::remove_dir_all(&root).unwrap();

        assert_eq!(bare, slashed);
    }

    #[test]
    fn the_trailing_separator_of_a_root_belongs_to_its_name() {
        assert_eq!("D:/", trim_trailing_slash("D:/"));
        assert_eq!("/", trim_trailing_slash("/"));
        assert_eq!("D:/x", trim_trailing_slash("D:/x/"));
        assert_eq!("D:/x", trim_trailing_slash("D:/x//"));
        assert_eq!("//server/share", trim_trailing_slash("//server/share/"));
    }

    #[test]
    fn text_that_names_nothing_is_a_pattern_only_when_it_carries_the_syntax() {
        let nowhere = std::env::temp_dir().join("mezura-nowhere");
        let pattern_str = nowhere.join("a?b").to_str().unwrap().replace('\\', "/");
        let plain_str = nowhere.join("plain").to_str().unwrap().replace('\\', "/");

        assert!(matches!(resolve(&[Target::of(pattern_str)], true, false), Err(TargetError::NoGlobMatches(_))));
        assert!(matches!(validate_and_absolutize(&[Target::of(plain_str.clone())]), Err(TargetError::InvalidPath(p)) if p == plain_str));
    }

    #[test]
    fn two_spellings_of_a_module_name_are_one_module() {
        let root = std::env::temp_dir().join("mezura-module-name-case");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::create_dir_all(root.join("b")).unwrap();
        let path = |x: &str| root.join(x).to_str().unwrap().replace('\\', "/");

        let prepared = validate_and_absolutize(&[Target::named("code", path("a")), Target::named("CODE", path("b"))]);
        let separate = validate_and_absolutize(&[Target::named("code", path("a")), Target::named("suite", path("b"))]);
        std::fs::remove_dir_all(&root).unwrap();

        assert_eq!(vec![Some("code".to_owned()), Some("code".to_owned())],
                prepared.unwrap().iter().map(|x| x.module.clone()).collect::<Vec<_>>(),
                "'CODE' was kept apart from 'code'");
        // and two names that really differ stay two
        assert_eq!(vec![Some("code".to_owned()), Some("suite".to_owned())],
                separate.unwrap().iter().map(|x| x.module.clone()).collect::<Vec<_>>());
    }

    #[test]
    fn the_roots_of_the_traversal_never_contain_one_another() {
        let targets = vec![Target::named("backend", "D:/api"),
                Target::named("tests", "D:/api/tests")];
        assert_eq!(vec!["backend=D:/api"], kept_paths(topmost_targets(&targets)));
    }
}

#[cfg(test)]
mod exclude_matcher_tests {
    use super::*;

    #[test]
    fn an_exclusion_naming_one_word_matches_it_at_any_depth_and_never_half_a_name() {
        let matcher = build_exclude_matcher(&["node_modules".to_owned(), "*.min.js".to_owned()]).unwrap();

        assert!(matcher.is_match("node_modules"));
        assert!(matcher.is_match("D:/proj/node_modules"));
        assert!(!matcher.is_match("D:/proj/node_modules_2"));
        assert!(matcher.is_match("D:/proj/app/bundle.min.js"));
        assert!(!matcher.is_match("D:/proj/app/bundle.js"));
        assert!(!matcher.is_match("D:/proj/appbundle.min.js/other.js"));
    }

    #[test]
    fn an_exclusion_naming_a_path_is_anchored_on_whole_components() {
        let matcher = build_exclude_matcher(&["Rusty/mezura".to_owned(), "D:/dev/bench".to_owned()]).unwrap();

        assert!(matcher.is_match("D:/dev/Rusty/mezura"));
        assert!(!matcher.is_match("D:/dev/aRusty/mezura"));
        assert!(matcher.is_match("D:/dev/bench"));
        assert!(!matcher.is_match("D:/dev/benchx"));
    }

    #[test]
    fn an_exclusion_written_with_backslashes_or_a_trailing_slash_still_matches() {
        let matcher = build_exclude_matcher(&["Rusty\\mezura\\bench".to_owned(), "target/".to_owned()]).unwrap();

        assert!(matcher.is_match("D:/dev/Rusty/mezura/bench"));
        assert!(matcher.is_match("D:/dev/proj/target"));
    }

    #[test]
    fn an_exclusion_that_is_not_a_valid_glob_stops_the_run() {
        assert!(build_exclude_matcher(&["[invalid".to_owned()]).is_err());
        assert!(build_exclude_matcher(&["valid".to_owned(), "[invalid".to_owned()]).is_err());
    }
}
