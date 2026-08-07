// How a reading is acquired from each kind of source. Nothing here prints: what a reader must be
// told travels back as notes and is said where the comparison is shown.
use std::collections::HashMap;
use std::path::Path;

use mezura_core::{FilesPresent, Language};

use super::config_manager::Configuration;
use super::diff::{Note, Reading};
use super::git::GitError;

// A file and not merely something that exists: a directory called 'main' beside a branch called
// 'main' would otherwise be read as a document and fail with an I/O error about permissions.
pub fn read_document(name: &str) -> Option<Result<Reading, String>> {
    Path::new(name).is_file()
            .then(|| super::diff::load(name).map_err(|x| x.to_string()))
}

// The files are written out, the targets are found again inside them, and 'run' does the rest,
// under this run's settings and this build
pub fn count_git_revision(git_revision: &str, config: &Configuration, languages: Vec<Language>,
        extension_priority: &HashMap<String,Vec<String>>) -> Result<(Reading, Vec<Note>), GitError>
{
    // The run's own rule for telling a pattern from a folder that carries those characters in its
    // name: what exists exactly as written is always literal
    if let Some(target) = config.engine.dirs.iter().find(|x|
            !Path::new(&x.path).exists() && x.path.contains(['*', '?', '[', '{'])) {
        return Err(GitError::PatternTarget { pattern: target.path.clone() });
    }

    let declared = config.engine.dirs.iter().map(|x| x.path.clone()).collect::<Vec<_>>();
    let repository = super::git::find_common_repository_of(&declared)?;
    let checkout = super::git::checkout(&repository, git_revision)?;

    let mut notes = Vec::new();
    // A checkout holds only what git tracks, so the ignored files exist on the other side alone
    if config.engine.no_gitignore {
        notes.push(Note::NoGitignoreInCheckout { git_revision: git_revision.to_owned() });
    }

    // A target the revision never had counts as nothing rather than stopping the run. The declared
    // form of each one that was found is kept beside its checkout path, because the checkout is a
    // temporary directory: a reading that named it would say a different tree on every run, when
    // what was measured is the declared tree as that commit held it.
    let mut dirs = Vec::with_capacity(config.engine.dirs.len());
    let mut counted_declared = Vec::with_capacity(config.engine.dirs.len());
    let mut missing = Vec::new();
    for target in &config.engine.dirs {
        let (_, prefix) = super::git::find_repository_of(&target.path)?;
        match checkout.find_target_of(&prefix) {
            Some(path) => {
                dirs.push(mezura_core::Target { module: target.module.clone(), path });
                counted_declared.push(target.clone());
            },
            None => missing.push(target.path.clone())
        }
    }
    if !missing.is_empty() {
        notes.push(Note::MissingInRevision { git_revision: git_revision.to_owned(), targets: missing });
    }

    let of_git_revision = mezura_core::EngineConfig { dirs,
            exclude_dirs: move_excludes_into_checkout(&checkout.path, &repository, &config.engine.exclude_dirs),
            ..config.engine.clone() };
    // A reading of zero and not a failure: it is what a revision older than every target holds
    let result = if of_git_revision.dirs.is_empty() {
        mezura_core::RunResult {
            per_language: HashMap::new(), total: mezura_core::Stats::default(), modules: Vec::new(),
            faulty_files: Vec::new(), unreadable_dirs: Vec::new(), targets: Vec::new(),
            files_present: FilesPresent::default(),
            performance: mezura_core::Performance { duration_millis: 0, threads: config.engine.threads.clone() }
        }
    } else {
        // Resolved against this configuration, as 'run' demands: the two differ only in where they
        // look, and the complaints are voiced by whoever asked for the counting
        let resolved = mezura_core::Languages::resolve(&of_git_revision, languages, extension_priority).0;
        let mut result = mezura_core::run(&of_git_revision, resolved, |_| {})
                .map_err(|x| GitError::Refused { doing: "counting the revision", message: x.to_string() })?;
        result.targets = counted_declared;
        result
    };

    Ok((Reading::of_git_revision(git_revision, checkout.commit.clone(), checkout.taken_at.clone(),
            result, &config.engine), notes))
}

// The checkout is the same tree at another root, so a pattern written as a full path moves with it
// or it would exclude on one side and count on the other.
fn move_excludes_into_checkout(checkout: &str, repository: &str, patterns: &[String]) -> Vec<String> {
    // ASCII folding only, so that folding never moves a byte and the remainder can be cut off the
    // unfolded pattern at the root's own length
    let key = |path: &str| {
        let path = path.replace('\\', "/");
        if cfg!(windows) {path.to_ascii_lowercase()} else {path}
    };
    let root = repository.trim_end_matches('/');
    let folded_root = key(root);

    patterns.iter().map(|pattern| {
        let normalized = pattern.replace('\\', "/");
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

    // The '--diff' help promises it: a name that is a file is a document, anything else is git's
    #[test]
    fn a_directory_is_never_read_as_a_document() {
        let dir = SCRATCH_DIR.to_owned() + "a-directory-named-like-a-branch";
        std::fs::create_dir_all(&dir).unwrap();
        assert!(read_document(&dir).is_none(), "a directory was taken for a document");
        std::fs::remove_dir_all(&dir).unwrap();

        // a file is still read, and a name that is nowhere on disk still falls through
        let file = SCRATCH_DIR.to_owned() + "not-a-document.json";
        std::fs::write(&file, "{ not a document").unwrap();
        assert!(matches!(read_document(&file), Some(Err(_))));
        std::fs::remove_file(&file).unwrap();
        assert!(read_document("no-such-thing-anywhere").is_none());
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

        // the platform's own idea of case, so a drive letter typed either way still travels
        if cfg!(windows) {
            assert_eq!(vec!["C:/tmp/chk/target".to_owned()],
                    move_excludes_into_checkout("C:/tmp/chk", "D:/repo", &["d:/REPO/target".to_owned()]));
        }
    }
}
