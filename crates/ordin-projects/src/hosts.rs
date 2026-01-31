//! Repository hosts in process (`GithubAccess`; Internal Logic 14).

use base64::Engine as _;
use ordin_types::domain::{PullRequestRef, PullRequestState};
use ordin_types::time::Timestamp;
use serde_json::Value;

use crate::error::GithubError;
use crate::host_cli::{failure_of, ApiCall, Field, HostKind, HostReply};
use crate::service::ProjectService;
use crate::types::{
    BranchReviewRules, BranchReviewStatus, BranchWithFiles, GitCredential, GitToken, GithubAccess,
    GithubPerson, OpenPullRequest, PullRequestView, RepoFile, RepoFolderRef, RepoPermission,
    ReviewComment, ReviewDecision, ReviewRequest, ReviewVerdict,
};

/// Percent-encodes everything but the unreserved characters.
pub(crate) fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// As [`encode`], keeping `/` between path segments.
fn encode_path(text: &str) -> String {
    text.split('/').map(encode).collect::<Vec<_>>().join("/")
}

fn folder_path(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.to_owned()
    } else {
        format!("{}/{name}", folder.trim_end_matches('/'))
    }
}

fn unreadable() -> GithubError {
    GithubError::Other("The host's reply could not be read.".to_owned())
}

fn text_of(value: &Value) -> Result<String, GithubError> {
    value.as_str().map(str::to_owned).ok_or_else(unreadable)
}

fn number_of(value: &Value) -> Result<u64, GithubError> {
    value.as_u64().ok_or_else(unreadable)
}

fn time_of(value: &Value) -> Result<Timestamp, GithubError> {
    let text = value.as_str().ok_or_else(unreadable)?;
    let parsed = chrono::DateTime::parse_from_rfc3339(text).map_err(|_| unreadable())?;
    Timestamp::from_unix_millis(parsed.timestamp_millis()).map_err(|_| unreadable())
}

/// `*` and `?` only (*chosen here*, Internal Logic 14.4).
pub(crate) fn glob_matches(glob: &str, name: &str) -> bool {
    fn matches(glob: &[char], name: &[char]) -> bool {
        match glob.split_first() {
            None => name.is_empty(),
            Some(('*', rest)) => (0..=name.len()).any(|skip| matches(rest, &name[skip..])),
            Some(('?', rest)) => !name.is_empty() && matches(rest, &name[1..]),
            Some((c, rest)) => name.first() == Some(c) && matches(rest, &name[1..]),
        }
    }
    let (glob, name): (Vec<char>, Vec<char>) = (glob.chars().collect(), name.chars().collect());
    matches(&glob, &name)
}

fn pull_request_state(body: &Value) -> PullRequestState {
    if body["merged"].as_bool() == Some(true) || !body["merged_at"].is_null() {
        PullRequestState::Merged
    } else if body["state"].as_str() == Some("open") {
        PullRequestState::Open
    } else {
        PullRequestState::Closed
    }
}

/// The latest review per reviewer decides (Internal Logic 14.3).
fn review_of(reviews: &[Value]) -> Result<(ReviewDecision, Vec<ReviewComment>), GithubError> {
    let mut latest: Vec<(&str, &str)> = Vec::new();
    let mut requests = Vec::new();
    for review in reviews {
        let reviewer = review["user"]["login"].as_str().unwrap_or_default();
        let state = review["state"].as_str().unwrap_or_default();
        latest.retain(|(who, _)| *who != reviewer);
        latest.push((reviewer, state));
    }
    for review in reviews {
        let reviewer = review["user"]["login"].as_str().unwrap_or_default();
        if review["state"].as_str() == Some("CHANGES_REQUESTED")
            && latest.contains(&(reviewer, "CHANGES_REQUESTED"))
        {
            requests.push(ReviewComment {
                reviewer: reviewer.to_owned(),
                body: review["body"].as_str().unwrap_or_default().to_owned(),
                at: time_of(&review["submitted_at"])?,
            });
        }
    }
    let decision = if latest.iter().any(|(_, s)| *s == "CHANGES_REQUESTED") {
        ReviewDecision::ChangesRequested
    } else if latest.iter().any(|(_, s)| *s == "APPROVED") {
        ReviewDecision::Approved
    } else {
        ReviewDecision::Pending
    };
    Ok((decision, requests))
}

/// GitLab access levels (ADR-110 14.1).
fn gitlab_permission(body: &Value) -> RepoPermission {
    let project = body["permissions"]["project_access"]["access_level"]
        .as_u64()
        .unwrap_or(0);
    let group = body["permissions"]["group_access"]["access_level"]
        .as_u64()
        .unwrap_or(0);
    match project.max(group) {
        50.. => RepoPermission::Admin,
        40..=49 => RepoPermission::Maintain,
        30..=39 => RepoPermission::Write,
        20..=29 => RepoPermission::Read,
        _ => RepoPermission::None,
    }
}

/// GitHub's `permissions` object: the highest true flag.
fn github_permission(body: &Value) -> RepoPermission {
    let flags = &body["permissions"];
    let on = |name: &str| flags[name].as_bool() == Some(true);
    if on("admin") {
        RepoPermission::Admin
    } else if on("maintain") {
        RepoPermission::Maintain
    } else if on("push") {
        RepoPermission::Write
    } else if on("triage") {
        RepoPermission::Triage
    } else if on("pull") {
        RepoPermission::Read
    } else {
        RepoPermission::None
    }
}

/// ADR-110 14.6 GitHub step 1: the rules that apply to the branch. `None` means the rules name no
/// review. A `pull_request` rule whose count cannot be read is uncertain (C-92), so `CannotTell`.
fn review_from_rules(body: &Value) -> Option<BranchReviewStatus> {
    let Some(rules) = body.as_array() else {
        return Some(BranchReviewStatus::CannotTell);
    };
    let mut approvals = 0u32;
    let mut code_owners = false;
    let mut uncertain = false;
    for rule in rules.iter().filter(|r| r["type"] == "pull_request") {
        let parameters = &rule["parameters"];
        match parameters["required_approving_review_count"].as_u64() {
            Some(count) if count >= 1 => {
                approvals = approvals.max(u32::try_from(count).unwrap_or(u32::MAX));
                code_owners |= parameters["require_code_owner_review"].as_bool() == Some(true);
            }
            Some(_) => {}
            None => uncertain = true,
        }
    }
    if approvals >= 1 {
        Some(BranchReviewStatus::Required {
            required_approvals: approvals,
            code_owner_review_required: code_owners,
        })
    } else if uncertain {
        Some(BranchReviewStatus::CannotTell)
    } else {
        None
    }
}

/// ADR-110 14.6 GitLab step 3: the largest count among the rules covering the branch.
fn largest_gitlab_approvals(rules: &[Value], branch: &str) -> u32 {
    rules
        .iter()
        .filter(|rule| {
            rule["applies_to_all_protected_branches"].as_bool() == Some(true)
                || rule["protected_branches"]
                    .as_array()
                    .is_some_and(|list| list.iter().any(|b| b["name"] == branch))
        })
        .filter_map(|rule| rule["approvals_required"].as_u64())
        .map(|count| u32::try_from(count).unwrap_or(u32::MAX))
        .max()
        .unwrap_or(0)
}

/// `true` when a person or a group may push to the protected branch, or anyone by level.
fn gitlab_push_open(body: &Value) -> bool {
    body["push_access_levels"].as_array().is_none_or(|levels| {
        levels.iter().any(|level| {
            level["access_level"].as_u64().unwrap_or(0) > 0
                || !level["user_id"].is_null()
                || !level["group_id"].is_null()
        })
    })
}

fn review_answer(status: &BranchReviewStatus) -> &'static str {
    match status {
        BranchReviewStatus::Required { .. } => "required",
        BranchReviewStatus::NotRequired => "not_required",
        BranchReviewStatus::CannotTell => "cannot_tell",
    }
}

impl ProjectService {
    /// Internal Logic 14.1: which kind of host this is.
    pub(crate) fn host_kind(&self, host: &str) -> Result<HostKind, GithubError> {
        let host = host.to_ascii_lowercase();
        if host == "github.com" {
            Ok(HostKind::Github)
        } else if host == "gitlab.com"
            || host == self.current_settings().gitlab_host.to_ascii_lowercase()
        {
            Ok(HostKind::Gitlab)
        } else {
            Err(GithubError::HostNotSupported)
        }
    }

    /// Every method but the ones ADR-110 14.1 names is GitHub-only.
    fn github_only(&self, host: &str) -> Result<(), GithubError> {
        match self.host_kind(host)? {
            HostKind::Github => Ok(()),
            HostKind::Gitlab => Err(GithubError::HostNotSupported),
        }
    }

    async fn github(&self, call: ApiCall) -> Result<HostReply, GithubError> {
        self.api_ok(HostKind::Github, "github.com", call).await
    }

    async fn github_json(&self, call: ApiCall) -> Result<Value, GithubError> {
        self.github(call).await?.json()
    }

    async fn gitlab_json(&self, host: &str, call: ApiCall) -> Result<Value, GithubError> {
        self.api_ok(HostKind::Gitlab, host, call).await?.json()
    }

    async fn github_review_status(
        &self,
        repository: &str,
        branch: &str,
    ) -> Result<BranchReviewStatus, GithubError> {
        let operation = "branch_review_status";
        let branch = encode_path(branch);
        let call = |path: String| ApiCall::get(operation, path);
        let host = "github.com";
        let rules = self
            .api(
                HostKind::Github,
                host,
                call(format!("repos/{repository}/rules/branches/{branch}")),
            )
            .await?;
        match rules.status {
            404 => return Err(GithubError::NotFound),
            403 => return Ok(BranchReviewStatus::CannotTell),
            status if !(200..300).contains(&status) => return Err(failure_of(&rules)),
            _ => {}
        }
        if let Some(answer) = review_from_rules(&rules.json()?) {
            return Ok(answer);
        }
        let found = self
            .api(
                HostKind::Github,
                host,
                call(format!("repos/{repository}/branches/{branch}")),
            )
            .await?;
        match found.status {
            404 => return Err(GithubError::NotFound),
            403 => return Ok(BranchReviewStatus::CannotTell),
            status if !(200..300).contains(&status) => return Err(failure_of(&found)),
            _ => {}
        }
        match found.json()?["protected"].as_bool() {
            Some(false) => return Ok(BranchReviewStatus::NotRequired),
            Some(true) => {}
            None => return Ok(BranchReviewStatus::CannotTell),
        }
        let reviews = self
            .api(
                HostKind::Github,
                host,
                call(format!(
                    "repos/{repository}/branches/{branch}/protection/required_pull_request_reviews"
                )),
            )
            .await?;
        match reviews.status {
            403 | 404 => Ok(BranchReviewStatus::CannotTell),
            status if !(200..300).contains(&status) => Err(failure_of(&reviews)),
            _ => {
                let body = reviews.json()?;
                match body["required_approving_review_count"].as_u64() {
                    Some(0) => Ok(BranchReviewStatus::NotRequired),
                    Some(count) => Ok(BranchReviewStatus::Required {
                        required_approvals: u32::try_from(count).unwrap_or(u32::MAX),
                        code_owner_review_required: body["require_code_owner_reviews"].as_bool()
                            == Some(true),
                    }),
                    None => Ok(BranchReviewStatus::CannotTell),
                }
            }
        }
    }

    async fn gitlab_review_status(
        &self,
        host: &str,
        repository: &str,
        branch: &str,
    ) -> Result<BranchReviewStatus, GithubError> {
        let operation = "branch_review_status";
        let project = encode(repository);
        let call = |path: String| ApiCall::get(operation, path);
        let found = self
            .api(HostKind::Gitlab, host, call(format!("projects/{project}")))
            .await?;
        match found.status {
            404 => return Err(GithubError::NotFound),
            403 => return Ok(BranchReviewStatus::CannotTell),
            status if !(200..300).contains(&status) => return Err(failure_of(&found)),
            _ => {}
        }
        let protection = self
            .api(
                HostKind::Gitlab,
                host,
                call(format!(
                    "projects/{project}/protected_branches/{}",
                    encode(branch)
                )),
            )
            .await?;
        match protection.status {
            404 => return Ok(BranchReviewStatus::NotRequired),
            403 => return Ok(BranchReviewStatus::CannotTell),
            status if !(200..300).contains(&status) => return Err(failure_of(&protection)),
            _ => {}
        }
        let protected = protection.json()?;
        if gitlab_push_open(&protected) {
            return Ok(BranchReviewStatus::NotRequired);
        }
        let rules = self
            .api(
                HostKind::Gitlab,
                host,
                call(format!("projects/{project}/approval_rules")),
            )
            .await?;
        match rules.status {
            403 | 404 => Ok(BranchReviewStatus::CannotTell),
            status if !(200..300).contains(&status) => Err(failure_of(&rules)),
            _ => {
                let body = rules.json()?;
                let Some(list) = body.as_array() else {
                    return Ok(BranchReviewStatus::CannotTell);
                };
                let approvals = largest_gitlab_approvals(list, branch);
                if approvals >= 1 {
                    Ok(BranchReviewStatus::Required {
                        required_approvals: approvals,
                        code_owner_review_required: protected["code_owner_approval_required"]
                            .as_bool()
                            == Some(true),
                    })
                } else {
                    Ok(BranchReviewStatus::NotRequired)
                }
            }
        }
    }

    /// The last commit touching a file of a folder: (id, author, date).
    async fn last_commit(
        &self,
        kind: HostKind,
        folder: &RepoFolderRef,
        name: &str,
    ) -> Result<(String, String, Timestamp), GithubError> {
        let path = encode(&folder_path(&folder.folder, name));
        let operation = "file_last_commit";
        match kind {
            HostKind::Github => {
                let body = self
                    .github_json(ApiCall::get(
                        operation,
                        format!("repos/{}/commits?path={path}&per_page=1", folder.repository),
                    ))
                    .await?;
                let first = body.get(0).ok_or(GithubError::NotFound)?;
                Ok((
                    text_of(&first["sha"])?,
                    text_of(&first["commit"]["author"]["name"])?,
                    time_of(&first["commit"]["author"]["date"])?,
                ))
            }
            HostKind::Gitlab => {
                let body = self
                    .gitlab_json(
                        &folder.host,
                        ApiCall::get(
                            operation,
                            format!(
                                "projects/{}/repository/commits?path={path}&per_page=1",
                                encode(&folder.repository)
                            ),
                        ),
                    )
                    .await?;
                let first = body.get(0).ok_or(GithubError::NotFound)?;
                Ok((
                    text_of(&first["id"])?,
                    text_of(&first["author_name"])?,
                    time_of(&first["committed_date"])?,
                ))
            }
        }
    }

    async fn github_file_at(
        &self,
        repository: &str,
        git_ref: &str,
        path: &str,
    ) -> Result<(Vec<u8>, String), GithubError> {
        let body = self
            .github_json(ApiCall::get(
                "file_at",
                format!(
                    "repos/{repository}/contents/{}?ref={}",
                    encode_path(path),
                    encode(git_ref)
                ),
            ))
            .await?;
        let content = text_of(&body["content"])?.replace(['\n', '\r'], "");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(content)
            .map_err(|_| unreadable())?;
        Ok((bytes, text_of(&body["sha"])?))
    }
}

#[async_trait::async_trait]
impl GithubAccess for ProjectService {
    async fn git_credentials(&self, host: &str) -> Result<GitCredential, GithubError> {
        let kind = self.host_kind(host)?;
        let token = self.host_token(kind).await?;
        Ok(GitCredential {
            username: kind.git_username().to_owned(),
            token: GitToken::new(token),
        })
    }

    // spec: ordin-desktop/components/ProjectService#ADR-110
    async fn repository_permission(
        &self,
        host: &str,
        repository: &str,
    ) -> Result<RepoPermission, GithubError> {
        let operation = "repository_permission";
        match self.host_kind(host)? {
            HostKind::Github => {
                let call = ApiCall::get(operation, format!("repos/{repository}"));
                Ok(github_permission(&self.github_json(call).await?))
            }
            HostKind::Gitlab => {
                let call = ApiCall::get(operation, format!("projects/{}", encode(repository)));
                Ok(gitlab_permission(&self.gitlab_json(host, call).await?))
            }
        }
    }

    async fn default_branch(&self, host: &str, repository: &str) -> Result<String, GithubError> {
        self.github_only(host)?;
        let call = ApiCall::get("default_branch", format!("repos/{repository}"));
        text_of(&self.github_json(call).await?["default_branch"])
    }

    async fn commit_verified(
        &self,
        host: &str,
        repository: &str,
        sha: &str,
    ) -> Result<bool, GithubError> {
        self.github_only(host)?;
        let call = ApiCall::get(
            "commit_verified",
            format!("repos/{repository}/commits/{sha}"),
        );
        Ok(
            self.github_json(call).await?["commit"]["verification"]["verified"].as_bool()
                == Some(true),
        )
    }

    /// Derived from `branch_review_status`, so a 403 or a 404 never reads as "no review".
    // spec: ordin-desktop/components/ProjectService#ADR-110
    async fn branch_review_rules(
        &self,
        host: &str,
        repository: &str,
        branch: &str,
    ) -> Result<BranchReviewRules, GithubError> {
        Ok(
            match self.branch_review_status(host, repository, branch).await? {
                BranchReviewStatus::Required {
                    required_approvals,
                    code_owner_review_required,
                } => BranchReviewRules {
                    review_required: true,
                    code_owner_review_required,
                    required_approvals,
                },
                BranchReviewStatus::NotRequired => BranchReviewRules {
                    review_required: false,
                    code_owner_review_required: false,
                    required_approvals: 0,
                },
                BranchReviewStatus::CannotTell => BranchReviewRules {
                    review_required: true,
                    code_owner_review_required: false,
                    required_approvals: 0,
                },
            },
        )
    }

    async fn pull_request(
        &self,
        host: &str,
        repository: &str,
        number: u64,
    ) -> Result<PullRequestView, GithubError> {
        self.github_only(host)?;
        let operation = "pull_request";
        let body = self
            .github_json(ApiCall::get(
                operation,
                format!("repos/{repository}/pulls/{number}"),
            ))
            .await?;
        let reviews = self
            .github_json(ApiCall::get(
                operation,
                format!("repos/{repository}/pulls/{number}/reviews"),
            ))
            .await?;
        let (review, change_requests) =
            review_of(reviews.as_array().map(Vec::as_slice).unwrap_or_default())?;
        let state = pull_request_state(&body);
        Ok(PullRequestView {
            number,
            state,
            review,
            change_requests,
            merge_commit: (state == PullRequestState::Merged)
                .then(|| body["merge_commit_sha"].as_str().map(str::to_owned))
                .flatten(),
            head: text_of(&body["head"]["ref"])?,
            base: text_of(&body["base"]["ref"])?,
            url: text_of(&body["html_url"])?,
        })
    }

    async fn open_pull_request(&self, req: OpenPullRequest) -> Result<PullRequestRef, GithubError> {
        self.github_only(&req.host)?;
        let operation = "open_pull_request";
        let repository = &req.repository;
        let raw = |key: &str, value: &str| Field(key.to_owned(), value.to_owned());
        let body = self
            .github_json(ApiCall::with(
                operation,
                "POST",
                format!("repos/{repository}/pulls"),
                vec![
                    raw("title", &req.title),
                    raw("head", &req.head),
                    raw("base", &req.base),
                    raw("body", &req.body),
                ],
            ))
            .await?;
        let number = number_of(&body["number"])?;
        if !req.labels.is_empty() {
            self.github(ApiCall::with(
                operation,
                "POST",
                format!("repos/{repository}/issues/{number}/labels"),
                req.labels.iter().map(|l| raw("labels[]", l)).collect(),
            ))
            .await?;
        }
        if !req.reviewers.is_empty() {
            self.github(ApiCall::with(
                operation,
                "POST",
                format!("repos/{repository}/pulls/{number}/requested_reviewers"),
                req.reviewers
                    .iter()
                    .map(|r| raw("reviewers[]", r))
                    .collect(),
            ))
            .await?;
        }
        Ok(PullRequestRef {
            number,
            url: text_of(&body["html_url"])?,
            state: PullRequestState::Open,
        })
    }

    async fn pull_request_for_branch(
        &self,
        host: &str,
        repository: &str,
        head: &str,
    ) -> Result<Option<PullRequestRef>, GithubError> {
        self.github_only(host)?;
        let owner = repository.split('/').next().unwrap_or_default();
        let body = self
            .github_json(ApiCall::get(
                "pull_request_for_branch",
                format!(
                    "repos/{repository}/pulls?head={}&state=all&per_page=1&sort=created&direction=desc",
                    encode(&format!("{owner}:{head}"))
                ),
            ))
            .await?;
        body.get(0)
            .map(|first| {
                Ok(PullRequestRef {
                    number: number_of(&first["number"])?,
                    url: text_of(&first["html_url"])?,
                    state: pull_request_state(first),
                })
            })
            .transpose()
    }

    // File contents are `-f` fields, which limits a file to what one process argument can hold
    // (128 KiB on Linux); no larger route is stated.
    async fn create_branch_with_files(&self, req: BranchWithFiles) -> Result<String, GithubError> {
        self.github_only(&req.host)?;
        let operation = "create_branch_with_files";
        let repository = &req.repository;
        let raw = |key: &str, value: &str| Field(key.to_owned(), value.to_owned());
        let base = self
            .github_json(ApiCall::get(
                operation,
                format!(
                    "repos/{repository}/git/ref/heads/{}",
                    encode_path(&req.base)
                ),
            ))
            .await?;
        let base_commit = text_of(&base["object"]["sha"])?;
        let mut fields = vec![raw("base_tree", &base_commit)];
        for (path, bytes) in &req.files {
            let content = std::str::from_utf8(bytes)
                .map_err(|_| GithubError::Other("A file is not text.".to_owned()))?;
            fields.push(raw("tree[][path]", path));
            fields.push(raw("tree[][mode]", "100644"));
            fields.push(raw("tree[][type]", "blob"));
            fields.push(raw("tree[][content]", content));
        }
        let tree = self
            .github_json(ApiCall::with(
                operation,
                "POST",
                format!("repos/{repository}/git/trees"),
                fields,
            ))
            .await?;
        let commit = self
            .github_json(ApiCall::with(
                operation,
                "POST",
                format!("repos/{repository}/git/commits"),
                vec![
                    raw("message", &req.message),
                    raw("tree", &text_of(&tree["sha"])?),
                    raw("parents[]", &base_commit),
                ],
            ))
            .await?;
        let sha = text_of(&commit["sha"])?;
        let created = self
            .api(
                HostKind::Github,
                "github.com",
                ApiCall::with(
                    operation,
                    "POST",
                    format!("repos/{repository}/git/refs"),
                    vec![
                        raw("ref", &format!("refs/heads/{}", req.branch)),
                        raw("sha", &sha),
                    ],
                ),
            )
            .await?;
        match created.status {
            422 => Err(GithubError::Conflict {
                current_sha: String::new(),
            }),
            _ if created.success() => Ok(sha),
            _ => Err(failure_of(&created)),
        }
    }

    async fn review_requests(&self, label: &str) -> Result<Vec<ReviewRequest>, GithubError> {
        let operation = "review_requests";
        let query = encode(&format!(
            "is:pr is:open review-requested:@me label:\"{label}\""
        ));
        let body = self
            .github_json(ApiCall::get(operation, format!("search/issues?q={query}")))
            .await?;
        let mut out = Vec::new();
        for item in body["items"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let number = number_of(&item["number"])?;
            let repository = text_of(&item["repository_url"])?
                .rsplit("/repos/")
                .next()
                .unwrap_or_default()
                .to_owned();
            let pull = self
                .github_json(ApiCall::get(
                    operation,
                    format!("repos/{repository}/pulls/{number}"),
                ))
                .await?;
            out.push(ReviewRequest {
                host: "github.com".to_owned(),
                repository,
                number,
                head: text_of(&pull["head"]["ref"])?,
            });
        }
        Ok(out)
    }

    async fn review(
        &self,
        host: &str,
        repository: &str,
        number: u64,
        verdict: ReviewVerdict,
        body: &str,
    ) -> Result<(), GithubError> {
        self.github_only(host)?;
        let event = match verdict {
            ReviewVerdict::Approve => "APPROVE",
            ReviewVerdict::RequestChanges => "REQUEST_CHANGES",
        };
        self.github(ApiCall::with(
            "review",
            "POST",
            format!("repos/{repository}/pulls/{number}/reviews"),
            vec![
                Field("event".to_owned(), event.to_owned()),
                Field("body".to_owned(), body.to_owned()),
            ],
        ))
        .await
        .map(|_| ())
    }

    async fn merge(&self, host: &str, repository: &str, number: u64) -> Result<bool, GithubError> {
        self.github_only(host)?;
        let reply = self
            .api(
                HostKind::Github,
                host,
                ApiCall::with(
                    "merge",
                    "PUT",
                    format!("repos/{repository}/pulls/{number}/merge"),
                    Vec::new(),
                ),
            )
            .await?;
        match reply.status {
            405 => Ok(false),
            _ if reply.success() => Ok(reply.json()?["merged"].as_bool() == Some(true)),
            _ => Err(failure_of(&reply)),
        }
    }

    async fn close(&self, host: &str, repository: &str, number: u64) -> Result<(), GithubError> {
        self.github_only(host)?;
        self.github(ApiCall::with(
            "close",
            "PATCH",
            format!("repos/{repository}/pulls/{number}"),
            vec![Field("state".to_owned(), "closed".to_owned())],
        ))
        .await
        .map(|_| ())
    }

    async fn reviewers(
        &self,
        host: &str,
        repository: &str,
        query: &str,
    ) -> Result<Vec<GithubPerson>, GithubError> {
        self.github_only(host)?;
        let operation = "reviewers";
        let body = self
            .github_json(ApiCall::get(
                operation,
                format!("repos/{repository}/collaborators?per_page=100"),
            ))
            .await?;
        let wanted = query.to_lowercase();
        let mut out = Vec::new();
        for person in body.as_array().map(Vec::as_slice).unwrap_or_default() {
            if out.len() >= crate::MAX_REVIEWERS {
                break;
            }
            let login = text_of(&person["login"])?;
            if login.to_lowercase().contains(&wanted) {
                let user = self
                    .github_json(ApiCall::get(operation, format!("users/{login}")))
                    .await?;
                out.push(GithubPerson {
                    login,
                    name: user["name"].as_str().map(str::to_owned),
                    email: user["email"].as_str().map(str::to_owned),
                });
            }
        }
        Ok(out)
    }

    async fn file_at(
        &self,
        host: &str,
        repository: &str,
        git_ref: &str,
        path: &str,
    ) -> Result<(Vec<u8>, String), GithubError> {
        self.github_only(host)?;
        self.github_file_at(repository, git_ref, path).await
    }

    async fn update_file(
        &self,
        host: &str,
        repository: &str,
        branch: &str,
        path: &str,
        bytes: Vec<u8>,
        expected_sha: &str,
        message: &str,
    ) -> Result<String, GithubError> {
        self.github_only(host)?;
        let raw = |key: &str, value: &str| Field(key.to_owned(), value.to_owned());
        let content = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let reply = self
            .api(
                HostKind::Github,
                host,
                ApiCall::with(
                    "update_file",
                    "PUT",
                    format!("repos/{repository}/contents/{}", encode_path(path)),
                    vec![
                        raw("message", message),
                        raw("content", &content),
                        raw("sha", expected_sha),
                        raw("branch", branch),
                    ],
                ),
            )
            .await?;
        let names_sha = String::from_utf8_lossy(&reply.body).contains("sha");
        if reply.status == 409 || (reply.status == 422 && names_sha) {
            let (_, current_sha) = self.github_file_at(repository, branch, path).await?;
            return Err(GithubError::Conflict { current_sha });
        }
        if !reply.success() {
            return Err(failure_of(&reply));
        }
        text_of(&reply.json()?["commit"]["sha"])
    }

    async fn list_files(
        &self,
        folder: &RepoFolderRef,
        glob: &str,
    ) -> Result<Vec<RepoFile>, GithubError> {
        let kind = self.host_kind(&folder.host)?;
        let operation = "list_files";
        let names: Vec<String> = match kind {
            HostKind::Github => {
                let body = self
                    .github_json(ApiCall::get(
                        operation,
                        format!(
                            "repos/{}/contents/{}",
                            folder.repository,
                            encode_path(&folder.folder)
                        ),
                    ))
                    .await?;
                body.as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                    .iter()
                    .filter(|item| item["type"] == "file")
                    .filter_map(|item| item["name"].as_str().map(str::to_owned))
                    .collect()
            }
            HostKind::Gitlab => {
                let body = self
                    .gitlab_json(
                        &folder.host,
                        ApiCall::get(
                            operation,
                            format!(
                                "projects/{}/repository/tree?path={}",
                                encode(&folder.repository),
                                encode(&folder.folder)
                            ),
                        ),
                    )
                    .await?;
                body.as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                    .iter()
                    .filter(|item| item["type"] == "blob")
                    .filter_map(|item| item["name"].as_str().map(str::to_owned))
                    .collect()
            }
        };
        let mut out = Vec::new();
        for name in names
            .into_iter()
            .filter(|name| glob_matches(glob, name))
            .take(crate::MAX_LISTED_FILES)
        {
            let (commit, last_changed_by, last_changed_at) =
                self.last_commit(kind, folder, &name).await?;
            out.push(RepoFile {
                name,
                last_changed_by,
                last_changed_at,
                commit,
            });
        }
        Ok(out)
    }

    async fn read_file(
        &self,
        folder: &RepoFolderRef,
        name: &str,
    ) -> Result<(Vec<u8>, String), GithubError> {
        let kind = self.host_kind(&folder.host)?;
        let path = folder_path(&folder.folder, name);
        let (commit, _, _) = self.last_commit(kind, folder, name).await?;
        let operation = "read_file";
        let bytes = match kind {
            HostKind::Github => {
                let info = self
                    .github_json(ApiCall::get(
                        operation,
                        format!("repos/{}", folder.repository),
                    ))
                    .await?;
                let branch = text_of(&info["default_branch"])?;
                self.github_file_at(&folder.repository, &branch, &path)
                    .await?
                    .0
            }
            HostKind::Gitlab => {
                let project = encode(&folder.repository);
                let info = self
                    .gitlab_json(
                        &folder.host,
                        ApiCall::get(operation, format!("projects/{project}")),
                    )
                    .await?;
                let branch = text_of(&info["default_branch"])?;
                self.api_ok(
                    HostKind::Gitlab,
                    &folder.host,
                    ApiCall::get(
                        operation,
                        format!(
                            "projects/{project}/repository/files/{}/raw?ref={}",
                            encode(&path),
                            encode(&branch)
                        ),
                    ),
                )
                .await?
                .body
            }
        };
        Ok((bytes, commit))
    }

    async fn file_last_commit(
        &self,
        folder: &RepoFolderRef,
        name: &str,
    ) -> Result<String, GithubError> {
        let kind = self.host_kind(&folder.host)?;
        Ok(self.last_commit(kind, folder, name).await?.0)
    }

    /// ADR-110 14.6. Read endpoints only, the person's own token, no answer cached.
    // spec: ordin-desktop/components/ProjectService#ADR-110
    // spec: ordin-desktop/components/ProjectService#C-92
    async fn branch_review_status(
        &self,
        host: &str,
        repository: &str,
        branch: &str,
    ) -> Result<BranchReviewStatus, GithubError> {
        let kind = self.host_kind(host)?;
        let answer = match kind {
            HostKind::Github => self.github_review_status(repository, branch).await?,
            HostKind::Gitlab => self.gitlab_review_status(host, repository, branch).await?,
        };
        crate::log::branch_review_answer(kind.name(), review_answer(&answer));
        Ok(answer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_glob_knows_star_and_question_mark_only() {
        assert!(glob_matches("*.workflow.yaml", "a.workflow.yaml"));
        assert!(glob_matches("a?c", "abc"));
        assert!(!glob_matches("a?c", "ac"));
        assert!(!glob_matches("*.md", "a.txt"));
        assert!(glob_matches("[x", "[x"));
    }

    #[test]
    fn github_permissions_read_the_highest_flag() {
        for (flags, expected) in [
            (json!({"admin": true}), RepoPermission::Admin),
            (json!({"maintain": true}), RepoPermission::Maintain),
            (json!({"push": true, "pull": true}), RepoPermission::Write),
            (json!({"triage": true}), RepoPermission::Triage),
            (json!({"pull": true}), RepoPermission::Read),
            (json!({}), RepoPermission::None),
        ] {
            assert_eq!(
                github_permission(&json!({ "permissions": flags })),
                expected
            );
        }
    }

    #[test]
    fn gitlab_permissions_take_the_higher_access_level() {
        for (project, group, expected) in [
            (50, 0, RepoPermission::Admin),
            (0, 40, RepoPermission::Maintain),
            (30, 20, RepoPermission::Write),
            (20, 0, RepoPermission::Read),
            (10, 0, RepoPermission::None),
        ] {
            let body = json!({"permissions": {
                "project_access": {"access_level": project},
                "group_access": {"access_level": group},
            }});
            assert_eq!(gitlab_permission(&body), expected);
        }
        assert_eq!(gitlab_permission(&json!({})), RepoPermission::None);
    }

    #[test]
    fn the_latest_review_of_each_reviewer_decides() {
        let at = "2026-01-02T10:00:00Z";
        let change = json!({"user": {"login": "a"}, "state": "CHANGES_REQUESTED", "body": "no", "submitted_at": at});
        let approve =
            json!({"user": {"login": "a"}, "state": "APPROVED", "body": "", "submitted_at": at});
        let (decision, requests) = review_of(&[change.clone(), approve.clone()]).unwrap();
        assert_eq!(decision, ReviewDecision::Approved);
        assert!(requests.is_empty());
        let (decision, requests) = review_of(&[approve, change]).unwrap();
        assert_eq!(decision, ReviewDecision::ChangesRequested);
        assert_eq!(requests[0].body, "no");
        assert_eq!(review_of(&[]).unwrap().0, ReviewDecision::Pending);
    }

    #[test]
    fn a_ruleset_with_an_unreadable_count_is_an_uncertain_answer() {
        let unreadable_rule = json!([{"type": "pull_request", "parameters": {}}]);
        assert_eq!(
            review_from_rules(&unreadable_rule),
            Some(BranchReviewStatus::CannotTell)
        );
        assert_eq!(
            review_from_rules(&json!({"not": "a list"})),
            Some(BranchReviewStatus::CannotTell)
        );
        assert_eq!(review_from_rules(&json!([{"type": "deletion"}])), None);
        let zero =
            json!([{"type": "pull_request", "parameters": {"required_approving_review_count": 0}}]);
        assert_eq!(review_from_rules(&zero), None);
    }

    #[test]
    fn segments_are_encoded() {
        assert_eq!(encode("ops/automation"), "ops%2Fautomation");
        assert_eq!(encode_path("a b/c"), "a%20b/c");
        assert_eq!(folder_path("", "x"), "x");
        assert_eq!(folder_path("a/", "x"), "a/x");
    }
}
