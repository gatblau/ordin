//! The Session Supervisor (SessionSupervisor.md): the lifecycle of every run, local or remote. It
//! records each task, starts and stops runs through the Runtime Adapter interface, applies each
//! run's limits (wall clock, request budget, idle timeout), limits how many local sessions run at
//! once, propagates cancellation, records every state change in the journal before emitting it,
//! assembles each run's start-up report, and on application start reconciles the runs the store
//! shows as in progress. In the agent profile it takes runs from the Run Queue with a lease, writes
//! one run summary per stretch, and hands the harness's session record to the Run Queue at a pause.
//!
//! Tables `tasks` and `runs` live in the Journal's datastore, registered as module
//! `session_supervisor` through [`SessionSupervisor::migrations`]. No environment variable and no
//! configuration key is read here: the program passes [`SupervisorConfig`].
//!
//! Confirmation items this crate is conditional on (Playbook §7.8), each built with the spec's
//! fallback:
//! - **C-05** (a Copilot session resumes after a restart): a local session the adapter cannot
//!   re-attach at `recover` is recorded `interrupted`.
//! - **C-29** (the harness reports context-window use): `RunEvent::Usage` carries no
//!   `context_used_percent` yet, so [`SessionSupervisor::context_used_percent`] answers `None`.
//! - **C-74** (credentials stay with the adapters): the supervisor never passes a credential.
//! - **C-80** (a session record moves between machines): the record travels through the Run
//!   Queue; a record taken without it resumes only on the instance that paused it (SA-SUP-02).
#![forbid(unsafe_code)]

mod agent;
mod db;
mod error;
mod hooks;
mod run;
mod store;
mod supervisor;
mod types;
mod validate;

pub use error::{SupervisorError, ALL_CODES};
// `resume_with_decision` takes this type; the Hook Relay may not depend on the Runtime Adapter
// crate (Architecture Test), so the supervisor hands it on (HookRelay.md *Other crates used*).
pub use ordin_runtime_adapter::ResumeDecision;
pub use supervisor::SessionSupervisor;
pub use types::{
    ContentCheck, ContentPoint, ConversationStart, HostError, MetricsSink, ProgramKind,
    RecoveryReport, RunControls, RunView, ShutdownReason, ShutdownReport, StartRequest,
    SupervisorConfig, SupervisorHost,
};
