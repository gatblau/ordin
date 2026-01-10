# SPEC: Project Service

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-103) · amended January 2026 (ADR-110; promoted to Approved by the user January 2026) · amended January 2026 (`FetchOutcome` and `CheckResult` are named `ProjectFetchOutcome` and `ProjectCheckResult`, apart from the Catalogue Service's and the Setup Assistant's types)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-projects` · **Build phase:** 5 · **Programs:** D P
> **Dependencies:** MCP Front, File Service, Rules Engine, Identity, Journal

## Table of contents

- [Purpose](#purpose)
- [Approach](#approach)
- [Shared Context](#shared-context)
- [Public Interface](#public-interface)
- [Internal Logic](#internal-logic)
- [Data Model](#data-model)
- [Error Table](#error-table)
- [Acceptance Criteria (Gherkin)](#acceptance-criteria-gherkin)
- [Performance / Security / Observability](#performance--security--observability)
- [Gaps](#gaps)
- [Amendment January 2026 — Team knowledge (ADR-103)](#amendment-january-2026--team-knowledge-adr-103)
- [Amendment January 2026 — Reading other teams' knowledge (ADR-110)](#amendment-january-2026--reading-other-teams-knowledge-adr-110)

Line references: `02:N` is `docs/design/ordin-desktop/02-architecture.md` line N; `03:N`, `05:N`, `07:N`, `08:N`, `10:N`, `11:N` likewise; `09 ADR-0NN` is an ADR in `09-decisions.md`; `2C` is `docs/spec/ordin-desktop/phase-2c-shared-types-*.md`; `B7 contract` is the batch's shared contract sheet whose decisions are restated here.

## Purpose

The Project Service supports software engineering in workspaces that link a code repository (02:760, 09 ADR-019 to ADR-024, ADR-047, ADR-050 to ADR-056). It opens a repository folder as a **project**; runs each **coding task** in its own git worktree on a local branch `ordin/<task>` under the application data root and derives the task's **change set** from that worktree, which the engineer accepts or rejects entry by entry, an accepted entry being applied to the engineer's working copy with a three-way merge (ADR-053); runs **check commands** under the Rules Engine; serves the **code-intelligence** MCP server and the GitHub and GitLab tools to sessions through the MCP Front (ADR-054, ADR-023, ADR-051); starts the **language servers** the organisation approves and relays the Language Server Protocol to the code editor (ADR-055, ADR-056); writes the engineer's saved edits through the File Service; finds installed **IDEs** and opens the chosen one (ADR-052); gives the engineer a **terminal** no agent can reach (ADR-022); shows status, diffs, history and branches and performs the engineer's commit and push; and, for every repository holding a Markdown **original** with published copies, fetches the followed branch at workspace open, at start and every 15 minutes and tells the Push Service which original changed, in which commit and by whom (ADR-069, A-36). It owns project records, change sets, language-server lifecycles and git operations (02:775).

## Approach

Git runs in-process through `git2` 0.21.0 with libgit2 bundled (SA-GIT-01; OQ-67 settled: libgit2 is a named licence exception), with HTTPS credentials supplied by a callback from Identity and Credentials, so no git installation is needed (05:34). Worktrees share the repository's object store; a task's change set is `git diff` of its worktree against its base commit, so the service depends on no harness adapter (2B-2 D5). Language servers are child processes speaking LSP over stdio, framed in this crate (`Content-Length` headers) with `serde_json` messages, so no LSP library is added. The terminal is `portable-pty` 0.9.0 (SA-TERM-01). Rejected: driving an installed `git` binary (business laptops have none, 05:34); deriving change sets from the harness's edit events (would tie the service to each adapter, 2B-2 D5).

## Shared Context

### Identifiers and time (2C part 1 *Conventions*)

```rust
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>);   // RFC 3339, UTC, ms, `Z`
pub struct ProjectId(pub uuid::Uuid);  pub struct PlaceId(pub uuid::Uuid);  pub struct WorkspaceId(pub uuid::Uuid);
pub struct CodingTaskId(pub uuid::Uuid);  pub struct AttemptGroupId(pub uuid::Uuid);
pub struct ChangeSetId(pub uuid::Uuid);  pub struct ChangeSetEntryId(pub uuid::Uuid);
pub struct GithubOperationSettingId(pub uuid::Uuid);  pub struct LanguagePluginId(pub uuid::Uuid);
pub struct RunId(pub uuid::Uuid);  pub struct TaskId(pub uuid::Uuid);  pub struct CrewRunId(pub uuid::Uuid);
pub struct AgentInstanceId(pub uuid::Uuid);  pub struct FileVersionId(pub uuid::Uuid);
pub struct Sha256Hex(pub String);
```

### Domain types owned here (2C part 1, copied unchanged)

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Project {
    pub id: ProjectId,
    pub place_id: PlaceId,
    pub display_name: String,
    pub is_git_repository: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_branch: Option<String>,
    pub language_servers_in_use: Vec<String>,
    pub check_commands: Vec<String>,
    pub state: ProjectState,
    pub opened_at: Timestamp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectState { Opened, Indexed, Closed }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GithubOperationSetting {
    pub id: GithubOperationSettingId,
    pub project_id: ProjectId,
    pub operation_kind: String,
    pub is_write: bool,
    pub approval_required: bool,
    pub set_by: GithubSettingSource,
    pub set_at: Timestamp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GithubSettingSource { OrganisationProfile, Engineer }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LanguagePlugin {
    pub id: LanguagePluginId,
    pub language: String,
    pub approved_servers: Vec<String>,
    pub default_server: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chosen_server: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pinned_version: Option<String>,
    pub source: LanguagePluginSource,
    pub enabled: bool,
    pub server_settings: serde_json::Value,
    pub status: LanguagePluginStatus,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguagePluginSource { AgentPackage, OrganisationProfile }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguagePluginStatus { Available, Added, Installed, Removed }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CodingTask {
    pub id: CodingTaskId,
    pub origin: RunOrigin,
    pub project_id: ProjectId,
    pub agent_instance_id: AgentInstanceId,
    pub request: String,
    pub worktree_path: String,
    /// `ordin/<task name>`.
    pub branch: String,
    pub base_branch: String,
    pub base_commit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt_group_id: Option<AttemptGroupId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_outcome: Option<TestOutcome>,
    pub new_problems: u32,
    pub state: CodingTaskState,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestOutcome { Passed, Failed }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodingTaskState { Working, ReadyForReview, Settled, SetAside }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunOrigin { Task { task_id: TaskId }, CrewRound { crew_run_id: CrewRunId, round_number: u32 } }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ChangeSet {
    pub id: ChangeSetId,
    pub coding_task_id: CodingTaskId,
    pub run_id: RunId,
    pub project_id: ProjectId,
    pub entries: Vec<ChangeSetEntry>,
    pub state: ChangeSetState,
    pub created_at: Timestamp,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ChangeSetEntry { pub id: ChangeSetEntryId, pub file: String, pub hunk: String, pub state: ChangeEntryState }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeEntryState { Pending, Accepted, Rejected }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeSetState { Open, InReview, Settled }
```

| Entity | Rule | Error code |
|---|---|---|
| Project.place_id | unique among non-closed projects | `PROJ_PLACE_ALREADY_OPEN` |
| Project.current_branch | present iff `is_git_repository` | `PROJ_BRANCH_MISMATCH` |
| Project.check_commands | ≤ 20, each ≤ 1000 chars | `PROJ_CHECKS_INVALID` |
| GithubOperationSetting (project_id, operation_kind) | unique | `GHOP_DUPLICATE` |
| approval_required = false | refused when the profile's ceiling requires approval | `GHOP_ABOVE_CEILING` |
| LanguagePlugin.language | 1–40 chars, unique | `LANG_DUPLICATE` |
| default_server, chosen_server | members of approved_servers | `LANG_SERVER_NOT_APPROVED` |
| server_settings | JSON object ≤ 64 KiB | `LANG_SETTINGS_INVALID` |
| CodingTask.branch | `ordin/` + `[a-z0-9-]{1,100}`, valid git ref | `CODE_BRANCH_INVALID` |
| CodingTask.base_commit | 40 lowercase hex | `CODE_COMMIT_INVALID` |
| CodingTask.worktree_path | under the application data root | `CODE_WORKTREE_OUTSIDE_ROOT` |
| attempt group | ≥ 2 tasks on the same request | `CODE_GROUP_TOO_SMALL` |
| CodingTask.request | 1–4000 chars | `CODE_REQUEST_INVALID` |
| ChangeSet.coding_task_id | unique | `CSET_DUPLICATE` |
| ChangeSetEntry.hunk | ≤ 1 MiB | `CSET_HUNK_TOO_LARGE` |
| ChangeSet settled | no pending entry | `CSET_UNDECIDED_ENTRIES` |

State machines (2C): Project `→ opened → indexed`; `opened, indexed → closed`; `closed → opened`. LanguagePlugin `→ available → added → installed`; `installed → installed` (pin changed, on/off); `added, installed → removed`. CodingTask `→ working → ready_for_review → settled`; `working, ready_for_review → set_aside` (another attempt kept). ChangeSet `→ open → in_review → settled`.

### Operation kinds (SA-GH-01) and settings injected by the program (B7 contract D-4)

Kinds: `read`, `comment`, `open_pull_request`, `update_pull_request`, `request_review`, `push_branch`, `create_branch`, `delete_branch`, `label`, `merge`, `release`, `close`. Reads never need approval; every write kind needs approval by default.

```rust
#[derive(Debug, Clone)]
pub struct ProjectSettings {
    /// Keyed by `go`, `rust`, `java`, `typescript`. Built by the program from the profile's `language_servers[]`
    /// entries `{ language, name, version, platforms }` (2D-2, K12): every entry of a language is approved and the
    /// first in profile order is the default; a language with no profile entry takes the entries of the build's
    /// `language-servers.json` table (same shape, pinned at release) (SA-CLOSE-02).
    pub language_servers: std::collections::BTreeMap<String, ApprovedServers>,
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
pub struct ApprovedServers { pub approved: Vec<ServerPin>, pub default: String }
#[derive(Debug, Clone)]
pub struct ServerPin { pub name: String, pub version: String }
```

The first languages and servers (ADR-024, ADR-056, SA-LSP-01/02): Go `gopls`; Rust `rust-analyzer`; Java `jdtls` (Eclipse JDT LS, with a JDK the profile pins); TypeScript the TypeScript 7 built-in server (`typescript` 7.0.2), with `typescript-language-server` 6.0.2 as an approved alternative.

### Boundary interfaces of other components (each provider's current signature)

```rust
// ordin-journal (concrete `ordin_journal::Journal`, D4): `datastore() -> &Datastore` for this crate's tables;
//   `append_entry(NewJournalEntry { kind, component: "projects", run_id, crew_run_id, workspace_id, summary, data })`
//   for the journal entries named in Internal Logic; `ModuleMigrations { module, sqlite, postgres }` built from
//   `MigrationStep = (i64, &'static str, &'static str)`; errors `JournalError`.

// ordin-mcp-front: this service implements `ToolProvider` (McpFront.md) and the program registers it:
//   fn provider_id() -> "projects"; fn servers() -> Vec<ServerSpec { name: ServerName, tools: Vec<ToolSpec> }>;
//   async fn offer(&self, grant: &SessionGrant) -> Result<Vec<ToolKey>, ToolFailure>;
//   fn check_arguments(&self, key: &ToolKey, arguments: &serde_json::Value) -> Result<(), ArgumentProblem>;
//   async fn call(&self, ctx: ToolCallContext, key: &ToolKey, arguments: serde_json::Value) -> Result<ToolOutput, ToolFailure>;
//   async fn session_closed(&self, run_id: RunId).
//   `SessionGrant` and `ToolCallContext` carry `coding_task_id: Option<CodingTaskId>` (McpFront SA-RECON-01).
//   `ToolRegistry::tools_changed(&ServerName)` after a project opens or closes in a session's workspace.

// ordin-rules (concrete `ordin_rules::RulesEngine`, held as `Arc<ordin_rules::RulesEngine>`):
//   pub async fn decide_command(&self, req: CommandRequest) -> RulesOutcome;      // never fails (K.1)
//   pub async fn wait_for_approval(&self, approval_id: ApprovalId, until: Option<tokio::time::Instant>)
//       -> Result<ApprovalWait, RulesError>;                                      // Approved | Rejected | Expired | Pending
//   pub fn replace_git_host_settings(&self, project_place: PlaceId, settings: Vec<GitHostSetting>);   // K.3
//   pub struct CommandRequest { workspace_id, origin: CommandOrigin, command (1–8,000), working_dir (absolute), declared_check }
//   pub enum CommandOrigin { Person, Run { run_id, agent_instance_id } }          // serde tag "kind"
//   pub struct GitHostSetting { kind: String, approval_required: bool }
//   pub struct RulesOutcome { decision: ToolDecision /* 2C wire */, tool_request_id: ToolRequestId, approval_id: Option<ApprovalId> }

// ordin-identity (concrete `IdentityService`):
//   pub async fn access_token(&self, kind: CredentialKind, account_label: Option<&str>) -> Result<AccessToken, CredError>;
//   pub struct AccessToken { token: SecretString, expires_at: Option<Timestamp>, credential_id: CredentialId }
//   CredentialKind::Github and CredentialKind::Gitlab (2C, D9); `CRED_SIGN_IN_REQUIRED` means no sign-in.

// ordin-files (concrete `FileService`): add_place(AddPlace { path, friendly_name: Option<String>, kind: PlaceKind })
//   -> Place; remove_place(place_id); place(place_id) -> Place; read_file(place_id, path) -> FileBytes;
//   write_version(WriteVersion { place_id, path, bytes, origin: FileVersionOrigin, expected_hash }) -> FileVersion;
//   watch(&[PlaceId]); unwatch(&[PlaceId]); subscribe_changes() -> broadcast::Receiver<PlaceChange> (`by_ordin`);
//   coverage(&[PlaceId]) -> IndexCoverage { indexed, total }; FileError (`FILE_CHANGED_MEANWHILE { current }`).
//   PlaceKind::CodingTaskWorktree (2C, C-9). FileVersionOrigin::{Person, Run { run_id, tool }}.

/// Mine (owned here; implemented by the program over `SetupAssistant::ensure_language_server(language,
/// &pin.name, &pin.version) -> LanguageServerLaunch`, Setup Assistant SA-RECON-05): installs a pinned language
/// server or its toolchain on first use and returns how to start it.
#[async_trait::async_trait]
pub trait ServerInstaller: Send + Sync {
    async fn ensure(&self, language: &str, server: &ServerPin) -> Result<ServerLaunch, String>;
}
pub struct ServerLaunch { pub program: std::path::PathBuf, pub args: Vec<String>, pub env: Vec<(String, String)> }

/// Mine: an unjournalled stream to the screens for terminal bytes and LSP messages (terminal content is never
/// journalled, ADR-022). Implemented by the desktop program over the Command Bridge's
/// `ScreenStreams::send(stream, StreamKind::Terminal | StreamKind::Lsp, payload)` and `close`.
pub trait StreamSink: Send + Sync {
    fn send(&self, stream: StreamId, payload: StreamPayload);
}
pub struct StreamId(pub uuid::Uuid);
pub enum StreamPayload { TerminalBytes(Vec<u8>), Lsp(serde_json::Value), Closed { reason: String } }
```

### Reconciliation assumptions

- **SA-RECON-01** Ruling R3, D4: tables live in the Journal's `Datastore`; migrations are exported as `migrations() -> ModuleMigrations` (module `projects`) and registered by the program.
- **SA-RECON-02** Ruling R2: `ToolProvider` is McpFront's trait; the program registers this provider, so `ProjectDeps` holds the MCP Front only as `ToolRegistry`, for `tools_changed` (reconciliation row 643).
- **SA-RECON-03** Ruling R5: the Rules Engine's names are used (`decide_command`, `wait_for_approval`, `replace_git_host_settings`, `GitHostSetting`); an *ask* outcome is awaited here with `wait_for_approval` (reconciliation rows 206, 207).
- **SA-RECON-04** Ruling R12: this service commits only inside coding-task worktrees and the engineer's own `commit`/`push`; the Push Service commits to Git originals with `GithubAccess::git_credentials`.
- **SA-CLOSE-01** (*chosen here*; closes the GitLab host gap; closing decision K10) The company GitLab is the profile key `gitlab.host` (a host name, default `gitlab.com`); `gitlab.client_id` is Identity's (device sign-in on that host when present, else a pasted personal access token kept as `CredentialKind::Gitlab`). This service reads only the host, through `ProjectSettings.gitlab_host`; `gitlab.com` stays reachable beside a company host, with the same `CredentialKind::Gitlab` sign-in (a token the other host refuses gives `PROJ_REMOTE_UNREACHABLE`).
- **SA-CLOSE-02** (*chosen here*; closing decision K12) Language servers: the profile's `language_servers[]` entries `{ language: go|rust|java|typescript, name, version, platforms: { <platform>: { asset_url, sha256, file_name, binary_path } } }`, or, for a language with no entry, the build's `language-servers.json` table of the same shape. Installation (download, SHA-256 check, unpack into `<data root>/language-servers/<name>/<version>/`) is the Setup Assistant's, reached through `ServerInstaller`; this service chooses only which approved server runs.
- **SA-CLOSE-03** (*chosen here*) A project's check commands have no organisation-profile key. The engineer sets them on the project's settings screen, which sends the Service API command `projects.set_check_commands { project_id, commands: Vec<String> }` (owner this service, scope `Any`; ServiceApi.md SA-CLOSE-07); the command calls `set_check_commands`, which trims the commands, drops empty ones and duplicates, checks the limits (Internal Logic 6) and writes the list as a JSON array to this service's own `projects` table, column `check_commands`. Default when none are set: none — a project starts with an empty list, no check runs for its coding tasks (`test_outcome` stays absent) and the screens show "No checks set". No program calls `set_check_commands`.
- **SA-RECON-05** Ruling R6, D9: tokens come from Identity and Credentials' `access_token(kind, None)` with `CredentialKind::Github` or `CredentialKind::Gitlab`.
- **SA-RECON-06** Ruling R1: the Service API catalogue carries this service's `projects.*` commands with the names, inputs and results below.
- **SA-RECON-07** `ToolCallContext` carries no Rules Engine classification, so the log line `git_host_call` records the exit code only; the Rules Engine's `ToolRequest` holds the kind (reconciliation row 644).
- **SA-RECON-08** Ruling R6 (row 662), amended January 2026: the Ordin Host starts coding tasks; the session is granted the worktree place through `StartRun.coding_task_id`, which the Session Supervisor copies from the Ordin Host's `submit_task` and the local adapters put in the MCP Front's `SessionGrant`. The Session Supervisor and the Crew Orchestrator do not call this service.

### Constants (*chosen here* unless cited)

| Constant | Value | Source |
|---|---|---|
| `FETCH_INTERVAL` | 15 minutes; doubled up to 2 hours while the host reports a rate limit | A-36, 2D-4 |
| `MAX_FOLLOWED_REPOSITORIES` | 20 per person | A-36 |
| `WORKTREE_ROOT` | `<app data root>/worktrees/<project_id>/<coding_task_id>/` | ADR-053 |
| `BASE_WORKTREE_ROOT` | `<app data root>/worktrees/<project_id>/base-<coding_task_id>/` | SA-LSP-03 |
| `CHECK_TIMEOUT` | 30 minutes per check command | *assumed here* |
| `CHECK_OUTPUT_MAX` | 1 MiB returned; the rest cut with a line saying so | *assumed here* |
| `GIT_NETWORK_TIMEOUT` | 120 s per fetch or push | *chosen here* |
| `LSP_START_TIMEOUT` | 60 s for `initialize` | *chosen here* |
| `LSP_REQUEST_TIMEOUT` | 10 s per code-intelligence request | *chosen here* |
| `DIAGNOSTICS_SETTLE` | 3 s without a new `publishDiagnostics` before diagnostics are read | *chosen here* |
| `MAX_LANGUAGE_INSTANCES` | 12 server processes at once | *chosen here*; R-37 |
| `EDITOR_LSP_METHODS` | client → server: `initialize`-free; `textDocument/didOpen`, `didChange`, `didClose`, `didSave`, `completion`, `completionItem/resolve`, `signatureHelp`, `hover`, `definition`, `semanticTokens/full` | ADR-056 (completion, signature, resolve) |
| `GH_TIMEOUT` | 120 s per `gh` or `glab` call | *chosen here* |

### Confirmation items bound here

- **C-10** — one `gh` sign-in serves `gh` and git. *Reading:* git does not use `gh` at all: `git2` takes the token from Identity, and `gh` is started with `GH_TOKEN` from the same Identity token, so one sign-in serves both whatever C-10 finds.
- **C-30** — `glab` signs in through the browser and its token serves git. Same reading with `GITLAB_TOKEN`, from Identity and Credentials' `CredentialKind::Gitlab` sign-in (D9).
- **C-32** — IDEs are found and started with a folder or file on macOS and Windows. *Until confirmed*, the detection table below is used, and an IDE whose start fails is dropped from the list with a log entry; *Open in [IDE]* is then not shown for it.
- **C-33** — pinned servers work in a worktree. *Until confirmed* for a language, change review shows no "new problems" line for that language and code-intelligence tools for it return `PROJ_LSP_UNAVAILABLE` (ADR-055's behaviour for a server turned off).

## Public Interface

Error prefix: `PROJ_` for this component's codes; 2C's `CODE_`, `CSET_`, `GHOP_`, `LANG_`.

```rust
pub struct ProjectDeps {
    /// Datastore owner (D4): this crate's tables through `journal.datastore()`; journal entries through `append_entry`.
    pub journal: std::sync::Arc<ordin_journal::Journal>,
    /// To call `tools_changed` for `code`, `github`, `gitlab`, `checks` (McpFront `ToolRegistry`).
    pub tool_registry: std::sync::Arc<dyn ToolRegistry>,
    pub files: std::sync::Arc<FileService>,
    pub rules: std::sync::Arc<ordin_rules::RulesEngine>,
    pub identity: std::sync::Arc<IdentityService>,
    pub installer: std::sync::Arc<dyn ServerInstaller>,
    pub streams: std::sync::Arc<dyn StreamSink>,
    pub settings: tokio::sync::watch::Receiver<ProjectSettings>,
    pub data_root: std::path::PathBuf,
}

pub struct ProjectService { /* private */ }

// ---- originals in repositories (Push Service; ADR-069) ----
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RepositoryOriginChange { pub place_id: PlaceId, pub path: String, pub branch: String,
    pub commit: String, pub author: String, pub committed_at: Timestamp }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProjectFetchOutcome { pub place_id: PlaceId, pub branch: String, pub commit: String, pub checked_at: Timestamp, pub changes: Vec<RepositoryOriginChange> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RemoteBranchState { pub branch: String, pub commit: String, pub checked_at: Timestamp }
/// Result of the engineer's `push`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PushOutcome { pub commit: String, pub branch: String }

// ---- coding tasks ----
pub struct StartCodingTask {
    pub project_id: ProjectId, pub origin: RunOrigin, pub agent_instance_id: AgentInstanceId,
    pub request: String, pub task_name: String, pub attempt_group_id: Option<AttemptGroupId>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StartedTask { pub task: CodingTask, pub worktree_place_id: PlaceId }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntryDecision { Accept, Reject, KeepYours, UseTheAgents }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DecisionOutcome { Applied { file_version_id: FileVersionId }, Dropped, Conflict { yours: String, agents: String } }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReviewTask { pub task: CodingTask, pub change_set: Option<ChangeSet>, pub new_problems: Vec<Problem> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Problem { pub file: String, pub line: u32, pub severity: String, pub code: Option<String>, pub message: String }

// ---- git views, IDEs, terminal ----
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StatusEntry { pub path: String, pub status: String }        // "modified", "new", "deleted", "renamed", "conflicted"
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CommitSummary { pub commit: String, pub author: String, pub at: Timestamp, pub subject: String }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Ide { pub id: String, pub name: String, pub enabled: bool }

impl ProjectService {
    pub const MODULE: &'static str = "projects";
    /// Data Model as `MigrationStep` tuples, version 1; the program registers it (SA-RECON-01).
    pub fn migrations() -> ordin_journal::ModuleMigrations;
    pub async fn new(deps: ProjectDeps) -> Result<std::sync::Arc<ProjectService>, ProjectError>;
    /// The provider serving `code`, `github`, `gitlab`, `checks`; the program registers it with the MCP Front.
    pub fn tool_provider(self: &std::sync::Arc<Self>) -> std::sync::Arc<dyn ToolProvider>;
    /// Starts the fetch timer; reconciles settings; removes worktrees of tasks already settled or set aside
    /// (crash recovery).
    pub async fn start(self: &std::sync::Arc<Self>) -> Result<(), ProjectError>;
    pub async fn shutdown(&self);

    // projects
    pub async fn open_project(&self, place_id: PlaceId, workspace_id: WorkspaceId) -> Result<Project, ProjectError>;
    pub async fn close_project(&self, project_id: ProjectId) -> Result<Project, ProjectError>;
    pub async fn project(&self, project_id: ProjectId) -> Result<Project, ProjectError>;
    /// The project not `closed` on a place, if any (Internal Logic 2.5).
    pub async fn project_for_place(&self, place_id: PlaceId) -> Result<Option<Project>, ProjectError>;
    pub async fn set_check_commands(&self, project_id: ProjectId, commands: Vec<String>) -> Result<Project, ProjectError>;

    // originals (ADR-069)
    pub async fn follow_original(&self, place_id: PlaceId, path: &str, branch: &str) -> Result<(), ProjectError>;
    pub async fn unfollow_original(&self, place_id: PlaceId, path: &str, branch: &str) -> Result<(), ProjectError>;
    pub async fn fetch_followed(&self, repo_place: PlaceId, branch: &str) -> Result<ProjectFetchOutcome, ProjectError>;
    /// The program calls it when a workspace opens, with that workspace's repository places.
    pub async fn workspace_opened(&self, repo_places: &[PlaceId]);
    pub fn subscribe_original_changes(&self) -> tokio::sync::broadcast::Receiver<RepositoryOriginChange>;
    pub async fn read_at_commit(&self, repo_place: PlaceId, commit: &str, path: &str) -> Result<Vec<u8>, ProjectError>;
    pub async fn remote_state(&self, place_id: PlaceId, branch: &str) -> Result<RemoteBranchState, ProjectError>;
    pub async fn default_branch(&self, place_id: PlaceId) -> Result<String, ProjectError>;
    /// Lower-case names of the languages of the workspace's open projects (Internal Logic 2.4; ADR-024).
    pub async fn workspace_languages(&self, workspace_id: WorkspaceId) -> Result<Vec<String>, ProjectError>;

    // coding tasks and change review (ADR-053, ADR-054)
    pub async fn start_coding_task(&self, req: StartCodingTask) -> Result<StartedTask, ProjectError>;
    pub async fn end_coding_run(&self, task: CodingTaskId, run_id: RunId) -> Result<ChangeSet, ProjectError>;
    pub async fn review(&self, project_id: ProjectId) -> Result<Vec<ReviewTask>, ProjectError>;
    pub async fn decide_entry(&self, entry: ChangeSetEntryId, decision: EntryDecision) -> Result<DecisionOutcome, ProjectError>;
    pub async fn keep_attempt(&self, task: CodingTaskId) -> Result<(), ProjectError>;
    pub async fn run_check(&self, project_id: ProjectId, task: Option<CodingTaskId>, command: &str,
                           origin: CommandOrigin) -> Result<ProjectCheckResult, ProjectError>;

    // engineer's git
    pub async fn status(&self, project_id: ProjectId) -> Result<Vec<StatusEntry>, ProjectError>;
    pub async fn diff(&self, project_id: ProjectId, path: Option<&str>) -> Result<String, ProjectError>;
    pub async fn history(&self, project_id: ProjectId, path: Option<&str>, limit: u32) -> Result<Vec<CommitSummary>, ProjectError>;
    pub async fn branches(&self, project_id: ProjectId) -> Result<Vec<String>, ProjectError>;
    pub async fn create_branch(&self, project_id: ProjectId, name: &str) -> Result<(), ProjectError>;
    pub async fn switch_branch(&self, project_id: ProjectId, name: &str) -> Result<(), ProjectError>;
    pub async fn commit(&self, project_id: ProjectId, message: &str, terminal: Option<StreamId>) -> Result<CommitSummary, ProjectError>;
    pub async fn push(&self, project_id: ProjectId, terminal: Option<StreamId>) -> Result<PushOutcome, ProjectError>;
    pub async fn save_file(&self, project_id: ProjectId, path: &str, text: String, expected_hash: Option<Sha256Hex>) -> Result<FileVersionId, ProjectError>;

    // GitHub / GitLab approval settings (ADR-023)
    pub async fn git_host_settings(&self, project_id: ProjectId) -> Result<Vec<GithubOperationSetting>, ProjectError>;
    pub async fn set_git_host_approval(&self, project_id: ProjectId, kind: &str, approval_required: bool) -> Result<GithubOperationSetting, ProjectError>;

    // language servers (ADR-055, ADR-056)
    pub async fn language_plugins(&self) -> Result<Vec<LanguagePlugin>, ProjectError>;
    pub async fn configure_language(&self, language: &str, chosen_server: Option<String>, enabled: bool,
                                    server_settings: serde_json::Value) -> Result<LanguagePlugin, ProjectError>;
    pub async fn restart_language_server(&self, language: &str) -> Result<(), ProjectError>;
    pub async fn editor_open(&self, project_id: ProjectId, path: &str) -> Result<EditorSession, ProjectError>;
    pub async fn lsp_send(&self, session: StreamId, message: serde_json::Value) -> Result<(), ProjectError>;
    pub async fn editor_close(&self, session: StreamId);

    // IDEs and terminal
    pub async fn ides(&self) -> Result<Vec<Ide>, ProjectError>;
    pub async fn set_ide_enabled(&self, ide_id: &str, enabled: bool) -> Result<(), ProjectError>;
    pub async fn open_in_ide(&self, project_id: ProjectId, ide_id: &str, path: Option<&str>) -> Result<(), ProjectError>;
    pub async fn terminal_open(&self, project_id: ProjectId, cols: u16, rows: u16) -> Result<StreamId, ProjectError>;
    pub async fn terminal_input(&self, terminal: StreamId, bytes: Vec<u8>) -> Result<(), ProjectError>;
    pub async fn terminal_resize(&self, terminal: StreamId, cols: u16, rows: u16) -> Result<(), ProjectError>;
    pub async fn terminal_close(&self, terminal: StreamId);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EditorSession { pub stream: StreamId, pub language: Option<String>, pub server_line: String }
```

### Repository hosts in process (`GithubAccess`; reconciliation rows 646–648, 653–654)

One trait, owned here and implemented by `ProjectService`, serves every in-process call to GitHub and GitLab; the Catalogue Service, Push Service, Handover Service and Ordin Host import it instead of restating their own. Every method runs `gh api` (GitHub) or `glab api` (GitLab) with the token from Identity, as Internal Logic 8.3 does for the tools (Internal Logic 14). This service opens no approval and no external action for these calls: the caller that decided the write records it (the Push Service with `PullRequestOpened`, the Handover Service with `HandoverWritten`).

```rust
/// A secret value: `expose()` is the one accessor; `Debug` prints `GitToken(***)`; never logged or stored.
pub struct GitToken(/* private */ String);
impl GitToken { pub fn expose(&self) -> &str; }
pub struct GitCredential { pub username: String, pub token: GitToken }   // GitHub "x-access-token", GitLab "oauth2"

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepoPermission { None, Read, Triage, Write, Maintain, Admin }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PullRequestState { Open, Closed, Merged }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision { Pending, ChangesRequested, Approved }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReviewComment { pub reviewer: String, pub body: String, pub at: Timestamp }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PullRequestView {
    pub number: u64, pub state: PullRequestState, pub review: ReviewDecision,
    pub change_requests: Vec<ReviewComment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_commit: Option<String>,
    pub head: String, pub base: String, pub url: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 2C domain `PullRequestRef { number, url, state }` is used as is (no type of this name is defined here).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BranchReviewRules { pub review_required: bool, pub code_owner_review_required: bool, pub required_approvals: u32 }
pub struct OpenPullRequest { pub host: String, pub repository: String, pub head: String, pub base: String,
                             pub title: String, pub body: String, pub labels: Vec<String>, pub reviewers: Vec<String> }
pub struct BranchWithFiles { pub host: String, pub repository: String, pub base: String, pub branch: String,
                             pub message: String, pub files: Vec<(String /* repository path */, Vec<u8>)> }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReviewRequest { pub host: String, pub repository: String, pub number: u64, pub head: String }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict { Approve, RequestChanges }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GithubPerson { pub login: String, pub name: Option<String>, pub email: Option<String> }
/// A folder of a remote repository; `folder` is relative to the repository root, empty for the root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RepoFolderRef { pub host: String, pub repository: String, pub folder: String }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RepoFile { pub name: String, pub last_changed_by: String, pub last_changed_at: Timestamp, pub commit: String }

#[derive(Debug, Clone, thiserror::Error)]
pub enum GithubError {
    #[error("GH_SIGN_IN_NEEDED")] SignInNeeded,
    #[error("GH_NOT_FOUND")] NotFound,                       // not found or no access
    #[error("GH_RATE_LIMITED")] RateLimited { retry_after_s: u32 },
    #[error("GH_UNREACHABLE")] Unreachable,
    #[error("GH_CONFLICT")] Conflict { current_sha: String }, // update_file with a stale sha
    #[error("GH_HOST_NOT_SUPPORTED")] HostNotSupported,      // a host that is neither GitHub nor the company's GitLab
    #[error("GH_FAILED")] Other(String),                     // the tool's first error line, no token
}
impl GithubError { pub fn code(&self) -> &'static str; pub fn retryable(&self) -> bool; }

#[async_trait::async_trait]
pub trait GithubAccess: Send + Sync {
    // credentials (Catalogue Service, Push Service: git2 fetch and push of their own clones)
    async fn git_credentials(&self, host: &str) -> Result<GitCredential, GithubError>;
    // repository facts (Catalogue Service, Handover Service)
    async fn repository_permission(&self, host: &str, repository: &str) -> Result<RepoPermission, GithubError>;
    async fn default_branch(&self, host: &str, repository: &str) -> Result<String, GithubError>;
    async fn commit_verified(&self, host: &str, repository: &str, sha: &str) -> Result<bool, GithubError>;
    async fn branch_review_rules(&self, host: &str, repository: &str, branch: &str) -> Result<BranchReviewRules, GithubError>;
    // pull requests (Catalogue Service, Push Service, Handover Service)
    async fn pull_request(&self, host: &str, repository: &str, number: u64) -> Result<PullRequestView, GithubError>;
    /// Returns the 2C `PullRequestRef` with `state = Open`.
    async fn open_pull_request(&self, req: OpenPullRequest) -> Result<PullRequestRef, GithubError>;
    /// The open or most recent pull request whose head is `head`; `None` when there is none (Push Service recovery).
    async fn pull_request_for_branch(&self, host: &str, repository: &str, head: &str) -> Result<Option<PullRequestRef>, GithubError>;
    async fn create_branch_with_files(&self, req: BranchWithFiles) -> Result<String /* commit */, GithubError>;
    async fn review_requests(&self, label: &str) -> Result<Vec<ReviewRequest>, GithubError>;
    async fn review(&self, host: &str, repository: &str, number: u64, verdict: ReviewVerdict, body: &str) -> Result<(), GithubError>;
    async fn merge(&self, host: &str, repository: &str, number: u64) -> Result<bool /* merged */, GithubError>;
    async fn close(&self, host: &str, repository: &str, number: u64) -> Result<(), GithubError>;
    async fn reviewers(&self, host: &str, repository: &str, query: &str) -> Result<Vec<GithubPerson>, GithubError>;
    // files on a branch without a local clone (Handover Service; Ordin Host X-15, GitHub and GitLab)
    async fn file_at(&self, host: &str, repository: &str, git_ref: &str, path: &str) -> Result<(Vec<u8>, String /* blob sha */), GithubError>;
    async fn update_file(&self, host: &str, repository: &str, branch: &str, path: &str, bytes: Vec<u8>,
                         expected_sha: &str, message: &str) -> Result<String /* commit */, GithubError>;
    async fn list_files(&self, folder: &RepoFolderRef, glob: &str) -> Result<Vec<RepoFile>, GithubError>;
    async fn read_file(&self, folder: &RepoFolderRef, name: &str) -> Result<(Vec<u8>, String /* commit */), GithubError>;
    async fn file_last_commit(&self, folder: &RepoFolderRef, name: &str) -> Result<String, GithubError>;
}
impl GithubAccess for ProjectService { /* Internal Logic 14 */ }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProjectCheckResult { pub command: String, pub exit_code: Option<i32>, pub output: String, pub truncated: bool, pub elapsed_ms: u64 }

#[derive(Debug, Clone, thiserror::Error)]
pub enum ProjectError {
    #[error("PROJ_NOT_FOUND")] NotFound,
    #[error("PROJ_NOT_A_REPOSITORY")] NotARepository,
    #[error("PROJ_PLACE_ALREADY_OPEN")] PlaceAlreadyOpen,
    #[error("PROJ_SIGN_IN_NEEDED")] SignInNeeded,
    #[error("PROJ_REMOTE_UNREACHABLE")] RemoteUnreachable,
    #[error("PROJ_RATE_LIMITED")] RateLimited,
    #[error("PROJ_ORIGIN_CHANGED")] OriginChanged { remote_commit: String },
    #[error("PROJ_PUSH_REFUSED")] PushRefused,
    #[error("PROJ_NOTHING_TO_COMMIT")] NothingToCommit,
    #[error("PROJ_UNDECIDED_CHANGES")] UndecidedChanges,
    #[error("PROJ_BRANCH_EXISTS")] BranchExists,
    #[error("PROJ_WORKING_COPY_DIRTY")] WorkingCopyDirty,
    #[error("PROJ_TOO_MANY_ATTEMPTS")] TooManyAttempts,
    #[error("PROJ_TASK_NOT_FOUND")] TaskNotFound,
    #[error("PROJ_TASK_STATE_INVALID")] TaskStateInvalid,
    #[error("PROJ_WORKTREE_FAILED")] WorktreeFailed,
    #[error("PROJ_ENTRY_NOT_FOUND")] EntryNotFound,
    #[error("PROJ_DECISION_INVALID")] DecisionInvalid,
    #[error("PROJ_CHECK_DENIED")] CheckDenied { reason: String },
    #[error("PROJ_CHECK_TIMEOUT")] CheckTimeout,
    #[error("PROJ_LSP_UNAVAILABLE")] LspUnavailable,
    #[error("PROJ_LSP_METHOD_NOT_ALLOWED")] LspMethodNotAllowed,
    #[error("PROJ_SERVER_INSTALL_FAILED")] ServerInstallFailed,
    #[error("PROJ_IDE_NOT_FOUND")] IdeNotFound,
    #[error("PROJ_IDE_START_FAILED")] IdeStartFailed,
    #[error("PROJ_TERMINAL_FAILED")] TerminalFailed,
    #[error("PROJ_GIT_FAILED")] GitFailed,
    #[error("PROJ_FILES_FAILED")] FilesFailed,
    #[error("PROJ_TOO_MANY_FOLLOWED")] TooManyFollowed,
    #[error("PROJ_DATASTORE_UNAVAILABLE")] DatastoreUnavailable,
    #[error("{0}")] Invalid(&'static str),   // CODE_*, CSET_*, GHOP_*, LANG_*, PROJ_CHECKS_INVALID, PROJ_BRANCH_MISMATCH
}
impl ProjectError { pub fn code(&self) -> &'static str; pub fn retryable(&self) -> bool; }
```

### Callers of the public interface (reconciliation §3)

| Item | Called by |
|---|---|
| `open_project`, `close_project` | program (on the Workspace Service's journal entry `source_link_added` of kind `git_repository_folder`, and on its removal); Service API `projects.open`, `projects.close` |
| `workspace_opened` | program, when the Workspace Service's `opened()` reports a workspace |
| `set_check_commands` | Service API `projects.set_check_commands`, from the project's settings screen (SA-CLOSE-03) |
| `start_coding_task` | Ordin Host, when it hands a coding task to an agent (`delegate` with `code_in`, ADR-053); the returned task's id reaches the session as `StartRun.coding_task_id`, through which the session is granted the worktree place |
| `project_for_place` | Ordin Host (`delegate` with `code_in`) |
| `end_coding_run` | program, from its `SupervisorHost::run_ended` (SessionSupervisor.md Internal Logic 6) |
| `review` | Service API `projects.review` (failing tests and new problems as a shortfall, ADR-054) |
| `follow_original`, `unfollow_original`, `subscribe_original_changes`, `read_at_commit`, `remote_state`, `default_branch(place)` | Push Service |
| `workspace_languages` | program, filling the Package Catalogue's and the Resolver and Compiler's workspace languages |
| `GithubAccess` (every method) | Catalogue Service, Push Service, Handover Service, Ordin Host (`list_files`, `read_file`, `file_last_commit`), Team Knowledge Service (`git_credentials`, `repository_permission`, `branch_review_rules`, through the program's implementation of its `TeamRepos`; amended January 2026, ADR-103) |
| `tool_provider()` (MCP servers `code`, `github`, `gitlab`, `checks`) | the program registers it on the MCP Front |
| `migrations()` | the program, with the Journal's `MigrationRegistry` |
| Service API commands below | Service API catalogue (screens) |

### MCP servers served (registered with the MCP Front; names *chosen here* except `code`, SA-NAME-04)

| Server | Tool | Arguments (JSON, `deny_unknown_fields`) | Offered to |
|---|---|---|---|
| `code` | `definition`, `references`, `hover` | `{ "path": string (relative to the worktree), "line": u32 (0-based), "character": u32 }` | sessions on a coding task (`coding_task_id` set) |
| `code` | `symbols` | `{ "path": string? }` (absent = the workspace's symbols, `query` empty) | same |
| `code` | `diagnostics` | `{ "path": string? }` | same |
| `github` | `gh` | `{ "args": [string] }` (1–64 items, each ≤ 4,000 chars) | sessions in a workspace linking a GitHub repository, when the package declares it |
| `gitlab` | `glab` | `{ "args": [string] }` | as `github`, for GitLab |
| `checks` | `run_check` | `{ "command": string (1–1,000) }` | sessions on a coding task |

The MCP Front's Rules Engine decision precedes every call, including the kind classification of `gh`/`glab` arguments (ADR-023) and the check-command policy (ADR-021: commands that commit, push, deploy, publish or change a shared database are refused).

### Service API commands (*chosen here*; screens: Code group, *Changes waiting*, editor, terminal, Settings › Code)

`projects.open`, `projects.close`, `projects.status`, `projects.diff`, `projects.history`, `projects.branches`, `projects.create_branch`, `projects.switch_branch`, `projects.commit`, `projects.push`, `projects.save_file`, `projects.review`, `projects.decide_entry`, `projects.keep_attempt`, `projects.run_check`, `projects.git_host_settings`, `projects.set_git_host_approval`, `projects.language_plugins`, `projects.configure_language`, `projects.restart_language_server`, `projects.editor_open`, `projects.lsp_send`, `projects.editor_close`, `projects.ides`, `projects.set_ide_enabled`, `projects.open_in_ide`, `projects.terminal_open`, `projects.terminal_input`, `projects.terminal_resize`, `projects.terminal_close`, `projects.check_now` (`fetch_followed` for one original), `projects.set_check_commands` (`set_check_commands`, the project settings screen; scope `Any`, SA-CLOSE-03). These are the catalogue's names (R1); the engineer's commit is `projects.commit` then `projects.push`, with no approval card. Inputs and results are the method parameters and return types above. `terminal_*` and `lsp_send` are desktop-only (their bytes travel on the `StreamSink`, never as journalled events; *assumed here*: the personal server offers no terminal over the Remote API in the first release).

### Example

```rust
let started = projects.start_coding_task(StartCodingTask {
    project_id, origin: RunOrigin::Task { task_id }, agent_instance_id: coder,
    request: "Add retry to the rates client".into(), task_name: "rates-retry".into(), attempt_group_id: None,
}).await?;
// CodingTask { branch: "ordin/rates-retry", base_branch: "main", base_commit: "a1b2c3…(40 hex)",
//   worktree_path: "/Users/asha/Library/Application Support/ordin/worktrees/0192…/0192…", state: Working, .. }

let cs = projects.end_coding_run(started.task.id, run_id).await?;
// ChangeSet { entries: [ChangeSetEntry { file: "src/rates.rs", hunk: "@@ -40,6 +40,14 @@ …", state: Pending }], state: InReview }

let outcome = projects.decide_entry(cs.entries[0].id, EntryDecision::Accept).await?;
// DecisionOutcome::Applied { file_version_id }  — or Conflict { yours, agents } when the engineer changed the same lines
```

`code.definition` call from a session: `{"path":"src/rates.rs","line":41,"character":12}` → `{"locations":[{"path":"src/http.rs","line":88,"character":4}]}`.

## Internal Logic

### 1. Start

1. The program has registered `tool_provider()` with the MCP Front before `start`; nothing is registered here.
2. Reconcile language plugins and git-host settings with the current `ProjectSettings` (steps 9.1, 8.1) and subscribe to changes.
3. Crash recovery: tasks `settled` or `set_aside` whose worktree folder still exists → remove worktree and branch (step 5.8); tasks `working` whose run ended while ordin was closed stay `working` until the program calls `end_coding_run` from `SupervisorHost::run_ended` (Interaction).
4. Run the start fetch of every followed original (step 4) and start the `FETCH_INTERVAL` timer.

### 2. Projects

1. `open_project(place, workspace)`: `files.place` (kind `persons_folder`); detect git with `git2::Repository::open` (no repository → a project with `is_git_repository = false`, still usable by the editor); a non-closed project on the place → `PROJ_PLACE_ALREADY_OPEN`; `current_branch` from `HEAD` (detached → the short commit id); `check_commands` starts empty (no checks until the engineer sets them, SA-CLOSE-03); insert `opened`; `files.watch([place])`; `tool_registry.tools_changed` for `github` and `gitlab` (the workspace's offer may change); when `files.coverage(&[place])` reports `indexed == total`, set `indexed` (the File Service indexes it, 02:762 "indexes its files for the tree and search"); checked after each `PlaceChange` batch for the place, at most once a minute.
2. Watch the engineer's working copy: subscribe to `PlaceChange` for open projects; a change with `by_ordin = false` to a file with a pending change-set entry marks the entry for conflict checking at decision time (ADR-052).
3. `close_project`: `closed`; stop the project's editor language-server instances; `files.unwatch([place])` unless the place is held by a workspace; `tool_registry.tools_changed` for `github` and `gitlab`.
4. `workspace_languages(workspace_id)`: for each project of the workspace not `closed`, test its folder root for marker files: `go.mod` → `go`; `Cargo.toml` → `rust`; `pom.xml`, `build.gradle` or `build.gradle.kts` → `java`; `tsconfig.json` → `typescript` (*chosen here*: the four first languages of ADR-024 and ADR-056). Return the distinct names sorted; no project → empty list.
5. `project_for_place(place_id)`: select the project on `place_id` whose state is not `closed` (at most one, step 1); none → `Ok(None)`. A store error → `PROJ_DATASTORE_UNAVAILABLE`.
6. `set_check_commands(project_id, commands)` (Service API `projects.set_check_commands`, SA-CLOSE-03): project unknown or `closed` → `PROJ_NOT_FOUND`; each command trimmed, empty ones dropped, duplicates removed keeping the first; more than 20, or one over 1000 chars → `PROJ_CHECKS_INVALID` and nothing stored; store the list as a JSON array in `projects.check_commands` (an empty list clears the checks); a store error → `PROJ_DATASTORE_UNAVAILABLE`; journal `check_commands_set {project_id, count}` (the commands themselves are not journalled); return the updated `Project`.

### 3. Credentials and remotes

1. Remote host from `origin`'s URL: `github.com` → `CredentialKind::Github`; `settings.gitlab_host` or `gitlab.com` → `CredentialKind::Gitlab` (D9; SA-CLOSE-01); other hosts → `PROJ_REMOTE_UNREACHABLE` with log `unsupported_host` (*assumed here*: ADR-051 names GitHub and GitLab only).
2. `git2::RemoteCallbacks::credentials` returns `Cred::userpass_plaintext("x-access-token", token)` for GitHub (`oauth2` user for GitLab); the token comes from `identity.access_token(kind, None)` per operation (`token.expose_secret()`) and is dropped after it; `CRED_SIGN_IN_REQUIRED` → `PROJ_SIGN_IN_NEEDED`, any other `CredError` → `PROJ_REMOTE_UNREACHABLE` when retryable, else `PROJ_GIT_FAILED`. HTTPS only; SSH remotes → `PROJ_REMOTE_UNREACHABLE` (*assumed here*: keys are not in ordin's keychain model).
3. Network operations run on a blocking thread with `GIT_NETWORK_TIMEOUT`; an HTTP 403/429 with a rate-limit header → `PROJ_RATE_LIMITED`.

### 4. Originals in repositories (ADR-069, A-36)

1. `follow_original(place, path, branch)`: at most `MAX_FOLLOWED_REPOSITORIES` distinct places (else `PROJ_TOO_MANY_FOLLOWED`); insert `followed_originals`; the first fetch records the current blob id of the path as `last_blob`.
2. Triggers: `workspace_opened(places)`, start, every `FETCH_INTERVAL` (per repository, doubled while rate-limited, reset after a success), `projects.check_now`.
3. `fetch_followed(place, branch)`: fetch only `refs/heads/<branch>:refs/remotes/origin/<branch>` (no checkout, no change to the working copy or `HEAD`); set `last_checked_at` (shown as "Last checked GitHub at 10:15", 11 §5.6) or `last_error`.
4. For each followed path on that branch: blob id at the new remote tip ≠ `last_blob` → find the newest commit between the previous tip and the new tip that touched the path (`git2` revwalk, first-parent) → `RepositoryOriginChange { commit, author (name), committed_at }`; broadcast it; update `last_blob`, `last_commit`. A commit on another branch changes no copy (ADR-069).
5. `read_at_commit`: the blob at `<commit>:<path>` from the object store, never the working copy (02:761 "without changing the person's working copy"). The Push Service calls it to remake copies of a Git original.
6. `remote_state`: fetch as step 3 and return the tip; `default_branch`: the remote `HEAD` symbolic ref after a fetch, else `main`.
7. This service does not commit or push changes to Git originals: the Push Service does that itself after the person's approval (ruling R12, ADR-029), taking its credential from `GithubAccess::git_credentials`. This service commits only inside coding-task worktrees and the engineer's own `commit` and `push` (section 10).

### 5. Coding tasks and worktrees (ADR-053)

1. `start_coding_task`: project must be a git repository; `request` 1–4000 (`CODE_REQUEST_INVALID`); `task_name` slugged to `[a-z0-9-]{1,100}` (lower-case, other characters to `-`, collapsed); branch `ordin/<slug>`; an existing branch of that name → append `-2`, `-3` … (`CODE_BRANCH_INVALID` if still invalid).
2. Attempts: with `attempt_group_id`, the group's tasks counted; ≥ `settings.max_attempts` → `PROJ_TOO_MANY_ATTEMPTS`. A group is valid only with ≥ 2 tasks once the second starts (`CODE_GROUP_TOO_SMALL` checked when `keep_attempt` is called on a one-task group).
3. Base: the engineer's current branch and its `HEAD` commit (`base_branch`, `base_commit`, 40 hex).
4. Create the branch at `base_commit`, then `Repository::worktree(<task id>, WORKTREE_ROOT, opts.reference(branch))`; the path must be under the data root (`CODE_WORKTREE_OUTSIDE_ROOT`). Failure → `PROJ_WORKTREE_FAILED` and the branch is deleted again.
5. `files.add_place(kind = coding_task_worktree, friendly_name = branch)` → `worktree_place_id`; the session whose `StartRun.coding_task_id` is this task is granted this place, read and change, as its only place in the repository (SA-RECON-08). Insert the task `working`; journal `coding_task_started {task_id, project_id, branch}`.
6. `end_coding_run(task, run_id)`: `working` → `ready_for_review` only if the worktree differs from `base_commit` (else the task is `settled` with no change set and cleaned up). Diff: `git2` diff of `base_commit`'s tree to the worktree's working directory including untracked files, binary files listed as one entry with the hunk `Binary file changed` (cannot be accepted partially); one `ChangeSetEntry` per file per hunk, each hunk text ≤ 1 MiB (`CSET_HUNK_TOO_LARGE` → that file becomes one entry "too large to review here" that can only be accepted or rejected whole). Insert the change set `in_review` (`CSET_DUPLICATE` guards one per task). Then step 6.
7. Test outcome: when the project has check commands, run them in the worktree (step 7) with origin `Run`; `test_outcome = Passed` when every exit code is 0, else `Failed`. With none set (SA-CLOSE-03) no check runs and `test_outcome` stays `None` (the screens show "No checks set").
8. Cleanup on `settled` or `set_aside`: `files.remove_place`, prune the worktree (git2 `Worktree::prune` with valid and working-tree flags), delete the local branch, delete the base worktree if any. Journal `coding_task_removed`.

### 6. New problems (ADR-054, SA-LSP-03)

1. For each language with an enabled server among the changed files: start (or reuse) the task's instance on the worktree and a transient instance on a read-only worktree at the merge base (`BASE_WORKTREE_ROOT`, detached `HEAD`); open the changed files in both; wait `DIAGNOSTICS_SETTLE`; collect diagnostics.
2. A task diagnostic is *new* when no base diagnostic has the same file path, code (message when no code) and severity within the same enclosing symbol (from `textDocument/documentSymbol`, innermost range containing the diagnostic's start). Store them; `new_problems` = their count. Stop the base instance and remove the base worktree.
3. A task with failing tests or new problems is a shortfall shown to the person, never as done (ADR-054): `review` returns both, read through Service API `projects.review` (Interaction).

### 7. Check commands (ADR-021, ADR-053)

1. Agent: tool `checks.run_check` (already decided by the MCP Front's `decide_tool_call`). Person, and checks this service runs for a task (step 5.7): `rules.decide_command(CommandRequest { workspace_id, origin, command, working_dir, declared_check })` with `declared_check` true when `command` is in the project's `check_commands`; `RulesOutcome.decision` deny → `PROJ_CHECK_DENIED { reason }`; ask → `rules.wait_for_approval(approval_id, Some(now + CHECK_TIMEOUT))`: `Approved` runs it, `Rejected` or `Expired` → `PROJ_CHECK_DENIED`, `Pending` (deadline reached) → `PROJ_CHECK_TIMEOUT`.
2. Run in the task's worktree (or the project folder for the person without a task) with the platform shell as an argument list (`/bin/sh -c <command>`; Windows `cmd.exe /d /s /c <command>`), environment cleared of credential variables (`GH_TOKEN`, `GITHUB_TOKEN`, `GITLAB_TOKEN`, `ANTHROPIC_API_KEY` and 2D-1c's list), `CHECK_TIMEOUT` (kill the process tree → `PROJ_CHECK_TIMEOUT`), output captured up to `CHECK_OUTPUT_MAX`.
3. Output goes to the agent or to the conversation (11 §5.11), never to the engineer's terminal (ADR-022). Journal `check_run {project_id, task_id?, exit_code, elapsed_ms}` without the output (07 *Logging*: no tool output in logs; the journal keeps the digest).

### 8. GitHub and GitLab tools and approval settings (ADR-023, ADR-051)

1. Settings rows: on start and on each profile change, ensure one row per (project, kind) for each SA-GH-01 kind: `is_write = kind != "read"`; default `approval_required = is_write`; a kind in `always_approved_kinds` is forced to `approval_required = true, set_by = organisation_profile` (04:139). Push each project's rows to `rules.replace_git_host_settings(project.place_id, rows as GitHostSetting { kind: operation_kind, approval_required })`, also after each engineer's change.
2. `set_git_host_approval(project, kind, false)` for a kind in the ceiling → `GHOP_ABOVE_CEILING`; `read` cannot be set to require approval (*assumed here*; SA-GH-01 "Reads never need approval"). Journal `git_host_approval_set {project_id, kind, approval_required}`.
3. Tool `github.gh` (after the MCP Front's decision): run `gh` (installed by the Setup Assistant) with the arguments as an argument list in the task worktree or the project folder, `GH_TOKEN` from Identity, `GH_PROMPT_DISABLED=1`, `NO_COLOR=1`, `GH_TIMEOUT`; output (≤ 1 MiB) returned as the tool result. `gitlab.glab` the same with `GITLAB_TOKEN` and `GITLAB_HOST`. Log `git_host_call {server, exit_code}` (no arguments; SA-RECON-07).

### 9. Language plug-ins and the editor (ADR-024, ADR-055, ADR-056)

1. Reconcile `language_plugins` with `settings.language_servers`: new languages `available`; `approved_servers` and `default_server` updated; a `chosen_server` no longer approved is cleared (`LANG_SERVER_NOT_APPROVED` guards writes); languages withdrawn from the profile → `removed`.
2. `configure_language`: `server_settings` must be a JSON object ≤ 64 KiB (`LANG_SETTINGS_INVALID`; "invalid JSON is refused and nothing is saved", ADR-055); `available → added`; running instances of the language get `workspace/didChangeConfiguration` with the settings, or are stopped when `enabled = false` (highlighting stays in the editor). Journal `language_configured {language, enabled}`.
3. Start an instance (first file of the language opened, or a task needing it): `installer.ensure(language, pin)` (first use: `added → installed`; failure → `PROJ_SERVER_INSTALL_FAILED`, the editor shows "highlighted · no language server"); spawn with stdio pipes, cwd the project folder or worktree, cleared credential variables; `initialize` with `rootUri`, `initializationOptions` = settings, within `LSP_START_TIMEOUT`; `initialized`. Instances: one per (project, language) for the editor, one per (coding task, language), the transient base instance; at most `MAX_LANGUAGE_INSTANCES` (the least recently used editor instance is stopped first).
4. Framing: `Content-Length: N\r\n\r\n` + N bytes of UTF-8 JSON, both ways; a malformed frame from a server kills that instance (restart on next use, `language_server_crashed` logged).
5. `editor_open(project, path)`: returns a `StreamId`; the line "Go · gopls · Running" or "highlighted · no language server" (ADR-055). `lsp_send(stream, message)`: only `EDITOR_LSP_METHODS` pass (else `PROJ_LSP_METHOD_NOT_ALLOWED`); document URIs must be inside the project folder; ids are remapped per editor session so several editors share one instance. Server responses and `publishDiagnostics`, `semanticTokens` go back through `StreamSink` as `Lsp`. Server-to-client requests other than `workspace/configuration` (answered with the settings) are answered with an empty result.
6. Code-intelligence tools: map `path` (relative, no `..`) to the task worktree's URI, send `textDocument/definition`, `references` (`includeDeclaration: false`), `hover`, `documentSymbol` / `workspace/symbol`, or return the last `publishDiagnostics` for the path; results mapped to worktree-relative paths; locations outside the worktree are dropped. `LSP_REQUEST_TIMEOUT` → tool error `PROJ_LSP_UNAVAILABLE`. Tools only read (ADR-054).
7. `save_file(project, path, text, expected_hash)`: `files.write_version(origin Person)` into the working copy (ADR-056 "through the File Service, as a new version"); `FILE_CHANGED_MEANWHILE` → `PROJ_FILES_FAILED` with the editor keeping the unsaved text. The file then shows in *Changes waiting* as the engineer's own change (status from step 10).

### 10. Engineer's git views, commit and push

1. `status`: git2 status of the working copy (untracked included, ignored excluded). `diff`: unified diff of the working copy against `HEAD`, for one path or all. `history`: revwalk from `HEAD`, path-filtered, ≤ 200. `branches`: local branches except `ordin/*`. `create_branch` (exists → `PROJ_BRANCH_EXISTS`), `switch_branch` (uncommitted changes that the switch would overwrite → `PROJ_WORKING_COPY_DIRTY`).
2. `commit(project, message)`: refused while any change-set entry of the project is `pending` (`PROJ_UNDECIDED_CHANGES`; 11 §5.11 "Commit is available once every change waiting has been decided"); stage every change in the working copy (the accepted entries and the engineer's own edits), commit with the repository's `user.name`/`user.email`; nothing staged → `PROJ_NOTHING_TO_COMMIT`. `push`: push the current branch without force; rejected → `PROJ_PUSH_REFUSED` (the commit stays local, 03:256 error posture).
3. Reading taken on "Commit, Push and Show history run in the engineer's terminal" (11 §5.11) with in-process git (05:34): when `terminal` is given, ordin writes into that terminal's stream the equivalent command line (`git commit -m "…"`, `git push origin main`, `git log -- <file>`) and the outcome lines ordin produced, marked as written by ordin; no shell runs them and the person's shell session is not used (HLD disagreement recorded). These writes are the engineer's own actions, not an agent's, and are not journalled beyond `git_committed` / `git_pushed` with ids.

### 11. Change review decisions (ADR-053, 11 §5.11)

1. `decide_entry(entry, Reject)` → `rejected`; the engineer's working copy is untouched.
2. `Accept`: ancestor = the file at `base_commit`; theirs = ancestor with this hunk applied (in-crate unified-hunk application; a hunk that does not apply to the ancestor → `PROJ_DECISION_INVALID`); ours = the engineer's working-copy file. Run `git2::merge_file(ancestor, ours, theirs)`; clean → `files.write_version(WriteVersion { origin: Run { run_id, tool: "change_review.accept" }, expected_hash: ours' hash })` → `Applied`, entry `accepted`; conflict → `Conflict { yours, agents }` with the conflicting regions, entry stays `pending` and the screens offer **Keep yours** and **Use the agent's** (ADR-052: never applied over the engineer's change on its own).
3. `KeepYours` → `rejected`. `UseTheAgents` → write theirs-resolved content (ours with the conflicting regions taken from theirs) → `accepted`.
4. When no entry is `pending`: change set `settled` (`CSET_UNDECIDED_ENTRIES` guards), task `settled`, cleanup (step 5.8).
5. `keep_attempt(task)`: the task's group's other tasks → `set_aside` and cleaned up; the kept task stays in review. Journal `attempt_kept {group, task_id}`.

### 12. IDEs (ADR-052, C-32)

1. Detection at `ides()` (cached 10 minutes): macOS — `/Applications` and `~/Applications` (including `~/Applications/JetBrains Toolbox`) for bundles named `IntelliJ IDEA*.app`, `GoLand*.app`, `RustRover*.app`, `WebStorm*.app`, `PyCharm*.app`, `CLion*.app`, `Rider*.app`, `PhpStorm*.app`, `Visual Studio Code.app`; Windows — `%LOCALAPPDATA%\Programs\Microsoft VS Code\Code.exe`, `%ProgramFiles%\Microsoft VS Code\Code.exe`, `%LOCALAPPDATA%\JetBrains\Toolbox\scripts\*.cmd`, `%ProgramFiles%\JetBrains\*\bin\*64.exe`; Linux — `code` on `PATH` and `~/.local/share/JetBrains/Toolbox/scripts/*`.
2. `open_in_ide`: macOS `open -na <bundle> --args <path>`; Windows and Linux the launcher with the path as its argument; argument lists only. A start failure → `PROJ_IDE_START_FAILED`. ordin writes no IDE setting (ADR-052). `set_ide_enabled` stores the Settings › Code › *Open code in* choice.

### 13. Terminal (ADR-022, SA-TERM-01)

1. `terminal_open(project, cols, rows)`: `portable-pty` native PTY (ConPTY on Windows) with the person's shell (`$SHELL`, else `/bin/zsh` on macOS and `/bin/bash` on Linux; `%ComSpec%` on Windows), cwd the project folder; output bytes go to `StreamSink` only.
2. No MCP tool, agent, journal entry or log line receives terminal input or output (ADR-022); `terminal_input` is accepted only from the Service API (the screens), never from the MCP Front. Closing the tab kills the shell.

### 14. Repository hosts in process (`GithubAccess`)

1. Host: `github.com` → GitHub (`gh api`, `GH_TOKEN`); `settings.gitlab_host` or `gitlab.com` → GitLab (`glab api`, `GITLAB_TOKEN`, `GITLAB_HOST`); any other host → `GH_HOST_NOT_SUPPORTED`. Every method except `git_credentials`, `list_files`, `read_file` and `file_last_commit` is GitHub-only and returns `GH_HOST_NOT_SUPPORTED` for GitLab. Tokens come from `identity.access_token(kind, None)` per call (step 3.2); `CRED_SIGN_IN_REQUIRED` → `GH_SIGN_IN_NEEDED`. Each call has `GH_TIMEOUT`, `GH_PROMPT_DISABLED=1`, `NO_COLOR=1`, argument lists only; exit status with an HTTP 404 or 403 without a rate-limit header → `GH_NOT_FOUND`; 403 or 429 with a rate-limit header → `GH_RATE_LIMITED { retry_after_s }`; no connection → `GH_UNREACHABLE`; other failures → `GH_FAILED` with the tool's first error line (never the token).
2. `git_credentials(host)`: `GitCredential { username: "x-access-token" (GitHub) or "oauth2" (GitLab), token }`; no call to the host.
3. GitHub REST requests (`gh api <path>` with `--method` and `-f`/`-F` fields): `repository_permission` — `GET repos/{repository}` → highest true flag of `permissions` (`admin`, `maintain`, `push` → `Write`, `triage`, `pull` → `Read`), no `permissions` → `None`; `default_branch` — the same response's `default_branch`; `commit_verified` — `GET repos/{repository}/commits/{sha}` → `commit.verification.verified`; `branch_review_rules` — `GET repos/{repository}/branches/{branch}/protection` → `required_pull_request_reviews` (`required_approving_review_count`, `require_code_owner_reviews`), a 404 → all false and 0; `pull_request` — `GET repos/{repository}/pulls/{number}` (`state`, `merged`, `merge_commit_sha`, `head.ref`, `base.ref`, `html_url`) and `GET repos/{repository}/pulls/{number}/reviews` (latest review per reviewer: any `CHANGES_REQUESTED` → `ChangesRequested`, else any `APPROVED` → `Approved`, else `Pending`; `change_requests` = the `CHANGES_REQUESTED` bodies); `open_pull_request` — `POST repos/{repository}/pulls` (`title`, `head`, `base`, `body`), then `POST repos/{repository}/issues/{number}/labels` when labels are given and `POST repos/{repository}/pulls/{number}/requested_reviewers` when reviewers are given; `create_branch_with_files` — `GET repos/{repository}/git/ref/heads/{base}` for the base commit, `POST repos/{repository}/git/trees` with `base_tree` and one entry per file (`mode` `100644`, `content`), `POST repos/{repository}/git/commits` with that tree and the base commit as parent, `POST repos/{repository}/git/refs` with `refs/heads/{branch}` (an existing branch → `GH_CONFLICT`); `review_requests(label)` — `GET search/issues?q=is:pr is:open review-requested:@me label:"{label}"`; `review` — `POST repos/{repository}/pulls/{number}/reviews` with `event` `APPROVE` or `REQUEST_CHANGES` and `body`; `merge` — `PUT repos/{repository}/pulls/{number}/merge` → `merged`, a 405 → `false`; `pull_request_for_branch` — `GET repos/{repository}/pulls?head={owner}:{head}&state=all&per_page=1&sort=created&direction=desc` → the first item's `number`, `html_url` and state (`merged_at` set → `Merged`), or `None`; `close` — `PATCH repos/{repository}/pulls/{number}` with `state=closed`; `reviewers(query)` — `GET repos/{repository}/collaborators?per_page=100`, logins containing `query` case-insensitively, at most 10, each completed with `GET users/{login}` (`name`, `email`); `file_at` — `GET repos/{repository}/contents/{path}?ref={git_ref}` → decoded `content` and `sha`; `update_file` — `PUT repos/{repository}/contents/{path}` with `message`, base64 `content`, `sha = expected_sha`, `branch`; a 409 or 422 naming the sha → `GH_CONFLICT { current_sha }` read again with `file_at`.
4. Repository folders (GitHub and GitLab): `list_files(folder, glob)` — GitHub `GET repos/{repository}/contents/{folder}`, GitLab `GET projects/{url-encoded repository}/repository/tree?path={folder}`; keep files whose name matches `glob` (`*` and `?` only, *chosen here*); for each, the last commit touching it — GitHub `GET repos/{repository}/commits?path={folder}/{name}&per_page=1`, GitLab `GET projects/{id}/repository/commits?path=…&per_page=1` — gives `last_changed_by` (author name), `last_changed_at`, `commit`; at most 200 files. `read_file` — GitHub `file_at` on the default branch, GitLab `GET projects/{id}/repository/files/{path}/raw?ref={default branch}`, with the commit from `file_last_commit`. Missing → `GH_NOT_FOUND`.
5. Nothing here is journalled beyond the log line `git_host_api {operation, host_kind, status, elapsed_ms}` (no repository names, paths or content).

## Data Model

SQLite (desktop, personal server).

```sql
CREATE TABLE projects (
    id                       TEXT PRIMARY KEY,
    place_id                 TEXT NOT NULL,
    workspace_id             TEXT NOT NULL,
    display_name             TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 120),
    is_git_repository        INTEGER NOT NULL CHECK (is_git_repository IN (0,1)),
    current_branch           TEXT,
    language_servers_in_use  TEXT NOT NULL DEFAULT '[]',
    check_commands           TEXT NOT NULL DEFAULT '[]',
    state                    TEXT NOT NULL CHECK (state IN ('opened','indexed','closed')),
    opened_at                TEXT NOT NULL,
    CHECK ((is_git_repository = 1) = (current_branch IS NOT NULL))
);
CREATE UNIQUE INDEX projects_open_place ON projects (place_id) WHERE state != 'closed';

CREATE TABLE github_operation_settings (
    id                 TEXT PRIMARY KEY,
    project_id         TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    operation_kind     TEXT NOT NULL,
    is_write           INTEGER NOT NULL,
    approval_required  INTEGER NOT NULL,
    set_by             TEXT NOT NULL CHECK (set_by IN ('organisation_profile','engineer')),
    set_at             TEXT NOT NULL,
    UNIQUE (project_id, operation_kind)
);

CREATE TABLE language_plugins (
    id                TEXT PRIMARY KEY,
    language          TEXT NOT NULL UNIQUE CHECK (length(language) BETWEEN 1 AND 40),
    approved_servers  TEXT NOT NULL,
    default_server    TEXT NOT NULL,
    chosen_server     TEXT,
    pinned_version    TEXT,
    source            TEXT NOT NULL CHECK (source IN ('agent_package','organisation_profile')),
    enabled           INTEGER NOT NULL,
    server_settings   TEXT NOT NULL DEFAULT '{}' CHECK (length(server_settings) <= 65536),
    status            TEXT NOT NULL CHECK (status IN ('available','added','installed','removed'))
);

CREATE TABLE coding_tasks (
    id                 TEXT PRIMARY KEY,
    origin             TEXT NOT NULL,                     -- JSON RunOrigin
    project_id         TEXT NOT NULL REFERENCES projects(id),
    agent_instance_id  TEXT NOT NULL,
    request            TEXT NOT NULL CHECK (length(request) BETWEEN 1 AND 4000),
    worktree_path      TEXT NOT NULL,
    worktree_place_id  TEXT NOT NULL,
    branch             TEXT NOT NULL,
    base_branch        TEXT NOT NULL,
    base_commit        TEXT NOT NULL CHECK (length(base_commit) = 40),
    attempt_group_id   TEXT,
    test_outcome       TEXT CHECK (test_outcome IN ('passed','failed')),
    new_problems       INTEGER NOT NULL DEFAULT 0,
    new_problem_list   TEXT NOT NULL DEFAULT '[]',        -- JSON Vec<Problem>
    state              TEXT NOT NULL CHECK (state IN ('working','ready_for_review','settled','set_aside'))
);
CREATE INDEX coding_tasks_group ON coding_tasks (attempt_group_id);

CREATE TABLE change_sets (
    id              TEXT PRIMARY KEY,
    coding_task_id  TEXT NOT NULL UNIQUE REFERENCES coding_tasks(id),
    run_id          TEXT NOT NULL,
    project_id      TEXT NOT NULL,
    state           TEXT NOT NULL CHECK (state IN ('open','in_review','settled')),
    created_at      TEXT NOT NULL
);
CREATE TABLE change_set_entries (
    id             TEXT PRIMARY KEY,
    change_set_id  TEXT NOT NULL REFERENCES change_sets(id) ON DELETE CASCADE,
    position       INTEGER NOT NULL,
    file           TEXT NOT NULL,
    hunk           TEXT NOT NULL CHECK (length(hunk) <= 1048576),
    state          TEXT NOT NULL CHECK (state IN ('pending','accepted','rejected'))
);

CREATE TABLE followed_originals (
    place_id     TEXT NOT NULL,
    path         TEXT NOT NULL,
    branch       TEXT NOT NULL,
    last_blob    TEXT,
    last_commit  TEXT,
    PRIMARY KEY (place_id, path, branch)
);
CREATE TABLE repository_fetch_state (
    place_id         TEXT NOT NULL,
    branch           TEXT NOT NULL,
    remote_commit    TEXT,
    last_checked_at  TEXT,
    last_error       TEXT,
    interval_s       INTEGER NOT NULL DEFAULT 900,
    PRIMARY KEY (place_id, branch)
);
CREATE TABLE ide_choices (
    ide_id   TEXT PRIMARY KEY,
    enabled  INTEGER NOT NULL
);
```

Rollback: SA-XC-07 backup. Worktrees and `ordin/*` branches are created and removed only by this module, under the data root and in the local repository; the engineer's branches, commits and working copy are changed only by the engineer's commit, push, branch actions and accepted entries.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Project id unknown | `Err(permanent)` | `PROJ_NOT_FOUND` | `{"error":"That project is no longer open.","code":"PROJ_NOT_FOUND","action":null}` |
| Git action on a folder that is not a repository | `Err(permanent)` | `PROJ_NOT_A_REPOSITORY` | `{"error":"This folder isn't a code repository.","code":"PROJ_NOT_A_REPOSITORY","action":null}` |
| Place already open as a project | `Err(permanent)` | `PROJ_PLACE_ALREADY_OPEN` | — (callers reuse the open project) |
| No GitHub or GitLab sign-in | `Err(permanent)` until sign-in | `PROJ_SIGN_IN_NEEDED` | `{"error":"Sign in to GitHub to reach this repository.","code":"PROJ_SIGN_IN_NEEDED","action":"sign_in_again"}` |
| Remote unreachable, unsupported host, SSH remote | `Err(retryable)` | `PROJ_REMOTE_UNREACHABLE` | `{"error":"Can't reach the repository right now.","code":"PROJ_REMOTE_UNREACHABLE","action":"try_again"}` |
| Host rate limit | `Err(retryable)` | `PROJ_RATE_LIMITED` | `{"error":"GitHub asked ordin to check less often. ordin will try again later.","code":"PROJ_RATE_LIMITED","action":null}` |
| Remote moved since the files were read | `Err(permanent)` for this push | `PROJ_ORIGIN_CHANGED` | `{"error":"Someone changed this on GitHub after ordin read it.","code":"PROJ_ORIGIN_CHANGED","action":null}` |
| Push rejected (permission, protected branch, non-fast-forward) | `Err(retryable)` | `PROJ_PUSH_REFUSED` | `{"error":"GitHub refused the push. Your commit is kept on this computer.","code":"PROJ_PUSH_REFUSED","action":"try_again"}` |
| Nothing to commit | `Err(permanent)` | `PROJ_NOTHING_TO_COMMIT` | `{"error":"There's nothing to commit.","code":"PROJ_NOTHING_TO_COMMIT","action":null}` |
| Commit while changes wait for a decision | `Err(permanent)` | `PROJ_UNDECIDED_CHANGES` | `{"error":"Decide every change waiting before you commit.","code":"PROJ_UNDECIDED_CHANGES","action":null}` |
| Branch name taken | `Err(permanent)` | `PROJ_BRANCH_EXISTS` | `{"error":"A branch with that name already exists.","code":"PROJ_BRANCH_EXISTS","action":null}` |
| Switch would overwrite uncommitted changes | `Err(permanent)` | `PROJ_WORKING_COPY_DIRTY` | `{"error":"Commit or discard your changes before switching branch.","code":"PROJ_WORKING_COPY_DIRTY","action":null}` |
| Attempts above the company's cap | `Err(permanent)` | `PROJ_TOO_MANY_ATTEMPTS` | `{"error":"Your company allows at most 3 attempts at one request.","code":"PROJ_TOO_MANY_ATTEMPTS","action":null}` |
| Task id unknown / wrong state for the call | `Err(permanent)` | `PROJ_TASK_NOT_FOUND`, `PROJ_TASK_STATE_INVALID` | — (internal) |
| Worktree or branch could not be created or removed | `Err(retryable)` | `PROJ_WORKTREE_FAILED` | `{"error":"ordin couldn't prepare a copy of the repository for this task.","code":"PROJ_WORKTREE_FAILED","action":"try_again"}` |
| Entry id unknown / hunk no longer applies | `Err(permanent)` | `PROJ_ENTRY_NOT_FOUND`, `PROJ_DECISION_INVALID` | `{"error":"This change can no longer be applied.","code":"PROJ_DECISION_INVALID","action":null}` |
| Check command denied by the rules or the person | `Err(permanent)` | `PROJ_CHECK_DENIED` | `{"error":"This command isn't allowed here.","code":"PROJ_CHECK_DENIED","action":null}` |
| Check command over 30 minutes | `Err(permanent)` | `PROJ_CHECK_TIMEOUT` | `{"error":"The check took too long and was stopped.","code":"PROJ_CHECK_TIMEOUT","action":null}` |
| Language server off, failed, or not confirmed for worktrees (C-33) | `Err(retryable)` | `PROJ_LSP_UNAVAILABLE` | tool result `{"error":"Code intelligence isn't available for this language.","code":"PROJ_LSP_UNAVAILABLE"}` |
| Editor sent an LSP method outside the list | `Err(permanent)` | `PROJ_LSP_METHOD_NOT_ALLOWED` | — (internal) |
| Language server could not be installed | `Err(retryable)` | `PROJ_SERVER_INSTALL_FAILED` | `{"error":"ordin couldn't install the language server.","code":"PROJ_SERVER_INSTALL_FAILED","action":"send_to_it"}` |
| IDE not found / failed to start | `Err(permanent)` / `Err(retryable)` | `PROJ_IDE_NOT_FOUND`, `PROJ_IDE_START_FAILED` | `{"error":"ordin couldn't open the IDE.","code":"PROJ_IDE_START_FAILED","action":"try_again"}` |
| Terminal could not start | `Err(retryable)` | `PROJ_TERMINAL_FAILED` | `{"error":"ordin couldn't open a terminal.","code":"PROJ_TERMINAL_FAILED","action":"try_again"}` |
| Other git2 error | `Err(permanent)` | `PROJ_GIT_FAILED` | `{"error":"Something went wrong with the repository.","code":"PROJ_GIT_FAILED","action":"send_to_it"}` |
| File Service write refused | `Err(retryable)` | `PROJ_FILES_FAILED` | `{"error":"ordin couldn't save the file.","code":"PROJ_FILES_FAILED","action":"try_again"}` |
| `GithubAccess`: no sign-in for the host | `Err(permanent)` until sign-in | `GH_SIGN_IN_NEEDED` | `{"error":"Sign in to GitHub to reach this repository.","code":"GH_SIGN_IN_NEEDED","action":"sign_in_again"}` |
| `GithubAccess`: repository, file or pull request not found or not visible | `Err(permanent)` | `GH_NOT_FOUND` | `{"error":"ordin can't find that in the repository, or you can't see it.","code":"GH_NOT_FOUND","action":null}` |
| `GithubAccess`: host rate limit | `Err(retryable)` | `GH_RATE_LIMITED` | `{"error":"GitHub asked ordin to slow down. ordin will try again shortly.","code":"GH_RATE_LIMITED","action":"try_again"}` |
| `GithubAccess`: host unreachable | `Err(retryable)` | `GH_UNREACHABLE` | `{"error":"Can't reach the repository right now.","code":"GH_UNREACHABLE","action":"try_again"}` |
| `GithubAccess`: file changed since its sha was read, or branch exists | `Err(permanent)` for this write | `GH_CONFLICT` | — (internal; the caller re-reads and retries, A-44) |
| `GithubAccess`: host is neither GitHub nor the company's GitLab, or the method is GitHub-only | `Err(permanent)` | `GH_HOST_NOT_SUPPORTED` | `{"error":"ordin works with GitHub and your company's GitLab only.","code":"GH_HOST_NOT_SUPPORTED","action":null}` |
| `GithubAccess`: any other tool failure | `Err(permanent)` | `GH_FAILED` | `{"error":"Something went wrong talking to the repository host.","code":"GH_FAILED","action":"send_to_it"}` |
| More than 20 followed repositories | `Err(permanent)` | `PROJ_TOO_MANY_FOLLOWED` | `{"error":"ordin follows at most 20 repositories for copies.","code":"PROJ_TOO_MANY_FOLLOWED","action":null}` |
| Own tables unreachable | `Err(retryable)` | `PROJ_DATASTORE_UNAVAILABLE` | `{"error":"ordin couldn't reach its own records.","code":"PROJ_DATASTORE_UNAVAILABLE","action":"try_again"}` |
| 2C rule broken | `Err(permanent)` | `CODE_*`, `CSET_*`, `GHOP_*`, `LANG_*`, `PROJ_CHECKS_INVALID`, `PROJ_BRANCH_MISMATCH` | `{"error":"Your company requires approval for this kind of GitHub action.","code":"GHOP_ABOVE_CEILING","action":null}` (one sentence per code) |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Project Service

  Scenario: The project open on a place is found for a coding task
    Given place "P1" holds an open git project "rates" and place "P2" holds only a closed project
    When project_for_place is called for "P1"
    Then it returns the project "rates"
    And project_for_place for "P2" returns none

  Background:
    Given a local git repository "supplier-portal" on branch main, opened as a project in workspace W

  Scenario: Happy path — the engineer sets the project's check commands (SA-CLOSE-03)
    When projects.set_check_commands is sent with commands ["cargo test", " cargo test ", "cargo clippy"]
    Then the project's check_commands are ["cargo test", "cargo clippy"] in the projects table
    And the journal has check_commands_set with count 2

  Scenario: Error path — too many check commands are refused
    When set_check_commands is called with 21 commands
    Then it fails with PROJ_CHECKS_INVALID and the stored commands are unchanged

  Scenario: Edge case — a project with no check commands runs no checks
    Given the project's check_commands are empty
    When end_coding_run produces a change set
    Then no check command runs and test_outcome is absent

  Scenario: Happy path — a coding task gets its own worktree and branch
    When start_coding_task is called with task name "Rates retry"
    Then branch "ordin/rates-retry" exists at main's HEAD and a worktree under the application data root holds it
    And a coding_task_worktree place is returned and the engineer's working copy is unchanged

  Scenario: Happy path — the change set comes from the worktree diff
    Given the agent changed src/rates.rs in two places in the worktree
    When end_coding_run is called
    Then the change set is in_review with two entries for src/rates.rs and the task is ready_for_review

  Scenario: Happy path — accepting an entry applies it to the engineer's working copy
    Given the engineer has not changed src/rates.rs
    When decide_entry(first entry, accept) is called
    Then src/rates.rs in the working copy holds the hunk, recorded as a new version, and nothing is committed

  Scenario: Error path — the engineer changed the same lines
    Given the engineer edited the same lines of src/rates.rs in the IDE
    When decide_entry(first entry, accept) is called
    Then the outcome is a conflict with both versions and the working copy is unchanged

  Scenario: Edge case — the last decision settles and cleans up
    When the last pending entry is rejected
    Then the change set and task are settled and the worktree and branch "ordin/rates-retry" are removed

  Scenario: Edge case — keeping one attempt sets the others aside
    Given two tasks in one attempt group
    When keep_attempt(first) is called
    Then the second is set_aside and its worktree and branch are removed

  Scenario: Error path — attempts above the cap
    Given max_attempts is 3 and a group already has 3 tasks
    When a fourth task is started in the group
    Then it fails with PROJ_TOO_MANY_ATTEMPTS

  Scenario: Happy path — a merged change to an original is reported
    Given rates.md on main is followed and Sam Lee's merged commit d4e5f6 changed it on GitHub
    When the 15-minute fetch runs
    Then a RepositoryOriginChange for rates.md with commit d4e5f6 and author "Sam Lee" is broadcast
    And the working copy and HEAD are unchanged

  Scenario: Edge case — a change on another branch updates nothing
    Given a commit changing rates.md is pushed to branch "draft"
    When the fetch of main runs
    Then no RepositoryOriginChange is broadcast

  Scenario: Happy path — the workspace's languages come from its projects
    Given workspace W's open project "supplier-portal" has go.mod and tsconfig.json at its root
    When workspace_languages(W) is called
    Then it returns ["go", "typescript"]

  Scenario: Happy path — the Push Service gets a git credential without a host call
    Given Identity holds a GitHub token
    When git_credentials("github.com") is called
    Then the username is "x-access-token" and the token's Debug form is "GitToken(***)"

  Scenario: Happy path — a pull request is opened with labels and reviewers
    When open_pull_request is called for "acme/agents" from "ordin/rates" into "main" with label "handover" and reviewer "samlee"
    Then gh api posts the pull request, then its labels, then its requested reviewers, and the returned number is the host's

  Scenario: Error path — a handover record written over a newer one is refused
    Given handover.yaml on branch "handover/northwind" has sha "b2" and the caller read "b1"
    When update_file is called with expected_sha "b1"
    Then it fails with GH_CONFLICT carrying current_sha "b2"

  Scenario: Happy path — workflow files are listed in a GitLab folder
    Given the company GitLab host is "gitlab.acme.co.uk" and folder "workflows" of "ops/automation" holds two .workflow.yaml files
    When list_files is called with glob "*.workflow.yaml"
    Then two RepoFiles are returned, each with its last commit and author

  Scenario: Error path — a GitHub-only call on GitLab
    When merge is called for host "gitlab.acme.co.uk"
    Then it fails with GH_HOST_NOT_SUPPORTED

  Scenario: Error path — commit with undecided changes
    Given an entry is pending
    When commit is called
    Then it fails with PROJ_UNDECIDED_CHANGES

  Scenario: Error path — switching off approval above the company's ceiling
    Given "merge" is in always_approved_kinds
    When set_git_host_approval(project, "merge", false) is called
    Then it fails with GHOP_ABOVE_CEILING

  Scenario: Happy path — new problems compare with the base branch
    Given the base has one warning in fn fetch and the task adds an error in fn fetch
    When end_coding_run is called
    Then new_problems is 1 and the warning is not counted

  Scenario: Error path — invalid language server settings are refused
    When configure_language("go", None, true, "[1,2]") is called
    Then it fails with LANG_SETTINGS_INVALID and nothing is saved

  Scenario: Error path — the editor cannot send arbitrary LSP methods
    When lsp_send carries "workspace/executeCommand"
    Then it fails with PROJ_LSP_METHOD_NOT_ALLOWED

  Scenario: Edge case — the terminal never reaches an agent or the journal
    Given a terminal is open and the engineer types "ls"
    Then the output goes only to the terminal stream and no journal entry or log line holds it

  Scenario: A repository on the company GitLab host uses the GitLab sign-in
    Given the profile sets gitlab.host to "gitlab.acme.co.uk"
    And a project's origin is "https://gitlab.acme.co.uk/ops/automation.git"
    When the project fetches
    Then the token comes from Identity with CredentialKind gitlab

  Scenario: Without a profile entry the built-in table approves the language server
    Given the profile has no language_servers entry for go and language-servers.json lists gopls
    When the program builds ProjectSettings
    Then go's approved servers are gopls at the table's version and gopls is the default
```

## Performance / Security / Observability

- **Performance targets.** `status` and `review` ≤ 150 ms p95 for a repository of 50,000 files (A-03); `start_coding_task` ≤ 3 s p95 for that repository (worktree checkout); `end_coding_run` diff ≤ 2 s p95 for 200 changed files excluding check commands and diagnostics; editor completion relay overhead ≤ 20 ms p95 beyond the server; fetch of one followed branch ≤ 10 s p95 on the company network; at most `MAX_LANGUAGE_INSTANCES` server processes (R-37).
- **Security.** Agents edit only their task's worktree; their changes reach the engineer's working copy only by the engineer's decision, and history only by the engineer's commit or an approved GitHub/GitLab operation (ADR-021, ADR-023); `ordin/*` branches stay local; no force push; tokens come from Identity per operation, are passed in a credentials callback or a child's environment, and never logged; child processes get credential variables removed except the one token they need; the terminal is the engineer's alone and unjournalled (ADR-022); check commands and `gh`/`glab` calls are decided by the Rules Engine; the editor's LSP methods are limited to a list; IDE launch uses argument lists, never a shell.
- **Never logged.** Tokens, file content, diffs, hunks, commit messages, check output, terminal bytes, `gh`/`glab` arguments, LSP payloads.
- **Observability (logs only; SA-XC-04/05).** `component = "projects"`: `project_opened {project_id, is_git}`, `coding_task_started|removed {task_id}`, `change_set_ready {task_id, entries, new_problems, test_outcome}`, `entry_decided {entry_id, decision, outcome}`, `check_run {project_id, task_id?, exit_code, elapsed_ms}`, `git_fetch {place_id, branch, result, elapsed_ms}`, `original_changed {place_id, commit}`, `git_pushed {place_id, branch}`, `git_host_call {server, exit_code}`, `language_server_started|crashed|stopped {language, server, instance}`, `ide_opened {ide_id}`.

## Gaps

None.

## Amendment January 2026 — Team knowledge (ADR-103)

> Source: decision 83 (the user's decisions D1–D10 of January 2026); HLD revision 0.51.0, ADR-103. Counterpart: `components/TeamKnowledgeService.md` (§B, §D, §E and its `TeamRepos` trait).

### Purpose (addition)

The Team Knowledge Service is a further caller of `GithubAccess`: it takes HTTPS credentials for its own bare clone of a team's repository, reads the person's permission on that repository and reads whether the linked branch requires review. This service's behaviour does not change.

### Approach (addition)

The Team Knowledge Service keeps its own clone and runs `git2` itself, as the Catalogue Service does; it calls only the three methods below, unchanged, through the program's implementation of its `TeamRepos` trait (D3). Rejected: *a team-folder fetch in this service, as `fetch_followed` does for originals* — this service fetches repository places the person linked as folders, and a team folder is a `GitRemote` link with no place.

### Shared Context (additions)

N/A — `GithubAccess`, `GitCredential`, `RepoPermission`, `BranchReviewRules` and `GithubError` are those above, unchanged.

### Public Interface (additions)

No item is added. The *Called by* row for `GithubAccess` names the Team Knowledge Service for `git_credentials`, `repository_permission` and `branch_review_rules` (the program's `TeamRepos` delegates to them one for one; `TeamRepos::person` delegates to Identity and Credentials, not to this service).

### Internal Logic (changes)

N/A — Internal Logic 14 serves these calls for every caller, GitHub through `gh api` and the company's GitLab through `glab api`.

### Data Model (additive migration)

N/A — no table or column changes.

### Error Table (rows added)

N/A — no new code; the Team Knowledge Service maps `GH_*` to its own `TEAM_*` codes.

### Acceptance Criteria (Gherkin, added)

```gherkin
  Scenario: The Team Knowledge Service reads permission and review rules
    Given the person signed in to GitHub with write access to "acme/supplier-team", whose "main" requires one approving review
    When repository_permission("github.com", "acme/supplier-team") and branch_review_rules("github.com", "acme/supplier-team", "main") are called
    Then they return Write and { review_required: true, code_owner_review_required: false, required_approvals: 1 }
```

### Performance, Security, Observability (additions)

N/A — the calls and their bounds are those of Internal Logic 14; each is logged `git_host_call` as today.

### Gaps

None.

---

## Amendment January 2026 — Reading other teams' knowledge (ADR-110)

> Source: decision 90 (the user's decisions of January 2026); HLD revision 0.59.0, ADR-110 (amends ADR-103: one own folder, any number of read folders; adds Adopt and folders the company names). Counterpart: `components/TeamKnowledgeService.md` (amendment January 2026, §R and §S, and its `TeamRepos` trait).
> Every rule below adds to the sections above, except the two corrections marked **(changed)** in *Internal Logic*.

### Purpose (addition)

`GithubAccess` says whether a branch requires review in three answers — required, not required, or *cannot tell with the person's access* — on GitHub and on the company's GitLab. The Team Knowledge Service needs the third answer for a folder the company names with `review_required: true`, which a person usually reads with read access only, and for the person's own team folder, where the earlier reading took "no protection settings visible" for "no review" (*Internal Logic* 14 step 3, corrected below).

### Approach (addition)

The answer is built from the endpoints a person with read access can call. Evidence (spike of January 2026, official documentation; the items marked *unverified* are confirmation **C-92**):

- **GitHub rulesets.** `GET repos/{repository}/rules/branches/{branch}` returns every active rule that applies to the branch, from repository and organisation rulesets; fine-grained tokens need *Metadata: read*, and "anyone with read access to a repository can view its active rulesets". Rules in rulesets with `evaluate` or `disabled` enforcement are not returned (https://docs.github.com/en/rest/repos/rules#get-rules-for-a-branch; https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/about-rulesets). The `pull_request` rule's `parameters.required_approving_review_count` (0–10) is documented in the ruleset schema; its exact shape on this endpoint is *unverified* (C-92).
- **GitHub classic branch protection.** `GET repos/{repository}/branches/{branch}/protection/required_pull_request_reviews` needs *Administration: read*, which a person with read or write access but no admin role does not hold (https://docs.github.com/en/rest/branches/branch-protection#get-pull-request-review-protection). `GET repos/{repository}/branches/{branch}` needs *Contents: read* and returns `protected` reliably; whether its `protection` object shows the review settings to a non-admin is *unverified* and is not relied on (https://docs.github.com/en/rest/branches/branches#get-a-branch).
- **GitLab protected branches.** `GET projects/{id}/protected_branches/{name}` is readable by Reporter on private projects (Guest on public and internal ones) with a `read_api` token, and returns `push_access_levels` and `merge_access_levels` (access level `0` = no one), but no approval count (https://docs.gitlab.com/api/protected_branches/; https://docs.gitlab.com/security/tokens/access_token_scopes/).
- **GitLab approval rules.** `GET projects/{id}/approval_rules` returns `approvals_required`, `applies_to_all_protected_branches` and `protected_branches[]`; it exists on Premium and Ultimate only, and GitLab Free cannot require approvals at all ("These approvals are optional and don't prevent merging without approval") (https://docs.gitlab.com/api/merge_request_approvals/; https://docs.gitlab.com/user/project/merge_requests/approvals/). Its minimum role is *unverified* (C-92).

"Cannot tell" is never read as "not required". Rejected: *treating a protected branch as review required* — protection without a review rule is common (status checks only), so ordin would accept unreviewed content; *asking the company for a token that can read protection settings* — ordin never asks the company for a token (ADR-103 item 8, decision 83).

### Shared Context (additions)

```rust
// Owned here (GithubAccess family), beside BranchReviewRules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BranchReviewStatus {
    Required { required_approvals: u32, code_owner_review_required: bool },
    NotRequired,
    /// The person's access does not show whether review is required.
    CannotTell,
}
```

### Public Interface (additions)

```rust
#[async_trait::async_trait]
pub trait GithubAccess: Send + Sync {
    // … every method above, unchanged …
    /// Amendment January 2026 (ADR-110). GitHub and the company's GitLab.
    async fn branch_review_status(&self, host: &str, repository: &str, branch: &str) -> Result<BranchReviewStatus, GithubError>;
}
```

*Called by:* the Team Knowledge Service, through the program's implementation of its `TeamRepos` trait (TeamKnowledgeService.md amendment January 2026, *Shared Context*).

### Internal Logic (additions and changes)

**14.6 `branch_review_status(host, repository, branch)`** (*added*). Host resolution, token, timeouts and the rate-limit and network mappings are step 1's. Within this method the HTTP status of each call is read directly, so a 403 or 404 here is an answer, not `GH_NOT_FOUND`:

- **GitHub.**
  1. `GET repos/{repository}/rules/branches/{branch}`: any rule with `type = "pull_request"` and `parameters.required_approving_review_count ≥ 1` → `Required { required_approvals: <the largest such count>, code_owner_review_required: <any such rule's parameters.require_code_owner_review> }`. A 404 (the repository is not visible) → `GH_NOT_FOUND`.
  2. Otherwise `GET repos/{repository}/branches/{branch}`: 404 → `GH_NOT_FOUND`; `protected = false` → `NotRequired`.
  3. Protected: `GET repos/{repository}/branches/{branch}/protection/required_pull_request_reviews` → 200 with `required_approving_review_count ≥ 1` → `Required { required_approvals, code_owner_review_required: require_code_owner_reviews }`; 200 with 0 → `NotRequired`; 403 or 404 → `CannotTell`.
- **GitLab** (`glab api`, `projects/{url-encoded repository}`):
  1. `GET projects/{id}/protected_branches/{url-encoded branch}`: 404 → `NotRequired` (anyone who can write may push to an unprotected branch). A 404 for the project itself (`GET projects/{id}` first, once per call) → `GH_NOT_FOUND`.
  2. Any entry of `push_access_levels` with `access_level > 0`, or a user or group entry → `NotRequired` (changes may reach the branch without a merge request).
  3. Nobody may push: `GET projects/{id}/approval_rules` → 200: a rule with `approvals_required ≥ 1` that covers the branch (`applies_to_all_protected_branches = true`, or `protected_branches[].name` equal to the branch) → `Required { required_approvals: <the largest such value>, code_owner_review_required: <the protected branch's code_owner_approval_required, false when absent> }`; 200 with no such rule → `NotRequired`; 403 or 404 (GitLab Free, or the role cannot read rules) → `CannotTell`.
- Log `git_host_api {operation: "branch_review_status", host_kind, status, elapsed_ms}` as step 5; the answer's kind is logged, never the repository name.

**14.3 `branch_review_rules` (changed).** It is now derived from `branch_review_status`: `Required { n, c }` → `{ review_required: true, code_owner_review_required: c, required_approvals: n }`; `NotRequired` → all false and 0; `CannotTell` → `{ review_required: true, code_owner_review_required: false, required_approvals: 0 }`. This replaces "a 404 → all false and 0": a person with write access but no admin role receives a 404 or 403 from the classic protection endpoint, which was read as "no review", so a share was pushed directly and refused by the host. Every existing caller keeps its signature; for a caller that opens a pull request when `review_required` is true, a request opened on a branch that turns out not to need review is harmless (it can be merged at once).

**14.1 (changed).** `repository_permission`, `branch_review_rules` and `branch_review_status` serve the company's GitLab too, beside `git_credentials`, `list_files`, `read_file` and `file_last_commit`; every other method stays GitHub-only. GitLab `repository_permission`: `GET projects/{id}` → the higher of `permissions.project_access.access_level` and `permissions.group_access.access_level`: 50 → `Admin`, 40 → `Maintain`, 30 → `Write`, 20 → `Read`, 10 or none → `None` (a Guest cannot read a private project's files). This makes the January 2026 amendment's statement that Internal Logic 14 serves these calls "the company's GitLab through `glab api`" true.

### Data Model (additive migration)

N/A — no table or column changes.

### Error Table (rows added)

N/A — no new code. `CannotTell` is an answer, not an error; `GH_NOT_FOUND`, `GH_SIGN_IN_NEEDED`, `GH_RATE_LIMITED`, `GH_UNREACHABLE`, `GH_HOST_NOT_SUPPORTED` and `GH_FAILED` keep their meanings.

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Project Service — branch review status

  Scenario: Happy path — a GitHub ruleset requires review, read access only
    Given the person has read access to "acme/standards", whose "main" has an active ruleset with a pull_request rule requiring 2 approving reviews
    When branch_review_status("github.com", "acme/standards", "main") is called
    Then it returns Required with required_approvals 2

  Scenario: Edge — classic protection hidden from the person
    Given "main" of "acme/standards" is protected by classic branch protection and has no ruleset
    And the protection review endpoint answers 404 for the person
    When branch_review_status is called
    Then it returns CannotTell

  Scenario: Edge — an unprotected GitHub branch
    Given "main" of "acme/notes" has no ruleset and protected false
    When branch_review_status is called
    Then it returns NotRequired

  Scenario: Edge — GitLab Free with nobody allowed to push
    Given the company GitLab project "platform/knowledge" whose "main" allows push to no one and whose approval_rules call answers 403
    When branch_review_status is called with the company's GitLab host
    Then it returns CannotTell

  Scenario: Edge — GitLab approval rule covers the branch
    Given the same project on a tier where approval_rules returns a rule applying to all protected branches with approvals_required 1
    When branch_review_status is called
    Then it returns Required with required_approvals 1

  Scenario: Regression fix — write access without admin on GitHub
    Given the person has write access to "acme/supplier-team", whose "main" is protected by classic protection requiring 1 review, which the person cannot read
    When branch_review_rules is called
    Then it returns review_required true

  Scenario: Error — repository not visible
    Given the person cannot see "acme/secret"
    When branch_review_status is called
    Then it fails with GH_NOT_FOUND
```

### Performance, Security, Observability (additions)

- **Performance:** at most three host calls per answer, each bounded by `GH_TIMEOUT`; the Team Knowledge Service calls it at most once an hour per folder (its amendment §S).
- **Security:** read endpoints only; the person's own token; no answer is cached here.
- **Observability:** `git_host_api` lines as step 5, with `operation: "branch_review_status"`.

### Gaps

None. The *unverified* items above are confirmation C-92 (HLD `10-risks.md`); until it is confirmed, every uncertain response is read as `CannotTell`, which is the safe answer.
