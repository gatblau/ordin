//! The transport rules of Internal Logic 1 and 7, and the reads and writes no Gherkin scenario
//! reaches: 401 handling, OAuth refresh, the gateway cache, pagination and order, downloads, and
//! the retry rules for writes.

use std::time::Duration;

use ordin_identity::{CredError, SecretString};
use ordin_types::domain::runs::ExternalActionState;
use ordin_types::ExternalActionKey;
use serde_json::json;
use sha2::{Digest, Sha256};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

use super::harness::{body_json, issue_ref, project_ref, Fixture, Mode};
use super::support::fixtures::*;
use crate::client::{map_cred, redirect_allowed};
use crate::{JiraAdapter, JiraCommentBody, JiraError, JiraQuery};

fn api(tail: &str) -> String {
    format!("/rest/api/3{tail}")
}

fn gateway(tail: &str) -> String {
    format!("/ex/jira/c1/rest/api/3{tail}")
}

async fn grant(f: &Fixture) {
    f.reply(
        "GET",
        "/oauth/token/accessible-resources",
        200,
        json!([{"id": "c1", "url": "https://acme.atlassian.net", "scopes": ["read:jira-work", "write:jira-work"]}]),
    )
    .await;
}

#[test]
fn a_credential_error_maps_by_its_code() {
    let permanent = |code| CredError::Permanent { code };
    for code in [
        "CRED_SIGN_IN_REQUIRED",
        "CRED_SCOPE_MISSING",
        "CRED_PROVIDER_REFUSED",
        "CRED_KEYCHAIN_MISSING",
        "CRED_NOT_FOUND",
    ] {
        assert_eq!(
            map_cred(&permanent(code)),
            JiraError::SignInNeeded,
            "{code}"
        );
    }
    assert_eq!(
        map_cred(&CredError::Retryable {
            code: "CRED_STORE_FAILED"
        }),
        JiraError::Unavailable
    );
}

#[test]
fn only_atlassian_hosts_are_followed_on_a_redirect() {
    let allowed = |u: &str| redirect_allowed(&url::Url::parse(u).unwrap());
    assert!(allowed("https://acme.atlassian.net/secure/attachment/1"));
    assert!(allowed("https://api.atlassian.com/ex/jira/c1/x"));
    assert!(allowed("https://api.media.atlassian.com/file/1"));
    assert!(!allowed("http://acme.atlassian.net/x"));
    assert!(!allowed("https://evil.example/x"));
    assert!(!allowed("https://evilatlassian.net/x"));
    assert!(!allowed("https://acme.atlassian.net.evil.example/x"));
    assert!(!allowed("https://acme.atlassian.net:8443/x"));
    assert!(!allowed("https://user@acme.atlassian.net/x"));
}

#[tokio::test]
async fn an_oauth_401_refreshes_once_and_repeats() {
    let f = Fixture::with(Mode::Oauth, None).await;
    grant(&f).await;
    Mock::given(method("GET"))
        .and(path(gateway("/myself")))
        .respond_with(ResponseTemplate::new(401))
        .up_to_n_times(1)
        .mount(&f.mock)
        .await;
    f.reply("GET", &gateway("/myself"), 200, myself()).await;
    let person = f.adapter.myself(SITE).await.unwrap();
    assert_eq!(person.display_name, "Priya Shah");
    assert_eq!(person.email.as_deref(), Some(EMAIL));
    assert_eq!(
        f.creds.refreshes.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    assert!(f.creds.refused.lock().unwrap().is_empty());
    // The refresh cleared the gateway cache, so the resource was asked for again.
    assert_eq!(
        f.received("GET", "/oauth/token/accessible-resources")
            .await
            .len(),
        2
    );
}

#[tokio::test]
async fn an_oauth_second_401_reports_the_credential_refused() {
    let f = Fixture::with(Mode::Oauth, None).await;
    grant(&f).await;
    f.status("GET", &gateway("/myself"), 401).await;
    assert_eq!(f.adapter.myself(SITE).await, Err(JiraError::SignInNeeded));
    assert_eq!(
        f.creds.refreshes.load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    let refused = f.creds.refused.lock().unwrap().clone();
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].0, f.creds.credential_id);
    assert_eq!(refused[0].1, "JIRA_TOKEN_REFUSED");
}

#[tokio::test]
async fn the_gateway_resource_is_looked_up_once_per_credential() {
    let f = Fixture::with(Mode::Oauth, None).await;
    grant(&f).await;
    f.reply("GET", &gateway("/myself"), 200, myself()).await;
    f.adapter.myself(SITE).await.unwrap();
    f.adapter.myself(SITE).await.unwrap();
    assert_eq!(
        f.received("GET", "/oauth/token/accessible-resources")
            .await
            .len(),
        1
    );
}

#[tokio::test]
async fn a_resource_without_the_read_scope_is_not_chosen() {
    let f = Fixture::with(Mode::Oauth, None).await;
    f.reply(
        "GET",
        "/oauth/token/accessible-resources",
        200,
        json!([{"id": "c1", "url": "https://acme.atlassian.net", "scopes": ["write:jira-work"]}]),
    )
    .await;
    assert_eq!(f.adapter.myself(SITE).await, Err(JiraError::SignInNeeded));
    f.status("GET", "/oauth/token/accessible-resources", 401)
        .await;
}

#[tokio::test]
async fn accessible_resources_that_refuse_or_are_garbled_are_reported() {
    let f = Fixture::with(Mode::Oauth, None).await;
    f.status("GET", "/oauth/token/accessible-resources", 401)
        .await;
    assert_eq!(f.adapter.myself(SITE).await, Err(JiraError::SignInNeeded));
    let f = Fixture::with(Mode::Oauth, None).await;
    f.reply(
        "GET",
        "/oauth/token/accessible-resources",
        200,
        json!({"not": "a list"}),
    )
    .await;
    assert_eq!(
        f.adapter.myself(SITE).await,
        Err(JiraError::ResponseInvalid)
    );
}

#[tokio::test]
async fn every_request_asks_for_json_with_the_credential() {
    let f = Fixture::new().await;
    f.reply("GET", &api("/myself"), 200, myself()).await;
    f.adapter.myself(SITE).await.unwrap();
    let sent = &f.received("GET", &api("/myself")).await[0];
    assert_eq!(
        sent.headers.get("accept").unwrap().to_str().unwrap(),
        "application/json"
    );
    assert!(sent
        .headers
        .get("authorization")
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("Basic "));
}

#[tokio::test]
async fn a_failed_save_after_a_good_check_maps_by_retryability() {
    let f = Fixture::with(Mode::NoCredential, None).await;
    f.reply("GET", &api("/myself"), 200, myself()).await;
    *f.creds.save_error.lock().unwrap() = Some(CredError::Permanent {
        code: "CRED_DUPLICATE",
    });
    let token = || SecretString::new(TOKEN.to_owned());
    assert_eq!(
        f.adapter.connect_with_token(SITE, EMAIL, token()).await,
        Err(JiraError::SignInNeeded)
    );
    *f.creds.save_error.lock().unwrap() = Some(CredError::Retryable {
        code: "CRED_STORE_FAILED",
    });
    assert_eq!(
        f.adapter.connect_with_token(SITE, EMAIL, token()).await,
        Err(JiraError::Unavailable)
    );
}

#[tokio::test]
async fn project_myself_and_assignable_reads_return_their_values() {
    let f = Fixture::new().await;
    f.reply(
        "GET",
        &api("/project/ABC"),
        200,
        json!({"id": "10000", "key": "ABC", "name": "Accounts"}),
    )
    .await;
    let project = f.adapter.project(&project_ref("ABC")).await.unwrap();
    assert_eq!(
        (project.id.as_str(), project.key.as_str()),
        ("10000", "ABC")
    );
    Mock::given(method("GET"))
        .and(path(api("/user/assignable/search")))
        .and(query_param("accountId", ACCOUNT_SAM))
        .and(query_param("maxResults", "1"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!([user(ACCOUNT_SAM, "Sam Lee")])),
        )
        .mount(&f.mock)
        .await;
    assert!(f
        .adapter
        .is_assignable(&project_ref("ABC"), ACCOUNT_SAM)
        .await
        .unwrap());
    Mock::given(method("GET"))
        .and(path(api("/user/assignable/search")))
        .and(query_param("accountId", "someone-else"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&f.mock)
        .await;
    assert!(!f
        .adapter
        .is_assignable(&project_ref("ABC"), "someone-else")
        .await
        .unwrap());
}

#[tokio::test]
async fn a_search_text_that_is_an_issue_key_reads_that_issue() {
    let f = Fixture::new().await;
    f.reply(
        "GET",
        &api("/issue/ABC-12"),
        200,
        issue("10012", "ABC-12", "Northwind assessment"),
    )
    .await;
    let found = f
        .adapter
        .search(SITE, &JiraQuery::Text("abc-12".into()), 5)
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].issue.key, "ABC-12");
    assert!(f.received("POST", &api("/search/jql")).await.is_empty());
}

#[tokio::test]
async fn comments_are_paged_and_ordered_by_created_then_numeric_id() {
    let f = Fixture::new().await;
    let at = |start: &str| {
        Mock::given(method("GET"))
            .and(path(api("/issue/ABC-12/comment")))
            .and(query_param("startAt", start))
            .and(query_param("maxResults", "100"))
            .and(query_param("orderBy", "created"))
    };
    let first: Vec<_> = (0..100)
        .map(|n| {
            comment(
                &format!("{}", 1000 + n),
                "2026-01-10T09:00:00.000+0000",
                "early",
            )
        })
        .collect();
    at("0")
        .respond_with(ResponseTemplate::new(200).set_body_json(comments_page(0, 102, first)))
        .mount(&f.mock)
        .await;
    // Ids 9 and 10 share a time: numeric order puts 9 first, text order would not. The earlier
    // time arrives last, as a reordering on Jira's side would deliver it.
    let second = vec![
        comment("10", "2026-01-12T09:00:00.000+0000", "ten"),
        comment("9", "2026-01-12T09:00:00.000+0000", "nine"),
    ];
    at("100")
        .respond_with(ResponseTemplate::new(200).set_body_json(comments_page(100, 102, second)))
        .mount(&f.mock)
        .await;
    let all = f.adapter.comments(&issue_ref("ABC-12")).await.unwrap();
    assert_eq!(all.len(), 102);
    let tail: Vec<&str> = all[100..].iter().map(|c| c.id.as_str()).collect();
    assert_eq!(tail, ["9", "10"]);
    assert_eq!(all[0].body_markdown, "early");
    assert_eq!(
        all[0].created,
        crate::parse::timestamp("2026-01-10T09:00:00.000Z").unwrap()
    );
}

#[tokio::test]
async fn an_attachment_is_streamed_hashed_and_renamed() {
    let f = Fixture::new().await;
    f.reply("GET", &api("/issue/ABC-12"), 200, attachment_issue(11))
        .await;
    Mock::given(method("GET"))
        .and(path(api("/attachment/content/10001")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"hello world".to_vec()))
        .mount(&f.mock)
        .await;
    let dest = f.dir.path().join("terms.pdf");
    let done = f
        .adapter
        .download_attachment(&issue_ref("ABC-12"), "10001", &dest)
        .await
        .unwrap();
    assert_eq!(done.bytes_written, 11);
    let expected: String = Sha256::digest(b"hello world")
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(done.sha256.0, expected);
    assert_eq!(std::fs::read(&dest).unwrap(), b"hello world");
    assert!(!f.dir.path().join("terms.pdf.part").exists());
    let sent = &f.received("GET", &api("/attachment/content/10001")).await[0];
    assert!(
        sent.headers.get("authorization").is_some(),
        "the first hop carries the credential"
    );
}

#[tokio::test]
async fn a_failed_download_leaves_no_part_file() {
    let f = Fixture::new().await;
    f.reply("GET", &api("/issue/ABC-12"), 200, attachment_issue(11))
        .await;
    f.status("GET", &api("/attachment/content/10001"), 403)
        .await;
    let dest = f.dir.path().join("terms.pdf");
    let result = f
        .adapter
        .download_attachment(&issue_ref("ABC-12"), "10001", &dest)
        .await;
    assert_eq!(result.err(), Some(JiraError::AccessDenied));
    assert!(!dest.exists() && !f.dir.path().join("terms.pdf.part").exists());
    // A destination that cannot be written is reported, and nothing is left behind.
    let f = Fixture::new().await;
    f.reply("GET", &api("/issue/ABC-13"), 200, attachment_issue(11))
        .await;
    let missing = f.dir.path().join("no-such-folder").join("x.bin");
    Mock::given(method("GET"))
        .and(path(api("/attachment/content/10001")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"hello world".to_vec()))
        .mount(&f.mock)
        .await;
    let result = f
        .adapter
        .download_attachment(&issue_ref("ABC-13"), "10001", &missing)
        .await;
    assert_eq!(result.err(), Some(JiraError::Unavailable));
}

#[tokio::test]
async fn a_server_error_on_a_read_is_retried_and_on_a_write_is_an_unknown_outcome() {
    let f = Fixture::new().await;
    Mock::given(method("POST"))
        .and(path(api("/issue/ABC-12/comment")))
        .respond_with(ResponseTemplate::new(500))
        .mount(&f.mock)
        .await;
    let key = ExternalActionKey::new();
    let body = JiraCommentBody {
        markdown: "x".into(),
        code_block: None,
    };
    assert_eq!(
        f.adapter
            .add_comment(&issue_ref("ABC-12"), &body, key)
            .await
            .err(),
        Some(JiraError::OutcomeUnknown)
    );
    assert_eq!(
        f.received("POST", &api("/issue/ABC-12/comment"))
            .await
            .len(),
        1
    );
    // An idempotent write is repeated once with the same key.
    Mock::given(method("PUT"))
        .and(path(api("/issue/ABC-12")))
        .respond_with(ResponseTemplate::new(500))
        .mount(&f.mock)
        .await;
    let key = ExternalActionKey::new();
    assert_eq!(
        f.adapter
            .add_label(&issue_ref("ABC-12"), "ordin-handover", key)
            .await
            .err(),
        Some(JiraError::OutcomeUnknown)
    );
    assert_eq!(f.received("PUT", &api("/issue")).await.len(), 0);
    assert_eq!(f.received("PUT", &api("/issue/ABC-12")).await.len(), 2);
    assert_eq!(f.action_state(key).await, Some(ExternalActionState::Sent));
}

#[tokio::test]
async fn add_label_sends_the_add_update_and_confirms() {
    let f = Fixture::new().await;
    f.status("PUT", &api("/issue/ABC-12"), 204).await;
    let key = ExternalActionKey::new();
    f.adapter
        .add_label(&issue_ref("ABC-12"), "ordin-handover", key)
        .await
        .unwrap();
    let body = body_json(&f.received("PUT", &api("/issue/ABC-12")).await[0]);
    assert_eq!(
        body,
        json!({"update": {"labels": [{"add": "ordin-handover"}]}})
    );
    assert_eq!(
        f.action_state(key).await,
        Some(ExternalActionState::Confirmed)
    );
}

#[tokio::test]
async fn a_503_on_a_write_is_retried_because_it_was_not_applied() {
    let f = Fixture::new().await;
    Mock::given(method("PUT"))
        .and(path(api("/issue/ABC-12/assignee")))
        .respond_with(ResponseTemplate::new(503).insert_header("Retry-After", "0"))
        .up_to_n_times(1)
        .mount(&f.mock)
        .await;
    f.status("PUT", &api("/issue/ABC-12/assignee"), 204).await;
    let key = ExternalActionKey::new();
    f.adapter
        .assign(&issue_ref("ABC-12"), ACCOUNT_SAM, key)
        .await
        .unwrap();
    assert_eq!(
        f.received("PUT", &api("/issue/ABC-12/assignee"))
            .await
            .len(),
        2
    );
    assert_eq!(
        f.action_state(key).await,
        Some(ExternalActionState::Confirmed)
    );
}

#[tokio::test]
async fn nothing_listening_is_unavailable_after_three_attempts() {
    // A port that was free a moment ago and now refuses connections.
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let f = Fixture::new().await;
    let adapter = JiraAdapter::from_parts(
        f.journal.clone(),
        f.creds.clone() as std::sync::Arc<dyn crate::client::Credentials>,
        reqwest::Client::new(),
        crate::JiraEndpoints {
            site_base_url: Some(format!("http://127.0.0.1:{port}")),
            gateway_base_url: None,
            request_timeout: Some(Duration::from_secs(2)),
        },
    );
    let started = std::time::Instant::now();
    assert_eq!(adapter.myself(SITE).await, Err(JiraError::Unavailable));
    // 1 s and 2 s back-off, each with at most 20 % jitter, between three attempts.
    assert!(started.elapsed() >= Duration::from_millis(2400));
}

#[test]
fn the_back_off_is_one_two_four_seconds_with_jitter_unless_retry_after_wins() {
    use crate::client::delay_for_tests as delay;
    for _ in 0..50 {
        let (lo, hi) = (|s: f64| s * 800.0, |s: f64| s * 1200.0);
        for (n, secs) in [(1u32, 1.0), (2, 2.0), (3, 4.0)] {
            let ms = delay(n, None).as_millis() as f64;
            assert!(ms >= lo(secs) && ms <= hi(secs), "retry {n}: {ms}");
        }
    }
    assert_eq!(delay(1, Some(5)), Duration::from_secs(5));
    assert_eq!(delay(1, Some(60)), Duration::from_secs(60));
    let over = delay(1, Some(61)).as_millis();
    assert!(
        (800..=1200).contains(&over),
        "a Retry-After above 60 s is ignored"
    );
}
