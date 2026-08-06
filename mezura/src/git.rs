// This file handles the comparisons against a git commit.
// Resolves the repository to be compared, builds the worktree of the commit and provides
// the temp location, and cleans up after itself.
use std::path::Path;
use std::process::Command;

// Named after the run rather than after the revision, since two runs of one revision can overlap and
// a worktree cannot be added twice at one place
const CHECKOUT_PREFIX : &str = "mezura-diff-";

#[derive(Debug)]
pub enum GitError {
    NotInstalled(std::io::Error),
    NotARepository { path: String },
    // Counting a revision means finding each target inside a checkout of one repository, by what
    // follows that repository's root in the target's path. Targets from two repositories have two
    // roots, and the revision names a different commit in each.
    TwoRepositories { first: String, second: String },
    NoSuchRevision { revision: String, repository: String },
    Refused { doing: &'static str, message: String }
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInstalled(x) => write!(f, "git could not be run, so a revision cannot be counted: {x}."),
            Self::NotARepository { path } => write!(f, "'{path}' is not inside a git repository, so there is no revision of it to count."),
            Self::TwoRepositories { first, second } => write!(f, "the targets are in two different repositories, '{first}' and '{second}', so a revision names two different things. Count them one repository at a time."),
            Self::NoSuchRevision { revision, repository } => write!(f, "'{revision}' is not a branch, tag or commit of the repository at '{repository}', and there is no file by that name either."),
            Self::Refused { doing, message } => write!(f, "git refused while {doing}: {message}")
        }
    }
}

impl std::error::Error for GitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotInstalled(x) => Some(x),
            _ => None
        }
    }
}

// The repository a path belongs to, and where the path sits inside it. Both come from git and
// neither is worked out here: 'rev-parse' walks up looking for the repository the way every git
// command does, and the prefix it answers with is already the path to use inside a checkout.
pub fn repository_of(path: &str) -> Result<(String, String), GitError> {
    let root = ask(path, &["rev-parse", "--show-toplevel"])?
            .ok_or_else(|| GitError::NotARepository { path: path.to_owned() })?;
    let prefix = ask(path, &["rev-parse", "--show-prefix"])?
            .ok_or_else(|| GitError::NotARepository { path: path.to_owned() })?;

    Ok((root, prefix))
}

// The one repository every path belongs to, refusing the moment two of them disagree.
pub fn one_repository_of(paths: &[String]) -> Result<String, GitError> {
    let mut found : Option<String> = None;
    for path in paths {
        let (root, _) = repository_of(path)?;
        match &found {
            Some(first) if *first != root => return Err(GitError::TwoRepositories {
                    first: first.clone(), second: root }),
            Some(_) => (),
            None => found = Some(root)
        }
    }

    found.ok_or_else(|| GitError::NotARepository { path: String::new() })
}

// A commit is compressed objects inside '.git' and mezura counts files, so a revision has to be
// written out before it can be read. Removed when this goes out of scope, whichever way the run
// ended, and a run that was killed before that is cleaned up by the prune of the next one.
pub struct Checkout {
    pub path: String,
    // The full hash the revision resolved to. 'HEAD' names a different commit next week, so what
    // was asked for identifies nothing later, and this is the one thing that does.
    pub commit: String,
    // When the commit was made, which is when this reading was taken: the files in here are that
    // moment, whatever the clock says while they are being counted
    pub taken_at: String,
    repository: String
}

impl Checkout {
    // 'target_of' answers where a path of the working tree is to be found in here, which is what the
    // prefix from 'repository_of' is for. None when the revision predates it: a directory that did
    // not exist then counts zero rather than stopping the run.
    pub fn target_of(&self, prefix: &str) -> Option<String> {
        let inside = self.path.clone() + "/" + prefix.trim_end_matches('/');
        Path::new(&inside).exists().then_some(inside)
    }
}

impl Drop for Checkout {
    fn drop(&mut self) {
        let _ = Command::new("git").args(["-C", &self.repository, "worktree", "remove", "--force", &self.path])
                .output();
    }
}

pub fn checkout(repository: &str, revision: &str) -> Result<Checkout, GitError> {
    let commit = ask(repository, &["rev-parse", "--verify", &format!("{revision}^{{commit}}")])?
            .ok_or_else(|| GitError::NoSuchRevision { revision: revision.to_owned(),
                    repository: repository.to_owned() })?;

    // Whatever a run that was killed left behind under '.git/worktrees', before adding to it
    let _ = Command::new("git").args(["-C", repository, "worktree", "prune"]).output();

    let path = std::env::temp_dir().join(CHECKOUT_PREFIX.to_owned() + &commit[..commit.len().min(12)]
            + "-" + &std::process::id().to_string()).to_string_lossy().replace('\\', "/");
    let _ = std::fs::remove_dir_all(&path);

    let outcome = Command::new("git").args(["-C", repository, "worktree", "add", "--detach", "--quiet",
            &path, &commit]).output().map_err(GitError::NotInstalled)?;
    if !outcome.status.success() {
        return Err(GitError::Refused { doing: "writing out the revision",
                message: String::from_utf8_lossy(&outcome.stderr).trim().to_owned() });
    }

    let taken_at = ask(repository, &["show", "-s", "--format=%cI", &commit])?.unwrap_or_default();

    Ok(Checkout { path, commit, taken_at, repository: repository.to_owned() })
}

// None when git ran and answered no, which every caller turns into its own words, and an error only
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

    // Both answers come from git and neither is worked out here, which is the whole point: the
    // prefix is already the path to use inside a checkout, so nothing takes a root off a path.
    // Anchored on the manifest and never on a bare relative path: cargo runs these from the package
    // root, which is one directory below the repository.
    const PACKAGE : &str = env!("CARGO_MANIFEST_DIR");

    #[test]
    fn a_path_is_answered_with_its_repository_and_its_place_inside_it() {
        let (root, prefix) = repository_of(&(PACKAGE.to_owned() + "/src")).unwrap();
        assert_eq!("mezura/src/", prefix);
        // the answer is a real ancestor of what was asked about, whichever slashes the platform uses
        assert!(PACKAGE.replace('\\', "/").starts_with(&root), "'{root}' is not above '{PACKAGE}'");

        // the root of the repository is in the repository and sits nowhere inside it
        let (same, none) = repository_of(&root).unwrap();
        assert_eq!(root, same);
        assert!(none.is_empty(), "'{none}'");

        // and a place that is in no repository says so rather than climbing to one that is not its
        let outside = std::env::temp_dir().to_string_lossy().replace('\\', "/");
        assert!(matches!(repository_of(&outside), Err(GitError::NotARepository { .. })),
                "'{outside}' was claimed by a repository");
    }

    // Every target has to be found inside one checkout by what follows one root, so two roots are
    // not accepted
    #[test]
    fn targets_from_two_repositories_are_refused_and_both_are_named() {
        let (root, _) = repository_of(PACKAGE).unwrap();
        let (here, sibling) = (PACKAGE.to_owned() + "/src", root.clone() + "/mezura-core");
        assert_eq!(root, one_repository_of(&[here.clone(), sibling]).unwrap());

        let outside = std::env::temp_dir().to_string_lossy().replace('\\', "/");
        assert!(matches!(one_repository_of(&[here, outside]), Err(GitError::NotARepository { .. })));
    }

    // What a comparison against a commit is made of: the files are written out, the place a target
    // sits in is found by its prefix, and everything goes away again.
    #[test]
    fn a_revision_is_written_out_and_taken_away_again() {
        let (root, _) = repository_of(PACKAGE).unwrap();
        let path = {
            let checkout = checkout(&root, "HEAD").unwrap();
            assert!(Path::new(&checkout.path).join("mezura/src/main.rs").exists(),
                    "the revision was not written out to {}", checkout.path);
            // the prefix of a target is where that target is inside here
            assert!(checkout.target_of("mezura-core/src/").is_some());
            // and one the revision never had counts as nothing rather than stopping the run
            assert_eq!(None, checkout.target_of("a-directory-this-commit-never-had/"));
            checkout.path.clone()
        };
        assert!(!Path::new(&path).exists(), "the checkout outlived the run that made it");

        assert!(matches!(checkout(&root, "no-such-branch-anywhere"),
                Err(GitError::NoSuchRevision { .. })));
    }
}
