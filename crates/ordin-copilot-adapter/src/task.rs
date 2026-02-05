//! One run's task (CopilotLocalAdapter.md *Session events → run events*, the sign-in wait of
//! `start` step 6, `resume`, `cancel`): it takes the CLI's events, the Hook Relay's session events,
//! the verb calls, the sign-in poll and the run's time limit, one at a time.

use std::sync::Arc;
use std::time::Instant;

use futures::stream::{BoxStream, StreamExt};
use ordin_hook_relay::HookSessionEvent;
use ordin_runtime_adapter::{
    HarnessKind, PauseReason, RunEndReason, RunEvent, StartInput, UsageDelta,
};
use ordin_types::wire::hook::{HookMessage, HookReply};
use tokio::sync::mpsc;

use crate::adapter::Inner;
use crate::error::Failure;
use crate::gate::{classify, ServerNames};
use crate::port::{CopilotEvent, PortErrorKind};
use crate::progress::{plain_line, WAITING_FOR_APPROVAL};
use crate::run::{
    cut, lock, run_log, Run, RunCommand, MAX_FINAL_MESSAGE, MAX_SERVICE_MESSAGE, MAX_TURN_TEXT,
};
use crate::session::Live;

/// How a run's task begins.
pub(crate) enum TaskStart {
    /// Paused on `SignIn` before the session exists; `input` is the first turn once signed in.
    SignIn { input: StartInput },
    /// The session runs.
    Live(Live),
}

/// A sign-in being waited for: the next poll and when the attempt gives up.
struct SignInWait {
    next_poll: Instant,
    give_up: Instant,
}

/// A harness plan holds 3–7 steps of at most 80 characters (C-41).
const PLAN_STEPS: std::ops::RangeInclusive<usize> = 3..=7;
const MAX_PLAN_STEP: usize = 80;

struct Task {
    inner: Arc<Inner>,
    run: Arc<Run>,
    live: Option<Live>,
    /// The first turn still to send after a sign-in at start.
    pending_input: Option<StartInput>,
    sign_in: Option<SignInWait>,
    /// Tool calls since the last `Usage` event, and in all.
    tool_calls_since_usage: u32,
    tool_calls: u64,
}

async fn next_of<T>(stream: Option<&mut BoxStream<'static, T>>) -> Option<T> {
    match stream {
        Some(stream) => stream.next().await,
        None => futures::future::pending().await,
    }
}

async fn wait_until(at: Option<Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await,
        None => futures::future::pending().await,
    }
}

pub(crate) async fn run_task(
    inner: Arc<Inner>,
    run: Arc<Run>,
    mut inbox: mpsc::UnboundedReceiver<RunCommand>,
    start: TaskStart,
) {
    let mut task = Task {
        inner,
        run,
        live: None,
        pending_input: None,
        sign_in: None,
        tool_calls_since_usage: 0,
        tool_calls: 0,
    };
    match start {
        TaskStart::SignIn { input } => {
            task.pending_input = Some(input);
            task.begin_sign_in_wait();
        }
        TaskStart::Live(live) => task.live = Some(live),
    }
    while !task.run.is_ended() {
        let limit = task.live.as_ref().and_then(|live| live.limit);
        let poll = task.sign_in.as_ref().map(|wait| wait.next_poll);
        let (copilot, hooks) = match task.live.as_mut() {
            Some(live) => (Some(&mut live.copilot), Some(&mut live.hooks)),
            None => (None, None),
        };
        tokio::select! {
            command = inbox.recv() => match command {
                Some(command) => task.command(command).await,
                // The adapter is gone: nothing can stop this run any more.
                None => task.stop(RunEndReason::Interrupted).await,
            },
            event = next_of(copilot) => match event {
                Some(event) => task.copilot_event(event).await,
                // The port closed the session's events: the server is gone.
                None => task.server_exited().await,
            },
            event = next_of(hooks) => {
                if let Some(event) = event {
                    task.hook_event(event);
                } else if let Some(live) = task.live.as_mut() {
                    // The relay session closed; its events will not come back.
                    live.hooks = futures::stream::pending().boxed();
                }
            },
            () = wait_until(poll) => task.poll_sign_in().await,
            () = wait_until(limit) => {
                run_log!(info, task.run, "copilot.run.ended", reason = "stopped_at_limit");
                task.stop(RunEndReason::StoppedAtLimit).await;
            },
        }
    }
}

impl Task {
    fn begin_sign_in_wait(&mut self) {
        let now = Instant::now();
        let timings = self.inner.host.timings;
        self.sign_in = Some(SignInWait {
            next_poll: now + timings.sign_in_poll,
            give_up: now + timings.sign_in_give_up,
        });
    }

    /// Step 6.2: every 5 s for 15 minutes; then the run stays paused on `SignIn`.
    async fn poll_sign_in(&mut self) {
        let Some(server) = self.run.server() else {
            self.sign_in = None;
            return;
        };
        let signed_in = server
            .auth_status()
            .await
            .map(|status| status.signed_in)
            .unwrap_or(false);
        run_log!(
            info,
            self.run,
            "copilot.sign_in.status",
            signed_in = signed_in
        );
        if !signed_in {
            if let Some(wait) = self.sign_in.as_mut() {
                let now = Instant::now();
                if now >= wait.give_up {
                    // `resume(Answer)` starts the sign-in again.
                    self.sign_in = None;
                } else {
                    wait.next_poll = now + self.inner.host.timings.sign_in_poll;
                }
            }
            return;
        }
        // Step 6.3.
        self.sign_in = None;
        self.run.emit(RunEvent::Resumed);
        run_log!(info, self.run, "copilot.run.resumed");
        let Some(input) = self.pending_input.take() else {
            return;
        };
        match self.inner.open_session(&self.run, &input).await {
            Ok(Some(live)) => self.live = Some(live),
            Ok(None) => {}
            Err(failure) => self.fail_after_sign_in(failure).await,
        }
    }

    /// Steps 7–10 name `Err` results, but after a sign-in no caller is waiting for them, so the
    /// failure is logged with its code and the run ends `Failed`.
    async fn fail_after_sign_in(&mut self, failure: Failure) {
        run_log!(
            warn,
            self.run,
            "copilot.run.ended",
            reason = "failed",
            code = failure.code
        );
        self.run.emit_end(RunEndReason::Failed, None);
        self.inner.close_session(&self.run).await;
    }

    async fn command(&mut self, command: RunCommand) {
        match command {
            RunCommand::Resume { turn, ack } => {
                let sent = self.resume_with(&turn).await;
                let _ = ack.send(sent);
            }
            // spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CONV-3
            RunCommand::Answer { turn, ack } => {
                let sent = self.answer(&turn).await;
                if let Err(failure) = &sent {
                    self.end_failed(failure.code, None).await;
                }
                let _ = ack.send(sent);
            }
            RunCommand::RestartSignIn => {
                if let Some(server) = self.run.server() {
                    self.inner.start_login(&self.run, server.as_ref()).await;
                }
                self.begin_sign_in_wait();
            }
            RunCommand::Stop { reason, ack } => {
                self.stop(reason).await;
                let _ = ack.send(());
            }
        }
    }

    /// A tool approval was decided, or the person sent the next message: `Resumed`, then the turn.
    async fn resume_with(&mut self, turn: &str) -> Result<(), Failure> {
        *lock(&self.run.holding) = None;
        self.run.emit(RunEvent::Resumed);
        run_log!(info, self.run, "copilot.run.resumed");
        self.send(turn).await
    }

    /// The person's next message: `Resumed`, then the turn. A session the CLI no longer has is
    /// resumed with `resume_session` and the turn sent once more (CPA-HT-6).
    // spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CONV-3
    async fn answer(&mut self, turn: &str) -> Result<(), Failure> {
        self.run.emit(RunEvent::Resumed);
        run_log!(info, self.run, "copilot.run.resumed");
        let (Some(server), Some(session_id)) = (self.run.server(), self.run.session_id()) else {
            return self.send(turn).await;
        };
        match server.send(&session_id, turn).await {
            Ok(()) => return Ok(()),
            Err(error) if error.kind == PortErrorKind::SessionNotFound => {}
            Err(_) => return Err(self.send_failed()),
        }
        // spec: ordin-desktop/components/CopilotLocalAdapter#CPA-HT-6
        let options = self.inner.session_options(&self.run, self.names())?;
        if server.resume_session(&session_id, options).await.is_err() {
            run_log!(
                warn,
                self.run,
                "copilot.send.failed",
                code = "CPA_SEND_FAILED",
                missing = "session"
            );
            return Err(Failure::unreachable("CPA_SEND_FAILED", true));
        }
        run_log!(info, self.run, "copilot.session.resumed");
        if let Some(live) = self.live.as_mut() {
            // The resumed session's events come on a new stream.
            live.copilot = server.events(&session_id);
        }
        server
            .send(&session_id, turn)
            .await
            .map_err(|_| self.send_failed())
    }

    fn send_failed(&self) -> Failure {
        run_log!(
            warn,
            self.run,
            "copilot.send.failed",
            code = "CPA_SEND_FAILED"
        );
        Failure::unreachable("CPA_SEND_FAILED", true)
    }

    async fn send(&mut self, turn: &str) -> Result<(), Failure> {
        let (Some(server), Some(session_id)) = (self.run.server(), self.run.session_id()) else {
            run_log!(
                warn,
                self.run,
                "copilot.send.failed",
                code = "CPA_SEND_FAILED",
                missing = "session"
            );
            return Err(Failure::unreachable("CPA_SEND_FAILED", true));
        };
        server.send(&session_id, turn).await.map_err(|_| {
            run_log!(
                warn,
                self.run,
                "copilot.send.failed",
                code = "CPA_SEND_FAILED"
            );
            Failure::unreachable("CPA_SEND_FAILED", true)
        })
    }

    /// `cancel` and `shutdown`: `abort`, then `stop(stop_deadline)`, then `Ended`.
    async fn stop(&mut self, reason: RunEndReason) {
        if let (Some(server), Some(session_id)) = (self.run.server(), self.run.session_id()) {
            if server.abort(&session_id).await.is_err() {
                run_log!(warn, self.run, "copilot.abort.failed");
            }
        }
        self.end(reason, None).await;
    }

    /// Ends the run and takes everything down.
    async fn end(&mut self, reason: RunEndReason, final_message: Option<String>) {
        self.run.emit_end(reason, final_message);
        self.inner.close_session(&self.run).await;
        self.live = None;
        self.sign_in = None;
        run_log!(
            info,
            self.run,
            "copilot.run.ended",
            reason = ordin_types::log::wire_word(&reason).as_str()
        );
    }

    async fn end_failed(&mut self, code: &'static str, final_message: Option<String>) {
        run_log!(warn, self.run, "copilot.run.failed", code = code);
        self.end(RunEndReason::Failed, final_message).await;
    }

    // spec: ordin-desktop/components/CopilotLocalAdapter#CPA_SERVER_EXITED
    async fn server_exited(&mut self) {
        self.end_failed("CPA_SERVER_EXITED", None).await;
    }

    fn names(&self) -> ServerNames {
        ServerNames {
            front: lock(&self.run.front_servers).clone(),
            // The package servers' names stay known after their secrets are dropped.
            package: self
                .run
                .setup
                .package_servers
                .iter()
                .map(|p| p.name.clone())
                .chain(
                    lock(&self.run.package_servers)
                        .iter()
                        .map(|p| p.name.clone()),
                )
                .collect(),
        }
    }

    async fn copilot_event(&mut self, event: CopilotEvent) {
        self.run.touch();
        match event {
            CopilotEvent::SessionStarted => run_log!(info, self.run, "copilot.session.started"),
            CopilotEvent::AssistantText { .. } | CopilotEvent::ToolStarted { .. } => {}
            CopilotEvent::ToolFinished { tool, .. } => {
                self.tool_calls += 1;
                self.tool_calls_since_usage += 1;
                let origin = classify(&tool, &self.names());
                if let Some(line) = plain_line(&tool, &origin) {
                    self.run.emit_progress(line, Some(self.tool_calls));
                }
            }
            // Conditional on 10 C-41.
            CopilotEvent::TodoList { items } => {
                let valid = PLAN_STEPS.contains(&items.len())
                    && items.iter().all(|item| {
                        let length = item.chars().count();
                        length > 0 && length <= MAX_PLAN_STEP
                    });
                if valid {
                    self.run.emit(RunEvent::HarnessPlan { steps: items });
                } else {
                    run_log!(
                        info,
                        self.run,
                        "copilot.plan.invalid",
                        code = "RA_PLAN_INVALID"
                    );
                }
            }
            // One `Usage` event reports one model request: budgets for Copilot are counted in
            // requests (09 ADR-038). Token fields stay `None` where Copilot reports none (C-37).
            CopilotEvent::Usage {
                input_tokens,
                output_tokens,
                cached_input_tokens,
                ..
            } => {
                let tool_calls = std::mem::take(&mut self.tool_calls_since_usage);
                self.run.emit(RunEvent::Usage {
                    usage: UsageDelta {
                        model_turns: 1,
                        tool_calls,
                        input_tokens,
                        output_tokens,
                        cached_input_tokens,
                    },
                });
            }
            CopilotEvent::TurnEnded { final_text } => self.turn_ended(final_text).await,
            CopilotEvent::SignedOut => {
                // spec: ordin-desktop/components/CopilotLocalAdapter#CPA_SIGNED_OUT
                self.run.emit(RunEvent::Paused {
                    reason: PauseReason::SignIn,
                });
                run_log!(
                    info,
                    self.run,
                    "copilot.run.paused",
                    reason_kind = "sign_in",
                    code = "CPA_SIGNED_OUT"
                );
                if let Some(server) = self.run.server() {
                    self.inner.start_login(&self.run, server.as_ref()).await;
                }
                self.begin_sign_in_wait();
            }
            // spec: ordin-desktop/components/CopilotLocalAdapter#CPA_ALLOWANCE_EXHAUSTED
            CopilotEvent::AllowanceExhausted { message } => {
                self.end_failed(
                    "CPA_ALLOWANCE_EXHAUSTED",
                    Some(cut(&message, MAX_SERVICE_MESSAGE)),
                )
                .await;
            }
            // The message is for Diagnostics only; this crate has no edge to it, and it is never
            // logged.
            // spec: ordin-desktop/components/CopilotLocalAdapter#CPA_SESSION_ERROR
            CopilotEvent::Error { .. } => self.end_failed("CPA_SESSION_ERROR", None).await,
            CopilotEvent::ServerExited { .. } => self.server_exited().await,
        }
    }

    /// `TurnEnded`: the run-end decision, skipped while an approval is outstanding.
    async fn turn_ended(&mut self, final_text: Option<String>) {
        let outstanding = lock(&self.run.holding).is_some()
            || matches!(self.run.paused_on(), Some(PauseReason::Approval { .. }));
        if outstanding {
            return;
        }
        let reply = self
            .inner
            .hooks
            .decide_in_process(
                self.run.run_id,
                HookMessage::Stop {
                    harness: HarnessKind::CopilotCli,
                },
            )
            .await;
        match reply {
            HookReply::Ack { .. } | HookReply::Allow { .. } if self.run.is_conversation() => {
                self.end_conversation_turn(final_text);
            }
            HookReply::Ack { .. } | HookReply::Allow { .. } => {
                let message = final_text.map(|text| cut(&text, MAX_FINAL_MESSAGE));
                self.end(RunEndReason::Completed, message).await;
            }
            HookReply::Deny { reason, .. } => {
                // The Rules Engine counts refusals and asks a person after its own limit, which
                // arrives as `HookSessionEvent::Holding`.
                if let Err(failure) = self.send(&reason).await {
                    self.end_failed(failure.code, None).await;
                }
            }
            HookReply::Holding { approval_id, .. } => {
                *lock(&self.run.holding) = Some(approval_id);
            }
        }
    }

    /// A conversation's turn has ended: the run waits for the person's next message, keeping the
    /// Copilot session and the task's loop. The text itself is never logged.
    // spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CONV-2
    fn end_conversation_turn(&mut self, final_text: Option<String>) {
        let question = final_text
            .as_deref()
            .map(|text| cut(text, MAX_TURN_TEXT))
            .unwrap_or_default();
        self.run.emit(RunEvent::Paused {
            reason: PauseReason::Person { question },
        });
        run_log!(info, self.run, "copilot.run.paused", reason_kind = "person");
    }

    /// Hook Relay session events.
    fn hook_event(&mut self, event: HookSessionEvent) {
        self.run.touch();
        match event {
            HookSessionEvent::Holding { approval_id, .. } => {
                *lock(&self.run.holding) = Some(approval_id);
                self.run
                    .emit_progress(WAITING_FOR_APPROVAL.to_owned(), None);
            }
            HookSessionEvent::DeniedAtDeadline { approval_id, .. } => {
                let already = matches!(
                    self.run.paused_on(),
                    Some(PauseReason::Approval { approval_id: paused }) if paused == approval_id
                );
                *lock(&self.run.holding) = None;
                if !already {
                    self.run.emit(RunEvent::Paused {
                        reason: PauseReason::Approval { approval_id },
                    });
                    run_log!(
                        info,
                        self.run,
                        "copilot.run.paused",
                        reason_kind = "approval"
                    );
                }
            }
            HookSessionEvent::Decided { .. } => *lock(&self.run.holding) = None,
            HookSessionEvent::ToolUsed { .. } | HookSessionEvent::TurnStopping => {}
        }
    }
}
