//! The shared types of the workspace: Phase 2C rendered as Rust, and the helpers every module needs to
//! build and check them the same way (SharedTypes.md).
//!
//! Identifiers, the millisecond UTC timestamp, text checks, the decimal-string and SHA-256 forms,
//! base64url, the keyset page, the one restricted YAML reader and the coded-error contract live at the
//! crate's top-level modules. Domain and wire types are reached by module path only
//! (`ordin_types::domain::runs::Run`, `ordin_types::wire::approval::ApprovalCard`), never through the
//! prelude or the root, so two types of one name cannot be imported by accident (SA-RECON-01).
//!
//! Besides SharedTypes.md, three cross-cutting parts land here: the error registry (ErrorHandling.md,
//! module [`error_registry`]), the log line form (Logging.md, module [`log`]) and the input-validation
//! helpers (InputValidation.md, modules [`text`], [`paging`], [`yaml`]).
#![forbid(unsafe_code)]

pub mod common;
pub mod domain;
pub mod encoding;
pub mod error;
pub mod error_registry;
pub mod id;
pub mod log;
pub mod num;
pub mod paging;
pub mod session;
pub mod text;
pub mod time;
pub mod wire;
pub mod yaml;

pub use error::{CodedError, RepairAction, TypesError, ALL_CODES};
pub use id::*;
pub use paging::{Cursor, Page, PageRequest};

/// Every identifier and the shared value types, for `use ordin_types::prelude::*`.
pub mod prelude {
    pub use crate::common::{AgentRef, InputUsed, SetBy, Setting};
    pub use crate::error::{CodedError, RepairAction, TypesError};
    pub use crate::id::*;
    pub use crate::num::{DecimalString, Sha256Hex};
    pub use crate::paging::{Cursor, Page, PageRequest};
    pub use crate::time::Timestamp;
    pub use crate::wire::events::EventKind;
    pub use crate::wire::run::{
        AdapterKind, AdapterVerb, ModelChoice, ModelLevel, ProgressCount, RunLimits, RunProgress,
        RunTarget, StepMark, ThinkingLevel,
    };
}
