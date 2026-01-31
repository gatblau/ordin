//! `CommandBridge`: construction, start, quit, and the transport-neutral entry points the Tauri
//! commands of `ipc.rs` call (CommandBridge.md *Public Interface*, Internal Logic 1, 3, 4, 5).

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use ordin_journal::{EventSink, Journal, JournalEvent, SubscriptionId};
use ordin_server_connector::{ServerConnector, ServerEvent, ServerEventSender};
use ordin_service_api::{ErrorSentences, EventEnvelope, ServiceApi};
use ordin_types::BackendId;
use tokio::sync::{broadcast, mpsc, Notify};
use tokio_util::sync::CancellationToken;

use crate::error::BridgeError;
use crate::events::{self, EventOut, EventsOpened, Hub};
use crate::log;
use crate::streams::{ScreenStreams, StreamOut, Streams};

/// Events kept per paired server for replay to a reloaded window when the program has no other
/// figure.
pub const DEFAULT_SERVER_REPLAY_EVENTS: usize = 1_000;
/// The capacity of the server event channel (step 1).
const SERVER_CHANNEL: usize = 1_024;
/// How long `close` waits for commands in flight.
const CLOSE_WAIT: Duration = Duration::from_secs(2);

/// What the desktop program passes in at construction.
#[derive(Debug, Clone)]
pub struct BridgeConfig {
    /// The desktop core's own backend id (minted at first start, held by the Journal).
    pub local_backend: BackendId,
    /// Events kept per paired server for replay to a reloaded window; default 1,000
    /// ([`DEFAULT_SERVER_REPLAY_EVENTS`]).
    pub server_replay_events: usize,
}

/// Everything the commands, the sink and the tasks share.
pub(crate) struct Inner {
    pub(crate) local_backend: BackendId,
    pub(crate) api: Arc<dyn ServiceApi>,
    pub(crate) sentences: Arc<dyn ErrorSentences>,
    pub(crate) connector: Arc<ServerConnector>,
    pub(crate) journal: Journal,
    pub(crate) hub: Arc<Hub>,
    pub(crate) streams: Arc<Streams>,
    server_rx: Mutex<Option<mpsc::Receiver<ServerEvent>>>,
    pub(crate) closing: AtomicBool,
    pub(crate) in_flight: AtomicUsize,
    pub(crate) idle: Notify,
    window_tokens: Mutex<HashMap<String, CancellationToken>>,
    subscription: Mutex<Option<SubscriptionId>>,
    run: Mutex<Option<CancellationToken>>,
    runtime: Option<tokio::runtime::Handle>,
    /// Every wait of the spec is multiplied by this (1.0 in a program; see `with_time_scale`).
    pub(crate) time_scale: f64,
}

/// The Service API's transport on Tauri IPC. Built once by the desktop program and stored with
/// `app.manage`; a clone shares the one Bridge (the program keeps one for quit, Tauri one in its
/// managed state).
#[derive(Clone)]
pub struct CommandBridge {
    pub(crate) inner: Arc<Inner>,
}

impl CommandBridge {
    /// Builds the Bridge and the channel the Server Connector sends server events on. The program
    /// hands the returned sender to `ServerConnector::start(events, cancel)`, so the Bridge never
    /// constructs the Connector and the Connector never sees the Bridge (step 1, D6).
    pub fn new(
        cfg: BridgeConfig,
        api: Arc<dyn ServiceApi>,
        sentences: Arc<dyn ErrorSentences>,
        connector: Arc<ServerConnector>,
        journal: Journal,
    ) -> (CommandBridge, ServerEventSender) {
        let (tx, rx) = mpsc::channel(SERVER_CHANNEL);
        let inner = Inner {
            local_backend: cfg.local_backend,
            api,
            sentences,
            connector,
            journal,
            hub: Arc::new(Hub::new(cfg.local_backend, cfg.server_replay_events)),
            streams: Arc::new(Streams::default()),
            server_rx: Mutex::new(Some(rx)),
            closing: AtomicBool::new(false),
            in_flight: AtomicUsize::new(0),
            idle: Notify::new(),
            window_tokens: Mutex::new(HashMap::new()),
            subscription: Mutex::new(None),
            run: Mutex::new(None),
            runtime: tokio::runtime::Handle::try_current().ok(),
            time_scale: 1.0,
        };
        (
            CommandBridge {
                inner: Arc::new(inner),
            },
            tx,
        )
    }

    /// Test seam, in the manner of the Server Connector's stand-in: every wait of the spec (command
    /// deadlines, the quit wait) is multiplied by `scale`. Must be called before the Bridge is shared
    /// or started. A program never uses it.
    #[doc(hidden)]
    pub fn with_time_scale(mut self, scale: f64) -> CommandBridge {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.time_scale = scale;
        }
        self
    }

    /// Subscribes the Bridge's sink to the Journal and starts the task that reads the server channel.
    /// A second call does nothing. Cancelling `cancel` stops the server task.
    pub fn start(&self, cancel: CancellationToken) {
        let inner = &self.inner;
        let Some(mut rx) = inner
            .server_rx
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        else {
            return;
        };
        let sink: Arc<dyn EventSink> = Arc::new(BridgeSink {
            inner: Arc::downgrade(inner),
        });
        let id = inner.journal.subscribe(sink);
        *inner.subscription.lock().unwrap_or_else(|p| p.into_inner()) = Some(id);
        let own = cancel.child_token();
        *inner.run.lock().unwrap_or_else(|p| p.into_inner()) = Some(own.clone());
        let hub = Arc::clone(&inner.hub);
        let Some(runtime) = inner.runtime() else {
            log::no_runtime();
            return;
        };
        // Step 5: one task reads the server channel, in the server's order.
        runtime.spawn(async move {
            loop {
                let item = tokio::select! {
                    biased;
                    () = own.cancelled() => return,
                    item = rx.recv() => item,
                };
                match item {
                    Some(ServerEvent::Event { backend, envelope }) => {
                        hub.server_event(backend, envelope);
                    }
                    Some(ServerEvent::Reset { backend }) => hub.server_reset(backend),
                    None => return,
                }
            }
        });
    }

    /// Every event the Bridge delivers (local and server), each backend in seq order, for the
    /// desktop program's notifications (UX-A4, D6). Capacity 1,024; a slow receiver gets
    /// `RecvError::Lagged` and misses events, never delays the screens.
    pub fn subscribe_all(&self) -> broadcast::Receiver<Arc<EventEnvelope>> {
        self.inner.hub.subscribe_all()
    }

    /// Quit: new commands are refused with `BRG_CLOSING`; waits up to 2 s for commands in flight,
    /// closes every window stream and unsubscribes from the Journal.
    pub async fn close(&self) {
        let inner = &self.inner;
        inner.closing.store(true, Ordering::SeqCst);
        let wait = CLOSE_WAIT.mul_f64(inner.time_scale);
        let _ = tokio::time::timeout(wait, async {
            loop {
                let notified = inner.idle.notified();
                if inner.in_flight.load(Ordering::SeqCst) == 0 {
                    return;
                }
                notified.await;
            }
        })
        .await;
        inner.hub.close_all();
        inner.streams.close_all();
        if let Some(id) = inner
            .subscription
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            inner.journal.unsubscribe(id);
        }
        if let Some(run) = inner.run.lock().unwrap_or_else(|p| p.into_inner()).take() {
            run.cancel();
        }
    }

    /// The sender through which the desktop program passes unjournalled stream messages (dictation
    /// updates, terminal bytes, language-server messages) to the main window.
    pub fn streams(&self) -> ScreenStreams {
        ScreenStreams {
            streams: Arc::clone(&self.inner.streams),
        }
    }

    // ---- transport-neutral entry points
    //
    // The Tauri commands' bodies are these methods, public but hidden, so the routing and the event
    // fan-out are tested on every target (the Tauri plugin is built for macOS and Windows only) and
    // the Tauri commands stay one line each.

    /// One command from `window`: `name` is the dotted catalogue name, `envelope` the raw envelope
    /// text. Returns the reply JSON, or `None` when the window was closed while a local command ran
    /// (step 3: its reply is dropped).
    #[doc(hidden)]
    pub async fn invoke(&self, window: &str, name: &str, envelope: &str) -> Option<String> {
        self.inner.command(window, name, envelope).await
    }

    /// `service_events` for `window`, with batches going to `out`.
    #[doc(hidden)]
    pub async fn open_events(
        &self,
        window: &str,
        after: BTreeMap<BackendId, u64>,
        out: EventOut,
    ) -> Result<EventsOpened, BridgeError> {
        self.inner.open_events(window, after, out).await
    }

    /// `service_streams` for `window`, with messages going to `out`.
    #[doc(hidden)]
    pub fn open_streams(&self, window: &str, out: StreamOut) {
        let Some(runtime) = self.inner.runtime() else {
            log::no_runtime();
            return;
        };
        self.inner.streams.open(window, out, &runtime);
    }

    /// The window `window` was destroyed: its commands in flight are cancelled and its streams close
    /// (step 3).
    #[doc(hidden)]
    pub fn window_destroyed(&self, window: &str) {
        self.inner.window_destroyed(window);
    }

    /// The sink `start` registers with the Journal, for a test that drives it as the Journal does.
    #[doc(hidden)]
    pub fn event_sink(&self) -> Arc<dyn EventSink> {
        Arc::new(BridgeSink {
            inner: Arc::downgrade(&self.inner),
        })
    }
}

impl Inner {
    /// The runtime tasks are spawned on: the caller's, else the one current at construction.
    pub(crate) fn runtime(&self) -> Option<tokio::runtime::Handle> {
        tokio::runtime::Handle::try_current()
            .ok()
            .or_else(|| self.runtime.clone())
    }

    /// The cancellation token of `window`, created on its first command (step 3).
    pub(crate) fn window_token(&self, window: &str) -> CancellationToken {
        self.window_tokens
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entry(window.to_owned())
            .or_default()
            .clone()
    }

    fn window_destroyed(&self, window: &str) {
        if let Some(token) = self
            .window_tokens
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(window)
        {
            token.cancel();
        }
        self.hub.close_window(window);
        self.streams.close_window(window);
    }

    /// Step 6.
    async fn open_events(
        &self,
        window: &str,
        after: BTreeMap<BackendId, u64>,
        out: EventOut,
    ) -> Result<EventsOpened, BridgeError> {
        // Without a Tokio runtime (a fault of the host program, never of the window)
        // no flusher can run; the call is refused rather than leaving a stream that never delivers.
        let Some(runtime) = self.runtime() else {
            log::no_runtime();
            return Err(BridgeError::backend_unknown());
        };
        let connector = Arc::clone(&self.connector);
        let opening = self.hub.open(
            window,
            &after,
            |backend| connector.connection_for(backend).is_some(),
            out,
            &runtime,
        )?;
        let mut reset = opening.reset;
        if let Some(from) = opening.local_after {
            // 6.2: live local events are held while the Journal is read.
            match events::read_since(&self.journal, from).await {
                Ok(read) => {
                    let catch_up = read
                        .iter()
                        .filter_map(|event| self.hub.encode_local(event))
                        .collect();
                    self.hub.release(&opening.stream, catch_up);
                }
                Err(_) => {
                    // `JRNL_RESUME_TOO_OLD` or `JRNL_SEQ_AHEAD`; any other read failure (datastore
                    // unavailable) cannot honour `after` either and is answered the same.
                    reset.push(self.local_backend);
                    self.hub.release(&opening.stream, Vec::new());
                }
            }
        }
        log::stream(window, "open", reset.len() as u64);
        Ok(EventsOpened {
            local_backend: self.local_backend,
            reset,
        })
    }
}

/// The Bridge's `EventSink` (ADR-100 item 1). Holds the Bridge weakly: the Journal holds the sink.
struct BridgeSink {
    inner: Weak<Inner>,
}

#[async_trait::async_trait]
impl EventSink for BridgeSink {
    fn name(&self) -> &'static str {
        "command_bridge"
    }

    /// Step 4: only enqueues, never awaits a window, so the Journal's sink queue drains at once.
    async fn deliver(&self, event: &JournalEvent) {
        if let Some(inner) = self.inner.upgrade() {
            inner.hub.local(event);
        }
    }

    /// Step 4: the missed events are re-read in pages and delivered; when they are too old to read,
    /// every window gets a `reset` for the local backend instead.
    async fn lagged(&self, next_seq: u64) {
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        log::journal_lagged(next_seq);
        match events::read_since(&inner.journal, next_seq.saturating_sub(1)).await {
            Ok(missed) => {
                for event in &missed {
                    inner.hub.local(event);
                }
            }
            // `JRNL_RESUME_TOO_OLD`; any other read failure leaves the same hole and is answered the
            // same way.
            Err(_) => inner.hub.local_reset(),
        }
    }
}
