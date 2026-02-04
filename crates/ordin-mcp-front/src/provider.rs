//! The `ToolProvider` trait and the types around it (McpFront.md *The `ToolProvider` trait*, ADR-100).

use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;
use ordin_service_api::blocks::Block;
use ordin_types::{
    AgentInstanceId, BlockId, CodingTaskId, RunId, ScopeId, ToolRequestId, WorkspaceId,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::error::McpError;
use crate::names::ServerName;
use crate::types::{SessionGrant, ToolCallResult, ToolCaller};
use crate::util::lock;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolKey {
    pub server: ServerName,
    /// `^[A-Za-z][A-Za-z0-9_-]{0,63}$` (SA-RECON-04)
    pub tool: String,
}

#[derive(Debug, Clone)]
pub struct ServerSpec {
    pub name: ServerName,
    pub tools: Vec<ToolSpec>,
}

#[derive(Debug, Clone)]
pub struct ToolSpec {
    /// `^[A-Za-z][A-Za-z0-9_-]{0,63}$`, unique within its server (SA-RECON-04).
    pub name: String,
    /// One line, no line break: 1-120 characters in the desktop and personal-server profiles,
    /// 1-1,024 characters in the agent profile (SA-RECON-04).
    pub description: String,
    /// JSON Schema (draft 2020-12); `"type": "object"` at the root.
    pub input_schema: serde_json::Value,
    /// One line, 1-300 characters: the correct form, quoted to the model on a wrong call.
    pub argument_form: String,
    /// Offered in the `ordin_tools` sandbox module (SA-NAME-06).
    pub sandbox_callable: bool,
    /// Start-up report sentence, 1-200 characters, with no identifier; also the progress line
    /// for this tool's counts.
    pub plain: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallVia {
    Harness,
    Sandbox,
}

#[derive(Default)]
struct ProgressShared {
    slot: Mutex<Option<(u64, Option<u64>)>>,
    notify: Notify,
}

/// Hands progress counts to the MCP Front, which relays them to the harness (when the call
/// carried a progress token) and to the Journal as `RunProgress` counts.
#[derive(Clone)]
pub struct ProgressSink {
    shared: Arc<ProgressShared>,
}

impl ProgressSink {
    pub(crate) fn new() -> Self {
        ProgressSink {
            shared: Arc::new(ProgressShared::default()),
        }
    }

    /// A sink that nobody reads, for a provider's own tests that build a `ToolCallContext`.
    pub fn detached() -> Self {
        ProgressSink::new()
    }

    /// `total` only when the source carried a real total (ADR-068). Never blocks; an unsent count
    /// is replaced by the latest one (the latest count supersedes it).
    pub fn report(&self, done: u64, total: Option<u64>) {
        *lock(&self.shared.slot) = Some((done, total));
        self.shared.notify.notify_one();
    }

    pub(crate) fn take(&self) -> Option<(u64, Option<u64>)> {
        lock(&self.shared.slot).take()
    }

    pub(crate) async fn notified(&self) {
        self.shared.notify.notified().await;
    }
}

#[derive(Clone)]
pub struct ToolCallContext {
    pub run_id: RunId,
    pub workspace_id: WorkspaceId,
    pub agent_instance_id: AgentInstanceId,
    /// Copied from the session's `SessionGrant` (SA-RECON-01).
    pub caller: ToolCaller,
    pub scope_id: Option<ScopeId>,
    pub coding_task_id: Option<CodingTaskId>,
    /// The Rules Engine's tool request identifier for the call.
    pub call_id: ToolRequestId,
    pub correlation_id: String,
    pub deadline: Instant,
    pub cancel: CancellationToken,
    pub progress: ProgressSink,
    /// **Give agents only the parts they need** (ADR-062).
    pub only_needed_parts: bool,
    pub via: CallVia,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolOutput {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolFailure {
    /// `<PREFIX>_<NAME>` of the provider's registry entry.
    pub code: String,
    /// One sentence for the model, 1-2,000 characters; no path outside the session's places,
    /// no token, no stack.
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ArgumentProblem {
    /// JSON Pointer to the failing field; `""` for the whole object.
    pub at: String,
    /// 1-300 characters.
    pub reason: String,
}

#[async_trait]
pub trait ToolProvider: Send + Sync + 'static {
    /// Stable identifier, `^[a-z][a-z0-9-]{0,39}$`, e.g. "ordin-host".
    fn provider_id(&self) -> &'static str;

    /// Servers and tools this provider serves. Read at `register`, at every `open_session` and
    /// at every `tools_changed` for one of its servers.
    fn servers(&self) -> Vec<ServerSpec>;

    /// A server-name prefix (for example `"craft_"`) under which this provider may list servers
    /// that appear after `start` (SA-RECON-06). Pattern `^[a-z][a-z0-9]{0,15}_$`. Default: none.
    fn server_prefix(&self) -> Option<&'static str> {
        None
    }

    /// Which of its tools this session is offered. Must answer within 2 s.
    async fn offer(&self, grant: &SessionGrant) -> Result<Vec<ToolKey>, ToolFailure>;

    /// Fixed-code argument check, no model call (ADR-062). Must not block.
    fn check_arguments(
        &self,
        key: &ToolKey,
        arguments: &serde_json::Value,
    ) -> Result<(), ArgumentProblem>;

    /// Runs a call the Rules Engine allowed (or the person approved). Must honour `ctx.cancel`
    /// and `ctx.deadline`.
    async fn call(
        &self,
        ctx: ToolCallContext,
        key: &ToolKey,
        arguments: serde_json::Value,
    ) -> Result<ToolOutput, ToolFailure>;

    /// The session ended; drop per-session state. Infallible.
    async fn session_closed(&self, run_id: RunId);
}

/// How providers reach the hub (SA-RECON-01). Implemented by `McpFront`.
pub trait ToolRegistry: Send + Sync {
    /// Same as `McpFront::register`. Only before `start`.
    fn register(&self, provider: Arc<dyn ToolProvider>) -> Result<(), McpError>;

    /// The provider serving `server` changed what it offers. Re-runs that provider's `offer` for
    /// every open session in the background and returns at once (SA-RECON-02). Infallible.
    fn tools_changed(&self, server: &ServerName);

    /// The tool names a registered provider's current `servers()` lists for `server`, sorted;
    /// `None` when no provider serves it. Infallible.
    fn served_tools(&self, server: &ServerName) -> Option<Vec<String>>;
}

/// The route for calls made by agent-written code through the Sandbox's `ordin_tools` module
/// (SA-RECON-05). Defined and implemented here; the Sandbox consumes it.
#[async_trait]
pub trait ToolDispatch: Send + Sync {
    /// The session's offered tools whose spec is `sandbox_callable`, sorted by server then tool.
    fn session_tools(&self, run_id: RunId) -> Result<Vec<(ToolKey, ToolSpec)>, McpError>;

    /// Runs one call with the pipeline of a harness call (`via = Sandbox`). Err only for
    /// session-level failures (`MCP_SESSION_UNKNOWN`); tool-level failures are in `ToolCallResult`.
    async fn dispatch(
        &self,
        run_id: RunId,
        key: ToolKey,
        arguments: serde_json::Value,
        cancel: CancellationToken,
    ) -> Result<ToolCallResult, McpError>;

    /// Every tool in the session's offered map, sorted by server then tool, whatever its
    /// `sandbox_callable`. Err only `MCP_SESSION_UNKNOWN`.
    // spec: ordin-desktop/components/McpFront#SA-MCP-HT-1
    fn harness_tools(&self, run_id: RunId) -> Result<Vec<(ToolKey, ToolSpec)>, McpError>;

    /// Runs one call with the pipeline of a bridge `tools/call` (`via = Harness`, no progress
    /// token). Err only for session-level failures (`MCP_SESSION_UNKNOWN`).
    // spec: ordin-desktop/components/McpFront#SA-MCP-HT-1
    async fn dispatch_harness(
        &self,
        run_id: RunId,
        key: ToolKey,
        arguments: serde_json::Value,
        cancel: CancellationToken,
    ) -> Result<ToolCallResult, McpError>;
}

/// Where a block announced as `block_available` is kept, so `ordin.block` returns it whole.
/// Owned here; implemented by the Ordin Host over `ordin_blocks`; bound by the program with
/// `McpFront::bind_block_store` (McpFront.md amendment 2026-01-24).
// spec: ordin-desktop/components/McpFront#SA-MCP-BLOCK-01
#[async_trait]
pub trait ShownBlockStore: Send + Sync {
    /// Keeps `block` under `block_id` for the workspace. A refusal is the `report` call's failure,
    /// with its code, message and retryable flag unchanged.
    // spec: ordin-desktop/components/McpFront#SA-MCP-BLOCK-03
    async fn keep_shown_block(
        &self,
        workspace_id: WorkspaceId,
        block_id: BlockId,
        block: &Block,
    ) -> Result<(), ToolFailure>;
}
