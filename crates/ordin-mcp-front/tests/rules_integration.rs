//! Integration with the real Rules Engine (Playbook section 7.2, `ToolProvider` row): a test provider
//! registers, is offered, answers one call the engine allowed, and is refused one it denied. The
//! Journal is real SQLite on a temporary file and the engine decides from a loaded rule set.

mod support;

use ordin_mcp_front::ToolDispatch;
use ordin_types::domain::runs::{ToolRequestDecision, ToolRequestState};
use serde_json::json;
use support::*;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn a_registered_provider_is_offered_answers_an_allowed_call_and_is_refused_a_denied_one() {
    let (env, _sandbox, memory) = started(|_| {}).await;
    memory.set_behaviour(Behaviour::Text("Q3 rates: 4.1%".to_owned()));
    // The rule set refuses `remember` and says nothing of `recall`.
    let session = env.open(&["remember"], &[]).await;

    // Registered and offered.
    let offered = env.front.offered_tools(session.run).unwrap();
    assert!(offered
        .iter()
        .any(|t| t.server.as_str() == "memory" && t.tool == "recall"));
    assert!(offered
        .iter()
        .any(|t| t.server.as_str() == "memory" && t.tool == "remember"));
    let mut bridge = session.connect("memory").await;
    assert_eq!(bridge.list().await, ["recall", "remember"]);

    // A call the engine allowed reaches the provider and is journalled as executed.
    let allowed = bridge.call(1, "recall", json!({"query": "Q3 rates"})).await;
    assert!(!is_error(&allowed), "{allowed}");
    assert_eq!(text_of(&allowed), "Q3 rates: 4.1%");
    assert_eq!(memory.call_count(), 1);
    let call_id = memory.calls.lock().unwrap()[0].0.call_id;
    let request = env
        .journal
        .tool_request(call_id)
        .await
        .unwrap()
        .expect("journalled");
    assert_eq!(request.tool, "mcp__memory__recall");
    assert_eq!(request.decision, Some(ToolRequestDecision::Allow));
    assert_eq!(request.state, ToolRequestState::Executed);

    // A call the engine denied never reaches the provider and is journalled as a denial that did not execute.
    let denied = bridge.call(2, "remember", json!({"query": "a fact"})).await;
    assert!(is_error(&denied));
    assert_eq!(
        text_of(&denied),
        "Refused by the agent's tool policy: remember is not allowed."
    );
    assert_eq!(memory.call_count(), 1, "the provider was not called again");
    assert_eq!(env.tool_requests(session.run).await, 2);
    let refused: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tool_requests WHERE run_id = ?1 AND decision = 'deny' AND state <> 'executed'",
    )
    .bind(session.run.to_string())
    .fetch_one(env.store.sqlite().unwrap())
    .await
    .unwrap();
    assert_eq!(refused, 1);

    // The sandbox route reaches the same decisions.
    let via_sandbox = env
        .front
        .dispatch(
            session.run,
            key("memory", "remember"),
            json!({"query": "x"}),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(via_sandbox.code.as_deref(), Some("MCP_DENIED"));
    assert_eq!(call_codes(session.run), ["", "MCP_DENIED", "MCP_DENIED"]);
}
