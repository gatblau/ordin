//! The in-memory record of one paired server and the state machine over `ServerConnectionState`
//! (ServerConnector.md Shared Context, Internal Logic 8).

use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use ordin_service_api::events::ServiceEvent;
use ordin_service_api::ServiceApiVersion;
use ordin_types::domain::server::ServerConnectionState;
use ordin_types::time::Timestamp;
use ordin_types::wire::events::EventKind;
use ordin_types::{BackendId, ServerConnectionId};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::cache::ReadCache;
use crate::store::{self, StoredServer};
use crate::tls::Bundle;
use crate::{log, version, Inner};

/// What changes while the server is open: its state and the facts shown with it.
pub(crate) struct Live {
    pub(crate) state: ServerConnectionState,
    /// The time of the last event received, kept while the state is `not_connected`.
    pub(crate) as_of: Option<Timestamp>,
    /// `"<major>.<minor>"` as stored.
    pub(crate) version: Option<String>,
    pub(crate) last_event_at: Option<Timestamp>,
}

/// The running supervisor of a server.
pub(crate) struct Supervisor {
    pub(crate) own: CancellationToken,
    pub(crate) join: tokio::task::JoinHandle<()>,
}

pub(crate) struct Conn {
    pub(crate) id: ServerConnectionId,
    pub(crate) backend: BackendId,
    pub(crate) display_name: String,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) keychain_ref: String,
    pub(crate) pin: Option<String>,
    pub(crate) live: Mutex<Live>,
    pub(crate) last_seq: AtomicU64,
    pub(crate) persisted_seq: AtomicU64,
    pub(crate) cache: Mutex<ReadCache>,
    pub(crate) bundle: tokio::sync::Mutex<Option<Arc<Bundle>>>,
    pub(crate) sup: Mutex<Option<Supervisor>>,
    /// True while the supervisor waits in its backoff.
    pub(crate) waiting: AtomicBool,
    /// Milliseconds since the connector's epoch at which the last byte of the stream arrived.
    pub(crate) last_byte_ms: AtomicU64,
    /// A command lost its connection: the supervisor probes at once.
    pub(crate) probe: Notify,
}

impl Conn {
    pub(crate) fn from_row(row: &StoredServer, cache_bytes: usize, epoch: Instant) -> Arc<Conn> {
        Arc::new(Conn {
            id: row.id,
            backend: row.backend,
            display_name: row.display_name.clone(),
            host: row.address.clone(),
            port: row.port,
            keychain_ref: row.keychain_ref.clone(),
            pin: row.pin.clone(),
            live: Mutex::new(Live {
                state: row.state,
                as_of: None,
                version: row.service_api_version.clone(),
                last_event_at: None,
            }),
            last_seq: AtomicU64::new(row.last_sequence_received),
            persisted_seq: AtomicU64::new(row.last_sequence_received),
            cache: Mutex::new(ReadCache::new(cache_bytes)),
            bundle: tokio::sync::Mutex::new(None),
            sup: Mutex::new(None),
            waiting: AtomicBool::new(false),
            last_byte_ms: AtomicU64::new(epoch.elapsed().as_millis() as u64),
            probe: Notify::new(),
        })
    }

    pub(crate) fn state(&self) -> ServerConnectionState {
        self.live.lock().unwrap_or_else(|p| p.into_inner()).state
    }

    pub(crate) fn clear_cache(&self) {
        self.cache.lock().unwrap_or_else(|p| p.into_inner()).clear();
    }

    /// Cancels the supervisor and hands back its task, if one is running.
    pub(crate) fn take_supervisor(&self) -> Option<Supervisor> {
        let sup = self.sup.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(s) = &sup {
            s.own.cancel();
        }
        sup
    }

    /// True when a supervisor is running.
    pub(crate) fn supervised(&self) -> bool {
        self.sup.lock().unwrap_or_else(|p| p.into_inner()).is_some()
    }
}

pub(crate) fn state_name(state: ServerConnectionState) -> &'static str {
    store::state_text(state)
}

impl Inner {
    /// Step 8: change a server's state in memory, then in the datastore, then publish
    /// `server_state_changed` through the Journal. A datastore or Journal failure is logged and the
    /// in-memory state stands, so the screens stay truthful for this session.
    pub(crate) async fn set_state(
        &self,
        conn: &Conn,
        to: ServerConnectionState,
        reason_code: &str,
        as_of: Option<Timestamp>,
        server_version: Option<ServiceApiVersion>,
    ) {
        let (from, changed) = {
            let mut live = conn.live.lock().unwrap_or_else(|p| p.into_inner());
            let from = live.state;
            let new_version = server_version.map(version::text);
            let version_changed = new_version.is_some() && new_version != live.version;
            live.state = to;
            live.as_of = if to == ServerConnectionState::NotConnected {
                as_of
            } else {
                None
            };
            if let Some(v) = new_version {
                live.version = Some(v);
            }
            (from, from != to || version_changed)
        };
        if !changed {
            return;
        }
        let version_text = server_version.map(version::text);
        if store::update_state(&self.pool, conn.id, to, version_text.as_deref())
            .await
            .is_err()
        {
            log::datastore_failed(conn.id, "CONN_DATASTORE");
        }
        if from != to {
            log::state(conn.id, state_name(from), state_name(to), reason_code);
            self.publish(conn, to, as_of).await;
        }
    }

    /// Appends `server_state_changed` to the Journal.
    pub(crate) async fn publish(
        &self,
        conn: &Conn,
        state: ServerConnectionState,
        as_of: Option<Timestamp>,
    ) {
        let event = ServiceEvent::ServerStateChanged {
            server_connection_id: conn.id,
            state,
            as_of: if state == ServerConnectionState::NotConnected {
                as_of
            } else {
                None
            },
        };
        if self
            .journal
            .append_event(
                EventKind::ServerStateChanged.as_str(),
                event.to_journal_data(),
            )
            .await
            .is_err()
        {
            log::datastore_failed(conn.id, "CONN_DATASTORE");
        }
    }
}
