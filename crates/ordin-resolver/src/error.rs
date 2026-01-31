//! The Resolver and Compiler's errors (ResolverAndCompiler.md *Public Interface*, *Error Table*).
//!
//! Every error is a returned value of an in-process API; none reaches the person directly, and the
//! caller maps it to its own surface. `Display` is the bare code, so a caller logging the error never
//! writes part content.

use ordin_agent_model::{PartKind, PartSourceError, PartStatus};
use ordin_types::CodedError;

use crate::conflict::Conflict;

/// Every code this crate owns (ErrorHandling.md *Internal Logic* 11), in registry order.
pub const ALL_CODES: &[&str] = &[
    "RES_BUNDLE_CORRUPT",
    "RES_BUNDLE_NOT_FOUND",
    "RES_CLOSURE_TOO_LARGE",
    "RES_CONFLICT",
    "RES_DEPENDENCY_CYCLE",
    "RES_DIGEST_MISMATCH",
    "RES_DRAFT_CONSTITUENT",
    "RES_LOCK_MISMATCH",
    "RES_LOCK_PART_UNAVAILABLE",
    "RES_LOOP_MISSING",
    "RES_LOOP_MODE_ESCAPE",
    "RES_LOOP_MULTIPLE",
    "RES_LOOP_NOT_PERMITTED",
    "RES_MANIFEST_INVALID",
    "RES_MEMORY_CARDINALITY",
    "RES_NOT_INSTALLABLE",
    "RES_OVERLAY_CARDINALITY",
    "RES_OVERLAY_NOT_ALLOWED",
    "RES_OVERLAY_NOT_APPLICABLE",
    "RES_PACK_NOT_ALLOWED",
    "RES_PART_WITHDRAWN",
    "RES_REFERENCE_UNRESOLVED",
    "RES_REQUIRED_OVERLAY_MISSING",
    "RES_REQUIRED_PACK_UNSETTLED",
    "RES_ROOT_NOT_APPROVED",
    "RES_ROOT_NOT_FOUND",
    "RES_SOURCE_ALREADY_BOUND",
    "RES_SOURCE_AMBIGUOUS",
    "RES_SOURCE_NOT_BOUND",
    "RES_SOURCE_UNAVAILABLE",
    "RES_STORE_WRITE_FAILED",
    "RES_TOOL_POLICY_INVALID",
];

#[derive(Debug, Clone, thiserror::Error)]
pub enum ResolveError {
    #[error("RES_ROOT_NOT_FOUND")]
    RootNotFound { id: String },
    #[error("RES_NOT_INSTALLABLE")]
    NotInstallable { kind: PartKind },
    #[error("RES_ROOT_NOT_APPROVED")]
    RootNotApproved { id: String, status: PartStatus },
    #[error("RES_DRAFT_CONSTITUENT")]
    DraftConstituent { part: String },
    #[error("RES_REFERENCE_UNRESOLVED")]
    ReferenceUnresolved {
        from: String,
        id: String,
        relation: String,
    },
    #[error("RES_SOURCE_AMBIGUOUS")]
    SourceAmbiguous {
        id: String,
        repositories: Vec<String>,
    },
    #[error("RES_DEPENDENCY_CYCLE")]
    DependencyCycle { path: Vec<String> },
    #[error("RES_CLOSURE_TOO_LARGE")]
    ClosureTooLarge { parts: u32 },
    #[error("RES_CONFLICT")]
    Conflict { conflicts: Vec<Conflict> },
    #[error("RES_PART_WITHDRAWN")]
    PartWithdrawn {
        part: String,
        replacement: Option<String>,
    },
    #[error("RES_REQUIRED_PACK_UNSETTLED")]
    RequiredPackUnsettled {
        rule_index: u32,
        class: Option<String>,
    },
    #[error("RES_PACK_NOT_ALLOWED")]
    PackNotAllowed { id: String },
    #[error("RES_REQUIRED_OVERLAY_MISSING")]
    RequiredOverlayMissing { axis: PartKind },
    #[error("RES_OVERLAY_NOT_ALLOWED")]
    OverlayNotAllowed { id: String },
    #[error("RES_OVERLAY_CARDINALITY")]
    OverlayCardinality { axis: PartKind },
    #[error("RES_OVERLAY_NOT_APPLICABLE")]
    OverlayNotApplicable { overlay: String },
    #[error("RES_MEMORY_CARDINALITY")]
    MemoryCardinality { parts: Vec<String> },
    #[error("RES_LOOP_MISSING")]
    LoopMissing,
    #[error("RES_LOOP_MULTIPLE")]
    LoopMultiple { parts: Vec<String> },
    #[error("RES_LOOP_NOT_PERMITTED")]
    LoopNotPermitted { loop_id: String },
    #[error("RES_LOOP_MODE_ESCAPE")]
    LoopModeEscape { loop_id: String, mode: String },
    #[error("RES_TOOL_POLICY_INVALID")]
    ToolPolicyInvalid {
        binding: String,
        reason: &'static str,
    },
    #[error("RES_MANIFEST_INVALID")]
    ManifestInvalid { part: String, code: String },
    #[error("RES_DIGEST_MISMATCH")]
    DigestMismatch { part: String },
    #[error("RES_LOCK_PART_UNAVAILABLE")]
    LockPartUnavailable { part: String },
    #[error("RES_LOCK_MISMATCH")]
    LockMismatch { expected: String, found: String },
    #[error("RES_BUNDLE_NOT_FOUND")]
    BundleNotFound { plan_hash: String },
    #[error("RES_BUNDLE_CORRUPT")]
    BundleCorrupt { plan_hash: String },
    #[error("RES_SOURCE_UNAVAILABLE")]
    SourceUnavailable { retryable: bool },
    #[error("RES_STORE_WRITE_FAILED")]
    StoreWriteFailed { retryable: bool },
    #[error("RES_SOURCE_NOT_BOUND")]
    SourceNotBound,
    #[error("RES_SOURCE_ALREADY_BOUND")]
    SourceAlreadyBound,
}

impl ResolveError {
    /// The registry code; equal to the `Display` text.
    pub fn code(&self) -> &'static str {
        match self {
            ResolveError::RootNotFound { .. } => "RES_ROOT_NOT_FOUND",
            ResolveError::NotInstallable { .. } => "RES_NOT_INSTALLABLE",
            ResolveError::RootNotApproved { .. } => "RES_ROOT_NOT_APPROVED",
            ResolveError::DraftConstituent { .. } => "RES_DRAFT_CONSTITUENT",
            ResolveError::ReferenceUnresolved { .. } => "RES_REFERENCE_UNRESOLVED",
            ResolveError::SourceAmbiguous { .. } => "RES_SOURCE_AMBIGUOUS",
            ResolveError::DependencyCycle { .. } => "RES_DEPENDENCY_CYCLE",
            ResolveError::ClosureTooLarge { .. } => "RES_CLOSURE_TOO_LARGE",
            ResolveError::Conflict { .. } => "RES_CONFLICT",
            ResolveError::PartWithdrawn { .. } => "RES_PART_WITHDRAWN",
            ResolveError::RequiredPackUnsettled { .. } => "RES_REQUIRED_PACK_UNSETTLED",
            ResolveError::PackNotAllowed { .. } => "RES_PACK_NOT_ALLOWED",
            ResolveError::RequiredOverlayMissing { .. } => "RES_REQUIRED_OVERLAY_MISSING",
            ResolveError::OverlayNotAllowed { .. } => "RES_OVERLAY_NOT_ALLOWED",
            ResolveError::OverlayCardinality { .. } => "RES_OVERLAY_CARDINALITY",
            ResolveError::OverlayNotApplicable { .. } => "RES_OVERLAY_NOT_APPLICABLE",
            ResolveError::MemoryCardinality { .. } => "RES_MEMORY_CARDINALITY",
            ResolveError::LoopMissing => "RES_LOOP_MISSING",
            ResolveError::LoopMultiple { .. } => "RES_LOOP_MULTIPLE",
            ResolveError::LoopNotPermitted { .. } => "RES_LOOP_NOT_PERMITTED",
            ResolveError::LoopModeEscape { .. } => "RES_LOOP_MODE_ESCAPE",
            ResolveError::ToolPolicyInvalid { .. } => "RES_TOOL_POLICY_INVALID",
            ResolveError::ManifestInvalid { .. } => "RES_MANIFEST_INVALID",
            ResolveError::DigestMismatch { .. } => "RES_DIGEST_MISMATCH",
            ResolveError::LockPartUnavailable { .. } => "RES_LOCK_PART_UNAVAILABLE",
            ResolveError::LockMismatch { .. } => "RES_LOCK_MISMATCH",
            ResolveError::BundleNotFound { .. } => "RES_BUNDLE_NOT_FOUND",
            ResolveError::BundleCorrupt { .. } => "RES_BUNDLE_CORRUPT",
            ResolveError::SourceUnavailable { .. } => "RES_SOURCE_UNAVAILABLE",
            ResolveError::StoreWriteFailed { .. } => "RES_STORE_WRITE_FAILED",
            ResolveError::SourceNotBound => "RES_SOURCE_NOT_BOUND",
            ResolveError::SourceAlreadyBound => "RES_SOURCE_ALREADY_BOUND",
        }
    }

    /// `RES_SOURCE_UNAVAILABLE` (when the source says so) and `RES_STORE_WRITE_FAILED` are retryable.
    pub fn retryable(&self) -> bool {
        match self {
            ResolveError::SourceUnavailable { retryable }
            | ResolveError::StoreWriteFailed { retryable } => *retryable,
            _ => false,
        }
    }
}

impl CodedError for ResolveError {
    fn code(&self) -> &'static str {
        ResolveError::code(self)
    }
    fn retryable(&self) -> bool {
        ResolveError::retryable(self)
    }
}

/// The mapping every `PartSource` call shares (*Internal Logic* 1): an unreadable source is
/// `RES_SOURCE_UNAVAILABLE` with the source's own retry flag, an unreadable withdrawn list fails closed,
/// and a row whose files are gone is `RES_DIGEST_MISMATCH`.
pub(crate) fn map_source_error(error: PartSourceError) -> ResolveError {
    match error {
        PartSourceError::Unavailable { retryable } => ResolveError::SourceUnavailable { retryable },
        PartSourceError::WithdrawnListUnreadable { .. } => {
            ResolveError::SourceUnavailable { retryable: false }
        }
        PartSourceError::Corrupt { part_identifier } => ResolveError::DigestMismatch {
            part: part_identifier,
        },
    }
}

/// A manifest reader that failed: `RES_MANIFEST_INVALID` naming the part and the reader's code.
pub(crate) fn manifest_invalid(part: &str, error: &ordin_agent_model::AmError) -> ResolveError {
    ResolveError::ManifestInvalid {
        part: part.to_owned(),
        code: error.code().to_owned(),
    }
}

/// A disk failure under the bundle root.
pub(crate) fn store_failed(_error: &std::io::Error) -> ResolveError {
    ResolveError::StoreWriteFailed { retryable: true }
}
