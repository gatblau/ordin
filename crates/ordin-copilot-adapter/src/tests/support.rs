//! The test doubles of the spec's *Background*: "a scripted CopilotPort double, a CopilotCliSource
//! double, and test doubles of the MCP Front, the Hook Relay, the Rules Engine and Identity".
#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

use futures::stream::{BoxStream, StreamExt};
use ordin_hook_relay::{HookRelayError, HookSessionEnv, HookSessionEvent, HookSessionSpec};
use ordin_identity::CredError;
use ordin_mcp_front::{
    McpError, McpServerLaunch, ProgramProfile, ServerName, SessionGrant, SessionMcpConfig,
    ToolCallResult, ToolKey, ToolSpec,
};
use ordin_rules::{RulesError, RunStartDecision, RunStartFacts, StartOutcome};
use ordin_runtime_adapter::{
    AdapterContext, RunEvent, RunHandle, RunLimits, RunTarget, RuntimeAdapter, StartInput,
    StartRun, ToolCaller,
};
use ordin_types::domain::identity::CredentialKind;
use ordin_types::wire::hook::{HookMessage, HookReply};
use ordin_types::{AgentInstanceId, ArtefactId, HookCallId, RunId, WorkspaceId};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::adapter::{CopilotLocalAdapter, Host, Services, Timings};
use crate::config::{
    CopilotAdapterConfig, CopilotCli, CopilotCliSource, CopilotSessionSetup, SessionSetupError,
    SessionSetupSource,
};
use crate::env::ParentEnv;
use crate::ports::{HookPort, IdentityPort, McpPort, RulesPort};
use crate::testkit::{Script, ScriptedCopilotPort};

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
        let text = String::from_utf8_lossy(&self.0).to_string();
        LOGS.lock().unwrap().push(text);
    }
}

/// Captures every line through the programs' own formatter.
pub fn capture_logs() {
    LOG_INIT.call_once(|| {
        let lines = ordin_types::log::subscriber(|| Capture(Vec::new()), tracing::Level::DEBUG);
        let _ = tracing::subscriber::set_global_default(lines);
    });
}

/// Every captured line holding all of `needles`.
pub fn log_lines(needles: &[&str]) -> Vec<String> {
    LOGS.lock()
        .unwrap()
        .iter()
        .filter(|line| needles.iter().all(|n| line.contains(n)))
        .cloned()
        .collect()
}

pub async fn expect_log(needles: &[&str]) {
    let what = needles.join(" ");
    poll(&format!("log line {what}"), || {
        (!log_lines(needles).is_empty()).then_some(())
    })
    .await;
}

// ---- waiting ----

pub async fn poll<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    for _ in 0..1500 {
        if let Some(found) = check() {
            return found;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("never: {what}");
}

/// A folder removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("cpa-{}", RunId::new()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---- doubles ----

#[derive(Default)]
pub struct McpDouble {
    pub opened: Mutex<Vec<SessionGrant>>,
    pub closed: Mutex<Vec<RunId>>,
    pub error: Mutex<Option<McpError>>,
    pub servers: Mutex<Vec<&'static str>>,
    /// What `harness_tools` lists: `(server, tool)`.
    pub offered: Mutex<Vec<(&'static str, String)>>,
    /// Every `dispatch_harness` call: `(run, server, tool, arguments)`.
    pub dispatched: Mutex<Vec<(RunId, String, String, serde_json::Value)>>,
    /// What `dispatch_harness` returns; `None` waits for the call's cancel and returns the
    /// pipeline's `MCP_CALL_CANCELLED` result.
    pub answer: Mutex<Option<Result<ToolCallResult, McpError>>>,
}

pub fn tool_result(is_error: bool, text: &str) -> ToolCallResult {
    ToolCallResult {
        is_error,
        text: text.to_owned(),
        structured: None,
        code: None,
    }
}

impl McpDouble {
    pub fn is_open(&self, run: RunId) -> bool {
        let opened = self
            .opened
            .lock()
            .unwrap()
            .iter()
            .filter(|g| g.run_id == run)
            .count();
        let closed = self
            .closed
            .lock()
            .unwrap()
            .iter()
            .filter(|r| **r == run)
            .count();
        opened > closed
    }
}

#[async_trait::async_trait]
impl McpPort for McpDouble {
    async fn open_session(&self, grant: SessionGrant) -> Result<SessionMcpConfig, McpError> {
        if let Some(error) = self.error.lock().unwrap().clone() {
            return Err(error);
        }
        self.opened.lock().unwrap().push(grant);
        let servers = self
            .servers
            .lock()
            .unwrap()
            .iter()
            .map(|name| McpServerLaunch {
                name: ServerName::new(*name).unwrap(),
                command: PathBuf::from("/opt/ordin/bin/ordin-hook"),
                args: vec!["mcp-bridge".to_owned(), (*name).to_owned()],
                env: BTreeMap::from([("ORDIN_MCP_TOKEN".to_owned(), "token-1".to_owned())]),
            })
            .collect();
        Ok(SessionMcpConfig { servers })
    }
    async fn close_session(&self, run_id: RunId) -> Result<(), McpError> {
        self.closed.lock().unwrap().push(run_id);
        Ok(())
    }
    fn harness_tools(&self, _run_id: RunId) -> Result<Vec<(ToolKey, ToolSpec)>, McpError> {
        Ok(self
            .offered
            .lock()
            .unwrap()
            .iter()
            .map(|(server, tool)| {
                (
                    ToolKey {
                        server: ServerName::new(*server).unwrap(),
                        tool: tool.clone(),
                    },
                    ToolSpec {
                        name: tool.clone(),
                        description: format!("The {tool} tool."),
                        input_schema: serde_json::json!({"type": "object"}),
                        argument_form: "{}".into(),
                        sandbox_callable: false,
                        plain: "Used a tool".into(),
                    },
                )
            })
            .collect())
    }
    async fn dispatch_harness(
        &self,
        run_id: RunId,
        key: ToolKey,
        arguments: serde_json::Value,
        cancel: CancellationToken,
    ) -> Result<ToolCallResult, McpError> {
        self.dispatched.lock().unwrap().push((
            run_id,
            key.server.as_str().to_owned(),
            key.tool.clone(),
            arguments,
        ));
        let answer = self.answer.lock().unwrap().clone();
        match answer {
            Some(answer) => answer,
            None => {
                cancel.cancelled().await;
                Ok(tool_result(true, "The call was cancelled."))
            }
        }
    }
}

/// How the relay double answers `decide_in_process`.
#[derive(Clone)]
pub enum Decide {
    Allow,
    Deny(String),
    /// Never answers.
    Hang,
}

pub struct HookDouble {
    pub opened: Mutex<Vec<RunId>>,
    pub closed: Mutex<Vec<RunId>>,
    pub fail: Mutex<bool>,
    pub pre_tool_use: Mutex<Decide>,
    pub stop: Mutex<Decide>,
    pub messages: Mutex<Vec<(RunId, HookMessage)>>,
    sender: broadcast::Sender<(RunId, HookSessionEvent)>,
}

impl Default for HookDouble {
    fn default() -> Self {
        HookDouble {
            opened: Mutex::default(),
            closed: Mutex::default(),
            fail: Mutex::new(false),
            pre_tool_use: Mutex::new(Decide::Allow),
            stop: Mutex::new(Decide::Allow),
            messages: Mutex::default(),
            sender: broadcast::channel(256).0,
        }
    }
}

impl HookDouble {
    pub fn send(&self, run: RunId, event: HookSessionEvent) {
        let _ = self.sender.send((run, event));
    }

    pub fn is_open(&self, run: RunId) -> bool {
        let opened = self
            .opened
            .lock()
            .unwrap()
            .iter()
            .filter(|r| **r == run)
            .count();
        let closed = self
            .closed
            .lock()
            .unwrap()
            .iter()
            .filter(|r| **r == run)
            .count();
        opened > closed
    }

    pub fn messages(&self) -> Vec<serde_json::Value> {
        self.messages
            .lock()
            .unwrap()
            .iter()
            .map(|(_, m)| serde_json::to_value(m).unwrap())
            .collect()
    }
}

#[async_trait::async_trait]
impl HookPort for HookDouble {
    fn open_session(&self, spec: HookSessionSpec) -> Result<HookSessionEnv, HookRelayError> {
        if *self.fail.lock().unwrap() {
            return Err(HookRelayError::Random);
        }
        self.opened.lock().unwrap().push(spec.run_id);
        Ok(HookSessionEnv {
            endpoint: "/tmp/hook.sock".to_owned(),
            secret: "s3cret".to_owned(),
            session_id: spec.run_id,
            protocol: 1,
        })
    }
    fn close_session(&self, run_id: RunId) {
        self.closed.lock().unwrap().push(run_id);
    }
    fn session_events(&self, run_id: RunId) -> BoxStream<'static, HookSessionEvent> {
        let receiver = self.sender.subscribe();
        futures::stream::unfold(receiver, move |mut receiver| async move {
            loop {
                match receiver.recv().await {
                    Ok((run, event)) if run == run_id => return Some((event, receiver)),
                    Ok(_) => {}
                    Err(_) => return None,
                }
            }
        })
        .boxed()
    }
    async fn decide_in_process(&self, run_id: RunId, message: HookMessage) -> HookReply {
        let script = match &message {
            HookMessage::PreToolUse { .. } => self.pre_tool_use.lock().unwrap().clone(),
            HookMessage::Stop { .. } => self.stop.lock().unwrap().clone(),
            HookMessage::PostToolUse { .. } => Decide::Allow,
        };
        let post = matches!(message, HookMessage::PostToolUse { .. });
        self.messages.lock().unwrap().push((run_id, message));
        let call_id = HookCallId::new();
        if post {
            return HookReply::Ack { call_id };
        }
        match script {
            Decide::Allow => HookReply::Allow { call_id },
            Decide::Deny(reason) => HookReply::Deny { call_id, reason },
            Decide::Hang => futures::future::pending().await,
        }
    }
}

pub enum RulesScript {
    Proceed,
    Decision(RunStartDecision),
    Unavailable,
}

pub struct RulesDouble {
    pub script: Mutex<RulesScript>,
    pub facts: Mutex<Vec<RunStartFacts>>,
    /// `begin_run` answers after this long.
    pub delay: Mutex<Duration>,
}

impl Default for RulesDouble {
    fn default() -> Self {
        RulesDouble {
            script: Mutex::new(RulesScript::Proceed),
            facts: Mutex::default(),
            delay: Mutex::new(Duration::ZERO),
        }
    }
}

pub fn proceed() -> RunStartDecision {
    RunStartDecision {
        outcome: StartOutcome::Proceed,
        command: None,
        tool_subset: None,
        single_call: false,
        limit_seconds: None,
        actions_attached: vec![],
        delivery: None,
        controls_decided: vec![],
    }
}

#[async_trait::async_trait]
impl RulesPort for RulesDouble {
    async fn begin_run(&self, facts: RunStartFacts) -> Result<RunStartDecision, RulesError> {
        self.facts.lock().unwrap().push(facts);
        let delay = *self.delay.lock().unwrap();
        tokio::time::sleep(delay).await;
        match &*self.script.lock().unwrap() {
            RulesScript::Proceed => Ok(proceed()),
            RulesScript::Decision(decision) => Ok(decision.clone()),
            RulesScript::Unavailable => Err(RulesError::RuleSetNotLoaded),
        }
    }
}

/// Identity: `refresh_silently(Github)` answers `refreshed`; a `true` also signs the CLI in.
pub struct IdentityDouble {
    pub refreshed: Mutex<bool>,
    /// `refresh_silently` fails with this retryable code.
    pub fails: Mutex<Option<&'static str>>,
    pub calls: Mutex<Vec<CredentialKind>>,
    pub signs_in: Mutex<Option<Arc<Script>>>,
}

impl Default for IdentityDouble {
    fn default() -> Self {
        IdentityDouble {
            refreshed: Mutex::new(false),
            fails: Mutex::new(None),
            calls: Mutex::default(),
            signs_in: Mutex::new(None),
        }
    }
}

#[async_trait::async_trait]
impl IdentityPort for IdentityDouble {
    async fn refresh_silently(&self, kind: CredentialKind) -> Result<bool, CredError> {
        self.calls.lock().unwrap().push(kind);
        if let Some(code) = *self.fails.lock().unwrap() {
            return Err(CredError::Retryable { code });
        }
        let refreshed = *self.refreshed.lock().unwrap();
        if refreshed {
            if let Some(script) = self.signs_in.lock().unwrap().as_ref() {
                script.signed_in.store(true, Ordering::SeqCst);
            }
        }
        Ok(refreshed)
    }
}

/// The program's session setup: a fixed one, or a failure with `code`.
pub struct SetupDouble {
    pub setup: Mutex<CopilotSessionSetup>,
    pub fail: Mutex<Option<String>>,
    pub calls: Mutex<Vec<Vec<ArtefactId>>>,
}

#[async_trait::async_trait]
impl SessionSetupSource for SetupDouble {
    async fn session_setup(
        &self,
        _run_id: RunId,
        _agent_instance_id: AgentInstanceId,
        attachments: &[ArtefactId],
    ) -> Result<CopilotSessionSetup, SessionSetupError> {
        self.calls.lock().unwrap().push(attachments.to_vec());
        if let Some(code) = self.fail.lock().unwrap().clone() {
            return Err(SessionSetupError { code });
        }
        Ok(self.setup.lock().unwrap().clone())
    }
}

pub const CLI_PATH: &str = "/home/p/.local/bin/copilot";
pub const CLI_VERSION: &str = "1.0.98";

/// The person's CLI: "/home/p/.local/bin/copilot" at "1.0.98", or none.
pub struct CliDouble(pub Mutex<Option<CopilotCli>>);

impl CliDouble {
    pub fn installed() -> CliDouble {
        CliDouble(Mutex::new(Some(CopilotCli {
            path: PathBuf::from(CLI_PATH),
            version: CLI_VERSION.to_owned(),
        })))
    }
}

#[async_trait::async_trait]
impl CopilotCliSource for CliDouble {
    async fn current(&self) -> Option<CopilotCli> {
        self.0.lock().unwrap().clone()
    }
}

// ---- the harness ----

/// Short sign-in durations: polls every 20 ms, gives up after 2 s; gates bounded at 300 ms.
pub const TEST_TIMINGS: Timings = Timings {
    sign_in_poll: Duration::from_millis(20),
    sign_in_give_up: Duration::from_secs(2),
    gate_bound: Duration::from_millis(300),
};

/// The parent environment: credentials present, `PATH` without the CLI's folder.
pub fn parent_env() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        ("PATH", "/usr/bin:/bin"),
        ("HOME", "/home/p"),
        ("LANG", "en_GB.UTF-8"),
        ("COPILOT_GITHUB_TOKEN", "ghu_secret"),
        ("GH_TOKEN", "gho_secret"),
        ("GITHUB_TOKEN", "ghp_secret"),
        ("ORDIN_SECRET", "never"),
    ])
}

pub struct Harness {
    pub dir: TempDir,
    pub port: ScriptedCopilotPort,
    pub cli: Arc<CliDouble>,
    pub mcp: Arc<McpDouble>,
    pub hooks: Arc<HookDouble>,
    pub rules: Arc<RulesDouble>,
    pub identity: Arc<IdentityDouble>,
    pub setup: Arc<SetupDouble>,
    pub adapter: CopilotLocalAdapter,
}

pub fn setup_of(folder: PathBuf) -> CopilotSessionSetup {
    CopilotSessionSetup {
        agent_folder: folder,
        extra_dirs: vec![],
        model: None,
        attachment_paths: vec![],
        package_servers: vec![],
        resume_session_id: None,
        crew_charter_id: None,
    }
}

pub fn config(root: PathBuf, profile: ProgramProfile) -> CopilotAdapterConfig {
    CopilotAdapterConfig {
        tested_versions: vec![CLI_VERSION.to_owned()],
        app_data_root: root,
        profile,
        stop_deadline: Duration::from_millis(500),
    }
}

impl Harness {
    /// Signed in, everything answering.
    pub fn new() -> Harness {
        Harness::with(ScriptedCopilotPort::signed_in(), |_| {})
    }

    pub fn with(
        port: ScriptedCopilotPort,
        edit: impl FnOnce(&mut CopilotAdapterConfig),
    ) -> Harness {
        capture_logs();
        let dir = TempDir::new();
        let agent_folder = dir.0.join("agent");
        std::fs::create_dir_all(&agent_folder).unwrap();
        let cli = Arc::new(CliDouble::installed());
        let mcp = Arc::new(McpDouble::default());
        *mcp.servers.lock().unwrap() = vec!["work", "memory"];
        let hooks = Arc::new(HookDouble::default());
        let rules = Arc::new(RulesDouble::default());
        let identity = Arc::new(IdentityDouble::default());
        let setup = Arc::new(SetupDouble {
            setup: Mutex::new(setup_of(agent_folder)),
            fail: Mutex::new(None),
            calls: Mutex::default(),
        });
        let mut config = config(dir.0.clone(), ProgramProfile::Desktop);
        edit(&mut config);
        let env = parent_env();
        let parent: ParentEnv = Arc::new(move |name: &str| env.get(name).map(|v| (*v).into()));
        let adapter = CopilotLocalAdapter::build(
            config,
            Services {
                port: Arc::new(port.clone()),
                cli: cli.clone(),
                mcp: mcp.clone(),
                hooks: hooks.clone(),
                rules: rules.clone(),
                identity: identity.clone(),
                setup: setup.clone(),
            },
            Host {
                timings: TEST_TIMINGS,
                parent_env: parent,
            },
        )
        .expect("the adapter builds");
        Harness {
            dir,
            port,
            cli,
            mcp,
            hooks,
            rules,
            identity,
            setup,
            adapter,
        }
    }

    pub fn script(&self) -> &Script {
        &self.port.script
    }

    pub async fn start(
        &self,
        req: StartRun,
    ) -> Result<RunHandle, ordin_runtime_adapter::AdapterError> {
        self.adapter.start(&ctx(), req).await
    }

    /// Starts a free-text run and waits for its session to exist.
    pub async fn start_live(&self, text: &str) -> (RunHandle, String) {
        let handle = self.start(free_text(text)).await.expect("start");
        let session = handle.runtime_session_id.clone().expect("a session id");
        (handle, session)
    }

    pub fn events(&self, handle: &RunHandle) -> BoxStream<'static, RunEvent> {
        self.adapter.events(&ctx(), handle).expect("events")
    }

    /// The events so far, waiting until `done` holds for them.
    pub async fn events_until(
        &self,
        handle: &RunHandle,
        done: impl Fn(&[RunEvent]) -> bool,
    ) -> Vec<RunEvent> {
        let mut stream = self.events(handle);
        let mut seen = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while !done(&seen) {
            match tokio::time::timeout_at(deadline, stream.next()).await {
                Ok(Some(event)) => seen.push(event),
                Ok(None) => break,
                Err(_) => panic!("events never matched; saw {seen:?}"),
            }
        }
        seen
    }

    pub async fn until_ended(&self, handle: &RunHandle) -> Vec<RunEvent> {
        self.events_until(handle, |seen| {
            matches!(seen.last(), Some(RunEvent::Ended { .. }))
        })
        .await
    }
}

pub fn ctx() -> AdapterContext {
    AdapterContext {
        correlation_id: "corr-1".to_owned(),
        deadline: std::time::Instant::now() + Duration::from_secs(10),
        cancel: CancellationToken::new(),
    }
}

pub fn request(input: StartInput) -> StartRun {
    StartRun {
        run_id: RunId::new(),
        workspace_id: WorkspaceId::new(),
        target: RunTarget::AgentInstance {
            agent_instance_id: AgentInstanceId::new(),
        },
        input,
        attachments: vec![],
        limits: RunLimits {
            wall_clock_secs: 600,
            request_budget: None,
            idle_timeout_secs: 300,
        },
        thread: None,
        model: None,
        thinking: None,
        effort: None,
        caller: ToolCaller::Agent,
        scope_id: None,
        coding_task_id: None,
        started_by_person: true,
        only_servers: None,
    }
}

pub fn free_text(text: &str) -> StartRun {
    request(StartInput::FreeText {
        text: text.to_owned(),
    })
}

/// The kinds of `events`, as their wire `kind`.
pub fn kinds(events: &[RunEvent]) -> Vec<String> {
    events
        .iter()
        .map(|e| {
            serde_json::to_value(e).unwrap()["kind"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect()
}

pub fn end_reason(events: &[RunEvent]) -> Option<String> {
    events.iter().find_map(|e| match e {
        RunEvent::Ended { end } => Some(
            serde_json::to_value(end.reason)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned(),
        ),
        _ => None,
    })
}
