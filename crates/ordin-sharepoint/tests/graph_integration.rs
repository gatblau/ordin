//! Integration of the adapter with the real Identity service, the real Journal on a temporary SQLite file
//! and the Microsoft Graph stand-in (plan, *HTTP stand-ins*): the versioned write, `SP_TOO_LARGE`, the 429
//! back-off, and the paths of Internal Logic 1 and 6 that the scenarios do not reach.
//!
//! The stand-in's replies take the shapes of SharePointAdapter.md (*Microsoft Graph surface used*,
//! *Example*): `cTag` and `eTag` strings, `@odata.nextLink`, `uploadUrl`, `Retry-After`.

mod support;

use std::time::{Duration, Instant};

use ordin_journal::EntryFilter;
use ordin_sharepoint::SharePointError;
use ordin_types::domain::runs::ExternalActionKind;
use ordin_types::CodedError;
use serde_json::json;
use support::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn item_path() -> String {
    api(&format!("/drives/{DRIVE}/items/{FILE}"))
}

/// The reads of a versioned write at `ctag`, then the same item at `after` once the PUT has landed.
async fn mount_versioned_write(env: &Env, ctag: &str, after: &str) {
    get_json_once(
        env,
        &item_path(),
        200,
        file_json(
            FILE,
            "Kestrel MSA.docx",
            ctag,
            "/Suppliers/Contracts",
            "Sam Lee",
        ),
    )
    .await;
    get_json(
        env,
        &item_path(),
        200,
        file_json(
            FILE,
            "Kestrel MSA.docx",
            after,
            "/Suppliers/Contracts",
            "Sam Lee",
        ),
    )
    .await;
    mount_labels_and_site(env).await;
}

#[tokio::test]
async fn a_versioned_write_completes_and_is_journalled_with_ids_only() {
    let env = Env::new().await;
    mount_versioned_write(&env, "c4", "c5").await;
    Mock::given(method("PUT"))
        .and(path(format!("{}/content", item_path()).as_str()))
        .and(IfMatch(etag_of("c4")))
        .respond_with(ResponseTemplate::new(200).set_body_json(file_json(
            FILE,
            "Kestrel MSA.docx",
            "c5",
            "/Suppliers/Contracts",
            "Sam Lee",
        )))
        .expect(1)
        .mount(&env.mock)
        .await;
    let action = write(ExternalActionKind::OriginPush);
    let key = action.key;
    let version = env
        .adapter
        .upload_version(
            &file_ref(),
            b"Confidential contract text".to_vec(),
            "c4",
            action,
        )
        .await
        .unwrap();
    assert_eq!(version.tag, "c5");

    // The external action is confirmed, and the Journal entry holds the kind and the label only.
    let confirmed = env
        .journal
        .external_action(key)
        .await
        .unwrap()
        .expect("the action");
    assert_eq!(
        confirmed.state,
        ordin_types::domain::runs::ExternalActionState::Confirmed
    );
    let entries = env
        .journal
        .list_entries(
            EntryFilter {
                run_id: None,
                kind: Some("sharepoint_pushed".to_owned()),
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(entries.items.len(), 1);
    let entry = &entries.items[0];
    assert_eq!(entry.component, "sharepoint");
    assert_eq!(
        entry.data,
        json!({"kind": "origin_push", "version_label": "4.0"})
    );
    let stored = format!("{} {}", entry.summary, entry.data);
    for secret in ["Kestrel", "Confidential", TOKEN, "Sam Lee", "docx"] {
        assert!(!stored.contains(secret), "{secret} is in the entry");
    }
}

#[tokio::test]
async fn an_oversize_write_is_refused_as_too_large_before_anything_is_sent() {
    let env = Env::new().await;
    let bytes = vec![0u8; 250 * 1024 * 1024 + 1];
    let error = env
        .adapter
        .upload_version(
            &file_ref(),
            bytes,
            "c4",
            write(ExternalActionKind::OriginPush),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "SP_TOO_LARGE");
    assert!(env.requests().await.is_empty());
    assert_eq!(env.rows("external_actions").await, 0);

    // A create is refused the same way.
    let bytes = vec![0u8; 250 * 1024 * 1024 + 1];
    let error = env
        .adapter
        .create_file(
            &contracts_folder(),
            "big.bin",
            bytes,
            write(ExternalActionKind::CopyPublish),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "SP_TOO_LARGE");
    assert!(env.requests().await.is_empty());
}

#[tokio::test]
async fn a_429_without_retry_after_is_retried_with_back_off_and_jitter() {
    let env = Env::with_scale(SignIn::Token, 0.01).await;
    // Two throttled answers, then success: 1 s and 2 s of back-off, scaled by 0.01, each within +-20 %.
    Mock::given(method("GET"))
        .and(path(item_path().as_str()))
        .respond_with(ResponseTemplate::new(429))
        .up_to_n_times(2)
        .mount(&env.mock)
        .await;
    get_json(
        &env,
        &item_path(),
        200,
        file_json(
            FILE,
            "Kestrel MSA.docx",
            "c4",
            "/Suppliers/Contracts",
            "Sam Lee",
        ),
    )
    .await;
    get_json(
        &env,
        &format!("{}/versions", item_path()),
        200,
        json!({"value": [{"id": "4.0"}]}),
    )
    .await;
    let started = Instant::now();
    let version = env.adapter.item_version(&file_ref()).await.unwrap();
    let waited = started.elapsed();
    assert_eq!(version.tag, "c4");
    assert_eq!(env.count("GET", &item_path()).await, 3);
    // 10 ms * 0.8 + 20 ms * 0.8 = 24 ms at the least; the whole call is well under the unscaled 3 s.
    assert!(waited >= Duration::from_millis(24), "{waited:?}");
    assert!(waited < Duration::from_secs(2), "{waited:?}");
}

#[tokio::test]
async fn retry_after_wins_over_the_back_off_step() {
    let env = Env::with_scale(SignIn::Token, 0.05).await;
    Mock::given(method("GET"))
        .and(path(item_path().as_str()))
        .respond_with(ResponseTemplate::new(503).insert_header("Retry-After", "3"))
        .up_to_n_times(1)
        .mount(&env.mock)
        .await;
    get_json(
        &env,
        &item_path(),
        200,
        file_json(FILE, "a", "c1", "/", "Sam Lee"),
    )
    .await;
    get_json(
        &env,
        &format!("{}/versions", item_path()),
        200,
        json!({"value": []}),
    )
    .await;
    let started = Instant::now();
    let version = env.adapter.item_version(&file_ref()).await.unwrap();
    assert_eq!(version.label, None);
    // 3 s * 0.05 = 150 ms; the 1 s back-off step would have been 50 ms.
    assert!(
        started.elapsed() >= Duration::from_millis(145),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn a_401_refreshes_silently_once_and_repeats_the_request() {
    let env = Env::with(SignIn::Refreshable).await;
    get_with_token(&env, &item_path(), TOKEN, 401, json!({})).await;
    get_with_token(
        &env,
        &item_path(),
        "at-refreshed-0002",
        200,
        file_json(
            FILE,
            "Kestrel MSA.docx",
            "c4",
            "/Suppliers/Contracts",
            "Sam Lee",
        ),
    )
    .await;
    Mock::given(method("GET"))
        .and(path(format!("{}/versions", item_path()).as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [{"id": "4.0"}]})))
        .mount(&env.mock)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "at-refreshed-0002", "refresh_token": "rt-rotated-0002",
            "expires_in": 3600, "token_type": "Bearer",
        })))
        .expect(1)
        .mount(&env.mock)
        .await;
    let version = env.adapter.item_version(&file_ref()).await.unwrap();
    assert_eq!(version.tag, "c4");

    // The refresh is refused the second time round: the person must sign in again.
    let env = Env::with(SignIn::Refreshable).await;
    status_only(&env, "GET", &item_path(), 401).await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": "invalid_grant"})))
        .mount(&env.mock)
        .await;
    let error = env.adapter.item_version(&file_ref()).await.unwrap_err();
    assert_eq!(error.code(), "SP_SIGN_IN_NEEDED");
}

#[tokio::test]
async fn a_large_write_goes_by_an_upload_session_in_ranges_without_the_token() {
    let env = Env::with_timeout(SignIn::Token, Duration::from_secs(20)).await;
    mount_versioned_write(&env, "c4", "c5").await;
    let upload_url = format!("{}/upload/session-1", env.mock.uri());
    Mock::given(method("POST"))
        .and(path(
            format!("{}/createUploadSession", item_path()).as_str(),
        ))
        .and(IfMatch(etag_of("c4")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"uploadUrl": upload_url})))
        .expect(1)
        .mount(&env.mock)
        .await;
    let total = 10 * 1024 * 1024 + 5;
    Mock::given(method("PUT"))
        .and(path("/upload/session-1"))
        .and(|r: &wiremock::Request| {
            r.headers
                .get("content-range")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.starts_with("bytes 0-10485759/"))
        })
        .respond_with(
            ResponseTemplate::new(202).set_body_json(json!({"nextExpectedRanges": ["10485760-"]})),
        )
        .mount(&env.mock)
        .await;
    Mock::given(method("PUT"))
        .and(path("/upload/session-1"))
        .and(|r: &wiremock::Request| {
            r.headers
                .get("content-range")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v == "bytes 10485760-10485764/10485765")
        })
        .respond_with(ResponseTemplate::new(200).set_body_json(file_json(
            FILE,
            "Kestrel MSA.docx",
            "c5",
            "/Suppliers/Contracts",
            "Sam Lee",
        )))
        .mount(&env.mock)
        .await;
    let version = env
        .adapter
        .upload_version(
            &file_ref(),
            vec![7u8; total],
            "c4",
            write(ExternalActionKind::OriginPush),
        )
        .await
        .unwrap();
    assert_eq!(version.tag, "c5");
    let ranges: Vec<_> = env
        .mock
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path() == "/upload/session-1")
        .collect();
    assert_eq!(ranges.len(), 2, "two ranges of at most 10 MiB");
    assert!(
        ranges
            .iter()
            .all(|r| !r.headers.contains_key("authorization")),
        "the token is not sent to the session address"
    );
    assert_eq!(ranges[0].body.len(), 10 * 1024 * 1024);
    assert_eq!(ranges[1].body.len(), 5);
}

#[tokio::test]
async fn a_new_large_file_goes_by_an_upload_session_with_conflict_behaviour_fail() {
    let env = Env::with_timeout(SignIn::Token, Duration::from_secs(20)).await;
    let upload_url = format!("{}/upload/session-2", env.mock.uri());
    Mock::given(method("POST"))
        .and(path(
            "/v1.0/drives/b!x9Fq/items/01RATES:/big.bin:/createUploadSession",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"uploadUrl": upload_url})))
        .expect(1)
        .mount(&env.mock)
        .await;
    Mock::given(method("PUT"))
        .and(path("/upload/session-2"))
        .respond_with(
            ResponseTemplate::new(201)
                .set_body_json(file_json("01BIG", "big.bin", "c1", "/Rates", "Sam Lee")),
        )
        .expect(1)
        .mount(&env.mock)
        .await;
    let folder = ordin_sharepoint::SharePointLocation {
        item_id: Some("01RATES".to_owned()),
        path: "Rates".to_owned(),
        ..contracts_folder()
    };
    let (item, _) = env
        .adapter
        .create_file(
            &folder,
            "big.bin",
            vec![1u8; 4 * 1024 * 1024 + 1],
            write(ExternalActionKind::CopyPublish),
        )
        .await
        .unwrap();
    assert_eq!(item.item_id, "01BIG");
    let session = env
        .mock
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.url.path().ends_with("createUploadSession"))
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&session.body).unwrap();
    assert_eq!(
        body,
        json!({"item": {"@microsoft.graph.conflictBehavior": "fail"}})
    );
}

#[tokio::test]
async fn a_fetch_follows_the_download_redirect_without_sending_the_token() {
    let env = Env::new().await;
    let cdn = MockServer::start().await;
    get_json(
        &env,
        &item_path(),
        200,
        file_json(
            FILE,
            "Kestrel MSA.docx",
            "c4",
            "/Suppliers/Contracts",
            "Sam Lee",
        ),
    )
    .await;
    get_json(
        &env,
        &format!("{}/versions", item_path()),
        200,
        json!({"value": [{"id": "4.0"}]}),
    )
    .await;
    Mock::given(method("GET"))
        .and(path(format!("{}/content", item_path()).as_str()))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", format!("{}/download/abc", cdn.uri()).as_str()),
        )
        .mount(&env.mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/download/abc"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"the contract".to_vec()))
        .expect(1)
        .mount(&cdn)
        .await;
    let fetched = env.adapter.fetch(&file_ref()).await.unwrap();
    assert_eq!(fetched.bytes, b"the contract");
    assert_eq!(fetched.version.tag, "c4");
    assert_eq!(fetched.version.label.as_deref(), Some("4.0"));
    let download = &cdn.received_requests().await.unwrap()[0];
    assert!(
        !download.headers.contains_key("authorization"),
        "the token stays with Graph"
    );
}

#[tokio::test]
async fn a_file_that_changes_during_the_download_is_read_again_once_then_refused() {
    let env = Env::new().await;
    // Reads alternate c1, c2, c3, c4: no pair of reads agrees.
    for ctag in ["c1", "c2", "c3"] {
        get_json_once(
            &env,
            &item_path(),
            200,
            file_json(FILE, "a.docx", ctag, "/", "Sam Lee"),
        )
        .await;
    }
    get_json(
        &env,
        &item_path(),
        200,
        file_json(FILE, "a.docx", "c4", "/", "Sam Lee"),
    )
    .await;
    status_only(&env, "GET", &format!("{}/content", item_path()), 200).await;
    let error = env.adapter.fetch(&file_ref()).await.unwrap_err();
    match error {
        SharePointError::OriginChanged { current } => assert_eq!(current.tag, "c4"),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        env.count("GET", &format!("{}/content", item_path())).await,
        2,
        "read again once"
    );
}
