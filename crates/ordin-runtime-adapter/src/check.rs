//! The pure checks the Session Supervisor runs around every adapter call (Internal Logic 1, 3–6, 8;
//! the 2C wire §2 validation rows LW-9 to LW-16).

use ordin_types::text::{check_len, normalise_text};

use crate::adapter::{
    PauseReason, ResumeDecision, RunHandle, RunSnapshot, RuntimeAdapter, StartInput, StartRun,
};
use crate::error::{AdapterError, CheckError};
use crate::{AdapterKind, AdapterVerb, RunEndReason, RunState, RunTarget};

/// Longest `Command.name`, in scalar values.
const MAX_COMMAND_NAME: usize = 64;
/// Longest `Command.arguments` and `FreeText.text`, in scalar values.
const MAX_INPUT: usize = 16_384;
/// Most attachments on one start request.
const MAX_ATTACHMENTS: usize = 50;
/// Allowed `limits.wall_clock_secs`.
const WALL_CLOCK_SECS: std::ops::RangeInclusive<u32> = 1..=28_800;
/// Allowed `limits.idle_timeout_secs`.
const IDLE_TIMEOUT_SECS: std::ops::RangeInclusive<u32> = 30..=3_600;
/// Longest thread, session or remote identifier, in scalar values.
const MAX_HANDLE: usize = 256;
/// Longest model name, in characters (the bound matches LW-17's tool-name bound).
const MAX_MODEL_NAME: usize = 128;
/// Longest `Reject.reason`, in scalar values.
const MAX_REJECT_REASON: usize = 500;
/// Longest `Answer.text`, in scalar values.
const MAX_ANSWER: usize = 4_000;

/// The local adapters, which take `agent_instance` targets.
pub(crate) fn is_local(kind: AdapterKind) -> bool {
    matches!(kind, AdapterKind::ClaudeLocal | AdapterKind::CopilotLocal)
}

/// NFC-normalises `raw`, then counts scalar values against `min..=max`. A control character is
/// `TEXT_CONTROL_CHAR`; a count out of range is `code`.
fn text_len(
    raw: &str,
    min: usize,
    max: usize,
    code: &'static str,
    field: &'static str,
) -> Result<String, CheckError> {
    let text = normalise_text(raw).map_err(|e| CheckError::new(e.code(), field))?;
    check_len(&text, min, max, code).map_err(|_| CheckError::new(code, field))?;
    Ok(text)
}

/// Counts scalar values of an identifier against `1..=max`.
fn id_len(
    raw: &str,
    max: usize,
    code: &'static str,
    field: &'static str,
) -> Result<(), CheckError> {
    check_len(raw, 1, max, code).map_err(|_| CheckError::new(code, field))
}

/// `^[a-z0-9][a-z0-9_-]{0,63}$` (*assumed here* in the spec: task names are kebab-case file stems).
fn is_command_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    name.len() <= MAX_COMMAND_NAME
        && (first.is_ascii_lowercase() || first.is_ascii_digit())
        && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// 1–128 characters of `[A-Za-z0-9._:-]`.
fn is_model_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_MODEL_NAME
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

/// Checks a start request against the limits above and the target rule for `kind`.
/// Does not check that a command name exists or that attachments are granted: the Session
/// Supervisor checks those against the agent instance and the folder grants before calling.
///
/// The grant context (`caller`, `scope_id`, `coding_task_id`) is not checked here: the program
/// knows the profile, and the MCP Front refuses an unknown coding task when it builds the grant
/// (Internal Logic 1.7). No logging: the caller journals the refusal with the run.
pub fn check_start(kind: AdapterKind, req: &StartRun) -> Result<(), CheckError> {
    // 1. Target rule.
    let target_fits = match req.target {
        RunTarget::AgentInstance { .. } => is_local(kind),
        RunTarget::RemoteAgent { .. } => !is_local(kind),
    };
    if !target_fits {
        return Err(CheckError::new("RA_TARGET_MISMATCH", "target"));
    }

    // 2. Input.
    match &req.input {
        StartInput::Command { name, arguments } => {
            let name = text_len(
                name,
                1,
                MAX_COMMAND_NAME,
                "RA_COMMAND_UNKNOWN",
                "input.name",
            )?;
            if !is_command_name(&name) {
                return Err(CheckError::new("RA_COMMAND_UNKNOWN", "input.name"));
            }
            text_len(
                arguments,
                0,
                MAX_INPUT,
                "RA_INPUT_TOO_LARGE",
                "input.arguments",
            )?;
        }
        StartInput::FreeText { text } => {
            text_len(text, 1, MAX_INPUT, "RA_INPUT_TOO_LARGE", "input.text")?;
        }
    }

    // 3. Attachment count (*assumed here* in the spec: 2C names no code for the count).
    if req.attachments.len() > MAX_ATTACHMENTS {
        return Err(CheckError::new("RA_INPUT_TOO_LARGE", "attachments"));
    }

    // 4. Limits.
    let limits = &req.limits;
    if !WALL_CLOCK_SECS.contains(&limits.wall_clock_secs)
        || !IDLE_TIMEOUT_SECS.contains(&limits.idle_timeout_secs)
        || limits.request_budget == Some(0)
    {
        return Err(CheckError::new("RA_LIMIT_OUT_OF_RANGE", "limits"));
    }

    // 5. Thread: only for the hosted agent, 1–256 characters.
    if let Some(thread) = &req.thread {
        if kind != AdapterKind::HostedAgent {
            return Err(CheckError::new("RA_HANDLE_INVALID", "thread"));
        }
        id_len(thread, MAX_HANDLE, "RA_HANDLE_INVALID", "thread")?;
    }

    // 6. Model: a well-formed name, and none for a remote adapter. `thinking` is not checked.
    if let Some(model) = &req.model {
        if let Some(name) = &model.model {
            if !is_model_name(name) {
                return Err(CheckError::new("RA_MODEL_INVALID", "model.model"));
            }
        }
        if !is_local(kind) {
            return Err(CheckError::new("RA_MODEL_UNKNOWN", "model"));
        }
    }

    Ok(())
}

/// Refuses a verb `adapter` declares unsupported, without contacting the route.
pub fn check_verb(adapter: &dyn RuntimeAdapter, verb: AdapterVerb) -> Result<(), AdapterError> {
    if adapter.unsupported_verbs().contains(&verb) {
        return Err(AdapterError::VerbNotOffered { verb });
    }
    Ok(())
}

/// Checks a run handle's identifiers once `started` has been seen.
pub fn check_handle(handle: &RunHandle) -> Result<(), CheckError> {
    if let Some(id) = &handle.runtime_session_id {
        id_len(id, MAX_HANDLE, "RA_HANDLE_INVALID", "runtime_session_id")?;
    }
    match &handle.remote_thread_id {
        Some(id) => id_len(id, MAX_HANDLE, "RA_HANDLE_INVALID", "remote_thread_id")?,
        None if !is_local(handle.adapter) => {
            return Err(CheckError::new("RA_HANDLE_INVALID", "remote_thread_id"));
        }
        None => {}
    }
    Ok(())
}

/// Checks a resume decision against the reason the run paused on.
pub fn check_resume(decision: &ResumeDecision, paused_on: &PauseReason) -> Result<(), CheckError> {
    let mismatch = CheckError::new("RA_RESUME_MISMATCH", "decision");
    match (decision, paused_on) {
        (
            ResumeDecision::Approve { approval_id },
            PauseReason::Approval {
                approval_id: paused,
            },
        ) => {
            if approval_id != paused {
                return Err(mismatch);
            }
        }
        (
            ResumeDecision::Reject {
                approval_id,
                reason,
            },
            PauseReason::Approval {
                approval_id: paused,
            },
        ) => {
            if approval_id != paused {
                return Err(mismatch);
            }
            if let Some(reason) = reason {
                check_len(reason, 0, MAX_REJECT_REASON, "RA_INPUT_TOO_LARGE")
                    .map_err(|_| CheckError::new("RA_INPUT_TOO_LARGE", "decision.reason"))?;
            }
        }
        (ResumeDecision::Answer { text }, PauseReason::Person { .. }) => {
            check_len(text, 1, MAX_ANSWER, "RA_INPUT_TOO_LARGE")
                .map_err(|_| CheckError::new("RA_INPUT_TOO_LARGE", "decision.text"))?;
        }
        // Approve or Reject against any other reason, Answer against anything but Person, and
        // every decision against SignIn, DecisionElsewhere or RemoteApprovers (Internal Logic 5.2,
        // 5.3, 5.5).
        _ => return Err(mismatch),
    }
    Ok(())
}

/// True for the states a run ends in.
fn is_ended(state: RunState) -> bool {
    !matches!(
        state,
        RunState::Queued | RunState::Running | RunState::Paused
    )
}

/// Checks that a snapshot's `end` matches its `state`.
pub fn check_snapshot(snapshot: &RunSnapshot) -> Result<(), CheckError> {
    let inconsistent = CheckError::new("RA_SNAPSHOT_INCONSISTENT", "end");
    match &snapshot.end {
        Some(end) if end_state(end.reason) != snapshot.state => Err(inconsistent),
        Some(_) => Ok(()),
        None if is_ended(snapshot.state) => Err(inconsistent),
        None => Ok(()),
    }
}

/// Maps an ended reason to the `RunState` the Session Supervisor stores.
pub fn end_state(reason: RunEndReason) -> RunState {
    match reason {
        RunEndReason::Completed => RunState::Completed,
        RunEndReason::Failed => RunState::Failed,
        RunEndReason::StoppedAtLimit => RunState::StoppedAtLimit,
        RunEndReason::Cancelled => RunState::Cancelled,
        RunEndReason::Interrupted => RunState::Interrupted,
        RunEndReason::Abandoned => RunState::Abandoned,
        RunEndReason::NoReply => RunState::NoReply,
    }
}
