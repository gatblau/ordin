//! The run types shared by both contract crates (2C wire §1 and §2 *Placement*; decision 80: C-11,
//! C-17; Internal Logic 5). `ordin-service-api` and `ordin-runtime-adapter` re-export this module
//! (`pub use ordin_types::wire::run::*`), so each type has one definition.

use serde::{Deserialize, Serialize};

use crate::id::{AgentInstanceId, RemoteAgentId};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct RunProgress {
    /// One plain sentence, at most 160 characters.
    pub line: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<ProgressCount>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct ProgressCount {
    pub done: u64,
    /// Present only when the source carried a real total ("12 of 40"); absent → "12 so far".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum StepMark {
    Next,
    Now,
    Done,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum AdapterKind {
    ClaudeLocal,
    CopilotLocal,
    /// The wrightrun HTTP Adapter; the Hosted Agent Adapter from revision 0.43.0.
    HostedAgent,
    SlackThread,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum AdapterVerb {
    Start,
    Progress,
    Pause,
    Resume,
    Read,
    Cancel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunTarget {
    AgentInstance { agent_instance_id: AgentInstanceId },
    RemoteAgent { remote_agent_id: RemoteAgentId },
}

/// One definition for the domain `Run.limits` and the wire `StartRun.limits`, with the union of both
/// 2C parts' derive lists (Internal Logic 6). `request_budget` is optional because
/// `07-cross-cutting.md:49` sets no default ceiling until OQ-3; when present it is ≥ 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct RunLimits {
    pub wall_clock_secs: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_budget: Option<u32>,
    pub idle_timeout_secs: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ModelLevel {
    Automatic,
    Smaller,
    Standard,
    MostCapable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ThinkingLevel {
    Low,
    Automatic,
    High,
}

/// The model a run starts on. `model` is the harness's own model name; `None` means the
/// harness's own automatic selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ModelChoice {
    pub level: ModelLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}
