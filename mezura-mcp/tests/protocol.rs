use std::io::Write;
use std::process::{Command, Stdio};

use rmcp::serde_json::{self, Value};

const INITIALIZE : &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"the test","version":"1"}}}"#;
const INITIALIZED : &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
const LIST_THE_TOOLS : &str = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;

// A client asks for the tools once, on connecting, and never asks again, so a server that answers
// this wrongly is one that offers nothing for the rest of the session. Nothing else here can see
// it: the tools are called directly by the tests inside the crate, which is the half of the wiring
// that does not go through the protocol at all.
#[test]
fn a_client_connecting_is_told_who_this_is_and_what_it_can_ask_for() {
    let answers = speak_to_the_server("connecting", &[INITIALIZE, INITIALIZED, LIST_THE_TOOLS]);

    let introduction = &answers[0]["result"];
    assert_eq!("mezura-mcp", introduction["serverInfo"]["name"],
            "the server introduces itself as something else, which is what rmcp's own \
             'from_build_env' does");
    assert!(introduction["instructions"].as_str().is_some_and(|x| x.contains("mezura counts")),
            "the instructions a client shows on connecting are missing");
    assert!(!introduction["capabilities"]["tools"].is_null(), "the server does not offer tools at all");

    let offered = introduction_of_each_tool(&answers[1]);
    assert_eq!(vec!["count_lines_of_code", "count_lines_of_code_as_json", "explain_file"], offered);
}

#[test]
fn a_tool_called_over_the_protocol_answers_with_the_report() {
    let call = format!(r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"count_lines_of_code",
            "arguments":{{"path":{:?},"top":3}}}}}}"#,
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").to_string_lossy());
    let answers = speak_to_the_server("a-call", &[INITIALIZE, INITIALIZED, &call.replace('\n', "")]);

    let result = &answers[1]["result"];
    assert_eq!(Some(false), result["isError"].as_bool(), "the call came back as a failure: {result}");
    let text = result["content"][0]["text"].as_str().expect("the answer carries no text");
    assert!(text.contains("Rust"), "the report does not name the language of the files in it:\n{text}");
}

#[test]
fn a_tool_asked_for_a_path_that_is_not_there_says_so_instead_of_dying() {
    let call = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"count_lines_of_code","arguments":{"path":"no/such/place"}}}"#;
    let answers = speak_to_the_server("no-such-path", &[INITIALIZE, INITIALIZED, call]);

    let result = &answers[1]["result"];
    assert_eq!(Some(true), result["isError"].as_bool(), "a path that does not exist was not reported as a mistake: {result}");
    let text = result["content"][0]["text"].as_str().expect("the answer carries no text");
    assert!(text.contains("does not exist"), "the answer does not say what went wrong:\n{text}");
}

// Every line of the answer, parsed, in the order it arrived. A notification is not answered, so
// there are fewer of these than there were requests.
fn speak_to_the_server(named_after_the_test: &str, requests: &[&str]) -> Vec<Value> {
    // Inherited by the mezura the server starts, so the run writes its languages and themes into a
    // directory of this test's own and not into the one belonging to whoever is running the suite.
    let data_dir = std::env::temp_dir().join("mezura-mcp-test").join(named_after_the_test);
    let _ = std::fs::remove_dir_all(&data_dir);

    let mut server = Command::new(env!("CARGO_BIN_EXE_mezura-mcp"))
            .env("MEZURA_DATA_DIR", &data_dir)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .spawn().expect("the server would not start");

    let mut asking = server.stdin.take().expect("the server has no input to write to");
    for request in requests {
        writeln!(asking, "{request}").expect("the server stopped reading");
    }
    // The server runs until its input ends, so this is what makes the read below finish.
    drop(asking);

    let finished = server.wait_with_output().expect("the server could not be waited for");
    let _ = std::fs::remove_dir_all(&data_dir);
    let complaints = String::from_utf8_lossy(&finished.stderr);
    assert!(finished.status.success(), "the server ended badly: {complaints}");

    String::from_utf8_lossy(&finished.stdout).lines().filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).unwrap_or_else(|error|
                    panic!("the server wrote something that is not a message: {error}\n{line}")))
            .collect()
}

fn introduction_of_each_tool(listing: &Value) -> Vec<&str> {
    let tools = listing["result"]["tools"].as_array().expect("the listing carries no tools");
    for tool in tools {
        let name = tool["name"].as_str().unwrap_or("a tool with no name");
        assert!(tool["description"].as_str().is_some_and(|x| x.len() > 100),
                "'{name}' is offered without the text that decides whether it is ever called");
        assert!(!tool["inputSchema"]["properties"]["path"].is_null(),
                "'{name}' does not ask for a path, so nothing can be counted with it");
    }

    tools.iter().map(|tool| tool["name"].as_str().unwrap_or_default()).collect()
}
