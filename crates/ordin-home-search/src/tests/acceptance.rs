//! X2: one test per Gherkin scenario of HomeAndSearch.md (with the ADR-106 amendment), named
//! after it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ordin_files::PlaceChangeKind;
use ordin_types::common::InputUsed;
use ordin_types::domain::places::{SourceLinkKind, WorkspaceState};
use ordin_types::time::Timestamp;
use ordin_types::wire::approval::ContentRef;
use ordin_types::{ApprovalId, ArtefactId, BlockId, HandoverId, PlaceId, RemoteAgentId, RunId};

use super::*;
use crate::ports::RemoteChange;
use crate::{HomeError, OpenTarget, RunAgainSubject, RunSummary, VersionCheck, WaitingItem};

fn finished(ws: WorkspaceId, title: &str, when: Timestamp) -> RunSummary {
    RunSummary {
        workspace_id: ws,
        run_id: Some(RunId::new()),
        crew_run_id: None,
        title: title.to_owned(),
        state: "completed".to_owned(),
        at: when,
        can_run_again: true,
    }
}

// Scenario: Happy path — Home lists what changed since last opened
#[tokio::test]
async fn happy_path_home_lists_what_changed_since_last_opened() {
    let env = Env::new().await;
    let (q3, _) = env.workspace("Q3 supplier review", WorkspaceState::Active);
    env.set_home_opened(at("2026-01-10T09:00:00.000Z")).await;
    *env.reader.finished.lock().expect("lock") = Some(vec![finished(
        q3,
        "Supplier summary",
        at("2026-01-10T10:00:00.000Z"),
    )]);
    let view = env.home.home(None).await.expect("home");
    assert_eq!(view.since_last_looked.len(), 1);
    let row = &view.since_last_looked[0];
    assert!(row.is_new);
    assert!(row.run_again);
    assert_eq!(row.workspace_name.as_deref(), Some("Q3 supplier review"));
    assert_eq!(view.new_count, 1);
}

// Scenario: Edge case — a filtered Home shows one workspace only
#[tokio::test]
async fn edge_case_a_filtered_home_shows_one_workspace_only() {
    let env = Env::new().await;
    let (q3, _) = env.workspace("Q3 supplier review", WorkspaceState::Active);
    let (q4, _) = env.workspace("Q4 forecast", WorkspaceState::Active);
    let now = Timestamp::now();
    *env.reader.finished.lock().expect("lock") = Some(vec![
        finished(q3, "Q3 summary", now),
        finished(q4, "Q4 summary", now),
    ]);
    *env.reader.running.lock().expect("lock") = Some(vec![finished(q4, "Q4 running", now)]);
    *env.reader.waiting.lock().expect("lock") = Some(vec![WaitingItem {
        workspace_id: Some(q4),
        kind: "approval".to_owned(),
        title: "Approve the send".to_owned(),
        since: now,
        opens: OpenTarget::Approval {
            workspace_id: q4,
            approval_id: ApprovalId::new(),
        },
    }]);
    let view = env.home.home(Some(q3)).await.expect("home");
    let rows = view
        .waiting
        .iter()
        .chain(&view.running)
        .chain(&view.since_last_looked);
    for row in rows {
        assert_eq!(row.workspace_id, Some(q3));
    }
    assert_eq!(view.since_last_looked.len(), 1);
}

// Scenario: Happy path — a SharePoint change marks the document until opened
#[tokio::test]
async fn happy_path_a_sharepoint_change_marks_the_document_until_opened() {
    let env = Env::new().await;
    let (ws, _) = env.workspace("Q3 supplier review", WorkspaceState::Active);
    let copy = PlaceId::new();
    let link = env.link(
        ws,
        SourceLinkKind::SharepointLocation,
        "Contracts",
        Some(copy),
    );
    env.sharepoint
        .changes
        .lock()
        .expect("lock")
        .push(RemoteChange {
            source_link_id: link,
            origin_path: "Kestrel MSA.docx".to_owned(),
            changed_at: Timestamp::now(),
            removed: false,
        });
    env.home.poll_remote_marks().await;
    let marks = env.home.changed_marks(ws).await.expect("marks");
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].path, "Kestrel MSA.docx");
    assert_eq!(marks[0].place_id, copy);
    env.home
        .document_opened(
            ws,
            ContentRef::PlaceFile {
                place_id: copy,
                relative_path: "Kestrel MSA.docx".to_owned(),
            },
        )
        .await
        .expect("opened");
    assert!(env.home.changed_marks(ws).await.expect("marks").is_empty());
}

// Scenario: Edge case — ordin's own writes do not mark documents
#[tokio::test]
async fn edge_case_ordins_own_writes_do_not_mark_documents() {
    let env = Env::new().await;
    let (ws, place) = env.workspace("Q3 supplier review", WorkspaceState::Active);
    env.home
        .on_place_change(&place_change(
            place,
            "Q3/report.docx",
            PlaceChangeKind::Modified,
            true,
        ))
        .await;
    assert!(env.home.changed_marks(ws).await.expect("marks").is_empty());
}

// Scenario: Happy path — search across workspaces names the other workspace
#[tokio::test]
async fn happy_path_search_across_workspaces_names_the_other_workspace() {
    let env = Env::new().await;
    let (q3, _) = env.workspace("Q3 supplier review", WorkspaceState::Active);
    let (_q4, q4_place) = env.workspace("Q4 forecast", WorkspaceState::Active);
    env.files.hits.lock().expect("lock").push(file_hit(
        q4_place,
        "quotes/Kestrel quote.pdf",
        true,
        None,
    ));
    let started = Instant::now();
    let answer = env.home.search("Kestrel", Some(q3)).await.expect("search");
    assert!(started.elapsed() < Duration::from_secs(1));
    let doc = answer
        .go_to
        .iter()
        .find(|r| r.kind == GoToKind::Document)
        .expect("a document result");
    assert_eq!(doc.label, "Kestrel quote.pdf");
    assert_eq!(doc.detail.as_deref(), Some("· Q4 forecast"));
    assert!(answer.complete);
}

// Scenario: Edge case — a slow source does not delay first results
#[tokio::test]
async fn edge_case_a_slow_source_does_not_delay_first_results() {
    let env = Env::new().await;
    env.workspace("Q3 supplier review", WorkspaceState::Active);
    env.home.register_source(Arc::new(SlowSource {
        delay: Duration::from_secs(3),
        results: Vec::new(),
    }));
    let started = Instant::now();
    let answer = env.home.search("Kestrel", None).await.expect("search");
    assert!(started.elapsed() < Duration::from_millis(1500));
    assert!(!answer.complete);
}

// Scenario: Error path — empty search
#[tokio::test]
async fn error_path_empty_search() {
    let env = Env::new().await;
    assert_eq!(
        env.home.search("  ", None).await.unwrap_err(),
        HomeError::QueryInvalid
    );
}

// Scenario: Happy path — Run again names a newer input
#[tokio::test]
async fn happy_path_run_again_names_a_newer_input() {
    let env = Env::new().await;
    let place = PlaceId::new();
    env.files
        .hashes
        .lock()
        .expect("lock")
        .insert((place, "Q3/Kestrel quote.pdf".to_owned()), "h2".to_owned());
    *env.reader.inputs.lock().expect("lock") = Some(Ok(vec![InputUsed {
        name: "Kestrel quote.pdf".to_owned(),
        location: format!("place:{place}/Q3/Kestrel quote.pdf"),
        version_read: "h1".to_owned(),
    }]));
    let inputs = env
        .home
        .run_again_inputs(RunAgainSubject::Run {
            run_id: RunId::new(),
        })
        .await
        .expect("inputs");
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].check, VersionCheck::Newer);
    assert_eq!(inputs[0].newer_version.as_deref(), Some("h2"));
}

// Scenario: Happy path — Needs you lists a handover for the notifier
#[tokio::test]
async fn happy_path_needs_you_lists_a_handover_for_the_notifier() {
    let env = Env::new().await;
    let (q3, _) = env.workspace("Q3 supplier review", WorkspaceState::Active);
    let handover = HandoverId::new();
    *env.reader.waiting.lock().expect("lock") = Some(vec![WaitingItem {
        workspace_id: Some(q3),
        kind: "handover".to_owned(),
        title: "Contract review handed to you".to_owned(),
        since: Timestamp::now(),
        opens: OpenTarget::Handover {
            workspace_id: Some(q3),
            handover_id: handover,
        },
    }]);
    let rows = env.home.needs_you(None).await.expect("needs you");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].opens,
        OpenTarget::Handover {
            workspace_id: Some(q3),
            handover_id: handover
        }
    );
}

// Scenario: Edge case — opening an artefact clears nothing and succeeds
#[tokio::test]
async fn edge_case_opening_an_artefact_clears_nothing_and_succeeds() {
    let env = Env::new().await;
    let (ws, place) = env.workspace("Q3 supplier review", WorkspaceState::Active);
    crate::store::upsert_mark(&env.pool, ws, place, "a.docx", Timestamp::now())
        .await
        .expect("mark");
    env.home
        .document_opened(
            ws,
            ContentRef::Artefact {
                artefact_id: ArtefactId::new(),
            },
        )
        .await
        .expect("ok");
    assert_eq!(env.home.changed_marks(ws).await.expect("marks").len(), 1);
}

// Scenario: Error path — Run again on a pruned run
#[tokio::test]
async fn error_path_run_again_on_a_pruned_run() {
    let env = Env::new().await;
    *env.reader.inputs.lock().expect("lock") = Some(Err(HomeSourceError::NotFound));
    let error = env
        .home
        .run_again_inputs(RunAgainSubject::Run {
            run_id: RunId::new(),
        })
        .await
        .unwrap_err();
    assert_eq!(error, HomeError::RunNotFound);
}

// Scenario: A waiting question block appears in Needs you
#[tokio::test]
async fn a_waiting_question_block_appears_in_needs_you() {
    let env = Env::new().await;
    let (q3, _) = env.workspace("Q3 supplier review", WorkspaceState::Active);
    let block = BlockId::new();
    *env.reader.waiting.lock().expect("lock") = Some(vec![WaitingItem {
        workspace_id: Some(q3),
        kind: "question".to_owned(),
        title: "Which supplier first?".to_owned(),
        since: Timestamp::now(),
        opens: OpenTarget::Block {
            workspace_id: q3,
            block_id: block,
        },
    }]);
    let rows = env.home.needs_you(None).await.expect("needs you");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].opens,
        OpenTarget::Block {
            workspace_id: q3,
            block_id: block
        }
    );
}

// Scenario: A hosted agent's approver card appears under "Hosted agents" (SA-CLOSE-05)
#[tokio::test]
async fn a_hosted_agents_approver_card_appears_under_hosted_agents() {
    let env = Env::new().await;
    let (q3, _) = env.workspace("Q3 supplier review", WorkspaceState::Active);
    let approval = ApprovalId::new();
    let agent = RemoteAgentId::new();
    let handover = HandoverId::new();
    *env.reader.waiting.lock().expect("lock") = Some(vec![
        WaitingItem {
            workspace_id: None,
            kind: "hosted_approval".to_owned(),
            title: "support-agent asks to refund an order".to_owned(),
            since: at("2026-01-10T08:00:00.000Z"),
            opens: OpenTarget::HostedApproval {
                remote_agent_id: agent,
                approval_id: approval,
            },
        },
        WaitingItem {
            workspace_id: Some(q3),
            kind: "handover".to_owned(),
            title: "Handed to you".to_owned(),
            since: at("2026-01-10T09:00:00.000Z"),
            opens: OpenTarget::Handover {
                workspace_id: Some(q3),
                handover_id: handover,
            },
        },
    ]);
    let rows = env.home.needs_you(None).await.expect("needs you");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].workspace_id, Some(q3));
    assert_eq!(rows[1].workspace_id, None);
    assert_eq!(rows[1].workspace_name, None);
    assert_eq!(
        rows[1].opens,
        OpenTarget::HostedApproval {
            remote_agent_id: agent,
            approval_id: approval
        }
    );
    let filtered = env.home.needs_you(Some(q3)).await.expect("needs you");
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].workspace_id, Some(q3));
}

// Scenario: Error path — one hosted agent cannot be read
#[tokio::test]
async fn error_path_one_hosted_agent_cannot_be_read() {
    let env = Env::new().await;
    let (q3, _) = env.workspace("Q3 supplier review", WorkspaceState::Active);
    // The program's reader left the unreachable agent's cards out and returned the rest.
    *env.reader.waiting.lock().expect("lock") = Some(vec![WaitingItem {
        workspace_id: Some(q3),
        kind: "approval".to_owned(),
        title: "Approve the send".to_owned(),
        since: Timestamp::now(),
        opens: OpenTarget::Approval {
            workspace_id: q3,
            approval_id: ApprovalId::new(),
        },
    }]);
    let rows = env.home.needs_you(None).await.expect("no error");
    assert_eq!(rows.len(), 1);
}

// Scenario: Working copies refreshed from SharePoint are summarised
#[tokio::test]
async fn working_copies_refreshed_from_sharepoint_are_summarised() {
    let env = Env::new().await;
    let (ws, _) = env.workspace("Q3 supplier review", WorkspaceState::Active);
    let link = env.link(
        ws,
        SourceLinkKind::SharepointLocation,
        "Contracts",
        Some(PlaceId::new()),
    );
    env.set_home_opened(at("2026-01-10T09:00:00.000Z")).await;
    for (i, path) in ["a.docx", "b.docx", "c.docx"].iter().enumerate() {
        env.origin_change(
            ws,
            link,
            SourceLinkKind::SharepointLocation,
            path,
            at(&format!("2026-01-10T10:0{i}:00.000Z")),
            false,
        );
    }
    let view = env.home.home(None).await.expect("home");
    let lines: Vec<&str> = view
        .since_last_looked
        .iter()
        .map(|r| r.line.as_str())
        .collect();
    assert_eq!(lines, ["3 documents in Contracts updated from SharePoint"]);
    assert_eq!(view.since_last_looked[0].at, at("2026-01-10T10:02:00.000Z"));
}

// ---- Amendment January 2026 (ADR-106)

// Scenario: Happy path — a Jira change marks the item until opened
#[tokio::test]
async fn happy_path_a_jira_change_marks_the_item_until_opened() {
    let env = Env::new().await;
    let (ws, _) = env.workspace("Q3 supplier review", WorkspaceState::Active);
    let copy = PlaceId::new();
    let link = env.link(ws, SourceLinkKind::JiraIssue, "ABC-12", Some(copy));
    env.origin_change(
        ws,
        link,
        SourceLinkKind::JiraIssue,
        "summary-and-description.md",
        Timestamp::now(),
        false,
    );
    env.home.poll_remote_marks().await;
    let marks = env.home.changed_marks(ws).await.expect("marks");
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].path, "summary-and-description.md");
    env.home
        .document_opened(
            ws,
            ContentRef::PlaceFile {
                place_id: copy,
                relative_path: "summary-and-description.md".to_owned(),
            },
        )
        .await
        .expect("opened");
    assert!(env.home.changed_marks(ws).await.expect("marks").is_empty());
}

// Scenario: Edge case — a Jira change is not a copy update
#[tokio::test]
async fn edge_case_a_jira_change_is_not_a_copy_update() {
    let env = Env::new().await;
    let (ws, _) = env.workspace("Q3 supplier review", WorkspaceState::Active);
    let link = env.link(
        ws,
        SourceLinkKind::JiraIssue,
        "ABC-12",
        Some(PlaceId::new()),
    );
    env.set_home_opened(at("2026-01-10T09:00:00.000Z")).await;
    env.origin_change(
        ws,
        link,
        SourceLinkKind::JiraIssue,
        "summary-and-description.md",
        at("2026-01-10T10:00:00.000Z"),
        false,
    );
    let view = env.home.home(None).await.expect("home");
    assert!(!view
        .since_last_looked
        .iter()
        .any(|r| r.line.contains("updated from") && r.line.contains("ABC-12")));
}

// Scenario: Edge case — a removed Jira comment clears its mark
#[tokio::test]
async fn edge_case_a_removed_jira_comment_clears_its_mark() {
    let env = Env::new().await;
    let (ws, _) = env.workspace("Q3 supplier review", WorkspaceState::Active);
    let copy = PlaceId::new();
    let link = env.link(ws, SourceLinkKind::JiraIssue, "ABC-12", Some(copy));
    let path = "comments/20261021T090000Z-10001.md";
    crate::store::upsert_mark(&env.pool, ws, copy, path, Timestamp::now())
        .await
        .expect("mark");
    env.origin_change(
        ws,
        link,
        SourceLinkKind::JiraIssue,
        path,
        Timestamp::now(),
        true,
    );
    env.home.poll_remote_marks().await;
    assert!(env.home.changed_marks(ws).await.expect("marks").is_empty());
}
