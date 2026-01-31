//! The Approval Service (ApprovalService.md): every "ask a person" decision as an approval, the
//! card the person decides on, the held run that waits, and the one decision that gives the answer
//! back. A pending approval survives a restart and is never approved by timeout.
//!
//! Tables `approvals` and `approval_buttons` live in the Journal's datastore, registered as module
//! `approval` through [`ApprovalService::migrations`]. Every change is written through the Journal
//! (`approval_requested`, `approval_decided`, `needs_you_changed`); run-ended events come back
//! through the service's [`ordin_journal::EventSink`].
#![forbid(unsafe_code)]

mod error;
mod model;
mod service;
mod sink;
mod store;
mod validate;

pub use error::{ApprovalError, ALL_CODES};
pub use model::{
    ApprovalConfig, ApprovalOutcome, ApprovalRecord, ApprovalRequest, ApproverPolicy, ApproverVia,
    ChatChannel, PressOrigin, WaitOutcome,
};
pub use service::ApprovalService;
