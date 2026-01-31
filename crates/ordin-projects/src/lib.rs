//! The Project Service (ProjectService.md): software engineering in workspaces that link a code
//! repository. It opens a repository folder as a **project**; runs each **coding task** in its own
//! git worktree on a local branch `ordin/<task>` under the application data root, derives the
//! task's **change set** from that worktree and applies the entries the engineer accepts with a
//! three-way merge; runs **check commands** under the Rules Engine; serves the MCP servers `code`,
//! `github`, `gitlab` and `checks` through the MCP Front; starts the **language servers** the
//! organisation approves and relays the Language Server Protocol to the code editor; finds installed
//! **IDEs**; gives the engineer a **terminal** no agent can reach; performs the engineer's commit and
//! push; and fetches followed branches so the Push Service learns which original changed.
//!
//! Git runs in-process through `git2`, with the token from Identity and Credentials supplied per
//! operation, so no git installation is needed. The tables live in the Journal's datastore, SQLite
//! only (programs D and P), registered as module `projects` through [`ProjectService::migrations`].
//!
//! Confirmation items this crate is conditional on (Playbook §7.8), each built with the spec's
//! reading or fallback:
//! - **C-10**, **C-30**: `git2` takes the token from Identity, and `gh` and `glab` are started with
//!   `GH_TOKEN` and `GITLAB_TOKEN` from the same token, so one sign-in serves both whatever these
//!   items find.
//! - **C-32**: the detection table of Internal Logic 12 is used, and an IDE whose start fails is
//!   dropped from the list with a log entry.
//! - **C-33**: no language is confirmed for worktrees, so change review counts no new problem and
//!   the `code` tools return `PROJ_LSP_UNAVAILABLE`, until a language is listed in
//!   [`WORKTREE_CONFIRMED_LANGUAGES`].
//! - **C-92**: every uncertain host reply is read as [`BranchReviewStatus::CannotTell`].
#![forbid(unsafe_code)]

mod checks;
mod editor;
mod error;
mod git;
mod host_cli;
mod hosts;
mod hunks;
mod ide;
mod languages;
mod log;
mod lsp;
mod originals;
mod problems;
mod projects;
mod remote;
mod review;
mod service;
mod settings;
pub mod ssh;
mod store;
mod tasks;
mod terminal;
mod tools;
mod types;

// The shared test fixture names the crate as an integration test does.
#[cfg(test)]
extern crate self as ordin_projects;

#[cfg(test)]
mod tests;

use std::time::Duration;

pub use error::{GithubError, ProjectError, ALL_CODES};
pub use ordin_rules::CommandOrigin;
pub use ordin_types::domain::{
    ChangeEntryState, ChangeSet, ChangeSetEntry, ChangeSetState, CodingTask, CodingTaskState,
    GithubOperationSetting, GithubSettingSource, LanguagePlugin, LanguagePluginSource,
    LanguagePluginStatus, Project, ProjectState, PullRequestRef, PullRequestState, RunOrigin,
    TestOutcome,
};
pub use ordin_types::num::Sha256Hex;
pub use ordin_types::time::Timestamp;
pub use ordin_types::{
    AgentInstanceId, AttemptGroupId, ChangeSetEntryId, ChangeSetId, CodingTaskId, FileVersionId,
    PlaceId, ProjectId, RunId, TaskId, WorkspaceId,
};
pub use service::{ProjectDeps, ProjectService};
pub use types::{
    ApprovedServers, BranchReviewRules, BranchReviewStatus, BranchWithFiles, CommitSummary,
    DecisionOutcome, EditorSession, EntryDecision, GitCredential, GitToken, GithubAccess,
    GithubPerson, Ide, OpenPullRequest, Problem, ProjectCheckResult, ProjectFetchOutcome,
    ProjectSettings, PullRequestView, PushOutcome, RemoteBranchState, RepoFile, RepoFolderRef,
    RepoPermission, RepositoryOriginChange, ReviewComment, ReviewDecision, ReviewRequest,
    ReviewTask, ReviewVerdict, ServerInstaller, ServerLaunch, ServerPin, StartCodingTask,
    StartedTask, StatusEntry, StreamId, StreamPayload, StreamSink,
};

/// 15 minutes; doubled up to 2 hours while the host reports a rate limit (A-36, 2D-4).
pub const FETCH_INTERVAL: Duration = Duration::from_secs(15 * 60);
/// The longest interval a rate limit stretches `FETCH_INTERVAL` to.
pub const FETCH_INTERVAL_MAX: Duration = Duration::from_secs(2 * 3600);
/// 20 per person (A-36).
pub const MAX_FOLLOWED_REPOSITORIES: u32 = 20;
/// 30 minutes per check command.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// 1 MiB returned; the rest cut with a line saying so.
pub const CHECK_OUTPUT_MAX: usize = 1024 * 1024;
/// 120 s per fetch or push.
pub const GIT_NETWORK_TIMEOUT: Duration = Duration::from_secs(120);
/// 60 s for `initialize`.
pub const LSP_START_TIMEOUT: Duration = Duration::from_secs(60);
/// 10 s per code-intelligence request.
pub const LSP_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// 3 s without a new `publishDiagnostics` before diagnostics are read.
pub const DIAGNOSTICS_SETTLE: Duration = Duration::from_secs(3);
/// 12 server processes at once (R-37).
pub const MAX_LANGUAGE_INSTANCES: usize = 12;
/// Client → server methods the code editor may send (ADR-056); `initialize` is ordin's own.
pub const EDITOR_LSP_METHODS: [&str; 10] = [
    "textDocument/didOpen",
    "textDocument/didChange",
    "textDocument/didClose",
    "textDocument/didSave",
    "textDocument/completion",
    "completionItem/resolve",
    "textDocument/signatureHelp",
    "textDocument/hover",
    "textDocument/definition",
    "textDocument/semanticTokens/full",
];
/// 120 s per `gh` or `glab` call.
pub const GH_TIMEOUT: Duration = Duration::from_secs(120);
/// The operation kinds of SA-GH-01. Reads never need approval; every write kind does by default.
pub const OPERATION_KINDS: [&str; 12] = [
    "read",
    "comment",
    "open_pull_request",
    "update_pull_request",
    "request_review",
    "push_branch",
    "create_branch",
    "delete_branch",
    "label",
    "merge",
    "release",
    "close",
];
/// The languages whose pinned servers are confirmed to work in a worktree (C-33). Until a language
/// is listed, change review shows no "new problems" line for it and the `code` tools return
/// `PROJ_LSP_UNAVAILABLE` for it.
// spec: ordin-desktop/components/ProjectService#C-33
pub const WORKTREE_CONFIRMED_LANGUAGES: [&str; 0] = [];

/// At most 200 commits per `history` call (Internal Logic 10.1).
pub(crate) const HISTORY_MAX: u32 = 200;
/// Check-command limits (2C).
pub(crate) const MAX_CHECK_COMMANDS: usize = 20;
pub(crate) const MAX_CHECK_COMMAND_CHARS: usize = 1000;
/// `CodingTask.request` (2C).
pub(crate) const MAX_REQUEST_CHARS: usize = 4000;
/// `ChangeSetEntry.hunk` (2C).
pub(crate) const MAX_HUNK_BYTES: usize = 1024 * 1024;
/// `LanguagePlugin.server_settings` (2C).
pub(crate) const MAX_SERVER_SETTINGS_BYTES: usize = 64 * 1024;
/// `gh` and `glab` tool output (Internal Logic 8.3).
pub(crate) const TOOL_OUTPUT_MAX: usize = 1024 * 1024;
/// How long the detected IDE list is kept (Internal Logic 12.1).
pub(crate) const IDE_CACHE: Duration = Duration::from_secs(10 * 60);
/// The coverage check runs at most once a minute per place (Internal Logic 2.1).
pub(crate) const COVERAGE_CHECK_EVERY: Duration = Duration::from_secs(60);
/// How often the fetch timer looks for a repository that is due.
pub(crate) const FETCH_TICK: Duration = Duration::from_secs(60);
/// `list_files` returns at most 200 files (Internal Logic 14.4).
pub(crate) const MAX_LISTED_FILES: usize = 200;
/// `reviewers` returns at most 10 people (Internal Logic 14.3).
pub(crate) const MAX_REVIEWERS: usize = 10;
/// The hunk of a binary file's one entry (Internal Logic 5.6).
pub(crate) const BINARY_HUNK: &str = "Binary file changed";
/// The hunk of a file too large to review by hunk (Internal Logic 5.6).
pub(crate) const TOO_LARGE_HUNK: &str = "too large to review here";
/// Removed from the environment of every child; the one token a `gh` or `glab` child needs is set
/// back explicitly (2D-1c).
pub(crate) const CREDENTIAL_VARIABLES: [&str; 9] = [
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "GITLAB_TOKEN",
    "COPILOT_GITHUB_TOKEN",
    "GITHUB_APM_PAT",
    "ANTHROPIC_API_KEY",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "ORDIN_HOOK_SECRET",
    "ORDIN_MCP_TOKEN",
];
