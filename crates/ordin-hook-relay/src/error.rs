//! The Hook Relay's one error type and the codes the registry gives this crate (HookRelay.md
//! *Error Table*; ErrorHandling.md *Internal Logic* 11).

use ordin_types::CodedError;

/// Every code the registry gives this crate. The first five are raised as a returned error; the
/// rest are the reasons a hook call is denied (hook protocol: the code is logged or journalled and
/// never sent, so the reply carries only the fixed reason). `HOOK_APPROVAL_PENDING` is the one code
/// both ends raise: the relay at its 50 s hold, the hook at its own deadline (EH-D10).
pub const ALL_CODES: &[&str] = &[
    "HOOK_APPROVAL_PENDING",
    "HOOK_APPROVAL_UNAVAILABLE",
    "HOOK_DECISION_TIMEOUT",
    "HOOK_ENDPOINT_TOO_LONG",
    "HOOK_INTERNAL",
    "HOOK_LISTEN_FAILED",
    "HOOK_MALFORMED",
    "HOOK_PEER_REFUSED",
    "HOOK_PROTOCOL_UNSUPPORTED",
    "HOOK_RANDOM_UNAVAILABLE",
    "HOOK_REPLAYED",
    "HOOK_SESSION_EXISTS",
    "HOOK_SESSION_MISMATCH",
    "HOOK_SESSION_UNKNOWN",
    "HOOK_STORE_UNAVAILABLE",
    "HOOK_UNAUTHENTICATED",
];

/// What the relay returns to its callers (the programs and the two local adapters).
#[derive(Debug, thiserror::Error)]
pub enum HookRelayError {
    #[error("channel could not be opened: {0}")]
    Listen(#[source] std::io::Error),
    #[error("endpoint path too long")]
    EndpointTooLong,
    #[error("session already open")]
    SessionExists,
    #[error("random source unavailable")]
    Random,
    #[error("datastore unavailable")]
    Store(#[source] sqlx::Error),
}

impl HookRelayError {
    /// The registry code (Error Table).
    pub fn code(&self) -> &'static str {
        match self {
            HookRelayError::Listen(_) => "HOOK_LISTEN_FAILED",
            HookRelayError::EndpointTooLong => "HOOK_ENDPOINT_TOO_LONG",
            HookRelayError::SessionExists => "HOOK_SESSION_EXISTS",
            HookRelayError::Random => "HOOK_RANDOM_UNAVAILABLE",
            HookRelayError::Store(_) => "HOOK_STORE_UNAVAILABLE",
        }
    }

    /// True for the two rows the Error Table marks `Err(retryable)`: no randomness from the
    /// operating system, and an unavailable store.
    pub fn retryable(&self) -> bool {
        matches!(self, HookRelayError::Random | HookRelayError::Store(_))
    }
}

impl CodedError for HookRelayError {
    fn code(&self) -> &'static str {
        HookRelayError::code(self)
    }

    fn retryable(&self) -> bool {
        HookRelayError::retryable(self)
    }
}

impl From<sqlx::Error> for HookRelayError {
    fn from(e: sqlx::Error) -> Self {
        HookRelayError::Store(e)
    }
}
