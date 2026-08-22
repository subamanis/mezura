// How a reading is acquired from each kind of source. Nothing here prints: what a reader must be
// told travels back as notes and is said where the comparison is shown.
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::thread::JoinHandle;

use mezura_core::{EngineConfig, FilesPresent, Language, ScanProgress};
use mezura_core::language_file::PriorityRules;

use super::config_manager::Configuration;
use super::diff::{Note, Reading};
use super::git::{Checkout, GitError, ResolvedRevision};

// A file and not merely something that exists: a directory called 'main' beside a branch called
// 'main' would otherwise be read as a document and fail with an I/O error about permissions.
pub fn read_document(name: &str) -> Option<Result<Reading, String>> {
    Path::new(name).is_file()
            .then(|| super::diff::load(name).map_err(|x| x.to_string()))
}

// Every revision of a '--diff' resolves here, before anything is written out, so that a typo in the
// second side fails before the first is checked out and counted whole. Two spellings of one commit
// are refused while both hashes are on hand: they would otherwise share one checkout path and race
// each other's background removal on it. Answered in the order asked.
pub fn prepare_revisions(names: &[&str], engine: &EngineConfig) -> Result<Vec<ResolvedRevision>, GitError> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    // Telling a pattern from a folder whose name carries those characters: what exists exactly as
    // written is always literal
    if let Some(target) = engine.targets.iter().find(|x|
            !Path::new(&x.path).exists() && x.path.contains(['*', '?', '[', '{'])) {
        return Err(GitError::PatternTarget { pattern: target.path.clone() });
    }
    let declared = engine.targets.iter().map(|x| x.path.clone()).collect::<Vec<_>>();
    let repository = super::git::find_common_repository_of(&declared)?;

    let resolved = names.iter().map(|name| super::git::resolve_revision(&repository, name))
            .collect::<Result<Vec<_>, _>>()?;
    if let [first, second] = &resolved[..] && first.commit == second.commit {
        return Err(GitError::SameCommit { first: first.revision.clone(), second: second.revision.clone(),
                commit: first.commit.clone() });
    }

    Ok(resolved)
}

pub struct RevisionSide {
    resolved: ResolvedRevision,
    write: Option<JoinHandle<Result<Checkout, GitError>>>
}

// A side dropped uncounted still joins its write, so a checkout that completed queues its own
// background removal instead of outliving the run as litter.
impl Drop for RevisionSide {
    fn drop(&mut self) {
        if let Some(write) = self.write.take() {
            let _ = write.join();
        }
    }
}

// The leftovers are swept exactly once, before anything writes: the sweep's prune beside a half
// registered parallel write is the one interference between them. With two sides both writes start
// at once while the counting stays serial, so the second hides behind the first side's write and
// count; a single side, or a thread that cannot spawn, writes inline at its turn.
pub fn start_acquiring_revisions(resolved: Vec<ResolvedRevision>) -> Vec<RevisionSide> {
    if let Some(first) = resolved.first() {
        super::git::remove_leftover_checkouts(&first.repository);
    }
    let ahead = resolved.len() >= 2;
    resolved.into_iter().map(|resolved| {
        let write = if ahead {
            let for_thread = resolved.clone();
            std::thread::Builder::new().name("revision-write".to_owned())
                    .spawn(move || super::git::checkout(&for_thread)).ok()
        } else {
            None
        };
        RevisionSide { resolved, write }
    }).collect()
}

pub fn count_git_revision(mut side: RevisionSide, config: &Configuration, languages: Vec<Language>,
        extension_priority: &PriorityRules) -> Result<(Reading, Vec<Note>), GitError>
{
    // Erased when it drops, before this function returns: the whole phase leaves nothing behind on
    // the permanent output
    let progress = Arc::new(ScanProgress::default());
    let _live = super::animated_display::start_revision_display(config, &side.resolved.revision, progress.clone(),
            side.write.as_ref().is_some_and(|x| x.is_finished()));

    let checkout = match side.write.take() {
        Some(write) => match write.join() {
            Ok(outcome) => outcome?,
            // A write whose thread died took its error with it; writing again answers instead
            Err(_) => super::git::checkout(&side.resolved)?
        },
        None => super::git::checkout(&side.resolved)?
    };
    let resolved = &side.resolved;
    let git_revision = resolved.revision.as_str();

    let mut notes = Vec::new();
    // A checkout holds only what git tracks, so the ignored files exist on the other side alone
    if config.engine.no_gitignore {
        notes.push(Note::NoGitignoreInCheckout { git_revision: git_revision.to_owned() });
    }

    // A target the revision never had counts as nothing rather than stopping the run. The declared
    // form of each one that was found is kept beside its checkout path, because a reading that
    // named the checkout would say a different temporary directory on every run.
    let mut targets = Vec::with_capacity(config.engine.targets.len());
    let mut counted_declared = Vec::with_capacity(config.engine.targets.len());
    let mut missing = Vec::new();
    for target in &config.engine.targets {
        let (_, prefix) = super::git::find_repository_of(&target.path)?;
        match checkout.find_target_of(&prefix) {
            Some(path) => {
                targets.push(mezura_core::Target { module: target.module.clone(), path });
                counted_declared.push(target.clone());
            },
            None => missing.push(target.path.clone())
        }
    }
    if !missing.is_empty() {
        notes.push(Note::MissingInRevision { git_revision: git_revision.to_owned(), targets: missing });
    }

    let of_git_revision = EngineConfig { targets,
            exclude_dirs: move_excludes_into_checkout(&checkout.path, &resolved.repository, &config.engine.exclude_dirs),
            ..config.engine.clone() };
    // A reading of zero and not a failure: it is what a revision older than every target holds
    let result = if of_git_revision.targets.is_empty() {
        mezura_core::RunResult {
            per_language: HashMap::new(), total: mezura_core::Stats::default(), modules: Vec::new(),
            nested_languages: HashMap::new(),
            faulty_files: Vec::new(), minified_files: 0, generated_files: 0, unreadable_dirs: Vec::new(), targets: Vec::new(),
            files_present: FilesPresent::default(),
            performance: mezura_core::Performance { duration_millis: 0, threads: config.engine.threads }
        }
    } else {
        let resolved = mezura_core::Languages::resolve(&of_git_revision, languages, extension_priority).0;
        let mut result = mezura_core::run_watched(&of_git_revision, resolved, Some(progress.clone()), |_| {})
                .map_err(|error| GitError::CountingRevision { revision: git_revision.to_owned(), error })?;
        result.targets = counted_declared;
        result
    };

    Ok((Reading::of_git_revision(git_revision, resolved.commit.clone(), resolved.taken_at.clone(),
            result, config), notes))
}

// The checkout is the same tree at another root, so a pattern written as a full path moves with it
// or it would exclude on one side and count on the other.
fn move_excludes_into_checkout(checkout: &str, repository: &str, patterns: &[String]) -> Vec<String> {
    // ASCII folding only, so that folding never moves a byte and the remainder can be cut off the
    // unfolded pattern at the root's own length
    let key = |path: &str| {
        let path = super::paths::normalise_separators(path).into_owned();
        if cfg!(windows) {path.to_ascii_lowercase()} else {path}
    };
    let root = repository.trim_end_matches('/');
    let folded_root = key(root);

    patterns.iter().map(|pattern| {
        let normalized = super::paths::normalise_separators(pattern).into_owned();
        let folded = key(&normalized);
        if folded == folded_root {
            checkout.to_owned()
        } else if folded.starts_with(&(folded_root.clone() + "/")) {
            checkout.to_owned() + &normalized[root.len()..]
        } else {
            pattern.clone()
        }
    }).collect()
}

#[cfg(test)]
mod tests {
    use crate::paths::test_paths::SCRATCH_DIR;

    use super::*;

    #[test]
    fn a_directory_is_never_read_as_a_document() {
        let dir = SCRATCH_DIR.to_owned() + "a-directory-named-like-a-branch";
        std::fs::create_dir_all(&dir).unwrap();
        assert!(read_document(&dir).is_none(), "a directory was taken for a document");
        std::fs::remove_dir_all(&dir).unwrap();

        let file = SCRATCH_DIR.to_owned() + "not-a-document.json";
        std::fs::write(&file, "{ not a document").unwrap();
        assert!(matches!(read_document(&file), Some(Err(_))));
        std::fs::remove_file(&file).unwrap();
        assert!(read_document("no-such-thing-anywhere").is_none());
    }

    #[test]
    fn two_spellings_of_one_commit_are_refused_and_distinct_names_are_resolved() {
        let package = env!("CARGO_MANIFEST_DIR").replace('\\', "/");
        let engine = EngineConfig::new([package.clone()]);
        let (repository, _) = crate::git::find_repository_of(&package).unwrap();
        let head = crate::git::resolve_revision(&repository, "HEAD").unwrap();

        let refused = prepare_revisions(&["HEAD", &head.commit], &engine);
        assert!(matches!(refused, Err(GitError::SameCommit { .. })), "{refused:?}");
        assert!(matches!(prepare_revisions(&["HEAD", "HEAD"], &engine), Err(GitError::SameCommit { .. })));

        let alone = prepare_revisions(&["HEAD"], &engine).unwrap();
        assert_eq!(head.commit, alone[0].commit);
        assert!(prepare_revisions(&[], &engine).unwrap().is_empty());
    }

    #[test]
    fn an_exclusion_inside_the_repository_is_carried_into_the_checkout() {
        let moved = move_excludes_into_checkout("C:/tmp/chk", "D:/repo",
                &["fixtures".to_owned(), "*.min.js".to_owned(), "D:/repo/target".to_owned(),
                  "D:/repo".to_owned(), "D:/elsewhere/target".to_owned(), "D:\\repo\\a\\b".to_owned()]);

        assert_eq!(vec!["fixtures".to_owned(), "*.min.js".to_owned(), "C:/tmp/chk/target".to_owned(),
                "C:/tmp/chk".to_owned(), "D:/elsewhere/target".to_owned(), "C:/tmp/chk/a/b".to_owned()], moved);

        // 'D:/repository' is not inside 'D:/repo', whatever its first characters say
        assert_eq!(vec!["D:/repository/x".to_owned()],
                move_excludes_into_checkout("C:/tmp/chk", "D:/repo", &["D:/repository/x".to_owned()]));

        if cfg!(windows) {
            assert_eq!(vec!["C:/tmp/chk/target".to_owned()],
                    move_excludes_into_checkout("C:/tmp/chk", "D:/repo", &["d:/REPO/target".to_owned()]));
        }
    }
}
