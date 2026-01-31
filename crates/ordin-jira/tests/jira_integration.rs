//! The Jira Adapter against a real Journal (SQLite), a real Identity Service and a `wiremock` Jira
//! Cloud stand-in on loopback (plan, Phase 2 and "HTTP stand-ins"). No real network is used.
//!
//! The stand-in's replies are those of the spec's own examples (`tests/support/fixtures.rs`). Its
//! contract is JiraAdapter.md, *Jira Cloud surface used*: the paths, bodies and status codes of that
//! table, and Jira's `Retry-After` on 429 and 503.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use ordin_identity::SecretString;
use ordin_jira::{
    JiraAdapter, JiraCommentBody, JiraDeps, JiraEndpoints, JiraError, JiraIssueRef, JiraQuery,
};
use ordin_types::domain::runs::{ExternalActionKind, ExternalActionState};
use ordin_types::ExternalActionKey;
use serde_json::json;
use support::fixtures::*;
use support::identity::identity_with_token;
use support::{open_journal, TempDir};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const API: &str = "/rest/api/3";

struct Rig {
    adapter: Arc<JiraAdapter>,
    mock: MockServer,
    journal: Arc<ordin_journal::Journal>,
    _dir: TempDir,
}

async fn rig(with_token: bool) -> Rig {
    let dir = TempDir::new();
    let journal = open_journal(&dir).await;
    let identity = if with_token {
        identity_with_token(&dir).await
    } else {
        support::identity::identity(&dir).await
    };
    let mock = MockServer::start().await;
    let adapter = JiraAdapter::with_endpoints(
        JiraDeps {
            journal: Arc::clone(&journal),
            identity,
            http: reqwest::Client::new(),
        },
        JiraEndpoints {
            site_base_url: Some(mock.uri()),
            gateway_base_url: Some(mock.uri()),
            request_timeout: Some(Duration::from_secs(5)),
        },
    );
    Rig {
        adapter,
        mock,
        journal,
        _dir: dir,
    }
}

async fn count(rig: &Rig, http_method: &str, at: &str) -> usize {
    rig.mock
        .received_requests()
        .await
        .unwrap_or_default()
        .iter()
        .filter(|r| r.method.as_str() == http_method && r.url.path() == at)
        .count()
}

fn text_search() -> JiraQuery {
    JiraQuery::Text("rates".into())
}

#[tokio::test]
async fn a_429_with_retry_after_is_retried_at_most_three_times() {
    let rig = rig(true).await;
    Mock::given(method("POST"))
        .and(path(format!("{API}/search/jql")))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "1"))
        .mount(&rig.mock)
        .await;
    let started = Instant::now();
    let result = rig.adapter.search(SITE, &text_search(), 10).await;
    assert_eq!(result, Err(JiraError::Throttled));
    assert_eq!(count(&rig, "POST", &format!("{API}/search/jql")).await, 3);
    // Two waits of the Retry-After, which wins over the 1 s, 2 s back-off.
    assert!(
        started.elapsed() >= Duration::from_millis(1900),
        "{:?}",
        started.elapsed()
    );
    assert!(
        started.elapsed() < Duration::from_millis(2900),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn a_503_with_retry_after_is_retried_at_most_three_times() {
    let rig = rig(true).await;
    Mock::given(method("GET"))
        .and(path(format!("{API}/project/ABC")))
        .respond_with(ResponseTemplate::new(503).insert_header("Retry-After", "1"))
        .mount(&rig.mock)
        .await;
    let result = rig
        .adapter
        .project(&ordin_jira::JiraProjectRef {
            site: SITE.into(),
            key: "ABC".into(),
        })
        .await;
    assert_eq!(result, Err(JiraError::Unavailable));
    assert_eq!(count(&rig, "GET", &format!("{API}/project/ABC")).await, 3);
}

#[tokio::test]
async fn a_429_then_a_good_answer_returns_the_answer() {
    let rig = rig(true).await;
    Mock::given(method("GET"))
        .and(path(format!("{API}/myself")))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "1"))
        .up_to_n_times(1)
        .mount(&rig.mock)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{API}/myself")))
        .respond_with(ResponseTemplate::new(200).set_body_json(myself()))
        .mount(&rig.mock)
        .await;
    let person = rig.adapter.myself(SITE).await.unwrap();
    assert_eq!(person.account_id, ACCOUNT_PRIYA);
    assert_eq!(count(&rig, "GET", &format!("{API}/myself")).await, 2);
}

#[tokio::test]
async fn a_pasted_token_is_kept_by_identity_and_then_used() {
    let rig = rig(false).await;
    assert_eq!(rig.adapter.myself(SITE).await, Err(JiraError::NotConnected));
    Mock::given(method("GET"))
        .and(path(format!("{API}/myself")))
        .respond_with(ResponseTemplate::new(200).set_body_json(myself()))
        .mount(&rig.mock)
        .await;
    let person = rig
        .adapter
        .connect_with_token(SITE, EMAIL, SecretString::new(TOKEN.to_owned()))
        .await
        .unwrap();
    assert_eq!(person.display_name, "Priya Shah");
    // The real Identity Service now holds it: the next call finds a credential and sends Basic auth.
    rig.adapter.myself(SITE).await.unwrap();
    let requests = rig.mock.received_requests().await.unwrap();
    let last = requests.last().unwrap();
    assert!(last
        .headers
        .get("authorization")
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("Basic "));
    // A different site is refused against the stored credential.
    assert_eq!(
        rig.adapter.myself("other.atlassian.net").await,
        Err(JiraError::SiteMismatch)
    );
}

/// Behavioural check (plan, Phase 2): an issue read completes against the stand-in and is
/// journalled with ids only.
#[tokio::test]
async fn an_issue_read_completes_and_is_journalled_with_ids_only() {
    let rig = rig(true).await;
    Mock::given(method("GET"))
        .and(path(format!("{API}/issue/ABC-12")))
        .respond_with(ResponseTemplate::new(200).set_body_json(abc12()))
        .mount(&rig.mock)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{API}/issue/ABC-12/comment")))
        .respond_with(ResponseTemplate::new(200).set_body_json(comments_page(
            0,
            1,
            vec![comment("10300", JIRA_TIME, "Confidential pricing remark")],
        )))
        .mount(&rig.mock)
        .await;
    Mock::given(method("PUT"))
        .and(path(format!("{API}/issue/ABC-12")))
        .respond_with(ResponseTemplate::new(204))
        .mount(&rig.mock)
        .await;
    let issue = JiraIssueRef {
        site: SITE.into(),
        key: "ABC-12".into(),
    };
    let content = rig.adapter.issue(&issue).await.unwrap();
    assert_eq!(content.summary.summary, "Northwind assessment");
    assert_eq!(content.comments.len(), 1);

    // The read leaves no issue text in the Journal; the write that follows leaves its ids only.
    let key = ExternalActionKey::new();
    rig.adapter
        .add_label(&issue, "ordin-handover", key)
        .await
        .unwrap();
    let page = rig.journal.list_external_actions(None, None).await.unwrap();
    assert_eq!(page.items.len(), 1);
    let action = &page.items[0];
    assert_eq!(action.action_key, key);
    assert_eq!(action.kind, ExternalActionKind::HandoverWritten);
    assert_eq!(action.state, ExternalActionState::Confirmed);
    assert_eq!(
        action.destination,
        "Jira \u{203a} acme.atlassian.net \u{203a} ABC-12"
    );
    let journalled = format!("{page:?}");
    for text in [
        "Northwind",
        "Confidential",
        "Assess the",
        TOKEN,
        EMAIL,
        "Sam Lee",
    ] {
        assert!(!journalled.contains(text), "the Journal holds {text:?}");
    }
}

#[tokio::test]
async fn a_comment_round_trips_through_the_journal_bracket() {
    let rig = rig(true).await;
    Mock::given(method("POST"))
        .and(path(format!("{API}/issue/ABC-12/comment")))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "20001", "author": user(ACCOUNT_PRIYA, "Priya Shah"),
            "created": JIRA_TIME, "updated": JIRA_TIME, "body": adf("hello")
        })))
        .mount(&rig.mock)
        .await;
    let key = ExternalActionKey::new();
    let created = rig
        .adapter
        .add_comment(
            &JiraIssueRef {
                site: SITE.into(),
                key: "ABC-12".into(),
            },
            &JiraCommentBody {
                markdown: "hello".into(),
                code_block: None,
            },
            key,
        )
        .await
        .unwrap();
    assert_eq!(created.body_markdown, "hello");
    let action = rig.journal.external_action(key).await.unwrap().unwrap();
    assert_eq!(action.state, ExternalActionState::Confirmed);
}
