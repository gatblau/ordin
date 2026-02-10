//! X2: one test per scenario of the main *Acceptance Criteria (Gherkin)* of OrdinHost.md, named after
//! the scenario. Background: workspace W with agents "Supplier analyst" (A) and "Summary writer" (B);
//! company maxima rounds 30, wall-clock 480 minutes, members at once 8; every dependency a stand-in.
#![allow(unused_imports)]

use std::sync::Arc;
use std::time::Duration;

use ordin_service_api::commands::ordin::BlockAnswer;
use ordin_types::domain::runs::{ReusedAnswer, TaskTarget};
use ordin_types::time::Timestamp;
use ordin_types::{ArtefactId, PlaceId, ProjectId, ReusedAnswerId, RunId};
use ordin_usage_planner::{EstimateBand, UsageEstimate, WorkKind};
use serde_json::{json, Value};

use super::support::*;
use crate::ports::{DirectoryAgent, DirectoryCommand, LinkedRepo};
use crate::types::OrdinHostPolicy;
use crate::OrdinHost;

/// The Background's policy: maxima rounds 30, wall-clock 480 minutes, members at once 8.
fn background_policy() -> OrdinHostPolicy {
    let mut p = policy();
    p.crew_maxima = limits(30, 480, 8);
    p
}

/// The Background: W with "Supplier analyst" (A) and "Summary writer" (B).
async fn background() -> (Fixture, DirectoryAgent, DirectoryAgent) {
    let f = fixture_with(config(), background_policy()).await;
    let a = agent("Supplier analyst");
    let b = agent("Summary writer");
    *f.inspector.agents.lock().unwrap() = vec![a.clone(), b.clone()];
    (f, a, b)
}

fn key(a: &DirectoryAgent) -> String {
    a.agent_instance_id.to_string()
}

/// The data of every event of `kind` appended so far, as JSON text.
fn events(f: &Fixture, kind: &str) -> Vec<String> {
    f.journal
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|e| e.0 == kind)
        .map(|e| e.1.to_string())
        .collect()
}

fn waiting_in(f: &Fixture, kind: &str) -> Vec<crate::types::WaitingBlock> {
    f.host
        .waiting_blocks()
        .into_iter()
        .filter(|w| w.workspace_id == f.ws && w.kind == kind)
        .collect()
}

fn task_count(f: &Fixture) -> usize {
    f.supervisor.tasks.lock().unwrap().len()
}

// Scenario: Happy path — the first message opens Ordin's session with a seed in fixed order
#[tokio::test]
async fn happy_path_the_first_message_opens_ordins_session_with_a_seed_in_fixed_order() {
    let (f, _, _) = background().await;
    f.host
        .send_message(f.ws, "compare the three supplier quotes".to_owned(), vec![])
        .await
        .unwrap();
    let starts = f.supervisor.starts.lock().unwrap().clone();
    assert_eq!(starts.len(), 1, "one conversational run is started");
    assert!(matches!(
        starts[0].target,
        ordin_types::wire::run::RunTarget::AgentInstance { agent_instance_id }
            if agent_instance_id == f.host.config.ordin_agent_instance_id
    ));
    let seed = &starts[0].seed;
    let at = |h: &str| seed.find(h).unwrap_or_else(|| panic!("{h} in the seed"));
    let order = [
        at("## Instructions"),
        at("## Agent directory"),
        at("## Workspace"),
        at("## Voice"),
        at("## Carried items"),
    ];
    assert!(
        order.windows(2).all(|w| w[0] < w[1]),
        "fixed order: {order:?}"
    );
    let sup = f.supervisor.clone();
    assert!(
        eventually(|| sup
            .turn_texts()
            .iter()
            .any(|t| t.ends_with("compare the three supplier quotes")))
        .await,
        "the message is the next turn"
    );
    assert_eq!(
        f.count("SELECT COUNT(*) FROM ordin_conversations WHERE state = 'current'")
            .await,
        1
    );
}

// Scenario: Edge — commands are listed as the session names them on Claude Code
#[tokio::test]
async fn edge_commands_are_listed_as_the_session_names_them_on_claude_code() {
    let (f, mut a, _) = background().await;
    a.plugin_name = Some("supplier-analyst".to_owned());
    a.commands = vec![DirectoryCommand {
        name: "compare-quotes".to_owned(),
        session_name: None,
        argument_hint: None,
        ..Default::default()
    }];
    f.inspector.agents.lock().unwrap()[0] = a;
    f.open().await;
    let seed = f.supervisor.seeds().remove(0);
    assert!(
        seed.contains("Commands: supplier-analyst:compare-quotes"),
        "{seed}"
    );
}

// Scenario: Edge — an agent with no commands says so (OH-DIR-2)
#[tokio::test]
async fn edge_an_agent_with_no_commands_says_so() {
    let (f, mut a, _) = background().await;
    a.commands = Vec::new();
    f.inspector.agents.lock().unwrap()[0] = a;
    f.open().await;
    let seed = f.supervisor.seeds().remove(0);
    assert!(
        seed.contains("  Commands: none. Give it a brief; never pass a command.\n"),
        "{seed}"
    );
}

// Scenario: Happy path — delegate outside a crew starts a task
#[tokio::test]
async fn happy_path_delegate_outside_a_crew_starts_a_task() {
    let (f, mut a, _) = background().await;
    // OH-DLG-1: the command is one the agent's directory entry lists.
    a.commands = vec![DirectoryCommand {
        name: "compare-quotes".to_owned(),
        session_name: None,
        argument_hint: None,
        ..Default::default()
    }];
    f.inspector.agents.lock().unwrap()[0] = a.clone();
    f.open().await;
    let out = f
        .tool(
            "delegate",
            json!({
                "agent": key(&a),
                "brief": "Compare the quotes",
                "command": "compare-quotes",
                "arguments": "Kestrel, Acme and Northwind Q3 quotes"
            }),
        )
        .await
        .unwrap();
    assert_eq!(out.text, "Started: Supplier analyst is working on it.");
    let tasks = f.supervisor.tasks.lock().unwrap().clone();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].0.command.as_deref(), Some("compare-quotes"));
    assert_eq!(
        tasks[0].0.arguments[0].value,
        "Kestrel, Acme and Northwind Q3 quotes"
    );
    assert_eq!(
        tasks[0].0.target,
        TaskTarget::AgentInstance {
            agent_instance_id: a.agent_instance_id
        }
    );
}

// Scenario: An unchanged question is answered from an earlier run
#[tokio::test]
async fn an_unchanged_question_is_answered_from_an_earlier_run() {
    let (f, a, _) = background().await;
    let id = ReusedAnswerId::new();
    let now = Timestamp::now();
    *f.usage.reusable.lock().unwrap() = Some(ReusedAnswer {
        id,
        request: "Which supplier is cheapest?".to_owned(),
        agent: ordin_types::common::AgentRef::AgentInstance {
            agent_instance_id: a.agent_instance_id,
        },
        workspace_id: f.ws,
        input_versions: vec![],
        source_run_id: RunId::new(),
        text: "Kestrel is cheapest.".to_owned(),
        answered_at: now,
        shown_again_at: now,
        expires_at: now,
    });
    f.open().await;
    f.tool(
        "delegate",
        json!({ "agent": key(&a), "brief": "Which supplier is cheapest?" }),
    )
    .await
    .unwrap();
    assert_eq!(task_count(&f), 0, "no task starts");
    let shown = events(&f, "block_shown");
    assert!(shown.iter().any(|s| s.contains("Kestrel is cheapest.")));
    assert!(shown.iter().any(|s| s.contains("Ask again")));
    assert_eq!(f.usage.reused.lock().unwrap().clone(), vec![(id, None)]);
}

// Scenario: A task over many documents asks before it starts
#[tokio::test]
async fn a_task_over_many_documents_asks_before_it_starts() {
    let (f, a, _) = background().await;
    *f.usage.estimate.lock().unwrap() = Some(UsageEstimate {
        band: EstimateBand::Heavy,
        min_turns: 40,
        max_turns: 80,
        typical_day_permille: 300,
        based_on: vec![],
        ask_first: true,
    });
    f.open().await;
    let docs: Vec<ArtefactId> = (0..24).map(|_| ArtefactId::new()).collect();
    f.tool(
        "delegate",
        json!({ "agent": key(&a), "brief": "Read them", "artefacts": docs }),
    )
    .await
    .unwrap();
    assert_eq!(task_count(&f), 0, "no task starts");
    let asked = waiting_in(&f, "question");
    assert_eq!(asked.len(), 1);
    let shown = events(&f, "block_shown");
    assert!(shown
        .iter()
        .any(|s| s.contains("\"Start\"") && s.contains("\"Not now\"")));
    assert!(f.usage.estimates.lock().unwrap()[0]
        .work_key
        .starts_with("task:"));
    f.host
        .answer_block(
            f.ws,
            asked[0].block_id,
            BlockAnswer::Choice {
                choice: "Start".to_owned(),
            },
        )
        .await
        .unwrap();
    assert_eq!(task_count(&f), 1, "the Supervisor starts the task");
}

// Scenario: A coding task starts in its own worktree
#[tokio::test]
async fn a_coding_task_starts_in_its_own_worktree() {
    let (f, a, _) = background().await;
    let place = PlaceId::new();
    let project = ProjectId::new();
    f.workspaces.repos.lock().unwrap().push(LinkedRepo {
        host: ordin_types::domain::ShareHost::Github,
        repository: "acme/rates".to_owned(),
        host_name: "github.com".to_owned(),
        place_id: place,
    });
    f.projects
        .projects
        .lock()
        .unwrap()
        .insert(place, (project, true));
    f.open().await;
    f.tool(
        "delegate",
        json!({ "agent": key(&a), "brief": "Retry failed rate fetches", "code_in": "acme/rates" }),
    )
    .await
    .unwrap();
    {
        let coding = f.projects.coding.lock().unwrap();
        assert_eq!(coding.len(), 1);
        assert_eq!(coding[0].project_id, project);
    }
    let tasks = f.supervisor.tasks.lock().unwrap().clone();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].1, WorkKind::Code);
    assert!(tasks[0].3.is_some(), "the task carries the coding task id");
}

// Scenario: A crew plan carries its estimate
#[tokio::test]
async fn a_crew_plan_carries_its_estimate() {
    let (f, a, b) = background().await;
    let c = agent("Clause checker");
    f.inspector.agents.lock().unwrap().push(c.clone());
    *f.usage.estimate.lock().unwrap() = Some(UsageEstimate {
        band: EstimateBand::Medium,
        min_turns: 12,
        max_turns: 30,
        typical_day_permille: 120,
        based_on: vec![],
        ask_first: false,
    });
    f.open().await;
    f.tool(
        "create_crew",
        json!({
            "purpose": "Compare quotes",
            "members": [
                { "agent": key(&a), "role": "analyst" },
                { "agent": key(&b), "role": "writer" },
                { "agent": key(&c), "role": "checker" }
            ],
            "limits": { "rounds": 10 }
        }),
    )
    .await
    .unwrap();
    let plan: Vec<Value> = f
        .journal
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|e| e.0 == "block_shown")
        .map(|e| e.1.clone())
        .filter(|d| d.to_string().contains("crew_plan"))
        .collect();
    assert_eq!(plan.len(), 1);
    let text = plan[0].to_string();
    assert!(text.contains("\"estimate\""), "{text}");
    assert!(text.contains("\"max_turns\":30"), "{text}");
}

// Scenario: A waiting choice typed in words is answered without the model
#[tokio::test]
async fn a_waiting_choice_typed_in_words_is_answered_without_the_model() {
    let (f, _, _) = background().await;
    *f.usage.without_model.lock().unwrap() = true;
    f.open().await;
    f.tool(
        "show",
        json!({ "block": { "kind": "question", "question": "Which supplier?", "choices": ["Kestrel", "Acme"], "allow_other": false } }),
    )
    .await
    .unwrap();
    assert_eq!(waiting_in(&f, "question").len(), 1);
    let turns_before = f.supervisor.turn_texts().len();
    f.host
        .send_message(f.ws, "kestrel".to_owned(), vec![])
        .await
        .unwrap();
    assert!(
        waiting_in(&f, "question").is_empty(),
        "the question is answered"
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        f.supervisor.turn_texts().len(),
        turns_before,
        "no Ordin turn is sent"
    );
    assert_eq!(f.f.count("usage.record_answered_without_model"), 1);
}

// ---------------------------------------------------------------- helpers of the later scenarios

fn tool_ctx(ws: ordin_types::WorkspaceId, run_id: RunId) -> ordin_mcp_front::ToolCallContext {
    ordin_mcp_front::ToolCallContext {
        run_id,
        workspace_id: ws,
        agent_instance_id: ordin_types::AgentInstanceId::new(),
        caller: ordin_mcp_front::ToolCaller::Ordin,
        scope_id: None,
        coding_task_id: None,
        call_id: ordin_types::ToolRequestId::new(),
        correlation_id: "test".to_owned(),
        deadline: std::time::Instant::now() + Duration::from_secs(30),
        cancel: tokio_util::sync::CancellationToken::new(),
        progress: ordin_mcp_front::ProgressSink::detached(),
        only_needed_parts: false,
        via: ordin_mcp_front::CallVia::Harness,
    }
}

fn view(f: &Fixture, members: &[&DirectoryAgent]) -> ordin_crew_orchestrator::CrewView {
    ordin_crew_orchestrator::CrewView {
        crew_run_id: ordin_types::CrewRunId::new(),
        workspace_id: f.ws,
        purpose: "Refresh the supplier summary".to_owned(),
        members: members
            .iter()
            .map(|a| ordin_types::domain::runs::CrewMember {
                member: ordin_types::common::AgentRef::AgentInstance {
                    agent_instance_id: a.agent_instance_id,
                },
                role: a.display_name.clone(),
            })
            .collect(),
        round: 1,
        rounds_left: 9,
        seconds_left: 3_600,
        requests_left: None,
        inputs: vec![],
        latest_results: vec![],
        position: None,
        cancel: tokio_util::sync::CancellationToken::new(),
    }
}

/// A pack question of class "testing" with labels "pytest" and "unittest".
fn pack_question(a: &DirectoryAgent) -> ordin_package_catalogue::PendingChoice {
    ordin_package_catalogue::PendingChoice {
        agent_instance_id: a.agent_instance_id,
        rule_index: 0,
        at_least: 1,
        class_label: "testing".to_owned(),
        choices: vec![
            ordin_package_catalogue::PendingChoiceOption {
                pack: "pytest".to_owned(),
                label: "pytest".to_owned(),
            },
            ordin_package_catalogue::PendingChoiceOption {
                pack: "unittest".to_owned(),
                label: "unittest".to_owned(),
            },
        ],
    }
}

// Scenario: Error — delegate names an agent not in the directory
#[tokio::test]
async fn error_delegate_names_an_agent_not_in_the_directory() {
    let (f, _, _) = background().await;
    f.open().await;
    let e = f
        .tool(
            "delegate",
            json!({ "agent": "0192ffff-0000-7000-8000-000000000000", "brief": "Do it" }),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code, "OH_AGENT_UNKNOWN");
    assert_eq!(task_count(&f), 0);
}

// Scenario: Error — delegate to an agent whose folder is not ready
#[tokio::test]
async fn error_delegate_to_an_agent_whose_folder_is_not_ready() {
    let (f, a, _) = background().await;
    f.open().await;
    f.f.always("catalogue.ensure_ready", "PKG_FOLDER_UNUSABLE", false);
    let e = f
        .tool("delegate", json!({ "agent": key(&a), "brief": "Do it" }))
        .await
        .unwrap_err();
    assert_eq!(e.code, "PKG_FOLDER_UNUSABLE");
    assert_eq!(task_count(&f), 0);
}

// Scenario: Happy path — an agent's pack question is asked before its task
#[tokio::test]
async fn happy_path_an_agents_pack_question_is_asked_before_its_task() {
    let (f, a, _) = background().await;
    *f.catalogue.pending.lock().unwrap() = vec![pack_question(&a)];
    f.open().await;
    f.tool(
        "delegate",
        json!({ "agent": key(&a), "brief": "Run the tests" }),
    )
    .await
    .unwrap();
    assert_eq!(task_count(&f), 0, "no task starts");
    let asked = waiting_in(&f, "question");
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0].title,
        "Supplier analyst needs a testing to work with. Which one?"
    );
    let shown = events(&f, "block_shown");
    assert!(shown
        .iter()
        .any(|s| s.contains("needs a testing") && s.contains("[\"pytest\",\"unittest\"]")));
    // Once the answer is stored the catalogue has no open question left.
    f.catalogue.pending.lock().unwrap().clear();
    f.host
        .answer_block(
            f.ws,
            asked[0].block_id,
            BlockAnswer::Choice {
                choice: "pytest".to_owned(),
            },
        )
        .await
        .unwrap();
    {
        let answers = f.catalogue.answers.lock().unwrap();
        assert_eq!(answers.len(), 1);
        assert_eq!(answers[0].agent_instance_id, a.agent_instance_id);
        assert_eq!(answers[0].choice.rule_index, 0);
        assert_eq!(answers[0].choice.packs, vec!["pytest".to_owned()]);
    }
    assert_eq!(
        task_count(&f),
        1,
        "the task is submitted to the Session Supervisor"
    );
}

fn pack_question_of(a: &DirectoryAgent, labels: &[&str]) -> ordin_package_catalogue::PendingChoice {
    ordin_package_catalogue::PendingChoice {
        class_label: "stack".to_owned(),
        choices: labels
            .iter()
            .map(|l| ordin_package_catalogue::PendingChoiceOption {
                pack: format!("urn:pack:{l}"),
                label: (*l).to_owned(),
            })
            .collect(),
        ..pack_question(a)
    }
}

// OH-PACK-1: one candidate is confirmed with "Use <label>", and the task then starts.
// spec: ordin-desktop/components/OrdinHost#OH-PACK-1
#[tokio::test]
async fn happy_path_a_pack_question_with_one_candidate_is_confirmed() {
    let (f, a, _) = background().await;
    *f.catalogue.pending.lock().unwrap() = vec![pack_question_of(&a, &["Go"])];
    f.open().await;
    f.tool("delegate", json!({ "agent": key(&a), "brief": "Build it" }))
        .await
        .unwrap();
    let asked = waiting_in(&f, "question");
    assert_eq!(asked.len(), 1);
    assert_eq!(
        asked[0].title,
        "Supplier analyst needs a stack to work with. It can use Go."
    );
    assert!(events(&f, "block_shown")
        .iter()
        .any(|s| s.contains("[\"Use Go\",\"Not now\"]")));
    f.catalogue.pending.lock().unwrap().clear();
    f.host
        .answer_block(
            f.ws,
            asked[0].block_id,
            BlockAnswer::Choice {
                choice: "Use Go".to_owned(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        f.catalogue.answers.lock().unwrap()[0].choice.packs,
        vec!["urn:pack:Go".to_owned()]
    );
    assert_eq!(task_count(&f), 1);
}

// OH-PACK-1: "Not now" records nothing and starts nothing.
#[tokio::test]
async fn edge_case_not_now_to_a_pack_question_starts_nothing() {
    let (f, a, _) = background().await;
    *f.catalogue.pending.lock().unwrap() = vec![pack_question_of(&a, &["Go"])];
    f.open().await;
    f.tool("delegate", json!({ "agent": key(&a), "brief": "Build it" }))
        .await
        .unwrap();
    let asked = waiting_in(&f, "question");
    f.host
        .answer_block(
            f.ws,
            asked[0].block_id,
            BlockAnswer::Choice {
                choice: "Not now".to_owned(),
            },
        )
        .await
        .unwrap();
    assert!(f.catalogue.answers.lock().unwrap().is_empty());
    assert_eq!(task_count(&f), 0);
}

// OH-PACK-1: five to twenty candidates are a form with one list; its answer is stored.
#[tokio::test]
async fn happy_path_a_pack_question_with_many_candidates_is_a_form_with_a_list() {
    let (f, a, _) = background().await;
    let names = ["Go", "Java", "Python", "Rust", "TypeScript"];
    *f.catalogue.pending.lock().unwrap() = vec![pack_question_of(&a, &names)];
    f.open().await;
    f.tool("delegate", json!({ "agent": key(&a), "brief": "Build it" }))
        .await
        .unwrap();
    assert_eq!(task_count(&f), 0);
    let asked = waiting_in(&f, "form");
    assert_eq!(asked.len(), 1);
    assert!(events(&f, "block_shown")
        .iter()
        .any(|s| s.contains("\"options\":[\"Go\",\"Java\",\"Python\",\"Rust\",\"TypeScript\"]")));
    f.catalogue.pending.lock().unwrap().clear();
    let mut fields = std::collections::BTreeMap::new();
    fields.insert("pack".to_owned(), json!("Rust"));
    f.host
        .answer_block(f.ws, asked[0].block_id, BlockAnswer::Form { fields })
        .await
        .unwrap();
    assert_eq!(
        f.catalogue.answers.lock().unwrap()[0].choice.packs,
        vec!["urn:pack:Rust".to_owned()]
    );
    assert_eq!(task_count(&f), 1);
}

// Scenario: Error — the pack answer cannot be stored
#[tokio::test]
async fn error_the_pack_answer_cannot_be_stored() {
    let (f, a, _) = background().await;
    *f.catalogue.pending.lock().unwrap() = vec![pack_question(&a)];
    f.open().await;
    f.tool(
        "delegate",
        json!({ "agent": key(&a), "brief": "Run the tests" }),
    )
    .await
    .unwrap();
    let asked = waiting_in(&f, "question");
    f.f.always("catalogue.record_answer", "PKG_CHOICE_NOT_ALLOWED", false);
    f.host
        .answer_block(
            f.ws,
            asked[0].block_id,
            BlockAnswer::Choice {
                choice: "pytest".to_owned(),
            },
        )
        .await
        .unwrap();
    assert_eq!(task_count(&f), 0, "no task starts");
    let sup = f.supervisor.clone();
    assert!(
        eventually(|| sup
            .turn_texts()
            .iter()
            .any(|t| t.ends_with("The choice could not be saved (PKG_CHOICE_NOT_ALLOWED).")))
        .await,
        "{:?}",
        f.supervisor.turn_texts()
    );
}

/// A format 3 file of one workspace agent step per `(position, key)`, the agents installed.
fn file(
    name: &str,
    agents: &[(&str, &DirectoryAgent)],
    steps: Vec<ordin_types::wire::workflow_file::WorkflowFileStep>,
) -> ordin_types::wire::WorkflowFile {
    use ordin_types::wire::workflow_file::*;
    ordin_types::wire::WorkflowFile {
        format: 3,
        name: name.to_owned(),
        purpose: format!("{name} for the team"),
        inputs: vec![],
        agents: agents
            .iter()
            .map(|(k, a)| WorkflowFileAgent {
                key: (*k).to_owned(),
                repository: AgentRepositoryRef::ProfileKey("company".to_owned()),
                id: format!("urn:ordin:agent:{k}"),
                version: "^1.0".to_owned(),
                name: a.display_name.clone(),
                role: format!("{k} role"),
            })
            .collect(),
        steps,
        limits: WorkflowFileLimits {
            max_rounds: 10,
            wall_clock_minutes: 30,
            request_budget: None,
            agents_at_once: 3,
            failure_repeat_limit: 2,
            external_action_agents: vec![],
        },
        origin: "Drafted by Ordin".to_owned(),
    }
}

fn step(position: &str, agent: &str) -> ordin_types::wire::workflow_file::WorkflowFileStep {
    ordin_types::wire::workflow_file::WorkflowFileStep::Agent {
        position: position.to_owned(),
        agent: agent.to_owned(),
        line: "Do the work".to_owned(),
        uses: vec![],
        asks_first: false,
        ordin_checks: false,
        if_it_fails: Default::default(),
    }
}

/// Makes each `(key, agent)` present in the workspace under its file id.
fn install(f: &Fixture, agents: &[(&str, &DirectoryAgent)]) {
    for (k, a) in agents {
        f.catalogue
            .install(a, &format!("urn:ordin:agent:{k}"), "1.2.0");
    }
}

// Scenario: A step whose agent can send email must ask first
#[tokio::test]
async fn a_step_whose_agent_can_send_email_must_ask_first() {
    let (f, a, _) = background().await;
    install(&f, &[("mailer", &a)]);
    f.catalogue
        .send_email
        .lock()
        .unwrap()
        .push(a.agent_instance_id);
    let wf = file("Send rates", &[("mailer", &a)], vec![step("1", "mailer")]);
    let report = f.host.workflows_check(f.ws, wf).await.unwrap();
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.code == "WFF_SEND_WITHOUT_ASK" && p.at == "1"),
        "{report:?}"
    );
}

// Scenario: Happy path — create_crew clamps limits and shows a plan
#[tokio::test]
async fn happy_path_create_crew_clamps_limits_and_shows_a_plan() {
    let (f, a, b) = background().await;
    f.open().await;
    let out = f
        .tool(
            "create_crew",
            json!({
                "purpose": "Compare quotes",
                "members": [ { "agent": key(&a), "role": "analyst" }, { "agent": key(&b), "role": "writer" } ],
                "limits": { "rounds": 40 }
            }),
        )
        .await
        .unwrap();
    let id: ordin_types::CrewCharterId =
        serde_json::from_value(out.structured.unwrap()["crew_charter_id"].clone()).unwrap();
    let row = {
        let mut c = f.pool.acquire().await.unwrap();
        crate::store::charter(&mut c, id).await.unwrap().unwrap()
    };
    assert_eq!(row.charter.guardrails.max_rounds, 30);
    assert_eq!(
        row.charter.state,
        ordin_types::domain::runs::CrewCharterState::Valid
    );
    assert!(events(&f, "block_shown")
        .iter()
        .any(|s| s.contains("crew_plan") && s.contains(&id.to_string())));
    assert!(
        f.orchestrator.started.lock().unwrap().is_empty(),
        "no crew run starts yet"
    );
    f.host.crews_start(id).await.unwrap();
    assert_eq!(
        f.orchestrator.started.lock().unwrap().len(),
        1,
        "crews.start starts it"
    );
}

// Scenario: Happy path — a written plan is read (OH-WR-2)
#[tokio::test]
async fn happy_path_a_written_plan_is_read() {
    let (f, a, b) = background().await;
    f.open().await;
    let out = f
        .tool(
            "create_crew",
            json!({
                "purpose": "Compare the three quotes",
                "members": [ { "agent": key(&a), "role": "analyst" }, { "agent": key(&b), "role": "writer" } ]
            }),
        )
        .await
        .unwrap();
    let id: ordin_types::CrewCharterId =
        serde_json::from_value(out.structured.unwrap()["crew_charter_id"].clone()).unwrap();
    let before = events(&f, "block_shown").len() + events(&f, "needs_you_changed").len();
    let charter = f.host.crews_charter(id).await.unwrap();
    assert_eq!(charter.id, id);
    assert_eq!(charter.purpose, "Compare the three quotes");
    assert_eq!(charter.members.len(), 2);
    assert_eq!(charter.members[0].role, "analyst");
    assert_eq!(charter.members[1].role, "writer");
    assert_eq!(
        before,
        events(&f, "block_shown").len() + events(&f, "needs_you_changed").len(),
        "reading appends no event"
    );
}

// Scenario: Error path — a charter that does not exist (OH-WR-2)
#[tokio::test]
async fn error_path_a_crew_charter_that_does_not_exist() {
    let (f, _a, _b) = background().await;
    f.open().await;
    let e = f
        .host
        .crews_charter(ordin_types::CrewCharterId::new())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_CHARTER_UNKNOWN");
}

// Scenario: Edge case — a discarded plan is still read (OH-WR-2)
#[tokio::test]
async fn edge_case_a_discarded_plan_is_still_read() {
    let (f, a, _b) = background().await;
    f.open().await;
    let out = f
        .tool(
            "create_crew",
            json!({ "purpose": "Compare quotes", "members": [ { "agent": key(&a), "role": "analyst" } ] }),
        )
        .await
        .unwrap();
    let id: ordin_types::CrewCharterId =
        serde_json::from_value(out.structured.unwrap()["crew_charter_id"].clone()).unwrap();
    f.host.crews_discard_plan(id).await.unwrap();
    let charter = f.host.crews_charter(id).await.unwrap();
    assert_eq!(
        charter.state,
        ordin_types::domain::runs::CrewCharterState::Discarded
    );
    assert!(
        f.host.crews_start(id).await.is_err(),
        "Start is still refused"
    );
}

// Scenario: Edge case — the charter is read as it is now (OH-WR-2)
#[tokio::test]
async fn edge_case_the_charter_is_read_as_it_is_now() {
    let (f, a, _b) = background().await;
    f.open().await;
    let out = f
        .tool(
            "create_crew",
            json!({ "purpose": "Compare quotes", "members": [ { "agent": key(&a), "role": "analyst" } ] }),
        )
        .await
        .unwrap();
    let id: ordin_types::CrewCharterId =
        serde_json::from_value(out.structured.unwrap()["crew_charter_id"].clone()).unwrap();
    assert_eq!(
        f.host.crews_charter(id).await.unwrap().state,
        ordin_types::domain::runs::CrewCharterState::Valid
    );
    f.host.crews_start(id).await.unwrap();
    assert_eq!(
        f.host.crews_charter(id).await.unwrap().state,
        ordin_types::domain::runs::CrewCharterState::Started
    );
}

// Scenario: Edge case — a crew that ends before its start is recorded is archived (OH-CL-1, OH-CL-2)
// spec: ordin-desktop/components/OrdinHost#OH-CL-1
// spec: ordin-desktop/components/OrdinHost#OH-CL-2
#[tokio::test]
async fn edge_case_a_crew_that_ends_before_its_start_is_recorded_is_archived() {
    use ordin_crew_orchestrator::{CrewOutcome, CrewView};
    use ordin_types::domain::runs::{CrewCharterState, CrewRunState};

    let (f, a, _b) = background().await;
    f.open().await;
    let out = f
        .tool(
            "create_crew",
            json!({ "purpose": "Compare quotes", "members": [ { "agent": key(&a), "role": "analyst" } ] }),
        )
        .await
        .unwrap();
    let id: ordin_types::CrewCharterId =
        serde_json::from_value(out.structured.unwrap()["crew_charter_id"].clone()).unwrap();
    // The Crew Orchestrator ends the crew before `start` returns: `crew_ended` runs on its own task,
    // as the crew's driver does, and `start` waits for it to finish unless it waits for the start.
    let ended: Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<bool>>>> = Arc::default();
    let slot = ended.clone();
    *f.orchestrator.on_start.lock().unwrap() = Some(Arc::new(move |crew_run_id, start| {
        let coordinator = start.coordinator.clone();
        let view = CrewView {
            crew_run_id,
            workspace_id: start.charter.workspace_id,
            purpose: start.charter.purpose.clone(),
            members: start.charter.members.clone(),
            round: 1,
            rounds_left: 0,
            seconds_left: 0,
            requests_left: None,
            inputs: vec![],
            latest_results: vec![],
            position: None,
            cancel: tokio_util::sync::CancellationToken::new(),
        };
        let slot = slot.clone();
        Box::pin(async move {
            let mut task = tokio::spawn(async move {
                let outcome = CrewOutcome {
                    state: CrewRunState::Finished,
                    end_reason: None,
                    results: vec![],
                    changed_by_ordin: vec![],
                    recommendation: None,
                };
                coordinator.crew_ended(&view, &outcome).await.is_ok()
            });
            let finished = tokio::time::timeout(Duration::from_secs(2), &mut task)
                .await
                .is_ok();
            assert!(!finished, "crew_ended waits for the start to be recorded");
            *slot.lock().unwrap() = Some(task);
        })
    }));
    f.host.crews_start(id).await.unwrap();
    let task = ended.lock().unwrap().take().expect("the crew ended");
    let ok = tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .expect("crew_ended returns once the start is recorded")
        .unwrap();
    assert!(ok);
    let charter = f.host.crews_charter(id).await.unwrap();
    assert_eq!(charter.state, CrewCharterState::Archived);
}

// Scenario: Happy path — a coordinator round resolves through Ordin's delegate call
#[tokio::test]
async fn happy_path_a_coordinator_round_resolves_through_ordins_delegate_call() {
    let (f, a, b) = background().await;
    let run = f.open().await;
    let host = f.host.clone();
    let ws = f.ws;
    let b_key = key(&b);
    f.supervisor.respond(Arc::new(move |run_id, text| {
        let host = host.clone();
        let b_key = b_key.clone();
        Box::pin(async move {
            if text.contains("Crew round") {
                let ctx = tool_ctx(ws, run_id);
                host.tool(
                    &ctx,
                    "delegate",
                    json!({ "agent": b_key, "brief": "Update the summary" }),
                )
                .await
                .expect("delegate in the round");
            }
            Some("Done.".to_owned())
        })
    }));
    assert_eq!(f.host.current_run(f.ws), Some(run));
    let turn = f
        .host
        .coordinator()
        .round(&view(&f, &[&a, &b]))
        .await
        .unwrap();
    assert_eq!(turn.turns, 1);
    assert_eq!(turn.calls.len(), 1);
    match &turn.calls[0] {
        ordin_crew_orchestrator::CrewToolCall::Delegate { member, brief, .. } => {
            assert_eq!(
                *member,
                ordin_types::common::AgentRef::AgentInstance {
                    agent_instance_id: b.agent_instance_id
                }
            );
            assert_eq!(brief, "Update the summary");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(task_count(&f), 0, "a crew delegation is not a task");
}

// Scenario: Error — Ordin makes no move
#[tokio::test]
async fn error_ordin_makes_no_move() {
    let (f, a, b) = background().await;
    f.open().await;
    f.supervisor.respond(reply_with("Thinking about it."));
    let turn = f
        .host
        .coordinator()
        .round(&view(&f, &[&a, &b]))
        .await
        .unwrap();
    assert!(
        turn.calls.is_empty(),
        "no move: the orchestrator treats it as escalate"
    );
    assert_eq!(turn.turns, 2, "one reminder turn");
    let texts = f.supervisor.turn_texts();
    assert!(texts.iter().any(|t| t.contains("Crew round 1 of 10")));
    assert!(texts
        .iter()
        .any(|t| t.ends_with("Reply with delegate, finish or escalate.")
            && !t.contains("Crew round")));
}

/// Imports `wf` and saves it as version 1; its agents are installed first.
async fn saved(f: &Fixture, wf: &ordin_types::wire::WorkflowFile) -> ordin_types::WorkflowId {
    let yaml = crate::wff::to_yaml(wf);
    let (id, report) = f
        .host
        .workflows_import(f.ws, format!("{}.workflow.yaml", wf.name), yaml)
        .await
        .expect("the file imports");
    assert!(report.problems.is_empty(), "{report:?}");
    let v = f
        .host
        .workflows_save(id, None, wf.clone(), "First version".to_owned())
        .await
        .expect("the first save");
    assert_eq!(v, 1);
    id
}

fn wff_codes(
    r: &ordin_service_api::commands::workflows::WorkflowCheckReport,
) -> Vec<(String, String)> {
    r.problems
        .iter()
        .map(|p| (p.code.clone(), p.at.clone()))
        .collect()
}

// Scenario: Happy path — import a format 3 file with a limit lowered
#[tokio::test]
async fn happy_path_import_a_format_3_file_with_a_limit_lowered() {
    let (f, a, b) = background().await;
    let agents = [("analyst", &a), ("writer", &b)];
    install(&f, &agents);
    let mut wf = file(
        "Supplier rates refresh",
        &agents,
        vec![step("1", "analyst"), step("2", "writer")],
    );
    wf.limits.max_rounds = 40;
    let mut yaml = crate::wff::to_yaml(&wf);
    // About 12 KB, padded with comment lines.
    while yaml.len() < 12 * 1024 {
        yaml.push_str("# padding line of the file\n");
    }
    let (id, report) = f
        .host
        .workflows_import(f.ws, "rates.workflow.yaml".to_owned(), yaml)
        .await
        .unwrap();
    let view = f.host.workflows_get(id, None).await.unwrap();
    assert_eq!(
        view.workflow.state,
        ordin_types::domain::WorkflowState::NotSavedYet
    );
    assert!(matches!(
        view.workflow.origin,
        ordin_types::domain::WorkflowOrigin::Imported { .. }
    ));
    let lowered = report
        .notices
        .iter()
        .find(|n| n.code == "WFF_LIMIT_LOWERED")
        .expect("the limit-lowered notice");
    assert_eq!(lowered.args.get("asked").map(String::as_str), Some("40"));
    assert_eq!(lowered.args.get("maximum").map(String::as_str), Some("30"));
    assert!(
        f.supervisor.turns.lock().unwrap().is_empty(),
        "no model turn"
    );
    assert!(
        f.supervisor.starts.lock().unwrap().is_empty(),
        "no model turn"
    );
}

// Scenario: Error — import a newer format
#[tokio::test]
async fn error_import_a_newer_format() {
    let (f, _, _) = background().await;
    let e = f
        .host
        .workflows_import(
            f.ws,
            "new.workflow.yaml".to_owned(),
            "format: 4\nname: \"New\"\n".to_owned(),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "WFF_FORMAT_NEWER");
    assert_eq!(f.count("SELECT COUNT(*) FROM workflows").await, 0);
}

// Scenario: Error — import a file of 300 KB
#[tokio::test]
async fn error_import_a_file_of_300_kb() {
    let (f, _, _) = background().await;
    let yaml = format!("format: 3\n{}", "#".repeat(300 * 1024));
    let e = f
        .host
        .workflows_import(f.ws, "big.workflow.yaml".to_owned(), yaml)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "WFF_FILE_TOO_LARGE");
}

// Scenario: Error — a run_workflow cycle is refused at save
#[tokio::test]
async fn error_a_run_workflow_cycle_is_refused_at_save() {
    use ordin_types::wire::workflow_file::WorkflowFileStep;
    let (f, a, _) = background().await;
    install(&f, &[("checker", &a)]);
    let clause = file(
        "Clause check",
        &[("checker", &a)],
        vec![step("1", "checker")],
    );
    let clause_id = saved(&f, &clause).await;
    let contract = file(
        "Contract review",
        &[("checker", &a)],
        vec![
            step("1", "checker"),
            WorkflowFileStep::Call {
                position: "2".to_owned(),
                workflow: "Clause check".to_owned(),
            },
        ],
    );
    saved(&f, &contract).await;
    let mut changed = clause.clone();
    changed.steps.push(WorkflowFileStep::Call {
        position: "2".to_owned(),
        workflow: "Contract review".to_owned(),
    });
    let e = f
        .host
        .workflows_save(clause_id, Some(1), changed, "Run the review".to_owned())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_CHECK_FAILED");
    let crate::OhError::CheckFailed { report } = e else {
        unreachable!()
    };
    let cycle = report
        .problems
        .iter()
        .find(|p| p.code == "WFF_CALL_CYCLE")
        .expect("the cycle problem");
    assert_eq!(cycle.at, "2", "at the run_workflow step");
    // "This would run Contract review again inside itself": Localisation words it from the name.
    assert_eq!(
        cycle.args.get("name").map(String::as_str),
        Some("Contract review")
    );
}

// Scenario: Edge — a step uses a result made on another path of an if
#[tokio::test]
async fn edge_a_step_uses_a_result_made_on_another_path_of_an_if() {
    use ordin_types::wire::workflow_file::{Decider, NamedPath, StepUse, WorkflowFileStep};
    let (f, a, _) = background().await;
    install(&f, &[("analyst", &a)]);
    let mut uses_2_1 = step("3", "analyst");
    if let WorkflowFileStep::Agent { uses, .. } = &mut uses_2_1 {
        uses.push(StepUse::Step("2.1".to_owned()));
    }
    let wf = file(
        "Quote route",
        &[("analyst", &a)],
        vec![
            step("1", "analyst"),
            WorkflowFileStep::If {
                position: "2".to_owned(),
                decided_by: Decider::Ordin {
                    words: "Is a quote over budget?".to_owned(),
                },
                paths: vec![
                    NamedPath {
                        name: "Yes".to_owned(),
                        steps: vec![step("2.1", "analyst")],
                    },
                    NamedPath {
                        name: "No".to_owned(),
                        steps: vec![step("2.2", "analyst")],
                    },
                ],
            },
            uses_2_1,
        ],
    );
    let report = f.host.workflows_check(f.ws, wf).await.unwrap();
    assert!(
        wff_codes(&report).contains(&("WFF_USE_NOT_ON_PATH".to_owned(), "3".to_owned())),
        "{report:?}"
    );
}

// Scenario: Happy path — save, restore and versions
#[tokio::test]
async fn happy_path_save_restore_and_versions() {
    let (f, a, _) = background().await;
    install(&f, &[("analyst", &a)]);
    let v1 = file(
        "Supplier rates refresh",
        &[("analyst", &a)],
        vec![step("1", "analyst")],
    );
    let id = saved(&f, &v1).await;
    let mut v2 = v1.clone();
    v2.purpose = "A changed purpose".to_owned();
    assert_eq!(
        f.host
            .workflows_save(id, Some(1), v2, "Change".to_owned())
            .await
            .unwrap(),
        2
    );
    assert_eq!(f.host.workflows_restore(id, 1).await.unwrap(), 3);
    let view = f.host.workflows_get(id, Some(3)).await.unwrap();
    let line = view.versions.iter().find(|v| v.version == 3).unwrap();
    assert_eq!(line.restored_from_version, Some(1));
    let one = f.host.workflows_get(id, Some(1)).await.unwrap();
    assert_eq!(
        crate::wff::to_yaml(&view.file),
        crate::wff::to_yaml(&one.file)
    );
}

// Scenario: Edge — save on a stale base
#[tokio::test]
async fn edge_save_on_a_stale_base() {
    let (f, a, _) = background().await;
    install(&f, &[("analyst", &a)]);
    let wf = file(
        "Supplier rates refresh",
        &[("analyst", &a)],
        vec![step("1", "analyst")],
    );
    let id = saved(&f, &wf).await;
    f.host
        .workflows_save(id, Some(1), wf.clone(), "Two".to_owned())
        .await
        .unwrap();
    f.host
        .workflows_save(id, Some(2), wf.clone(), "Three".to_owned())
        .await
        .unwrap();
    let e = f
        .host
        .workflows_save(id, Some(2), wf, "Stale".to_owned())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_STALE_VERSION");
}

// Scenario: Error — import of a file with a YAML alias
#[tokio::test]
async fn error_import_of_a_file_with_a_yaml_alias() {
    let (f, _, _) = background().await;
    let yaml = "format: 3\nname: &n \"Aliased\"\npurpose: *n\n";
    let refused =
        ordin_types::yaml::read_restricted::<Value>(yaml, crate::wff::FILE_MAX_BYTES).unwrap_err();
    assert_eq!(
        ordin_types::CodedError::code(&refused),
        "YAML_FEATURE_REFUSED"
    );
    let e = f
        .host
        .workflows_import(f.ws, "alias.workflow.yaml".to_owned(), yaml.to_owned())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "WFF_FILE_MALFORMED");
}

fn supplier_portal(f: &Fixture) {
    f.workspaces.repos.lock().unwrap().push(LinkedRepo {
        host: ordin_types::domain::ShareHost::Github,
        repository: "acme/supplier-portal".to_owned(),
        host_name: "github.com".to_owned(),
        place_id: PlaceId::new(),
    });
}

fn portal_folder() -> crate::types::RepositoryFolder {
    crate::types::RepositoryFolder {
        host: ordin_types::domain::ShareHost::Github,
        repository: "acme/supplier-portal".to_owned(),
        folder: "workflows".to_owned(),
    }
}

/// Saves `wf` up to version 3.
async fn at_version_3(
    f: &Fixture,
    wf: &ordin_types::wire::WorkflowFile,
) -> ordin_types::WorkflowId {
    let id = saved(f, wf).await;
    f.host
        .workflows_save(id, Some(1), wf.clone(), "Two".to_owned())
        .await
        .unwrap();
    f.host
        .workflows_save(id, Some(2), wf.clone(), "Three".to_owned())
        .await
        .unwrap();
    id
}

// Scenario: Happy path — export returns format 3 YAML
#[tokio::test]
async fn happy_path_export_returns_format_3_yaml() {
    let (f, a, _) = background().await;
    install(&f, &[("analyst", &a)]);
    let wf = file(
        "Supplier rates refresh",
        &[("analyst", &a)],
        vec![step("1", "analyst")],
    );
    let id = at_version_3(&f, &wf).await;
    let (file_name, yaml) = f.host.workflows_export(id, 3).await.unwrap();
    assert_eq!(file_name, "Supplier rates refresh.workflow.yaml");
    assert!(yaml.starts_with("format: 3"), "{yaml}");
    // Format 3's struct refuses unknown fields: reading it back proves no other field is there.
    let back: ordin_types::wire::WorkflowFile =
        ordin_types::yaml::read_restricted(&yaml, crate::wff::FILE_MAX_BYTES)
            .expect("format 3 keys only");
    assert_eq!(back.format, 3);
    let top: Value = ordin_types::yaml::read_restricted(&yaml, crate::wff::FILE_MAX_BYTES).unwrap();
    let mut keys: Vec<&str> = top
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["agents", "format", "inputs", "limits", "name", "origin", "purpose", "steps"]
    );
}

// Scenario: Happy path — share goes through the Push Service
#[tokio::test]
async fn happy_path_share_goes_through_the_push_service() {
    let (f, a, _) = background().await;
    install(&f, &[("analyst", &a)]);
    supplier_portal(&f);
    let wf = file(
        "Supplier rates refresh",
        &[("analyst", &a)],
        vec![step("1", "analyst")],
    );
    let id = at_version_3(&f, &wf).await;
    let submission = f
        .host
        .workflows_share(id, 3, portal_folder())
        .await
        .unwrap();
    let (_, yaml) = f.host.workflows_export(id, 3).await.unwrap();
    {
        let pulls = f.push.pulls.lock().unwrap();
        assert_eq!(pulls.len(), 1);
        assert_eq!(pulls[0].repository, "acme/supplier-portal");
        match &pulls[0].content {
            ordin_push::PullRequestContent::Files { files } => {
                assert_eq!(files[0].0, "workflows/Supplier rates refresh.workflow.yaml");
                assert_eq!(files[0].1, yaml.as_bytes());
            }
            other => panic!("{other:?}"),
        }
    }
    // The Push Service reports the pull request sent; the file in the repository has that commit.
    let uuid: uuid::Uuid = submission.as_str().parse().unwrap();
    let _ = f
        .push
        .notices
        .send(ordin_push::PushNotice::PullRequestOpened {
            submission: ordin_push::SubmissionId(uuid),
            origin: ordin_push::SubmissionOrigin::WorkflowShare {
                workspace_id: f.ws,
                file_name: "Supplier rates refresh.workflow.yaml".to_owned(),
            },
            pull_request: ordin_types::domain::agents::PullRequestRef {
                number: 7,
                url: "https://github.com/acme/supplier-portal/pull/7".to_owned(),
                state: ordin_types::domain::agents::PullRequestState::Open,
            },
        });
    let host = f.host.clone();
    let sent = async {
        for _ in 0..200 {
            let v = host.workflows_get(id, None).await.unwrap();
            if v.workflow
                .shared
                .as_ref()
                .is_some_and(|s| !s.changed_here_not_sent)
            {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        false
    };
    assert!(sent.await, "the share is recorded as sent");
    f.projects.contents.lock().unwrap().insert(
        "Supplier rates refresh.workflow.yaml".to_owned(),
        (yaml.into_bytes(), "9f2c4e1".to_owned()),
    );
    f.host.recheck().await;
    let shared = f
        .host
        .workflows_get(id, None)
        .await
        .unwrap()
        .workflow
        .shared
        .unwrap();
    assert_eq!(shared.followed_commit, "9f2c4e1");
    assert!(!shared.newer_in_repository);
}

// Scenario: Edge — a newer version in the repository is noticed, not used
#[tokio::test]
async fn edge_a_newer_version_in_the_repository_is_noticed_not_used() {
    let (f, a, _) = background().await;
    install(&f, &[("analyst", &a)]);
    supplier_portal(&f);
    let wf = file(
        "Supplier rates refresh",
        &[("analyst", &a)],
        vec![step("1", "analyst")],
    );
    let name = "Supplier rates refresh.workflow.yaml";
    f.projects.contents.lock().unwrap().insert(
        name.to_owned(),
        (crate::wff::to_yaml(&wf).into_bytes(), "c0ffee1".to_owned()),
    );
    let (id, _) = f
        .host
        .workflows_add_from_repository(
            f.ws,
            ordin_types::domain::ShareHost::Github,
            "acme/supplier-portal".to_owned(),
            "workflows".to_owned(),
            name.to_owned(),
        )
        .await
        .unwrap();
    f.host
        .workflows_save(id, None, wf.clone(), "First".to_owned())
        .await
        .unwrap();
    f.projects.contents.lock().unwrap().get_mut(name).unwrap().1 = "d00d2".to_owned();
    f.host.recheck().await;
    let view = f.host.workflows_get(id, None).await.unwrap();
    assert!(view.workflow.shared.unwrap().newer_in_repository);
    assert_eq!(view.versions.len(), 1, "the latest version is unchanged");
    assert_eq!(view.version, Some(1));
}

// Scenario: Error — start fresh while a crew runs
#[tokio::test]
async fn error_start_fresh_while_a_crew_runs() {
    let (f, _, _) = background().await;
    f.open().await;
    *f.orchestrator.running.lock().unwrap() = Some(ordin_types::CrewRunId::new());
    let e = f.host.prepare_start_fresh(f.ws, None).await.unwrap_err();
    assert_eq!(e.code(), "OH_CREW_RUNNING");
}

// Scenario: Happy path — summarise starts a seeded session and keeps the earlier conversation
#[tokio::test]
async fn happy_path_summarise_starts_a_seeded_session_and_keeps_the_earlier_conversation() {
    let (f, _, _) = background().await;
    let first = f.open().await;
    f.tool(
        "show",
        json!({ "block": { "kind": "question", "question": "Which supplier?", "choices": ["Kestrel", "Acme"], "allow_other": false } }),
    )
    .await
    .unwrap();
    f.supervisor.respond(Arc::new(|_, text| {
        Box::pin(async move {
            Some(if text.contains("Write a summary") {
                "We compared three quotes; Kestrel is cheapest.".to_owned()
            } else {
                "Ready.".to_owned()
            })
        })
    }));
    f.host.summarise(f.ws).await.unwrap();
    let earlier: (String, Option<String>) =
        sqlx::query_as("SELECT state, ended_by FROM ordin_conversations WHERE run_id = ?")
            .bind(first.to_string())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    assert_eq!(
        earlier,
        ("earlier".to_owned(), Some("summarised".to_owned()))
    );
    let seeds = f.supervisor.seeds();
    assert_eq!(seeds.len(), 2, "a new run is started");
    assert!(seeds[1].contains("We compared three quotes; Kestrel is cheapest."));
    assert!(
        seeds[1].contains("Waiting for the person: question \"Which supplier?\""),
        "{}",
        seeds[1]
    );
    assert_ne!(f.host.current_run(f.ws), Some(first));
    // OH-SUM-1: the summary is Ordin writing for its next session; the person never sees it as a
    // message (found on Windows, where it appeared above the person's next message).
    let shown: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ordin_messages WHERE text = ?")
        .bind("We compared three quotes; Kestrel is cheapest.")
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(shown, 0, "the summary is not stored or shown as a message");
}

// Scenario: Edge — a message after a break of more than an hour continues from a summary
#[tokio::test]
async fn edge_a_message_after_a_break_of_more_than_an_hour_continues_from_a_summary() {
    let (f, _, _) = background().await;
    let first = f.open().await;
    // The planner's rule: the last message was 70 minutes ago.
    *f.usage.continue_from_summary.lock().unwrap() = true;
    // OH-BRK-1: the message is accepted at once; the summary and the new conversation follow in
    // the background.
    let accepted = f
        .host
        .send_message(f.ws, "where were we".to_owned(), vec![])
        .await
        .unwrap();
    assert!(accepted.accepted);
    let mut ended: Option<String> = None;
    for _ in 0..100 {
        ended = sqlx::query_scalar("SELECT ended_by FROM ordin_conversations WHERE run_id = ?")
            .bind(first.to_string())
            .fetch_one(&f.pool)
            .await
            .unwrap();
        if ended.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(ended.as_deref(), Some("after_break"));
    assert!(
        eventually(|| f.host.current_run(f.ws).is_some_and(|r| r != first)).await,
        "a new conversation is open"
    );
    let second = f.host.current_run(f.ws).unwrap();
    assert_ne!(second, first);
    let sup = f.supervisor.clone();
    assert!(
        eventually(|| sup
            .turns
            .lock()
            .unwrap()
            .iter()
            .any(|(run, t)| *run == second && t.ends_with("where were we")))
        .await,
        "the message goes to the new session"
    );
    assert!(!f
        .supervisor
        .turns
        .lock()
        .unwrap()
        .iter()
        .any(|(run, t)| *run == first && t.ends_with("where were we")));
}

// Scenario: Error — a voice setting fixed by the company
#[tokio::test]
async fn error_a_voice_setting_fixed_by_the_company() {
    let mut p = background_policy();
    p.voice_defaults.tone = Some(ordin_types::domain::VoiceTone::Warm);
    let f = fixture_with(config(), p).await;
    let e = f
        .host
        .voice_update(ordin_service_api::commands::voice::VoiceUpdateInput {
            tone: Some(ordin_types::domain::VoiceTone::Direct),
            register: None,
            length: None,
            name_to_use: None,
            notes: None,
        })
        .await
        .unwrap_err();
    assert_eq!(e.code(), "VOICE_FIXED_BY_COMPANY");
}

// Scenario: Error — show with an invalid block
#[tokio::test]
async fn error_show_with_an_invalid_block() {
    let (f, _, _) = background().await;
    f.open().await;
    f.blocks
        .reject
        .lock()
        .unwrap()
        .push(ordin_service_api::blocks::BlockRejection {
            code: "BLOCK_CHOICES_INVALID".to_owned(),
            at: "/choices".to_owned(),
            reason: "A question has 2 to 4 choices.".to_owned(),
        });
    let shown_before = events(&f, "block_shown").len();
    let e = f
        .tool(
            "show",
            json!({ "block": { "kind": "question", "question": "Which?", "choices": ["a", "b", "c", "d", "e", "f"], "allow_other": false } }),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code, "BLOCK_REFUSED");
    assert!(e.message.starts_with("A question has 2 to 4 choices."));
    assert_eq!(
        events(&f, "block_shown").len(),
        shown_before,
        "no BlockShown event"
    );
}

// Scenario: Happy path — a shown question is waiting and announced
#[tokio::test]
async fn happy_path_a_shown_question_is_waiting_and_announced() {
    let (f, _, _) = background().await;
    f.open().await;
    let out = f
        .tool(
            "show",
            json!({ "block": { "kind": "question", "question": "Which supplier?", "choices": ["Kestrel", "Acme", "Northwind"], "allow_other": false } }),
        )
        .await
        .unwrap();
    let block_id: ordin_types::BlockId =
        serde_json::from_value(out.structured.unwrap()["block_id"].clone()).unwrap();
    let ws = f.ws.to_string();
    assert!(events(&f, "block_shown")
        .iter()
        .any(|s| s.contains("Which supplier?")));
    let needs = |f: &Fixture| {
        events(f, "needs_you_changed")
            .iter()
            .filter(|s| s.contains(&ws))
            .count()
    };
    assert_eq!(needs(&f), 1);
    f.host
        .answer_block(
            f.ws,
            block_id,
            BlockAnswer::Choice {
                choice: "Kestrel".to_owned(),
            },
        )
        .await
        .unwrap();
    assert!(!f.host.block(f.ws, block_id).await.unwrap().waiting);
    assert_eq!(needs(&f), 2);
    let sup = f.supervisor.clone();
    assert!(
        eventually(|| sup
            .turn_texts()
            .last()
            .is_some_and(|t| t.ends_with("Kestrel")))
        .await
    );
}

/// "Invoice check" with the required document input "Invoice", saved up to version 2.
async fn invoice_check(f: &Fixture, a: &DirectoryAgent) -> ordin_types::WorkflowId {
    use ordin_types::wire::workflow_file::{
        StepUse, WorkflowFileInput, WorkflowFileStep, WorkflowInputKind,
    };
    install(f, &[("checker", a)]);
    let mut first = step("1", "checker");
    if let WorkflowFileStep::Agent { uses, .. } = &mut first {
        uses.push(StepUse::Input("Invoice".to_owned()));
    }
    let mut wf = file("Invoice check", &[("checker", a)], vec![first]);
    wf.inputs = vec![WorkflowFileInput {
        label: "Invoice".to_owned(),
        kind: WorkflowInputKind::Document,
        required: true,
    }];
    let id = saved(f, &wf).await;
    f.host
        .workflows_save(id, Some(1), wf, "Two".to_owned())
        .await
        .unwrap();
    id
}

fn invoice(content: ordin_types::wire::ContentRef) -> Vec<crate::types::RunInput> {
    vec![crate::types::RunInput {
        label: "Invoice".to_owned(),
        value: crate::types::RunInputValue::Document { content },
    }]
}

// Scenario: Happy path — a turned-on trigger starts a workflow at once
#[tokio::test]
async fn happy_path_a_turned_on_trigger_starts_a_workflow_at_once() {
    let (f, a, _) = background().await;
    let id = invoice_check(&f, &a).await;
    let trigger = ordin_types::TriggerId::new();
    let artefact = ArtefactId::new();
    let crew = f
        .host
        .start_workflow_run(crate::types::StartWorkflowRun {
            workspace_id: f.ws,
            workflow_id: id,
            version: 2,
            inputs: invoice(ordin_types::wire::ContentRef::Artefact {
                artefact_id: artefact,
            }),
            started_by: crate::types::RunStartedBy::EventTrigger {
                trigger_id: trigger,
                event_label: "invoice.received".to_owned(),
            },
        })
        .await
        .unwrap();
    assert!(
        f.supervisor.turns.lock().unwrap().is_empty(),
        "no Ordin turn"
    );
    assert!(
        f.supervisor.starts.lock().unwrap().is_empty(),
        "no Ordin turn"
    );
    assert!(
        !events(&f, "block_shown")
            .iter()
            .any(|s| s.contains("crew_plan")),
        "no crew plan"
    );
    {
        let started = f.orchestrator.started.lock().unwrap();
        assert_eq!(started.len(), 1);
        assert!(!started[0].started_by_person);
        assert_eq!(started[0].trigger_id, Some(trigger));
    }
    let (by, inputs): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT started_by, run_inputs FROM crew_charters WHERE crew_run_id = ?")
            .bind(crew.to_string())
            .fetch_one(&f.pool)
            .await
            .unwrap();
    let by: Value = serde_json::from_str(&by.unwrap()).unwrap();
    assert_eq!(by["kind"], "event_trigger");
    assert_eq!(by["trigger_id"], trigger.to_string());
    let inputs: Value = serde_json::from_str(&inputs.unwrap()).unwrap();
    assert_eq!(inputs[0]["label"], "Invoice");
    assert_eq!(
        inputs[0]["value"]["content"]["artefact_id"],
        artefact.to_string()
    );
}

// Scenario: Error — immediate start without a required input
#[tokio::test]
async fn error_immediate_start_without_a_required_input() {
    let (f, a, _) = background().await;
    let id = invoice_check(&f, &a).await;
    let e = f
        .host
        .start_workflow_run(crate::types::StartWorkflowRun {
            workspace_id: f.ws,
            workflow_id: id,
            version: 2,
            inputs: vec![],
            started_by: crate::types::RunStartedBy::EventTrigger {
                trigger_id: ordin_types::TriggerId::new(),
                event_label: "invoice.received".to_owned(),
            },
        })
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_INPUT_MISSING");
    assert!(matches!(e, crate::OhError::InputMissing { ref label } if label == "Invoice"));
    assert!(f.orchestrator.started.lock().unwrap().is_empty());
}

// Scenario: Edge — Run again reports a newer version
#[tokio::test]
async fn edge_run_again_reports_a_newer_version() {
    let (f, a, _) = background().await;
    let id = invoice_check(&f, &a).await;
    let artefact = ArtefactId::new();
    let crew = f
        .host
        .start_workflow_run(crate::types::StartWorkflowRun {
            workspace_id: f.ws,
            workflow_id: id,
            version: 2,
            inputs: invoice(ordin_types::wire::ContentRef::Artefact {
                artefact_id: artefact,
            }),
            started_by: crate::types::RunStartedBy::Person {
                channel: "chat".to_owned(),
            },
        })
        .await
        .unwrap();
    let latest = f.host.workflows_get(id, Some(2)).await.unwrap().file;
    f.host
        .workflows_save(id, Some(2), latest, "Three".to_owned())
        .await
        .unwrap();
    let again = f.host.run_again_inputs(crew).await.unwrap();
    assert_eq!(again.version, 2);
    assert_eq!(again.newer_version, Some(3));
    assert_eq!(again.inputs[0].input.label, "Invoice");
    assert!(matches!(
        again.inputs[0].value,
        Some(crate::types::RunInputValue::Document {
            content: ordin_types::wire::ContentRef::Artefact { artefact_id }
        }) if artefact_id == artefact
    ));
}

// Scenario: Edge — the trigger tool is offered only with a trigger desk
#[tokio::test]
async fn edge_the_trigger_tool_is_offered_only_with_a_trigger_desk() {
    let (f, _, _) = background().await;
    let run = f.open().await;
    let grant = ordin_mcp_front::SessionGrant {
        run_id: run,
        workspace_id: f.ws,
        agent_instance_id: f.host.config.ordin_agent_instance_id,
        harness: ordin_types::domain::identity::HarnessKind::ClaudeCode,
        profile: crate::types::ProgramProfile::Desktop,
        caller: ordin_mcp_front::ToolCaller::Ordin,
        scope_id: None,
        coding_task_id: None,
        only_servers: None,
    };
    let offered = f.host.tool_provider().offer(&grant).await.unwrap();
    let names: Vec<&str> = offered.iter().map(|k| k.tool.as_str()).collect();
    assert!(names.contains(&"delegate"));
    assert!(!names.contains(&"propose_trigger"), "{names:?}");
}

// Scenario: Error — an unknown generation template
#[tokio::test]
async fn error_an_unknown_generation_template() {
    use crate::types::AuthoringHost;
    let (f, _, _) = background().await;
    let e = f
        .host
        .generate(crate::types::GenerationRequest {
            template_id: "unknown-template",
            data: json!({}),
            workspace_id: f.ws,
        })
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_TEMPLATE_UNKNOWN");
    assert_eq!(task_count(&f), 0, "no run is started");
}

// Scenario: Happy path — refreshing a dashboard made by a task
#[tokio::test]
async fn happy_path_refreshing_a_dashboard_made_by_a_task() {
    use ordin_types::domain::runs::{Dashboard, DashboardSource};
    let (f, _, _) = background().await;
    let d = ordin_types::DashboardId::new();
    let task = ordin_types::TaskId::new();
    f.journal.dashboards.lock().unwrap().insert(
        d,
        Dashboard {
            id: d,
            workspace_id: f.ws,
            name: "Supplier prices".to_owned(),
            made_by: DashboardSource::Task { task_id: task },
            inputs: vec![ordin_types::common::InputUsed {
                name: "Q3 quotes.xlsx".to_owned(),
                location: "Docs".to_owned(),
                version_read: "4".to_owned(),
            }],
            refresh_runs: vec![],
            kept_at: Timestamp::now(),
        },
    );
    let started = f.host.dashboards_refresh(f.ws, d).await.unwrap();
    let run_id = match started {
        ordin_service_api::commands::dashboards::DashboardRefreshStarted::Run { run_id } => run_id,
        other => panic!("{other:?}"),
    };
    {
        let tasks = f.supervisor.tasks.lock().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].0.run_again_of, Some(task));
        // On Ordin's own instance (OH-R1-4).
        assert_eq!(
            tasks[0].0.target,
            ordin_types::domain::runs::TaskTarget::AgentInstance {
                agent_instance_id: f.host.config.ordin_agent_instance_id,
            }
        );
        assert!(tasks[0]
            .0
            .free_text
            .as_deref()
            .unwrap()
            .contains("Q3 quotes.xlsx"));
    }
    assert_eq!(
        f.journal.refreshes.lock().unwrap().clone(),
        vec![(d, run_id)]
    );
}

// Scenario: Happy path — removing a workspace removes its workflows and conversations
#[tokio::test]
async fn happy_path_removing_a_workspace_removes_its_workflows_and_conversations() {
    let (f, a, b) = background().await;
    install(&f, &[("analyst", &a)]);
    saved(
        &f,
        &file(
            "Supplier rates refresh",
            &[("analyst", &a)],
            vec![step("1", "analyst")],
        ),
    )
    .await;
    let run = f.open().await;
    f.tool(
        "create_crew",
        json!({ "purpose": "Compare", "members": [ { "agent": key(&a), "role": "x" }, { "agent": key(&b), "role": "y" } ] }),
    )
    .await
    .unwrap();
    f.host.workspace_removed(f.ws).await.unwrap();
    assert!(
        f.supervisor.cancelled.lock().unwrap().contains(&run),
        "Ordin's run is stopped"
    );
    assert_eq!(f.host.current_run(f.ws), None);
    let ws = f.ws.to_string();
    for table in ["ordin_conversations", "workflows", "crew_charters"] {
        let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {table} WHERE workspace_id = ?"
        )))
        .bind(&ws)
        .fetch_one(&f.pool)
        .await
        .unwrap();
        assert_eq!(n, 0, "{table}");
    }
}

// Scenario: A chat file starts a workflow through its place
#[tokio::test]
async fn a_chat_file_starts_a_workflow_through_its_place() {
    let (f, a, _) = background().await;
    let id = invoice_check(&f, &a).await;
    let place = PlaceId::new();
    let trigger = ordin_types::TriggerId::new();
    f.host
        .start_workflow_run(crate::types::StartWorkflowRun {
            workspace_id: f.ws,
            workflow_id: id,
            version: 2,
            inputs: invoice(ordin_types::wire::ContentRef::PlaceFile {
                place_id: place,
                relative_path: "Inbox/inv-118.pdf".to_owned(),
            }),
            started_by: crate::types::RunStartedBy::ChatFileTrigger {
                trigger_id: trigger,
                file_name: "inv-118.pdf".to_owned(),
            },
        })
        .await
        .unwrap();
    assert_eq!(
        f.f.count("files.read_file"),
        1,
        "the File Service reads the file"
    );
    {
        let artefacts = f.journal.artefacts.lock().unwrap();
        assert_eq!(artefacts.len(), 1);
        assert_eq!(artefacts[0].path, "Inbox/inv-118.pdf");
        assert!(matches!(
            artefacts[0].location,
            ordin_types::domain::places::ArtefactLocation::Place { place_id } if place_id == place
        ));
    }
    let started = f.orchestrator.started.lock().unwrap();
    assert_eq!(started[0].inputs[0].label, "Invoice");
    assert!(matches!(
        &started[0].inputs[0].value,
        ordin_crew_orchestrator::CrewInputValue::Documents { artefacts } if artefacts.len() == 1
    ));
    assert!(!started[0].started_by_person);
    assert_eq!(started[0].trigger_id, Some(trigger));
}

// Scenario: An unreadable place-file input stops the start
#[tokio::test]
async fn an_unreadable_place_file_input_stops_the_start() {
    let (f, a, _) = background().await;
    let id = invoice_check(&f, &a).await;
    f.f.always("files.read_file", "FILE_NOT_FOUND", false);
    let e = f
        .host
        .start_workflow_run(crate::types::StartWorkflowRun {
            workspace_id: f.ws,
            workflow_id: id,
            version: 2,
            inputs: invoice(ordin_types::wire::ContentRef::PlaceFile {
                place_id: PlaceId::new(),
                relative_path: "Inbox/inv-118.pdf".to_owned(),
            }),
            started_by: crate::types::RunStartedBy::ChatFileTrigger {
                trigger_id: ordin_types::TriggerId::new(),
                file_name: "inv-118.pdf".to_owned(),
            },
        })
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_INPUT_UNREADABLE");
    assert!(matches!(e, crate::OhError::InputUnreadable { ref label } if label == "Invoice"));
    assert!(f.orchestrator.started.lock().unwrap().is_empty());
}

const FORMAT_2: &str = r#"format: 2
name: "Contract pass"
purpose: "Review each contract"
inputs:
  - label: "Contracts"
    kind: "list"
    required: true
agents:
  - name: "contract-reviewer"
    version: "^1.2"
    role: "Reviews contracts"
steps:
  - kind: "each"
    position: "1"
    list:
      input: "Contracts"
    max_items: 10
    steps:
      - kind: "agent"
        position: "1.1"
        agent: "contract-reviewer"
        line: "Review the contract"
        uses:
          - item: true
        asks_first: false
        ordin_checks: false
limits:
  max_rounds: 10
  wall_clock_minutes: 30
  agents_at_once: 2
  failure_repeat_limit: 2
  external_action_agents: []
origin: "Exported from ordin"
"#;

// Scenario: A format 2 workflow file is read into the current model
#[tokio::test]
async fn a_format_2_workflow_file_is_read_into_the_current_model() {
    let (f, _, _) = background().await;
    let reviewer = agent("contract-reviewer");
    f.inspector.agents.lock().unwrap().push(reviewer.clone());
    f.catalogue
        .install(&reviewer, "urn:ordin:agent:contract-reviewer", "1.2.3");
    let (id, report) = f
        .host
        .workflows_import(
            f.ws,
            "contracts.workflow.yaml".to_owned(),
            FORMAT_2.to_owned(),
        )
        .await
        .unwrap();
    assert!(report.problems.is_empty(), "{report:?}");
    let view = f.host.workflows_get(id, None).await.unwrap();
    assert_eq!(
        view.workflow.state,
        ordin_types::domain::WorkflowState::NotSavedYet
    );
    let agent0 = &view.file.agents[0];
    assert_eq!(agent0.key, "contract-reviewer");
    assert_eq!(
        agent0.repository,
        ordin_types::wire::workflow_file::AgentRepositoryRef::ProfileKey("company".to_owned())
    );
    assert_eq!(agent0.id, "urn:ordin:agent:contract-reviewer");
    assert!(matches!(
        view.file.steps[0],
        ordin_types::wire::workflow_file::WorkflowFileStep::Each { .. }
    ));
}

// Scenario: A format 1 file names an agent the workspace lacks
#[tokio::test]
async fn a_format_1_file_names_an_agent_the_workspace_lacks() {
    let (f, _, _) = background().await;
    let yaml = r#"format: 1
name: "Invoice pass"
purpose: "Check invoices"
inputs: []
agents:
  - name: "invoice-checker"
    version: "^1.0"
    role: "Checks invoices"
steps:
  - position: "1"
    agent: "invoice-checker"
    line: "Check the invoice"
    uses: []
    asks_first: false
    ordin_checks: false
limits:
  max_rounds: 10
  wall_clock_minutes: 30
  agents_at_once: 2
  failure_repeat_limit: 2
  external_action_agents: []
origin: "Exported from ordin"
"#;
    let (id, report) = f
        .host
        .workflows_import(f.ws, "invoices.workflow.yaml".to_owned(), yaml.to_owned())
        .await
        .unwrap();
    assert!(
        wff_codes(&report).contains(&("WFF_AGENT_UNMATCHED".to_owned(), "/agents/0".to_owned())),
        "{report:?}"
    );
    let view = f.host.workflows_get(id, None).await.unwrap();
    let e = f
        .host
        .workflows_save(id, None, view.file, "First".to_owned())
        .await
        .unwrap_err();
    assert_eq!(
        e.code(),
        "OH_CHECK_FAILED",
        "Save waits until an agent is picked"
    );
}

// Scenario: A file without a format value is refused
#[tokio::test]
async fn a_file_without_a_format_value_is_refused() {
    let (f, _, _) = background().await;
    let e = f
        .host
        .workflows_import(
            f.ws,
            "x.workflow.yaml".to_owned(),
            "name: \"No format\"\n".to_owned(),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "WFF_FORMAT_UNSUPPORTED");
    assert_eq!(f.count("SELECT COUNT(*) FROM workflows").await, 0);
}

// Scenario: Memory rules come from the agent's Memory part
#[tokio::test]
async fn memory_rules_come_from_the_agents_memory_part() {
    let (f, a, b) = background().await;
    f.inspector.memory.lock().unwrap().insert(
        a.agent_instance_id,
        crate::ports::DeclaredMemory {
            retains: vec!["supplier_terms".to_owned()],
            excludes: vec![],
            max_age_days: Some(90),
            unbounded: false,
            store_location: "workspace".to_owned(),
        },
    );
    f.open().await;
    let out = f
        .tool(
            "create_crew",
            json!({ "purpose": "Compare", "members": [ { "agent": key(&a), "role": "x" }, { "agent": key(&b), "role": "y" } ] }),
        )
        .await
        .unwrap();
    let id: ordin_types::CrewCharterId =
        serde_json::from_value(out.structured.unwrap()["crew_charter_id"].clone()).unwrap();
    f.host.crews_start(id).await.unwrap();
    let rules = f.memory.rules.lock().unwrap();
    let m = rules.get(&a.agent_instance_id).expect("rules for M");
    assert_eq!(m.retains, vec!["supplier_terms".to_owned()]);
    assert_eq!(m.max_age_days, Some(90));
    assert!(
        !rules.contains_key(&b.agent_instance_id),
        "no Memory part, no call"
    );
}

/// The Supervisor stand-in whose tasks run: each task's run pauses once for an approval, then ends.
struct TaskRuns {
    inner: Arc<FakeSupervisor>,
    tasks: std::sync::Mutex<Vec<RunId>>,
}

#[async_trait::async_trait]
impl crate::ports::SupervisorPort for TaskRuns {
    async fn start_conversation(
        &self,
        req: ordin_session_supervisor::ConversationStart,
    ) -> Result<RunId, crate::ports::DepError> {
        self.inner.start_conversation(req).await
    }
    fn subscribe(
        &self,
        run_id: RunId,
    ) -> Result<
        tokio::sync::broadcast::Receiver<ordin_runtime_adapter::RunEvent>,
        crate::ports::DepError,
    > {
        let rx = self.inner.subscribe(run_id)?;
        if self.tasks.lock().unwrap().contains(&run_id) {
            let tx = self
                .inner
                .runs
                .lock()
                .unwrap()
                .get(&run_id)
                .cloned()
                .unwrap();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(20)).await;
                let _ = tx.send(ordin_runtime_adapter::RunEvent::Paused {
                    reason: ordin_runtime_adapter::PauseReason::Approval {
                        approval_id: ordin_types::ApprovalId::new(),
                    },
                });
                let _ = tx.send(ordin_runtime_adapter::RunEvent::Ended {
                    end: ordin_runtime_adapter::RunEnd {
                        reason: ordin_runtime_adapter::RunEndReason::Completed,
                        final_message: Some("Compared three quotes.".to_owned()),
                        artefacts: vec![],
                    },
                });
            });
        }
        Ok(rx)
    }
    async fn resume_with_decision(
        &self,
        run_id: RunId,
        decision: ordin_runtime_adapter::ResumeDecision,
    ) -> Result<(), crate::ports::DepError> {
        self.inner.resume_with_decision(run_id, decision).await
    }
    async fn cancel(&self, run_id: RunId) -> Result<(), crate::ports::DepError> {
        self.inner.cancel(run_id).await
    }
    async fn submit_task(
        &self,
        task: ordin_types::domain::runs::Task,
        work_kind: WorkKind,
        started_by_person: bool,
        coding_task_id: Option<ordin_types::CodingTaskId>,
    ) -> Result<RunId, crate::ports::DepError> {
        let run_id = self
            .inner
            .submit_task(task, work_kind, started_by_person, coding_task_id)
            .await?;
        let (tx, _) = tokio::sync::broadcast::channel(16);
        self.inner.runs.lock().unwrap().insert(run_id, tx);
        // The run's own events, not Ordin's seed answer.
        self.inner.subscribed.lock().unwrap().push(run_id);
        self.tasks.lock().unwrap().push(run_id);
        Ok(run_id)
    }
    async fn list(
        &self,
        ws: ordin_types::WorkspaceId,
    ) -> Result<Vec<ordin_types::domain::runs::Run>, crate::ports::DepError> {
        self.inner.list(ws).await
    }
    async fn run_task(
        &self,
        run_id: RunId,
    ) -> Result<Option<crate::ports::RunWithTask>, crate::ports::DepError> {
        self.inner.run_task(run_id).await
    }
    fn context_used_percent(&self, run_id: RunId) -> Option<u8> {
        self.inner.context_used_percent(run_id)
    }
}

// Scenario: A rehearsal returns its evidence
#[tokio::test]
async fn a_rehearsal_returns_its_evidence() {
    use crate::types::AuthoringHost;
    let (f, _, _) = background().await;
    let ports = crate::ports::Ports {
        supervisor: Arc::new(TaskRuns {
            inner: f.supervisor.clone(),
            tasks: std::sync::Mutex::new(vec![]),
        }),
        orchestrator: f.orchestrator.clone(),
        inspector: f.inspector.clone(),
        usage: f.usage.clone(),
        workspaces: f.workspaces.clone(),
        journal: f.journal.clone(),
        catalogue: f.catalogue.clone(),
        memory: f.memory.clone(),
        knowledge: f.knowledge.clone(),
        push: f.push.clone(),
        projects: f.projects.clone(),
        files: Arc::new(FakeFiles {
            f: f.f.clone(),
            missing: std::sync::Mutex::new(vec![]),
            places: std::sync::Mutex::new(std::collections::HashMap::new()),
        }),
        localisation: f.localisation.clone(),
        blocks: f.blocks.clone(),
    };
    let host = OrdinHost::with_ports(config(), background_policy(), ports, f.pool.clone());
    *f.journal.artefact_names.lock().unwrap() = vec!["comparison.md".to_owned()];
    let evidence = host
        .rehearse(crate::types::RehearsalRunRequest {
            agent_draft_id: ordin_types::AgentDraftId::new(),
            agent_repository_id: ordin_types::AgentRepositoryId::new(),
            worktree: std::path::PathBuf::from("/tmp/draft"),
            situation: "Compare the three supplier quotes".to_owned(),
            documents: vec![],
            workspace_id: f.ws,
        })
        .await
        .unwrap();
    assert_eq!(evidence.approvals_asked, 1);
    assert_eq!(evidence.final_message, "Compared three quotes.");
    assert_eq!(evidence.artefact_names, vec!["comparison.md".to_owned()]);
    // In R1 `tools_used` is empty: no run event names a tool (OrdinHost.md step 42.3, OH-R1-2).
    assert!(evidence.tools_used.is_empty());
    assert_eq!(
        f.catalogue.trials.lock().unwrap().len(),
        1,
        "the rehearsal runs on a trial instance"
    );
}

// Scenario: Summaries count turns until context use is reported
#[tokio::test]
async fn summaries_count_turns_until_context_use_is_reported() {
    let (f, _, _) = background().await;
    f.open().await;
    // 39 Ordin turns since the last summary; the next one is the 40th.
    if let Some(c) = crate::host::lock(&f.host.state).convs.get_mut(&f.ws) {
        c.turns_since_summary = 39;
    }
    f.host
        .send_message(f.ws, "one more".to_owned(), vec![])
        .await
        .unwrap();
    let usage = f.usage.clone();
    assert!(
        eventually(|| usage.summarise_asked.lock().unwrap().contains(&(None, 40))).await,
        "{:?}",
        f.usage.summarise_asked.lock().unwrap()
    );
}

// Extra gate (not a scenario): two seeds built from the same inputs are byte-identical — the
// directory in a different order, waiting items and a carried summary give the same bytes.
#[tokio::test]
async fn extra_gate_two_core_seeds_from_the_same_inputs_are_byte_identical() {
    let (f, a, b) = background().await;
    f.open().await;
    f.tool(
        "show",
        json!({ "block": { "kind": "question", "question": "Which supplier?", "choices": ["Kestrel", "Acme"], "allow_other": false } }),
    )
    .await
    .unwrap();
    let carry = crate::host::Carry {
        summary: Some("We compared three quotes.".to_owned()),
        ..Default::default()
    };
    let (one, tag_one) = f.host.build_seed(f.ws, &carry).await.unwrap();
    *f.inspector.agents.lock().unwrap() = vec![b, a];
    let (two, tag_two) = f.host.build_seed(f.ws, &carry).await.unwrap();
    assert_eq!(one.as_bytes(), two.as_bytes());
    assert_eq!(tag_one, tag_two);
}

// Scenario: Happy path — the provider passes the desktop's description limit (OH-REC-1)
#[tokio::test]
async fn happy_path_the_provider_passes_the_desktops_description_limit() {
    use ordin_mcp_front::ToolProvider;
    let f = fixture().await;
    let servers = f.host.tool_provider().servers();
    let tools: Vec<_> = servers.iter().flat_map(|s| s.tools.iter()).collect();
    assert!(tools.iter().any(|t| t.name == "remember"));
    // Every rule of the MCP Front's registration check on the desktop (McpFront.md SA-RECON-04).
    let mut refused = Vec::new();
    for tool in tools {
        let n = tool.description.chars().count();
        if !(1..=120).contains(&n) || tool.description.contains(['\n', '\r']) {
            refused.push(format!("{}: description {n}", tool.name));
        }
        if !(1..=300).contains(&tool.argument_form.chars().count()) {
            refused.push(format!("{}: argument form", tool.name));
        }
        if !(1..=200).contains(&tool.plain.chars().count()) {
            refused.push(format!("{}: plain words", tool.name));
        }
        if tool.input_schema.get("type").and_then(|t| t.as_str()) != Some("object") {
            refused.push(format!("{}: input schema", tool.name));
        }
    }
    // OH-REC-4: `decide` and `change_setting` publish one object schema each, so no tool breaks a rule.
    assert!(refused.is_empty(), "{refused:?}");
}

// Scenario: Edge case — the seed names the crate's instructions version (OH-REC-2)
#[tokio::test]
async fn edge_case_the_seed_names_the_crates_instructions_version() {
    let mut config = crate::testkit::config();
    config.fixed_instructions_version = crate::FIXED_INSTRUCTIONS_VERSION.to_owned();
    let f = fixture_with(config, crate::testkit::policy()).await;
    f.host
        .send_message(f.ws, "hello".to_owned(), vec![])
        .await
        .unwrap();
    let starts = f.supervisor.starts.lock().unwrap().clone();
    assert!(starts[0].seed.contains(&format!(
        "tool catalogue are version {}",
        crate::FIXED_INSTRUCTIONS_VERSION
    )));
}

// OH-DIR-1: found on Windows — after **Add it** during a conversation, Ordin was told only the new
// agent's name, so it had no key to give it work with `delegate`.
// spec: ordin-desktop/components/OrdinHost#OH-DIR-1
#[tokio::test]
async fn an_agent_added_during_the_conversation_reaches_ordin_with_its_key() {
    let f = fixture_with(config(), policy()).await;
    let run = f.open().await;
    let added = agent("Document agent");
    f.inspector.agents.lock().unwrap().push(added.clone());
    f.inspector.changes_tx.unbounded_send(f.ws).unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    f.host
        .send_message(f.ws, "Go ahead".to_owned(), vec![])
        .await
        .unwrap();
    let sup = f.supervisor.clone();
    let line = format!("- Document agent (key {})", key(&added));
    assert!(
        eventually(|| sup.turns.lock().unwrap().iter().any(|(r, t)| *r == run
            && t.starts_with("Agent directory changed:\n")
            && t.contains(&line)
            && t.ends_with("Go ahead")))
        .await,
        "{:?}",
        sup.turns.lock().unwrap()
    );
}

// OH-FA-1: found in the Phase 5 walk — find_agent's lines named the agent, its summary and source only,
// and propose_agent needs the agent repository id and identifier; an agent already in the workspace
// shows its key for delegate instead.
// spec: ordin-desktop/components/OrdinHost#OH-FA-1
#[tokio::test]
async fn find_agent_gives_ordin_what_propose_agent_and_delegate_take() {
    let f = fixture_with(config(), policy()).await;
    let run = f.open().await;
    let repo = ordin_types::AgentRepositoryId::new();
    let installed = ordin_types::AgentInstanceId::new();
    *f.catalogue.found.lock().unwrap() = vec![
        ordin_package_catalogue::AgentFound {
            agent: ordin_types::PartId::new(),
            agent_repository_id: repo,
            identifier: "urn:ordin:default:workspaces:document-agent".to_owned(),
            name: "Document agent".to_owned(),
            summary: "Summarises documents.".to_owned(),
            from: "ordin".to_owned(),
            in_workspace: None,
        },
        ordin_package_catalogue::AgentFound {
            agent: ordin_types::PartId::new(),
            agent_repository_id: repo,
            identifier: "urn:ordin:default:workspaces:cspa-431".to_owned(),
            name: "Software engineer".to_owned(),
            summary: "Builds software.".to_owned(),
            from: "ordin".to_owned(),
            in_workspace: Some(installed),
        },
    ];
    let out = f
        .host
        .tool(
            &f.ctx_for(run),
            "find_agent",
            json!({ "need": "summaries" }),
        )
        .await
        .unwrap();
    assert_eq!(
        out.text,
        format!(
            "Document agent — Summarises documents. (ordin; agent_repository_id {repo}, agent urn:ordin:default:workspaces:document-agent)\nSoftware engineer — Builds software. (ordin; already in this workspace, key {installed})"
        )
    );
}

/// The rows of one text column, polled until one satisfies `want` (about 2 seconds at most).
async fn poll_rows(f: &Fixture, sql: &'static str, want: impl Fn(&str) -> bool) -> Vec<String> {
    let mut rows = Vec::new();
    for _ in 0..100 {
        rows = sqlx::query_scalar::<_, String>(sql)
            .fetch_all(&f.pool)
            .await
            .unwrap();
        if rows.iter().any(|r| want(r)) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    rows
}

// OH-RC-1: found in the Phase 5 walk — the document agent's task completed and nothing in the
// conversation said so; the person could not see what it produced.
// spec: ordin-desktop/components/OrdinHost#OH-RC-1
#[tokio::test]
async fn a_completed_task_shows_what_it_produced_in_the_conversation() {
    let (f, a, _) = background().await;
    f.open().await;
    f.tool(
        "delegate",
        json!({ "agent": key(&a), "brief": "Compare the quotes" }),
    )
    .await
    .unwrap();
    let run = *f.supervisor.task_runs.lock().unwrap().last().unwrap();
    let artefact = ordin_types::ArtefactId::new();
    f.supervisor.end_with(
        run,
        ordin_runtime_adapter::RunEnd {
            reason: ordin_runtime_adapter::RunEndReason::Completed,
            final_message: Some("Kestrel is cheapest at 1,240 GBP per container.".to_owned()),
            artefacts: vec![artefact],
        },
    );
    let rows = poll_rows(&f, "SELECT body FROM ordin_blocks ORDER BY shown_at", |b| {
        b.contains("\"produced\"")
    })
    .await;
    let produced = rows
        .iter()
        .find(|b| b.contains("\"produced\""))
        .unwrap_or_else(|| panic!("no produced card: {rows:?}"));
    assert!(produced.contains("Kestrel is cheapest at 1,240 GBP per container."));
    assert!(produced.contains(&run.to_string()), "{produced}");
    assert!(produced.contains(&artefact.to_string()), "{produced}");
}

// OH-RC-2: found on the 0.1.1 Mac build — the document agent's summary was on screen, and Ordin, asked to
// "add it to knowledge", said the agent had not sent its summary: nothing told Ordin what a completed task
// said, only that a failed one failed.
// spec: ordin-desktop/components/OrdinHost#OH-RC-2
#[tokio::test]
async fn what_a_completed_task_said_reaches_ordins_next_turn_once() {
    let (f, a, _) = background().await;
    let run = f.open().await;
    f.tool(
        "delegate",
        json!({ "agent": key(&a), "brief": "Compare the quotes" }),
    )
    .await
    .unwrap();
    let task = *f.supervisor.task_runs.lock().unwrap().last().unwrap();
    f.supervisor.end_with(
        task,
        ordin_runtime_adapter::RunEnd {
            reason: ordin_runtime_adapter::RunEndReason::Completed,
            final_message: Some("Kestrel is cheapest at 1,240 GBP per container.".to_owned()),
            artefacts: vec![],
        },
    );
    poll_rows(&f, "SELECT body FROM ordin_blocks ORDER BY shown_at", |b| {
        b.contains("\"produced\"")
    })
    .await;
    for text in ["add it to knowledge", "Thanks"] {
        f.host
            .send_message(f.ws, text.to_owned(), vec![])
            .await
            .unwrap();
        let sup = f.supervisor.clone();
        assert!(
            eventually(|| sup
                .turns
                .lock()
                .unwrap()
                .iter()
                .any(|(r, t)| *r == run && t.ends_with(text)))
            .await
        );
    }
    let turns: Vec<String> = f
        .supervisor
        .turns
        .lock()
        .unwrap()
        .iter()
        .filter(|(r, _)| *r == run)
        .map(|(_, t)| t.clone())
        .collect();
    let first = turns
        .iter()
        .find(|t| t.ends_with("add it to knowledge"))
        .unwrap();
    let second = turns.iter().find(|t| t.ends_with("Thanks")).unwrap();
    assert!(
        first.contains("Tasks finished since your last turn"),
        "{first}"
    );
    assert!(
        first.contains("Supplier analyst said:\nKestrel is cheapest at 1,240 GBP per container."),
        "{first}"
    );
    assert!(
        !second.contains("Tasks finished since your last turn"),
        "{second}"
    );
}

// OH-RC-1: a task that does not complete is one sentence naming the agent.
// spec: ordin-desktop/components/OrdinHost#OH-RC-1
#[tokio::test]
async fn a_task_that_stops_says_so_in_one_sentence() {
    let (f, a, _) = background().await;
    f.open().await;
    f.tool(
        "delegate",
        json!({ "agent": key(&a), "brief": "Compare the quotes" }),
    )
    .await
    .unwrap();
    let run = *f.supervisor.task_runs.lock().unwrap().last().unwrap();
    f.supervisor.end_with(
        run,
        ordin_runtime_adapter::RunEnd {
            reason: ordin_runtime_adapter::RunEndReason::Failed,
            final_message: None,
            artefacts: vec![],
        },
    );
    let want = "Supplier analyst stopped before it finished.";
    let rows = poll_rows(&f, "SELECT text FROM ordin_messages", |t| t == want).await;
    assert!(rows.iter().any(|t| t == want), "{rows:?}");
}

// Scenario: Error path — a command the agent does not offer (OH-DLG-1). Found in the field: Ordin
// passed a library use case's title as a command to the document agent, which has none.
// spec: ordin-desktop/components/OrdinHost#OH-DLG-1
#[tokio::test]
async fn error_a_command_the_agent_does_not_offer() {
    let (f, a, _) = background().await;
    f.open().await;
    let e = f
        .tool(
            "delegate",
            json!({ "agent": key(&a), "brief": "Summarise the PDF", "command": "Summarise documents" }),
        )
        .await
        .expect_err("refused");
    assert_eq!(e.code, "OH_COMMAND_UNKNOWN");
    assert!(!e.retryable);
    assert_eq!(
        e.message,
        "Supplier analyst has no command \"Summarise documents\". It has no commands. Give the work as a brief, without a command."
    );
    assert_eq!(task_count(&f), 0, "no task starts");
}

// OH-DLG-1: an agent with commands names them in the refusal, as the directory lists them.
// spec: ordin-desktop/components/OrdinHost#OH-DLG-1
#[tokio::test]
async fn error_a_command_not_listed_names_the_agents_commands() {
    let (f, mut a, _) = background().await;
    a.plugin_name = Some("supplier-analyst".to_owned());
    a.commands = vec![
        DirectoryCommand {
            name: "compare-quotes".to_owned(),
            session_name: None,
            argument_hint: Some("<quotes>".to_owned()),
            ..Default::default()
        },
        DirectoryCommand {
            name: "rank".to_owned(),
            session_name: None,
            argument_hint: None,
            ..Default::default()
        },
    ];
    f.inspector.agents.lock().unwrap()[0] = a.clone();
    f.open().await;
    let e = f
        .tool(
            "delegate",
            json!({ "agent": key(&a), "brief": "Compare", "command": "compare" }),
        )
        .await
        .expect_err("refused");
    assert_eq!(e.code, "OH_COMMAND_UNKNOWN");
    assert_eq!(
        e.message,
        "Supplier analyst has no command \"compare\". Its commands: supplier-analyst:compare-quotes, supplier-analyst:rank. Give the work as a brief, without a command."
    );
    assert_eq!(task_count(&f), 0);
}

// OH-DLG-1: the name the directory shows on Claude Code, and the command's own name, both start it.
// spec: ordin-desktop/components/OrdinHost#OH-DLG-1
#[tokio::test]
async fn edge_a_listed_command_starts_by_either_name() {
    let (f, mut a, _) = background().await;
    a.plugin_name = Some("supplier-analyst".to_owned());
    a.commands = vec![DirectoryCommand {
        name: "compare-quotes".to_owned(),
        session_name: None,
        argument_hint: None,
        ..Default::default()
    }];
    f.inspector.agents.lock().unwrap()[0] = a.clone();
    f.open().await;
    for command in ["supplier-analyst:compare-quotes", "compare-quotes"] {
        let out = f
            .tool(
                "delegate",
                json!({ "agent": key(&a), "brief": "Compare", "command": command }),
            )
            .await
            .unwrap_or_else(|e| panic!("{command}: {e:?}"));
        assert_eq!(out.text, "Started: Supplier analyst is working on it.");
    }
    assert_eq!(task_count(&f), 2);
}

/// The task run Ordin just delegated, as the Supervisor keeps it once it has ended `failed`.
fn ended_run(f: &Fixture, a: &DirectoryAgent, started: bool, end_reason: &str) -> RunId {
    let run_id = *f.supervisor.task_runs.lock().unwrap().last().unwrap();
    let mut run = json!({
        "id": run_id,
        "origin": { "kind": "task", "task_id": ordin_types::TaskId::new() },
        "agent": { "kind": "agent_instance", "agent_instance_id": a.agent_instance_id },
        "adapter": "claude_local",
        "state": "failed",
        "limits": { "wall_clock_secs": 3600, "idle_timeout_secs": 600 },
        "usage": { "model_turns": 0, "tool_calls": 0 },
        "inputs_used": [],
        "queued_at": Timestamp::now(),
        "ended_at": Timestamp::now(),
        "end_reason": end_reason,
    });
    if started {
        run["started_at"] = json!(Timestamp::now());
    }
    let run: ordin_types::domain::runs::Run = serde_json::from_value(run).unwrap();
    f.supervisor
        .stored
        .lock()
        .unwrap()
        .insert(run_id, (run, f.ws, None));
    run_id
}

/// Waits for a turn sent to Ordin that contains `want`; the turns sent so far.
async fn poll_turns(f: &Fixture, want: &str) -> Vec<String> {
    let mut turns = Vec::new();
    for _ in 0..100 {
        turns = f
            .supervisor
            .turns
            .lock()
            .unwrap()
            .iter()
            .map(|t| t.1.clone())
            .collect();
        if turns.iter().any(|t| t.contains(want)) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    turns
}

fn failed_end() -> ordin_runtime_adapter::RunEnd {
    ordin_runtime_adapter::RunEnd {
        reason: ordin_runtime_adapter::RunEndReason::Failed,
        final_message: None,
        artefacts: vec![],
    }
}

// Scenario: Error path — a task that could not start is told to Ordin (OH-DLG-2).
// spec: ordin-desktop/components/OrdinHost#OH-DLG-2
#[tokio::test]
async fn error_a_task_that_could_not_start_is_told_to_ordin() {
    let (f, a, _) = background().await;
    f.open().await;
    f.tool(
        "delegate",
        json!({ "agent": key(&a), "brief": "Summarise the PDF" }),
    )
    .await
    .unwrap();
    let run = ended_run(&f, &a, false, "couldn't start the agent");
    f.journal
        .run_codes
        .lock()
        .unwrap()
        .insert(run, "RA_COMMAND_UNKNOWN".to_owned());
    f.supervisor.end_with(run, failed_end());
    let want = "Supplier analyst couldn't start: couldn't start the agent.";
    let rows = poll_rows(&f, "SELECT text FROM ordin_messages", |t| t == want).await;
    assert!(rows.iter().any(|t| t == want), "{rows:?}");
    let told = format!(
        "The task for Supplier analyst ({}) ended: couldn't start: couldn't start the agent (code RA_COMMAND_UNKNOWN). It is not running. Tell the person in one sentence what happened and what you will do next.",
        a.agent_instance_id
    );
    let turns = poll_turns(&f, &told).await;
    assert!(turns.iter().any(|t| t.contains(&told)), "{turns:?}");
}

// OH-DLG-2: a task that ran and then failed says so with its reason; with no code, none is named.
// spec: ordin-desktop/components/OrdinHost#OH-DLG-2
#[tokio::test]
async fn edge_a_task_that_stopped_after_starting_names_its_reason() {
    let (f, a, _) = background().await;
    f.open().await;
    f.tool(
        "delegate",
        json!({ "agent": key(&a), "brief": "Compare the quotes" }),
    )
    .await
    .unwrap();
    let run = ended_run(&f, &a, true, "The quotes folder could not be read.");
    f.supervisor.end_with(run, failed_end());
    let want = "Supplier analyst stopped before it finished: The quotes folder could not be read.";
    let rows = poll_rows(&f, "SELECT text FROM ordin_messages", |t| t == want).await;
    assert!(rows.iter().any(|t| t == want), "{rows:?}");
    let told = "ended: stopped before it finished: The quotes folder could not be read. It is not running.";
    let turns = poll_turns(&f, told).await;
    assert!(turns.iter().any(|t| t.contains(told)), "{turns:?}");
}

// OH-DLG-2.3: a task the person stopped keeps OH-RC-1's sentence, and Ordin takes no turn for it.
// spec: ordin-desktop/components/OrdinHost#OH-DLG-2
#[tokio::test]
async fn edge_a_task_the_person_stopped_takes_no_turn() {
    let (f, a, _) = background().await;
    f.open().await;
    f.tool(
        "delegate",
        json!({ "agent": key(&a), "brief": "Compare the quotes" }),
    )
    .await
    .unwrap();
    let before = f.supervisor.turns.lock().unwrap().len();
    let run = *f.supervisor.task_runs.lock().unwrap().last().unwrap();
    f.supervisor.end_with(
        run,
        ordin_runtime_adapter::RunEnd {
            reason: ordin_runtime_adapter::RunEndReason::Cancelled,
            final_message: None,
            artefacts: vec![],
        },
    );
    let want = "Supplier analyst was stopped.";
    let rows = poll_rows(&f, "SELECT text FROM ordin_messages", |t| t == want).await;
    assert!(rows.iter().any(|t| t == want), "{rows:?}");
    tokio::time::sleep(Duration::from_millis(100)).await;
    let turns = f.supervisor.turns.lock().unwrap().clone();
    assert!(
        turns[before..]
            .iter()
            .all(|t| !t.1.contains("The task for")),
        "{turns:?}"
    );
}

// OH-ACC-1: found in the Phase 5 walk — the document agent could use none of the workspace's
// folders, so its writes were refused and nobody was asked. Delegating now asks once, with a tick
// box per folder, and starts nothing; asked once, the next delegation starts the task.
// spec: ordin-desktop/components/OrdinHost#OH-ACC-1
#[tokio::test]
async fn an_agent_with_no_folder_access_is_offered_access_once() {
    let (f, a, _) = background().await;
    f.open().await;
    *f.workspaces.folders.lock().unwrap() = vec![crate::ports::AgentFolder {
        place_id: ordin_types::PlaceId::new(),
        name: "Sample documents".to_owned(),
        read_only: false,
        granted: false,
    }];
    let first = f
        .tool(
            "delegate",
            json!({ "agent": key(&a), "brief": "Summarise the notes" }),
        )
        .await
        .unwrap();
    assert!(
        first
            .text
            .starts_with("Supplier analyst can't use any folder of this workspace yet."),
        "{}",
        first.text
    );
    assert!(
        f.supervisor.tasks.lock().unwrap().is_empty(),
        "no task starts"
    );
    let rows = poll_rows(&f, "SELECT body FROM ordin_blocks", |b| {
        b.contains("\"access\"")
    })
    .await;
    let access = rows
        .iter()
        .find(|b| b.contains("\"access\""))
        .expect("an access card");
    assert!(access.contains("Sample documents"), "{access}");
    let second = f
        .tool(
            "delegate",
            json!({ "agent": key(&a), "brief": "Summarise the notes" }),
        )
        .await
        .unwrap();
    assert_eq!(second.text, "Started: Supplier analyst is working on it.");
    assert_eq!(f.supervisor.tasks.lock().unwrap().len(), 1);
}

// OH-ACC-1: an agent that may use at least one folder is not asked.
// spec: ordin-desktop/components/OrdinHost#OH-ACC-1
#[tokio::test]
async fn an_agent_with_some_folder_access_is_not_asked() {
    let (f, a, _) = background().await;
    f.open().await;
    *f.workspaces.folders.lock().unwrap() = vec![crate::ports::AgentFolder {
        place_id: ordin_types::PlaceId::new(),
        name: "Sample documents".to_owned(),
        read_only: false,
        granted: true,
    }];
    let out = f
        .tool(
            "delegate",
            json!({ "agent": key(&a), "brief": "Summarise the notes" }),
        )
        .await
        .unwrap();
    assert_eq!(out.text, "Started: Supplier analyst is working on it.");
}

// OH-ATT-2: found on 0.2.1-test.11 — the Document agent was told only the place and file name of
// an attached PDF and could not find it. The files the person attached in a folder the agent may
// read now reach its run as attachments; a file in a folder it may not read does not.
// spec: ordin-desktop/components/OrdinHost#OH-ATT-2
#[tokio::test]
async fn the_persons_attached_files_reach_the_agent_in_folders_it_may_read() {
    let (f, a, _) = background().await;
    f.open().await;
    let allowed = ordin_types::PlaceId::new();
    let other = ordin_types::PlaceId::new();
    // `other` is no folder the agent holds: its file is left out, and nobody is asked about it.
    *f.workspaces.folders.lock().unwrap() = vec![crate::ports::AgentFolder {
        place_id: allowed,
        name: "pdf".to_owned(),
        read_only: false,
        granted: true,
    }];
    f.host
        .send_message(
            f.ws,
            "List the facts in these".to_owned(),
            vec![
                ordin_types::wire::ContentRef::PlaceFile {
                    place_id: allowed,
                    relative_path: "Agent-Platform.pdf".to_owned(),
                },
                ordin_types::wire::ContentRef::PlaceFile {
                    place_id: other,
                    relative_path: "secret.pdf".to_owned(),
                },
            ],
        )
        .await
        .unwrap();
    let out = f
        .tool(
            "delegate",
            json!({ "agent": key(&a), "brief": "List the facts in the attached PDF" }),
        )
        .await
        .unwrap();
    assert_eq!(out.text, "Started: Supplier analyst is working on it.");
    let recorded = f.journal.artefacts.lock().unwrap().clone();
    assert_eq!(recorded.len(), 1, "{recorded:?}");
    assert_eq!(recorded[0].path, "Agent-Platform.pdf");
    assert_eq!(
        recorded[0].location,
        ordin_types::domain::places::ArtefactLocation::Place { place_id: allowed }
    );
    let tasks = f.supervisor.tasks.lock().unwrap().clone();
    assert_eq!(tasks.last().unwrap().0.attached_artefacts.len(), 1);
}

// OH-RST-1: found on the 0.2.1-test.12 Mac build — after an update the person said "try again" and
// Ordin answered it had nothing to try again: the new run was seeded with the conversation's opening
// summary only, so the failure before the update was not in front of it.
// spec: ordin-desktop/components/OrdinHost#OH-RST-1
#[tokio::test]
async fn after_a_restart_ordin_sees_the_conversation_so_far() {
    let (f, _, _) = background().await;
    let run = f.open().await;
    f.supervisor
        .respond(reply_with("I could not add notes.pdf to memory (MEM_NOT_READY)."));
    f.host
        .send_message(f.ws, "Add notes.pdf to memory".to_owned(), vec![])
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    f.supervisor.end(run);
    f.supervisor.forgotten.lock().unwrap().push(run);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    f.host
        .send_message(f.ws, "try again".to_owned(), vec![])
        .await
        .unwrap();
    let seed = f.supervisor.seeds().last().cloned().unwrap_or_default();
    let at = seed
        .find("## This conversation so far")
        .unwrap_or_else(|| panic!("no history in the seed: {seed}"));
    let history = &seed[at..];
    assert!(history.contains("Person: Add notes.pdf to memory"), "{history}");
    assert!(history.contains("MEM_NOT_READY"), "{history}");
    assert!(history.contains("try something again"));
}

// OH-BRK-1: found on Windows — after a break the summary ran inside `ordin.send_message`, and a slow
// model made the person's message fail with "That took too long". The message is accepted at once
// even when the summary turn has not been answered.
// spec: ordin-desktop/components/OrdinHost#OH-BRK-1
#[tokio::test]
async fn a_message_after_a_break_is_accepted_before_the_summary_is_written() {
    let (f, _, _) = background().await;
    f.open().await;
    *f.usage.continue_from_summary.lock().unwrap() = true;
    // The summary turn is never answered.
    f.supervisor.respond(Arc::new(|_, text| {
        Box::pin(async move {
            if text.contains("Write a summary") {
                None
            } else {
                Some("Ready.".to_owned())
            }
        })
    }));
    let sent = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        f.host
            .send_message(f.ws, "where were we".to_owned(), vec![]),
    )
    .await
    .expect("send_message answers without waiting for the summary")
    .unwrap();
    assert!(sent.accepted);
}

// OH-WC-1: found in the Phase 5 walk — `propose_push` takes working-copy item ids, and nothing gave
// Ordin one, so it could not offer to send an agent's change back.
// spec: ordin-desktop/components/OrdinHost#OH-WC-1
#[tokio::test]
async fn a_changed_linked_document_reaches_ordin_with_its_item_once() {
    let f = fixture_with(config(), policy()).await;
    let run = f.open().await;
    let item = ordin_types::WorkingCopyItemId(uuid::Uuid::now_v7());
    f.workspaces
        .changed
        .lock()
        .unwrap()
        .push(crate::ports::ChangedDocument {
            item,
            path: "Rates".to_owned(),
            link: "Procurement".to_owned(),
        });
    let line =
        format!("Linked documents changed, not sent back (propose_push takes the item):\n- Rates in Procurement (item {item})");
    for text in ["Is it done?", "Thanks"] {
        f.host
            .send_message(f.ws, text.to_owned(), vec![])
            .await
            .unwrap();
        let sup = f.supervisor.clone();
        assert!(
            eventually(|| sup
                .turns
                .lock()
                .unwrap()
                .iter()
                .any(|(r, t)| *r == run && t.ends_with(text)))
            .await
        );
    }
    let turns: Vec<String> = f
        .supervisor
        .turns
        .lock()
        .unwrap()
        .iter()
        .filter(|(r, _)| *r == run)
        .map(|(_, t)| t.clone())
        .collect();
    let first = turns.iter().find(|t| t.ends_with("Is it done?")).unwrap();
    let second = turns.iter().find(|t| t.ends_with("Thanks")).unwrap();
    assert!(first.contains(&line), "{first}");
    assert!(!second.contains("Linked documents changed"), "{second}");
}

// OH-WC-1: found in the Phase 5 walk on Windows — a change in a repository folder on this computer
// was never named, though the Push Service sends it back itself (PushService.md §C step 4, Git). A
// repository linked without a clone goes by pull request, a Jira issue is read only and a local folder
// is its own origin, so those stay out.
// spec: ordin-desktop/components/OrdinHost#OH-WC-1
#[test]
fn a_repository_folder_on_this_computer_is_named_with_online_documents() {
    use crate::ports::sent_back_directly;
    use ordin_types::domain::places::SourceLinkKind as K;
    for kind in [
        K::SharepointLocation,
        K::ConfluenceLocation,
        K::GitRepositoryFolder,
    ] {
        assert!(sent_back_directly(kind), "{kind:?} is named");
    }
    for kind in [K::LocalFolder, K::GitRemote, K::JiraIssue] {
        assert!(!sent_back_directly(kind), "{kind:?} is not named");
    }
}

// OH-WC-1: a read that fails names nothing and the turn goes on.
#[tokio::test]
async fn edge_case_a_failed_read_of_changed_documents_names_none() {
    let f = fixture_with(config(), policy()).await;
    let run = f.open().await;
    f.workspaces
        .f
        .always("workspaces.changed_documents", "WS_STORE_FAILED", false);
    f.host
        .send_message(f.ws, "Hello".to_owned(), vec![])
        .await
        .unwrap();
    let sup = f.supervisor.clone();
    assert!(
        eventually(|| sup
            .turns
            .lock()
            .unwrap()
            .iter()
            .any(|(r, t)| *r == run && t.ends_with("Hello") && !t.contains("Linked documents")))
        .await
    );
}

// OH-PF-1: found in the Phase 5 walk — a send-back that failed showed only "Decided" on its card, and
// nothing offered Try again.
// spec: ordin-desktop/components/OrdinHost#OH-PF-1
#[tokio::test]
async fn a_failed_send_back_is_shown_with_try_again() {
    let f = fixture_with(config(), policy()).await;
    f.open().await;
    let push = ordin_types::PendingPushId::new();
    let item = ordin_types::WorkingCopyItemId(uuid::Uuid::now_v7());
    f.push
        .notices
        .send(ordin_push::PushNotice::PushFinished {
            pending_push_id: push,
            workspace_id: f.ws,
            pushed: vec![],
            failed: vec![(item, "PUSH_ORIGIN_UNREACHABLE")],
            origin_changed: vec![],
        })
        .unwrap();
    let fc = &f;
    assert!(eventually(|| !waiting_in(fc, "question").is_empty()).await);
    let asked = waiting_in(&f, "question");
    assert_eq!(
        asked[0].title,
        "Couldn't send 1 change back: the service could not be reached."
    );
    f.host
        .answer_block(
            f.ws,
            asked[0].block_id,
            BlockAnswer::Choice {
                choice: "Try again".to_owned(),
            },
        )
        .await
        .unwrap();
    assert_eq!(*f.push.tried_again.lock().unwrap(), vec![(push, item)]);
}

// OH-PF-1: an item that needs a new approval, or whose outcome is unknown, is told, never tried again.
#[tokio::test]
async fn edge_case_a_send_back_that_needs_a_new_approval_offers_no_try_again() {
    let f = fixture_with(config(), policy()).await;
    f.open().await;
    let item = ordin_types::WorkingCopyItemId(uuid::Uuid::now_v7());
    f.push
        .notices
        .send(ordin_push::PushNotice::PushFinished {
            pending_push_id: ordin_types::PendingPushId::new(),
            workspace_id: f.ws,
            pushed: vec![],
            failed: vec![(item, "PUSH_APPROVED_VERSION_CHANGED")],
            origin_changed: vec![],
        })
        .unwrap();
    let fc = &f;
    assert!(
        eventually(|| events(fc, "block_shown")
            .iter()
            .any(|s| s.contains("Couldn't send 1 change back: it changed after you approved it")))
        .await
    );
    assert!(waiting_in(&f, "question").is_empty());
    assert!(f.push.tried_again.lock().unwrap().is_empty());
}

// OH-PF-1: Not now sends nothing.
#[tokio::test]
async fn edge_case_not_now_sends_nothing_again() {
    let f = fixture_with(config(), policy()).await;
    f.open().await;
    f.push
        .notices
        .send(ordin_push::PushNotice::PushFinished {
            pending_push_id: ordin_types::PendingPushId::new(),
            workspace_id: f.ws,
            pushed: vec![],
            failed: vec![(
                ordin_types::WorkingCopyItemId(uuid::Uuid::now_v7()),
                "PUSH_FORBIDDEN",
            )],
            origin_changed: vec![],
        })
        .unwrap();
    let fc = &f;
    assert!(eventually(|| !waiting_in(fc, "question").is_empty()).await);
    let asked = waiting_in(&f, "question");
    f.host
        .answer_block(
            f.ws,
            asked[0].block_id,
            BlockAnswer::Choice {
                choice: "Not now".to_owned(),
            },
        )
        .await
        .unwrap();
    assert!(f.push.tried_again.lock().unwrap().is_empty());
    assert!(waiting_in(&f, "question").is_empty());
}

/// OH-TA-1: a task run of `agent` in `ws` that ended in `state`, as the Supervisor stand-in keeps it.
fn stopped_task(
    f: &Fixture,
    ws: ordin_types::WorkspaceId,
    agent: &DirectoryAgent,
    state: &str,
) -> (RunId, ordin_types::domain::runs::Task) {
    let task = ordin_types::domain::runs::Task {
        id: ordin_types::TaskId::new(),
        workspace_id: ws,
        target: TaskTarget::AgentInstance {
            agent_instance_id: agent.agent_instance_id,
        },
        command: Some("compare-quotes".to_owned()),
        free_text: None,
        arguments: vec![ordin_types::domain::runs::TaskArgument {
            name: "arguments".to_owned(),
            value: "Kestrel and Acme".to_owned(),
        }],
        attached_artefacts: vec![],
        run_again_of: None,
        requested_at: Timestamp::now(),
    };
    let run_id = RunId::new();
    let run: ordin_types::domain::runs::Run = serde_json::from_value(json!({
        "id": run_id,
        "origin": { "kind": "task", "task_id": task.id },
        "agent": { "kind": "agent_instance", "agent_instance_id": agent.agent_instance_id },
        "adapter": "claude_local",
        "state": state,
        "limits": { "wall_clock_secs": 3600, "idle_timeout_secs": 600 },
        "usage": { "model_turns": 3, "tool_calls": 1 },
        "inputs_used": [],
        "queued_at": Timestamp::now(),
    }))
    .unwrap();
    f.supervisor
        .stored
        .lock()
        .unwrap()
        .insert(run_id, (run, ws, Some(task.clone())));
    (run_id, task)
}

// OH-TA-1: found in the Phase 5 walk on Windows — a run that ended interrupted offered no Try again.
// The task runs again for the same agent as a new task that names the old one, never answered from
// an earlier run, and started by the person.
// spec: ordin-desktop/components/OrdinHost#OH-TA-1
#[tokio::test]
async fn happy_path_try_again_runs_an_interrupted_task_again() {
    let (f, a, _) = background().await;
    let now = Timestamp::now();
    *f.usage.reusable.lock().unwrap() = Some(ReusedAnswer {
        id: ReusedAnswerId::new(),
        request: "compare-quotes".to_owned(),
        agent: ordin_types::common::AgentRef::AgentInstance {
            agent_instance_id: a.agent_instance_id,
        },
        workspace_id: f.ws,
        input_versions: vec![],
        source_run_id: RunId::new(),
        text: "Kestrel is cheapest.".to_owned(),
        answered_at: now,
        shown_again_at: now,
        expires_at: now,
    });
    f.open().await;
    let (run_id, old) = stopped_task(&f, f.ws, &a, "interrupted");
    let outcome = f.host.try_again_run(f.ws, run_id).await.unwrap();
    let crate::TryAgainOutcome::Started { run_id: new_run } = outcome else {
        panic!("{outcome:?}");
    };
    assert_ne!(new_run, run_id);
    let tasks = f.supervisor.tasks.lock().unwrap().clone();
    assert_eq!(tasks.len(), 1, "the earlier answer is not reused");
    assert_eq!(tasks[0].0.run_again_of, Some(old.id));
    assert_ne!(tasks[0].0.id, old.id);
    assert_eq!(tasks[0].0.command.as_deref(), Some("compare-quotes"));
    assert_eq!(tasks[0].0.arguments, old.arguments);
    assert_eq!(tasks[0].0.target, old.target);
    assert!(tasks[0].2, "started by the person");
    assert!(f.usage.reused.lock().unwrap().is_empty());
}

// OH-TA-1: a failed run is tried again as an interrupted one is.
// spec: ordin-desktop/components/OrdinHost#OH-TA-1
#[tokio::test]
async fn happy_path_try_again_runs_a_failed_task_again() {
    let (f, a, _) = background().await;
    f.open().await;
    let (run_id, _) = stopped_task(&f, f.ws, &a, "failed");
    let outcome = f.host.try_again_run(f.ws, run_id).await.unwrap();
    assert!(matches!(outcome, crate::TryAgainOutcome::Started { .. }));
    assert_eq!(task_count(&f), 1);
}

// OH-TA-1: only an interrupted or failed run is tried again.
// spec: ordin-desktop/components/OrdinHost#OH-TA-1
#[tokio::test]
async fn edge_case_try_again_refuses_a_completed_run() {
    let (f, a, _) = background().await;
    f.open().await;
    let (run_id, _) = stopped_task(&f, f.ws, &a, "completed");
    let e = f.host.try_again_run(f.ws, run_id).await.unwrap_err();
    assert_eq!(e.code(), "OH_RUN_NOT_INTERRUPTED");
    assert_eq!(task_count(&f), 0);
}

// OH-TA-1: a run of another workspace is answered as an unknown run, as is a run nobody knows.
// spec: ordin-desktop/components/OrdinHost#OH-TA-1
#[tokio::test]
async fn edge_case_try_again_refuses_another_workspace_s_run() {
    let (f, a, _) = background().await;
    f.open().await;
    let (run_id, _) = stopped_task(&f, ordin_types::WorkspaceId::new(), &a, "interrupted");
    let e = f.host.try_again_run(f.ws, run_id).await.unwrap_err();
    assert_eq!(e.code(), "OH_RUN_UNKNOWN");
    let e = f.host.try_again_run(f.ws, RunId::new()).await.unwrap_err();
    assert_eq!(e.code(), "OH_RUN_UNKNOWN");
    assert_eq!(task_count(&f), 0);
}

// OH-TA-1: a coding task's run, or one whose agent left the workspace, is not started again here.
// spec: ordin-desktop/components/OrdinHost#OH-TA-1
#[tokio::test]
async fn edge_case_try_again_refuses_a_coding_task_and_a_departed_agent() {
    let (f, a, b) = background().await;
    f.open().await;
    let (coding_run, coding) = stopped_task(&f, f.ws, &a, "interrupted");
    f.projects
        .coding
        .lock()
        .unwrap()
        .push(ordin_projects::StartCodingTask {
            project_id: ProjectId::new(),
            origin: ordin_types::domain::RunOrigin::Task { task_id: coding.id },
            agent_instance_id: a.agent_instance_id,
            request: "Add retry".to_owned(),
            task_name: "retry".to_owned(),
            attempt_group_id: None,
        });
    let e = f.host.try_again_run(f.ws, coding_run).await.unwrap_err();
    assert_eq!(e.code(), "OH_RUN_NOT_REPEATABLE");
    let (gone_run, _) = stopped_task(&f, f.ws, &b, "failed");
    f.inspector
        .agents
        .lock()
        .unwrap()
        .retain(|x| x.agent_instance_id != b.agent_instance_id);
    let e = f.host.try_again_run(f.ws, gone_run).await.unwrap_err();
    assert_eq!(e.code(), "OH_RUN_NOT_REPEATABLE");
    assert_eq!(task_count(&f), 0);
}

/// OH-CLS-1: a task run of the fixture's workspace that a restart ended `interrupted` just now, as
/// the Supervisor's `list` and `get` answer it.
fn interrupted_by_restart(f: &Fixture) -> RunId {
    let run_id = RunId::new();
    let run: ordin_types::domain::runs::Run = serde_json::from_value(json!({
        "id": run_id,
        "origin": { "kind": "task", "task_id": ordin_types::TaskId::new() },
        "agent": { "kind": "agent_instance", "agent_instance_id": ordin_types::AgentInstanceId::new() },
        "adapter": "claude_local",
        "state": "interrupted",
        "limits": { "wall_clock_secs": 3600, "idle_timeout_secs": 600 },
        "usage": { "model_turns": 3, "tool_calls": 1 },
        "inputs_used": [],
        "queued_at": Timestamp::now(),
        "ended_at": Timestamp::now(),
    }))
    .unwrap();
    f.supervisor.listed.lock().unwrap().push(run.clone());
    f.supervisor
        .stored
        .lock()
        .unwrap()
        .insert(run_id, (run, f.ws, None));
    run_id
}

/// OH-CLS-1: the messages of the fixture's conversations that tell what happened while ordin was closed.
async fn closed_messages(f: &Fixture) -> Vec<String> {
    sqlx::query_scalar::<_, String>("SELECT text FROM ordin_messages ORDER BY at, seq")
        .fetch_all(&f.pool)
        .await
        .unwrap()
        .into_iter()
        .filter(|t| t.starts_with("While ordin was closed"))
        .collect()
}

// OH-CLS-1: found in the Phase 5 walk on Windows — two runs were interrupted by a restart and nothing
// told the person. The first open after start shows one message, and Ordin takes no turn for it.
// spec: ordin-desktop/components/OrdinHost#OH-CLS-1
#[tokio::test]
async fn happy_path_an_interrupted_task_and_a_paused_crew_are_told_once() {
    let f = fixture().await;
    interrupted_by_restart(&f);
    *f.orchestrator.restart_paused.lock().unwrap() = 1;
    f.open().await;
    assert_eq!(
        closed_messages(&f).await,
        vec!["While ordin was closed: 1 task was interrupted and 1 crew is paused.".to_owned()]
    );
    assert!(f.supervisor.turn_texts().is_empty(), "no model turn");
}

// OH-CLS-1: found in the Phase 5 walk on Windows — an interrupted task's run screen, and so its Try
// again, could not be reached after a restart. Each interrupted task gets a progress card after the
// summary; the card leads to the run's screen (ScreensModule.md SM-RS-1).
// spec: ordin-desktop/components/OrdinHost#OH-CLS-1
#[tokio::test]
async fn happy_path_each_interrupted_task_gets_a_card_that_opens_its_run() {
    let f = fixture().await;
    let first = interrupted_by_restart(&f);
    let second = interrupted_by_restart(&f);
    f.open().await;
    let rows: Vec<Value> =
        sqlx::query_scalar::<_, String>("SELECT body FROM ordin_blocks ORDER BY shown_at, rowid")
            .fetch_all(&f.pool)
            .await
            .unwrap()
            .iter()
            .map(|b| serde_json::from_str(b).unwrap())
            .collect();
    let summary = rows
        .iter()
        .position(|b| b["text"] == "While ordin was closed: 2 tasks were interrupted.")
        .unwrap_or_else(|| panic!("no summary: {rows:?}"));
    let cards: Vec<&Value> = rows[summary + 1..]
        .iter()
        .filter(|b| b["kind"] == "plan_progress")
        .collect();
    assert_eq!(cards.len(), 2, "{rows:?}");
    for (card, run) in cards.iter().zip([first, second]) {
        assert_eq!(card["subject"]["kind"], "run");
        assert_eq!(card["subject"]["run_id"], json!(run));
    }
    assert!(f.supervisor.turn_texts().is_empty(), "no model turn");
}

// OH-CLS-1: found in the Phase 5 walk on Windows — after a restart the screen only re-reads the
// conversation, so the summary waited for the person's next message. Reading it tells, once.
// spec: ordin-desktop/components/OrdinHost#OH-CLS-1
#[tokio::test]
async fn happy_path_reading_the_conversation_after_a_restart_tells_once() {
    let f = fixture().await;
    interrupted_by_restart(&f);
    let conv = f.host.ensure_conversation(f.ws).await.unwrap();
    let page = f.host.read_conversation(conv, None).await.unwrap();
    assert!(
        page.items
            .iter()
            .any(|m| m.text == "While ordin was closed: 1 task was interrupted."),
        "the first read carries the summary"
    );
    f.host.read_conversation(conv, None).await.unwrap();
    f.host.open_conversation(f.ws).await.unwrap();
    assert_eq!(closed_messages(&f).await.len(), 1);
}

// OH-CLS-1: a send whose outcome is unknown, from a run the restart ended, is named too.
// spec: ordin-desktop/components/OrdinHost#OH-CLS-1
#[tokio::test]
async fn happy_path_a_send_left_unsure_is_named() {
    let f = fixture().await;
    let run_id = interrupted_by_restart(&f);
    *f.journal.unknown_outcome.lock().unwrap() = vec![run_id, run_id, RunId::new()];
    f.open().await;
    assert_eq!(
        closed_messages(&f).await,
        vec![
            "While ordin was closed: 1 task was interrupted and I'm not sure 2 messages were sent."
                .to_owned()
        ]
    );
}

// OH-CLS-1: nothing to tell shows nothing.
// spec: ordin-desktop/components/OrdinHost#OH-CLS-1
#[tokio::test]
async fn edge_nothing_to_tell_after_a_restart_shows_nothing() {
    let f = fixture().await;
    f.open().await;
    assert!(closed_messages(&f).await.is_empty());
}

// OH-CLS-1: reopening the conversation and a later message show nothing new.
// spec: ordin-desktop/components/OrdinHost#OH-CLS-1
#[tokio::test]
async fn edge_what_happened_while_closed_is_told_once_only() {
    let f = fixture().await;
    interrupted_by_restart(&f);
    f.open().await;
    f.host.open_conversation(f.ws).await.unwrap();
    f.host
        .send_message(f.ws, "Hello".to_owned(), vec![])
        .await
        .unwrap();
    assert_eq!(
        closed_messages(&f).await,
        vec!["While ordin was closed: 1 task was interrupted.".to_owned()]
    );
    assert_eq!(f.f.count("orchestrator.paused_for_restart"), 1, "read once");
}

// OH-CLS-1: a failed read shows nothing and the conversation opens.
// spec: ordin-desktop/components/OrdinHost#OH-CLS-1
#[tokio::test]
async fn error_a_failed_read_of_what_happened_while_closed_shows_nothing() {
    let f = fixture().await;
    interrupted_by_restart(&f);
    f.f.always("supervisor.list", "SUP_STORE_FAILED", true);
    f.open().await;
    assert!(closed_messages(&f).await.is_empty());
    assert!(!log_lines("ordin_host.closed_summary_not_read").is_empty());
}

// OH-CLS-1: the message forms — plurals, and three parts joined with commas and "and".
// spec: ordin-desktop/components/OrdinHost#OH-CLS-1
#[test]
fn the_closed_summary_names_each_count_in_its_form() {
    use crate::closed::{closed_text, Closed};
    assert_eq!(closed_text(Closed::default()), None);
    assert_eq!(
        closed_text(Closed {
            interrupted: 2,
            paused_crews: 1,
            unsure: 1,
        })
        .as_deref(),
        Some("While ordin was closed: 2 tasks were interrupted, 1 crew is paused, and I'm not sure 1 message was sent.")
    );
    assert_eq!(
        closed_text(Closed {
            interrupted: 0,
            paused_crews: 2,
            unsure: 0,
        })
        .as_deref(),
        Some("While ordin was closed: 2 crews are paused.")
    );
}

// ---------------------------------------------------------------- OH-DIS: dismissing what waits

/// Ordin's question "Which supplier?" with no Not now of its own, waiting.
async fn ordins_question(f: &Fixture) -> ordin_types::BlockId {
    f.open().await;
    f.tool(
        "show",
        json!({ "block": { "kind": "question", "question": "Which supplier?", "choices": ["Kestrel", "Acme"], "allow_other": false } }),
    )
    .await
    .unwrap();
    let asked = waiting_in(f, "question");
    assert_eq!(asked.len(), 1);
    asked[0].block_id
}

fn turns_ending(f: &Fixture, end: &str) -> usize {
    f.supervisor
        .turn_texts()
        .iter()
        .filter(|t| t.contains(end))
        .count()
}

// Scenario: Happy path — Skip on a question leaves Needs you and Ordin carries on (OH-DIS-1)
// spec: ordin-desktop/components/OrdinHost#OH-DIS-1
#[tokio::test]
async fn happy_path_skip_on_a_question_leaves_needs_you_and_ordin_carries_on() {
    let (f, _, _) = background().await;
    let block = ordins_question(&f).await;
    let changed = events(&f, "needs_you_changed").len();
    f.host
        .answer_block(f.ws, block, BlockAnswer::Dismissed)
        .await
        .unwrap();
    assert!(waiting_in(&f, "question").is_empty(), "it leaves Needs you");
    assert_eq!(events(&f, "needs_you_changed").len(), changed + 1);
    assert_eq!(
        f.count("SELECT COUNT(*) FROM ordin_messages WHERE author = 'person' AND text = 'Skipped'")
            .await,
        1
    );
    let sup = f.supervisor.clone();
    assert!(
        eventually(|| sup.turn_texts().iter().any(|t| t.contains(
            "The person skipped the question \"Which supplier?\", so it has no answer. Carry on without it, and say in one sentence what happens next."
        )))
        .await,
        "{:?}",
        f.supervisor.turn_texts()
    );
    let row = {
        let mut c = f.pool.acquire().await.unwrap();
        crate::store::block(&mut c, block).await.unwrap().unwrap()
    };
    assert!(!row.waiting);
}

// Scenario: Error — a dismissed block cannot be dismissed or answered again (OH-DIS-1)
#[tokio::test]
async fn error_a_dismissed_block_is_already_answered() {
    let (f, _, _) = background().await;
    let block = ordins_question(&f).await;
    f.host
        .answer_block(f.ws, block, BlockAnswer::Dismissed)
        .await
        .unwrap();
    let again = f
        .host
        .answer_block(f.ws, block, BlockAnswer::Dismissed)
        .await
        .unwrap_err();
    assert_eq!(again.code(), "OH_BLOCK_ANSWERED");
    let answer = f
        .host
        .answer_block(
            f.ws,
            block,
            BlockAnswer::Choice {
                choice: "Kestrel".to_owned(),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(answer.code(), "OH_BLOCK_ANSWERED");
    let unknown = f
        .host
        .answer_block(f.ws, ordin_types::BlockId::new(), BlockAnswer::Dismissed)
        .await
        .unwrap_err();
    assert_eq!(unknown.code(), "OH_BLOCK_UNKNOWN");
}

// Scenario: Edge — a dismissed block does not come back after a restart (OH-DIS-1)
#[tokio::test]
async fn edge_a_dismissed_block_does_not_come_back_after_a_restart() {
    let (f, _, _) = background().await;
    let block = ordins_question(&f).await;
    f.host
        .answer_block(f.ws, block, BlockAnswer::Dismissed)
        .await
        .unwrap();
    // A second host over the same datastore, as after quitting and opening ordin again.
    let again = OrdinHost::with_ports(
        config(),
        background_policy(),
        Fakes::new().ports,
        f.pool.clone(),
    );
    let stop = tokio_util::sync::CancellationToken::new();
    again.start(stop.clone()).await.unwrap();
    assert!(
        again.waiting_blocks().iter().all(|w| w.block_id != block),
        "the dismissed question is not in Needs you"
    );
    stop.cancel();
    again.background.cancel();
}

// Scenario: Happy path — Dismiss on a form Ordin opened starts nothing and Ordin is told (OH-DIS-2)
// spec: ordin-desktop/components/OrdinHost#OH-DIS-2
#[tokio::test]
async fn happy_path_dismiss_on_a_form_starts_nothing_and_ordin_is_told() {
    let (f, a, _) = background().await;
    let names = ["Go", "Java", "Python", "Rust", "TypeScript"];
    *f.catalogue.pending.lock().unwrap() = vec![pack_question_of(&a, &names)];
    f.open().await;
    f.tool("delegate", json!({ "agent": key(&a), "brief": "Build it" }))
        .await
        .unwrap();
    let asked = waiting_in(&f, "form");
    assert_eq!(asked.len(), 1);
    f.host
        .answer_block(f.ws, asked[0].block_id, BlockAnswer::Dismissed)
        .await
        .unwrap();
    assert!(waiting_in(&f, "form").is_empty(), "it leaves Needs you");
    assert!(
        f.catalogue.answers.lock().unwrap().is_empty(),
        "nothing recorded"
    );
    assert_eq!(task_count(&f), 0, "nothing starts");
    assert_eq!(
        f.count(
            "SELECT COUNT(*) FROM ordin_messages WHERE author = 'person' AND text = 'Dismissed'"
        )
        .await,
        1
    );
    let sup = f.supervisor.clone();
    assert!(
        eventually(|| sup.turn_texts().iter().any(|t| t.contains("The person dismissed ")
            && t.contains(": nothing was sent and nothing started. Say in one sentence what happens next.")))
        .await,
        "{:?}",
        f.supervisor.turn_texts()
    );
}

// Scenario: Happy path — Not now on a crew plan starts no crew and Ordin is told (OH-DIS-4)
// spec: ordin-desktop/components/OrdinHost#OH-DIS-4
#[tokio::test]
async fn happy_path_not_now_on_a_crew_plan_starts_no_crew() {
    let (f, a, b) = background().await;
    f.open().await;
    f.tool(
        "create_crew",
        json!({
            "purpose": "Compare quotes",
            "members": [ { "agent": key(&a), "role": "analyst" }, { "agent": key(&b), "role": "writer" } ]
        }),
    )
    .await
    .unwrap();
    let plans = waiting_in(&f, "crew_plan");
    assert_eq!(plans.len(), 1);
    f.host
        .answer_block(f.ws, plans[0].block_id, BlockAnswer::Dismissed)
        .await
        .unwrap();
    assert!(
        waiting_in(&f, "crew_plan").is_empty(),
        "it leaves Needs you"
    );
    assert!(
        f.orchestrator.started.lock().unwrap().is_empty(),
        "no crew starts"
    );
    let sup = f.supervisor.clone();
    assert!(
        eventually(|| sup.turn_texts().iter().any(
            |t| t.contains("The person chose Not now on the crew plan, so the crew did not start.")
        ))
        .await
    );
}

// Scenario: Edge — Skip on a question with its own Not now is that Not now (OH-DIS-3)
// spec: ordin-desktop/components/OrdinHost#OH-DIS-3
#[tokio::test]
async fn edge_skip_on_a_pack_question_is_its_own_not_now() {
    let (f, a, _) = background().await;
    *f.catalogue.pending.lock().unwrap() = vec![pack_question_of(&a, &["Go"])];
    f.open().await;
    f.tool("delegate", json!({ "agent": key(&a), "brief": "Build it" }))
        .await
        .unwrap();
    let asked = waiting_in(&f, "question");
    f.host
        .answer_block(f.ws, asked[0].block_id, BlockAnswer::Dismissed)
        .await
        .unwrap();
    assert!(waiting_in(&f, "question").is_empty());
    assert!(
        f.catalogue.answers.lock().unwrap().is_empty(),
        "nothing recorded"
    );
    assert_eq!(task_count(&f), 0, "nothing starts");
    assert_eq!(
        f.count("SELECT COUNT(*) FROM ordin_messages WHERE author = 'person' AND text = 'Not now'")
            .await,
        1,
        "stored as the card's own Not now"
    );
    assert_eq!(turns_ending(&f, "The person skipped"), 0);
}

// Error-table row: a dismissal of a waiting block of another kind (OH-DIS-1).
#[tokio::test]
async fn error_a_dismissal_of_a_waiting_block_of_another_kind() {
    let (f, _, _) = background().await;
    f.open().await;
    let block: ordin_service_api::blocks::Block =
        serde_json::from_value(json!({ "kind": "message", "text": "Hello.", "attachments": [] }))
            .unwrap();
    let id = f
        .host
        .show_block(
            f.ws,
            ordin_service_api::commands::ordin::MessageAuthor::Ordin,
            block,
            true,
        )
        .await
        .unwrap();
    let e = f
        .host
        .answer_block(f.ws, id, BlockAnswer::Dismissed)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_INPUT_INVALID");
}

// Error-table row: the datastore fails while the dismissal is stored (OH-DIS-1).
#[tokio::test]
async fn error_the_datastore_fails_while_a_dismissal_is_stored() {
    let (f, _, _) = background().await;
    let block = ordins_question(&f).await;
    f.pool.close().await;
    let e = f
        .host
        .answer_block(f.ws, block, BlockAnswer::Dismissed)
        .await
        .unwrap_err();
    assert_eq!(e.code(), "OH_DATASTORE_UNAVAILABLE");
    assert!(e.retryable());
    assert!(
        f.host.waiting_blocks().iter().any(|w| w.block_id == block),
        "the question still waits"
    );
}

/// OH-KD-1: a run of the fixture's workspace with this origin, as the Supervisor's `list` answers it,
/// and a report of it in the Journal, newest first.
fn report_of(f: &Fixture, origin: Value, title: &str) -> RunId {
    let run_id = RunId::new();
    let run: ordin_types::domain::runs::Run = serde_json::from_value(json!({
        "id": run_id,
        "origin": origin,
        "agent": { "kind": "agent_instance", "agent_instance_id": ordin_types::AgentInstanceId::new() },
        "adapter": "claude_local",
        "state": "completed",
        "limits": { "wall_clock_secs": 3600, "idle_timeout_secs": 600 },
        "usage": { "model_turns": 3, "tool_calls": 1 },
        "inputs_used": [{ "name": "Q3 quotes.xlsx", "location": "Docs", "version_read": "4" }],
        "queued_at": Timestamp::now(),
        "ended_at": Timestamp::now(),
    }))
    .unwrap();
    f.supervisor.listed.lock().unwrap().push(run);
    f.journal.reports.lock().unwrap().insert(
        0,
        ordin_journal::ReportSummary {
            id: ordin_types::ReportId::new(),
            run_id,
            title: title.to_owned(),
            as_of: Timestamp::now(),
        },
    );
    run_id
}

// Scenario: Happy path — Ordin keeps the conversation's latest report as a dashboard (OH-KD-1)
#[tokio::test]
async fn keep_dashboard_keeps_the_latest_report_of_the_conversation() {
    use ordin_types::domain::runs::DashboardSource;
    let (f, _, _) = background().await;
    f.open().await;
    report_of(
        &f,
        json!({ "kind": "task", "task_id": ordin_types::TaskId::new() }),
        "Older",
    );
    let latest = report_of(
        &f,
        json!({ "kind": "conversation", "workspace_id": f.ws }),
        "Supplier prices",
    );
    let out = f
        .tool("keep_dashboard", json!({ "name": " Prices " }))
        .await
        .unwrap();
    assert!(out.text.contains("Supplier prices"));
    let kept = f.journal.kept.lock().unwrap().clone();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].run_id, latest);
    assert_eq!(kept[0].name, "Prices");
    assert_eq!(kept[0].made_by, DashboardSource::Conversation);
    assert_eq!(kept[0].inputs.len(), 1);
}

// Scenario: Edge case — a report named by its title, made by a task (OH-KD-1)
#[tokio::test]
async fn keep_dashboard_keeps_the_report_it_names() {
    use ordin_types::domain::runs::DashboardSource;
    let (f, _, _) = background().await;
    f.open().await;
    let task = ordin_types::TaskId::new();
    let named = report_of(
        &f,
        json!({ "kind": "task", "task_id": task }),
        "Supplier prices",
    );
    report_of(
        &f,
        json!({ "kind": "conversation", "workspace_id": f.ws }),
        "Other",
    );
    f.tool(
        "keep_dashboard",
        json!({ "name": "Prices", "report": "supplier PRICES" }),
    )
    .await
    .unwrap();
    let kept = f.journal.kept.lock().unwrap().clone();
    assert_eq!(kept[0].run_id, named);
    assert_eq!(kept[0].made_by, DashboardSource::Task { task_id: task });
}

// Scenario: Error path — no report yet, a crew's report, an unknown title, a blank name (OH-KD-1)
#[tokio::test]
async fn keep_dashboard_refusals() {
    let (f, _, _) = background().await;
    f.open().await;
    let e = f
        .tool("keep_dashboard", json!({ "name": "Prices" }))
        .await
        .unwrap_err();
    assert_eq!(e.code, "OH_RUN_UNKNOWN");
    report_of(
        &f,
        json!({ "kind": "crew_round", "crew_run_id": ordin_types::CrewRunId::new(), "round_number": 1 }),
        "Crew report",
    );
    let e = f
        .tool("keep_dashboard", json!({ "name": "Prices" }))
        .await
        .unwrap_err();
    assert_eq!(e.code, "OH_RUN_NOT_REPEATABLE");
    let e = f
        .tool(
            "keep_dashboard",
            json!({ "name": "Prices", "report": "Nothing like it" }),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code, "OH_RUN_UNKNOWN");
    assert!(f.journal.kept.lock().unwrap().is_empty());
}

// Scenario: Happy path — refreshing a dashboard Ordin made in the conversation (OH-KD-2)
#[tokio::test]
async fn refreshing_a_conversation_dashboard_asks_ordin_again() {
    use ordin_types::domain::runs::{Dashboard, DashboardSource};
    let (f, _, _) = background().await;
    let d = ordin_types::DashboardId::new();
    f.journal.dashboards.lock().unwrap().insert(
        d,
        Dashboard {
            id: d,
            workspace_id: f.ws,
            name: "Prices".to_owned(),
            made_by: DashboardSource::Conversation,
            inputs: vec![],
            refresh_runs: vec![],
            kept_at: Timestamp::now(),
        },
    );
    let started = f.host.dashboards_refresh(f.ws, d).await.unwrap();
    assert!(matches!(
        started,
        ordin_service_api::commands::dashboards::DashboardRefreshStarted::Run { .. }
    ));
    let tasks = f.supervisor.tasks.lock().unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].0.run_again_of, None);
}
