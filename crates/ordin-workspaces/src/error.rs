//! The component's error type and the codes it owns (WorkspaceService.md *Public Interface*,
//! *Error Table*; ErrorHandling.md *Internal Logic* 11).

use ordin_types::domain::places::{WorkingCopyState, WorkspaceState};
use ordin_types::{CodedError, RepairAction};

/// Every code the registry gives this crate (its `WS_`, `SRC_`, `WCI_`, `GRANT_`, `CFL_` and the
/// 2C `MAIL_` validation rows), sorted.
// spec: ordin-desktop/phase-1-analysis#SA-ERR-01
pub const ALL_CODES: &[&str] = &[
    "CFL_AUDIENCE_INVALID",
    "CFL_NOT_APPROVED",
    "CFL_PARENT_INVALID",
    "CFL_SPACE_INVALID",
    "GRANT_DUPLICATE",
    "GRANT_PLACE_NOT_IN_WORKSPACE",
    "MAIL_ADDRESS_INVALID",
    "MAIL_DOMAIN_INVALID",
    "MAIL_GRANT_DUPLICATE",
    "MAIL_IMAP_STATE_INVALID",
    "MAIL_LINK_FIELDS_MISMATCH",
    "SRC_DUPLICATE",
    "SRC_KIND_MISMATCH",
    "SRC_LOCATION_INVALID",
    "SRC_NAME_INVALID",
    "SRC_PLACE_MISMATCH",
    "WCI_DUPLICATE",
    "WCI_PATH_INVALID",
    "WCI_VERSION_INVALID",
    "WS_BIN_MISMATCH",
    "WS_CONFLUENCE_UNAVAILABLE",
    "WS_DATASTORE_UNAVAILABLE",
    "WS_FILES_FAILED",
    "WS_FILE_NAME_INVALID",
    "WS_FILE_TOO_LARGE",
    "WS_GIT_REMOTE_INVALID",
    "WS_GRANT_PUBLISH_FAILED",
    "WS_ITEM_TOO_LARGE",
    "WS_JIRA_ADDRESS_INVALID",
    "WS_JIRA_READ_ONLY",
    "WS_JIRA_SITE_MISMATCH",
    "WS_JIRA_UNAVAILABLE",
    "WS_LINK_NOT_FOUND",
    "WS_LINK_STATE_INVALID",
    "WS_LOCAL_FOLDER_ON_SERVER",
    "WS_NAME_DUPLICATE",
    "WS_NAME_EMPTY",
    "WS_NAME_TOO_LONG",
    "WS_NOT_ACTIVE",
    "WS_NOT_A_REPOSITORY",
    "WS_NOT_FOUND",
    "WS_OWN_FOLDER_FAILED",
    "WS_PLACE_REFUSED",
    "WS_SOURCE_ACCESS_DENIED",
    "WS_SOURCE_NOT_CONFIGURED",
    "WS_SOURCE_NOT_FOUND",
    "WS_SOURCE_SIGN_IN_NEEDED",
    "WS_SOURCE_UNAVAILABLE",
    "WS_STATE_INVALID",
    "WS_WORKING_COPY_ITEM_NOT_FOUND",
    "WS_WORKING_COPY_TRANSITION_INVALID",
];

/// Every failure of the Workspace Service. `Display` is the code alone: no error carries a name, a
/// path, an address or file content.
// spec: ordin-desktop/phase-1-analysis#SA-ERR-01
#[derive(Debug, Clone, thiserror::Error)]
pub enum WorkspaceError {
    #[error("WS_NOT_FOUND")]
    NotFound,
    #[error("WS_NOT_ACTIVE")]
    NotActive { state: WorkspaceState },
    #[error("WS_STATE_INVALID")]
    StateInvalid,
    #[error("WS_OWN_FOLDER_FAILED")]
    OwnFolderFailed,
    #[error("WS_LINK_NOT_FOUND")]
    LinkNotFound,
    #[error("WS_LINK_STATE_INVALID")]
    LinkStateInvalid,
    #[error("WS_NOT_A_REPOSITORY")]
    NotARepository,
    #[error("WS_PLACE_REFUSED")]
    PlaceRefused { cause: &'static str },
    #[error("WS_SOURCE_SIGN_IN_NEEDED")]
    SourceSignInNeeded,
    #[error("WS_SOURCE_NOT_CONFIGURED")]
    SourceNotConfigured,
    #[error("WS_SOURCE_ACCESS_DENIED")]
    SourceAccessDenied,
    #[error("WS_SOURCE_NOT_FOUND")]
    SourceNotFound,
    #[error("WS_SOURCE_UNAVAILABLE")]
    SourceUnavailable,
    #[error("WS_CONFLUENCE_UNAVAILABLE")]
    ConfluenceUnavailable,
    #[error("WS_ITEM_TOO_LARGE")]
    ItemTooLarge,
    #[error("WS_WORKING_COPY_TRANSITION_INVALID")]
    WorkingCopyTransitionInvalid {
        from: WorkingCopyState,
        to: WorkingCopyState,
    },
    #[error("WS_GRANT_PUBLISH_FAILED")]
    GrantPublishFailed,
    #[error("WS_FILES_FAILED")]
    FilesFailed,
    #[error("WS_DATASTORE_UNAVAILABLE")]
    DatastoreUnavailable,
    #[error("WS_LOCAL_FOLDER_ON_SERVER")]
    LocalFolderOnServer,
    #[error("WS_WORKING_COPY_ITEM_NOT_FOUND")]
    WorkingCopyItemNotFound,
    #[error("WS_FILE_NAME_INVALID")]
    FileNameInvalid,
    #[error("WS_FILE_TOO_LARGE")]
    FileTooLarge,
    #[error("WS_GIT_REMOTE_INVALID")]
    GitRemoteInvalid,
    /// Amended January 2026, ADR-106.
    #[error("WS_JIRA_UNAVAILABLE")]
    JiraUnavailable,
    /// Amended January 2026, ADR-106.
    #[error("WS_JIRA_ADDRESS_INVALID")]
    JiraAddressInvalid,
    /// Amended January 2026, ADR-106.
    #[error("WS_JIRA_SITE_MISMATCH")]
    JiraSiteMismatch,
    /// Amended January 2026, ADR-106.
    #[error("WS_JIRA_READ_ONLY")]
    JiraReadOnly,
    /// A 2C validation code (WS_NAME_*, WS_BIN_MISMATCH, SRC_*, WCI_*, GRANT_*, CFL_*, MAIL_*).
    #[error("{0}")]
    Invalid(&'static str),
}

impl WorkspaceError {
    /// The registry code.
    pub fn code(&self) -> &'static str {
        match self {
            WorkspaceError::NotFound => "WS_NOT_FOUND",
            WorkspaceError::NotActive { .. } => "WS_NOT_ACTIVE",
            WorkspaceError::StateInvalid => "WS_STATE_INVALID",
            WorkspaceError::OwnFolderFailed => "WS_OWN_FOLDER_FAILED",
            WorkspaceError::LinkNotFound => "WS_LINK_NOT_FOUND",
            WorkspaceError::LinkStateInvalid => "WS_LINK_STATE_INVALID",
            WorkspaceError::NotARepository => "WS_NOT_A_REPOSITORY",
            WorkspaceError::PlaceRefused { .. } => "WS_PLACE_REFUSED",
            WorkspaceError::SourceSignInNeeded => "WS_SOURCE_SIGN_IN_NEEDED",
            WorkspaceError::SourceNotConfigured => "WS_SOURCE_NOT_CONFIGURED",
            WorkspaceError::SourceAccessDenied => "WS_SOURCE_ACCESS_DENIED",
            WorkspaceError::SourceNotFound => "WS_SOURCE_NOT_FOUND",
            WorkspaceError::SourceUnavailable => "WS_SOURCE_UNAVAILABLE",
            WorkspaceError::ConfluenceUnavailable => "WS_CONFLUENCE_UNAVAILABLE",
            WorkspaceError::ItemTooLarge => "WS_ITEM_TOO_LARGE",
            WorkspaceError::WorkingCopyTransitionInvalid { .. } => {
                "WS_WORKING_COPY_TRANSITION_INVALID"
            }
            WorkspaceError::GrantPublishFailed => "WS_GRANT_PUBLISH_FAILED",
            WorkspaceError::FilesFailed => "WS_FILES_FAILED",
            WorkspaceError::DatastoreUnavailable => "WS_DATASTORE_UNAVAILABLE",
            WorkspaceError::LocalFolderOnServer => "WS_LOCAL_FOLDER_ON_SERVER",
            WorkspaceError::WorkingCopyItemNotFound => "WS_WORKING_COPY_ITEM_NOT_FOUND",
            WorkspaceError::FileNameInvalid => "WS_FILE_NAME_INVALID",
            WorkspaceError::FileTooLarge => "WS_FILE_TOO_LARGE",
            WorkspaceError::GitRemoteInvalid => "WS_GIT_REMOTE_INVALID",
            WorkspaceError::JiraUnavailable => "WS_JIRA_UNAVAILABLE",
            WorkspaceError::JiraAddressInvalid => "WS_JIRA_ADDRESS_INVALID",
            WorkspaceError::JiraSiteMismatch => "WS_JIRA_SITE_MISMATCH",
            WorkspaceError::JiraReadOnly => "WS_JIRA_READ_ONLY",
            WorkspaceError::Invalid(code) => code,
        }
    }

    /// The Error Table's status column: true for `Err(retryable)`. `WS_CONFLUENCE_UNAVAILABLE` is
    /// `Err(permanent)` on the personal server and `Err(retryable)` when the approval could not be
    /// requested; the registry records the retryable reading, which this follows.
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            WorkspaceError::OwnFolderFailed
                | WorkspaceError::SourceUnavailable
                | WorkspaceError::ConfluenceUnavailable
                | WorkspaceError::GrantPublishFailed
                | WorkspaceError::FilesFailed
                | WorkspaceError::DatastoreUnavailable
        )
    }
}

impl CodedError for WorkspaceError {
    fn code(&self) -> &'static str {
        WorkspaceError::code(self)
    }

    fn retryable(&self) -> bool {
        WorkspaceError::retryable(self)
    }

    /// The `action` column of the Error Table.
    fn repair(&self) -> Option<RepairAction> {
        match self {
            WorkspaceError::SourceSignInNeeded => Some(RepairAction::SignInAgain),
            WorkspaceError::SourceNotConfigured => None,
            WorkspaceError::OwnFolderFailed
            | WorkspaceError::SourceUnavailable
            | WorkspaceError::GrantPublishFailed
            | WorkspaceError::FilesFailed
            | WorkspaceError::DatastoreUnavailable => Some(RepairAction::TryAgain),
            _ => None,
        }
    }
}

/// Any statement failure on this module's tables is `WS_DATASTORE_UNAVAILABLE`.
impl From<sqlx::Error> for WorkspaceError {
    fn from(error: sqlx::Error) -> Self {
        let class = match &error {
            sqlx::Error::Database(_) => "database",
            sqlx::Error::PoolClosed | sqlx::Error::PoolTimedOut => "pool",
            sqlx::Error::Io(_) => "io",
            _ => "other",
        };
        tracing::warn!(
            component = crate::log::COMPONENT,
            error_class = class,
            "datastore_failed"
        );
        WorkspaceError::DatastoreUnavailable
    }
}

/// A File Service failure is `WS_FILES_FAILED`; the file code goes to the log only.
pub(crate) fn file_failed(error: ordin_files::FileError) -> WorkspaceError {
    tracing::warn!(
        component = crate::log::COMPONENT,
        code = error.code(),
        "files_failed"
    );
    WorkspaceError::FilesFailed
}

/// True when a statement failed on a UNIQUE constraint.
pub(crate) fn is_unique_violation(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(db) if db.is_unique_violation())
}
