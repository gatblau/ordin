//! `CliCopilotPort`: the production `CopilotPort`, which talks to the person's Copilot CLI itself,
//! as the Claude Local Adapter talks to Claude Code: ordin starts the CLI as a child process and owns
//! the protocol on its standard streams. No SDK is linked.
//!
//! The CLI runs `--server --stdio --no-auto-update` with the environment the adapter built and
//! nothing inherited (2D-1c). It speaks JSON-RPC 2.0, each message framed by a `Content-Length`
//! header. What ordin sends and answers:
//!
//! | Port item | Wire |
//! |---|---|
//! | `start_server` | spawn, then `connect` (`ping` for a CLI without it) |
//! | `auth_status` | request `auth.getStatus` → `isAuthenticated`, `authType` |
//! | `start_login` | the CLI's own `copilot login`, as a separate child process (SA-CLOSE-01 last row) |
//! | `create_session` / `resume_session` | request `session.create` / `session.resume` with `requestPermission` and `hooks` on |
//! | `send` / `abort` | requests `session.send` (`prompt`) / `session.abort` |
//! | events | notifications `session.event` (`sessionId`, `event.type`, `event.data`) |
//! | permission gate | event `permission.requested` → request `session.permissions.handlePendingPermissionRequest` |
//! | pre- and post-tool-use | server request `hooks.invoke` (`preToolUse`, `postToolUse`, `postToolUseFailure`) → `{ "output": … }` |
//! | `stop` | standard input closed; the child is killed after the deadline |

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::stream::{BoxStream, StreamExt};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};

use crate::port::{
    CliServerSpec, CopilotAuthStatus, CopilotEvent, CopilotPort, CopilotServer, GateAnswer,
    PortError, SessionOptions, ToolGate, ToolObserver,
};

/// How long one request waits for its reply (the turn itself is streamed, not awaited).
const CALL_TIMEOUT: Duration = Duration::from_secs(60);
/// The largest message ordin reads (a peer's `Content-Length` is a claim, not a size received).
const MAX_MESSAGE: usize = 64 * 1024 * 1024;
/// JSON-RPC "method not found".
const METHOD_NOT_FOUND: i64 = -32601;

/// The production `CopilotPort` (*Public Interface*: the program passes it to `new`).
#[derive(Debug, Default, Clone)]
pub struct CliCopilotPort;

impl CliCopilotPort {
    pub fn new() -> CliCopilotPort {
        CliCopilotPort
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn port_error(error: impl std::fmt::Display) -> PortError {
    PortError::new(error.to_string())
}

#[async_trait::async_trait]
impl CopilotPort for CliCopilotPort {
    async fn start_server(&self, spec: CliServerSpec) -> Result<Box<dyn CopilotServer>, PortError> {
        let mut child = Command::new(&spec.cli_path)
            .args(["--server", "--stdio", "--no-auto-update"])
            .env_clear()
            .envs(&spec.env)
            .current_dir(&spec.working_folder)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(port_error)?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(PortError::new("the CLI server's streams are missing"));
        };
        let connection = Connection::open(stdout, stdin);
        match connection.call("connect", json!({})).await {
            Ok(_) => {}
            Err(CallError::Refused { code, .. }) if code == METHOD_NOT_FOUND => {
                connection
                    .call("ping", json!({}))
                    .await
                    .map_err(port_error)?;
            }
            Err(error) => return Err(port_error(error)),
        }
        Ok(Box::new(CliServer {
            connection,
            child: tokio::sync::Mutex::new(child),
            cli_path: spec.cli_path,
            env: spec.env,
            working_folder: spec.working_folder,
        }))
    }
}

// ---- the connection ----

#[derive(Debug, thiserror::Error)]
enum CallError {
    #[error("the CLI server refused {method}: {message}")]
    Refused {
        method: String,
        code: i64,
        message: String,
    },
    #[error("the CLI server did not answer {0}")]
    NoAnswer(String),
    #[error("the CLI server is gone")]
    Closed,
}

/// The session's gates, as the reader task needs them to answer the CLI's own requests.
struct SessionRoute {
    events: mpsc::UnboundedSender<(String, Value)>,
    pre_tool_use: Arc<dyn ToolGate>,
    permission: Arc<dyn ToolGate>,
    post_tool_use: Arc<dyn ToolObserver>,
    /// The event receiver, until `events` takes it.
    pending: Option<mpsc::UnboundedReceiver<(String, Value)>>,
}

struct Shared {
    writer: tokio::sync::Mutex<Box<dyn AsyncWrite + Send + Unpin>>,
    next_id: AtomicU64,
    waiting: Mutex<HashMap<u64, oneshot::Sender<Result<Value, CallError>>>>,
    sessions: Mutex<HashMap<String, SessionRoute>>,
    closed: Mutex<bool>,
}

#[derive(Clone)]
struct Connection(Arc<Shared>);

impl Connection {
    fn open(
        reader: impl AsyncRead + Send + Unpin + 'static,
        writer: impl AsyncWrite + Send + Unpin + 'static,
    ) -> Connection {
        let connection = Connection(Arc::new(Shared {
            writer: tokio::sync::Mutex::new(Box::new(writer)),
            next_id: AtomicU64::new(1),
            waiting: Mutex::new(HashMap::new()),
            sessions: Mutex::new(HashMap::new()),
            closed: Mutex::new(false),
        }));
        tokio::spawn(connection.clone().read_loop(BufReader::new(reader)));
        connection
    }

    async fn write(&self, message: &Value) -> Result<(), CallError> {
        let body = serde_json::to_vec(message).map_err(|_| CallError::Closed)?;
        let mut writer = self.0.writer.lock().await;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        writer
            .write_all(header.as_bytes())
            .await
            .map_err(|_| CallError::Closed)?;
        writer
            .write_all(&body)
            .await
            .map_err(|_| CallError::Closed)?;
        writer.flush().await.map_err(|_| CallError::Closed)
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, CallError> {
        if *lock(&self.0.closed) {
            return Err(CallError::Closed);
        }
        let id = self.0.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        lock(&self.0.waiting).insert(id, tx);
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if let Err(error) = self.write(&message).await {
            lock(&self.0.waiting).remove(&id);
            return Err(error);
        }
        match tokio::time::timeout(CALL_TIMEOUT, rx).await {
            Ok(Ok(result)) => result.map_err(|error| match error {
                CallError::Refused { code, message, .. } => CallError::Refused {
                    method: method.to_owned(),
                    code,
                    message,
                },
                other => other,
            }),
            Ok(Err(_)) => Err(CallError::Closed),
            Err(_) => {
                lock(&self.0.waiting).remove(&id);
                Err(CallError::NoAnswer(method.to_owned()))
            }
        }
    }

    async fn reply(&self, id: Value, result: Result<Value, (i64, &str)>) {
        let message = match result {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err((code, text)) => {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": text}})
            }
        };
        let _ = self.write(&message).await;
    }

    /// Reads messages until the CLI's output ends; then every call and event stream ends too.
    async fn read_loop(self, mut reader: BufReader<impl AsyncRead + Send + Unpin>) {
        while let Some(message) = read_message(&mut reader).await {
            self.dispatch(message);
        }
        *lock(&self.0.closed) = true;
        for (_, waiter) in lock(&self.0.waiting).drain() {
            let _ = waiter.send(Err(CallError::Closed));
        }
        lock(&self.0.sessions).clear();
    }

    fn dispatch(&self, message: Value) {
        let method = message.get("method").and_then(Value::as_str);
        let id = message.get("id").cloned().filter(|id| !id.is_null());
        match (method, id) {
            // A reply to one of ordin's requests.
            (None, Some(id)) => {
                let Some(waiter) = id.as_u64().and_then(|id| lock(&self.0.waiting).remove(&id))
                else {
                    return;
                };
                let result = match message.get("error") {
                    Some(error) => Err(CallError::Refused {
                        method: String::new(),
                        code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
                        message: text(error, "message").unwrap_or_default(),
                    }),
                    None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = waiter.send(result);
            }
            // A request from the CLI: answered on its own task, so reading never waits for a gate.
            (Some(method), Some(id)) => {
                let connection = self.clone();
                let method = method.to_owned();
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                tokio::spawn(async move { connection.answer(&method, id, params).await });
            }
            (Some("session.event"), None) => {
                let params = message.get("params").cloned().unwrap_or(Value::Null);
                self.session_event(params);
            }
            _ => {}
        }
    }

    fn session_event(&self, params: Value) {
        let Some(session_id) = text(&params, "sessionId") else {
            return;
        };
        let event = params.get("event").cloned().unwrap_or(Value::Null);
        let Some(kind) = text(&event, "type") else {
            return;
        };
        let data = event.get("data").cloned().unwrap_or(Value::Null);
        if kind == "permission.requested"
            && !data
                .get("resolvedByHook")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            let connection = self.clone();
            let session = session_id.clone();
            let request = data.clone();
            tokio::spawn(async move { connection.decide_permission(&session, request).await });
        }
        if let Some(route) = lock(&self.0.sessions).get(&session_id) {
            let _ = route.events.send((kind, data));
        }
    }

    /// `permission.requested`: the gate decides, and the decision goes back to the CLI.
    async fn decide_permission(&self, session_id: &str, data: Value) {
        let Some(request_id) = text(&data, "requestId") else {
            return;
        };
        let Some(gate) = lock(&self.0.sessions)
            .get(session_id)
            .map(|route| route.permission.clone())
        else {
            return;
        };
        let request = data.get("permissionRequest").unwrap_or(&data);
        let tool = permission_tool(request);
        let tool_use_id = text(request, "toolCallId");
        let result = match gate.decide(&tool, request.clone(), tool_use_id).await {
            GateAnswer::Allow => json!({"kind": "approve-once"}),
            GateAnswer::Deny { reason } => json!({"kind": "reject", "feedback": reason}),
        };
        let params = json!({"sessionId": session_id, "requestId": request_id, "result": result});
        let _ = self
            .call("session.permissions.handlePendingPermissionRequest", params)
            .await;
    }

    /// A request from the CLI. Only `hooks.invoke` is offered; anything else is refused.
    async fn answer(&self, method: &str, id: Value, params: Value) {
        if method != "hooks.invoke" {
            self.reply(id, Err((METHOD_NOT_FOUND, "not offered"))).await;
            return;
        }
        let route = text(&params, "sessionId").and_then(|session| {
            lock(&self.0.sessions)
                .get(&session)
                .map(|r| (r.pre_tool_use.clone(), r.post_tool_use.clone()))
        });
        let hook = text(&params, "hookType").unwrap_or_default();
        let input = params.get("input").cloned().unwrap_or(Value::Null);
        let tool = text(&input, "toolName").unwrap_or_default();
        let output = match (route, hook.as_str()) {
            (Some((gate, _)), "preToolUse") => {
                let arguments = input.get("toolArgs").cloned().unwrap_or(Value::Null);
                match gate
                    .decide(&tool, arguments, text(&input, "toolCallId"))
                    .await
                {
                    GateAnswer::Allow => json!({"permissionDecision": "allow"}),
                    GateAnswer::Deny { reason } => json!({
                        "permissionDecision": "deny",
                        "permissionDecisionReason": reason,
                    }),
                }
            }
            (Some((_, observer)), "postToolUse" | "postToolUseFailure") => {
                let succeeded = hook == "postToolUse";
                observer
                    .tool_used(&tool, text(&input, "toolCallId"), succeeded)
                    .await;
                json!({})
            }
            // A pre-tool-use request for a session ordin does not know is denied (fail closed).
            (None, "preToolUse") => json!({
                "permissionDecision": "deny",
                "permissionDecisionReason": crate::gate::COULD_NOT_CHECK,
            }),
            _ => json!({}),
        };
        self.reply(id, Ok(json!({ "output": output }))).await;
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// One `Content-Length`-framed message; `None` at the end of the stream or on a broken frame.
async fn read_message(reader: &mut BufReader<impl AsyncRead + Unpin>) -> Option<Value> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await.ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse::<usize>().ok();
            }
        }
    }
    let length = length.filter(|n| *n <= MAX_MESSAGE)?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body).await.ok()?;
    serde_json::from_slice(&body).ok()
}

/// The tool a permission request is about: an MCP tool as `mcp__<server>__<tool>`, otherwise the
/// request's tool name, otherwise its kind.
fn permission_tool(request: &Value) -> String {
    match (text(request, "serverName"), text(request, "toolName")) {
        (Some(server), Some(tool)) => format!("mcp__{server}__{tool}"),
        (None, Some(tool)) => tool,
        _ => text(request, "kind").unwrap_or_else(|| "unknown".to_owned()),
    }
}

// ---- the server ----

struct CliServer {
    connection: Connection,
    child: tokio::sync::Mutex<Child>,
    cli_path: PathBuf,
    env: BTreeMap<String, String>,
    working_folder: PathBuf,
}

impl CliServer {
    /// Registers the session's route before the CLI can send its first event or request.
    fn route(&self, session_id: &str, opts: &SessionOptions) {
        let (events, pending) = mpsc::unbounded_channel();
        lock(&self.connection.0.sessions).insert(
            session_id.to_owned(),
            SessionRoute {
                events,
                pre_tool_use: opts.pre_tool_use.clone(),
                permission: opts.permission.clone(),
                post_tool_use: opts.post_tool_use.clone(),
                pending: Some(pending),
            },
        );
    }

    fn session_params(session_id: &str, opts: &SessionOptions) -> Value {
        let mut servers = opts
            .mcp_servers
            .get("mcpServers")
            .cloned()
            .unwrap_or_else(|| json!({}));
        // Every server's tools are offered: the CLI's own examples always name `"tools": ["*"]`,
        // and a server registered without it was offered no tool by CLI 1.0.114 (plan 08 Phase 8).
        // A `tools` list already given is kept.
        if let Some(servers) = servers.as_object_mut() {
            for server in servers.values_mut().filter_map(Value::as_object_mut) {
                server.entry("tools").or_insert_with(|| json!(["*"]));
            }
        }
        let mut params = json!({
            "sessionId": session_id,
            "mcpServers": servers,
            "requestPermission": true,
            "hooks": true,
            "envValueMode": "direct",
        });
        if let Some(model) = &opts.model {
            params["model"] = json!(model);
        }
        params
    }
}

#[async_trait::async_trait]
impl CopilotServer for CliServer {
    async fn auth_status(&self) -> Result<CopilotAuthStatus, PortError> {
        let status = self
            .connection
            .call("auth.getStatus", json!({}))
            .await
            .map_err(port_error)?;
        Ok(CopilotAuthStatus {
            signed_in: status
                .get("isAuthenticated")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            method: text(&status, "authType"),
        })
    }

    async fn start_login(&self) -> Result<(), PortError> {
        // No standard input and no terminal (C-25, Copilot half open); the child is reaped when it
        // ends, whatever the person does.
        let mut login = Command::new(&self.cli_path)
            .arg("login")
            .env_clear()
            .envs(&self.env)
            .current_dir(&self.working_folder)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(port_error)?;
        tokio::spawn(async move {
            let _ = login.wait().await;
        });
        Ok(())
    }

    async fn create_session(&self, opts: SessionOptions) -> Result<String, PortError> {
        let session_id = uuid::Uuid::now_v7().to_string();
        self.route(&session_id, &opts);
        let params = CliServer::session_params(&session_id, &opts);
        if let Err(error) = self.connection.call("session.create", params).await {
            lock(&self.connection.0.sessions).remove(&session_id);
            return Err(port_error(error));
        }
        Ok(session_id)
    }

    async fn resume_session(
        &self,
        session_id: &str,
        opts: SessionOptions,
    ) -> Result<(), PortError> {
        self.route(session_id, &opts);
        let params = CliServer::session_params(session_id, &opts);
        if let Err(error) = self.connection.call("session.resume", params).await {
            lock(&self.connection.0.sessions).remove(session_id);
            return Err(port_error(error));
        }
        Ok(())
    }

    async fn send(&self, session_id: &str, text: &str) -> Result<(), PortError> {
        let params = json!({"sessionId": session_id, "prompt": text});
        self.connection
            .call("session.send", params)
            .await
            .map(|_| ())
            .map_err(port_error)
    }

    fn events(&self, session_id: &str) -> BoxStream<'static, CopilotEvent> {
        let pending = lock(&self.connection.0.sessions)
            .get_mut(session_id)
            .and_then(|route| route.pending.take());
        let Some(receiver) = pending else {
            return futures::stream::iter([CopilotEvent::ServerExited { status: None }]).boxed();
        };
        futures::stream::unfold(
            (receiver, EventState::default(), false),
            |(mut receiver, mut state, done)| async move {
                if done {
                    return None;
                }
                loop {
                    match receiver.recv().await {
                        Some((kind, data)) => {
                            if let Some(event) = state.map(&kind, &data) {
                                return Some((event, (receiver, state, false)));
                            }
                        }
                        // The route is gone: the CLI's output ended.
                        None => {
                            return Some((
                                CopilotEvent::ServerExited { status: None },
                                (receiver, state, true),
                            ))
                        }
                    }
                }
            },
        )
        .boxed()
    }

    async fn abort(&self, session_id: &str) -> Result<(), PortError> {
        self.connection
            .call("session.abort", json!({"sessionId": session_id}))
            .await
            .map(|_| ())
            .map_err(port_error)
    }

    async fn stop(&self, deadline: Duration) {
        // Closing standard input asks the CLI to end; it is killed if it has not after `deadline`.
        let _ = self.connection.0.writer.lock().await.shutdown().await;
        let mut child = self.child.lock().await;
        if tokio::time::timeout(deadline, child.wait()).await.is_err() {
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        lock(&self.connection.0.sessions).clear();
    }
}

/// What the event mapping remembers between events: each tool call's name (the completion carries
/// only its id) and the assistant's last message (the turn's final text).
#[derive(Default)]
struct EventState {
    tools: HashMap<String, String>,
    last_message: Option<String>,
}

fn count_of(data: &Value, key: &str) -> Option<u64> {
    data.get(key).and_then(Value::as_u64)
}

impl EventState {
    fn map(&mut self, kind: &str, data: &Value) -> Option<CopilotEvent> {
        match kind {
            "session.start" => Some(CopilotEvent::SessionStarted),
            "assistant.message" => {
                let text = text(data, "content")?;
                self.last_message = Some(text.clone());
                Some(CopilotEvent::AssistantText { text })
            }
            "tool.execution_start" => {
                let tool = text(data, "toolName")?;
                if let Some(id) = text(data, "toolCallId") {
                    self.tools.insert(id, tool.clone());
                }
                Some(CopilotEvent::ToolStarted { tool })
            }
            "tool.execution_complete" => {
                let tool = self.tools.remove(&text(data, "toolCallId")?)?;
                let succeeded = data
                    .get("success")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                Some(CopilotEvent::ToolFinished { tool, succeeded })
            }
            "assistant.usage" => Some(CopilotEvent::Usage {
                requests: Some(1),
                input_tokens: count_of(data, "inputTokens"),
                output_tokens: count_of(data, "outputTokens"),
                cached_input_tokens: count_of(data, "cacheReadTokens"),
            }),
            "session.idle" => Some(CopilotEvent::TurnEnded {
                final_text: self.last_message.take(),
            }),
            "session.error" => {
                let message = text(data, "message").unwrap_or_default();
                Some(match text(data, "errorType").as_deref() {
                    Some("authentication") => CopilotEvent::SignedOut,
                    Some("quota") => CopilotEvent::AllowanceExhausted { message },
                    _ => CopilotEvent::Error { message },
                })
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn cli_events_map_to_port_events() {
        let mut state = EventState::default();
        let map = |s: &mut EventState, kind: &str, data: Value| s.map(kind, &data);
        assert_eq!(
            map(&mut state, "session.start", json!({})),
            Some(CopilotEvent::SessionStarted)
        );
        assert_eq!(
            map(
                &mut state,
                "tool.execution_start",
                json!({"toolName": "view", "toolCallId": "c1"})
            ),
            Some(CopilotEvent::ToolStarted {
                tool: "view".into()
            })
        );
        assert_eq!(
            map(
                &mut state,
                "tool.execution_complete",
                json!({"toolCallId": "c1", "success": true})
            ),
            Some(CopilotEvent::ToolFinished {
                tool: "view".into(),
                succeeded: true
            })
        );
        assert_eq!(
            map(&mut state, "assistant.message", json!({"content": "Here."})),
            Some(CopilotEvent::AssistantText {
                text: "Here.".into()
            })
        );
        assert_eq!(
            map(
                &mut state,
                "assistant.usage",
                json!({"inputTokens": 10, "outputTokens": 4})
            ),
            Some(CopilotEvent::Usage {
                requests: Some(1),
                input_tokens: Some(10),
                output_tokens: Some(4),
                cached_input_tokens: None
            })
        );
        assert_eq!(
            map(&mut state, "session.idle", json!({})),
            Some(CopilotEvent::TurnEnded {
                final_text: Some("Here.".into())
            })
        );
        assert_eq!(
            map(
                &mut state,
                "session.error",
                json!({"errorType": "authentication"})
            ),
            Some(CopilotEvent::SignedOut)
        );
        assert_eq!(
            map(
                &mut state,
                "session.error",
                json!({"errorType": "quota", "message": "Used up"})
            ),
            Some(CopilotEvent::AllowanceExhausted {
                message: "Used up".into()
            })
        );
        assert_eq!(
            map(
                &mut state,
                "session.error",
                json!({"errorType": "query", "message": "m"})
            ),
            Some(CopilotEvent::Error {
                message: "m".into()
            })
        );
        assert_eq!(map(&mut state, "assistant.message_delta", json!({})), None);
    }

    #[test]
    fn every_mcp_server_offers_all_its_tools_unless_it_names_them() {
        struct Never;
        #[async_trait::async_trait]
        impl ToolGate for Never {
            async fn decide(&self, _t: &str, _a: Value, _id: Option<String>) -> GateAnswer {
                GateAnswer::Allow
            }
        }
        #[async_trait::async_trait]
        impl ToolObserver for Never {
            async fn tool_used(&self, _t: &str, _id: Option<String>, _ok: bool) {}
        }
        let opts = SessionOptions {
            model: None,
            mcp_servers: json!({"mcpServers": {
                "work": {"type": "stdio", "command": "/opt/w"},
                "quotes": {"type": "stdio", "command": "/opt/q", "tools": ["price"]},
            }}),
            pre_tool_use: Arc::new(Never),
            permission: Arc::new(Never),
            post_tool_use: Arc::new(Never),
        };
        let params = CliServer::session_params("s1", &opts);
        assert_eq!(params["mcpServers"]["work"]["tools"], json!(["*"]));
        assert_eq!(params["mcpServers"]["quotes"]["tools"], json!(["price"]));
        assert_eq!(params["mcpServers"]["work"]["command"], "/opt/w");
    }

    #[test]
    fn a_permission_request_names_its_tool() {
        assert_eq!(
            permission_tool(&json!({"kind": "mcp", "serverName": "work", "toolName": "plan"})),
            "mcp__work__plan"
        );
        assert_eq!(permission_tool(&json!({"kind": "shell"})), "shell");
        assert_eq!(permission_tool(&json!({"toolName": "bash"})), "bash");
    }

    struct Gate(GateAnswer, Arc<Mutex<Vec<String>>>);

    #[async_trait::async_trait]
    impl ToolGate for Gate {
        async fn decide(&self, tool: &str, _a: Value, _id: Option<String>) -> GateAnswer {
            lock(&self.1).push(tool.to_owned());
            self.0.clone()
        }
    }

    struct Observer(Arc<AtomicUsize>);

    #[async_trait::async_trait]
    impl ToolObserver for Observer {
        async fn tool_used(&self, _t: &str, _id: Option<String>, _ok: bool) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    async fn write_frame(w: &mut (impl AsyncWrite + Unpin), v: Value) {
        let body = serde_json::to_vec(&v).unwrap();
        w.write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
            .await
            .unwrap();
        w.write_all(&body).await.unwrap();
    }

    /// The protocol against a CLI played by the test over in-memory pipes: requests and replies,
    /// a `hooks.invoke` pre-tool-use answered by the gate, a permission request answered back,
    /// events routed to the session, and the end of the stream.
    #[tokio::test]
    async fn the_protocol_round_trips_over_framed_streams() {
        let (ours, theirs) = tokio::io::duplex(1 << 16);
        let (our_read, our_write) = tokio::io::split(ours);
        let (their_read, mut their_write) = tokio::io::split(theirs);
        let connection = Connection::open(our_read, our_write);
        let asked = Arc::new(Mutex::new(Vec::new()));
        let used = Arc::new(AtomicUsize::new(0));
        let deny: Arc<dyn ToolGate> = Arc::new(Gate(
            GateAnswer::Deny {
                reason: "No.".into(),
            },
            asked.clone(),
        ));
        let (events, pending) = mpsc::unbounded_channel();
        lock(&connection.0.sessions).insert(
            "s1".into(),
            SessionRoute {
                events,
                pre_tool_use: deny.clone(),
                permission: deny,
                post_tool_use: Arc::new(Observer(used.clone())),
                pending: Some(pending),
            },
        );
        let mut their_read = BufReader::new(their_read);

        let call = tokio::spawn({
            let connection = connection.clone();
            async move { connection.call("auth.getStatus", json!({})).await }
        });
        let request = read_message(&mut their_read).await.unwrap();
        assert_eq!(request["method"], "auth.getStatus");
        write_frame(
            &mut their_write,
            json!({"jsonrpc": "2.0", "id": request["id"], "result": {"isAuthenticated": true}}),
        )
        .await;
        assert_eq!(call.await.unwrap().unwrap()["isAuthenticated"], true);

        write_frame(&mut their_write, json!({"jsonrpc": "2.0", "id": 900, "method": "hooks.invoke",
            "params": {"sessionId": "s1", "hookType": "preToolUse", "input": {"toolName": "bash", "toolArgs": {}}}})).await;
        let reply = read_message(&mut their_read).await.unwrap();
        assert_eq!(reply["id"], 900);
        assert_eq!(reply["result"]["output"]["permissionDecision"], "deny");
        assert_eq!(reply["result"]["output"]["permissionDecisionReason"], "No.");

        write_frame(&mut their_write, json!({"jsonrpc": "2.0", "id": 901, "method": "hooks.invoke",
            "params": {"sessionId": "s1", "hookType": "postToolUse", "input": {"toolName": "bash"}}})).await;
        assert_eq!(read_message(&mut their_read).await.unwrap()["id"], 901);
        assert_eq!(used.load(Ordering::SeqCst), 1);

        write_frame(
            &mut their_write,
            json!({"jsonrpc": "2.0", "id": 902, "method": "userInput.request", "params": {}}),
        )
        .await;
        assert_eq!(
            read_message(&mut their_read).await.unwrap()["error"]["code"],
            METHOD_NOT_FOUND
        );

        write_frame(&mut their_write, json!({"jsonrpc": "2.0", "method": "session.event", "params": {"sessionId": "s1",
            "event": {"type": "permission.requested", "data": {"requestId": "r1", "permissionRequest": {"kind": "shell"}}}}})).await;
        let answer = read_message(&mut their_read).await.unwrap();
        assert_eq!(
            answer["method"],
            "session.permissions.handlePendingPermissionRequest"
        );
        assert_eq!(answer["params"]["requestId"], "r1");
        assert_eq!(
            answer["params"]["result"],
            json!({"kind": "reject", "feedback": "No."})
        );
        assert_eq!(lock(&asked).as_slice(), ["bash", "shell"]);

        let mut events = lock(&connection.0.sessions)
            .get_mut("s1")
            .unwrap()
            .pending
            .take()
            .unwrap();
        assert_eq!(events.recv().await.unwrap().0, "permission.requested");
        drop(their_write);
        drop(their_read);
        assert!(
            events.recv().await.is_none(),
            "the stream's end closes the session's events"
        );
        assert!(matches!(
            connection.call("ping", json!({})).await,
            Err(CallError::Closed)
        ));
    }
}
