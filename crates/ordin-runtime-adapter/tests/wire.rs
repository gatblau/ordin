//! The JSON forms the spec states for this crate's types (2C wire §2; *Public Interface* example).

use ordin_runtime_adapter::*;
use serde_json::json;

const RUN: &str = "0192a1c2-3344-7556-8677-8899aabbccdd";

#[test]
fn start_run_model_and_a_journalled_violation_have_the_stated_json() {
    let model = ModelChoice {
        level: ModelLevel::Standard,
        model: Some("sonnet".into()),
    };
    assert_eq!(
        serde_json::to_string(&model).unwrap(),
        r#"{"level":"standard","model":"sonnet"}"#
    );
    let violation = CheckError {
        code: "RA_EVENT_ORDER",
        field: "events",
    };
    assert_eq!(
        serde_json::to_string(&violation).unwrap(),
        r#"{"code":"RA_EVENT_ORDER","field":"events"}"#
    );
    assert_eq!(violation.to_string(), "RA_EVENT_ORDER");
}

#[test]
fn a_start_request_without_model_and_thinking_still_parses() {
    let body = json!({
        "run_id": RUN,
        "workspace_id": "0192a0f1-1111-7222-8333-444455556666",
        "target": {"kind": "agent_instance", "agent_instance_id": "0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff"},
        "input": {"kind": "command", "name": "compare-quotes", "arguments": "Q3"},
        "attachments": [],
        "limits": {"wall_clock_secs": 3600, "idle_timeout_secs": 600},
        "caller": {"kind": "agent"}
    });
    let req: StartRun = serde_json::from_value(body.clone()).unwrap();
    assert!(req.model.is_none() && req.thinking.is_none() && req.thread.is_none());
    assert_eq!(check_start(AdapterKind::ClaudeLocal, &req), Ok(()));
    assert_eq!(serde_json::to_value(&req).unwrap(), body);

    // `deny_unknown_fields`: a session export field is refused (C-80: no such call or field).
    let mut extra = body;
    extra["session_record"] = json!("…");
    assert!(serde_json::from_value::<StartRun>(extra).is_err());
}

#[test]
fn run_events_resume_decisions_and_errors_use_the_kind_tag() {
    let event = RunEvent::Paused {
        reason: PauseReason::DecisionElsewhere {
            link: url::Url::parse("https://example.slack.com/archives/C1/p1").unwrap(),
        },
    };
    assert_eq!(
        serde_json::to_value(&event).unwrap(),
        json!({"kind": "paused", "reason": {"kind": "decision_elsewhere", "link": "https://example.slack.com/archives/C1/p1"}})
    );
    let plan: RunEvent =
        serde_json::from_value(json!({"kind": "harness_plan", "steps": ["a", "b", "c"]})).unwrap();
    assert!(matches!(plan, RunEvent::HarnessPlan { steps } if steps.len() == 3));
    let usage = RunEvent::Usage {
        usage: UsageDelta {
            model_turns: 2,
            tool_calls: 3,
            input_tokens: Some(10),
            ..UsageDelta::default()
        },
    };
    assert_eq!(
        serde_json::to_value(&usage).unwrap(),
        json!({"kind": "usage", "usage": {"model_turns": 2, "tool_calls": 3, "input_tokens": 10}})
    );
    let reject: ResumeDecision = serde_json::from_value(json!({
        "kind": "reject", "approval_id": "0192a1c3-9a8b-7c6d-8e5f-4a3b2c1d0e9f"
    }))
    .unwrap();
    assert!(matches!(
        reject,
        ResumeDecision::Reject { reason: None, .. }
    ));
    assert_eq!(
        serde_json::to_value(AdapterError::VerbNotOffered {
            verb: AdapterVerb::Cancel
        })
        .unwrap(),
        json!({"kind": "verb_not_offered", "verb": "cancel"})
    );
    let refused: AdapterError =
        serde_json::from_value(json!({"kind": "refused", "code": "HAA_CARD_DECIDED"})).unwrap();
    assert!(matches!(refused, AdapterError::Refused { code } if code == "HAA_CARD_DECIDED"));
    let handle = RunHandle {
        run_id: RUN.parse().unwrap(),
        adapter: AdapterKind::SlackThread,
        runtime_session_id: None,
        remote_thread_id: Some("1712345678.000100".into()),
    };
    assert_eq!(
        serde_json::to_value(&handle).unwrap(),
        json!({"run_id": RUN, "adapter": "slack_thread", "remote_thread_id": "1712345678.000100"})
    );
    let snapshot = RunSnapshot {
        run_id: RUN.parse().unwrap(),
        state: RunState::Running,
        end: None,
        checked_at: ordin_types::time::Timestamp::from_unix_millis(1_760_000_000_000).unwrap(),
    };
    let value = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(value["state"], "running");
    assert!(value.get("end").is_none());
    assert_eq!(
        serde_json::to_value(RunEndReason::StoppedAtLimit).unwrap(),
        "stopped_at_limit"
    );
}
