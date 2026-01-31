//! Events to the screens (CommandBridge.md Internal Logic 4–6): the desktop's own events from the
//! Journal's `EventSink`, each paired server's events from the Server Connector's channel, the replay
//! buffer of each server, and one batched stream per window (`service_events`).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use ordin_journal::{JournalError, JournalEvent};
use ordin_service_api::{
    encode_for_transport, EventEnvelope, ServiceApiVersion, ServiceEvent, TransportEvent,
};
use ordin_types::time::Timestamp;
use ordin_types::BackendId;
use serde::Serialize;
use serde_json::value::RawValue;
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use crate::error::BridgeError;
use crate::log;

/// The capacity of a window's event queue (step 6.5).
pub(crate) const WINDOW_QUEUE: usize = 8_192;
/// A batch is flushed when this many events wait (step 6.5).
pub(crate) const BATCH_EVENTS: usize = 50;
/// … or this long after its first event, whichever comes first (step 6.5).
pub(crate) const BATCH_INTERVAL: Duration = Duration::from_millis(16);
/// Events read per `events_since` page (steps 4 and 6.2).
const PAGE: u32 = 1_000;
/// The capacity of `subscribe_all`'s channel.
pub(crate) const ALL_EVENTS: usize = 1_024;

/// One batch of events for one window.
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct EventBatch {
    /// Each element is one serialised `EventEnvelope`, in delivery order.
    pub events: Vec<Box<RawValue>>,
}

/// The answer of `service_events`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct EventsOpened {
    pub local_backend: BackendId,
    /// Backends for which `after` could not be honoured; the window reloads them by command.
    pub reset: Vec<BackendId>,
}

/// Where a window's batches go: Tauri's `Channel::send` in the program. `Err` means the window is
/// gone and the stream closes (step 6.7).
pub type EventOut = Box<dyn Fn(EventBatch) -> Result<(), ()> + Send + Sync>;

/// One encoded event, shared by every window it is sent to.
#[derive(Debug, Clone)]
pub(crate) struct Queued {
    pub(crate) backend: BackendId,
    pub(crate) seq: u64,
    pub(crate) json: Arc<str>,
}

/// One open `service_events` stream.
struct WindowStream {
    generation: u64,
    tx: mpsc::Sender<Queued>,
    /// Set when the queue overflowed; the flusher drops the queue and sends resets (step 6.6).
    lagging: AtomicBool,
    /// Local events held while the window catches up from the Journal (step 6.2).
    held: Mutex<Option<Vec<Queued>>>,
    /// The highest local seq sent by the catch-up; a later live copy of it is skipped.
    local_floor: AtomicU64,
    cancel: CancellationToken,
}

impl WindowStream {
    fn held(&self) -> MutexGuard<'_, Option<Vec<Queued>>> {
        self.held.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Everything the fan-out shares, behind one lock so that registering a window, replaying a buffer
/// and delivering a live event are ordered against each other.
#[derive(Default)]
struct HubState {
    windows: HashMap<String, Arc<WindowStream>>,
    /// The replay buffer of each server backend (step 5).
    replay: HashMap<BackendId, VecDeque<Queued>>,
    /// The last seq seen of each backend, local and server: the seq of a `reset` envelope.
    last_seq: HashMap<BackendId, u64>,
}

/// The event fan-out.
pub(crate) struct Hub {
    local_backend: BackendId,
    replay_cap: usize,
    state: Mutex<HubState>,
    all: broadcast::Sender<Arc<EventEnvelope>>,
    next_generation: AtomicU64,
}

/// A `reset` envelope for `backend` with `seq`.
pub(crate) fn reset_envelope(backend: BackendId, seq: u64) -> EventEnvelope {
    EventEnvelope {
        api_version: ServiceApiVersion::CURRENT,
        backend,
        seq,
        at: Timestamp::now(),
        event: ServiceEvent::Reset,
    }
}

impl Hub {
    pub(crate) fn new(local_backend: BackendId, replay_cap: usize) -> Hub {
        let (all, _) = broadcast::channel(ALL_EVENTS);
        Hub {
            local_backend,
            replay_cap,
            state: Mutex::new(HubState::default()),
            all,
            next_generation: AtomicU64::new(0),
        }
    }

    fn lock(&self) -> MutexGuard<'_, HubState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub(crate) fn subscribe_all(&self) -> broadcast::Receiver<Arc<EventEnvelope>> {
        self.all.subscribe()
    }

    /// Encodes one envelope; an `EventError` is logged as `brg.event_dropped` and `None` returned.
    fn encode(envelope: &EventEnvelope, kind: &str) -> Option<TransportEvent> {
        match encode_for_transport(envelope) {
            Ok(event) => Some(event),
            Err(error) => {
                log::event_dropped(envelope.backend, envelope.seq, kind, error.code);
                None
            }
        }
    }

    /// Step 4: one local event from the Journal. An event already sent (re-read after a lag) is
    /// skipped, so each seq reaches a window once.
    // spec: ordin-desktop/components/CommandBridge#C-73
    pub(crate) fn local(&self, event: &JournalEvent) {
        let envelope = match EventEnvelope::new(
            self.local_backend,
            event.seq,
            event.at,
            &event.kind,
            event.data.clone(),
        ) {
            Ok(envelope) => envelope,
            Err(error) => {
                log::event_dropped(self.local_backend, event.seq, &event.kind, error.code);
                return;
            }
        };
        let Some(encoded) = Self::encode(&envelope, &event.kind) else {
            return;
        };
        let mut state = self.lock();
        let last = state.last_seq.entry(self.local_backend).or_insert(0);
        if event.seq <= *last {
            return;
        }
        *last = event.seq;
        let queued = Queued {
            backend: self.local_backend,
            seq: encoded.seq,
            json: Arc::from(encoded.json),
        };
        self.fan_out(&state, &queued);
        drop(state);
        let _ = self.all.send(Arc::new(envelope));
    }

    /// Step 4, `JRNL_RESUME_TOO_OLD`: a `reset` for the local backend to every window.
    pub(crate) fn local_reset(&self) {
        self.reset(self.local_backend, false);
    }

    /// Step 5: one event of a paired server.
    pub(crate) fn server_event(&self, backend: BackendId, envelope: Arc<EventEnvelope>) {
        let kind = envelope.event.kind();
        let Some(encoded) = Self::encode(&envelope, kind.as_str()) else {
            return;
        };
        let queued = Queued {
            backend,
            seq: encoded.seq,
            json: Arc::from(encoded.json),
        };
        let mut state = self.lock();
        state.last_seq.insert(backend, queued.seq);
        let cap = self.replay_cap;
        let buffer = state.replay.entry(backend).or_default();
        buffer.push_back(queued.clone());
        while buffer.len() > cap {
            buffer.pop_front();
        }
        self.fan_out(&state, &queued);
        drop(state);
        let _ = self.all.send(envelope);
    }

    /// Step 5, `ServerEvent::Reset`: the buffer is cleared and every window receives one `reset`
    /// envelope for `backend`, with the last seq seen, so the screens' order rule marks no gap.
    pub(crate) fn server_reset(&self, backend: BackendId) {
        self.reset(backend, true);
    }

    fn reset(&self, backend: BackendId, clear_replay: bool) {
        let mut state = self.lock();
        if clear_replay {
            state.replay.entry(backend).or_default().clear();
        }
        let seq = state.last_seq.get(&backend).copied().unwrap_or(0);
        let envelope = reset_envelope(backend, seq);
        let Some(encoded) = Self::encode(&envelope, "reset") else {
            return;
        };
        let queued = Queued {
            backend,
            seq,
            json: Arc::from(encoded.json),
        };
        // A reset is never held behind a catch-up: it supersedes it.
        for window in state.windows.values() {
            Self::enqueue(window, &queued);
        }
        drop(state);
        let _ = self.all.send(Arc::new(envelope));
    }

    /// Queues one event for every open window, holding local events of a window catching up and
    /// skipping local events a window already received in its catch-up.
    fn fan_out(&self, state: &HubState, queued: &Queued) {
        let local = queued.backend == self.local_backend;
        for window in state.windows.values() {
            if local {
                if queued.seq <= window.local_floor.load(Ordering::SeqCst) {
                    continue;
                }
                let mut held = window.held();
                if let Some(held) = held.as_mut() {
                    held.push(queued.clone());
                    continue;
                }
            }
            Self::enqueue(window, queued);
        }
    }

    /// Queues one event for one window; a full queue marks the window lagging (step 6.6).
    fn enqueue(window: &WindowStream, queued: &Queued) -> bool {
        if window.lagging.load(Ordering::SeqCst) {
            // Already lagging: the resets that follow cover this event.
            return false;
        }
        match window.tx.try_send(queued.clone()) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => {
                window.lagging.store(true, Ordering::SeqCst);
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => false,
        }
    }

    /// Step 6: validates `after`, registers the window (replacing its earlier stream), replays server
    /// buffers, and returns the backends that must be reset and whether local catch-up is needed.
    pub(crate) fn open(
        self: &Arc<Self>,
        window: &str,
        after: &BTreeMap<BackendId, u64>,
        paired: impl Fn(BackendId) -> bool,
        out: EventOut,
        runtime: &tokio::runtime::Handle,
    ) -> Result<Opening, BridgeError> {
        // 6.4: a backend neither local nor paired refuses the whole call.
        if after
            .keys()
            .any(|b| *b != self.local_backend && !paired(*b))
        {
            return Err(BridgeError::backend_unknown());
        }
        let local_after = after.get(&self.local_backend).copied();
        let (tx, rx) = mpsc::channel::<Queued>(WINDOW_QUEUE);
        let stream = Arc::new(WindowStream {
            generation: self.next_generation.fetch_add(1, Ordering::SeqCst),
            tx,
            lagging: AtomicBool::new(false),
            held: Mutex::new(local_after.map(|_| Vec::new())),
            local_floor: AtomicU64::new(0),
            cancel: CancellationToken::new(),
        });
        let mut reset = Vec::new();
        {
            let mut state = self.lock();
            // 6.1: a second call from the same window closes the earlier stream first.
            if let Some(earlier) = state.windows.remove(window) {
                earlier.cancel.cancel();
                log::stream(window, "closed", 0);
            }
            // 6.3: each server backend from its replay buffer, under the lock so no live event of
            // that backend slips between the replay and live delivery.
            for (backend, from) in after {
                if *backend == self.local_backend {
                    continue;
                }
                let last = state.last_seq.get(backend).copied();
                if last == Some(*from) {
                    continue;
                }
                let buffer = state.replay.get(backend);
                let covered = last.is_some_and(|l| l > *from)
                    && buffer
                        .and_then(|b| b.front())
                        .is_some_and(|first| first.seq <= from.saturating_add(1));
                match (covered, buffer) {
                    (true, Some(buffer)) => {
                        for queued in buffer.iter().filter(|q| q.seq > *from) {
                            Self::enqueue(&stream, queued);
                        }
                    }
                    _ => reset.push(*backend),
                }
            }
            state.windows.insert(window.to_owned(), Arc::clone(&stream));
        }
        let hub = Arc::clone(self);
        let label = window.to_owned();
        let flusher = Arc::clone(&stream);
        runtime.spawn(async move { hub.flush_loop(label, flusher, rx, out).await });
        Ok(Opening {
            stream: OpenStream(stream),
            local_after,
            reset,
        })
    }

    /// Step 6.2, second half: sends the catch-up read from the Journal, then releases the held live
    /// events, dropping any whose seq was already sent.
    pub(crate) fn release(&self, stream: &OpenStream, catch_up: Vec<Queued>) {
        let state = self.lock();
        let stream = &stream.0;
        let mut sent = 0u64;
        for queued in &catch_up {
            Self::enqueue(stream, queued);
            sent = sent.max(queued.seq);
        }
        stream.local_floor.fetch_max(sent, Ordering::SeqCst);
        let held = stream.held().take().unwrap_or_default();
        for queued in held.iter().filter(|q| q.seq > sent) {
            Self::enqueue(stream, queued);
        }
        drop(state);
    }

    /// Encodes a Journal record for a catch-up; `None` when it is skipped (logged).
    pub(crate) fn encode_local(&self, event: &JournalEvent) -> Option<Queued> {
        let envelope = match EventEnvelope::new(
            self.local_backend,
            event.seq,
            event.at,
            &event.kind,
            event.data.clone(),
        ) {
            Ok(envelope) => envelope,
            Err(error) => {
                log::event_dropped(self.local_backend, event.seq, &event.kind, error.code);
                return None;
            }
        };
        Self::encode(&envelope, &event.kind).map(|encoded| Queued {
            backend: self.local_backend,
            seq: encoded.seq,
            json: Arc::from(encoded.json),
        })
    }

    /// Step 6.6: the queue is dropped and one `reset` per backend the window may hold is queued.
    fn lagging_resets(
        &self,
        rx: &mut mpsc::Receiver<Queued>,
        stream: &WindowStream,
    ) -> Vec<Queued> {
        let state = self.lock();
        while rx.try_recv().is_ok() {}
        let _ = stream.held().take();
        let mut backends: Vec<BackendId> = vec![self.local_backend];
        backends.extend(state.last_seq.keys().filter(|b| **b != self.local_backend));
        let resets = backends
            .into_iter()
            .filter_map(|backend| {
                let seq = state.last_seq.get(&backend).copied().unwrap_or(0);
                Self::encode(&reset_envelope(backend, seq), "reset").map(|encoded| Queued {
                    backend,
                    seq,
                    json: Arc::from(encoded.json),
                })
            })
            .collect();
        stream.lagging.store(false, Ordering::SeqCst);
        drop(state);
        resets
    }

    /// Step 6.5: one batch every 16 ms or when 50 events wait, whichever comes first.
    async fn flush_loop(
        self: Arc<Self>,
        window: String,
        stream: Arc<WindowStream>,
        mut rx: mpsc::Receiver<Queued>,
        out: EventOut,
    ) {
        let cancel = stream.cancel.clone();
        loop {
            let first = tokio::select! {
                biased;
                () = cancel.cancelled() => return,
                first = rx.recv() => first,
            };
            let Some(first) = first else { return };
            let mut batch = vec![first];
            let deadline = tokio::time::Instant::now() + BATCH_INTERVAL;
            while batch.len() < BATCH_EVENTS {
                tokio::select! {
                    biased;
                    () = cancel.cancelled() => return,
                    next = rx.recv() => match next {
                        Some(next) => batch.push(next),
                        None => break,
                    },
                    () = tokio::time::sleep_until(deadline) => break,
                }
            }
            if stream.lagging.load(Ordering::SeqCst) {
                // 6.6: the WebView stopped reading. What was taken is dropped with the queue.
                log::window_lagging(&window);
                batch = self.lagging_resets(&mut rx, &stream);
            }
            if send_batch(&out, &batch).is_err() {
                // 6.7: the window is gone.
                self.close_generation(&window, stream.generation);
                return;
            }
        }
    }

    /// Closes the stream of `window` if it is still the one opened as `generation`.
    fn close_generation(&self, window: &str, generation: u64) {
        let mut state = self.lock();
        if state
            .windows
            .get(window)
            .is_some_and(|w| w.generation == generation)
        {
            if let Some(stream) = state.windows.remove(window) {
                stream.cancel.cancel();
                log::stream(window, "closed", 0);
            }
        }
    }

    /// Closes the stream of `window`, if one is open (window destroyed).
    pub(crate) fn close_window(&self, window: &str) {
        if let Some(stream) = self.lock().windows.remove(window) {
            stream.cancel.cancel();
            log::stream(window, "closed", 0);
        }
    }

    /// Closes every window stream (quit).
    pub(crate) fn close_all(&self) {
        let windows: Vec<(String, Arc<WindowStream>)> = self.lock().windows.drain().collect();
        for (window, stream) in windows {
            stream.cancel.cancel();
            log::stream(&window, "closed", 0);
        }
    }
}

/// A registered window stream, handed back for the catch-up.
pub(crate) struct OpenStream(Arc<WindowStream>);

/// What `Hub::open` hands back.
pub(crate) struct Opening {
    pub(crate) stream: OpenStream,
    pub(crate) local_after: Option<u64>,
    pub(crate) reset: Vec<BackendId>,
}

/// Builds and sends one `EventBatch`.
fn send_batch(out: &EventOut, batch: &[Queued]) -> Result<(), ()> {
    let events = batch
        .iter()
        .filter_map(|q| RawValue::from_string(q.json.to_string()).ok())
        .collect();
    out(EventBatch { events })
}

/// Reads the Journal from `after` in pages until caught up. `Err` carries the Journal's code when
/// `after` cannot be honoured (`JRNL_RESUME_TOO_OLD`, `JRNL_SEQ_AHEAD`) or the read failed.
pub(crate) async fn read_since(
    journal: &ordin_journal::Journal,
    after: u64,
) -> Result<Vec<JournalEvent>, JournalError> {
    let mut out = Vec::new();
    let mut from = after;
    loop {
        let page = journal.events_since(from, PAGE).await?;
        let full = page.len() >= PAGE as usize;
        if let Some(last) = page.last() {
            from = last.seq;
        }
        out.extend(page);
        if !full {
            return Ok(out);
        }
    }
}
