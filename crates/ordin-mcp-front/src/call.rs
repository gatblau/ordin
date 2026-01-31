//! The call pipeline (McpFront.md Internal Logic E), progress relaying and the tool-set change pass
//! (Internal Logic C2).

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ordin_rules::{ApprovalWait, CallContext};
use ordin_service_api::ServiceEvent;
use ordin_types::wire::events::EventKind;
use ordin_types::wire::rules::{DecisionPoint, ToolCall, ToolOrigin, Verdict};
use ordin_types::wire::run::{ProgressCount, RunProgress};
use ordin_types::{CodedError, RunId};
use serde_json::{json, Value};
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::front::{offer_from, Inner, McpFront, Session};
use crate::provider::{CallVia, ProgressSink, ToolCallContext, ToolKey, ToolSpec};
use crate::types::{CallOutcome, ToolCallResult};
use crate::util::{arguments_digest, clip, lock};
use crate::work::WorkFailure;

/// The most a call's serialised arguments may be (LW-18).
const MAX_ARGUMENT_BYTES: usize = 1024 * 1024;
/// How long a call waits for one of the session's concurrent-call permits (Internal Logic 12).
const PERMIT_WAIT: Duration = Duration::from_secs(30);
/// At most one `RunProgress` journal event per second per call (Internal Logic 22).
const PROGRESS_INTERVAL: Duration = Duration::from_secs(1);
/// A progress line is at most this many characters (2C `RunProgress`).
const PROGRESS_LINE_CHARS: usize = 160;

/// Where a call's progress counts go on the bridge.
pub(crate) struct ProgressRelay {
    pub(crate) token: Value,
    pub(crate) tx: UnboundedSender<String>,
}

/// The kinds of end a call has, for the log line, the span and the metrics (Internal Logic 21).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Ok,
    Denied,
    Rejected,
    AwaitingApproval,
    Invalid,
    NotOffered,
    Timeout,
    Cancelled,
    Failed,
}

impl Outcome {
    fn word(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::Denied => "denied",
            Outcome::Rejected => "rejected",
            Outcome::AwaitingApproval => "awaiting_approval",
            Outcome::Invalid => "invalid",
            Outcome::NotOffered => "not_offered",
            Outcome::Timeout => "timeout",
            Outcome::Cancelled => "cancelled",
            Outcome::Failed => "failed",
        }
    }

    fn span_word(self) -> &'static str {
        match self {
            Outcome::Ok => "allowed",
            Outcome::Denied | Outcome::Rejected | Outcome::Invalid | Outcome::NotOffered => {
                "refused"
            }
            Outcome::AwaitingApproval => "paused",
            Outcome::Timeout | Outcome::Failed | Outcome::Cancelled => "failed",
        }
    }

    fn metric(self) -> CallOutcome {
        match self {
            Outcome::Ok => CallOutcome::Answered,
            Outcome::Denied
            | Outcome::Rejected
            | Outcome::AwaitingApproval
            | Outcome::Invalid
            | Outcome::NotOffered => CallOutcome::Refused,
            Outcome::Timeout | Outcome::Failed => CallOutcome::Failed,
            Outcome::Cancelled => CallOutcome::Cancelled,
        }
    }
}

/// What the pipeline learned beyond the result, for the end-of-call bookkeeping.
struct Ended {
    result: ToolCallResult,
    outcome: Outcome,
    control: String,
}

impl Ended {
    fn new(result: ToolCallResult, outcome: Outcome) -> Self {
        Ended {
            result,
            outcome,
            control: "none".to_owned(),
        }
    }
}

/// Runs one call through the pipeline: offered check, size check, argument check, Rules Engine
/// decision, provider call under a deadline, output limit, journal (Internal Logic 12 to 22).
// spec: ordin-desktop/components/McpFront#ADR-062
pub(crate) async fn run_call(
    inner: &Arc<Inner>,
    session: &Arc<Session>,
    key: ToolKey,
    arguments: Value,
    via: CallVia,
    external_cancel: Option<CancellationToken>,
    relay: Option<ProgressRelay>,
) -> ToolCallResult {
    let span = tracing::info_span!(
        "ordin.tool_call",
        tool = key.tool.as_str(),
        origin = "ordin_mcp_front",
        outcome = tracing::field::Empty,
        control = tracing::field::Empty,
    );
    let started = Instant::now();
    let local_id = uuid::Uuid::now_v7();
    let correlation_id = format!("{}:{}", session.grant.run_id, local_id);
    let digest = arguments_digest(&arguments);
    session.calls.fetch_add(1, Ordering::SeqCst);

    let ended = pipeline(
        inner,
        session,
        &key,
        arguments,
        via,
        external_cancel,
        relay,
        &correlation_id,
    )
    .instrument(span.clone())
    .await;

    match ended.outcome {
        Outcome::Denied | Outcome::Rejected => {
            session.denied.fetch_add(1, Ordering::SeqCst);
        }
        Outcome::Timeout | Outcome::Failed => {
            session.failed.fetch_add(1, Ordering::SeqCst);
        }
        _ => {}
    }
    let seconds = started.elapsed().as_secs_f64();
    span.record("outcome", ended.outcome.span_word());
    span.record("control", ended.control.as_str());
    // The agent profile feeds Telemetry's families 48 and 49 through the program's sink.
    // spec: ordin-desktop/components/McpFront#SA-RECON-09
    if inner.config.profile == crate::types::ProgramProfile::AgentProfile {
        if let Some(metrics) = &inner.metrics {
            metrics.call_finished(&key.server, ended.outcome.metric(), seconds);
        }
    }
    tracing::info!(
        component = "mcp_front",
        correlation_id = %correlation_id,
        run_id = %session.grant.run_id,
        call_id = local_id.to_string().as_str(),
        server = key.server.as_str(),
        tool = key.tool.as_str(),
        via = match via {
            CallVia::Harness => "harness",
            CallVia::Sandbox => "sandbox",
        },
        outcome = ended.outcome.word(),
        code = ended.result.code.as_deref().unwrap_or(""),
        duration_ms = started.elapsed().as_millis() as u64,
        arguments_digest = digest.as_str(),
        output_chars = ended.result.text.chars().count() as u64,
        "mcp_front.call"
    );
    ended.result
}

#[allow(clippy::too_many_arguments)]
async fn pipeline(
    inner: &Arc<Inner>,
    session: &Arc<Session>,
    key: &ToolKey,
    arguments: Value,
    via: CallVia,
    external_cancel: Option<CancellationToken>,
    relay: Option<ProgressRelay>,
    correlation_id: &str,
) -> Ended {
    let grant = &session.grant;
    let shutdown = inner.shutdown.get().cloned().unwrap_or_default();
    // A call is cancelled by the harness (the external token), by session close or by shutdown.
    let external = external_cancel.unwrap_or_default();
    let call_cancel = session.cancel.child_token();
    let cancelled = {
        let call_cancel = call_cancel.clone();
        let external = external.clone();
        async move {
            tokio::select! {
                _ = call_cancel.cancelled() => {}
                _ = external.cancelled() => {}
            }
        }
    };
    let cancelled_end = |shutdown: &CancellationToken| -> Ended {
        if shutdown.is_cancelled() {
            Ended::new(
                ToolCallResult::failure(
                    "MCP_SHUTTING_DOWN",
                    "ordin is stopping; try again after it restarts.",
                ),
                Outcome::Cancelled,
            )
        } else if session.cancel.is_cancelled() {
            Ended::new(
                ToolCallResult::failure("MCP_SESSION_CLOSED", "The session was closed."),
                Outcome::Cancelled,
            )
        } else {
            Ended::new(
                ToolCallResult::failure("MCP_CALL_CANCELLED", "The call was cancelled."),
                Outcome::Cancelled,
            )
        }
    };

    // 12. A permit from the session's semaphore.
    let permit = tokio::select! {
        biased;
        _ = cancelled => return cancelled_end(&shutdown),
        acquired = tokio::time::timeout(PERMIT_WAIT, session.semaphore.clone().acquire_owned()) => match acquired {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) => return cancelled_end(&shutdown),
            Err(_) => {
                return Ended::new(
                    ToolCallResult::failure(
                        "MCP_TOO_MANY_CALLS",
                        "Too many calls at once; wait for one to finish.",
                    ),
                    Outcome::Failed,
                )
            }
        },
    };
    let _permit = permit;
    // `cancelled` was moved into the select above; build another waiter for the later stages.
    let cancelled = {
        let call_cancel = call_cancel.clone();
        let external = external.clone();
        async move {
            tokio::select! {
                _ = call_cancel.cancelled() => {}
                _ = external.cancelled() => {}
            }
        }
    };
    tokio::pin!(cancelled);

    // 14. Offered check.
    let entry = lock(&session.offered).get(key).cloned();
    let Some((provider_index, spec)) =
        entry.filter(|(_, spec)| via == CallVia::Harness || spec.sandbox_callable)
    else {
        return Ended::new(
            ToolCallResult::failure(
                "MCP_TOOL_NOT_OFFERED",
                "This tool is not offered to this session.",
            ),
            Outcome::NotOffered,
        );
    };
    let slots = inner.providers();
    let Some(slot) = slots.get(provider_index).cloned() else {
        return Ended::new(
            ToolCallResult::failure(
                "MCP_TOOL_NOT_OFFERED",
                "This tool is not offered to this session.",
            ),
            Outcome::NotOffered,
        );
    };

    // 15. Size check.
    let size = serde_json::to_vec(&arguments)
        .map(|v| v.len())
        .unwrap_or(usize::MAX);
    if size > MAX_ARGUMENT_BYTES {
        return Ended::new(
            ToolCallResult::failure(
                "MCP_ARGUMENTS_TOO_LARGE",
                "The arguments are larger than 1 MiB; pass a reference to the document instead.",
            ),
            Outcome::Invalid,
        );
    }

    // 16. Argument check, with no Rules Engine call.
    if let Err(problem) = slot.provider.check_arguments(key, &arguments) {
        tracing::debug!(
            component = "mcp_front",
            run_id = %grant.run_id,
            server = key.server.as_str(),
            tool = key.tool.as_str(),
            at = %problem.at,
            "mcp_front.arguments_invalid"
        );
        return Ended::new(
            ToolCallResult::failure(
                "MCP_ARGUMENTS_INVALID",
                format!(
                    "Wrong arguments for {} at {}: {}. Correct form: {}",
                    key.tool, problem.at, problem.reason, spec.argument_form
                ),
            ),
            Outcome::Invalid,
        );
    }

    // 17. Decision by the Rules Engine, which never fails (an internal error is a deny).
    let tool_call = ToolCall {
        run_id: grant.run_id,
        agent_instance_id: grant.agent_instance_id,
        workspace_id: grant.workspace_id,
        point: DecisionPoint::BeforeToolCall,
        tool: format!("mcp__{}__{}", key.server.as_str(), key.tool),
        origin: ToolOrigin::OrdinMcpFront {
            server: key.server.as_str().to_owned(),
        },
        arguments: arguments.clone(),
    };
    // spec: ordin-desktop/components/McpFront#SA-RECON-08
    let outcome = inner
        .rules
        .decide_tool_call(
            &tool_call,
            &CallContext {
                subagent: None,
                class: None,
            },
        )
        .await;
    let tool_request_id = outcome.tool_request_id;
    let control = outcome.decision.rule.rule_id.clone();
    let with_control = |mut ended: Ended| {
        ended.control = control.clone();
        ended
    };
    match outcome.decision.verdict {
        Verdict::Deny { reason } => {
            return with_control(Ended::new(
                ToolCallResult::failure("MCP_DENIED", reason),
                Outcome::Denied,
            ))
        }
        Verdict::Ask { .. } => {
            let Some(approval_id) = outcome.approval_id else {
                return with_control(decision_failed(
                    grant.run_id,
                    &tool_call.tool,
                    "no approval",
                ));
            };
            let until = tokio::time::Instant::now() + inner.config.approval_wait;
            let waited = tokio::select! {
                biased;
                _ = &mut cancelled => return with_control(cancelled_end(&shutdown)),
                waited = inner.rules.wait_for_approval(approval_id, Some(until)) => waited,
            };
            match waited {
                Ok(ApprovalWait::Approved) => {}
                Ok(ApprovalWait::Rejected) => {
                    return with_control(Ended::new(
                        ToolCallResult::failure("MCP_REJECTED", "The person rejected this."),
                        Outcome::Rejected,
                    ))
                }
                Ok(ApprovalWait::Expired) => {
                    return with_control(Ended::new(
                        ToolCallResult::failure(
                            "MCP_REJECTED",
                            "The approval expired without a decision.",
                        ),
                        Outcome::Rejected,
                    ))
                }
                Ok(ApprovalWait::Pending) => {
                    return with_control(Ended::new(
                        ToolCallResult::failure(
                            "MCP_AWAITING_APPROVAL",
                            "Waiting for the person's approval. Call again with the same arguments once they have decided.",
                        ),
                        Outcome::AwaitingApproval,
                    ))
                }
                Err(error) => {
                    return with_control(decision_failed(
                        grant.run_id,
                        &tool_call.tool,
                        CodedError::code(&error),
                    ))
                }
            }
        }
        Verdict::Allow => {}
    }

    // 18. Provider call under a deadline.
    if cancelled_now(&call_cancel, &external) {
        inner.rules.record_outcome(tool_request_id, false).await;
        return with_control(cancelled_end(&shutdown));
    }
    let deadline = tokio::time::Instant::now() + inner.config.call_timeout;
    let child = call_cancel.child_token();
    let progress = ProgressSink::new();
    let forwarder_done = CancellationToken::new();
    let forwarder = tokio::spawn(forward_progress(
        inner.clone(),
        progress.clone(),
        forwarder_done.clone(),
        grant.run_id,
        spec.plain.clone(),
        relay,
    ));
    let ctx = ToolCallContext {
        run_id: grant.run_id,
        workspace_id: grant.workspace_id,
        agent_instance_id: grant.agent_instance_id,
        caller: grant.caller,
        scope_id: grant.scope_id,
        coding_task_id: grant.coding_task_id,
        call_id: tool_request_id,
        correlation_id: correlation_id.to_owned(),
        deadline: deadline.into_std(),
        cancel: child.clone(),
        progress,
        only_needed_parts: inner.work.only_needed(),
        via,
    };
    let provider = slot.provider.clone();
    let work = slot.work.clone();
    let call_key = key.clone();
    let mut handle = tokio::spawn(async move {
        match work {
            Some(work) => work
                .run(&ctx, &call_key.tool, arguments)
                .await
                .map_err(|failure: WorkFailure| (failure.failure, failure.structured)),
            None => provider
                .call(ctx, &call_key, arguments)
                .await
                .map_err(|failure| (failure, None)),
        }
    });
    enum Ran {
        Joined(
            Result<
                Result<crate::provider::ToolOutput, (crate::provider::ToolFailure, Option<Value>)>,
                tokio::task::JoinError,
            >,
        ),
        Timeout,
        Cancelled,
    }
    let ran = tokio::select! {
        biased;
        joined = &mut handle => Ran::Joined(joined),
        _ = tokio::time::sleep_until(deadline) => Ran::Timeout,
        _ = &mut cancelled => Ran::Cancelled,
    };
    let ended = match ran {
        Ran::Timeout => {
            child.cancel();
            handle.abort();
            Ended::new(
                ToolCallResult::failure(
                    "MCP_CALL_TIMEOUT",
                    "The tool took too long and was stopped.",
                ),
                Outcome::Timeout,
            )
        }
        Ran::Cancelled => {
            child.cancel();
            handle.abort();
            cancelled_end(&shutdown)
        }
        Ran::Joined(Err(error)) => {
            if error.is_panic() {
                tracing::error!(
                    component = "mcp_front",
                    correlation_id = %correlation_id,
                    provider_id = slot.id,
                    "mcp_front.provider_panicked"
                );
                Ended::new(
                    ToolCallResult::failure("MCP_PROVIDER_PANICKED", "The tool failed."),
                    Outcome::Failed,
                )
            } else {
                Ended::new(
                    ToolCallResult::failure("MCP_PROVIDER_PANICKED", "The tool failed."),
                    Outcome::Failed,
                )
            }
        }
        Ran::Joined(Ok(Err((failure, structured)))) => Ended::new(
            ToolCallResult {
                is_error: true,
                text: failure.message,
                structured,
                code: Some(failure.code),
            },
            Outcome::Failed,
        ),
        Ran::Joined(Ok(Ok(output))) => {
            // 19. Output limit.
            let text = limit_output(inner, grant.run_id, correlation_id, output.text);
            Ended::new(
                ToolCallResult {
                    is_error: false,
                    text,
                    structured: output.structured,
                    code: None,
                },
                Outcome::Ok,
            )
        }
    };
    // Flush the last progress count before the call ends.
    forwarder_done.cancel();
    let _ = forwarder.await;
    // 20. The outcome of a call the Rules Engine allowed: the provider was invoked.
    inner.rules.record_outcome(tool_request_id, true).await;
    with_control(ended)
}

fn cancelled_now(call: &CancellationToken, external: &CancellationToken) -> bool {
    call.is_cancelled() || external.is_cancelled()
}

fn decision_failed(run_id: RunId, tool: &str, code: &str) -> Ended {
    tracing::error!(
        component = "mcp_front",
        run_id = %run_id,
        tool,
        code,
        "mcp_front.decision_failed"
    );
    Ended::new(
        ToolCallResult::failure(
            "MCP_DECISION_FAILED",
            "This call could not be checked, so it was not run.",
        ),
        Outcome::Failed,
    )
}

/// Step 19: trims long output with a reference to the rest, then caps the whole text.
fn limit_output(inner: &Inner, run_id: RunId, correlation_id: &str, text: String) -> String {
    let config = &inner.config;
    let mut text = text;
    if inner.work.only_needed() && text.chars().count() > config.trim_above_chars {
        let (kept, rest, kept_chars) = trim_at_line(&text, config.trim_keep_chars);
        if let Some(reference) = inner.work.store_rest(run_id, kept_chars, rest) {
            text = format!(
                "{kept}\n[Cut short. Call read_rest with reference \"{reference}\" and from {kept_chars} to read on.]"
            );
        }
    }
    if text.len() > config.max_output_bytes {
        let mut cut = config.max_output_bytes;
        while cut > 0 && !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push_str("\n[Cut at 4 MiB.]");
        // spec: ordin-desktop/components/McpFront#ADR-062
        tracing::warn!(
            component = "mcp_front",
            run_id = %run_id,
            correlation_id = %correlation_id,
            bytes = text.len() as u64,
            code = "MCP_OUTPUT_TRUNCATED",
            "mcp_front.output_truncated"
        );
    }
    text
}

/// Splits `text` after `keep` characters, backing up to the last line break when one lies within
/// the final 1,000 characters kept. Returns the kept part, the remainder and the kept length in
/// characters.
fn trim_at_line(text: &str, keep: usize) -> (String, String, u64) {
    let chars: Vec<char> = text.chars().collect();
    let mut cut = keep.min(chars.len());
    let window_start = cut.saturating_sub(1000);
    if let Some(at) = chars[window_start..cut].iter().rposition(|c| *c == '\n') {
        cut = window_start + at + 1;
    }
    let kept: String = chars[..cut].iter().collect();
    let rest: String = chars[cut..].iter().collect();
    (kept.trim_end_matches('\n').to_owned(), rest, cut as u64)
}

// spec: ordin-desktop/components/McpFront#ADR-068
/// Relays a call's progress counts to the bridge and, at most once a second, to the Journal
/// (Internal Logic 22).
async fn forward_progress(
    inner: Arc<Inner>,
    sink: ProgressSink,
    done: CancellationToken,
    run_id: RunId,
    plain: String,
    relay: Option<ProgressRelay>,
) {
    let mut pending: Option<(u64, Option<u64>)> = None;
    let mut last_emit: Option<tokio::time::Instant> = None;
    let mut finished = false;
    loop {
        let due = match (pending, last_emit) {
            (Some(_), Some(at)) => Some(at + PROGRESS_INTERVAL),
            _ => None,
        };
        tokio::select! {
            _ = sink.notified() => {}
            _ = done.cancelled() => { finished = true; }
            _ = async {
                match due {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending::<()>().await,
                }
            } => {}
        }
        if let Some((count, total)) = sink.take() {
            if total.is_some_and(|t| count > t) {
                tracing::debug!(component = "mcp_front", run_id = %run_id, "mcp_front.progress_dropped");
            } else {
                if let Some(relay) = &relay {
                    let mut params = json!({"progressToken": relay.token, "progress": count});
                    if let (Some(total), Some(map)) = (total, params.as_object_mut()) {
                        map.insert("total".to_owned(), json!(total));
                    }
                    let line = json!({"jsonrpc": "2.0", "method": "notifications/progress", "params": params});
                    let _ = relay.tx.send(line.to_string());
                }
                pending = Some((count, total));
            }
        }
        let may_emit = match last_emit {
            None => true,
            Some(at) => at.elapsed() >= PROGRESS_INTERVAL,
        };
        if let Some((count, total)) = pending {
            if may_emit || finished {
                emit_progress(&inner, run_id, &plain, count, total).await;
                pending = None;
                last_emit = Some(tokio::time::Instant::now());
            }
        }
        if finished {
            // One more look for a count reported just before the end.
            if let Some((count, total)) = sink.take() {
                if total.is_none_or(|t| count <= t) {
                    emit_progress(&inner, run_id, &plain, count, total).await;
                }
            }
            break;
        }
    }
}

async fn emit_progress(inner: &Inner, run_id: RunId, plain: &str, done: u64, total: Option<u64>) {
    let event = ServiceEvent::RunProgress {
        run_id,
        progress: RunProgress {
            line: clip(plain, PROGRESS_LINE_CHARS),
            count: Some(ProgressCount { done, total }),
        },
    };
    if let Err(error) = inner
        .journal
        .append_event(EventKind::RunProgress.as_str(), event.to_journal_data())
        .await
    {
        tracing::error!(
            component = "mcp_front",
            run_id = %run_id,
            code = CodedError::code(&error),
            "mcp_front.journal_failed"
        );
    }
}

/// One pass of `tools_changed(server)`: re-runs the serving provider's `offer` for every open
/// session and tells the bridges of the sessions whose tool set changed (Internal Logic 7a).
pub(crate) async fn tools_changed_pass(front: &McpFront, server: &crate::names::ServerName) {
    let inner = &front.inner;
    let slots = inner.providers();
    let Some(index) = slots.iter().position(|slot| {
        slot.claimed.contains(server)
            || (slot.prefix.is_some_and(|p| server.as_str().starts_with(p))
                && slot.provider.servers().iter().any(|s| &s.name == server))
    }) else {
        return;
    };
    let provider_id = slots[index].id;
    let sessions: Vec<Arc<Session>> = lock(&inner.sessions).values().cloned().collect();
    let (mut added, mut removed) = (0u64, 0u64);
    for session in &sessions {
        let before: Vec<ToolKey> = lock(&session.offered)
            .iter()
            .filter(|(_, (i, _))| *i == index)
            .map(|(k, _)| k.clone())
            .collect();
        let fresh: Result<Vec<(ToolKey, ToolSpec)>, _> =
            offer_from(inner, index, &slots, &session.grant).await;
        let after: BTreeMap<ToolKey, ToolSpec> = match fresh {
            Ok(items) => items
                .into_iter()
                .filter(|(key, _)| {
                    let launched = session.launched.contains(&key.server);
                    if !launched {
                        tracing::debug!(
                            component = "mcp_front",
                            run_id = %session.grant.run_id,
                            server = key.server.as_str(),
                            "mcp_front.tools_changed_unlaunched"
                        );
                    }
                    launched
                })
                .collect(),
            Err(failure) => {
                // Fail closed: a tool whose grant cannot be confirmed is withdrawn.
                tracing::warn!(
                    component = "mcp_front",
                    run_id = %session.grant.run_id,
                    provider_id,
                    code = failure.code.as_str(),
                    "mcp_front.tools_changed_failed"
                );
                BTreeMap::new()
            }
        };
        {
            let mut offered = lock(&session.offered);
            offered.retain(|_, (i, _)| *i != index);
            for (key, spec) in &after {
                offered.insert(key.clone(), (index, spec.clone()));
            }
        }
        added += after.keys().filter(|k| !before.contains(k)).count() as u64;
        removed += before.iter().filter(|k| !after.contains_key(k)).count() as u64;
        let mut changed_servers: Vec<crate::names::ServerName> = Vec::new();
        for key in before.iter().chain(after.keys()) {
            let in_before = before.contains(key);
            let in_after = after.contains_key(key);
            if in_before != in_after && !changed_servers.contains(&key.server) {
                changed_servers.push(key.server.clone());
            }
        }
        for changed in changed_servers {
            session.notify_server(
                &changed,
                &json!({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"})
                    .to_string(),
            );
        }
    }
    tracing::info!(
        component = "mcp_front",
        server = server.as_str(),
        sessions = sessions.len() as u64,
        added,
        removed,
        "mcp_front.tools_changed"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_prefers_a_line_break_near_the_cut() {
        let text = format!("{}\n{}", "a".repeat(50), "b".repeat(100));
        let (kept, rest, chars) = trim_at_line(&text, 80);
        assert_eq!(kept, "a".repeat(50));
        assert_eq!(chars, 51);
        assert!(rest.starts_with('b'));
        let flat = "x".repeat(200);
        let (kept, rest, chars) = trim_at_line(&flat, 120);
        assert_eq!((kept.len(), rest.len(), chars), (120, 80, 120));
    }

    #[test]
    fn outcome_words() {
        for (o, word, span, metric) in [
            (Outcome::Ok, "ok", "allowed", CallOutcome::Answered),
            (Outcome::Denied, "denied", "refused", CallOutcome::Refused),
            (
                Outcome::Rejected,
                "rejected",
                "refused",
                CallOutcome::Refused,
            ),
            (
                Outcome::AwaitingApproval,
                "awaiting_approval",
                "paused",
                CallOutcome::Refused,
            ),
            (Outcome::Invalid, "invalid", "refused", CallOutcome::Refused),
            (
                Outcome::NotOffered,
                "not_offered",
                "refused",
                CallOutcome::Refused,
            ),
            (Outcome::Timeout, "timeout", "failed", CallOutcome::Failed),
            (
                Outcome::Cancelled,
                "cancelled",
                "failed",
                CallOutcome::Cancelled,
            ),
            (Outcome::Failed, "failed", "failed", CallOutcome::Failed),
        ] {
            assert_eq!(o.word(), word);
            assert_eq!(o.span_word(), span);
            assert_eq!(o.metric(), metric);
        }
    }
}
