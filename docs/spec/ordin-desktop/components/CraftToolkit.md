# SPEC: Craft Toolkit

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-craft-toolkit` · **Build phase:** 5 · **Programs:** D P
> **Dependencies:** MCP Front, Rules Engine, File Service, Organisation Profile, Journal

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

Line references: `02:N` is `docs/design/ordin-desktop/02-architecture.md` line N; `03:N`, `05:N`, `07:N`, `08:N`, `10:N`, `11:N` likewise; `09 ADR-0NN` is an ADR in `09-decisions.md`; `2C` is `docs/spec/ordin-desktop/phase-2c-shared-types-*.md`; `2D` is `phase-2-architecture.md` §2D; `B7 contract` is the batch's shared contract sheet whose decisions are restated here.

## Purpose

The Craft Toolkit makes the ArtCraft apps available to people and agents (02:751, 09 ADR-016, ADR-017, ADR-039). It installs the Craft apps and their command-line tools that the organisation profile approves, at the pinned versions, under the application data root with no administrator rights, and verifies each download (ADR-072). For the person it resolves a file's kind through the file-type map and opens the file in the matching app's window, or offers the system's default app when no ArtCraft app is mapped or installed. For agents it serves each app's own MCP mode (`<app>-cli mcp`) through ordin's MCP Front as the MCP server `craft_<app>`, offering only the tools the session's package and the profile allow, and writes every output as a new file version through the File Service. For ordin's own published copies it makes the Word, PDF and slides copies of a Markdown original through WordCraft's, PdfCraft's and DeckCraft's MCP mode, with the line naming the original at the top (09 ADR-048, ADR-069). It owns Craft installations, the file-type map in use and the Craft tool definitions (02:757).

## Approach

Each approved app's MCP server is a child process the toolkit starts per agent session (rmcp client over stdio, SA-MCP-01), so the MCP Front decides every call before it reaches the app (ADR-039), and the toolkit itself only maps arguments and records outputs. Which arguments are file paths, and which tool converts Markdown, comes from **Craft tool definitions** shipped with ordin per app version (*chosen here*), so a tool not described there is never offered (fail closed). Rejected: custom tools over the CLIs' `run` and `batch` commands (withdrawn by ADR-039, although 02:755 still describes them — 1D-style disagreement, reading ADR-039); one long-lived MCP server per app shared by every session (a hung or crashed app would affect every session, and a session's grants could not be told apart at the process).

Reconciliation assumptions (Phase 3 reconciliation):

- **SA-RECON-01** The toolkit gives the MCP Front one provider, `CraftAppsProvider`, which claims the server-name prefix `craft_` and lists one server `craft_<app>` per installed app with MCP mode; apps installed or removed while ordin runs appear and disappear through `servers()` and `tools_changed`. There is no `unregister` (MCP Front SA-RECON-06; reconciliation row 299). The program registers the provider returned by `tool_provider()` (ruling R2).
- **SA-CLOSE-01** (*chosen here*; closes the `craft.apps[]` gap; closing decision K9) The profile's `craft.apps[]` entry is `{ app_id, version, allowed_tools, platforms: { <platform>: { asset_url, sha256, file_name } } }` with platforms `macos_arm64`, `macos_x86_64`, `windows_x86_64`, `linux_x86_64`; the toolkit downloads each file from its `asset_url`, so `craft.mirror_url` is not used for downloads; an absent `allowed_tools` means every tool the app's MCP mode lists, within the shipped definitions.
- **SA-RECON-02** The Setup Assistant checks the apps through `check_apps(repair)`, which wraps `installations`, `sync_installations` and the start check (reconciliation row 354, ruling R6).
- **SA-RECON-03** Reverse conversion: `to_markdown(format, bytes)` turns a Word file into Markdown for the Push Service and, through the program, the Confluence Adapter's `WordToMarkdown`. *Conditional on C-13* (`wordcraft-cli` converts Word to Markdown headless): until confirmed, no definition carries a `converts_to_markdown_from` tool and the call returns `CRAFT_CONVERSION_UNAVAILABLE` (reconciliation row 356).
- **SA-RECON-05** No `McpFront::package_allows`: the package's tool declaration reaches the call through the Rules Engine, which denies undeclared tools before the provider acts (reconciliation row 300); the provider offers the tools its definitions and the profile allow.
- **SA-RECON-06** The tables are reached through the Journal's `Datastore` and journal writes use `append_entry` (D4, ruling R3); the profile is read from `OrganisationProfileService::current().document.craft` (no getter trait, OrganisationProfile.md).
- **SA-RECON-04** The Service API catalogue adopts this module's `craft.*` command names, inputs and results; opening a file in its app is `craft.open` (reconciliation rows 357, 358; ruling R1).

## Shared Context

### Identifiers and time (2C part 1 *Conventions*)

```rust
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>);   // RFC 3339, UTC, ms, `Z`
pub struct CraftInstallationId(pub uuid::Uuid);  pub struct FileTypeMapEntryId(pub uuid::Uuid);
pub struct PlaceId(pub uuid::Uuid);  pub struct RunId(pub uuid::Uuid);  pub struct WorkspaceId(pub uuid::Uuid);
pub struct AgentInstanceId(pub uuid::Uuid);  pub struct CodingTaskId(pub uuid::Uuid);  pub struct FileVersionId(pub uuid::Uuid);
pub struct Sha256Hex(pub String);   // 64 lowercase hex
```

### Domain types owned here (2C part 1, copied unchanged)

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CraftInstallation {
    pub id: CraftInstallationId,
    pub app_name: String,
    pub pinned_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_result: Option<VerificationResult>,
    pub cli_modes: Vec<CraftCliMode>,
    pub status: CraftInstallationStatus,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationResult { Verified, Failed }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CraftCliMode { Mcp, Run, Batch }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CraftInstallationStatus { Approved, Downloaded, Installed, VerificationFailed, Removed }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FileTypeMapEntry {
    pub id: FileTypeMapEntryId,
    pub extensions: Vec<String>,
    pub media_types: Vec<String>,
    /// App that opens it for the person; `None` = the system's default app.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_with_app: Option<String>,
    /// Craft tools agents use to change it; empty = agents do not change it.
    pub craft_tools: Vec<String>,
    pub profile_version: String,
}
```

| Entity | Rule | Error code |
|---|---|---|
| CraftInstallation.app_name | unique; 1–40 chars | `CRAFT_DUPLICATE` |
| CraftInstallation.status = installed | requires `verification_result = verified` | `CRAFT_UNVERIFIED_INSTALL` |
| FileTypeMapEntry.extensions | ≥ 1, each `[a-z0-9]{1,16}` without the dot | `FTM_EXTENSION_INVALID` |
| FileTypeMapEntry.extensions | an extension in at most one entry | `FTM_EXTENSION_DUPLICATE` |
| FileTypeMapEntry.media_types | RFC 6838 | `FTM_MEDIA_TYPE_INVALID` |
| FileTypeMapEntry row | changed only by a new profile | `FTM_IMMUTABLE` |

CraftInstallation state machine (2C): `→ approved` (approved in the profile); `approved → downloaded`; `downloaded → installed` (verified); `downloaded → verification_failed`; `installed → downloaded` (pin changed); `installed, approved, verification_failed → removed` (withdrawn).

Other 2C types used: `FileVersionOrigin { Person, Run { run_id, tool }, ArtCraftEdit { app } }`; `CopyFormat { Page, Word, Pdf, Slides, ExactFile }`; `CopyMaker { Ordin, WordCraft, PdfCraft, DeckCraft, None }`; `StartUpReportItem::UnavailablePart { part: String, plain: String, reason: UnavailableReason }` with `UnavailableReason { NotTrusted, MissingDependency, HarnessCannotRun }` (2C part 2 §6; `plain` 1–200 chars); `ToolDecision { verdict: Verdict { Allow | Deny { reason } | Ask { proposal } }, rule, decided_at }` (2C part 2 §3); `FolderAccess { Read, ReadAndChange }`.

### Organisation profile keys read (2D-2; key names chosen in 2D)

| Key | Type | Use |
|---|---|---|
| `craft.apps[]` | list of `{ app_id, version, allowed_tools, platforms }` (2D-2 as fixed by closing decision K9) | Approved apps and pins: `{ "app_id": "photocraft", "version": "0.7.0", "allowed_tools": ["<tool>", …], "platforms": { "<platform>": { "asset_url": "<https URL>", "sha256": "<64 hex>", "file_name": "<file name>" } } }`. `app_id` is the app's name everywhere in this spec (`[a-z0-9-]`, 1–40). An absent or empty `allowed_tools` means every tool the app's MCP mode lists (still limited to the shipped definitions and the Rules Engine's decision) (SA-CLOSE-01). |
| `craft.mirror_url` | `https` URL | Not read for downloads: each platform's `asset_url` is the full address, which names the company mirror when there is one (SA-CLOSE-01). |
| `craft.file_types` | map extension → app name | The file-type map. |

Platforms (K9): `macos_arm64`, `macos_x86_64`, `windows_x86_64`, `linux_x86_64`. A universal macOS file such as 08:67's `photocraft-cli-0.7.0-macos-universal.zip` is pinned under both macOS keys.

### `ToolProvider` and `ToolRegistry` — owned by the MCP Front (McpFront.md, ADR-100; copied, ruling R2)

```rust
// crate ordin-mcp-front (owner). This crate implements ToolProvider; the program registers it.
pub use ordin_types::session::ToolCaller;   // Ordin | Agent | CrewMember { crew_run_id } | HostedAgent
pub struct ServerName(String);              // ^[a-z][a-z0-9_]{0,31}$
pub enum ProgramProfile { Desktop, PersonalServer, AgentProfile }

pub struct SessionGrant {
    pub run_id: RunId,
    pub workspace_id: WorkspaceId,          // agent profile: the one workspace the Agent Loader assigns
    pub agent_instance_id: AgentInstanceId,
    pub harness: HarnessKind,
    pub profile: ProgramProfile,
    pub caller: ToolCaller,
    pub scope_id: Option<ScopeId>,          // agent profile only; None = agent-wide or no scope
    pub coding_task_id: Option<CodingTaskId>,
}
pub struct ToolKey { pub server: ServerName, pub tool: String }   // tool ^[A-Za-z][A-Za-z0-9_-]{0,63}$
pub struct ServerSpec { pub name: ServerName, pub tools: Vec<ToolSpec> }
pub struct ToolSpec {
    pub name: String,
    pub description: String,                // one line; 1–120 chars (1–1,024 in the agent profile)
    pub input_schema: serde_json::Value,    // JSON Schema, "type": "object"
    pub argument_form: String,              // 1–300 chars, quoted to the model on a wrong call
    pub sandbox_callable: bool,             // offered in the Sandbox's ordin_tools module
    pub plain: String,                      // start-up report sentence, 1–200 chars
}
pub enum CallVia { Harness, Sandbox }
pub struct ProgressSink { /* private */ }
impl ProgressSink { pub fn report(&self, done: u64, total: Option<u64>); }
pub struct ToolCallContext {
    pub run_id: RunId, pub workspace_id: WorkspaceId, pub agent_instance_id: AgentInstanceId,
    pub caller: ToolCaller, pub scope_id: Option<ScopeId>, pub coding_task_id: Option<CodingTaskId>,
    pub call_id: ToolRequestId, pub correlation_id: String,
    pub deadline: std::time::Instant, pub cancel: tokio_util::sync::CancellationToken,
    pub progress: ProgressSink, pub only_needed_parts: bool, pub via: CallVia,
}
pub struct ToolOutput { pub text: String, pub structured: Option<serde_json::Value> }
pub struct ToolFailure { pub code: String, pub message: String /* 1–300 chars */, pub retryable: bool }
pub struct ArgumentProblem { pub at: String /* JSON Pointer */, pub reason: String /* 1–300 chars */ }

#[async_trait::async_trait]
pub trait ToolProvider: Send + Sync + 'static {
    fn provider_id(&self) -> &'static str;                 // ^[a-z][a-z0-9-]{0,39}$
    fn servers(&self) -> Vec<ServerSpec>;
    fn server_prefix(&self) -> Option<&'static str> { None }
    async fn offer(&self, grant: &SessionGrant) -> Result<Vec<ToolKey>, ToolFailure>;   // within 2 s
    fn check_arguments(&self, key: &ToolKey, arguments: &serde_json::Value) -> Result<(), ArgumentProblem>;
    async fn call(&self, ctx: ToolCallContext, key: &ToolKey, arguments: serde_json::Value)
        -> Result<ToolOutput, ToolFailure>;
    async fn session_closed(&self, run_id: RunId);
}
pub trait ToolRegistry: Send + Sync {
    fn register(&self, provider: std::sync::Arc<dyn ToolProvider>) -> Result<(), McpError>;
    fn tools_changed(&self, server: &ServerName);
    fn served_tools(&self, server: &ServerName) -> Option<Vec<String>>;
}
```

How this spec reads the earlier shape (SA-RECON, reconciliation rows 279, 283–286, 294–297): what it called `tools(session)` is `servers()` (the full tool list with `ToolSpec`s) plus `offer(&grant)` (the keys this session gets); `session.caller`, `session.workspace_id`, `session.scope_id` are the grant's or the context's fields of the same names, and the agent instance is `agent_instance_id`; `session.program` is `grant.profile` (`AgentSingle` / `AgentScaled` both read as `AgentProfile`; a provider that must tell the single and scaled shapes apart reads its own configuration, row 285); the session's progress channel is `ctx.progress`. A `ToolError::InvalidArguments { code, reason }` raised by a schema or type check is returned from `check_arguments` as `ArgumentProblem { at, reason }` (the MCP Front answers `MCP_ARGUMENTS_INVALID`); an invalid-argument finding that needs state (a missing record) stays in `call` as `ToolFailure { code, message: reason, retryable: false }`. `ToolError::Refused { code, reason }` → `ToolFailure { code, message: reason, retryable: false }`; `ToolError::Unavailable { code, reason, retryable }` → `ToolFailure { code, message: reason, retryable }`; `ToolError::Internal { code }` → `ToolFailure { code, message: <the error table's sentence>, retryable: false }`. `ToolResult { content }` → `ToolOutput { text: <content as compact JSON, or the result text where this spec gives one>, structured: Some(content) for JSON content, None for a text result }`. `session_ended` / session end → `session_closed(run_id)`.

The Craft Toolkit's provider claims `server_prefix() = Some("craft_")` (SA-RECON-01). What this spec called `SessionContext` is the `SessionGrant` in `offer` and the `ToolCallContext` in `call` (both carry `coding_task_id`); `session_ended(session)` is `session_closed(run_id)`; `ToolResult { content, is_error: false }` is `ToolOutput { text, structured: Some(content) }` and `is_error: true` is `ToolFailure`. There is no `McpFront::unregister` and no `package_allows`: an app's server leaves `servers()` and `tools_changed` announces it, and the package's tool declaration is applied by the Rules Engine's decision before `call` (`DeclarationToolRule`), so this provider offers every tool its definition and the profile allow (SA-RECON-05).

### Journal (`ordin-journal`, JournalAndArtefactStore.md; ruling R3)

```rust
#[derive(Clone)]
pub enum Datastore { Sqlite(sqlx::SqlitePool), Postgres(sqlx::PgPool) }
pub type MigrationStep = (i64, &'static str, &'static str);
pub struct ModuleMigrations { pub module: &'static str, pub sqlite: &'static [MigrationStep], pub postgres: &'static [MigrationStep] }
impl Journal {   // the concrete `ordin_journal::Journal`
    pub fn datastore(&self) -> &Datastore;
    pub async fn append_entry(&self, entry: NewJournalEntry) -> Result<JournalEntry, JournalError>;
}
pub struct NewJournalEntry { pub kind: String /* ^[a-z][a-z0-9_]{0,63}$ */, pub component: String,
    pub run_id: Option<RunId>, pub crew_run_id: Option<CrewRunId>, pub workspace_id: Option<WorkspaceId>,
    pub summary: String, pub data: serde_json::Value }
```
Every `Journal <kind> {…}` of this spec is `journal.append_entry(NewJournalEntry { kind: "<kind>", component: "craft", … })`; the tables are reached through `journal.datastore()` (D4).

### Other components' calls (owners' exact signatures; ruling R6)

```rust
// Rules Engine (RulesEngine.md K.5) — direct edge; never Ask.
pub fn check_path_access(&self, req: &PathAccessRequest) -> ToolDecision;
pub struct PathAccessRequest { pub workspace_id: Option<WorkspaceId>, pub agent: Option<AgentInstanceId>,
    pub crew_charter_id: Option<CrewCharterId>, pub path: std::path::PathBuf, pub access: FolderAccess }

// File Service (FileService.md) — direct edge.
pub async fn place(&self, place_id: PlaceId) -> Result<Place, FileError>;
pub async fn locate(&self, absolute: &std::path::Path) -> Result<Option<(PlaceId, String)>, FileError>;
pub async fn read_file(&self, place_id: PlaceId, path: &str) -> Result<FileBytes, FileError>;
pub async fn write_version(&self, w: WriteVersion) -> Result<FileVersion, FileError>;
pub struct WriteVersion { pub place_id: PlaceId, pub path: String, pub bytes: Vec<u8>,
                          pub origin: FileVersionOrigin, pub expected_hash: Option<Sha256Hex> }

// Organisation Profile (OrganisationProfile.md) — direct edge; no getter trait: this crate reads
// `current().document.craft` and follows `subscribe()`.
pub fn current(&self) -> std::sync::Arc<ProfileSnapshot>;
pub fn subscribe(&self) -> tokio::sync::watch::Receiver<std::sync::Arc<ProfileSnapshot>>;
pub struct CraftSection {
    #[serde(default)] pub apps: Vec<CraftApp>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub mirror_url: Option<String>,
    /// file extension (no dot) → app name.
    #[serde(default)] pub file_types: BTreeMap<String, String>,
}
pub struct CraftApp {
    pub app_id: String,
    pub version: String,
    /// The app's allowed list (ADR-017); `None` = every tool the app's MCP mode lists (SA-CLOSE-01).
    #[serde(default, skip_serializing_if = "Option::is_none")] pub allowed_tools: Option<Vec<String>>,
    pub platforms: BTreeMap<ProfilePlatform, CraftAsset>,
}
pub struct CraftAsset { pub asset_url: String, pub sha256: Sha256Hex, pub file_name: String }
/// Platforms a pinned download names (craft apps and language servers; K9, K12).
pub enum ProfilePlatform {
    #[serde(rename = "macos_arm64")] MacosArm64,
    #[serde(rename = "macos_x86_64")] MacosX86_64,
    #[serde(rename = "windows_x86_64")] WindowsX86_64,
    #[serde(rename = "linux_x86_64")] LinuxX86_64,
}
```

`CraftProfile` below is this crate's own view, built from `CraftSection` at each snapshot: `profile_version` is the snapshot's version, `apps` from `CraftApp` (`name` = `app_id`; each `ProfilePlatform` maps to the `CraftPlatform` of the same name, and each platform's `asset_url` (parsed as a `url::Url`), `file_name` and `sha256` are taken from its `CraftAsset`; `allowed_tools` as given, `None` meaning every listed tool), `file_types` as given. A platform entry whose `asset_url` does not parse as a URL or is not `https`, whose `sha256` is not 64 hex characters, or whose `file_name` contains `/`, `\` or `..` is dropped and logged `craft_pin_invalid {app, platform}` (that app is then not available on that platform, `CRAFT_PLATFORM_UNSUPPORTED`).

```rust
#[derive(Debug, Clone)]
pub struct CraftProfile { pub profile_version: String, pub apps: Vec<CraftAppPin>,
                          pub file_types: std::collections::BTreeMap<String, String> }
#[derive(Debug, Clone)]
pub struct CraftAppPin { pub name: String, pub version: String,
                         pub platforms: std::collections::BTreeMap<CraftPlatform, CraftPinnedAsset>,
                         /// `None` = every tool the app's MCP mode lists (K9).
                         pub allowed_tools: Option<Vec<String>> }
#[derive(Debug, Clone)]
pub struct CraftPinnedAsset { pub asset_url: url::Url, pub file_name: String, pub sha256: Sha256Hex }
```

### Craft tool definitions (*chosen here*; owned here, 02:757)

A JSON file shipped inside ordin's build, `craft-tools.json`, one entry per app and version range, reviewed against C-07, C-21 and C-28 before an app is approved:

```rust
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CraftToolDefinitions { pub format: u32, pub apps: Vec<CraftAppDefinition> }   // format = 1

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CraftAppDefinition {
    pub app: String,                 // "photocraft"
    pub versions: String,            // Cargo SemVer requirement, "^0.7"
    pub cli: String,                 // binary name, "photocraft-cli"
    pub window: AppWindow,           // how to open the app's window
    pub tools: Vec<CraftToolDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AppWindow {
    pub macos_bundle: Option<String>,    // "PhotoCraft.app"
    pub windows_exe: Option<String>,     // "PhotoCraft.exe"
    pub linux_exe: Option<String>,       // "photocraft"
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CraftToolDefinition {
    pub tool: String,                    // the app's MCP tool name
    pub description: String,             // one line (ADR-062)
    /// Arguments holding file paths, with their role. Any other argument is passed unchanged.
    pub path_args: Vec<PathArg>,
    /// Set on the conversion tools of ADR-048: which copy format this tool makes from Markdown.
    pub converts_markdown_to: Option<CopyFormat>,
    /// Set on a reverse-conversion tool: which copy format this tool turns into Markdown
    /// (SA-RECON-03; empty for every app until C-13 is confirmed).
    #[serde(default)]
    pub converts_to_markdown_from: Option<CopyFormat>,
    /// For a slides conversion: the argument that sets a first-slide footer, when the tool has one.
    pub footer_arg: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PathArg { pub name: String, pub role: PathRole }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathRole { Input, Output }
```

### Constants (*chosen here* unless cited)

| Constant | Value | Source |
|---|---|---|
| `CALL_TIMEOUT` | 120 s per Craft tool call or conversion | 2D-4 |
| `START_TIMEOUT` | 15 s for `<app>-cli mcp` to answer `initialize` | *chosen here* |
| `SESSION_IDLE_STOP` | 10 minutes without a call stops a session's app server | *chosen here* |
| `MAX_SERVERS` | 16 app servers at once (8 sessions × 2 apps, A-04) | *chosen here* |
| `MAX_OUTPUT_BYTES` | 256 MiB per output file | *assumed here* |
| `DOWNLOAD_TIMEOUT` | 10 minutes per asset | *chosen here* |
| `INSTALL_ROOT` | `<app data root>/craft/<app>/<version>/` | ADR-072 |
| `SCRATCH_ROOT` | `<app data root>/craft-scratch/` | *chosen here* |
| `BANNER_TEMPLATE` | "Published copy of {original}, from {from}, branch {branch}. Last updated {date, time}. Make changes in the original." (branch clause left out when there is no branch) | ADR-069 |

### Confirmation items bound here (Phase 1, *Confirmation items*)

- **C-07 / C-21** — each app's CLI name, modes and MCP tool list on macOS and Windows. *Until confirmed for an app*, `craft-tools.json` has no entry for it, the app is installed and opens for the person, and agents get no Craft tool for it; the start-up report says so (ADR-039).
- **C-08** — each app's licence; the licence check (Licence Check spec) refuses an app whose licence fails, and the profile cannot approve it into a build.
- **C-09** — the formats each app reads and writes. *Until confirmed* for a format, no tool for that format is listed in `craft-tools.json` (ADR-018: no interim library).
- **C-28** — WordCraft, PdfCraft and DeckCraft convert Markdown through their MCP mode. *Until confirmed*, `conversion_available(format)` is false for Word, PDF and slides, and **Publish a copy…** offers only a Confluence page (ADR-048, ADR-069).
- **C-47** — the apps are signed (notarised on macOS) and start from the application data root without a system prompt. *Until confirmed*, the SHA-256 pin from the profile is the verification that gates installation; the platform signature check is run and its result recorded for Diagnostics but does not gate.

## Public Interface

Error prefix: `CRAFT_` for this component's codes; `FTM_` for 2C's file-type map rules.

```rust
pub struct CraftDeps {
    pub journal: ordin_journal::Journal,
    /// For `tools_changed`; registration is the program's (SA-RECON-01).
    pub registry: std::sync::Arc<dyn ToolRegistry>,
    pub rules: std::sync::Arc<ordin_rules::RulesEngine>,
    pub files: std::sync::Arc<FileService>,
    pub profile: std::sync::Arc<ordin_organisation_profile::OrganisationProfileService>,
    pub http: reqwest::Client,                       // SA-HTTP-02 (system proxy, trust store, A-39)
    pub data_root: std::path::PathBuf,
    pub platform: CraftPlatform,
    pub definitions: CraftToolDefinitions,           // parsed from the bundled craft-tools.json at build time
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Keys `macos_arm64`, `macos_x86_64`, `windows_x86_64`, `linux_x86_64` (K9); the program picks the one
/// matching the build target's operating system and architecture.
pub enum CraftPlatform { MacosArm64, MacosX86_64, WindowsX86_64, LinuxX86_64 }

pub struct CraftToolkit { /* private */ }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpenOutcome {
    /// Opened in the mapped ArtCraft app's window.
    OpenedInApp { app: String },
    /// No ArtCraft app is mapped or installed: the screens ask
    /// "No editor for this kind of file — open it in your usual app?" (11 §6).
    NoEditor,
    /// Opened with the system's default app.
    OpenedWithSystemDefault,
}

pub struct MakeCopy {
    pub original_name: String,         // "rates.md"
    pub original_from: String,         // "supplier-docs" (repository) or the place's friendly name
    pub branch: Option<String>,        // "main" for a repository original
    pub markdown: String,              // the original at the version the Push Service names
    pub format: CopyFormat,            // Word, Pdf or Slides
    pub updated_at: Timestamp,
}

pub struct MadeCopy { pub bytes: Vec<u8>, pub file_name: String, pub maker: CopyMaker }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SyncReport {
    pub installations: Vec<CraftInstallation>,
    /// Programs the operating system refused to start, for "Send to IT" (ADR-072, UX-A46).
    pub blocked: Vec<BlockedProgram>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BlockedProgram { pub program: String, pub publisher: Option<String>, pub sha256: Sha256Hex, pub reason: String }

impl CraftToolkit {
    /// Module `craft`, for the Journal's registry (ruling R3).
    pub fn migrations() -> ModuleMigrations;
    pub async fn new(deps: CraftDeps) -> Result<std::sync::Arc<CraftToolkit>, CraftError>;
    /// Loads rows, applies the current profile's file-type map, and watches the profile for changes.
    /// Does not register with the MCP Front (SA-RECON-01).
    pub async fn start(self: &std::sync::Arc<Self>) -> Result<(), CraftError>;
    /// The one `craft_*` provider, for the program to register with the MCP Front before its `start`.
    pub fn tool_provider(self: &std::sync::Arc<Self>) -> std::sync::Arc<dyn ToolProvider>;
    pub async fn shutdown(&self);

    /// Installs, updates and removes apps to match the profile. Reached through `check_apps(true)`,
    /// which the Setup Assistant calls in its guided and check runs (02:752 "With the Setup Assistant").
    /// A profile change starts no download.
    pub async fn sync_installations(&self) -> Result<SyncReport, CraftError>;
    pub async fn installations(&self) -> Result<Vec<CraftInstallation>, CraftError>;
    /// Setup Assistant: each pinned app's state; with `repair`, runs `sync_installations` first
    /// (SA-RECON-02).
    pub async fn check_apps(&self, repair: bool) -> Result<Vec<CraftProgramCheck>, CraftError>;

    pub fn file_kind(&self, extension: &str) -> Option<FileTypeMapEntry>;
    pub async fn open_in_app(&self, place_id: PlaceId, path: &str) -> Result<OpenOutcome, CraftError>;
    pub async fn open_with_system_default(&self, place_id: PlaceId, path: &str) -> Result<OpenOutcome, CraftError>;

    pub fn conversion_available(&self, format: CopyFormat) -> bool;
    pub async fn make_copy(&self, req: MakeCopy) -> Result<MadeCopy, CraftError>;
    /// Reverse conversion, conditional on C-13 (SA-RECON-03). `format` must be `Word`.
    pub async fn to_markdown(&self, format: CopyFormat, bytes: Vec<u8>) -> Result<String, CraftError>;

    /// Start-up report items for a session: one `UnavailablePart` per approved app agents cannot use.
    pub async fn start_up_items(&self, grant: &SessionGrant) -> Vec<StartUpReportItem>;
}

/// The one provider returned by `tool_provider()`: prefix `craft_`, one server `craft_<app>` per
/// installed app with MCP mode and a definition (SA-NAME-04, SA-RECON-01).
pub struct CraftAppsProvider { /* private */ }
impl ToolProvider for CraftAppsProvider { /* see Internal Logic 5 and 6 */ }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CraftProgramCheck { pub app_name: String, pub version: String, pub state: ProgramCheckState }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProgramCheckState {
    /// Installed at the pinned version, hash verified, and the start check passed.
    Ready,
    /// Was not ready and `repair` made it ready.
    Repaired,
    /// Not installed at the pinned version (download, hash or platform failure; `repair` false or failed).
    Missing,
    /// The operating system refused to start it (ADR-072, `BlockedProgram`).
    Refused { publisher: String, sha256: Sha256Hex },
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum CraftError {
    #[error("CRAFT_NOT_APPROVED")] NotApproved,
    #[error("CRAFT_NOT_INSTALLED")] NotInstalled,
    #[error("CRAFT_DOWNLOAD_FAILED")] DownloadFailed,
    #[error("CRAFT_HASH_MISMATCH")] HashMismatch,
    #[error("CRAFT_PLATFORM_UNSUPPORTED")] PlatformUnsupported,
    #[error("CRAFT_INSTALL_FAILED")] InstallFailed,
    #[error("CRAFT_START_BLOCKED")] StartBlocked { program: String, reason: String },
    #[error("CRAFT_NO_MCP_MODE")] NoMcpMode,
    #[error("CRAFT_SERVER_START_FAILED")] ServerStartFailed,
    #[error("CRAFT_TOOL_NOT_OFFERED")] ToolNotOffered,
    #[error("CRAFT_PATH_NOT_ALLOWED")] PathNotAllowed,
    #[error("CRAFT_ARGUMENTS_INVALID")] ArgumentsInvalid,
    #[error("CRAFT_TIMEOUT")] Timeout,
    #[error("CRAFT_TOOL_FAILED")] ToolFailed,
    #[error("CRAFT_OUTPUT_MISSING")] OutputMissing,
    #[error("CRAFT_OUTPUT_TOO_LARGE")] OutputTooLarge,
    #[error("CRAFT_CONVERSION_UNAVAILABLE")] ConversionUnavailable,
    #[error("CRAFT_OPEN_FAILED")] OpenFailed,
    #[error("CRAFT_FILES_FAILED")] FilesFailed,
    #[error("CRAFT_DATASTORE_UNAVAILABLE")] DatastoreUnavailable,
    #[error("{0}")] Invalid(&'static str),       // CRAFT_DUPLICATE, CRAFT_UNVERIFIED_INSTALL, FTM_*
}
impl CraftError { pub fn code(&self) -> &'static str; pub fn retryable(&self) -> bool; }
```

**Called by** (each interaction is listed on the caller's side too):

| Public item | Called by |
|---|---|
| `migrations`, `new`, `start`, `shutdown`, `tool_provider` | the desktop and personal-server programs (OrdinDesktop, OrdinServer); `tool_provider` is passed to `McpFront::register` |
| `check_apps` | Setup Assistant (set-up and its checks) |
| `sync_installations`, `installations` | Setup Assistant, through `check_apps`; `installations` also by the program's Service API implementation (`craft.installations`) |
| `file_kind`, `open_in_app`, `open_with_system_default`, `conversion_available` | the program's Service API implementation (`craft.open`, `craft.open_with_system_default`, `craft.conversions`; SA-RECON-04) |
| `conversion_available`, `make_copy`, `to_markdown` | Push Service (published copies); `to_markdown` also by the desktop program's `WordToMarkdown` for the Confluence Adapter |
| `start_up_items` | the programs' `SupervisorHost::startup_items` (the Session Supervisor's start-up report) |
| servers `craft_<app>` | the MCP Front, for sessions the package and profile allow |

### Service API commands (*chosen here*)

| Command | Input | Result |
|---|---|---|
| `craft.open` | `{ "place_id", "path" }` (**Edit in [app]**, routed by the Command Bridge, 2B-2 D3) | `OpenOutcome` |
| `craft.open_with_system_default` | `{ "place_id", "path" }` (**Open in your usual app**) | `OpenOutcome` |
| `craft.installations` | `{}` | `Vec<CraftInstallation>` |
| `craft.conversions` | `{}` | `{ "word": bool, "pdf": bool, "slides": bool }` |

### Example

```rust
let outcome = craft.open_in_app(place_id, "Q3/Kestrel logo.png").await?;
// OpenOutcome::OpenedInApp { app: "photocraft" }

// An agent's call, after the MCP Front's Rules Engine decision allowed it:
let result = photocraft_provider.call(&session, "resize",
    serde_json::json!({ "input": "/Users/asha/Documents/Brand/logo.png",
                        "output": "/Users/asha/Documents/Brand/logo-512.png", "width": 512 })).await;
// ToolOutput { structured: Some({"written": "logo-512.png", "version": 1}), .. }
// — the app wrote to a scratch file; the File Service recorded Brand/logo-512.png version 1
//   with origin Run { run_id, tool: "craft_photocraft.resize" }.

let copy = craft.make_copy(MakeCopy { original_name: "rates.md".into(), original_from: "supplier-docs".into(),
    branch: Some("main".into()), markdown, format: CopyFormat::Word, updated_at }).await?;
// MadeCopy { file_name: "rates.docx", maker: WordCraft, bytes: … }   (only when C-28 is confirmed)
```

## Internal Logic

### 1. Start and profile changes

1. `migrations()`: module `craft`, step 1 (tables below), registered by the program.
2. `start`: validate `definitions.format == 1`; each definition's `versions` must parse as a SemVer requirement, each `path_args` name unique per tool, at most one tool per `converts_markdown_to` per app — a defect fails the build's test, and at run time the offending app entry is skipped and logged `definition_invalid {app}`.
3. Apply the profile's file-type map (step 3). `CraftAppsProvider::servers()` lists one server `craft_<app>` for each installation with `status = installed`, `Mcp ∈ cli_modes` and a matching definition; the program registers the provider before the MCP Front starts (SA-RECON-01).
4. Subscribe to `profile.subscribe()` and build `CraftProfile` from `document.craft` at each snapshot. On a new `profile_version`: re-apply the file-type map. The toolkit does not download on its own: apps the new profile adds or changes are installed at the Setup Assistant's next guided or check run, through `check_apps(true)`, where it shows progress.

### 2. Installing (`sync_installations`; ADR-016, ADR-072)

1. For each `craft.apps[]` pin: app name 1–40 chars `[a-z0-9-]` (else skipped with `CRAFT_NOT_APPROVED` logged); a row is created `approved` if absent (`CRAFT_DUPLICATE` guards the unique name).
2. Rows whose app is no longer in the profile → `removed`; their server leaves `servers()` (announced in step 4), their running servers are stopped, and `INSTALL_ROOT/<app>/` deleted.
3. For each row needing work (`approved`, `verification_failed` with a changed pin, or `installed` with `installed_version != pinned_version`):
   1. The platform entry must exist in the pin (else `CRAFT_PLATFORM_UNSUPPORTED`, the row stays `approved`, and the app is reported as not available on this computer).
   2. URL = the platform's `asset_url` (SA-CLOSE-01). Download through `reqwest` to `SCRATCH_ROOT/downloads/<file_name>.part` within `DOWNLOAD_TIMEOUT`, hashing as it streams. Failure → `CRAFT_DOWNLOAD_FAILED` (retryable; backoff 1 s, 2 s, 4 s, 3 tries).
   3. SHA-256 ≠ the pin → delete the file, `verification_result = failed`, `status = verification_failed`, `CRAFT_HASH_MISMATCH` (permanent until the profile pins another file). Journal `craft_verification_failed {app, version}`.
   4. Hash matches → `status = downloaded`. Unpack the zip (`zip`, B7 contract D-11) into `INSTALL_ROOT/<app>/<version>.tmp/`, refusing any entry whose path is absolute or contains `..` (`CRAFT_INSTALL_FAILED`); rename to `<version>/`. On macOS run `/usr/bin/codesign --verify --deep --strict` on the CLI and the app bundle and record the result (C-47: recorded, not gating); clear the quarantine attribute is **not** done (ADR-072: company allow-listing is respected, never worked round).
   5. Start check (ADR-072 "After installing or updating a program it checks that the program starts"): run `<cli> --version` with a 10 s limit. The OS refusing to start it (permission denied, a code-signing or allow-listing refusal reported by the OS, or no output before the limit) → `CRAFT_START_BLOCKED { program, reason }` and a `BlockedProgram` entry with the publisher read from the signature when available and the asset's hash; only this app is turned off (status stays `downloaded`).
   6. Detect modes: run `<cli> --help` (10 s) and set `cli_modes` from the subcommands it lists among `mcp`, `run`, `batch` (C-07).
   7. `verification_result = verified`, `installed_version = pinned`, `status = installed` (`CRAFT_UNVERIFIED_INSTALL` guards the rule). Delete the previous version folder after any running servers of it stopped. Journal `craft_installed {app, version, modes}`.
4. For each app whose server was added or removed, or whose tool list changed, call `tools_changed("craft_<app>")` on the MCP Front's registry (SA-RECON-01). Return the report.

### 3. File-type map (`craft.file_types`)

1. Group the map by app; extensions lower-cased, without a dot, validated `[a-z0-9]{1,16}` (`FTM_EXTENSION_INVALID`; that extension is skipped and logged). An app value that is not an approved app maps the extension to `open_with_app = None` (system default).
2. Each group becomes one `FileTypeMapEntry` with `media_types` from the extension's IANA type (the same table the File Service uses), `craft_tools` = the app's `allowed_tools` that exist in its definition (every tool of its definition when `allowed_tools` is `None`), `profile_version` = the profile's.
3. Replace all rows in one transaction (the rows are never edited otherwise, `FTM_IMMUTABLE`). Extensions not in the map have no entry (system default).
4. Word, Excel and PowerPoint kinds open in the person's own Microsoft app (11 §5.6 table) because IT maps them to no ArtCraft app, or because the mapped app is not installed; the toolkit has no special case for them.

### 4. Opening for the person

1. `open_in_app(place, path)`: resolve the absolute path through `files.place` and the relative path (normalised; no `..`); the file must exist (`CRAFT_OPEN_FAILED`).
2. Entry for the extension with `open_with_app = Some(app)` and that app `installed` → open the app's window with the file: macOS `open -a "<INSTALL_ROOT>/<app>/<version>/<macos_bundle>" "<file>"`; Windows `<INSTALL_ROOT>\<app>\<version>\<windows_exe> "<file>"`; Linux `<INSTALL_ROOT>/<app>/<version>/<linux_exe> "<file>"`, each started detached with arguments passed as an argument list (never through a shell). → `OpenedInApp`. A start refused by the OS → `CRAFT_START_BLOCKED` (UX-A46 words: "Your company's settings stopped [program] from opening on this computer." with **Send to IT**, 11 §6).
3. Otherwise → `NoEditor`; the screens ask "No editor for this kind of file — open it in your usual app?" with **Open in your usual app** (11 §6), which calls `open_with_system_default`: macOS `open "<file>"`, Windows `explorer.exe "<file>"` *(chosen here: the shell's own handler, without `cmd /c`)*, Linux `xdg-open "<file>"`.
4. Hand edits in the app's window are saved by the app; the File Service's watcher records them as versions (03:182 sequence). Journal `file_opened_in_app {app}`.

### 5. Session servers (`CraftAppProvider`)

1. One provider for every app; one server `craft_<app>` per app (SA-RECON-01).
2. A session's server is started on the first `tools` or `call` for that session: spawn `<cli> mcp` with the session's environment cleared except `PATH`, `HOME`/`USERPROFILE`, `TMPDIR`/`TEMP`, `LANG` (*chosen here*: no tokens or credentials reach the app, 07 *Security posture*), working directory `SCRATCH_ROOT/<run_id>/`; connect an rmcp client over its stdio; `initialize` within `START_TIMEOUT` (else `CRAFT_SERVER_START_FAILED`, process killed). At most `MAX_SERVERS`; when full, the least recently used idle server is stopped first.
3. `servers()` lists `craft_<app>` with the definition's tools; `offer(grant)`: the app's `tools/list`, kept only where the tool is in the app's definition and in the profile's `allowed_tools` (when that list is `None`, every tool in the definition) (the package's declaration is the Rules Engine's check before `call`); each descriptor's `description` is the definition's one line (ADR-062); `input_schema` is the app's. An app without MCP mode, or with no definition, offers no tools and is reported by `start_up_items` as `UnavailablePart { part: "craft_<app>", plain: "<App name> can't be used by agents on this computer yet.", reason: MissingDependency }`.
4. `session_closed(run_id)` and `SESSION_IDLE_STOP` stop the server (close stdin, 2 s, then kill) and delete `SCRATCH_ROOT/<run_id>/`.

### 6. A Craft tool call (`CraftAppProvider::call`)

The MCP Front's Rules Engine decision has already allowed the call, including its path rules over the arguments (ADR-017, ADR-039). Then:

1. The tool must be offered to this session (step 5.3), else `CRAFT_TOOL_NOT_OFFERED`.
2. For each `PathArg` of the tool's definition, the argument must be a string holding an absolute path (else `CRAFT_ARGUMENTS_INVALID`). Each `Input` path must lie in a place (`files.locate`) — else `CRAFT_PATH_NOT_ALLOWED` (a defence behind the Rules Engine's decision). Each `Output` path is resolved the same way to `(place_id, relative)` and its argument is **replaced** by a scratch path `SCRATCH_ROOT/<run_id>/<uuid>.<same extension>`.
3. Call the tool through the session's rmcp client within `CALL_TIMEOUT`. Timeout → kill the session's server (it is restarted on the next call) and return `CRAFT_TIMEOUT` (03:182 "The CLI exceeds its time limit: it is stopped and the call fails"). An MCP error result → `CRAFT_TOOL_FAILED` with the app's message text passed to the agent as data, cut to 2,000 chars.
4. For each output: the scratch file must exist and be non-empty (else `CRAFT_OUTPUT_MISSING`; no partial file replaces a version, 03:182), ≤ `MAX_OUTPUT_BYTES` (else `CRAFT_OUTPUT_TOO_LARGE`); then `files.write_version(WriteVersion { place_id, path: relative, bytes, origin: Run { run_id, tool: "craft_<app>.<tool>" }, expected_hash: None })`. A File Service error → `CRAFT_FILES_FAILED`. Delete the scratch file.
5. Return `ToolOutput { text: <compact JSON>, structured: Some({ "written": [ { "path": relative, "version": n } ], "app_result": <the app's result with scratch paths replaced by the original output paths> }) }`. Errors return `ToolFailure { code: "<CRAFT_…>", message: "<plain sentence>", retryable: CraftError::retryable() }`.
6. Journal `craft_tool_called {app, tool, run_id, outputs, elapsed_ms, result}` (no argument values, digests only).

### 7. Conversions for published copies (`make_copy`; ADR-048, ADR-069)

1. `format` must be `Word`, `Pdf` or `Slides` (else `CRAFT_CONVERSION_UNAVAILABLE`); the maker app is WordCraft, PdfCraft or DeckCraft respectively (app names `wordcraft`, `pdfcraft`, `deckcraft`); `conversion_available(format)` = that app is `installed` with `Mcp` mode and its definition has a tool with `converts_markdown_to = format` (C-28). False → `CRAFT_CONVERSION_UNAVAILABLE`.
2. Banner (ADR-069): `BANNER_TEMPLATE` filled with `original_name`, `original_from`, `branch`, and `updated_at` formatted in the active locale (SA-I18N-02). Word and PDF: the Markdown passed is the banner as one paragraph, a blank line, then the original. Slides: when the definition has `footer_arg`, pass the banner there for the first slide; otherwise append the banner as the last paragraph of the first slide's content (the text under the first heading) *(assumed here until C-28 shows DeckCraft's arguments)*.
3. Path checks (02:754 "under the same path checks"): input `SCRATCH_ROOT/conversions/<uuid>.md` (written by the toolkit) and output `SCRATCH_ROOT/conversions/<uuid>.<docx|pdf|pptx>`; each is checked with `rules.check_path_access(&PathAccessRequest { workspace_id: None, agent: None, crew_charter_id: None, path, access })`; a verdict other than `Allow` → `CRAFT_PATH_NOT_ALLOWED`.
4. Start a conversion server for the app (one shared conversion session, `run_id` absent; stopped after `SESSION_IDLE_STOP`), call the tool within `CALL_TIMEOUT`, read the output (`CRAFT_OUTPUT_MISSING` / `CRAFT_OUTPUT_TOO_LARGE` as step 6.4), delete both scratch files.
5. `file_name` = the original's stem plus the format's extension (`rates.docx`, `rates.pdf`, `rates.pptx`); `maker` = `WordCraft`, `PdfCraft` or `DeckCraft`. The caller (Push Service) uploads it; no file version is written here. Journal `craft_copy_made {format, elapsed_ms}`.

### 8. Setup Assistant check (`check_apps`)

1. When `repair`: run `sync_installations` and keep its `blocked` list; an error of it is not returned (each app's state shows the outcome).
2. For each `craft.apps[]` pin, in profile order: a row `installed` at the pinned version → `Ready` (or `Repaired` when it was not `installed` at that version before step 1); an app in the `blocked` list, or whose last start check failed → `Refused { publisher, sha256 }` from its `BlockedProgram` (publisher `"unknown"` when no signature was read); any other state → `Missing`.
3. Without `repair`, step 2 reads the rows only (no download, no start check). A datastore failure → `CRAFT_DATASTORE_UNAVAILABLE`.

### 9. Reverse conversion (`to_markdown`; SA-RECON-03)

1. `format` other than `Word` → `CRAFT_CONVERSION_UNAVAILABLE`. The maker app is `wordcraft`; it must be `installed` with `Mcp` mode and its definition must have a tool with `converts_to_markdown_from = Word` (C-13), else `CRAFT_CONVERSION_UNAVAILABLE`.
2. `bytes` over `MAX_OUTPUT_BYTES` → `CRAFT_OUTPUT_TOO_LARGE`. Write them to `SCRATCH_ROOT/conversions/<uuid>.docx`, output `SCRATCH_ROOT/conversions/<uuid>.md`; path checks as section 7 step 3.
3. Call the tool through the shared conversion session within `CALL_TIMEOUT` (`CRAFT_TIMEOUT`, `CRAFT_TOOL_FAILED`, `CRAFT_OUTPUT_MISSING` as section 6); read the Markdown as UTF-8 (invalid UTF-8 → `CRAFT_TOOL_FAILED`); delete both scratch files. Journal `craft_markdown_made {elapsed_ms}`.

## Data Model

SQLite (desktop, personal server).

```sql
CREATE TABLE craft_installations (
    id                   TEXT PRIMARY KEY,
    app_name             TEXT NOT NULL UNIQUE CHECK (length(app_name) BETWEEN 1 AND 40),
    pinned_version       TEXT NOT NULL,
    installed_version    TEXT,
    verification_result  TEXT CHECK (verification_result IN ('verified','failed')),
    cli_modes            TEXT NOT NULL DEFAULT '[]',          -- JSON Vec<CraftCliMode>
    status               TEXT NOT NULL CHECK (status IN ('approved','downloaded','installed','verification_failed','removed')),
    signature_check      TEXT,                                -- 'valid', 'invalid', 'unsigned', 'not_checked' (C-47, Diagnostics)
    updated_at           TEXT NOT NULL,
    CHECK (status != 'installed' OR verification_result = 'verified')
);

CREATE TABLE file_type_map_entries (
    id               TEXT PRIMARY KEY,
    extensions       TEXT NOT NULL,                           -- JSON Vec<String>
    media_types      TEXT NOT NULL,                           -- JSON Vec<String>
    open_with_app    TEXT,
    craft_tools      TEXT NOT NULL,                           -- JSON Vec<String>
    profile_version  TEXT NOT NULL
);

CREATE TABLE file_type_extensions (                           -- lookup and the one-entry-per-extension rule
    extension  TEXT PRIMARY KEY CHECK (length(extension) BETWEEN 1 AND 16),
    entry_id   TEXT NOT NULL REFERENCES file_type_map_entries(id) ON DELETE CASCADE
);
```

Craft tool definitions live in the bundled `craft-tools.json`, not in a table (*chosen here*: they change with an ordin release, never at run time). Rollback: the migration only creates tables; removing an app deletes only files under `INSTALL_ROOT`.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| App not approved in the profile | `Err(permanent)` | `CRAFT_NOT_APPROVED` | `{"error":"Your company hasn't approved this app.","code":"CRAFT_NOT_APPROVED","action":null}` |
| App approved but not installed | `Err(retryable)` | `CRAFT_NOT_INSTALLED` | `{"error":"This app isn't installed yet. ordin will install it.","code":"CRAFT_NOT_INSTALLED","action":"try_again"}` |
| Download failed after retries | `Err(retryable)` | `CRAFT_DOWNLOAD_FAILED` | `{"error":"ordin couldn't download this app.","code":"CRAFT_DOWNLOAD_FAILED","action":"try_again"}` |
| Downloaded file does not match the pinned hash | `Err(permanent)` | `CRAFT_HASH_MISMATCH` | `{"error":"This app's download didn't pass ordin's safety check.","code":"CRAFT_HASH_MISMATCH","action":"send_to_it"}` |
| No asset for this platform in the pin | `Err(permanent)` | `CRAFT_PLATFORM_UNSUPPORTED` | `{"error":"This app isn't available for this computer yet.","code":"CRAFT_PLATFORM_UNSUPPORTED","action":null}` |
| Unpacking failed or an unsafe archive path | `Err(permanent)` | `CRAFT_INSTALL_FAILED` | `{"error":"ordin couldn't install this app.","code":"CRAFT_INSTALL_FAILED","action":"send_to_it"}` |
| The operating system refused to start the program | `Err(permanent)` | `CRAFT_START_BLOCKED` | `{"error":"Your company's settings stopped PhotoCraft from opening on this computer.","code":"CRAFT_START_BLOCKED","action":"send_to_it"}` |
| App has no MCP mode | `Err(permanent)` | `CRAFT_NO_MCP_MODE` | — (start-up report item; agents get no tool) |
| App's MCP server did not start in 15 s | `Err(retryable)` | `CRAFT_SERVER_START_FAILED` | tool result `{"error":"The app didn't start.","code":"CRAFT_SERVER_START_FAILED"}` |
| Tool not offered to this session | `Err(permanent)` | `CRAFT_TOOL_NOT_OFFERED` | tool result `{"error":"This tool isn't available to you.","code":"CRAFT_TOOL_NOT_OFFERED"}` |
| A path argument outside every place, or a conversion path refused | `Err(permanent)` | `CRAFT_PATH_NOT_ALLOWED` | tool result `{"error":"That file is outside the folders you can use.","code":"CRAFT_PATH_NOT_ALLOWED"}` |
| A path argument missing or not an absolute path | `Err(permanent)` | `CRAFT_ARGUMENTS_INVALID` | tool result `{"error":"Give the file as a full path.","code":"CRAFT_ARGUMENTS_INVALID"}` |
| Tool call or conversion exceeded 120 s | `Err(retryable)` | `CRAFT_TIMEOUT` | tool result `{"error":"The app took too long and was stopped.","code":"CRAFT_TIMEOUT"}` |
| The app returned an error | `Err(permanent)` | `CRAFT_TOOL_FAILED` | tool result `{"error":"<the app's message, as data>","code":"CRAFT_TOOL_FAILED"}` |
| Output file missing or empty | `Err(permanent)` | `CRAFT_OUTPUT_MISSING` | tool result `{"error":"The app didn't write the file.","code":"CRAFT_OUTPUT_MISSING"}` |
| Output above 256 MiB | `Err(permanent)` | `CRAFT_OUTPUT_TOO_LARGE` | tool result `{"error":"The file the app wrote is too large.","code":"CRAFT_OUTPUT_TOO_LARGE"}` |
| Conversion app not confirmed or not installed (C-28; for `to_markdown`, C-13) | `Err(permanent)` | `CRAFT_CONVERSION_UNAVAILABLE` | `{"error":"Word, PDF and slides copies aren't ready yet.","code":"CRAFT_CONVERSION_UNAVAILABLE","action":null}` |
| Opening the window or the system default failed | `Err(retryable)` | `CRAFT_OPEN_FAILED` | `{"error":"ordin couldn't open this file.","code":"CRAFT_OPEN_FAILED","action":"try_again"}` |
| File Service write failed | `Err(retryable)` | `CRAFT_FILES_FAILED` | tool result `{"error":"ordin couldn't save the file.","code":"CRAFT_FILES_FAILED"}` |
| Own tables unreachable | `Err(retryable)` | `CRAFT_DATASTORE_UNAVAILABLE` | `{"error":"ordin couldn't reach its own records.","code":"CRAFT_DATASTORE_UNAVAILABLE","action":"try_again"}` |
| Duplicate app row; installed without verification; bad file-type map entry | `Err(permanent)` (defect guard) | `CRAFT_DUPLICATE`, `CRAFT_UNVERIFIED_INSTALL`, `FTM_EXTENSION_INVALID`, `FTM_EXTENSION_DUPLICATE`, `FTM_MEDIA_TYPE_INVALID`, `FTM_IMMUTABLE` | — (internal) |

Tool results reach the model through the MCP Front, never the person; the person sees run progress (11 §5.4).

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Craft Toolkit

  Scenario: Happy path — install an approved app from the mirror
    Given the profile approves photocraft 0.7.0 for macos_arm64 with asset_url "https://mirror.acme.co.uk/craft/photocraft/0.7.0/photocraft-cli-0.7.0-macos-universal.zip", file_name "photocraft-cli-0.7.0-macos-universal.zip" and its sha256
    When sync_installations is called
    Then the asset is downloaded from "https://mirror.acme.co.uk/craft/photocraft/0.7.0/photocraft-cli-0.7.0-macos-universal.zip"
    And the installation is installed with verification verified and cli modes including mcp

  Scenario: Error path — hash mismatch is never installed
    Given the downloaded file's sha256 differs from the pin
    When sync_installations is called
    Then the installation is verification_failed, the file is deleted and nothing is unpacked

  Scenario: Error path — the operating system blocks the program
    Given starting "photocraft-cli --version" is refused by the operating system
    When sync_installations is called
    Then the report lists photocraft-cli as blocked with its sha256
    And only photocraft is turned off

  Scenario: Edge case — an archive entry escaping the folder is refused
    Given the zip holds an entry "../../evil"
    When sync_installations unpacks it
    Then it fails with CRAFT_INSTALL_FAILED and no file is written outside the install folder

  Scenario: Happy path — open an image in PhotoCraft
    Given "png" maps to photocraft, which is installed
    When open_in_app is called for "Brand/logo.png"
    Then PhotoCraft's window is started with that file and the outcome is opened_in_app

  Scenario: Edge case — no editor for the kind
    Given "xyz" has no entry in the file-type map
    When open_in_app is called for "notes.xyz"
    Then the outcome is no_editor
    And open_with_system_default opens it with the system's default app

  Scenario: Happy path — an agent's output becomes a new version
    Given a session with photocraft offered and the tool "resize" defined with output path argument "output"
    When call is made with output "/Users/asha/Documents/Brand/logo-512.png"
    Then the app is given a scratch path in place of the output
    And the File Service records "logo-512.png" with origin run and tool "craft_photocraft.resize"

  Scenario: Error path — the app times out
    Given the app does not answer within 120 seconds
    When call is made
    Then the session's app server is stopped and the result is CRAFT_TIMEOUT with no version written

  Scenario: Error path — the app writes nothing
    Given the app returns success but the scratch output is empty
    When call is made
    Then the result is CRAFT_OUTPUT_MISSING and no version replaces the existing file

  Scenario: Edge case — a tool missing from the definitions is never offered
    Given photocraft's MCP server lists "export_all" which craft-tools.json does not define
    When tools is called for a session
    Then "export_all" is not in the list

  Scenario: Edge case — an app without MCP mode
    Given gridcraft is installed with modes run and batch only
    When start_up_items is called for a session
    Then it includes an unavailable_part item for "craft_gridcraft" with reason missing_dependency

  Scenario: Error path — Word copies before C-28
    Given wordcraft's definition has no tool converting markdown to word
    When make_copy is called with format word
    Then it fails with CRAFT_CONVERSION_UNAVAILABLE and conversion_available(word) is false

  Scenario: Happy path — the Setup Assistant repairs a missing app
    Given wordcraft is pinned at 1.4.0 and not installed
    When the Setup Assistant calls check_apps(true) and the download verifies
    Then wordcraft's state is repaired

  Scenario: Error path — check_apps reports an app the system refuses to start
    Given pdfcraft is installed and the operating system refuses to start it
    When the Setup Assistant calls check_apps(false)
    Then pdfcraft's state is refused with its publisher and sha256

  Scenario: Edge case — an app installed while ordin runs reaches new sessions
    Given the MCP Front has started with the craft provider and no app installed
    When sync_installations installs deckcraft with MCP mode
    Then tools_changed("craft_deckcraft") is called
    And servers() lists "craft_deckcraft"

  Scenario: Error path — Word to Markdown before C-13
    Given wordcraft's definition has no tool converting word to markdown
    When to_markdown is called with format word
    Then it fails with CRAFT_CONVERSION_UNAVAILABLE

  Scenario: Happy path — a PDF copy carries the line naming its original
    Given pdfcraft is installed and defines a markdown-to-pdf tool
    When make_copy is called for "rates.md" from "supplier-docs" on branch "main"
    Then the Markdown given to PdfCraft starts with "Published copy of rates.md, from supplier-docs, branch main."
    And the result is "rates.pdf" made by pdf_craft

  Scenario: An app without an allowed-tools list offers every defined tool
    Given the profile approves photocraft 0.7.0 with no allowed_tools key and it is installed with MCP mode
    When a session's grant asks the craft_photocraft server for its tools
    Then every tool that is both in photocraft's tools/list and in craft-tools.json is offered

  Scenario: A pin with a non-https address is dropped
    Given the profile pins photocraft for linux_x86_64 with asset_url "http://mirror.acme.co.uk/p.zip"
    When sync_installations is called on Linux x86-64
    Then photocraft is reported with CRAFT_PLATFORM_UNSUPPORTED and nothing is downloaded
```

## Performance / Security / Observability

- **Performance targets.** `file_kind` ≤ 1 ms (in-memory map); `open_in_app` ≤ 150 ms p95 to the window's start request (A-03); first `tools` for a session ≤ 3 s p95 including the app server's start (*chosen here*); tool-call overhead outside the app ≤ 100 ms p95 plus the File Service write; at most `MAX_SERVERS` child processes.
- **Security.** Downloads only from the `https` address the profile pins for each platform, verified against the profile's SHA-256 before anything is unpacked; archive paths cannot escape the install folder; installation never needs administrator rights and never clears quarantine or bypasses allow-listing (ADR-072); app servers start with a cleared environment, no tokens, and the session's scratch folder as working folder; every call was already decided by the Rules Engine at the MCP Front, and paths are checked again against places; outputs are written only through the File Service as versions; only tools in both the shipped definitions and the profile's allowed list are offered (fail closed); programs are started with argument lists, never a shell.
- **Never logged.** Tool argument values (digests only, 07 *Logging*), file paths and content, the app's result text.
- **Observability (logs only; SA-XC-04/05).** `tracing` events with `component = "craft"`: `craft_download {app, version, bytes, elapsed_ms, result}`, `craft_verification_failed {app, version}`, `craft_installed {app, version, modes}`, `craft_start_blocked {program}`, `craft_server_started {app, run_id}`, `craft_server_stopped {app, run_id, reason}`, `craft_tool_called {app, tool, run_id, outputs, elapsed_ms, result}`, `craft_copy_made {format, elapsed_ms}`, `craft_markdown_made {elapsed_ms}`, `definition_invalid {app}`.

## Gaps

None.
