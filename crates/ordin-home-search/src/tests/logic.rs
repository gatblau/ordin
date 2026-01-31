//! The Internal Logic steps the scenarios do not reach on their own.

use std::time::Duration;

use ordin_files::{IndexCoverage, PlaceChangeKind};
use ordin_types::common::InputUsed;
use ordin_types::domain::places::{PlaceKind, SourceLinkKind, WorkspaceState};
use ordin_types::time::Timestamp;
use ordin_types::{CrewRunId, KnowledgePageId, PlaceId, RunId};

use super::*;
use crate::ports::{confluence_item, sharepoint_item, RemoteChange, VersionRead};
use crate::service::valid_mark_path;
use crate::{HomeView, OpenTarget, RunAgainSubject, RunSummary, VersionCheck};

#[tokio::test]
async fn local_changes_mark_every_holding_workspace_and_removal_clears_them() {
    let env = Env::new().await;
    let (a, shared) = env.workspace("A", WorkspaceState::Active);
    let (b, _) = env.workspace("B", WorkspaceState::Archived);
    let (binned, _) = env.workspace("Binned", WorkspaceState::InBin);
    env.add_place(b, shared, PlaceKind::PersonsFolder);
    env.add_place(binned, shared, PlaceKind::PersonsFolder);
    env.home
        .on_place_change(&place_change(
            shared,
            "x.docx",
            PlaceChangeKind::Created,
            false,
        ))
        .await;
    assert_eq!(env.home.changed_marks(a).await.expect("a").len(), 1);
    assert_eq!(env.home.changed_marks(b).await.expect("b").len(), 1);
    assert!(env
        .home
        .changed_marks(binned)
        .await
        .expect("bin")
        .is_empty());
    env.home
        .on_place_change(&place_change(
            shared,
            "y.docx",
            PlaceChangeKind::Renamed {
                from: "x.docx".to_owned(),
            },
            false,
        ))
        .await;
    let marks = env.home.changed_marks(a).await.expect("a");
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].path, "y.docx");
    env.home
        .on_place_change(&place_change(
            shared,
            "y.docx",
            PlaceChangeKind::Removed,
            false,
        ))
        .await;
    assert!(env.home.changed_marks(a).await.expect("a").is_empty());
}

#[tokio::test]
async fn a_change_to_a_document_just_opened_is_not_marked() {
    let env = Env::new().await;
    let (ws, place) = env.workspace("A", WorkspaceState::Active);
    env.home
        .document_opened(
            ws,
            ordin_types::wire::approval::ContentRef::PlaceFile {
                place_id: place,
                relative_path: "open.docx".to_owned(),
            },
        )
        .await
        .expect("opened");
    env.home
        .on_place_change(&place_change(
            place,
            "open.docx",
            PlaceChangeKind::Modified,
            false,
        ))
        .await;
    assert!(env.home.changed_marks(ws).await.expect("marks").is_empty());
}

#[tokio::test]
async fn start_listens_to_the_file_service_and_a_second_start_does_nothing() {
    let env = Env::new().await;
    let (ws, place) = env.workspace("A", WorkspaceState::Active);
    env.home.start().await;
    env.home.start().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    env.files
        .sender
        .send(place_change(
            place,
            "live.docx",
            PlaceChangeKind::Modified,
            false,
        ))
        .expect("send");
    for _ in 0..100 {
        if !env.home.changed_marks(ws).await.expect("marks").is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the listener did not mark the document");
}

#[tokio::test]
async fn the_poll_skips_a_failed_adapter_and_keeps_its_time() {
    let env = Env::new().await;
    let (ws, _) = env.workspace("A", WorkspaceState::Active);
    let sp = env.link(
        ws,
        SourceLinkKind::SharepointLocation,
        "Docs",
        Some(PlaceId::new()),
    );
    let cf_place = PlaceId::new();
    let cf = env.link(
        ws,
        SourceLinkKind::ConfluenceLocation,
        "Wiki",
        Some(cf_place),
    );
    env.link(ws, SourceLinkKind::SharepointLocation, "Unconfirmed", None);
    *env.sharepoint.fail.lock().expect("lock") = true;
    env.confluence
        .changes
        .lock()
        .expect("lock")
        .push(RemoteChange {
            source_link_id: cf,
            origin_path: "Page.md".to_owned(),
            changed_at: Timestamp::now(),
            removed: false,
        });
    env.home.poll_remote_marks().await;
    assert_eq!(env.home.changed_marks(ws).await.expect("marks").len(), 1);
    assert!(crate::store::last_poll(&env.pool)
        .await
        .expect("poll")
        .is_none());
    assert_eq!(env.sharepoint.asked.lock().expect("lock")[0].len(), 2);
    assert!(env.sharepoint.asked.lock().expect("lock")[0].contains(&sp));
    *env.sharepoint.fail.lock().expect("lock") = false;
    env.home.poll_remote_marks().await;
    assert!(crate::store::last_poll(&env.pool)
        .await
        .expect("poll")
        .is_some());
}

#[tokio::test]
async fn the_personal_server_has_no_confluence_and_skips_it() {
    let env = Env::with_confluence(false).await;
    let (ws, _) = env.workspace("A", WorkspaceState::Active);
    env.link(
        ws,
        SourceLinkKind::ConfluenceLocation,
        "Wiki",
        Some(PlaceId::new()),
    );
    env.home.poll_remote_marks().await;
    assert!(env.confluence.asked.lock().expect("lock").is_empty());
    *env.reader.inputs.lock().expect("lock") = Some(Ok(vec![InputUsed {
        name: "Page".to_owned(),
        location: "confluence:page/123".to_owned(),
        version_read: "7".to_owned(),
    }]));
    let inputs = env
        .home
        .run_again_inputs(RunAgainSubject::Run {
            run_id: RunId::new(),
        })
        .await
        .expect("inputs");
    assert_eq!(inputs[0].check, VersionCheck::CannotCheck);
}

#[tokio::test]
async fn marks_of_workspaces_no_longer_listed_are_deleted_at_the_poll() {
    let env = Env::new().await;
    let (kept, place) = env.workspace("Kept", WorkspaceState::Active);
    let gone = ordin_types::WorkspaceId::new();
    crate::store::upsert_mark(&env.pool, kept, place, "a.md", Timestamp::now())
        .await
        .expect("mark");
    crate::store::upsert_mark(&env.pool, gone, place, "a.md", Timestamp::now())
        .await
        .expect("mark");
    crate::store::set_last_seen(&env.pool, gone, Timestamp::now())
        .await
        .expect("seen");
    env.home.poll_remote_marks().await;
    assert_eq!(env.home.changed_marks(kept).await.expect("kept").len(), 1);
    assert!(env.home.changed_marks(gone).await.expect("gone").is_empty());
    assert!(!crate::store::last_seen_all(&env.pool)
        .await
        .expect("seen")
        .contains_key(&gone));
}

#[tokio::test]
async fn home_opened_and_last_seen_are_kept() {
    let env = Env::new().await;
    let (ws, _) = env.workspace("A", WorkspaceState::Active);
    env.home.home_opened().await.expect("opened");
    assert!(crate::store::home_last_opened(&env.pool)
        .await
        .expect("read")
        .is_some());
    env.home.workspace_opened(ws).await.expect("seen");
    let list = env.workspaces.list.lock().expect("lock").clone();
    let filled = env.home.with_last_seen(list).await.expect("filled");
    assert!(filled
        .iter()
        .find(|w| w.id == ws)
        .expect("ws")
        .last_seen_at
        .is_some());
}

#[tokio::test]
async fn home_groups_changed_documents_and_lists_pages_and_running_work() {
    let env = Env::new().await;
    let (ws, own) = env.workspace("A", WorkspaceState::Active);
    let copy = PlaceId::new();
    env.link(ws, SourceLinkKind::SharepointLocation, "Docs", Some(copy));
    let now = Timestamp::now();
    for p in ["1.docx", "2.docx", "3.docx"] {
        crate::store::upsert_mark(&env.pool, ws, copy, p, now)
            .await
            .expect("mark");
    }
    crate::store::upsert_mark(&env.pool, ws, own, "notes/local.md", now)
        .await
        .expect("mark");
    let page = env.page(ws, "Supplier rates", now);
    *env.knowledge.out_of_date.lock().expect("lock") = Some(vec![page]);
    *env.reader.running.lock().expect("lock") = Some(vec![RunSummary {
        workspace_id: ws,
        run_id: None,
        crew_run_id: Some(CrewRunId::new()),
        title: "Crew review".to_owned(),
        state: "running".to_owned(),
        at: now,
        can_run_again: false,
    }]);
    *env.reader.finished.lock().expect("lock") = Some(vec![RunSummary {
        workspace_id: ws,
        run_id: Some(RunId::new()),
        crew_run_id: None,
        title: "support-agent".to_owned(),
        state: "replied_in_slack".to_owned(),
        at: now,
        can_run_again: false,
    }]);
    let view: HomeView = env.home.home(None).await.expect("home");
    assert_eq!(view.running.len(), 1);
    assert!(matches!(view.running[0].opens, OpenTarget::CrewRun { .. }));
    let lines: Vec<&str> = view
        .since_last_looked
        .iter()
        .map(|r| r.line.as_str())
        .collect();
    assert!(lines.contains(&"3 documents changed in SharePoint"));
    assert!(lines.contains(&"local.md changed"));
    assert!(lines.contains(&"Supplier rates may be out of date"));
    assert!(lines.contains(&"support-agent replied in Slack"));
    // Never opened: every row is new.
    assert_eq!(view.new_count, 4);
}

#[tokio::test]
async fn a_workspace_list_failure_keeps_rows_without_names() {
    let env = Env::new().await;
    let (ws, _) = env.workspace("A", WorkspaceState::Active);
    *env.workspaces.fail_list.lock().expect("lock") = true;
    *env.reader.finished.lock().expect("lock") = Some(vec![RunSummary {
        workspace_id: ws,
        run_id: Some(RunId::new()),
        crew_run_id: None,
        title: "Done".to_owned(),
        state: "completed".to_owned(),
        at: Timestamp::now(),
        can_run_again: true,
    }]);
    let view = env.home.home(Some(ws)).await.expect("home");
    assert_eq!(view.since_last_looked.len(), 1);
    assert_eq!(view.since_last_looked[0].workspace_name, None);
    // Search still answers, from the sources that could be read.
    let answer = env.home.search("Done", None).await.expect("search");
    assert!(answer.go_to.is_empty());
}

#[tokio::test]
async fn search_ranks_groups_and_offers_actions_and_coverage() {
    let env = Env::new().await;
    let (a, place) = env.workspace("Settings review", WorkspaceState::Active);
    let (_binned, _) = env.workspace("Settings old", WorkspaceState::InBin);
    for i in 0..12 {
        env.files.hits.lock().expect("lock").push(file_hit(
            place,
            &format!("dir/settings {i}.md"),
            true,
            None,
        ));
    }
    env.files.hits.lock().expect("lock").push(file_hit(
        place,
        "dir/notes.md",
        false,
        Some("…the settings…"),
    ));
    *env.files.coverage.lock().expect("lock") = IndexCoverage {
        indexed: 41_200,
        total: 58_000,
    };
    env.knowledge.hits.lock().expect("lock").extend([
        env.hit(a, "Settings", "exact"),
        env.hit(a, "Rates", "…settings…"),
    ]);
    let answer = env.home.search("settings", Some(a)).await.expect("search");
    let workspaces: Vec<_> = answer
        .go_to
        .iter()
        .filter(|r| r.kind == GoToKind::Workspace)
        .collect();
    assert_eq!(workspaces.len(), 1, "the Bin's workspace is left out");
    let documents = answer
        .go_to
        .iter()
        .filter(|r| r.kind == GoToKind::Document)
        .count();
    assert_eq!(documents, 8);
    let pages: Vec<_> = answer
        .go_to
        .iter()
        .filter(|r| r.kind == GoToKind::KnowledgePage)
        .collect();
    assert_eq!(pages[0].label, "Settings");
    assert_eq!(pages[0].detail, None, "the current workspace is not named");
    assert_eq!(answer.found.len(), 2);
    assert_eq!(answer.coverage, Some((41_200, 58_000)));
    assert_eq!(answer.actions.len(), 1);
    assert_eq!(
        answer.actions[0].opens,
        OpenTarget::Screen {
            name: "settings".to_owned()
        }
    );
    let docs = env.home.search("doc", None).await.expect("search");
    assert_eq!(docs.actions[0].label, "Add documents");
}

#[tokio::test]
async fn failed_search_sources_are_left_out_and_a_slow_file_index_marks_incomplete() {
    let env = Env::new().await;
    env.workspace("A", WorkspaceState::Active);
    *env.knowledge.fail.lock().expect("lock") = true;
    *env.files.fail.lock().expect("lock") = true;
    let answer = env.home.search("a", None).await.expect("search");
    assert!(answer.complete);
    assert!(answer.found.is_empty());
    *env.files.fail.lock().expect("lock") = false;
    *env.files.delay.lock().expect("lock") = Duration::from_secs(2);
    let answer = env.home.search("a", None).await.expect("search");
    assert!(!answer.complete);
}

#[tokio::test]
async fn a_registered_source_answers_in_its_group() {
    let env = Env::new().await;
    env.home.register_source(std::sync::Arc::new(SlowSource {
        delay: Duration::ZERO,
        results: vec![crate::GoToResult {
            kind: GoToKind::Dashboard,
            label: "Kestrel analyst".to_owned(),
            detail: None,
            workspace_id: None,
            opens: OpenTarget::Item {
                kind: "agent".to_owned(),
                id: "kestrel".to_owned(),
                workspace_id: None,
            },
        }],
    }));
    let answer = env.home.search("Kestrel", None).await.expect("search");
    assert_eq!(answer.go_to.len(), 1);
    assert_eq!(answer.go_to[0].kind, GoToKind::Agent);
    let json = serde_json::to_value(&answer.go_to[0].opens).expect("json");
    assert_eq!(json["kind"], "item");
    assert_eq!(json["item_kind"], "agent");
}

#[tokio::test]
async fn run_again_compares_every_location_form() {
    let env = Env::new().await;
    let place = PlaceId::new();
    let page = KnowledgePageId::new();
    let gone_page = KnowledgePageId::new();
    env.files
        .hashes
        .lock()
        .expect("lock")
        .insert((place, "same.pdf".to_owned()), "h1".to_owned());
    env.knowledge
        .versions
        .lock()
        .expect("lock")
        .insert(page, Some("4".to_owned()));
    {
        let mut sp = env.sharepoint.versions.lock().expect("lock");
        sp.insert(
            "drives/D/items/I".to_owned(),
            VersionRead::Tag("c2".to_owned()),
        );
        sp.insert("drives/D/items/GONE".to_owned(), VersionRead::NotFound);
    }
    env.confluence.versions.lock().expect("lock").insert(
        "attachment/1/2".to_owned(),
        VersionRead::Tag("3".to_owned()),
    );
    let input = |location: String, version: &str| InputUsed {
        name: "in".to_owned(),
        location,
        version_read: version.to_owned(),
    };
    *env.reader.inputs.lock().expect("lock") = Some(Ok(vec![
        input(format!("place:{place}/same.pdf"), "h1"),
        input(format!("place:{place}/missing.pdf"), "h1"),
        input("place:not-a-uuid/x".to_owned(), "h1"),
        input("graph:drives/D/items/I".to_owned(), "c1"),
        input("graph:drives/D/items/GONE".to_owned(), "c1"),
        input("graph:drives/D/items/ERR".to_owned(), "c1"),
        input("confluence:attachment/1/2".to_owned(), "3"),
        input(format!("knowledge:{page}"), "4"),
        input(format!("knowledge:{gone_page}"), "4"),
        input("git:abc".to_owned(), "x"),
    ]));
    let checks: Vec<VersionCheck> = env
        .home
        .run_again_inputs(RunAgainSubject::Run {
            run_id: RunId::new(),
        })
        .await
        .expect("inputs")
        .into_iter()
        .map(|i| i.check)
        .collect();
    use VersionCheck::*;
    assert_eq!(
        checks,
        [
            Same,
            Gone,
            CannotCheck,
            Newer,
            Gone,
            CannotCheck,
            Same,
            Same,
            Gone,
            CannotCheck
        ]
    );
    *env.reader.inputs.lock().expect("lock") = Some(Err(HomeSourceError::Unavailable));
    let error = env
        .home
        .run_again_inputs(RunAgainSubject::Run {
            run_id: RunId::new(),
        })
        .await
        .unwrap_err();
    assert_eq!(error, crate::HomeError::SourceUnavailable);
}

#[test]
fn location_forms_and_mark_paths_are_read_strictly() {
    assert!(sharepoint_item("drives/a/items/b").is_some());
    assert!(sharepoint_item("drives//items/b").is_none());
    assert!(sharepoint_item("sites/a").is_none());
    assert!(confluence_item("page/7").is_some());
    assert!(confluence_item("page/").is_none());
    assert!(confluence_item("attachment/7").is_none());
    assert!(confluence_item("attachment/7/8/9").is_none());
    assert!(valid_mark_path("a/b.md"));
    assert!(!valid_mark_path("a//b.md"));
    assert!(!valid_mark_path("\\x"));
}

#[test]
fn the_wire_forms_match_the_spec_example() {
    let subject = RunAgainSubject::CrewRun {
        crew_run_id: CrewRunId::new(),
    };
    let json = serde_json::to_value(subject).expect("json");
    assert_eq!(json["kind"], "crew_run");
    let check = serde_json::to_value(VersionCheck::CannotCheck).expect("json");
    assert_eq!(check, "cannot_check");
}
