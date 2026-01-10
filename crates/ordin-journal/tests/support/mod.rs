//! Shared test helpers: a temporary SQLite journal, a settable clock and recording sinks.
#![allow(dead_code)]

pub mod pg;
pub mod steps;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_journal::{
    ApplyMode, Clock, Datastore, DatastoreConfig, EventSink, Journal, JournalEvent, JournalOptions,
    MigrationRegistry, MigrationStep, ModuleMigrations, PruneFailure, PruneParticipant,
    PruneRequest, MIGRATIONS,
};
use ordin_types::time::Timestamp;
use ordin_types::CodedError;

/// A folder removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-journal-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// `<dir>/data/journal.db`, with backups in `<dir>/data/backups`.
    pub fn db_file(&self) -> PathBuf {
        self.0.join("data").join("journal.db")
    }

    pub fn backup_dir(&self) -> PathBuf {
        self.0.join("data").join("backups")
    }

    pub fn backups(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.backup_dir())
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .filter(|n| n.starts_with("journal-") && n.ends_with(".db"))
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub async fn open_sqlite(dir: &TempDir) -> Datastore {
    Datastore::open(DatastoreConfig::Sqlite {
        path: dir.db_file(),
        backup_dir: dir.backup_dir(),
    })
    .await
    .expect("open the SQLite datastore")
}

/// Registers the journal's own steps and applies them.
pub async fn migrate(store: &Datastore) {
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("register the journal");
    registry
        .apply(store, ApplyMode::ApplyPending)
        .await
        .expect("apply the journal's migrations");
}

/// A clock the test moves.
pub struct TestClock(Mutex<Timestamp>);

impl TestClock {
    pub fn at(raw: &str) -> Arc<TestClock> {
        Arc::new(TestClock(Mutex::new(ts(raw))))
    }

    pub fn now_real() -> Arc<TestClock> {
        Arc::new(TestClock(Mutex::new(Timestamp::now())))
    }

    pub fn now_value(&self) -> Timestamp {
        *self.0.lock().unwrap()
    }

    pub fn set(&self, at: Timestamp) {
        *self.0.lock().unwrap() = at;
    }

    pub fn advance_days(&self, days: i64) {
        let mut now = self.0.lock().unwrap();
        *now = Timestamp(now.0 + chrono::Duration::days(days));
    }

    pub fn advance_ms(&self, ms: i64) {
        let mut now = self.0.lock().unwrap();
        *now = Timestamp(now.0 + chrono::Duration::milliseconds(ms));
    }
}

impl Clock for TestClock {
    fn now(&self) -> Timestamp {
        *self.0.lock().unwrap()
    }
}

pub fn ts(raw: &str) -> Timestamp {
    serde_json::from_value(serde_json::Value::String(raw.to_owned())).expect("timestamp")
}

pub fn options(clock: Arc<dyn Clock>) -> JournalOptions {
    JournalOptions {
        automatic_pruning: false,
        prune_interval: Duration::from_secs(24 * 3600),
        clock,
    }
}

/// A migrated, started journal on a fresh SQLite file.
pub struct Fixture {
    pub dir: TempDir,
    pub journal: Journal,
    pub clock: Arc<TestClock>,
}

pub async fn sqlite_journal() -> Fixture {
    let dir = TempDir::new();
    let store = open_sqlite(&dir).await;
    migrate(&store).await;
    let clock = TestClock::now_real();
    let journal = Journal::start(store, options(clock.clone()))
        .await
        .expect("start the journal");
    Fixture {
        dir,
        journal,
        clock,
    }
}

/// A migrated, started journal on a copy of the PostgreSQL template.
pub async fn pg_journal() -> (Journal, Arc<TestClock>) {
    let store = pg::open_url(pg::migrated_database().await).await;
    let clock = TestClock::now_real();
    let journal = Journal::start(store, options(clock.clone()))
        .await
        .expect("start the journal");
    (journal, clock)
}

/// The code, the retry flag and the repair action of an error.
pub fn assert_code(error: &dyn CodedError, code: &str, retryable: bool) {
    assert_eq!(error.code(), code);
    assert_eq!(error.retryable(), retryable, "{code} retryable");
}

/// A sink that records every `seq` it receives and every `lagged` call; `deliver` can be held.
pub struct RecordingSink {
    pub name: &'static str,
    pub seen: Mutex<Vec<u64>>,
    pub lagged: Mutex<Vec<u64>>,
    pub gate: Option<tokio::sync::Semaphore>,
}

impl RecordingSink {
    pub fn new(name: &'static str) -> Arc<RecordingSink> {
        Arc::new(RecordingSink {
            name,
            seen: Mutex::new(Vec::new()),
            lagged: Mutex::new(Vec::new()),
            gate: None,
        })
    }

    /// `deliver` waits until `release` is called.
    pub fn held(name: &'static str) -> Arc<RecordingSink> {
        Arc::new(RecordingSink {
            name,
            seen: Mutex::new(Vec::new()),
            lagged: Mutex::new(Vec::new()),
            gate: Some(tokio::sync::Semaphore::new(0)),
        })
    }

    pub fn release(&self) {
        if let Some(gate) = &self.gate {
            gate.add_permits(1_000_000);
        }
    }

    pub fn seen(&self) -> Vec<u64> {
        self.seen.lock().unwrap().clone()
    }

    /// Waits until `count` events were seen, at most 10 seconds.
    pub async fn wait_for(&self, count: usize) {
        for _ in 0..1_000 {
            if self.seen.lock().unwrap().len() >= count {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!(
            "{} saw {} events, expected {count}",
            self.name,
            self.seen.lock().unwrap().len()
        );
    }
}

#[async_trait::async_trait]
impl EventSink for RecordingSink {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn deliver(&self, event: &JournalEvent) {
        if let Some(gate) = &self.gate {
            let _ = gate.acquire().await.map(|p| p.forget());
        }
        self.seen.lock().unwrap().push(event.seq);
    }

    async fn lagged(&self, next_seq: u64) {
        self.lagged.lock().unwrap().push(next_seq);
    }
}

/// A prune participant that records its request and answers as told.
pub struct Participant {
    pub module: &'static str,
    pub answer: Result<u64, &'static str>,
    pub requests: Mutex<Vec<PruneRequest>>,
}

impl Participant {
    pub fn new(module: &'static str, answer: Result<u64, &'static str>) -> Arc<Participant> {
        Arc::new(Participant {
            module,
            answer,
            requests: Mutex::new(Vec::new()),
        })
    }
}

#[async_trait::async_trait]
impl PruneParticipant for Participant {
    fn module(&self) -> &'static str {
        self.module
    }

    async fn prune(&self, _store: &Datastore, request: &PruneRequest) -> Result<u64, PruneFailure> {
        self.requests.lock().unwrap().push(request.clone());
        self.answer.map_err(|code| PruneFailure { code })
    }
}

/// A module with the given steps on both engines.
pub fn module(name: &'static str, steps: &'static [MigrationStep]) -> ModuleMigrations {
    ModuleMigrations {
        module: name,
        sqlite: steps,
        postgres: steps,
    }
}

/// The rows of `schema_migrations`, in (module, version) order.
pub async fn applied(store: &Datastore) -> Vec<(String, i64)> {
    match store {
        Datastore::Sqlite(pool) => sqlx::query_as::<_, (String, i64)>(
            "SELECT module, version FROM schema_migrations ORDER BY module, version",
        )
        .fetch_all(pool)
        .await
        .unwrap(),
        Datastore::Postgres(pool) => sqlx::query_as::<_, (String, i64)>(
            "SELECT module, version FROM schema_migrations ORDER BY module, version",
        )
        .fetch_all(pool)
        .await
        .unwrap(),
    }
}

/// Runs one statement on either engine (test set-up only).
pub async fn exec(store: &Datastore, sql: &'static str) {
    match store {
        Datastore::Sqlite(pool) => {
            sqlx::raw_sql(sql).execute(pool).await.unwrap();
        }
        Datastore::Postgres(pool) => {
            sqlx::raw_sql(sql).execute(pool).await.unwrap();
        }
    }
}

/// One integer from a query on either engine.
pub async fn count(store: &Datastore, sql: &'static str) -> i64 {
    match store {
        Datastore::Sqlite(pool) => sqlx::query_scalar::<_, i64>(sql)
            .fetch_one(pool)
            .await
            .unwrap(),
        Datastore::Postgres(pool) => sqlx::query_scalar::<_, i64>(sql)
            .fetch_one(pool)
            .await
            .unwrap(),
    }
}

pub fn digest() -> ordin_types::num::Sha256Hex {
    ordin_types::num::Sha256Hex("ab".repeat(32))
}
