//! `connect`, `disconnect` and `remove` (ServerConnector.md Internal Logic 3, 7).

use std::sync::Arc;

use ordin_service_api::commands::{ConnectServer, ServerView};
use ordin_service_api::pairing::{normalise_code, PairReply, PairRequest, PairingLine};
use ordin_service_api::ServiceApiVersion;
use ordin_types::domain::server::ServerConnectionState;
use ordin_types::encoding::b64url_decode_exact;
use ordin_types::time::Timestamp;
use ordin_types::ServerConnectionId;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use zeroize::Zeroizing;

use crate::address::{authority, split_address};
use crate::command::{bearer, read_body};
use crate::conn::Conn;
use crate::error::ConnectorError;
use crate::store::{self, StoredServer};
use crate::tls::Bundle;
use crate::version::{self, Verdict};
use crate::wire::{read_reply, Shape};
use crate::{log, Inner, ServerConnector};

/// Longest display name the server may send (field rule `display_name`).
const DISPLAY_NAME_MAX_CHARS: usize = 120;

impl ServerConnector {
    /// Pairs this computer with a server from an address and a code, or from a pairing line.
    pub async fn connect(&self, input: ConnectServer) -> Result<ServerView, ConnectorError> {
        self.inner.connect(input).await
    }

    /// Ends the pairing on the server (best effort), stops the stream, forgets the key and marks the
    /// server `disconnected`.
    pub async fn disconnect(&self, id: ServerConnectionId) -> Result<(), ConnectorError> {
        self.inner.disconnect(id).await
    }

    /// As `disconnect` if not already disconnected, then deletes the server's rows.
    pub async fn remove(&self, id: ServerConnectionId) -> Result<(), ConnectorError> {
        self.inner.remove(id).await
    }
}

/// What the person typed, split into the parts pairing uses.
struct Target {
    host: String,
    port: u16,
    code: Zeroizing<String>,
    pin: Option<String>,
}

fn parse_input(input: ConnectServer) -> Result<Target, ConnectorError> {
    match input {
        ConnectServer::AddressAndCode { address, code } => {
            let (host, port) = split_address(&address)?;
            let code = normalise_code(&code).map_err(|_| ConnectorError::CodeRefused)?;
            Ok(Target {
                host,
                port,
                code: Zeroizing::new(code),
                pin: None,
            })
        }
        ConnectServer::PairingLine { line } => {
            let line = Zeroizing::new(line);
            let parsed =
                PairingLine::parse(&line).map_err(|_| ConnectorError::PairingLineInvalid)?;
            Ok(Target {
                host: parsed.host,
                port: parsed.port,
                code: Zeroizing::new(parsed.code),
                pin: Some(parsed.fingerprint.0.to_ascii_lowercase()),
            })
        }
    }
}

impl Inner {
    async fn connect(self: &Arc<Self>, input: ConnectServer) -> Result<ServerView, ConnectorError> {
        // 3.1
        let target = parse_input(input)?;
        // 3.2
        let reuse = match store::find_by_address(&self.pool, &target.host, target.port).await? {
            Some((_, state)) if state != ServerConnectionState::Disconnected => {
                return Err(ConnectorError::Duplicate)
            }
            Some((id, _)) => Some(id),
            None => None,
        };
        // 3.3
        let bundle = self.build_bundle(target.pin.clone()).await?;
        // 3.4
        let reply = self.request_pairing(&target, &bundle).await?;
        // 3.5
        let key = decode_key(&reply)?;
        if !ServiceApiVersion::desktop_accepts(self.desktop, reply.api_version) {
            return Err(ConnectorError::RefusedForVersion {
                server_older: version::server_older(self.desktop, reply.api_version),
            });
        }
        let name_chars = reply.server_name.chars().count();
        if !(1..=DISPLAY_NAME_MAX_CHARS).contains(&name_chars) {
            return Err(ConnectorError::BadAnswer);
        }
        // 3.6
        let id = reuse.unwrap_or_else(ServerConnectionId::new);
        let row = StoredServer {
            id,
            display_name: reply.server_name.clone(),
            address: target.host.clone(),
            port: target.port,
            service_api_version: Some(version::text(reply.api_version)),
            last_sequence_received: 0,
            // spec: ordin-desktop/components/ServerConnector#SA-RECON-03
            keychain_ref: format!("server-connector/desktop-key/{id}"),
            state: ServerConnectionState::Connected,
            paired_at: Timestamp::now(),
            pin: target.pin.clone(),
            backend: reply.backend,
            desktop_pairing_id: reply.desktop_pairing_id,
        };
        store::save_paired(&self.pool, &row).await?;
        if self
            .keys
            .put_desktop_key(&row.keychain_ref, &key)
            .await
            .is_err()
        {
            let _ = store::delete(&self.pool, id).await;
            return Err(ConnectorError::Keychain);
        }
        drop(key);
        // 3.7
        let conn = Conn::from_row(&row, self.cfg.read_cache_bytes, self.epoch);
        *conn.bundle.lock().await = Some(bundle);
        let old = self
            .conns
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id, Arc::clone(&conn));
        if let Some(old) = old {
            let _ = old.take_supervisor();
        }
        self.spawn_supervisor(&conn);
        self.publish(&conn, ServerConnectionState::Connected, None)
            .await;
        log::paired(id, &conn.host);
        Ok(ServerView {
            server_connection_id: id,
            display_name: conn.display_name.clone(),
            state: ServerConnectionState::Connected,
            as_of: None,
            backend: Some(conn.backend),
            older_side: None,
        })
    }

    /// 3.4: `POST /v1/pairing`, 15 s, and the mapping of its answers.
    async fn request_pairing(
        &self,
        target: &Target,
        bundle: &Bundle,
    ) -> Result<PairReply, ConnectorError> {
        let body = serde_json::to_vec(&PairRequest {
            api_version: self.desktop,
            code: target.code.to_string(),
            computer_name: self.cfg.computer_name.clone(),
        })
        .map_err(|_| ConnectorError::BadAnswer)?;
        let url = format!(
            "{}://{}/v1/pairing",
            self.scheme(),
            authority(&target.host, target.port)
        );
        bundle.reset_flag();
        let response = bundle
            .client
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .body(body)
            .timeout(self.timing.pairing())
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    ConnectorError::Unreachable
                } else {
                    bundle.classify()
                }
            })?;
        let status = response.status().as_u16();
        let header = response.headers().get(version::HEADER).cloned();
        let bytes = read_body(response)
            .await
            .map_err(|_| ConnectorError::BadAnswer)?;
        let shape = read_reply(&bytes);
        let error_code = match &shape {
            Some(Shape::Error(e)) => Some(e.code.as_str()),
            _ => None,
        };
        match (status, error_code) {
            (200, _) => match shape {
                Some(Shape::Ok(result)) => {
                    serde_json::from_value(result).map_err(|_| ConnectorError::BadAnswer)
                }
                _ => Err(ConnectorError::BadAnswer),
            },
            (403, Some("DESK_CODE_INVALID" | "DESK_CODE_EXPIRED")) => {
                Err(ConnectorError::CodeRefused)
            }
            (403, Some("DESK_PAIRING_STOPPED")) => Err(ConnectorError::PairingStopped),
            // spec: ordin-desktop/components/ServerConnector#SA-RECON-04
            (403, Some("DESK_TOO_MANY")) => Err(ConnectorError::TooManyDesktops),
            (422, Some("DESK_NAME_INVALID")) => Err(ConnectorError::ComputerNameInvalid),
            (422, Some("SVC_INPUT_INVALID")) => Err(ConnectorError::CodeRefused),
            (429, _) => Err(ConnectorError::AddressLocked),
            (409, _) => {
                let server_older = match version::judge(self.desktop, header.as_ref()) {
                    Verdict::Unusable { server_older, .. } => server_older,
                    Verdict::Usable(_) | Verdict::Unreadable => false,
                };
                Err(ConnectorError::RefusedForVersion { server_older })
            }
            _ => Err(ConnectorError::BadAnswer),
        }
    }

    async fn disconnect(self: &Arc<Self>, id: ServerConnectionId) -> Result<(), ConnectorError> {
        let conn = self.conn_or_load(id).await?;
        if conn.state() == ServerConnectionState::Disconnected {
            return Ok(());
        }
        // Best effort: a failure does not stop the disconnect.
        if !self.end_pairing(&conn).await {
            log::end_unconfirmed(id);
        }
        if let Some(sup) = conn.take_supervisor() {
            let _ = sup.join.await;
        }
        self.set_state(
            &conn,
            ServerConnectionState::Disconnected,
            "person_ended",
            None,
            None,
        )
        .await;
        conn.clear_cache();
        self.keys
            .delete_desktop_key(&conn.keychain_ref)
            .await
            .map_err(|_| ConnectorError::Keychain)
    }

    /// `POST /v1/pairing/end` with the key, 10 s. True when the server answered 200.
    async fn end_pairing(&self, conn: &Conn) -> bool {
        let Ok(Some(key)) = self.keys.get_desktop_key(&conn.keychain_ref).await else {
            return false;
        };
        let Ok(bundle) = self.bundle(conn).await else {
            return false;
        };
        bundle.reset_flag();
        let sent = bundle
            .client
            .post(self.url(conn, "/v1/pairing/end"))
            .header(AUTHORIZATION, bearer(&key))
            .header(CONTENT_TYPE, "application/json")
            .body("{}")
            .timeout(self.timing.end())
            .send()
            .await;
        matches!(sent, Ok(r) if r.status().is_success())
    }

    async fn remove(self: &Arc<Self>, id: ServerConnectionId) -> Result<(), ConnectorError> {
        let conn = self.conn_or_load(id).await?;
        if conn.state() != ServerConnectionState::Disconnected {
            self.disconnect(id).await?;
        }
        store::delete(&self.pool, id).await?;
        self.conns
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&id);
        conn.clear_cache();
        Ok(())
    }
}

/// 3.5: the key must decode to exactly 32 bytes.
fn decode_key(reply: &PairReply) -> Result<Zeroizing<[u8; 32]>, ConnectorError> {
    let raw = Zeroizing::new(
        b64url_decode_exact(&reply.desktop_key, 32, "CONN_BAD_ANSWER")
            .map_err(|_| ConnectorError::BadAnswer)?,
    );
    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(&raw);
    Ok(key)
}
