//! The Internal Logic the Gherkin scenarios do not reach: plan checks, approvals, conditions,
//! calls, retries, take-overs and recovery.

mod support;

use std::sync::Arc;
use std::time::Duration;

use ordin_crew_orchestrator::{
    CheckVerdict, CrewInputValue, CrewOrchestrator, CrewToolCall, PersonQuestionKind, PlanProblem,
    TakeOverReason,
};
use ordin_runtime_adapter::RunEndReason;
use ordin_types::common::AgentRef;
use ordin_types::domain::runs::{
    CallBlock, Condition, CrewPauseReason, CrewRunState, EachBlock, IfBlock, IfItFails, NamedPath,
    RunState, UntilBlock, WorkflowStep,
};
use ordin_types::{ArtefactId, WorkflowId};
use support::*;

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

fn block(
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

fn application(kind: &str, looks_at: &str) -> Condition {
    Condition::Application {
        condition_kind: kind.into(),
        looks_at_position: looks_at.into(),
    }
}

// ---- A. check_plan

#[tokio::test]
async fn check_plan_lists_a_changed_agent_but_still_fits() {
    let h = Harness::new().await;
    let a = agent();
    let c = saved_charter(&h, &[("analyst", a)], vec![agent_step("1", a, "Compare")]);
    h.host.saved.lock().unwrap().push((a, "1".into()));
    h.host.versions.lock().unwrap().insert(a, "2".into());
    let plan = h.orch.check_plan(&c, &[]).await;
    assert!(
        plan.fits,
        "ADR-064: the run starts and Ordin takes over at that step"
    );
    assert!(matches!(
        plan.problems.as_slice(),
        [PlanProblem::AgentChanged { saved, present, .. }] if saved == "1" && present == "2"
    ));
    // Started anyway: the walk takes over at the step with `AgentChanged`.
    let coord = TestCoordinator::scripted(vec![vec![finish()]]);
    let id = h.start(c, coord.clone()).await;
    h.wait_ended(id).await;
    let takeovers = coord.takeovers.lock().unwrap().clone();
    assert_eq!(
        takeovers,
        vec![("1".to_owned(), TakeOverReason::AgentChanged { member: a })]
    );
}

#[tokio::test]
async fn check_plan_reports_missing_agents_inputs_and_lowered_limits() {
    let h = Harness::new().await;
    let a = agent();
    let mut c = h.charter(&[("analyst", a)]);
    c.input_fields = vec!["Quotes".into()];
    c.guardrails.max_rounds = 30;
    c.guardrails.wall_clock_secs = 8 * 3600;
    c.guardrails.max_members_running = 8;
    h.host.missing.lock().unwrap().insert(a);
    let plan = h.orch.check_plan(&c, &[]).await;
    assert!(!plan.fits);
    assert!(plan
        .problems
        .iter()
        .any(|p| matches!(p, PlanProblem::AgentMissing { .. })));
    assert!(plan
        .problems
        .iter()
        .any(|p| matches!(p, PlanProblem::InputMissing { label } if label == "Quotes")));
    // The company's defaults (10 rounds, 60 minutes, 4 at once) lower the asks and say so.
    let names: Vec<&str> = plan.lowered.iter().map(|l| l.limit.as_str()).collect();
    assert_eq!(
        names,
        [
            "limits.crew.rounds",
            "limits.crew.wall_clock_minutes",
            "limits.crew.members_at_once"
        ]
    );
    assert_eq!(plan.lowered[0].asked, 30);
    assert_eq!(plan.lowered[0].maximum, 10);
}

#[tokio::test]
async fn started_guardrails_are_lowered_to_the_maxima() {
    let h = Harness::new().await;
    let a = agent();
    let mut c = h.charter(&[("analyst", a)]);
    c.guardrails.max_rounds = 30;
    let coord = TestCoordinator::scripted(vec![vec![finish()]]);
    let id = h.start(c, coord).await;
    h.wait_ended(id).await;
    let stored: String = sqlx::query_scalar("SELECT charter FROM crew_runs WHERE id = ?1")
        .bind(id.to_string())
        .fetch_one(h.pool())
        .await
        .unwrap();
    let charter: serde_json::Value = serde_json::from_str(&stored).unwrap();
    assert_eq!(charter["guardrails"]["max_rounds"], 10);
}

#[tokio::test]
async fn check_plan_runs_the_step_rules() {
    let h = Harness::new().await;
    let a = agent();
    let code_of = |steps: Vec<WorkflowStep>, inputs: Vec<ordin_crew_orchestrator::CrewInput>| {
        let c = saved_charter(&h, &[("analyst", a)], steps);
        let orch = h.orch.clone();
        async move {
            let plan = orch.check_plan(&c, &inputs).await;
            assert!(!plan.fits);
            plan.problems
                .into_iter()
                .find_map(|p| match p {
                    PlanProblem::StepsInvalid { code, .. } => Some(code),
                    _ => None,
                })
                .expect("a step problem")
        }
    };
    let each = |max, list| {
        WorkflowStep::Each(EachBlock {
            position: "1".into(),
            list,
            maximum: max,
            steps: vec![agent_step("1.1", a, "Work")],
        })
    };
    assert_eq!(
        code_of(
            vec![agent_step("1", a, "A"), agent_step("1", a, "B")],
            vec![]
        )
        .await,
        "STEP_POSITION_INVALID"
    );
    assert_eq!(
        code_of(vec![agent_step("a b", a, "A")], vec![]).await,
        "STEP_POSITION_INVALID"
    );
    assert_eq!(
        code_of(vec![agent_step("1", a, "  ")], vec![]).await,
        "STEP_LINE_INVALID"
    );
    let mut later = step("1", a, "A");
    later.uses = vec![uses_result("2")];
    assert_eq!(
        code_of(
            vec![WorkflowStep::Agent(later), agent_step("2", a, "B")],
            vec![]
        )
        .await,
        "STEP_USES_LATER_OR_ABSENT"
    );
    assert_eq!(
        code_of(vec![each(0, uses_input("Q"))], vec![]).await,
        "STEP_EACH_MAX_OUT_OF_RANGE"
    );
    assert_eq!(
        code_of(vec![each(51, uses_input("Q"))], vec![]).await,
        "STEP_EACH_MAX_OUT_OF_RANGE"
    );
    assert_eq!(
        code_of(
            vec![each(5, uses_input("Q"))],
            vec![text_input("Q", "words")]
        )
        .await,
        "STEP_EACH_NOT_LIST"
    );
    assert_eq!(
        code_of(vec![each(5, uses_result("9"))], vec![]).await,
        "STEP_USES_LATER_OR_ABSENT"
    );
    let until = |max, condition| {
        WorkflowStep::Until(UntilBlock {
            position: "1".into(),
            condition,
            maximum: max,
            steps: vec![agent_step("1.1", a, "Work")],
        })
    };
    assert_eq!(
        code_of(vec![until(11, application("step_failed", "1.1"))], vec![]).await,
        "STEP_UNTIL_MAX_OUT_OF_RANGE"
    );
    assert_eq!(
        code_of(vec![until(3, application("made_up", "1.1"))], vec![]).await,
        "STEP_CONDITION_INVALID"
    );
    assert_eq!(
        code_of(vec![until(3, application("step_failed", "9"))], vec![]).await,
        "STEP_CONDITION_INVALID"
    );
    assert_eq!(
        code_of(
            vec![until(3, Condition::Person { words: " ".into() })],
            vec![]
        )
        .await,
        "STEP_CONDITION_INVALID"
    );
    let one_path = WorkflowStep::If(IfBlock {
        position: "1".into(),
        condition: Condition::Ordin {
            words: "Is it?".into(),
        },
        paths: vec![NamedPath {
            name: "Yes".into(),
            steps: vec![],
        }],
    });
    assert_eq!(
        code_of(vec![one_path], vec![]).await,
        "STEP_IF_PATHS_OUT_OF_RANGE"
    );
    // Nesting: five blocks inside one another.
    let mut nested = agent_step("5.1", a, "Deep");
    for level in (1..=5).rev() {
        nested = WorkflowStep::Until(UntilBlock {
            position: level.to_string(),
            condition: Condition::Ordin {
                words: "Done?".into(),
            },
            maximum: 2,
            steps: vec![nested],
        });
    }
    assert_eq!(code_of(vec![nested], vec![]).await, "STEP_NESTING_TOO_DEEP");
    let many: Vec<WorkflowStep> = (1..=51)
        .map(|n| agent_step(&n.to_string(), a, "Work"))
        .collect();
    assert_eq!(code_of(many, vec![]).await, "STEP_TOO_MANY");
    let big_group: Vec<WorkflowStep> = (0..9)
        .map(|n| {
            let mut s = step(&format!("1{}", (b'a' + n) as char), a, "Work");
            s.group = Some(1);
            WorkflowStep::Agent(s)
        })
        .collect();
    assert_eq!(code_of(big_group, vec![]).await, "STEP_GROUP_TOO_LARGE");
    // Calls: a workflow that is not there, a cycle, and a chain deeper than three.
    let gone = WorkflowId::new();
    let call = |id| {
        WorkflowStep::Call(CallBlock {
            position: "1".into(),
            workflow_id: id,
        })
    };
    assert_eq!(code_of(vec![call(gone)], vec![]).await, "STEP_CALL_INVALID");
    let (w1, w2, w3, w4) = (
        WorkflowId::new(),
        WorkflowId::new(),
        WorkflowId::new(),
        WorkflowId::new(),
    );
    {
        let mut flows = h.host.workflows.lock().unwrap();
        flows.insert(w1, vec![call(w2)]);
        flows.insert(w2, vec![call(w3)]);
        flows.insert(w3, vec![call(w4)]);
        flows.insert(w4, vec![agent_step("1", a, "Leaf")]);
    }
    assert_eq!(
        code_of(vec![call(w1)], vec![]).await,
        "STEP_CALL_INVALID",
        "four levels deep"
    );
    h.host.workflows.lock().unwrap().insert(w4, vec![call(w1)]);
    h.host.workflows.lock().unwrap().remove(&w3);
    h.host.workflows.lock().unwrap().insert(w3, vec![call(w1)]);
    assert_eq!(code_of(vec![call(w1)], vec![]).await, "STEP_CALL_CYCLE");
}

// ---- approvals

#[tokio::test]
async fn an_approval_point_for_a_step_or_a_member_asks_first() {
    let h = Harness::new().await;
    let (a, b) = (agent(), agent());
    let AgentRef::AgentInstance { agent_instance_id } = b else {
        panic!()
    };
    let mut c = saved_charter(
        &h,
        &[("analyst", a), ("writer", b)],
        vec![agent_step("1", a, "First"), agent_step("2", b, "Second")],
    );
    c.guardrails.approval_points = vec!["step:1".into(), format!("member:{agent_instance_id}")];
    let coord = TestCoordinator::new();
    let id = h.start(c, coord).await;
    let first = h.decide_pending(true).await;
    let second = h.decide_pending(true).await;
    assert_ne!(first, second);
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    assert_eq!(h.host.cards.lock().unwrap().clone(), vec!["1", "2"]);
    // The crew showed `paused` (approval) while it waited, and `running` after.
    let changes = h.events("crew_round_changed").await;
    assert!(changes.iter().any(|e| e["state"] == "paused"));
}

#[tokio::test]
async fn an_approval_point_for_a_member_in_a_coordinated_crew_asks_first_and_a_no_escalates() {
    let h = Harness::new().await;
    let a = agent();
    let AgentRef::AgentInstance { agent_instance_id } = a else {
        panic!()
    };
    let mut c = h.charter(&[("analyst", a)]);
    c.guardrails.approval_points = vec![format!("member:{agent_instance_id}")];
    let coord = TestCoordinator::scripted(vec![vec![delegate(a, "Compare")]]);
    let id = h.start(c, coord).await;
    let (run, _) = tokio::join!(h.wait_ended(id), h.decide_pending(false));
    assert_eq!(run.state, CrewRunState::Escalated);
    assert_eq!(
        run.end_reason.as_deref(),
        Some("The person chose not to run step round 1.")
    );
    assert_eq!(h.local.start_count(), 0);
}

// ---- D.1 failure handling

fn failing_step(position: &str, a: AgentRef, how: IfItFails) -> WorkflowStep {
    let mut s = step(position, a, "Fragile work");
    s.if_it_fails = how;
    WorkflowStep::Agent(s)
}

#[tokio::test]
async fn skip_it_marks_the_step_skipped_and_carries_on() {
    let h = Harness::new().await;
    let a = agent();
    let steps = vec![
        failing_step("1", a, IfItFails::SkipIt),
        agent_step("2", a, "Second"),
    ];
    h.local.behave(|req, _| {
        if text_of(req).contains("Fragile") {
            fails()
        } else {
            done("Fine.")
        }
    });
    let c = saved_charter(&h, &[("analyst", a)], steps);
    let coord = TestCoordinator::new();
    let id = h.start(c, coord.clone()).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    assert_eq!(
        h.local.start_count(),
        3,
        "two attempts at step 1, then step 2"
    );
    let state: String = sqlx::query_scalar(
        "SELECT state FROM crew_step_states WHERE crew_run_id = ?1 AND position = '1'",
    )
    .bind(id.to_string())
    .fetch_one(h.pool())
    .await
    .unwrap();
    assert_eq!(state, "skipped");
    assert_eq!(coord.count("take_over"), 0);
}

#[tokio::test]
async fn ask_me_after_two_failures_offers_try_again_skip_it_and_stop() {
    for (answer, expect) in [
        ("Skip it", "finished"),
        ("Stop", "escalated"),
        ("Try again", "takeover"),
        ("Unknown", "takeover"),
    ] {
        let h = Harness::new().await;
        let a = agent();
        let steps = vec![failing_step("1", a, IfItFails::AskMe)];
        h.local.behave(|_, _| fails());
        let c = saved_charter(&h, &[("analyst", a)], steps);
        let coord = TestCoordinator::scripted(vec![vec![finish()]]);
        coord.person.lock().unwrap().push_back(answer.to_owned());
        let id = h.start(c, coord.clone()).await;
        let run = h.wait_ended(id).await;
        let questions = coord.questions.lock().unwrap().clone();
        assert_eq!(
            questions[0].kind,
            PersonQuestionKind::StepFailedTwice,
            "{answer}"
        );
        assert_eq!(questions[0].choices, vec!["Try again", "Skip it", "Stop"]);
        match expect {
            "finished" => {
                assert_eq!(run.state, CrewRunState::Finished, "{answer}");
                assert_eq!(coord.count("take_over"), 0);
            }
            "escalated" => {
                assert_eq!(run.state, CrewRunState::Escalated, "{answer}");
                assert_eq!(
                    run.end_reason.as_deref(),
                    Some("The person chose to stop at step 1.")
                );
            }
            _ => {
                assert_eq!(run.taken_over_from_step.as_deref(), Some("1"), "{answer}");
                if answer == "Try again" {
                    assert_eq!(h.local.start_count(), 3, "one more attempt");
                    assert_eq!(
                        coord.takeovers.lock().unwrap()[0].1,
                        TakeOverReason::StepFailedTwice {
                            position: "1".into()
                        }
                    );
                }
            }
        }
    }
}

#[tokio::test]
async fn a_check_that_finds_a_shortfall_takes_over() {
    let h = Harness::new().await;
    let a = agent();
    let mut s = step("1", a, "Compare");
    s.ordin_checks_result = true;
    let c = saved_charter(
        &h,
        &[("analyst", a)],
        vec![WorkflowStep::Agent(s), agent_step("2", a, "After")],
    );
    let coord = TestCoordinator::scripted(vec![vec![finish()]]);
    coord
        .verdicts
        .lock()
        .unwrap()
        .push_back(CheckVerdict::Shortfall {
            reason: "No totals".into(),
            turns: 1,
        });
    let id = h.start(c, coord.clone()).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    assert_eq!(
        coord.takeovers.lock().unwrap().clone(),
        vec![(
            "1".to_owned(),
            TakeOverReason::CheckFoundShortfall {
                position: "1".into(),
                reason: "No totals".into()
            }
        )]
    );
    assert_eq!(h.local.start_count(), 1);
}

#[tokio::test]
async fn a_step_that_uses_an_input_nobody_gave_takes_over_with_input_missing() {
    let h = Harness::new().await;
    let a = agent();
    let mut s = step("1", a, "Compare");
    s.uses = vec![uses_input("Quotes")];
    let c = saved_charter(&h, &[("analyst", a)], vec![WorkflowStep::Agent(s)]);
    let coord = TestCoordinator::scripted(vec![vec![finish()]]);
    let id = h.start(c, coord.clone()).await;
    h.wait_ended(id).await;
    assert_eq!(
        coord.takeovers.lock().unwrap().clone(),
        vec![(
            "1".to_owned(),
            TakeOverReason::InputMissing {
                label: "Quotes".into()
            }
        )]
    );
}

#[tokio::test]
async fn a_later_step_gets_the_earlier_result_and_the_inputs_in_its_brief() {
    let h = Harness::new().await;
    let a = agent();
    let mut second = step("2", a, "Summarise");
    second.uses = vec![uses_input("Request"), uses_result("1")];
    let c = saved_charter(
        &h,
        &[("analyst", a)],
        vec![agent_step("1", a, "Compare"), WorkflowStep::Agent(second)],
    );
    let mut request = text_input("Request", "Which supplier is cheapest?");
    request.used = vec![ordin_types::common::InputUsed {
        name: "Request".into(),
        location: "chat".into(),
        version_read: "1".into(),
    }];
    h.local.behave(|_, nth| done(&format!("Result {nth}.")));
    let coord = TestCoordinator::new();
    let id = h.start_with(c, vec![request], coord).await;
    h.wait_ended(id).await;
    let briefs: Vec<String> = h.local.starts.lock().unwrap().iter().map(text_of).collect();
    assert!(
        briefs[1].contains("- Request: chat (version 1)"),
        "{}",
        briefs[1]
    );
    assert!(briefs[1].contains("- Request: Which supplier is cheapest?"));
    assert!(briefs[1].contains("- 1: Result 0."));
    let started = h.local.starts.lock().unwrap()[1].clone();
    assert_eq!(started.limits.request_budget, None);
    assert!(started.limits.wall_clock_secs <= 3_600);
    assert!(!matches!(
        started.caller,
        ordin_runtime_adapter::ToolCaller::Ordin
    ));
}

// ---- D.2 to D.5 blocks

#[tokio::test]
async fn application_conditions_choose_the_first_path_when_they_hold() {
    let h = Harness::new().await;
    let a = agent();
    let artefact = ArtefactId::new();
    // list_has_items: step 1 made an artefact; step_failed: step 1 did not fail.
    let iff = |pos: &str, kind: &str, yes: &str, no: &str| {
        block(
            pos,
            application(kind, "1"),
            vec![agent_step(yes, a, "Yes path")],
            vec![agent_step(no, a, "No path")],
        )
    };
    let steps = vec![
        agent_step("1", a, "Make the list"),
        iff("2", "list_has_items", "2.1", "2.2"),
        iff("3", "step_failed", "3.1", "3.2"),
        iff("4", "check_found_shortfall", "4.1", "4.2"),
        iff("5", "document_changed", "5.1", "5.2"),
    ];
    let mut first = step("1", a, "Make the list");
    first.uses = vec![uses_input("Doc")];
    let mut steps = steps;
    steps[0] = WorkflowStep::Agent(first);
    h.local.behave(move |_, nth| {
        if nth == 0 {
            done_with("Made.", vec![artefact])
        } else {
            done("Fine.")
        }
    });
    let c = saved_charter(&h, &[("analyst", a)], steps);
    let doc = ordin_crew_orchestrator::CrewInput {
        label: "Doc".into(),
        value: CrewInputValue::Documents {
            artefacts: vec![artefact],
        },
        used: vec![],
    };
    *h.host.changed.lock().unwrap() = true;
    let coord = TestCoordinator::new();
    let id = h.start_with(c, vec![doc], coord).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    let briefs: Vec<String> = h.local.starts.lock().unwrap().iter().map(text_of).collect();
    // 1, then 2.1 (has items), 3.2 (not failed), 4.2 (no shortfall), 5.1 (document changed).
    assert_eq!(briefs.len(), 5);
    let paths = h.entries(id, "crew_path_taken").await;
    let taken: std::collections::BTreeMap<String, String> = paths
        .iter()
        .map(|p| {
            (
                p["position"].as_str().unwrap().to_owned(),
                p["path"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(taken["2"], "Yes");
    assert_eq!(taken["3"], "No");
    assert_eq!(taken["4"], "No");
    assert_eq!(taken["5"], "Yes");
    // The document was attached to the step that used it.
    assert_eq!(
        h.local.starts.lock().unwrap()[0].attachments,
        vec![artefact]
    );
}

#[tokio::test]
async fn a_person_decided_condition_asks_the_person_and_an_unknown_answer_takes_over() {
    let h = Harness::new().await;
    let a = agent();
    let person = |p: &str| {
        block(
            p,
            Condition::Person {
                words: "Shall I go on?".into(),
            },
            vec![agent_step("2.1", a, "Go on")],
            vec![],
        )
    };
    let c = saved_charter(&h, &[("analyst", a)], vec![person("2")]);
    let coord = TestCoordinator::scripted(vec![vec![finish()]]);
    coord.person.lock().unwrap().push_back("Yes".into());
    let id = h.start(c.clone(), coord.clone()).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    assert_eq!(h.local.start_count(), 1);
    assert_eq!(
        coord.questions.lock().unwrap()[0].kind,
        PersonQuestionKind::AskMe
    );
    let paths = h.entries(id, "crew_path_taken").await;
    assert_eq!(paths[0]["decided_by"], "person");

    let coord = TestCoordinator::scripted(vec![vec![finish()]]);
    coord.person.lock().unwrap().push_back("Perhaps".into());
    let id = h.start(c, coord.clone()).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.taken_over_from_step.as_deref(), Some("2"));
    assert_eq!(coord.takeovers.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_repeat_until_leaves_when_its_condition_holds_and_stops_when_the_person_says_so() {
    let h = Harness::new().await;
    let a = agent();
    let until = |condition| {
        WorkflowStep::Until(UntilBlock {
            position: "1".into(),
            condition,
            maximum: 4,
            steps: vec![agent_step("1.1", a, "Improve")],
        })
    };
    // Ordin says "No" twice, then "Yes".
    let c = saved_charter(
        &h,
        &[("analyst", a)],
        vec![until(Condition::Ordin {
            words: "Good enough?".into(),
        })],
    );
    let coord = TestCoordinator::new();
    let paths = std::sync::Mutex::new(vec!["Yes", "No", "No"]);
    // The double answers with the path set before each pass; flip it on every decision.
    let flipper = {
        let coord = coord.clone();
        tokio::spawn(async move {
            let mut seen = 0;
            loop {
                let asked = coord.paths_asked.lock().unwrap().len();
                if asked != seen {
                    seen = asked;
                    let next = paths.lock().unwrap().pop().unwrap_or("Yes");
                    *coord.path.lock().unwrap() = next.to_owned();
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
    };
    *coord.path.lock().unwrap() = "No".into();
    let id = h.start(c, coord.clone()).await;
    let run = h.wait_ended(id).await;
    flipper.abort();
    assert_eq!(run.state, CrewRunState::Finished);
    assert!(h.local.start_count() >= 2 && h.local.start_count() <= 4);
    assert!(coord.questions.lock().unwrap().is_empty() || h.local.start_count() == 4);

    // "Stop" at the maximum escalates the crew.
    let c = saved_charter(
        &h,
        &[("analyst", a)],
        vec![until(application("step_failed", "1.1"))],
    );
    let coord = TestCoordinator::new();
    coord.person.lock().unwrap().push_back("Stop".into());
    let id = h.start(c, coord).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Escalated);
    assert_eq!(
        run.end_reason.as_deref(),
        Some("The person chose to stop at step 1.")
    );
}

#[tokio::test]
async fn a_person_decided_repeat_until_asks_with_yes_and_no() {
    let h = Harness::new().await;
    let a = agent();
    let until = WorkflowStep::Until(UntilBlock {
        position: "1".into(),
        condition: Condition::Person {
            words: "Is it good?".into(),
        },
        maximum: 3,
        steps: vec![agent_step("1.1", a, "Improve")],
    });
    let c = saved_charter(&h, &[("analyst", a)], vec![until]);
    let coord = TestCoordinator::new();
    coord
        .person
        .lock()
        .unwrap()
        .extend(["No".to_owned(), "Yes".to_owned()]);
    let id = h.start(c, coord.clone()).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    assert_eq!(h.local.start_count(), 2);
    let questions = coord.questions.lock().unwrap().clone();
    assert!(questions
        .iter()
        .all(|q| q.kind == PersonQuestionKind::RepeatUntil && q.choices == ["Yes", "No"]));
}

#[tokio::test]
async fn another_workflow_runs_in_place_and_a_missing_one_takes_over() {
    let h = Harness::new().await;
    let a = agent();
    let inner_id = WorkflowId::new();
    h.host.workflows.lock().unwrap().insert(
        inner_id,
        vec![
            agent_step("1", a, "Inner one"),
            agent_step("2", a, "Inner two"),
        ],
    );
    let call = |id| {
        WorkflowStep::Call(CallBlock {
            position: "3".into(),
            workflow_id: id,
        })
    };
    let c = saved_charter(
        &h,
        &[("analyst", a)],
        vec![agent_step("1", a, "Outer"), call(inner_id)],
    );
    let coord = TestCoordinator::new();
    let id = h.start(c, coord).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    assert_eq!(h.local.start_count(), 3);
    // Numbered under the block.
    let positions: Vec<String> = sqlx::query_scalar(
        "SELECT position FROM crew_step_states WHERE crew_run_id = ?1 ORDER BY position",
    )
    .bind(id.to_string())
    .fetch_all(h.pool())
    .await
    .unwrap();
    assert_eq!(positions, ["1", "3.1", "3.2"]);
    // Gone between the plan check and the walk: Ordin takes over with `InputMissing`.
    let h2 = Harness::new().await;
    let b = agent();
    let flow = WorkflowId::new();
    h2.host
        .workflows
        .lock()
        .unwrap()
        .insert(flow, vec![agent_step("1", b, "Inner")]);
    let c = saved_charter(&h2, &[("analyst", b)], vec![call(flow)]);
    assert!(h2.orch.check_plan(&c, &[]).await.fits);
    let coord = TestCoordinator::scripted(vec![vec![finish()]]);
    let started = h2.start(c, coord.clone()).await;
    h2.wait_ended(started).await;
    // The same workflow vanishing after the check, in a second crew.
    let c = saved_charter(
        &h2,
        &[("analyst", b)],
        vec![WorkflowStep::Call(CallBlock {
            position: "3".into(),
            workflow_id: WorkflowId::new(),
        })],
    );
    let plan = h2.orch.check_plan(&c, &[]).await;
    assert!(!plan.fits, "the plan check sees the missing workflow");
}

// ---- E. member runs

#[tokio::test]
async fn a_remote_member_runs_on_its_route() {
    let h = Harness::new().await;
    let r = h.remote_agent();
    let coord = TestCoordinator::scripted(vec![vec![delegate(r, "Remote work")], vec![finish()]]);
    h.remote.behave(|_, _| done("Remote done."));
    let id = h.start(h.charter(&[("remote", r)]), coord).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    assert_eq!(h.remote.start_count(), 1);
    assert!(matches!(
        h.remote.starts.lock().unwrap()[0].target,
        ordin_runtime_adapter::RunTarget::RemoteAgent { .. }
    ));
}

#[tokio::test]
async fn an_empty_turn_is_treated_as_an_escalation() {
    let h = Harness::new().await;
    let a = agent();
    let coord = TestCoordinator::new();
    *coord.round_fn.lock().unwrap() = Box::new(|_| {
        Ok(ordin_crew_orchestrator::CoordinatorTurn {
            calls: vec![],
            turns: 1,
        })
    });
    let id = h.start(h.charter(&[("analyst", a)]), coord).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Escalated);
    assert_eq!(
        run.end_reason.as_deref(),
        Some("Ordin did not say what to do next.")
    );
    let (_, rounds) = h.orch.get(id).await.unwrap();
    assert_eq!(rounds[0].coordinator_decision, "escalate");
}

#[tokio::test]
async fn several_delegates_in_one_turn_run_together_and_a_finish_after_them_waits() {
    let h = Harness::new().await;
    let (a, b) = (agent(), agent());
    // Long enough that both starts land while the other runs, however slowly they begin.
    h.local.behave(|_, _| slow(1_000, "Worked."));
    let coord = TestCoordinator::scripted(vec![vec![
        delegate(a, "One"),
        delegate(b, "Two"),
        CrewToolCall::Finish {
            summary: "All done".into(),
        },
    ]]);
    let id = h
        .start(h.charter(&[("analyst", a), ("writer", b)]), coord)
        .await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    assert_eq!(run.end_reason.as_deref(), Some("All done"));
    assert_eq!(h.local.start_count(), 2);
    assert_eq!(h.local.peak(), 2);
    let (_, rounds) = h.orch.get(id).await.unwrap();
    assert_eq!(rounds[0].member_runs.len(), 2);
    assert!(rounds[0]
        .coordinator_decision
        .starts_with("delegate analyst: One; delegate writer: Two; finish"));
    let ended = h
        .supervisor
        .get(rounds[0].member_runs[0])
        .await
        .unwrap()
        .run;
    assert_eq!(ended.state, RunState::Completed);
}

// ---- J. recovery and continue

fn app_after_restart(h: &Harness) -> Arc<CrewOrchestrator> {
    CrewOrchestrator::new(
        h.store.clone(),
        h.journal.clone(),
        h.supervisor.clone(),
        h.rules.clone(),
        h.approvals.clone(),
        h.usage.clone(),
        Arc::new(futures::executor::block_on(profile())),
        h.host.clone(),
    )
}

async fn profile() -> ordin_organisation_profile::OrganisationProfileService {
    struct Wall;
    impl ordin_organisation_profile::Clock for Wall {
        fn now(&self) -> ordin_organisation_profile::Timestamp {
            ordin_organisation_profile::Timestamp::now()
        }
    }
    ordin_organisation_profile::OrganisationProfileService::start(
        ordin_organisation_profile::ProfileMode::NotUsed,
        ordin_organisation_profile::ProfileOptions {
            refresh_interval: Duration::from_secs(3600),
            clock: Arc::new(Wall),
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn a_saved_steps_crew_continues_from_its_cursor_and_an_interrupted_run_is_a_failed_attempt() {
    let h = Harness::new().await;
    let a = agent();
    let mut first_step = step("1", a, "First");
    first_step.ordin_checks_result = true;
    let steps = vec![
        WorkflowStep::Agent(first_step),
        agent_step("2", a, "Second"),
    ];
    let c = saved_charter(&h, &[("analyst", a)], steps);
    // The first coordinator never answers the check: the crew stands after step 1.
    let before = TestCoordinator::new();
    before
        .stall
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let id = h.start(c, before.clone()).await;
    for _ in 0..500 {
        if before.count("check_result") == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(h.local.start_count(), 1);
    // Step 2 was going when the application stopped: its run ended `interrupted`.
    h.local.behave(|_, nth| {
        if nth == 1 {
            vec![started(), ended(RunEndReason::Interrupted, None, vec![])]
        } else {
            done("Fine.")
        }
    });
    let lost = h
        .supervisor
        .start_run(ordin_session_supervisor::StartRequest {
            origin: ordin_types::domain::runs::RunOrigin::CrewRound {
                crew_run_id: id,
                round_number: 2,
            },
            workspace_id: h.workspace,
            target: ordin_runtime_adapter::RunTarget::AgentInstance {
                agent_instance_id: match a {
                    AgentRef::AgentInstance { agent_instance_id } => agent_instance_id,
                    _ => unreachable!(),
                },
            },
            input: ordin_runtime_adapter::StartInput::FreeText {
                text: "Second".into(),
            },
            attachments: vec![],
            limits: None,
            work_kind: ordin_usage_planner::WorkKind::Documents,
            work_key: "crew:test".into(),
            inputs_used: vec![],
            thread: None,
            started_by_person: false,
            model_override: None,
            coding_task_id: None,
            only_servers: None,
        })
        .await
        .unwrap();
    assert_eq!(run_end_state(&h, lost).await, RunState::Interrupted);
    sqlx::query(
        "INSERT INTO crew_step_states (crew_run_id, position, state, attempts, run_id) VALUES (?1, '2', 'running', 1, ?2)",
    )
    .bind(id.to_string())
    .bind(lost.to_string())
    .execute(h.pool())
    .await
    .unwrap();
    let app = app_after_restart(&h);
    assert_eq!(app.recover().await.unwrap(), 1);
    let coord = TestCoordinator::new();
    app.continue_crew(id, coord.clone()).await.unwrap();
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    assert_eq!(run.end_reason.as_deref(), Some("all steps done"));
    // Step 1 was done before the restart and is not run again (nor checked again); the
    // interrupted run of step 2 counted as its first attempt, so it ran once more.
    assert_eq!(coord.count("check_result"), 0);
    assert_eq!(coord.count("round"), 0);
    let attempts: i64 = sqlx::query_scalar(
        "SELECT attempts FROM crew_step_states WHERE crew_run_id = ?1 AND position = '2'",
    )
    .bind(id.to_string())
    .fetch_one(h.pool())
    .await
    .unwrap();
    assert_eq!(attempts, 2);
}

#[tokio::test]
async fn a_coordinated_crew_continues_and_follows_the_member_runs_still_in_progress() {
    let h = Harness::new().await;
    let (a, b) = (agent(), agent());
    h.local.behave(|_, _| slow(300, "Late."));
    let first = TestCoordinator::scripted_then_stall(vec![vec![delegate(a, "Slow work")]]);
    let id = h
        .start(h.charter(&[("analyst", a), ("writer", b)]), first)
        .await;
    wait_starts(&h.local, 1).await;
    h.wait_member_recorded(id).await;
    let app = app_after_restart(&h);
    app.recover().await.unwrap();
    let coord = TestCoordinator::scripted(vec![vec![finish()]]);
    app.continue_crew(id, coord.clone()).await.unwrap();
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    h.wait_reported(&coord).await;
    // The open round was closed with the run it had, and the coordinator sees its result.
    let (_, rounds) = h.orch.get(id).await.unwrap();
    assert!(rounds.iter().all(|r| r.closed_at.is_some()));
    let view = coord.views.lock().unwrap()[0].clone();
    assert_eq!(view.latest_results[0].summary.as_deref(), Some("Late."));
    assert_eq!(h.local.start_count(), 1, "no run started again");
}

#[tokio::test]
async fn continue_refuses_a_crew_that_is_already_going_and_kill_ends_one_paused_by_a_restart() {
    let h = Harness::new().await;
    let a = agent();
    h.local.behave(|_, _| hangs());
    let first = TestCoordinator::scripted_then_stall(vec![vec![delegate(a, "Work")]]);
    let id = h.start(h.charter(&[("analyst", a)]), first).await;
    wait_starts(&h.local, 1).await;
    h.wait_member_recorded(id).await;
    // Still going in this process: nothing to continue.
    assert_eq!(
        h.orch
            .continue_crew(id, TestCoordinator::new())
            .await
            .unwrap_err()
            .code(),
        "CREW_NOT_PAUSED"
    );
    let app = app_after_restart(&h);
    app.recover().await.unwrap();
    // The person stops the crew instead of continuing it.
    app.kill(id).await.unwrap();
    let (run, _) = app.get(id).await.unwrap();
    assert_eq!(run.state, CrewRunState::Killed);
    assert_eq!(run.end_reason.as_deref(), Some("Stopped by you."));
    assert_eq!(app.running_crew(h.workspace).await.unwrap(), None);
    assert_eq!(run.pause_reason, None::<CrewPauseReason>);
    assert_eq!(
        run_end_state(&h, h.local.started_runs()[0]).await,
        RunState::Cancelled
    );
}

#[tokio::test]
async fn recover_with_nothing_in_progress_pauses_nothing() {
    let h = Harness::new().await;
    assert_eq!(h.orch.recover().await.unwrap(), 0);
}

#[tokio::test]
async fn a_member_question_that_ordin_cannot_answer_cancels_the_member_run() {
    let h = Harness::new().await;
    let a = agent();
    h.local.behave(|_, _| asks("Which one?"));
    let coord = TestCoordinator::scripted(vec![vec![delegate(a, "Compare")], vec![finish()]]);
    coord
        .answer_unusable
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let id = h.start(h.charter(&[("analyst", a)]), coord.clone()).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    assert!(
        h.local.resumes.lock().unwrap().is_empty(),
        "no answer was given: {:?}",
        h.local.resumes.lock().unwrap()
    );
    assert_eq!(
        run_end_state(&h, h.local.started_runs()[0]).await,
        RunState::Cancelled
    );
}

#[tokio::test]
async fn the_person_stopping_ordin_kills_the_crew() {
    let h = Harness::new().await;
    let a = agent();
    let coord = TestCoordinator::new();
    *coord.round_fn.lock().unwrap() =
        Box::new(|_| Err(ordin_crew_orchestrator::CoordinatorError::StoppedByPerson));
    let id = h.start(h.charter(&[("analyst", a)]), coord.clone()).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Killed);
    assert_eq!(run.end_reason.as_deref(), Some("Stopped by you."));
}

#[tokio::test]
async fn an_answer_that_cannot_be_used_twice_in_a_round_fails_the_crew() {
    let h = Harness::new().await;
    let a = agent();
    let coord = TestCoordinator::new();
    *coord.round_fn.lock().unwrap() = Box::new(|_| {
        Err(ordin_crew_orchestrator::CoordinatorError::Unusable(
            "garbled".into(),
        ))
    });
    let id = h.start(h.charter(&[("analyst", a)]), coord.clone()).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Failed);
    assert_eq!(coord.count("round"), 2);
}

#[tokio::test]
async fn a_for_each_over_documents_attaches_each_document() {
    let h = Harness::new().await;
    let a = agent();
    let docs = vec![ArtefactId::new(), ArtefactId::new()];
    let each = WorkflowStep::Each(EachBlock {
        position: "1".into(),
        list: uses_input("Docs"),
        maximum: 5,
        steps: vec![agent_step("1.1", a, "Read the document")],
    });
    let c = saved_charter(&h, &[("analyst", a)], vec![each]);
    let input = ordin_crew_orchestrator::CrewInput {
        label: "Docs".into(),
        value: CrewInputValue::Documents {
            artefacts: docs.clone(),
        },
        used: vec![],
    };
    let id = h.start_with(c, vec![input], TestCoordinator::new()).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    let mut attached: Vec<ArtefactId> = h
        .local
        .starts
        .lock()
        .unwrap()
        .iter()
        .flat_map(|s| s.attachments.clone())
        .collect();
    let mut wanted = docs;
    attached.sort_by_key(|a| a.to_string());
    wanted.sort_by_key(|a| a.to_string());
    assert_eq!(attached, wanted);
}

#[tokio::test]
async fn a_for_each_over_an_earlier_result_runs_once_per_artefact() {
    let h = Harness::new().await;
    let a = agent();
    let made = vec![ArtefactId::new(), ArtefactId::new(), ArtefactId::new()];
    let each = WorkflowStep::Each(EachBlock {
        position: "2".into(),
        list: uses_result("1"),
        maximum: 5,
        steps: vec![agent_step("2.1", a, "Check one")],
    });
    let c = saved_charter(
        &h,
        &[("analyst", a)],
        vec![agent_step("1", a, "Make the list"), each],
    );
    let list = made.clone();
    h.local.behave(move |_, nth| {
        if nth == 0 {
            done_with("Made three.", list.clone())
        } else {
            done("Checked.")
        }
    });
    let id = h.start(c, TestCoordinator::new()).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    assert_eq!(h.local.start_count(), 4);
}

#[tokio::test]
async fn the_crews_request_budget_is_the_budget_of_each_member_run() {
    let h = Harness::new().await;
    let a = agent();
    let mut c = h.charter(&[("analyst", a)]);
    c.guardrails.request_budget = 50;
    let coord = TestCoordinator::scripted(vec![
        vec![delegate(a, "One")],
        vec![delegate(a, "Two")],
        vec![finish()],
    ]);
    let id = h.start(c, coord).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    let budgets: Vec<Option<u32>> = h
        .local
        .starts
        .lock()
        .unwrap()
        .iter()
        .map(|s| s.limits.request_budget)
        .collect();
    // 50 less the first turn; then less the member's turn and the second turn.
    assert_eq!(budgets, vec![Some(49), Some(47)]);
}
