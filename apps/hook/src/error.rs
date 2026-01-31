//! `HookFailure`, the one failure type of the program (OrdinHook.md *Public Interface*, *Error Table*).

use ordin_types::CodedError;

/// Every code this program raises: the rows of the Error Table. `HOOK_APPROVAL_PENDING` is the one code
/// both ends of the protocol raise (the registry records the Hook Relay as its owner).
pub const ALL_CODES: &[&str] = &[
    "HOOK_APPROVAL_PENDING",
    "HOOK_BRIDGE_ENV_INVALID",
    "HOOK_BRIDGE_REFUSED",
    "HOOK_CORE_TIMEOUT",
    "HOOK_CORE_UNREACHABLE",
    "HOOK_ENV_INVALID",
    "HOOK_KEY_UNREADABLE",
    "HOOK_LAUNCH_FAILED",
    "HOOK_OUTPUT_FAILED",
    "HOOK_PAYLOAD_INVALID",
    "HOOK_REPLY_MALFORMED",
    "HOOK_REQUEST_TOO_LARGE",
    "HOOK_USAGE",
];

pub const USAGE: &str = "HOOK_USAGE";
pub const ENV_INVALID: &str = "HOOK_ENV_INVALID";
pub const PAYLOAD_INVALID: &str = "HOOK_PAYLOAD_INVALID";
pub const REQUEST_TOO_LARGE: &str = "HOOK_REQUEST_TOO_LARGE";
pub const CORE_UNREACHABLE: &str = "HOOK_CORE_UNREACHABLE";
pub const CORE_TIMEOUT: &str = "HOOK_CORE_TIMEOUT";
pub const REPLY_MALFORMED: &str = "HOOK_REPLY_MALFORMED";
pub const APPROVAL_PENDING: &str = "HOOK_APPROVAL_PENDING";
pub const OUTPUT_FAILED: &str = "HOOK_OUTPUT_FAILED";
pub const BRIDGE_ENV_INVALID: &str = "HOOK_BRIDGE_ENV_INVALID";
pub const BRIDGE_REFUSED: &str = "HOOK_BRIDGE_REFUSED";
pub const KEY_UNREADABLE: &str = "HOOK_KEY_UNREADABLE";
pub const LAUNCH_FAILED: &str = "HOOK_LAUNCH_FAILED";

/// A failure: the code and the reason the harness shows the model. Never carries a secret or a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookFailure {
    pub code: &'static str,
    pub reason: &'static str,
}

impl HookFailure {
    pub const fn new(code: &'static str, reason: &'static str) -> Self {
        HookFailure { code, reason }
    }

    pub const fn env_invalid() -> Self {
        Self::new(ENV_INVALID, "ordin is not set up for this session")
    }

    pub const fn payload_invalid() -> Self {
        Self::new(PAYLOAD_INVALID, "ordin could not read this tool call")
    }

    pub const fn request_too_large() -> Self {
        Self::new(
            REQUEST_TOO_LARGE,
            "this tool call is too large for ordin to check",
        )
    }

    pub const fn core_unreachable() -> Self {
        Self::new(CORE_UNREACHABLE, "ordin is not running")
    }

    pub const fn core_timeout() -> Self {
        Self::new(CORE_TIMEOUT, "ordin did not answer in time")
    }

    pub const fn reply_malformed() -> Self {
        Self::new(REPLY_MALFORMED, "ordin's answer could not be read")
    }

    pub const fn approval_pending() -> Self {
        Self::new(APPROVAL_PENDING, "waiting for the person's approval")
    }

    pub const fn bridge_env_invalid() -> Self {
        Self::new(BRIDGE_ENV_INVALID, "ordin is not set up for this session")
    }

    pub const fn output_failed() -> Self {
        Self::new(OUTPUT_FAILED, "ordin could not write its answer")
    }

    pub const fn key_unreadable() -> Self {
        Self::new(KEY_UNREADABLE, "the key could not be read")
    }
}

impl std::fmt::Display for HookFailure {
    /// The code only (ErrorHandling: no provider message, secret or path).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code)
    }
}

impl std::error::Error for HookFailure {}

impl CodedError for HookFailure {
    fn code(&self) -> &'static str {
        self.code
    }

    fn retryable(&self) -> bool {
        false
    }
}
