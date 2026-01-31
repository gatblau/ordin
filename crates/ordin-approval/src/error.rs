//! The Approval Service's one error type and the codes it owns (ApprovalService.md *Error Table*;
//! ErrorHandling.md *Internal Logic* 11).

use ordin_journal::JournalError;
use ordin_types::domain::runs::ApprovalState;
use ordin_types::{CodedError, RepairAction};

/// Every code the registry gives this crate (its `APPR_` rows).
///
/// Six of them are owned here but never returned by this crate: `APPR_ALREADY_DECIDED`,
/// `APPR_ATTACHMENT_NOT_ALLOWED`, `APPR_CARD_REF_INVALID`, `APPR_DECISION_MISMATCH` and
/// `APPR_RECIPIENT_NOT_ALLOWED` are registered but absent from the Error Table (recipient and
/// attachment checks are the Mail Service's and the Rules Engine's, Internal Logic B.2), and
/// `APPR_EMIT_FAILED` is only ever logged. `SRV_BAD_CURSOR` is raised but owned by `ordin-types`.
pub const ALL_CODES: &[&str] = &[
    "APPR_ACTION_INVALID",
    "APPR_AFFECTS_INVALID",
    "APPR_ALREADY_DECIDED",
    "APPR_ASKER_MAY_NOT_APPROVE",
    "APPR_ATTACHMENT_NOT_ALLOWED",
    "APPR_CARD_ALREADY_DECIDED",
    "APPR_CARD_CLOSED",
    "APPR_CARD_REF_INVALID",
    "APPR_CARD_REF_MISMATCH",
    "APPR_CARD_REF_UNKNOWN",
    "APPR_CHANGES_INVALID",
    "APPR_CONSEQUENCE_INVALID",
    "APPR_CONTENT_LABEL_INVALID",
    "APPR_CONTENT_TOO_LARGE",
    "APPR_DECISION_MISMATCH",
    "APPR_DESTINATION_INVALID",
    "APPR_EMIT_FAILED",
    "APPR_NOT_AN_APPROVER",
    "APPR_NOT_FOUND",
    "APPR_OPERATOR_NOT_ALLOWED",
    "APPR_QUESTION_INVALID",
    "APPR_RECIPIENT_NOT_ALLOWED",
    "APPR_RECORDS_TOO_MANY",
    "APPR_SHOW_DETAILS_WITHOUT_CONTENT",
    "APPR_STORE_UNAVAILABLE",
    "APPR_VERB_INVALID",
    "APPR_WHERE_INVALID",
];

/// Every failure of this crate. `Display` never carries a value from the caller or the datastore.
#[derive(Debug, thiserror::Error)]
pub enum ApprovalError {
    /// A card or proposal field broke its limit (B.1); `code` is the 2C code, `field` the field name.
    #[error("approval request invalid: {code}")]
    Invalid {
        code: &'static str,
        field: &'static str,
    },
    #[error("card reference unknown")]
    CardRefUnknown,
    #[error("card reference does not match the decision")]
    CardRefMismatch,
    /// Carries where the decision was made so the caller can say so (2C example).
    #[error("already decided")]
    AlreadyDecided {
        state: ApprovalState,
        decided_on: String,
    },
    #[error("card closed")]
    CardClosed,
    #[error("not an approver")]
    NotAnApprover,
    #[error("asker may not approve")]
    AskerMayNotApprove,
    #[error("operator decision not allowed")]
    OperatorNotAllowed,
    #[error("approval not found")]
    NotFound,
    /// `SRV_BAD_CURSOR` (H.2).
    #[error("cursor not valid")]
    BadCursor,
    #[error("datastore unavailable")]
    Store(#[source] sqlx::Error),
    #[error("journal unavailable")]
    Journal(#[source] JournalError),
}

impl ApprovalError {
    /// The registry code.
    pub fn code(&self) -> &'static str {
        match self {
            ApprovalError::Invalid { code, .. } => code,
            ApprovalError::CardRefUnknown => "APPR_CARD_REF_UNKNOWN",
            ApprovalError::CardRefMismatch => "APPR_CARD_REF_MISMATCH",
            ApprovalError::AlreadyDecided { .. } => "APPR_CARD_ALREADY_DECIDED",
            ApprovalError::CardClosed => "APPR_CARD_CLOSED",
            ApprovalError::NotAnApprover => "APPR_NOT_AN_APPROVER",
            ApprovalError::AskerMayNotApprove => "APPR_ASKER_MAY_NOT_APPROVE",
            ApprovalError::OperatorNotAllowed => "APPR_OPERATOR_NOT_ALLOWED",
            ApprovalError::NotFound => "APPR_NOT_FOUND",
            ApprovalError::BadCursor => "SRV_BAD_CURSOR",
            ApprovalError::Store(_) => "APPR_STORE_UNAVAILABLE",
            // An emit failure is only logged; the variant reports the Journal's own code and retry
            // flag.
            ApprovalError::Journal(inner) => CodedError::code(inner),
        }
    }

    /// `true` for a failed store and for a Journal error that is itself retryable; every other
    /// error is permanent.
    pub fn retryable(&self) -> bool {
        match self {
            ApprovalError::Store(_) => true,
            ApprovalError::Journal(inner) => CodedError::retryable(inner),
            _ => false,
        }
    }
}

impl CodedError for ApprovalError {
    fn code(&self) -> &'static str {
        ApprovalError::code(self)
    }

    fn retryable(&self) -> bool {
        ApprovalError::retryable(self)
    }

    fn repair(&self) -> Option<RepairAction> {
        match self {
            // "Something went wrong saving this decision. Try again." (Error Table).
            ApprovalError::Store(_) => Some(RepairAction::TryAgain),
            ApprovalError::Journal(inner) => CodedError::repair(inner),
            _ => None,
        }
    }
}

/// Any datastore failure is `APPR_STORE_UNAVAILABLE` (retryable).
impl From<sqlx::Error> for ApprovalError {
    fn from(error: sqlx::Error) -> Self {
        ApprovalError::Store(error)
    }
}

impl From<JournalError> for ApprovalError {
    fn from(error: JournalError) -> Self {
        ApprovalError::Journal(error)
    }
}

impl ApprovalError {
    /// A stored row that no longer parses: the store cannot be trusted, which is the same
    /// retryable condition as an unreachable store.
    pub(crate) fn corrupt(what: &'static str) -> ApprovalError {
        ApprovalError::Store(sqlx::Error::Decode(what.into()))
    }
}
