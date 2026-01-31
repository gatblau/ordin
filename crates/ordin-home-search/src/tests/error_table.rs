//! X1: one test per Error Table row of HomeAndSearch.md (the ADR-106 amendment adds none).

use ordin_types::domain::places::WorkspaceState;
use ordin_types::time::Timestamp;
use ordin_types::wire::approval::ContentRef;
use ordin_types::{ApprovalId, CodedError, PlaceId, RepairAction, RunId, WorkspaceId};

use super::*;
use crate::{HomeError, OpenTarget, RunAgainSubject, WaitingItem};

#[tokio::test]
async fn search_text_empty_or_over_200_characters_is_home_query_invalid() {
    let env = Env::new().await;
    for query in ["", "   ", &"k".repeat(201)] {
        let error = env.home.search(query, None).await.unwrap_err();
        assert_eq!(error, HomeError::QueryInvalid);
        assert_eq!(error.code(), "HOME_QUERY_INVALID");
        assert!(!error.retryable());
        assert_eq!(error.repair(), None);
    }
    // 200 characters is accepted.
    env.home
        .search(&"k".repeat(200), None)
        .await
        .expect("200 characters");
}

#[tokio::test]
async fn an_unknown_workspace_filter_is_home_workspace_not_found() {
    let env = Env::new().await;
    env.workspace("Q3 supplier review", WorkspaceState::Active);
    let error = env.home.home(Some(WorkspaceId::new())).await.unwrap_err();
    assert_eq!(error, HomeError::WorkspaceNotFound);
    assert_eq!(error.code(), "HOME_WORKSPACE_NOT_FOUND");
    assert!(!error.retryable());
    assert_eq!(
        env.home
            .needs_you(Some(WorkspaceId::new()))
            .await
            .unwrap_err(),
        HomeError::WorkspaceNotFound
    );
}

#[tokio::test]
async fn run_again_on_an_unknown_run_is_home_run_not_found() {
    let env = Env::new().await;
    let error = env
        .home
        .run_again_inputs(RunAgainSubject::CrewRun {
            crew_run_id: ordin_types::CrewRunId::new(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.code(), "HOME_RUN_NOT_FOUND");
    assert!(!error.retryable());
}

#[tokio::test]
async fn a_dependency_failing_while_building_a_view_leaves_its_section_out() {
    let env = Env::new().await;
    let (ws, _) = env.workspace("Q3 supplier review", WorkspaceState::Active);
    *env.reader.running.lock().expect("lock") = None;
    *env.reader.finished.lock().expect("lock") = None;
    *env.knowledge.out_of_date.lock().expect("lock") = None;
    *env.workspaces.fail_origin.lock().expect("lock") = true;
    *env.reader.waiting.lock().expect("lock") = Some(vec![WaitingItem {
        workspace_id: Some(ws),
        kind: "approval".to_owned(),
        title: "Approve".to_owned(),
        since: Timestamp::now(),
        opens: OpenTarget::Approval {
            workspace_id: ws,
            approval_id: ApprovalId::new(),
        },
    }]);
    let view = env.home.home(None).await.expect("Home still shows");
    assert_eq!(view.waiting.len(), 1);
    assert!(view.running.is_empty());
    assert!(view.since_last_looked.is_empty());
    // The waiting section failing alone leaves the others.
    *env.reader.waiting.lock().expect("lock") = None;
    let view = env.home.home(None).await.expect("Home still shows");
    assert!(view.waiting.is_empty());
}

#[tokio::test]
async fn the_journal_failing_while_listing_needs_you_is_home_source_unavailable() {
    let env = Env::new().await;
    *env.reader.waiting.lock().expect("lock") = None;
    let error = env.home.needs_you(None).await.unwrap_err();
    assert_eq!(error, HomeError::SourceUnavailable);
    assert_eq!(error.code(), "HOME_SOURCE_UNAVAILABLE");
    assert!(error.retryable());
    assert_eq!(error.repair(), Some(RepairAction::TryAgain));
}

#[tokio::test]
async fn one_hosted_agent_unreadable_leaves_its_cards_out_and_no_error() {
    // The program's reader drops that agent's cards and returns the others: Home and Search sees
    // only the items it is given and adds no error.
    let env = Env::new().await;
    let (ws, _) = env.workspace("Q3 supplier review", WorkspaceState::Active);
    *env.reader.waiting.lock().expect("lock") = Some(vec![WaitingItem {
        workspace_id: Some(ws),
        kind: "handover".to_owned(),
        title: "Handed to you".to_owned(),
        since: Timestamp::now(),
        opens: OpenTarget::Handover {
            workspace_id: Some(ws),
            handover_id: ordin_types::HandoverId::new(),
        },
    }]);
    let rows = env.home.needs_you(None).await.expect("no error");
    assert_eq!(rows.len(), 1);
    assert!(rows.iter().all(|r| r.workspace_id.is_some()));
}

#[tokio::test]
async fn own_tables_unreachable_is_home_datastore_unavailable() {
    let env = Env::new().await;
    env.store.close().await;
    let error = env.home.home(None).await.unwrap_err();
    assert_eq!(error, HomeError::DatastoreUnavailable);
    assert_eq!(error.code(), "HOME_DATASTORE_UNAVAILABLE");
    assert!(error.retryable());
    assert_eq!(error.repair(), Some(RepairAction::TryAgain));
    assert_eq!(
        env.home.home_opened().await.unwrap_err(),
        HomeError::DatastoreUnavailable
    );
}

#[tokio::test]
async fn a_malformed_mark_path_is_home_path_invalid() {
    let env = Env::new().await;
    let (ws, _) = env.workspace("Q3 supplier review", WorkspaceState::Active);
    for path in ["", "/etc/passwd", "a/../b", &"x".repeat(4097)] {
        let error = env
            .home
            .document_opened(
                ws,
                ContentRef::PlaceFile {
                    place_id: PlaceId::new(),
                    relative_path: path.to_owned(),
                },
            )
            .await
            .unwrap_err();
        assert_eq!(error, HomeError::Invalid("HOME_PATH_INVALID"));
        assert_eq!(error.code(), "HOME_PATH_INVALID");
        assert!(!error.retryable());
    }
}

#[tokio::test]
async fn a_duplicate_mark_is_absorbed_by_the_upsert() {
    let env = Env::new().await;
    let (ws, place) = env.workspace("Q3 supplier review", WorkspaceState::Active);
    let first = crate::store::upsert_mark(&env.pool, ws, place, "a.docx", Timestamp::now())
        .await
        .expect("first");
    let later = Timestamp(Timestamp::now().0 + chrono::Duration::seconds(5));
    let second = crate::store::upsert_mark(&env.pool, ws, place, "a.docx", later)
        .await
        .expect("second");
    assert!(first);
    assert!(!second);
    let marks = env.home.changed_marks(ws).await.expect("marks");
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].changed_at, later);
    assert_eq!(
        HomeError::Invalid("HOME_MARK_DUPLICATE").code(),
        "HOME_MARK_DUPLICATE"
    );
    let _ = RunId::new();
}
