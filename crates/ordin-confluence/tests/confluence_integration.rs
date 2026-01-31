//! Integration against a `wiremock` Confluence stand-in, with the real Identity, Approval and File
//! services (plan, Phase 7): a versioned page write, an attachment and `CONF_TOO_LARGE`, and the whole
//! way from a linked folder through an approval card to the published page.

mod support;

use ordin_approval::PressOrigin;
use ordin_confluence::{ConfluenceAdapter, ReadyKind, RemoteItemRef};
use ordin_files::PublishedCopyState;
use ordin_types::domain::places::{ConfluenceLinkState, CopyDestination, FileVersionOrigin};
use ordin_types::wire::approval::{ApprovalPress, PressDecision};
use serde_json::json;
use support::*;
use wiremock::matchers::{body_string_contains, header, method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

const PAGE: &str = "4411";

#[tokio::test]
async fn a_versioned_page_write_reads_then_writes_with_the_persons_token() {
    let env = Env::new().await;
    Mock::given(method("GET"))
        .and(path(api(&format!("/api/v2/pages/{PAGE}"))))
        .and(query_param("body-format", "storage"))
        .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_json(PAGE, "Rates", 7)))
        .expect(1)
        .mount(&env.mock)
        .await;
    Mock::given(method("PUT"))
        .and(path(api(&format!("/api/v2/pages/{PAGE}"))))
        .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .and(body_string_contains(
            "<h1>Rates</h1><p>Day rate: <strong>£640</strong></p>",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_json(PAGE, "Rates", 8)))
        .expect(1)
        .mount(&env.mock)
        .await;

    let version = env
        .adapter
        .update_page(
            PAGE,
            "Rates",
            "# Rates\n\nDay rate: **£640**\n",
            None,
            "7",
            key(),
            None,
        )
        .await
        .unwrap();

    assert_eq!(version.tag, "8");
    assert_eq!(env.actions().await, ["confirmed"]);
    assert_eq!(env.action_kinds().await, ["copy_publish"]);
    assert_eq!(
        env.count("GET", "/oauth/token/accessible-resources").await,
        1,
        "cloudId read once"
    );
}

#[tokio::test]
async fn a_working_copy_page_goes_back_as_an_origin_push() {
    let env = Env::new().await;
    mount_page(&env, PAGE, "Rates", 7).await;
    mount_put(&env, PAGE, "Rates", 8).await;

    let version = env
        .adapter
        .update_page_storage(PAGE, "<p>Edited</p>", "7", key(), None)
        .await
        .unwrap();

    assert_eq!(version.tag, "8");
    assert_eq!(env.action_kinds().await, ["origin_push"]);
    let body = env
        .body_of("PUT", &api(&format!("/api/v2/pages/{PAGE}")))
        .await;
    assert_eq!(body["title"], "Rates", "keeps the page's title");
    assert_eq!(body["body"]["value"], "<p>Edited</p>");
}

#[tokio::test]
async fn an_attachment_is_added_then_gets_a_new_version() {
    let env = Env::new().await;
    mount_page(&env, PAGE, "Rates", 7).await;
    Mock::given(method("POST"))
        .and(path(api(&format!(
            "/rest/api/content/{PAGE}/child/attachment"
        ))))
        .and(header("x-atlassian-token", "no-check"))
        .and(body_string_contains("name=\"file\"; filename=\"terms.pdf\""))
        .and(body_string_contains("%PDF-first"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [{
                "id": "att77",
                "title": "terms.pdf",
                "version": {"number": 1, "when": "2026-01-10T13:40:00.000Z", "by": {"accountId": "acc-1"}},
            }],
        })))
        .expect(1)
        .mount(&env.mock)
        .await;

    let (id, version) = env
        .adapter
        .attach(PAGE, "terms.pdf", b"%PDF-first".to_vec(), None, key(), None)
        .await
        .unwrap();

    assert_eq!((id.as_str(), version.tag.as_str()), ("att77", "1"));

    mount_attachments(
        &env,
        PAGE,
        vec![attachment_json("att77", "terms.pdf", 1, 10)],
    )
    .await;
    Mock::given(method("POST"))
        .and(path(api(&format!(
            "/rest/api/content/{PAGE}/child/attachment/att77/data"
        ))))
        .and(header("x-atlassian-token", "no-check"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "att77",
            "title": "terms.pdf",
            "version": {"number": 2, "when": "2026-01-11T13:40:00.000Z"},
        })))
        .expect(1)
        .mount(&env.mock)
        .await;

    let (id, version) = env
        .adapter
        .attach(
            PAGE,
            "terms.pdf",
            b"%PDF-second".to_vec(),
            Some("1"),
            key(),
            None,
        )
        .await
        .unwrap();

    assert_eq!((id.as_str(), version.tag.as_str()), ("att77", "2"));
    assert_eq!(env.actions().await, ["confirmed", "confirmed"]);
}

#[tokio::test]
async fn too_large_is_refused_on_every_path() {
    let env = Env::new().await;
    // A page body above 5 MiB on the way in.
    let body = "x".repeat(5 * 1024 * 1024 + 1);
    let mut big = page_json(PAGE, "Rates", 7);
    big["body"]["storage"]["value"] = json!(body);
    reply(
        &env,
        "GET",
        &api(&format!("/api/v2/pages/{PAGE}")),
        200,
        big,
    )
    .await;
    let item = RemoteItemRef::Page {
        page_id: PAGE.to_owned(),
    };
    assert_eq!(
        env.adapter.fetch(&item).await.unwrap_err().code(),
        "CONF_TOO_LARGE"
    );
    // An attachment above 100 MiB is not downloaded.
    mount_attachments(
        &env,
        PAGE,
        vec![attachment_json("att1", "big.zip", 1, 100 * 1024 * 1024 + 1)],
    )
    .await;
    let attachment = RemoteItemRef::Attachment {
        page_id: PAGE.to_owned(),
        attachment_id: "att1".to_owned(),
    };
    assert_eq!(
        env.adapter.fetch(&attachment).await.unwrap_err().code(),
        "CONF_TOO_LARGE"
    );
    assert_eq!(
        env.count("GET", &api("/download/attachments/att1/big.zip"))
            .await,
        0
    );
    // A 413 from Confluence on a create.
    mount_space(&env).await;
    status_only(&env, "POST", &api("/api/v2/pages"), 413).await;
    let error = env
        .adapter
        .publish_page(
            &CopyDestination::ConfluencePage {
                space: "PROC".to_owned(),
                parent_page_id: PARENT.to_owned(),
            },
            "Rates",
            "x",
            None,
            key(),
            None,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "CONF_TOO_LARGE");
}

#[tokio::test]
async fn a_linked_folder_publishes_through_one_real_approval() {
    let env = Env::new().await;
    mount_where(&env).await;
    env.save("rates.md", "# Rates");
    env.save("sub/terms.pdf", "%PDF");
    let link = env.link(ConfluenceLinkState::Active);

    let report = env
        .adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();
    assert_eq!(report.ready_to_publish, 2);
    let items = env.adapter.ready_to_publish(link.id).await.unwrap();
    assert!(items.iter().all(|i| i.kind == ReadyKind::New));

    // Folder pages, the text page, then the attachment on its folder's page.
    Mock::given(method("POST"))
        .and(path(api("/api/v2/pages")))
        .and(body_string_contains("\"title\":\"Supplier quotes\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_json(
            "6001",
            "Supplier quotes",
            1,
        )))
        .expect(1)
        .mount(&env.mock)
        .await;
    Mock::given(method("POST"))
        .and(path(api("/api/v2/pages")))
        .and(body_string_contains("\"title\":\"rates\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_json("6002", "rates", 1)))
        .expect(1)
        .mount(&env.mock)
        .await;
    Mock::given(method("POST"))
        .and(path(api("/api/v2/pages")))
        .and(body_string_contains("\"title\":\"sub\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(page_json("6003", "sub", 1)))
        .expect(1)
        .mount(&env.mock)
        .await;
    mount_page(&env, "6003", "sub", 1).await;
    Mock::given(method("POST"))
        .and(path(api("/rest/api/content/6003/child/attachment")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "results": [attachment_json("att5", "terms.pdf", 1, 4)],
        })))
        .expect(1)
        .mount(&env.mock)
        .await;

    let id = env
        .adapter
        .request_publish_checked(&link, &env.place, items)
        .await
        .unwrap();
    let pending = env.approvals.list_pending(None, None).await.unwrap();
    let card = pending
        .items
        .iter()
        .find(|c| c.card.approval_id == id)
        .map(|c| &c.card)
        .expect("the card");
    assert_eq!(card.question, "Publish 2 checked items to Confluence?");
    assert_eq!(card.verb, "Publish");
    assert_eq!(card.changes, "2 new, 0 renamed, 0 removed.");
    assert_eq!(card.affects.len(), 2);
    assert!(card.content.is_none());
    env.approvals
        .decide(
            ApprovalPress {
                card_ref: card.buttons.approve.clone(),
                decision: PressDecision::Approve,
            },
            PressOrigin::LocalScreen,
        )
        .await
        .unwrap();

    // The decision reaches the sink as the Journal would redeliver it.
    env.adapter.event_sink().lagged(1).await;

    let copies = env.files.published_copies_via_link(link.id).await.unwrap();
    assert_eq!(copies.len(), 2);
    assert!(copies
        .iter()
        .all(|c| c.state == PublishedCopyState::Current));
    assert!(copies
        .iter()
        .all(|c| c.destination_version_pushed.as_deref() == Some("1")));
    assert!(env
        .adapter
        .ready_to_publish(link.id)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        env.scalar("SELECT count(*) FROM confluence_publish_requests")
            .await,
        0
    );
    assert_eq!(
        env.scalar("SELECT count(*) FROM confluence_folder_pages")
            .await,
        2
    );
    let root: serde_json::Value = {
        let requests = env.mock.received_requests().await.unwrap();
        let first = requests
            .iter()
            .find(|r| r.method.as_str() == "POST" && r.url.path() == api("/api/v2/pages"))
            .unwrap();
        serde_json::from_slice(&first.body).unwrap()
    };
    assert_eq!(
        root["parentId"], PARENT,
        "the root page sits under the link's page"
    );
    assert!(env.actions().await.iter().all(|s| s == "confirmed"));
    env.mock.verify().await;
}

#[tokio::test]
async fn a_sync_with_nothing_changed_sends_nothing() {
    let env = Env::new().await;
    let link = env.link(ConfluenceLinkState::Active);
    env.version("rates.md", "# Rates", FileVersionOrigin::Person)
        .await;
    let hash = env.hash_of("rates.md").await;
    env.page_copy(&link, "rates.md", PAGE, "7", &hash).await;

    let report = env
        .adapter
        .sync_linked_folder(&link, &env.place)
        .await
        .unwrap();

    assert_eq!(
        report.pushed + report.waiting_for_approval + report.ready_to_publish,
        0
    );
    assert!(env.requests().await.is_empty());
}

#[test]
fn the_page_as_markdown_round_trips() {
    let storage = ConfluenceAdapter::markdown_to_storage("# Rates\n").unwrap();
    assert_eq!(
        ConfluenceAdapter::storage_to_markdown(&storage).unwrap(),
        "# Rates\n"
    );
}
