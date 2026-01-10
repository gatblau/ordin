//! 2C part 2 §3 Rules Engine tool decision.

use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::prelude::*;

// ---- ToolOrigin ----
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolOrigin {
    BuiltIn,
    OrdinMcpFront { server: String },
    PackageMcpServer { server: String },
}

// ---- ToolCall ----
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ToolCall {
    pub run_id: RunId,
    pub agent_instance_id: AgentInstanceId,
    pub workspace_id: WorkspaceId,
    pub point: DecisionPoint,
    pub tool: String,
    pub origin: ToolOrigin,
    /// The arguments as the harness sent them; never journalled, only their digest.
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DecisionPoint {
    RunStart,
    BeforeToolCall,
    RunEnd,
}

// ---- ToolDecision ----
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
/// The Rules Engine's full decision; keeps its name. The domain part's stored verdict of a
/// Tool request is `ToolRequestDecision` (amended January 2026, decision 80: C-2).
pub struct ToolDecision {
    #[serde(flatten)]
    pub verdict: Verdict,
    pub rule: RuleRef,
    pub decided_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Verdict {
    Allow,
    Deny {
        reason: String,
    },
    /// Ask a person: what the Approval Service builds the card from.
    Ask {
        proposal: ActionProposal,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct RuleRef {
    pub source: RuleSource,
    /// Rule identifier within its source (a declaration rule name, or a fixed application
    /// policy identifier such as `app.folder_grant`).
    pub rule_id: String,
    /// For declaration and agent-tool-policy rules: the agent instance whose rule set it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_instance_id: Option<AgentInstanceId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum RuleSource {
    DeclarationGate,
    DeclarationToolRule,
    DeclarationArgumentRule,
    AgentToolPolicy,
    AppFolderPolicy,
    AppShellPolicy,
    AppApprovalPolicy,
    /// No rule could be evaluated (parse error, engine error): deny, fail closed.
    FailClosed,
}

/// What an *ask* proposes; the Approval Service turns it into an `ApprovalCard`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ActionProposal {
    pub action: String,
    pub destination: String,
    pub affects: Vec<AffectedItem>,
    pub consequence: String,
}
