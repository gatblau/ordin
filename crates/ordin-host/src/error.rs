//! The Ordin Host's error type and the codes it owns (OrdinHost.md *Errors* and *Error Table*).

use ordin_types::{CodedError, RepairAction};

/// Every code the registry gives this crate (its `OH_`, `WFF_`, `VOICE_`, `CHRT_` and `BLOCK_` rows).
pub const ALL_CODES: &[&str] = &[
    "BLOCK_REFUSED",
    "CHRT_EXTERNAL_NOT_MEMBER",
    "CHRT_GUARDRAIL_ABOVE_MAX",
    "CHRT_HANDOFF_INVALID",
    "CHRT_MEMBER_UNKNOWN",
    "CHRT_NAME_INVALID",
    "CHRT_NOT_STARTED_BY_PERSON",
    "CHRT_PURPOSE_INVALID",
    "CHRT_ROLE_INVALID",
    "OH_AGENT_MISSING",
    "OH_AGENT_UNKNOWN",
    "OH_ATTACHMENTS_INVALID",
    "OH_BLOCK_ANSWERED",
    "OH_BLOCK_UNKNOWN",
    "OH_CHARTER_UNKNOWN",
    "OH_CHECK_FAILED",
    "OH_COMMAND_UNKNOWN",
    "OH_CONVERSATION_NOT_CONTINUABLE",
    "OH_CONVERSATION_UNKNOWN",
    "OH_COORDINATOR_NO_MOVE",
    "OH_COORDINATOR_SESSION_LOST",
    "OH_COORDINATOR_TIMEOUT",
    "OH_CREW_RUNNING",
    "OH_CREW_RUN_UNKNOWN",
    "OH_CREW_TOO_MANY_MEMBERS",
    "OH_DASHBOARD_UNKNOWN",
    "OH_DATASTORE_UNAVAILABLE",
    "OH_DECISION_KIND_MISMATCH",
    "OH_DEPENDENCY_FAILED",
    "OH_DRAFT_RUNNING",
    "OH_FOLDER_NOT_LINKED",
    "OH_INPUT_INVALID",
    "OH_INPUT_MISSING",
    "OH_INPUT_UNREADABLE",
    "OH_MESSAGE_INVALID",
    "OH_NAME_SUFFIXED",
    "OH_NEEDS_AGENTS",
    "OH_NOTHING_TO_DRAFT",
    "OH_NOT_COORDINATING",
    "OH_NOT_ORDIN_SESSION",
    "OH_NOT_PERSONS_WORDS",
    "OH_PATH_UNKNOWN",
    "OH_REMOTE_AGENT_IN_WORKFLOW",
    "OH_REMOVE_NOT_CONFIRMED",
    "OH_REPOSITORY_UNREACHABLE",
    "OH_RESTORE_SAME",
    "OH_RUN_NOT_INTERRUPTED",
    "OH_RUN_NOT_REPEATABLE",
    "OH_RUN_UNKNOWN",
    "OH_SESSION_START_FAILED",
    "OH_STALE_VERSION",
    "OH_SUMMARY_FAILED",
    "OH_TEMPLATE_UNKNOWN",
    "OH_TOO_MANY_WAITING",
    "OH_VERSION_UNKNOWN",
    "OH_WORKFLOW_UNKNOWN",
    "OH_WORKSPACE_UNKNOWN",
    "VOICE_FIXED_BY_COMPANY",
    "VOICE_NAME_TOO_LONG",
    "VOICE_NOTES_TOO_LONG",
    "WFF_AGENTS_INVALID",
    "WFF_AGENT_ID_INVALID",
    "WFF_AGENT_KEY_INVALID",
    "WFF_AGENT_KEY_UNKNOWN",
    "WFF_AGENT_NAME_INVALID",
    "WFF_AGENT_ROLE_INVALID",
    "WFF_AGENT_UNMATCHED",
    "WFF_AGENT_VERSION_INVALID",
    "WFF_CALL_CYCLE",
    "WFF_CALL_TOO_DEEP",
    "WFF_CALL_UNKNOWN",
    "WFF_CHANGE_NOTE_INVALID",
    "WFF_CHECK_FAILED",
    "WFF_COMMIT_INVALID",
    "WFF_CONDITION_INVALID",
    "WFF_FIELD_UNKNOWN",
    "WFF_FILE_MALFORMED",
    "WFF_FILE_TOO_LARGE",
    "WFF_FORMAT_NEWER",
    "WFF_FORMAT_UNSUPPORTED",
    "WFF_GROUP_TOO_LARGE",
    "WFF_IF_PATHS_INVALID",
    "WFF_INPUT_INVALID",
    "WFF_INPUT_LABEL_INVALID",
    "WFF_LIMIT_ABOVE_MAX",
    "WFF_LIMIT_INVALID",
    "WFF_LIMIT_LOWERED",
    "WFF_LINE_INVALID",
    "WFF_LOOP_LIST_INVALID",
    "WFF_LOOP_MAX_INVALID",
    "WFF_NAME_DUPLICATE",
    "WFF_NAME_EMPTY",
    "WFF_NAME_INVALID",
    "WFF_NAME_TOO_LONG",
    "WFF_ORIGIN_INVALID",
    "WFF_POSITION_INVALID",
    "WFF_PURPOSE_INVALID",
    "WFF_REPOSITORY_INVALID",
    "WFF_SEND_WITHOUT_ASK",
    "WFF_TOO_DEEP",
    "WFF_TOO_MANY_STEPS",
    "WFF_USE_NOT_ON_PATH",
    "WFF_VERSION_GAP",
];

/// The report of a check that refused a save (`OH_CHECK_FAILED`); restated from the Service API.
pub use ordin_service_api::commands::workflows::WorkflowCheckReport;

#[derive(Debug, Clone, thiserror::Error)]
pub enum OhError {
    #[error("workspace unknown")]
    WorkspaceUnknown,
    #[error("input invalid")]
    InputInvalid { code: &'static str, at: String },
    #[error("session start failed")]
    SessionStartFailed { cause: String, retryable: bool },
    #[error("too many waiting messages")]
    TooManyWaiting,
    #[error("a crew is running")]
    CrewRunning,
    #[error("input unreadable")]
    InputUnreadable { label: String },
    #[error("not coordinating")]
    NotCoordinating,
    #[error("conversation unknown")]
    ConversationUnknown,
    #[error("conversation cannot be continued")]
    NotContinuable,
    #[error("charter unknown")]
    CharterUnknown,
    #[error("workflow unknown")]
    WorkflowUnknown,
    #[error("version unknown")]
    VersionUnknown,
    #[error("stale base version")]
    StaleVersion { latest: u32 },
    #[error("check failed")]
    CheckFailed { report: WorkflowCheckReport },
    #[error("file refused")]
    FileRefused { code: &'static str },
    #[error("block unknown")]
    BlockUnknown,
    #[error("block already answered")]
    BlockAnswered,
    #[error("dashboard unknown")]
    DashboardUnknown,
    #[error("crew run unknown")]
    CrewRunUnknown,
    #[error("input missing")]
    InputMissing { label: String },
    #[error("workflow needs agents")]
    NeedsAgents,
    #[error("template unknown")]
    TemplateUnknown,
    #[error("not confirmed")]
    NotConfirmed,
    #[error("folder not linked")]
    FolderNotLinked,
    #[error("dependency failed")]
    DependencyFailed {
        dependency: &'static str,
        code: String,
        retryable: bool,
    },
    #[error("datastore unavailable")]
    DatastoreUnavailable,
    #[error("voice setting fixed by company")]
    VoiceFixed,
    #[error("not the person's words")]
    NotPersonsWords,
    #[error("a language draft is running")]
    DraftRunning,
    #[error("nothing to draft")]
    NothingToDraft,
    /// OH-TA-1: the run is unknown, or of another workspace.
    #[error("run unknown")]
    RunUnknown,
    /// OH-TA-1: the run did not end `interrupted` or `failed`.
    #[error("run not interrupted")]
    RunNotInterrupted,
    /// OH-TA-1: the run is not a task an agent of the directory can be given again from here.
    #[error("run not repeatable")]
    RunNotRepeatable,
}

impl OhError {
    /// The registry code (Error Table). `DependencyFailed` answers `OH_DEPENDENCY_FAILED` and carries
    /// the dependency's own code in `code`, except where the Error Table names an `OH_` code for the
    /// row (`OH_SUMMARY_FAILED`), which it then answers.
    pub fn code(&self) -> String {
        let code: &str = match self {
            OhError::WorkspaceUnknown => "OH_WORKSPACE_UNKNOWN",
            OhError::InputInvalid { code, .. } => code,
            OhError::SessionStartFailed { .. } => "OH_SESSION_START_FAILED",
            OhError::TooManyWaiting => "OH_TOO_MANY_WAITING",
            OhError::CrewRunning => "OH_CREW_RUNNING",
            OhError::InputUnreadable { .. } => "OH_INPUT_UNREADABLE",
            OhError::NotCoordinating => "OH_NOT_COORDINATING",
            OhError::ConversationUnknown => "OH_CONVERSATION_UNKNOWN",
            OhError::NotContinuable => "OH_CONVERSATION_NOT_CONTINUABLE",
            OhError::CharterUnknown => "OH_CHARTER_UNKNOWN",
            OhError::WorkflowUnknown => "OH_WORKFLOW_UNKNOWN",
            OhError::VersionUnknown => "OH_VERSION_UNKNOWN",
            OhError::StaleVersion { .. } => "OH_STALE_VERSION",
            OhError::CheckFailed { .. } => "OH_CHECK_FAILED",
            OhError::FileRefused { code } => code,
            OhError::BlockUnknown => "OH_BLOCK_UNKNOWN",
            OhError::BlockAnswered => "OH_BLOCK_ANSWERED",
            OhError::DashboardUnknown => "OH_DASHBOARD_UNKNOWN",
            OhError::CrewRunUnknown => "OH_CREW_RUN_UNKNOWN",
            OhError::InputMissing { .. } => "OH_INPUT_MISSING",
            OhError::NeedsAgents => "OH_NEEDS_AGENTS",
            OhError::TemplateUnknown => "OH_TEMPLATE_UNKNOWN",
            OhError::NotConfirmed => "OH_REMOVE_NOT_CONFIRMED",
            OhError::FolderNotLinked => "OH_FOLDER_NOT_LINKED",
            OhError::DependencyFailed { code, .. } if code.starts_with("OH_") => code,
            OhError::DependencyFailed { .. } => "OH_DEPENDENCY_FAILED",
            OhError::DatastoreUnavailable => "OH_DATASTORE_UNAVAILABLE",
            OhError::VoiceFixed => "VOICE_FIXED_BY_COMPANY",
            OhError::NotPersonsWords => "OH_NOT_PERSONS_WORDS",
            OhError::DraftRunning => "OH_DRAFT_RUNNING",
            OhError::NothingToDraft => "OH_NOTHING_TO_DRAFT",
            OhError::RunUnknown => "OH_RUN_UNKNOWN",
            OhError::RunNotInterrupted => "OH_RUN_NOT_INTERRUPTED",
            OhError::RunNotRepeatable => "OH_RUN_NOT_REPEATABLE",
        };
        code.to_owned()
    }

    /// True when the same call may succeed if repeated unchanged later. `CrewRunning` is permanent
    /// in every row, `react_to_slack` included (OrdinHost.md Internal Logic step 46.3 and its Error
    /// Table row; ErrorHandling.md registry).
    pub fn retryable(&self) -> bool {
        match self {
            OhError::SessionStartFailed { retryable, .. } => *retryable,
            OhError::DependencyFailed { retryable, .. } => *retryable,
            OhError::TooManyWaiting | OhError::DatastoreUnavailable => true,
            _ => false,
        }
    }

    /// The sentence the Error Table gives the person, where this crate owns the words.
    pub fn message(&self) -> &'static str {
        match self {
            OhError::WorkspaceUnknown => "This workspace isn't here any more.",
            OhError::InputInvalid {
                code: "OH_MESSAGE_INVALID",
                ..
            } => "That message is too long; send it in parts.",
            OhError::InputInvalid {
                code: "OH_ATTACHMENTS_INVALID",
                ..
            } => "Attach at most 20 files to one message.",
            OhError::TooManyWaiting => "Ordin is still answering; try again in a moment.",
            OhError::SessionStartFailed { .. } => "Ordin couldn't start.",
            OhError::CrewRunning => "Wait for the crew to finish before starting fresh.",
            OhError::DependencyFailed { code, .. } if code == "OH_SUMMARY_FAILED" => {
                "Ordin couldn't summarise just now."
            }
            OhError::ConversationUnknown => "That conversation isn't here any more.",
            OhError::NotContinuable => {
                "This conversation was continued later; continue the later one."
            }
            OhError::StaleVersion { .. } => {
                "This workflow changed since you opened it; reopen it to see the latest."
            }
            OhError::BlockAnswered => "This was already answered.",
            OhError::FileRefused {
                code: "WFF_FORMAT_NEWER",
            } => "This workflow was made by a newer ordin. Update ordin to import it.",
            OhError::FileRefused { .. } => "This file isn't a workflow ordin can read.",
            OhError::VoiceFixed => "Your company sets this.",
            OhError::DatastoreUnavailable => "Something went wrong; try again.",
            OhError::DraftRunning => {
                "Ordin is already drafting a language. Wait for it to finish or stop it."
            }
            OhError::NothingToDraft => "Every message of that language already has its wording.",
            OhError::RunUnknown => "That work isn't in this workspace any more.",
            OhError::RunNotInterrupted => {
                "Only work that was interrupted or couldn't finish can be tried again."
            }
            OhError::RunNotRepeatable => {
                "This work can't be started again from here. Ask ordin in the conversation instead."
            }
            OhError::DependencyFailed { .. } => "That did not work; try again.",
            _ => "",
        }
    }

    pub(crate) fn input(code: &'static str, at: impl Into<String>) -> Self {
        OhError::InputInvalid {
            code,
            at: at.into(),
        }
    }

    pub(crate) fn dependency(dependency: &'static str, e: &crate::ports::DepError) -> Self {
        OhError::DependencyFailed {
            dependency,
            code: e.code.clone(),
            retryable: e.retryable,
        }
    }
}

impl CodedError for OhError {
    fn code(&self) -> &'static str {
        let code = OhError::code(self);
        ALL_CODES
            .iter()
            .find(|c| **c == code)
            .copied()
            .unwrap_or("OH_DEPENDENCY_FAILED")
    }

    fn retryable(&self) -> bool {
        OhError::retryable(self)
    }

    fn repair(&self) -> Option<RepairAction> {
        match self {
            OhError::TooManyWaiting
            | OhError::SessionStartFailed { .. }
            | OhError::DatastoreUnavailable
            | OhError::DependencyFailed { .. } => Some(RepairAction::TryAgain),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for OhError {
    fn from(error: sqlx::Error) -> Self {
        tracing::warn!(
            component = "ordin_host",
            code = "OH_DATASTORE_UNAVAILABLE",
            error_class = error_class(&error),
            "ordin_host.datastore"
        );
        OhError::DatastoreUnavailable
    }
}

/// A short class of a database error, for the log line: never the error's text, which can quote a
/// statement's values or stored workflow and charter text (OrdinHost.md Security, "Never logged").
fn error_class(error: &sqlx::Error) -> &'static str {
    match error {
        sqlx::Error::Database(_) => "database",
        sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed => "pool",
        sqlx::Error::Io(_) => "io",
        sqlx::Error::RowNotFound => "row_not_found",
        sqlx::Error::ColumnDecode { .. } | sqlx::Error::Decode(_) => "decode",
        _ => "other",
    }
}
