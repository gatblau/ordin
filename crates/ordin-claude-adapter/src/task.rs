//! The one task per run: it owns the child process and turns the CLI's stdout lines, the Hook
//! Relay's events and the verb calls into run events (ClaudeLocalAdapter.md *Approach*, *Reading the
//! CLI's output*, *Hook Relay session events → run events*, *Pausing in the agent profile*, `cancel`,
//! *CLI exits unexpectedly*, *Sign-in flow*).

use std::collections::HashSet;
use std::sync::Arc;

use futures::stream::{BoxStream, StreamExt};
use ordin_hook_relay::HookSessionEvent;
use ordin_runtime_adapter::{PauseReason, RunEndReason, RunEvent, StartInput, UsageDelta};
use ordin_types::ApprovalId;
use tokio::process::Child;
use tokio::sync::mpsc;
use tokio::time::{sleep, sleep_until, Instant};
use tracing::Instrument;

use crate::adapter::Inner;
use crate::cli::{check_auth, claude_command, AuthCheck};
use crate::env::child_environment;
use crate::error::Failure;
use crate::process::{Line, Process};
use crate::progress::{plain_line, WAITING_FOR_APPROVAL};
use crate::run::{
    cut, lock, run_log, Run, RunCommand, MAX_FINAL_MESSAGE, MAX_SERVICE_MESSAGE, MAX_TURN_TEXT,
};
use crate::session::OpenStop;
use crate::stream::{parse_line, Malformed, StreamItem, TurnResult};

/// Consecutive malformed lines that end the run.
const MAX_MALFORMED: u32 = 20;

/// What a run's task begins with.
pub(crate) enum TaskStart {
    /// A process is running its first turn.
    Live(Box<Process>),
    /// The harness is signed out; `input` is the first turn once it is signed in.
    SignIn { input: StartInput },
    /// No process: a second instance resumes a paused run (agent profile).
    Parked,
}

/// How a process's stretch ended.
enum Stretch {
    /// The run has ended.
    Done,
    /// The run is paused and its process has ended (`resume`'s "process ended" branch).
    Parked,
}

/// The state of one process's stretch.
#[derive(Default)]
struct Turn {
    /// The approval of a held hook.
    holding: Option<ApprovalId>,
    /// Approvals already emitted as `Paused`.
    paused: HashSet<ApprovalId>,
    /// `ToolUsed` events since the last `Usage`.
    tool_calls: u32,
    /// Tool calls so far in the run.
    total_tool_calls: u64,
    malformed: u32,
    allowance_used_up: bool,
    /// When the paused process is ended, while on the desktop or the personal server.
    idle_until: Option<Instant>,
}

/// Runs the whole life of one run.
pub(crate) async fn run_task(
    inner: Arc<Inner>,
    run: Arc<Run>,
    mut inbox: mpsc::UnboundedReceiver<RunCommand>,
    start: TaskStart,
) {
    let mut process = match start {
        TaskStart::Live(process) => Some(*process),
        TaskStart::Parked => None,
        TaskStart::SignIn { input } => {
            if !sign_in(&inner, &run, &mut inbox).await {
                return;
            }
            match inner.launch(&run, &input).await {
                Ok(Some(process)) => Some(process),
                Ok(None) => return,
                Err(failure) => {
                    fail_after_sign_in(&inner, &run, failure).await;
                    return;
                }
            }
        }
    };
    let limit = limit_of(&run);
    let mut carried = Turn::default();
    loop {
        if let Some(live) = process.take() {
            let span = stretch_span(&inner, &run);
            let stretch = drive(&inner, &run, &mut inbox, live, &mut carried, limit)
                .instrument(span.clone())
                .await;
            match stretch {
                Stretch::Done => {
                    span.record("reason", "ended");
                    return;
                }
                Stretch::Parked => {
                    span.record("reason", "paused");
                }
            }
        }
        match parked(&inner, &run, &mut inbox).await {
            Some(next) => {
                process = Some(next);
                carried.holding = None;
                carried.idle_until = None;
            }
            None => return,
        }
    }
}

/// Agent profile only (SA-XC-05): one local `tracing` span per stretch of a run, never exported.
fn stretch_span(inner: &Inner, run: &Run) -> tracing::Span {
    if inner.agent_profile() {
        tracing::info_span!(
            target: "ordin_claude_adapter",
            "claude.session",
            run_id = %run.run_id,
            reason = tracing::field::Empty
        )
    } else {
        tracing::Span::none()
    }
}

/// `limit_seconds` lowers the wall-clock limit (`start` step 6). Without it the Session Supervisor
/// alone applies the run's limits.
fn limit_of(run: &Run) -> Option<Instant> {
    let seconds = lock(&run.plan).limit_seconds?;
    Some(Instant::now() + std::time::Duration::from_secs(u64::from(seconds)))
}

/// Steps 6–10 name `Err` results, but after a sign-in no caller is waiting for them, so the failure
/// is logged with its code and the run ends `Failed`.
async fn fail_after_sign_in(inner: &Inner, run: &Run, failure: Failure) {
    run_log!(warn, run, "claude.run.ended", code = failure.code);
    run.emit_end(RunEndReason::Failed, None);
    inner.close_session(run).await;
}

/// Ends a run that has no process.
async fn end_parked(inner: &Inner, run: &Run, reason: RunEndReason, code: Option<&'static str>) {
    run.emit_end(reason, None);
    inner.close_session(run).await;
    run_log!(
        info,
        run,
        "claude.run.ended",
        reason = ordin_types::log::wire_word(&reason).as_str(),
        code = code
    );
}

/// Waits, with no process, for a decision or a stop. `Some` is the restarted process.
async fn parked(
    inner: &Inner,
    run: &Arc<Run>,
    inbox: &mut mpsc::UnboundedReceiver<RunCommand>,
) -> Option<Process> {
    loop {
        match inbox.recv().await? {
            RunCommand::Stop { reason, ack } => {
                end_parked(inner, run, reason, None).await;
                let _ = ack.send(());
                return None;
            }
            RunCommand::RestartSignIn => {}
            RunCommand::Resume { turn, ack } => {
                if run.paused_on().is_none() {
                    let _ = ack.send(Err(not_paused()));
                    continue;
                }
                match inner.open_process(run).await {
                    Ok(mut process) => {
                        run.emit(RunEvent::Resumed);
                        run_log!(info, run, "claude.run.resumed");
                        process.write_turn(&turn).await;
                        let _ = ack.send(Ok(()));
                        return Some(process);
                    }
                    Err(OpenStop::Ended) => {
                        let _ = ack.send(Ok(()));
                        return None;
                    }
                    Err(OpenStop::Failed(failure)) => {
                        inner.close_session(run).await;
                        let _ = ack.send(Err(failure));
                    }
                }
            }
        }
    }
}

fn not_paused() -> Failure {
    Failure::invalid("CLA_NOT_PAUSED", "RA_RESUME_MISMATCH")
}

/// The run with a live process, until the process's stretch ends. A conversation run ends only by
/// the paths that end any run, never at a turn's end.
// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-CONV-5
async fn drive(
    inner: &Inner,
    run: &Arc<Run>,
    inbox: &mut mpsc::UnboundedReceiver<RunCommand>,
    mut process: Process,
    turn: &mut Turn,
    limit: Option<Instant>,
) -> Stretch {
    loop {
        tokio::select! {
            biased;
            command = inbox.recv() => {
                match command {
                    Some(RunCommand::Stop { reason, ack }) => {
                        finish_with_stop(inner, run, &mut process, reason, None, None).await;
                        let _ = ack.send(());
                        return Stretch::Done;
                    }
                    Some(RunCommand::Resume { turn: text, ack }) => {
                        if run.paused_on().is_some() {
                            run.emit(RunEvent::Resumed);
                            run_log!(info, run, "claude.run.resumed");
                            turn.idle_until = None;
                            process.write_turn(&text).await;
                            let _ = ack.send(Ok(()));
                        } else {
                            let _ = ack.send(Err(not_paused()));
                        }
                    }
                    Some(RunCommand::RestartSignIn) => {}
                    None => {
                        finish_with_stop(
                            inner, run, &mut process, RunEndReason::Interrupted, None, None,
                        )
                        .await;
                        return Stretch::Done;
                    }
                }
            }
            event = next_hook_event(&mut process.hook_events) => {
                match event {
                    Some(event) => {
                        run.touch();
                        on_hook_event(inner, run, turn, event);
                    }
                    None => process.hook_events = None,
                }
            }
            line = process.lines.recv() => {
                run.touch();
                match line {
                    Some(Line::Text(bytes)) => {
                        if let Some(stretch) =
                            on_line(inner, run, &mut process, turn, &bytes).await
                        {
                            return stretch;
                        }
                    }
                    Some(Line::TooLong) => {
                        run_log!(warn, run, "claude.stream.too_long", code = "CLA_STREAM_LINE_TOO_LONG");
                    }
                    None => {
                        on_exit(inner, run, &mut process).await;
                        return Stretch::Done;
                    }
                }
            }
            () = wait_until(turn.idle_until) => {
                // 2D-4: the idle harness process goes; the record stays where Claude Code wrote it.
                turn.idle_until = None;
                process.close_and_wait(run, inner.config.stop_deadline).await;
                inner.close_session(run).await;
                return Stretch::Parked;
            }
            () = wait_until(limit) => {
                finish_with_stop(
                    inner, run, &mut process, RunEndReason::StoppedAtLimit, None, None,
                )
                .await;
                return Stretch::Done;
            }
        }
    }
}

async fn next_hook_event(
    events: &mut Option<BoxStream<'static, HookSessionEvent>>,
) -> Option<HookSessionEvent> {
    match events.as_mut() {
        Some(events) => events.next().await,
        None => std::future::pending().await,
    }
}

async fn wait_until(moment: Option<Instant>) {
    match moment {
        Some(moment) => sleep_until(moment).await,
        None => std::future::pending().await,
    }
}

/// *Hook Relay session events → run events*.
// spec: ordin-desktop/components/ClaudeLocalAdapter#ADR-045
fn on_hook_event(inner: &Inner, run: &Run, turn: &mut Turn, event: HookSessionEvent) {
    match event {
        HookSessionEvent::Holding { approval_id, .. } => {
            turn.holding = Some(approval_id);
            run.emit_progress(WAITING_FOR_APPROVAL.to_owned(), None);
        }
        HookSessionEvent::DeniedAtDeadline { approval_id, .. } => {
            turn.holding = None;
            if turn.paused.insert(approval_id) {
                run.emit(RunEvent::Paused {
                    reason: PauseReason::Approval { approval_id },
                });
                run_log!(
                    info,
                    run,
                    "claude.run.paused",
                    reason_kind = "approval",
                    approval_id = approval_id.to_string().as_str()
                );
                if !inner.agent_profile() {
                    turn.idle_until = Some(Instant::now() + inner.host.timings.idle_process);
                }
            }
        }
        HookSessionEvent::Decided { .. } => turn.holding = None,
        HookSessionEvent::ToolUsed {
            tool,
            origin,
            succeeded,
        } => {
            turn.tool_calls = turn.tool_calls.saturating_add(1);
            turn.total_tool_calls += 1;
            if let Some(line) = plain_line(&tool, &origin, succeeded) {
                run.emit_progress(line, Some(turn.total_tool_calls));
            }
        }
        // The Rules Engine's run-end decision is still asked at each turn's `Stop`; an allowed
        // ending ends the turn, and the conversation run goes on.
        // spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-CONV-6
        HookSessionEvent::TurnStopping => {
            run_log!(info, run, "claude.turn.stopping");
        }
    }
}

/// One stdout line. `Some` when the process's stretch is over.
async fn on_line(
    inner: &Inner,
    run: &Arc<Run>,
    process: &mut Process,
    turn: &mut Turn,
    bytes: &[u8],
) -> Option<Stretch> {
    let item = match parse_line(bytes) {
        Ok(item) => item,
        Err(Malformed) => {
            turn.malformed += 1;
            run_log!(
                warn,
                run,
                "claude.stream.malformed",
                code = "CLA_STREAM_MALFORMED",
                line_len = bytes.len()
            );
            if turn.malformed >= MAX_MALFORMED {
                process.keep_stderr_tail(run);
                finish_with_stop(
                    inner,
                    run,
                    process,
                    RunEndReason::Failed,
                    None,
                    Some("CLA_STREAM_MALFORMED"),
                )
                .await;
                return Some(Stretch::Done);
            }
            return None;
        }
    };
    turn.malformed = 0;
    match item {
        StreamItem::Init => {
            if !process.init_seen {
                process.init_seen = true;
                run_log!(info, run, "claude.session.init");
            }
        }
        StreamItem::Activity { plan } => {
            if let Some(steps) = plan {
                run.emit(RunEvent::HarnessPlan { steps });
            }
        }
        StreamItem::RateLimit { allowance_used_up } => {
            turn.allowance_used_up = allowance_used_up;
        }
        StreamItem::Result(result) => return on_result(inner, run, process, turn, result).await,
        StreamItem::Other => {
            tracing::debug!(target: "ordin_claude_adapter", run_id = %run.run_id, "claude.stream.ignored");
        }
    }
    None
}

/// The end of a turn.
async fn on_result(
    inner: &Inner,
    run: &Arc<Run>,
    process: &mut Process,
    turn: &mut Turn,
    result: TurnResult,
) -> Option<Stretch> {
    let tool_calls = std::mem::take(&mut turn.tool_calls);
    run_log!(
        info,
        run,
        "claude.turn.result",
        is_error = result.is_error,
        model_turns = 1u32,
        tool_calls = tool_calls
    );
    run.emit(RunEvent::Usage {
        usage: UsageDelta {
            model_turns: 1,
            tool_calls,
            input_tokens: result.input_tokens,
            output_tokens: result.output_tokens,
            cached_input_tokens: result.cached_input_tokens,
        },
    });
    let allowance_used_up = std::mem::take(&mut turn.allowance_used_up);
    let paused = run.paused_on().is_some();
    let outstanding = paused || turn.holding.is_some();
    if result.is_error {
        process.keep_stderr_tail(run);
        let (message, code) = if allowance_used_up {
            let message = result
                .result
                .as_deref()
                .map(|m| cut(m, MAX_SERVICE_MESSAGE));
            (message, "CLA_ALLOWANCE_EXHAUSTED")
        } else {
            (None, "CLA_TURN_FAILED")
        };
        finish_with_stop(
            inner,
            run,
            process,
            RunEndReason::Failed,
            message,
            Some(code),
        )
        .await;
        return Some(Stretch::Done);
    }
    let single_call = lock(&run.plan).single_call;
    if !outstanding && run.is_conversation() {
        end_conversation_turn(inner, run, turn, &result);
        return None;
    }
    if !outstanding || single_call {
        let message = result.result.as_deref().map(|m| cut(m, MAX_FINAL_MESSAGE));
        finish_with_stop(inner, run, process, RunEndReason::Completed, message, None).await;
        return Some(Stretch::Done);
    }
    if paused && inner.agent_profile() {
        return Some(save_and_park(inner, run, process).await);
    }
    None
}

/// A conversation's turn has ended: the run waits for the person's next message with its process
/// kept, until the idle harness process goes (2D-4).
// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-CONV-2
// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-CONV-4
fn end_conversation_turn(inner: &Inner, run: &Run, turn: &mut Turn, result: &TurnResult) {
    let question = result
        .result
        .as_deref()
        .map(|text| cut(text, MAX_TURN_TEXT))
        .unwrap_or_default();
    run.emit(RunEvent::Paused {
        reason: PauseReason::Person { question },
    });
    run_log!(info, run, "claude.run.paused", reason_kind = "person");
    if !inner.agent_profile() {
        turn.idle_until = Some(Instant::now() + inner.host.timings.idle_process);
    }
}

/// *Pausing in the agent profile*: the turn has ended, so the process goes and its record is stored.
// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA_SESSION_RECORD_SAVE_FAILED
async fn save_and_park(inner: &Inner, run: &Arc<Run>, process: &mut Process) -> Stretch {
    process
        .close_and_wait(run, inner.config.stop_deadline)
        .await;
    let record = inner
        .session_record_path(run)
        .and_then(|path| std::fs::read(path).ok());
    let saved = match (record, &inner.records) {
        (Some(record), Some(records)) => records
            .save(run.run_id, &run.runtime_session_id, record)
            .await
            .map_err(|error| {
                run_log!(
                    warn,
                    run,
                    "claude.session.record_unavailable",
                    record_code = error.code
                );
            })
            .is_ok(),
        _ => false,
    };
    if !saved {
        end_parked(
            inner,
            run,
            RunEndReason::Interrupted,
            Some("CLA_SESSION_RECORD_SAVE_FAILED"),
        )
        .await;
        return Stretch::Done;
    }
    inner.close_session(run).await;
    Stretch::Parked
}

/// `Ended`, then the process is stopped and the session removed.
async fn finish_with_stop(
    inner: &Inner,
    run: &Run,
    process: &mut Process,
    reason: RunEndReason,
    message: Option<String>,
    code: Option<&'static str>,
) {
    run.emit_end(reason, message);
    let status = process.stop(run, inner.config.stop_deadline).await;
    inner.close_session(run).await;
    run_log!(
        info,
        run,
        "claude.run.ended",
        reason = ordin_types::log::wire_word(&reason).as_str(),
        exit_status = status.and_then(|s| s.code()),
        code = code
    );
}

/// *CLI exits unexpectedly*: stdout closed before the run's `Ended`.
// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA_HARNESS_EXITED
async fn on_exit(inner: &Inner, run: &Run, process: &mut Process) {
    let status = process
        .close_and_wait(run, inner.config.stop_deadline)
        .await;
    process.keep_stderr_tail(run);
    run.emit_end(RunEndReason::Failed, None);
    inner.close_session(run).await;
    run_log!(
        warn,
        run,
        "claude.run.ended",
        reason = ordin_types::log::wire_word(&RunEndReason::Failed).as_str(),
        exit_status = status.and_then(|s| s.code()),
        code = "CLA_HARNESS_EXITED"
    );
}

/// Starts `claude auth login` with no standard input and no terminal: the child has its own process
/// group, and its output goes nowhere.
// spec: ordin-desktop/components/ClaudeLocalAdapter#C-25
fn spawn_login(inner: &Inner) -> Option<Child> {
    let env = child_environment(
        &inner.host.parent_env,
        &inner.config.sign_in,
        None,
        &inner.config.claude_path,
    );
    let mut command = claude_command(&inner.config.claude_path, &env);
    command
        .args(["auth", "login"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    command.process_group(0);
    command.spawn().ok()
}

/// *Sign-in flow*. True when the harness is signed in; false when the run has ended.
async fn sign_in(
    inner: &Inner,
    run: &Run,
    inbox: &mut mpsc::UnboundedReceiver<RunCommand>,
) -> bool {
    let timings = inner.host.timings;
    let env = child_environment(
        &inner.host.parent_env,
        &inner.config.sign_in,
        None,
        &inner.config.claude_path,
    );
    loop {
        // One attempt: from step 1 until signed in, failed or timed out.
        run_log!(info, run, "claude.sign_in.started");
        let mut login = spawn_login(inner);
        if login.is_none() {
            run_log!(
                warn,
                run,
                "claude.sign_in.failed",
                code = "CLA_SIGN_IN_FAILED"
            );
        }
        let give_up = Instant::now() + timings.sign_in_give_up;
        let mut attempting = login.is_some();
        loop {
            tokio::select! {
                biased;
                command = inbox.recv() => match command {
                    Some(RunCommand::RestartSignIn) => {
                        drop(login.take());
                        break;
                    }
                    Some(RunCommand::Stop { reason, ack }) => {
                        drop(login.take());
                        end_parked(inner, run, reason, None).await;
                        let _ = ack.send(());
                        return false;
                    }
                    Some(RunCommand::Resume { ack, .. }) => {
                        let _ = ack.send(Err(not_paused()));
                    }
                    None => return false,
                },
                done = wait_login(&mut login), if login.is_some() => {
                    login = None;
                    if !done {
                        run_log!(warn, run, "claude.sign_in.failed", code = "CLA_SIGN_IN_FAILED");
                        attempting = false;
                    }
                }
                () = sleep(timings.sign_in_poll), if attempting => {
                    if let AuthCheck::Read(status) =
                        check_auth(&inner.config.claude_path, &env, timings.cli_check).await
                    {
                        if status.logged_in {
                            drop(login.take());
                            run.emit(RunEvent::Resumed);
                            run_log!(info, run, "claude.run.resumed");
                            return true;
                        }
                    }
                }
                () = sleep_until(give_up), if attempting => {
                    drop(login.take());
                    attempting = false;
                }
            }
        }
    }
}

/// True when the login process ended with status 0.
async fn wait_login(login: &mut Option<Child>) -> bool {
    match login.as_mut() {
        Some(child) => child.wait().await.is_ok_and(|status| status.success()),
        None => std::future::pending().await,
    }
}
