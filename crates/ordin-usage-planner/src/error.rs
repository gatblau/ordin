//! The Usage Planner's one error type and the codes it owns (UsagePlanner.md *Error Table*;
//! ErrorHandling.md *Internal Logic* 11).

use ordin_journal::JournalError;
use ordin_types::{CodedError, RepairAction};

/// Every code the registry gives this crate (its `USAGE_` and `REUSE_` rows).
///
/// `REUSE_EXPIRY_INVALID` and `REUSE_INPUT_NEWER` are owned here but never returned: the expiry is
/// computed as `answered_at + 7 days`, so it cannot be invalid, and a newer input only skips a row
/// (Internal Logic G.2), which is logged.
pub const ALL_CODES: &[&str] = &[
    "REUSE_EXPIRY_INVALID",
    "REUSE_INPUT_NEWER",
    "REUSE_REQUEST_INVALID",
    "USAGE_ABOVE_PROFILE_LIMIT",
    "USAGE_AGENTS_OUT_OF_RANGE",
    "USAGE_DAILY_PAUSE_REACHED",
    "USAGE_FIXED_BY_COMPANY",
    "USAGE_JOURNAL_UNAVAILABLE",
    "USAGE_LEVEL_UNKNOWN",
    "USAGE_NOT_ANSWER_ONLY",
    "USAGE_STORE_UNAVAILABLE",
];

/// Every failure of this crate. `Display` never carries a value from the caller or the datastore.
#[derive(Debug, thiserror::Error)]
pub enum UsageError {
    #[error("set by your company")]
    FixedByCompany { setting: &'static str },
    #[error("above the company's limit")]
    AboveProfileLimit { setting: &'static str },
    #[error("unknown level or kind")]
    LevelUnknown,
    #[error("agents at once out of range")]
    AgentsOutOfRange,
    #[error("daily pause reached")]
    DailyPauseReached { turns_today: u32, limit: u32 },
    #[error("answer not reusable")]
    NotAnswerOnly,
    #[error("request text invalid")]
    RequestInvalid,
    #[error("datastore unavailable")]
    Store(#[source] sqlx::Error),
    #[error("journal unavailable")]
    Journal(#[source] JournalError),
}

impl UsageError {
    /// The registry code.
    pub fn code(&self) -> &'static str {
        match self {
            UsageError::FixedByCompany { .. } => "USAGE_FIXED_BY_COMPANY",
            UsageError::AboveProfileLimit { .. } => "USAGE_ABOVE_PROFILE_LIMIT",
            UsageError::LevelUnknown => "USAGE_LEVEL_UNKNOWN",
            UsageError::AgentsOutOfRange => "USAGE_AGENTS_OUT_OF_RANGE",
            UsageError::DailyPauseReached { .. } => "USAGE_DAILY_PAUSE_REACHED",
            UsageError::NotAnswerOnly => "USAGE_NOT_ANSWER_ONLY",
            UsageError::RequestInvalid => "REUSE_REQUEST_INVALID",
            UsageError::Store(_) => "USAGE_STORE_UNAVAILABLE",
            UsageError::Journal(_) => "USAGE_JOURNAL_UNAVAILABLE",
        }
    }

    /// `true` for the two unavailable-store rows; every other error is permanent.
    pub fn retryable(&self) -> bool {
        matches!(self, UsageError::Store(_) | UsageError::Journal(_))
    }
}

impl CodedError for UsageError {
    fn code(&self) -> &'static str {
        UsageError::code(self)
    }

    fn retryable(&self) -> bool {
        UsageError::retryable(self)
    }

    fn repair(&self) -> Option<RepairAction> {
        match self {
            // "Couldn't save this setting. Try again." (Error Table).
            UsageError::Store(_) | UsageError::Journal(_) => Some(RepairAction::TryAgain),
            _ => None,
        }
    }
}

/// Any datastore failure is `USAGE_STORE_UNAVAILABLE` (retryable).
impl From<sqlx::Error> for UsageError {
    fn from(error: sqlx::Error) -> Self {
        UsageError::Store(error)
    }
}

impl From<JournalError> for UsageError {
    fn from(error: JournalError) -> Self {
        UsageError::Journal(error)
    }
}

impl UsageError {
    /// A stored row that no longer parses: the store cannot be trusted, which is the same
    /// retryable condition as an unreachable store.
    pub(crate) fn corrupt(what: &'static str) -> UsageError {
        UsageError::Store(sqlx::Error::Decode(what.into()))
    }
}
