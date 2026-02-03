//! The adapter's public error and its `CPA_` codes (CopilotLocalAdapter.md *Error Table*).

use ordin_runtime_adapter::AdapterError;
use ordin_types::CodedError;

/// Every code the registry gives this crate, sorted. Most are internal: journalled and logged, with
/// a 2C `AdapterError` at the trait boundary.
pub const ALL_CODES: &[&str] = &[
    "CPA_AGENT_FOLDER_MISSING",
    "CPA_ALLOWANCE_EXHAUSTED",
    "CPA_AUTH_STATUS_UNREADABLE",
    "CPA_CONFIG_INVALID",
    "CPA_DEADLINE",
    "CPA_GATE_FAILED",
    "CPA_HARNESS_UNTESTED",
    "CPA_HOOK_OPEN_FAILED",
    "CPA_INPUT_INVALID",
    "CPA_MCP_OPEN_FAILED",
    "CPA_NOT_PAUSED",
    "CPA_PROBE_FAILED",
    "CPA_PROFILE_NOT_OFFERED",
    "CPA_RESUME_MISMATCH",
    "CPA_RULES_UNAVAILABLE",
    "CPA_RUN_START_ENDED",
    "CPA_SEND_FAILED",
    "CPA_SERVER_EXITED",
    "CPA_SERVER_NAME_CLASH",
    "CPA_SERVER_SIGN_IN_UNCONFIRMED",
    "CPA_SERVER_START_FAILED",
    "CPA_SESSION_CREATE_FAILED",
    "CPA_SESSION_ERROR",
    "CPA_SESSION_EXISTS",
    "CPA_SESSION_NOT_RESUMABLE",
    "CPA_SETUP_FAILED",
    "CPA_SIGNED_OUT",
    "CPA_STOP_DEADLINE",
    "CPA_TARGET_MISMATCH",
    "CPA_UNKNOWN_RUN",
];

#[derive(Debug, thiserror::Error)]
pub enum CopilotAdapterError {
    #[error("Copilot is not offered in this profile")]
    ProfileNotOffered,
    #[error("Copilot sign-in on a server is not confirmed")]
    ServerSignInUnconfirmed,
    #[error("adapter configuration invalid: {0}")]
    ConfigInvalid(&'static str),
    #[error("Copilot CLI server unreachable: {code}")]
    PortFailed { code: &'static str, retryable: bool },
}

impl CopilotAdapterError {
    /// The registry code (Error Table).
    pub fn code(&self) -> &'static str {
        match self {
            CopilotAdapterError::ProfileNotOffered => "CPA_PROFILE_NOT_OFFERED",
            CopilotAdapterError::ServerSignInUnconfirmed => "CPA_SERVER_SIGN_IN_UNCONFIRMED",
            CopilotAdapterError::ConfigInvalid(_) => "CPA_CONFIG_INVALID",
            CopilotAdapterError::PortFailed { code, .. } => code,
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(
            self,
            CopilotAdapterError::PortFailed {
                retryable: true,
                ..
            }
        )
    }

    /// `CPA_PROBE_FAILED`, retryable (Error Table; CPA-CPX-4).
    pub(crate) fn probe_failed() -> CopilotAdapterError {
        CopilotAdapterError::PortFailed {
            code: "CPA_PROBE_FAILED",
            retryable: true,
        }
    }
}

impl CodedError for CopilotAdapterError {
    fn code(&self) -> &'static str {
        CopilotAdapterError::code(self)
    }

    fn retryable(&self) -> bool {
        CopilotAdapterError::retryable(self)
    }
}

/// A failed `start` or `resume` step: the internal code and retryability the Error Table gives the
/// row, and the 2C error the trait returns.
#[derive(Debug)]
pub(crate) struct Failure {
    pub code: &'static str,
    pub retryable: bool,
    pub error: AdapterError,
}

impl Failure {
    pub(crate) fn new(code: &'static str, retryable: bool, error: AdapterError) -> Failure {
        Failure {
            code,
            retryable,
            error,
        }
    }

    /// A row answered with `AdapterError::Unreachable`.
    pub(crate) fn unreachable(code: &'static str, retryable: bool) -> Failure {
        Failure::new(code, retryable, AdapterError::Unreachable)
    }

    /// A permanent row answered with `AdapterError::InvalidRequest`.
    pub(crate) fn invalid(code: &'static str, ra_code: &str) -> Failure {
        Failure::new(
            code,
            false,
            AdapterError::InvalidRequest {
                code: ra_code.to_owned(),
            },
        )
    }
}
