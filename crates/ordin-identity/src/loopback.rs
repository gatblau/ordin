//! The loopback redirect listener of the browser method (Internal Logic C.5; ADR-072): `127.0.0.1`
//! only, the first free port of 47821–47823, one `GET /callback` answered, closed after use or at the
//! challenge's expiry.

use std::collections::HashMap;

use subtle::ConstantTimeEq as _;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};
use zeroize::Zeroizing;

use crate::error::{code, fail, CredError};

/// Loopback redirect ports, tried in order (Fixed constants).
pub(crate) const PORTS: [u16; 3] = [47821, 47822, 47823];
/// The page shown after a successful sign-in, the one sentence this component owns.
const SIGNED_IN_PAGE: &str = "You're signed in. You can close this tab.";
/// The longest request head read from the browser.
const MAX_HEAD_BYTES: usize = 8 * 1024;
/// How long one connection may take to send its request head, so a silent connection cannot hold the
/// listener (chosen here).
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// `http://127.0.0.1:<port>/callback`.
pub(crate) fn redirect_uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}/callback")
}

/// Binds the first free port; all busy is `CRED_LOOPBACK_BUSY`.
pub(crate) async fn bind() -> Result<(TcpListener, u16), CredError> {
    for port in PORTS {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)).await {
            return Ok((listener, port));
        }
    }
    fail(code::LOOPBACK_BUSY)
}

/// What the provider's redirect carried.
pub(crate) enum Callback {
    Code(Zeroizing<String>),
    /// `error=access_denied`.
    Denied,
    /// Any other `error`.
    Error,
}

/// One accepted callback with the connection to answer on.
pub(crate) struct Accepted {
    pub callback: Callback,
    pub stream: TcpStream,
}

/// Accepts connections until one `GET /callback` carries the expected `state`. A request with a
/// wrong or missing `state` is answered 400 and the flow stays open; any other path is answered 404.
pub(crate) async fn accept(listener: &TcpListener, state: &str) -> Accepted {
    loop {
        let Ok((mut stream, _)) = listener.accept().await else {
            continue;
        };
        let target = tokio::time::timeout(READ_TIMEOUT, read_target(&mut stream))
            .await
            .ok()
            .flatten();
        let Some(target) = target else {
            respond(&mut stream, 400, "").await;
            continue;
        };
        let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
        if path != "/callback" {
            respond(&mut stream, 404, "").await;
            continue;
        }
        let params: HashMap<String, Zeroizing<String>> =
            url::form_urlencoded::parse(query.as_bytes())
                .map(|(k, v)| (k.into_owned(), Zeroizing::new(v.into_owned())))
                .collect();
        let state_ok = params
            .get("state")
            .is_some_and(|given| bool::from(given.as_bytes().ct_eq(state.as_bytes())));
        if !state_ok {
            respond(&mut stream, 400, "").await;
            continue;
        }
        let callback = match (params.get("error"), params.get("code")) {
            (Some(error), _) if error.as_str() == "access_denied" => Callback::Denied,
            (Some(_), _) => Callback::Error,
            (None, Some(code)) => Callback::Code(code.clone()),
            (None, None) => {
                respond(&mut stream, 400, "").await;
                continue;
            }
        };
        return Accepted { callback, stream };
    }
}

/// The request target of a `GET` line, or None for anything else.
async fn read_target(stream: &mut TcpStream) -> Option<String> {
    let mut head = Zeroizing::new(Vec::with_capacity(1024));
    let mut chunk = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < MAX_HEAD_BYTES {
        let read = stream.read(&mut chunk).await.ok()?;
        if read == 0 {
            break;
        }
        head.extend_from_slice(&chunk[..read]);
    }
    let line_end = head.windows(2).position(|w| w == b"\r\n")?;
    let line = std::str::from_utf8(&head[..line_end]).ok()?;
    let mut parts = line.split(' ');
    match (parts.next(), parts.next()) {
        (Some("GET"), Some(target)) => Some(target.to_owned()),
        _ => None,
    }
}

/// Writes one response and closes the connection.
pub(crate) async fn respond(stream: &mut TcpStream, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        _ => "Bad Request",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

/// The success page.
pub(crate) async fn signed_in(stream: &mut TcpStream) {
    respond(stream, 200, SIGNED_IN_PAGE).await;
}
