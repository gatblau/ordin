//! The Ordin Host (OrdinHost.md): Ordin, the coordinator, as one harness session per workspace
//! conversation. It builds the seed Ordin works from, serves Ordin's tools as the `ToolProvider` of
//! server `ordin`, writes crew charters and implements the Crew Orchestrator's `Coordinator`, owns
//! workflows and their versions with a fixed-code check, and owns summaries, Clear, continuing an
//! earlier conversation, voice settings and Ordin's language (ADR-027, ADR-049, ADR-062, ADR-064,
//! ADR-108, ADR-109, ADR-110, ADR-111, ADR-112, ADR-113).
//!
//! Tables live in the Journal's datastore, registered as module `ordin_host` through
//! [`OrdinHost::migrations`]. No environment variable is read: the program passes
//! [`OrdinHostConfig`] and [`OrdinHostPolicy`].
//!
//! Conditional on C-29 (the harness reports context use): until it is confirmed, the Usage Planner
//! decides when to summarise on the turn count alone (SA-CLOSE-07).
#![forbid(unsafe_code)]

mod authoring;
mod closed;
mod conversation;
mod coordinator;
mod crews;
mod error;
mod host;
mod ids;
mod language;
mod memory;
#[cfg(feature = "minutes")]
mod minutes;
#[cfg(feature = "minutes")]
mod minutes_tools;
#[cfg(any(test, feature = "testkit"))]
pub mod ports;
#[cfg(not(any(test, feature = "testkit")))]
mod ports;
mod seed;
mod send_back;
mod store;
mod task_form;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
mod tools;
mod types;
mod wff;
mod workflows;

#[cfg(test)]
mod tests;

pub use error::{OhError, WorkflowCheckReport, ALL_CODES};
pub use host::OrdinHost;
pub use ids::OrdinConversationId;
#[cfg(feature = "minutes")]
pub use minutes::{WriteMinutesRun, MINUTES_INSTRUCTIONS_VERSION};
#[cfg(feature = "minutes")]
pub use minutes_tools::{check_path, export_name, LocalFile, MinutesExportFormat, MinutesPort};
/// The error a `MinutesPort` answers with (OH-MIN-5).
#[cfg(feature = "minutes")]
pub use ports::DepError;
pub use types::*;
