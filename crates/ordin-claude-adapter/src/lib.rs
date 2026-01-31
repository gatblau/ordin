//! The Claude Local Adapter (ClaudeLocalAdapter.md): one local agent session on the Claude Code CLI,
//! presented to the core as a `RuntimeAdapter` (09 ADR-038, ADR-044, ADR-045). For each run it checks
//! the harness's version and sign-in, starts one CLI process in headless streamed-JSON mode with the
//! agent folder as its working folder, registers the MCP Front's servers and the ordin hook for that
//! session only, passes the instructions and plugin the Claude Code Installer produced, and turns the
//! CLI's output and the Hook Relay's session events into the ordered `RunEvent` stream the Session
//! Supervisor consumes.
//!
//! Each run has one task, which owns the child process and takes three inputs: the CLI's stdout
//! lines, the Hook Relay's events and the verb calls. `CLA_` codes are internal, journalled and
//! logged; at the trait boundary every failure is a 2C `AdapterError`. The crate owns no table and
//! reads no configuration key: the program passes [`ClaudeAdapterConfig`].
//!
//! Confirmation items this crate is conditional on (Playbook §7.8), each built with the spec's
//! wording:
//! - **C-20** (with shell, code and web tools off, the remaining built-in calls reach the hook): the
//!   hook is registered for every tool (`matcher: "*"`); the harness job's behavioural check runs it.
//! - **C-25** (`claude auth login` needs no terminal): started with no standard input and in its own
//!   process group.
//! - **C-26** (passed): `--restricted` with everything passed by flag or per-session file.
//! - **C-37** (model and thinking level per session; usage reported): `--model` and `--effort` when
//!   the setup names them; where `result` has no usage the token fields stay `None`.
//! - **C-41** (to-do lists in the stream): `HarnessPlan` where an `assistant` line carries a
//!   `TodoWrite` input; none otherwise. At Claude Code 2.1.316 no such line arrives.
//! - **C-61** (settled, result b): `--append-system-prompt-file` and `--plugin-dir`.
//! - **C-74**: [`KeyRoute`] carries both routes; the program chooses.
//! - **C-77** (settled): `--fallback-model`, `ANTHROPIC_BASE_URL` and the declared settings.
//! - **C-79** (file-writing tools off): `--tools Read`, `--disallowedTools Write,Edit`.
//! - **C-80** (a session record moves between machines): where the record cannot be read or stored,
//!   the run ends `Interrupted` and is not started again.
// Denied everywhere but `job`, whose Windows calls hold a session's process tree (CLA-JOB-1).
#![deny(unsafe_code)]

mod adapter;
mod cli;
mod command;
mod config;
mod env;
mod error;
mod files;
#[cfg(windows)]
mod job;
mod models;
mod ports;
mod process;
mod progress;
mod run;
mod session;
mod stream;
mod task;

#[cfg(all(test, unix))]
mod tests;
#[cfg(all(test, windows))]
mod windows_tests;

pub use adapter::ClaudeLocalAdapter;
pub use command::DESKTOP_BUILT_IN_TOOLS;
pub use config::{
    ClaudeAdapterConfig, ClaudeEffort, KeyRoute, PackageMcpServer, PausedRun, RecordError,
    SessionRecordStore, SessionSetup, SessionSetupError, SessionSetupSource, SignInMode,
    SignInStatus,
};
pub use error::{ClaudeAdapterError, ALL_CODES};
