//! X2: one test per Gherkin scenario of WorkspaceService.md, named after it.

use std::time::Duration;

use ordin_files::{PlaceChange, PlaceChangeKind};
use ordin_types::domain::places::{
    ConfluenceLinkState, FolderAccess, Grantee, SourceLinkKind, SourceLinkState, WorkingCopyState,
    WorkspaceState,
};
use ordin_types::domain::server::ChannelKind;
use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, PlaceId, RunId};

use super::support::*;
use crate::{InboxOrigin, LinkLocation, WorkspaceError};

fn analyst() -> AgentInstanceId {
    AgentInstanceId::new()
}

fn grantee(agent: AgentInstanceId) -> Grantee {
    Grantee::AgentInstance {
        agent_instance_id: agent,
    }
}

/// A file change as the File Service reports it.
fn change(
    place: PlaceId,
    path: &str,
    kind: PlaceChangeKind,
    by_ordin: bool,
    version: Option<u32>,
) -> PlaceChange {
    PlaceChange {
        place_id: place,
        path: path.to_owned(),
        change: kind,
        observed_at: Timestamp::now(),
        by_ordin,
        version,
        origin: None,
    }
}

fn code_of<T: std::fmt::Debug>(result: Result<T, WorkspaceError>) -> &'static str {
    result.expect_err("an error").code()
}

// Scenario: Happy path — create a workspace with its own folder
#[tokio::test]
async fn happy_path_create_a_workspace_with_its_own_folder() {
    let env = Env::new().await;
    let ws = env
        .service
        .create("Q3 supplier review".to_owned())
        .await
        .unwrap();
    assert_eq!(ws.state, WorkspaceState::Active);
    let folder = env.documents.join("ordin").join("Q3 supplier review");
    assert!(folder.is_dir());
    let place = env.files.place(ws.own_folder_place_id).await.unwrap();
    assert_eq!(place.kind, ordin_files::PlaceKind::WorkspaceFolder);
    assert_eq!(
        std::path::PathBuf::from(&place.path),
        std::fs::canonicalize(folder)
            .map(ordin_types::paths::plain_form)
            .unwrap()
    );
    assert_eq!(env.service.places(ws.id).await.unwrap().len(), 1);
}

// WS-WC-1: a workspace named like the linked copies' folder gets a folder of its own beside it.
// spec: ordin-desktop/components/WorkspaceService#WS-WC-1
#[tokio::test]
async fn edge_case_a_workspace_named_linked_copies_does_not_take_their_folder() {
    let env = Env::new().await;
    let ws = env
        .service
        .create("Linked copies".to_owned())
        .await
        .unwrap();
    let place = env.files.place(ws.own_folder_place_id).await.unwrap();
    assert!(place.path.ends_with("Linked copies (2)"), "{}", place.path);
}

// Scenario: Error path — duplicate name among workspaces not in the Bin
#[tokio::test]
async fn error_path_duplicate_name_among_workspaces_not_in_the_bin() {
    let env = Env::new().await;
    env.create_workspace("Q3 supplier review").await;
    let outcome = env.service.create("q3 supplier review".to_owned()).await;
    assert_eq!(code_of(outcome), "WS_NAME_DUPLICATE");
}

// Scenario: Edge case — a binned workspace's name can be reused
#[tokio::test]
async fn edge_case_a_binned_workspaces_name_can_be_reused() {
    let env = Env::new().await;
    let first = env.create_workspace("Q3 supplier review").await;
    env.service.move_to_bin(first.id).await.unwrap();
    env.service
        .create("Q3 supplier review".to_owned())
        .await
        .unwrap();
    assert_eq!(
        code_of(env.service.restore_from_bin(first.id).await),
        "WS_NAME_DUPLICATE"
    );
}

// Scenario: Happy path — add a SharePoint location and read it
#[tokio::test]
async fn happy_path_add_a_sharepoint_location_and_read_it() {
    let env = Env::new().await;
    mount_library(&env, &many_files(212)).await;
    let ws = env.create_workspace("Q3 supplier review").await;
    let link = env
        .service
        .add_link(
            ws.id,
            LinkLocation::Sharepoint {
                location: contracts_folder(),
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(link.state, SourceLinkState::Active);
    let place = env
        .files
        .place(link.place_id.expect("a working-copy place"))
        .await
        .unwrap();
    assert_eq!(place.kind, ordin_files::PlaceKind::WorkingCopy);
    let progress = env.wait_read(link.id).await;
    assert_eq!(
        (
            progress.listed,
            progress.read,
            progress.not_read,
            progress.done
        ),
        (212, 212, 0, true)
    );
    let items = env.service.working_copy_items(link.id).await.unwrap();
    assert_eq!(items.len(), 212);
    assert!(items
        .iter()
        .all(|i| i.state == WorkingCopyState::Read && i.origin_version_read == "c4"));
    assert!(env.service.source_links(ws.id).await.unwrap()[0]
        .last_read_at
        .is_some());
}

// Scenario: Error path — SharePoint needs a sign-in
#[tokio::test]
async fn error_path_sharepoint_needs_a_sign_in() {
    let env = Env::with(Options {
        microsoft: false,
        ..Options::default()
    })
    .await;
    mount_library(&env, &many_files(3)).await;
    let ws = env.create_workspace("Q3 supplier review").await;
    let outcome = env
        .service
        .add_link(
            ws.id,
            LinkLocation::Sharepoint {
                location: contracts_folder(),
            },
            None,
        )
        .await;
    assert_eq!(code_of(outcome), "WS_SOURCE_SIGN_IN_NEEDED");
    assert!(env.service.source_links(ws.id).await.unwrap().is_empty());
    assert_eq!(env.service.places(ws.id).await.unwrap().len(), 1);
    assert_eq!(
        std::fs::read_dir(env.service.linked_copies_root())
            .map(|d| d.count())
            .unwrap_or(0),
        0
    );
}

// Scenario: Error path — a protected folder is refused
#[tokio::test]
async fn error_path_a_protected_folder_is_refused() {
    let env = Env::new().await;
    let ws = env.create_workspace("Q3 supplier review").await;
    let outcome = env
        .service
        .add_link(
            ws.id,
            LinkLocation::LocalFolder {
                path: "/System".to_owned(),
            },
            None,
        )
        .await;
    assert_eq!(code_of(outcome), "WS_PLACE_REFUSED");
    let protected = env.protected.to_string_lossy().into_owned();
    let outcome = env
        .service
        .add_link(ws.id, LinkLocation::LocalFolder { path: protected }, None)
        .await;
    assert!(matches!(
        outcome,
        Err(WorkspaceError::PlaceRefused { cause: "protected" })
    ));
    assert_eq!(env.service.places(ws.id).await.unwrap().len(), 1);
}

// Scenario: Error path — a folder without .git is not a repository
#[tokio::test]
async fn error_path_a_folder_without_git_is_not_a_repository() {
    let env = Env::new().await;
    let ws = env.create_workspace("Q3 supplier review").await;
    let notes = env.folder("notes");
    let outcome = env
        .service
        .add_link(
            ws.id,
            LinkLocation::GitRepositoryFolder {
                path: notes.to_string_lossy().into_owned(),
            },
            None,
        )
        .await;
    assert_eq!(code_of(outcome), "WS_NOT_A_REPOSITORY");
}

// Scenario: Happy path — a grant in one workspace gives nothing in another
#[tokio::test]
async fn happy_path_a_grant_in_one_workspace_gives_nothing_in_another() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let b = env.create_workspace("B").await;
    let folder = env.folder("shared");
    std::fs::write(folder.join("a.txt"), "hello").unwrap();
    let path = folder.to_string_lossy().into_owned();
    let in_a = env
        .service
        .add_link(a.id, LinkLocation::LocalFolder { path: path.clone() }, None)
        .await
        .unwrap();
    let in_b = env
        .service
        .add_link(b.id, LinkLocation::LocalFolder { path }, None)
        .await
        .unwrap();
    assert_eq!(in_a.place_id, in_b.place_id);
    let place = in_a.place_id.unwrap();
    let agent = analyst();
    let grant = env
        .service
        .set_grant(a.id, grantee(agent), place, Some(FolderAccess::Read))
        .await
        .unwrap();
    assert!(grant.is_some());
    assert_eq!(
        env.files
            .read_for_agent(agent, a.id, place, "a.txt", 100)
            .await
            .unwrap(),
        b"hello"
    );
    assert!(env
        .files
        .read_for_agent(agent, b.id, place, "a.txt", 100)
        .await
        .is_err());
    assert_eq!(env.service.grants(a.id).await.unwrap().len(), 1);
    assert!(env.service.grants(b.id).await.unwrap().is_empty());
}

// Scenario: Edge case — a folder on this computer is done reading at once (WS-LP-1)
#[tokio::test]
async fn edge_case_a_local_folders_progress_is_done_at_once() {
    let env = Env::new().await;
    let ws = env.create_workspace("A").await;
    let folder = env.folder("notes");
    std::fs::write(folder.join("a.txt"), "hello").unwrap();
    let link = env
        .service
        .add_link(
            ws.id,
            LinkLocation::LocalFolder {
                path: folder.to_string_lossy().into_owned(),
            },
            None,
        )
        .await
        .unwrap();
    let progress = env.service.link_progress(link.id).await.unwrap();
    assert!(progress.done, "{progress:?}");
    assert_eq!(
        (progress.listed, progress.read, progress.not_read),
        (0, 0, 0)
    );
}

// Scenario: Edge case — "can't use" removes a grant
#[tokio::test]
async fn edge_case_cant_use_removes_a_grant() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let folder = env.folder("shared");
    std::fs::write(folder.join("a.txt"), "hello").unwrap();
    let link = env
        .service
        .add_link(
            a.id,
            LinkLocation::LocalFolder {
                path: folder.to_string_lossy().into_owned(),
            },
            None,
        )
        .await
        .unwrap();
    let place = link.place_id.unwrap();
    let agent = analyst();
    env.service
        .set_grant(a.id, grantee(agent), place, Some(FolderAccess::Read))
        .await
        .unwrap();
    let removed = env
        .service
        .set_grant(a.id, grantee(agent), place, None)
        .await
        .unwrap();
    assert!(removed.is_none());
    assert!(env.service.grants(a.id).await.unwrap().is_empty());
    assert!(env
        .files
        .read_for_agent(agent, a.id, place, "a.txt", 100)
        .await
        .is_err());
}

// Scenario: Error path — grant hand-off failure leaves the stored grant unchanged
#[tokio::test]
async fn error_path_grant_hand_off_failure_leaves_the_stored_grant_unchanged() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let folder = env.folder("shared");
    let link = env
        .service
        .add_link(
            a.id,
            LinkLocation::LocalFolder {
                path: folder.to_string_lossy().into_owned(),
            },
            None,
        )
        .await
        .unwrap();
    let place = link.place_id.unwrap();
    let agent = analyst();
    env.service
        .set_grant(a.id, grantee(agent), place, Some(FolderAccess::Read))
        .await
        .unwrap();
    let before = env.service.grants(a.id).await.unwrap();
    // The Agent Folder Manager's table is gone, so the hand-off fails.
    sqlx::query("DROP TABLE workspace_folder_grants")
        .execute(&env.pool)
        .await
        .unwrap();
    let outcome = env
        .service
        .set_grant(
            a.id,
            grantee(agent),
            place,
            Some(FolderAccess::ReadAndChange),
        )
        .await;
    assert_eq!(code_of(outcome), "WS_GRANT_PUBLISH_FAILED");
    assert_eq!(env.service.grants(a.id).await.unwrap(), before);
}

// Scenario: Happy path — an origin change refreshes an unchanged working copy
#[tokio::test]
async fn happy_path_an_origin_change_refreshes_an_unchanged_working_copy() {
    let env = Env::new().await;
    mount_library(&env, &[kestrel("c4")]).await;
    let ws = env.create_workspace("Q3 supplier review").await;
    let link = env
        .service
        .add_link(
            ws.id,
            LinkLocation::Sharepoint {
                location: contracts_folder(),
            },
            None,
        )
        .await
        .unwrap();
    env.wait_read(link.id).await;
    assert_eq!(
        env.service.working_copy_items(link.id).await.unwrap()[0].origin_version_read,
        "c4"
    );
    env.mock.reset().await;
    mount_library(&env, &[kestrel("c5")]).await;
    mount_delta(&env, "T1", vec![kestrel_change("c5")], "T2").await;
    let report = env.service.open(ws.id).await.unwrap();
    assert_eq!(report.origin_changes, 1);
    assert!(report.checks_failed.is_empty());
    let item = &env.service.working_copy_items(link.id).await.unwrap()[0];
    assert_eq!(item.state, WorkingCopyState::Read);
    assert_eq!(item.origin_version_read, "c5");
}

// Scenario: Edge — an item that could not be read is read at the next origin check
// (Internal Logic 4.3)
#[tokio::test]
async fn edge_an_item_left_unread_is_read_at_the_next_origin_check() {
    let env = Env::new().await;
    // The first mock mounted wins: the download fails while the link is first read.
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path_regex(
            r"^/v1\.0/drives/[^/]+/items/[^/]+/content$",
        ))
        .respond_with(wiremock::ResponseTemplate::new(500))
        .mount(&env.mock)
        .await;
    mount_library(&env, &[kestrel("c4")]).await;
    let ws = env.create_workspace("Q3 supplier review").await;
    let link = env
        .service
        .add_link(
            ws.id,
            LinkLocation::Sharepoint {
                location: contracts_folder(),
            },
            None,
        )
        .await
        .unwrap();
    let progress = env.wait_read(link.id).await;
    assert_eq!((progress.read, progress.not_read), (0, 1));
    assert!(env
        .service
        .working_copy_items(link.id)
        .await
        .unwrap()
        .is_empty());
    env.mock.reset().await;
    mount_library(&env, &[kestrel("c4")]).await;
    mount_delta(&env, "T1", vec![], "T2").await;
    let report = env.service.open(ws.id).await.unwrap();
    assert!(report.checks_failed.is_empty());
    let items = env.service.working_copy_items(link.id).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].state, WorkingCopyState::Read);
    assert_eq!(items[0].origin_version_read, "c4");
}

// Scenario: Error path — an origin change during a pending push holds it
#[tokio::test]
async fn error_path_an_origin_change_during_a_pending_push_holds_it() {
    let env = Env::new().await;
    mount_library(&env, &[kestrel("c4")]).await;
    let ws = env.create_workspace("Q3 supplier review").await;
    let link = env
        .service
        .add_link(
            ws.id,
            LinkLocation::Sharepoint {
                location: contracts_folder(),
            },
            None,
        )
        .await
        .unwrap();
    env.wait_read(link.id).await;
    let item = env
        .service
        .working_copy_items(link.id)
        .await
        .unwrap()
        .remove(0);
    env.service
        .set_working_copy_state(item.id, WorkingCopyState::Changed, None)
        .await
        .unwrap();
    env.service
        .set_working_copy_state(item.id, WorkingCopyState::AwaitingPush, None)
        .await
        .unwrap();
    env.mock.reset().await;
    mount_library(&env, &[kestrel("c5")]).await;
    mount_delta(&env, "T1", vec![kestrel_change("c5")], "T2").await;
    env.service.open(ws.id).await.unwrap();
    assert_eq!(
        env.service.working_copy_item(item.id).await.unwrap().state,
        WorkingCopyState::OriginChanged
    );
}

// Scenario: Edge case — an agent's edit marks the item changed
#[tokio::test]
async fn edge_case_an_agents_edit_marks_the_item_changed() {
    let env = Env::new().await;
    mount_library(
        &env,
        &[(
            "F0001".to_owned(),
            "Pricing.pptx".to_owned(),
            "c4".to_owned(),
        )],
    )
    .await;
    let ws = env.create_workspace("Q3 supplier review").await;
    let link = env
        .service
        .add_link(
            ws.id,
            LinkLocation::Sharepoint {
                location: contracts_folder(),
            },
            None,
        )
        .await
        .unwrap();
    env.wait_read(link.id).await;
    let item = env
        .service
        .working_copy_items(link.id)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(item.state, WorkingCopyState::Read);
    env.service
        .handle_place_change(change(
            link.place_id.unwrap(),
            "Pricing.pptx",
            PlaceChangeKind::Modified,
            true,
            Some(2),
        ))
        .await;
    let item = env.service.working_copy_item(item.id).await.unwrap();
    assert_eq!(item.state, WorkingCopyState::Changed);
    assert_eq!(item.local_version, 2);
}

// Scenario: Happy path — moving to the Bin withdraws agents' access and keeps documents
#[tokio::test]
async fn happy_path_moving_to_the_bin_withdraws_agents_access_and_keeps_documents() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let folder = env.folder("shared");
    std::fs::write(folder.join("a.txt"), "hello").unwrap();
    let link = env
        .service
        .add_link(
            a.id,
            LinkLocation::LocalFolder {
                path: folder.to_string_lossy().into_owned(),
            },
            None,
        )
        .await
        .unwrap();
    let place = link.place_id.unwrap();
    let agent = analyst();
    env.service
        .set_grant(a.id, grantee(agent), place, Some(FolderAccess::Read))
        .await
        .unwrap();
    assert!(env
        .files
        .read_for_agent(agent, a.id, place, "a.txt", 100)
        .await
        .is_ok());
    let binned = env.service.move_to_bin(a.id).await.unwrap();
    assert_eq!(binned.state, WorkspaceState::InBin);
    assert!(binned.moved_to_bin_at.is_some());
    assert!(env
        .files
        .read_for_agent(agent, a.id, place, "a.txt", 100)
        .await
        .is_err());
    assert!(folder.join("a.txt").is_file());
}

// Scenario: Happy path — deleting from the Bin removes ordin's records only
#[tokio::test]
async fn happy_path_deleting_from_the_bin_removes_ordins_records_only() {
    let env = Env::new().await;
    mount_library(&env, &[kestrel("c4")]).await;
    let a = env.create_workspace("A").await;
    let link = env
        .service
        .add_link(
            a.id,
            LinkLocation::Sharepoint {
                location: contracts_folder(),
            },
            None,
        )
        .await
        .unwrap();
    env.wait_read(link.id).await;
    let working_copy = env.service.working_copy_dir(link.id);
    assert!(working_copy.is_dir());
    env.service.move_to_bin(a.id).await.unwrap();
    let mut deleted = env.service.deleted();
    env.service.delete_from_bin(a.id).await.unwrap();
    assert_eq!(deleted.recv().await.unwrap(), a.id);
    assert_eq!(env.scalar("SELECT count(*) FROM workspaces").await, 0);
    assert_eq!(env.scalar("SELECT count(*) FROM source_links").await, 0);
    assert_eq!(
        env.scalar("SELECT count(*) FROM working_copy_items").await,
        0
    );
    assert_eq!(env.scalar("SELECT count(*) FROM workspace_places").await, 0);
    assert!(!working_copy.exists());
    assert!(env.documents.join("ordin").join("A").is_dir());
    assert_eq!(env.entries("workspace_forgotten").await.len(), 1);
}

// Scenario: Happy path — a Confluence link becomes active on approval
#[tokio::test]
async fn happy_path_a_confluence_link_becomes_active_on_approval() {
    let env = Env::new().await;
    let ws = env.create_workspace("A").await;
    let (link, approval) = env.proposed_confluence_link(&ws).await;
    // Written through the File Service, so the watched folder's index holds it.
    env.files
        .write_version(ordin_files::WriteVersion {
            place_id: ws.own_folder_place_id,
            path: "rates.md".to_owned(),
            bytes: b"# Rates".to_vec(),
            origin: ordin_files::FileVersionOrigin::Person,
            expected_hash: None,
        })
        .await
        .unwrap();
    env.decide(approval, "approved").await;
    let held = &env.service.confluence_links(ws.id).await.unwrap()[0];
    assert_eq!(held.id, link);
    assert_eq!(held.state, ConfluenceLinkState::Active);
    assert!(held.approved_at.is_some());
    // sync_linked_folder ran: the folder's file became a published copy waiting to go.
    assert_eq!(
        env.files
            .published_copies_via_link(link)
            .await
            .unwrap()
            .len(),
        1
    );
}

// WS-R1-1: one Confluence link is read by its id, in any state; an unknown id is WS_LINK_NOT_FOUND.
// spec: ordin-desktop/components/WorkspaceService#WS-R1-1
#[tokio::test]
async fn happy_path_a_confluence_link_is_read_by_its_id() {
    let env = Env::new().await;
    let ws = env.create_workspace("A").await;
    let (link, _approval) = env.proposed_confluence_link(&ws).await;
    let held = env.service.confluence_link(link).await.unwrap();
    assert_eq!(held.id, link);
    assert_eq!(held.workspace_id, ws.id);
    let unknown = env
        .service
        .confluence_link(ordin_types::ConfluenceLinkId::new())
        .await
        .unwrap_err();
    assert_eq!(unknown.code(), "WS_LINK_NOT_FOUND");
}

// Scenario: Edge case — Not now on the link card leaves nothing linked
#[tokio::test]
async fn edge_case_not_now_on_the_link_card_leaves_nothing_linked() {
    let env = Env::new().await;
    let ws = env.create_workspace("A").await;
    let (_, approval) = env.proposed_confluence_link(&ws).await;
    env.decide(approval, "rejected").await;
    assert!(env
        .service
        .confluence_links(ws.id)
        .await
        .unwrap()
        .is_empty());
}

// Scenario: Error path — work cannot start in an archived workspace
#[tokio::test]
async fn error_path_work_cannot_start_in_an_archived_workspace() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    env.service.archive(a.id).await.unwrap();
    let outcome = env.service.ensure_active(a.id).await;
    assert!(matches!(
        outcome,
        Err(WorkspaceError::NotActive {
            state: WorkspaceState::Archived
        })
    ));
}

// Scenario: Error path — a local folder cannot be linked on the server
#[tokio::test]
async fn error_path_a_local_folder_cannot_be_linked_on_the_server() {
    let env = Env::with(Options {
        on_server: true,
        ..Options::default()
    })
    .await;
    let a = env.create_workspace("A").await;
    let folder = env.folder("data");
    let outcome = env
        .service
        .add_link(
            a.id,
            LinkLocation::LocalFolder {
                path: folder.to_string_lossy().into_owned(),
            },
            None,
        )
        .await;
    assert_eq!(code_of(outcome), "WS_LOCAL_FOLDER_ON_SERVER");
    assert_eq!(env.service.places(a.id).await.unwrap().len(), 1);
}

// Scenario: Happy path — an origin change is broadcast and its version is readable
#[tokio::test]
async fn happy_path_an_origin_change_is_broadcast_and_its_version_is_readable() {
    let env = Env::new().await;
    mount_library(&env, &[kestrel("c4")]).await;
    let a = env.create_workspace("A").await;
    let link = env
        .service
        .add_link(
            a.id,
            LinkLocation::Sharepoint {
                location: contracts_folder(),
            },
            None,
        )
        .await
        .unwrap();
    env.wait_read(link.id).await;
    let mut changes = env.service.subscribe_origin_changes();
    env.mock.reset().await;
    mount_library(&env, &[kestrel("c5")]).await;
    mount_delta(&env, "T1", vec![kestrel_change("c5")], "T2").await;
    env.service.open(a.id).await.unwrap();
    let heard = tokio::time::timeout(Duration::from_secs(5), changes.recv())
        .await
        .expect("OriginChanged is sent")
        .unwrap();
    assert_eq!(heard.workspace_id, a.id);
    assert_eq!(heard.version.version, "c5");
    assert_eq!(heard.version.changed_by.as_deref(), Some("Sam Lee"));
    let held = env
        .service
        .origin_version(a.id, SourceLinkKind::SharepointLocation, "Kestrel MSA.docx")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(held.version, "c5");
    assert!(env
        .service
        .origin_version(a.id, SourceLinkKind::SharepointLocation, "nothing.docx")
        .await
        .unwrap()
        .is_none());
}

// Scenario: Happy path — an agent's edit to a working copy is announced to the Push Service's store
#[tokio::test]
async fn happy_path_an_agents_edit_to_a_working_copy_is_announced_to_the_push_services_store() {
    let env = Env::new().await;
    mount_library(
        &env,
        &[(
            "F0001".to_owned(),
            "Pricing.pptx".to_owned(),
            "c4".to_owned(),
        )],
    )
    .await;
    let a = env.create_workspace("A").await;
    let link = env
        .service
        .add_link(
            a.id,
            LinkLocation::Sharepoint {
                location: contracts_folder(),
            },
            None,
        )
        .await
        .unwrap();
    env.wait_read(link.id).await;
    let item = env
        .service
        .working_copy_items(link.id)
        .await
        .unwrap()
        .remove(0);
    let mut heard = env.service.subscribe_working_copy_changes();
    env.service
        .handle_place_change(change(
            link.place_id.unwrap(),
            "Pricing.pptx",
            PlaceChangeKind::Modified,
            true,
            Some(2),
        ))
        .await;
    assert_eq!(heard.recv().await.unwrap(), item.id);
    let entry = env.service.working_copy_entry(item.id).await.unwrap();
    assert_eq!(Some(entry.place_id), link.place_id);
    assert_eq!(entry.relative_path, "Pricing.pptx");
    assert_eq!(entry.link_kind, SourceLinkKind::SharepointLocation);
}

// Scenario: Happy path — an agent's change in a git folder becomes a working-copy item
#[tokio::test]
async fn happy_path_an_agents_change_in_a_git_folder_becomes_a_working_copy_item() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let folder = env.git_folder("supplier-portal", "main", "", Some("c0ffee"));
    let link = env
        .service
        .add_link(
            a.id,
            LinkLocation::GitRepositoryFolder {
                path: folder.to_string_lossy().into_owned(),
            },
            None,
        )
        .await
        .unwrap();
    let mut heard = env.service.subscribe_working_copy_changes();
    env.service
        .handle_place_change(change(
            link.place_id.unwrap(),
            "docs/rates.md",
            PlaceChangeKind::Modified,
            true,
            Some(1),
        ))
        .await;
    let id = heard.recv().await.unwrap();
    let item = env.service.working_copy_item(id).await.unwrap();
    assert_eq!(item.state, WorkingCopyState::Changed);
    assert_eq!(item.origin_version_read, "c0ffee");
    assert_eq!(item.local_version, 2);
    let entry = env.service.working_copy_entry(id).await.unwrap();
    assert_eq!(entry.git_branch.as_deref(), Some("main"));
}

// Scenario: Edge case — the person's own edit of a local folder records no item
#[tokio::test]
async fn edge_case_the_persons_own_edit_of_a_local_folder_records_no_item() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let folder = env.folder("notes");
    let link = env
        .service
        .add_link(
            a.id,
            LinkLocation::LocalFolder {
                path: folder.to_string_lossy().into_owned(),
            },
            None,
        )
        .await
        .unwrap();
    env.service
        .handle_place_change(change(
            link.place_id.unwrap(),
            "todo.md",
            PlaceChangeKind::Modified,
            false,
            Some(1),
        ))
        .await;
    assert!(env
        .service
        .working_copy_items(link.id)
        .await
        .unwrap()
        .is_empty());
    // An agent's change to a local folder records an item whose version is the file version before it.
    env.service
        .handle_place_change(change(
            link.place_id.unwrap(),
            "todo.md",
            PlaceChangeKind::Modified,
            true,
            Some(3),
        ))
        .await;
    let items = env.service.working_copy_items(link.id).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].origin_version_read, "fver:2");
}

// Scenario: Happy path — a Telegram grant is recorded and then removed
#[tokio::test]
async fn happy_path_a_telegram_grant_is_recorded_and_then_removed() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let agent = analyst();
    env.service
        .set_telegram_grant(a.id, agent, Some(crate::TelegramAccess::ReadTelegram))
        .await
        .unwrap();
    assert_eq!(
        env.service
            .telegram_grant(a.id, agent)
            .await
            .unwrap()
            .unwrap()
            .access,
        crate::TelegramAccess::ReadTelegram
    );
    env.service
        .set_telegram_grant(a.id, agent, None)
        .await
        .unwrap();
    assert!(env
        .service
        .telegram_grant(a.id, agent)
        .await
        .unwrap()
        .is_none());
}

// Scenario: Happy path — opening a workspace is announced
#[tokio::test]
async fn happy_path_opening_a_workspace_is_announced() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let mut opened = env.service.opened();
    env.service.open(a.id).await.unwrap();
    assert_eq!(opened.recv().await.unwrap(), a.id);
}

// Scenario: Happy path — a linked GitHub repository is summarised
#[tokio::test]
async fn happy_path_a_linked_github_repository_is_summarised() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let folder = env.git_folder(
        "pricing",
        "main",
        "[remote \"origin\"]\n\turl = git@github.com:acme/pricing.git\n",
        None,
    );
    env.service
        .add_link(
            a.id,
            LinkLocation::GitRepositoryFolder {
                path: folder.to_string_lossy().into_owned(),
            },
            None,
        )
        .await
        .unwrap();
    let summary = env.service.summary(a.id).await.unwrap();
    assert_eq!(summary.repositories.len(), 1);
    assert_eq!(summary.repositories[0].host, crate::ShareHost::Github);
    assert_eq!(summary.repositories[0].repository, "acme/pricing");
    assert_eq!(summary.places[0], "A");
    assert_eq!(summary.places.len(), 2);
}

// Scenario: Edge case — a repository on another host is left out
#[tokio::test]
async fn edge_case_a_repository_on_another_host_is_left_out() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let folder = env.git_folder(
        "other",
        "main",
        "[remote \"origin\"]\n\turl = https://git.example.net/acme/pricing.git\n",
        None,
    );
    env.service
        .add_link(
            a.id,
            LinkLocation::GitRepositoryFolder {
                path: folder.to_string_lossy().into_owned(),
            },
            None,
        )
        .await
        .unwrap();
    assert!(env
        .service
        .linked_repositories(a.id)
        .await
        .unwrap()
        .is_empty());
}

// Scenario: Happy path — a change the tree shows emits workspace_changed
#[tokio::test]
async fn happy_path_a_change_the_tree_shows_emits_workspace_changed() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let before = env.events("workspace_changed").await.len();
    env.service
        .rename(a.id, "Q3 suppliers".to_owned())
        .await
        .unwrap();
    let events = env.events("workspace_changed").await;
    assert_eq!(events.len(), before + 1);
    assert_eq!(
        events.last().unwrap().data["workspace_id"],
        serde_json::json!(a.id)
    );
}

// Scenario: A chat file is saved in the workspace's Inbox
#[tokio::test]
async fn a_chat_file_is_saved_in_the_workspaces_inbox() {
    let env = Env::new().await;
    let a = env.create_workspace("Q3 supplier review").await;
    let place = env.files.place(a.own_folder_place_id).await.unwrap();
    std::fs::create_dir_all(std::path::Path::new(&place.path).join("Inbox")).unwrap();
    std::fs::write(
        std::path::Path::new(&place.path)
            .join("Inbox")
            .join("quote.pdf"),
        "old",
    )
    .unwrap();
    let saved = env
        .service
        .save_to_inbox(
            a.id,
            "quote.pdf",
            bytes::Bytes::from_static(b"%PDF"),
            InboxOrigin::Chat {
                channel: ChannelKind::Telegram,
            },
        )
        .await
        .unwrap();
    assert_eq!(saved.relative_path, "Inbox/quote (2).pdf");
    assert_eq!(
        saved.display_path,
        "Q3 supplier review › Inbox › quote (2).pdf"
    );
    assert_eq!(
        std::fs::read(
            std::path::Path::new(&place.path)
                .join("Inbox")
                .join("quote (2).pdf")
        )
        .unwrap(),
        b"%PDF"
    );
    let versions = env
        .files
        .versions(a.own_folder_place_id, "Inbox/quote (2).pdf")
        .await
        .unwrap();
    assert!(matches!(
        versions[0].origin,
        ordin_files::FileVersionOrigin::Person
    ));
    assert_eq!(
        env.entries("inbox_file_saved").await[0].data["channel"],
        "telegram"
    );
}

// Scenario: A file name with a path character is refused
#[tokio::test]
async fn a_file_name_with_a_path_character_is_refused() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let outcome = env
        .service
        .save_generated(
            a.id,
            "../x.json",
            bytes::Bytes::from_static(b"{}"),
            RunId::new(),
        )
        .await;
    assert_eq!(code_of(outcome), "WS_FILE_NAME_INVALID");
    let place = env.files.place(a.own_folder_place_id).await.unwrap();
    assert!(!std::path::Path::new(&place.path).join("Results").exists());
}

// Scenario: Origin changes since a time
#[tokio::test]
async fn origin_changes_since_a_time() {
    let env = Env::new().await;
    mount_library(&env, &many_files(2)).await;
    let a = env.create_workspace("A").await;
    let link = env
        .service
        .add_link(
            a.id,
            LinkLocation::Sharepoint {
                location: contracts_folder(),
            },
            None,
        )
        .await
        .unwrap();
    env.wait_read(link.id).await;
    let items = env.service.working_copy_items(link.id).await.unwrap();
    for (item, at) in items
        .iter()
        .zip(["2026-01-10T10:00:00.000Z", "2026-01-10T08:00:00.000Z"])
    {
        sqlx::query("UPDATE working_copy_items SET origin_changed_at = ?2, origin_changed_by = 'Sam Lee' WHERE id = ?1")
            .bind(item.id.to_string())
            .bind(at)
            .execute(&env.pool)
            .await
            .unwrap();
    }
    let since = Timestamp(
        crate::store::parse_ts("2026-01-10T09:00:00.000Z")
            .unwrap()
            .0,
    );
    let found = env.service.origin_changes_since(since).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].origin_path, items[0].origin_path);
    assert!(!found[0].removed);
}

// Scenario: A remote Git folder is linked without a clone
#[tokio::test]
async fn a_remote_git_folder_is_linked_without_a_clone() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let places_before = env.service.places(a.id).await.unwrap().len();
    let link = env
        .service
        .add_link(
            a.id,
            LinkLocation::GitRemote {
                url: "https://github.com/acme/contracts".to_owned(),
                branch: "main".to_owned(),
                path: "handover".to_owned(),
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(link.kind, SourceLinkKind::GitRemote);
    assert_eq!(link.state, SourceLinkState::Active);
    assert!(link.place_id.is_none());
    assert_eq!(
        link.origin_location,
        "git:https://github.com/acme/contracts#main:handover"
    );
    assert_eq!(env.service.places(a.id).await.unwrap().len(), places_before);
    assert!(env.requests().await.is_empty());
    // A check over the workspace touches no origin for it.
    let report = env.service.open(a.id).await.unwrap();
    assert_eq!(report.origin_changes, 0);
}

// Scenario: Happy path — a Slack grant is recorded and then removed
#[tokio::test]
async fn happy_path_a_slack_grant_is_recorded_and_then_removed() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let agent = analyst();
    let before = env.events("workspace_changed").await.len();
    env.service
        .set_slack_grant(a.id, agent, Some(crate::SlackAccess::ReadAndDraftSlack))
        .await
        .unwrap();
    assert_eq!(
        env.service
            .slack_grant(a.id, agent)
            .await
            .unwrap()
            .unwrap()
            .access,
        crate::SlackAccess::ReadAndDraftSlack
    );
    let journalled = env.entries("slack_grant_set").await;
    assert_eq!(journalled[0].data["access"], "read_and_draft_slack");
    assert_eq!(env.events("workspace_changed").await.len(), before + 1);
    env.service
        .set_slack_grant(a.id, agent, None)
        .await
        .unwrap();
    assert!(env
        .service
        .slack_grant(a.id, agent)
        .await
        .unwrap()
        .is_none());
}

// Scenario: Edge case — a grant in one workspace gives nothing in another (Slack)
#[tokio::test]
async fn edge_case_a_slack_grant_in_one_workspace_gives_nothing_in_another() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let b = env.create_workspace("B").await;
    let agent = analyst();
    env.service
        .set_slack_grant(a.id, agent, Some(crate::SlackAccess::ReadSlack))
        .await
        .unwrap();
    assert!(env
        .service
        .slack_grant(b.id, agent)
        .await
        .unwrap()
        .is_none());
}

// Scenario: Error path — a Slack grant in a binned workspace
#[tokio::test]
async fn error_path_a_slack_grant_in_a_binned_workspace() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    env.service.move_to_bin(a.id).await.unwrap();
    let outcome = env
        .service
        .set_slack_grant(a.id, analyst(), Some(crate::SlackAccess::ReadSlack))
        .await;
    assert_eq!(code_of(outcome), "WS_NOT_ACTIVE");
    assert_eq!(env.scalar("SELECT count(*) FROM slack_grants").await, 0);
}

// Scenario: Happy path — removing an agent removes its Slack grants
#[tokio::test]
async fn happy_path_removing_an_agent_removes_its_slack_grants() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let b = env.create_workspace("B").await;
    let agent = analyst();
    env.service
        .set_slack_grant(a.id, agent, Some(crate::SlackAccess::ReadSlack))
        .await
        .unwrap();
    env.service
        .set_slack_grant(b.id, agent, Some(crate::SlackAccess::ReadSlack))
        .await
        .unwrap();
    let removed = env.service.revoke_grantee(grantee(agent)).await.unwrap();
    assert_eq!(removed, 2);
    assert!(env.service.slack_grants(a.id).await.unwrap().is_empty());
    assert!(env.service.slack_grants(b.id).await.unwrap().is_empty());
}

// Scenario: Happy path — removing a link is announced to later modules
#[tokio::test]
async fn happy_path_removing_a_link_is_announced_to_later_modules() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let link = env
        .service
        .add_link(
            a.id,
            LinkLocation::GitRemote {
                url: "https://github.com/acme/contracts".to_owned(),
                branch: "main".to_owned(),
                path: String::new(),
            },
            None,
        )
        .await
        .unwrap();
    let mut removed = env.service.links_removed();
    let before = env.events("workspace_changed").await.len();
    env.service.remove_link(link.id).await.unwrap();
    assert_eq!(removed.recv().await.unwrap(), (a.id, link.id));
    assert!(env.service.source_links(a.id).await.unwrap().is_empty());
    assert_eq!(env.events("workspace_changed").await.len(), before + 1);
}

/// Keeps the timing helper in use for scenarios that wait on a background task.
#[allow(dead_code)]
async fn settle() {
    tokio::time::sleep(Duration::from_millis(20)).await;
}

// Scenario: Happy path — a linked folder keeps its plain form on Windows (WS-PATH-1)
// spec: ordin-desktop/components/WorkspaceService#WS-PATH-1
#[cfg(windows)]
#[tokio::test]
async fn happy_path_a_linked_folder_keeps_its_plain_form_on_windows() {
    let env = Env::new().await;
    let ws = env.create_workspace("Q3 supplier review").await;
    // The fixture's folder is in its plain form; the operating system resolves it to `\\?\C:\...`.
    let plain = env.folder("notes");
    let verbatim = std::fs::canonicalize(&plain).unwrap();
    assert!(
        verbatim.to_str().unwrap().starts_with(r"\\?\"),
        "{verbatim:?}"
    );
    let link = env
        .service
        .add_link(
            ws.id,
            LinkLocation::LocalFolder {
                path: verbatim.to_string_lossy().into_owned(),
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(link.kind, SourceLinkKind::LocalFolder);
    assert!(
        !link.origin_location.starts_with(r"\\?\"),
        "{}",
        link.origin_location
    );
    assert_eq!(link.origin_location, plain.to_str().unwrap());
    let place = env.files.place(link.place_id.unwrap()).await.unwrap();
    assert_eq!(place.path, plain.to_str().unwrap());
}
