// Which paths the walk is actually given, once the ones that lie inside other ones have been taken
// out, and which of the things it finds are excluded. Everything here decides what gets counted.

use std::path::Path;

use crate::GitignoreStack;
use crate::engine::config::Target;


// Proof inside this crate that resolution has already happened: 'run' makes one at its entry from
// the declared targets of the configuration, using that same configuration's settings, and
// everything downstream of it operates on validated absolute paths. It never leaves the crate,
// because a caller has nothing to do with it: the configuration carries targets as declared, and
// resolving them is the run's own first step.
#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) struct Targets(Vec<Target>);

impl std::ops::Deref for Targets {
    type Target = [Target];
    fn deref(&self) -> &[Target] {
        &self.0
    }
}


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


// What went wrong while working out which paths to walk. Carried on the run's own error, and the
// command line turns it into its own wording; the Display below is the plain-text form a library
// caller prints.
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

// Literal paths must exist and are always used, even if they are ignored or dotted, since the user
// named them explicitly. Glob patterns are expanded to the existing paths they match, and those
// matches are discovered by the program, so they are subject to the same rules as every other
// discovered path. Finally, targets contained in other targets are dropped, so that no file
// is counted twice, unless the nested one carries a module of its own and is therefore the boundary
// that takes those files off the one around it. Idempotent, because the whole of it is
// existence-first: what a first pass resolved, a second pass takes literally.
pub(crate) fn resolve(declared: &[Target], respect_gitignore: bool, search_in_dotted: bool)
-> Result<Targets, TargetError>
{
    expand_patterns(validate_and_absolutize(declared)?, respect_gitignore, search_in_dotted).map(Targets)
}

// A target that names nothing is refused, and everything else comes back in the absolute spelling
// the run will use: a relative path is made absolute, a relative pattern is joined to the working
// directory so that a saved configuration still names the same places when it is loaded from some
// other one, and two spellings of one module name are unified.
//
// This is the half of resolution that no setting can change, which is why it can be done on its
// own. 'run' does it anyway as its first step, so call this only to refuse a bad path early, at the
// moment somebody typed it and before a run is worth starting. What it deliberately does not do is
// expand patterns: which of a pattern's matches survive depends on the settings of the
// configuration the targets belong to, and those are the run's to read.
pub fn validate_and_absolutize(declared: &[Target]) -> Result<Vec<Target>, TargetError> {
    let mut prepared: Vec<Target> = Vec::with_capacity(declared.len());
    for target in declared {
        let (module, trimmed) = (&target.module, target.path.trim());
        // Two spellings of one name are one module, the way two spellings of one extension are one
        // extension. The first one seen is the one the report prints.
        let module = module.as_ref().map(|name| prepared.iter()
                .find_map(|x: &Target| x.module.clone().filter(|seen| seen.to_lowercase() == name.to_lowercase()))
                .unwrap_or_else(|| name.clone()));
        // The place that exists wins over the syntax: a path that names something on disk is taken
        // literally whatever characters it carries, and pattern syntax is read only in text that
        // names nothing. Deciding by syntax first read the brackets of an existing directory, or
        // of the working directory a relative path was joined to, as a character class, and
        // refused a place that exists.
        if !is_valid_path(trimmed) && has_glob_metacharacters(trimmed) {
            prepared.push(Target { module, path: absolutize_pattern(trimmed) });
        } else if is_valid_path(trimmed) {
            prepared.push(Target { module, path: convert_to_absolute(trimmed) });
        } else {
            return Err(TargetError::InvalidPath(trimmed.to_owned()));
        }
    }
    Ok(prepared)
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
            let paths = match glob::glob(&target.path) {
                Ok(x) => x,
                Err(_) => return Err(TargetError::InvalidGlob(target.path.clone()))
            };
            let matches = paths.flatten().filter(|x| x.is_dir() || x.is_file()).collect::<Vec<_>>();
            if matches.is_empty() {
                return Err(TargetError::NoGlobMatches(target.path.clone()));
            }

            let relevant = matches.iter()
                    .filter(|x| search_in_dotted || !is_dotted(x))
                    .filter(|x| !respect_gitignore || !GitignoreStack::is_path_ignored(x))
                    // A pattern is not a name: what it matched was found by the program, and a link
                    // it found is a link the walk would have skipped for counting twice whatever it
                    // points at. Named on its own it is still followed, as any target is.
                    .filter(|x| !x.is_symlink())
                    .filter_map(|x| x.to_str().map(convert_to_absolute))
                    .map(|path| Target { module: target.module.clone(), path }).collect::<Vec<_>>();
            if relevant.is_empty() {
                return Err(TargetError::AllGlobMatchesIgnored(target.path.clone()));
            }
            resolved.extend(relevant);
        } else {
            resolved.push(target);
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

// 'convert_to_absolute' cannot do this one, because it asks the file system and a pattern is not a
// path that exists. Joining with the working directory changes nothing about how the pattern
// expands, since a relative one was expanded against that directory anyway; what it buys is that
// the pattern still means the same thing written into a file and read back somewhere else.
fn absolutize_pattern(pattern: &str) -> String {
    let normalized = pattern.replace('\\', "/");
    if Path::new(&normalized).is_absolute() {
        return normalized;
    }
    match std::env::current_dir() {
        Ok(cwd) => format!("{}/{}", cwd.to_string_lossy().replace('\\', "/").trim_end_matches('/'),
                normalized.trim_start_matches("./")),
        Err(_) => normalized
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


// The spelling every resolved path carries: absolute, forward slashes, no trailing separator, and
// without the '\\?\' prefix that std's 'canonicalize' puts on Windows. Every literal target goes
// through it, so a caller wanting the spelling a resolved target will have starts here.
pub fn convert_to_absolute(s: &str) -> String {
    let p = Path::new(s);
    if p.is_absolute() {
        return without_trailing_slash(&s.replace("\\", "/")).to_owned();
    }

    // The canonical form of a path that was typed as valid UTF-8 need not be valid UTF-8 itself,
    // since canonicalizing resolves links and the target's real name is whatever the file system
    // holds. Falling back to what was typed keeps a string that still names the place, which
    // 'to_string_lossy' would not: this one is handed back to 'is_dir' and 'is_file' further down.
    match std::fs::canonicalize(p).ok().and_then(|buf| buf.to_str().map(str::to_owned)) {
        Some(str_path) => without_trailing_slash(&str_path.strip_prefix(r"\\?\").unwrap_or(&str_path).replace("\\", "/")).to_owned(),
        None => without_trailing_slash(&s.replace("\\", "/")).to_owned()
    }
}

// One place, one spelling: 'D:/x/' and 'D:/x' name the same directory, and two runs over it have to
// record the same string or a comparison between them reports a change nobody made. Not taken off a
// root, where the separator belongs to the name: 'D:/' is the root of the drive while 'D:' is the
// current directory on it, and '/' is the root of the file system.
fn without_trailing_slash(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() || trimmed.ends_with(':') {path} else {trimmed}
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

    // A place that exists is the place the user named, whatever characters its name carries:
    // pattern syntax applies only to text that names nothing on disk. Deciding by syntax first
    // read the brackets of an existing directory, or of the working directory a relative path had
    // been joined to, as a character class, and refused a place that exists: a saved configuration
    // holds its targets in absolute form, so one saved under a bracketed ancestor could never load.
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

    // There is no more specific one of the two to decide it, and whichever won would take the
    // other's files away without a word
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

    // A trailing separator names the same place as none, and the resolved spelling has to settle on
    // one of them: the log compares these strings to decide whether two runs measured the same tree,
    // so 'D:/x/' against 'D:/x' reported a change nobody made. The pruning inside one run already
    // treated them as one place, which is exactly what kept the disagreement out of sight.
    #[test]
    fn a_trailing_separator_is_not_a_second_spelling_of_one_place() {
        let root = std::env::temp_dir().join("mezura-trailing-slash");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let root_str = root.to_str().unwrap().replace('\\', "/");

        let bare = convert_to_absolute(&root_str);
        let slashed = convert_to_absolute(&(root_str.clone() + "/"));
        std::fs::remove_dir_all(&root).unwrap();

        assert_eq!(bare, slashed);
    }

    // A root is not a directory with a separator after it: 'D:/' is the root of the drive while
    // 'D:' is the current directory on it, and '/' is the root of the file system.
    #[test]
    fn the_trailing_separator_of_a_root_belongs_to_its_name() {
        assert_eq!("D:/", without_trailing_slash("D:/"));
        assert_eq!("/", without_trailing_slash("/"));
        assert_eq!("D:/x", without_trailing_slash("D:/x/"));
        assert_eq!("D:/x", without_trailing_slash("D:/x//"));
        assert_eq!("//server/share", without_trailing_slash("//server/share/"));
    }

    // The other side of existence-first: text that names nothing on disk is a pattern when it
    // carries the syntax, and a missing place when it does not.
    #[test]
    fn text_that_names_nothing_is_a_pattern_only_when_it_carries_the_syntax() {
        let nowhere = std::env::temp_dir().join("mezura-nowhere");
        let pattern_str = nowhere.join("a?b").to_str().unwrap().replace('\\', "/");
        let plain_str = nowhere.join("plain").to_str().unwrap().replace('\\', "/");

        assert!(matches!(resolve(&[Target::of(pattern_str)], true, false), Err(TargetError::NoGlobMatches(_))));
        assert!(matches!(validate_and_absolutize(&[Target::of(plain_str.clone())]), Err(TargetError::InvalidPath(p)) if p == plain_str));
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
