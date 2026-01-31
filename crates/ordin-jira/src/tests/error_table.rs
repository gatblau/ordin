//! X1: one test per row of the Error Table, provoking the condition and asserting the code, the
//! retryability and the repair action of the *Response Body* column.

use std::time::Duration;

use ordin_identity::{CredError, SecretString};
use ordin_types::domain::runs::ExternalActionState;
use ordin_types::{CodedError as _, ExternalActionKey, RepairAction};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

use super::harness::{issue_ref, project_ref, Fixture, Mode};
use super::support::fixtures::*;
use crate::{JiraAdapter, JiraCommentBody, JiraError, JiraQuery};

fn api(tail: &str) -> String {
    format!("/rest/api/3{tail}")
}

fn assert_error<T: std::fmt::Debug>(
    result: Result<T, JiraError>,
    code: &str,
    retryable: bool,
    repair: Option<RepairAction>,
) {
    let error = result.expect_err(code);
    assert_eq!(error.code(), code);
    assert_eq!(error.retryable(), retryable, "{code}");
    assert_eq!(error.repair(), repair, "{code}");
}

fn invalid<T: std::fmt::Debug>(r: Result<T, JiraError>) {
    assert_error(r, "JIRA_RESPONSE_INVALID", false, None);
}

fn text_search() -> JiraQuery {
    JiraQuery::Text("rates".into())
}

fn comment_body() -> JiraCommentBody {
    JiraCommentBody {
        markdown: "hello".into(),
        code_block: None,
    }
}

async fn always(f: &Fixture, http_method: &str, at: &str, status: u16) {
    // `Retry-After: 0` keeps the three attempts quick; the header wins over the 1 s, 2 s back-off.
    Mock::given(method(http_method))
        .and(path(at))
        .respond_with(ResponseTemplate::new(status).insert_header("Retry-After", "0"))
        .mount(&f.mock)
        .await;
}

// Row: address not https, not an *.atlassian.net host, bad key or number, over 2048 characters
#[test]
fn address_invalid() {
    let long = format!(
        "https://acme.atlassian.net/browse/ABC-1?q={}",
        "x".repeat(2048)
    );
    for address in [
        "http://acme.atlassian.net/browse/ABC-1",
        "https://example.com/browse/ABC-1",
        "https://acme.atlassian.net/browse/abc-0",
        "https://acme.atlassian.net/browse/ABC-1x",
        long.as_str(),
    ] {
        assert_error(
            JiraAdapter::parse_issue_address(address),
            "JIRA_ADDRESS_INVALID",
            false,
            None,
        );
    }
    assert_error(
        JiraAdapter::parse_project_address("https://example.com/projects/ABC"),
        "JIRA_ADDRESS_INVALID",
        false,
        None,
    );
}

// Row: no Jira credential
#[tokio::test]
async fn not_connected() {
    let f = Fixture::with(Mode::NoCredential, None).await;
    assert_error(
        f.adapter.search(SITE, &text_search(), 10).await,
        "JIRA_NOT_CONNECTED",
        false,
        Some(RepairAction::SignInAgain),
    );
    assert_error(
        f.adapter.myself(SITE).await,
        "JIRA_NOT_CONNECTED",
        false,
        Some(RepairAction::SignInAgain),
    );
}

// Row: credential needs sign-in, scope missing, second 401, or OAuth site not granted
#[tokio::test]
async fn sign_in_needed() {
    for code in [
        "CRED_SIGN_IN_REQUIRED",
        "CRED_SCOPE_MISSING",
        "CRED_PROVIDER_REFUSED",
        "CRED_KEYCHAIN_MISSING",
        "CRED_CONFIG_INVALID",
    ] {
        let f = Fixture::with(Mode::Error(CredError::Permanent { code }), None).await;
        assert_error(
            f.adapter.myself(SITE).await,
            "JIRA_SIGN_IN_NEEDED",
            false,
            Some(RepairAction::SignInAgain),
        );
    }
    // A second 401: an API token is refused at once and the credential is marked.
    let f = Fixture::new().await;
    f.status("GET", &api("/myself"), 401).await;
    assert_error(
        f.adapter.myself(SITE).await,
        "JIRA_SIGN_IN_NEEDED",
        false,
        Some(RepairAction::SignInAgain),
    );
    let refused = f.creds.refused.lock().unwrap().clone();
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].1, "JIRA_TOKEN_REFUSED");
    // OAuth site not granted.
    let f = Fixture::with(Mode::Oauth, None).await;
    f.reply(
        "GET",
        "/oauth/token/accessible-resources",
        200,
        json!([{"id": "c9", "url": "https://other.atlassian.net", "scopes": ["read:jira-work"]}]),
    )
    .await;
    assert_error(
        f.adapter.myself(SITE).await,
        "JIRA_SIGN_IN_NEEDED",
        false,
        Some(RepairAction::SignInAgain),
    );
}

// Row: pasted token refused (401/403, blocked by an admin, not an Atlassian account), or email or
// token out of range
#[tokio::test]
async fn token_refused() {
    let token = || SecretString::new(TOKEN.to_owned());
    for status in [401, 403] {
        let f = Fixture::with(Mode::NoCredential, None).await;
        f.status("GET", &api("/myself"), status).await;
        assert_error(
            f.adapter.connect_with_token(SITE, EMAIL, token()).await,
            "JIRA_TOKEN_REFUSED",
            false,
            None,
        );
    }
    let f = Fixture::with(Mode::NoCredential, None).await;
    let mut app = myself();
    app["accountType"] = json!("app");
    f.reply("GET", &api("/myself"), 200, app).await;
    assert_error(
        f.adapter.connect_with_token(SITE, EMAIL, token()).await,
        "JIRA_TOKEN_REFUSED",
        false,
        None,
    );
    for (email, secret) in [
        ("a@", TOKEN.to_owned()),
        ("two@@acme.co.uk", TOKEN.to_owned()),
        (EMAIL, String::new()),
        (EMAIL, "t".repeat(1025)),
    ] {
        assert_error(
            f.adapter
                .connect_with_token(SITE, email, SecretString::new(secret))
                .await,
            "JIRA_TOKEN_REFUSED",
            false,
            None,
        );
    }
    assert!(f.creds.saved.lock().unwrap().is_empty());
    // A site that is not a Jira Cloud host is an address problem, and 404 is the same.
    assert_error(
        f.adapter
            .connect_with_token("example.com", EMAIL, token())
            .await,
        "JIRA_ADDRESS_INVALID",
        false,
        None,
    );
    let f = Fixture::with(Mode::NoCredential, None).await;
    f.status("GET", &api("/myself"), 404).await;
    assert_error(
        f.adapter.connect_with_token(SITE, EMAIL, token()).await,
        "JIRA_ADDRESS_INVALID",
        false,
        None,
    );
}

// Row: argument names a site other than the connected one
#[tokio::test]
async fn site_mismatch() {
    let f = Fixture::new().await;
    assert_error(
        f.adapter.myself("other.atlassian.net").await,
        "JIRA_SITE_MISMATCH",
        false,
        None,
    );
    let other = crate::JiraProjectRef {
        site: "other.atlassian.net".into(),
        key: "ABC".into(),
    };
    assert_error(
        f.adapter.project(&other).await,
        "JIRA_SITE_MISMATCH",
        false,
        None,
    );
    assert_eq!(f.total_requests().await, 0);
    // A write checks the site before it journals anything.
    let key = ExternalActionKey::new();
    let issue = crate::JiraIssueRef {
        site: "other.atlassian.net".into(),
        key: "ABC-1".into(),
    };
    assert_error(
        f.adapter.add_label(&issue, "ordin-handover", key).await,
        "JIRA_SITE_MISMATCH",
        false,
        None,
    );
    assert_eq!(f.action_state(key).await, None);
}

// Row: 403; assignment refused (400 on assign)
#[tokio::test]
async fn access_denied() {
    let f = Fixture::new().await;
    f.status("GET", &api("/issue/ABC-12"), 403).await;
    assert_error(
        f.adapter.issue(&issue_ref("ABC-12")).await,
        "JIRA_ACCESS_DENIED",
        false,
        None,
    );
    f.status("PUT", &api("/issue/ABC-12/assignee"), 400).await;
    assert_error(
        f.adapter
            .assign(&issue_ref("ABC-12"), ACCOUNT_SAM, ExternalActionKey::new())
            .await,
        "JIRA_ACCESS_DENIED",
        false,
        None,
    );
}

// Row: 404; attachment id not on the issue
#[tokio::test]
async fn not_found() {
    let f = Fixture::new().await;
    f.status("GET", &api("/project/ZZZ"), 404).await;
    assert_error(
        f.adapter.project(&project_ref("ZZZ")).await,
        "JIRA_NOT_FOUND",
        false,
        None,
    );
    f.reply("GET", &api("/issue/ABC-12"), 200, attachment_issue(10))
        .await;
    let dest = f.dir.path().join("x.bin");
    assert_error(
        f.adapter
            .download_attachment(&issue_ref("ABC-12"), "99999", &dest)
            .await,
        "JIRA_NOT_FOUND",
        false,
        None,
    );
    // A search text that is an issue key which does not exist is an empty list, not an error.
    f.status("GET", &api("/issue/ABC-99"), 404).await;
    let found = f
        .adapter
        .search(SITE, &JiraQuery::Text("abc-99".into()), 10)
        .await
        .unwrap();
    assert!(found.is_empty());
}

// Row: search text, label, key list, account id, summary or limit out of range; 400 on a built JQL
#[tokio::test]
async fn query_invalid() {
    let f = Fixture::new().await;
    let project = project_ref("ABC");
    assert_error(
        f.adapter
            .search(SITE, &JiraQuery::Text(" ".into()), 10)
            .await,
        "JIRA_QUERY_INVALID",
        false,
        None,
    );
    assert_error(
        f.adapter.find_assignable(&project, "", 5).await,
        "JIRA_QUERY_INVALID",
        false,
        None,
    );
    assert_error(
        f.adapter.find_assignable(&project, "sam", 21).await,
        "JIRA_QUERY_INVALID",
        false,
        None,
    );
    assert_error(
        f.adapter
            .find_assignable(&project, &"s".repeat(101), 5)
            .await,
        "JIRA_QUERY_INVALID",
        false,
        None,
    );
    assert_error(
        f.adapter.is_assignable(&project, "bad id!").await,
        "JIRA_QUERY_INVALID",
        false,
        None,
    );
    let key = ExternalActionKey::new();
    assert_error(
        f.adapter.assign(&issue_ref("ABC-12"), "", key).await,
        "JIRA_QUERY_INVALID",
        false,
        None,
    );
    assert_error(
        f.adapter
            .add_label(&issue_ref("ABC-12"), "Bad Label", key)
            .await,
        "JIRA_QUERY_INVALID",
        false,
        None,
    );
    for summary in ["", "   ", "two\nlines", &"s".repeat(256)] {
        assert_error(
            f.adapter.create_issue(&project, summary, key).await,
            "JIRA_QUERY_INVALID",
            false,
            None,
        );
    }
    assert_eq!(f.total_requests().await, 0);
    assert_eq!(f.action_state(key).await, None);
    // Jira's own 400 on a built JQL.
    f.status("POST", &api("/search/jql"), 400).await;
    assert_error(
        f.adapter.search(SITE, &text_search(), 10).await,
        "JIRA_QUERY_INVALID",
        false,
        None,
    );
}

// Row: project offers no issue type that is not a sub-task
#[tokio::test]
async fn issue_type_none() {
    let f = Fixture::new().await;
    f.reply(
        "GET",
        &api("/issue/createmeta/ABC/issuetypes"),
        200,
        issue_types(&[]),
    )
    .await;
    assert_error(
        f.adapter
            .create_issue(&project_ref("ABC"), "x", ExternalActionKey::new())
            .await,
        "JIRA_ISSUE_TYPE_NONE",
        false,
        None,
    );
}

// Row: attachment over 100 MiB, over 2,000 comments, comment body over 32,767 characters, or 413
#[tokio::test]
async fn too_large() {
    let f = Fixture::new().await;
    f.reply(
        "GET",
        &api("/issue/ABC-12"),
        200,
        attachment_issue(104_857_601),
    )
    .await;
    let dest = f.dir.path().join("big.bin");
    assert_error(
        f.adapter
            .download_attachment(&issue_ref("ABC-12"), "10001", &dest)
            .await,
        "JIRA_TOO_LARGE",
        false,
        None,
    );
    f.reply(
        "GET",
        &api("/issue/ABC-12/comment"),
        200,
        comments_page(0, 2001, vec![]),
    )
    .await;
    assert_error(
        f.adapter.comments(&issue_ref("ABC-12")).await,
        "JIRA_TOO_LARGE",
        false,
        None,
    );
    let key = ExternalActionKey::new();
    let huge = JiraCommentBody {
        markdown: "x".repeat(40_000),
        code_block: None,
    };
    assert_error(
        f.adapter
            .add_comment(&issue_ref("ABC-12"), &huge, key)
            .await,
        "JIRA_TOO_LARGE",
        false,
        None,
    );
    assert_eq!(f.action_state(key).await, None, "nothing was begun or sent");
    f.status("GET", &api("/project/BIG"), 413).await;
    assert_error(
        f.adapter.project(&project_ref("BIG")).await,
        "JIRA_TOO_LARGE",
        false,
        None,
    );
}

// Row: 429 after retries
#[tokio::test]
async fn throttled() {
    let f = Fixture::new().await;
    always(&f, "POST", &api("/search/jql"), 429).await;
    assert_error(
        f.adapter.search(SITE, &text_search(), 10).await,
        "JIRA_THROTTLED",
        true,
        Some(RepairAction::TryAgain),
    );
    assert_eq!(f.received("POST", &api("/search/jql")).await.len(), 3);
}

// Row: 5xx, network failure or timeout before sending, after retries; Journal unavailable
#[tokio::test]
async fn unavailable() {
    let f = Fixture::new().await;
    for status in [503, 500, 502] {
        always(&f, "GET", &api(&format!("/project/P{status}")), status).await;
        let key = format!("P{status}");
        assert_error(
            f.adapter.project(&project_ref(&key)).await,
            "JIRA_UNAVAILABLE",
            true,
            Some(RepairAction::TryAgain),
        );
        assert_eq!(
            f.received("GET", &api(&format!("/project/{key}")))
                .await
                .len(),
            3
        );
    }
    // Journal unavailable: its datastore is closed, so the bracket cannot begin and nothing is sent.
    let f = Fixture::new().await;
    f.status("PUT", &api("/issue/ABC-12/assignee"), 204).await;
    f.journal.datastore().close().await;
    assert_error(
        f.adapter
            .assign(&issue_ref("ABC-12"), ACCOUNT_SAM, ExternalActionKey::new())
            .await,
        "JIRA_UNAVAILABLE",
        true,
        Some(RepairAction::TryAgain),
    );
    assert!(f
        .received("PUT", &api("/issue/ABC-12/assignee"))
        .await
        .is_empty());
}

// Row: body unparseable, over 10 MiB, missing a field, unknown status category, redirect to
// another host
#[tokio::test]
async fn response_invalid() {
    let f = Fixture::new().await;
    Mock::given(method("GET"))
        .and(path(api("/myself")))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>not json</html>"))
        .mount(&f.mock)
        .await;
    invalid(f.adapter.myself(SITE).await);
    f.reply(
        "GET",
        &api("/project/ABC"),
        200,
        json!({"id": "1", "key": "ABC"}),
    )
    .await;
    invalid(f.adapter.project(&project_ref("ABC")).await);
    let mut bad_category = issue("10012", "ABC-12", "x");
    bad_category["fields"]["status"]["statusCategory"]["key"] = json!("undefined");
    f.reply(
        "POST",
        &api("/search/jql"),
        200,
        search_reply(vec![bad_category]),
    )
    .await;
    invalid(f.adapter.search(SITE, &text_search(), 10).await);
    let mut bad_time = issue("10012", "ABC-12", "x");
    bad_time["fields"]["updated"] = json!("yesterday");
    f.reply("GET", &api("/issue/ABC-12"), 200, bad_time).await;
    invalid(f.adapter.issue(&issue_ref("ABC-12")).await);
    // Over 10 MiB.
    Mock::given(method("GET"))
        .and(path(api("/project/HUGE")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b' '; 10 * 1024 * 1024 + 1]))
        .mount(&f.mock)
        .await;
    invalid(f.adapter.project(&project_ref("HUGE")).await);
    // A redirect on an attachment to another host.
    f.reply("GET", &api("/issue/ABC-14"), 200, json!({"id": "1", "key": "ABC-14", "fields": {"attachment": [
        {"id": "10001", "filename": "a", "mimeType": "text/plain", "size": 1, "created": JIRA_TIME}]}})).await;
    Mock::given(method("GET"))
        .and(path(api("/attachment/content/10001")))
        .respond_with(
            ResponseTemplate::new(302).insert_header("Location", "https://evil.example/steal"),
        )
        .mount(&f.mock)
        .await;
    let dest = f.dir.path().join("a.bin");
    invalid(
        f.adapter
            .download_attachment(&issue_ref("ABC-14"), "10001", &dest)
            .await,
    );
    assert!(!dest.exists() && !f.dir.path().join("a.bin.part").exists());
    // Another 4xx has no row of its own.
    f.status("GET", &api("/project/GONE"), 410).await;
    invalid(f.adapter.project(&project_ref("GONE")).await);
}

// Row: comment or issue creation timed out after sending and could not be found
#[tokio::test]
async fn outcome_unknown() {
    let f = Fixture::with(Mode::ApiToken, Some(Duration::from_millis(300))).await;
    Mock::given(method("POST"))
        .and(path(api("/issue/ABC-12/comment")))
        .respond_with(ResponseTemplate::new(201).set_delay(Duration::from_secs(3)))
        .mount(&f.mock)
        .await;
    let key = ExternalActionKey::new();
    assert_error(
        f.adapter
            .add_comment(&issue_ref("ABC-12"), &comment_body(), key)
            .await,
        "JIRA_OUTCOME_UNKNOWN",
        false,
        None,
    );
    // create_issue: the look-up finds nothing, so the outcome stays unknown and nothing is resent.
    f.reply(
        "GET",
        &api("/issue/createmeta/ABC/issuetypes"),
        200,
        issue_types(&[("1", "Task", false)]),
    )
    .await;
    Mock::given(method("POST"))
        .and(path(api("/issue")))
        .respond_with(ResponseTemplate::new(201).set_delay(Duration::from_secs(3)))
        .mount(&f.mock)
        .await;
    f.reply("POST", &api("/search/jql"), 200, search_reply(vec![]))
        .await;
    let key = ExternalActionKey::new();
    assert_error(
        f.adapter
            .create_issue(&project_ref("ABC"), "Plan", key)
            .await,
        "JIRA_OUTCOME_UNKNOWN",
        false,
        None,
    );
    assert_eq!(f.received("POST", &api("/issue")).await.len(), 1);
    assert_eq!(f.action_state(key).await, Some(ExternalActionState::Sent));
}

// Row: external-action key already used
#[tokio::test]
async fn action_key_used() {
    let f = Fixture::new().await;
    f.status("PUT", &api("/issue/ABC-12/assignee"), 204).await;
    let key = ExternalActionKey::new();
    f.adapter
        .assign(&issue_ref("ABC-12"), ACCOUNT_SAM, key)
        .await
        .unwrap();
    assert_error(
        f.adapter
            .assign(&issue_ref("ABC-12"), ACCOUNT_SAM, key)
            .await,
        "JIRA_ACTION_KEY_USED",
        false,
        None,
    );
    assert_eq!(
        f.received("PUT", &api("/issue/ABC-12/assignee"))
            .await
            .len(),
        1,
        "nothing sent the second time"
    );
}
