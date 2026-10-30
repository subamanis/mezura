// Everything a comparison against a git commit or a count of a revision or remote repository
// needs: the repository a target belongs to, the worktree the commit is written out to, and the
// removal of it afterwards.
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;

use super::paths::{GLOB_METACHARACTERS, fold_ascii_case, fold_for_comparison,
        is_unquoted_glob_pattern, normalise_separators};

// The path built from this also carries the process id, since a worktree cannot be added twice at
// one place and two runs of one revision can overlap
const CHECKOUT_PREFIX : &str = "mezura-diff-";
const REMOTE_PREFIX : &str = "mezura-remote-";
const DEFAULT_SSH_COMMAND : &str = "ssh -o BatchMode=yes -o ConnectTimeout=15 -o ServerAliveInterval=10 -o ServerAliveCountMax=2";
const REMOTE_SCHEMES : [&str; 4] = ["https", "http", "ssh", "git"];
// Untrusted repositories must never execute hooks, fsmonitor, or smudge filters, or write symlinks
// that point outside the checkout
const HARDENED_GIT_CONFIG : [&str; 18] = [
    "-c", "core.longpaths=true",
    "-c", "core.hooksPath=/dev/null",
    "-c", "core.fsmonitor=false",
    "-c", "core.symlinks=false",
    "-c", "filter.lfs.smudge=",
    "-c", "filter.lfs.process=",
    "-c", "filter.lfs.required=false",
    "-c", "protocol.file.allow=never",
    "-c", "advice.detachedHead=false",
];
const NETWORK_GIT_CONFIG : [&str; 8] = [
    "-c", "credential.interactive=never",
    "-c", "http.connectTimeout=15",
    "-c", "http.lowSpeedLimit=1000",
    "-c", "http.lowSpeedTime=20",
];

static PENDING_REMOVALS : Mutex<Vec<(String, JoinHandle<()>)>> = Mutex::new(Vec::new());
static REMOTE_HOLDERS : Mutex<Vec<String>> = Mutex::new(Vec::new());
static NEXT_CHECKOUT_ID : AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static TEST_GIT_SSH : Mutex<Option<String>> = Mutex::new(None);
#[cfg(test)]
pub(crate) static GIT_TEST_LOCK : Mutex<()> = Mutex::new(());

#[derive(Debug)]
pub enum GitError {
    NotInstalled(std::io::Error),
    NotARepository { path: String },
    TwoRepositories { first: String, second: String },
    NoSuchRevision { revision: String, repository: String },
    NoSuchRemoteRevision { revision: String, repository: String },
    EmptyRevision { target: String },
    MissingTargetInRevision { target: String, revision: String },
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
            Self::NoSuchRemoteRevision { revision, repository } => write!(f, "'{revision}' is not a branch, tag or commit of '{repository}'."),
            Self::EmptyRevision { target } => write!(f, "'{target}' ends in '@' with no branch, tag or commit after it."),
            Self::MissingTargetInRevision { target, revision } => write!(f, "'{target}' does not exist in '{revision}'."),
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetSpec {
    LocalPath(String),
    LocalRevision {
        path: String,
        revision: String,
        display: String
    },
    Remote(RemoteSpec)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteSpec {
    pub url: String,
    pub repo_name: String,
    pub revision: Option<String>,
    pub display: String
}

impl RemoteSpec {
    pub fn format_short_label(&self) -> String {
        match &self.revision {
            Some(revision) => format!("{}@{revision}", self.repo_name),
            None => self.repo_name.clone()
        }
    }
}

pub fn parse_target_spec(raw: &str) -> Result<TargetSpec, GitError> {
    let trimmed = raw.trim();
    if !trimmed.is_empty() && Path::new(trimmed).exists() {
        return Ok(TargetSpec::LocalPath(trimmed.to_owned()));
    }
    if let Some(remote) = parse_remote_spec(trimmed)? {
        return Ok(TargetSpec::Remote(remote));
    }
    if let Some((before_at, after_at)) = trimmed.rsplit_once('@') {
        if after_at.is_empty() {
            return Err(GitError::EmptyRevision { target: trimmed.to_owned() });
        }
        // A pattern over filenames containing '@' (such as 'src/*@*.rs' or 'src/@types/*.ts') is a
        // local glob and not a revision suffix
        if after_at.contains(GLOB_METACHARACTERS) || (before_at.contains(GLOB_METACHARACTERS) && after_at.contains(['/', '\\'])) {
            return Ok(TargetSpec::LocalPath(trimmed.to_owned()));
        }
        if is_unquoted_glob_pattern(before_at) {
            return Err(GitError::PatternTarget { pattern: trimmed.to_owned() });
        }
        let path = if before_at.is_empty() { ".".to_owned() } else { before_at.to_owned() };
        let absolute = mezura_core::convert_to_absolute(&path);
        let display = format!("{absolute}@{after_at}");
        return Ok(TargetSpec::LocalRevision { path, revision: after_at.to_owned(), display });
    }

    Ok(TargetSpec::LocalPath(trimmed.to_owned()))
}

pub fn parse_remote_spec(raw: &str) -> Result<Option<RemoteSpec>, GitError> {
    let trimmed = raw.trim();
    if let Some((scheme, after_scheme)) = trimmed.split_once("://")
            && REMOTE_SCHEMES.iter().any(|known| scheme.eq_ignore_ascii_case(known)) {
        let Some(slash_at) = after_scheme.find('/') else {
            return Ok(None);
        };
        let (authority, path_part) = (&after_scheme[..slash_at], after_scheme[slash_at + 1..].trim_end_matches('/'));
        if authority.is_empty() || path_part.is_empty() {
            return Ok(None);
        }
        return build_remote_spec(&format!("{scheme}://{authority}"), "/", path_part, trimmed).map(Some);
    }

    if !trimmed.starts_with(['.', '/', '\\'])
            && let Some((before_colon, after_colon)) = trimmed.split_once(':')
            && !before_colon.contains(['/', '\\'])
            && let Some((user, host)) = before_colon.split_once('@')
            && !user.is_empty() && !host.is_empty() && !host.contains('@') {
        let path_part = after_colon.trim_end_matches('/');
        if path_part.is_empty() {
            return Ok(None);
        }
        return build_remote_spec(before_colon, ":", path_part, trimmed).map(Some);
    }

    Ok(None)
}

pub fn is_checkout_target(raw: &str) -> bool {
    !matches!(parse_target_spec(raw), Ok(TargetSpec::LocalPath(_)))
}

pub fn format_revision_label(asked_for: &str) -> String {
    match parse_target_spec(asked_for) {
        Ok(TargetSpec::Remote(remote)) => remote.format_short_label(),
        _ => asked_for.to_owned()
    }
}

pub fn strip_dot_git(text: &str) -> &str {
    let trimmed = text.trim_end_matches('/');
    trimmed.strip_suffix(".git").unwrap_or(trimmed)
}

// The repository a path belongs to, and where the path sits inside it. Both come from git: the
// prefix it answers with is already the path to use inside a checkout, so nothing here takes a root
// off a path. A path that was deleted since climbs to the nearest folder that still exists and puts
// the missing tail back on the prefix.
pub fn find_repository_of(path: &str) -> Result<(String, String), GitError> {
    let as_path = Path::new(path);
    if !as_path.is_dir() {
        let mut current = as_path;
        let mut tail = Vec::new();
        while !current.is_dir() {
            let Some(name) = current.file_name() else { break };
            tail.push(name.to_string_lossy().into_owned());
            let Some(parent) = current.parent() else { break };
            current = if parent.as_os_str().is_empty() { Path::new(".") } else { parent };
        }
        if current.is_dir() && !tail.is_empty() {
            let (root, prefix) = find_repository_of(&current.to_string_lossy())
                    .map_err(|_| GitError::NotARepository { path: path.to_owned() })?;
            tail.reverse();
            return Ok((root, prefix + &tail.join("/")));
        }
    }

    if is_remote_temp_repository(path) {
        return Ok((normalise_separators(path).into_owned(), String::new()));
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
            Some(first) if fold_for_comparison(first) != fold_for_comparison(&root) =>
                return Err(GitError::TwoRepositories { first: first.clone(), second: root }),
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
// fails 'rev-parse --verify'. On a temporary remote repository, a revision that has not been
// fetched yet is fetched from 'origin' on the spot.
pub fn resolve_revision(repository: &str, revision: &str) -> Result<ResolvedRevision, GitError> {
    if let Some(resolved) = resolve_commit_and_date(repository, &format!("{revision}^{{commit}}"), revision)? {
        return Ok(resolved);
    }
    if is_remote_temp_repository(repository)
            && let Some(url) = ask(repository, &["remote", "get-url", "origin"])? {
        return fetch_revision_into_remote(repository, &url, Some(revision));
    }

    Err(GitError::NoSuchRevision { revision: revision.to_owned(), repository: repository.to_owned() })
}

pub fn fetch_remote_revision(remote: &RemoteSpec) -> Result<ResolvedRevision, GitError> {
    remove_leftover_remotes();
    let repo_dir = ensure_remote_repository(&remote.url, &remote.repo_name)?;
    fetch_revision_into_remote(&repo_dir, &remote.url, remote.revision.as_deref())
}

// git keeps no empty folder, so every folder listed holds a tracked file
pub fn collect_folders_at(repository: &str, commit: &str) -> Option<HashSet<String>> {
    let listed = ask(repository, &["ls-tree", "-r", "-d", "-z", "--name-only", commit]).ok()??;
    Some(listed.split('\0').filter(|folder| !folder.is_empty()).map(|folder| fold_ascii_case(folder).into_owned()).collect())
}

// The tree of a commit, written out where mezura can count it. Removed when this goes out of scope,
// whichever way the run ended, and a run that was killed before that is swept by the next one.
#[derive(Debug)]
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
    let holders = std::mem::take(&mut *REMOTE_HOLDERS.lock().unwrap());
    for holder in holders {
        let _ = remove_directory_tree(Path::new(&holder));
    }
}

pub fn is_remote_temp_repository(repository: &str) -> bool {
    let check = |path: &str| Path::new(path).parent().and_then(|parent| parent.file_name())
            .is_some_and(|name| is_a_holder_of_a_remote(&name.to_string_lossy()));
    if check(repository) {
        return true;
    }
    ask(repository, &["rev-parse", "--git-common-dir"]).ok().flatten()
            .is_some_and(|common| check(&normalise_separators(&common)))
}

// The tree is written into a folder named like the repository's own, inside one of this run's, so
// that a name pattern sees the same folders on both sides of the comparison
pub fn checkout(resolved: &ResolvedRevision) -> Result<Checkout, GitError> {
    let repository = resolved.repository.as_str();
    let short_commit = &resolved.commit[..resolved.commit.len().min(12)];
    let seq = NEXT_CHECKOUT_ID.fetch_add(1, Ordering::Relaxed);
    let holder = normalise_separators(&std::env::temp_dir()
            .join(if seq == 0 {
                format!("{CHECKOUT_PREFIX}{short_commit}-{}", std::process::id())
            } else {
                format!("{CHECKOUT_PREFIX}{short_commit}-{seq}-{}", std::process::id())
            })
            .to_string_lossy()).into_owned();
    let _ = remove_directory_tree(Path::new(&holder));
    let name = Path::new(repository.trim_end_matches('/')).file_name()
            .map_or_else(|| "repository".to_owned(), |x| x.to_string_lossy().into_owned());
    let path = format!("{holder}/{name}");
    std::fs::create_dir_all(&holder).map_err(|error| GitError::Refused { doing: "making room for the revision",
            message: error.to_string() })?;

    // git writes the tree out on one thread unless told otherwise, and a git too old to know the
    // option ignores it, so no version check guards it
    let mut command = Command::new("git");
    configure_hardened_git(&mut command);
    let outcome = command.args(["-C", repository, "-c", "checkout.workers=0",
            "worktree", "add", "--detach", "--force", "--quiet", &path, &resolved.commit])
            .output().map_err(GitError::NotInstalled)?;
    if !outcome.status.success() {
        let _ = remove_directory_tree(Path::new(&holder));
        return Err(GitError::Refused { doing: "writing out the revision",
                message: String::from_utf8_lossy(&outcome.stderr).trim().to_owned() });
    }

    Ok(Checkout { path, resolved: resolved.clone() })
}

// What a killed run left behind. 'prune' alone clears nothing while the directory still exists, so
// every worktree under the temp directory whose holding folder carries this prefix and another
// process's id is removed whole, and the prune afterwards drops whatever registration has already
// lost its directory. A holding folder whose worktree is already gone is removed last, since a run
// killed between the two leaves it empty. Called once per run and never from inside 'checkout'. The
// prune walking the registrations while a parallel write is half registered is the one interference
// between them.
pub fn remove_leftover_checkouts(repository: &str) {
    let ours = format!("-{}", std::process::id());
    let temp = std::env::temp_dir();
    let folded_temp = fold_for_comparison(&temp.to_string_lossy()).into_owned();
    let is_a_leftover = |name: &str| is_a_holder_of_a_checkout(name) && !name.ends_with(&ours);
    if let Ok(listed) = Command::new("git").args(["-C", repository, "worktree", "list", "--porcelain"]).output() {
        for line in String::from_utf8_lossy(&listed.stdout).lines() {
            let Some(path) = line.strip_prefix("worktree ") else { continue };
            let Some(holder) = Path::new(path).parent() else { continue };
            let name = holder.file_name().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
            if is_a_leftover(&name) && fold_for_comparison(path).starts_with(&folded_temp) {
                remove_worktree(repository, path);
            }
        }
    }
    let _ = Command::new("git").args(["-C", repository, "worktree", "prune"]).output();
    remove_leftover_holders(&temp, CHECKOUT_PREFIX, &ours);
}

pub(crate) fn remove_directory_tree(path: &Path) -> std::io::Result<()> {
    clear_readonly_recursively(path);
    std::fs::remove_dir_all(path)
}

fn clear_readonly_recursively(path: &Path) {
    let Ok(entries) = std::fs::read_dir(path) else { return };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        if let Ok(file_type) = entry.file_type() && file_type.is_dir() {
            clear_readonly_recursively(&entry_path);
        } else if let Ok(metadata) = entry.metadata() {
            let mut permissions = metadata.permissions();
            if permissions.readonly() {
                #[expect(clippy::permissions_set_readonly_false)]
                permissions.set_readonly(false);
                let _ = std::fs::set_permissions(&entry_path, permissions);
            }
        }
    }
}

fn remove_leftover_remotes() {
    let ours = format!("-{}", std::process::id());
    remove_leftover_holders(&std::env::temp_dir(), REMOTE_PREFIX, &ours);
}

fn remove_leftover_holders(temp: &Path, prefix: &str, ours: &str) {
    if let Ok(entries) = std::fs::read_dir(temp) {
        for holder in entries.flatten().filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            is_a_holder_with_prefix(&name, prefix) && !name.ends_with(ours)
        }) {
            let _ = remove_directory_tree(&holder.path());
        }
    }
}

fn ensure_remote_repository(url: &str, repo_name: &str) -> Result<String, GitError> {
    let hash = hash_to_twelve_hex(strip_dot_git(url));
    let holder = normalise_separators(&std::env::temp_dir().join(format!("{REMOTE_PREFIX}{hash}-{}",
            std::process::id())).to_string_lossy()).into_owned();
    let repo_dir = format!("{holder}/{repo_name}");
    if Path::new(&repo_dir).join("HEAD").exists() {
        return Ok(repo_dir);
    }

    let _ = remove_directory_tree(Path::new(&holder));
    std::fs::create_dir_all(&repo_dir).map_err(|error| GitError::Refused {
            doing: "making room for the remote repository", message: error.to_string() })?;
    let init = Command::new("git").args(["init", "--bare", "--quiet", &repo_dir])
            .output().map_err(GitError::NotInstalled)?;
    if !init.status.success() {
        let _ = remove_directory_tree(Path::new(&holder));
        return Err(GitError::Refused { doing: "initializing the temporary repository",
                message: String::from_utf8_lossy(&init.stderr).trim().to_owned() });
    }
    let remote_add = Command::new("git").args(["-C", &repo_dir, "remote", "add", "origin", url])
            .output().map_err(GitError::NotInstalled)?;
    if !remote_add.status.success() {
        let _ = remove_directory_tree(Path::new(&holder));
        return Err(GitError::Refused { doing: "configuring the remote repository",
                message: String::from_utf8_lossy(&remote_add.stderr).trim().to_owned() });
    }
    REMOTE_HOLDERS.lock().unwrap().push(holder);

    Ok(repo_dir)
}

fn fetch_revision_into_remote(repo_dir: &str, url: &str, revision: Option<&str>) -> Result<ResolvedRevision, GitError> {
    let asked = revision.unwrap_or("HEAD");
    let primary = run_remote_git(repo_dir, &["fetch", "--depth", "1", "--quiet", "--no-tags", "origin", asked])?;
    if primary.status.success()
            && let Some(resolved) = pin_fetched_commit(repo_dir, "FETCH_HEAD^{commit}", asked)? {
        return Ok(resolved);
    }

    // Git's wire protocol does not expand abbreviated commit hashes or relative expressions in
    // '--depth 1' fetches, so those fall back to a blobless fetch and resolve locally
    if is_short_commit_sha(asked) || asked.contains(['~', '^']) {
        let mut fetch_args = vec!["fetch", "--filter=blob:none"];
        if Path::new(repo_dir).join("shallow").exists() {
            fetch_args.push("--unshallow");
        }
        fetch_args.extend(["--tags", "--quiet", "origin", "+refs/heads/*:refs/heads/*", "+HEAD:refs/mezura/head"]);
        if run_remote_git(repo_dir, &fetch_args)?.status.success() {
            let candidate = asked.strip_prefix("HEAD")
                    .map_or_else(|| asked.to_owned(), |rest| format!("refs/mezura/head{rest}"));
            if let Some(resolved) = pin_fetched_commit(repo_dir, &format!("{candidate}^{{commit}}"), asked)?
                    .or(pin_fetched_commit(repo_dir, &format!("{asked}^{{commit}}"), asked)?) {
                return Ok(resolved);
            }
            return Err(GitError::NoSuchRemoteRevision { revision: asked.to_owned(),
                    repository: strip_dot_git(url).to_owned() });
        }
    }

    let message = String::from_utf8_lossy(&primary.stderr).trim().to_owned();
    if revision.is_some() && is_missing_remote_ref_error(&message) {
        return Err(GitError::NoSuchRemoteRevision { revision: asked.to_owned(),
                repository: strip_dot_git(url).to_owned() });
    }

    Err(GitError::Refused { doing: "fetching the remote repository", message })
}

fn resolve_commit_and_date(repository: &str, object: &str, revision: &str) -> Result<Option<ResolvedRevision>, GitError> {
    let Some(answer) = ask(repository, &["show", "-s", "--format=%H%n%cI", object])? else {
        return Ok(None);
    };
    let (commit, taken_at) = answer.split_once('\n').unwrap_or((&answer, ""));
    Ok(Some(ResolvedRevision {
        repository: repository.to_owned(),
        revision: revision.to_owned(),
        commit: commit.trim().to_owned(),
        taken_at: taken_at.trim().to_owned()
    }))
}

fn pin_fetched_commit(repo_dir: &str, object: &str, revision: &str) -> Result<Option<ResolvedRevision>, GitError> {
    let Some(resolved) = resolve_commit_and_date(repo_dir, object, revision)? else {
        return Ok(None);
    };
    let _ = Command::new("git").args(["-C", repo_dir, "update-ref",
            &format!("refs/mezura/{}", resolved.commit), &resolved.commit]).output();
    Ok(Some(resolved))
}

fn build_remote_spec(prefix: &str, separator: &str, path_part: &str, full_raw: &str) -> Result<RemoteSpec, GitError> {
    let (repo_path, revision) = split_revision_suffix(path_part, full_raw)?;
    let url = format!("{prefix}{separator}{repo_path}");
    let display_base = strip_dot_git(&url);
    let display = match &revision {
        Some(rev) => format!("{display_base}@{rev}"),
        None => display_base.to_owned()
    };
    Ok(RemoteSpec { url, repo_name: extract_repo_name(repo_path), revision, display })
}

fn run_remote_git(repo_dir: &str, arguments: &[&str]) -> Result<std::process::Output, GitError> {
    let mut command = Command::new("git");
    configure_hardened_git(&mut command);
    command.arg("-C").arg(repo_dir).args(arguments).output().map_err(GitError::NotInstalled)
}

fn configure_hardened_git(command: &mut Command) {
    command.env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "never")
            .env("GIT_LFS_SKIP_SMUDGE", "1");
    #[cfg(test)]
    let test_ssh = TEST_GIT_SSH.lock().unwrap().clone();
    #[cfg(not(test))]
    let test_ssh : Option<String> = None;
    if let Some(ssh) = test_ssh {
        command.env("GIT_SSH", ssh);
    } else if std::env::var_os("GIT_SSH_COMMAND").is_none() && std::env::var_os("GIT_SSH").is_none() {
        command.env("GIT_SSH_COMMAND", DEFAULT_SSH_COMMAND);
    }
    command.args(HARDENED_GIT_CONFIG).args(NETWORK_GIT_CONFIG);
}

fn is_short_commit_sha(revision: &str) -> bool {
    (7..=39).contains(&revision.len()) && revision.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_missing_remote_ref_error(stderr: &str) -> bool {
    let lower = stderr.to_ascii_lowercase();
    lower.contains("couldn't find remote ref")
            || lower.contains("not our ref")
            || lower.contains("unadvertised object")
            || lower.contains("invalid refspec")
}

fn split_revision_suffix<'a>(path_part: &'a str, full_raw: &str) -> Result<(&'a str, Option<String>), GitError> {
    match path_part.rsplit_once('@') {
        Some((_, "")) => Err(GitError::EmptyRevision { target: full_raw.to_owned() }),
        Some((before, rev)) if !before.trim_matches('/').is_empty() =>
                Ok((before.trim_end_matches('/'), Some(rev.to_owned()))),
        _ => Ok((path_part.trim_end_matches('/'), None))
    }
}

fn extract_repo_name(repo_path: &str) -> String {
    let clean = strip_dot_git(repo_path);
    clean.rsplit(['/', ':']).next().filter(|name| !name.is_empty())
            .unwrap_or("repository").to_owned()
}

fn hash_to_twelve_hex(text: &str) -> String {
    let mut hash : u64 = 0xcbf29ce484222325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:012x}", hash & 0xffffffffffff)
}

fn remove_worktree(repository: &str, path: &str) {
    let _ = Command::new("git").args(["-C", repository, "worktree", "remove", "--force", "--force", path]).output();
    if let Some(holder) = Path::new(path).parent() {
        let _ = remove_directory_tree(holder);
    }
}

// The exact shape 'checkout' writes, since the sweep deletes what it recognises
fn is_a_holder_of_a_checkout(name: &str) -> bool {
    is_a_holder_with_prefix(name, CHECKOUT_PREFIX)
}

fn is_a_holder_of_a_remote(name: &str) -> bool {
    is_a_holder_with_prefix(name, REMOTE_PREFIX)
}

fn is_a_holder_with_prefix(name: &str, prefix: &str) -> bool {
    let Some(rest) = name.strip_prefix(prefix) else {
        return false;
    };
    let Some((before_pid, pid)) = rest.rsplit_once('-') else {
        return false;
    };
    let commit = before_pid.split('-').next().unwrap_or(before_pid);
    commit.len() == 12 && commit.chars().all(|x| x.is_ascii_hexdigit())
            && !pid.is_empty() && pid.chars().all(|x| x.is_ascii_digit())
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
        let _guard = GIT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let repo = create_test_repository("mezura-checkout-test");
        let root = normalise_separators(&repo.to_string_lossy()).into_owned();
        let path = {
            let checkout = checkout(&resolve_revision(&root, "HEAD").unwrap()).unwrap();
            assert!(Path::new(&checkout.path).join("mezura/src/main.rs").exists(),
                    "the revision was not written out to {}", checkout.path);
            assert_eq!(Path::new(&root).file_name(), Path::new(&checkout.path).file_name(),
                    "the checkout is not named like the repository's folder");
            assert!(Path::new(&checkout.path).parent().unwrap().file_name().unwrap().to_string_lossy().starts_with(CHECKOUT_PREFIX));
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
        assert!(!Path::new(&path).parent().unwrap().exists(), "the folder holding the checkout outlived it");

        let killed = std::env::temp_dir().join(CHECKOUT_PREFIX.to_owned() + "0123456789ab-1");
        let locked_holder = std::env::temp_dir().join(CHECKOUT_PREFIX.to_owned() + "fedcba987654-1");
        let locked_worktree = locked_holder.join("locked-checkout");
        let ours = std::env::temp_dir().join(format!("{CHECKOUT_PREFIX}0123456789ab-{}", std::process::id()));
        let somebody_elses = std::env::temp_dir().join(CHECKOUT_PREFIX.to_owned() + "notes");
        let _ = remove_directory_tree(&locked_holder);
        for folder in [&killed, &locked_holder, &ours, &somebody_elses] {
            std::fs::create_dir_all(folder).unwrap();
        }
        assert!(Command::new("git").arg("-C").arg(&root)
                .args(["worktree", "add", "--detach", "--force", "--quiet"])
                .arg(&locked_worktree).arg("HEAD").output().unwrap().status.success());
        let locked_admin = repo.join(".git/worktrees/locked-checkout");
        std::fs::write(locked_admin.join("locked"), "initializing\n").unwrap();
        remove_leftover_checkouts(&root);
        assert!(!killed.exists(), "the empty folder a killed run left was not swept");
        assert!(!locked_holder.exists(), "the locked checkout folder a killed run left was not swept");
        assert!(!locked_admin.exists(), "a locked worktree registration left by an interrupted checkout was not swept");
        assert!(ours.exists(), "a folder of this run was swept");
        assert!(somebody_elses.exists(), "a folder that only starts with the prefix was swept");
        remove_directory_tree(&ours).unwrap();
        remove_directory_tree(&somebody_elses).unwrap();
        remove_directory_tree(&repo).unwrap();
        assert!(is_a_holder_of_a_checkout("mezura-diff-0123456789ab-1") && !is_a_holder_of_a_checkout("mezura-diff-0123456789a-1")
                && !is_a_holder_of_a_checkout("mezura-diff-0123456789ab-") && !is_a_holder_of_a_checkout("mezura-diff-0123456789ab-x"));
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

    #[test]
    fn the_folders_of_a_commit_are_listed_whole_and_folded() {
        let (root, _) = find_repository_of(PACKAGE).unwrap();
        let folders = collect_folders_at(&root, &resolve_revision(&root, "HEAD").unwrap().commit).unwrap();
        assert!(folders.contains("mezura/src") && folders.contains("mezura-core/src/engine/test_detection"), "{folders:?}");
        assert!(!folders.contains("mezura/src/main.rs"), "a file was listed among the folders");
        assert_eq!(None, collect_folders_at(&root, "no-such-branch-anywhere"));
    }

    #[test]
    fn a_target_is_split_into_its_place_and_its_revision_only_after_checking_the_disk_and_the_host() {
        let on_disk = std::env::temp_dir().join("mezura-target-spec-at@2x");
        let _ = std::fs::remove_dir_all(&on_disk);
        std::fs::create_dir_all(&on_disk).unwrap();
        let on_disk_str = normalise_separators(&on_disk.to_string_lossy()).into_owned();
        assert_eq!(TargetSpec::LocalPath(on_disk_str.clone()), parse_target_spec(&on_disk_str).unwrap(),
                "a folder that exists on disk with '@' in its name was split as a revision");
        std::fs::remove_dir_all(&on_disk).unwrap();

        assert!(matches!(parse_target_spec("@v3.2.0").unwrap(),
                TargetSpec::LocalRevision { ref path, ref revision, .. } if path == "." && revision == "v3.2.0"));
        assert!(matches!(parse_target_spec("./mezura-core@v3.1.0").unwrap(),
                TargetSpec::LocalRevision { ref path, ref revision, .. } if path == "./mezura-core" && revision == "v3.1.0"));
        assert!(matches!(parse_target_spec("https://github.com/owner/repo.git@v1.0").unwrap(),
                TargetSpec::Remote(RemoteSpec { ref url, ref repo_name, ref revision, ref display })
                if url == "https://github.com/owner/repo.git" && repo_name == "repo"
                        && revision.as_deref() == Some("v1.0") && display == "https://github.com/owner/repo@v1.0"));
        assert!(matches!(parse_target_spec("https://token@github.com/owner/repo").unwrap(),
                TargetSpec::Remote(RemoteSpec { ref url, ref repo_name, ref revision, ref display })
                if url == "https://token@github.com/owner/repo" && repo_name == "repo"
                        && revision.is_none() && display == "https://token@github.com/owner/repo"));
        assert!(matches!(parse_target_spec("git@github.com:owner/repo.git@main").unwrap(),
                TargetSpec::Remote(RemoteSpec { ref url, ref repo_name, ref revision, ref display })
                if url == "git@github.com:owner/repo.git" && repo_name == "repo"
                        && revision.as_deref() == Some("main") && display == "git@github.com:owner/repo@main"));
        assert!(matches!(parse_target_spec("./src@"), Err(GitError::EmptyRevision { .. })));
        assert!(matches!(parse_target_spec("https://github.com/owner/repo@"), Err(GitError::EmptyRevision { .. })));
        assert!(matches!(parse_target_spec("src/*@v1.0"), Err(GitError::PatternTarget { .. })));
    }

    #[test]
    fn a_remote_repository_is_fetched_by_head_by_tag_and_by_short_commit_hash_and_cleaned_up() {
        let _guard = GIT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let repo = create_test_repository("mezura-remote-origin-test");
        let root = normalise_separators(&repo.to_string_lossy()).into_owned();
        let initial_commit = resolve_revision(&root, "HEAD~2").unwrap();
        let first_tag = resolve_revision(&root, "v1.0").unwrap();
        let head = resolve_revision(&root, "HEAD").unwrap();
        assert_ne!(initial_commit.commit, first_tag.commit);
        assert_ne!(first_tag.commit, head.commit);
        let _ssh_guard = install_local_ssh_helper();

        let base_url = format!("ssh://local/{root}");
        let by_head = parse_remote_spec(&base_url).unwrap().unwrap();
        assert_eq!(head.commit, fetch_remote_revision(&by_head).unwrap().commit);

        // Fetching an untagged older commit by short hash after a shallow HEAD fetch must unshallow
        // the commit graph on the existing bare repository
        let short_sha = &initial_commit.commit[..10];
        let by_short = parse_remote_spec(&format!("{base_url}@{short_sha}")).unwrap().unwrap();
        let resolved = fetch_remote_revision(&by_short).unwrap();
        assert_eq!(initial_commit.commit, resolved.commit);

        let by_relative = parse_remote_spec(&format!("{base_url}@HEAD~1")).unwrap().unwrap();
        assert_eq!(first_tag.commit, fetch_remote_revision(&by_relative).unwrap().commit);

        let by_tag = parse_remote_spec(&format!("{base_url}@v1.0")).unwrap().unwrap();
        assert_eq!(first_tag.commit, fetch_remote_revision(&by_tag).unwrap().commit);

        let checkout = checkout(&resolved).unwrap();
        assert!(Path::new(&checkout.path).join("mezura/src/main.rs").exists());
        let repo_dir = resolved.repository.clone();
        drop(checkout);
        await_checkout_removals();
        remove_directory_tree(&repo).unwrap();
        assert!(!Path::new(&repo_dir).exists(), "the temporary remote repository outlived the run");
    }

    fn create_test_repository(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(name);
        let _ = remove_directory_tree(&root);
        std::fs::create_dir_all(root.join("mezura/src")).unwrap();
        std::fs::create_dir_all(root.join("mezura-core/src")).unwrap();
        std::fs::write(root.join("mezura/src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(root.join("mezura-core/src/lib.rs"), "pub fn run() {}\n").unwrap();
        let git = |arguments: &[&str]| {
            let outcome = Command::new("git").arg("-C").arg(&root).args(arguments).output().unwrap();
            assert!(outcome.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&outcome.stderr));
        };
        git(&["init", "-q"]);
        git(&["config", "uploadpack.allowFilter", "true"]);
        git(&["config", "uploadpack.allowAnySHA1InWant", "true"]);
        git(&["add", "."]);
        git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "--no-verify", "-q", "-m", "c0"]);
        std::fs::write(root.join("mezura/src/main.rs"), "fn main() {\n    let _ = 0;\n}\n").unwrap();
        git(&["add", "."]);
        git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "--no-verify", "-q", "-m", "c1"]);
        git(&["tag", "v1.0"]);
        std::fs::write(root.join("mezura/src/main.rs"), "fn main() {\n    let _ = 1;\n}\n").unwrap();
        git(&["add", "."]);
        git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "--no-verify", "-q", "-m", "c2"]);
        root
    }

    struct LocalSshGuard(std::path::PathBuf);

    impl Drop for LocalSshGuard {
        fn drop(&mut self) {
            *TEST_GIT_SSH.lock().unwrap_or_else(|e| e.into_inner()) = None;
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn install_local_ssh_helper() -> LocalSshGuard {
        let path = if cfg!(windows) {
            let path = std::env::temp_dir().join("mezura-test-git-ssh.bat");
            std::fs::write(&path,
                    "@echo off\r\n\
                     if \"%1\"==\"-G\" exit /b 1\r\n\
                     for /f \"tokens=2 delims='\" %%p in (%2) do set \"p=%%p\"\r\n\
                     if \"%p:~0,1%\"==\"/\" if \"%p:~2,1%\"==\":\" set \"p=%p:~1%\"\r\n\
                     git upload-pack \"%p%\"\r\n").unwrap();
            path
        } else {
            let path = std::env::temp_dir().join("mezura-test-git-ssh.sh");
            std::fs::write(&path, "#!/bin/sh\n[ \"$1\" = \"-G\" ] && exit 1\neval \"exec $2\"\n").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            path
        };
        *TEST_GIT_SSH.lock().unwrap_or_else(|e| e.into_inner()) = Some(path.to_string_lossy().into_owned());
        LocalSshGuard(path)
    }
}
