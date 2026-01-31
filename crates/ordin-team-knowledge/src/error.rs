//! The service's one error type and the codes it owns (TeamKnowledgeService.md *Error Table* and the
//! amendments' rows; ErrorHandling.md, prefix `TEAM_`).

use ordin_types::{CodedError, RepairAction};

/// Every code the registry gives this crate. The ADR-110 and ADR-112 codes are owned here but not
/// returned by the first build (ADR-121 item 9).
pub const ALL_CODES: &[&str] = &[
    "TEAM_ADOPT_INVALID",
    "TEAM_ADOPT_NOT_READ_ITEM",
    "TEAM_ADOPT_PERSONAL_DATA",
    "TEAM_ADOPT_REFUSED",
    "TEAM_ALREADY_ON",
    "TEAM_BUNDLE_LIMIT",
    "TEAM_CLONE_FAILED",
    "TEAM_COMPANY_FOLDER_NO_ACCESS",
    "TEAM_DATASTORE_UNAVAILABLE",
    "TEAM_DESCRIPTION_NOT_TAKEN",
    "TEAM_FACT_NOT_TAKEN",
    "TEAM_FILE_UNREADABLE",
    "TEAM_FOLDER_ALREADY_READ",
    "TEAM_FOLDER_IS_OWN",
    "TEAM_FOLDER_IS_READ",
    "TEAM_FOLDER_NOT_FOUND",
    "TEAM_FOLDER_SET_BY_COMPANY",
    "TEAM_FORMAT_NEWER",
    "TEAM_INDEX_NOT_OURS",
    "TEAM_ITEM_HELD_ELSEWHERE",
    "TEAM_LINK_NOT_SUITABLE",
    "TEAM_LOG_UNREADABLE",
    "TEAM_MEMORY_UNAVAILABLE",
    "TEAM_NOTHING_TO_SHARE",
    "TEAM_NOT_ADOPTED",
    "TEAM_NOT_ON",
    "TEAM_NO_ACCESS",
    "TEAM_NO_OWN_FOLDER",
    "TEAM_NO_SOURCE_CHANGE",
    "TEAM_PAGES_UNAVAILABLE",
    "TEAM_PUSH_FAILED",
    "TEAM_READ_LIMIT",
    "TEAM_READ_ONLY",
    "TEAM_REMOTE_UNREACHABLE",
    "TEAM_REVIEW_CANNOT_TELL",
    "TEAM_REVIEW_NOT_REQUIRED",
    "TEAM_SHARE_PENDING",
    "TEAM_SHARE_RACE",
    "TEAM_SHARE_UNKNOWN",
    "TEAM_SIGN_IN_NEEDED",
    "TEAM_WORKSPACE_NOT_FOUND",
    "TEAM_WS_AGENT_NEEDS_IT",
    "TEAM_WS_AGENT_NOT_ADDED",
    "TEAM_WS_AGENT_PACK_MISSING",
    "TEAM_WS_AGENT_UNSAFE",
    "TEAM_WS_ALLOW_FAILED",
    "TEAM_WS_ALREADY_OPEN",
    "TEAM_WS_ALREADY_TEAM",
    "TEAM_WS_DESCRIPTION_UNREADABLE",
    "TEAM_WS_FOLDER_TAKEN",
    "TEAM_WS_FORMAT_NEWER",
    "TEAM_WS_GRANT_NOT_FOUND",
    "TEAM_WS_NAME_IN_USE",
    "TEAM_WS_NOT_A_TEAM_WORKSPACE",
    "TEAM_WS_NOT_TEAM",
    "TEAM_WS_OPEN_FAILED",
    "TEAM_WS_REMOVAL_WAITS",
    "TEAM_WS_RESULTS_NOT_SUITABLE",
    "TEAM_WS_RESULTS_NO_ACCESS",
    "TEAM_WS_RESULT_KEPT",
    "TEAM_WS_RESULT_NOT_TAKEN",
    "TEAM_WS_RESULT_TOO_LARGE",
];

/// The codes the program's `TeamAgents` implementation returns for an `AddOutcome` that is not
/// `Ready` (Shared Context 3 of the team workspace amendment: owned by this crate, exported for the
/// program).
pub const TEAM_WS_AGENT_UNSAFE: &str = "TEAM_WS_AGENT_UNSAFE";
pub const TEAM_WS_AGENT_NEEDS_IT: &str = "TEAM_WS_AGENT_NEEDS_IT";
pub const TEAM_WS_AGENT_PACK_MISSING: &str = "TEAM_WS_AGENT_PACK_MISSING";

/// Every failure of a public method: the code, whether the same call may succeed later, and the one
/// repair action the person is offered.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{code}")]
pub struct TeamError {
    pub code: &'static str,
    pub retryable: bool,
    pub action: Option<RepairAction>,
}

impl TeamError {
    /// A permanent refusal with no repair action.
    pub(crate) fn permanent(code: &'static str) -> TeamError {
        TeamError {
            code,
            retryable: false,
            action: None,
        }
    }

    /// The code with the Error Table's own status and action.
    pub(crate) fn of(code: &'static str) -> TeamError {
        match code {
            "TEAM_SIGN_IN_NEEDED" => TeamError {
                code,
                retryable: false,
                action: Some(RepairAction::SignInAgain),
            },
            "TEAM_REMOTE_UNREACHABLE" | "TEAM_SHARE_RACE" | "TEAM_DATASTORE_UNAVAILABLE" => {
                TeamError {
                    code,
                    retryable: true,
                    action: Some(RepairAction::TryAgain),
                }
            }
            _ => TeamError::permanent(code),
        }
    }

    /// A code whose status follows its cause (`Err(retryable / permanent per cause)` rows).
    pub(crate) fn per_cause(code: &'static str, retryable: bool) -> TeamError {
        TeamError {
            code,
            retryable,
            action: Some(RepairAction::TryAgain),
        }
    }

    pub(crate) fn datastore() -> TeamError {
        TeamError::of("TEAM_DATASTORE_UNAVAILABLE")
    }
}

impl From<sqlx::Error> for TeamError {
    fn from(error: sqlx::Error) -> Self {
        tracing::warn!(target: "ordin::team_knowledge", error_class = error_class(&error), "team.datastore_failed");
        TeamError::datastore()
    }
}

fn error_class(error: &sqlx::Error) -> &'static str {
    match error {
        sqlx::Error::Database(_) => "database",
        sqlx::Error::Io(_) => "io",
        sqlx::Error::PoolTimedOut => "pool_timed_out",
        sqlx::Error::RowNotFound => "row_not_found",
        _ => "other",
    }
}

impl CodedError for TeamError {
    fn code(&self) -> &'static str {
        self.code
    }

    fn retryable(&self) -> bool {
        self.retryable
    }

    fn repair(&self) -> Option<RepairAction> {
        self.action
    }
}
