//! `GithubAccess` against the `wiremock` GitHub and GitLab stand-ins with recorded replies: the
//! scenarios of *Acceptance Criteria* on repository hosts (X2), the `GH_*` rows of the Error Table
//! (X1) and every answer of `branch_review_status`.

use ordin_types::{CodedError, RepairAction};
use serde_json::{json, Value};
use wiremock::matchers::{body_string_contains, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::standin::{hosts, hosts_with, point_tools};
use super::support::{fixture, GITHUB_TOKEN, GITLAB_TOKEN, NOBODY};
use crate::error::GithubError;
use crate::{
    BranchReviewStatus, BranchWithFiles, GithubAccess, OpenPullRequest, PullRequestState,
    RepoFolderRef, RepoPermission, ReviewDecision, ReviewVerdict,
};

const GITHUB: &str = "github.com";
const GITLAB: &str = "gitlab.acme.co.uk";

async fn reply(server: &MockServer, verb: &str, url: &str, status: u16, body: Value) {
    Mock::given(method(verb))
        .and(path(url))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(server)
        .await;
}

async fn get(server: &MockServer, url: &str, status: u16, body: Value) {
    reply(server, "GET", url, status, body).await;
}

fn pull_request_rule(count: u64) -> Value {
    json!([{
        "type": "pull_request",
        "ruleset_source_type": "Repository",
        "ruleset_source": "acme/standards",
        "ruleset_id": 42,
        "parameters": {
            "required_approving_review_count": count,
            "dismiss_stale_reviews_on_push": false,
            "require_code_owner_review": false,
            "require_last_push_approval": false,
            "required_review_thread_resolution": false,
        },
    }])
}

fn not_found() -> Value {
    json!({"message": "Not Found", "status": "404"})
}

/// Classic protection the person cannot read: no ruleset, `protected` true, reviews 404.
async fn hidden_classic_protection(server: &MockServer, repository: &str, status: u16) {
    get(
        server,
        &format!("/repos/{repository}/rules/branches/main"),
        200,
        json!([]),
    )
    .await;
    get(
        server,
        &format!("/repos/{repository}/branches/main"),
        200,
        json!({"name": "main", "protected": true}),
    )
    .await;
    get(
        server,
        &format!("/repos/{repository}/branches/main/protection/required_pull_request_reviews"),
        status,
        json!({"message": "Resource not accessible by personal access token"}),
    )
    .await;
}

async fn gitlab_project(server: &MockServer) {
    get(
        server,
        "/projects/platform%2Fknowledge",
        200,
        json!({"id": 7, "default_branch": "main"}),
    )
    .await;
}

fn nobody_may_push() -> Value {
    json!({
        "name": "main",
        "push_access_levels": [{"access_level": 0, "access_level_description": "No one"}],
        "merge_access_levels": [{"access_level": 30}],
    })
}

// ---- X2: the ADR-110 scenarios

#[tokio::test]
async fn happy_path_a_github_ruleset_requires_review_read_access_only() {
    let h = hosts().await;
    get(
        &h.server,
        "/repos/acme/standards/rules/branches/main",
        200,
        pull_request_rule(2),
    )
    .await;
    let status = h
        .service()
        .branch_review_status(GITHUB, "acme/standards", "main")
        .await
        .unwrap();
    assert_eq!(
        status,
        BranchReviewStatus::Required {
            required_approvals: 2,
            code_owner_review_required: false,
        }
    );
}

#[tokio::test]
async fn edge_classic_protection_hidden_from_the_person() {
    let h = hosts().await;
    hidden_classic_protection(&h.server, "acme/standards", 404).await;
    let status = h
        .service()
        .branch_review_status(GITHUB, "acme/standards", "main")
        .await
        .unwrap();
    assert_eq!(status, BranchReviewStatus::CannotTell);
}

#[tokio::test]
async fn a_read_only_token_on_classic_protection_answers_403_and_cannot_tell() {
    let h = hosts().await;
    hidden_classic_protection(&h.server, "acme/standards", 403).await;
    let status = h
        .service()
        .branch_review_status(GITHUB, "acme/standards", "main")
        .await
        .unwrap();
    assert_eq!(status, BranchReviewStatus::CannotTell);
}

#[tokio::test]
async fn edge_an_unprotected_github_branch() {
    let h = hosts().await;
    get(
        &h.server,
        "/repos/acme/notes/rules/branches/main",
        200,
        json!([]),
    )
    .await;
    get(
        &h.server,
        "/repos/acme/notes/branches/main",
        200,
        json!({"name": "main", "protected": false}),
    )
    .await;
    let status = h
        .service()
        .branch_review_status(GITHUB, "acme/notes", "main")
        .await
        .unwrap();
    assert_eq!(status, BranchReviewStatus::NotRequired);
}

#[tokio::test]
async fn edge_gitlab_free_with_nobody_allowed_to_push() {
    let h = hosts().await;
    gitlab_project(&h.server).await;
    get(
        &h.server,
        "/projects/platform%2Fknowledge/protected_branches/main",
        200,
        nobody_may_push(),
    )
    .await;
    get(
        &h.server,
        "/projects/platform%2Fknowledge/approval_rules",
        403,
        json!({"message": "403 Forbidden"}),
    )
    .await;
    let status = h
        .service()
        .branch_review_status(GITLAB, "platform/knowledge", "main")
        .await
        .unwrap();
    assert_eq!(status, BranchReviewStatus::CannotTell);
}

#[tokio::test]
async fn edge_gitlab_approval_rule_covers_the_branch() {
    let h = hosts().await;
    gitlab_project(&h.server).await;
    get(
        &h.server,
        "/projects/platform%2Fknowledge/protected_branches/main",
        200,
        nobody_may_push(),
    )
    .await;
    get(
        &h.server,
        "/projects/platform%2Fknowledge/approval_rules",
        200,
        json!([{
            "id": 1,
            "name": "Everything",
            "approvals_required": 1,
            "applies_to_all_protected_branches": true,
            "protected_branches": [],
        }]),
    )
    .await;
    let status = h
        .service()
        .branch_review_status(GITLAB, "platform/knowledge", "main")
        .await
        .unwrap();
    assert_eq!(
        status,
        BranchReviewStatus::Required {
            required_approvals: 1,
            code_owner_review_required: false,
        }
    );
}

#[tokio::test]
async fn regression_fix_write_access_without_admin_on_github() {
    let h = hosts().await;
    hidden_classic_protection(&h.server, "acme/supplier-team", 404).await;
    let rules = h
        .service()
        .branch_review_rules(GITHUB, "acme/supplier-team", "main")
        .await
        .unwrap();
    assert!(rules.review_required);
    assert!(!rules.code_owner_review_required);
    assert_eq!(rules.required_approvals, 0);
}

#[tokio::test]
async fn error_repository_not_visible() {
    let h = hosts().await;
    get(
        &h.server,
        "/repos/acme/secret/rules/branches/main",
        404,
        not_found(),
    )
    .await;
    let error = h
        .service()
        .branch_review_status(GITHUB, "acme/secret", "main")
        .await
        .unwrap_err();
    assert_eq!(error.code(), "GH_NOT_FOUND");
}

// ---- X2: the other host scenarios

#[tokio::test]
async fn the_team_knowledge_service_reads_permission_and_review_rules() {
    let h = hosts().await;
    get(
        &h.server,
        "/repos/acme/supplier-team",
        200,
        json!({"default_branch": "main", "permissions": {"pull": true, "push": true}}),
    )
    .await;
    get(
        &h.server,
        "/repos/acme/supplier-team/rules/branches/main",
        200,
        pull_request_rule(1),
    )
    .await;
    let service = h.service();
    assert_eq!(
        service
            .repository_permission(GITHUB, "acme/supplier-team")
            .await
            .unwrap(),
        RepoPermission::Write
    );
    let rules = service
        .branch_review_rules(GITHUB, "acme/supplier-team", "main")
        .await
        .unwrap();
    assert!(rules.review_required);
    assert!(!rules.code_owner_review_required);
    assert_eq!(rules.required_approvals, 1);
}

#[tokio::test]
async fn happy_path_the_push_service_gets_a_git_credential_without_a_host_call() {
    let h = hosts().await;
    let credential = h.service().git_credentials(GITHUB).await.unwrap();
    assert_eq!(credential.username, "x-access-token");
    assert_eq!(format!("{:?}", credential.token), "GitToken(***)");
    assert_eq!(credential.token.expose(), GITHUB_TOKEN);
    assert!(h.server.received_requests().await.unwrap().is_empty());
    let gitlab = h.service().git_credentials(GITLAB).await.unwrap();
    assert_eq!(gitlab.username, "oauth2");
    assert_eq!(gitlab.token.expose(), GITLAB_TOKEN);
}

#[tokio::test]
async fn happy_path_a_pull_request_is_opened_with_labels_and_reviewers() {
    let h = hosts().await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/agents/pulls"))
        .and(header(
            "Authorization",
            format!("token {GITHUB_TOKEN}").as_str(),
        ))
        .and(body_string_contains("head=ordin/rates"))
        .and(body_string_contains("base=main"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "number": 41,
            "html_url": "https://github.com/acme/agents/pull/41",
        })))
        .expect(1)
        .mount(&h.server)
        .await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/agents/issues/41/labels"))
        .and(body_string_contains("labels[]=handover"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&h.server)
        .await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/agents/pulls/41/requested_reviewers"))
        .and(body_string_contains("reviewers[]=samlee"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({})))
        .expect(1)
        .mount(&h.server)
        .await;
    let opened = h
        .service()
        .open_pull_request(OpenPullRequest {
            host: GITHUB.to_owned(),
            repository: "acme/agents".to_owned(),
            head: "ordin/rates".to_owned(),
            base: "main".to_owned(),
            title: "Rates".to_owned(),
            body: "Please look".to_owned(),
            labels: vec!["handover".to_owned()],
            reviewers: vec!["samlee".to_owned()],
        })
        .await
        .unwrap();
    assert_eq!(opened.number, 41);
    assert_eq!(opened.state, PullRequestState::Open);
    let order: Vec<String> = h
        .server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| r.url.path().to_owned())
        .collect();
    assert_eq!(
        order,
        [
            "/repos/acme/agents/pulls",
            "/repos/acme/agents/issues/41/labels",
            "/repos/acme/agents/pulls/41/requested_reviewers",
        ]
    );
}

#[tokio::test]
async fn error_path_a_handover_record_written_over_a_newer_one_is_refused() {
    let h = hosts().await;
    reply(
        &h.server,
        "PUT",
        "/repos/acme/agents/contents/handover.yaml",
        409,
        json!({"message": "handover.yaml does not match b1"}),
    )
    .await;
    get(
        &h.server,
        "/repos/acme/agents/contents/handover.yaml",
        200,
        json!({"content": "eQ==\n", "sha": "b2"}),
    )
    .await;
    let error = h
        .service()
        .update_file(
            GITHUB,
            "acme/agents",
            "handover/northwind",
            "handover.yaml",
            b"y".to_vec(),
            "b1",
            "Update",
        )
        .await
        .unwrap_err();
    match error {
        GithubError::Conflict { current_sha } => assert_eq!(current_sha, "b2"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn happy_path_workflow_files_are_listed_in_a_gitlab_folder() {
    let h = hosts().await;
    Mock::given(method("GET"))
        .and(path("/projects/ops%2Fautomation/repository/tree"))
        .and(query_param("path", "workflows"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"name": "a.workflow.yaml", "type": "blob"},
            {"name": "b.workflow.yaml", "type": "blob"},
            {"name": "notes.md", "type": "blob"},
            {"name": "sub", "type": "tree"},
        ])))
        .mount(&h.server)
        .await;
    Mock::given(method("GET"))
        .and(path("/projects/ops%2Fautomation/repository/commits"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "id": "c0ffee",
            "author_name": "Sam Lee",
            "committed_date": "2026-01-02T10:00:00.000+01:00",
        }])))
        .mount(&h.server)
        .await;
    let folder = RepoFolderRef {
        host: GITLAB.to_owned(),
        repository: "ops/automation".to_owned(),
        folder: "workflows".to_owned(),
    };
    let files = h
        .service()
        .list_files(&folder, "*.workflow.yaml")
        .await
        .unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].name, "a.workflow.yaml");
    assert_eq!(files[0].commit, "c0ffee");
    assert_eq!(files[1].last_changed_by, "Sam Lee");
    let received = h.server.received_requests().await.unwrap();
    let token = format!("token {GITLAB_TOKEN}");
    assert!(received
        .iter()
        .all(|r| r.headers.get("authorization").unwrap().to_str().unwrap() == token));
}

#[tokio::test]
async fn error_path_a_github_only_call_on_gitlab() {
    let h = hosts().await;
    let error = h
        .service()
        .merge(GITLAB, "ops/automation", 1)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "GH_HOST_NOT_SUPPORTED");
}

// ---- X1: the `GithubAccess` rows

fn assert_row(error: &GithubError, code: &str, retryable: bool, repair: Option<RepairAction>) {
    assert_eq!(error.code(), code);
    assert_eq!(error.to_string(), code);
    assert_eq!(error.retryable(), retryable);
    assert_eq!(CodedError::code(error), code);
    assert_eq!(CodedError::retryable(error), retryable);
    assert_eq!(CodedError::repair(error), repair);
}

#[tokio::test]
async fn gh_sign_in_needed_with_no_sign_in_for_the_host() {
    let h = hosts_with(NOBODY).await;
    let error = h.service().git_credentials(GITHUB).await.unwrap_err();
    assert_row(
        &error,
        "GH_SIGN_IN_NEEDED",
        false,
        Some(RepairAction::SignInAgain),
    );
}

#[tokio::test]
async fn gh_not_found_for_a_404_and_a_403_with_no_rate_limit_header() {
    let h = hosts().await;
    get(&h.server, "/repos/acme/gone", 404, not_found()).await;
    let error = h
        .service()
        .repository_permission(GITHUB, "acme/gone")
        .await
        .unwrap_err();
    assert_row(&error, "GH_NOT_FOUND", false, None);
    get(
        &h.server,
        "/repos/acme/hidden",
        403,
        json!({"message": "no"}),
    )
    .await;
    let hidden = GithubAccess::default_branch(&**h.service(), GITHUB, "acme/hidden")
        .await
        .unwrap_err();
    assert_eq!(hidden.code(), "GH_NOT_FOUND");
}

#[tokio::test]
async fn gh_rate_limited_for_a_403_or_429_with_a_rate_limit_header() {
    let h = hosts().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/busy"))
        .respond_with(
            ResponseTemplate::new(403)
                .insert_header("x-ratelimit-remaining", "0")
                .insert_header("retry-after", "120")
                .set_body_json(json!({"message": "API rate limit exceeded"})),
        )
        .mount(&h.server)
        .await;
    let error = h
        .service()
        .repository_permission(GITHUB, "acme/busy")
        .await
        .unwrap_err();
    assert_row(
        &error,
        "GH_RATE_LIMITED",
        true,
        Some(RepairAction::TryAgain),
    );
    assert!(matches!(
        error,
        GithubError::RateLimited { retry_after_s: 120 }
    ));
}

#[tokio::test]
async fn a_rate_limit_is_never_read_as_an_answer_on_review_status() {
    let h = hosts().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/busy/rules/branches/main"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "5"))
        .mount(&h.server)
        .await;
    let error = h
        .service()
        .branch_review_status(GITHUB, "acme/busy", "main")
        .await
        .unwrap_err();
    assert_eq!(error.code(), "GH_RATE_LIMITED");
}

#[tokio::test]
async fn gh_unreachable_when_the_host_cannot_be_reached() {
    let f = fixture().await;
    // A port nothing listens on any more.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    point_tools(&f, &base);
    let error = f
        .service
        .repository_permission(GITHUB, "acme/x")
        .await
        .unwrap_err();
    assert_row(&error, "GH_UNREACHABLE", true, Some(RepairAction::TryAgain));
}

#[tokio::test]
async fn gh_conflict_for_a_branch_that_exists() {
    let h = hosts().await;
    get(
        &h.server,
        "/repos/acme/agents/git/ref/heads/main",
        200,
        json!({"object": {"sha": "base"}}),
    )
    .await;
    reply(
        &h.server,
        "POST",
        "/repos/acme/agents/git/trees",
        201,
        json!({"sha": "tree"}),
    )
    .await;
    reply(
        &h.server,
        "POST",
        "/repos/acme/agents/git/commits",
        201,
        json!({"sha": "new"}),
    )
    .await;
    reply(
        &h.server,
        "POST",
        "/repos/acme/agents/git/refs",
        422,
        json!({"message": "Reference already exists"}),
    )
    .await;
    let error = h
        .service()
        .create_branch_with_files(BranchWithFiles {
            host: GITHUB.to_owned(),
            repository: "acme/agents".to_owned(),
            base: "main".to_owned(),
            branch: "taken".to_owned(),
            message: "Add".to_owned(),
            files: vec![("a.txt".to_owned(), b"a".to_vec())],
        })
        .await
        .unwrap_err();
    assert_row(&error, "GH_CONFLICT", false, None);
}

#[tokio::test]
async fn gh_host_not_supported_for_any_other_host() {
    let h = hosts().await;
    let error = h
        .service()
        .git_credentials("git.example.org")
        .await
        .unwrap_err();
    assert_row(&error, "GH_HOST_NOT_SUPPORTED", false, None);
    let list = RepoFolderRef {
        host: "git.example.org".to_owned(),
        repository: "a/b".to_owned(),
        folder: String::new(),
    };
    assert_eq!(
        h.service().list_files(&list, "*").await.unwrap_err().code(),
        "GH_HOST_NOT_SUPPORTED"
    );
}

#[tokio::test]
async fn gh_failed_for_any_other_tool_failure_and_never_with_the_token() {
    let h = hosts().await;
    get(
        &h.server,
        "/repos/acme/broken",
        500,
        json!({"message": "Server error"}),
    )
    .await;
    let error = h
        .service()
        .repository_permission(GITHUB, "acme/broken")
        .await
        .unwrap_err();
    assert_row(&error, "GH_FAILED", false, None);
    assert!(matches!(&error, GithubError::Other(line) if line == "Server error"));
    {
        let mut tunables = crate::service::lock(&h.fixture.service.tunables);
        tunables.gh_program = "/no/such/gh".into();
    }
    let missing = h
        .service()
        .repository_permission(GITHUB, "acme/x")
        .await
        .unwrap_err();
    assert_eq!(missing.code(), "GH_FAILED");
}

// ---- every other answer and method

#[tokio::test]
async fn a_ruleset_takes_the_largest_count_and_any_code_owner_rule() {
    let h = hosts().await;
    get(
        &h.server,
        "/repos/acme/many/rules/branches/release%2F1",
        200,
        json!([
            {"type": "deletion"},
            {"type": "pull_request", "parameters": {"required_approving_review_count": 1, "require_code_owner_review": true}},
            {"type": "pull_request", "parameters": {"required_approving_review_count": 3}},
        ]),
    )
    .await;
    // A slash in a branch name stays a path separator for GitHub.
    let status = h
        .service()
        .branch_review_status(GITHUB, "acme/many", "release/1")
        .await;
    assert!(status.is_err());
    Mock::given(method("GET"))
        .and(path("/repos/acme/many/rules/branches/release/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"type": "pull_request", "parameters": {"required_approving_review_count": 1, "require_code_owner_review": true}},
            {"type": "pull_request", "parameters": {"required_approving_review_count": 3}},
        ])))
        .mount(&h.server)
        .await;
    let status = h
        .service()
        .branch_review_status(GITHUB, "acme/many", "release/1")
        .await
        .unwrap();
    assert_eq!(
        status,
        BranchReviewStatus::Required {
            required_approvals: 3,
            code_owner_review_required: true,
        }
    );
}

#[tokio::test]
async fn classic_protection_the_person_can_read_is_believed() {
    let h = hosts().await;
    get(
        &h.server,
        "/repos/acme/open/rules/branches/main",
        200,
        json!([]),
    )
    .await;
    get(
        &h.server,
        "/repos/acme/open/branches/main",
        200,
        json!({"protected": true}),
    )
    .await;
    get(
        &h.server,
        "/repos/acme/open/branches/main/protection/required_pull_request_reviews",
        200,
        json!({"required_approving_review_count": 2, "require_code_owner_reviews": true}),
    )
    .await;
    assert_eq!(
        h.service()
            .branch_review_status(GITHUB, "acme/open", "main")
            .await
            .unwrap(),
        BranchReviewStatus::Required {
            required_approvals: 2,
            code_owner_review_required: true,
        }
    );
    get(
        &h.server,
        "/repos/acme/zero/rules/branches/main",
        200,
        json!([]),
    )
    .await;
    get(
        &h.server,
        "/repos/acme/zero/branches/main",
        200,
        json!({"protected": true}),
    )
    .await;
    get(
        &h.server,
        "/repos/acme/zero/branches/main/protection/required_pull_request_reviews",
        200,
        json!({"required_approving_review_count": 0}),
    )
    .await;
    assert_eq!(
        h.service()
            .branch_review_status(GITHUB, "acme/zero", "main")
            .await
            .unwrap(),
        BranchReviewStatus::NotRequired
    );
}

#[tokio::test]
async fn a_uncertain_step_on_github_cannot_tell_and_a_missing_branch_is_not_found() {
    let h = hosts().await;
    get(
        &h.server,
        "/repos/acme/a/rules/branches/main",
        403,
        json!({"message": "no"}),
    )
    .await;
    assert_eq!(
        h.service()
            .branch_review_status(GITHUB, "acme/a", "main")
            .await
            .unwrap(),
        BranchReviewStatus::CannotTell
    );
    get(
        &h.server,
        "/repos/acme/b/rules/branches/main",
        200,
        json!([]),
    )
    .await;
    get(&h.server, "/repos/acme/b/branches/main", 404, not_found()).await;
    assert_eq!(
        h.service()
            .branch_review_status(GITHUB, "acme/b", "main")
            .await
            .unwrap_err()
            .code(),
        "GH_NOT_FOUND"
    );
    get(
        &h.server,
        "/repos/acme/c/rules/branches/main",
        200,
        json!([]),
    )
    .await;
    get(
        &h.server,
        "/repos/acme/c/branches/main",
        200,
        json!({"name": "main"}),
    )
    .await;
    assert_eq!(
        h.service()
            .branch_review_status(GITHUB, "acme/c", "main")
            .await
            .unwrap(),
        BranchReviewStatus::CannotTell
    );
}

#[tokio::test]
async fn branch_review_rules_reads_not_required_as_all_false() {
    let h = hosts().await;
    get(
        &h.server,
        "/repos/acme/notes/rules/branches/main",
        200,
        json!([]),
    )
    .await;
    get(
        &h.server,
        "/repos/acme/notes/branches/main",
        200,
        json!({"protected": false}),
    )
    .await;
    let rules = h
        .service()
        .branch_review_rules(GITHUB, "acme/notes", "main")
        .await
        .unwrap();
    assert!(!rules.review_required);
    assert_eq!(rules.required_approvals, 0);
}

#[tokio::test]
async fn gitlab_answers_for_each_kind_of_branch() {
    let h = hosts().await;
    gitlab_project(&h.server).await;
    let base = "/projects/platform%2Fknowledge";
    // Unprotected.
    get(
        &h.server,
        &format!("{base}/protected_branches/open"),
        404,
        json!({"message": "404 Not found"}),
    )
    .await;
    // Developers may push.
    get(
        &h.server,
        &format!("{base}/protected_branches/dev"),
        200,
        json!({"push_access_levels": [{"access_level": 30}]}),
    )
    .await;
    // One named user may push.
    get(
        &h.server,
        &format!("{base}/protected_branches/person"),
        200,
        json!({"push_access_levels": [{"access_level": 0, "user_id": 9}]}),
    )
    .await;
    // Nobody may push; no rule covers this branch.
    get(
        &h.server,
        &format!("{base}/protected_branches/other"),
        200,
        nobody_may_push(),
    )
    .await;
    // Nobody may push; a named rule covers it, with code owners.
    get(
        &h.server,
        &format!("{base}/protected_branches/named"),
        200,
        json!({
            "push_access_levels": [{"access_level": 0}],
            "code_owner_approval_required": true,
        }),
    )
    .await;
    get(
        &h.server,
        &format!("{base}/approval_rules"),
        200,
        json!([
            {"approvals_required": 2, "applies_to_all_protected_branches": false, "protected_branches": [{"name": "named"}]},
            {"approvals_required": 0, "applies_to_all_protected_branches": true, "protected_branches": []},
        ]),
    )
    .await;
    let status = |branch: &'static str| {
        let service = h.service().clone();
        async move {
            service
                .branch_review_status(GITLAB, "platform/knowledge", branch)
                .await
                .unwrap()
        }
    };
    assert_eq!(status("open").await, BranchReviewStatus::NotRequired);
    assert_eq!(status("dev").await, BranchReviewStatus::NotRequired);
    assert_eq!(status("person").await, BranchReviewStatus::NotRequired);
    assert_eq!(status("other").await, BranchReviewStatus::NotRequired);
    assert_eq!(
        status("named").await,
        BranchReviewStatus::Required {
            required_approvals: 2,
            code_owner_review_required: true,
        }
    );
}

#[tokio::test]
async fn a_gitlab_step_the_role_cannot_read_cannot_tell_and_a_server_error_fails() {
    let h = hosts().await;
    gitlab_project(&h.server).await;
    let base = "/projects/platform%2Fknowledge";
    get(
        &h.server,
        &format!("{base}/protected_branches/hidden"),
        403,
        json!({"message": "403 Forbidden"}),
    )
    .await;
    get(
        &h.server,
        &format!("{base}/protected_branches/broken"),
        500,
        json!({"message": "Internal Server Error"}),
    )
    .await;
    let service = h.service();
    assert_eq!(
        service
            .branch_review_status(GITLAB, "platform/knowledge", "hidden")
            .await
            .unwrap(),
        BranchReviewStatus::CannotTell
    );
    let error = service
        .branch_review_status(GITLAB, "platform/knowledge", "broken")
        .await
        .unwrap_err();
    assert_eq!(error.code(), "GH_FAILED");
}

#[tokio::test]
async fn a_gitlab_project_the_person_cannot_see_is_not_found() {
    let h = hosts().await;
    get(
        &h.server,
        "/projects/ops%2Fsecret",
        404,
        json!({"message": "404 Project Not Found"}),
    )
    .await;
    let error = h
        .service()
        .branch_review_status(GITLAB, "ops/secret", "main")
        .await
        .unwrap_err();
    assert_eq!(error.code(), "GH_NOT_FOUND");
}

#[tokio::test]
async fn gitlab_serves_repository_permission_and_gitlab_com_stays_reachable() {
    let h = hosts().await;
    get(
        &h.server,
        "/projects/platform%2Fknowledge",
        200,
        json!({"permissions": {"project_access": {"access_level": 30}, "group_access": null}}),
    )
    .await;
    for host in [GITLAB, "gitlab.com"] {
        assert_eq!(
            h.service()
                .repository_permission(host, "platform/knowledge")
                .await
                .unwrap(),
            RepoPermission::Write
        );
    }
}

fn hosts_pull_request() -> Value {
    json!({
        "state": "closed",
        "merged": true,
        "merge_commit_sha": "cafe",
        "head": {"ref": "ordin/rates"},
        "base": {"ref": "main"},
        "html_url": "https://github.com/acme/agents/pull/5",
    })
}

#[tokio::test]
async fn a_pull_request_view_reads_state_and_reviews() {
    let h = hosts().await;
    get(
        &h.server,
        "/repos/acme/agents/pulls/5",
        200,
        hosts_pull_request(),
    )
    .await;
    get(
        &h.server,
        "/repos/acme/agents/pulls/5/reviews",
        200,
        json!([
            {"user": {"login": "a"}, "state": "APPROVED", "body": "", "submitted_at": "2026-01-02T10:00:00Z"},
            {"user": {"login": "b"}, "state": "CHANGES_REQUESTED", "body": "Fix it", "submitted_at": "2026-01-02T11:00:00Z"},
        ]),
    )
    .await;
    let view = h
        .service()
        .pull_request(GITHUB, "acme/agents", 5)
        .await
        .unwrap();
    assert_eq!(view.state, PullRequestState::Merged);
    assert_eq!(view.review, ReviewDecision::ChangesRequested);
    assert_eq!(view.change_requests[0].reviewer, "b");
    assert_eq!(view.merge_commit.as_deref(), Some("cafe"));
    assert_eq!(view.head, "ordin/rates");
}

#[tokio::test]
async fn pull_request_for_branch_finds_the_newest_or_none() {
    let h = hosts().await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/agents/pulls"))
        .and(query_param("head", "acme:ordin/rates"))
        .and(query_param("state", "all"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "number": 8, "html_url": "u", "state": "closed", "merged_at": "2026-01-02T10:00:00Z",
        }])))
        .mount(&h.server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/agents/pulls"))
        .and(query_param("head", "acme:none"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&h.server)
        .await;
    let found = h
        .service()
        .pull_request_for_branch(GITHUB, "acme/agents", "ordin/rates")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.state, PullRequestState::Merged);
    assert!(h
        .service()
        .pull_request_for_branch(GITHUB, "acme/agents", "none")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn create_branch_with_files_posts_the_tree_the_commit_and_the_ref() {
    let h = hosts().await;
    get(
        &h.server,
        "/repos/acme/agents/git/ref/heads/main",
        200,
        json!({"object": {"sha": "base"}}),
    )
    .await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/agents/git/trees"))
        .and(body_string_contains("tree[][mode]=100644"))
        .and(body_string_contains("tree[][path]=a.txt"))
        .and(body_string_contains("base_tree=base"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"sha": "tree"})))
        .expect(1)
        .mount(&h.server)
        .await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/agents/git/commits"))
        .and(body_string_contains("parents[]=base"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"sha": "new"})))
        .expect(1)
        .mount(&h.server)
        .await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/agents/git/refs"))
        .and(body_string_contains("ref=refs/heads/fresh"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({})))
        .expect(1)
        .mount(&h.server)
        .await;
    let commit = h
        .service()
        .create_branch_with_files(BranchWithFiles {
            host: GITHUB.to_owned(),
            repository: "acme/agents".to_owned(),
            base: "main".to_owned(),
            branch: "fresh".to_owned(),
            message: "Add".to_owned(),
            files: vec![("a.txt".to_owned(), b"a".to_vec())],
        })
        .await
        .unwrap();
    assert_eq!(commit, "new");
}

#[tokio::test]
async fn review_requests_read_the_search_and_each_head() {
    let h = hosts().await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .and(query_param(
            "q",
            "is:pr is:open review-requested:@me label:\"handover\"",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items": [{
            "number": 5,
            "repository_url": "https://api.github.com/repos/acme/agents",
        }]})))
        .mount(&h.server)
        .await;
    get(
        &h.server,
        "/repos/acme/agents/pulls/5",
        200,
        hosts_pull_request(),
    )
    .await;
    let requests = h.service().review_requests("handover").await.unwrap();
    assert_eq!(requests[0].repository, "acme/agents");
    assert_eq!(requests[0].head, "ordin/rates");
}

#[tokio::test]
async fn review_merge_and_close_send_their_requests() {
    let h = hosts().await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/agents/pulls/5/reviews"))
        .and(body_string_contains("event=REQUEST_CHANGES"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&h.server)
        .await;
    reply(
        &h.server,
        "PUT",
        "/repos/acme/agents/pulls/5/merge",
        200,
        json!({"merged": true}),
    )
    .await;
    reply(
        &h.server,
        "PUT",
        "/repos/acme/agents/pulls/6/merge",
        405,
        json!({"message": "no"}),
    )
    .await;
    reply(
        &h.server,
        "PUT",
        "/repos/acme/agents/pulls/7/merge",
        500,
        json!({"message": "oops"}),
    )
    .await;
    Mock::given(method("PATCH"))
        .and(path("/repos/acme/agents/pulls/5"))
        .and(body_string_contains("state=closed"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&h.server)
        .await;
    let s = h.service();
    GithubAccess::review(
        &**s,
        GITHUB,
        "acme/agents",
        5,
        ReviewVerdict::RequestChanges,
        "No",
    )
    .await
    .unwrap();
    assert!(s.merge(GITHUB, "acme/agents", 5).await.unwrap());
    assert!(!s.merge(GITHUB, "acme/agents", 6).await.unwrap());
    assert_eq!(
        s.merge(GITHUB, "acme/agents", 7).await.unwrap_err().code(),
        "GH_FAILED"
    );
    s.close(GITHUB, "acme/agents", 5).await.unwrap();
}

#[tokio::test]
async fn reviewers_filter_by_login_and_complete_the_person() {
    let h = hosts().await;
    get(
        &h.server,
        "/repos/acme/agents/collaborators",
        200,
        json!([{"login": "SamLee"}, {"login": "other"}]),
    )
    .await;
    get(
        &h.server,
        "/users/SamLee",
        200,
        json!({"name": "Sam Lee", "email": null}),
    )
    .await;
    let people = h
        .service()
        .reviewers(GITHUB, "acme/agents", "sam")
        .await
        .unwrap();
    assert_eq!(people.len(), 1);
    assert_eq!(people[0].name.as_deref(), Some("Sam Lee"));
    assert_eq!(people[0].email, None);
}

#[tokio::test]
async fn files_on_a_branch_are_read_and_updated_without_a_clone() {
    let h = hosts().await;
    get(
        &h.server,
        "/repos/acme/agents/contents/a/b.md",
        200,
        json!({"content": "aGVs\nbG8=\n", "sha": "blob"}),
    )
    .await;
    let (bytes, sha) = h
        .service()
        .file_at(GITHUB, "acme/agents", "main", "a/b.md")
        .await
        .unwrap();
    assert_eq!(bytes, b"hello");
    assert_eq!(sha, "blob");
    Mock::given(method("PUT"))
        .and(path("/repos/acme/agents/contents/a/b.md"))
        .and(body_string_contains("content=aGVsbG8="))
        .and(body_string_contains("sha=blob"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"commit": {"sha": "written"}})),
        )
        .mount(&h.server)
        .await;
    let commit = h
        .service()
        .update_file(
            GITHUB,
            "acme/agents",
            "main",
            "a/b.md",
            b"hello".to_vec(),
            "blob",
            "Up",
        )
        .await
        .unwrap();
    assert_eq!(commit, "written");
}

#[tokio::test]
async fn repository_folders_on_github_list_and_read() {
    let h = hosts().await;
    get(
        &h.server,
        "/repos/acme/agents/contents/flows",
        200,
        json!([{"name": "a.yaml", "type": "file"}, {"name": "dir", "type": "dir"}]),
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/agents/commits"))
        .and(query_param("path", "flows/a.yaml"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "sha": "c1",
            "commit": {"author": {"name": "Sam Lee", "date": "2026-01-02T10:00:00Z"}},
        }])))
        .mount(&h.server)
        .await;
    get(
        &h.server,
        "/repos/acme/agents",
        200,
        json!({"default_branch": "main", "permissions": {}}),
    )
    .await;
    get(
        &h.server,
        "/repos/acme/agents/contents/flows/a.yaml",
        200,
        json!({"content": "eA==", "sha": "b"}),
    )
    .await;
    let folder = RepoFolderRef {
        host: GITHUB.to_owned(),
        repository: "acme/agents".to_owned(),
        folder: "flows".to_owned(),
    };
    let service = h.service();
    let files = service.list_files(&folder, "*.yaml").await.unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].last_changed_by, "Sam Lee");
    let (bytes, commit) = GithubAccess::read_file(&**service, &folder, "a.yaml")
        .await
        .unwrap();
    assert_eq!((bytes.as_slice(), commit.as_str()), (&b"x"[..], "c1"));
    assert_eq!(
        service.file_last_commit(&folder, "a.yaml").await.unwrap(),
        "c1"
    );
    let missing = service.file_last_commit(&folder, "b.yaml").await;
    assert_eq!(missing.unwrap_err().code(), "GH_NOT_FOUND");
}

#[tokio::test]
async fn repository_folders_on_gitlab_read_the_raw_file() {
    let h = hosts().await;
    Mock::given(method("GET"))
        .and(path("/projects/ops%2Fautomation/repository/commits"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "id": "c2", "author_name": "Sam", "committed_date": "2026-01-02T10:00:00Z",
        }])))
        .mount(&h.server)
        .await;
    get(
        &h.server,
        "/projects/ops%2Fautomation",
        200,
        json!({"default_branch": "trunk"}),
    )
    .await;
    Mock::given(method("GET"))
        .and(path(
            "/projects/ops%2Fautomation/repository/files/flows%2Fa.yaml/raw",
        ))
        .and(query_param("ref", "trunk"))
        .respond_with(ResponseTemplate::new(200).set_body_string("raw text"))
        .mount(&h.server)
        .await;
    let folder = RepoFolderRef {
        host: GITLAB.to_owned(),
        repository: "ops/automation".to_owned(),
        folder: "flows".to_owned(),
    };
    let (bytes, commit) = GithubAccess::read_file(&**h.service(), &folder, "a.yaml")
        .await
        .unwrap();
    assert_eq!(bytes, b"raw text");
    assert_eq!(commit, "c2");
}

#[tokio::test]
async fn commit_verified_and_default_branch_read_the_repository() {
    let h = hosts().await;
    get(
        &h.server,
        "/repos/acme/agents/commits/abc",
        200,
        json!({"commit": {"verification": {"verified": true}}}),
    )
    .await;
    get(
        &h.server,
        "/repos/acme/agents",
        200,
        json!({"default_branch": "trunk"}),
    )
    .await;
    let service = h.service();
    assert!(service
        .commit_verified(GITHUB, "acme/agents", "abc")
        .await
        .unwrap());
    assert_eq!(
        GithubAccess::default_branch(&**service, GITHUB, "acme/agents")
            .await
            .unwrap(),
        "trunk"
    );
}
