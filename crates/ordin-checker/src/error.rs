//! The Checker's one error type and the codes it owns (Checker.md *Error Table*; ErrorHandling.md
//! *Internal Logic* 11).
//!
//! Findings are results, not errors: a failing check returns `Ok` with a report that does not pass.
//! The errors below are failures to run a check. `Display` is the bare code, so a caller logging the
//! error never writes part content.

use ordin_agent_model::PartSourceError;
use ordin_resolver::ResolveError;
use ordin_types::{AgentInstanceId, CodedError};

use crate::model::CheckRunId;

/// Every code the registry gives this crate: its ten `CHK_` rows and the two `EXP_` rules of 2C.
///
/// `EXP_PART_DUPLICATE` and `EXP_SOURCE_NOT_POLICY` are enforced by construction and never returned:
/// a duplicate (part name, kind) is dropped before it is stored (step 31), and every stored decision
/// is written with `source = organisation_policy`.
pub const ALL_CODES: &[&str] = &[
    "CHK_CONTRACT_INVALID",
    "CHK_DATASTORE_FAILED",
    "CHK_INPUT_INVALID",
    "CHK_INSTANCE_UNKNOWN",
    "CHK_RESOLVER_FAILED",
    "CHK_RUN_NOT_FOUND",
    "CHK_SOURCE_ALREADY_BOUND",
    "CHK_SOURCE_NOT_BOUND",
    "CHK_SOURCE_UNAVAILABLE",
    "CHK_TOO_MANY_FINDINGS",
    "EXP_PART_DUPLICATE",
    "EXP_SOURCE_NOT_POLICY",
];

// spec: ordin-desktop/components/Checker#SA-RECON-02
#[derive(Debug, Clone, thiserror::Error)]
pub enum CheckError {
    #[error("CHK_SOURCE_UNAVAILABLE")]
    SourceUnavailable { retryable: bool },
    #[error("CHK_RESOLVER_FAILED")]
    ResolverFailed { code: String, retryable: bool },
    #[error("CHK_INPUT_INVALID")]
    InputInvalid { field: &'static str },
    #[error("CHK_TOO_MANY_FINDINGS")]
    TooManyFindings,
    #[error("CHK_RUN_NOT_FOUND")]
    RunNotFound { id: CheckRunId },
    #[error("CHK_DATASTORE_FAILED")]
    DatastoreFailed { retryable: bool },
    #[error("CHK_SOURCE_NOT_BOUND")]
    SourceNotBound,
    #[error("CHK_SOURCE_ALREADY_BOUND")]
    SourceAlreadyBound,
    #[error("CHK_CONTRACT_INVALID")]
    ContractInvalid { reason: &'static str },
    #[error("CHK_INSTANCE_UNKNOWN")]
    InstanceUnknown { id: AgentInstanceId },
}

impl CheckError {
    /// The registry code; equal to the `Display` text.
    pub fn code(&self) -> &'static str {
        match self {
            CheckError::SourceUnavailable { .. } => "CHK_SOURCE_UNAVAILABLE",
            CheckError::ResolverFailed { .. } => "CHK_RESOLVER_FAILED",
            CheckError::InputInvalid { .. } => "CHK_INPUT_INVALID",
            CheckError::TooManyFindings => "CHK_TOO_MANY_FINDINGS",
            CheckError::RunNotFound { .. } => "CHK_RUN_NOT_FOUND",
            CheckError::DatastoreFailed { .. } => "CHK_DATASTORE_FAILED",
            CheckError::SourceNotBound => "CHK_SOURCE_NOT_BOUND",
            CheckError::SourceAlreadyBound => "CHK_SOURCE_ALREADY_BOUND",
            CheckError::ContractInvalid { .. } => "CHK_CONTRACT_INVALID",
            CheckError::InstanceUnknown { .. } => "CHK_INSTANCE_UNKNOWN",
        }
    }

    /// The three variants that carry a flag report it; every other error is permanent.
    pub fn retryable(&self) -> bool {
        match self {
            CheckError::SourceUnavailable { retryable }
            | CheckError::ResolverFailed { retryable, .. }
            | CheckError::DatastoreFailed { retryable } => *retryable,
            _ => false,
        }
    }
}

impl CodedError for CheckError {
    fn code(&self) -> &'static str {
        CheckError::code(self)
    }
    fn retryable(&self) -> bool {
        CheckError::retryable(self)
    }
}

/// Step 1: a source failure. An unreadable withdrawn list with no earlier copy and a corrupt row are
/// permanent; `Unavailable` keeps the source's own flag.
pub(crate) fn from_source(error: PartSourceError) -> CheckError {
    match error {
        PartSourceError::Unavailable { retryable } => CheckError::SourceUnavailable { retryable },
        PartSourceError::WithdrawnListUnreadable { .. } | PartSourceError::Corrupt { .. } => {
            CheckError::SourceUnavailable { retryable: false }
        }
    }
}

/// Step 1: a `ResolveError` that is not a finding (a source or store failure, or a program that has
/// not bound the resolver's source) stops the check; every other one is a finding.
pub(crate) fn resolver_failure(error: &ResolveError) -> Option<CheckError> {
    match error {
        ResolveError::SourceUnavailable { .. }
        | ResolveError::StoreWriteFailed { .. }
        | ResolveError::SourceNotBound
        | ResolveError::SourceAlreadyBound => Some(CheckError::ResolverFailed {
            code: error.code().to_owned(),
            retryable: error.retryable(),
        }),
        _ => None,
    }
}

/// A datastore failure: busy, locked or a pool timeout is retryable, anything else permanent.
pub(crate) fn from_sqlx(error: &sqlx::Error) -> CheckError {
    let retryable = match error {
        sqlx::Error::PoolTimedOut | sqlx::Error::Io(_) => true,
        sqlx::Error::Database(db) => {
            let code = db.code().map(|c| c.into_owned()).unwrap_or_default();
            // SQLite primary codes 5 (SQLITE_BUSY) and 6 (SQLITE_LOCKED), with extended forms.
            let primary = code.parse::<i64>().map(|c| c & 0xff).unwrap_or(-1);
            primary == 5 || primary == 6
        }
        _ => false,
    };
    CheckError::DatastoreFailed { retryable }
}
