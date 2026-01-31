//! The component's two error types and the codes it owns (ProjectService.md *Error Table*;
//! ErrorHandling.md *Internal Logic* 11).

use ordin_types::{CodedError, RepairAction};

/// Every code the registry gives this crate (its `PROJ_`, `GH_`, `CODE_`, `CSET_`, `GHOP_` and
/// `LANG_` rows), sorted.
pub const ALL_CODES: &[&str] = &[
    "CODE_BRANCH_INVALID",
    "CODE_COMMIT_INVALID",
    "CODE_GROUP_TOO_SMALL",
    "CODE_REQUEST_INVALID",
    "CODE_WORKTREE_OUTSIDE_ROOT",
    "CSET_DUPLICATE",
    "CSET_HUNK_TOO_LARGE",
    "CSET_UNDECIDED_ENTRIES",
    "GHOP_ABOVE_CEILING",
    "GHOP_DUPLICATE",
    "GH_CONFLICT",
    "GH_FAILED",
    "GH_HOST_NOT_SUPPORTED",
    "GH_NOT_FOUND",
    "GH_RATE_LIMITED",
    "GH_SIGN_IN_NEEDED",
    "GH_UNREACHABLE",
    "LANG_DUPLICATE",
    "LANG_SERVER_NOT_APPROVED",
    "LANG_SETTINGS_INVALID",
    "PROJ_BRANCH_EXISTS",
    "PROJ_BRANCH_MISMATCH",
    "PROJ_CHECKS_INVALID",
    "PROJ_CHECK_DENIED",
    "PROJ_CHECK_TIMEOUT",
    "PROJ_DATASTORE_UNAVAILABLE",
    "PROJ_DECISION_INVALID",
    "PROJ_ENTRY_NOT_FOUND",
    "PROJ_FILES_FAILED",
    "PROJ_GIT_FAILED",
    "PROJ_IDE_NOT_FOUND",
    "PROJ_IDE_START_FAILED",
    "PROJ_LSP_METHOD_NOT_ALLOWED",
    "PROJ_LSP_UNAVAILABLE",
    "PROJ_NOTHING_TO_COMMIT",
    "PROJ_NOT_A_REPOSITORY",
    "PROJ_NOT_FOUND",
    "PROJ_ORIGIN_CHANGED",
    "PROJ_PLACE_ALREADY_OPEN",
    "PROJ_PUSH_REFUSED",
    "PROJ_RATE_LIMITED",
    "PROJ_REMOTE_UNREACHABLE",
    "PROJ_SERVER_INSTALL_FAILED",
    "PROJ_SIGN_IN_NEEDED",
    "PROJ_TASK_NOT_FOUND",
    "PROJ_TASK_STATE_INVALID",
    "PROJ_TERMINAL_FAILED",
    "PROJ_TOO_MANY_ATTEMPTS",
    "PROJ_TOO_MANY_FOLLOWED",
    "PROJ_UNDECIDED_CHANGES",
    "PROJ_WORKING_COPY_DIRTY",
    "PROJ_WORKTREE_FAILED",
];

/// Every failure of the Project Service. `Display` is the code alone, except `Invalid`, whose text
/// is its code: no error carries a path, a token or file content.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ProjectError {
    #[error("PROJ_NOT_FOUND")]
    NotFound,
    #[error("PROJ_NOT_A_REPOSITORY")]
    NotARepository,
    #[error("PROJ_PLACE_ALREADY_OPEN")]
    PlaceAlreadyOpen,
    #[error("PROJ_SIGN_IN_NEEDED")]
    SignInNeeded,
    #[error("PROJ_REMOTE_UNREACHABLE")]
    RemoteUnreachable,
    #[error("PROJ_RATE_LIMITED")]
    RateLimited,
    #[error("PROJ_ORIGIN_CHANGED")]
    OriginChanged { remote_commit: String },
    #[error("PROJ_PUSH_REFUSED")]
    PushRefused,
    #[error("PROJ_NOTHING_TO_COMMIT")]
    NothingToCommit,
    #[error("PROJ_UNDECIDED_CHANGES")]
    UndecidedChanges,
    #[error("PROJ_BRANCH_EXISTS")]
    BranchExists,
    #[error("PROJ_WORKING_COPY_DIRTY")]
    WorkingCopyDirty,
    #[error("PROJ_TOO_MANY_ATTEMPTS")]
    TooManyAttempts,
    #[error("PROJ_TASK_NOT_FOUND")]
    TaskNotFound,
    #[error("PROJ_TASK_STATE_INVALID")]
    TaskStateInvalid,
    #[error("PROJ_WORKTREE_FAILED")]
    WorktreeFailed,
    #[error("PROJ_ENTRY_NOT_FOUND")]
    EntryNotFound,
    #[error("PROJ_DECISION_INVALID")]
    DecisionInvalid,
    #[error("PROJ_CHECK_DENIED")]
    CheckDenied { reason: String },
    #[error("PROJ_CHECK_TIMEOUT")]
    CheckTimeout,
    #[error("PROJ_LSP_UNAVAILABLE")]
    LspUnavailable,
    #[error("PROJ_LSP_METHOD_NOT_ALLOWED")]
    LspMethodNotAllowed,
    #[error("PROJ_SERVER_INSTALL_FAILED")]
    ServerInstallFailed,
    #[error("PROJ_IDE_NOT_FOUND")]
    IdeNotFound,
    #[error("PROJ_IDE_START_FAILED")]
    IdeStartFailed,
    #[error("PROJ_TERMINAL_FAILED")]
    TerminalFailed,
    #[error("PROJ_GIT_FAILED")]
    GitFailed,
    #[error("PROJ_FILES_FAILED")]
    FilesFailed,
    #[error("PROJ_TOO_MANY_FOLLOWED")]
    TooManyFollowed,
    #[error("PROJ_DATASTORE_UNAVAILABLE")]
    DatastoreUnavailable,
    /// `CODE_*`, `CSET_*`, `GHOP_*`, `LANG_*`, `PROJ_CHECKS_INVALID`, `PROJ_BRANCH_MISMATCH`.
    #[error("{0}")]
    Invalid(&'static str),
}

impl ProjectError {
    /// The registry code.
    pub fn code(&self) -> &'static str {
        match self {
            ProjectError::NotFound => "PROJ_NOT_FOUND",
            ProjectError::NotARepository => "PROJ_NOT_A_REPOSITORY",
            ProjectError::PlaceAlreadyOpen => "PROJ_PLACE_ALREADY_OPEN",
            ProjectError::SignInNeeded => "PROJ_SIGN_IN_NEEDED",
            ProjectError::RemoteUnreachable => "PROJ_REMOTE_UNREACHABLE",
            ProjectError::RateLimited => "PROJ_RATE_LIMITED",
            ProjectError::OriginChanged { .. } => "PROJ_ORIGIN_CHANGED",
            ProjectError::PushRefused => "PROJ_PUSH_REFUSED",
            ProjectError::NothingToCommit => "PROJ_NOTHING_TO_COMMIT",
            ProjectError::UndecidedChanges => "PROJ_UNDECIDED_CHANGES",
            ProjectError::BranchExists => "PROJ_BRANCH_EXISTS",
            ProjectError::WorkingCopyDirty => "PROJ_WORKING_COPY_DIRTY",
            ProjectError::TooManyAttempts => "PROJ_TOO_MANY_ATTEMPTS",
            ProjectError::TaskNotFound => "PROJ_TASK_NOT_FOUND",
            ProjectError::TaskStateInvalid => "PROJ_TASK_STATE_INVALID",
            ProjectError::WorktreeFailed => "PROJ_WORKTREE_FAILED",
            ProjectError::EntryNotFound => "PROJ_ENTRY_NOT_FOUND",
            ProjectError::DecisionInvalid => "PROJ_DECISION_INVALID",
            ProjectError::CheckDenied { .. } => "PROJ_CHECK_DENIED",
            ProjectError::CheckTimeout => "PROJ_CHECK_TIMEOUT",
            ProjectError::LspUnavailable => "PROJ_LSP_UNAVAILABLE",
            ProjectError::LspMethodNotAllowed => "PROJ_LSP_METHOD_NOT_ALLOWED",
            ProjectError::ServerInstallFailed => "PROJ_SERVER_INSTALL_FAILED",
            ProjectError::IdeNotFound => "PROJ_IDE_NOT_FOUND",
            ProjectError::IdeStartFailed => "PROJ_IDE_START_FAILED",
            ProjectError::TerminalFailed => "PROJ_TERMINAL_FAILED",
            ProjectError::GitFailed => "PROJ_GIT_FAILED",
            ProjectError::FilesFailed => "PROJ_FILES_FAILED",
            ProjectError::TooManyFollowed => "PROJ_TOO_MANY_FOLLOWED",
            ProjectError::DatastoreUnavailable => "PROJ_DATASTORE_UNAVAILABLE",
            ProjectError::Invalid(code) => code,
        }
    }

    /// The Error Table's status column: true for `Err(retryable)`.
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            ProjectError::RemoteUnreachable
                | ProjectError::RateLimited
                | ProjectError::PushRefused
                | ProjectError::WorktreeFailed
                | ProjectError::LspUnavailable
                | ProjectError::ServerInstallFailed
                | ProjectError::IdeStartFailed
                | ProjectError::TerminalFailed
                | ProjectError::FilesFailed
                | ProjectError::DatastoreUnavailable
        )
    }
}

impl CodedError for ProjectError {
    fn code(&self) -> &'static str {
        ProjectError::code(self)
    }

    fn retryable(&self) -> bool {
        ProjectError::retryable(self)
    }

    /// The `action` column of the Error Table.
    fn repair(&self) -> Option<RepairAction> {
        match self {
            ProjectError::SignInNeeded => Some(RepairAction::SignInAgain),
            ProjectError::RemoteUnreachable
            | ProjectError::PushRefused
            | ProjectError::WorktreeFailed
            | ProjectError::IdeStartFailed
            | ProjectError::TerminalFailed
            | ProjectError::FilesFailed
            | ProjectError::DatastoreUnavailable => Some(RepairAction::TryAgain),
            ProjectError::ServerInstallFailed | ProjectError::GitFailed => None,
            _ => None,
        }
    }
}

/// Any statement failure on this module's tables is `PROJ_DATASTORE_UNAVAILABLE`.
impl From<sqlx::Error> for ProjectError {
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
        ProjectError::DatastoreUnavailable
    }
}

/// The failures of [`crate::GithubAccess`].
#[derive(Debug, Clone, thiserror::Error)]
pub enum GithubError {
    #[error("GH_SIGN_IN_NEEDED")]
    SignInNeeded,
    /// Not found or no access.
    #[error("GH_NOT_FOUND")]
    NotFound,
    #[error("GH_RATE_LIMITED")]
    RateLimited { retry_after_s: u32 },
    #[error("GH_UNREACHABLE")]
    Unreachable,
    /// `update_file` with a stale sha, or a branch that already exists.
    #[error("GH_CONFLICT")]
    Conflict { current_sha: String },
    /// A host that is neither GitHub nor the company's GitLab.
    #[error("GH_HOST_NOT_SUPPORTED")]
    HostNotSupported,
    /// The tool's first error line, no token.
    #[error("GH_FAILED")]
    Other(String),
}

impl GithubError {
    /// The registry code.
    pub fn code(&self) -> &'static str {
        match self {
            GithubError::SignInNeeded => "GH_SIGN_IN_NEEDED",
            GithubError::NotFound => "GH_NOT_FOUND",
            GithubError::RateLimited { .. } => "GH_RATE_LIMITED",
            GithubError::Unreachable => "GH_UNREACHABLE",
            GithubError::Conflict { .. } => "GH_CONFLICT",
            GithubError::HostNotSupported => "GH_HOST_NOT_SUPPORTED",
            GithubError::Other(_) => "GH_FAILED",
        }
    }

    /// The Error Table's status column: true for `Err(retryable)`.
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            GithubError::RateLimited { .. } | GithubError::Unreachable
        )
    }
}

impl CodedError for GithubError {
    fn code(&self) -> &'static str {
        GithubError::code(self)
    }

    fn retryable(&self) -> bool {
        GithubError::retryable(self)
    }

    /// The `action` column of the Error Table.
    fn repair(&self) -> Option<RepairAction> {
        match self {
            GithubError::SignInNeeded => Some(RepairAction::SignInAgain),
            GithubError::RateLimited { .. } | GithubError::Unreachable => {
                Some(RepairAction::TryAgain)
            }
            GithubError::Other(_) => None,
            GithubError::NotFound
            | GithubError::Conflict { .. }
            | GithubError::HostNotSupported => None,
        }
    }
}

/// A file-service failure is `PROJ_FILES_FAILED`; the file code goes to the log only.
pub(crate) fn file_failed(error: ordin_files::FileError) -> ProjectError {
    tracing::warn!(
        component = crate::log::COMPONENT,
        code = error.code(),
        "files_failed"
    );
    ProjectError::FilesFailed
}
