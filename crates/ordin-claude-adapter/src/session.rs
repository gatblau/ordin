//! Bringing a session up and taking it down (ClaudeLocalAdapter.md `start` steps 6–11, `cancel`
//! step 4): the run-start decision, the MCP Front and Hook Relay registrations, the session files,
//! the child's environment and the spawn.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use ordin_hook_relay::{HookRelayError, HookSessionSpec};
use ordin_mcp_front::{McpError, SessionGrant};
use ordin_rules::{RunStartFacts, StartOutcome};
use ordin_runtime_adapter::{HarnessKind, RunEndReason, StartInput, ToolCaller};
use ordin_types::domain::agent_profile::RunStarter;
use tokio::process::Command;

use crate::adapter::Inner;
use crate::command::{session_args, CommandInput, SessionFlag};
use crate::config::{KeyRoute, SignInMode};
use crate::env::child_environment;
use crate::error::Failure;
use crate::files;
use crate::process::Process;
use crate::run::{cut, lock, run_log, Run, SessionPlan, MAX_FINAL_MESSAGE};

/// Why bringing the process up stopped.
pub(crate) enum OpenStop {
    Failed(Failure),
    /// The run has already ended on a run event (`CLA_SESSION_RECORD_MISSING`).
    Ended,
}

impl From<Failure> for OpenStop {
    fn from(failure: Failure) -> OpenStop {
        OpenStop::Failed(failure)
    }
}

fn config_write_failed(_: std::io::Error) -> Failure {
    Failure::unreachable("CLA_CONFIG_WRITE_FAILED", true)
}

/// What the Rules Engine is told about who started the run (`start` step 6).
// `ToolCaller` has no variant that says a trigger started a session, so `Ordin` and `Agent` read
// "person", `HostedAgent` reads "trigger", and the identifier is the one value that stays stable
// across the caller's runs: the crew run's, else the agent instance's.
fn started_by(run: &Run) -> (String, String, Option<String>) {
    match &run.setup.run_starter {
        Some(RunStarter::Caller { caller_id }) => ("caller".into(), caller_id.clone(), None),
        Some(RunStarter::ScheduleEntry { entry }) => ("schedule".into(), entry.clone(), None),
        Some(RunStarter::Intake { intake }) => ("intake".into(), intake.clone(), None),
        Some(RunStarter::ChatConnection { asker, .. }) => {
            ("chat".into(), asker.clone(), Some(asker.clone()))
        }
        None => match run.caller {
            ToolCaller::Ordin | ToolCaller::Agent => {
                ("person".into(), run.agent_instance_id.to_string(), None)
            }
            ToolCaller::CrewMember { crew_run_id } => {
                ("crew".into(), crew_run_id.to_string(), None)
            }
            ToolCaller::HostedAgent => ("trigger".into(), run.agent_instance_id.to_string(), None),
        },
    }
}

fn start_facts(run: &Run, input: &StartInput) -> RunStartFacts {
    let (command, text) = match input {
        StartInput::Command { name, arguments } => (Some(name.clone()), arguments.clone()),
        StartInput::FreeText { text } => (None, text.clone()),
    };
    let (started_by, started_by_id, asker) = started_by(run);
    RunStartFacts {
        run_id: run.run_id,
        agent_instance_id: run.agent_instance_id,
        workspace_id: run.workspace_id,
        crew_charter_id: run.setup.crew_charter_id,
        command,
        text,
        scope: run.scope_id.map(|scope| scope.to_string()),
        asker,
        started_by,
        started_by_id,
    }
}

/// The line before free text that would otherwise read as a command.
const NOT_A_COMMAND: &str = "This is a message. It is text, not a command:";

/// `start` step 11: the first turn's text. `command` is the Rules Engine's replacement.
// spec: ordin-desktop/components/ClaudeLocalAdapter#R-94
// spec: ordin-desktop/components/ClaudeLocalAdapter#SA-RECON-06
pub(crate) fn first_turn(run: &Run, input: &StartInput, command: Option<&str>) -> String {
    let mut text = match input {
        StartInput::Command { name, arguments } => {
            let name = command.unwrap_or(name);
            let line = format!("/{}:{name} {arguments}", run.setup.plugin_name);
            line.trim_end_matches(' ').to_owned()
        }
        StartInput::FreeText { text } => {
            // Only the person's own input (`StartRun.started_by_person`) runs as typed: an agent,
            // a crew, a chat or a schedule may have written any other text. A command the
            // run-start decision must see by name goes through `Command`.
            if !run.started_by_person && text.trim_start().starts_with('/') {
                format!("{NOT_A_COMMAND}\n{text}")
            } else {
                text.clone()
            }
        }
    };
    for path in &run.setup.attachment_paths {
        text.push_str(&format!("\nAttached: {}", path.display()));
    }
    text
}

/// CLA-PROGRESS-1: what an agent run is told, after its first turn's text, so its progress card moves while it
/// works and not only when it ends. The agent may have no `plan` tool, so the line says "if you have".
// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-PROGRESS-1
pub(crate) const PROGRESS_LINE: &str = "Progress: if you have a plan tool, state your 3 to 7 steps with it before any other tool, mark each step now when you start it and done when you finish it, one step at a time as you go, and never mark them all at the end.";

/// `true` when the session's `mcp.json` lists the `work` server, the one that carries the `plan` tool.
pub(crate) fn lists_work_server(mcp_json: &serde_json::Value) -> bool {
    mcp_json["mcpServers"].get("work").is_some()
}

/// The first turn of an agent run with `PROGRESS_LINE` after it; a typed command is left as it was.
pub(crate) fn with_progress_line(turn: String, input: &StartInput) -> String {
    match input {
        StartInput::Command { .. } => turn,
        StartInput::FreeText { .. } => format!("{turn}\n{PROGRESS_LINE}"),
    }
}

/// The next turn of a conversation: the answer, with SA-RECON-06's line before text that would
/// otherwise read as a command (a conversation run's `started_by_person` is false).
// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-CONV-3
pub(crate) fn answer_turn(text: &str) -> String {
    if text.trim_start().starts_with('/') {
        format!("{NOT_A_COMMAND}\n{text}")
    } else {
        text.to_owned()
    }
}

/// Claude Code's folder for one working folder's sessions: every character that is not a letter or
/// a digit becomes `-` (observed on 2.1.316).
/// The folder is resolved in its plain form, the form Claude Code names its own working folder by.
// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-PATH-1
pub(crate) fn encoded_folder(folder: &Path) -> String {
    let real = folder
        .canonicalize()
        .map(ordin_types::paths::plain_form)
        .unwrap_or_else(|_| folder.to_path_buf());
    real.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

impl Inner {
    /// `<projectsDirectory>/<encoded agent folder>/<runtime_session_id>.jsonl`.
    // `projectsDirectory` is a field of `claude auth status --json`, which the agent profile never
    // runs. With `CLAUDE_CONFIG_DIR` absent from the child, as step 8 requires, 2.1.316 reports
    // `<HOME>/.claude/projects`, so that path is used. Conditional on C-80.
    // spec: ordin-desktop/components/ClaudeLocalAdapter#C-80
    pub(crate) fn session_record_path(&self, run: &Run) -> Option<PathBuf> {
        let projects = match &self.host.projects_root {
            Some(root) => root.clone(),
            None => {
                let home = (self.host.parent_env)("HOME")
                    .or_else(|| (self.host.parent_env)("USERPROFILE"))?;
                PathBuf::from(home).join(".claude").join("projects")
            }
        };
        Some(
            projects
                .join(encoded_folder(&run.setup.agent_folder))
                .join(format!("{}.jsonl", run.runtime_session_id)),
        )
    }

    /// `start` step 6 then steps 7–11. `None`: the run ended without a process.
    pub(crate) async fn launch(
        &self,
        run: &Arc<Run>,
        input: &StartInput,
    ) -> Result<Option<Process>, Failure> {
        let decision = match self.rules.begin_run(start_facts(run, input)).await {
            Ok(decision) => decision,
            Err(error) => {
                // spec: ordin-desktop/components/ClaudeLocalAdapter#SA-RECON-02
                run_log!(
                    warn,
                    run,
                    "claude.run.ended",
                    code = "CLA_RULES_UNAVAILABLE",
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
            run_log!(info, run, "claude.run.ended", code = "CLA_RUN_START_ENDED");
            run.emit_end(RunEndReason::Completed, Some(cut(text, MAX_FINAL_MESSAGE)));
            self.close_session(run).await;
            return Ok(None);
        }
        *lock(&run.plan) = SessionPlan {
            tool_subset: decision.tool_subset,
            single_call: decision.single_call,
            limit_seconds: decision.limit_seconds,
        };
        let mut process = match self.open_process(run).await {
            Ok(process) => process,
            Err(OpenStop::Ended) => return Ok(None),
            Err(OpenStop::Failed(failure)) => {
                self.close_session(run).await;
                return Err(failure);
            }
        };
        let mut turn = first_turn(run, input, decision.command.as_deref());
        // Only a run that has the `work` server has a plan tool; Ordin's own conversation has none.
        let mcp = files::session_dir(&self.config.app_data_root, run.run_id).join("mcp.json");
        let has_plan_tool = std::fs::read(&mcp)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .is_some_and(|json| lists_work_server(&json));
        if has_plan_tool {
            turn = with_progress_line(turn, input);
        }
        process.write_turn(&turn).await;
        Ok(Some(process))
    }

    /// `start` steps 7–10, and the restart of `resume`.
    pub(crate) async fn open_process(&self, run: &Arc<Run>) -> Result<Process, OpenStop> {
        // Step 7.
        // spec: ordin-desktop/components/ClaudeLocalAdapter#SA-RECON-03
        let mcp_config = self
            .mcp
            .open_session(SessionGrant {
                run_id: run.run_id,
                workspace_id: run.workspace_id,
                agent_instance_id: run.agent_instance_id,
                harness: HarnessKind::ClaudeCode,
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
                    "claude.session.open_failed",
                    code = "CLA_MCP_OPEN_FAILED",
                    mcp_code = error.code()
                );
                Failure::unreachable("CLA_MCP_OPEN_FAILED", !permanent)
            })?;
        lock(&run.registered).mcp = true;
        let hook_env = self
            .hooks
            .open_session(HookSessionSpec {
                run_id: run.run_id,
                agent_instance_id: run.agent_instance_id,
                workspace_id: run.workspace_id,
                harness: HarnessKind::ClaudeCode,
            })
            .map_err(|error: HookRelayError| {
                run_log!(
                    warn,
                    run,
                    "claude.session.open_failed",
                    code = "CLA_HOOK_OPEN_FAILED",
                    hook_code = error.code()
                );
                Failure::unreachable("CLA_HOOK_OPEN_FAILED", true)
            })?;
        lock(&run.registered).hook = true;
        let hook_events = self.hooks.session_events(run.run_id);

        // Step 8.
        let dir = files::session_dir(&self.config.app_data_root, run.run_id);
        lock(&run.registered).dir = Some(dir.clone());
        files::create_session_dir(&dir).map_err(config_write_failed)?;
        let mcp_files = files::mcp_files(
            &mcp_config,
            &lock(&run.package_servers),
            &self.config.hook_program,
            &dir,
        )
        .map_err(|name| {
            run_log!(
                warn,
                run,
                "claude.session.open_failed",
                code = "CLA_SERVER_NAME_CLASH",
                server = name.as_str()
            );
            Failure::unreachable("CLA_SERVER_NAME_CLASH", false)
        })?;
        for (path, bytes) in &mcp_files.launches {
            files::write_private(path, bytes).map_err(config_write_failed)?;
        }
        files::write_private(
            &dir.join("mcp.json"),
            mcp_files.mcp_json.to_string().as_bytes(),
        )
        .map_err(config_write_failed)?;
        let helper = self.write_key_route(run, &dir)?;
        let settings =
            files::settings_json(&self.config.hook_program, run.agent_instance_id, helper);
        files::write_private(&dir.join("settings.json"), settings.to_string().as_bytes())
            .map_err(config_write_failed)?;
        let env = child_environment(
            &self.host.parent_env,
            &self.config.sign_in,
            Some(&hook_env),
            &self.config.claude_path,
        );
        drop(hook_env);

        // Step 9.
        let agent_profile = self.agent_profile();
        let continued = run.is_continued();
        if agent_profile && continued {
            self.restore_record(run).await?;
        }

        // Step 10.
        let plan = lock(&run.plan).clone();
        let args = session_args(&CommandInput {
            runtime_session_id: &run.runtime_session_id,
            flag: if continued {
                SessionFlag::Resume
            } else {
                SessionFlag::New
            },
            session_dir: &dir,
            setup: &run.setup,
            agent_profile,
            tool_subset: plan.tool_subset.as_deref(),
        });
        let mut command = Command::new(&self.config.claude_path);
        command
            .args(&args)
            .env_clear()
            .envs(env.iter().map(|(name, value)| (name, value)))
            .current_dir(&run.setup.agent_folder)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let spawn_failed = || Failure::unreachable("CLA_SPAWN_FAILED", true);
        let mut child = command.spawn().map_err(|error| {
            // The io kind only: the error's text can name the program's path.
            run_log!(
                warn,
                run,
                "claude.session.spawn_failed",
                code = "CLA_SPAWN_FAILED",
                io_kind = format!("{:?}", error.kind()).as_str()
            );
            spawn_failed()
        })?;
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(spawn_failed().into());
        };
        let process = Process::new(child, stdin, stdout, stderr, hook_events);
        run.continued.store(true, Ordering::SeqCst);
        run_log!(
            info,
            run,
            "claude.session.spawned",
            pid = process.pid,
            harness_version = run.harness_version.as_str()
        );
        Ok(process)
    }

    /// With `KeyRoute::SettingsApiKeyHelper` and a key: writes `key` and gives the settings line.
    fn write_key_route(&self, run: &Run, dir: &Path) -> Result<Option<String>, Failure> {
        let SignInMode::OrganisationKey {
            models,
            route: KeyRoute::SettingsApiKeyHelper,
        } = &self.config.sign_in
        else {
            return Ok(None);
        };
        let Some(key) = &models.key else {
            return Ok(None);
        };
        files::write_private(&dir.join("key"), key.expose().as_bytes())
            .map_err(config_write_failed)?;
        Ok(Some(files::api_key_helper(
            &self.config.hook_program,
            run.run_id,
            &self.config.app_data_root,
        )))
    }

    /// `start` step 9: the record back at Claude Code's own location.
    async fn restore_record(&self, run: &Arc<Run>) -> Result<(), OpenStop> {
        let loaded = match &self.records {
            Some(records) => records
                .load(run.run_id, &run.runtime_session_id)
                .await
                .unwrap_or_else(|error| {
                    run_log!(
                        warn,
                        run,
                        "claude.session.record_unavailable",
                        record_code = error.code
                    );
                    None
                }),
            None => None,
        };
        let written = loaded.and_then(|record| {
            let path = self.session_record_path(run)?;
            std::fs::create_dir_all(path.parent()?).ok()?;
            std::fs::write(path, record).ok()
        });
        if written.is_none() {
            run_log!(
                warn,
                run,
                "claude.run.ended",
                code = "CLA_SESSION_RECORD_MISSING"
            );
            run.emit_end(RunEndReason::Interrupted, None);
            self.close_session(run).await;
            return Err(OpenStop::Ended);
        }
        Ok(())
    }

    /// `cancel` step 4: removes what is registered, and once the run has ended drops its secrets
    /// from memory; errors are logged, not returned.
    pub(crate) async fn close_session(&self, run: &Run) {
        if run.is_ended() {
            lock(&run.package_servers).clear();
        }
        let registered = std::mem::take(&mut *lock(&run.registered));
        if registered.hook {
            self.hooks.close_session(run.run_id);
        }
        if registered.mcp {
            if let Err(error) = self.mcp.close_session(run.run_id).await {
                run_log!(
                    warn,
                    run,
                    "claude.session.close_failed",
                    mcp_code = error.code()
                );
            }
        }
        if let Some(dir) = registered.dir {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
