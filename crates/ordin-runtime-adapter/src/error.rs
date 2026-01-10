//! The adapter error, the check error and the registry error, with their registry codes
//! (RuntimeAdapterInterface.md *Error Table*; ErrorHandling.md).

use ordin_types::error_registry::lookup;
use ordin_types::{CodedError, RepairAction};
use serde::{Deserialize, Serialize};

use crate::{AdapterKind, AdapterVerb};

/// Every code this crate owns (ErrorHandling.md *Internal Logic* 11; the `RA_` prefix, 23 codes).
pub const ALL_CODES: &[&str] = &[
    "RA_ADAPTER_DUPLICATE",
    "RA_ADAPTER_UNKNOWN",
    "RA_ALLOWANCE_EXHAUSTED",
    "RA_ATTACHMENT_NOT_GRANTED",
    "RA_COMMAND_UNKNOWN",
    "RA_DEADLINE",
    "RA_EVENT_INVALID",
    "RA_EVENT_ORDER",
    "RA_HANDLE_INVALID",
    "RA_HARNESS_UNTESTED",
    "RA_INPUT_TOO_LARGE",
    "RA_LIMIT_OUT_OF_RANGE",
    "RA_MODEL_INVALID",
    "RA_MODEL_UNKNOWN",
    "RA_PLAN_INVALID",
    "RA_RESUME_MISMATCH",
    "RA_SIGNED_OUT",
    "RA_SNAPSHOT_INCONSISTENT",
    "RA_TARGET_MISMATCH",
    "RA_THREAD_BUSY",
    "RA_UNKNOWN_RUN",
    "RA_UNREACHABLE",
    "RA_VERB_NOT_OFFERED",
];

// SPEC-GAP-1: `CodedError::code` returns `&'static str`, but `InvalidRequest { code }` and
// `Refused { code }` carry an owned `String`. The spec does not say what `code()` returns for a
// string the registry does not hold. A registered string is answered with the registry's own
// `&'static str`; any other string is answered with `SVC_INTERNAL`, the code ErrorHandling.md
// *Internal Logic* 2 turns every unregistered code into at the Service API.
const UNREGISTERED: &str = "SVC_INTERNAL";

/// The registry's `&'static` copy of `code`, or [`UNREGISTERED`].
fn registered(code: &str) -> &'static str {
    lookup(code).map_or(UNREGISTERED, |entry| entry.code)
}

#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AdapterError {
    #[error("verb not offered")]
    VerbNotOffered { verb: AdapterVerb },
    #[error("harness signed out")]
    SignedOut,
    #[error("harness version not tested")]
    HarnessUntested { found: String },
    #[error("route unreachable")]
    Unreachable,
    #[error("plan allowance exhausted")]
    AllowanceExhausted { service_message: String },
    #[error("remote thread busy")]
    ThreadBusy,
    #[error("unknown run")]
    UnknownRun,
    #[error("deadline passed")]
    Deadline,
    /// The request failed the adapter's own validation; `code` is one of `RA_TARGET_MISMATCH`,
    /// `RA_COMMAND_UNKNOWN`, `RA_INPUT_TOO_LARGE`, `RA_HANDLE_INVALID`, `RA_RESUME_MISMATCH`.
    /// 2C wire §2, decision 80: C-13.
    #[error("invalid request")]
    InvalidRequest { code: String },
    /// The remote side refused the request (no access, unknown agent, refused resume); `code` is
    /// the adapter's journalled code. 2C wire §2, decision 80: C-13.
    #[error("refused")]
    Refused { code: String },
}

/// The Error Table's code, retryability and repair action for each variant (Internal Logic 10).
impl CodedError for AdapterError {
    fn code(&self) -> &'static str {
        match self {
            AdapterError::VerbNotOffered { .. } => "RA_VERB_NOT_OFFERED",
            AdapterError::SignedOut => "RA_SIGNED_OUT",
            AdapterError::HarnessUntested { .. } => "RA_HARNESS_UNTESTED",
            AdapterError::Unreachable => "RA_UNREACHABLE",
            AdapterError::AllowanceExhausted { .. } => "RA_ALLOWANCE_EXHAUSTED",
            AdapterError::ThreadBusy => "RA_THREAD_BUSY",
            AdapterError::UnknownRun => "RA_UNKNOWN_RUN",
            AdapterError::Deadline => "RA_DEADLINE",
            AdapterError::InvalidRequest { code } | AdapterError::Refused { code } => {
                registered(code)
            }
        }
    }

    fn retryable(&self) -> bool {
        matches!(
            self,
            AdapterError::Unreachable | AdapterError::ThreadBusy | AdapterError::Deadline
        )
    }

    fn repair(&self) -> Option<RepairAction> {
        match self {
            AdapterError::SignedOut => Some(RepairAction::SignInAgain),
            AdapterError::HarnessUntested { .. } => Some(RepairAction::SendToIt),
            AdapterError::Unreachable | AdapterError::Deadline => Some(RepairAction::TryAgain),
            _ => None,
        }
    }
}

/// A failed check of adapter input or output.
///
/// `Serialize` gives the form the Session Supervisor journals (`{"code":"RA_EVENT_ORDER",
/// "field":"events"}`, *Public Interface* example).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
#[error("{code}")]
pub struct CheckError {
    pub code: &'static str,
    /// The field that failed, as a dotted path (`input.arguments`), for Diagnostics only.
    pub field: &'static str,
}

impl CheckError {
    pub(crate) const fn new(code: &'static str, field: &'static str) -> Self {
        CheckError { code, field }
    }
}

/// `code = self.code`; never retryable: each is a property of the value.
impl CodedError for CheckError {
    fn code(&self) -> &'static str {
        self.code
    }
    fn retryable(&self) -> bool {
        false
    }
}

/// An adapter's own input check becomes `AdapterError::InvalidRequest { code }` (SA-RECON-02).
// spec: ordin-desktop/components/RuntimeAdapterInterface#SA-RECON-02
impl From<CheckError> for AdapterError {
    fn from(e: CheckError) -> Self {
        AdapterError::InvalidRequest {
            code: e.code.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    #[error("adapter kind already registered")]
    Duplicate { kind: AdapterKind },
    #[error("adapter kind not registered")]
    Unknown { kind: AdapterKind },
}

/// `RA_ADAPTER_DUPLICATE` / `RA_ADAPTER_UNKNOWN`; not retryable.
impl CodedError for RegistryError {
    fn code(&self) -> &'static str {
        match self {
            RegistryError::Duplicate { .. } => "RA_ADAPTER_DUPLICATE",
            RegistryError::Unknown { .. } => "RA_ADAPTER_UNKNOWN",
        }
    }
    fn retryable(&self) -> bool {
        false
    }
}
