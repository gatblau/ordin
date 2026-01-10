//! `CredError`, the one error type of this crate (IdentityAndCredentials.md *Errors*, *Error Table*).

use ordin_types::{CodedError, RepairAction};

/// Every code this crate owns (ErrorHandling.md *Internal Logic* 11): the rows of the Error Table.
pub const ALL_CODES: &[&str] = &[
    "CRED_AMBIGUOUS",
    "CRED_BROWSER_FAILED",
    "CRED_CERTIFICATE_EXPIRED",
    "CRED_CERTIFICATE_INVALID",
    "CRED_CHALLENGE_UNKNOWN",
    "CRED_CONFIG_INVALID",
    "CRED_DUPLICATE",
    "CRED_FLOW_NOT_OFFERED",
    "CRED_KEYCHAIN_MISSING",
    "CRED_KEYCHAIN_UNAVAILABLE",
    "CRED_KIND_HAS_NO_TOKEN",
    "CRED_KIND_MISMATCH",
    "CRED_LABEL_INVALID",
    "CRED_LOOPBACK_BUSY",
    "CRED_NOT_FOUND",
    "CRED_PASSWORD_INVALID",
    "CRED_PROVIDER_NOT_CONFIGURED",
    "CRED_PROVIDER_REFUSED",
    "CRED_PROVIDER_UNREACHABLE",
    "CRED_REFRESH_REFUSED",
    "CRED_RELAY_STATE_MISMATCH",
    "CRED_SCOPE_MISSING",
    "CRED_SECRET_IN_RECORD",
    "CRED_SECRET_KEY_CONFLICT",
    "CRED_SECRET_KEY_EXISTS",
    "CRED_SECRET_KEY_INVALID",
    "CRED_SECRET_KEY_MISSING",
    "CRED_SECRET_NAME_INVALID",
    "CRED_SECRET_STORE_ABSENT",
    "CRED_SECRET_STORE_CORRUPT",
    "CRED_SECRET_STORE_WRITE_FAILED",
    "CRED_SECRET_TOO_LARGE",
    "CRED_SIGN_IN_CANCELLED",
    "CRED_SIGN_IN_REQUIRED",
    "CRED_SIGN_IN_TIMEOUT",
    "CRED_STORE_FAILED",
    "CRED_SYSTEMD_CREDS_FAILED",
    "CRED_TOO_MANY_SIGN_INS",
    "CRED_URL_INVALID",
];

/// The codes whose Error Table status is `Err(retryable)`; every other code is permanent.
const RETRYABLE: &[&str] = &[
    "CRED_BROWSER_FAILED",
    "CRED_KEYCHAIN_UNAVAILABLE",
    "CRED_LOOPBACK_BUSY",
    "CRED_PROVIDER_UNREACHABLE",
    "CRED_SECRET_STORE_WRITE_FAILED",
    "CRED_SIGN_IN_TIMEOUT",
    "CRED_STORE_FAILED",
    "CRED_TOO_MANY_SIGN_INS",
];

/// The codes whose *Response Body* action is `sign_in_again`.
const SIGN_IN_AGAIN: &[&str] = &[
    "CRED_KEYCHAIN_MISSING",
    "CRED_RELAY_STATE_MISMATCH",
    "CRED_SCOPE_MISSING",
    "CRED_SIGN_IN_CANCELLED",
    "CRED_SIGN_IN_REQUIRED",
    "CRED_SIGN_IN_TIMEOUT",
];

/// The codes whose *Response Body* action is `send_to_it`.
///
/// SPEC-GAP-1: the `repair` doc comment lists four codes for `SendToIt` and says it follows the
/// *Response Body* column, which gives `send_to_it` to `CRED_CERTIFICATE_EXPIRED` as well. The column
/// is followed, so the fifth code is here.
const SEND_TO_IT: &[&str] = &[
    "CRED_CERTIFICATE_EXPIRED",
    "CRED_LABEL_INVALID",
    "CRED_PROVIDER_NOT_CONFIGURED",
    "CRED_PROVIDER_REFUSED",
    "CRED_URL_INVALID",
];

/// Every failure of this crate. `Display` writes the code only, never a provider message, token or path.
#[derive(Debug, thiserror::Error)]
pub enum CredError {
    #[error("{code}")]
    Retryable { code: &'static str },
    #[error("{code}")]
    Permanent { code: &'static str },
}

impl CredError {
    /// The error of `code`, retryable or permanent as the Error Table states.
    pub(crate) fn of(code: &'static str) -> Self {
        if RETRYABLE.contains(&code) {
            CredError::Retryable { code }
        } else {
            CredError::Permanent { code }
        }
    }
}

impl CodedError for CredError {
    fn code(&self) -> &'static str {
        match self {
            CredError::Retryable { code } | CredError::Permanent { code } => code,
        }
    }

    fn retryable(&self) -> bool {
        matches!(self, CredError::Retryable { .. })
    }

    /// `SignInAgain`, `TryAgain` (every other retryable code) or `SendToIt`, as the *Response Body*
    /// column of the Error Table gives; `None` otherwise.
    fn repair(&self) -> Option<RepairAction> {
        let code = self.code();
        if SIGN_IN_AGAIN.contains(&code) {
            Some(RepairAction::SignInAgain)
        } else if self.retryable() {
            Some(RepairAction::TryAgain)
        } else if SEND_TO_IT.contains(&code) {
            Some(RepairAction::SendToIt)
        } else {
            None
        }
    }
}

/// The codes, as constants, so a misspelt code fails to compile.
pub(crate) mod code {
    pub const AMBIGUOUS: &str = "CRED_AMBIGUOUS";
    pub const BROWSER_FAILED: &str = "CRED_BROWSER_FAILED";
    pub const CERTIFICATE_EXPIRED: &str = "CRED_CERTIFICATE_EXPIRED";
    pub const CERTIFICATE_INVALID: &str = "CRED_CERTIFICATE_INVALID";
    pub const CHALLENGE_UNKNOWN: &str = "CRED_CHALLENGE_UNKNOWN";
    pub const CONFIG_INVALID: &str = "CRED_CONFIG_INVALID";
    pub const DUPLICATE: &str = "CRED_DUPLICATE";
    pub const FLOW_NOT_OFFERED: &str = "CRED_FLOW_NOT_OFFERED";
    pub const KEYCHAIN_MISSING: &str = "CRED_KEYCHAIN_MISSING";
    pub const KEYCHAIN_UNAVAILABLE: &str = "CRED_KEYCHAIN_UNAVAILABLE";
    pub const KIND_HAS_NO_TOKEN: &str = "CRED_KIND_HAS_NO_TOKEN";
    pub const KIND_MISMATCH: &str = "CRED_KIND_MISMATCH";
    pub const LABEL_INVALID: &str = "CRED_LABEL_INVALID";
    pub const LOOPBACK_BUSY: &str = "CRED_LOOPBACK_BUSY";
    pub const NOT_FOUND: &str = "CRED_NOT_FOUND";
    pub const PASSWORD_INVALID: &str = "CRED_PASSWORD_INVALID";
    pub const PROVIDER_NOT_CONFIGURED: &str = "CRED_PROVIDER_NOT_CONFIGURED";
    pub const PROVIDER_REFUSED: &str = "CRED_PROVIDER_REFUSED";
    pub const PROVIDER_UNREACHABLE: &str = "CRED_PROVIDER_UNREACHABLE";
    pub const REFRESH_REFUSED: &str = "CRED_REFRESH_REFUSED";
    pub const RELAY_STATE_MISMATCH: &str = "CRED_RELAY_STATE_MISMATCH";
    pub const SCOPE_MISSING: &str = "CRED_SCOPE_MISSING";
    pub const SECRET_IN_RECORD: &str = "CRED_SECRET_IN_RECORD";
    pub const SECRET_KEY_CONFLICT: &str = "CRED_SECRET_KEY_CONFLICT";
    pub const SECRET_KEY_EXISTS: &str = "CRED_SECRET_KEY_EXISTS";
    pub const SECRET_KEY_INVALID: &str = "CRED_SECRET_KEY_INVALID";
    pub const SECRET_KEY_MISSING: &str = "CRED_SECRET_KEY_MISSING";
    pub const SECRET_NAME_INVALID: &str = "CRED_SECRET_NAME_INVALID";
    pub const SECRET_STORE_ABSENT: &str = "CRED_SECRET_STORE_ABSENT";
    pub const SECRET_STORE_CORRUPT: &str = "CRED_SECRET_STORE_CORRUPT";
    pub const SECRET_STORE_WRITE_FAILED: &str = "CRED_SECRET_STORE_WRITE_FAILED";
    pub const SECRET_TOO_LARGE: &str = "CRED_SECRET_TOO_LARGE";
    pub const SIGN_IN_CANCELLED: &str = "CRED_SIGN_IN_CANCELLED";
    pub const SIGN_IN_REQUIRED: &str = "CRED_SIGN_IN_REQUIRED";
    pub const SIGN_IN_TIMEOUT: &str = "CRED_SIGN_IN_TIMEOUT";
    pub const STORE_FAILED: &str = "CRED_STORE_FAILED";
    pub const SYSTEMD_CREDS_FAILED: &str = "CRED_SYSTEMD_CREDS_FAILED";
    pub const TOO_MANY_SIGN_INS: &str = "CRED_TOO_MANY_SIGN_INS";
    pub const URL_INVALID: &str = "CRED_URL_INVALID";
}

/// Shorthand: `Err(CredError::of(code))`.
pub(crate) fn fail<T>(code: &'static str) -> Result<T, CredError> {
    Err(CredError::of(code))
}
