//! One `python_sandbox` call (Sandbox.md *Internal Logic* C and D).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use ordin_journal::NewJournalEntry;
use ordin_mcp_front::{ToolCallContext, ToolCaller, ToolFailure, ToolKey, ToolOutput, ToolSpec};
use ordin_types::domain::places::ArtefactLocation;
use ordin_types::{AgentInstanceId, CredentialId, PlaceId, RunId, WorkspaceId};
use sandboxd::controller::{ControllerError, RunOutcome};
use sandboxd::runtime::{RUNTIME_INSTANTIATE_EXIT, RUNTIME_LIMIT_EXIT};
use sandboxd::types::{ClientIdentity, RunRequest, SessionHandle};
use sandboxd::workdir::{ChangeKind, WorkDirError, WorkFile};
use serde_json::{json, Value};
use tokio::sync::SemaphorePermit;

use crate::agent::{self, AgentController, BuildParts, RunContext};
use crate::args::{self, SandboxArgs, RESERVED_CAPABILITY};
use crate::error::Refusal;
use crate::grants::{PlaceFileError, SandboxCredential};
use crate::work::{self, WorkSession, MAX_SESSIONS};
use crate::{lock, manifest, Core, PYTHON_TOOL, SERVER};

/// Waiting for a guest slot gives up after this long.
const SLOT_WAIT: Duration = Duration::from_secs(30);
/// The 30-second guest deadline plus a 5-second margin.
const RUN_WAIT: Duration = Duration::from_secs(35);
/// One place input may be at most 16 MiB.
const MAX_INPUT_BYTES: u64 = 16 * 1024 * 1024;
/// All inputs together: 64 MiB and 1,000 files (the `WorkLimits` defaults).
const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_FILES: usize = 1_000;
/// The folder, inside the agent's folder, for a run's output files.
const OUTPUT_FOLDER: &str = "sandbox-output";

/// Releases what a call holds, on every path: the run context and the run's tokens, and the
/// work session's busy flag (a session whose first run never wrote its folder is dropped).
struct CallGuard<'a> {
    core: &'a Core,
    run_key: String,
    run_id: RunId,
    work_session: bool,
    controller: Option<Arc<AgentController>>,
}

impl Drop for CallGuard<'_> {
    fn drop(&mut self) {
        lock(&self.core.runs).remove(&self.run_key);
        if let Some(c) = &self.controller {
            c.creds.clear();
        }
        if self.work_session {
            let mut sessions = lock(&self.core.sessions);
            let drop_it = match sessions.get_mut(&self.run_id) {
                Some(s) => {
                    s.busy = false;
                    !s.created
                }
                None => false,
            };
            if drop_it {
                sessions.remove(&self.run_id);
            }
        }
    }
}

fn refuse(ctx: &ToolCallContext, r: Refusal) -> ToolFailure {
    tracing::info!(
        target: "ordin_sandbox",
        run_id = %ctx.run_id, agent_instance_id = ctx.agent_instance_id.to_string().as_str(),
        tool = PYTHON_TOOL, code = r.code,
        "sandbox.run_refused"
    );
    r.into_failure()
}

/// `ToolProvider::call` (C.1–C.15).
pub(crate) async fn call(
    core: &Core,
    ctx: ToolCallContext,
    key: &ToolKey,
    arguments: Value,
) -> Result<ToolOutput, ToolFailure> {
    run(core, &ctx, key, arguments)
        .await
        .map_err(|r| refuse(&ctx, r))
}

async fn run(
    core: &Core,
    ctx: &ToolCallContext,
    key: &ToolKey,
    arguments: Value,
) -> Result<ToolOutput, Refusal> {
    // C.1 Tool.
    let offered = core.tool_specs();
    if key.server.as_str() != SERVER || !offered.iter().any(|s| s.name == key.tool) {
        return Err(Refusal::tool_unknown());
    }
    if matches!(ctx.caller, ToolCaller::HostedAgent) {
        return Err(Refusal::caller_not_supported());
    }
    let (agent, workspace, run_id) = (ctx.agent_instance_id, ctx.workspace_id, ctx.run_id);

    // C.2 Arguments.
    let args = args::check(&arguments).map_err(|p| Refusal::args_invalid(&p))?;
    if !args.environment.is_empty() {
        // spec: ordin-desktop/components/Sandbox#SBX_ENVIRONMENT_UNSUPPORTED
        // SPEC-GAP-2: under `extra-guests` C.2 accepts `environment`, but `sandboxd`'s
        // `RunRequest` has no field to carry it until SH-4 lands, so it is refused in every build.
        return Err(Refusal::environment_unsupported());
    }

    // C.3 Concurrency.
    let mut guard = CallGuard {
        core,
        run_key: run_id.to_string(),
        run_id,
        work_session: false,
        controller: None,
    };
    if args.work_session {
        reserve_session(core, run_id)?;
        guard.work_session = true;
    }
    let _permit = acquire_slot(core).await?;

    // C.4 and D: the agent's controller.
    let controller = controller_for(core, agent, workspace).await?;
    guard.controller = Some(controller.clone());

    // C.5 Capability check, before any input is read.
    let person_credential = check_capabilities(&controller, &args.requested_capabilities)?;

    // C.6 Inputs.
    let (files, files_in, first_inputs) = gather_inputs(core, ctx, &args).await?;

    // C.7 Module prelude.
    let tools = module_tools(core, run_id);
    let (prelude, prelude_lines) = prelude(&tools);
    let mut requested = args.requested_capabilities.clone();
    if !tools.is_empty() {
        requested.push(RESERVED_CAPABILITY.to_owned());
    }

    // C.8 Run context.
    lock(&core.runs).insert(
        guard.run_key.clone(),
        RunContext {
            run_id,
            person_credential,
            cancel: ctx.cancel.clone(),
            tools,
            module_calls: 0,
            fault: None,
        },
    );

    // C.9 Run.
    let session_inputs = if args.work_session {
        files.clone().unwrap_or_default()
    } else {
        Vec::new()
    };
    let started = Instant::now();
    let request = RunRequest {
        code: format!("{prelude}{}", args.code),
        requested_capabilities: requested,
        timeout_ms: None,
        dry_run: args.dry_run,
        workspace_id: workspace.to_string(),
        run_id: Some(run_id.to_string()),
        files,
    };
    let handle = SessionHandle {
        client: ClientIdentity {
            principal: agent.to_string(),
            client_label: run_id.to_string(),
        },
        workspace_id: workspace.to_string(),
    };
    let outcome =
        match tokio::time::timeout(RUN_WAIT, controller.controller.run(request, handle)).await {
            Err(_) => return Err(Refusal::run_timeout()),
            Ok(Err(e)) => {
                return Err(controller_refusal(
                    core,
                    &guard.run_key,
                    &controller,
                    &args,
                    e,
                ))
            }
            Ok(Ok(outcome)) => outcome,
        };
    let module_calls = lock(&core.runs)
        .get(&guard.run_key)
        .map(|c| c.module_calls)
        .unwrap_or(0);

    // C.11 Result.
    let mut result = match outcome {
        RunOutcome::DryRun(plan) => {
            let value = json!({ "plannedCalls": plan.planned_calls });
            return Ok(output(value));
        }
        RunOutcome::Run(result) => result,
    };
    if result.exit_code == RUNTIME_INSTANTIATE_EXIT {
        return Err(Refusal::guest_start_failed());
    }
    if result.exit_code == RUNTIME_LIMIT_EXIT {
        result.stderr = "RUNTIME_LIMIT".to_owned();
    } else {
        result.stderr = shift_lines(&result.stderr, prelude_lines);
    }

    // C.12 Outputs as artefacts.
    let changes = result.changes.clone().unwrap_or_default();
    let mut listed = Vec::new();
    let mut save_failed = false;
    let mut saved = Vec::new();
    let mut files_out = 0usize;
    for change in &changes {
        let kind = match change.kind {
            ChangeKind::Added => "added",
            ChangeKind::Changed => "changed",
            ChangeKind::Removed => "removed",
        };
        let mut entry = json!({ "path": change.path, "kind": kind });
        if let Some(content) = &change.content_base64 {
            let saved_as = format!("{OUTPUT_FOLDER}/{run_id}/{}", change.path);
            files_out += 1;
            if save_output(core, agent, run_id, &saved_as, content).await {
                saved.push(saved_as.clone());
            } else {
                save_failed = true;
            }
            entry["savedAs"] = json!(saved_as);
        }
        listed.push(entry);
    }

    // C.13 Work session.
    let session_changes = if args.work_session {
        Some(update_session(
            core,
            run_id,
            &first_inputs,
            &session_inputs,
            &changes,
        ))
    } else {
        None
    };
    if save_failed {
        tracing::warn!(target: "ordin_sandbox", run_id = %run_id, saved = saved.len(), "sandbox.output_save_failed");
        return Err(Refusal::output_save_failed());
    }

    // C.14 Journal.
    let duration_ms = started.elapsed().as_millis() as u64;
    let code_sha256 = work::hex(&work::sha256(args.code.as_bytes()));
    let entry = NewJournalEntry {
        kind: "sandbox_run".to_owned(),
        component: "sandbox".to_owned(),
        run_id: Some(run_id),
        crew_run_id: None,
        workspace_id: Some(workspace),
        summary: format!("Sandbox run finished with exit code {}.", result.exit_code),
        data: json!({
            "tool": key.tool,
            "agent_instance_id": agent,
            "code_sha256": code_sha256,
            "exit_code": result.exit_code,
            "duration_ms": duration_ms,
            "api_calls": result.api_calls.len(),
            "module_calls": module_calls,
            "files_in": files_in,
            "files_out": files_out,
            "truncated": result.truncated,
            "work_session": args.work_session,
            "environment_names": Vec::<String>::new(),
        }),
    };
    if core.journal.append_entry(entry).await.is_err() {
        tracing::warn!(target: "ordin_sandbox", run_id = %run_id, kind = "sandbox_run", "sandbox.journal_failed");
    }

    // C.15
    tracing::info!(
        target: "ordin_sandbox",
        run_id = %run_id, agent_instance_id = agent.to_string().as_str(), workspace_id = workspace.to_string().as_str(),
        tool = key.tool.as_str(), exit_code = result.exit_code, duration_ms,
        api_calls = result.api_calls.len(), module_calls, files_in, files_out,
        truncated = result.truncated, work_session = args.work_session,
        code_sha256 = code_sha256.as_str(),
        "sandbox.run_finished"
    );
    let mut value = json!({
        "stdout": result.stdout,
        "stderr": result.stderr,
        "exitCode": result.exit_code,
        "truncated": result.truncated,
        "apiCalls": result.api_calls,
        "changes": listed,
    });
    if let Some(session_changes) = session_changes {
        value["sessionChanges"] = json!(session_changes
            .into_iter()
            .map(|(path, kind)| json!({"path": path, "kind": kind}))
            .collect::<Vec<_>>());
    }
    Ok(output(value))
}

fn output(value: Value) -> ToolOutput {
    ToolOutput {
        text: value.to_string(),
        structured: Some(value),
    }
}

/// C.3: marks the run's work session busy, reserving one when it does not exist yet.
// spec: ordin-desktop/components/Sandbox#SBX_SESSION_BUSY
fn reserve_session(core: &Core, run_id: RunId) -> Result<(), Refusal> {
    let mut sessions = lock(&core.sessions);
    let open = sessions.len();
    match sessions.get_mut(&run_id) {
        Some(s) if s.busy => Err(Refusal::session_busy()),
        Some(s) => {
            s.busy = true;
            Ok(())
        }
        None if open >= MAX_SESSIONS => Err(Refusal::sessions_full()),
        None => {
            sessions.insert(
                run_id,
                WorkSession::reserved(core.sessions_root.join(run_id.to_string())),
            );
            Ok(())
        }
    }
}

// spec: ordin-desktop/components/Sandbox#SBX_BUSY
async fn acquire_slot(core: &Core) -> Result<SemaphorePermit<'_>, Refusal> {
    match tokio::time::timeout(SLOT_WAIT, core.guest_slots.acquire()).await {
        Ok(Ok(permit)) => Ok(permit),
        _ => Err(Refusal::busy()),
    }
}

/// D.1–D.2: the cached controller, rebuilt when the grants changed.
// spec: ordin-desktop/components/Sandbox#SBX_RULES_UNAVAILABLE
async fn controller_for(
    core: &Core,
    agent: AgentInstanceId,
    workspace: WorkspaceId,
) -> Result<Arc<AgentController>, Refusal> {
    let grants = core
        .grants
        .sandbox_capabilities(agent, workspace)
        .await
        .map_err(|_| Refusal::rules_unavailable())?;
    let capabilities = manifest::kept(grants);
    let digest = work::hex(&work::sha256(format!("{capabilities:?}").as_bytes()));
    let mut controllers = core.controllers.lock().await;
    if let Some(found) = controllers.get(&(agent, workspace)) {
        if found.digest == digest {
            *lock(&found.last_used) = Instant::now();
            return Ok(found.clone());
        }
    }
    let built = Arc::new(
        agent::build(BuildParts {
            agent,
            workspace,
            capabilities,
            digest,
            identity: &core.identity,
            audit_key: core.audit_key.clone(),
            audit_tx: core.audit_tx.clone(),
            runs: core.runs.clone(),
            module: core.module.clone(),
            guest: core.guest,
            guest_digest: core.guest_digest,
        })
        .await?,
    );
    controllers.insert((agent, workspace), built.clone());
    Ok(built)
}

/// C.5: every requested name granted, at most one person credential. Returns that credential.
// spec: ordin-desktop/components/Sandbox#SBX_MIXED_CREDENTIALS
fn check_capabilities(
    controller: &AgentController,
    requested: &[String],
) -> Result<Option<CredentialId>, Refusal> {
    let mut credentials = BTreeSet::new();
    for name in requested {
        let Some(cap) = controller.capabilities.iter().find(|c| &c.name == name) else {
            return Err(Refusal::capability_unknown(
                name,
                &controller.granted_names(),
            ));
        };
        if let SandboxCredential::PersonToken { credential_id, .. } = &cap.credential {
            credentials.insert(*credential_id);
        }
    }
    if credentials.len() > 1 {
        return Err(Refusal::mixed_credentials());
    }
    Ok(credentials.into_iter().next())
}

/// The file list of C.6, the count of files given and the first-run baseline candidates.
type Inputs = (Option<Vec<WorkFile>>, usize, BTreeMap<String, [u8; 32]>);

/// C.6: session files, then place inputs, then `files`; later sources override earlier ones.
async fn gather_inputs(
    core: &Core,
    ctx: &ToolCallContext,
    args: &SandboxArgs,
) -> Result<Inputs, Refusal> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut merged: BTreeMap<String, WorkFile> = BTreeMap::new();
    let mut total: u64 = 0;
    // Files this run writes into a new session's folder: its baseline (C.13).
    let mut written: BTreeMap<String, [u8; 32]> = BTreeMap::new();

    // a. The work session's folder.
    if args.work_session {
        let existing = {
            let sessions = lock(&core.sessions);
            sessions
                .get(&ctx.run_id)
                .filter(|s| s.created)
                .map(|s| s.dir.clone())
        };
        if let Some(dir) = existing {
            let files = work::read_files(&dir).map_err(|_| Refusal::internal())?;
            for (path, bytes) in files {
                total += bytes.len() as u64;
                merged.insert(
                    path.clone(),
                    WorkFile {
                        path,
                        content_base64: b64.encode(&bytes),
                    },
                );
            }
        }
    }

    // b. Place inputs.
    // spec: ordin-desktop/components/Sandbox#SBX_INPUT_NOT_GRANTED
    let mut seen = BTreeSet::new();
    for input in &args.inputs {
        let place =
            PlaceId::parse(&input.place_id).map_err(|_| Refusal::input_not_found(&input.path))?;
        let target = input.as_path.clone().unwrap_or_else(|| {
            input
                .path
                .rsplit('/')
                .next()
                .unwrap_or(&input.path)
                .to_owned()
        });
        if !seen.insert(target.clone()) {
            return Err(Refusal::file_invalid(&target));
        }
        let bytes = core
            .place_files
            .read_for_agent(
                ctx.agent_instance_id,
                ctx.workspace_id,
                place,
                &input.path,
                MAX_INPUT_BYTES,
            )
            .await
            .map_err(|e| match e {
                PlaceFileError::NotGranted => Refusal::input_not_granted(),
                PlaceFileError::NotFound => Refusal::input_not_found(&input.path),
                PlaceFileError::TooLarge => Refusal::inputs_too_large(),
                PlaceFileError::Unreadable => Refusal::input_unreadable(&input.path),
            })?;
        total += bytes.len() as u64;
        written.insert(target.clone(), work::sha256(&bytes));
        merged.insert(
            target.clone(),
            WorkFile {
                path: target,
                content_base64: b64.encode(&bytes),
            },
        );
    }

    // c. Files given as content.
    let mut seen = BTreeSet::new();
    for file in &args.files {
        if !seen.insert(file.path.clone()) {
            return Err(Refusal::file_invalid(&file.path));
        }
        let decoded = b64.decode(&file.content_base64).ok();
        total += decoded
            .as_ref()
            .map(|d| d.len() as u64)
            .unwrap_or((file.content_base64.len() as u64 / 4) * 3);
        if let Some(bytes) = decoded {
            written.insert(file.path.clone(), work::sha256(&bytes));
        }
        merged.insert(
            file.path.clone(),
            WorkFile {
                path: file.path.clone(),
                content_base64: file.content_base64.clone(),
            },
        );
    }

    // spec: ordin-desktop/components/Sandbox#SBX_INPUTS_TOO_LARGE
    if total > MAX_TOTAL_BYTES || merged.len() > MAX_TOTAL_FILES {
        return Err(Refusal::inputs_too_large());
    }
    let count = merged.len();
    let files = if merged.is_empty() && !args.work_session {
        None
    } else {
        Some(merged.into_values().collect())
    };
    Ok((files, count, written))
}

/// `^[A-Za-z][A-Za-z0-9_]{0,63}$`: a tool name usable as a Python attribute.
fn module_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && name.len() <= 64
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// C.7: the session's sandbox-callable tools usable in the module.
// spec: ordin-desktop/components/Sandbox#SA-NAME-06
fn module_tools(core: &Core, run_id: RunId) -> Vec<(ToolKey, ToolSpec)> {
    let listed = match core.dispatch.session_tools(run_id) {
        Ok(listed) => listed,
        Err(_) => {
            tracing::info!(target: "ordin_sandbox", run_id = %run_id, reason = "session_unknown", "sandbox.module_tool_skipped");
            return Vec::new();
        }
    };
    let mut kept: Vec<(ToolKey, ToolSpec)> = Vec::new();
    for (key, spec) in listed {
        if key.server.as_str() == SERVER {
            continue;
        }
        if !module_name(&key.tool) {
            tracing::info!(target: "ordin_sandbox", run_id = %run_id, tool = key.tool.as_str(), reason = "name", "sandbox.module_tool_skipped");
            continue;
        }
        if kept.iter().any(|(k, _)| k.tool == key.tool) {
            tracing::info!(target: "ordin_sandbox", run_id = %run_id, tool = key.tool.as_str(), reason = "duplicate", "sandbox.module_tool_skipped");
            continue;
        }
        kept.push((key, spec));
    }
    kept
}

/// The Python prelude of SA-NAME-06 and its line count; empty without tools.
pub(crate) fn prelude(tools: &[(ToolKey, ToolSpec)]) -> (String, usize) {
    if tools.is_empty() {
        return (String::new(), 0);
    }
    let mut lines = vec![
        "class _OrdinTools: pass".to_owned(),
        "ordin_tools = _OrdinTools()".to_owned(),
    ];
    for (key, _) in tools {
        let tool = &key.tool;
        lines.push(format!(
            "ordin_tools.{tool} = lambda **kw: mcp.ordin_tools.call_tool(\"{tool}\", kw)"
        ));
    }
    let count = lines.len();
    (format!("{}\n", lines.join("\n")), count)
}

/// C.11: `line N` becomes `line N−L` where `N > L`.
pub(crate) fn shift_lines(stderr: &str, prelude_lines: usize) -> String {
    if prelude_lines == 0 {
        return stderr.to_owned();
    }
    let Ok(re) = regex::Regex::new(r"line (\d+)") else {
        return stderr.to_owned();
    };
    re.replace_all(stderr, |caps: &regex::Captures<'_>| {
        match caps[1].parse::<usize>() {
            Ok(n) if n > prelude_lines => format!("line {}", n - prelude_lines),
            _ => caps[0].to_owned(),
        }
    })
    .into_owned()
}

/// C.10: the controller's refusal, read with the run context's recorded sign-in fault.
// spec: ordin-desktop/components/Sandbox#SBX_SIGN_IN_NEEDED
fn controller_refusal(
    core: &Core,
    run_key: &str,
    controller: &AgentController,
    args: &SandboxArgs,
    error: ControllerError,
) -> Refusal {
    let fault = lock(&core.runs).get(run_key).and_then(|c| c.fault);
    refusal_for(
        error,
        fault,
        &controller.capabilities,
        &args.requested_capabilities,
    )
}

/// The mapping of C.10.
pub(crate) fn refusal_for(
    error: ControllerError,
    fault: Option<(&'static str, bool)>,
    capabilities: &[crate::grants::SandboxCapability],
    requested: &[String],
) -> Refusal {
    let granted: Vec<String> = capabilities.iter().map(|c| c.name.clone()).collect();
    match error {
        ControllerError::CapUnknown => {
            let name = requested
                .iter()
                .find(|n| !granted.contains(n))
                .cloned()
                .unwrap_or_default();
            Refusal::capability_unknown(&name, &granted)
        }
        ControllerError::InteractionDenied => match fault {
            Some((_, false)) => {
                let system = requested
                    .iter()
                    .find(|n| {
                        capabilities.iter().any(|c| {
                            &c.name == *n
                                && matches!(c.credential, SandboxCredential::PersonToken { .. })
                        })
                    })
                    .cloned()
                    .unwrap_or_default();
                Refusal::sign_in_needed(&system)
            }
            Some((_, true)) => Refusal::credential_unavailable(),
            None => Refusal::interaction_refused(),
        },
        ControllerError::RateLimited => Refusal::run_budget_exhausted(),
        ControllerError::WorkDir(e) => match e {
            WorkDirError::InvalidPath(p)
            | WorkDirError::DuplicatePath(p)
            | WorkDirError::InvalidContent(p) => Refusal::file_invalid(&p),
            WorkDirError::InputsTooLarge => Refusal::inputs_too_large(),
            WorkDirError::LimitExceeded | WorkDirError::Io => Refusal::internal(),
        },
    }
}

/// C.12 media types by extension.
pub(crate) fn media_type(path: &str) -> &'static str {
    let ext = path
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "csv" => "text/csv",
        "json" => "application/json",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "pdf" => "application/pdf",
        "png" => "image/png",
        _ => "application/octet-stream",
    }
}

/// C.12: writes one output into the agent's folder and indexes it. False on any failure.
// spec: ordin-desktop/components/Sandbox#SA-RECON-07
async fn save_output(
    core: &Core,
    agent: AgentInstanceId,
    run_id: RunId,
    saved_as: &str,
    content_base64: &str,
) -> bool {
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(content_base64) else {
        return false;
    };
    let Ok(hash) = core
        .place_files
        .write_agent_output(agent, saved_as, &bytes)
        .await
    else {
        return false;
    };
    core.journal
        .record_artefact(ordin_journal::NewArtefact {
            location: ArtefactLocation::AgentFolder {
                agent_instance_id: agent,
            },
            path: saved_as.to_owned(),
            hash,
            media_type: media_type(saved_as).to_owned(),
            producing_run_id: Some(run_id),
        })
        .await
        .is_ok()
}

/// C.13: creates the folder on first use, writes this run's inputs and applies its changes, so
/// the folder holds `/work` as the run left it; returns `sessionChanges`.
fn update_session(
    core: &Core,
    run_id: RunId,
    first_inputs: &BTreeMap<String, [u8; 32]>,
    inputs: &[WorkFile],
    changes: &[sandboxd::workdir::FileChange],
) -> Vec<(String, &'static str)> {
    let mut sessions = lock(&core.sessions);
    let Some(session) = sessions.get_mut(&run_id) else {
        return Vec::new();
    };
    if !session.created {
        if work::create_private_dir(&session.dir).is_err() {
            return Vec::new();
        }
        session.baseline = first_inputs.clone();
        session.created = true;
    }
    let b64 = base64::engine::general_purpose::STANDARD;
    // An input the run left unchanged appears in no change record; write it so it stays.
    for input in inputs {
        if let Ok(bytes) = b64.decode(&input.content_base64) {
            let _ = work::apply(&session.dir, &input.path, Some(&bytes));
        }
    }
    for change in changes {
        let content = change
            .content_base64
            .as_deref()
            .and_then(|c| b64.decode(c).ok());
        let _ = match change.kind {
            ChangeKind::Removed => work::apply(&session.dir, &change.path, None),
            _ => work::apply(&session.dir, &change.path, content.as_deref()),
        };
    }
    session.last_used = Instant::now();
    let now: BTreeMap<String, [u8; 32]> = work::read_files(&session.dir)
        .unwrap_or_default()
        .into_iter()
        .map(|(p, b)| (p, work::sha256(&b)))
        .collect();
    work::session_changes(&session.baseline, &now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordin_mcp_front::ServerName;

    fn tool(server: &str, name: &str) -> (ToolKey, ToolSpec) {
        (
            ToolKey {
                server: ServerName::new(server).unwrap(),
                tool: name.to_owned(),
            },
            ToolSpec {
                name: name.to_owned(),
                description: "d".to_owned(),
                input_schema: json!({"type": "object"}),
                argument_form: "f".to_owned(),
                sandbox_callable: true,
                plain: "p".to_owned(),
            },
        )
    }

    #[test]
    fn the_prelude_binds_each_tool_exactly() {
        let (text, lines) = prelude(&[tool("mail", "email_search")]);
        assert_eq!(
            text,
            "class _OrdinTools: pass\nordin_tools = _OrdinTools()\nordin_tools.email_search = lambda **kw: mcp.ordin_tools.call_tool(\"email_search\", kw)\n"
        );
        assert_eq!(lines, 3);
        assert_eq!(prelude(&[]), (String::new(), 0));
    }

    #[test]
    fn stderr_lines_are_shifted_past_the_prelude() {
        assert_eq!(
            shift_lines("File \"<s>\", line 5, in <module>; line 2", 3),
            "File \"<s>\", line 2, in <module>; line 2"
        );
        assert_eq!(shift_lines("line 9", 0), "line 9");
    }

    #[test]
    fn module_names_refuse_hyphens() {
        assert!(module_name("email_search"));
        assert!(!module_name("get-ticket"));
        assert!(!module_name("_x"));
        assert!(!module_name(""));
    }

    #[test]
    fn controller_errors_map_to_the_error_table() {
        use crate::grants::{SandboxCapability, SandboxCapabilityKind};
        use ordin_types::domain::identity::CredentialKind;
        let caps = vec![SandboxCapability {
            name: "tickets".to_owned(),
            kind: SandboxCapabilityKind::Rest {
                host: "tickets.example.com".to_owned(),
                path_allow: vec![".*".to_owned()],
                methods: vec!["GET".to_owned()],
            },
            allow_write: false,
            allow_private_network: false,
            credential: SandboxCredential::PersonToken {
                credential_id: CredentialId::new(),
                kind: CredentialKind::Microsoft,
            },
        }];
        let req = vec!["tickets".to_owned()];
        let code = |e, fault| refusal_for(e, fault, &caps, &req).code;
        assert_eq!(
            code(ControllerError::CapUnknown, None),
            "SBX_CAPABILITY_UNKNOWN"
        );
        assert_eq!(
            code(
                ControllerError::InteractionDenied,
                Some(("CRED_SIGN_IN_REQUIRED", false))
            ),
            "SBX_SIGN_IN_NEEDED"
        );
        assert_eq!(
            code(
                ControllerError::InteractionDenied,
                Some(("CRED_KEYCHAIN_UNAVAILABLE", true))
            ),
            "SBX_CREDENTIAL_UNAVAILABLE"
        );
        assert_eq!(
            code(ControllerError::InteractionDenied, None),
            "SBX_INTERACTION_REFUSED"
        );
        assert_eq!(
            code(ControllerError::RateLimited, None),
            "SBX_RUN_BUDGET_EXHAUSTED"
        );
        let file = refusal_for(
            ControllerError::WorkDir(WorkDirError::DuplicatePath("a.txt".to_owned())),
            None,
            &caps,
            &req,
        );
        assert_eq!(file.code, "SBX_FILE_INVALID");
        assert!(file.message.contains("a.txt"));
        assert_eq!(
            code(ControllerError::WorkDir(WorkDirError::InputsTooLarge), None),
            "SBX_INPUTS_TOO_LARGE"
        );
        assert_eq!(
            code(ControllerError::WorkDir(WorkDirError::Io), None),
            "SBX_INTERNAL"
        );
        let signed = refusal_for(
            ControllerError::InteractionDenied,
            Some(("CRED_NOT_FOUND", false)),
            &caps,
            &req,
        );
        assert!(signed.message.contains("tickets"));
    }

    #[test]
    fn media_types_follow_the_extension() {
        for (path, media) in [
            ("a.csv", "text/csv"),
            ("a.JSON", "application/json"),
            ("a.txt", "text/plain"),
            ("a.md", "text/markdown"),
            ("a.pdf", "application/pdf"),
            ("a.png", "image/png"),
            ("a.bin", "application/octet-stream"),
            ("noext", "application/octet-stream"),
        ] {
            assert_eq!(media_type(path), media, "{path}");
        }
    }
}
