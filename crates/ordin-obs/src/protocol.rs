//! A small obs-websocket v5 client (ObsRecorderAdapter.md Internal Logic B): one connection per
//! operation, closed when the operation ends.

use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use futures::{SinkExt, StreamExt};
use ordin_identity::SecretString;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::error::ObsError;

/// The subprotocol that asks for JSON frames.
const SUBPROTOCOL: &str = "obswebsocket.json";
/// The only RPC version spoken.
const RPC_VERSION: u64 = 1;
/// OBS closes with this code when the authentication string is wrong.
const CLOSE_AUTHENTICATION_FAILED: u16 = 4009;
/// OBS closes with this code when it does not speak the RPC version asked for.
const CLOSE_UNSUPPORTED_RPC_VERSION: u16 = 4010;
/// Frames and messages larger than this are refused (*Security*).
const MAX_FRAME_BYTES: usize = 1024 * 1024;

const OP_HELLO: u64 = 0;
const OP_IDENTIFY: u64 = 1;
const OP_IDENTIFIED: u64 = 2;
const OP_REQUEST: u64 = 6;
const OP_REQUEST_RESPONSE: u64 = 7;

/// The time limits of Internal Logic B; the tests shorten them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    pub connect: Duration,
    pub handshake: Duration,
    pub request: Duration,
    pub stop_record: Duration,
    /// How long OBS may take to finish writing the file after StopRecord (D2).
    pub output_wait: Duration,
    /// How long the file's size must stay the same before it counts as finished (OBS-STOP-1).
    pub output_settled: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            connect: Duration::from_secs(3),
            handshake: Duration::from_secs(3),
            request: Duration::from_secs(5),
            stop_record: Duration::from_secs(15),
            output_wait: Duration::from_secs(10),
            output_settled: Duration::from_secs(1),
        }
    }
}

/// B2: `base64(sha256(base64(sha256(password + salt)) + challenge))`. The parts are hashed one
/// after the other, so no joined copy of the password is made.
pub(crate) fn authentication(password: &str, salt: &str, challenge: &str) -> String {
    let secret = STANDARD.encode(
        Sha256::new()
            .chain_update(password.as_bytes())
            .chain_update(salt.as_bytes())
            .finalize(),
    );
    STANDARD.encode(
        Sha256::new()
            .chain_update(secret.as_bytes())
            .chain_update(challenge.as_bytes())
            .finalize(),
    )
}

/// What the `Hello` asks for.
pub(crate) struct Hello {
    pub authentication: Option<(String, String)>,
}

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// One identified connection.
pub(crate) struct Session {
    socket: Socket,
    limits: Limits,
}

/// B1: connects to `127.0.0.1:<port>` only, and reads the `Hello`.
pub(crate) async fn connect(port: u16, limits: Limits) -> Result<(Socket, Hello), ObsError> {
    let mut request = format!("ws://127.0.0.1:{port}")
        .into_client_request()
        .map_err(|_| ObsError::NotReachable)?;
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        HeaderValue::from_static(SUBPROTOCOL),
    );
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME_BYTES))
        .max_frame_size(Some(MAX_FRAME_BYTES));
    let connecting = tokio_tungstenite::connect_async_with_config(request, Some(config), true);
    let (mut socket, _) = tokio::time::timeout(limits.connect, connecting)
        .await
        .map_err(|_| ObsError::NotReachable)?
        .map_err(|_| ObsError::NotReachable)?;
    let hello = tokio::time::timeout(limits.handshake, next_op(&mut socket, OP_HELLO))
        .await
        .map_err(|_| ObsError::Timeout)??;
    if hello.get("rpcVersion").and_then(Value::as_u64) != Some(RPC_VERSION) {
        return Err(ObsError::VersionUnsupported);
    }
    let authentication = match hello.get("authentication") {
        None | Some(Value::Null) => None,
        Some(auth) => {
            let field = |name: &str| auth.get(name).and_then(Value::as_str).map(str::to_owned);
            match (field("challenge"), field("salt")) {
                (Some(challenge), Some(salt)) => Some((challenge, salt)),
                _ => return Err(ObsError::Request(0)),
            }
        }
    };
    Ok((socket, Hello { authentication }))
}

impl Session {
    /// B2: sends `Identify`, with the authentication string when the `Hello` asked for one, and
    /// waits for `Identified`. The password is used for the hash and dropped with `password`.
    pub(crate) async fn identify(
        mut socket: Socket,
        hello: &Hello,
        password: Option<SecretString>,
        limits: Limits,
    ) -> Result<Session, ObsError> {
        let mut identify = json!({ "rpcVersion": RPC_VERSION, "eventSubscriptions": 0 });
        if let Some((challenge, salt)) = &hello.authentication {
            let password = password.ok_or(ObsError::PasswordNeeded)?;
            identify["authentication"] =
                Value::String(authentication(password.expose(), salt, challenge));
        }
        send(&mut socket, OP_IDENTIFY, identify).await?;
        tokio::time::timeout(limits.handshake, next_op(&mut socket, OP_IDENTIFIED))
            .await
            .map_err(|_| ObsError::Timeout)??;
        Ok(Session { socket, limits })
    }

    /// B3: one request with a UUIDv7 id and its answer's `responseData` (`Null` when absent).
    /// `result: false` → `Request(code)`; no answer within the limit → `Timeout`.
    pub(crate) async fn request(
        &mut self,
        request_type: &'static str,
        data: Option<Value>,
    ) -> Result<Value, ObsError> {
        let limit = if request_type == "StopRecord" {
            self.limits.stop_record
        } else {
            self.limits.request
        };
        let result = tokio::time::timeout(limit, self.exchange(request_type, data))
            .await
            .unwrap_or(Err(ObsError::Timeout));
        if let Err(error) = &result {
            tracing::warn!(
                component = "obs",
                code = ordin_types::CodedError::code(error),
                request_type,
                "obs.failed"
            );
        }
        result
    }

    async fn exchange(
        &mut self,
        request_type: &'static str,
        data: Option<Value>,
    ) -> Result<Value, ObsError> {
        let id = uuid::Uuid::now_v7().to_string();
        let mut body = json!({ "requestType": request_type, "requestId": id });
        if let Some(data) = data {
            body["requestData"] = data;
        }
        send(&mut self.socket, OP_REQUEST, body).await?;
        loop {
            let answer = next_op(&mut self.socket, OP_REQUEST_RESPONSE).await?;
            if answer.get("requestId").and_then(Value::as_str) != Some(id.as_str()) {
                continue;
            }
            let status = answer.get("requestStatus");
            let ok = status
                .and_then(|s| s.get("result"))
                .and_then(Value::as_bool)
                .ok_or(ObsError::Request(0))?;
            if !ok {
                let code = status
                    .and_then(|s| s.get("code"))
                    .and_then(Value::as_u64)
                    .and_then(|c| u16::try_from(c).ok())
                    .unwrap_or(0);
                return Err(ObsError::Request(code));
            }
            return Ok(answer.get("responseData").cloned().unwrap_or(Value::Null));
        }
    }

    /// Closes the connection; a failure to close changes nothing for the caller.
    pub(crate) async fn close(mut self) {
        let _ = self.socket.close(None).await;
    }
}

async fn send(socket: &mut Socket, op: u64, d: Value) -> Result<(), ObsError> {
    let frame = json!({ "op": op, "d": d }).to_string();
    socket
        .send(Message::text(frame))
        .await
        .map_err(|_| ObsError::NotReachable)
}

/// The `d` of the next frame with opcode `op`; frames with other opcodes (events) are skipped.
/// A close with code 4009 → `PasswordRefused`, 4010 → `VersionUnsupported`, any other end of the
/// connection → `NotReachable`.
async fn next_op(socket: &mut Socket, op: u64) -> Result<Value, ObsError> {
    while let Some(message) = socket.next().await {
        let message = message.map_err(|_| ObsError::NotReachable)?;
        match message {
            Message::Text(text) => {
                let Ok(frame) = serde_json::from_str::<Value>(text.as_str()) else {
                    continue;
                };
                if frame.get("op").and_then(Value::as_u64) == Some(op) {
                    return Ok(frame.get("d").cloned().unwrap_or(Value::Null));
                }
            }
            Message::Close(frame) => {
                return Err(match frame.map(|f| u16::from(f.code)) {
                    Some(CLOSE_AUTHENTICATION_FAILED) => ObsError::PasswordRefused,
                    Some(CLOSE_UNSUPPORTED_RPC_VERSION) => ObsError::VersionUnsupported,
                    _ => ObsError::NotReachable,
                });
            }
            _ => {}
        }
    }
    Err(ObsError::NotReachable)
}

/// `major.minor.patch` of a version string such as `30.2.0` or `30.2.0-rc1`; a missing or
/// unreadable part counts as 0.
pub(crate) fn version_parts(version: &str) -> (u64, u64, u64) {
    let mut parts = version.split('.').map(|part| {
        part.chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse::<u64>()
            .unwrap_or(0)
    });
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}
