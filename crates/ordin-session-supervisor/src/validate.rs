//! The 2C validation rules this module applies: Task (A.1), Run (before a run is recorded), and the
//! start limits (B.2).

use ordin_runtime_adapter::{AdapterKind, RunLimits};
use ordin_types::common::AgentRef;
use ordin_types::domain::runs::{Run, RunState, Task};

/// Longest task command, in characters (2C Task).
const MAX_TASK_COMMAND: usize = 100;
/// Longest task free text, in characters (2C Task).
const MAX_TASK_TEXT: usize = 8_000;
/// Most task arguments (2C Task).
const MAX_TASK_ARGUMENTS: usize = 50;
/// Longest argument name, in characters (2C Task).
const MAX_ARGUMENT_NAME: usize = 64;
/// Longest argument value, in characters (2C Task).
const MAX_ARGUMENT_VALUE: usize = 4_000;
/// Run wall clock: 2C range, and the application maximum of 8 hours (`limits.run.wall_clock_minutes`
/// maximum 480).
pub(crate) const WALL_CLOCK_SECS: std::ops::RangeInclusive<u32> = 1..=28_800;
/// Run idle timeout: 2C range.
pub(crate) const IDLE_TIMEOUT_SECS: std::ops::RangeInclusive<u32> = 30..=3_600;
/// Longest `end_reason` (2C Run).
pub(crate) const MAX_END_REASON: usize = 500;
/// Most inputs used by one run (2C Run).
const MAX_INPUTS: usize = 100;
/// Longest command kept in `runs.command`.
pub(crate) const MAX_RUN_COMMAND: usize = 64;

fn chars_within(text: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&text.chars().count())
}

/// A.1: the 2C Task rules, in the order the spec lists them.
pub(crate) fn check_task(task: &Task) -> Result<(), &'static str> {
    match (&task.command, &task.free_text) {
        (Some(command), None) => {
            if !chars_within(command, 1, MAX_TASK_COMMAND) {
                return Err("TASK_COMMAND_INVALID");
            }
        }
        (None, Some(text)) => {
            if !chars_within(text, 1, MAX_TASK_TEXT) {
                return Err("TASK_TEXT_INVALID");
            }
        }
        _ => return Err("TASK_COMMAND_XOR_TEXT"),
    }
    if task.arguments.len() > MAX_TASK_ARGUMENTS
        || task.arguments.iter().any(|a| {
            !chars_within(&a.name, 1, MAX_ARGUMENT_NAME)
                || !chars_within(&a.value, 0, MAX_ARGUMENT_VALUE)
        })
    {
        return Err("TASK_ARGUMENTS_INVALID");
    }
    Ok(())
}

/// B.2: the 2C ranges and the profile ceiling.
pub(crate) fn check_limits(limits: &RunLimits) -> Result<(), &'static str> {
    if !WALL_CLOCK_SECS.contains(&limits.wall_clock_secs)
        || !IDLE_TIMEOUT_SECS.contains(&limits.idle_timeout_secs)
        || limits.request_budget == Some(0)
    {
        return Err("RA_LIMIT_OUT_OF_RANGE");
    }
    Ok(())
}

pub(crate) fn is_local(kind: AdapterKind) -> bool {
    matches!(kind, AdapterKind::ClaudeLocal | AdapterKind::CopilotLocal)
}

pub(crate) fn is_ended(state: RunState) -> bool {
    !matches!(
        state,
        RunState::Queued | RunState::Running | RunState::Paused
    )
}

/// The 2C Run rules (ordin's `RUNST_` codes), checked before a run is recorded.
pub(crate) fn check_run(run: &Run) -> Result<(), &'static str> {
    let local_agent = matches!(run.agent, AgentRef::AgentInstance { .. });
    if local_agent != is_local(run.adapter) {
        return Err("RUNST_ADAPTER_MISMATCH");
    }
    if (run.state == RunState::Paused) != run.pause_reason.is_some() {
        return Err("RUNST_PAUSE_REASON_MISMATCH");
    }
    if run.state == RunState::NoReply && run.adapter != AdapterKind::SlackThread {
        return Err("RUNST_NO_REPLY_NOT_SLACK");
    }
    if run.state == RunState::Abandoned && is_local(run.adapter) {
        return Err("RUNST_ABANDONED_NOT_REMOTE");
    }
    if is_ended(run.state) != run.ended_at.is_some() {
        return Err("RUNST_ENDED_AT_MISMATCH");
    }
    if run
        .end_reason
        .as_ref()
        .is_some_and(|r| r.chars().count() > MAX_END_REASON)
    {
        return Err("RUNST_END_REASON_TOO_LONG");
    }
    if !WALL_CLOCK_SECS.contains(&run.limits.wall_clock_secs)
        || !IDLE_TIMEOUT_SECS.contains(&run.limits.idle_timeout_secs)
        || run.limits.request_budget == Some(0)
    {
        return Err("RUNST_LIMIT_OUT_OF_RANGE");
    }
    if let (Some(cache), Some(tokens)) = (run.usage.tokens_from_cache, run.usage.tokens) {
        if cache > tokens {
            return Err("RUNST_CACHE_EXCEEDS_TOKENS");
        }
    }
    if run.inputs_used.len() > MAX_INPUTS {
        return Err("RUNST_INPUTS_TOO_MANY");
    }
    Ok(())
}

/// Cuts `text` to at most `max` characters.
pub(crate) fn cut(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((at, _)) => text[..at].to_owned(),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordin_types::domain::runs::{
        RunOrigin, RunPauseReason, TaskArgument, TaskTarget, UsageCounters,
    };
    use ordin_types::time::Timestamp;
    use ordin_types::{AgentInstanceId, RemoteAgentId, RunId, TaskId, WorkspaceId};

    fn task() -> Task {
        Task {
            id: TaskId::new(),
            workspace_id: WorkspaceId::new(),
            target: TaskTarget::AgentInstance {
                agent_instance_id: AgentInstanceId::new(),
            },
            command: Some("compare-quotes".into()),
            free_text: None,
            arguments: vec![],
            attached_artefacts: vec![],
            run_again_of: None,
            requested_at: Timestamp::now(),
        }
    }

    fn run() -> Run {
        Run {
            id: RunId::new(),
            origin: RunOrigin::Task {
                task_id: TaskId::new(),
            },
            agent: AgentRef::AgentInstance {
                agent_instance_id: AgentInstanceId::new(),
            },
            adapter: AdapterKind::ClaudeLocal,
            runtime_session_id: None,
            remote_run_id: None,
            remote_thread_id: None,
            state: RunState::Queued,
            pause_reason: None,
            limits: RunLimits {
                wall_clock_secs: 3600,
                request_budget: None,
                idle_timeout_secs: 600,
            },
            usage: UsageCounters {
                model_turns: 0,
                tool_calls: 0,
                model_level: None,
                tokens: None,
                tokens_from_cache: None,
            },
            started_at: None,
            ended_at: None,
            end_reason: None,
            inputs_used: vec![],
            queued_at: Timestamp::now(),
        }
    }

    #[test]
    fn task_rules_give_their_codes() {
        assert_eq!(check_task(&task()), Ok(()));
        let mut t = task();
        t.free_text = Some("x".into());
        assert_eq!(check_task(&t), Err("TASK_COMMAND_XOR_TEXT"));
        t.command = None;
        t.free_text = None;
        assert_eq!(check_task(&t), Err("TASK_COMMAND_XOR_TEXT"));
        t.command = Some("c".repeat(101));
        assert_eq!(check_task(&t), Err("TASK_COMMAND_INVALID"));
        t.command = None;
        t.free_text = Some(String::new());
        assert_eq!(check_task(&t), Err("TASK_TEXT_INVALID"));
        t.free_text = Some("y".repeat(8_001));
        assert_eq!(check_task(&t), Err("TASK_TEXT_INVALID"));
        let mut t = task();
        t.arguments = vec![
            TaskArgument {
                name: "a".into(),
                value: "b".into()
            };
            51
        ];
        assert_eq!(check_task(&t), Err("TASK_ARGUMENTS_INVALID"));
        t.arguments = vec![TaskArgument {
            name: String::new(),
            value: "b".into(),
        }];
        assert_eq!(check_task(&t), Err("TASK_ARGUMENTS_INVALID"));
        t.arguments = vec![TaskArgument {
            name: "n".into(),
            value: "v".repeat(4_001),
        }];
        assert_eq!(check_task(&t), Err("TASK_ARGUMENTS_INVALID"));
    }

    #[test]
    fn limits_outside_their_ranges_are_refused() {
        let ok = RunLimits {
            wall_clock_secs: 60,
            request_budget: Some(1),
            idle_timeout_secs: 30,
        };
        assert_eq!(check_limits(&ok), Ok(()));
        for bad in [
            RunLimits {
                wall_clock_secs: 0,
                ..ok
            },
            RunLimits {
                wall_clock_secs: 28_801,
                ..ok
            },
            RunLimits {
                idle_timeout_secs: 29,
                ..ok
            },
            RunLimits {
                idle_timeout_secs: 3_601,
                ..ok
            },
            RunLimits {
                request_budget: Some(0),
                ..ok
            },
        ] {
            assert_eq!(check_limits(&bad), Err("RA_LIMIT_OUT_OF_RANGE"));
        }
    }

    #[test]
    fn run_rules_give_their_codes() {
        assert_eq!(check_run(&run()), Ok(()));
        let mut r = run();
        r.adapter = AdapterKind::HostedAgent;
        assert_eq!(check_run(&r), Err("RUNST_ADAPTER_MISMATCH"));
        let mut r = run();
        r.state = RunState::Paused;
        assert_eq!(check_run(&r), Err("RUNST_PAUSE_REASON_MISMATCH"));
        r.pause_reason = Some(RunPauseReason::Person);
        assert_eq!(check_run(&r), Ok(()));
        let mut r = run();
        r.state = RunState::NoReply;
        r.ended_at = Some(Timestamp::now());
        assert_eq!(check_run(&r), Err("RUNST_NO_REPLY_NOT_SLACK"));
        r.state = RunState::Abandoned;
        assert_eq!(check_run(&r), Err("RUNST_ABANDONED_NOT_REMOTE"));
        let mut r = run();
        r.state = RunState::Completed;
        assert_eq!(check_run(&r), Err("RUNST_ENDED_AT_MISMATCH"));
        let mut r = run();
        r.end_reason = Some("e".repeat(501));
        assert_eq!(check_run(&r), Err("RUNST_END_REASON_TOO_LONG"));
        let mut r = run();
        r.limits.idle_timeout_secs = 1;
        assert_eq!(check_run(&r), Err("RUNST_LIMIT_OUT_OF_RANGE"));
        let mut r = run();
        r.usage.tokens = Some(1);
        r.usage.tokens_from_cache = Some(2);
        assert_eq!(check_run(&r), Err("RUNST_CACHE_EXCEEDS_TOKENS"));
        let mut r = run();
        r.inputs_used = vec![
            ordin_types::common::InputUsed {
                name: "n".into(),
                location: "l".into(),
                version_read: "v".into()
            };
            101
        ];
        assert_eq!(check_run(&r), Err("RUNST_INPUTS_TOO_MANY"));
        let mut r = run();
        r.agent = AgentRef::RemoteAgent {
            remote_agent_id: RemoteAgentId::new(),
        };
        r.adapter = AdapterKind::SlackThread;
        r.state = RunState::NoReply;
        r.ended_at = Some(Timestamp::now());
        assert_eq!(check_run(&r), Ok(()));
    }

    #[test]
    fn cut_keeps_whole_characters() {
        assert_eq!(cut("héllo", 2), "hé");
        assert_eq!(cut("hi", 5), "hi");
    }
}
