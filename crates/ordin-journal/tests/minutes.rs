//! Minutes kept by Ordin (JournalAndArtefactStore.md JA-MIN-1 to JA-MIN-3) on SQLite: the four
//! scenarios of the amendment's Gherkin, one test each named after it, and its Error Table rows.
//! pg_minutes.rs repeats the paths on PostgreSQL.

mod support;

use ordin_journal::{
    ApplyMode, Datastore, Journal, JournalError, MigrationRegistry, MinutesUpdate,
    ModuleMigrations, NewMinutes, MIGRATIONS,
};
use ordin_types::wire::minutes::MinutesSource;
use ordin_types::{MinutesId, RecordingId, RunId, WorkspaceId};
use support::{
    applied, assert_code, count, exec, open_sqlite, options, sqlite_journal, TempDir, TestClock,
};

pub fn new_minutes(workspace: WorkspaceId, title: &str) -> NewMinutes {
    NewMinutes {
        workspace_id: workspace,
        title: title.to_owned(),
        source: MinutesSource::TranscriptFile,
        source_name: "meeting.vtt".to_owned(),
        body: "## Summary\n\nWe agreed the supplier terms.\n".to_owned(),
        recording: None,
        run_id: Some(RunId::new()),
    }
}

async fn workspace_changed(store: &Datastore) -> i64 {
    count(
        store,
        "SELECT COUNT(*) FROM journal_events WHERE kind = 'workspace_changed'",
    )
    .await
}

async fn minutes_rows(store: &Datastore) -> i64 {
    count(store, "SELECT COUNT(*) FROM minutes").await
}

// Scenario: Happy path — kept, listed, edited
#[tokio::test]
async fn happy_path_kept_listed_edited() {
    let f = sqlite_journal().await;
    let j = &f.journal;
    let ws = WorkspaceId::new();
    let older = j
        .keep_minutes(new_minutes(ws, "Earlier call"))
        .await
        .unwrap();
    f.clock.advance_ms(5);
    let kept = j
        .keep_minutes(new_minutes(ws, "Supplier call"))
        .await
        .unwrap();
    assert_eq!(kept.revision, 1);
    assert_eq!(kept.edited_at, None);
    assert_eq!(kept.id.0.get_version_num(), 7);
    assert_eq!(workspace_changed(j.datastore()).await, 2);

    let page = j.list_minutes(ws, None).await.unwrap();
    let titles: Vec<&str> = page.items.iter().map(|m| m.title.as_str()).collect();
    assert_eq!(titles, ["Supplier call", "Earlier call"]);
    assert_eq!(page.items[0].id, kept.id);
    assert_eq!(page.items[1].id, older.id);
    assert!(page.next_cursor.is_none());
    assert!(j
        .list_minutes(WorkspaceId::new(), None)
        .await
        .unwrap()
        .items
        .is_empty());

    f.clock.advance_ms(5);
    let MinutesUpdate::Updated(edited) = j
        .update_minutes_body(kept.id, "## Summary\n\nChanged.\n".to_owned(), 1)
        .await
        .unwrap()
    else {
        panic!("not updated");
    };
    assert_eq!(edited.revision, 2);
    assert_eq!(edited.edited_at, Some(f.clock.now_value()));
    assert_eq!(j.minutes(kept.id).await.unwrap(), Some(edited));
    assert_eq!(workspace_changed(j.datastore()).await, 3);
}

// Scenario: Edge — a stale edit
#[tokio::test]
async fn edge_a_stale_edit() {
    let f = sqlite_journal().await;
    let j = &f.journal;
    let kept = j
        .keep_minutes(new_minutes(WorkspaceId::new(), "Supplier call"))
        .await
        .unwrap();
    j.update_minutes_body(kept.id, "Second".to_owned(), 1)
        .await
        .unwrap();
    let answer = j
        .update_minutes_body(kept.id, "Third".to_owned(), 1)
        .await
        .unwrap();
    assert_eq!(
        answer,
        MinutesUpdate::Stale {
            current_revision: 2
        }
    );
    let stored = j.minutes(kept.id).await.unwrap().unwrap();
    assert_eq!(stored.body, "Second");
    assert_eq!(stored.revision, 2);
    assert_eq!(
        j.update_minutes_body(MinutesId::new(), "Body".to_owned(), 1)
            .await
            .unwrap(),
        MinutesUpdate::NotFound
    );
}

// Scenario: Error — a title over 120 characters
#[tokio::test]
async fn error_a_title_over_120_characters() {
    let f = sqlite_journal().await;
    let j = &f.journal;
    let error = j
        .keep_minutes(new_minutes(WorkspaceId::new(), &"t".repeat(121)))
        .await
        .unwrap_err();
    assert_code(&error, "MINUTES_TITLE_INVALID", false);
    assert_eq!(minutes_rows(j.datastore()).await, 0);
    assert_eq!(workspace_changed(j.datastore()).await, 0);
    // 120 characters, counted as characters rather than bytes, is kept.
    j.keep_minutes(new_minutes(WorkspaceId::new(), &"é".repeat(120)))
        .await
        .unwrap();
}

// Scenario: Edge — migration step 4 on an existing database
#[tokio::test]
async fn edge_migration_step_4_on_an_existing_database() {
    let dir = TempDir::new();
    let store = open_sqlite(&dir).await;
    let mut registry = MigrationRegistry::new();
    registry
        .register(ModuleMigrations {
            module: "journal",
            sqlite: &MIGRATIONS.sqlite[..3],
            postgres: &MIGRATIONS.postgres[..3],
        })
        .unwrap();
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    exec(
        &store,
        "INSERT INTO dashboards (id, workspace_id, name, made_by, inputs, refresh_runs, kept_at) \
         VALUES ('0192f0e0-0000-7000-8000-000000000001', '0192f0e0-0000-7000-8000-000000000002', \
         'Weekly sales', '{\"kind\":\"conversation\"}', '[]', \
         '[\"0192f0e0-0000-7000-8000-000000000003\"]', '2026-01-10T09:00:00.000Z')",
    )
    .await;
    let before = count(
        &store,
        "SELECT COUNT(*) FROM dashboards WHERE name = 'Weekly sales' AND kept_at = '2026-01-10T09:00:00.000Z'",
    )
    .await;
    assert_eq!(before, 1);

    support::migrate(&store).await;

    let versions: Vec<i64> = applied(&store).await.into_iter().map(|(_, v)| v).collect();
    assert_eq!(versions, [1, 2, 3, 4]);
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'minutes'"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'minutes_workspace'"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &store,
            "SELECT COUNT(*) FROM dashboards WHERE name = 'Weekly sales' AND kept_at = '2026-01-10T09:00:00.000Z'",
        )
        .await,
        1,
        "the dashboards are unchanged"
    );
    let journal = Journal::start(store, options(TestClock::now_real()))
        .await
        .unwrap();
    let ws = WorkspaceId::new();
    journal
        .keep_minutes(new_minutes(ws, "After the upgrade"))
        .await
        .unwrap();
    assert_eq!(journal.list_minutes(ws, None).await.unwrap().items.len(), 1);
}

// Error Table: title, body or source name outside its rule → Err(permanent), nothing kept.
#[tokio::test]
async fn each_field_outside_its_rule_is_refused_with_its_code() {
    let f = sqlite_journal().await;
    let j = &f.journal;
    let ws = WorkspaceId::new();
    let cases: Vec<(&str, NewMinutes)> = vec![
        ("MINUTES_TITLE_INVALID", new_minutes(ws, "")),
        ("MINUTES_TITLE_INVALID", new_minutes(ws, "Line\nbreak")),
        (
            "MINUTES_BODY_INVALID",
            NewMinutes {
                body: String::new(),
                ..new_minutes(ws, "T")
            },
        ),
        (
            "MINUTES_BODY_INVALID",
            NewMinutes {
                body: "a".repeat(1_048_577),
                ..new_minutes(ws, "T")
            },
        ),
        (
            "MINUTES_SOURCE_NAME_INVALID",
            NewMinutes {
                source_name: String::new(),
                ..new_minutes(ws, "T")
            },
        ),
        (
            "MINUTES_SOURCE_NAME_INVALID",
            NewMinutes {
                source_name: "x".repeat(256),
                ..new_minutes(ws, "T")
            },
        ),
        (
            "MINUTES_SOURCE_NAME_INVALID",
            NewMinutes {
                source_name: "calls/meeting.vtt".to_owned(),
                ..new_minutes(ws, "T")
            },
        ),
        (
            "MINUTES_SOURCE_NAME_INVALID",
            NewMinutes {
                source_name: "calls\\meeting.vtt".to_owned(),
                ..new_minutes(ws, "T")
            },
        ),
    ];
    for (code, new) in cases {
        let error = j.keep_minutes(new).await.unwrap_err();
        assert_code(&error, code, false);
        assert!(matches!(error, JournalError::Invalid { .. }));
    }
    // The limits themselves are kept: a 1 MiB body and a 255-character source name.
    j.keep_minutes(NewMinutes {
        body: "a".repeat(1_048_576),
        source_name: "x".repeat(255),
        ..new_minutes(ws, "T")
    })
    .await
    .unwrap();
    assert_eq!(minutes_rows(j.datastore()).await, 1);
    // An edit is held to the body rule too, and writes nothing.
    let kept = j.list_minutes(ws, None).await.unwrap().items[0].id;
    let error = j
        .update_minutes_body(kept, String::new(), 1)
        .await
        .unwrap_err();
    assert_code(&error, "MINUTES_BODY_INVALID", false);
    assert_eq!(j.minutes(kept).await.unwrap().unwrap().revision, 1);
}

// Error Table: `set_minutes_recording` for an unknown id → JRNL_NOT_FOUND.
#[tokio::test]
async fn the_recording_is_set_and_cleared_and_an_unknown_id_is_not_found() {
    let f = sqlite_journal().await;
    let j = &f.journal;
    let kept = j
        .keep_minutes(NewMinutes {
            source: MinutesSource::Recording,
            source_name: "2026-01-31 10-00-00.mkv".to_owned(),
            ..new_minutes(WorkspaceId::new(), "Recorded call")
        })
        .await
        .unwrap();
    let recording = RecordingId::new();
    let with = j
        .set_minutes_recording(kept.id, Some(recording))
        .await
        .unwrap();
    assert_eq!(with.recording, Some(recording));
    assert_eq!(
        j.minutes(kept.id).await.unwrap().unwrap().recording,
        Some(recording)
    );
    let without = j.set_minutes_recording(kept.id, None).await.unwrap();
    assert_eq!(without.recording, None);
    assert_eq!(
        j.minutes(kept.id).await.unwrap().unwrap().source,
        MinutesSource::Recording
    );
    let error = j
        .set_minutes_recording(MinutesId::new(), None)
        .await
        .unwrap_err();
    assert_code(&error, "JRNL_NOT_FOUND", false);
}

// JA-MIN-2 `remove_minutes` (Ok when absent) and JA-MIN-3 erasure with the workspace's other rows.
#[tokio::test]
async fn minutes_are_removed_alone_and_with_their_workspace() {
    let f = sqlite_journal().await;
    let j = &f.journal;
    let ws = WorkspaceId::new();
    let other = WorkspaceId::new();
    let one = j.keep_minutes(new_minutes(ws, "One")).await.unwrap();
    j.keep_minutes(new_minutes(ws, "Two")).await.unwrap();
    j.keep_minutes(new_minutes(ws, "Three")).await.unwrap();
    j.keep_minutes(new_minutes(other, "Elsewhere"))
        .await
        .unwrap();

    j.remove_minutes(one.id).await.unwrap();
    assert_eq!(j.minutes(one.id).await.unwrap(), None);
    let events = workspace_changed(j.datastore()).await;
    j.remove_minutes(one.id).await.unwrap();
    assert_eq!(
        workspace_changed(j.datastore()).await,
        events,
        "nothing removed, no event"
    );

    assert_eq!(j.remove_workspace_minutes(ws).await.unwrap(), 2);
    assert_eq!(j.remove_workspace_minutes(ws).await.unwrap(), 0);
    assert_eq!(j.list_minutes(other, None).await.unwrap().items.len(), 1);
    assert_eq!(j.forget_workspace(other).await.unwrap(), 1);
    assert_eq!(minutes_rows(j.datastore()).await, 0);
}

// JA-MIN-2: 50 a page, newest first, and the cursor continues where the page ended.
#[tokio::test]
async fn the_list_is_paged_by_fifty() {
    let f = sqlite_journal().await;
    let j = &f.journal;
    let ws = WorkspaceId::new();
    for n in 0..51 {
        f.clock.advance_ms(1);
        j.keep_minutes(new_minutes(ws, &format!("Call {n}")))
            .await
            .unwrap();
    }
    let first = j.list_minutes(ws, None).await.unwrap();
    assert_eq!(first.items.len(), 50);
    assert_eq!(first.items[0].title, "Call 50");
    let second = j.list_minutes(ws, first.next_cursor).await.unwrap();
    assert_eq!(second.items.len(), 1);
    assert_eq!(second.items[0].title, "Call 0");
    assert!(second.next_cursor.is_none());
}
