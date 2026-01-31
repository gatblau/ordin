//! X1: one test per row of ConfluenceAdapter.md's *Error Table*, provoking the condition and asserting the
//! code, the retryability and the repair action of the Response Body. The row's second cause,
//! `CRED_PROVIDER_NOT_CONFIGURED`, cannot come from a stored credential; it is the unit test
//! `provider_not_configured_is_not_configured` in `src/error.rs`.

mod support;

use ordin_confluence::{ConfluenceAdapter, ConfluenceError, ReadyItem, ReadyKind, RemoteItemRef};
use ordin_types::domain::places::{ConfluenceLinkState, CopyDestination};
use ordin_types::{CodedError, ConfluenceLinkId, PlaceId, RepairAction, SourceLinkId};
use serde_json::json;
use std::time::Duration;
use support::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

fn check(error: &ConfluenceError, code: &str, retryable: bool, repair: Option<RepairAction>) {
    assert_eq!(error.code(), code);
    assert_eq!(error.retryable(), retryable, "{code}");
    assert_eq!(CodedError::repair(error), repair, "{code}");
    assert_eq!(error.to_string(), code, "Display is the code only");
}

const PAGE: &str = "4411";

fn page() -> RemoteItemRef {
    RemoteItemRef::Page {
        page_id: PAGE.to_owned(),
    }
}

fn page_url() -> String {
    api(&format!("/api/v2/pages/{PAGE}"))
}

// Row: no `confluence.site` in the profile, or Identity answers `CRED_PROVIDER_NOT_CONFIGURED`.
#[tokio::test]
async fn not_configured() {
    let env = Env::with(Options {
        site: None,
        ..Options::default()
    })
    .await;
    let error = env.adapter.find_locations("rates", 10).await.unwrap_err();
    check(&error, "CONF_NOT_CONFIGURED", false, None);
    assert!(env.requests().await.is_empty());
}

// Row: no sign-in, refused, or a second 401.
#[tokio::test]
async fn sign_in_needed() {
    let env = Env::with_sign_in(SignIn::Nobody).await;
    let error = env.adapter.item_version(&page()).await.unwrap_err();
    check(
        &error,
        "CONF_SIGN_IN_NEEDED",
        false,
        Some(RepairAction::SignInAgain),
    );
    // A credential without the scopes is the same row (CRED_SCOPE_MISSING).
    let narrow = Env::with_sign_in(SignIn::WrongScopes).await;
    let error = narrow.adapter.item_version(&page()).await.unwrap_err();
    check(
        &error,
        "CONF_SIGN_IN_NEEDED",
        false,
        Some(RepairAction::SignInAgain),
    );
    // A 401 with no refresh token, and a 401 after a refresh, are the same row.
    let env = Env::new().await;
    status_only(&env, "GET", &page_url(), 401).await;
    let error = env.adapter.item_version(&page()).await.unwrap_err();
    check(
        &error,
        "CONF_SIGN_IN_NEEDED",
        false,
        Some(RepairAction::SignInAgain),
    );
    let env = Env::with_sign_in(SignIn::Refreshable).await;
    reply(
        &env,
        "POST",
        "/token",
        200,
        json!({"access_token": "at-2", "expires_in": 3600, "token_type": "bearer"}),
    )
    .await;
    status_only(&env, "GET", &page_url(), 401).await;
    let error = env.adapter.item_version(&page()).await.unwrap_err();
    check(
        &error,
        "CONF_SIGN_IN_NEEDED",
        false,
        Some(RepairAction::SignInAgain),
    );
    assert_eq!(env.count("GET", &page_url()).await, 2, "refreshed once");
}

// Row: signed-in account cannot reach the company site.
#[tokio::test]
async fn site_not_granted() {
    let env = Env::with(Options {
        site: Some("https://other.atlassian.net"),
        ..Options::default()
    })
    .await;
    let error = env.adapter.item_version(&page()).await.unwrap_err();
    check(
        &error,
        "CONF_SITE_NOT_GRANTED",
        false,
        Some(RepairAction::SignInAgain),
    );
}

// Row: 403 (C-11: every write method answers this when Confluence answers 403).
#[tokio::test]
async fn access_denied() {
    let env = Env::new().await;
    mount_page(&env, PAGE, "Rates", 7).await;
    status_only(&env, "PUT", &page_url(), 403).await;
    let error = env
        .adapter
        .update_page(PAGE, "Rates", "x", None, "7", key(), None)
        .await
        .unwrap_err();
    check(&error, "CONF_ACCESS_DENIED", false, None);
}

// Row: 404.
#[tokio::test]
async fn not_found() {
    let env = Env::new().await;
    status_only(&env, "GET", &page_url(), 404).await;
    let error = env.adapter.item_version(&page()).await.unwrap_err();
    check(&error, "CONF_NOT_FOUND", false, None);
}

// Row: stored origin form malformed.
#[tokio::test]
async fn location_invalid() {
    let env = Env::new().await;
    for origin in [
        "not an origin",
        "confluence:page/",
        "confluence:page/12 OR 1=1",
        "confluence:space/",
        "confluence:space/a b",
    ] {
        let error = env.adapter.resolve_origin(origin).await.unwrap_err();
        check(&error, "CONF_LOCATION_INVALID", false, None);
    }
    assert!(env.requests().await.is_empty());
}

// Row: search text empty or too long; more than 50 checked items.
#[tokio::test]
async fn query_invalid() {
    let env = Env::new().await;
    for query in ["", "   ", &"q".repeat(201)] {
        let error = env.adapter.find_locations(query, 10).await.unwrap_err();
        check(&error, "CONF_QUERY_INVALID", false, None);
    }
    let link = env.link(ConfluenceLinkState::Active);
    let items: Vec<ReadyItem> = (0..51)
        .map(|n| ReadyItem {
            confluence_link_id: link.id,
            kind: ReadyKind::New,
            path: format!("{n}.md"),
            from_path: None,
            published_copy_id: None,
        })
        .collect();
    let error = env
        .adapter
        .request_publish_checked(&link, &env.place, items)
        .await
        .unwrap_err();
    check(&error, "CONF_QUERY_INVALID", false, None);
    assert!(env.requests().await.is_empty());
}

// Row: page or attachment version differs from the one read, or 409.
#[tokio::test]
async fn origin_changed() {
    let env = Env::new().await;
    mount_page(&env, PAGE, "Rates", 8).await;
    let error = env
        .adapter
        .update_page(PAGE, "Rates", "x", None, "7", key(), None)
        .await
        .unwrap_err();
    check(&error, "CONF_ORIGIN_CHANGED", false, None);
    // The 409 form.
    status_only(&env, "PUT", &page_url(), 409).await;
    let error = env
        .adapter
        .update_page(PAGE, "Rates", "x", None, "8", key(), None)
        .await
        .unwrap_err();
    check(&error, "CONF_ORIGIN_CHANGED", false, None);
    // The attachment form.
    mount_attachments(&env, PAGE, vec![attachment_json("att1", "a.pdf", 5, 10)]).await;
    let error = env
        .adapter
        .attach(PAGE, "a.pdf", b"x".to_vec(), Some("4"), key(), None)
        .await
        .unwrap_err();
    check(&error, "CONF_ORIGIN_CHANGED", false, None);
}

// Row: a page with this title already exists in the space.
#[tokio::test]
async fn title_exists() {
    let env = Env::new().await;
    mount_space(&env).await;
    reply(
        &env,
        "POST",
        &api("/api/v2/pages"),
        400,
        json!({"message": "A page with this title already exists"}),
    )
    .await;
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
    check(&error, "CONF_TITLE_EXISTS", false, None);
}

// Row: body above 5 MiB, attachment above 100 MiB, or 413.
#[tokio::test]
async fn too_large() {
    let env = Env::new().await;
    let body = "x".repeat(5 * 1024 * 1024 + 1);
    let error = env
        .adapter
        .update_page(PAGE, "Rates", &body, None, "7", key(), None)
        .await
        .unwrap_err();
    check(&error, "CONF_TOO_LARGE", false, None);
    let bytes = vec![0u8; 100 * 1024 * 1024 + 1];
    let error = env
        .adapter
        .attach(PAGE, "a.pdf", bytes, None, key(), None)
        .await
        .unwrap_err();
    check(&error, "CONF_TOO_LARGE", false, None);
    assert!(env.requests().await.is_empty(), "nothing was sent");
    mount_page(&env, PAGE, "Rates", 7).await;
    status_only(&env, "PUT", &page_url(), 413).await;
    let error = env
        .adapter
        .update_page(PAGE, "Rates", "x", None, "7", key(), None)
        .await
        .unwrap_err();
    check(&error, "CONF_TOO_LARGE", false, None);
}

// Row: 429 after retries.
#[tokio::test]
async fn throttled() {
    let env = Env::new().await;
    status_only(&env, "GET", &page_url(), 429).await;
    let error = env.adapter.item_version(&page()).await.unwrap_err();
    check(&error, "CONF_THROTTLED", true, Some(RepairAction::TryAgain));
    assert_eq!(env.count("GET", &page_url()).await, 4, "three retries");
}

// Row: 5xx or network failure after retries.
#[tokio::test]
async fn unavailable() {
    for status in [500, 503] {
        let env = Env::new().await;
        status_only(&env, "GET", &page_url(), status).await;
        let error = env.adapter.item_version(&page()).await.unwrap_err();
        check(
            &error,
            "CONF_UNAVAILABLE",
            true,
            Some(RepairAction::TryAgain),
        );
        assert_eq!(env.count("GET", &page_url()).await, 4, "three retries");
    }
    // A network failure: every read times out.
    let env = Env::with_timeout(Duration::from_millis(50)).await;
    Mock::given(method("GET"))
        .and(path(page_url()))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(2)))
        .mount(&env.mock)
        .await;
    let error = env.adapter.item_version(&page()).await.unwrap_err();
    check(
        &error,
        "CONF_UNAVAILABLE",
        true,
        Some(RepairAction::TryAgain),
    );
}

// Row: timeout after sending and the destination moved.
#[tokio::test]
async fn outcome_unknown() {
    let env = Env::with_timeout(Duration::from_millis(250)).await;
    reply_once(&env, "GET", &page_url(), 200, page_json(PAGE, "Rates", 7)).await;
    mount_page(&env, PAGE, "Rates", 8).await;
    Mock::given(method("PUT"))
        .and(path(page_url()))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(2)))
        .mount(&env.mock)
        .await;
    let error = env
        .adapter
        .update_page(PAGE, "Rates", "x", None, "7", key(), None)
        .await
        .unwrap_err();
    check(&error, "CONF_OUTCOME_UNKNOWN", false, None);
}

// Row: response body does not parse.
#[tokio::test]
async fn response_invalid() {
    let env = Env::new().await;
    Mock::given(method("GET"))
        .and(path(page_url()))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
        .mount(&env.mock)
        .await;
    let error = env.adapter.item_version(&page()).await.unwrap_err();
    check(&error, "CONF_RESPONSE_INVALID", false, None);
}

// Row: storage XHTML malformed (bring back, storage push).
#[tokio::test]
async fn conversion_failed() {
    let error = ConfluenceAdapter::storage_to_markdown("<p>unclosed").unwrap_err();
    check(&error, "CONF_CONVERSION_FAILED", false, None);
    let env = Env::new().await;
    let error = env
        .adapter
        .update_page_storage(PAGE, "<p>unclosed", "7", key(), None)
        .await
        .unwrap_err();
    check(&error, "CONF_CONVERSION_FAILED", false, None);
    assert!(env.requests().await.is_empty());
}

// Row: link not in the state the call needs.
#[tokio::test]
async fn link_not_active() {
    let env = Env::new().await;
    for state in [ConfluenceLinkState::Paused, ConfluenceLinkState::Proposed] {
        let error = env
            .adapter
            .sync_linked_folder(&env.link(state), &env.place)
            .await
            .unwrap_err();
        check(&error, "CONF_LINK_NOT_ACTIVE", false, None);
    }
    let error = env
        .adapter
        .request_link_approval(
            &env.link(ConfluenceLinkState::Active),
            &env.place,
            &env.workspace,
        )
        .await
        .unwrap_err();
    check(&error, "CONF_LINK_NOT_ACTIVE", false, None);
}

// Row: link has no tracking state (`check_changes`, `item_for_path`).
#[tokio::test]
async fn not_tracked() {
    let env = Env::new().await;
    let error = env
        .adapter
        .item_for_path(SourceLinkId::new(), "a.confluence.xhtml")
        .await
        .unwrap_err();
    check(&error, "CONF_NOT_TRACKED", false, None);
    let link = env.source_link("confluence:space/PROC");
    let error = env.adapter.check_changes(&link).await.unwrap_err();
    check(&error, "CONF_NOT_TRACKED", false, None);
}

// Row: external-action key already used.
#[tokio::test]
async fn action_key_used() {
    let env = Env::new().await;
    mount_page(&env, PAGE, "Rates", 7).await;
    mount_put(&env, PAGE, "Rates", 8).await;
    let used = key();
    env.adapter
        .update_page(PAGE, "Rates", "x", None, "7", used, None)
        .await
        .unwrap();
    let error = env
        .adapter
        .update_page(PAGE, "Rates", "x", None, "7", used, None)
        .await
        .unwrap_err();
    check(&error, "CONF_ACTION_KEY_USED", false, None);
    assert_eq!(env.count("PUT", &page_url()).await, 1, "nothing sent");
}

// Row: Approval Service refused the request (an empty selection breaks the card's limits).
#[tokio::test]
async fn approval_failed() {
    let env = Env::new().await;
    mount_where(&env).await;
    let link = env.link(ConfluenceLinkState::Active);
    let error = env
        .adapter
        .request_publish_checked(&link, &env.place, Vec::new())
        .await
        .unwrap_err();
    check(
        &error,
        "CONF_APPROVAL_FAILED",
        true,
        Some(RepairAction::TryAgain),
    );
}

// Row: File Service call failed during sync.
#[tokio::test]
async fn files_failed() {
    let env = Env::new().await;
    let mut link = env.link(ConfluenceLinkState::Active);
    let mut place = env.place.clone();
    link.place_id = PlaceId::new();
    place.id = link.place_id;
    let error = env
        .adapter
        .sync_linked_folder(&link, &place)
        .await
        .unwrap_err();
    check(
        &error,
        "CONF_FILES_FAILED",
        true,
        Some(RepairAction::TryAgain),
    );
}

// Row: own tables unreachable, or the Journal refused to record the external action for a reason other
// than a used key.
#[tokio::test]
async fn datastore_unavailable() {
    let env = Env::new().await;
    mount_page(&env, PAGE, "Rates", 7).await;
    env.pool.close().await;
    let error = env
        .adapter
        .update_page(PAGE, "Rates", "x", None, "7", key(), None)
        .await
        .unwrap_err();
    check(
        &error,
        "CONF_DATASTORE_UNAVAILABLE",
        true,
        Some(RepairAction::TryAgain),
    );
    assert_eq!(env.count("PUT", &page_url()).await, 0, "nothing sent");
    let error = env
        .adapter
        .ready_to_publish(ConfluenceLinkId::new())
        .await
        .unwrap_err();
    check(
        &error,
        "CONF_DATASTORE_UNAVAILABLE",
        true,
        Some(RepairAction::TryAgain),
    );
}

// Not a row of its own: the paging cursor (Internal Logic 1.4).
#[tokio::test]
async fn bad_cursor() {
    let env = Env::new().await;
    let loc = ordin_confluence::ConfluenceLocation {
        space_key: "PROC".to_owned(),
        space_name: "Procurement".to_owned(),
        page_id: Some(PAGE.to_owned()),
        title: "Rates".to_owned(),
        web_url: String::new(),
    };
    let elsewhere = ordin_types::Cursor::encode(&"https://elsewhere.example/x".to_owned());
    let error = env
        .adapter
        .list_children(&loc, Some(elsewhere))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "SRV_BAD_CURSOR");
    assert_eq!(CodedError::repair(&error), Some(RepairAction::TryAgain));
    assert!(env.requests().await.is_empty());
}
