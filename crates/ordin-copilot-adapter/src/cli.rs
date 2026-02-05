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
//! | declared tools | `tools` in `session.create` / `session.resume`; event `external_tool.requested` → request `session.tools.handlePendingToolCall` (CPA-HT-1) |
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
    HostToolAnswer, HostToolHandler, PortError, SessionOptions, ToolGate, ToolObserver,
};

/// How long one request waits for its reply (the turn itself is streamed, not awaited).
const CALL_TIMEOUT: Duration = Duration::from_secs(60);
/// The largest message ordin reads (a peer's `Content-Length` is a claim, not a size received).
const MAX_MESSAGE: usize = 64 * 1024 * 1024;
/// JSON-RPC "method not found".
const METHOD_NOT_FOUND: i64 = -32601;
/// The answer to a declared-tool call for a session this connection does not hold (CPA-HT-1).
const NOT_OFFERED: &str = "This tool is not offered to this session.";

/// The production `CopilotPort` (*Public Interface*: the program passes it to `new`).
#[derive(Debug, Default, Clone)]
pub struct CliCopilotPort;

impl CliCopilotPort {
    pub fn new() -> CliCopilotPort {
        CliCopilotPort
    }
}

/// Why reading the CLI's output stopped. Logged by its name only: the frame itself may hold text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadEnd {
    /// The output ended cleanly between messages.
    Ended,
    /// A header line could not be read (an I/O error, or text that is not UTF-8).
    HeaderUnreadable(std::io::ErrorKind),
    /// The header block held no usable `Content-Length`.
    NoLength,
    /// The claimed length is over [`MAX_MESSAGE`].
    TooLarge(usize),
    /// The body ended before its claimed length.
    BodyUnreadable(std::io::ErrorKind, usize),
    /// The body is not JSON.
    NotJson(serde_json::error::Category, usize),
}

impl ReadEnd {
    fn kind(&self) -> &'static str {
        match self {
            ReadEnd::Ended => "ended",
            ReadEnd::HeaderUnreadable(_) => "header_unreadable",
            ReadEnd::NoLength => "no_length",
            ReadEnd::TooLarge(_) => "too_large",
            ReadEnd::BodyUnreadable(..) => "body_unreadable",
            ReadEnd::NotJson(..) => "not_json",
        }
    }

    /// The claimed length of the frame, where one was read; zero otherwise.
    fn length(&self) -> u64 {
        match self {
            ReadEnd::TooLarge(n) | ReadEnd::BodyUnreadable(_, n) | ReadEnd::NotJson(_, n) => {
                *n as u64
            }
            _ => 0,
        }
    }

    /// The I/O error kind or JSON error category, as a name; empty when there is none.
    fn cause(&self) -> String {
        match self {
            ReadEnd::HeaderUnreadable(kind) | ReadEnd::BodyUnreadable(kind, _) => {
                format!("{kind:?}")
            }
            ReadEnd::NotJson(category, _) => format!("{category:?}"),
            _ => String::new(),
        }
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

/// How the CLI words its refusal of a request for a session it does not hold. The CLI gives this
/// refusal no code of its own (its schema names none, and the runtime answers with a plain error),
/// so the start of the message is the only thing that tells it apart; the message is never logged.
const SESSION_NOT_FOUND: &str = "Session not found";

/// `session.send`'s failure: a refusal for a session the CLI does not hold is
/// [`PortErrorKind::SessionNotFound`](crate::port::PortErrorKind::SessionNotFound).
// spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CONV-3
fn send_error(error: CallError) -> PortError {
    match &error {
        CallError::Refused { message, .. } if message.starts_with(SESSION_NOT_FOUND) => {
            PortError::session_not_found(error.to_string())
        }
        _ => port_error(error),
    }
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
            .map_err(|error| {
                tracing::warn!(
                    target: "ordin_copilot_adapter",
                    io_kind = format!("{:?}", error.kind()).as_str(),
                    "copilot.cli.spawn_failed"
                );
                port_error(error)
            })?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            tracing::warn!(
                target: "ordin_copilot_adapter",
                "copilot.cli.streams_missing"
            );
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
    host_tool_handler: Arc<dyn HostToolHandler>,
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
        let body = serde_json::to_vec(message).map_err(|_| {
            tracing::warn!(
                target: "ordin_copilot_adapter",
                stage = "encode",
                "copilot.cli.write_failed"
            );
            CallError::Closed
        })?;
        let mut writer = self.0.writer.lock().await;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        let written = match writer.write_all(header.as_bytes()).await {
            Ok(()) => match writer.write_all(&body).await {
                Ok(()) => writer.flush().await.map_err(|e| ("flush", e)),
                Err(error) => Err(("body", error)),
            },
            Err(error) => Err(("header", error)),
        };
        written.map_err(|(stage, error)| {
            // The error's kind only: its text is the operating system's and adds nothing.
            tracing::warn!(
                target: "ordin_copilot_adapter",
                stage = stage,
                io_kind = format!("{:?}", error.kind()).as_str(),
                bytes = body.len() as u64,
                "copilot.cli.write_failed"
            );
            CallError::Closed
        })
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, CallError> {
        if *lock(&self.0.closed) {
            tracing::warn!(
                target: "ordin_copilot_adapter",
                method = method,
                "copilot.cli.call_after_close"
            );
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
                CallError::Refused { code, message, .. } => {
                    // The code only: the CLI's message may quote what the person wrote.
                    tracing::warn!(
                        target: "ordin_copilot_adapter",
                        method = method,
                        code = code,
                        message_chars = message.chars().count() as u64,
                        "copilot.cli.call_refused"
                    );
                    CallError::Refused {
                        method: method.to_owned(),
                        code,
                        message,
                    }
                }
                other => {
                    tracing::warn!(
                        target: "ordin_copilot_adapter",
                        method = method,
                        "copilot.cli.call_closed"
                    );
                    other
                }
            }),
            Ok(Err(_)) => {
                tracing::warn!(
                    target: "ordin_copilot_adapter",
                    method = method,
                    "copilot.cli.call_closed"
                );
                Err(CallError::Closed)
            }
            Err(_) => {
                lock(&self.0.waiting).remove(&id);
                tracing::warn!(
                    target: "ordin_copilot_adapter",
                    method = method,
                    timeout_ms = CALL_TIMEOUT.as_millis() as u64,
                    "copilot.cli.call_timed_out"
                );
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
        // A failed write is logged by `write`; there is no one else to tell.
        let _ = self.write(&message).await;
    }

    /// Reads messages until the CLI's output ends; then every call and event stream ends too.
    async fn read_loop(self, mut reader: BufReader<impl AsyncRead + Send + Unpin>) {
        let mut messages: u64 = 0;
        let end = loop {
            match read_message(&mut reader).await {
                Ok(message) => {
                    messages += 1;
                    self.dispatch(message);
                }
                Err(end) => break end,
            }
        };
        // Every call and event stream of this connection ends here; this line says why.
        let pending_calls = lock(&self.0.waiting).len() as u64;
        let open_sessions = lock(&self.0.sessions).len() as u64;
        if end == ReadEnd::Ended {
            tracing::info!(
                target: "ordin_copilot_adapter",
                messages = messages,
                pending_calls = pending_calls,
                open_sessions = open_sessions,
                "copilot.cli.output_ended"
            );
        } else {
            tracing::warn!(
                target: "ordin_copilot_adapter",
                kind = end.kind(),
                cause = end.cause().as_str(),
                length = end.length(),
                messages = messages,
                pending_calls = pending_calls,
                open_sessions = open_sessions,
                "copilot.cli.read_failed"
            );
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
                    // A reply after its call timed out, or to an id ordin never sent.
                    tracing::warn!(
                        target: "ordin_copilot_adapter",
                        is_error = message.get("error").is_some(),
                        "copilot.cli.reply_unmatched"
                    );
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
            (method, _) => tracing::warn!(
                target: "ordin_copilot_adapter",
                method = method.unwrap_or(""),
                "copilot.cli.message_ignored"
            ),
        }
    }

    fn session_event(&self, params: Value) {
        let Some(session_id) = text(&params, "sessionId") else {
            tracing::warn!(
                target: "ordin_copilot_adapter",
                missing = "sessionId",
                "copilot.cli.event_malformed"
            );
            return;
        };
        let event = params.get("event").cloned().unwrap_or(Value::Null);
        let Some(kind) = text(&event, "type") else {
            tracing::warn!(
                target: "ordin_copilot_adapter",
                missing = "type",
                "copilot.cli.event_malformed"
            );
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
        if kind == "external_tool.requested" {
            let connection = self.clone();
            let session = session_id.clone();
            let request = data.clone();
            tokio::spawn(async move { connection.answer_tool_call(&session, request).await });
        }
        match lock(&self.0.sessions).get(&session_id) {
            // A send fails only once the session's stream was dropped: the run has ended.
            Some(route) => {
                let _ = route.events.send((kind, data));
            }
            None => tracing::warn!(
                target: "ordin_copilot_adapter",
                kind = kind.as_str(),
                "copilot.cli.event_unrouted"
            ),
        }
    }

    /// `permission.requested`: the gate decides, and the decision goes back to the CLI.
    async fn decide_permission(&self, session_id: &str, data: Value) {
        let Some(request_id) = text(&data, "requestId") else {
            tracing::warn!(
                target: "ordin_copilot_adapter",
                kind = "permission.requested",
                missing = "requestId",
                "copilot.cli.event_malformed"
            );
            return;
        };
        let Some(gate) = lock(&self.0.sessions)
            .get(session_id)
            .map(|route| route.permission.clone())
        else {
            tracing::warn!(
                target: "ordin_copilot_adapter",
                kind = "permission.requested",
                "copilot.cli.event_unrouted"
            );
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

    /// `external_tool.requested`: the session's handler answers on its own task, and the answer goes
    /// back to the CLI. A session this connection does not hold is answered with a failure.
    // spec: ordin-desktop/components/CopilotLocalAdapter#CPA-HT-1
    async fn answer_tool_call(&self, session_id: &str, data: Value) {
        let Some(request_id) = text(&data, "requestId") else {
            tracing::warn!(
                target: "ordin_copilot_adapter",
                kind = "external_tool.requested",
                missing = "requestId",
                "copilot.cli.event_malformed"
            );
            return;
        };
        let handler = lock(&self.0.sessions)
            .get(session_id)
            .map(|route| route.host_tool_handler.clone());
        let answer = match handler {
            Some(handler) => {
                let name = text(&data, "toolName").unwrap_or_default();
                let arguments = data.get("arguments").cloned().unwrap_or(Value::Null);
                handler
                    .call(&name, arguments, tokio_util::sync::CancellationToken::new())
                    .await
            }
            None => HostToolAnswer::Failure {
                text: NOT_OFFERED.to_owned(),
            },
        };
        let (text_result, kind) = match answer {
            HostToolAnswer::Success { text } => (text, "success"),
            HostToolAnswer::Failure { text } => (text, "failure"),
        };
        let params = json!({
            "sessionId": session_id,
            "requestId": request_id,
            "result": {"textResultForLlm": text_result, "resultType": kind},
        });
        let _ = self
            .call("session.tools.handlePendingToolCall", params)
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
            (None, "preToolUse") => {
                tracing::warn!(
                    target: "ordin_copilot_adapter",
                    hook = "preToolUse",
                    "copilot.cli.hook_unrouted"
                );
                json!({
                    "permissionDecision": "deny",
                    "permissionDecisionReason": crate::gate::COULD_NOT_CHECK,
                })
            }
            _ => json!({}),
        };
        self.reply(id, Ok(json!({ "output": output }))).await;
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// One `Content-Length`-framed message; at the end of the stream or on a broken frame, why not.
async fn read_message(reader: &mut BufReader<impl AsyncRead + Unpin>) -> Result<Value, ReadEnd> {
    let mut length = None;
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .await
            .map_err(|error| ReadEnd::HeaderUnreadable(error.kind()))?;
        if read == 0 {
            return Err(ReadEnd::Ended);
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
    let length = length.ok_or(ReadEnd::NoLength)?;
    if length > MAX_MESSAGE {
        return Err(ReadEnd::TooLarge(length));
    }
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .await
        .map_err(|error| ReadEnd::BodyUnreadable(error.kind(), length))?;
    serde_json::from_slice(&body).map_err(|error| ReadEnd::NotJson(error.classify(), length))
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
                host_tool_handler: opts.host_tool_handler.clone(),
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
        // spec: ordin-desktop/components/CopilotLocalAdapter#CPA-HT-1
        if !opts.host_tools.is_empty() {
            params["tools"] = Value::Array(
                opts.host_tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.parameters,
                        })
                    })
                    .collect(),
            );
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
            .map_err(|error| {
                tracing::warn!(
                    target: "ordin_copilot_adapter",
                    io_kind = format!("{:?}", error.kind()).as_str(),
                    "copilot.sign_in.login_not_started"
                );
                port_error(error)
            })?;
        tokio::spawn(async move {
            // How `copilot login` ended: its exit code only, as it writes nowhere ordin reads.
            match login.wait().await {
                Ok(status) if status.success() => tracing::info!(
                    target: "ordin_copilot_adapter",
                    "copilot.sign_in.login_ended"
                ),
                Ok(status) => tracing::warn!(
                    target: "ordin_copilot_adapter",
                    exit_code = status.code().map(i64::from),
                    "copilot.sign_in.login_failed"
                ),
                Err(error) => tracing::warn!(
                    target: "ordin_copilot_adapter",
                    io_kind = format!("{:?}", error.kind()).as_str(),
                    "copilot.sign_in.login_failed"
                ),
            }
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
            .map_err(send_error)
    }

    fn events(&self, session_id: &str) -> BoxStream<'static, CopilotEvent> {
        let pending = lock(&self.connection.0.sessions)
            .get_mut(session_id)
            .and_then(|route| route.pending.take());
        let Some(receiver) = pending else {
            tracing::warn!(
                target: "ordin_copilot_adapter",
                "copilot.cli.events_unavailable"
            );
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

    // spec: ordin-desktop/components/CopilotLocalAdapter#CPA-STOP-1
    async fn stop(&self, deadline: Duration) {
        let started = tokio::time::Instant::now();
        // Each open session's turn is ended first, so none is cut off mid-write by the steps below.
        let sessions: Vec<String> = lock(&self.connection.0.sessions).keys().cloned().collect();
        for session_id in sessions {
            let abort = self.connection.call("session.abort", json!({"sessionId": session_id}));
            if !matches!(tokio::time::timeout(STOP_ABORT_LIMIT, abort).await, Ok(Ok(_))) {
                tracing::info!(target: "ordin_copilot_adapter", "copilot.cli.stop_abort_failed");
            }
        }
        // Closing standard input asks the CLI to end. A failed close is expected when the CLI has
        // already ended; the wait below says how.
        if let Err(error) = self.connection.0.writer.lock().await.shutdown().await {
            tracing::info!(
                target: "ordin_copilot_adapter",
                io_kind = format!("{:?}", error.kind()).as_str(),
                "copilot.cli.stdin_close_failed"
            );
        }
        let mut child = self.child.lock().await;
        let grace = STOP_TERMINATE_AFTER.min(deadline.saturating_sub(started.elapsed()));
        let mut ended = tokio::time::timeout(grace, child.wait()).await.is_ok();
        if !ended {
            // The CLI in server mode does not end on a closed standard input; the quit signal lets
            // it save its state and exit.
            terminate(&child);
            let rest = deadline.saturating_sub(started.elapsed());
            ended = tokio::time::timeout(rest, child.wait()).await.is_ok();
        }
        if !ended {
            tracing::warn!(
                target: "ordin_copilot_adapter",
                deadline_ms = deadline.as_millis() as u64,
                "copilot.cli.stop_timed_out"
            );
            if let Err(error) = child.start_kill() {
                tracing::warn!(
                    target: "ordin_copilot_adapter",
                    io_kind = format!("{:?}", error.kind()).as_str(),
                    "copilot.cli.kill_failed"
                );
            }
            let _ = child.wait().await;
        }
        lock(&self.connection.0.sessions).clear();
    }
}

/// CPA-STOP-1: how long one session's abort may take while stopping, and when the quit signal is
/// sent if the CLI has not ended on its closed standard input.
const STOP_ABORT_LIMIT: Duration = Duration::from_millis(500);
const STOP_TERMINATE_AFTER: Duration = Duration::from_secs(2);

/// Sends the CLI the ordinary quit signal (SIGTERM). Windows has none; there the deadline's kill
/// is the only step left.
fn terminate(child: &Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        use nix::sys::signal::{kill, Signal};
        if kill(nix::unistd::Pid::from_raw(pid as i32), Signal::SIGTERM).is_err() {
            tracing::info!(target: "ordin_copilot_adapter", "copilot.cli.terminate_failed");
        }
    }
    #[cfg(not(unix))]
    let _ = child;
}

/// What the event mapping remembers between events: each tool call's name (the completion carries
/// only its id) and the assistant's last message (the turn's final text).
#[derive(Default)]
struct EventState {
    tools: HashMap<String, String>,
    last_message: Option<String>,
    /// The event types already logged as ignored, so a stream of one type logs once.
    ignored: std::collections::HashSet<String>,
}

/// An event ordin reads but which lacks the field it needs: logged, then dropped.
fn incomplete(kind: &str, missing: &'static str) -> Option<CopilotEvent> {
    tracing::warn!(
        target: "ordin_copilot_adapter",
        kind = kind,
        missing = missing,
        "copilot.event.incomplete"
    );
    None
}

fn count_of(data: &Value, key: &str) -> Option<u64> {
    data.get(key).and_then(Value::as_u64)
}

impl EventState {
    fn map(&mut self, kind: &str, data: &Value) -> Option<CopilotEvent> {
        match kind {
            "session.start" => Some(CopilotEvent::SessionStarted),
            "assistant.message" => {
                let Some(text) = text(data, "content") else {
                    return incomplete(kind, "content");
                };
                self.last_message = Some(text.clone());
                Some(CopilotEvent::AssistantText { text })
            }
            "tool.execution_start" => {
                let Some(tool) = text(data, "toolName") else {
                    return incomplete(kind, "toolName");
                };
                if let Some(id) = text(data, "toolCallId") {
                    self.tools.insert(id, tool.clone());
                }
                Some(CopilotEvent::ToolStarted { tool })
            }
            "tool.execution_complete" => {
                let Some(id) = text(data, "toolCallId") else {
                    return incomplete(kind, "toolCallId");
                };
                let Some(tool) = self.tools.remove(&id) else {
                    return incomplete(kind, "started_tool");
                };
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
            "session.idle" => {
                let final_text = self.last_message.take();
                tracing::info!(
                    target: "ordin_copilot_adapter",
                    has_final_text = final_text.is_some(),
                    final_text_chars = final_text.as_ref().map_or(0, |t| t.chars().count() as u64),
                    "copilot.event.turn_ended"
                );
                Some(CopilotEvent::TurnEnded { final_text })
            }
            "session.error" => {
                let message = text(data, "message").unwrap_or_default();
                // The error's type only: its message is the service's and may quote the turn.
                tracing::warn!(
                    target: "ordin_copilot_adapter",
                    error_type = text(data, "errorType").as_deref().unwrap_or(""),
                    message_chars = message.chars().count() as u64,
                    "copilot.event.session_error"
                );
                Some(match text(data, "errorType").as_deref() {
                    Some("authentication") => CopilotEvent::SignedOut,
                    Some("quota") => CopilotEvent::AllowanceExhausted { message },
                    _ => CopilotEvent::Error { message },
                })
            }
            _ => {
                if self.ignored.insert(kind.to_owned()) {
                    tracing::info!(
                        target: "ordin_copilot_adapter",
                        kind = kind,
                        "copilot.event.ignored"
                    );
                }
                None
            }
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
            host_tools: vec![],
            host_tool_handler: Arc::new(Answer(HostToolAnswer::Success { text: "-".into() })),
        };
        let params = CliServer::session_params("s1", &opts);
        assert_eq!(params["mcpServers"]["work"]["tools"], json!(["*"]));
        assert_eq!(params["mcpServers"]["quotes"]["tools"], json!(["price"]));
        assert_eq!(params["mcpServers"]["work"]["command"], "/opt/w");
        assert!(
            params.get("tools").is_none(),
            "no declared tool, no `tools`"
        );
    }

    /// Answers every declared-tool call with one fixed answer, recording the names.
    struct Answer(HostToolAnswer);

    #[async_trait::async_trait]
    impl HostToolHandler for Answer {
        async fn call(
            &self,
            _name: &str,
            _arguments: Value,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> HostToolAnswer {
            self.0.clone()
        }
    }

    // spec: ordin-desktop/components/CopilotLocalAdapter#CPA-HT-1
    #[test]
    fn declared_tools_are_sent_as_tools() {
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
            mcp_servers: json!({}),
            pre_tool_use: Arc::new(Never),
            permission: Arc::new(Never),
            post_tool_use: Arc::new(Never),
            host_tools: vec![crate::port::HostTool {
                name: "mcp__work__report".into(),
                description: "Reports.".into(),
                parameters: json!({"type": "object"}),
            }],
            host_tool_handler: Arc::new(Answer(HostToolAnswer::Success { text: "-".into() })),
        };
        let params = CliServer::session_params("s1", &opts);
        assert_eq!(
            params["tools"],
            json!([{"name": "mcp__work__report", "description": "Reports.", "parameters": {"type": "object"}}])
        );
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
                host_tool_handler: Arc::new(Answer(HostToolAnswer::Failure {
                    text: "Not in this workspace.".into(),
                })),
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

        // A declared-tool call is answered by the session's handler (CPA-HT-1).
        write_frame(&mut their_write, json!({"jsonrpc": "2.0", "method": "session.event", "params": {"sessionId": "s1",
            "event": {"type": "external_tool.requested", "data": {"requestId": "t1", "toolName": "mcp__work__report", "arguments": {}}}}})).await;
        let answer = read_message(&mut their_read).await.unwrap();
        assert_eq!(answer["method"], "session.tools.handlePendingToolCall");
        assert_eq!(
            answer["params"],
            json!({"sessionId": "s1", "requestId": "t1",
                   "result": {"textResultForLlm": "Not in this workspace.", "resultType": "failure"}})
        );
        // One for a session this connection does not hold is answered with a failure.
        write_frame(&mut their_write, json!({"jsonrpc": "2.0", "method": "session.event", "params": {"sessionId": "other",
            "event": {"type": "external_tool.requested", "data": {"requestId": "t2", "toolName": "mcp__work__report"}}}})).await;
        let answer = read_message(&mut their_read).await.unwrap();
        assert_eq!(answer["params"]["requestId"], "t2");
        assert_eq!(
            answer["params"]["result"],
            json!({"textResultForLlm": NOT_OFFERED, "resultType": "failure"})
        );

        let mut events = lock(&connection.0.sessions)
            .get_mut("s1")
            .unwrap()
            .pending
            .take()
            .unwrap();
        assert_eq!(events.recv().await.unwrap().0, "permission.requested");
        assert_eq!(events.recv().await.unwrap().0, "external_tool.requested");
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

    // ---- failures are logged (plan 08 Phase 18) ----

    #[derive(Clone, Default)]
    struct Lines(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Lines {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            lock(&self.0).extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Lines {
        /// Installs ordin's own line format for this thread until the guard drops.
        fn install(&self) -> tracing::subscriber::DefaultGuard {
            let writer = self.clone();
            tracing::subscriber::set_default(ordin_types::log::subscriber(
                move || writer.clone(),
                tracing::Level::INFO,
            ))
        }
        fn text(&self) -> String {
            String::from_utf8(lock(&self.0).clone()).expect("utf-8")
        }
        /// The one line of `event`, parsed.
        fn line(&self, event: &str) -> Value {
            let text = self.text();
            assert!(!text.contains("invalid_event_name"), "{text}");
            assert!(!text.contains("<debug>"), "{text}");
            let found = text
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).expect("json line"))
                .find(|line| line["event"] == event);
            found.unwrap_or_else(|| panic!("no {event} line in {text}"))
        }
    }

    /// A connection whose CLI wrote `output` and then ended; returns once reading has stopped.
    async fn read_to_end(output: &[u8]) -> Connection {
        let connection = Connection::open(std::io::Cursor::new(output.to_vec()), tokio::io::sink());
        while !*lock(&connection.0.closed) {
            tokio::task::yield_now().await;
        }
        connection
    }

    #[tokio::test]
    async fn the_end_of_the_cli_output_is_logged_with_why() {
        let frame =
            |body: &str| format!("Content-Length: {}\r\n\r\n{body}", body.len()).into_bytes();
        let short = b"Content-Length: 50\r\n\r\n{\"secret words\"".to_vec();
        let cases: Vec<(&str, Vec<u8>, &str, &str, u64)> = vec![
            (
                "not_json",
                frame("{secret words}"),
                "not_json",
                "Syntax",
                14,
            ),
            (
                "too_large",
                b"Content-Length: 999999999999\r\n\r\n".to_vec(),
                "too_large",
                "",
                999_999_999_999,
            ),
            (
                "no_length",
                b"X-Secret-Words: 1\r\n\r\n".to_vec(),
                "no_length",
                "",
                0,
            ),
            (
                "body_unreadable",
                short,
                "body_unreadable",
                "UnexpectedEof",
                50,
            ),
            (
                "header_unreadable",
                b"\xff\xfe secret words\r\n".to_vec(),
                "header_unreadable",
                "InvalidData",
                0,
            ),
        ];
        for (name, output, kind, cause, length) in cases {
            let lines = Lines::default();
            let _guard = lines.install();
            read_to_end(&output).await;
            let line = lines.line("copilot.cli.read_failed");
            assert_eq!(line["level"], "WARNING", "{name}");
            assert_eq!(line["kind"], kind, "{name}");
            assert_eq!(line["cause"], cause, "{name}");
            assert_eq!(line["length"], length, "{name}");
            assert_eq!(line["messages"], 0, "{name}");
            assert!(!lines.text().contains("secret"), "{name}: {}", lines.text());
        }

        let lines = Lines::default();
        let _guard = lines.install();
        let output = frame(
            r#"{"jsonrpc":"2.0","method":"session.event","params":{"sessionId":"s9","event":{"type":"session.idle"}}}"#,
        );
        read_to_end(&output).await;
        let line = lines.line("copilot.cli.output_ended");
        assert_eq!(line["level"], "INFO");
        assert_eq!(line["messages"], 1);
        assert_eq!(line["pending_calls"], 0);
        // The event for a session ordin does not hold is logged by its type.
        assert_eq!(
            lines.line("copilot.cli.event_unrouted")["kind"],
            "session.idle"
        );
    }

    #[tokio::test]
    async fn messages_ordin_cannot_use_are_logged_by_shape() {
        let lines = Lines::default();
        let _guard = lines.install();
        let connection = read_to_end(b"").await;
        connection
            .dispatch(json!({"jsonrpc": "2.0", "id": 77, "result": {"text": "secret words"}}));
        assert_eq!(lines.line("copilot.cli.reply_unmatched")["is_error"], false);
        connection.dispatch(
            json!({"jsonrpc": "2.0", "method": "log.note", "params": {"text": "secret words"}}),
        );
        assert_eq!(
            lines.line("copilot.cli.message_ignored")["method"],
            "log.note"
        );
        connection.dispatch(json!({"jsonrpc": "2.0", "method": "session.event", "params": {"event": {"type": "x"}}}));
        assert_eq!(
            lines.line("copilot.cli.event_malformed")["missing"],
            "sessionId"
        );
        assert!(!lines.text().contains("secret"), "{}", lines.text());
    }

    // spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CONV-3
    #[test]
    fn a_send_refused_for_an_unknown_session_is_told_apart() {
        use crate::port::PortErrorKind;
        let refused = |message: &str| CallError::Refused {
            method: "session.send".into(),
            code: -32603,
            message: message.into(),
        };
        assert_eq!(
            send_error(refused("Session not found for sessionId: s1")).kind,
            PortErrorKind::SessionNotFound
        );
        assert_eq!(
            send_error(refused("Session not found: s1")).kind,
            PortErrorKind::SessionNotFound
        );
        assert_eq!(send_error(refused("boom")).kind, PortErrorKind::Other);
        assert_eq!(send_error(CallError::Closed).kind, PortErrorKind::Other);
    }

    #[tokio::test]
    async fn failed_calls_are_logged_by_method_and_code() {
        let lines = Lines::default();
        let _guard = lines.install();
        // Refused: the code is logged, the CLI's message is not.
        let (ours, theirs) = tokio::io::duplex(1 << 16);
        let (our_read, our_write) = tokio::io::split(ours);
        let (their_read, mut their_write) = tokio::io::split(theirs);
        let connection = Connection::open(our_read, our_write);
        let mut their_read = BufReader::new(their_read);
        let call = tokio::spawn({
            let connection = connection.clone();
            async move { connection.call("session.send", json!({})).await }
        });
        let request = read_message(&mut their_read).await.unwrap();
        write_frame(
            &mut their_write,
            json!({"jsonrpc": "2.0", "id": request["id"],
            "error": {"code": -32000, "message": "secret words"}}),
        )
        .await;
        assert!(call.await.unwrap().is_err());
        let line = lines.line("copilot.cli.call_refused");
        assert_eq!(line["method"], "session.send");
        assert_eq!(line["code"], -32000);
        assert_eq!(line["message_chars"], 12);

        // A write to a CLI that has gone: the I/O error's kind.
        drop(their_read);
        drop(their_write);
        while !*lock(&connection.0.closed) {
            tokio::task::yield_now().await;
        }
        *lock(&connection.0.closed) = false;
        assert!(connection.call("ping", json!({})).await.is_err());
        let line = lines.line("copilot.cli.write_failed");
        assert_eq!(line["stage"], "header");
        assert_eq!(line["io_kind"], "BrokenPipe");

        // After the output ended.
        *lock(&connection.0.closed) = true;
        assert!(connection.call("ping", json!({})).await.is_err());
        assert_eq!(lines.line("copilot.cli.call_after_close")["method"], "ping");
        assert!(!lines.text().contains("secret"), "{}", lines.text());
    }

    #[test]
    fn events_ordin_drops_are_logged_without_their_text() {
        let lines = Lines::default();
        let _guard = lines.install();
        let mut state = EventState::default();
        assert_eq!(
            state.map("assistant.message", &json!({"other": "secret words"})),
            None
        );
        assert_eq!(state.map("tool.execution_start", &json!({})), None);
        assert_eq!(
            state.map("tool.execution_complete", &json!({"toolCallId": "nope"})),
            None
        );
        let incomplete: Vec<(String, String)> = lines
            .text()
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .filter(|l| l["event"] == "copilot.event.incomplete")
            .map(|l| {
                (
                    l["kind"].as_str().unwrap().to_owned(),
                    l["missing"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        assert_eq!(
            incomplete,
            [
                ("assistant.message".to_owned(), "content".to_owned()),
                ("tool.execution_start".to_owned(), "toolName".to_owned()),
                (
                    "tool.execution_complete".to_owned(),
                    "started_tool".to_owned()
                ),
            ]
        );
        assert_eq!(lines.line("copilot.event.incomplete")["level"], "WARNING");

        // An unknown type is logged once per stream.
        state.map(
            "assistant.message_delta",
            &json!({"deltaContent": "secret words"}),
        );
        state.map(
            "assistant.message_delta",
            &json!({"deltaContent": "secret words"}),
        );
        assert_eq!(
            lines.line("copilot.event.ignored")["kind"],
            "assistant.message_delta"
        );
        assert_eq!(lines.text().matches("copilot.event.ignored").count(), 1);

        state.map(
            "session.error",
            &json!({"errorType": "network", "message": "secret words"}),
        );
        let line = lines.line("copilot.event.session_error");
        assert_eq!(line["error_type"], "network");
        assert_eq!(line["message_chars"], 12);

        state.map("session.idle", &json!({}));
        let line = lines.line("copilot.event.turn_ended");
        assert_eq!(line["has_final_text"], false);
        assert_eq!(line["final_text_chars"], 0);
        assert!(!lines.text().contains("secret"), "{}", lines.text());
    }

    #[tokio::test]
    async fn a_cli_that_cannot_start_logs_the_error_kind_not_the_path() {
        let lines = Lines::default();
        let _guard = lines.install();
        let spec = CliServerSpec {
            cli_path: PathBuf::from("/nowhere/secret-words/copilot"),
            working_folder: std::env::temp_dir(),
            env: BTreeMap::new(),
        };
        assert!(CliCopilotPort::new().start_server(spec).await.is_err());
        assert_eq!(
            lines.line("copilot.cli.spawn_failed")["io_kind"],
            "NotFound"
        );

        // `copilot login` that cannot start: the same, through a server whose child is this test.
        let child = Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .stdout(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let server = CliServer {
            connection: read_to_end(b"").await,
            child: tokio::sync::Mutex::new(child),
            cli_path: PathBuf::from("/nowhere/secret-words/copilot"),
            env: BTreeMap::new(),
            working_folder: std::env::temp_dir(),
        };
        assert!(server.start_login().await.is_err());
        assert_eq!(
            lines.line("copilot.sign_in.login_not_started")["io_kind"],
            "NotFound"
        );
        assert!(!lines.text().contains("secret"), "{}", lines.text());
    }

    /// A writer whose close fails, as a pipe to a CLI that has already gone.
    struct CloseFails;
    impl AsyncWrite for CloseFails {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            std::task::Poll::Ready(Ok(buf.len()))
        }
        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Ok(()))
        }
        fn poll_shutdown(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Err(std::io::ErrorKind::BrokenPipe.into()))
        }
    }

    /// A server over `connection` whose child runs `program args`, and whose `copilot login` is
    /// `cli_path`.
    #[cfg(unix)]
    fn server_of(
        connection: Connection,
        program: &str,
        args: &[&str],
        cli_path: &str,
    ) -> CliServer {
        let child = Command::new(program)
            .args(args)
            .kill_on_drop(true)
            .spawn()
            .expect("child");
        CliServer {
            connection,
            child: tokio::sync::Mutex::new(child),
            cli_path: PathBuf::from(cli_path),
            env: BTreeMap::from([("PATH".to_owned(), "/usr/bin:/bin".to_owned())]),
            working_folder: std::env::temp_dir(),
        }
    }

    /// Waits until `lines` holds `event`.
    async fn until_logged(lines: &Lines, event: &str) {
        for _ in 0..500 {
            if lines.text().contains(event) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("never logged {event}: {}", lines.text());
    }

    #[tokio::test(start_paused = true)]
    async fn a_request_with_no_answer_logs_its_method_and_time_limit() {
        let lines = Lines::default();
        let _guard = lines.install();
        let (ours, _theirs) = tokio::io::duplex(1 << 16);
        let (our_read, our_write) = tokio::io::split(ours);
        let connection = Connection::open(our_read, our_write);
        assert!(matches!(
            connection.call("auth.getStatus", json!({})).await,
            Err(CallError::NoAnswer(_))
        ));
        let line = lines.line("copilot.cli.call_timed_out");
        assert_eq!(line["method"], "auth.getStatus");
        assert_eq!(line["timeout_ms"], 60_000);
    }

    #[tokio::test]
    async fn a_hook_for_an_unknown_session_is_logged_and_denied() {
        let lines = Lines::default();
        let _guard = lines.install();
        let connection = read_to_end(b"").await;
        connection
            .answer(
                "hooks.invoke",
                json!(5),
                json!({"sessionId": "gone", "hookType": "preToolUse",
                       "input": {"toolName": "bash", "toolArgs": {"command": "secret words"}}}),
            )
            .await;
        assert_eq!(
            lines.line("copilot.cli.hook_unrouted")["hook"],
            "preToolUse"
        );
        assert!(!lines.text().contains("secret"), "{}", lines.text());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_servers_own_failures_are_logged() {
        let lines = Lines::default();
        let _guard = lines.install();

        // Events for a session with no route.
        let server = server_of(read_to_end(b"").await, "true", &[], "true");
        let mut events = server.events("unknown");
        assert_eq!(
            events.next().await,
            Some(CopilotEvent::ServerExited { status: None })
        );
        lines.line("copilot.cli.events_unavailable");

        // `copilot login` that ends well, and one that fails.
        server.start_login().await.expect("login");
        until_logged(&lines, "copilot.sign_in.login_ended").await;
        assert_eq!(lines.line("copilot.sign_in.login_ended")["level"], "INFO");
        let failing = server_of(read_to_end(b"").await, "true", &[], "false");
        failing.start_login().await.expect("login");
        until_logged(&lines, "copilot.sign_in.login_failed").await;
        assert_eq!(lines.line("copilot.sign_in.login_failed")["exit_code"], 1);

        // A stop whose close fails and whose child ignores the quit signal, so outlives the deadline.
        let connection = Connection::open(tokio::io::empty(), CloseFails);
        let slow = server_of(connection, "sh", &["-c", "trap '' TERM; sleep 30"], "true");
        slow.stop(Duration::from_millis(50)).await;
        assert_eq!(
            lines.line("copilot.cli.stdin_close_failed")["io_kind"],
            "BrokenPipe"
        );
        assert_eq!(lines.line("copilot.cli.stop_timed_out")["deadline_ms"], 50);
        assert!(!lines.text().contains("copilot.cli.kill_failed"));
    }

    /// CPA-STOP-1: a CLI that does not end on its closed input but does on the quit signal stops
    /// before the deadline, with no kill.
    #[cfg(unix)]
    #[tokio::test]
    async fn stop_sends_the_quit_signal_before_the_deadline() {
        let lines = Lines::default();
        let _guard = lines.install();
        let connection = Connection::open(tokio::io::empty(), tokio::io::sink());
        let server = server_of(connection, "sleep", &["30"], "true");
        let started = std::time::Instant::now();
        server.stop(Duration::from_secs(10)).await;
        let took = started.elapsed();
        assert!(took >= STOP_TERMINATE_AFTER && took < Duration::from_secs(5), "{took:?}");
        assert!(!lines.text().contains("copilot.cli.stop_timed_out"));
    }
}
