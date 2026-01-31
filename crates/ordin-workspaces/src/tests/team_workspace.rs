//! The amendment "A team workspace (ADR-121)" of WorkspaceService.md: one test per row of its Error
//! Table (named after the code), one per Gherkin scenario (named after it), the migration test of
//! its Data Model, and the rules WS-TW-2, WS-TW-8 and WS-TW-9.

use ordin_files::{PlaceChange, PlaceChangeKind, WriteVersion};
use ordin_types::domain::places::{
    FileVersionOrigin, FolderAccess, Grantee, SourceLinkKind, SourceLinkState,
};
use ordin_types::time::Timestamp;
use ordin_types::wire::workflow_file::AgentRepositoryRef;
use ordin_types::{AgentInstanceId, PlaceId, RunId, TeamGrantId, WorkspaceId};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

use super::support::*;
use crate::{
    check_result_path, GrantDecision, NewPendingGrant, PendingAccess, PendingGrantState, Reach,
    WorkspaceError, TEAM_RESULTS_NAME,
};

fn code<T: std::fmt::Debug>(result: Result<T, WorkspaceError>) -> &'static str {
    result.expect_err("an error").code()
}

/// A workspace with its results folder.
async fn team(env: &Env, name: &str) -> (ordin_workspaces::Workspace, PlaceId) {
    let ws = env.create_workspace(name).await;
    let place = env.service.make_results_folder(ws.id).await.unwrap();
    (ws, place.id)
}

/// An agent of run `run` writes `path` in the results folder, and the File Service reports it.
async fn agent_writes(env: &Env, place: PlaceId, path: &str, bytes: &[u8], run: RunId) {
    let origin = FileVersionOrigin::Run {
        run_id: run,
        tool: "write".to_owned(),
    };
    let version = env
        .files
        .write_version(WriteVersion {
            place_id: place,
            path: path.to_owned(),
            bytes: bytes.to_vec(),
            origin: origin.clone(),
            expected_hash: None,
        })
        .await
        .unwrap();
    env.service
        .handle_place_change(PlaceChange {
            place_id: place,
            path: path.to_owned(),
            change: PlaceChangeKind::Created,
            observed_at: Timestamp::now(),
            by_ordin: true,
            version: Some(version.version),
            origin: Some(origin),
        })
        .await;
}

fn grant(ws: WorkspaceId, agent_key: &str, place: Option<PlaceId>) -> NewPendingGrant {
    NewPendingGrant {
        workspace_id: ws,
        agent_key: agent_key.to_owned(),
        agent_name: "Contract checker".to_owned(),
        repository: AgentRepositoryRef::ProfileKey("acme-agents".to_owned()),
        agent_id: "contract-checker".to_owned(),
        agent_version: "1.4.0".to_owned(),
        agent_instance_id: None,
        access: vec![PendingAccess {
            place_key: "contracts".to_owned(),
            place_id: place,
            place_name: "Contracts".to_owned(),
            access: FolderAccess::ReadAndChange,
        }],
        added_by: "priya-shah".to_owned(),
        added_by_confirmed: true,
        added_at: Timestamp::now(),
    }
}

fn key() -> String {
    uuid::Uuid::now_v7().to_string()
}

async fn file_names(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

async fn results_dir(env: &Env, place: PlaceId) -> std::path::PathBuf {
    std::path::PathBuf::from(env.files.place(place).await.unwrap().path)
}

// ---- Error Table (rows added)

#[tokio::test]
async fn ws_origin_invalid_for_a_place_not_in_one_of_the_four_forms() {
    let env = Env::new().await;
    let ws = env.create_workspace("A").await;
    let refused = [
        (SourceLinkKind::LocalFolder, "/Users/asha/Documents"),
        (SourceLinkKind::GitRepositoryFolder, "/Users/asha/src/a"),
        (SourceLinkKind::SharepointLocation, "graph:drives/b!x9Fq"),
        (
            SourceLinkKind::SharepointLocation,
            "graph:drives/b!x9Fq/items/01AB?token=x",
        ),
        (SourceLinkKind::ConfluenceLocation, "confluence:blog/42"),
        (SourceLinkKind::ConfluenceLocation, "confluence:page/12a"),
        (
            SourceLinkKind::GitRemote,
            "git:https://github.com/acme/a.git#main:",
        ),
        (
            SourceLinkKind::GitRemote,
            "git:https://github.com/acme/a?x=1#main:",
        ),
        (SourceLinkKind::GitRemote, "git:https://github.com/acme/a"),
        (
            SourceLinkKind::JiraIssue,
            "https://acme.atlassian.net/browse/ABC-12?focusedCommentId=1",
        ),
        (
            SourceLinkKind::JiraIssue,
            "https://acme.atlassian.net/browse/abc-12",
        ),
        (SourceLinkKind::SharepointLocation, "confluence:page/42"),
    ];
    for (kind, origin) in refused {
        assert_eq!(
            code(env.service.check_origin(kind, origin).await),
            "WS_ORIGIN_INVALID",
            "{origin}"
        );
        assert_eq!(
            code(
                env.service
                    .add_link_from_origin(ws.id, kind, origin, "A place")
                    .await
            ),
            "WS_ORIGIN_INVALID",
            "{origin}"
        );
        assert_eq!(
            code(env.service.resolve_origin(kind, origin).await),
            "WS_ORIGIN_INVALID",
            "{origin}"
        );
    }
    assert!(env.service.source_links(ws.id).await.unwrap().is_empty());
    assert!(env.requests().await.is_empty(), "no adapter was asked");
}

#[tokio::test]
async fn ws_not_team_for_a_results_call_on_a_workspace_with_no_results_folder() {
    let env = Env::new().await;
    let ws = env.create_workspace("A").await;
    let s = &env.service;
    assert_eq!(
        code(s.results_of_run(ws.id, RunId::new()).await),
        "WS_NOT_TEAM"
    );
    assert_eq!(code(s.unsent_results(ws.id).await), "WS_NOT_TEAM");
    assert_eq!(code(s.read_result(ws.id, "a.md").await), "WS_NOT_TEAM");
    assert_eq!(
        code(s.mark_results_sent(ws.id, Vec::new()).await),
        "WS_NOT_TEAM"
    );
    assert_eq!(
        code(s.store_team_result(ws.id, "a.md", b"x".to_vec()).await),
        "WS_NOT_TEAM"
    );
    assert_eq!(
        code(s.remove_team_result(ws.id, "a.md").await),
        "WS_NOT_TEAM"
    );
    assert_eq!(
        code(
            s.keep_both_results(ws.id, "a.md", b"x".to_vec(), "a (asha).md")
                .await
        ),
        "WS_NOT_TEAM"
    );
    assert_eq!(s.results_folder(ws.id).await.unwrap(), None);
}

#[tokio::test]
async fn ws_result_path_invalid_for_a_path_that_breaks_the_rule() {
    let env = Env::new().await;
    let (ws, place) = team(&env, "A").await;
    let long = format!("{}/x.md", "a".repeat(1100));
    let refused = [
        "/etc/x.md",
        "../x.md",
        "a/../x.md",
        "./x.md",
        ".hidden/x.md",
        "a\\b.md",
        "C:/x.md",
        "a:b.md",
        "a*b.md",
        "a?b.md",
        "a\"b.md",
        "a<b.md",
        "a>b.md",
        "a|b.md",
        "a\u{0}b.md",
        "a\u{7}b.md",
        "CON.md",
        "notes/aux.txt",
        "com1",
        "Lpt9.log",
        "report.",
        "report ",
        "a//b.md",
        "a/",
        "",
        long.as_str(),
    ];
    let s = &env.service;
    for path in refused {
        assert_eq!(
            code(check_result_path(path)),
            "WS_RESULT_PATH_INVALID",
            "{path:?}"
        );
        assert_eq!(
            code(s.store_team_result(ws.id, path, b"x".to_vec()).await),
            "WS_RESULT_PATH_INVALID",
            "{path:?}"
        );
        assert_eq!(
            code(s.read_result(ws.id, path).await),
            "WS_RESULT_PATH_INVALID"
        );
        assert_eq!(
            code(s.remove_team_result(ws.id, path).await),
            "WS_RESULT_PATH_INVALID"
        );
        assert_eq!(
            code(
                s.mark_results_sent(
                    ws.id,
                    vec![(path.to_owned(), crate::Sha256Hex("0".repeat(64)))]
                )
                .await
            ),
            "WS_RESULT_PATH_INVALID"
        );
        assert_eq!(
            code(
                s.keep_both_results(ws.id, path, b"x".to_vec(), "ok.md")
                    .await
            ),
            "WS_RESULT_PATH_INVALID"
        );
        assert_eq!(
            code(
                s.keep_both_results(ws.id, "ok.md", b"x".to_vec(), path)
                    .await
            ),
            "WS_RESULT_PATH_INVALID"
        );
    }
    assert!(file_names(&results_dir(&env, place).await).await.is_empty());
    for allowed in [
        "Board pack.md",
        "Kestrel quote — comparison.md",
        "2026/Q1/notes v2.txt",
        "console.md",
        "COM10.txt",
        "aux-notes.md",
    ] {
        assert!(check_result_path(allowed).is_ok(), "{allowed}");
    }
}

#[tokio::test]
async fn ws_result_path_taken_for_keep_both_results_with_a_mine_to_already_used() {
    let env = Env::new().await;
    let (ws, place) = team(&env, "A").await;
    let run = RunId::new();
    agent_writes(&env, place, "Board pack.md", b"Asha's", run).await;
    // A row at `mine_to`.
    agent_writes(&env, place, "Board pack (asha-patel).md", b"older", run).await;
    assert_eq!(
        code(
            env.service
                .keep_both_results(
                    ws.id,
                    "Board pack.md",
                    b"Priya's".to_vec(),
                    "Board pack (asha-patel).md"
                )
                .await
        ),
        "WS_RESULT_PATH_TAKEN"
    );
    // A file at `mine_to` with no row.
    std::fs::write(
        results_dir(&env, place)
            .await
            .join("Board pack (asha-patel 2).md"),
        "x",
    )
    .unwrap();
    assert_eq!(
        code(
            env.service
                .keep_both_results(
                    ws.id,
                    "Board pack.md",
                    b"Priya's".to_vec(),
                    "Board pack (asha-patel 2).md"
                )
                .await
        ),
        "WS_RESULT_PATH_TAKEN"
    );
    assert_eq!(
        env.service
            .read_result(ws.id, "Board pack.md")
            .await
            .unwrap(),
        b"Asha's"
    );
}

#[tokio::test]
async fn a_file_whose_name_breaks_the_rule_is_recorded_and_never_listed_as_unsent() {
    let env = Env::new().await;
    let (ws, place) = team(&env, "A").await;
    let run = RunId::new();
    agent_writes(&env, place, ".draft.md", b"draft", run).await;
    agent_writes(&env, place, "Summary.md", b"summary", run).await;
    let recorded = env
        .scalar("SELECT count(*) FROM team_result_files WHERE path = '.draft.md' AND sent_sha256 IS NULL")
        .await;
    assert_eq!(recorded, 1, "recorded, sent_sha256 left as it is");
    let unsent: Vec<String> = env
        .service
        .unsent_results(ws.id)
        .await
        .unwrap()
        .into_iter()
        .map(|f| f.path)
        .collect();
    assert_eq!(unsent, vec!["Summary.md".to_owned()]);
    // A second change to it is recorded again and still never listed.
    agent_writes(&env, place, ".draft.md", b"draft 2", run).await;
    assert_eq!(env.service.unsent_results(ws.id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn ws_grant_not_found_for_deciding_a_grant_that_is_not_waiting() {
    let env = Env::new().await;
    let ws = env.create_workspace("A").await;
    let waiting = env
        .service
        .record_pending_grant(grant(ws.id, &key(), Some(ws.own_folder_place_id)))
        .await
        .unwrap();
    let decided = env
        .service
        .decide_pending_grant(waiting.id, GrantDecision::Declined)
        .await
        .unwrap();
    assert_eq!(decided.state, PendingGrantState::Declined);
    assert!(decided.decided_at.is_some());
    assert_eq!(
        code(
            env.service
                .decide_pending_grant(waiting.id, GrantDecision::Allowed)
                .await
        ),
        "WS_GRANT_NOT_FOUND"
    );
    assert_eq!(
        code(
            env.service
                .decide_pending_grant(TeamGrantId::new(), GrantDecision::Allowed)
                .await
        ),
        "WS_GRANT_NOT_FOUND"
    );
}

#[tokio::test]
async fn src_name_invalid_and_ws_not_active_for_a_waiting_grant() {
    let env = Env::new().await;
    let ws = env.create_workspace("A").await;
    let mut long_name = grant(ws.id, &key(), None);
    long_name.agent_name = "n".repeat(121);
    let mut no_access = grant(ws.id, &key(), None);
    no_access.access.clear();
    let mut bad_key = grant(ws.id, "not-a-key", None);
    bad_key.agent_key = "not-a-key".to_owned();
    for g in [long_name, no_access, bad_key] {
        assert_eq!(
            code(env.service.record_pending_grant(g).await),
            "SRC_NAME_INVALID"
        );
    }
    env.service.move_to_bin(ws.id).await.unwrap();
    assert_eq!(
        code(
            env.service
                .record_pending_grant(grant(ws.id, &key(), None))
                .await
        ),
        "WS_NOT_ACTIVE"
    );
}

#[tokio::test]
async fn ws_not_active_and_ws_file_too_large_keep_their_meanings() {
    let env = Env::new().await;
    let (ws, _) = team(&env, "A").await;
    let big = vec![0u8; 100 * 1024 * 1024 + 1];
    assert_eq!(
        code(env.service.store_team_result(ws.id, "big.bin", big).await),
        "WS_FILE_TOO_LARGE"
    );
    assert_eq!(
        code(env.service.remove_team_result(ws.id, "missing.md").await),
        "WS_FILES_FAILED"
    );
    let other = env.create_workspace("B").await;
    env.service.archive(other.id).await.unwrap();
    assert_eq!(
        code(env.service.make_results_folder(other.id).await),
        "WS_NOT_ACTIVE"
    );
}

// ---- Acceptance Criteria (Gherkin, added)

// Scenario: Happy path — the results folder
// spec: ordin-desktop/components/WorkspaceService#WS-TW-1
#[tokio::test]
async fn happy_path_the_results_folder() {
    let env = Env::new().await;
    let ws = env.create_workspace("Q3 supplier review").await;
    let first = env.service.make_results_folder(ws.id).await.unwrap();
    let second = env.service.make_results_folder(ws.id).await.unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(first.friendly_name, TEAM_RESULTS_NAME);
    assert_eq!(first.kind, ordin_files::PlaceKind::WorkingCopy);
    let expected = env
        .documents
        .join("ordin")
        .join("Linked copies")
        .join(format!("results-{}", ws.id));
    assert_eq!(
        std::path::PathBuf::from(&first.path),
        std::fs::canonicalize(expected)
            .map(ordin_types::paths::plain_form)
            .unwrap()
    );
    let places = env.service.places(ws.id).await.unwrap();
    assert_eq!(places.iter().filter(|p| p.id == first.id).count(), 1);
    assert_eq!(places.len(), 2);
    assert_eq!(
        env.service.results_folder(ws.id).await.unwrap(),
        Some(first.id)
    );
    assert_eq!(env.entries("results_folder_made").await.len(), 1);
}

// Scenario: Happy path — a run's result is recorded with its run
// spec: ordin-desktop/components/WorkspaceService#WS-TW-3
#[tokio::test]
async fn happy_path_a_runs_result_is_recorded_with_its_run() {
    let env = Env::new().await;
    let (ws, place) = team(&env, "A").await;
    let run = RunId::new();
    agent_writes(
        &env,
        place,
        "Kestrel quote — comparison.md",
        b"# Comparison",
        run,
    )
    .await;
    assert_eq!(
        env.service.results_of_run(ws.id, run).await.unwrap(),
        vec!["Kestrel quote — comparison.md".to_owned()]
    );
    let unsent = env.service.unsent_results(ws.id).await.unwrap();
    assert_eq!(unsent.len(), 1);
    assert_eq!(unsent[0].path, "Kestrel quote — comparison.md");
    assert_eq!(unsent[0].run_id, Some(run));
    assert_eq!(unsent[0].size_bytes, 12);
    assert_eq!(
        env.service
            .read_result(ws.id, "Kestrel quote — comparison.md")
            .await
            .unwrap(),
        b"# Comparison"
    );
    // Sent as it is, it is no longer unsent; a removal deletes its row.
    env.service
        .mark_results_sent(
            ws.id,
            vec![(unsent[0].path.clone(), unsent[0].sha256.clone())],
        )
        .await
        .unwrap();
    assert!(env.service.unsent_results(ws.id).await.unwrap().is_empty());
    env.service
        .handle_place_change(PlaceChange {
            place_id: place,
            path: "Kestrel quote — comparison.md".to_owned(),
            change: PlaceChangeKind::Removed,
            observed_at: Timestamp::now(),
            by_ordin: false,
            version: None,
            origin: None,
        })
        .await;
    assert!(env
        .service
        .results_of_run(ws.id, run)
        .await
        .unwrap()
        .is_empty());
}

// WS-TW-3 step 2: a renamed result keeps its run.
#[tokio::test]
async fn a_renamed_result_keeps_its_run() {
    let env = Env::new().await;
    let (ws, place) = team(&env, "A").await;
    let run = RunId::new();
    agent_writes(&env, place, "draft.md", b"text", run).await;
    env.service
        .handle_place_change(PlaceChange {
            place_id: place,
            path: "final.md".to_owned(),
            change: PlaceChangeKind::Renamed {
                from: "draft.md".to_owned(),
            },
            observed_at: Timestamp::now(),
            by_ordin: false,
            version: None,
            origin: None,
        })
        .await;
    assert_eq!(
        env.service.results_of_run(ws.id, run).await.unwrap(),
        vec!["final.md".to_owned()]
    );
}

// Scenario: Edge — a result stored from the team is not sent back
// spec: ordin-desktop/components/WorkspaceService#WS-TW-4
#[tokio::test]
async fn edge_a_result_stored_from_the_team_is_not_sent_back() {
    let env = Env::new().await;
    let (ws, place) = team(&env, "A").await;
    let hash = env
        .service
        .store_team_result(ws.id, "Board pack.md", b"Priya's".to_vec())
        .await
        .unwrap();
    assert!(env.service.unsent_results(ws.id).await.unwrap().is_empty());
    // The change it causes, if reported, is ignored.
    env.service
        .handle_place_change(PlaceChange {
            place_id: place,
            path: "Board pack.md".to_owned(),
            change: PlaceChangeKind::Modified,
            observed_at: Timestamp::now(),
            by_ordin: false,
            version: None,
            origin: None,
        })
        .await;
    assert!(env.service.unsent_results(ws.id).await.unwrap().is_empty());
    assert_eq!(
        std::fs::read(results_dir(&env, place).await.join("Board pack.md")).unwrap(),
        b"Priya's"
    );
    assert_eq!(hash.0.len(), 64);
    // Removed by the team, it goes to ordin's Bin and its row goes.
    env.service
        .remove_team_result(ws.id, "Board pack.md")
        .await
        .unwrap();
    assert!(!results_dir(&env, place)
        .await
        .join("Board pack.md")
        .exists());
    assert_eq!(
        env.scalar("SELECT count(*) FROM team_result_files").await,
        0
    );
}

// Scenario: Security — a waiting grant grants nothing
// spec: ordin-desktop/components/WorkspaceService#WS-TW-5
#[tokio::test]
async fn security_a_waiting_grant_grants_nothing() {
    let env = Env::new().await;
    let ws = env.create_workspace("A").await;
    let contracts = env.folder("Contracts");
    let link = env
        .service
        .add_link(
            ws.id,
            crate::LinkLocation::LocalFolder {
                path: contracts.to_string_lossy().into_owned(),
            },
            None,
        )
        .await
        .unwrap();
    let before = env.service.grants(ws.id).await.unwrap();
    let waiting = env
        .service
        .record_pending_grant(grant(ws.id, &key(), link.place_id))
        .await
        .unwrap();
    assert_eq!(env.service.grants(ws.id).await.unwrap(), before);
    assert!(env.entries("grant_set").await.is_empty(), "no set_grant");
    let listed = env.service.pending_grants(ws.id).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0], waiting);
    assert_eq!(listed[0].state, PendingGrantState::Waiting);
    assert_eq!(
        listed[0].grant.access[0].access,
        FolderAccess::ReadAndChange
    );
    assert_eq!(env.entries("team_grant_waiting").await.len(), 1);
}

// Scenario: Edge — a newer grant for the same agent replaces the waiting one
// spec: ordin-desktop/components/WorkspaceService#WS-TW-5
#[tokio::test]
async fn edge_a_newer_grant_for_the_same_agent_replaces_the_waiting_one() {
    let env = Env::new().await;
    let ws = env.create_workspace("A").await;
    let k = key();
    let g1 = env
        .service
        .record_pending_grant(grant(ws.id, &k, None))
        .await
        .unwrap();
    let mut newer = grant(ws.id, &k, None);
    newer.added_by_confirmed = false;
    let g2 = env.service.record_pending_grant(newer).await.unwrap();
    let waiting = env.service.pending_grants(ws.id).await.unwrap();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].id, g2.id);
    assert!(!waiting[0].grant.added_by_confirmed);
    let replaced = env
        .scalar("SELECT count(*) FROM team_pending_grants WHERE state = 'replaced'")
        .await;
    assert_eq!(replaced, 1);
    assert_eq!(
        code(
            env.service
                .decide_pending_grant(g1.id, GrantDecision::Allowed)
                .await
        ),
        "WS_GRANT_NOT_FOUND"
    );
}

// Scenario: Happy path — a place from a description is linked with this person's sign-in
// spec: ordin-desktop/components/WorkspaceService#WS-TW-6
#[tokio::test]
async fn happy_path_a_place_from_a_description_is_linked_with_this_persons_sign_in() {
    let env = Env::new().await;
    mount_library(&env, &many_files(2)).await;
    let ws = env.create_workspace("A").await;
    let origin = format!("graph:drives/{DRIVE}/items/{FOLDER}");
    let link = env
        .service
        .add_link_from_origin(
            ws.id,
            SourceLinkKind::SharepointLocation,
            &origin,
            "Contracts",
        )
        .await
        .unwrap();
    assert_eq!(link.kind, SourceLinkKind::SharepointLocation);
    assert_eq!(link.state, SourceLinkState::Active);
    assert_eq!(link.display_name, "Contracts");
    assert_eq!(link.origin_location, origin);
    let signed: Vec<String> = env
        .mock
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|r| r.url.path().starts_with("/v1.0/drives/"))
        .filter_map(|r| r.headers.get("authorization"))
        .map(|v| v.to_str().unwrap_or_default().to_owned())
        .collect();
    assert!(!signed.is_empty());
    assert!(signed.iter().all(|v| v == &format!("Bearer {MS_TOKEN}")));
    // The same origin again returns the link (no SRC_DUPLICATE).
    let again = env
        .service
        .add_link_from_origin(
            ws.id,
            SourceLinkKind::SharepointLocation,
            &origin,
            "Contracts",
        )
        .await
        .unwrap();
    assert_eq!(again.id, link.id);
    assert_eq!(
        env.service
            .check_origin(SourceLinkKind::SharepointLocation, &origin)
            .await
            .unwrap()
            .reach,
        Reach::Reachable
    );
}

// Scenario: Happy path — two results of one name are both kept (WS-TW-4)
// spec: ordin-desktop/components/WorkspaceService#WS-TW-4
#[tokio::test]
async fn happy_path_two_results_of_one_name_are_both_kept() {
    let env = Env::new().await;
    let (ws, place) = team(&env, "A").await;
    let run = RunId::new();
    agent_writes(&env, place, "Board pack.md", b"Asha's", run).await;
    let hash = env
        .service
        .keep_both_results(
            ws.id,
            "Board pack.md",
            b"Priya's".to_vec(),
            "Board pack (asha-patel).md",
        )
        .await
        .unwrap();
    assert_eq!(
        env.service
            .read_result(ws.id, "Board pack.md")
            .await
            .unwrap(),
        b"Priya's"
    );
    assert_eq!(
        env.service
            .read_result(ws.id, "Board pack (asha-patel).md")
            .await
            .unwrap(),
        b"Asha's"
    );
    let unsent = env.service.unsent_results(ws.id).await.unwrap();
    assert_eq!(unsent.len(), 1);
    assert_eq!(unsent[0].path, "Board pack (asha-patel).md");
    assert_eq!(unsent[0].run_id, Some(run));
    assert_eq!(
        env.service.results_of_run(ws.id, run).await.unwrap(),
        vec!["Board pack (asha-patel).md".to_owned()]
    );
    let stored: String = sqlx::query_scalar(
        "SELECT synced_sha256 FROM team_result_files WHERE path = 'Board pack.md'",
    )
    .fetch_one(&env.pool)
    .await
    .unwrap();
    assert_eq!(stored, hash.0);
    assert_eq!(env.entries("team_results_both_kept").await.len(), 1);
}

// Scenario Outline: Error — a result path the rule refuses (WS-TW-9)
// spec: ordin-desktop/components/WorkspaceService#WS-TW-9
#[tokio::test]
async fn error_a_result_path_the_rule_refuses() {
    let env = Env::new().await;
    let (ws, place) = team(&env, "A").await;
    for path in [
        "../x.md",
        ".hidden/x.md",
        "a\\b.md",
        "C:/x.md",
        "CON.md",
        "notes/aux.txt",
        "report.",
    ] {
        assert_eq!(
            code(
                env.service
                    .store_team_result(ws.id, path, b"bytes".to_vec())
                    .await
            ),
            "WS_RESULT_PATH_INVALID",
            "{path}"
        );
    }
    let dir = results_dir(&env, place).await;
    assert!(file_names(&dir).await.is_empty(), "no file is written");
    assert!(!dir.parent().unwrap().join("x.md").exists());
    assert_eq!(
        env.scalar("SELECT count(*) FROM team_result_files").await,
        0
    );
}

// Scenario: Happy path — a Confluence space from a description (WS-TW-6)
// spec: ordin-desktop/components/WorkspaceService#WS-TW-6
#[tokio::test]
async fn happy_path_a_confluence_space_from_a_description() {
    let env = Env::new().await;
    wiki_json(
        &env,
        "/api/v2/spaces",
        json!({"results": [{"id": SPACE_ID, "key": "PROC", "name": "Procurement", "_links": {"webui": "/spaces/PROC"}}]}),
    )
    .await;
    wiki_json(
        &env,
        &format!("/api/v2/spaces/{SPACE_ID}/pages"),
        json!({"results": [], "_links": {}}),
    )
    .await;
    let reach = env
        .service
        .check_origin(SourceLinkKind::ConfluenceLocation, "confluence:space/PROC")
        .await
        .unwrap();
    assert_eq!(reach.reach, Reach::Reachable);
    assert!(env
        .requests()
        .await
        .iter()
        .any(|(_, p)| p == &wiki("/api/v2/spaces")));
}

// Scenario: Error — an origin that is not a canonical form
// spec: ordin-desktop/components/WorkspaceService#WS-TW-6
#[tokio::test]
async fn error_an_origin_that_is_not_a_canonical_form() {
    let env = Env::new().await;
    assert_eq!(
        code(
            env.service
                .check_origin(
                    SourceLinkKind::GitRemote,
                    "git:https://x:secret@github.com/acme/a#main:"
                )
                .await
        ),
        "WS_ORIGIN_INVALID"
    );
    // The canonical form of the same place is reachable after parsing.
    assert_eq!(
        env.service
            .check_origin(
                SourceLinkKind::GitRemote,
                "git:https://github.com/acme/a#main:"
            )
            .await
            .unwrap()
            .reach,
        Reach::Reachable
    );
}

// Scenario: Happy path — ending a team workspace keeps the results folder
// spec: ordin-desktop/components/WorkspaceService#WS-TW-7
#[tokio::test]
async fn happy_path_ending_a_team_workspace_keeps_the_results_folder() {
    let env = Env::new().await;
    let (ws, place) = team(&env, "A").await;
    for name in ["a.md", "b.md", "c.md"] {
        env.service
            .store_team_result(ws.id, name, name.as_bytes().to_vec())
            .await
            .unwrap();
    }
    let agent = AgentInstanceId::new();
    env.service
        .set_grant(
            ws.id,
            Grantee::AgentInstance {
                agent_instance_id: agent,
            },
            place,
            Some(FolderAccess::Read),
        )
        .await
        .unwrap();
    let waiting = env
        .service
        .record_pending_grant(grant(ws.id, &key(), Some(place)))
        .await
        .unwrap();
    env.service.end_team(ws.id).await.unwrap();
    // The folder and its files stay in W, with its grant.
    assert!(env
        .service
        .places(ws.id)
        .await
        .unwrap()
        .iter()
        .any(|p| p.id == place));
    assert_eq!(
        file_names(&results_dir(&env, place).await).await,
        vec!["a.md", "b.md", "c.md"]
    );
    assert!(env
        .service
        .grants(ws.id)
        .await
        .unwrap()
        .iter()
        .any(|g| g.place_id == place));
    // The grant is declined.
    assert!(env.service.pending_grants(ws.id).await.unwrap().is_empty());
    let state: String = sqlx::query_scalar("SELECT state FROM team_pending_grants WHERE id = ?1")
        .bind(waiting.id.to_string())
        .fetch_one(&env.pool)
        .await
        .unwrap();
    assert_eq!(state, "declined");
    // save_generated writes to W's own folder again.
    assert_eq!(env.service.results_folder(ws.id).await.unwrap(), None);
    let saved = env
        .service
        .save_generated(
            ws.id,
            "Reply.md",
            bytes::Bytes::from_static(b"x"),
            RunId::new(),
        )
        .await
        .unwrap();
    assert_eq!(saved.place_id, ws.own_folder_place_id);
    assert_eq!(saved.relative_path, "Results/Reply.md");
    assert_eq!(
        env.scalar("SELECT count(*) FROM team_result_files").await,
        0
    );
    assert_eq!(env.entries("team_workspace_ended").await.len(), 1);
}

// ---- Internal Logic

// WS-TW-2: Ordin's saved files go to the results folder's root in a team workspace.
// spec: ordin-desktop/components/WorkspaceService#WS-TW-2
#[tokio::test]
async fn save_generated_writes_to_the_results_folder_of_a_team_workspace() {
    let env = Env::new().await;
    let (ws, place) = team(&env, "Q3 supplier review").await;
    let run = RunId::new();
    let saved = env
        .service
        .save_generated(ws.id, "Reply.md", bytes::Bytes::from_static(b"reply"), run)
        .await
        .unwrap();
    assert_eq!(saved.place_id, place);
    assert_eq!(saved.relative_path, "Reply.md");
    assert_eq!(
        saved.display_path,
        "Q3 supplier review › Team results › Reply.md"
    );
    assert!(results_dir(&env, place).await.join("Reply.md").is_file());
    // The File Service reports the write with its run; WS-TW-3 records it.
    env.service
        .handle_place_change(PlaceChange {
            place_id: place,
            path: "Reply.md".to_owned(),
            change: PlaceChangeKind::Created,
            observed_at: Timestamp::now(),
            by_ordin: true,
            version: Some(1),
            origin: None,
        })
        .await;
    assert_eq!(
        env.service.results_of_run(ws.id, run).await.unwrap(),
        vec!["Reply.md".to_owned()]
    );
}

// WS-TW-6: an adapter's answer read as a reach, and the adapters a computer lacks.
#[tokio::test]
async fn check_origin_reads_an_adapters_refusal_as_its_reach() {
    let env = Env::new().await;
    Mock::given(method("GET"))
        .and(path(graph(&format!("/drives/{DRIVE}/items/{FOLDER}"))))
        .respond_with(ResponseTemplate::new(403))
        .mount(&env.mock)
        .await;
    let origin = format!("graph:drives/{DRIVE}/items/{FOLDER}");
    assert_eq!(
        env.service
            .check_origin(SourceLinkKind::SharepointLocation, &origin)
            .await
            .unwrap()
            .reach,
        Reach::NoAccess
    );
    let bare = Env::with(Options {
        confluence: false,
        jira: false,
        ..Options::default()
    })
    .await;
    for (kind, origin) in [
        (SourceLinkKind::ConfluenceLocation, "confluence:page/4000"),
        (
            SourceLinkKind::JiraIssue,
            "https://acme.atlassian.net/browse/ABC-12",
        ),
    ] {
        assert_eq!(
            bare.service.check_origin(kind, origin).await.unwrap().reach,
            Reach::NotConfigured
        );
    }
}

// Scenario: Happy path — the location an origin names (WS-TW-6 step 3; amended January 2026, the
// user's decision)
// spec: ordin-desktop/components/WorkspaceService#WS-TW-6
#[tokio::test]
async fn happy_path_the_location_an_origin_names() {
    let env = Env::new().await;
    mount_library(&env, &many_files(1)).await;
    let ws = env.create_workspace("A").await;
    let origin = format!("graph:drives/{DRIVE}/items/{FOLDER}");
    let location = env
        .service
        .resolve_origin(SourceLinkKind::SharepointLocation, &origin)
        .await
        .unwrap();
    let crate::LinkLocation::Sharepoint { location } = location else {
        panic!("a SharePoint location, got {location:?}")
    };
    assert_eq!(location.drive_id, DRIVE);
    assert_eq!(location.item_id.as_deref(), Some(FOLDER));
    assert!(location.is_folder);
    assert!(!location.site_id.is_empty());
    // Nothing is linked.
    assert!(env.service.source_links(ws.id).await.unwrap().is_empty());
}

// Scenario: Error — resolving an origin this person cannot open (WS-TW-6 step 3)
// spec: ordin-desktop/components/WorkspaceService#WS-TW-6
#[tokio::test]
async fn error_resolving_an_origin_this_person_cannot_open() {
    let env = Env::new().await;
    Mock::given(method("GET"))
        .and(path(graph(&format!("/drives/{DRIVE}/items/{FOLDER}"))))
        .respond_with(ResponseTemplate::new(403))
        .mount(&env.mock)
        .await;
    let origin = format!("graph:drives/{DRIVE}/items/{FOLDER}");
    assert_eq!(
        code(
            env.service
                .resolve_origin(SourceLinkKind::SharepointLocation, &origin)
                .await
        ),
        "WS_SOURCE_ACCESS_DENIED"
    );
    // A computer with no Confluence Adapter.
    let bare = Env::with(Options {
        confluence: false,
        ..Options::default()
    })
    .await;
    assert_eq!(
        code(
            bare.service
                .resolve_origin(SourceLinkKind::ConfluenceLocation, "confluence:page/4000")
                .await
        ),
        "WS_CONFLUENCE_UNAVAILABLE"
    );
}

// WS-TW-8: removal from the Bin deletes the team workspace's rows.
// spec: ordin-desktop/components/WorkspaceService#WS-TW-8
#[tokio::test]
async fn delete_from_bin_deletes_the_team_workspaces_rows() {
    let env = Env::new().await;
    let (ws, _) = team(&env, "A").await;
    env.service
        .store_team_result(ws.id, "a.md", b"a".to_vec())
        .await
        .unwrap();
    env.service
        .record_pending_grant(grant(ws.id, &key(), None))
        .await
        .unwrap();
    env.service.move_to_bin(ws.id).await.unwrap();
    env.service.delete_from_bin(ws.id).await.unwrap();
    for sql in [
        "SELECT count(*) FROM team_results_folders",
        "SELECT count(*) FROM team_result_files",
        "SELECT count(*) FROM team_pending_grants",
    ] {
        assert_eq!(env.scalar(sql).await, 0, "{sql}");
    }
}

// Data Model: a database at step 3 with 3 links, 40 working-copy items and 5 grants keeps every row
// after step 4.
// spec: ordin-desktop/components/WorkspaceService#WS-TW-1
#[tokio::test]
async fn migration_step_4_keeps_every_row() {
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
            sqlite: &crate::SQLITE_MIGRATIONS[..3],
            postgres: &[],
        })
        .unwrap();
    registry
        .apply(&store, ApplyMode::ApplyPending)
        .await
        .unwrap();
    let pool = store.sqlite().unwrap().clone();
    let ws = WorkspaceId::new();
    let now = Timestamp::now().to_wire();
    sqlx::query("INSERT INTO workspaces (id, name, name_key, own_folder_place_id, state, created_at) VALUES (?1, 'A', 'a', ?2, 'active', ?3)")
        .bind(ws.to_string()).bind(PlaceId::new().to_string()).bind(&now).execute(&pool).await.unwrap();
    let mut places = Vec::new();
    for _ in 0..5 {
        let place = PlaceId::new();
        sqlx::query(
            "INSERT INTO workspace_places (workspace_id, place_id, added_at) VALUES (?1, ?2, ?3)",
        )
        .bind(ws.to_string())
        .bind(place.to_string())
        .bind(&now)
        .execute(&pool)
        .await
        .unwrap();
        places.push(place);
    }
    let kinds = ["local_folder", "sharepoint_location", "jira_issue"];
    let mut links = Vec::new();
    for (n, kind) in kinds.iter().enumerate() {
        let id = ordin_types::SourceLinkId::new();
        sqlx::query("INSERT INTO source_links (id, workspace_id, kind, origin_location, display_name, state, added_at) VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6)")
            .bind(id.to_string()).bind(ws.to_string()).bind(kind).bind(format!("origin-{n}")).bind(format!("Link {n}")).bind(&now)
            .execute(&pool).await.unwrap();
        links.push(id);
    }
    for n in 0..40 {
        sqlx::query("INSERT INTO working_copy_items (id, source_link_id, origin_path, origin_version_read, local_version, state, read_at) VALUES (?1, ?2, ?3, 'c1', 1, 'read', ?4)")
            .bind(ordin_types::WorkingCopyItemId::new().to_string()).bind(links[n % 3].to_string()).bind(format!("file-{n}.md")).bind(&now)
            .execute(&pool).await.unwrap();
    }
    for place in &places {
        sqlx::query("INSERT INTO folder_grants (id, workspace_id, grantee, grantee_key, place_id, access, granted_at) VALUES (?1, ?2, ?3, ?4, ?5, 'read', ?6)")
            .bind(ordin_types::FolderGrantId::new().to_string()).bind(ws.to_string())
            .bind(json!({"kind": "agent_instance", "agent_instance_id": AgentInstanceId::new()}).to_string())
            .bind(format!("agent:{place}")).bind(place.to_string()).bind(&now)
            .execute(&pool).await.unwrap();
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
    assert_eq!(count("SELECT count(*) FROM folder_grants").await, 5);
    assert_eq!(count("SELECT count(*) FROM workspaces").await, 1);
    for table in [
        "team_results_folders",
        "team_result_files",
        "team_pending_grants",
        "team_result_files_run",
        "team_pending_grants_waiting",
    ] {
        let found: i64 = sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE name = ?1")
            .bind(table)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(found, 1, "{table}");
    }
    let broken = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(broken.is_empty());
}
