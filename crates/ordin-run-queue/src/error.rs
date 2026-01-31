//! The Run Queue's one error type and the codes the registry gives this crate (RunQueue.md
//! *Error Table*; ErrorHandling.md *Internal Logic* 11).

use ordin_types::CodedError;

/// Every code the registry gives this crate: the `RUN_` queue codes, `DB_UNAVAILABLE`, and the 2C
/// `RREC_` record-validation codes.
///
/// Five are owned here but never raised as such: `RREC_LEASE_MISMATCH` and `RREC_END_KIND_MISMATCH`
/// are enforced by the tables' `CHECK` constraints, `RREC_INTERRUPTED_RESTART` by `mark_interrupted`
/// being the only writer of `interrupted` and nothing queueing such a run again,
/// `RREC_MESSAGE_INVALID` is reported as `RUN_BAD_INPUT` naming `text` (RunQueue.md step 1), and
/// `RUN_LIMIT` is written by the Session Supervisor as a log field only (ErrorHandling.md).
/// `SRV_BAD_CURSOR` is raised but owned by `ordin-types`.
pub const ALL_CODES: &[&str] = &[
    "DB_UNAVAILABLE",
    "RREC_END_KIND_MISMATCH",
    "RREC_INTERRUPTED_RESTART",
    "RREC_LEASE_MISMATCH",
    "RREC_MESSAGE_INVALID",
    "RUN_BAD_INPUT",
    "RUN_DUPLICATE_ID",
    "RUN_LEASE_LOST",
    "RUN_LIMIT",
    "RUN_NOT_CANCELLABLE",
    "RUN_NOT_PAUSED",
    "RUN_SESSION_RECORD_TOO_LARGE",
    "RUN_THREAD_BUSY",
    "RUN_THREAD_NOT_FOUND",
];

#[derive(Debug, thiserror::Error)]
pub enum RunQueueError {
    #[error("No thread has this identifier.")]
    ThreadNotFound,
    #[error("The thread has a run that has not ended.")]
    ThreadBusy,
    #[error("A run has this identifier already.")]
    DuplicateId,
    #[error("A run starts with a command or with a message, not both.")]
    BadInput { field: &'static str },
    #[error("The run is not paused.")]
    NotPaused,
    #[error("The run has ended and cannot be cancelled.")]
    NotCancellable,
    #[error("The run is no longer executing under this instance's lease.")]
    LeaseLost,
    #[error("The session record exceeds 64 MiB.")]
    SessionRecordTooLarge,
    #[error("the cursor is not one this listing returned")]
    BadCursor,
    #[error("The runtime database did not answer.")]
    Unavailable,
}

impl RunQueueError {
    /// The registry code (see the Error Table).
    pub fn code(&self) -> &'static str {
        match self {
            RunQueueError::ThreadNotFound => "RUN_THREAD_NOT_FOUND",
            RunQueueError::ThreadBusy => "RUN_THREAD_BUSY",
            RunQueueError::DuplicateId => "RUN_DUPLICATE_ID",
            RunQueueError::BadInput { .. } => "RUN_BAD_INPUT",
            RunQueueError::NotPaused => "RUN_NOT_PAUSED",
            RunQueueError::NotCancellable => "RUN_NOT_CANCELLABLE",
            RunQueueError::LeaseLost => "RUN_LEASE_LOST",
            RunQueueError::SessionRecordTooLarge => "RUN_SESSION_RECORD_TOO_LARGE",
            RunQueueError::BadCursor => "SRV_BAD_CURSOR",
            RunQueueError::Unavailable => "DB_UNAVAILABLE",
        }
    }

    /// True for `Unavailable` only.
    pub fn retryable(&self) -> bool {
        matches!(self, RunQueueError::Unavailable)
    }
}

impl CodedError for RunQueueError {
    fn code(&self) -> &'static str {
        RunQueueError::code(self)
    }

    fn retryable(&self) -> bool {
        RunQueueError::retryable(self)
    }
}
