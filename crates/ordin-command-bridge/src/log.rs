//! The component's log events (CommandBridge.md Internal Logic 7, *Observability*; Logging.md). Each
//! event is one `tracing` call whose message is the event name. Never logged: the envelope body, the
//! input, the result, event data or a stream payload. Identifiers are recorded as strings: the line
//! formatter writes a field recorded through `Display` or `Debug` as `<debug>`.
//!
//! `brg.stream` carries `state` (`open` or `closed`), so the two lines step 7 asks for can be told
//! apart.

use ordin_types::BackendId;

/// The `component` field of every line.
pub(crate) const COMPONENT: &str = "command_bridge";

/// `brg.command` at `info`, once per command (step 2.6).
pub(crate) fn command(
    request_id: &str,
    command: &str,
    backend_kind: &'static str,
    outcome: &str,
    elapsed_ms: u64,
) {
    tracing::info!(
        component = COMPONENT,
        request_id,
        command,
        backend_kind,
        outcome,
        elapsed_ms,
        "brg.command"
    );
}

/// `brg.event_dropped` at `warn`: an event record that could not be encoded (steps 4 and 5).
pub(crate) fn event_dropped(backend: BackendId, seq: u64, kind: &str, code: &str) {
    tracing::warn!(
        component = COMPONENT,
        backend = backend.to_string().as_str(),
        seq,
        kind,
        code,
        "brg.event_dropped"
    );
}

/// `brg.stream` at `info` when a window stream opens or closes (steps 6 and 8).
pub(crate) fn stream(window: &str, state: &'static str, reset_count: u64) {
    tracing::info!(
        component = COMPONENT,
        window,
        state,
        reset_count,
        "brg.stream"
    );
}

/// `brg.stream_lagging` at `warn`, at most once per stream id per second (step 8).
pub(crate) fn stream_lagging(stream: uuid::Uuid, kind: &'static str) {
    tracing::warn!(
        component = COMPONENT,
        stream = stream.to_string().as_str(),
        kind,
        "brg.stream_lagging"
    );
}

/// The Bridge's Journal queue overflowed (step 4).
pub(crate) fn journal_lagged(next_seq: u64) {
    tracing::warn!(
        component = COMPONENT,
        code = crate::error::CODE_JOURNAL_LAGGED,
        next_seq,
        "brg.journal_lagged"
    );
}

/// A window stopped reading its events (step 6.6).
pub(crate) fn window_lagging(window: &str) {
    tracing::warn!(
        component = COMPONENT,
        code = crate::error::CODE_WINDOW_LAGGING,
        window,
        "brg.window_lagging"
    );
}

/// The host program called the Bridge outside a Tokio runtime, so a task could not start.
pub(crate) fn no_runtime() {
    tracing::error!(component = COMPONENT, "brg.no_runtime");
}
