//! The three sibling services the adapter calls, behind private traits.
//!
//! The production implementations forward to the real MCP Front, Hook Relay and Rules Engine (`new`
//! takes those concrete types, as the Public Interface says). The traits are the test doubles of the
//! spec's *Background*; the integration tests use the real services.

use futures::stream::BoxStream;
use ordin_hook_relay::{
    HookRelay, HookRelayError, HookSessionEnv, HookSessionEvent, HookSessionSpec,
};
use ordin_mcp_front::{McpError, McpFront, SessionGrant, SessionMcpConfig};
use ordin_rules::{AdoptedRun, RulesEngine, RulesError, RunStartDecision, RunStartFacts};
use ordin_types::RunId;

#[async_trait::async_trait]
pub(crate) trait McpPort: Send + Sync {
    async fn open_session(&self, grant: SessionGrant) -> Result<SessionMcpConfig, McpError>;
    async fn close_session(&self, run_id: RunId) -> Result<(), McpError>;
}

#[async_trait::async_trait]
impl McpPort for McpFront {
    async fn open_session(&self, grant: SessionGrant) -> Result<SessionMcpConfig, McpError> {
        McpFront::open_session(self, grant).await
    }
    async fn close_session(&self, run_id: RunId) -> Result<(), McpError> {
        McpFront::close_session(self, run_id).await
    }
}

pub(crate) trait HookPort: Send + Sync {
    fn open_session(&self, spec: HookSessionSpec) -> Result<HookSessionEnv, HookRelayError>;
    fn close_session(&self, run_id: RunId);
    fn session_events(&self, run_id: RunId) -> BoxStream<'static, HookSessionEvent>;
}

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
}

/// The adapter calls `begin_run` (SA-RECON-02), and `adopted_run` when it takes a paused run from
/// another instance (SA-RECON-08).
#[async_trait::async_trait]
pub(crate) trait RulesPort: Send + Sync {
    async fn begin_run(&self, facts: RunStartFacts) -> Result<RunStartDecision, RulesError>;
    async fn adopted_run(&self, run_id: RunId) -> Result<Option<AdoptedRun>, RulesError>;
}

#[async_trait::async_trait]
impl RulesPort for RulesEngine {
    async fn begin_run(&self, facts: RunStartFacts) -> Result<RunStartDecision, RulesError> {
        RulesEngine::begin_run(self, facts).await
    }
    async fn adopted_run(&self, run_id: RunId) -> Result<Option<AdoptedRun>, RulesError> {
        RulesEngine::adopted_run(self, run_id).await
    }
}
