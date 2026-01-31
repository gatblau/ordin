//! `ordin-hook`: the small program Claude Code runs before every tool call (OrdinHook.md).
//!
//! It fails closed: every failure of its own ends as the harness's deny form and exit status 0
//! (hook protocol), except a bad command line (64), an unwritable standard output (2) and the
//! `mcp-bridge`, `mcp-launch` and `api-key` modes (1).

mod api_key;
mod app;
mod bridge;
mod channel;
mod claude;
mod cli;
mod copilot;
mod env;
mod error;
mod event;
mod launch;
mod mac;

pub use error::ALL_CODES;

#[cfg(test)]
mod tests;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let get = |name: &str| std::env::var(name).ok();
    let io = app::Io {
        stdin: Box::new(std::io::stdin()),
        stdout: Box::new(std::io::stdout()),
        stderr: Box::new(std::io::stderr()),
    };
    let status = app::run(&args, &get, io, &app::Timings::STANDARD);
    std::process::exit(status);
}
