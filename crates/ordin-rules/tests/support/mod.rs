//! Shared helpers of this crate's integration tests. They use the public surface only: the engine is
//! built with `RulesEngine::new` over a real SQLite journal, the real Approval Service and an
//! organisation profile service in its `NotUsed` mode (the defaults). Nothing here skips.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_approval::{ApprovalConfig, ApprovalService};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry, SystemClock,
    MIGRATIONS,
};
use ordin_organisation_profile::{
    Clock, OrganisationProfileService, ProfileMode, ProfileOptions, Timestamp,
};
use ordin_rules::{
    CardWording, CheckerUnreachable, ContentCheckReply, ContentCheckRequest, ContentChecker,
    ControlLine, FactError, InstanceRuleSet, MessageRule, ProgramKind, Proposal, RulesConfig,
    RulesEngine, RulesHost, RunKey, RunStartFacts,
};
use ordin_types::domain::agents::Authority;
use ordin_types::wire::approval::AffectedItem;
use ordin_types::wire::rules::{ActionProposal, DecisionPoint, ToolCall, ToolOrigin};
use ordin_types::{AgentInstanceId, BackendId, RunId, WorkspaceId};

pub const APP_ROOT: &str = "/Users/asha/Library/Application Support/ordin";

/// A folder removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-rules-it-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct WallClock;

impl Clock for WallClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

/// A host whose work-folder files a test sets, with an optional content checker.
#[derive(Default)]
pub struct Host {
    pub files: Mutex<HashMap<String, String>>,
    pub checker: Mutex<Option<Arc<dyn ContentChecker>>>,
}

#[async_trait::async_trait]
impl RulesHost for Host {
    async fn path_exists(&self, _run: &RunKey, path: &str) -> Result<bool, FactError> {
        Ok(self.files.lock().unwrap().contains_key(path))
    }
    async fn path_contains(
        &self,
        _run: &RunKey,
        path: &str,
        text: &str,
    ) -> Result<bool, FactError> {
        Ok(self
            .files
            .lock()
            .unwrap()
            .get(path)
            .is_some_and(|c| c.contains(text)))
    }
    async fn read_text(&self, _run: &RunKey, path: &str) -> Result<Option<String>, FactError> {
        Ok(self.files.lock().unwrap().get(path).cloned())
    }
    async fn append_line(&self, _run: &RunKey, path: &str, line: &str) -> Result<(), FactError> {
        let mut files = self.files.lock().unwrap();
        let entry = files.entry(path.to_owned()).or_default();
        entry.push_str(line);
        entry.push('\n');
        Ok(())
    }
    async fn remove(&self, _run: &RunKey, path: &str) -> Result<(), FactError> {
        self.files.lock().unwrap().remove(path);
        Ok(())
    }
    async fn runs_in_progress(&self, _command: &str) -> Result<u32, FactError> {
        Ok(1)
    }
    async fn paused_run_with_command(
        &self,
        _commands: &[String],
        _except: RunId,
    ) -> Result<bool, FactError> {
        Ok(false)
    }
    async fn run_ended(&self, _run_id: &str) -> Result<Option<bool>, FactError> {
        Ok(None)
    }
    async fn setting_value(
        &self,
        _scope: Option<&str>,
        _name: &str,
    ) -> Result<serde_json::Value, FactError> {
        Ok(serde_json::Value::Null)
    }
    async fn setting_rules(&self, _scope: &str) -> Result<Vec<(String, MessageRule)>, FactError> {
        Ok(Vec::new())
    }
    fn word(&self, _run: &RunKey, _proposal: &Proposal) -> CardWording {
        let affects = vec![AffectedItem::Record {
            label: "The run's work".to_owned(),
        }];
        CardWording {
            question: "Allow this action?".to_owned(),
            verb: "Allow".to_owned(),
            where_line: "This computer".to_owned(),
            affects: affects.clone(),
            changes: "The action runs.".to_owned(),
            content_label: None,
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
}

/// The checker over HTTP, as the server program binds it: a JSON POST to
/// `<base>/<connection>/<operation>`, with the control's timeout.
pub struct HttpChecker {
    pub base: String,
    pub client: reqwest::Client,
}

#[async_trait::async_trait]
impl ContentChecker for HttpChecker {
    async fn check(
        &self,
        connection: &str,
        operation: &str,
        request: &ContentCheckRequest,
        timeout: Duration,
    ) -> Result<ContentCheckReply, CheckerUnreachable> {
        let url = format!("{}/{connection}/{operation}", self.base);
        let response = self
            .client
            .post(url)
            .json(request)
            .timeout(timeout)
            .send()
            .await
            .map_err(|error| CheckerUnreachable {
                reason: if error.is_timeout() {
                    "timeout"
                } else {
                    "refused"
                },
            })?;
        if !response.status().is_success() {
            return Err(CheckerUnreachable { reason: "status" });
        }
        response
            .json::<ContentCheckReply>()
            .await
            .map_err(|_| CheckerUnreachable { reason: "reply" })
    }
}

pub struct Fixture {
    _dir: TempDir,
    pub store: Datastore,
    pub journal: Arc<Journal>,
    pub approvals: Arc<ApprovalService>,
    pub host: Arc<Host>,
    pub engine: Arc<RulesEngine>,
}

pub fn config(program: ProgramKind) -> RulesConfig {
    RulesConfig {
        ordin_servers: ["ordin", "work", "code", "mail"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
        ordin_server_prefixes: vec!["craft_".to_owned()],
        protected_roots: vec![
            PathBuf::from("/System"),
            PathBuf::from("/etc"),
            PathBuf::from("/Users/asha/.ssh"),
        ],
        app_data_root: PathBuf::from(APP_ROOT),
        program,
        instance_name: "local".to_owned(),
    }
}

pub async fn fixture(program: ProgramKind) -> Fixture {
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
    let profile = Arc::new(
        OrganisationProfileService::start(
            ProfileMode::NotUsed,
            ProfileOptions {
                refresh_interval: Duration::from_secs(3600),
                clock: Arc::new(WallClock),
            },
        )
        .await
        .expect("profile service"),
    );
    let host = Arc::new(Host::default());
    let engine = RulesEngine::new(
        store.clone(),
        journal.clone(),
        profile,
        approvals.clone(),
        host.clone(),
        config(program),
    );
    Fixture {
        _dir: dir,
        store,
        journal,
        approvals,
        host,
        engine,
    }
}

impl Fixture {
    pub fn pool(&self) -> &sqlx::SqlitePool {
        self.store.sqlite().expect("SQLite")
    }

    pub fn folder(&self, name: &str) -> PathBuf {
        Path::new(APP_ROOT).join("agents").join(name)
    }
}

/// A rule set with nothing declared.
pub fn rule_set(authority: Authority, folder: &Path) -> InstanceRuleSet {
    InstanceRuleSet {
        agent_instance_id: AgentInstanceId::new(),
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

/// An agent instance with a loaded set and one started run.
pub struct Running {
    pub agent: AgentInstanceId,
    pub run: RunId,
    pub workspace: WorkspaceId,
}

impl Running {
    pub fn call(&self, tool: &str, origin: ToolOrigin, arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            run_id: self.run,
            agent_instance_id: self.agent,
            workspace_id: self.workspace,
            point: DecisionPoint::BeforeToolCall,
            tool: tool.to_owned(),
            origin,
            arguments,
        }
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
