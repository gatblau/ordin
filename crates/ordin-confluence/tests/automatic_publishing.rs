//! X2 for ConfluenceAdapter.md, amendment 2026-01-24: automatic publishing off where an agent that
//! can run commands may change the folder, and changed files through **Publish checked items…**
//! (SA-CONF-AUTO-01 to 04). One test per scenario, of the same name.

mod support;

use ordin_confluence::{AutomaticPublishing, ConfluenceAdapter, ReadyKind};
use ordin_journal::{
    ApplyMode, Datastore, DatastoreConfig, EntryFilter, JournalEvent, MigrationRegistry,
    ModuleMigrations, MIGRATIONS,
};
use ordin_types::domain::places::{ConfluenceLinkState, FileVersionOrigin, PublishedCopyState};
use ordin_types::time::Timestamp;
use ordin_types::{ApprovalId, RunId};
use serde_json::json;
use sqlx::Row;
use support::*;

const PAGE: &str = "4411";

fn page_path() -> String {
    api(&format!("/api/v2/pages/{PAGE}"))
}

async fn decide(env: &Env, approval: ApprovalId, state: &str) {
    env.adapter
        .event_sink()
        .deliver(&JournalEvent {
            backend: uuid::Uuid::now_v7(),
            seq: 1,
            at: Timestamp::now(),
            kind: "approval_decided".to_owned(),
            data: json!({"approval_id": approval, "state": state, "decided_on": "here"}),
        })
        .await;
}

async fn held_reasons(env: &Env) -> Vec<String> {
    env.journal
        .list_entries(
            EntryFilter {
                run_id: None,
                kind: Some("confluence_item_held".to_owned()),
            },
            None,
        )
        .await
        .unwrap()
        .items
        .iter()
        .map(|e| e.data["reason"].as_str().unwrap_or("").to_owned())
        .collect()
}

/// A current page copy of `rates.md` at page version 7, then the person's own new version.
async fn persons_edit(
    env: &Env,
    link: &ordin_types::domain::places::ConfluenceLink,
) -> ordin_types::PublishedCopyId {
    env.version("rates.md", "# Rates", FileVersionOrigin::Person)
        .await;
    let old = env.hash_of("rates.md").await;
    let copy = env.page_copy(link, "rates.md", PAGE, "7", &old).await;
    env.version("rates.md", "# New rates", FileVersionOrigin::Person)
        .await;
    copy.id
}

// Scenario: Error path — with automatic publishing off, the person's own edit is not pushed (SA-CONF-AUTO-02)
#[tokio::test]
async fn error_path_with_automatic_publishing_off_the_persons_own_edit_is_not_pushed() {
    let env = Env::new().await;
    let link = env.link(ConfluenceLinkState::Active);
    let copy = persons_edit(&env, &link).await;
    mount_page(&env, PAGE, "rates", 7).await;
    mount_put(&env, PAGE, "rates", 8).await;

    let report = env
        .adapter
        .sync_linked_folder_with(&link, &env.place, AutomaticPublishing::Off)
        .await
        .unwrap();

    assert_eq!(report.pushed, 0);
    assert_eq!(report.waiting_for_approval, 1);
    assert!(env.writes().await.is_empty(), "nothing is pushed");
    assert_eq!(env.copy(copy).await.state, PublishedCopyState::OutOfDate);
    let ready = env.adapter.ready_to_publish(link.id).await.unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].kind, ReadyKind::Changed);
    assert_eq!(ready[0].path, "rates.md");
    assert_eq!(ready[0].published_copy_id, Some(copy));
}

// Scenario: Happy path — with automatic publishing on, the person's own edit is still pushed
#[tokio::test]
async fn happy_path_with_automatic_publishing_on_the_persons_own_edit_is_still_pushed() {
    let env = Env::new().await;
    let link = env.link(ConfluenceLinkState::Active);
    let copy = persons_edit(&env, &link).await;
    let new = env.hash_of("rates.md").await;
    mount_page(&env, PAGE, "rates", 7).await;
    mount_put(&env, PAGE, "rates", 8).await;

    let report = env
        .adapter
        .sync_linked_folder_with(&link, &env.place, AutomaticPublishing::On)
        .await
        .unwrap();

    assert_eq!(report.pushed, 1);
    let copy = env.copy(copy).await;
    assert_eq!(copy.state, PublishedCopyState::Current);
    assert_eq!(
        copy.original_version_published.as_deref(),
        Some(new.as_str())
    );
    assert_eq!(copy.destination_version_pushed.as_deref(), Some("8"));
}

// Scenario: Happy path — a changed file goes to Confluence through the card (SA-CONF-AUTO-04)
#[tokio::test]
async fn happy_path_a_changed_file_goes_to_confluence_through_the_card() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    let copy = persons_edit(&env, &link).await;
    let new = env.hash_of("rates.md").await;
    env.adapter
        .sync_linked_folder_with(&link, &env.place, AutomaticPublishing::Off)
        .await
        .unwrap();
    let items = env.adapter.ready_to_publish(link.id).await.unwrap();
    let approval = env
        .adapter
        .request_publish_checked(&link, &env.place, items)
        .await
        .unwrap();
    let pending = env.approvals.list_pending(None, None).await.unwrap();
    let card = pending
        .items
        .iter()
        .find(|r| r.card.approval_id == approval)
        .expect("the card");
    assert_eq!(card.card.changes, "0 new, 1 changed, 0 renamed, 0 removed.");
    mount_page(&env, PAGE, "rates", 7).await;
    mount_put(&env, PAGE, "rates", 8).await;

    decide(&env, approval, "approved").await;

    assert_eq!(env.count("PUT", &page_path()).await, 1);
    let body = env.body_of("PUT", &page_path()).await;
    assert!(
        body["body"]["value"]
            .as_str()
            .unwrap_or("")
            .contains("New rates"),
        "the checked bytes are sent"
    );
    let copy = env.copy(copy).await;
    assert_eq!(copy.state, PublishedCopyState::Current);
    assert_eq!(
        copy.original_version_published.as_deref(),
        Some(new.as_str())
    );
    assert_eq!(copy.destination_version_pushed.as_deref(), Some("8"));
    assert!(env
        .adapter
        .ready_to_publish(link.id)
        .await
        .unwrap()
        .is_empty());
}

// Scenario: Error path — a changed file edited after the card is held
#[tokio::test]
async fn error_path_a_changed_file_edited_after_the_card_is_held() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    let copy = persons_edit(&env, &link).await;
    env.adapter
        .sync_linked_folder_with(&link, &env.place, AutomaticPublishing::Off)
        .await
        .unwrap();
    let items = env.adapter.ready_to_publish(link.id).await.unwrap();
    let approval = env
        .adapter
        .request_publish_checked(&link, &env.place, items)
        .await
        .unwrap();
    env.version("rates.md", "# Rates again", FileVersionOrigin::Person)
        .await;
    mount_page(&env, PAGE, "rates", 7).await;
    mount_put(&env, PAGE, "rates", 8).await;

    decide(&env, approval, "approved").await;

    assert!(env.writes().await.is_empty(), "nothing is sent");
    assert_eq!(env.copy(copy).await.state, PublishedCopyState::OutOfDate);
    assert_eq!(held_reasons(&env).await, ["changed_since_card"]);
}

// Scenario: Error path — a page changed in Confluence is not overwritten from the card
#[tokio::test]
async fn error_path_a_page_changed_in_confluence_is_not_overwritten_from_the_card() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    let copy = persons_edit(&env, &link).await;
    env.adapter
        .sync_linked_folder_with(&link, &env.place, AutomaticPublishing::Off)
        .await
        .unwrap();
    let items = env.adapter.ready_to_publish(link.id).await.unwrap();
    let approval = env
        .adapter
        .request_publish_checked(&link, &env.place, items)
        .await
        .unwrap();
    mount_page(&env, PAGE, "rates", 9).await;
    mount_put(&env, PAGE, "rates", 10).await;

    decide(&env, approval, "approved").await;

    assert!(env.writes().await.is_empty(), "nothing is sent");
    assert_eq!(
        env.copy(copy).await.state,
        PublishedCopyState::ChangedAtDestination
    );
    assert_eq!(held_reasons(&env).await, ["changed_in_confluence"]);
}

// Scenario: Edge case — an agent's change is listed as changed
#[tokio::test]
async fn edge_case_an_agents_change_is_listed_as_changed() {
    let env = Env::new().await;
    let link = env.link(ConfluenceLinkState::Active);
    env.version("rates.md", "# Rates", FileVersionOrigin::Person)
        .await;
    let old = env.hash_of("rates.md").await;
    let copy = env.page_copy(&link, "rates.md", PAGE, "7", &old).await;
    env.version(
        "rates.md",
        "# Agent rates",
        FileVersionOrigin::Run {
            run_id: RunId::new(),
            tool: "write_file".to_owned(),
        },
    )
    .await;

    let report = env
        .adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();

    assert_eq!(report.waiting_for_approval, 1);
    assert_eq!(env.copy(copy.id).await.state, PublishedCopyState::OutOfDate);
    let ready = env.adapter.ready_to_publish(link.id).await.unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].kind, ReadyKind::Changed);
}

// Scenario: Edge case — migration step 2 keeps every Ready item
#[tokio::test]
async fn edge_case_migration_step_2_keeps_every_ready_item() {
    let dir = TempDir::new();
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.path().join("journal.db"),
        backup_dir: dir.path().join("backups"),
    })
    .await
    .unwrap();
    let all = ConfluenceAdapter::migrations();
    let mut first = MigrationRegistry::new();
    first.register(MIGRATIONS).unwrap();
    first
        .register(ModuleMigrations {
            module: all.module,
            sqlite: &all.sqlite[..1],
            postgres: &[],
        })
        .unwrap();
    first.apply(&store, ApplyMode::ApplyPending).await.unwrap();
    let pool = store.sqlite().cloned().unwrap();
    let link = ordin_types::ConfluenceLinkId::new().to_string();
    for (kind, path) in [("new", "a.md"), ("renamed", "b.md"), ("removed", "c.md")] {
        sqlx::query(
            "INSERT INTO confluence_ready_items (confluence_link_id, kind, path, from_path, published_copy_id, found_at) \
             VALUES (?1, ?2, ?3, NULL, NULL, '2026-01-21T10:00:00Z')",
        )
        .bind(&link)
        .bind(kind)
        .bind(path)
        .execute(&pool)
        .await
        .unwrap();
    }
    let mut all_steps = MigrationRegistry::new();
    all_steps.register(MIGRATIONS).unwrap();
    all_steps.register(ConfluenceAdapter::migrations()).unwrap();
    all_steps
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .expect("step 2 applies");
    let rows = sqlx::query("SELECT kind, path FROM confluence_ready_items ORDER BY path")
        .fetch_all(&pool)
        .await
        .unwrap();
    let read: Vec<(String, String)> = rows.iter().map(|r| (r.get(0), r.get(1))).collect();
    assert_eq!(
        read,
        [
            ("new".to_owned(), "a.md".to_owned()),
            ("renamed".to_owned(), "b.md".to_owned()),
            ("removed".to_owned(), "c.md".to_owned())
        ]
    );
    let broken = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(
        broken.is_empty(),
        "PRAGMA foreign_key_check returns nothing"
    );
    sqlx::query(
        "INSERT INTO confluence_ready_items (confluence_link_id, kind, path, from_path, published_copy_id, found_at) \
         VALUES (?1, 'changed', 'd.md', NULL, NULL, '2026-01-21T10:00:00Z')",
    )
    .bind(&link)
    .execute(&pool)
    .await
    .expect("a changed item can be stored");
}

/// The person's edit of `rates.md` after a published copy, whose push then failed (step 7.6's moves).
async fn failed_after_edit(
    env: &Env,
    link: &ordin_types::domain::places::ConfluenceLink,
) -> ordin_types::PublishedCopyId {
    let copy = persons_edit(env, link).await;
    for (state, reason) in [
        (PublishedCopyState::OutOfDate, None),
        (PublishedCopyState::Updating, None),
        (
            PublishedCopyState::Failed,
            Some("ordin couldn't publish this to Confluence (CONF_UNREACHABLE).".to_owned()),
        ),
    ] {
        env.files
            .set_published_copy_state(copy, state, reason)
            .await
            .expect("a state move");
    }
    copy
}

// Scenario: Error path — a failed copy of a changed file goes back on Ready to publish (SA-CONF-AUTO-05)
#[tokio::test]
async fn error_path_a_failed_copy_of_a_changed_file_goes_back_on_ready_to_publish() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    let copy = failed_after_edit(&env, &link).await;
    let new = env.hash_of("rates.md").await;
    mount_page(&env, PAGE, "rates", 7).await;
    mount_put(&env, PAGE, "rates", 8).await;

    env.adapter
        .sync_linked_folder_with(&link, &env.place, AutomaticPublishing::On)
        .await
        .unwrap();

    assert!(env.writes().await.is_empty(), "nothing is sent by the sync");
    assert_eq!(env.copy(copy).await.state, PublishedCopyState::Failed);
    let items = env.adapter.ready_to_publish(link.id).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, ReadyKind::Changed);
    assert_eq!(items[0].path, "rates.md");
    assert_eq!(items[0].published_copy_id, Some(copy));

    let approval = env
        .adapter
        .request_publish_checked(&link, &env.place, items)
        .await
        .unwrap();
    decide(&env, approval, "approved").await;

    assert_eq!(env.count("PUT", &page_path()).await, 1);
    let body = env.body_of("PUT", &page_path()).await;
    assert!(
        body["body"]["value"]
            .as_str()
            .unwrap_or("")
            .contains("New rates"),
        "the checked bytes are sent"
    );
    let copy = env.copy(copy).await;
    assert_eq!(copy.state, PublishedCopyState::Current);
    assert_eq!(
        copy.original_version_published.as_deref(),
        Some(new.as_str())
    );
}

// Scenario: Error path — a copy whose first push failed goes back on Ready to publish (SA-CONF-AUTO-05)
#[tokio::test]
async fn error_path_a_copy_whose_first_push_failed_goes_back_on_ready_to_publish() {
    let env = Env::new().await;
    let link = env.link(ConfluenceLinkState::Active);
    env.version("rates.md", "# Rates", FileVersionOrigin::Person)
        .await;
    use ordin_types::domain::places::{
        CopyDestination, CopyFormat, CopyMaker, CopyOriginal, PublishedCopy, PublishedVia,
    };
    let copy = env
        .files
        .record_published_copy(PublishedCopy {
            id: ordin_types::PublishedCopyId::new(),
            original: CopyOriginal::PlaceFile {
                place_id: env.place.id,
                path: "rates.md".to_owned(),
            },
            destination: CopyDestination::ConfluencePage {
                space: "PROC".to_owned(),
                parent_page_id: PARENT.to_owned(),
            },
            destination_item_id: None,
            format: CopyFormat::Page,
            maker: CopyMaker::Ordin,
            published_via: PublishedVia::ConfluenceLink {
                confluence_link_id: link.id,
            },
            followed_branch: None,
            keep_up_to_date: true,
            audience: link.audience.clone(),
            original_version_published: None,
            destination_version_pushed: None,
            last_updated_at: None,
            state: PublishedCopyState::Failed,
            failure_reason: Some(
                "ordin couldn't publish this to Confluence (CONF_ACCESS_DENIED).".to_owned(),
            ),
        })
        .await
        .expect("a failed copy")
        .id;

    env.adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();

    assert!(env.writes().await.is_empty(), "nothing is sent by the sync");
    assert_eq!(env.copy(copy).await.state, PublishedCopyState::Failed);
    let ready = env.adapter.ready_to_publish(link.id).await.unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].kind, ReadyKind::New);
    assert_eq!(ready[0].published_copy_id, Some(copy));
}

// Scenario: Error path — a failed copy whose page changed in Confluence is not overwritten from the
// card (SA-CONF-AUTO-05)
#[tokio::test]
async fn error_path_a_failed_copy_whose_page_changed_in_confluence_is_not_overwritten_from_the_card(
) {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    let copy = failed_after_edit(&env, &link).await;
    env.adapter
        .sync_linked_folder_with(&link, &env.place, AutomaticPublishing::Off)
        .await
        .unwrap();
    let items = env.adapter.ready_to_publish(link.id).await.unwrap();
    assert_eq!(items.len(), 1);
    let approval = env
        .adapter
        .request_publish_checked(&link, &env.place, items)
        .await
        .unwrap();
    mount_page(&env, PAGE, "rates", 9).await;
    mount_put(&env, PAGE, "rates", 10).await;

    decide(&env, approval, "approved").await;

    assert!(env.writes().await.is_empty(), "nothing is sent");
    assert_eq!(env.copy(copy).await.state, PublishedCopyState::Failed);
    assert_eq!(held_reasons(&env).await, ["changed_in_confluence"]);
}
