//! Shared test helpers: a migrated SQLite or PostgreSQL datastore, a recording `MetricsSink`, input
//! builders, a capturing log subscriber and the scenario bodies that run on both engines.
//! A missing container runtime fails the test; nothing here skips.
#![allow(dead_code)]

pub mod pg;
pub mod scenarios;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use ordin_journal::{ApplyMode, Datastore, DatastoreConfig, MigrationRegistry, MIGRATIONS};
use ordin_run_queue::{
    DatastoreRunQueue, MetricsSink, NewRun, PendingItem, PendingKind, RunEnd, RunOutcome, RunPause,
    RunQueue, RunQueueError,
};
use ordin_types::domain::agent_profile::{RunEndKind, RunRecord, RunRequest, RunStarter};
use ordin_types::domain::TaskArgument;
use ordin_types::ScopeId;
use sqlx::AssertSqlSafe;
use tracing_subscriber::layer::SubscriberExt;

/// A folder removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-run-queue-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One recorded counter call: name, labels, amount.
type Call = (String, Vec<(String, String)>, u64);

/// A `MetricsSink` that keeps every call.
#[derive(Default)]
pub struct Recording {
    calls: Mutex<Vec<Call>>,
}

impl MetricsSink for Recording {
    fn add(&self, name: &'static str, labels: &[(&'static str, &str)], by: u64) {
        let labels = labels
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        self.calls
            .lock()
            .unwrap()
            .push((name.to_owned(), labels, by));
    }
}

impl Recording {
    /// The sum of `by` over the calls of `name` whose labels include every pair in `labels`.
    pub fn total(&self, name: &str, labels: &[(&str, &str)]) -> u64 {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(n, l, _)| {
                n == name
                    && labels
                        .iter()
                        .all(|(k, v)| l.iter().any(|(lk, lv)| lk == k && lv == v))
            })
            .map(|(_, _, by)| *by)
            .sum()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Sqlite,
    Postgres,
}

/// A migrated datastore with a queue over it and the recording sink.
pub struct Harness {
    pub which: Which,
    pub datastore: Datastore,
    pub metrics: Arc<Recording>,
    pub queue: DatastoreRunQueue,
    pub url: Option<String>,
    /// Pools opened by `instance`, closed by `finish`.
    extra: Mutex<Vec<Datastore>>,
    _dir: Option<TempDir>,
}

impl Harness {
    /// A fresh SQLite file with the journal's and the run queue's migrations applied.
    pub async fn sqlite() -> Harness {
        let dir = TempDir::new();
        let datastore = Datastore::open(DatastoreConfig::Sqlite {
            path: dir.0.join("data").join("runs.db"),
            backup_dir: dir.0.join("data").join("backups"),
        })
        .await
        .expect("open the SQLite datastore");
        let mut registry = MigrationRegistry::new();
        registry.register(MIGRATIONS).expect("register the journal");
        registry
            .register(ordin_run_queue::migrations())
            .expect("register the run queue");
        registry
            .apply(&datastore, ApplyMode::ApplyPending)
            .await
            .expect("apply the migrations");
        Harness::over(Which::Sqlite, datastore, None, Some(dir))
    }

    /// A copy of the migrated template on the shared container.
    pub async fn postgres() -> Harness {
        let url = pg::migrated_database().await;
        let datastore = pg::open_url(url.clone(), 10).await;
        Harness::over(Which::Postgres, datastore, Some(url), None)
    }

    fn over(
        which: Which,
        datastore: Datastore,
        url: Option<String>,
        dir: Option<TempDir>,
    ) -> Harness {
        // The capturing subscriber is installed before any queue call (callsite interest is cached).
        logs();
        let metrics = Arc::new(Recording::default());
        let queue = DatastoreRunQueue::new(&datastore, metrics.clone());
        Harness {
            which,
            datastore,
            metrics,
            queue,
            url,
            extra: Mutex::new(Vec::new()),
            _dir: dir,
        }
    }

    /// Another queue on the same database: a new one-connection pool on PostgreSQL (one "instance"),
    /// the shared pool on SQLite. It has its own recording sink.
    pub async fn instance(&self) -> (DatastoreRunQueue, Arc<Recording>) {
        let metrics = Arc::new(Recording::default());
        let datastore = match &self.url {
            Some(url) => pg::open_url(url.clone(), 1).await,
            None => self.datastore.clone(),
        };
        if self.url.is_some() {
            self.extra.lock().unwrap().push(datastore.clone());
        }
        (DatastoreRunQueue::new(&datastore, metrics.clone()), metrics)
    }

    /// Closes every pool `instance` opened, so a scenario that opened fifty gives their
    /// connections back to the shared server.
    pub async fn finish(&self) {
        let pools: Vec<Datastore> = self.extra.lock().unwrap().drain(..).collect();
        for pool in pools {
            pool.close().await;
        }
    }

    /// Runs one statement (no result), with the text for each engine; `$1` is bound to `arg`.
    pub async fn exec(&self, sqlite: &'static str, postgres: &'static str, arg: Option<&str>) {
        match &self.datastore {
            Datastore::Sqlite(pool) => {
                let mut q = sqlx::query(AssertSqlSafe(sqlite.to_owned()));
                if let Some(arg) = arg {
                    q = q.bind(arg.to_owned());
                }
                q.execute(pool).await.expect("test statement");
            }
            Datastore::Postgres(pool) => {
                let mut q = sqlx::query(AssertSqlSafe(postgres.to_owned()));
                if let Some(arg) = arg {
                    q = q.bind(arg.to_owned());
                }
                q.execute(pool).await.expect("test statement");
            }
        }
    }

    /// Puts the run's lease one second in the past.
    pub async fn lapse_lease(&self, run_id: &str) {
        self.exec(
            "UPDATE run_records SET lease_expires_at = strftime('%Y-%m-%dT%H:%M:%fZ','now','-1 seconds') WHERE id = $1",
            "UPDATE run_records SET lease_expires_at = now() - interval '1 second' WHERE id = $1::uuid",
            Some(run_id),
        )
        .await;
    }

    /// Moves an ended run's `ended_at` back 100 days.
    pub async fn age_end(&self, run_id: &str) {
        self.exec(
            "UPDATE run_records SET ended_at = strftime('%Y-%m-%dT%H:%M:%fZ','now','-100 days') WHERE id = $1",
            "UPDATE run_records SET ended_at = now() - interval '100 days' WHERE id = $1::uuid",
            Some(run_id),
        )
        .await;
    }

    pub async fn count(&self, table: &'static str) -> i64 {
        let sql = match table {
            "run_records" => "SELECT count(*) FROM run_records",
            "run_threads" => "SELECT count(*) FROM run_threads",
            "run_events" => "SELECT count(*) FROM run_events",
            other => panic!("unknown table {other}"),
        };
        match &self.datastore {
            Datastore::Sqlite(pool) => sqlx::query_scalar::<_, i64>(AssertSqlSafe(sql.to_owned()))
                .fetch_one(pool)
                .await
                .expect("count"),
            Datastore::Postgres(pool) => {
                sqlx::query_scalar::<_, i64>(AssertSqlSafe(sql.to_owned()))
                    .fetch_one(pool)
                    .await
                    .expect("count")
            }
        }
    }
}

pub fn agent_wide() -> ScopeId {
    ScopeId::new()
}

pub fn caller(id: &str) -> RunStarter {
    RunStarter::Caller {
        caller_id: id.to_owned(),
    }
}

pub fn command_request(command: &str, argument: &str) -> RunRequest {
    RunRequest::Command {
        command: command.to_owned(),
        arguments: vec![TaskArgument {
            name: "arguments".to_owned(),
            value: argument.to_owned(),
        }],
    }
}

/// A `NewRun` on a new thread with every optional part empty.
pub fn new_run(started_by: RunStarter, request: RunRequest) -> NewRun {
    NewRun {
        started_by,
        scope_id: agent_wide(),
        request,
        gathered_context: serde_json::json!({}),
        thread_id: None,
        run_id: None,
    }
}

pub fn go_no_go() -> NewRun {
    new_run(
        caller("svc-release-desk"),
        command_request("run-go-no-go", "release 4.12"),
    )
}

pub fn message(text: &str) -> NewRun {
    new_run(
        caller("svc-release-desk"),
        RunRequest::Message {
            text: text.to_owned(),
        },
    )
}

pub async fn enqueue(q: &DatastoreRunQueue, new: NewRun) -> RunRecord {
    q.enqueue(new).await.expect("enqueue")
}

/// Enqueues a run and takes it as `instance`.
pub async fn taken(q: &DatastoreRunQueue, instance: &str) -> RunRecord {
    let record = enqueue(q, go_no_go()).await;
    let got = q.take(instance).await.expect("take").expect("a queued run");
    assert_eq!(got.view.record.run_id, record.run_id);
    got.view.record
}

pub fn completed(reply: &str) -> RunEnd {
    RunEnd {
        end_kind: RunEndKind::Completed,
        outcome: RunOutcome {
            reply: Some(reply.to_owned()),
            ..Default::default()
        },
        stretch_summary: None,
    }
}

pub fn tool_pending(tool: &str) -> PendingItem {
    PendingItem {
        kind: PendingKind::ToolCall,
        tool: Some(tool.to_owned()),
        gate: None,
        reason: "approval-list".to_owned(),
        approval_id: None,
    }
}

pub fn pause_with(items: Vec<PendingItem>, session: &[u8]) -> RunPause {
    RunPause {
        outcome: RunOutcome {
            pending: items,
            ..Default::default()
        },
        session_record: session.to_vec(),
        stretch_summary: None,
    }
}

/// The code of an error result, or "ok".
pub fn code<T>(result: &Result<T, RunQueueError>) -> &'static str {
    match result {
        Ok(_) => "ok",
        Err(error) => error.code(),
    }
}

pub fn expect_code<T: std::fmt::Debug>(result: Result<T, RunQueueError>, expected: &str) {
    match result {
        Ok(value) => panic!("expected {expected}, got Ok({value:?})"),
        Err(error) => assert_eq!(error.code(), expected),
    }
}

/// Every log line of this process, as JSON lines (the Logging formatter), captured from the first
/// call on. Installed once per test binary before any queue call.
#[derive(Clone, Default)]
pub struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for LogBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl LogBuffer {
    pub fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }

    pub fn lines(&self) -> Vec<serde_json::Value> {
        self.text()
            .lines()
            .map(|line| serde_json::from_str(line).expect("a JSON line"))
            .collect()
    }
}

static LOGS: OnceLock<LogBuffer> = OnceLock::new();

/// Installs the capturing subscriber on first use and returns its buffer.
pub fn logs() -> LogBuffer {
    LOGS.get_or_init(|| {
        let buffer = LogBuffer::default();
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(ordin_types::log::JsonLineFormat)
                .with_writer(move || writer.clone()),
        );
        tracing::subscriber::set_global_default(subscriber).expect("install the subscriber");
        buffer
    })
    .clone()
}
