//! X1: one test per row of the Error Table of WorkspaceService.md (and of its amendments), named
//! after the code it raises. Rows that carry several codes have one test per code.

use super::support::*;
use crate::mail::Table;
use crate::{LinkLocation, NewEmailLink, WorkspaceError};
use ordin_types::domain::places::{
    FolderAccess, FolderGrant, Grantee, SourceLinkKind, WorkingCopyState, WorkspaceState,
};
use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, FolderGrantId, PlaceId, WorkspaceId};

fn code<T: std::fmt::Debug>(result: Result<T, WorkspaceError>) -> &'static str {
    result.expect_err("an error").code()
}

fn local(folder: &std::path::Path) -> LinkLocation {
    LinkLocation::LocalFolder {
        path: folder.to_string_lossy().into_owned(),
    }
}

fn remote_git(path: &str) -> LinkLocation {
    LinkLocation::GitRemote {
        url: "https://github.com/acme/contracts".to_owned(),
        branch: "main".to_owned(),
        path: path.to_owned(),
    }
}

#[tokio::test]
async fn ws_file_name_invalid_for_an_empty_too_long_or_path_name_and_when_no_numbered_name_is_free()
{
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    for name in ["", &"n".repeat(256), "a/b", "..", "a\\b"] {
        let outcome = env
            .service
            .save_to_inbox(
                a.id,
                name,
                bytes::Bytes::from_static(b"x"),
                crate::InboxOrigin::Chat {
                    channel: crate::ChannelKind::Slack,
                },
            )
            .await;
        assert_eq!(code(outcome), "WS_FILE_NAME_INVALID", "{name:?}");
    }
    let place = env.files.place(a.own_folder_place_id).await.unwrap();
    let inbox = std::path::Path::new(&place.path).join("Inbox");
    std::fs::create_dir_all(&inbox).unwrap();
    std::fs::write(inbox.join("x.txt"), "1").unwrap();
    for n in 2..=99 {
        std::fs::write(inbox.join(format!("x ({n}).txt")), "1").unwrap();
    }
    let outcome = env
        .service
        .save_to_inbox(
            a.id,
            "x.txt",
            bytes::Bytes::from_static(b"x"),
            crate::InboxOrigin::Chat {
                channel: crate::ChannelKind::Slack,
            },
        )
        .await;
    assert_eq!(code(outcome), "WS_FILE_NAME_INVALID");
}

#[tokio::test]
async fn ws_file_too_large_for_a_saved_file_over_100_mib() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let big = bytes::Bytes::from(vec![0u8; 100 * 1024 * 1024 + 1]);
    assert_eq!(
        code(
            env.service
                .save_generated(a.id, "big.bin", big, ordin_types::RunId::new())
                .await
        ),
        "WS_FILE_TOO_LARGE"
    );
}

#[tokio::test]
async fn ws_git_remote_invalid_for_a_malformed_address_branch_or_path() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let bad_url = LinkLocation::GitRemote {
        url: "https://github.com/acme".to_owned(),
        branch: "main".to_owned(),
        path: String::new(),
    };
    assert_eq!(
        code(env.service.propose_link(a.id, bad_url, None).await),
        "WS_GIT_REMOTE_INVALID"
    );
    assert_eq!(
        code(
            env.service
                .propose_link(a.id, remote_git("../up"), None)
                .await
        ),
        "WS_GIT_REMOTE_INVALID"
    );
    let bad_branch = LinkLocation::GitRemote {
        url: "https://github.com/acme/contracts".to_owned(),
        branch: String::new(),
        path: String::new(),
    };
    assert_eq!(
        code(env.service.propose_link(a.id, bad_branch, None).await),
        "WS_GIT_REMOTE_INVALID"
    );
}

#[tokio::test]
async fn ws_not_found_for_an_unknown_workspace() {
    let env = Env::new().await;
    let unknown = WorkspaceId::new();
    assert_eq!(code(env.service.get(unknown).await), "WS_NOT_FOUND");
    assert_eq!(
        code(env.service.ensure_active(unknown).await),
        "WS_NOT_FOUND"
    );
    assert_eq!(
        code(env.service.rename(unknown, "x".to_owned()).await),
        "WS_NOT_FOUND"
    );
    assert_eq!(
        code(
            env.service
                .set_slack_grant(unknown, AgentInstanceId::new(), None)
                .await
        ),
        "WS_NOT_FOUND"
    );
}

#[tokio::test]
async fn ws_not_active_for_work_in_an_archived_or_binned_workspace() {
    let env = Env::new().await;
    let archived = env.create_workspace("A").await;
    env.service.archive(archived.id).await.unwrap();
    let binned = env.create_workspace("B").await;
    env.service.move_to_bin(binned.id).await.unwrap();
    assert!(matches!(
        env.service.ensure_active(archived.id).await,
        Err(WorkspaceError::NotActive {
            state: WorkspaceState::Archived
        })
    ));
    assert!(matches!(
        env.service.ensure_active(binned.id).await,
        Err(WorkspaceError::NotActive {
            state: WorkspaceState::InBin
        })
    ));
    assert_eq!(
        code(env.service.add_link(binned.id, remote_git(""), None).await),
        "WS_NOT_ACTIVE"
    );
}

#[tokio::test]
async fn ws_name_empty() {
    let env = Env::new().await;
    assert_eq!(
        code(env.service.create("   ".to_owned()).await),
        "WS_NAME_EMPTY"
    );
}

#[tokio::test]
async fn ws_name_too_long() {
    let env = Env::new().await;
    assert_eq!(
        code(env.service.create("x".repeat(81)).await),
        "WS_NAME_TOO_LONG"
    );
}

#[tokio::test]
async fn ws_name_duplicate_on_create_and_on_rename() {
    let env = Env::new().await;
    env.create_workspace("A").await;
    let b = env.create_workspace("B").await;
    assert_eq!(
        code(env.service.create(" a ".to_owned()).await),
        "WS_NAME_DUPLICATE"
    );
    assert_eq!(
        code(env.service.rename(b.id, "A".to_owned()).await),
        "WS_NAME_DUPLICATE"
    );
    assert!(env.service.rename(b.id, "b".to_owned()).await.is_ok());
}

#[tokio::test]
async fn ws_state_invalid_for_a_lifecycle_change_the_state_does_not_allow() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    assert_eq!(code(env.service.restore(a.id).await), "WS_STATE_INVALID");
    assert_eq!(
        code(env.service.restore_from_bin(a.id).await),
        "WS_STATE_INVALID"
    );
    assert_eq!(
        code(env.service.delete_from_bin(a.id).await),
        "WS_STATE_INVALID"
    );
    env.service.archive(a.id).await.unwrap();
    assert_eq!(code(env.service.archive(a.id).await), "WS_STATE_INVALID");
    env.service.move_to_bin(a.id).await.unwrap();
    assert_eq!(
        code(env.service.move_to_bin(a.id).await),
        "WS_STATE_INVALID"
    );
    assert_eq!(code(env.service.archive(a.id).await), "WS_STATE_INVALID");
}

#[tokio::test]
async fn ws_own_folder_failed_when_the_folder_cannot_be_made() {
    let env = Env::new().await;
    // `Documents/ordin` is a file, so no folder can be made under it.
    std::fs::write(env.documents.join("ordin"), "in the way").unwrap();
    let outcome = env.service.create("A".to_owned()).await;
    let error = outcome.expect_err("an error");
    assert_eq!(error.code(), "WS_OWN_FOLDER_FAILED");
    assert!(error.retryable());
    assert!(env
        .service
        .list(crate::WorkspaceListFilter::NotInBin)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn ws_link_not_found_for_an_unknown_link() {
    let env = Env::new().await;
    let unknown = ordin_types::SourceLinkId::new();
    assert_eq!(
        code(env.service.confirm_link(unknown).await),
        "WS_LINK_NOT_FOUND"
    );
    assert_eq!(
        code(env.service.remove_link(unknown).await),
        "WS_LINK_NOT_FOUND"
    );
    assert_eq!(
        code(env.service.link_progress(unknown).await),
        "WS_LINK_NOT_FOUND"
    );
    assert_eq!(
        code(env.service.decline_link(unknown).await),
        "WS_LINK_NOT_FOUND"
    );
}

#[tokio::test]
async fn ws_link_state_invalid_when_the_link_is_not_in_the_state_the_call_needs() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let proposed = env
        .service
        .propose_link(a.id, remote_git("one"), None)
        .await
        .unwrap();
    assert_eq!(
        code(env.service.remove_link(proposed.id).await),
        "WS_LINK_STATE_INVALID"
    );
    let active = env.service.confirm_link(proposed.id).await.unwrap();
    assert_eq!(
        code(env.service.confirm_link(active.id).await),
        "WS_LINK_STATE_INVALID"
    );
    assert_eq!(
        code(env.service.decline_link(active.id).await),
        "WS_LINK_STATE_INVALID"
    );
}

#[tokio::test]
async fn ws_not_a_repository_for_a_folder_without_git() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let folder = env.folder("plain");
    let outcome = env
        .service
        .propose_link(
            a.id,
            LinkLocation::GitRepositoryFolder {
                path: folder.to_string_lossy().into_owned(),
            },
            None,
        )
        .await;
    assert_eq!(code(outcome), "WS_NOT_A_REPOSITORY");
}

#[tokio::test]
async fn ws_place_refused_for_a_protected_unreadable_or_hidden_folder() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let protected = env
        .service
        .propose_link(a.id, local(&env.protected), None)
        .await;
    assert!(matches!(
        protected,
        Err(WorkspaceError::PlaceRefused { cause: "protected" })
    ));
    let missing = env.dir.path().join("not-there");
    let unreadable = env.service.propose_link(a.id, local(&missing), None).await;
    assert!(matches!(
        unreadable,
        Err(WorkspaceError::PlaceRefused {
            cause: "unreadable"
        })
    ));
    // A hidden place (an agent folder) is never granted here.
    let hidden = env.folder("agents");
    let place = env
        .files
        .add_place(ordin_files::AddPlace {
            path: hidden,
            friendly_name: Some("Agent".to_owned()),
            kind: ordin_files::PlaceKind::AgentFolder,
        })
        .await
        .unwrap();
    env.service.add_membership(a.id, place.id).await.unwrap();
    let outcome = env
        .service
        .set_grant(
            a.id,
            Grantee::AgentInstance {
                agent_instance_id: AgentInstanceId::new(),
            },
            place.id,
            Some(FolderAccess::Read),
        )
        .await;
    assert!(matches!(
        outcome,
        Err(WorkspaceError::PlaceRefused {
            cause: "hidden_place"
        })
    ));
}

#[tokio::test]
async fn src_duplicate_for_a_location_already_linked() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let folder = env.folder("notes");
    env.service
        .add_link(a.id, local(&folder), None)
        .await
        .unwrap();
    assert_eq!(
        code(env.service.add_link(a.id, local(&folder), None).await),
        "SRC_DUPLICATE"
    );
    env.service
        .add_link(a.id, remote_git("x"), None)
        .await
        .unwrap();
    assert_eq!(
        code(env.service.add_link(a.id, remote_git("x"), None).await),
        "SRC_DUPLICATE"
    );
}

#[tokio::test]
async fn src_name_invalid_for_a_name_the_person_gave() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    assert_eq!(
        code(
            env.service
                .propose_link(a.id, remote_git(""), Some("  ".to_owned()))
                .await
        ),
        "SRC_NAME_INVALID"
    );
    assert_eq!(
        code(
            env.service
                .propose_link(a.id, remote_git(""), Some("n".repeat(121)))
                .await
        ),
        "SRC_NAME_INVALID"
    );
}

#[tokio::test]
async fn src_location_invalid_for_an_origin_over_2048_bytes() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let long = "p".repeat(1024);
    let location = LinkLocation::GitRemote {
        url: format!("https://github.com/{}/{}", "o".repeat(900), "n".repeat(900)),
        branch: "b".repeat(255),
        path: long,
    };
    assert_eq!(
        code(env.service.propose_link(a.id, location, None).await),
        "SRC_LOCATION_INVALID"
    );
}

#[tokio::test]
async fn src_place_mismatch_for_a_place_the_kind_does_not_allow() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let place = Some(PlaceId::new());
    assert_eq!(
        code(crate::links::check_place_rule(
            SourceLinkKind::GitRemote,
            place
        )),
        "SRC_PLACE_MISMATCH"
    );
    assert_eq!(
        code(crate::links::check_place_rule(
            SourceLinkKind::LocalFolder,
            None
        )),
        "SRC_PLACE_MISMATCH"
    );
    // A Confluence link of a place that is not in the workspace.
    let other = env.create_workspace("B").await;
    mount_where(&env).await;
    let outcome = env
        .service
        .propose_confluence_link(
            a.id,
            other.own_folder_place_id,
            String::new(),
            parent_location(),
        )
        .await;
    assert_eq!(code(outcome), "SRC_PLACE_MISMATCH");
}

#[tokio::test]
async fn ws_source_sign_in_needed_for_sharepoint_and_confluence() {
    let env = Env::with(Options {
        microsoft: false,
        atlassian: false,
        ..Options::default()
    })
    .await;
    mount_library(&env, &many_files(1)).await;
    let a = env.create_workspace("A").await;
    let outcome = env
        .service
        .add_link(
            a.id,
            LinkLocation::Sharepoint {
                location: contracts_folder(),
            },
            None,
        )
        .await;
    assert_eq!(code(outcome), "WS_SOURCE_SIGN_IN_NEEDED");
    mount_where(&env).await;
    let outcome = env
        .service
        .add_link(
            a.id,
            LinkLocation::Confluence {
                location: parent_location(),
            },
            None,
        )
        .await;
    assert_eq!(code(outcome), "WS_SOURCE_SIGN_IN_NEEDED");
}

#[tokio::test]
async fn ws_source_not_configured_when_confluence_is_not_set_up_for_the_company() {
    let env = Env::new().await;
    env.settings
        .send(ordin_confluence::ConfluenceSettings { site: None })
        .unwrap();
    let a = env.create_workspace("A").await;
    let outcome = env
        .service
        .add_link(
            a.id,
            LinkLocation::Confluence {
                location: parent_location(),
            },
            None,
        )
        .await;
    assert_eq!(code(outcome), "WS_SOURCE_NOT_CONFIGURED");
}

#[tokio::test]
async fn ws_source_access_denied_when_the_origin_refuses_the_person() {
    let env = Env::new().await;
    status_only(
        &env,
        "GET",
        &graph(&format!("/drives/{DRIVE}/items/{FOLDER}")),
        403,
    )
    .await;
    let a = env.create_workspace("A").await;
    let outcome = env
        .service
        .add_link(
            a.id,
            LinkLocation::Sharepoint {
                location: contracts_folder(),
            },
            None,
        )
        .await;
    assert_eq!(code(outcome), "WS_SOURCE_ACCESS_DENIED");
}

#[tokio::test]
async fn ws_source_not_found_when_the_origin_location_is_gone() {
    let env = Env::new().await;
    status_only(
        &env,
        "GET",
        &graph(&format!("/drives/{DRIVE}/items/{FOLDER}")),
        404,
    )
    .await;
    let a = env.create_workspace("A").await;
    let outcome = env
        .service
        .add_link(
            a.id,
            LinkLocation::Sharepoint {
                location: contracts_folder(),
            },
            None,
        )
        .await;
    assert_eq!(code(outcome), "WS_SOURCE_NOT_FOUND");
}

#[tokio::test]
async fn ws_source_unavailable_when_the_origin_cannot_be_reached() {
    let env = Env::new().await;
    status_only(
        &env,
        "GET",
        &graph(&format!("/drives/{DRIVE}/items/{FOLDER}")),
        503,
    )
    .await;
    let a = env.create_workspace("A").await;
    let error = env
        .service
        .add_link(
            a.id,
            LinkLocation::Sharepoint {
                location: contracts_folder(),
            },
            None,
        )
        .await
        .expect_err("an error");
    assert_eq!(error.code(), "WS_SOURCE_UNAVAILABLE");
    assert!(error.retryable());
}

#[tokio::test]
async fn ws_confluence_unavailable_on_the_personal_server_and_when_the_approval_cannot_be_asked() {
    let server = Env::with(Options {
        confluence: false,
        ..Options::default()
    })
    .await;
    let a = server.create_workspace("A").await;
    assert_eq!(
        code(
            server
                .service
                .propose_link(
                    a.id,
                    LinkLocation::Confluence {
                        location: parent_location()
                    },
                    None
                )
                .await
        ),
        "WS_CONFLUENCE_UNAVAILABLE"
    );
    assert_eq!(
        code(
            server
                .service
                .propose_confluence_link(
                    a.id,
                    a.own_folder_place_id,
                    String::new(),
                    parent_location()
                )
                .await
        ),
        "WS_CONFLUENCE_UNAVAILABLE"
    );
    // The audience is readable but the card's own facts are not: no approval can be asked, no row stays.
    let env = Env::new().await;
    mount_restrictions(&env).await;
    let b = env.create_workspace("B").await;
    let error = env
        .service
        .propose_confluence_link(
            b.id,
            b.own_folder_place_id,
            String::new(),
            parent_location(),
        )
        .await
        .expect_err("an error");
    assert_eq!(error.code(), "WS_CONFLUENCE_UNAVAILABLE");
    assert!(error.retryable());
    assert!(env.service.confluence_links(b.id).await.unwrap().is_empty());
}

#[tokio::test]
async fn ws_item_too_large_for_a_read_item_of_a_file_above_the_adapters_limit() {
    let env = Env::new().await;
    // The first mock mounted wins: this file is 300 MiB, above the SharePoint Adapter's 250 MiB.
    let mut big = file_json("BIG1", "Huge.iso", "c4", "/Suppliers/Contracts", "Sam Lee");
    big["size"] = serde_json::json!(300u64 * 1024 * 1024);
    get_json(
        &env,
        &graph(&format!("/drives/{DRIVE}/items/BIG1")),
        200,
        big,
    )
    .await;
    mount_library(
        &env,
        &[("BIG1".to_owned(), "Huge.iso".to_owned(), "c4".to_owned())],
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
    let progress = env.wait_read(link.id).await;
    assert_eq!((progress.read, progress.not_read), (0, 1));
    assert_eq!(
        code(env.service.read_item(link.id, "Huge.iso").await),
        "WS_ITEM_TOO_LARGE"
    );
}

#[tokio::test]
async fn ws_working_copy_transition_invalid_for_a_change_the_table_does_not_allow() {
    let env = Env::new().await;
    mount_library(&env, &many_files(1)).await;
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
    let outcome = env
        .service
        .set_working_copy_state(item.id, WorkingCopyState::Pushed, Some("c9".to_owned()))
        .await;
    assert!(matches!(
        outcome,
        Err(WorkspaceError::WorkingCopyTransitionInvalid {
            from: WorkingCopyState::Read,
            to: WorkingCopyState::Pushed
        })
    ));
    // A push needs the new origin version.
    env.service
        .set_working_copy_state(item.id, WorkingCopyState::Changed, None)
        .await
        .unwrap();
    env.service
        .set_working_copy_state(item.id, WorkingCopyState::AwaitingPush, None)
        .await
        .unwrap();
    assert_eq!(
        code(
            env.service
                .set_working_copy_state(item.id, WorkingCopyState::Pushed, None)
                .await
        ),
        "WCI_VERSION_INVALID"
    );
    let pushed = env
        .service
        .set_working_copy_state(item.id, WorkingCopyState::Pushed, Some("c9".to_owned()))
        .await
        .unwrap();
    assert_eq!(
        (pushed.state, pushed.origin_version_read.as_str()),
        (WorkingCopyState::Pushed, "c9")
    );
}

#[tokio::test]
async fn grant_place_not_in_workspace_for_a_place_of_another_workspace() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let b = env.create_workspace("B").await;
    let outcome = env
        .service
        .set_grant(
            a.id,
            Grantee::AgentInstance {
                agent_instance_id: AgentInstanceId::new(),
            },
            b.own_folder_place_id,
            Some(FolderAccess::Read),
        )
        .await;
    assert_eq!(code(outcome), "GRANT_PLACE_NOT_IN_WORKSPACE");
}

#[tokio::test]
async fn grant_duplicate_is_the_unique_key_and_never_reaches_the_caller_of_set_grant() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let who = Grantee::AgentInstance {
        agent_instance_id: AgentInstanceId::new(),
    };
    let grant = FolderGrant {
        id: FolderGrantId::new(),
        workspace_id: a.id,
        grantee: who,
        place_id: a.own_folder_place_id,
        access: FolderAccess::Read,
        granted_at: Timestamp::now(),
    };
    let key = crate::grants::grantee_key(&who);
    env.service.insert_grant(&grant, &key).await.unwrap();
    let again = FolderGrant {
        id: FolderGrantId::new(),
        ..grant.clone()
    };
    assert_eq!(
        code(env.service.insert_grant(&again, &key).await),
        "GRANT_DUPLICATE"
    );
    // set_grant on the same key is an update.
    let updated = env
        .service
        .set_grant(
            a.id,
            who,
            a.own_folder_place_id,
            Some(FolderAccess::ReadAndChange),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (updated.id, updated.access),
        (grant.id, FolderAccess::ReadAndChange)
    );
}

#[tokio::test]
async fn ws_grant_publish_failed_is_retryable() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    sqlx::query("DROP TABLE workspace_folder_grants")
        .execute(&env.pool)
        .await
        .unwrap();
    let error = env
        .service
        .set_grant(
            a.id,
            Grantee::AgentInstance {
                agent_instance_id: AgentInstanceId::new(),
            },
            a.own_folder_place_id,
            Some(FolderAccess::Read),
        )
        .await
        .expect_err("an error");
    assert_eq!(error.code(), "WS_GRANT_PUBLISH_FAILED");
    assert!(error.retryable());
    assert!(env.service.grants(a.id).await.unwrap().is_empty());
}

#[tokio::test]
async fn cfl_space_invalid_cfl_parent_invalid_and_cfl_audience_invalid_on_a_proposal() {
    let env = Env::new().await;
    mount_where(&env).await;
    let a = env.create_workspace("A").await;
    let mut location = parent_location();
    location.space_key = String::new();
    assert_eq!(
        code(
            env.service
                .propose_confluence_link(a.id, a.own_folder_place_id, String::new(), location)
                .await
        ),
        "CFL_SPACE_INVALID"
    );
    let mut whole_space = parent_location();
    whole_space.page_id = None;
    assert_eq!(
        code(
            env.service
                .propose_confluence_link(a.id, a.own_folder_place_id, String::new(), whole_space)
                .await
        ),
        "CFL_PARENT_INVALID"
    );
    // A folder outside the place is not a folder of it.
    assert_eq!(
        code(
            env.service
                .propose_confluence_link(
                    a.id,
                    a.own_folder_place_id,
                    "../up".to_owned(),
                    parent_location()
                )
                .await
        ),
        "SRC_LOCATION_INVALID"
    );
    assert_eq!(
        code(crate::confluence::check_audience(&"a".repeat(501))),
        "CFL_AUDIENCE_INVALID"
    );
}

#[tokio::test]
async fn cfl_not_approved_for_an_active_link_without_its_approval() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    // The table refuses the row itself.
    let refused = sqlx::query(
        "INSERT INTO confluence_links (id, workspace_id, place_id, folder, confluence_space, parent_page_id, audience, state) \
         VALUES (?1, ?2, ?3, '', 'PROC', '4000', 'Everyone', 'active')",
    )
    .bind(ordin_types::ConfluenceLinkId::new().to_string())
    .bind(a.id.to_string())
    .bind(a.own_folder_place_id.to_string())
    .execute(&env.pool)
    .await;
    assert!(refused.is_err());
    assert_eq!(
        code(crate::confluence::check_approved(
            ordin_types::domain::places::ConfluenceLinkState::Active,
            None,
            None
        )),
        "CFL_NOT_APPROVED"
    );
}

#[tokio::test]
async fn mail_link_fields_mismatch_mail_address_invalid_and_mail_domain_invalid() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let link = |kind, folder: Option<&str>, addresses: &[&str], domains: &[&str]| NewEmailLink {
        kind,
        folder_or_label: folder.map(str::to_owned),
        addresses: addresses.iter().map(|s| (*s).to_owned()).collect(),
        domains: domains.iter().map(|s| (*s).to_owned()).collect(),
        display_name: "Suppliers".to_owned(),
    };
    use crate::EmailLinkKind::{FolderOrLabel, PeopleAndDomains};
    assert_eq!(
        code(
            env.service
                .propose_email_link(a.id, link(FolderOrLabel, None, &[], &[]))
                .await
        ),
        "MAIL_LINK_FIELDS_MISMATCH"
    );
    assert_eq!(
        code(
            env.service
                .propose_email_link(a.id, link(PeopleAndDomains, None, &["nobody"], &[]))
                .await
        ),
        "MAIL_ADDRESS_INVALID"
    );
    assert_eq!(
        code(
            env.service
                .propose_email_link(a.id, link(PeopleAndDomains, None, &[], &["bad domain"]))
                .await
        ),
        "MAIL_DOMAIN_INVALID"
    );
    let ok = env
        .service
        .propose_email_link(
            a.id,
            link(PeopleAndDomains, None, &["Sam@Acme.Example"], &[]),
        )
        .await
        .unwrap();
    assert_eq!(ok.addresses, vec!["sam@acme.example"]);
}

#[tokio::test]
async fn mail_imap_state_invalid_and_mail_grant_duplicate() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    assert_eq!(
        code(crate::check_imap_state(&[("INBOX".to_owned(), 0, -5)])),
        "MAIL_IMAP_STATE_INVALID"
    );
    let agent = AgentInstanceId::new();
    env.service
        .insert_simple(Table::Email, a.id, agent, "read_email")
        .await
        .unwrap();
    assert_eq!(
        code(
            env.service
                .insert_simple(Table::Email, a.id, agent, "read_email")
                .await
        ),
        "MAIL_GRANT_DUPLICATE"
    );
    // set_email_grant on the same pair is an update that keeps the row.
    let updated = env
        .service
        .set_email_grant(a.id, agent, Some(crate::EmailAccess::ReadAndDraftEmail))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.access, crate::EmailAccess::ReadAndDraftEmail);
    assert_eq!(env.service.email_grants(a.id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn ws_files_failed_when_the_file_service_cannot_do_what_was_asked() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    env.files.remove_place(a.own_folder_place_id).await.unwrap();
    let error = env
        .service
        .save_to_inbox(
            a.id,
            "q.pdf",
            bytes::Bytes::from_static(b"x"),
            crate::InboxOrigin::Chat {
                channel: crate::ChannelKind::Telegram,
            },
        )
        .await
        .expect_err("an error");
    assert_eq!(error.code(), "WS_FILES_FAILED");
    assert!(error.retryable());
}

#[tokio::test]
async fn ws_local_folder_on_server() {
    let env = Env::with(Options {
        on_server: true,
        ..Options::default()
    })
    .await;
    let a = env.create_workspace("A").await;
    let folder = env.folder("data");
    assert_eq!(
        code(
            env.service
                .propose_link(
                    a.id,
                    LinkLocation::GitRepositoryFolder {
                        path: folder.to_string_lossy().into_owned()
                    },
                    None
                )
                .await
        ),
        "WS_LOCAL_FOLDER_ON_SERVER"
    );
    // A remote Git folder is allowed on the server.
    assert!(env
        .service
        .add_link(a.id, remote_git("ok"), None)
        .await
        .is_ok());
}

#[tokio::test]
async fn ws_working_copy_item_not_found_for_an_unknown_item() {
    let env = Env::new().await;
    let unknown = ordin_types::WorkingCopyItemId::new();
    assert_eq!(
        code(env.service.working_copy_entry(unknown).await),
        "WS_WORKING_COPY_ITEM_NOT_FOUND"
    );
    assert_eq!(
        code(env.service.working_copy_item(unknown).await),
        "WS_WORKING_COPY_ITEM_NOT_FOUND"
    );
    assert_eq!(
        code(
            env.service
                .set_working_copy_state(unknown, WorkingCopyState::Changed, None)
                .await
        ),
        "WS_WORKING_COPY_ITEM_NOT_FOUND"
    );
}

#[tokio::test]
async fn src_kind_mismatch_for_an_origin_version_of_a_local_or_git_link() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    for kind in [
        SourceLinkKind::LocalFolder,
        SourceLinkKind::GitRepositoryFolder,
        SourceLinkKind::GitRemote,
    ] {
        assert_eq!(
            code(env.service.origin_version(a.id, kind, "x.md").await),
            "SRC_KIND_MISMATCH"
        );
    }
}

#[tokio::test]
async fn wci_duplicate_wci_path_invalid_and_wci_version_invalid() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let link = env
        .service
        .add_link(a.id, remote_git("x"), None)
        .await
        .unwrap();
    let insert = |path: String, version: &'static str| {
        let pool = env.pool.clone();
        async move {
            crate::store::insert_item(
                &pool,
                link.id,
                &path,
                version,
                1,
                WorkingCopyState::Read,
                None,
            )
            .await
        }
    };
    insert("a.md".to_owned(), "c1").await.unwrap();
    assert_eq!(code(insert("a.md".to_owned(), "c1").await), "WCI_DUPLICATE");
    assert_eq!(code(insert(String::new(), "c1").await), "WCI_PATH_INVALID");
    assert_eq!(
        code(insert("b.md".to_owned(), "").await),
        "WCI_VERSION_INVALID"
    );
}

#[tokio::test]
async fn ws_bin_mismatch_for_a_row_whose_bin_time_and_state_disagree() {
    let env = Env::new().await;
    let row = sqlx::query(
        "SELECT ?1 AS id, 'A' AS name, ?2 AS own_folder_place_id, 'in_bin' AS state, NULL AS moved_to_bin_at, ?3 AS created_at",
    )
    .bind(WorkspaceId::new().to_string())
    .bind(PlaceId::new().to_string())
    .bind(Timestamp::now().to_wire())
    .fetch_one(&env.pool)
    .await
    .unwrap();
    assert_eq!(code(crate::store::workspace_row(&row)), "WS_BIN_MISMATCH");
    // The table's own CHECK refuses such a row.
    let a = env.create_workspace("A").await;
    let refused = sqlx::query("UPDATE workspaces SET state = 'in_bin' WHERE id = ?1")
        .bind(a.id.to_string())
        .execute(&env.pool)
        .await;
    assert!(refused.is_err());
}

#[tokio::test]
async fn ws_datastore_unavailable_when_the_tables_cannot_be_reached() {
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    env.pool.close().await;
    let error = env.service.get(a.id).await.expect_err("an error");
    assert_eq!(error.code(), "WS_DATASTORE_UNAVAILABLE");
    assert!(error.retryable());
    assert_eq!(
        code(env.service.list(crate::WorkspaceListFilter::Active).await),
        "WS_DATASTORE_UNAVAILABLE"
    );
}

// The other rows of ADR-106 (site mismatch, read only) are exercised in `jira_scenarios`; a refused
// Jira link asks the stand-in for nothing.
#[tokio::test]
async fn ws_jira_unavailable_and_ws_jira_address_invalid() {
    let server = Env::with(Options {
        jira: false,
        ..Options::default()
    })
    .await;
    let s = server.create_workspace("A").await;
    let location = LinkLocation::JiraIssue {
        address: "https://acme.atlassian.net/browse/ABC-12".to_owned(),
    };
    assert_eq!(
        code(server.service.propose_link(s.id, location, None).await),
        "WS_JIRA_UNAVAILABLE"
    );
    let env = Env::new().await;
    let a = env.create_workspace("A").await;
    let location = LinkLocation::JiraIssue {
        address: "https://acme.atlassian.net/projects/ABC".to_owned(),
    };
    assert_eq!(
        code(env.service.propose_link(a.id, location, None).await),
        "WS_JIRA_ADDRESS_INVALID"
    );
    assert!(env.requests().await.is_empty());
}

#[tokio::test]
async fn a_retry_reads_only_the_unread_items_that_fit_the_link_budget() {
    let env = Env::new().await;
    mount_library(&env, &many_files(3)).await;
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
    assert_eq!(env.wait_read(link.id).await.read, 3);
    let held = crate::store::load_link(&env.pool, link.id).await.unwrap();
    let listed = env.service.walk(&held).await.unwrap();
    // The link has read almost all of its budget in doc-0001; doc-0002 (100 bytes) and doc-0003
    // (1 byte) go back to unread, so only doc-0003 still fits.
    sqlx::query("DELETE FROM working_copy_items WHERE source_link_id = ?1 AND origin_path != 'doc-0001.txt'")
        .bind(link.id.to_string())
        .execute(&env.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE working_copy_items SET size_bytes = ?2 WHERE source_link_id = ?1")
        .bind(link.id.to_string())
        .bind(i64::try_from(crate::LINK_READ_BUDGET - 1).unwrap())
        .execute(&env.pool)
        .await
        .unwrap();
    for (path, size) in [("doc-0002.txt", 100), ("doc-0003.txt", 1)] {
        let item = listed
            .iter()
            .find(|l| l.origin_path == path)
            .expect("listed");
        env.service.keep_unread(link.id, item).await;
        sqlx::query("UPDATE working_copy_unread SET size_bytes = ?3 WHERE source_link_id = ?1 AND origin_path = ?2")
            .bind(link.id.to_string())
            .bind(path)
            .bind(size)
            .execute(&env.pool)
            .await
            .unwrap();
    }
    env.service.retry_unread(&held).await;
    let read: Vec<String> = sqlx::query_scalar(
        "SELECT origin_path FROM working_copy_items WHERE source_link_id = ?1 ORDER BY origin_path",
    )
    .bind(link.id.to_string())
    .fetch_all(&env.pool)
    .await
    .unwrap();
    assert_eq!(read, ["doc-0001.txt", "doc-0003.txt"]);
    let unread: Vec<String> =
        sqlx::query_scalar("SELECT origin_path FROM working_copy_unread WHERE source_link_id = ?1")
            .bind(link.id.to_string())
            .fetch_all(&env.pool)
            .await
            .unwrap();
    assert_eq!(unread, ["doc-0002.txt"]);
}
