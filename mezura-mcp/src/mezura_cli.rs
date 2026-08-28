use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

pub const BY_FILE   : &str = "--by-file";
pub const COUNTING  : &str = "--counting";
pub const EXCLUDE   : &str = "--exclude";
pub const EXPLAIN   : &str = "--explain";
pub const HIDE      : &str = "--hide";
pub const LANGUAGES : &str = "--languages";
pub const OUTPUT    : &str = "--output";
pub const TOP       : &str = "--top";

pub const BINARY_PATH_VARIABLE : &str = "MEZURA_BIN";

// mezura's own, read by the binary this server starts, and named here only so that a test can hand
// the run a data directory of its own.
pub const DATA_DIR_VARIABLE : &str = "MEZURA_DATA_DIR";

const BINARY_NAME : &str = "mezura";

// The report this server parses, and the language files it shares a data directory with, are the
// shape mezura 3 writes. An older binary on the path answers with its own and nothing here makes
// sense, so it is refused by name rather than left to fail as a parse.
const EXPECTED_MAJOR : &str = "3";
const TIME_LIMIT  : Duration = Duration::from_secs(180);

// The two streams stay apart: a JSON document is written to the first and has to reach the caller
// with nothing added to it.
pub struct Output {
    pub text: String,
    pub warnings: String,
}

// Never fails: a name with nothing behind it is left to the spawn below, which is where the mistake
// can be reported with the path that was tried in it.
pub fn find_binary() -> PathBuf {
    if let Some(given) = std::env::var_os(BINARY_PATH_VARIABLE).filter(|x| !x.is_empty()) {
        return PathBuf::from(given);
    }

    if let Ok(server) = std::env::current_exe()
            && let Some(directory) = server.parent() {
        let beside_this_server = directory.join(BINARY_NAME).with_extension(std::env::consts::EXE_EXTENSION);
        if beside_this_server.is_file() {
            return beside_this_server;
        }
    }

    PathBuf::from(BINARY_NAME)
}

pub async fn run(arguments: &[String]) -> Result<Output, String> {
    run_the_binary(&find_binary(), None, arguments).await
}

// Asked once for the life of the server: the answer cannot change under a running process, and a
// spawn per tool call to find that out would cost more than the call itself.
fn refuse_a_mezura_of_another_age(binary: &Path) -> Result<(), String> {
    static CHECKED : std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    CHECKED.get_or_init(|| {
        let spoken = std::process::Command::new(binary).arg("--version").output();
        let Ok(spoken) = spoken else { return Ok(()) };
        let text = String::from_utf8_lossy(&spoken.stdout);
        let Some(version) = text.lines().map(str::trim).find(|line| !line.is_empty()) else {
            return Ok(())
        };
        let major = version.trim_start_matches('v').split('.').next().unwrap_or_default();
        if major == EXPECTED_MAJOR {
            return Ok(());
        }
        Err(format!("'{}' is mezura {version}, and this server speaks to mezura {EXPECTED_MAJOR}. \
Point {BINARY_PATH_VARIABLE} at a mezura {EXPECTED_MAJOR} binary, or update the one that is \
installed.", binary.display()))
    }).clone()
}

// Both of the first two are arguments so that a test can measure the binary it just built rather
// than whichever one this machine has installed, and can point it at a data directory of its own
// rather than writing languages and themes into the real one.
pub async fn run_the_binary(binary: &Path, data_dir: Option<&Path>, arguments: &[String])
        -> Result<Output, String> {
    refuse_a_mezura_of_another_age(binary)?;

    // The client's own environment is inherited, and one variable in it decides whether mezura
    // paints. 'CLICOLOR_FORCE' makes it paint into a pipe, which fills the answer with escape
    // codes, and it outranks 'NO_COLOR', so the variable has to go rather than be answered.
    let mut command = tokio::process::Command::new(binary);
    command.args(arguments).env_remove("CLICOLOR_FORCE").env("NO_COLOR", "1")
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .kill_on_drop(true);
    if let Some(data_dir) = data_dir {
        command.env(DATA_DIR_VARIABLE, data_dir);
    }
    let started = command.spawn();

    let child = match started {
        Ok(child) => child,
        Err(error) => return Err(format!(
                "mezura could not be started from '{}': {error}.\nInstall it with \
'cargo install --locked --git https://github.com/subamanis/mezura mezura', or set the environment \
variable {BINARY_PATH_VARIABLE} to the path of the binary.", binary.display()))
    };

    let finished = match tokio::time::timeout(TIME_LIMIT, child.wait_with_output()).await {
        Ok(Ok(finished)) => finished,
        Ok(Err(error)) => return Err(format!("mezura was started and then could not be read: {error}")),
        Err(_) => return Err(format!("mezura was still running after {} seconds and was stopped. \
Ask for a smaller part of the tree.", TIME_LIMIT.as_secs()))
    };

    let text = String::from_utf8_lossy(&finished.stdout).trim().to_owned();
    let warnings = String::from_utf8_lossy(&finished.stderr).trim().to_owned();
    if finished.status.success() {
        return Ok(Output {text, warnings});
    }

    // Everything mezura refuses is written to the second stream, so an empty one here means it died
    // without saying why and the report is all there is to hand back.
    Err(if warnings.is_empty() {format!("mezura failed without a message.\n{text}")} else {warnings})
}
