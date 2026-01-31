//! Read paths of the remote links that the scenario tests leave out: how an unread item is
//! stored and parsed back, a listing that fails or nests, the read budget at the first read and
//! at the retry, and the Jira reads that arrive late, fail or are refused.

use ordin_types::domain::places::{SourceLinkKind, WorkingCopyState};
use serde_json::json;

use super::support::*;
use crate::remote::ItemRef;
use crate::{LinkLocation, WorkspaceError, LINK_READ_BUDGET};

const ADDRESS: &str = "https://acme.atlassian.net/browse/ABC-12";

fn issue_location() -> LinkLocation {
    LinkLocation::JiraIssue {
        address: ADDRESS.to_owned(),
    }
}

fn sharepoint_location() -> LinkLocation {
    LinkLocation::Sharepoint {
        location: contracts_folder(),
    }
}

fn code<T: std::fmt::Debug>(result: Result<T, WorkspaceError>) -> &'static str {
    result.expect_err("an error").code()
}

async fn paths_of(env: &Env, table: &str, link: crate::SourceLinkId) -> Vec<String> {
    let sql = match table {
        "working_copy_items" => {
            "SELECT origin_path FROM working_copy_items WHERE source_link_id = ?1 ORDER BY origin_path"
        }
        _ => "SELECT origin_path FROM working_copy_unread WHERE source_link_id = ?1 ORDER BY origin_path",
    };
    sqlx::query_scalar(sql)
        .bind(link.to_string())
        .fetch_all(&env.pool)
        .await
        .unwrap()
}

#[test]
fn an_unread_jira_attachment_reference_is_stored_and_read_back() {
    let stored = ItemRef::JiraAttachment("10001".to_owned()).to_json();
    match ItemRef::from_json(SourceLinkKind::JiraIssue, &stored) {
        Ok(ItemRef::JiraAttachment(id)) => assert_eq!(id, "10001"),
        other => panic!("expected the attachment, got {other:?}"),
    }
}

#[test]
fn a_damaged_unread_reference_is_a_datastore_error() {
    for (kind, text) in [
        (SourceLinkKind::SharepointLocation, "{}"),
        (SourceLinkKind::SharepointLocation, "not json"),
        (SourceLinkKind::ConfluenceLocation, "[]"),
        (SourceLinkKind::JiraIssue, "not json"),
        (SourceLinkKind::JiraIssue, r#"{"other":"x"}"#),
        (SourceLinkKind::LocalFolder, "{}"),
    ] {
        let outcome = ItemRef::from_json(kind, text);
        assert_eq!(
            outcome.expect_err("an error").code(),
            "WS_DATASTORE_UNAVAILABLE",
            "{kind:?} {text}"
        );
    }
}

#[tokio::test]
async fn a_listing_that_fails_at_the_start_adds_no_link() {
    let env = Env::new().await;
    // The first mock mounted wins: the folder listing fails.
    get_json(
        &env,
        &graph(&format!("/drives/{DRIVE}/items/{FOLDER}/children")),
        500,
        json!({}),
    )
    .await;
    mount_library(&env, &many_files(2)).await;
    let ws = env.create_workspace("A").await;
    assert_eq!(
        code(
            env.service
                .add_link(ws.id, sharepoint_location(), None)
                .await
        ),
        "WS_SOURCE_UNAVAILABLE"
    );
    assert!(env.service.source_links(ws.id).await.unwrap().is_empty());
}

#[tokio::test]
async fn files_in_a_subfolder_are_read_under_the_subfolder_name() {
    let env = Env::new().await;
    let top = file_json("F0001", "top.txt", "c4", "/Suppliers/Contracts", "Sam Lee");
    let sub = folder_json("SUB1", "Annex", "/Suppliers/Contracts");
    let inner = file_json(
        "F0002",
        "inner.txt",
        "c4",
        "/Suppliers/Contracts/Annex",
        "Sam Lee",
    );
    mount_children(&env, FOLDER, vec![top, sub]).await;
    mount_children(&env, "SUB1", vec![inner]).await;
    mount_library(&env, &many_files(2)).await;
    let ws = env.create_workspace("A").await;
    let link = env
        .service
        .add_link(ws.id, sharepoint_location(), None)
        .await
        .unwrap();
    let progress = env.wait_read(link.id).await;
    assert_eq!(progress.listed, 2);
    assert_eq!(
        paths_of(&env, "working_copy_items", link.id).await,
        ["Annex/inner.txt", "top.txt"]
    );
}

#[tokio::test]
async fn the_first_read_stops_at_the_first_file_past_the_budget_and_the_retry_takes_what_fits() {
    let env = Env::new().await;
    let sized = |id: &str, name: &str, size: u64| {
        let mut file = file_json(id, name, "c4", "/Suppliers/Contracts", "Sam Lee");
        file["size"] = json!(size);
        file
    };
    let listing = vec![
        sized("F0001", "doc-a.txt", 10),
        sized("F0002", "doc-b.txt", LINK_READ_BUDGET + 1),
        sized("F0003", "doc-c.txt", 10),
    ];
    let files: Vec<(String, String, String)> = ["F0001", "F0002", "F0003"]
        .iter()
        .zip(["doc-a.txt", "doc-b.txt", "doc-c.txt"])
        .map(|(id, name)| ((*id).to_owned(), name.to_owned(), "c4".to_owned()))
        .collect();
    mount_children(&env, FOLDER, listing.clone()).await;
    mount_library(&env, &files).await;
    let ws = env.create_workspace("A").await;
    let link = env
        .service
        .add_link(ws.id, sharepoint_location(), None)
        .await
        .unwrap();
    let progress = env.wait_read(link.id).await;
    assert_eq!((progress.read, progress.not_read), (1, 2));
    assert_eq!(
        paths_of(&env, "working_copy_items", link.id).await,
        ["doc-a.txt"]
    );
    assert_eq!(
        paths_of(&env, "working_copy_unread", link.id).await,
        ["doc-b.txt", "doc-c.txt"]
    );
    // At the next origin check the file that fits is read; the one past the budget stays unread.
    env.mock.reset().await;
    mount_children(&env, FOLDER, listing).await;
    mount_library(&env, &files).await;
    mount_delta(&env, "T1", vec![], "T2").await;
    let report = env.service.open(ws.id).await.unwrap();
    assert!(report.checks_failed.is_empty());
    assert_eq!(
        paths_of(&env, "working_copy_items", link.id).await,
        ["doc-a.txt", "doc-c.txt"]
    );
    assert_eq!(
        paths_of(&env, "working_copy_unread", link.id).await,
        ["doc-b.txt"]
    );
}

#[tokio::test]
async fn related_md_lists_the_linked_issues_with_their_relation() {
    let env = Env::new().await;
    let mut issue = abc12("Assess the Northwind account.", JIRA_UPDATED);
    issue["fields"]["issuelinks"] = json!([{
        "type": {"name": "Blocks", "inward": "is blocked by", "outward": "blocks"},
        "outwardIssue": {"id": "10020", "key": "ABC-20",
            "fields": {"summary": "Fix the rates", "status": {"statusCategory": {"key": "done"}}}}
    }]);
    mount_jira_issue(&env, issue, vec![]).await;
    let ws = env.create_workspace("A").await;
    let link = env
        .service
        .add_link(ws.id, issue_location(), None)
        .await
        .unwrap();
    let related =
        std::fs::read_to_string(env.service.working_copy_dir(link.id).join("related.md")).unwrap();
    assert!(
        related.contains("blocks: ABC-20 Fix the rates — Done"),
        "{related}"
    );
}

#[tokio::test]
async fn a_jira_attachment_that_cannot_be_downloaded_is_read_on_first_use() {
    let env = Env::new().await;
    // The first mock mounted wins: the download fails while the issue is first read.
    get_json(
        &env,
        &format!("{JIRA_API}/attachment/content/10001"),
        500,
        json!({}),
    )
    .await;
    mount_jira_issue(
        &env,
        abc12("Assess the Northwind account.", JIRA_UPDATED),
        vec![],
    )
    .await;
    let ws = env.create_workspace("A").await;
    let link = env
        .service
        .add_link(ws.id, issue_location(), None)
        .await
        .unwrap();
    let paths = paths_of(&env, "working_copy_items", link.id).await;
    assert!(!paths.contains(&"attachments/10001-rates.xlsx".to_owned()));
    assert_eq!(
        paths_of(&env, "working_copy_unread", link.id).await,
        ["attachments/10001-rates.xlsx"]
    );
    env.mock.reset().await;
    mount_jira_issue(
        &env,
        abc12("Assess the Northwind account.", JIRA_UPDATED),
        vec![],
    )
    .await;
    let item = env
        .service
        .read_item(link.id, "attachments/10001-rates.xlsx")
        .await
        .unwrap();
    assert_eq!(item.state, WorkingCopyState::Read);
    assert_eq!(item.origin_version_read, "attachment:10001");
    assert!(paths_of(&env, "working_copy_unread", link.id)
        .await
        .is_empty());
    let size: i64 = sqlx::query_scalar(
        "SELECT size_bytes FROM working_copy_items WHERE source_link_id = ?1 AND origin_path = 'attachments/10001-rates.xlsx'",
    )
    .bind(link.id.to_string())
    .fetch_one(&env.pool)
    .await
    .unwrap();
    assert_eq!(size, 11);
}

#[tokio::test]
async fn a_jira_attachment_over_the_limit_is_listed_unread_and_refused_on_first_use() {
    let env = Env::new().await;
    let mut issue = abc12("Assess the Northwind account.", JIRA_UPDATED);
    issue["fields"]["attachment"][0]["size"] = json!(crate::JIRA_ATTACHMENT_MAX_BYTES + 1);
    mount_jira_issue(&env, issue, vec![]).await;
    let ws = env.create_workspace("A").await;
    let link = env
        .service
        .add_link(ws.id, issue_location(), None)
        .await
        .unwrap();
    assert_eq!(
        paths_of(&env, "working_copy_unread", link.id).await,
        ["attachments/10001-rates.xlsx"]
    );
    assert!(env
        .jira_requests()
        .await
        .iter()
        .all(|(_, p)| !p.contains("attachment/content")));
    assert_eq!(
        code(
            env.service
                .read_item(link.id, "attachments/10001-rates.xlsx")
                .await
        ),
        "WS_ITEM_TOO_LARGE"
    );
}

#[tokio::test]
async fn a_new_comment_is_read_at_the_next_check_and_a_deleted_one_is_announced_once() {
    let env = Env::new().await;
    let first = jira_comment("10300", "2026-01-10T09:30:00.000+0000", adf("First remark"));
    let second = jira_comment(
        "10301",
        "2026-01-10T09:45:00.000+0000",
        adf("Second remark"),
    );
    mount_jira_issue(
        &env,
        abc12("Assess the Northwind account.", JIRA_UPDATED),
        vec![first.clone()],
    )
    .await;
    let ws = env.create_workspace("A").await;
    let link = env
        .service
        .add_link(ws.id, issue_location(), None)
        .await
        .unwrap();
    let mut changes = env.service.subscribe_origin_changes();
    env.mock.reset().await;
    mount_jira_issue(
        &env,
        abc12("Assess the Northwind account.", JIRA_UPDATED),
        vec![first.clone(), second],
    )
    .await;
    let report = env.service.open(ws.id).await.unwrap();
    assert_eq!(report.origin_changes, 1);
    assert!(paths_of(&env, "working_copy_items", link.id)
        .await
        .contains(&"comments/20260110T094500Z-10301.md".to_owned()));
    // The second comment is deleted at the origin: the item stays and the removal is announced once.
    env.mock.reset().await;
    mount_jira_issue(
        &env,
        abc12("Assess the Northwind account.", JIRA_UPDATED),
        vec![first],
    )
    .await;
    env.service.open(ws.id).await.unwrap();
    env.service.open(ws.id).await.unwrap();
    let mut removed = 0;
    while let Ok(change) = changes.try_recv() {
        if change.removed && change.origin_path == "comments/20260110T094500Z-10301.md" {
            removed += 1;
        }
    }
    assert_eq!(removed, 1);
    assert!(paths_of(&env, "working_copy_items", link.id)
        .await
        .contains(&"comments/20260110T094500Z-10301.md".to_owned()));
}
