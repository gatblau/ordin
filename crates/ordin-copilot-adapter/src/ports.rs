//! The sibling services the adapter calls, behind private traits.
//!
//! The production implementations forward to the real MCP Front, Hook Relay, Rules Engine and
//! Identity (`new` takes those concrete types, as the Public Interface says). The traits are the
//! test doubles of the spec's *Background*; the integration test uses the real services.

use futures::stream::BoxStream;
use ordin_hook_relay::{
    HookRelay, HookRelayError, HookSessionEnv, HookSessionEvent, HookSessionSpec,
};
use ordin_identity::{CredError, IdentityService};
use ordin_mcp_front::{
    McpError, McpFront, SessionGrant, SessionMcpConfig, ToolCallResult, ToolDispatch, ToolKey,
    ToolSpec,
};
use ordin_rules::{RulesEngine, RulesError, RunStartDecision, RunStartFacts};
use ordin_types::domain::identity::CredentialKind;
use ordin_types::wire::hook::{HookMessage, HookReply};
use ordin_types::RunId;
use tokio_util::sync::CancellationToken;

#[async_trait::async_trait]
pub(crate) trait McpPort: Send + Sync {
    async fn open_session(&self, grant: SessionGrant) -> Result<SessionMcpConfig, McpError>;
    async fn close_session(&self, run_id: RunId) -> Result<(), McpError>;
    /// Every tool the session is offered (McpFront.md SA-MCP-HT-1).
    // spec: ordin-desktop/components/CopilotLocalAdapter#CPA-HT-2
    fn harness_tools(&self, run_id: RunId) -> Result<Vec<(ToolKey, ToolSpec)>, McpError>;
    /// One declared-tool call through the MCP Front's pipeline (McpFront.md SA-MCP-HT-1).
    // spec: ordin-desktop/components/CopilotLocalAdapter#CPA-HT-3
    async fn dispatch_harness(
        &self,
        run_id: RunId,
        key: ToolKey,
        arguments: serde_json::Value,
        cancel: CancellationToken,
    ) -> Result<ToolCallResult, McpError>;
}

#[async_trait::async_trait]
impl McpPort for McpFront {
    async fn open_session(&self, grant: SessionGrant) -> Result<SessionMcpConfig, McpError> {
        McpFront::open_session(self, grant).await
    }
    async fn close_session(&self, run_id: RunId) -> Result<(), McpError> {
        McpFront::close_session(self, run_id).await
    }
    fn harness_tools(&self, run_id: RunId) -> Result<Vec<(ToolKey, ToolSpec)>, McpError> {
        ToolDispatch::harness_tools(self, run_id)
    }
    async fn dispatch_harness(
        &self,
        run_id: RunId,
        key: ToolKey,
        arguments: serde_json::Value,
        cancel: CancellationToken,
    ) -> Result<ToolCallResult, McpError> {
        ToolDispatch::dispatch_harness(self, run_id, key, arguments, cancel).await
    }
}

#[async_trait::async_trait]
pub(crate) trait HookPort: Send + Sync {
    fn open_session(&self, spec: HookSessionSpec) -> Result<HookSessionEnv, HookRelayError>;
    fn close_session(&self, run_id: RunId);
    fn session_events(&self, run_id: RunId) -> BoxStream<'static, HookSessionEvent>;
    /// Copilot route: the same decision path as a hook call, in process (09 ADR-045).
    async fn decide_in_process(&self, run_id: RunId, message: HookMessage) -> HookReply;
}

#[async_trait::async_trait]
impl HookPort for HookRelay {
    fn open_session(&self, spec: HookSessionSpec) -> Result<HookSessionEnv, HookRelayError> {
        HookRelay::open_session(self, spec)
    }
    fn close_session(&self, run_id: RunId) {
        HookRelay::close_session(self, run_id)
    }
    fn session_events(&self, run_id: RunId) -> BoxStream<'static, HookSessionEvent> {
        HookRelay::session_events(self, run_id)
    }
    async fn decide_in_process(&self, run_id: RunId, message: HookMessage) -> HookReply {
        HookRelay::decide_in_process(self, run_id, message).await
    }
}

/// The adapter calls only `begin_run` (R5).
#[async_trait::async_trait]
pub(crate) trait RulesPort: Send + Sync {
    async fn begin_run(&self, facts: RunStartFacts) -> Result<RunStartDecision, RulesError>;
}

#[async_trait::async_trait]
impl RulesPort for RulesEngine {
    async fn begin_run(&self, facts: RunStartFacts) -> Result<RunStartDecision, RulesError> {
        RulesEngine::begin_run(self, facts).await
    }
}

/// The silent GitHub refresh of `start` step 6.1 (03 golden path).
#[async_trait::async_trait]
pub(crate) trait IdentityPort: Send + Sync {
    async fn refresh_silently(&self, kind: CredentialKind) -> Result<bool, CredError>;
}

#[async_trait::async_trait]
impl IdentityPort for IdentityService {
    async fn refresh_silently(&self, kind: CredentialKind) -> Result<bool, CredError> {
        IdentityService::refresh_silently(self, kind).await
    }
}
