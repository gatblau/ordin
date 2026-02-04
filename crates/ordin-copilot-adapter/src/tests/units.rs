//! The rules of *Internal Logic* not covered by a Gherkin scenario or an Error Table row.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use ordin_hook_relay::HookSessionEvent;
use ordin_rules::RunStartDecision;
use ordin_runtime_adapter::{
    AdapterKind, HarnessKind, ModelLevel, PauseReason, ResumeDecision, RunEvent, RuntimeAdapter,
    StartInput,
};
use ordin_types::wire::rules::ToolOrigin;
use ordin_types::ApprovalId;

use super::support::*;
use crate::env::{server_environment, ParentEnv};
use crate::gate::{classify, ServerNames};
use crate::models::CopilotModels;
use crate::port::CopilotEvent;
use crate::session::package_servers;
use crate::testkit::ScriptedCopilotPort;
use ordin_runtime_adapter::HarnessModels;

fn names() -> ServerNames {
    ServerNames {
        front: vec!["work".into(), "memory".into()],
        package: vec!["quotes".into()],
    }
}

#[test]
fn tools_are_classified_by_the_sessions_servers() {
    let origin = |tool: &str| serde_json::to_value(classify(tool, &names())).unwrap();
    assert_eq!(origin("mcp__work__plan")["server"], "work");
    assert_eq!(origin("memory-recall")["kind"], "ordin_mcp_front");
    assert_eq!(origin("mcp__quotes__list")["kind"], "package_mcp_server");
    assert_eq!(origin("shell")["kind"], "built_in");
    assert_eq!(origin("mcp__workshop__x")["kind"], "built_in");
    assert!(matches!(classify("view", &names()), ToolOrigin::BuiltIn));
}

#[test]
fn the_environment_is_an_allow_list_with_the_clis_folder_last_on_path() {
    let env = parent_env();
    let parent: ParentEnv = Arc::new(move |name: &str| env.get(name).map(|v| (*v).into()));
    let built = server_environment(&parent, Path::new("/home/p/.local/bin/copilot"));
    assert_eq!(built["PATH"], "/usr/bin:/bin:/home/p/.local/bin");
    assert!(!built.contains_key("GH_TOKEN") && !built.contains_key("ORDIN_SECRET"));
    let again = server_environment(&parent, Path::new("/usr/bin/copilot"));
    assert_eq!(
        again["PATH"], "/usr/bin:/bin",
        "a folder already on PATH is not added"
    );
}

#[test]
// spec: ordin-desktop/components/CopilotLocalAdapter#CPA-HT-2
fn only_package_servers_are_passed_in_the_mcp_servers_json() {
    let front = vec!["work".to_owned()];
    let package = crate::config::PackageMcpServer {
        name: "quotes".into(),
        command: PathBuf::from("/opt/q"),
        args: vec!["--fast".into()],
        env: [("KEY".to_owned(), "secret".to_owned())].into(),
    };
    let merged = package_servers(&front, std::slice::from_ref(&package)).unwrap();
    assert_eq!(merged["mcpServers"]["quotes"]["command"], "/opt/q");
    assert!(merged["mcpServers"].get("work").is_none());
    let clash = crate::config::PackageMcpServer {
        name: "work".into(),
        ..package.clone()
    };
    assert_eq!(package_servers(&front, &[clash]).unwrap_err(), "work");
    assert!(
        !format!("{package:?}").contains("secret"),
        "env values are never printed"
    );
}

#[test]
fn copilot_models() {
    let models = CopilotModels;
    assert_eq!(models.harness(), HarnessKind::CopilotCli);
    assert!(models.per_session_model());
    assert!(models.accepts_model("automatic") && models.accepts_model("gpt-5.4"));
    assert!(!models.accepts_model("") && !models.accepts_model("bad name"));
    assert!(!models.accepts_model(&"m".repeat(101)));
    let levels = models.default_level_models();
    assert_eq!(
        levels.get(&ModelLevel::Standard).map(String::as_str),
        Some("automatic")
    );
}

#[tokio::test]
async fn the_adapter_offers_every_verb_as_copilot_local() {
    let h = Harness::new();
    assert_eq!(h.adapter.kind(), AdapterKind::CopilotLocal);
    assert!(h.adapter.unsupported_verbs().is_empty());
    assert_eq!(
        h.adapter.harness_models().harness(),
        HarnessKind::CopilotCli
    );
}

#[tokio::test]
async fn a_command_is_sent_as_slash_name_with_the_rules_replacement() {
    let h = Harness::new();
    h.start(request(StartInput::Command {
        name: "summarise".into(),
        arguments: "the quotes".into(),
    }))
    .await
    .expect("start");
    assert_eq!(h.script().sent_texts()[0], "/summarise the quotes");
    *h.rules.script.lock().unwrap() = RulesScript::Decision(RunStartDecision {
        command: Some("triage".into()),
        ..proceed()
    });
    h.start(request(StartInput::Command {
        name: "summarise".into(),
        arguments: String::new(),
    }))
    .await
    .expect("start");
    assert_eq!(h.script().sent_texts()[1], "/triage");
}

#[tokio::test]
async fn the_grant_carries_the_start_requests_caller_scope_task_and_servers() {
    let h = Harness::new();
    let mut req = free_text("Do the work.");
    req.only_servers = Some(vec!["work".into(), "not a name!".into()]);
    h.start(req).await.expect("start");
    let grant = h.mcp.opened.lock().unwrap()[0].clone();
    assert_eq!(grant.harness, HarnessKind::CopilotCli);
    let only = serde_json::to_value(&grant.only_servers).unwrap();
    assert_eq!(only, serde_json::json!(["work"]));
    let facts = h.rules.facts.lock().unwrap()[0].clone();
    assert_eq!(
        (facts.started_by.as_str(), facts.asker.is_none()),
        ("person", true)
    );
}

#[tokio::test]
async fn a_stop_deny_is_sent_as_the_next_turn() {
    let h = Harness::new();
    *h.hooks.stop.lock().unwrap() = Decide::Deny("Write the table first.".into());
    let (handle, session) = h.start_live("Do the work.").await;
    h.script().emit(
        &session,
        CopilotEvent::TurnEnded {
            final_text: Some("Done.".into()),
        },
    );
    poll("the refusal sent", || {
        (h.script().sent_texts().len() == 2).then_some(())
    })
    .await;
    assert_eq!(h.script().sent_texts()[1], "Write the table first.");
    assert!(h.adapter.read(&ctx(), &handle).await.unwrap().end.is_none());
}

#[tokio::test]
async fn a_held_approval_shows_waiting_and_skips_the_end_decision() {
    let h = Harness::new();
    let (handle, session) = h.start_live("Do the work.").await;
    let approval = ApprovalId::new();
    h.hooks.send(
        handle.run_id,
        HookSessionEvent::Holding {
            approval_id: approval,
            tool: "shell".into(),
        },
    );
    let events = h.events_until(&handle, |seen| seen.len() >= 2).await;
    let RunEvent::Progress { progress } = &events[1] else {
        panic!("{events:?}")
    };
    assert_eq!(progress.line, "Waiting for your approval");
    h.script()
        .emit(&session, CopilotEvent::TurnEnded { final_text: None });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !h.hooks.messages().iter().any(|m| m["kind"] == "stop"),
        "no end decision"
    );
    // `Decided` clears the hold; the next turn end is decided.
    h.hooks.send(
        handle.run_id,
        HookSessionEvent::Decided {
            tool: "shell".into(),
            allowed: true,
        },
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    h.script().emit(
        &session,
        CopilotEvent::TurnEnded {
            final_text: Some("Done.".into()),
        },
    );
    let events = h.until_ended(&handle).await;
    assert_eq!(end_reason(&events).as_deref(), Some("completed"));
}

#[tokio::test]
async fn a_rejected_approval_sends_the_reason() {
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
    let reject = ResumeDecision::Reject {
        approval_id: approval,
        reason: Some("Too risky".into()),
    };
    h.adapter
        .resume(&ctx(), &handle, reject)
        .await
        .expect("resume");
    assert_eq!(
        h.script().sent_texts().last().unwrap(),
        "The person did not approve the action. Reason: Too risky. Continue without it."
    );
}

#[tokio::test]
async fn cancel_aborts_stops_and_ends_once() {
    let h = Harness::new();
    let (handle, _) = h.start_live("Do the work.").await;
    h.adapter.cancel(&ctx(), &handle).await.expect("cancel");
    h.adapter.cancel(&ctx(), &handle).await.expect("idempotent");
    let events = h.until_ended(&handle).await;
    assert_eq!(end_reason(&events).as_deref(), Some("cancelled"));
    assert_eq!(h.script().aborted.load(Ordering::SeqCst), 1);
    assert_eq!(h.script().stopped.load(Ordering::SeqCst), 1);
    assert!(!h.mcp.is_open(handle.run_id) && !h.hooks.is_open(handle.run_id));
}

#[tokio::test]
async fn shutdown_interrupts_every_live_run() {
    let h = Harness::new();
    let (first, _) = h.start_live("One.").await;
    let (second, _) = h.start_live("Two.").await;
    h.adapter.shutdown().await;
    for handle in [first, second] {
        let events = h.until_ended(&handle).await;
        assert_eq!(end_reason(&events).as_deref(), Some("interrupted"));
    }
}

#[tokio::test]
async fn the_run_start_time_limit_stops_the_run() {
    let h = Harness::new();
    *h.rules.script.lock().unwrap() = RulesScript::Decision(RunStartDecision {
        limit_seconds: Some(0),
        ..proceed()
    });
    let (handle, _) = h.start_live("Do the work.").await;
    let events = h.until_ended(&handle).await;
    assert_eq!(end_reason(&events).as_deref(), Some("stopped_at_limit"));
}

// spec: ordin-desktop/components/CopilotLocalAdapter#C-05
#[tokio::test]
async fn a_persisted_session_is_resumed_after_a_restart() {
    let h = Harness::new();
    h.setup.setup.lock().unwrap().resume_session_id = Some("sess_8f3a2c".into());
    let handle = h.start(free_text("Carry on.")).await.expect("start");
    assert_eq!(handle.runtime_session_id.as_deref(), Some("sess_8f3a2c"));
    assert_eq!(
        h.script().resumed.lock().unwrap().as_slice(),
        ["sess_8f3a2c"]
    );
    assert!(h.script().created.lock().unwrap().is_empty());
    let events = h.events_until(&handle, |seen| !seen.is_empty()).await;
    assert!(
        matches!(&events[0], RunEvent::Started { runtime_session_id: Some(id), .. } if id == "sess_8f3a2c")
    );
}

#[tokio::test]
async fn the_post_tool_use_observer_forwards_to_the_relay() {
    let h = Harness::new();
    let (_, session) = h.start_live("Do the work.").await;
    let observer = h.script().gates(&session).unwrap().post_tool_use;
    observer
        .tool_used("view", Some("call_1".into()), false)
        .await;
    let post = h
        .hooks
        .messages()
        .into_iter()
        .find(|m| m["kind"] == "post_tool_use")
        .unwrap();
    assert_eq!(
        (post["tool"].as_str(), post["succeeded"].as_bool()),
        (Some("view"), Some(false))
    );
}

#[tokio::test]
async fn signed_out_mid_run_pauses_and_answer_restarts_the_sign_in() {
    let h = Harness::new();
    let (handle, session) = h.start_live("Do the work.").await;
    h.script().signed_in.store(false, Ordering::SeqCst);
    h.script().emit(&session, CopilotEvent::SignedOut);
    let events = h
        .events_until(&handle, |seen| {
            seen.iter().any(|e| matches!(e, RunEvent::Paused { .. }))
        })
        .await;
    assert!(matches!(
        events.last(),
        Some(RunEvent::Paused {
            reason: PauseReason::SignIn
        })
    ));
    let logins = h.script().logins.load(Ordering::SeqCst);
    let approve = ResumeDecision::Approve {
        approval_id: ApprovalId::new(),
    };
    assert_eq!(
        h.adapter
            .resume_checked(&handle, approve)
            .await
            .unwrap_err()
            .code,
        "CPA_RESUME_MISMATCH"
    );
    h.adapter
        .resume(
            &ctx(),
            &handle,
            ResumeDecision::Answer {
                text: "Sign in again".into(),
            },
        )
        .await
        .expect("resume");
    poll("login restarted", || {
        (h.script().logins.load(Ordering::SeqCst) > logins).then_some(())
    })
    .await;
    h.script().signed_in.store(true, Ordering::SeqCst);
    h.events_until(&handle, |seen| {
        seen.iter().any(|e| matches!(e, RunEvent::Resumed))
    })
    .await;
}

#[tokio::test]
async fn a_tool_call_moves_the_progress_line_without_naming_the_tool() {
    let h = Harness::new();
    let (handle, session) = h.start_live("Do the work.").await;
    h.script().emit(
        &session,
        CopilotEvent::ToolFinished {
            tool: "mcp__memory__recall".into(),
            succeeded: true,
        },
    );
    h.script().emit(
        &session,
        CopilotEvent::ToolFinished {
            tool: "edit".into(),
            succeeded: true,
        },
    );
    h.script().emit(
        &session,
        CopilotEvent::TodoList {
            items: vec!["One".into()],
        },
    );
    let events = h.events_until(&handle, |seen| seen.len() >= 3).await;
    let lines: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            RunEvent::Progress { progress } => {
                Some((progress.line.clone(), progress.count.map(|c| c.done)))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        lines,
        [
            ("Checked what it remembers".into(), Some(1)),
            ("Changed a file".into(), Some(2))
        ]
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    expect_log(&[&handle.run_id.to_string(), "RA_PLAN_INVALID"]).await;
}

#[tokio::test]
async fn the_probe_repair_restarts_the_server() {
    let h = Harness::with(ScriptedCopilotPort::signed_in(), |_| {});
    h.adapter.probe_start_sign_in().await.expect("login");
    h.adapter.probe_repair().await.expect("repair");
    assert_eq!(h.script().started.lock().unwrap().len(), 2);
    assert_eq!(h.script().stopped.load(Ordering::SeqCst), 1);
    assert_eq!(h.script().logins.load(Ordering::SeqCst), 1);
}
