//! The public types of ProjectService.md that are not 2C's. 2C's own types (`Project`, `CodingTask`,
//! `ChangeSet` and their parts) are re-exported from `ordin_types::domain` unchanged.

use std::collections::BTreeMap;
use std::path::PathBuf;

use ordin_types::domain::{ChangeSet, CodingTask, PullRequestRef, PullRequestState, RunOrigin};
use ordin_types::time::Timestamp;
use ordin_types::{AgentInstanceId, AttemptGroupId, FileVersionId, PlaceId, ProjectId};
use serde::{Deserialize, Serialize};

use crate::error::GithubError;

/// Settings injected by the program (B7 contract D-4).
#[derive(Debug, Clone)]
pub struct ProjectSettings {
    /// Keyed by `go`, `rust`, `java`, `typescript`. Built by the program from the profile's
    /// `language_servers[]` entries `{ language, name, version, platforms }` (2D-2, K12): every entry
    /// of a language is approved and the first in profile order is the default; a language with no
    /// profile entry takes the entries of the build's `language-servers.json` table (same shape,
    /// pinned at release) (SA-CLOSE-02).
    pub language_servers: BTreeMap<String, ApprovedServers>,
    /// `engineering.github.always_approved_kinds[]` (2D-2): the ceiling.
    pub always_approved_kinds: Vec<String>,
    /// `engineering.coding_tasks.max_attempts` (2D-2; default 3, ≥ 1; ADR-053).
    pub max_attempts: u32,
    /// The company's GitLab host name (ADR-051), from the profile key `gitlab.host` (2D-2, K10);
    /// "gitlab.com" when the key is absent (SA-CLOSE-01). A host name only: no scheme, port or path.
    pub gitlab_host: String,
    pub profile_version: String,
}

#[derive(Debug, Clone)]
pub struct ApprovedServers {
    pub approved: Vec<ServerPin>,
    pub default: String,
}

#[derive(Debug, Clone)]
pub struct ServerPin {
    pub name: String,
    pub version: String,
}

/// Mine (owned here; implemented by the program over `SetupAssistant::ensure_language_server(language,
/// &pin.name, &pin.version) -> LanguageServerLaunch`, Setup Assistant SA-RECON-05): installs a
/// pinned language server or its toolchain on first use and returns how to start it.
#[async_trait::async_trait]
pub trait ServerInstaller: Send + Sync {
    async fn ensure(&self, language: &str, server: &ServerPin) -> Result<ServerLaunch, String>;
}

pub struct ServerLaunch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// Mine: an unjournalled stream to the screens for terminal bytes and LSP messages (terminal content
/// is never journalled, ADR-022). Implemented by the desktop program over the Command Bridge's
/// `ScreenStreams::send(stream, StreamKind::Terminal | StreamKind::Lsp, payload)` and `close`.
pub trait StreamSink: Send + Sync {
    fn send(&self, stream: StreamId, payload: StreamPayload);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StreamId(pub uuid::Uuid);

#[derive(Debug, Clone)]
pub enum StreamPayload {
    TerminalBytes(Vec<u8>),
    Lsp(serde_json::Value),
    Closed { reason: String },
}

// ---- originals in repositories (Push Service; ADR-069) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RepositoryOriginChange {
    pub place_id: PlaceId,
    pub path: String,
    pub branch: String,
    pub commit: String,
    pub author: String,
    pub committed_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProjectFetchOutcome {
    pub place_id: PlaceId,
    pub branch: String,
    pub commit: String,
    pub checked_at: Timestamp,
    pub changes: Vec<RepositoryOriginChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RemoteBranchState {
    pub branch: String,
    pub commit: String,
    pub checked_at: Timestamp,
}

/// Result of the engineer's `push`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PushOutcome {
    pub commit: String,
    pub branch: String,
}

// ---- coding tasks ----

pub struct StartCodingTask {
    pub project_id: ProjectId,
    pub origin: RunOrigin,
    pub agent_instance_id: AgentInstanceId,
    pub request: String,
    pub task_name: String,
    pub attempt_group_id: Option<AttemptGroupId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StartedTask {
    pub task: CodingTask,
    pub worktree_place_id: PlaceId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntryDecision {
    Accept,
    Reject,
    KeepYours,
    UseTheAgents,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DecisionOutcome {
    Applied { file_version_id: FileVersionId },
    Dropped,
    Conflict { yours: String, agents: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReviewTask {
    pub task: CodingTask,
    pub change_set: Option<ChangeSet>,
    pub new_problems: Vec<Problem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Problem {
    pub file: String,
    pub line: u32,
    pub severity: String,
    pub code: Option<String>,
    pub message: String,
}

// ---- git views, IDEs, terminal ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StatusEntry {
    pub path: String,
    /// "modified", "new", "deleted", "renamed", "conflicted".
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CommitSummary {
    pub commit: String,
    pub author: String,
    pub at: Timestamp,
    pub subject: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Ide {
    pub id: String,
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EditorSession {
    pub stream: StreamId,
    pub language: Option<String>,
    pub server_line: String,
    /// The screens ask whether language servers may run on this repository (SA-RECON-11).
    pub asks_to_start_servers: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProjectCheckResult {
    pub command: String,
    pub exit_code: Option<i32>,
    pub output: String,
    pub truncated: bool,
    pub elapsed_ms: u64,
}

// ---- repository hosts in process (`GithubAccess`) ----

/// A secret value: `expose()` is the one accessor; `Debug` prints `GitToken(***)`; never logged or
/// stored.
pub struct GitToken(String);

impl GitToken {
    pub(crate) fn new(token: String) -> GitToken {
        GitToken(token)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for GitToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GitToken(***)")
    }
}

/// GitHub "x-access-token", GitLab "oauth2".
#[derive(Debug)]
pub struct GitCredential {
    pub username: String,
    pub token: GitToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepoPermission {
    None,
    Read,
    Triage,
    Write,
    Maintain,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision {
    Pending,
    ChangesRequested,
    Approved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReviewComment {
    pub reviewer: String,
    pub body: String,
    pub at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PullRequestView {
    pub number: u64,
    pub state: PullRequestState,
    pub review: ReviewDecision,
    pub change_requests: Vec<ReviewComment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_commit: Option<String>,
    pub head: String,
    pub base: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BranchReviewRules {
    pub review_required: bool,
    pub code_owner_review_required: bool,
    pub required_approvals: u32,
}

/// Whether a branch requires review, in three answers (ADR-110).
// spec: ordin-desktop/components/ProjectService#ADR-110
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BranchReviewStatus {
    Required {
        required_approvals: u32,
        code_owner_review_required: bool,
    },
    NotRequired,
    /// The person's access does not show whether review is required.
    CannotTell,
}

pub struct OpenPullRequest {
    pub host: String,
    pub repository: String,
    pub head: String,
    pub base: String,
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    pub reviewers: Vec<String>,
}

pub struct BranchWithFiles {
    pub host: String,
    pub repository: String,
    pub base: String,
    pub branch: String,
    pub message: String,
    /// (repository path, bytes).
    pub files: Vec<(String, Vec<u8>)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReviewRequest {
    pub host: String,
    pub repository: String,
    pub number: u64,
    pub head: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict {
    Approve,
    RequestChanges,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GithubPerson {
    pub login: String,
    pub name: Option<String>,
    pub email: Option<String>,
}

/// A folder of a remote repository; `folder` is relative to the repository root, empty for the root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RepoFolderRef {
    pub host: String,
    pub repository: String,
    pub folder: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RepoFile {
    pub name: String,
    pub last_changed_by: String,
    pub last_changed_at: Timestamp,
    pub commit: String,
}

/// One trait, owned here and implemented by `ProjectService`, for every in-process call to GitHub
/// and GitLab. This service opens no approval and no external action for these calls: the caller
/// that decided the write records it.
#[async_trait::async_trait]
pub trait GithubAccess: Send + Sync {
    // credentials (Catalogue Service, Push Service: git2 fetch and push of their own clones)
    async fn git_credentials(&self, host: &str) -> Result<GitCredential, GithubError>;
    // repository facts (Catalogue Service, Handover Service)
    async fn repository_permission(
        &self,
        host: &str,
        repository: &str,
    ) -> Result<RepoPermission, GithubError>;
    async fn default_branch(&self, host: &str, repository: &str) -> Result<String, GithubError>;
    async fn commit_verified(
        &self,
        host: &str,
        repository: &str,
        sha: &str,
    ) -> Result<bool, GithubError>;
    async fn branch_review_rules(
        &self,
        host: &str,
        repository: &str,
        branch: &str,
    ) -> Result<BranchReviewRules, GithubError>;
    // pull requests (Catalogue Service, Push Service, Handover Service)
    async fn pull_request(
        &self,
        host: &str,
        repository: &str,
        number: u64,
    ) -> Result<PullRequestView, GithubError>;
    /// Returns the 2C `PullRequestRef` with `state = Open`.
    async fn open_pull_request(&self, req: OpenPullRequest) -> Result<PullRequestRef, GithubError>;
    /// The open or most recent pull request whose head is `head`; `None` when there is none (Push
    /// Service recovery).
    async fn pull_request_for_branch(
        &self,
        host: &str,
        repository: &str,
        head: &str,
    ) -> Result<Option<PullRequestRef>, GithubError>;
    /// Returns the commit.
    async fn create_branch_with_files(&self, req: BranchWithFiles) -> Result<String, GithubError>;
    async fn review_requests(&self, label: &str) -> Result<Vec<ReviewRequest>, GithubError>;
    async fn review(
        &self,
        host: &str,
        repository: &str,
        number: u64,
        verdict: ReviewVerdict,
        body: &str,
    ) -> Result<(), GithubError>;
    /// Returns whether it merged.
    async fn merge(&self, host: &str, repository: &str, number: u64) -> Result<bool, GithubError>;
    async fn close(&self, host: &str, repository: &str, number: u64) -> Result<(), GithubError>;
    async fn reviewers(
        &self,
        host: &str,
        repository: &str,
        query: &str,
    ) -> Result<Vec<GithubPerson>, GithubError>;
    // files on a branch without a local clone (Handover Service; Ordin Host X-15, GitHub and GitLab)
    /// Returns the bytes and the blob sha.
    async fn file_at(
        &self,
        host: &str,
        repository: &str,
        git_ref: &str,
        path: &str,
    ) -> Result<(Vec<u8>, String), GithubError>;
    /// Returns the commit.
    #[allow(clippy::too_many_arguments)]
    async fn update_file(
        &self,
        host: &str,
        repository: &str,
        branch: &str,
        path: &str,
        bytes: Vec<u8>,
        expected_sha: &str,
        message: &str,
    ) -> Result<String, GithubError>;
    async fn list_files(
        &self,
        folder: &RepoFolderRef,
        glob: &str,
    ) -> Result<Vec<RepoFile>, GithubError>;
    /// Returns the bytes and the commit.
    async fn read_file(
        &self,
        folder: &RepoFolderRef,
        name: &str,
    ) -> Result<(Vec<u8>, String), GithubError>;
    async fn file_last_commit(
        &self,
        folder: &RepoFolderRef,
        name: &str,
    ) -> Result<String, GithubError>;
    /// Amendment January 2026 (ADR-110). GitHub and the company's GitLab.
    async fn branch_review_status(
        &self,
        host: &str,
        repository: &str,
        branch: &str,
    ) -> Result<BranchReviewStatus, GithubError>;
}
