//! The adapter's public error and its `CLA_` codes (ClaudeLocalAdapter.md *Error Table*;
//! ErrorHandling.md *Internal Logic* 11).

use ordin_runtime_adapter::AdapterError;
use ordin_types::CodedError;

/// Every code the registry gives this crate, sorted. Most are internal: journalled and logged, with
/// a 2C `AdapterError` at the trait boundary.
pub const ALL_CODES: &[&str] = &[
    "CLA_AGENT_FOLDER_MISSING",
    "CLA_ALLOWANCE_EXHAUSTED",
    "CLA_AUTH_STATUS_UNREADABLE",
    "CLA_CONFIG_INVALID",
    "CLA_CONFIG_WRITE_FAILED",
    "CLA_DEADLINE",
    "CLA_ENV_RESERVED",
    "CLA_HARNESS_EXITED",
    "CLA_HARNESS_NOT_FOUND",
    "CLA_HARNESS_UNTESTED",
    "CLA_HOOK_OPEN_FAILED",
    "CLA_INPUT_INVALID",
    "CLA_MCP_OPEN_FAILED",
    "CLA_NOT_PAUSED",
    "CLA_RESUME_MISMATCH",
    "CLA_RULES_UNAVAILABLE",
    "CLA_RUN_START_ENDED",
    "CLA_SERVER_NAME_CLASH",
    "CLA_SESSION_EXISTS",
    "CLA_SESSION_RECORD_MISSING",
    "CLA_SESSION_RECORD_SAVE_FAILED",
    "CLA_SETUP_FAILED",
    "CLA_SIGNED_OUT",
    "CLA_SIGN_IN_FAILED",
    "CLA_SIGN_IN_STATUS_FAILED",
    "CLA_SPAWN_FAILED",
    "CLA_STOP_DEADLINE",
    "CLA_STREAM_LINE_TOO_LONG",
    "CLA_STREAM_MALFORMED",
    "CLA_TARGET_MISMATCH",
    "CLA_TURN_FAILED",
    "CLA_UNKNOWN_RUN",
    "CLA_VERSION_CHECK_FAILED",
];

#[derive(Debug, thiserror::Error)]
pub enum ClaudeAdapterError {
    #[error("adapter configuration invalid: {0}")]
    ConfigInvalid(&'static str),
    #[error("sign-in status could not be read")]
    SignInStatusFailed,
    /// ADR-107. Names the variable, never its value.
    #[error("the variable {0} is set by ordin itself and cannot be declared")]
    EnvReserved(String),
}

impl ClaudeAdapterError {
    /// The registry code (Error Table).
    pub fn code(&self) -> &'static str {
        match self {
            ClaudeAdapterError::ConfigInvalid(_) => "CLA_CONFIG_INVALID",
            ClaudeAdapterError::SignInStatusFailed => "CLA_SIGN_IN_STATUS_FAILED",
            ClaudeAdapterError::EnvReserved(_) => "CLA_ENV_RESERVED",
        }
    }

    /// True for the one row marked `Err(retryable)`: the status could not be read.
    pub fn retryable(&self) -> bool {
        matches!(self, ClaudeAdapterError::SignInStatusFailed)
    }
}

impl CodedError for ClaudeAdapterError {
    fn code(&self) -> &'static str {
        ClaudeAdapterError::code(self)
    }

    fn retryable(&self) -> bool {
        ClaudeAdapterError::retryable(self)
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
