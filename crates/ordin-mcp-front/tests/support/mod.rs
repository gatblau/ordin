//! Shared helpers of this crate's integration tests. They use the public surface only: the hub is
//! built with `McpFront::new` over the real Rules Engine, the real Approval Service and a real
//! SQLite Journal on a temporary file. Nothing here skips and nothing touches the network.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use ordin_approval::{ApprovalConfig, ApprovalService};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, Journal, JournalOptions, MigrationRegistry, SystemClock,
    MIGRATIONS,
};
use ordin_mcp_front::{
    ArgumentProblem, McpFront, McpFrontConfig, ProgramProfile, ServerName, ServerSpec,
    SessionGrant, SessionMcpConfig, ToolCallContext, ToolCaller, ToolFailure, ToolKey, ToolOutput,
    ToolProvider, ToolSpec,
};
use ordin_organisation_profile::{
    Clock, OrganisationProfileService, ProfileMode, ProfileOptions, Timestamp,
};
use ordin_rules::{
    CardWording, ControlLine, FactError, InstanceRuleSet, MessageRule, ProgramKind, Proposal,
    RulesConfig, RulesEngine, RulesHost, RunKey, RunStartFacts,
};
use ordin_service_api::blocks::{
    Block, BlockCatalogueCheck, BlockCheckContext, BlockRejection, CheckedReport, EntityRef,
    PlanState, PlanToolArgs, RefResolver, StandardBlockCheck,
};
use ordin_types::domain::agents::Authority;
use ordin_types::domain::identity::HarnessKind;
use ordin_types::wire::approval::AffectedItem;
use ordin_types::wire::rules::ActionProposal;
use ordin_types::{AgentInstanceId, BackendId, RunId, WorkspaceId};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
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

/// Installs the process-wide capture of log lines (once per test binary), written by the
/// programs' own formatter so a test reads the line a log file would hold (Logging.md LOG-ID-1).
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
        // Short names: a Unix socket path is limited to about 100 bytes.
        let dir = std::env::temp_dir().join(format!("omf-{:08x}", rand_u32()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }
}

fn rand_u32() -> u32 {
    let id = uuid::Uuid::new_v4();
    let bytes = id.as_bytes();
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
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

// ---- rules host ----

#[derive(Default)]
pub struct Host;

#[async_trait]
impl RulesHost for Host {
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
    async fn setting_value(&self, _scope: Option<&str>, _name: &str) -> Result<Value, FactError> {
        Ok(Value::Null)
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
    fn content_checker(&self) -> Option<Arc<dyn ordin_rules::ContentChecker>> {
        None
    }
}

// ---- block-check doubles ----

/// The Service API's `RefResolver`, implemented as the program would: plan state is read from
/// the real Journal unless a test overrides it.
pub struct TestResolver {
    pub journal: Arc<Journal>,
    pub plan_override: Mutex<Option<PlanState>>,
    pub verbs: Vec<String>,
}

#[async_trait]
impl RefResolver for TestResolver {
    async fn content(&self, _w: WorkspaceId, _item: &ordin_types::wire::ContentRef) -> bool {
        true
    }
    async fn email_message(&self, _w: WorkspaceId, _l: ordin_types::EmailLinkId, _m: &str) -> bool {
        true
    }
    async fn entity(&self, _w: WorkspaceId, _e: EntityRef) -> bool {
        true
    }
    async fn run_agent_name(&self, _run: RunId) -> Option<String> {
        Some("Analyst".to_owned())
    }
    async fn plan_state(&self, run_id: RunId) -> PlanState {
        if let Some(state) = *self.plan_override.lock().unwrap() {
            return state;
        }
        let steps = self.journal.current_plan(run_id).await.unwrap_or_default();
        PlanState {
            versions: steps.first().map(|s| s.plan_version).unwrap_or(0),
            current_len: u8::try_from(steps.len()).unwrap_or(u8::MAX),
        }
    }
    fn approval_verbs(&self) -> &[String] {
        &self.verbs
    }
}

/// A block check that lets a test dictate what the report check answers; plans use the standard
/// check.
pub struct ScriptedCheck {
    pub report: Mutex<Option<Result<CheckedReport, Vec<BlockRejection>>>>,
}

#[async_trait]
impl BlockCatalogueCheck for ScriptedCheck {
    async fn check_block(
        &self,
        raw: &Value,
        ctx: &BlockCheckContext,
        refs: &dyn RefResolver,
    ) -> Result<Block, Vec<BlockRejection>> {
        StandardBlockCheck.check_block(raw, ctx, refs).await
    }
    async fn check_report_tool(
        &self,
        raw: &Value,
        ctx: &BlockCheckContext,
        refs: &dyn RefResolver,
    ) -> Result<CheckedReport, Vec<BlockRejection>> {
        let scripted = self.report.lock().unwrap().clone();
        match scripted {
            Some(answer) => answer,
            None => StandardBlockCheck.check_report_tool(raw, ctx, refs).await,
        }
    }
    async fn check_plan_tool(
        &self,
        raw: &Value,
        run_id: RunId,
        refs: &dyn RefResolver,
    ) -> Result<PlanToolArgs, Vec<BlockRejection>> {
        StandardBlockCheck.check_plan_tool(raw, run_id, refs).await
    }
}

// ---- test provider ----

#[derive(Clone)]
pub enum Behaviour {
    Text(String),
    /// Sleeps, then answers; the cancel token of the call is recorded.
    Sleep(Duration, String),
    Fail(ToolFailure),
    Panic,
    /// Reports progress, then answers.
    Progress(u64, Option<u64>),
}

type OfferFn = dyn Fn(&SessionGrant) -> Result<Vec<ToolKey>, ToolFailure> + Send + Sync;

pub struct TestProvider {
    pub id: &'static str,
    pub specs: Mutex<Vec<ServerSpec>>,
    pub prefix: Option<&'static str>,
    pub offer_fn: Mutex<Box<OfferFn>>,
    pub offer_delay: Mutex<Duration>,
    pub behaviour: Mutex<Behaviour>,
    pub calls: Mutex<Vec<(ToolCallContext, ToolKey, Value)>>,
    pub closed: Mutex<Vec<RunId>>,
    pub tokens: Mutex<Vec<CancellationToken>>,
    pub check_fn: Mutex<Option<Box<CheckFn>>>,
}

type CheckFn = dyn Fn(&Value) -> Result<(), ArgumentProblem> + Send + Sync;

/// A tool spec with the given name, a permissive object schema and valid text fields.
pub fn spec(name: &str, sandbox_callable: bool) -> ToolSpec {
    ToolSpec {
        name: name.to_owned(),
        description: format!("Does {name}."),
        input_schema: json!({
            "type": "object",
            "properties": {"query": {"type": "string"}},
            "additionalProperties": false
        }),
        argument_form: format!("{name}(query: string)"),
        sandbox_callable,
        plain: format!("Can {name}."),
    }
}

pub fn server(name: &str, tools: Vec<ToolSpec>) -> ServerSpec {
    ServerSpec {
        name: ServerName::new(name).unwrap(),
        tools,
    }
}

pub fn key(server: &str, tool: &str) -> ToolKey {
    ToolKey {
        server: ServerName::new(server).unwrap(),
        tool: tool.to_owned(),
    }
}

impl TestProvider {
    /// A provider that offers every tool it lists to every session.
    pub fn new(id: &'static str, servers: Vec<ServerSpec>) -> Arc<TestProvider> {
        let keys: Vec<ToolKey> = servers
            .iter()
            .flat_map(|s| {
                s.tools.iter().map(|t| ToolKey {
                    server: s.name.clone(),
                    tool: t.name.clone(),
                })
            })
            .collect();
        Arc::new(TestProvider {
            id,
            specs: Mutex::new(servers),
            prefix: None,
            offer_fn: Mutex::new(Box::new(move |_| Ok(keys.clone()))),
            offer_delay: Mutex::new(Duration::ZERO),
            behaviour: Mutex::new(Behaviour::Text("done".to_owned())),
            calls: Mutex::new(Vec::new()),
            closed: Mutex::new(Vec::new()),
            tokens: Mutex::new(Vec::new()),
            check_fn: Mutex::new(None),
        })
    }

    pub fn with_prefix(id: &'static str, prefix: &'static str) -> Arc<TestProvider> {
        let mut provider = TestProvider::new(id, Vec::new());
        if let Some(p) = Arc::get_mut(&mut provider) {
            p.prefix = Some(prefix);
        }
        provider
    }

    pub fn set_offer(
        &self,
        f: impl Fn(&SessionGrant) -> Result<Vec<ToolKey>, ToolFailure> + Send + Sync + 'static,
    ) {
        *self.offer_fn.lock().unwrap() = Box::new(f);
    }

    pub fn set_check(
        &self,
        f: impl Fn(&Value) -> Result<(), ArgumentProblem> + Send + Sync + 'static,
    ) {
        *self.check_fn.lock().unwrap() = Some(Box::new(f));
    }

    pub fn set_behaviour(&self, b: Behaviour) {
        *self.behaviour.lock().unwrap() = b;
    }

    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

#[async_trait]
impl ToolProvider for TestProvider {
    fn provider_id(&self) -> &'static str {
        self.id
    }
    fn servers(&self) -> Vec<ServerSpec> {
        self.specs.lock().unwrap().clone()
    }
    fn server_prefix(&self) -> Option<&'static str> {
        self.prefix
    }
    async fn offer(&self, grant: &SessionGrant) -> Result<Vec<ToolKey>, ToolFailure> {
        let delay = *self.offer_delay.lock().unwrap();
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        (self.offer_fn.lock().unwrap())(grant)
    }
    fn check_arguments(&self, _key: &ToolKey, arguments: &Value) -> Result<(), ArgumentProblem> {
        match self.check_fn.lock().unwrap().as_ref() {
            Some(f) => f(arguments),
            None => check_object(arguments),
        }
    }
    async fn call(
        &self,
        ctx: ToolCallContext,
        key: &ToolKey,
        arguments: Value,
    ) -> Result<ToolOutput, ToolFailure> {
        self.tokens.lock().unwrap().push(ctx.cancel.clone());
        self.calls
            .lock()
            .unwrap()
            .push((ctx.clone(), key.clone(), arguments));
        let behaviour = self.behaviour.lock().unwrap().clone();
        match behaviour {
            Behaviour::Text(text) => Ok(ToolOutput {
                text,
                structured: None,
            }),
            Behaviour::Sleep(delay, text) => {
                tokio::select! {
                    _ = tokio::time::sleep(delay) => Ok(ToolOutput { text, structured: None }),
                    _ = ctx.cancel.cancelled() => Err(ToolFailure {
                        code: "TEST_CANCELLED".to_owned(),
                        message: "cancelled".to_owned(),
                        retryable: false,
                    }),
                }
            }
            Behaviour::Fail(failure) => Err(failure),
            Behaviour::Panic => panic!("the test provider panics"),
            Behaviour::Progress(done, total) => {
                ctx.progress.report(done, total);
                Ok(ToolOutput {
                    text: "progressed".to_owned(),
                    structured: None,
                })
            }
        }
    }
    async fn session_closed(&self, run_id: RunId) {
        self.closed.lock().unwrap().push(run_id);
    }
}

/// Typed check with unknown fields refused, as a provider would write it; the failing field's
/// JSON Pointer is the `at`.
pub fn typed<T: serde::de::DeserializeOwned>(arguments: &Value) -> Result<(), ArgumentProblem> {
    serde_path_to_error::deserialize::<_, T>(arguments)
        .map(|_| ())
        .map_err(|error| {
            let mut at = String::new();
            for segment in error.path().iter() {
                match segment {
                    serde_path_to_error::Segment::Seq { index } => {
                        at.push_str(&format!("/{index}"))
                    }
                    serde_path_to_error::Segment::Map { key } => at.push_str(&format!("/{key}")),
                    _ => {}
                }
            }
            let message = error.inner().to_string();
            if let Some(field) = message
                .strip_prefix("unknown field `")
                .and_then(|r| r.split('`').next())
            {
                let suffix = format!("/{field}");
                if !at.ends_with(&suffix) {
                    at.push_str(&suffix);
                }
            }
            ArgumentProblem {
                at,
                reason: message,
            }
        })
}

/// The JSON Schema of `T` as a tool spec carries it.
pub fn schema_of<T: schemars::JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(T)).expect("schema")
}

/// The permissive check of the test tools: an object whose only allowed field is `query`, a string.
pub fn check_object(arguments: &Value) -> Result<(), ArgumentProblem> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Args {
        #[allow(dead_code)]
        query: Option<String>,
    }
    serde_path_to_error::deserialize::<_, Args>(arguments)
        .map(|_| ())
        .map_err(|error| {
            let path = error.path().to_string();
            let mut at = if path == "." {
                String::new()
            } else {
                format!("/{}", path.replace('.', "/"))
            };
            let message = error.inner().to_string();
            if let Some(field) = message
                .strip_prefix("unknown field `")
                .and_then(|r| r.split('`').next())
            {
                at = format!("/{field}");
            }
            ArgumentProblem {
                at,
                reason: message,
            }
        })
}

// ---- environment ----

pub struct Env {
    dir: TempDir,
    pub journal: Arc<Journal>,
    pub approvals: Arc<ApprovalService>,
    pub engine: Arc<RulesEngine>,
    pub front: McpFront,
    pub resolver: Arc<TestResolver>,
    pub check: Arc<ScriptedCheck>,
    pub store: Datastore,
}

pub fn app_root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| PathBuf::from("/Users/asha/Library/Application Support/ordin"))
}

fn rules_config() -> RulesConfig {
    RulesConfig {
        ordin_servers: ["ordin", "work", "memory", "sandbox", "mail", "knowledge"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
        ordin_server_prefixes: vec!["craft_".to_owned()],
        protected_roots: vec![PathBuf::from("/System"), PathBuf::from("/etc")],
        app_data_root: app_root().to_path_buf(),
        program: ProgramKind::Desktop,
        instance_name: "local".to_owned(),
    }
}

/// Builds the hub over the real engine; `tweak` adjusts the configuration before it is used.
pub async fn env(tweak: impl FnOnce(&mut McpFrontConfig)) -> Env {
    capture_logs();
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
    let engine = RulesEngine::new(
        store.clone(),
        journal.clone(),
        profile,
        approvals.clone(),
        Arc::new(Host),
        rules_config(),
    );
    let mut config = McpFrontConfig {
        app_data_root: dir.0.clone(),
        installation_id: uuid::Uuid::now_v7(),
        hook_program: PathBuf::from("/Applications/ordin.app/Contents/MacOS/ordin-hook"),
        ..McpFrontConfig::default()
    };
    tweak(&mut config);
    let check = Arc::new(ScriptedCheck {
        report: Mutex::new(None),
    });
    let front = McpFront::new(
        config,
        engine.clone(),
        (*journal).clone(),
        check.clone(),
        None,
    );
    let resolver = Arc::new(TestResolver {
        journal: journal.clone(),
        plan_override: Mutex::new(None),
        verbs: vec!["Approve".to_owned(), "Reject".to_owned()],
    });
    Env {
        dir,
        journal,
        approvals,
        engine,
        front,
        resolver,
        check,
        store,
    }
}

/// A started hub with the usual providers: `sandbox` (python_sandbox), `memory` (recall,
/// remember, both sandbox-callable) and the built-in `work`.
pub async fn started(
    tweak: impl FnOnce(&mut McpFrontConfig),
) -> (Env, Arc<TestProvider>, Arc<TestProvider>) {
    let e = env(tweak).await;
    let sandbox = TestProvider::new(
        "sandbox",
        vec![server("sandbox", vec![spec("python_sandbox", false)])],
    );
    let memory = TestProvider::new(
        "memory-service",
        vec![server(
            "memory",
            vec![spec("recall", true), spec("remember", true)],
        )],
    );
    e.front.register(sandbox.clone()).unwrap();
    e.front.register(memory.clone()).unwrap();
    e.front.bind_ref_resolver(e.resolver.clone()).unwrap();
    e.front.start(CancellationToken::new()).await.unwrap();
    (e, sandbox, memory)
}

/// Like `started`, with `extra` providers registered before `start`.
pub async fn started_with(
    tweak: impl FnOnce(&mut McpFrontConfig),
    extra: Vec<Arc<dyn ToolProvider>>,
) -> (Env, Arc<TestProvider>, Arc<TestProvider>) {
    let e = env(tweak).await;
    let sandbox = TestProvider::new(
        "sandbox",
        vec![server("sandbox", vec![spec("python_sandbox", false)])],
    );
    let memory = TestProvider::new(
        "memory-service",
        vec![server(
            "memory",
            vec![spec("recall", true), spec("remember", true)],
        )],
    );
    e.front.register(sandbox.clone()).unwrap();
    e.front.register(memory.clone()).unwrap();
    for provider in extra {
        e.front.register(provider).unwrap();
    }
    e.front.bind_ref_resolver(e.resolver.clone()).unwrap();
    e.front.start(CancellationToken::new()).await.unwrap();
    (e, sandbox, memory)
}

/// An opened session with what a test needs to reach it.
pub struct Opened {
    pub run: RunId,
    pub workspace: WorkspaceId,
    pub agent: AgentInstanceId,
    pub config: SessionMcpConfig,
}

impl Opened {
    pub fn launch(&self, server: &str) -> &ordin_mcp_front::McpServerLaunch {
        self.config
            .servers
            .iter()
            .find(|s| s.name.as_str() == server)
            .unwrap_or_else(|| panic!("server {server} not launched"))
    }

    pub fn token(&self) -> String {
        self.config.servers[0].env["ORDIN_MCP_TOKEN"].clone()
    }

    pub fn endpoint(&self) -> String {
        self.config.servers[0].env["ORDIN_MCP_ENDPOINT"].clone()
    }

    /// Connects a bridge for `server` with this session's token and reads the handshake answer.
    pub async fn connect(&self, server: &str) -> Bridge {
        let mut bridge = Bridge::dial(&self.endpoint()).await;
        let reply = bridge
            .handshake(&self.run.to_string(), server, &self.token())
            .await;
        assert_eq!(reply, json!({"kind": "ready"}), "handshake for {server}");
        bridge
    }
}

impl Env {
    pub fn socket(&self) -> PathBuf {
        self.dir.0.join("run").join("mcp.sock")
    }

    /// Loads a rule set for a new agent instance: tools in `deny` are refused by its tool policy,
    /// tools in `ask` always ask.
    pub fn agent(&self, deny: &[&str], ask: &[&str]) -> AgentInstanceId {
        let id = AgentInstanceId::new();
        let set = InstanceRuleSet {
            agent_instance_id: id,
            package_name: "analyst".to_owned(),
            authority: Authority::SystemChanging,
            agent_folder: app_root().join("agents").join("eng-1"),
            agent_folder_read_only: false,
            declaration: None,
            configured: None,
            tool_policy: Some(ordin_rules::AgentToolPolicy {
                prohibited_verbs: deny.iter().map(|s| (*s).to_owned()).collect(),
                ..Default::default()
            }),
            approval_list: ask.iter().map(|s| (*s).to_owned()).collect(),
            package_read_only_tools: Vec::new(),
            tools: Vec::new(),
            unmapped_operations: Vec::new(),
            external: Vec::new(),
            commands: Vec::new(),
            reviewer: None,
        };
        self.engine.load_rule_set(set).expect("the set loads");
        id
    }

    pub async fn begin_run(&self, agent: AgentInstanceId) -> (RunId, WorkspaceId) {
        let run = RunId::new();
        let workspace = WorkspaceId::new();
        self.engine
            .begin_run(RunStartFacts {
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
            })
            .await
            .expect("the run begins");
        (run, workspace)
    }

    pub fn grant(
        &self,
        agent: AgentInstanceId,
        run: RunId,
        workspace: WorkspaceId,
    ) -> SessionGrant {
        SessionGrant {
            run_id: run,
            workspace_id: workspace,
            agent_instance_id: agent,
            harness: HarnessKind::ClaudeCode,
            profile: ProgramProfile::Desktop,
            caller: ToolCaller::Agent,
            scope_id: None,
            coding_task_id: None,
            only_servers: None,
        }
    }

    /// Loads an agent and opens a session for a new run of it.
    pub async fn open(&self, deny: &[&str], ask: &[&str]) -> Opened {
        let agent = self.agent(deny, ask);
        self.open_for(agent).await
    }

    pub async fn open_for(&self, agent: AgentInstanceId) -> Opened {
        let (run, workspace) = self.begin_run(agent).await;
        let config = self
            .front
            .open_session(self.grant(agent, run, workspace))
            .await
            .expect("open_session");
        Opened {
            run,
            workspace,
            agent,
            config,
        }
    }

    /// The data of every journal event of `kind`, oldest first.
    pub async fn events(&self, kind: &str) -> Vec<Value> {
        let rows: Vec<String> =
            sqlx::query_scalar("SELECT data FROM journal_events WHERE kind = ?1 ORDER BY seq")
                .bind(kind)
                .fetch_all(self.store.sqlite().expect("SQLite"))
                .await
                .expect("events");
        rows.iter()
            .map(|d| serde_json::from_str(d).expect("event JSON"))
            .collect()
    }

    /// Runs SQL against the test database (used to install failure triggers).
    pub async fn sql(&self, statement: &'static str) {
        sqlx::query(statement)
            .execute(self.store.sqlite().expect("SQLite"))
            .await
            .expect("sql");
    }

    /// The number of tool requests the Rules Engine journalled for `run`.
    pub async fn tool_requests(&self, run: RunId) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM tool_requests WHERE run_id = ?1")
            .bind(run.to_string())
            .fetch_one(self.store.sqlite().expect("SQLite"))
            .await
            .expect("count tool requests")
    }
}

// ---- bridge client ----

type ReadHalf = Box<dyn tokio::io::AsyncRead + Unpin + Send>;
type WriteHalf = Box<dyn tokio::io::AsyncWrite + Unpin + Send>;

pub struct Bridge {
    reader: BufReader<ReadHalf>,
    writer: WriteHalf,
}

/// The endpoint's two halves: a Unix domain socket on macOS and Linux.
#[cfg(unix)]
async fn open_endpoint(path: &str) -> (ReadHalf, WriteHalf) {
    let stream = tokio::net::UnixStream::connect(path)
        .await
        .expect("connect to the socket");
    let (r, w) = stream.into_split();
    (Box::new(r), Box::new(w))
}

/// The endpoint's two halves: the named pipe on Windows.
#[cfg(windows)]
async fn open_endpoint(path: &str) -> (ReadHalf, WriteHalf) {
    use tokio::net::windows::named_pipe::ClientOptions;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let client = loop {
        match ClientOptions::new().open(path) {
            Ok(client) => break client,
            // ERROR_PIPE_BUSY (231): the front has not yet made the next pipe instance.
            Err(e) if e.raw_os_error() == Some(231) && std::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Err(e) => panic!("connect to the pipe: {e}"),
        }
    };
    let (r, w) = tokio::io::split(client);
    (Box::new(r), Box::new(w))
}

impl Bridge {
    pub async fn dial(path: &str) -> Bridge {
        let (r, w) = open_endpoint(path).await;
        Bridge {
            reader: BufReader::new(r),
            writer: w,
        }
    }

    pub async fn send_raw(&mut self, line: &str) {
        self.writer.write_all(line.as_bytes()).await.expect("write");
        self.writer.write_all(b"\n").await.expect("write");
    }

    pub async fn send(&mut self, value: Value) {
        self.send_raw(&value.to_string()).await;
    }

    /// One line, parsed; `None` when the peer closed.
    pub async fn recv_opt(&mut self, wait: Duration) -> Option<Value> {
        let mut line = String::new();
        let read = tokio::time::timeout(wait, self.reader.read_line(&mut line))
            .await
            .expect("a line within the wait")
            .expect("read");
        if read == 0 {
            return None;
        }
        Some(serde_json::from_str(line.trim()).expect("a JSON line"))
    }

    /// True when nothing arrives and the peer stays connected for `wait`.
    pub async fn quiet(&mut self, wait: Duration) -> bool {
        let mut line = String::new();
        tokio::time::timeout(wait, self.reader.read_line(&mut line))
            .await
            .is_err()
    }

    pub async fn recv(&mut self) -> Value {
        self.recv_opt(Duration::from_secs(20))
            .await
            .expect("the connection stayed open")
    }

    pub async fn handshake(&mut self, session: &str, server: &str, token: &str) -> Value {
        self.send(json!({"protocol": 1, "session": session, "server": server, "token": token}))
            .await;
        self.recv().await
    }

    /// Sends a request and reads until its answer; other lines (notifications) are returned beside it.
    pub async fn request(&mut self, id: i64, method: &str, params: Value) -> (Value, Vec<Value>) {
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await;
        let mut others = Vec::new();
        loop {
            let line = self.recv().await;
            if line.get("id") == Some(&json!(id)) {
                return (line, others);
            }
            others.push(line);
        }
    }

    pub async fn call(&mut self, id: i64, tool: &str, arguments: Value) -> Value {
        self.request(
            id,
            "tools/call",
            json!({"name": tool, "arguments": arguments}),
        )
        .await
        .0
    }

    pub async fn list_raw(&mut self) -> Vec<Value> {
        let (answer, _) = self.request(1000, "tools/list", json!({})).await;
        answer["result"]["tools"]
            .as_array()
            .expect("tools array")
            .clone()
    }

    pub async fn list(&mut self) -> Vec<String> {
        let (answer, _) = self.request(1000, "tools/list", json!({})).await;
        answer["result"]["tools"]
            .as_array()
            .expect("tools array")
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_owned())
            .collect()
    }
}

/// The text of a tools/call answer.
pub fn text_of(answer: &Value) -> String {
    answer["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_owned()
}

pub fn is_error(answer: &Value) -> bool {
    answer["result"]["isError"].as_bool().unwrap_or(false)
}

/// A valid table block as the `report` tool takes it (kind `table`).
pub fn table_block(title: &str, padding: usize) -> Value {
    json!({
        "kind": "table",
        "title": title,
        "columns": [{"name": "Item", "type": "text"}],
        "rows": [[ "x".repeat(padding.max(1)) ]],
        "source": {"kind": "earlier_result", "run_id": RunId::new().to_string()}
    })
}

/// A report of `parts` table parts.
pub fn report_block(parts: usize) -> Value {
    let parts: Vec<Value> = (0..parts)
        .map(|i| {
            let mut table = table_block(&format!("Part {i}"), 4);
            table["kind"] = json!("table");
            json!({"heading": format!("Heading {i}"), "content": table})
        })
        .collect();
    json!({"kind": "report", "title": "Quarter", "as_of": "2026-01-01T00:00:00.000Z", "parts": parts})
}

/// The `code` of every `mcp_front.call` log line of `run`, oldest first.
pub fn call_codes(run: RunId) -> Vec<String> {
    let run = run.to_string();
    log_lines(&["mcp_front.call", &run])
        .iter()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|v| v["event"] == "mcp_front.call" && v["run_id"] == run.as_str())
        .map(|v| v["code"].as_str().unwrap_or("").to_owned())
        .collect()
}

/// The `outcome` of every `mcp_front.call` log line of `run`.
pub fn call_outcomes(run: RunId) -> Vec<String> {
    let run = run.to_string();
    log_lines(&["mcp_front.call", &run])
        .iter()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|v| v["event"] == "mcp_front.call" && v["run_id"] == run.as_str())
        .map(|v| v["outcome"].as_str().unwrap_or("").to_owned())
        .collect()
}

pub fn lookup_codes() -> HashMap<&'static str, bool> {
    ordin_mcp_front::ALL_CODES
        .iter()
        .map(|c| (*c, ordin_types::error_registry::lookup(c).is_some()))
        .collect()
}

/// A `ShownBlockStore` that records what it is asked to keep, or refuses (McpFront.md amendment
/// 2026-01-24).
#[derive(Default)]
pub struct KeptBlocks {
    calls: Mutex<
        Vec<(
            WorkspaceId,
            ordin_types::BlockId,
            ordin_service_api::blocks::Block,
        )>,
    >,
    refusal: Mutex<Option<ordin_mcp_front::ToolFailure>>,
}

impl KeptBlocks {
    pub fn new() -> Arc<KeptBlocks> {
        Arc::new(KeptBlocks::default())
    }

    pub fn refuse(&self, failure: ordin_mcp_front::ToolFailure) {
        *self.refusal.lock().unwrap() = Some(failure);
    }

    pub fn calls(
        &self,
    ) -> Vec<(
        WorkspaceId,
        ordin_types::BlockId,
        ordin_service_api::blocks::Block,
    )> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl ordin_mcp_front::ShownBlockStore for KeptBlocks {
    async fn keep_shown_block(
        &self,
        workspace_id: WorkspaceId,
        block_id: ordin_types::BlockId,
        block: &ordin_service_api::blocks::Block,
    ) -> Result<(), ordin_mcp_front::ToolFailure> {
        if let Some(failure) = self.refusal.lock().unwrap().clone() {
            return Err(failure);
        }
        self.calls
            .lock()
            .unwrap()
            .push((workspace_id, block_id, block.clone()));
        Ok(())
    }
}
