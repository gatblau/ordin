//! `CraftAppsProvider`: the one provider of the `craft_*` servers (CraftToolkit.md *Internal Logic*
//! 5 and 6).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use ordin_files::{FileVersionOrigin, WriteVersion};
use ordin_mcp_front::{
    ArgumentProblem, ServerName, ServerSpec, SessionGrant, ToolCallContext, ToolFailure, ToolKey,
    ToolOutput, ToolProvider, ToolSpec,
};
use ordin_rules::PathAccessRequest;
use ordin_types::domain::places::FolderAccess;
use ordin_types::{PlaceId, RunId};
use rmcp::model::{CallToolRequestParams, CallToolResult};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::definitions::{CraftAppDefinition, CraftToolDefinition, PathRole};
use crate::error::CraftError;
use crate::filetypes::allowed_by;
use crate::model::CraftProfile;
use crate::profile::display_name;
use crate::servers::{Owner, ServerHandle};
use crate::toolkit::{lock, CraftToolkit};

const SERVER_PREFIX: &str = "craft_";
/// The most of an app's error message passed to the agent (6.3).
const MAX_APP_MESSAGE: usize = 2_000;

/// The one provider returned by `tool_provider()`: prefix `craft_`, one server `craft_<app>` per
/// installed app with MCP mode and a definition (SA-NAME-04, SA-RECON-01).
// spec: ordin-desktop/components/CraftToolkit#SA-RECON-01
pub struct CraftAppsProvider {
    toolkit: Arc<CraftToolkit>,
}

impl CraftAppsProvider {
    pub(crate) fn new(toolkit: Arc<CraftToolkit>) -> CraftAppsProvider {
        CraftAppsProvider { toolkit }
    }
}

/// One output argument, after 6.2.
struct Output {
    place_id: PlaceId,
    relative: String,
    original: String,
    scratch: PathBuf,
}

/// Why a tool call failed; `app_message` is the app's own text, passed on as data.
pub(crate) struct CallFailure {
    pub(crate) error: CraftError,
    pub(crate) app_message: Option<String>,
}

impl From<CraftError> for CallFailure {
    fn from(error: CraftError) -> Self {
        CallFailure {
            error,
            app_message: None,
        }
    }
}

impl CallFailure {
    // 6.3 passes the app's message "cut to 2,000 chars"; `ToolFailure::message` allows 1–2,000.
    fn into_failure(self) -> ToolFailure {
        let message = match (&self.error, self.app_message) {
            (CraftError::ToolFailed, Some(text)) if !text.is_empty() => {
                text.chars().take(MAX_APP_MESSAGE).collect()
            }
            _ => self.error.sentence(),
        };
        ToolFailure {
            code: self.error.code().to_owned(),
            message,
            retryable: self.error.retryable(),
        }
    }
}

/// The text of an app's error result.
fn message_of(result: &CallToolResult) -> String {
    let text: Vec<String> = result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|t| t.text.clone()))
        .collect();
    text.join("\n")
}

/// Scratch paths in `value` become the original output paths (6.5).
fn replace_paths(value: Value, paths: &[(String, String)]) -> Value {
    match value {
        Value::String(text) => Value::String(replace_text(text, paths)),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| replace_paths(item, paths))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, item)| (key, replace_paths(item, paths)))
                .collect(),
        ),
        other => other,
    }
}

fn replace_text(mut text: String, paths: &[(String, String)]) -> String {
    for (scratch, original) in paths {
        text = text.replace(scratch, original);
    }
    text
}

fn digest_of(arguments: &Value) -> String {
    let digest: [u8; 32] = Sha256::digest(arguments.to_string().as_bytes()).into();
    ordin_types::num::Sha256Hex::from_digest(&digest).0
}

/// A definition's path arguments as the form quoted to the model on a wrong call.
fn argument_form(tool: &CraftToolDefinition) -> String {
    let names: Vec<String> = tool
        .path_args
        .iter()
        .map(|arg| format!("{}: full path", arg.name))
        .collect();
    let form = format!("{}({})", tool.tool, names.join(", "));
    form.chars().take(300).collect()
}

impl CraftToolkit {
    /// Apps with a `craft_<app>` server: installed, `Mcp` mode, a definition, and a name the MCP
    /// Front accepts.
    pub(crate) fn served_apps(&self) -> Vec<(String, ServerName)> {
        let rows: Vec<_> = lock(&self.state).rows.values().cloned().collect();
        rows.iter()
            .filter(|row| row.is_installed() && row.has_mcp())
            .filter(|row| self.installed_definition(row).is_some())
            .filter_map(|row| {
                let name = ServerName::new(format!("{SERVER_PREFIX}{}", row.app_name)).ok()?;
                Some((row.app_name.clone(), name))
            })
            .collect()
    }

    pub(crate) fn server_specs(&self) -> Vec<ServerSpec> {
        let schemas = lock(&self.state).schemas.clone();
        self.served_apps()
            .into_iter()
            .filter_map(|(app, name)| {
                let row = self.row(&app)?;
                let definition = self.installed_definition(&row)?;
                let tools = definition
                    .tools
                    .iter()
                    .map(|tool| ToolSpec {
                        name: tool.tool.clone(),
                        description: tool.description.clone(),
                        input_schema: schemas
                            .get(&(app.clone(), tool.tool.clone()))
                            .cloned()
                            .unwrap_or_else(|| json!({ "type": "object" })),
                        argument_form: argument_form(tool),
                        sandbox_callable: false,
                        plain: format!("{} can change files for this agent.", display_name(&app)),
                    })
                    .collect();
                Some(ServerSpec { name, tools })
            })
            .collect()
    }

    /// The tools of `definition` this profile allows; none for an app the profile does not pin.
    pub(crate) fn allowed_tools<'a>(
        profile: &CraftProfile,
        app: &str,
        definition: &'a CraftAppDefinition,
    ) -> Vec<&'a CraftToolDefinition> {
        let Some(pin) = profile.apps.iter().find(|pin| pin.name == app) else {
            return Vec::new();
        };
        definition
            .tools
            .iter()
            .filter(|tool| allowed_by(pin.allowed_tools.as_deref(), &tool.tool))
            .collect()
    }

    /// Internal Logic 5.3.
    // spec: ordin-desktop/components/CraftToolkit#SA-RECON-05
    pub(crate) async fn offer_tools(self: &Arc<Self>, run_id: RunId) -> Vec<ToolKey> {
        let profile = self.craft_profile();
        let deadline = tokio::time::Instant::now() + self.limits.offer;
        let mut starts = Vec::new();
        for (app, name) in self.served_apps() {
            let toolkit = Arc::clone(self);
            let task_app = app.clone();
            let task = tokio::spawn(async move {
                toolkit
                    .server(Owner::Run(run_id), &task_app)
                    .await
                    .map(|handle| Arc::clone(&handle.tools))
            });
            starts.push((app, name, task));
        }
        let mut keys = Vec::new();
        for (app, server, mut task) in starts {
            let Some(row) = self.row(&app) else { continue };
            let Some(definition) = self.installed_definition(&row) else {
                continue;
            };
            let allowed = Self::allowed_tools(&profile, &app, definition);
            let listed = match tokio::time::timeout_at(deadline, &mut task).await {
                Ok(Ok(Ok(tools))) => Some(tools),
                Err(_) => None,
                Ok(Ok(Err(error))) => {
                    tracing::warn!(component = "craft", app = %app, code = error.code(), "craft_offer_failed");
                    continue;
                }
                Ok(Err(_)) => continue,
            };
            for tool in allowed {
                let in_list = listed
                    .as_ref()
                    .is_none_or(|tools| tools.iter().any(|t| t.name == tool.tool.as_str()));
                if in_list {
                    keys.push(ToolKey {
                        server: server.clone(),
                        tool: tool.tool.clone(),
                    });
                }
            }
        }
        keys
    }

    /// Step 6.1: everything that decides whether this tool may run for this session.
    async fn offered_tool(
        &self,
        run_id: RunId,
        app: &str,
        tool: &str,
    ) -> Result<(CraftToolDefinition, ServerHandle), CraftError> {
        let profile = self.craft_profile();
        if !profile.apps.iter().any(|pin| pin.name == app) {
            return Err(CraftError::NotApproved);
        }
        let row = self
            .row(app)
            .filter(|row| row.is_installed())
            .ok_or(CraftError::NotInstalled)?;
        if !row.has_mcp() {
            return Err(CraftError::NoMcpMode);
        }
        let definition = self
            .installed_definition(&row)
            .ok_or(CraftError::ToolNotOffered)?;
        let found = Self::allowed_tools(&profile, app, definition)
            .into_iter()
            .find(|t| t.tool == tool)
            .cloned()
            .ok_or(CraftError::ToolNotOffered)?;
        let handle = self.server(Owner::Run(run_id), app).await?;
        if !handle.lists(tool) {
            return Err(CraftError::ToolNotOffered);
        }
        Ok((found, handle))
    }

    /// Step 6.2: the calling agent's folder grants, for the file `locate` found. The Rules Engine's
    /// path rules at the MCP Front cover the built-in file tools only, and `locate` searches every
    /// place, so this is the only check of a Craft tool's paths. The place's root and `relative` are
    /// the location with links followed: a link inside a grant answers for its target.
    async fn check_agent_path(
        &self,
        ctx: &ToolCallContext,
        place_id: PlaceId,
        relative: &str,
        role: PathRole,
    ) -> Result<(), CraftError> {
        let place = self
            .files
            .place(place_id)
            .await
            .map_err(|_| CraftError::FilesFailed)?;
        self.path_access(&PathAccessRequest {
            workspace_id: Some(ctx.workspace_id),
            agent: Some(ctx.agent_instance_id),
            crew_charter_id: None,
            path: Path::new(&place.path).join(relative),
            access: match role {
                PathRole::Input => FolderAccess::Read,
                PathRole::Output => FolderAccess::ReadAndChange,
            },
        })
    }

    /// Step 6.2: checks the path arguments and replaces each output by a scratch path.
    async fn map_paths(
        &self,
        ctx: &ToolCallContext,
        tool: &CraftToolDefinition,
        arguments: &mut Value,
        outputs: &mut Vec<Output>,
    ) -> Result<(), CraftError> {
        for arg in &tool.path_args {
            let path = arguments
                .get(&arg.name)
                .and_then(Value::as_str)
                .filter(|p| Path::new(p).is_absolute())
                .ok_or(CraftError::ArgumentsInvalid)?
                .to_owned();
            let located = self
                .files
                .locate(Path::new(&path))
                .await
                .map_err(|_| CraftError::FilesFailed)?;
            let (place_id, relative) = located.ok_or(CraftError::PathNotAllowed)?;
            self.check_agent_path(ctx, place_id, &relative, arg.role)
                .await?;
            if arg.role == PathRole::Output {
                let id = uuid::Uuid::new_v4();
                let name = match Path::new(&path).extension().and_then(|e| e.to_str()) {
                    Some(extension) => format!("{id}.{extension}"),
                    None => id.to_string(),
                };
                let scratch = self.owner_folder(Owner::Run(ctx.run_id)).join(name);
                arguments[arg.name.as_str()] = json!(scratch.to_string_lossy());
                outputs.push(Output {
                    place_id,
                    relative,
                    original: path,
                    scratch,
                });
            }
        }
        Ok(())
    }

    /// Step 6.3. The limit is `CALL_TIMEOUT`, or the MCP Front's deadline when that comes first; a
    /// cancelled call ends the same way.
    async fn run_app_tool(
        &self,
        owner: Owner,
        app: &str,
        handle: &ServerHandle,
        params: CallToolRequestParams,
        limit: Duration,
        cancel: &CancellationToken,
    ) -> Result<CallToolResult, CallFailure> {
        let outcome = tokio::select! {
            result = tokio::time::timeout(limit, handle.peer.call_tool(params)) => result.ok(),
            () = cancel.cancelled() => None,
        };
        let Some(outcome) = outcome else {
            self.kill_server(owner, app, "timeout").await;
            return Err(CraftError::Timeout.into());
        };
        match outcome {
            Ok(result) if result.is_error != Some(true) => Ok(result),
            Ok(result) => Err(CallFailure {
                error: CraftError::ToolFailed,
                app_message: Some(message_of(&result)),
            }),
            Err(error) => Err(CallFailure {
                error: CraftError::ToolFailed,
                app_message: Some(error.to_string()),
            }),
        }
    }

    /// Reads a tool's output file (6.4): it must exist, be non-empty and fit `MAX_OUTPUT_BYTES`.
    pub(crate) async fn read_output(&self, path: &Path) -> Result<Vec<u8>, CraftError> {
        let meta = tokio::fs::metadata(path)
            .await
            .map_err(|_| CraftError::OutputMissing)?;
        if meta.len() == 0 {
            return Err(CraftError::OutputMissing);
        }
        if meta.len() > self.limits.max_output {
            return Err(CraftError::OutputTooLarge);
        }
        tokio::fs::read(path)
            .await
            .map_err(|_| CraftError::OutputMissing)
    }

    /// Steps 6.1 to 6.5.
    async fn run_tool(
        &self,
        ctx: &ToolCallContext,
        app: &str,
        tool: &str,
        mut arguments: Value,
        outputs: &mut Vec<Output>,
    ) -> Result<ToolOutput, CallFailure> {
        let (definition, handle) = self.offered_tool(ctx.run_id, app, tool).await?;
        self.map_paths(ctx, &definition, &mut arguments, outputs)
            .await?;
        let mut params = CallToolRequestParams::new(tool.to_owned());
        if let Value::Object(map) = arguments {
            params = params.with_arguments(map);
        }
        let limit = self
            .limits
            .call
            .min(ctx.deadline.saturating_duration_since(Instant::now()));
        let paths: Vec<(String, String)> = outputs
            .iter()
            .map(|o| (o.scratch.to_string_lossy().into_owned(), o.original.clone()))
            .collect();
        let owner = Owner::Run(ctx.run_id);
        let result = self
            .run_app_tool(owner, app, &handle, params, limit, &ctx.cancel)
            .await
            .map_err(|mut failure| {
                failure.app_message = failure.app_message.map(|m| replace_text(m, &paths));
                failure
            })?;
        drop(handle);
        // All outputs are checked before any is written: no partial result replaces a version.
        let mut bytes = Vec::with_capacity(outputs.len());
        for output in outputs.iter() {
            bytes.push(self.read_output(&output.scratch).await?);
        }
        let mut written = Vec::with_capacity(outputs.len());
        for (output, bytes) in outputs.iter().zip(bytes) {
            let version = self
                .files
                .write_version(WriteVersion {
                    place_id: output.place_id,
                    path: output.relative.clone(),
                    bytes,
                    origin: FileVersionOrigin::Run {
                        run_id: ctx.run_id,
                        tool: format!("{SERVER_PREFIX}{app}.{tool}"),
                    },
                    expected_hash: None,
                })
                .await
                .map_err(|_| CraftError::FilesFailed)?;
            written.push(json!({ "path": output.relative, "version": version.version }));
        }
        let app_result = serde_json::to_value(&result).unwrap_or(Value::Null);
        let structured = json!({
            "written": written,
            "app_result": replace_paths(app_result, &paths),
        });
        Ok(ToolOutput {
            text: structured.to_string(),
            structured: Some(structured),
        })
    }

    /// `CraftAppsProvider::call`, with step 6.6.
    pub(crate) async fn call_tool(
        &self,
        ctx: ToolCallContext,
        key: &ToolKey,
        arguments: Value,
    ) -> Result<ToolOutput, ToolFailure> {
        let started = Instant::now();
        let app = key
            .server
            .as_str()
            .strip_prefix(SERVER_PREFIX)
            .unwrap_or(key.server.as_str())
            .to_owned();
        let digest = digest_of(&arguments);
        let mut outputs = Vec::new();
        let outcome = self
            .run_tool(&ctx, &app, &key.tool, arguments, &mut outputs)
            .await;
        for output in &outputs {
            let _ = tokio::fs::remove_file(&output.scratch).await;
        }
        let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let result = outcome
            .as_ref()
            .map_or_else(|failure| failure.error.code(), |_| "ok");
        tracing::info!(
            component = "craft",
            app = %app,
            tool = %key.tool,
            run_id = %ctx.run_id,
            outputs = outputs.len(),
            elapsed_ms,
            result,
            "craft_tool_called"
        );
        self.record(
            "craft_tool_called",
            Some(ctx.run_id),
            format!("{} ran a tool for an agent.", display_name(&app)),
            json!({
                "app": app,
                "tool": key.tool,
                "run_id": ctx.run_id.to_string(),
                "outputs": outputs.len(),
                "elapsed_ms": elapsed_ms,
                "result": result,
                "arguments_digest": digest,
            }),
        )
        .await;
        outcome.map_err(CallFailure::into_failure)
    }

    /// The path arguments of a defined tool, for `check_arguments`.
    fn path_args_of(&self, key: &ToolKey) -> Vec<String> {
        let app = key.server.as_str().strip_prefix(SERVER_PREFIX);
        let definition = app
            .and_then(|a| self.row(a))
            .and_then(|row| self.installed_definition(&row).cloned());
        definition
            .and_then(|d| d.tools.into_iter().find(|t| t.tool == key.tool))
            .map(|tool| tool.path_args.into_iter().map(|a| a.name).collect())
            .unwrap_or_default()
    }
}

#[async_trait]
impl ToolProvider for CraftAppsProvider {
    fn provider_id(&self) -> &'static str {
        "craft-toolkit"
    }

    fn servers(&self) -> Vec<ServerSpec> {
        self.toolkit.server_specs()
    }

    fn server_prefix(&self) -> Option<&'static str> {
        Some(SERVER_PREFIX)
    }

    async fn offer(&self, grant: &SessionGrant) -> Result<Vec<ToolKey>, ToolFailure> {
        Ok(self.toolkit.offer_tools(grant.run_id).await)
    }

    /// The type check of 6.2. The check of the places needs the File Service and stays in `call`.
    fn check_arguments(&self, key: &ToolKey, arguments: &Value) -> Result<(), ArgumentProblem> {
        for name in self.toolkit.path_args_of(key) {
            let absolute = arguments
                .get(&name)
                .and_then(Value::as_str)
                .is_some_and(|p| Path::new(p).is_absolute());
            if !absolute {
                return Err(ArgumentProblem {
                    at: format!("/{name}"),
                    reason: CraftError::ArgumentsInvalid.sentence(),
                });
            }
        }
        Ok(())
    }

    async fn call(
        &self,
        ctx: ToolCallContext,
        key: &ToolKey,
        arguments: Value,
    ) -> Result<ToolOutput, ToolFailure> {
        self.toolkit.call_tool(ctx, key, arguments).await
    }

    async fn session_closed(&self, run_id: RunId) {
        self.toolkit.close_session(run_id).await;
    }
}
