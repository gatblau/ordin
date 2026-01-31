//! The Catalogue Service's one error type and the codes the registry gives this crate
//! (CatalogueService.md *Error Table*; ErrorHandling.md *Internal Logic* 11).

use ordin_types::{CodedError, RepairAction};

/// Every code this crate owns: the `CAT_` codes, the 2C `AREPO_`, `PART_`, `WDL_`, `VOCAB_` and
/// `LIFE_` codes the registry places here, and `SRV_BAD_CURSOR` (raised here, owned by `ordin-types`).
///
/// Codes that are raised as ingest outcomes, health lines or Diagnostics entries rather than as
/// `CatalogueError` values (`CAT_MERGE_UNSIGNED`, `CAT_WITHDRAWN_LIST_UNREADABLE`, `CAT_FORMAT_CHANGED`,
/// `CAT_REPOSITORY_TOO_LARGE`, `PART_DUPLICATE`, `CAT_CHECK_UNAVAILABLE`, `CAT_CHECK_FAILED`,
/// `CAT_PROFILE_KEY_RESERVED`, the `LIFE_`, `WDL_` and `VOCAB_` rules) are listed too: the registry owns them
/// under this crate.
pub const ALL_CODES: &[&str] = &[
    "AREPO_ADD_DISABLED",
    "AREPO_BRANCH_INVALID",
    "AREPO_COMMIT_INVALID",
    "AREPO_DUPLICATE",
    "AREPO_HOST_UNSUPPORTED",
    "AREPO_NAME_INVALID",
    "AREPO_REMOVE_NOT_OWN",
    "AREPO_SIGNED_MERGES_MISMATCH",
    "CAT_BRANCH_EXISTS",
    "CAT_CHECK_FAILED",
    "CAT_CHECK_UNAVAILABLE",
    "CAT_FETCH_FAILED",
    "CAT_FORMAT_CHANGED",
    "CAT_FORMAT_UNKNOWN",
    "CAT_LIFECYCLE_UNKNOWN",
    "CAT_MERGE_UNSIGNED",
    "CAT_NAME_INVALID",
    "CAT_NO_READ_ACCESS",
    "CAT_NO_REVIEW_RIGHT",
    "CAT_PART_UNKNOWN",
    "CAT_PROBE_EXPIRED",
    "CAT_PROFILE_KEY_RESERVED",
    "CAT_QUERY_INVALID",
    "CAT_REPOSITORY_LIMIT",
    "CAT_REPOSITORY_TOO_LARGE",
    "CAT_REPOSITORY_UNKNOWN",
    "CAT_REQUIREMENTS_INVALID",
    "CAT_SIGN_IN_NEEDED",
    "CAT_STORE_FAILED",
    "CAT_UNREACHABLE",
    "CAT_WITHDRAWN_LIST_UNREADABLE",
    "LIFE_CHOICES_INVALID",
    "LIFE_DAYS_OUT_OF_RANGE",
    "LIFE_PLACE_INVALID",
    "LIFE_ROLE_INVALID",
    "LIFE_SEND_BACK_NOT_EARLIER",
    "LIFE_SEND_BACK_ROUNDS_OUT_OF_RANGE",
    "LIFE_SLACK_CHANNEL_INVALID",
    "LIFE_STAGES_INVALID",
    "LIFE_STAGE_BACKWARD",
    "LIFE_STAGE_UNREACHABLE",
    "PART_COMMIT_INVALID",
    "PART_DUPLICATE",
    "PART_KIND_UNKNOWN",
    "PART_SUPERSEDES_INVALID",
    "PART_VERSION_INVALID",
    "PART_WITHDRAWAL_FINAL",
    "SRV_BAD_CURSOR",
    "VOCAB_CODE_INVALID",
    "VOCAB_DESCRIPTION_TOO_LONG",
    "VOCAB_LABEL_INVALID",
    "WDL_APPEND_ONLY",
    "WDL_REASON_INVALID",
    "WDL_REPLACEMENT_INCOMPLETE",
    "WDL_VERSION_DIGEST_MISMATCH",
];

/// The one error every method of the service returns (CatalogueService.md *Public Interface*).
#[derive(Debug, thiserror::Error)]
#[error("{code}")]
pub struct CatalogueError {
    /// A code from the Error Table.
    pub code: &'static str,
    pub retryable: bool,
    pub action: Option<RepairAction>,
    /// Diagnostics only; never shown to the person.
    pub detail: String,
}

impl CatalogueError {
    fn new(
        code: &'static str,
        retryable: bool,
        action: Option<RepairAction>,
        detail: impl Into<String>,
    ) -> Self {
        CatalogueError {
            code,
            retryable,
            action,
            detail: detail.into(),
        }
    }

    /// A permanent error with no repair action.
    pub(crate) fn permanent(code: &'static str, detail: impl Into<String>) -> Self {
        Self::new(code, false, None, detail)
    }

    pub(crate) fn name_invalid(detail: impl Into<String>) -> Self {
        Self::permanent("AREPO_NAME_INVALID", detail)
    }

    pub(crate) fn add_disabled() -> Self {
        Self::new(
            "AREPO_ADD_DISABLED",
            false,
            None,
            "agent_catalogue.person_may_add is false",
        )
    }

    pub(crate) fn sign_in_needed() -> Self {
        Self::new(
            "CAT_SIGN_IN_NEEDED",
            false,
            Some(RepairAction::SignInAgain),
            "the GitHub sign-in is missing or expired",
        )
    }

    pub(crate) fn unreachable(detail: impl Into<String>) -> Self {
        Self::new(
            "CAT_UNREACHABLE",
            true,
            Some(RepairAction::TryAgain),
            detail,
        )
    }

    pub(crate) fn fetch_failed(detail: impl Into<String>) -> Self {
        Self::new(
            "CAT_FETCH_FAILED",
            true,
            Some(RepairAction::TryAgain),
            detail,
        )
    }

    pub(crate) fn store(detail: impl Into<String>) -> Self {
        Self::new(
            "CAT_STORE_FAILED",
            true,
            Some(RepairAction::TryAgain),
            detail,
        )
    }

    pub(crate) fn part_unknown() -> Self {
        Self::permanent("CAT_PART_UNKNOWN", "no head part has this identifier")
    }

    pub(crate) fn repository_unknown() -> Self {
        Self::permanent(
            "CAT_REPOSITORY_UNKNOWN",
            "no agent repository has this identifier",
        )
    }

    pub(crate) fn bad_cursor() -> Self {
        Self::permanent(
            "SRV_BAD_CURSOR",
            "the cursor is not one this listing returned",
        )
    }
}

impl From<sqlx::Error> for CatalogueError {
    fn from(error: sqlx::Error) -> Self {
        CatalogueError::store(error.to_string())
    }
}

impl CodedError for CatalogueError {
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
