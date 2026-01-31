//! The component's log events (ServerConnector.md *Observability*; Logging.md). Each event is one
//! `tracing` call whose message is the event name. Never logged: the desktop key, the pairing code, the
//! pairing line, command bodies, replies, event data. Identifiers are recorded as strings: the line
//! formatter writes a field recorded through `Display` or `Debug` as `<debug>`.
//!
//! SPEC-GAP-1: the spec names a field `address` on `conn.paired`, but the Logging formatter refuses a
//! field called `address` (`ordin_types::log::FORBIDDEN_FIELDS`). The host is logged as `host`.

use ordin_types::ServerConnectionId;

use crate::MODULE;

pub(crate) fn paired(id: ServerConnectionId, host: &str) {
    tracing::info!(
        component = MODULE,
        server_connection_id = id.to_string().as_str(),
        host,
        "conn.paired"
    );
}

pub(crate) fn state(
    id: ServerConnectionId,
    from: &'static str,
    to: &'static str,
    reason_code: &str,
) {
    tracing::info!(
        component = MODULE,
        server_connection_id = id.to_string().as_str(),
        from,
        to,
        reason_code,
        "conn.state"
    );
}

pub(crate) fn reconnect(id: ServerConnectionId, attempt: u32, delay_ms: u64) {
    tracing::info!(
        component = MODULE,
        server_connection_id = id.to_string().as_str(),
        attempt = u64::from(attempt),
        delay_ms,
        "conn.reconnect"
    );
}

pub(crate) fn command(
    id: ServerConnectionId,
    command: &str,
    status: u64,
    elapsed_ms: u64,
    retries: u32,
) {
    tracing::debug!(
        component = MODULE,
        server_connection_id = id.to_string().as_str(),
        command,
        status,
        elapsed_ms,
        retries = u64::from(retries),
        "conn.command"
    );
}

pub(crate) fn event_unreadable(id: ServerConnectionId, seq: Option<u64>, code: &'static str) {
    tracing::warn!(
        component = MODULE,
        server_connection_id = id.to_string().as_str(),
        seq = seq.unwrap_or(0),
        code,
        "conn.event_unreadable"
    );
}

pub(crate) fn event_gap(id: ServerConnectionId, expected: u64, got: u64, code: &'static str) {
    tracing::warn!(
        component = MODULE,
        server_connection_id = id.to_string().as_str(),
        expected,
        got,
        code,
        "conn.event_gap"
    );
}

pub(crate) fn end_unconfirmed(id: ServerConnectionId) {
    tracing::warn!(
        component = MODULE,
        server_connection_id = id.to_string().as_str(),
        "conn.end_unconfirmed"
    );
}

pub(crate) fn wake(woken: usize) {
    tracing::info!(component = MODULE, woken = woken as u64, "conn.wake");
}

pub(crate) fn datastore_failed(id: ServerConnectionId, code: &'static str) {
    tracing::warn!(
        component = MODULE,
        server_connection_id = id.to_string().as_str(),
        code,
        "conn.datastore_failed"
    );
}
