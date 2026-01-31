//! The MCP Front: the hub through which every ordin service reaches a harness session as an MCP
//! server (McpFront.md). It owns the `ToolProvider` trait, answers each session's tool list with
//! only the tools that session is offered, checks every call's arguments in fixed code, has the
//! Rules Engine decide every call before a provider acts, relays progress counts to the Journal and
//! serves its own `work` tools (`report`, `plan`, `read_rest`).
//!
//! The JSON-RPC methods of the bridge connection are answered directly (see `bridge`); the crate
//! owns no durable data and registers no migration.

mod bridge;
mod call;
mod error;
mod front;
mod names;
mod provider;
mod types;
mod util;
mod work;

pub use error::{McpError, ALL_CODES};
pub use front::McpFront;
pub use names::ServerName;
pub use provider::{
    ArgumentProblem, CallVia, ProgressSink, ServerSpec, ShownBlockStore, ToolCallContext,
    ToolDispatch, ToolFailure, ToolKey, ToolOutput, ToolProvider, ToolRegistry, ToolSpec,
};
pub use types::{
    CallOutcome, McpFrontConfig, McpMetrics, McpServerLaunch, OfferedTool, ProgramProfile,
    SessionGrant, SessionMcpConfig, ToolCallResult, ToolCaller,
};
