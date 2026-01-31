//! `ServerConnector`: construction, start, wake and the read-side calls (ServerConnector.md *Public
//! Interface*, Internal Logic 1, 10, 11, 12). The commands live in `command.rs`, pairing in
//! `pairing.rs`, the per-server stream in `supervisor.rs`.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use ordin_identity::DesktopKeyStore;
use ordin_journal::{Journal, ModuleMigrations};
use ordin_service_api::commands::{OlderSide, ServerView};
use ordin_service_api::ServiceApiVersion;
use ordin_types::domain::server::ServerConnectionState;
use ordin_types::{BackendId, ServerConnectionId};
use sqlx::SqlitePool;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::conn::{Conn, Supervisor};
use crate::error::ConnectorError;
use crate::store;
use crate::tls::{self, Bundle, ClientSpec};
use crate::types::{ConnectorConfig, ServerEventSender, StandIn, Timing};
use crate::{log, version};

/// The sender and the quit token handed to `start`.
pub(crate) struct Run {
    pub(crate) events: ServerEventSender,
    pub(crate) cancel: CancellationToken,
}

/// Everything the supervisors and the calls share.
pub(crate) struct Inner {
    pub(crate) cfg: ConnectorConfig,
    pub(crate) journal: Journal,
    pub(crate) pool: SqlitePool,
    pub(crate) keys: Arc<dyn DesktopKeyStore>,
    pub(crate) timing: Timing,
    pub(crate) plain_http: bool,
    pub(crate) desktop: ServiceApiVersion,
    pub(crate) epoch: Instant,
    pub(crate) conns: Mutex<HashMap<ServerConnectionId, Arc<Conn>>>,
    pub(crate) run: Mutex<Option<Run>>,
    pub(crate) wake: watch::Sender<u64>,
}

/// The desktop's side of the link to the person's own ordin servers.
pub struct ServerConnector {
    pub(crate) inner: Arc<Inner>,
}

impl ServerConnector {
    /// This crate's migrations (module `server_connector`); the program registers them.
    pub fn migrations() -> ModuleMigrations {
        crate::migrations()
    }

    /// Builds the connector over the Journal's datastore. The computer name must be 1–63 characters
    /// (`ComputerNameInvalid`) and the datastore must be SQLite, the desktop's shape (`Datastore`).
    pub fn new(
        cfg: ConnectorConfig,
        journal: Journal,
        keys: Arc<dyn DesktopKeyStore>,
    ) -> Result<ServerConnector, ConnectorError> {
        let name_chars = cfg.computer_name.chars().count();
        if !(1..=63).contains(&name_chars) {
            return Err(ConnectorError::ComputerNameInvalid);
        }
        let pool = journal
            .datastore()
            .sqlite()
            .ok_or(ConnectorError::Datastore)?
            .clone();
        let (wake, _) = watch::channel(0u64);
        Ok(ServerConnector {
            inner: Arc::new(Inner {
                cfg,
                journal,
                pool,
                keys,
                timing: Timing::new(1.0),
                plain_http: false,
                desktop: ServiceApiVersion::CURRENT,
                epoch: Instant::now(),
                conns: Mutex::new(HashMap::new()),
                run: Mutex::new(None),
                wake,
            }),
        })
    }

    /// Test seam: see [`StandIn`]. Must be called before the connector is shared or started.
    #[doc(hidden)]
    pub fn with_stand_in(mut self, stand_in: StandIn) -> ServerConnector {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.plain_http = stand_in.plain_http;
            inner.timing = Timing::new(stand_in.time_scale);
            inner.desktop = stand_in.desktop_version;
        }
        self
    }

    /// Loads every connection and starts one supervisor per server not `disconnected` or
    /// `refused_for_version`. Server events go to `events` (the Command Bridge's channel). Called once;
    /// a second call returns `AlreadyStarted`. Cancelling `cancel` is the quit path: every stream closes
    /// and the states are persisted (step 12); there is no separate `close`.
    // spec: ordin-desktop/components/ServerConnector#SA-RECON-01
    pub async fn start(
        &self,
        events: ServerEventSender,
        cancel: CancellationToken,
    ) -> Result<(), ConnectorError> {
        let inner = &self.inner;
        {
            let mut run = inner.run.lock().unwrap_or_else(|p| p.into_inner());
            if run.is_some() {
                return Err(ConnectorError::AlreadyStarted);
            }
            *run = Some(Run {
                events,
                cancel: cancel.clone(),
            });
        }
        match inner.start_all().await {
            Ok(()) => Ok(()),
            Err(e) => {
                *inner.run.lock().unwrap_or_else(|p| p.into_inner()) = None;
                Err(e)
            }
        }
    }

    /// The computer woke from sleep (or its network came back): every supervisor waiting in backoff skips
    /// the rest of its wait and reconnects now; a supervisor in `connected` drops a stream silent for more
    /// than 20 s and reconnects. Does nothing before `start` or after `cancel`. Never fails.
    // spec: ordin-desktop/components/ServerConnector#SA-RECON-01
    pub fn on_wake(&self) {
        let inner = &self.inner;
        let running = inner
            .run
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .is_some_and(|r| !r.cancel.is_cancelled());
        if !running {
            return;
        }
        let now_ms = inner.epoch.elapsed().as_millis() as u64;
        let stale_ms = inner.timing.stale().as_millis() as u64;
        let woken = inner
            .conns
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .filter(|c| c.supervised())
            .filter(|c| {
                c.waiting.load(Ordering::SeqCst)
                    || (c.state() == ServerConnectionState::Connected
                        && now_ms.saturating_sub(c.last_byte_ms.load(Ordering::SeqCst)) > stale_ms)
            })
            .count();
        log::wake(woken);
        inner.wake.send_modify(|n| *n = n.wrapping_add(1));
    }

    /// Every server as the screens see it, ordered by display name.
    pub async fn list(&self) -> Result<Vec<ServerView>, ConnectorError> {
        let inner = &self.inner;
        let rows = store::load_all(&inner.pool).await?;
        let mut views: Vec<ServerView> = rows
            .iter()
            .map(|row| {
                let live = inner.find(row.id).map(|c| {
                    let l = c.live.lock().unwrap_or_else(|p| p.into_inner());
                    (l.state, l.as_of, l.version.clone())
                });
                let (state, as_of, stored_version) =
                    live.unwrap_or((row.state, None, row.service_api_version.clone()));
                ServerView {
                    server_connection_id: row.id,
                    display_name: row.display_name.clone(),
                    state,
                    as_of,
                    backend: Some(row.backend),
                    older_side: inner.older_side(state, stored_version.as_deref()),
                }
            })
            .collect();
        views.sort_by(|a, b| a.display_name.cmp(&b.display_name));
        Ok(views)
    }

    /// The connection that serves `backend`.
    pub fn connection_for(&self, backend: BackendId) -> Option<ServerConnectionId> {
        self.inner.by_backend(backend).map(|c| c.id)
    }

    /// The server's display name, for "On [server]" in notifications (11 §7).
    pub fn display_name(&self, backend: BackendId) -> Option<String> {
        self.inner
            .by_backend(backend)
            .map(|c| c.display_name.clone())
    }
}

impl Inner {
    pub(crate) fn find(&self, id: ServerConnectionId) -> Option<Arc<Conn>> {
        self.conns
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&id)
            .cloned()
    }

    /// The open record of a server, loaded from the datastore when `start` has not run yet.
    pub(crate) async fn conn_or_load(
        &self,
        id: ServerConnectionId,
    ) -> Result<Arc<Conn>, ConnectorError> {
        if let Some(conn) = self.find(id) {
            return Ok(conn);
        }
        let rows = store::load_all(&self.pool).await?;
        let row = rows
            .iter()
            .find(|r| r.id == id)
            .ok_or(ConnectorError::UnknownServer)?;
        let conn = Conn::from_row(row, self.cfg.read_cache_bytes, self.epoch);
        let mut map = self.conns.lock().unwrap_or_else(|p| p.into_inner());
        Ok(Arc::clone(map.entry(id).or_insert(conn)))
    }

    pub(crate) fn by_backend(&self, backend: BackendId) -> Option<Arc<Conn>> {
        self.conns
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .find(|c| c.backend == backend)
            .cloned()
    }

    /// Step 10: the side to update, only for a server refused over its version.
    // spec: ordin-desktop/components/ServerConnector#SA-RECON-02
    pub(crate) fn older_side(
        &self,
        state: ServerConnectionState,
        stored_version: Option<&str>,
    ) -> Option<OlderSide> {
        if state != ServerConnectionState::RefusedForVersion {
            return None;
        }
        let older = stored_version
            .and_then(version::parse)
            .is_some_and(|v| version::server_older(self.desktop, v));
        Some(if older {
            OlderSide::Server
        } else {
            OlderSide::Desktop
        })
    }

    pub(crate) fn scheme(&self) -> &'static str {
        if self.plain_http {
            "http"
        } else {
            "https"
        }
    }

    pub(crate) fn url(&self, conn: &Conn, path: &str) -> String {
        format!(
            "{}://{}{}",
            self.scheme(),
            crate::address::authority(&conn.host, conn.port),
            path
        )
    }

    /// The events sender and quit token, once `start` has run.
    pub(crate) fn running(&self) -> Option<(ServerEventSender, CancellationToken)> {
        self.run
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(|r| (r.events.clone(), r.cancel.clone()))
    }

    /// Builds the client of a server (step 2). A required client certificate the platform does not
    /// give → `ClientCertUnavailable`.
    pub(crate) async fn build_bundle(
        &self,
        pin: Option<String>,
    ) -> Result<Arc<Bundle>, ConnectorError> {
        let client_cert = match &self.cfg.client_certificate {
            Some(name) => match self.keys.client_certificate(name).await {
                Ok(Some(key)) => Some(key),
                Ok(None) => return Err(ConnectorError::ClientCertUnavailable),
                Err(_) => return Err(ConnectorError::Keychain),
            },
            None => None,
        };
        tls::build(ClientSpec {
            pin,
            client_cert,
            connect_timeout: self.timing.connect(),
            keep_alive: self.timing.keep_alive(),
            no_proxy: self.plain_http,
        })
        .map(Arc::new)
    }

    /// The server's client, built on first use and kept.
    pub(crate) async fn bundle(&self, conn: &Conn) -> Result<Arc<Bundle>, ConnectorError> {
        let mut slot = conn.bundle.lock().await;
        if let Some(bundle) = &*slot {
            return Ok(Arc::clone(bundle));
        }
        let built = self.build_bundle(conn.pin.clone()).await?;
        *slot = Some(Arc::clone(&built));
        Ok(built)
    }

    /// Step 1.
    async fn start_all(self: &Arc<Self>) -> Result<(), ConnectorError> {
        let rows = store::load_all(&self.pool).await?;
        let mut to_run = Vec::new();
        for row in &rows {
            let conn = match self.find(row.id) {
                Some(existing) => existing,
                None => {
                    let conn = Conn::from_row(row, self.cfg.read_cache_bytes, self.epoch);
                    self.conns
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .insert(row.id, Arc::clone(&conn));
                    conn
                }
            };
            if conn.supervised() {
                continue;
            }
            match conn.state() {
                ServerConnectionState::Connected | ServerConnectionState::NotConnected => {
                    // The link is not up yet: not_connected in memory, no event.
                    {
                        let mut live = conn.live.lock().unwrap_or_else(|p| p.into_inner());
                        live.state = ServerConnectionState::NotConnected;
                    }
                    match self.keys.get_desktop_key(&conn.keychain_ref).await {
                        Ok(None) => {
                            self.set_state(
                                &conn,
                                ServerConnectionState::Disconnected,
                                "CONN_KEYCHAIN",
                                None,
                                None,
                            )
                            .await;
                        }
                        Ok(Some(_)) | Err(_) => to_run.push(conn),
                    }
                }
                ServerConnectionState::Disconnected | ServerConnectionState::RefusedForVersion => {}
            }
        }
        for conn in to_run {
            self.spawn_supervisor(&conn);
        }
        Ok(())
    }

    /// Step 6: one supervisor task per server.
    pub(crate) fn spawn_supervisor(self: &Arc<Self>, conn: &Arc<Conn>) {
        let Some((events, global)) = self.running() else {
            return;
        };
        let own = global.child_token();
        let join = tokio::spawn(crate::supervisor::run(
            Arc::clone(self),
            Arc::clone(conn),
            events,
            global,
            own.clone(),
        ));
        let mut slot = conn.sup.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(old) = slot.replace(Supervisor { own, join }) {
            old.own.cancel();
        }
    }
}
