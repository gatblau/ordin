//! Shared test helpers: a Journal on a temporary SQLite file with the Server Connector's migrations, a
//! real Server Connector over a hand-written `DesktopKeyStore`, a hand-written loopback stand-in for a
//! paired server's Remote API, a `ServiceApi` generated with `for_each_command!` that records each
//! call, and collectors for a window's batches and stream messages. Real network is never used.
#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_command_bridge::{
    BridgeConfig, CommandBridge, EventBatch, EventOut, StreamMessage, StreamOut,
    DEFAULT_SERVER_REPLAY_EVENTS,
};
use ordin_identity::{CredError, DesktopKeyStore};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry, SystemClock,
    MIGRATIONS,
};
use ordin_server_connector::{
    ConnectorConfig, ServerConnector, ServerEvent, ServerEventSender, StandIn as ConnectorStandIn,
    DEFAULT_READ_CACHE_BYTES,
};
use ordin_service_api::*;
use ordin_types::domain::server::ServerConnectionState;
use ordin_types::{BackendId, RepairAction, ServerConnectionId};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

/// The paired server's backend of the spec's examples.
pub const SERVER: &str = "0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b";
/// The local backend of the spec's first scenario.
pub const LOCAL: &str = "0192a0aa-0000-7000-8000-000000000001";
pub const REQUEST: &str = "0192a1c0-0d1e-7f20-8a31-425364758697";
pub const RUN: &str = "0192a1c2-3344-7556-8677-8899aabbccdd";
pub const WORKSPACE: &str = "0192a1d0-1111-7222-8333-444455556667";
pub const KEY_BYTES: [u8; 32] = [1u8; 32];
pub const PAIRING_ID: &str = "0192a1d0-1111-7222-8333-444455556666";

pub fn server() -> BackendId {
    BackendId::parse(SERVER).expect("server backend")
}

pub fn local() -> BackendId {
    BackendId::parse(LOCAL).expect("local backend")
}

// ---------------------------------------------------------------- temporary folder and keychain

pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir =
            std::env::temp_dir().join(format!("ordin-command-bridge-{}", uuid::Uuid::now_v7()));
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

/// An in-memory `DesktopKeyStore`.
#[derive(Default)]
pub struct FakeKeys {
    pub items: Mutex<HashMap<String, [u8; 32]>>,
}

#[async_trait::async_trait]
impl DesktopKeyStore for FakeKeys {
    async fn put_desktop_key(&self, keychain_ref: &str, key: &[u8; 32]) -> Result<(), CredError> {
        self.items
            .lock()
            .unwrap()
            .insert(keychain_ref.to_owned(), *key);
        Ok(())
    }

    async fn get_desktop_key(
        &self,
        keychain_ref: &str,
    ) -> Result<Option<zeroize::Zeroizing<[u8; 32]>>, CredError> {
        Ok(self
            .items
            .lock()
            .unwrap()
            .get(keychain_ref)
            .map(|k| zeroize::Zeroizing::new(*k)))
    }

    async fn delete_desktop_key(&self, keychain_ref: &str) -> Result<(), CredError> {
        self.items.lock().unwrap().remove(keychain_ref);
        Ok(())
    }

    async fn client_certificate(
        &self,
        _name: &str,
    ) -> Result<Option<Arc<rustls::sign::CertifiedKey>>, CredError> {
        Ok(None)
    }
}

// ---------------------------------------------------------------- the ServiceApi

pub type Reply = Pin<Box<dyn Future<Output = Result<Value, ServiceError>> + Send>>;
pub type Handler = Arc<dyn Fn(&'static str, Value) -> Reply + Send + Sync>;

/// One recorded call: the method and whether the origin was the local screens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub method: &'static str,
    pub local_screens: bool,
}

pub struct Mock {
    pub handler: Handler,
    pub calls: Mutex<Vec<Call>>,
    pub groups: &'static [&'static str],
}

/// Every group of the catalogue, sorted (a build with every switch).
pub fn every_group() -> &'static [&'static str] {
    static GROUPS: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    GROUPS.get_or_init(|| {
        let mut groups: Vec<&'static str> = CATALOGUE.iter().map(|m| m.module).collect();
        groups.sort_unstable();
        groups.dedup();
        groups
    })
}

/// The result shape each command used in the tests answers with.
pub fn default_result(method: &str) -> Value {
    match method {
        "approval_list_pending" => json!({ "items": [], "next_cursor": null }),
        _ => json!({}),
    }
}

impl Mock {
    pub fn new(handler: impl Fn(&'static str, Value) -> Reply + Send + Sync + 'static) -> Mock {
        Mock {
            handler: Arc::new(handler),
            calls: Mutex::new(Vec::new()),
            groups: every_group(),
        }
    }

    /// A mock answering each command with [`default_result`].
    pub fn ok() -> Mock {
        Mock::new(|method, _| Box::pin(async move { Ok(default_result(method)) }))
    }

    /// A mock answering after `wait`.
    pub fn slow(wait: Duration) -> Mock {
        Mock::new(move |method, _| {
            Box::pin(async move {
                tokio::time::sleep(wait).await;
                Ok(default_result(method))
            })
        })
    }

    pub fn with_groups(mut self, groups: &'static [&'static str]) -> Mock {
        self.groups = groups;
        self
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }
}

macro_rules! mock_methods {
    ($( ($module:ident, $verb:ident, $method:ident, $input:ty, $result:ty, $scope:ident, $effect:ident); )*) => {
        #[async_trait::async_trait]
        impl ServiceApi for Mock {
            $(
                async fn $method(&self, ctx: &CallContext, input: $input) -> Result<$result, ServiceError> {
                    self.calls.lock().unwrap().push(Call {
                        method: stringify!($method),
                        local_screens: matches!(ctx.origin, CallOrigin::LocalScreens),
                    });
                    let value = serde_json::to_value(&input).expect("input serialises");
                    let out = (self.handler)(stringify!($method), value).await?;
                    Ok(serde_json::from_value(out).expect("result has the command's shape"))
                }
            )*

            fn groups(&self) -> &'static [&'static str] {
                self.groups
            }
        }
    };
}

ordin_service_api::for_each_command!(mock_methods);

/// Sentences that name the code; no action of their own, so a reply's action is the Bridge's.
pub struct Sentences;

impl ErrorSentences for Sentences {
    fn sentence(&self, code: &str) -> String {
        format!("Sentence for {code}.")
    }

    fn action(&self, _code: &str) -> Option<RepairAction> {
        None
    }
}

// ---------------------------------------------------------------- a paired server's stand-in

/// How the stand-in answers a command route.
#[derive(Debug, Clone)]
pub enum Answer {
    /// 200 with this JSON body.
    Reply(String),
    /// Read the request, then close without answering a byte.
    Drop,
    /// Wait, then answer 200 with this body.
    Late(Duration, String),
    /// Answer with this status and body.
    Status(u16, String),
}

#[derive(Debug, Clone)]
pub struct Seen {
    pub path: String,
    pub body: Vec<u8>,
}

/// `GET /v1/events` answers a held-open event stream; every other route answers `answer`.
pub struct RemoteStandIn {
    pub port: u16,
    pub answer: Arc<Mutex<Answer>>,
    pub seen: Arc<Mutex<Vec<Seen>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for RemoteStandIn {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl RemoteStandIn {
    pub async fn start(answer: Answer) -> RemoteStandIn {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let answer = Arc::new(Mutex::new(answer));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (a, s) = (Arc::clone(&answer), Arc::clone(&seen));
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let (a, s) = (Arc::clone(&a), Arc::clone(&s));
                tokio::spawn(async move {
                    let _ = serve(stream, a, s).await;
                });
            }
        });
        RemoteStandIn {
            port,
            answer,
            seen,
            task,
        }
    }

    pub fn commands(&self) -> Vec<Seen> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.path != "/v1/events")
            .cloned()
            .collect()
    }
}

async fn read_request(stream: &mut TcpStream) -> Option<Seen> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.lines();
    let path = lines.next()?.split(' ').nth(1)?.to_owned();
    let length = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while buf.len() < head_end + length {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    Some(Seen {
        path,
        body: buf[head_end..].to_vec(),
    })
}

async fn serve(
    mut stream: TcpStream,
    answer: Arc<Mutex<Answer>>,
    seen: Arc<Mutex<Vec<Seen>>>,
) -> std::io::Result<()> {
    let Some(request) = read_request(&mut stream).await else {
        return Ok(());
    };
    let events = request.path == "/v1/events";
    seen.lock().unwrap().push(request);
    if events {
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nOrdin-Api-Version: 1.0\r\nContent-Type: text/event-stream\r\n\
                  Cache-Control: no-store\r\nTransfer-Encoding: chunked\r\n\r\n",
            )
            .await?;
        let heartbeat = ": heartbeat\n\n";
        stream
            .write_all(format!("{:x}\r\n{heartbeat}\r\n", heartbeat.len()).as_bytes())
            .await?;
        stream.flush().await?;
        let mut sink = [0u8; 64];
        while stream.read(&mut sink).await.unwrap_or(0) > 0 {}
        return Ok(());
    }
    let answer = answer.lock().unwrap().clone();
    let (status, body) = match answer {
        Answer::Drop => return Ok(()),
        Answer::Reply(body) => (200, body),
        Answer::Status(status, body) => (status, body),
        Answer::Late(wait, body) => {
            tokio::time::sleep(wait).await;
            (200, body)
        }
    };
    let head = format!(
        "HTTP/1.1 {status} X\r\nOrdin-Api-Version: 1.0\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.shutdown().await
}

/// A loopback port nothing listens on.
pub async fn closed_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    listener.local_addr().expect("addr").port()
}

// ---------------------------------------------------------------- the environment

/// A stored server, as if paired earlier.
pub struct Seed {
    pub port: u16,
    pub state: &'static str,
    pub version: Option<&'static str>,
    pub backend: &'static str,
}

impl Seed {
    pub fn new(port: u16, state: &'static str) -> Seed {
        Seed {
            port,
            state,
            version: Some("1.0"),
            backend: SERVER,
        }
    }
}

pub struct Fixture {
    pub dir: TempDir,
    pub journal: Journal,
    pub pool: SqlitePool,
    pub api: Arc<Mock>,
    pub connector: Arc<ServerConnector>,
    pub bridge: Arc<CommandBridge>,
    pub server_tx: ServerEventSender,
    pub cancel: CancellationToken,
    pub local: BackendId,
}

pub struct Options {
    pub api: Mock,
    pub seeds: Vec<Seed>,
    pub local: BackendId,
    pub time_scale: f64,
    pub replay: usize,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            api: Mock::ok(),
            seeds: Vec::new(),
            local: local(),
            time_scale: 1.0,
            replay: DEFAULT_SERVER_REPLAY_EVENTS,
        }
    }
}

pub async fn fixture() -> Fixture {
    fixture_with(Options::default()).await
}

pub async fn fixture_with(options: Options) -> Fixture {
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
    let keys = Arc::new(FakeKeys::default());
    for seed in &options.seeds {
        let id = ServerConnectionId::new();
        let keychain_ref = format!("server-connector/desktop-key/{id}");
        sqlx::query(
            "INSERT INTO server_connections (id, display_name, address, port, service_api_version, \
             last_sequence_received, keychain_ref, state, paired_at) \
             VALUES (?1, 'Northwind server', '127.0.0.1', ?2, ?3, 0, ?4, ?5, '2026-01-10T10:02:11.004Z')",
        )
        .bind(id.to_string())
        .bind(i64::from(seed.port))
        .bind(seed.version)
        .bind(&keychain_ref)
        .bind(seed.state)
        .execute(&pool)
        .await
        .expect("seed row");
        sqlx::query(
            "INSERT INTO server_connection_backends (server_connection_id, backend_id, \
             desktop_pairing_id) VALUES (?1, ?2, ?3)",
        )
        .bind(id.to_string())
        .bind(seed.backend)
        .bind(PAIRING_ID)
        .execute(&pool)
        .await
        .expect("seed backend");
        keys.put_desktop_key(&keychain_ref, &KEY_BYTES)
            .await
            .expect("key");
    }
    let connector = Arc::new(
        ServerConnector::new(
            ConnectorConfig {
                computer_name: "Asha's laptop".to_owned(),
                client_certificate: None,
                read_cache_bytes: DEFAULT_READ_CACHE_BYTES,
            },
            journal.clone(),
            keys,
        )
        .expect("connector")
        .with_stand_in(ConnectorStandIn {
            plain_http: true,
            time_scale: 1.0,
            desktop_version: ServiceApiVersion::CURRENT,
        }),
    );
    let api = Arc::new(options.api);
    let (bridge, server_tx) = CommandBridge::new(
        BridgeConfig {
            local_backend: options.local,
            server_replay_events: options.replay,
        },
        api.clone(),
        Arc::new(Sentences),
        Arc::clone(&connector),
        journal.clone(),
    );
    let bridge = Arc::new(bridge.with_time_scale(options.time_scale));
    let cancel = CancellationToken::new();
    connector
        .start(server_tx.clone(), cancel.clone())
        .await
        .expect("start the connector");
    bridge.start(cancel.clone());
    Fixture {
        dir,
        journal,
        pool,
        api,
        connector,
        bridge,
        server_tx,
        cancel,
        local: options.local,
    }
}

impl Fixture {
    /// Waits until the stored server is connected.
    pub async fn connected(&self) {
        let ok = eventually(Duration::from_secs(10), || async {
            self.connector
                .list()
                .await
                .map(|v| {
                    v.iter()
                        .any(|s| s.state == ServerConnectionState::Connected)
                })
                .unwrap_or(false)
        })
        .await;
        assert!(ok, "the server connects");
    }

    /// One command from window `main`.
    pub async fn invoke(&self, command: &str, envelope: &str) -> Value {
        let reply = self
            .bridge
            .invoke("main", command, envelope)
            .await
            .expect("a reply");
        serde_json::from_str(&reply).expect("reply is JSON")
    }

    /// Appends one local event to the Journal.
    pub async fn append(&self) -> u64 {
        self.journal
            .append_event("needs_you_changed", json!({ "workspace_id": WORKSPACE }))
            .await
            .expect("append")
            .seq
    }

    /// Sends one server event, as the Server Connector does.
    pub async fn server_event(&self, backend: BackendId, seq: u64) {
        self.server_tx
            .send(ServerEvent::Event {
                backend,
                envelope: Arc::new(event(backend, seq)),
            })
            .await
            .expect("server channel open");
    }

    pub async fn server_reset(&self, backend: BackendId) {
        self.server_tx
            .send(ServerEvent::Reset { backend })
            .await
            .expect("server channel open");
    }

    pub async fn open(&self, window: &str, after: BTreeMap<BackendId, u64>) -> (Window, Value) {
        let window_out = Window::default();
        let opened = self
            .bridge
            .open_events(window, after, window_out.out())
            .await
            .expect("stream opens");
        (window_out, serde_json::to_value(opened).unwrap())
    }
}

/// A `needs_you_changed` envelope of `backend` with `seq`.
pub fn event(backend: BackendId, seq: u64) -> EventEnvelope {
    EventEnvelope::new(
        backend,
        seq,
        ordin_types::time::Timestamp::now(),
        "needs_you_changed",
        json!({ "workspace_id": WORKSPACE }),
    )
    .expect("a valid event")
}

/// An envelope for `command` with `input` and `backend`.
pub fn envelope(command: &str, backend: &str, input: Value) -> String {
    json!({
        "api_version": { "major": 1, "minor": 0 },
        "request_id": REQUEST,
        "command": command,
        "backend": backend,
        "input": input,
    })
    .to_string()
}

/// Waits until `check` is true, polling every 10 ms, for at most `limit`.
pub async fn eventually<F, Fut>(limit: Duration, mut check: F) -> bool
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
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

// ---------------------------------------------------------------- what a window receives

/// The batches one window received, as JSON.
#[derive(Clone, Default)]
pub struct Window {
    pub batches: Arc<Mutex<Vec<Vec<Value>>>>,
    /// When set, the window stops reading: `out` blocks until it is cleared.
    pub paused: Arc<Mutex<bool>>,
    pub gone: Arc<Mutex<bool>>,
    /// How many batches met a closed channel.
    pub refused: Arc<Mutex<usize>>,
}

impl Window {
    pub fn out(&self) -> EventOut {
        let me = self.clone();
        Box::new(move |batch: EventBatch| {
            while *me.paused.lock().unwrap() {
                std::thread::sleep(Duration::from_millis(5));
            }
            if *me.gone.lock().unwrap() {
                *me.refused.lock().unwrap() += 1;
                return Err(());
            }
            let events = batch
                .events
                .iter()
                .map(|e| serde_json::from_str(e.get()).unwrap())
                .collect();
            me.batches.lock().unwrap().push(events);
            Ok(())
        })
    }

    pub fn events(&self) -> Vec<Value> {
        self.batches.lock().unwrap().concat()
    }

    pub fn sizes(&self) -> Vec<usize> {
        self.batches.lock().unwrap().iter().map(Vec::len).collect()
    }

    pub async fn wait_for(&self, n: usize) -> Vec<Value> {
        let ok = eventually(Duration::from_secs(10), || async {
            self.events().len() >= n
        })
        .await;
        assert!(ok, "{n} events arrive; got {}", self.events().len());
        self.events()
    }

    pub fn seqs(&self) -> Vec<u64> {
        self.events()
            .iter()
            .map(|e| e["seq"].as_u64().unwrap())
            .collect()
    }
}

/// The messages the main window's unjournalled stream received.
#[derive(Clone, Default)]
pub struct StreamWindow {
    pub messages: Arc<Mutex<Vec<Value>>>,
    pub paused: Arc<Mutex<bool>>,
    pub gone: Arc<Mutex<bool>>,
}

impl StreamWindow {
    pub fn out(&self) -> StreamOut {
        let me = self.clone();
        Box::new(move |message: StreamMessage| {
            while *me.paused.lock().unwrap() {
                std::thread::sleep(Duration::from_millis(5));
            }
            if *me.gone.lock().unwrap() {
                return Err(());
            }
            me.messages
                .lock()
                .unwrap()
                .push(serde_json::to_value(message).unwrap());
            Ok(())
        })
    }

    pub fn all(&self) -> Vec<Value> {
        self.messages.lock().unwrap().clone()
    }

    pub async fn wait_for(&self, n: usize) -> Vec<Value> {
        let ok = eventually(Duration::from_secs(10), || async { self.all().len() >= n }).await;
        assert!(ok, "{n} messages arrive");
        self.all()
    }
}

// ---------------------------------------------------------------- log capture

/// A log capture with ordin's own line formatter (Logging.md), installed on the test's thread.
#[derive(Clone, Default)]
pub struct Capture(pub Arc<Mutex<Vec<u8>>>);

impl Capture {
    pub fn lines(&self) -> Vec<Value> {
        String::from_utf8(self.0.lock().unwrap().clone())
            .unwrap()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    pub fn of(&self, event: &str) -> Vec<Value> {
        self.lines()
            .into_iter()
            .filter(|l| l["event"] == event)
            .collect()
    }

    pub fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
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
