//! The Package Catalogue's one error type and the codes the registry gives this crate
//! (PackageCatalogue.md *Error Table*; ErrorHandling.md *Internal Logic* 11).

use ordin_types::{CodedError, RepairAction};

/// Every code this crate owns: its `PKG_` codes and the 2C `AGI_` record-validation codes the
/// registry places here.
///
/// `PKG_UNSAFE`, `PKG_NEEDS_IT_APPROVAL` and `PKG_PACK_MISSING` are outcomes of `add` rather than
/// `PackageError` values, and `PKG_FOLDER_MISMATCH` is a line of `AgentFolderReport`; the registry
/// owns them under this crate, so they are listed too.
pub const ALL_CODES: &[&str] = &[
    "AGI_COMMIT_INVALID",
    "AGI_FOLDER_INVALID",
    "AGI_NAME_INVALID",
    "AGI_PACKAGE_NAME_INVALID",
    "AGI_PACKAGE_VERSION_INVALID",
    "AGI_REPOSITORY_MISMATCH",
    "AGI_STATE_INCONSISTENT",
    "PKG_AGENT_NAME_AMBIGUOUS",
    "PKG_AGENT_NOT_OFFERED",
    "PKG_AGENT_UNKNOWN",
    "PKG_ALREADY_ADDED",
    "PKG_BUNDLED_AGENT_FIXED",
    "PKG_BUNDLED_AGENT_MISSING",
    "PKG_BUNDLED_AGENT_REFUSED",
    "PKG_CHECK_UNAVAILABLE",
    "PKG_CHOICE_NOT_ALLOWED",
    "PKG_DEPRECATED_AS_AGENT",
    "PKG_FOLDER_MISMATCH",
    "PKG_FOLDER_UNUSABLE",
    "PKG_INSTALL_FAILED",
    "PKG_INSTANCE_NOT_READY",
    "PKG_INSTANCE_UNKNOWN",
    "PKG_LIBRARY_UNAVAILABLE",
    "PKG_NEEDS_IT_APPROVAL",
    "PKG_NEED_INVALID",
    "PKG_NOT_AN_AGENT",
    "PKG_NO_UPDATE",
    "PKG_PACK_MISSING",
    "PKG_STORE_FAILED",
    "PKG_TRIAL_NEEDS_TRUST",
    "PKG_TRIAL_NOT_NAMEABLE",
    "PKG_TRIAL_NO_SCOPE",
    "PKG_UNSAFE",
];

/// The one error every method returns (PackageCatalogue.md *Public Interface*).
#[derive(Debug, thiserror::Error)]
#[error("{code}")]
pub struct PackageError {
    /// A code from the Error Table, or an `AGI_` record-validation code.
    pub code: &'static str,
    pub retryable: bool,
    pub action: Option<RepairAction>,
    /// Diagnostics only; never shown to the person and never part or check text.
    pub detail: String,
}

impl PackageError {
    fn new(
        code: &'static str,
        retryable: bool,
        action: Option<RepairAction>,
        detail: impl Into<String>,
    ) -> Self {
        PackageError {
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

    pub(crate) fn agent_unknown() -> Self {
        Self::permanent("PKG_AGENT_UNKNOWN", "the library has no such agent")
    }

    pub(crate) fn not_an_agent() -> Self {
        Self::permanent("PKG_NOT_AN_AGENT", "the part is not a WorkspaceProfile")
    }

    pub(crate) fn agent_not_offered(detail: impl Into<String>) -> Self {
        Self::new("PKG_AGENT_NOT_OFFERED", false, None, detail)
    }

    pub(crate) fn deprecated_as_agent() -> Self {
        Self::permanent("PKG_DEPRECATED_AS_AGENT", "the agent is not for new agents")
    }

    pub(crate) fn already_added() -> Self {
        Self::permanent(
            "PKG_ALREADY_ADDED",
            "an instance of this agent serves the workspace",
        )
    }

    pub(crate) fn check_unavailable(detail: impl Into<String>) -> Self {
        Self::new(
            "PKG_CHECK_UNAVAILABLE",
            true,
            Some(RepairAction::TryAgain),
            detail,
        )
    }

    pub(crate) fn install_failed(detail: impl Into<String>) -> Self {
        Self::new(
            "PKG_INSTALL_FAILED",
            true,
            Some(RepairAction::TryAgain),
            detail,
        )
    }

    pub(crate) fn choice_not_allowed(detail: impl Into<String>) -> Self {
        Self::permanent("PKG_CHOICE_NOT_ALLOWED", detail)
    }

    pub(crate) fn no_update() -> Self {
        Self::permanent("PKG_NO_UPDATE", "no update is waiting for the instance")
    }

    pub(crate) fn instance_unknown() -> Self {
        Self::permanent("PKG_INSTANCE_UNKNOWN", "no such instance on this computer")
    }

    pub(crate) fn instance_not_ready(detail: impl Into<String>) -> Self {
        Self::permanent("PKG_INSTANCE_NOT_READY", detail)
    }

    pub(crate) fn folder_unusable(detail: impl Into<String>) -> Self {
        Self::new("PKG_FOLDER_UNUSABLE", false, None, detail)
    }

    pub(crate) fn trial_needs_trust() -> Self {
        Self::permanent(
            "PKG_TRIAL_NEEDS_TRUST",
            "the draft needs parts outside the trust policy",
        )
    }

    pub(crate) fn name_ambiguous() -> Self {
        Self::permanent(
            "PKG_AGENT_NAME_AMBIGUOUS",
            "two instances of the workspace share the name",
        )
    }

    pub(crate) fn trial_not_nameable() -> Self {
        Self::permanent(
            "PKG_TRIAL_NOT_NAMEABLE",
            "a trial of an own draft has no repository",
        )
    }

    pub(crate) fn trial_no_scope() -> Self {
        Self::permanent(
            "PKG_TRIAL_NO_SCOPE",
            "a trial of an own draft has no merged repository",
        )
    }

    pub(crate) fn need_invalid() -> Self {
        Self::permanent(
            "PKG_NEED_INVALID",
            "the need is empty or over 500 characters",
        )
    }

    /// `remove` or `change_choice` names a bundled instance (PKG-REC-3).
    pub(crate) fn bundled_agent_fixed() -> Self {
        Self::permanent(
            "PKG_BUNDLED_AGENT_FIXED",
            "This agent comes with ordin and can't be changed or removed.",
        )
    }

    /// The bundled agent's identifier is not supplied, or the bundled catalogue holds no current,
    /// offered WorkspaceProfile with it (PKG-REC-2 step 3).
    pub(crate) fn bundled_agent_missing(detail: impl Into<String>) -> Self {
        Self::permanent("PKG_BUNDLED_AGENT_MISSING", detail)
    }

    /// The bundled agent fails its checks, needs IT approval, cannot settle a required pack, or was
    /// stopped (PKG-REC-2 steps 2 and 5).
    pub(crate) fn bundled_agent_refused(detail: impl Into<String>) -> Self {
        Self::new("PKG_BUNDLED_AGENT_REFUSED", false, None, detail)
    }

    /// A catalogue read failed; the flag is the Catalogue Service's own.
    pub(crate) fn library_unavailable(retryable: bool, detail: impl Into<String>) -> Self {
        Self::new("PKG_LIBRARY_UNAVAILABLE", retryable, None, detail)
    }

    /// A datastore failure (retryable) or an unreadable stored value (permanent).
    pub(crate) fn store(retryable: bool, detail: impl Into<String>) -> Self {
        Self::new(
            "PKG_STORE_FAILED",
            retryable,
            retryable.then_some(RepairAction::TryAgain),
            detail,
        )
    }
}

impl From<sqlx::Error> for PackageError {
    fn from(error: sqlx::Error) -> Self {
        PackageError::store(true, error.to_string())
    }
}

impl CodedError for PackageError {
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
