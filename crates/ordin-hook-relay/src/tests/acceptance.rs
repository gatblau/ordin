//! One test per Gherkin scenario of HookRelay.md, named as the scenario (X2), then the behaviours
//! the scenarios imply and the spec states elsewhere. Scenarios that need the real Rules Engine
//! (the journalled tool request, the real verdicts) run in `tests/hook_integration.rs`.

use std::time::Duration;

/// A file in a place, at an absolute path on this platform: a Windows path needs a drive.
const RATES: &str = if cfg!(windows) {
    "C:/place/rates.md"
} else {
    "/place/rates.md"
};

use futures::StreamExt;
use ordin_session_supervisor::ResumeDecision;
use ordin_types::domain::runs::ApprovalState;
use ordin_types::time::Timestamp;
use ordin_types::wire::hook::{HarnessKind, HookMessage, HookReply};
use ordin_types::wire::rules::ToolOrigin;
use ordin_types::{ApprovalId, HookCallId};
use serde_json::json;

use super::*;
use crate::consts::Timings;

const JUDGED_LOOP: &str = "The run cannot end yet. The control judged-loop:draft requires a call to the sub-agent checker before the run ends.";

#[tokio::test]
async fn happy_path_an_allowed_call() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let started = Instant::now();
    let (call_id, bytes) = rig.line(&opened, pre("Read", Some("toolu_1")));
    let replies = rig.send(&bytes).await;
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "within the hook's 2 s"
    );
    assert_eq!(replies.len(), 1);
    assert!(matches!(&replies[0], HookReply::Allow { call_id: c } if *c == call_id));
    // The engine was asked once, with the session's identifiers and the point BeforeToolCall.
    let calls = rig.rules.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0.run_id, opened.run_id);
    assert_eq!(calls[0].0.agent_instance_id, opened.agent_instance_id);
    assert_eq!(calls[0].0.workspace_id, opened.workspace_id);
    assert_eq!(calls[0].0.tool, "Read");
    assert_eq!(
        serde_json::to_value(calls[0].0.point).unwrap(),
        json!("before_tool_call")
    );
    assert!(calls[0].1.class.is_none());
}

#[tokio::test]
async fn ask_and_approve_within_the_deadline() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let approval = ApprovalId::new();
    rig.rules.set_decide(Decide::Ask(Some(approval)));
    *rig.rules.wait_delay.lock().unwrap() = Duration::from_millis(120);
    let before = Timestamp::now().unix_millis();
    let (call_id, bytes) = rig.line(&opened, pre("Bash", Some("toolu_2")));
    let replies = rig.send(&bytes).await;
    let after = Timestamp::now().unix_millis();
    assert_eq!(replies.len(), 2, "a holding line, then the final line");
    match &replies[0] {
        HookReply::Holding {
            call_id: c,
            approval_id,
            deadline,
        } => {
            assert_eq!(*c, call_id);
            assert_eq!(*approval_id, approval);
            // The deadline is the receipt plus the hold (500 ms in these timings).
            assert!(deadline.unix_millis() >= before + 500);
            assert!(deadline.unix_millis() <= after + 500);
        }
        other => panic!("expected holding, got {other:?}"),
    }
    assert!(matches!(&replies[1], HookReply::Allow { call_id: c } if *c == call_id));
}

#[tokio::test]
async fn the_holding_deadline_is_fifty_seconds_after_receipt_with_the_spec_timings() {
    let rig = Rig::with_timings(Timings::SPEC).await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    rig.rules.set_decide(Decide::Ask(Some(ApprovalId::new())));
    let before = Timestamp::now().unix_millis();
    let replies = rig.call(&opened, pre("Bash", None)).await;
    let after = Timestamp::now().unix_millis();
    let HookReply::Holding { deadline, .. } = &replies[0] else {
        panic!("expected holding, got {:?}", replies[0]);
    };
    assert!(deadline.unix_millis() >= before + 50_000);
    assert!(deadline.unix_millis() <= after + 50_000);
    assert_eq!(Timings::SPEC.read, Duration::from_secs(5));
    assert_eq!(Timings::SPEC.decision, Duration::from_millis(1_500));
    assert_eq!(Timings::SPEC.shutdown_grace, Duration::from_secs(2));
}

#[tokio::test]
async fn ask_and_the_deadline_passes_then_the_person_approves() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let approval = ApprovalId::new();
    rig.rules.set_decide(Decide::Ask(Some(approval)));
    *rig.rules.waited.lock().unwrap() = Waited::UntilDeadline;
    let replies = rig
        .call(
            &opened,
            pre("mcp__supplier-db__update_rate", Some("toolu_3")),
        )
        .await;
    assert_eq!(replies.len(), 2);
    assert!(matches!(replies[0], HookReply::Holding { .. }));
    assert_eq!(
        deny_reason(&replies[1]),
        "waiting for the person's approval"
    );
    assert_eq!(
        rig.late_rows().await,
        1,
        "a late-decision row exists for the approval"
    );
    // The person approves later: the supervisor resumes the run with Approve, and the row goes.
    rig.approvals.decide(approval, ApprovalState::Approved);
    eventually("the run is resumed", || {
        !rig.supervisor.resumes.lock().unwrap().is_empty()
    })
    .await;
    let resumes = rig.supervisor.resumes.lock().unwrap().clone();
    assert_eq!(resumes.len(), 1);
    assert_eq!(resumes[0].0, opened.run_id);
    assert!(
        matches!(&resumes[0].1, ResumeDecision::Approve { approval_id } if *approval_id == approval)
    );
    wait_for_rows(&rig, 0).await;
}

#[tokio::test]
async fn late_decision_survives_a_restart() {
    // The first relay holds a call past its deadline and is shut down with the row still stored.
    let first = Rig::new().await;
    let opened = first.open(HarnessKind::ClaudeCode);
    let approval = ApprovalId::new();
    first.rules.set_decide(Decide::Ask(Some(approval)));
    *first.rules.waited.lock().unwrap() = Waited::UntilDeadline;
    let _ = first.call(&opened, pre("Bash", None)).await;
    assert_eq!(first.late_rows().await, 1);
    first.relay.shutdown().await;

    // The application restarts: a new relay over the same store reloads the row at start.
    let supervisor = ScriptedSupervisor::new();
    let approvals = ScriptedApprovals::new();
    let second = Rig::over(
        TempDir::new(),
        first.store.clone(),
        first.journal.clone(),
        fast(),
        approvals.clone(),
        supervisor.clone(),
    );
    second.relay.start().await.expect("start");
    approvals.decide(approval, ApprovalState::Approved);
    eventually("the run is resumed", || {
        !supervisor.resumes.lock().unwrap().is_empty()
    })
    .await;
    let resumes = supervisor.resumes.lock().unwrap().clone();
    assert_eq!(resumes[0].0, opened.run_id);
    assert!(
        matches!(&resumes[0].1, ResumeDecision::Approve { approval_id } if *approval_id == approval)
    );
    second.relay.shutdown().await;
}

#[tokio::test]
async fn wrong_mac() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let message = pre("Bash", Some("toolu_4"));
    let mut request = signed(&opened, HookCallId::new(), message);
    request.mac = mac_of(&[9u8; 32], &request.message);
    let mut bytes = serde_json::to_vec(&request).unwrap();
    bytes.push(b'\n');
    let replies = rig.send(&bytes).await;
    assert_eq!(deny_reason(&replies[0]), "refused");
    let refusals = rig.refusals().await;
    assert_eq!(refusals.len(), 1);
    assert_eq!(refusals[0]["code"], "HOOK_UNAUTHENTICATED");
    // The journal records the refusal without the arguments, the tool, the MAC or the secret.
    let text = serde_json::to_string(&rig.entries("hook_refused").await).unwrap();
    for leaked in [
        "a.txt",
        "toolu_4",
        "Bash",
        request.mac.as_str(),
        opened.env.secret.as_str(),
    ] {
        assert!(!text.contains(leaked), "{leaked} must not be journalled");
    }
    assert!(
        rig.rules.calls.lock().unwrap().is_empty(),
        "the engine was never asked"
    );
}

#[tokio::test]
async fn replay() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let (call_id, bytes) = rig.line(&opened, pre("Read", None));
    let first = rig.send(&bytes).await;
    assert!(matches!(first[0], HookReply::Allow { .. }));
    let again = rig.send(&bytes).await;
    assert!(matches!(&again[0], HookReply::Deny { call_id: c, .. } if *c == call_id));
    assert_eq!(deny_reason(&again[0]), "refused");
    assert_eq!(
        rig.rules.calls.lock().unwrap().len(),
        1,
        "the engine was asked once"
    );
}

#[tokio::test]
async fn unknown_session() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    rig.relay.close_session(opened.run_id);
    let replies = rig.call(&opened, pre("Read", None)).await;
    assert_eq!(
        deny_reason(&replies[0]),
        "this session is not known to ordin"
    );
}

#[tokio::test]
async fn malformed_line() {
    let rig = Rig::new().await;
    let replies = rig.send(b"not json\n").await;
    assert_eq!(replies.len(), 1);
    match &replies[0] {
        HookReply::Deny { call_id, reason } => {
            assert_eq!(call_id.0, uuid::Uuid::nil(), "the nil call id");
            assert_eq!(reason, "ordin could not read this request");
        }
        other => panic!("expected a deny, got {other:?}"),
    }
}

#[tokio::test]
async fn newer_protocol() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let mut request = signed(&opened, HookCallId::new(), pre("Read", None));
    request.protocol = 2;
    let mut bytes = serde_json::to_vec(&request).unwrap();
    bytes.push(b'\n');
    let replies = rig.send(&bytes).await;
    assert_eq!(deny_reason(&replies[0]), "ordin needs updating");
}

#[tokio::test]
async fn the_engine_is_slow() {
    // The spec's engine takes 3 seconds; the relay answers inside its own 1.5 s budget.
    let rig = Rig::with_timings(Timings::SPEC).await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    rig.rules.set_decide(Decide::Slow(Duration::from_secs(3)));
    let started = Instant::now();
    let replies = rig.call(&opened, pre("Read", None)).await;
    assert!(
        started.elapsed() < Duration::from_millis(1_600),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(deny_reason(&replies[0]), "ordin could not decide in time");
}

#[tokio::test]
async fn stop_refused_by_a_judged_loop() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    *rig.rules.end.lock().unwrap() = EndScript::Continue(JUDGED_LOOP.into());
    let replies = rig.call(&opened, stop()).await;
    assert_eq!(deny_reason(&replies[0]), JUDGED_LOOP);
    // E.3: the stop is journalled for the Ordin Host's check of the result.
    let entries = rig.entries("hook_stop").await;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].run_id, Some(opened.run_id));
}

#[tokio::test]
async fn post_tool_use_is_acknowledged() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let allowed = rig.call(&opened, pre("Read", Some("toolu_01AbCdEf"))).await;
    assert!(matches!(allowed[0], HookReply::Allow { .. }));
    let request = rig.rules.requests.lock().unwrap()[0];
    let (call_id, bytes) = rig.line(&opened, post("Read", Some("toolu_01AbCdEf"), true));
    let replies = rig.send(&bytes).await;
    assert!(matches!(&replies[0], HookReply::Ack { call_id: c } if *c == call_id));
    assert_eq!(*rig.rules.outcomes.lock().unwrap(), vec![(request, true)]);
}

#[tokio::test]
async fn a_line_from_an_older_bridge_is_refused() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let request = signed(&opened, HookCallId::new(), pre("Read", None));
    let mut value = serde_json::to_value(&request).unwrap();
    value["bridge"] = json!({"server": "memory"});
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    let replies = rig.send(&bytes).await;
    assert_eq!(replies.len(), 1, "the relay answers once and closes");
    assert_eq!(
        deny_reason(&replies[0]),
        "ordin could not read this request"
    );
    assert!(rig.rules.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn session_events_reach_the_adapter() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let mut events = rig.relay.session_events(opened.run_id);
    let approval = ApprovalId::new();
    rig.rules.set_decide(Decide::Ask(Some(approval)));
    *rig.rules.waited.lock().unwrap() = Waited::UntilDeadline;
    let mut message = pre("mcp__supplier-db__update_rate", None);
    if let HookMessage::PreToolUse { origin, .. } = &mut message {
        *origin = ToolOrigin::PackageMcpServer {
            server: "supplier-db".into(),
        };
    }
    let _ = rig.call(&opened, message).await;
    let got = events_within(&mut events, 2, Duration::from_secs(2)).await;
    assert_eq!(got[0]["kind"], "holding");
    assert_eq!(got[0]["approval_id"], approval.to_string());
    assert_eq!(got[0]["tool"], "mcp__supplier-db__update_rate");
    assert_eq!(got[1]["kind"], "denied_at_deadline");
    assert_eq!(got[1]["approval_id"], approval.to_string());
    assert_eq!(got[1]["tool"], "mcp__supplier-db__update_rate");

    // The hook later reports an allowed Read; the stream yields ToolUsed with origin built_in.
    rig.rules.set_decide(Decide::Allow);
    let _ = rig.call(&opened, pre("Read", Some("toolu_5"))).await;
    let _ = rig.call(&opened, post("Read", Some("toolu_5"), true)).await;
    let got = events_within(&mut events, 2, Duration::from_secs(2)).await;
    assert_eq!(
        got[0],
        json!({"kind": "decided", "tool": "Read", "allowed": true})
    );
    assert_eq!(
        got[1],
        json!({"kind": "tool_used", "tool": "Read", "origin": {"kind": "built_in"}, "succeeded": true})
    );
}

#[tokio::test]
async fn copilot_decision_in_process() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::CopilotCli);
    let mut events = rig.relay.session_events(opened.run_id);
    rig.rules
        .set_decide(Decide::Deny("The shell may not run rm.".into()));
    let reply = rig
        .relay
        .decide_in_process(opened.run_id, pre("bash", Some("c-1")))
        .await;
    assert_eq!(
        deny_reason(&reply),
        "The shell may not run rm.",
        "the engine's reason"
    );
    let got = events_within(&mut events, 1, Duration::from_secs(2)).await;
    assert_eq!(
        got[0],
        json!({"kind": "decided", "tool": "bash", "allowed": false})
    );
    // No socket was involved: the relay was never started.
    assert!(!std::path::Path::new(rig.relay.endpoint()).exists());
}

#[tokio::test]
async fn in_process_call_for_an_unknown_session() {
    let rig = Rig::new().await;
    let reply = rig
        .relay
        .decide_in_process(ordin_types::RunId::new(), pre("bash", None))
        .await;
    assert_eq!(deny_reason(&reply), "this session is not known to ordin");
}

#[tokio::test]
async fn peer_from_another_user() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let (_, bytes) = rig.line(&opened, pre("Read", None));
    let replies = rig.send_as(&bytes, false).await;
    match &replies[0] {
        HookReply::Deny { call_id, reason } => {
            assert_eq!(call_id.0, uuid::Uuid::nil());
            assert_eq!(reason, "refused");
        }
        other => panic!("expected a deny, got {other:?}"),
    }
    let refusals = rig.refusals().await;
    assert_eq!(refusals[0]["code"], "HOOK_PEER_REFUSED");
    assert!(rig.rules.calls.lock().unwrap().is_empty());
}

// ---- behaviours the scenarios imply

#[tokio::test]
async fn the_environment_for_the_harness_names_the_session_and_hides_the_secret() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let vars = opened.env.env_vars();
    assert_eq!(
        vars[0],
        ("ORDIN_HOOK_ENDPOINT", rig.relay.endpoint().to_owned())
    );
    assert_eq!(vars[1].0, "ORDIN_HOOK_SECRET");
    assert_eq!(vars[1].1.len(), 43, "32 bytes, base64url without padding");
    assert!(!vars[1].1.contains('='));
    assert_eq!(vars[2], ("ORDIN_SESSION_ID", opened.run_id.to_string()));
    assert_eq!(vars[3], ("ORDIN_HOOK_PROTOCOL", "1".to_owned()));
    let shown = format!("{:?}", opened.env);
    assert!(
        !shown.contains(&opened.env.secret),
        "Debug never prints the secret"
    );
    assert!(shown.contains(&opened.run_id.to_string()));
}

#[tokio::test]
async fn a_closed_session_ends_its_streams_and_refuses_calls() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let mut events = rig.relay.session_events(opened.run_id);
    rig.relay.close_session(opened.run_id);
    assert!(
        events.next().await.is_none(),
        "closing the session ends the stream"
    );
    let mut unknown = rig.relay.session_events(opened.run_id);
    assert!(
        unknown.next().await.is_none(),
        "an unknown run gives a stream that ends at once"
    );
    // A resumed session closes the old one first, then opens again with a new secret.
    let again = rig.open_spec(crate::HookSessionSpec {
        run_id: opened.run_id,
        agent_instance_id: opened.agent_instance_id,
        workspace_id: opened.workspace_id,
        harness: HarnessKind::ClaudeCode,
    });
    assert_ne!(again.env.secret, opened.env.secret);
}

#[tokio::test]
async fn an_engine_deny_reason_is_cut_to_500_characters() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    rig.rules.set_decide(Decide::Deny("é".repeat(900)));
    let replies = rig.call(&opened, pre("Bash", None)).await;
    assert_eq!(deny_reason(&replies[0]).chars().count(), 500);
    rig.rules.set_decide(Decide::Deny(String::new()));
    let replies = rig.call(&opened, pre("Bash", None)).await;
    assert_eq!(
        deny_reason(&replies[0]),
        "refused",
        "a reason is 1 to 500 characters"
    );
}

#[tokio::test]
async fn the_subagent_reaches_the_engine_as_context() {
    // C-78: the context carries the message's `subagent`; the hook leaves it absent until the spike.
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let mut named = pre("Read", None);
    if let HookMessage::PreToolUse { subagent, .. } = &mut named {
        *subagent = Some("reviewer".into());
    }
    let _ = rig.call(&opened, named).await;
    let _ = rig.call(&opened, pre("Read", None)).await;
    let calls = rig.rules.calls.lock().unwrap();
    assert_eq!(calls[0].1.subagent.as_deref(), Some("reviewer"));
    assert_eq!(calls[1].1.subagent, None);
}

#[tokio::test]
async fn a_rejected_approval_tells_the_model_so() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    rig.rules.set_decide(Decide::Ask(Some(ApprovalId::new())));
    *rig.rules.waited.lock().unwrap() = Waited::Rejected;
    let replies = rig.call(&opened, pre("Bash", None)).await;
    assert_eq!(deny_reason(&replies[1]), "The person rejected this action.");
    *rig.rules.waited.lock().unwrap() = Waited::Expired;
    let replies = rig.call(&opened, pre("Bash", None)).await;
    assert_eq!(
        deny_reason(&replies[1]),
        "The run ended before the person decided."
    );
    assert_eq!(rig.late_rows().await, 0);
}

#[tokio::test]
async fn an_unmatched_after_tool_event_is_acknowledged_with_the_built_in_origin() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let mut events = rig.relay.session_events(opened.run_id);
    let replies = rig
        .call(&opened, post("Read", Some("never-seen"), false))
        .await;
    assert!(matches!(replies[0], HookReply::Ack { .. }));
    let got = events_within(&mut events, 1, Duration::from_secs(2)).await;
    assert_eq!(got[0]["kind"], "tool_used");
    assert_eq!(got[0]["origin"], json!({"kind": "built_in"}));
    assert!(rig.rules.outcomes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_package_tool_keeps_its_origin_through_the_after_tool_event() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let mut events = rig.relay.session_events(opened.run_id);
    let origin = ToolOrigin::PackageMcpServer {
        server: "supplier-db".into(),
    };
    let mut message = pre("mcp__supplier-db__lookup", Some("toolu_9"));
    if let HookMessage::PreToolUse { origin: o, .. } = &mut message {
        *o = origin;
    }
    let _ = rig.call(&opened, message).await;
    let _ = rig
        .call(
            &opened,
            post("mcp__supplier-db__lookup", Some("toolu_9"), true),
        )
        .await;
    let got = events_within(&mut events, 2, Duration::from_secs(2)).await;
    assert_eq!(
        got[1]["origin"],
        json!({"kind": "package_mcp_server", "server": "supplier-db"})
    );
}

#[tokio::test]
async fn a_slow_adapter_loses_the_oldest_events_and_the_stream_goes_on() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let mut events = rig.relay.session_events(opened.run_id);
    // 70 events with nobody reading: the 64-event sender drops the oldest ones.
    for i in 0..70 {
        let id = format!("t{i}");
        let _ = rig.call(&opened, post("Read", Some(&id), true)).await;
    }
    let got = events_within(&mut events, 64, Duration::from_secs(2)).await;
    assert_eq!(got.len(), 64);
    let _ = rig.call(&opened, stop()).await;
    let more = events_within(&mut events, 1, Duration::from_secs(2)).await;
    assert_eq!(more[0]["kind"], "turn_stopping");
}

#[tokio::test]
async fn a_stop_that_needs_a_person_is_held_then_settled() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let mut events = rig.relay.session_events(opened.run_id);
    let approval = ApprovalId::new();
    *rig.rules.end.lock().unwrap() = EndScript::Ask(approval);
    let approvals = rig.approvals.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        approvals.decide(approval, ApprovalState::Approved);
    });
    let replies = rig.call(&opened, stop()).await;
    assert!(
        matches!(replies[0], HookReply::Holding { approval_id, .. } if approval_id == approval)
    );
    assert!(matches!(replies[1], HookReply::Ack { .. }));
    assert_eq!(
        *rig.rules.settled.lock().unwrap(),
        vec![(opened.run_id, true)]
    );
    let got = events_within(&mut events, 2, Duration::from_secs(2)).await;
    assert_eq!(got[0], json!({"kind": "turn_stopping"}));
    assert_eq!(got[1]["kind"], "holding");

    // Rejected: the engine's refusal text goes back as the deny reason.
    let approval = ApprovalId::new();
    *rig.rules.end.lock().unwrap() = EndScript::Ask(approval);
    rig.approvals.decide(approval, ApprovalState::Rejected);
    let replies = rig.call(&opened, stop()).await;
    assert_eq!(deny_reason(&replies[1]), "The run cannot end yet.");

    // Expired: answered as an expired tool approval.
    let approval = ApprovalId::new();
    *rig.rules.end.lock().unwrap() = EndScript::Ask(approval);
    rig.approvals.decide(approval, ApprovalState::Expired);
    let replies = rig.call(&opened, stop()).await;
    assert_eq!(
        deny_reason(&replies[1]),
        "The run ended before the person decided."
    );
}

#[tokio::test]
async fn a_stop_gate_past_the_deadline_is_settled_later() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let approval = ApprovalId::new();
    *rig.rules.end.lock().unwrap() = EndScript::Ask(approval);
    let replies = rig.call(&opened, stop()).await;
    assert_eq!(
        deny_reason(&replies[1]),
        "waiting for the person's approval"
    );
    assert_eq!(rig.late_rows().await, 1);
    let kind: String = sqlx::query_scalar("SELECT kind FROM hook_late_decisions")
        .fetch_one(rig.store.sqlite().unwrap())
        .await
        .unwrap();
    assert_eq!(kind, "stop_gate");
    rig.approvals.decide(approval, ApprovalState::Approved);
    eventually("the run is resumed", || {
        !rig.supervisor.resumes.lock().unwrap().is_empty()
    })
    .await;
    assert_eq!(
        *rig.rules.settled.lock().unwrap(),
        vec![(opened.run_id, true)]
    );
}

#[tokio::test]
async fn a_slow_end_decision_is_denied_inside_the_budget() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    *rig.rules.end.lock().unwrap() = EndScript::Slow(Duration::from_secs(3));
    let replies = rig.call(&opened, stop()).await;
    assert_eq!(deny_reason(&replies[0]), "ordin could not decide in time");
}

#[tokio::test]
async fn the_in_process_path_holds_through_events_and_returns_the_final_reply() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::CopilotCli);
    let mut events = rig.relay.session_events(opened.run_id);
    let approval = ApprovalId::new();
    rig.rules.set_decide(Decide::Ask(Some(approval)));
    *rig.rules.wait_delay.lock().unwrap() = Duration::from_millis(50);
    let reply = rig
        .relay
        .decide_in_process(opened.run_id, pre("bash", Some("c-2")))
        .await;
    assert!(
        matches!(reply, HookReply::Allow { .. }),
        "never Holding: {reply:?}"
    );
    let got = events_within(&mut events, 2, Duration::from_secs(2)).await;
    assert_eq!(got[0]["kind"], "holding");
    assert_eq!(
        got[1],
        json!({"kind": "decided", "tool": "bash", "allowed": true})
    );
    let reply = rig
        .relay
        .decide_in_process(opened.run_id, post("bash", Some("c-2"), true))
        .await;
    assert!(matches!(reply, HookReply::Ack { .. }));
    *rig.rules.end.lock().unwrap() = EndScript::Continue("not yet".into());
    let reply = rig.relay.decide_in_process(opened.run_id, stop()).await;
    assert_eq!(deny_reason(&reply), "not yet");
}

fn write(path: &str, id: &str) -> HookMessage {
    let HookMessage::PreToolUse {
        harness,
        tool: _,
        origin,
        harness_tool_use_id,
        subagent,
        ..
    } = pre("Write", Some(id))
    else {
        unreachable!("pre builds a PreToolUse");
    };
    HookMessage::PreToolUse {
        harness,
        tool: "Write".to_owned(),
        origin,
        arguments: json!({"file_path": path}),
        harness_tool_use_id,
        subagent,
    }
}

#[tokio::test]
async fn an_allowed_write_is_claimed_before_it_runs_and_settled_after() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let (_, bytes) = rig.line(&opened, write(RATES, "toolu_1"));
    let replies = rig.send(&bytes).await;
    assert!(matches!(&replies[0], HookReply::Allow { .. }));
    assert_eq!(
        rig.writes.opened.lock().unwrap().as_slice(),
        [("Write".to_owned(), WriteTarget::File(RATES.into()))]
    );
    assert!(rig.writes.settled.lock().unwrap().is_empty());
    let (_, bytes) = rig.line(&opened, post("Write", Some("toolu_1"), false));
    rig.send(&bytes).await;
    assert_eq!(rig.writes.settled.lock().unwrap().as_slice(), ["claim"]);
}

#[tokio::test]
async fn a_read_opens_no_claim_and_a_shell_call_a_wide_one() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let (_, bytes) = rig.line(&opened, pre("Read", Some("toolu_1")));
    rig.send(&bytes).await;
    assert!(rig.writes.opened.lock().unwrap().is_empty());
    let (_, bytes) = rig.line(&opened, pre("Bash", Some("toolu_2")));
    rig.send(&bytes).await;
    assert_eq!(
        rig.writes.opened.lock().unwrap().as_slice(),
        [("Bash".to_owned(), WriteTarget::Anywhere)]
    );
}

/// A call to an MCP server's tool, as the hook names it: every server a package's.
fn mcp_call(server: &str, tool: &str, id: &str) -> HookMessage {
    let HookMessage::PreToolUse {
        harness,
        harness_tool_use_id,
        subagent,
        ..
    } = pre("Read", Some(id))
    else {
        unreachable!("pre builds a PreToolUse");
    };
    HookMessage::PreToolUse {
        harness,
        tool: format!("mcp__{server}__{tool}"),
        origin: ordin_types::wire::rules::ToolOrigin::PackageMcpServer {
            server: server.to_owned(),
        },
        arguments: json!({}),
        harness_tool_use_id,
        subagent,
    }
}

// HR-ORIG-1: Ordin's own tool takes no claim; a package server's tool still takes a wide one.
// spec: ordin-desktop/components/HookRelay#HR-ORIG-1
#[tokio::test]
async fn ordins_own_tool_opens_no_claim_and_a_package_servers_a_wide_one() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let (_, bytes) = rig.line(&opened, mcp_call("ordin", "delegate", "toolu_1"));
    rig.send(&bytes).await;
    assert!(rig.writes.opened.lock().unwrap().is_empty());
    let (_, bytes) = rig.line(&opened, mcp_call("supplier-db", "update_rate", "toolu_2"));
    rig.send(&bytes).await;
    assert_eq!(
        rig.writes.opened.lock().unwrap().as_slice(),
        [(
            "mcp__supplier-db__update_rate".to_owned(),
            WriteTarget::Anywhere
        )]
    );
}

#[tokio::test]
async fn a_write_with_no_claim_does_not_run() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    rig.writes.fail_open.store(true, Ordering::SeqCst);
    let (_, bytes) = rig.line(&opened, write(RATES, "toolu_1"));
    let replies = rig.send(&bytes).await;
    assert!(matches!(&replies[0], HookReply::Deny { .. }));
    rig.writes.fail_open.store(false, Ordering::SeqCst);
    rig.writes.hang_open.store(true, Ordering::SeqCst);
    let (_, bytes) = rig.line(&opened, write(RATES, "toolu_2"));
    let replies = rig.send(&bytes).await;
    assert!(matches!(&replies[0], HookReply::Deny { .. }));
}

#[tokio::test]
async fn a_refused_write_opens_no_claim() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    rig.rules.set_decide(Decide::Deny("no".to_owned()));
    let (_, bytes) = rig.line(&opened, write(RATES, "toolu_1"));
    let replies = rig.send(&bytes).await;
    assert!(matches!(&replies[0], HookReply::Deny { .. }));
    assert!(rig.writes.opened.lock().unwrap().is_empty());
}

#[tokio::test]
async fn closing_the_session_ends_the_runs_claims() {
    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    rig.relay.close_session(opened.run_id);
    tokio::task::yield_now().await;
    let ended = async {
        while rig.writes.ended.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(Duration::from_secs(2), ended)
        .await
        .expect("the claims end");
    assert_eq!(rig.writes.ended.lock().unwrap().as_slice(), [opened.run_id]);
}

#[tokio::test]
async fn a_relay_says_whether_it_attributes_writes() {
    let rig = Rig::new().await;
    assert!(rig.relay.attributes_writes());
    assert!(!crate::AgentWrites::attributes_writes(&crate::NoPlaces));
}
