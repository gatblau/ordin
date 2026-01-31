//! `ordin.send_message` starts Ordin's Claude Code session through the real Session Supervisor; the
//! session plays a script on the harness's adapter (plan 01, Phase 9).

mod support;

use ordin_runtime_adapter::AdapterKind;
use ordin_types::domain::runs::RunState;
use support::*;

async fn run_state(w: &World, run_id: ordin_types::RunId) -> Option<RunState> {
    w.h.supervisor.get(run_id).await.ok().map(|v| v.run.state)
}

async fn messages(w: &World, author: &str) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT m.text FROM ordin_messages m JOIN ordin_conversations c ON c.id = m.conversation_id \
         WHERE c.workspace_id = ? AND m.author = ? ORDER BY m.rowid",
    )
    .bind(w.h.workspace.to_string())
    .bind(author)
    .fetch_all(w.h.pool())
    .await
    .unwrap()
}

#[tokio::test]
async fn a_message_starts_ordins_session_through_the_real_supervisor() {
    let w = World::new().await;
    let ordin = w.ordin();
    w.h.local.behave(move |start, _| {
        assert!(matches!(
            start.target,
            ordin_types::wire::run::RunTarget::AgentInstance { agent_instance_id } if agent_instance_id == ordin
        ));
        ordin_script(&["Hello, I am Ordin.", "Kestrel is cheapest.", "Northwind is second."])
    });
    let ws = w.h.workspace;
    let sent = w
        .host
        .send_message(ws, "Compare the Q3 quotes.".to_owned(), vec![])
        .await
        .expect("the message is accepted");
    assert!(sent.accepted);

    // One run of Ordin's own agent instance, on Claude Code's local adapter, held by the Supervisor.
    let run_id = w.host.current_run(ws).expect("Ordin has a run");
    assert_eq!(w.ordin_runs(), vec![run_id]);
    let view =
        w.h.supervisor
            .get(run_id)
            .await
            .expect("the Supervisor holds the run");
    assert_eq!(view.run.adapter, AdapterKind::ClaudeLocal);
    assert!(matches!(
        view.run.agent,
        ordin_types::common::AgentRef::AgentInstance { agent_instance_id } if agent_instance_id == ordin
    ));
    // The seed went as the session's first input, opening with the fixed instructions.
    let start = w.h.local.starts.lock().unwrap()[0].clone();
    match start.input {
        ordin_runtime_adapter::StartInput::FreeText { text } => assert!(!text.is_empty()),
        other => panic!("the seed is free text, got {other:?}"),
    }

    // The greeting answers the seed and is not shown (OH-SEED-1); the person's message is the next
    // turn, and only its answer is stored as Ordin's.
    // spec: ordin-desktop/components/OrdinHost#OH-SEED-1
    assert!(
        until(5, || w
            .turns_to(run_id)
            .iter()
            .any(|t| t.ends_with("Compare the Q3 quotes.")))
        .await
    );
    let mut stored = vec![];
    for _ in 0..250 {
        stored = messages(&w, "ordin").await;
        if !stored.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(stored, vec!["Kestrel is cheapest."]);
    assert_eq!(messages(&w, "person").await, vec!["Compare the Q3 quotes."]);
    assert_eq!(run_state(&w, run_id).await, Some(RunState::Paused));

    // A second message goes to the same session.
    w.host
        .send_message(ws, "And the second cheapest?".to_owned(), vec![])
        .await
        .expect("the second message is accepted");
    assert!(until(5, || w.turns_to(run_id).len() == 2).await);
    assert_eq!(w.host.current_run(ws), Some(run_id));
    assert_eq!(w.ordin_runs().len(), 1, "no second session");
}
