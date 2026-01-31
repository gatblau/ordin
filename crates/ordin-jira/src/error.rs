//! `JiraError`, the one error type of this crate (JiraAdapter.md *Public Interface*, *Error Table*).

use ordin_types::{CodedError, RepairAction};

/// Every code this crate owns (ErrorHandling.md *Internal Logic* 11): the rows of the Error Table.
pub const ALL_CODES: &[&str] = &[
    "JIRA_ACCESS_DENIED",
    "JIRA_ACTION_KEY_USED",
    "JIRA_ADDRESS_INVALID",
    "JIRA_ISSUE_TYPE_NONE",
    "JIRA_NOT_CONNECTED",
    "JIRA_NOT_FOUND",
    "JIRA_OUTCOME_UNKNOWN",
    "JIRA_QUERY_INVALID",
    "JIRA_RESPONSE_INVALID",
    "JIRA_SIGN_IN_NEEDED",
    "JIRA_SITE_MISMATCH",
    "JIRA_THROTTLED",
    "JIRA_TOKEN_REFUSED",
    "JIRA_TOO_LARGE",
    "JIRA_UNAVAILABLE",
];

/// Every failure of this crate. `Display` writes the code only, never a Jira message, token or text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JiraError {
    #[error("JIRA_ADDRESS_INVALID")]
    AddressInvalid,
    #[error("JIRA_NOT_CONNECTED")]
    NotConnected,
    #[error("JIRA_SIGN_IN_NEEDED")]
    SignInNeeded,
    #[error("JIRA_TOKEN_REFUSED")]
    TokenRefused,
    #[error("JIRA_SITE_MISMATCH")]
    SiteMismatch,
    #[error("JIRA_ACCESS_DENIED")]
    AccessDenied,
    #[error("JIRA_NOT_FOUND")]
    NotFound,
    #[error("JIRA_QUERY_INVALID")]
    QueryInvalid,
    #[error("JIRA_ISSUE_TYPE_NONE")]
    IssueTypeNone,
    #[error("JIRA_TOO_LARGE")]
    TooLarge,
    #[error("JIRA_THROTTLED")]
    Throttled,
    #[error("JIRA_UNAVAILABLE")]
    Unavailable,
    #[error("JIRA_RESPONSE_INVALID")]
    ResponseInvalid,
    #[error("JIRA_OUTCOME_UNKNOWN")]
    OutcomeUnknown,
    #[error("JIRA_ACTION_KEY_USED")]
    ActionKeyUsed,
}

impl JiraError {
    /// The registry code of this error.
    pub fn code(&self) -> &'static str {
        match self {
            JiraError::AddressInvalid => "JIRA_ADDRESS_INVALID",
            JiraError::NotConnected => "JIRA_NOT_CONNECTED",
            JiraError::SignInNeeded => "JIRA_SIGN_IN_NEEDED",
            JiraError::TokenRefused => "JIRA_TOKEN_REFUSED",
            JiraError::SiteMismatch => "JIRA_SITE_MISMATCH",
            JiraError::AccessDenied => "JIRA_ACCESS_DENIED",
            JiraError::NotFound => "JIRA_NOT_FOUND",
            JiraError::QueryInvalid => "JIRA_QUERY_INVALID",
            JiraError::IssueTypeNone => "JIRA_ISSUE_TYPE_NONE",
            JiraError::TooLarge => "JIRA_TOO_LARGE",
            JiraError::Throttled => "JIRA_THROTTLED",
            JiraError::Unavailable => "JIRA_UNAVAILABLE",
            JiraError::ResponseInvalid => "JIRA_RESPONSE_INVALID",
            JiraError::OutcomeUnknown => "JIRA_OUTCOME_UNKNOWN",
            JiraError::ActionKeyUsed => "JIRA_ACTION_KEY_USED",
        }
    }

    /// `true` for `Throttled` and `Unavailable`; every other variant is permanent.
    pub fn retryable(&self) -> bool {
        matches!(self, JiraError::Throttled | JiraError::Unavailable)
    }
}

impl CodedError for JiraError {
    fn code(&self) -> &'static str {
        JiraError::code(self)
    }

    fn retryable(&self) -> bool {
        JiraError::retryable(self)
    }

    /// The *Response Body* `action` column of the Error Table.
    fn repair(&self) -> Option<RepairAction> {
        match self {
            JiraError::NotConnected | JiraError::SignInNeeded => Some(RepairAction::SignInAgain),
            JiraError::Throttled | JiraError::Unavailable => Some(RepairAction::TryAgain),
            JiraError::ResponseInvalid => None,
            _ => None,
        }
    }
}
