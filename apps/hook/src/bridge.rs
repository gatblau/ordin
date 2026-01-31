//! G. `mcp-bridge` mode: a stdio MCP server connection carried to the MCP Front's own local socket,
//! authenticated by the session's MCP token. Bytes are copied, never parsed.

use std::io::{Read, Write};
use std::net::Shutdown;
use std::sync::mpsc;
use std::time::Duration;

use ordin_types::encoding::b64url_decode_exact;
use ordin_types::RunId;

use crate::channel::{connect, read_line_within, Conn};
use crate::env::{endpoint_ok, Lookup};
use crate::error::{HookFailure, BRIDGE_ENV_INVALID, BRIDGE_REFUSED};

/// Connect timeout, as F.1.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
/// Wait for the core's `ready` line (G.3).
pub const READY_TIMEOUT: Duration = Duration::from_secs(5);
/// After standard input ends, how long the connection may still deliver its last bytes (chosen here).
pub const DRAIN_TIMEOUT: Duration = Duration::from_secs(1);

pub struct BridgeEnv {
    pub endpoint: String,
    pub session: RunId,
    pub server: String,
    pub token: String,
}

/// `^[a-z][a-z0-9_]{0,63}$`
fn server_ok(server: &str) -> bool {
    let mut chars = server.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && server.len() <= 64
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

pub fn read_env(get: Lookup<'_>) -> Result<BridgeEnv, HookFailure> {
    let fail = HookFailure::bridge_env_invalid;
    let endpoint = get("ORDIN_MCP_ENDPOINT").ok_or_else(fail)?;
    if !endpoint_ok(&endpoint) {
        return Err(fail());
    }
    let session = RunId::parse(&get("ORDIN_SESSION_ID").ok_or_else(fail)?).map_err(|_| fail())?;
    let server = get("ORDIN_MCP_SERVER").ok_or_else(fail)?;
    if !server_ok(&server) {
        return Err(fail());
    }
    let token = get("ORDIN_MCP_TOKEN").ok_or_else(fail)?;
    b64url_decode_exact(&token, 32, BRIDGE_ENV_INVALID).map_err(|_| fail())?;
    Ok(BridgeEnv {
        endpoint,
        session,
        server,
        token,
    })
}

fn handshake_line(env: &BridgeEnv) -> String {
    let quote = |s: &str| serde_json::Value::String(s.to_string()).to_string();
    format!(
        "{{\"protocol\":1,\"session\":{},\"server\":{},\"token\":{}}}\n",
        quote(&env.session.to_string()),
        quote(&env.server),
        quote(&env.token)
    )
}

fn refusal(line: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    if value.get("kind")?.as_str()? == "ready" {
        return None;
    }
    let code = value.get("code").and_then(|c| c.as_str()).unwrap_or("");
    let shaped = code.starts_with("MCP_")
        && code.len() <= 64
        && code
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    Some(if shaped { code } else { BRIDGE_REFUSED }.to_string())
}

fn copy_flushing(from: &mut dyn Read, to: &mut dyn Write) -> std::io::Result<()> {
    let mut buf = [0u8; 8192];
    loop {
        let n = match from.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        to.write_all(&buf[..n])?;
        to.flush()?;
    }
}

enum Ended {
    Up(std::io::Result<()>),
    Down(std::io::Result<()>),
}

fn broken() -> HookFailure {
    HookFailure::new(
        BRIDGE_REFUSED,
        "the connection to ordin ended with an error",
    )
}

/// Runs the bridge until either side ends. The core's refusal code goes to `stderr`; the token never does.
pub fn run(
    env: &BridgeEnv,
    mut stdin: Box<dyn Read + Send>,
    mut stdout: Box<dyn Write + Send>,
    stderr: &mut dyn Write,
) -> Result<(), HookFailure> {
    let Conn {
        reader,
        mut writer,
        shutdown,
    } = connect(&env.endpoint, CONNECT_TIMEOUT)?;
    let sent = writer
        .write_all(handshake_line(env).as_bytes())
        .and_then(|_| writer.flush());
    if sent.is_err() {
        // The core closed the connection before the handshake: G.3, a closed connection.
        let _ = writeln!(stderr, "{BRIDGE_REFUSED}");
        return Err(broken());
    }
    let Some((line, mut buffered)) = read_line_within(reader, READY_TIMEOUT) else {
        let _ = writeln!(stderr, "{BRIDGE_REFUSED}");
        return Err(broken());
    };
    if let Some(code) = refusal(&line) {
        let _ = writeln!(stderr, "{code}");
        return Err(broken());
    }

    let (tx, rx) = mpsc::channel();
    let up = tx.clone();
    std::thread::spawn(move || {
        let _ = up.send(Ended::Up(copy_flushing(&mut stdin, &mut writer)));
    });
    std::thread::spawn(move || {
        let _ = tx.send(Ended::Down(copy_flushing(&mut buffered, &mut stdout)));
    });

    let result = match rx.recv() {
        Ok(Ended::Down(Ok(()))) => Ok(()),
        Ok(Ended::Up(Ok(()))) => {
            // Standard input ended: tell the core, then let its last bytes arrive. On Windows a pipe
            // client cannot close one direction, so the core learns of the end when the connection
            // closes after the drain below.
            // spec: ordin-desktop/components/OrdinHook#SA-HOOK-PIPE-02
            shutdown(Shutdown::Write);
            match rx.recv_timeout(DRAIN_TIMEOUT) {
                Ok(Ended::Down(Err(_))) => Err(broken()),
                _ => Ok(()),
            }
        }
        _ => Err(broken()),
    };
    shutdown(Shutdown::Both);
    result
}
