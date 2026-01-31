//! X2: the Gherkin of JiraAdapter.md, one test per scenario named after it; the exact title is the
//! comment above each test.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_identity::SecretString;
use ordin_types::domain::runs::{ExternalActionKind, ExternalActionState};
use ordin_types::ExternalActionKey;
use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, Request, Respond, ResponseTemplate};

use super::harness::{authorization, body_json, issue_ref, project_ref, Fixture, Mode};
use super::support::fixtures::*;
use crate::{
    JiraAdapter, JiraCodeBlock, JiraCommentBody, JiraError, JiraQuery, JiraStatusCategory,
};

const API: &str = "/rest/api/3";

fn api(tail: &str) -> String {
    format!("{API}{tail}")
}

/// Answers a comment POST with the comment it was given, as Jira does.
struct EchoComment;

impl Respond for EchoComment {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body = body_json(request)["body"].clone();
        ResponseTemplate::new(201).set_body_json(json!({
            "id": "20001", "author": user(ACCOUNT_PRIYA, "Priya Shah"),
            "created": JIRA_TIME, "updated": JIRA_TIME, "body": body
        }))
    }
}

async fn echo_comments(f: &Fixture) {
    Mock::given(method("POST"))
        .and(path(api("/issue/ABC-12/comment")))
        .respond_with(EchoComment)
        .mount(&f.mock)
        .await;
}

// Scenario: Happy path — addresses parse to the canonical form
#[test]
fn happy_path_addresses_parse_to_the_canonical_form() {
    let project = JiraAdapter::parse_project_address(
        "https://acme.atlassian.net/jira/software/projects/abc/boards/3",
    )
    .unwrap();
    assert_eq!(project.site, "acme.atlassian.net");
    assert_eq!(project.key, "ABC");
    assert_eq!(
        JiraAdapter::project_address(&project),
        "https://acme.atlassian.net/projects/ABC"
    );
    let issue = JiraAdapter::parse_issue_address(
        "https://acme.atlassian.net/jira/software/projects/ABC/boards/3?selectedIssue=ABC-12",
    )
    .unwrap();
    assert_eq!(issue.key, "ABC-12");
}

// Scenario Outline: Error path — addresses that are not Jira Cloud
#[tokio::test]
async fn error_path_addresses_that_are_not_jira_cloud() {
    let f = Fixture::new().await;
    for address in [
        "http://acme.atlassian.net/browse/ABC-12",
        "https://jira.acme.co.uk/browse/ABC-12",
        "https://acme.atlassian.net/browse/ABC-0",
        "https://acme.atlassian.net/projects/ABC",
    ] {
        assert_eq!(
            JiraAdapter::parse_issue_address(address),
            Err(JiraError::AddressInvalid),
            "{address}"
        );
    }
    assert_eq!(f.total_requests().await, 0, "parsing sends nothing");
}

// Scenario: Happy path — only assignable people are found
#[tokio::test]
async fn happy_path_only_assignable_people_are_found() {
    let f = Fixture::new().await;
    // The stand-in lists the assignable people only; "Sam Jones" is not among them. An inactive
    // user and an app account are dropped by the adapter.
    let mut inactive = user("old-sam", "Sam Old");
    inactive["active"] = json!(false);
    let mut app = user("app-sam", "Sam Bot");
    app["accountType"] = json!("app");
    Mock::given(method("GET"))
        .and(path(api("/user/assignable/search")))
        .and(query_param("project", "ABC"))
        .and(query_param("query", "sam"))
        .and(query_param("maxResults", "20"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            user("sam-lee", "Sam Lee"),
            user("sam-patel", "Sam Patel"),
            inactive,
            app
        ])))
        .mount(&f.mock)
        .await;
    let found = f
        .adapter
        .find_assignable(&project_ref("ABC"), "sam", 20)
        .await
        .unwrap();
    let names: Vec<&str> = found.iter().map(|u| u.display_name.as_str()).collect();
    assert_eq!(names, ["Sam Lee", "Sam Patel"]);
    assert!(found.iter().all(|u| u.email.is_none()), "C-88: no email");
}

// Scenario: Edge case — an email address that is not assignable gives no choice
#[tokio::test]
async fn edge_case_an_email_address_that_is_not_assignable_gives_no_choice() {
    let f = Fixture::new().await;
    f.reply("GET", &api("/user/assignable/search"), 200, json!([]))
        .await;
    let found = f
        .adapter
        .find_assignable(&project_ref("ABC"), "someone@other.co.uk", 20)
        .await
        .unwrap();
    assert!(found.is_empty());
}

// Scenario: Happy path — a pasted token is checked and kept without IT set-up
#[tokio::test]
async fn happy_path_a_pasted_token_is_checked_and_kept_without_it_set_up() {
    let f = Fixture::with(Mode::NoCredential, None).await;
    f.reply("GET", &api("/myself"), 200, myself()).await;
    let person = f
        .adapter
        .connect_with_token(
            SITE,
            "sam.lee@acme.co.uk",
            SecretString::new(TOKEN.to_owned()),
        )
        .await
        .unwrap();
    let sent = f.received("GET", &api("/myself")).await;
    assert_eq!(sent.len(), 1, "one GET /rest/api/3/myself");
    use base64::Engine as _;
    let expected = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("sam.lee@acme.co.uk:{TOKEN}"))
    );
    assert_eq!(authorization(&sent[0]), expected);
    let saved = f.creds.saved.lock().unwrap().clone();
    assert_eq!(
        saved,
        vec![(
            SITE.to_owned(),
            "sam.lee@acme.co.uk".to_owned(),
            TOKEN.to_owned(),
            ACCOUNT_PRIYA.to_owned(),
            "Priya Shah".to_owned()
        )]
    );
    // The command result and the journal carry no token.
    assert!(!serde_json::to_string(&person).unwrap().contains(TOKEN));
    assert!(!format!("{person:?}").contains(TOKEN));
    let actions = f
        .journal
        .list_external_actions(None, None)
        .await
        .map(|a| format!("{a:?}"))
        .unwrap_or_default();
    assert!(!actions.contains(TOKEN));
}

// Scenario: Error path — an admin has blocked API tokens
#[tokio::test]
async fn error_path_an_admin_has_blocked_api_tokens() {
    let f = Fixture::with(Mode::NoCredential, None).await;
    f.status("GET", &api("/myself"), 401).await;
    let result = f
        .adapter
        .connect_with_token(SITE, EMAIL, SecretString::new(TOKEN.to_owned()))
        .await;
    assert_eq!(result, Err(JiraError::TokenRefused));
    assert!(
        f.creds.saved.lock().unwrap().is_empty(),
        "save_jira_token is not called"
    );
}

// Scenario: Happy path — OAuth goes through the gateway
#[tokio::test]
async fn happy_path_oauth_goes_through_the_gateway() {
    let f = Fixture::with(Mode::Oauth, None).await;
    f.reply(
        "GET",
        "/oauth/token/accessible-resources",
        200,
        json!([{"id": "c1", "url": "https://acme.atlassian.net", "scopes": ["read:jira-work"]}]),
    )
    .await;
    f.reply(
        "GET",
        "/ex/jira/c1/rest/api/3/project/ABC",
        200,
        json!({"id": "10000", "key": "ABC", "name": "Accounts"}),
    )
    .await;
    let project = f.adapter.project(&project_ref("ABC")).await.unwrap();
    assert_eq!(project.name, "Accounts");
    let sent = f
        .received("GET", "/ex/jira/c1/rest/api/3/project/ABC")
        .await;
    assert_eq!(sent.len(), 1);
    assert_eq!(authorization(&sent[0]), format!("Bearer {TOKEN}"));
}

// Scenario: Error path — no Jira credential
#[tokio::test]
async fn error_path_no_jira_credential() {
    let f = Fixture::with(Mode::NoCredential, None).await;
    let result = f
        .adapter
        .search(SITE, &JiraQuery::Text("rates".into()), 10)
        .await;
    assert_eq!(result, Err(JiraError::NotConnected));
    assert_eq!(f.total_requests().await, 0);
}

// Scenario: Error path — another site
#[tokio::test]
async fn error_path_another_site() {
    let f = Fixture::new().await;
    let other = crate::JiraIssueRef {
        site: "other.atlassian.net".into(),
        key: "ABC-12".into(),
    };
    assert_eq!(
        f.adapter.issue(&other).await.err(),
        Some(JiraError::SiteMismatch)
    );
    assert_eq!(f.total_requests().await, 0);
}

// Scenario: Happy path — the handover search is one call with escaped values
#[tokio::test]
async fn happy_path_the_handover_search_is_one_call_with_escaped_values() {
    let f = Fixture::new().await;
    f.reply(
        "POST",
        &api("/search/jql"),
        200,
        search_reply(vec![issue("10012", "ABC-12", "Northwind assessment")]),
    )
    .await;
    let found = f
        .adapter
        .search(
            SITE,
            &JiraQuery::HandoversFor {
                label: "ordin-handover".into(),
                also_keys: vec!["ABC-12".into(), "ABC-13".into()],
            },
            50,
        )
        .await
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].status_category, JiraStatusCategory::InProgress);
    assert_eq!(found[0].assignee.as_ref().unwrap().display_name, "Sam Lee");
    let sent = f.received("POST", &api("/search/jql")).await;
    assert_eq!(sent.len(), 1, "exactly one POST");
    let body = body_json(&sent[0]);
    assert_eq!(
        body["jql"],
        "labels = \"ordin-handover\" AND ((assignee = currentUser() AND statusCategory != Done) OR key in (ABC-12, ABC-13)) ORDER BY updated DESC"
    );
    assert_eq!(
        body["fields"],
        json!(["summary", "assignee", "status", "labels", "updated"])
    );
    assert_eq!(body["maxResults"], 50);
}

// Scenario: Error path — a key that is not a key is refused before sending
#[tokio::test]
async fn error_path_a_key_that_is_not_a_key_is_refused_before_sending() {
    let f = Fixture::new().await;
    let result = f
        .adapter
        .search(
            SITE,
            &JiraQuery::HandoversFor {
                label: "ordin-handover".into(),
                also_keys: vec!["ABC-12) OR project = HR".into()],
            },
            50,
        )
        .await;
    assert_eq!(result, Err(JiraError::QueryInvalid));
    assert_eq!(f.total_requests().await, 0);
}

// Scenario: Edge case — search text cannot change the query
#[tokio::test]
async fn edge_case_search_text_cannot_change_the_query() {
    let f = Fixture::new().await;
    f.reply("POST", &api("/search/jql"), 200, search_reply(vec![]))
        .await;
    f.adapter
        .search(
            SITE,
            &JiraQuery::Text("rates\" OR project = HR OR text ~ \"x".into()),
            10,
        )
        .await
        .unwrap();
    let body = body_json(&f.received("POST", &api("/search/jql")).await[0]);
    assert_eq!(
        body["jql"],
        "text ~ \"rates\\\" OR project = HR OR text ~ \\\"x\" ORDER BY updated DESC"
    );
}

// Scenario: Happy path — reading an issue brings its parts
#[tokio::test]
async fn happy_path_reading_an_issue_brings_its_parts() {
    let f = Fixture::new().await;
    f.reply("GET", &api("/issue/ABC-12"), 200, abc12()).await;
    f.reply(
        "GET",
        &api("/issue/ABC-12/comment"),
        200,
        comments_page(
            0,
            3,
            vec![
                comment("10300", "2026-01-10T09:00:00.000+0000", "first"),
                comment("10301", "2026-01-10T10:00:00.000+0000", "second"),
                comment("10302", "2026-01-10T11:00:00.000+0000", "third"),
            ],
        ),
    )
    .await;
    let content = f.adapter.issue(&issue_ref("ABC-12")).await.unwrap();
    assert_eq!(
        content.description_markdown,
        "Assess the Northwind account."
    );
    let texts: Vec<&str> = content
        .comments
        .iter()
        .map(|c| c.body_markdown.as_str())
        .collect();
    assert_eq!(texts, ["first", "second", "third"]);
    assert_eq!(content.attachments.len(), 2);
    assert_eq!(content.subtasks.len(), 1);
    assert_eq!(content.subtasks[0].issue.key, "ABC-13");
    assert_eq!(content.linked.len(), 1);
    assert_eq!(content.linked[0].relation, "blocks");
    assert_eq!(content.linked[0].issue.issue.key, "ABC-20");
    assert_eq!(content.summary.summary, "Northwind assessment");
    // The parent's listing carries no time for them, so the parent's stands in.
    assert_eq!(content.subtasks[0].updated, content.summary.updated);
    for other in ["/issue/ABC-13", "/issue/ABC-20"] {
        assert!(
            f.received("GET", &api(other)).await.is_empty(),
            "{other} is not read"
        );
    }
}

// Scenario: Happy path — a comment carries a code block that is read back exactly
#[tokio::test]
async fn happy_path_a_comment_carries_a_code_block_that_is_read_back_exactly() {
    let f = Fixture::new().await;
    echo_comments(&f).await;
    let block = JiraCodeBlock {
        language: Some("yaml".into()),
        text: "# ordin.handover/v1 event\nkind: sent".into(),
    };
    let created = f
        .adapter
        .add_comment(
            &issue_ref("ABC-12"),
            &JiraCommentBody {
                markdown: "Handed over to Sam Lee".into(),
                code_block: Some(block.clone()),
            },
            ExternalActionKey::new(),
        )
        .await
        .unwrap();
    let posted = body_json(&f.received("POST", &api("/issue/ABC-12/comment")).await[0]);
    let nodes = posted["body"]["content"].as_array().unwrap();
    assert_eq!(nodes[0]["type"], "paragraph");
    assert_eq!(nodes[1]["type"], "codeBlock");
    assert_eq!(nodes[1]["attrs"]["language"], "yaml");
    assert_eq!(nodes[1]["content"][0]["text"], block.text.as_str());
    assert_eq!(created.code_blocks, vec![block]);
}

/// Records what the Journal held when the write reached the stand-in.
struct SeenState {
    handle: tokio::runtime::Handle,
    journal: Arc<ordin_journal::Journal>,
    key: ExternalActionKey,
    seen: Arc<Mutex<Option<(ExternalActionState, ExternalActionKind, String)>>>,
}

impl Respond for SeenState {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        // The stand-in answers on its own thread; the Journal's pool belongs to the test's runtime.
        let action = std::thread::scope(|scope| {
            scope
                .spawn(|| self.handle.block_on(self.journal.external_action(self.key)))
                .join()
                .expect("the look-up thread")
        });
        if let Ok(Some(a)) = action {
            *self.seen.lock().unwrap() = Some((a.state, a.kind, a.destination));
        }
        ResponseTemplate::new(204)
    }
}

// Scenario: Happy path — every write is bracketed as an external action
#[tokio::test]
async fn happy_path_every_write_is_bracketed_as_an_external_action() {
    let f = Fixture::new().await;
    let key = ExternalActionKey::new();
    let seen = Arc::new(Mutex::new(None));
    Mock::given(method("PUT"))
        .and(path(api("/issue/ABC-12/assignee")))
        .respond_with(SeenState {
            handle: tokio::runtime::Handle::current(),
            journal: Arc::clone(&f.journal),
            key,
            seen: Arc::clone(&seen),
        })
        .mount(&f.mock)
        .await;
    f.adapter
        .assign(&issue_ref("ABC-12"), ACCOUNT_SAM, key)
        .await
        .unwrap();
    let before = seen.lock().unwrap().clone().expect("begun before the PUT");
    assert_eq!(before.0, ExternalActionState::Sent);
    assert_eq!(before.1, ExternalActionKind::HandoverWritten);
    assert_eq!(before.2, "Jira \u{203a} acme.atlassian.net \u{203a} ABC-12");
    assert_eq!(
        f.action_state(key).await,
        Some(ExternalActionState::Confirmed)
    );
    let body = body_json(&f.received("PUT", &api("/issue/ABC-12/assignee")).await[0]);
    assert_eq!(body, json!({"accountId": ACCOUNT_SAM}));
}

// Scenario: Error path — the write is refused
#[tokio::test]
async fn error_path_the_write_is_refused() {
    let f = Fixture::new().await;
    f.status("PUT", &api("/issue/ABC-12/assignee"), 403).await;
    let key = ExternalActionKey::new();
    let result = f
        .adapter
        .assign(&issue_ref("ABC-12"), ACCOUNT_SAM, key)
        .await;
    assert_eq!(result, Err(JiraError::AccessDenied));
    assert_eq!(f.action_state(key).await, Some(ExternalActionState::Sent));
}

// Scenario: Error path — a comment whose outcome is unknown is not resent
#[tokio::test]
async fn error_path_a_comment_whose_outcome_is_unknown_is_not_resent() {
    let f = Fixture::with(Mode::ApiToken, Some(Duration::from_millis(300))).await;
    Mock::given(method("POST"))
        .and(path(api("/issue/ABC-12/comment")))
        .respond_with(ResponseTemplate::new(201).set_delay(Duration::from_secs(3)))
        .mount(&f.mock)
        .await;
    let key = ExternalActionKey::new();
    let result = f
        .adapter
        .add_comment(
            &issue_ref("ABC-12"),
            &JiraCommentBody {
                markdown: "hello".into(),
                code_block: None,
            },
            key,
        )
        .await;
    assert_eq!(result.err(), Some(JiraError::OutcomeUnknown));
    assert_eq!(
        f.received("POST", &api("/issue/ABC-12/comment"))
            .await
            .len(),
        1
    );
    assert_eq!(f.action_state(key).await, Some(ExternalActionState::Sent));
}

// Scenario: Edge case — an assignment whose outcome is unknown is repeated once with the same key
#[tokio::test]
async fn edge_case_an_assignment_whose_outcome_is_unknown_is_repeated_once_with_the_same_key() {
    let f = Fixture::with(Mode::ApiToken, Some(Duration::from_millis(300))).await;
    Mock::given(method("PUT"))
        .and(path(api("/issue/ABC-12/assignee")))
        .respond_with(ResponseTemplate::new(204).set_delay(Duration::from_secs(3)))
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

// Scenario: Edge case — an issue created with an unknown outcome is found again
#[tokio::test]
async fn edge_case_an_issue_created_with_an_unknown_outcome_is_found_again() {
    let f = Fixture::with(Mode::ApiToken, Some(Duration::from_millis(300))).await;
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
    f.reply(
        "POST",
        &api("/search/jql"),
        200,
        search_reply(vec![issue("10040", "ABC-40", "Northwind assessment")]),
    )
    .await;
    let key = ExternalActionKey::new();
    let made = f
        .adapter
        .create_issue(&project_ref("ABC"), "Northwind assessment", key)
        .await
        .unwrap();
    assert_eq!(made.key, "ABC-40");
    assert_eq!(f.received("POST", &api("/issue")).await.len(), 1);
    let lookup = body_json(&f.received("POST", &api("/search/jql")).await[0]);
    assert_eq!(
        lookup["jql"],
        "project = \"ABC\" AND reporter = currentUser() AND summary ~ \"Northwind assessment\" AND created >= -15m ORDER BY created DESC"
    );
    assert_eq!(lookup["maxResults"], 2);
    assert_eq!(
        f.action_state(key).await,
        Some(ExternalActionState::Confirmed)
    );
}

// Scenario: Happy path — a new issue uses the Task type
#[tokio::test]
async fn happy_path_a_new_issue_uses_the_task_type() {
    let f = Fixture::new().await;
    f.reply(
        "GET",
        &api("/issue/createmeta/ABC/issuetypes"),
        200,
        issue_types(&[
            ("1", "Epic", false),
            ("2", "Task", false),
            ("3", "Sub-task", true),
        ]),
    )
    .await;
    f.reply(
        "POST",
        &api("/issue"),
        201,
        json!({"id": "10041", "key": "ABC-41"}),
    )
    .await;
    let key = ExternalActionKey::new();
    let made = f
        .adapter
        .create_issue(&project_ref("ABC"), "Plan the handover", key)
        .await
        .unwrap();
    assert_eq!(made.key, "ABC-41");
    let body = body_json(&f.received("POST", &api("/issue")).await[0]);
    assert_eq!(body["fields"]["issuetype"], json!({"id": "2"}));
    assert_eq!(body["fields"]["project"], json!({"key": "ABC"}));
    assert_eq!(body["fields"]["summary"], "Plan the handover");
    let action = f.journal.external_action(key).await.unwrap().unwrap();
    assert_eq!(
        action.destination,
        "Jira \u{203a} acme.atlassian.net \u{203a} ABC"
    );
    assert_eq!(action.state, ExternalActionState::Confirmed);
}

// Scenario: Error path — a project with only sub-task types
#[tokio::test]
async fn error_path_a_project_with_only_sub_task_types() {
    let f = Fixture::new().await;
    f.reply(
        "GET",
        &api("/issue/createmeta/ABC/issuetypes"),
        200,
        issue_types(&[("3", "Sub-task", true)]),
    )
    .await;
    let key = ExternalActionKey::new();
    let result = f.adapter.create_issue(&project_ref("ABC"), "x", key).await;
    assert_eq!(result.err(), Some(JiraError::IssueTypeNone));
    assert!(f.received("POST", &api("/issue")).await.is_empty());
    assert_eq!(f.action_state(key).await, None, "nothing was begun");
}

// Scenario: Error path — throttling is retried, then reported
#[tokio::test]
async fn error_path_throttling_is_retried_then_reported() {
    let f = Fixture::new().await;
    Mock::given(method("POST"))
        .and(path(api("/search/jql")))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "1"))
        .mount(&f.mock)
        .await;
    let result = f
        .adapter
        .search(SITE, &JiraQuery::Text("rates".into()), 10)
        .await;
    assert_eq!(result, Err(JiraError::Throttled));
    assert_eq!(
        f.received("POST", &api("/search/jql")).await.len(),
        3,
        "3 attempts"
    );
}

// Scenario: Edge case — throttling once does not lose the write
#[tokio::test]
async fn edge_case_throttling_once_does_not_lose_the_write() {
    let f = Fixture::new().await;
    Mock::given(method("POST"))
        .and(path(api("/issue/ABC-12/comment")))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "1"))
        .up_to_n_times(1)
        .mount(&f.mock)
        .await;
    echo_comments(&f).await;
    let key = ExternalActionKey::new();
    let created = f
        .adapter
        .add_comment(
            &issue_ref("ABC-12"),
            &JiraCommentBody {
                markdown: "hello".into(),
                code_block: None,
            },
            key,
        )
        .await
        .unwrap();
    assert_eq!(created.body_markdown, "hello");
    assert_eq!(
        f.received("POST", &api("/issue/ABC-12/comment"))
            .await
            .len(),
        2
    );
    assert_eq!(
        f.action_state(key).await,
        Some(ExternalActionState::Confirmed)
    );
}

// Scenario: Error path — an attachment that is too large is not downloaded
#[tokio::test]
async fn error_path_an_attachment_that_is_too_large_is_not_downloaded() {
    let f = Fixture::new().await;
    f.reply(
        "GET",
        &api("/issue/ABC-12"),
        200,
        attachment_issue(104_857_601),
    )
    .await;
    let dest = f.dir.path().join("terms.pdf");
    let result = f
        .adapter
        .download_attachment(&issue_ref("ABC-12"), "10001", &dest)
        .await;
    assert_eq!(result.err(), Some(JiraError::TooLarge));
    assert!(f
        .received("GET", &api("/attachment/content/10001"))
        .await
        .is_empty());
    assert!(!dest.exists());
    assert!(!f.dir.path().join("terms.pdf.part").exists());
}

// Scenario: Edge case — raw HTML and unknown nodes stay text
#[test]
fn edge_case_raw_html_and_unknown_nodes_stay_text() {
    let adf = JiraAdapter::markdown_to_adf("<script>alert(1)</script>", None).unwrap();
    let nodes = adf["content"].as_array().unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0]["type"], "paragraph");
    assert_eq!(nodes[0]["content"].as_array().unwrap().len(), 1);
    assert_eq!(nodes[0]["content"][0]["text"], "<script>alert(1)</script>");
    let doc = json!({"version": 1, "type": "doc", "content": [{"type": "extension"}]});
    let (markdown, _) = JiraAdapter::adf_to_markdown(&doc).unwrap();
    assert_eq!(markdown, "[Jira content ordin can't show: extension]");
}
