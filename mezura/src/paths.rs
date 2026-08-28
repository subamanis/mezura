// Where a user's own languages, themes, configurations and logs live, and where a project's own
// configuration and log live beside its code. Here and not in the library because the two sandboxes
// below are chosen with 'cfg!(test)', which is only true in the crate actually being tested: in the
// library they would stop applying the moment the binary became its own crate, and the binary's
// tests would go back to reading and writing the real directories without saying so.
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use directories::ProjectDirs;

pub const APP_NAME : &str = "mezura";
pub const LANGUAGES_DIR_NAME : &str = "languages";
pub const THEMES_DIR_NAME : &str = "themes";
pub const CONFIG_DIR_NAME : &str = "config";
pub const LOGS_DIR_NAME : &str = "logs";
pub const DEFAULT_CONFIG_NAME : &str = "default.txt";
pub const LOCAL_DIR_NAME : &str = ".mezura";
pub const LOCAL_CONFIG_FILE_NAME : &str = "config.txt";
pub const LOCAL_LOG_FILE_NAME : &str = "log.jsonl";
pub const DATA_DIR_VARIABLE : &str = "MEZURA_DATA_DIR";

const GLOB_METACHARACTERS : [char; 4] = ['*', '?', '[', '{'];

pub static PERSISTENT_APP_PATHS : LazyLock<PersistentAppPaths> = LazyLock::new(PersistentAppPaths::get);

#[derive(Debug)]
pub struct PersistentAppPaths {
    pub data_dir: String,
    pub languages_dir: String,
    pub themes_dir: String,
    pub config_dir: String,
    pub logs_dir: String,
    // Whether the environment named the directory rather than the system. The run says so while it
    // is true, since a variable left set months ago hides every saved configuration and theme with
    // nothing on screen to explain where they went.
    pub named_by_the_environment: bool
}

impl PersistentAppPaths {
    //Persistent paths:
    // Windows:  C:/Users/<user_name>/AppData/Roaming/mezura
    // Linux:    /home/<user_name>/.local/share/mezura
    // MacOs:    /Users/<user_name>/Library/Application Support/mezura
    pub fn get() -> Self {
        // Tests write real configuration and theme files through these paths, and a run interrupted
        // before its cleanup leaves loadable configurations behind: 'test_save_load_configs' starts
        // by demanding its own file is absent, so one such run fails it forever after. Asking the
        // system where the real directory is has to stay inside the other branch, or a machine with
        // no home directory fails every test that reaches this.
        let (data_dir, named_by_the_environment) = if cfg!(test) {
            (std::env::temp_dir().join(APP_NAME.to_owned() + "-test").to_string_lossy().into_owned() + "/", false)
        } else if let Some(named) = find_the_directory_the_environment_names() {
            (named, true)
        } else {
            (ProjectDirs::from("", "", APP_NAME)
                    .expect("no home directory could be found to put the application's data in")
                    .data_dir().to_str()
                    .expect("the application data directory path is not valid UTF-8").to_owned() + "/", false)
        };
        PersistentAppPaths {
            languages_dir: data_dir.clone() + LANGUAGES_DIR_NAME + "/",
            themes_dir: data_dir.clone() + THEMES_DIR_NAME + "/",
            config_dir: data_dir.clone() + CONFIG_DIR_NAME + "/",
            logs_dir: data_dir.clone() + LOGS_DIR_NAME + "/",
            data_dir,
            named_by_the_environment
        }
    }
}

// A '.mezura' folder sitting beside the code, holding the settings and the log of one project.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct LocalDir {
    // The directory holding the folder, without a trailing separator. A relative target in the
    // project's configuration is joined to this and not to the working directory, so the settings
    // mean the same thing from wherever inside the project the command was typed.
    pub project_dir: String,
    // A folder may hold a log and no settings at all, and then there is nothing to announce
    pub configuration_applied: bool
}

impl LocalDir {
    pub fn of(project_dir: &str) -> Self {
        LocalDir { project_dir: project_dir.trim_end_matches('/').to_owned(), configuration_applied: false }
    }

    // With the trailing separator that the configuration reader joins a file name to
    pub fn get_dir_path(&self) -> String {
        format!("{}/{LOCAL_DIR_NAME}/", self.project_dir)
    }

    pub fn get_config_path(&self) -> String {
        format!("{}/{LOCAL_DIR_NAME}/{LOCAL_CONFIG_FILE_NAME}", self.project_dir)
    }

    pub fn get_log_path(&self) -> String {
        format!("{}/{LOCAL_DIR_NAME}/{LOCAL_LOG_FILE_NAME}", self.project_dir)
    }
}

// The project's own folder, when the run is inside one. The search starts at the deepest directory
// holding every target and climbs towards the root, so the nearest folder wins and a project inside
// another one shadows it. An empty list is a run that named no targets and starts at the working
// directory.
pub fn find_local_dir(target_paths: &[String]) -> Option<LocalDir> {
    search_upwards_from(&find_common_ancestor(target_paths)?)
}

// Where a folder is created when one was asked for and the search above found none
pub fn choose_place_for_a_local_dir(target_paths: &[String]) -> Option<LocalDir> {
    let directory = find_common_ancestor(target_paths)?;
    may_be_searched(&directory).then(|| build_local_dir_at(&directory))
}

pub fn normalise_separators(path: &str) -> Cow<'_, str> {
    if cfg!(windows) {Cow::Owned(path.replace('\\', "/"))} else {Cow::Borrowed(path)}
}

pub fn fold_for_comparison(path: &str) -> Cow<'_, str> {
    if cfg!(windows) {Cow::Owned(normalise_separators(path).to_lowercase())} else {Cow::Borrowed(path)}
}

fn find_the_directory_the_environment_names() -> Option<String> {
    build_data_dir_path(std::env::var_os(DATA_DIR_VARIABLE)?.to_str()?)
}

// A trailing separator is put back if it is missing, since every path in the directory is built by
// appending to this one. An empty value is no value, the way RUST_BACKTRACE reads one.
fn build_data_dir_path(given: &str) -> Option<String> {
    let named = normalise_separators(given.trim()).into_owned();
    if named.is_empty() {
        return None;
    }

    Some(if named.ends_with('/') {named} else {named + "/"})
}

fn search_upwards_from(start: &Path) -> Option<LocalDir> {
    let mut directory = start;
    loop {
        if !may_be_searched(directory) {
            return None;
        }
        if directory.join(LOCAL_DIR_NAME).is_dir() {
            return Some(build_local_dir_at(directory));
        }
        directory = directory.parent()?;
    }
}

fn find_common_ancestor(target_paths: &[String]) -> Option<PathBuf> {
    let Some((first, rest)) = target_paths.split_first() else {
        return std::env::current_dir().ok();
    };

    let mut ancestor = find_anchor_of(first)?;
    for path in rest {
        ancestor = find_shared_prefix(&ancestor, &find_anchor_of(path)?)?;
    }

    Some(ancestor)
}

// The directory a target is searched from: itself when it names one, its folder when it names a
// file, and for a pattern the last directory written before the first wildcard, since everything
// 'D:/dev/*' can match sits inside 'D:/dev'.
fn find_anchor_of(path: &str) -> Option<PathBuf> {
    let target = Path::new(path);
    if target.is_dir() {
        return Some(target.to_path_buf());
    }
    if target.is_file() {
        return target.parent().map(Path::to_path_buf);
    }

    let named = &path[..path.find(GLOB_METACHARACTERS)?];
    if named.ends_with('/') {
        Some(PathBuf::from(named))
    } else {
        named.rsplit_once('/').map(|(directory, _)| PathBuf::from(directory))
    }
}

// Component by component and not byte by byte, or 'D:/dev/mez' and 'D:/dev/mezura' would share a
// directory that is neither of them. Two paths with nothing in common leave nothing rooted, which
// is the answer for targets on two drives: there is no directory holding both.
fn find_shared_prefix(one: &Path, other: &Path) -> Option<PathBuf> {
    let shared = one.components().zip(other.components())
            .take_while(|(a, b)| fold_for_comparison(&a.as_os_str().to_string_lossy())
                    == fold_for_comparison(&b.as_os_str().to_string_lossy()))
            .map(|(a, _)| a).collect::<PathBuf>();

    shared.has_root().then_some(shared)
}

fn build_local_dir_at(directory: &Path) -> LocalDir {
    LocalDir::of(&normalise_separators(&directory.to_string_lossy()))
}

// Under test the search never leaves the temporary directory. The tests of this crate run with the
// working directory at the package root, so a live search would really climb through the checkout
// and every folder above it, and the day one of them holds a '.mezura' every configuration test and
// every golden would read it and answer differently from one machine to the next. A test that needs
// a project builds one under 'std::env::temp_dir()'.
fn may_be_searched(directory: &Path) -> bool {
    !cfg!(test) || fold_for_comparison(&normalise_separators(&directory.to_string_lossy()))
            .starts_with(fold_for_comparison(&normalise_separators(
                    &std::env::temp_dir().to_string_lossy())).trim_end_matches('/'))
}

// What the tests read and what they write, kept apart. 'tests/fixtures' holds checked-in inputs and
// is never written to; 'test_dir' is scratch, ignored by git, and a test that writes there makes the
// directory it needs. Mixing the two hides a dependency: a test that writes a file without creating
// its directory then passes only because a checked-in fixture happened to be sitting in it.
#[cfg(test)]
pub mod test_paths {
    pub const FIXTURES_DIR       : &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
    pub const SCRATCH_DIR        : &str = concat!(env!("CARGO_MANIFEST_DIR"), "/test_dir/");
    pub const SCRATCH_CONFIG_DIR : &str = concat!(env!("CARGO_MANIFEST_DIR"), "/test_dir/config/");
    pub const SCRATCH_LOG_DIR    : &str = concat!(env!("CARGO_MANIFEST_DIR"), "/test_dir/logs/");
}

#[cfg(test)]
mod tests {
    use super::*;

    // One tree per test, cleared on the way in: a run that died before its cleanup would otherwise
    // leave a folder behind that decides the next run's answer.
    fn build_test_tree(test_name: &str, directories: &[&str]) -> PathBuf {
        let root = std::env::temp_dir().join("mezura-local-".to_owned() + test_name);
        let _ = std::fs::remove_dir_all(&root);
        for directory in directories {
            std::fs::create_dir_all(root.join(directory)).unwrap();
        }

        root
    }

    fn path_of(root: &Path, relative: &str) -> String {
        normalise_separators(&root.join(relative).to_string_lossy()).into_owned()
    }

    // Every path under the data directory is built by appending to it, so one arriving without a
    // separator at the end would put the languages in a folder called 'datalanguages'.
    #[test]
    fn a_data_directory_named_by_the_environment_always_ends_in_a_separator() {
        assert_eq!(Some("C:/tools/mezura-data/".to_owned()), build_data_dir_path("C:/tools/mezura-data"));
        assert_eq!(Some("C:/tools/mezura-data/".to_owned()), build_data_dir_path("  C:/tools/mezura-data/  "));
        if cfg!(windows) {
            assert_eq!(Some("C:/tools/mezura-data/".to_owned()), build_data_dir_path("C:\\tools\\mezura-data"));
        }
        assert_eq!(None, build_data_dir_path("   "));
        assert_eq!(None, build_data_dir_path(""));
    }

    #[test]
    fn a_project_folder_is_found_from_a_target_deep_inside_it() {
        let root = build_test_tree("deep-inside", &["proj/.mezura", "proj/a/b/c"]);

        let found = find_local_dir(&[path_of(&root, "proj/a/b/c")]).expect("the folder above the target was not found");
        assert_eq!(path_of(&root, "proj"), found.project_dir);
        assert_eq!(path_of(&root, "proj/.mezura/config.txt"), found.get_config_path());
        assert_eq!(path_of(&root, "proj/.mezura/log.jsonl"), found.get_log_path());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_project_inside_a_project_answers_for_what_is_inside_it() {
        let root = build_test_tree("nested-project", &["proj/.mezura", "proj/inner/.mezura", "proj/inner/src"]);

        assert_eq!(path_of(&root, "proj/inner"),
                find_local_dir(&[path_of(&root, "proj/inner/src")]).unwrap().project_dir);
        assert_eq!(path_of(&root, "proj"), find_local_dir(&[path_of(&root, "proj")]).unwrap().project_dir);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn several_targets_are_searched_from_the_directory_holding_all_of_them() {
        let root = build_test_tree("several-targets", &["proj/.mezura", "proj/a/one", "proj/b/two", "outside"]);

        let inside = [path_of(&root, "proj/a/one"), path_of(&root, "proj/b/two")];
        assert_eq!(path_of(&root, "proj"), find_local_dir(&inside).unwrap().project_dir);

        // The directory holding both is above the project, so the project's folder is not this
        // run's to use
        let one_outside = [path_of(&root, "proj/a/one"), path_of(&root, "outside")];
        assert_eq!(None, find_local_dir(&one_outside));

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_file_and_a_pattern_are_searched_from_the_directory_they_name() {
        let root = build_test_tree("file-and-pattern", &["proj/.mezura", "proj/src"]);
        std::fs::write(root.join("proj/src/main.rs"), "fn main() {}").unwrap();

        for target in [path_of(&root, "proj/src/main.rs"), path_of(&root, "proj/src/*"),
                path_of(&root, "proj/src/*.rs"), path_of(&root, "proj/*/main.rs")] {
            assert_eq!(Some(path_of(&root, "proj")), find_local_dir(std::slice::from_ref(&target)).map(|x| x.project_dir),
                    "'{target}' was not searched from the directory it names");
        }

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_search_that_finds_no_folder_at_all_is_no_answer_rather_than_the_nearest_directory() {
        let root = build_test_tree("nothing-found", &["proj/src"]);

        assert_eq!(None, find_local_dir(&[path_of(&root, "proj/src")]));
        // A place to create one is still known, and it is the directory the target named
        assert_eq!(Some(path_of(&root, "proj/src")),
                choose_place_for_a_local_dir(&[path_of(&root, "proj/src")]).map(|x| x.project_dir));

        std::fs::remove_dir_all(&root).unwrap();
    }

    // The sandbox of 'may_be_searched'. Without it every test that builds a configuration would
    // really climb through the checkout, and a '.mezura' anywhere above it would change what they
    // all answer.
    #[test]
    fn nothing_outside_the_temporary_directory_is_reached_by_a_test() {
        let package_root = env!("CARGO_MANIFEST_DIR").to_owned();

        assert_eq!(None, find_local_dir(std::slice::from_ref(&package_root)));
        assert_eq!(None, choose_place_for_a_local_dir(&[package_root]));
    }

    #[test]
    fn two_paths_with_nothing_in_common_have_no_directory_holding_both() {
        assert_eq!(None, find_shared_prefix(Path::new("D:/one"), Path::new("E:/other")));
        assert_eq!(Some(PathBuf::from("/a")), find_shared_prefix(Path::new("/a/b"), Path::new("/a/c")));
        // A shared spelling that is not a shared directory: 'mez' is not a prefix of the path, it
        // is a prefix of the name of one directory in it
        assert_eq!(Some(PathBuf::from("/a")), find_shared_prefix(Path::new("/a/mez"), Path::new("/a/mezura")));
    }
}
