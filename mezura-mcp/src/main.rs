#![forbid(unsafe_code)]

mod mezura_cli;
mod server;

use std::process::ExitCode;

use rmcp::ServiceExt;
use rmcp::transport::stdio;

// The first stream is the protocol, so nothing of this program's own ever goes to it. A mistake is
// written to the second one, where a client that keeps a log of its servers will show it.
#[tokio::main]
async fn main() -> ExitCode {
    let running = match crate::server::MezuraServer.serve(stdio()).await {
        Ok(running) => running,
        Err(error) => {
            eprintln!("mezura-mcp could not start: {error}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(error) = running.waiting().await {
        eprintln!("mezura-mcp stopped: {error}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
