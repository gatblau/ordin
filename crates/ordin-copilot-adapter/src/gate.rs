//! Tool requests: `RelayGate` and the post-tool-use observer (CopilotLocalAdapter.md *Tool requests —
//! `RelayGate::decide`*; 09 ADR-045).

use std::sync::Arc;
use std::time::Duration;

use futures::FutureExt;
use ordin_runtime_adapter::HarnessKind;
use ordin_types::wire::hook::{HookMessage, HookReply};
use ordin_types::wire::rules::ToolOrigin;
use ordin_types::RunId;

use crate::port::{GateAnswer, ToolGate, ToolObserver};
use crate::ports::HookPort;

/// The answer when the decision call errors, panics or exceeds its bound (fail closed).
pub(crate) const COULD_NOT_CHECK: &str = "ordin could not check this action.";

/// The names a session's tools are classified by.
#[derive(Debug, Clone, Default)]
pub(crate) struct ServerNames {
    pub front: Vec<String>,
    pub package: Vec<String>,
}

/// Step 1: a tool of a server registered for this session — `mcp__<server>__<tool>` or the CLI's
/// server-qualified `<server>-<tool>` — is that server's; anything else is built in. The MCP Front
/// classification is the core's, not the harness's (2C).
pub(crate) fn classify(tool: &str, names: &ServerNames) -> ToolOrigin {
    let server_of = |server: &str| {
        tool.strip_prefix("mcp__")
            .and_then(|rest| rest.strip_prefix(server))
            .is_some_and(|rest| rest.starts_with("__"))
            || tool
                .strip_prefix(server)
                .is_some_and(|rest| rest.starts_with('-'))
    };
    if let Some(server) = names.front.iter().find(|s| server_of(s)) {
        return ToolOrigin::OrdinMcpFront {
            server: server.clone(),
        };
    }
    if let Some(server) = names.package.iter().find(|s| server_of(s)) {
        return ToolOrigin::PackageMcpServer {
            server: server.clone(),
        };
    }
    ToolOrigin::BuiltIn
}

fn tool_kind(origin: &ToolOrigin) -> &'static str {
    match origin {
        ToolOrigin::BuiltIn => "built_in",
        ToolOrigin::OrdinMcpFront { .. } => "ordin",
        ToolOrigin::PackageMcpServer { .. } => "package",
    }
}

/// Both gates of a session: the pre-tool-use hook and the permission handler (*chosen here*).
pub(crate) struct RelayGate {
    pub run_id: RunId,
    pub hooks: Arc<dyn HookPort>,
    pub names: ServerNames,
    /// 55 s (*chosen here: above the relay's 50 s deadline*).
    pub bound: Duration,
}

#[async_trait::async_trait]
impl ToolGate for RelayGate {
    // spec: ordin-desktop/components/CopilotLocalAdapter#CPA_GATE_FAILED
    async fn decide(
        &self,
        tool: &str,
        arguments: serde_json::Value,
        tool_use_id: Option<String>,
    ) -> GateAnswer {
        let origin = classify(tool, &self.names);
        let kind = tool_kind(&origin);
        let message = HookMessage::PreToolUse {
            harness: HarnessKind::CopilotCli,
            tool: tool.to_owned(),
            origin,
            arguments,
            harness_tool_use_id: tool_use_id,
            subagent: None,
        };
        let call = std::panic::AssertUnwindSafe(self.hooks.decide_in_process(self.run_id, message))
            .catch_unwind();
        let answer = match tokio::time::timeout(self.bound, call).await {
            Ok(Ok(HookReply::Allow { .. })) => GateAnswer::Allow,
            Ok(Ok(HookReply::Deny { reason, .. })) => GateAnswer::Deny { reason },
            // An in-process call returns no `Holding`, and `Ack` answers no tool request: neither
            // is an allow.
            Ok(Ok(HookReply::Holding { .. } | HookReply::Ack { .. })) | Ok(Err(_)) | Err(_) => {
                tracing::warn!(
                    target: "ordin_copilot_adapter",
                    run_id = %self.run_id,
                    code = "CPA_GATE_FAILED",
                    tool_kind = kind,
                    "copilot.gate.failed"
                );
                GateAnswer::Deny {
                    reason: COULD_NOT_CHECK.to_owned(),
                }
            }
        };
        tracing::info!(
            target: "ordin_copilot_adapter",
            run_id = %self.run_id,
            tool_kind = kind,
            allowed = matches!(answer, GateAnswer::Allow),
            "copilot.gate.decided"
        );
        answer
    }
}

/// Forwards `PostToolUse` to the relay, which journals it.
pub(crate) struct RelayObserver {
    pub run_id: RunId,
    pub hooks: Arc<dyn HookPort>,
}

#[async_trait::async_trait]
impl ToolObserver for RelayObserver {
    async fn tool_used(&self, tool: &str, tool_use_id: Option<String>, succeeded: bool) {
        let message = HookMessage::PostToolUse {
            harness: HarnessKind::CopilotCli,
            tool: tool.to_owned(),
            harness_tool_use_id: tool_use_id,
            succeeded,
        };
        let _ = self.hooks.decide_in_process(self.run_id, message).await;
    }
}
