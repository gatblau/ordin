//! The Workspace Service through its public interface over the real SharePoint, Confluence and Jira
//! adapters, the real File Service, Journal and Identity, and their `wiremock` stand-ins on loopback
//! (plan 01, Phase 8 and "HTTP stand-ins"): SharePoint, Confluence and Jira links are read into the
//! working copy, kept up to date at their origins, and removed with the workspace. No real network is
//! used; the stand-ins' replies follow the adapters' own specs.

mod support;

use std::time::Duration;

use ordin_workspaces::{
    ConfluenceLinkState, LinkLocation, SourceLinkKind, SourceLinkState, WorkingCopyState,
    WorkspaceListFilter,
};
use serde_json::json;
use support::*;

/// A Confluence tree: `Supplier pages` (4000) with the child page `Rates` (4411) at version 7.
async fn mount_confluence_tree(env: &Env) {
    mount_where(env).await;
    wiki_json(
        env,
        "/rest/api/user",
        json!({"accountId": "acc-1", "displayName": "Sam Lee"}),
    )
    .await;
    wiki_json(
        env,
        &format!("/api/v2/pages/{PARENT}/attachments"),
        json!({"results": [], "_links": {}}),
    )
    .await;
    wiki_json(
        env,
        &format!("/api/v2/pages/{PARENT}/children"),
        json!({"results": [{"id": "4411", "title": "Rates"}], "_links": {}}),
    )
    .await;
    wiki_json(env, "/api/v2/pages/4411", page_json("4411", "Rates", 7)).await;
    wiki_json(
        env,
        "/api/v2/pages/4411/attachments",
        json!({"results": [], "_links": {}}),
    )
    .await;
    wiki_json(
        env,
        "/api/v2/pages/4411/children",
        json!({"results": [], "_links": {}}),
    )
    .await;
    wiki_json(
        env,
        "/rest/api/search",
        json!({"results": [{
            "content": {"id": PARENT, "type": "page", "title": "Supplier pages"},
            "resultGlobalContainer": {"title": "Procurement", "displayUrl": "/spaces/PROC"},
            "url": format!("/spaces/PROC/pages/{PARENT}"),
        }], "_links": {}}),
    )
    .await;
}

#[tokio::test]
async fn a_sharepoint_link_is_read_and_a_changed_document_is_read_again() {
    let env = Env::new().await;
    mount_library(
        &env,
        &[
            kestrel("c4"),
            ("F0002".to_owned(), "Terms.docx".to_owned(), "c4".to_owned()),
        ],
    )
    .await;
    let workspace = env.create_workspace("Q3 supplier review").await;
    let link = env
        .service
        .add_link(
            workspace.id,
            LinkLocation::Sharepoint {
                location: contracts_folder(),
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(link.kind, SourceLinkKind::SharepointLocation);
    let progress = env.wait_read(link.id).await;
    assert_eq!((progress.read, progress.not_read), (2, 0));
    let items = env.service.working_copy_items(link.id).await.unwrap();
    assert_eq!(items.len(), 2);
    // The bytes are in the working copy, and the Push Service's view of an item names them.
    let kestrel_item = items
        .iter()
        .find(|i| i.origin_path == "Kestrel MSA.docx")
        .unwrap();
    let entry = env
        .service
        .working_copy_entry(kestrel_item.id)
        .await
        .unwrap();
    let place = env.files.place(entry.place_id).await.unwrap();
    assert_eq!(
        std::fs::read(std::path::Path::new(&place.path).join(&entry.relative_path)).unwrap(),
        format!("content of {FILE}").into_bytes()
    );
    assert!(entry.origin_location.starts_with("graph:drives/"));

    // Sam Lee changes the document at SharePoint; opening the workspace asks the adapter.
    env.mock.reset().await;
    mount_library(
        &env,
        &[
            kestrel("c5"),
            ("F0002".to_owned(), "Terms.docx".to_owned(), "c4".to_owned()),
        ],
    )
    .await;
    mount_delta(&env, "T1", vec![kestrel_change("c5")], "T2").await;
    let report = env.service.open(workspace.id).await.unwrap();
    assert_eq!((report.origin_changes, report.checks_failed.len()), (1, 0));
    let refreshed = env
        .service
        .working_copy_item(kestrel_item.id)
        .await
        .unwrap();
    assert_eq!(
        (refreshed.state, refreshed.origin_version_read.as_str()),
        (WorkingCopyState::Read, "c5")
    );
    let version = env
        .service
        .origin_version(
            workspace.id,
            SourceLinkKind::SharepointLocation,
            "Kestrel MSA.docx",
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (version.version.as_str(), version.changed_by.as_deref()),
        ("c5", Some("Sam Lee"))
    );
}

#[tokio::test]
async fn a_confluence_link_is_read_and_its_folder_link_goes_active_on_an_approval_from_the_journal()
{
    let env = Env::new().await;
    mount_confluence_tree(&env).await;
    // The program registers the service with the Journal; a decision then reaches it as an event.
    env.journal.subscribe(env.service.clone());
    let workspace = env.create_workspace("Supplier pages").await;
    let link = env
        .service
        .add_link(
            workspace.id,
            LinkLocation::Confluence {
                location: parent_location(),
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(link.kind, SourceLinkKind::ConfluenceLocation);
    assert_eq!(link.state, SourceLinkState::Active);
    let progress = env.wait_read(link.id).await;
    assert_eq!((progress.read, progress.not_read), (1, 0));
    let items = env.service.working_copy_items(link.id).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].origin_version_read, "7");

    // A folder of the workspace linked to a Confluence page: one approval, then active.
    let proposed = env
        .service
        .propose_confluence_link(
            workspace.id,
            workspace.own_folder_place_id,
            String::new(),
            parent_location(),
        )
        .await
        .unwrap();
    assert_eq!(proposed.state, ConfluenceLinkState::Proposed);
    assert_eq!(proposed.audience, "Procurement team");
    env.journal
        .append_event("approval_decided", json!({"approval_id": proposed.approval_id.unwrap(), "state": "approved", "decided_on": "desktop"}))
        .await
        .unwrap();
    let mut active = false;
    for _ in 0..200 {
        let links = env.service.confluence_links(workspace.id).await.unwrap();
        if links[0].state == ConfluenceLinkState::Active {
            active = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(
        active,
        "the approval reached the service through the Journal"
    );
    let paused = env
        .service
        .pause_confluence_link(proposed.id)
        .await
        .unwrap();
    assert_eq!(paused.state, ConfluenceLinkState::Paused);
    assert_eq!(
        env.service
            .resume_confluence_link(proposed.id)
            .await
            .unwrap()
            .state,
        ConfluenceLinkState::Active
    );
    env.service
        .remove_confluence_link(proposed.id)
        .await
        .unwrap();
    assert!(env
        .service
        .confluence_links(workspace.id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn a_jira_issue_is_linked_read_only_and_a_newer_version_waits_for_the_person() {
    let env = Env::new().await;
    mount_jira_issue(
        &env,
        abc12(
            "Assess the Northwind account.",
            "2026-01-10T09:00:00.000+0000",
        ),
        vec![jira_comment(
            "10300",
            "2026-01-10T09:30:00.000+0000",
            adf("First remark"),
        )],
    )
    .await;
    let workspace = env.create_workspace("Northwind").await;
    let link = env
        .service
        .add_link(
            workspace.id,
            LinkLocation::JiraIssue {
                address: "https://acme.atlassian.net/browse/ABC-12".to_owned(),
            },
            None,
        )
        .await
        .unwrap();
    let items = env.service.working_copy_items(link.id).await.unwrap();
    assert_eq!(items.len(), 4);
    assert!(items.iter().all(|i| i.state == WorkingCopyState::Read));
    assert!(env
        .jira_requests()
        .await
        .iter()
        .all(|(method, _)| method == "GET"));
    env.mock.reset().await;
    mount_jira_issue(
        &env,
        abc12(
            "Assess the Northwind account again.",
            "2026-01-12T08:00:00.000+0000",
        ),
        vec![jira_comment(
            "10300",
            "2026-01-10T09:30:00.000+0000",
            adf("First remark"),
        )],
    )
    .await;
    let report = env.service.open(workspace.id).await.unwrap();
    assert!(report.origin_changes >= 1);
    let waiting = env
        .service
        .working_copy_items(link.id)
        .await
        .unwrap()
        .into_iter()
        .filter(|i| i.origin_newer_at.is_some())
        .count();
    assert!(waiting >= 1);
    let brought = env
        .service
        .read_item(link.id, "summary-and-description.md")
        .await
        .unwrap();
    assert!(brought.origin_newer_at.is_none());
}

#[tokio::test]
async fn the_three_sources_leave_with_the_workspace() {
    let env = Env::new().await;
    mount_library(&env, &[kestrel("c4")]).await;
    mount_delta(&env, "T1", vec![], "T2").await;
    mount_confluence_tree(&env).await;
    mount_jira_issue(
        &env,
        abc12("Assess.", "2026-01-10T09:00:00.000+0000"),
        vec![],
    )
    .await;
    let workspace = env.create_workspace("Everything").await;
    let sharepoint = env
        .service
        .add_link(
            workspace.id,
            LinkLocation::Sharepoint {
                location: contracts_folder(),
            },
            None,
        )
        .await
        .unwrap();
    let confluence = env
        .service
        .add_link(
            workspace.id,
            LinkLocation::Confluence {
                location: parent_location(),
            },
            None,
        )
        .await
        .unwrap();
    let jira = env
        .service
        .add_link(
            workspace.id,
            LinkLocation::JiraIssue {
                address: "https://acme.atlassian.net/browse/ABC-12".to_owned(),
            },
            None,
        )
        .await
        .unwrap();
    env.wait_read(sharepoint.id).await;
    env.wait_read(confluence.id).await;
    let report = env.service.open(workspace.id).await.unwrap();
    assert!(
        report.checks_failed.is_empty(),
        "failed: sharepoint {} confluence {} jira {} -> {:?}",
        sharepoint.id,
        confluence.id,
        jira.id,
        report.checks_failed
    );
    let impact = env.service.bin_impact(workspace.id).await.unwrap();
    assert_eq!(impact.unsent_changes, 0);
    for link in [&sharepoint, &confluence, &jira] {
        assert!(env.service.working_copy_dir(link.id).is_dir());
    }
    env.service.move_to_bin(workspace.id).await.unwrap();
    assert_eq!(
        env.service
            .list(WorkspaceListFilter::InBin)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(env.service.empty_bin().await.unwrap(), 1);
    for link in [&sharepoint, &confluence, &jira] {
        assert!(!env.service.working_copy_dir(link.id).exists());
    }
    assert_eq!(env.scalar("SELECT count(*) FROM source_links").await, 0);
    assert_eq!(
        env.scalar("SELECT count(*) FROM working_copy_items").await,
        0
    );
    assert!(env.documents.join("ordin").join("Everything").is_dir());
}
