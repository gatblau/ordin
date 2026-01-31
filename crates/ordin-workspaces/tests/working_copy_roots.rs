//! The Workspace Service with the program's real roots. Working copies live under `<documents>/ordin/Linked copies`,
//! outside the Rules Engine's application data root (`WORKING_COPY_ROOT`, WS-WC-1), so an agent can be
//! given one (plan 01, FU-095, for the earlier root). A SharePoint, Confluence and Jira link each still
//! gets its working-copy place, over the same `wiremock` stand-ins as `sources_integration.rs`.

mod support;

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
async fn links_of_the_three_sources_are_added_when_the_data_root_is_protected() {
    let env = Env::with(Options {
        rules_cover_data_root: true,
        ..Options::default()
    })
    .await;
    mount_library(&env, &[kestrel("c4")]).await;
    mount_delta(&env, "T1", vec![], "T2").await;
    mount_confluence_tree(&env).await;
    mount_jira_issue(
        &env,
        abc12("Assess.", "2026-01-10T09:00:00.000+0000"),
        vec![],
    )
    .await;
    let workspace = env.create_workspace("Real roots").await;
    let sharepoint = env
        .service
        .add_link(
            workspace.id,
            ordin_workspaces::LinkLocation::Sharepoint {
                location: contracts_folder(),
            },
            None,
        )
        .await
        .expect("the SharePoint link is added");
    let confluence = env
        .service
        .add_link(
            workspace.id,
            ordin_workspaces::LinkLocation::Confluence {
                location: parent_location(),
            },
            None,
        )
        .await
        .expect("the Confluence link is added");
    let jira = env
        .service
        .add_link(
            workspace.id,
            ordin_workspaces::LinkLocation::JiraIssue {
                address: "https://acme.atlassian.net/browse/ABC-12".to_owned(),
            },
            None,
        )
        .await
        .expect("the Jira link is added");
    env.wait_read(sharepoint.id).await;
    env.wait_read(confluence.id).await;
    for link in [&sharepoint, &confluence, &jira] {
        // WS-WC-1: outside ordin's data root, so an agent can be given it.
        let copy = env.service.working_copy_dir(link.id);
        assert!(copy.is_dir());
        assert!(!copy.starts_with(&env.data_root));
        assert!(!env
            .service
            .working_copy_items(link.id)
            .await
            .unwrap()
            .is_empty());
    }
}
