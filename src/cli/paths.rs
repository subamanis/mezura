// Where a user's own languages, themes, configurations and logs live. It belongs to the command line
// and not to the library for two reasons: nothing about counting depends on it, and the decision
// below is made with 'cfg!(test)', which is only true in the crate that is actually being tested. In
// the library it silently stopped applying the moment the binary became a crate of its own, and the
// tests went back to reading and writing the real directory.
use std::{fs, path::Path, sync::LazyLock};

use directories::ProjectDirs;

// The layout of the application's own directory. None of it is a question about counting, so none of
// it belongs to the library: a caller measuring lines of code has no use for where the logs go.
pub const APP_NAME : &str = "mezura";
pub const LANGUAGES_DIR_NAME : &str = "languages";
pub const THEMES_DIR_NAME : &str = "themes";
pub const CONFIG_DIR_NAME : &str = "config";
pub const LOGS_DIR_NAME : &str = "logs";
pub const DEFAULT_CONFIG_NAME : &str = "default.txt";
pub const MANIFEST_FILE_NAME : &str = "installed.txt";
pub const REPLACED_DIR_NAME : &str = "replaced";

pub static PERSISTENT_APP_PATHS : LazyLock<PersistentAppPaths> = LazyLock::new(PersistentAppPaths::get);

// The repository's own 'test_dir', which only tests read. Anchored on the manifest rather than on the
// executable, so it does not depend on where cargo put the test binary or on the working directory.
#[cfg(test)]
pub mod test_paths {
    pub const TEST_DIR   : &str = concat!(env!("CARGO_MANIFEST_DIR"), "/test_dir/");
    pub const CONFIG_DIR : &str = concat!(env!("CARGO_MANIFEST_DIR"), "/test_dir/config/");
    pub const LOG_DIR    : &str = concat!(env!("CARGO_MANIFEST_DIR"), "/test_dir/logs/");
}

#[derive(Debug)]
pub struct PersistentAppPaths {
    pub data_dir: String,
    pub languages_dir: String,
    pub themes_dir: String,
    pub config_dir: String,
    pub logs_dir: String,
    pub are_initialized: bool
}

// Returns false both when the dir doesn't exist and when it exists but is empty.
pub fn dir_contains_entries(path: &str) -> bool {
    fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_some())
}

impl PersistentAppPaths {
    //Persistent paths:
    // Windows:  C:/Users/<user_name>/AppData/Roaming/mezura
    // Linux:    /home/<user_name>/.local/share/mezura
    // MacOs:    /Users/<user_name>/Library/Application Support/mezura
    pub fn get() -> Self {
        let proj_dirs = ProjectDirs::from("", "",  APP_NAME).unwrap();
        // A test writes real configuration and theme files through these paths, and one that is
        // interrupted before its cleanup leaves them behind. In the real directory that is not
        // litter: the leftovers are loadable configurations that '--show-configs' lists, and
        // 'test_save_load_configs' begins by demanding that its own file is absent, so a single
        // interrupted run makes it fail on every run after it until the file is deleted by hand.
        // Pointing the whole thing at a temporary directory also stops the machine's own default
        // configuration from taking part in the tests, which is what made them differ per machine.
        let data_dir = if cfg!(test) {
            std::env::temp_dir().join(APP_NAME.to_owned() + "-test").to_string_lossy().into_owned() + "/"
        } else {
            // Every path in this struct is a String, so a data directory that is not valid UTF-8
            // cannot be represented at all and nothing below would work. Said out loud rather than
            // left as a bare unwrap, because the message is the only clue anyone would get.
            proj_dirs.data_dir().to_str()
                    .expect("the application data directory path is not valid UTF-8").to_owned() + "/"
        };
        let languages_dir = data_dir.clone() + LANGUAGES_DIR_NAME + "/";
        let config_dir = data_dir.clone() + CONFIG_DIR_NAME + "/";
        let logs_dir = data_dir.clone() + LOGS_DIR_NAME + "/";
        // The existence of the project dir alone means nothing, since any part of the program (or the test
        // suite) that touches these paths can create it. The baked-in data must actually be present, otherwise
        // a half-created dir would be mistaken for a valid installation and every run would fail.
        let are_initialized = dir_contains_entries(&languages_dir) && Path::new(&config_dir).exists()
                && Path::new(&logs_dir).exists();

        PersistentAppPaths {
            themes_dir: data_dir.clone() + THEMES_DIR_NAME + "/",
            data_dir,
            config_dir,
            languages_dir,
            logs_dir,
            are_initialized
        }
    }
}
