//! Public value types of the crate root: sessions, launch lists, configuration and results.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use ordin_types::domain::identity::HarnessKind;
pub use ordin_types::session::ToolCaller;
use ordin_types::wire::rules::ToolOrigin;
use ordin_types::wire::startup::StartUpReportItem;
use ordin_types::{AgentInstanceId, CodingTaskId, RunId, ScopeId, WorkspaceId};
use serde::{Deserialize, Serialize};

use crate::names::ServerName;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramProfile {
    Desktop,
    PersonalServer,
    AgentProfile,
}

/// Who the session is. Built by the adapter that opens the session from `StartRun`.
// spec: ordin-desktop/components/McpFront#SA-RECON-01
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionGrant {
    pub run_id: RunId,
    /// In the agent profile: the single workspace identifier the Agent Loader assigns.
    pub workspace_id: WorkspaceId,
    pub agent_instance_id: AgentInstanceId,
    pub harness: HarnessKind,
    pub profile: ProgramProfile,
    /// Whose session it is (SA-RECON-01).
    pub caller: ToolCaller,
    /// Agent profile only: the scope of the conversation the run serves.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<ScopeId>,
    /// Set for a session that works on a coding task (servers `code`, `checks`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coding_task_id: Option<CodingTaskId>,
    /// The only servers whose tools the session is offered; `None` offers every server.
    // spec: ordin-desktop/components/McpFront#SA-MCP-SRV-01
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub only_servers: Option<Vec<ServerName>>,
}

/// What the adapter registers with the harness for this session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionMcpConfig {
    pub servers: Vec<McpServerLaunch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct McpServerLaunch {
    pub name: ServerName,
    /// Absolute path of the ordin hook program (`ordin-hook`, `ordin-hook.exe`).
    pub command: PathBuf,
    /// Always `["mcp-bridge", "<server>", "--agent-instance", "<agent_instance_id>"]`.
    pub args: Vec<String>,
    /// `ORDIN_MCP_ENDPOINT`, `ORDIN_SESSION_ID`, `ORDIN_MCP_SERVER`, `ORDIN_MCP_TOKEN`.
    pub env: BTreeMap<String, String>,
}

impl SessionMcpConfig {
    /// `{"mcpServers": {"<name>": {"type": "stdio", "command": "…", "args": […], "env": {…}}}}`.
    pub fn to_mcp_servers_json(&self) -> serde_json::Value {
        let mut servers = serde_json::Map::new();
        for launch in &self.servers {
            servers.insert(
                launch.name.as_str().to_owned(),
                serde_json::json!({
                    "type": "stdio",
                    "command": launch.command.to_string_lossy(),
                    "args": launch.args,
                    "env": launch.env,
                }),
            );
        }
        serde_json::json!({ "mcpServers": servers })
    }
}

/// One offered tool, for the start-up report.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OfferedTool {
    pub server: ServerName,
    pub tool: String,
    /// The provider's `ToolSpec::plain`.
    pub plain: String,
}

impl OfferedTool {
    /// `StartUpReportItem::AvailableTool { tool: "mcp__<server>__<tool>", origin: OrdinMcpFront { server }, plain }`.
    pub fn to_start_up_item(&self) -> StartUpReportItem {
        StartUpReportItem::AvailableTool {
            tool: format!("mcp__{}__{}", self.server.as_str(), self.tool),
            origin: ToolOrigin::OrdinMcpFront {
                server: self.server.as_str().to_owned(),
            },
            plain: self.plain.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct McpFrontConfig {
    /// The application data root; the socket is `<root>/run/mcp.sock`.
    pub app_data_root: PathBuf,
    /// Windows only: used in `\\.\pipe\ordin-mcp-<installation id>`.
    pub installation_id: uuid::Uuid,
    /// Absolute path of `ordin-hook` / `ordin-hook.exe`, shipped beside the program.
    pub hook_program: PathBuf,
    pub profile: ProgramProfile,
    /// Default 300 s.
    pub call_timeout: Duration,
    /// Default 50 s: how long a call waits for a person's approval.
    pub approval_wait: Duration,
    /// Default 16,000 characters: above it, and with only-needed-parts on, output is trimmed.
    pub trim_above_chars: usize,
    /// Default 12,000 characters kept in the answer when trimming.
    pub trim_keep_chars: usize,
    /// Default 4 MiB: the most text one call returns, trimming or not.
    pub max_output_bytes: usize,
    /// Default 16 MiB: trimmed remainders kept per session, oldest dropped first.
    pub max_rest_bytes_per_session: usize,
    /// Default 64: concurrent `tools/call` per session.
    pub max_concurrent_calls_per_session: usize,
}

impl Default for McpFrontConfig {
    /// The defaults above; `app_data_root`, `installation_id` and `hook_program` have no default
    /// and are set by the program.
    fn default() -> Self {
        McpFrontConfig {
            app_data_root: PathBuf::new(),
            installation_id: uuid::Uuid::nil(),
            hook_program: PathBuf::new(),
            profile: ProgramProfile::Desktop,
            call_timeout: Duration::from_secs(300),
            approval_wait: Duration::from_secs(50),
            trim_above_chars: 16_000,
            trim_keep_chars: 12_000,
            max_output_bytes: 4 * 1024 * 1024,
            max_rest_bytes_per_session: 16 * 1024 * 1024,
            max_concurrent_calls_per_session: 64,
        }
    }
}

/// Agent profile only: where call counts go. Implemented by the server program.
pub trait McpMetrics: Send + Sync {
    fn call_finished(&self, server: &ServerName, outcome: CallOutcome, seconds: f64);
}

/// Telemetry's `McpCallOutcome` values: `answered`, `refused`, `failed`, `cancelled`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallOutcome {
    Answered,
    Refused,
    Failed,
    Cancelled,
}

/// The outcome of one tool call, as returned to the harness or to the sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolCallResult {
    pub is_error: bool,
    /// Text for the model.
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured: Option<serde_json::Value>,
    /// Stable code when `is_error`; never shown to the person.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl ToolCallResult {
    pub(crate) fn failure(code: &str, text: impl Into<String>) -> Self {
        ToolCallResult {
            is_error: true,
            text: text.into(),
            structured: None,
            code: Some(code.to_owned()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_spec() {
        let c = McpFrontConfig::default();
        assert_eq!(c.call_timeout, Duration::from_secs(300));
        assert_eq!(c.approval_wait, Duration::from_secs(50));
        assert_eq!(c.trim_above_chars, 16_000);
        assert_eq!(c.trim_keep_chars, 12_000);
        assert_eq!(c.max_output_bytes, 4 * 1024 * 1024);
        assert_eq!(c.max_rest_bytes_per_session, 16 * 1024 * 1024);
        assert_eq!(c.max_concurrent_calls_per_session, 64);
        assert_eq!(c.profile, ProgramProfile::Desktop);
    }

    #[test]
    fn launch_json_and_start_up_item() {
        let server = ServerName::new("memory").unwrap();
        let config = SessionMcpConfig {
            servers: vec![McpServerLaunch {
                name: server.clone(),
                command: PathBuf::from("/bin/ordin-hook"),
                args: vec!["mcp-bridge".into(), "memory".into()],
                env: BTreeMap::from([("A".to_owned(), "b".to_owned())]),
            }],
        };
        let json = config.to_mcp_servers_json();
        assert_eq!(json["mcpServers"]["memory"]["type"], "stdio");
        assert_eq!(json["mcpServers"]["memory"]["command"], "/bin/ordin-hook");
        assert_eq!(json["mcpServers"]["memory"]["env"]["A"], "b");
        let item = OfferedTool {
            server,
            tool: "recall".into(),
            plain: "Remembers.".into(),
        }
        .to_start_up_item();
        match item {
            StartUpReportItem::AvailableTool {
                tool,
                origin,
                plain,
            } => {
                assert_eq!(tool, "mcp__memory__recall");
                assert_eq!(plain, "Remembers.");
                assert!(
                    matches!(origin, ToolOrigin::OrdinMcpFront { server } if server == "memory")
                );
            }
            other => panic!("unexpected item {other:?}"),
        }
    }
}
