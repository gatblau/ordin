//! *Internal Logic* the error table and the scenarios do not reach.

use std::time::Duration;

use ordin_hook_relay::HookSessionEvent;
use ordin_rules::AdoptedRun;
use ordin_runtime_adapter::{
    AdapterError, AdapterKind, AdapterVerb, ResumeDecision, RunEndReason, RunEvent, RunState,
    RuntimeAdapter, StartInput, ToolCaller,
};
use ordin_types::domain::agent_profile::RunStarter;
use ordin_types::wire::rules::ToolOrigin;
use ordin_types::{AgentInstanceId, ApprovalId, CrewCharterId, CrewRunId, ScopeId, WorkspaceId};

use super::support::*;
use crate::config::{KeyRoute, PackageMcpServer, PausedRun};
use crate::run::MAX_ENDED_RUNS;

fn tool_used(tool: &str, succeeded: bool) -> HookSessionEvent {
    HookSessionEvent::ToolUsed {
        tool: tool.to_owned(),
        origin: ToolOrigin::BuiltIn,
        succeeded,
    }
}

#[tokio::test]
async fn the_kind_and_verbs_are_those_of_the_table() {
    let h = Harness::new();
    let adapter = h.adapter();
    assert_eq!(adapter.kind(), AdapterKind::ClaudeLocal);
    let none: &[AdapterVerb] = &[];
    assert_eq!(adapter.unsupported_verbs(), none);
    assert!(adapter.harness_models().per_session_model());
    assert!(format!("{adapter:?}").contains("ClaudeLocalAdapter"));
}

// Logging.md LOG-ID-1 and LOG-ID-2: the run's lines hold its identifiers and its end reason as text.
#[tokio::test]
async fn the_run_s_log_lines_hold_its_identifiers_as_text() {
    let h = Harness::new();
    let adapter = h.adapter();
    let req = command_request();
    let run = req.run_id;
    let workspace = req.workspace_id;
    let agent = match &req.target {
        ordin_types::wire::run::RunTarget::AgentInstance { agent_instance_id } => {
            *agent_instance_id
        }
        ordin_types::wire::run::RunTarget::RemoteAgent { .. } => unreachable!("an agent run"),
    };
    let handle = adapter.start(&ctx(), req).await.unwrap();
    let _ = all_events(&adapter, &handle).await;
    let run_text = run.to_string();
    for event in ["claude.start.received", "claude.turn.result"] {
        let line = poll(event, || {
            log_lines(&[&run_text, &format!("\"{event}\"")])
                .into_iter()
                .next()
        })
        .await;
        let fields: serde_json::Value = serde_json::from_str(line.trim()).expect("a JSON line");
        assert_eq!(fields["agent_instance_id"], agent.to_string(), "{line}");
        assert_eq!(fields["workspace_id"], workspace.to_string(), "{line}");
        assert!(!line.contains("<debug>"), "{line}");
    }
    let turn = log_lines(&[&run_text, "\"claude.turn.result\""]).remove(0);
    let fields: serde_json::Value = serde_json::from_str(turn.trim()).expect("a JSON line");
    assert!(
        fields["runtime_session_id"]
            .as_str()
            .is_some_and(|s| !s.is_empty()),
        "{turn}"
    );
}

#[tokio::test]
async fn hook_events_become_progress_and_usage_counts_tool_calls() {
    let h = Harness::new();
    h.fake.script(
        "session.sh",
        &format!(
            "while [ ! -f \"$D/turn-ends\" ]; do sleep 0.02; done\nsay '{RESULT_OK}'\nnext_turn"
        ),
    );
    let adapter = h.adapter();
    let req = command_request();
    let run = req.run_id;
    let handle = adapter.start(&ctx(), req).await.unwrap();
    let approval = ApprovalId::new();
    h.hooks.send(run, tool_used("Read", true));
    h.hooks.send(run, tool_used("TodoWrite", true));
    h.hooks.send(run, tool_used("Write", false));
    h.hooks.send(
        run,
        HookSessionEvent::Holding {
            approval_id: approval,
            tool: "Write".to_owned(),
        },
    );
    h.hooks.send(
        run,
        HookSessionEvent::Decided {
            tool: "Write".to_owned(),
            allowed: true,
        },
    );
    h.hooks.send(run, HookSessionEvent::TurnStopping);
    wait_event(&adapter, &handle, |e| {
        matches!(e, RunEvent::Progress { progress } if progress.line == "Waiting for your approval")
    })
    .await;
    poll("the stopping log", || {
        (!log_lines(&[&run.to_string(), "claude.turn.stopping"]).is_empty()).then_some(())
    })
    .await;
    std::fs::write(h.fake.root().join("turn-ends"), "").unwrap();
    let events = all_events(&adapter, &handle).await;
    let lines: Vec<(String, Option<u64>)> = events
        .iter()
        .filter_map(|event| match event {
            RunEvent::Progress { progress } => Some((
                progress.line.clone(),
                progress.count.as_ref().map(|c| c.done),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        lines,
        [
            ("Read files".to_owned(), Some(1)),
            ("Changed a file (it didn't work)".to_owned(), Some(3)),
            ("Waiting for your approval".to_owned(), None),
        ]
    );
    let usage = events
        .iter()
        .find_map(|event| match event {
            RunEvent::Usage { usage } => Some(*usage),
            _ => None,
        })
        .expect("usage");
    assert_eq!(usage.tool_calls, 3);
    assert_eq!(usage.model_turns, 1);
    assert_eq!(usage.input_tokens, Some(18_234));
    assert_eq!(usage.output_tokens, Some(1_210));
    assert_eq!(usage.cached_input_tokens, Some(15_002));
}

#[tokio::test]
async fn a_pause_is_emitted_once_and_the_turn_s_result_keeps_the_run_paused() {
    let h = Harness::new();
    h.fake.script(
        "session.sh",
        &format!(
            "while [ ! -f \"$D/turn-ends\" ]; do sleep 0.02; done\nsay '{RESULT_OK}'\nnext_turn\nsay '{RESULT_OK}'\nnext_turn"
        ),
    );
    let adapter = h.adapter();
    let req = command_request();
    let run = req.run_id;
    let handle = adapter.start(&ctx(), req).await.unwrap();
    let approval = ApprovalId::new();
    for _ in 0..2 {
        h.hooks.send(
            run,
            HookSessionEvent::DeniedAtDeadline {
                approval_id: approval,
                tool: "Write".to_owned(),
            },
        );
    }
    h.hooks.send(
        run,
        HookSessionEvent::Decided {
            tool: "Write".to_owned(),
            allowed: false,
        },
    );
    wait_event(&adapter, &handle, |e| matches!(e, RunEvent::Paused { .. })).await;
    std::fs::write(h.fake.root().join("turn-ends"), "").unwrap();
    wait_event(&adapter, &handle, |e| matches!(e, RunEvent::Usage { .. })).await;
    assert_eq!(
        adapter.read(&ctx(), &handle).await.unwrap().state,
        RunState::Paused
    );
    adapter
        .resume(
            &ctx(),
            &handle,
            ResumeDecision::Reject {
                approval_id: approval,
                reason: None,
            },
        )
        .await
        .unwrap();
    let events = all_events(&adapter, &handle).await;
    assert_eq!(
        kinds(&events),
        ["started", "paused", "usage", "resumed", "usage", "ended"]
    );
    assert!(h.fake.stdin_lines(1)[1].contains(
        "The person did not approve the action. Reason: none given. Continue without it."
    ));
}

#[tokio::test]
async fn a_reject_reason_reaches_the_model_and_an_over_long_one_is_refused() {
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
    wait_event(&adapter, &handle, |e| matches!(e, RunEvent::Paused { .. })).await;
    let failure = adapter
        .resume_checked(
            &ctx(),
            &handle,
            ResumeDecision::Reject {
                approval_id: approval,
                reason: Some("x".repeat(501)),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CLA_INPUT_INVALID");
    adapter
        .resume(
            &ctx(),
            &handle,
            ResumeDecision::Reject {
                approval_id: approval,
                reason: Some("too risky".to_owned()),
            },
        )
        .await
        .unwrap();
    all_events(&adapter, &handle).await;
    assert!(h.fake.stdin_lines(1)[1].contains("Reason: too risky. Continue without it."));
}

#[tokio::test]
async fn an_idle_paused_process_ends_and_a_decision_restarts_it_with_resume() {
    let mut h = Harness::new();
    h.timings.idle_process = Duration::from_millis(100);
    h.fake.script("session.1.sh", "next_turn");
    h.fake
        .script("session.2.sh", &format!("say '{RESULT_OK}'\nnext_turn"));
    let adapter = h.adapter();
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
    poll("the sessions close", || {
        (!h.mcp.is_open(run) && h.mcp.opened.lock().unwrap().len() == 1).then_some(())
    })
    .await;
    assert!(!h.fake.session_dir(run).exists());
    assert_eq!(
        adapter.read(&ctx(), &handle).await.unwrap().state,
        RunState::Paused
    );
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
    let events = all_events(&adapter, &handle).await;
    assert_eq!(last_end(&events).reason, RunEndReason::Completed);
    assert_eq!(
        kinds(&events),
        ["started", "paused", "resumed", "usage", "ended"]
    );
    assert!(h.fake.has_args(2, &["--resume", &session]));
}

#[tokio::test]
async fn a_restart_that_fails_leaves_the_run_paused_and_cancel_ends_it() {
    let mut h = Harness::new();
    h.timings.idle_process = Duration::from_millis(50);
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
    poll("the sessions close", || (!h.mcp.is_open(run)).then_some(())).await;
    *h.hooks.fail.lock().unwrap() = true;
    let failure = adapter
        .resume_checked(
            &ctx(),
            &handle,
            ResumeDecision::Approve {
                approval_id: approval,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CLA_HOOK_OPEN_FAILED");
    assert_eq!(
        adapter.read(&ctx(), &handle).await.unwrap().state,
        RunState::Paused
    );
    assert!(!h.mcp.is_open(run));
    adapter.cancel(&ctx(), &handle).await.unwrap();
    let events = all_events(&adapter, &handle).await;
    assert_eq!(last_end(&events).reason, RunEndReason::Cancelled);
}

#[tokio::test]
async fn sign_in_again_restarts_the_flow_without_a_second_pause() {
    let h = Harness::new();
    h.fake.script(
        "auth.sh",
        r#"if [ -f "$D/signed-in" ]; then say '{"loggedIn":true}'; else say '{"loggedIn":false}'; fi"#,
    );
    h.fake.script("login.sh", "exit 2");
    let adapter = h.adapter();
    let req = command_request();
    let run = req.run_id;
    let handle = adapter.start(&ctx(), req).await.unwrap();
    expect_log(run, "CLA_SIGN_IN_FAILED").await;
    for decision in [ResumeDecision::Approve {
        approval_id: ApprovalId::new(),
    }] {
        let failure = adapter
            .resume_checked(&ctx(), &handle, decision)
            .await
            .unwrap_err();
        assert_eq!(failure.code, "CLA_RESUME_MISMATCH");
    }
    let failure = adapter
        .resume_checked(
            &ctx(),
            &handle,
            ResumeDecision::Answer {
                text: String::new(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CLA_INPUT_INVALID");
    std::fs::write(h.fake.root().join("signed-in"), "").unwrap();
    h.fake.script("login.sh", "exit 0");
    adapter
        .resume(
            &ctx(),
            &handle,
            ResumeDecision::Answer {
                text: "Sign in again".to_owned(),
            },
        )
        .await
        .unwrap();
    let events = all_events(&adapter, &handle).await;
    assert_eq!(
        kinds(&events),
        ["started", "paused", "resumed", "usage", "ended"]
    );
    assert_eq!(
        std::fs::read_to_string(h.fake.root().join("login"))
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[tokio::test]
async fn a_sign_in_that_times_out_stays_paused_and_cancel_ends_it() {
    let mut h = Harness::new();
    h.timings.sign_in_give_up = Duration::from_millis(80);
    h.fake.script("auth.sh", r#"say '{"loggedIn":false}'"#);
    h.fake.script("login.sh", "sleep 20");
    let adapter = h.adapter();
    let handle = adapter.start(&ctx(), command_request()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        adapter.read(&ctx(), &handle).await.unwrap().state,
        RunState::Paused
    );
    adapter.cancel(&ctx(), &handle).await.unwrap();
    let events = all_events(&adapter, &handle).await;
    assert_eq!(kinds(&events), ["started", "paused", "ended"]);
    assert_eq!(last_end(&events).reason, RunEndReason::Cancelled);
}

#[tokio::test]
async fn a_failure_after_sign_in_ends_the_run_failed() {
    let h = Harness::new();
    h.fake.script(
        "auth.sh",
        r#"c=$(cat "$D/auth-count" 2>/dev/null || echo 0); c=$((c + 1)); echo $c > "$D/auth-count"
if [ "$c" -gt 1 ]; then say '{"loggedIn":true}'; else say '{"loggedIn":false}'; fi"#,
    );
    *h.hooks.fail.lock().unwrap() = true;
    let adapter = h.adapter();
    let req = command_request();
    let run = req.run_id;
    let handle = adapter.start(&ctx(), req).await.unwrap();
    let events = all_events(&adapter, &handle).await;
    assert_eq!(last_end(&events).reason, RunEndReason::Failed);
    expect_log(run, "CLA_HOOK_OPEN_FAILED").await;
    assert!(!h.mcp.is_open(run));
}

#[tokio::test]
async fn a_rules_decision_shapes_the_session() {
    let h = Harness::new();
    let mut decision = proceed();
    decision.command = Some("other-command".to_owned());
    decision.tool_subset = Some(vec!["Read".to_owned(), "Grep".to_owned()]);
    *h.rules.script.lock().unwrap() = RulesScript::Decision(decision);
    let adapter = h.adapter();
    let handle = adapter.start(&ctx(), command_request()).await.unwrap();
    all_events(&adapter, &handle).await;
    assert!(h.fake.has_args(1, &["--tools", "Read,Grep"]));
    assert!(h.fake.stdin_lines(1)[0].contains("/supplier-analyst:other-command Kestrel"));
}

#[tokio::test]
async fn single_call_ends_the_run_at_the_first_result_even_when_paused() {
    let h = Harness::new();
    let mut decision = proceed();
    decision.single_call = true;
    *h.rules.script.lock().unwrap() = RulesScript::Decision(decision);
    h.fake.script(
        "session.sh",
        &format!(
            "while [ ! -f \"$D/turn-ends\" ]; do sleep 0.02; done\nsay '{RESULT_OK}'\nnext_turn"
        ),
    );
    let adapter = h.adapter();
    let req = command_request();
    let run = req.run_id;
    let handle = adapter.start(&ctx(), req).await.unwrap();
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
    assert_eq!(last_end(&events).reason, RunEndReason::Completed);
}

#[tokio::test]
async fn limit_seconds_stops_the_run_at_the_limit() {
    let h = Harness::new();
    let mut decision = proceed();
    decision.limit_seconds = Some(0);
    *h.rules.script.lock().unwrap() = RulesScript::Decision(decision);
    h.fake.script("session.sh", "next_turn");
    let adapter = h.adapter();
    let handle = adapter.start(&ctx(), command_request()).await.unwrap();
    let events = all_events(&adapter, &handle).await;
    assert_eq!(last_end(&events).reason, RunEndReason::StoppedAtLimit);
}

#[tokio::test]
async fn the_facts_of_a_person_s_run() {
    let h = Harness::new();
    let adapter = h.adapter();
    let mut req = command_request();
    let scope = ScopeId::new();
    req.scope_id = Some(scope);
    let handle = adapter.start(&ctx(), req.clone()).await.unwrap();
    all_events(&adapter, &handle).await;
    let facts = h.rules.facts.lock().unwrap().pop().unwrap();
    assert_eq!(facts.run_id, req.run_id);
    assert_eq!(facts.command.as_deref(), Some("compare-quotes"));
    assert_eq!(facts.text, "Kestrel, Acme and Northwind Q3 quotes");
    assert_eq!(facts.scope, Some(scope.to_string()));
    assert_eq!(facts.started_by, "person");
    assert_eq!(facts.asker, None);
    let grant = h.mcp.opened.lock().unwrap().pop().unwrap();
    assert_eq!(grant.caller, ToolCaller::Agent);
    assert_eq!(grant.scope_id, Some(scope));
    assert_eq!(grant.profile, ordin_mcp_front::ProgramProfile::Desktop);
}

#[tokio::test]
async fn the_facts_of_a_crew_member_and_free_text() {
    let h = Harness::new();
    let charter = CrewCharterId::new();
    h.edit_setup(|setup| setup.crew_charter_id = Some(charter));
    let adapter = h.adapter();
    let mut req = command_request();
    let crew = CrewRunId::new();
    req.caller = ToolCaller::CrewMember { crew_run_id: crew };
    req.input = StartInput::FreeText {
        text: "Hello".to_owned(),
    };
    let handle = adapter.start(&ctx(), req).await.unwrap();
    all_events(&adapter, &handle).await;
    let facts = h.rules.facts.lock().unwrap().pop().unwrap();
    assert_eq!(facts.started_by, "crew");
    assert_eq!(facts.started_by_id, crew.to_string());
    assert_eq!(facts.crew_charter_id, Some(charter));
    assert_eq!(facts.command, None);
    assert_eq!(facts.text, "Hello");
}

#[tokio::test]
async fn the_facts_of_each_agent_profile_starter() {
    for (starter, by, id, asker) in [
        (
            RunStarter::Caller {
                caller_id: "c1".to_owned(),
            },
            "caller",
            "c1",
            None,
        ),
        (
            RunStarter::ScheduleEntry {
                entry: "nightly".to_owned(),
            },
            "schedule",
            "nightly",
            None,
        ),
        (
            RunStarter::Intake {
                intake: "form".to_owned(),
            },
            "intake",
            "form",
            None,
        ),
        (
            RunStarter::ChatConnection {
                connection: "slack".to_owned(),
                asker: "U1".to_owned(),
            },
            "chat",
            "U1",
            Some("U1"),
        ),
    ] {
        let h = Harness::new();
        h.edit_setup(|setup| setup.run_starter = Some(starter.clone()));
        let adapter = h.agent_adapter(models(Some("k")), KeyRoute::Environment);
        let handle = adapter.start(&ctx(), command_request()).await.unwrap();
        all_events(&adapter, &handle).await;
        let facts = h.rules.facts.lock().unwrap().pop().unwrap();
        assert_eq!(facts.started_by, by);
        assert_eq!(facts.started_by_id, id);
        assert_eq!(facts.asker.as_deref(), asker);
    }
}

#[tokio::test]
async fn the_helper_route_writes_the_key_and_the_setting() {
    let h = Harness::new();
    h.fake.script(
        "session.sh",
        r#"sed -n p "$(dirname "$(sed -n '/--settings/{n;p;}' "$D/args.1")")/key" > "$D/seen-key"
cp "$(sed -n '/--settings/{n;p;}' "$D/args.1")" "$D/seen-settings"
exit 0"#,
    );
    let mut config = h
        .fake
        .agent_config(models(Some("sk-helper")), KeyRoute::SettingsApiKeyHelper);
    config.stop_deadline = Duration::from_secs(1);
    let adapter = h.build(config).unwrap();
    let req = command_request();
    let run = req.run_id;
    let handle = adapter.start(&ctx(), req).await.unwrap();
    all_events(&adapter, &handle).await;
    assert_eq!(
        std::fs::read_to_string(h.fake.root().join("seen-key"))
            .unwrap()
            .trim(),
        "sk-helper"
    );
    let settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(h.fake.root().join("seen-settings")).unwrap())
            .unwrap();
    assert_eq!(
        settings["apiKeyHelper"],
        format!(
            "{} api-key --run {run} --data-root {}",
            crate::files::shell_word("/opt/ordin/bin/ordin-hook"),
            crate::files::shell_word(&h.fake.data_root.to_string_lossy())
        )
    );
    assert!(!h.fake.env(1).contains_key("ANTHROPIC_API_KEY"));
    assert!(
        !h.fake.session_dir(run).exists(),
        "the key goes with the session"
    );
}

#[tokio::test]
async fn a_harness_plan_is_emitted_where_the_stream_carries_one() {
    let h = Harness::new();
    h.fake.script(
        "session.sh",
        &format!(
            r#"say '{{"type":"system","subtype":"init","tools":["Read"]}}'
say '{{"type":"system","subtype":"init","tools":["Read"]}}'
say '{{"type":"assistant","message":{{"content":[{{"type":"tool_use","name":"TodoWrite","input":{{"todos":[{{"content":"One"}},{{"content":"Two"}},{{"content":"Three"}}]}}}}]}}}}'
say '{{"type":"something_new"}}'
say '{RESULT_OK}'
next_turn"#
        ),
    );
    let adapter = h.adapter();
    let req = command_request();
    let run = req.run_id;
    let handle = adapter.start(&ctx(), req).await.unwrap();
    let events = all_events(&adapter, &handle).await;
    assert_eq!(
        kinds(&events),
        ["started", "harness_plan", "usage", "ended"]
    );
    assert_eq!(
        log_lines(&[&run.to_string(), "claude.session.init"]).len(),
        1,
        "the first init of the process only"
    );
}

#[tokio::test]
async fn shutdown_stops_every_session_and_runs_end_interrupted() {
    let h = Harness::new();
    h.fake
        .script("session.sh", "trap '' TERM\nwhile :; do sleep 0.05; done");
    let mut config = h.fake.config();
    config.stop_deadline = Duration::from_millis(100);
    let adapter = h.build(config).unwrap();
    let live = adapter.start(&ctx(), command_request()).await.unwrap();
    let ended = {
        let mut decision = proceed();
        decision.outcome = ordin_rules::StartOutcome::Ended {
            control: "c".to_owned(),
            ending: "done".to_owned(),
        };
        *h.rules.script.lock().unwrap() = RulesScript::Decision(decision);
        adapter.start(&ctx(), command_request()).await.unwrap()
    };
    let started = std::time::Instant::now();
    adapter.shutdown().await;
    assert!(started.elapsed() < Duration::from_millis(1100) + Duration::from_secs(1));
    let events = all_events(&adapter, &live).await;
    assert_eq!(last_end(&events).reason, RunEndReason::Interrupted);
    assert_eq!(
        adapter.read(&ctx(), &ended).await.unwrap().state,
        RunState::Completed
    );
}

#[tokio::test]
async fn cancel_of_an_ended_run_is_idempotent_and_a_new_start_may_follow() {
    let h = Harness::new();
    h.fake.script("session.sh", "exit 1");
    let adapter = h.adapter();
    let req = command_request();
    let handle = adapter.start(&ctx(), req.clone()).await.unwrap();
    all_events(&adapter, &handle).await;
    adapter.cancel(&ctx(), &handle).await.unwrap();
    // The Session Supervisor starts a failed run again from its persisted session.
    h.fake
        .script("session.sh", &format!("say '{RESULT_OK}'\nnext_turn"));
    h.edit_setup(|setup| {
        setup.resume_session_id = Some(handle.runtime_session_id.clone().unwrap())
    });
    let again = adapter.start(&ctx(), req).await.unwrap();
    assert_eq!(again.runtime_session_id, handle.runtime_session_id);
    let events = all_events(&adapter, &again).await;
    assert_eq!(last_end(&events).reason, RunEndReason::Completed);
    let session = handle.runtime_session_id.unwrap();
    assert!(h.fake.has_args(2, &["--resume", &session]));
}

#[tokio::test]
async fn two_subscribers_each_receive_the_full_sequence_and_read_reports_the_end() {
    let h = Harness::new();
    let adapter = h.adapter();
    let handle = adapter.start(&ctx(), command_request()).await.unwrap();
    let (first, second) =
        tokio::join!(all_events(&adapter, &handle), all_events(&adapter, &handle));
    assert_eq!(kinds(&first), kinds(&second));
    let snapshot = adapter.read(&ctx(), &handle).await.unwrap();
    assert_eq!(snapshot.state, RunState::Completed);
    assert!(snapshot.end.is_some());
}

#[tokio::test]
async fn package_servers_are_merged_and_their_secrets_dropped_when_the_run_ends() {
    let h = Harness::new();
    h.edit_setup(|setup| {
        setup.package_servers = vec![PackageMcpServer {
            name: "tools".to_owned(),
            command: "/bin/true".into(),
            args: vec![],
            env: [("TOKEN".to_owned(), "package-secret".to_owned())].into(),
        }]
    });
    h.fake.script(
        "session.sh",
        r#"mcp="$(sed -n '/--mcp-config/{n;p;}' "$D/args.1")"
cp "$mcp" "$D/seen-mcp"
cp "$(dirname "$mcp")/package-server-0.json" "$D/seen-launch"
say '{"type":"result","is_error":false,"result":"ok"}'
next_turn"#,
    );
    let adapter = h.adapter();
    let handle = adapter.start(&ctx(), command_request()).await.unwrap();
    all_events(&adapter, &handle).await;
    let seen = std::fs::read_to_string(h.fake.root().join("seen-mcp")).unwrap();
    assert!(!seen.contains("package-secret"), "{seen}");
    let launch = std::fs::read_to_string(h.fake.root().join("seen-launch")).unwrap();
    assert!(launch.contains("package-secret"));
    poll("the secrets go", || {
        (adapter.package_server_count(handle.run_id) == Some(0)).then_some(())
    })
    .await;
    assert!(log_lines(&["package-secret"]).is_empty());
}

#[tokio::test]
async fn a_crash_s_session_directories_are_removed_at_construction() {
    let h = Harness::new();
    let left = h.fake.data_root.join("run/sessions/leftover");
    std::fs::create_dir_all(&left).unwrap();
    h.adapter();
    assert!(!left.exists());
}

#[tokio::test]
async fn ended_runs_past_the_limit_are_evicted_oldest_first() {
    let h = Harness::new();
    let adapter = h.adapter();
    let mut handles = Vec::new();
    for _ in 0..MAX_ENDED_RUNS + 3 {
        let handle = adapter.start(&ctx(), command_request()).await.unwrap();
        all_events(&adapter, &handle).await;
        handles.push(handle);
    }
    assert!(adapter.held_runs() <= MAX_ENDED_RUNS + 1);
    let unknown = adapter.read(&ctx(), &handles[0]).await.unwrap_err();
    assert!(matches!(unknown, AdapterError::UnknownRun));
    let last = handles.last().unwrap();
    assert!(adapter.read(&ctx(), last).await.is_ok());
}

const SESSION: &str = "0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff";

/// A run another instance paused on `approval`: its record is here and the program reports it.
fn paused_elsewhere(h: &Harness, approval: ApprovalId) -> ordin_runtime_adapter::RunHandle {
    h.records
        .saved
        .lock()
        .unwrap()
        .insert(SESSION.to_owned(), b"record".to_vec());
    h.edit_setup(|setup| {
        setup.paused = Some(PausedRun {
            approval_id: approval,
            caller: ToolCaller::HostedAgent,
        })
    });
    let mut handle = run_of(&command_request());
    handle.runtime_session_id = Some(SESSION.to_owned());
    handle
}

#[tokio::test]
async fn an_adopted_run_keeps_its_workspace_scope_caller_and_tool_subset() {
    let h = Harness::new();
    let workspace = WorkspaceId::new();
    let scope = ScopeId::new();
    let agent = AgentInstanceId::new();
    *h.rules.adopted.lock().unwrap() = Some(Ok(Some(AdoptedRun {
        agent_instance_id: agent,
        workspace_id: workspace,
        scope: Some(scope.to_string()),
        tool_subset: Some(vec!["Read".to_owned()]),
        single_call: false,
        limit_seconds: None,
    })));
    let approval = ApprovalId::new();
    let handle = paused_elsewhere(&h, approval);
    let crew = CrewRunId::new();
    h.edit_setup(|setup| {
        setup.paused = Some(PausedRun {
            approval_id: approval,
            caller: ToolCaller::CrewMember { crew_run_id: crew },
        })
    });
    h.fake.script(
        "session.sh",
        r#"say '{"type":"result","is_error":false,"result":"ok"}'
next_turn"#,
    );
    let adapter = h.agent_adapter(models(Some("k")), KeyRoute::Environment);
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
    all_events(&adapter, &handle).await;
    let grant = h.mcp.opened.lock().unwrap().last().cloned().unwrap();
    assert_eq!(grant.workspace_id, workspace);
    assert_eq!(grant.scope_id, Some(scope));
    assert_eq!(grant.agent_instance_id, agent);
    assert_eq!(grant.caller, ToolCaller::CrewMember { crew_run_id: crew });
    assert!(h.fake.has_args(1, &["--tools", "Read"]));
    assert!(h.fake.has_args(1, &["--resume", SESSION]));
}

// Scenario: Agent profile — an adopted run keeps its own time limit (SA-RECON-08, amended
// 2026-01-24).
#[tokio::test]
async fn an_adopted_run_keeps_its_own_time_limit_and_has_no_coding_task() {
    let h = Harness::new();
    let agent = AgentInstanceId::new();
    *h.rules.adopted.lock().unwrap() = Some(Ok(Some(AdoptedRun {
        agent_instance_id: agent,
        workspace_id: WorkspaceId::new(),
        scope: None,
        tool_subset: None,
        single_call: false,
        limit_seconds: Some(0),
    })));
    let approval = ApprovalId::new();
    let handle = paused_elsewhere(&h, approval);
    h.fake.script("session.sh", "next_turn");
    let adapter = h.agent_adapter(models(Some("k")), KeyRoute::Environment);
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
    let events = all_events(&adapter, &handle).await;
    assert_eq!(last_end(&events).reason, RunEndReason::StoppedAtLimit);
    let grant = h.mcp.opened.lock().unwrap().last().cloned().unwrap();
    assert_eq!(grant.agent_instance_id, agent);
    assert_eq!(grant.coding_task_id, None);
}

#[tokio::test]
async fn a_run_cannot_be_adopted_without_each_thing_that_shows_it_is_paused_here() {
    let approve = |approval_id| ResumeDecision::Approve { approval_id };
    let approval = ApprovalId::new();

    // No record in the Rules Engine, then the engine unreachable.
    let h = Harness::new();
    let handle = paused_elsewhere(&h, approval);
    let adapter = h.agent_adapter(models(Some("k")), KeyRoute::Environment);
    *h.rules.adopted.lock().unwrap() = Some(Ok(None));
    let failure = adapter
        .resume_checked(&ctx(), &handle, approve(approval))
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CLA_UNKNOWN_RUN");
    *h.rules.adopted.lock().unwrap() = Some(Err(()));
    let failure = adapter
        .resume_checked(&ctx(), &handle, approve(approval))
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CLA_RULES_UNAVAILABLE");
    assert!(failure.retryable);

    // Ended or never paused: the program reports nothing.
    let h = Harness::new();
    let handle = paused_elsewhere(&h, approval);
    h.edit_setup(|setup| setup.paused = None);
    let adapter = h.agent_adapter(models(Some("k")), KeyRoute::Environment);
    let failure = adapter
        .resume_checked(&ctx(), &handle, approve(approval))
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CLA_UNKNOWN_RUN");

    // Another approval than the one the run is paused on: the decision proves nothing.
    let h = Harness::new();
    let handle = paused_elsewhere(&h, approval);
    let adapter = h.agent_adapter(models(Some("k")), KeyRoute::Environment);
    let failure = adapter
        .resume_checked(&ctx(), &handle, approve(ApprovalId::new()))
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CLA_RESUME_MISMATCH");
    assert_eq!(adapter.held_runs(), 0);

    // No session record here, then a store that fails.
    let h = Harness::new();
    let handle = paused_elsewhere(&h, approval);
    h.records.saved.lock().unwrap().clear();
    let adapter = h.agent_adapter(models(Some("k")), KeyRoute::Environment);
    let failure = adapter
        .resume_checked(&ctx(), &handle, approve(approval))
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CLA_UNKNOWN_RUN");
    *h.records.fail_load.lock().unwrap() = true;
    let failure = adapter
        .resume_checked(&ctx(), &handle, approve(approval))
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CLA_SESSION_RECORD_MISSING");
}

#[tokio::test]
async fn a_session_id_from_a_handle_must_be_a_canonical_uuid() {
    let h = Harness::new();
    let approval = ApprovalId::new();
    let mut handle = paused_elsewhere(&h, approval);
    let adapter = h.agent_adapter(models(Some("k")), KeyRoute::Environment);
    for bad in [
        "../../../../etc/x",
        "sid",
        "0192A0F9-AAAA-7BBB-8CCC-DDDDEEEEFFFF",
        "0192a0f9aaaa7bbb8cccddddeeeeffff",
        "{0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff}",
        "urn:uuid:0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff",
    ] {
        // The record store would also say no; this shows the identifier is refused on its own.
        h.records
            .saved
            .lock()
            .unwrap()
            .insert(bad.to_owned(), b"record".to_vec());
        handle.runtime_session_id = Some(bad.to_owned());
        let failure = adapter
            .resume_checked(
                &ctx(),
                &handle,
                ResumeDecision::Approve {
                    approval_id: approval,
                },
            )
            .await
            .unwrap_err();
        assert_eq!(failure.code, "CLA_UNKNOWN_RUN", "{bad}");
    }
}

#[tokio::test]
async fn a_second_instance_refuses_an_answer_and_a_desktop_refuses_an_unknown_run() {
    let h = Harness::new();
    let adapter = h.agent_adapter(models(Some("k")), KeyRoute::Environment);
    let mut handle = run_of(&command_request());
    handle.runtime_session_id = Some(SESSION.to_owned());
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
    assert_eq!(failure.code, "CLA_UNKNOWN_RUN");
    let failure = h
        .adapter()
        .resume_checked(
            &ctx(),
            &handle,
            ResumeDecision::Approve {
                approval_id: ApprovalId::new(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CLA_UNKNOWN_RUN");
}

#[tokio::test]
async fn the_stderr_tail_stays_with_the_run_after_a_failure() {
    let h = Harness::new();
    h.fake.script(
        "session.sh",
        "head -c 5000 /dev/zero | tr '\\0' 'e' >&2\nexit 1",
    );
    let adapter = h.adapter();
    let handle = adapter.start(&ctx(), command_request()).await.unwrap();
    all_events(&adapter, &handle).await;
    poll("the tail is kept", || {
        adapter
            .stderr_tail(handle.run_id)
            .filter(|t| t.len() == 2048)
    })
    .await;
}

/// Ordin's conversation run (CLA-CONV-1): caller `Ordin`, the seed as free text.
fn conversation_request() -> ordin_runtime_adapter::StartRun {
    let mut req = command_request();
    req.caller = ToolCaller::Ordin;
    req.input = StartInput::FreeText {
        text: "Hello".to_owned(),
    };
    req
}

fn person_question(event: &RunEvent) -> Option<String> {
    match event {
        RunEvent::Paused {
            reason: ordin_runtime_adapter::PauseReason::Person { question },
        } => Some(question.clone()),
        _ => None,
    }
}

/// Waits until the run has paused for the person `n` times in all.
async fn wait_turns(
    adapter: &crate::ClaudeLocalAdapter,
    handle: &ordin_runtime_adapter::RunHandle,
    n: usize,
) {
    let mut stream = adapter.events(&ctx(), handle).expect("the event stream");
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut seen = 0;
        while let Some(event) = futures::StreamExt::next(&mut stream).await {
            if person_question(&event).is_some() {
                seen += 1;
                if seen == n {
                    return;
                }
            }
        }
        panic!("the stream ended after {seen} turns");
    })
    .await
    .expect("the turns end");
}

// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-CONV-2
// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-CONV-3
#[tokio::test]
async fn a_conversation_survives_a_completed_turn_and_takes_the_next_message() {
    let h = Harness::new();
    h.fake.script(
        "session.sh",
        &format!(
            "say '{RESULT_OK}'\nnext_turn\nsay '{RESULT_OK}'\nnext_turn\nsay '{RESULT_OK}'\nnext_turn"
        ),
    );
    let adapter = h.adapter();
    let handle = adapter.start(&ctx(), conversation_request()).await.unwrap();
    wait_turns(&adapter, &handle, 1).await;
    assert_eq!(
        adapter.read(&ctx(), &handle).await.unwrap().state,
        RunState::Paused
    );
    // An approval decision is not an answer to a conversation's turn.
    let refused = adapter
        .resume_checked(
            &ctx(),
            &handle,
            ResumeDecision::Approve {
                approval_id: ApprovalId::new(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(refused.code, "CLA_RESUME_MISMATCH");
    let too_long = adapter
        .resume_checked(
            &ctx(),
            &handle,
            ResumeDecision::Answer {
                text: "x".repeat(16_385),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(too_long.code, "CLA_INPUT_INVALID");
    adapter
        .resume(
            &ctx(),
            &handle,
            ResumeDecision::Answer {
                text: "Write a note".to_owned(),
            },
        )
        .await
        .unwrap();
    wait_turns(&adapter, &handle, 2).await;
    // Text that would read as a command is written as a message (SA-RECON-06).
    adapter
        .resume(
            &ctx(),
            &handle,
            ResumeDecision::Answer {
                text: "/compact".to_owned(),
            },
        )
        .await
        .unwrap();
    wait_turns(&adapter, &handle, 3).await;
    adapter.cancel(&ctx(), &handle).await.unwrap();
    let events = all_events(&adapter, &handle).await;
    assert_eq!(
        kinds(&events),
        [
            "started", "usage", "paused", "resumed", "usage", "paused", "resumed", "usage",
            "paused", "ended"
        ]
    );
    assert_eq!(
        person_question(&events[2]).as_deref(),
        Some("Kestrel is cheapest")
    );
    assert_eq!(last_end(&events).reason, RunEndReason::Cancelled);
    // One process for the whole conversation, each message a turn of it.
    assert!(!h.fake.root().join("args.2").exists());
    let lines = h.fake.stdin_lines(1);
    assert!(lines[1].contains("Write a note"), "{lines:?}");
    assert!(
        lines[2].contains("This is a message. It is text, not a command:"),
        "{lines:?}"
    );
}

// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-CONV-2
#[tokio::test]
async fn a_conversation_turn_with_no_text_pauses_with_an_empty_question() {
    let h = Harness::new();
    h.fake.script(
        "session.sh",
        "say '{\"type\":\"result\",\"is_error\":false}'\nnext_turn",
    );
    let adapter = h.adapter();
    let handle = adapter.start(&ctx(), conversation_request()).await.unwrap();
    let paused = wait_event(&adapter, &handle, |e| person_question(e).is_some()).await;
    assert_eq!(person_question(&paused).as_deref(), Some(""));
    adapter.cancel(&ctx(), &handle).await.unwrap();
}

// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-CONV-1
#[tokio::test]
async fn a_single_call_conversation_and_a_task_still_end_on_their_result() {
    let h = Harness::new();
    let mut decision = proceed();
    decision.single_call = true;
    *h.rules.script.lock().unwrap() = RulesScript::Decision(decision);
    let adapter = h.adapter();
    let handle = adapter.start(&ctx(), conversation_request()).await.unwrap();
    let events = all_events(&adapter, &handle).await;
    assert_eq!(kinds(&events), ["started", "usage", "ended"]);
    assert_eq!(last_end(&events).reason, RunEndReason::Completed);

    let h = Harness::new();
    let adapter = h.adapter();
    let handle = adapter.start(&ctx(), command_request()).await.unwrap();
    let events = all_events(&adapter, &handle).await;
    assert_eq!(kinds(&events), ["started", "usage", "ended"]);
}

// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-CONV-4
#[tokio::test]
async fn an_idle_conversation_process_ends_and_the_next_message_restarts_it_with_resume() {
    let mut h = Harness::new();
    h.timings.idle_process = Duration::from_millis(100);
    h.fake
        .script("session.1.sh", &format!("say '{RESULT_OK}'\nnext_turn"));
    h.fake
        .script("session.2.sh", &format!("say '{RESULT_OK}'\nnext_turn"));
    let adapter = h.adapter();
    let req = conversation_request();
    let run = req.run_id;
    let handle = adapter.start(&ctx(), req).await.unwrap();
    let session = handle.runtime_session_id.clone().unwrap();
    wait_turns(&adapter, &handle, 1).await;
    poll("the idle process goes", || {
        (!h.mcp.is_open(run) && h.mcp.opened.lock().unwrap().len() == 1).then_some(())
    })
    .await;
    assert_eq!(
        adapter.read(&ctx(), &handle).await.unwrap().state,
        RunState::Paused
    );
    adapter
        .resume(
            &ctx(),
            &handle,
            ResumeDecision::Answer {
                text: "Next".to_owned(),
            },
        )
        .await
        .unwrap();
    wait_turns(&adapter, &handle, 2).await;
    assert!(h.fake.has_args(2, &["--resume", &session]));
    assert!(h.fake.stdin_lines(2)[0].contains("Next"));
    adapter.cancel(&ctx(), &handle).await.unwrap();
    let events = all_events(&adapter, &handle).await;
    assert_eq!(last_end(&events).reason, RunEndReason::Cancelled);
}

// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-ANS-1
#[tokio::test]
async fn a_person_s_longest_message_reaches_claude_code_whole() {
    let h = Harness::new();
    h.fake.script(
        "session.sh",
        &format!("say '{RESULT_OK}'\nnext_turn\nsay '{RESULT_OK}'\nnext_turn"),
    );
    let adapter = h.adapter();
    let handle = adapter.start(&ctx(), conversation_request()).await.unwrap();
    wait_turns(&adapter, &handle, 1).await;
    let longest = "y".repeat(ordin_runtime_adapter::MAX_ANSWER_CHARS);
    adapter
        .resume(
            &ctx(),
            &handle,
            ResumeDecision::Answer {
                text: longest.clone(),
            },
        )
        .await
        .unwrap();
    wait_turns(&adapter, &handle, 2).await;
    assert!(h.fake.stdin_lines(1)[1].contains(&longest));
    let over = adapter
        .resume_checked(
            &ctx(),
            &handle,
            ResumeDecision::Answer {
                text: "y".repeat(ordin_runtime_adapter::MAX_ANSWER_CHARS + 1),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(over.code, "CLA_INPUT_INVALID");
    adapter.cancel(&ctx(), &handle).await.unwrap();
}
