//! The Session Supervisor's one error type and the codes the registry gives this crate
//! (SessionSupervisor.md *Error Table*; ErrorHandling.md *Internal Logic* 11).

use ordin_journal::JournalError;
use ordin_runtime_adapter::{AdapterError, AdapterKind, AdapterVerb};
use ordin_types::CodedError;

/// Every code the registry gives this crate: the `SUP_` codes, the 2C `TASK_` and `RUNST_`
/// validation codes, and the two start-up report codes the registry places here.
///
/// Raised as a returned error: the `SUP_` codes of the Error Table that say `Err`, and the
/// `TASK_` codes through [`SupervisorError::TaskInvalid`]. Journalled with a run, never returned:
/// `SUP_LIMIT_REACHED`, `SUP_LEASE_LOST`, `SUP_RESUME_REFUSED`. `TASK_IMMUTABLE` is enforced by
/// having no update statement for `tasks`. The `RUNST_` codes are checked before every `runs`
/// write ([`crate::validate::check_run`]); `RUNST_INPUTS_TOO_MANY` is the one a caller can reach
/// through `start_run`. `START_TOO_MANY_ITEMS` is logged when a start-up report is cut;
/// `START_PLAIN_INVALID` names an item whose `plain` line is empty, which is dropped and logged.
pub const ALL_CODES: &[&str] = &[
    "RUNST_ABANDONED_NOT_REMOTE",
    "RUNST_ADAPTER_MISMATCH",
    "RUNST_CACHE_EXCEEDS_TOKENS",
    "RUNST_ENDED_AT_MISMATCH",
    "RUNST_END_REASON_TOO_LONG",
    "RUNST_INPUTS_TOO_MANY",
    "RUNST_LIMIT_OUT_OF_RANGE",
    "RUNST_NO_REPLY_NOT_SLACK",
    "RUNST_PAUSE_REASON_MISMATCH",
    "START_PLAIN_INVALID",
    "START_TOO_MANY_ITEMS",
    "SUP_ADAPTER_REFUSED",
    "SUP_LEASE_LOST",
    "SUP_LIMIT_REACHED",
    "SUP_NO_ADAPTER",
    "SUP_RESUME_REFUSED",
    "SUP_RUN_ENDED",
    "SUP_RUN_NOT_FOUND",
    "SUP_RUN_NOT_PAUSED",
    "SUP_SEED_EMPTY",
    "SUP_STOPPING",
    "SUP_STORE_UNAVAILABLE",
    "SUP_TARGET_IS_CREW",
    "SUP_VERB_NOT_OFFERED",
    "TASK_ARGUMENTS_INVALID",
    "TASK_COMMAND_INVALID",
    "TASK_COMMAND_XOR_TEXT",
    "TASK_IMMUTABLE",
    "TASK_TEXT_INVALID",
];

#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    #[error("task invalid: {code}")]
    TaskInvalid { code: &'static str },
    #[error("start request invalid: {code}")]
    StartInvalid { code: &'static str },
    #[error("no adapter for {0:?}")]
    NoAdapter(AdapterKind),
    #[error("adapter refused: {0}")]
    Adapter(#[source] AdapterError),
    #[error("run not found")]
    RunNotFound,
    #[error("run already ended")]
    RunEnded,
    #[error("run not paused")]
    RunNotPaused,
    #[error("verb not offered")]
    VerbNotOffered(AdapterVerb),
    #[error("stopping")]
    Stopping,
    #[error("datastore unavailable")]
    Store(#[source] sqlx::Error),
    #[error("journal unavailable")]
    Journal(#[source] JournalError),
}

impl SupervisorError {
    /// The registry code (see the Error Table). `TaskInvalid` and `StartInvalid` carry theirs;
    /// a Journal failure is reported as the store being unavailable, since the Journal holds the
    /// datastore this module writes to (SA-RECON-03).
    pub fn code(&self) -> &'static str {
        match self {
            SupervisorError::TaskInvalid { code } | SupervisorError::StartInvalid { code } => code,
            SupervisorError::NoAdapter(_) => "SUP_NO_ADAPTER",
            SupervisorError::Adapter(_) => "SUP_ADAPTER_REFUSED",
            SupervisorError::RunNotFound => "SUP_RUN_NOT_FOUND",
            SupervisorError::RunEnded => "SUP_RUN_ENDED",
            SupervisorError::RunNotPaused => "SUP_RUN_NOT_PAUSED",
            SupervisorError::VerbNotOffered(_) => "SUP_VERB_NOT_OFFERED",
            SupervisorError::Stopping => "SUP_STOPPING",
            SupervisorError::Store(_) | SupervisorError::Journal(_) => "SUP_STORE_UNAVAILABLE",
        }
    }

    /// True for an adapter refusing a resume and for an unavailable store.
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            SupervisorError::Adapter(_) | SupervisorError::Store(_) | SupervisorError::Journal(_)
        )
    }
}

impl CodedError for SupervisorError {
    fn code(&self) -> &'static str {
        SupervisorError::code(self)
    }

    fn retryable(&self) -> bool {
        SupervisorError::retryable(self)
    }
}

impl From<sqlx::Error> for SupervisorError {
    fn from(e: sqlx::Error) -> Self {
        SupervisorError::Store(e)
    }
}

impl From<JournalError> for SupervisorError {
    fn from(e: JournalError) -> Self {
        SupervisorError::Journal(e)
    }
}
