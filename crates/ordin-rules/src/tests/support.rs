//! Shared helpers of the in-crate tests: a temporary SQLite journal with the approval and rules
//! tables, a scripted `RulesHost`, a fixed organisation profile and builders for rule sets and calls.
//! Nothing here skips; a missing fixture fails the test.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_approval::{ApprovalConfig, ApprovalService};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry, SystemClock,
    MIGRATIONS,
};
use ordin_organisation_profile::ProfileSnapshot;
use ordin_types::domain::agents::Authority;
use ordin_types::wire::approval::AffectedItem;
use ordin_types::wire::rules::{ActionProposal, DecisionPoint, ToolCall, ToolOrigin};
use ordin_types::{AgentInstanceId, BackendId, RunId, WorkspaceId};

use crate::engine::{ProfileSource, RulesEngine};
use crate::types::{
    CardWording, ContentChecker, ControlLine, FactError, MetricsSink, ProgramKind, Proposal,
    RulesConfig, RulesHost, RunKey, RunStartFacts,
};
use crate::vocab::{InstanceRuleSet, MessageRule, ToolDescription};

/// A folder removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-rules-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The organisation profile the engine reads, changeable by a test.
pub struct FixedProfile(pub Mutex<Arc<ProfileSnapshot>>);

impl ProfileSource for FixedProfile {
    fn current(&self) -> Arc<ProfileSnapshot> {
        self.0.lock().unwrap().clone()
    }
}

/// One metric added: its family and its labels.
pub type Added = (String, Vec<(String, String)>);

/// Counts every metric added.
#[derive(Default)]
pub struct Metrics(pub Mutex<Vec<Added>>);

impl MetricsSink for Metrics {
    fn add(&self, name: &str, labels: &[(&str, &str)], _by: u64) {
        self.0.lock().unwrap().push((
            name.to_owned(),
            labels
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        ));
    }
}

/// A scripted host: work-folder files, runs in progress, settings and a switch that fails facts.
#[derive(Default)]
pub struct TestHost {
    pub files: Mutex<HashMap<String, String>>,
    pub in_progress: Mutex<HashMap<String, u32>>,
    pub paused: AtomicBool,
    pub ended: Mutex<HashMap<String, Option<bool>>>,
    pub settings: Mutex<HashMap<String, serde_json::Value>>,
    pub rules: Mutex<Vec<(String, MessageRule)>>,
    pub fail: AtomicBool,
    pub appended: Mutex<Vec<(String, String)>>,
    pub removed: Mutex<Vec<String>>,
    pub checker: Mutex<Option<Arc<dyn ContentChecker>>>,
    pub metrics: Mutex<Option<Arc<Metrics>>>,
    pub proposals: Mutex<Vec<String>>,
}

impl TestHost {
    fn fact<T>(&self, value: T) -> Result<T, FactError> {
        if self.fail.load(Ordering::SeqCst) {
            Err(FactError("scripted failure".into()))
        } else {
            Ok(value)
        }
    }
}

#[async_trait::async_trait]
impl RulesHost for TestHost {
    async fn path_exists(&self, _run: &RunKey, path: &str) -> Result<bool, FactError> {
        let found = self.files.lock().unwrap().contains_key(path);
        self.fact(found)
    }
    async fn path_contains(
        &self,
        _run: &RunKey,
        path: &str,
        text: &str,
    ) -> Result<bool, FactError> {
        let found = self
            .files
            .lock()
            .unwrap()
            .get(path)
            .is_some_and(|c| c.contains(text));
        self.fact(found)
    }
    async fn read_text(&self, _run: &RunKey, path: &str) -> Result<Option<String>, FactError> {
        let found = self.files.lock().unwrap().get(path).cloned();
        self.fact(found)
    }
    async fn append_line(&self, _run: &RunKey, path: &str, line: &str) -> Result<(), FactError> {
        self.fact(())?;
        self.appended
            .lock()
            .unwrap()
            .push((path.to_owned(), line.to_owned()));
        let mut files = self.files.lock().unwrap();
        let entry = files.entry(path.to_owned()).or_default();
        entry.push_str(line);
        entry.push('\n');
        Ok(())
    }
    async fn remove(&self, _run: &RunKey, path: &str) -> Result<(), FactError> {
        self.fact(())?;
        self.files.lock().unwrap().remove(path);
        self.removed.lock().unwrap().push(path.to_owned());
        Ok(())
    }
    async fn runs_in_progress(&self, command: &str) -> Result<u32, FactError> {
        let n = self
            .in_progress
            .lock()
            .unwrap()
            .get(command)
            .copied()
            .unwrap_or(1);
        self.fact(n)
    }
    async fn paused_run_with_command(
        &self,
        _commands: &[String],
        _except: RunId,
    ) -> Result<bool, FactError> {
        self.fact(self.paused.load(Ordering::SeqCst))
    }
    async fn run_ended(&self, run_id: &str) -> Result<Option<bool>, FactError> {
        let state = self
            .ended
            .lock()
            .unwrap()
            .get(run_id)
            .copied()
            .unwrap_or(None);
        self.fact(state)
    }
    async fn setting_value(
        &self,
        _scope: Option<&str>,
        name: &str,
    ) -> Result<serde_json::Value, FactError> {
        let value = self
            .settings
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        self.fact(value)
    }
    async fn setting_rules(&self, _scope: &str) -> Result<Vec<(String, MessageRule)>, FactError> {
        let rules = self.rules.lock().unwrap().clone();
        self.fact(rules)
    }
    fn word(&self, _run: &RunKey, proposal: &Proposal) -> CardWording {
        self.proposals
            .lock()
            .unwrap()
            .push(format!("{proposal:?}").chars().take(40).collect());
        let affects = vec![AffectedItem::Record {
            label: "The run's work".to_owned(),
        }];
        let shell = matches!(proposal, Proposal::ShellCommand { .. });
        CardWording {
            question: "Allow this action?".to_owned(),
            verb: "Allow".to_owned(),
            where_line: "This computer".to_owned(),
            affects: affects.clone(),
            changes: "The action runs.".to_owned(),
            content_label: shell.then(|| "See the command".to_owned()),
            proposal: ActionProposal {
                action: "Run a tool".to_owned(),
                destination: "This computer".to_owned(),
                affects,
                consequence: "The action runs.".to_owned(),
            },
        }
    }
    fn plain_line(&self, line: &ControlLine) -> String {
        format!("{} applies.", line.name)
    }
    fn content_checker(&self) -> Option<Arc<dyn ContentChecker>> {
        self.checker.lock().unwrap().clone()
    }
    fn metrics_sink(&self) -> Option<Arc<dyn MetricsSink>> {
        self.metrics
            .lock()
            .unwrap()
            .clone()
            .map(|m| m as Arc<dyn MetricsSink>)
    }
}

pub struct Fixture {
    /// Held so the temporary folder lives as long as the fixture.
    pub _dir: TempDir,
    pub store: Datastore,
    pub journal: Arc<Journal>,
    pub approvals: Arc<ApprovalService>,
    pub host: Arc<TestHost>,
    pub profile: Arc<FixedProfile>,
    pub engine: Arc<RulesEngine>,
    pub data_root: PathBuf,
}

/// An absolute path literal on this platform: on Windows a drive goes before the root.
#[cfg(unix)]
macro_rules! abs {
    ($path:literal) => {
        $path
    };
}
#[cfg(windows)]
macro_rules! abs {
    ($path:literal) => {
        concat!("C:", $path)
    };
}
pub(crate) use abs;

pub const APP_ROOT: &str = abs!("/Users/asha/Library/Application Support/ordin");
pub const HOME: &str = abs!("/Users/asha");

pub fn config(program: ProgramKind) -> RulesConfig {
    let home = Path::new(HOME);
    RulesConfig {
        ordin_servers: [
            "ordin",
            "work",
            "sandbox",
            "memory",
            "code",
            "mail",
            "telegram",
            "slack",
            "knowledge",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect(),
        ordin_server_prefixes: vec!["craft_".to_owned()],
        protected_roots: vec![
            PathBuf::from(abs!("/System")),
            PathBuf::from(abs!("/etc")),
            PathBuf::from(abs!("/private")),
            home.join(".ssh"),
            home.join(".aws"),
            home.join("Library/Keychains"),
            // The program passes its data root as a protected root too (`wiring.rs` `protected_roots`), so
            // the tests hold the same list.
            PathBuf::from(APP_ROOT),
        ],
        app_data_root: PathBuf::from(APP_ROOT),
        program,
        instance_name: "local".to_owned(),
    }
}

pub async fn fixture() -> Fixture {
    fixture_with(ProgramKind::Desktop).await
}

pub async fn fixture_with(program: ProgramKind) -> Fixture {
    let dir = TempDir::new();
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.0.join("data").join("journal.db"),
        backup_dir: dir.0.join("data").join("backups"),
    })
    .await
    .expect("open the SQLite datastore");
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("journal");
    registry
        .register(ApprovalService::migrations())
        .expect("approval");
    registry.register(RulesEngine::migrations()).expect("rules");
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
    let approvals = ApprovalService::new(
        store.clone(),
        journal.clone(),
        ApprovalConfig {
            backend: BackendId(journal.backend_uuid()),
            local_computer_name: "Asha's laptop".to_owned(),
            approvers: None,
        },
    )
    .await
    .expect("approval service");
    let host = Arc::new(TestHost::default());
    let profile = Arc::new(FixedProfile(Mutex::new(Arc::new(
        ProfileSnapshot::defaults(),
    ))));
    let engine = RulesEngine::build(
        store.clone(),
        journal.clone(),
        profile.clone(),
        approvals.clone(),
        host.clone(),
        config(program),
    );
    Fixture {
        data_root: PathBuf::from(APP_ROOT),
        _dir: dir,
        store,
        journal,
        approvals,
        host,
        profile,
        engine,
    }
}

impl Fixture {
    pub fn pool(&self) -> &sqlx::SqlitePool {
        self.store.sqlite().expect("SQLite")
    }

    /// Changes the organisation profile through its JSON document.
    pub fn set_profile(&self, change: serde_json::Value) {
        let mut document = serde_json::to_value(&ProfileSnapshot::defaults().document).unwrap();
        merge(&mut document, change);
        let snapshot = ProfileSnapshot {
            document: serde_json::from_value(document).expect("a valid profile document"),
            source: ordin_organisation_profile::SnapshotSource::Defaults,
        };
        *self.profile.0.lock().unwrap() = Arc::new(snapshot);
    }

    pub fn metrics(&self) -> Arc<Metrics> {
        let metrics = Arc::new(Metrics::default());
        *self.host.metrics.lock().unwrap() = Some(metrics.clone());
        metrics
    }

    pub async fn drop_table(&self, table: &str) {
        let sql = format!("DROP TABLE {table}");
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .execute(self.pool())
            .await
            .expect("drop a table");
    }

    pub async fn count(&self, sql: &'static str) -> i64 {
        sqlx::query_scalar::<_, i64>(sql)
            .fetch_one(self.pool())
            .await
            .expect("count")
    }

    /// The agent folder of an instance under the application data root.
    pub fn folder(&self, name: &str) -> PathBuf {
        self.data_root.join("agents").join(name)
    }
}

fn merge(into: &mut serde_json::Value, change: serde_json::Value) {
    match (into, change) {
        (serde_json::Value::Object(target), serde_json::Value::Object(source)) => {
            for (key, value) in source {
                match target.get_mut(&key) {
                    Some(existing) if existing.is_object() && value.is_object() => {
                        merge(existing, value)
                    }
                    _ => {
                        target.insert(key, value);
                    }
                }
            }
        }
        (target, change) => *target = change,
    }
}

pub fn tool(name: &str, arguments: &[&str]) -> ToolDescription {
    ToolDescription {
        name: name.to_owned(),
        arguments: arguments.iter().map(|a| (*a).to_owned()).collect(),
        served_by: None,
    }
}

/// A rule set with nothing declared.
pub fn rule_set(id: AgentInstanceId, authority: Authority, folder: &Path) -> InstanceRuleSet {
    InstanceRuleSet {
        agent_instance_id: id,
        package_name: "analyst".to_owned(),
        authority,
        agent_folder: folder.to_path_buf(),
        agent_folder_read_only: false,
        declaration: None,
        configured: None,
        tool_policy: None,
        approval_list: Vec::new(),
        package_read_only_tools: Vec::new(),
        tools: Vec::new(),
        unmapped_operations: Vec::new(),
        external: Vec::new(),
        commands: Vec::new(),
        reviewer: None,
    }
}

pub fn start(run: RunId, agent: AgentInstanceId, workspace: WorkspaceId) -> RunStartFacts {
    RunStartFacts {
        run_id: run,
        agent_instance_id: agent,
        workspace_id: workspace,
        crew_charter_id: None,
        command: None,
        text: String::new(),
        scope: None,
        asker: None,
        started_by: "person".to_owned(),
        started_by_id: "person:asha".to_owned(),
    }
}

pub fn call(
    run: RunId,
    agent: AgentInstanceId,
    workspace: WorkspaceId,
    tool: &str,
    origin: ToolOrigin,
    arguments: serde_json::Value,
) -> ToolCall {
    ToolCall {
        run_id: run,
        agent_instance_id: agent,
        workspace_id: workspace,
        point: DecisionPoint::BeforeToolCall,
        tool: tool.to_owned(),
        origin,
        arguments,
    }
}

/// An agent instance with a loaded set and one started run.
pub struct Running {
    pub agent: AgentInstanceId,
    pub run: RunId,
    pub workspace: WorkspaceId,
}

impl Running {
    pub fn call(&self, tool: &str, origin: ToolOrigin, arguments: serde_json::Value) -> ToolCall {
        call(
            self.run,
            self.agent,
            self.workspace,
            tool,
            origin,
            arguments,
        )
    }

    pub fn builtin(&self, tool: &str, arguments: serde_json::Value) -> ToolCall {
        self.call(tool, ToolOrigin::BuiltIn, arguments)
    }
}

/// Loads `set` and begins one run of it with `command`.
pub async fn running(f: &Fixture, set: InstanceRuleSet, command: Option<&str>) -> Running {
    let agent = set.agent_instance_id;
    f.engine.load_rule_set(set).expect("the set loads");
    let run = RunId::new();
    let workspace = WorkspaceId::new();
    let mut facts = start(run, agent, workspace);
    facts.command = command.map(str::to_owned);
    f.engine.begin_run(facts).await.expect("the run begins");
    Running {
        agent,
        run,
        workspace,
    }
}
