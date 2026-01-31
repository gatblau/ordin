//! Behaviour of the Internal Logic beyond the Gherkin scenarios: searching, listing, fetching, tracking,
//! the audience line, retries of a write, and the linked-folder paths.

mod support;

use std::sync::Arc;
use std::time::Duration;

use ordin_confluence::{
    ConfluenceAdapter, ConfluenceLocation, ConfluenceSettings, ReadyItem, ReadyKind, RemoteItemRef,
    WordToMarkdown,
};
use ordin_journal::JournalEvent;
use ordin_types::domain::places::{
    ConfluenceLinkState, CopyDestination, CopyFormat, CopyMaker, CopyOriginal, FileVersionOrigin,
    PublishedCopy, PublishedCopyState,
};
use ordin_types::time::Timestamp;
use ordin_types::wire::approval::{AffectedItem, ApprovalContent, ContentRef};
use ordin_types::{ApprovalId, Cursor, PublishedCopyId, RunId};
use serde_json::json;
use support::*;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

const PAGE: &str = "4411";

fn location() -> ConfluenceLocation {
    ConfluenceLocation {
        space_key: "PROC".to_owned(),
        space_name: "Procurement".to_owned(),
        page_id: Some(PAGE.to_owned()),
        title: "Rates".to_owned(),
        web_url: String::new(),
    }
}

fn space_location() -> ConfluenceLocation {
    ConfluenceLocation {
        page_id: None,
        title: "Procurement".to_owned(),
        ..location()
    }
}

fn page_url(id: &str) -> String {
    api(&format!("/api/v2/pages/{id}"))
}

fn page_ref() -> RemoteItemRef {
    RemoteItemRef::Page {
        page_id: PAGE.to_owned(),
    }
}

fn attachment_ref() -> RemoteItemRef {
    RemoteItemRef::Attachment {
        page_id: PAGE.to_owned(),
        attachment_id: "att1".to_owned(),
    }
}

async fn decide(env: &Env, approval: ApprovalId, state: &str) {
    env.adapter
        .event_sink()
        .deliver(&JournalEvent {
            backend: uuid::Uuid::now_v7(),
            seq: 1,
            at: Timestamp::now(),
            kind: "approval_decided".to_owned(),
            data: json!({"approval_id": approval, "state": state, "decided_on": "here"}),
        })
        .await;
}

#[tokio::test]
async fn find_quotes_the_query_and_maps_spaces_and_pages() {
    let env = Env::new().await;
    Mock::given(method("GET"))
        .and(path(api("/rest/api/search")))
        .and(query_param(
            "cql",
            r#"(type=space OR type=page) AND title ~ "a\"b\\c""#,
        ))
        .and(query_param("limit", "25"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [
                space_hit(),
                page_hit(PAGE, "Rates"),
                {"content": {"id": "att1", "type": "attachment", "title": "a.pdf"},
                 "resultGlobalContainer": {"title": "Procurement", "displayUrl": "/spaces/PROC"},
                 "url": "/x"},
                {"url": "/nothing"},
            ],
        })))
        .mount(&env.mock)
        .await;

    let found = env.adapter.find_locations(r#"  a"b\c "#, 99).await.unwrap();

    assert_eq!(found.len(), 2);
    assert_eq!(found[0].page_id, None);
    assert_eq!(found[0].space_key, "PROC");
    assert_eq!(found[0].web_url, format!("{SITE}/wiki/spaces/PROC"));
    assert_eq!(found[1].page_id.as_deref(), Some(PAGE));
    assert_eq!(found[1].space_name, "Procurement");
    assert_eq!(found[1].title, "Rates");
}

#[tokio::test]
async fn origins_go_both_ways() {
    let env = Env::new().await;
    mount_space(&env).await;
    let space = env
        .adapter
        .resolve_origin("confluence:space/PROC")
        .await
        .unwrap();
    assert_eq!(space.space_name, "Procurement");
    assert_eq!(space.web_url, format!("{SITE}/wiki/spaces/PROC"));
    assert_eq!(
        ConfluenceAdapter::origin_location(&space),
        "confluence:space/PROC"
    );

    mount_search(&env, vec![page_hit(PAGE, "Rates")]).await;
    let page = env
        .adapter
        .resolve_origin(&format!("confluence:page/{PAGE}"))
        .await
        .unwrap();
    assert_eq!(page.page_id.as_deref(), Some(PAGE));
    assert_eq!(page.space_key, "PROC");
    assert_eq!(
        ConfluenceAdapter::origin_location(&page),
        format!("confluence:page/{PAGE}")
    );
}

#[tokio::test]
async fn a_page_with_no_search_row_is_not_found() {
    let env = Env::new().await;
    mount_search(&env, vec![]).await;
    let error = env
        .adapter
        .resolve_origin("confluence:page/1")
        .await
        .unwrap_err();
    assert_eq!(error.code(), "CONF_NOT_FOUND");
}

#[tokio::test]
async fn a_space_lists_its_top_pages() {
    let env = Env::new().await;
    mount_space(&env).await;
    mount_author(&env).await;
    let listing = api(&format!("/api/v2/spaces/{SPACE_ID}/pages"));
    Mock::given(method("GET"))
        .and(path(listing))
        .and(query_param("depth", "root"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [page_json("1", "A/B", 2)],
            "_links": {},
        })))
        .mount(&env.mock)
        .await;

    let page = env
        .adapter
        .list_children(&space_location(), None)
        .await
        .unwrap();

    assert!(page.next_cursor.is_none());
    assert_eq!(page.items[0].path, "A\u{2215}B.confluence.xhtml");
    assert!(page.items[0].has_children);
    assert_eq!(page.items[0].version.tag, "2");
    assert_eq!(page.items[0].version.changed_by.as_deref(), Some("Sam Lee"));
}

#[tokio::test]
async fn a_page_lists_its_children_then_its_attachments() {
    let env = Env::new().await;
    mount_author(&env).await;
    let next = format!("/wiki/api/v2/pages/{PAGE}/children?cursor=abc");
    Mock::given(method("GET"))
        .and(path(api(&format!("/api/v2/pages/{PAGE}/children"))))
        .and(query_param("cursor", "abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{"id": "2", "title": "Second"}],
            "_links": {},
        })))
        .mount(&env.mock)
        .await;
    reply_once(
        &env,
        "GET",
        &api(&format!("/api/v2/pages/{PAGE}/children")),
        200,
        json!({"results": [{"id": "1", "title": "First"}], "_links": {"next": next}}),
    )
    .await;
    mount_page(&env, "1", "First", 4).await;
    mount_page(&env, "2", "Second", 5).await;
    mount_attachments(&env, PAGE, vec![attachment_json("att1", "a.pdf", 3, 88)]).await;

    let first = env.adapter.list_children(&location(), None).await.unwrap();
    assert_eq!(first.items[0].path, "Rates/First.confluence.xhtml");
    assert_eq!(first.items[0].version.tag, "4", "read page by page");
    let second = env
        .adapter
        .list_children(&location(), first.next_cursor)
        .await
        .unwrap();
    assert_eq!(second.items[0].title, "Second");
    let third = env
        .adapter
        .list_children(&location(), second.next_cursor)
        .await
        .unwrap();
    assert!(third.next_cursor.is_none());
    assert_eq!(third.items[0].path, "Rates/a.pdf");
    assert_eq!(third.items[0].size_bytes, Some(88));
    assert!(!third.items[0].has_children);
}

#[tokio::test]
async fn a_cursor_is_refused_when_it_is_not_a_wiki_path_or_not_a_cursor() {
    let env = Env::new().await;
    let bad: Cursor = serde_json::from_value(json!("%%%")).unwrap();
    let error = env
        .adapter
        .list_children(&location(), Some(bad))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "SRV_BAD_CURSOR");
    let attachments = Cursor::encode(&"/wiki/api/v2/pages/1/attachments".to_owned());
    let error = env
        .adapter
        .list_children(&space_location(), Some(attachments))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "SRV_BAD_CURSOR");
}

#[tokio::test]
async fn a_next_link_off_the_gateway_is_a_bad_answer() {
    let env = Env::new().await;
    mount_author(&env).await;
    reply(
        &env,
        "GET",
        &api(&format!("/api/v2/pages/{PAGE}/children")),
        200,
        json!({"results": [], "_links": {"next": "https://elsewhere.example/wiki/x"}}),
    )
    .await;
    let error = env
        .adapter
        .list_children(&location(), None)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "CONF_RESPONSE_INVALID");
}

#[tokio::test]
async fn fetching_a_page_gives_its_storage_and_version() {
    let env = Env::new().await;
    mount_author(&env).await;
    mount_page(&env, PAGE, "Rates", 7).await;

    let fetched = env.adapter.fetch(&page_ref()).await.unwrap();

    assert_eq!(fetched.bytes, b"<p>Body</p>");
    assert_eq!(fetched.path, "Rates.confluence.xhtml");
    assert_eq!(fetched.version.tag, "7");
    assert_eq!(fetched.version.changed_by.as_deref(), Some("Sam Lee"));
    let version = env.adapter.item_version(&page_ref()).await.unwrap();
    assert_eq!(version.tag, "7");
}

#[tokio::test]
async fn fetching_an_attachment_gives_the_file() {
    let env = Env::new().await;
    mount_page(&env, PAGE, "Rates", 7).await;
    mount_attachments(&env, PAGE, vec![attachment_json("att1", "a.pdf", 3, 4)]).await;
    Mock::given(method("GET"))
        .and(path(api("/download/attachments/att1/a.pdf")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"%PDF".to_vec()))
        .mount(&env.mock)
        .await;

    let fetched = env.adapter.fetch(&attachment_ref()).await.unwrap();

    assert_eq!(fetched.bytes, b"%PDF");
    assert_eq!(fetched.path, "Rates/a.pdf");
    assert_eq!(fetched.version.tag, "3");
    let version = env.adapter.item_version(&attachment_ref()).await.unwrap();
    assert_eq!(version.tag, "3");
}

#[tokio::test]
async fn an_attachment_gone_from_its_page_is_not_found() {
    let env = Env::new().await;
    mount_attachments(&env, PAGE, vec![]).await;
    let error = env
        .adapter
        .item_version(&attachment_ref())
        .await
        .unwrap_err();
    assert_eq!(error.code(), "CONF_NOT_FOUND");
}

#[tokio::test]
async fn an_attachment_without_a_link_is_a_bad_answer() {
    let env = Env::new().await;
    mount_attachments(
        &env,
        PAGE,
        vec![json!({"id": "att1", "title": "a.pdf", "version": version_json(1)})],
    )
    .await;
    assert_eq!(
        env.adapter
            .fetch(&attachment_ref())
            .await
            .unwrap_err()
            .code(),
        "CONF_RESPONSE_INVALID"
    );
}

#[tokio::test]
async fn a_fetch_repeats_once_when_the_version_moves() {
    let env = Env::new().await;
    // Transfer at 7, re-read at 8, transfer at 8, re-read at 8.
    reply_once(
        &env,
        "GET",
        &page_url(PAGE),
        200,
        page_json(PAGE, "Rates", 7),
    )
    .await;
    mount_page(&env, PAGE, "Rates", 8).await;

    let fetched = env.adapter.fetch(&page_ref()).await.unwrap();

    assert_eq!(fetched.version.tag, "8");
    assert_eq!(env.count("GET", &page_url(PAGE)).await, 4);
}

#[tokio::test]
async fn a_fetch_gives_up_when_the_version_keeps_moving() {
    let env = Env::new().await;
    for version in [7, 8, 9] {
        reply_once(
            &env,
            "GET",
            &page_url(PAGE),
            200,
            page_json(PAGE, "Rates", version),
        )
        .await;
    }
    mount_page(&env, PAGE, "Rates", 10).await;

    let error = env.adapter.fetch(&page_ref()).await.unwrap_err();

    match error {
        ordin_confluence::ConfluenceError::OriginChanged { current } => {
            assert_eq!(current.tag, "10");
        }
        other => panic!("{other:?}"),
    }
}

/// A tracked tree: `Supplier pages` (4000) with the page `Rates` and an attachment on it.
async fn tracked_tree(env: &Env) -> ordin_types::domain::places::SourceLink {
    mount_author(env).await;
    mount_page(env, PARENT, "Supplier pages", 3).await;
    mount_attachments(env, PARENT, vec![]).await;
    mount_children(env, PARENT, vec![json!({"id": PAGE, "title": "Rates"})]).await;
    reply_once(
        env,
        "GET",
        &page_url(PAGE),
        200,
        page_json(PAGE, "Rates", 7),
    )
    .await;
    reply_once(
        env,
        "GET",
        &api(&format!("/api/v2/pages/{PAGE}/attachments")),
        200,
        json!({"results": [
            attachment_json("att1", "a.pdf", 1, 4),
            attachment_json("att2", "b.pdf", 2, 4),
        ]}),
    )
    .await;
    reply_once(
        env,
        "GET",
        &api(&format!("/api/v2/pages/{PAGE}/children")),
        200,
        json!({"results": []}),
    )
    .await;
    reply_once(
        env,
        "GET",
        &api("/rest/api/search"),
        200,
        json!({"results": [page_hit(PARENT, "Supplier pages")]}),
    )
    .await;
    let link = env.source_link(&format!("confluence:page/{PARENT}"));
    env.adapter.track_link(&link).await.unwrap();
    link
}

#[tokio::test]
async fn tracking_stores_the_tree_and_a_check_finds_what_moved() {
    let env = Env::new().await;
    let link = tracked_tree(&env).await;
    assert_eq!(env.scalar("SELECT count(*) FROM confluence_items").await, 4);
    let attachment = env
        .adapter
        .item_for_path(link.id, "Supplier pages/Rates/a.pdf")
        .await
        .unwrap();
    assert_eq!(attachment, attachment_ref());

    // The page moved to 8, a.pdf moved to 2, b.pdf is gone, c.pdf and a child page are new.
    let since = Timestamp::now();
    Mock::given(method("GET"))
        .and(path(api("/rest/api/search")))
        .and(query_param("limit", "100"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [
                page_hit(PAGE, "Rates"),
                {"content": {"id": "att1", "type": "attachment", "title": "a.pdf"}},
            ],
        })))
        .mount(&env.mock)
        .await;
    mount_page(&env, PAGE, "Rates", 8).await;
    mount_attachments(
        &env,
        PAGE,
        vec![
            attachment_json("att1", "a.pdf", 2, 4),
            attachment_json("att3", "c.pdf", 1, 4),
        ],
    )
    .await;
    mount_children(&env, PAGE, vec![]).await;

    let changes = env.adapter.check_changes(&link).await.unwrap();

    let by_path = |wanted: &str| {
        changes
            .iter()
            .find(|c| c.origin_path == wanted)
            .unwrap_or_else(|| panic!("{wanted}: {changes:?}"))
    };
    assert_eq!(changes.len(), 4);
    let page = by_path("Supplier pages/Rates.confluence.xhtml");
    assert_eq!(page.new_version, "8");
    assert_eq!(page.changed_by.as_deref(), Some("Sam Lee"));
    assert!(!page.removed);
    assert_eq!(by_path("Supplier pages/Rates/a.pdf").new_version, "2");
    assert!(by_path("Supplier pages/Rates/b.pdf").removed);
    assert!(!by_path("Supplier pages/Rates/c.pdf").removed);

    let found = env.adapter.changes_since(&[link.id], since).await.unwrap();
    assert_eq!(found.len(), 4);
    assert!(env
        .adapter
        .changes_since(&[], since)
        .await
        .unwrap()
        .is_empty());
    let gone = env
        .adapter
        .item_for_path(link.id, "Supplier pages/Rates/b.pdf")
        .await
        .unwrap_err();
    assert_eq!(gone.code(), "CONF_NOT_FOUND");

    // A second look finds nothing new.
    let again = env.adapter.check_changes(&link).await.unwrap();
    assert!(again.is_empty(), "{again:?}");
}

#[tokio::test]
async fn a_child_page_no_longer_listed_is_removed_with_its_attachments() {
    let env = Env::new().await;
    let link = tracked_tree(&env).await;
    reply(
        &env,
        "GET",
        &api("/rest/api/search"),
        200,
        json!({"results": [page_hit(PARENT, "Supplier pages")]}),
    )
    .await;
    mount_page(&env, PARENT, "Supplier pages", 3).await;
    Mock::given(method("GET"))
        .and(path(api(&format!("/api/v2/pages/{PARENT}/children"))))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"results": []})))
        .with_priority(1)
        .mount(&env.mock)
        .await;

    let changes = env.adapter.check_changes(&link).await.unwrap();

    assert_eq!(changes.len(), 3, "{changes:?}");
    assert!(changes.iter().all(|c| c.removed));
}

#[tokio::test]
async fn a_page_in_the_results_that_404s_is_removed() {
    let env = Env::new().await;
    let link = tracked_tree(&env).await;
    mount_search(&env, vec![page_hit(PAGE, "Rates")]).await;
    Mock::given(method("GET"))
        .and(path(page_url(PAGE)))
        .respond_with(ResponseTemplate::new(404))
        .with_priority(1)
        .mount(&env.mock)
        .await;

    let changes = env.adapter.check_changes(&link).await.unwrap();

    assert_eq!(changes.len(), 3, "{changes:?}");
    assert!(changes.iter().all(|c| c.removed));
}

#[tokio::test]
async fn a_new_page_is_placed_under_its_parent() {
    let env = Env::new().await;
    let link = tracked_tree(&env).await;
    mount_search(&env, vec![page_hit("7000", "New")]).await;
    mount_page(&env, "7000", "New", 1).await;
    mount_attachments(&env, "7000", vec![]).await;
    mount_children(&env, "7000", vec![]).await;

    let changes = env.adapter.check_changes(&link).await.unwrap();

    assert_eq!(changes.len(), 1);
    assert_eq!(
        changes[0].origin_path,
        "Supplier pages/New.confluence.xhtml"
    );
}

#[tokio::test]
async fn ordins_own_push_is_not_reported() {
    let env = Env::new().await;
    let link = tracked_tree(&env).await;
    mount_page(&env, PAGE, "Rates", 7).await;
    mount_put(&env, PAGE, "Rates", 8).await;
    env.adapter
        .update_page(PAGE, "Rates", "x", None, "7", key(), None)
        .await
        .unwrap();
    assert_eq!(
        env.scalar("SELECT max(version_number) FROM confluence_items")
            .await,
        8
    );
    env.mock.reset().await;
    env.mount_resources(&[(CLOUD, SITE)]).await;
    mount_search(&env, vec![page_hit(PAGE, "Rates")]).await;
    mount_page(&env, PAGE, "Rates", 8).await;
    mount_author(&env).await;
    mount_attachments(
        &env,
        PAGE,
        vec![
            attachment_json("att1", "a.pdf", 1, 4),
            attachment_json("att2", "b.pdf", 2, 4),
        ],
    )
    .await;
    mount_children(&env, PAGE, vec![]).await;

    let changes = env.adapter.check_changes(&link).await.unwrap();

    assert!(changes.is_empty(), "{changes:?}");
}

#[tokio::test]
async fn a_failed_check_records_the_code_only() {
    let env = Env::new().await;
    let link = tracked_tree(&env).await;
    status_only(&env, "GET", &api("/rest/api/search"), 403).await;

    let error = env.adapter.check_changes(&link).await.unwrap_err();

    assert_eq!(error.code(), "CONF_ACCESS_DENIED");
    use sqlx::Row as _;
    let row = sqlx::query("SELECT last_error FROM confluence_link_state")
        .fetch_one(&env.pool)
        .await
        .unwrap();
    assert_eq!(row.get::<String, _>(0), "CONF_ACCESS_DENIED");
}

#[tokio::test]
async fn a_whole_space_is_tracked_and_searched_by_key() {
    let env = Env::new().await;
    mount_author(&env).await;
    mount_space(&env).await;
    reply(
        &env,
        "GET",
        &api(&format!("/api/v2/spaces/{SPACE_ID}/pages")),
        200,
        json!({"results": [page_json(PAGE, "Rates", 7)]}),
    )
    .await;
    mount_attachments(&env, PAGE, vec![]).await;
    mount_children(&env, PAGE, vec![]).await;
    let link = env.source_link("confluence:space/PROC");
    env.adapter.track_link(&link).await.unwrap();
    assert_eq!(env.scalar("SELECT count(*) FROM confluence_items").await, 1);

    let requests = env.mock.received_requests().await.unwrap();
    assert!(requests.iter().all(|r| !r.url.path().ends_with("/search")));
    Mock::given(method("GET"))
        .and(path(api("/rest/api/search")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [],
            "_links": {"next": "/rest/api/search?cursor=more"},
        })))
        .up_to_n_times(1)
        .mount(&env.mock)
        .await;
    mount_search(&env, vec![]).await;

    let changes = env.adapter.check_changes(&link).await.unwrap();

    assert!(changes.is_empty());
    let searched = env.mock.received_requests().await.unwrap();
    let query = searched
        .iter()
        .find(|r| r.url.path().ends_with("/search"))
        .unwrap()
        .url
        .query_pairs()
        .find(|(k, _)| k == "cql")
        .unwrap()
        .1
        .to_string();
    assert!(
        query.starts_with("space=\"PROC\" AND lastmodified >= \""),
        "{query}"
    );
    assert_eq!(
        searched
            .iter()
            .filter(|r| r.url.path().ends_with("/search"))
            .count(),
        2,
        "the next link is followed"
    );
}

#[tokio::test]
async fn untracking_removes_the_links_rows() {
    let env = Env::new().await;
    let link = tracked_tree(&env).await;
    env.adapter.untrack_link(link.id).await.unwrap();
    assert_eq!(env.scalar("SELECT count(*) FROM confluence_items").await, 0);
    assert_eq!(
        env.scalar("SELECT count(*) FROM confluence_link_state")
            .await,
        0
    );
}

#[tokio::test]
async fn the_audience_of_a_restricted_page_names_groups_and_people() {
    let env = Env::new().await;
    reply(
        &env,
        "GET",
        &api(&format!("/rest/api/content/{PAGE}/restriction/byOperation/read")),
        200,
        json!({"restrictions": {
            "user": {"results": [{"accountId": "acc-2"}, {"accountId": "acc-1"}]},
            "group": {"results": [{"name": "Finance", "id": "g-1"}, {"name": "Legal", "id": "g-2"}]},
        }}),
    )
    .await;

    let audience = env.adapter.audience(&location()).await.unwrap();

    assert_eq!(audience.line, "Finance and Legal and 2 people");
    assert_eq!(audience.principals, ["acc-1", "acc-2", "g-1", "g-2"]);
    assert_eq!(audience.people, None);
    assert!(!audience.everyone_who_can_open);
}

async fn mount_no_restrictions(env: &Env) {
    reply(
        env,
        "GET",
        &api(&format!(
            "/rest/api/content/{PAGE}/restriction/byOperation/read"
        )),
        200,
        json!({"restrictions": {}}),
    )
    .await;
}

async fn mount_grants(env: &Env, grants: serde_json::Value) {
    mount_space(env).await;
    reply(
        env,
        "GET",
        &api(&format!("/api/v2/spaces/{SPACE_ID}/permissions")),
        200,
        json!({"results": grants}),
    )
    .await;
}

fn grant(kind: &str, id: &str) -> serde_json::Value {
    json!({
        "principal": {"type": kind, "id": id},
        "operation": {"key": "read", "targetType": "space"},
    })
}

#[tokio::test]
async fn the_audience_of_an_open_page_comes_from_the_space() {
    for (grants, line) in [
        (vec![grant("anonymous", "anon")], "Anyone on the internet"),
        (
            vec![grant("group", "confluence-users")],
            "Everyone in the company",
        ),
        (vec![grant("user", "acc-1")], "1 person"),
        (
            vec![
                json!({"principal": {"type": "group", "id": "g-9", "name": "Buyers"},
                       "operation": {"key": "read", "targetType": "space"}}),
                grant("user", "acc-1"),
                json!({"principal": {"type": "user", "id": "acc-2"},
                       "operation": {"key": "administer", "targetType": "space"}}),
            ],
            "Buyers and 1 person",
        ),
    ] {
        let env = Env::new().await;
        mount_no_restrictions(&env).await;
        mount_grants(&env, json!(grants)).await;
        let audience = env.adapter.audience(&location()).await.unwrap();
        assert_eq!(audience.line, line);
        assert!(!audience.principals.is_empty());
    }
}

#[tokio::test]
async fn the_audience_of_a_space_reads_only_its_permissions() {
    let env = Env::new().await;
    mount_grants(&env, json!([grant("user", "acc-1")])).await;
    let audience = env.adapter.audience(&space_location()).await.unwrap();
    assert_eq!(audience.people, Some(1));
    let paths = env.requests().await;
    assert!(paths.iter().all(|(_, p)| !p.contains("restriction")));
}

#[tokio::test]
async fn the_fallback_covers_an_empty_answer_a_403_on_the_space_and_a_refused_scope() {
    let env = Env::new().await;
    mount_no_restrictions(&env).await;
    mount_grants(&env, json!([])).await;
    let audience = env.adapter.audience(&location()).await.unwrap();
    assert_eq!(audience.line, "everyone who can open it");

    let env = Env::new().await;
    mount_no_restrictions(&env).await;
    mount_space(&env).await;
    status_only(
        &env,
        "GET",
        &api(&format!("/api/v2/spaces/{SPACE_ID}/permissions")),
        403,
    )
    .await;
    let audience = env.adapter.audience(&location()).await.unwrap();
    assert!(audience.everyone_who_can_open);

    // Atlassian refused the restriction-read scope and the space's permissions are refused as well.
    let env = Env::with_sign_in(SignIn::NoRestrictionScope).await;
    mount_space(&env).await;
    status_only(
        &env,
        "GET",
        &api(&format!("/api/v2/spaces/{SPACE_ID}/permissions")),
        403,
    )
    .await;
    let audience = env.adapter.audience(&location()).await.unwrap();
    assert!(audience.everyone_who_can_open);
}

#[tokio::test]
async fn a_refused_restriction_scope_still_reads_the_space() {
    let env = Env::with_sign_in(SignIn::NoRestrictionScope).await;
    mount_page(&env, PAGE, "Rates", 7).await;
    mount_grants(&env, json!([grant("anonymous", "anon")])).await;

    let audience = env.adapter.audience(&location()).await.unwrap();

    assert_eq!(audience.line, "Anyone on the internet");
    assert!(!audience.everyone_who_can_open);
    assert_eq!(
        env.adapter.item_version(&page_ref()).await.unwrap().tag,
        "7"
    );
    assert!(env
        .requests()
        .await
        .iter()
        .all(|(_, p)| !p.contains("restriction")));
}

#[tokio::test]
async fn a_reader_of_an_unknown_kind_is_not_counted_as_a_person() {
    let buyers = json!({"principal": {"type": "group", "id": "g-9", "name": "Buyers"},
                        "operation": {"key": "read", "targetType": "space"}});
    for (grants, line) in [
        (vec![grant("role", "r-1")], "Audience not known"),
        (
            vec![grant("role", "r-1"), grant("user", "acc-1")],
            "1 person and others not known",
        ),
        (
            vec![grant("role", "r-1"), buyers.clone()],
            "Buyers and others not known",
        ),
    ] {
        let env = Env::new().await;
        mount_no_restrictions(&env).await;
        mount_grants(&env, json!(grants)).await;

        let audience = env.adapter.audience(&location()).await.unwrap();

        assert_eq!(audience.line, line);
        assert_eq!(audience.people, None);
        assert!(!audience.principals.is_empty());
    }
}

#[tokio::test]
async fn the_audience_leads_the_where_line_so_a_long_title_cannot_cut_it() {
    let env = Env::new().await;
    mount_space(&env).await;
    let title = "T".repeat(240);
    mount_page(&env, PARENT, &title, 3).await;
    reply(
        &env,
        "GET",
        &api(&format!(
            "/rest/api/content/{PARENT}/restriction/byOperation/read"
        )),
        200,
        json!({"restrictions": {
            "user": {"results": []},
            "group": {"results": [{"name": "Procurement team", "id": "g-1"}]},
        }}),
    )
    .await;
    let link = env.link(ConfluenceLinkState::Proposed);

    env.adapter
        .request_link_approval(&link, &env.place, &env.workspace)
        .await
        .unwrap();

    let pending = env.approvals.list_pending(None, None).await.unwrap();
    let where_line = &pending.items[0].card.where_line;
    assert_eq!(where_line.chars().count(), 200);
    assert!(where_line.starts_with("Procurement team \u{2014} Confluence"));
}

#[tokio::test]
async fn a_403_on_a_read_of_the_page_is_not_swallowed_by_other_errors() {
    let env = Env::new().await;
    status_only(
        &env,
        "GET",
        &api(&format!(
            "/rest/api/content/{PAGE}/restriction/byOperation/read"
        )),
        404,
    )
    .await;
    assert_eq!(
        env.adapter.audience(&location()).await.unwrap_err().code(),
        "CONF_NOT_FOUND"
    );
}

#[tokio::test]
async fn a_timeout_with_the_version_unmoved_is_retried_once_with_the_same_key() {
    let env = Env::with_timeout(Duration::from_millis(250)).await;
    mount_page(&env, PAGE, "Rates", 7).await;
    Mock::given(method("PUT"))
        .and(path(page_url(PAGE)))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(2)))
        .up_to_n_times(1)
        .mount(&env.mock)
        .await;
    mount_put(&env, PAGE, "Rates", 8).await;

    let version = env
        .adapter
        .update_page(PAGE, "Rates", "x", None, "7", key(), None)
        .await
        .unwrap();

    assert_eq!(version.tag, "8");
    assert_eq!(env.count("PUT", &page_url(PAGE)).await, 2);
    assert_eq!(env.actions().await, ["confirmed"]);
}

#[tokio::test]
async fn a_second_timeout_is_not_tried_a_third_time() {
    let env = Env::with_timeout(Duration::from_millis(200)).await;
    mount_page(&env, PAGE, "Rates", 7).await;
    Mock::given(method("PUT"))
        .and(path(page_url(PAGE)))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(2)))
        .mount(&env.mock)
        .await;

    let error = env
        .adapter
        .update_page(PAGE, "Rates", "x", None, "7", key(), None)
        .await
        .unwrap_err();

    assert_eq!(error.code(), "CONF_OUTCOME_UNKNOWN");
    assert_eq!(env.count("PUT", &page_url(PAGE)).await, 2);
}

#[tokio::test]
async fn a_busy_write_proven_not_landed_is_retried_then_throttled() {
    let env = Env::new().await;
    mount_page(&env, PAGE, "Rates", 7).await;
    status_only(&env, "PUT", &page_url(PAGE), 429).await;

    let error = env
        .adapter
        .update_page(PAGE, "Rates", "x", None, "7", key(), None)
        .await
        .unwrap_err();

    assert_eq!(error.code(), "CONF_THROTTLED");
    assert_eq!(env.count("PUT", &page_url(PAGE)).await, 2);
}

#[tokio::test]
async fn a_create_has_no_version_to_prove_it_did_not_land() {
    let env = Env::new().await;
    mount_space(&env).await;
    let dest = CopyDestination::ConfluencePage {
        space: "PROC".to_owned(),
        parent_page_id: PARENT.to_owned(),
    };
    status_only(&env, "POST", &api("/api/v2/pages"), 500).await;
    let error = env
        .adapter
        .publish_page(&dest, "Rates", "x", None, key(), None)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "CONF_OUTCOME_UNKNOWN");
    assert_eq!(env.count("POST", &api("/api/v2/pages")).await, 1);

    // A 429 is Confluence refusing it.
    let env = Env::new().await;
    mount_space(&env).await;
    status_only(&env, "POST", &api("/api/v2/pages"), 429).await;
    let error = env
        .adapter
        .publish_page(&dest, "Rates", "x", None, key(), None)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "CONF_THROTTLED");
}

#[tokio::test]
async fn a_copy_destination_of_another_kind_is_refused() {
    let env = Env::new().await;
    let error = env
        .adapter
        .publish_page(
            &CopyDestination::SharepointFile {
                site: "s".to_owned(),
                library: "l".to_owned(),
                folder: "f".to_owned(),
            },
            "Rates",
            "x",
            None,
            key(),
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "CONF_LOCATION_INVALID");
}

#[tokio::test]
async fn an_attachment_with_a_version_expects_that_name_on_the_page() {
    let env = Env::new().await;
    mount_page(&env, PAGE, "Rates", 7).await;
    mount_attachments(&env, PAGE, vec![]).await;
    let error = env
        .adapter
        .attach(PAGE, "a.pdf", b"x".to_vec(), Some("1"), key(), None)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "CONF_NOT_FOUND");
}

#[tokio::test]
async fn an_attachment_is_moved_to_the_trash_and_its_row_goes() {
    let env = Env::new().await;
    let link = tracked_tree(&env).await;
    mount_page(&env, PAGE, "Rates", 7).await;
    mount_attachments(&env, PAGE, vec![attachment_json("att1", "a.pdf", 1, 4)]).await;
    status_only(&env, "DELETE", &api("/api/v2/attachments/att1"), 204).await;

    env.adapter
        .delete(&attachment_ref(), None, key(), None)
        .await
        .unwrap();

    let gone = env
        .adapter
        .item_for_path(link.id, "Supplier pages/Rates/a.pdf")
        .await
        .unwrap_err();
    assert_eq!(gone.code(), "CONF_NOT_FOUND");
}

#[tokio::test]
async fn deleting_a_page_already_gone_is_not_found() {
    let env = Env::new().await;
    status_only(&env, "GET", &page_url(PAGE), 404).await;
    let error = env
        .adapter
        .delete(&page_ref(), None, key(), None)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "CONF_NOT_FOUND");
    assert!(env.actions().await.is_empty());
}

#[tokio::test]
async fn a_delete_that_timed_out_but_landed_is_confirmed() {
    let env = Env::with_timeout(Duration::from_millis(250)).await;
    reply_once(
        &env,
        "GET",
        &page_url(PAGE),
        200,
        page_json(PAGE, "Rates", 3),
    )
    .await;
    status_only(&env, "GET", &page_url(PAGE), 404).await;
    Mock::given(method("DELETE"))
        .and(path(page_url(PAGE)))
        .respond_with(ResponseTemplate::new(204).set_delay(Duration::from_secs(2)))
        .mount(&env.mock)
        .await;

    env.adapter
        .delete(&page_ref(), Some("3"), key(), None)
        .await
        .unwrap();

    assert_eq!(env.actions().await, ["confirmed"]);
}

#[tokio::test]
async fn a_delete_that_timed_out_with_the_version_unmoved_goes_once_more() {
    let env = Env::with_timeout(Duration::from_millis(250)).await;
    mount_page(&env, PAGE, "Rates", 3).await;
    Mock::given(method("DELETE"))
        .and(path(page_url(PAGE)))
        .respond_with(ResponseTemplate::new(204).set_delay(Duration::from_secs(2)))
        .up_to_n_times(1)
        .mount(&env.mock)
        .await;
    status_only(&env, "DELETE", &page_url(PAGE), 204).await;

    env.adapter
        .delete(&page_ref(), Some("3"), key(), None)
        .await
        .unwrap();

    assert_eq!(env.count("DELETE", &page_url(PAGE)).await, 2);
}

#[tokio::test]
async fn a_delete_that_timed_out_on_a_moved_item_is_unknown() {
    let env = Env::with_timeout(Duration::from_millis(250)).await;
    reply_once(
        &env,
        "GET",
        &page_url(PAGE),
        200,
        page_json(PAGE, "Rates", 3),
    )
    .await;
    mount_page(&env, PAGE, "Rates", 4).await;
    Mock::given(method("DELETE"))
        .and(path(page_url(PAGE)))
        .respond_with(ResponseTemplate::new(204).set_delay(Duration::from_secs(2)))
        .mount(&env.mock)
        .await;

    let error = env
        .adapter
        .delete(&page_ref(), Some("3"), key(), None)
        .await
        .unwrap_err();

    assert_eq!(error.code(), "CONF_OUTCOME_UNKNOWN");
    assert_eq!(env.count("DELETE", &page_url(PAGE)).await, 1);
}

#[tokio::test]
async fn a_refreshed_token_is_used_and_the_cloud_id_read_again() {
    let env = Env::with_sign_in(SignIn::Refreshable).await;
    reply(
        &env,
        "POST",
        "/token",
        200,
        json!({"access_token": "at-2", "refresh_token": "rt-2", "expires_in": 3600, "token_type": "bearer"}),
    )
    .await;
    Mock::given(method("GET"))
        .and(path(page_url(PAGE)))
        .respond_with(ResponseTemplate::new(401))
        .up_to_n_times(1)
        .mount(&env.mock)
        .await;
    mount_page(&env, PAGE, "Rates", 7).await;

    assert_eq!(
        env.adapter.item_version(&page_ref()).await.unwrap().tag,
        "7"
    );
    assert_eq!(
        env.count("GET", "/oauth/token/accessible-resources").await,
        2,
        "a new token resolves the site again"
    );
}

#[tokio::test]
async fn a_site_change_clears_the_cached_cloud_id() {
    let env = Env::new().await;
    env.mock.reset().await;
    env.mount_resources(&[(CLOUD, SITE), ("cloud-2", "https://new.atlassian.net/")])
        .await;
    mount_page(&env, PAGE, "Rates", 7).await;
    env.adapter.item_version(&page_ref()).await.unwrap();
    env.settings
        .send(ConfluenceSettings {
            site: Some(url::Url::parse("https://new.atlassian.net").unwrap()),
        })
        .unwrap();
    reply(
        &env,
        "GET",
        &format!("/ex/confluence/cloud-2/wiki/api/v2/pages/{PAGE}"),
        200,
        page_json(PAGE, "Rates", 9),
    )
    .await;

    assert_eq!(
        env.adapter.item_version(&page_ref()).await.unwrap().tag,
        "9"
    );
}

#[tokio::test]
async fn an_author_lookup_that_fails_names_nobody() {
    let env = Env::new().await;
    mount_page(&env, PAGE, "Rates", 7).await;
    status_only(&env, "GET", &api("/rest/api/user"), 403).await;
    let version = env.adapter.item_version(&page_ref()).await.unwrap();
    assert_eq!(version.changed_by, None);
}

#[tokio::test]
async fn an_author_is_looked_up_once_per_account() {
    let env = Env::new().await;
    mount_author(&env).await;
    mount_page(&env, PAGE, "Rates", 7).await;
    env.adapter.item_version(&page_ref()).await.unwrap();
    env.adapter.item_version(&page_ref()).await.unwrap();
    assert_eq!(env.count("GET", &api("/rest/api/user")).await, 1);
}

#[tokio::test]
async fn an_empty_folder_shows_no_files_on_the_card() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Proposed);
    env.adapter
        .request_link_approval(&link, &env.place, &env.workspace)
        .await
        .unwrap();
    let pending = env.approvals.list_pending(None, None).await.unwrap();
    assert!(pending.items[0].card.content.is_none());
    assert!(pending.items[0].card.buttons.show_details.is_none());
}

#[tokio::test]
async fn a_link_card_names_at_most_fifty_files_and_counts_them_all() {
    let env = Env::new().await;
    mount_where(&env).await;
    for n in 0..120 {
        env.save(&format!("quotes/f{n}.md"), "x");
    }
    let mut link = env.link(ConfluenceLinkState::Proposed);
    link.folder = "quotes".to_owned();
    env.adapter
        .request_link_approval(&link, &env.place, &env.workspace)
        .await
        .unwrap();
    let pending = env.approvals.list_pending(None, None).await.unwrap();
    let card = &pending.items[0].card;
    assert_eq!(card.question, "Publish quotes to Confluence?");
    match &card.affects[0] {
        ordin_types::wire::approval::AffectedItem::File { label, .. } => {
            assert_eq!(label, "120 documents in quotes");
        }
        other => panic!("{other:?}"),
    }
    match &card.content {
        Some(ordin_types::wire::approval::ApprovalContent::Files { files, .. }) => {
            assert_eq!(files.len(), 50);
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_parent_page_that_is_gone_is_not_found() {
    let env = Env::new().await;
    mount_space(&env).await;
    status_only(&env, "GET", &page_url(PARENT), 404).await;
    let error = env
        .adapter
        .request_link_approval(
            &env.link(ConfluenceLinkState::Proposed),
            &env.place,
            &env.workspace,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "CONF_NOT_FOUND");
}

#[tokio::test]
async fn a_folder_link_uses_paths_relative_to_its_folder() {
    let env = Env::new().await;
    let mut link = env.link(ConfluenceLinkState::Active);
    link.folder = "quotes".to_owned();
    env.save("quotes/deep/notes.md", "x");
    env.save("outside.md", "y");

    env.adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();

    let ready = env.adapter.ready_to_publish(link.id).await.unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].path, "deep/notes.md");
    let copies = env.files.published_copies_via_link(link.id).await.unwrap();
    match &copies[0].original {
        CopyOriginal::PlaceFile { path, .. } => assert_eq!(path, "quotes/deep/notes.md"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_file_waiting_stays_on_the_ready_list_across_syncs() {
    let env = Env::new().await;
    let link = env.link(ConfluenceLinkState::Active);
    env.save("notes.md", "Notes");
    env.adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();
    let report = env
        .adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();
    assert_eq!(report.ready_to_publish, 1);
    assert_eq!(
        env.files
            .published_copies_via_link(link.id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn a_copy_whose_file_is_gone_waits_as_removed() {
    let env = Env::new().await;
    let link = env.link(ConfluenceLinkState::Active);
    let copy = env.page_copy(&link, "gone.md", PAGE, "2", "h").await;

    let report = env
        .adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();

    assert_eq!(report.ready_to_publish, 1);
    let ready = env.adapter.ready_to_publish(link.id).await.unwrap();
    assert_eq!(ready[0].kind, ReadyKind::Removed);
    assert_eq!(ready[0].published_copy_id, Some(copy.id));
    assert!(
        env.confluence_requests().await.is_empty(),
        "nothing deleted"
    );
}

#[tokio::test]
async fn copies_in_other_states_are_not_pushed() {
    let env = Env::new().await;
    let link = env.link(ConfluenceLinkState::Active);
    env.version("rates.md", "# Rates", FileVersionOrigin::Person)
        .await;
    let hash = env.hash_of("rates.md").await;
    let copy = env.page_copy(&link, "rates.md", PAGE, "7", &hash).await;
    env.files
        .set_published_copy_state(copy.id, PublishedCopyState::NotKeptUpToDate, None)
        .await
        .unwrap();
    env.version("rates.md", "# Changed", FileVersionOrigin::Person)
        .await;

    let report = env
        .adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();

    assert_eq!(report.pushed, 0);
    assert!(env.confluence_requests().await.is_empty());
}

#[tokio::test]
async fn a_failed_automatic_push_counts_and_the_others_go_on() {
    let env = Env::new().await;
    let link = env.link(ConfluenceLinkState::Active);
    env.version("a.md", "a", FileVersionOrigin::Person).await;
    env.version("b.md", "b", FileVersionOrigin::Person).await;
    let a = env.hash_of("a.md").await;
    let b = env.hash_of("b.md").await;
    env.page_copy(&link, "a.md", "1", "1", &a).await;
    let good = env.page_copy(&link, "b.md", "2", "1", &b).await;
    env.version("a.md", "a2", FileVersionOrigin::Person).await;
    env.version("b.md", "b2", FileVersionOrigin::Person).await;
    status_only(&env, "GET", &page_url("1"), 403).await;
    mount_page(&env, "2", "b", 1).await;
    mount_put(&env, "2", "b", 2).await;

    let report = env
        .adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();

    assert_eq!((report.failed, report.pushed), (1, 1));
    assert_eq!(
        env.copy(good.id)
            .await
            .destination_version_pushed
            .as_deref(),
        Some("2")
    );
}

#[tokio::test]
async fn a_lost_sign_in_stops_the_sync() {
    let env = Env::new().await;
    let link = env.link(ConfluenceLinkState::Active);
    env.version("a.md", "a", FileVersionOrigin::Person).await;
    let a = env.hash_of("a.md").await;
    env.page_copy(&link, "a.md", "1", "1", &a).await;
    env.version("a.md", "a2", FileVersionOrigin::Person).await;
    status_only(&env, "GET", &page_url("1"), 401).await;

    let error = env
        .adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap_err();

    assert_eq!(error.code(), "CONF_SIGN_IN_NEEDED");
}

#[tokio::test]
async fn an_unknown_outcome_on_an_automatic_push_fails_the_copy_with_the_sentence() {
    let env = Env::with_timeout(Duration::from_millis(250)).await;
    let link = env.link(ConfluenceLinkState::Active);
    env.version("a.md", "a", FileVersionOrigin::Person).await;
    let a = env.hash_of("a.md").await;
    let copy = env.page_copy(&link, "a.md", PAGE, "7", &a).await;
    env.version("a.md", "a2", FileVersionOrigin::Person).await;
    // The check and the update's read see 7; the read after the timeout sees 8.
    reply_once(&env, "GET", &page_url(PAGE), 200, page_json(PAGE, "a", 7)).await;
    reply_once(&env, "GET", &page_url(PAGE), 200, page_json(PAGE, "a", 7)).await;
    mount_page(&env, PAGE, "a", 8).await;
    Mock::given(method("PUT"))
        .and(path(page_url(PAGE)))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(2)))
        .mount(&env.mock)
        .await;

    let report = env
        .adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();

    assert_eq!(report.failed, 1);
    let copy = env.copy(copy.id).await;
    assert_eq!(copy.state, PublishedCopyState::Failed);
    assert_eq!(
        copy.failure_reason.as_deref(),
        Some("ordin isn't sure whether Confluence received this change.")
    );
    assert_eq!(env.count("PUT", &page_url(PAGE)).await, 1);
}

#[tokio::test]
async fn an_attachment_copy_gets_a_new_version_on_its_page() {
    let env = Env::new().await;
    let link = env.link(ConfluenceLinkState::Active);
    env.version("a.pdf", "one", FileVersionOrigin::Person).await;
    let hash = env.hash_of("a.pdf").await;
    let copy = env
        .files
        .record_published_copy(PublishedCopy {
            id: PublishedCopyId::new(),
            original: CopyOriginal::PlaceFile {
                place_id: env.place_id(),
                path: "a.pdf".to_owned(),
            },
            destination: CopyDestination::ConfluenceAttachment {
                space: "PROC".to_owned(),
                parent_page_id: PAGE.to_owned(),
            },
            destination_item_id: Some("att1".to_owned()),
            format: CopyFormat::ExactFile,
            maker: CopyMaker::None,
            published_via: ordin_types::domain::places::PublishedVia::ConfluenceLink {
                confluence_link_id: link.id,
            },
            followed_branch: None,
            keep_up_to_date: true,
            audience: "Team".to_owned(),
            original_version_published: Some(hash),
            destination_version_pushed: Some("3".to_owned()),
            last_updated_at: None,
            state: PublishedCopyState::Current,
            failure_reason: None,
        })
        .await
        .unwrap();
    env.version("a.pdf", "two", FileVersionOrigin::Person).await;
    mount_page(&env, PAGE, "Rates", 7).await;
    mount_attachments(&env, PAGE, vec![attachment_json("att1", "a.pdf", 3, 3)]).await;
    reply(
        &env,
        "POST",
        &api(&format!(
            "/rest/api/content/{PAGE}/child/attachment/att1/data"
        )),
        200,
        attachment_json("att1", "a.pdf", 4, 3),
    )
    .await;

    let report = env
        .adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();

    assert_eq!(report.pushed, 1);
    assert_eq!(
        env.copy(copy.id)
            .await
            .destination_version_pushed
            .as_deref(),
        Some("4")
    );
}

/// `Keep mine`: the copy holds the destination's version, so the next sync pushes.
#[tokio::test]
async fn keep_mine_lets_the_next_push_replace_the_page() {
    let env = Env::new().await;
    let link = env.link(ConfluenceLinkState::Active);
    env.version("rates.md", "# Rates", FileVersionOrigin::Person)
        .await;
    let hash = env.hash_of("rates.md").await;
    let copy = env.page_copy(&link, "rates.md", PAGE, "7", &hash).await;
    env.version("rates.md", "# Mine", FileVersionOrigin::Person)
        .await;
    mount_page(&env, PAGE, "rates", 8).await;
    env.adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();

    env.adapter.keep_local(copy.id).await.unwrap();

    let held = env.copy(copy.id).await;
    assert_eq!(held.state, PublishedCopyState::Current);
    assert_eq!(held.destination_version_pushed.as_deref(), Some("8"));
    mount_put(&env, PAGE, "rates", 9).await;
    let report = env
        .adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();
    assert_eq!(report.pushed, 1);
}

#[tokio::test]
async fn forgetting_a_link_deletes_its_rows_and_sends_nothing() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    env.save("notes.md", "Notes");
    env.adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();
    let items = env.adapter.ready_to_publish(link.id).await.unwrap();
    env.adapter
        .request_publish_checked(&link, &env.place, items)
        .await
        .unwrap();

    env.adapter.forget_link(link.id).await.unwrap();

    assert_eq!(
        env.scalar("SELECT count(*) FROM confluence_ready_items")
            .await,
        0
    );
    assert_eq!(
        env.scalar("SELECT count(*) FROM confluence_publish_requests")
            .await,
        0
    );
    assert!(env.writes().await.is_empty());
}

async fn asked(
    env: &Env,
    link: &ordin_types::domain::places::ConfluenceLink,
    items: Vec<ReadyItem>,
) -> ApprovalId {
    env.adapter
        .request_publish_checked(link, &env.place, items)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_refused_card_leaves_the_items_ready() {
    for state in ["rejected", "expired"] {
        let env = Env::new().await;
        mount_where(&env).await;
        let link = env.link(ConfluenceLinkState::Active);
        env.save("notes.md", "Notes");
        env.adapter
            .sync_linked_folder(&link, &env.place)
            .await
            .unwrap();
        let items = env.adapter.ready_to_publish(link.id).await.unwrap();
        let approval = asked(&env, &link, items).await;

        decide(&env, approval, state).await;

        assert_eq!(
            env.adapter.ready_to_publish(link.id).await.unwrap().len(),
            1
        );
        assert_eq!(
            env.scalar("SELECT count(*) FROM confluence_publish_requests")
                .await,
            0
        );
        assert!(env.writes().await.is_empty());
    }
}

#[tokio::test]
async fn events_of_other_kinds_and_shapes_are_ignored() {
    let env = Env::new().await;
    let sink = env.adapter.event_sink();
    assert_eq!(sink.name(), "confluence");
    for (kind, data) in [
        (
            "run_state_changed",
            json!({"approval_id": ApprovalId::new()}),
        ),
        ("approval_decided", json!({"approval_id": "nonsense"})),
    ] {
        sink.deliver(&JournalEvent {
            backend: uuid::Uuid::now_v7(),
            seq: 1,
            at: Timestamp::now(),
            kind: kind.to_owned(),
            data,
        })
        .await;
    }
    assert!(env.requests().await.is_empty());
}

#[tokio::test]
async fn a_pending_decision_changes_nothing() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    env.save("notes.md", "Notes");
    env.adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();
    let items = env.adapter.ready_to_publish(link.id).await.unwrap();
    let approval = asked(&env, &link, items).await;
    decide(&env, approval, "pending").await;
    assert_eq!(
        env.scalar("SELECT count(*) FROM confluence_publish_requests")
            .await,
        1
    );
}

#[tokio::test]
async fn an_approved_rename_retitles_the_page_and_moves_the_copy() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    env.save("final.md", "Same");
    let hash = env.hash_of("final.md").await;
    let copy = env.page_copy(&link, "draft.md", PAGE, "2", &hash).await;
    env.adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();
    let items = env.adapter.ready_to_publish(link.id).await.unwrap();
    let approval = asked(&env, &link, items).await;
    mount_page(&env, PAGE, "draft", 2).await;
    mount_put(&env, PAGE, "final", 3).await;

    decide(&env, approval, "approved").await;

    let body = env.body_of("PUT", &page_url(PAGE)).await;
    assert_eq!(body["title"], "final");
    assert_eq!(body["version"]["number"], 3);
    assert_eq!(body["body"]["value"], "<p>Body</p>");
    let copy = env.copy(copy.id).await;
    assert_eq!(copy.destination_version_pushed.as_deref(), Some("3"));
    match copy.original {
        CopyOriginal::PlaceFile { path, .. } => assert_eq!(path, "final.md"),
        other => panic!("{other:?}"),
    }
    assert!(env
        .adapter
        .ready_to_publish(link.id)
        .await
        .unwrap()
        .is_empty());
}

/// A published attachment whose file was renamed to `final.pdf`, read as the sync reads it.
async fn renamed_attachment(
    env: &Env,
    link: &ordin_types::domain::places::ConfluenceLink,
) -> PublishedCopy {
    env.save("final.pdf", "Same");
    let hash = env.hash_of("final.pdf").await;
    env.files
        .record_published_copy(PublishedCopy {
            id: PublishedCopyId::new(),
            original: CopyOriginal::PlaceFile {
                place_id: env.place_id(),
                path: "draft.pdf".to_owned(),
            },
            destination: CopyDestination::ConfluenceAttachment {
                space: "PROC".to_owned(),
                parent_page_id: PAGE.to_owned(),
            },
            destination_item_id: Some("att1".to_owned()),
            format: CopyFormat::ExactFile,
            maker: CopyMaker::None,
            published_via: ordin_types::domain::places::PublishedVia::ConfluenceLink {
                confluence_link_id: link.id,
            },
            followed_branch: None,
            keep_up_to_date: true,
            audience: "Team".to_owned(),
            original_version_published: Some(hash),
            destination_version_pushed: Some("3".to_owned()),
            last_updated_at: None,
            state: PublishedCopyState::Current,
            failure_reason: None,
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn an_approved_rename_of_an_attachment_uploads_it_under_the_new_name() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    let copy = renamed_attachment(&env, &link).await;
    env.adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();
    let items = env.adapter.ready_to_publish(link.id).await.unwrap();
    let approval = asked(&env, &link, items).await;
    mount_page(&env, PAGE, "Rates", 7).await;
    reply(
        &env,
        "POST",
        &api(&format!("/rest/api/content/{PAGE}/child/attachment")),
        200,
        json!({"results": [attachment_json("att2", "final.pdf", 1, 4)]}),
    )
    .await;

    decide(&env, approval, "approved").await;

    let copy = env.copy(copy.id).await;
    assert_eq!(copy.destination_item_id.as_deref(), Some("att2"));
}

#[tokio::test]
async fn an_approved_removal_leaves_the_page_in_place() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    let copy = env.page_copy(&link, "gone.md", PAGE, "2", "h").await;
    env.adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();
    let items = env.adapter.ready_to_publish(link.id).await.unwrap();
    let approval = asked(&env, &link, items).await;

    decide(&env, approval, "approved").await;

    assert_eq!(env.copy(copy.id).await.state, PublishedCopyState::Removed);
    assert!(
        env.writes().await.is_empty(),
        "no Confluence page is deleted"
    );
    // A removed copy does not come back to the list.
    env.adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();
    assert!(env
        .adapter
        .ready_to_publish(link.id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn an_item_that_fails_stays_ready_and_the_request_is_closed() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    env.save("notes.md", "Notes");
    env.adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();
    let items = env.adapter.ready_to_publish(link.id).await.unwrap();
    let approval = asked(&env, &link, items).await;
    status_only(&env, "POST", &api("/api/v2/pages"), 403).await;

    decide(&env, approval, "approved").await;

    assert_eq!(
        env.adapter.ready_to_publish(link.id).await.unwrap().len(),
        1
    );
    assert_eq!(
        env.scalar("SELECT count(*) FROM confluence_publish_requests")
            .await,
        0
    );
}

/// A linked file synced into Ready to publish, and the card that asks for it.
async fn carded(
    env: &Env,
    link: &ordin_types::domain::places::ConfluenceLink,
) -> (PublishedCopyId, ApprovalId) {
    env.adapter
        .sync_linked_folder(link, &env.place)
        .await
        .unwrap();
    let copies = env.files.published_copies_via_link(link.id).await.unwrap();
    let items = env.adapter.ready_to_publish(link.id).await.unwrap();
    (copies[0].id, asked(env, link, items).await)
}

async fn mount_new_page(env: &Env) {
    reply(
        env,
        "POST",
        &api("/api/v2/pages"),
        200,
        page_json("6100", "notes", 1),
    )
    .await;
    mount_page(env, "6100", "notes", 1).await;
}

#[tokio::test]
async fn text_changed_after_the_card_waits_for_a_fresh_approval() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    env.save("notes.md", "What the person saw");
    let (copy, approval) = carded(&env, &link).await;
    env.save("notes.md", "Something else");
    mount_new_page(&env).await;

    decide(&env, approval, "approved").await;

    assert!(env.writes().await.is_empty(), "nothing was sent");
    assert_eq!(
        env.copy(copy).await.state,
        PublishedCopyState::ReadyToPublish
    );
    let items = env.adapter.ready_to_publish(link.id).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(
        env.scalar("SELECT count(*) FROM confluence_publish_requests")
            .await,
        0
    );

    // A fresh card binds the new text, and that is what goes.
    let approval = asked(&env, &link, items).await;
    decide(&env, approval, "approved").await;

    let copy = env.copy(copy).await;
    assert_eq!(copy.state, PublishedCopyState::Current);
    assert_eq!(
        copy.original_version_published,
        Some(env.hash_of("notes.md").await)
    );
}

#[tokio::test]
async fn a_rename_changed_after_the_card_is_not_uploaded() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    let copy = renamed_attachment(&env, &link).await;
    env.adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();
    let items = env.adapter.ready_to_publish(link.id).await.unwrap();
    let approval = asked(&env, &link, items).await;
    env.save("final.pdf", "Something else");
    mount_page(&env, PAGE, "Rates", 7).await;
    reply(
        &env,
        "POST",
        &api(&format!("/rest/api/content/{PAGE}/child/attachment")),
        200,
        json!({"results": [attachment_json("att2", "final.pdf", 1, 4)]}),
    )
    .await;

    decide(&env, approval, "approved").await;

    assert!(env.writes().await.is_empty(), "nothing was sent");
    assert_eq!(
        env.copy(copy.id).await.destination_item_id.as_deref(),
        Some("att1")
    );
    assert_eq!(
        env.adapter.ready_to_publish(link.id).await.unwrap().len(),
        1
    );
}

#[tokio::test]
async fn a_renamed_attachment_an_agent_wrote_after_the_card_is_not_uploaded() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    renamed_attachment(&env, &link).await;
    env.adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();
    env.version("final.pdf", "Same", FileVersionOrigin::Person)
        .await;
    let items = env.adapter.ready_to_publish(link.id).await.unwrap();
    let approval = asked(&env, &link, items).await;
    // The same bytes, so the card's hash still holds; only the author changed.
    env.version(
        "final.pdf",
        "Same",
        FileVersionOrigin::Run {
            run_id: RunId::new(),
            tool: "write_file".to_owned(),
        },
    )
    .await;
    mount_page(&env, PAGE, "Rates", 7).await;
    reply(
        &env,
        "POST",
        &api(&format!("/rest/api/content/{PAGE}/child/attachment")),
        200,
        json!({"results": [attachment_json("att2", "final.pdf", 1, 4)]}),
    )
    .await;

    decide(&env, approval, "approved").await;

    assert!(env.writes().await.is_empty(), "nothing was sent");
}

// Scenario: Happy path — an agent's new file goes once the card names it
#[tokio::test]
async fn happy_path_an_agents_new_file_goes_once_the_card_names_it() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    env.version(
        "notes.md",
        "Written by an agent",
        FileVersionOrigin::Run {
            run_id: RunId::new(),
            tool: "write_file".to_owned(),
        },
    )
    .await;
    let (copy, approval) = carded(&env, &link).await;
    let record = env.approvals.get(approval).await.unwrap();
    assert!(
        record
            .card
            .changes
            .contains("An agent may have written 1 of them."),
        "{}",
        record.card.changes
    );
    assert!(record.card.buttons.show_details.is_some());
    mount_new_page(&env).await;

    decide(&env, approval, "approved").await;

    assert_eq!(env.copy(copy).await.state, PublishedCopyState::Current);
    assert!(env
        .adapter
        .ready_to_publish(link.id)
        .await
        .unwrap()
        .is_empty());
}

// A card that named the agent showed the kept bytes; with none kept it showed the live file.
#[tokio::test]
async fn error_path_a_card_naming_the_agent_needs_the_bytes_it_showed_to_be_kept() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    env.version(
        "notes.md",
        "Written by an agent",
        FileVersionOrigin::Run {
            run_id: RunId::new(),
            tool: "write_file".to_owned(),
        },
    )
    .await;
    let (copy, approval) = carded(&env, &link).await;
    sqlx::query("DELETE FROM file_versions")
        .execute(&env.pool)
        .await
        .unwrap();

    decide(&env, approval, "approved").await;

    // Held, not sent: an attempted send would leave the copy `Failed` or `Current`.
    assert_eq!(
        env.copy(copy).await.state,
        PublishedCopyState::ReadyToPublish
    );
}

// Scenario: Error path — a card that did not name the agent holds the item and says why
#[tokio::test]
async fn error_path_a_card_that_did_not_name_the_agent_holds_the_item_and_says_why() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    env.version("notes.md", "The text", FileVersionOrigin::Person)
        .await;
    let (copy, approval) = carded(&env, &link).await;
    let record = env.approvals.get(approval).await.unwrap();
    assert!(!record.card.changes.contains("agent"));
    assert!(record.card.buttons.show_details.is_none());
    env.version(
        "notes.md",
        "The text",
        FileVersionOrigin::Run {
            run_id: RunId::new(),
            tool: "write_file".to_owned(),
        },
    )
    .await;
    mount_new_page(&env).await;

    decide(&env, approval, "approved").await;

    assert!(env.writes().await.is_empty(), "nothing was sent");
    assert_eq!(
        env.copy(copy).await.state,
        PublishedCopyState::ReadyToPublish
    );
    assert_eq!(
        env.adapter.ready_to_publish(link.id).await.unwrap().len(),
        1
    );
    let held = env
        .journal
        .list_entries(
            ordin_journal::EntryFilter {
                run_id: None,
                kind: Some("confluence_item_held".to_owned()),
            },
            None,
        )
        .await
        .unwrap()
        .items;
    assert!(held[0].summary.contains("an agent changed it"));
}

// The approval binds a hash, so the card must show those bytes and not the file as it stands.
#[tokio::test]
async fn the_card_shows_the_bytes_it_binds_not_the_live_file() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    env.version(
        "notes.md",
        "What the approval covers",
        FileVersionOrigin::Run {
            run_id: RunId::new(),
            tool: "write_file".to_owned(),
        },
    )
    .await;
    let (_, approval) = carded(&env, &link).await;
    env.save("notes.md", "Something harmless to look at");

    let record = env.approvals.get(approval).await.unwrap();
    let Some(ApprovalContent::Files { files, .. }) = record.card.content else {
        panic!("the card lists the file");
    };
    let [ContentRef::FileVersion { file_version_id }] = files.as_slice() else {
        panic!("the card names a kept version: {files:?}");
    };
    let shown = env.files.read_version(*file_version_id).await.unwrap();
    assert_eq!(shown.bytes, b"What the approval covers");
}

// Scenario: Error path — a rename that sent changed bytes records them
#[tokio::test]
async fn error_path_a_rename_that_sent_changed_bytes_records_them() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    let copy = renamed_attachment(&env, &link).await;
    env.adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();
    env.save("final.pdf", "Changed since it was renamed");
    let hash = env.hash_of("final.pdf").await;
    let items = env.adapter.ready_to_publish(link.id).await.unwrap();
    let approval = asked(&env, &link, items).await;
    mount_page(&env, PAGE, "Rates", 7).await;
    reply(
        &env,
        "POST",
        &api(&format!("/rest/api/content/{PAGE}/child/attachment")),
        200,
        json!({"results": [attachment_json("att2", "final.pdf", 1, 4)]}),
    )
    .await;

    decide(&env, approval, "approved").await;

    let copy = env.copy(copy.id).await;
    assert_eq!(
        copy.original_version_published.as_deref(),
        Some(hash.as_str())
    );
    assert_eq!(
        env.files
            .versions(env.place_id(), "final.pdf")
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn a_first_push_gives_the_file_a_history() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    env.save("notes.md", "The person's text");
    let (copy, approval) = carded(&env, &link).await;
    mount_new_page(&env).await;

    decide(&env, approval, "approved").await;

    assert_eq!(env.copy(copy).await.state, PublishedCopyState::Current);
    let versions = env
        .files
        .versions(env.place_id(), "notes.md")
        .await
        .unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].origin, FileVersionOrigin::Person);
}

#[tokio::test]
async fn the_card_names_the_version_that_holds_the_bytes() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    env.version("a.md", "one", FileVersionOrigin::Person).await;
    env.version("a.md", "two", FileVersionOrigin::Person).await;
    // Changed outside ordin: no version holds these bytes.
    env.version("b.md", "one", FileVersionOrigin::Person).await;
    env.save("b.md", "two");
    // Never versioned.
    env.save("c.md", "one");
    carded(&env, &link).await;

    let pending = env.approvals.list_pending(None, None).await.unwrap();
    let versions: Vec<(String, Option<String>)> = pending.items[0]
        .card
        .affects
        .iter()
        .map(|item| match item {
            AffectedItem::File { label, version, .. } => (label.clone(), version.clone()),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        versions,
        [
            ("a.md".to_owned(), Some("2".to_owned())),
            ("b.md".to_owned(), None),
            ("c.md".to_owned(), None),
        ]
    );
}

#[tokio::test]
async fn a_failed_first_push_fails_the_copy_with_its_reason_and_can_be_approved_again() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    env.save("notes.md", "Notes");
    let (copy, approval) = carded(&env, &link).await;
    reply_once(&env, "POST", &api("/api/v2/pages"), 403, json!({})).await;

    decide(&env, approval, "approved").await;

    let failed = env.copy(copy).await;
    assert_eq!(failed.state, PublishedCopyState::Failed);
    assert_eq!(
        failed.failure_reason.as_deref(),
        Some("ordin couldn't publish this to Confluence (CONF_ACCESS_DENIED).")
    );

    let items = env.adapter.ready_to_publish(link.id).await.unwrap();
    let approval = asked(&env, &link, items).await;
    mount_new_page(&env).await;
    decide(&env, approval, "approved").await;

    let published = env.copy(copy).await;
    assert_eq!(published.state, PublishedCopyState::Current);
    assert_eq!(published.failure_reason, None);
}

#[tokio::test]
async fn an_unknown_outcome_on_a_first_push_says_the_page_may_be_live() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    env.save("notes.md", "Notes");
    let (copy, approval) = carded(&env, &link).await;
    status_only(&env, "POST", &api("/api/v2/pages"), 500).await;

    decide(&env, approval, "approved").await;

    let copy = env.copy(copy).await;
    assert_eq!(copy.state, PublishedCopyState::Failed);
    assert!(
        copy.failure_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("may already be live")),
        "{:?}",
        copy.failure_reason
    );
    assert_eq!(env.count("POST", &api("/api/v2/pages")).await, 1);
}

struct Word(Result<String, String>);

#[async_trait::async_trait]
impl WordToMarkdown for Word {
    async fn convert(&self, _: Vec<u8>) -> Result<String, String> {
        self.0.clone()
    }
}

async fn word_env(reply_with: Result<String, String>) -> Env {
    Env::with(Options {
        word: Some(Arc::new(Word(reply_with))),
        ..Options::default()
    })
    .await
}

#[tokio::test]
async fn with_word_wired_a_document_becomes_a_page() {
    let env = word_env(Ok("# Terms".to_owned())).await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    env.save("terms.docx", "PK");
    env.adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();
    let copies = env.files.published_copies_via_link(link.id).await.unwrap();
    assert_eq!(copies[0].format, CopyFormat::Page);
    assert_eq!(copies[0].maker, CopyMaker::Ordin);
    let items = env.adapter.ready_to_publish(link.id).await.unwrap();
    let approval = asked(&env, &link, items).await;
    reply(
        &env,
        "POST",
        &api("/api/v2/pages"),
        200,
        page_json("6100", "terms", 1),
    )
    .await;

    decide(&env, approval, "approved").await;

    let copy = env.copy(copies[0].id).await;
    assert_eq!(copy.state, PublishedCopyState::Current);
    assert_eq!(copy.format, CopyFormat::Page);
    assert_eq!(copy.destination_item_id.as_deref(), Some("6100"));
}

#[tokio::test]
async fn a_failed_word_conversion_is_attached_as_the_exact_file() {
    let env = word_env(Err("CRAFT_FAILED".to_owned())).await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    env.save("terms.docx", "PK");
    env.adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();
    let copies = env.files.published_copies_via_link(link.id).await.unwrap();
    let items = env.adapter.ready_to_publish(link.id).await.unwrap();
    let approval = asked(&env, &link, items).await;
    reply(
        &env,
        "POST",
        &api("/api/v2/pages"),
        200,
        page_json("6100", "Supplier quotes", 1),
    )
    .await;
    mount_page(&env, "6100", "Supplier quotes", 1).await;
    reply(
        &env,
        "POST",
        &api("/rest/api/content/6100/child/attachment"),
        200,
        json!({"results": [attachment_json("att5", "terms.docx", 1, 2)]}),
    )
    .await;

    decide(&env, approval, "approved").await;

    let copy = env.copy(copies[0].id).await;
    assert_eq!(copy.state, PublishedCopyState::Current);
    assert_eq!(copy.format, CopyFormat::ExactFile);
    assert_eq!(copy.maker, CopyMaker::None);
    assert!(matches!(
        copy.destination,
        CopyDestination::ConfluenceAttachment { .. }
    ));
    assert_eq!(copy.destination_item_id.as_deref(), Some("att5"));
}

#[tokio::test]
async fn text_that_is_not_utf8_cannot_become_a_page() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    std::fs::write(
        std::path::Path::new(&env.place.path).join("bad.md"),
        [0xff, 0xfe],
    )
    .unwrap();
    env.adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();
    let items = env.adapter.ready_to_publish(link.id).await.unwrap();
    let approval = asked(&env, &link, items).await;

    decide(&env, approval, "approved").await;

    assert!(env.writes().await.is_empty());
}
