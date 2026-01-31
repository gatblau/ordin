//! Shared helpers of this crate's integration tests. They use the public surface only: the Rules
//! Engine, the Approval Service, the Session Supervisor and the Journal are the real crates over a
//! real SQLite file; the only stand-ins are a channel-driven runtime adapter (the harness) and the
//! hosts the programs would implement. Nothing here skips.
#![allow(dead_code)]

pub mod pg;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::stream::{BoxStream, StreamExt};
use ordin_approval::{ApprovalConfig, ApprovalService, PressOrigin};
use ordin_hook_relay::{HookRelay, HookRelayConfig, HookSessionEnv, HookSessionSpec};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry, SystemClock,
    MIGRATIONS,
};
use ordin_organisation_profile::{OrganisationProfileService, ProfileMode, ProfileOptions};
use ordin_rules::{
    CardWording, ContentChecker, ControlLine, FactError, InstanceRuleSet, MessageRule, ProgramKind,
    Proposal, RulesConfig, RulesEngine, RulesHost, RunKey, RunStartFacts,
};
use ordin_runtime_adapter::{
    AdapterContext, AdapterError, AdapterKind, AdapterVerb, ResumeDecision, RunEvent, RunHandle,
    RunSnapshot, RunTarget, RuntimeAdapter, StartInput, StartRun,
};
use ordin_session_supervisor::{
    ContentCheck, ContentPoint, HostError, RunControls, SessionSupervisor, StartRequest,
    SupervisorConfig, SupervisorHost,
};
use ordin_types::domain::agent_profile::{CallPlace, ModelFailureCause};
use ordin_types::domain::agents::Authority;
use ordin_types::domain::runs::{ApprovalState, Run, RunOrigin};
use ordin_types::wire::approval::{AffectedItem, ApprovalPress, PressDecision};
use ordin_types::wire::hook::HarnessKind;
use ordin_types::wire::rules::ActionProposal;
use ordin_types::wire::startup::StartUpReportItem;
use ordin_types::{
    AgentInstanceId, ApprovalId, BackendId, RemoteAgentId, RunId, TaskId, WorkspaceId,
};
use ordin_usage_planner::{UsagePlanner, WorkKind};
use tokio::sync::mpsc;

pub const APP_ROOT: &str = "/Users/asha/Library/Application Support/ordin";

/// A short folder removed when dropped (a Unix socket path is limited to 104 bytes on macOS).
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "hri-{}",
            &uuid::Uuid::now_v7().simple().to_string()[20..]
        ));
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

impl ordin_organisation_profile::Clock for WallClock {
    fn now(&self) -> ordin_organisation_profile::Timestamp {
        ordin_organisation_profile::Timestamp::now()
    }
}

/// The Rules Engine's host: no work-folder files, no checker, fixed card wording.
#[derive(Default)]
pub struct RulesHostStub;

#[async_trait::async_trait]
impl RulesHost for RulesHostStub {
    async fn path_exists(&self, _run: &RunKey, _path: &str) -> Result<bool, FactError> {
        Ok(false)
    }
    async fn path_contains(
        &self,
        _run: &RunKey,
        _path: &str,
        _text: &str,
    ) -> Result<bool, FactError> {
        Ok(false)
    }
    async fn read_text(&self, _run: &RunKey, _path: &str) -> Result<Option<String>, FactError> {
        Ok(None)
    }
    async fn append_line(&self, _run: &RunKey, _path: &str, _line: &str) -> Result<(), FactError> {
        Ok(())
    }
    async fn remove(&self, _run: &RunKey, _path: &str) -> Result<(), FactError> {
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
        None
    }
}

/// The Session Supervisor's host: the answers a run needs, nothing more.
pub struct SupervisorHostStub;

#[async_trait::async_trait]
impl SupervisorHost for SupervisorHostStub {
    async fn startup_items(
        &self,
        _run: &Run,
        _target: &RunTarget,
        _unsupported: &[AdapterVerb],
    ) -> Vec<StartUpReportItem> {
        vec![]
    }
    fn remote_route(&self, _remote_agent_id: RemoteAgentId) -> Option<AdapterKind> {
        None
    }
    async fn expand_command(
        &self,
        _target: &RunTarget,
        _name: &str,
        _arguments: &str,
    ) -> Result<String, HostError> {
        Ok(String::new())
    }
    async fn run_controls(&self, _run_id: RunId) -> RunControls {
        RunControls::default()
    }
    async fn run_resuming(&self, _run_id: RunId) -> Option<String> {
        None
    }
    async fn run_ended(&self, _run_id: RunId) {}
    fn run_abandoned(&self, _run_id: RunId, _adapter: AdapterKind) {}
    fn reply_for(&self, _cause: ModelFailureCause, _place: CallPlace) -> Option<String> {
        None
    }
    fn retry_after_block(&self, _carried: bool, _already_retried: bool) -> bool {
        false
    }
    async fn check_content(
        &self,
        _run_id: RunId,
        _point: ContentPoint,
        _text: &str,
    ) -> ContentCheck {
        ContentCheck::Passed
    }
    async fn run_starting(&self, _start: &StartRun) {}
}

/// The harness: a local adapter whose session stays running until the test says otherwise. It
/// records every resume the supervisor passes it, so a test can see a late decision arrive.
pub struct HoldAdapter {
    pub resumes: Mutex<Vec<(RunId, ResumeDecision)>>,
    channels: Mutex<std::collections::HashMap<RunId, mpsc::UnboundedSender<RunEvent>>>,
    receivers: Mutex<std::collections::HashMap<RunId, mpsc::UnboundedReceiver<RunEvent>>>,
}

impl HoldAdapter {
    pub fn new() -> Arc<HoldAdapter> {
        Arc::new(HoldAdapter {
            resumes: Mutex::new(vec![]),
            channels: Mutex::new(Default::default()),
            receivers: Mutex::new(Default::default()),
        })
    }

    /// Sends one event on the run's stream.
    pub fn push(&self, run_id: RunId, event: RunEvent) {
        if let Some(sender) = self.channels.lock().unwrap().get(&run_id) {
            let _ = sender.send(event);
        }
    }
}

#[async_trait::async_trait]
impl RuntimeAdapter for HoldAdapter {
    fn kind(&self) -> AdapterKind {
        AdapterKind::ClaudeLocal
    }
    fn unsupported_verbs(&self) -> &'static [AdapterVerb] {
        &[]
    }
    async fn start(&self, _ctx: &AdapterContext, req: StartRun) -> Result<RunHandle, AdapterError> {
        let (tx, rx) = mpsc::unbounded_channel();
        let _ = tx.send(RunEvent::Started {
            runtime_session_id: Some(format!("s-{}", req.run_id)),
            remote_thread_id: None,
        });
        self.channels.lock().unwrap().insert(req.run_id, tx);
        self.receivers.lock().unwrap().insert(req.run_id, rx);
        Ok(RunHandle {
            run_id: req.run_id,
            adapter: AdapterKind::ClaudeLocal,
            runtime_session_id: Some(format!("s-{}", req.run_id)),
            remote_thread_id: None,
        })
    }
    fn events(
        &self,
        _ctx: &AdapterContext,
        run: &RunHandle,
    ) -> Result<BoxStream<'static, RunEvent>, AdapterError> {
        let rx = self
            .receivers
            .lock()
            .unwrap()
            .remove(&run.run_id)
            .ok_or(AdapterError::UnknownRun)?;
        Ok(futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|event| (event, rx))
        })
        .boxed())
    }
    async fn resume(
        &self,
        _ctx: &AdapterContext,
        run: &RunHandle,
        decision: ResumeDecision,
    ) -> Result<(), AdapterError> {
        self.resumes.lock().unwrap().push((run.run_id, decision));
        Ok(())
    }
    async fn read(
        &self,
        _ctx: &AdapterContext,
        _run: &RunHandle,
    ) -> Result<RunSnapshot, AdapterError> {
        Err(AdapterError::UnknownRun)
    }
    async fn cancel(&self, _ctx: &AdapterContext, _run: &RunHandle) -> Result<(), AdapterError> {
        Ok(())
    }
}

pub struct Stack {
    pub dir: TempDir,
    pub store: Datastore,
    pub journal: Arc<Journal>,
    pub approvals: Arc<ApprovalService>,
    pub engine: Arc<RulesEngine>,
    pub supervisor: Arc<SessionSupervisor>,
    pub adapter: Arc<HoldAdapter>,
    pub relay: Arc<HookRelay>,
}

/// One session of the stack: a run the supervisor holds live, a rule set loaded in the engine, the
/// relay's session open, and the environment the hook is given.
pub struct Session {
    pub run_id: RunId,
    pub agent: AgentInstanceId,
    pub workspace: WorkspaceId,
    pub env: HookSessionEnv,
}

impl Stack {
    /// Everything real, over one SQLite file, with the relay started on a real socket.
    pub async fn start() -> Stack {
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
            .register(UsagePlanner::migrations())
            .expect("usage planner");
        registry
            .register(SessionSupervisor::migrations())
            .expect("supervisor");
        registry
            .register(HookRelay::migrations())
            .expect("hook relay");
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
        let engine = RulesEngine::new(
            store.clone(),
            journal.clone(),
            profile.clone(),
            approvals.clone(),
            Arc::new(RulesHostStub),
            RulesConfig {
                ordin_servers: ["ordin", "work", "code", "mail"]
                    .iter()
                    .map(|s| (*s).to_owned())
                    .collect(),
                ordin_server_prefixes: vec!["craft_".to_owned()],
                protected_roots: vec![PathBuf::from("/System"), PathBuf::from("/etc")],
                app_data_root: PathBuf::from(APP_ROOT),
                program: ProgramKind::Desktop,
                instance_name: "local".to_owned(),
            },
        );
        let usage = UsagePlanner::new(store.clone(), journal.clone(), profile, vec![])
            .await
            .expect("usage planner");
        let supervisor = SessionSupervisor::new(
            store.clone(),
            journal.clone(),
            usage,
            None,
            Arc::new(SupervisorHostStub),
            SupervisorConfig {
                program: ordin_session_supervisor::ProgramKind::Desktop,
                local_adapter: AdapterKind::ClaudeLocal,
                local_session_limit: 8,
                instance_name: "desktop".into(),
                default_wall_clock_secs: 3_600,
                idle_timeout_secs: 600,
                stop_deadline: Duration::from_secs(1),
            },
        );
        let adapter = HoldAdapter::new();
        supervisor
            .register_adapter(adapter.clone())
            .expect("register the adapter");
        let relay = HookRelay::new(
            store.clone(),
            journal.clone(),
            engine.clone(),
            approvals.clone(),
            supervisor.clone(),
            Arc::new(ordin_hook_relay::NoPlaces),
            HookRelayConfig {
                app_data_root: dir.0.clone(),
                installation_id: "integration".into(),
            },
        );
        journal.subscribe(relay.event_sink());
        relay.start().await.expect("the relay starts");
        Stack {
            dir,
            store,
            journal,
            approvals,
            engine,
            supervisor,
            adapter,
            relay,
        }
    }

    pub fn pool(&self) -> &sqlx::SqlitePool {
        self.store.sqlite().expect("SQLite")
    }

    pub fn folder(&self, name: &str) -> PathBuf {
        Path::new(APP_ROOT).join("agents").join(name)
    }

    /// Starts a run the supervisor holds live, loads `set` for the agent, begins the run in the
    /// engine and opens the relay's session for it.
    pub async fn session(&self, mut set: InstanceRuleSet) -> Session {
        let agent = AgentInstanceId::new();
        set.agent_instance_id = agent;
        let workspace = WorkspaceId::new();
        self.engine.load_rule_set(set).expect("the set loads");
        let run_id = self
            .supervisor
            .start_run(StartRequest {
                origin: RunOrigin::Task {
                    task_id: TaskId::new(),
                },
                workspace_id: workspace,
                target: RunTarget::AgentInstance {
                    agent_instance_id: agent,
                },
                input: StartInput::FreeText {
                    text: "Do the work.".into(),
                },
                attachments: vec![],
                limits: None,
                work_kind: WorkKind::Documents,
                work_key: "test".into(),
                inputs_used: vec![],
                thread: None,
                started_by_person: true,
                model_override: None,
                coding_task_id: None,
                only_servers: None,
            })
            .await
            .expect("start the run");
        self.engine
            .begin_run(RunStartFacts {
                run_id,
                agent_instance_id: agent,
                workspace_id: workspace,
                crew_charter_id: None,
                command: None,
                text: String::new(),
                scope: None,
                asker: None,
                started_by: "person".to_owned(),
                started_by_id: "person:asha".to_owned(),
            })
            .await
            .expect("the run begins");
        let env = self
            .relay
            .open_session(HookSessionSpec {
                run_id,
                agent_instance_id: agent,
                workspace_id: workspace,
                harness: HarnessKind::ClaudeCode,
            })
            .expect("open the session");
        assert!(
            self.supervisor.is_live(run_id),
            "the supervisor holds the run live"
        );
        Session {
            run_id,
            agent,
            workspace,
            env,
        }
    }

    /// The person presses Approve on the card of `approval`.
    pub async fn approve(&self, approval: ApprovalId) {
        let record = self.approvals.get(approval).await.expect("the approval");
        self.approvals
            .decide(
                ApprovalPress {
                    card_ref: record.card.buttons.approve.clone(),
                    decision: PressDecision::Approve,
                },
                PressOrigin::LocalScreen,
            )
            .await
            .expect("approve");
        assert_eq!(
            self.approvals.get(approval).await.unwrap().state,
            ApprovalState::Approved
        );
    }

    pub async fn pending_approvals(&self) -> Vec<ApprovalId> {
        sqlx::query_scalar::<_, String>("SELECT id FROM approvals WHERE state = 'pending'")
            .fetch_all(self.pool())
            .await
            .expect("pending")
            .iter()
            .filter_map(|id| ApprovalId::parse(id).ok())
            .collect()
    }

    pub async fn shutdown(&self) {
        self.relay.shutdown().await;
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
