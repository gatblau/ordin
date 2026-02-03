//! Bringing a session up and taking it down (CopilotLocalAdapter.md `start` steps 7–10, `cancel`
//! clean-up): the run-start decision, the MCP Front and Hook Relay registrations, the Copilot session
//! and the first turn.

use std::sync::Arc;
use std::time::Instant;

use futures::stream::BoxStream;
use ordin_hook_relay::{HookSessionEvent, HookSessionSpec};
use ordin_mcp_front::{McpError, SessionGrant};
use ordin_rules::{RunStartFacts, StartOutcome};
use ordin_runtime_adapter::{HarnessKind, RunEndReason, StartInput, ToolCaller};

use crate::adapter::Inner;
use crate::error::Failure;
use crate::gate::{RelayGate, RelayObserver, ServerNames};
use crate::port::{CopilotEvent, SessionOptions};
use crate::run::{cut, lock, run_log, Run, MAX_FINAL_MESSAGE};

/// A session that runs: its two event streams, subscribed before the first turn was sent.
pub(crate) struct Live {
    pub copilot: BoxStream<'static, CopilotEvent>,
    pub hooks: BoxStream<'static, HookSessionEvent>,
    /// `limit_seconds` of the run-start decision, counted from now.
    pub limit: Option<Instant>,
}

/// What the Rules Engine is told about who started the run, as the Claude Local Adapter fills it
/// outside the agent profile: `Ordin` and `Agent` read "person", a crew member reads "crew" with
/// the crew run, `HostedAgent` reads "trigger".
fn started_by(run: &Run) -> (String, String) {
    match run.caller {
        ToolCaller::Ordin | ToolCaller::Agent => {
            ("person".into(), run.agent_instance_id.to_string())
        }
        ToolCaller::CrewMember { crew_run_id } => ("crew".into(), crew_run_id.to_string()),
        ToolCaller::HostedAgent => ("trigger".into(), run.agent_instance_id.to_string()),
    }
}

fn start_facts(run: &Run, input: &StartInput) -> RunStartFacts {
    let (command, text) = match input {
        StartInput::Command { name, arguments } => (Some(name.clone()), arguments.clone()),
        StartInput::FreeText { text } => (None, text.clone()),
    };
    let (started_by, started_by_id) = started_by(run);
    RunStartFacts {
        run_id: run.run_id,
        agent_instance_id: run.agent_instance_id,
        workspace_id: run.workspace_id,
        crew_charter_id: run.setup.crew_charter_id,
        command,
        text,
        scope: run.scope_id.map(|scope| scope.to_string()),
        asker: None,
        started_by,
        started_by_id,
    }
}

/// Step 10: the first turn. `command` is the Rules Engine's replacement. *Assumed here*, conditional
/// on 10 C-01 and C-60: `/<name> <arguments>` is the form the Copilot CLI accepts for a prompt file
/// the vscode or agents-md Installer installed.
pub(crate) fn first_turn(run: &Run, input: &StartInput, command: Option<&str>) -> String {
    let mut text = match input {
        StartInput::Command { name, arguments } => {
            let name = command.unwrap_or(name);
            format!("/{name} {arguments}")
                .trim_end_matches(' ')
                .to_owned()
        }
        StartInput::FreeText { text } => text.clone(),
    };
    for path in &run.setup.attachment_paths {
        text.push_str(&format!("\nAttached: {}", path.display()));
    }
    text
}

/// Step 8: the MCP Front's servers JSON with the package servers merged in; `Err` names a package
/// server whose name an MCP Front server already has.
pub(crate) fn merged_servers(
    front: serde_json::Value,
    packages: &[crate::config::PackageMcpServer],
) -> Result<serde_json::Value, String> {
    let mut servers = match front {
        serde_json::Value::Object(mut json) => match json.remove("mcpServers") {
            Some(serde_json::Value::Object(servers)) => servers,
            _ => serde_json::Map::new(),
        },
        _ => serde_json::Map::new(),
    };
    for package in packages {
        if servers.contains_key(&package.name) {
            return Err(package.name.clone());
        }
        servers.insert(
            package.name.clone(),
            serde_json::json!({
                "type": "stdio",
                "command": package.command.to_string_lossy(),
                "args": package.args,
                "env": package.env,
            }),
        );
    }
    Ok(serde_json::json!({ "mcpServers": servers }))
}

impl Inner {
    /// Steps 7–10. `Ok(None)`: the run ended on a run event, with no session.
    pub(crate) async fn open_session(
        &self,
        run: &Arc<Run>,
        input: &StartInput,
    ) -> Result<Option<Live>, Failure> {
        run.emit_started(run.setup.resume_session_id.clone());

        // Step 7.
        let decision = match self.rules.begin_run(start_facts(run, input)).await {
            Ok(decision) => decision,
            Err(error) => {
                // spec: ordin-desktop/components/CopilotLocalAdapter#CPA_RULES_UNAVAILABLE
                run_log!(
                    warn,
                    run,
                    "copilot.run.ended",
                    reason = "failed",
                    code = "CPA_RULES_UNAVAILABLE",
                    rule_code = error.code()
                );
                run.emit_end(RunEndReason::Failed, None);
                self.close_session(run).await;
                return Ok(None);
            }
        };
        let ended_with = match &decision.outcome {
            StartOutcome::Proceed => None,
            StartOutcome::Ended { ending, .. } => Some(ending),
            StartOutcome::Replied { text, .. } => Some(text),
        };
        if let Some(text) = ended_with {
            // spec: ordin-desktop/components/CopilotLocalAdapter#CPA_RUN_START_ENDED
            run_log!(
                info,
                run,
                "copilot.run.ended",
                reason = "completed",
                code = "CPA_RUN_START_ENDED"
            );
            run.emit_end(RunEndReason::Completed, Some(cut(text, MAX_FINAL_MESSAGE)));
            self.close_session(run).await;
            return Ok(None);
        }
        // `tool_subset` and `single_call` are applied by the Rules Engine's decision on each tool
        // request, which reaches it through the gates below; `limit_seconds` ends the run here.
        let limit = decision
            .limit_seconds
            .map(|seconds| Instant::now() + std::time::Duration::from_secs(u64::from(seconds)));

        // Step 8.
        // spec: ordin-desktop/components/CopilotLocalAdapter#SA-RECON-03
        let mcp_config = self
            .mcp
            .open_session(SessionGrant {
                run_id: run.run_id,
                workspace_id: run.workspace_id,
                agent_instance_id: run.agent_instance_id,
                harness: HarnessKind::CopilotCli,
                profile: self.config.profile,
                caller: run.caller,
                scope_id: run.scope_id,
                coding_task_id: run.coding_task_id,
                // A name that is not a server name is dropped, so the limit only narrows.
                // spec: ordin-desktop/components/McpFront#SA-MCP-SRV-01
                only_servers: run.only_servers.as_ref().map(|names| {
                    names
                        .iter()
                        .filter_map(|n| ordin_mcp_front::ServerName::new(n.as_str()).ok())
                        .collect()
                }),
            })
            .await
            .map_err(|error| {
                let permanent = matches!(error, McpError::SessionExists { .. });
                run_log!(
                    warn,
                    run,
                    "copilot.session.open_failed",
                    code = "CPA_MCP_OPEN_FAILED",
                    mcp_code = error.code()
                );
                Failure::unreachable("CPA_MCP_OPEN_FAILED", !permanent)
            })?;
        lock(&run.registered).mcp = true;
        // `open_session` keeps the relay's per-session state; its environment is not passed to the
        // CLI, because no hook process runs (X3b).
        self.hooks
            .open_session(HookSessionSpec {
                run_id: run.run_id,
                agent_instance_id: run.agent_instance_id,
                workspace_id: run.workspace_id,
                harness: HarnessKind::CopilotCli,
            })
            .map_err(|error| {
                run_log!(
                    warn,
                    run,
                    "copilot.session.open_failed",
                    code = "CPA_HOOK_OPEN_FAILED",
                    hook_code = error.code()
                );
                Failure::unreachable("CPA_HOOK_OPEN_FAILED", true)
            })?;
        lock(&run.registered).hook = true;
        let hook_events = self.hooks.session_events(run.run_id);
        let names = ServerNames {
            front: mcp_config
                .servers
                .iter()
                .map(|launch| launch.name.as_str().to_owned())
                .collect(),
            package: lock(&run.package_servers)
                .iter()
                .map(|p| p.name.clone())
                .collect(),
        };
        *lock(&run.front_servers) = names.front.clone();
        let mcp_servers = merged_servers(
            mcp_config.to_mcp_servers_json(),
            &lock(&run.package_servers),
        )
        .map_err(|name| {
            run_log!(
                warn,
                run,
                "copilot.session.open_failed",
                code = "CPA_SERVER_NAME_CLASH",
                server = name.as_str()
            );
            Failure::unreachable("CPA_SERVER_NAME_CLASH", false)
        })?;

        // Step 9.
        let gate = Arc::new(RelayGate {
            run_id: run.run_id,
            hooks: self.hooks.clone(),
            names,
            bound: self.host.timings.gate_bound,
        });
        let options = SessionOptions {
            model: run.setup.model.clone(),
            mcp_servers,
            pre_tool_use: gate.clone(),
            permission: gate,
            post_tool_use: Arc::new(RelayObserver {
                run_id: run.run_id,
                hooks: self.hooks.clone(),
            }),
        };
        let server = run
            .server()
            .ok_or_else(|| Failure::unreachable("CPA_SERVER_START_FAILED", true))?;
        let session_id = match &run.setup.resume_session_id {
            Some(id) => {
                if server.resume_session(id, options).await.is_err() {
                    // Conditional on 10 C-05 (03 Recovery after restart).
                    // spec: ordin-desktop/components/CopilotLocalAdapter#CPA_SESSION_NOT_RESUMABLE
                    run_log!(
                        warn,
                        run,
                        "copilot.run.ended",
                        reason = "interrupted",
                        code = "CPA_SESSION_NOT_RESUMABLE"
                    );
                    run.emit_end(RunEndReason::Interrupted, None);
                    self.close_session(run).await;
                    return Ok(None);
                }
                id.clone()
            }
            None => server.create_session(options).await.map_err(|_| {
                run_log!(
                    warn,
                    run,
                    "copilot.session.open_failed",
                    code = "CPA_SESSION_CREATE_FAILED"
                );
                Failure::unreachable("CPA_SESSION_CREATE_FAILED", true)
            })?,
        };
        run.set_session_id(session_id.clone());
        run_log!(info, run, "copilot.session.created");
        let copilot_events = server.events(&session_id);

        // Step 10.
        let turn = first_turn(run, input, decision.command.as_deref());
        server.send(&session_id, &turn).await.map_err(|_| {
            run_log!(
                warn,
                run,
                "copilot.session.open_failed",
                code = "CPA_SEND_FAILED"
            );
            Failure::unreachable("CPA_SEND_FAILED", true)
        })?;
        Ok(Some(Live {
            copilot: copilot_events,
            hooks: hook_events,
            limit,
        }))
    }

    /// Clean-up: the relay and MCP sessions (errors logged), the CLI server, the package servers'
    /// secrets.
    pub(crate) async fn close_session(&self, run: &Run) {
        let (mcp, hook) = {
            let mut registered = lock(&run.registered);
            let flags = (registered.mcp, registered.hook);
            registered.mcp = false;
            registered.hook = false;
            flags
        };
        if hook {
            self.hooks.close_session(run.run_id);
        }
        if mcp {
            if let Err(error) = self.mcp.close_session(run.run_id).await {
                run_log!(
                    warn,
                    run,
                    "copilot.session.close_failed",
                    mcp_code = error.code()
                );
            }
        }
        let server = lock(&run.server).take();
        if let Some(server) = server {
            self.stop_server(run, server.as_ref()).await;
        }
        lock(&run.package_servers).clear();
    }

    /// `stop(stop_deadline)`: the port kills the server after the deadline; reaching it is logged.
    pub(crate) async fn stop_server(&self, run: &Run, server: &dyn crate::port::CopilotServer) {
        let deadline = self.config.stop_deadline;
        let began = Instant::now();
        server.stop(deadline).await;
        if began.elapsed() >= deadline {
            // spec: ordin-desktop/components/CopilotLocalAdapter#CPA_STOP_DEADLINE
            run_log!(
                warn,
                run,
                "copilot.stop.deadline",
                code = "CPA_STOP_DEADLINE"
            );
        }
    }
}
