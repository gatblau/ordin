//! The three sibling services the relay calls, behind private traits.
//!
//! The production implementations forward to the real Rules Engine, Approval Service and Session
//! Supervisor (the constructor takes those concrete types, as the spec's Public Interface says).
//! The traits exist so that this crate's own tests can script a slow engine, an *ask*, a refused
//! ending or a failing Approval Service deterministically; the integration tests use the real
//! services.

use std::sync::Arc;

use ordin_approval::{ApprovalError, ApprovalService, WaitOutcome};
use ordin_rules::{ApprovalWait, CallContext, EndDecision, RulesEngine, RulesError, RulesOutcome};
use ordin_session_supervisor::{ResumeDecision, SessionSupervisor, SupervisorError};
use ordin_types::wire::rules::{ToolCall, ToolOrigin};
use ordin_types::{ApprovalId, RunId, ToolRequestId};
use tokio::time::Instant;

use crate::writes::AgentWrites;

/// The Rules Engine calls of D and E.
#[async_trait::async_trait]
pub(crate) trait RulesPort: Send + Sync {
    async fn decide_tool_call(&self, call: &ToolCall, ctx: &CallContext) -> RulesOutcome;
    /// The call's origin as the Rules Engine classifies it (a server the MCP Front serves is ordin's).
    fn classify_origin(&self, origin: &ToolOrigin) -> ToolOrigin;
    async fn record_outcome(&self, tool_request_id: ToolRequestId, executed: bool);
    async fn decide_run_end(&self, run_id: RunId) -> EndDecision;
    async fn settle_end_gates(&self, run_id: RunId, approved: bool) -> EndDecision;
    async fn wait_for_approval(
        &self,
        approval_id: ApprovalId,
        until: Option<Instant>,
    ) -> Result<ApprovalWait, RulesError>;
}

#[async_trait::async_trait]
impl RulesPort for RulesEngine {
    async fn decide_tool_call(&self, call: &ToolCall, ctx: &CallContext) -> RulesOutcome {
        RulesEngine::decide_tool_call(self, call, ctx).await
    }
    fn classify_origin(&self, origin: &ToolOrigin) -> ToolOrigin {
        RulesEngine::classify_origin(self, origin)
    }
    async fn record_outcome(&self, tool_request_id: ToolRequestId, executed: bool) {
        RulesEngine::record_outcome(self, tool_request_id, executed).await
    }
    async fn decide_run_end(&self, run_id: RunId) -> EndDecision {
        RulesEngine::decide_run_end(self, run_id).await
    }
    async fn settle_end_gates(&self, run_id: RunId, approved: bool) -> EndDecision {
        RulesEngine::settle_end_gates(self, run_id, approved).await
    }
    async fn wait_for_approval(
        &self,
        approval_id: ApprovalId,
        until: Option<Instant>,
    ) -> Result<ApprovalWait, RulesError> {
        RulesEngine::wait_for_approval(self, approval_id, until).await
    }
}

/// The Approval Service call of E.2 and G (SA-RECON-03).
#[async_trait::async_trait]
pub(crate) trait ApprovalPort: Send + Sync {
    async fn wait(
        &self,
        approval_id: ApprovalId,
        deadline: Option<Instant>,
    ) -> Result<WaitOutcome, ApprovalError>;
}

#[async_trait::async_trait]
impl ApprovalPort for ApprovalService {
    async fn wait(
        &self,
        approval_id: ApprovalId,
        deadline: Option<Instant>,
    ) -> Result<WaitOutcome, ApprovalError> {
        ApprovalService::wait(self, approval_id, deadline).await
    }
}

/// The Session Supervisor calls of C.5, F.2 and G.
#[async_trait::async_trait]
pub(crate) trait SupervisorPort: Send + Sync {
    fn is_live(&self, run_id: RunId) -> bool;
    async fn resume_with_decision(
        &self,
        run_id: RunId,
        decision: ResumeDecision,
    ) -> Result<(), SupervisorError>;
}

#[async_trait::async_trait]
impl SupervisorPort for SessionSupervisor {
    fn is_live(&self, run_id: RunId) -> bool {
        SessionSupervisor::is_live(self, run_id)
    }
    async fn resume_with_decision(
        &self,
        run_id: RunId,
        decision: ResumeDecision,
    ) -> Result<(), SupervisorError> {
        SessionSupervisor::resume_with_decision(self, run_id, decision).await
    }
}

/// The four ports together, as the relay holds them.
#[derive(Clone)]
pub(crate) struct Ports {
    pub rules: Arc<dyn RulesPort>,
    pub approvals: Arc<dyn ApprovalPort>,
    pub supervisor: Arc<dyn SupervisorPort>,
    pub writes: Arc<dyn AgentWrites>,
}
