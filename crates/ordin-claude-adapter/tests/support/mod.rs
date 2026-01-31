//! Shared helpers of this crate's integration tests. The Rules Engine, Hook Relay, MCP Front, Session
//! Supervisor, Approval Service and Journal are the real crates over a real SQLite file, and the
//! adapter is the real public one, started by the real supervisor. The stand-ins are the programs'
//! hosts and the harness itself (a program written by the test, or the real CLI with a test wrapper
//! around it). Nothing here skips.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use ordin_approval::{ApprovalConfig, ApprovalService};
use ordin_claude_adapter::{
    ClaudeAdapterConfig, ClaudeLocalAdapter, RecordError, SessionRecordStore, SessionSetup,
    SessionSetupError, SessionSetupSource, SignInMode,
};
use ordin_hook_relay::{HookRelay, HookRelayConfig};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry, SystemClock,
    MIGRATIONS,
};
use ordin_mcp_front::{McpFront, McpFrontConfig, ProgramProfile};
use ordin_organisation_profile::{OrganisationProfileService, ProfileMode, ProfileOptions};
use ordin_rules::{
    CardWording, ContentChecker, ControlLine, Declaration, FactError, InstanceRuleSet, MessageRule,
    ProgramKind, Proposal, RulesConfig, RulesEngine, RulesHost, RunKey,
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
use tokio_util::sync::CancellationToken;

/// A short folder removed when dropped (a Unix socket path is limited to 104 bytes on macOS).
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir()
            .canonicalize()
            .map(ordin_types::paths::plain_form)
            .expect("temporary root")
            .join(format!("cli-{}", &uuid_text()[20..]));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }
}

fn uuid_text() -> String {
    RunId::new().0.simple().to_string()
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `target/<profile>/ordin-hook`, built on first use.
pub fn hook_binary() -> &'static PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let exe = std::env::current_exe().expect("this test's path");
        let profile_dir = exe
            .parent()
            .and_then(|deps| deps.parent())
            .expect("target/<profile>")
            .to_path_buf();
        let target_dir = profile_dir
            .parent()
            .expect("the target folder")
            .to_path_buf();
        let status =
            std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
                .args(["build", "-p", "ordin-hook", "--target-dir"])
                .arg(&target_dir)
                .env("CARGO_INCREMENTAL", "0")
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .status()
                .expect("run cargo");
        assert!(status.success(), "cargo build -p ordin-hook failed");
        let binary = profile_dir.join("ordin-hook");
        assert!(binary.exists(), "{} was not built", binary.display());
        binary
    })
}

/// Writes an executable script.
pub fn write_script(path: &Path, body: &str) {
    std::fs::write(path, body).expect("the script");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("its mode");
}

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

/// The program's setup for every session of a test: one agent folder.
struct FixedSetup(SessionSetup);

#[async_trait::async_trait]
impl SessionSetupSource for FixedSetup {
    async fn session_setup(
        &self,
        _run_id: RunId,
        _agent_instance_id: AgentInstanceId,
        _attachments: &[ordin_types::ArtefactId],
    ) -> Result<SessionSetup, SessionSetupError> {
        Ok(self.0.clone())
    }
}

/// A setup source for an adapter that never starts.
pub struct NoSetup;

#[async_trait::async_trait]
impl SessionSetupSource for NoSetup {
    async fn session_setup(
        &self,
        _run_id: RunId,
        _agent_instance_id: AgentInstanceId,
        _attachments: &[ordin_types::ArtefactId],
    ) -> Result<SessionSetup, SessionSetupError> {
        Err(SessionSetupError {
            code: "UNUSED".to_owned(),
        })
    }
}

/// The agent profile's record store; these runs never pause.
pub struct NoRecords;

#[async_trait::async_trait]
impl SessionRecordStore for NoRecords {
    async fn save(&self, _r: RunId, _s: &str, _b: Vec<u8>) -> Result<(), RecordError> {
        Ok(())
    }
    async fn load(&self, _r: RunId, _s: &str) -> Result<Option<Vec<u8>>, RecordError> {
        Ok(None)
    }
}

/// How the adapter under test is set up.
pub struct Options {
    pub claude: PathBuf,
    pub hook: PathBuf,
    pub profile: ProgramProfile,
    pub sign_in: SignInMode,
    pub tested_versions: Vec<String>,
    pub agent_folder: PathBuf,
}

pub struct Stack {
    pub store: Datastore,
    pub journal: Arc<Journal>,
    pub engine: Arc<RulesEngine>,
    pub supervisor: Arc<SessionSupervisor>,
    pub relay: Arc<HookRelay>,
    pub front: Arc<McpFront>,
    pub adapter: Arc<ClaudeLocalAdapter>,
    pub agent_folder: PathBuf,
    stop: CancellationToken,
}

/// The session setup of `folder`: installer files that do not exist, nothing else.
pub fn setup_of(folder: &Path) -> SessionSetup {
    SessionSetup {
        agent_folder: folder.to_path_buf(),
        append_system_prompt_file: folder.join("instructions.md"),
        plugin_dir: folder.join("plugin"),
        plugin_name: "analyst".to_owned(),
        model: None,
        effort: None,
        fallback_model: None,
        attachment_paths: vec![],
        extra_dirs: vec![],
        shell_allowed: false,
        package_servers: vec![],
        resume_session_id: None,
        crew_charter_id: None,
        run_starter: None,
        paused: None,
    }
}

impl Stack {
    /// Everything real, over one SQLite file under `dir`, with the relay and the MCP Front started
    /// on real sockets.
    pub async fn start(dir: &Path, options: Options) -> Stack {
        Stack::start_edited(dir, options, |_| {}).await
    }

    /// `start`, with the session setup changed by `edit`.
    pub async fn start_edited(
        dir: &Path,
        options: Options,
        edit: impl FnOnce(&mut SessionSetup),
    ) -> Stack {
        let mut setup = setup_of(&options.agent_folder);
        edit(&mut setup);
        Stack::start_with(dir, options, setup, Arc::new(ordin_hook_relay::NoPlaces)).await
    }

    /// `start`, with the relay telling `writes` what each call may write.
    pub async fn start_writing(
        dir: &Path,
        options: Options,
        writes: Arc<dyn ordin_hook_relay::AgentWrites>,
    ) -> Stack {
        let setup = setup_of(&options.agent_folder);
        Stack::start_with(dir, options, setup, writes).await
    }

    async fn start_with(
        dir: &Path,
        options: Options,
        setup: SessionSetup,
        writes: Arc<dyn ordin_hook_relay::AgentWrites>,
    ) -> Stack {
        let store = Datastore::open(DatastoreConfig::Sqlite {
            path: dir.join("data").join("journal.db"),
            backup_dir: dir.join("data").join("backups"),
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
            .register(ordin_files::FileService::migrations())
            .expect("files");
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
                // The supervisor runs in the same program as the adapter: in the agent profile no
                // run has the person's own input (SA-SUP-INPUT-01).
                program: if options.profile == ProgramProfile::AgentProfile {
                    ordin_session_supervisor::ProgramKind::AgentSingle
                } else {
                    ordin_session_supervisor::ProgramKind::Desktop
                },
                local_adapter: AdapterKind::ClaudeLocal,
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
            writes,
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
                hook_program: options.hook.clone(),
                profile: options.profile,
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
        let records: Option<Arc<dyn SessionRecordStore>> =
            (options.profile == ProgramProfile::AgentProfile).then(|| Arc::new(NoRecords) as _);
        let adapter = Arc::new(
            ClaudeLocalAdapter::new(
                ClaudeAdapterConfig {
                    claude_path: options.claude,
                    hook_program: options.hook,
                    app_data_root: dir.to_path_buf(),
                    profile: options.profile,
                    tested_versions: options.tested_versions,
                    sign_in: options.sign_in,
                    stop_deadline: Duration::from_secs(2),
                },
                front.clone(),
                relay.clone(),
                engine.clone(),
                Arc::new(FixedSetup(setup)),
                records,
            )
            .expect("the adapter builds"),
        );
        supervisor
            .register_adapter(adapter.clone())
            .expect("register the adapter");
        Stack {
            store,
            journal,
            engine,
            supervisor,
            relay,
            front,
            adapter,
            agent_folder: options.agent_folder,
            stop,
        }
    }

    /// Loads a rule set for a new agent over `self.agent_folder` and has the supervisor start a
    /// free-text run on it.
    pub async fn start_run(&self, authority: Authority) -> (RunId, AgentInstanceId) {
        self.start_declared_run(authority, None).await
    }

    /// `start_run` for a package that declares `declaration`.
    pub async fn start_declared_run(
        &self,
        authority: Authority,
        declaration: Option<Declaration>,
    ) -> (RunId, AgentInstanceId) {
        let input = StartInput::FreeText {
            text: "Do the work.".into(),
        };
        self.start_input_run(authority, declaration, input).await
    }

    /// `start_declared_run` with `input` as the first message.
    pub async fn start_input_run(
        &self,
        authority: Authority,
        declaration: Option<Declaration>,
        input: StartInput,
    ) -> (RunId, AgentInstanceId) {
        let agent = AgentInstanceId::new();
        self.engine
            .load_rule_set(InstanceRuleSet {
                agent_instance_id: agent,
                package_name: "analyst".to_owned(),
                authority,
                agent_folder: self.agent_folder.clone(),
                agent_folder_read_only: false,
                declaration,
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
        let run_id = self
            .supervisor
            .start_run(StartRequest {
                origin: RunOrigin::Task {
                    task_id: TaskId::new(),
                },
                workspace_id: WorkspaceId::new(),
                target: RunTarget::AgentInstance {
                    agent_instance_id: agent,
                },
                input,
                attachments: vec![],
                limits: None,
                work_kind: WorkKind::Documents,
                work_key: "test".into(),
                inputs_used: vec![],
                thread: None,
                started_by_person: true,
                model_override: None,
                coding_task_id: None,
            })
            .await
            .expect("start the run");
        (run_id, agent)
    }

    /// Waits for the run to end and gives its final state and end reason.
    pub async fn wait_end(&self, run: RunId) -> (RunState, Option<String>) {
        for _ in 0..12_000 {
            let view = self.supervisor.get(run).await.expect("the run");
            if !matches!(
                view.run.state,
                RunState::Queued | RunState::Running | RunState::Paused
            ) {
                return (view.run.state, view.run.end_reason);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the run never ended");
    }

    /// The tool requests journalled for a run: (tool, decision, state).
    pub async fn requests(&self, run: RunId) -> Vec<(String, String, String)> {
        sqlx::query(
            "SELECT tool, decision, state FROM tool_requests WHERE run_id = ?1 ORDER BY requested_at",
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
                row.get::<String, _>("state"),
            )
        })
        .collect()
    }

    pub async fn stop(&self) {
        self.adapter.shutdown().await;
        self.relay.shutdown().await;
        self.stop.cancel();
    }
}
