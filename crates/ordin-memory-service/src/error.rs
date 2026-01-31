//! The Memory Service's one error type, the codes it owns and the mapping of the memory component's
//! errors onto them (MemoryService.md *Error Table*; ErrorHandling.md *Internal Logic* 11).

use memory::error::{AppError, ValidationKind};
use memory::service::CallError;
use memory::types::ports::{ProviderError, StoreError};
use ordin_types::{CodedError, RepairAction};

/// Every code the registry gives this crate (its `MEM_` rows).
pub const ALL_CODES: &[&str] = &[
    "MEM_ABOUT_INVALID",
    "MEM_ADOPT_PERSONAL_DATA",
    "MEM_ADOPT_REFUSED",
    "MEM_ADOPT_SOURCE_NOT_READ",
    "MEM_ALREADY_EVERYWHERE",
    "MEM_ALREADY_KEPT",
    "MEM_CLASS_EXCLUDED",
    "MEM_CLASS_INVALID",
    "MEM_CLASS_NOT_RETAINED",
    "MEM_CLASS_REQUIRED",
    "MEM_DELETE_UNVERIFIED",
    "MEM_DIM_MISMATCH",
    "MEM_ENTITIES_INVALID",
    "MEM_FACT_NOT_FOUND",
    "MEM_FOLDER_UNKNOWN",
    "MEM_HELD_CHANGED",
    "MEM_IDS_INVALID",
    "MEM_INTERNAL",
    "MEM_INVALID_ARGUMENTS",
    "MEM_LIMIT_INVALID",
    "MEM_LINKS_INVALID",
    "MEM_LINK_NOT_FOUND",
    "MEM_MADE_FROM_INVALID",
    "MEM_MADE_FROM_NOT_ALLOWED",
    "MEM_MODEL_UNAVAILABLE",
    "MEM_NOT_HELD",
    "MEM_NOT_SUPPORTED",
    "MEM_PAGE_SECTIONS_INVALID",
    "MEM_PAGE_SECTION_INVALID",
    "MEM_QUERY_INVALID",
    "MEM_QUEUE_UNAVAILABLE",
    "MEM_RATE_LIMITED",
    "MEM_RECALL_IDS_INVALID",
    "MEM_RESULT_CAP_INVALID",
    "MEM_RULES_INVALID",
    "MEM_SAME_THING",
    "MEM_SCOPE_CROSS_WRITE",
    "MEM_SCOPE_MISSING",
    "MEM_SEARCHES_INVALID",
    "MEM_SEARCH_INVALID",
    "MEM_SOURCE_INVALID",
    "MEM_STARTUP_FAILED",
    "MEM_STATEMENT_INVALID",
    "MEM_STORE_TIMEOUT",
    "MEM_STORE_UNAVAILABLE",
    "MEM_TEAM_BATCH_INVALID",
    "MEM_TEAM_FACT_ELSEWHERE",
    "MEM_TEAM_FACT_INVALID",
    "MEM_TEAM_GATE_REFUSED",
    "MEM_THING_COUNT_CHANGED",
    "MEM_THING_NOT_FOUND",
    "MEM_TOOL_UNKNOWN",
    "MEM_TOO_MANY_FOLDERS",
    "MEM_TRUST_NOT_ALLOWED",
    "MEM_WIDER_NOT_SUPPORTED",
];

/// Every failure of this crate (*Public Interface*).
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("{code}")]
    Invalid {
        code: &'static str,
        field: &'static str,
    },
    #[error("{code}")]
    Refused { code: &'static str },
    #[error("{code}")]
    NotFound { code: &'static str },
    #[error("{code}")]
    Unavailable { code: &'static str, retryable: bool },
    #[error("{code}")]
    Internal { code: &'static str },
}

impl MemoryError {
    /// The registry code.
    pub fn code(&self) -> &'static str {
        match self {
            MemoryError::Invalid { code, .. }
            | MemoryError::Refused { code }
            | MemoryError::NotFound { code }
            | MemoryError::Unavailable { code, .. }
            | MemoryError::Internal { code } => code,
        }
    }

    /// True when the same call may succeed if repeated unchanged later.
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            MemoryError::Unavailable {
                retryable: true,
                ..
            }
        )
    }

    pub(crate) fn internal() -> MemoryError {
        MemoryError::Internal {
            code: "MEM_INTERNAL",
        }
    }

    pub(crate) fn store_unavailable() -> MemoryError {
        MemoryError::Unavailable {
            code: "MEM_STORE_UNAVAILABLE",
            retryable: true,
        }
    }

    pub(crate) fn store_timeout() -> MemoryError {
        MemoryError::Unavailable {
            code: "MEM_STORE_TIMEOUT",
            retryable: true,
        }
    }

    pub(crate) fn fact_not_found() -> MemoryError {
        MemoryError::NotFound {
            code: "MEM_FACT_NOT_FOUND",
        }
    }

    pub(crate) fn thing_not_found() -> MemoryError {
        MemoryError::NotFound {
            code: "MEM_THING_NOT_FOUND",
        }
    }

    pub(crate) fn link_not_found() -> MemoryError {
        MemoryError::NotFound {
            code: "MEM_LINK_NOT_FOUND",
        }
    }

    pub(crate) fn not_supported() -> MemoryError {
        MemoryError::Refused {
            code: "MEM_NOT_SUPPORTED",
        }
    }

    pub(crate) fn delete_unverified() -> MemoryError {
        MemoryError::Refused {
            code: "MEM_DELETE_UNVERIFIED",
        }
    }

    pub(crate) fn invalid(code: &'static str, field: &'static str) -> MemoryError {
        MemoryError::Invalid { code, field }
    }
}

impl CodedError for MemoryError {
    fn code(&self) -> &'static str {
        MemoryError::code(self)
    }

    fn retryable(&self) -> bool {
        MemoryError::retryable(self)
    }

    /// The `action` of the Error Table's Service API replies.
    fn repair(&self) -> Option<RepairAction> {
        match self.code() {
            "MEM_DIM_MISMATCH" => None,
            "MEM_STORE_UNAVAILABLE"
            | "MEM_STORE_TIMEOUT"
            | "MEM_QUEUE_UNAVAILABLE"
            | "MEM_DELETE_UNVERIFIED"
            | "MEM_THING_COUNT_CHANGED" => Some(RepairAction::TryAgain),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for MemoryError {
    fn from(error: sqlx::Error) -> MemoryError {
        match &error {
            sqlx::Error::PoolTimedOut => MemoryError::store_timeout(),
            sqlx::Error::ColumnDecode { .. }
            | sqlx::Error::ColumnNotFound(_)
            | sqlx::Error::ColumnIndexOutOfBounds { .. }
            | sqlx::Error::TypeNotFound { .. }
            | sqlx::Error::Decode(_) => MemoryError::internal(),
            other if is_locked(&other.to_string()) => MemoryError::store_timeout(),
            _ => MemoryError::store_unavailable(),
        }
    }
}

/// SQLite answers a statement that waited out the busy timeout with `database is locked`, which the
/// memory component's error mapping files under "unavailable". For this service that wait is the
/// store statement timeout of the Error Table (scenario *the store is unreachable*).
fn is_locked(detail: &str) -> bool {
    detail.contains("database is locked") || detail.contains("database table is locked")
}

/// The fixed sentence of a code for a tool result (*Error Table*). `MEM_CLASS_REQUIRED`,
/// `MEM_FOLDER_UNKNOWN` and `MEM_INVALID_ARGUMENTS` carry a value and are completed by the caller.
pub(crate) fn sentence(code: &str, retryable: bool) -> &'static str {
    match code {
        "MEM_HELD_CHANGED" => "This fact changed after you read it. Read it again before keeping it.",
        "MEM_MODEL_UNAVAILABLE" if retryable => "Memory did not answer in time; try again.",
        "MEM_MODEL_UNAVAILABLE" => "Memory is not available on this computer right now.",
        "MEM_STATEMENT_INVALID" => "statement: one plain sentence of 1 to 500 characters.",
        "MEM_ABOUT_INVALID" => "about: 1 to 200 characters.",
        "MEM_CLASS_INVALID" => "fact_class: 1 to 128 characters.",
        "MEM_SOURCE_INVALID" => "source: origin_ref 1 to 2048 characters and version 1 to 256, in plain text, naming a document, with no personal data or instructions.",
        "MEM_QUERY_INVALID" => "query: 1 to 4096 characters.",
        "MEM_RESULT_CAP_INVALID" => "result_cap: 1 to 20.",
        "MEM_TOOL_UNKNOWN" => "The memory server offers recall and remember.",
        "MEM_CLASS_EXCLUDED" => "This agent's memory rules forbid keeping facts of that class.",
        "MEM_CLASS_NOT_RETAINED" => {
            "This agent's memory rules do not let it keep facts of that class."
        }
        "MEM_SCOPE_MISSING" => "Memory is kept by conversation; this run has none.",
        "MEM_SCOPE_CROSS_WRITE" => "Facts from this conversation stay in this conversation.",
        "MEM_RATE_LIMITED" => "Too many memory calls just now; try again in a minute.",
        "MEM_STORE_UNAVAILABLE" | "MEM_STORE_TIMEOUT" | "MEM_QUEUE_UNAVAILABLE" => {
            "Memory is not available just now."
        }
        "MEM_SEARCHES_INVALID" => {
            "Give either query or 1 to 4 searches, each {type: lex or vec, query: 1 to 500 characters}."
        }
        "MEM_TOO_MANY_FOLDERS" => {
            "This workspace reads too many folders to search at once; name the ones to search in folders."
        }
        "MEM_ENTITIES_INVALID" => {
            "entities: up to 16, each {name: 1 to 200 characters, other_names: up to 8}."
        }
        "MEM_LINKS_INVALID" => {
            "links: up to 16, each {from, kind: 1 to 64 characters, to}; from and to must be things named in entities or about."
        }
        "MEM_MADE_FROM_NOT_ALLOWED" => "Only Ordin writes summaries.",
        "MEM_MADE_FROM_INVALID" => "made_from: 2 to 64 facts kept in this workspace.",
        "MEM_WIDER_NOT_SUPPORTED" => "wider is not available here.",
        _ => "Memory could not do that.",
    }
}

/// The `Err` of `Service::remember`, whose `VAL_*` findings name the field (§4 steps 5 and 6 of the
/// ADR-111 change).
pub(crate) fn map_remember_error(error: CallError) -> MemoryError {
    if let AppError::Validation(kind, field) = &error.error {
        let invalid = |code| MemoryError::Invalid {
            code,
            field: "body",
        };
        match kind {
            ValidationKind::InvalidBody if field.starts_with("entities") => {
                return invalid("MEM_ENTITIES_INVALID")
            }
            ValidationKind::InvalidBody if field.starts_with("links") => {
                return invalid("MEM_LINKS_INVALID")
            }
            ValidationKind::InvalidBody if field.starts_with("derived_from") => {
                return invalid("MEM_MADE_FROM_INVALID")
            }
            // A first text the store could not name: only a 4,000-character section or document
            // fact with neither a subject nor a thing can reach it.
            ValidationKind::OutOfRange if field == "remember first text" => {
                return invalid("MEM_STATEMENT_INVALID")
            }
            _ => {}
        }
    }
    map_call_error(error)
}

/// §4 step 5 and §5 step 2: a `CallError` by its code. An unverified deletion carries `INTERNAL`,
/// so it is told by the store error underneath.
// spec: ordin-desktop/components/MemoryService#MEM_STORE_TIMEOUT
pub(crate) fn map_call_error(error: CallError) -> MemoryError {
    let code = error.error.code();
    match &error.error {
        AppError::Store(StoreError::PartialDelete { .. }) => {
            return MemoryError::delete_unverified()
        }
        AppError::Store(StoreError::Unavailable(detail)) if is_locked(detail) => {
            return MemoryError::store_timeout()
        }
        AppError::Provider(ProviderError::Timeout) | AppError::RecallTimeout => {
            return MemoryError::Unavailable {
                code: "MEM_MODEL_UNAVAILABLE",
                retryable: true,
            }
        }
        _ => {}
    }
    match code {
        "RATE_LIMITED" => MemoryError::Unavailable {
            code: "MEM_RATE_LIMITED",
            retryable: true,
        },
        "AUTHZ_CLASS_FORBIDDEN" => MemoryError::Refused {
            code: "MEM_CLASS_NOT_RETAINED",
        },
        "STORE_UNAVAILABLE" => MemoryError::store_unavailable(),
        "STORE_TIMEOUT" => MemoryError::store_timeout(),
        "QUEUE_UNAVAILABLE" => MemoryError::Unavailable {
            code: "MEM_QUEUE_UNAVAILABLE",
            retryable: true,
        },
        "PROVIDER_TIMEOUT" | "PROVIDER_ERROR" => MemoryError::Unavailable {
            code: "MEM_MODEL_UNAVAILABLE",
            retryable: true,
        },
        // spec: ordin-desktop/components/MemoryService#MEM_TOO_MANY_FOLDERS
        "VAL_TOO_MANY_COLLECTIONS" => MemoryError::Refused {
            code: "MEM_TOO_MANY_FOLDERS",
        },
        other => {
            if other.starts_with("VAL_") {
                tracing::error!(target: "ordin::memory", code = other, "memory.body_refused");
            }
            MemoryError::internal()
        }
    }
}

/// `NOT_FOUND` from a call on one fact.
pub(crate) fn is_not_found(error: &CallError) -> bool {
    error.error.code() == "NOT_FOUND"
}
