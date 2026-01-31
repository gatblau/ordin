//! Shared test helpers: a Journal on a temporary SQLite file with this crate's migrations, a hand-written
//! `DesktopKeyStore`, a `wiremock` Remote API stand-in on loopback, a hand-written SSE stand-in
//! (`sse.rs`) and a hand-written TLS stand-in (`tls.rs`). Real network is never used.
#![allow(dead_code)]

pub mod sse;
pub mod tls;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_identity::{CredError, DesktopKeyStore};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalEvent, JournalOptions,
    MigrationRegistry, SystemClock, MIGRATIONS,
};
use ordin_server_connector::{
    ConnectorConfig, ServerConnector, ServerEvent, StandIn, DEFAULT_READ_CACHE_BYTES,
};
use ordin_service_api::ServiceApiVersion;
use ordin_types::ServerConnectionId;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tokio::sync::mpsc;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub const BACKEND: &str = "0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b";
pub const PAIRING_ID: &str = "0192a1d0-1111-7222-8333-444455556666";
/// 32 bytes, base64url without padding (43 characters).
pub const KEY_TEXT: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE";
pub const KEY_BYTES: [u8; 32] = [1u8; 32];
pub const CODE: &str = "K7QM2XPA";
pub const COMPUTER: &str = "Asha's laptop";
pub const SERVER_NAME: &str = "Northwind server";

pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir =
            std::env::temp_dir().join(format!("ordin-server-connector-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// An in-memory `DesktopKeyStore` that can be made to fail.
#[derive(Default)]
pub struct FakeKeys {
    pub items: Mutex<HashMap<String, [u8; 32]>>,
    pub fail_put: Mutex<bool>,
    pub fail_get: Mutex<bool>,
    pub fail_delete: Mutex<bool>,
    pub client_cert: Mutex<Option<Arc<rustls::sign::CertifiedKey>>>,
    pub client_cert_error: Mutex<bool>,
}

impl FakeKeys {
    pub fn has(&self, name: &str) -> bool {
        self.items.lock().unwrap().contains_key(name)
    }

    pub fn put(&self, name: &str, key: [u8; 32]) {
        self.items.lock().unwrap().insert(name.to_owned(), key);
    }

    pub fn count(&self) -> usize {
        self.items.lock().unwrap().len()
    }

    pub fn names(&self) -> Vec<String> {
        self.items.lock().unwrap().keys().cloned().collect()
    }
}

#[async_trait::async_trait]
impl DesktopKeyStore for FakeKeys {
    async fn put_desktop_key(&self, keychain_ref: &str, key: &[u8; 32]) -> Result<(), CredError> {
        if *self.fail_put.lock().unwrap() {
            return Err(CredError::Retryable {
                code: "CRED_KEYCHAIN_UNAVAILABLE",
            });
        }
        self.put(keychain_ref, *key);
        Ok(())
    }

    async fn get_desktop_key(
        &self,
        keychain_ref: &str,
    ) -> Result<Option<zeroize::Zeroizing<[u8; 32]>>, CredError> {
        if *self.fail_get.lock().unwrap() {
            return Err(CredError::Retryable {
                code: "CRED_KEYCHAIN_UNAVAILABLE",
            });
        }
        Ok(self
            .items
            .lock()
            .unwrap()
            .get(keychain_ref)
            .map(|k| zeroize::Zeroizing::new(*k)))
    }

    async fn delete_desktop_key(&self, keychain_ref: &str) -> Result<(), CredError> {
        if *self.fail_delete.lock().unwrap() {
            return Err(CredError::Retryable {
                code: "CRED_KEYCHAIN_UNAVAILABLE",
            });
        }
        self.items.lock().unwrap().remove(keychain_ref);
        Ok(())
    }

    async fn client_certificate(
        &self,
        _name: &str,
    ) -> Result<Option<Arc<rustls::sign::CertifiedKey>>, CredError> {
        if *self.client_cert_error.lock().unwrap() {
            return Err(CredError::Retryable {
                code: "CRED_KEYCHAIN_UNAVAILABLE",
            });
        }
        Ok(self.client_cert.lock().unwrap().clone())
    }
}

pub struct Env {
    pub dir: TempDir,
    pub journal: Journal,
    pub pool: SqlitePool,
    pub keys: Arc<FakeKeys>,
    pub mock: MockServer,
    seeded: std::sync::atomic::AtomicUsize,
}

impl Env {
    pub async fn new() -> Env {
        let dir = TempDir::new();
        let data = dir.path().join("data");
        std::fs::create_dir_all(&data).expect("data folder");
        let store = Datastore::open(DatastoreConfig::Sqlite {
            path: data.join("journal.db"),
            backup_dir: data.join("backups"),
        })
        .await
        .expect("open the datastore");
        let mut registry = MigrationRegistry::new();
        registry.register(MIGRATIONS).expect("journal");
        registry
            .register(ServerConnector::migrations())
            .expect("server connector");
        registry
            .apply(&store, ApplyMode::ApplyPending)
            .await
            .expect("apply the migrations");
        let pool = store.sqlite().expect("sqlite").clone();
        let journal = Journal::start(
            store,
            JournalOptions {
                automatic_pruning: false,
                prune_interval: Duration::from_secs(24 * 3600),
                clock: Arc::new(SystemClock),
            },
        )
        .await
        .expect("start the journal");
        Env {
            dir,
            journal,
            pool,
            keys: Arc::new(FakeKeys::default()),
            mock: MockServer::start().await,
            seeded: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub fn config(&self) -> ConnectorConfig {
        ConnectorConfig {
            computer_name: COMPUTER.to_owned(),
            client_certificate: None,
            read_cache_bytes: DEFAULT_READ_CACHE_BYTES,
        }
    }

    /// A connector over plain loopback HTTP whose waits are the spec's own times `scale`.
    pub fn connector(&self, scale: f64) -> ServerConnector {
        self.connector_with(self.config(), scale, ServiceApiVersion::CURRENT)
    }

    pub fn connector_with(
        &self,
        cfg: ConnectorConfig,
        scale: f64,
        desktop_version: ServiceApiVersion,
    ) -> ServerConnector {
        ServerConnector::new(cfg, self.journal.clone(), self.keys.clone())
            .expect("connector")
            .with_stand_in(StandIn {
                plain_http: true,
                time_scale: scale,
                desktop_version,
            })
    }

    /// A connector that really verifies TLS (for the TLS stand-in).
    pub fn tls_connector(&self, cfg: ConnectorConfig, scale: f64) -> ServerConnector {
        ServerConnector::new(cfg, self.journal.clone(), self.keys.clone())
            .expect("connector")
            .with_stand_in(StandIn {
                plain_http: false,
                time_scale: scale,
                desktop_version: ServiceApiVersion::CURRENT,
            })
    }

    pub fn mock_address(&self) -> String {
        format!("127.0.0.1:{}", self.mock.address().port())
    }

    pub fn mock_port(&self) -> u16 {
        self.mock.address().port()
    }

    /// Journal events of kind `server_state_changed`, oldest first.
    pub async fn state_events(&self) -> Vec<JournalEvent> {
        self.journal
            .events_since(0, 1000)
            .await
            .expect("events")
            .into_iter()
            .filter(|e| e.kind == "server_state_changed")
            .collect()
    }

    pub async fn row(&self, id: ServerConnectionId) -> Option<sqlx::sqlite::SqliteRow> {
        sqlx::query("SELECT * FROM server_connections WHERE id = ?1")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .expect("row")
    }

    pub async fn count(&self, table: &'static str) -> i64 {
        use sqlx::Row as _;
        let sql: &'static str = match table {
            "server_connections" => "SELECT count(*) FROM server_connections",
            "server_connection_pins" => "SELECT count(*) FROM server_connection_pins",
            _ => "SELECT count(*) FROM server_connection_backends",
        };
        sqlx::query(sql)
            .fetch_one(&self.pool)
            .await
            .expect("count")
            .get(0)
    }

    /// Writes a stored server (all three tables) and its key, as if paired earlier.
    pub async fn seed(&self, seed: Seed) -> ServerConnectionId {
        let nth = self
            .seeded
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let id = ServerConnectionId::new();
        let keychain_ref = format!("server-connector/desktop-key/{id}");
        sqlx::query(
            "INSERT INTO server_connections (id, display_name, address, port, service_api_version, \
             last_sequence_received, keychain_ref, state, paired_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, '2026-01-10T10:02:11.004Z')",
        )
        .bind(id.to_string())
        .bind(seed.name)
        .bind(seed.host)
        .bind(i64::from(seed.port))
        .bind(seed.version)
        .bind(seed.last_seq)
        .bind(&keychain_ref)
        .bind(seed.state)
        .execute(&self.pool)
        .await
        .expect("seed row");
        sqlx::query(
            "INSERT INTO server_connection_backends (server_connection_id, backend_id, \
             desktop_pairing_id) VALUES (?1, ?2, ?3)",
        )
        .bind(id.to_string())
        .bind(match (seed.backend, nth) {
            (Some(b), _) => b.to_owned(),
            (None, 0) => BACKEND.to_owned(),
            (None, _) => ordin_types::BackendId::new().to_string(),
        })
        .bind(PAIRING_ID)
        .execute(&self.pool)
        .await
        .expect("seed backend");
        if let Some(pin) = seed.pin {
            sqlx::query(
                "INSERT INTO server_connection_pins (server_connection_id, fingerprint_sha256) \
                 VALUES (?1, ?2)",
            )
            .bind(id.to_string())
            .bind(pin)
            .execute(&self.pool)
            .await
            .expect("seed pin");
        }
        if seed.with_key {
            self.keys.put(&keychain_ref, KEY_BYTES);
        }
        id
    }
}

pub struct Seed {
    pub name: &'static str,
    pub host: &'static str,
    pub port: u16,
    pub state: &'static str,
    pub version: Option<&'static str>,
    pub last_seq: i64,
    pub pin: Option<String>,
    pub backend: Option<&'static str>,
    pub with_key: bool,
}

impl Seed {
    pub fn new(port: u16, state: &'static str) -> Seed {
        Seed {
            name: SERVER_NAME,
            host: "127.0.0.1",
            port,
            state,
            version: Some("1.0"),
            last_seq: 0,
            pin: None,
            backend: None,
            with_key: true,
        }
    }
}

/// The body of a successful `POST /v1/pairing`.
pub fn pair_reply(server_name: &str, key: &str, api: (u16, u16)) -> Value {
    json!({
        "kind": "ok",
        "result": {
            "desktop_pairing_id": PAIRING_ID,
            "desktop_key": key,
            "backend": BACKEND,
            "server_name": server_name,
            "api_version": { "major": api.0, "minor": api.1 },
        }
    })
}

pub fn error_body(code: &str) -> Value {
    json!({ "kind": "error", "error": { "code": code, "message": "A sentence." } })
}

pub fn json_response(status: u16, body: &Value) -> ResponseTemplate {
    ResponseTemplate::new(status)
        .insert_header("Ordin-Api-Version", "1.0")
        .insert_header("Content-Type", "application/json")
        .set_body_string(body.to_string())
}

pub async fn mock_pairing(mock: &MockServer, response: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path("/v1/pairing"))
        .respond_with(response)
        .mount(mock)
        .await;
}

pub async fn mock_pairing_ok(mock: &MockServer) {
    mock_pairing(
        mock,
        json_response(200, &pair_reply(SERVER_NAME, KEY_TEXT, (1, 0))),
    )
    .await;
}

/// A command envelope for `command` with the given input.
pub fn envelope(command: &str, request_id: &str, input: Value) -> bytes::Bytes {
    bytes::Bytes::from(
        json!({
            "api_version": { "major": 1, "minor": 0 },
            "request_id": request_id,
            "command": command,
            "backend": BACKEND,
            "input": input,
        })
        .to_string(),
    )
}

pub fn reply_ok(request_id: &str) -> Value {
    json!({ "kind": "ok", "request_id": request_id, "result": [] })
}

pub fn meta(name: &str) -> &'static ordin_service_api::CommandMeta {
    ordin_service_api::command(name).expect("a catalogue row")
}

pub fn backend() -> ordin_types::BackendId {
    ordin_types::BackendId::parse(BACKEND).expect("backend id")
}

/// Waits until `check` is true, polling every 10 ms, for at most `limit`.
pub async fn eventually<F, Fut>(limit: Duration, mut check: F) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let end = tokio::time::Instant::now() + limit;
    loop {
        if check().await {
            return true;
        }
        if tokio::time::Instant::now() >= end {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Reads the state text of a server's row.
pub async fn state_of(env: &Env, id: ServerConnectionId) -> String {
    use sqlx::Row as _;
    env.row(id)
        .await
        .map(|r| r.get::<String, _>("state"))
        .unwrap_or_default()
}

/// The channel the Command Bridge would own.
pub fn bridge() -> (mpsc::Sender<ServerEvent>, mpsc::Receiver<ServerEvent>) {
    mpsc::channel(64)
}

/// A frame body: the envelope JSON of an event with `seq`.
pub fn event_data(seq: u64, kind: &str, at: &str) -> String {
    let mut v = json!({
        "api_version": { "major": 1, "minor": 0 },
        "backend": BACKEND,
        "seq": seq,
        "at": at,
        "kind": kind,
    });
    if kind == "needs_you_changed" {
        v["workspace_id"] = json!("0192a1d0-1111-7222-8333-444455556667");
    }
    v.to_string()
}

/// A log capture with ordin's own line formatter (Logging.md). Installed per test on the test's own
/// thread, so no global subscriber is touched.
#[derive(Clone, Default)]
pub struct Capture(pub Arc<Mutex<Vec<u8>>>);

impl Capture {
    pub fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }

    pub fn lines(&self) -> Vec<Value> {
        self.text()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    /// The lines of one event name.
    pub fn of(&self, event: &str) -> Vec<Value> {
        self.lines()
            .into_iter()
            .filter(|l| l["event"] == event)
            .collect()
    }

    pub fn install(&self) -> tracing::subscriber::DefaultGuard {
        let writer = self.clone();
        let subscriber = tracing_subscriber::fmt()
            .event_format(ordin_types::log::JsonLineFormat)
            .with_max_level(tracing::Level::TRACE)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::set_default(subscriber)
    }
}

impl std::io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Receives `n` events or fails after `limit`.
pub async fn recv_n(
    rx: &mut mpsc::Receiver<ServerEvent>,
    n: usize,
    limit: Duration,
) -> Vec<ServerEvent> {
    let mut out = Vec::new();
    let end = tokio::time::Instant::now() + limit;
    while out.len() < n {
        match tokio::time::timeout_at(end, rx.recv()).await {
            Ok(Some(e)) => out.push(e),
            _ => break,
        }
    }
    out
}

/// `seq` of an event, `None` for a reset (as `-` in a summary string).
pub fn summary(events: &[ServerEvent]) -> Vec<String> {
    events
        .iter()
        .map(|e| match e {
            ServerEvent::Event { envelope, .. } => envelope.seq.to_string(),
            ServerEvent::Reset { .. } => "reset".to_owned(),
        })
        .collect()
}
