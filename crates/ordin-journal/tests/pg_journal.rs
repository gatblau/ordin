//! The Journal on PostgreSQL 18 (agent scaled shape): the core paths of every record family on a
//! copy of the migrated template, the cross-instance delivery of Internal Logic 5.5, and the ping
//! scenario. Needs a reachable container runtime (`DOCKER_HOST`).

mod support;

use std::time::Duration;

use ordin_journal::{
    ArtefactFilter, EntryFilter, Journal, NewArtefact, NewDashboard, NewExternalAction,
    NewJournalEntry, PlannedStep,
};
use ordin_types::domain::places::ArtefactLocation;
use ordin_types::domain::runs::{
    CellValue, ColumnType, DashboardSource, ExternalActionKind, ExternalActionState, ReportData,
    ReportSection, ReportSource, ReportTableColumn, StartupReport, StartupSubject, StepMark,
    StepSource, ToolRequestDecision, ToolRequestState,
};
use ordin_types::num::Sha256Hex;
use ordin_types::{CodedError, ExternalActionKey, PlaceId, ReportId, RunId, TaskId, WorkspaceId};
use serde_json::json;
use support::{digest, options, pg, pg_journal, Participant, RecordingSink, TestClock};

#[tokio::test]
async fn events_entries_and_plans_on_postgres() {
    let (j, clock) = pg_journal().await;
    let run = RunId::new();
    j.append_event(
        "run_state_changed",
        json!({"run_id": run, "state": "running"}),
    )
    .await
    .unwrap();
    let plan = j
        .state_plan(
            run,
            (1..=3)
                .map(|i| PlannedStep {
                    line: format!("Step {i}"),
                    came_from: StepSource::Workflow,
                })
                .collect(),
        )
        .await
        .unwrap();
    assert_eq!(plan[2].position, 3);
    j.mark_step(run, 1, StepMark::Now).await.unwrap();
    let seen = j
        .append_event("run_progress", json!({"run_id": run}))
        .await
        .unwrap();
    assert_eq!(seen.seq, 4);
    let current = j.current_plan(run).await.unwrap();
    assert!(current[0].events_seen.contains(&4));
    let events = j.events_since(0, 100).await.unwrap();
    assert_eq!(
        events.iter().map(|e| e.seq).collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
    assert_eq!(events[0].data["state"], "running");
    assert_eq!(j.running_runs().await.unwrap()[0].run_id, run);
    clock.advance_ms(5);
    j.append_event(
        "run_state_changed",
        json!({"run_id": run, "state": "completed"}),
    )
    .await
    .unwrap();
    assert!(j.runs_in_progress().await.unwrap().is_empty());
    let ended = j.runs_ended_since(events[0].at, 10).await.unwrap();
    assert_eq!(ended[0].run_id, run);
    assert_eq!(
        j.mark_step(run, 1, StepMark::Done)
            .await
            .unwrap_err()
            .code(),
        "RSTEP_IMMUTABLE"
    );
    assert!(matches!(
        j.events_since(99, 1).await,
        Err(ordin_journal::JournalError::SeqAhead { last_seq: 5 })
    ));
    for i in 0..101 {
        clock.advance_ms(1);
        j.append_entry(NewJournalEntry {
            kind: "repair_done".to_owned(),
            component: "rules".to_owned(),
            run_id: Some(run),
            crew_run_id: None,
            workspace_id: None,
            summary: format!("Entry {i}"),
            data: json!({"level": "warning", "code": "RULE_X_Y"}),
        })
        .await
        .unwrap();
    }
    let page = j
        .list_entries(
            EntryFilter {
                run_id: Some(run),
                kind: None,
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(page.items.len(), 100);
    assert_eq!(page.items[0].summary, "Entry 100");
    let rest = j.diagnostic_lines(page.next_cursor).await.unwrap();
    assert_eq!(rest.items.len(), 1);
    assert_eq!(rest.items[0].level, "warning");
    let bundle = j.support_export(&[run]).await.unwrap();
    assert!(!bundle.bytes.is_empty());
    assert_eq!(j.count_for_runs(&[run]).await.unwrap(), 5 + 101 + 3 + 1);
    j.compact().await.unwrap();
    j.shutdown().await;
    j.datastore().close().await;
}

#[tokio::test]
async fn records_on_postgres() {
    let (j, clock) = pg_journal().await;
    let run = RunId::new();
    // Tool requests.
    let r = j
        .record_tool_request(run, "github.gh", digest())
        .await
        .unwrap();
    j.decide_tool_request(
        r.id,
        ToolRequestDecision::Ask,
        "ask_writes",
        Some(ordin_types::ApprovalId::new()),
    )
    .await
    .unwrap();
    let done = j.finish_tool_request(r.id, true).await.unwrap();
    assert_eq!(done.state, ToolRequestState::Executed);
    assert_eq!(j.tool_request(r.id).await.unwrap(), Some(done));
    // External actions.
    let key = ExternalActionKey::new();
    let new = NewExternalAction {
        action_key: key,
        run_id: None,
        kind: ExternalActionKind::OriginPush,
        destination: "origin".to_owned(),
    };
    j.begin_external_action(new.clone()).await.unwrap();
    assert_eq!(
        j.begin_external_action(new).await.unwrap_err().code(),
        "EXT_KEY_DUPLICATE"
    );
    assert_eq!(j.recover_external_actions().await.unwrap(), 1);
    assert_eq!(
        j.unsettled_external_actions(&[ExternalActionKind::OriginPush])
            .await
            .unwrap()
            .len(),
        1
    );
    let settled = j.mark_external_action_arrived(key).await.unwrap();
    assert_eq!(settled.state, ExternalActionState::Confirmed);
    assert_eq!(j.external_action(key).await.unwrap(), Some(settled));
    assert_eq!(
        j.list_external_actions(None, None)
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    // Artefacts.
    let location = ArtefactLocation::Place {
        place_id: PlaceId::new(),
    };
    let artefact = |c: char| NewArtefact {
        location,
        path: "out/a.md".to_owned(),
        hash: Sha256Hex(c.to_string().repeat(64)),
        media_type: "text/markdown".to_owned(),
        producing_run_id: Some(run),
    };
    j.record_artefact(artefact('1')).await.unwrap();
    assert_eq!(j.record_artefact(artefact('2')).await.unwrap().version, 2);
    assert_eq!(j.record_artefact(artefact('2')).await.unwrap().version, 2);
    assert_eq!(
        j.artefact_versions(location, "out/a.md")
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        j.list_artefacts(ArtefactFilter::default(), None)
            .await
            .unwrap()
            .items
            .len(),
        2
    );
    // Start-up reports.
    let report = StartupReport {
        id: ordin_types::StartupReportId::new(),
        subject: StartupSubject::Run { run_id: run },
        enforced_rules: vec!["no_shell".to_owned()],
        rules_not_enforced: vec![],
        unavailable_parts: vec![],
        available_tools: vec![],
        adapter_verbs_not_offered: vec![],
        generated_at: clock.now_value(),
    };
    j.store_startup_report(report.clone(), false).await.unwrap();
    assert_eq!(
        j.store_startup_report(report.clone(), false)
            .await
            .unwrap_err()
            .code(),
        "START_DUPLICATE"
    );
    j.store_startup_report(report.clone(), true).await.unwrap();
    assert_eq!(
        j.startup_report(report.subject).await.unwrap(),
        Some(report)
    );
    // Reports and dashboards.
    let ws = WorkspaceId::new();
    j.store_report(ordin_types::domain::runs::Report {
        id: ReportId::new(),
        workspace_id: ws,
        run_id: run,
        title: "Totals".to_owned(),
        as_of: clock.now_value(),
        parts: vec![ReportSection {
            heading: "Money".to_owned(),
            data: ReportData::Table {
                columns: vec![ReportTableColumn {
                    name: "Total".to_owned(),
                    column_type: ColumnType::Money,
                    currency: Some("EUR".to_owned()),
                    decimals: None,
                }],
                rows: vec![vec![CellValue::Null]],
            },
            sources: vec![ReportSource::EarlierResult { run_id: run }],
        }],
    })
    .await
    .unwrap();
    assert_eq!(j.list_reports(ws, None).await.unwrap().items.len(), 1);
    let d = j
        .keep_dashboard(NewDashboard {
            workspace_id: ws,
            name: "Totals".to_owned(),
            made_by: DashboardSource::Task {
                task_id: TaskId::new(),
            },
            inputs: vec![],
            run_id: run,
        })
        .await
        .unwrap();
    assert_eq!(j.dashboard(d.id).await.unwrap(), Some(d.clone()));
    assert_eq!(
        j.add_dashboard_refresh(d.id, run)
            .await
            .unwrap()
            .refresh_runs
            .len(),
        2
    );
    assert_eq!(j.list_dashboards(ws, None).await.unwrap().items.len(), 1);
    // Retention, pruning and erasure.
    j.set_company_max_days(Some(30)).await.unwrap();
    assert_eq!(j.retention().await.unwrap().days, 30);
    assert_eq!(
        j.set_retention_days(31).await.unwrap_err().code(),
        "JRNL_RETENTION_ABOVE_COMPANY_MAX"
    );
    j.register_prune_participant(Participant::new("approval", Ok(0)));
    clock.advance_days(400);
    let summary = j.prune(None).await.unwrap();
    assert!(summary.removed.contains_key("journal"));
    assert!(j.delete_for_runs(&[run]).await.unwrap() > 0);
    assert!(j
        .dashboard(d.id)
        .await
        .unwrap()
        .unwrap()
        .refresh_runs
        .is_empty());
    assert_eq!(j.forget_workspace(ws).await.unwrap(), 1);
    j.shutdown().await;
    j.datastore().close().await;
}

// Internal Logic 5.5: events appended by another instance reach this instance's sinks in order.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn events_of_another_instance_reach_this_instances_sinks() {
    let url = pg::migrated_database().await;
    let first = Journal::start(
        pg::open_url(url.clone()).await,
        options(TestClock::now_real()),
    )
    .await
    .unwrap();
    let second = Journal::start(pg::open_url(url).await, options(TestClock::now_real()))
        .await
        .unwrap();
    assert_eq!(first.backend_uuid(), second.backend_uuid());
    let sink = RecordingSink::new("remote_api");
    first.subscribe(sink.clone());
    second
        .append_event("run_progress", json!({}))
        .await
        .unwrap();
    first.append_event("run_progress", json!({})).await.unwrap();
    second
        .append_event("run_progress", json!({}))
        .await
        .unwrap();
    sink.wait_for(3).await;
    assert_eq!(sink.seen(), vec![1, 2, 3]);
    assert_eq!(first.last_seq(), 3);
    first.shutdown().await;
    second.shutdown().await;
}

// Scenario: Ping answers false when the datastore is unreachable
#[tokio::test]
async fn ping_answers_false_when_the_datastore_is_unreachable() {
    let (journal, _) = pg_journal().await;
    assert!(journal.ping(Duration::from_secs(2)).await);
    // The server stops answering this instance: its pool is gone.
    journal.datastore().close().await;
    let started = std::time::Instant::now();
    assert!(!journal.ping(Duration::from_secs(2)).await);
    assert!(started.elapsed() <= Duration::from_secs(2) + Duration::from_millis(500));
}
