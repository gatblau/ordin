//! The Copilot Local Adapter (CopilotLocalAdapter.md): one local agent session on the person's own
//! Copilot CLI (ADR-123), driven over its own JSON-RPC and presented to the core
//! as a `RuntimeAdapter` (09 ADR-038, ADR-044, ADR-045). For each run it asks the program for the
//! CLI, starts one CLI server with the agent folder as its working folder (A-11), checks the CLI's
//! own sign-in, registers the MCP Front's servers for the session, sends every tool request to the
//! Hook Relay's decision path in process before it executes, and turns the session's events into the
//! `RunEvent` stream the Session Supervisor consumes.
//!
//! The logic sits behind [`CopilotPort`]. [`CliCopilotPort`] binds it to the CLI itself: ordin
//! starts the CLI and speaks its JSON-RPC on the standard streams, as the Claude Local Adapter owns
//! Claude Code's stream (user decision 2026-02-03, in place of `github-copilot-sdk`). The scripted
//! double of feature `testkit` tests every rule without the CLI.
//! `CPA_` codes are internal, journalled and logged; at the trait boundary every failure is a 2C
//! `AdapterError`. The crate owns no table.
//!
//! Confirmation items this crate is conditional on, each built with the spec's wording:
//! - **C-01, C-60**: a command is sent as `/<name> <arguments>`.
//! - **C-02, C-27**: every tool request reaches the permission handler or the pre-tool-use hook.
//! - **C-03**: one CLI server per session either way.
//! - **C-05**: a persisted session that cannot be resumed ends `Interrupted`.
//! - **C-25** (Copilot half): `copilot login` started with no terminal; the poll runs either way.
//! - **C-37**: token fields stay `None` where Copilot reports none.
//! - **C-41**: no CLI event is known to carry a to-do list, so none is mapped yet; the adapter turns
//!   the port's `TodoList` into a `HarnessPlan`.
#![deny(unsafe_code)]

mod adapter;
mod cli;
mod config;
mod env;
mod error;
mod gate;
mod models;
mod port;
mod ports;
mod progress;
mod run;
mod session;
mod task;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;

#[cfg(test)]
mod tests;

pub use adapter::CopilotLocalAdapter;
pub use cli::CliCopilotPort;
pub use config::{
    CopilotAdapterConfig, CopilotCli, CopilotCliSource, CopilotSessionSetup, PackageMcpServer,
    SessionSetupError, SessionSetupSource, C48_SERVER_SIGN_IN_CONFIRMED,
};
pub use error::{CopilotAdapterError, ALL_CODES};
pub use port::{
    CliServerSpec, CopilotAuthStatus, CopilotEvent, CopilotPort, CopilotServer, GateAnswer,
    HostTool, HostToolAnswer, HostToolHandler, PortError, SessionOptions, ToolGate, ToolObserver,
};
