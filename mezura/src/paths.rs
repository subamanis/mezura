// Where a user's own languages, themes, configurations and logs live. Here and not in the library
// because the sandbox below is chosen with 'cfg!(test)', which is only true in the crate actually
// being tested: in the library it would stop applying the moment the binary became its own crate,
// and the binary's tests would go back to reading and writing the real directory without saying so.
use std::borrow::Cow;
use std::sync::LazyLock;

use directories::ProjectDirs;

pub const APP_NAME : &str = "mezura";
pub const LANGUAGES_DIR_NAME : &str = "languages";
pub const THEMES_DIR_NAME : &str = "themes";
pub const CONFIG_DIR_NAME : &str = "config";
pub const LOGS_DIR_NAME : &str = "logs";
pub const DEFAULT_CONFIG_NAME : &str = "default.txt";

pub static PERSISTENT_APP_PATHS : LazyLock<PersistentAppPaths> = LazyLock::new(PersistentAppPaths::get);

#[derive(Debug)]
pub struct PersistentAppPaths {
    pub data_dir: String,
    pub languages_dir: String,
    pub themes_dir: String,
    pub config_dir: String,
    pub logs_dir: String
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
        let data_dir = if cfg!(test) {
            std::env::temp_dir().join(APP_NAME.to_owned() + "-test").to_string_lossy().into_owned() + "/"
        } else {
            ProjectDirs::from("", "", APP_NAME)
                    .expect("no home directory could be found to put the application's data in")
                    .data_dir().to_str()
                    .expect("the application data directory path is not valid UTF-8").to_owned() + "/"
        };
        PersistentAppPaths {
            languages_dir: data_dir.clone() + LANGUAGES_DIR_NAME + "/",
            themes_dir: data_dir.clone() + THEMES_DIR_NAME + "/",
            config_dir: data_dir.clone() + CONFIG_DIR_NAME + "/",
            logs_dir: data_dir.clone() + LOGS_DIR_NAME + "/",
            data_dir
        }
    }
}

pub fn normalise_separators(path: &str) -> Cow<'_, str> {
    if cfg!(windows) {Cow::Owned(path.replace('\\', "/"))} else {Cow::Borrowed(path)}
}

pub fn fold_for_comparison(path: &str) -> Cow<'_, str> {
    if cfg!(windows) {Cow::Owned(normalise_separators(path).to_lowercase())} else {Cow::Borrowed(path)}
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
