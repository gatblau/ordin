//! Stand-ins behind every port, for this crate's unit tests and, with the `testkit` feature, its
//! integration tests. A stand-in answers a sensible default, records what it was called with, and
//! fails a call when the test has named that call in [`Fails`].
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::future::BoxFuture;
use futures::stream::BoxStream;
use ordin_crew_orchestrator::{CrewInput, CrewStart, PlanCheck};
use ordin_journal::{NewArtefact, NewJournalEntry};
use ordin_memory_service::{
    AgentMemoryRules, FactView, MemoryCaller, MemoryPlace, RecallInput, RecallResult,
    RememberFactsInput, RememberFactsReceipt, RememberInput, RememberReceipt, RememberedInRun,
    VocabularyView,
};
use ordin_package_catalogue::{
    AgentFound, AgentMatch, ChangeChoiceInput, PendingChoice, TrialInput,
};
use ordin_runtime_adapter::{PauseReason, ResumeDecision, RunEnd, RunEndReason, RunEvent};
use ordin_service_api::blocks::{Block, BlockCheckContext, BlockRejection};
use ordin_session_supervisor::ConversationStart;
use ordin_types::domain::runs::{CrewCharter, CrewRunState, Dashboard, ReusedAnswer, Run, Task};
use ordin_types::domain::HarnessKind;
use ordin_types::time::Timestamp;
use ordin_types::wire::workflow_file::AgentRepositoryRef;
use ordin_types::{
    AgentInstanceId, AgentRepositoryId, ArtefactId, CodingTaskId, CrewRunId, DashboardId,
    KnowledgePageId, MemoryFactId, MemoryThingId, PendingPushId, PlaceId, ProjectId,
    ReusedAnswerId, RunId, TaskId, WorkspaceId,
};
use ordin_usage_planner::{
    AnswerToIndex, EstimateRequest, LevelChoice, ReuseQuery, UsageEstimate, WorkKind,
};
use tokio::sync::broadcast;

use crate::ports::*;
use crate::types::{
    CompanyVoice, CrewLimitPolicy, OrdinHostConfig, OrdinHostPolicy, ProgramProfile,
};

// ---------------------------------------------------------------- failures and calls

/// Calls the test makes fail, by `"<port>.<method>"`. A queued failure is used once; a standing one
/// every time.
#[derive(Default)]
pub struct Fails {
    standing: Mutex<HashMap<&'static str, DepError>>,
    once: Mutex<HashMap<&'static str, VecDeque<DepError>>>,
    calls: Mutex<Vec<String>>,
    /// `fail_nth`: the call number (from 1) that fails once, with its failure.
    nth: Mutex<HashMap<&'static str, (usize, DepError)>>,
}

impl Fails {
    /// The `n`th call of `call` (counting from 1, from now on) fails once; the others succeed.
    pub fn fail_nth(&self, call: &'static str, n: usize, code: &str, retryable: bool) {
        let done = self.count(call);
        self.nth
            .lock()
            .unwrap()
            .insert(call, (done + n, DepError::new(code, retryable)));
    }

    pub fn always(&self, call: &'static str, code: &str, retryable: bool) {
        self.standing
            .lock()
            .unwrap()
            .insert(call, DepError::new(code, retryable));
    }

    pub fn once(&self, call: &'static str, code: &str, retryable: bool) {
        self.once
            .lock()
            .unwrap()
            .entry(call)
            .or_default()
            .push_back(DepError::new(code, retryable));
    }

    pub fn clear(&self, call: &'static str) {
        self.standing.lock().unwrap().remove(call);
        self.once.lock().unwrap().remove(call);
    }

    /// Records the call, then answers its failure if the test set one.
    pub fn check(&self, call: &'static str) -> Result<(), DepError> {
        self.calls.lock().unwrap().push(call.to_owned());
        let at = self.count(call);
        if let Some((n, e)) = self.nth.lock().unwrap().get(call) {
            if *n == at {
                return Err(e.clone());
            }
        }
        if let Some(q) = self.once.lock().unwrap().get_mut(call) {
            if let Some(e) = q.pop_front() {
                return Err(e);
            }
        }
        match self.standing.lock().unwrap().get(call) {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }

    pub fn count(&self, call: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| *c == call)
            .count()
    }
}

fn unset(call: &str) -> DepError {
    DepError {
        code: "FAKE_UNSET".to_owned(),
        retryable: false,
        message: format!("{call} has no value in this test"),
    }
}

/// What Ordin answers to a turn: `Some(text)` pauses with that text; `None` sends nothing.
pub type Responder = Arc<dyn Fn(RunId, String) -> BoxFuture<'static, Option<String>> + Send + Sync>;

pub fn reply_with(text: &'static str) -> Responder {
    Arc::new(move |_, _| Box::pin(async move { Some(text.to_owned()) }))
}

// ---------------------------------------------------------------- Session Supervisor

/// One task the Supervisor was given, with how it was started.
pub type SubmittedTask = (Task, WorkKind, bool, Option<CodingTaskId>);

pub struct FakeSupervisor {
    pub f: Arc<Fails>,
    pub runs: Mutex<HashMap<RunId, broadcast::Sender<RunEvent>>>,
    pub subscribed: Mutex<Vec<RunId>>,
    pub starts: Mutex<Vec<ConversationStart>>,
    /// Every turn sent, in order, with its run.
    pub turns: Mutex<Vec<(RunId, String)>>,
    pub cancelled: Mutex<Vec<RunId>>,
    pub tasks: Mutex<Vec<SubmittedTask>>,
    /// The run of each submitted task, in order.
    pub task_runs: Mutex<Vec<RunId>>,
    pub listed: Mutex<Vec<Run>>,
    pub responder: Mutex<Responder>,
    pub context_percent: Mutex<Option<u8>>,
    /// Runs the Supervisor no longer supervises (subscribe fails), as after a restart.
    pub forgotten: Mutex<Vec<RunId>>,
    /// What `run_task` answers, by run (`get`).
    pub stored: Mutex<HashMap<RunId, RunWithTask>>,
}

impl FakeSupervisor {
    pub fn new(f: Arc<Fails>) -> Self {
        FakeSupervisor {
            f,
            runs: Mutex::new(HashMap::new()),
            subscribed: Mutex::new(vec![]),
            starts: Mutex::new(vec![]),
            turns: Mutex::new(vec![]),
            cancelled: Mutex::new(vec![]),
            tasks: Mutex::new(vec![]),
            task_runs: Mutex::new(vec![]),
            listed: Mutex::new(vec![]),
            responder: Mutex::new(reply_with("Ready.")),
            context_percent: Mutex::new(None),
            forgotten: Mutex::new(vec![]),
            stored: Mutex::new(HashMap::new()),
        }
    }

    pub fn respond(&self, r: Responder) {
        *self.responder.lock().unwrap() = r;
    }

    /// Sends Ordin's answer `text` on `run_id`, as a pause for the person.
    pub fn pause(&self, run_id: RunId, text: &str) {
        if let Some(tx) = self.runs.lock().unwrap().get(&run_id) {
            let _ = tx.send(RunEvent::Paused {
                reason: PauseReason::Person {
                    question: text.to_owned(),
                },
            });
        }
    }

    /// Ends `run_id` with `end`.
    pub fn end_with(&self, run_id: RunId, end: RunEnd) {
        if let Some(tx) = self.runs.lock().unwrap().get(&run_id) {
            let _ = tx.send(RunEvent::Ended { end });
        }
    }

    /// Ends `run_id` as if the harness stopped it.
    pub fn end(&self, run_id: RunId) {
        if let Some(tx) = self.runs.lock().unwrap().get(&run_id) {
            let _ = tx.send(RunEvent::Ended {
                end: RunEnd {
                    reason: RunEndReason::Completed,
                    final_message: None,
                    artefacts: vec![],
                },
            });
        }
    }

    pub fn seeds(&self) -> Vec<String> {
        self.starts
            .lock()
            .unwrap()
            .iter()
            .map(|s| s.seed.clone())
            .collect()
    }

    pub fn turn_texts(&self) -> Vec<String> {
        self.turns
            .lock()
            .unwrap()
            .iter()
            .map(|t| t.1.clone())
            .collect()
    }

    fn answer(&self, run_id: RunId, text: String) {
        let responder = self.responder.lock().unwrap().clone();
        let tx = self.runs.lock().unwrap().get(&run_id).cloned();
        if let Some(tx) = tx {
            tokio::spawn(async move {
                if let Some(reply) = responder(run_id, text).await {
                    let _ = tx.send(RunEvent::Paused {
                        reason: PauseReason::Person { question: reply },
                    });
                }
            });
        }
    }
}

#[async_trait::async_trait]
impl SupervisorPort for FakeSupervisor {
    async fn start_conversation(&self, req: ConversationStart) -> Result<RunId, DepError> {
        self.f.check("supervisor.start_conversation")?;
        let run_id = RunId::new();
        let (tx, _) = broadcast::channel(64);
        self.runs.lock().unwrap().insert(run_id, tx);
        self.starts.lock().unwrap().push(req);
        Ok(run_id)
    }

    fn subscribe(&self, run_id: RunId) -> Result<broadcast::Receiver<RunEvent>, DepError> {
        self.f.check("supervisor.subscribe")?;
        if self.forgotten.lock().unwrap().contains(&run_id) {
            return Err(DepError::new("SUP_RUN_UNKNOWN", false));
        }
        let rx = self
            .runs
            .lock()
            .unwrap()
            .get(&run_id)
            .map(|tx| tx.subscribe())
            .ok_or_else(|| DepError::new("SUP_RUN_UNKNOWN", false))?;
        let first = {
            let mut s = self.subscribed.lock().unwrap();
            let first = !s.contains(&run_id);
            s.push(run_id);
            first
        };
        // The seed is Ordin's first turn: answered once the host reads the run.
        if first {
            let seed = self
                .starts
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find(|_| true)
                .map(|s| s.seed.clone())
                .unwrap_or_default();
            self.answer(run_id, seed);
        }
        Ok(rx)
    }

    async fn resume_with_decision(
        &self,
        run_id: RunId,
        decision: ResumeDecision,
    ) -> Result<(), DepError> {
        self.f.check("supervisor.resume_with_decision")?;
        let text = match decision {
            ResumeDecision::Answer { text } => text,
            other => format!("{other:?}"),
        };
        self.turns.lock().unwrap().push((run_id, text.clone()));
        self.answer(run_id, text);
        Ok(())
    }

    async fn cancel(&self, run_id: RunId) -> Result<(), DepError> {
        self.f.check("supervisor.cancel")?;
        self.cancelled.lock().unwrap().push(run_id);
        Ok(())
    }

    async fn submit_task(
        &self,
        task: Task,
        work_kind: WorkKind,
        started_by_person: bool,
        coding_task_id: Option<CodingTaskId>,
    ) -> Result<RunId, DepError> {
        self.f.check("supervisor.submit_task")?;
        self.tasks
            .lock()
            .unwrap()
            .push((task, work_kind, started_by_person, coding_task_id));
        // The task's run can be followed and ended (`end_with`), as the Supervisor's can.
        let run_id = RunId::new();
        let (tx, _) = broadcast::channel(64);
        self.runs.lock().unwrap().insert(run_id, tx);
        self.task_runs.lock().unwrap().push(run_id);
        Ok(run_id)
    }

    async fn list(&self, _workspace_id: WorkspaceId) -> Result<Vec<Run>, DepError> {
        self.f.check("supervisor.list")?;
        Ok(self.listed.lock().unwrap().clone())
    }

    async fn run_task(&self, run_id: RunId) -> Result<Option<RunWithTask>, DepError> {
        self.f.check("supervisor.get")?;
        Ok(self.stored.lock().unwrap().get(&run_id).cloned())
    }

    fn context_used_percent(&self, _run_id: RunId) -> Option<u8> {
        *self.context_percent.lock().unwrap()
    }
}

// ---------------------------------------------------------------- Crew Orchestrator

/// Runs inside `FakeOrchestrator::start` once the crew run's id is made, before `start` returns, as
/// a crew that is already under way when its start returns.
pub type StartHook = Arc<dyn Fn(CrewRunId, &CrewStart) -> BoxFuture<'static, ()> + Send + Sync>;

pub struct FakeOrchestrator {
    pub f: Arc<Fails>,
    pub plan: Mutex<PlanCheck>,
    pub started: Mutex<Vec<CrewStart>>,
    pub states: Mutex<HashMap<CrewRunId, CrewRunState>>,
    pub running: Mutex<Option<CrewRunId>>,
    /// The workspace of each crew run `start` made (`workspace_of`); a crew run absent here is
    /// unknown to `workspace_of`.
    pub workspaces: Mutex<HashMap<CrewRunId, WorkspaceId>>,
    pub killed: Mutex<Vec<CrewRunId>>,
    pub take_overs: Mutex<Vec<(CrewRunId, String, String)>>,
    pub on_start: Mutex<Option<StartHook>>,
    /// What `paused_for_restart` answers (OH-CLS-1).
    pub restart_paused: Mutex<u32>,
}

impl FakeOrchestrator {
    pub fn new(f: Arc<Fails>) -> Self {
        FakeOrchestrator {
            f,
            plan: Mutex::new(PlanCheck {
                fits: true,
                problems: vec![],
                lowered: vec![],
            }),
            started: Mutex::new(vec![]),
            states: Mutex::new(HashMap::new()),
            running: Mutex::new(None),
            workspaces: Mutex::new(HashMap::new()),
            killed: Mutex::new(vec![]),
            take_overs: Mutex::new(vec![]),
            on_start: Mutex::new(None),
            restart_paused: Mutex::new(0),
        }
    }
}

#[async_trait::async_trait]
impl OrchestratorPort for FakeOrchestrator {
    async fn check_plan(&self, _charter: &CrewCharter, _inputs: &[CrewInput]) -> PlanCheck {
        let _ = self.f.check("orchestrator.check_plan");
        self.plan.lock().unwrap().clone()
    }
    async fn start(&self, start: CrewStart) -> Result<CrewRunId, DepError> {
        self.f.check("orchestrator.start")?;
        let id = CrewRunId::new();
        self.workspaces
            .lock()
            .unwrap()
            .insert(id, start.charter.workspace_id);
        let hook = self.on_start.lock().unwrap().clone();
        if let Some(hook) = hook {
            hook(id, &start).await;
        }
        self.started.lock().unwrap().push(start);
        self.states
            .lock()
            .unwrap()
            .insert(id, CrewRunState::Running);
        Ok(id)
    }
    async fn kill(&self, crew_run_id: CrewRunId) -> Result<(), DepError> {
        self.f.check("orchestrator.kill")?;
        self.killed.lock().unwrap().push(crew_run_id);
        Ok(())
    }
    async fn state(&self, crew_run_id: CrewRunId) -> Result<Option<CrewRunState>, DepError> {
        self.f.check("orchestrator.state")?;
        Ok(self.states.lock().unwrap().get(&crew_run_id).copied())
    }
    async fn running_crew(
        &self,
        _workspace_id: WorkspaceId,
    ) -> Result<Option<CrewRunId>, DepError> {
        self.f.check("orchestrator.running_crew")?;
        Ok(*self.running.lock().unwrap())
    }
    async fn workspace_of(&self, crew_run_id: CrewRunId) -> Result<Option<WorkspaceId>, DepError> {
        self.f.check("orchestrator.workspace_of")?;
        Ok(self.workspaces.lock().unwrap().get(&crew_run_id).copied())
    }
    async fn paused_for_restart(&self, _workspace_id: WorkspaceId) -> Result<u32, DepError> {
        self.f.check("orchestrator.paused_for_restart")?;
        Ok(*self.restart_paused.lock().unwrap())
    }
    async fn request_take_over(
        &self,
        crew_run_id: CrewRunId,
        from_position: String,
        reason: String,
    ) -> Result<(), DepError> {
        self.f.check("orchestrator.request_take_over")?;
        self.take_overs
            .lock()
            .unwrap()
            .push((crew_run_id, from_position, reason));
        Ok(())
    }
}

// ---------------------------------------------------------------- Package Inspector

pub struct FakeInspector {
    pub f: Arc<Fails>,
    pub agents: Mutex<Vec<DirectoryAgent>>,
    pub memory: Mutex<HashMap<AgentInstanceId, DeclaredMemory>>,
    pub changes: Mutex<Option<futures::channel::mpsc::UnboundedReceiver<WorkspaceId>>>,
    pub changes_tx: futures::channel::mpsc::UnboundedSender<WorkspaceId>,
}

impl FakeInspector {
    pub fn new(f: Arc<Fails>) -> Self {
        let (tx, rx) = futures::channel::mpsc::unbounded();
        FakeInspector {
            f,
            agents: Mutex::new(vec![]),
            memory: Mutex::new(HashMap::new()),
            changes: Mutex::new(Some(rx)),
            changes_tx: tx,
        }
    }
}

#[async_trait::async_trait]
impl InspectorPort for FakeInspector {
    async fn directory(&self, _workspace_id: WorkspaceId) -> Result<Vec<DirectoryAgent>, DepError> {
        self.f.check("inspector.directory")?;
        Ok(self.agents.lock().unwrap().clone())
    }
    fn changes(&self) -> BoxStream<'static, WorkspaceId> {
        use futures::StreamExt;
        match self.changes.lock().unwrap().take() {
            Some(rx) => rx.boxed(),
            None => futures::stream::empty().boxed(),
        }
    }
    async fn memory_contract(
        &self,
        id: AgentInstanceId,
    ) -> Result<Option<DeclaredMemory>, DepError> {
        self.f.check("inspector.memory_contract")?;
        Ok(self.memory.lock().unwrap().get(&id).cloned())
    }
}

/// An installed agent of the directory.
pub fn agent(name: &str) -> DirectoryAgent {
    DirectoryAgent {
        agent_instance_id: AgentInstanceId::new(),
        display_name: name.to_owned(),
        description: format!("{name} does its work."),
        plugin_name: Some(name.to_lowercase().replace(' ', "-")),
        commands: vec![],
        skills: vec![],
        lifecycle: None,
        authority: "can_change_things".to_owned(),
        declared_rules: vec![],
    }
}

// ---------------------------------------------------------------- Usage Planner

pub struct FakeUsage {
    pub f: Arc<Fails>,
    pub summarise: Mutex<bool>,
    pub continue_from_summary: Mutex<bool>,
    pub without_model: Mutex<bool>,
    pub reusable: Mutex<Option<ReusedAnswer>>,
    pub reused: Mutex<Vec<(ReusedAnswerId, Option<RunId>)>>,
    pub indexed: Mutex<Vec<AnswerToIndex>>,
    pub estimate: Mutex<Option<UsageEstimate>>,
    pub estimates: Mutex<Vec<EstimateRequest>>,
    pub summarise_asked: Mutex<Vec<(Option<u32>, u32)>>,
}

impl FakeUsage {
    pub fn new(f: Arc<Fails>) -> Self {
        FakeUsage {
            f,
            summarise: Mutex::new(false),
            continue_from_summary: Mutex::new(false),
            without_model: Mutex::new(false),
            reusable: Mutex::new(None),
            reused: Mutex::new(vec![]),
            indexed: Mutex::new(vec![]),
            estimate: Mutex::new(None),
            estimates: Mutex::new(vec![]),
            summarise_asked: Mutex::new(vec![]),
        }
    }
}

#[async_trait::async_trait]
impl UsagePort for FakeUsage {
    fn choose_talking(&self, _harness: ordin_types::wire::run::AdapterKind) -> Option<LevelChoice> {
        None
    }
    async fn may_start_ordin_work(&self) -> Result<(), DepError> {
        self.f.check("usage.may_start_ordin_work")
    }
    fn should_summarise(
        &self,
        context_used_permille: Option<u32>,
        turns_since_summary: u32,
    ) -> bool {
        self.summarise_asked
            .lock()
            .unwrap()
            .push((context_used_permille, turns_since_summary));
        *self.summarise.lock().unwrap()
    }
    fn continue_from_summary(&self, _last_message_at: Timestamp, _now: Timestamp) -> bool {
        *self.continue_from_summary.lock().unwrap()
    }
    fn answer_without_model(&self) -> bool {
        *self.without_model.lock().unwrap()
    }
    async fn record_answered_without_model(&self) -> Result<(), DepError> {
        self.f.check("usage.record_answered_without_model")
    }
    async fn find_reusable(&self, _q: ReuseQuery) -> Result<Option<ReusedAnswer>, DepError> {
        self.f.check("usage.find_reusable")?;
        Ok(self.reusable.lock().unwrap().clone())
    }
    async fn record_reuse(
        &self,
        id: ReusedAnswerId,
        run_id: Option<RunId>,
    ) -> Result<(), DepError> {
        self.f.check("usage.record_reuse")?;
        self.reused.lock().unwrap().push((id, run_id));
        Ok(())
    }
    async fn index_answer(&self, a: AnswerToIndex) -> Result<ReusedAnswerId, DepError> {
        self.f.check("usage.index_answer")?;
        self.indexed.lock().unwrap().push(a);
        Ok(ReusedAnswerId::new())
    }
    async fn estimate(&self, req: EstimateRequest) -> Result<UsageEstimate, DepError> {
        self.f.check("usage.estimate")?;
        self.estimates.lock().unwrap().push(req);
        self.estimate
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| unset("usage.estimate"))
    }
}

// ---------------------------------------------------------------- Workspace Service

pub struct FakeWorkspaces {
    pub f: Arc<Fails>,
    pub name: Mutex<String>,
    pub places: Mutex<Vec<String>>,
    pub repos: Mutex<Vec<LinkedRepo>>,
    /// OH-ACC-1: the folders every agent is asked about; empty, no folder is linked.
    pub folders: Mutex<Vec<AgentFolder>>,
    /// OH-WC-1: the linked documents changed and not sent back.
    pub changed: Mutex<Vec<ChangedDocument>>,
    /// OH-TR-2: the working-copy items of the workspace's links.
    pub documents: Mutex<Vec<SourceDocument>>,
    /// OH-TF-1: every `set_agent_grant` call, in order: (agent, place, access).
    pub grants_set: Mutex<
        Vec<(
            AgentInstanceId,
            PlaceId,
            Option<ordin_types::domain::places::FolderAccess>,
        )>,
    >,
    /// OH-TW-1: what `results_folder` answers; None, the workspace has no results folder.
    pub results_folder: Mutex<Option<PlaceId>>,
    /// OH-TW-1: what `grants` answers.
    pub grants: Mutex<Vec<ordin_types::domain::places::FolderGrant>>,
}

#[async_trait::async_trait]
impl WorkspacesPort for FakeWorkspaces {
    async fn ensure_active(&self, _id: WorkspaceId) -> Result<(), DepError> {
        self.f.check("workspaces.ensure_active")
    }
    async fn summary(&self, _id: WorkspaceId) -> Result<WorkspaceFacts, DepError> {
        self.f.check("workspaces.summary")?;
        Ok(WorkspaceFacts {
            name: self.name.lock().unwrap().clone(),
            places: self.places.lock().unwrap().clone(),
        })
    }
    async fn linked_repositories(&self, _id: WorkspaceId) -> Result<Vec<LinkedRepo>, DepError> {
        self.f.check("workspaces.linked_repositories")?;
        Ok(self.repos.lock().unwrap().clone())
    }
    async fn agent_folders(
        &self,
        _id: WorkspaceId,
        _agent: AgentInstanceId,
    ) -> Result<Vec<AgentFolder>, DepError> {
        self.f.check("workspaces.agent_folders")?;
        Ok(self.folders.lock().unwrap().clone())
    }
    async fn changed_documents(&self, _id: WorkspaceId) -> Result<Vec<ChangedDocument>, DepError> {
        self.f.check("workspaces.changed_documents")?;
        Ok(self.changed.lock().unwrap().clone())
    }
    async fn source_documents(&self, _id: WorkspaceId) -> Result<Vec<SourceDocument>, DepError> {
        self.f.check("workspaces.source_documents")?;
        Ok(self.documents.lock().unwrap().clone())
    }
    /// Records the call and keeps `folders[].granted` in step with it.
    async fn set_agent_grant(
        &self,
        _id: WorkspaceId,
        agent: AgentInstanceId,
        place_id: PlaceId,
        access: Option<ordin_types::domain::places::FolderAccess>,
    ) -> Result<(), DepError> {
        self.f.check("workspaces.set_grant")?;
        self.grants_set
            .lock()
            .unwrap()
            .push((agent, place_id, access));
        for folder in self.folders.lock().unwrap().iter_mut() {
            if folder.place_id == place_id {
                folder.granted = access.is_some();
            }
        }
        Ok(())
    }
    async fn results_folder(&self, _id: WorkspaceId) -> Result<Option<PlaceId>, DepError> {
        self.f.check("workspaces.results_folder")?;
        Ok(*self.results_folder.lock().unwrap())
    }
    async fn grants(
        &self,
        _id: WorkspaceId,
    ) -> Result<Vec<ordin_types::domain::places::FolderGrant>, DepError> {
        self.f.check("workspaces.grants")?;
        Ok(self.grants.lock().unwrap().clone())
    }
}

// ---------------------------------------------------------------- Journal

pub struct FakeJournal {
    pub f: Arc<Fails>,
    pub events: Mutex<Vec<(String, serde_json::Value)>>,
    pub entries: Mutex<Vec<NewJournalEntry>>,
    pub dashboards: Mutex<HashMap<DashboardId, Dashboard>>,
    pub refreshes: Mutex<Vec<(DashboardId, RunId)>>,
    /// What `recent_reports` answers, newest first (OH-KD-1).
    pub reports: Mutex<Vec<ordin_journal::ReportSummary>>,
    /// The dashboards `keep_dashboard` was asked to keep (OH-KD-1).
    pub kept: Mutex<Vec<ordin_journal::NewDashboard>>,
    pub artefacts: Mutex<Vec<NewArtefact>>,
    pub external_actions: Mutex<u32>,
    pub artefact_names: Mutex<Vec<String>>,
    /// What `unknown_outcome_runs` answers (OH-CLS-1).
    pub unknown_outcome: Mutex<Vec<RunId>>,
    /// What `run_code` answers, by run (OH-DLG-2).
    pub run_codes: Mutex<HashMap<RunId, String>>,
    /// The minutes `keep_minutes` was asked to keep (OH-MIN-2).
    #[cfg(feature = "minutes")]
    pub minutes: Mutex<Vec<ordin_journal::NewMinutes>>,
}

#[async_trait::async_trait]
impl JournalPort for FakeJournal {
    async fn append_event(&self, kind: &str, data: serde_json::Value) -> Result<(), DepError> {
        self.f.check("journal.append_event")?;
        self.events.lock().unwrap().push((kind.to_owned(), data));
        Ok(())
    }
    async fn append_entry(&self, entry: NewJournalEntry) -> Result<(), DepError> {
        self.f.check("journal.append_entry")?;
        self.entries.lock().unwrap().push(entry);
        Ok(())
    }
    async fn dashboard(&self, id: DashboardId) -> Result<Option<Dashboard>, DepError> {
        self.f.check("journal.dashboard")?;
        Ok(self.dashboards.lock().unwrap().get(&id).cloned())
    }
    async fn add_dashboard_refresh(&self, id: DashboardId, run_id: RunId) -> Result<(), DepError> {
        self.f.check("journal.add_dashboard_refresh")?;
        self.refreshes.lock().unwrap().push((id, run_id));
        Ok(())
    }
    async fn recent_reports(
        &self,
        _workspace_id: WorkspaceId,
    ) -> Result<Vec<ordin_journal::ReportSummary>, DepError> {
        self.f.check("journal.recent_reports")?;
        Ok(self.reports.lock().unwrap().clone())
    }
    #[cfg(feature = "minutes")]
    async fn keep_minutes(
        &self,
        new: ordin_journal::NewMinutes,
    ) -> Result<ordin_types::wire::minutes::Minutes, DepError> {
        self.f.check("journal.keep_minutes")?;
        let minutes = ordin_types::wire::minutes::Minutes {
            id: ordin_types::MinutesId::new(),
            workspace_id: new.workspace_id,
            title: new.title.clone(),
            source: new.source,
            source_name: new.source_name.clone(),
            body: new.body.clone(),
            recording: new.recording,
            run_id: new.run_id,
            revision: 1,
            kept_at: Timestamp::now(),
            edited_at: None,
        };
        self.minutes.lock().unwrap().push(new);
        Ok(minutes)
    }
    async fn keep_dashboard(
        &self,
        new: ordin_journal::NewDashboard,
    ) -> Result<Dashboard, DepError> {
        self.f.check("journal.keep_dashboard")?;
        let dashboard = Dashboard {
            id: DashboardId::new(),
            workspace_id: new.workspace_id,
            name: new.name.clone(),
            made_by: new.made_by,
            inputs: new.inputs.clone(),
            refresh_runs: vec![new.run_id],
            kept_at: Timestamp::now(),
        };
        self.kept.lock().unwrap().push(new);
        self.dashboards
            .lock()
            .unwrap()
            .insert(dashboard.id, dashboard.clone());
        Ok(dashboard)
    }
    async fn record_artefact(&self, new: NewArtefact) -> Result<ArtefactId, DepError> {
        self.f.check("journal.record_artefact")?;
        self.artefacts.lock().unwrap().push(new);
        Ok(ArtefactId::new())
    }
    async fn external_actions_for_run(&self, _run_id: RunId) -> Result<u32, DepError> {
        self.f.check("journal.external_actions_for_run")?;
        Ok(*self.external_actions.lock().unwrap())
    }
    async fn artefact_names(&self, _run_id: RunId) -> Result<Vec<String>, DepError> {
        self.f.check("journal.artefact_names")?;
        Ok(self.artefact_names.lock().unwrap().clone())
    }
    async fn unknown_outcome_runs(&self) -> Result<Vec<RunId>, DepError> {
        self.f.check("journal.unknown_outcome_runs")?;
        Ok(self.unknown_outcome.lock().unwrap().clone())
    }
    async fn run_code(&self, run_id: RunId) -> Result<Option<String>, DepError> {
        self.f.check("journal.run_code")?;
        Ok(self.run_codes.lock().unwrap().get(&run_id).cloned())
    }
}

impl FakeJournal {
    pub fn event_kinds(&self) -> Vec<String> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.0.clone())
            .collect()
    }
}

// ---------------------------------------------------------------- Package Catalogue

pub struct FakeCatalogue {
    pub f: Arc<Fails>,
    pub pending: Mutex<Vec<PendingChoice>>,
    pub answers: Mutex<Vec<ChangeChoiceInput>>,
    /// By agent name; absent names answer `Missing`.
    pub matches: Mutex<HashMap<String, AgentMatch>>,
    pub identities: Mutex<HashMap<AgentInstanceId, (AgentRepositoryRef, String, String, String)>>,
    pub found: Mutex<Vec<AgentFound>>,
    pub send_email: Mutex<Vec<AgentInstanceId>>,
    pub trials: Mutex<Vec<TrialInput>>,
}

#[async_trait::async_trait]
impl CataloguePort for FakeCatalogue {
    async fn ensure_ready(&self, _id: AgentInstanceId) -> Result<(), DepError> {
        self.f.check("catalogue.ensure_ready")
    }
    async fn pending_choices(&self, _id: AgentInstanceId) -> Result<Vec<PendingChoice>, DepError> {
        self.f.check("catalogue.pending_choices")?;
        Ok(self.pending.lock().unwrap().clone())
    }
    async fn record_answer(&self, input: ChangeChoiceInput) -> Result<(), DepError> {
        self.f.check("catalogue.record_answer")?;
        self.answers.lock().unwrap().push(input);
        Ok(())
    }
    async fn match_workflow_agent(
        &self,
        _workspace_id: WorkspaceId,
        _repository: &AgentRepositoryRef,
        id: &str,
        _version: &str,
    ) -> Result<AgentMatch, DepError> {
        self.f.check("catalogue.match_workflow_agent")?;
        Ok(self
            .matches
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .unwrap_or(AgentMatch::Missing))
    }
    async fn match_by_name(
        &self,
        _workspace_id: WorkspaceId,
        name: &str,
    ) -> Result<AgentMatch, DepError> {
        self.f.check("catalogue.match_by_name")?;
        Ok(self
            .matches
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .unwrap_or(AgentMatch::Missing))
    }
    async fn agent_identity(
        &self,
        id: AgentInstanceId,
    ) -> Result<(AgentRepositoryRef, String, String, String), DepError> {
        self.f.check("catalogue.agent_identity")?;
        self.identities
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| DepError::new("PKG_INSTANCE_UNKNOWN", false))
    }
    async fn find_agents(
        &self,
        _workspace_id: WorkspaceId,
        _need: &str,
    ) -> Result<Vec<AgentFound>, DepError> {
        self.f.check("catalogue.find_agents")?;
        Ok(self.found.lock().unwrap().clone())
    }
    async fn check_proposal(&self, _repo: AgentRepositoryId, _agent: &str) -> Result<(), DepError> {
        self.f.check("catalogue.check_proposal")
    }
    async fn can_send_email(&self, id: AgentInstanceId) -> bool {
        self.send_email.lock().unwrap().contains(&id)
    }
    async fn add_trial(&self, input: TrialInput) -> Result<AgentInstanceId, DepError> {
        self.f.check("catalogue.add_trial")?;
        self.trials.lock().unwrap().push(input);
        Ok(AgentInstanceId::new())
    }
}

impl FakeCatalogue {
    /// Makes `name` (of version `version`) present in the workspace and known by identity.
    pub fn install(&self, a: &DirectoryAgent, id: &str, version: &str) {
        self.matches.lock().unwrap().insert(
            id.to_owned(),
            AgentMatch::Present {
                agent_instance_id: a.agent_instance_id,
                display_name: a.display_name.clone(),
            },
        );
        self.matches.lock().unwrap().insert(
            a.display_name.clone(),
            AgentMatch::Present {
                agent_instance_id: a.agent_instance_id,
                display_name: a.display_name.clone(),
            },
        );
        self.identities.lock().unwrap().insert(
            a.agent_instance_id,
            (
                AgentRepositoryRef::ProfileKey("company".to_owned()),
                id.to_owned(),
                version.to_owned(),
                a.display_name.clone(),
            ),
        );
    }
}

// ---------------------------------------------------------------- Memory Service

pub struct FakeMemory {
    pub f: Arc<Fails>,
    pub remembered: Mutex<Vec<(MemoryCaller, RememberInput)>>,
    pub receipt_state: Mutex<Option<ordin_memory_service::FactState>>,
    pub recalls: Mutex<Vec<(MemoryCaller, RecallInput)>>,
    pub recall: Mutex<Option<RecallResult>>,
    pub rules: Mutex<HashMap<AgentInstanceId, AgentMemoryRules>>,
    pub in_run: Mutex<Option<RememberedInRun>>,
    pub consolidated: Mutex<HashMap<String, Timestamp>>,
    pub facts: Mutex<Vec<FactView>>,
    /// Every `remember_facts` call, in order (amendment ADR-118).
    pub facts_remembered: Mutex<Vec<(MemoryCaller, RememberFactsInput)>>,
    /// What `vocabulary` answers; `None` is an empty vocabulary.
    pub vocabulary: Mutex<Option<VocabularyView>>,
    /// What `settings` answers (OH-MS-1).
    pub settings: Mutex<ordin_memory_service::MemorySettings>,
}

#[async_trait::async_trait]
impl MemoryPort for FakeMemory {
    fn settings(&self) -> ordin_memory_service::MemorySettings {
        *self.settings.lock().unwrap()
    }

    async fn remember_facts(
        &self,
        who: MemoryCaller,
        input: RememberFactsInput,
    ) -> Result<RememberFactsReceipt, DepError> {
        self.f.check("memory.remember_facts")?;
        let n = input.facts.len();
        self.facts_remembered.lock().unwrap().push((who, input));
        let state = self
            .receipt_state
            .lock()
            .unwrap()
            .unwrap_or(ordin_memory_service::FactState::Kept);
        Ok(RememberFactsReceipt {
            items: (0..n)
                .map(|_| RememberReceipt {
                    id: MemoryFactId::new(),
                    state,
                })
                .collect(),
        })
    }
    async fn vocabulary(&self, _place: MemoryPlace) -> Result<VocabularyView, DepError> {
        self.f.check("memory.vocabulary")?;
        Ok(self
            .vocabulary
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(VocabularyView { terms: vec![] }))
    }
    async fn remember(
        &self,
        who: MemoryCaller,
        input: RememberInput,
    ) -> Result<RememberReceipt, DepError> {
        self.f.check("memory.remember")?;
        self.remembered.lock().unwrap().push((who, input));
        Ok(RememberReceipt {
            id: MemoryFactId::new(),
            state: self
                .receipt_state
                .lock()
                .unwrap()
                .unwrap_or(ordin_memory_service::FactState::Kept),
        })
    }
    async fn recall(
        &self,
        who: MemoryCaller,
        input: RecallInput,
    ) -> Result<RecallResult, DepError> {
        self.f.check("memory.recall")?;
        self.recalls.lock().unwrap().push((who, input));
        self.recall
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| unset("memory.recall"))
    }
    fn set_agent_rules(
        &self,
        id: AgentInstanceId,
        rules: AgentMemoryRules,
    ) -> Result<(), DepError> {
        self.f.check("memory.set_agent_rules")?;
        self.rules.lock().unwrap().insert(id, rules);
        Ok(())
    }
    fn clear_agent_rules(&self, id: AgentInstanceId) {
        self.rules.lock().unwrap().remove(&id);
    }
    async fn remembered_in_run(&self, _run_id: RunId) -> Result<RememberedInRun, DepError> {
        self.f.check("memory.remembered_in_run")?;
        Ok(self
            .in_run
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(RememberedInRun { items: vec![] }))
    }
    async fn last_consolidated(
        &self,
        _workspace_id: WorkspaceId,
        thing: MemoryThingId,
    ) -> Result<Option<Timestamp>, DepError> {
        self.f.check("memory.last_consolidated")?;
        Ok(self.consolidated.lock().unwrap().get(&thing.0).copied())
    }
    async fn get_facts(
        &self,
        _workspace_id: WorkspaceId,
        ids: Vec<MemoryFactId>,
    ) -> Result<Vec<FactView>, DepError> {
        self.f.check("memory.get_facts")?;
        Ok(self
            .facts
            .lock()
            .unwrap()
            .iter()
            .filter(|f| ids.contains(&f.id))
            .cloned()
            .collect())
    }
}

// ---------------------------------------------------------------- Knowledge Service

pub struct FakeKnowledge {
    pub f: Arc<Fails>,
    pub written: Mutex<Vec<ordin_knowledge::WritePage>>,
    pub pages: Mutex<HashMap<KnowledgePageId, PageText>>,
    pub proposed: Mutex<Vec<(KnowledgePageId, String, RunId)>>,
}

#[async_trait::async_trait]
impl KnowledgePort for FakeKnowledge {
    async fn write_page(
        &self,
        req: ordin_knowledge::WritePage,
    ) -> Result<KnowledgePageId, DepError> {
        self.f.check("knowledge.write_page")?;
        self.written.lock().unwrap().push(req);
        Ok(KnowledgePageId::new())
    }
    async fn get_page(&self, page_id: KnowledgePageId) -> Result<PageText, DepError> {
        self.f.check("knowledge.get_page")?;
        self.pages
            .lock()
            .unwrap()
            .get(&page_id)
            .cloned()
            .ok_or_else(|| DepError::new("KNOW_PAGE_UNKNOWN", false))
    }
    async fn propose_update(
        &self,
        page_id: KnowledgePageId,
        body: String,
        _sources: Vec<ordin_types::domain::places::KnowledgeSource>,
        run_id: RunId,
    ) -> Result<(), DepError> {
        self.f.check("knowledge.propose_update")?;
        self.proposed.lock().unwrap().push((page_id, body, run_id));
        Ok(())
    }
}

// ---------------------------------------------------------------- Push Service

pub struct FakePush {
    pub f: Arc<Fails>,
    pub proposals: Mutex<Vec<ordin_push::PushProposal>>,
    pub pulls: Mutex<Vec<ordin_push::PullRequestProposal>>,
    pub notices: broadcast::Sender<ordin_push::PushNotice>,
    /// OH-PF-1: the items sent again, in order.
    pub tried_again: Mutex<Vec<(PendingPushId, ordin_types::WorkingCopyItemId)>>,
}

#[async_trait::async_trait]
impl PushPort for FakePush {
    async fn propose(&self, proposal: ordin_push::PushProposal) -> Result<PendingPushId, DepError> {
        self.f.check("push.propose")?;
        self.proposals.lock().unwrap().push(proposal);
        Ok(PendingPushId::new())
    }
    async fn propose_pull_request(
        &self,
        req: ordin_push::PullRequestProposal,
    ) -> Result<ordin_push::SubmissionId, DepError> {
        self.f.check("push.propose_pull_request")?;
        self.pulls.lock().unwrap().push(req);
        Ok(ordin_push::SubmissionId::new())
    }
    fn subscribe(&self) -> broadcast::Receiver<ordin_push::PushNotice> {
        self.notices.subscribe()
    }
    async fn try_again(
        &self,
        pending_push_id: PendingPushId,
        item: ordin_types::WorkingCopyItemId,
    ) -> Result<(), DepError> {
        self.f.check("push.try_again")?;
        self.tried_again
            .lock()
            .unwrap()
            .push((pending_push_id, item));
        Ok(())
    }
}

// ---------------------------------------------------------------- Project Service

pub struct FakeProjects {
    pub f: Arc<Fails>,
    pub projects: Mutex<HashMap<PlaceId, (ProjectId, bool)>>,
    pub coding: Mutex<Vec<ordin_projects::StartCodingTask>>,
    pub files: Mutex<Vec<ordin_projects::RepoFile>>,
    /// By file name: bytes and commit.
    pub contents: Mutex<HashMap<String, (Vec<u8>, String)>>,
}

#[async_trait::async_trait]
impl ProjectsPort for FakeProjects {
    async fn project_for_place(
        &self,
        place_id: PlaceId,
    ) -> Result<Option<(ProjectId, bool)>, DepError> {
        self.f.check("projects.project_for_place")?;
        Ok(self.projects.lock().unwrap().get(&place_id).copied())
    }
    async fn start_coding_task(
        &self,
        req: ordin_projects::StartCodingTask,
    ) -> Result<CodingTaskId, DepError> {
        self.f.check("projects.start_coding_task")?;
        self.coding.lock().unwrap().push(req);
        Ok(CodingTaskId::new())
    }
    async fn is_coding_task(&self, task_id: TaskId) -> Result<bool, DepError> {
        self.f.check("projects.is_coding_task")?;
        let origin = ordin_types::domain::RunOrigin::Task { task_id };
        Ok(self
            .coding
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.origin == origin))
    }
    async fn list_files(
        &self,
        _folder: &ordin_projects::RepoFolderRef,
        _glob: &str,
    ) -> Result<Vec<ordin_projects::RepoFile>, DepError> {
        self.f.check("projects.list_files")?;
        Ok(self.files.lock().unwrap().clone())
    }
    async fn read_file(
        &self,
        _folder: &ordin_projects::RepoFolderRef,
        name: &str,
    ) -> Result<(Vec<u8>, String), DepError> {
        self.f.check("projects.read_file")?;
        self.contents
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .ok_or_else(|| DepError::new("PROJ_FILE_UNKNOWN", false))
    }
    async fn file_last_commit(
        &self,
        _folder: &ordin_projects::RepoFolderRef,
        name: &str,
    ) -> Result<String, DepError> {
        self.f.check("projects.file_last_commit")?;
        self.contents
            .lock()
            .unwrap()
            .get(name)
            .map(|c| c.1.clone())
            .ok_or_else(|| DepError::new("PROJ_FILE_UNKNOWN", false))
    }
    async fn default_branch(&self, _host: &str, _repository: &str) -> Result<String, DepError> {
        self.f.check("projects.default_branch")?;
        Ok("main".to_owned())
    }
}

// ---------------------------------------------------------------- File Service

pub struct FakeFiles {
    pub f: Arc<Fails>,
    /// Paths that answer `FILE_NOT_FOUND` (OH-TF-1 `document_gone`).
    pub missing: Mutex<Vec<String>>,
    /// OH-TW-1: the places `place` knows, with their paths; any other answers `FILE_PLACE_NOT_FOUND`.
    pub places: Mutex<HashMap<PlaceId, String>>,
}

#[async_trait::async_trait]
impl FilesPort for FakeFiles {
    async fn read_file(&self, _place_id: PlaceId, path: &str) -> Result<PlaceFile, DepError> {
        self.f.check("files.read_file")?;
        if self.missing.lock().unwrap().iter().any(|m| m == path) {
            return Err(DepError::new("FILE_NOT_FOUND", false));
        }
        use sha2::Digest;
        let hex = format!("{:x}", sha2::Sha256::digest(path.as_bytes()));
        Ok(PlaceFile {
            hash: ordin_types::num::Sha256Hex::parse(&hex).expect("a sha-256 in hex"),
            media_type: "text/plain".to_owned(),
        })
    }
    async fn place(
        &self,
        place_id: PlaceId,
    ) -> Result<ordin_types::domain::places::Place, DepError> {
        self.f.check("files.place")?;
        let path = self
            .places
            .lock()
            .unwrap()
            .get(&place_id)
            .cloned()
            .ok_or_else(|| DepError::new("FILE_PLACE_NOT_FOUND", false))?;
        Ok(ordin_types::domain::places::Place {
            id: place_id,
            friendly_name: "Results".to_owned(),
            path,
            kind: ordin_types::domain::places::PlaceKind::WorkingCopy,
            added_at: Timestamp::now(),
        })
    }
}

// ---------------------------------------------------------------- Localisation

pub struct FakeLocalisation {
    pub f: Arc<Fails>,
    pub active: Mutex<String>,
    pub set: Mutex<Vec<String>>,
    pub matches: Mutex<HashMap<String, Match>>,
    pub sources: Mutex<Vec<Source>>,
    pub began: Mutex<Vec<(String, String, u32)>>,
    pub drafted: Mutex<Vec<String>>,
    pub ended: Mutex<Vec<(String, DraftEnding)>>,
}

#[async_trait::async_trait]
impl LocalisationPort for FakeLocalisation {
    fn active_locale(&self) -> String {
        self.active.lock().unwrap().clone()
    }
    async fn set_locale(&self, locale: &str) -> Result<String, DepError> {
        self.f.check("localisation.set_locale")?;
        self.set.lock().unwrap().push(locale.to_owned());
        *self.active.lock().unwrap() = locale.to_owned();
        Ok(locale.to_owned())
    }
    fn message(&self, message: &LocMessage) -> String {
        // The minutes words are the en-GB catalogue's, so tests read the body as a person would.
        #[cfg(feature = "minutes")]
        {
            let word = match message {
                LocMessage::MinutesHeadingSummary => Some("Summary"),
                LocMessage::MinutesHeadingDecisions => Some("Decisions"),
                LocMessage::MinutesHeadingActions => Some("Actions"),
                LocMessage::MinutesHeadingOpenQuestions => Some("Open questions"),
                LocMessage::MinutesOwnerUnassigned => Some("unassigned"),
                LocMessage::MinutesNoneRecorded => Some("None recorded."),
                _ => None,
            };
            if let Some(word) = word {
                return word.to_owned();
            }
        }
        format!("{message:?}")
    }
    fn match_language(&self, requested: &str) -> Result<Match, DepError> {
        self.f.check("localisation.match_language")?;
        Ok(self
            .matches
            .lock()
            .unwrap()
            .get(requested)
            .cloned()
            .unwrap_or_else(|| Match::Choosable(requested.to_owned())))
    }
    fn draft_sources(&self, _locale: &str) -> Result<Vec<Source>, DepError> {
        self.f.check("localisation.draft_sources")?;
        Ok(self.sources.lock().unwrap().clone())
    }
    async fn begin_draft(&self, locale: &str, name: &str, total: u32) -> Result<(), DepError> {
        self.f.check("localisation.begin_draft")?;
        self.began
            .lock()
            .unwrap()
            .push((locale.to_owned(), name.to_owned(), total));
        Ok(())
    }
    async fn add_drafted(&self, _locale: &str, reply: &str) -> Result<(u32, u32), DepError> {
        self.f.check("localisation.add_drafted")?;
        self.drafted.lock().unwrap().push(reply.to_owned());
        let lines =
            u32::try_from(reply.lines().filter(|l| !l.trim().is_empty()).count()).unwrap_or(0);
        Ok((lines, 0))
    }
    async fn end_draft(&self, locale: &str, end: DraftEnding) -> Result<u32, DepError> {
        self.f.check("localisation.end_draft")?;
        self.ended.lock().unwrap().push((locale.to_owned(), end));
        Ok(0)
    }
}

// ---------------------------------------------------------------- block check

/// Accepts every block that deserialises as a `Block`; a test lists rejections to refuse it.
pub struct FakeBlocks {
    pub f: Arc<Fails>,
    pub reject: Mutex<Vec<BlockRejection>>,
}

#[async_trait::async_trait]
impl BlocksPort for FakeBlocks {
    async fn check_block(
        &self,
        raw: &serde_json::Value,
        _ctx: &BlockCheckContext,
    ) -> Result<Block, Vec<BlockRejection>> {
        let _ = self.f.check("blocks.check_block");
        let reject = self.reject.lock().unwrap().clone();
        if !reject.is_empty() {
            return Err(reject);
        }
        serde_json::from_value(raw.clone()).map_err(|e| {
            vec![BlockRejection {
                code: "BLOCK_SHAPE_INVALID".to_owned(),
                at: String::new(),
                reason: e.to_string(),
            }]
        })
    }
}

// ---------------------------------------------------------------- every stand-in

/// One stand-in per port, sharing one [`Fails`].
pub struct Fakes {
    pub f: Arc<Fails>,
    pub supervisor: Arc<FakeSupervisor>,
    pub orchestrator: Arc<FakeOrchestrator>,
    pub inspector: Arc<FakeInspector>,
    pub usage: Arc<FakeUsage>,
    pub workspaces: Arc<FakeWorkspaces>,
    pub journal: Arc<FakeJournal>,
    pub catalogue: Arc<FakeCatalogue>,
    pub memory: Arc<FakeMemory>,
    pub knowledge: Arc<FakeKnowledge>,
    pub push: Arc<FakePush>,
    pub projects: Arc<FakeProjects>,
    pub localisation: Arc<FakeLocalisation>,
    pub blocks: Arc<FakeBlocks>,
    pub files: Arc<FakeFiles>,
    pub ports: Ports,
}

impl Fakes {
    pub fn new() -> Fakes {
        let f = Arc::new(Fails::default());
        let supervisor = Arc::new(FakeSupervisor::new(f.clone()));
        let orchestrator = Arc::new(FakeOrchestrator::new(f.clone()));
        let inspector = Arc::new(FakeInspector::new(f.clone()));
        let usage = Arc::new(FakeUsage::new(f.clone()));
        let workspaces = Arc::new(FakeWorkspaces {
            f: f.clone(),
            name: Mutex::new("Launch".to_owned()),
            places: Mutex::new(vec!["Docs".to_owned()]),
            repos: Mutex::new(vec![]),
            folders: Mutex::new(vec![]),
            changed: Mutex::new(vec![]),
            documents: Mutex::new(vec![]),
            grants_set: Mutex::new(vec![]),
            results_folder: Mutex::new(None),
            grants: Mutex::new(vec![]),
        });
        let journal = Arc::new(FakeJournal {
            f: f.clone(),
            events: Mutex::new(vec![]),
            entries: Mutex::new(vec![]),
            dashboards: Mutex::new(HashMap::new()),
            refreshes: Mutex::new(vec![]),
            reports: Mutex::new(vec![]),
            kept: Mutex::new(vec![]),
            artefacts: Mutex::new(vec![]),
            external_actions: Mutex::new(0),
            artefact_names: Mutex::new(vec![]),
            unknown_outcome: Mutex::new(vec![]),
            run_codes: Mutex::new(HashMap::new()),
            #[cfg(feature = "minutes")]
            minutes: Mutex::new(vec![]),
        });
        let catalogue = Arc::new(FakeCatalogue {
            f: f.clone(),
            pending: Mutex::new(vec![]),
            answers: Mutex::new(vec![]),
            matches: Mutex::new(HashMap::new()),
            identities: Mutex::new(HashMap::new()),
            found: Mutex::new(vec![]),
            send_email: Mutex::new(vec![]),
            trials: Mutex::new(vec![]),
        });
        let memory = Arc::new(FakeMemory {
            f: f.clone(),
            remembered: Mutex::new(vec![]),
            receipt_state: Mutex::new(None),
            recalls: Mutex::new(vec![]),
            recall: Mutex::new(None),
            rules: Mutex::new(HashMap::new()),
            in_run: Mutex::new(None),
            consolidated: Mutex::new(HashMap::new()),
            facts: Mutex::new(vec![]),
            facts_remembered: Mutex::new(vec![]),
            vocabulary: Mutex::new(None),
            settings: Mutex::new(ordin_memory_service::MemorySettings::default()),
        });
        let knowledge = Arc::new(FakeKnowledge {
            f: f.clone(),
            written: Mutex::new(vec![]),
            pages: Mutex::new(HashMap::new()),
            proposed: Mutex::new(vec![]),
        });
        let push = Arc::new(FakePush {
            f: f.clone(),
            proposals: Mutex::new(vec![]),
            pulls: Mutex::new(vec![]),
            notices: broadcast::channel(16).0,
            tried_again: Mutex::new(vec![]),
        });
        let projects = Arc::new(FakeProjects {
            f: f.clone(),
            projects: Mutex::new(HashMap::new()),
            coding: Mutex::new(vec![]),
            files: Mutex::new(vec![]),
            contents: Mutex::new(HashMap::new()),
        });
        let localisation = Arc::new(FakeLocalisation {
            f: f.clone(),
            active: Mutex::new("en-GB".to_owned()),
            set: Mutex::new(vec![]),
            matches: Mutex::new(HashMap::new()),
            sources: Mutex::new(vec![]),
            began: Mutex::new(vec![]),
            drafted: Mutex::new(vec![]),
            ended: Mutex::new(vec![]),
        });
        let blocks = Arc::new(FakeBlocks {
            f: f.clone(),
            reject: Mutex::new(vec![]),
        });
        let files = Arc::new(FakeFiles {
            f: f.clone(),
            missing: Mutex::new(vec![]),
            places: Mutex::new(HashMap::new()),
        });
        let ports = Ports {
            supervisor: supervisor.clone(),
            orchestrator: orchestrator.clone(),
            inspector: inspector.clone(),
            usage: usage.clone(),
            workspaces: workspaces.clone(),
            journal: journal.clone(),
            catalogue: catalogue.clone(),
            memory: memory.clone(),
            knowledge: knowledge.clone(),
            push: push.clone(),
            projects: projects.clone(),
            files: files.clone(),
            localisation: localisation.clone(),
            blocks: blocks.clone(),
        };
        Fakes {
            f,
            supervisor,
            orchestrator,
            inspector,
            usage,
            workspaces,
            journal,
            catalogue,
            memory,
            knowledge,
            push,
            projects,
            localisation,
            blocks,
            files,
            ports,
        }
    }
}

impl Default for Fakes {
    fn default() -> Self {
        Fakes::new()
    }
}

pub fn limits(rounds: u16, minutes: u16, members: u8) -> CrewLimitPolicy {
    CrewLimitPolicy {
        rounds,
        wall_clock_minutes: minutes,
        members_at_once: members,
        request_budget: None,
        failure_repeat_limit: 3,
    }
}

pub fn config() -> OrdinHostConfig {
    OrdinHostConfig {
        ordin_agent_instance_id: AgentInstanceId::new(),
        agent_maker_instance_id: AgentInstanceId::new(),
        harness: HarnessKind::ClaudeCode,
        profile: ProgramProfile::Desktop,
        fixed_instructions_version: crate::FIXED_INSTRUCTIONS_VERSION.to_owned(),
        coordinator_turn_timeout: Duration::from_secs(5),
        repository_check_period: Duration::from_secs(900),
        max_waiting_messages: 5,
    }
}

pub fn policy() -> OrdinHostPolicy {
    OrdinHostPolicy {
        coordinator_name: "Ordin".to_owned(),
        voice_defaults: CompanyVoice::default(),
        company_workflow_repository: None,
        crew_defaults: limits(10, 30, 3),
        crew_maxima: limits(20, 60, 4),
        repository_profile_keys: vec!["company".to_owned()],
    }
}
