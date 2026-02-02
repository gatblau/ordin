//! Shared helpers of this crate's tests: a real Journal on a temporary SQLite file, a real Identity
//! Service over an encrypted secret file, and doubles of the three seams the program implements
//! (`SandboxGrants`, `PlaceFiles`) or the MCP Front serves (`ToolDispatch`). Nothing here skips
//! and nothing reaches the network.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::Engine as _;
use ordin_identity::{IdentityConfig, IdentityService, KeySource, Program, SecretBackendConfig};
use ordin_journal::{
    ApplyMode, ArtefactFilter, Datastore, DatastoreConfig, EntryFilter, Journal, JournalEntry,
    JournalOptions, MigrationRegistry, SystemClock, MIGRATIONS,
};
use ordin_mcp_front::{
    CallVia, McpError, ProgressSink, ServerName, ToolCallContext, ToolCallResult, ToolCaller,
    ToolDispatch, ToolKey, ToolSpec,
};
use ordin_sandbox::{
    GrantsUnavailable, PlaceFileError, PlaceFiles, Sandbox, SandboxCapability, SandboxDeps,
    SandboxGrants,
};
use ordin_types::domain::places::Artefact;
use ordin_types::num::Sha256Hex;
use ordin_types::{AgentInstanceId, PlaceId, RunId, ToolRequestId, WorkspaceId};
use serde_json::{json, Value};
use sha2::Digest;
use tokio_util::sync::CancellationToken;

// ---- logs ----

static LOGS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static LOG_INIT: Once = Once::new();

struct Capture(Vec<u8>);

impl std::io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let text = String::from_utf8_lossy(&self.0).into_owned();
        if let Ok(mut logs) = LOGS.lock() {
            logs.push(text);
        }
    }
}

struct CaptureMaker;

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CaptureMaker {
    type Writer = Capture;
    fn make_writer(&'a self) -> Capture {
        Capture(Vec::new())
    }
}

/// Installs the process-wide capture of log lines, written by the programs' own formatter.
pub fn capture_logs() {
    LOG_INIT.call_once(|| {
        let subscriber = ordin_types::log::subscriber(CaptureMaker, tracing::Level::DEBUG);
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

/// Every captured log line that contains all of `needles`.
pub fn log_lines(needles: &[&str]) -> Vec<String> {
    LOGS.lock()
        .map(|logs| {
            logs.iter()
                .filter(|line| needles.iter().all(|n| line.contains(n)))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

// ---- files ----

/// A folder removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-sbx-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn sha_hex(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

// ---- real services ----

pub async fn journal(dir: &TempDir) -> Journal {
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.0.join("data").join("journal.db"),
        backup_dir: dir.0.join("data").join("backups"),
    })
    .await
    .expect("open the SQLite datastore");
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("register the journal");
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("apply the migrations");
    Journal::start(
        store,
        JournalOptions {
            automatic_pruning: false,
            prune_interval: Duration::from_secs(24 * 3600),
            clock: Arc::new(SystemClock),
        },
    )
    .await
    .expect("start the journal")
}

/// A desktop Identity Service with no credential; the operating system's keychain is never used.
pub async fn identity(dir: &TempDir) -> Arc<IdentityService> {
    let root = dir.0.join("identity");
    std::fs::create_dir_all(&root).expect("identity folder");
    let key_file = root.join("secrets.key");
    std::fs::write(&key_file, b64(&[7u8; 32])).expect("key file");
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(root.join("identity.db"))
        .create_if_missing(true);
    let pool = sqlx::SqlitePool::connect_with(options).await.expect("pool");
    for (_, _, sql) in ordin_identity::SQLITE_MIGRATIONS {
        sqlx::raw_sql(*sql)
            .execute(&pool)
            .await
            .expect("identity migration");
    }
    let config = IdentityConfig {
        program: Program::Desktop,
        data_root: root,
        keychain_service: "io.gatblau.ordin.sandbox-test".to_owned(),
        secret_backend: SecretBackendConfig::EncryptedFile {
            key_source: KeySource::KeyFile(key_file),
        },
        github_app_client_id: None,
        http: reqwest::Client::new(),
        browser: None,
    };
    Arc::new(
        IdentityService::new(config, Some(pool))
            .await
            .expect("the identity service starts"),
    )
}

// ---- doubles ----

/// The program's grants: a fixed answer, or unavailable.
#[derive(Default)]
pub struct Grants {
    pub answer: Mutex<Option<Vec<SandboxCapability>>>,
    pub calls: Mutex<usize>,
}

impl Grants {
    pub fn set(&self, caps: Vec<SandboxCapability>) {
        *self.answer.lock().unwrap() = Some(caps);
    }
    pub fn fail(&self) {
        *self.answer.lock().unwrap() = None;
    }
}

#[async_trait]
impl SandboxGrants for Grants {
    async fn sandbox_capabilities(
        &self,
        _agent: AgentInstanceId,
        _workspace: WorkspaceId,
    ) -> Result<Vec<SandboxCapability>, GrantsUnavailable> {
        *self.calls.lock().unwrap() += 1;
        self.answer.lock().unwrap().clone().ok_or(GrantsUnavailable)
    }
}

/// The program's place files: granted places with their files, and the agent's folder on disk.
pub struct Places {
    pub agent_root: PathBuf,
    pub granted: Mutex<HashSet<PlaceId>>,
    pub files: Mutex<HashMap<(PlaceId, String), Vec<u8>>>,
    pub unreadable: Mutex<bool>,
    pub fail_writes: Mutex<bool>,
    pub reads: Mutex<usize>,
}

impl Places {
    pub fn new(agent_root: PathBuf) -> Places {
        Places {
            agent_root,
            granted: Mutex::new(HashSet::new()),
            files: Mutex::new(HashMap::new()),
            unreadable: Mutex::new(false),
            fail_writes: Mutex::new(false),
            reads: Mutex::new(0),
        }
    }

    /// Grants `place` holding `path` with `bytes`.
    pub fn put(&self, place: PlaceId, path: &str, bytes: &[u8]) {
        self.granted.lock().unwrap().insert(place);
        self.files
            .lock()
            .unwrap()
            .insert((place, path.to_owned()), bytes.to_vec());
    }

    pub fn written(&self, relative: &str) -> Option<Vec<u8>> {
        std::fs::read(self.agent_root.join(relative)).ok()
    }
}

#[async_trait]
impl PlaceFiles for Places {
    async fn read_for_agent(
        &self,
        _agent: AgentInstanceId,
        _workspace: WorkspaceId,
        place: PlaceId,
        path: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, PlaceFileError> {
        *self.reads.lock().unwrap() += 1;
        if !self.granted.lock().unwrap().contains(&place) {
            return Err(PlaceFileError::NotGranted);
        }
        if *self.unreadable.lock().unwrap() {
            return Err(PlaceFileError::Unreadable);
        }
        let bytes = self
            .files
            .lock()
            .unwrap()
            .get(&(place, path.to_owned()))
            .cloned()
            .ok_or(PlaceFileError::NotFound)?;
        if bytes.len() as u64 > max_bytes {
            return Err(PlaceFileError::TooLarge);
        }
        Ok(bytes)
    }

    async fn write_agent_output(
        &self,
        _agent: AgentInstanceId,
        relative_path: &str,
        bytes: &[u8],
    ) -> Result<Sha256Hex, PlaceFileError> {
        if *self.fail_writes.lock().unwrap() {
            return Err(PlaceFileError::Unreadable);
        }
        let target = self.agent_root.join(relative_path);
        std::fs::create_dir_all(target.parent().expect("parent")).expect("agent folder");
        std::fs::write(&target, bytes).expect("write output");
        Ok(Sha256Hex(sha_hex(bytes)))
    }
}

/// The MCP Front's `ToolDispatch`: a fixed list of sandbox-callable tools, recorded calls.
#[derive(Default)]
pub struct Dispatch {
    pub tools: Mutex<Option<Vec<(ToolKey, ToolSpec)>>>,
    pub calls: Mutex<Vec<(RunId, ToolKey, Value)>>,
    pub answer: Mutex<Option<ToolCallResult>>,
}

pub fn tool(server: &str, name: &str) -> (ToolKey, ToolSpec) {
    (
        ToolKey {
            server: ServerName::new(server).unwrap(),
            tool: name.to_owned(),
        },
        ToolSpec {
            name: name.to_owned(),
            description: "A tool.".to_owned(),
            input_schema: json!({"type": "object"}),
            argument_form: format!("{name}()"),
            sandbox_callable: true,
            plain: "Can do a thing.".to_owned(),
        },
    )
}

impl Dispatch {
    pub fn offer(&self, tools: Vec<(ToolKey, ToolSpec)>) {
        *self.tools.lock().unwrap() = Some(tools);
    }
}

#[async_trait]
impl ToolDispatch for Dispatch {
    fn session_tools(&self, run_id: RunId) -> Result<Vec<(ToolKey, ToolSpec)>, McpError> {
        self.tools
            .lock()
            .unwrap()
            .clone()
            .ok_or(McpError::SessionUnknown { run_id })
    }

    async fn dispatch(
        &self,
        run_id: RunId,
        key: ToolKey,
        arguments: Value,
        _cancel: CancellationToken,
    ) -> Result<ToolCallResult, McpError> {
        self.calls.lock().unwrap().push((run_id, key, arguments));
        Ok(self
            .answer
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(ToolCallResult {
                is_error: false,
                text: "found".to_owned(),
                structured: Some(json!({"hits": 1})),
                code: None,
            }))
    }
}

// ---- the fixture ----

pub struct Fixture {
    pub dir: TempDir,
    pub journal: Journal,
    pub identity: Arc<IdentityService>,
    pub grants: Arc<Grants>,
    pub places: Arc<Places>,
    pub dispatch: Arc<Dispatch>,
    pub sandbox: Arc<Sandbox>,
    pub agent: AgentInstanceId,
    pub workspace: WorkspaceId,
}

pub struct Parts {
    pub dir: TempDir,
    pub journal: Journal,
    pub identity: Arc<IdentityService>,
    pub grants: Arc<Grants>,
    pub places: Arc<Places>,
    pub dispatch: Arc<Dispatch>,
}

/// The services and doubles, before the Sandbox starts. The agent has no capabilities.
pub async fn parts() -> Parts {
    capture_logs();
    let dir = TempDir::new();
    let journal = journal(&dir).await;
    let identity = identity(&dir).await;
    let grants = Arc::new(Grants::default());
    grants.set(Vec::new());
    let places = Arc::new(Places::new(dir.0.join("agent")));
    let dispatch = Arc::new(Dispatch::default());
    dispatch.offer(Vec::new());
    Parts {
        dir,
        journal,
        identity,
        grants,
        places,
        dispatch,
    }
}

impl Parts {
    pub fn deps(&self, session_idle: Duration) -> SandboxDeps {
        SandboxDeps {
            dispatch: self.dispatch.clone(),
            grants: self.grants.clone(),
            identity: self.identity.clone(),
            journal: self.journal.clone(),
            place_files: self.places.clone(),
            session_idle,
            data_root: self.dir.0.join("app"),
            log_level: "info".to_owned(),
        }
    }

    pub async fn start(self) -> Fixture {
        let sandbox = Sandbox::start(self.deps(Duration::from_secs(30 * 60))).await;
        Fixture {
            dir: self.dir,
            journal: self.journal,
            identity: self.identity,
            grants: self.grants,
            places: self.places,
            dispatch: self.dispatch,
            sandbox,
            agent: AgentInstanceId::new(),
            workspace: WorkspaceId::new(),
        }
    }
}

/// A started Sandbox with status Ready, for one agent with no capabilities.
pub async fn fixture() -> Fixture {
    parts().await.start().await
}

pub fn python() -> ToolKey {
    ToolKey {
        server: ServerName::new("sandbox").unwrap(),
        tool: "python_sandbox".to_owned(),
    }
}

impl Fixture {
    pub fn ctx(&self, run: RunId) -> ToolCallContext {
        self.ctx_as(run, ToolCaller::Agent)
    }

    pub fn ctx_as(&self, run: RunId, caller: ToolCaller) -> ToolCallContext {
        ToolCallContext {
            run_id: run,
            workspace_id: self.workspace,
            agent_instance_id: self.agent,
            caller,
            scope_id: None,
            coding_task_id: None,
            call_id: ToolRequestId::new(),
            correlation_id: "test".to_owned(),
            deadline: Instant::now() + Duration::from_secs(60),
            cancel: CancellationToken::new(),
            progress: ProgressSink::detached(),
            only_needed_parts: false,
            via: CallVia::Harness,
        }
    }

    /// Calls `python_sandbox` for `run` with `arguments`.
    pub async fn call(
        &self,
        run: RunId,
        arguments: Value,
    ) -> Result<Value, ordin_mcp_front::ToolFailure> {
        let provider = self.sandbox.tool_provider();
        provider
            .call(self.ctx(run), &python(), arguments)
            .await
            .map(|out| {
                let structured = out.structured.expect("a structured result");
                assert_eq!(out.text, structured.to_string());
                structured
            })
    }

    pub async fn entries(&self, kind: &str) -> Vec<JournalEntry> {
        self.journal
            .list_entries(
                EntryFilter {
                    run_id: None,
                    kind: Some(kind.to_owned()),
                },
                None,
            )
            .await
            .expect("entries")
            .items
    }

    pub async fn artefacts(&self) -> Vec<Artefact> {
        self.journal
            .list_artefacts(ArtefactFilter::default(), None)
            .await
            .expect("artefacts")
            .items
    }

    pub fn session_dir(&self, run: RunId) -> PathBuf {
        self.dir
            .0
            .join("app")
            .join("sandbox")
            .join("sessions")
            .join(run.to_string())
    }
}
