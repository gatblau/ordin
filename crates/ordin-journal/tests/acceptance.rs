//! X2: the Gherkin of JournalAndArtefactStore.md (other than the migration scenarios, which are in
//! migrations.rs and pg_migrations.rs), one test per scenario named after it; the exact title is the
//! comment above each test. Runs on SQLite; pg_journal.rs repeats the core paths on PostgreSQL.

mod support;

use std::time::Duration;

use ordin_journal::{
    EntryFilter, JournalError, NewArtefact, NewDashboard, NewExternalAction, NewJournalEntry,
    PlannedStep,
};
use ordin_types::common::InputUsed;
use ordin_types::domain::places::ArtefactLocation;
use ordin_types::domain::runs::{
    CellValue, ChartKind, ColumnType, DashboardSource, ExternalActionKind, ExternalActionState,
    Report, ReportData, ReportKeyFigure, ReportSection, ReportSource, ReportTableColumn, RunState,
    StepMark, StepSource, ToolRequestDecision, ToolRequestState,
};
use ordin_types::num::{DecimalString, Sha256Hex};
use ordin_types::{
    AgentInstanceId, ApprovalId, CodedError, ExternalActionKey, ReportId, RunId, TaskId,
    WorkspaceId,
};
use serde_json::json;
use support::{assert_code, count, digest, exec, sqlite_journal, ts, Participant, RecordingSink};

fn hash(c: char) -> Sha256Hex {
    Sha256Hex(c.to_string().repeat(64))
}

async fn state(journal: &ordin_journal::Journal, run: RunId, state: &str) {
    journal
        .append_event("run_state_changed", json!({"run_id": run, "state": state}))
        .await
        .unwrap();
}

fn action(
    key: ExternalActionKey,
    kind: ExternalActionKind,
    run: Option<RunId>,
) -> NewExternalAction {
    NewExternalAction {
        action_key: key,
        run_id: run,
        kind,
        destination: "#team-updates".to_owned(),
    }
}

pub fn table_part(columns: Vec<ReportTableColumn>, rows: Vec<Vec<CellValue>>) -> ReportSection {
    ReportSection {
        heading: "Sales".to_owned(),
        data: ReportData::Table { columns, rows },
        sources: vec![ReportSource::EarlierResult {
            run_id: RunId::new(),
        }],
    }
}

pub fn text_column() -> ReportTableColumn {
    ReportTableColumn {
        name: "Region".to_owned(),
        column_type: ColumnType::Text,
        currency: None,
        decimals: None,
    }
}

pub fn report(run: RunId, workspace: WorkspaceId, parts: Vec<ReportSection>) -> Report {
    Report {
        id: ReportId::new(),
        workspace_id: workspace,
        run_id: run,
        title: "Weekly sales".to_owned(),
        as_of: ts("2026-01-10T09:00:00.000Z"),
        parts,
    }
}

fn steps(n: usize) -> Vec<PlannedStep> {
    (0..n)
        .map(|i| PlannedStep {
            line: format!("Step {}", i + 1),
            came_from: StepSource::AgentsPlan,
        })
        .collect()
}

// Scenario: Events get contiguous sequence numbers and reach every sink in order
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn events_get_contiguous_sequence_numbers_and_reach_every_sink_in_order() {
    let f = sqlite_journal().await;
    let bridge = RecordingSink::new("command_bridge");
    let remote = RecordingSink::new("remote_api");
    f.journal.subscribe(bridge.clone());
    f.journal.subscribe(remote.clone());
    let mut appends = Vec::new();
    for _ in 0..3 {
        let journal = f.journal.clone();
        appends.push(tokio::spawn(async move {
            journal
                .append_event("run_progress", json!({"run_id": RunId::new()}))
                .await
                .unwrap()
                .seq
        }));
    }
    let mut seqs = Vec::new();
    for append in appends {
        seqs.push(append.await.unwrap());
    }
    seqs.sort();
    assert_eq!(seqs, vec![1, 2, 3]);
    bridge.wait_for(3).await;
    remote.wait_for(3).await;
    assert_eq!(bridge.seen(), vec![1, 2, 3]);
    assert_eq!(remote.seen(), vec![1, 2, 3]);
    assert_eq!(f.journal.last_seq(), 3);
    f.journal.shutdown().await;
}

// Scenario: A slow sink is told it lagged and nothing else is affected
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_slow_sink_is_told_it_lagged_and_nothing_else_is_affected() {
    let f = sqlite_journal().await;
    let remote = RecordingSink::held("remote_api");
    let bridge = RecordingSink::new("command_bridge");
    f.journal.subscribe(remote.clone());
    f.journal.subscribe(bridge.clone());
    for _ in 0..1_100 {
        f.journal
            .append_event("block_shown", json!({"n": 1}))
            .await
            .unwrap();
    }
    bridge.wait_for(1_100).await;
    assert_eq!(bridge.seen(), (1..=1_100).collect::<Vec<u64>>());
    remote.release();
    // The first event was taken by the task before the queue filled; 1,024 more were queued.
    remote.wait_for(1_025).await;
    for _ in 0..500 {
        if !remote.lagged.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let lagged = remote.lagged.lock().unwrap().clone();
    assert_eq!(
        lagged,
        vec![1_026],
        "lagged once, with the first undelivered seq"
    );
    assert_eq!(remote.seen(), (1..=1_025).collect::<Vec<u64>>());
    // Delivery resumes with the next append.
    f.journal
        .append_event("block_shown", json!({}))
        .await
        .unwrap();
    remote.wait_for(1_026).await;
    assert_eq!(remote.seen().last(), Some(&1_101));
    let reread = f.journal.events_since(1_025, 1_000).await.unwrap();
    assert_eq!(reread.first().map(|e| e.seq), Some(1_026));
    f.journal.shutdown().await;
}

// Scenario: Resuming from a sequence older than the journal keeps
#[tokio::test]
async fn resuming_from_a_sequence_older_than_the_journal_keeps() {
    let f = sqlite_journal().await;
    let store = f.journal.datastore().clone();
    exec(
        &store,
        "WITH RECURSIVE n(seq) AS (SELECT 41201 UNION ALL SELECT seq + 1 FROM n WHERE seq < 41300)
         INSERT INTO journal_events (seq, at, kind, data)
         SELECT seq, '2026-01-10T09:41:07.312Z', 'run_progress', '{}' FROM n;
         UPDATE journal_meta SET last_seq = 41300;",
    )
    .await;
    f.journal.shutdown().await;
    let journal = ordin_journal::Journal::start(store, support::options(f.clock.clone()))
        .await
        .unwrap();
    match journal.events_since(40_000, 500).await {
        Err(JournalError::ResumeTooOld { first_kept_seq }) => assert_eq!(first_kept_seq, 41_201),
        other => panic!("{other:?}"),
    }
    let kept = journal.events_since(41_200, 500).await.unwrap();
    assert_eq!(kept.len(), 100);
    assert_eq!(kept[0].seq, 41_201);
    assert_eq!(kept[0].backend, journal.backend_uuid());
}

// Scenario: Resuming from a sequence beyond the last one
#[tokio::test]
async fn resuming_from_a_sequence_beyond_the_last_one() {
    let f = sqlite_journal().await;
    exec(
        f.journal.datastore(),
        "UPDATE journal_meta SET last_seq = 500",
    )
    .await;
    let journal = ordin_journal::Journal::start(
        f.journal.datastore().clone(),
        support::options(f.clock.clone()),
    )
    .await
    .unwrap();
    assert_eq!(journal.last_seq(), 500);
    match journal.events_since(900, 100).await {
        Err(JournalError::SeqAhead { last_seq }) => assert_eq!(last_seq, 500),
        other => panic!("{other:?}"),
    }
}

// Scenario: Oversized event data is refused without consuming a sequence number
#[tokio::test]
async fn oversized_event_data_is_refused_without_consuming_a_sequence_number() {
    let f = sqlite_journal().await;
    for _ in 0..10 {
        f.journal
            .append_event("run_progress", json!({}))
            .await
            .unwrap();
    }
    assert_eq!(f.journal.last_seq(), 10);
    let error = f
        .journal
        .append_event("run_progress", json!({"blob": "x".repeat(300_000)}))
        .await
        .unwrap_err();
    assert!(matches!(error, JournalError::EventTooLarge { bytes } if bytes > 300_000));
    let next = f
        .journal
        .append_event("run_progress", json!({}))
        .await
        .unwrap();
    assert_eq!(next.seq, 11);
}

// Scenario: An external action left unconfirmed by a crash becomes unknown outcome
#[tokio::test]
async fn an_external_action_left_unconfirmed_by_a_crash_becomes_unknown_outcome() {
    let f = sqlite_journal().await;
    let k1 = ExternalActionKey::new();
    f.journal
        .begin_external_action(action(
            k1,
            ExternalActionKind::SlackPost,
            Some(RunId::new()),
        ))
        .await
        .unwrap();
    assert_eq!(f.journal.recover_external_actions().await.unwrap(), 1);
    let stored = f.journal.external_action(k1).await.unwrap().unwrap();
    assert_eq!(stored.state, ExternalActionState::UnknownOutcome);
    let again = f
        .journal
        .begin_external_action(action(k1, ExternalActionKind::SlackPost, None))
        .await
        .unwrap_err();
    assert_code(&again, "EXT_KEY_DUPLICATE", false);
}

// Scenario: The person settles an unknown outcome
#[tokio::test]
async fn the_person_settles_an_unknown_outcome() {
    let f = sqlite_journal().await;
    let k1 = ExternalActionKey::new();
    f.journal
        .begin_external_action(action(k1, ExternalActionKind::EmailSend, None))
        .await
        .unwrap();
    f.journal.recover_external_actions().await.unwrap();
    let refused = f.journal.confirm_external_action(k1).await.unwrap_err();
    assert_code(&refused, "JRNL_EXT_STATE_INVALID", false);
    let settled = f.journal.mark_external_action_arrived(k1).await.unwrap();
    assert_eq!(settled.state, ExternalActionState::Confirmed);
    assert!(settled.confirmed_at.is_some());
    let stored = f.journal.external_action(k1).await.unwrap().unwrap();
    assert_eq!(stored, settled);
    // Confirming again is idempotent; marking arrived again is refused.
    assert_eq!(
        f.journal.confirm_external_action(k1).await.unwrap(),
        settled
    );
    assert!(matches!(
        f.journal.mark_external_action_arrived(k1).await,
        Err(JournalError::ExternalActionState)
    ));
}

// Scenario Outline: Tool request transitions
#[tokio::test]
async fn tool_request_transitions() {
    let f = sqlite_journal().await;
    let j = &f.journal;
    let run = RunId::new();
    // | requested | decide allow | state decided |
    let r = j
        .record_tool_request(run, "github.gh", digest())
        .await
        .unwrap();
    assert_eq!(r.state, ToolRequestState::Requested);
    let d = j
        .decide_tool_request(r.id, ToolRequestDecision::Allow, "allow_reads", None)
        .await
        .unwrap();
    assert_eq!(d.state, ToolRequestState::Decided);
    // | requested | finish executed | TREQ_UNDECIDED |
    let r2 = j
        .record_tool_request(run, "github.gh", digest())
        .await
        .unwrap();
    assert_code(
        &j.finish_tool_request(r2.id, true).await.unwrap_err(),
        "TREQ_UNDECIDED",
        false,
    );
    // | decided | finish executed | state executed |
    let e = j.finish_tool_request(d.id, true).await.unwrap();
    assert_eq!(e.state, ToolRequestState::Executed);
    // | executed | decide deny | TREQ_IMMUTABLE |
    let err = j
        .decide_tool_request(d.id, ToolRequestDecision::Deny, "deny_all", None)
        .await
        .unwrap_err();
    assert_code(&err, "TREQ_IMMUTABLE", false);
    // | requested | decide allow with approval | TREQ_APPROVAL_MISMATCH |
    let err = j
        .decide_tool_request(
            r2.id,
            ToolRequestDecision::Allow,
            "allow_reads",
            Some(ApprovalId::new()),
        )
        .await
        .unwrap_err();
    assert_code(&err, "TREQ_APPROVAL_MISMATCH", false);
    // An ask may carry its approval; the stored row reads back as written.
    let asked = j
        .decide_tool_request(
            r2.id,
            ToolRequestDecision::Ask,
            "ask_writes",
            Some(ApprovalId::new()),
        )
        .await
        .unwrap();
    assert_eq!(j.tool_request(r2.id).await.unwrap(), Some(asked.clone()));
    let refused = j.finish_tool_request(asked.id, false).await.unwrap();
    assert_eq!(refused.state, ToolRequestState::Refused);
}

// Scenario: An unchanged artefact does not get a new version
#[tokio::test]
async fn an_unchanged_artefact_does_not_get_a_new_version() {
    let f = sqlite_journal().await;
    let location = ArtefactLocation::AgentFolder {
        agent_instance_id: AgentInstanceId::new(),
    };
    let new = |h: Sha256Hex| NewArtefact {
        location,
        path: "out/summary.md".to_owned(),
        hash: h,
        media_type: "text/markdown".to_owned(),
        producing_run_id: Some(RunId::new()),
    };
    f.journal.record_artefact(new(hash('1'))).await.unwrap();
    let v2 = f.journal.record_artefact(new(hash('2'))).await.unwrap();
    assert_eq!(v2.version, 2);
    let same = f.journal.record_artefact(new(hash('2'))).await.unwrap();
    assert_eq!(same, v2);
    let versions = f
        .journal
        .artefact_versions(location, "out/summary.md")
        .await
        .unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(
        count(f.journal.datastore(), "SELECT COUNT(*) FROM artefacts").await,
        2
    );
}

// Scenario Outline: Report structural rules
#[tokio::test]
async fn report_structural_rules() {
    let f = sqlite_journal().await;
    let ws = WorkspaceId::new();
    let ok_part = || {
        table_part(
            vec![text_column()],
            vec![vec![CellValue::Text("North".into())]],
        )
    };
    let pie = |slices: usize| ReportSection {
        heading: "Share".to_owned(),
        data: ReportData::Chart {
            chart_kind: ChartKind::Pie,
            columns: vec![text_column()],
            rows: (0..slices).map(|_| vec![CellValue::Null]).collect(),
            x_column: "Region".to_owned(),
            series_columns: vec!["Region".to_owned()],
            x_title: "Region".to_owned(),
            y_title: "Share".to_owned(),
            bar_arrangement: None,
        },
        sources: vec![ReportSource::EarlierResult {
            run_id: RunId::new(),
        }],
    };
    let mut no_source = ok_part();
    no_source.sources.clear();
    let money = table_part(
        vec![ReportTableColumn {
            name: "Total".to_owned(),
            column_type: ColumnType::Money,
            currency: None,
            decimals: None,
        }],
        vec![],
    );
    let big = table_part(
        vec![text_column()],
        vec![vec![CellValue::Text("x".repeat(2_100_000))]],
    );
    let examples: Vec<(&str, Vec<ReportSection>, &str)> = vec![
        (
            "13 parts",
            (0..13).map(|_| ok_part()).collect(),
            "REPORT_PARTS_OUT_OF_RANGE",
        ),
        (
            "a part with no source",
            vec![no_source],
            "REPORT_PART_NO_SOURCE",
        ),
        (
            "a pie chart with 7 slices",
            vec![pie(7)],
            "REPORT_PIE_TOO_MANY_SLICES",
        ),
        (
            "a money column without currency",
            vec![money],
            "REPORT_CURRENCY_MISMATCH",
        ),
        ("2,100,000 bytes serialised", vec![big], "REPORT_TOO_LARGE"),
    ];
    for (defect, parts, code) in examples {
        let error = f
            .journal
            .store_report(report(RunId::new(), ws, parts))
            .await
            .unwrap_err();
        assert_eq!(error.code(), code, "{defect}");
        assert!(!error.retryable());
    }
    f.journal
        .store_report(report(RunId::new(), ws, vec![pie(6), ok_part()]))
        .await
        .unwrap();
}

// Scenario: A stated plan emits plan_changed and later events are seen by the current step
#[tokio::test]
async fn a_stated_plan_emits_plan_changed_and_later_events_are_seen_by_the_current_step() {
    let f = sqlite_journal().await;
    let run = RunId::new();
    assert!(f.journal.current_plan(run).await.unwrap().is_empty());
    let plan = f.journal.state_plan(run, steps(4)).await.unwrap();
    assert_eq!(plan.len(), 4);
    assert!(plan
        .iter()
        .all(|s| s.plan_version == 1 && s.mark == StepMark::Next));
    assert_eq!(
        count(f.journal.datastore(), "SELECT COUNT(*) FROM run_steps").await,
        4
    );
    let events = f.journal.events_since(0, 10).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, "plan_changed");
    assert_eq!(events[0].data["plan_version"], 1);
    assert_eq!(events[0].data["steps"].as_array().map(Vec::len), Some(4));
    let marked = f.journal.mark_step(run, 1, StepMark::Now).await.unwrap();
    assert_eq!(marked.mark, StepMark::Now);
    let step_marked = f.journal.events_since(1, 10).await.unwrap();
    assert_eq!(step_marked[0].kind, "step_marked");
    assert_eq!(
        step_marked[0].data,
        json!({"run_id": run, "plan_version": 1, "position": 1, "mark": "now"})
    );
    let seen = f
        .journal
        .append_event("run_progress", json!({"run_id": run}))
        .await
        .unwrap();
    let current = f.journal.current_plan(run).await.unwrap();
    assert!(current[0].events_seen.contains(&seen.seq));
    // A second step cannot be `now` while the first is.
    assert_code(
        &f.journal
            .mark_step(run, 2, StepMark::Now)
            .await
            .unwrap_err(),
        "RSTEP_IMMUTABLE",
        false,
    );
    let done = f.journal.mark_step(run, 1, StepMark::Done).await.unwrap();
    assert_eq!(done.marks_set.len(), 3);
}

// Scenario: A plan of an ended run cannot change
#[tokio::test]
async fn a_plan_of_an_ended_run_cannot_change() {
    let f = sqlite_journal().await;
    let run = RunId::new();
    f.journal.state_plan(run, steps(3)).await.unwrap();
    state(&f.journal, run, "running").await;
    state(&f.journal, run, "completed").await;
    let error = f
        .journal
        .mark_step(run, 1, StepMark::Now)
        .await
        .unwrap_err();
    assert_code(&error, "RSTEP_IMMUTABLE", false);
    assert_code(
        &f.journal.state_plan(run, steps(3)).await.unwrap_err(),
        "RSTEP_IMMUTABLE",
        false,
    );
}

// Scenario: History retention is refused below one day
#[tokio::test]
async fn history_retention_is_refused_below_one_day() {
    let f = sqlite_journal().await;
    let error = f.journal.set_retention_days(0).await.unwrap_err();
    assert_code(&error, "JRNL_RETENTION_INVALID", false);
    let entry = ordin_types::error_registry::lookup(error.code()).unwrap();
    assert_eq!(entry.sentence_id, "jrnl-retention-invalid");
    assert_eq!(f.journal.retention().await.unwrap().days, 90);
}

// Scenario: A lower company maximum lowers the stored value
#[tokio::test]
async fn a_lower_company_maximum_lowers_the_stored_value() {
    let f = sqlite_journal().await;
    f.journal.set_retention_days(200).await.unwrap();
    let setting = f.journal.set_company_max_days(Some(90)).await.unwrap();
    assert_eq!(setting.days, 90);
    assert_eq!(setting.company_max_days, Some(90));
    assert_eq!(f.journal.retention().await.unwrap().days, 90);
    let above = f.journal.set_retention_days(120).await.unwrap_err();
    assert!(matches!(
        above,
        JournalError::RetentionAboveCompanyMax { max_days: 90 }
    ));
    let cleared = f.journal.set_company_max_days(None).await.unwrap();
    assert_eq!((cleared.days, cleared.company_max_days), (90, None));
}

// Scenario: Pruning keeps runs in progress and calls every participant
#[tokio::test]
async fn pruning_keeps_runs_in_progress_and_calls_every_participant() {
    let f = sqlite_journal().await;
    let j = &f.journal;
    let (a, b) = (RunId::new(), RunId::new());
    f.clock.advance_days(-120);
    state(j, a, "running").await;
    state(j, b, "running").await;
    j.record_tool_request(a, "files.read", digest())
        .await
        .unwrap();
    j.record_tool_request(b, "files.read", digest())
        .await
        .unwrap();
    state(j, a, "completed").await;
    f.clock.advance_days(120);
    let approval = Participant::new("approval", Ok(2));
    let supervisor = Participant::new("session_supervisor", Ok(5));
    j.register_prune_participant(approval.clone());
    j.register_prune_participant(supervisor.clone());
    let summary = j.prune(None).await.unwrap();
    let store = j.datastore();
    let a_text = a.to_string();
    let b_text = b.to_string();
    let rows = |run: String| async move {
        let pool = store.sqlite().unwrap();
        let events: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM journal_events WHERE run_id = ?")
                .bind(&run)
                .fetch_one(pool)
                .await
                .unwrap();
        let requests: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM tool_requests WHERE run_id = ?")
                .bind(&run)
                .fetch_one(pool)
                .await
                .unwrap();
        (events, requests)
    };
    assert_eq!(rows(a_text).await, (0, 0));
    assert_eq!(rows(b_text).await, (1, 1));
    for participant in [&approval, &supervisor] {
        let requests = participant.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].runs_in_progress.contains(&b));
        assert!(!requests[0].runs_in_progress.contains(&a));
    }
    assert_eq!(summary.removed.get("approval"), Some(&2));
    assert_eq!(summary.removed.get("session_supervisor"), Some(&5));
    assert!(summary.removed["journal"] >= 3);
    assert!(j.retention().await.unwrap().last_pruned_at.is_some());
    assert_eq!(j.runs_in_progress().await.unwrap(), vec![b]);
}

// Scenario: Pruning applies the 100,000-run cap
#[tokio::test]
async fn pruning_applies_the_100000_run_cap() {
    let f = sqlite_journal().await;
    let store = f.journal.datastore();
    // 100,050 ended runs, one second apart, all within the retention period; each with one event.
    exec(
        store,
        "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i < 100049)
         INSERT INTO journal_runs (run_id, first_seen_at, state, ended_at)
         SELECT printf('00000000-0000-7000-8000-%012d', i),
                strftime('%Y-%m-%dT%H:%M:%fZ', '2026-01-10T00:00:00.000', '+' || i || ' seconds'),
                'completed',
                strftime('%Y-%m-%dT%H:%M:%fZ', '2026-01-10T00:00:00.000', '+' || i || ' seconds')
         FROM n;
         INSERT INTO journal_events (seq, at, kind, run_id, data)
         SELECT i + 1, ended_at, 'run_state_changed', run_id, '{}' FROM
           (SELECT run_id, ended_at, CAST(substr(run_id, 25) AS INTEGER) AS i FROM journal_runs);",
    )
    .await;
    f.clock.set(ts("2026-01-10T00:00:00.000Z"));
    f.journal.prune(None).await.unwrap();
    assert_eq!(
        count(store, "SELECT COUNT(*) FROM journal_runs").await,
        100_000
    );
    assert_eq!(
        count(store, "SELECT COUNT(*) FROM journal_events").await,
        100_000
    );
    assert_eq!(
        count(
            store,
            "SELECT COUNT(*) FROM journal_runs WHERE run_id < '00000000-0000-7000-8000-000000000050'"
        )
        .await,
        0,
        "the 50 oldest runs are gone"
    );
}

// Scenario: An external action is read back by its key after a crash
#[tokio::test]
async fn an_external_action_is_read_back_by_its_key_after_a_crash() {
    let f = sqlite_journal().await;
    let k2 = ExternalActionKey::new();
    f.journal
        .begin_external_action(action(
            k2,
            ExternalActionKind::TelegramPost,
            Some(RunId::new()),
        ))
        .await
        .unwrap();
    f.journal.shutdown().await;
    let journal = ordin_journal::Journal::start(
        f.journal.datastore().clone(),
        support::options(f.clock.clone()),
    )
    .await
    .unwrap();
    journal.recover_external_actions().await.unwrap();
    let read = journal.external_action(k2).await.unwrap().unwrap();
    assert_eq!(read.action_key, k2);
    assert_eq!(read.state, ExternalActionState::UnknownOutcome);
    assert_eq!(
        journal
            .external_action(ExternalActionKey::new())
            .await
            .unwrap(),
        None
    );
}

// Scenario: An external action with no run is accepted
#[tokio::test]
async fn an_external_action_with_no_run_is_accepted() {
    let f = sqlite_journal().await;
    let key = ExternalActionKey::new();
    f.journal
        .begin_external_action(action(key, ExternalActionKind::OriginPush, None))
        .await
        .unwrap();
    let stored = f.journal.external_action(key).await.unwrap().unwrap();
    assert_eq!(stored.run_id, None);
    assert_eq!(stored.state, ExternalActionState::Sent);
}

// Scenario: Unsettled actions are listed by kind for the Push Service
#[tokio::test]
async fn unsettled_actions_are_listed_by_kind_for_the_push_service() {
    let f = sqlite_journal().await;
    let j = &f.journal;
    let (a, b, c) = (
        ExternalActionKey::new(),
        ExternalActionKey::new(),
        ExternalActionKey::new(),
    );
    j.begin_external_action(action(a, ExternalActionKind::OriginPush, None))
        .await
        .unwrap();
    j.begin_external_action(action(b, ExternalActionKind::CopyPublish, None))
        .await
        .unwrap();
    j.begin_external_action(action(c, ExternalActionKind::EmailSend, None))
        .await
        .unwrap();
    j.confirm_external_action(b).await.unwrap();
    j.recover_external_actions().await.unwrap();
    let unsettled = j
        .unsettled_external_actions(&[
            ExternalActionKind::OriginPush,
            ExternalActionKind::CopyPublish,
        ])
        .await
        .unwrap();
    assert_eq!(
        unsettled.iter().map(|x| x.action_key).collect::<Vec<_>>(),
        vec![a]
    );
    assert!(j.unsettled_external_actions(&[]).await.unwrap().is_empty());
    let unknown = j
        .list_external_actions(Some(ExternalActionState::UnknownOutcome), None)
        .await
        .unwrap();
    assert_eq!(unknown.items.len(), 2);
    assert!(unknown.next_cursor.is_none());
}

// Scenario: Runs ended since a time come from the run index
#[tokio::test]
async fn runs_ended_since_a_time_come_from_the_run_index() {
    let f = sqlite_journal().await;
    let (a, b) = (RunId::new(), RunId::new());
    f.clock.set(ts("2026-01-10T07:00:00.000Z"));
    state(&f.journal, a, "running").await;
    state(&f.journal, b, "running").await;
    f.clock.set(ts("2026-01-10T08:00:00.000Z"));
    state(&f.journal, a, "completed").await;
    let ended = f
        .journal
        .runs_ended_since(ts("2026-01-10T00:00:00.000Z"), 50)
        .await
        .unwrap();
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].run_id, a);
    assert_eq!(ended[0].state, RunState::Completed);
    assert_eq!(ended[0].ended_at, Some(ts("2026-01-10T08:00:00.000Z")));
    let running = f.journal.running_runs().await.unwrap();
    assert_eq!(running.len(), 1);
    assert_eq!(running[0].run_id, b);
    assert_eq!(running[0].ended_at, None);
}

// Scenario: Erasure removes every journal row of the given runs
#[tokio::test]
async fn erasure_removes_every_journal_row_of_the_given_runs() {
    let f = sqlite_journal().await;
    let j = &f.journal;
    let (a, other) = (RunId::new(), RunId::new());
    state(j, a, "running").await;
    let other_event = j
        .append_event("run_progress", json!({"run_id": other}))
        .await
        .unwrap();
    for _ in 0..3 {
        j.append_event("run_progress", json!({"run_id": a}))
            .await
            .unwrap();
    }
    for _ in 0..2 {
        j.append_entry(NewJournalEntry {
            kind: "repair_done".to_owned(),
            component: "session_supervisor".to_owned(),
            run_id: Some(a),
            crew_run_id: None,
            workspace_id: None,
            summary: "Restarted the agent.".to_owned(),
            data: json!({}),
        })
        .await
        .unwrap();
    }
    j.record_tool_request(a, "files.read", digest())
        .await
        .unwrap();
    let key = ExternalActionKey::new();
    j.begin_external_action(action(key, ExternalActionKind::SlackPost, Some(a)))
        .await
        .unwrap();
    j.recover_external_actions().await.unwrap();
    assert_eq!(j.count_for_runs(&[a]).await.unwrap(), 9);
    assert_eq!(j.delete_for_runs(&[a]).await.unwrap(), 9);
    assert_eq!(j.count_for_runs(&[a]).await.unwrap(), 0);
    let remaining = j.events_since(0, 100).await;
    let remaining = match remaining {
        Ok(events) => events,
        // The erased run's events were the oldest; the reader resumes from what is kept.
        Err(JournalError::ResumeTooOld { first_kept_seq }) => {
            j.events_since(first_kept_seq - 1, 100).await.unwrap()
        }
        Err(other) => panic!("{other:?}"),
    };
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].seq, other_event.seq);
    assert_eq!(j.count_for_runs(&[]).await.unwrap(), 0);
    assert_eq!(j.delete_for_runs(&[]).await.unwrap(), 0);
}

// Scenario: Diagnostics lines take level and code from the entry's data
#[tokio::test]
async fn diagnostics_lines_take_level_and_code_from_the_entrys_data() {
    let f = sqlite_journal().await;
    let entry = f
        .journal
        .append_entry(NewJournalEntry {
            kind: "sign_in_failed".to_owned(),
            component: "identity".to_owned(),
            run_id: None,
            crew_run_id: None,
            workspace_id: None,
            summary: "Sign-in to GitHub failed".to_owned(),
            data: json!({"level": "error", "code": "CRED_PROVIDER_REFUSED"}),
        })
        .await
        .unwrap();
    let lines = f.journal.diagnostic_lines(None).await.unwrap();
    assert_eq!(lines.items.len(), 1);
    let line = &lines.items[0];
    assert_eq!(line.level, "error");
    assert_eq!(line.code, "CRED_PROVIDER_REFUSED");
    assert_eq!(line.reference.len(), 8);
    assert!(entry.id.0.to_string().starts_with(&line.reference));
    assert_eq!(line.summary, "Sign-in to GitHub failed");
}

// Scenario: A support export of too many runs is refused
#[tokio::test]
async fn a_support_export_of_too_many_runs_is_refused() {
    let f = sqlite_journal().await;
    let runs: Vec<RunId> = (0..21).map(|_| RunId::new()).collect();
    assert_code(
        &f.journal.support_export(&runs).await.unwrap_err(),
        "JRNL_EXPORT_INVALID",
        false,
    );
    assert_code(
        &f.journal.support_export(&[]).await.unwrap_err(),
        "JRNL_EXPORT_INVALID",
        false,
    );
}

// Scenario: Ping answers false when the datastore is unreachable (SQLite form: a closed pool;
// the PostgreSQL form is in pg_journal.rs)
#[tokio::test]
async fn ping_answers_false_when_the_datastore_is_unreachable() {
    let f = sqlite_journal().await;
    assert!(f.journal.ping(Duration::from_secs(2)).await);
    f.journal.datastore().close().await;
    let started = std::time::Instant::now();
    assert!(!f.journal.ping(Duration::from_secs(2)).await);
    assert!(started.elapsed() < Duration::from_secs(3));
}

// Scenario: One dashboard is read by id
#[tokio::test]
async fn one_dashboard_is_read_by_id() {
    let f = sqlite_journal().await;
    let (ws, run) = (WorkspaceId::new(), RunId::new());
    f.journal
        .store_report(report(
            run,
            ws,
            vec![table_part(vec![text_column()], vec![])],
        ))
        .await
        .unwrap();
    let kept = f
        .journal
        .keep_dashboard(NewDashboard {
            workspace_id: ws,
            name: "Weekly sales".to_owned(),
            made_by: DashboardSource::Task {
                task_id: TaskId::new(),
            },
            inputs: vec![InputUsed {
                name: "Sales sheet".to_owned(),
                location: "https://example.sharepoint.com/sales.xlsx".to_owned(),
                version_read: "v12".to_owned(),
            }],
            run_id: run,
        })
        .await
        .unwrap();
    let read = f.journal.dashboard(kept.id).await.unwrap().unwrap();
    assert_eq!(read.name, "Weekly sales");
    assert_eq!(read.refresh_runs, vec![run]);
    assert_eq!(read, kept);
    assert_eq!(
        f.journal
            .dashboard(ordin_types::DashboardId::new())
            .await
            .unwrap(),
        None
    );
}

// Scenario: A failing participant does not stop pruning
#[tokio::test]
async fn a_failing_participant_does_not_stop_pruning() {
    let f = sqlite_journal().await;
    let j = &f.journal;
    f.clock.advance_days(-200);
    j.append_event("run_progress", json!({})).await.unwrap();
    j.append_entry(NewJournalEntry {
        kind: "setup_check_result".to_owned(),
        component: "setup".to_owned(),
        run_id: None,
        crew_run_id: None,
        workspace_id: None,
        summary: "Checked the harness.".to_owned(),
        data: json!({}),
    })
    .await
    .unwrap();
    f.clock.advance_days(200);
    let failing = Participant::new("approval", Err("APPR_STORE_UNAVAILABLE"));
    let later = Participant::new("usage_planner", Ok(1));
    j.register_prune_participant(failing);
    j.register_prune_participant(later.clone());
    let summary = j.prune(None).await.unwrap();
    assert_eq!(
        summary.failed.get("approval").map(String::as_str),
        Some("APPR_STORE_UNAVAILABLE")
    );
    assert_eq!(summary.removed.get("usage_planner"), Some(&1));
    assert_eq!(summary.removed.get("journal"), Some(&2));
    assert_eq!(later.requests.lock().unwrap().len(), 1);
    assert_eq!(
        count(j.datastore(), "SELECT COUNT(*) FROM journal_entries").await,
        0
    );
}

// ---- Further behaviour of Internal Logic not named by a scenario.

#[tokio::test]
async fn entries_and_artefacts_page_one_hundred_at_a_time() {
    let f = sqlite_journal().await;
    let run = RunId::new();
    for i in 0..150 {
        f.clock.advance_ms(1);
        f.journal
            .append_entry(NewJournalEntry {
                kind: "repair_done".to_owned(),
                component: "rules".to_owned(),
                run_id: (i % 2 == 0).then_some(run),
                crew_run_id: None,
                workspace_id: Some(WorkspaceId::new()),
                summary: format!("Entry {i}"),
                data: json!({"i": i}),
            })
            .await
            .unwrap();
    }
    let first = f
        .journal
        .list_entries(EntryFilter::default(), None)
        .await
        .unwrap();
    assert_eq!(first.items.len(), 100);
    assert_eq!(first.items[0].summary, "Entry 149");
    let second = f
        .journal
        .list_entries(EntryFilter::default(), first.next_cursor.clone())
        .await
        .unwrap();
    assert_eq!(second.items.len(), 50);
    assert!(second.next_cursor.is_none());
    let filtered = f
        .journal
        .list_entries(
            EntryFilter {
                run_id: Some(run),
                kind: Some("repair_done".to_owned()),
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(filtered.items.len(), 75);
    let location = ArtefactLocation::Place {
        place_id: ordin_types::PlaceId::new(),
    };
    for i in 0..101 {
        f.clock.advance_ms(1);
        f.journal
            .record_artefact(NewArtefact {
                location,
                path: format!("report-{i}.md"),
                hash: hash('a'),
                media_type: "text/markdown".to_owned(),
                producing_run_id: None,
            })
            .await
            .unwrap();
    }
    let page = f
        .journal
        .list_artefacts(
            ordin_journal::ArtefactFilter {
                location: Some(location),
                producing_run_id: None,
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(page.items.len(), 100);
    let rest = f
        .journal
        .list_artefacts(ordin_journal::ArtefactFilter::default(), page.next_cursor)
        .await
        .unwrap();
    assert_eq!(rest.items.len(), 1);
}

#[tokio::test]
async fn reports_dashboards_and_workspaces_follow_their_rules() {
    let f = sqlite_journal().await;
    let j = &f.journal;
    let ws = WorkspaceId::new();
    let (r1, r2) = (RunId::new(), RunId::new());
    let figures = ReportSection {
        heading: "Totals".to_owned(),
        data: ReportData::KeyFigures {
            figures: vec![ReportKeyFigure {
                label: "Orders".to_owned(),
                value: DecimalString("12".to_owned()),
                unit: "".to_owned(),
                change_against: None,
            }],
        },
        sources: vec![ReportSource::CompanySystem {
            system: "erp".to_owned(),
            read_at: ts("2026-01-10T09:00:00.000Z"),
        }],
    };
    let first = report(r1, ws, vec![figures.clone()]);
    j.store_report(first.clone()).await.unwrap();
    j.store_report(report(r2, ws, vec![figures])).await.unwrap();
    assert_eq!(j.report(first.id).await.unwrap(), first);
    assert_eq!(j.report_for_run(r1).await.unwrap(), Some(first.clone()));
    assert!(matches!(
        j.report(ReportId::new()).await,
        Err(JournalError::NotFound)
    ));
    assert_eq!(j.list_reports(ws, None).await.unwrap().items.len(), 2);
    let new = |name: &str, run| NewDashboard {
        workspace_id: ws,
        name: name.to_owned(),
        made_by: DashboardSource::Workflow {
            workflow_id: ordin_types::WorkflowId::new(),
            version: 2,
        },
        inputs: vec![],
        run_id: run,
    };
    let d = j.keep_dashboard(new("Sales", r1)).await.unwrap();
    assert_code(
        &j.keep_dashboard(new("Sales", r2)).await.unwrap_err(),
        "REPORT_DASHBOARD_NAME_DUPLICATE",
        false,
    );
    assert!(matches!(
        j.keep_dashboard(new("Other", RunId::new())).await,
        Err(JournalError::NotFound)
    ));
    let refreshed = j.add_dashboard_refresh(d.id, r2).await.unwrap();
    assert_eq!(refreshed.refresh_runs, vec![r1, r2]);
    assert_eq!(j.list_dashboards(ws, None).await.unwrap().items.len(), 1);
    j.remove_dashboard(d.id).await.unwrap();
    assert!(matches!(
        j.remove_dashboard(d.id).await,
        Err(JournalError::NotFound)
    ));
    assert_eq!(j.forget_workspace(ws).await.unwrap(), 2);
}

#[tokio::test]
async fn startup_reports_support_export_compaction_and_shutdown() {
    let f = sqlite_journal().await;
    let j = &f.journal;
    let run = RunId::new();
    let report = ordin_types::domain::runs::StartupReport {
        id: ordin_types::StartupReportId::new(),
        subject: ordin_types::domain::runs::StartupSubject::Run { run_id: run },
        enforced_rules: vec!["no_shell".to_owned()],
        rules_not_enforced: vec![ordin_types::domain::runs::RuleNotEnforced {
            rule: "budget".to_owned(),
            reason: "No budget was set.".to_owned(),
        }],
        unavailable_parts: vec![],
        available_tools: vec!["files.read".to_owned()],
        adapter_verbs_not_offered: vec![],
        generated_at: ts("2026-01-10T09:00:00.000Z"),
    };
    j.store_startup_report(report.clone(), false).await.unwrap();
    assert_code(
        &j.store_startup_report(report.clone(), false)
            .await
            .unwrap_err(),
        "START_DUPLICATE",
        false,
    );
    j.store_startup_report(report.clone(), true).await.unwrap();
    assert_eq!(
        j.startup_report(report.subject).await.unwrap(),
        Some(report)
    );
    state(j, run, "running").await;
    j.state_plan(run, steps(3)).await.unwrap();
    j.record_tool_request(run, "files.read", digest())
        .await
        .unwrap();
    j.begin_external_action(action(
        ExternalActionKey::new(),
        ExternalActionKind::SlackPost,
        Some(run),
    ))
    .await
    .unwrap();
    let bundle = j.support_export(&[run]).await.unwrap();
    let text = String::from_utf8(bundle.bytes).unwrap();
    let lines: Vec<serde_json::Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[0]["format"], "ordin-support");
    assert!(!bundle.truncated);
    assert!(bundle.suggested_file_name.starts_with("ordin-support-"));
    assert!(!text.contains("#team-updates"), "no destination");
    assert!(!text.contains("Step 1"), "no step line");
    assert!(lines.iter().any(|l| l["record"] == "startup_report"));
    j.compact().await.unwrap();
    let sink = RecordingSink::new("command_bridge");
    let id = j.subscribe(sink.clone());
    j.append_event("run_progress", json!({})).await.unwrap();
    sink.wait_for(1).await;
    j.unsubscribe(id);
    j.shutdown().await;
    j.datastore().close().await;
}
