//! The bridge protocol, core end (McpFront.md *Bridge protocol* and Internal Logic B and D).
//!
//! The listener is a Unix domain socket in the application data root; a connection is served by
//! `handle_connection`, which is generic over the stream so that it is exercised in memory too.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::call::{run_call, ProgressRelay};
use crate::error::McpError;
use crate::front::{Conn, Inner, Session};
use crate::names::ServerName;
use crate::provider::{CallVia, ToolKey};
use crate::types::ToolCallResult;
use crate::util::lock;

/// The handshake line is at most 1 KiB and must arrive within 5 s (Internal Logic 9).
const HANDSHAKE_MAX: usize = 1024;
const HANDSHAKE_WAIT: Duration = Duration::from_secs(5);
/// A relayed line is at most 4 MiB: the 1 MiB argument limit plus the envelope (Internal Logic 10).
const LINE_MAX: usize = 4 * 1024 * 1024;
/// Bridge protocol version.
const PROTOCOL: u64 = 1;

#[derive(Debug)]
enum LineError {
    Eof,
    TooLong,
    Io,
}

/// Reads one line of at most `max` bytes without the line break; an unterminated final line is
/// returned as a line.
async fn read_line_limited<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max: usize,
) -> Result<Vec<u8>, LineError> {
    let mut out: Vec<u8> = Vec::new();
    loop {
        let buf = reader.fill_buf().await.map_err(|_| LineError::Io)?;
        if buf.is_empty() {
            return if out.is_empty() {
                Err(LineError::Eof)
            } else {
                Ok(out)
            };
        }
        match buf.iter().position(|b| *b == b'\n') {
            Some(at) => {
                if out.len() + at > max {
                    return Err(LineError::TooLong);
                }
                out.extend_from_slice(&buf[..at]);
                reader.consume(at + 1);
                if out.last() == Some(&b'\r') {
                    out.pop();
                }
                return Ok(out);
            }
            None => {
                let len = buf.len();
                if out.len() + len > max {
                    return Err(LineError::TooLong);
                }
                out.extend_from_slice(buf);
                reader.consume(len);
            }
        }
    }
}

/// Step 8 of Internal Logic D: the peer must be the process's own user.
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn peer_allowed(peer_uid: Option<u32>, own_uid: Option<u32>) -> bool {
    matches!((peer_uid, own_uid), (Some(peer), Some(own)) if peer == own)
}

/// Compares two digests by a fixed-time XOR fold over the 32 bytes.
fn digests_equal(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

fn refuse_log(run_id: Option<&str>, server: Option<&str>, code: &str) {
    tracing::warn!(
        component = "mcp_front",
        run_id = run_id.unwrap_or(""),
        server = server.unwrap_or(""),
        code,
        "mcp_front.bridge_refused"
    );
}

async fn refuse<W: AsyncWrite + Unpin>(writer: &mut W, code: &str) {
    let line = format!("{}\n", json!({"kind": "refused", "code": code}));
    let _ = writer.write_all(line.as_bytes()).await;
    let _ = writer.shutdown().await;
}

/// Serves one accepted connection from the handshake to its close.
/// One bridge connection. `peer_ok` is the transport's peer check: on Unix the peer's user id equals
/// the core's ([`peer_allowed`]); on Windows the pipe's security descriptor admits the current user
/// only, so every connected peer is that user.
pub(crate) async fn handle_connection<S>(inner: Arc<Inner>, stream: S, peer_ok: bool)
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    // 8. Peer check; a refused peer is closed without a byte read or written.
    // spec: ordin-desktop/components/McpFront#SA-MCP-01
    if !peer_ok {
        refuse_log(None, None, "MCP_BRIDGE_PEER_REFUSED");
        return;
    }
    let (read_half, mut write_half) = tokio::io::split(stream);
    let mut reader = BufReader::new(read_half);

    // 9. Handshake.
    let line = match tokio::time::timeout(
        HANDSHAKE_WAIT,
        read_line_limited(&mut reader, HANDSHAKE_MAX),
    )
    .await
    {
        Ok(Ok(line)) => line,
        _ => {
            refuse_log(None, None, "MCP_BRIDGE_MALFORMED");
            refuse(&mut write_half, "MCP_BRIDGE_MALFORMED").await;
            return;
        }
    };
    let hello: Value = match serde_json::from_slice(&line) {
        Ok(v) => v,
        Err(_) => {
            refuse_log(None, None, "MCP_BRIDGE_MALFORMED");
            refuse(&mut write_half, "MCP_BRIDGE_MALFORMED").await;
            return;
        }
    };
    let (protocol, session_text, server_text, token) = (
        hello.get("protocol"),
        hello.get("session").and_then(Value::as_str),
        hello.get("server").and_then(Value::as_str),
        hello.get("token").and_then(Value::as_str),
    );
    let (Some(protocol), Some(session_text), Some(server_text), Some(token)) =
        (protocol, session_text, server_text, token)
    else {
        refuse_log(None, None, "MCP_BRIDGE_MALFORMED");
        refuse(&mut write_half, "MCP_BRIDGE_MALFORMED").await;
        return;
    };
    if protocol.as_u64() != Some(PROTOCOL) {
        refuse_log(
            Some(session_text),
            Some(server_text),
            "MCP_BRIDGE_PROTOCOL_UNSUPPORTED",
        );
        refuse(&mut write_half, "MCP_BRIDGE_PROTOCOL_UNSUPPORTED").await;
        return;
    }
    let session = ordin_types::RunId::parse(session_text)
        .ok()
        .and_then(|run_id| inner.session(run_id));
    let Some(session) = session else {
        refuse_log(Some(session_text), Some(server_text), "MCP_SESSION_UNKNOWN");
        refuse(&mut write_half, "MCP_SESSION_UNKNOWN").await;
        return;
    };
    let presented: [u8; 32] = Sha256::digest(token.as_bytes()).into();
    if !digests_equal(&presented, &session.token_digest) {
        // The presented token is never logged.
        refuse_log(
            Some(session_text),
            Some(server_text),
            "MCP_BRIDGE_UNAUTHENTICATED",
        );
        refuse(&mut write_half, "MCP_BRIDGE_UNAUTHENTICATED").await;
        return;
    }
    let server = match ServerName::new(server_text) {
        Ok(server) if session.launched.contains(&server) => server,
        _ => {
            refuse_log(
                Some(session_text),
                Some(server_text),
                "MCP_SERVER_NOT_OFFERED",
            );
            refuse(&mut write_half, "MCP_SERVER_NOT_OFFERED").await;
            return;
        }
    };

    // 10. Ready, then MCP JSON-RPC lines.
    if write_half
        .write_all(b"{\"kind\":\"ready\"}\n")
        .await
        .is_err()
    {
        return;
    }
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let conn_cancel = session.cancel.child_token();
    let conn_id = session.next_conn.fetch_add(1, Ordering::SeqCst);
    lock(&session.conns).push(Conn {
        id: conn_id,
        server: server.clone(),
        tx: tx.clone(),
    });
    // The writer ends when every sender is gone: the reader loop, the session's list and each
    // in-flight call hold one, so a call ended by shutdown still delivers its answer.
    let writer = tokio::spawn(async move {
        while let Some(line) = rx.recv().await {
            if write_half.write_all(line.as_bytes()).await.is_err()
                || write_half.write_all(b"\n").await.is_err()
                || write_half.flush().await.is_err()
            {
                break;
            }
        }
        let _ = write_half.shutdown().await;
    });

    let inflight: Arc<Mutex<HashMap<String, CancellationToken>>> =
        Arc::new(Mutex::new(HashMap::new()));
    loop {
        let line = tokio::select! {
            _ = conn_cancel.cancelled() => break,
            line = read_line_limited(&mut reader, LINE_MAX) => line,
        };
        let bytes = match line {
            Ok(bytes) => bytes,
            Err(LineError::TooLong) => {
                refuse_log(
                    Some(session_text),
                    Some(server_text),
                    "MCP_BRIDGE_MALFORMED",
                );
                break;
            }
            Err(LineError::Eof | LineError::Io) => break,
        };
        if bytes.is_empty() {
            continue;
        }
        serve_line(
            &inner,
            &session,
            &server,
            &tx,
            &inflight,
            &conn_cancel,
            &bytes,
        )
        .await;
    }
    // The connection is over: its calls are cancelled and it leaves the session's list.
    conn_cancel.cancel();
    lock(&session.conns).retain(|c| c.id != conn_id);
    drop(tx);
    let _ = writer.await;
}

fn rpc_error(id: Value, code: i64, message: &str) -> String {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}).to_string()
}

fn rpc_result(id: Value, result: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string()
}

/// Answers one JSON-RPC line.
async fn serve_line(
    inner: &Arc<Inner>,
    session: &Arc<Session>,
    server: &ServerName,
    tx: &mpsc::UnboundedSender<String>,
    inflight: &Arc<Mutex<HashMap<String, CancellationToken>>>,
    conn_cancel: &CancellationToken,
    bytes: &[u8],
) {
    let Ok(message) = serde_json::from_slice::<Value>(bytes) else {
        let _ = tx.send(rpc_error(Value::Null, -32700, "parse error"));
        return;
    };
    let Some(object) = message.as_object() else {
        let _ = tx.send(rpc_error(Value::Null, -32600, "invalid request"));
        return;
    };
    let id = object.get("id").cloned();
    let method = object.get("method").and_then(Value::as_str).unwrap_or("");
    let params = object.get("params").cloned().unwrap_or(Value::Null);
    match method {
        "initialize" => {
            let Some(id) = id else { return };
            let version = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("2025-06-18");
            let _ = tx.send(rpc_result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": true}},
                    "serverInfo": {"name": server.as_str(), "version": env!("CARGO_PKG_VERSION")},
                }),
            ));
        }
        "notifications/initialized" => {}
        "notifications/cancelled" => {
            if let Some(request) = params.get("requestId") {
                if let Some(token) = lock(inflight).get(&request.to_string()) {
                    token.cancel();
                }
            }
        }
        "ping" => {
            if let Some(id) = id {
                let _ = tx.send(rpc_result(id, json!({})));
            }
        }
        "tools/list" => {
            let Some(id) = id else { return };
            let tools: Vec<Value> = lock(&session.offered)
                .iter()
                .filter(|(key, _)| &key.server == server)
                .map(|(key, (_, spec))| {
                    json!({
                        "name": key.tool,
                        "description": spec.description,
                        "inputSchema": spec.input_schema,
                    })
                })
                .collect();
            let _ = tx.send(rpc_result(id, json!({ "tools": tools })));
        }
        "tools/call" => {
            let Some(id) = id else { return };
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                let _ = tx.send(rpc_error(id, -32602, "invalid params"));
                return;
            };
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let relay = params
                .get("_meta")
                .and_then(|m| m.get("progressToken"))
                .map(|token| ProgressRelay {
                    token: token.clone(),
                    tx: tx.clone(),
                });
            let key = ToolKey {
                server: server.clone(),
                tool: name.to_owned(),
            };
            let cancel = conn_cancel.child_token();
            let id_key = id.to_string();
            lock(inflight).insert(id_key.clone(), cancel.clone());
            let (inner, session, tx, inflight) =
                (inner.clone(), session.clone(), tx.clone(), inflight.clone());
            tokio::spawn(async move {
                let result = run_call(
                    &inner,
                    &session,
                    key,
                    arguments,
                    CallVia::Harness,
                    Some(cancel),
                    relay,
                )
                .await;
                lock(&inflight).remove(&id_key);
                // A cancelled call, or one ended by session close, gets no answer (MCP cancellation).
                let silent = matches!(
                    result.code.as_deref(),
                    Some("MCP_CALL_CANCELLED" | "MCP_SESSION_CLOSED")
                );
                if !silent {
                    let _ = tx.send(rpc_result(id, call_result_json(&result)));
                }
            });
        }
        _ => {
            if let Some(id) = id {
                let _ = tx.send(rpc_error(id, -32601, "method not found"));
            }
        }
    }
}

/// The MCP `tools/call` result of a `ToolCallResult`.
fn call_result_json(result: &ToolCallResult) -> Value {
    let mut out = json!({
        "content": [{"type": "text", "text": result.text}],
        "isError": result.is_error,
    });
    if let (Some(structured), Some(map)) = (&result.structured, out.as_object_mut()) {
        map.insert("structuredContent".to_owned(), structured.clone());
    }
    out
}

// ---- listener ----

#[cfg(unix)]
pub(crate) type Listener = tokio::net::UnixListener;
#[cfg(windows)]
pub(crate) type Listener = pipe::Pipe;
#[cfg(not(any(unix, windows)))]
pub(crate) type Listener = ();

/// Creates the socket folder and binds the listener (Internal Logic 3).
#[cfg(unix)]
pub(crate) async fn bind(inner: &Arc<Inner>) -> Result<Listener, McpError> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};

    let unavailable = |error: std::io::Error| McpError::SocketUnavailable {
        detail: format!("{:?}", error.kind()),
    };
    let run_dir = inner.config.app_data_root.join("run");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&run_dir)
        .map_err(unavailable)?;
    let own_uid = std::fs::metadata(&run_dir).map_err(unavailable)?.uid();
    let path = run_dir.join("mcp.sock");
    if path.exists() {
        // A stale socket is removed only when nothing accepts on it.
        if tokio::net::UnixStream::connect(&path).await.is_ok() {
            return Err(McpError::SocketUnavailable {
                detail: format!("{:?}", std::io::ErrorKind::AddrInUse),
            });
        }
        std::fs::remove_file(&path).map_err(unavailable)?;
    }
    let listener = tokio::net::UnixListener::bind(&path).map_err(unavailable)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(unavailable)?;
    let _ = inner.own_uid.set(own_uid);
    let _ = inner.endpoint.set(path.to_string_lossy().into_owned());
    Ok(listener)
}

/// Creates the named pipe `\\.\pipe\ordin-mcp-<installation id>` with a security descriptor that
/// grants the current user only, refusing remote clients (Internal Logic 3).
// spec: ordin-desktop/components/McpFront#SA-MCP-01
#[cfg(windows)]
pub(crate) async fn bind(inner: &Arc<Inner>) -> Result<Listener, McpError> {
    let name = format!(r"\\.\pipe\ordin-mcp-{}", inner.config.installation_id);
    let pipe = pipe::Pipe::create(name).map_err(|error| McpError::SocketUnavailable {
        detail: format!("{:?}", error.kind()),
    })?;
    let _ = inner.endpoint.set(pipe.name.clone());
    Ok(pipe)
}

/// Neither Unix nor Windows: no local channel exists.
#[cfg(not(any(unix, windows)))]
pub(crate) async fn bind(inner: &Arc<Inner>) -> Result<Listener, McpError> {
    let _ = inner.config.installation_id;
    Err(McpError::SocketUnavailable {
        detail: "no local channel on this platform".to_owned(),
    })
}

/// The Windows named pipe with its current-user-only security descriptor.
#[cfg(windows)]
pub(crate) mod pipe {
    use std::ffi::c_void;

    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};

    /// A protected DACL with one entry: full access for the object's owner, the user who created
    /// the pipe (Owner Rights). No other account, administrators included, is granted access.
    const CURRENT_USER_ONLY: &str = "D:P(A;;GA;;;OW)";

    /// The security descriptor, freed on drop.
    struct Descriptor(PSECURITY_DESCRIPTOR);

    // SAFETY: the descriptor is written once by `ConvertStringSecurityDescriptorToSecurityDescriptorW`
    // and only read afterwards, by `CreateNamedPipeW` through tokio; nothing mutates it.
    unsafe impl Send for Descriptor {}
    // SAFETY: as for `Send`: shared reads of an immutable descriptor.
    unsafe impl Sync for Descriptor {}

    impl Drop for Descriptor {
        fn drop(&mut self) {
            // SAFETY: the pointer was allocated by `ConvertStringSecurityDescriptorToSecurityDescriptorW`
            // with `LocalAlloc` and is freed once, here.
            unsafe {
                LocalFree(self.0);
            }
        }
    }

    impl Descriptor {
        fn current_user_only() -> std::io::Result<Descriptor> {
            let sddl: Vec<u16> = CURRENT_USER_ONLY.encode_utf16().chain(Some(0)).collect();
            let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
            // SAFETY: `sddl` is a NUL-terminated UTF-16 string that outlives the call; `descriptor`
            // is a valid out pointer; the size out pointer may be null.
            let ok = unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 || descriptor.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            Ok(Descriptor(descriptor))
        }
    }

    /// The pipe's name, its descriptor, and the instance waiting for the next connection.
    pub(crate) struct Pipe {
        pub(crate) name: String,
        descriptor: Descriptor,
        pub(crate) server: NamedPipeServer,
    }

    impl Pipe {
        /// The first instance: it fails when another process already owns the name.
        pub(crate) fn create(name: String) -> std::io::Result<Pipe> {
            let descriptor = Descriptor::current_user_only()?;
            let server = instance(&name, &descriptor, true)?;
            Ok(Pipe {
                name,
                descriptor,
                server,
            })
        }

        /// The next instance, created before the connected one is handed off.
        pub(crate) fn next(&self) -> std::io::Result<NamedPipeServer> {
            instance(&self.name, &self.descriptor, false)
        }
    }

    fn instance(
        name: &str,
        descriptor: &Descriptor,
        first: bool,
    ) -> std::io::Result<NamedPipeServer> {
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        let mut options = ServerOptions::new();
        options
            .first_pipe_instance(first)
            .reject_remote_clients(true);
        // SAFETY: `attributes` is a valid SECURITY_ATTRIBUTES whose descriptor lives as long as
        // `descriptor`, which outlives this call; tokio passes the pointer to `CreateNamedPipeW`.
        unsafe {
            options.create_with_security_attributes_raw(
                name,
                &mut attributes as *mut SECURITY_ATTRIBUTES as *mut c_void,
            )
        }
    }
}

#[cfg(unix)]
pub(crate) fn spawn_accept_loop(
    inner: Arc<Inner>,
    listener: Listener,
    shutdown: CancellationToken,
) {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                accepted = listener.accept() => match accepted {
                    Ok((stream, _)) => {
                        let peer_uid = stream.peer_cred().ok().map(|c| c.uid());
                        let peer_ok = peer_allowed(peer_uid, inner.own_uid.get().copied());
                        tokio::spawn(handle_connection(inner.clone(), stream, peer_ok));
                    }
                    Err(error) => {
                        tracing::warn!(
                            component = "mcp_front",
                            detail = format!("{:?}", error.kind()).as_str(),
                            "mcp_front.accept_failed"
                        );
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                },
            }
        }
        // Stop accepting, end every call (their tokens are children of `shutdown`), remove the socket.
        if let Some(path) = inner.endpoint.get() {
            let _ = std::fs::remove_file(path);
        }
        tracing::info!(component = "mcp_front", "mcp_front.stopped");
    });
}

/// Windows: one new pipe instance is made before each connected one is served; the pipe's security
/// descriptor already admits the current user only.
#[cfg(windows)]
pub(crate) fn spawn_accept_loop(
    inner: Arc<Inner>,
    mut listener: Listener,
    shutdown: CancellationToken,
) {
    tokio::spawn(async move {
        loop {
            let connected = tokio::select! {
                _ = shutdown.cancelled() => break,
                connected = listener.server.connect() => connected,
            };
            if let Err(error) = connected {
                tracing::warn!(
                    component = "mcp_front",
                    detail = format!("{:?}", error.kind()).as_str(),
                    "mcp_front.accept_failed"
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
            let next = match listener.next() {
                Ok(next) => next,
                Err(error) => {
                    tracing::warn!(
                        component = "mcp_front",
                        detail = format!("{:?}", error.kind()).as_str(),
                        "mcp_front.accept_failed"
                    );
                    break;
                }
            };
            let stream = std::mem::replace(&mut listener.server, next);
            tokio::spawn(handle_connection(inner.clone(), stream, true));
        }
        tracing::info!(component = "mcp_front", "mcp_front.stopped");
    });
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn spawn_accept_loop(
    _inner: Arc<Inner>,
    _listener: Listener,
    _shutdown: CancellationToken,
) {
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lines_are_limited_and_split() {
        let data: &[u8] = b"abc\r\nde\nfgh";
        let mut reader = BufReader::new(data);
        assert_eq!(read_line_limited(&mut reader, 10).await.unwrap(), b"abc");
        assert_eq!(read_line_limited(&mut reader, 10).await.unwrap(), b"de");
        assert_eq!(read_line_limited(&mut reader, 10).await.unwrap(), b"fgh");
        assert!(matches!(
            read_line_limited(&mut reader, 10).await,
            Err(LineError::Eof)
        ));
        let long: &[u8] = b"0123456789abc\n";
        let mut reader = BufReader::new(long);
        assert!(matches!(
            read_line_limited(&mut reader, 5).await,
            Err(LineError::TooLong)
        ));
    }

    /// Error Table: the bridge peer is another OS user, so the connection is closed unread. A peer
    /// of another user cannot be made in a test, so the decision the connection handler takes is
    /// checked directly: only an equal, known user id is admitted.
    #[test]
    fn error_table_bridge_peer_is_another_user() {
        assert!(peer_allowed(Some(5), Some(5)));
        assert!(!peer_allowed(Some(5), Some(6)));
        assert!(!peer_allowed(None, Some(5)));
        assert!(!peer_allowed(Some(5), None));
    }

    #[test]
    fn digests_compare_by_a_fixed_time_fold() {
        let a = [1u8; 32];
        let mut b = a;
        assert!(digests_equal(&a, &b));
        b[31] = 2;
        assert!(!digests_equal(&a, &b));
    }

    #[test]
    fn call_results_carry_structured_content_only_when_present() {
        let plain = call_result_json(&ToolCallResult {
            is_error: false,
            text: "hi".into(),
            structured: None,
            code: None,
        });
        assert!(plain.get("structuredContent").is_none());
        assert_eq!(plain["content"][0]["text"], "hi");
        let rich = call_result_json(&ToolCallResult {
            is_error: true,
            text: "x".into(),
            structured: Some(json!({"a": 1})),
            code: Some("C".into()),
        });
        assert_eq!(rich["structuredContent"]["a"], 1);
        assert_eq!(rich["isError"], true);
    }
}
