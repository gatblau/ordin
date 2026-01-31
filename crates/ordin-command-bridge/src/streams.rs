//! Unjournalled streams to the main window (`service_streams`; CommandBridge.md Internal Logic 8,
//! SA-RECON-01): dictation updates, terminal bytes and language-server messages. Nothing here is
//! journalled or replayed, and the Bridge never reads or logs a payload.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::error::StreamSendError;
use crate::log;

/// The capacity of a window's stream queue (step 8).
pub(crate) const STREAM_QUEUE: usize = 4_096;
/// The window that holds the `ordin:default` permission, and so the only stream `ScreenStreams` feeds.
pub(crate) const MAIN_WINDOW: &str = "main";
/// `brg.stream_lagging` is logged at most once per stream id in this interval.
const LAGGING_LOG_INTERVAL: Duration = Duration::from_secs(1);

/// What an unjournalled stream carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamKind {
    Dictation,
    Terminal,
    Lsp,
}

impl StreamKind {
    fn as_str(self) -> &'static str {
        match self {
            StreamKind::Dictation => "dictation",
            StreamKind::Terminal => "terminal",
            StreamKind::Lsp => "lsp",
        }
    }
}

/// One message on the unjournalled stream.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct StreamMessage {
    pub stream: uuid::Uuid,
    pub kind: StreamKind,
    /// Dictation: one `DictationUpdate` as JSON. Terminal: `{"bytes_b64": "<base64url, no padding>"}`.
    /// Lsp: the language server's JSON-RPC message. Closing: `{"closed": "<reason>"}`.
    pub payload: serde_json::Value,
}

/// Where a window's stream messages go: Tauri's `Channel::send` in the program. `Err` means the
/// window is gone and the stream closes.
pub type StreamOut = Box<dyn Fn(StreamMessage) -> Result<(), ()> + Send + Sync>;

/// One open `service_streams` channel.
struct Slot {
    generation: u64,
    tx: mpsc::Sender<StreamMessage>,
    cancel: CancellationToken,
}

/// The streams of every window, by label (one per label; step 8).
#[derive(Default)]
pub(crate) struct Streams {
    slots: Mutex<HashMap<String, Slot>>,
    next_generation: AtomicU64,
    lagging_logged: Mutex<HashMap<uuid::Uuid, Instant>>,
}

impl Streams {
    /// Opens the stream of `window`, closing any earlier one of that window, and starts its sender.
    // spec: ordin-desktop/components/CommandBridge#SA-RECON-01
    pub(crate) fn open(
        self: &Arc<Self>,
        window: &str,
        out: StreamOut,
        runtime: &tokio::runtime::Handle,
    ) {
        let (tx, mut rx) = mpsc::channel::<StreamMessage>(STREAM_QUEUE);
        let cancel = CancellationToken::new();
        let generation = self.next_generation.fetch_add(1, Ordering::SeqCst);
        let earlier = self.lock().insert(
            window.to_owned(),
            Slot {
                generation,
                tx,
                cancel: cancel.clone(),
            },
        );
        if let Some(earlier) = earlier {
            earlier.cancel.cancel();
            log::stream(window, "closed", 0);
        }
        log::stream(window, "open", 0);
        let streams = Arc::clone(self);
        let label = window.to_owned();
        runtime.spawn(async move {
            loop {
                let message = tokio::select! {
                    biased;
                    () = cancel.cancelled() => break,
                    message = rx.recv() => message,
                };
                let Some(message) = message else { break };
                // One `Channel::send` per message, without batching: dictation partials and terminal
                // echo are latency-sensitive (step 8).
                if out(message).is_err() {
                    streams.close_generation(&label, generation);
                    break;
                }
            }
        });
    }

    /// Closes the stream of `window` if it is still the one opened as `generation`.
    fn close_generation(&self, window: &str, generation: u64) {
        let mut slots = self.lock();
        if slots
            .get(window)
            .is_some_and(|s| s.generation == generation)
        {
            if let Some(slot) = slots.remove(window) {
                slot.cancel.cancel();
                log::stream(window, "closed", 0);
            }
        }
    }

    /// Closes the stream of `window`, if one is open (window destroyed).
    pub(crate) fn close_window(&self, window: &str) {
        if let Some(slot) = self.lock().remove(window) {
            slot.cancel.cancel();
            log::stream(window, "closed", 0);
        }
    }

    /// Closes every stream (quit).
    pub(crate) fn close_all(&self) {
        let slots: Vec<(String, Slot)> = self.lock().drain().collect();
        for (window, slot) in slots {
            slot.cancel.cancel();
            log::stream(&window, "closed", 0);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Slot>> {
        self.slots.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn send(&self, message: StreamMessage) -> Result<(), StreamSendError> {
        let stream = message.stream;
        let kind = message.kind;
        let mut slots = self.lock();
        let Some(slot) = slots.get(MAIN_WINDOW) else {
            return Err(StreamSendError::NoWindow);
        };
        match slot.tx.try_send(message) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => {
                drop(slots);
                self.log_lagging(stream, kind);
                Err(StreamSendError::Lagging)
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                // The sender task ended (the window went away): the stream is closed.
                if let Some(slot) = slots.remove(MAIN_WINDOW) {
                    slot.cancel.cancel();
                }
                Err(StreamSendError::NoWindow)
            }
        }
    }

    /// `brg.stream_lagging`, once per stream id per second.
    fn log_lagging(&self, stream: uuid::Uuid, kind: StreamKind) {
        let now = Instant::now();
        let mut logged = self
            .lagging_logged
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        logged.retain(|_, at| now.duration_since(*at) < LAGGING_LOG_INTERVAL);
        if let std::collections::hash_map::Entry::Vacant(entry) = logged.entry(stream) {
            entry.insert(now);
            log::stream_lagging(stream, kind.as_str());
        }
    }
}

/// Cloneable sender into the main window's unjournalled stream (`service_streams`).
#[derive(Clone)]
pub struct ScreenStreams {
    pub(crate) streams: Arc<Streams>,
}

impl ScreenStreams {
    /// Queues one message for the main window. Never awaits the WebView.
    /// `Err(StreamSendError::NoWindow)` when no window has opened `service_streams`;
    /// `Err(StreamSendError::Lagging)` when the window's stream queue (4,096 messages) is full.
    // spec: ordin-desktop/components/CommandBridge#SA-RECON-01
    pub fn send(
        &self,
        stream: uuid::Uuid,
        kind: StreamKind,
        payload: serde_json::Value,
    ) -> Result<(), StreamSendError> {
        self.streams.send(StreamMessage {
            stream,
            kind,
            payload,
        })
    }

    /// Queues the closing message of `stream` (`kind` unchanged, payload `{"closed": reason}`). When
    /// no window is open or its queue is full there is no one to tell, and nothing is queued.
    pub fn close(&self, stream: uuid::Uuid, kind: StreamKind, reason: &str) {
        let _ = self.send(stream, kind, serde_json::json!({ "closed": reason }));
    }
}
