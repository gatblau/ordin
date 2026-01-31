//! A fake `claude` program written by each test, and the test doubles of the spec's *Background*:
//! "a fake "claude" executable that records its arguments and environment and replays scripted
//! stream-json output", "an MCP Front, a Hook Relay and a Rules Engine test double".
#![allow(dead_code)]

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

use futures::stream::{BoxStream, StreamExt};
use ordin_hook_relay::{HookRelayError, HookSessionEnv, HookSessionEvent, HookSessionSpec};
use ordin_mcp_front::{
    McpError, McpServerLaunch, ProgramProfile, ServerName, SessionGrant, SessionMcpConfig,
};
use ordin_rules::{AdoptedRun, RulesError, RunStartDecision, RunStartFacts, StartOutcome};
use ordin_runtime_adapter::{
    AdapterContext, RunEvent, RunHandle, RunLimits, RunTarget, RuntimeAdapter, StartInput,
    StartRun, ToolCaller,
};
use ordin_types::domain::agent_profile::{
    HarnessEnvVar, HarnessModelSettings, ModelProvider, OrganisationKey,
};
use ordin_types::{AgentInstanceId, RunId, WorkspaceId};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::adapter::{ClaudeLocalAdapter, Host, Timings};
use crate::config::{
    ClaudeAdapterConfig, KeyRoute, RecordError, SessionRecordStore, SessionSetup,
    SessionSetupError, SessionSetupSource, SignInMode,
};
use crate::env::ParentEnv;
use crate::ports::{HookPort, McpPort, RulesPort};

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

pub fn capture_logs() {
    LOG_INIT.call_once(|| {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_ansi(false)
            .with_writer(|| Capture(Vec::new()))
            .init();
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

pub async fn expect_log(run: RunId, code: &str) {
    let run_text = run.to_string();
    poll(&format!("log line {code}"), || {
        (!log_lines(&[&run_text, code]).is_empty()).then_some(())
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

// ---- the fake claude ----

/// A folder removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir()
            .canonicalize()
            .map(ordin_types::paths::plain_form)
            .expect("temporary root")
            .join(format!(
                "cla-{}",
                &uuid::Uuid::now_v7().simple().to_string()[16..]
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

/// Keeps the text in a shell's single quotes.
fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The script's main body: records what it was given, then runs the test's scripted parts, each a
/// file of shell lines. `next_turn` reads and records one more stdin line.
const FAKE: &str = r#"#!/bin/sh
next_turn() { IFS= read -r line && printf '%s\n' "$line" >> "$D/stdin.$n"; }
say() { printf '%s\n' "$1"; }
echo "$*" >> "$D/calls"
case "$1" in
  --version) . "$D/version.sh"; exit $? ;;
  auth)
    case "$2" in
      status) . "$D/auth.sh"; exit $? ;;
      login)
        test -t 0 && echo tty >> "$D/login-tty"
        echo login >> "$D/login"
        . "$D/login.sh"; exit $? ;;
    esac ;;
esac
n=$(cat "$D/count" 2>/dev/null || echo 0)
n=$((n + 1))
echo $n > "$D/count"
printf '%s\n' "$@" > "$D/args.$n"
env > "$D/env.$n"
pwd -P > "$D/cwd.$n"
echo $$ > "$D/pid.$n"
next_turn
if [ -f "$D/session.$n.sh" ]; then . "$D/session.$n.sh"; else . "$D/session.sh"; fi
"#;

pub const RESULT_OK: &str = r#"{"type":"result","is_error":false,"result":"Kestrel is cheapest","usage":{"input_tokens":18234,"output_tokens":1210,"cache_read_input_tokens":15002}}"#;

/// One test's fake program and folders.
pub struct Fake {
    pub dir: TempDir,
    pub claude: PathBuf,
    pub agent_folder: PathBuf,
    pub data_root: PathBuf,
}

/// A file this process has just written cannot be executed while another test thread still holds
/// an inherited copy of its write handle (`ETXTBSY`, "text file busy", errno 26): the other thread's
/// child has been forked but has not yet executed. The handle closes when that child executes, within
/// milliseconds, so the fake is probed until the operating system lets it run. The probe argument
/// makes the script exit before it records anything.
fn wait_until_executable(claude: &Path) {
    const TEXT_FILE_BUSY: i32 = 26;
    for _ in 0..500 {
        match std::process::Command::new(claude)
            .arg("--exec-probe")
            .status()
        {
            Err(error) if error.raw_os_error() == Some(TEXT_FILE_BUSY) => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            _ => return,
        }
    }
    panic!("the fake program stayed busy for 5 s");
}

impl Fake {
    pub fn new() -> Fake {
        capture_logs();
        let dir = TempDir::new();
        let root = dir.0.clone();
        let agent_folder = root.join("agent");
        let data_root = root.join("data");
        std::fs::create_dir_all(&agent_folder).unwrap();
        std::fs::create_dir_all(&data_root).unwrap();
        let claude = root.join("claude");
        let script = FAKE.replacen(
            "#!/bin/sh\n",
            &format!(
                "#!/bin/sh\nD={}\n[ \"$1\" = --exec-probe ] && exit 0\n",
                sh_quote(&root.to_string_lossy())
            ),
            1,
        );
        std::fs::write(&claude, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
        wait_until_executable(&claude);
        let fake = Fake {
            dir,
            claude,
            agent_folder,
            data_root,
        };
        fake.script("version.sh", "say '2.1.304 (Claude Code)'");
        fake.script(
            "auth.sh",
            r#"say '{"loggedIn":true,"authMethod":"claude.ai"}'"#,
        );
        fake.script("login.sh", "exit 0");
        fake.script("session.sh", &format!("say '{RESULT_OK}'\nnext_turn"));
        fake
    }

    pub fn root(&self) -> &Path {
        &self.dir.0
    }

    /// Replaces one scripted part.
    pub fn script(&self, name: &str, body: &str) {
        std::fs::write(self.root().join(name), body).unwrap();
    }

    fn lines_of(&self, name: &str) -> Vec<String> {
        std::fs::read_to_string(self.root().join(name))
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    /// The arguments of the `n`th session child (from 1).
    pub fn args(&self, n: u32) -> Vec<String> {
        self.lines_of(&format!("args.{n}"))
    }

    pub fn has_args(&self, n: u32, wanted: &[&str]) -> bool {
        self.args(n).windows(wanted.len()).any(|w| w == wanted)
    }

    /// The environment of the `n`th session child, as `NAME=value` lines.
    pub fn env(&self, n: u32) -> BTreeMap<String, String> {
        self.lines_of(&format!("env.{n}"))
            .into_iter()
            .filter_map(|line| {
                line.split_once('=')
                    .map(|(name, value)| (name.to_owned(), value.to_owned()))
            })
            .collect()
    }

    pub fn stdin_lines(&self, n: u32) -> Vec<String> {
        self.lines_of(&format!("stdin.{n}"))
    }

    /// Every invocation of the program, as its joined arguments.
    pub fn calls(&self) -> Vec<String> {
        self.lines_of("calls")
    }

    pub fn session_count(&self) -> u32 {
        self.lines_of("count")
            .first()
            .and_then(|n| n.parse().ok())
            .unwrap_or(0)
    }

    pub fn config(&self) -> ClaudeAdapterConfig {
        ClaudeAdapterConfig {
            claude_path: self.claude.clone(),
            hook_program: PathBuf::from("/opt/ordin/bin/ordin-hook"),
            app_data_root: self.data_root.clone(),
            profile: ProgramProfile::Desktop,
            tested_versions: vec!["2.1.304".to_owned()],
            sign_in: SignInMode::PersonSignIn,
            stop_deadline: Duration::from_secs(5),
        }
    }

    pub fn agent_config(
        &self,
        models: HarnessModelSettings,
        route: KeyRoute,
    ) -> ClaudeAdapterConfig {
        ClaudeAdapterConfig {
            profile: ProgramProfile::AgentProfile,
            sign_in: SignInMode::OrganisationKey { models, route },
            ..self.config()
        }
    }

    pub fn session_dir(&self, run: RunId) -> PathBuf {
        self.data_root
            .join("run")
            .join("sessions")
            .join(run.to_string())
    }

    pub fn setup(&self) -> SessionSetup {
        SessionSetup {
            agent_folder: self.agent_folder.clone(),
            append_system_prompt_file: self.agent_folder.join("instructions.md"),
            plugin_dir: self.agent_folder.join("plugin"),
            plugin_name: "supplier-analyst".to_owned(),
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
}

pub fn models(key: Option<&str>) -> HarnessModelSettings {
    HarnessModelSettings {
        provider: ModelProvider(ModelProvider::DEFAULT.to_owned()),
        primary_model: "claude-sonnet-4-6".to_owned(),
        fallback_model: None,
        gateway_url: None,
        key: key.map(|k| OrganisationKey::new(k.to_owned())),
        environment: vec![],
    }
}

pub fn declared(name: &str, value: &str) -> HarnessEnvVar {
    HarnessEnvVar::new(name.to_owned(), value.to_owned())
}

// ---- the doubles ----

#[derive(Default)]
pub struct McpDouble {
    pub opened: Mutex<Vec<SessionGrant>>,
    pub closed: Mutex<Vec<RunId>>,
    pub error: Mutex<Option<McpError>>,
    pub servers: Mutex<Vec<&'static str>>,
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
        self.opened.lock().unwrap().push(grant.clone());
        let servers = self
            .servers
            .lock()
            .unwrap()
            .iter()
            .map(|name| McpServerLaunch {
                name: ServerName::new(*name).unwrap(),
                command: PathBuf::from("/opt/ordin/bin/ordin-hook"),
                args: vec!["mcp-bridge".to_owned()],
                env: BTreeMap::from([("ORDIN_MCP_TOKEN".to_owned(), "token-1".to_owned())]),
            })
            .collect();
        Ok(SessionMcpConfig { servers })
    }
    async fn close_session(&self, run_id: RunId) -> Result<(), McpError> {
        self.closed.lock().unwrap().push(run_id);
        Ok(())
    }
}

pub const HOOK_SECRET: &str = "s3cret-hook-value";

pub struct HookDouble {
    pub opened: Mutex<Vec<RunId>>,
    pub closed: Mutex<Vec<RunId>>,
    pub fail: Mutex<bool>,
    sender: broadcast::Sender<(RunId, HookSessionEvent)>,
}

impl Default for HookDouble {
    fn default() -> Self {
        HookDouble {
            opened: Mutex::default(),
            closed: Mutex::default(),
            fail: Mutex::new(false),
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
}

impl HookPort for HookDouble {
    fn open_session(&self, spec: HookSessionSpec) -> Result<HookSessionEnv, HookRelayError> {
        if *self.fail.lock().unwrap() {
            return Err(HookRelayError::Random);
        }
        self.opened.lock().unwrap().push(spec.run_id);
        Ok(HookSessionEnv {
            endpoint: "/tmp/hook.sock".to_owned(),
            secret: HOOK_SECRET.to_owned(),
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
}

pub enum RulesScript {
    Proceed,
    Decision(RunStartDecision),
    Unavailable,
}

pub struct RulesDouble {
    pub script: Mutex<RulesScript>,
    pub facts: Mutex<Vec<RunStartFacts>>,
    /// `adopted_run`'s reply; `None` answers a record with no scope and no subset.
    pub adopted: Mutex<Option<Result<Option<AdoptedRun>, ()>>>,
}

impl Default for RulesDouble {
    fn default() -> Self {
        RulesDouble {
            script: Mutex::new(RulesScript::Proceed),
            adopted: Mutex::new(None),
            facts: Mutex::default(),
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
        match &*self.script.lock().unwrap() {
            RulesScript::Proceed => Ok(proceed()),
            RulesScript::Decision(decision) => Ok(decision.clone()),
            RulesScript::Unavailable => Err(RulesError::RuleSetNotLoaded),
        }
    }
    async fn adopted_run(&self, run_id: RunId) -> Result<Option<AdoptedRun>, RulesError> {
        match &*self.adopted.lock().unwrap() {
            Some(Ok(record)) => Ok(record.clone()),
            Some(Err(())) => Err(RulesError::RuleSetNotLoaded),
            None => Ok(Some(AdoptedRun {
                agent_instance_id: ordin_types::AgentInstanceId(run_id.0),
                workspace_id: ordin_types::WorkspaceId(run_id.0),
                scope: None,
                tool_subset: None,
                single_call: false,
                limit_seconds: None,
            })),
        }
    }
}

pub struct SetupDouble {
    pub setup: Mutex<SessionSetup>,
    pub fail: Mutex<bool>,
    pub delay: Mutex<Option<Duration>>,
    /// The attachments of every call, in call order.
    pub attachments: Mutex<Vec<Vec<ordin_types::ArtefactId>>>,
}

#[async_trait::async_trait]
impl SessionSetupSource for SetupDouble {
    async fn session_setup(
        &self,
        _run_id: RunId,
        _agent_instance_id: AgentInstanceId,
        attachments: &[ordin_types::ArtefactId],
    ) -> Result<SessionSetup, SessionSetupError> {
        self.attachments.lock().unwrap().push(attachments.to_vec());
        let delay = *self.delay.lock().unwrap();
        if let Some(delay) = delay {
            tokio::time::sleep(delay).await;
        }
        if *self.fail.lock().unwrap() {
            return Err(SessionSetupError {
                code: "SUP_RUN_NOT_FOUND".to_owned(),
            });
        }
        Ok(self.setup.lock().unwrap().clone())
    }
}

#[derive(Default)]
pub struct RecordsDouble {
    pub saved: Mutex<HashMap<String, Vec<u8>>>,
    pub fail_save: Mutex<bool>,
    pub fail_load: Mutex<bool>,
}

#[async_trait::async_trait]
impl SessionRecordStore for RecordsDouble {
    async fn save(
        &self,
        _run_id: RunId,
        runtime_session_id: &str,
        record: Vec<u8>,
    ) -> Result<(), RecordError> {
        if *self.fail_save.lock().unwrap() {
            return Err(RecordError {
                code: "SUP_STORE_UNAVAILABLE",
                retryable: true,
            });
        }
        self.saved
            .lock()
            .unwrap()
            .insert(runtime_session_id.to_owned(), record);
        Ok(())
    }
    async fn load(
        &self,
        _run_id: RunId,
        runtime_session_id: &str,
    ) -> Result<Option<Vec<u8>>, RecordError> {
        if *self.fail_load.lock().unwrap() {
            return Err(RecordError {
                code: "SUP_RUN_NOT_FOUND",
                retryable: false,
            });
        }
        Ok(self.saved.lock().unwrap().get(runtime_session_id).cloned())
    }
}

pub const FAST: Timings = Timings {
    cli_check: Duration::from_secs(10),
    sign_in_poll: Duration::from_millis(30),
    sign_in_give_up: Duration::from_secs(600),
    idle_process: Duration::from_secs(600),
};

/// The doubles around one adapter.
pub struct Harness {
    pub fake: Fake,
    pub mcp: Arc<McpDouble>,
    pub hooks: Arc<HookDouble>,
    pub rules: Arc<RulesDouble>,
    pub setup: Arc<SetupDouble>,
    pub records: Arc<RecordsDouble>,
    pub parent: Arc<Mutex<HashMap<String, String>>>,
    pub timings: Timings,
}

pub fn run_of(req: &StartRun) -> RunHandle {
    RunHandle {
        run_id: req.run_id,
        adapter: ordin_runtime_adapter::AdapterKind::ClaudeLocal,
        runtime_session_id: None,
        remote_thread_id: None,
    }
}

impl Harness {
    pub fn new() -> Harness {
        let fake = Fake::new();
        let setup = fake.setup();
        Harness {
            fake,
            mcp: Arc::default(),
            hooks: Arc::default(),
            rules: Arc::default(),
            setup: Arc::new(SetupDouble {
                setup: Mutex::new(setup),
                fail: Mutex::new(false),
                delay: Mutex::new(None),
                attachments: Mutex::new(Vec::new()),
            }),
            records: Arc::default(),
            parent: Arc::new(Mutex::new(HashMap::from([
                ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
                ("HOME".to_owned(), "/home/asha".to_owned()),
            ]))),
            timings: FAST,
        }
    }

    pub fn host(&self) -> Host {
        let parent = self.parent.clone();
        let parent_env: ParentEnv =
            Arc::new(move |name: &str| parent.lock().unwrap().get(name).map(Into::into));
        Host {
            timings: self.timings,
            parent_env,
            projects_root: Some(self.fake.root().join("projects")),
        }
    }

    pub fn build(
        &self,
        config: ClaudeAdapterConfig,
    ) -> Result<ClaudeLocalAdapter, crate::ClaudeAdapterError> {
        let records: Option<Arc<dyn SessionRecordStore>> =
            (config.profile == ProgramProfile::AgentProfile).then(|| self.records.clone() as _);
        ClaudeLocalAdapter::build(
            config,
            self.mcp.clone(),
            self.hooks.clone(),
            self.rules.clone(),
            self.setup.clone(),
            records,
            self.host(),
        )
    }

    pub fn adapter(&self) -> ClaudeLocalAdapter {
        self.build(self.fake.config()).expect("the adapter builds")
    }

    pub fn agent_adapter(
        &self,
        models: HarnessModelSettings,
        route: KeyRoute,
    ) -> ClaudeLocalAdapter {
        self.build(self.fake.agent_config(models, route))
            .expect("the adapter builds")
    }

    pub fn edit_setup(&self, edit: impl FnOnce(&mut SessionSetup)) {
        edit(&mut self.setup.setup.lock().unwrap());
    }
}

pub fn ctx() -> AdapterContext {
    AdapterContext {
        correlation_id: "corr-1".to_owned(),
        deadline: Instant::now() + Duration::from_secs(60),
        cancel: CancellationToken::new(),
    }
}

pub fn command_request() -> StartRun {
    StartRun {
        run_id: RunId::new(),
        workspace_id: WorkspaceId::new(),
        target: RunTarget::AgentInstance {
            agent_instance_id: AgentInstanceId::new(),
        },
        input: StartInput::Command {
            name: "compare-quotes".to_owned(),
            arguments: "Kestrel, Acme and Northwind Q3 quotes".to_owned(),
        },
        attachments: vec![],
        limits: RunLimits {
            wall_clock_secs: 3600,
            request_budget: None,
            idle_timeout_secs: 600,
        },
        thread: None,
        model: None,
        thinking: None,
        caller: ToolCaller::Agent,
        scope_id: None,
        coding_task_id: None,
        started_by_person: false,
    }
}

/// Every event of the run, up to and including `Ended`.
pub async fn all_events(adapter: &ClaudeLocalAdapter, handle: &RunHandle) -> Vec<RunEvent> {
    let stream = adapter.events(&ctx(), handle).expect("the event stream");
    tokio::time::timeout(Duration::from_secs(20), stream.collect::<Vec<_>>())
        .await
        .expect("the stream ends")
}

/// Waits for an event that matches.
pub async fn wait_event(
    adapter: &ClaudeLocalAdapter,
    handle: &RunHandle,
    wanted: impl Fn(&RunEvent) -> bool,
) -> RunEvent {
    let mut stream = adapter.events(&ctx(), handle).expect("the event stream");
    tokio::time::timeout(Duration::from_secs(20), async {
        while let Some(event) = stream.next().await {
            if wanted(&event) {
                return event;
            }
        }
        panic!("the stream ended without the event");
    })
    .await
    .expect("the event arrives")
}

pub fn kinds(events: &[RunEvent]) -> Vec<&'static str> {
    events
        .iter()
        .map(|event| match event {
            RunEvent::Started { .. } => "started",
            RunEvent::Progress { .. } => "progress",
            RunEvent::HarnessPlan { .. } => "harness_plan",
            RunEvent::Paused { .. } => "paused",
            RunEvent::Resumed => "resumed",
            RunEvent::Usage { .. } => "usage",
            RunEvent::Ended { .. } => "ended",
        })
        .collect()
}

pub fn last_end(events: &[RunEvent]) -> ordin_runtime_adapter::RunEnd {
    match events.last() {
        Some(RunEvent::Ended { end }) => end.clone(),
        other => panic!("last event {other:?}"),
    }
}
