//! Cross-component test: a Copilot session started by the real adapter, through the real Session
//! Supervisor, sends its tool requests and its turn end to the REAL Hook Relay in process, and the
//! verdicts follow the real Rules Engine over a real SQLite file (CopilotLocalAdapter.md 09 ADR-045;
//! plan 08 Phase 6 *Integrating services*). The Copilot CLI is a port written by the test, which
//! hands the session's gates back to it; the real CLI runs in the Copilot compatibility proof.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use futures::stream::{BoxStream, StreamExt};
use ordin_approval::{ApprovalConfig, ApprovalService};
use ordin_copilot_adapter::{
    CliServerSpec, CopilotAdapterConfig, CopilotAuthStatus, CopilotCli, CopilotCliSource,
    CopilotEvent, CopilotLocalAdapter, CopilotPort, CopilotServer, CopilotSessionSetup, GateAnswer,
    HostToolAnswer, HostToolHandler, PortError, SessionOptions, SessionSetupError,
    SessionSetupSource, ToolGate,
};
use ordin_hook_relay::{HookRelay, HookRelayConfig, NoPlaces};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry, SystemClock,
    MIGRATIONS,
};
use ordin_mcp_front::{McpFront, McpFrontConfig, ProgramProfile};
use ordin_organisation_profile::{OrganisationProfileService, ProfileMode, ProfileOptions};
use ordin_rules::{
    CardWording, ContentChecker, ControlLine, FactError, InstanceRuleSet, MessageRule, ProgramKind,
    Proposal, RulesConfig, RulesEngine, RulesHost, RunKey,
};
use ordin_runtime_adapter::{AdapterKind, AdapterVerb, RunState, RunTarget, StartInput, StartRun};
use ordin_service_api::blocks::{EntityRef, PlanState, RefResolver, StandardBlockCheck};
use ordin_session_supervisor::{
    ContentCheck, ContentPoint, HostError, RunControls, SessionSupervisor, StartRequest,
    SupervisorConfig, SupervisorHost,
};
use ordin_types::domain::agent_profile::{CallPlace, ModelFailureCause};
use ordin_types::domain::agents::Authority;
use ordin_types::domain::runs::{Run, RunOrigin};
use ordin_types::wire::approval::AffectedItem;
use ordin_types::wire::rules::ActionProposal;
use ordin_types::wire::startup::StartUpReportItem;
use ordin_types::{
    AgentInstanceId, BackendId, EmailLinkId, RemoteAgentId, RunId, TaskId, WorkspaceId,
};
use ordin_usage_planner::{UsagePlanner, WorkKind};
use sqlx::Row;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

struct WallClock;

impl ordin_organisation_profile::Clock for WallClock {
    fn now(&self) -> ordin_organisation_profile::Timestamp {
        ordin_organisation_profile::Timestamp::now()
    }
}

/// The Rules Engine's host: no work-folder files, no checker, fixed card wording.
struct RulesHostStub;

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
struct SupervisorHostStub;

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

/// The MCP Front's references: everything resolves.
struct AnyRef {
    verbs: Vec<String>,
}

#[async_trait::async_trait]
impl RefResolver for AnyRef {
    async fn content(&self, _w: WorkspaceId, _item: &ordin_types::wire::ContentRef) -> bool {
        true
    }
    async fn email_message(&self, _w: WorkspaceId, _l: EmailLinkId, _m: &str) -> bool {
        true
    }
    async fn entity(&self, _w: WorkspaceId, _e: EntityRef) -> bool {
        true
    }
    async fn run_agent_name(&self, _run: RunId) -> Option<String> {
        Some("Analyst".to_owned())
    }
    async fn plan_state(&self, _run: RunId) -> PlanState {
        PlanState {
            versions: 0,
            current_len: 0,
        }
    }
    fn approval_verbs(&self) -> &[String] {
        &self.verbs
    }
}

// ---- the Copilot CLI, written by the test ----

/// What the test sees of the session: its gates, and a way to send it events.
#[derive(Default)]
struct Session {
    pre_tool_use: Mutex<Option<Arc<dyn ToolGate>>>,
    /// The declared tools and their handler (CPA-HT-1).
    host_tools: Mutex<Vec<String>>,
    host_tool_handler: Mutex<Option<Arc<dyn HostToolHandler>>>,
    events: Mutex<Option<mpsc::UnboundedSender<CopilotEvent>>>,
    receiver: Mutex<Option<mpsc::UnboundedReceiver<CopilotEvent>>>,
    sent: Mutex<Vec<String>>,
}

#[derive(Clone, Default)]
struct TestPort(Arc<Session>);

#[async_trait::async_trait]
impl CopilotPort for TestPort {
    async fn start_server(
        &self,
        _spec: CliServerSpec,
    ) -> Result<Box<dyn CopilotServer>, PortError> {
        Ok(Box::new(TestServer(self.0.clone())))
    }
}

struct TestServer(Arc<Session>);

#[async_trait::async_trait]
impl CopilotServer for TestServer {
    async fn auth_status(&self) -> Result<CopilotAuthStatus, PortError> {
        Ok(CopilotAuthStatus {
            signed_in: true,
            method: Some("device".into()),
        })
    }
    async fn start_login(&self) -> Result<(), PortError> {
        Ok(())
    }
    async fn create_session(&self, opts: SessionOptions) -> Result<String, PortError> {
        *self.0.pre_tool_use.lock().unwrap() = Some(opts.pre_tool_use.clone());
        *self.0.host_tools.lock().unwrap() =
            opts.host_tools.iter().map(|t| t.name.clone()).collect();
        *self.0.host_tool_handler.lock().unwrap() = Some(opts.host_tool_handler.clone());
        let (tx, rx) = mpsc::unbounded_channel();
        *self.0.events.lock().unwrap() = Some(tx);
        *self.0.receiver.lock().unwrap() = Some(rx);
        Ok("sess_1".into())
    }
    async fn resume_session(&self, _id: &str, _opts: SessionOptions) -> Result<(), PortError> {
        Err(PortError::new("not used"))
    }
    async fn send(&self, _id: &str, text: &str) -> Result<(), PortError> {
        self.0.sent.lock().unwrap().push(text.to_owned());
        Ok(())
    }
    fn events(&self, _id: &str) -> BoxStream<'static, CopilotEvent> {
        match self.0.receiver.lock().unwrap().take() {
            Some(rx) => {
                futures::stream::unfold(
                    rx,
                    |mut rx| async move { rx.recv().await.map(|e| (e, rx)) },
                )
                .boxed()
            }
            None => futures::stream::pending().boxed(),
        }
    }
    async fn abort(&self, _id: &str) -> Result<(), PortError> {
        Ok(())
    }
    async fn stop(&self, _deadline: Duration) {}
}

struct Cli;

#[async_trait::async_trait]
impl CopilotCliSource for Cli {
    async fn current(&self) -> Option<CopilotCli> {
        Some(CopilotCli {
            path: PathBuf::from("/usr/local/bin/copilot"),
            version: "1.0.98".into(),
        })
    }
}

struct FixedSetup(PathBuf);

#[async_trait::async_trait]
impl SessionSetupSource for FixedSetup {
    async fn session_setup(
        &self,
        _run_id: RunId,
        _agent_instance_id: AgentInstanceId,
        _attachments: &[ordin_types::ArtefactId],
    ) -> Result<CopilotSessionSetup, SessionSetupError> {
        Ok(CopilotSessionSetup {
            agent_folder: self.0.clone(),
            extra_dirs: vec![],
            model: None,
            attachment_paths: vec![],
            package_servers: vec![],
            resume_session_id: None,
            crew_charter_id: None,
        })
    }
}

/// A short folder removed when dropped (a Unix socket path is limited to 104 bytes on macOS).
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> TempDir {
        let dir = std::env::temp_dir()
            .canonicalize()
            .map(ordin_types::paths::plain_form)
            .expect("temporary root")
            .join(format!(
                "cpa-{}",
                &RunId::new().0.simple().to_string()[20..]
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

struct Stack {
    store: Datastore,
    engine: Arc<RulesEngine>,
    supervisor: Arc<SessionSupervisor>,
    relay: Arc<HookRelay>,
    adapter: Arc<CopilotLocalAdapter>,
    agent_folder: PathBuf,
    stop: CancellationToken,
}

impl Stack {
    async fn start(dir: &Path, port: TestPort) -> Stack {
        let agent_folder = dir.join("agent");
        std::fs::create_dir_all(&agent_folder).expect("agent folder");
        let store = Datastore::open(DatastoreConfig::Sqlite {
            path: dir.join("data").join("journal.db"),
            backup_dir: dir.join("data").join("backups"),
        })
        .await
        .expect("open the SQLite datastore");
        let mut registry = MigrationRegistry::new();
        registry.register(MIGRATIONS).expect("journal");
        registry
            .register(ordin_journal::ModuleMigrations {
                module: ordin_identity::MODULE_NAME,
                sqlite: ordin_identity::SQLITE_MIGRATIONS,
                postgres: &[],
            })
            .expect("identity");
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
                app_data_root: dir.to_path_buf(),
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
                local_adapter: AdapterKind::CopilotLocal,
                local_session_limit: 8,
                instance_name: "desktop".into(),
                default_wall_clock_secs: 3_600,
                idle_timeout_secs: 600,
                stop_deadline: Duration::from_secs(2),
            },
        );
        let relay = HookRelay::new(
            store.clone(),
            journal.clone(),
            engine.clone(),
            approvals.clone(),
            supervisor.clone(),
            Arc::new(NoPlaces),
            HookRelayConfig {
                app_data_root: dir.to_path_buf(),
                installation_id: "integration".into(),
            },
        );
        journal.subscribe(relay.event_sink());
        relay.start().await.expect("the relay starts");
        let front = Arc::new(McpFront::new(
            McpFrontConfig {
                app_data_root: dir.to_path_buf(),
                installation_id: RunId::new().0,
                hook_program: PathBuf::from("/opt/ordin/bin/ordin-hook"),
                profile: ProgramProfile::Desktop,
                ..McpFrontConfig::default()
            },
            engine.clone(),
            (*journal).clone(),
            Arc::new(StandardBlockCheck),
            None,
        ));
        front
            .bind_ref_resolver(Arc::new(AnyRef { verbs: vec![] }))
            .expect("bind the resolver");
        let stop = CancellationToken::new();
        front.start(stop.clone()).await.expect("the front starts");
        let key_file = dir.join("secrets.key");
        std::fs::write(
            &key_file,
            base64::engine::general_purpose::STANDARD.encode([7u8; 32]),
        )
        .expect("key file");
        let identity = ordin_identity::IdentityService::new(
            ordin_identity::IdentityConfig {
                program: ordin_identity::Program::Desktop,
                data_root: dir.to_path_buf(),
                keychain_service: "io.gatblau.ordin.copilot-test".to_owned(),
                secret_backend: ordin_identity::SecretBackendConfig::EncryptedFile {
                    key_source: ordin_identity::KeySource::KeyFile(key_file),
                },
                github_app_client_id: None,
                microsoft_client_id: None,
                http: reqwest::Client::new(),
                browser: None,
            },
            store.sqlite().cloned(),
        )
        .await
        .expect("identity over a file");
        let adapter = Arc::new(
            CopilotLocalAdapter::new(
                CopilotAdapterConfig {
                    tested_versions: vec!["1.0.98".into()],
                    app_data_root: dir.to_path_buf(),
                    profile: ProgramProfile::Desktop,
                    stop_deadline: Duration::from_secs(2),
                },
                Arc::new(port),
                Arc::new(Cli),
                front,
                relay.clone(),
                engine.clone(),
                Arc::new(identity),
                Arc::new(FixedSetup(agent_folder.clone())),
            )
            .expect("the adapter builds"),
        );
        supervisor
            .register_adapter(adapter.clone())
            .expect("register the adapter");
        Stack {
            store,
            engine,
            supervisor,
            relay,
            adapter,
            agent_folder,
            stop,
        }
    }

    async fn start_run(&self, authority: Authority) -> RunId {
        let agent = AgentInstanceId::new();
        self.engine
            .load_rule_set(InstanceRuleSet {
                agent_instance_id: agent,
                package_name: "analyst".to_owned(),
                authority,
                agent_folder: self.agent_folder.clone(),
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
            })
            .expect("the set loads");
        self.supervisor
            .start_run(StartRequest {
                origin: RunOrigin::Task {
                    task_id: TaskId::new(),
                },
                workspace_id: WorkspaceId::new(),
                target: RunTarget::AgentInstance {
                    agent_instance_id: agent,
                },
                input: StartInput::FreeText {
                    text: "Summarise the three quotes in one table".into(),
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
            .expect("start the run")
    }

    async fn wait_state(&self, run: RunId) -> RunState {
        for _ in 0..1000 {
            let view = self.supervisor.get(run).await.expect("the run");
            if !matches!(
                view.run.state,
                RunState::Queued | RunState::Running | RunState::Paused
            ) {
                return view.run.state;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the run never ended");
    }

    async fn requests(&self, run: RunId) -> Vec<(String, String)> {
        sqlx::query(
            "SELECT tool, decision FROM tool_requests WHERE run_id = ?1 ORDER BY requested_at",
        )
        .bind(run.to_string())
        .fetch_all(self.store.sqlite().expect("SQLite"))
        .await
        .expect("tool requests")
        .iter()
        .map(|row| {
            (
                row.get::<String, _>("tool"),
                row.get::<Option<String>, _>("decision").unwrap_or_default(),
            )
        })
        .collect()
    }

    async fn stop(&self) {
        self.adapter.shutdown().await;
        self.relay.shutdown().await;
        self.stop.cancel();
    }
}

async fn gate_of(session: &Session) -> Arc<dyn ToolGate> {
    for _ in 0..1000 {
        if let Some(gate) = session.pre_tool_use.lock().unwrap().clone() {
            return gate;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the session never started");
}

#[tokio::test]
async fn a_denied_tool_is_refused_by_the_real_rules_engine_and_the_turn_end_completes_the_run() {
    let dir = TempDir::new();
    let port = TestPort::default();
    let stack = Stack::start(&dir.0, port.clone()).await;
    let run = stack.start_run(Authority::DocumentOnly).await;
    let gate = gate_of(&port.0).await;

    let answer = gate
        .decide(
            "bash",
            serde_json::json!({"command": "rm -rf build"}),
            Some("call_7".into()),
        )
        .await;
    assert!(matches!(answer, GateAnswer::Deny { .. }), "{answer:?}");
    let rows = stack.requests(run).await;
    assert_eq!(
        rows,
        [("bash".to_owned(), "deny".to_owned())],
        "the Rules Engine journalled the call"
    );

    let events = port
        .0
        .events
        .lock()
        .unwrap()
        .clone()
        .expect("the session's events");
    events
        .send(CopilotEvent::TurnEnded {
            final_text: Some("Here is the table.".into()),
        })
        .unwrap();
    assert_eq!(stack.wait_state(run).await, RunState::Completed);
    assert_eq!(
        port.0.sent.lock().unwrap()[0],
        "Summarise the three quotes in one table"
    );
    stack.stop().await;
}

/// CPA-HT-3 through the real services: a declared `work` tool's call goes through the real MCP Front
/// and is decided by the real Rules Engine, journalled under its `mcp__` name (plan 08 Phase 13
/// behavioural check).
// spec: ordin-desktop/components/CopilotLocalAdapter#CPA-HT-3
#[tokio::test]
async fn a_declared_tool_call_is_decided_by_the_real_rules_engine() {
    let dir = TempDir::new();
    let port = TestPort::default();
    let stack = Stack::start(&dir.0, port.clone()).await;
    let run = stack.start_run(Authority::DocumentOnly).await;
    gate_of(&port.0).await;
    let declared = port.0.host_tools.lock().unwrap().clone();
    assert!(
        declared.iter().any(|name| name == "mcp__work__plan"),
        "the MCP Front's work tools are declared: {declared:?}"
    );
    let handler = port
        .0
        .host_tool_handler
        .lock()
        .unwrap()
        .clone()
        .expect("the handler");
    let answer = handler
        .call(
            "mcp__work__plan",
            serde_json::json!({"action": "state", "steps": ["Read the quotes", "Compare the rates", "Write the summary"]}),
            CancellationToken::new(),
        )
        .await;
    let rows = stack.requests(run).await;
    assert_eq!(
        rows.len(),
        1,
        "one journalled request: {rows:?}; answer {answer:?}"
    );
    assert_eq!(rows[0].0, "mcp__work__plan");
    assert!(!rows[0].1.is_empty(), "the Rules Engine decided: {rows:?}");
    match (&answer, rows[0].1.as_str()) {
        (HostToolAnswer::Success { .. }, "allow") | (HostToolAnswer::Failure { .. }, _) => {}
        other => panic!("answer and decision disagree: {other:?}"),
    }
    stack.stop().await;
}
