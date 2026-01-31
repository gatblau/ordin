//! Shared test helpers: captured streams, a stand-in relay, environments and timings.

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use ordin_types::encoding::b64url_encode;
use ordin_types::wire::hook::HookRequest;
use ordin_types::{AgentInstanceId, RunId};

use crate::app::{self, Io, Timings};
use crate::channel::read_line;

pub const SECRET: [u8; 32] = [7u8; 32];

#[derive(Clone, Default)]
pub struct Sink(pub Arc<Mutex<Vec<u8>>>);

impl Sink {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).to_string()
    }
}

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A writer that always fails.
pub struct Broken;

impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("closed"))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A short unique path (macOS limits socket paths to 104 bytes).
pub fn unique_path(stem: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!("{stem}{}-{n}", std::process::id()))
}

/// An endpoint nothing listens on: a socket path on macOS and Linux, a pipe name on Windows (the
/// hook refuses any other form there before connecting).
pub fn unused_endpoint(stem: &str) -> String {
    if cfg!(windows) {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        format!(r"\\.\pipe\{stem}{}-{n}", std::process::id())
    } else {
        unique_path(stem).to_string_lossy().to_string()
    }
}

pub fn good_env(endpoint: &str) -> HashMap<String, String> {
    HashMap::from([
        ("ORDIN_HOOK_ENDPOINT".into(), endpoint.to_string()),
        ("ORDIN_HOOK_SECRET".into(), b64url_encode(&SECRET)),
        ("ORDIN_SESSION_ID".into(), RunId::new().to_string()),
        ("ORDIN_HOOK_PROTOCOL".into(), "1".into()),
    ])
}

pub fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

pub fn hook_args(harness: &str, event: &str) -> Vec<String> {
    let agent = AgentInstanceId::new().to_string();
    args(&[
        "--harness",
        harness,
        "--event",
        event,
        "--agent-instance",
        &agent,
    ])
}

pub struct Outcome {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

pub fn run_with_input(
    args: &[String],
    env: &HashMap<String, String>,
    stdin: Box<dyn Read + Send>,
    timings: &Timings,
) -> Outcome {
    let out = Sink::default();
    let err = Sink::default();
    let io = Io {
        stdin,
        stdout: Box::new(out.clone()),
        stderr: Box::new(err.clone()),
    };
    let get = |name: &str| env.get(name).cloned();
    let status = app::run(args, &get, io, timings);
    Outcome {
        status,
        stdout: out.text(),
        stderr: err.text(),
    }
}

pub fn run_hook(
    args: &[String],
    env: &HashMap<String, String>,
    stdin: &str,
    timings: &Timings,
) -> Outcome {
    run_with_input(
        args,
        env,
        Box::new(Cursor::new(stdin.as_bytes().to_vec())),
        timings,
    )
}

pub fn quick() -> Timings {
    Timings {
        overall: Duration::from_secs(5),
        connect: Duration::from_millis(500),
        first_reply: Duration::from_secs(2),
    }
}

pub const READ_PAYLOAD: &str = r#"{"session_id":"s","hook_event_name":"PreToolUse","tool_name":"Read","tool_input":{"file_path":"a.txt"},"tool_use_id":"toolu_1"}"#;

/// The stream a stand-in holds: a Unix domain socket on macOS and Linux, the server end of a named
/// pipe on Windows — the two channels `channel::connect` opens.
#[cfg(unix)]
pub type Stream = std::os::unix::net::UnixStream;
#[cfg(windows)]
pub type Stream = std::fs::File;

/// A stand-in's local endpoint, listening from the moment it is bound, for one connection.
pub struct Listener {
    pub endpoint: String,
    #[cfg(unix)]
    inner: std::os::unix::net::UnixListener,
    #[cfg(windows)]
    inner: std::os::windows::io::OwnedHandle,
}

impl Listener {
    /// Unix: a socket at a short unique path. Windows: a pipe `\\.\pipe\<stem><pid>-<n>`.
    #[cfg(unix)]
    pub fn bind(stem: &str) -> Listener {
        let path = unique_path(stem);
        let inner = std::os::unix::net::UnixListener::bind(&path).expect("bind");
        let endpoint = path.to_string_lossy().to_string();
        Listener { endpoint, inner }
    }

    #[cfg(windows)]
    pub fn bind(stem: &str) -> Listener {
        use std::os::windows::io::{FromRawHandle, OwnedHandle};
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
        use windows_sys::Win32::System::Pipes::{
            CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
            PIPE_WAIT,
        };
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let endpoint = format!(r"\\.\pipe\{stem}{}-{n}", std::process::id());
        let wide: Vec<u16> = endpoint.encode_utf16().chain(Some(0)).collect();
        // SAFETY: `wide` is a NUL-terminated UTF-16 name that outlives the call, and a null security
        // attributes pointer asks for the default descriptor.
        let handle = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                64 * 1024,
                64 * 1024,
                0,
                std::ptr::null(),
            )
        };
        assert!(
            handle != INVALID_HANDLE_VALUE,
            "bind: {}",
            std::io::Error::last_os_error()
        );
        // SAFETY: `handle` is a valid pipe handle owned by nothing else.
        let inner = unsafe { OwnedHandle::from_raw_handle(handle) };
        Listener { endpoint, inner }
    }

    /// Waits for the one connection.
    #[cfg(unix)]
    pub fn accept(self) -> std::io::Result<Stream> {
        self.inner.accept().map(|(stream, _)| stream)
    }

    #[cfg(windows)]
    pub fn accept(self) -> std::io::Result<Stream> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::ERROR_PIPE_CONNECTED;
        use windows_sys::Win32::System::Pipes::ConnectNamedPipe;
        // SAFETY: the handle is a valid pipe handle, and a null OVERLAPPED makes the call block.
        let connected =
            unsafe { ConnectNamedPipe(self.inner.as_raw_handle(), std::ptr::null_mut()) };
        let error = std::io::Error::last_os_error();
        // A client that opened the pipe before this call is reported as ERROR_PIPE_CONNECTED.
        if connected == 0 && error.raw_os_error() != Some(ERROR_PIPE_CONNECTED as i32) {
            return Err(error);
        }
        let Listener { inner, .. } = self;
        Ok(std::fs::File::from(inner))
    }
}

#[cfg(unix)]
impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.endpoint);
    }
}

/// A stand-in relay: accepts one connection, parses the request line and hands both to `script`.
pub struct Relay {
    pub endpoint: String,
    pub handle: JoinHandle<Option<HookRequest>>,
}

impl Relay {
    pub fn start<F>(script: F) -> Relay
    where
        F: FnOnce(&mut Stream, &HookRequest) + Send + 'static,
    {
        let listener = Listener::bind("oh");
        let endpoint = listener.endpoint.clone();
        let handle = std::thread::spawn(move || {
            let mut stream = listener.accept().ok()?;
            let mut reader = std::io::BufReader::new(stream.try_clone().ok()?);
            let line = read_line(&mut reader).ok()??;
            let request: HookRequest = serde_json::from_str(&line).ok()?;
            script(&mut stream, &request);
            Some(request)
        });
        Relay { endpoint, handle }
    }

    pub fn finish(self) -> HookRequest {
        self.handle
            .join()
            .expect("relay thread")
            .expect("a request arrived")
    }
}

pub fn send(stream: &mut Stream, value: serde_json::Value) {
    let mut line = value.to_string();
    line.push('\n');
    stream.write_all(line.as_bytes()).expect("reply written");
}

pub fn reply_allow(request: &HookRequest) -> serde_json::Value {
    serde_json::json!({"kind": "allow", "call_id": request.call_id.to_string()})
}

pub fn reply_deny(request: &HookRequest, reason: &str) -> serde_json::Value {
    serde_json::json!({"kind": "deny", "call_id": request.call_id.to_string(), "reason": reason})
}

pub fn reply_ack(request: &HookRequest) -> serde_json::Value {
    serde_json::json!({"kind": "ack", "call_id": request.call_id.to_string()})
}

pub fn reply_holding(request: &HookRequest, deadline_in: Duration) -> serde_json::Value {
    let delta = chrono::Duration::milliseconds(deadline_in.as_millis() as i64);
    let deadline = ordin_types::time::Timestamp(ordin_types::time::Timestamp::now().0 + delta);
    serde_json::json!({
        "kind": "holding",
        "call_id": request.call_id.to_string(),
        "approval_id": ordin_types::ApprovalId::new().to_string(),
        "deadline": deadline.to_string(),
    })
}
