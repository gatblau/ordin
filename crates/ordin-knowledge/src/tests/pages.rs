//! Gherkin: "Knowledge Service — workspace pages", "Home and Search reads" and the Jira edge
//! (KnowledgeService.md *Acceptance Criteria*; amendment ADR-106).

use ordin_files::PlaceChangeKind;
use ordin_mcp_front::ToolCaller;
use ordin_types::domain::places::{
    CopyDestination, KnowledgeAuthor, KnowledgePageState, SourceLinkKind,
};
use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, RunId, SourceLinkId};
use ordin_workspaces::{OriginChanged, OriginVersion};
use serde_json::json;

use super::rig::Rig;
use crate::error::KnowledgeError;
use crate::store;

const RATE_CARD: &str = "Suppliers/Rate card.xlsx";
const BODY: &str = "## Review\nRates are reviewed each quarter against the purchasing policy.\n\n## Exceptions\nKestrel has a fixed rate until March.";

async fn rated_page(rig: &Rig) -> ordin_types::KnowledgePageId {
    rig.f.files.external_write(rig.place, RATE_CARD, "v1");
    let req = rig.req(
        "How supplier rates are set",
        BODY,
        vec![rig.place_src(RATE_CARD, "7")],
    );
    rig.svc
        .write_page(KnowledgeAuthor::Ordin, req)
        .await
        .unwrap()
}

/// SA-KLINK-04 replaces B-3's lower-case file name (ADR-104): the file is named after the title.
#[tokio::test]
async fn happy_path_ordin_writes_a_page_and_it_is_indexed() {
    let rig = Rig::desktop().await;
    let id = rated_page(&rig).await;
    let text = rig.text("knowledge/How supplier rates are set.md");
    assert!(text.contains(&format!("page_id: {id}")));
    assert!(text.contains("title: How supplier rates are set"));
    assert!(text.contains("written_by: ordin"));
    assert!(text.contains(&format!("location: \"place:{}:{RATE_CARD}\"", rig.place)));
    assert_eq!(rig.row(id).await.state, KnowledgePageState::Current);
    rig.index().await;
    let calls = rig.f.memory.calls_for(id);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].sections.len(), 2);
    assert_eq!(calls[0].version, "1");
    let written = rig
        .journal()
        .await
        .into_iter()
        .find(|(k, _)| k == "knowledge_page_written")
        .unwrap()
        .1;
    let data = written.to_string();
    assert!(!data.contains("How supplier") && !data.contains("Rates are reviewed"));
}

#[tokio::test]
async fn edge_case_a_cited_file_changes_and_the_page_is_marked() {
    let rig = Rig::desktop().await;
    let id = rated_page(&rig).await;
    rig.report(RATE_CARD, PlaceChangeKind::Modified, Some(8))
        .await;
    assert_eq!(rig.row(id).await.state, KnowledgePageState::MayBeOutOfDate);
    // The rate card is at version 8 in the File Service when get_page re-checks it.
    for _ in 0..7 {
        rig.f.files.external_write(rig.place, RATE_CARD, "v");
    }
    let view = rig.svc.get_page(id).await.unwrap();
    assert_eq!(view.changes.len(), 1);
    assert_eq!(view.changes[0].version_read, "7");
    assert_eq!(view.changes[0].newer_version, "8");
}

// FU-106: a lagged change channel (`RecvError::Lagged`) calls `resync`, which checks every live page
// against the File Service. Here no change event reached the Knowledge Service at all, and the
// cited file moved on; `resync` alone marks the page.
// spec: ordin-desktop/components/KnowledgeService#ADR-104
#[tokio::test]
async fn edge_case_a_lagged_channel_is_repaired_by_resync_alone() {
    let rig = Rig::desktop().await;
    let id = rated_page(&rig).await;
    assert_eq!(rig.row(id).await.state, KnowledgePageState::Current);
    for _ in 0..7 {
        rig.f.files.external_write(rig.place, RATE_CARD, "v");
    }
    // No `rig.report(…)`: the events were lost.
    assert_eq!(rig.row(id).await.state, KnowledgePageState::Current);
    rig.svc.resync().await;
    assert_eq!(rig.row(id).await.state, KnowledgePageState::MayBeOutOfDate);
}

#[tokio::test]
async fn leave_it_clears_the_mark_and_the_same_version_does_not_mark_again() {
    let rig = Rig::desktop().await;
    let id = rated_page(&rig).await;
    for _ in 0..7 {
        rig.f.files.external_write(rig.place, RATE_CARD, "v");
    }
    assert_eq!(
        rig.svc.check_page(id).await.unwrap(),
        KnowledgePageState::MayBeOutOfDate
    );
    rig.svc.leave_it(id).await.unwrap();
    assert_eq!(
        rig.svc.check_page(id).await.unwrap(),
        KnowledgePageState::Current
    );
    assert!(rig
        .kinds()
        .await
        .contains(&"knowledge_page_left".to_owned()));
}

async fn out_of_date(rig: &Rig) -> ordin_types::KnowledgePageId {
    let id = rated_page(rig).await;
    rig.report(RATE_CARD, PlaceChangeKind::Modified, Some(8))
        .await;
    id
}

#[tokio::test]
async fn bring_up_to_date_waits_for_the_persons_choice() {
    let rig = Rig::desktop().await;
    let id = out_of_date(&rig).await;
    let new_body = "## Review\nRates are reviewed each month.\n";
    let diff = rig
        .svc
        .propose_update(
            id,
            new_body.to_owned(),
            vec![rig.place_src(RATE_CARD, "8")],
            RunId::new(),
        )
        .await
        .unwrap();
    assert!(diff.changed_lines > 0);
    assert_eq!(rig.row(id).await.body, BODY);
    rig.svc.use_update(id).await.unwrap();
    let row = rig.row(id).await;
    assert_eq!(row.body, new_body);
    assert_eq!(row.sources[0].version_read, "8");
    assert_eq!(row.state, KnowledgePageState::Current);
}

#[tokio::test]
async fn keep_the_page_as_it_is_discards_the_update() {
    let rig = Rig::desktop().await;
    let id = out_of_date(&rig).await;
    rig.svc
        .propose_update(id, "new".to_owned(), vec![], RunId::new())
        .await
        .unwrap();
    rig.svc.keep_page(id).await.unwrap();
    let view = rig.svc.view_of(&rig.row(id).await).await.unwrap();
    assert!(!view.pending_update);
    assert_eq!(rig.row(id).await.state, KnowledgePageState::MayBeOutOfDate);
}

#[tokio::test]
async fn error_path_use_an_update_that_is_not_pending() {
    let rig = Rig::desktop().await;
    let id = out_of_date(&rig).await;
    let e = rig.svc.use_update(id).await.unwrap_err();
    assert_eq!(e.code(), "KNOW_NO_PENDING_UPDATE");
}

#[tokio::test]
async fn an_agents_search_withholds_a_page_citing_a_place_it_cannot_read() {
    let rig = Rig::desktop().await;
    let (_, q) = rig.workspace("Other");
    let a = rig
        .svc
        .write_page(
            KnowledgeAuthor::Ordin,
            rig.req(
                "Rates A",
                "rates for page A",
                vec![super::rig::src(&format!("place:{q}:x.xlsx"), "1")],
            ),
        )
        .await
        .unwrap();
    rig.write("Rates B", "rates for page B").await;
    let analyst = AgentInstanceId::new();
    rig.f
        .rules
        .refused
        .lock()
        .unwrap()
        .insert((analyst, format!("place:{q}:x.xlsx")));
    let out = rig
        .call(
            rig.ctx(ToolCaller::Agent, analyst),
            "knowledge_search_pages",
            json!({"query": "rates"}),
        )
        .await
        .unwrap();
    let results = out["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["title"], "Rates B");
    assert_eq!(out["withheld"], 1);
    let refused = rig
        .call(
            rig.ctx(ToolCaller::Agent, analyst),
            "knowledge_read_page",
            json!({"page_id": a}),
        )
        .await
        .unwrap_err();
    assert_eq!(
        refused.message,
        "No page with that id is available in this workspace."
    );
}

#[tokio::test]
async fn error_path_500_pages_reached() {
    let rig = Rig::desktop().await;
    for i in 0..500 {
        rig.write(&format!("Page {i}"), "x").await;
    }
    let before = rig.f.files.paths(rig.place).len();
    let e = rig
        .svc
        .write_page(KnowledgeAuthor::Ordin, rig.req("One more", "x", vec![]))
        .await
        .unwrap_err();
    assert_eq!(e.code(), "KNOW_PAGE_LIMIT");
    assert_eq!(rig.f.files.paths(rig.place).len(), before);
}

#[tokio::test]
async fn error_path_duplicate_title() {
    let rig = Rig::desktop().await;
    rig.write("How supplier rates are set", "x").await;
    let e = rig
        .svc
        .write_page(
            KnowledgeAuthor::Person,
            rig.req("how supplier rates are set", "y", vec![]),
        )
        .await
        .unwrap_err();
    assert_eq!(e.code(), "KNOW_TITLE_DUPLICATE");
}

#[tokio::test]
async fn remove_and_undo() {
    let rig = Rig::desktop().await;
    let id = rig.write("Rates", "Body text.").await;
    rig.index().await;
    rig.svc.remove_page(id).await.unwrap();
    assert!(rig.f.files.text(rig.place, "knowledge/Rates.md").is_none());
    assert_eq!(rig.row(id).await.state, KnowledgePageState::Removed);
    assert_eq!(rig.f.memory.removed.lock().unwrap().as_slice(), &[id]);
    rig.svc.undo_remove(id).await.unwrap();
    assert!(rig.f.files.text(rig.place, "knowledge/Rates.md").is_some());
    let row = rig.row(id).await;
    assert_eq!(row.state, KnowledgePageState::Current);
    assert_eq!(row.memory_indexed_version, None);
    rig.index().await;
    assert_eq!(rig.f.memory.calls_for(id).len(), 2);
}

#[tokio::test]
async fn the_person_edits_the_page_file_outside_ordin() {
    let rig = Rig::desktop().await;
    let id = rig.write("Rates", "Old body.").await;
    let path = "knowledge/Rates.md";
    let edited = rig
        .text(path)
        .replace("Old body.", "New body from another app.");
    rig.edit_outside(path, &edited).await;
    let row = rig.row(id).await;
    assert_eq!(row.body, "New body from another app.");
    assert!(row.edited_by_person);
}

#[tokio::test]
async fn error_path_the_page_file_cannot_be_written() {
    let rig = Rig::desktop().await;
    rig.f.files.fail_writes(true);
    let e = rig
        .svc
        .write_page(KnowledgeAuthor::Ordin, rig.req("Rates", "x", vec![]))
        .await
        .unwrap_err();
    assert_eq!(e.code(), "KNOW_FILE_UNAVAILABLE");
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_pages")
        .fetch_one(rig.svc.pool().unwrap())
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn memory_indexing_retries_after_a_failure() {
    let rig = Rig::desktop().await;
    let id = rig.write("Rates", "Body.").await;
    *rig.f.memory.fail_index.lock().unwrap() = 1;
    let before = Timestamp::now();
    rig.index().await;
    let row = rig.row(id).await;
    assert_eq!(row.memory_index_attempts, 1);
    let next = row.memory_next_attempt_at.unwrap();
    let delay = (next.0 - before.0).num_seconds();
    assert!((29..=31).contains(&delay), "{delay}");
    rig.exec("UPDATE knowledge_pages SET memory_next_attempt_at = '2000-01-01T00:00:00.000Z'")
        .await;
    rig.index().await;
    let row = rig.row(id).await;
    assert_eq!(row.memory_indexed_version, Some(row.file_version));
}

#[tokio::test(flavor = "current_thread")]
async fn a_source_cannot_be_checked_in_time() {
    let (logs, _guard) = crate::tests::logs::capture();
    let rig = Rig::desktop().await;
    let src = crate::tests::rig::src_kind("sharepoint", "Suppliers › Rate card.xlsx", "7");
    let id = rig
        .svc
        .write_page(KnowledgeAuthor::Ordin, rig.req("Rates", "x", vec![src]))
        .await
        .unwrap();
    *rig.f.workspaces.slow_origins.lock().unwrap() = true;
    let state = rig.svc.check_page(id).await.unwrap();
    assert_eq!(state, KnowledgePageState::Current);
    assert_eq!(logs.named("knowledge_check_incomplete").len(), 1);
}

#[tokio::test]
async fn agents_reference_pages_are_read_only_and_labelled_without_a_version() {
    let rig = Rig::desktop().await;
    let agent = AgentInstanceId::new();
    rig.f.workspaces.agents.lock().unwrap().insert(
        rig.w,
        vec![crate::ports::WorkspaceAgent {
            agent_instance_id: agent,
            display_name: "Contract clause checker".to_owned(),
        }],
    );
    rig.f
        .files
        .add_agent_file(agent, "knowledge/clauses.md", "# Clauses");
    let listing = rig.svc.list_pages(rig.w, None).await.unwrap();
    assert_eq!(listing.from_agents.len(), 1);
    assert_eq!(listing.from_agents[0].title, "clauses");
    assert_eq!(listing.from_agents[0].agent_name, "Contract clause checker");
    assert!(!serde_json::to_string(&listing.from_agents[0])
        .unwrap()
        .contains("version"));
    let text = rig
        .svc
        .read_agent_page(rig.w, agent, "knowledge/clauses.md")
        .await
        .unwrap();
    assert_eq!(text, "# Clauses");
}

#[tokio::test]
async fn publish_to_confluence_names_the_page_file() {
    let rig = Rig::desktop().await;
    let id = rig.write("How supplier rates are set", "x").await;
    rig.svc
        .publish_to_confluence(
            id,
            CopyDestination::ConfluencePage {
                space: "SUP".into(),
                parent_page_id: "1".into(),
            },
            true,
        )
        .await
        .unwrap();
    let req = rig.f.push.requests.lock().unwrap()[0].clone();
    assert_eq!(
        req.page_file,
        Some((
            rig.place,
            "knowledge/How supplier rates are set.md".to_owned()
        ))
    );
}

// ---------------------------------------------------------------- Home and Search

#[tokio::test]
async fn search_across_two_workspaces() {
    let rig = Rig::desktop().await;
    let (w2, _) = rig.workspace("W2");
    rig.write("Rate review", "The rate is reviewed.").await;
    let mut req = rig.req("Rate card notes", "Notes on the rate card.", vec![]);
    req.workspace_id = w2;
    rig.svc
        .write_page(KnowledgeAuthor::Ordin, req)
        .await
        .unwrap();
    let hits = rig
        .svc
        .reader()
        .search_pages(&[rig.w, w2], "rate", 10)
        .await
        .unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().any(|h| h.workspace_id == rig.w));
    assert!(hits.iter().any(|h| h.workspace_id == w2));
}

#[tokio::test]
async fn pages_out_of_date_since_a_time() {
    let rig = Rig::desktop().await;
    let (w2, p2) = rig.workspace("W2");
    let p1 = rig
        .svc
        .write_page(
            KnowledgeAuthor::Ordin,
            rig.req("P1", "x", vec![rig.place_src("a.txt", "1")]),
        )
        .await
        .unwrap();
    let mut req = rig.req(
        "P2",
        "x",
        vec![super::rig::src(&format!("place:{p2}:b.txt"), "1")],
    );
    req.workspace_id = w2;
    let p2_id = rig
        .svc
        .write_page(KnowledgeAuthor::Ordin, req)
        .await
        .unwrap();
    rig.report("a.txt", PlaceChangeKind::Modified, Some(2))
        .await;
    rig.report_in(p2, "b.txt", PlaceChangeKind::Modified, Some(2))
        .await;
    rig.exec(&format!("UPDATE knowledge_source_marks SET marked_at = '2026-01-10T10:00:00.000Z' WHERE page_id = '{p1}'")).await;
    rig.exec(&format!("UPDATE knowledge_source_marks SET marked_at = '2026-01-10T08:00:00.000Z' WHERE page_id = '{p2_id}'")).await;
    let since = Timestamp(
        chrono::DateTime::parse_from_rfc3339("2026-01-10T09:00:00Z")
            .unwrap()
            .into(),
    );
    let pages = rig.svc.reader().out_of_date_since(since).await.unwrap();
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].id, p1);
    assert_eq!(pages[0].workspace_id, rig.w);
}

#[tokio::test]
async fn version_of_a_removed_page() {
    let rig = Rig::desktop().await;
    let p3 = rig.write("P3", "x").await;
    rig.svc.remove_page(p3).await.unwrap();
    assert_eq!(rig.svc.reader().page_version(p3).await.unwrap(), None);
}

#[tokio::test]
async fn error_search_with_no_workspace() {
    let rig = Rig::desktop().await;
    let e = rig
        .svc
        .reader()
        .search_pages(&[], "rate", 10)
        .await
        .unwrap_err();
    assert!(matches!(e, KnowledgeError::QueryInvalid));
}

#[tokio::test]
async fn edge_case_a_jira_origin_change_marks_no_page() {
    let rig = Rig::desktop().await;
    let path = "Jira/ABC-12/summary-and-description.md";
    let id = rig
        .svc
        .write_page(
            KnowledgeAuthor::Ordin,
            rig.req("Issue", "x", vec![rig.place_src(path, "1")]),
        )
        .await
        .unwrap();
    rig.svc
        .on_origin_change(OriginChanged {
            workspace_id: rig.w,
            source_link_id: SourceLinkId::new(),
            link_kind: SourceLinkKind::JiraIssue,
            origin_path: path.to_owned(),
            version: OriginVersion {
                version: "2".into(),
                changed_by: None,
                changed_at: None,
            },
            removed: false,
        })
        .await
        .unwrap();
    assert_eq!(rig.row(id).await.state, KnowledgePageState::Current);
    assert!(rig.f.memory.source_changes.lock().unwrap().is_empty());
}

// FU-103: a workspace this service has not met (no page row, no `workspaces.get` call) is found by
// listing the Workspace Service's workspaces.
#[tokio::test]
async fn a_new_file_in_a_workspace_with_no_pages_yet_is_adopted() {
    let rig = Rig::desktop().await;
    let (other, place) = rig.workspace("Unmet");
    let path = "knowledge/Fresh.md";
    let v = rig.f.files.external_write(place, path, "Fresh text.\n");
    rig.report_in(place, path, PlaceChangeKind::Created, Some(v.version))
        .await;
    let mut conn = rig.svc.conn().await.unwrap();
    let pages = store::workspace_pages(&mut conn, other).await.unwrap();
    assert_eq!(pages.len(), 1, "the file is adopted as a page");
}

#[tokio::test]
async fn the_start_up_scan_adopts_files_of_a_workspace_with_no_pages_yet() {
    let rig = Rig::desktop().await;
    let (other, place) = rig.workspace("Unmet");
    rig.f
        .files
        .external_write(place, "knowledge/Fresh.md", "Fresh text.\n");
    rig.svc.startup_scan().await.unwrap();
    let mut conn = rig.svc.conn().await.unwrap();
    let pages = store::workspace_pages(&mut conn, other).await.unwrap();
    assert_eq!(pages.len(), 1, "the file is adopted as a page");
}

// ---- KS-PC-1: the screens hear when pages change ----

// Scenario: Writing a page sends the event
// spec: ordin-desktop/components/KnowledgeService#KS-PC-1
#[tokio::test]
async fn writing_a_page_sends_knowledge_pages_changed() {
    let rig = Rig::desktop().await;
    rig.write("Budget 2026", "Body text.").await;
    assert_eq!(rig.pages_changed().await, [rig.w.to_string()]);
}

// Scenario: Removing and restoring a page each send the event
// spec: ordin-desktop/components/KnowledgeService#KS-PC-1
#[tokio::test]
async fn removing_and_restoring_a_page_each_send_knowledge_pages_changed() {
    let rig = Rig::desktop().await;
    let id = rig.write("Rates", "Body text.").await;
    rig.svc.remove_page(id).await.unwrap();
    rig.svc.undo_remove(id).await.unwrap();
    let w = rig.w.to_string();
    assert_eq!(rig.pages_changed().await, [w.clone(), w.clone(), w]);
}

// Scenario: Error — a refused write sends nothing
// spec: ordin-desktop/components/KnowledgeService#KS-PC-1
#[tokio::test]
async fn a_refused_write_sends_no_knowledge_pages_changed() {
    let rig = Rig::desktop().await;
    let e = rig
        .svc
        .write_page(KnowledgeAuthor::Ordin, rig.req("", "x", vec![]))
        .await
        .unwrap_err();
    assert!(!e.code().is_empty());
    assert!(rig.pages_changed().await.is_empty());
}
