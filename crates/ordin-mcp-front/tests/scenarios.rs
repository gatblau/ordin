//! One test per Gherkin scenario of McpFront.md, named after the scenario (X2). The Rules Engine
//! is the real one over a real SQLite Journal, so a scenario's "Rules Engine double" is a rule set
//! that denies, asks or allows the tool.

mod support;

use std::sync::Arc;
use std::time::Duration;

use ordin_approval::PressOrigin;
use ordin_mcp_front::{
    McpError, ProgramProfile, ServerName, ToolCaller, ToolDispatch, ToolFailure, ToolRegistry,
};
use ordin_service_api::blocks::{
    Block, BlockRejection, CheckedReport, ReportBlock, ReportToolArgs,
};
use ordin_types::wire::approval::{ApprovalPress, PressDecision};
use ordin_types::{AgentInstanceId, CrewRunId};
use serde_json::{json, Value};
use support::*;
use tokio_util::sync::CancellationToken;

fn q(text: &str) -> Value {
    json!({ "query": text })
}

async fn decide_when_pending(env: &Env, decision: PressDecision, after: Duration) {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        let pending = env.approvals.list_pending(None, None).await.unwrap().items;
        if let Some(record) = pending.first() {
            tokio::time::sleep(after).await;
            let card_ref = match decision {
                PressDecision::Approve => record.card.buttons.approve.clone(),
                PressDecision::Reject => record.card.buttons.reject.clone(),
            };
            env.approvals
                .decide(
                    ApprovalPress { card_ref, decision },
                    PressOrigin::LocalScreen,
                )
                .await
                .expect("decide");
            return;
        }
        assert!(std::time::Instant::now() < deadline, "no pending approval");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn happy_path_a_session_is_offered_only_its_tools_and_a_call_runs() {
    let (env, _sandbox, memory) = started(|_| {}).await;
    let agent = env.agent(&[], &[]);
    memory.set_offer(move |g| {
        Ok(if g.agent_instance_id == agent {
            vec![key("memory", "recall")]
        } else {
            vec![key("memory", "recall"), key("memory", "remember")]
        })
    });
    memory.set_behaviour(Behaviour::Text("SECRETANSWER42".to_owned()));
    let session = env.open_for(agent).await;
    let names: Vec<&str> = session
        .config
        .servers
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, ["memory", "sandbox", "work"]);
    for launch in &session.config.servers {
        assert_eq!(
            launch.command.to_str().unwrap(),
            "/Applications/ordin.app/Contents/MacOS/ordin-hook"
        );
        assert_eq!(
            launch.args,
            [
                "mcp-bridge",
                launch.name.as_str(),
                "--agent-instance",
                &agent.to_string()
            ]
        );
        assert_eq!(launch.env["ORDIN_SESSION_ID"], session.run.to_string());
        assert_eq!(launch.env["ORDIN_MCP_SERVER"], launch.name.as_str());
        assert_eq!(launch.env["ORDIN_MCP_TOKEN"].len(), 43);
    }
    let mut bridge = session.connect("memory").await;
    assert_eq!(bridge.list().await, ["recall"]);
    let answer = bridge.call(1, "recall", q("Q3 rates")).await;
    assert!(!is_error(&answer));
    assert_eq!(text_of(&answer), "SECRETANSWER42");
    assert_eq!(memory.call_count(), 1);
    assert_eq!(memory.calls.lock().unwrap()[0].0.run_id, session.run);
    let outcomes = call_outcomes(session.run);
    assert_eq!(outcomes, ["ok"]);
    let line = log_lines(&["mcp_front.call", &session.run.to_string()]).remove(0);
    assert!(line.contains("arguments_digest"));
    // Logging.md LOG-ID-1 and LOG-ID-3: the identifiers and names are written as text.
    let fields: Value = serde_json::from_str(&line).expect("a JSON line");
    assert_eq!(fields["event"], "mcp_front.call");
    assert_eq!(fields["server"], "memory");
    assert_eq!(fields["tool"], "recall");
    assert_eq!(fields["via"], "harness");
    let call_id = fields["call_id"].as_str().expect("call_id is text");
    assert!(uuid::Uuid::parse_str(call_id).is_ok(), "{line}");
    let digest = fields["arguments_digest"].as_str().expect("digest is text");
    assert_eq!(digest.len(), 64, "{line}");
    assert!(digest.bytes().all(|b| b.is_ascii_hexdigit()), "{line}");
    assert!(!line.contains("<debug>"), "{line}");
    assert!(log_lines(&["Q3 rates"]).is_empty());
    assert!(log_lines(&["SECRETANSWER42"]).is_empty());
    // The tool request is journalled as executed by the Rules Engine.
    let ctx_call_id = memory.calls.lock().unwrap()[0].0.call_id;
    let request = env
        .journal
        .tool_request(ctx_call_id)
        .await
        .unwrap()
        .expect("journalled");
    assert_eq!(
        request.state,
        ordin_types::domain::runs::ToolRequestState::Executed
    );
}

#[tokio::test]
async fn edge_registration_after_start_is_refused() {
    let (env, _s, _m) = started(|_| {}).await;
    let late = TestProvider::new("late", vec![server("late", vec![spec("t", false)])]);
    assert_eq!(
        env.front.register(late).unwrap_err().code(),
        "MCP_REGISTRY_FROZEN"
    );
}

#[tokio::test]
async fn edge_two_providers_claim_the_same_server() {
    let (env, _s, _m) = started_with(|_| {}, vec![]).await;
    // Registration after start is frozen, so a second hub is built without starting.
    let other_env = self::env(|_| {}).await;
    let first = TestProvider::new(
        "memory-service",
        vec![server("memory", vec![spec("recall", true)])],
    );
    other_env.front.register(first).unwrap();
    let other = TestProvider::new("other", vec![server("memory", vec![spec("recall", true)])]);
    match other_env.front.register(other) {
        Err(McpError::ServerClaimed { server, by }) => {
            assert_eq!(server.as_str(), "memory");
            assert_eq!(by, "memory-service");
        }
        other => panic!("unexpected {other:?}"),
    }
    drop(env);
}

#[tokio::test]
async fn error_a_bridge_with_the_wrong_token_is_refused() {
    let (env, _s, _m) = started(|_| {}).await;
    let session = env.open(&[], &[]).await;
    let wrong = "WRONGTOKENwrongtokenWRONGTOKENwrongtoken123";
    let mut bridge = Bridge::dial(&session.endpoint()).await;
    let reply = bridge
        .handshake(&session.run.to_string(), "memory", wrong)
        .await;
    assert_eq!(
        reply,
        json!({"kind": "refused", "code": "MCP_BRIDGE_UNAUTHENTICATED"})
    );
    assert!(
        bridge.recv_opt(Duration::from_secs(5)).await.is_none(),
        "the core closes"
    );
    assert!(
        log_lines(&[wrong]).is_empty(),
        "the presented token appears in no log line"
    );
    assert!(!log_lines(&["mcp_front.bridge_refused", "MCP_BRIDGE_UNAUTHENTICATED"]).is_empty());
}

#[tokio::test]
async fn error_wrong_arguments_are_answered_without_a_decision() {
    let (env, _s, _m) = started(|_| {}).await;
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("work").await;
    let answer = bridge
        .call(
            1,
            "plan",
            json!({"action": "mark", "position": 9, "mark": "now"}),
        )
        .await;
    assert!(is_error(&answer));
    assert!(text_of(&answer).starts_with("Wrong arguments for plan at /position"));
    assert_eq!(call_codes(session.run), ["MCP_ARGUMENTS_INVALID"]);
    assert_eq!(
        env.tool_requests(session.run).await,
        0,
        "the Rules Engine received no call"
    );
}

#[tokio::test]
async fn error_a_denied_call_is_not_run() {
    let (env, sandbox, _m) = started(|_| {}).await;
    let session = env.open(&["python_sandbox"], &[]).await;
    let mut bridge = session.connect("sandbox").await;
    let answer = bridge.call(1, "python_sandbox", q("print(1)")).await;
    assert!(is_error(&answer));
    assert_eq!(
        text_of(&answer),
        "Refused by the agent's tool policy: python_sandbox is not allowed."
    );
    assert_eq!(sandbox.call_count(), 0);
    assert_eq!(call_codes(session.run), ["MCP_DENIED"]);
}

#[tokio::test]
async fn edge_an_ask_not_decided_within_50_seconds() {
    // The 50 s wait is shortened through `approval_wait`; the answer is the same.
    let (env, _s, memory) = started(|c| c.approval_wait = Duration::from_millis(300)).await;
    let session = env.open(&[], &["remember"]).await;
    let mut bridge = session.connect("memory").await;
    let answer = bridge.call(1, "remember", q("a fact")).await;
    assert!(is_error(&answer));
    assert!(text_of(&answer).starts_with("Waiting for the person's approval."));
    assert_eq!(call_codes(session.run), ["MCP_AWAITING_APPROVAL"]);
    assert_eq!(memory.call_count(), 0);
}

#[tokio::test]
async fn happy_path_an_approved_ask_runs_the_call() {
    let (env, _s, memory) = started(|c| c.approval_wait = Duration::from_secs(20)).await;
    let session = env.open(&[], &["remember"]).await;
    let mut bridge = session.connect("memory").await;
    let (answer, _) = tokio::join!(
        bridge.call(1, "remember", q("a fact")),
        decide_when_pending(&env, PressDecision::Approve, Duration::from_millis(200))
    );
    assert!(!is_error(&answer), "{answer}");
    assert_eq!(memory.call_count(), 1);
}

#[tokio::test]
async fn error_waiting_for_an_approval_fails_the_call_is_denied() {
    let (env, _s, memory) = started(|c| c.approval_wait = Duration::from_secs(20)).await;
    // Marking the approval as used fails, so the wait returns an error.
    env.sql("CREATE TRIGGER block_used BEFORE UPDATE ON rules_approved_calls BEGIN SELECT RAISE(ABORT, 'blocked'); END")
        .await;
    let session = env.open(&[], &["remember"]).await;
    let mut bridge = session.connect("memory").await;
    let (answer, _) = tokio::join!(
        bridge.call(1, "remember", q("a fact")),
        decide_when_pending(&env, PressDecision::Approve, Duration::from_millis(100))
    );
    assert!(is_error(&answer));
    assert_eq!(
        text_of(&answer),
        "This call could not be checked, so it was not run."
    );
    assert_eq!(call_codes(session.run), ["MCP_DECISION_FAILED"]);
    assert_eq!(memory.call_count(), 0);
}

#[tokio::test]
async fn edge_a_provider_times_out() {
    let (env, _s, memory) = started(|c| c.call_timeout = Duration::from_secs(2)).await;
    memory.set_behaviour(Behaviour::Sleep(Duration::from_secs(5), "late".to_owned()));
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("memory").await;
    let started_at = std::time::Instant::now();
    let answer = bridge.call(1, "recall", q("slow")).await;
    assert!(started_at.elapsed() >= Duration::from_secs(2));
    assert!(started_at.elapsed() < Duration::from_secs(5));
    assert!(is_error(&answer));
    assert_eq!(text_of(&answer), "The tool took too long and was stopped.");
    assert_eq!(call_codes(session.run), ["MCP_CALL_TIMEOUT"]);
    assert!(memory.tokens.lock().unwrap()[0].is_cancelled());
}

#[tokio::test]
async fn happy_path_a_plan_is_stated_and_marked() {
    let (env, _s, _m) = started(|_| {}).await;
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("work").await;
    let answer = bridge
        .call(
            1,
            "plan",
            json!({"action": "state", "steps": ["Read the supplier quotes", "Compare the Q3 rates", "Write the summary"]}),
        )
        .await;
    assert_eq!(
        text_of(&answer),
        "Plan noted: 3 steps. Mark each step now when you start it and done when it is finished (plan action mark), so the person can follow your progress."
    );
    assert_eq!(answer["result"]["structuredContent"]["plan_version"], 1);
    let steps = env.journal.current_plan(session.run).await.unwrap();
    assert_eq!(steps.len(), 3);
    assert!(steps
        .iter()
        .all(|s| s.came_from == ordin_types::domain::runs::StepSource::AgentsPlan));
    // spec: ordin-desktop/components/McpFront#MCP-PLAN-START
    // Stating the steps starts the first one, so the person's card never sits on "next" for the whole run.
    assert_eq!(steps[0].mark, ordin_types::wire::run::StepMark::Now);
    assert_eq!(steps[1].mark, ordin_types::wire::run::StepMark::Next);
    let first = bridge
        .call(
            2,
            "plan",
            json!({"action": "mark", "position": 1, "mark": "now"}),
        )
        .await;
    assert!(!is_error(&first), "{first}");
    assert_eq!(text_of(&first), "Marked step 1 now.");
    assert!(!call_codes(session.run).contains(&"MCP_PLAN_ONE_NOW".to_owned()));
    let second = bridge
        .call(
            3,
            "plan",
            json!({"action": "mark", "position": 2, "mark": "now"}),
        )
        .await;
    assert!(is_error(&second));
    assert_eq!(text_of(&second), "Mark step 2 done or skipped first.");
    assert!(call_codes(session.run).contains(&"MCP_PLAN_ONE_NOW".to_owned()));
}

fn rejection_at_part_one() -> BlockRejection {
    BlockRejection {
        code: "BLOCK_SOURCE_MISSING".to_owned(),
        at: "/parts/1/content/source".to_owned(),
        reason: "Give the part a source.".to_owned(),
    }
}

#[tokio::test]
async fn edge_a_report_with_one_failing_part_is_shown_without_it() {
    let (env, _s, _m) = started(|_| {}).await;
    let reduced: ReportBlock = serde_json::from_value({
        let mut v = report_block(2);
        v.as_object_mut().unwrap().remove("kind");
        v
    })
    .unwrap();
    *env.check.report.lock().unwrap() = Some(Ok(CheckedReport {
        block: ReportToolArgs::Report(reduced),
        dropped_parts: vec![rejection_at_part_one()],
    }));
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("work").await;
    let answer = bridge.call(1, "report", report_block(3)).await;
    assert!(!is_error(&answer), "{answer}");
    assert_eq!(
        answer["result"]["structuredContent"]["left_out"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(text_of(&answer).starts_with("Shown: Quarter."));
    assert!(text_of(&answer).contains("1 part(s) left out; see left_out."));
    let stored = env
        .journal
        .report_for_run(session.run)
        .await
        .unwrap()
        .expect("a stored report");
    assert_eq!(stored.parts.len(), 2);
    let shown = env.events("block_shown").await;
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0]["workspace_id"], session.workspace.to_string());
}

#[tokio::test]
async fn edge_a_report_too_large_for_one_event_is_announced_by_id() {
    let (env, _s, _m) = started(|_| {}).await;
    let big = table_block("Big", 300 * 1024);
    let table: ordin_service_api::blocks::TableBlock = serde_json::from_value({
        let mut v = big.clone();
        v.as_object_mut().unwrap().remove("kind");
        v
    })
    .unwrap();
    *env.check.report.lock().unwrap() = Some(Ok(CheckedReport {
        block: ReportToolArgs::Table(table.clone()),
        dropped_parts: Vec::new(),
    }));
    // Amended 2026-01-24 (SA-MCP-BLOCK-01): the block is kept before it is announced.
    let kept = KeptBlocks::new();
    env.front.bind_block_store(kept.clone()).unwrap();
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("work").await;
    let answer = bridge.call(1, "report", big).await;
    assert!(!is_error(&answer), "{answer}");
    let available = env.events("block_available").await;
    assert_eq!(available.len(), 1);
    assert_eq!(available[0]["workspace_id"], session.workspace.to_string());
    assert_eq!(
        available[0]["block_id"],
        answer["result"]["structuredContent"]["block_id"]
    );
    assert!(env.events("block_shown").await.is_empty());
    let calls = kept.calls();
    assert_eq!(calls.len(), 1, "kept once");
    assert_eq!(calls[0].0, session.workspace);
    assert_eq!(
        json!(calls[0].1),
        answer["result"]["structuredContent"]["block_id"]
    );
    assert_eq!(
        serde_json::to_value(&calls[0].2).unwrap(),
        serde_json::to_value(Block::Table(table)).unwrap(),
        "the whole table is kept"
    );
}

/// A table block whose `block_shown` event data serialises to exactly `bytes`.
fn table_of_event_size(bytes: usize) -> (Value, ordin_service_api::blocks::TableBlock) {
    let parse = |v: &Value| -> ordin_service_api::blocks::TableBlock {
        let mut v = v.clone();
        v.as_object_mut().unwrap().remove("kind");
        serde_json::from_value(v).unwrap()
    };
    let size = |table: &ordin_service_api::blocks::TableBlock| {
        let event = ordin_service_api::ServiceEvent::BlockShown {
            workspace_id: ordin_types::WorkspaceId::new(),
            block_id: ordin_types::BlockId::new(),
            block: Block::Table(table.clone()),
            author: None,
        };
        serde_json::to_vec(&event.to_journal_data()).unwrap().len()
    };
    let probe = table_block("Big", 1000);
    let overhead = size(&parse(&probe)) - 1000;
    let raw = table_block("Big", bytes - overhead);
    let table = parse(&raw);
    assert_eq!(size(&table), bytes);
    (raw, table)
}

async fn report_of_table(env: &Env, table: ordin_service_api::blocks::TableBlock) {
    *env.check.report.lock().unwrap() = Some(Ok(CheckedReport {
        block: ReportToolArgs::Table(table),
        dropped_parts: Vec::new(),
    }));
}

// Scenario: Edge — a block between 255 KiB and 256 KiB is kept and shown (SA-MCP-BLOCK-01)
#[tokio::test]
async fn edge_a_block_between_255_and_256_kib_is_kept_and_shown() {
    let (env, _s, _m) = started(|_| {}).await;
    let (raw, table) = table_of_event_size(255 * 1024 + 512);
    report_of_table(&env, table).await;
    let kept = KeptBlocks::new();
    env.front.bind_block_store(kept.clone()).unwrap();
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("work").await;
    let answer = bridge.call(1, "report", raw).await;
    assert!(!is_error(&answer), "{answer}");
    assert_eq!(kept.calls().len(), 1);
    assert_eq!(env.events("block_shown").await.len(), 1);
    assert!(env.events("block_available").await.is_empty());
}

// Scenario: Edge — a small block is not kept
#[tokio::test]
async fn edge_a_small_block_is_not_kept() {
    let (env, _s, _m) = started(|_| {}).await;
    let (raw, table) = table_of_event_size(2 * 1024);
    report_of_table(&env, table).await;
    let kept = KeptBlocks::new();
    env.front.bind_block_store(kept.clone()).unwrap();
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("work").await;
    let answer = bridge.call(1, "report", raw).await;
    assert!(!is_error(&answer), "{answer}");
    assert!(kept.calls().is_empty());
    assert_eq!(env.events("block_shown").await.len(), 1);
}

// Scenario: Error — no store bound (SA-MCP-BLOCK-02)
#[tokio::test]
async fn error_a_large_block_without_a_store() {
    let (env, _s, _m) = started(|_| {}).await;
    let (raw, table) = table_of_event_size(300 * 1024);
    report_of_table(&env, table).await;
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("work").await;
    let answer = bridge.call(1, "report", raw).await;
    assert!(is_error(&answer), "{answer}");
    assert_eq!(
        text_of(&answer),
        "This is too large to show yet; show it as smaller tables or charts."
    );
    assert_eq!(call_codes(session.run), ["MCP_BLOCK_STORE_NOT_BOUND"]);
    assert!(env.events("block_available").await.is_empty());
    assert!(env.events("block_shown").await.is_empty());
}

// Scenario: Error — the store refuses (SA-MCP-BLOCK-03)
#[tokio::test]
async fn error_the_store_refuses_and_nothing_is_stored() {
    let (env, _s, _m) = started(|_| {}).await;
    let mut raw = report_block(3);
    raw["parts"][0]["content"]["rows"][0][0] = json!("x".repeat(300 * 1024));
    let mut report = raw.clone();
    report.as_object_mut().unwrap().remove("kind");
    let report: ReportBlock = serde_json::from_value(report).unwrap();
    *env.check.report.lock().unwrap() = Some(Ok(CheckedReport {
        block: ReportToolArgs::Report(report),
        dropped_parts: Vec::new(),
    }));
    let kept = KeptBlocks::new();
    kept.refuse(ToolFailure {
        code: "OH_DATASTORE_UNAVAILABLE".to_owned(),
        message: "The report could not be saved; try again.".to_owned(),
        retryable: true,
    });
    env.front.bind_block_store(kept.clone()).unwrap();
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("work").await;
    let answer = bridge.call(1, "report", raw).await;
    assert!(is_error(&answer), "{answer}");
    assert_eq!(
        text_of(&answer),
        "The report could not be saved; try again."
    );
    assert_eq!(call_codes(session.run), ["OH_DATASTORE_UNAVAILABLE"]);
    assert!(
        env.journal
            .report_for_run(session.run)
            .await
            .unwrap()
            .is_none(),
        "store_report was not called"
    );
    assert!(env.events("block_available").await.is_empty());
}

// Scenario: Error — the store is bound twice
#[tokio::test]
async fn error_the_block_store_is_bound_once() {
    let e = env(|_| {}).await;
    e.front.bind_block_store(KeptBlocks::new()).unwrap();
    let again = e.front.bind_block_store(KeptBlocks::new()).unwrap_err();
    assert!(matches!(again, McpError::BlockStoreAlreadyBound));
    assert_eq!(again.code(), "MCP_BLOCK_STORE_ALREADY_BOUND");
    assert!(!again.retryable());
}

#[tokio::test]
async fn edge_long_output_is_trimmed_with_a_reference() {
    let (env, _s, memory) = started(|_| {}).await;
    env.front.set_only_needed_parts(true);
    memory.set_behaviour(Behaviour::Text("y".repeat(50_000)));
    let session = env.open(&[], &[]).await;
    let mut memory_bridge = session.connect("memory").await;
    let answer = memory_bridge.call(1, "recall", q("long")).await;
    let text = text_of(&answer);
    let (body, last) = text.rsplit_once('\n').expect("a final line");
    assert!(body.chars().count() <= 12_000);
    assert!(last.contains("rest:"), "{last}");
    let reference = last
        .split('"')
        .nth(1)
        .expect("a quoted reference")
        .to_owned();
    let mut work = session.connect("work").await;
    assert!(work.list().await.contains(&"read_rest".to_owned()));
    let more = work
        .call(
            2,
            "read_rest",
            json!({"reference": reference, "from": 12000}),
        )
        .await;
    assert!(!is_error(&more), "{more}");
    let part = text_of(&more);
    assert!(part.starts_with(&"y".repeat(100)));
    assert!(
        part.ends_with("[Next: from 24000]"),
        "{}",
        &part[part.len() - 30..]
    );
}

#[tokio::test]
async fn happy_path_progress_counts_are_relayed() {
    let (env, sandbox, _m) = started(|_| {}).await;
    sandbox.set_behaviour(Behaviour::Progress(12, Some(40)));
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("sandbox").await;
    let (answer, others) = bridge
        .request(
            1,
            "tools/call",
            json!({"name": "python_sandbox", "arguments": {"query": "go"}, "_meta": {"progressToken": "tok-1"}}),
        )
        .await;
    assert!(!is_error(&answer), "{answer}");
    // The notification may arrive before or after the answer.
    let mut seen = others;
    if seen.is_empty() {
        seen.push(bridge.recv().await);
    }
    let note = seen
        .iter()
        .find(|v| v["method"] == "notifications/progress")
        .expect("a progress notification");
    assert_eq!(note["params"]["progressToken"], "tok-1");
    assert_eq!(note["params"]["progress"], 12);
    assert_eq!(note["params"]["total"], 40);
    let events = env.events("run_progress").await;
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(
        events[0]["progress"]["count"],
        json!({"done": 12, "total": 40})
    );
    assert_eq!(events[0]["run_id"], session.run.to_string());
}

#[tokio::test]
async fn happy_path_the_sandbox_is_given_only_sandbox_callable_tools() {
    let (env, _s, memory) = started(|_| {}).await;
    let agent = env.agent(&["remember"], &[]);
    let session = env.open_for(agent).await;
    let tools = env.front.session_tools(session.run).unwrap();
    let keys: Vec<(String, String)> = tools
        .iter()
        .map(|(k, _)| (k.server.to_string(), k.tool.clone()))
        .collect();
    assert!(keys.contains(&("memory".to_owned(), "recall".to_owned())));
    assert!(!keys.iter().any(|(s, _)| s == "sandbox"));
    // An allowed dispatch reaches the provider with via Sandbox.
    let allowed = env
        .front
        .dispatch(
            session.run,
            key("memory", "recall"),
            q("x"),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(!allowed.is_error);
    assert_eq!(
        memory.calls.lock().unwrap()[0].0.via,
        ordin_mcp_front::CallVia::Sandbox
    );
    // A denied one gets the Rules Engine's refusal, as a harness call does.
    let denied = env
        .front
        .dispatch(
            session.run,
            key("memory", "remember"),
            q("x"),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(denied.code.as_deref(), Some("MCP_DENIED"));
    let mut bridge = session.connect("memory").await;
    let harness = bridge.call(1, "remember", q("x")).await;
    assert_eq!(text_of(&harness), denied.text);
    // A tool that is not sandbox-callable is not dispatched.
    let refused = env
        .front
        .dispatch(
            session.run,
            key("sandbox", "python_sandbox"),
            q("x"),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(refused.code.as_deref(), Some("MCP_TOOL_NOT_OFFERED"));
}

fn mail_provider() -> Arc<TestProvider> {
    TestProvider::new(
        "mail-service",
        vec![server(
            "mail",
            vec![spec("email_search", true), spec("email_read", true)],
        )],
    )
}

#[tokio::test]
async fn happy_path_a_grant_made_during_a_session_reaches_it() {
    let mail = mail_provider();
    mail.set_offer(|_| Ok(vec![key("mail", "email_search")]));
    let (env, _s, _m) = started_with(|_| {}, vec![mail.clone()]).await;
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("mail").await;
    assert_eq!(bridge.list().await, ["email_search"]);
    mail.set_offer(|_| Ok(vec![key("mail", "email_search"), key("mail", "email_read")]));
    env.front.tools_changed(&ServerName::new("mail").unwrap());
    let note = bridge.recv().await;
    assert_eq!(note["method"], "notifications/tools/list_changed");
    // The list is sorted by name.
    assert_eq!(bridge.list().await, ["email_read", "email_search"]);
}

#[tokio::test]
async fn edge_a_tools_changed_whose_offer_fails_withdraws_the_providers_tools() {
    let mail = mail_provider();
    mail.set_offer(|_| Ok(vec![key("mail", "email_search")]));
    let (env, _s, _m) = started_with(|_| {}, vec![mail.clone()]).await;
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("mail").await;
    assert_eq!(bridge.list().await, ["email_search"]);
    mail.set_offer(|_| {
        Err(ToolFailure {
            code: "MAIL_OFFLINE".to_owned(),
            message: "Mail is offline.".to_owned(),
            retryable: true,
        })
    });
    env.front.tools_changed(&ServerName::new("mail").unwrap());
    let note = bridge.recv().await;
    assert_eq!(note["method"], "notifications/tools/list_changed");
    assert!(bridge.list().await.is_empty());
    let answer = bridge.call(5, "email_search", q("x")).await;
    assert!(is_error(&answer));
    assert_eq!(call_codes(session.run), ["MCP_TOOL_NOT_OFFERED"]);
}

#[tokio::test]
async fn edge_a_server_added_under_a_claimed_prefix_after_start() {
    let craft = TestProvider::with_prefix("craft-toolkit", "craft_");
    let (env, _s, _m) = started_with(|_| {}, vec![craft.clone()]).await;
    craft
        .specs
        .lock()
        .unwrap()
        .push(server("craft_artcraft", vec![spec("convert", false)]));
    craft.set_offer(|_| Ok(vec![key("craft_artcraft", "convert")]));
    let session = env.open(&[], &[]).await;
    let names: Vec<&str> = session
        .config
        .servers
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert!(names.contains(&"craft_artcraft"), "{names:?}");
    assert_eq!(
        env.front
            .served_tools(&ServerName::new("craft_artcraft").unwrap()),
        Some(vec!["convert".to_owned()])
    );
    let (all, prefixes) = env.front.server_names();
    assert!(all.iter().any(|n| n.as_str() == "craft_artcraft"));
    assert_eq!(prefixes, ["craft_"]);
}

#[tokio::test]
async fn edge_a_wrightrun_tool_name_and_long_description_register_in_the_agent_profile() {
    let long = |name: &str| {
        let mut s = spec(name, false);
        s.description = "d".repeat(900);
        s
    };
    let agent_env = env(|c| c.profile = ProgramProfile::AgentProfile).await;
    let provider = TestProvider::new(
        "connection-binder",
        vec![server("agent", vec![long("getTicket")])],
    );
    assert!(agent_env.front.register(provider).is_ok());
    let desktop_env = env(|_| {}).await;
    let provider = TestProvider::new(
        "connection-binder",
        vec![server("agent", vec![long("getTicket")])],
    );
    match desktop_env.front.register(provider) {
        Err(McpError::ToolSpecInvalid { rule, tool, .. }) => {
            assert_eq!(rule, "description");
            assert_eq!(tool, "getTicket");
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn happy_path_the_grants_caller_and_scope_reach_the_provider() {
    let (env, _s, memory) = started(|_| {}).await;
    let agent = env.agent(&[], &[]);
    let (run, workspace) = env.begin_run(agent).await;
    let crew = CrewRunId::new();
    let mut grant = env.grant(agent, run, workspace);
    grant.caller = ToolCaller::CrewMember { crew_run_id: crew };
    grant.scope_id = None;
    let config = env.front.open_session(grant).await.unwrap();
    let opened = Opened {
        run,
        workspace,
        agent,
        config,
    };
    let mut bridge = opened.connect("memory").await;
    let answer = bridge.call(1, "recall", q("x")).await;
    assert!(!is_error(&answer));
    let calls = memory.calls.lock().unwrap();
    match calls[0].0.caller {
        ToolCaller::CrewMember { crew_run_id } => assert_eq!(crew_run_id, crew),
        other => panic!("unexpected caller {other:?}"),
    }
    assert!(calls[0].0.scope_id.is_none());
}

#[tokio::test]
async fn edge_closing_a_session_cancels_its_calls_and_tells_providers() {
    let (env, sandbox, memory) = started(|_| {}).await;
    memory.set_behaviour(Behaviour::Sleep(
        Duration::from_secs(60),
        "never".to_owned(),
    ));
    let session = env.open(&[], &[]).await;
    let mut bridge = session.connect("memory").await;
    bridge
        .send(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "recall", "arguments": {"query": "x"}}}))
        .await;
    let waited = std::time::Instant::now();
    while memory.call_count() == 0 {
        assert!(
            waited.elapsed() < Duration::from_secs(15),
            "the call never started"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    env.front.close_session(session.run).await.unwrap();
    assert!(memory.tokens.lock().unwrap()[0].is_cancelled());
    assert!(memory.closed.lock().unwrap().contains(&session.run));
    assert!(sandbox.closed.lock().unwrap().contains(&session.run));
    // The cancelled call gets no answer; the connection is closed.
    assert!(bridge.recv_opt(Duration::from_secs(10)).await.is_none());
    let mut late = Bridge::dial(&session.endpoint()).await;
    let reply = late
        .handshake(&session.run.to_string(), "memory", &session.token())
        .await;
    assert_eq!(
        reply,
        json!({"kind": "refused", "code": "MCP_SESSION_UNKNOWN"})
    );
    // Closing again is idempotent.
    env.front.close_session(session.run).await.unwrap();
}

#[tokio::test]
async fn the_ref_resolver_is_bound_once() {
    let e = env(|_| {}).await;
    e.front.bind_ref_resolver(e.resolver.clone()).unwrap();
    let again = e.front.bind_ref_resolver(e.resolver.clone()).unwrap_err();
    assert_eq!(again.code(), "MCP_REF_RESOLVER_ALREADY_BOUND");
    assert!(!again.retryable());
}

#[tokio::test]
async fn a_report_before_the_resolver_is_bound() {
    let e = env(|_| {}).await;
    e.front.start(CancellationToken::new()).await.unwrap();
    let session = e.open(&[], &[]).await;
    let mut bridge = session.connect("work").await;
    let answer = bridge.call(1, "report", table_block("T", 4)).await;
    assert!(is_error(&answer));
    assert_eq!(text_of(&answer), "Reports can't be shown yet.");
    assert_eq!(call_codes(session.run), ["MCP_REF_RESOLVER_NOT_BOUND"]);
    assert!(e.events("block_shown").await.is_empty());
    assert!(e.events("block_available").await.is_empty());
    let plan = bridge
        .call(
            2,
            "plan",
            json!({"action": "state", "steps": ["a", "b", "c"]}),
        )
        .await;
    assert_eq!(text_of(&plan), "The plan can't be recorded yet.");
    assert_eq!(call_codes(session.run)[1], "MCP_REF_RESOLVER_NOT_BOUND");
}

#[tokio::test]
async fn the_slack_services_server_is_registered_and_named() {
    let slack = TestProvider::new(
        "slack-service",
        vec![server("slack", vec![spec("slack_search", false)])],
    );
    slack.set_offer(|_| Ok(vec![key("slack", "slack_search")]));
    let (env, _s, _m) = started_with(|_| {}, vec![slack]).await;
    let (names, _) = env.front.server_names();
    assert!(names.iter().any(|n| n.as_str() == "slack"));
    let session = env.open(&[], &[]).await;
    let offered = env.front.offered_tools(session.run).unwrap();
    assert!(offered
        .iter()
        .any(|t| t.server.as_str() == "slack" && t.tool == "slack_search"));
}

// ---- Amendment: reading other teams' knowledge (ADR-110) ----

mod adr_110 {
    use super::*;

    #[derive(serde::Deserialize, schemars::JsonSchema)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    pub struct Recall {
        query: String,
        searches: Option<Vec<String>>,
        folders: Option<Vec<String>>,
        result_cap: Option<u32>,
    }

    #[tokio::test]
    async fn no_served_tool_adopts() {
        let (env, _s, _m) = started(|_| {}).await;
        let (names, _) = env.front.server_names();
        assert!(!names.is_empty());
        for server_name in names {
            for tool in env.front.served_tools(&server_name).unwrap_or_default() {
                for banned in ["adopt", "adopt_again", "keep_as_is"] {
                    assert!(!tool.contains(banned), "{server_name}/{tool}");
                }
            }
        }
    }

    #[tokio::test]
    async fn the_amended_recall_schema_reaches_the_session_unchanged() {
        let schema = schema_of::<Recall>();
        let mut recall = spec("recall", true);
        recall.input_schema = schema.clone();
        let memory = TestProvider::new("memory-service", vec![server("memory", vec![recall])]);
        let env = env(|_| {}).await;
        env.front.register(memory).unwrap();
        env.front.bind_ref_resolver(env.resolver.clone()).unwrap();
        env.front.start(CancellationToken::new()).await.unwrap();
        let agent = env.agent(&[], &[]);
        let (run, workspace) = env.begin_run(agent).await;
        let mut grant = env.grant(agent, run, workspace);
        grant.caller = ToolCaller::CrewMember {
            crew_run_id: CrewRunId::new(),
        };
        let config = env.front.open_session(grant).await.unwrap();
        let opened = Opened {
            run,
            workspace,
            agent,
            config,
        };
        let mut bridge = opened.connect("memory").await;
        let tools = bridge.list_raw().await;
        let served = &tools[0]["inputSchema"];
        assert_eq!(served, &schema);
        let properties = served["properties"].as_object().unwrap();
        for name in ["query", "searches", "folders", "result_cap"] {
            assert!(properties.contains_key(name), "{name}");
        }
        assert_eq!(served["additionalProperties"], false);
    }
}

// ---- Amendment: agentic memory as a core capability (ADR-111) ----

mod adr_111 {
    use super::*;

    #[derive(serde::Deserialize, schemars::JsonSchema)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    pub struct Entity {
        name: String,
        other_names: Option<Vec<String>>,
    }

    #[derive(serde::Deserialize, schemars::JsonSchema)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    pub struct Link {
        from: String,
        kind: String,
        to: String,
        ended: Option<String>,
        confidence_permille: Option<u32>,
    }

    #[derive(serde::Deserialize, schemars::JsonSchema)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    pub struct Remember {
        statement: String,
        about: Option<String>,
        fact_class: Option<String>,
        source: Option<String>,
        entities: Option<Vec<Entity>>,
        links: Option<Vec<Link>>,
    }

    type Rules = Arc<std::sync::Mutex<std::collections::HashSet<AgentInstanceId>>>;

    fn memory_provider(rules: Rules) -> Arc<TestProvider> {
        let mut remember = spec("remember", true);
        remember.input_schema = schema_of::<Remember>();
        let provider = TestProvider::new(
            "memory-service",
            vec![server("memory", vec![spec("recall", true), remember])],
        );
        // The Memory Service offers the tools to an agent task session only when memory rules are
        // registered for its agent instance; other callers are offered them as before.
        provider.set_offer(move |g| {
            if matches!(g.caller, ToolCaller::Agent)
                && !rules.lock().unwrap().contains(&g.agent_instance_id)
            {
                return Ok(Vec::new());
            }
            Ok(vec![key("memory", "recall"), key("memory", "remember")])
        });
        provider.set_check(typed::<Remember>);
        provider
    }

    async fn hub_with(rules: Rules) -> (Env, Arc<TestProvider>) {
        let memory = memory_provider(rules);
        let env = env(|_| {}).await;
        env.front.register(memory.clone()).unwrap();
        env.front.bind_ref_resolver(env.resolver.clone()).unwrap();
        env.front.start(CancellationToken::new()).await.unwrap();
        (env, memory)
    }

    async fn hub(_unused: Option<AgentInstanceId>) -> (Env, Arc<TestProvider>) {
        hub_with(Rules::default()).await
    }

    #[tokio::test]
    async fn an_agent_task_session_with_memory_rules_is_offered_memory() {
        let rules = Rules::default();
        let (env, _memory) = hub_with(rules.clone()).await;
        let agent = env.agent(&[], &[]);
        rules.lock().unwrap().insert(agent);
        let session = env.open_for(agent).await;
        let names: Vec<&str> = session
            .config
            .servers
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, ["memory", "work"]);
        let mut bridge = session.connect("memory").await;
        assert_eq!(bridge.list().await, ["recall", "remember"]);
    }

    #[tokio::test]
    async fn an_agent_task_session_without_memory_rules_is_not_offered_memory() {
        let (env, _memory) = hub(None).await;
        let session = env.open(&[], &[]).await;
        let names: Vec<&str> = session
            .config
            .servers
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert!(!names.contains(&"memory"), "{names:?}");
    }

    #[tokio::test]
    async fn the_amended_remember_schema_reaches_the_session_unchanged() {
        let (env, _memory) = hub(None).await;
        let agent = env.agent(&[], &[]);
        let (run, workspace) = env.begin_run(agent).await;
        let mut grant = env.grant(agent, run, workspace);
        grant.caller = ToolCaller::CrewMember {
            crew_run_id: CrewRunId::new(),
        };
        let config = env.front.open_session(grant).await.unwrap();
        let opened = Opened {
            run,
            workspace,
            agent,
            config,
        };
        let mut bridge = opened.connect("memory").await;
        let tools = bridge.list_raw().await;
        let remember = tools.iter().find(|t| t["name"] == "remember").unwrap();
        assert_eq!(remember["inputSchema"], schema_of::<Remember>());
        let properties = remember["inputSchema"]["properties"].as_object().unwrap();
        for name in [
            "statement",
            "about",
            "fact_class",
            "source",
            "entities",
            "links",
        ] {
            assert!(properties.contains_key(name), "{name}");
        }
        assert_eq!(remember["inputSchema"]["additionalProperties"], false);
    }

    #[tokio::test]
    async fn error_a_link_object_with_an_unknown_field_is_refused_before_the_provider() {
        let (env, memory) = hub(None).await;
        let agent = env.agent(&[], &[]);
        let (run, workspace) = env.begin_run(agent).await;
        let mut grant = env.grant(agent, run, workspace);
        grant.caller = ToolCaller::CrewMember {
            crew_run_id: CrewRunId::new(),
        };
        let config = env.front.open_session(grant).await.unwrap();
        let opened = Opened {
            run,
            workspace,
            agent,
            config,
        };
        let mut bridge = opened.connect("memory").await;
        let answer = bridge
            .call(
                1,
                "remember",
                json!({"statement": "s", "links": [{"from": "Borealis", "kind": "supplies", "to": "packaging", "why": "x"}]}),
            )
            .await;
        assert!(is_error(&answer));
        assert!(
            text_of(&answer).contains("/links/0/why"),
            "{}",
            text_of(&answer)
        );
        assert_eq!(call_codes(run), ["MCP_ARGUMENTS_INVALID"]);
        assert_eq!(memory.call_count(), 0);
    }
}

// ---- Amendment: knowledge agents (ADR-113) ----

mod adr_113 {
    use super::*;

    #[derive(serde::Deserialize, schemars::JsonSchema)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    pub struct Recall {
        query: String,
        searches: Option<Vec<String>>,
        folders: Option<Vec<String>>,
        result_cap: Option<u32>,
        #[serde(default)]
        wider: bool,
    }

    async fn hub() -> (Env, Arc<TestProvider>) {
        let mut recall = spec("recall", true);
        recall.input_schema = schema_of::<Recall>();
        let memory = TestProvider::new("memory-service", vec![server("memory", vec![recall])]);
        memory.set_check(typed::<Recall>);
        let env = env(|_| {}).await;
        env.front.register(memory.clone()).unwrap();
        env.front.bind_ref_resolver(env.resolver.clone()).unwrap();
        env.front.start(CancellationToken::new()).await.unwrap();
        (env, memory)
    }

    #[tokio::test]
    async fn the_amended_recall_schema_reaches_the_session_unchanged() {
        let (env, _memory) = hub().await;
        let agent = env.agent(&[], &[]);
        let (run, workspace) = env.begin_run(agent).await;
        let mut grant = env.grant(agent, run, workspace);
        grant.caller = ToolCaller::CrewMember {
            crew_run_id: CrewRunId::new(),
        };
        let config = env.front.open_session(grant).await.unwrap();
        let opened = Opened {
            run,
            workspace,
            agent,
            config,
        };
        let mut bridge = opened.connect("memory").await;
        let tools = bridge.list_raw().await;
        let schema = &tools[0]["inputSchema"];
        let properties = schema["properties"].as_object().unwrap();
        for name in ["query", "searches", "folders", "result_cap", "wider"] {
            assert!(properties.contains_key(name), "{name}");
        }
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["properties"]["wider"]["type"], "boolean");
    }

    #[tokio::test]
    async fn error_wider_of_the_wrong_type_is_refused_before_the_provider() {
        let (env, memory) = hub().await;
        let session = env.open(&[], &[]).await;
        let mut bridge = session.connect("memory").await;
        let answer = bridge
            .call(
                1,
                "recall",
                json!({"query": "Borealis payment terms", "wider": "yes"}),
            )
            .await;
        assert!(is_error(&answer));
        assert!(text_of(&answer).contains("/wider"), "{}", text_of(&answer));
        assert_eq!(call_codes(session.run), ["MCP_ARGUMENTS_INVALID"]);
        assert_eq!(memory.call_count(), 0);
    }

    #[tokio::test]
    async fn no_served_tool_is_a_knowledge_agents_endpoint_tool() {
        let (env, _s, _m) = started(|_| {}).await;
        let (names, _) = env.front.server_names();
        for server_name in names {
            for tool in env.front.served_tools(&server_name).unwrap_or_default() {
                assert!(
                    tool != "knowledge_search" && tool != "knowledge_contents",
                    "{tool}"
                );
            }
        }
    }

    #[tokio::test]
    async fn the_front_opens_no_tcp_port_in_the_agent_profile() {
        let (env, _s, _m) = started(|c| c.profile = ProgramProfile::AgentProfile).await;
        let session = env.open(&[], &[]).await;
        let endpoint = session.endpoint();
        #[cfg(unix)]
        {
            use std::os::unix::fs::{FileTypeExt, PermissionsExt};
            // The endpoint every bridge is given is a path to a local socket, not a host and port.
            assert!(endpoint.starts_with('/'), "{endpoint}");
            let meta = std::fs::metadata(&endpoint).unwrap();
            assert!(meta.file_type().is_socket());
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
            let folder =
                std::fs::metadata(std::path::Path::new(&endpoint).parent().unwrap()).unwrap();
            assert_eq!(folder.permissions().mode() & 0o777, 0o700);
        }
        // On Windows the endpoint is the local named pipe, not a host and port, and a bridge opens it.
        #[cfg(windows)]
        {
            assert!(endpoint.starts_with(r"\\.\pipe\ordin-mcp-"), "{endpoint}");
            session
                .connect(session.config.servers[0].name.as_str())
                .await;
        }
    }
}

// Scenario: Security — a session limited to one server (SA-MCP-SRV-01)
// spec: ordin-desktop/components/McpFront#SA-MCP-SRV-01
#[tokio::test]
async fn security_a_session_limited_to_one_server() {
    let (env, _sandbox, memory) = started(|_| {}).await;
    let agent = env.agent(&[], &[]);
    // Without a limit every server is launched, the built-in `work` with them.
    let (run, workspace) = env.begin_run(agent).await;
    let all = env
        .front
        .open_session(env.grant(agent, run, workspace))
        .await
        .expect("open_session");
    let names: Vec<&str> = all.servers.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["memory", "sandbox", "work"]);
    // With `only_servers: ["memory"]` only server `memory` is launched and offered.
    let (run, workspace) = env.begin_run(agent).await;
    let mut grant = env.grant(agent, run, workspace);
    grant.only_servers = Some(vec![ServerName::new("memory").unwrap()]);
    let limited = env.front.open_session(grant).await.expect("open_session");
    let names: Vec<&str> = limited.servers.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["memory"]);
    // A provider that widens its offer later still reaches only the named server.
    memory.set_offer(|_| Ok(vec![key("memory", "recall"), key("memory", "remember")]));
    env.front.tools_changed(&ServerName::new("memory").unwrap());
    // An empty list offers nothing.
    let (run, workspace) = env.begin_run(agent).await;
    let mut grant = env.grant(agent, run, workspace);
    grant.only_servers = Some(vec![]);
    let none = env.front.open_session(grant).await.expect("open_session");
    assert!(none.servers.is_empty());
}

// FU-013 (plan 07 Phase 10): a minutes run's whole offer through the MCP Front is exactly
// `ordin/keep_minutes` while another provider offers Ordin sessions its tools. The Ordin Host's
// half — the run asks for `["ordin"]` and its own offer is `keep_minutes` — is tested in
// `ordin-host` (`security_a_minutes_run_asks_for_server_ordin_only`).
// spec: ordin-desktop/components/McpFront#SA-MCP-SRV-01
#[tokio::test]
async fn security_a_minutes_run_is_offered_exactly_keep_minutes() {
    let ordin = TestProvider::new(
        "ordin-host",
        vec![server("ordin", vec![spec("keep_minutes", false)])],
    );
    let knowledge = TestProvider::new(
        "knowledge",
        vec![server(
            "knowledge",
            vec![spec("note_write", false), spec("note_retire", false)],
        )],
    );
    let (env, _s, _m) = started_with(|_| {}, vec![ordin.clone(), knowledge.clone()]).await;
    let agent = env.agent(&[], &[]);
    let (run, workspace) = env.begin_run(agent).await;
    let mut grant = env.grant(agent, run, workspace);
    grant.caller = ordin_mcp_front::ToolCaller::Ordin;
    grant.only_servers = Some(vec![ServerName::new("ordin").unwrap()]);
    let config = env.front.open_session(grant).await.expect("open_session");
    let launched: Vec<&str> = config.servers.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        launched,
        ["ordin"],
        "no other provider's server is launched"
    );
    let session = Opened {
        run,
        workspace,
        agent,
        config,
    };
    let mut bridge = session.connect("ordin").await;
    assert_eq!(bridge.list().await, ["keep_minutes"]);
    // The knowledge provider widens its offer later; the run still sees nothing of it.
    env.front
        .tools_changed(&ServerName::new("knowledge").unwrap());
    assert_eq!(bridge.list().await, ["keep_minutes"]);
}
