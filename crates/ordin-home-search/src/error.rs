//! The module's error type and the codes it owns (HomeAndSearch.md *Public Interface*, *Error Table*;
//! ErrorHandling.md *Internal Logic* 11).

use ordin_types::{CodedError, RepairAction};

/// Every code the registry gives this crate (prefix `HOME_`), sorted.
// spec: ordin-desktop/phase-1-analysis#SA-ERR-01
pub const ALL_CODES: &[&str] = &[
    "HOME_DATASTORE_UNAVAILABLE",
    "HOME_MARK_DUPLICATE",
    "HOME_PATH_INVALID",
    "HOME_QUERY_INVALID",
    "HOME_RUN_NOT_FOUND",
    "HOME_SOURCE_UNAVAILABLE",
    "HOME_WORKSPACE_NOT_FOUND",
];

/// `HOME_PATH_INVALID`, carried by `HomeError::Invalid`.
pub(crate) const PATH_INVALID: &str = "HOME_PATH_INVALID";

/// Every failure of Home and Search. `Display` is the code alone: no error carries a search text,
/// a document name or a workspace name (*Never logged*).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HomeError {
    #[error("HOME_QUERY_INVALID")]
    QueryInvalid,
    #[error("HOME_WORKSPACE_NOT_FOUND")]
    WorkspaceNotFound,
    #[error("HOME_RUN_NOT_FOUND")]
    RunNotFound,
    #[error("HOME_SOURCE_UNAVAILABLE")]
    SourceUnavailable,
    #[error("HOME_DATASTORE_UNAVAILABLE")]
    DatastoreUnavailable,
    /// `HOME_MARK_DUPLICATE` or `HOME_PATH_INVALID`.
    #[error("{0}")]
    Invalid(&'static str),
}

impl CodedError for HomeError {
    fn code(&self) -> &'static str {
        match self {
            HomeError::QueryInvalid => "HOME_QUERY_INVALID",
            HomeError::WorkspaceNotFound => "HOME_WORKSPACE_NOT_FOUND",
            HomeError::RunNotFound => "HOME_RUN_NOT_FOUND",
            HomeError::SourceUnavailable => "HOME_SOURCE_UNAVAILABLE",
            HomeError::DatastoreUnavailable => "HOME_DATASTORE_UNAVAILABLE",
            HomeError::Invalid(code) => code,
        }
    }

    /// The Error Table's status column. `HOME_SOURCE_UNAVAILABLE` is returned only where the rows
    /// are the whole answer (`needs_you`, `run_again_inputs`), which the table marks retryable; on
    /// Home it is logged, never returned. The registry marks the code "per row".
    fn retryable(&self) -> bool {
        matches!(
            self,
            HomeError::SourceUnavailable | HomeError::DatastoreUnavailable
        )
    }

    fn repair(&self) -> Option<RepairAction> {
        self.retryable().then_some(RepairAction::TryAgain)
    }
}

impl From<sqlx::Error> for HomeError {
    fn from(error: sqlx::Error) -> Self {
        tracing::warn!(
            component = crate::COMPONENT,
            error_class = error_class(&error),
            code = "HOME_DATASTORE_UNAVAILABLE",
            "home_datastore_unavailable"
        );
        HomeError::DatastoreUnavailable
    }
}

/// A short class of a database error, for the log line (never the statement or its values).
fn error_class(error: &sqlx::Error) -> &'static str {
    match error {
        sqlx::Error::Database(_) => "database",
        sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed => "pool",
        sqlx::Error::Io(_) => "io",
        sqlx::Error::RowNotFound => "row_not_found",
        _ => "other",
    }
}
