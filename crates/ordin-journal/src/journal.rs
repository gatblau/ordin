//! The Journal service: start, events and their delivery, liveness and shutdown
//! (JournalAndArtefactStore.md Internal Logic 3–6, 15, 16).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use ordin_types::domain::runs::{RunState, StepMark};
use ordin_types::time::Timestamp;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::codec;
use crate::datastore::{Datastore, Engine};
use crate::db::{self, Arg, Tx};
use crate::error::JournalError;
use crate::types::{EventSink, JournalEvent, PruneParticipant, PruneSummary, SubscriptionId};
use crate::validate;

/// Event `data` is at most 256 KiB serialised (LW-6).
const MAX_EVENT_BYTES: usize = 262_144;
/// Each subscriber's queue (*Configuration and constants*: 1,024 events per subscriber).
const SINK_QUEUE: usize = 1_024;
/// `events_since` returns 1 to 1,000 events per call (Internal Logic 5.6).
const MAX_EVENTS_PER_READ: u32 = 1_000;
/// Event sequencing across PostgreSQL instances (Internal Logic 4.2; constant chosen there).
const EVENT_LOCK_KEY: i64 = 7_243_100_002;
/// The scaled shape's cross-instance poll (Internal Logic 5.5).
const POLL_INTERVAL: Duration = Duration::from_millis(500);
const POLL_BATCH: i64 = 500;
/// Shutdown waits at most this long for the queues to drain (Internal Logic 15.2).
const DRAIN_BOUND: Duration = Duration::from_secs(2);
/// The first automatic prune runs this long after start (Internal Logic 3.5).
const FIRST_PRUNE_DELAY: Duration = Duration::from_secs(60);
/// A step keeps at most this many sequence numbers in `events_seen` (Internal Logic 6.2).
const MAX_EVENTS_SEEN: usize = 10_000;
/// The run states after which a run has ended (Internal Logic 6.3).
const ENDED_STATES: [RunState; 7] = [
    RunState::Completed,
    RunState::Failed,
    RunState::StoppedAtLimit,
    RunState::Cancelled,
    RunState::Interrupted,
    RunState::Abandoned,
    RunState::NoReply,
];

pub struct JournalOptions {
    /// `true` on the desktop and the personal server; `false` in the agent profile.
    pub automatic_pruning: bool,
    /// 24 hours unless a test shortens it.
    pub prune_interval: std::time::Duration,
    pub clock: std::sync::Arc<dyn Clock>,
}

pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

/// One registered sink and its delivery task.
struct Subscriber {
    id: u64,
    name: &'static str,
    sender: mpsc::Sender<JournalEvent>,
    shared: Arc<SinkShared>,
    task: JoinHandle<()>,
}

/// What the pushing side and the delivery task of one sink share.
struct SinkShared {
    /// `Some(next_seq)` while the sink is lagged: events from `next_seq` on were dropped.
    lagged_from: Mutex<Option<u64>>,
    /// Set by `unsubscribe`: the task ends after the event it is delivering.
    closed: AtomicBool,
    pushed: AtomicU64,
    delivered: AtomicU64,
}

pub(crate) struct Inner {
    pub(crate) store: Datastore,
    pub(crate) engine: Engine,
    backend: uuid::Uuid,
    last_seq: AtomicU64,
    pub(crate) clock: Arc<dyn Clock>,
    /// One writer at a time in this process (Internal Logic 4.2: one writer per SQLite file);
    /// every write transaction of this crate holds it.
    pub(crate) write_lock: tokio::sync::Mutex<()>,
    /// The highest `seq` pushed to the queues; pushes happen in `seq` order under this lock.
    delivered_seq: tokio::sync::Mutex<u64>,
    subscribers: Mutex<Vec<Subscriber>>,
    next_subscription: AtomicU64,
    pub(crate) participants: Mutex<Vec<Arc<dyn PruneParticipant>>>,
    /// Held while a prune runs; keeps the last summary for a caller that waited (Internal Logic 14.3.7).
    pub(crate) prune_slot: tokio::sync::Mutex<Option<PruneSummary>>,
    pub(crate) prune_generation: AtomicU64,
    pub(crate) stopping: AtomicBool,
    stop: watch::Sender<bool>,
    background: Mutex<Vec<JoinHandle<()>>>,
    runtime: tokio::runtime::Handle,
}

/// Cheap to clone; consumers hold `Journal` (or `Arc<Journal>`), never a trait object.
#[derive(Clone)]
pub struct Journal {
    pub(crate) inner: Arc<Inner>,
}

impl Journal {
    /// After `MigrationRegistry::apply`. Reads or creates the installation's backend identifier,
    /// starts the delivery tasks and, when `automatic_pruning`, the prune timer.
    pub async fn start(store: Datastore, options: JournalOptions) -> Result<Journal, JournalError> {
        let engine = store.engine();
        let runtime =
            tokio::runtime::Handle::try_current().map_err(|_| JournalError::Unavailable)?;
        // Step 3.1: the meta row, created on the first start.
        let select = "SELECT {s:backend_id}, last_seq FROM journal_meta WHERE id = 1";
        let row = match db::fetch_optional(&store, select, &[]).await? {
            Some(row) => row,
            None => {
                db::execute(
                    &store,
                    "INSERT INTO journal_meta (id, backend_id, last_seq, retention_days, created_at) \
                     VALUES (1, $1{uuid}, 0, 90, $2{ts}) ON CONFLICT (id) DO NOTHING",
                    &[
                        Arg::Text(uuid::Uuid::now_v7().to_string()),
                        Arg::Text(options.clock.now().to_wire()),
                    ],
                )
                .await?;
                db::fetch_optional(&store, select, &[])
                    .await?
                    .ok_or(JournalError::Unavailable)?
            }
        };
        let backend = codec::uuid(&row.text(0)?)?;
        let last_seq = codec::u64_of(row.int(1)?)?;
        let (stop, _) = watch::channel(false);
        let journal = Journal {
            inner: Arc::new(Inner {
                store,
                engine,
                backend,
                last_seq: AtomicU64::new(last_seq),
                clock: options.clock,
                write_lock: tokio::sync::Mutex::new(()),
                delivered_seq: tokio::sync::Mutex::new(last_seq),
                subscribers: Mutex::new(Vec::new()),
                next_subscription: AtomicU64::new(1),
                participants: Mutex::new(Vec::new()),
                prune_slot: tokio::sync::Mutex::new(None),
                prune_generation: AtomicU64::new(0),
                stopping: AtomicBool::new(false),
                stop,
                background: Mutex::new(Vec::new()),
                runtime,
            }),
        };
        // Step 3.4: the cross-instance poller (scaled shape).
        if engine == Engine::Postgres {
            let task = journal.inner.runtime.spawn(poll_loop(
                Arc::downgrade(&journal.inner),
                journal.inner.stop.subscribe(),
            ));
            journal.keep_background(task);
        }
        // Step 3.5: the prune timer.
        if options.automatic_pruning {
            let task = journal.inner.runtime.spawn(prune_loop(
                Arc::downgrade(&journal.inner),
                journal.inner.stop.subscribe(),
                options.prune_interval,
            ));
            journal.keep_background(task);
        }
        tracing::info!(
            component = "journal",
            engine = engine.as_str(),
            last_seq,
            "journal.started"
        );
        Ok(journal)
    }

    fn keep_background(&self, task: JoinHandle<()>) {
        if let Ok(mut tasks) = self.inner.background.lock() {
            tasks.push(task);
        }
    }

    pub fn datastore(&self) -> &Datastore {
        &self.inner.store
    }

    /// The installation's backend identifier, minted once (UUIDv7). The program wraps it as
    /// the Service API's `BackendId` (`BackendId(journal.backend_uuid())`).
    pub fn backend_uuid(&self) -> uuid::Uuid {
        self.inner.backend
    }

    pub(crate) fn now(&self) -> Timestamp {
        self.inner.clock.now()
    }

    /// Stops the timer and delivery tasks after delivering what is queued (at most 2 s).
    /// There is no separate `flush` or `close` on the Journal: the program calls `shutdown`,
    /// then `Datastore::close`.
    pub async fn shutdown(&self) {
        // Step 15.1: the timer and the poller stop; a running prune ends after its current batch.
        self.inner.stopping.store(true, Ordering::SeqCst);
        let _ = self.inner.stop.send(true);
        let background: Vec<JoinHandle<()>> = match self.inner.background.lock() {
            Ok(mut tasks) => tasks.drain(..).collect(),
            Err(_) => Vec::new(),
        };
        for task in background {
            let _ = task.await;
        }
        // Step 15.2: closing each queue lets its task deliver what is queued, then end.
        let subscribers: Vec<Subscriber> = match self.inner.subscribers.lock() {
            Ok(mut subs) => subs.drain(..).collect(),
            Err(_) => Vec::new(),
        };
        let mut tasks = Vec::with_capacity(subscribers.len());
        let mut counters = Vec::with_capacity(subscribers.len());
        for subscriber in subscribers {
            drop(subscriber.sender);
            counters.push((subscriber.name, subscriber.shared));
            tasks.push(subscriber.task);
        }
        let drained = tokio::time::timeout(DRAIN_BOUND, async {
            for task in tasks.iter_mut() {
                let _ = task.await;
            }
        })
        .await;
        if drained.is_err() {
            for task in &tasks {
                task.abort();
            }
        }
        let undelivered: BTreeMap<&str, u64> = counters
            .iter()
            .map(|(name, shared)| {
                let pushed = shared.pushed.load(Ordering::SeqCst);
                let delivered = shared.delivered.load(Ordering::SeqCst);
                (*name, pushed.saturating_sub(delivered))
            })
            .collect();
        let undelivered_by_sink = serde_json::to_string(&undelivered).unwrap_or_default();
        tracing::info!(
            component = "journal",
            undelivered_by_sink = undelivered_by_sink.as_str(),
            "journal.shutdown"
        );
    }

    /// `true` when one `SELECT 1` on the datastore answers within `within`; never an error
    /// (step 16). For readiness and watchdog checks.
    pub async fn ping(&self, within: std::time::Duration) -> bool {
        let probe = async {
            match &self.inner.store {
                Datastore::Sqlite(pool) => sqlx::query("SELECT 1").execute(pool).await.is_ok(),
                Datastore::Postgres(pool) => sqlx::query("SELECT 1").execute(pool).await.is_ok(),
            }
        };
        match tokio::time::timeout(within, probe).await {
            Ok(true) => true,
            _ => {
                tracing::debug!(component = "journal", "journal.ping.failed");
                false
            }
        }
    }

    // ---- events (EventSink hub; ADR-100)

    /// `kind` is `EventKind::as_str()` (2C wire); `data` is the `ServiceEvent` variant's
    /// fields without `kind`. There is no `emit(ServiceEvent)`: this crate cannot name it.
    pub async fn append_event(
        &self,
        kind: &str,
        data: serde_json::Value,
    ) -> Result<JournalEvent, JournalError> {
        let text = check_event(kind, &data)?;
        let _writer = self.inner.write_lock.lock().await;
        let appended = async {
            let mut tx = Tx::begin(&self.inner.store).await?;
            let event = self.append_in_tx(&mut tx, kind, data, text).await?;
            tx.commit().await?;
            Ok::<JournalEvent, JournalError>(event)
        }
        .await;
        match appended {
            Ok(event) => {
                self.published(event.clone()).await;
                Ok(event)
            }
            Err(error) => Err(self.append_failed(kind, error)),
        }
    }

    /// Logs a failed append; the sequence number was not consumed (Internal Logic 4.3).
    pub(crate) fn append_failed(&self, kind: &str, error: JournalError) -> JournalError {
        let error = match error {
            JournalError::Invalid { .. } | JournalError::EventKindInvalid => error,
            _ => JournalError::Unavailable,
        };
        let kind = if validate::is_kind(kind) {
            kind
        } else {
            "unknown"
        };
        tracing::warn!(
            component = "journal",
            kind,
            code = ordin_types::CodedError::code(&error),
            "journal.event.append_failed"
        );
        error
    }

    /// Step 4.3 inside the caller's transaction, under the writer lock. The caller commits, then
    /// calls [`Journal::published`].
    pub(crate) async fn append_in_tx(
        &self,
        tx: &mut Tx,
        kind: &str,
        data: serde_json::Value,
        text: String,
    ) -> Result<JournalEvent, JournalError> {
        let last = match tx.engine() {
            Engine::Sqlite => self.inner.last_seq.load(Ordering::SeqCst),
            Engine::Postgres => {
                tx.execute(
                    "SELECT pg_advisory_xact_lock($1)",
                    &[Arg::Int(EVENT_LOCK_KEY)],
                )
                .await?;
                let row = tx
                    .fetch_optional("SELECT last_seq FROM journal_meta WHERE id = 1", &[])
                    .await?
                    .ok_or(JournalError::Unavailable)?;
                codec::u64_of(row.int(0)?)?
            }
        };
        let seq = last + 1;
        let at = self.now();
        let run_id = data
            .get("run_id")
            .and_then(|v| v.as_str())
            .and_then(|s| uuid::Uuid::try_parse(s).ok())
            .map(|u| u.hyphenated().to_string());
        tx.execute(
            "INSERT INTO journal_events (seq, at, kind, run_id, data) \
             VALUES ($1, $2{ts}, $3, $4{uuid}, $5{json})",
            &[
                Arg::Int(codec::i64_of(seq)?),
                Arg::Text(at.to_wire()),
                Arg::Text(kind.to_owned()),
                Arg::OptText(run_id.clone()),
                Arg::Text(text),
            ],
        )
        .await?;
        tx.execute(
            "UPDATE journal_meta SET last_seq = $1 WHERE id = 1",
            &[Arg::Int(codec::i64_of(seq)?)],
        )
        .await?;
        if let Some(run_id) = &run_id {
            self.event_side_effects(tx, seq, at, kind, run_id, &data)
                .await?;
        }
        Ok(JournalEvent {
            backend: self.inner.backend,
            seq,
            at,
            kind: kind.to_owned(),
            data,
        })
    }

    /// Internal Logic 6: the step marks see the event, and `run_state_changed` keeps the run index.
    async fn event_side_effects(
        &self,
        tx: &mut Tx,
        seq: u64,
        at: Timestamp,
        kind: &str,
        run_id: &str,
        data: &serde_json::Value,
    ) -> Result<(), JournalError> {
        // 6.2
        let now_step = tx
            .fetch_optional(
                "SELECT {s:id}, {s:events_seen} FROM run_steps \
                 WHERE run_id = $1{uuid} AND mark = 'now' \
                 AND plan_version = (SELECT MAX(plan_version) FROM run_steps WHERE run_id = $1{uuid})",
                &[Arg::Text(run_id.to_owned())],
            )
            .await?;
        if let Some(step) = now_step {
            let mut seen: Vec<u64> = codec::from_json(&step.text(1)?)?;
            seen.push(seq);
            if seen.len() > MAX_EVENTS_SEEN {
                let surplus = seen.len() - MAX_EVENTS_SEEN;
                seen.drain(..surplus);
            }
            tx.execute(
                "UPDATE run_steps SET events_seen = $1{json} WHERE id = $2{uuid}",
                &[Arg::Text(codec::to_json(&seen)?), Arg::Text(step.text(0)?)],
            )
            .await?;
        }
        // 6.3
        if kind == "run_state_changed" {
            let state = data
                .get("state")
                .cloned()
                .and_then(|v| serde_json::from_value::<RunState>(v).ok());
            match state {
                Some(state) => {
                    let ended = ENDED_STATES.contains(&state).then(|| at.to_wire());
                    tx.execute(
                        "INSERT INTO journal_runs (run_id, first_seen_at, state, ended_at) \
                         VALUES ($1{uuid}, $2{ts}, $3, $4{ts}) \
                         ON CONFLICT (run_id) DO UPDATE SET state = excluded.state, \
                         ended_at = COALESCE(excluded.ended_at, journal_runs.ended_at)",
                        &[
                            Arg::Text(run_id.to_owned()),
                            Arg::Text(at.to_wire()),
                            Arg::Text(codec::name_of(&state)?),
                            Arg::OptText(ended),
                        ],
                    )
                    .await?;
                }
                None => {
                    tracing::warn!(component = "journal", seq, "journal.run_index.skipped");
                }
            }
        }
        Ok(())
    }

    /// Step 4.4: after commit, the in-memory head moves and the event is queued to every sink.
    /// Called with the writer lock held, so events are pushed in `seq` order.
    pub(crate) async fn published(&self, event: JournalEvent) {
        self.inner.last_seq.fetch_max(event.seq, Ordering::SeqCst);
        let mut delivered = self.inner.delivered_seq.lock().await;
        // PostgreSQL: an event of another instance came in between; the poller pushes both in order.
        if self.inner.engine == Engine::Postgres && event.seq != *delivered + 1 {
            return;
        }
        *delivered = event.seq;
        self.push(&event);
        tracing::debug!(
            component = "journal",
            seq = event.seq,
            kind = event.kind.as_str(),
            "journal.event.appended"
        );
    }

    /// Step 5.2: a full queue marks its sink lagged and drops this and every later event for it.
    fn push(&self, event: &JournalEvent) {
        let Ok(subscribers) = self.inner.subscribers.lock() else {
            return;
        };
        for subscriber in subscribers.iter() {
            let Ok(mut lagged) = subscriber.shared.lagged_from.lock() else {
                continue;
            };
            if lagged.is_some() {
                continue;
            }
            match subscriber.sender.try_send(event.clone()) {
                Ok(()) => {
                    subscriber.shared.pushed.fetch_add(1, Ordering::SeqCst);
                }
                Err(mpsc::error::TrySendError::Full(_)) => {
                    *lagged = Some(event.seq);
                    tracing::warn!(
                        component = "journal",
                        sink = subscriber.name,
                        next_seq = event.seq,
                        "journal.sink.lagged"
                    );
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {}
            }
        }
    }

    pub async fn events_since(
        &self,
        after_seq: u64,
        limit: u32,
    ) -> Result<Vec<JournalEvent>, JournalError> {
        let limit = limit.clamp(1, MAX_EVENTS_PER_READ);
        let last_seq = match self.inner.engine {
            Engine::Sqlite => self.last_seq(),
            Engine::Postgres => {
                let row = db::fetch_optional(
                    &self.inner.store,
                    "SELECT last_seq FROM journal_meta WHERE id = 1",
                    &[],
                )
                .await?
                .ok_or(JournalError::Unavailable)?;
                codec::u64_of(row.int(0)?)?
            }
        };
        if after_seq > last_seq {
            return Err(JournalError::SeqAhead { last_seq });
        }
        let first = db::fetch_optional(
            &self.inner.store,
            "SELECT MIN(seq) FROM journal_events",
            &[],
        )
        .await?
        .map(|row| row.opt_int(0))
        .transpose()?
        .flatten();
        let first_kept_seq = match first {
            Some(min) => codec::u64_of(min)?,
            None => last_seq + 1,
        };
        if after_seq + 1 < first_kept_seq {
            return Err(JournalError::ResumeTooOld { first_kept_seq });
        }
        self.read_events(after_seq, i64::from(limit)).await
    }

    /// Events after `after_seq`, oldest first.
    async fn read_events(
        &self,
        after_seq: u64,
        limit: i64,
    ) -> Result<Vec<JournalEvent>, JournalError> {
        let rows = db::fetch_all(
            &self.inner.store,
            "SELECT seq, {t:at}, kind, {s:data} FROM journal_events \
             WHERE seq > $1 ORDER BY seq LIMIT $2",
            &[Arg::Int(codec::i64_of(after_seq)?), Arg::Int(limit)],
        )
        .await?;
        rows.iter()
            .map(|row| {
                Ok(JournalEvent {
                    backend: self.inner.backend,
                    seq: codec::u64_of(row.int(0)?)?,
                    at: codec::ts(&row.text(1)?)?,
                    kind: row.text(2)?,
                    data: codec::from_json(&row.text(3)?)?,
                })
            })
            .collect()
    }

    /// The highest sequence number appended so far (0 before the first event).
    pub fn last_seq(&self) -> u64 {
        self.inner.last_seq.load(Ordering::SeqCst)
    }

    pub fn subscribe(&self, sink: std::sync::Arc<dyn EventSink>) -> SubscriptionId {
        let id = self.inner.next_subscription.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = mpsc::channel(SINK_QUEUE);
        let shared = Arc::new(SinkShared {
            lagged_from: Mutex::new(None),
            closed: AtomicBool::new(false),
            pushed: AtomicU64::new(0),
            delivered: AtomicU64::new(0),
        });
        let name = sink.name();
        let task = self
            .inner
            .runtime
            .spawn(deliver_loop(sink, receiver, Arc::clone(&shared)));
        if let Ok(mut subscribers) = self.inner.subscribers.lock() {
            subscribers.push(Subscriber {
                id,
                name,
                sender,
                shared,
                task,
            });
        }
        SubscriptionId(id)
    }

    pub fn unsubscribe(&self, id: SubscriptionId) {
        if let Ok(mut subscribers) = self.inner.subscribers.lock() {
            if let Some(at) = subscribers.iter().position(|s| s.id == id.0) {
                let subscriber = subscribers.remove(at);
                subscriber.shared.closed.store(true, Ordering::SeqCst);
                // Dropping the sender closes the queue; the task ends after its current event.
                drop(subscriber.sender);
            }
        }
    }
}

/// Internal Logic 4.1: the kind, `reset`, the object form and the size; returns the JSON text.
pub(crate) fn check_event(kind: &str, data: &serde_json::Value) -> Result<String, JournalError> {
    if !validate::is_kind(kind) || kind == "reset" || !data.is_object() {
        return Err(JournalError::EventKindInvalid);
    }
    let text = serde_json::to_string(data).map_err(|_| JournalError::EventKindInvalid)?;
    if text.len() > MAX_EVENT_BYTES {
        return Err(JournalError::EventTooLarge { bytes: text.len() });
    }
    Ok(text)
}

/// Step 5.1 and 5.3: one task per sink, delivering in order; a lagged sink is told once its queue
/// has drained, and then receives new events again.
async fn deliver_loop(
    sink: Arc<dyn EventSink>,
    mut receiver: mpsc::Receiver<JournalEvent>,
    shared: Arc<SinkShared>,
) {
    while let Some(event) = receiver.recv().await {
        if shared.closed.load(Ordering::SeqCst) {
            break;
        }
        sink.deliver(&event).await;
        shared.delivered.fetch_add(1, Ordering::SeqCst);
        if receiver.is_empty() {
            let next = shared.lagged_from.lock().ok().and_then(|mut l| l.take());
            if let Some(next_seq) = next {
                sink.lagged(next_seq).await;
                tracing::info!(
                    component = "journal",
                    sink = sink.name(),
                    "journal.sink.resumed"
                );
            }
        }
    }
}

/// Step 5.5: events appended by other instances reach this instance's sinks, in order.
async fn poll_loop(inner: Weak<Inner>, mut stop: watch::Receiver<bool>) {
    loop {
        tokio::select! {
            _ = tokio::time::sleep(POLL_INTERVAL) => {}
            _ = stop.changed() => return,
        }
        let Some(inner) = inner.upgrade() else {
            return;
        };
        let journal = Journal { inner };
        let mut delivered = journal.inner.delivered_seq.lock().await;
        let Ok(events) = journal.read_events(*delivered, POLL_BATCH).await else {
            continue;
        };
        for event in events {
            *delivered = event.seq;
            journal
                .inner
                .last_seq
                .fetch_max(event.seq, Ordering::SeqCst);
            journal.push(&event);
        }
    }
}

/// Internal Logic 3.5: the first prune 60 seconds after start, then every `interval`.
async fn prune_loop(inner: Weak<Inner>, mut stop: watch::Receiver<bool>, interval: Duration) {
    let mut wait = FIRST_PRUNE_DELAY;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = stop.changed() => return,
        }
        wait = interval;
        let Some(inner) = inner.upgrade() else {
            return;
        };
        // A failed prune is logged by `prune` and retried at the next tick.
        let _ = Journal { inner }.prune(None).await;
    }
}

/// `true` for a mark transition Internal Logic 13.3 allows.
pub(crate) fn transition_allowed(from: StepMark, to: StepMark) -> bool {
    matches!(
        (from, to),
        (StepMark::Next, StepMark::Now)
            | (StepMark::Now, StepMark::Done)
            | (StepMark::Next, StepMark::Skipped)
            | (StepMark::Now, StepMark::Skipped)
    )
}
