// A revision of a repository read from git's objects, with nothing written to disk. The engine is
// handed it as a mounted tree under a root whose last folder is named like the repository's, and
// every path it reports is moved back under the repository, so it pairs with the working tree's.
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};

use mezura_core::MountedTree;

use super::git::{GitError, ResolvedRevision, build_git_fetch, build_git_read, is_true_to_git, read_git_config,
        read_scoped_git_settings};
use super::line_endings::{CheckoutSettings, Conversion, convert_line_endings, decide_conversions, read_as_git_writes_it};
use super::paths::fold_ascii_case;

const LINK_MODE : &str = "120000";
const SUBMODULE_MODE : &str = "160000";

#[derive(Debug)]
pub struct MountedRevision {
    pub path: String,
    pub resolved: ResolvedRevision,
    pub tree: Arc<MountedTree>,
    // Folded where the file system folds names. A submodule holds no file in the revision.
    submodules: Vec<String>
}

impl MountedRevision {
    // From the prefix 'find_repository_of' answered with, trimmed of its separators. The prefix of
    // the repository's root is empty, and a target ending in '/' matches no file path on a check by
    // whole components.
    pub fn find_target_of(&self, prefix: &str) -> TargetInRevision {
        let inside = prefix.trim_matches('/');
        let path = format!("{}/{inside}", self.path).trim_end_matches('/').to_owned();
        if self.tree.holds(Path::new(&path)) {
            TargetInRevision::At(path)
        } else if !inside.is_empty() && self.submodules.iter().any(|submodule| *submodule == fold_ascii_case(inside)) {
            TargetInRevision::HoldingNoFile
        } else {
            TargetInRevision::Missing
        }
    }

    pub fn read_file(&self, path: &str) -> Option<Vec<u8>> {
        let inside = super::sources::replace_root_prefix(path, &self.path, "")?;
        let object = format!("{}:{}", self.resolved.commit, inside.trim_start_matches('/'));
        let outcome = build_git_read(&self.resolved.repository).args(["cat-file", "blob", &object]).output().ok()?;
        outcome.status.success().then_some(outcome.stdout)
    }
}

// Where a path of the working tree is to be found in a revision
pub enum TargetInRevision {
    At(String),
    // A submodule, which holds no file of the revision and is no missing target
    HoldingNoFile,
    Missing
}

// The listing names every file of the revision, and a file's size is learned when it is read
pub fn mount_revision(resolved: &ResolvedRevision) -> Result<MountedRevision, GitError> {
    let (repository, remote) = (resolved.repository.as_str(), super::git::is_remote_temp_repository(&resolved.repository));
    let (files, blobs, submodules) = list_revision(resolved, remote)?;
    let path = super::git::find_place_for_revision(resolved);
    let blobs = Arc::new(blobs);
    let tree = MountedTree::new(&path, (!remote).then_some(repository), files, move |at, buf| blobs.read(at, buf));
    Ok(MountedRevision { path, resolved: resolved.clone(), tree: Arc::new(tree), submodules })
}

fn list_revision(resolved: &ResolvedRevision, remote: bool) -> Result<(Vec<PathBuf>, BlobReader, Vec<String>), GitError> {
    let repository = resolved.repository.as_str();
    fetch_missing_objects(resolved)?;
    let settings = CheckoutSettings::read(repository);
    let listing = read_output_of(build_git_read(repository).args(["ls-tree", "-r", "-z", "--full-tree", &resolved.commit]),
            "listing the files of the revision")?;
    // A remote has no working tree for its links to agree with
    let links_are_files = !remote && !settings.symlinks;

    let mut files = Vec::new();
    let mut ids = Vec::new();
    let mut listed_paths = Vec::new();
    let mut links = Vec::new();
    let mut submodules = Vec::new();
    for record in listing.split(|byte| *byte == 0) {
        let Some(tab) = record.iter().position(|byte| *byte == b'\t') else { continue };
        let (head, path) = (String::from_utf8_lossy(&record[..tab]), String::from_utf8_lossy(&record[tab + 1..]));
        let mut fields = head.split(' ');
        let (Some(mode), Some(_), Some(id)) = (fields.next(), fields.next(), fields.next()) else { continue };
        match mode {
            SUBMODULE_MODE => submodules.push(fold_ascii_case(&path).into_owned()),
            LINK_MODE if !links_are_files => {},
            _ => {
                if mode == LINK_MODE {
                    links.push(files.len());
                }
                files.push(PathBuf::from(path.as_ref()));
                ids.push(id.to_owned());
                listed_paths.push(&record[tab + 1..]);
            }
        }
    }

    let mut blobs = BlobReader { repository: repository.to_owned(), commit: resolved.commit.clone(), ids, conversions: Vec::new(),
            paths_git_writes_out: HashMap::new(), pool: Mutex::new(Pool::default()), returned: Condvar::new(),
            most: std::thread::available_parallelism().map_or(1, std::num::NonZero::get) };
    let (conversions, paths_git_writes_out) = decide_conversions(&settings, repository, &resolved.commit, &listed_paths, &links,
            |at, buf| blobs.read(at, buf));
    (blobs.conversions, blobs.paths_git_writes_out) = (conversions, paths_git_writes_out);
    Ok((files, blobs, submodules))
}

// No read fetches an object, so what a partial clone lacks of the revision is fetched here first. A
// clone without trees learns which blobs it lacks once the trees are in, hence the rounds. The fetch
// offers the remote no commit, or the remote takes the objects for known and sends none. A clone made
// by a git before 2.24 names its remote in 'extensions.partialclone' only.
fn fetch_missing_objects(resolved: &ResolvedRevision) -> Result<(), GitError> {
    let repository = resolved.repository.as_str();
    let promisors = read_git_config(repository, &["--get-regexp", r"^remote\..*\.promisor$"]).unwrap_or_default();
    let Some(promisor) = promisors.lines().filter_map(|line| line.split_once(' '))
            .find(|(_, value)| is_true_to_git(Some(value.trim())))
            .and_then(|(key, _)| Some(key.strip_prefix("remote.")?.strip_suffix(".promisor")?.to_owned()))
            .or_else(|| read_git_config(repository, &["--get", "extensions.partialclone"]).filter(|name| !name.is_empty())) else {
        return Ok(());
    };
    let mut asked_for = String::new();
    loop {
        let objects = read_output_of(build_git_read(repository).args(["rev-list", "--objects", "--no-walk", "--missing=print",
                &resolved.commit]), "listing the objects the revision lacks")?;
        let missing = String::from_utf8_lossy(&objects).lines().filter_map(|line| line.strip_prefix('?'))
                .map(|id| format!("{id}\n")).collect::<String>();
        if missing.is_empty() || missing == asked_for {
            return Ok(());
        }
        let mut command = build_git_fetch(repository);
        if asked_for.is_empty() && let Some(setting) = find_setting_naming_a_fetch_program(repository, &command) {
            return Err(GitError::FetchRunsRepositoryProgram { repository: repository.to_owned(),
                    revision: resolved.revision.clone(), setting });
        }
        let mut fetch = command.args(["-c", "fetch.negotiationAlgorithm=noop", "-c", "fetch.bundleURI=", "fetch", "--no-tags", "--no-auto-gc",
                "--no-write-fetch-head", "--recurse-submodules=no", "--filter=blob:none", "--stdin", "--", &promisor])
                .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(GitError::NotInstalled)?;
        if let Some(mut input) = fetch.stdin.take() {
            let _ = input.write_all(missing.as_bytes());
        }
        let outcome = fetch.wait_with_output().map_err(GitError::NotInstalled)?;
        if !outcome.status.success() {
            return Err(GitError::Refused { doing: "fetching the files of the revision",
                    message: String::from_utf8_lossy(&outcome.stderr).trim().to_owned() });
        }
        asked_for = missing;
    }
}

// Only the repository's own configuration counts. 'GIT_SSH_COMMAND' outranks 'core.sshCommand'.
fn find_setting_naming_a_fetch_program(repository: &str, fetch: &Command) -> Option<String> {
    let ssh_command_is_given = fetch.get_envs().find(|(name, _)| *name == "GIT_SSH_COMMAND")
            .map_or_else(|| std::env::var_os("GIT_SSH_COMMAND").is_some(), |(_, value)| value.is_some());
    read_scoped_git_settings(repository, r"^(core\.(gitproxy|sshcommand)|credential\.(.*\.)?helper|remote\..*\.vcs)$")
            .into_iter().filter(|(scope, _, _)| matches!(scope.as_str(), "local" | "worktree"))
            .find(|(_, name, program)| !program.trim().is_empty() && !(name == "core.sshcommand" && ssh_command_is_given))
            .map(|(_, name, _)| name)
}

fn read_output_of(command: &mut Command, doing: &'static str) -> Result<Vec<u8>, GitError> {
    let outcome = command.output().map_err(GitError::NotInstalled)?;
    match outcome.status.success() {
        true => Ok(outcome.stdout),
        false => Err(GitError::Refused { doing, message: String::from_utf8_lossy(&outcome.stderr).trim().to_owned() })
    }
}

// The bytes of each file, from a pool of 'git cat-file --batch' processes. A read takes a process,
// starts one when every one is taken and the pool is below its size, and waits otherwise.
struct BlobReader {
    repository: String,
    commit: String,
    ids: Vec<String>,
    conversions: Vec<Conversion>,
    paths_git_writes_out: HashMap<usize, String>,
    pool: Mutex<Pool>,
    returned: Condvar,
    most: usize
}

impl BlobReader {
    // A process that failed a read is dropped, since what it would answer next is unknown
    fn read(&self, at: usize, buf: &mut Vec<u8>) -> std::io::Result<()> {
        if let Some(path) = self.paths_git_writes_out.get(&at) {
            return read_as_git_writes_it(&self.repository, &self.commit, path, &self.ids[at], buf);
        }
        let mut process = self.take()?;
        match process.read_blob(&self.ids[at], buf) {
            Ok(()) => {
                self.pool.lock().unwrap().idle.push(process);
                self.returned.notify_one();
                convert_line_endings(self.conversions.get(at).copied().unwrap_or(Conversion::Untouched), buf);
                Ok(())
            },
            Err(error) => {
                drop(process);
                self.forget_one();
                Err(error)
            }
        }
    }

    fn take(&self) -> std::io::Result<CatFile> {
        let mut pool = self.pool.lock().unwrap();
        loop {
            if let Some(process) = pool.idle.pop() {
                return Ok(process);
            }
            if pool.started < self.most {
                pool.started += 1;
                drop(pool);
                return CatFile::start(&self.repository).inspect_err(|_| self.forget_one());
            }
            pool = self.returned.wait(pool).unwrap();
        }
    }

    fn forget_one(&self) {
        self.pool.lock().unwrap().started -= 1;
        self.returned.notify_one();
    }
}

#[derive(Default)]
struct Pool {
    idle: Vec<CatFile>,
    started: usize
}

struct CatFile {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>
}

impl CatFile {
    fn start(repository: &str) -> std::io::Result<CatFile> {
        let mut child = build_git_read(repository).args(["cat-file", "--batch"])
                .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
        let (Some(input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            return Err(std::io::Error::other("git cat-file was started with no pipes"));
        };
        Ok(CatFile { child, input, output: BufReader::new(output) })
    }

    // The reply is '<id> blob <size>', the bytes and a line break, or '<id> missing'
    fn read_blob(&mut self, id: &str, buf: &mut Vec<u8>) -> std::io::Result<()> {
        writeln!(self.input, "{id}")?;
        self.input.flush()?;
        let mut header = String::new();
        if self.output.read_line(&mut header)? == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "git cat-file ended before it answered"));
        }
        let size = match header.split_whitespace().collect::<Vec<_>>()[..] {
            [_, "blob", size] => size.parse::<u64>().map_err(std::io::Error::other)?,
            _ => return Err(std::io::Error::new(std::io::ErrorKind::NotFound, format!("git holds no blob {id}")))
        };
        let read = (&mut self.output).take(size).read_to_end(buf)?;
        let mut line_break = [0u8];
        if read as u64 != size || self.output.read_exact(&mut line_break).is_err() {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "git cat-file ended in the middle of a file"));
        }
        Ok(())
    }
}

impl Drop for CatFile {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use mezura_core::{EngineConfig, RunResult, Stats, TestCode};

    use crate::config_manager::{ArgParsingError, Configuration};
    use crate::git::{GitError, remove_directory_tree, resolve_revision};
    use crate::git::GIT_TEST_LOCK;
    use crate::paths::normalise_separators;
    use crate::paths::test_paths::CORE_LANGUAGES_DIR;
    use crate::test_support::{checkout, run_git};

    #[test]
    fn ignore_files_inside_and_a_test_tools_file_above_the_repository_count_as_in_a_worktree() {
        let _guard = GIT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let holder = create_fresh_folder("mezura-mounted-ignore-files-and-above");
        let repository = holder.join("repo");
        write(&holder, &[("jest.config.js", "module.exports = {};\n")]);
        write(&repository, &[(".gitignore", "*.gen.rs\n"), ("src/a.rs", "fn a() {}\n"), ("src/b.gen.rs", "fn b() {}\n"),
                ("src/sub/.gitignore", "!keep.gen.rs\nlocal.rs\n"), ("src/sub/keep.gen.rs", "fn k() {}\nfn l() {}\n"),
                ("src/sub/local.rs", "fn m() {}\nfn n() {}\nfn o() {}\n"), ("ui/a.test.ts", "test('a', () => {});\n"),
                ("ui/a.ts", "export const a = 1;\n")]);
        create_repository(&repository);
        commit_everything(&repository, "c1");

        assert_mounted_counts_as_a_worktree(&repository, "HEAD", &["", "src", "src/sub", "ui"]);
        remove_directory_tree(&holder).unwrap();
    }

    #[test]
    fn a_file_deleted_since_and_a_file_and_a_folder_that_swapped_count_as_in_a_worktree() {
        let _guard = GIT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let repository = create_fresh_folder("mezura-mounted-deleted-and-swapped");
        write(&repository, &[("gone.rs", "fn g() {}\n"), ("x.rs", "fn x() {}\n"), ("y.rs/inner.rs", "fn i() {}\nfn j() {}\n"),
                ("kept.rs", "fn k() {}\n")]);
        create_repository(&repository);
        commit_everything(&repository, "c1");
        std::fs::remove_file(repository.join("gone.rs")).unwrap();
        std::fs::remove_file(repository.join("x.rs")).unwrap();
        remove_directory_tree(&repository.join("y.rs")).unwrap();
        write(&repository, &[("x.rs/inner.rs", "fn x() {}\n"), ("y.rs", "fn y() {}\n")]);
        commit_everything(&repository, "c2");

        assert_mounted_counts_as_a_worktree(&repository, "HEAD~1", &["", "gone.rs", "x.rs", "y.rs"]);
        remove_directory_tree(&repository).unwrap();
    }

    // A checkout on a file system that folds case holds one of the two names, and the revision holds
    // both. The submodule's name has an extension, so a submodule taken for a file would be counted.
    #[test]
    fn a_submodule_counts_nothing_and_two_names_that_differ_in_case_are_both_counted() {
        let _guard = GIT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let holder = create_fresh_folder("mezura-mounted-submodule-and-case");
        let inner = holder.join("inner");
        write(&inner, &[("lib.rs", "fn i() {}\n")]);
        create_repository(&inner);
        commit_everything(&inner, "i1");
        let repository = holder.join("repo");
        write(&repository, &[("a.rs", "fn a() {}\n")]);
        write(&holder, &[("upper.txt", "fn upper() {}\nfn more() {}\n"), ("lower.txt", "fn lower() {}\n")]);
        create_repository(&repository);
        run_git(&repository, &["add", "a.rs"]);
        run_git(&repository, &["-c", "protocol.file.allow=always", "submodule", "add", "-q",
                &normalise_separators(&inner.to_string_lossy()), "vendor.js"]);
        for (file, name) in [("upper.txt", "CASE.rs"), ("lower.txt", "case.rs")] {
            let id = run_git(&repository, &["hash-object", "-w", &normalise_separators(&holder.join(file).to_string_lossy())]);
            run_git(&repository, &["update-index", "--add", "--cacheinfo", &format!("100644,{id},{name}")]);
        }
        run_git(&repository, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "--no-verify", "-q", "-m", "c1"]);
        // A target naming an initialized submodule counts the submodule's repository
        run_git(&repository, &["submodule", "deinit", "-q", "-f", "vendor.js"]);

        assert_mounted_counts_as_a_worktree(&repository, "HEAD", &["vendor.js"]);
        let root = normalise_separators(&repository.to_string_lossy()).into_owned();
        let (mounted, _) = count_mounted("HEAD", &root);
        let worktree = count_in_a_worktree(&root, "HEAD", "").unwrap();
        let rust_files = |result: &RunResult| result.per_language["Rust"].files;
        assert_eq!(rust_files(&worktree) + usize::from(cfg!(any(windows, target_os = "macos"))), rust_files(&mounted));
        if !cfg!(any(windows, target_os = "macos")) {
            assert_eq!(describe(&worktree), describe(&mounted));
        }

        let alone = crate::config_manager::create_config_from_args(&[format!("{root}/vendor.js@HEAD")]);
        assert!(matches!(&alone, Err(ArgParsingError::GitTarget(message)) if message.contains("is a submodule")), "{alone:?}");
        let beside = crate::config_manager::create_config_from_args(&[format!("{root}/vendor.js@HEAD"), format!("{root}/a.rs@HEAD")])
                .unwrap();
        assert_eq!(1, beside.engine.targets.len());
        assert!(crate::warning_collector::get_collected_warnings().iter()
                .any(|warning| warning.subject.ends_with("vendor.js") && warning.message.contains("is a submodule")));
        drop(beside);
        remove_directory_tree(&holder).unwrap();
    }

    #[test]
    fn a_link_follows_core_symlinks_as_a_worktree_written_with_that_setting_does() {
        let _guard = GIT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let holder = create_fresh_folder("mezura-mounted-links");
        let repository = holder.join("repo");
        write(&repository, &[("src/a.rs", "fn a() {}\nfn b() {}\n")]);
        write(&holder, &[("pointed.txt", "src/a.rs")]);
        create_repository(&repository);
        run_git(&repository, &["add", "src/a.rs"]);
        let id = run_git(&repository, &["hash-object", "-w", &normalise_separators(&holder.join("pointed.txt").to_string_lossy())]);
        run_git(&repository, &["update-index", "--add", "--cacheinfo", &format!("120000,{id},link.rs")]);
        run_git(&repository, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "--no-verify", "-q", "-m", "c1"]);

        run_git(&repository, &["config", "core.symlinks", "false"]);
        assert_mounted_counts_as_a_worktree(&repository, "HEAD", &["", "link.rs"]);
        #[cfg(unix)]
        {
            run_git(&repository, &["config", "core.symlinks", "true"]);
            assert_mounted_counts_as_a_worktree(&repository, "HEAD", &[""]);
        }
        remove_directory_tree(&holder).unwrap();
    }

    #[test]
    fn a_partial_clone_fetches_what_the_revision_lacks_before_the_first_read_and_counts_as_a_worktree() {
        let _guard = GIT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let holder = create_fresh_folder("mezura-mounted-partial-clone");
        let origin = create_origin(&holder);
        let _ssh = crate::test_support::install_local_ssh_helper();
        for filter in ["blob:none", "tree:0"] {
            let clone = clone_partially(&origin, &holder, filter);
            let resolved = resolve_revision(&normalise_separators(&clone.to_string_lossy()), "HEAD~1").unwrap();
            let count_missing = || run_git(&clone, &["rev-list", "--objects", "--no-walk", "--missing=print", &resolved.commit])
                    .lines().filter(|line| line.starts_with('?')).count();
            assert_ne!(0, count_missing(), "the clone made with {filter} lacked nothing of the revision");
            drop(super::mount_revision(&resolved).unwrap());
            assert_eq!(0, count_missing(), "what the revision lacked with {filter} was not fetched before the first read");

            assert_mounted_counts_as_a_worktree(&clone, "HEAD~1", &[""]);
        }
        remove_directory_tree(&holder).unwrap();
    }

    #[test]
    fn the_fetch_of_what_a_revision_lacks_refuses_a_transport_that_runs_a_program() {
        let _guard = GIT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let holder = create_fresh_folder("mezura-mounted-hostile-transport");
        let origin = create_origin(&holder);
        let _ssh = crate::test_support::install_local_ssh_helper();
        let clone = clone_partially(&origin, &holder, "blob:none");
        let ran = normalise_separators(&holder.join("ran").to_string_lossy()).into_owned();
        let resolved = resolve_revision(&normalise_separators(&clone.to_string_lossy()), "HEAD~1").unwrap();
        let through_a_shell = format!("ext::sh -c echo>'{}'", ran.replace('%', "%%").replace(' ', "% "));

        for (transport, url) in [("ext", through_a_shell.as_str()), ("mzr", "mzr::anything"), ("mzr", "mzr://anything")] {
            run_git(&clone, &["config", "remote.origin.url", url]);
            run_git(&clone, &["config", &format!("protocol.{transport}.allow"), "always"]);
            assert!(matches!(super::mount_revision(&resolved), Err(GitError::Refused { message, .. }) if message.contains("not allowed")),
                    "git was left to start the program of the transport of '{url}'");
        }
        assert!(!Path::new(&ran).exists(), "the program the repository names as its transport was run");
        remove_directory_tree(&holder).unwrap();
    }

    #[test]
    fn the_fetch_is_refused_while_the_repositorys_own_configuration_names_a_program_for_it() {
        let _guard = GIT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let holder = create_fresh_folder("mezura-mounted-program-settings");
        let origin = create_origin(&holder);
        let _ssh = crate::test_support::install_local_ssh_helper();
        let clone = clone_partially(&origin, &holder, "blob:none");
        let resolved = resolve_revision(&normalise_separators(&clone.to_string_lossy()), "HEAD~1").unwrap();
        let ran = normalise_separators(&holder.join("ran").to_string_lossy()).into_owned();
        let ssh_command = format!("echo ran > '{ran}' #");
        let ssh_command_on_a_second_line = format!("\n{ssh_command}");

        for (setting, program) in [("credential.helper", "store"), ("credential.https://example.com.helper", "store"),
                ("core.gitProxy", "proxy-command"), ("core.sshCommand", ssh_command.as_str()),
                ("core.sshCommand", ssh_command_on_a_second_line.as_str()), ("remote.origin.vcs", "folder/helper")] {
            run_git(&clone, &["config", setting, program]);
            assert!(matches!(super::mount_revision(&resolved), Err(GitError::FetchRunsRepositoryProgram { setting: named, .. })
                    if named.eq_ignore_ascii_case(setting)), "'{setting}' in the repository's own configuration did not stop the fetch");
            run_git(&clone, &["config", "--unset", setting]);
        }
        assert!(!Path::new(&ran).exists(), "the ssh command the repository names was run");

        run_git(&clone, &["config", "credential.helper", ""]);
        drop(super::mount_revision(&resolved).unwrap());
        remove_directory_tree(&holder).unwrap();
    }

    #[test]
    fn the_fetch_of_what_a_revision_lacks_starts_no_maintenance_of_the_repository() {
        let _guard = GIT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let holder = create_fresh_folder("mezura-mounted-maintenance");
        let origin = create_origin(&holder);
        let _ssh = crate::test_support::install_local_ssh_helper();
        let clone = clone_partially(&origin, &holder, "blob:none");
        let resolved = resolve_revision(&normalise_separators(&clone.to_string_lossy()), "HEAD~1").unwrap();
        let ran = normalise_separators(&holder.join("ran").to_string_lossy()).into_owned();
        let hook = format!("echo ran > '{ran}'");
        for (setting, value) in [("maintenance.strategy", "gc"), ("gc.auto", "1"), ("gc.autoPackLimit", "1"),
                ("gc.autoDetach", "false"), ("maintenance.autoDetach", "false"), ("gc.recentObjectsHook", hook.as_str())] {
            run_git(&clone, &["config", setting, value]);
        }
        // Without an old object that nothing reaches, the gc never runs the hook
        write(&holder, &[("unreachable.txt", "reached by nothing\n")]);
        let id = run_git(&clone, &["hash-object", "-w", &normalise_separators(&holder.join("unreachable.txt").to_string_lossy())]);
        let object = clone.join(".git").join("objects").join(&id[..2]).join(&id[2..]);
        let contents = std::fs::read(&object).unwrap();
        std::fs::remove_file(&object).unwrap();
        std::fs::write(&object, contents).unwrap();
        std::fs::File::options().write(true).open(&object).unwrap()
                .set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_500_000_000)).unwrap();

        drop(super::mount_revision(&resolved).unwrap());
        assert!(!Path::new(&ran).exists(), "the fetch started a gc, which ran the hook the repository names");
        remove_directory_tree(&holder).unwrap();
    }

    #[test]
    fn every_file_of_a_revision_reads_byte_for_byte_as_a_worktree_holds_it_under_each_setting_of_line_endings() {
        let _guard = GIT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let holder = create_fresh_folder("mezura-mounted-line-endings");
        let repository = holder.join("repo");
        std::fs::create_dir(&repository).unwrap();
        create_repository(&repository);
        let pad_with_printable = |printable: usize| [vec![b'x'; printable], b"\x01\na\n".to_vec()].concat();
        let contents = [("lf", b"a\nb\n".to_vec()), ("mixed", b"a\r\nb\n".to_vec()), ("lone-cr", b"a\rb\n".to_vec()),
                ("crlf", b"a\r\nb\r\n".to_vec()), ("nul-byte", b"a\0b\nc\n".to_vec()), ("no-last-break", b"a\nb".to_vec()),
                ("break-first", b"\na\n".to_vec()), ("empty", Vec::new()), ("control", b"\x01\x02\nx\n".to_vec()),
                ("dos-end", b"a\nb\n\x1a".to_vec()), ("dos-end-inside", b"a\n\x1ab\n".to_vec()), ("latin1", b"caf\xe9\n".to_vec()),
                ("printable-enough", pad_with_printable(127)), ("too-few-printable", pad_with_printable(126))];
        let rows = [("none", ""), ("text", "text"), ("auto", "text=auto"), ("autocrlf", "text=auto eol=crlf"),
                ("autolf", "text=auto eol=lf"), ("textlf", "text eol=lf"), ("textcrlf", "text eol=crlf"), ("eolcrlf", "eol=crlf"),
                ("eollf", "eol=lf"), ("notext", "-text"), ("binary", "binary"), ("crlf", "crlf"), ("nocrlf", "-crlf"),
                ("crlfinput", "crlf=input"), ("inputcrlf", "crlf=input eol=crlf"), ("lfs", "filter=lfs -text"), ("dos", "dos"),
                ("odd", "text=odd crlf"), ("utf8", "working-tree-encoding=UTF-8 eol=crlf"), ("ident", "ident"),
                ("identcrlf", "ident eol=crlf"), ("utf16", "working-tree-encoding=UTF-16LE-BOM")];
        let attributes = rows.iter().filter(|(_, attributes)| !attributes.is_empty())
                .map(|(row, attributes)| format!("*.{row} {attributes}\n")).collect::<String>();
        let mut files = vec![(".gitattributes".to_owned(), format!("[attr]dos text eol=crlf\n{attributes}").into_bytes()),
                ("sub/.gitattributes".to_owned(), b"*.text -text\n".to_vec()), ("sub/lf.text".to_owned(), b"a\nb\n".to_vec())];
        for (row, _) in &rows[..rows.len() - 3] {
            files.extend(contents.iter().map(|(name, bytes)| (format!("{name}.{row}"), bytes.clone())));
        }
        files.extend(["ident", "identcrlf"].map(|row| (format!("keyword.{row}"), b"x $Id$ y\nz\n".to_vec())));
        files.push(("wide.utf16".to_owned(), b"a\nb\n".to_vec()));
        commit_files_as_given(&repository, &holder, &files);
        let root = normalise_separators(&repository.to_string_lossy()).into_owned();
        let resolved = resolve_revision(&root, "HEAD").unwrap();

        let mut converted = 0;
        for (autocrlf, eol) in [("false", None), ("true", None), ("input", None), ("false", Some("lf")), ("false", Some("crlf")),
                ("true", Some("lf")), ("input", Some("crlf"))] {
            run_git(&repository, &["config", "core.autocrlf", autocrlf]);
            if let Some(eol) = eol {
                run_git(&repository, &["config", "core.eol", eol]);
            }
            let read = read_files_held_against_a_worktree(&resolved, &format!("core.autocrlf={autocrlf} and core.eol={eol:?}"));
            assert_eq!(files.len(), read.len());
            converted += read.iter().filter(|(name, bytes)| files.iter().any(|(path, stored)| path == name && stored != bytes)).count();
        }
        assert_ne!(0, converted, "no setting changed a byte of any file");
        remove_directory_tree(&holder).unwrap();
    }

    #[test]
    fn a_revision_with_no_attributes_and_attributes_outside_the_revision_read_as_a_worktree_holds_them() {
        let _guard = GIT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let holder = create_fresh_folder("mezura-mounted-attributes-outside");
        let repository = holder.join("repo");
        std::fs::create_dir(&repository).unwrap();
        create_repository(&repository);
        commit_files_as_given(&repository, &holder, &[("a.txt".to_owned(), b"a\nb\n".to_vec()), ("b.bin".to_owned(), b"a\nb\n".to_vec())]);
        commit_files_as_given(&repository, &holder, &[(".gitattributes".to_owned(), b"*.bin binary\n".to_vec())]);
        commit_files_as_given(&repository, &holder, &[(".gitattributes".to_owned(), b"\"*.txt\"eol=crlf\n".to_vec())]);
        let root = normalise_separators(&repository.to_string_lossy()).into_owned();
        let [with_none, with_binary, with_quotes] = ["HEAD~2", "HEAD~1", "HEAD"].map(|revision| resolve_revision(&root, revision).unwrap());
        let read = |resolved, settings| read_files_held_against_a_worktree(resolved, settings).into_iter()
                .map(|(name, bytes)| format!("{name} {}", bytes.escape_ascii())).collect::<Vec<_>>();

        assert_eq!(vec![r"a.txt a\nb\n", r"b.bin a\nb\n"], read(&with_none, "no attributes"));
        assert_eq!(vec![r#".gitattributes \"*.txt\"eol=crlf\n"#, r"a.txt a\r\nb\r\n", r"b.bin a\nb\n"],
                read(&with_quotes, "a pattern in quotes with no blank after it"));
        run_git(&repository, &["config", "core.autocrlf", "true"]);
        assert_eq!(vec![r"a.txt a\r\nb\r\n", r"b.bin a\r\nb\r\n"], read(&with_none, "no attributes and core.autocrlf"));
        assert_eq!(vec![r".gitattributes *.bin binary\r\n", r"a.txt a\r\nb\r\n", r"b.bin a\nb\n"], read(&with_binary, "a binary attribute"));
        run_git(&repository, &["config", "core.autocrlf", "false"]);
        std::fs::create_dir_all(repository.join(".git").join("info")).unwrap();
        std::fs::write(repository.join(".git").join("info").join("attributes"), "*.txt eol=crlf\n").unwrap();
        assert_eq!(vec![r"a.txt a\r\nb\r\n", r"b.bin a\nb\n"], read(&with_none, "attributes of the clone"));
        remove_directory_tree(&holder).unwrap();
    }

    #[test]
    fn a_path_under_a_filter_of_the_repository_is_read_without_running_its_program() {
        let _guard = GIT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let holder = create_fresh_folder("mezura-mounted-filter-program");
        let repository = holder.join("repo");
        std::fs::create_dir(&repository).unwrap();
        create_repository(&repository);
        commit_files_as_given(&repository, &holder, &[
                (".gitattributes".to_owned(), b"*.up filter=upper ident eol=crlf\n*.lfs filter=lfs ident\n".to_vec()),
                ("keyword.up".to_owned(), b"x $Id$ y\n".to_vec()), ("keyword.lfs".to_owned(), b"x $Id$ y\n".to_vec())]);
        let ran = normalise_separators(&holder.join("ran").to_string_lossy()).into_owned();
        let program = format!("echo ran > '{ran}' #");
        for setting in ["filter.upper.smudge", "filter.upper.process", "filter.lfs.smudge", "filter.lfs.process"] {
            run_git(&repository, &["config", setting, &program]);
        }
        let resolved = resolve_revision(&normalise_separators(&repository.to_string_lossy()), "HEAD").unwrap();
        let id = run_git(&repository, &["rev-parse", "HEAD:keyword.lfs"]);

        let (listed, blobs, _) = super::list_revision(&resolved, false).unwrap();
        let read = |name: &str| {
            let mut read = Vec::new();
            blobs.read(listed.iter().position(|file| file == Path::new(name)).unwrap(), &mut read).unwrap();
            read.escape_ascii().to_string()
        };
        let (under_a_filter, under_lfs) = (read("keyword.up"), read("keyword.lfs"));
        drop(blobs);
        assert_eq!(r"x $Id$ y\r\n", under_a_filter);
        assert_eq!(format!(r"x $Id: {id} $ y\n"), under_lfs);
        assert!(!Path::new(&ran).exists(), "reading a file under a filter ran the program the repository names for it");
        remove_directory_tree(&holder).unwrap();
    }

    #[test]
    fn a_path_that_is_no_utf8_is_never_handed_to_git_where_its_respelling_is_under_a_filter() {
        let _guard = GIT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let holder = create_fresh_folder("mezura-mounted-path-no-utf8");
        let repository = holder.join("repo");
        std::fs::create_dir(&repository).unwrap();
        create_repository(&repository);
        let store = |contents: &[u8]| {
            std::fs::write(holder.join("blob"), contents).unwrap();
            run_git(&repository, &["hash-object", "-w", "--no-filters", &normalise_separators(&holder.join("blob").to_string_lossy())])
        };
        let (keyword, attributes) = (store(b"x $Id$ y\n"), store(b"a\xff.rs ident\n*\xef\xbf\xbd* filter=evil\n"));
        let entries = [format!("100644 blob {attributes}\t.gitattributes\0100644 blob {keyword}\ta").as_bytes(), b"\xff.rs\0"].concat();
        let mut mktree = Command::new("git").arg("-C").arg(&repository).args(["mktree", "-z"])
                .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
        std::io::Write::write_all(&mut mktree.stdin.take().unwrap(), &entries).unwrap();
        let tree = String::from_utf8(mktree.wait_with_output().unwrap().stdout).unwrap();
        let commit = run_git(&repository, &["-c", "user.name=t", "-c", "user.email=t@t", "commit-tree", "-m", "c1", tree.trim()]);
        run_git(&repository, &["update-ref", "HEAD", &commit]);
        let ran = normalise_separators(&holder.join("ran").to_string_lossy()).into_owned();
        for setting in ["filter.evil.smudge", "filter.evil.process"] {
            run_git(&repository, &["config", setting, &format!("echo ran > '{ran}' #")]);
        }
        let resolved = resolve_revision(&normalise_separators(&repository.to_string_lossy()), "HEAD").unwrap();

        let (listed, blobs, _) = super::list_revision(&resolved, false).unwrap();
        assert_eq!(2, listed.len());
        for at in 0..listed.len() {
            blobs.read(at, &mut Vec::new()).unwrap();
        }
        drop(blobs);
        assert!(!Path::new(&ran).exists(), "the file was handed to git under a name whose attributes name the repository's filter");
        remove_directory_tree(&holder).unwrap();
    }

    #[test]
    fn the_fetch_of_what_a_revision_lacks_reads_no_bundle_the_repository_names() {
        let _guard = GIT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let holder = create_fresh_folder("mezura-mounted-bundle");
        let origin = create_origin(&holder);
        let _ssh = crate::test_support::install_local_ssh_helper();
        let clone = clone_partially(&origin, &holder, "blob:none");
        let bundle = normalise_separators(&holder.join("elsewhere.bundle").to_string_lossy()).into_owned();
        run_git(&origin, &["bundle", "create", &bundle, "--all"]);
        run_git(&clone, &["config", "fetch.bundleURI", &bundle]);
        let resolved = resolve_revision(&normalise_separators(&clone.to_string_lossy()), "HEAD~1").unwrap();

        drop(super::mount_revision(&resolved).unwrap());
        assert_eq!("", run_git(&clone, &["for-each-ref", "refs/bundles"]), "the fetch unpacked the bundle the repository names");
        remove_directory_tree(&holder).unwrap();
    }

    // A worktree of each revision counted by itself reads the configuration above its repository, and
    // one run holding both revisions reads it for each. The test file at the repository's root lies
    // outside 'testpaths', so it is test code only where 'testpaths' went unread.
    #[test]
    fn one_run_reads_the_folders_above_the_repository_for_each_of_two_revisions_of_it() {
        let _guard = GIT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let holder = create_fresh_folder("mezura-mounted-two-revisions");
        let repository = holder.join("repo");
        write(&holder, &[("pytest.ini", "[pytest]\ntestpaths = repo/tests\n")]);
        write(&repository, &[("tests/test_a.py", "def test_a():\n    pass\n"), ("app.py", "x = 1\n"),
                ("test_c.py", "def test_c():\n    pass\n")]);
        create_repository(&repository);
        commit_everything(&repository, "c1");
        run_git(&repository, &["tag", "v1"]);
        write(&repository, &[("tests/test_b.py", "def test_b():\n    pass\n")]);
        commit_everything(&repository, "c2");
        run_git(&repository, &["tag", "v2"]);
        let root = normalise_separators(&repository.to_string_lossy()).into_owned();

        let config = crate::config_manager::create_config_from_args(&[format!("old={root}@v1"), format!("new={root}@v2")]).unwrap();
        let counted = mezura_core::run(&config.engine, resolve_languages(&config.engine)).unwrap();
        for (module, revision, test_files) in [("old", "v1", 1), ("new", "v2", 2)] {
            let of_module = counted.modules.iter().find(|of_module| of_module.name.as_deref() == Some(module)).unwrap();
            let worktree = count_in_a_worktree(&root, revision, "").unwrap();
            assert_eq!(describe_figures(&worktree.per_language, &worktree.tests),
                    describe_figures(&of_module.per_language, &of_module.tests), "the module '{module}'");
            assert_eq!(test_files, of_module.tests["Python"].stats.files, "the testpaths above the repository were not read");
        }
        drop(config);
        remove_directory_tree(&holder).unwrap();
    }

    // Each place counted through the run of a '--diff' and from 'git worktree add'. The figures are
    // compared with every path left out, since the two sides read their files under two roots.
    fn assert_mounted_counts_as_a_worktree(repository: &Path, revision: &str, places: &[&str]) {
        let root = normalise_separators(&repository.to_string_lossy()).into_owned();
        for place in places {
            let (mounted, notes) = count_mounted(revision, format!("{root}/{place}").trim_end_matches('/'));
            match count_in_a_worktree(&root, revision, place) {
                Some(worktree) => {
                    assert_eq!(describe(&worktree), describe(&mounted), "'{place}' at {revision}");
                    assert!(notes.is_empty(), "'{place}' at {revision}: {notes:?}");
                },
                None => assert!(!notes.is_empty(), "'{place}' is in no worktree of {revision} and was counted")
            }
        }
    }

    fn count_mounted(revision: &str, target: &str) -> (RunResult, Vec<crate::diff::Note>) {
        let config = Configuration::new(vec![target.to_owned()]);
        let prepared = crate::sources::prepare_revisions_for_config(&[revision], &config).unwrap();
        let side = crate::sources::start_acquiring_prepared_revisions(prepared).into_iter().next().unwrap();
        let parsed = mezura_core::language_file::parse_languages_in_dir(CORE_LANGUAGES_DIR).unwrap().0;
        let (reading, notes) = crate::sources::count_git_revision(side, &config, parsed, &Default::default()).unwrap();
        (reading.result, notes)
    }

    // Counted from the folder 'git worktree add' wrote, with the files beside the repository copied beside it
    fn count_in_a_worktree(root: &str, revision: &str, place: &str) -> Option<RunResult> {
        let written = checkout(&resolve_revision(root, revision).unwrap()).unwrap();
        let (beside_the_repository, beside_the_worktree) = (Path::new(root).parent().unwrap(), Path::new(&written.path).parent().unwrap());
        if beside_the_repository != std::env::temp_dir() {
            for file in std::fs::read_dir(beside_the_repository).unwrap().flatten().filter(|entry| entry.path().is_file()) {
                std::fs::copy(file.path(), beside_the_worktree.join(file.file_name())).unwrap();
            }
        }
        let target = format!("{}/{place}", written.path).trim_end_matches('/').to_owned();
        if !Path::new(&target).exists() {
            return None;
        }
        let engine = EngineConfig::new([target]);
        Some(mezura_core::run(&engine, resolve_languages(&engine)).unwrap())
    }

    fn describe(result: &RunResult) -> String {
        format!("{}\nfaulty files: {}", describe_figures(&result.per_language, &result.tests), result.faulty_files.len())
    }

    fn describe_figures(per_language: &HashMap<String, Stats>, tests: &HashMap<String, TestCode>) -> String {
        let languages = per_language.iter()
                .map(|(name, stats)| (name, (stats.files, stats.lines, stats.bytes, format!("{:?}", stats.classes))))
                .collect::<BTreeMap<_, _>>();
        let tests = tests.iter().map(|(name, code)| (name, (code.stats.files, code.stats.lines, code.whole_files)))
                .collect::<BTreeMap<_, _>>();
        format!("{languages:?}\n{tests:?}")
    }

    fn resolve_languages(engine: &EngineConfig) -> mezura_core::Languages {
        let parsed = mezura_core::language_file::parse_languages_in_dir(CORE_LANGUAGES_DIR).unwrap().0;
        mezura_core::Languages::resolve(engine, parsed, &Default::default()).0
    }

    fn create_fresh_folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(name);
        let _ = remove_directory_tree(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn write(folder: &Path, files: &[(&str, &str)]) {
        for (file, contents) in files {
            let path = folder.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, contents).unwrap();
        }
    }

    // With 'core.autocrlf' off, so a checkout writes every file as git holds it
    fn create_repository(folder: &Path) {
        run_git(folder, &["init", "-q"]);
        run_git(folder, &["config", "core.autocrlf", "false"]);
    }

    fn commit_everything(folder: &Path, message: &str) {
        run_git(folder, &["add", "-f", "-A", "."]);
        run_git(folder, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "--no-verify", "-q", "-m", message]);
    }

    fn read_files_held_against_a_worktree(resolved: &crate::git::ResolvedRevision, settings: &str) -> Vec<(String, Vec<u8>)> {
        let written = checkout(resolved).unwrap();
        let (listed, blobs, _) = super::list_revision(resolved, false).unwrap();
        listed.iter().enumerate().map(|(at, file)| {
            let mut read = Vec::new();
            blobs.read(at, &mut read).unwrap();
            let in_the_worktree = std::fs::read(Path::new(&written.path).join(file)).unwrap();
            assert!(in_the_worktree == read, "{} with {settings} read as '{}' where a worktree holds '{}'", file.display(),
                    read.escape_ascii(), in_the_worktree.escape_ascii());
            (normalise_separators(&file.to_string_lossy()).into_owned(), read)
        }).collect()
    }

    fn commit_files_as_given(repository: &Path, holder: &Path, files: &[(String, Vec<u8>)]) {
        let mut ids : HashMap<&[u8], String> = HashMap::new();
        let mut entries = Vec::new();
        for (path, contents) in files {
            let id = ids.entry(contents.as_slice()).or_insert_with(|| {
                std::fs::write(holder.join("blob"), contents).unwrap();
                run_git(repository, &["hash-object", "-w", "--no-filters", &normalise_separators(&holder.join("blob").to_string_lossy())])
            });
            entries.push(format!("100644,{id},{path}"));
        }
        for some in entries.chunks(80) {
            let arguments = ["update-index", "--add"].into_iter().chain(some.iter().flat_map(|entry| ["--cacheinfo", entry.as_str()]))
                    .collect::<Vec<_>>();
            run_git(repository, &arguments);
        }
        run_git(repository, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "--no-verify", "-q", "-m", "c1"]);
    }

    fn create_origin(holder: &Path) -> PathBuf {
        let origin = holder.join("origin");
        write(&origin, &[("src/a.rs", "fn a() {}\n"), ("src/b.rs", "fn b() {}\nfn c() {}\n")]);
        create_repository(&origin);
        run_git(&origin, &["config", "uploadpack.allowFilter", "true"]);
        run_git(&origin, &["config", "uploadpack.allowAnySHA1InWant", "true"]);
        commit_everything(&origin, "c1");
        write(&origin, &[("src/a.rs", "fn a() {}\nfn d() {}\nfn e() {}\n")]);
        commit_everything(&origin, "c2");
        origin
    }

    // Over the transport the tests of a remote use, since the settings every command of a revision
    // runs under refuse a file transport
    fn clone_partially(origin: &Path, holder: &Path, filter: &str) -> PathBuf {
        let helper = crate::git::TEST_GIT_SSH.lock().unwrap().clone().unwrap();
        let url = format!("ssh://local/{}", normalise_separators(&origin.to_string_lossy()));
        let name = filter.replace(':', "-");
        let cloned = Command::new("git").env("GIT_SSH", helper).env_remove("GIT_SSH_COMMAND")
                .env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1").arg("-C").arg(holder)
                .args(["clone", "-q", &format!("--filter={filter}"), &url, &name]).output().unwrap();
        assert!(cloned.status.success(), "{}", String::from_utf8_lossy(&cloned.stderr));
        let clone = holder.join(name);
        run_git(&clone, &["config", "core.autocrlf", "false"]);
        clone
    }
}
