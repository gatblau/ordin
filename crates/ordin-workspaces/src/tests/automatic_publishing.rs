//! X2 for WorkspaceService.md, amendment 2026-01-24: automatic Confluence publishing and agents that
//! can run commands (SA-WS-AUTO-01 to 03). One test per scenario, named after it.

use ordin_confluence::AutomaticPublishing;
use ordin_rules::{AgentToolPolicy, InstanceRuleSet};
use ordin_types::domain::places::{
    CopyDestination, CopyFormat, CopyMaker, CopyOriginal, FolderAccess, Grantee, PublishedCopy,
    PublishedCopyState, PublishedVia,
};
use ordin_types::domain::Authority;
use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, ConfluenceLinkId, CrewCharterId, PublishedCopyId};

use super::support::*;
use crate::LinkLocation;

/// An agent instance whose loaded rule set has `authority`.
fn agent(env: &Env, authority: Authority) -> AgentInstanceId {
    let id = AgentInstanceId::new();
    env.rules
        .load_rule_set(InstanceRuleSet {
            agent_instance_id: id,
            package_name: "analyst".to_owned(),
            authority,
            agent_folder: env.dir.path().join(format!("agent-{id}")),
            agent_folder_read_only: false,
            declaration: None,
            configured: None,
            tool_policy: Some(AgentToolPolicy {
                prohibited_verbs: Vec::new(),
                forbidden_flags: Vec::new(),
                requires_approval: Vec::new(),
                mcp_servers: Vec::new(),
            }),
            approval_list: Vec::new(),
            package_read_only_tools: Vec::new(),
            tools: Vec::new(),
            unmapped_operations: Vec::new(),
            external: Vec::new(),
            commands: Vec::new(),
            reviewer: None,
        })
        .expect("the rule set loads");
    id
}

fn of(agent: AgentInstanceId) -> Grantee {
    Grantee::AgentInstance {
        agent_instance_id: agent,
    }
}

async fn publishing(env: &Env, link: ConfluenceLinkId) -> crate::ConfluencePublishing {
    env.service
        .confluence_publishing(link)
        .await
        .expect("the value")
}

// Scenario: Error path — a grant to an agent that can run commands turns automatic publishing off (SA-WS-AUTO-01)
#[tokio::test]
async fn error_path_a_grant_to_an_agent_that_can_run_commands_turns_automatic_publishing_off() {
    let env = Env::new().await;
    let ws = env.create_workspace("A").await;
    let (link, approval) = env.proposed_confluence_link(&ws).await;
    env.decide(approval, "approved").await;
    // A current copy of a file the person then edits: with automatic publishing on, the sync
    // would push it.
    let place = ws.own_folder_place_id;
    env.files
        .write_version(ordin_files::WriteVersion {
            place_id: place,
            path: "rates.md".to_owned(),
            bytes: b"# Rates".to_vec(),
            origin: ordin_files::FileVersionOrigin::Person,
            expected_hash: None,
        })
        .await
        .unwrap();
    let published = env.files.read_file(place, "rates.md").await.unwrap().hash.0;
    let copy = PublishedCopyId::new();
    env.files
        .record_published_copy(PublishedCopy {
            id: copy,
            original: CopyOriginal::PlaceFile {
                place_id: place,
                path: "rates.md".to_owned(),
            },
            destination: CopyDestination::ConfluencePage {
                space: "PROC".to_owned(),
                parent_page_id: "4410".to_owned(),
            },
            destination_item_id: Some("4411".to_owned()),
            format: CopyFormat::Page,
            maker: CopyMaker::Ordin,
            published_via: PublishedVia::ConfluenceLink {
                confluence_link_id: link,
            },
            followed_branch: None,
            keep_up_to_date: true,
            audience: "Procurement team".to_owned(),
            original_version_published: Some(published),
            destination_version_pushed: Some("7".to_owned()),
            last_updated_at: Some(Timestamp::now()),
            state: PublishedCopyState::Current,
            failure_reason: None,
        })
        .await
        .unwrap();
    env.files
        .save_person_edit(place, "rates.md", b"# New rates".to_vec())
        .await
        .unwrap();
    let a = agent(&env, Authority::SystemChanging);

    env.service
        .set_grant(ws.id, of(a), place, Some(FolderAccess::ReadAndChange))
        .await
        .unwrap();

    let value = publishing(&env, link).await;
    assert_eq!(value.automatic, AutomaticPublishing::Off);
    assert_eq!(value.turned_off_by, [of(a)]);
    env.service.sync_confluence_link(link).await;
    let after = env.files.published_copy(copy).await.unwrap();
    assert_eq!(after.state, PublishedCopyState::OutOfDate, "not pushed");
    assert!(
        env.requests()
            .await
            .iter()
            .all(|(m, _)| m != "PUT" && m != "POST"),
        "nothing is sent to Confluence"
    );
}

// Scenario: Happy path — a document-only agent's grant leaves it on
#[tokio::test]
async fn happy_path_a_document_only_agents_grant_leaves_it_on() {
    let env = Env::new().await;
    let ws = env.create_workspace("A").await;
    let (link, _) = env.proposed_confluence_link(&ws).await;
    let d = agent(&env, Authority::DocumentOnly);
    env.service
        .set_grant(
            ws.id,
            of(d),
            ws.own_folder_place_id,
            Some(FolderAccess::ReadAndChange),
        )
        .await
        .unwrap();
    let value = publishing(&env, link).await;
    assert_eq!(value.automatic, AutomaticPublishing::On);
    assert!(value.turned_off_by.is_empty());
}

// Scenario: Edge case — a read grant counts, and so does a grant in another workspace
#[tokio::test]
async fn edge_case_a_read_grant_counts_and_so_does_a_grant_in_another_workspace() {
    let env = Env::new().await;
    let folder = env.folder("quotes").to_string_lossy().into_owned();
    let ws = env.create_workspace("A").await;
    let p = env
        .service
        .add_link(
            ws.id,
            LinkLocation::LocalFolder {
                path: folder.clone(),
            },
            None,
        )
        .await
        .unwrap()
        .place_id
        .unwrap();
    mount_where(&env).await;
    let link = env
        .service
        .propose_confluence_link(ws.id, p, String::new(), parent_location())
        .await
        .unwrap()
        .id;
    let v = env.create_workspace("V").await;
    let q = env
        .service
        .add_link(v.id, LinkLocation::LocalFolder { path: folder }, None)
        .await
        .unwrap()
        .place_id
        .unwrap();
    let a = agent(&env, Authority::SystemChanging);
    env.service
        .set_grant(v.id, of(a), q, Some(FolderAccess::Read))
        .await
        .unwrap();
    let value = publishing(&env, link).await;
    assert_eq!(value.automatic, AutomaticPublishing::Off);
    assert_eq!(value.turned_off_by, [of(a)]);
}

// Scenario: Edge case — a crew's grant turns it off
#[tokio::test]
async fn edge_case_a_crews_grant_turns_it_off() {
    let env = Env::new().await;
    let ws = env.create_workspace("A").await;
    let (link, _) = env.proposed_confluence_link(&ws).await;
    let crew = Grantee::Crew {
        crew_charter_id: CrewCharterId::new(),
    };
    env.service
        .set_grant(
            ws.id,
            crew,
            ws.own_folder_place_id,
            Some(FolderAccess::ReadAndChange),
        )
        .await
        .unwrap();
    let value = publishing(&env, link).await;
    assert_eq!(value.automatic, AutomaticPublishing::Off);
    assert_eq!(value.turned_off_by, [crew]);
}

// Scenario: Edge case — a grant on a folder outside the linked folder leaves it on
#[tokio::test]
async fn edge_case_a_grant_on_a_folder_outside_the_linked_folder_leaves_it_on() {
    let env = Env::new().await;
    let ws = env.create_workspace("A").await;
    let (link, _) = env.proposed_confluence_link(&ws).await;
    let q = env
        .service
        .add_link(
            ws.id,
            LinkLocation::LocalFolder {
                path: env.folder("elsewhere").to_string_lossy().into_owned(),
            },
            None,
        )
        .await
        .unwrap()
        .place_id
        .unwrap();
    let a = agent(&env, Authority::SystemChanging);
    env.service
        .set_grant(ws.id, of(a), q, Some(FolderAccess::ReadAndChange))
        .await
        .unwrap();
    assert_eq!(
        publishing(&env, link).await.automatic,
        AutomaticPublishing::On
    );
}

// Scenario: Happy path — removing the grant turns it on again
#[tokio::test]
async fn happy_path_removing_the_grant_turns_it_on_again() {
    let env = Env::new().await;
    let ws = env.create_workspace("A").await;
    let (link, _) = env.proposed_confluence_link(&ws).await;
    let a = agent(&env, Authority::SystemChanging);
    let place = ws.own_folder_place_id;
    env.service
        .set_grant(ws.id, of(a), place, Some(FolderAccess::ReadAndChange))
        .await
        .unwrap();
    assert_eq!(
        publishing(&env, link).await.automatic,
        AutomaticPublishing::Off
    );
    env.service
        .set_grant(ws.id, of(a), place, None)
        .await
        .unwrap();
    let value = publishing(&env, link).await;
    assert_eq!(value.automatic, AutomaticPublishing::On);
    assert!(value.turned_off_by.is_empty());
}

// Scenario: Error path — an unknown link
#[tokio::test]
async fn error_path_an_unknown_link() {
    let env = Env::new().await;
    let e = env
        .service
        .confluence_publishing(ConfluenceLinkId::new())
        .await
        .unwrap_err();
    assert_eq!(e.code(), "WS_LINK_NOT_FOUND");
}
