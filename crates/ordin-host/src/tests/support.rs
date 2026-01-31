//! Shared helpers: a migrated SQLite datastore with this crate's tables, the stand-ins of
//! `crate::testkit` behind every port, and the host built over them.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_journal::{ApplyMode, Datastore, DatastoreConfig, MigrationRegistry, MIGRATIONS};
use ordin_types::{AgentInstanceId, RunId, WorkspaceId};
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;

#[allow(unused_imports)]
pub(crate) use crate::ports::*;
pub(crate) use crate::testkit::*;
use crate::types::{OrdinHostConfig, OrdinHostPolicy};
use crate::OrdinHost;

/// A folder removed when dropped.
pub(crate) struct TempDir(pub PathBuf);

impl TempDir {
    pub(crate) fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-host-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Everything the log lines of this test process wrote.
pub(crate) static LOG: Mutex<Vec<u8>> = Mutex::new(Vec::new());

struct LogWriter;

impl std::io::Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        LOG.lock()
            .unwrap_or_else(|p| p.into_inner())
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// One process-wide subscriber, installed before any test logs (plan FU-017).
pub(crate) fn init_logging() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        use tracing_subscriber::layer::SubscriberExt;
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(ordin_types::log::JsonLineFormat)
                .with_writer(|| LogWriter),
        );
        tracing::subscriber::set_global_default(subscriber).expect("install the subscriber");
    });
}

/// The log lines written so far that contain `needle`.
pub(crate) fn log_lines(needle: &str) -> Vec<String> {
    let bytes = LOG.lock().unwrap_or_else(|p| p.into_inner()).clone();
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter(|l| l.contains(needle))
        .map(str::to_owned)
        .collect()
}

/// A migrated datastore holding the Journal's and this crate's tables.
pub(crate) async fn open_store(dir: &TempDir) -> Datastore {
    init_logging();
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.0.join("data").join("journal.db"),
        backup_dir: dir.0.join("data").join("backups"),
    })
    .await
    .expect("open the SQLite datastore");
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("register the journal");
    registry
        .register(OrdinHost::migrations())
        .expect("register the ordin host module");
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("apply the migrations");
    store
}

// ---------------------------------------------------------------- the fixture

pub(crate) struct Fixture {
    pub dir: TempDir,
    pub store: Datastore,
    pub pool: SqlitePool,
    pub f: Arc<Fails>,
    pub supervisor: Arc<FakeSupervisor>,
    pub orchestrator: Arc<FakeOrchestrator>,
    pub inspector: Arc<FakeInspector>,
    pub usage: Arc<FakeUsage>,
    pub workspaces: Arc<FakeWorkspaces>,
    pub journal: Arc<FakeJournal>,
    pub catalogue: Arc<FakeCatalogue>,
    pub memory: Arc<FakeMemory>,
    pub knowledge: Arc<FakeKnowledge>,
    pub push: Arc<FakePush>,
    pub projects: Arc<FakeProjects>,
    pub localisation: Arc<FakeLocalisation>,
    pub blocks: Arc<FakeBlocks>,
    pub files: Arc<FakeFiles>,
    pub host: Arc<OrdinHost>,
    pub ws: WorkspaceId,
    pub shutdown: CancellationToken,
}

pub(crate) async fn fixture() -> Fixture {
    fixture_with(config(), policy()).await
}

pub(crate) async fn fixture_with(config: OrdinHostConfig, policy: OrdinHostPolicy) -> Fixture {
    let dir = TempDir::new();
    let store = open_store(&dir).await;
    let pool = store.sqlite().cloned().expect("a SQLite pool");
    let k = Fakes::new();
    let Fakes {
        f,
        supervisor,
        orchestrator,
        inspector,
        usage,
        workspaces,
        journal,
        catalogue,
        memory,
        knowledge,
        push,
        projects,
        localisation,
        blocks,
        files,
        ports,
    } = k;
    let host = OrdinHost::with_ports(config, policy, ports, pool.clone());
    let shutdown = CancellationToken::new();
    host.start(shutdown.clone()).await.expect("the host starts");
    Fixture {
        dir,
        store,
        pool,
        f,
        supervisor,
        orchestrator,
        inspector,
        usage,
        workspaces,
        journal,
        catalogue,
        memory,
        knowledge,
        push,
        projects,
        localisation,
        blocks,
        files,
        host,
        ws: WorkspaceId::new(),
        shutdown,
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.shutdown.cancel();
        self.host.background.cancel();
    }
}

/// Waits up to two seconds for `cond`.
pub(crate) async fn eventually(mut cond: impl FnMut() -> bool) -> bool {
    for _ in 0..200 {
        if cond() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    cond()
}

impl Fixture {
    /// A tool-call context for Ordin's current run in the fixture's workspace.
    pub(crate) fn ctx(&self) -> ordin_mcp_front::ToolCallContext {
        self.ctx_for(self.host.current_run(self.ws).expect("Ordin has a run"))
    }

    pub(crate) fn ctx_for(&self, run_id: RunId) -> ordin_mcp_front::ToolCallContext {
        ordin_mcp_front::ToolCallContext {
            run_id,
            workspace_id: self.ws,
            agent_instance_id: AgentInstanceId::new(),
            caller: ordin_mcp_front::ToolCaller::Ordin,
            scope_id: None,
            coding_task_id: None,
            call_id: ordin_types::ToolRequestId::new(),
            correlation_id: "test".to_owned(),
            deadline: tokio::time::Instant::now().into_std() + Duration::from_secs(30),
            cancel: CancellationToken::new(),
            progress: ordin_mcp_front::ProgressSink::detached(),
            only_needed_parts: false,
            via: ordin_mcp_front::CallVia::Harness,
        }
    }

    /// Calls one of Ordin's tools in the current run.
    pub(crate) async fn tool(
        &self,
        name: &str,
        args: serde_json::Value,
    ) -> Result<ordin_mcp_front::ToolOutput, ordin_mcp_front::ToolFailure> {
        let ctx = self.ctx();
        self.host.tool(&ctx, name, args).await
    }

    /// Opens Ordin's conversation with one message and waits for the reply to be stored.
    pub(crate) async fn open(&self) -> RunId {
        self.host
            .open_conversation(self.ws)
            .await
            .expect("the conversation opens");
        let host = self.host.clone();
        let ws = self.ws;
        assert!(eventually(|| host.current_run(ws).is_some()).await);
        let run = self.host.current_run(self.ws).unwrap();
        // The seed turn's answer frees the conversation.
        let host = self.host.clone();
        assert!(
            eventually(|| {
                crate::host::lock(&host.state)
                    .convs
                    .get(&ws)
                    .map(|c| !c.busy)
                    .unwrap_or(false)
            })
            .await
        );
        run
    }

    /// The person sends `text` with `attachments` (step 4), so the host knows their latest message
    /// (OH-TF-5, OH-ACC-2).
    pub(crate) async fn say(&self, text: &str, attachments: Vec<ordin_types::wire::ContentRef>) {
        self.host
            .send_message(self.ws, text.to_owned(), attachments)
            .await
            .expect("the message is accepted");
    }

    /// The count of rows a query returns.
    pub(crate) async fn count(&self, sql: &'static str) -> i64 {
        sqlx::query_scalar::<_, i64>(sql)
            .fetch_one(&self.pool)
            .await
            .expect("a count")
    }
}
