//! X2: one test per Gherkin scenario of ConfluenceAdapter.md, of the same name.

mod support;

use std::time::Duration;

use ordin_confluence::{ConfluenceAdapter, ConfluenceError, ReadyKind, RemoteItemRef};
use ordin_files::AgentWriteTarget;
use ordin_journal::JournalEvent;
use ordin_types::domain::places::{
    ConfluenceLinkState, CopyDestination, CopyFormat, CopyMaker, FileVersionOrigin,
    PublishedCopyState,
};
use ordin_types::time::Timestamp;
use ordin_types::wire::approval::{AffectedItem, ApprovalContent, RejectStyle};
use ordin_types::{ApprovalId, RunId};
use serde_json::json;
use support::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

const PAGE: &str = "4411";

fn page_item() -> RemoteItemRef {
    RemoteItemRef::Page {
        page_id: PAGE.to_owned(),
    }
}

fn run_origin() -> FileVersionOrigin {
    FileVersionOrigin::Run {
        run_id: RunId::new(),
        tool: "write_file".to_owned(),
    }
}

fn page_path_of(id: &str) -> String {
    api(&format!("/api/v2/pages/{id}"))
}

// Scenario: Happy path — linking a folder asks one approval
#[tokio::test]
async fn happy_path_linking_a_folder_asks_one_approval() {
    let env = Env::new().await;
    mount_where(&env).await;
    env.save("rates.md", "# Rates");
    env.save("terms.pdf", "%PDF");
    let link = env.link(ConfluenceLinkState::Proposed);

    let id = env
        .adapter
        .request_link_approval(&link, &env.place, &env.workspace)
        .await
        .unwrap();

    let pending = env
        .approvals
        .list_pending(None, None)
        .await
        .expect("the list");
    assert_eq!(pending.items.len(), 1, "one approval");
    let card = &pending.items[0].card;
    assert_eq!(card.approval_id, id);
    assert_eq!(card.question, "Publish Supplier quotes to Confluence?");
    assert_eq!(card.verb, "Link and publish");
    assert_eq!(card.reject_style, RejectStyle::NotNow);
    assert_eq!(
        card.changes,
        "Text documents become pages. Other files are attached to the folder's page as they are."
    );
    assert_eq!(
        card.where_line,
        "Procurement team \u{2014} Confluence \u{2014} Procurement \u{203A} Supplier pages"
    );
    match &card.affects[..] {
        [AffectedItem::File { label, .. }] => {
            assert_eq!(label, "2 documents in Supplier quotes");
        }
        other => panic!("{other:?}"),
    }
    match &card.content {
        Some(ApprovalContent::Files { label, files }) => {
            assert_eq!(label, "See the files");
            assert_eq!(files.len(), 2);
        }
        other => panic!("{other:?}"),
    }
    assert!(card.buttons.show_details.is_some());
}

// Scenario: Happy path — the person's own edit is pushed automatically
#[tokio::test]
async fn happy_path_the_persons_own_edit_is_pushed_automatically() {
    let env = Env::new().await;
    let link = env.link(ConfluenceLinkState::Active);
    env.version("rates.md", "# Rates", FileVersionOrigin::Person)
        .await;
    let old = env.hash_of("rates.md").await;
    let copy = env.page_copy(&link, "rates.md", PAGE, "7", &old).await;
    // The person saved a new version in another app.
    env.version("rates.md", "# New rates", FileVersionOrigin::Person)
        .await;
    let new = env.hash_of("rates.md").await;
    mount_page(&env, PAGE, "rates", 7).await;
    mount_put(&env, PAGE, "rates", 8).await;

    let report = env
        .adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();

    assert_eq!(report.pushed, 1);
    assert_eq!(env.count("PUT", &page_path_of(PAGE)).await, 1);
    assert_eq!(env.actions().await, ["confirmed"]);
    assert_eq!(env.action_kinds().await, ["copy_publish"]);
    let copy = env.copy(copy.id).await;
    assert_eq!(copy.state, PublishedCopyState::Current);
    assert_eq!(
        copy.original_version_published.as_deref(),
        Some(new.as_str())
    );
    assert_eq!(copy.destination_version_pushed.as_deref(), Some("8"));
}

// Scenario: Error path — an agent's change is never pushed automatically
#[tokio::test]
async fn error_path_an_agents_change_is_never_pushed_automatically() {
    let env = Env::new().await;
    let link = env.link(ConfluenceLinkState::Active);
    env.version("rates.md", "# Rates", FileVersionOrigin::Person)
        .await;
    let old = env.hash_of("rates.md").await;
    let copy = env.page_copy(&link, "rates.md", PAGE, "7", &old).await;
    env.version("rates.md", "# Agent rates", run_origin()).await;

    let report = env
        .adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();

    assert!(env.confluence_requests().await.is_empty());
    assert_eq!(report.waiting_for_approval, 1);
    assert_eq!(report.pushed, 0);
    assert_eq!(env.copy(copy.id).await.state, PublishedCopyState::OutOfDate);
}

// Scenario: Error path — a harness write is the run's and is not pushed
#[tokio::test]
async fn error_path_a_harness_write_is_the_runs_and_is_not_pushed() {
    let env = Env::new().await;
    let link = env.link(ConfluenceLinkState::Active);
    env.version("rates.md", "# Rates", FileVersionOrigin::Person)
        .await;
    let old = env.hash_of("rates.md").await;
    let copy = env.page_copy(&link, "rates.md", PAGE, "7", &old).await;
    let run = RunId::new();
    let path = std::path::Path::new(&env.place.path).join("rates.md");
    let claim = env
        .files
        .open_agent_write(run, "Write", AgentWriteTarget::File(path.clone()))
        .await
        .unwrap()
        .expect("the path is in a place");
    std::fs::write(path, "# Agent rates").unwrap();

    // The call has not finished: the author is not known yet, so nothing moves.
    let report = env
        .adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();
    assert!(env.confluence_requests().await.is_empty());
    assert_eq!(report.waiting_for_approval, 1);
    assert_eq!(env.copy(copy.id).await.state, PublishedCopyState::Current);

    env.files.settle_agent_write(&claim).await.unwrap();
    let versions = env
        .files
        .versions(env.place_id(), "rates.md")
        .await
        .unwrap();
    assert!(
        matches!(&versions.last().unwrap().origin, FileVersionOrigin::Run { run_id, tool } if *run_id == run && tool == "Write")
    );
    let report = env
        .adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();
    assert!(env.confluence_requests().await.is_empty());
    assert_eq!(report.pushed, 0);
    assert_eq!(report.waiting_for_approval, 1);
    assert_eq!(env.copy(copy.id).await.state, PublishedCopyState::OutOfDate);
}

// Scenario: Error path — a path with .. is not pushed
#[tokio::test]
async fn error_path_a_path_with_dots_is_not_pushed() {
    let env = Env::new().await;
    let link = env.link(ConfluenceLinkState::Active);
    env.version("rates.md", "# Rates", FileVersionOrigin::Person)
        .await;
    let old = env.hash_of("rates.md").await;
    let copy = env.page_copy(&link, "rates.md", PAGE, "7", &old).await;
    let root = std::path::Path::new(&env.place.path);
    env.files
        .open_agent_write(
            RunId::new(),
            "Write",
            AgentWriteTarget::File(root.join("nope/../rates.md")),
        )
        .await
        .unwrap();
    std::fs::write(root.join("rates.md"), "# Agent rates").unwrap();

    let report = env
        .adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();

    assert!(env.confluence_requests().await.is_empty());
    assert_eq!(report.pushed, 0);
    assert_eq!(report.waiting_for_approval, 1);
    assert_eq!(env.copy(copy.id).await.state, PublishedCopyState::Current);
}

// Scenario: Error path — a person's edit does not carry an agent's text out
#[tokio::test]
async fn error_path_a_persons_edit_does_not_carry_an_agents_text_out() {
    let env = Env::new().await;
    let link = env.link(ConfluenceLinkState::Active);
    env.version("rates.md", "# Rates", FileVersionOrigin::Person)
        .await;
    let old = env.hash_of("rates.md").await;
    let copy = env.page_copy(&link, "rates.md", PAGE, "7", &old).await;
    env.version("rates.md", "# Agent rates", run_origin()).await;
    env.version(
        "rates.md",
        "# Agent rates, tidied",
        FileVersionOrigin::Person,
    )
    .await;

    let report = env
        .adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();

    assert!(env.confluence_requests().await.is_empty());
    assert_eq!(report.pushed, 0);
    assert_eq!(env.copy(copy.id).await.state, PublishedCopyState::OutOfDate);
}

// A file with no versions has no known author: an agent's shell may have changed it.
#[tokio::test]
async fn edge_case_a_changed_file_with_no_history_is_not_pushed_as_the_persons() {
    let env = Env::new().await;
    let link = env.link(ConfluenceLinkState::Active);
    env.save("rates.md", "# Rates");
    let old = env.hash_of("rates.md").await;
    let copy = env.page_copy(&link, "rates.md", PAGE, "7", &old).await;
    env.save("rates.md", "# Changed");

    let report = env
        .adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();

    assert!(env.confluence_requests().await.is_empty());
    assert_eq!(report.pushed, 0);
    assert_eq!(env.copy(copy.id).await.state, PublishedCopyState::OutOfDate);
}

// A write reaches the disk before its version row, so bytes the row does not record go to a person.
#[tokio::test]
async fn edge_case_bytes_with_no_version_yet_are_not_pushed_as_the_persons() {
    let env = Env::new().await;
    let link = env.link(ConfluenceLinkState::Active);
    env.version("rates.md", "# Rates", FileVersionOrigin::Person)
        .await;
    let old = env.hash_of("rates.md").await;
    let copy = env.page_copy(&link, "rates.md", PAGE, "7", &old).await;
    let path = std::path::Path::new(&env.place.path).join("rates.md");
    std::fs::write(path, "# Agent rates, row not written").unwrap();

    let report = env
        .adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();

    assert!(env.confluence_requests().await.is_empty());
    assert_eq!(report.pushed, 0);
    assert_eq!(report.waiting_for_approval, 1);
    assert_eq!(env.copy(copy.id).await.state, PublishedCopyState::OutOfDate);
}

// Scenario: Edge case — a new file waits in Ready to publish
#[tokio::test]
async fn edge_case_a_new_file_waits_in_ready_to_publish() {
    let env = Env::new().await;
    let link = env.link(ConfluenceLinkState::Active);
    env.save("notes.md", "Notes");

    let report = env
        .adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();

    assert_eq!(report.ready_to_publish, 1);
    let copies = env.files.published_copies_via_link(link.id).await.unwrap();
    assert_eq!(copies.len(), 1);
    assert_eq!(copies[0].state, PublishedCopyState::ReadyToPublish);
    assert_eq!(copies[0].format, CopyFormat::Page);
    assert_eq!(copies[0].maker, CopyMaker::Ordin);
    let ready = env.adapter.ready_to_publish(link.id).await.unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].kind, ReadyKind::New);
    assert_eq!(ready[0].path, "notes.md");
    assert_eq!(ready[0].published_copy_id, Some(copies[0].id));
    assert!(env.confluence_requests().await.is_empty());
}

// Scenario: Edge case — a renamed file is recognised by its content
#[tokio::test]
async fn edge_case_a_renamed_file_is_recognised_by_its_content() {
    let env = Env::new().await;
    let link = env.link(ConfluenceLinkState::Active);
    env.save("final.md", "Same content");
    let hash = env.hash_of("final.md").await;
    let copy = env.page_copy(&link, "draft.md", PAGE, "2", &hash).await;

    let report = env
        .adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();

    assert_eq!(report.ready_to_publish, 1);
    let ready = env.adapter.ready_to_publish(link.id).await.unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].kind, ReadyKind::Renamed);
    assert_eq!(ready[0].path, "final.md");
    assert_eq!(ready[0].from_path.as_deref(), Some("draft.md"));
    assert_eq!(ready[0].published_copy_id, Some(copy.id));
}

// Scenario: Error path — a page changed in Confluence is held
#[tokio::test]
async fn error_path_a_page_changed_in_confluence_is_held() {
    let env = Env::new().await;
    let link = env.link(ConfluenceLinkState::Active);
    env.version("rates.md", "# Rates", FileVersionOrigin::Person)
        .await;
    let old = env.hash_of("rates.md").await;
    let copy = env.page_copy(&link, "rates.md", PAGE, "7", &old).await;
    env.version("rates.md", "# Mine", FileVersionOrigin::Person)
        .await;
    mount_page(&env, PAGE, "rates", 8).await;

    let report = env
        .adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();

    assert_eq!(report.changed_in_confluence, 1);
    assert_eq!(report.pushed, 0);
    assert_eq!(
        env.copy(copy.id).await.state,
        PublishedCopyState::ChangedAtDestination
    );
    assert!(env.writes().await.is_empty(), "nothing is pushed");
    let local = std::fs::read_to_string(std::path::Path::new(&env.place.path).join("rates.md"));
    assert_eq!(local.unwrap(), "# Mine", "the local file is unchanged");
}

// Scenario: Error path — update refused when the page moved on
#[tokio::test]
async fn error_path_update_refused_when_the_page_moved_on() {
    let env = Env::new().await;
    mount_page(&env, PAGE, "Rates", 8).await;

    let error = env
        .adapter
        .update_page(PAGE, "Rates", "# Rates", None, "7", key(), None)
        .await
        .unwrap_err();

    assert_eq!(error.code(), "CONF_ORIGIN_CHANGED");
    assert_eq!(env.count("PUT", &page_path_of(PAGE)).await, 0);
    assert!(env.actions().await.is_empty(), "nothing was begun");
}

// Scenario: Error path — timeout after sending is not resent
#[tokio::test]
async fn error_path_timeout_after_sending_is_not_resent() {
    let env = Env::with_timeout(Duration::from_millis(250)).await;
    // The read before the PUT sees 7; the read after the timeout sees 8.
    reply_once(
        &env,
        "GET",
        &page_path_of(PAGE),
        200,
        page_json(PAGE, "Rates", 7),
    )
    .await;
    mount_page(&env, PAGE, "Rates", 8).await;
    Mock::given(method("PUT"))
        .and(path(page_path_of(PAGE)))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(2)))
        .mount(&env.mock)
        .await;

    let error = env
        .adapter
        .update_page(PAGE, "Rates", "# Rates", None, "7", key(), None)
        .await
        .unwrap_err();

    assert_eq!(error.code(), "CONF_OUTCOME_UNKNOWN");
    assert!(!error.retryable());
    assert_eq!(env.count("PUT", &page_path_of(PAGE)).await, 1);
    assert_eq!(env.actions().await, ["sent"], "left for the recovery");
}

// Scenario: Edge case — Word is attached until C-13
#[tokio::test]
async fn edge_case_word_is_attached_until_c_13() {
    let env = Env::new().await;
    let link = env.link(ConfluenceLinkState::Active);
    env.save("terms.docx", "PK");

    env.adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();

    let copies = env.files.published_copies_via_link(link.id).await.unwrap();
    assert_eq!(copies.len(), 1);
    assert!(matches!(
        copies[0].destination,
        CopyDestination::ConfluenceAttachment { .. }
    ));
    assert_eq!(copies[0].format, CopyFormat::ExactFile);
    assert_eq!(copies[0].maker, CopyMaker::None);
}

// Scenario: Happy path — Markdown becomes storage format with the copy's line on top
#[tokio::test]
async fn happy_path_markdown_becomes_storage_format_with_the_copys_line_on_top() {
    let env = Env::new().await;
    mount_space(&env).await;
    reply(
        &env,
        "POST",
        &api("/api/v2/pages"),
        200,
        page_json("5120", "Rates", 1),
    )
    .await;
    let line = "Published copy of rates.md, from supplier-docs, branch main. Last updated 16 Jan 2026, 10:15. Make changes in the original.";

    let (id, version) = env
        .adapter
        .publish_page(
            &CopyDestination::ConfluencePage {
                space: "PROC".to_owned(),
                parent_page_id: PARENT.to_owned(),
            },
            "Rates",
            "# Rates",
            Some(line),
            key(),
            None,
        )
        .await
        .unwrap();

    assert_eq!(id, "5120");
    assert_eq!(version.tag, "1");
    let body = env.body_of("POST", &api("/api/v2/pages")).await;
    assert_eq!(body["spaceId"], SPACE_ID);
    assert_eq!(body["parentId"], PARENT);
    assert_eq!(body["status"], "current");
    assert_eq!(body["body"]["representation"], "storage");
    let value = body["body"]["value"].as_str().unwrap();
    let macro_text = format!(
        "<ac:structured-macro ac:name=\"info\"><ac:rich-text-body><p>{line}</p></ac:rich-text-body></ac:structured-macro>"
    );
    assert_eq!(value, format!("{macro_text}<h1>Rates</h1>"));
    assert_eq!(env.actions().await, ["confirmed"]);
}

// Scenario: Edge case — raw HTML in Markdown is shown as text
#[test]
fn edge_case_raw_html_in_markdown_is_shown_as_text() {
    let out = ConfluenceAdapter::markdown_to_storage("<script>alert(1)</script>").unwrap();
    assert!(out.contains("&lt;script&gt;"), "{out}");
    assert!(!out.contains("<script"), "{out}");
}

// Scenario: Edge case — audience cannot be read (C-42 fallback)
#[tokio::test]
async fn edge_case_audience_cannot_be_read_c_42_fallback() {
    let env = Env::new().await;
    status_only(
        &env,
        "GET",
        &api(&format!(
            "/rest/api/content/{PAGE}/restriction/byOperation/read"
        )),
        403,
    )
    .await;

    let audience = env.adapter.audience(&location(PAGE)).await.unwrap();

    assert_eq!(audience.line, "everyone who can open it");
    assert!(audience.everyone_who_can_open);
    assert!(audience.principals.is_empty());
    assert_eq!(audience.people, None);
}

fn location(page: &str) -> ordin_confluence::ConfluenceLocation {
    ordin_confluence::ConfluenceLocation {
        space_key: "PROC".to_owned(),
        space_name: "Procurement".to_owned(),
        page_id: Some(page.to_owned()),
        title: "Rates".to_owned(),
        web_url: format!("{SITE}/wiki/spaces/PROC/pages/{page}"),
    }
}

// Scenario: Error path — signed in with an account that cannot reach the site
#[tokio::test]
async fn error_path_signed_in_with_an_account_that_cannot_reach_the_site() {
    let env = Env::with(Options {
        site: Some("https://other.atlassian.net"),
        ..Options::default()
    })
    .await;

    let error = env.adapter.find_locations("rates", 10).await.unwrap_err();

    assert_eq!(error.code(), "CONF_SITE_NOT_GRANTED");
    assert!(env.confluence_requests().await.is_empty());
}

// Scenario: Happy path — the caller passes the version it read and the adapter adds one (R19)
#[tokio::test]
async fn happy_path_the_caller_passes_the_version_it_read_and_the_adapter_adds_one_r19() {
    let env = Env::new().await;
    mount_page(&env, PAGE, "Rates", 7).await;
    mount_put(&env, PAGE, "Rates", 8).await;

    let version = env
        .adapter
        .update_page(PAGE, "Rates", "# Rates", None, "7", key(), None)
        .await
        .unwrap();

    assert_eq!(version.tag, "8");
    let body = env.body_of("PUT", &page_path_of(PAGE)).await;
    assert_eq!(body["version"]["number"], 8);
    assert_eq!(body["version"]["message"], "Updated from ordin");
    assert_eq!(body["id"], PAGE);
    assert_eq!(body["status"], "current");
    assert_eq!(body["title"], "Rates");
}

// Scenario: Error path — a caller that adds one itself is refused
#[tokio::test]
async fn error_path_a_caller_that_adds_one_itself_is_refused() {
    let env = Env::new().await;
    mount_page(&env, PAGE, "Rates", 7).await;

    let error = env
        .adapter
        .update_page(PAGE, "Rates", "# Rates", None, "8", key(), None)
        .await
        .unwrap_err();

    match error {
        ConfluenceError::OriginChanged { current } => assert_eq!(current.tag, "7"),
        other => panic!("{other:?}"),
    }
    assert_eq!(env.count("PUT", &page_path_of(PAGE)).await, 0);
}

// Scenario: Happy path — a published copy is moved to the trash
#[tokio::test]
async fn happy_path_a_published_copy_is_moved_to_the_trash() {
    let env = Env::new().await;
    mount_page(&env, "5120", "Rates", 3).await;
    status_only(&env, "DELETE", &page_path_of("5120"), 204).await;
    let action = key();

    env.adapter
        .delete(
            &RemoteItemRef::Page {
                page_id: "5120".to_owned(),
            },
            Some("3"),
            action,
            None,
        )
        .await
        .unwrap();

    assert_eq!(env.count("DELETE", &page_path_of("5120")).await, 1);
    let held = env.journal.external_action(action).await.unwrap();
    assert!(
        held.is_some_and(|a| a.confirmed_at.is_some()),
        "begun and confirmed"
    );
}

// Scenario: Error path — a copy changed in Confluence is not removed
#[tokio::test]
async fn error_path_a_copy_changed_in_confluence_is_not_removed() {
    let env = Env::new().await;
    mount_page(&env, "5120", "Rates", 4).await;

    let error = env
        .adapter
        .delete(
            &RemoteItemRef::Page {
                page_id: "5120".to_owned(),
            },
            Some("3"),
            key(),
            None,
        )
        .await
        .unwrap_err();

    assert_eq!(error.code(), "CONF_ORIGIN_CHANGED");
    assert_eq!(env.count("DELETE", &page_path_of("5120")).await, 0);
}

// Scenario: Happy path — the Push Service finds the page behind a working-copy path
#[tokio::test]
async fn happy_path_the_push_service_finds_the_page_behind_a_working_copy_path() {
    let env = Env::new().await;
    mount_search(&env, vec![page_hit(PARENT, "Supplier pages")]).await;
    mount_page(&env, PARENT, "Supplier pages", 3).await;
    mount_attachments(&env, PARENT, vec![]).await;
    mount_children(&env, PARENT, vec![json!({"id": PAGE, "title": "Rates"})]).await;
    mount_page(&env, PAGE, "Rates", 7).await;
    mount_attachments(&env, PAGE, vec![]).await;
    mount_children(&env, PAGE, vec![]).await;
    let link = env.source_link(&format!("confluence:page/{PARENT}"));
    env.adapter.track_link(&link).await.unwrap();
    let before = env.requests().await.len();

    let item = env
        .adapter
        .item_for_path(link.id, "Supplier pages/Rates.confluence.xhtml")
        .await
        .unwrap();

    assert_eq!(item, page_item());
    assert_eq!(env.requests().await.len(), before, "no request is sent");
}

// Scenario: Error path — a decision for an approval this adapter did not ask is ignored
#[tokio::test]
async fn error_path_a_decision_for_an_approval_this_adapter_did_not_ask_is_ignored() {
    let env = Env::new().await;
    mount_where(&env).await;
    env.save("notes.md", "Notes");
    let link = env.link(ConfluenceLinkState::Active);
    env.adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();
    let items = env.adapter.ready_to_publish(link.id).await.unwrap();
    env.adapter
        .request_publish_checked(&link, &env.place, items)
        .await
        .unwrap();
    let before = env.confluence_requests().await.len();
    let sink = env.adapter.event_sink();

    sink.deliver(&JournalEvent {
        backend: uuid::Uuid::now_v7(),
        seq: 1,
        at: Timestamp::now(),
        kind: "approval_decided".to_owned(),
        data: json!({"approval_id": ApprovalId::new(), "state": "approved", "decided_on": "here"}),
    })
    .await;

    assert_eq!(
        env.confluence_requests().await.len(),
        before,
        "nothing sent"
    );
    assert_eq!(
        env.scalar("SELECT count(*) FROM confluence_publish_requests")
            .await,
        1
    );
    assert_eq!(
        env.scalar("SELECT count(*) FROM confluence_ready_items")
            .await,
        1
    );
    let copies = env.files.published_copies_via_link(link.id).await.unwrap();
    assert_eq!(copies[0].state, PublishedCopyState::ReadyToPublish);
}
