//! The public value types of the Hook Relay (HookRelay.md *Public Interface*).

use std::path::PathBuf;

use ordin_types::wire::hook::HarnessKind;
use ordin_types::wire::rules::ToolOrigin;
use ordin_types::{AgentInstanceId, ApprovalId, RunId, WorkspaceId};
use serde::{Deserialize, Serialize};

/// What the program passes to [`crate::HookRelay::new`]. The crate reads no environment variable.
#[derive(Debug, Clone)]
pub struct HookRelayConfig {
    pub app_data_root: PathBuf,
    /// Windows only: used in the pipe name `\\.\pipe\ordin-hook-<installation id>`.
    pub installation_id: String,
}

/// One harness session the relay will accept calls for.
#[derive(Debug, Clone)]
pub struct HookSessionSpec {
    pub run_id: RunId,
    pub agent_instance_id: AgentInstanceId,
    pub workspace_id: WorkspaceId,
    pub harness: HarnessKind,
}

/// What the adapter places in the harness's environment. `secret` is shown nowhere else.
pub struct HookSessionEnv {
    pub endpoint: String,
    pub secret: String,
    pub session_id: RunId,
    pub protocol: u16,
}

impl HookSessionEnv {
    /// `[("ORDIN_HOOK_ENDPOINT", …), ("ORDIN_HOOK_SECRET", …), ("ORDIN_SESSION_ID", …),
    /// ("ORDIN_HOOK_PROTOCOL", "1")]`.
    pub fn env_vars(&self) -> [(&'static str, String); 4] {
        [
            ("ORDIN_HOOK_ENDPOINT", self.endpoint.clone()),
            ("ORDIN_HOOK_SECRET", self.secret.clone()),
            ("ORDIN_SESSION_ID", self.session_id.to_string()),
            ("ORDIN_HOOK_PROTOCOL", self.protocol.to_string()),
        ]
    }
}

/// Prints the endpoint and the session only; never the secret.
impl std::fmt::Debug for HookSessionEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookSessionEnv")
            .field("endpoint", &self.endpoint)
            .field("session_id", &self.session_id)
            .finish_non_exhaustive()
    }
}

/// What one session's hook calls did, for the adapter that started the session (it turns them
/// into `Paused { Approval }`, `Resumed` and progress). One stream per session; it ends when the
/// session is closed.
///
/// `ToolOrigin` (2C wire, owned by `ordin-types`) does not implement `PartialEq`, so this enum
/// does not either; callers compare through the serialised form.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HookSessionEvent {
    /// The engine asked a person; the hook is held (D.5.1).
    Holding {
        approval_id: ApprovalId,
        tool: String,
    },
    /// The hook's deadline passed with no decision; the call was denied and a late decision will
    /// resume the session (D.5.2, G).
    DeniedAtDeadline {
        approval_id: ApprovalId,
        tool: String,
    },
    /// The final answer to a tool call: allowed or denied (D.3, D.4, D.5.2).
    Decided { tool: String, allowed: bool },
    /// An after-tool event (E.1); `origin` is the one the call's `PreToolUse` carried.
    ToolUsed {
        tool: String,
        origin: ToolOrigin,
        succeeded: bool,
    },
    /// A stop event arrived (E.2), before the end decision is made.
    TurnStopping,
}
