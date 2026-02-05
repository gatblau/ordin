//! C. One connection: read exactly one request line, check it, dispatch it, write one or two reply
//! lines (2C wire §4: one request per connection). Every failure on this path denies (fail closed).

use std::panic::AssertUnwindSafe;
use std::sync::Mutex;

use futures::FutureExt;
use ordin_journal::NewJournalEntry;
use ordin_types::wire::hook::{HookReply, HookRequest};
use ordin_types::{HookCallId, RunId, WorkspaceId};
use serde_json::Value;
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};
use tokio::time::Instant;

use crate::consts::{nil_call_id, MAX_LINE_BYTES, PROTOCOL};
use crate::decide::{cut_reason, Answer, Link, Received};
use crate::relay::HookRelay;

// The fixed deny reasons of the Error Table.
pub(crate) const REASON_REFUSED: &str = "refused";
pub(crate) const REASON_MALFORMED: &str = "ordin could not read this request";
pub(crate) const REASON_UPDATING: &str = "ordin needs updating";
pub(crate) const REASON_UNKNOWN_SESSION: &str = "this session is not known to ordin";
pub(crate) const REASON_COULD_NOT_DECIDE: &str = "ordin could not decide";

/// One refusal of C.1 to C.8.
struct Refusal {
    code: &'static str,
    reason: &'static str,
    call_id: HookCallId,
    run_id: Option<RunId>,
    workspace_id: Option<WorkspaceId>,
    /// C.1 and C.6 are journalled (without the payload); the others are logged only.
    journal: bool,
}

impl Refusal {
    fn new(code: &'static str, reason: &'static str, call_id: HookCallId) -> Refusal {
        Refusal {
            code,
            reason,
            call_id,
            run_id: None,
            workspace_id: None,
            journal: false,
        }
    }
}

/// A request that parsed: `bad_call_id` is true when `call_id` was not a version-7 UUID (the field
/// then holds a placeholder and C.8 refuses the call as `HOOK_REPLAYED`).
struct Parsed {
    request: HookRequest,
    bad_call_id: bool,
}

/// C.3. `None` is `HOOK_MALFORMED`: not JSON, not UTF-8, an unknown field (a line from an older
/// ordin hook carries a `bridge` key), a wrong kind, or a missing field.
// spec: ordin-desktop/components/HookRelay#SA-RECON-01
fn parse_request(line: &[u8]) -> Option<Parsed> {
    let mut value: Value = serde_json::from_slice(line).ok()?;
    let object = value.as_object_mut()?;
    let mut bad_call_id = false;
    let raw = object.get("call_id")?.as_str()?.to_owned();
    if HookCallId::parse(&raw).is_err() {
        bad_call_id = true;
        object.insert(
            "call_id".to_owned(),
            Value::String(HookCallId::new().to_string()),
        );
    }
    let request: HookRequest = serde_json::from_value(value).ok()?;
    Some(Parsed {
        request,
        bad_call_id,
    })
}

/// C.2: one line of at most 1 MiB, newline included as the one byte more. `None` is a line that is
/// too long or that ends the stream before its newline.
async fn read_request_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> std::io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    let limit = MAX_LINE_BYTES as u64 + 1;
    let mut limited = AsyncReadExt::take(&mut *reader, limit);
    limited.read_until(b'\n', &mut line).await?;
    if line.last() != Some(&b'\n') {
        return Ok(None);
    }
    Ok(Some(line))
}

/// Writes one reply line and flushes. False when the peer has gone.
pub(crate) async fn write_line<W: AsyncWrite + Unpin>(writer: &mut W, reply: &HookReply) -> bool {
    let Ok(mut bytes) = serde_json::to_vec(reply) else {
        return false;
    };
    bytes.push(b'\n');
    writer.write_all(&bytes).await.is_ok() && writer.flush().await.is_ok()
}

/// A deny reply that could not be written; the hook sees a closed connection, which it treats as a
/// refusal too.
fn reply_not_written(reply: &'static str) {
    hook_log!(warn, reply, "hook.reply_not_written");
}

impl HookRelay {
    /// Serves one accepted connection. `peer_allowed` is the outcome of the platform's peer check
    /// (C.1): on Unix the peer's user id against the core's; on Windows always true, since the pipe
    /// refuses remote clients and every call is authenticated by its HMAC.
    pub(crate) async fn serve<S>(&self, stream: S, peer_allowed: bool)
    where
        S: AsyncRead + AsyncWrite + Unpin + Send,
    {
        let (read_half, mut write_half) = tokio::io::split(stream);
        let mut reader = BufReader::new(read_half);
        let known: Mutex<Option<HookCallId>> = Mutex::new(None);
        tokio::select! {
            _ = self.serve_body(&mut reader, &mut write_half, peer_allowed, &known) => {}
            // I: a connection still open when the shutdown grace ends is denied.
            _ = self.force_signal.cancelled() => {
                let call_id = known
                    .lock()
                    .ok()
                    .and_then(|slot| *slot)
                    .unwrap_or_else(nil_call_id);
                let reply = HookReply::Deny {
                    call_id,
                    reason: REASON_COULD_NOT_DECIDE.to_owned(),
                };
                if !write_line(&mut write_half, &reply).await {
                    reply_not_written("shutdown_deny");
                }
            }
        }
        let _ = write_half.shutdown().await;
    }

    async fn serve_body<R, W>(
        &self,
        reader: &mut R,
        writer: &mut W,
        peer_allowed: bool,
        known: &Mutex<Option<HookCallId>>,
    ) where
        R: AsyncBufRead + Unpin,
        W: AsyncWrite + Unpin + Send,
    {
        let started = Instant::now();
        // C.1
        if !peer_allowed {
            let mut refusal = Refusal::new("HOOK_PEER_REFUSED", REASON_REFUSED, nil_call_id());
            refusal.journal = true;
            self.refuse(writer, refusal).await;
            return;
        }
        // C.2
        let line = match tokio::time::timeout(self.timings.read, read_request_line(reader)).await {
            Ok(Ok(Some(line))) => line,
            _ => {
                let refusal = Refusal::new("HOOK_MALFORMED", REASON_MALFORMED, nil_call_id());
                self.refuse(writer, refusal).await;
                return;
            }
        };
        let received = Received::now();
        // C.3
        let Some(Parsed {
            request,
            bad_call_id,
        }) = parse_request(&line)
        else {
            let refusal = Refusal::new("HOOK_MALFORMED", REASON_MALFORMED, nil_call_id());
            self.refuse(writer, refusal).await;
            return;
        };
        let reply_id = if bad_call_id {
            nil_call_id()
        } else {
            request.call_id
        };
        if let Ok(mut slot) = known.lock() {
            *slot = Some(reply_id);
        }
        // C.4
        if request.protocol != PROTOCOL {
            let refusal = Refusal::new("HOOK_PROTOCOL_UNSUPPORTED", REASON_UPDATING, reply_id);
            self.refuse(writer, refusal).await;
            return;
        }
        // C.5
        let session = self
            .session(request.session)
            .filter(|_| self.ports.supervisor.is_live(request.session));
        let Some(session) = session else {
            let refusal = Refusal::new("HOOK_SESSION_UNKNOWN", REASON_UNKNOWN_SESSION, reply_id);
            self.refuse(writer, refusal).await;
            return;
        };
        let known_session = |mut refusal: Refusal| {
            refusal.run_id = Some(session.spec.run_id);
            refusal.workspace_id = Some(session.spec.workspace_id);
            refusal
        };
        // C.6
        let message = serde_json::to_value(&request.message).unwrap_or(Value::Null);
        if !session.mac_matches(&message, &request.mac) {
            let mut refusal = known_session(Refusal::new(
                "HOOK_UNAUTHENTICATED",
                REASON_REFUSED,
                reply_id,
            ));
            refusal.journal = true;
            self.refuse(writer, refusal).await;
            return;
        }
        // C.7
        if request.agent_instance_id != session.spec.agent_instance_id {
            let refusal = known_session(Refusal::new(
                "HOOK_SESSION_MISMATCH",
                REASON_REFUSED,
                reply_id,
            ));
            self.refuse(writer, refusal).await;
            return;
        }
        // C.8
        if bad_call_id || !session.first_sight(request.call_id) {
            let refusal = known_session(Refusal::new("HOOK_REPLAYED", REASON_REFUSED, reply_id));
            self.refuse(writer, refusal).await;
            return;
        }
        // C.9, C.10
        let run_id = session.spec.run_id;
        let mut link = Link::socket(writer, run_id, request.call_id);
        let answer = AssertUnwindSafe(self.dispatch(
            &session,
            request.call_id,
            request.message,
            received,
            &mut link,
        ))
        .catch_unwind()
        .await
        .unwrap_or_else(|_| {
            hook_log!(
                error,
                run_id = run_id.to_string().as_str(),
                code = "HOOK_INTERNAL",
                "hook_internal"
            );
            Answer::deny(
                request.call_id,
                REASON_COULD_NOT_DECIDE,
                Some("HOOK_INTERNAL"),
            )
        });
        link.finish(&answer.reply).await;
        self.log_decided(run_id, request.call_id, &answer, started, "socket");
    }

    /// Logs `hook_refused`, journals it when the row says so (the entry holds the code and never the
    /// payload), then writes the deny. The entry is written first, so a caller that sees the deny
    /// can rely on the record.
    async fn refuse<W: AsyncWrite + Unpin>(&self, writer: &mut W, refusal: Refusal) {
        match refusal.run_id {
            Some(run_id) => hook_log!(
                warn,
                run_id = run_id.to_string().as_str(),
                code = refusal.code,
                "hook_refused"
            ),
            None => hook_log!(warn, code = refusal.code, "hook_refused"),
        }
        if refusal.journal {
            self.journal_refused(&refusal).await;
        }
        let reply = HookReply::Deny {
            call_id: refusal.call_id,
            reason: cut_reason(refusal.reason),
        };
        if !write_line(writer, &reply).await {
            reply_not_written("refusal_deny");
        }
    }

    async fn journal_refused(&self, refusal: &Refusal) {
        let entry = NewJournalEntry {
            kind: "hook_refused".to_owned(),
            component: Self::MODULE.to_owned(),
            run_id: refusal.run_id,
            crew_run_id: None,
            workspace_id: refusal.workspace_id,
            summary: "A call to the ordin hook was refused.".to_owned(),
            data: serde_json::json!({ "code": refusal.code }),
        };
        if let Err(error) = self.journal.append_entry(entry).await {
            hook_log!(
                warn,
                code = ordin_types::CodedError::code(&error),
                "hook_journal_failed"
            );
        }
    }

    /// `hook_decided` {run_id, call_id, verdict, elapsed_ms, route}, with `code` when the answer
    /// is a refusal that has one.
    pub(crate) fn log_decided(
        &self,
        run_id: RunId,
        call_id: HookCallId,
        answer: &Answer,
        started: Instant,
        route: &'static str,
    ) {
        let verdict = match answer.reply {
            HookReply::Allow { .. } => "allow",
            HookReply::Deny { .. } => "deny",
            HookReply::Ack { .. } => "ack",
            HookReply::Holding { .. } => "holding",
        };
        let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        hook_log!(
            info,
            run_id = run_id.to_string().as_str(),
            call_id = call_id.to_string().as_str(),
            verdict,
            elapsed_ms,
            route,
            code = answer.code.unwrap_or("none"),
            "hook_decided"
        );
    }
}

#[cfg(test)]
mod discard_log_tests {
    use super::*;

    #[derive(Clone, Default)]
    struct Lines(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Lines {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("lines").extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// The text ordin's own log format writes while `f` runs.
    fn logged(f: impl FnOnce()) -> String {
        let lines = Lines::default();
        let writer = lines.clone();
        let subscriber = ordin_types::log::subscriber(move || writer.clone(), tracing::Level::INFO);
        tracing::subscriber::with_default(subscriber, f);
        let text = String::from_utf8(lines.0.lock().expect("lines").clone()).expect("utf-8");
        text
    }

    fn assert_readable(text: &str, event: &str, fields: &[&str]) {
        assert!(text.contains(&format!(r#""event":"{event}""#)), "{text}");
        for field in fields {
            assert!(text.contains(field), "{field} missing: {text}");
        }
        assert!(!text.contains("<debug>"), "{text}");
        assert!(!text.contains("invalid_event_name"), "{text}");
    }

    #[test]
    fn each_unwritten_deny_reply_is_logged_with_its_kind() {
        for reply in ["shutdown_deny", "refusal_deny"] {
            let text = logged(|| reply_not_written(reply));
            assert_readable(
                &text,
                "hook.reply_not_written",
                &[&format!(r#""reply":"{reply}""#)],
            );
        }
    }
}
