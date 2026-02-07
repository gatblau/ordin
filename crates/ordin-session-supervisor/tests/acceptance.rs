//! One test per Gherkin scenario of SessionSupervisor.md (and its ADR-111 amendment), named after
//! the scenario. The scenarios about log lines (run summaries, the slow host) are in `logging.rs`,
//! which owns its process's log subscriber.

mod support;

use std::time::{Duration, Instant};

use ordin_runtime_adapter::{
    AdapterError, AdapterKind, PauseReason, ResumeDecision, RunEnd, RunEndReason, RunEvent,
    RunTarget, ToolCaller, UsageDelta,
};
use ordin_session_supervisor::{
    ContentCheck, ConversationStart, ProgramKind, ShutdownReason, SupervisorError,
};
use ordin_types::domain::agent_profile::{RunEndKind, RunRecordState, RunStarter};
use ordin_types::domain::runs::{RunOrigin, RunPauseReason, RunState, TaskTarget};
use ordin_types::{AgentInstanceId, ApprovalId, CrewCharterId, CrewRunId, RunId, WorkspaceId};
use ordin_usage_planner::WorkKind;
use support::*;
use tokio_util::sync::CancellationToken;

fn usage(turns: u32) -> RunEvent {
    RunEvent::Usage {
        usage: UsageDelta {
            model_turns: turns,
            tool_calls: 1,
            input_tokens: Some(100),
            output_tokens: Some(20),
            cached_input_tokens: None,
        },
    }
}

#[tokio::test]
async fn context_use_is_exposed_when_the_harness_reports_it_c29_fallback() {
    // C-29 is not confirmed: `RunEvent::Usage` has no `context_used_percent`, so the answer is
    // none while the run runs and after it ends (SA-CLOSE-01 fallback).
    let h = Harness::desktop().await;
    let run = h.start_local(None).await;
    h.wait_state(run, RunState::Running).await;
    h.local.push(run, usage(1));
    assert_eq!(h.sup.context_used_percent(run), None);
    h.local.end(run, RunEndReason::Completed, Some("done"));
    h.wait_state(run, RunState::Completed).await;
    assert_eq!(h.sup.context_used_percent(run), None);
}

#[tokio::test]
async fn context_use_before_c29_is_confirmed() {
    let h = Harness::desktop().await;
    let run = h.start_local(None).await;
    h.wait_state(run, RunState::Running).await;
    h.local.push(run, usage(1));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(h.sup.context_used_percent(run), None);
    assert_eq!(h.sup.context_used_percent(RunId::new()), None);
}

#[tokio::test]
async fn the_caller_follows_the_runs_origin() {
    let h = Harness::desktop().await;
    let run = h.submit("compare-quotes").await;
    Node::wait_starts(&h.local, 1).await;
    let crew_run_id = CrewRunId::new();
    let mut req = request(
        RunTarget::AgentInstance {
            agent_instance_id: AgentInstanceId::new(),
        },
        None,
    );
    req.origin = RunOrigin::CrewRound {
        crew_run_id,
        round_number: 1,
    };
    h.sup.start_run(req).await.unwrap();
    Node::wait_starts(&h.local, 2).await;
    let starts = h.local.starts.lock().unwrap().clone();
    assert_eq!(starts[0].run_id, run);
    assert_eq!(starts[0].caller, ToolCaller::Agent);
    assert_eq!(starts[1].caller, ToolCaller::CrewMember { crew_run_id });
}

#[tokio::test]
async fn happy_path_a_task_runs_to_completion() {
    let h = Harness::desktop().await;
    let run = h.submit("compare-quotes").await;
    assert_eq!(h.count("SELECT count(*) FROM tasks").await, 1);
    let queued = h.events("run_state_changed").await;
    assert_eq!(queued[0]["state"], "queued");
    h.wait_state(run, RunState::Running).await;
    let start = h.local.starts.lock().unwrap()[0].clone();
    assert_eq!(start.limits.wall_clock_secs, 3_600);
    assert_eq!(start.limits.idle_timeout_secs, 600);
    assert!(start.model.is_some());
    h.local.push(run, usage(3));
    h.local
        .end(run, RunEndReason::Completed, Some("Kestrel is cheapest."));
    let ended = h.wait_state(run, RunState::Completed).await;
    assert!(ended.ended_at.is_some());
    assert_eq!(ended.usage.model_turns, 3);
    assert_eq!(h.events("start_up_report_ready").await.len(), 1);
    // The row is written first, then the event, then the ending's other steps.
    h.wait_ended_steps(run).await;
    let states: Vec<_> = h
        .events("run_state_changed")
        .await
        .into_iter()
        .map(|e| e["state"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(states, ["queued", "running", "completed"]);
    assert!(h.usage.today().await.unwrap().model_turns >= 3);
    assert_eq!(h.host.ended.lock().unwrap().as_slice(), &[run]);
}

#[tokio::test]
async fn the_ninth_local_session_waits() {
    let h = Harness::desktop().await;
    let mut runs = vec![];
    for _ in 0..8 {
        runs.push(h.start_local(None).await);
    }
    Node::wait_starts(&h.local, 8).await;
    let ninth = h.start_local(None).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(h.local.start_count(), 8);
    assert_eq!(h.sup.get(ninth).await.unwrap().run.state, RunState::Queued);
    assert_eq!(h.sup.running_local_sessions(), 8);
    h.local.end(runs[0], RunEndReason::Completed, None);
    Node::wait_starts(&h.local, 9).await;
    h.wait_state(ninth, RunState::Running).await;
}

#[tokio::test]
async fn wall_clock_limit() {
    let h = Harness::desktop().await;
    let run = h.start_local(limits(1, None, 600)).await;
    let run_view = h.wait_state(run, RunState::StoppedAtLimit).await;
    assert_eq!(run_view.end_reason.as_deref(), Some("wall-clock limit"));
    assert_eq!(h.local.cancels.lock().unwrap().as_slice(), &[run]);
}

#[tokio::test]
async fn time_paused_for_an_approval_does_not_count() {
    let h = Harness::desktop().await;
    let run = h.start_local(limits(2, None, 600)).await;
    h.wait_state(run, RunState::Running).await;
    tokio::time::sleep(Duration::from_millis(1_000)).await;
    h.local.push(
        run,
        RunEvent::Paused {
            reason: PauseReason::Approval {
                approval_id: ApprovalId::new(),
            },
        },
    );
    h.wait_state(run, RunState::Paused).await;
    tokio::time::sleep(Duration::from_millis(2_500)).await;
    assert_eq!(h.sup.get(run).await.unwrap().run.state, RunState::Paused);
    h.local.push(run, RunEvent::Resumed);
    h.wait_state(run, RunState::Running).await;
    let resumed = Instant::now();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(h.sup.get(run).await.unwrap().run.state, RunState::Running);
    let stopped = h.wait_state(run, RunState::StoppedAtLimit).await;
    assert_eq!(stopped.end_reason.as_deref(), Some("wall-clock limit"));
    assert!(resumed.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn request_budget() {
    let h = Harness::desktop().await;
    let run = h.start_local(limits(3_600, Some(10), 600)).await;
    h.wait_state(run, RunState::Running).await;
    h.local.push(run, usage(4));
    h.local.push(run, usage(6));
    let stopped = h.wait_state(run, RunState::StoppedAtLimit).await;
    assert_eq!(stopped.end_reason.as_deref(), Some("request budget"));
    assert_eq!(stopped.usage.model_turns, 10);
}

#[tokio::test]
async fn idle_timeout() {
    // The shortest idle timeout 2C allows is 30 s.
    let h = Harness::desktop().await;
    let run = h.start_local(limits(3_600, None, 30)).await;
    h.wait_state(run, RunState::Running).await;
    let began = Instant::now();
    for _ in 0..2_000 {
        if h.sup.get(run).await.unwrap().run.state == RunState::StoppedAtLimit {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let stopped = h.sup.get(run).await.unwrap().run;
    assert_eq!(stopped.state, RunState::StoppedAtLimit);
    assert_eq!(stopped.end_reason.as_deref(), Some("idle timeout"));
    assert!(began.elapsed() >= Duration::from_secs(29));
}

#[tokio::test]
async fn signed_out_at_start() {
    let h = Harness::desktop().await;
    *h.local.start_error.lock().unwrap() = Some(AdapterError::SignedOut);
    let run = h.start_local(None).await;
    let paused = h.wait_state(run, RunState::Paused).await;
    assert_eq!(paused.pause_reason, Some(RunPauseReason::SignIn));
}

#[tokio::test]
async fn the_harness_exits_unexpectedly() {
    let h = Harness::desktop().await;
    let run = h.start_local(None).await;
    h.wait_state(run, RunState::Running).await;
    h.local.close(run);
    let failed = h.wait_state(run, RunState::Failed).await;
    assert_eq!(
        failed.end_reason.as_deref(),
        Some("the session ended unexpectedly")
    );
}

#[tokio::test]
async fn stop_a_remote_run_that_cannot_be_cancelled() {
    let h = Harness::desktop().await;
    let run = h.start_remote(AdapterKind::SlackThread).await;
    h.wait_state(run, RunState::Running).await;
    h.sup.cancel(run).await.unwrap();
    let abandoned = h.wait_state(run, RunState::Abandoned).await;
    assert_eq!(abandoned.end_reason, None);
    assert_eq!(
        h.host.abandoned.lock().unwrap().as_slice(),
        &[(run, AdapterKind::SlackThread)]
    );
    assert!(h.entries(run, "run_result").await.is_empty());
}

#[tokio::test]
async fn a_hosted_agent_waits_for_its_own_approvers() {
    let h = Harness::desktop().await;
    let remote = ordin_types::RemoteAgentId::new();
    h.host
        .routes
        .lock()
        .unwrap()
        .insert(remote, AdapterKind::HostedAgent);
    let run = h
        .sup
        .start_run(request(
            RunTarget::RemoteAgent {
                remote_agent_id: remote,
            },
            limits(1, None, 600),
        ))
        .await
        .unwrap();
    h.wait_state(run, RunState::Running).await;
    h.hosted.push(
        run,
        RunEvent::Paused {
            reason: PauseReason::RemoteApprovers,
        },
    );
    let paused = h.wait_state(run, RunState::Paused).await;
    assert_eq!(paused.pause_reason, Some(RunPauseReason::Approval));
    let approval: Option<String> =
        sqlx::query_scalar("SELECT pause_approval_id FROM runs WHERE id = ?")
            .bind(run.to_string())
            .fetch_one(h.pool())
            .await
            .unwrap();
    assert_eq!(approval, None);
    // The 1 s wall clock does not run while the approvers decide.
    tokio::time::sleep(Duration::from_millis(1_500)).await;
    assert_eq!(h.sup.get(run).await.unwrap().run.state, RunState::Paused);
}

#[tokio::test]
async fn an_adapter_refuses_the_start() {
    let h = Harness::desktop().await;
    *h.hosted.start_error.lock().unwrap() = Some(AdapterError::Refused {
        code: "HAA_NO_ACCESS".into(),
    });
    let run = h.start_remote(AdapterKind::HostedAgent).await;
    let failed = h.wait_state(run, RunState::Failed).await;
    assert_eq!(
        failed.end_reason.as_deref(),
        Some("couldn't start the agent")
    );
    assert_eq!(h.codes(run).await, ["HAA_NO_ACCESS"]);
}

#[tokio::test]
async fn agent_profile_progress_reaches_the_run_queue() {
    let h = Harness::agent(ProgramKind::AgentSingle, "runs-1").await;
    let record = h
        .enqueue(
            chat("U1"),
            message("How many tickets?"),
            serde_json::json!({}),
        )
        .await;
    h.sup.start_queue_workers();
    Node::wait_starts(&h.local, 1).await;
    let run = h.local.started_runs()[0];
    h.wait_state(run, RunState::Running).await;
    h.local.push(
        run,
        RunEvent::Progress {
            progress: ordin_runtime_adapter::RunProgress {
                line: "Reading 3 tickets".into(),
                count: None,
            },
        },
    );
    Node::wait_until("progress kept", || {
        !h.queue.progress.lock().unwrap().is_empty()
    })
    .await;
    let progress = h.queue.progress.lock().unwrap().clone();
    assert_eq!(
        progress[0],
        (
            record.run_id.clone(),
            "runs-1".to_owned(),
            "Reading 3 tickets".to_owned()
        )
    );
    h.local.end(run, RunEndReason::Completed, Some("Three."));
    h.wait_record(&record.run_id, RecordState::Ended).await;
}

type RecordState = RunRecordState;

#[tokio::test]
async fn recovery_after_restart() {
    let h = Harness::desktop().await;
    let local = h.start_local(None).await;
    let hosted = h.start_remote(AdapterKind::HostedAgent).await;
    h.wait_state(local, RunState::Running).await;
    h.wait_state(hosted, RunState::Running).await;
    // The program restarts: a new supervisor over the same store.
    let restarted = h.another(config(ProgramKind::Desktop, "desktop"));
    for (id, kind) in h.host.routes.lock().unwrap().iter() {
        restarted.host.routes.lock().unwrap().insert(*id, *kind);
    }
    *restarted.hosted.read_answer.lock().unwrap() =
        Some(ended_snapshot(hosted, RunEndReason::Completed));
    // The local session cannot be resumed: `read` answers UnknownRun.
    let report = restarted.sup.recover().await.unwrap();
    assert_eq!(report.interrupted, 1);
    assert_eq!(report.ended_remotely, 1);
    restarted.wait_state(local, RunState::Interrupted).await;
    let done = restarted.wait_state(hosted, RunState::Completed).await;
    assert_eq!(done.end_reason.as_deref(), Some("Done remotely."));
}

#[tokio::test]
async fn a_crew_task_is_refused_here() {
    let h = Harness::desktop().await;
    let mut crew = task("compare-quotes");
    crew.target = TaskTarget::Crew {
        crew_charter_id: CrewCharterId::new(),
    };
    let err = h
        .sup
        .submit_task(crew, WorkKind::Documents, false, None)
        .await
        .unwrap_err();
    assert_eq!(err.code(), "SUP_TARGET_IS_CREW");
}

#[tokio::test]
async fn invalid_task() {
    let h = Harness::desktop().await;
    let mut both = task("compare-quotes");
    both.free_text = Some("and some text".into());
    let err = h
        .sup
        .submit_task(both, WorkKind::Documents, false, None)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        SupervisorError::TaskInvalid {
            code: "TASK_COMMAND_XOR_TEXT"
        }
    ));
    assert_eq!(h.count("SELECT count(*) FROM tasks").await, 0);
    assert_eq!(h.count("SELECT count(*) FROM runs").await, 0);
}

#[tokio::test]
async fn agent_profile_pause_and_resume_on_another_instance() {
    let h = Harness::agent(ProgramKind::AgentScaled, "runs-1").await;
    h.journal.subscribe(h.sup.event_sink());
    let record = h
        .enqueue(
            RunStarter::Caller {
                caller_id: "c-1".into(),
            },
            message("Close the ticket."),
            serde_json::json!({}),
        )
        .await;
    h.sup.start_queue_workers();
    Node::wait_starts(&h.local, 1).await;
    let run = h.local.started_runs()[0];
    h.wait_state(run, RunState::Running).await;
    let session = TestAdapter::session_of(run);
    h.sup
        .save_session_record(run, &session, b"session-bytes".to_vec())
        .await
        .unwrap();
    let a1 = ApprovalId::new();
    h.local.push(
        run,
        RunEvent::Paused {
            reason: PauseReason::Approval { approval_id: a1 },
        },
    );
    h.wait_record(&record.run_id, RecordState::Paused).await;
    assert_eq!(h.sup.get(run).await.unwrap().run.state, RunState::Paused);
    h.sup.stop_taking_runs().await;
    tokio::time::sleep(Duration::from_millis(1_200)).await;

    h.journal
        .append_event(
            "approval_decided",
            serde_json::json!({ "approval_id": a1, "state": "approved", "decided_on": "screen" }),
        )
        .await
        .unwrap();
    h.wait_record(&record.run_id, RecordState::Queued).await;

    let second = h.another(config(ProgramKind::AgentScaled, "runs-2"));
    second.sup.start_queue_workers();
    Node::wait_until("resumed on runs-2", || {
        !second.local.resumes.lock().unwrap().is_empty()
    })
    .await;
    let (resumed_run, decision) = second.local.resumes.lock().unwrap()[0].clone();
    assert_eq!(resumed_run, run);
    assert!(matches!(decision, ResumeDecision::Approve { approval_id } if approval_id == a1));
    assert_eq!(
        second.sup.load_session_record(run, &session).await.unwrap(),
        Some(b"session-bytes".to_vec())
    );
    second.local.push(run, second.local.started(run));
    second
        .local
        .end(run, RunEndReason::Completed, Some("Closed."));
    let view = h.wait_record(&record.run_id, RecordState::Ended).await;
    assert_eq!(view.record.end_kind, Some(RunEndKind::Completed));
    assert_eq!(view.record.result.as_deref(), Some("Closed."));
}

#[tokio::test]
async fn ordins_conversation_run() {
    let h = Harness::desktop().await;
    let sales = WorkspaceId::new();
    let run = h
        .sup
        .start_conversation(ConversationStart {
            workspace_id: sales,
            target: RunTarget::AgentInstance {
                agent_instance_id: AgentInstanceId::new(),
            },
            seed: "You are Ordin. The person asked for a comparison.".into(),
            limits: ordin_runtime_adapter::RunLimits {
                wall_clock_secs: 3_600,
                request_budget: None,
                idle_timeout_secs: 600,
            },
            model_override: None,
            only_servers: None,
        })
        .await
        .unwrap();
    let mut events = h.sup.subscribe(run).unwrap();
    assert_eq!(h.count("SELECT count(*) FROM tasks").await, 0);
    let work_key: String = sqlx::query_scalar("SELECT work_key FROM runs WHERE id = ?")
        .bind(run.to_string())
        .fetch_one(h.pool())
        .await
        .unwrap();
    assert_eq!(work_key, format!("conversation:{sales}"));
    let view = h.wait_state(run, RunState::Running).await;
    assert_eq!(
        view.origin,
        RunOrigin::Conversation {
            workspace_id: sales
        }
    );
    assert_eq!(h.local.starts.lock().unwrap()[0].caller, ToolCaller::Ordin);
    // SUP-QUIT-1: Ordin's turn in progress is a running local session.
    assert_eq!(h.sup.running_local_sessions(), 1);
    h.local.push(
        run,
        RunEvent::Paused {
            reason: PauseReason::Person {
                question: "Here is the comparison.".into(),
            },
        },
    );
    let turn = loop {
        match events.recv().await.unwrap() {
            RunEvent::Paused {
                reason: PauseReason::Person { question },
            } => break question,
            _ => continue,
        }
    };
    assert_eq!(turn, "Here is the comparison.");
    // SUP-QUIT-1: between turns, waiting for the person, it is not counted for the quit question.
    // spec: ordin-desktop/components/SessionSupervisor#SUP-QUIT-1
    for _ in 0..50 {
        if h.sup.running_local_sessions() == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(h.sup.running_local_sessions(), 0);
    h.sup
        .resume_with_decision(
            run,
            ResumeDecision::Answer {
                text: "Thanks, now draft the email".into(),
            },
        )
        .await
        .unwrap();
    let resumes = h.local.resumes.lock().unwrap().clone();
    assert!(
        matches!(&resumes[0].1, ResumeDecision::Answer { text } if text == "Thanks, now draft the email")
    );
}

#[tokio::test]
async fn wake_after_sleep() {
    let h = Harness::desktop().await;
    let run = h.start_local(None).await;
    h.wait_state(run, RunState::Running).await;
    h.sup.on_wake(Duration::from_secs(40 * 60)).await;
    h.wait_state(run, RunState::Interrupted).await;
}

#[tokio::test]
async fn idle_conversation_stopped_on_the_personal_server() {
    let h = Harness::with(config(ProgramKind::PersonalServer, "server")).await;
    let run = h.start_local(None).await;
    h.wait_state(run, RunState::Running).await;
    h.local.push(
        run,
        RunEvent::Paused {
            reason: PauseReason::Person {
                question: "Anything else?".into(),
            },
        },
    );
    h.wait_state(run, RunState::Paused).await;
    // The scenario's 31 and 30 minutes, scaled down: paused 300 ms, stopped after 100 ms.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        h.sup.stop_idle_sessions(Duration::from_millis(100)).await,
        1
    );
    h.wait_state(run, RunState::Interrupted).await;
}

#[tokio::test]
async fn a_duty_lock_refuses_the_resume() {
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
    assert!(h.local.resumes.lock().unwrap().is_empty());
    assert!(h
        .codes(run)
        .await
        .contains(&"SUP_RESUME_REFUSED".to_owned()));
}

#[tokio::test]
async fn session_record_saved_and_loaded() {
    let h = Harness::desktop().await;
    let run = h.start_local(None).await;
    h.wait_state(run, RunState::Running).await;
    h.local.push(
        run,
        RunEvent::Paused {
            reason: PauseReason::Approval {
                approval_id: ApprovalId::new(),
            },
        },
    );
    h.wait_state(run, RunState::Paused).await;
    h.sup
        .save_session_record(run, "s-1", vec![1, 2, 3])
        .await
        .unwrap();
    assert_eq!(
        h.sup.load_session_record(run, "s-1").await.unwrap(),
        Some(vec![1, 2, 3])
    );
    assert_eq!(h.sup.load_session_record(run, "s-2").await.unwrap(), None);
}

#[tokio::test]
async fn agent_profile_lease_lost() {
    let h = Harness::agent(ProgramKind::AgentSingle, "runs-1").await;
    let record = h
        .enqueue(chat("U1"), message("Hello"), serde_json::json!({}))
        .await;
    h.sup.start_queue_workers();
    Node::wait_starts(&h.local, 1).await;
    let run = h.local.started_runs()[0];
    h.wait_state(run, RunState::Running).await;
    *h.queue.renew_answer.lock().unwrap() = Some(ordin_run_queue::LeaseRenewal::Lost);
    // The lease keeper renews every 10 s.
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
    assert_eq!(h.local.cancels.lock().unwrap().as_slice(), &[run]);
    assert!(h.codes(run).await.contains(&"SUP_LEASE_LOST".to_owned()));
    assert_eq!(h.counters.count("wrightrun_run_leases_lost_total"), 1);
    // Never started again, and the record is left for another instance's `mark_interrupted`.
    tokio::time::sleep(Duration::from_millis(1_500)).await;
    assert_eq!(h.local.start_count(), 1);
    assert_eq!(
        h.record(&record.run_id).await.record.state,
        RecordState::Executing
    );
}

#[tokio::test]
async fn an_empty_conversation_seed_is_refused() {
    let h = Harness::desktop().await;
    let err = h
        .sup
        .start_conversation(ConversationStart {
            workspace_id: WorkspaceId::new(),
            target: RunTarget::AgentInstance {
                agent_instance_id: AgentInstanceId::new(),
            },
            seed: String::new(),
            limits: ordin_runtime_adapter::RunLimits {
                wall_clock_secs: 60,
                request_budget: None,
                idle_timeout_secs: 600,
            },
            model_override: None,
            only_servers: None,
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), "SUP_SEED_EMPTY");
    assert_eq!(h.count("SELECT count(*) FROM runs").await, 0);
}

#[tokio::test]
async fn personal_server_runs_may_finish_until_30_s_after_sigterm() {
    let h = Harness::with(config(ProgramKind::PersonalServer, "server")).await;
    let a = h.start_local(None).await;
    let b = h.start_local(None).await;
    h.wait_state(a, RunState::Running).await;
    h.wait_state(b, RunState::Running).await;
    // SIGTERM at T: the program waits; run A ends inside the window.
    h.local.end(a, RunEndReason::Completed, Some("done"));
    h.wait_state(a, RunState::Completed).await;
    h.wait_ended_steps(a).await;
    assert_eq!(h.sup.runs_going(), 1);
    let report = h.sup.shutdown(ShutdownReason::Signal).await;
    assert_eq!(report.interrupted, 1);
    h.wait_state(b, RunState::Interrupted).await;
    assert_eq!(h.local.cancels.lock().unwrap().as_slice(), &[b]);
}

async fn never_cut_short(program: ProgramKind) {
    let h = Harness::agent(program, "runs-1").await;
    let r1 = h
        .enqueue(chat("U1"), message("Hello"), serde_json::json!({}))
        .await;
    h.sup.start_queue_workers();
    Node::wait_starts(&h.local, 1).await;
    let run = h.local.started_runs()[0];
    h.wait_state(run, RunState::Running).await;
    h.sup.stop_taking_runs().await;
    let stop = CancellationToken::new();
    let cancel_later = stop.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1_500)).await;
        cancel_later.cancel();
    });
    let r2 = h
        .enqueue(chat("U2"), message("Another"), serde_json::json!({}))
        .await;
    h.sup.wait_for_runs(stop).await;
    assert!(h.local.cancels.lock().unwrap().is_empty());
    assert_eq!(h.local.start_count(), 1);
    assert_eq!(
        h.record(&r1.run_id).await.record.state,
        RecordState::Executing
    );
    assert_eq!(h.record(&r2.run_id).await.record.state, RecordState::Queued);
    assert_eq!(h.sup.get(run).await.unwrap().run.state, RunState::Running);
}

#[tokio::test]
async fn agent_profile_runs_are_never_cut_short_on_sigterm_agent_single() {
    never_cut_short(ProgramKind::AgentSingle).await;
}

#[tokio::test]
async fn agent_profile_runs_are_never_cut_short_on_sigterm_agent_scaled() {
    never_cut_short(ProgramKind::AgentScaled).await;
}

#[tokio::test]
async fn agent_profile_the_input_check_blocks_before_the_model() {
    let h = Harness::agent(ProgramKind::AgentSingle, "runs-1").await;
    h.host
        .checks
        .lock()
        .unwrap()
        .push_back(ContentCheck::Blocked {
            retry_without_context: true,
        });
    let record = h
        .enqueue(
            chat("U1"),
            message("A blocked question"),
            serde_json::json!({}),
        )
        .await;
    h.sup.start_queue_workers();
    let view = h.wait_record(&record.run_id, RecordState::Ended).await;
    assert_eq!(h.local.start_count(), 0);
    assert_eq!(view.record.end_kind, Some(RunEndKind::Completed));
    let outcome = view.outcome.unwrap();
    assert!(outcome.safe_reply);
    assert_eq!(outcome.reply.as_deref(), Some(SAFE_REPLY));
    assert_eq!(h.counters.count("wrightrun_safe_replies_total"), 1);
}

#[tokio::test]
async fn agent_profile_a_blocked_answer_after_gathered_context_is_tried_once_more_without_it() {
    let h = Harness::agent(ProgramKind::AgentSingle, "runs-1").await;
    {
        let mut checks = h.host.checks.lock().unwrap();
        checks.push_back(ContentCheck::Passed);
        checks.push_back(ContentCheck::Blocked {
            retry_without_context: true,
        });
        checks.push_back(ContentCheck::Blocked {
            retry_without_context: true,
        });
    }
    let context = serde_json::json!({ "context": [
        { "kind": "piece", "piece": "transcript", "label": "thread", "media_type": "text/plain",
          "content": "…", "size_bytes": 10, "cut": false, "step": 1, "items": 2 }
    ]});
    let record = h.enqueue(chat("U1"), message("Question"), context).await;
    h.sup.start_queue_workers();
    Node::wait_starts(&h.local, 1).await;
    let run = h.local.started_runs()[0];
    h.wait_state(run, RunState::Running).await;
    h.local
        .end(run, RunEndReason::Completed, Some("first answer"));
    Node::wait_starts(&h.local, 2).await;
    h.local
        .end(run, RunEndReason::Completed, Some("second answer"));
    let view = h.wait_record(&record.run_id, RecordState::Ended).await;
    let outcome = view.outcome.unwrap();
    assert!(outcome.safe_reply);
    assert_eq!(outcome.reply.as_deref(), Some(SAFE_REPLY));
    let summary = view.record.stretch_summaries.last().unwrap();
    let summary: serde_json::Value = serde_json::from_str(summary).unwrap();
    assert_eq!(summary["retried_without_context"], true);
    assert_eq!(summary["checks"], 3);
    assert_eq!(summary["checks_blocked"], 2);
}

#[tokio::test]
async fn edge_crew_rounds_and_conversations_do_not_call_run_starting() {
    let h = Harness::desktop().await;
    let mut req = request(
        RunTarget::AgentInstance {
            agent_instance_id: AgentInstanceId::new(),
        },
        None,
    );
    req.origin = RunOrigin::CrewRound {
        crew_run_id: CrewRunId::new(),
        round_number: 1,
    };
    h.sup.start_run(req).await.unwrap();
    h.sup
        .start_conversation(ConversationStart {
            workspace_id: WorkspaceId::new(),
            target: RunTarget::AgentInstance {
                agent_instance_id: AgentInstanceId::new(),
            },
            seed: "Seed".into(),
            limits: ordin_runtime_adapter::RunLimits {
                wall_clock_secs: 60,
                request_budget: None,
                idle_timeout_secs: 600,
            },
            model_override: None,
            only_servers: None,
        })
        .await
        .unwrap();
    Node::wait_starts(&h.local, 2).await;
    assert!(h.host.starting.lock().unwrap().is_empty());
}

#[tokio::test]
async fn happy_path_the_host_is_told_before_an_agent_task_starts() {
    let h = Harness::desktop().await;
    let mut t = task("compare-quotes");
    let agent = AgentInstanceId::new();
    t.target = TaskTarget::AgentInstance {
        agent_instance_id: agent,
    };
    h.sup
        .submit_task(t, WorkKind::Documents, false, None)
        .await
        .unwrap();
    Node::wait_starts(&h.local, 1).await;
    let starting = h.host.starting.lock().unwrap().clone();
    assert_eq!(starting.len(), 1);
    assert_eq!(starting[0].caller, ToolCaller::Agent);
    assert!(matches!(
        starting[0].target,
        RunTarget::AgentInstance { agent_instance_id } if agent_instance_id == agent
    ));
    let calls = h.calls.lock().unwrap().clone();
    assert_eq!(calls, ["run_starting", "start"]);
}

#[tokio::test]
async fn edge_a_check_failure_still_calls_nothing() {
    let h = Harness::desktop().await;
    // A task command may hold characters a harness command name may not (2C RA_COMMAND_UNKNOWN).
    let run = h.submit("Compare Quotes!").await;
    let failed = h.wait_state(run, RunState::Failed).await;
    assert_eq!(
        failed.end_reason.as_deref(),
        Some("couldn't start the agent")
    );
    assert_eq!(h.codes(run).await, ["RA_COMMAND_UNKNOWN"]);
    assert!(h.host.starting.lock().unwrap().is_empty());
    assert_eq!(h.local.start_count(), 0);
}

#[tokio::test]
async fn the_ended_run_is_followed_by_the_host_and_forgotten() {
    // C.3 `Ended`: `run_ended` once, the run is no longer live, and `has_ended` reads the row.
    let h = Harness::desktop().await;
    let run = h.start_local(None).await;
    h.wait_state(run, RunState::Running).await;
    assert!(h.sup.is_live(run));
    assert_eq!(h.sup.has_ended(run).await.unwrap(), Some(false));
    h.local.end(run, RunEndReason::Failed, None);
    h.wait_state(run, RunState::Failed).await;
    Node::wait_until("forgotten", || !h.sup.is_live(run)).await;
    assert_eq!(h.sup.has_ended(run).await.unwrap(), Some(true));
    assert_eq!(h.sup.has_ended(RunId::new()).await.unwrap(), None);
    assert_eq!(h.host.ended.lock().unwrap().as_slice(), &[run]);
}

#[tokio::test]
async fn command_facts_and_listing() {
    let h = Harness::desktop().await;
    *h.local.start_error.lock().unwrap() = Some(AdapterError::SignedOut);
    let a = h.submit("compare-quotes").await;
    let b = h.submit("compare-quotes").await;
    h.wait_state(a, RunState::Paused).await;
    h.wait_state(b, RunState::Paused).await;
    assert_eq!(h.sup.runs_with_command("compare-quotes").await.unwrap(), 2);
    assert_eq!(h.sup.runs_with_command("other").await.unwrap(), 0);
    assert!(h
        .sup
        .paused_with_command(&["compare-quotes".into()], a)
        .await
        .unwrap());
    assert!(!h.sup.paused_with_command(&[], a).await.unwrap());
    let page = h.sup.list(None, None).await.unwrap();
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].id, b);
    assert!(page.next_cursor.is_none());
    let bad = h.sup.list(None, Some("nonsense".into())).await.unwrap_err();
    assert_eq!(bad.code(), "SRV_BAD_CURSOR");
}

// Amendment 2026-01-24: the person's own input (SA-SUP-INPUT-01).

/// A task whose input is free text, for a new agent instance.
fn free_text_task(text: &str) -> ordin_types::domain::runs::Task {
    let mut t = task("unused");
    t.command = None;
    t.arguments = vec![];
    t.free_text = Some(text.into());
    t
}

#[tokio::test]
async fn happy_path_the_owners_typed_command_reaches_the_adapter_as_the_persons_own() {
    let h = Harness::desktop().await;
    h.sup
        .submit_task(
            free_text_task("/analyst:deploy now"),
            WorkKind::Documents,
            true,
            None,
        )
        .await
        .unwrap();
    Node::wait_starts(&h.local, 1).await;
    let start = h.local.starts.lock().unwrap()[0].clone();
    assert!(start.started_by_person);
    assert!(
        matches!(&start.input, ordin_runtime_adapter::StartInput::FreeText { text } if text == "/analyst:deploy now")
    );
}

#[tokio::test]
async fn error_path_text_nobody_typed_for_the_owner_is_not_the_persons_own() {
    let h = Harness::desktop().await;
    // A task an agent delegated.
    h.sup
        .submit_task(
            free_text_task("/analyst:deploy now"),
            WorkKind::Documents,
            false,
            None,
        )
        .await
        .unwrap();
    // A crew round, although its request says the person started the crew.
    let mut round = request(
        RunTarget::AgentInstance {
            agent_instance_id: AgentInstanceId::new(),
        },
        None,
    );
    round.origin = RunOrigin::CrewRound {
        crew_run_id: CrewRunId::new(),
        round_number: 1,
    };
    assert!(round.started_by_person);
    h.sup.start_run(round).await.unwrap();
    // Ordin's conversation, whose seed the Ordin Host composed.
    h.sup
        .start_conversation(ConversationStart {
            workspace_id: WorkspaceId::new(),
            target: RunTarget::AgentInstance {
                agent_instance_id: AgentInstanceId::new(),
            },
            seed: "/analyst:deploy now".into(),
            limits: ordin_runtime_adapter::RunLimits {
                wall_clock_secs: 3_600,
                request_budget: None,
                idle_timeout_secs: 600,
            },
            model_override: None,
            only_servers: None,
        })
        .await
        .unwrap();
    Node::wait_starts(&h.local, 3).await;
    for start in h.local.starts.lock().unwrap().iter() {
        assert!(!start.started_by_person, "{:?}", start.caller);
    }

    // The agent profile: a chat asker's message.
    let a = Harness::agent(ProgramKind::AgentSingle, "runs-1").await;
    a.enqueue(
        chat("U1"),
        message("/analyst:deploy now"),
        serde_json::json!({}),
    )
    .await;
    a.sup.start_queue_workers();
    Node::wait_starts(&a.local, 1).await;
    let start = a.local.starts.lock().unwrap()[0].clone();
    assert!(!start.started_by_person);
}

// Amendment 2026-01-24: the current pause for a late subscriber (SA-SUP-PAUSE-01).

const WITHIN: Duration = Duration::from_secs(5);

/// The next event a subscriber receives, within [`WITHIN`].
async fn next_event(events: &mut tokio::sync::broadcast::Receiver<RunEvent>) -> RunEvent {
    tokio::time::timeout(WITHIN, events.recv())
        .await
        .expect("an event arrives")
        .expect("the channel is open")
}

#[tokio::test]
async fn happy_path_a_late_subscriber_receives_the_current_pause_once() {
    let h = Harness::desktop().await;
    let run = h.start_local(None).await;
    h.wait_state(run, RunState::Running).await;
    let mut first = h.sup.subscribe(run).unwrap();
    h.local.push(
        run,
        RunEvent::Paused {
            reason: PauseReason::Person {
                question: "Which quarter?".into(),
            },
        },
    );
    let asked = |reason: &PauseReason| matches!(reason, PauseReason::Person { question } if question == "Which quarter?");
    // The first subscriber may see `Started` first: it subscribed once the row said running.
    loop {
        match next_event(&mut first).await {
            RunEvent::Paused { reason } => {
                assert!(asked(&reason), "{reason:?}");
                break;
            }
            RunEvent::Started { .. } => continue,
            other => panic!("unexpected event {other:?}"),
        }
    }
    let mut second = h.sup.subscribe(run).unwrap();
    match next_event(&mut second).await {
        RunEvent::Paused { reason } => assert!(asked(&reason), "{reason:?}"),
        other => panic!("the late subscriber's first event is {other:?}"),
    }
    h.local.push(run, RunEvent::Resumed);
    // Exactly once: the next event of each is `Resumed`, not the pause again.
    assert!(matches!(next_event(&mut second).await, RunEvent::Resumed));
    assert!(matches!(next_event(&mut first).await, RunEvent::Resumed));
}

#[tokio::test]
async fn edge_case_no_replay_when_no_pause_is_current() {
    let h = Harness::desktop().await;
    let run = h.start_local(None).await;
    h.wait_state(run, RunState::Running).await;
    let mut watcher = h.sup.subscribe(run).unwrap();
    h.local.push(
        run,
        RunEvent::Paused {
            reason: PauseReason::Person {
                question: "Which quarter?".into(),
            },
        },
    );
    h.local.push(run, RunEvent::Resumed);
    while !matches!(next_event(&mut watcher).await, RunEvent::Resumed) {}
    let mut late = h.sup.subscribe(run).unwrap();
    h.local.push(
        run,
        RunEvent::Progress {
            progress: ordin_runtime_adapter::RunProgress {
                line: "Reading the quotes".into(),
                count: None,
            },
        },
    );
    match next_event(&mut late).await {
        RunEvent::Progress { progress } => assert_eq!(progress.line, "Reading the quotes"),
        other => panic!("the subscriber's first event is {other:?}"),
    }
}

// Scenario: Happy path — a task run's view names its workspace and task (SUP-REC-1)
#[tokio::test]
async fn happy_path_a_task_runs_view_names_its_workspace_and_task() {
    let h = Harness::desktop().await;
    let t = task("compare-quotes");
    let run = h
        .sup
        .submit_task(t.clone(), WorkKind::Documents, false, None)
        .await
        .expect("submit");
    let view = h.sup.get(run).await.unwrap();
    assert_eq!(view.workspace_id, t.workspace_id);
    let read = view.task.expect("the task of a task run");
    assert_eq!(read.id, t.id);
    assert_eq!(read.workspace_id, t.workspace_id);
    assert_eq!(read.command, t.command);
}

// Scenario: Edge case — a crew member run has no task (SUP-REC-1)
#[tokio::test]
async fn edge_case_a_crew_member_run_has_no_task() {
    let h = Harness::desktop().await;
    let mut req = request(
        RunTarget::AgentInstance {
            agent_instance_id: AgentInstanceId::new(),
        },
        None,
    );
    req.origin = RunOrigin::CrewRound {
        crew_run_id: CrewRunId::new(),
        round_number: 1,
    };
    let workspace_id = req.workspace_id;
    let run = h.sup.start_run(req).await.unwrap();
    let view = h.sup.get(run).await.unwrap();
    assert_eq!(view.workspace_id, workspace_id);
    assert!(view.task.is_none());
}

// SUP-END-1: found in the Phase 5 walk on Windows — the run's final message was journalled but the
// Ended a subscriber received carried none, so the Ordin Host's card read "<agent> finished.".
// spec: ordin-desktop/components/SessionSupervisor#SUP-END-1
#[tokio::test]
async fn a_subscriber_receives_the_final_message_with_ended() {
    let h = Harness::desktop().await;
    let run = h.start_local(None).await;
    h.wait_state(run, RunState::Running).await;
    let mut watcher = h.sup.subscribe(run).unwrap();
    h.local.push(
        run,
        RunEvent::Ended {
            end: RunEnd {
                reason: RunEndReason::Completed,
                final_message: Some("Kestrel is cheapest.".into()),
                artefacts: vec![],
            },
        },
    );
    loop {
        match next_event(&mut watcher).await {
            RunEvent::Ended { end } => {
                assert_eq!(end.reason, RunEndReason::Completed);
                assert_eq!(end.final_message.as_deref(), Some("Kestrel is cheapest."));
                break;
            }
            RunEvent::Started { .. } => continue,
            other => panic!("unexpected event {other:?}"),
        }
    }
}

// ---- Amendment January 2026 — several named values in one command run (ADR-119) ----

/// The arguments text the adapter received for a task with these (name, value) arguments.
async fn started_arguments(command: &str, arguments: &[(&str, &str)]) -> String {
    let h = Harness::desktop().await;
    let mut t = task(command);
    t.arguments = arguments
        .iter()
        .map(|(name, value)| ordin_types::domain::runs::TaskArgument {
            name: (*name).into(),
            value: (*value).into(),
        })
        .collect();
    h.sup
        .submit_task(t, WorkKind::Documents, false, None)
        .await
        .unwrap();
    Node::wait_starts(&h.local, 1).await;
    let starts = h.local.starts.lock().unwrap().clone();
    match &starts[0].input {
        ordin_runtime_adapter::StartInput::Command { arguments, .. } => arguments.clone(),
        other => panic!("a command start, not {other:?}"),
    }
}

// Scenario: Happy path — a task with two inputs (SUP-TF-1)
// spec: ordin-desktop/components/SessionSupervisor#SUP-TF-1
#[tokio::test]
async fn happy_path_a_task_with_two_inputs() {
    let arguments = started_arguments(
        "apply",
        &[
            ("plan", "Key rotation plan (in Plans: 03-key-rotation.md)"),
            ("phase", "2"),
        ],
    )
    .await;
    assert_eq!(
        arguments,
        "plan: Key rotation plan (in Plans: 03-key-rotation.md)\nphase: 2"
    );
}

// Scenario: Edge — a value of several lines (SUP-TF-1)
#[tokio::test]
async fn edge_a_value_of_several_lines() {
    let arguments = started_arguments(
        "rca",
        &[("error", "panic: boom\n  at main.go:12\r\nexit 2")],
    )
    .await;
    assert_eq!(arguments, "error: panic: boom\n    at main.go:12\n  exit 2");
}

// Scenario: Edge — an empty value is not listed (SUP-TF-1)
#[tokio::test]
async fn edge_an_empty_value_is_not_listed() {
    let arguments = started_arguments(
        "summarise-document",
        &[
            ("document", "strategy.pdf (in Docs: plans/strategy.pdf)"),
            ("reader", ""),
        ],
    )
    .await;
    assert_eq!(
        arguments,
        "document: strategy.pdf (in Docs: plans/strategy.pdf)"
    );
}
