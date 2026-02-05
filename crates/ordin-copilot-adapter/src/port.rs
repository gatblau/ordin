//! `CopilotPort`, the internal port over the Copilot CLI (CopilotLocalAdapter.md *`CopilotPort`*).
//! Every name here is the spec's; [`crate::CliCopilotPort`] binds each to the CLI's protocol and
//! the scripted double of feature `testkit` implements it for tests.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures::stream::BoxStream;

/// A failed port call.
// SPEC-GAP-1: the spec names `PortError` in every `CopilotPort` and `CopilotServer` signature but
// gives it no fields. The adapter only needs to know that a call failed (each failure maps to one
// Error Table row by the step that made it), so it carries a reason for Diagnostics and nothing else.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{reason}")]
pub struct PortError {
    pub reason: String,
    /// What kind of failure it was, where the adapter acts on the difference.
    pub kind: PortErrorKind,
}

/// The failures the adapter tells apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortErrorKind {
    /// Any other failure.
    Other,
    /// The CLI server does not hold the session named (CPA-CONV-3: it is resumed, then the turn is
    /// sent again).
    SessionNotFound,
}

impl PortError {
    pub fn new(reason: impl Into<String>) -> PortError {
        PortError {
            reason: reason.into(),
            kind: PortErrorKind::Other,
        }
    }

    pub fn session_not_found(reason: impl Into<String>) -> PortError {
        PortError {
            reason: reason.into(),
            kind: PortErrorKind::SessionNotFound,
        }
    }
}

#[async_trait::async_trait]
pub trait CopilotPort: Send + Sync {
    /// Starts one Copilot CLI server for one session (A-11) with the given environment and working
    /// folder.
    async fn start_server(&self, spec: CliServerSpec) -> Result<Box<dyn CopilotServer>, PortError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliServerSpec {
    pub cli_path: PathBuf,
    pub working_folder: PathBuf,
    pub env: BTreeMap<String, String>,
}

#[async_trait::async_trait]
pub trait CopilotServer: Send + Sync {
    /// The CLI's auth.getStatus (09 ADR-044; 08:93).
    async fn auth_status(&self) -> Result<CopilotAuthStatus, PortError>;
    /// Starts the CLI's own `copilot login` flow (C-25, Copilot half open).
    async fn start_login(&self) -> Result<(), PortError>;
    async fn create_session(&self, opts: SessionOptions) -> Result<String, PortError>;
    async fn resume_session(&self, session_id: &str, opts: SessionOptions)
        -> Result<(), PortError>;
    async fn send(&self, session_id: &str, text: &str) -> Result<(), PortError>;
    fn events(&self, session_id: &str) -> BoxStream<'static, CopilotEvent>;
    async fn abort(&self, session_id: &str) -> Result<(), PortError>;
    /// Stops the CLI server; kills it after `deadline`.
    async fn stop(&self, deadline: Duration);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopilotAuthStatus {
    pub signed_in: bool,
    pub method: Option<String>,
}

pub struct SessionOptions {
    pub model: Option<String>,
    /// Same JSON as `SessionMcpConfig::to_mcp_servers_json()` plus package servers.
    pub mcp_servers: serde_json::Value,
    /// Called for every tool request before it executes; must return within 55 s.
    pub pre_tool_use: Arc<dyn ToolGate>,
    /// Called for every permission request the CLI raises.
    pub permission: Arc<dyn ToolGate>,
    /// Called after every tool call.
    pub post_tool_use: Arc<dyn ToolObserver>,
    /// Tools ordin declares in `session.create` and `session.resume` (`tools`).
    // spec: ordin-desktop/components/CopilotLocalAdapter#CPA-HT-1
    pub host_tools: Vec<HostTool>,
    /// Answers every call the CLI sends for a declared tool.
    pub host_tool_handler: Arc<dyn HostToolHandler>,
}

/// A tool ordin declares to the CLI and answers itself.
// spec: ordin-desktop/components/CopilotLocalAdapter#CPA-HT-1
#[derive(Debug, Clone, PartialEq)]
pub struct HostTool {
    /// `mcp__<server>__<tool>`.
    pub name: String,
    pub description: String,
    /// JSON Schema, `"type": "object"` at the root.
    pub parameters: serde_json::Value,
}

/// Answers the CLI's calls of declared tools.
// spec: ordin-desktop/components/CopilotLocalAdapter#CPA-HT-1
#[async_trait::async_trait]
pub trait HostToolHandler: Send + Sync {
    async fn call(
        &self,
        name: &str,
        arguments: serde_json::Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> HostToolAnswer;
}

/// What the CLI is told a declared tool's call produced.
// spec: ordin-desktop/components/CopilotLocalAdapter#CPA-HT-1
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostToolAnswer {
    Success { text: String },
    Failure { text: String },
}

#[async_trait::async_trait]
pub trait ToolGate: Send + Sync {
    async fn decide(
        &self,
        tool: &str,
        arguments: serde_json::Value,
        tool_use_id: Option<String>,
    ) -> GateAnswer;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateAnswer {
    Allow,
    Deny { reason: String },
}

#[async_trait::async_trait]
pub trait ToolObserver: Send + Sync {
    async fn tool_used(&self, tool: &str, tool_use_id: Option<String>, succeeded: bool);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopilotEvent {
    SessionStarted,
    AssistantText {
        text: String,
    },
    ToolStarted {
        tool: String,
    },
    ToolFinished {
        tool: String,
        succeeded: bool,
    },
    /// The CLI's to-do list, where its events carry it (10 C-41).
    TodoList {
        items: Vec<String>,
    },
    Usage {
        requests: Option<u32>,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        cached_input_tokens: Option<u64>,
    },
    /// The turn ended; `final_text` is the assistant's last message.
    TurnEnded {
        final_text: Option<String>,
    },
    SignedOut,
    AllowanceExhausted {
        message: String,
    },
    Error {
        message: String,
    },
    ServerExited {
        status: Option<i32>,
    },
}
