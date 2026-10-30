// How a reading is acquired from each kind of source. Nothing here prints: what a reader must be
// told travels back as notes and is said where the comparison is shown.
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::thread::JoinHandle;

use mezura_core::{EngineConfig, FilesPresent, Language, PathPatterns, ScanProgress};
use mezura_core::language_file::ConflictRules;

use super::config_manager::{Configuration, TargetCheckout, absolutize_local_path};
use super::diff::{Note, Reading};
use super::git::{Checkout, GitError, ResolvedRevision, TargetSpec, format_revision_label, parse_target_spec};
use super::paths::is_unquoted_glob_pattern;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevisionTargetOverride {
    pub prefix: String,
    pub declared_target: String,
    pub display_repository: String
}

#[derive(Clone, Debug)]
pub struct PreparedRevision {
    pub resolved: ResolvedRevision,
    pub target_override: Option<RevisionTargetOverride>
}

pub struct RevisionSide {
    resolved: ResolvedRevision,
    target_override: Option<RevisionTargetOverride>,
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
pub fn prepare_revisions_for_config(names: &[&str], config: &Configuration)
-> Result<Vec<PreparedRevision>, GitError>
{
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let hidden = config.view.hidden.animations || !config.view.prints_text();
    let mut prepared = Vec::with_capacity(names.len());
    let mut common_repository : Option<String> = None;

    for &name in names {
        match parse_target_spec(name)? {
            TargetSpec::Remote(remote) => {
                let live = super::animated_display::start_checkout_display(
                        "Fetching", &remote.format_short_label(), hidden);
                let mut resolved = super::git::fetch_remote_revision(&remote)?;
                live.finish();
                resolved.revision = name.to_owned();
                let same_repo_root = config.view.single_unnamed_checkout_root
                        && config.view.target_checkouts.first()
                                .and_then(|tc| tc.remote_url.as_deref()) == Some(remote.url.as_str());
                let target_override = (!same_repo_root).then(|| RevisionTargetOverride {
                    prefix: String::new(),
                    declared_target: remote.display.clone(),
                    display_repository: remote.display
                });
                prepared.push(PreparedRevision { resolved, target_override });
            },
            TargetSpec::LocalRevision { path, revision, display } if !name.trim().starts_with('@') => {
                let absolute = absolutize_local_path(&path);
                let (repository, prefix) = super::git::find_repository_of(&absolute)?;
                let mut resolved = super::git::resolve_revision(&repository, &revision)?;
                resolved.revision = name.to_owned();
                let same_repo_root = config.view.single_unnamed_checkout_root
                        && prefix.trim_matches('/').is_empty()
                        && find_common_repository_for_config(config).ok().as_deref()
                                .is_some_and(|common| super::paths::fold_for_comparison(common)
                                        == super::paths::fold_for_comparison(&repository));
                let target_override = (!same_repo_root).then(|| RevisionTargetOverride {
                    prefix,
                    declared_target: display,
                    display_repository: format!("{repository}@{revision}")
                });
                prepared.push(PreparedRevision { resolved, target_override });
            },
            TargetSpec::LocalRevision { revision, .. } | TargetSpec::LocalPath(revision) => {
                let repository = match &common_repository {
                    Some(repo) => repo.clone(),
                    None => {
                        let repo = find_common_repository_for_config(config)?;
                        common_repository = Some(repo.clone());
                        repo
                    }
                };
                let mut resolved = if super::git::is_remote_temp_repository(&repository) {
                    let live = super::animated_display::start_checkout_display("Fetching", &revision, hidden);
                    let resolved = super::git::resolve_revision(&repository, &revision)?;
                    live.finish();
                    resolved
                } else {
                    super::git::resolve_revision(&repository, &revision)?
                };
                resolved.revision = name.to_owned();
                prepared.push(PreparedRevision { resolved, target_override: None });
            }
        }
    }

    if let [first, second] = &prepared[..]
            && first.resolved.repository == second.resolved.repository
            && first.resolved.commit == second.resolved.commit
            && first.target_override.as_ref().map(|x| &x.prefix) == second.target_override.as_ref().map(|x| &x.prefix) {
        return Err(GitError::SameCommit { first: first.resolved.revision.clone(),
                second: second.resolved.revision.clone(), commit: first.resolved.commit.clone() });
    }

    Ok(prepared)
}

// The leftovers are swept exactly once, before anything writes: the sweep's prune beside a half
// registered parallel write is the one interference between them. With two sides both writes start
// at once while the counting stays serial, so the second hides behind the first side's write and
// count; a single side, or a thread that cannot spawn, writes inline at its turn.
pub fn start_acquiring_prepared_revisions(prepared: Vec<PreparedRevision>) -> Vec<RevisionSide> {
    if let Some(first) = prepared.first() {
        super::git::remove_leftover_checkouts(&first.resolved.repository);
    }
    let ahead = prepared.len() >= 2;
    prepared.into_iter().map(|PreparedRevision { resolved, target_override }| {
        let write = if ahead {
            let for_thread = resolved.clone();
            std::thread::Builder::new().name("revision-write".to_owned())
                    .spawn(move || super::git::checkout(&for_thread)).ok()
        } else {
            None
        };
        RevisionSide { resolved, target_override, write }
    }).collect()
}

pub fn count_git_revision(mut side: RevisionSide, config: &Configuration, languages: Vec<Language>,
        conflicts: &ConflictRules) -> Result<(Reading, Vec<Note>), GitError>
{
    // Erased when it drops, before this function returns: the whole phase leaves nothing behind on
    // the permanent output
    let progress = Arc::new(ScanProgress::default());
    let display_label = format_revision_label(&side.resolved.revision);
    let _live = super::animated_display::start_revision_display(config, &display_label, "Checking out",
            progress.clone(), side.write.as_ref().is_some_and(|x| x.is_finished()));

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
    let display_repository = if let Some(override_target) = &side.target_override {
        match checkout.find_target_of(&override_target.prefix) {
            Some(path) => {
                targets.push(mezura_core::Target { module: None, path });
                counted_declared.push(mezura_core::Target::of(&override_target.declared_target));
            },
            None => missing.push(override_target.declared_target.clone())
        }
        override_target.display_repository.clone()
    } else if !config.view.target_checkouts.is_empty() {
        for target in &config.engine.targets {
            let (prefix, declared_path) = match config.view.target_checkouts.iter()
                    .find_map(|tc| replace_root_prefix(&target.path, &tc.checkout_root, "")
                            .map(|suffix| (tc, suffix))) {
                Some((tc, suffix)) => {
                    let prefix = suffix.trim_start_matches('/').to_owned();
                    let declared = format!("{}{suffix}", tc.get_base_root().trim_end_matches('/'));
                    (prefix, declared)
                },
                None => {
                    let (_, prefix) = super::git::find_repository_of(&target.path)?;
                    (prefix, target.path.clone())
                }
            };
            match checkout.find_target_of(&prefix) {
                Some(path) => {
                    targets.push(mezura_core::Target { module: target.module.clone(), path });
                    counted_declared.push(mezura_core::Target { module: target.module.clone(), path: declared_path });
                },
                None => missing.push(declared_path)
            }
        }
        config.view.target_checkouts[0].get_base_root().to_owned()
    } else {
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
        resolved.repository.clone()
    };
    if !missing.is_empty() {
        notes.push(Note::MissingInRevision { git_revision: git_revision.to_owned(), targets: missing });
    }

    let mut exclude_patterns = move_patterns_into_checkout(&checkout.path, &resolved.repository,
            &config.engine.exclude_patterns);
    let mut test_patterns = move_patterns_into_checkout(&checkout.path, &resolved.repository,
            &config.engine.test_patterns);
    for tc in &config.view.target_checkouts {
        exclude_patterns = move_patterns_into_checkout(&checkout.path, &tc.checkout_root, &exclude_patterns);
        test_patterns = move_patterns_into_checkout(&checkout.path, &tc.checkout_root, &test_patterns);
    }

    let of_git_revision = EngineConfig { targets, exclude_patterns, test_patterns,
            checkout: Some(mezura_core::RepositoryCheckout { root: checkout.path.clone(), repository: resolved.repository.clone() }),
            ..config.engine.clone() };
    // A revision older than every target holds a reading of zero, which is no failure
    let result = if of_git_revision.targets.is_empty() {
        mezura_core::RunResult {
            per_language: HashMap::new(), total: mezura_core::Stats::default(), modules: Vec::new(),
            nested_languages: HashMap::new(), tests: HashMap::new(),
            faulty_files: Vec::new(), skipped_files: mezura_core::SkippedFiles::default(),
            unreadable_dirs: Vec::new(), targets: Vec::new(), warnings: Vec::new(), applied_presets: Vec::new(),
            files_present: FilesPresent::default(),
            performance: mezura_core::Performance { duration_millis: 0, threads: config.engine.threads }
        }
    } else {
        let resolved_langs = mezura_core::Languages::resolve(&of_git_revision, languages, conflicts).0;
        let mut result = mezura_core::run_watched(&of_git_revision, resolved_langs, Some(&progress), |_| {})
                .map_err(|error| GitError::CountingRevision { revision: git_revision.to_owned(), error })?;
        move_file_paths_out_of_checkout(&mut result, &of_git_revision.targets, &counted_declared);
        move_preset_directories_out_of_checkout(&mut result, &checkout.path, &display_repository);
        result.targets = counted_declared;
        result
    };

    Ok((Reading::of_git_revision(git_revision, resolved.commit.clone(), resolved.repository.clone(),
            display_repository, resolved.taken_at.clone(), result, config), notes))
}

pub fn replace_root_prefix(path: &str, from_root: &str, to_root: &str) -> Option<String> {
    let normalized = super::paths::normalise_separators(path);
    let folded = super::paths::fold_ascii_case(&normalized);
    let from = from_root.trim_end_matches('/');
    super::paths::is_inside(&folded, &super::paths::fold_ascii_case(from)).then(|| {
        let suffix = &normalized[from.len()..];
        format!("{}{suffix}", to_root.trim_end_matches('/'))
    })
}

pub fn format_path_outside_checkouts(path: &str, checkouts: &[TargetCheckout]) -> String {
    checkouts.iter().find_map(|tc| replace_root_prefix(path, &tc.checkout_root, &tc.display_root))
            .unwrap_or_else(|| path.to_owned())
}

pub fn format_path_inside_checkouts(path: &str, checkouts: &[TargetCheckout]) -> String {
    checkouts.iter().find_map(|tc| replace_root_prefix(path, &tc.display_root, &tc.checkout_root)
            .or_else(|| replace_root_prefix(path, tc.get_base_root(), &tc.checkout_root)))
            .unwrap_or_else(|| path.to_owned())
}

pub fn format_pattern_outside_checkouts(pattern: &str, checkouts: &[TargetCheckout]) -> String {
    let (negation, place) = mezura_core::split_negation(pattern);
    match checkouts.iter().find_map(|tc| replace_root_prefix(place, &tc.checkout_root, tc.get_base_root())) {
        Some(rewritten) => format!("{negation}{rewritten}"),
        None => pattern.to_owned()
    }
}

pub fn format_pattern_inside_checkouts(pattern: &str, checkouts: &[TargetCheckout]) -> String {
    let (negation, place) = mezura_core::split_negation(pattern);
    format!("{negation}{}", format_path_inside_checkouts(place, checkouts))
}

pub fn move_result_paths_out_of_checkouts(result: &mut mezura_core::RunResult, checkouts: &[TargetCheckout]) {
    if checkouts.is_empty() {
        return;
    }
    let out = |path: &mut String| *path = format_path_outside_checkouts(path, checkouts);
    result.targets.iter_mut().for_each(|t| out(&mut t.path));
    result.modules.iter_mut().flat_map(|m| m.files.values_mut()).flatten().for_each(|f| out(&mut f.path));
    result.applied_presets.iter_mut().for_each(|p| out(&mut p.directory));
    result.faulty_files.iter_mut().for_each(|f| out(&mut f.path));
    for list in [&mut result.skipped_files.minified, &mut result.skipped_files.generated, &mut result.skipped_files.not_code] {
        list.iter_mut().for_each(out);
    }
    result.unreadable_dirs.iter_mut().for_each(|d| out(&mut d.path));
    for warning in &mut result.warnings {
        warning.subject = format_pattern_outside_checkouts(&warning.subject, checkouts);
        for tc in checkouts {
            let (root, base) = (tc.checkout_root.trim_end_matches('/'), tc.get_base_root().trim_end_matches('/'));
            if warning.message.contains(root) {
                warning.message = warning.message.replace(root, base);
            }
        }
    }
}

fn find_common_repository_for_config(config: &Configuration) -> Result<String, GitError> {
    if config.engine.targets.is_empty() {
        return Ok(super::git::find_repository_of(".")?.0);
    }
    let paths : Vec<String> = config.engine.targets.iter().map(|target| {
        if is_unquoted_glob_pattern(&target.path) {
            return format_path_outside_checkouts(&target.path, &config.view.target_checkouts);
        }
        match config.view.target_checkouts.iter()
                .find(|tc| replace_root_prefix(&target.path, &tc.checkout_root, "").is_some()) {
            Some(tc) => tc.repository.clone(),
            None => target.path.clone()
        }
    }).collect();
    super::git::find_common_repository_of(&paths)
}

// The file rows still name the temporary checkout, which is a different directory on every run, so
// each is given back the declared form of the target it was found under. The two lists correspond
// by position, having been built together above; the longest match decides, since one declared
// target may sit inside another's checkout path.
fn move_file_paths_out_of_checkout(result: &mut mezura_core::RunResult,
        checkout_targets: &[mezura_core::Target], declared: &[mezura_core::Target])
{
    let mut pairs = checkout_targets.iter().zip(declared)
            .map(|(counted, declared)| (counted.path.as_str(), declared.path.as_str()))
            .collect::<Vec<_>>();
    pairs.sort_by_key(|(counted, _)| std::cmp::Reverse(counted.len()));

    for files in result.modules.iter_mut().flat_map(|module| module.files.values_mut()) {
        for file in files {
            if let Some((counted, declared)) = pairs.iter().find(|(counted, _)|
                    super::paths::is_inside(&file.path, counted)) {
                file.path = declared.to_string() + &file.path[counted.len()..];
            }
        }
    }
}

// A tool's file inside the checkout stands for the same file in the repository, and the checkout is a
// different directory on every run
fn move_preset_directories_out_of_checkout(result: &mut mezura_core::RunResult, checkout: &str, repository: &str) {
    let checkout = super::paths::normalise_separators(checkout).trim_end_matches('/').to_owned();
    let repository = super::paths::normalise_separators(repository).trim_end_matches('/').to_owned();
    for preset in &mut result.applied_presets {
        if super::paths::is_inside(&preset.directory, &checkout) {
            preset.directory = repository.clone() + &preset.directory[checkout.len()..];
        }
    }
}

// The checkout is the same tree at another root, so a place inside the repository moves with it, and
// so does the project folder the names are read from, or a pattern would apply on one side only
fn move_patterns_into_checkout(checkout: &str, repository: &str, patterns: &PathPatterns) -> PathPatterns {
    let key = |path: &str| super::paths::fold_ascii_case(&super::paths::normalise_separators(path)).into_owned();
    let root = repository.trim_end_matches('/');
    let folded_root = key(root);
    let moved = |place: &str| {
        let normalized = super::paths::normalise_separators(place).into_owned();
        let folded = key(&normalized);
        if folded == folded_root {
            checkout.to_owned()
        } else if folded.starts_with(&(folded_root.clone() + "/")) {
            checkout.to_owned() + &normalized[root.len()..]
        } else {
            place.to_owned()
        }
    };

    PathPatterns {
        patterns: patterns.patterns.iter().map(|pattern| {
            let (negation, place) = mezura_core::split_negation(pattern);
            format!("{negation}{}", moved(place))
        }).collect(),
        read_from: patterns.read_from.as_deref().map(moved),
        reports_unmatched_names: patterns.reports_unmatched_names
    }
}

#[cfg(test)]
mod tests {
    use crate::paths::test_paths::{CORE_LANGUAGES_DIR, SCRATCH_DIR};

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
        let config = Configuration::new(vec![package.clone()]);
        let (repository, _) = crate::git::find_repository_of(&package).unwrap();
        let head = crate::git::resolve_revision(&repository, "HEAD").unwrap();

        let refused = prepare_revisions_for_config(&["HEAD", &head.commit], &config);
        assert!(matches!(refused, Err(GitError::SameCommit { .. })), "{refused:?}");
        assert!(matches!(prepare_revisions_for_config(&["HEAD", "HEAD"], &config), Err(GitError::SameCommit { .. })));

        let alone = prepare_revisions_for_config(&["HEAD"], &config).unwrap();
        assert_eq!(head.commit, alone[0].resolved.commit);
        assert!(alone[0].target_override.is_none());
        assert!(prepare_revisions_for_config(&[], &config).unwrap().is_empty());

        let src = format!("{package}/src@HEAD");
        let tests_dir = format!("{package}/tests@HEAD");
        let distinct_subpaths = prepare_revisions_for_config(&[&src, &tests_dir], &config).unwrap();
        assert_eq!(Some("mezura/src/"), distinct_subpaths[0].target_override.as_ref().map(|x| x.prefix.as_str()));
        assert_eq!(Some("mezura/tests/"), distinct_subpaths[1].target_override.as_ref().map(|x| x.prefix.as_str()));
        assert!(matches!(prepare_revisions_for_config(&[&src, &src], &config), Err(GitError::SameCommit { .. })));
    }

    #[test]
    fn a_pattern_declares_the_same_tests_in_a_revision_as_in_the_tree() {
        let _guard = crate::git::GIT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join("mezura-revision-patterns");
        let _ = crate::git::remove_directory_tree(&root);
        std::fs::create_dir_all(root.join("spec")).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("api/src/gen")).unwrap();
        std::fs::write(root.join("spec/a.rs"), "fn a() {}\nfn b() {}\n").unwrap();
        std::fs::write(root.join("src/b.rs"), "fn c() {}\n").unwrap();
        std::fs::write(root.join("api/src/c.rs"), "fn d() {}\n").unwrap();
        std::fs::write(root.join("api/src/gen/g.rs"), "fn e() {}\n").unwrap();
        commit_everything_in(&root);

        let (repository, _) = crate::git::find_repository_of(&root.to_string_lossy()).unwrap();
        let parsed = mezura_core::language_file::parse_languages_in_dir(CORE_LANGUAGES_DIR).unwrap().0;
        let both_sides = |config: &Configuration| {
            let prepared = prepare_revisions_for_config(&["HEAD"], config).unwrap();
            let side = start_acquiring_prepared_revisions(prepared).into_iter().next().unwrap();
            let (revision, notes) = count_git_revision(side, config, parsed.clone(), &Default::default()).unwrap();
            assert!(notes.is_empty(), "{notes:?}");
            let tree = mezura_core::run(&config.engine,
                    mezura_core::Languages::resolve(&config.engine, parsed.clone(), &Default::default()).0).unwrap();
            (tree, revision)
        };
        // Lines and files alone, since a checkout may spell its line endings differently
        let counted = |tests: &HashMap<String, mezura_core::TestCode>| tests.iter()
                .map(|(language, code)| (language.clone(), code.stats.lines, code.stats.files, code.whole_files))
                .collect::<Vec<_>>();

        let mut config = Configuration::new(vec![repository.clone()]);
        config.engine.test_patterns = mezura_core::PathPatterns::of(["spec/"]);
        let (tree, revision) = both_sides(&config);
        assert_eq!(vec![("Rust".to_owned(), 2, 1, 1)], counted(&tree.tests));
        assert_eq!(counted(&tree.tests), counted(&revision.result.tests));
        assert_eq!(tree.total.lines, revision.result.total.lines);
        assert_eq!(vec!["spec/".to_owned()], revision.scope.tests);

        let mut config = Configuration::new(vec![repository.clone()]);
        config.engine.test_patterns = mezura_core::PathPatterns::of([format!("{repository}/spec")]);
        config.engine.exclude_patterns = mezura_core::PathPatterns::of([format!("{repository}/src")]);
        let (tree, revision) = both_sides(&config);
        assert_eq!(vec![("Rust".to_owned(), 2, 1, 1)], counted(&tree.tests), "a place did not declare the tests of the tree");
        assert_eq!(counted(&tree.tests), counted(&revision.result.tests), "a place was not carried into the checkout");
        assert_eq!((4, 4), (tree.total.lines, revision.result.total.lines), "a place did not leave 'src' out on both sides");
        assert_eq!(vec![format!("{repository}/src")], revision.scope.exclude, "the scope named the checkout");

        let mut config = Configuration::new(vec![format!("{repository}/api/src")]);
        config.engine.exclude_patterns = mezura_core::PathPatterns::of(["api/src/gen/"]);
        let (tree, revision) = both_sides(&config);
        assert_eq!((2, 2), (tree.total.lines, revision.result.total.lines), "a name saw a folder above the target");
        config.engine.exclude_patterns = mezura_core::PathPatterns { patterns: vec!["api/src/gen/".to_owned()],
                read_from: Some(format!("{repository}/api")), ..Default::default() };
        let (tree, revision) = both_sides(&config);
        assert_eq!((1, 1), (tree.total.lines, revision.result.total.lines),
                "the folder a project's names are read from was not carried into the checkout");

        crate::git::await_checkout_removals();
        crate::git::remove_directory_tree(&root).unwrap();
    }

    #[test]
    fn a_revision_reads_the_test_tool_files_above_its_repository() {
        let _guard = crate::git::GIT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let holder = std::env::temp_dir().join("mezura-revision-above");
        let _ = crate::git::remove_directory_tree(&holder);
        let root = holder.join("repo");
        std::fs::create_dir_all(root.join("ui")).unwrap();
        std::fs::write(holder.join("jest.config.js"), "module.exports = {};\n").unwrap();
        std::fs::write(root.join("ui/a.test.ts"), "test('a', () => {});\n").unwrap();
        std::fs::write(root.join("ui/a.ts"), "export const a = 1;\n").unwrap();
        commit_everything_in(&root);

        let parsed = mezura_core::language_file::parse_languages_in_dir(CORE_LANGUAGES_DIR).unwrap().0;
        let config = Configuration::new(vec![crate::paths::normalise_separators(&root.to_string_lossy()).into_owned()]);
        let side = start_acquiring_prepared_revisions(prepare_revisions_for_config(&["HEAD"], &config).unwrap()).into_iter().next().unwrap();
        let (revision, _) = count_git_revision(side, &config, parsed.clone(), &Default::default()).unwrap();
        let tree = mezura_core::run(&config.engine,
                mezura_core::Languages::resolve(&config.engine, parsed, &Default::default()).0).unwrap();
        let files_of_tests = |result: &mezura_core::RunResult| result.tests.get("TypeScript").map(|code| code.stats.files);
        assert_eq!(Some(1), files_of_tests(&tree));
        assert_eq!(files_of_tests(&tree), files_of_tests(&revision.result),
                "the revision did not read the jest configuration above its repository");

        crate::git::await_checkout_removals();
        crate::git::remove_directory_tree(&holder).unwrap();
    }

    #[test]
    fn the_file_rows_of_a_revision_get_back_the_declared_form_of_their_targets() {
        let entry = |path: &str| mezura_core::FileEntry { path: path.to_owned(),
                stats: mezura_core::Stats::default(), nested_languages: HashMap::new(), tests: None };
        let mut result = crate::test_support::plain_result_of(HashMap::new(),
                vec![mezura_core::ModuleResult { name: None, per_language: HashMap::new(),
                        nested_languages: HashMap::new(), tests: HashMap::new(), total: mezura_core::Stats::default(),
                        files: hashmap!["Rust".to_owned() => vec![entry("C:/t/chk/api/a.rs"),
                                entry("C:/t/chk/apix/b.rs"), entry("C:/t/chk/main.rs"),
                                entry("D:/elsewhere/c.rs")]] }],
                Vec::new());

        let checkout = [mezura_core::Target::of("C:/t/chk/api"), mezura_core::Target::of("C:/t/chk/main.rs"),
                mezura_core::Target::of("C:/t/chk")];
        let declared = [mezura_core::Target::of("D:/r/api-v2"), mezura_core::Target::of("D:/r/main.rs"),
                mezura_core::Target::of("D:/r")];
        move_file_paths_out_of_checkout(&mut result, &checkout, &declared);

        // The longest checkout prefix decides, whole components only, and a path outside them stays
        assert_eq!(vec!["D:/r/api-v2/a.rs", "D:/r/apix/b.rs", "D:/r/main.rs", "D:/elsewhere/c.rs"],
                result.modules[0].files["Rust"].iter().map(|x| x.path.as_str()).collect::<Vec<_>>());
    }

    #[test]
    fn every_path_bearing_collection_of_a_result_is_moved_out_of_its_checkouts() {
        let _guard = crate::git::GIT_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join("mezura-move-result-paths");
        let _ = crate::git::remove_directory_tree(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.rs"), "fn a() {}\n").unwrap();
        commit_everything_in(&root);
        let root_str = crate::paths::normalise_separators(&root.to_string_lossy()).into_owned();
        let resolved = crate::git::resolve_revision(&root_str, "HEAD").unwrap();
        let checkout = crate::git::checkout(&resolved).unwrap();
        let chk = checkout.path.clone();
        let base = "https://github.com/owner/repo";
        let display = format!("{base}@v1.0");
        let checkouts = [crate::config_manager::TargetCheckout::new(
                checkout, display.clone(), Some(base.to_owned()))];

        let parsed = mezura_core::language_file::parse_languages_in_dir(CORE_LANGUAGES_DIR).unwrap().0;
        let mut engine = EngineConfig::new([format!("{chk}/src")]);
        engine.exclude_patterns = PathPatterns::of([format!("{chk}/missing")]);
        let (languages, _) = mezura_core::Languages::resolve(&engine, parsed, &Default::default());
        let mut result = mezura_core::run(&engine, languages).unwrap();

        let entry = |path: &str| mezura_core::FileEntry { path: path.to_owned(),
                stats: mezura_core::Stats::default(), nested_languages: HashMap::new(), tests: None };
        result.targets = vec![mezura_core::Target::named("core", format!("{chk}/src"))];
        result.modules[0].files.insert("Rust".to_owned(), vec![entry(&format!("{chk}/src/a.rs"))]);
        result.applied_presets = vec![mezura_core::AppliedPreset {
            name: "pytest".to_owned(),
            directory: format!("{chk}/src"),
            trigger_file: "pytest.ini".to_owned(),
            matched: true
        }];
        result.faulty_files = vec![mezura_core::FaultyFileDetails {
            path: format!("{chk}/src/bad.rs"),
            size: 12,
            error_msg: "invalid utf-8".to_owned()
        }];
        result.skipped_files.minified = vec![format!("{chk}/src/app.min.js")];
        result.unreadable_dirs = vec![mezura_core::UnreadableDirDetails {
            path: format!("{chk}/src/secret"),
            error_msg: "denied".to_owned()
        }];

        move_result_paths_out_of_checkouts(&mut result, &checkouts);
        assert_eq!(vec![mezura_core::Target::named("core", format!("{display}/src"))], result.targets);
        assert_eq!(format!("{display}/src/a.rs"), result.modules[0].files["Rust"][0].path);
        assert_eq!(format!("{display}/src"), result.applied_presets[0].directory);
        assert_eq!(format!("{display}/src/bad.rs"), result.faulty_files[0].path);
        assert_eq!(vec![format!("{display}/src/app.min.js")], result.skipped_files.minified);
        assert_eq!(format!("{display}/src/secret"), result.unreadable_dirs[0].path);
        assert_eq!(format!("{base}/missing"), result.warnings[0].subject);
        assert!(result.warnings[0].message.contains(&format!("{base}/missing")), "{}", result.warnings[0].message);
        assert!(!result.warnings[0].message.contains(&chk), "{}", result.warnings[0].message);

        drop(checkouts);
        crate::git::await_checkout_removals();
        crate::git::remove_directory_tree(&root).unwrap();
    }

    #[test]
    fn a_place_inside_the_repository_is_carried_into_the_checkout_and_so_is_the_project_folder() {
        let of = |patterns: &[&str]| PathPatterns::of(patterns);
        let moved = move_patterns_into_checkout("C:/tmp/chk/repo", "D:/repo",
                &of(&["fixtures", "*.min.js", "D:/repo/target", "D:/repo", "D:/elsewhere/target", "D:/repo/a/b/",
                        "!D:/repo/spec/fixtures", "D:/repo/*/gen"]));

        assert_eq!(of(&["fixtures", "*.min.js", "C:/tmp/chk/repo/target", "C:/tmp/chk/repo", "D:/elsewhere/target",
                "C:/tmp/chk/repo/a/b/", "!C:/tmp/chk/repo/spec/fixtures", "C:/tmp/chk/repo/*/gen"]), moved);

        assert_eq!(of(&["D:/repository/x"]), move_patterns_into_checkout("C:/tmp/chk/repo", "D:/repo", &of(&["D:/repository/x"])),
                "a path sharing the repository's first characters was moved");

        let of_a_project = |project: &str| PathPatterns { patterns: vec!["spec/".to_owned()], read_from: Some(project.to_owned()),
                ..Default::default() };
        assert_eq!(of_a_project("C:/tmp/chk/repo/api"),
                move_patterns_into_checkout("C:/tmp/chk/repo", "D:/repo", &of_a_project("D:/repo/api")),
                "the folder the names are read from stayed outside the checkout");
        assert_eq!(of_a_project("D:/elsewhere"), move_patterns_into_checkout("C:/tmp/chk/repo", "D:/repo", &of_a_project("D:/elsewhere")));

        if cfg!(windows) {
            assert_eq!(of(&["C:/tmp/chk/repo/target"]), move_patterns_into_checkout("C:/tmp/chk/repo", "D:/repo", &of(&["d:/REPO/target"])));
            assert_eq!(of(&["C:/tmp/chk/repo/a/b"]), move_patterns_into_checkout("C:/tmp/chk/repo", "D:/repo", &of(&["D:\\repo\\a\\b"])));
        }
    }

    fn commit_everything_in(root: &Path) {
        let git = |arguments: &[&str]| {
            let outcome = std::process::Command::new("git").arg("-C").arg(root).args(arguments).output().unwrap();
            assert!(outcome.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&outcome.stderr));
        };
        git(&["init", "-q"]);
        git(&["add", "."]);
        git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "--no-verify", "-q", "-m", "x"]);
    }
}
