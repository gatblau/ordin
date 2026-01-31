//! Shared helpers: a temporary SQLite journal with the planner tables, a profile snapshot a test can
//! swap, and a fake harness.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, EntryFilter, Journal, JournalEntry, JournalOptions,
    MigrationRegistry, SystemClock, MIGRATIONS,
};
use ordin_organisation_profile::{ProfileSnapshot, UsageSection};
use ordin_runtime_adapter::{HarnessKind, HarnessModels, LevelModels};
use ordin_types::common::{AgentRef, InputUsed};
use ordin_types::time::Timestamp;
use ordin_types::wire::run::ModelLevel;
use ordin_types::{AgentInstanceId, RunId, WorkspaceId};

use crate::planner::SnapshotSource;
use crate::types::{AnswerToIndex, ReuseQuery};
use crate::UsagePlanner;

/// A folder removed when dropped.
pub(crate) struct TempDir(pub PathBuf);

impl TempDir {
    pub(crate) fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-usage-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A profile snapshot the test can replace, as the organisation profile service does on a refresh.
pub(crate) struct TestSource(Mutex<Arc<ProfileSnapshot>>);

impl TestSource {
    pub(crate) fn new(snapshot: ProfileSnapshot) -> Arc<TestSource> {
        Arc::new(TestSource(Mutex::new(Arc::new(snapshot))))
    }

    pub(crate) fn set(&self, snapshot: ProfileSnapshot) {
        *self.0.lock().unwrap() = Arc::new(snapshot);
    }
}

impl SnapshotSource for TestSource {
    fn current(&self) -> Arc<ProfileSnapshot> {
        self.0.lock().unwrap().clone()
    }
}

/// The default profile with `change` applied to its usage section.
pub(crate) fn profile(change: impl FnOnce(&mut UsageSection)) -> ProfileSnapshot {
    let mut snapshot = ProfileSnapshot::defaults();
    change(&mut snapshot.document.usage);
    snapshot
}

/// A harness with Claude Code's default map.
pub(crate) struct FakeHarness {
    pub kind: HarnessKind,
    pub defaults: LevelModels,
    pub known: Vec<&'static str>,
}

impl FakeHarness {
    pub(crate) fn claude() -> Arc<dyn HarnessModels> {
        let mut defaults = BTreeMap::new();
        defaults.insert(ModelLevel::Smaller, "haiku".to_owned());
        defaults.insert(ModelLevel::Standard, "sonnet".to_owned());
        defaults.insert(ModelLevel::MostCapable, "opus".to_owned());
        Arc::new(FakeHarness {
            kind: HarnessKind::ClaudeCode,
            defaults,
            known: vec!["haiku", "sonnet", "opus"],
        })
    }

    pub(crate) fn copilot() -> Arc<dyn HarnessModels> {
        let mut defaults = BTreeMap::new();
        defaults.insert(ModelLevel::Standard, "gpt-standard".to_owned());
        Arc::new(FakeHarness {
            kind: HarnessKind::CopilotCli,
            defaults,
            known: vec!["gpt-standard"],
        })
    }
}

impl HarnessModels for FakeHarness {
    fn harness(&self) -> HarnessKind {
        self.kind
    }

    fn default_level_models(&self) -> LevelModels {
        self.defaults.clone()
    }

    fn per_session_model(&self) -> bool {
        true
    }

    fn accepts_model(&self, name: &str) -> bool {
        self.known.contains(&name)
    }
}

pub(crate) struct Fixture {
    pub dir: TempDir,
    pub store: Datastore,
    pub journal: Arc<Journal>,
    pub source: Arc<TestSource>,
    pub planner: Arc<UsagePlanner>,
}

/// Everything the log lines of this test process wrote.
pub(crate) static LOG: Mutex<Vec<u8>> = Mutex::new(Vec::new());

struct LogWriter;

impl std::io::Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        LOG.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Installs one process-wide subscriber before any test emits an event: `tracing` caches per call
/// site whether anything listens, so a subscriber installed later would miss events (plan FU-017).
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

/// A migrated journal on a fresh SQLite file; the planner tables are applied too.
pub(crate) async fn open_store(dir: &TempDir) -> (Datastore, Arc<Journal>) {
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
        .register(UsagePlanner::migrations())
        .expect("register the usage planner module");
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("apply the migrations");
    let journal = Arc::new(
        Journal::start(
            store.clone(),
            JournalOptions {
                automatic_pruning: false,
                prune_interval: Duration::from_secs(24 * 3600),
                clock: Arc::new(SystemClock),
            },
        )
        .await
        .expect("start the journal"),
    );
    (store, journal)
}

/// The default profile and Claude Code's harness.
pub(crate) async fn fixture() -> Fixture {
    fixture_with(ProfileSnapshot::defaults(), vec![FakeHarness::claude()]).await
}

pub(crate) async fn fixture_with(
    snapshot: ProfileSnapshot,
    harnesses: Vec<Arc<dyn HarnessModels>>,
) -> Fixture {
    let dir = TempDir::new();
    let (store, journal) = open_store(&dir).await;
    let source = TestSource::new(snapshot);
    let planner = UsagePlanner::build(store.clone(), journal.clone(), source.clone(), harnesses)
        .await
        .expect("start the usage planner");
    Fixture {
        dir,
        store,
        journal,
        source,
        planner,
    }
}

impl Fixture {
    pub(crate) fn pool(&self) -> &sqlx::SqlitePool {
        self.store.sqlite().expect("a SQLite datastore")
    }

    /// A second planner over the same file, as after an application restart.
    pub(crate) async fn restart(&self) -> Arc<UsagePlanner> {
        UsagePlanner::build(
            self.store.clone(),
            self.journal.clone(),
            self.source.clone(),
            vec![FakeHarness::claude()],
        )
        .await
        .expect("restart the usage planner")
    }

    /// Every journal entry of one kind.
    pub(crate) async fn entries_of(&self, kind: &str) -> Vec<JournalEntry> {
        self.journal
            .list_entries(
                EntryFilter {
                    run_id: None,
                    kind: Some(kind.to_owned()),
                },
                None,
            )
            .await
            .expect("list journal entries")
            .items
    }
}

/// Waits (up to 5 s) until `check` holds.
pub(crate) async fn eventually<F: FnMut() -> bool>(mut check: F) {
    for _ in 0..500 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the condition did not hold within 5 s");
}

pub(crate) fn agent() -> AgentRef {
    AgentRef::AgentInstance {
        agent_instance_id: AgentInstanceId::new(),
    }
}

pub(crate) fn quote(version: &str) -> InputUsed {
    InputUsed {
        name: "Quote.pdf".to_owned(),
        location: "workspace:Sales/Quote.pdf".to_owned(),
        version_read: version.to_owned(),
    }
}

pub(crate) fn query(
    request: &str,
    agent: AgentRef,
    workspace_id: WorkspaceId,
    inputs: Vec<InputUsed>,
) -> ReuseQuery {
    ReuseQuery {
        request: request.to_owned(),
        agent,
        workspace_id,
        input_versions: inputs,
    }
}

pub(crate) fn answer(query: ReuseQuery, answered_at: Timestamp, text: &str) -> AnswerToIndex {
    AnswerToIndex {
        query,
        source_run_id: RunId::new(),
        answered_at,
        text: text.to_owned(),
        external_actions: 0,
        files_changed: 0,
        approvals: 0,
    }
}

/// `n` days before now.
pub(crate) fn days_ago(n: i64) -> Timestamp {
    Timestamp(Timestamp::now().0 - chrono::TimeDelta::days(n))
}
