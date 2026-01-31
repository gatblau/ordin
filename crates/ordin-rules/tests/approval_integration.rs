//! Cross-component tests with the real Approval Service and Journal (plan 01, Phase 4):
//! - `approval_integration`: a call needing approval creates a pending approval;
//! - the phase behavioural check: a scripted session asks for a tool call that a rule marks *ask*,
//!   the engine creates a pending approval, approving it lets the session continue, and every step
//!   is journalled.

mod support;

use futures::StreamExt;
use ordin_approval::PressOrigin;
use ordin_rules::{CallContext, EndDecision, ProgramKind};
use ordin_runtime_adapter::{
    AdapterContext, AdapterKind, PauseReason, ResumeDecision, RunEnd, RunEndReason, RunEvent,
    RunHandle, RuntimeAdapter, ScriptedAdapter,
};
use ordin_types::domain::agents::Authority;
use ordin_types::domain::runs::{ApprovalState, ToolRequestDecision, ToolRequestState};
use ordin_types::wire::approval::{ApprovalPress, PressDecision};
use ordin_types::wire::rules::{RuleSource, ToolOrigin, Verdict};
use ordin_types::ApprovalId;
use serde_json::json;
use support::{fixture, rule_set, running, Fixture};

async fn approve(f: &Fixture, approval: ApprovalId) {
    let record = f.approvals.get(approval).await.expect("the approval");
    f.approvals
        .decide(
            ApprovalPress {
                card_ref: record.card.buttons.approve.clone(),
                decision: PressDecision::Approve,
            },
            PressOrigin::LocalScreen,
        )
        .await
        .expect("approve");
}

#[tokio::test]
async fn a_call_needing_approval_creates_a_pending_approval() {
    let f = fixture(ProgramKind::Desktop).await;
    let set = rule_set(Authority::SystemChanging, &f.folder("eng-1"));
    let r = running(&f, set, None).await;
    let call = r.call(
        "Bash",
        ToolOrigin::BuiltIn,
        json!({"command": "make deploy"}),
    );
    let outcome = f
        .engine
        .decide_tool_call(&call, &CallContext::default())
        .await;
    assert!(matches!(outcome.decision.verdict, Verdict::Ask { .. }));
    assert_eq!(outcome.decision.rule.source, RuleSource::AppShellPolicy);
    let approval = outcome.approval_id.expect("an approval id");
    let record = f
        .approvals
        .get(approval)
        .await
        .expect("the approval exists");
    assert_eq!(record.state, ApprovalState::Pending);
    let pending = f.approvals.list_pending(None, None).await.unwrap().items;
    assert_eq!(pending.len(), 1);
    let request = f
        .journal
        .tool_request(outcome.tool_request_id)
        .await
        .unwrap()
        .expect("the ToolRequest is journalled");
    assert_eq!(request.decision, Some(ToolRequestDecision::Ask));
    assert_eq!(request.approval_id, Some(approval));
}

#[tokio::test]
async fn behavioural_check_an_asked_call_continues_once_approved() {
    let f = fixture(ProgramKind::Desktop).await;
    let set = rule_set(Authority::SystemChanging, &f.folder("eng-1"));
    let r = running(&f, set, None).await;
    let call = r.call(
        "Bash",
        ToolOrigin::BuiltIn,
        json!({"command": "make deploy"}),
    );

    // The session asks for the tool call; the engine marks it *ask*.
    let asked = f
        .engine
        .decide_tool_call(&call, &CallContext::default())
        .await;
    let approval = asked.approval_id.expect("an approval");

    // The scripted session pauses on that approval, resumes and ends.
    let adapter = ScriptedAdapter {
        kind: AdapterKind::ClaudeLocal,
        unsupported: &[],
        script: vec![
            RunEvent::Started {
                runtime_session_id: Some("session-1".to_owned()),
                remote_thread_id: None,
            },
            RunEvent::Paused {
                reason: PauseReason::Approval {
                    approval_id: approval,
                },
            },
            RunEvent::Resumed,
            RunEvent::Ended {
                end: RunEnd {
                    reason: RunEndReason::Completed,
                    final_message: Some("Deployed.".to_owned()),
                    artefacts: Vec::new(),
                },
            },
        ],
        snapshot: None,
        start_error: None,
    };
    let ctx = AdapterContext {
        correlation_id: "behavioural-check".to_owned(),
        deadline: std::time::Instant::now() + std::time::Duration::from_secs(30),
        cancel: tokio_util::sync::CancellationToken::new(),
    };
    let handle = RunHandle {
        run_id: r.run,
        adapter: AdapterKind::ClaudeLocal,
        runtime_session_id: Some("session-1".to_owned()),
        remote_thread_id: None,
    };
    let mut events = adapter.events(&ctx, &handle).expect("events");
    let mut repeated = None;
    let mut ended = false;
    while let Some(event) = events.next().await {
        match event {
            RunEvent::Paused {
                reason: PauseReason::Approval { approval_id },
            } => {
                assert_eq!(approval_id, approval);
                // The person approves; the session is resumed and repeats the call, which spends
                // the approved-once record (a caller that waits instead spends it on waking).
                approve(&f, approval_id).await;
                adapter
                    .resume(&ctx, &handle, ResumeDecision::Approve { approval_id })
                    .await
                    .expect("resume");
            }
            RunEvent::Resumed => {
                // The session makes the same call again; the approval lets it through once.
                repeated = Some(
                    f.engine
                        .decide_tool_call(&call, &CallContext::default())
                        .await,
                );
            }
            RunEvent::Ended { end } => {
                assert_eq!(end.reason, RunEndReason::Completed);
                assert!(matches!(
                    f.engine.decide_run_end(r.run).await,
                    EndDecision::End
                ));
                f.engine.end_run(r.run).await;
                ended = true;
            }
            _ => {}
        }
    }
    assert!(ended);
    let repeated = repeated.expect("the call was repeated after the resume");
    assert!(matches!(repeated.decision.verdict, Verdict::Allow));
    assert_eq!(repeated.decision.rule.rule_id, "app.approved_once");

    // Every step is journalled: both ToolRequests, the approval and the run's start.
    for (id, decision) in [
        (asked.tool_request_id, ToolRequestDecision::Ask),
        (repeated.tool_request_id, ToolRequestDecision::Allow),
    ] {
        let request = f
            .journal
            .tool_request(id)
            .await
            .unwrap()
            .expect("journalled");
        assert_eq!(request.state, ToolRequestState::Decided);
        assert_eq!(request.decision, Some(decision));
    }
    assert_eq!(
        f.approvals.get(approval).await.unwrap().state,
        ApprovalState::Approved
    );
    let starts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM journal_entries WHERE kind = 'rules_run_start' AND run_id = ?1",
    )
    .bind(r.run.to_string())
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!(starts, 1);
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM rules_run_facts")
        .fetch_one(f.pool())
        .await
        .unwrap();
    assert_eq!(rows, 0, "the run's facts are dropped when it ends");
}
