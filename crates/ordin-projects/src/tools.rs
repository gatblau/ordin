//! The MCP servers `code`, `github`, `gitlab` and `checks` (ADR-023, ADR-051, ADR-054). The MCP
//! Front's Rules Engine decision precedes every call here; `checks.run_check` also takes the
//! decision on its command (Internal Logic 7.1). Tools of `code` only read.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ordin_mcp_front::{
    ArgumentProblem, ServerName, ServerSpec, SessionGrant, ToolCallContext, ToolFailure, ToolKey,
    ToolOutput, ToolProvider, ToolSpec,
};
use ordin_rules::{ApprovalWait, CommandOrigin, CommandRequest};
use ordin_types::wire::rules::Verdict;
use ordin_types::{CodingTaskId, RunId, WorkspaceId};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::editor::language_of;
use crate::error::{GithubError, ProjectError};
use crate::git;
use crate::host_cli::HostKind;
use crate::lsp::{path_of, uri_of, Instance, InstanceKey};
use crate::service::{lock, ProjectService};
use crate::store;
use ordin_rules::host_forms::{self, Effect};

const MAX_ARGS: usize = 64;
const MAX_ARG_CHARS: usize = 4_000;
const MAX_COMMAND_CHARS: usize = 1_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PositionArgs {
    path: String,
    line: u32,
    character: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathArgs {
    path: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HostArgs {
    args: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckArgs {
    command: String,
}

/// The Error Table's sentence for a code a model can meet.
pub(crate) fn sentence(code: &str) -> &'static str {
    match code {
        "PROJ_LSP_UNAVAILABLE" => "Code intelligence isn't available for this language.",
        "PROJ_NOT_FOUND" => "That project is no longer open.",
        "PROJ_SIGN_IN_NEEDED" | "GH_SIGN_IN_NEEDED" => {
            "Sign in to GitHub to reach this repository."
        }
        "PROJ_REMOTE_UNREACHABLE" | "GH_UNREACHABLE" => "Can't reach the repository right now.",
        "PROJ_CHECK_DENIED" => "This command isn't allowed here.",
        "PROJ_CHECK_TIMEOUT" => "The check took too long and was stopped.",
        "PROJ_DATASTORE_UNAVAILABLE" => "ordin couldn't reach its own records.",
        "GH_RATE_LIMITED" => "GitHub asked ordin to slow down. ordin will try again shortly.",
        "GH_FAILED" => "Something went wrong talking to the repository host.",
        _ => "This tool could not finish.",
    }
}

fn failed(code: &'static str, retryable: bool) -> ToolFailure {
    ToolFailure {
        code: code.to_owned(),
        message: sentence(code).to_owned(),
        retryable,
    }
}

fn failure(error: ProjectError) -> ToolFailure {
    let retryable = error.retryable();
    failed(error.code(), retryable)
}

fn host_failure(error: GithubError) -> ToolFailure {
    let retryable = error.retryable();
    failed(error.code(), retryable)
}

fn output(value: Value) -> ToolOutput {
    ToolOutput {
        text: value.to_string(),
        structured: Some(value),
    }
}

fn parse<T: DeserializeOwned>(arguments: &Value) -> Result<T, ArgumentProblem> {
    serde_json::from_value(arguments.clone()).map_err(|error| ArgumentProblem {
        at: String::new(),
        reason: error.to_string().chars().take(300).collect(),
    })
}

fn path_problem() -> ArgumentProblem {
    ArgumentProblem {
        at: "/path".to_owned(),
        reason: "The path must be relative to the worktree, with no `..`.".to_owned(),
    }
}

fn check_path(path: &str) -> Result<(), ArgumentProblem> {
    if git::relative_path_ok(path) {
        Ok(())
    } else {
        Err(path_problem())
    }
}

/// `(worktree-relative path, line, character)`: a location outside the worktree is dropped.
fn location_of(root: &Path, value: &Value) -> Option<Value> {
    let (uri, range) = match value.get("targetUri") {
        Some(uri) => (uri, &value["targetSelectionRange"]),
        None => (&value["uri"], &value["range"]),
    };
    let path = path_of(uri.as_str()?)?;
    let relative = path.strip_prefix(root).ok()?;
    Some(json!({
        "path": relative.to_string_lossy(),
        "line": range["start"]["line"],
        "character": range["start"]["character"],
    }))
}

fn locations_of(root: &Path, result: &Value) -> Value {
    let list: Vec<Value> = match result {
        Value::Array(items) => items.iter().filter_map(|v| location_of(root, v)).collect(),
        Value::Null => Vec::new(),
        single => location_of(root, single).into_iter().collect(),
    };
    json!({ "locations": list })
}

fn symbols_of(root: &Path, result: &Value) -> Value {
    let list: Vec<Value> = result
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|symbol| {
            let located = symbol.get("location");
            if let Some(location) = located {
                location_of(root, location).map(|place| {
                    json!({"name": symbol["name"], "kind": symbol["kind"], "location": place})
                })
            } else {
                Some(json!({
                    "name": symbol["name"],
                    "kind": symbol["kind"],
                    "line": symbol["range"]["start"]["line"],
                }))
            }
        })
        .collect();
    json!({ "symbols": list })
}

fn tool(name: &str, description: &str, schema: Value, form: &str, plain: &str) -> ToolSpec {
    ToolSpec {
        name: name.to_owned(),
        description: description.to_owned(),
        input_schema: schema,
        argument_form: form.to_owned(),
        sandbox_callable: false,
        plain: plain.to_owned(),
    }
}

fn position_tool(name: &str, description: &str, plain: &str) -> ToolSpec {
    tool(
        name,
        description,
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["path", "line", "character"],
            "properties": {
                "path": {"type": "string"},
                "line": {"type": "integer", "minimum": 0},
                "character": {"type": "integer", "minimum": 0},
            },
        }),
        r#"{"path": "src/lib.rs", "line": 41, "character": 12}"#,
        plain,
    )
}

fn path_tool(name: &str, description: &str, plain: &str) -> ToolSpec {
    tool(
        name,
        description,
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {"path": {"type": "string"}},
        }),
        r#"{"path": "src/lib.rs"} or {}"#,
        plain,
    )
}

fn host_tool(name: &str, description: &str, plain: &str) -> ToolSpec {
    tool(
        name,
        description,
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["args"],
            "properties": {"args": {
                "type": "array",
                "minItems": 1,
                "maxItems": MAX_ARGS,
                "items": {"type": "string", "maxLength": MAX_ARG_CHARS},
            }},
        }),
        r#"{"args": ["pr", "list"]}"#,
        plain,
    )
}

fn key(server: &str, tool: &str) -> Option<ToolKey> {
    Some(ToolKey {
        server: ServerName::new(server).ok()?,
        tool: tool.to_owned(),
    })
}

/// The provider the program registers with the MCP Front.
pub(crate) struct ProjectTools {
    service: Arc<ProjectService>,
}

impl ProjectTools {
    pub(crate) fn new(service: Arc<ProjectService>) -> ProjectTools {
        ProjectTools { service }
    }

    /// The host kinds of the repositories the workspace's open projects link.
    async fn workspace_hosts(&self, workspace: WorkspaceId) -> BTreeSet<&'static str> {
        let service = &self.service;
        let mut kinds = BTreeSet::new();
        let Ok(rows) = store::open_projects_in(&service.pool, workspace).await else {
            return kinds;
        };
        for row in rows {
            if !row.project.is_git_repository {
                continue;
            }
            let Ok(folder) = service.place_folder(row.project.place_id).await else {
                continue;
            };
            if let Ok(kind) = service.origin_kind(&folder).await {
                kinds.insert(kind.name());
            }
        }
        kinds
    }

    /// The worktree instance serving a file's language (Internal Logic 9.6). A language not
    /// confirmed for worktrees is unavailable (C-33).
    // spec: ordin-desktop/components/ProjectService#C-33
    async fn task_instance(
        &self,
        task: CodingTaskId,
        language: &str,
    ) -> Result<(Arc<Instance>, PathBuf), ProjectError> {
        let confirmed = lock(&self.service.tunables)
            .worktree_confirmed
            .contains(language);
        if !confirmed {
            return Err(ProjectError::LspUnavailable);
        }
        let row = self
            .service
            .task_row(task)
            .await
            .map_err(|_| ProjectError::LspUnavailable)?;
        let root = PathBuf::from(&row.task.worktree_path);
        let key = InstanceKey::Task(task, language.to_owned());
        let instance = self
            .service
            .instance(key, &root, language)
            .await
            .map_err(|_| ProjectError::LspUnavailable)?;
        Ok((instance, root))
    }

    /// Whether the task's worktree differs from its base, untracked files included. A worktree
    /// that cannot be read counts as changed.
    async fn worktree_changed(&self, task: CodingTaskId) -> bool {
        let Ok(row) = self.service.task_row(task).await else {
            return true;
        };
        let (path, base) = (
            PathBuf::from(&row.task.worktree_path),
            row.task.base_commit.clone(),
        );
        git::blocking(move || git::worktree_changes(&path, &base))
            .await
            .map_or(true, |changes| !changes.is_empty())
    }

    async fn opened(
        &self,
        task: CodingTaskId,
        path: &str,
    ) -> Result<(Arc<Instance>, PathBuf, String), ProjectError> {
        let language = language_of(path).ok_or(ProjectError::LspUnavailable)?;
        let (instance, root) = self.task_instance(task, language).await?;
        let file = root.join(path);
        let text = tokio::fs::read_to_string(&file)
            .await
            .map_err(|_| ProjectError::LspUnavailable)?;
        let uri = uri_of(&file);
        instance.open_document(&uri, language, &text)?;
        Ok((instance, root, uri))
    }

    /// The task's running instances, or the confirmed languages' when none has started.
    async fn task_instances(
        &self,
        task: CodingTaskId,
    ) -> Result<Vec<(Arc<Instance>, PathBuf)>, ProjectError> {
        let languages: Vec<String> = lock(&self.service.tunables)
            .worktree_confirmed
            .iter()
            .cloned()
            .collect();
        let mut out = Vec::new();
        for language in languages {
            if let Ok(found) = self.task_instance(task, &language).await {
                out.push(found);
            }
        }
        if out.is_empty() {
            return Err(ProjectError::LspUnavailable);
        }
        Ok(out)
    }

    async fn code_call(
        &self,
        task: CodingTaskId,
        tool: &str,
        arguments: &Value,
    ) -> Result<Value, ProjectError> {
        let limit = lock(&self.service.tunables).lsp_request_timeout;
        let invalid = |_| ProjectError::LspUnavailable;
        match tool {
            "definition" | "references" | "hover" => {
                let args: PositionArgs = parse(arguments).map_err(invalid)?;
                let (instance, root, uri) = self.opened(task, &args.path).await?;
                let mut params = json!({
                    "textDocument": {"uri": uri},
                    "position": {"line": args.line, "character": args.character},
                });
                if tool == "references" {
                    params["context"] = json!({"includeDeclaration": false});
                }
                let result = instance
                    .request(&format!("textDocument/{tool}"), params, limit)
                    .await?;
                Ok(if tool == "hover" {
                    json!({ "contents": result.get("contents").cloned().unwrap_or(Value::Null) })
                } else {
                    locations_of(&root, &result)
                })
            }
            "symbols" => {
                let args: PathArgs = parse(arguments).map_err(invalid)?;
                match args.path {
                    Some(path) => {
                        let (instance, root, uri) = self.opened(task, &path).await?;
                        let result = instance
                            .request(
                                "textDocument/documentSymbol",
                                json!({"textDocument": {"uri": uri}}),
                                limit,
                            )
                            .await?;
                        Ok(symbols_of(&root, &result))
                    }
                    None => {
                        let mut all = Vec::new();
                        for (instance, root) in self.task_instances(task).await? {
                            let result = instance
                                .request("workspace/symbol", json!({"query": ""}), limit)
                                .await?;
                            if let Some(list) = symbols_of(&root, &result)["symbols"].as_array() {
                                all.extend(list.iter().cloned());
                            }
                        }
                        Ok(json!({ "symbols": all }))
                    }
                }
            }
            "diagnostics" => {
                let args: PathArgs = parse(arguments).map_err(invalid)?;
                match args.path {
                    Some(path) => {
                        let (instance, _, uri) = self.opened(task, &path).await?;
                        Ok(json!({ "diagnostics": instance.diagnostics_of(&uri) }))
                    }
                    None => {
                        let mut all = serde_json::Map::new();
                        for (instance, root) in self.task_instances(task).await? {
                            for (uri, list) in instance.all_diagnostics() {
                                let inside = path_of(&uri).and_then(|path| {
                                    path.strip_prefix(&root)
                                        .ok()
                                        .map(|p| p.to_string_lossy().into_owned())
                                });
                                if let Some(path) = inside {
                                    all.insert(path, Value::Array(list));
                                }
                            }
                        }
                        Ok(json!({ "diagnostics": all }))
                    }
                }
            }
            _ => Err(ProjectError::LspUnavailable),
        }
    }

    /// Internal Logic 8.3: in the task's worktree or the project's folder; the output is the result.
    // spec: ordin-desktop/components/ProjectService#ADR-023
    // spec: ordin-desktop/components/ProjectService#ADR-051
    // spec: ordin-desktop/components/ProjectService#SA-RECON-07
    async fn host_call(
        &self,
        ctx: &ToolCallContext,
        kind: HostKind,
        arguments: Vec<String>,
    ) -> Result<ToolOutput, ToolFailure> {
        let service = &self.service;
        // Checked here too, so that no path to this call reaches a program off the list.
        let effect = host_forms::check(kind.program(), &arguments)
            .map_err(|_| failed("PROJ_CHECK_DENIED", false))?;
        let folder = self.call_folder(ctx, kind).await.map_err(failure)?;
        if effect == Effect::Write {
            self.decide_host_write(ctx, kind, &arguments, &folder)
                .await?;
        }
        let token = service.host_token(kind).await.map_err(host_failure)?;
        let host = match kind {
            HostKind::Github => "github.com".to_owned(),
            HostKind::Gitlab => service.current_settings().gitlab_host,
        };
        let run = service
            .run_tool(kind, &host, &token, &arguments, Some(&folder))
            .await
            .map_err(host_failure)?;
        crate::log::git_host_call(kind.name(), run.exit_code);
        let mut bytes = run.stdout;
        bytes.extend_from_slice(&run.stderr);
        bytes.truncate(crate::TOOL_OUTPUT_MAX);
        let text = String::from_utf8_lossy(&bytes).replace(&token, "***");
        Ok(ToolOutput {
            text,
            structured: Some(json!({ "exit_code": run.exit_code })),
        })
    }

    /// A form that changes the host takes the Rules Engine's decision on its command, so the
    /// project's approval settings and the agent's policy (K.1, K.3) apply as to a shell command.
    // spec: ordin-desktop/components/ProjectService#SA-RECON-10
    async fn decide_host_write(
        &self,
        ctx: &ToolCallContext,
        kind: HostKind,
        arguments: &[String],
        folder: &Path,
    ) -> Result<(), ToolFailure> {
        let rules = &self.service.rules;
        let outcome = rules
            .decide_command(CommandRequest {
                workspace_id: ctx.workspace_id,
                origin: CommandOrigin::Run {
                    run_id: ctx.run_id,
                    agent_instance_id: ctx.agent_instance_id,
                },
                command: host_forms::command_text(kind.program(), arguments),
                working_dir: folder.to_path_buf(),
                declared_check: false,
            })
            .await;
        let denied = || failed("PROJ_CHECK_DENIED", false);
        match outcome.decision.verdict {
            Verdict::Allow => Ok(()),
            Verdict::Deny { .. } => Err(denied()),
            Verdict::Ask { .. } => {
                let approval = outcome.approval_id.ok_or_else(denied)?;
                let until = tokio::time::Instant::from_std(ctx.deadline);
                match rules.wait_for_approval(approval, Some(until)).await {
                    Ok(ApprovalWait::Approved) => Ok(()),
                    Ok(ApprovalWait::Pending) => Err(failed("PROJ_CHECK_TIMEOUT", false)),
                    Ok(ApprovalWait::Rejected | ApprovalWait::Expired) | Err(_) => Err(denied()),
                }
            }
        }
    }

    async fn call_folder(
        &self,
        ctx: &ToolCallContext,
        kind: HostKind,
    ) -> Result<PathBuf, ProjectError> {
        let service = &self.service;
        if let Some(task) = ctx.coding_task_id {
            return Ok(PathBuf::from(
                service.task_row(task).await?.task.worktree_path,
            ));
        }
        for row in store::open_projects_in(&service.pool, ctx.workspace_id).await? {
            if !row.project.is_git_repository {
                continue;
            }
            let folder = service.place_folder(row.project.place_id).await?;
            if service.origin_kind(&folder).await.ok() == Some(kind) {
                return Ok(folder);
            }
        }
        Err(ProjectError::NotFound)
    }
}

#[async_trait::async_trait]
impl ToolProvider for ProjectTools {
    fn provider_id(&self) -> &'static str {
        "projects"
    }

    fn servers(&self) -> Vec<ServerSpec> {
        let mut servers = Vec::new();
        if let Ok(name) = ServerName::new("code") {
            servers.push(ServerSpec {
                name,
                tools: vec![
                    position_tool(
                        "definition",
                        "Where the symbol at a position is defined.",
                        "Find where code is defined",
                    ),
                    position_tool(
                        "references",
                        "Where the symbol at a position is used.",
                        "Find where code is used",
                    ),
                    position_tool(
                        "hover",
                        "The type and documentation of the symbol at a position.",
                        "Read what a piece of code is",
                    ),
                    path_tool(
                        "symbols",
                        "The symbols of a file, or of the workspace with no path.",
                        "List the code in a file",
                    ),
                    path_tool(
                        "diagnostics",
                        "The problems the language server reports.",
                        "Read the problems in the code",
                    ),
                ],
            });
        }
        if let Ok(name) = ServerName::new("github") {
            servers.push(ServerSpec {
                name,
                tools: vec![host_tool(
                    "gh",
                    "Runs the GitHub CLI with these arguments.",
                    "Work with GitHub",
                )],
            });
        }
        if let Ok(name) = ServerName::new("gitlab") {
            servers.push(ServerSpec {
                name,
                tools: vec![host_tool(
                    "glab",
                    "Runs the GitLab CLI with these arguments.",
                    "Work with GitLab",
                )],
            });
        }
        if let Ok(name) = ServerName::new("checks") {
            servers.push(ServerSpec {
                name,
                tools: vec![tool(
                    "run_check",
                    "Runs a check command in the coding task's worktree.",
                    json!({
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["command"],
                        "properties": {"command": {
                            "type": "string", "minLength": 1, "maxLength": MAX_COMMAND_CHARS,
                        }},
                    }),
                    r#"{"command": "cargo test"}"#,
                    "Run the project's checks",
                )],
            });
        }
        servers
    }

    // spec: ordin-desktop/components/ProjectService#SA-RECON-10
    async fn offer(&self, grant: &SessionGrant) -> Result<Vec<ToolKey>, ToolFailure> {
        let mut keys = Vec::new();
        if grant.coding_task_id.is_some() {
            for tool in [
                "definition",
                "references",
                "hover",
                "symbols",
                "diagnostics",
            ] {
                keys.extend(key("code", tool));
            }
            keys.extend(key("checks", "run_check"));
        }
        let kinds = self.workspace_hosts(grant.workspace_id).await;
        let declared = |server, tool| {
            self.service
                .rules
                .declares_tool(grant.agent_instance_id, server, tool)
        };
        if kinds.contains("github") && declared("github", "gh") {
            keys.extend(key("github", "gh"));
        }
        if kinds.contains("gitlab") && declared("gitlab", "glab") {
            keys.extend(key("gitlab", "glab"));
        }
        Ok(keys)
    }

    fn check_arguments(&self, key: &ToolKey, arguments: &Value) -> Result<(), ArgumentProblem> {
        match (key.server.as_str(), key.tool.as_str()) {
            ("code", "definition" | "references" | "hover") => {
                check_path(&parse::<PositionArgs>(arguments)?.path)
            }
            ("code", "symbols" | "diagnostics") => parse::<PathArgs>(arguments)?
                .path
                .map_or(Ok(()), |path| check_path(&path)),
            ("github" | "gitlab", _) => {
                let args = parse::<HostArgs>(arguments)?.args;
                let kind = if key.server.as_str() == "github" {
                    HostKind::Github
                } else {
                    HostKind::Gitlab
                };
                if args.is_empty()
                    || args.len() > MAX_ARGS
                    || args.iter().any(|a| a.chars().count() > MAX_ARG_CHARS)
                {
                    return Err(ArgumentProblem {
                        at: "/args".to_owned(),
                        reason: "Give 1 to 64 arguments of at most 4,000 characters.".to_owned(),
                    });
                }
                host_forms::check(kind.program(), &args)
                    .map(drop)
                    .map_err(|reason| ArgumentProblem {
                        at: "/args".to_owned(),
                        reason,
                    })
            }
            ("checks", _) => {
                let command = parse::<CheckArgs>(arguments)?.command;
                if (1..=MAX_COMMAND_CHARS).contains(&command.chars().count()) {
                    Ok(())
                } else {
                    Err(ArgumentProblem {
                        at: "/command".to_owned(),
                        reason: "Give a command of 1 to 1,000 characters.".to_owned(),
                    })
                }
            }
            _ => Err(ArgumentProblem {
                at: String::new(),
                reason: "This tool is not served here.".to_owned(),
            }),
        }
    }

    async fn call(
        &self,
        ctx: ToolCallContext,
        key: &ToolKey,
        arguments: Value,
    ) -> Result<ToolOutput, ToolFailure> {
        let unavailable = || failed("PROJ_LSP_UNAVAILABLE", true);
        let work = async {
            match key.server.as_str() {
                "code" => {
                    let task = ctx.coding_task_id.ok_or_else(unavailable)?;
                    // A language server runs the repository's code (`build.rs`, macros, plugins),
                    // so on a worktree an agent has changed it would run what the agent wrote: an
                    // agent that may not run commands gets no server there.
                    // spec: ordin-desktop/components/ProjectService#SA-RECON-11
                    if !self.service.rules.may_run_commands(ctx.agent_instance_id)
                        && self.worktree_changed(task).await
                    {
                        return Err(failed("PROJ_LSP_UNAVAILABLE", false));
                    }
                    self.code_call(task, &key.tool, &arguments)
                        .await
                        .map(output)
                        .map_err(failure)
                }
                "checks" => {
                    let task = ctx.coding_task_id.ok_or_else(unavailable)?;
                    let command = parse::<CheckArgs>(&arguments)
                        .map_err(|_| failed("PROJ_CHECK_DENIED", false))?
                        .command;
                    let origin = CommandOrigin::Run {
                        run_id: ctx.run_id,
                        agent_instance_id: ctx.agent_instance_id,
                    };
                    let result = self
                        .service
                        .run_tool_check(task, &command, origin)
                        .await
                        .map_err(failure)?;
                    serde_json::to_value(&result)
                        .map(output)
                        .map_err(|_| failed("PROJ_CHECK_DENIED", false))
                }
                "github" | "gitlab" => {
                    let kind = if key.server.as_str() == "github" {
                        HostKind::Github
                    } else {
                        HostKind::Gitlab
                    };
                    let args = parse::<HostArgs>(&arguments)
                        .map_err(|_| failed("GH_FAILED", false))?
                        .args;
                    self.host_call(&ctx, kind, args).await
                }
                _ => Err(unavailable()),
            }
        };
        let deadline = tokio::time::Instant::from_std(ctx.deadline);
        let gave_up = || match key.server.as_str() {
            "code" => unavailable(),
            "checks" => failed("PROJ_CHECK_TIMEOUT", false),
            _ => failed("GH_UNREACHABLE", true),
        };
        tokio::select! {
            result = work => result,
            _ = ctx.cancel.cancelled() => Err(gave_up()),
            _ = tokio::time::sleep_until(deadline) => Err(gave_up()),
        }
    }

    /// No per-session state is kept: instances live with their coding task.
    async fn session_closed(&self, _run_id: RunId) {}
}
