//! Logging (Performance / Security / Observability; Logging.md): a log grep finds no token and no
//! issue text.
//!
//! The only test in its own binary (plan FU-017): `tracing` caches per call site whether anything
//! listens, so a subscriber installed while other tests run can miss a call site. Alone in the
//! process, the global subscriber is in place before the first line is logged.

mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_identity::SecretString;
use ordin_jira::{
    JiraAdapter, JiraCodeBlock, JiraCommentBody, JiraDeps, JiraEndpoints, JiraIssueRef, JiraQuery,
};
use ordin_types::ExternalActionKey;
use serde_json::json;
use support::fixtures::*;
use support::identity::identity;
use support::{open_journal, TempDir};
use tracing_subscriber::layer::SubscriberExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct Echo;

impl Respond for Echo {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
        ResponseTemplate::new(201).set_body_json(json!({
            "id": "20001", "author": user(ACCOUNT_PRIYA, "Priya Shah"),
            "created": JIRA_TIME, "updated": JIRA_TIME, "body": body["body"]
        }))
    }
}

const API: &str = "/rest/api/3";

#[tokio::test]
async fn no_log_line_holds_a_token_or_issue_text() {
    let buffer = Buffer::default();
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .event_format(ordin_types::log::JsonLineFormat)
            .with_writer(move || writer.clone()),
    );
    tracing::subscriber::set_global_default(subscriber).expect("the only subscriber");

    let dir = TempDir::new();
    let journal = open_journal(&dir).await;
    let mock = MockServer::start().await;
    let adapter = JiraAdapter::with_endpoints(
        JiraDeps {
            journal,
            identity: identity(&dir).await,
            http: reqwest::Client::new(),
        },
        JiraEndpoints {
            site_base_url: Some(mock.uri()),
            gateway_base_url: Some(mock.uri()),
            request_timeout: Some(Duration::from_secs(5)),
        },
    );
    let on = |m: &'static str, p: String, template: ResponseTemplate| {
        Mock::given(method(m)).and(path(p)).respond_with(template)
    };
    on(
        "GET",
        format!("{API}/myself"),
        ResponseTemplate::new(200).set_body_json(myself()),
    )
    .mount(&mock)
    .await;
    on(
        "GET",
        format!("{API}/issue/ABC-12"),
        ResponseTemplate::new(200).set_body_json(abc12()),
    )
    .mount(&mock)
    .await;
    on(
        "GET",
        format!("{API}/issue/ABC-12/comment"),
        ResponseTemplate::new(200).set_body_json(comments_page(
            0,
            1,
            vec![comment("10300", JIRA_TIME, "Confidential pricing remark")],
        )),
    )
    .mount(&mock)
    .await;
    Mock::given(method("POST"))
        .and(path(format!("{API}/issue/ABC-12/comment")))
        .respond_with(Echo)
        .mount(&mock)
        .await;
    on(
        "POST",
        format!("{API}/search/jql"),
        ResponseTemplate::new(429).insert_header("Retry-After", "0"),
    )
    .mount(&mock)
    .await;

    adapter
        .connect_with_token(SITE, EMAIL, SecretString::new(TOKEN.to_owned()))
        .await
        .unwrap();
    let issue = JiraIssueRef {
        site: SITE.into(),
        key: "ABC-12".into(),
    };
    adapter.issue(&issue).await.unwrap();
    adapter
        .add_comment(
            &issue,
            &JiraCommentBody {
                markdown: "Secret handover note about Kestrel".into(),
                code_block: Some(JiraCodeBlock {
                    language: Some("yaml".into()),
                    text: "kind: sent".into(),
                }),
            },
            ExternalActionKey::new(),
        )
        .await
        .unwrap();
    // A throttled search logs the back-off.
    let _ = adapter
        .search(SITE, &JiraQuery::Text("Kestrel pricing".into()), 10)
        .await;

    let text = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    // The events of the spec are there ...
    for event in [
        "jira_request",
        "jira_throttled",
        "jira_written",
        "jira_token_checked",
    ] {
        assert!(text.contains(event), "no {event} line in:\n{text}");
    }
    // ... and nothing of the person's data is.
    for secret in [
        TOKEN,
        EMAIL,
        "priya.shah",
        ACCOUNT_SAM,
        ACCOUNT_PRIYA,
        "Priya Shah",
        "Sam Lee",
        "Northwind",
        "Assess the",
        "Confidential",
        "Kestrel",
        "terms.pdf",
        "kind: sent",
    ] {
        assert!(
            !text.contains(secret),
            "a log line holds {secret:?}:\n{text}"
        );
    }
}
