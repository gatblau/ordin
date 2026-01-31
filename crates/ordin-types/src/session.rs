//! Who a session's ordin tools act for (module `ordin_types::session`, 2C wire §2, decision 80).

use serde::{Deserialize, Serialize};

use crate::id::CrewRunId;

/// Who a session's ordin tools act for (2C wire §2). Carried by `StartRun.caller`
/// (`ordin-runtime-adapter`) and by the MCP Front's `SessionGrant`; it lives here because the
/// MCP Front has no build edge to `ordin-runtime-adapter`. Both crates re-export it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolCaller {
    /// Ordin's own session.
    Ordin,
    /// An agent's session started from a task.
    Agent,
    /// A crew member's session in a crew round.
    CrewMember { crew_run_id: CrewRunId },
    /// The hosted agent's session (agent profile).
    HostedAgent,
}
