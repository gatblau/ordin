//! Shared helpers of this crate's tests: a migrated SQLite datastore with the real Journal,
//! Approval Service, Rules Engine, Usage Planner and Session Supervisor; a channel-driven adapter
//! whose runs follow a script; a recording `CrewHost`; and a scriptable `Coordinator`.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::stream::{BoxStream, StreamExt};
use ordin_approval::{ApprovalConfig, ApprovalService, PressOrigin};
use ordin_crew_orchestrator::{
    AgentAvailability, AnsweredBy, CheckVerdict, Coordinator, CoordinatorError, CoordinatorTurn,
    CrewHost, CrewInput, CrewInputValue, CrewOrchestrator, CrewOutcome, CrewStart, CrewToolCall,
    CrewView, MemberAnswer, PathDecision, PersonQuestion, StepOutcome, TakeOverReason,
};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, EntryFilter, Journal, JournalOptions, MigrationRegistry,
    SystemClock, MIGRATIONS,
};
use ordin_organisation_profile::{Clock, OrganisationProfileService, ProfileMode, ProfileOptions};
use ordin_rules::{
    CardWording, ControlLine, FactError, MessageRule, ProgramKind as RulesProgram, Proposal,
    RulesConfig, RulesEngine, RulesHost, RunKey,
};
use ordin_runtime_adapter::{
    check_verb, AdapterContext, AdapterError, AdapterKind, AdapterVerb, ResumeDecision, RunEnd,
    RunEndReason, RunEvent, RunHandle, RunSnapshot, RuntimeAdapter, StartRun, UsageDelta,
};
use ordin_session_supervisor::{
    ContentCheck, ContentPoint, HostError, ProgramKind, RunControls, SessionSupervisor,
    SupervisorConfig, SupervisorHost,
};
use ordin_types::common::AgentRef;
use ordin_types::domain::agent_profile::{CallPlace, ModelFailureCause};
use ordin_types::domain::runs::{
    AgentStep, CoordinatorKind, CrewCharter, CrewCharterState, CrewMember, CrewRun, CrewRunState,
    Guardrails, Handoff, IfItFails, Run, StepUse, WorkflowStep,
};
use ordin_types::time::Timestamp;
use ordin_types::wire::approval::AffectedItem;
use ordin_types::wire::rules::ActionProposal;
use ordin_types::wire::startup::StartUpReportItem;
use ordin_types::{
    AgentInstanceId, ArtefactId, BackendId, CrewCharterId, CrewRunId, RemoteAgentId, RunId,
    WorkflowId, WorkspaceId,
};
use ordin_usage_planner::UsagePlanner;
use tokio::sync::{mpsc, Notify};

/// A folder removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-crew-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Now;

impl Clock for Now {
    fn now(&self) -> ordin_organisation_profile::Timestamp {
        ordin_organisation_profile::Timestamp::now()
    }
}

// ---- the scripted adapter

/// One step of a run's script.
#[derive(Clone)]
pub enum Act {
    Event(RunEvent),
    Sleep(u64),
    /// Waits until the supervisor resumes the run.
    AwaitResume,
    /// Waits until the run is cancelled.
    Hang,
}

pub type Plan = Vec<Act>;

pub fn started() -> Act {
    Act::Event(RunEvent::Started {
        runtime_session_id: Some("session-1".into()),
        remote_thread_id: Some("thread-1".into()),
    })
}

pub fn usage(turns: u32) -> Act {
    Act::Event(RunEvent::Usage {
        usage: UsageDelta {
            model_turns: turns,
            ..UsageDelta::default()
        },
    })
}

pub fn ended(reason: RunEndReason, message: Option<&str>, artefacts: Vec<ArtefactId>) -> Act {
    Act::Event(RunEvent::Ended {
        end: RunEnd {
            reason,
            final_message: message.map(str::to_owned),
            artefacts,
        },
    })
}

/// Completes with `message` after one model turn.
pub fn done(message: &str) -> Plan {
    vec![
        started(),
        usage(1),
        ended(RunEndReason::Completed, Some(message), vec![]),
    ]
}

pub fn done_with(message: &str, artefacts: Vec<ArtefactId>) -> Plan {
    vec![
        started(),
        usage(1),
        ended(RunEndReason::Completed, Some(message), artefacts),
    ]
}

pub fn fails() -> Plan {
    vec![
        started(),
        ended(RunEndReason::Failed, Some("It failed."), vec![]),
    ]
}

pub fn hangs() -> Plan {
    vec![started(), Act::Hang]
}

pub fn slow(ms: u64, message: &str) -> Plan {
    vec![
        started(),
        Act::Sleep(ms),
        ended(RunEndReason::Completed, Some(message), vec![]),
    ]
}

/// Pauses with a question, waits for the answer, then completes.
pub fn asks(question: &str) -> Plan {
    vec![
        started(),
        Act::Event(RunEvent::Paused {
            reason: ordin_runtime_adapter::PauseReason::Person {
                question: question.to_owned(),
            },
        }),
        Act::AwaitResume,
        Act::Event(RunEvent::Resumed),
        ended(RunEndReason::Completed, Some("Answered."), vec![]),
    ]
}

type Behaviour = Box<dyn FnMut(&StartRun, usize) -> Plan + Send>;

struct Channel {
    tx: mpsc::UnboundedSender<RunEvent>,
    rx: Option<mpsc::UnboundedReceiver<RunEvent>>,
    resume: Arc<Notify>,
    cancel: Arc<Notify>,
}

/// A channel-driven adapter: every started run plays the plan `behave` returns for it.
pub struct FakeAdapter {
    kind: AdapterKind,
    unsupported: &'static [AdapterVerb],
    behave: Mutex<Behaviour>,
    pub starts: Mutex<Vec<StartRun>>,
    pub resumes: Mutex<Vec<(RunId, ResumeDecision)>>,
    pub cancels: Mutex<Vec<RunId>>,
    running: Arc<AtomicUsize>,
    pub peak: Arc<AtomicUsize>,
    channels: Mutex<HashMap<RunId, Channel>>,
}

impl FakeAdapter {
    pub fn new(kind: AdapterKind, unsupported: &'static [AdapterVerb]) -> Arc<FakeAdapter> {
        Arc::new(FakeAdapter {
            kind,
            unsupported,
            behave: Mutex::new(Box::new(|_, _| done("Done."))),
            starts: Mutex::new(vec![]),
            resumes: Mutex::new(vec![]),
            cancels: Mutex::new(vec![]),
            running: Arc::new(AtomicUsize::new(0)),
            peak: Arc::new(AtomicUsize::new(0)),
            channels: Mutex::new(HashMap::new()),
        })
    }

    pub fn behave(&self, f: impl FnMut(&StartRun, usize) -> Plan + Send + 'static) {
        *self.behave.lock().unwrap() = Box::new(f);
    }

    pub fn start_count(&self) -> usize {
        self.starts.lock().unwrap().len()
    }

    pub fn started_runs(&self) -> Vec<RunId> {
        self.starts
            .lock()
            .unwrap()
            .iter()
            .map(|s| s.run_id)
            .collect()
    }

    pub fn peak(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }

    /// Sends one event on a run's stream, as the harness would.
    pub fn push(&self, run_id: RunId, event: RunEvent) {
        if let Some(channel) = self.channels.lock().unwrap().get(&run_id) {
            let _ = channel.tx.send(event);
            channel.cancel.notify_one();
        }
    }
}

#[async_trait::async_trait]
impl RuntimeAdapter for FakeAdapter {
    fn kind(&self) -> AdapterKind {
        self.kind
    }

    fn unsupported_verbs(&self) -> &'static [AdapterVerb] {
        self.unsupported
    }

    async fn start(&self, _ctx: &AdapterContext, req: StartRun) -> Result<RunHandle, AdapterError> {
        check_verb(self, AdapterVerb::Start)?;
        let nth = self.starts.lock().unwrap().len();
        let plan = (self.behave.lock().unwrap())(&req, nth);
        let (tx, rx) = mpsc::unbounded_channel();
        let resume = Arc::new(Notify::new());
        let cancel = Arc::new(Notify::new());
        self.channels.lock().unwrap().insert(
            req.run_id,
            Channel {
                tx: tx.clone(),
                rx: Some(rx),
                resume: resume.clone(),
                cancel: cancel.clone(),
            },
        );
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        let run_id = req.run_id;
        self.starts.lock().unwrap().push(req);
        let running = self.running.clone();
        // The script is played by its own task; any way out of it lowers the running count.
        tokio::spawn(async move {
            for act in plan {
                match act {
                    Act::Event(event) => {
                        let is_end = matches!(event, RunEvent::Ended { .. });
                        let _ = tx.send(event);
                        if is_end {
                            break;
                        }
                    }
                    Act::Sleep(ms) => {
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_millis(ms)) => {}
                            _ = cancel.notified() => break,
                        }
                    }
                    Act::AwaitResume => {
                        tokio::select! {
                            _ = resume.notified() => {}
                            _ = cancel.notified() => break,
                        }
                    }
                    Act::Hang => {
                        cancel.notified().await;
                        break;
                    }
                }
            }
            running.fetch_sub(1, Ordering::SeqCst);
        });
        Ok(RunHandle {
            run_id,
            adapter: self.kind,
            runtime_session_id: Some("session-1".into()),
            remote_thread_id: None,
        })
    }

    fn events(
        &self,
        _ctx: &AdapterContext,
        run: &RunHandle,
    ) -> Result<BoxStream<'static, RunEvent>, AdapterError> {
        let mut channels = self.channels.lock().unwrap();
        let channel = channels
            .get_mut(&run.run_id)
            .ok_or(AdapterError::UnknownRun)?;
        let rx = channel.rx.take().ok_or(AdapterError::UnknownRun)?;
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
        check_verb(self, AdapterVerb::Resume)?;
        self.resumes.lock().unwrap().push((run.run_id, decision));
        if let Some(channel) = self.channels.lock().unwrap().get(&run.run_id) {
            channel.resume.notify_one();
        }
        Ok(())
    }

    async fn read(
        &self,
        _ctx: &AdapterContext,
        _run: &RunHandle,
    ) -> Result<RunSnapshot, AdapterError> {
        Err(AdapterError::UnknownRun)
    }

    async fn cancel(&self, _ctx: &AdapterContext, run: &RunHandle) -> Result<(), AdapterError> {
        check_verb(self, AdapterVerb::Cancel)?;
        self.cancels.lock().unwrap().push(run.run_id);
        if let Some(channel) = self.channels.lock().unwrap().get(&run.run_id) {
            channel.cancel.notify_one();
            let _ = channel.tx.send(RunEvent::Ended {
                end: RunEnd {
                    reason: RunEndReason::Cancelled,
                    final_message: None,
                    artefacts: vec![],
                },
            });
        }
        Ok(())
    }
}

// ---- the hosts

/// The Session Supervisor's host: nothing to word, routes for remote agents set by the test.
pub struct SupHost {
    pub routes: Mutex<HashMap<RemoteAgentId, AdapterKind>>,
}

#[async_trait::async_trait]
impl SupervisorHost for SupHost {
    async fn startup_items(
        &self,
        _run: &Run,
        _target: &ordin_runtime_adapter::RunTarget,
        _unsupported: &[AdapterVerb],
    ) -> Vec<StartUpReportItem> {
        vec![]
    }
    fn remote_route(&self, remote_agent_id: RemoteAgentId) -> Option<AdapterKind> {
        self.routes.lock().unwrap().get(&remote_agent_id).copied()
    }
    async fn expand_command(
        &self,
        _target: &ordin_runtime_adapter::RunTarget,
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
    fn retry_after_block(&self, _carried: bool, _already: bool) -> bool {
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

/// The Rules Engine's host: no file facts, plain words.
pub struct RulesStub;

fn proposal() -> ActionProposal {
    ActionProposal {
        action: "Run a tool".into(),
        destination: "This computer".into(),
        affects: vec![AffectedItem::Record {
            label: "The run's work".into(),
        }],
        consequence: "The action runs.".into(),
    }
}

#[async_trait::async_trait]
impl RulesHost for RulesStub {
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
        wording("Allow this action?")
    }
    fn plain_line(&self, line: &ControlLine) -> String {
        format!("{} applies.", line.name)
    }
}

pub fn wording(question: &str) -> CardWording {
    CardWording {
        question: question.to_owned(),
        verb: "Start".into(),
        where_line: "This computer".into(),
        affects: vec![AffectedItem::Record {
            label: "The crew's work".into(),
        }],
        changes: "The step runs.".into(),
        content_label: None,
        proposal: proposal(),
    }
}

/// The `CrewHost` of the tests: what the Package Catalogue, the Ordin Host and the File Service
/// would answer, set by the test.
pub struct TestHost {
    pub missing: Mutex<HashSet<AgentRef>>,
    pub versions: Mutex<HashMap<AgentRef, String>>,
    pub saved: Mutex<Vec<(AgentRef, String)>>,
    pub workflows: Mutex<HashMap<WorkflowId, Vec<WorkflowStep>>>,
    pub changed: Mutex<bool>,
    pub routes: Mutex<HashMap<RemoteAgentId, AdapterKind>>,
    pub cards: Mutex<Vec<String>>,
}

impl TestHost {
    pub fn new() -> Arc<TestHost> {
        Arc::new(TestHost {
            missing: Mutex::new(HashSet::new()),
            versions: Mutex::new(HashMap::new()),
            saved: Mutex::new(vec![]),
            workflows: Mutex::new(HashMap::new()),
            changed: Mutex::new(false),
            routes: Mutex::new(HashMap::new()),
            cards: Mutex::new(vec![]),
        })
    }
}

#[async_trait::async_trait]
impl CrewHost for TestHost {
    async fn agent_status(
        &self,
        _workspace_id: WorkspaceId,
        member: &AgentRef,
    ) -> AgentAvailability {
        if self.missing.lock().unwrap().contains(member) {
            return AgentAvailability::Missing;
        }
        let version = self
            .versions
            .lock()
            .unwrap()
            .get(member)
            .cloned()
            .unwrap_or_else(|| "1".to_owned());
        AgentAvailability::Present { version }
    }
    async fn saved_agent_versions(
        &self,
        _workflow_id: WorkflowId,
        _version: u32,
    ) -> Vec<(AgentRef, String)> {
        self.saved.lock().unwrap().clone()
    }
    async fn workflow_steps(
        &self,
        _workspace_id: WorkspaceId,
        workflow_id: WorkflowId,
    ) -> Option<Vec<WorkflowStep>> {
        self.workflows.lock().unwrap().get(&workflow_id).cloned()
    }
    async fn document_changed(&self, _workspace_id: WorkspaceId, _input: &CrewInput) -> bool {
        *self.changed.lock().unwrap()
    }
    fn word_step_approval(
        &self,
        _view: &CrewView,
        step_label: &str,
        _member: &AgentRef,
        line: &str,
    ) -> CardWording {
        self.cards.lock().unwrap().push(step_label.to_owned());
        wording(&format!("Start step {step_label}: {line}?"))
    }
    async fn startup_items(&self, _charter: &CrewCharter) -> Vec<StartUpReportItem> {
        vec![StartUpReportItem::EnforcedRule {
            rule: ordin_types::wire::rules::RuleRef {
                source: ordin_types::wire::rules::RuleSource::AppApprovalPolicy,
                rule_id: "app.crew".into(),
                agent_instance_id: None,
            },
            plain: "Members ask before anything leaves the company.".into(),
        }]
    }
    fn remote_route(&self, remote_agent_id: RemoteAgentId) -> Option<AdapterKind> {
        self.routes.lock().unwrap().get(&remote_agent_id).copied()
    }
}

// ---- the coordinator

pub type RoundFn = Box<dyn FnMut(&CrewView) -> Result<CoordinatorTurn, CoordinatorError> + Send>;

/// A scriptable Ordin. Every call it receives is recorded by name in `calls`.
pub struct TestCoordinator {
    pub round_fn: Mutex<RoundFn>,
    pub calls: Mutex<Vec<String>>,
    pub views: Mutex<Vec<CrewView>>,
    pub verdicts: Mutex<VecDeque<CheckVerdict>>,
    pub path: Mutex<String>,
    pub paths_asked: Mutex<Vec<(String, Vec<String>)>>,
    pub person: Mutex<VecDeque<String>>,
    pub questions: Mutex<Vec<PersonQuestion>>,
    pub answer: Mutex<String>,
    pub member_questions: Mutex<Vec<String>>,
    pub takeovers: Mutex<Vec<(String, TakeOverReason)>>,
    pub outcomes: Mutex<Vec<CrewOutcome>>,
    /// When set, `round` never answers: the crew is in the middle of a round.
    pub stall: AtomicBool,
    /// Lets a stalled `round` answer.
    pub release: Notify,
    /// `round` stalls once it has been asked this many times.
    pub stall_after: Mutex<Option<usize>>,
    /// When set, Ordin's answer to a member's question cannot be used.
    pub answer_unusable: AtomicBool,
}

impl TestCoordinator {
    pub fn new() -> Arc<TestCoordinator> {
        Arc::new(TestCoordinator {
            round_fn: Mutex::new(Box::new(|_| Err(CoordinatorError::Unavailable))),
            calls: Mutex::new(vec![]),
            views: Mutex::new(vec![]),
            verdicts: Mutex::new(VecDeque::new()),
            path: Mutex::new("Yes".into()),
            paths_asked: Mutex::new(vec![]),
            person: Mutex::new(VecDeque::new()),
            questions: Mutex::new(vec![]),
            answer: Mutex::new("Use the second quote.".into()),
            member_questions: Mutex::new(vec![]),
            takeovers: Mutex::new(vec![]),
            outcomes: Mutex::new(vec![]),
            stall: AtomicBool::new(false),
            release: Notify::new(),
            stall_after: Mutex::new(None),
            answer_unusable: AtomicBool::new(false),
        })
    }

    /// The coordinator answers each round with the next list of calls, then `finish`.
    pub fn scripted(rounds: Vec<Vec<CrewToolCall>>) -> Arc<TestCoordinator> {
        let c = TestCoordinator::new();
        let mut queue: VecDeque<Vec<CrewToolCall>> = rounds.into();
        *c.round_fn.lock().unwrap() = Box::new(move |_| {
            let calls = queue.pop_front().unwrap_or_else(|| {
                vec![CrewToolCall::Finish {
                    summary: "Done.".into(),
                }]
            });
            Ok(CoordinatorTurn { calls, turns: 1 })
        });
        c
    }

    /// Like `scripted`, but once the rounds are used up the coordinator never answers: the crew is
    /// left in the middle of its next round, as a crash would leave it.
    pub fn scripted_then_stall(rounds: Vec<Vec<CrewToolCall>>) -> Arc<TestCoordinator> {
        let n = rounds.len();
        let c = TestCoordinator::scripted(rounds);
        *c.stall_after.lock().unwrap() = Some(n);
        c
    }

    pub fn names(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    pub fn count(&self, name: &str) -> usize {
        self.names().iter().filter(|n| n.starts_with(name)).count()
    }

    fn note(&self, name: impl Into<String>, view: &CrewView) {
        self.calls.lock().unwrap().push(name.into());
        self.views.lock().unwrap().push(view.clone());
    }
}

#[async_trait::async_trait]
impl Coordinator for TestCoordinator {
    async fn round(&self, view: &CrewView) -> Result<CoordinatorTurn, CoordinatorError> {
        self.note("round", view);
        let asked = self.count("round");
        let late = self.stall_after.lock().unwrap().is_some_and(|n| asked > n);
        if late || self.stall.load(Ordering::SeqCst) {
            self.release.notified().await;
        }
        (self.round_fn.lock().unwrap())(view)
    }
    async fn check_result(
        &self,
        view: &CrewView,
        step: &StepOutcome,
    ) -> Result<CheckVerdict, CoordinatorError> {
        self.note(format!("check_result:{}", step.label), view);
        if self.stall.load(Ordering::SeqCst) {
            self.release.notified().await;
        }
        Ok(self
            .verdicts
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(CheckVerdict::CarryOn { turns: 1 }))
    }
    async fn decide_condition(
        &self,
        view: &CrewView,
        words: &str,
        paths: &[String],
    ) -> Result<PathDecision, CoordinatorError> {
        self.note("decide_condition", view);
        self.paths_asked
            .lock()
            .unwrap()
            .push((words.to_owned(), paths.to_vec()));
        Ok(PathDecision {
            path: self.path.lock().unwrap().clone(),
            reason: "Kestrel's rate rose 7 per cent".into(),
            turns: 1,
        })
    }
    async fn answer_member(
        &self,
        view: &CrewView,
        _member: &AgentRef,
        question: &str,
    ) -> Result<MemberAnswer, CoordinatorError> {
        self.note("answer_member", view);
        self.member_questions
            .lock()
            .unwrap()
            .push(question.to_owned());
        if self.answer_unusable.load(Ordering::SeqCst) {
            return Err(CoordinatorError::Unusable("no answer".into()));
        }
        Ok(MemberAnswer {
            text: self.answer.lock().unwrap().clone(),
            answered_by: AnsweredBy::Ordin,
            turns: 1,
        })
    }
    async fn ask_person(
        &self,
        view: &CrewView,
        question: PersonQuestion,
    ) -> Result<String, CoordinatorError> {
        self.note(format!("ask_person:{:?}", question.kind), view);
        let choice = self
            .person
            .lock()
            .unwrap()
            .pop_front()
            .or_else(|| question.choices.first().cloned())
            .unwrap_or_default();
        self.questions.lock().unwrap().push(question);
        Ok(choice)
    }
    async fn take_over(
        &self,
        view: &CrewView,
        from_position: String,
        why: TakeOverReason,
    ) -> Result<(), CoordinatorError> {
        self.note("take_over", view);
        self.takeovers.lock().unwrap().push((from_position, why));
        Ok(())
    }
    async fn crew_ended(
        &self,
        view: &CrewView,
        outcome: &CrewOutcome,
    ) -> Result<(), CoordinatorError> {
        self.note("crew_ended", view);
        self.outcomes.lock().unwrap().push(outcome.clone());
        Ok(())
    }
}

// ---- the harness

pub struct Harness {
    _dir: TempDir,
    pub store: Datastore,
    pub journal: Arc<Journal>,
    pub approvals: Arc<ApprovalService>,
    pub rules: Arc<RulesEngine>,
    pub usage: Arc<UsagePlanner>,
    pub supervisor: Arc<SessionSupervisor>,
    pub host: Arc<TestHost>,
    pub sup_host: Arc<SupHost>,
    pub local: Arc<FakeAdapter>,
    pub remote: Arc<FakeAdapter>,
    pub orch: Arc<CrewOrchestrator>,
    pub workspace: WorkspaceId,
}

impl Harness {
    pub async fn new() -> Harness {
        Harness::build(None).await
    }

    /// Local runs go to the Runtime Adapter interface's `ScriptedAdapter` (feature `testkit`),
    /// which replays `script` for every run.
    pub async fn scripted(script: Vec<RunEvent>) -> Harness {
        let adapter: Arc<dyn RuntimeAdapter> = Arc::new(ordin_runtime_adapter::ScriptedAdapter {
            kind: AdapterKind::ClaudeLocal,
            unsupported: &[],
            script,
            snapshot: None,
            start_error: None,
        });
        Harness::build(Some(adapter)).await
    }

    async fn build(local_override: Option<Arc<dyn RuntimeAdapter>>) -> Harness {
        let dir = TempDir::new();
        let store = Datastore::open(DatastoreConfig::Sqlite {
            path: dir.0.join("data").join("journal.db"),
            backup_dir: dir.0.join("data").join("backups"),
        })
        .await
        .expect("open the datastore");
        let mut registry = MigrationRegistry::new();
        registry.register(MIGRATIONS).unwrap();
        registry.register(ApprovalService::migrations()).unwrap();
        registry.register(RulesEngine::migrations()).unwrap();
        registry.register(UsagePlanner::migrations()).unwrap();
        registry.register(SessionSupervisor::migrations()).unwrap();
        registry.register(CrewOrchestrator::migrations()).unwrap();
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
            .unwrap(),
        );
        let profile = Arc::new(
            OrganisationProfileService::start(
                ProfileMode::NotUsed,
                ProfileOptions {
                    refresh_interval: Duration::from_secs(3600),
                    clock: Arc::new(Now),
                },
            )
            .await
            .unwrap(),
        );
        let usage = UsagePlanner::new(store.clone(), journal.clone(), profile.clone(), vec![])
            .await
            .unwrap();
        let approvals = ApprovalService::new(
            store.clone(),
            journal.clone(),
            ApprovalConfig {
                backend: BackendId(journal.backend_uuid()),
                local_computer_name: "Asha's laptop".into(),
                approvers: None,
            },
        )
        .await
        .unwrap();
        let rules = RulesEngine::new(
            store.clone(),
            journal.clone(),
            profile.clone(),
            approvals.clone(),
            Arc::new(RulesStub),
            RulesConfig {
                ordin_servers: vec!["ordin".into()],
                ordin_server_prefixes: vec![],
                protected_roots: vec![],
                app_data_root: PathBuf::from("/Users/asha/Library/Application Support/ordin"),
                program: RulesProgram::Desktop,
                instance_name: "local".into(),
            },
        );
        let sup_host = Arc::new(SupHost {
            routes: Mutex::new(HashMap::new()),
        });
        let supervisor = SessionSupervisor::new(
            store.clone(),
            journal.clone(),
            usage.clone(),
            None,
            sup_host.clone(),
            SupervisorConfig {
                program: ProgramKind::Desktop,
                local_adapter: AdapterKind::ClaudeLocal,
                local_session_limit: 8,
                instance_name: "desktop".into(),
                default_wall_clock_secs: 3_600,
                idle_timeout_secs: 600,
                stop_deadline: Duration::from_secs(1),
            },
        );
        let local = FakeAdapter::new(AdapterKind::ClaudeLocal, &[]);
        let remote = FakeAdapter::new(AdapterKind::HostedAgent, &[AdapterVerb::Cancel]);
        match local_override {
            Some(adapter) => supervisor.register_adapter(adapter).unwrap(),
            None => supervisor.register_adapter(local.clone()).unwrap(),
        }
        supervisor.register_adapter(remote.clone()).unwrap();
        let host = TestHost::new();
        let orch = CrewOrchestrator::new(
            store.clone(),
            journal.clone(),
            supervisor.clone(),
            rules.clone(),
            approvals.clone(),
            usage.clone(),
            profile,
            host.clone(),
        );
        Harness {
            _dir: dir,
            store,
            journal,
            approvals,
            rules,
            usage,
            supervisor,
            host,
            sup_host,
            local,
            remote,
            orch,
            workspace: WorkspaceId::new(),
        }
    }

    pub fn pool(&self) -> &sqlx::SqlitePool {
        self.store.sqlite().expect("SQLite")
    }

    /// A remote agent whose route is `HostedAgent`, known to both hosts.
    pub fn remote_agent(&self) -> AgentRef {
        let id = RemoteAgentId::new();
        self.sup_host
            .routes
            .lock()
            .unwrap()
            .insert(id, AdapterKind::HostedAgent);
        self.host
            .routes
            .lock()
            .unwrap()
            .insert(id, AdapterKind::HostedAgent);
        AgentRef::RemoteAgent {
            remote_agent_id: id,
        }
    }

    pub fn charter(&self, members: &[(&str, AgentRef)]) -> CrewCharter {
        charter(self.workspace, members)
    }

    pub async fn start(
        &self,
        charter: CrewCharter,
        coordinator: Arc<TestCoordinator>,
    ) -> CrewRunId {
        self.orch
            .start(CrewStart {
                charter,
                inputs: vec![],
                coordinator,
                started_by_person: true,
                trigger_id: None,
            })
            .await
            .expect("the crew starts")
    }

    pub async fn start_with(
        &self,
        charter: CrewCharter,
        inputs: Vec<CrewInput>,
        coordinator: Arc<TestCoordinator>,
    ) -> CrewRunId {
        self.orch
            .start(CrewStart {
                charter,
                inputs,
                coordinator,
                started_by_person: true,
                trigger_id: None,
            })
            .await
            .expect("the crew starts")
    }

    /// Waits until the crew run is in a state `ok` accepts.
    /// Waits until the open round names its member run: the adapter starts a run a moment before
    /// the round records it, and a restart in that moment would find no run to follow.
    pub async fn wait_member_recorded(&self, id: CrewRunId) {
        for _ in 0..1_000 {
            let (_, rounds) = self.orch.get(id).await.expect("the crew run");
            if rounds.iter().any(|r| !r.member_runs.is_empty()) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("the round never named its member run");
    }

    pub async fn wait_until(&self, id: CrewRunId, ok: impl Fn(&CrewRun) -> bool) -> CrewRun {
        for _ in 0..1_000 {
            let (run, _) = self.orch.get(id).await.expect("the crew run");
            if ok(&run) {
                return run;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let (run, _) = self.orch.get(id).await.unwrap();
        panic!(
            "the crew run never reached the awaited state; it is {:?} {:?} {:?}",
            run.state, run.pause_reason, run.end_reason
        );
    }

    pub async fn wait_ended(&self, id: CrewRunId) -> CrewRun {
        self.wait_until(id, |r| {
            !matches!(
                r.state,
                CrewRunState::Starting | CrewRunState::Running | CrewRunState::Paused
            )
        })
        .await
    }

    /// Waits until `crew_ended` reached the coordinator (the journal entry follows it).
    pub async fn wait_reported(&self, coordinator: &TestCoordinator) {
        for _ in 0..1_000 {
            if !coordinator.outcomes.lock().unwrap().is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("crew_ended never reached the coordinator");
    }

    /// The journal entries of `kind` written for `id`, as data.
    pub async fn entries(&self, id: CrewRunId, kind: &str) -> Vec<serde_json::Value> {
        // The ending writes `crew_ended` last, after the state a test waits on.
        let tries = if kind == "crew_ended" { 500 } else { 1 };
        for _ in 1..tries {
            if !self.entries_now(id, kind).await.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        self.entries_now(id, kind).await
    }

    async fn entries_now(&self, id: CrewRunId, kind: &str) -> Vec<serde_json::Value> {
        self.journal
            .list_entries(
                EntryFilter {
                    run_id: None,
                    kind: Some(kind.into()),
                },
                None,
            )
            .await
            .unwrap()
            .items
            .into_iter()
            .filter(|e| e.crew_run_id == Some(id))
            .map(|e| e.data)
            .collect()
    }

    /// Every event of `kind` the journal emitted, as data.
    pub async fn events(&self, kind: &str) -> Vec<serde_json::Value> {
        self.journal
            .events_since(0, 1_000)
            .await
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == kind)
            .map(|e| e.data)
            .collect()
    }

    /// Presses the approve (or reject) button of the first pending approval, once there is one.
    pub async fn decide_pending(&self, approve: bool) -> ordin_types::ApprovalId {
        for _ in 0..1_000 {
            let pending = self.approvals.list_pending(None, None).await.unwrap().items;
            if let Some(record) = pending.first() {
                let card = &record.card;
                let button = if approve {
                    card.buttons.approve.clone()
                } else {
                    card.buttons.reject.clone()
                };
                self.approvals
                    .decide(
                        ordin_types::wire::approval::ApprovalPress {
                            card_ref: button,
                            decision: if approve {
                                ordin_types::wire::approval::PressDecision::Approve
                            } else {
                                ordin_types::wire::approval::PressDecision::Reject
                            },
                        },
                        PressOrigin::LocalScreen,
                    )
                    .await
                    .expect("press");
                return card.approval_id;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("no approval became pending");
    }
}

// ---- builders

pub fn agent() -> AgentRef {
    AgentRef::AgentInstance {
        agent_instance_id: AgentInstanceId::new(),
    }
}

pub fn guardrails() -> Guardrails {
    Guardrails {
        max_rounds: 10,
        wall_clock_secs: 3_600,
        request_budget: 0,
        max_members_running: 4,
        members_allowed_external_actions: vec![],
        approval_points: vec![],
        failure_repeat_limit: 2,
    }
}

pub fn charter(workspace: WorkspaceId, members: &[(&str, AgentRef)]) -> CrewCharter {
    CrewCharter {
        id: CrewCharterId::new(),
        workspace_id: workspace,
        name: None,
        purpose: "Compare the supplier quotes".into(),
        charter_version: 1,
        coordinator: CoordinatorKind::Ordin,
        members: members
            .iter()
            .map(|(role, a)| CrewMember {
                member: *a,
                role: (*role).to_owned(),
            })
            .collect(),
        allowed_handoffs: vec![],
        guardrails: guardrails(),
        input_fields: vec![],
        workflow_id: None,
        workflow_version: None,
        steps: vec![],
        state: CrewCharterState::Valid,
        written_at: Timestamp::now(),
    }
}

pub fn handoff(from: AgentRef, to: AgentRef) -> Handoff {
    Handoff { from, to }
}

pub fn delegate(member: AgentRef, brief: &str) -> CrewToolCall {
    CrewToolCall::Delegate {
        member,
        brief: brief.to_owned(),
        uses_results_of: vec![],
        uses_inputs: vec![],
    }
}

pub fn finish() -> CrewToolCall {
    CrewToolCall::Finish {
        summary: "Done.".into(),
    }
}

pub fn step(position: &str, agent: AgentRef, line: &str) -> AgentStep {
    AgentStep {
        position: position.to_owned(),
        group: None,
        agent,
        line: line.to_owned(),
        uses: vec![],
        asks_first: false,
        ordin_checks_result: false,
        if_it_fails: IfItFails::OrdinTakesOver,
    }
}

pub fn agent_step(position: &str, agent: AgentRef, line: &str) -> WorkflowStep {
    WorkflowStep::Agent(step(position, agent, line))
}

pub fn uses_input(label: &str) -> StepUse {
    StepUse::Input {
        label: label.to_owned(),
    }
}

pub fn uses_result(position: &str) -> StepUse {
    StepUse::Result {
        position: position.to_owned(),
    }
}

pub fn text_input(label: &str, text: &str) -> CrewInput {
    CrewInput {
        label: label.to_owned(),
        value: CrewInputValue::Text {
            text: text.to_owned(),
        },
        used: vec![],
    }
}

pub fn list_input(label: &str, items: &[&str]) -> CrewInput {
    CrewInput {
        label: label.to_owned(),
        value: CrewInputValue::List {
            items: items.iter().map(|s| (*s).to_owned()).collect(),
        },
        used: vec![],
    }
}

/// A charter of saved steps with `members`.
pub fn saved_charter(
    h: &Harness,
    members: &[(&str, AgentRef)],
    steps: Vec<WorkflowStep>,
) -> CrewCharter {
    let mut c = h.charter(members);
    c.workflow_id = Some(WorkflowId::new());
    c.workflow_version = Some(3);
    c.steps = steps;
    c
}

/// A rule set with nothing declared.
pub fn rule_set(
    authority: ordin_types::domain::agents::Authority,
    agent_instance_id: AgentInstanceId,
) -> ordin_rules::InstanceRuleSet {
    ordin_rules::InstanceRuleSet {
        agent_instance_id,
        package_name: "analyst".to_owned(),
        authority,
        agent_folder: PathBuf::from("/Users/asha/Library/Application Support/ordin/agents/crew"),
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

/// Waits until the supervisor shows the run ended, and returns its state.
pub async fn run_end_state(h: &Harness, run: RunId) -> ordin_types::domain::runs::RunState {
    use ordin_types::domain::runs::RunState;
    for _ in 0..1_500 {
        let state = h.supervisor.get(run).await.unwrap().run.state;
        if !matches!(
            state,
            RunState::Queued | RunState::Running | RunState::Paused
        ) {
            return state;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the run never ended")
}
