//! One test per row of CrewOrchestrator.md's Error Table, each provoking the condition and
//! checking the code and whether it is retryable; plus the code registry check (X4).

mod support;

use std::time::Duration;

use ordin_crew_orchestrator::{
    CoordinatorError, CrewError, CrewStart, CrewToolCall, PlanProblem, TakeOverReason, ALL_CODES,
};
use ordin_types::domain::runs::{
    Condition, CrewRunState, IfBlock, NamedPath, RunState, WorkflowStep,
};
use ordin_types::{CrewRunId, TriggerId};
use support::*;

fn permanent(err: &CrewError, code: &str) {
    assert_eq!(err.code(), code);
    assert!(!err.retryable(), "{code} is permanent");
}

async fn start_err(h: &Harness, charter: ordin_types::domain::runs::CrewCharter) -> CrewError {
    h.orch
        .start(CrewStart {
            charter,
            inputs: vec![],
            coordinator: TestCoordinator::new(),
            started_by_person: true,
            trigger_id: None,
        })
        .await
        .map(|_| ())
        .unwrap_err()
}

#[tokio::test]
async fn start_without_the_persons_press() {
    let h = Harness::new().await;
    let a = agent();
    let attempt = |charter, trigger| {
        let orch = h.orch.clone();
        async move {
            orch.start(CrewStart {
                charter,
                inputs: vec![],
                coordinator: TestCoordinator::new(),
                started_by_person: false,
                trigger_id: trigger,
            })
            .await
            .map(|_| ())
            .unwrap_err()
        }
    };
    permanent(
        &attempt(h.charter(&[("analyst", a)]), None).await,
        "CREW_NOT_STARTED_BY_PERSON",
    );
    // A trigger starts saved workflows only.
    let trigger = Some(TriggerId::new());
    permanent(
        &attempt(h.charter(&[("analyst", a)]), trigger).await,
        "CREW_NOT_STARTED_BY_PERSON",
    );
    assert_eq!(h.local.start_count(), 0);
}

#[tokio::test]
async fn charter_breaks_a_2c_rule_or_the_plan_does_not_fit() {
    let h = Harness::new().await;
    let a = agent();
    let code_of = |e: CrewError| match e {
        CrewError::CharterInvalid { code } => code,
        other => panic!("a charter error expected, got {other:?}"),
    };
    let mut no_purpose = h.charter(&[("analyst", a)]);
    no_purpose.purpose.clear();
    let err = start_err(&h, no_purpose).await;
    permanent(&err, "CREW_CHARTER_INVALID");
    assert_eq!(code_of(err), "CHRT_PURPOSE_INVALID");

    let gone = agent();
    h.host.missing.lock().unwrap().insert(gone);
    let err = start_err(&h, h.charter(&[("analyst", gone)])).await;
    assert_eq!(code_of(err), "CHRT_MEMBER_UNKNOWN");

    let mut needs_input = h.charter(&[("analyst", a)]);
    needs_input.input_fields = vec!["Quotes".into()];
    let err = start_err(&h, needs_input).await;
    assert_eq!(code_of(err), "CREW_CHARTER_INVALID");

    let twice = vec![agent_step("1", a, "One"), agent_step("1", a, "Two")];
    let err = start_err(&h, saved_charter(&h, &[("analyst", a)], twice)).await;
    assert_eq!(code_of(err), "STEP_POSITION_INVALID");

    let mut external = h.charter(&[("analyst", a)]);
    external.guardrails.members_allowed_external_actions = vec![agent()];
    assert_eq!(
        code_of(start_err(&h, external).await),
        "CHRT_EXTERNAL_NOT_MEMBER"
    );
}

#[tokio::test]
async fn more_than_8_members() {
    let h = Harness::new().await;
    let members: Vec<_> = (0..9).map(|n| (format!("m{n}"), agent())).collect();
    let pairs: Vec<(&str, _)> = members.iter().map(|(r, m)| (r.as_str(), *m)).collect();
    let c = h.charter(&pairs);
    let plan = h.orch.check_plan(&c, &[]).await;
    assert!(!plan.fits);
    assert!(plan
        .problems
        .iter()
        .any(|p| matches!(p, PlanProblem::TooManyMembers { count: 9 })));
    let err = start_err(&h, c).await;
    assert!(matches!(
        err,
        CrewError::CharterInvalid {
            code: "CREW_TOO_MANY_MEMBERS"
        }
    ));
}

#[tokio::test]
async fn kill_continue_or_get_of_an_unknown_crew_run() {
    let h = Harness::new().await;
    let unknown = CrewRunId::new();
    permanent(&h.orch.kill(unknown).await.unwrap_err(), "CREW_NOT_FOUND");
    permanent(
        &h.orch
            .continue_crew(unknown, TestCoordinator::new())
            .await
            .unwrap_err(),
        "CREW_NOT_FOUND",
    );
    permanent(
        &h.orch.get(unknown).await.map(|_| ()).unwrap_err(),
        "CREW_NOT_FOUND",
    );
    permanent(
        &h.orch
            .request_take_over(unknown, "1".into(), "why".into())
            .await
            .unwrap_err(),
        "CREW_NOT_FOUND",
    );
}

async fn finished_crew(h: &Harness) -> CrewRunId {
    let a = agent();
    let coord = TestCoordinator::scripted(vec![vec![finish()]]);
    let id = h.start(h.charter(&[("analyst", a)]), coord.clone()).await;
    h.wait_ended(id).await;
    h.wait_reported(&coord).await;
    id
}

#[tokio::test]
async fn kill_of_an_ended_crew_run() {
    let h = Harness::new().await;
    let id = finished_crew(&h).await;
    permanent(&h.orch.kill(id).await.unwrap_err(), "CREW_ENDED");
    permanent(
        &h.orch
            .request_take_over(id, "1".into(), "why".into())
            .await
            .unwrap_err(),
        "CREW_ENDED",
    );
}

#[tokio::test]
async fn continue_of_a_crew_not_paused_by_a_restart() {
    let h = Harness::new().await;
    let id = finished_crew(&h).await;
    permanent(
        &h.orch
            .continue_crew(id, TestCoordinator::new())
            .await
            .unwrap_err(),
        "CREW_NOT_PAUSED",
    );
}

#[tokio::test]
async fn delegate_refused_by_a_guardrail() {
    let h = Harness::new().await;
    let (a, w) = (agent(), agent());
    let mut c = h.charter(&[("analyst", a), ("writer", w)]);
    c.allowed_handoffs = vec![handoff(a, w)];
    let coord = TestCoordinator::scripted(vec![
        vec![
            delegate(agent(), "Outsider"),
            CrewToolCall::Delegate {
                member: a,
                brief: "Compare".into(),
                uses_results_of: vec![w],
                uses_inputs: vec![],
            },
            CrewToolCall::Delegate {
                member: a,
                brief: "Compare".into(),
                uses_results_of: vec![],
                uses_inputs: vec!["Quotes".into()],
            },
        ],
        vec![finish()],
    ]);
    let id = h.start(c, coord.clone()).await;
    h.wait_ended(id).await;
    h.wait_reported(&coord).await;
    let closed = h.entries(id, "crew_round_closed").await;
    let first = closed
        .iter()
        .find(|e| e["round"] == 1)
        .expect("round 1 closed");
    assert_eq!(
        first["guardrail_codes"],
        serde_json::json!(vec!["CREW_GUARDRAIL_REFUSED"; 3])
    );
    let (_, rounds) = h.orch.get(id).await.unwrap();
    assert_eq!(
        rounds[0].guardrail_decisions,
        vec![
            "refused: This agent is not in the crew.",
            "refused: This hand-off is not allowed.",
            "refused: This input is not part of the crew.",
        ]
    );
    assert_eq!(h.local.start_count(), 0);
    // The crew goes on after a refusal.
    assert_eq!(coord.count("round"), 2);
}

#[tokio::test]
async fn round_time_or_request_limit() {
    // Rounds: covered by the scenario "Round limit"; requests and time here.
    let h = Harness::new().await;
    let a = agent();
    let mut c = h.charter(&[("analyst", a)]);
    c.guardrails.request_budget = 2;
    let coord = TestCoordinator::scripted(vec![vec![delegate(a, "One")], vec![delegate(a, "Two")]]);
    let id = h.start(c, coord.clone()).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::StoppedAtLimit);
    assert_eq!(run.end_reason.as_deref(), Some("Stopped after 2 requests."));
    assert_eq!(coord.count("round"), 1);

    let mut c = h.charter(&[("analyst", a)]);
    c.guardrails.wall_clock_secs = 1;
    // Far past the limit, so that a slow computer cannot finish the step before the limit fires.
    h.local.behave(|_, _| slow(5_000, "Slow."));
    let coord = TestCoordinator::scripted(vec![vec![delegate(a, "One")], vec![delegate(a, "Two")]]);
    let id = h.start(c, coord.clone()).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::StoppedAtLimit);
    assert_eq!(run.end_reason.as_deref(), Some("Stopped after 1 seconds."));
    assert_eq!(coord.count("round"), 1);
    let ended = h.entries(id, "crew_ended").await;
    assert_eq!(ended[0]["code"], "CREW_LIMIT_REACHED");
}

#[tokio::test]
async fn same_member_failed_twice() {
    let h = Harness::new().await;
    let a = agent();
    h.local.behave(|_, _| fails());
    let coord = TestCoordinator::scripted(vec![vec![delegate(a, "One")], vec![delegate(a, "Two")]]);
    let id = h.start(h.charter(&[("analyst", a)]), coord.clone()).await;
    let run = h.wait_ended(id).await;
    h.wait_reported(&coord).await;
    assert_eq!(run.state, CrewRunState::Escalated);
    assert_eq!(
        run.end_reason.as_deref(),
        Some("analyst failed twice in a row.")
    );
    assert_eq!(run.budget.repeated_failures, 2);
    assert_eq!(
        h.entries(id, "crew_ended").await[0]["code"],
        "CREW_MEMBER_FAILED_TWICE"
    );
}

#[tokio::test]
async fn ordin_unavailable_twice() {
    let h = Harness::new().await;
    let a = agent();
    let coord = TestCoordinator::new();
    let id = h.start(h.charter(&[("analyst", a)]), coord.clone()).await;
    let run = h.wait_ended(id).await;
    h.wait_reported(&coord).await;
    assert_eq!(run.state, CrewRunState::Failed);
    assert_eq!(
        run.end_reason.as_deref(),
        Some("Ordin could not continue the crew.")
    );
    assert_eq!(coord.count("round"), 2, "tried once more, then failed");
    assert_eq!(
        h.entries(id, "crew_ended").await[0]["code"],
        "CREW_COORDINATOR_UNAVAILABLE"
    );
}

#[tokio::test]
async fn ordins_answer_unusable() {
    let h = Harness::new().await;
    let a = agent();
    let iff = WorkflowStep::If(IfBlock {
        position: "2".into(),
        condition: Condition::Ordin {
            words: "Did a rate rise?".into(),
        },
        paths: vec![
            NamedPath {
                name: "Yes".into(),
                steps: vec![],
            },
            NamedPath {
                name: "No".into(),
                steps: vec![],
            },
        ],
    });
    let c = saved_charter(
        &h,
        &[("analyst", a)],
        vec![agent_step("1", a, "Compare"), iff],
    );
    let coord = TestCoordinator::scripted(vec![vec![finish()]]);
    *coord.path.lock().unwrap() = "Maybe".into();
    let id = h.start(c, coord.clone()).await;
    let run = h.wait_ended(id).await;
    assert_eq!(run.state, CrewRunState::Finished);
    // The answer names no path: journalled with the position, then Ordin takes over.
    let journalled = h.entries(id, "crew_coordinator_unusable").await;
    assert_eq!(journalled[0]["position"], "2");
    assert_eq!(journalled[0]["code"], "CREW_COORDINATOR_UNUSABLE");
    let takeovers = coord.takeovers.lock().unwrap().clone();
    assert_eq!(takeovers.len(), 1);
    assert_eq!(takeovers[0].0, "2");
    assert!(matches!(
        takeovers[0].1,
        TakeOverReason::CheckFoundShortfall { .. }
    ));
    assert_eq!(coord.count("round"), 1);
}

#[tokio::test]
async fn a_member_run_cannot_be_started() {
    let h = Harness::new().await;
    // A remote agent the program knows no route for.
    let lost = ordin_types::common::AgentRef::RemoteAgent {
        remote_agent_id: ordin_types::RemoteAgentId::new(),
    };
    let a = agent();
    let coord =
        TestCoordinator::scripted(vec![vec![delegate(lost, "Remote work")], vec![finish()]]);
    let id = h
        .start(
            h.charter(&[("remote", lost), ("analyst", a)]),
            coord.clone(),
        )
        .await;
    let run = h.wait_ended(id).await;
    h.wait_reported(&coord).await;
    assert_eq!(run.state, CrewRunState::Finished);
    // The step failed with the supervisor's code and counted as a failed attempt.
    let second = coord.views.lock().unwrap()[1].clone();
    assert_eq!(second.latest_results[0].state, RunState::Failed);
    assert_eq!(
        second.latest_results[0].summary.as_deref(),
        Some("Could not start: SUP_NO_ADAPTER")
    );
    assert_eq!(run.budget.repeated_failures, 1);
    assert_eq!(h.remote.start_count(), 0);
}

#[tokio::test]
async fn store_unavailable() {
    // A caller sees a retryable error.
    let h = Harness::new().await;
    let id = finished_crew(&h).await;
    h.pool().close().await;
    let err = h.orch.get(id).await.map(|_| ()).unwrap_err();
    assert_eq!(err.code(), "CREW_STORE_UNAVAILABLE");
    assert!(err.retryable());
    assert!(matches!(err, CrewError::Store(_)));
    assert!(h.orch.running_crew(h.workspace).await.is_err());
}

#[tokio::test]
async fn store_unavailable_in_a_driver_pauses_the_crew_with_reason_escalation() {
    let h = Harness::new().await;
    let a = agent();
    let coord = TestCoordinator::scripted(vec![vec![delegate(a, "Compare")], vec![finish()]]);
    coord.stall.store(true, std::sync::atomic::Ordering::SeqCst);
    let id = h.start(h.charter(&[("analyst", a)]), coord.clone()).await;
    for _ in 0..500 {
        if coord.count("round") == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // The rounds table disappears under the driver: its next write fails.
    sqlx::query("DROP TABLE rounds")
        .execute(h.pool())
        .await
        .unwrap();
    coord.release.notify_one();
    // `get` reads the rounds table too, so the state is read from the crew run row itself.
    let mut row = None;
    for _ in 0..500 {
        let found: Option<(String, Option<String>)> =
            sqlx::query_as("SELECT state, pause_reason FROM crew_runs WHERE id = ?1")
                .bind(id.to_string())
                .fetch_optional(h.pool())
                .await
                .unwrap();
        if found.as_ref().is_some_and(|(state, _)| state == "paused") {
            row = found;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let (_, reason) = row.expect("the crew paused");
    assert_eq!(reason.as_deref(), Some("escalation"));
    assert_eq!(h.local.start_count(), 0);
    // The pause commits the row before it emits `needs_you_changed`, so the event is waited for.
    let mut needs = Vec::new();
    for _ in 0..500 {
        needs = h.events("needs_you_changed").await;
        if !needs.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(needs.len(), 1);
}

#[tokio::test]
async fn ordin_asks_for_a_take_over_the_orchestrator_does_not_accept() {
    let h = Harness::new().await;
    let a = agent();
    h.local.behave(|_, _| slow(600, "Slow."));
    let steps = vec![agent_step("1", a, "First"), agent_step("2", a, "Second")];
    let c = saved_charter(&h, &[("analyst", a)], steps);
    let id = h
        .start(c, TestCoordinator::scripted(vec![vec![finish()]]))
        .await;
    for _ in 0..500 {
        if h.local.start_count() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let refused = |e: CrewError, why: &str| {
        permanent(&e, "CREW_TAKE_OVER_REFUSED");
        match e {
            CrewError::TakeOverRefused { reason } => assert_eq!(reason, why),
            other => panic!("{other:?}"),
        }
    };
    refused(
        h.orch
            .request_take_over(id, "2".into(), String::new())
            .await
            .unwrap_err(),
        "a reason is needed",
    );
    refused(
        h.orch
            .request_take_over(id, "2".into(), "x".repeat(501))
            .await
            .unwrap_err(),
        "a reason is needed",
    );
    refused(
        h.orch
            .request_take_over(id, "9".into(), "why".into())
            .await
            .unwrap_err(),
        "that step is not ahead of the crew",
    );
    h.wait_ended(id).await;
    // Coordinated crews are Ordin's already.
    let b = agent();
    let coord = TestCoordinator::scripted(vec![vec![delegate(b, "Slow work")], vec![finish()]]);
    let id = h.start(h.charter(&[("analyst", b)]), coord).await;
    refused(
        h.orch
            .request_take_over(id, "1".into(), "why".into())
            .await
            .unwrap_err(),
        "Ordin already coordinates this crew",
    );
    h.wait_ended(id).await;
}

#[tokio::test]
async fn a_take_over_request_for_a_crew_that_is_not_running_is_refused() {
    let h = Harness::new().await;
    let a = agent();
    h.local.behave(|_, _| hangs());
    let c = saved_charter(
        &h,
        &[("analyst", a)],
        vec![agent_step("1", a, "First"), agent_step("2", a, "Second")],
    );
    let id = h.start(c, TestCoordinator::new()).await;
    for _ in 0..500 {
        if h.local.start_count() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // After a restart nothing walks the outline until Continue.
    let app = ordin_crew_orchestrator::CrewOrchestrator::new(
        h.store.clone(),
        h.journal.clone(),
        h.supervisor.clone(),
        h.rules.clone(),
        h.approvals.clone(),
        h.usage.clone(),
        std::sync::Arc::new(
            ordin_organisation_profile::OrganisationProfileService::start(
                ordin_organisation_profile::ProfileMode::NotUsed,
                ordin_organisation_profile::ProfileOptions {
                    refresh_interval: Duration::from_secs(3600),
                    clock: std::sync::Arc::new(Wall),
                },
            )
            .await
            .unwrap(),
        ),
        h.host.clone(),
    );
    app.recover().await.unwrap();
    let err = app
        .request_take_over(id, "2".into(), "why".into())
        .await
        .unwrap_err();
    permanent(&err, "CREW_TAKE_OVER_REFUSED");
}

struct Wall;

impl ordin_organisation_profile::Clock for Wall {
    fn now(&self) -> ordin_organisation_profile::Timestamp {
        ordin_organisation_profile::Timestamp::now()
    }
}

#[test]
fn every_owned_code_is_in_the_registry() {
    for code in ALL_CODES {
        let entry = ordin_types::error_registry::lookup(code).expect("registered");
        assert_eq!(entry.owner_crate, "ordin-crew-orchestrator", "{code}");
    }
    let mut sorted = ALL_CODES.to_vec();
    sorted.sort_unstable();
    assert_eq!(sorted, ALL_CODES, "the list is sorted");
    for prefix in ["CREW", "ROUND", "STEP"] {
        assert!(
            ordin_types::error_registry::owns_prefix("ordin-crew-orchestrator", prefix),
            "{prefix}"
        );
    }
}

#[test]
fn every_returned_code_is_owned_and_retryable_follows_the_store() {
    let errors = [
        CrewError::CharterInvalid {
            code: "CHRT_PURPOSE_INVALID",
        },
        CrewError::NotStartedByPerson,
        CrewError::NotFound,
        CrewError::Ended,
        CrewError::NotPaused,
        CrewError::Busy,
        CrewError::TakeOverRefused {
            reason: "a reason is needed",
        },
        CrewError::Store(sqlx::Error::PoolClosed),
        CrewError::Journal(ordin_journal::JournalError::NotFound),
    ];
    for error in errors {
        assert!(ALL_CODES.contains(&error.code()), "{}", error.code());
        let coded: &dyn ordin_types::CodedError = &error;
        assert_eq!(coded.code(), error.code());
        assert_eq!(coded.retryable(), error.retryable());
        assert_eq!(
            error.retryable(),
            matches!(error, CrewError::Store(_) | CrewError::Journal(_))
        );
    }
}

#[test]
fn the_coordinator_errors_say_what_happened() {
    assert_eq!(
        CoordinatorError::Unavailable.to_string(),
        "Ordin's session is not available"
    );
    assert_eq!(
        CoordinatorError::StoppedByPerson.to_string(),
        "the person stopped the crew"
    );
    assert!(CoordinatorError::Unusable("x".into())
        .to_string()
        .contains('x'));
}
