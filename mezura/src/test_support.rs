use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use mezura_core::{FilesPresent, LineClasses, ModuleResult, Performance, RunResult, Stats, Target, Threads};

use crate::git::{GitError, ResolvedRevision};

pub struct LocalSshGuard(PathBuf);

impl Drop for LocalSshGuard {
    fn drop(&mut self) {
        *crate::git::TEST_GIT_SSH.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let _ = std::fs::remove_file(&self.0);
    }
}

// A commit written out with 'git worktree add', which the tests hold a mounted revision against
#[derive(Debug)]
pub struct Checkout {
    pub path: String,
    pub resolved: ResolvedRevision
}

impl Drop for Checkout {
    fn drop(&mut self) {
        crate::git::remove_worktree(&self.resolved.repository, &self.path);
    }
}

// The lines are the classes added up and are never passed in: a hand written pair that disagreed
// would print a third column holding a number no class accounts for.
pub fn stats_of(files: usize, bytes: usize, classes: LineClasses,
    keyword_occurrences: HashMap<String, usize>) -> Stats
{
    let lines = classes.calculate_lines();

    Stats::new(files, bytes, lines, classes, keyword_occurrences)
}

pub fn plain_stats_of(files: usize, bytes: usize, lines: usize, code: usize, comments: usize,
    keyword_occurrences: HashMap<String, usize>) -> Stats
{
    assert!(code + comments <= lines, "{code} code and {comments} comments do not fit in {lines} lines");
    let classes = LineClasses { words_in_code: code, words_in_comment: comments,
            blank: lines - code - comments, ..Default::default() };

    stats_of(files, bytes, classes, keyword_occurrences)
}

// A golden is written with '\n' and read back through git, which may have given it '\r\n', so the
// two are compared with the endings normalised
pub fn assert_matches_golden(file_name: &str, rendered: &str, regenerate_with: &str) {
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(file_name);
    if std::env::var_os("MEZURA_UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, rendered.as_bytes()).unwrap();
        return;
    }

    let expected = std::fs::read_to_string(&golden).expect("run with MEZURA_UPDATE_GOLDEN=1 to create it");
    assert_eq!(expected.replace("\r\n", "\n"), rendered,
            "'{file_name}' changed. Read the diff, and if every difference is intended, \
             regenerate with MEZURA_UPDATE_GOLDEN=1 cargo test -p mezura {regenerate_with}");
}

// Only what a test varies is passed in; everything else is the emptiest value a run can produce.
pub fn plain_result_of(per_language: HashMap<String, Stats>, modules: Vec<ModuleResult>,
    targets: Vec<Target>) -> RunResult
{
    RunResult {
        total: Stats::calculate_total_of(&per_language), per_language, modules, targets,
        nested_languages: HashMap::new(), tests: HashMap::new(), faulty_files: Vec::new(),
        skipped_files: mezura_core::SkippedFiles::default(), unreadable_dirs: Vec::new(), warnings: Vec::new(),
        applied_presets: Vec::new(), files_present: FilesPresent { total_files: 2, relevant_files: 2, excluded_files: 0 },
        performance: Performance { duration_millis: 0, threads: Threads::new(1, 1) }
    }
}

// A command line typed as one string, split the way a shell splits it when no argument holds a space
pub fn split_typed_line(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_owned).collect()
}

pub fn install_local_ssh_helper() -> LocalSshGuard {
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
    *crate::git::TEST_GIT_SSH.lock().unwrap_or_else(|e| e.into_inner()) = Some(path.to_string_lossy().into_owned());
    LocalSshGuard(path)
}

pub fn checkout(resolved: &ResolvedRevision) -> Result<Checkout, GitError> {
    let path = crate::git::find_place_for_revision(resolved);
    let holder = Path::new(&path).parent().unwrap();
    let _ = crate::git::remove_directory_tree(holder);
    std::fs::create_dir_all(holder).map_err(|error| GitError::Refused { doing: "making room for the revision",
            message: error.to_string() })?;
    let outcome = crate::git::build_hardened_git(&resolved.repository)
            .args(["worktree", "add", "--detach", "--force", "--quiet", &path, &resolved.commit])
            .output().map_err(GitError::NotInstalled)?;
    if !outcome.status.success() {
        let _ = crate::git::remove_directory_tree(holder);
        return Err(GitError::Refused { doing: "writing out the revision",
                message: String::from_utf8_lossy(&outcome.stderr).trim().to_owned() });
    }

    Ok(Checkout { path, resolved: resolved.clone() })
}

pub fn run_git(at: &Path, arguments: &[&str]) -> String {
    let outcome = Command::new("git").arg("-C").arg(at).args(arguments).output().unwrap();
    assert!(outcome.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&outcome.stderr));
    String::from_utf8_lossy(&outcome.stdout).trim().to_owned()
}
