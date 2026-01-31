//! `KqError`, the one error type of this crate (KnowledgeQueryClient.md *Public Interface*,
//! *Error Table*).

use ordin_types::{CodedError, RepairAction};

/// Every code this crate raises (ErrorHandling.md *Internal Logic* 11): the rows of the Error Table.
pub const ALL_CODES: &[&str] = &[
    "KQ_ALREADY_CONNECTED",
    "KQ_BAD_QUERY",
    "KQ_CONFIG_INVALID",
    "KQ_CONNECTION_NOT_FOUND",
    "KQ_NOT_A_KNOWLEDGE_AGENT",
    "KQ_NOT_IN_AUDIENCE",
    "KQ_NOT_SUPPORTED",
    "KQ_NO_CONNECTIONS",
    "KQ_OTHER_COMPANY",
    "KQ_SIGN_IN_NEEDED",
    "KQ_STORE_UNAVAILABLE",
    "KQ_TOO_MANY_CONNECTIONS",
    "KQ_UNKNOWN_PROXY",
    "KQ_UNREACHABLE",
    "KQ_URL_INVALID",
];

/// Every failure of this crate. `Display` writes the code only, never a URL, token or answer text.
/// Per-agent failures inside `ask` are never errors of `ask`: they become `unanswered` entries.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KqError {
    #[error("KQ_URL_INVALID")]
    UrlInvalid,
    #[error("KQ_TOO_MANY_CONNECTIONS")]
    TooManyConnections,
    #[error("KQ_ALREADY_CONNECTED")]
    AlreadyConnected,
    #[error("KQ_NOT_A_KNOWLEDGE_AGENT")]
    NotAKnowledgeAgent,
    #[error("KQ_OTHER_COMPANY")]
    OtherCompany,
    #[error("KQ_SIGN_IN_NEEDED")]
    SignInNeeded,
    #[error("KQ_NOT_IN_AUDIENCE")]
    NotInAudience,
    #[error("KQ_UNREACHABLE")]
    Unreachable,
    #[error("KQ_CONNECTION_NOT_FOUND")]
    ConnectionNotFound,
    #[error("KQ_NOT_SUPPORTED")]
    NotSupported,
    #[error("KQ_CONFIG_INVALID")]
    ConfigInvalid,
    #[error("KQ_BAD_QUERY")]
    BadQuery,
    #[error("KQ_NO_CONNECTIONS")]
    NoConnections,
    #[error("KQ_UNKNOWN_PROXY")]
    UnknownProxy,
    #[error("KQ_STORE_UNAVAILABLE")]
    StoreUnavailable,
}

impl KqError {
    /// The registry code of this error.
    pub fn code(&self) -> &'static str {
        match self {
            KqError::UrlInvalid => "KQ_URL_INVALID",
            KqError::TooManyConnections => "KQ_TOO_MANY_CONNECTIONS",
            KqError::AlreadyConnected => "KQ_ALREADY_CONNECTED",
            KqError::NotAKnowledgeAgent => "KQ_NOT_A_KNOWLEDGE_AGENT",
            KqError::OtherCompany => "KQ_OTHER_COMPANY",
            KqError::SignInNeeded => "KQ_SIGN_IN_NEEDED",
            KqError::NotInAudience => "KQ_NOT_IN_AUDIENCE",
            KqError::Unreachable => "KQ_UNREACHABLE",
            KqError::ConnectionNotFound => "KQ_CONNECTION_NOT_FOUND",
            KqError::NotSupported => "KQ_NOT_SUPPORTED",
            KqError::ConfigInvalid => "KQ_CONFIG_INVALID",
            KqError::BadQuery => "KQ_BAD_QUERY",
            KqError::NoConnections => "KQ_NO_CONNECTIONS",
            KqError::UnknownProxy => "KQ_UNKNOWN_PROXY",
            KqError::StoreUnavailable => "KQ_STORE_UNAVAILABLE",
        }
    }

    /// `true` for `Unreachable` and `StoreUnavailable`; every other code is permanent
    /// (Error Table, last paragraph).
    pub fn retryable(&self) -> bool {
        matches!(self, KqError::Unreachable | KqError::StoreUnavailable)
    }
}

impl CodedError for KqError {
    fn code(&self) -> &'static str {
        KqError::code(self)
    }

    fn retryable(&self) -> bool {
        KqError::retryable(self)
    }

    /// The Error Table's response sentences name what the person can do: sign in again, or try again.
    fn repair(&self) -> Option<RepairAction> {
        match self {
            KqError::SignInNeeded => Some(RepairAction::SignInAgain),
            KqError::Unreachable | KqError::StoreUnavailable => Some(RepairAction::TryAgain),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for KqError {
    fn from(_: sqlx::Error) -> Self {
        KqError::StoreUnavailable
    }
}
