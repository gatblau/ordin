//! A command arrives (CommandBridge.md Internal Logic 2): the body limit, the routing on `backend`,
//! the local `handle`, the Server Connector, and the one `brg.command` line. The Bridge never alters a
//! reply that came from the local `handle` or from a server.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use ordin_server_connector::ConnectorError;
use ordin_service_api::{
    handle, Budget, CallOrigin, CommandMeta, Incoming, Scope, ServiceError, MAX_BODY_BYTES,
    MAX_TEXT_SAVE_BODY_BYTES,
};
use ordin_types::{BackendId, CodedError, CommandRequestId, RepairAction};
use serde::Deserialize;

use crate::bridge::Inner;
use crate::error::CODE_CLOSING;
use crate::log;

/// The Service API (amended January 2026, ADR-109) and the ErrorHandling registry row of
/// `SVC_BODY_TOO_LARGE` name five commands with the 9 MiB limit. The Bridge follows the Service API,
/// so a body `accept` would take is never refused here first.
const TEXT_SAVE_COMMANDS: [&str; 5] = [
    "files.save_edit",
    "knowledge.save_edit",
    "projects.save_file",
    "localisation.import_language",
    "files.save_export",
];

/// The only keys the Bridge reads from an envelope (step 2.2); every other key is ignored.
#[derive(Deserialize)]
struct Peek {
    backend: BackendId,
    request_id: CommandRequestId,
}

/// The outcome of a server's reply, for the log line only.
#[derive(Deserialize)]
struct ReplyPeek {
    kind: String,
    #[serde(default)]
    error: Option<CodeOnly>,
}

#[derive(Deserialize)]
struct CodeOnly {
    code: String,
}

/// Decrements the in-flight count when a command ends, waking `close`.
struct InFlight<'a>(&'a Inner);

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        if self.0.in_flight.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.0.idle.notify_waiters();
        }
    }
}

// spec: ordin-desktop/components/ServiceApi#SVC-R1-7
/// The budget deadline of a command (ServiceApi.md Internal Logic 3): 10 s for `Interactive` and
/// `Search`, 60 s for `Transfer`, 11 minutes for `WaitsForPerson` (SVC-R1-7).
fn budget(meta: Option<&CommandMeta>) -> Duration {
    match meta.map(|m| m.budget) {
        Some(Budget::Transfer) => Duration::from_secs(60),
        Some(Budget::WaitsForPerson) => Duration::from_secs(660),
        _ => Duration::from_secs(10),
    }
}

/// The body limit of the command `name` (step 2.1).
fn body_limit(name: &str) -> usize {
    if TEXT_SAVE_COMMANDS.contains(&name) {
        MAX_TEXT_SAVE_BODY_BYTES
    } else {
        MAX_BODY_BYTES
    }
}

impl Inner {
    /// Step 2.
    pub(crate) async fn command(&self, window: &str, name: &str, body: &str) -> Option<String> {
        let started = Instant::now();
        self.in_flight.fetch_add(1, Ordering::SeqCst);
        let _guard = InFlight(self);
        let peek = serde_json::from_str::<Peek>(body).ok();
        let request_id = peek.as_ref().map(|p| p.request_id);
        let backend_kind = match &peek {
            Some(p) if p.backend != self.local_backend => "server",
            _ => "local",
        };
        // GracefulShutdown: a command after `close` began is refused.
        if self.closing.load(Ordering::SeqCst) {
            let reply = self.error_reply(request_id, CODE_CLOSING, None);
            self.log(request_id, name, backend_kind, CODE_CLOSING, started);
            return Some(reply);
        }
        let meta = ordin_service_api::command(name);
        let deadline = Instant::now() + budget(meta).mul_f64(self.time_scale);
        // 2.1 – 2.3: an oversized, unreadable or local envelope goes to `handle`, which produces the
        // exact `SVC_` refusal or runs the command. A body over the limit is never forwarded.
        let to_server = match (&peek, meta) {
            (Some(p), Some(meta))
                if body.len() <= body_limit(name) && p.backend != self.local_backend =>
            {
                Some((p.backend, meta))
            }
            _ => None,
        };
        let Some((backend, meta)) = to_server else {
            return self
                .local(window, name, body, deadline, request_id, started)
                .await;
        };
        // 2.4: a paired server's backend.
        if self.connector.connection_for(backend).is_some() {
            if meta.scope == Scope::Local {
                let code = "SVC_COMMAND_LOCAL_ONLY";
                self.log(request_id, name, "server", code, started);
                return Some(self.error_reply(request_id, code, None));
            }
            let sent = self
                .connector
                .send_command(
                    backend,
                    meta,
                    bytes::Bytes::copy_from_slice(body.as_bytes()),
                    deadline,
                )
                .await;
            let reply = match sent.map(|bytes| String::from_utf8(bytes.to_vec())) {
                Ok(Ok(reply)) => reply,
                Ok(Err(_)) => self.connector_error(request_id, &ConnectorError::BadAnswer),
                Err(error) => self.connector_error(request_id, &error),
            };
            let outcome = serde_json::from_str::<ReplyPeek>(&reply)
                .ok()
                .map(|r| match (r.kind.as_str(), r.error) {
                    ("ok", _) => "ok".to_owned(),
                    (_, Some(error)) => error.code,
                    (kind, None) => kind.to_owned(),
                })
                .unwrap_or_else(|| "unreadable".to_owned());
            self.log(request_id, name, "server", &outcome, started);
            return Some(reply);
        }
        // 2.5: neither local nor paired.
        let code = "SVC_BACKEND_UNKNOWN";
        self.log(request_id, name, "server", code, started);
        Some(self.error_reply(request_id, code, None))
    }

    /// 2.3: the desktop's own backend, through `ordin_service_api::handle` unchanged.
    async fn local(
        &self,
        window: &str,
        name: &str,
        body: &str,
        deadline: Instant,
        request_id: Option<CommandRequestId>,
        started: Instant,
    ) -> Option<String> {
        let reply = handle(
            self.api.as_ref(),
            self.sentences.as_ref(),
            Incoming {
                body: body.as_bytes(),
                routed_name: name,
                // `CallOrigin::LocalScreens` is set here, never read from input.
                origin: CallOrigin::LocalScreens,
                this_backend: self.local_backend,
                // spec: ordin-desktop/components/CommandBridge#SVC-RL-2
                groups: self.api.groups(),
            },
            deadline,
            self.window_token(window),
        )
        .await;
        if reply.json.is_empty() {
            // Step 3: the window was closed; the reply is dropped.
            self.log(request_id, name, "local", "cancelled", started);
            return None;
        }
        self.log(
            request_id,
            name,
            "local",
            reply.code.unwrap_or("ok"),
            started,
        );
        Some(reply.json)
    }

    /// A reply for a Server Connector failure (2.4). The Error Table promises `try_again` for
    /// `CONN_UNREACHABLE` and `CONN_DEADLINE`, so the Connector's own repair action fills in when the
    /// sentences give none.
    fn connector_error(
        &self,
        request_id: Option<CommandRequestId>,
        error: &ConnectorError,
    ) -> String {
        self.error_reply(request_id, error.code(), error.repair())
    }

    /// `{request_id, kind: "error", error: {code, message, action?}}`, worded by the program's
    /// sentences.
    pub(crate) fn error_reply(
        &self,
        request_id: Option<CommandRequestId>,
        code: &str,
        fallback: Option<RepairAction>,
    ) -> String {
        let error = ServiceError {
            code: code.to_owned(),
            message: self.sentences.sentence(code),
            action: self.sentences.action(code).or(fallback),
            reference: None,
        };
        let id = request_id.map_or(serde_json::Value::Null, |id| {
            serde_json::Value::String(id.to_string())
        });
        serde_json::json!({ "request_id": id, "kind": "error", "error": error }).to_string()
    }

    /// 2.6: `brg.command`, never with the envelope, input or result.
    fn log(
        &self,
        request_id: Option<CommandRequestId>,
        name: &str,
        backend_kind: &'static str,
        outcome: &str,
        started: Instant,
    ) {
        let request_id = request_id.map(|id| id.to_string()).unwrap_or_default();
        log::command(
            &request_id,
            name,
            backend_kind,
            outcome,
            u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        );
    }
}
