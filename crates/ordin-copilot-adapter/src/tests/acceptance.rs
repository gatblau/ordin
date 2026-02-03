//! One test per Gherkin scenario of CopilotLocalAdapter.md, its January 2026 amendment (CPA-REC-1)
//! and its 2026-02-02 amendment (CPA-CPX-1 to CPA-CPX-5).

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Duration;

use ordin_mcp_front::ProgramProfile;
use ordin_runtime_adapter::{AdapterError, PauseReason, ResumeDecision, RunEvent, RuntimeAdapter};
use ordin_types::{ApprovalId, ArtefactId};

use super::support::*;
use crate::adapter::{CopilotLocalAdapter, Host, Services};
use crate::error::CopilotAdapterError;
use crate::port::{CopilotEvent, GateAnswer};
use crate::testkit::ScriptedCopilotPort;
use ordin_hook_relay::HookSessionEvent;

// spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CPX-3
#[tokio::test]
async fn happy_path_free_text_runs_to_completion() {
    let h = Harness::new();
    let (handle, session) = h
        .start_live("Summarise the three quotes in one table")
        .await;

    let started = h.script().started.lock().unwrap().clone();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].working_folder, h.dir.0.join("agent"));
    let created = h.script().created.lock().unwrap().clone();
    let servers = &created[0].1["mcpServers"];
    assert!(servers.get("work").is_some() && servers.get("memory").is_some());
    assert_eq!(servers["work"]["args"][1], "work");

    h.script().emit(
        &session,
        CopilotEvent::ToolFinished {
            tool: "view".into(),
            succeeded: true,
        },
    );
    h.script().emit(
        &session,
        CopilotEvent::Usage {
            requests: Some(1),
            input_tokens: None,
            output_tokens: None,
            cached_input_tokens: None,
        },
    );
    h.script().emit(
        &session,
        CopilotEvent::TurnEnded {
            final_text: Some("Here is the table.".into()),
        },
    );
    let events = h.until_ended(&handle).await;
    assert_eq!(kinds(&events), ["started", "progress", "usage", "ended"]);
    assert_eq!(end_reason(&events).as_deref(), Some("completed"));
    let RunEvent::Usage { usage } = &events[2] else {
        panic!()
    };
    assert_eq!((usage.model_turns, usage.tool_calls), (1, 1));
}

#[tokio::test]
async fn credential_variables_never_reach_the_cli_server() {
    let h = Harness::new();
    h.start_live("Do the work.").await;
    let env = h.script().started.lock().unwrap()[0].env.clone();
    for name in [
        "COPILOT_GITHUB_TOKEN",
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "ORDIN_SECRET",
    ] {
        assert!(!env.contains_key(name), "{name} reached the CLI");
    }
    assert_eq!(env.get("HOME").map(String::as_str), Some("/home/p"));
}

#[tokio::test]
async fn every_tool_request_goes_through_the_hook_relay_and_a_deny_blocks_it() {
    let h = Harness::new();
    let reason = "Shell commands are not allowed for this agent.";
    *h.hooks.pre_tool_use.lock().unwrap() = Decide::Deny(reason.into());
    let (_, session) = h.start_live("Do the work.").await;
    let gates = h.script().gates(&session).expect("gates");
    let answer = gates
        .pre_tool_use
        .decide(
            "shell",
            serde_json::json!({"command": "rm -rf build"}),
            Some("call_7".into()),
        )
        .await;
    assert_eq!(
        answer,
        GateAnswer::Deny {
            reason: reason.into()
        }
    );
    let message = h
        .hooks
        .messages()
        .into_iter()
        .find(|m| m["kind"] == "pre_tool_use")
        .unwrap();
    assert_eq!(message["harness"], "copilot_cli");
    assert_eq!(message["origin"]["kind"], "built_in");
    assert_eq!(message["harness_tool_use_id"], "call_7");
}

// spec: ordin-desktop/components/CopilotLocalAdapter#CPA_GATE_FAILED
#[tokio::test]
async fn fail_closed_the_decision_call_times_out() {
    let h = Harness::new();
    *h.hooks.pre_tool_use.lock().unwrap() = Decide::Hang;
    let (_, session) = h.start_live("Do the work.").await;
    let gates = h.script().gates(&session).expect("gates");
    let answer = gates
        .permission
        .decide("write", serde_json::json!({}), None)
        .await;
    assert_eq!(
        answer,
        GateAnswer::Deny {
            reason: "ordin could not check this action.".into()
        }
    );
    expect_log(&["copilot.gate.failed", "CPA_GATE_FAILED"]).await;
}

#[tokio::test]
async fn signed_out_the_github_sign_in_is_refreshed_silently_first() {
    let port = ScriptedCopilotPort::default();
    let h = Harness::with(port, |_| {});
    *h.identity.refreshed.lock().unwrap() = true;
    *h.identity.signs_in.lock().unwrap() = Some(h.port.script.clone());
    let (handle, _) = h.start_live("Do the work.").await;
    assert_eq!(
        h.identity.calls.lock().unwrap().as_slice(),
        [ordin_types::domain::identity::CredentialKind::Github]
    );
    assert_eq!(h.script().created.lock().unwrap().len(), 1);
    let snapshot = h.adapter.read(&ctx(), &handle).await.unwrap();
    assert_eq!(serde_json::to_value(snapshot.state).unwrap(), "running");
    let events = h.events_until(&handle, |seen| !seen.is_empty()).await;
    assert!(!kinds(&events).contains(&"paused".to_owned()));
}

#[tokio::test]
async fn still_signed_out_the_run_pauses_for_the_persons_sign_in() {
    let h = Harness::with(ScriptedCopilotPort::default(), |_| {});
    *h.script().login_signs_in_after.lock().unwrap() = Some(Duration::from_millis(100));
    let handle = h.start(free_text("Do the work.")).await.expect("start");
    let events = h
        .events_until(&handle, |seen| {
            seen.iter().any(|e| matches!(e, RunEvent::Resumed))
        })
        .await;
    assert_eq!(kinds(&events)[..3], ["started", "paused", "resumed"]);
    assert!(matches!(
        &events[1],
        RunEvent::Paused {
            reason: PauseReason::SignIn
        }
    ));
    assert!(h.script().logins.load(Ordering::SeqCst) >= 1);
    // The session follows the sign-in, with the first turn.
    poll("the first turn", || {
        (!h.script().sent_texts().is_empty()).then_some(())
    })
    .await;
}

#[tokio::test]
async fn an_approval_outlives_the_deadline_and_is_approved_later() {
    let h = Harness::new();
    let (handle, _) = h.start_live("Do the work.").await;
    let approval = ApprovalId::new();
    h.hooks.send(
        handle.run_id,
        HookSessionEvent::DeniedAtDeadline {
            approval_id: approval,
            tool: "shell".into(),
        },
    );
    let events = h
        .events_until(&handle, |seen| {
            seen.iter().any(|e| matches!(e, RunEvent::Paused { .. }))
        })
        .await;
    assert!(
        matches!(events.last(), Some(RunEvent::Paused { reason: PauseReason::Approval { approval_id } }) if *approval_id == approval)
    );
    h.adapter
        .resume(
            &ctx(),
            &handle,
            ResumeDecision::Approve {
                approval_id: approval,
            },
        )
        .await
        .expect("resume");
    let events = h
        .events_until(&handle, |seen| {
            seen.iter().any(|e| matches!(e, RunEvent::Resumed))
        })
        .await;
    assert!(matches!(events.last(), Some(RunEvent::Resumed)));
    let sent = h.script().sent_texts();
    assert_eq!(
        sent.last().unwrap(),
        "The person approved the action you asked for. Make the same call again now."
    );
}

// spec: ordin-desktop/components/CopilotLocalAdapter#CPA_PROFILE_NOT_OFFERED
#[tokio::test]
async fn refused_in_the_agent_profile() {
    let dir = TempDir::new();
    let h = Harness::new();
    let error = CopilotLocalAdapter::build(
        config(dir.0.clone(), ProgramProfile::AgentProfile),
        services(&h),
        Host {
            timings: TEST_TIMINGS,
            parent_env: std::sync::Arc::new(|_: &str| None),
        },
    )
    .unwrap_err();
    assert_eq!(error.code(), "CPA_PROFILE_NOT_OFFERED");
    assert!(matches!(error, CopilotAdapterError::ProfileNotOffered));
}

pub(super) fn services(h: &Harness) -> Services {
    Services {
        port: std::sync::Arc::new(h.port.clone()),
        cli: h.cli.clone(),
        mcp: h.mcp.clone(),
        hooks: h.hooks.clone(),
        rules: h.rules.clone(),
        identity: h.identity.clone(),
        setup: h.setup.clone(),
    }
}

// spec: ordin-desktop/components/CopilotLocalAdapter#CPA_SERVER_EXITED
#[tokio::test]
async fn the_cli_server_exits_during_a_turn() {
    let h = Harness::new();
    let (handle, session) = h.start_live("Do the work.").await;
    h.script()
        .emit(&session, CopilotEvent::ServerExited { status: Some(1) });
    let events = h.until_ended(&handle).await;
    assert_eq!(end_reason(&events).as_deref(), Some("failed"));
    poll("sessions closed", || {
        (!h.mcp.is_open(handle.run_id) && !h.hooks.is_open(handle.run_id)).then_some(())
    })
    .await;
}

#[tokio::test]
async fn a_harness_to_do_list_becomes_a_plan() {
    let h = Harness::new();
    let (handle, session) = h.start_live("Do the work.").await;
    let steps: Vec<String> = [
        "Read the three quotes",
        "Compare the rates",
        "Write the table",
    ]
    .map(String::from)
    .to_vec();
    h.script().emit(
        &session,
        CopilotEvent::TodoList {
            items: steps.clone(),
        },
    );
    let events = h
        .events_until(&handle, |seen| {
            seen.iter()
                .any(|e| matches!(e, RunEvent::HarnessPlan { .. }))
        })
        .await;
    assert!(matches!(events.last(), Some(RunEvent::HarnessPlan { steps: got }) if *got == steps));
}

#[tokio::test]
async fn the_programs_copilot_probe_reads_the_sign_in_through_the_adapter() {
    let h = Harness::new();
    *h.script().method.lock().unwrap() = Some("device".into());
    let status = h.adapter.probe_sign_in().await.expect("probe");
    assert!(status.signed_in);
    assert_eq!(status.method.as_deref(), Some("device"));
    h.adapter.probe_sign_in().await.expect("probe again");
    assert_eq!(h.script().started.lock().unwrap().len(), 1, "started once");
}

// spec: ordin-desktop/components/CopilotLocalAdapter#CPA-REC-1
#[tokio::test]
async fn the_start_requests_attachments_reach_the_session_setup() {
    let h = Harness::new();
    let (a, b) = (ArtefactId::new(), ArtefactId::new());
    let mut req = free_text("Read these.");
    req.attachments = vec![a, b];
    h.start(req).await.expect("start");
    assert_eq!(h.setup.calls.lock().unwrap()[0], vec![a, b]);
}

/// The adapter side of the crew-round rule: only the program's `attachment_paths` reach the turn.
#[tokio::test]
async fn a_crew_members_input_outside_its_agent_folder_is_not_attached() {
    let h = Harness::new();
    let inside = h.dir.0.join("agent").join("brief.md");
    h.setup.setup.lock().unwrap().attachment_paths = vec![inside.clone()];
    let mut req = free_text("Do your part.");
    req.attachments = vec![ArtefactId::new(), ArtefactId::new()];
    h.start(req).await.expect("the session starts");
    let turn = h.script().sent_texts()[0].clone();
    assert_eq!(
        turn,
        format!("Do your part.\nAttached: {}", inside.display())
    );
}

// spec: ordin-desktop/components/CopilotLocalAdapter#CPA_SETUP_FAILED
#[tokio::test]
async fn a_tasks_attachment_outside_its_agent_folder_fails_the_setup() {
    let h = Harness::new();
    *h.setup.fail.lock().unwrap() = Some("RA_ATTACHMENT_NOT_GRANTED".into());
    let req = free_text("Read it.");
    let run = req.run_id.to_string();
    let error = h.start(req).await.unwrap_err();
    assert!(matches!(error, AdapterError::Unreachable));
    expect_log(&[&run, "CPA_SETUP_FAILED", "RA_ATTACHMENT_NOT_GRANTED"]).await;
}

#[tokio::test]
async fn the_sessions_server_is_started_from_the_persons_cli() {
    let h = Harness::new();
    let req = free_text("Summarise the three quotes in one table");
    let run = req.run_id.to_string();
    h.start(req).await.expect("start");
    let spec = h.script().started.lock().unwrap()[0].clone();
    assert_eq!(spec.cli_path, PathBuf::from(CLI_PATH));
    assert!(spec.env["PATH"].ends_with("/home/p/.local/bin"));
    expect_log(&[&run, "copilot.server.started", "1.0.98"]).await;
}

// spec: ordin-desktop/components/CopilotLocalAdapter#CPA_SERVER_START_FAILED
#[tokio::test]
async fn no_copilot_cli_installed() {
    let h = Harness::new();
    *h.cli.0.lock().unwrap() = None;
    let req = free_text("Do the work.");
    let run = req.run_id.to_string();
    let failure = h.adapter.start_checked(&ctx(), req).await.unwrap_err();
    assert_eq!(
        (failure.code, failure.retryable),
        ("CPA_SERVER_START_FAILED", true)
    );
    assert!(matches!(failure.error, AdapterError::Unreachable));
    assert!(h.script().started.lock().unwrap().is_empty());
    expect_log(&[&run, "copilot.cli.missing"]).await;
}

#[tokio::test]
async fn a_cli_installed_after_construction_is_used_without_a_restart() {
    let h = Harness::new();
    *h.cli.0.lock().unwrap() = None;
    assert!(h.start(free_text("First.")).await.is_err());
    *h.cli.0.lock().unwrap() = CliDouble::installed().0.into_inner().unwrap();
    h.start(free_text("Second.")).await.expect("start");
    assert_eq!(
        h.script().started.lock().unwrap()[0].cli_path,
        PathBuf::from(CLI_PATH)
    );
}

// spec: ordin-desktop/components/CopilotLocalAdapter#CPA-CPX-4
#[tokio::test]
async fn a_probe_call_with_no_cli_installed() {
    let h = Harness::new();
    *h.cli.0.lock().unwrap() = None;
    let error = h.adapter.probe_sign_in().await.unwrap_err();
    assert_eq!(
        (error.code(), error.retryable()),
        ("CPA_PROBE_FAILED", true)
    );
}
