//! Cross-component runs (plan 01, Phase 5): a test `Coordinator` drives a crew over the real Session
//! Supervisor, Approval Service, Rules Engine, Usage Planner and Journal on a temporary SQLite file,
//! with the Runtime Adapter interface's `ScriptedAdapter` (feature `testkit`) behind the members.
//! The Ordin Host implements `Coordinator` in Phase 9; here a test double stands in for it.

mod support;

use ordin_crew_orchestrator::CrewToolCall;
use ordin_journal::PruneRequest;
use ordin_runtime_adapter::{RunEnd, RunEndReason, RunEvent, UsageDelta};
use ordin_types::domain::runs::{CrewRunState, StartupSubject};
use ordin_types::time::Timestamp;
use support::*;

fn script() -> Vec<RunEvent> {
    vec![
        RunEvent::Started {
            runtime_session_id: Some("session-1".into()),
            remote_thread_id: None,
        },
        RunEvent::Usage {
            usage: UsageDelta {
                model_turns: 4,
                tool_calls: 2,
                ..UsageDelta::default()
            },
        },
        RunEvent::Ended {
            end: RunEnd {
                reason: RunEndReason::Completed,
                final_message: Some("Kestrel is cheapest.".into()),
                artefacts: vec![],
            },
        },
    ]
}

#[tokio::test]
async fn a_coordinated_crew_runs_over_the_real_components() {
    let h = Harness::scripted(script()).await;
    let (a, w) = (agent(), agent());
    let mut c = h.charter(&[("analyst", a), ("writer", w)]);
    c.allowed_handoffs = vec![handoff(a, w)];
    // The writer is behind an approval point: a card in the real Approval Service.
    if let ordin_types::common::AgentRef::AgentInstance { agent_instance_id } = w {
        c.guardrails.approval_points = vec![format!("member:{agent_instance_id}")];
    }
    let coord = TestCoordinator::scripted(vec![
        vec![delegate(a, "Compare the quotes")],
        vec![CrewToolCall::Delegate {
            member: w,
            brief: "Write the summary".into(),
            uses_results_of: vec![a],
            uses_inputs: vec![],
        }],
        vec![finish()],
    ]);
    let id = h.start(c, coord.clone()).await;
    let (run, approval) = tokio::join!(h.wait_ended(id), h.decide_pending(true));
    h.wait_reported(&coord).await;
    assert_eq!(run.state, CrewRunState::Finished);
    // The card was the crew's: asked by the crew round that was about to run the writer.
    let record = h.approvals.get(approval).await.unwrap();
    assert!(matches!(
        record.card.asked_by,
        ordin_types::wire::approval::ApprovalAsker::CrewRound { round: 2, .. }
    ));
    assert_eq!(h.host.cards.lock().unwrap().len(), 1);
    // Three coordinator turns and two members of four turns each.
    assert_eq!(run.budget.requests_used, 3 + 4 + 4);
    // The start-up report went to the Journal and to the screens.
    let report = h
        .journal
        .startup_report(StartupSubject::CrewRun { crew_run_id: id })
        .await
        .unwrap()
        .expect("a start-up report");
    assert_eq!(
        report.enforced_rules,
        vec!["Members ask before anything leaves the company."]
    );
    let ready = h.events("start_up_report_ready").await;
    let crew_reports = ready
        .iter()
        .filter(|e| e["subject"]["kind"] == "crew_run")
        .count();
    assert_eq!(crew_reports, 1);
    // The crew's rounds, ending and the supervisor's runs are all in the same journal.
    let changed = h.events("crew_round_changed").await;
    assert!(
        changed.len() >= 5,
        "start, three rounds, the approval pause and the end"
    );
    assert_eq!(h.entries(id, "crew_ended").await.len(), 1);
    // The Usage Planner counted the crew's turns as one piece of work.
    assert!(h.usage.today().await.unwrap().model_turns >= 8);
    // Both member runs belong to the crew: each was started from a crew round.
    let rounds = h.orch.get(id).await.unwrap().1;
    let runs: usize = rounds.iter().map(|r| r.member_runs.len()).sum();
    assert_eq!(runs, 2);
    for round in rounds.iter().flat_map(|r| r.member_runs.iter()) {
        let run = h.supervisor.get(*round).await.unwrap().run;
        assert!(
            matches!(run.origin, ordin_types::domain::runs::RunOrigin::CrewRound { crew_run_id, .. } if crew_run_id == id)
        );
    }
}

#[tokio::test]
async fn a_saved_workflow_runs_over_the_real_components_and_the_journal_prunes_it() {
    let h = Harness::scripted(script()).await;
    let a = agent();
    let mut s = step("1", a, "Compare the quotes");
    s.ordin_checks_result = true;
    let c = saved_charter(
        &h,
        &[("analyst", a)],
        vec![ordin_types::domain::runs::WorkflowStep::Agent(s)],
    );
    let coord = TestCoordinator::new();
    let id = h.start(c, coord.clone()).await;
    let run = h.wait_ended(id).await;
    h.wait_reported(&coord).await;
    assert_eq!(run.state, CrewRunState::Finished);
    assert_eq!(coord.count("round"), 0);
    assert_eq!(coord.count("check_result:1"), 1);
    // The Usage Planner holds the workflow's history under the workflow's work key.
    assert_eq!(run.budget.requests_used, 4 + 1);
    // Pruning: an ended crew older than the cutoff goes with its rounds and step states.
    let participant = h.orch.prune_participant();
    assert_eq!(participant.module(), "crew_orchestrator");
    let kept = participant
        .prune(
            &h.store,
            &PruneRequest {
                cutoff: Timestamp::from_unix_millis(1_000).unwrap(),
                runs_in_progress: vec![],
            },
        )
        .await
        .unwrap();
    assert_eq!(kept, 0, "nothing is older than the cutoff");
    let removed = participant
        .prune(
            &h.store,
            &PruneRequest {
                cutoff: Timestamp::from_unix_millis(Timestamp::now().unix_millis() + 60_000)
                    .unwrap(),
                runs_in_progress: vec![],
            },
        )
        .await
        .unwrap();
    assert_eq!(removed, 1);
    assert!(h.orch.get(id).await.is_err());
    let left: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM rounds) + (SELECT count(*) FROM crew_step_states)",
    )
    .fetch_one(h.pool())
    .await
    .unwrap();
    assert_eq!(left, 0);
}
