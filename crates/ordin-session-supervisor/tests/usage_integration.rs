//! Cross-component runs: the real Usage Planner, the real Run Queue (`DatastoreRunQueue`) and the
//! Runtime Adapter interface's `ScriptedAdapter` (feature `testkit`), on a temporary SQLite file.

mod support;

use std::sync::Arc;

use ordin_run_queue::RunQueue;
use ordin_runtime_adapter::{
    AdapterKind, RunEnd, RunEndReason, RunEvent, RunProgress, ScriptedAdapter, UsageDelta,
};
use ordin_session_supervisor::{ProgramKind, SessionSupervisor};
use ordin_types::domain::agent_profile::{RunEndKind, RunRecordState};
use ordin_types::domain::runs::RunState;
use ordin_types::RunId;
use ordin_usage_planner::WorkKind;
use support::*;

fn script() -> Vec<RunEvent> {
    vec![
        RunEvent::Started {
            runtime_session_id: Some("session-1".into()),
            remote_thread_id: None,
        },
        RunEvent::Progress {
            progress: RunProgress {
                line: "Reading the quotes".into(),
                count: None,
            },
        },
        RunEvent::Usage {
            usage: UsageDelta {
                model_turns: 4,
                tool_calls: 2,
                input_tokens: Some(1_000),
                output_tokens: Some(200),
                cached_input_tokens: Some(100),
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

fn scripted() -> Arc<ScriptedAdapter> {
    Arc::new(ScriptedAdapter {
        kind: AdapterKind::CopilotLocal,
        unsupported: &[],
        script: script(),
        snapshot: None,
        start_error: None,
    })
}

/// A node whose local runs go to the scripted adapter (the profile's first harness is Copilot).
fn scripted_node(h: &Harness, program: ProgramKind, instance: &str) -> Arc<SessionSupervisor> {
    let mut cfg = config(program, instance);
    cfg.local_adapter = AdapterKind::CopilotLocal;
    let node = h.another(cfg);
    node.sup.register_adapter(scripted()).unwrap();
    node.sup.clone()
}

async fn wait_ended(sup: &SessionSupervisor, run: RunId) -> ordin_types::domain::runs::Run {
    for _ in 0..500 {
        let view = sup.get(run).await.unwrap();
        if view.run.state == RunState::Completed {
            return view.run;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("the scripted run never completed");
}

#[tokio::test]
async fn a_task_runs_on_the_scripted_adapter_and_the_planner_counts_it() {
    let h = Harness::desktop().await;
    let sup = scripted_node(&h, ProgramKind::Desktop, "desktop");
    let before = h.usage.today().await.unwrap().model_turns;
    let run = sup
        .submit_task(task("compare-quotes"), WorkKind::Documents, false, None)
        .await
        .unwrap();
    let done = wait_ended(&sup, run).await;
    assert_eq!(done.usage.model_turns, 4);
    assert_eq!(done.usage.tool_calls, 2);
    assert_eq!(done.usage.tokens, Some(1_200));
    assert_eq!(done.usage.tokens_from_cache, Some(100));
    assert_eq!(done.runtime_session_id.as_deref(), Some("session-1"));
    assert_eq!(done.end_reason.as_deref(), Some("Kestrel is cheapest."));
    assert_eq!(h.usage.today().await.unwrap().model_turns, before + 4);
    assert_eq!(h.events("run_progress").await.len(), 1);
}

#[tokio::test]
async fn an_agent_profile_record_runs_on_the_scripted_adapter() {
    let h = Harness::desktop().await;
    let sup = scripted_node(&h, ProgramKind::AgentSingle, "runs-1");
    let record = h
        .enqueue(
            chat("U1"),
            message("Compare the quotes"),
            serde_json::json!({}),
        )
        .await;
    sup.start_queue_workers();
    let view = h.wait_record(&record.run_id, RunRecordState::Ended).await;
    assert_eq!(view.record.end_kind, Some(RunEndKind::Completed));
    assert_eq!(view.record.result.as_deref(), Some("Kestrel is cheapest."));
    assert_eq!(view.record.stretch_summaries.len(), 1);
    let events = h.queue.events_after(&record.run_id, 0, 100).await.unwrap();
    assert!(events
        .iter()
        .any(|e| e.data.to_string().contains("Reading the quotes")));
    let run = RunId::parse(&record.run_id).unwrap();
    let done = sup.get(run).await.unwrap().run;
    assert_eq!(done.state, RunState::Completed);
    assert_eq!(done.usage.model_turns, 4);
}

#[tokio::test]
async fn the_prune_participant_removes_ended_runs_and_their_tasks() {
    let h = Harness::desktop().await;
    let sup = scripted_node(&h, ProgramKind::Desktop, "desktop");
    let run = sup
        .submit_task(task("compare-quotes"), WorkKind::Documents, false, None)
        .await
        .unwrap();
    wait_ended(&sup, run).await;
    let kept = h.start_local(None).await;
    h.wait_state(kept, RunState::Running).await;
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    let participant = sup.prune_participant();
    assert_eq!(participant.module(), "session_supervisor");
    let removed = participant
        .prune(
            &h.store,
            &ordin_journal::PruneRequest {
                cutoff: ordin_types::time::Timestamp::now(),
                runs_in_progress: vec![kept],
            },
        )
        .await
        .unwrap();
    assert_eq!(removed, 2);
    assert_eq!(h.count("SELECT count(*) FROM runs").await, 1);
    assert_eq!(h.count("SELECT count(*) FROM tasks").await, 0);
}
