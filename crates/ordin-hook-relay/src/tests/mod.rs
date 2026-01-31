//! Unit tests of the Hook Relay. The Rules Engine, the Approval Service and the Session Supervisor
//! are scripted behind the crate's private ports so that a slow engine, an *ask*, a refused ending
//! or a failing Approval Service happen on demand; the Journal and the datastore are real (SQLite
//! on a temporary file). `tests/hook_integration.rs` runs the real services and the real hook.

mod acceptance;
mod error_table;
mod late;
mod logging;
mod pg;
mod socket;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hmac::{Hmac, Mac};
use ordin_approval::{ApprovalError, WaitOutcome};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, EntryFilter, Journal, JournalOptions, MigrationRegistry,
    SystemClock, MIGRATIONS,
};
use ordin_rules::{ApprovalWait, CallContext, EndDecision, RulesError, RulesOutcome};
use ordin_session_supervisor::{ResumeDecision, SupervisorError};
use ordin_types::domain::runs::ApprovalState;
use ordin_types::encoding::{b64url_decode_exact, b64url_encode, canonical_json};
use ordin_types::time::Timestamp;
use ordin_types::wire::hook::{HarnessKind, HookMessage, HookReply, HookRequest};
use ordin_types::wire::rules::{
    ActionProposal, RuleRef, RuleSource, ToolCall, ToolDecision, ToolOrigin, Verdict,
};
use ordin_types::{AgentInstanceId, ApprovalId, HookCallId, RunId, ToolRequestId, WorkspaceId};
use sha2::Sha256;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::watch;
use tokio::time::Instant;

use crate::consts::Timings;
use crate::ports::{ApprovalPort, Ports, RulesPort, SupervisorPort};
use crate::{
    AgentWriteError, AgentWrites, HookRelay, HookRelayConfig, HookSessionEnv, HookSessionSpec,
    WriteTarget,
};

/// A folder removed when dropped.
pub(crate) struct TempDir(pub PathBuf);

impl TempDir {
    pub(crate) fn new() -> TempDir {
        // A short name: a Unix socket path is limited to 104 bytes on macOS.
        let dir = std::env::temp_dir().join(format!(
            "hr-{}",
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

/// Shorter timings than the spec's, so that "50 seconds pass" takes a fraction of a second.
pub(crate) fn fast() -> Timings {
    Timings {
        read: Duration::from_millis(300),
        decision: Duration::from_millis(250),
        hold: Duration::from_millis(500),
        shutdown_grace: Duration::from_millis(300),
    }
}

// ---- scripted Rules Engine

#[derive(Clone)]
pub(crate) enum Decide {
    Allow,
    Deny(String),
    Ask(Option<ApprovalId>),
    Slow(Duration),
    Panic,
}

#[derive(Clone, Copy)]
pub(crate) enum Waited {
    Approved,
    Rejected,
    Expired,
    /// Waits until `hold` has passed, then answers `Pending` (the engine's deadline answer).
    UntilDeadline,
    Unavailable,
    State,
}

#[derive(Clone)]
pub(crate) enum EndScript {
    End,
    Continue(String),
    Ask(ApprovalId),
    Slow(Duration),
}

pub(crate) struct ScriptedRules {
    pub decide: Mutex<Decide>,
    /// How long `wait_for_approval` takes before it answers `waited`.
    pub wait_delay: Mutex<Duration>,
    pub waited: Mutex<Waited>,
    pub end: Mutex<EndScript>,
    pub calls: Mutex<Vec<(ToolCall, CallContext)>>,
    pub requests: Mutex<Vec<ToolRequestId>>,
    pub outcomes: Mutex<Vec<(ToolRequestId, bool)>>,
    pub settled: Mutex<Vec<(RunId, bool)>>,
    pub settle_approved: Mutex<EndDecision>,
    pub settle_rejected: Mutex<EndDecision>,
}

impl ScriptedRules {
    fn new() -> Arc<ScriptedRules> {
        Arc::new(ScriptedRules {
            decide: Mutex::new(Decide::Allow),
            wait_delay: Mutex::new(Duration::ZERO),
            waited: Mutex::new(Waited::Approved),
            end: Mutex::new(EndScript::End),
            calls: Mutex::new(vec![]),
            requests: Mutex::new(vec![]),
            outcomes: Mutex::new(vec![]),
            settled: Mutex::new(vec![]),
            settle_approved: Mutex::new(EndDecision::End),
            settle_rejected: Mutex::new(EndDecision::Continue {
                control: "judged-loop:draft".into(),
                message: "The run cannot end yet.".into(),
            }),
        })
    }

    pub(crate) fn set_decide(&self, decide: Decide) {
        *self.decide.lock().unwrap() = decide;
    }
}

fn rule() -> RuleRef {
    RuleRef {
        source: RuleSource::AppShellPolicy,
        rule_id: "app.shell".into(),
        agent_instance_id: None,
    }
}

#[async_trait::async_trait]
impl RulesPort for ScriptedRules {
    /// As the Rules Engine with the MCP Front serving "ordin": that server is ordin's.
    fn classify_origin(
        &self,
        origin: &ordin_types::wire::rules::ToolOrigin,
    ) -> ordin_types::wire::rules::ToolOrigin {
        use ordin_types::wire::rules::ToolOrigin;
        match origin {
            ToolOrigin::PackageMcpServer { server } if server == "ordin" => {
                ToolOrigin::OrdinMcpFront {
                    server: server.clone(),
                }
            }
            other => other.clone(),
        }
    }
    async fn decide_tool_call(&self, call: &ToolCall, ctx: &CallContext) -> RulesOutcome {
        self.calls.lock().unwrap().push((call.clone(), ctx.clone()));
        let request = ToolRequestId::new();
        self.requests.lock().unwrap().push(request);
        let script = self.decide.lock().unwrap().clone();
        let (verdict, approval_id) = match script {
            Decide::Allow => (Verdict::Allow, None),
            Decide::Deny(reason) => (Verdict::Deny { reason }, None),
            Decide::Ask(id) => (
                Verdict::Ask {
                    proposal: ActionProposal {
                        action: "Run a tool".into(),
                        destination: "This computer".into(),
                        affects: vec![],
                        consequence: "The action runs.".into(),
                    },
                },
                id,
            ),
            Decide::Slow(delay) => {
                tokio::time::sleep(delay).await;
                (Verdict::Allow, None)
            }
            Decide::Panic => panic!("scripted engine defect"),
        };
        RulesOutcome {
            decision: ToolDecision {
                verdict,
                rule: rule(),
                decided_at: Timestamp::now(),
            },
            tool_request_id: request,
            approval_id,
        }
    }

    async fn record_outcome(&self, tool_request_id: ToolRequestId, executed: bool) {
        self.outcomes
            .lock()
            .unwrap()
            .push((tool_request_id, executed));
    }

    async fn decide_run_end(&self, _run_id: RunId) -> EndDecision {
        let script = self.end.lock().unwrap().clone();
        match script {
            EndScript::End => EndDecision::End,
            EndScript::Continue(message) => EndDecision::Continue {
                control: "judged-loop:draft".into(),
                message,
            },
            EndScript::Ask(approval_id) => EndDecision::Ask {
                gates: vec!["judged-loop:draft".into()],
                approval_id,
            },
            EndScript::Slow(delay) => {
                tokio::time::sleep(delay).await;
                EndDecision::End
            }
        }
    }

    async fn settle_end_gates(&self, run_id: RunId, approved: bool) -> EndDecision {
        self.settled.lock().unwrap().push((run_id, approved));
        if approved {
            self.settle_approved.lock().unwrap().clone()
        } else {
            self.settle_rejected.lock().unwrap().clone()
        }
    }

    async fn wait_for_approval(
        &self,
        _approval_id: ApprovalId,
        until: Option<Instant>,
    ) -> Result<ApprovalWait, RulesError> {
        let delay = *self.wait_delay.lock().unwrap();
        let waited = *self.waited.lock().unwrap();
        match waited {
            Waited::UntilDeadline => {
                if let Some(until) = until {
                    tokio::time::sleep_until(until).await;
                }
                Ok(ApprovalWait::Pending)
            }
            other => {
                tokio::time::sleep(delay).await;
                match other {
                    Waited::Approved => Ok(ApprovalWait::Approved),
                    Waited::Rejected => Ok(ApprovalWait::Rejected),
                    Waited::Expired => Ok(ApprovalWait::Expired),
                    Waited::Unavailable => Err(RulesError::ApprovalUnavailable),
                    Waited::State => Err(RulesError::State(sqlx::Error::PoolClosed)),
                    Waited::UntilDeadline => Ok(ApprovalWait::Pending),
                }
            }
        }
    }
}

// ---- scripted Approval Service

pub(crate) struct ScriptedApprovals {
    states: Mutex<HashMap<ApprovalId, watch::Sender<ApprovalState>>>,
    pub unavailable: AtomicBool,
    pub waits: Mutex<Vec<ApprovalId>>,
}

impl ScriptedApprovals {
    fn new() -> Arc<ScriptedApprovals> {
        Arc::new(ScriptedApprovals {
            states: Mutex::new(HashMap::new()),
            unavailable: AtomicBool::new(false),
            waits: Mutex::new(vec![]),
        })
    }

    fn sender(&self, id: ApprovalId) -> watch::Sender<ApprovalState> {
        self.states
            .lock()
            .unwrap()
            .entry(id)
            .or_insert_with(|| watch::channel(ApprovalState::Pending).0)
            .clone()
    }

    /// The person decides.
    pub(crate) fn decide(&self, id: ApprovalId, state: ApprovalState) {
        self.sender(id).send_replace(state);
    }
}

#[async_trait::async_trait]
impl ApprovalPort for ScriptedApprovals {
    async fn wait(
        &self,
        approval_id: ApprovalId,
        deadline: Option<Instant>,
    ) -> Result<WaitOutcome, ApprovalError> {
        self.waits.lock().unwrap().push(approval_id);
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(ApprovalError::NotFound);
        }
        let mut receiver = self.sender(approval_id).subscribe();
        let decided = async {
            receiver
                .wait_for(|state| *state != ApprovalState::Pending)
                .await
                .map(|state| *state)
        };
        match deadline {
            Some(deadline) => tokio::select! {
                state = decided => state.map(WaitOutcome::Decided).map_err(|_| ApprovalError::NotFound),
                _ = tokio::time::sleep_until(deadline) => Ok(WaitOutcome::DeadlinePassed),
            },
            None => decided
                .await
                .map(WaitOutcome::Decided)
                .map_err(|_| ApprovalError::NotFound),
        }
    }
}

// ---- scripted File Service claims

pub(crate) struct ScriptedWrites {
    pub opened: Mutex<Vec<(String, WriteTarget)>>,
    pub settled: Mutex<Vec<String>>,
    pub ended: Mutex<Vec<RunId>>,
    pub fail_open: AtomicBool,
    pub hang_open: AtomicBool,
}

impl ScriptedWrites {
    fn new() -> Arc<ScriptedWrites> {
        Arc::new(ScriptedWrites {
            opened: Mutex::new(vec![]),
            settled: Mutex::new(vec![]),
            ended: Mutex::new(vec![]),
            fail_open: AtomicBool::new(false),
            hang_open: AtomicBool::new(false),
        })
    }
}

#[async_trait::async_trait]
impl AgentWrites for ScriptedWrites {
    async fn open(
        &self,
        _: RunId,
        tool: &str,
        target: WriteTarget,
    ) -> Result<Option<String>, AgentWriteError> {
        if self.hang_open.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        if self.fail_open.load(Ordering::SeqCst) {
            return Err(AgentWriteError);
        }
        let claim = matches!(target, WriteTarget::File(_)).then(|| "claim".to_owned());
        self.opened.lock().unwrap().push((tool.to_owned(), target));
        Ok(claim)
    }
    async fn settle(&self, claim: &str) -> Result<(), AgentWriteError> {
        self.settled.lock().unwrap().push(claim.to_owned());
        Ok(())
    }
    async fn run_ended(&self, run_id: RunId) -> Result<(), AgentWriteError> {
        self.ended.lock().unwrap().push(run_id);
        Ok(())
    }
}

// ---- scripted Session Supervisor

pub(crate) struct ScriptedSupervisor {
    pub live: Mutex<HashSet<RunId>>,
    pub resumes: Mutex<Vec<(RunId, ResumeDecision)>>,
    pub fail_resume: AtomicBool,
}

impl ScriptedSupervisor {
    fn new() -> Arc<ScriptedSupervisor> {
        Arc::new(ScriptedSupervisor {
            live: Mutex::new(HashSet::new()),
            resumes: Mutex::new(vec![]),
            fail_resume: AtomicBool::new(false),
        })
    }
}

#[async_trait::async_trait]
impl SupervisorPort for ScriptedSupervisor {
    fn is_live(&self, run_id: RunId) -> bool {
        self.live.lock().unwrap().contains(&run_id)
    }

    async fn resume_with_decision(
        &self,
        run_id: RunId,
        decision: ResumeDecision,
    ) -> Result<(), SupervisorError> {
        if self.fail_resume.load(Ordering::SeqCst) {
            return Err(SupervisorError::RunEnded);
        }
        self.resumes.lock().unwrap().push((run_id, decision));
        Ok(())
    }
}

// ---- the rig

/// One session as the adapter opened it.
pub(crate) struct Opened {
    pub env: HookSessionEnv,
    pub run_id: RunId,
    pub agent_instance_id: AgentInstanceId,
    pub workspace_id: WorkspaceId,
    pub secret: [u8; 32],
}

pub(crate) struct Rig {
    pub dir: TempDir,
    pub store: Datastore,
    pub journal: Arc<Journal>,
    pub rules: Arc<ScriptedRules>,
    pub approvals: Arc<ScriptedApprovals>,
    pub supervisor: Arc<ScriptedSupervisor>,
    pub writes: Arc<ScriptedWrites>,
    pub relay: Arc<HookRelay>,
}

pub(crate) async fn open_store(dir: &TempDir) -> (Datastore, Arc<Journal>) {
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.0.join("data").join("journal.db"),
        backup_dir: dir.0.join("data").join("backups"),
    })
    .await
    .expect("open the datastore");
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).expect("journal");
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
    (store, journal)
}

impl Rig {
    pub(crate) async fn new() -> Rig {
        Rig::with_timings(fast()).await
    }

    pub(crate) async fn with_timings(timings: Timings) -> Rig {
        let dir = TempDir::new();
        let (store, journal) = open_store(&dir).await;
        Rig::over(
            dir,
            store,
            journal,
            timings,
            ScriptedApprovals::new(),
            ScriptedSupervisor::new(),
        )
    }

    /// A relay over an existing store (a restart).
    pub(crate) fn over(
        dir: TempDir,
        store: Datastore,
        journal: Arc<Journal>,
        timings: Timings,
        approvals: Arc<ScriptedApprovals>,
        supervisor: Arc<ScriptedSupervisor>,
    ) -> Rig {
        let rules = ScriptedRules::new();
        let writes = ScriptedWrites::new();
        let relay = HookRelay::build(
            store.clone(),
            journal.clone(),
            Ports {
                rules: rules.clone(),
                approvals: approvals.clone(),
                supervisor: supervisor.clone(),
                writes: writes.clone(),
            },
            HookRelayConfig {
                app_data_root: dir.0.clone(),
                installation_id: "test-installation".into(),
            },
            timings,
        );
        Rig {
            dir,
            store,
            journal,
            rules,
            approvals,
            supervisor,
            writes,
            relay,
        }
    }

    /// Opens a session for a new run and marks the run live in the supervisor.
    pub(crate) fn open(&self, harness: HarnessKind) -> Opened {
        let spec = HookSessionSpec {
            run_id: RunId::new(),
            agent_instance_id: AgentInstanceId::new(),
            workspace_id: WorkspaceId::new(),
            harness,
        };
        let opened = self.open_spec(spec);
        self.supervisor.live.lock().unwrap().insert(opened.run_id);
        opened
    }

    pub(crate) fn open_spec(&self, spec: HookSessionSpec) -> Opened {
        let env = self
            .relay
            .open_session(spec.clone())
            .expect("open the session");
        let secret: [u8; 32] = b64url_decode_exact(&env.secret, 32, "TEST")
            .expect("a 32-byte secret")
            .try_into()
            .expect("32 bytes");
        Opened {
            env,
            run_id: spec.run_id,
            agent_instance_id: spec.agent_instance_id,
            workspace_id: spec.workspace_id,
            secret,
        }
    }

    /// Sends `bytes` on a new in-memory connection and returns the reply lines until the relay closes it.
    pub(crate) async fn send(&self, bytes: &[u8]) -> Vec<HookReply> {
        self.send_as(bytes, true).await
    }

    pub(crate) async fn send_as(&self, bytes: &[u8], peer_allowed: bool) -> Vec<HookReply> {
        let (mut client, server) = tokio::io::duplex(1 << 20);
        let relay = self.relay.clone();
        let served = tokio::spawn(async move { relay.serve(server, peer_allowed).await });
        client.write_all(bytes).await.expect("write the request");
        let replies = read_replies(client).await;
        let _ = served.await;
        replies
    }

    /// A signed request line for `message`, with a fresh call id.
    pub(crate) fn line(&self, opened: &Opened, message: HookMessage) -> (HookCallId, Vec<u8>) {
        let call_id = HookCallId::new();
        let request = signed(opened, call_id, message);
        let mut bytes = serde_json::to_vec(&request).expect("serialise");
        bytes.push(b'\n');
        (call_id, bytes)
    }

    pub(crate) async fn call(&self, opened: &Opened, message: HookMessage) -> Vec<HookReply> {
        let (_, bytes) = self.line(opened, message);
        self.send(&bytes).await
    }

    pub(crate) async fn refusals(&self) -> Vec<serde_json::Value> {
        self.journal
            .list_entries(
                EntryFilter {
                    run_id: None,
                    kind: Some("hook_refused".into()),
                },
                None,
            )
            .await
            .expect("list")
            .items
            .into_iter()
            .map(|e| e.data)
            .collect()
    }

    pub(crate) async fn entries(&self, kind: &str) -> Vec<ordin_journal::JournalEntry> {
        self.journal
            .list_entries(
                EntryFilter {
                    run_id: None,
                    kind: Some(kind.into()),
                },
                None,
            )
            .await
            .expect("list")
            .items
    }

    pub(crate) async fn late_rows(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM hook_late_decisions")
            .fetch_one(self.store.sqlite().expect("SQLite"))
            .await
            .expect("count")
    }
}

/// Reads reply lines until the stream ends.
pub(crate) async fn read_replies<R: tokio::io::AsyncRead + Unpin>(client: R) -> Vec<HookReply> {
    let mut lines = BufReader::new(client).lines();
    let mut replies = vec![];
    while let Some(line) = lines.next_line().await.expect("read a reply line") {
        replies.push(reply_of(&line));
    }
    replies
}

/// Reads one reply line. A deny that answers an unreadable request carries the nil call id, which
/// `HookCallId` refuses to decode (it takes version-7 UUIDs only), so a deny is read by hand.
pub(crate) fn reply_of(line: &str) -> HookReply {
    let value: serde_json::Value =
        serde_json::from_str(line).unwrap_or_else(|e| panic!("reply {line}: {e}"));
    if value["kind"] == "deny" {
        let call_id =
            uuid::Uuid::parse_str(value["call_id"].as_str().expect("call_id")).expect("uuid");
        return HookReply::Deny {
            call_id: HookCallId(call_id),
            reason: value["reason"].as_str().expect("reason").to_owned(),
        };
    }
    serde_json::from_value(value).unwrap_or_else(|e| panic!("reply {line}: {e}"))
}

/// The MAC the ordin hook computes: HMAC-SHA256 over the canonical JSON of `message`.
pub(crate) fn mac_of(secret: &[u8; 32], message: &HookMessage) -> String {
    let value = serde_json::to_value(message).expect("message");
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret).expect("key");
    mac.update(&canonical_json(&value));
    b64url_encode(&mac.finalize().into_bytes())
}

pub(crate) fn signed(opened: &Opened, call_id: HookCallId, message: HookMessage) -> HookRequest {
    HookRequest {
        protocol: 1,
        call_id,
        session: opened.run_id,
        agent_instance_id: opened.agent_instance_id,
        mac: mac_of(&opened.secret, &message),
        message,
    }
}

pub(crate) fn pre(tool: &str, id: Option<&str>) -> HookMessage {
    HookMessage::PreToolUse {
        harness: HarnessKind::ClaudeCode,
        tool: tool.into(),
        origin: ToolOrigin::BuiltIn,
        arguments: serde_json::json!({"file_path": "a.txt"}),
        harness_tool_use_id: id.map(str::to_owned),
        subagent: None,
    }
}

pub(crate) fn post(tool: &str, id: Option<&str>, succeeded: bool) -> HookMessage {
    HookMessage::PostToolUse {
        harness: HarnessKind::ClaudeCode,
        tool: tool.into(),
        harness_tool_use_id: id.map(str::to_owned),
        succeeded,
    }
}

pub(crate) fn stop() -> HookMessage {
    HookMessage::Stop {
        harness: HarnessKind::ClaudeCode,
    }
}

/// The deny reason, panicking on any other reply.
pub(crate) fn deny_reason(reply: &HookReply) -> &str {
    match reply {
        HookReply::Deny { reason, .. } => reason,
        other => panic!("expected a deny, got {other:?}"),
    }
}

/// Polls `check` for up to 5 s.
pub(crate) async fn eventually(what: &str, mut check: impl FnMut() -> bool) {
    for _ in 0..250 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("never: {what}");
}

/// Waits until the late-decision table holds `n` rows.
pub(crate) async fn wait_for_rows(rig: &Rig, n: i64) {
    for _ in 0..250 {
        if rig.late_rows().await == n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the late-decision table never held {n} rows");
}

/// The queue of events a session stream has yielded so far, within `wait`.
pub(crate) async fn events_within(
    stream: &mut futures::stream::BoxStream<'static, crate::HookSessionEvent>,
    n: usize,
    wait: Duration,
) -> Vec<serde_json::Value> {
    use futures::StreamExt;
    let mut out = VecDeque::new();
    let _ = tokio::time::timeout(wait, async {
        while out.len() < n {
            match stream.next().await {
                Some(event) => out.push_back(serde_json::to_value(&event).expect("event")),
                None => break,
            }
        }
    })
    .await;
    out.into_iter().collect()
}
