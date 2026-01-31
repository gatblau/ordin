//! The crate's error type and the codes it owns (SetupAssistant.md *Errors*, *Error Table* and the
//! ADR-114 amendment's rows; ErrorHandling.md *Internal Logic* 11).

use ordin_types::{CodedError, RepairAction};

/// Every code the registry gives this crate (prefixes `SETUP_`, `HARN_`), sorted.
// spec: ordin-desktop/phase-1-analysis#SA-ERR-01
pub const ALL_CODES: &[&str] = &[
    "HARN_DUPLICATE",
    "HARN_PATH_INVALID",
    "HARN_SIGN_IN_METHOD_INVALID",
    "HARN_STATE_INCONSISTENT",
    "HARN_VERSION_INVALID",
    "SETUP_AGENT_FOLDER_FAILED",
    "SETUP_CATALOGUE_FAILED",
    "SETUP_CRAFT_APP_FAILED",
    "SETUP_HARNESS_CHECK_FAILED",
    "SETUP_HARNESS_MISSING",
    "SETUP_HARNESS_UNKNOWN",
    "SETUP_HARNESS_UNTESTED",
    "SETUP_LANGUAGE_SERVER_DIGEST_MISMATCH",
    "SETUP_LANGUAGE_SERVER_INSTALL_FAILED",
    "SETUP_LANGUAGE_SERVER_NOT_APPROVED",
    "SETUP_LANGUAGE_SERVER_PLATFORM_UNSUPPORTED",
    "SETUP_NO_HARNESS_ACCESS",
    "SETUP_NO_PROFILE_ADDRESS",
    "SETUP_NO_READY_HARNESS",
    "SETUP_PROFILE_UNREACHABLE",
    "SETUP_PROGRAM_DAMAGED",
    "SETUP_PROGRAM_FAILED",
    "SETUP_PROGRAM_REFUSED",
    "SETUP_SECOND_INSTALL",
    "SETUP_SELF_UPDATE_UNAVAILABLE",
    "SETUP_SIGN_IN_ON_SERVER",
    "SETUP_SIGN_IN_TIMEOUT",
    "SETUP_STORE_FAILED",
    "SETUP_STORE_UNSUPPORTED",
    "SETUP_UPDATE_FEED_INVALID",
    "SETUP_UPDATE_INSTALL_FAILED",
    "SETUP_UPDATE_IN_PROGRESS",
    "SETUP_UPDATE_NONE",
    "SETUP_UPDATE_NO_ARCHIVE",
    "SETUP_UPDATE_REPLACE_FAILED",
    "SETUP_UPDATE_SIGNATURE_INVALID",
    "SETUP_UPDATE_UNREACHABLE",
    "SETUP_UPDATE_VERSION_CHANGED",
];

/// The codes the Error Table marks `Err(retryable)`. `SETUP_LANGUAGE_SERVER_INSTALL_FAILED` is
/// retryable here and built permanent explicitly for an archive breaking the unpack limits (H.3).
const RETRYABLE: &[&str] = &[
    "SETUP_CATALOGUE_FAILED",
    "SETUP_CRAFT_APP_FAILED",
    "SETUP_HARNESS_CHECK_FAILED",
    "SETUP_LANGUAGE_SERVER_INSTALL_FAILED",
    "SETUP_PROFILE_UNREACHABLE",
    "SETUP_PROGRAM_FAILED",
    "SETUP_SIGN_IN_TIMEOUT",
    "SETUP_STORE_FAILED",
    "SETUP_UPDATE_INSTALL_FAILED",
    "SETUP_UPDATE_REPLACE_FAILED",
    "SETUP_UPDATE_UNREACHABLE",
];

/// A Setup Assistant failure. `Display` is the code alone. `Clone` so that one running check or
/// guided run can hand the same result to every caller waiting on it (A.1, SET-UP-2).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SetupError {
    #[error("{code}")]
    Retryable { code: &'static str },
    #[error("{code}")]
    Permanent { code: &'static str },
}

impl SetupError {
    /// The error of `code`, retryable or permanent as the Error Table states.
    pub(crate) fn of(code: &'static str) -> SetupError {
        if RETRYABLE.contains(&code) {
            SetupError::Retryable { code }
        } else {
            SetupError::Permanent { code }
        }
    }

    pub(crate) fn permanent(code: &'static str) -> SetupError {
        SetupError::Permanent { code }
    }

    /// A code another crate owns (Identity's `CRED_…`), kept with that crate's retry flag.
    pub(crate) fn foreign(error: &dyn CodedError) -> SetupError {
        if error.retryable() {
            SetupError::Retryable { code: error.code() }
        } else {
            SetupError::Permanent { code: error.code() }
        }
    }
}

impl CodedError for SetupError {
    fn code(&self) -> &'static str {
        match self {
            SetupError::Retryable { code } | SetupError::Permanent { code } => code,
        }
    }

    fn retryable(&self) -> bool {
        matches!(self, SetupError::Retryable { .. })
    }

    /// The action of the Error Table row of the code.
    fn repair(&self) -> Option<RepairAction> {
        let code = self.code();
        match code {
            "SETUP_LANGUAGE_SERVER_INSTALL_FAILED" => {
                self.retryable().then_some(RepairAction::TryAgain)
            }
            "CRED_SIGN_IN_REQUIRED" | "SETUP_NO_READY_HARNESS" => Some(RepairAction::SignInAgain),
            _ => repair_of(code),
        }
    }
}

/// The Error Table's action column for a code.
pub(crate) fn repair_of(code: &str) -> Option<RepairAction> {
    match code {
        "SETUP_UPDATE_UNREACHABLE"
        | "SETUP_UPDATE_INSTALL_FAILED"
        | "SETUP_SIGN_IN_TIMEOUT"
        | "SETUP_HARNESS_CHECK_FAILED"
        | "SETUP_CATALOGUE_FAILED"
        | "SETUP_PROGRAM_FAILED"
        | "SETUP_CRAFT_APP_FAILED"
        | "SETUP_STORE_FAILED"
        | "SETUP_UPDATE_REPLACE_FAILED"
        | "SETUP_LANGUAGE_SERVER_INSTALL_FAILED" => Some(RepairAction::TryAgain),
        "CRED_SIGN_IN_REQUIRED" | "SETUP_NO_READY_HARNESS" => Some(RepairAction::SignInAgain),
        _ => None,
    }
}

impl From<sqlx::Error> for SetupError {
    fn from(error: sqlx::Error) -> Self {
        let class = match &error {
            sqlx::Error::Database(_) => "database",
            sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed => "pool",
            sqlx::Error::Io(_) => "io",
            _ => "other",
        };
        tracing::warn!(
            target: "ordin_setup",
            error_class = class,
            code = "SETUP_STORE_FAILED",
            "setup.store_failed"
        );
        SetupError::of("SETUP_STORE_FAILED")
    }
}
