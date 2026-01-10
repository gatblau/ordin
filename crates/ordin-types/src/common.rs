//! The small shared types of the 2C domain conventions block.

use serde::{Deserialize, Serialize};

use crate::id::{AgentInstanceId, RemoteAgentId};

/// Who fixed a setting's current value (04:181, 04:185).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SetBy {
    Person,
    Company,
}

/// A setting value with the mark of who set it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct Setting<T> {
    pub value: T,
    pub set_by: SetBy,
}

/// An agent instance or a remote agent (crew members, run agents, workflow agents).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentRef {
    AgentInstance { agent_instance_id: AgentInstanceId },
    RemoteAgent { remote_agent_id: RemoteAgentId },
}

/// One input as a run or crew run used it: name, where it lives, version read (A-29, 10:41).
///
/// Limits (2C shared rule table): `name` 1–120 chars (`INPUT_NAME_INVALID`), `location` 1–2048 chars in
/// one of the 2C location forms (`INPUT_LOCATION_INVALID`), `version_read` 1–256 chars
/// (`INPUT_VERSION_INVALID`); the writing module checks them with [`crate::text::check_len`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub struct InputUsed {
    pub name: String,
    pub location: String,
    pub version_read: String,
}
