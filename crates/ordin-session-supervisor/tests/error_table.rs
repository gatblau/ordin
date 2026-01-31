//! One test per row of SessionSupervisor.md's Error Table (and the ADR-111 row), each provoking the
//! condition and checking the code and whether it is retryable; plus the code registry check.

mod support;

use std::time::Duration;

use ordin_runtime_adapter::{
    AdapterError, AdapterKind, AdapterVerb, PauseReason, ResumeDecision, RunEndReason, RunEvent,
    RunTarget, UsageDelta,
};
use ordin_session_supervisor::{
    ConversationStart, ProgramKind, ShutdownReason, SupervisorError, ALL_CODES,
};
use ordin_types::domain::agent_profile::{RunEndKind, RunRecordState, RunRequest, RunStarter};
use ordin_types::domain::runs::{RunPauseReason, RunState, TaskArgument, TaskTarget};
use ordin_types::{AgentInstanceId, ApprovalId, CrewCharterId, RemoteAgentId, RunId, WorkspaceId};
use ordin_usage_planner::WorkKind;
use support::*;

fn conversation(seed: String) -> ConversationStart {
    ConversationStart {
        workspace_id: WorkspaceId::new(),
        target: RunTarget::AgentInstance {
            agent_instance_id: AgentInstanceId::new(),
        },
        seed,
        limits: ordin_runtime_adapter::RunLimits {
            wall_clock_secs: 60,
            request_budget: None,
            idle_timeout_secs: 600,
        },
        model_override: None,
    }
}

fn permanent(err: &SupervisorError, code: &str) {
    assert_eq!(err.code(), code);
    assert!(!err.retryable(), "{code} is permanent");
}

#[tokio::test]
async fn task_breaks_a_2c_rule() {
    let h = Harness::desktop().await;
    let mut cases = vec![];
    let mut both = task("compare-quotes");
    both.free_text = Some("text".into());
    cases.push((both, "TASK_COMMAND_XOR_TEXT"));
    cases.push((task(&"c".repeat(101)), "TASK_COMMAND_INVALID"));
    let mut text = task("x");
    text.command = None;
    text.free_text = Some("t".repeat(8_001));
    cases.push((text, "TASK_TEXT_INVALID"));
    let mut args = task("compare-quotes");
    args.arguments = vec![
        TaskArgument {
            name: "n".into(),
            value: "v".into()
        };
        51
    ];
    cases.push((args, "TASK_ARGUMENTS_INVALID"));
    for (t, code) in cases {
        let err = h
            .sup
            .submit_task(t, WorkKind::Documents, false, None)
            .await
            .unwrap_err();
        assert!(matches!(err, SupervisorError::TaskInvalid { .. }));
        permanent(&err, code);
    }
}

#[tokio::test]
async fn a_crew_task_submitted_here() {
    let h = Harness::desktop().await;
    let mut t = task("compare-quotes");
    t.target = TaskTarget::Crew {
        crew_charter_id: CrewCharterId::new(),
    };
    let err = h
        .sup
        .submit_task(t, WorkKind::Documents, false, None)
        .await
        .unwrap_err();
    assert!(matches!(err, SupervisorError::StartInvalid { .. }));
    permanent(&err, "SUP_TARGET_IS_CREW");
}

#[tokio::test]
async fn limits_out_of_range() {
    let h = Harness::desktop().await;
    for bad in [
        limits(0, None, 600),
        limits(28_801, None, 600),
        limits(60, None, 29),
        limits(60, Some(0), 600),
    ] {
        let err = h
            .sup
            .start_run(request(
                RunTarget::AgentInstance {
                    agent_instance_id: AgentInstanceId::new(),
                },
                bad,
            ))
            .await
            .unwrap_err();
        permanent(&err, "RA_LIMIT_OUT_OF_RANGE");
    }
}

#[tokio::test]
async fn target_does_not_match_the_adapter() {
    let h = Harness::desktop().await;
    // An unknown remote agent has no route.
    let err = h
        .sup
        .start_run(request(
            RunTarget::RemoteAgent {
                remote_agent_id: RemoteAgentId::new(),
            },
            None,
        ))
        .await
        .unwrap_err();
    permanent(&err, "RA_TARGET_MISMATCH");
    let mut conv = conversation("seed".into());
    conv.target = RunTarget::RemoteAgent {
        remote_agent_id: RemoteAgentId::new(),
    };
    permanent(
        &h.sup.start_conversation(conv).await.unwrap_err(),
        "RA_TARGET_MISMATCH",
    );
}

#[tokio::test]
async fn no_adapter_registered_for_the_kind() {
    let h = Harness::desktop().await;
    let mut cfg = config(ProgramKind::Desktop, "desktop");
    cfg.local_adapter = AdapterKind::CopilotLocal;
    let node = h.another(cfg);
    let err = node
        .sup
        .start_run(request(
            RunTarget::AgentInstance {
                agent_instance_id: AgentInstanceId::new(),
            },
            None,
        ))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        SupervisorError::NoAdapter(AdapterKind::CopilotLocal)
    ));
    permanent(&err, "SUP_NO_ADAPTER");
}

#[tokio::test]
async fn a_second_adapter_of_one_kind_registered() {
    let h = Harness::desktop().await;
    let again = TestAdapter::new(AdapterKind::ClaudeLocal, &[], h.calls.clone());
    let err = h.sup.register_adapter(again).unwrap_err();
    assert!(matches!(err, SupervisorError::StartInvalid { .. }));
    permanent(&err, "RA_ADAPTER_DUPLICATE");
}

#[tokio::test]
async fn conversation_seed_empty() {
    let h = Harness::desktop().await;
    let err = h
        .sup
        .start_conversation(conversation(String::new()))
        .await
        .unwrap_err();
    permanent(&err, "SUP_SEED_EMPTY");
}

#[tokio::test]
async fn conversation_seed_over_16384_characters() {
    let h = Harness::desktop().await;
    let err = h
        .sup
        .start_conversation(conversation("s".repeat(16_385)))
        .await
        .unwrap_err();
    permanent(&err, "RA_INPUT_TOO_LARGE");
}

#[tokio::test]
async fn a_duty_lock_refuses_a_resume() {
    let h = Harness::desktop().await;
    let run = h.start_local(None).await;
    h.wait_state(run, RunState::Running).await;
    let approval_id = ApprovalId::new();
    h.local.push(
        run,
        RunEvent::Paused {
            reason: PauseReason::Approval { approval_id },
        },
    );
    h.wait_state(run, RunState::Paused).await;
    *h.host.resuming.lock().unwrap() = Some("duty:triage-inbox:lock".into());
    h.sup
        .resume_with_decision(run, ResumeDecision::Approve { approval_id })
        .await
        .unwrap();
    let done = h.wait_state(run, RunState::Completed).await;
    assert_eq!(
        done.end_reason.as_deref(),
        Some("ended by the control duty:triage-inbox:lock")
    );
    assert!(h
        .codes(run)
        .await
        .contains(&"SUP_RESUME_REFUSED".to_owned()));
}

#[tokio::test]
async fn a_duty_lock_refuses_a_resume_agent_profile() {
    let h = Harness::agent(ProgramKind::AgentSingle, "runs-1").await;
    h.journal.subscribe(h.sup.event_sink());
    let record = h
        .enqueue(chat("U1"), message("Close it"), serde_json::json!({}))
        .await;
    h.sup.start_queue_workers();
    Node::wait_starts(&h.local, 1).await;
    let run = h.local.started_runs()[0];
    h.wait_state(run, RunState::Running).await;
    let approval_id = ApprovalId::new();
    h.local.push(
        run,
        RunEvent::Paused {
            reason: PauseReason::Approval { approval_id },
        },
    );
    h.wait_record(&record.run_id, RunRecordState::Paused).await;
    *h.host.resuming.lock().unwrap() = Some("duty:triage-inbox:lock".into());
    h.journal
        .append_event(
            "approval_decided",
            serde_json::json!({ "approval_id": approval_id, "state": "approved", "decided_on": "screen" }),
        )
        .await
        .unwrap();
    let view = h.wait_record(&record.run_id, RunRecordState::Ended).await;
    assert_eq!(view.record.end_kind, Some(RunEndKind::Control));
    assert!(view.outcome.unwrap().ended_by_control);
    assert!(h.local.resumes.lock().unwrap().is_empty());
    assert!(h
        .codes(run)
        .await
        .contains(&"SUP_RESUME_REFUSED".to_owned()));
}

#[tokio::test]
async fn session_record_saved_for_an_unknown_or_ended_run() {
    let h = Harness::desktop().await;
    let err = h
        .sup
        .save_session_record(RunId::new(), "s-1", vec![1])
        .await
        .unwrap_err();
    permanent(&err, "SUP_RUN_NOT_FOUND");
    let run = h.start_local(None).await;
    h.wait_state(run, RunState::Running).await;
    h.local.end(run, RunEndReason::Completed, None);
    h.wait_state(run, RunState::Completed).await;
    Node::wait_until("forgotten", || !h.sup.is_live(run)).await;
    let err = h
        .sup
        .save_session_record(run, "s-1", vec![1])
        .await
        .unwrap_err();
    permanent(&err, "SUP_RUN_NOT_FOUND");
}

#[tokio::test]
async fn harness_signed_out_at_start() {
    let h = Harness::desktop().await;
    *h.local.start_error.lock().unwrap() = Some(AdapterError::SignedOut);
    let run = h.start_local(None).await;
    let paused = h.wait_state(run, RunState::Paused).await;
    assert_eq!(paused.pause_reason, Some(RunPauseReason::SignIn));
    assert_eq!(h.codes(run).await, ["RA_SIGNED_OUT"]);
}

/// The start errors that fail the run: (error, end reason, journalled code).
#[tokio::test]
async fn start_errors_fail_the_run() {
    let cases = [
        (
            AdapterError::AllowanceExhausted {
                service_message: "Your plan's allowance is used up until 18:00.".into(),
            },
            "Your plan's allowance is used up until 18:00.",
            "RA_ALLOWANCE_EXHAUSTED",
        ),
        (
            AdapterError::InvalidRequest {
                code: "RA_INPUT_TOO_LARGE".into(),
            },
            "couldn't start the agent",
            "RA_INPUT_TOO_LARGE",
        ),
        (
            AdapterError::Refused {
                code: "HAA_NO_ACCESS".into(),
            },
            "couldn't start the agent",
            "HAA_NO_ACCESS",
        ),
        (
            AdapterError::Unreachable,
            "couldn't reach the agent",
            "RA_UNREACHABLE",
        ),
        (
            AdapterError::Deadline,
            "couldn't reach the agent",
            "RA_DEADLINE",
        ),
        (
            AdapterError::HarnessUntested {
                found: "9.9".into(),
            },
            "harness version not tested",
            "RA_HARNESS_UNTESTED",
        ),
    ];
    let h = Harness::desktop().await;
    for (error, reason, code) in cases {
        *h.local.start_error.lock().unwrap() = Some(error);
        let run = h.start_local(None).await;
        let failed = h.wait_state(run, RunState::Failed).await;
        assert_eq!(failed.end_reason.as_deref(), Some(reason));
        assert_eq!(h.codes(run).await, [code]);
    }
}

#[tokio::test]
async fn events_out_of_order_or_stream_ends_early() {
    let h = Harness::desktop().await;
    let run = h.start_local(None).await;
    h.wait_state(run, RunState::Running).await;
    // A second `Started` breaks the order.
    h.local.push(run, h.local.started(run));
    let failed = h.wait_state(run, RunState::Failed).await;
    assert_eq!(
        failed.end_reason.as_deref(),
        Some("the session ended unexpectedly")
    );
    assert!(h.codes(run).await.contains(&"RA_EVENT_ORDER".to_owned()));
    // A stream that ends early, with an ended snapshot: the snapshot is applied.
    let run = h.start_local(None).await;
    h.wait_state(run, RunState::Running).await;
    *h.local.read_answer.lock().unwrap() = Some(ended_snapshot(run, RunEndReason::Completed));
    h.local.close(run);
    h.wait_state(run, RunState::Completed).await;
}

#[tokio::test]
async fn wall_clock_budget_or_idle_limit() {
    let h = Harness::desktop().await;
    let run = h.start_local(limits(3_600, Some(2), 600)).await;
    h.wait_state(run, RunState::Running).await;
    h.local.push(
        run,
        RunEvent::Usage {
            usage: UsageDelta {
                model_turns: 2,
                tool_calls: 0,
                input_tokens: None,
                output_tokens: None,
                cached_input_tokens: None,
            },
        },
    );
    let stopped = h.wait_state(run, RunState::StoppedAtLimit).await;
    assert_eq!(stopped.end_reason.as_deref(), Some("request budget"));
    assert_eq!(h.codes(run).await, ["SUP_LIMIT_REACHED"]);
}

#[tokio::test]
async fn cancel_or_resume_of_an_unknown_run() {
    let h = Harness::desktop().await;
    permanent(
        &h.sup.cancel(RunId::new()).await.unwrap_err(),
        "SUP_RUN_NOT_FOUND",
    );
    let err = h
        .sup
        .resume_with_decision(RunId::new(), ResumeDecision::Answer { text: "hi".into() })
        .await
        .unwrap_err();
    permanent(&err, "SUP_RUN_NOT_FOUND");
    permanent(
        &h.sup.subscribe(RunId::new()).unwrap_err(),
        "SUP_RUN_NOT_FOUND",
    );
    permanent(
        &h.sup.get(RunId::new()).await.unwrap_err(),
        "SUP_RUN_NOT_FOUND",
    );
}

#[tokio::test]
async fn cancel_or_resume_of_an_ended_run() {
    let h = Harness::desktop().await;
    let run = h.start_local(None).await;
    h.wait_state(run, RunState::Running).await;
    h.local.end(run, RunEndReason::Completed, None);
    h.wait_state(run, RunState::Completed).await;
    Node::wait_until("forgotten", || !h.sup.is_live(run)).await;
    permanent(&h.sup.cancel(run).await.unwrap_err(), "SUP_RUN_ENDED");
    let err = h
        .sup
        .resume_with_decision(run, ResumeDecision::Answer { text: "hi".into() })
        .await
        .unwrap_err();
    permanent(&err, "SUP_RUN_ENDED");
}

#[tokio::test]
async fn answer_to_a_run_not_paused_for_the_person() {
    let h = Harness::desktop().await;
    let run = h.start_local(None).await;
    h.wait_state(run, RunState::Running).await;
    let err = h
        .sup
        .resume_with_decision(run, ResumeDecision::Answer { text: "hi".into() })
        .await
        .unwrap_err();
    permanent(&err, "SUP_RUN_NOT_PAUSED");
    h.local.push(
        run,
        RunEvent::Paused {
            reason: PauseReason::Approval {
                approval_id: ApprovalId::new(),
            },
        },
    );
    h.wait_state(run, RunState::Paused).await;
    let err = h
        .sup
        .resume_with_decision(run, ResumeDecision::Answer { text: "hi".into() })
        .await
        .unwrap_err();
    permanent(&err, "SUP_RUN_NOT_PAUSED");
}

#[tokio::test]
async fn adapter_lacks_the_verb() {
    let h = Harness::desktop().await;
    let mut cfg = config(ProgramKind::Desktop, "desktop");
    cfg.local_adapter = AdapterKind::CopilotLocal;
    let node = h.another(cfg);
    let copilot = TestAdapter::new(
        AdapterKind::CopilotLocal,
        &[AdapterVerb::Resume],
        node.calls.clone(),
    );
    node.sup.register_adapter(copilot.clone()).unwrap();
    let run = node.start_local(None).await;
    node.wait_state(run, RunState::Running).await;
    copilot.push(
        run,
        RunEvent::Paused {
            reason: PauseReason::Person {
                question: "?".into(),
            },
        },
    );
    node.wait_state(run, RunState::Paused).await;
    let err = node
        .sup
        .resume_with_decision(run, ResumeDecision::Answer { text: "hi".into() })
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        SupervisorError::VerbNotOffered(AdapterVerb::Resume)
    ));
    permanent(&err, "SUP_VERB_NOT_OFFERED");
}

#[tokio::test]
async fn adapter_refuses_a_resume() {
    let h = Harness::desktop().await;
    let run = h.start_local(None).await;
    h.wait_state(run, RunState::Running).await;
    h.local.push(
        run,
        RunEvent::Paused {
            reason: PauseReason::Person {
                question: "?".into(),
            },
        },
    );
    h.wait_state(run, RunState::Paused).await;
    *h.local.resume_error.lock().unwrap() = Some(AdapterError::Unreachable);
    let err = h
        .sup
        .resume_with_decision(run, ResumeDecision::Answer { text: "hi".into() })
        .await
        .unwrap_err();
    assert_eq!(err.code(), "SUP_ADAPTER_REFUSED");
    assert!(err.retryable());
}

#[tokio::test]
async fn lease_lost_agent_profile() {
    let h = Harness::agent(ProgramKind::AgentSingle, "runs-1").await;
    h.enqueue(chat("U1"), message("Hello"), serde_json::json!({}))
        .await;
    h.sup.start_queue_workers();
    Node::wait_starts(&h.local, 1).await;
    let run = h.local.started_runs()[0];
    h.wait_state(run, RunState::Running).await;
    *h.queue.renew_answer.lock().unwrap() = Some(ordin_run_queue::LeaseRenewal::Lost);
    for _ in 0..1_000 {
        if h.sup.get(run).await.unwrap().run.state == RunState::Interrupted {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        h.sup.get(run).await.unwrap().run.state,
        RunState::Interrupted
    );
    assert!(h.codes(run).await.contains(&"SUP_LEASE_LOST".to_owned()));
}

#[tokio::test]
async fn command_expansion_fails_agent_profile() {
    let h = Harness::agent(ProgramKind::AgentSingle, "runs-1").await;
    *h.host.expansion.lock().unwrap() = Err("CMD_UNKNOWN_COMMAND".into());
    let record = h
        .enqueue(
            RunStarter::ScheduleEntry {
                entry: "daily".into(),
            },
            RunRequest::Command {
                command: "triage-inbox".into(),
                arguments: vec![],
            },
            serde_json::json!({}),
        )
        .await;
    h.sup.start_queue_workers();
    let view = h.wait_record(&record.run_id, RunRecordState::Ended).await;
    assert_eq!(view.record.end_kind, Some(RunEndKind::Failed));
    assert_eq!(
        view.outcome.unwrap().error_code.as_deref(),
        Some("CMD_UNKNOWN_COMMAND")
    );
    assert_eq!(h.local.start_count(), 0);
}

#[tokio::test]
async fn new_run_while_shutting_down() {
    let h = Harness::desktop().await;
    h.sup.shutdown(ShutdownReason::Quit).await;
    let err = h
        .sup
        .start_run(request(
            RunTarget::AgentInstance {
                agent_instance_id: AgentInstanceId::new(),
            },
            None,
        ))
        .await
        .unwrap_err();
    assert!(matches!(err, SupervisorError::Stopping));
    permanent(&err, "SUP_STOPPING");
    let err = h
        .sup
        .submit_task(task("compare-quotes"), WorkKind::Documents, false, None)
        .await
        .unwrap_err();
    permanent(&err, "SUP_STOPPING");
}

#[tokio::test]
async fn store_unavailable() {
    let h = Harness::desktop().await;
    h.store.close().await;
    let err = h
        .sup
        .submit_task(task("compare-quotes"), WorkKind::Documents, false, None)
        .await
        .unwrap_err();
    assert!(matches!(err, SupervisorError::Store(_)));
    assert_eq!(err.code(), "SUP_STORE_UNAVAILABLE");
    assert!(err.retryable());
    let err = h.sup.runs_with_command("x").await.unwrap_err();
    assert_eq!(err.code(), "SUP_STORE_UNAVAILABLE");
    let failure = h
        .sup
        .prune_participant()
        .prune(
            &h.store,
            &ordin_journal::PruneRequest {
                cutoff: ordin_types::time::Timestamp::now(),
                runs_in_progress: vec![],
            },
        )
        .await
        .unwrap_err();
    assert_eq!(failure.code, "SUP_STORE_UNAVAILABLE");
}

#[tokio::test]
async fn run_starting_does_not_return_within_2_s() {
    let h = Harness::desktop().await;
    *h.host.starting_delay.lock().unwrap() = Duration::from_secs(5);
    let began = std::time::Instant::now();
    let run = h.submit("compare-quotes").await;
    Node::wait_starts(&h.local, 1).await;
    let waited = began.elapsed();
    assert!(
        waited >= Duration::from_secs(2) && waited < Duration::from_secs(4),
        "{waited:?}"
    );
    h.wait_state(run, RunState::Running).await;
}

#[test]
fn every_owned_code_is_in_the_registry() {
    for code in ALL_CODES {
        let entry = ordin_types::error_registry::lookup(code).expect("registered");
        assert_eq!(entry.owner_crate, "ordin-session-supervisor", "{code}");
    }
}

#[test]
fn every_returned_code_is_owned_or_carried() {
    let errors = [
        SupervisorError::RunNotFound,
        SupervisorError::RunEnded,
        SupervisorError::RunNotPaused,
        SupervisorError::Stopping,
        SupervisorError::VerbNotOffered(AdapterVerb::Cancel),
        SupervisorError::NoAdapter(AdapterKind::HostedAgent),
        SupervisorError::Adapter(AdapterError::Unreachable),
    ];
    for error in errors {
        assert!(ALL_CODES.contains(&error.code()), "{}", error.code());
    }
}
