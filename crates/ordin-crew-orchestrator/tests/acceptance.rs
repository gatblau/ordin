//! One test per Gherkin scenario of CrewOrchestrator.md, with the scenario's name, plus the
//! behaviour the scenarios leave to the Internal Logic (restart, conditions, calls, approvals).

mod support;

use std::time::Duration;

use ordin_crew_orchestrator::{
    CrewError, CrewStart, CrewToolCall, PersonQuestionKind, TakeOverReason,
};
use ordin_runtime_adapter::ResumeDecision;
use ordin_types::domain::runs::{
    Condition, CrewPauseReason, CrewRunState, EachBlock, IfBlock, NamedPath, RunState, UntilBlock,
    WorkflowStep,
};
use ordin_types::TriggerId;
use support::*;

// ---- coordinated mode

#[tokio::test]
async fn happy_path_coordinated_crew_finishes() {
    let h = Harness::new().await;
    let (a, w) = (agent(), agent());
    let mut c = h.charter(&[("analyst", a), ("writer", w)]);
    c.allowed_handoffs = vec![handoff(a, w)];
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
    let run = h.wait_ended(id).await;
    h.wait_reported(&coord).await;
    assert_eq!(run.state, CrewRunState::Finished);
    assert_eq!(run.end_reason.as_deref(), Some("Done."));
    let (_, rounds) = h.orch.get(id).await.unwrap();
    assert_eq!(rounds.len(), 3);
    assert!(rounds.iter().all(|r| r.closed_at.is_some()));
    assert!(rounds.iter().all(|r| !r.guardrail_decisions.is_empty()));
    assert_eq!(rounds[0].member_runs.len(), 1);
    assert_eq!(coord.count("crew_ended"), 1);
    // The writer's brief carries the analyst's result (a hand-off), never the conversation.
    let writer_start = h.local.starts.lock().unwrap()[1].clone();
    let ordin_runtime_adapter::StartInput::FreeText { text } = writer_start.input else {
        panic!("free text expected");
    };
    assert!(text.contains("Purpose: Compare the supplier quotes"));
    assert!(text.contains("Your part: Write the summary"));
    assert!(text.contains("Use these earlier results:"));
    assert!(text.contains("- analyst: Done."), "{text}");
    assert!(text.contains("Return a short summary and references to what you made."));
    // Requests are counted: one coordinator turn per round plus the members' turns.
    assert_eq!(run.budget.requests_used, 5);
}

#[tokio::test]
async fn refused_hand_off() {
    let h = Harness::new().await;
    let (a, w) = (agent(), agent());
    let mut c = h.charter(&[("analyst", a), ("writer", w)]);
    c.allowed_handoffs = vec![handoff(a, w)];
    let coord = TestCoordinator::scripted(vec![
        vec![CrewToolCall::Delegate {
            member: a,
            brief: "Compare".into(),
            uses_results_of: vec![w],
            uses_inputs: vec![],
        }],
        vec![finish()],
    ]);
    let id = h.start(c, coord.clone()).await;
    h.wait_ended(id).await;
    h.wait_reported(&coord).await;
    assert_eq!(h.local.start_count(), 0, "no member run starts");
    let (_, rounds) = h.orch.get(id).await.unwrap();
    assert_eq!(
        rounds[0].guardrail_decisions,
        vec!["refused: This hand-off is not allowed."]
    );
    // The next round's view shows the refusal.
    let views = coord.views.lock().unwrap();
    let second = views
        .iter()
        .find(|v| v.latest_results.len() == 1)
        .expect("a view");
    assert_eq!(second.latest_results[0].state, RunState::Failed);
    assert_eq!(
        second.latest_results[0].summary.as_deref(),
        Some("Refused: This hand-off is not allowed.")
    );
}

#[tokio::test]
async fn agent_not_in_the_crew() {
    let h = Harness::new().await;
    let a = agent();
    let c = h.charter(&[("analyst", a)]);
    let coord = TestCoordinator::scripted(vec![vec![delegate(agent(), "Do it")], vec![finish()]]);
    let id = h.start(c, coord.clone()).await;
    h.wait_ended(id).await;
    let (_, rounds) = h.orch.get(id).await.unwrap();
    assert_eq!(
        rounds[0].guardrail_decisions,
        vec!["refused: This agent is not in the crew."]
    );
    assert_eq!(h.local.start_count(), 0);
}

#[tokio::test]
async fn round_limit() {
    let h = Harness::new().await;
    let a = agent();
    let mut c = h.charter(&[("analyst", a)]);
    c.guardrails.max_rounds = 3;
    let coord = TestCoordinator::scripted(vec![
        vec![delegate(a, "One")],
        vec![delegate(a, "Two")],
        vec![delegate(a, "Three")],
        vec![delegate(a, "Four")],
    ]);
    let id = h.start(c, coord.clone()).await;
    let run = h.wait_ended(id).await;
    h.wait_reported(&coord).await;
    assert_eq!(run.state, CrewRunState::StoppedAtLimit);
    assert_eq!(run.end_reason.as_deref(), Some("Stopped after 3 rounds."));
    assert_eq!(run.round_counter, 3);
    assert_eq!(coord.count("round"), 3, "no fourth round is asked for");
    let (_, rounds) = h.orch.get(id).await.unwrap();
    assert_eq!(rounds.len(), 3, "the state is kept");
    let ended = h.entries(id, "crew_ended").await;
    assert_eq!(ended[0]["code"], "CREW_LIMIT_REACHED");
    assert_eq!(h.events("needs_you_changed").await.len(), 1);
}

#[tokio::test]
async fn same_member_fails_twice() {
    let h = Harness::new().await;
    let a = agent();
    let c = h.charter(&[("analyst", a)]);
    h.local.behave(|_, _| fails());
    let coord = TestCoordinator::scripted(vec![vec![delegate(a, "One")], vec![delegate(a, "Two")]]);
    let id = h.start(c, coord.clone()).await;
    let run = h.wait_ended(id).await;
    h.wait_reported(&coord).await;
    assert_eq!(run.state, CrewRunState::Escalated);
    assert_eq!(
        run.end_reason.as_deref(),
        Some("analyst failed twice in a row.")
    );
    let ended = h.entries(id, "crew_ended").await;
    assert_eq!(ended[0]["code"], "CREW_MEMBER_FAILED_TWICE");
}

#[tokio::test]
async fn members_question_answered_by_ordin() {
    let h = Harness::new().await;
    let a = agent();
    let c = h.charter(&[("analyst", a)]);
    h.local.behave(|_, _| asks("Which quote first?"));
    let coord = TestCoordinator::scripted(vec![vec![delegate(a, "Compare")], vec![finish()]]);
    let id = h.start(c, coord.clone()).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    assert_eq!(
        coord.member_questions.lock().unwrap().clone(),
        vec!["Which quote first?"]
    );
    let resumes = h.local.resumes.lock().unwrap().clone();
    assert_eq!(resumes.len(), 1);
    match &resumes[0].1 {
        ResumeDecision::Answer { text } => assert_eq!(text, "Use the second quote."),
        other => panic!("an answer expected, got {other:?}"),
    }
    // Two coordinator turns for the rounds and one for the answer, plus the member's none.
    assert_eq!(run.budget.requests_used, 3);
}

// spec: ordin-desktop/components/CrewOrchestrator#CO-PQ-1
#[tokio::test]
async fn a_long_member_question_is_cut_before_ordin_answers_it() {
    let h = Harness::new().await;
    let a = agent();
    let c = h.charter(&[("analyst", a)]);
    let question = format!("{}{}", "é".repeat(500), "x".repeat(1_500));
    let asked = question.clone();
    h.local.behave(move |_, _| asks(&asked));
    let coord = TestCoordinator::scripted(vec![vec![delegate(a, "Compare")], vec![finish()]]);
    let id = h.start(c, coord.clone()).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    assert_eq!(
        coord.member_questions.lock().unwrap().clone(),
        vec!["é".repeat(500)]
    );
}

#[tokio::test]
async fn external_actions_only_for_allowed_members() {
    let h = Harness::new().await;
    let (a, w) = (agent(), agent());
    let mut c = h.charter(&[("analyst", a), ("writer", w)]);
    c.guardrails.members_allowed_external_actions = vec![w];
    let coord = TestCoordinator::scripted(vec![
        vec![delegate(a, "Compare"), delegate(w, "Write")],
        vec![finish()],
    ]);
    let id = h.start(c, coord.clone()).await;
    h.wait_ended(id).await;
    let (analyst_run, writer_run) = {
        let starts = h.local.starts.lock().unwrap();
        let find = |agent: ordin_types::AgentInstanceId| {
            starts
                .iter()
                .find(|s| matches!(s.target, ordin_runtime_adapter::RunTarget::AgentInstance { agent_instance_id } if agent_instance_id == agent))
                .map(|s| s.run_id)
                .unwrap()
        };
        let AgentRefId(ai) = AgentRefId::of(a);
        let AgentRefId(wi) = AgentRefId::of(w);
        (find(ai), find(wi))
    };
    // The restriction is observable through the engine: an external tool is denied for the run
    // restricted with `external_actions_allowed: false`, and allowed for the other.
    assert!(
        external_denied(&h, a, analyst_run).await,
        "analyst is restricted"
    );
    assert!(
        !external_denied(&h, w, writer_run).await,
        "writer is allowed"
    );
}

struct AgentRefId(ordin_types::AgentInstanceId);

impl AgentRefId {
    fn of(agent: ordin_types::common::AgentRef) -> AgentRefId {
        match agent {
            ordin_types::common::AgentRef::AgentInstance { agent_instance_id } => {
                AgentRefId(agent_instance_id)
            }
            ordin_types::common::AgentRef::RemoteAgent { .. } => panic!("a local agent"),
        }
    }
}

/// Begins `run` of `agent` in the Rules Engine and asks for an external tool call; true when denied.
async fn external_denied(
    h: &Harness,
    agent: ordin_types::common::AgentRef,
    run: ordin_types::RunId,
) -> bool {
    use ordin_types::domain::agents::Authority;
    use ordin_types::wire::rules::{DecisionPoint, ToolCall, ToolOrigin, Verdict};
    let AgentRefId(instance) = AgentRefId::of(agent);
    let mut set = rule_set(Authority::SystemChanging, instance);
    set.external = vec!["post_message".to_owned()];
    h.rules.load_rule_set(set).expect("the set loads");
    h.rules
        .begin_run(ordin_rules::RunStartFacts {
            run_id: run,
            agent_instance_id: instance,
            workspace_id: h.workspace,
            crew_charter_id: None,
            command: None,
            text: String::new(),
            scope: None,
            asker: None,
            started_by: "crew".to_owned(),
            started_by_id: "crew".to_owned(),
        })
        .await
        .expect("the run begins");
    let outcome = h
        .rules
        .decide_tool_call(
            &ToolCall {
                run_id: run,
                agent_instance_id: instance,
                workspace_id: h.workspace,
                point: DecisionPoint::BeforeToolCall,
                tool: "post_message".to_owned(),
                origin: ToolOrigin::BuiltIn,
                arguments: serde_json::json!({}),
            },
            &ordin_rules::CallContext::default(),
        )
        .await;
    matches!(outcome.decision.verdict, Verdict::Deny { .. })
        && outcome.decision.rule.rule_id == "app.crew.external_not_allowed"
}

#[tokio::test]
async fn kill_switch() {
    let h = Harness::new().await;
    let a = h.remote_agent();
    let l = agent();
    let c = h.charter(&[("local", l), ("remote", a)]);
    h.local.behave(|_, _| hangs());
    h.remote.behave(|_, _| hangs());
    let coord = TestCoordinator::scripted(vec![vec![
        delegate(l, "Local work"),
        delegate(a, "Remote work"),
    ]]);
    let id = h.start(c, coord.clone()).await;
    for _ in 0..500 {
        if h.local.start_count() == 1 && h.remote.start_count() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!((h.local.start_count(), h.remote.start_count()), (1, 1));
    tokio::time::sleep(Duration::from_millis(100)).await;
    h.orch.kill(id).await.expect("kill");
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Killed);
    assert_eq!(run.end_reason.as_deref(), Some("Stopped by you."));
    let local_run = h.local.started_runs()[0];
    let remote_run = h.remote.started_runs()[0];
    assert_eq!(h.local.cancels.lock().unwrap().clone(), vec![local_run]);
    assert!(
        h.remote.cancels.lock().unwrap().is_empty(),
        "the remote route cannot cancel"
    );
    assert_eq!(run_end_state(&h, local_run).await, RunState::Cancelled);
    assert_eq!(run_end_state(&h, remote_run).await, RunState::Abandoned);
    h.wait_reported(&coord).await;
}

#[tokio::test]
async fn escalation_carries_ordins_recommendation() {
    let h = Harness::new().await;
    let a = agent();
    let c = h.charter(&[("analyst", a)]);
    let coord = TestCoordinator::scripted(vec![vec![CrewToolCall::Escalate {
        question: "Which supplier first?".into(),
        recommendation: Some("Kestrel".into()),
    }]]);
    let id = h.start(c, coord.clone()).await;
    let run = h.wait_ended(id).await;
    h.wait_reported(&coord).await;
    assert_eq!(run.state, CrewRunState::Escalated);
    assert_eq!(run.end_reason.as_deref(), Some("Which supplier first?"));
    assert_eq!(
        coord.outcomes.lock().unwrap()[0].recommendation.as_deref(),
        Some("Kestrel")
    );
    let needs = h.events("needs_you_changed").await;
    assert_eq!(needs.len(), 1);
    assert_eq!(needs[0]["workspace_id"], serde_json::json!(h.workspace));
}

#[tokio::test]
async fn not_started_by_the_person() {
    let h = Harness::new().await;
    let c = h.charter(&[("analyst", agent())]);
    let err = h
        .orch
        .start(CrewStart {
            charter: c,
            inputs: vec![],
            coordinator: TestCoordinator::new(),
            started_by_person: false,
            trigger_id: None,
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), "CREW_NOT_STARTED_BY_PERSON");
}

#[tokio::test]
async fn a_trigger_starts_a_saved_workflow() {
    let h = Harness::new().await;
    let a = agent();
    let c = saved_charter(
        &h,
        &[("analyst", a)],
        vec![agent_step("1", a, "Compare the quotes")],
    );
    let trigger = TriggerId::new();
    let coord = TestCoordinator::new();
    let id = h
        .orch
        .start(CrewStart {
            charter: c,
            inputs: vec![],
            coordinator: coord.clone(),
            started_by_person: false,
            trigger_id: Some(trigger),
        })
        .await
        .expect("the crew starts");
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    let started = h.entries(id, "crew_started").await;
    assert_eq!(started[0]["trigger_id"], serde_json::json!(trigger));
}

#[tokio::test]
async fn one_crew_per_workspace() {
    let h = Harness::new().await;
    let a = agent();
    let c = h.charter(&[("analyst", a)]);
    h.local.behave(|_, _| hangs());
    let coord = TestCoordinator::scripted(vec![vec![delegate(a, "Work")]]);
    let id = h.start(c, coord).await;
    for _ in 0..500 {
        if h.local.start_count() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // The application restarts: the crew is paused for the restart.
    assert_eq!(h.orch.recover().await.unwrap(), 1);
    assert_eq!(h.orch.running_crew(h.workspace).await.unwrap(), Some(id));
    assert_eq!(
        h.orch
            .running_crew(ordin_types::WorkspaceId::new())
            .await
            .unwrap(),
        None
    );
}

// ---- saved steps

/// Waits until the adapter has started `n` runs.
async fn wait_starts(adapter: &FakeAdapter, n: usize) {
    for _ in 0..1_000 {
        if adapter.start_count() >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the adapter never started {n} runs");
}

fn text_of(start: &ordin_runtime_adapter::StartRun) -> String {
    match &start.input {
        ordin_runtime_adapter::StartInput::FreeText { text } => text.clone(),
        other => panic!("free text expected, got {other:?}"),
    }
}

fn if_block(
    position: &str,
    condition: Condition,
    yes: Vec<WorkflowStep>,
    no: Vec<WorkflowStep>,
) -> WorkflowStep {
    WorkflowStep::If(IfBlock {
        position: position.to_owned(),
        condition,
        paths: vec![
            NamedPath {
                name: "Yes".into(),
                steps: yes,
            },
            NamedPath {
                name: "No".into(),
                steps: no,
            },
        ],
    })
}

#[tokio::test]
async fn saved_steps_run_with_no_coordinator_turn_per_step() {
    let h = Harness::new().await;
    let (a, w) = (agent(), agent());
    let each = WorkflowStep::Each(EachBlock {
        position: "1".into(),
        list: uses_input("Quotes"),
        maximum: 20,
        steps: vec![agent_step("1.1", a, "Compare the quote")],
    });
    let mut s21 = step("2.1", w, "Update the summary and flag the rises");
    s21.asks_first = true;
    s21.ordin_checks_result = true;
    let iff = if_block(
        "2",
        Condition::Ordin {
            words: "Any rate rose by more than 5 per cent".into(),
        },
        vec![WorkflowStep::Agent(s21)],
        vec![],
    );
    let c = saved_charter(&h, &[("analyst", a), ("writer", w)], vec![each, iff]);
    let coord = TestCoordinator::new();
    let id = h
        .start_with(
            c,
            vec![list_input("Quotes", &["Kestrel", "Heron", "Plover"])],
            coord.clone(),
        )
        .await;
    let (run, approved) = tokio::join!(h.wait_ended(id), h.decide_pending(true));
    h.wait_reported(&coord).await;
    assert_eq!(run.state, CrewRunState::Finished);
    assert_eq!(run.end_reason.as_deref(), Some("all steps done"));
    assert_eq!(coord.count("round"), 0, "coordinator.round is never called");
    assert_eq!(coord.count("decide_condition"), 1);
    assert_eq!(coord.count("check_result"), 1);
    assert_eq!(h.local.start_count(), 4, "three quotes and the summary");
    assert_eq!(h.host.cards.lock().unwrap().clone(), vec!["2.1"]);
    let _ = approved;
    // Five rounds: the three quotes, the condition, the summary.
    let (_, rounds) = h.orch.get(id).await.unwrap();
    assert_eq!(rounds.len(), 5);
    let paths = h.entries(id, "crew_path_taken").await;
    assert_eq!(paths[0]["path"], "Yes");
    assert_eq!(paths[0]["decided_by"], "ordin");
    // Each quote's brief carries the item.
    let briefs: Vec<String> = h.local.starts.lock().unwrap().iter().map(text_of).collect();
    for item in ["Kestrel", "Heron", "Plover"] {
        assert!(
            briefs
                .iter()
                .any(|b| b.contains(&format!("- item: {item}"))),
            "{item}"
        );
    }
}

#[tokio::test]
async fn steps_at_the_same_time_respect_members_at_once() {
    let h = Harness::new().await;
    let members: Vec<_> = (0..5).map(|_| agent()).collect();
    let steps: Vec<WorkflowStep> = members
        .iter()
        .enumerate()
        .map(|(n, m)| {
            let mut s = step(&format!("1{}", (b'a' + n as u8) as char), *m, "Work");
            s.group = Some(1);
            WorkflowStep::Agent(s)
        })
        .collect();
    let roles: Vec<(String, _)> = members
        .iter()
        .enumerate()
        .map(|(n, m)| (format!("m{n}"), *m))
        .collect();
    let pairs: Vec<(&str, _)> = roles.iter().map(|(r, m)| (r.as_str(), *m)).collect();
    let mut c = saved_charter(&h, &pairs, steps);
    c.guardrails.max_members_running = 4;
    // Long enough that every start lands while the others run, however slowly they begin.
    h.local.behave(|_, _| slow(1_000, "Worked."));
    let coord = TestCoordinator::new();
    let id = h.start(c, coord).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    // D.1: at most min(members at once, the person's agents at once) start together; the rest
    // wait for a place.
    let allowed = 4.min(h.usage.agents_at_once() as usize);
    assert!(allowed < 5);
    assert_eq!(h.local.start_count(), 5);
    assert_eq!(h.local.peak(), allowed);
    let (_, rounds) = h.orch.get(id).await.unwrap();
    assert_eq!(rounds.len(), 1, "a group is one round");
}

#[tokio::test]
async fn step_fails_once_tried_again_one_level_up_then_succeeds() {
    let h = Harness::new().await;
    let a = agent();
    let steps = vec![agent_step("1", a, "First"), agent_step("2", a, "Second")];
    let c = saved_charter(&h, &[("analyst", a)], steps);
    h.local
        .behave(|_, nth| if nth == 1 { fails() } else { done("Fine.") });
    let coord = TestCoordinator::new();
    let id = h.start(c, coord.clone()).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    assert_eq!(h.local.start_count(), 3);
    let runs = h.local.started_runs();
    let level = |r| {
        let sup = h.supervisor.clone();
        async move { sup.get(r).await.unwrap().run.usage.model_level }
    };
    let first = level(runs[1]).await;
    let retry = level(runs[2]).await;
    assert_eq!(first.as_deref(), Some("standard"));
    assert_eq!(
        retry.as_deref(),
        Some("most_capable"),
        "the retry is one level up"
    );
    assert_eq!(coord.count("take_over"), 0);
    // Both attempts belong to the same round.
    let (_, rounds) = h.orch.get(id).await.unwrap();
    assert_eq!(rounds[1].member_runs.len(), 2);
}

#[tokio::test]
async fn step_fails_twice_with_if_it_fails_ordin_takes_over() {
    let h = Harness::new().await;
    let a = agent();
    let steps = vec![
        agent_step("1", a, "First"),
        agent_step("2", a, "Second"),
        agent_step("3", a, "Third"),
    ];
    let c = saved_charter(&h, &[("analyst", a)], steps);
    h.local.behave(|req, _| {
        if text_of(req).contains("Your part: Third") {
            fails()
        } else {
            done("Fine.")
        }
    });
    let coord =
        TestCoordinator::scripted(vec![vec![delegate(a, "Finish step three")], vec![finish()]]);
    let id = h.start(c, coord.clone()).await;
    let run = h.wait_ended(id).await;
    h.wait_reported(&coord).await;
    assert_eq!(run.state, CrewRunState::Finished);
    assert_eq!(run.taken_over_from_step.as_deref(), Some("3"));
    let takeovers = coord.takeovers.lock().unwrap().clone();
    assert_eq!(takeovers.len(), 1);
    assert_eq!(takeovers[0].0, "3");
    assert_eq!(
        takeovers[0].1,
        TakeOverReason::StepFailedTwice {
            position: "3".into()
        }
    );
    // The rest of the run is coordinated round by round, and Ordin's step is journalled as changed.
    assert_eq!(coord.count("round"), 2);
    let changed = h.entries(id, "crew_step_changed_by_ordin").await;
    assert_eq!(changed.len(), 1);
    assert_eq!(coord.outcomes.lock().unwrap()[0].changed_by_ordin.len(), 1);
}

#[tokio::test]
async fn agent_missing_at_a_step() {
    let h = Harness::new().await;
    let a = agent();
    let gone = agent();
    let steps = vec![agent_step("1", a, "First"), agent_step("2", gone, "Second")];
    let c = saved_charter(&h, &[("analyst", a)], steps);
    h.host.missing.lock().unwrap().insert(gone);
    let coord = TestCoordinator::scripted(vec![vec![finish()]]);
    let id = h.start(c, coord.clone()).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    let takeovers = coord.takeovers.lock().unwrap().clone();
    assert_eq!(
        takeovers,
        vec![(
            "2".to_owned(),
            TakeOverReason::AgentMissing { member: gone }
        )]
    );
    assert_eq!(h.local.start_count(), 1, "step 2 never ran");
}

#[tokio::test]
async fn ask_me_first_then_rejected() {
    let h = Harness::new().await;
    let a = agent();
    let mut s = step("2.1", a, "Update the summary");
    s.asks_first = true;
    let c = saved_charter(&h, &[("analyst", a)], vec![WorkflowStep::Agent(s)]);
    let coord = TestCoordinator::new();
    let id = h.start(c, coord.clone()).await;
    let (run, _) = tokio::join!(h.wait_ended(id), h.decide_pending(false));
    assert_eq!(run.state, CrewRunState::Escalated);
    assert_eq!(
        run.end_reason.as_deref(),
        Some("The person chose not to run step 2.1.")
    );
    assert_eq!(h.local.start_count(), 0);
}

#[tokio::test]
async fn repeat_until_reaches_its_maximum() {
    let h = Harness::new().await;
    let a = agent();
    let until = WorkflowStep::Until(UntilBlock {
        position: "1".into(),
        condition: Condition::Application {
            condition_kind: "step_failed".into(),
            looks_at_position: "1.1".into(),
        },
        maximum: 3,
        steps: vec![agent_step("1.1", a, "Improve the draft")],
    });
    let c = saved_charter(&h, &[("analyst", a)], vec![until]);
    let coord = TestCoordinator::new();
    let id = h.start(c, coord.clone()).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    assert_eq!(h.local.start_count(), 3);
    let questions = coord.questions.lock().unwrap().clone();
    assert_eq!(questions.len(), 1);
    assert_eq!(questions[0].kind, PersonQuestionKind::RepeatAtMaximum);
    assert_eq!(
        questions[0].choices,
        vec!["Carry on with the last result", "Stop"]
    );
}

#[tokio::test]
async fn for_each_beyond_its_maximum() {
    let h = Harness::new().await;
    let a = agent();
    // The company's default of 10 rounds stops a crew whose For each needs 20, so 8 and 12 are
    // the figures here.
    let each = WorkflowStep::Each(EachBlock {
        position: "1".into(),
        list: uses_input("Quotes"),
        maximum: 8,
        steps: vec![agent_step("1.1", a, "Compare the quote")],
    });
    let c = saved_charter(&h, &[("analyst", a)], vec![each]);
    let items: Vec<String> = (1..=12).map(|n| format!("quote-{n}")).collect();
    let refs: Vec<&str> = items.iter().map(String::as_str).collect();
    let coord = TestCoordinator::new();
    let id = h
        .start_with(c, vec![list_input("Quotes", &refs)], coord)
        .await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    assert_eq!(h.local.start_count(), 8, "only the first 8 items run");
    let briefs: Vec<String> = h.local.starts.lock().unwrap().iter().map(text_of).collect();
    assert!(briefs.iter().any(|b| b.contains("- item: quote-8")));
    assert!(!briefs.iter().any(|b| b.contains("- item: quote-9")));
    let cut = h.entries(id, "crew_items_cut").await;
    assert_eq!(cut.len(), 1);
    assert_eq!(cut[0]["items"], 12);
    assert_eq!(cut[0]["maximum"], 8);
}

// ---- restart, continue, take-over

/// A second orchestrator over the same stores: the application after a restart.
fn restarted(h: &Harness) -> std::sync::Arc<ordin_crew_orchestrator::CrewOrchestrator> {
    ordin_crew_orchestrator::CrewOrchestrator::new(
        h.store.clone(),
        h.journal.clone(),
        h.supervisor.clone(),
        h.rules.clone(),
        h.approvals.clone(),
        h.usage.clone(),
        std::sync::Arc::new(
            futures::executor::block_on(
                ordin_organisation_profile::OrganisationProfileService::start(
                    ordin_organisation_profile::ProfileMode::NotUsed,
                    ordin_organisation_profile::ProfileOptions {
                        refresh_interval: Duration::from_secs(3600),
                        clock: std::sync::Arc::new(Wall),
                    },
                ),
            )
            .unwrap(),
        ),
        h.host.clone(),
    )
}

struct Wall;

impl ordin_organisation_profile::Clock for Wall {
    fn now(&self) -> ordin_organisation_profile::Timestamp {
        ordin_organisation_profile::Timestamp::now()
    }
}

#[tokio::test]
async fn restart_never_continues_on_its_own() {
    let h = Harness::new().await;
    let a = agent();
    let c = h.charter(&[("analyst", a)]);
    // The first coordinator never answers: the crew is in the middle of a round.
    let stuck = TestCoordinator::new();
    stuck.stall.store(true, std::sync::atomic::Ordering::SeqCst);
    let id = h.start(c, stuck.clone()).await;
    for _ in 0..500 {
        if stuck.count("round") == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let app = restarted(&h);
    assert_eq!(app.recover().await.unwrap(), 1);
    let (run, _) = app.get(id).await.unwrap();
    assert_eq!(run.state, CrewRunState::Paused);
    assert_eq!(run.pause_reason, Some(CrewPauseReason::Restart));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        h.local.start_count(),
        0,
        "no member run starts until continue_crew"
    );
    let needs = h.events("needs_you_changed").await;
    assert_eq!(needs.len(), 1);
    // The person presses Continue.
    let coord = TestCoordinator::scripted(vec![vec![delegate(a, "Compare")], vec![finish()]]);
    app.continue_crew(id, coord.clone())
        .await
        .expect("continue");
    let ended = h.wait_ended(id).await;
    assert_eq!(ended.state, CrewRunState::Finished);
    assert_eq!(h.local.start_count(), 1);
    // Leaving `paused` for `running` told the screens again.
    assert!(h.events("needs_you_changed").await.len() >= 2);
}

#[tokio::test]
async fn ordin_asks_to_take_over_and_the_orchestrator_accepts() {
    let h = Harness::new().await;
    let a = agent();
    let steps = vec![
        agent_step("1", a, "First"),
        agent_step("2", a, "Second"),
        agent_step("3", a, "Third"),
    ];
    let c = saved_charter(&h, &[("analyst", a)], steps);
    h.local.behave(|_, nth| {
        if nth == 1 {
            slow(400, "Slow.")
        } else {
            done("Fine.")
        }
    });
    let coord = TestCoordinator::scripted(vec![vec![finish()]]);
    let id = h.start(c, coord.clone()).await;
    wait_starts(&h.local, 2).await;
    h.orch
        .request_take_over(id, "3".into(), "the summary needs a new section".into())
        .await
        .expect("accepted");
    let run = h.wait_ended(id).await;
    h.wait_reported(&coord).await;
    assert_eq!(run.state, CrewRunState::Finished);
    assert_eq!(
        h.local.start_count(),
        2,
        "step 2 finished and step 3 was not run by the walk"
    );
    let takeovers = coord.takeovers.lock().unwrap().clone();
    assert_eq!(
        takeovers,
        vec![(
            "3".to_owned(),
            TakeOverReason::OrdinAsked {
                position: "3".into(),
                reason: "the summary needs a new section".into()
            }
        )]
    );
    assert_eq!(
        coord.count("round"),
        1,
        "the rest is coordinated round by round"
    );
    let asked = h.entries(id, "crew_take_over_requested").await;
    assert_eq!(asked[0]["accepted"], true);
}

#[tokio::test]
async fn ordin_asks_to_take_over_from_a_step_already_done() {
    let h = Harness::new().await;
    let a = agent();
    let steps = vec![agent_step("1", a, "First"), agent_step("2", a, "Second")];
    let c = saved_charter(&h, &[("analyst", a)], steps);
    h.local.behave(|_, nth| {
        if nth == 1 {
            slow(400, "Slow.")
        } else {
            done("Fine.")
        }
    });
    let coord = TestCoordinator::new();
    let id = h.start(c, coord.clone()).await;
    wait_starts(&h.local, 2).await;
    let err = h
        .orch
        .request_take_over(id, "1".into(), "redo it".into())
        .await
        .unwrap_err();
    assert_eq!(err.code(), "CREW_TAKE_OVER_REFUSED");
    assert!(matches!(
        err,
        CrewError::TakeOverRefused {
            reason: "that step is not ahead of the crew"
        }
    ));
    // The walk goes on.
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    assert_eq!(run.end_reason.as_deref(), Some("all steps done"));
    assert_eq!(coord.count("take_over"), 0);
    let asked = h.entries(id, "crew_take_over_requested").await;
    assert_eq!(asked[0]["accepted"], false);
}
