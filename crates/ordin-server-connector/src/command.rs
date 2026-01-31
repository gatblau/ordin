//! `send_command` (ServerConnector.md Internal Logic 4): a command the screens address to a server's
//! backend, sent to that server's Remote API.

use std::time::Instant;

use bytes::Bytes;
use ordin_service_api::CommandMeta;
use ordin_service_api::Effect;
use ordin_types::domain::server::ServerConnectionState;
use ordin_types::encoding::b64url_encode;
use ordin_types::BackendId;
use reqwest::header::{HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use zeroize::Zeroizing;

use crate::cache::ReadCache;
use crate::error::ConnectorError;
use crate::version::{self, Verdict};
use crate::wire::{read_reply, Shape};
use crate::{log, Inner, ServerConnector};

/// `Authorization: Bearer <b64url key>`, marked sensitive so no log of the headers shows it.
pub(crate) fn bearer(key: &[u8; 32]) -> HeaderValue {
    let text = Zeroizing::new(format!("Bearer {}", b64url_encode(key)));
    // base64url text is always a valid header value.
    let mut value = HeaderValue::from_str(&text).unwrap_or_else(|_| HeaderValue::from_static(""));
    value.set_sensitive(true);
    value
}

/// The most response bytes a reply is read up to: 8 MiB (a command reply is small; a page of a list is
/// the largest and is bounded by the Service API's own page limits).
const MAX_REPLY_BYTES: usize = 8 * 1024 * 1024;

impl ServerConnector {
    /// Sends one command to the server behind `backend`; the reply bytes are returned unchanged for the
    /// screens to read.
    pub async fn send_command(
        &self,
        backend: BackendId,
        meta: &'static CommandMeta,
        envelope_json: Bytes,
        deadline: Instant,
    ) -> Result<Bytes, ConnectorError> {
        self.inner
            .send_command(backend, meta, envelope_json, deadline)
            .await
    }
}

impl Inner {
    async fn send_command(
        &self,
        backend: BackendId,
        meta: &'static CommandMeta,
        envelope_json: Bytes,
        deadline: Instant,
    ) -> Result<Bytes, ConnectorError> {
        let conn = self
            .by_backend(backend)
            .ok_or(ConnectorError::UnknownServer)?;
        let cache_key =
            (meta.effect == Effect::Read).then(|| ReadCache::key(meta.name, &envelope_json));
        let (state, stored_version) = {
            let live = conn.live.lock().unwrap_or_else(|p| p.into_inner());
            (live.state, live.version.clone())
        };
        match state {
            ServerConnectionState::RefusedForVersion => {
                let older = stored_version
                    .as_deref()
                    .and_then(version::parse)
                    .is_some_and(|v| version::server_older(self.desktop, v));
                return Err(ConnectorError::RefusedForVersion {
                    server_older: older,
                });
            }
            ServerConnectionState::Disconnected => return Err(ConnectorError::KeyRefused),
            ServerConnectionState::NotConnected => {
                if let Some(key) = &cache_key {
                    let hit = conn
                        .cache
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .get(key);
                    if let Some(reply) = hit {
                        return Ok(reply);
                    }
                }
                return Err(ConnectorError::NotConnected);
            }
            ServerConnectionState::Connected => {}
        }

        let started = Instant::now();
        let key = self
            .keys
            .get_desktop_key(&conn.keychain_ref)
            .await
            .map_err(|_| ConnectorError::Keychain)?
            .ok_or(ConnectorError::Keychain)?;
        let bundle = self.bundle(&conn).await?;
        let url = self.url(&conn, meta.rest_path);
        let max_retries: u32 = if meta.effect == Effect::Change { 1 } else { 2 };
        let mut retries = 0u32;
        let response = loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(ConnectorError::Deadline);
            }
            bundle.reset_flag();
            let sent = bundle
                .client
                .post(&url)
                .header(AUTHORIZATION, bearer(&key))
                .header(CONTENT_TYPE, "application/json")
                .body(envelope_json.clone())
                .timeout(deadline - now)
                .send()
                .await;
            match sent {
                Ok(response) => break response,
                Err(e) if e.is_timeout() => return Err(ConnectorError::Deadline),
                Err(_) => {
                    let cause = bundle.classify();
                    if !matches!(cause, ConnectorError::Unreachable) {
                        return Err(cause);
                    }
                    if retries >= max_retries {
                        conn.probe.notify_one();
                        return Err(ConnectorError::Unreachable);
                    }
                    let wait = self.timing.retry(retries);
                    retries += 1;
                    if Instant::now() + wait >= deadline {
                        return Err(ConnectorError::Deadline);
                    }
                    tokio::time::sleep(wait).await;
                }
            }
        };

        let status = response.status();
        match version::judge(self.desktop, response.headers().get(version::HEADER)) {
            Verdict::Unreadable => return Err(ConnectorError::BadAnswer),
            Verdict::Unusable {
                version: seen,
                server_older,
            } => {
                self.set_state(
                    &conn,
                    ServerConnectionState::RefusedForVersion,
                    "CONN_REFUSED_FOR_VERSION",
                    None,
                    Some(seen),
                )
                .await;
                let _ = conn.take_supervisor();
                return Err(ConnectorError::RefusedForVersion { server_older });
            }
            Verdict::Usable(_) => {}
        }
        if status == reqwest::StatusCode::UNAUTHORIZED {
            self.set_state(
                &conn,
                ServerConnectionState::Disconnected,
                "CONN_KEY_REFUSED",
                None,
                None,
            )
            .await;
            let _ = conn.take_supervisor();
            conn.clear_cache();
            return Err(ConnectorError::KeyRefused);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        let body = match tokio::time::timeout(remaining, read_body(response)).await {
            Err(_) => return Err(ConnectorError::Deadline),
            Ok(Err(e)) => return Err(e),
            Ok(Ok(body)) => body,
        };
        let shape = read_reply(&body).ok_or(ConnectorError::BadAnswer)?;
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            if let Shape::Error(error) = &shape {
                if error.code == "DESK_ADDRESS_LOCKED" {
                    return Err(ConnectorError::AddressLocked);
                }
            }
        }
        log::command(
            conn.id,
            meta.name,
            u64::from(status.as_u16()),
            started.elapsed().as_millis() as u64,
            retries,
        );
        if let (Some(key), Shape::Ok(_)) = (cache_key, &shape) {
            conn.cache
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .put(key, body.clone());
        }
        Ok(body)
    }
}

/// Reads a whole body up to [`MAX_REPLY_BYTES`]; a lost connection → `Unreachable`, an over-long body →
/// `BadAnswer`.
pub(crate) async fn read_body(mut response: reqwest::Response) -> Result<Bytes, ConnectorError> {
    let mut out = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if out.len() + chunk.len() > MAX_REPLY_BYTES {
                    return Err(ConnectorError::BadAnswer);
                }
                out.extend_from_slice(&chunk);
            }
            Ok(None) => return Ok(Bytes::from(out)),
            Err(_) => return Err(ConnectorError::Unreachable),
        }
    }
}
