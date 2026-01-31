//! One test per row of the Error Table (X1): each provokes the condition and asserts the code, the
//! retryability and the 2C result, or the run event and the logged code.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use ordin_hook_relay::HookSessionEvent;
use ordin_mcp_front::{McpError, ProgramProfile};
use ordin_rules::StartOutcome;
use ordin_runtime_adapter::{
    AdapterError, PauseReason, ResumeDecision, RunEndReason, RunEvent, RunTarget, RuntimeAdapter,
    StartInput,
};
use ordin_types::error_registry::lookup;
use ordin_types::{ApprovalId, RemoteAgentId};

use super::support::*;
use crate::config::{KeyRoute, PackageMcpServer};
use crate::error::Failure;
use crate::{ClaudeAdapterError, SignInMode};

/// The failure carries `code`, `retryable` and `error`, and the registry knows the code.
fn assert_failure(failure: &Failure, code: &str, retryable: bool, error: &str) {
    assert_eq!(failure.code, code);
    assert_eq!(failure.retryable, retryable, "retryability of {code}");
    assert!(lookup(code).is_some(), "{code} is registered");
    assert_eq!(format!("{:?}", failure.error), error);
}

const UNREACHABLE: &str = "Unreachable";

fn invalid(code: &str) -> String {
    format!("InvalidRequest {{ code: {code:?} }}")
}

async fn start_failure(h: &Harness) -> Failure {
    let adapter = h.adapter();
    adapter
        .start_checked(&ctx(), command_request())
        .await
        .unwrap_err()
}

#[test]
fn row_configuration_path_not_absolute_profile_and_sign_in_inconsistent_empty_key() {
    let h = Harness::new();
    let mut relative = h.fake.config();
    relative.claude_path = PathBuf::from("claude");
    let mut hook = h.fake.config();
    hook.hook_program = PathBuf::from("bin/ordin-hook");
    let mut root = h.fake.config();
    root.app_data_root = PathBuf::from("data");
    let mut key_on_desktop = h
        .fake
        .agent_config(models(Some("k")), KeyRoute::Environment);
    key_on_desktop.profile = ProgramProfile::Desktop;
    let mut person_in_agent = h.fake.config();
    person_in_agent.profile = ProgramProfile::AgentProfile;
    for (config, field) in [
        (relative, "claude_path"),
        (hook, "hook_program"),
        (root, "app_data_root"),
        (key_on_desktop, "sign_in"),
        (person_in_agent, "sign_in"),
        (
            h.fake.agent_config(models(Some("")), KeyRoute::Environment),
            "models.key",
        ),
    ] {
        let error = h.build(config).unwrap_err();
        assert_eq!(error.code(), "CLA_CONFIG_INVALID");
        assert!(!error.retryable());
        assert!(matches!(error, ClaudeAdapterError::ConfigInvalid(f) if f == field));
    }
    // The record store is required in the agent profile.
    let error = crate::ClaudeLocalAdapter::build(
        h.fake
            .agent_config(models(Some("k")), KeyRoute::Environment),
        h.mcp.clone(),
        h.hooks.clone(),
        h.rules.clone(),
        h.setup.clone(),
        None,
        h.host(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        ClaudeAdapterError::ConfigInvalid("records")
    ));
}

#[test]
fn row_a_models_environment_name_is_reserved() {
    let h = Harness::new();
    for name in ["CLAUDE_CONFIG_DIR", "path", "ORDIN_ANYTHING"] {
        let mut settings = models(Some("k"));
        settings.environment = vec![declared(name, "secret-value")];
        let error = h
            .build(h.fake.agent_config(settings, KeyRoute::Environment))
            .unwrap_err();
        assert_eq!(error.code(), "CLA_ENV_RESERVED");
        assert!(!error.retryable());
        assert!(matches!(&error, ClaudeAdapterError::EnvReserved(n) if n == name));
        assert!(!error.to_string().contains("secret-value"));
    }
}

#[tokio::test]
async fn row_start_target_is_a_remote_agent() {
    let h = Harness::new();
    let adapter = h.adapter();
    let mut req = command_request();
    req.target = RunTarget::RemoteAgent {
        remote_agent_id: RemoteAgentId::new(),
    };
    let failure = adapter.start_checked(&ctx(), req).await.unwrap_err();
    assert_failure(
        &failure,
        "CLA_TARGET_MISMATCH",
        false,
        &invalid("RA_TARGET_MISMATCH"),
    );
}

#[tokio::test]
async fn row_command_name_arguments_or_text_outside_2c_limits() {
    let h = Harness::new();
    let adapter = h.adapter();
    let cases = [
        (
            StartInput::Command {
                name: "x".repeat(65),
                arguments: String::new(),
            },
            "RA_COMMAND_UNKNOWN",
        ),
        (
            StartInput::Command {
                name: "go".to_owned(),
                arguments: "x".repeat(16_385),
            },
            "RA_INPUT_TOO_LARGE",
        ),
        (
            StartInput::FreeText {
                text: String::new(),
            },
            "RA_INPUT_TOO_LARGE",
        ),
        (
            StartInput::FreeText {
                text: "x".repeat(16_385),
            },
            "RA_INPUT_TOO_LARGE",
        ),
    ];
    for (input, code) in cases {
        let mut req = command_request();
        req.input = input;
        let failure = adapter.start_checked(&ctx(), req).await.unwrap_err();
        assert_failure(&failure, "CLA_INPUT_INVALID", false, &invalid(code));
    }
}

#[tokio::test]
async fn row_start_for_a_run_already_held() {
    let h = Harness::new();
    h.fake.script("session.sh", "next_turn");
    let adapter = h.adapter();
    let req = command_request();
    let handle = adapter.start(&ctx(), req.clone()).await.unwrap();
    let failure = adapter.start_checked(&ctx(), req).await.unwrap_err();
    assert_failure(
        &failure,
        "CLA_SESSION_EXISTS",
        false,
        &invalid("RA_HANDLE_INVALID"),
    );
    adapter.cancel(&ctx(), &handle).await.unwrap();
}

#[tokio::test]
async fn row_session_setup_source_fails() {
    let h = Harness::new();
    *h.setup.fail.lock().unwrap() = true;
    let req = command_request();
    let run = req.run_id;
    let failure = h.adapter().start(&ctx(), req).await.unwrap_err();
    assert!(matches!(failure, AdapterError::Unreachable));
    assert_failure(
        &start_failure(&h).await,
        "CLA_SETUP_FAILED",
        true,
        UNREACHABLE,
    );
    expect_log(run, "SUP_RUN_NOT_FOUND").await;
}

#[tokio::test]
async fn row_sign_in_status_times_out_after_10_s_or_prints_no_json() {
    let mut h = Harness::new();
    h.timings.cli_check = Duration::from_millis(100);
    h.fake.script("auth.sh", "sleep 5");
    let error = h.adapter().sign_in_status().await.unwrap_err();
    assert_eq!(error.code(), "CLA_SIGN_IN_STATUS_FAILED");
    assert!(error.retryable());

    let h = Harness::new();
    h.fake.script("auth.sh", "say 'not json'");
    let error = h.adapter().sign_in_status().await.unwrap_err();
    assert!(matches!(error, ClaudeAdapterError::SignInStatusFailed));
}

#[tokio::test]
async fn row_sign_in_status_called_with_organisation_key() {
    let h = Harness::new();
    let adapter = h.agent_adapter(models(Some("k")), KeyRoute::Environment);
    let error = adapter.sign_in_status().await.unwrap_err();
    assert_eq!(error.code(), "CLA_CONFIG_INVALID");
    assert!(!error.retryable());
}

#[tokio::test]
async fn row_agent_folder_missing() {
    let h = Harness::new();
    h.edit_setup(|setup| setup.agent_folder = PathBuf::from("/nonexistent/agent"));
    assert_failure(
        &start_failure(&h).await,
        "CLA_AGENT_FOLDER_MISSING",
        false,
        UNREACHABLE,
    );
}

#[tokio::test]
async fn row_claude_executable_not_found() {
    let h = Harness::new();
    let mut config = h.fake.config();
    config.claude_path = h.fake.root().join("no-such-claude");
    let adapter = h.build(config).unwrap();
    let failure = adapter
        .start_checked(&ctx(), command_request())
        .await
        .unwrap_err();
    assert_failure(&failure, "CLA_HARNESS_NOT_FOUND", false, UNREACHABLE);
}

#[tokio::test]
async fn row_claude_version_times_out_or_is_unparsable() {
    let mut h = Harness::new();
    h.timings.cli_check = Duration::from_millis(100);
    h.fake.script("version.sh", "sleep 5");
    assert_failure(
        &start_failure(&h).await,
        "CLA_VERSION_CHECK_FAILED",
        true,
        UNREACHABLE,
    );
    let h = Harness::new();
    h.fake.script("version.sh", "say 'something else'");
    assert_failure(
        &start_failure(&h).await,
        "CLA_VERSION_CHECK_FAILED",
        true,
        UNREACHABLE,
    );
}

#[tokio::test]
async fn row_version_not_in_the_tested_list() {
    let h = Harness::new();
    h.fake.script("version.sh", "say '2.1.299 (Claude Code)'");
    assert_failure(
        &start_failure(&h).await,
        "CLA_HARNESS_UNTESTED",
        false,
        r#"HarnessUntested { found: "2.1.299" }"#,
    );
}

#[tokio::test]
async fn row_claude_auth_status_unreadable() {
    let h = Harness::new();
    h.fake.script("auth.sh", "say 'garbage'");
    assert_failure(
        &start_failure(&h).await,
        "CLA_AUTH_STATUS_UNREADABLE",
        true,
        UNREACHABLE,
    );
}

#[tokio::test]
async fn row_harness_signed_out_at_start() {
    let h = Harness::new();
    h.fake
        .script("auth.sh", r#"say '{"loggedIn":false}'; exit 1"#);
    h.fake.script("login.sh", "sleep 30");
    let adapter = h.adapter();
    let req = command_request();
    let run = req.run_id;
    let handle = adapter.start(&ctx(), req).await.unwrap();
    let paused = wait_event(&adapter, &handle, |e| matches!(e, RunEvent::Paused { .. })).await;
    assert!(matches!(
        paused,
        RunEvent::Paused {
            reason: PauseReason::SignIn
        }
    ));
    expect_log(run, "CLA_SIGNED_OUT").await;
    adapter.cancel(&ctx(), &handle).await.unwrap();
}

#[tokio::test]
async fn row_claude_auth_login_exits_non_zero() {
    let h = Harness::new();
    h.fake.script("auth.sh", r#"say '{"loggedIn":false}'"#);
    h.fake.script("login.sh", "exit 3");
    let adapter = h.adapter();
    let req = command_request();
    let run = req.run_id;
    let handle = adapter.start(&ctx(), req).await.unwrap();
    expect_log(run, "CLA_SIGN_IN_FAILED").await;
    assert!(adapter.read(&ctx(), &handle).await.is_ok());
    let snapshot = adapter.read(&ctx(), &handle).await.unwrap();
    assert_eq!(snapshot.state, ordin_runtime_adapter::RunState::Paused);
    adapter.cancel(&ctx(), &handle).await.unwrap();
    let events = all_events(&adapter, &handle).await;
    assert_eq!(kinds(&events), ["started", "paused", "ended"]);
}

#[tokio::test]
async fn row_rules_engine_ends_the_run_at_start() {
    for (outcome, text) in [
        (
            StartOutcome::Ended {
                control: "duty".to_owned(),
                ending: "nothing-to-do".to_owned(),
            },
            "nothing-to-do",
        ),
        (
            StartOutcome::Replied {
                control: "rule".to_owned(),
                text: "We are closed.".to_owned(),
            },
            "We are closed.",
        ),
    ] {
        let h = Harness::new();
        let mut decision = proceed();
        decision.outcome = outcome;
        *h.rules.script.lock().unwrap() = RulesScript::Decision(decision);
        let adapter = h.adapter();
        let req = command_request();
        let run = req.run_id;
        let handle = adapter.start(&ctx(), req).await.unwrap();
        let events = all_events(&adapter, &handle).await;
        assert_eq!(kinds(&events), ["started", "ended"]);
        let end = last_end(&events);
        assert_eq!(end.reason, RunEndReason::Completed);
        assert_eq!(end.final_message.as_deref(), Some(text));
        assert_eq!(h.fake.session_count(), 0, "no process started");
        expect_log(run, "CLA_RUN_START_ENDED").await;
    }
}

#[tokio::test]
async fn row_rules_engine_unavailable_at_run_start() {
    let h = Harness::new();
    *h.rules.script.lock().unwrap() = RulesScript::Unavailable;
    let adapter = h.adapter();
    let req = command_request();
    let run = req.run_id;
    let handle = adapter.start(&ctx(), req).await.unwrap();
    let events = all_events(&adapter, &handle).await;
    assert_eq!(last_end(&events).reason, RunEndReason::Failed);
    assert_eq!(h.fake.session_count(), 0, "fail closed");
    expect_log(run, "CLA_RULES_UNAVAILABLE").await;
}

#[tokio::test]
async fn row_mcp_front_open_session_fails() {
    let h = Harness::new();
    *h.mcp.error.lock().unwrap() = Some(McpError::NotStarted);
    assert_failure(
        &start_failure(&h).await,
        "CLA_MCP_OPEN_FAILED",
        true,
        UNREACHABLE,
    );
    *h.mcp.error.lock().unwrap() = Some(McpError::SessionExists {
        run_id: ordin_types::RunId::new(),
    });
    assert_failure(
        &start_failure(&h).await,
        "CLA_MCP_OPEN_FAILED",
        false,
        UNREACHABLE,
    );
}

#[tokio::test]
async fn row_hook_relay_open_session_fails() {
    let h = Harness::new();
    *h.hooks.fail.lock().unwrap() = true;
    let adapter = h.adapter();
    let req = command_request();
    let run = req.run_id;
    let failure = adapter.start_checked(&ctx(), req).await.unwrap_err();
    assert_failure(&failure, "CLA_HOOK_OPEN_FAILED", true, UNREACHABLE);
    assert!(!h.mcp.is_open(run), "the MCP session is closed again");
    assert!(adapter.read(&ctx(), &run_handle(run)).await.is_err());
}

fn run_handle(run: ordin_types::RunId) -> ordin_runtime_adapter::RunHandle {
    ordin_runtime_adapter::RunHandle {
        run_id: run,
        adapter: ordin_runtime_adapter::AdapterKind::ClaudeLocal,
        runtime_session_id: Some("s-1".to_owned()),
        remote_thread_id: None,
    }
}

#[tokio::test]
async fn row_package_server_name_equals_an_mcp_front_server_name() {
    let h = Harness::new();
    h.mcp.servers.lock().unwrap().push("memory");
    h.edit_setup(|setup| {
        setup.package_servers = vec![PackageMcpServer {
            name: "memory".to_owned(),
            command: PathBuf::from("/bin/true"),
            args: vec![],
            env: Default::default(),
        }]
    });
    let adapter = h.adapter();
    let req = command_request();
    let run = req.run_id;
    let failure = adapter.start_checked(&ctx(), req).await.unwrap_err();
    assert_failure(&failure, "CLA_SERVER_NAME_CLASH", false, UNREACHABLE);
    assert!(!h.mcp.is_open(run) && !h.hooks.is_open(run));
    assert!(!h.fake.session_dir(run).exists());
}

#[tokio::test]
async fn row_session_file_cannot_be_written() {
    let h = Harness::new();
    let mut config = h.fake.config();
    // `run` is a file, so the session directory cannot be made.
    std::fs::write(h.fake.data_root.join("run"), "").unwrap();
    config.tested_versions = vec![];
    let adapter = h.build(config).unwrap();
    let failure = adapter
        .start_checked(&ctx(), command_request())
        .await
        .unwrap_err();
    assert_failure(&failure, "CLA_CONFIG_WRITE_FAILED", true, UNREACHABLE);
}

#[tokio::test]
async fn row_child_cannot_be_spawned() {
    let h = Harness::new();
    // The folder goes between the check of step 2 and the spawn of step 10.
    h.fake
        .script("auth.sh", r#"rmdir "$D/agent"; say '{"loggedIn":true}'"#);
    assert_failure(
        &start_failure(&h).await,
        "CLA_SPAWN_FAILED",
        true,
        UNREACHABLE,
    );
}

#[tokio::test]
async fn row_start_exceeds_ctx_deadline() {
    let h = Harness::new();
    *h.setup.delay.lock().unwrap() = Some(Duration::from_millis(400));
    let adapter = h.adapter();
    let mut context = ctx();
    context.deadline = Instant::now() + Duration::from_millis(50);
    let req = command_request();
    let run = req.run_id;
    let failure = adapter.start_checked(&context, req).await.unwrap_err();
    assert_failure(&failure, "CLA_DEADLINE", true, "Deadline");
    assert!(!h.mcp.is_open(run));
}

#[tokio::test]
async fn row_stdout_line_not_json_20_in_a_row_ends_the_run() {
    let h = Harness::new();
    h.fake.script(
        "session.sh",
        "i=0; while [ $i -lt 20 ]; do say 'not json'; i=$((i + 1)); done\nnext_turn",
    );
    let adapter = h.adapter();
    let req = command_request();
    let run = req.run_id;
    let handle = adapter.start(&ctx(), req).await.unwrap();
    let events = all_events(&adapter, &handle).await;
    assert_eq!(last_end(&events).reason, RunEndReason::Failed);
    expect_log(run, "CLA_STREAM_MALFORMED").await;
    assert!(
        log_lines(&["not json"]).is_empty(),
        "stdout is never logged"
    );
}

#[tokio::test]
async fn row_stdout_line_above_4_mib() {
    let h = Harness::new();
    h.fake.script(
        "session.sh",
        &format!("head -c 4194305 /dev/zero | tr '\\0' 'x'; echo\nsay '{RESULT_OK}'\nnext_turn"),
    );
    let adapter = h.adapter();
    let req = command_request();
    let run = req.run_id;
    let handle = adapter.start(&ctx(), req).await.unwrap();
    let events = all_events(&adapter, &handle).await;
    assert_eq!(last_end(&events).reason, RunEndReason::Completed);
    expect_log(run, "CLA_STREAM_LINE_TOO_LONG").await;
}

#[tokio::test]
async fn row_result_with_is_error_after_an_allowance_notice() {
    let h = Harness::new();
    let long = "x".repeat(600);
    h.fake.script(
        "session.sh",
        &format!(
            r#"say '{{"type":"rate_limit_event","rate_limit_info":{{"status":"rejected"}}}}'
say '{{"type":"result","is_error":true,"result":"{long}"}}'
next_turn"#
        ),
    );
    let adapter = h.adapter();
    let req = command_request();
    let run = req.run_id;
    let handle = adapter.start(&ctx(), req).await.unwrap();
    let events = all_events(&adapter, &handle).await;
    let end = last_end(&events);
    assert_eq!(end.reason, RunEndReason::Failed);
    assert_eq!(end.final_message.unwrap().chars().count(), 500);
    expect_log(run, "CLA_ALLOWANCE_EXHAUSTED").await;
}

#[tokio::test]
async fn row_result_with_is_error_otherwise() {
    let h = Harness::new();
    h.fake.script(
        "session.sh",
        r#"say '{"type":"rate_limit_event","rate_limit_info":{"status":"allowed"}}'
say '{"type":"result","is_error":true,"result":"it broke"}'
next_turn"#,
    );
    let adapter = h.adapter();
    let req = command_request();
    let run = req.run_id;
    let handle = adapter.start(&ctx(), req).await.unwrap();
    let events = all_events(&adapter, &handle).await;
    let end = last_end(&events);
    assert_eq!(end.reason, RunEndReason::Failed);
    assert_eq!(end.final_message, None);
    expect_log(run, "CLA_TURN_FAILED").await;
}

#[tokio::test]
async fn row_child_exits_before_the_run_ended() {
    for status in [0, 7] {
        let h = Harness::new();
        h.fake.script(
            "session.sh",
            &format!("echo 'some harness diagnostic' >&2\nexit {status}"),
        );
        let adapter = h.adapter();
        let req = command_request();
        let run = req.run_id;
        let handle = adapter.start(&ctx(), req).await.unwrap();
        let events = all_events(&adapter, &handle).await;
        assert_eq!(last_end(&events).reason, RunEndReason::Failed);
        expect_log(run, "CLA_HARNESS_EXITED").await;
        assert!(log_lines(&["some harness diagnostic"]).is_empty());
    }
}

#[tokio::test]
async fn row_verbs_for_an_unknown_run() {
    let h = Harness::new();
    let adapter = h.adapter();
    let handle = run_handle(ordin_types::RunId::new());
    let failure = adapter
        .resume_checked(
            &ctx(),
            &handle,
            ResumeDecision::Answer {
                text: "x".to_owned(),
            },
        )
        .await
        .unwrap_err();
    assert_failure(&failure, "CLA_UNKNOWN_RUN", false, "UnknownRun");
    assert!(matches!(
        adapter.read(&ctx(), &handle).await,
        Err(AdapterError::UnknownRun)
    ));
    assert!(matches!(
        adapter.cancel(&ctx(), &handle).await,
        Err(AdapterError::UnknownRun)
    ));
    assert!(matches!(
        adapter.events(&ctx(), &handle),
        Err(AdapterError::UnknownRun)
    ));
    expect_log(handle.run_id, "CLA_UNKNOWN_RUN").await;
}

/// Gherkin "Edge case — reading a run this process does not hold is not a warning": after a restart
/// the Session Supervisor reads every run the journal shows in progress; the answer is `UnknownRun`,
/// logged at INFO, and no `claude.call.failed` warning is written for it.
// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-LOG-1
#[tokio::test]
async fn reading_a_run_this_process_does_not_hold_is_not_a_warning() {
    let h = Harness::new();
    let adapter = h.adapter();
    let handle = run_handle(ordin_types::RunId::new());
    assert!(matches!(
        adapter.read(&ctx(), &handle).await,
        Err(AdapterError::UnknownRun)
    ));
    let run = handle.run_id.to_string();
    poll("claude.read.unknown_run", || {
        (!log_lines(&[&run, "INFO", "claude.read.unknown_run"]).is_empty()).then_some(())
    })
    .await;
    assert!(log_lines(&[&run, "claude.call.failed"]).is_empty());
}

#[tokio::test]
async fn row_resume_of_a_run_that_is_not_paused() {
    let h = Harness::new();
    h.fake.script("session.sh", "next_turn");
    let adapter = h.adapter();
    let handle = adapter.start(&ctx(), command_request()).await.unwrap();
    let failure = adapter
        .resume_checked(
            &ctx(),
            &handle,
            ResumeDecision::Approve {
                approval_id: ApprovalId::new(),
            },
        )
        .await
        .unwrap_err();
    assert_failure(
        &failure,
        "CLA_NOT_PAUSED",
        false,
        &invalid("RA_RESUME_MISMATCH"),
    );
    adapter.cancel(&ctx(), &handle).await.unwrap();
}

#[tokio::test]
async fn row_resume_with_another_approval_id_or_a_wrong_decision_kind() {
    let h = Harness::new();
    h.fake.script("session.sh", "next_turn");
    let adapter = h.adapter();
    let req = command_request();
    let run = req.run_id;
    let handle = adapter.start(&ctx(), req).await.unwrap();
    let approval = ApprovalId::new();
    h.hooks.send(
        run,
        HookSessionEvent::DeniedAtDeadline {
            approval_id: approval,
            tool: "Write".to_owned(),
        },
    );
    wait_event(&adapter, &handle, |e| matches!(e, RunEvent::Paused { .. })).await;
    for decision in [
        ResumeDecision::Reject {
            approval_id: ApprovalId::new(),
            reason: None,
        },
        ResumeDecision::Answer {
            text: "yes".to_owned(),
        },
    ] {
        let failure = adapter
            .resume_checked(&ctx(), &handle, decision)
            .await
            .unwrap_err();
        assert_failure(
            &failure,
            "CLA_RESUME_MISMATCH",
            false,
            &invalid("RA_RESUME_MISMATCH"),
        );
    }
    adapter.cancel(&ctx(), &handle).await.unwrap();
}

/// Starts an agent-profile run with `resume_session_id` set.
async fn resumed_agent_run(h: &Harness) -> Vec<RunEvent> {
    h.edit_setup(|setup| {
        setup.resume_session_id = Some("0192a1c2-4410-7aa1-9b22-c3d4e5f60718".to_owned())
    });
    let adapter = h.agent_adapter(models(Some("k")), KeyRoute::Environment);
    let handle = adapter.start(&ctx(), command_request()).await.unwrap();
    all_events(&adapter, &handle).await
}

#[tokio::test]
async fn row_agent_profile_no_session_record_for_a_resume_or_a_record_error() {
    let h = Harness::new();
    let events = resumed_agent_run(&h).await;
    assert_eq!(last_end(&events).reason, RunEndReason::Interrupted);
    assert_eq!(h.fake.session_count(), 0, "not started again");

    let h = Harness::new();
    *h.records.fail_load.lock().unwrap() = true;
    let events = resumed_agent_run(&h).await;
    assert_eq!(last_end(&events).reason, RunEndReason::Interrupted);
    assert!(!log_lines(&["CLA_SESSION_RECORD_MISSING"]).is_empty());
    assert!(!log_lines(&["SUP_RUN_NOT_FOUND"]).is_empty());
}

#[tokio::test]
async fn row_agent_profile_session_record_cannot_be_saved_at_a_pause() {
    let h = Harness::new();
    h.fake.script(
        "session.sh",
        &format!(
            "while [ ! -f \"$D/turn-ends\" ]; do sleep 0.02; done\nsay '{RESULT_OK}'\nnext_turn"
        ),
    );
    *h.records.fail_save.lock().unwrap() = true;
    let adapter = h.agent_adapter(models(Some("k")), KeyRoute::Environment);
    let req = command_request();
    let run = req.run_id;
    let handle = adapter.start(&ctx(), req).await.unwrap();
    let session = handle.runtime_session_id.clone().unwrap();
    let folder = crate::session::encoded_folder(&h.fake.agent_folder);
    let dir = h.fake.root().join("projects").join(folder);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{session}.jsonl")), "bytes").unwrap();
    h.hooks.send(
        run,
        HookSessionEvent::DeniedAtDeadline {
            approval_id: ApprovalId::new(),
            tool: "Write".to_owned(),
        },
    );
    wait_event(&adapter, &handle, |e| matches!(e, RunEvent::Paused { .. })).await;
    std::fs::write(h.fake.root().join("turn-ends"), "").unwrap();
    let events = all_events(&adapter, &handle).await;
    assert_eq!(last_end(&events).reason, RunEndReason::Interrupted);
    expect_log(run, "CLA_SESSION_RECORD_SAVE_FAILED").await;
    poll("the sessions close", || (!h.mcp.is_open(run)).then_some(())).await;
}

#[tokio::test]
async fn row_child_did_not_exit_within_the_stop_deadline() {
    let h = Harness::new();
    h.fake.script(
        "session.sh",
        "trap '' TERM\ntouch \"$D/ready\"\nwhile :; do sleep 0.05; done",
    );
    let mut config = h.fake.config();
    config.stop_deadline = Duration::from_millis(150);
    let adapter = h.build(config).unwrap();
    let req = command_request();
    let run = req.run_id;
    let handle = adapter.start(&ctx(), req).await.unwrap();
    poll("the child runs", || {
        h.fake.root().join("ready").exists().then_some(())
    })
    .await;
    adapter.cancel(&ctx(), &handle).await.unwrap();
    expect_log(run, "CLA_STOP_DEADLINE").await;
    assert_eq!(
        lookup("CLA_STOP_DEADLINE").map(|entry| entry.retryable),
        Some(false)
    );
}

#[test]
fn sign_in_mode_debug_shows_no_secret() {
    let mode = SignInMode::OrganisationKey {
        models: models(Some("sk-hidden")),
        route: KeyRoute::Environment,
    };
    assert!(!format!("{mode:?}").contains("sk-hidden"));
    assert_eq!(format!("{:?}", SignInMode::PersonSignIn), "PersonSignIn");
}
