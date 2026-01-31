//! Shared test helpers: a migrated SQLite datastore with the real Journal, Usage Planner and Run
//! Queue; a channel-driven adapter whose events the test pushes; a recording host; a Run Queue
//! wrapper whose `renew` answer the test sets; and wait helpers.
#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::stream::{BoxStream, StreamExt};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, EntryFilter, Journal, JournalOptions, MigrationRegistry,
    SystemClock, MIGRATIONS,
};
use ordin_organisation_profile::{OrganisationProfileService, ProfileMode, ProfileOptions};
use ordin_run_queue::{
    CancelOutcome, DatastoreRunQueue, DeletedRuns, InProgressFilter, LeaseRenewal, NewRun, RunEnd,
    RunOutcome, RunPause, RunQueue, RunQueueError, RunView as QueueView, StoredRunEvent, TakenRun,
};
use ordin_runtime_adapter::{
    check_verb, AdapterContext, AdapterError, AdapterKind, AdapterVerb, ResumeDecision,
    RunEndReason, RunEvent, RunHandle, RunLimits, RunSnapshot, RunTarget, RuntimeAdapter,
    StartInput, StartRun,
};
use ordin_session_supervisor::{
    ContentCheck, ContentPoint, HostError, ProgramKind, RunControls, SessionSupervisor,
    SupervisorConfig, SupervisorHost,
};
use ordin_types::domain::agent_profile::{
    CallPlace, ModelFailureCause, RunRecord, RunRecordState, RunRequest, RunStarter,
};
use ordin_types::domain::runs::{Run, RunOrigin, RunState, Task, TaskTarget};
use ordin_types::paging::Page;
use ordin_types::time::Timestamp;
use ordin_types::wire::startup::StartUpReportItem;
use ordin_types::{AgentInstanceId, RemoteAgentId, RunId, ScopeId, TaskId, WorkspaceId};
use ordin_usage_planner::{UsagePlanner, WorkKind};
use tokio::sync::mpsc;

pub const SAFE_REPLY: &str = "I can't answer that here.";

/// A folder removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!("ordin-sup-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).expect("temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Calls seen by the host and the adapters, in order ("run_starting", "start", …).
pub type CallLog = Arc<Mutex<Vec<String>>>;

type Channel = (
    mpsc::UnboundedSender<RunEvent>,
    Option<mpsc::UnboundedReceiver<RunEvent>>,
);

/// An adapter the test drives: `start` records the request and (by default) sends `Started`;
/// every other event is pushed by the test with [`TestAdapter::push`].
pub struct TestAdapter {
    pub kind: AdapterKind,
    pub unsupported: &'static [AdapterVerb],
    pub calls: CallLog,
    pub starts: Mutex<Vec<StartRun>>,
    pub resumes: Mutex<Vec<(RunId, ResumeDecision)>>,
    pub cancels: Mutex<Vec<RunId>>,
    pub start_error: Mutex<Option<AdapterError>>,
    pub resume_error: Mutex<Option<AdapterError>>,
    /// None: `read` answers `UnknownRun`.
    pub read_answer: Mutex<Option<RunSnapshot>>,
    /// Send `Ended { Cancelled }` when `cancel` is called.
    pub end_on_cancel: AtomicBool,
    /// Send `Started` when `start` is called.
    pub auto_started: AtomicBool,
    channels: Mutex<HashMap<RunId, Channel>>,
}

impl TestAdapter {
    pub fn new(
        kind: AdapterKind,
        unsupported: &'static [AdapterVerb],
        calls: CallLog,
    ) -> Arc<Self> {
        Arc::new(TestAdapter {
            kind,
            unsupported,
            calls,
            starts: Mutex::new(vec![]),
            resumes: Mutex::new(vec![]),
            cancels: Mutex::new(vec![]),
            start_error: Mutex::new(None),
            resume_error: Mutex::new(None),
            read_answer: Mutex::new(None),
            end_on_cancel: AtomicBool::new(true),
            auto_started: AtomicBool::new(true),
            channels: Mutex::new(HashMap::new()),
        })
    }

    pub fn session_of(run_id: RunId) -> String {
        format!("s-{run_id}")
    }

    fn fresh(&self, run_id: RunId) {
        let (tx, rx) = mpsc::unbounded_channel();
        self.channels.lock().unwrap().insert(run_id, (tx, Some(rx)));
    }

    /// Sends one event on the run's stream (creating the stream when none exists yet).
    pub fn push(&self, run_id: RunId, event: RunEvent) {
        let mut channels = self.channels.lock().unwrap();
        let entry = channels.entry(run_id).or_insert_with(|| {
            let (tx, rx) = mpsc::unbounded_channel();
            (tx, Some(rx))
        });
        let _ = entry.0.send(event);
    }

    pub fn started(&self, run_id: RunId) -> RunEvent {
        RunEvent::Started {
            runtime_session_id: Some(Self::session_of(run_id)),
            remote_thread_id: (self.kind == AdapterKind::HostedAgent
                || self.kind == AdapterKind::SlackThread)
                .then(|| "t-1".to_owned()),
        }
    }

    pub fn end(&self, run_id: RunId, reason: RunEndReason, message: Option<&str>) {
        self.push(
            run_id,
            RunEvent::Ended {
                end: ordin_runtime_adapter::RunEnd {
                    reason,
                    final_message: message.map(str::to_owned),
                    artefacts: vec![],
                },
            },
        );
    }

    /// Ends the run's stream without `Ended` (the harness exited).
    pub fn close(&self, run_id: RunId) {
        self.channels.lock().unwrap().remove(&run_id);
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
}

#[async_trait::async_trait]
impl RuntimeAdapter for TestAdapter {
    fn kind(&self) -> AdapterKind {
        self.kind
    }

    fn unsupported_verbs(&self) -> &'static [AdapterVerb] {
        self.unsupported
    }

    async fn start(&self, _ctx: &AdapterContext, req: StartRun) -> Result<RunHandle, AdapterError> {
        check_verb(self, AdapterVerb::Start)?;
        self.calls.lock().unwrap().push("start".into());
        let run_id = req.run_id;
        if let Some(error) = self.start_error.lock().unwrap().clone() {
            self.starts.lock().unwrap().push(req);
            return Err(error);
        }
        self.fresh(run_id);
        if self.auto_started.load(Ordering::SeqCst) {
            self.push(run_id, self.started(run_id));
        }
        self.starts.lock().unwrap().push(req);
        Ok(RunHandle {
            run_id,
            adapter: self.kind,
            runtime_session_id: Some(Self::session_of(run_id)),
            remote_thread_id: None,
        })
    }

    fn events(
        &self,
        _ctx: &AdapterContext,
        run: &RunHandle,
    ) -> Result<BoxStream<'static, RunEvent>, AdapterError> {
        let mut channels = self.channels.lock().unwrap();
        let entry = channels.entry(run.run_id).or_insert_with(|| {
            let (tx, rx) = mpsc::unbounded_channel();
            (tx, Some(rx))
        });
        let rx = match entry.1.take() {
            Some(rx) => rx,
            None => {
                let (tx, rx) = mpsc::unbounded_channel();
                entry.0 = tx;
                rx
            }
        };
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
        self.calls.lock().unwrap().push("resume".into());
        self.resumes.lock().unwrap().push((run.run_id, decision));
        match self.resume_error.lock().unwrap().clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    async fn read(
        &self,
        _ctx: &AdapterContext,
        _run: &RunHandle,
    ) -> Result<RunSnapshot, AdapterError> {
        check_verb(self, AdapterVerb::Read)?;
        self.read_answer
            .lock()
            .unwrap()
            .clone()
            .ok_or(AdapterError::UnknownRun)
    }

    async fn cancel(&self, _ctx: &AdapterContext, run: &RunHandle) -> Result<(), AdapterError> {
        check_verb(self, AdapterVerb::Cancel)?;
        self.cancels.lock().unwrap().push(run.run_id);
        if self.end_on_cancel.load(Ordering::SeqCst) {
            self.end(run.run_id, RunEndReason::Cancelled, None);
        }
        Ok(())
    }
}

/// A host that records what it is asked and answers what the test sets.
pub struct TestHost {
    pub calls: CallLog,
    pub routes: Mutex<HashMap<RemoteAgentId, AdapterKind>>,
    pub resuming: Mutex<Option<String>>,
    pub ended: Mutex<Vec<RunId>>,
    pub abandoned: Mutex<Vec<(RunId, AdapterKind)>>,
    /// Answers of `check_content`, one per call; `Passed` once empty.
    pub checks: Mutex<VecDeque<ContentCheck>>,
    pub checked: Mutex<Vec<(ContentPoint, String)>>,
    pub retry: AtomicBool,
    pub starting_delay: Mutex<Duration>,
    pub starting: Mutex<Vec<StartRun>>,
    pub expansion: Mutex<Result<String, String>>,
    pub items: Mutex<Vec<StartUpReportItem>>,
    pub controls: Mutex<RunControls>,
}

impl TestHost {
    pub fn new(calls: CallLog) -> Arc<Self> {
        Arc::new(TestHost {
            calls,
            routes: Mutex::new(HashMap::new()),
            resuming: Mutex::new(None),
            ended: Mutex::new(vec![]),
            abandoned: Mutex::new(vec![]),
            checks: Mutex::new(VecDeque::new()),
            checked: Mutex::new(vec![]),
            retry: AtomicBool::new(true),
            starting_delay: Mutex::new(Duration::ZERO),
            starting: Mutex::new(vec![]),
            expansion: Mutex::new(Ok("Compare the three quotes.".into())),
            items: Mutex::new(vec![]),
            controls: Mutex::new(RunControls::default()),
        })
    }
}

#[async_trait::async_trait]
impl SupervisorHost for TestHost {
    async fn startup_items(
        &self,
        _run: &Run,
        _target: &RunTarget,
        _unsupported: &[AdapterVerb],
    ) -> Vec<StartUpReportItem> {
        self.items.lock().unwrap().clone()
    }

    fn remote_route(&self, remote_agent_id: RemoteAgentId) -> Option<AdapterKind> {
        self.routes.lock().unwrap().get(&remote_agent_id).copied()
    }

    async fn expand_command(
        &self,
        _target: &RunTarget,
        _name: &str,
        _arguments: &str,
    ) -> Result<String, HostError> {
        self.expansion
            .lock()
            .unwrap()
            .clone()
            .map_err(|code| HostError { code })
    }

    async fn run_controls(&self, _run_id: RunId) -> RunControls {
        self.controls.lock().unwrap().clone()
    }

    async fn run_resuming(&self, _run_id: RunId) -> Option<String> {
        self.resuming.lock().unwrap().clone()
    }

    async fn run_ended(&self, run_id: RunId) {
        self.ended.lock().unwrap().push(run_id);
    }

    fn run_abandoned(&self, run_id: RunId, adapter: AdapterKind) {
        self.abandoned.lock().unwrap().push((run_id, adapter));
    }

    fn reply_for(&self, _cause: ModelFailureCause, _place: CallPlace) -> Option<String> {
        Some(SAFE_REPLY.into())
    }

    fn retry_after_block(&self, _carried: bool, already_retried: bool) -> bool {
        self.retry.load(Ordering::SeqCst) && !already_retried
    }

    async fn check_content(&self, _run_id: RunId, point: ContentPoint, text: &str) -> ContentCheck {
        self.checked.lock().unwrap().push((point, text.to_owned()));
        self.checks
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(ContentCheck::Passed)
    }

    async fn run_starting(&self, start: &StartRun) {
        self.calls.lock().unwrap().push("run_starting".into());
        self.starting.lock().unwrap().push(start.clone());
        let delay = *self.starting_delay.lock().unwrap();
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
    }
}

/// A counter sink for the Run Queue that keeps nothing.
pub struct NoMetrics;

impl ordin_run_queue::MetricsSink for NoMetrics {
    fn add(&self, _name: &'static str, _labels: &[(&'static str, &str)], _by: u64) {}
}

/// The supervisor's counter sink, keeping every call.
#[derive(Default)]
pub struct Counters(pub Mutex<Vec<Counted>>);

/// One counter call: name and labels.
pub type Counted = (String, Vec<(String, String)>);

impl ordin_session_supervisor::MetricsSink for Counters {
    fn add(&self, name: &str, labels: &[(&str, &str)], _by: u64) {
        self.0.lock().unwrap().push((
            name.to_owned(),
            labels
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        ));
    }
}

impl Counters {
    pub fn count(&self, name: &str) -> usize {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(n, _)| n == name)
            .count()
    }
}

/// The real Run Queue, with a `renew` answer the test may set and every progress line kept.
pub struct TestQueue {
    pub inner: DatastoreRunQueue,
    pub renew_answer: Mutex<Option<LeaseRenewal>>,
    pub progress: Mutex<Vec<(String, String, String)>>,
}

#[async_trait::async_trait]
impl RunQueue for TestQueue {
    async fn enqueue(&self, new: NewRun) -> Result<RunRecord, RunQueueError> {
        self.inner.enqueue(new).await
    }
    async fn enqueue_firing(
        &self,
        entry_key: &str,
        due: Timestamp,
        scope_id: ScopeId,
        command: &str,
        arguments: &str,
    ) -> Result<Option<RunRecord>, RunQueueError> {
        self.inner
            .enqueue_firing(entry_key, due, scope_id, command, arguments)
            .await
    }
    async fn thread_owner(&self, thread_id: &str) -> Result<Option<RunStarter>, RunQueueError> {
        self.inner.thread_owner(thread_id).await
    }
    async fn take(&self, instance: &str) -> Result<Option<TakenRun>, RunQueueError> {
        self.inner.take(instance).await
    }
    async fn renew(&self, run_id: &str, instance: &str) -> Result<LeaseRenewal, RunQueueError> {
        if let Some(answer) = *self.renew_answer.lock().unwrap() {
            return Ok(answer);
        }
        self.inner.renew(run_id, instance).await
    }
    async fn end(&self, run_id: &str, instance: &str, end: RunEnd) -> Result<(), RunQueueError> {
        self.inner.end(run_id, instance, end).await
    }
    async fn pause(
        &self,
        run_id: &str,
        instance: &str,
        pause: RunPause,
    ) -> Result<(), RunQueueError> {
        self.inner.pause(run_id, instance, pause).await
    }
    async fn resume(
        &self,
        run_id: &str,
        decision: serde_json::Value,
    ) -> Result<RunRecord, RunQueueError> {
        self.inner.resume(run_id, decision).await
    }
    async fn cancel(&self, run_id: &str) -> Result<CancelOutcome, RunQueueError> {
        self.inner.cancel(run_id).await
    }
    async fn record_cancelled(
        &self,
        run_id: &str,
        instance: &str,
        outcome: RunOutcome,
    ) -> Result<(), RunQueueError> {
        self.inner.record_cancelled(run_id, instance, outcome).await
    }
    async fn decision(&self, run_id: &str) -> Result<Option<serde_json::Value>, RunQueueError> {
        self.inner.decision(run_id).await
    }
    async fn get(&self, run_id: &str) -> Result<Option<QueueView>, RunQueueError> {
        self.inner.get(run_id).await
    }
    async fn list(
        &self,
        states: Option<&[RunRecordState]>,
        cursor: Option<&str>,
    ) -> Result<Page<QueueView>, RunQueueError> {
        self.inner.list(states, cursor).await
    }
    async fn in_progress(&self, filter: &InProgressFilter) -> Result<u64, RunQueueError> {
        self.inner.in_progress(filter).await
    }
    async fn paused_with_command(
        &self,
        commands: &[String],
        except_run_id: &str,
    ) -> Result<bool, RunQueueError> {
        self.inner
            .paused_with_command(commands, except_run_id)
            .await
    }
    async fn mark_interrupted(&self) -> Result<Vec<String>, RunQueueError> {
        self.inner.mark_interrupted().await
    }
    async fn append_progress(
        &self,
        run_id: &str,
        instance: &str,
        line: &str,
    ) -> Result<bool, RunQueueError> {
        self.progress
            .lock()
            .unwrap()
            .push((run_id.into(), instance.into(), line.into()));
        self.inner.append_progress(run_id, instance, line).await
    }
    async fn events_after(
        &self,
        run_id: &str,
        after_seq: u64,
        limit: u32,
    ) -> Result<Vec<StoredRunEvent>, RunQueueError> {
        self.inner.events_after(run_id, after_seq, limit).await
    }
    async fn count_ended_before(
        &self,
        cutoff: Timestamp,
        limit: u32,
    ) -> Result<DeletedRuns, RunQueueError> {
        self.inner.count_ended_before(cutoff, limit).await
    }
    async fn delete_ended_before(
        &self,
        cutoff: Timestamp,
        limit: u32,
    ) -> Result<DeletedRuns, RunQueueError> {
        self.inner.delete_ended_before(cutoff, limit).await
    }
    async fn count_started_by_asker(
        &self,
        connection: &str,
        asker: &str,
    ) -> Result<DeletedRuns, RunQueueError> {
        self.inner.count_started_by_asker(connection, asker).await
    }
    async fn delete_started_by_asker(
        &self,
        connection: &str,
        asker: &str,
    ) -> Result<DeletedRuns, RunQueueError> {
        self.inner.delete_started_by_asker(connection, asker).await
    }
    async fn count_in_scope(&self, scope_id: ScopeId) -> Result<DeletedRuns, RunQueueError> {
        self.inner.count_in_scope(scope_id).await
    }
    async fn delete_in_scope(&self, scope_id: ScopeId) -> Result<DeletedRuns, RunQueueError> {
        self.inner.delete_in_scope(scope_id).await
    }
}

struct Now;

impl ordin_organisation_profile::Clock for Now {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

pub fn config(program: ProgramKind, instance: &str) -> SupervisorConfig {
    SupervisorConfig {
        program,
        local_adapter: AdapterKind::ClaudeLocal,
        local_session_limit: 8,
        instance_name: instance.into(),
        default_wall_clock_secs: 3_600,
        idle_timeout_secs: 600,
        stop_deadline: Duration::from_secs(1),
    }
}

/// One supervisor with its adapters and host, over a shared store.
pub struct Node {
    pub sup: Arc<SessionSupervisor>,
    pub local: Arc<TestAdapter>,
    pub hosted: Arc<TestAdapter>,
    pub slack: Arc<TestAdapter>,
    pub host: Arc<TestHost>,
    pub calls: CallLog,
    pub counters: Arc<Counters>,
}

pub struct Harness {
    pub store: Datastore,
    pub journal: Arc<Journal>,
    pub usage: Arc<UsagePlanner>,
    pub queue: Arc<TestQueue>,
    pub node: Node,
    _dir: TempDir,
}

impl std::ops::Deref for Harness {
    type Target = Node;
    fn deref(&self) -> &Node {
        &self.node
    }
}

impl Harness {
    pub async fn desktop() -> Harness {
        Harness::with(config(ProgramKind::Desktop, "desktop")).await
    }

    pub async fn agent(program: ProgramKind, instance: &str) -> Harness {
        Harness::with(config(program, instance)).await
    }

    pub async fn with(config: SupervisorConfig) -> Harness {
        let dir = TempDir::new();
        let store = Datastore::open(DatastoreConfig::Sqlite {
            path: dir.0.join("data").join("journal.db"),
            backup_dir: dir.0.join("data").join("backups"),
        })
        .await
        .expect("open the datastore");
        let mut registry = MigrationRegistry::new();
        registry.register(MIGRATIONS).unwrap();
        registry.register(UsagePlanner::migrations()).unwrap();
        registry.register(ordin_run_queue::migrations()).unwrap();
        registry.register(SessionSupervisor::migrations()).unwrap();
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
        let usage = UsagePlanner::new(store.clone(), journal.clone(), profile, vec![])
            .await
            .unwrap();
        let queue = Arc::new(TestQueue {
            inner: DatastoreRunQueue::new(&store, Arc::new(NoMetrics)),
            renew_answer: Mutex::new(None),
            progress: Mutex::new(vec![]),
        });
        let node = node(&store, &journal, &usage, &queue, config);
        Harness {
            store,
            journal,
            usage,
            queue,
            node,
            _dir: dir,
        }
    }

    /// A second instance over the same store, journal and Run Queue (agent profile).
    pub fn another(&self, config: SupervisorConfig) -> Node {
        node(&self.store, &self.journal, &self.usage, &self.queue, config)
    }

    pub fn pool(&self) -> &sqlx::SqlitePool {
        self.store.sqlite().expect("SQLite")
    }

    pub async fn count(&self, sql: &'static str) -> i64 {
        sqlx::query_scalar::<_, i64>(sql)
            .fetch_one(self.pool())
            .await
            .unwrap()
    }

    /// The codes journalled with a run (entries `run_code`).
    pub async fn codes(&self, run_id: RunId) -> Vec<String> {
        self.entries(run_id, "run_code")
            .await
            .iter()
            .filter_map(|e| e["code"].as_str().map(str::to_owned))
            .collect()
    }

    pub async fn entries(&self, run_id: RunId, kind: &str) -> Vec<serde_json::Value> {
        self.journal
            .list_entries(
                EntryFilter {
                    run_id: Some(run_id),
                    kind: Some(kind.into()),
                },
                None,
            )
            .await
            .unwrap()
            .items
            .into_iter()
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

    pub async fn enqueue(
        &self,
        started_by: RunStarter,
        request: RunRequest,
        context: serde_json::Value,
    ) -> RunRecord {
        self.queue
            .enqueue(NewRun {
                started_by,
                scope_id: ScopeId::new(),
                request,
                gathered_context: context,
                thread_id: None,
                run_id: None,
            })
            .await
            .expect("enqueue")
    }

    pub async fn record(&self, run_id: &str) -> QueueView {
        self.queue.get(run_id).await.unwrap().expect("record")
    }

    /// Waits until the Run Queue record is in `state`.
    pub async fn wait_record(&self, run_id: &str, state: RunRecordState) -> QueueView {
        for _ in 0..500 {
            let view = self.record(run_id).await;
            if view.record.state == state {
                return view;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("record {run_id} never reached {state:?}");
    }
}

fn node(
    store: &Datastore,
    journal: &Arc<Journal>,
    usage: &Arc<UsagePlanner>,
    queue: &Arc<TestQueue>,
    config: SupervisorConfig,
) -> Node {
    let calls: CallLog = Arc::new(Mutex::new(vec![]));
    let host = TestHost::new(calls.clone());
    let run_queue: Option<Arc<dyn RunQueue>> = config
        .program
        .is_agent()
        .then(|| queue.clone() as Arc<dyn RunQueue>);
    let sup = SessionSupervisor::new(
        store.clone(),
        journal.clone(),
        usage.clone(),
        run_queue,
        host.clone(),
        config,
    );
    let counters = Arc::new(Counters::default());
    sup.set_metrics_sink(counters.clone());
    let local = TestAdapter::new(AdapterKind::ClaudeLocal, &[], calls.clone());
    let hosted = TestAdapter::new(AdapterKind::HostedAgent, &[], calls.clone());
    let slack = TestAdapter::new(
        AdapterKind::SlackThread,
        &[AdapterVerb::Cancel],
        calls.clone(),
    );
    sup.register_adapter(local.clone()).unwrap();
    sup.register_adapter(hosted.clone()).unwrap();
    sup.register_adapter(slack.clone()).unwrap();
    Node {
        sup,
        local,
        hosted,
        slack,
        host,
        calls,
        counters,
    }
}

impl Node {
    /// Waits until the run is in `state`.
    pub async fn wait_state(&self, run_id: RunId, state: RunState) -> Run {
        for _ in 0..750 {
            if let Ok(view) = self.sup.get(run_id).await {
                if view.run.state == state {
                    return view.run;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let now = self.sup.get(run_id).await.map(|v| v.run.state);
        panic!("run {run_id} never reached {state:?} (now {now:?})");
    }

    /// Waits until an ended run's task has finished its ending and let the run go.
    pub async fn wait_ended_steps(&self, run_id: RunId) {
        for _ in 0..750 {
            if self.sup.subscribe(run_id).is_err() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("run {run_id} never finished its ending");
    }

    pub async fn wait_starts(adapter: &TestAdapter, n: usize) {
        for _ in 0..500 {
            if adapter.start_count() >= n {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!(
            "the adapter saw {} starts, expected {n}",
            adapter.start_count()
        );
    }

    pub async fn wait_until(what: &str, mut check: impl FnMut() -> bool) {
        for _ in 0..500 {
            if check() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("never: {what}");
    }

    /// Submits a task for a new agent instance and returns its run.
    pub async fn submit(&self, command: &str) -> RunId {
        self.sup
            .submit_task(task(command), WorkKind::Documents, false, None)
            .await
            .expect("submit")
    }

    /// Starts a local run with the given limits.
    pub async fn start_local(&self, limits: Option<RunLimits>) -> RunId {
        self.sup
            .start_run(request(
                RunTarget::AgentInstance {
                    agent_instance_id: AgentInstanceId::new(),
                },
                limits,
            ))
            .await
            .expect("start")
    }

    /// Starts a run on a remote agent routed to `kind`.
    pub async fn start_remote(&self, kind: AdapterKind) -> RunId {
        let remote_agent_id = RemoteAgentId::new();
        self.host
            .routes
            .lock()
            .unwrap()
            .insert(remote_agent_id, kind);
        self.sup
            .start_run(request(RunTarget::RemoteAgent { remote_agent_id }, None))
            .await
            .expect("start")
    }
}

pub fn task(command: &str) -> Task {
    Task {
        id: TaskId::new(),
        workspace_id: WorkspaceId::new(),
        target: TaskTarget::AgentInstance {
            agent_instance_id: AgentInstanceId::new(),
        },
        command: Some(command.into()),
        free_text: None,
        arguments: vec![ordin_types::domain::runs::TaskArgument {
            name: "quotes".into(),
            value: "Kestrel, Acme and Northwind Q3 quotes".into(),
        }],
        attached_artefacts: vec![],
        run_again_of: None,
        requested_at: Timestamp::now(),
    }
}

pub fn request(
    target: RunTarget,
    limits: Option<RunLimits>,
) -> ordin_session_supervisor::StartRequest {
    ordin_session_supervisor::StartRequest {
        origin: RunOrigin::Task {
            task_id: TaskId::new(),
        },
        workspace_id: WorkspaceId::new(),
        target,
        input: StartInput::FreeText {
            text: "Summarise the notes.".into(),
        },
        attachments: vec![],
        limits,
        work_kind: WorkKind::Documents,
        work_key: "test".into(),
        inputs_used: vec![],
        thread: None,
        started_by_person: true,
        model_override: None,
        coding_task_id: None,
        only_servers: None,
    }
}

pub fn limits(
    wall_clock_secs: u32,
    request_budget: Option<u32>,
    idle_timeout_secs: u32,
) -> Option<RunLimits> {
    Some(RunLimits {
        wall_clock_secs,
        request_budget,
        idle_timeout_secs,
    })
}

pub fn ended_snapshot(run_id: RunId, reason: RunEndReason) -> RunSnapshot {
    RunSnapshot {
        run_id,
        state: ordin_runtime_adapter::end_state(reason),
        end: Some(ordin_runtime_adapter::RunEnd {
            reason,
            final_message: Some("Done remotely.".into()),
            artefacts: vec![],
        }),
        checked_at: Timestamp::now(),
    }
}

pub fn chat(asker: &str) -> RunStarter {
    RunStarter::ChatConnection {
        connection: "support".into(),
        asker: asker.into(),
    }
}

pub fn message(text: &str) -> RunRequest {
    RunRequest::Message { text: text.into() }
}
