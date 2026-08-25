use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::mezura_cli;

// The version line and the elapsed time say nothing to a caller with no terminal to look at, the
// parsing lines are two words of progress, and the history is a comparison against runs somebody
// made by hand on this machine, which is not what was asked for.
const REPORT_HIDES : &str = "version,timing,parsing-info,history";

// How much text one call may hand back. Measured: the report of a tree of 135,000 files is 16 KB
// without file rows and 20 MB with a row for every file, and one 3,795 line source file explained
// whole is 332 KB. Nothing above this is a thing to read, it is rows nobody asked to see one by
// one, so the answer is refused with what to ask instead rather than cut into a half answer.
const SIZE_LIMIT : usize = 100 * 1024;

// One row per file is about 150 bytes, so fifty of them under each of a dozen languages is already
// most of the limit above.
const MOST_FILE_ROWS : u32 = 50;

// What each tool is for is in 'descriptions/', one file per tool, because it is the text a model
// reads to decide whether to call the tool at all and it sits in the context of every message of
// every session whether it is called or not.
const INSTRUCTIONS : &str = include_str!("../descriptions/instructions.txt");

// Nothing of a run is kept between calls: every tool starts the binary again, which is what keeps
// one call's theme, warnings and temporary checkouts out of the next one's answer.
#[derive(Debug, Clone, Default)]
pub struct MezuraServer;

#[tool_router]
impl MezuraServer {
    #[doc = include_str!("../descriptions/count_lines_of_code.txt")]
    #[tool]
    pub async fn count_lines_of_code(&self, parameters: Parameters<CountArguments>) -> Result<String, String> {
        let finished = mezura_cli::run(&parameters.0.to_report_command_line()?).await?;
        refuse_if_too_long(join_the_warnings_to(finished),
                "Ask for a part of the tree instead, or leave 'by_file' out.")
    }

    #[doc = include_str!("../descriptions/count_lines_of_code_as_json.txt")]
    #[tool]
    pub async fn count_lines_of_code_as_json(&self, parameters: Parameters<CountArguments>) -> Result<String, String> {
        // The document and nothing else, since whatever asked for JSON is going to parse it. A
        // warning of the run is written to the other stream and is dropped here on purpose.
        refuse_if_too_long(mezura_cli::run(&parameters.0.to_document_command_line()?).await?.text,
                "Ask for a part of the tree instead, or leave 'by_file' out.")
    }

    #[doc = include_str!("../descriptions/explain_file.txt")]
    #[tool]
    pub async fn explain_file(&self, parameters: Parameters<ExplainArguments>) -> Result<String, String> {
        let finished = mezura_cli::run(&parameters.0.to_command_line()?).await?;
        refuse_if_too_long(join_the_warnings_to(finished),
                "Ask for the lines that matter with 'first_line' and 'last_line'.")
    }
}

#[tool_handler]
impl ServerHandler for MezuraServer {
    fn get_info(&self) -> ServerInfo {
        // Named here and not through rmcp's 'from_build_env', which reads the build environment of
        // rmcp itself and introduces this server to every client as 'rmcp 3.1.4'.
        let this_server = Implementation::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));

        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
                .with_server_info(this_server)
                .with_instructions(INSTRUCTIONS)
    }
}

// An argument nobody declared is refused rather than dropped: serde ignores what it does not know,
// so a misnamed 'exclude_dirs' would leave the run counting everything and the answer would look
// perfectly ordinary. The same attribute closes the published schema with 'additionalProperties'.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CountArguments {
    #[schemars(description = "The directory or file to count. An absolute path is safest; a \
relative one is taken from the directory this server was started in.")]
    pub path: String,
    #[schemars(description = "Paths to leave out, as glob patterns. A pattern with no slash in it \
leaves out a file or directory of that name at any depth ('node_modules', '*.min.js'); a pattern \
with slashes matches the end of the whole path ('src/generated').")]
    pub exclude: Option<Vec<String>>,
    #[schemars(description = "Count only these languages and leave every other one out, named \
either by language ('rust', 'c++') or by any extension they claim ('js' names JavaScript). Leave \
it out to count everything.")]
    pub languages: Option<Vec<String>>,
    #[schemars(description = "Show only this many languages, the largest first, with a line \
underneath saying how many were left out. 0 shows every language, and so does leaving it out.")]
    pub top: Option<u32>,
    #[schemars(description = "Also give files a row of their own, this many under each language, \
the largest first, at most 50. Leave it out for no file rows at all.")]
    #[schemars(range(max = 50))]
    pub by_file: Option<u32>,
    #[schemars(description = "What a line of code is. 'content', the default, counts a line by the \
words on it, so a line holding nothing but '}' is neither code nor comment. 'region' counts a line \
by where it sits, the way cloc, tokei and scc do; use it when the numbers are going to be compared \
against one of those.")]
    pub counting: Option<Counting>,
}

impl CountArguments {
    fn to_report_command_line(&self) -> Result<Vec<String>, String> {
        let mut arguments = self.to_command_line()?;
        arguments.push(mezura_cli::HIDE.to_owned());
        arguments.push(REPORT_HIDES.to_owned());

        Ok(arguments)
    }

    fn to_document_command_line(&self) -> Result<Vec<String>, String> {
        let mut arguments = self.to_command_line()?;
        arguments.push(mezura_cli::OUTPUT.to_owned());
        arguments.push("json".to_owned());

        Ok(arguments)
    }

    fn to_command_line(&self) -> Result<Vec<String>, String> {
        let mut arguments = vec![as_a_path(&self.path)?];

        if let Some(exclude) = self.exclude.as_ref().filter(|x| !x.is_empty()) {
            arguments.push(mezura_cli::EXCLUDE.to_owned());
            arguments.push(as_a_list_of_paths(exclude)?);
        }
        if let Some(languages) = self.languages.as_ref().filter(|x| !x.is_empty()) {
            arguments.push(mezura_cli::LANGUAGES.to_owned());
            arguments.push(as_a_list_of_names(languages)?);
        }
        // mezura wants 1 or greater and answers 0 with the whole of its own help, while the '0
        // means all of them' of '--by-file' sits in the next field of this same struct. Dropping
        // the command instead lets the two zeros mean the same thing.
        if let Some(top) = self.top.filter(|x| *x > 0) {
            arguments.push(mezura_cli::TOP.to_owned());
            arguments.push(top.to_string());
        }
        // The schema says the same thing, and a schema is what a model reads rather than what a
        // client is held to, so the number is checked here as well as declared there.
        if let Some(by_file) = self.by_file {
            if by_file > MOST_FILE_ROWS {
                return Err(format!("'by_file' was {by_file}, and at most {MOST_FILE_ROWS} files can \
be listed under each language. The report would be too long to read."));
            }
            arguments.push(mezura_cli::BY_FILE.to_owned());
            arguments.push(by_file.to_string());
        }
        if let Some(counting) = self.counting {
            arguments.push(mezura_cli::COUNTING.to_owned());
            arguments.push(counting.to_written_form().to_owned());
        }

        Ok(arguments)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExplainArguments {
    #[schemars(description = "The one file to go through, line by line. An absolute path is \
safest; a relative one is taken from the directory this server was started in.")]
    pub path: String,
    #[schemars(description = "The first line to print. The whole file is still read, so a comment \
or a string that opened above this line is named on every line that carries it. Leave it out to \
start at the top.")]
    pub first_line: Option<u32>,
    #[schemars(description = "The last line to print. Past the end of the file is not a mistake. \
Leave it out to go to the end, and leave both out for the whole file, which is a lot of text for a \
long one.")]
    pub last_line: Option<u32>,
    #[schemars(description = "What a line of code is. 'content', the default, counts a line by the \
words on it, so a line holding nothing but '}' is neither code nor comment. 'region' counts a line \
by where it sits, the way cloc, tokei and scc do.")]
    pub counting: Option<Counting>,
}

impl ExplainArguments {
    fn to_command_line(&self) -> Result<Vec<String>, String> {
        let mut arguments = vec![as_a_path(&self.path)?, mezura_cli::EXPLAIN.to_owned()];
        if let Some(lines) = self.to_written_range()? {
            arguments.push(lines);
        }
        arguments.push(mezura_cli::HIDE.to_owned());
        arguments.push("version".to_owned());
        if let Some(counting) = self.counting {
            arguments.push(mezura_cli::COUNTING.to_owned());
            arguments.push(counting.to_written_form().to_owned());
        }

        Ok(arguments)
    }

    // 'None' is the whole file. mezura writes a range as '10..40' and leaves either end off to mean
    // the start or the end of the file, and reads a last line past the end as the end of it.
    fn to_written_range(&self) -> Result<Option<String>, String> {
        if self.first_line.is_none() && self.last_line.is_none() {
            return Ok(None);
        }
        if self.first_line == Some(0) {
            return Err("'first_line' is 0, and the first line of a file is line 1".to_owned());
        }
        if let (Some(first), Some(last)) = (self.first_line, self.last_line)
                && last < first {
            return Err(format!("'last_line' is {last} and 'first_line' is {first}, so there is \
nothing between them"));
        }

        let written = |line: Option<u32>| line.map(|x| x.to_string()).unwrap_or_default();
        Ok(Some(format!("{}..{}", written(self.first_line), written(self.last_line))))
    }
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Counting {
    Content,
    Region,
}

impl Counting {
    fn to_written_form(self) -> &'static str {
        match self {
            Counting::Content => "content",
            Counting::Region => "region",
        }
    }
}

// mezura writes what it could not do to the second stream, and a run that ends well can still have
// something there: a language named in the request that no file matched, a language file this build
// could not read. Dropping it would answer with numbers that are quietly for something else.
// Refused and not cut short, because none of the three answers survives being cut: the report's
// total and percentages are printed under the rows, a cut document does not parse, and an
// explanation that stops in the middle looks like the file stops there.
fn refuse_if_too_long(answer: String, advice: &str) -> Result<String, String> {
    if answer.len() <= SIZE_LIMIT {
        return Ok(answer);
    }

    Err(format!("The answer is {} KB, which is more than can usefully be read at once. {advice}",
            answer.len() / 1024))
}

fn join_the_warnings_to(finished: mezura_cli::Output) -> String {
    if finished.warnings.is_empty() {
        return finished.text;
    }

    format!("{}\n\nmezura also reported:\n{}", finished.text, finished.warnings)
}

// Nothing is quoted, and quoting was tried: mezura joins its arguments back into one line and reads
// the target as everything before the first command, so a space inside a path arrives whole on its
// own. A comma is the one character that cannot arrive at all, since it separates one path from the
// next wherever it appears, and saying so beats handing over half a path.
fn as_a_path(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("an empty path was given, so there is nothing to count".to_owned());
    }
    if value.contains(',') {
        return Err(format!("'{value}' holds a comma, which mezura reads as the end of one path and \
the start of the next, so this path cannot be given to it"));
    }
    if is_read_as_a_command(value) {
        return Err(format_refusal_of_a_command(value));
    }

    Ok(value.to_owned())
}

fn as_a_list_of_paths(values: &[String]) -> Result<String, String> {
    let paths = values.iter().map(|x| as_a_path(x)).collect::<Result<Vec<_>, _>>()?;
    Ok(paths.join(","))
}

// A name may hold a space, since the list is only ever split on commas, and it may not hold a comma
// for the same reason.
fn as_a_list_of_names(values: &[String]) -> Result<String, String> {
    let mut names = Vec::with_capacity(values.len());
    for value in values {
        let name = value.trim();
        if name.is_empty() {
            return Err("an empty language name was given".to_owned());
        }
        if name.contains(',') {
            return Err(format!("'{name}' holds a comma, so it is two language names and not one"));
        }
        if is_read_as_a_command(name) {
            return Err(format_refusal_of_a_command(name));
        }
        names.push(name);
    }

    Ok(names.join(","))
}

// mezura is given its arguments and joins them back into one line, where a '--' at the start or
// after a space begins a command. So a value carrying one is not a value: '--restore' rewrites the
// data directory, and 'src --output json' turns the report into a document. The rule is the one
// mezura's own reader uses, in 'args::split_into_command_segments'.
fn is_read_as_a_command(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.starts_with(b"--") || bytes.windows(3)
            .any(|window| window[0].is_ascii_whitespace() && window[1] == b'-' && window[2] == b'-')
}

fn format_refusal_of_a_command(value: &str) -> String {
    format!("'{value}' holds '--', which mezura reads as the start of a command rather than as part \
of a value, so it cannot be given")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Counting, CountArguments, ExplainArguments};
    use crate::mezura_cli;

    #[test]
    fn every_option_reaches_the_command_line_in_the_form_mezura_reads() {
        let arguments = every_option_set().to_command_line().unwrap();

        assert_eq!(vec!["src".to_owned(), "--exclude".to_owned(), "*.txt,vendor".to_owned(),
                "--languages".to_owned(), "rust,visual basic".to_owned(), "--top".to_owned(), "5".to_owned(),
                "--by-file".to_owned(), "3".to_owned(), "--counting".to_owned(), "region".to_owned()], arguments);
    }

    // A comma inside one of them would be handed over as two paths, and the run would count
    // something that was never asked for or fail over a path nobody wrote.
    #[test]
    fn a_value_holding_a_comma_is_refused_before_the_binary_is_started() {
        let path = CountArguments {path: "src/a,b".to_owned(), ..every_option_set()};
        assert!(path.to_command_line().unwrap_err().contains("holds a comma"));

        let pattern = CountArguments {exclude: Some(vec!["*.{js,ts}".to_owned()]), ..every_option_set()};
        assert!(pattern.to_command_line().unwrap_err().contains("holds a comma"));

        let language = CountArguments {languages: Some(vec!["rust,python".to_owned()]), ..every_option_set()};
        assert!(language.to_command_line().unwrap_err().contains("two language names"));

        let empty = CountArguments {path: "   ".to_owned(), ..every_option_set()};
        assert!(empty.to_command_line().unwrap_err().contains("nothing to count"));
    }

    // Without this the whole tool is a way of running mezura with any command at all: '--restore'
    // rewrites the data directory, '--save-theme' writes a file, and 'src --output json' turns the
    // report tool into the document one, all of which were reachable and are now refused.
    #[test]
    fn a_value_carrying_a_command_is_refused_before_the_binary_is_started() {
        for value in ["--restore", "./src --output json", "./src\t--diff HEAD~1"] {
            let arguments = CountArguments {path: value.to_owned(), ..every_option_set()};
            assert!(arguments.to_command_line().unwrap_err().contains("holds '--'"),
                    "'{value}' was handed over as a path");
        }

        let pattern = CountArguments {exclude: Some(vec!["x --no-gitignore".to_owned()]), ..every_option_set()};
        assert!(pattern.to_command_line().unwrap_err().contains("holds '--'"));

        let language = CountArguments {languages: Some(vec!["rust --restore".to_owned()]), ..every_option_set()};
        assert!(language.to_command_line().unwrap_err().contains("holds '--'"));

        // A path is allowed to have two dashes inside a name, since only a '--' opening a word is
        // a command, and refusing more than mezura does would be refusing real directories
        let ordinary = CountArguments {path: "./my--dir".to_owned(), ..every_option_set()};
        assert!(ordinary.to_command_line().is_ok());
    }

    // '--top 0' is refused by mezura with twenty five lines of its own help, while '--by-file 0'
    // means every file, and the two sit next to each other in the schema a model reads.
    #[test]
    fn a_top_of_zero_leaves_the_command_off_instead_of_failing() {
        let arguments = CountArguments {top: Some(0), ..every_option_set()}.to_command_line().unwrap();

        assert!(!arguments.contains(&"--top".to_owned()), "'--top 0' was handed over: {arguments:?}");
        assert!(arguments.contains(&"--by-file".to_owned()), "the rest of the command line was lost with it");
    }

    // The whole command line against the real binary, and not a list of names against '--help':
    // this is the only thing that says a renamed command, a renamed value and a quotation rule that
    // stopped holding are all still what this server writes.
    #[tokio::test]
    async fn a_call_with_every_option_set_is_a_command_line_mezura_accepts() {
        let mut arguments = every_option_set();
        arguments.path = source_directory().to_string_lossy().into_owned();

        let report = run("every-option", &arguments.to_report_command_line().unwrap()).await;
        assert!(report.contains("Rust"), "the report does not name the language of the files in it:\n{report}");
        assert!(report.contains("server.rs"), "'--by-file' gave no file rows:\n{report}");
    }

    #[tokio::test]
    async fn the_json_tool_writes_a_document_and_nothing_else() {
        let mut arguments = every_option_set();
        arguments.path = source_directory().to_string_lossy().into_owned();

        let document = run("json", &arguments.to_document_command_line().unwrap()).await;
        let parsed = rmcp::serde_json::from_str::<rmcp::serde_json::Value>(&document)
                .unwrap_or_else(|error| panic!("the document does not parse: {error}\n{document}"));
        assert_eq!("run", parsed["kind"]);
    }

    #[tokio::test]
    async fn explain_goes_through_one_file_line_by_line() {
        let arguments = ExplainArguments {
            path: source_directory().join("main.rs").to_string_lossy().into_owned(),
            first_line: None, last_line: None, counting: Some(Counting::Content)
        };

        let explanation = run("explain", &arguments.to_command_line().unwrap()).await;
        assert!(explanation.contains("main.rs as Rust"), "the file was not read as Rust:\n{explanation}");
        assert!(explanation.contains("mod server;"), "the lines of the file are not in the answer:\n{explanation}");
    }

    // The range is why the whole file no longer has to be handed over, so a run that quietly
    // ignored it would be a tool that cannot answer about a long file at all.
    #[tokio::test]
    async fn a_range_of_lines_is_the_only_part_of_the_file_that_comes_back() {
        let arguments = ExplainArguments {
            path: source_directory().join("main.rs").to_string_lossy().into_owned(),
            first_line: Some(3), last_line: Some(4), counting: None
        };

        let explanation = run("explain-range", &arguments.to_command_line().unwrap()).await;
        assert!(explanation.contains("mod mezura_cli;"), "line 3 is not in the answer:\n{explanation}");
        assert!(!explanation.contains("ExitCode::SUCCESS"), "the whole file came back:\n{explanation}");
        assert!(explanation.contains("2 lines shown"), "the two totals are not both there:\n{explanation}");
    }

    // The arguments are joined back into one line before they are read, so a spaced path is where
    // this would break if the target ever started being split on whitespace.
    #[tokio::test]
    async fn a_path_with_a_space_in_it_reaches_the_binary_whole() {
        let directory = std::env::temp_dir().join("mezura mcp spaced test");
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("only.rs"), "fn main() {}\n").unwrap();

        let arguments = CountArguments {path: directory.to_string_lossy().into_owned(), exclude: None,
                languages: None, top: None, by_file: None, counting: None};
        let report = run("spaced", &arguments.to_report_command_line().unwrap()).await;
        let _ = std::fs::remove_dir_all(&directory);

        assert!(report.contains("Rust"), "the spaced path was not counted:\n{report}");
    }

    fn every_option_set() -> CountArguments {
        CountArguments {
            path: "src".to_owned(),
            exclude: Some(vec!["*.txt".to_owned(), "vendor".to_owned()]),
            languages: Some(vec!["rust".to_owned(), "visual basic".to_owned()]),
            top: Some(5),
            by_file: Some(3),
            counting: Some(Counting::Region)
        }
    }

    fn source_directory() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
    }

    // A data directory of its own per test, built from nothing by the run itself, since the binary
    // is not compiled for testing and would otherwise write its languages and themes into the real
    // one. It is also the only place the first-installation path is exercised through the binary.
    async fn run(named_after_the_test: &str, arguments: &[String]) -> String {
        let data_dir = std::env::temp_dir().join("mezura-mcp-test").join(named_after_the_test);
        let _ = std::fs::remove_dir_all(&data_dir);

        let finished = mezura_cli::run_the_binary(&find_the_binary_that_was_built(), Some(&data_dir),
                arguments).await;
        let answer = finished.unwrap_or_else(|error|
                panic!("mezura refused '{}':\n{error}", arguments.join(" ")));
        // Without this the test passes just as well against a mezura that ignores the variable, and
        // the whole suite goes back to writing into the real data directory with nothing saying so.
        let languages = data_dir.join("languages");
        let was_built = languages.is_dir();
        let _ = std::fs::remove_dir_all(&data_dir);
        assert!(was_built, "no data directory was built at '{}', so the run used another one",
                data_dir.display());

        answer.text
    }

    // Not 'find_binary', which answers for an installed server and would measure whichever mezura
    // this machine has on its path. Both profiles are searched and the newest wins, because
    // 'cargo test' does not build the plain binary of another package at all: it is left by
    // whichever 'cargo build' ran last, and on a CI runner that is the release one.
    fn find_the_binary_that_was_built() -> PathBuf {
        let start = std::env::current_exe().expect("the test binary has no path of its own");
        let name = PathBuf::from("mezura").with_extension(std::env::consts::EXE_EXTENSION);

        let built = start.ancestors().skip(1)
                .flat_map(|directory| [directory.join(&name), directory.join("debug").join(&name),
                        directory.join("release").join(&name)])
                .filter(|candidate| candidate.is_file())
                .max_by_key(|candidate| candidate.metadata().and_then(|about| about.modified()).ok());

        built.expect("mezura has not been built, so there is nothing for this server to run. \
                'cargo test' never builds the binary of another package, so build it first: \
                'cargo build' or 'cargo build --release'.")
    }
}
