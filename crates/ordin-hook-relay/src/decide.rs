//! D and E: what the relay does with a request that passed C. The same code serves the socket and
//! the in-process path (F.2); only the way an interim `holding` line leaves differs.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use futures::FutureExt;
use ordin_approval::WaitOutcome;
use ordin_journal::NewJournalEntry;
use ordin_rules::{ApprovalWait, CallContext, EndDecision, RulesError};
use ordin_types::domain::runs::ApprovalState;
use ordin_types::time::Timestamp;
use ordin_types::wire::hook::{HookMessage, HookReply};
use ordin_types::wire::rules::{DecisionPoint, RuleRef, RuleSource, ToolCall, ToolOrigin, Verdict};
use ordin_types::{ApprovalId, HookCallId, RunId, ToolRequestId};
use tokio::io::{AsyncWrite, Sink};
use tokio::time::Instant;
use tracing::field::Empty;
use tracing::{Instrument, Span};

use crate::conn::{write_line, REASON_COULD_NOT_DECIDE, REASON_UNKNOWN_SESSION};
use crate::consts::{REASON_MAX_CHARS, SPAN_LABEL_MAX_CHARS};
use crate::relay::HookRelay;
use crate::session::Session;
use crate::store::{self, LateKind, LateRow};
use crate::types::HookSessionEvent;
use crate::writes::target_of;

const REASON_TIMEOUT: &str = "ordin could not decide in time";
const REASON_PENDING: &str = "waiting for the person's approval";
const REASON_REJECTED: &str = "The person rejected this action.";
const REASON_RUN_ENDED: &str = "The run ended before the person decided.";

/// The `Holding` event's tool for a held stop gate.
const STOP_TOOL: &str = "stop";

/// 1–500 characters (`HookReply::Deny.reason`): longer text is cut, an empty one becomes "refused".
pub(crate) fn cut_reason(reason: &str) -> String {
    let cut: String = reason.chars().take(REASON_MAX_CHARS).collect();
    if cut.is_empty() {
        "refused".to_owned()
    } else {
        cut
    }
}

/// When a request was received: the instant for deadlines, the wall-clock millisecond for the
/// `Holding.deadline` field.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Received {
    pub at: Instant,
    pub ms: i64,
}

impl Received {
    pub(crate) fn now() -> Received {
        Received {
            at: Instant::now(),
            ms: Timestamp::now().unix_millis(),
        }
    }
}

/// The final reply, and the registry code behind it when it is a refusal.
#[derive(Debug, Clone)]
pub(crate) struct Answer {
    pub reply: HookReply,
    pub code: Option<&'static str>,
}

impl Answer {
    pub(crate) fn allow(call_id: HookCallId) -> Answer {
        Answer {
            reply: HookReply::Allow { call_id },
            code: None,
        }
    }

    pub(crate) fn ack(call_id: HookCallId) -> Answer {
        Answer {
            reply: HookReply::Ack { call_id },
            code: None,
        }
    }

    pub(crate) fn deny(call_id: HookCallId, reason: &str, code: Option<&'static str>) -> Answer {
        Answer {
            reply: HookReply::Deny {
                call_id,
                reason: cut_reason(reason),
            },
            code,
        }
    }
}

/// How a call's interim and final lines leave. On the socket both are written to the connection; in
/// process there is no writer and a `holding` answer is only an event.
pub(crate) struct Link<'a, W> {
    out: Option<&'a mut W>,
    gone: bool,
    run_id: RunId,
    call_id: HookCallId,
}

impl<'a, W: AsyncWrite + Unpin> Link<'a, W> {
    pub(crate) fn socket(out: &'a mut W, run_id: RunId, call_id: HookCallId) -> Self {
        Link {
            out: Some(out),
            gone: false,
            run_id,
            call_id,
        }
    }

    fn note_gone(&mut self) {
        if !self.gone {
            self.gone = true;
            hook_log!(
                warn,
                run_id = self.run_id.to_string().as_str(),
                call_id = self.call_id.to_string().as_str(),
                "hook_peer_gone"
            );
        }
    }

    /// D.5.1: the interim line. A write failure is logged once; nothing else changes (C.10).
    async fn holding(&mut self, reply: &HookReply) {
        if self.gone {
            return;
        }
        let Some(out) = self.out.as_deref_mut() else {
            return;
        };
        if !write_line(out, reply).await {
            self.note_gone();
        }
    }

    /// C.10: the final line.
    pub(crate) async fn finish(&mut self, reply: &HookReply) {
        if self.gone {
            return;
        }
        let Some(out) = self.out.as_deref_mut() else {
            return;
        };
        if !write_line(out, reply).await {
            self.note_gone();
        }
    }
}

impl Link<'static, Sink> {
    pub(crate) fn in_process(run_id: RunId, call_id: HookCallId) -> Self {
        Link {
            out: None,
            gone: false,
            run_id,
            call_id,
        }
    }
}

/// The span field `control`: a control name when a declaration produced the decision, else `none`.
fn control_of(rule: &RuleRef) -> String {
    match rule.source {
        RuleSource::DeclarationGate
        | RuleSource::DeclarationToolRule
        | RuleSource::DeclarationArgumentRule => {
            rule.rule_id.chars().take(SPAN_LABEL_MAX_CHARS).collect()
        }
        _ => "none".to_owned(),
    }
}

struct PreToolUse {
    tool: String,
    origin: ToolOrigin,
    arguments: serde_json::Value,
    harness_tool_use_id: Option<String>,
    subagent: Option<String>,
}

impl HookRelay {
    /// D and E, by message kind.
    pub(crate) async fn dispatch<W: AsyncWrite + Unpin>(
        &self,
        session: &Arc<Session>,
        call_id: HookCallId,
        message: HookMessage,
        received: Received,
        link: &mut Link<'_, W>,
    ) -> Answer {
        match message {
            HookMessage::PreToolUse {
                tool,
                origin,
                arguments,
                harness_tool_use_id,
                subagent,
                ..
            } => {
                let pre = PreToolUse {
                    tool,
                    origin,
                    arguments,
                    harness_tool_use_id,
                    subagent,
                };
                self.pre_tool_use(session, call_id, received, link, pre)
                    .await
            }
            HookMessage::PostToolUse {
                tool,
                harness_tool_use_id,
                succeeded,
                ..
            } => {
                self.post_tool_use(session, call_id, tool, harness_tool_use_id, succeeded)
                    .await
            }
            HookMessage::Stop { .. } => self.stop(session, call_id, received, link).await,
        }
    }

    /// D. For a built-in tool the call runs inside the exported span `ordin.tool_call` with fields
    /// `tool`, `origin`, `outcome` and `control` only (Tracing allow list; T-D1).
    async fn pre_tool_use<W: AsyncWrite + Unpin>(
        &self,
        session: &Arc<Session>,
        call_id: HookCallId,
        received: Received,
        link: &mut Link<'_, W>,
        pre: PreToolUse,
    ) -> Answer {
        let span = if matches!(pre.origin, ToolOrigin::BuiltIn) {
            let label: String = pre.tool.chars().take(SPAN_LABEL_MAX_CHARS).collect();
            tracing::info_span!(
                "ordin.tool_call",
                tool = label.as_str(),
                origin = "built_in",
                outcome = Empty,
                control = Empty
            )
        } else {
            Span::none()
        };
        self.pre_tool_use_in(session, call_id, received, link, pre, &span)
            .instrument(span.clone())
            .await
    }

    async fn pre_tool_use_in<W: AsyncWrite + Unpin>(
        &self,
        session: &Arc<Session>,
        call_id: HookCallId,
        received: Received,
        link: &mut Link<'_, W>,
        pre: PreToolUse,
        span: &Span,
    ) -> Answer {
        let run_id = session.spec.run_id;
        // D.1: the sub-agent is the message's `subagent`, conditional on C-78: absent until the
        // spike shows the harness payload names it, and the Rules Engine then lists the reviewer's
        // restriction as not enforced (its fallback).
        // spec: ordin-desktop/components/HookRelay#C-78
        let call = ToolCall {
            run_id,
            agent_instance_id: session.spec.agent_instance_id,
            workspace_id: session.spec.workspace_id,
            point: DecisionPoint::BeforeToolCall,
            tool: pre.tool.clone(),
            origin: pre.origin.clone(),
            arguments: pre.arguments,
        };
        let ctx = CallContext {
            subagent: pre.subagent,
            class: None,
        };
        // D.2
        let decided = tokio::time::timeout(
            self.timings.decision,
            self.ports.rules.decide_tool_call(&call, &ctx),
        )
        .await;
        let Ok(outcome) = decided else {
            span.record("outcome", "failed");
            span.record("control", "none");
            session.emit(HookSessionEvent::Decided {
                tool: pre.tool,
                allowed: false,
            });
            return Answer::deny(call_id, REASON_TIMEOUT, Some("HOOK_DECISION_TIMEOUT"));
        };
        span.record("control", control_of(&outcome.decision.rule).as_str());
        let remember = |request: ToolRequestId| {
            if let Some(id) = &pre.harness_tool_use_id {
                session.remember_tool_use(id, request, pre.origin.clone());
            }
        };
        match outcome.decision.verdict {
            // D.3
            Verdict::Allow => {
                if let Err(answer) = self
                    .claim_write(session, call_id, &call, pre.harness_tool_use_id.as_deref())
                    .await
                {
                    span.record("outcome", "failed");
                    session.emit(HookSessionEvent::Decided {
                        tool: pre.tool,
                        allowed: false,
                    });
                    return answer;
                }
                remember(outcome.tool_request_id);
                span.record("outcome", "allowed");
                session.emit(HookSessionEvent::Decided {
                    tool: pre.tool,
                    allowed: true,
                });
                Answer::allow(call_id)
            }
            // D.4
            Verdict::Deny { reason } => {
                span.record("outcome", "refused");
                session.emit(HookSessionEvent::Decided {
                    tool: pre.tool,
                    allowed: false,
                });
                Answer::deny(call_id, &reason, None)
            }
            Verdict::Ask { .. } => match outcome.approval_id {
                Some(approval_id) => {
                    let answer = self
                        .hold_tool_call(
                            session,
                            call_id,
                            received,
                            link,
                            &pre.tool,
                            approval_id,
                            span,
                            remember,
                            outcome.tool_request_id,
                        )
                        .await;
                    if !matches!(answer.reply, HookReply::Allow { .. }) {
                        return answer;
                    }
                    match self
                        .claim_write(session, call_id, &call, pre.harness_tool_use_id.as_deref())
                        .await
                    {
                        Ok(()) => answer,
                        Err(refused) => {
                            span.record("outcome", "failed");
                            refused
                        }
                    }
                }
                // D.5.3: an engine defect.
                None => {
                    span.record("outcome", "failed");
                    hook_log!(
                        error,
                        run_id = run_id.to_string().as_str(),
                        code = "HOOK_INTERNAL",
                        "hook_internal"
                    );
                    session.emit(HookSessionEvent::Decided {
                        tool: pre.tool,
                        allowed: false,
                    });
                    Answer::deny(call_id, REASON_COULD_NOT_DECIDE, Some("HOOK_INTERNAL"))
                }
            },
        }
    }

    /// D.3a: an allowed call that can write tells the File Service first. A call whose writes would
    /// have no author does not run.
    // spec: ordin-desktop/components/HookRelay#SA-RECON-05
    async fn claim_write(
        &self,
        session: &Arc<Session>,
        call_id: HookCallId,
        call: &ToolCall,
        harness_tool_use_id: Option<&str>,
    ) -> Result<(), Answer> {
        // HR-ORIG-1: the hook names every MCP server a package's; ordin's own tools write nothing
        // a claim covers, so the origin is the Rules Engine's.
        // spec: ordin-desktop/components/HookRelay#HR-ORIG-1
        let origin = self.ports.rules.classify_origin(&call.origin);
        let Some(target) = target_of(&call.tool, &origin, &call.arguments) else {
            return Ok(());
        };
        let opened = tokio::time::timeout(
            self.timings.decision,
            self.ports.writes.open(call.run_id, &call.tool, target),
        )
        .await;
        match opened {
            Ok(Ok(claim)) => {
                if let (Some(claim), Some(id)) = (claim, harness_tool_use_id) {
                    session.remember_write_claim(id, claim);
                }
                Ok(())
            }
            Ok(Err(_)) => {
                hook_log!(
                    error,
                    run_id = call.run_id.to_string().as_str(),
                    code = "HOOK_STORE_UNAVAILABLE",
                    "hook_write_claim_failed"
                );
                Err(Answer::deny(
                    call_id,
                    REASON_COULD_NOT_DECIDE,
                    Some("HOOK_STORE_UNAVAILABLE"),
                ))
            }
            Err(_) => Err(Answer::deny(
                call_id,
                REASON_TIMEOUT,
                Some("HOOK_DECISION_TIMEOUT"),
            )),
        }
    }

    /// D.5: the engine asked a person. The interim `holding` line goes out at once; the wait runs
    /// until the hook's 50 s deadline.
    #[allow(clippy::too_many_arguments)]
    async fn hold_tool_call<W: AsyncWrite + Unpin>(
        &self,
        session: &Arc<Session>,
        call_id: HookCallId,
        received: Received,
        link: &mut Link<'_, W>,
        tool: &str,
        approval_id: ApprovalId,
        span: &Span,
        remember: impl Fn(ToolRequestId),
        request: ToolRequestId,
    ) -> Answer {
        let run_id = session.spec.run_id;
        let decided = |allowed: bool| {
            session.emit(HookSessionEvent::Decided {
                tool: tool.to_owned(),
                allowed,
            });
        };
        // D.5.1
        link.holding(&self.holding_reply(call_id, approval_id, received))
            .await;
        session.emit(HookSessionEvent::Holding {
            approval_id,
            tool: tool.to_owned(),
        });
        hook_log!(
            info,
            run_id = run_id.to_string().as_str(),
            approval_id = approval_id.to_string().as_str(),
            "hook_holding"
        );
        // D.5.2: the engine spends its approved-once record when the call runs now.
        let waited = self
            .ports
            .rules
            .wait_for_approval(approval_id, Some(received.at + self.timings.hold))
            .await;
        match waited {
            Ok(ApprovalWait::Approved) => {
                remember(request);
                span.record("outcome", "allowed");
                decided(true);
                Answer::allow(call_id)
            }
            Ok(ApprovalWait::Rejected) => {
                span.record("outcome", "refused");
                decided(false);
                Answer::deny(call_id, REASON_REJECTED, None)
            }
            Ok(ApprovalWait::Expired) => {
                span.record("outcome", "refused");
                decided(false);
                Answer::deny(call_id, REASON_RUN_ENDED, None)
            }
            // The hook gives up at its deadline: deny now, resume the session if the person decides later.
            Ok(ApprovalWait::Pending) => {
                span.record("outcome", "paused");
                match self
                    .record_late(session, approval_id, LateKind::ToolCall)
                    .await
                {
                    Ok(()) => {
                        session.emit(HookSessionEvent::DeniedAtDeadline {
                            approval_id,
                            tool: tool.to_owned(),
                        });
                        Answer::deny(call_id, REASON_PENDING, Some("HOOK_APPROVAL_PENDING"))
                    }
                    Err(code) => {
                        span.record("outcome", "failed");
                        decided(false);
                        Answer::deny(call_id, REASON_COULD_NOT_DECIDE, Some(code))
                    }
                }
            }
            Err(RulesError::ApprovalUnavailable) => {
                span.record("outcome", "failed");
                decided(false);
                Answer::deny(
                    call_id,
                    REASON_COULD_NOT_DECIDE,
                    Some("HOOK_APPROVAL_UNAVAILABLE"),
                )
            }
            Err(error) => {
                span.record("outcome", "failed");
                hook_log!(
                    error,
                    run_id = run_id.to_string().as_str(),
                    code = "HOOK_INTERNAL",
                    cause = ordin_types::CodedError::code(&error),
                    "hook_internal"
                );
                decided(false);
                Answer::deny(call_id, REASON_COULD_NOT_DECIDE, Some("HOOK_INTERNAL"))
            }
        }
    }

    fn holding_reply(
        &self,
        call_id: HookCallId,
        approval_id: ApprovalId,
        received: Received,
    ) -> HookReply {
        let hold_ms = i64::try_from(self.timings.hold.as_millis()).unwrap_or(i64::MAX);
        let deadline = Timestamp::from_unix_millis(received.ms.saturating_add(hold_ms))
            .unwrap_or_else(|_| Timestamp::now());
        HookReply::Holding {
            call_id,
            approval_id,
            deadline,
        }
    }

    /// Inserts the late-decision row and starts its watcher (D.5.2, E.2, G). The error is the code
    /// the call is denied with.
    async fn record_late(
        &self,
        session: &Arc<Session>,
        approval_id: ApprovalId,
        kind: LateKind,
    ) -> Result<(), &'static str> {
        let row = LateRow {
            approval_id,
            run_id: session.spec.run_id,
            kind,
        };
        match store::insert(&self.datastore, &row, Timestamp::now()).await {
            Ok(()) => {
                self.spawn_late_watcher(approval_id);
                Ok(())
            }
            Err(error) => {
                hook_log!(
                    error,
                    run_id = session.spec.run_id.to_string().as_str(),
                    code = "HOOK_STORE_UNAVAILABLE",
                    cause = error.to_string().as_str(),
                    "hook_internal"
                );
                Err("HOOK_STORE_UNAVAILABLE")
            }
        }
    }

    /// E.1: the after-tool event feeds the tool request recorded by D.3. It is always acknowledged.
    async fn post_tool_use(
        &self,
        session: &Arc<Session>,
        call_id: HookCallId,
        tool: String,
        harness_tool_use_id: Option<String>,
        succeeded: bool,
    ) -> Answer {
        // A failed call may still have written, so it settles too.
        let claim = harness_tool_use_id
            .as_deref()
            .and_then(|id| session.take_write_claim(id));
        if let Some(claim) = claim {
            if self.ports.writes.settle(&claim).await.is_err() {
                hook_log!(
                    warn,
                    run_id = session.spec.run_id.to_string().as_str(),
                    "hook_write_claim_left_open"
                );
            }
        }
        let found = harness_tool_use_id
            .as_deref()
            .and_then(|id| session.take_tool_use(id));
        let origin = match found {
            Some((request, origin)) => {
                self.ports.rules.record_outcome(request, succeeded).await;
                origin
            }
            None => {
                hook_log!(
                    info,
                    run_id = session.spec.run_id.to_string().as_str(),
                    "hook_post_unmatched"
                );
                ToolOrigin::BuiltIn
            }
        };
        session.emit(HookSessionEvent::ToolUsed {
            tool,
            origin,
            succeeded,
        });
        Answer::ack(call_id)
    }

    /// E.2: the end of the agent's turn.
    async fn stop<W: AsyncWrite + Unpin>(
        &self,
        session: &Arc<Session>,
        call_id: HookCallId,
        received: Received,
        link: &mut Link<'_, W>,
    ) -> Answer {
        let run_id = session.spec.run_id;
        session.emit(HookSessionEvent::TurnStopping);
        // E.3: Ordin's check of the result is made by the Ordin Host on the run's result (the relay
        // would exceed the 50 s deadline holding a stop event for a model turn); the relay journals
        // `hook_stop` for it.
        // spec: ordin-desktop/components/HookRelay#SA-HOOK-01
        let entry = NewJournalEntry {
            kind: "hook_stop".to_owned(),
            component: HookRelay::MODULE.to_owned(),
            run_id: Some(run_id),
            crew_run_id: None,
            workspace_id: Some(session.spec.workspace_id),
            summary: "The harness is about to end its turn.".to_owned(),
            data: serde_json::json!({}),
        };
        if let Err(error) = self.journal.append_entry(entry).await {
            hook_log!(
                warn,
                run_id = run_id.to_string().as_str(),
                code = ordin_types::CodedError::code(&error),
                "hook_journal_failed"
            );
        }
        let ended = tokio::time::timeout(
            self.timings.decision,
            self.ports.rules.decide_run_end(run_id),
        )
        .await;
        let Ok(decision) = ended else {
            self.log_stop(run_id, "timeout");
            return Answer::deny(call_id, REASON_TIMEOUT, Some("HOOK_DECISION_TIMEOUT"));
        };
        match decision {
            EndDecision::End => {
                self.log_stop(run_id, "end");
                Answer::ack(call_id)
            }
            // The ordin hook turns a deny to a stop event into the harness's "do not stop yet".
            EndDecision::Continue { message, .. } => {
                self.log_stop(run_id, "continue");
                Answer::deny(call_id, &message, None)
            }
            EndDecision::Ask { approval_id, .. } => {
                self.hold_stop(session, call_id, received, link, approval_id)
                    .await
            }
        }
    }

    fn log_stop(&self, run_id: RunId, outcome: &'static str) {
        hook_log!(
            info,
            run_id = run_id.to_string().as_str(),
            outcome,
            "hook_stop"
        );
    }

    /// E.2: the ending was refused often enough that a person decides whether to set the gates aside.
    /// The wait goes to the Approval Service directly, since no held call runs now.
    // spec: ordin-desktop/components/HookRelay#SA-RECON-03
    async fn hold_stop<W: AsyncWrite + Unpin>(
        &self,
        session: &Arc<Session>,
        call_id: HookCallId,
        received: Received,
        link: &mut Link<'_, W>,
        approval_id: ApprovalId,
    ) -> Answer {
        let run_id = session.spec.run_id;
        link.holding(&self.holding_reply(call_id, approval_id, received))
            .await;
        session.emit(HookSessionEvent::Holding {
            approval_id,
            tool: STOP_TOOL.to_owned(),
        });
        hook_log!(
            info,
            run_id = run_id.to_string().as_str(),
            approval_id = approval_id.to_string().as_str(),
            "hook_holding"
        );
        let waited = self
            .ports
            .approvals
            .wait(approval_id, Some(received.at + self.timings.hold))
            .await;
        let settled = |end: EndDecision| match end {
            EndDecision::End => {
                self.log_stop(run_id, "end");
                Answer::ack(call_id)
            }
            EndDecision::Continue { message, .. } => {
                self.log_stop(run_id, "continue");
                Answer::deny(call_id, &message, None)
            }
            EndDecision::Ask { .. } => {
                Answer::deny(call_id, REASON_COULD_NOT_DECIDE, Some("HOOK_INTERNAL"))
            }
        };
        match waited {
            Ok(WaitOutcome::Decided(ApprovalState::Approved)) => {
                settled(self.ports.rules.settle_end_gates(run_id, true).await)
            }
            Ok(WaitOutcome::Decided(ApprovalState::Rejected)) => {
                settled(self.ports.rules.settle_end_gates(run_id, false).await)
            }
            // Answered as D.5.2 answers an expired approval.
            Ok(WaitOutcome::Decided(ApprovalState::Expired)) => {
                self.log_stop(run_id, "expired");
                Answer::deny(call_id, REASON_RUN_ENDED, None)
            }
            Ok(WaitOutcome::Decided(ApprovalState::Pending)) | Ok(WaitOutcome::DeadlinePassed) => {
                self.log_stop(run_id, "pending");
                match self
                    .record_late(session, approval_id, LateKind::StopGate)
                    .await
                {
                    Ok(()) => Answer::deny(call_id, REASON_PENDING, Some("HOOK_APPROVAL_PENDING")),
                    Err(code) => Answer::deny(call_id, REASON_COULD_NOT_DECIDE, Some(code)),
                }
            }
            Err(error) => {
                self.log_stop(run_id, "unavailable");
                hook_log!(
                    warn,
                    run_id = run_id.to_string().as_str(),
                    code = "HOOK_APPROVAL_UNAVAILABLE",
                    cause = ordin_types::CodedError::code(&error),
                    "hook_internal"
                );
                Answer::deny(
                    call_id,
                    REASON_COULD_NOT_DECIDE,
                    Some("HOOK_APPROVAL_UNAVAILABLE"),
                )
            }
        }
    }

    /// F.2: the decision path of one hook call, in process: no peer, no line, no MAC, no replay set.
    // spec: ordin-desktop/components/HookRelay#SA-RECON-02
    pub(crate) async fn in_process(&self, run_id: RunId, message: HookMessage) -> HookReply {
        let started = Instant::now();
        let call_id = HookCallId::new();
        let session = self
            .session(run_id)
            .filter(|_| self.ports.supervisor.is_live(run_id));
        let Some(session) = session else {
            hook_log!(
                warn,
                run_id = run_id.to_string().as_str(),
                code = "HOOK_SESSION_UNKNOWN",
                "hook_refused"
            );
            return HookReply::Deny {
                call_id,
                reason: REASON_UNKNOWN_SESSION.to_owned(),
            };
        };
        let received = Received::now();
        let mut link = Link::in_process(run_id, call_id);
        let answer =
            AssertUnwindSafe(self.dispatch(&session, call_id, message, received, &mut link))
                .catch_unwind()
                .await
                .unwrap_or_else(|_| {
                    hook_log!(
                        error,
                        run_id = run_id.to_string().as_str(),
                        code = "HOOK_INTERNAL",
                        "hook_internal"
                    );
                    Answer::deny(call_id, REASON_COULD_NOT_DECIDE, Some("HOOK_INTERNAL"))
                });
        self.log_decided(run_id, call_id, &answer, started, "in_process");
        answer.reply
    }
}
