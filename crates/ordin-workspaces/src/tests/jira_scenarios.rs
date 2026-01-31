//! X2 for the Jira amendment (ADR-106, §J): one test per added scenario, named after it, and the
//! rows of its Error Table that need a Jira stand-in.

use ordin_files::PlaceChangeKind;
use ordin_types::domain::places::{FolderAccess, Grantee, SourceLinkState, WorkingCopyState};
use ordin_types::time::Timestamp;
use ordin_types::AgentInstanceId;

use super::support::*;
use crate::{LinkLocation, WorkspaceError};

const ADDRESS: &str = "https://acme.atlassian.net/browse/ABC-12";

fn issue_location() -> LinkLocation {
    LinkLocation::JiraIssue {
        address: ADDRESS.to_owned(),
    }
}

fn two_comments() -> Vec<serde_json::Value> {
    vec![
        jira_comment("10300", "2026-01-10T09:30:00.000+0000", adf("First remark")),
        jira_comment(
            "10301",
            "2026-01-10T09:45:00.000+0000",
            adf("Second remark"),
        ),
    ]
}

fn code<T: std::fmt::Debug>(result: Result<T, WorkspaceError>) -> &'static str {
    result.expect_err("an error").code()
}

fn read_text(env: &Env, link: crate::SourceLinkId, relative: &str) -> String {
    let path = env.service.working_copy_dir(link).join(relative);
    std::fs::read_to_string(path).expect("the item is in the working copy")
}

// Scenario: Happy path — linking a Jira issue makes read-only items
#[tokio::test]
async fn happy_path_linking_a_jira_issue_makes_read_only_items() {
    let env = Env::new().await;
    mount_jira_issue(
        &env,
        abc12(
            "Assess the Northwind account.",
            "2026-01-11T10:30:00.000+0000",
        ),
        two_comments(),
    )
    .await;
    let ws = env.create_workspace("A").await;
    let link = env
        .service
        .add_link(ws.id, issue_location(), None)
        .await
        .unwrap();
    assert_eq!(link.state, SourceLinkState::Active);
    let items = env.service.working_copy_items(link.id).await.unwrap();
    let paths: Vec<&str> = items.iter().map(|i| i.origin_path.as_str()).collect();
    assert_eq!(
        paths,
        vec![
            "attachments/10001-rates.xlsx",
            "comments/20260110T093000Z-10300.md",
            "comments/20260110T094500Z-10301.md",
            "related.md",
            "summary-and-description.md",
        ]
    );
    assert!(items.iter().all(|i| i.state == WorkingCopyState::Read
        && i.local_version == 1
        && i.origin_newer_at.is_none()));
    let version = |path: &str| {
        items
            .iter()
            .find(|i| i.origin_path == path)
            .unwrap()
            .origin_version_read
            .clone()
    };
    assert_eq!(
        version("summary-and-description.md"),
        "issue:2026-01-11T10:30:00.000Z"
    );
    assert_eq!(
        version("comments/20260110T093000Z-10300.md"),
        "comment:2026-01-10T09:30:00.000Z"
    );
    assert_eq!(version("attachments/10001-rates.xlsx"), "attachment:10001");
    let description = read_text(&env, link.id, "summary-and-description.md");
    assert!(description.starts_with("# ABC-12 Northwind assessment\nStatus: In Progress · Assignee: Sam Lee\n\nAssess the Northwind account."));
    assert!(
        read_text(&env, link.id, "comments/20260110T093000Z-10300.md")
            .starts_with("**Sam Lee** · 2026-01-10T09:30:00.000Z\n\nFirst remark")
    );
    assert_eq!(
        std::fs::read(
            env.service
                .working_copy_dir(link.id)
                .join("attachments/10001-rates.xlsx")
        )
        .unwrap(),
        b"rates bytes"
    );
    let related = read_text(&env, link.id, "related.md");
    assert!(related.contains("## Sub-tasks\nABC-13 Collect references — To Do"));
    assert!(related.contains("## Linked issues\nNone"));
    // Only reads, and none for the sub-task.
    let requests = env.jira_requests().await;
    assert!(requests.iter().all(|(m, _)| m == "GET"));
    assert!(requests.iter().all(|(_, p)| !p.contains("ABC-13")));
    assert_eq!(
        env.service.source_links(ws.id).await.unwrap()[0].display_name,
        "ABC-12 Northwind assessment"
    );
    assert_eq!(env.entries("source_link_read").await.len(), 1);
    // The temporary download is gone.
    assert_eq!(
        std::fs::read_dir(env.data_root.join("tmp").join("jira"))
            .map(|d| d.count())
            .unwrap_or(0),
        0
    );
}

// Scenario: Edge case — handover record comments are not items
#[tokio::test]
async fn edge_case_handover_record_comments_are_not_items() {
    let env = Env::new().await;
    let comments = vec![
        jira_comment(
            "20001",
            "2026-01-10T09:00:00.000+0000",
            adf_with_code(
                "Please review the rates.",
                "# ordin.handover/v1 record\nid: h-1",
            ),
        ),
        jira_comment(
            "20002",
            "2026-01-10T09:10:00.000+0000",
            adf_with_code("started", "# ordin.handover/v1 event\nkind: started"),
        ),
        jira_comment(
            "20003",
            "2026-01-10T09:20:00.000+0000",
            adf_with_code("done", "# ordin.handover/v1 event\nkind: done"),
        ),
    ];
    mount_jira_issue(
        &env,
        abc12("Assess.", "2026-01-11T10:30:00.000+0000"),
        comments,
    )
    .await;
    let ws = env.create_workspace("A").await;
    let link = env
        .service
        .add_link(ws.id, issue_location(), None)
        .await
        .unwrap();
    let items = env.service.working_copy_items(link.id).await.unwrap();
    let comment_items: Vec<&str> = items
        .iter()
        .filter(|i| i.origin_path.starts_with("comments/"))
        .map(|i| i.origin_path.as_str())
        .collect();
    assert_eq!(comment_items, vec!["comments/20260110T090000Z-20001.md"]);
    let note = read_text(&env, link.id, "comments/20260110T090000Z-20001.md");
    assert!(note.contains("Please review the rates."));
    assert!(!note.contains("ordin.handover"));
    assert!(!note.contains("id: h-1"));
}

// Scenario: Happy path — an origin change is shown and nothing local is overwritten
#[tokio::test]
async fn happy_path_an_origin_change_is_shown_and_nothing_local_is_overwritten() {
    let env = Env::new().await;
    mount_jira_issue(
        &env,
        abc12(
            "Assess the Northwind account.",
            "2026-01-10T09:00:00.000+0000",
        ),
        two_comments(),
    )
    .await;
    let ws = env.create_workspace("A").await;
    let link = env
        .service
        .add_link(ws.id, issue_location(), None)
        .await
        .unwrap();
    let before = read_text(&env, link.id, "summary-and-description.md");
    let mut changes = env.service.subscribe_origin_changes();
    env.mock.reset().await;
    mount_jira_issue(
        &env,
        abc12(
            "Assess the Northwind account again.",
            "2026-01-12T08:00:00.000+0000",
        ),
        two_comments(),
    )
    .await;
    let report = env.service.open(ws.id).await.unwrap();
    assert!(report.origin_changes >= 1);
    let item = env
        .service
        .working_copy_items(link.id)
        .await
        .unwrap()
        .into_iter()
        .find(|i| i.origin_path == "summary-and-description.md")
        .unwrap();
    assert!(item.origin_newer_at.is_some());
    assert_eq!(item.origin_version_read, "issue:2026-01-10T09:00:00.000Z");
    assert_eq!(
        read_text(&env, link.id, "summary-and-description.md"),
        before
    );
    let mut heard = Vec::new();
    while let Ok(change) = changes.try_recv() {
        heard.push(change);
    }
    let summary = heard
        .iter()
        .find(|c| c.origin_path == "summary-and-description.md")
        .expect("OriginChanged is sent");
    assert_eq!(summary.version.version, "issue:2026-01-12T08:00:00.000Z");
    assert!(!summary.removed);
    // The same version is not announced again at the next check.
    let again = env.service.open(ws.id).await.unwrap();
    assert_eq!(again.origin_changes, 0);
    // Bring in the new version.
    let updated = env
        .service
        .read_item(link.id, "summary-and-description.md")
        .await
        .unwrap();
    assert!(updated.origin_newer_at.is_none());
    assert_eq!(
        updated.origin_version_read,
        "issue:2026-01-12T08:00:00.000Z"
    );
    assert!(read_text(&env, link.id, "summary-and-description.md")
        .contains("Assess the Northwind account again."));
    assert_eq!(env.entries("jira_item_brought_up_to_date").await.len(), 1);
}

// Scenario: Happy path — removing the link removes the local copy only
#[tokio::test]
async fn happy_path_removing_the_link_removes_the_local_copy_only() {
    let env = Env::new().await;
    mount_jira_issue(
        &env,
        abc12("Assess.", "2026-01-11T10:30:00.000+0000"),
        two_comments(),
    )
    .await;
    let ws = env.create_workspace("A").await;
    let link = env
        .service
        .add_link(ws.id, issue_location(), None)
        .await
        .unwrap();
    let place = link.place_id.unwrap();
    let dir = env.service.working_copy_dir(link.id);
    assert!(dir.is_dir());
    let requests_before = env.requests().await.len();
    env.service.remove_link(link.id).await.unwrap();
    assert_eq!(
        env.scalar("SELECT count(*) FROM working_copy_items").await,
        0
    );
    assert!(env.files.place(place).await.is_err());
    assert!(!dir.exists());
    assert_eq!(env.requests().await.len(), requests_before);
}

// Scenario: Edge case — an agent's edit of a Jira item is not sent back
#[tokio::test]
async fn edge_case_an_agents_edit_of_a_jira_item_is_not_sent_back() {
    let env = Env::new().await;
    mount_jira_issue(
        &env,
        abc12("Assess.", "2026-01-11T10:30:00.000+0000"),
        two_comments(),
    )
    .await;
    let ws = env.create_workspace("A").await;
    let link = env
        .service
        .add_link(ws.id, issue_location(), None)
        .await
        .unwrap();
    let mut heard = env.service.subscribe_working_copy_changes();
    env.service
        .handle_place_change(ordin_files::PlaceChange {
            place_id: link.place_id.unwrap(),
            path: "summary-and-description.md".to_owned(),
            change: PlaceChangeKind::Modified,
            observed_at: Timestamp::now(),
            by_ordin: true,
            version: Some(2),
            origin: None,
        })
        .await;
    let item = env
        .service
        .working_copy_items(link.id)
        .await
        .unwrap()
        .into_iter()
        .find(|i| i.origin_path == "summary-and-description.md")
        .unwrap();
    assert_eq!(
        (item.state, item.local_version),
        (WorkingCopyState::Read, 1)
    );
    assert!(matches!(
        heard.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
}

// Scenario: Error path — a Jira link on the personal server
#[tokio::test]
async fn error_path_a_jira_link_on_the_personal_server() {
    let env = Env::with(Options {
        jira: false,
        ..Options::default()
    })
    .await;
    let ws = env.create_workspace("A").await;
    assert_eq!(
        code(
            env.service
                .propose_link(ws.id, issue_location(), None)
                .await
        ),
        "WS_JIRA_UNAVAILABLE"
    );
    assert_eq!(env.scalar("SELECT count(*) FROM source_links").await, 0);
}

// Scenario: Error path — a pasted address that is not an issue
#[tokio::test]
async fn error_path_a_pasted_address_that_is_not_an_issue() {
    let env = Env::new().await;
    let ws = env.create_workspace("A").await;
    let location = LinkLocation::JiraIssue {
        address: "https://acme.atlassian.net/projects/ABC".to_owned(),
    };
    assert_eq!(
        code(env.service.propose_link(ws.id, location, None).await),
        "WS_JIRA_ADDRESS_INVALID"
    );
    assert_eq!(env.scalar("SELECT count(*) FROM source_links").await, 0);
}

// Scenario: Error path — Jira not connected
#[tokio::test]
async fn error_path_jira_not_connected() {
    let env = Env::with(Options {
        jira_token: false,
        ..Options::default()
    })
    .await;
    mount_jira_issue(
        &env,
        abc12("Assess.", "2026-01-11T10:30:00.000+0000"),
        two_comments(),
    )
    .await;
    let ws = env.create_workspace("A").await;
    let outcome = env.service.add_link(ws.id, issue_location(), None).await;
    assert_eq!(code(outcome), "WS_SOURCE_SIGN_IN_NEEDED");
    let links = env.service.source_links(ws.id).await.unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].state, SourceLinkState::Proposed);
    // Nothing is half-added: no working-copy place, folder or item.
    assert!(links[0].place_id.is_none());
    assert_eq!(env.service.places(ws.id).await.unwrap().len(), 1);
    assert_eq!(
        env.scalar("SELECT count(*) FROM working_copy_items").await,
        0
    );
    assert!(!env.service.working_copy_dir(links[0].id).exists());
}

// WS-ADD-1: found in the Phase 5 walk on Windows — after connecting Jira, adding the issue again was
// refused "That's already linked", and the link stayed proposed with nothing to confirm it.
// spec: ordin-desktop/components/WorkspaceService#WS-ADD-1
#[tokio::test]
async fn edge_case_adding_again_after_connecting_jira_confirms_the_waiting_link() {
    let env = Env::with(Options {
        jira_token: false,
        ..Options::default()
    })
    .await;
    mount_jira_issue(
        &env,
        abc12("Assess.", "2026-01-11T10:30:00.000+0000"),
        two_comments(),
    )
    .await;
    let ws = env.create_workspace("A").await;
    let first = env.service.add_link(ws.id, issue_location(), None).await;
    assert_eq!(code(first), "WS_SOURCE_SIGN_IN_NEEDED");
    env.identity
        .save_jira_token(
            JIRA_SITE,
            JIRA_EMAIL,
            ordin_identity::SecretString::new(JIRA_TOKEN.to_owned()),
            "712020:7d2a-priya",
            "Priya Shah",
        )
        .await
        .unwrap();
    let link = env
        .service
        .add_link(ws.id, issue_location(), None)
        .await
        .expect("the waiting link is confirmed");
    assert_eq!(link.state, SourceLinkState::Active);
    let links = env.service.source_links(ws.id).await.unwrap();
    assert_eq!(links.len(), 1, "one link, not a second");
    assert_eq!(links[0].id, link.id);
}

// Scenario: Edge case — the migration keeps every link and item
#[tokio::test]
async fn edge_case_the_migration_keeps_every_link_and_item() {
    use ordin_journal::{
        ApplyMode, Datastore, DatastoreConfig, MigrationRegistry, ModuleMigrations, MIGRATIONS,
    };
    let dir = TempDir::new();
    let store = Datastore::open(DatastoreConfig::Sqlite {
        path: dir.path().join("journal.db"),
        backup_dir: dir.path().join("backups"),
    })
    .await
    .unwrap();
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    registry
        .register(ModuleMigrations {
            module: crate::MODULE_NAME,
            sqlite: &crate::SQLITE_MIGRATIONS[..2],
            postgres: &[],
        })
        .unwrap();
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    let pool = store.sqlite().unwrap().clone();
    // A journal database at workspaces step 2 with 3 source links, 40 items and 2 unread rows.
    let ws = ordin_types::WorkspaceId::new();
    let now = Timestamp::now().to_wire();
    sqlx::query("INSERT INTO workspaces (id, name, name_key, own_folder_place_id, state, created_at) VALUES (?1, 'A', 'a', ?2, 'active', ?3)")
        .bind(ws.to_string()).bind(ordin_types::PlaceId::new().to_string()).bind(&now).execute(&pool).await.unwrap();
    let kinds = ["local_folder", "sharepoint_location", "confluence_location"];
    let mut links = Vec::new();
    for (n, kind) in kinds.iter().enumerate() {
        let id = ordin_types::SourceLinkId::new();
        sqlx::query("INSERT INTO source_links (id, workspace_id, kind, origin_location, display_name, state, added_at) VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6)")
            .bind(id.to_string()).bind(ws.to_string()).bind(kind).bind(format!("origin-{n}")).bind(format!("Link {n}")).bind(&now)
            .execute(&pool).await.unwrap();
        links.push(id);
    }
    for n in 0..40 {
        sqlx::query("INSERT INTO working_copy_items (id, source_link_id, origin_path, origin_version_read, local_version, state, read_at, origin_changed_by, git_branch, origin_changed_at) VALUES (?1, ?2, ?3, 'c1', 1, 'read', ?4, 'Sam', 'main', ?4)")
            .bind(ordin_types::WorkingCopyItemId::new().to_string()).bind(links[n % 3].to_string()).bind(format!("file-{n}.md")).bind(&now)
            .execute(&pool).await.unwrap();
    }
    for n in 0..2 {
        sqlx::query("INSERT INTO working_copy_unread (source_link_id, origin_path, item_ref, size_bytes) VALUES (?1, ?2, '{}', 10)")
            .bind(links[1].to_string()).bind(format!("big-{n}.bin")).execute(&pool).await.unwrap();
    }
    let mut registry = MigrationRegistry::new();
    registry.register(MIGRATIONS).unwrap();
    registry.register(crate::migrations()).unwrap();
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    let count = |sql: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>(sql)
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    assert_eq!(count("SELECT count(*) FROM source_links").await, 3);
    assert_eq!(count("SELECT count(*) FROM working_copy_items").await, 40);
    assert_eq!(count("SELECT count(*) FROM working_copy_items WHERE origin_newer_at IS NULL AND origin_changed_by = 'Sam' AND git_branch = 'main'").await, 40);
    assert_eq!(count("SELECT count(*) FROM working_copy_unread").await, 2);
    let broken = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(broken.is_empty());
    sqlx::query("INSERT INTO source_links (id, workspace_id, kind, origin_location, display_name, state, added_at) VALUES (?1, ?2, 'jira_issue', 'https://acme.atlassian.net/browse/ABC-1', 'ABC-1', 'proposed', ?3)")
        .bind(ordin_types::SourceLinkId::new().to_string()).bind(ws.to_string()).bind(&now).execute(&pool).await.unwrap();
    assert_eq!(count("SELECT count(*) FROM source_links").await, 4);
    // The index of the rebuilt table is back, and a child still cascades from its link.
    assert_eq!(
        count(
            "SELECT count(*) FROM sqlite_master WHERE name = 'working_copy_items_origin_changed'"
        )
        .await,
        1
    );
}

// The ADR-106 error rows that need a Jira stand-in.
#[tokio::test]
async fn ws_jira_site_mismatch_for_an_issue_on_another_site() {
    let env = Env::new().await;
    mount_jira_issue(
        &env,
        abc12("Assess.", "2026-01-11T10:30:00.000+0000"),
        two_comments(),
    )
    .await;
    let ws = env.create_workspace("A").await;
    let other = LinkLocation::JiraIssue {
        address: "https://other.atlassian.net/browse/ABC-12".to_owned(),
    };
    assert_eq!(
        code(env.service.add_link(ws.id, other, None).await),
        "WS_JIRA_SITE_MISMATCH"
    );
}

#[tokio::test]
async fn ws_jira_read_only_for_change_access_on_a_jira_issues_items() {
    let env = Env::new().await;
    mount_jira_issue(
        &env,
        abc12("Assess.", "2026-01-11T10:30:00.000+0000"),
        two_comments(),
    )
    .await;
    let ws = env.create_workspace("A").await;
    let link = env
        .service
        .add_link(ws.id, issue_location(), None)
        .await
        .unwrap();
    let place = link.place_id.unwrap();
    let who = Grantee::AgentInstance {
        agent_instance_id: AgentInstanceId::new(),
    };
    assert_eq!(
        code(
            env.service
                .set_grant(ws.id, who, place, Some(FolderAccess::ReadAndChange))
                .await
        ),
        "WS_JIRA_READ_ONLY"
    );
    assert!(env
        .service
        .set_grant(ws.id, who, place, Some(FolderAccess::Read))
        .await
        .unwrap()
        .is_some());
}
