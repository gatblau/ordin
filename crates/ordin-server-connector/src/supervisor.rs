//! The supervisor of one server (ServerConnector.md Internal Logic 6): the event stream, the silence
//! rule, the catch-up with `Last-Event-ID`, the reconnect loop.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use bytes::Bytes;
use futures::StreamExt as _;
use ordin_service_api::events::ServiceEvent;
use ordin_service_api::EventEnvelope;
use ordin_types::domain::server::ServerConnectionState;
use ordin_types::time::Timestamp;
use reqwest::header::{HeaderValue, ACCEPT, AUTHORIZATION};
use sse_stream::{Sse, SseByteStream};
use tokio_util::sync::CancellationToken;

use crate::command::{bearer, read_body};
use crate::conn::Conn;
use crate::error::{CODE_EVENT_GAP, CODE_EVENT_UNREADABLE};
use crate::types::{backoff_base, jitter_factor};
use crate::version::{self, Verdict};
use crate::wire::{read_reply, Shape};
use crate::{log, Inner, ServerEvent, ServerEventSender};

/// How one pass of steps 6.1 to 6.3 ended.
enum End {
    /// The connector is stopping or this server's supervisor was cancelled.
    Stop,
    /// 401, 409 or an unusable version: the state is set and the supervisor ends.
    Terminal,
    /// Reconnect at once, with the backoff reset (wake, probe).
    Immediate,
    /// Wait the backoff, then reconnect.
    Retry,
    /// `DESK_ADDRESS_LOCKED`: wait ten minutes.
    Locked,
}

/// A failure of the byte stream, carried through `sse-stream` as its body error.
#[derive(Debug)]
enum ChunkError {
    Silence,
    Broken,
}

impl std::fmt::Display for ChunkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChunkError::Silence => f.write_str("no byte within the silence limit"),
            ChunkError::Broken => f.write_str("stream broken"),
        }
    }
}

impl std::error::Error for ChunkError {}

/// Runs until the server is disconnected, refused, or the connector stops.
pub(crate) async fn run(
    inner: Arc<Inner>,
    conn: Arc<Conn>,
    events: ServerEventSender,
    global: CancellationToken,
    own: CancellationToken,
) {
    let mut wake = inner.wake.subscribe();
    let mut attempt = 0u32;
    loop {
        if own.is_cancelled() {
            break;
        }
        let end = session(&inner, &conn, &events, &own, &mut wake, &mut attempt).await;
        // The sequence number reached is kept whenever a stream ends (step 6.4).
        persist_sequence(&inner, &conn).await;
        match end {
            End::Stop | End::Terminal => break,
            End::Immediate => continue,
            End::Locked => {
                tokio::select! {
                    () = own.cancelled() => break,
                    () = tokio::time::sleep(inner.timing.locked()) => {}
                }
                continue;
            }
            End::Retry => {}
        }
        // Step 6.5: a lost link marks the server not connected, as of its last event.
        if conn.state() == ServerConnectionState::Connected {
            let as_of = {
                let live = conn.live.lock().unwrap_or_else(|p| p.into_inner());
                live.last_event_at
            }
            .unwrap_or_else(Timestamp::now);
            inner
                .set_state(
                    &conn,
                    ServerConnectionState::NotConnected,
                    "CONN_UNREACHABLE",
                    Some(as_of),
                    None,
                )
                .await;
        }
        // Step 6.6: wait the backoff, each wait multiplied by a random factor in [0.8, 1.2].
        let delay = backoff_base(attempt).mul_f64(jitter_factor());
        let scaled = inner.timing.scaled(delay);
        log::reconnect(conn.id, attempt + 1, scaled.as_millis() as u64);
        attempt = attempt.saturating_add(1);
        conn.waiting.store(true, Ordering::SeqCst);
        let woken = tokio::select! {
            () = own.cancelled() => None,
            () = tokio::time::sleep(scaled) => Some(false),
            r = wake.changed() => { let _ = r; Some(true) }
            () = conn.probe.notified() => Some(true),
        };
        conn.waiting.store(false, Ordering::SeqCst);
        match woken {
            None => break,
            Some(true) => attempt = 0,
            Some(false) => {}
        }
    }
    finish(&inner, &conn, &global).await;
}

/// Step 6.4 and 12: persist the last sequence number; on quit, `connected` is written as `not_connected`.
async fn finish(inner: &Inner, conn: &Conn, global: &CancellationToken) {
    persist_sequence(inner, conn).await;
    if global.is_cancelled() && conn.state() == ServerConnectionState::Connected {
        {
            let mut live = conn.live.lock().unwrap_or_else(|p| p.into_inner());
            live.state = ServerConnectionState::NotConnected;
        }
        if crate::store::update_state(
            &inner.pool,
            conn.id,
            ServerConnectionState::NotConnected,
            None,
        )
        .await
        .is_err()
        {
            log::datastore_failed(conn.id, "CONN_DATASTORE");
        }
    }
}

async fn persist_sequence(inner: &Inner, conn: &Conn) {
    let seq = conn.last_seq.load(Ordering::SeqCst);
    if seq == conn.persisted_seq.load(Ordering::SeqCst) {
        return;
    }
    if crate::store::update_sequence(&inner.pool, conn.id, seq)
        .await
        .is_ok()
    {
        conn.persisted_seq.store(seq, Ordering::SeqCst);
    } else {
        log::datastore_failed(conn.id, "CONN_DATASTORE");
    }
}

/// Steps 6.1 to 6.3 and 6.7: open the stream and handle it until it ends.
async fn session(
    inner: &Arc<Inner>,
    conn: &Arc<Conn>,
    events: &ServerEventSender,
    own: &CancellationToken,
    wake: &mut tokio::sync::watch::Receiver<u64>,
    attempt: &mut u32,
) -> End {
    let key = match inner.keys.get_desktop_key(&conn.keychain_ref).await {
        Ok(Some(key)) => key,
        Ok(None) => {
            inner
                .set_state(
                    conn,
                    ServerConnectionState::Disconnected,
                    "CONN_KEYCHAIN",
                    None,
                    None,
                )
                .await;
            return End::Terminal;
        }
        Err(_) => return End::Retry,
    };
    let Ok(bundle) = inner.bundle(conn).await else {
        return End::Retry;
    };
    bundle.reset_flag();
    let mut request = bundle
        .client
        .get(inner.url(conn, "/v1/events"))
        .header(ACCEPT, "text/event-stream")
        .header(AUTHORIZATION, bearer(&key));
    let last = conn.last_seq.load(Ordering::SeqCst);
    if last > 0 {
        request = request.header("Last-Event-ID", last.to_string());
    }
    let sent = tokio::select! {
        () = own.cancelled() => return End::Stop,
        r = tokio::time::timeout(inner.timing.silence(), request.send()) => r,
    };
    let response = match sent {
        Ok(Ok(response)) => response,
        Ok(Err(_)) | Err(_) => return End::Retry,
    };
    let status = response.status();
    let header = response.headers().get(version::HEADER).cloned();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        inner
            .set_state(
                conn,
                ServerConnectionState::Disconnected,
                "CONN_KEY_REFUSED",
                None,
                None,
            )
            .await;
        return End::Terminal;
    }
    if status == reqwest::StatusCode::CONFLICT {
        refuse_for_version(inner, conn, header.as_ref()).await;
        return End::Terminal;
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let locked = match read_body(response)
            .await
            .ok()
            .as_deref()
            .and_then(read_reply)
        {
            Some(Shape::Error(e)) => e.code == "DESK_ADDRESS_LOCKED",
            _ => false,
        };
        return if locked { End::Locked } else { End::Retry };
    }
    if status != reqwest::StatusCode::OK {
        return End::Retry;
    }
    match version::judge(inner.desktop, header.as_ref()) {
        Verdict::Unreadable => return End::Retry,
        Verdict::Unusable { .. } => {
            refuse_for_version(inner, conn, header.as_ref()).await;
            return End::Terminal;
        }
        Verdict::Usable(seen) => {
            inner
                .set_state(
                    conn,
                    ServerConnectionState::Connected,
                    "CONN_UNREACHABLE",
                    None,
                    Some(seen),
                )
                .await;
        }
    }
    *attempt = 0;
    stream(inner, conn, events, own, wake, response).await
}

/// Step 6.7 / 9: a version this desktop cannot use.
async fn refuse_for_version(inner: &Inner, conn: &Conn, header: Option<&HeaderValue>) {
    let seen = header
        .and_then(|v| v.to_str().ok())
        .and_then(version::parse);
    inner
        .set_state(
            conn,
            ServerConnectionState::RefusedForVersion,
            "CONN_REFUSED_FOR_VERSION",
            None,
            seen,
        )
        .await;
}

/// Step 6.3: the frames of one open stream.
async fn stream(
    inner: &Arc<Inner>,
    conn: &Arc<Conn>,
    events: &ServerEventSender,
    own: &CancellationToken,
    wake: &mut tokio::sync::watch::Receiver<u64>,
    response: reqwest::Response,
) -> End {
    let silence = inner.timing.silence();
    let stale_ms = inner.timing.stale().as_millis() as u64;
    let epoch = inner.epoch;
    let chunks = {
        let conn = Arc::clone(conn);
        futures::stream::unfold(response, move |mut response| {
            let conn = Arc::clone(&conn);
            async move {
                match tokio::time::timeout(silence, response.chunk()).await {
                    Ok(Ok(Some(bytes))) => {
                        conn.last_byte_ms
                            .store(epoch.elapsed().as_millis() as u64, Ordering::SeqCst);
                        Some((Ok::<Bytes, ChunkError>(bytes), response))
                    }
                    Ok(Ok(None)) => None,
                    Ok(Err(_)) => Some((Err(ChunkError::Broken), response)),
                    Err(_) => Some((Err(ChunkError::Silence), response)),
                }
            }
        })
    };
    conn.last_byte_ms
        .store(epoch.elapsed().as_millis() as u64, Ordering::SeqCst);
    let mut frames = Box::pin(SseByteStream::new(chunks));
    let mut persist = tokio::time::interval_at(
        tokio::time::Instant::now() + inner.timing.persist(),
        inner.timing.persist(),
    );
    loop {
        tokio::select! {
            () = own.cancelled() => return End::Stop,
            _ = persist.tick() => persist_sequence(inner, conn).await,
            r = wake.changed() => {
                let _ = r;
                let age = (epoch.elapsed().as_millis() as u64)
                    .saturating_sub(conn.last_byte_ms.load(Ordering::SeqCst));
                if age > stale_ms {
                    return End::Immediate;
                }
            }
            () = conn.probe.notified() => return End::Immediate,
            frame = frames.next() => match frame {
                Some(Ok(sse)) => {
                    if let Some(end) = handle_frame(conn, events, own, sse).await {
                        return end;
                    }
                }
                Some(Err(_)) | None => return End::Retry,
            },
        }
    }
}

/// Sends to the Bridge; a full channel waits (back-pressure on the stream, never dropping).
async fn deliver(own: &CancellationToken, events: &ServerEventSender, event: ServerEvent) -> bool {
    tokio::select! {
        () = own.cancelled() => false,
        r = events.send(event) => r.is_ok(),
    }
}

/// One frame (step 6.3). `Some(end)` stops the stream.
async fn handle_frame(
    conn: &Arc<Conn>,
    events: &ServerEventSender,
    own: &CancellationToken,
    sse: Sse,
) -> Option<End> {
    // A comment (heartbeat) only refreshes the silence timer, which the byte stream already did.
    let data = sse.data?;
    let frame_id = sse.id.as_deref().and_then(|s| s.trim().parse::<u64>().ok());
    let Ok(envelope) = serde_json::from_str::<EventEnvelope>(&data) else {
        log::event_unreadable(conn.id, frame_id, CODE_EVENT_UNREADABLE);
        return None;
    };
    if envelope.backend != conn.backend {
        log::event_unreadable(conn.id, Some(envelope.seq), CODE_EVENT_UNREADABLE);
        return None;
    }
    conn.live
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .last_event_at = Some(envelope.at);
    let last = conn.last_seq.load(Ordering::SeqCst);
    if matches!(envelope.event, ServiceEvent::Reset) {
        conn.clear_cache();
        let alive = deliver(
            own,
            events,
            ServerEvent::Reset {
                backend: conn.backend,
            },
        )
        .await;
        conn.last_seq
            .store(frame_id.unwrap_or(envelope.seq), Ordering::SeqCst);
        return (!alive).then_some(End::Stop);
    }
    if envelope.seq <= last {
        return None;
    }
    if envelope.seq > last + 1 {
        log::event_gap(conn.id, last + 1, envelope.seq, CODE_EVENT_GAP);
        if !deliver(
            own,
            events,
            ServerEvent::Reset {
                backend: conn.backend,
            },
        )
        .await
        {
            return Some(End::Stop);
        }
    }
    let seq = envelope.seq;
    if !deliver(
        own,
        events,
        ServerEvent::Event {
            backend: conn.backend,
            envelope: Arc::new(envelope),
        },
    )
    .await
    {
        return Some(End::Stop);
    }
    conn.last_seq.store(seq, Ordering::SeqCst);
    None
}
