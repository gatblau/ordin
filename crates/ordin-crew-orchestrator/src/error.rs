//! The Crew Orchestrator's one error type and the codes the registry gives this crate
//! (CrewOrchestrator.md *Error Table*; ErrorHandling.md *Internal Logic* 11).

use ordin_journal::JournalError;
use ordin_types::CodedError;

/// Every code the registry gives this crate: the `CREW_` codes, the 2C `ROUND_` codes and the 2C
/// `STEP_` codes, sorted.
///
/// Returned as an error: `CREW_NOT_STARTED_BY_PERSON`, `CREW_CHARTER_INVALID` (with the 2C code in
/// [`CrewError::CharterInvalid`]), `CREW_NOT_FOUND`, `CREW_ENDED`, `CREW_NOT_PAUSED`,
/// `CREW_TAKE_OVER_REFUSED` and `CREW_STORE_UNAVAILABLE`. Carried as a [`crate::PlanProblem`]:
/// `CREW_TOO_MANY_MEMBERS` and the `STEP_` codes. Recorded with a crew run, never returned:
/// `CREW_GUARDRAIL_REFUSED`, `CREW_LIMIT_REACHED`, `CREW_MEMBER_FAILED_TWICE`,
/// `CREW_COORDINATOR_UNAVAILABLE` and `CREW_COORDINATOR_UNUSABLE`. Checked before a write by the
/// internal run and round validators: `CREW_WORKFLOW_MISMATCH`, `CREW_ROUNDS_EXCEEDED`,
/// `CREW_BUDGET_EXCEEDED`, `CREW_PAUSE_REASON_MISMATCH`, `CREW_END_REASON_TOO_LONG`,
/// `ROUND_NUMBER_GAP`, `ROUND_DECISION_INVALID`. `ROUND_IMMUTABLE` is enforced by having no
/// statement that updates a closed round. `ROUND_TOO_MANY_RUNNING` is registered to this crate and
/// never raised (see `guard.rs`).
// spec: ordin-desktop/components/CrewOrchestrator#SA-RECON-03
pub const ALL_CODES: &[&str] = &[
    "CREW_BUDGET_EXCEEDED",
    "CREW_CHARTER_INVALID",
    "CREW_COORDINATOR_UNAVAILABLE",
    "CREW_COORDINATOR_UNUSABLE",
    "CREW_ENDED",
    "CREW_END_REASON_TOO_LONG",
    "CREW_GUARDRAIL_REFUSED",
    "CREW_LIMIT_REACHED",
    "CREW_MEMBER_FAILED_TWICE",
    "CREW_NOT_FOUND",
    "CREW_NOT_PAUSED",
    "CREW_NOT_STARTED_BY_PERSON",
    "CREW_PAUSE_REASON_MISMATCH",
    "CREW_ROUNDS_EXCEEDED",
    "CREW_STORE_UNAVAILABLE",
    "CREW_TAKE_OVER_REFUSED",
    "CREW_TOO_MANY_MEMBERS",
    "CREW_WORKFLOW_MISMATCH",
    "ROUND_DECISION_INVALID",
    "ROUND_IMMUTABLE",
    "ROUND_NUMBER_GAP",
    "ROUND_TOO_MANY_RUNNING",
    "STEP_CALL_CYCLE",
    "STEP_CALL_INVALID",
    "STEP_CONDITION_INVALID",
    "STEP_EACH_MAX_OUT_OF_RANGE",
    "STEP_EACH_NOT_LIST",
    "STEP_GROUP_TOO_LARGE",
    "STEP_IF_PATHS_OUT_OF_RANGE",
    "STEP_LINE_INVALID",
    "STEP_NESTING_TOO_DEEP",
    "STEP_POSITION_INVALID",
    "STEP_TOO_MANY",
    "STEP_UNTIL_MAX_OUT_OF_RANGE",
    "STEP_USES_LATER_OR_ABSENT",
];

/// Every failure a caller of [`crate::CrewOrchestrator`] can see.
#[derive(Debug, thiserror::Error)]
pub enum CrewError {
    #[error("charter invalid: {code}")]
    CharterInvalid { code: &'static str },
    #[error("not started by the person")]
    NotStartedByPerson,
    #[error("crew run not found")]
    NotFound,
    #[error("crew run already ended")]
    Ended,
    #[error("crew run not paused")]
    NotPaused,
    #[error("too many crew runs")]
    Busy,
    #[error("take-over refused: {reason}")]
    TakeOverRefused { reason: &'static str },
    #[error("datastore unavailable")]
    Store(#[source] sqlx::Error),
    #[error("journal unavailable")]
    Journal(#[source] JournalError),
}

impl CrewError {
    /// The registry code. `CharterInvalid` is `CREW_CHARTER_INVALID`; the 2C code that broke the
    /// charter travels in its `code` field. A Journal failure is reported as the store being
    /// unavailable, since the Journal holds the datastore this module writes to (SA-RECON-03).
    pub fn code(&self) -> &'static str {
        match self {
            CrewError::CharterInvalid { .. } => "CREW_CHARTER_INVALID",
            CrewError::NotStartedByPerson => "CREW_NOT_STARTED_BY_PERSON",
            CrewError::NotFound => "CREW_NOT_FOUND",
            CrewError::Ended => "CREW_ENDED",
            CrewError::NotPaused => "CREW_NOT_PAUSED",
            // The registry has no code of its own for `Busy`. Nothing returns it.
            CrewError::Busy => "CREW_LIMIT_REACHED",
            CrewError::TakeOverRefused { .. } => "CREW_TAKE_OVER_REFUSED",
            CrewError::Store(_) | CrewError::Journal(_) => "CREW_STORE_UNAVAILABLE",
        }
    }

    /// True for an unavailable store (Error Table: `Err(retryable)`); every other error is a
    /// property of the call.
    pub fn retryable(&self) -> bool {
        matches!(self, CrewError::Store(_) | CrewError::Journal(_))
    }
}

impl CodedError for CrewError {
    fn code(&self) -> &'static str {
        CrewError::code(self)
    }

    fn retryable(&self) -> bool {
        CrewError::retryable(self)
    }
}

impl From<sqlx::Error> for CrewError {
    fn from(e: sqlx::Error) -> Self {
        CrewError::Store(e)
    }
}

impl From<JournalError> for CrewError {
    fn from(e: JournalError) -> Self {
        CrewError::Journal(e)
    }
}
