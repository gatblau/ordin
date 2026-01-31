//! 2C part 2 §4 ordin hook protocol (ADR-045).

use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::prelude::*;

// ---- HookRequest ----
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HookRequest {
    pub protocol: u16,
    pub call_id: HookCallId,
    /// From ORDIN_SESSION_ID.
    pub session: RunId,
    pub agent_instance_id: AgentInstanceId,
    /// HMAC-SHA256 of the canonical JSON of `message`, keyed with ORDIN_HOOK_SECRET,
    /// base64url without padding.
    pub mac: String,
    pub message: HookMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HookMessage {
    /// Before every tool call: decided by the Rules Engine.
    PreToolUse {
        harness: HarnessKind,
        tool: String,
        origin: ToolOrigin,
        arguments: serde_json::Value,
        /// The harness's own tool-use identifier, where it gives one (`tool_use_id`).
        #[serde(skip_serializing_if = "Option::is_none")]
        harness_tool_use_id: Option<String>,
        /// The sub-agent that asked, when the harness payload names it. Conditional on C-78:
        /// filled only once the spike confirms the payload carries it; absent until then
        /// (additive; amended January 2026, decision 80: C-20).
        #[serde(skip_serializing_if = "Option::is_none")]
        subagent: Option<String>,
    },
    /// After a tool call: feeds the journal (ADR-045 *Other events*).
    PostToolUse {
        harness: HarnessKind,
        tool: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        harness_tool_use_id: Option<String>,
        succeeded: bool,
    },
    /// The agent's turn is ending: lets Ordin check the result first (ADR-036).
    Stop { harness: HarnessKind },
}

// `HarnessKind` is defined once, in part 1 (`ordin_types::domain::identity`, entity
// `HarnessAttachment`); this module re-exports it (amended January 2026, decision 80: C-1).
pub use crate::domain::identity::HarnessKind;

// ---- HookReply ----
/// The relay writes zero or one `Holding` line, then exactly one final line.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HookReply {
    /// The decision is *ask*: the card is open; the hook keeps waiting until `deadline`.
    Holding {
        call_id: HookCallId,
        approval_id: ApprovalId,
        deadline: Timestamp,
    },
    Allow {
        call_id: HookCallId,
    },
    /// To a `stop`, `Deny` means "do not end yet": the harness keeps the turn open and
    /// `reason` is given to the agent (amended January 2026, decision 80: C-20).
    Deny {
        call_id: HookCallId,
        reason: String,
    },
    /// Reply to `post_tool_use` and `stop`.
    Ack {
        call_id: HookCallId,
    },
}
