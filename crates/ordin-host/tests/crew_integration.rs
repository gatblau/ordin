//! A crew run over the real Crew Orchestrator, Session Supervisor, Rules Engine, Approval Service,
//! Usage Planner and Journal calls the Ordin Host as its `Coordinator` (plan 01, Phase 9; §7.2).
//! Ordin's own session is driven by the test: it reads each turn the host sends, makes Ordin's tool
//! call through the host's `ToolProvider` (after the Rules Engine allows it, as the MCP Front would),
//! then ends the turn.

mod support;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_host::ports::DirectoryAgent;
use ordin_host::testkit::agent;
use ordin_mcp_front::{CallVia, ProgressSink, ServerName, ToolCallContext, ToolCaller, ToolKey};
use ordin_rules::{CallContext, RunStartFacts, ToolClass};
use ordin_runtime_adapter::{ResumeDecision, RunEndReason, RunEvent};
use ordin_service_api::blocks::Block;
use ordin_types::domain::runs::CrewRunState;
use ordin_types::wire::rules::{DecisionPoint, ToolCall, ToolOrigin, Verdict};
use ordin_types::wire::run::RunTarget;
use ordin_types::{AgentInstanceId, CrewCharterId, RunId, ToolRequestId, WorkspaceId};
use support::orch::{self, Act};
use support::*;
use tokio_util::sync::CancellationToken;

/// What Ordin does with one turn: the tool calls it makes, then the text that ends the turn.
type Brain = Arc<dyn Fn(&str) -> (Vec<(&'static str, serde_json::Value)>, String) + Send + Sync>;

/// Every turn Ordin received and every tool call it made, with the Rules Engine's verdict.
#[derive(Default)]
struct Log {
    turns: Vec<String>,
    calls: Vec<(String, Result<String, String>)>,
    verdicts: Vec<String>,
}

fn ctx(w: &World, run_id: RunId) -> ToolCallContext {
    ToolCallContext {
        run_id,
        workspace_id: w.h.workspace,
        agent_instance_id: w.ordin(),
        caller: ToolCaller::Ordin,
        scope_id: None,
        coding_task_id: None,
        call_id: ToolRequestId::new(),
        correlation_id: "crew-integration".to_owned(),
        deadline: std::time::Instant::now() + Duration::from_secs(30),
        cancel: CancellationToken::new(),
        progress: ProgressSink::detached(),
        only_needed_parts: false,
        via: CallVia::Harness,
    }
}

/// The Rules Engine's start of a run, as the program does it when the harness opens a session.
async fn begin(w: &World, run_id: RunId, agent: AgentInstanceId, ws: WorkspaceId) {
    // The Package Inspector loads each installed agent's rule set; here the test does.
    let _ = w.h.rules.load_rule_set(orch::rule_set(
        ordin_types::domain::agents::Authority::SystemChanging,
        agent,
    ));
    w.h.rules
        .begin_run(RunStartFacts {
            run_id,
            agent_instance_id: agent,
            workspace_id: ws,
            crew_charter_id: None,
            command: None,
            text: String::new(),
            scope: None,
            asker: None,
            started_by: "person".into(),
            started_by_id: "local".into(),
        })
        .await
        .expect("the Rules Engine records the run");
}

/// One of Ordin's tool calls: the Rules Engine first (as the MCP Front asks it), then the provider.
async fn ordin_tool(
    w: &World,
    run_id: RunId,
    tool: &str,
    args: serde_json::Value,
    log: &Mutex<Log>,
) {
    let outcome =
        w.h.rules
            .decide_tool_call(
                &ToolCall {
                    run_id,
                    agent_instance_id: w.ordin(),
                    workspace_id: w.h.workspace,
                    point: DecisionPoint::BeforeToolCall,
                    tool: format!("mcp__ordin__{tool}"),
                    origin: ToolOrigin::OrdinMcpFront {
                        server: "ordin".into(),
                    },
                    arguments: args.clone(),
                },
                &CallContext {
                    subagent: None,
                    class: None,
                },
            )
            .await;
    let verdict = match &outcome.decision.verdict {
        Verdict::Allow => "allow".to_owned(),
        Verdict::Deny { reason } => format!("deny: {reason}"),
        Verdict::Ask { .. } => "ask".to_owned(),
    };
    log.lock()
        .unwrap()
        .verdicts
        .push(format!("{tool}: {verdict}"));
    if verdict != "allow" {
        return;
    }
    let provider = w.host.tool_provider();
    let key = ToolKey {
        server: ServerName::new("ordin").unwrap(),
        tool: tool.to_owned(),
    };
    let result = provider.call(ctx(w, run_id), &key, args).await;
    w.h.rules
        .record_outcome(outcome.tool_request_id, result.is_ok())
        .await;
    log.lock()
        .unwrap()
        .calls
        .push((tool.to_owned(), result.map(|o| o.text).map_err(|f| f.code)));
}

/// Plays Ordin's session: greets each new run, answers each turn with the brain's calls and text.
fn drive(w: Arc<World>, brain: Brain, log: Arc<Mutex<Log>>, stop: CancellationToken) {
    tokio::spawn(async move {
        let mut greeted: HashSet<RunId> = HashSet::new();
        let mut begun: HashSet<RunId> = HashSet::new();
        let mut handled = 0usize;
        loop {
            if stop.is_cancelled() {
                return;
            }
            // Every run the adapter started passes the Rules Engine's run start.
            let starts: Vec<(RunId, AgentInstanceId)> =
                w.h.local
                    .starts
                    .lock()
                    .unwrap()
                    .iter()
                    .filter_map(|s| match s.target {
                        RunTarget::AgentInstance { agent_instance_id } => {
                            Some((s.run_id, agent_instance_id))
                        }
                        RunTarget::RemoteAgent { .. } => None,
                    })
                    .collect();
            for (run_id, agent_id) in starts {
                if begun.insert(run_id) {
                    begin(&w, run_id, agent_id, w.h.workspace).await;
                }
            }
            if let Some(run_id) = w.host.current_run(w.h.workspace) {
                if w.ordin_runs().contains(&run_id) {
                    if greeted.insert(run_id) {
                        handled = 0;
                        w.ordin_says(run_id, "Ready.");
                    }
                    let turns = w.turns_to(run_id);
                    while handled < turns.len() {
                        let turn = turns[handled].clone();
                        handled += 1;
                        log.lock().unwrap().turns.push(turn.clone());
                        let (calls, text) = brain(&turn);
                        for (tool, args) in calls {
                            ordin_tool(&w, run_id, tool, args, &log).await;
                        }
                        w.h.local.push(run_id, RunEvent::Resumed);
                        w.ordin_says(run_id, &text);
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
}

fn ordin_session() -> orch::Plan {
    vec![orch::started(), Act::Hang]
}

fn workflow_yaml() -> String {
    "format: 3
name: Quote check
purpose: Compare the Q3 quotes
inputs: []
agents:
  - key: analyst
    repository:
      profile_key: company
    id: \"urn:ordin:agent:analyst\"
    version: \"^1.0\"
    name: Analyst
    role: Compares the quotes
steps:
  - kind: agent
    position: \"1\"
    agent: analyst
    line: Compare the Q3 quotes
    uses: []
    asks_first: false
    ordin_checks: true
limits:
  max_rounds: 10
  wall_clock_minutes: 30
  agents_at_once: 2
  failure_repeat_limit: 3
  external_action_agents: []
origin: \"\"
"
    .to_owned()
}

/// The person presses Start on the crew plan the conversation shows.
async fn press_start(w: &World, charter: CrewCharterId) -> ordin_types::CrewRunId {
    w.host.crews_start(charter).await.expect("the crew starts")
}

#[tokio::test]
async fn a_saved_workflow_calls_ordin_to_check_answer_a_member_and_end() {
    let w = Arc::new(World::new().await);
    let analyst: DirectoryAgent = agent("Analyst");
    w.fakes
        .inspector
        .agents
        .lock()
        .unwrap()
        .push(analyst.clone());
    w.fakes
        .catalogue
        .install(&analyst, "urn:ordin:agent:analyst", "1.0.0");
    let ordin = w.ordin();
    w.h.local.behave(move |start, _| match start.target {
        RunTarget::AgentInstance { agent_instance_id } if agent_instance_id == ordin => {
            ordin_session()
        }
        _ => orch::asks("Which quarter's quotes?"),
    });
    let log = Arc::new(Mutex::new(Log::default()));
    let brain: Brain = Arc::new(|turn: &str| {
        if turn.contains(" asks, quoted as data") {
            (
                vec![(
                    "decide",
                    serde_json::json!({"kind": "question", "answer": "Use the Q3 quotes."}),
                )],
                "Answered the analyst.".to_owned(),
            )
        } else if turn.starts_with("Check ") {
            (
                vec![(
                    "decide",
                    serde_json::json!({"kind": "check", "verdict": "carry_on", "reason": "It compares all three."}),
                )],
                "The result fits.".to_owned(),
            )
        } else if turn.starts_with("The crew ended") {
            (
                vec![],
                "Kestrel is cheapest; the analyst compared all three.".to_owned(),
            )
        } else {
            (vec![], "Noted.".to_owned())
        }
    });
    let stop = CancellationToken::new();
    drive(w.clone(), brain, log.clone(), stop.clone());

    let ws = w.h.workspace;
    w.host
        .open_conversation(ws)
        .await
        .expect("Ordin's conversation opens");
    let (workflow_id, report) = w
        .host
        .workflows_import(ws, "Quote check.workflow.yaml".to_owned(), workflow_yaml())
        .await
        .expect("the workflow is imported");
    assert!(report.problems.is_empty(), "{:?}", report.problems);
    let draft = w
        .host
        .workflows_get(workflow_id, None)
        .await
        .expect("the draft");
    w.host
        .workflows_save(workflow_id, None, draft.file, "Imported".to_owned())
        .await
        .expect("the workflow is saved");
    let charter = w
        .host
        .workflows_run(workflow_id, None, Some("Compare the Q3 quotes".to_owned()))
        .await
        .expect("the crew plan is shown");
    let crew_run_id = press_start(&w, charter).await;
    let run = tokio::time::timeout(Duration::from_secs(20), w.h.wait_ended(crew_run_id))
        .await
        .expect("the crew ends");
    assert_eq!(run.state, CrewRunState::Finished, "{run:?}");

    // The Coordinator was Ordin at the member question, at "Ordin checks the result" and at the end.
    assert!(
        until(10, || log
            .lock()
            .unwrap()
            .turns
            .iter()
            .any(|t| t.starts_with("The crew ended")))
        .await
    );
    let turns = log.lock().unwrap().turns.clone();
    let asked = turns
        .iter()
        .position(|t| t.contains("asks, quoted as data"))
        .expect("a member question");
    let checked = turns
        .iter()
        .position(|t| t.starts_with("Check 1"))
        .expect("a result check");
    let ended = turns
        .iter()
        .position(|t| t.starts_with("The crew ended"))
        .expect("the end");
    assert!(asked < checked && checked < ended, "{turns:?}");
    assert!(turns[asked].contains("Which quarter's quotes?"));
    assert!(turns[ended].contains("(finished)"), "{}", turns[ended]);
    // Ordin's answer reached the member as its answer.
    let member_runs: Vec<RunId> = w
        .h
        .local
        .starts
        .lock()
        .unwrap()
        .iter()
        .filter(|s| !matches!(s.target, RunTarget::AgentInstance { agent_instance_id } if agent_instance_id == ordin))
        .map(|s| s.run_id)
        .collect();
    assert_eq!(member_runs.len(), 1);
    let answered = w.h.local.resumes.lock().unwrap().iter().any(|(r, d)| {
        *r == member_runs[0]
            && matches!(d, ResumeDecision::Answer { text } if text.contains("Use the Q3 quotes."))
    });
    assert!(answered, "the member got Ordin's answer");
    // Every tool call Ordin made passed the Rules Engine and succeeded.
    {
        let l = log.lock().unwrap();
        assert_eq!(l.calls.len(), 2, "{:?}", l.calls);
        assert!(l.calls.iter().all(|(_, r)| r.is_ok()), "{:?}", l.calls);
        assert!(
            l.verdicts.iter().all(|v| v.ends_with("allow")),
            "{:?}",
            l.verdicts
        );
    }
    // Step 16.2: `crew_ended` archives the charter after Ordin's end-of-crew turn has its answer,
    // so the archive follows that answer; wait for the state itself, not for the turn.
    let mut state = String::new();
    for _ in 0..500 {
        state = sqlx::query_scalar("SELECT state FROM crew_charters WHERE id = ?")
            .bind(charter.to_string())
            .fetch_one(w.h.pool())
            .await
            .unwrap();
        if state == "archived" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(state, "archived");
    // Ordin's answer to the end-of-crew turn is its message in the conversation.
    let said: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ordin_messages WHERE author = 'ordin' AND text = ?",
    )
    .bind("Kestrel is cheapest; the analyst compared all three.")
    .fetch_one(w.h.pool())
    .await
    .unwrap();
    assert_eq!(said, 1);
    stop.cancel();
}

#[tokio::test]
async fn behavioural_a_message_starts_a_session_and_a_two_member_crew_with_one_approval() {
    let w = Arc::new(World::new().await);
    let (researcher, writer, mailer) = (agent("Researcher"), agent("Writer"), agent("Mailer"));
    for a in [&researcher, &writer, &mailer] {
        w.fakes.inspector.agents.lock().unwrap().push((*a).clone());
    }
    let ordin = w.ordin();
    let mailer_id = mailer.agent_instance_id;
    w.h.local.behave(move |start, _| match start.target {
        RunTarget::AgentInstance { agent_instance_id } if agent_instance_id == ordin => {
            ordin_session()
        }
        // The delegated session waits: its email waits on the person's approval, then it ends.
        RunTarget::AgentInstance { agent_instance_id } if agent_instance_id == mailer_id => {
            vec![orch::started(), Act::Hang]
        }
        _ => orch::done("Found three quotes."),
    });
    let log = Arc::new(Mutex::new(Log::default()));
    let (r, wr) = (
        researcher.agent_instance_id.to_string(),
        writer.agent_instance_id.to_string(),
    );
    let m = mailer.agent_instance_id.to_string();
    let rounds = Arc::new(Mutex::new(0u32));
    let brain: Brain = {
        let rounds = rounds.clone();
        Arc::new(move |turn: &str| {
            if turn.ends_with("Find and summarise the Q3 quotes.") {
                (
                    vec![
                        (
                            "delegate",
                            serde_json::json!({"agent": m, "brief": "Email the buyer that the Q3 summary is coming."}),
                        ),
                        (
                            "create_crew",
                            serde_json::json!({
                                "purpose": "Find the Q3 quotes and write the summary for the customer.",
                                "members": [
                                    {"agent": r, "role": "Finds the quotes"},
                                    {"agent": wr, "role": "Writes the summary"}
                                ]
                            }),
                        ),
                    ],
                    "The researcher is listing the quotes; a crew plan is ready.".to_owned(),
                )
            } else if turn.lines().any(|l| l.starts_with("Crew round")) {
                // A round's text may follow prefix lines (OH-RC-2: the mailer's answer goes with
                // Ordin's next turn), so the round is found by its own line.
                let mut n = rounds.lock().unwrap();
                *n += 1;
                let call = match *n {
                    1 => (
                        "delegate",
                        serde_json::json!({"agent": r, "brief": "Find the Q3 quotes."}),
                    ),
                    2 => (
                        "delegate",
                        serde_json::json!({"agent": wr, "brief": "Write the summary."}),
                    ),
                    _ => (
                        "finish",
                        serde_json::json!({"summary": "Quotes found and the summary sent."}),
                    ),
                };
                (vec![call], "Next move made.".to_owned())
            } else {
                (vec![], "Noted.".to_owned())
            }
        })
    };
    let stop = CancellationToken::new();
    drive(w.clone(), brain, log.clone(), stop.clone());

    let ws = w.h.workspace;
    w.host
        .send_message(ws, "Find and summarise the Q3 quotes.".to_owned(), vec![])
        .await
        .expect("the message is accepted");
    // Ordin's delegate started one session, and create_crew showed a two-member crew plan.
    assert!(
        until(10, || log.lock().unwrap().calls.len() == 2).await,
        "{:?}",
        log.lock().unwrap().calls
    );
    let charter = {
        let waiting = w.host.waiting_blocks();
        let mut found = None;
        for b in waiting {
            let block = w.host.block(ws, b.block_id).await.expect("the block");
            if let Block::CrewPlan(p) = block.block {
                found = Some(p.crew_charter_id);
            }
        }
        found.expect("a crew plan waits for Start")
    };
    let crew_run_id = press_start(&w, charter).await;

    // The delegated session's email passes the Rules Engine as an external action: it waits on an
    // approval, then the session ends.
    let mailer_run = {
        let mut found = None;
        for _ in 0..500 {
            found =
                w.h.local
                    .starts
                    .lock()
                    .unwrap()
                    .iter()
                    .find_map(|s| match s.target {
                        RunTarget::AgentInstance { agent_instance_id }
                            if agent_instance_id == mailer_id =>
                        {
                            Some(s.run_id)
                        }
                        _ => None,
                    });
            if found.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        found.expect("the delegated session starts")
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    let email = w
        .h
        .rules
        .decide_tool_call(
            &ToolCall {
                run_id: mailer_run,
                agent_instance_id: mailer_id,
                workspace_id: ws,
                point: DecisionPoint::BeforeToolCall,
                tool: "mcp__mail__send_email".into(),
                origin: ToolOrigin::PackageMcpServer { server: "mail".into() },
                arguments: serde_json::json!({"to": "buyer@example.com", "body": "The Q3 summary."}),
            },
            &CallContext {
                subagent: None,
                class: Some(ToolClass::External {
                    kind: "email".into(),
                    approval: ordin_rules::ApprovalNeed::Always,
                    destination: "buyer@example.com".into(),
                    leaves_company: true,
                }),
            },
        )
        .await;
    assert!(
        matches!(email.decision.verdict, Verdict::Ask { .. }),
        "{:?}",
        email.decision.verdict
    );
    let approval_id = email.approval_id.expect("an approval is held open");
    let pressed = w.h.decide_pending(true).await;
    assert_eq!(pressed, approval_id);
    let waited =
        w.h.rules
            .wait_for_approval(approval_id, None)
            .await
            .expect("the approval is decided");
    assert!(matches!(waited, ordin_rules::ApprovalWait::Approved));
    w.h.rules.record_outcome(email.tool_request_id, true).await;
    w.h.local.push(
        mailer_run,
        RunEvent::Ended {
            end: ordin_runtime_adapter::RunEnd {
                reason: RunEndReason::Completed,
                final_message: Some("Buyer emailed.".into()),
                artefacts: vec![],
            },
        },
    );

    let run = tokio::time::timeout(Duration::from_secs(20), w.h.wait_ended(crew_run_id))
        .await
        .expect("the crew ends");
    assert_eq!(run.state, CrewRunState::Finished, "{run:?}");

    // One session outside the crew (the delegated task), two member runs and Ordin's own session.
    let starts = w.h.local.starts.lock().unwrap().clone();
    let ordin_runs = starts
        .iter()
        .filter(|s| matches!(s.target, RunTarget::AgentInstance { agent_instance_id } if agent_instance_id == ordin))
        .count();
    assert_eq!(ordin_runs, 1);
    assert_eq!(
        starts.len(),
        1 + 1 + 2,
        "Ordin, the delegated task and two members"
    );
    // Every one of Ordin's tool calls passed the Rules Engine.
    {
        let l = log.lock().unwrap();
        assert!(l.verdicts.len() >= 5, "{:?}", l.verdicts);
        assert!(
            l.verdicts.iter().all(|v| v.ends_with("allow")),
            "{:?}",
            l.verdicts
        );
        assert!(l.calls.iter().all(|(_, r)| r.is_ok()), "{:?}", l.calls);
    }
    // The journal holds the crew run, its rounds and the approval.
    assert_eq!(w.h.entries(crew_run_id, "crew_ended").await.len(), 1);
    let rounds_changed = w.h.events("crew_round_changed").await;
    assert!(rounds_changed.len() >= 3, "{}", rounds_changed.len());
    let record =
        w.h.approvals
            .get(approval_id)
            .await
            .expect("the approval is journalled");
    assert_eq!(record.card.approval_id, approval_id);
    let id = approval_id.to_string();
    let names = |events: Vec<serde_json::Value>| {
        events
            .iter()
            .filter(|e| e.to_string().contains(&id))
            .count()
    };
    assert_eq!(
        names(w.h.events("approval_requested").await),
        1,
        "the request is journalled"
    );
    assert_eq!(
        names(w.h.events("approval_decided").await),
        1,
        "the decision is journalled"
    );
    stop.cancel();
}

/// Ordin's conversation stays one run across the crew's turns, and its turn text, long or empty,
/// reaches the host (RA-PQ-1, WIRE-PQ-1): a crew Ordin coordinates starts, Ordin answers its member
/// and its check, and the crew finishes, all on the conversation's first run.
// spec: ordin-desktop/components/RuntimeAdapterInterface#RA-PQ-1
// spec: ordin-desktop/components/ClaudeLocalAdapter#CLA-CONV-2
#[tokio::test]
async fn a_crew_ordin_coordinates_runs_on_one_conversation_run_with_long_and_empty_turns() {
    let w = Arc::new(World::new().await);
    let analyst: DirectoryAgent = agent("Analyst");
    w.fakes
        .inspector
        .agents
        .lock()
        .unwrap()
        .push(analyst.clone());
    w.fakes
        .catalogue
        .install(&analyst, "urn:ordin:agent:analyst", "1.0.0");
    let ordin = w.ordin();
    w.h.local.behave(move |start, _| match start.target {
        RunTarget::AgentInstance { agent_instance_id } if agent_instance_id == ordin => {
            ordin_session()
        }
        _ => orch::asks("Which quarter's quotes?"),
    });
    let long = format!("{}.", "The analyst compared all three quotes".repeat(50));
    let log = Arc::new(Mutex::new(Log::default()));
    let brain: Brain = {
        let long = long.clone();
        Arc::new(move |turn: &str| {
            if turn.contains(" asks, quoted as data") {
                (
                    vec![(
                        "decide",
                        serde_json::json!({"kind": "question", "answer": "Use the Q3 quotes."}),
                    )],
                    String::new(),
                )
            } else if turn.starts_with("Check ") {
                (
                    vec![(
                        "decide",
                        serde_json::json!({"kind": "check", "verdict": "carry_on", "reason": "It compares all three."}),
                    )],
                    String::new(),
                )
            } else if turn.starts_with("The crew ended") {
                (vec![], long.clone())
            } else {
                (vec![], "Noted.".to_owned())
            }
        })
    };
    let stop = CancellationToken::new();
    drive(w.clone(), brain, log.clone(), stop.clone());

    let ws = w.h.workspace;
    w.host
        .open_conversation(ws)
        .await
        .expect("Ordin's conversation opens");
    let (workflow_id, _) = w
        .host
        .workflows_import(ws, "Quote check.workflow.yaml".to_owned(), workflow_yaml())
        .await
        .expect("the workflow is imported");
    let draft = w
        .host
        .workflows_get(workflow_id, None)
        .await
        .expect("the draft");
    w.host
        .workflows_save(workflow_id, None, draft.file, "Imported".to_owned())
        .await
        .expect("the workflow is saved");
    let charter = w
        .host
        .workflows_run(workflow_id, None, Some("Compare the Q3 quotes".to_owned()))
        .await
        .expect("the crew plan is shown");
    let crew_run_id = press_start(&w, charter).await;
    let run = tokio::time::timeout(Duration::from_secs(20), w.h.wait_ended(crew_run_id))
        .await
        .expect("the crew ends");
    assert_eq!(run.state, CrewRunState::Finished, "{run:?}");
    // One run carried the whole conversation.
    assert_eq!(w.ordin_runs().len(), 1);
    // Ordin's long end-of-crew text reached the conversation whole.
    let mut said = 0i64;
    for _ in 0..500 {
        said = sqlx::query_scalar(
            "SELECT count(*) FROM ordin_messages WHERE author = 'ordin' AND text = ?",
        )
        .bind(&long)
        .fetch_one(w.h.pool())
        .await
        .unwrap();
        if said == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(said, 1, "a {} character turn is a message", long.len());
    stop.cancel();
}
