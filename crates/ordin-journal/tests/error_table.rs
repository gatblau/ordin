//! X1: one test per row of JournalAndArtefactStore.md's *Error Table*, provoking the condition and
//! asserting the code, the retryability and the repair action. No row states an HTTP status for
//! this crate's own calls; the cursor row's Remote API 422 is the Service API's check.

mod support;

use std::time::Duration;

use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, JournalError, MigrationRegistry, MigrationStep,
    ModuleMigrations, NewArtefact, NewExternalAction, NewJournalEntry, MIGRATIONS,
};
use ordin_types::domain::places::ArtefactLocation;
use ordin_types::domain::runs::{ExternalActionKind, StepMark, ToolRequestDecision};
use ordin_types::num::Sha256Hex;
use ordin_types::{
    AgentInstanceId, CodedError, Cursor, ExternalActionKey, RepairAction, RunId, ToolRequestId,
};
use serde_json::json;
use support::{exec, migrate, open_sqlite, sqlite_journal, Participant, TempDir};

fn check(error: &JournalError, code: &str, retryable: bool, repair: Option<RepairAction>) {
    assert_eq!(error.code(), code);
    assert_eq!(error.retryable(), retryable, "{code}");
    assert_eq!(error.repair(), repair, "{code}");
    assert_eq!(error.to_string(), code, "Display is the code only");
}

fn entry(summary: &str) -> NewJournalEntry {
    NewJournalEntry {
        kind: "repair_done".to_owned(),
        component: "rules".to_owned(),
        run_id: None,
        crew_run_id: None,
        workspace_id: None,
        summary: summary.to_owned(),
        data: json!({}),
    }
}

// Row: SQLite file fails `PRAGMA quick_check`, or cannot be created or opened.
#[tokio::test]
async fn datastore_open_failed() {
    let dir = TempDir::new();
    std::fs::create_dir_all(dir.db_file().parent().unwrap()).unwrap();
    std::fs::write(dir.db_file(), vec![7u8; 8192]).unwrap();
    let error = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.db_file(),
        backup_dir: dir.backup_dir(),
    })
    .await
    .err()
    .expect("a file that is not a database is refused");
    check(
        &error,
        "JRNL_DATASTORE_OPEN_FAILED",
        false,
        Some(RepairAction::SendToIt),
    );
}

// Row: a datastore call fails (connection lost, disk I/O, lock timeout).
#[tokio::test]
async fn datastore_unavailable() {
    let f = sqlite_journal().await;
    f.journal.datastore().close().await;
    let error = f.journal.append_entry(entry("Done.")).await.unwrap_err();
    check(
        &error,
        "JRNL_DATASTORE_UNAVAILABLE",
        true,
        Some(RepairAction::TryAgain),
    );
    let error = f
        .journal
        .append_event("run_progress", json!({}))
        .await
        .unwrap_err();
    check(
        &error,
        "JRNL_DATASTORE_UNAVAILABLE",
        true,
        Some(RepairAction::TryAgain),
    );
    // PostgreSQL refusing the connection is the same, retryable code.
    let refused = Datastore::open(DatastoreConfig::Postgres {
        url: ordin_journal::DatabaseUrl::new("postgres://u:p@127.0.0.1:1/db".to_owned()),
        max_connections: 1,
    })
    .await
    .err()
    .unwrap();
    check(
        &refused,
        "JRNL_DATASTORE_UNAVAILABLE",
        true,
        Some(RepairAction::TryAgain),
    );
    let url = ordin_journal::DatabaseUrl::new("postgres://secret@host/db".to_owned());
    assert_eq!(
        format!("{url:?} {url}"),
        "DatabaseUrl(<redacted>) DatabaseUrl(<redacted>)"
    );
}

// Row: database written by a newer ordin.
#[tokio::test]
async fn schema_newer() {
    let dir = TempDir::new();
    let store = open_sqlite(&dir).await;
    migrate(&store).await;
    let error = MigrationRegistry::new()
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap_err();
    check(
        &error,
        "JRNL_SCHEMA_NEWER",
        false,
        Some(RepairAction::SendToIt),
    );
}

// Row: scaled-shape instance started on a schema with pending steps.
#[tokio::test]
async fn schema_behind() {
    let dir = TempDir::new();
    let store = open_sqlite(&dir).await;
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    let error = registry
        .apply(&store, ApplyMode::CheckOnly)
        .await
        .unwrap_err();
    check(&error, "JRNL_SCHEMA_BEHIND", false, None);
}

// Row: a module registered twice, or versions not 1, 2, 3 ….
#[test]
fn migration_duplicate() {
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    let error = registry.register(MIGRATIONS).unwrap_err();
    check(&error, "JRNL_MIGRATION_DUPLICATE", false, None);
    const GAP: &[MigrationStep] = &[(2, "two", "SELECT 1")];
    let error = registry
        .register(ModuleMigrations {
            module: "approval",
            sqlite: GAP,
            postgres: &[],
        })
        .unwrap_err();
    check(&error, "JRNL_MIGRATION_DUPLICATE", false, None);
}

// Row: an applied step's SQL differs from the registered step.
#[tokio::test]
async fn migration_changed() {
    let dir = TempDir::new();
    let store = open_sqlite(&dir).await;
    migrate(&store).await;
    exec(
        &store,
        "UPDATE schema_migrations SET checksum = replace(checksum, substr(checksum, 1, 1), 'x')",
    )
    .await;
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    let error = registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap_err();
    check(
        &error,
        "JRNL_MIGRATION_CHANGED",
        false,
        Some(RepairAction::SendToIt),
    );
}

// Row: a migration step fails; transaction rolled back.
#[tokio::test]
async fn migration_failed() {
    let dir = TempDir::new();
    let store = open_sqlite(&dir).await;
    const BAD: &[MigrationStep] = &[(1, "bad", "CREATE TABLE (")];
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    registry
        .register(ModuleMigrations {
            module: "approval",
            sqlite: BAD,
            postgres: &[],
        })
        .unwrap();
    let error = registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap_err();
    check(
        &error,
        "JRNL_MIGRATION_FAILED",
        false,
        Some(RepairAction::SendToIt),
    );
    assert!(
        support::applied(&store).await.is_empty(),
        "the journal's step rolled back too"
    );
}

// Row: backup copy before migrating cannot be written.
#[tokio::test]
async fn backup_failed() {
    let dir = TempDir::new();
    let blocker = dir.path().join("not-a-folder");
    std::fs::write(&blocker, b"x").unwrap();
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.db_file(),
        backup_dir: blocker.join("backups"),
    })
    .await
    .unwrap();
    migrate(&store).await;
    const MORE: &[MigrationStep] = &[(1, "t", "CREATE TABLE t (x INTEGER)")];
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    registry
        .register(ModuleMigrations {
            module: "approval",
            sqlite: MORE,
            postgres: &[],
        })
        .unwrap();
    let error = registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap_err();
    check(
        &error,
        "JRNL_BACKUP_FAILED",
        false,
        Some(RepairAction::TryAgain),
    );
    assert_eq!(
        support::applied(&store).await.len(),
        1,
        "nothing is migrated"
    );
}

// Row: event kind malformed, `reset`, or `data` not an object.
#[tokio::test]
async fn event_kind_invalid() {
    let f = sqlite_journal().await;
    for (kind, data) in [
        ("Run-State", json!({})),
        ("reset", json!({})),
        ("run_progress", json!([1, 2])),
    ] {
        let error = f.journal.append_event(kind, data).await.unwrap_err();
        check(&error, "JRNL_EVENT_KIND_INVALID", false, None);
    }
    assert_eq!(f.journal.last_seq(), 0);
}

// Row: event data larger than 256 KiB.
#[tokio::test]
async fn event_too_large() {
    let f = sqlite_journal().await;
    let error = f
        .journal
        .append_event("block_shown", json!({"b": "x".repeat(262_144)}))
        .await
        .unwrap_err();
    check(&error, "JRNL_EVENT_TOO_LARGE", false, None);
}

// Row: journal entry fails its rules.
#[tokio::test]
async fn entry_invalid() {
    let f = sqlite_journal().await;
    let mut bad_kind = entry("Done.");
    bad_kind.kind = "Bad Kind".to_owned();
    let mut bad_component = entry("Done.");
    bad_component.component = "rules-engine".to_owned();
    let mut big = entry("Done.");
    big.data = json!({"x": "y".repeat(65_536)});
    for bad in [
        entry(""),
        entry(&"x".repeat(301)),
        entry("bell\u{7}"),
        bad_kind,
        bad_component,
        big,
    ] {
        let error = f.journal.append_entry(bad).await.unwrap_err();
        check(&error, "JRNL_ENTRY_INVALID", false, None);
    }
}

// Row: `events_since` asked for a sequence older than kept.
#[tokio::test]
async fn resume_too_old() {
    let f = sqlite_journal().await;
    for _ in 0..3 {
        f.journal
            .append_event("run_progress", json!({}))
            .await
            .unwrap();
    }
    exec(
        f.journal.datastore(),
        "DELETE FROM journal_events WHERE seq < 3",
    )
    .await;
    let error = f.journal.events_since(0, 10).await.unwrap_err();
    check(&error, "JRNL_RESUME_TOO_OLD", false, None);
}

// Row: `events_since` asked for a sequence beyond `last_seq`.
#[tokio::test]
async fn seq_ahead() {
    let f = sqlite_journal().await;
    let error = f.journal.events_since(1, 10).await.unwrap_err();
    check(&error, "JRNL_SEQ_AHEAD", false, None);
}

// Row: history days < 1.
#[tokio::test]
async fn retention_invalid() {
    let f = sqlite_journal().await;
    check(
        &f.journal.set_retention_days(0).await.unwrap_err(),
        "JRNL_RETENTION_INVALID",
        false,
        None,
    );
    check(
        &f.journal.set_company_max_days(Some(0)).await.unwrap_err(),
        "JRNL_RETENTION_INVALID",
        false,
        None,
    );
}

// Row: history days above the company maximum.
#[tokio::test]
async fn retention_above_company_max() {
    let f = sqlite_journal().await;
    f.journal.set_company_max_days(Some(365)).await.unwrap();
    let error = f.journal.set_retention_days(400).await.unwrap_err();
    check(&error, "JRNL_RETENTION_ABOVE_COMPANY_MAX", false, None);
    assert!(matches!(
        error,
        JournalError::RetentionAboveCompanyMax { max_days: 365 }
    ));
}

// Row: confirm or mark-arrived on an external action in the wrong state.
#[tokio::test]
async fn ext_state_invalid() {
    let f = sqlite_journal().await;
    let key = ExternalActionKey::new();
    f.journal
        .begin_external_action(NewExternalAction {
            action_key: key,
            run_id: None,
            kind: ExternalActionKind::OriginPush,
            destination: "origin".to_owned(),
        })
        .await
        .unwrap();
    let error = f
        .journal
        .mark_external_action_arrived(key)
        .await
        .unwrap_err();
    check(&error, "JRNL_EXT_STATE_INVALID", false, None);
}

// Row: record, report, dashboard or plan not found.
#[tokio::test]
async fn not_found_and_plan_not_found() {
    let f = sqlite_journal().await;
    let j = &f.journal;
    check(
        &j.decide_tool_request(ToolRequestId::new(), ToolRequestDecision::Allow, "r", None)
            .await
            .unwrap_err(),
        "JRNL_NOT_FOUND",
        false,
        None,
    );
    check(
        &j.confirm_external_action(ExternalActionKey::new())
            .await
            .unwrap_err(),
        "JRNL_NOT_FOUND",
        false,
        None,
    );
    check(
        &j.add_dashboard_refresh(ordin_types::DashboardId::new(), RunId::new())
            .await
            .unwrap_err(),
        "JRNL_NOT_FOUND",
        false,
        None,
    );
    check(
        &j.mark_step(RunId::new(), 1, StepMark::Now)
            .await
            .unwrap_err(),
        "JRNL_PLAN_NOT_FOUND",
        false,
        None,
    );
}

// Row: malformed or over-long cursor.
#[tokio::test]
async fn bad_cursor() {
    let f = sqlite_journal().await;
    let tampered: Cursor = serde_json::from_value(json!("not-a-cursor!")).unwrap();
    let error = f
        .journal
        .list_entries(Default::default(), Some(tampered))
        .await
        .unwrap_err();
    check(&error, "SRV_BAD_CURSOR", false, None);
    let long: Cursor = serde_json::from_value(json!("A".repeat(1_025))).unwrap();
    let error = f.journal.diagnostic_lines(Some(long)).await.unwrap_err();
    check(&error, "SRV_BAD_CURSOR", false, None);
}

// Row: a 2C entity rule fails (artefact, tool request, external action, start-up report, report,
// run step).
#[tokio::test]
async fn entity_rule_fails_with_its_2c_code() {
    let f = sqlite_journal().await;
    let j = &f.journal;
    let location = ArtefactLocation::AgentFolder {
        agent_instance_id: AgentInstanceId::new(),
    };
    let artefact = |path: &str, media: &str, hash: &str| NewArtefact {
        location,
        path: path.to_owned(),
        hash: Sha256Hex(hash.to_owned()),
        media_type: media.to_owned(),
        producing_run_id: None,
    };
    let good = "a".repeat(64);
    check(
        &j.record_artefact(artefact("../x", "text/plain", &good))
            .await
            .unwrap_err(),
        "ART_PATH_INVALID",
        false,
        None,
    );
    check(
        &j.record_artefact(artefact("x", "plain", &good))
            .await
            .unwrap_err(),
        "ART_MEDIA_TYPE_INVALID",
        false,
        None,
    );
    check(
        &j.record_artefact(artefact("x", "text/plain", "XYZ"))
            .await
            .unwrap_err(),
        "HASH_NOT_SHA256_HEX",
        false,
        None,
    );
    check(
        &j.record_tool_request(RunId::new(), "", support::digest())
            .await
            .unwrap_err(),
        "TREQ_NAME_INVALID",
        false,
        None,
    );
    check(
        &j.begin_external_action(NewExternalAction {
            action_key: ExternalActionKey::new(),
            run_id: None,
            kind: ExternalActionKind::EmailSend,
            destination: String::new(),
        })
        .await
        .unwrap_err(),
        "EXT_DESTINATION_INVALID",
        false,
        None,
    );
    let mut report = ordin_types::domain::runs::StartupReport {
        id: ordin_types::StartupReportId::new(),
        subject: ordin_types::domain::runs::StartupSubject::Run {
            run_id: RunId::new(),
        },
        enforced_rules: vec!["x".repeat(501)],
        rules_not_enforced: vec![],
        unavailable_parts: vec![],
        available_tools: vec![],
        adapter_verbs_not_offered: vec![],
        generated_at: ordin_types::time::Timestamp::now(),
    };
    check(
        &j.store_startup_report(report.clone(), false)
            .await
            .unwrap_err(),
        "START_LIST_TOO_LONG",
        false,
        None,
    );
    report.enforced_rules.clear();
    report.rules_not_enforced = vec![ordin_types::domain::runs::RuleNotEnforced {
        rule: "budget".to_owned(),
        reason: String::new(),
    }];
    check(
        &j.store_startup_report(report, false).await.unwrap_err(),
        "START_REASON_MISSING",
        false,
        None,
    );
    let steps = |n: usize, line: &str| {
        (0..n)
            .map(|_| ordin_journal::PlannedStep {
                line: line.to_owned(),
                came_from: ordin_types::domain::runs::StepSource::AgentsPlan,
            })
            .collect::<Vec<_>>()
    };
    check(
        &j.state_plan(RunId::new(), steps(2, "a")).await.unwrap_err(),
        "RSTEP_PLAN_SIZE_OUT_OF_RANGE",
        false,
        None,
    );
    check(
        &j.state_plan(RunId::new(), steps(3, &"a".repeat(81)))
            .await
            .unwrap_err(),
        "RSTEP_LINE_TOO_LONG",
        false,
        None,
    );
    let run = RunId::new();
    for _ in 0..20 {
        j.state_plan(run, steps(3, "a")).await.unwrap();
    }
    check(
        &j.state_plan(run, steps(3, "a")).await.unwrap_err(),
        "RSTEP_PLAN_VERSION_GAP",
        false,
        None,
    );
}

// Row: a prune participant fails.
#[tokio::test]
async fn prune_participant_fails() {
    let f = sqlite_journal().await;
    f.journal
        .register_prune_participant(Participant::new("approval", Err("APPR_STORE_UNAVAILABLE")));
    let summary = f.journal.prune(None).await.unwrap();
    assert_eq!(summary.failed["approval"], "APPR_STORE_UNAVAILABLE");
}

// Row: support export asked for no run or more than 20 runs.
#[tokio::test]
async fn export_invalid() {
    let f = sqlite_journal().await;
    let error = f.journal.support_export(&[]).await.unwrap_err();
    check(&error, "JRNL_EXPORT_INVALID", false, None);
}

// Row: `count_for_runs` / `delete_for_runs` given more than 500 run ids.
#[tokio::test]
async fn too_many_run_ids_for_erasure() {
    let f = sqlite_journal().await;
    let runs: Vec<RunId> = (0..501).map(|_| RunId::new()).collect();
    check(
        &f.journal.count_for_runs(&runs).await.unwrap_err(),
        "JRNL_ENTRY_INVALID",
        false,
        None,
    );
    check(
        &f.journal.delete_for_runs(&runs).await.unwrap_err(),
        "JRNL_ENTRY_INVALID",
        false,
        None,
    );
}

// Row: `ping` gets no answer within its bound (returns false, no error).
#[tokio::test]
async fn ping_without_answer_returns_false() {
    let f = sqlite_journal().await;
    f.journal.datastore().close().await;
    assert!(!f.journal.ping(Duration::from_millis(500)).await);
}
