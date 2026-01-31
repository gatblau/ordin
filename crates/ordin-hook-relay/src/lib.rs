//! The Hook Relay (HookRelay.md): the core's end of the ordin hook's local channel (ADR-045). It
//! mints a secret for every harness session, listens on one local channel (a `0600` Unix socket in
//! the application data root, or a per-installation named pipe on Windows), authenticates each hook
//! call by its session's secret, turns it into a tool request for the Rules Engine, and answers
//! allow, deny, or, when the engine asks a person, holds the hook with an interim `holding` line
//! until the person decides or the hook's deadline passes. Past the deadline it answers deny, and
//! when the person decides later it resumes the session through the Session Supervisor. It denies on
//! any error, so the hook fails closed. It also carries the harness's after-tool and stop events,
//! reports each session's hook events to the adapter that started it, and gives the Copilot Local
//! Adapter the same decision path in process (SA-RECON-02).
//!
//! The one table, `hook_late_decisions`, lives in the Journal's datastore, registered as module
//! `hook_relay` through [`HookRelay::migrations`]. The crate reads no environment variable and no
//! configuration key: the program passes [`HookRelayConfig`]. MCP-bridge connections never come
//! here (SA-RECON-01).
//!
//! Confirmation item this crate is conditional on (Playbook §7.8), built with the spec's fallback:
//! **C-78** (the harness payload names the sub-agent): `CallContext.subagent` is the message's
//! `subagent`, which the hook leaves absent until the spike confirms the field, so the Rules
//! Engine lists the reviewer's restriction as not enforced.
#![forbid(unsafe_code)]

/// Writes a log line with the target `ordin_hook_relay` (Observability).
macro_rules! hook_log {
    ($level:ident, $($rest:tt)+) => {
        tracing::$level!(target: "ordin_hook_relay", $($rest)+)
    };
}

mod conn;
mod consts;
mod decide;
mod error;
mod late;
mod listen;
mod ports;
mod relay;
mod session;
mod store;
mod types;
mod writes;

#[cfg(test)]
extern crate self as ordin_hook_relay;

#[cfg(test)]
mod tests;

pub use error::{HookRelayError, ALL_CODES};
pub use relay::HookRelay;
pub use types::{HookRelayConfig, HookSessionEnv, HookSessionEvent, HookSessionSpec};
pub use writes::{AgentWriteError, AgentWrites, NoPlaces, WriteTarget};
