//! The envelope functions both transports call (ServiceApi.md *Envelope functions*; Internal Logic
//! 2–5): `accept` checks an incoming command, `handle` runs it to a reply.

use std::panic::AssertUnwindSafe;
use std::time::Instant;

use futures::FutureExt;
use ordin_types::error_registry;
use ordin_types::text::normalise_text;
use ordin_types::{BackendId, CommandRequestId, RepairAction};
use serde::Deserialize;
use serde_json::Value;

use crate::api::{CallContext, CallOrigin, CommandMeta, ErrorSentences, Scope};
use crate::catalogue::{self, ServiceApi};
use crate::wire::{ServiceApiVersion, ServiceError};

/// The largest command body (LW-2).
pub const MAX_BODY_BYTES: usize = 1024 * 1024;
/// The largest body of `files.save_edit`, `knowledge.save_edit`, `projects.save_file`,
/// `localisation.import_language` and `files.save_export`.
pub const MAX_TEXT_SAVE_BODY_BYTES: usize = 9 * 1024 * 1024;
/// The largest serialised event `data` (LW-6).
pub use crate::events::MAX_EVENT_DATA_BYTES;

/// The commands whose body may reach [`MAX_TEXT_SAVE_BODY_BYTES`] (the *body of* row, amended
/// January 2026, ADR-109).
const TEXT_SAVE_COMMANDS: [&str; 5] = [
    "files.save_edit",
    "knowledge.save_edit",
    "projects.save_file",
    "localisation.import_language",
    "files.save_export",
];

/// A transport's view of one incoming command, after the transport authenticated the caller.
#[derive(Debug, Clone)]
pub struct Incoming<'a> {
    pub body: &'a [u8],
    /// For the Remote API: the command name from the path (`/v1/<module>/<verb>`); it must equal the
    /// envelope's `command`. For IPC: the Tauri command name mapped back to the dotted name.
    pub routed_name: &'a str,
    pub origin: CallOrigin,
    /// The backend this core is.
    pub this_backend: BackendId,
    /// The groups the serving `ServiceApi` answers, `api.groups()` (SVC-RL-2, amended January 2026,
    /// ADR-114).
    pub groups: &'a [&'static str],
}

/// A checked command, ready for `dispatch`.
#[derive(Debug, Clone)]
pub struct Accepted {
    pub meta: &'static CommandMeta,
    pub request_id: CommandRequestId,
    pub api_version: ServiceApiVersion,
    /// Re-serialised after `CommandInput::check` normalised it.
    pub input: Value,
}

/// A refusal before dispatch; `request_id` is absent when the body could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub request_id: Option<CommandRequestId>,
    pub code: &'static str,
    pub at: Option<String>,
}

/// The reply of one command.
#[derive(Debug, Clone)]
pub struct HandledReply {
    /// The serialised `CommandReply<serde_json::Value>`. Empty when the transport dropped the call
    /// (`cancel` fired): no reply is sent then.
    pub json: String,
    pub code: Option<&'static str>,
    /// HTTP status per the 2C status table (the Remote API uses it; IPC ignores it). 0 when no reply
    /// is sent (*chosen here*).
    pub http_status: u16,
}

/// The envelope as read before its parts are checked one by one (steps 2–7), so that a malformed
/// request id or backend gets its own code rather than the shape refusal.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEnvelope {
    api_version: ServiceApiVersion,
    request_id: String,
    command: String,
    backend: String,
    input: Value,
}

/// The request id alone, read from a body that is not an envelope (step 2).
#[derive(Deserialize)]
struct RequestIdOnly {
    request_id: CommandRequestId,
}

/// `^[a-z][a-z0-9_]{0,30}\.[a-z][a-z0-9_]{0,30}$`.
fn is_command_name(name: &str) -> bool {
    let part = |p: &str| {
        (1..=31).contains(&p.len())
            && p.starts_with(|c: char| c.is_ascii_lowercase())
            && p.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    };
    match name.split_once('.') {
        Some((module, verb)) => part(module) && part(verb),
        None => false,
    }
}

/// NFC-normalises every string of the input in place and refuses a control character other than
/// `\n` and `\t` (*any text field of any input* row), at the string's pointer.
fn normalise_input(value: &mut Value, at: &mut String) -> Result<(), String> {
    match value {
        Value::String(s) => {
            *s = normalise_text(s).map_err(|_| at.clone())?;
        }
        Value::Array(items) => {
            for (i, item) in items.iter_mut().enumerate() {
                let len = at.len();
                at.push_str(&format!("/{i}"));
                normalise_input(item, at)?;
                at.truncate(len);
            }
        }
        Value::Object(map) => {
            for (key, item) in map.iter_mut() {
                let len = at.len();
                at.push('/');
                at.push_str(&key.replace('~', "~0").replace('/', "~1"));
                normalise_input(item, at)?;
                at.truncate(len);
            }
        }
        _ => {}
    }
    Ok(())
}

/// Parses and checks an envelope: size, JSON, version, request id, command, group, scope, backend,
/// input. Returns the checked command ready for `dispatch`.
pub fn accept(incoming: &Incoming<'_>) -> Result<Accepted, Refusal> {
    let refuse = |request_id, code, at: Option<String>| Refusal {
        request_id,
        code,
        at,
    };
    // 1. Size.
    let limit = if TEXT_SAVE_COMMANDS.contains(&incoming.routed_name) {
        MAX_TEXT_SAVE_BODY_BYTES
    } else {
        MAX_BODY_BYTES
    };
    if incoming.body.len() > limit {
        return Err(refuse(None, "SVC_BODY_TOO_LARGE", None));
    }
    // 2. The envelope's shape.
    let raw: RawEnvelope = match serde_json::from_slice(incoming.body) {
        Ok(raw) => raw,
        Err(_) => {
            let request_id = serde_json::from_slice::<RequestIdOnly>(incoming.body)
                .ok()
                .map(|r| r.request_id);
            return Err(refuse(request_id, "SVC_INPUT_INVALID", None));
        }
    };
    // 3. Request id.
    let request_id = CommandRequestId::parse(&raw.request_id)
        .map_err(|_| refuse(None, "SVC_REQUEST_ID_MALFORMED", Some("/request_id".into())))?;
    let id = Some(request_id);
    // 4. Version.
    if !raw.api_version.in_range() {
        return Err(refuse(
            id,
            "SVC_VERSION_MALFORMED",
            Some("/api_version".into()),
        ));
    }
    if !ServiceApiVersion::desktop_accepts(raw.api_version, ServiceApiVersion::CURRENT) {
        return Err(refuse(
            id,
            "SVC_VERSION_UNSUPPORTED",
            Some("/api_version".into()),
        ));
    }
    // 5. Command: the name pattern, this version's catalogue, and the routed name.
    let meta = if is_command_name(&raw.command) && raw.command == incoming.routed_name {
        catalogue::command(&raw.command)
    } else {
        None
    };
    let Some(meta) = meta else {
        return Err(refuse(id, "SVC_COMMAND_UNKNOWN", Some("/command".into())));
    };
    // 5a. The group this build answers (SVC-RL-3, ADR-114).
    // spec: ordin-desktop/components/ServiceApi#SVC-RL-3
    if !incoming.groups.contains(&meta.module) {
        return Err(refuse(
            id,
            "SVC_NOT_IN_THIS_VERSION",
            Some("/command".into()),
        ));
    }
    // 6. Scope.
    if meta.scope == Scope::Local && matches!(incoming.origin, CallOrigin::PairedDesktop { .. }) {
        return Err(refuse(
            id,
            "SVC_COMMAND_LOCAL_ONLY",
            Some("/command".into()),
        ));
    }
    // 7. Backend.
    let backend = BackendId::parse(&raw.backend)
        .map_err(|_| refuse(id, "SVC_BACKEND_ID_MALFORMED", Some("/backend".into())))?;
    if backend != incoming.this_backend {
        return Err(refuse(id, "SVC_BACKEND_UNKNOWN", Some("/backend".into())));
    }
    // 8–9. Input: normalised text, then the typed read and the input's own check.
    let mut input = raw.input;
    let mut at = String::from("/input");
    normalise_input(&mut input, &mut at).map_err(|at| refuse(id, "SVC_INPUT_INVALID", Some(at)))?;
    let input = catalogue::check_input(meta, &input)
        .map_err(|rejection| refuse(id, rejection.code, Some(rejection.at)))?;
    // 10.
    Ok(Accepted {
        meta,
        request_id,
        api_version: raw.api_version,
        input,
    })
}

/// The HTTP status of an outcome (2C status table; Internal Logic 3).
pub(crate) fn http_status(code: Option<&str>) -> u16 {
    match code {
        None => 200,
        Some(
            "SVC_INPUT_INVALID"
            | "SVC_BODY_TOO_LARGE"
            | "SVC_REQUEST_ID_MALFORMED"
            | "SVC_VERSION_MALFORMED"
            | "SRV_BAD_CURSOR",
        ) => 422,
        Some("SVC_VERSION_UNSUPPORTED") => 409,
        Some(
            "SVC_COMMAND_UNKNOWN"
            | "SVC_COMMAND_LOCAL_ONLY"
            | "SVC_BACKEND_UNKNOWN"
            | "SVC_NOT_IN_THIS_VERSION",
        ) => 404,
        Some("SVC_DEADLINE") => 503,
        Some("SVC_INTERNAL") => 500,
        Some(_) => 200,
    }
}

/// The registry's `&'static` copy of a code, so a reply can name it without owning it.
fn static_code(code: &str) -> &'static str {
    error_registry::lookup(code).map_or("SVC_INTERNAL", |entry| entry.code)
}

/// One serialised reply.
fn reply_json(
    request_id: Option<CommandRequestId>,
    outcome: Result<Value, &ServiceError>,
) -> String {
    let id = request_id.map_or(Value::Null, |id| Value::String(id.to_string()));
    let body = match outcome {
        Ok(result) => serde_json::json!({"request_id": id, "kind": "ok", "result": result}),
        Err(error) => serde_json::json!({"request_id": id, "kind": "error", "error": error}),
    };
    body.to_string()
}

/// A `ServiceError` for one of this crate's codes, with the sentence and action of `sentences`.
fn own_error(
    sentences: &dyn ErrorSentences,
    code: &str,
    action: Option<RepairAction>,
) -> ServiceError {
    ServiceError {
        code: code.to_owned(),
        message: sentences.sentence(code),
        action: action.or_else(|| sentences.action(code)),
        reference: None,
    }
}

/// Logs `svc.command` (Internal Logic 5): never the input or result body.
fn log_command(
    request_id: Option<CommandRequestId>,
    command: &str,
    origin: &CallOrigin,
    outcome: &str,
    started: Instant,
) {
    let request_id = request_id.map(|id| id.to_string()).unwrap_or_default();
    tracing::info!(
        request_id = request_id.as_str(),
        command,
        origin = origin.log_name(),
        outcome,
        elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        "svc.command"
    );
}

/// Runs a whole command: accept → dispatch → reply. Never panics; a panic inside the implementation
/// is caught and answered as `SVC_INTERNAL`.
pub async fn handle(
    api: &dyn ServiceApi,
    sentences: &dyn ErrorSentences,
    incoming: Incoming<'_>,
    deadline: Instant,
    cancel: tokio_util::sync::CancellationToken,
) -> HandledReply {
    let started = Instant::now();
    let accepted = match accept(&incoming) {
        Ok(accepted) => accepted,
        Err(refusal) => {
            let error = own_error(sentences, refusal.code, None);
            log_command(
                refusal.request_id,
                incoming.routed_name,
                &incoming.origin,
                refusal.code,
                started,
            );
            return HandledReply {
                json: reply_json(refusal.request_id, Err(&error)),
                code: Some(refusal.code),
                http_status: http_status(Some(refusal.code)),
            };
        }
    };
    let ctx = CallContext {
        request_id: accepted.request_id,
        correlation_id: accepted.request_id.to_string(),
        origin: incoming.origin.clone(),
        deadline,
        cancel: cancel.clone(),
    };
    let call = AssertUnwindSafe(catalogue::dispatch(
        api,
        &ctx,
        accepted.meta,
        accepted.input,
    ))
    .catch_unwind();
    let timed = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), call);
    let outcome = tokio::select! {
        biased;
        () = cancel.cancelled() => {
            log_command(Some(accepted.request_id), accepted.meta.name, &ctx.origin, "cancelled", started);
            return HandledReply { json: String::new(), code: None, http_status: 0 };
        }
        outcome = timed => outcome,
    };
    let result = match outcome {
        Err(_elapsed) => Err(own_error(
            sentences,
            "SVC_DEADLINE",
            Some(RepairAction::TryAgain),
        )),
        Ok(Err(_panic)) => Err(own_error(
            sentences,
            "SVC_INTERNAL",
            Some(RepairAction::SendToIt),
        )),
        Ok(Ok(Ok(value))) => Ok(value),
        Ok(Ok(Err(mut error))) => {
            if error_registry::lookup(&error.code).is_none() {
                tracing::error!(
                    request_id = ctx.correlation_id.as_str(),
                    command = accepted.meta.name,
                    module_code = error.code.as_str(),
                    "svc.unregistered_code"
                );
                error = own_error(sentences, "SVC_INTERNAL", Some(RepairAction::SendToIt));
            } else if error.message.is_empty() {
                // An error `dispatch` raised itself carries no sentence yet.
                error.message = sentences.sentence(&error.code);
                error.action = error.action.or_else(|| sentences.action(&error.code));
            }
            Err(error)
        }
    };
    let code = result.as_ref().err().map(|e| static_code(&e.code));
    log_command(
        Some(accepted.request_id),
        accepted.meta.name,
        &ctx.origin,
        code.unwrap_or("ok"),
        started,
    );
    HandledReply {
        json: reply_json(Some(accepted.request_id), result.as_ref().cloned()),
        code,
        http_status: http_status(code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_names() {
        for good in ["approval.decide", "a.b", "team_knowledge.add_read_folder"] {
            assert!(is_command_name(good), "{good}");
        }
        for bad in [
            "approval",
            "Approval.decide",
            "a.1b",
            "a..b",
            ".b",
            "a.b.c",
            &format!("{}.b", "a".repeat(32)),
        ] {
            assert!(!is_command_name(bad), "{bad}");
        }
    }

    #[test]
    fn statuses() {
        assert_eq!(http_status(None), 200);
        assert_eq!(http_status(Some("SRV_BAD_CURSOR")), 422);
        assert_eq!(http_status(Some("SVC_NOT_IN_THIS_VERSION")), 404);
        assert_eq!(http_status(Some("APPR_CARD_ALREADY_DECIDED")), 200);
        assert_eq!(static_code("NOT_A_CODE"), "SVC_INTERNAL");
    }

    #[test]
    fn input_normalisation() {
        let mut v = serde_json::json!({"a": ["e\u{301}"], "b/c": {"d": "x\u{7}"}});
        let mut at = String::from("/input");
        assert_eq!(
            normalise_input(&mut v, &mut at).unwrap_err(),
            "/input/b~1c/d"
        );
        let mut v = serde_json::json!({"a": ["e\u{301}", 1, null]});
        let mut at = String::from("/input");
        normalise_input(&mut v, &mut at).unwrap();
        assert_eq!(v["a"][0], "é");
    }
}
