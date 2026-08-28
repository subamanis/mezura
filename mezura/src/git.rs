// Everything a comparison against a git commit needs: the repository a target belongs to, the
// worktree the commit is written out to, and the removal of it afterwards.
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::thread::JoinHandle;

use super::paths::{fold_for_comparison, normalise_separators};

// The path built from this also carries the process id, since a worktree cannot be added twice at
// one place and two runs of one revision can overlap
const CHECKOUT_PREFIX : &str = "mezura-diff-";

static PENDING_REMOVALS : Mutex<Vec<(String, JoinHandle<()>)>> = Mutex::new(Vec::new());

#[derive(Debug)]
pub enum GitError {
    NotInstalled(std::io::Error),
    NotARepository { path: String },
    TwoRepositories { first: String, second: String },
    NoSuchRevision { revision: String, repository: String },
    SameCommit { first: String, second: String, commit: String },
    PatternTarget { pattern: String },
    // The checkout succeeded and the counting of it did not, so git had no part in this one.
    CountingRevision { revision: String, error: mezura_core::RunError },
    Refused { doing: &'static str, message: String }
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInstalled(x) => write!(f, "git could not be run, so a revision cannot be counted: {x}."),
            Self::NotARepository { path } => write!(f, "'{path}' is not inside a git repository, so there is no revision of it to count."),
            Self::TwoRepositories { first, second } => write!(f, "the targets are in two different repositories, '{first}' and '{second}', so a revision names two different things. Count them one repository at a time."),
            Self::NoSuchRevision { revision, repository } => write!(f, "'{revision}' is not a branch, tag or commit of the repository at '{repository}', and there is no file by that name either. One that lives only on a remote needs a 'git fetch' first."),
            Self::SameCommit { first, second, commit } => write!(f, "'{first}' and '{second}' name the same commit, {}, so the comparison has nothing to say.", &commit[..commit.len().min(12)]),
            Self::PatternTarget { pattern } => write!(f, "'{pattern}' is a glob pattern, and what it matches in the working tree and what it would match at that commit are two different sets of files. Write out the paths it should mean."),
            Self::CountingRevision { revision, error } => write!(f, "'{revision}' was written out, but counting it failed. {error}"),
            Self::Refused { doing, message } => write!(f, "git refused while {doing}: {message}")
        }
    }
}

impl std::error::Error for GitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotInstalled(x) => Some(x),
            Self::CountingRevision { error, .. } => Some(error),
            _ => None
        }
    }
}

// The repository a path belongs to, and where the path sits inside it. Both come from git: the
// prefix it answers with is already the path to use inside a checkout, so nothing here takes a root
// off a path.
pub fn find_repository_of(path: &str) -> Result<(String, String), GitError> {
    // 'git -C' needs a directory, so a file asks from beside itself and puts its name back on the
    // prefix
    let as_path = Path::new(path);
    if as_path.is_file() && let (Some(parent), Some(name)) = (as_path.parent(), as_path.file_name()) {
        let (root, prefix) = find_repository_of(&parent.to_string_lossy())?;
        return Ok((root, prefix + &name.to_string_lossy()));
    }

    let root = ask(path, &["rev-parse", "--show-toplevel"])?
            .ok_or_else(|| GitError::NotARepository { path: path.to_owned() })?;
    let prefix = ask(path, &["rev-parse", "--show-prefix"])?
            .ok_or_else(|| GitError::NotARepository { path: path.to_owned() })?;

    Ok((root, prefix))
}

pub fn find_common_repository_of(paths: &[String]) -> Result<String, GitError> {
    let mut found : Option<String> = None;
    for path in paths {
        let (root, _) = find_repository_of(path)?;
        match &found {
            Some(first) if *first != root => return Err(GitError::TwoRepositories {
                    first: first.clone(), second: root }),
            Some(_) => (),
            None => found = Some(root)
        }
    }

    found.ok_or_else(|| GitError::NotARepository { path: String::new() })
}

// 'HEAD' moves, so a name can answer differently a moment later: everything that acts on a revision
// takes this and never resolves the name again.
#[derive(Clone, Debug)]
pub struct ResolvedRevision {
    pub repository: String,
    pub revision: String,
    pub commit: String,
    pub taken_at: String
}

// One call answers both the hash and the date, and a name that is no commit fails it exactly as it
// fails 'rev-parse --verify'.
pub fn resolve_revision(repository: &str, revision: &str) -> Result<ResolvedRevision, GitError> {
    let answer = ask(repository, &["show", "-s", "--format=%H%n%cI", &format!("{revision}^{{commit}}")])?
            .ok_or_else(|| GitError::NoSuchRevision { revision: revision.to_owned(),
                    repository: repository.to_owned() })?;
    let (commit, taken_at) = answer.split_once('\n').unwrap_or((&answer, ""));

    Ok(ResolvedRevision { repository: repository.to_owned(), revision: revision.to_owned(),
            commit: commit.trim().to_owned(), taken_at: taken_at.trim().to_owned() })
}

// The tree of a commit, written out where mezura can count it. Removed when this goes out of scope,
// whichever way the run ended, and a run that was killed before that is swept by the next one.
pub struct Checkout {
    pub path: String,
    pub resolved: ResolvedRevision
}

impl Checkout {
    // Where a path of the working tree is to be found in here, from the prefix that
    // 'find_repository_of' answered with. None when the revision predates it: a directory that did
    // not exist then counts zero rather than stopping the run.
    // Trimmed of any trailing separator: the repository root's prefix is empty, and a target
    // ending in '/' matches no file path on a whole-component check
    pub fn find_target_of(&self, prefix: &str) -> Option<String> {
        let inside = (self.path.clone() + "/" + prefix.trim_end_matches('/'))
                .trim_end_matches('/').to_owned();
        Path::new(&inside).exists().then_some(inside)
    }
}

// Removing a large tree takes seconds and nothing that prints depends on it, so it runs on its own
// thread and the comparison does not wait. The join happens at the very end of main, or the process
// could exit mid-delete and leave the temporary directory half there.
impl Drop for Checkout {
    fn drop(&mut self) {
        let (repository, path) = (self.resolved.repository.clone(), self.path.clone());
        match std::thread::Builder::new().name("checkout-removal".to_owned())
                .spawn(move || remove_worktree(&repository, &path)) {
            Ok(handle) => PENDING_REMOVALS.lock().unwrap().push((self.resolved.revision.clone(), handle)),
            Err(_) => remove_worktree(&self.resolved.repository, &self.path)
        }
    }
}

pub fn find_running_removal() -> Option<String> {
    PENDING_REMOVALS.lock().unwrap().iter()
            .find(|(_, removal)| !removal.is_finished())
            .map(|(revision, _)| revision.clone())
}

pub fn await_checkout_removals() {
    let pending = std::mem::take(&mut *PENDING_REMOVALS.lock().unwrap());
    for (_, removal) in pending {
        let _ = removal.join();
    }
}

pub fn checkout(resolved: &ResolvedRevision) -> Result<Checkout, GitError> {
    let repository = resolved.repository.as_str();
    let path = normalise_separators(&std::env::temp_dir().join(CHECKOUT_PREFIX.to_owned()
            + &resolved.commit[..resolved.commit.len().min(12)]
            + "-" + &std::process::id().to_string()).to_string_lossy()).into_owned();
    let _ = std::fs::remove_dir_all(&path);

    // git writes the tree out on one thread unless told otherwise, and a git too old to know the
    // option ignores it, so no version check guards it
    let outcome = Command::new("git").args(["-C", repository, "-c", "checkout.workers=0",
            "worktree", "add", "--detach", "--quiet", &path, &resolved.commit])
            .output().map_err(GitError::NotInstalled)?;
    if !outcome.status.success() {
        return Err(GitError::Refused { doing: "writing out the revision",
                message: String::from_utf8_lossy(&outcome.stderr).trim().to_owned() });
    }

    Ok(Checkout { path, resolved: resolved.clone() })
}

// What a killed run left behind. 'prune' alone clears nothing while the directory still exists, so
// every worktree under the temp directory carrying this prefix and another process's id is removed
// whole, and the prune afterwards drops whatever registration has already lost its directory.
// Called once per run and never from inside 'checkout': the prune walking the registrations while a
// parallel write is half registered is the one interference between them.
pub fn remove_leftover_checkouts(repository: &str) {
    let ours = format!("-{}", std::process::id());
    let temp = fold_for_comparison(&std::env::temp_dir().to_string_lossy()).into_owned();
    if let Ok(listed) = Command::new("git").args(["-C", repository, "worktree", "list", "--porcelain"]).output() {
        for line in String::from_utf8_lossy(&listed.stdout).lines() {
            let Some(path) = line.strip_prefix("worktree ") else { continue };
            let name = Path::new(path).file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
            if name.starts_with(CHECKOUT_PREFIX) && !name.ends_with(&ours)
                    && fold_for_comparison(path).starts_with(&temp) {
                let _ = Command::new("git").args(["-C", repository, "worktree", "remove", "--force", path]).output();
            }
        }
    }
    let _ = Command::new("git").args(["-C", repository, "worktree", "prune"]).output();
}

fn remove_worktree(repository: &str, path: &str) {
    let _ = Command::new("git").args(["-C", repository, "worktree", "remove", "--force", path]).output();
}

// None when git ran and answered no, which every caller turns into its own words; an error only
// when git could not be run at all.
fn ask(at: &str, arguments: &[&str]) -> Result<Option<String>, GitError> {
    let outcome = Command::new("git").arg("-C").arg(at).args(arguments).output()
            .map_err(GitError::NotInstalled)?;
    if !outcome.status.success() {
        return Ok(None);
    }

    Ok(Some(String::from_utf8_lossy(&outcome.stdout).trim().to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Anchored on the manifest and never on a bare relative path: cargo runs these from the package
    // root, which is one directory below the repository.
    const PACKAGE : &str = env!("CARGO_MANIFEST_DIR");

    #[test]
    fn a_path_is_answered_with_its_repository_and_its_place_inside_it() {
        let (root, prefix) = find_repository_of(&(PACKAGE.to_owned() + "/src")).unwrap();
        assert_eq!("mezura/src/", prefix);
        assert!(PACKAGE.replace('\\', "/").starts_with(&root), "'{root}' is not above '{PACKAGE}'");

        let (same, none) = find_repository_of(&root).unwrap();
        assert_eq!(root, same);
        assert!(none.is_empty(), "'{none}'");

        let (file_root, file_prefix) = find_repository_of(&(PACKAGE.to_owned() + "/src/main.rs")).unwrap();
        assert_eq!((root.as_str(), "mezura/src/main.rs"), (file_root.as_str(), file_prefix.as_str()));

        let outside = std::env::temp_dir().to_string_lossy().replace('\\', "/");
        assert!(matches!(find_repository_of(&outside), Err(GitError::NotARepository { .. })),
                "'{outside}' was claimed by a repository");
    }

    #[test]
    fn targets_from_two_repositories_are_refused_and_both_are_named() {
        let (root, _) = find_repository_of(PACKAGE).unwrap();
        let (here, sibling) = (PACKAGE.to_owned() + "/src", root.clone() + "/mezura-core");
        assert_eq!(root, find_common_repository_of(&[here.clone(), sibling]).unwrap());

        let outside = std::env::temp_dir().to_string_lossy().replace('\\', "/");
        assert!(matches!(find_common_repository_of(&[here, outside]), Err(GitError::NotARepository { .. })));
    }

    #[test]
    fn a_revision_is_written_out_and_taken_away_again() {
        let (root, _) = find_repository_of(PACKAGE).unwrap();
        let path = {
            let checkout = checkout(&resolve_revision(&root, "HEAD").unwrap()).unwrap();
            assert!(Path::new(&checkout.path).join("mezura/src/main.rs").exists(),
                    "the revision was not written out to {}", checkout.path);
            assert!(checkout.find_target_of("mezura-core/src/").is_some());
            assert_eq!(None, checkout.find_target_of("a-directory-this-commit-never-had/"));
            // The repository root's prefix is empty, and its target must not end in a separator
            // or no file path would match it on a whole-component check
            let root = checkout.find_target_of("").unwrap();
            assert!(!root.ends_with('/'), "{root}");
            checkout.path.clone()
        };
        await_checkout_removals();
        assert!(!Path::new(&path).exists(), "the checkout outlived the run that made it");
    }

    #[test]
    fn every_spelling_of_one_commit_resolves_to_one_hash() {
        let (root, _) = find_repository_of(PACKAGE).unwrap();
        let by_name = resolve_revision(&root, "HEAD").unwrap();
        let by_hash = resolve_revision(&root, &by_name.commit).unwrap();
        assert_eq!(by_name.commit, by_hash.commit);
        assert_eq!(40, by_name.commit.len(), "'{}'", by_name.commit);
        assert!(!by_name.taken_at.is_empty(), "the commit date came back empty");

        assert!(matches!(resolve_revision(&root, "no-such-branch-anywhere"),
                Err(GitError::NoSuchRevision { .. })));
    }
}
