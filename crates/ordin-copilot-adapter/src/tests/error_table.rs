//! One test per row of CopilotLocalAdapter.md's *Error Table* and the rows the 2026-02-02 amendment
//! changed. Each checks the code, its retryability, the 2C answer and the registry's entry.

use std::sync::atomic::Ordering;
use std::time::Duration;

use ordin_hook_relay::HookSessionEvent;
use ordin_mcp_front::{McpError, ProgramProfile};
use ordin_rules::StartOutcome;
use ordin_runtime_adapter::{
    AdapterError, ResumeDecision, RunEvent, RunTarget, RuntimeAdapter, StartInput,
};
use ordin_types::error_registry::lookup;
use ordin_types::{ApprovalId, RemoteAgentId, RunId};

use super::acceptance::services;
use super::support::*;
use crate::adapter::{CopilotLocalAdapter, Host};
use crate::error::{CopilotAdapterError, Failure};
use crate::port::{CopilotEvent, GateAnswer};
use crate::testkit::ScriptedCopilotPort;

fn registered(code: &str, retryable: bool) {
    let entry = lookup(code).unwrap_or_else(|| panic!("{code} is not registered"));
    assert_eq!(entry.retryable, retryable, "{code}: registry retryability");
}

fn assert_failure(failure: &Failure, code: &str, retryable: bool, error: &AdapterError) {
    assert_eq!(failure.code, code);
    assert_eq!(failure.retryable, retryable, "{code}");
    assert_eq!(
        serde_json::to_value(&failure.error).unwrap(),
        serde_json::to_value(error).unwrap(),
        "{code}"
    );
    registered(code, retryable);
}

fn invalid(code: &str) -> AdapterError {
    AdapterError::InvalidRequest {
        code: code.to_owned(),
    }
}

async fn start_failure(h: &Harness) -> Failure {
    h.adapter
        .start_checked(&ctx(), free_text("Do the work."))
        .await
        .unwrap_err()
}

fn build_error(profile: ProgramProfile, root: std::path::PathBuf) -> CopilotAdapterError {
    let h = Harness::new();
    CopilotLocalAdapter::build(
        config(root, profile),
        services(&h),
        Host {
            timings: TEST_TIMINGS,
            parent_env: std::sync::Arc::new(|_: &str| None),
        },
    )
    .unwrap_err()
}

#[test]
fn row_constructed_for_the_agent_profile() {
    let error = build_error(ProgramProfile::AgentProfile, std::env::temp_dir());
    assert!(matches!(error, CopilotAdapterError::ProfileNotOffered));
    assert!(!error.retryable());
    registered(error.code(), false);
}

#[test]
fn row_constructed_for_the_personal_server_while_c48_is_unconfirmed() {
    let error = build_error(ProgramProfile::PersonalServer, std::env::temp_dir());
    assert!(matches!(
        error,
        CopilotAdapterError::ServerSignInUnconfirmed
    ));
    registered(error.code(), false);
}

#[test]
fn row_configuration_path_not_absolute() {
    let error = build_error(ProgramProfile::Desktop, "relative/root".into());
    assert!(matches!(
        error,
        CopilotAdapterError::ConfigInvalid("app_data_root")
    ));
    registered(error.code(), false);
}

#[tokio::test]
async fn row_a_probe_call_cannot_reach_or_start_the_cli_server() {
    let h = Harness::new();
    h.script().start_fails.store(true, Ordering::SeqCst);
    let error = h.adapter.probe_repair().await.unwrap_err();
    assert!(matches!(
        error,
        CopilotAdapterError::PortFailed {
            code: "CPA_PROBE_FAILED",
            retryable: true
        }
    ));
    h.script().start_fails.store(false, Ordering::SeqCst);
    h.script().auth_fails.store(true, Ordering::SeqCst);
    assert_eq!(
        h.adapter.probe_sign_in().await.unwrap_err().code(),
        "CPA_PROBE_FAILED"
    );
    registered("CPA_PROBE_FAILED", true);
}

#[tokio::test]
async fn row_target_is_a_remote_agent() {
    let h = Harness::new();
    let mut req = free_text("Do the work.");
    req.target = RunTarget::RemoteAgent {
        remote_agent_id: RemoteAgentId::new(),
    };
    let failure = h.adapter.start_checked(&ctx(), req).await.unwrap_err();
    assert_failure(
        &failure,
        "CPA_TARGET_MISMATCH",
        false,
        &invalid("RA_TARGET_MISMATCH"),
    );
}

#[tokio::test]
async fn row_input_outside_2c_limits() {
    let h = Harness::new();
    let long = request(StartInput::FreeText {
        text: "x".repeat(16_385),
    });
    let failure = h.adapter.start_checked(&ctx(), long).await.unwrap_err();
    assert_failure(
        &failure,
        "CPA_INPUT_INVALID",
        false,
        &invalid("RA_INPUT_TOO_LARGE"),
    );
    let unnamed = request(StartInput::Command {
        name: String::new(),
        arguments: String::new(),
    });
    let failure = h.adapter.start_checked(&ctx(), unnamed).await.unwrap_err();
    assert_failure(
        &failure,
        "CPA_INPUT_INVALID",
        false,
        &invalid("RA_COMMAND_UNKNOWN"),
    );
}

#[tokio::test]
async fn row_run_already_held() {
    let h = Harness::new();
    let req = free_text("Do the work.");
    let again = req.clone();
    h.start(req).await.expect("start");
    let failure = h.adapter.start_checked(&ctx(), again).await.unwrap_err();
    assert_failure(
        &failure,
        "CPA_SESSION_EXISTS",
        false,
        &invalid("RA_HANDLE_INVALID"),
    );
}

#[tokio::test]
async fn row_session_setup_source_fails() {
    let h = Harness::new();
    *h.setup.fail.lock().unwrap() = Some("SUP_UNKNOWN".into());
    assert_failure(
        &start_failure(&h).await,
        "CPA_SETUP_FAILED",
        true,
        &AdapterError::Unreachable,
    );
}

#[tokio::test]
async fn row_agent_folder_missing() {
    let h = Harness::new();
    h.setup.setup.lock().unwrap().agent_folder = h.dir.0.join("gone");
    assert_failure(
        &start_failure(&h).await,
        "CPA_AGENT_FOLDER_MISSING",
        false,
        &AdapterError::Unreachable,
    );
}

// spec: ordin-desktop/components/CopilotLocalAdapter#CPA_HARNESS_UNTESTED
#[tokio::test]
async fn row_cli_version_not_tested() {
    let h = Harness::new();
    h.cli.0.lock().unwrap().as_mut().unwrap().version = "1.0.97".into();
    let found = AdapterError::HarnessUntested {
        found: "1.0.97".into(),
    };
    assert_failure(
        &start_failure(&h).await,
        "CPA_HARNESS_UNTESTED",
        false,
        &found,
    );
    assert!(h.script().started.lock().unwrap().is_empty());
}

#[tokio::test]
async fn row_cli_server_does_not_start() {
    let h = Harness::new();
    h.script().start_fails.store(true, Ordering::SeqCst);
    assert_failure(
        &start_failure(&h).await,
        "CPA_SERVER_START_FAILED",
        true,
        &AdapterError::Unreachable,
    );
}

#[tokio::test]
async fn row_no_copilot_cli_installed_at_start() {
    let h = Harness::new();
    h.cli.0.lock().unwrap().as_mut().unwrap().path = "copilot".into();
    assert_failure(
        &start_failure(&h).await,
        "CPA_SERVER_START_FAILED",
        true,
        &AdapterError::Unreachable,
    );
    assert!(h.script().started.lock().unwrap().is_empty());
}

// spec: ordin-desktop/components/CopilotLocalAdapter#CPA_AUTH_STATUS_UNREADABLE
#[tokio::test]
async fn row_sign_in_status_unreadable() {
    let h = Harness::new();
    h.script().auth_fails.store(true, Ordering::SeqCst);
    assert_failure(
        &start_failure(&h).await,
        "CPA_AUTH_STATUS_UNREADABLE",
        true,
        &AdapterError::Unreachable,
    );
    assert_eq!(
        h.script().stopped.load(Ordering::SeqCst),
        1,
        "the server is stopped"
    );
}

#[tokio::test]
async fn row_signed_out_after_the_silent_github_refresh() {
    let h = Harness::with(ScriptedCopilotPort::default(), |_| {});
    let req = free_text("Do the work.");
    let run = req.run_id.to_string();
    let handle = h.start(req).await.expect("start");
    let events = h.events_until(&handle, |seen| seen.len() >= 2).await;
    assert_eq!(kinds(&events), ["started", "paused"]);
    expect_log(&[&run, "CPA_SIGNED_OUT"]).await;
    registered("CPA_SIGNED_OUT", false);
}

#[tokio::test]
async fn row_rules_engine_ends_the_run_at_start() {
    for outcome in [
        StartOutcome::Ended {
            control: "gate".into(),
            ending: "Nothing to do today.".into(),
        },
        StartOutcome::Replied {
            control: "rule".into(),
            text: "Nothing to do today.".into(),
        },
    ] {
        let h = Harness::new();
        *h.rules.script.lock().unwrap() = RulesScript::Decision(ordin_rules::RunStartDecision {
            outcome,
            ..proceed()
        });
        let handle = h.start(free_text("Do the work.")).await.expect("start");
        let events = h.until_ended(&handle).await;
        assert_eq!(kinds(&events), ["started", "ended"]);
        let RunEvent::Ended { end } = &events[1] else {
            panic!()
        };
        assert_eq!(end.final_message.as_deref(), Some("Nothing to do today."));
        assert_eq!(end_reason(&events).as_deref(), Some("completed"));
        assert!(
            h.script().created.lock().unwrap().is_empty(),
            "no session created"
        );
    }
    registered("CPA_RUN_START_ENDED", false);
}

#[tokio::test]
async fn row_rules_engine_unavailable_at_run_start() {
    let h = Harness::new();
    *h.rules.script.lock().unwrap() = RulesScript::Unavailable;
    let handle = h.start(free_text("Do the work.")).await.expect("start");
    let events = h.until_ended(&handle).await;
    assert_eq!(end_reason(&events).as_deref(), Some("failed"));
    expect_log(&[&handle.run_id.to_string(), "CPA_RULES_UNAVAILABLE"]).await;
}

#[tokio::test]
async fn row_mcp_front_open_session_fails() {
    let h = Harness::new();
    *h.mcp.error.lock().unwrap() = Some(McpError::SocketUnavailable {
        detail: "down".into(),
    });
    assert_failure(
        &start_failure(&h).await,
        "CPA_MCP_OPEN_FAILED",
        true,
        &AdapterError::Unreachable,
    );
}

#[tokio::test]
async fn row_hook_relay_open_session_fails() {
    let h = Harness::new();
    *h.hooks.fail.lock().unwrap() = true;
    let req = free_text("Do the work.");
    let run = req.run_id;
    let failure = h.adapter.start_checked(&ctx(), req).await.unwrap_err();
    assert_failure(
        &failure,
        "CPA_HOOK_OPEN_FAILED",
        true,
        &AdapterError::Unreachable,
    );
    assert!(!h.mcp.is_open(run), "the MCP session is closed first");
}

#[tokio::test]
async fn row_package_server_name_clashes_with_an_mcp_front_server() {
    let h = Harness::new();
    h.setup.setup.lock().unwrap().package_servers = vec![crate::config::PackageMcpServer {
        name: "work".into(),
        command: "/opt/pkg/server".into(),
        args: vec![],
        env: Default::default(),
    }];
    assert_failure(
        &start_failure(&h).await,
        "CPA_SERVER_NAME_CLASH",
        false,
        &AdapterError::Unreachable,
    );
}

#[tokio::test]
async fn row_sdk_session_cannot_be_created() {
    let h = Harness::new();
    h.script().create_fails.store(true, Ordering::SeqCst);
    assert_failure(
        &start_failure(&h).await,
        "CPA_SESSION_CREATE_FAILED",
        true,
        &AdapterError::Unreachable,
    );
}

// spec: ordin-desktop/components/CopilotLocalAdapter#C-05
#[tokio::test]
async fn row_persisted_session_cannot_be_resumed() {
    let h = Harness::new();
    h.setup.setup.lock().unwrap().resume_session_id = Some("sess_old".into());
    h.script().resume_fails.store(true, Ordering::SeqCst);
    let handle = h.start(free_text("Carry on.")).await.expect("start");
    let events = h.until_ended(&handle).await;
    assert_eq!(end_reason(&events).as_deref(), Some("interrupted"));
    expect_log(&[&handle.run_id.to_string(), "CPA_SESSION_NOT_RESUMABLE"]).await;
}

#[tokio::test]
async fn row_first_turn_cannot_be_sent() {
    let h = Harness::new();
    h.script().send_fails.store(true, Ordering::SeqCst);
    assert_failure(
        &start_failure(&h).await,
        "CPA_SEND_FAILED",
        true,
        &AdapterError::Unreachable,
    );
}

#[tokio::test]
async fn row_start_past_the_deadline() {
    let h = Harness::new();
    *h.rules.delay.lock().unwrap() = Duration::from_secs(5);
    let mut ctx = ctx();
    ctx.deadline = std::time::Instant::now() + Duration::from_millis(200);
    let req = free_text("Do the work.");
    let run = req.run_id;
    let failure = h.adapter.start_checked(&ctx, req).await.unwrap_err();
    assert_failure(&failure, "CPA_DEADLINE", true, &AdapterError::Deadline);
    assert_eq!(
        h.script().stopped.load(Ordering::SeqCst),
        1,
        "the server is stopped"
    );
    assert!(
        h.adapter
            .read(
                &super::support::ctx(),
                &ordin_runtime_adapter::RunHandle {
                    run_id: run,
                    adapter: ordin_runtime_adapter::AdapterKind::CopilotLocal,
                    runtime_session_id: None,
                    remote_thread_id: None,
                }
            )
            .await
            .is_err(),
        "the run is not held"
    );
}

#[tokio::test]
async fn row_decision_call_errors_or_exceeds_its_bound() {
    let h = Harness::new();
    *h.hooks.pre_tool_use.lock().unwrap() = Decide::Hang;
    let (_, session) = h.start_live("Do the work.").await;
    let gate = h.script().gates(&session).unwrap().pre_tool_use;
    let answer = gate.decide("shell", serde_json::json!({}), None).await;
    assert_eq!(
        answer,
        GateAnswer::Deny {
            reason: "ordin could not check this action.".into()
        }
    );
    registered("CPA_GATE_FAILED", false);
}

#[tokio::test]
async fn row_allowance_used_up() {
    let h = Harness::new();
    let (handle, session) = h.start_live("Do the work.").await;
    h.script().emit(
        &session,
        CopilotEvent::AllowanceExhausted {
            message: "x".repeat(600),
        },
    );
    let events = h.until_ended(&handle).await;
    let RunEvent::Ended { end } = events.last().unwrap() else {
        panic!()
    };
    assert_eq!(end_reason(&events).as_deref(), Some("failed"));
    assert_eq!(
        end.final_message.as_deref().map(|m| m.chars().count()),
        Some(500)
    );
    registered("CPA_ALLOWANCE_EXHAUSTED", false);
}

#[tokio::test]
async fn row_sdk_session_error() {
    let h = Harness::new();
    let (handle, session) = h.start_live("Do the work.").await;
    h.script().emit(
        &session,
        CopilotEvent::Error {
            message: "secret detail".into(),
        },
    );
    let events = h.until_ended(&handle).await;
    assert_eq!(end_reason(&events).as_deref(), Some("failed"));
    let RunEvent::Ended { end } = events.last().unwrap() else {
        panic!()
    };
    assert!(
        end.final_message.is_none(),
        "the message goes to Diagnostics only"
    );
    expect_log(&[&handle.run_id.to_string(), "CPA_SESSION_ERROR"]).await;
}

#[tokio::test]
async fn row_cli_server_exits_before_the_run_ended() {
    let h = Harness::new();
    let (handle, session) = h.start_live("Do the work.").await;
    h.script()
        .emit(&session, CopilotEvent::ServerExited { status: None });
    let events = h.until_ended(&handle).await;
    assert_eq!(end_reason(&events).as_deref(), Some("failed"));
    expect_log(&[&handle.run_id.to_string(), "CPA_SERVER_EXITED"]).await;
}

#[tokio::test]
async fn row_verb_on_an_unknown_run() {
    let h = Harness::new();
    let mut handle = h.start(free_text("Do the work.")).await.expect("start");
    handle.run_id = RunId::new();
    assert!(matches!(
        h.adapter.read(&ctx(), &handle).await,
        Err(AdapterError::UnknownRun)
    ));
    assert!(matches!(
        h.adapter.cancel(&ctx(), &handle).await,
        Err(AdapterError::UnknownRun)
    ));
    assert!(matches!(
        h.adapter.events(&ctx(), &handle),
        Err(AdapterError::UnknownRun)
    ));
    let decision = ResumeDecision::Answer { text: "yes".into() };
    let failure = h
        .adapter
        .resume_checked(&handle, decision)
        .await
        .unwrap_err();
    assert_failure(
        &failure,
        "CPA_UNKNOWN_RUN",
        false,
        &AdapterError::UnknownRun,
    );
}

#[tokio::test]
async fn row_resume_of_a_run_not_paused() {
    let h = Harness::new();
    let (handle, _) = h.start_live("Do the work.").await;
    let decision = ResumeDecision::Approve {
        approval_id: ApprovalId::new(),
    };
    let failure = h
        .adapter
        .resume_checked(&handle, decision)
        .await
        .unwrap_err();
    assert_failure(
        &failure,
        "CPA_NOT_PAUSED",
        false,
        &invalid("RA_RESUME_MISMATCH"),
    );
}

#[tokio::test]
async fn row_resume_with_another_approval_or_a_wrong_decision_kind() {
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
    h.events_until(&handle, |seen| {
        seen.iter().any(|e| matches!(e, RunEvent::Paused { .. }))
    })
    .await;
    let other = ResumeDecision::Approve {
        approval_id: ApprovalId::new(),
    };
    let failure = h.adapter.resume_checked(&handle, other).await.unwrap_err();
    assert_failure(
        &failure,
        "CPA_RESUME_MISMATCH",
        false,
        &invalid("RA_RESUME_MISMATCH"),
    );
    let answer = ResumeDecision::Answer { text: "yes".into() };
    let failure = h.adapter.resume_checked(&handle, answer).await.unwrap_err();
    assert_eq!(failure.code, "CPA_RESUME_MISMATCH");
}

#[tokio::test]
async fn row_cli_server_not_stopped_within_the_deadline() {
    let h = Harness::new();
    *h.script().stop_takes.lock().unwrap() = Duration::from_secs(5);
    let (handle, _) = h.start_live("Do the work.").await;
    h.adapter.cancel(&ctx(), &handle).await.expect("cancel");
    expect_log(&[
        &handle.run_id.to_string(),
        "copilot.stop.deadline",
        "CPA_STOP_DEADLINE",
    ])
    .await;
    registered("CPA_STOP_DEADLINE", false);
}

#[test]
fn every_code_is_in_the_registry_and_owned_here() {
    for code in crate::ALL_CODES {
        let entry = lookup(code).unwrap_or_else(|| panic!("{code}"));
        assert_eq!(entry.owner_crate, "ordin-copilot-adapter");
    }
}
