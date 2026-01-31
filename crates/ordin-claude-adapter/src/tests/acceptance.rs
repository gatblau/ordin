//! One test per Gherkin scenario of ClaudeLocalAdapter.md, named after it (X2). The conformance
//! scenario runs against the real CLI in `tests/harness.rs`. The `Background` is `support`.

use std::time::Instant;

use ordin_hook_relay::HookSessionEvent;
use ordin_runtime_adapter::{
    AdapterError, PauseReason, ResumeDecision, RunEndReason, RunEvent, RunState, RuntimeAdapter,
};
use ordin_types::ApprovalId;

use super::support::*;
use crate::config::{KeyRoute, SignInStatus};
use crate::ClaudeAdapterError;

#[tokio::test]
async fn happy_path_a_command_runs_to_completion() {
    let h = Harness::new();
    h.fake.script("version.sh", "say '2.1.304 (Claude Code)'");
    let adapter = h.adapter();
    let req = command_request();
    let handle = adapter.start(&ctx(), req).await.expect("start");
    let session = handle.runtime_session_id.clone().expect("session id");
    let events = all_events(&adapter, &handle).await;

    assert!(h.fake.has_args(1, &["--restricted"]));
    assert!(h.fake.has_args(1, &["--strict-mcp-config"]));
    assert!(h.fake.has_args(1, &["--permission-prompts", "none"]));
    assert!(h.fake.has_args(1, &["--session-id", &session]));
    assert!(!h.fake.args(1).iter().any(|a| a == "--bare"));
    assert_eq!(
        std::fs::read_to_string(h.fake.root().join("cwd.1"))
            .unwrap()
            .trim(),
        h.fake.agent_folder.to_string_lossy()
    );
    assert_eq!(
        h.fake.stdin_lines(1)[0],
        r#"{"type":"user","message":{"role":"user","content":"/supplier-analyst:compare-quotes Kestrel, Acme and Northwind Q3 quotes"}}"#
    );
    assert_eq!(kinds(&events), ["started", "usage", "ended"]);
    let end = last_end(&events);
    assert_eq!(end.reason, RunEndReason::Completed);
    assert_eq!(end.final_message.as_deref(), Some("Kestrel is cheapest"));
    assert!(end.artefacts.is_empty());
}

#[tokio::test]
async fn instructions_and_plugin_from_the_claude_code_installer_are_passed_by_flag_c_61() {
    let h = Harness::new();
    let folder = h.fake.agent_folder.clone();
    std::fs::write(folder.join("instructions.md"), "be good").unwrap();
    std::fs::create_dir(folder.join("plugin")).unwrap();
    let adapter = h.adapter();
    let handle = adapter.start(&ctx(), command_request()).await.unwrap();
    all_events(&adapter, &handle).await;
    let file = folder.join("instructions.md").display().to_string();
    let plugin = folder.join("plugin").display().to_string();
    assert!(h.fake.has_args(1, &["--append-system-prompt-file", &file]));
    assert!(h.fake.has_args(1, &["--plugin-dir", &plugin]));
}

#[tokio::test]
async fn credential_variables_never_reach_the_child_on_the_desktop() {
    let h = Harness::new();
    {
        let mut parent = h.parent.lock().unwrap();
        for name in [
            "ANTHROPIC_API_KEY",
            "CLAUDE_CODE_OAUTH_TOKEN",
            "CLAUDE_CONFIG_DIR",
        ] {
            parent.insert(name.to_owned(), "from-ordin".to_owned());
        }
    }
    let adapter = h.adapter();
    let req = command_request();
    let run = req.run_id;
    let handle = adapter.start(&ctx(), req).await.unwrap();
    all_events(&adapter, &handle).await;
    let env = h.fake.env(1);
    for name in [
        "ANTHROPIC_API_KEY",
        "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDE_CONFIG_DIR",
    ] {
        assert!(!env.contains_key(name), "{name} reached the child");
    }
    assert_eq!(env.get("ORDIN_HOOK_ENDPOINT").unwrap(), "/tmp/hook.sock");
    assert_eq!(env.get("ORDIN_HOOK_SECRET").unwrap(), HOOK_SECRET);
    assert_eq!(env.get("ORDIN_SESSION_ID").unwrap(), &run.to_string());
    assert_eq!(env.get("ORDIN_HOOK_PROTOCOL").unwrap(), "1");
}

#[tokio::test]
async fn edge_signed_out_the_harness_s_own_sign_in_runs_and_the_run_continues() {
    let h = Harness::new();
    h.fake.script(
        "auth.sh",
        r#"c=$(cat "$D/auth-count" 2>/dev/null || echo 0); c=$((c + 1)); echo $c > "$D/auth-count"
if [ "$c" -gt 2 ]; then say '{"loggedIn":true}'; else say '{"loggedIn":false}'; fi"#,
    );
    h.fake.script("login.sh", "exit 0");
    let adapter = h.adapter();
    let handle = adapter.start(&ctx(), command_request()).await.unwrap();
    let events = all_events(&adapter, &handle).await;
    assert_eq!(
        kinds(&events),
        ["started", "paused", "resumed", "usage", "ended"]
    );
    assert!(matches!(
        &events[1],
        RunEvent::Paused {
            reason: PauseReason::SignIn
        }
    ));
    assert!(!h.fake.root().join("login-tty").exists(), "no terminal");
    assert!(
        h.fake.root().join("login").exists(),
        "claude auth login ran"
    );
    assert_eq!(h.fake.session_count(), 1, "the session child was spawned");
}

#[tokio::test]
async fn edge_an_approval_outlives_the_hook_deadline_and_is_decided_later() {
    let h = Harness::new();
    h.fake.script(
        "session.sh",
        &format!("next_turn\nsay '{RESULT_OK}'\nnext_turn"),
    );
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
    let paused = wait_event(&adapter, &handle, |e| matches!(e, RunEvent::Paused { .. })).await;
    assert!(matches!(
        paused,
        RunEvent::Paused { reason: PauseReason::Approval { approval_id } } if approval_id == approval
    ));
    adapter
        .resume(
            &ctx(),
            &handle,
            ResumeDecision::Approve {
                approval_id: approval,
            },
        )
        .await
        .unwrap();
    wait_event(&adapter, &handle, |e| matches!(e, RunEvent::Resumed)).await;
    all_events(&adapter, &handle).await;
    let turn = &h.fake.stdin_lines(1)[1];
    assert!(turn
        .contains("The person approved the action you asked for. Make the same call again now."));
}

#[tokio::test]
async fn error_version_not_tested() {
    let h = Harness::new();
    h.fake.script("version.sh", "say '2.1.299 (Claude Code)'");
    let adapter = h.adapter();
    let req = command_request();
    let run = req.run_id;
    let error = adapter.start(&ctx(), req).await.unwrap_err();
    assert!(matches!(error, AdapterError::HarnessUntested { found } if found == "2.1.299"));
    assert_eq!(h.fake.session_count(), 0);
    assert!(!h.mcp.is_open(run));
}

#[tokio::test]
async fn error_resume_with_the_wrong_approval() {
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
    assert_eq!(failure.code, "CLA_RESUME_MISMATCH");
    assert_eq!(
        adapter.read(&ctx(), &handle).await.unwrap().state,
        RunState::Paused
    );
    adapter.cancel(&ctx(), &handle).await.unwrap();
}

#[tokio::test]
async fn error_the_cli_exits_during_a_turn() {
    let h = Harness::new();
    h.fake.script("session.sh", "exit 1");
    let adapter = h.adapter();
    let req = command_request();
    let run = req.run_id;
    let handle = adapter.start(&ctx(), req).await.unwrap();
    let events = all_events(&adapter, &handle).await;
    assert_eq!(last_end(&events).reason, RunEndReason::Failed);
    poll("the sessions close", || {
        (!h.mcp.is_open(run) && !h.hooks.is_open(run)).then_some(())
    })
    .await;
    assert!(!h.fake.session_dir(run).exists());
}

#[tokio::test]
async fn error_the_plan_allowance_is_used_up() {
    let h = Harness::new();
    h.fake.script(
        "session.sh",
        r#"say '{"type":"rate_limit_event","rate_limit_info":{"status":"rejected"}}'
say '{"type":"result","is_error":true,"result":"You have reached your usage limit."}'
next_turn"#,
    );
    let adapter = h.adapter();
    let handle = adapter.start(&ctx(), command_request()).await.unwrap();
    let events = all_events(&adapter, &handle).await;
    let end = last_end(&events);
    assert_eq!(end.reason, RunEndReason::Failed);
    assert_eq!(
        end.final_message.as_deref(),
        Some("You have reached your usage limit.")
    );
}

#[tokio::test]
async fn cancel_stops_the_child_within_the_deadline() {
    let h = Harness::new();
    h.fake.script(
        "session.sh",
        "trap '' TERM\ntouch \"$D/ready\"\nwhile :; do sleep 0.05; done",
    );
    let deadline = std::time::Duration::from_millis(400);
    let mut config = h.fake.config();
    config.stop_deadline = deadline;
    let adapter = h.build(config).unwrap();
    let handle = adapter.start(&ctx(), command_request()).await.unwrap();
    poll("the child runs", || {
        h.fake.root().join("ready").exists().then_some(())
    })
    .await;
    let started = Instant::now();
    adapter.cancel(&ctx(), &handle).await.unwrap();
    assert!(started.elapsed() < deadline + std::time::Duration::from_secs(2));
    let pid: i32 = std::fs::read_to_string(h.fake.root().join("pid.1"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_err(),
        "the child is gone"
    );
    let events = all_events(&adapter, &handle).await;
    assert_eq!(last_end(&events).reason, RunEndReason::Cancelled);
}

#[tokio::test]
async fn agent_profile_organisation_key_no_sign_in_reading_only() {
    let h = Harness::new();
    let adapter = h.agent_adapter(models(Some("sk-org")), KeyRoute::Environment);
    let handle = adapter.start(&ctx(), command_request()).await.unwrap();
    all_events(&adapter, &handle).await;
    assert!(!h.fake.calls().iter().any(|c| c.starts_with("auth")));
    assert!(h.fake.has_args(1, &["--tools", "Read"]));
    assert!(h.fake.has_args(1, &["--disallowedTools", "Write,Edit"]));
    assert_eq!(h.fake.env(1).get("ANTHROPIC_API_KEY").unwrap(), "sk-org");
    assert!(std::env::var_os("ANTHROPIC_API_KEY").is_none());
    assert!(log_lines(&["sk-org"]).is_empty());
}

#[tokio::test]
async fn agent_profile_a_pause_saves_the_session_record_and_another_instance_resumes() {
    let h = Harness::new();
    // The harness writes its session record where `start` step 9 reads it.
    h.fake.script(
        "session.1.sh",
        &format!(
            r#"sid=""; prev=""; for a in "$@"; do [ "$prev" = "--session-id" ] && sid="$a"; prev="$a"; done
enc=$(pwd -P | sed 's/[^A-Za-z0-9]/-/g')
mkdir -p "$D/projects/$enc"; echo record-bytes > "$D/projects/$enc/$sid.jsonl"
while [ ! -f "$D/turn-ends" ]; do sleep 0.02; done
say '{RESULT_OK}'
next_turn"#
        ),
    );
    h.fake
        .script("session.2.sh", &format!("say '{RESULT_OK}'\nnext_turn"));
    let adapter = h.agent_adapter(models(Some("sk-org")), KeyRoute::Environment);
    let req = command_request();
    let run = req.run_id;
    let handle = adapter.start(&ctx(), req).await.unwrap();
    let session = handle.runtime_session_id.clone().unwrap();
    let approval = ApprovalId::new();
    h.hooks.send(
        run,
        HookSessionEvent::DeniedAtDeadline {
            approval_id: approval,
            tool: "Write".to_owned(),
        },
    );
    wait_event(&adapter, &handle, |e| matches!(e, RunEvent::Paused { .. })).await;
    // The turn ends: the fake prints `result` and reads on until stdin closes.
    std::fs::write(h.fake.root().join("turn-ends"), "").unwrap();
    poll("the record is saved", || {
        h.records.saved.lock().unwrap().get(&session).cloned()
    })
    .await;
    assert_eq!(
        h.records.saved.lock().unwrap().get(&session).unwrap(),
        b"record-bytes\n"
    );
    poll("the sessions close", || {
        (!h.mcp.is_open(run) && !h.hooks.is_open(run)).then_some(())
    })
    .await;
    assert!(!h.fake.session_dir(run).exists());

    // A second instance: no entry for the run here.
    h.edit_setup(|setup| {
        setup.paused = Some(crate::config::PausedRun {
            approval_id: approval,
            caller: ordin_runtime_adapter::ToolCaller::HostedAgent,
        })
    });
    let second = h.agent_adapter(models(Some("sk-org")), KeyRoute::Environment);
    second
        .resume(
            &ctx(),
            &handle,
            ResumeDecision::Approve {
                approval_id: approval,
            },
        )
        .await
        .unwrap();
    let events = all_events(&second, &handle).await;
    assert_eq!(kinds(&events), ["started", "resumed", "usage", "ended"]);
    assert!(h.fake.has_args(2, &["--resume", &session]));
}

#[tokio::test]
async fn personal_server_sign_in_status_read_from_the_cli() {
    let h = Harness::new();
    h.fake
        .script("auth.sh", r#"say '{"loggedIn":false}'; exit 0"#);
    let adapter = h.adapter();
    assert_eq!(
        adapter.sign_in_status().await.unwrap(),
        SignInStatus::SignedOut
    );
    assert_eq!(h.fake.calls(), ["auth status --json"]);
    assert!(
        !h.fake.root().join("projects").exists(),
        "nothing else was read"
    );
}

#[tokio::test]
async fn a_gateway_reaches_claude_code_through_its_environment() {
    let h = Harness::new();
    let mut settings = models(Some("sk-org"));
    settings.gateway_url = Some("https://models.gateway.example.com".to_owned());
    h.parent.lock().unwrap().insert(
        "ANTHROPIC_BASE_URL".to_owned(),
        "https://inherited.example".to_owned(),
    );
    let adapter = h.agent_adapter(settings, KeyRoute::Environment);
    let handle = adapter.start(&ctx(), command_request()).await.unwrap();
    all_events(&adapter, &handle).await;
    let env = h.fake.env(1);
    assert_eq!(
        env.get("ANTHROPIC_BASE_URL").unwrap(),
        "https://models.gateway.example.com"
    );
    let model_variables: Vec<&String> = env
        .keys()
        .filter(|name| name.starts_with("ANTHROPIC_"))
        .collect();
    assert_eq!(model_variables, ["ANTHROPIC_API_KEY", "ANTHROPIC_BASE_URL"]);
}

#[tokio::test]
async fn a_declared_setting_reaches_the_harness_child_only_adr_107() {
    let h = Harness::new();
    let mut settings = models(Some("sk-org"));
    settings.environment = vec![declared("SERVICE_REGION", "eu-declared-1")];
    let adapter = h.agent_adapter(settings, KeyRoute::Environment);
    let handle = adapter.start(&ctx(), command_request()).await.unwrap();
    all_events(&adapter, &handle).await;
    assert_eq!(
        h.fake.env(1).get("SERVICE_REGION").unwrap(),
        "eu-declared-1"
    );
    assert!(std::env::var_os("SERVICE_REGION").is_none());
    assert!(!h.parent.lock().unwrap().contains_key("SERVICE_REGION"));
    assert!(log_lines(&["eu-declared-1"]).is_empty());
}

#[tokio::test]
async fn a_secret_setting_reaches_the_child_and_its_name_alone_is_reported_adr_107() {
    let h = Harness::new();
    let mut settings = models(Some("sk-org"));
    settings.environment = vec![declared("TOKEN", "secret-token-value")];
    let shown = format!(
        "{:?}",
        h.fake.agent_config(settings.clone(), KeyRoute::Environment)
    );
    let adapter = h.agent_adapter(settings, KeyRoute::Environment);
    let handle = adapter.start(&ctx(), command_request()).await.unwrap();
    all_events(&adapter, &handle).await;
    assert_eq!(h.fake.env(1).get("TOKEN").unwrap(), "secret-token-value");
    // The start-up report item is the Session Supervisor's; what reaches it from here is the name.
    assert!(!shown.contains("secret-token-value"));
    let item = format!("{:?}", declared("TOKEN", "secret-token-value"));
    assert!(item.contains("TOKEN") && !item.contains("secret-token-value"));
    assert!(log_lines(&["secret-token-value"]).is_empty());
}

#[tokio::test]
async fn a_provider_other_than_the_default_with_no_key_adr_107() {
    let h = Harness::new();
    let mut settings = models(None);
    settings.environment = vec![declared("SERVICE_REGION", "eu-1")];
    let adapter = h.agent_adapter(settings, KeyRoute::Environment);
    let handle = adapter.start(&ctx(), command_request()).await.unwrap();
    all_events(&adapter, &handle).await;
    let env = h.fake.env(1);
    assert_eq!(env.get("SERVICE_REGION").unwrap(), "eu-1");
    assert!(!env.contains_key("ANTHROPIC_API_KEY"));
}

#[tokio::test]
async fn a_reserved_name_is_refused_at_construction_adr_107() {
    let h = Harness::new();
    let mut settings = models(Some("sk-org"));
    settings.environment = vec![declared("CLAUDE_CONFIG_DIR", "/elsewhere")];
    let error = h
        .build(h.fake.agent_config(settings, KeyRoute::Environment))
        .unwrap_err();
    assert_eq!(error.code(), "CLA_ENV_RESERVED");
    assert!(matches!(error, ClaudeAdapterError::EnvReserved(name) if name == "CLAUDE_CONFIG_DIR"));
}

// Scenario: Happy path — the start request's model reaches the command (CLA-REC-1)
#[tokio::test]
async fn happy_path_the_start_requests_model_reaches_the_command() {
    let h = Harness::new();
    h.fake.script("version.sh", "say '2.1.304 (Claude Code)'");
    h.setup.setup.lock().unwrap().model = None;
    let adapter = h.adapter();
    let mut req = command_request();
    req.model = Some(ordin_types::wire::run::ModelChoice {
        level: ordin_types::wire::run::ModelLevel::Standard,
        model: Some("sonnet".to_owned()),
    });
    let handle = adapter.start(&ctx(), req).await.expect("start");
    let _ = all_events(&adapter, &handle).await;
    assert!(h.fake.has_args(1, &["--model", "sonnet"]));
}

// Scenario: Edge case — the session setup's model wins (CLA-REC-1)
#[tokio::test]
async fn edge_case_the_session_setups_model_wins() {
    let h = Harness::new();
    h.fake.script("version.sh", "say '2.1.304 (Claude Code)'");
    h.setup.setup.lock().unwrap().model = Some("opus".to_owned());
    let adapter = h.adapter();
    let mut req = command_request();
    req.model = Some(ordin_types::wire::run::ModelChoice {
        level: ordin_types::wire::run::ModelLevel::Standard,
        model: Some("sonnet".to_owned()),
    });
    let handle = adapter.start(&ctx(), req).await.expect("start");
    let _ = all_events(&adapter, &handle).await;
    assert!(h.fake.has_args(1, &["--model", "opus"]));
    assert!(!h.fake.has_args(1, &["--model", "sonnet"]));
}

// Scenario: Happy path — the start request's attachments reach the session setup (CLA-REC-2)
#[tokio::test]
async fn happy_path_the_start_requests_attachments_reach_the_session_setup() {
    let h = Harness::new();
    h.fake.script("version.sh", "say '2.1.304 (Claude Code)'");
    let adapter = h.adapter();
    let (a, b) = (
        ordin_types::ArtefactId::new(),
        ordin_types::ArtefactId::new(),
    );
    let mut req = command_request();
    req.attachments = vec![a, b];
    let handle = adapter.start(&ctx(), req).await.expect("start");
    let _ = all_events(&adapter, &handle).await;
    assert_eq!(*h.setup.attachments.lock().unwrap(), vec![vec![a, b]]);
}

// Scenario: Edge case — a run with no attachments (CLA-REC-2)
#[tokio::test]
async fn edge_case_a_run_with_no_attachments() {
    let h = Harness::new();
    h.fake.script("version.sh", "say '2.1.304 (Claude Code)'");
    let adapter = h.adapter();
    let handle = adapter
        .start(&ctx(), command_request())
        .await
        .expect("start");
    let _ = all_events(&adapter, &handle).await;
    assert_eq!(
        *h.setup.attachments.lock().unwrap(),
        vec![Vec::<ordin_types::ArtefactId>::new()]
    );
}
