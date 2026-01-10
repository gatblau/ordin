//! The contract between the modules that run agents and the four adapters that reach a runtime
//! (RuntimeAdapterInterface.md; ADR-003, ADR-100 item 2).
//!
//! The crate is the `RuntimeAdapter` trait in ordin's caller vocabulary, the run event types, the
//! adapter error, the level-to-model contract (`HarnessModels`), the `AdapterRegistry` the
//! programs hand to the Session Supervisor, and the checks every adapter's output passes before
//! the Supervisor acts on it: `check_start`, `check_verb`, `check_handle`, `check_resume`,
//! `check_snapshot` and the stream wrapper `guard_events`. No module crate depends on a concrete
//! adapter; each depends on this crate.
//!
//! Confirmation items this crate is conditional on (Playbook §7.8), each built with the spec's
//! fallback:
//! - **C-31** (ACP permission requests and IDE support): no first-release use in this interface;
//!   nothing is built for it.
//! - **C-37** (model and thinking level per session): `StartRun.model` and `StartRun.thinking` are
//!   optional, and `HarnessModels::per_session_model` lets a harness that cannot take a model per
//!   session ignore the field.
//! - **C-41** (harness to-do lists in the event stream): `RunEvent::HarnessPlan` is optional; a
//!   plan outside 3–7 steps of 1–80 characters is dropped with `RA_PLAN_INVALID` and the run
//!   continues with events only.
//! - **C-80** (session record moves between machines): the trait carries no session export or
//!   import; the session record travels through the Run Queue, outside this crate.
//!
//! The scripted adapter `ScriptedAdapter` is compiled only with feature `testkit`, which only a
//! `[dev-dependencies]` entry turns on.
#![forbid(unsafe_code)]

mod adapter;
mod check;
mod error;
mod guard;
mod models;
mod registry;
#[cfg(any(test, feature = "testkit"))]
mod testkit;

#[cfg(test)]
mod tests;

pub use ordin_types::domain::identity::HarnessKind;
pub use ordin_types::domain::runs::RunState;
// spec: ordin-desktop/components/RuntimeAdapterInterface#SA-RECON-04
pub use ordin_types::session::ToolCaller;
pub use ordin_types::wire::run::{
    AdapterKind, AdapterVerb, ModelChoice, ModelLevel, ProgressCount, RunLimits, RunProgress,
    RunTarget, ThinkingLevel,
};

pub use adapter::{
    AdapterContext, PauseReason, ResumeDecision, RunEnd, RunEndReason, RunEvent, RunHandle,
    RunSnapshot, RuntimeAdapter, StartInput, StartRun, UsageDelta,
};
pub use check::{check_handle, check_resume, check_snapshot, check_start, check_verb, end_state};
pub use error::{AdapterError, CheckError, RegistryError, ALL_CODES};
pub use guard::{guard_events, Guarded};
pub use models::{HarnessModels, LevelModels};
pub use registry::AdapterRegistry;
#[cfg(any(test, feature = "testkit"))]
pub use testkit::ScriptedAdapter;
