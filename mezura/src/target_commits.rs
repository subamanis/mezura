// The commit each target of a run sits at, and the counted files that differ from it, so that an
// entry of the log can be counted again at those commits.
use std::collections::HashMap;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use mezura_core::{Languages, RunResult};

use super::config_manager::RevisionTarget;
use super::paths::{fold_ascii_case, fold_for_comparison, is_inside, normalise_separators};

const FILTER_SETTINGS : &str = r"^filter\..*\.(clean|process)$";

pub struct TargetCommit {
    // Only for a revision or a remote target, whose path in the log names no folder on disk
    pub repository: Option<String>,
    pub commit: String,
    pub path: String,
    // Spelled from the target's folder
    pub differing_files: Vec<String>
}

pub enum CommitOfTarget {
    Found(TargetCommit),
    // git's first line of error, for a target with a '.git' at or above it
    Unreadable(String),
    Absent
}

// In the order of the run's targets
pub fn find_commits_of_targets(result: &RunResult, revision_targets: &[RevisionTarget], languages: &Languages)
-> Vec<CommitOfTarget>
{
    let paths = result.targets.iter()
            .map(|target| normalise_separators(&target.path).trim_end_matches('/').to_owned())
            .collect::<Vec<_>>();
    let mut commits = paths.iter().map(|_| CommitOfTarget::Absent).collect::<Vec<_>>();
    let mut repositories : Vec<(String, Vec<PlaceInRepository>)> = Vec::new();
    for (at, path) in paths.iter().enumerate() {
        if let Some(found) = find_commit_of_revision_target(path, revision_targets) {
            commits[at] = CommitOfTarget::Found(found);
            continue;
        }
        let Some(root) = find_repository_root_above(path) else { continue };
        let place = PlaceInRepository { at, module: result.targets[at].module.as_deref(),
                inside: path.get(root.len()..).unwrap_or_default().trim_start_matches('/').to_owned() };
        match repositories.iter_mut().find(|(known, _)| fold_for_comparison(known) == fold_for_comparison(&root)) {
            Some((_, places)) => places.push(place),
            None => repositories.push((root, vec![place]))
        }
    }

    let counted = assign_counted_files_to_targets(result, &paths);
    for (root, places) in &repositories {
        let shared_folder = find_shared_folder(places.iter().map(|place| place.inside.as_str()));
        match read_repository(root, &shared_folder) {
            Err(reason) => for place in places {
                commits[place.at] = CommitOfTarget::Unreadable(reason.clone());
            },
            Ok(None) => (),
            Ok(Some(state)) => for place in places {
                let differing_files = find_differing_files(place, root, &counted[place.at], &state, languages);
                commits[place.at] = CommitOfTarget::Found(TargetCommit { repository: None, commit: state.commit.clone(),
                        path: place.inside.clone(), differing_files });
            }
        }
    }

    commits
}

struct PlaceInRepository<'a> {
    at: usize,
    module: Option<&'a str>,
    // From the repository's root, "" for the root itself
    inside: String
}

// Keyed by the path from the repository's root, folded where the file system folds names
struct RepositoryState {
    commit: String,
    // The letter 'ls-files -v' tags an entry with
    tracked: HashMap<String, (String, char)>,
    // Whether the entry is a deletion
    changed: HashMap<String, (String, bool)>
}

fn find_commit_of_revision_target(path: &str, revision_targets: &[RevisionTarget]) -> Option<TargetCommit> {
    revision_targets.iter().find_map(|revision| {
        let inside = super::sources::replace_root_prefix(path, &revision.display_root, "")
                .or_else(|| super::sources::replace_root_prefix(path, &revision.revision_root, ""))?;
        Some(TargetCommit { repository: Some(revision.base_root.clone()), commit: revision.commit.clone(),
                path: inside.trim_start_matches('/').to_owned(), differing_files: Vec::new() })
    })
}

// A '.git' file marks a worktree or a submodule as a '.git' folder marks a repository. A drive's
// root keeps its slash, since git given a bare drive letter works in the current folder of that drive.
fn find_repository_root_above(path: &str) -> Option<String> {
    let target = Path::new(path);
    let mut folder = if target.is_dir() { target } else { target.parent()? };
    loop {
        if folder.join(".git").exists() {
            let root = normalise_separators(&folder.to_string_lossy()).into_owned();
            return Some(match root.trim_end_matches('/') {
                trimmed if trimmed.is_empty() || trimmed.ends_with(':') => root,
                trimmed => trimmed.to_owned()
            });
        }
        folder = folder.parent()?;
    }
}

// Each file goes to the innermost target holding it, so a nested repository named as a separate
// target keeps its files
fn assign_counted_files_to_targets(result: &RunResult, paths: &[String]) -> Vec<Vec<String>> {
    let folded_targets = paths.iter().map(|path| fold_ascii_case(path).into_owned()).collect::<Vec<_>>();
    let mut counted = vec![Vec::new(); paths.len()];
    for file in result.modules.iter().flat_map(|module| module.files.values().flatten()) {
        let path = normalise_separators(&file.path).into_owned();
        let folded = fold_ascii_case(&path);
        let holder = folded_targets.iter().enumerate()
                .filter(|(_, target)| is_inside(&folded, target))
                .max_by_key(|(_, target)| target.len());
        if let Some((at, _)) = holder {
            counted[at].push(path);
        }
    }

    counted
}

fn find_shared_folder<'a>(places: impl Iterator<Item = &'a str>) -> String {
    let mut shared : Option<Vec<&str>> = None;
    for place in places {
        let segments = place.split('/').filter(|segment| !segment.is_empty()).collect::<Vec<_>>();
        shared = Some(match shared {
            None => segments,
            Some(so_far) => so_far.into_iter().zip(segments).take_while(|(one, other)| one == other)
                    .map(|(one, _)| one).collect()
        });
    }

    shared.unwrap_or_default().join("/")
}

// 'None' for a repository whose HEAD holds no commit yet. 'git status' runs a clean filter on a
// file whose stat changed, so the filters of the repository's own configuration are emptied.
fn read_repository(root: &str, shared_folder: &str) -> Result<Option<RepositoryState>, String> {
    let limit_to_shared_folder = |command: &mut Command| {
        if !shared_folder.is_empty() {
            command.args(["--", shared_folder]);
        }
    };
    let head = start(super::git::build_git_read(root).args(["rev-parse", "--verify", "-q", "HEAD"]));
    let mut listing = super::git::build_git_read(root);
    listing.args(["--literal-pathspecs", "ls-files", "-z", "-v"]);
    limit_to_shared_folder(&mut listing);
    let listed = start(&mut listing);

    let mut status = super::git::build_git_read(root);
    status.args(["--no-optional-locks", "--literal-pathspecs"]);
    let filters = match could_name_a_filter(root) {
        true => super::git::read_scoped_git_settings(root, FILTER_SETTINGS),
        false => Vec::new()
    };
    for (scope, setting, _) in filters {
        let driver = setting.strip_prefix("filter.").and_then(|rest| rest.rsplit_once('.')).map(|(driver, _)| driver);
        if let Some(driver) = driver.filter(|_| matches!(scope.as_str(), "local" | "worktree")) {
            for emptied in ["clean=", "process=", "required=false"] {
                status.arg("-c").arg(format!("filter.{driver}.{emptied}"));
            }
        }
    }
    status.args(["status", "--porcelain", "-z", "-uno", "--no-renames", "--ignore-submodules=all"]);
    limit_to_shared_folder(&mut status);
    let changes = start(&mut status);

    let (head, listed, changes) = (finish(head), finish(listed), finish(changes));
    let commit = match head? {
        Answer::Done(commit) => commit.trim().to_owned(),
        Answer::Refused { code: Some(1), .. } => return Ok(None),
        Answer::Refused { message, .. } => return Err(message)
    };
    let read = |answer: Result<Answer, String>| match answer? {
        Answer::Done(output) => Ok(output),
        Answer::Refused { message, .. } => Err(message)
    };
    let fold = |path: &str| fold_for_comparison(path).into_owned();
    let tracked = read(listed)?.split('\0').filter(|entry| entry.len() > 2)
            .map(|entry| (fold(&entry[2..]), (entry[2..].to_owned(), entry.chars().next().unwrap_or('H'))))
            .collect();
    let changed = read(changes)?.split('\0').filter(|entry| entry.len() > 3)
            .map(|entry| (fold(&entry[3..]), (entry[3..].to_owned(), entry[..2].contains('D'))))
            .collect();

    Ok(Some(RepositoryState { commit, tracked, changed }))
}

// A worktree or a submodule keeps its configuration outside its '.git', so git is asked there
fn could_name_a_filter(root: &str) -> bool {
    let folder = Path::new(root).join(".git");
    if !folder.is_dir() {
        return true;
    }
    ["config", "config.worktree"].iter().any(|name| std::fs::read_to_string(folder.join(name))
            .is_ok_and(|text| ["filter", "include"].iter().any(|word| text.to_ascii_lowercase().contains(word))))
}

enum Answer {
    Done(String),
    Refused { code: Option<i32>, message: String }
}

fn start(command: &mut Command) -> Result<Child, String> {
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()
            .map_err(|error| format!("git could not be started: {error}"))
}

fn finish(git: Result<Child, String>) -> Result<Answer, String> {
    let outcome = git?.wait_with_output().map_err(|error| format!("git could not be read: {error}"))?;
    if outcome.status.success() {
        return Ok(Answer::Done(String::from_utf8_lossy(&outcome.stdout).into_owned()));
    }
    let message = String::from_utf8_lossy(&outcome.stderr).lines().next().unwrap_or_default().trim().to_owned();

    Ok(Answer::Refused { code: outcome.status.code(),
            message: if message.is_empty() { format!("git exited with {}", outcome.status) } else { message } })
}

// 'ls-files -v' tags a skip-worktree file 'S' and an assume-unchanged one in lowercase, and
// 'git status' reports no edit to either
fn find_differing_files(place: &PlaceInRepository, root: &str, counted: &[String], state: &RepositoryState,
        languages: &Languages) -> Vec<String>
{
    let looks_away = |tag: char| tag == 'S' || tag.is_ascii_lowercase();
    let mut differing = Vec::new();
    for path in counted {
        let inside_root = path.get(root.len()..).unwrap_or(path).trim_start_matches('/');
        let key = fold_for_comparison(inside_root);
        let differs = match state.tracked.get(key.as_ref()) {
            None => true,
            Some((_, tag)) => looks_away(*tag) || state.changed.contains_key(key.as_ref())
        };
        if differs {
            differing.push(inside_root.to_owned());
        }
    }

    let folded_place = fold_for_comparison(&place.inside).into_owned();
    let is_under_place = |key: &str| folded_place.is_empty() || is_inside(key, &folded_place);
    let reads_name = |path: &str| languages.reads_files_named(place.module,
            path.rsplit('/').next().unwrap_or(path));
    let missing = state.changed.iter().filter(|(_, (_, deleted))| *deleted).map(|(key, (path, _))| (key, path))
            .chain(state.tracked.iter().filter(|(_, (_, tag))| tag.eq_ignore_ascii_case(&'S'))
                    .map(|(key, (path, _))| (key, path)));
    for (key, path) in missing {
        if is_under_place(key) && reads_name(path) {
            differing.push(path.clone());
        }
    }

    let mut from_the_target = differing.into_iter().map(|path| {
        if place.inside.is_empty() {
            return path;
        }
        match path.get(place.inside.len()..).filter(|rest| rest.starts_with('/')) {
            Some(rest) => rest[1..].to_owned(),
            None => path.rsplit('/').next().unwrap_or(&path).to_owned()
        }
    }).collect::<Vec<_>>();
    from_the_target.sort();
    from_the_target.dedup();

    from_the_target
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use mezura_core::{EngineConfig, Languages, Target, Threads};

    use crate::git::{GIT_TEST_LOCK, remove_directory_tree};
    use crate::paths::normalise_separators;
    use crate::paths::test_paths::CORE_LANGUAGES_DIR;
    use crate::test_support::run_git;

    use super::{CommitOfTarget, find_commits_of_targets};

    #[test]
    fn a_target_differs_from_its_commit_by_the_files_it_counted_and_by_those_alone() {
        let _guard = GIT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = create_fresh_folder("mezura-target-commits");
        write(&root, &[("api/src/a.rs", "fn a() {}\n"), ("api/src/b.rs", "fn b() {}\n"), ("api/src/c.rs", "fn c() {}\n"),
                ("api/README.md", "# api\n"), ("api/notes.md", "notes\n"), ("web/w.js", "let w = 1;\n")]);
        run_git(&root, &["init", "-q"]);
        run_git(&root, &["config", "core.autocrlf", "false"]);
        run_git(&root, &["add", "-A"]);
        run_git(&root, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "--no-verify", "-q", "-m", "c1"]);
        let commit = run_git(&root, &["rev-parse", "HEAD"]);
        let unborn = create_fresh_folder("mezura-target-commits-unborn");
        write(&unborn, &[("u.rs", "fn u() {}\n")]);
        run_git(&unborn, &["init", "-q"]);
        let targets = [("api", root.join("api")), ("web", root.join("web")), ("unborn", unborn.clone())];

        let clean = vec![Some(("api".to_owned(), Vec::new())), Some(("web".to_owned(), Vec::new())), None];
        assert_eq!(clean, find_differing_files_of(&targets, &commit));

        write(&root, &[("api/README.md", "# api, changed\n")]);
        std::fs::remove_file(root.join("api/notes.md")).unwrap();
        assert_eq!(clean, find_differing_files_of(&targets, &commit), "a file nobody counts made its target differ");

        write(&root, &[("api/src/a.rs", "fn a() {}\nfn a2() {}\n"), ("web/new.rs", "fn n() {}\n"),
                ("api/src/scratch.rs", "fn s() {}\n"), ("web/vendor/v.js", "let v = 2;\n")]);
        run_git(&root, &["add", "api/src/a.rs"]);
        std::fs::remove_file(root.join("api/src/b.rs")).unwrap();
        std::fs::write(root.join(".git/info/exclude"), "api/src/scratch.rs\n").unwrap();
        run_git(&root, &["update-index", "--assume-unchanged", "api/src/c.rs"]);
        write(&root, &[("api/src/c.rs", "fn c() {}\nfn c2() {}\n")]);
        run_git(&root.join("web/vendor"), &["init", "-q"]);
        assert_eq!(vec![Some(("api".to_owned(), owned(&["src/a.rs", "src/b.rs", "src/c.rs", "src/scratch.rs"]))),
                Some(("web".to_owned(), owned(&["new.rs", "vendor/v.js"]))), None],
                find_differing_files_of(&targets, &commit));

        remove_directory_tree(&root).unwrap();
        remove_directory_tree(&unborn).unwrap();
    }

    fn find_differing_files_of(targets: &[(&str, PathBuf)], commit: &str) -> Vec<Option<(String, Vec<String>)>> {
        let targets = targets.iter().map(|(module, path)| Target::named(*module, normalise_separators(&path.to_string_lossy())))
                .collect::<Vec<_>>();
        let engine = EngineConfig { collect_files: true, threads: Threads::new(1, 2), targets, ..EngineConfig::default() };
        let parsed = mezura_core::language_file::parse_languages_in_dir(CORE_LANGUAGES_DIR).unwrap().0;
        let languages = Languages::resolve(&engine, parsed, &Default::default()).0;
        let result = mezura_core::run(&engine, languages.clone()).unwrap();

        find_commits_of_targets(&result, &[], &languages).into_iter().map(|found| match found {
            CommitOfTarget::Found(found) => {
                assert_eq!(commit, found.commit);
                Some((found.path, found.differing_files))
            },
            CommitOfTarget::Absent => None,
            CommitOfTarget::Unreadable(reason) => panic!("git could not read the repository: {reason}")
        }).collect()
    }

    // Through the real spelling, since CI runners name TEMP through 8.3 names or a symlink
    fn create_fresh_folder(name: &str) -> PathBuf {
        let temp = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        let folder = PathBuf::from(temp.to_string_lossy().trim_start_matches(r"\\?\")).join(name);
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

    fn owned(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }
}
