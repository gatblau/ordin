//! The call context, the static description of a command, and the input contract every command input
//! type implements (ServiceApi.md *The `ServiceApi` trait and call context*, *Command input and result
//! types*).

use ordin_types::{CommandRequestId, DesktopPairingId, RepairAction};
use serde::{Deserialize, Serialize};

use crate::wire::ServiceApiVersion;

/// Where a command came from. The transports set it; a module never trusts a value from input.
#[derive(Debug, Clone)]
pub enum CallOrigin {
    /// The desktop's own screens, over Tauri IPC.
    LocalScreens,
    /// A paired desktop, over the Remote API (personal profile only).
    PairedDesktop {
        desktop_pairing_id: DesktopPairingId,
        computer_name: String,
    },
}

impl CallOrigin {
    /// The value of the `origin` field of the `svc.command` log line (Internal Logic 5).
    pub(crate) fn log_name(&self) -> &'static str {
        match self {
            CallOrigin::LocalScreens => "local",
            CallOrigin::PairedDesktop { .. } => "paired_desktop",
        }
    }
}

/// Per-command context.
#[derive(Debug, Clone)]
pub struct CallContext {
    pub request_id: CommandRequestId,
    /// The request id as a string; carried into every log line of the call.
    pub correlation_id: String,
    pub origin: CallOrigin,
    /// The time by which the reply must be sent (Internal Logic step 3).
    pub deadline: std::time::Instant,
    /// Cancelled when the transport drops the call (window closed, stream closed).
    pub cancel: tokio_util::sync::CancellationToken,
}

/// Turns a code into the one sentence and the action the person sees (implemented by the program with
/// Localisation; ADR-100 hub pattern).
pub trait ErrorSentences: Send + Sync {
    fn sentence(&self, code: &str) -> String;
    fn action(&self, code: &str) -> Option<RepairAction>;
}

/// Static description of one command.
#[derive(Debug, Clone, Copy)]
pub struct CommandMeta {
    /// `"approval.decide"`.
    pub name: &'static str,
    /// `"approval"`.
    pub module: &'static str,
    /// `"decide"`.
    pub verb: &'static str,
    /// `"approval_decide"`.
    pub ipc_name: &'static str,
    /// `"/v1/approval/decide"`.
    pub rest_path: &'static str,
    pub scope: Scope,
    pub effect: Effect,
    pub budget: Budget,
    /// 1.0 for every row of the catalogue.
    pub since: ServiceApiVersion,
    pub input_schema: fn(&mut schemars::SchemaGenerator) -> schemars::Schema,
    pub result_schema: fn(&mut schemars::SchemaGenerator) -> schemars::Schema,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Served by any backend: the desktop's own core and a personal server's Remote API.
    Any,
    /// Served only to `CallOrigin::LocalScreens` (desktop-only features, OQ-60).
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Read,
    Change,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Budget {
    /// A-03 / A-46: 150 ms p95.
    Interactive,
    /// A-21: first results within 1 s (`search.query`, `files.search`, `catalogue.search`,
    /// `catalogue.facets`).
    Search,
    /// A transfer of up to 25 MiB (`files.download`); deadline 60 s (Internal Logic step 3).
    Transfer,
}

/// The empty result. Serialises as `{}`.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Done {}

/// The contract of every command input type.
///
/// The NFC normalisation and the control-character refusal of every text field (the *any text field
/// of any input* row) are applied by [`crate::accept`] to the input's JSON before it is read into the
/// typed value, so every string of every input, nested or not, passes the same rule; `check` applies
/// the field limits (or, for a restated owner type, the owner's validation rows) to the typed value.
pub trait CommandInput:
    serde::de::DeserializeOwned + schemars::JsonSchema + Send + 'static
{
    /// Applies the input's rules. Returns the code and the JSON Pointer of the first failing field.
    fn check(&mut self) -> Result<(), InputRejection>;
}

/// A refused input: the code and the JSON Pointer (from the envelope root, `/input/...`) of the first
/// failing field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InputRejection {
    pub code: &'static str,
    pub at: String,
}

impl CommandInput for Done {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}
