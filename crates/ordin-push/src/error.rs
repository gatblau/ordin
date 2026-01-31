//! The Push Service's one error type and the codes it owns (PushService.md *Error Table*;
//! ErrorHandling.md *Internal Logic* 11).

use ordin_types::{CodedError, RepairAction};

/// Every code the registry gives this crate (its `PUSH_` rows).
///
/// Two are owned here but never returned by this crate: `PUSH_ACTION_KEY_PREMATURE` and
/// `PUSH_DELETE_UNCHECKED` are 2C rules the Error Table has no row for (the key is written only after
/// a push by construction, and the person's check before a deletion is the screen's).
pub const ALL_CODES: &[&str] = &[
    "PUSH_ACTION_KEY_PREMATURE",
    "PUSH_ALL_ORIGINS_CHANGED",
    "PUSH_APPROVAL_FAILED",
    "PUSH_APPROVAL_MISSING",
    "PUSH_APPROVED_VERSION_CHANGED",
    "PUSH_AUDIENCE_GREW",
    "PUSH_BRANCH_MOVED",
    "PUSH_CONFLUENCE_UNAVAILABLE",
    "PUSH_COPY_CANNOT_BRING_BACK",
    "PUSH_COPY_LINKED",
    "PUSH_COPY_MAKER_FAILED",
    "PUSH_COPY_STATE",
    "PUSH_DELETE_UNCHECKED",
    "PUSH_DESTINATION_REFUSED",
    "PUSH_FORBIDDEN",
    "PUSH_FORMAT_NOT_READY",
    "PUSH_GIT_BRANCH_UNKNOWN",
    "PUSH_GIT_IDENTITY_MISSING",
    "PUSH_ITEMS_OUT_OF_RANGE",
    "PUSH_ITEM_ALREADY_PENDING",
    "PUSH_ITEM_NOT_CHANGED",
    "PUSH_NAME_EXISTS",
    "PUSH_NOT_HELD",
    "PUSH_NO_REVIEW_RIGHT",
    "PUSH_ORIGINAL_UNREADABLE",
    "PUSH_ORIGIN_CHANGED",
    "PUSH_ORIGIN_NOT_FOUND",
    "PUSH_ORIGIN_UNREACHABLE",
    "PUSH_OUTCOME_UNKNOWN",
    "PUSH_REQUEST_INVALID",
    "PUSH_SIGN_IN_NEEDED",
    "PUSH_STORE_FAILED",
    "PUSH_SUMMARY_INVALID",
    "PUSH_TEAM_COMMIT_MISSING",
    "PUSH_TEAM_SHARE_STATE",
    "PUSH_TEAM_SHARE_UNKNOWN",
    "PUSH_TOO_LARGE",
    "PUSH_UNKNOWN",
];

/// Every failure of this crate (2C-style: the code, whether the same call may succeed later, the one
/// repair action the person is offered, and a detail for the log only).
///
/// `detail` never carries file content, request text, a token or a destination beyond its host.
#[derive(Debug, thiserror::Error)]
#[error("{code}")]
pub struct PushError {
    pub code: &'static str,
    pub retryable: bool,
    pub action: Option<RepairAction>,
    pub detail: String,
}

/// The Error Table's *Status* and *Response Body* columns, by code: (retryable, action).
fn table(code: &'static str) -> (bool, Option<RepairAction>) {
    match code {
        "PUSH_APPROVAL_FAILED" | "PUSH_STORE_FAILED" | "PUSH_ORIGIN_UNREACHABLE" => {
            (true, Some(RepairAction::TryAgain))
        }
        "PUSH_SIGN_IN_NEEDED" => (true, Some(RepairAction::SignInAgain)),
        "PUSH_ORIGINAL_UNREADABLE" | "PUSH_COPY_MAKER_FAILED" => {
            (false, Some(RepairAction::TryAgain))
        }
        "PUSH_DESTINATION_REFUSED" | "PUSH_GIT_IDENTITY_MISSING" => (false, None),
        _ => (false, None),
    }
}

impl PushError {
    /// The error the Error Table names for `code`.
    pub fn new(code: &'static str, detail: impl Into<String>) -> PushError {
        let (retryable, action) = table(code);
        PushError {
            code,
            retryable,
            action,
            detail: detail.into(),
        }
    }

    /// `PUSH_STORE_FAILED`, retryable only when the cause is.
    pub(crate) fn store(retryable: bool, detail: impl Into<String>) -> PushError {
        let mut error = PushError::new("PUSH_STORE_FAILED", detail);
        error.retryable = retryable;
        error
    }
}

impl CodedError for PushError {
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

/// Any datastore failure is `PUSH_STORE_FAILED` (retryable).
impl From<sqlx::Error> for PushError {
    fn from(error: sqlx::Error) -> Self {
        PushError::store(true, format!("datastore: {error}"))
    }
}

/// Maps the code a provider, adapter or git step answered with to the item outcome of Internal
/// Logic C.6 (and, for team shares and pull requests, I.6 and F.3).
///
/// The second value is `true` when the item stays `awaiting_push` without moving to
/// `origin_changed` (every failure except `PUSH_ORIGIN_CHANGED`).
pub(crate) fn classify_send(code: &str) -> &'static str {
    match code {
        "SP_ORIGIN_CHANGED" | "CONF_ORIGIN_CHANGED" | "PUSH_ORIGIN_CHANGED" => {
            "PUSH_ORIGIN_CHANGED"
        }
        "SP_SIGN_IN_NEEDED"
        | "CONF_SIGN_IN_NEEDED"
        | "CONF_SITE_NOT_GRANTED"
        | "GH_SIGN_IN_NEEDED"
        | "PROJ_SIGN_IN_NEEDED"
        | "PUSH_SIGN_IN_NEEDED" => "PUSH_SIGN_IN_NEEDED",
        "SP_ACCESS_DENIED" | "CONF_ACCESS_DENIED" | "PUSH_FORBIDDEN" => "PUSH_FORBIDDEN",
        "SP_NOT_FOUND" | "CONF_NOT_FOUND" | "PUSH_ORIGIN_NOT_FOUND" => "PUSH_ORIGIN_NOT_FOUND",
        "SP_TOO_LARGE" | "CONF_TOO_LARGE" | "PUSH_TOO_LARGE" => "PUSH_TOO_LARGE",
        "SP_THROTTLED"
        | "SP_UNAVAILABLE"
        | "CONF_THROTTLED"
        | "CONF_UNAVAILABLE"
        | "GH_UNREACHABLE"
        | "GH_RATE_LIMITED"
        | "PROJ_REMOTE_UNREACHABLE"
        | "PROJ_RATE_LIMITED"
        | "PUSH_ORIGIN_UNREACHABLE" => "PUSH_ORIGIN_UNREACHABLE",
        "SP_OUTCOME_UNKNOWN"
        | "CONF_OUTCOME_UNKNOWN"
        | "SP_ACTION_KEY_USED"
        | "CONF_ACTION_KEY_USED"
        | "EXT_KEY_DUPLICATE"
        | "PUSH_OUTCOME_UNKNOWN" => "PUSH_OUTCOME_UNKNOWN",
        "PUSH_CONFLUENCE_UNAVAILABLE" => "PUSH_CONFLUENCE_UNAVAILABLE",
        "PUSH_GIT_IDENTITY_MISSING" => "PUSH_GIT_IDENTITY_MISSING",
        "PUSH_GIT_BRANCH_UNKNOWN" => "PUSH_GIT_BRANCH_UNKNOWN",
        _ => "PUSH_DESTINATION_REFUSED",
    }
}
