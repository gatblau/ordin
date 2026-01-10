# SPEC: Agent Loader

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-107; promoted to Approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-agent-loader` · **Build phase:** 6 · **Programs:** A
> **Dependencies:** Agent Configuration, Model Policy, Connection Binder, MCP Front, Rules Engine, File Service

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

## Purpose

Obtains the agent release a deployment names and verifies its digest; refuses a release prepared with an ordin version or a contract version outside the range this ordin supports, and a datastore schema older than ordin needs; and builds what each run's harness session is given — the bundle as the working folder, read-only; the model settings; the tools the Connection Binder exposes and the File Service's file tools, which the server program registers on the MCP Front; the reviewer and general-purpose sub-agents under the Rules Engine; the controls; and the form of the single-call answer (`02:788`). It also builds the start-up report with wrightrun's items and logs it once when an instance starts (`02:791`, `08:35`). It owns the loaded release (`02:790`) and the release's local folder form, archive form and registry client (`08:42`, ADR-098 item 2: "the same artefact … the same reference form, media types and digest check"), which the Release Builder also uses to publish. It runs in every instance of the agent profile at start, in the obtain-a-release sub-command, and in the check and run-once commands (`03:1543`–`1574`).

## Approach

One asynchronous `load` performs a fixed sequence — release record and ranges, manifest, configuration, datastore schema, model settings, connections, file tools, rules, session set-up, start-up report — and returns one immutable `LoadedAgent` the server program hands to the Session Supervisor and the Caller API. The release code (folder form, archives, digest, registry client) is a module of this crate because both the obtain step here and the Release Builder's publish step need it, and the Release Builder depends on this crate (2B-3). Reads and writes that need the Journal's datastore — the schema version, the release record and the start-up report — go through a `LoaderStore` trait this crate defines and the server program implements over the Journal, because 2B-3 gives this crate no Journal dependency. Rejected: building the session set-up per run, because every run of a release gets the same set-up and a failure must stop the instance before it reports ready (`03:1574`); a registry client written by hand, because SA-OCI-01 names `oci-client`.

**Error prefixes:** `LOAD_` (wrightrun's, kept) and `REL_` (wrightrun's, kept; this crate's `release` module raises every `REL_` code, the Release Builder's publish included).

### Reconciliation assumptions

- **SA-RECON-01** The obtain sub-command is `ordin-server agent release obtain`; OrdinServer owns the command table (reconciliation row 583, ruling R6).
- **SA-RECON-02** The Agent Loader supplies the hosted agent's single `workspace_id`, read or created through `LoaderStore::hosted_workspace` so that every instance sharing a datastore agrees (reconciliation row 588, O75).
- **SA-CLOSE-01** *(K16)* The hosted agent's `AgentInstanceId` is the server program's: minted once as a UUIDv7 on the first agent start, kept in its `server_settings` table, passed in `LoadInput.agent_instance_id` and returned unchanged in `LoadedAgent.agent_instance_id`. This crate never mints or stores it.
- **SA-CLOSE-02** *(chosen here; owns the codes SessionSupervisor.md attributes to this crate)* Command expansion (wrightrun *CommandExpansion*, kept) is `LoadedAgent::expand_command(slug, arguments)` over the loaded bundle and manifest (L-23). It raises `CommandError` with exactly the three codes `CMD_UNKNOWN_COMMAND`, `CMD_PROMPT_UNREADABLE` and `CMD_PLACEHOLDER_MISSING`, which the server program's `SupervisorHost::expand_command` passes through as `HostError { code }`; the Session Supervisor then ends the run's Run Queue record `failed` with that code (SessionSupervisor.md, Error Table, *Command expansion fails*). It is synchronous, reads one file of the read-only bundle, and logs nothing, because the arguments are a person's message or an event's content.
- **SA-RECON-03** The callees are used under their own names (rulings R2, R5, R6): `RulesEngine::load_rule_set(InstanceRuleSet)` replaces `HostedRules::install`; `ToolRegistry::served_tools(&ServerName)`; the File Service's `WorkFolder` is wrapped here as the `files` provider (`delete_file` and the File Service's argument names), opened through the program's `WorkFolderOpener` because this crate has no Journal edge.
- **SA-RECON-04** This crate registers no tool provider with the MCP Front (McpFront.md *Registration at start-up*: only the programs call `register`; re-audit rows 11 and 76). `load` builds the `files` provider and returns it through `LoadedAgent::tool_provider()`; the Connection Binder's `agent` provider is returned through `LoadedAgent.bound.tool_provider()`. The server program registers both after `load` returns and before `McpFront::start`. The only MCP Front call this crate makes is `ToolRegistry::served_tools` (L-17).

## Shared Context

**From 2D (copied).**

| Variable | Type | Required | Owner | Description |
|---|---|---|---|---|
| `ORDIN_RELEASE` | string `<registry>/<repository>@sha256:<hex>` | Yes, for the obtain-a-release sub-command | Agent Loader | The agent release a deployment names. The start command does not read it; it reads the reference and digest the obtain step wrote beside the release. |
| `ORDIN_REGISTRY_AUTH_FILE` | path | No; without it the registry is read with no credential | Agent Loader, Release Builder | Credentials file for the container registry. Never printed or logged. |
| `ORDIN_RELEASE_DIR` | path | Yes, for the agent-profile start, obtain and `agent job run-one` sub-commands | Agent Loader | Folder holding the obtained release; the bundle is read-only there. |

The server program reads these once at start and passes them in (2D-1a). The sub-commands are `ordin-server agent <command>` with wrightrun's command names in kebab-case (`08:47`, *fixed* in 0.48.0).

**From 2C (copied).**

```rust
pub struct AgentReleaseId(pub uuid::Uuid);   // UUIDv7
pub struct StartupReportId(pub uuid::Uuid);
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>); // RFC 3339, ms, Z

pub struct AgentRelease {                     // owner per 2C: Release Builder (see Data Model)
    pub id: AgentReleaseId,
    pub reference: String,                    // registry/repository:tag
    pub digest: String,                       // "sha256:" + 64 lowercase hex
    pub bundle_path: String,
    pub agent_configuration_path: String,
    pub configuration_files: Vec<String>,
    pub package_name: String,
    pub package_version: String,
    pub ordin_version: String,
    pub contract_version: String,
    pub startup_report_id: StartupReportId,
    pub state: AgentReleaseState,
    pub published_at: Timestamp,
}
pub enum AgentReleaseState { Published, Obtained, Running }

pub struct StartupReport {
    pub id: StartupReportId,
    pub subject: StartupSubject,              // here: StartupSubject::AgentRelease { agent_release_id }
    pub enforced_rules: Vec<String>,
    pub rules_not_enforced: Vec<RuleNotEnforced>,
    pub unavailable_parts: Vec<String>,
    pub available_tools: Vec<String>,
    pub adapter_verbs_not_offered: Vec<String>,
    pub generated_at: Timestamp,
}
pub struct RuleNotEnforced { pub rule: String, pub reason: String }
// limits: each list ≤ 500 items of ≤ 500 characters (LD-42)
```

**From the Agent Configuration spec (copied; `ordin_types::domain::agent_profile` and `ordin-agent-configuration`).** `AgentConfig` (fields used: `package`, `work_namespace`, `run_limit_minutes`, `writable_paths: Option<Vec<String>>`, `chat`, `knowledge`, `controls`, `settings`, `contract`, `mappings`, `auth`, `traces`, `reach: Option<ReachConfig { channels, users, direct_message_users }>`, `approvers: Option<ApproversConfig { users, groups, group_claim, asker_may_approve_internal }>`, `intakes`, `schedules`, `retention`); `Manifest` (fields `schema_version`, `ordin_version`, `compiler`, `package { name, version }`, `authority { outcome, modes }`, `instruction_count`, `skills`, `tools`, `mcp_servers`, `commands`, `reviewer: Option<ReviewerFacts { name, description }>`, `recurring_duties`, `memory_skill`, `declaration: Option<Declaration>`), `Declaration.gates.duties: Vec<DutyGate { duty, command, folder, .. }>`; functions `load_config(&Path)`, `check_against_manifest(&AgentConfig, &Manifest)`, `not_carried(&AgentConfig)`; constants `NAMESPACE`, `GENERAL_PURPOSE = "general-purpose"`, `CONFIG_FILE_NAME_IN_RELEASE = "wrightrun.yaml"`. Errors: `ConfigError { code, message, location }` (`CFG_*`), passed through unchanged.

**From the Model Policy spec (copied).** `pub async fn ordin_model_policy::build(config: &AgentConfig, keys: &KeySource) -> Result<ModelPolicy, ModelPolicyError>`; `ModelPolicy::harness_settings() -> HarnessModelSettings`; `ModelPolicy::report() -> ModelPolicyReport { model, fallback, gateway_used, safe_reply, key_from, not_carried, environment_names }`; `KeySource { key_file, secret_store, environment_secrets: Arc<dyn EnvironmentSecrets> }` (the program supplies `environment_secrets`, ADR-107); `HarnessModelSettings { provider: ModelProvider /* struct(String) */, primary_model, fallback_model, gateway_url, key: Option<OrganisationKey>, environment: Vec<HarnessEnvVar> }` (in `ordin_types::domain::agent_profile`; *amended January 2026, ADR-107*: `key` is `None` only for a provider other than the default with no key set; `environment` holds the resolved `model.environment` entries, whose values are never logged). Errors `MPOL_*` (including `MPOL_SECRET_MISSING`, ADR-107), passed through.

**From the Connection Binder spec (copied).** `bind(BindInput { config, manifest, config_dir, secrets, hosts }) -> Result<BoundConnections, BindError>`; `BoundConnections { provider, descriptions, connections, connected_sections, unconnected_sections, approval, external, operations, sections, knowledge_tools }` and `tool_provider() -> Arc<dyn ToolProvider>` (called by the server program, SA-RECON-04); `ConnectionReport { name, kind, exposed, not_exposed, external }`; `Hosts`, `SecretSource`. Errors `TOOL_*`, passed through.

**From the MCP Front (copied from McpFront.md; ruling R2).** `ToolRegistry::served_tools(&self, &ServerName) -> Option<Vec<String>>` (the only registry method this crate calls, SA-RECON-04); the `ToolProvider` trait (`provider_id`, `servers() -> Vec<ServerSpec>`, `offer(&SessionGrant) -> Result<Vec<ToolKey>, ToolFailure>`, `check_arguments(&ToolKey, &Value) -> Result<(), ArgumentProblem>`, `call(ToolCallContext, &ToolKey, Value) -> Result<ToolOutput, ToolFailure>`, `session_closed(RunId)`) and its types `ServerName`, `ServerSpec`, `ToolSpec { name, description, input_schema, argument_form, sandbox_callable, plain }`, `ToolKey`, `ToolOutput`, `ToolFailure { code, message, retryable }`, as the Connection Binder spec copies them.

**From the File Service (copied from FileService.md, *Work folder* and *Agent file tools*).** The File Service implements no `ToolProvider`; this crate wraps a `WorkFolder` as the `files` provider.

```rust
pub struct WorkFolderConfig { pub namespace: String, pub bundle_dir: std::path::PathBuf,
    pub writable_paths: Option<Vec<String>>, pub protected_paths: Vec<String> }
impl WorkFolder {
    pub fn new(datastore: Datastore, config: WorkFolderConfig) -> Result<WorkFolder, FileError>;
    pub async fn ls(&self, path: &str) -> Result<Vec<WorkFileEntry>, AgentRefusal>;
    pub async fn read(&self, path: &str, offset: u32, limit: u32) -> Result<String, AgentRefusal>;
    pub async fn write(&self, path: &str, content: &str) -> Result<(), AgentRefusal>;
    pub async fn edit(&self, path: &str, old: &str, new: &str, replace_all: bool) -> Result<u32, AgentRefusal>;
    pub async fn delete(&self, path: &str) -> Result<(), AgentRefusal>;
    pub async fn glob(&self, pattern: &str, path: Option<&str>) -> Result<Vec<String>, AgentRefusal>;
    pub async fn grep(&self, pattern: &str, path: Option<&str>, max_count: Option<u32>) -> Result<Vec<GrepMatch>, AgentRefusal>;
}
pub struct AgentRefusal(pub String);   // the text is given to the model verbatim
```

| Tool (server `files`) | Arguments | `WorkFolder` method | `plain` |
|---|---|---|---|
| `ls` | `{ "path": string }` | `ls` | "Listed files in the work folder" |
| `read_file` | `{ "path": string, "offset": u32?, "limit": u32? }` (defaults 0, 2,000) | `read` | "Read a file in the work folder" |
| `write_file` | `{ "path": string, "content": string }` | `write` | "Wrote a file in the work folder" |
| `edit_file` | `{ "path": string, "old": string, "new": string, "replace_all": bool? }` (default false) | `edit` | "Changed a file in the work folder" |
| `delete_file` | `{ "path": string }` | `delete` | "Removed a file from the work folder" |
| `glob` | `{ "pattern": string, "path": string? }` | `glob` | "Looked for files by name in the work folder" |
| `grep` | `{ "pattern": string, "path": string?, "max_count": u32? }` (default 100) | `grep` | "Searched text in the work folder" |

Every tool is `sandbox_callable = false`; an `Err(AgentRefusal(text))` becomes `ToolFailure { code: "FILE_AGENT_REFUSED", message: text, retryable: false }`.

**From the Rules Engine (copied from RulesEngine.md; ruling R5).** `RulesEngine::load_rule_set(&self, set: InstanceRuleSet) -> Result<Vec<ControlLine>, RulesError>`;

```rust
pub struct InstanceRuleSet {
    pub agent_instance_id: AgentInstanceId, pub package_name: String, pub authority: Authority,
    pub agent_folder: std::path::PathBuf, pub agent_folder_read_only: bool,
    pub declaration: Option<Declaration>, pub configured: Option<ConfiguredControls>,
    pub tool_policy: Option<AgentToolPolicy>, pub approval_list: Vec<String>,
    pub package_read_only_tools: Vec<String>, pub tools: Vec<ToolDescription>,
    pub unmapped_operations: Vec<String>, pub external: Vec<String>,
    pub commands: Vec<String>, pub reviewer: Option<String>,
}
pub struct ConfiguredControls { pub message_rules: Vec<MessageRule>, pub steps: Vec<StepGate>,
    pub argument_rules: Vec<ArgumentRule>, pub schedule_entries: Vec<(String, String)> /* (duty, command) */,
    /// ADR-107; empty everywhere but the agent profile.
    #[serde(default)] pub content_checks: Vec<ContentCheck> }
// ContentCheck { name, connection, operation, points: Vec<CheckPoint>, on_block: OnBlock, on_unreachable: OnUnreachable,
//     categories: Vec<String>, timeout_seconds: u32, max_chars: u32 } — ordin_types::agent_profile (AgentConfiguration.md, ADR-107)
pub enum Authority { DocumentOnly, SystemChanging, RoutingOnly }
pub struct ToolDescription { pub name: String, pub arguments: Vec<String>, pub served_by: Option<String> }
pub struct ControlLine { pub name: String, pub source: ControlSource, pub point: Point, pub command: Option<String>,
    pub enforced: bool, pub reason: String, pub detail: String }
pub enum ControlSource { Package, Configuration, Application, AgentPolicy }
```

**Libraries (1A).** `oci-client` 0.18.0 (SA-OCI-01), `sha2` (already a dependency of `memory/`), `serde_json`, `tokio`, `tracing`, `thiserror`. **`tar`** and **`flate2`** (each `MIT OR Apache-2.0`) to unpack the two gzip-compressed tar parts are named in HLD 05, row *Archives*.

## Public Interface

### Module `release` (the artefact; wrightrun `AgentRelease.md`, kept)

```rust
pub const ARTIFACT_TYPE: &str = "application/vnd.wrightrun.release.v1";
pub const RECORD_MEDIA_TYPE: &str = "application/vnd.wrightrun.release.record.v1+json";
pub const BUNDLE_MEDIA_TYPE: &str = "application/vnd.wrightrun.release.bundle.v1.tar+gzip";
pub const CONFIG_MEDIA_TYPE: &str = "application/vnd.wrightrun.release.config.v1.tar+gzip";
pub const MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";
pub const MAX_MANIFEST_BYTES: u64 = 65_536;
pub const MAX_RECORD_BYTES: u64 = 1_048_576;
pub const MAX_PART_BYTES: u64 = 268_435_456;
pub const MAX_RELEASE_BYTES: u64 = 1_073_741_824;
pub const MAX_ENTRIES: u64 = 50_000;
pub const CONNECT_SECONDS: u64 = 10;
pub const REQUEST_SECONDS: u64 = 60;
pub const RELEASE_BUNDLE_DIR: &str = "bundle";
pub const RELEASE_CONFIG_DIR: &str = "config";
pub const RELEASE_CONFIG_FILE: &str = "config/wrightrun.yaml";
pub const RELEASE_RECORD_FILE: &str = "release.json";
pub const RELEASE_OBTAINED_FILE: &str = "obtained.json";
pub const CONTRACT_VERSION: u32 = 1;
pub const CONTRACT_MIN: u32 = 1;
pub const CONTRACT_MAX: u32 = 1;
pub const RELEASE_ID: &str = r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$";

/// `release.json`: UTF-8 JSON, these seven keys in this order, two-space indentation, one trailing newline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ReleaseRecord {
    pub package_name: String,
    pub package_version: String,
    pub release_id: String,                // RELEASE_ID
    pub runtime_version: String,           // the ordin version the release was prepared with (wrightrun's key, kept)
    pub contract_version: u32,
    pub manifest_schema_version: u32,
    pub startup_report: String,            // the check's report text, no digest; ≤ 500,000 characters
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedRelease {
    pub dir: std::path::PathBuf,
    pub record: ReleaseRecord,
    pub reference: Option<String>,         // from obtained.json; None for a local release folder
    pub digest: Option<String>,            // "sha256:<hex>"; None for a local release folder
}

/// A parsed reference `<registry>/<repository>[:<release_id>][@sha256:<64 hex>]`, ≤ 512 characters.
pub struct ReleaseReference { pub registry: String, pub repository: String, pub release_id: Option<String>, pub digest: Option<String> }
pub fn parse_reference(text: &str) -> Result<ReleaseReference, ReleaseError>;

pub struct RegistryAuth { /* parsed credentials file; Debug redacted */ }
pub fn read_auth_file(path: Option<&std::path::Path>) -> Result<RegistryAuth, ReleaseError>;

pub fn read_release(dir: &std::path::Path) -> Result<LoadedRelease, ReleaseError>;
pub fn check_supported(record: &ReleaseRecord, ordin_version: &str) -> Result<(), ReleaseError>;
pub async fn obtain(reference: &str, dest: &std::path::Path, auth: &RegistryAuth, ordin_version: &str) -> Result<LoadedRelease, ReleaseError>;
/// Used by the Release Builder: lays out a local release folder (bundle, configuration and the files it names, record).
pub fn write_release_dir(out_dir: &std::path::Path, bundle_dir: &std::path::Path, config_path: &std::path::Path,
                         named: &[std::path::PathBuf], record: &ReleaseRecord) -> Result<(), ReleaseError>;
/// Used by the Release Builder: packs a release folder and writes it to the registry; returns "sha256:<hex>".
pub async fn publish_dir(dir: &std::path::Path, reference: &str, auth: &RegistryAuth) -> Result<String, ReleaseError>;

#[derive(Debug, thiserror::Error)]
#[error("error[{code}]: {message}{}", .location.as_ref().map(|l| format!(" ({l})")).unwrap_or_default())]
pub struct ReleaseError { pub code: &'static str, pub message: String, pub location: Option<String> }
```

### Module `manifest`

```rust
pub const MANIFEST_FILE_NAME: &str = "wrightrun-manifest.json";
pub const MANIFEST_SCHEMA_VERSION: u32 = 3;   // ordin's form; 2 was wrightrun's (no wrightrun release exists)
pub fn read_manifest(bundle_dir: &std::path::Path) -> Result<Manifest, LoadError>;
```

### The load

```rust
// Types added to 2C (crate ordin-types, module domain::agent_profile): read by the server program, which maps
// `LoadedAgent.session` into the `SessionSetup` its `SessionSetupSource` gives the Claude Local Adapter.
#[derive(Debug, Clone)]
pub struct HostedSessionSetup {
    pub release_id: String,
    pub digest: Option<String>,
    pub working_dir: std::path::PathBuf,         // the bundle folder; read-only
    pub instructions_file: std::path::PathBuf,   // <bundle>/.ordin/claude/instructions.md (C-61: --append-system-prompt-file)
    pub plugin_dir: std::path::PathBuf,          // <bundle>/.ordin/claude/plugin (C-61: --plugin-dir)
    pub model: HarnessModelSettings,
    pub mcp_servers: Vec<String>,                // MCP Front servers the session is given: "agent", "files", and "knowledge" when any knowledge tool is exposed
    pub tool_policy: HarnessToolPolicy,
    pub subagents: Vec<SubagentSetup>,
    pub single_call: SingleCallForm,
    pub run_limit_secs: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarnessToolPolicy {
    pub builtin_file_writing: bool,              // false (ADR-098 item 3; C-79)
    pub builtin_file_reading_bundle_only: bool,  // true (C-79)
    pub shell: bool,                             // false (ADR-098 item 3: off)
    pub web: bool,                               // false (assumed here: wrightrun offered no web tool)
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentSetup { pub name: String, pub tools: SubagentTools }
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum SubagentTools { SameAsAgent, ReadAndSearchOnly }
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum SingleCallForm { OneTurnNoTool, NoBoundTool }

// crate ordin-agent-loader
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum LoadMode { Serve, Check, RunOnce }

#[async_trait::async_trait]
pub trait LoaderStore: Send + Sync {      // implemented by the server program over the Journal
    async fn schema_status(&self) -> Result<SchemaStatus, StoreUnavailable>;
    async fn record_loaded(&self, release: &AgentRelease, report: &StartupReport) -> Result<(), StoreUnavailable>;
    /// The hosted agent's one workspace identifier: created on the first call against a datastore, then
    /// the same for every instance that shares it (reconciliation row 588).
    async fn hosted_workspace(&self) -> Result<WorkspaceId, StoreUnavailable>;
}
pub enum SchemaStatus { Current, Older { found: String, needed: String }, NewerCompatible, NewerIncompatible { found: String } }
pub struct StoreUnavailable;

pub struct LoadInput<'a> {
    pub mode: LoadMode,
    pub release_dir: &'a std::path::Path,
    pub ordin_version: &'a str,
    pub harness_version: &'a str,             // the pinned Claude Code version (for the report)
    pub store: Option<&'a dyn LoaderStore>,   // None in Check and RunOnce
    pub keys: &'a ordin_model_policy::KeySource,
    pub secrets: std::sync::Arc<dyn ordin_connection_binder::SecretSource>,
    pub hosts: ordin_connection_binder::Hosts,
    pub mcp_front: &'a dyn ToolRegistry,     // read only through `served_tools` (L-17); nothing is registered here
    pub rules: &'a ordin_rules::RulesEngine,
    /// The hosted agent's instance identifier (SA-CLOSE-01): in `Serve`, the UUIDv7 the server program
    /// minted once and keeps in its `server_settings` table (OrdinServer.md SA-CLOSE-04); in `Check` and
    /// `RunOnce`, a fresh UUIDv7 the program mints for that one command and never stores.
    pub agent_instance_id: AgentInstanceId,
    /// Builds the work folder over the program's datastore (`WorkFolder::new(datastore, config)`); this crate
    /// has no Journal edge, so the program supplies it.
    pub work_folder: &'a dyn WorkFolderOpener,
    pub trace_status: TraceStatusView,        // from Telemetry (Off | On | SwitchedOnNoEndpoint), passed by the program
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum TraceStatusView { Off, On, SwitchedOnNoEndpoint }

/// Implemented by the server program over the File Service: `WorkFolder::new(<the datastore>, config)`.
pub trait WorkFolderOpener: Send + Sync {
    fn open(&self, config: ordin_files::WorkFolderConfig) -> Result<ordin_files::WorkFolder, ordin_files::FileError>;
}
/// The `files` server: the File Service's work-folder tools, wrapped here (FileService.md *Agent file tools*).
pub struct AgentFileTools { /* Arc<WorkFolder> */ }
// impl ToolProvider for AgentFileTools: provider_id() == "files"; one server "files" with the seven tools of the table above

pub struct LoadedAgent {
    pub release: LoadedRelease,
    pub config: AgentConfig,
    pub manifest: Manifest,
    pub session: HostedSessionSetup,
    pub bound: ordin_connection_binder::BoundConnections,
    pub controls: Vec<ControlLine>,
    pub report_text: String,                  // the start-up report, wrightrun's text form
    pub report: StartupReport,                // the 2C record of the same report
    pub namespace: (String, String),          // ("ordin", <work namespace>)
    /// The single workspace the hosted agent's tool calls and memory belong to (2C `ToolCall.workspace_id`).
    pub workspace_id: WorkspaceId,
    /// `LoadInput.agent_instance_id`, unchanged (SA-CLOSE-01): the id under which the rule set was
    /// installed and the memory rules are set.
    pub agent_instance_id: AgentInstanceId,
    // (private) the `files` provider built in L-16
}
impl LoadedAgent {
    /// The `files` provider (`AgentFileTools`), for the server program to pass to `McpFront::register`
    /// (SA-RECON-04). Every call returns the same provider.
    pub fn tool_provider(&self) -> std::sync::Arc<dyn ToolProvider>;
    /// The command's prompt with its `${input:arguments}` placeholder replaced by `arguments` (L-23, SA-CLOSE-02).
    pub fn expand_command(&self, slug: &str, arguments: &str) -> Result<String, CommandError>;
}
pub const PLACEHOLDER: &str = "${input:arguments}";
pub const FRONT_MATTER: &str = r"(?s)\A---\s*\n(.*?)\n---\s*\n";   // wrightrun `FRONT_MATTER` (regex crate syntax), matched at the start of the file
#[derive(Debug, thiserror::Error)]
#[error("error[{code}]: {message}{}", .location.as_ref().map(|l| format!(" ({l})")).unwrap_or_default())]
pub struct CommandError { pub code: &'static str /* CMD_* */, pub message: String, pub location: Option<String> }

pub async fn load(input: LoadInput<'_>) -> Result<LoadedAgent, LoadError>;
pub fn build_report_text(agent: &ReportInput<'_>) -> String;   // pure; `load` calls it (L-20); the Release Builder's check prints `LoadedAgent.report_text`

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("{0}")] Load(LoadFailure),           // LOAD_*
    #[error("{0}")] Release(ReleaseError),       // REL_*
    #[error("{0}")] Config(ConfigError),         // CFG_*
    #[error("{0}")] Model(ModelPolicyError),     // MPOL_*
    #[error("{0}")] Bind(BindError),             // TOOL_*
}
#[derive(Debug, thiserror::Error)]
#[error("error[{code}]: {message}{}", .location.as_ref().map(|l| format!(" ({l})")).unwrap_or_default())]
pub struct LoadFailure { pub code: &'static str, pub message: String, pub location: Option<String> }
```

**Called by.**

| Public item | Called by |
|---|---|
| `load`, `LoadInput`, `LoadMode`, `LoadedAgent`, `TraceStatusView` | the server program (OrdinServer): `Serve` at every agent-profile instance start, `Check` for `ordin-server agent check` (through the Release Builder), `RunOnce` for `ordin-server agent run-once`. A free function, not a method (reconciliation row 582) |
| `LoaderStore` | implemented by the server program over the Journal (O152) |
| `release::obtain` | the server program's `ordin-server agent release obtain` (reconciliation row 583) |
| `release::{read_auth_file, parse_reference}` | the server program's `ordin-server agent release obtain`; Release Builder (`publish`, U-1 and U-3) |
| `release::{write_release_dir, publish_dir, ReleaseRecord, CONTRACT_VERSION}` | Release Builder (C-1, U-2, U-3) |
| `release::{read_release, check_supported}`, `manifest::read_manifest`, `build_report_text` | this crate only (`obtain` L-3 and L-7, `load` L-10, L-11 and L-20, `write_release_dir` L-9a, `publish_dir` L-9b); no other component calls them |
| `HostedSessionSetup`, `HarnessToolPolicy`, `SubagentSetup`, `SingleCallForm` (in `ordin_types::domain::agent_profile`) | the server program, which maps `LoadedAgent.session` into the `SessionSetup` its `SessionSetupSource` gives the Claude Local Adapter (`instructions_file`, `plugin_dir`, `model`, `mcp_servers`, `tool_policy`, `subagents`, `single_call`, `run_limit_secs`) (O153) |
| `LoadedAgent.workspace_id` | the server program, which passes it to the MCP Front's session grants and the Memory Service (reconciliation row 588, O75) |
| `AgentFileTools` (`ToolProvider`, server `files`), `WorkFolderOpener` | MCP Front (lists and calls the file tools); the program implements `WorkFolderOpener` over `WorkFolder::new` |
| `LoadedAgent::tool_provider` | the server program, which passes it to `McpFront::register` after `load` (SA-RECON-04) |
| `LoadedAgent::expand_command`, `CommandError` | the server program's `SupervisorHost::expand_command` (agent profile), which the Session Supervisor calls before a queued `RunRequest::Command` run starts and which maps a `CommandError` to `HostError { code }` (SessionSupervisor.md; SA-CLOSE-02) |
| `LoadedAgent.release` | the server program calls `Telemetry::set_release(release_id, digest)` after `load` returns (reconciliation row 586) |

The Agent Loader builds its own `AgentFileTools` (`files`) and returns it, with the Connection Binder's provider (`agent`), for the server program to register on the MCP Front (SA-RECON-04); the File Service implements no `ToolProvider` (FileService.md *Agent file tools*), so this crate wraps the work folder (reconciliation row 584, N92).

`ReportInput` holds references to the manifest, the configuration, the bound connections, the control lines, the model policy report, the single-call form, the release (or `None`), `ordin_version`, `harness_version` and the trace status.

### Example

```rust
let loaded = load(LoadInput { mode: LoadMode::Serve, release_dir: Path::new("/var/lib/ordin/release"),
    ordin_version: "1.0.0", harness_version: "2.1.304", store: Some(&journal_store), keys: &keys,
    secrets, hosts, mcp_front: &front, rules: &rules, trace_status: TraceStatusView::On }).await?;
loaded.release.digest            // Some("sha256:4c1e…9b07")
loaded.session.mcp_servers       // ["agent", "files"]
loaded.session.single_call       // OneTurnNoTool (C-77)
```

The report text it logs begins:

```
ordin start-up report
release: 2026.10.1 sha256:4c1e0f6a2b7d49c08e5f3a1d9b6c7e2f0a4d8c3b5e9f1a2c6d7e8f9a0b1c9b07
package: support-agent 1.4.0
runtime: ordin 1.0.0, claude code 2.1.304
model: anthropic:claude-sonnet-4-6
model policy: fallback anthropic:claude-haiku-4-5, temperature not carried, gateway not used
safe reply: I could not answer that. Please ask the service desk.
who may reach: channels C04HELPDESK; users every member; direct messages U02ASHA; callers with the role claim ordin_role
approvers: users U07LEAD/00u1lead; groups service-desk-leads; the asker may approve actions that stay inside the company
connections:
  tickets (http): getTicket, createTicket; 1 not exposed
  supplier-mail (webhook, external): email_supplier; 0 not exposed
  runtime-contract (contract): event_record; 4 not exposed
approval before: email_supplier
traces: on
```

## Internal Logic

**Obtain** (`ordin-server agent release obtain`; wrightrun `AgentRelease.md` *obtain*, kept):

1. **L-1 Reference.** `parse_reference(ORDIN_RELEASE)`; a text of another form, over 512 characters, or with no digest → `REL_REFERENCE_INVALID` ("a release is obtained by its digest; write <registry>/<repository>@sha256:<hex>", location `ORDIN_RELEASE`). The registry is reached over `https`, or over `http` only for `localhost` or `127.0.0.1`.
2. **L-2 Credentials.** `read_auth_file(ORDIN_REGISTRY_AUTH_FILE)`: absent → anonymous. Present: a JSON file of the container-runtime form `{"auths": {"<registry>": {"auth": "<base64 user:password>"}}}` or `{"auths": {"<registry>": {"username": …, "password": …}}}`; unreadable, not that form, or no entry for the reference's registry → `REL_AUTH_FILE` (location `ORDIN_REGISTRY_AUTH_FILE`). The credential is never logged or printed.
3. **L-3 Destination.** When `dest/obtained.json` names the same digest and `read_release(dest)` passes, `check_supported` and return it with no request. When `dest` holds only `.partial`, `bundle`, `config` and `release.json` and no `obtained.json`, remove them and continue. Otherwise `dest` must be empty or absent → else `REL_DEST_NOT_EMPTY`.
4. **L-4 Manifest.** With `oci-client` (`CONNECT_SECONDS`, `REQUEST_SECONDS`, no redirect followed to another host *assumed here*), pull the image manifest by digest; absent → `REL_NOT_FOUND`; above `MAX_MANIFEST_BYTES` → `REL_TOO_LARGE`; SHA-256 of the bytes ≠ the digest → `REL_DIGEST_MISMATCH` (expected and found digests in the message). It must have `schemaVersion` 2, `artifactType` = `ARTIFACT_TYPE`, a `config` of `RECORD_MEDIA_TYPE` and exactly two layers of `BUNDLE_MEDIA_TYPE` then `CONFIG_MEDIA_TYPE` → else `REL_NOT_A_RELEASE`; a stated size above its limit → `REL_TOO_LARGE`.
5. **L-5 Parts.** Pull the three blobs into `dest/.partial/`, stopping at each stated size; a missing blob → `REL_NOT_FOUND`; a size or SHA-256 that differs → `REL_DIGEST_MISMATCH` naming the part (`bundle`, `config` or `record`). Nothing is unpacked before all three verify. A transport failure or timeout → `REL_REGISTRY_UNREACHABLE`; status 401 or 403 → `REL_REGISTRY_REFUSED`; another unexpected status → `REL_REGISTRY_ERROR` (status and operation, never the response text). No request is repeated.
6. **L-6 Unpack.** `flate2::read::GzDecoder` + `tar::Archive` into `dest/bundle/` and `dest/config/`. An entry with an absolute path, a `..` or empty segment, or a type other than a regular file or a folder → `REL_UNSAFE_ARCHIVE`; more than `MAX_ENTRIES` entries or `MAX_RELEASE_BYTES` of content → `REL_TOO_LARGE`. Files are written with mode `0o444`, folders `0o555`, whatever the archive states.
7. **L-7 Finish.** Write `dest/release.json` from the record blob, remove `.partial`, write `dest/obtained.json` (`{"reference": …, "digest": "sha256:<hex>"}`) last. `read_release(dest)`; a release identifier in the reference that differs from the record's → `REL_REFERENCE_INVALID`; `check_supported`. On any error in L-4 to L-7, everything written under `dest` is removed; `dest` itself stays (a volume mount point). Log `info` `release_obtained` with `release_id` and `digest`.

**`read_release(dir)`** (L-8): `release.json` must exist, be UTF-8 JSON of at most `MAX_RECORD_BYTES` with exactly the seven keys and their types → else `REL_INCOMPLETE` naming the file and the first failing key; `bundle/wrightrun-manifest.json` and `config/wrightrun.yaml` must be regular files → else `REL_INCOMPLETE` naming the part; an `obtained.json`, when present, must hold a string `reference` and a `digest` matching `^sha256:[a-f0-9]{64}$` → else `REL_INCOMPLETE`. No network call.

**`check_supported(record, ordin_version)`** (L-9): read both versions by their leading `<digits>.<digits>.<digits>`; the record's must have the same major and minor and a patch not above ordin's → else `REL_RUNTIME_RANGE` ("the release was prepared with ordin <r>; this image holds ordin <o> and supports releases prepared with <M.m>.0 to <o>"); `contract_version` outside `CONTRACT_MIN..=CONTRACT_MAX` → `REL_CONTRACT_RANGE`. The ordin range is checked first.

**`write_release_dir(out_dir, bundle_dir, config_path, named, record)`** (L-9a; wrightrun's, kept): `out_dir` absent or an empty folder → else `REL_OUTPUT_EXISTS`. `read_manifest(bundle_dir)` (`LOAD_*` pass through); the record's `release_id` matches `RELEASE_ID`, `package_name`/`package_version` equal the manifest's, `manifest_schema_version` equals the manifest's, `runtime_version` begins `<digits>.<digits>.<digits>`, `contract_version ≥ 1`, `startup_report` at most 500,000 characters (it may be empty for the first write of check and publish) → else `REL_INVALID_INPUT` naming the field. `config_path` a regular file; each `named` path a regular file under `config_path`'s folder and not at the relative path `wrightrun.yaml` → else `REL_INVALID_INPUT`. In a temporary folder beside `out_dir`: copy `bundle_dir` to `bundle/` (links not followed), the configuration to `config/wrightrun.yaml` byte for byte, each named file to `config/<relative path>`, write `release.json`; rename to `out_dir`; on any error remove the temporary folder.

**`publish_dir(dir, reference, auth)`** (L-9b; wrightrun's, kept): `read_release(dir)`; the reference must have a release identifier equal to the record's and no digest → else `REL_REFERENCE_INVALID`. Pack `bundle/` and `config/` so that equal content gives equal bytes: entries sorted by UTF-8 path, relative paths with `/`, regular files and folders only (anything else → `REL_UNSAFE_ARCHIVE`), modification time 0, owner and group 0 with empty names, mode `0o444` for files and `0o555` for folders, gzip header time 0 and no file name. Above `MAX_ENTRIES`, `MAX_RELEASE_BYTES`, `MAX_PART_BYTES` or `MAX_RECORD_BYTES` → `REL_TOO_LARGE`. Build the manifest as JSON with sorted keys and no white space (`{"artifactType":…,"config":{…},"layers":[{…bundle},{…config}],"mediaType":"application/vnd.oci.image.manifest.v1+json","schemaVersion":2}`); the release digest is the SHA-256 of those bytes. Look up the manifest under the release identifier: the same digest → return it, nothing written; another digest → `REL_EXISTS_DIFFERENT` (message names the identifier and the digest held). Otherwise push each part the registry lacks, then the manifest under the identifier; pull it back by digest and compare → else `REL_DIGEST_MISMATCH`. Registry failures map as in L-5. Return the digest.

**`load(input)`:**

10. **L-10 Release.** `release = read_release(release_dir)`; `check_supported(&release.record, ordin_version)`. In `Serve` mode a folder with no `obtained.json` is accepted (an image built with the release inside it), and `digest` is `None`.
11. **L-11 Manifest.** `read_manifest(release_dir/bundle)`: no folder or no `wrightrun-manifest.json` → `LOAD_BUNDLE_MISSING`; JSON that does not parse, has other keys or types, or `schema_version ≠ MANIFEST_SCHEMA_VERSION` → `LOAD_MANIFEST_INVALID` naming the first failing key. `bundle/AGENTS.md`, `bundle/.ordin/claude/instructions.md` and the folder `bundle/.ordin/claude/plugin/` must exist → else `LOAD_BUNDLE_MISSING` naming the missing path. When `manifest.reviewer` is set, `bundle/.apm/agents/<name>.agent.md` must exist and hold text after its front matter → else `LOAD_BUNDLE_MISSING`.
12. **L-12 Configuration.** `config = load_config(release_dir/config/wrightrun.yaml)`; `check_against_manifest(&config, &manifest)`. `ConfigError` passes through unchanged.
13. **L-13 Schema** (`Serve` only; `Check` and `RunOnce` skip it). `store.schema_status()`: `Err` → `LOAD_STORE_UNAVAILABLE` (retryable by restart); `Older { found, needed }` → `LOAD_SCHEMA_OLD` ("the datastore schema is <found>; this ordin needs <needed>; run ordin-server agent apply-schema"); `NewerIncompatible { found }` → `LOAD_SCHEMA_NEWER`; `Current` and `NewerCompatible` pass (within one minor line a change only adds, ADR-098 item 5). Then `workspace_id = store.hosted_workspace()`; `Err` → `LOAD_STORE_UNAVAILABLE`. `Check` and `RunOnce` use a new `WorkspaceId` for the process (nothing is stored).
14. **L-14 Model.** `policy = ordin_model_policy::build(&config, keys).await`; `ModelPolicyError` passes through.
15. **L-15 Connections.** `bound = ordin_connection_binder::bind(BindInput { config, manifest, config_dir: release_dir/config, secrets, hosts }).await`; `BindError` passes through. Nothing is registered here (SA-RECON-04).
16. **L-16 File tools.** `namespace = ("ordin", config.work_namespace.unwrap_or(manifest.package.name))`. `protected_paths` = for each `DutyGate` of the declaration, `<folder>STOP` and `<folder>escalation.md` (ADR-098 item 3: "A duty's STOP and escalation.md cannot be removed by the agent"). `folder = work_folder.open(WorkFolderConfig { namespace: namespace.1, bundle_dir: release_dir/bundle, writable_paths: config.writable_paths, protected_paths })`; an error → `LOAD_BUILD_FAILED` ("the file tools could not be built"). Wrap it as `AgentFileTools` — `servers()` lists the seven tools of the table with `input_schema` from each argument object, `argument_form` = `<tool>(<arguments>)`, `sandbox_callable = false` and the table's `plain`; `offer` returns all seven; `check_arguments` refuses unknown keys and wrong types (`ArgumentProblem` at the field); `call` runs exactly the table's method with its defaults — and keep it in the `LoadedAgent` for `tool_provider()` (SA-RECON-04).
17. **L-17 Knowledge tools.** When `bound.knowledge_tools` is not empty: `mcp_front.served_tools(&ServerName::new("knowledge")?)` must hold exactly those names → else `LOAD_KNOWLEDGE_MISMATCH` naming the names missing or extra. (The Knowledge Service registers its server before `load`; the program orders it.)
18. **L-18 Rules.** `rules.load_rule_set(InstanceRuleSet { agent_instance_id, package_name: manifest.package.name, authority: <manifest.authority.outcome: "routing-only" → RoutingOnly, "document-only" → DocumentOnly, "system-changing" → SystemChanging>, agent_folder: release_dir/bundle, agent_folder_read_only: true, declaration: manifest.declaration, configured: Some(ConfiguredControls { message_rules, steps, argument_rules: config.controls.*, schedule_entries: config.schedules → (duty, command), content_checks: config.controls.content_checks (ADR-107) }), tool_policy: None, approval_list: bound.approval, package_read_only_tools: vec![], tools: bound.descriptions, unmapped_operations: names of bound.operations with NotMapped, external: bound.external, commands: manifest.commands → slug, reviewer: manifest.reviewer.map(|r| r.name) })`. The approvers are not part of the rule set: the program gives `config.approvers` to Caller Authentication and the Approval Service. an error → `LOAD_BUILD_FAILED` ("the rules could not be installed (<the RulesError's code>)"). The returned `ControlLine`s go to the report.
19. **L-19 Session set-up.** `HostedSessionSetup`: `working_dir` = `release_dir/bundle`; `instructions_file`, `plugin_dir` under `.ordin/claude/` (written by the Release Builder in the Claude Code Installer's form, C-61 result (b)); `model` = `policy.harness_settings()`; `mcp_servers` = `["agent", "files"]` plus `"knowledge"` when knowledge tools are exposed; `tool_policy` = `{ builtin_file_writing: false, builtin_file_reading_bundle_only: true, shell: false, web: false }` — *conditional on C-79:* if Claude Code does not load the instructions and skills with its own file-writing tools off and reading limited to the bundle, the Claude Local Adapter reports it and the HLD's response applies (`10:215`; the adapter's spec carries it); `subagents` = `general-purpose` with `SameAsAgent`, and the reviewer (when the manifest records one) with `ReadAndSearchOnly` — the Rules Engine refuses every reviewer call but reading and searching files, *conditional on C-78* (the hook input naming the sub-agent); `single_call` = `OneTurnNoTool`, *conditional on C-77* (one harness turn with no tool offered): if C-77 finds no such turn, `NoBoundTool` (a turn offered only the File Service's read tools, the start-up report stating the form in use, ADR-098 item 3); `run_limit_secs` = `run_limit_minutes × 60`.
20. **L-20 Report.** Build `report_text` (L-22) and the `StartupReport` record: `enforced_rules` = names of enforced control lines; `rules_not_enforced` = the others with their reason; `unavailable_parts` = unconnected tool sections; `available_tools` = every offered name (Connection Binder, file tools, knowledge tools); `adapter_verbs_not_offered` = empty (`claude_local` offers every verb). Lists are cut to 500 items of 500 characters (LD-42).
21. **L-21 Record.** `Serve`: `store.record_loaded(&AgentRelease { state: Running, … }, &report)`; `Err` → `LOAD_STORE_UNAVAILABLE`. Log the report once as `info` event `startup_report` with field `text` and the fields `release_id`, `digest` (`03:1569`). `Check` and `RunOnce` log nothing; the command prints the text. Return `LoadedAgent`.

**`LoadedAgent::expand_command(slug, arguments)`** (L-23; wrightrun *CommandExpansion*, kept; SA-CLOSE-02). `bundle` = `self.session.working_dir`.
1. **Slug.** `slug` not the `slug` of an item of `self.manifest.commands` → `CMD_UNKNOWN_COMMAND`, `location` = `slug`, message "unknown command; the commands are: " followed by the known slugs sorted and separated by `, `, or "unknown command; the package has no command" when the manifest holds none. The slug is compared with the manifest before any path is built from it, so a slug such as `../../x` never reaches the file system.
2. **Read.** Read `bundle/.apm/prompts/<slug>.prompt.md` as UTF-8; a missing file, an unreadable file or bytes that are not UTF-8 → `CMD_PROMPT_UNREADABLE`, message "the prompt file of the command cannot be read", `location` = the path relative to the bundle.
3. **Body.** `FRONT_MATTER` must match at the start of the text → else `CMD_PROMPT_UNREADABLE`, message "the prompt file has no front matter", same `location`. The body is the text after the match, unchanged (no line removed, no white space trimmed).
4. **Placeholder.** The body does not hold `PLACEHOLDER` → `CMD_PLACEHOLDER_MISSING`, message "the prompt file holds no ${input:arguments}", `location` = the path relative to the bundle.
5. **Replace.** Return the body with every occurrence of `PLACEHOLDER` replaced by `arguments`, as one plain string replacement; `arguments` is not interpreted, so a `${input:arguments}` inside it is not replaced again. No length limit is set here: the caller bounds the arguments at its own boundary. Nothing is logged.

Steps 2 to 4 fail only when the bundle no longer holds what the Release Builder checked; the bundle is read-only (L-6), so in a running instance they indicate a damaged image.

**L-22 The report text** (`build_report_text`, pure). wrightrun's line order and forms (`StartupReport.md`), with these changes: first line `ordin start-up report`; the `release:` line only when a digest exists; `runtime: ordin <version>, claude code <harness version>`; `model policy: fallback <model or none>, temperature not carried, gateway used|not used` (the word `temperature not carried` only for the mapping form); after `safe reply:`, the line `who may reach:` — channels (the list, or `every enabled channel`), users (the list, or `every member`), direct messages (the list, or `none`), and `callers with the role claim <auth.role-claim>` when `auth` is set (R-91; ADR-097 item 3: "the start-up report lists, beside each connection, who may reach the agent"); the line `approvers:` (users as `<chat-user>/<subject>`, groups, and whether the asker may approve actions that stay inside the company), or `approvers: none; operators approve through the resume route` (ADR-097 item 4); each connection line gains `, external` after its kind when marked; the line `traces: on`, `traces: off`, or `traces: switched on, not sent — ORDIN_OTLP_ENDPOINT is not set` (2D); `single-call answer: one harness turn with no tool` or `single-call answer: one turn offered only the read tools`; `work folder: namespace ordin/<name>`; under `not carried across`, wrightrun's fixed lines with "the runtime" read as "ordin", plus one line per `not_carried(&config)` entry (`model.temperature`) and the line `- Gemini models and connector plug-ins are refused by the check (OQ-65, ADR-098 item 2).`; the `shell:` line reads `shell: off (ADR-098 item 3)`. A configuration text that is printed has each line break and tab written as one space. Two calls with equal inputs give equal bytes.

## Data Model

N/A for this crate's own storage — it owns no table. The loaded release lives in memory (`LoadedAgent`) and on disk under `ORDIN_RELEASE_DIR` (`release.json`, `obtained.json`, `bundle/`, `config/`, read-only). The release record row and the start-up report record are written through `LoaderStore` by the server program over the Journal: 2C places `startup_reports` with the Journal and `agent_releases` with the Release Builder, but the Release Builder runs on a publishing machine with no datastore, so this spec writes the `agent_releases` row through the same `LoaderStore` (reconciliation item in the findings). No destructive change: an obtained release is never changed in place; a new content is a new release.

## Error Table

All errors are in-process values; each stops the obtain sub-command (exit status 1) or the instance start before it reports ready (`03:1574`). The person never sees a code; the operator reads them in the log. The three `CMD_*` rows are the exception: they are raised by `LoadedAgent::expand_command` (L-23) after the instance is ready, fail one run only, and reach the operator as the run's `failed` record with the code (SessionSupervisor.md).

| Condition | Status | Code | Response Body |
|---|---|---|---|
| The folder a release is to be written into exists and is not empty | `Err(permanent)` | `REL_OUTPUT_EXISTS` | `error[REL_OUTPUT_EXISTS]: the output folder exists and is not empty (build/release)` |
| A record field breaks its rule, or a named file lies outside the configuration's folder or takes its place | `Err(permanent)` | `REL_INVALID_INPUT` | `error[REL_INVALID_INPUT]: the release identifier must match ^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$ (release_id)` |
| The release identifier exists in the registry with other content | `Err(permanent)` | `REL_EXISTS_DIFFERENT` | `error[REL_EXISTS_DIFFERENT]: the release 2026.10.1 exists with other content, as sha256:41be…77aa; choose a new release identifier (registry.example.org/agents/support-agent:2026.10.1)` |
| `ORDIN_RELEASE` has another form, no digest, or a release identifier that differs from the record | `Err(permanent)` | `REL_REFERENCE_INVALID` | `error[REL_REFERENCE_INVALID]: a release is obtained by its digest; write <registry>/<repository>@sha256:<hex> (ORDIN_RELEASE)` |
| The credentials file cannot be read, has another form, or has no entry for the registry | `Err(permanent)` | `REL_AUTH_FILE` | `error[REL_AUTH_FILE]: the registry credentials file cannot be read (ORDIN_REGISTRY_AUTH_FILE)` |
| The destination folder holds something other than this release | `Err(permanent)` | `REL_DEST_NOT_EMPTY` | `error[REL_DEST_NOT_EMPTY]: the folder holds files that are not this release (/var/lib/ordin/release)` |
| The registry holds no manifest under the digest, or lacks a part | `Err(permanent)` | `REL_NOT_FOUND` | `error[REL_NOT_FOUND]: the registry holds no release with this digest (registry.example.org/agents/support-agent@sha256:9f2c…0f24)` |
| The manifest or a part does not match its digest or size | `Err(permanent)` | `REL_DIGEST_MISMATCH` | `error[REL_DIGEST_MISMATCH]: expected sha256:9f2c…0f24 and found sha256:07aa…c1d3 (manifest)` |
| The artefact is not an agent release | `Err(permanent)` | `REL_NOT_A_RELEASE` | `error[REL_NOT_A_RELEASE]: the artefact has the type application/vnd.oci.image.config.v1+json and not application/vnd.wrightrun.release.v1 (registry.example.org/agents/support-agent@sha256:9f2c…0f24)` |
| An archive holds a link, a device, an absolute path or a `..` segment | `Err(permanent)` | `REL_UNSAFE_ARCHIVE` | `error[REL_UNSAFE_ARCHIVE]: a release holds regular files and folders only; this entry is a symbolic link (bundle/knowledge/latest.md)` |
| A part, the manifest, the entry count or the unpacked size is above its limit | `Err(permanent)` | `REL_TOO_LARGE` | `error[REL_TOO_LARGE]: the bundle part has 301989888 bytes; the limit is 268435456 (bundle)` |
| The registry cannot be reached or a request times out | `Err(retryable)` — by the deployment restarting the step | `REL_REGISTRY_UNREACHABLE` | `error[REL_REGISTRY_UNREACHABLE]: the registry did not answer within 10 seconds (registry.example.org)` |
| The registry refuses the credentials (401, 403) | `Err(permanent)` | `REL_REGISTRY_REFUSED` | `error[REL_REGISTRY_REFUSED]: the registry refused the request with status 403 (registry.example.org/agents/support-agent)` |
| The registry answers an unexpected status | `Err(retryable)` | `REL_REGISTRY_ERROR` | `error[REL_REGISTRY_ERROR]: the registry answered pull_manifest with status 500 (registry.example.org/agents/support-agent)` |
| The release folder lacks a part, or its record or `obtained.json` is malformed | `Err(permanent)` | `REL_INCOMPLETE` | `error[REL_INCOMPLETE]: the release holds no config/wrightrun.yaml (/var/lib/ordin/release)` |
| The release was prepared with an ordin version outside the supported range | `Err(permanent)` | `REL_RUNTIME_RANGE` | `error[REL_RUNTIME_RANGE]: the release was prepared with ordin 0.9.4; this image holds ordin 1.0.2 and supports releases prepared with 1.0.0 to 1.0.2 (release.json: runtime_version)` |
| The release was prepared with a contract version outside the supported range | `Err(permanent)` | `REL_CONTRACT_RANGE` | `error[REL_CONTRACT_RANGE]: the release was prepared with contract version 2; this image offers version 1 and supports 1 to 1 (release.json: contract_version)` |
| The bundle, its manifest, `AGENTS.md`, the Claude Code instructions or plugin folder, or the reviewer file is missing | `Err(permanent)` | `LOAD_BUNDLE_MISSING` | `error[LOAD_BUNDLE_MISSING]: the bundle holds no reviewer file (/var/lib/ordin/release/bundle/.apm/agents/work-reviewer.agent.md)` |
| The manifest does not parse or has another schema version | `Err(permanent)` | `LOAD_MANIFEST_INVALID` | `error[LOAD_MANIFEST_INVALID]: the manifest has schema version 2; this ordin reads 3 (bundle/wrightrun-manifest.json: schema_version)` |
| The datastore schema is older than ordin needs | `Err(permanent)` | `LOAD_SCHEMA_OLD` | `error[LOAD_SCHEMA_OLD]: the datastore schema is 1.0.3; this ordin needs 1.0.5; run ordin-server agent apply-schema` |
| The datastore schema is newer and removed or narrowed something | `Err(permanent)` | `LOAD_SCHEMA_NEWER` | `error[LOAD_SCHEMA_NEWER]: the datastore schema 1.2.0 is newer than this ordin can read` |
| The datastore cannot be read or written | `Err(retryable)` — by restart | `LOAD_STORE_UNAVAILABLE` | `error[LOAD_STORE_UNAVAILABLE]: the datastore cannot be reached` |
| The file tools or the rules cannot be built | `Err(permanent)` | `LOAD_BUILD_FAILED` | `error[LOAD_BUILD_FAILED]: the rules could not be installed (RULE_UNKNOWN_ARGUMENT)`; the bracketed code is the Rules Engine's `RulesError` code (L-18), passed through |
| The Knowledge Service does not serve the knowledge tools the configuration exposes | `Err(permanent)` | `LOAD_KNOWLEDGE_MISMATCH` | `error[LOAD_KNOWLEDGE_MISMATCH]: the knowledge server does not serve search_channel_history` |
| A run's command is not a command of the manifest (L-23.1) | `Err(permanent)` — the run ends `failed` | `CMD_UNKNOWN_COMMAND` | `error[CMD_UNKNOWN_COMMAND]: unknown command; the commands are: coordinate-rollback, cut-release, plan-release, publish-release-notes, run-go-no-go (run-go-nogo)` |
| The command's prompt file is absent, unreadable, not UTF-8 or has no front matter (L-23.2, L-23.3) | `Err(permanent)` — the run ends `failed` | `CMD_PROMPT_UNREADABLE` | `error[CMD_PROMPT_UNREADABLE]: the prompt file of the command cannot be read (.apm/prompts/run-go-no-go.prompt.md)` |
| The command's prompt body holds no `${input:arguments}` (L-23.4) | `Err(permanent)` — the run ends `failed` | `CMD_PLACEHOLDER_MISSING` | `error[CMD_PLACEHOLDER_MISSING]: the prompt file holds no ${input:arguments} (.apm/prompts/run-go-no-go.prompt.md)` |
| A configuration, model, or connection fault | `Err(permanent)` | `CFG_*`, `MPOL_*`, `TOOL_*` | as in the Agent Configuration, Model Policy and Connection Binder specs |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Agent Loader

  Scenario: Happy path — obtain and load a published release
    Given a registry container holding the fixture release published with digest D
    And ORDIN_RELEASE "<registry>/agents/support-agent@D" and an empty ORDIN_RELEASE_DIR
    When the obtain sub-command runs and then load runs in Serve mode
    Then the folder holds obtained.json with D, and every bundle file has mode 0444
    And the loaded session's working folder is the bundle folder
    And the session is given the MCP servers "agent" and "files"
    And the start-up report is logged once with the release identifier and D

  Scenario: Happy path — the program's instance id is carried through
    Given LoadInput.agent_instance_id "0192a1f0-1111-7222-8333-444455556666"
    When load runs in Serve mode
    Then the rule set is installed with that agent_instance_id
    And LoadedAgent.agent_instance_id equals "0192a1f0-1111-7222-8333-444455556666"

  Scenario: Happy path — a second start in the same pod makes no request
    Given the folder already holds obtained.json with D and a valid release
    When the obtain sub-command runs again with the same reference
    Then no request reaches the registry
    And it returns the same LoadedRelease

  Scenario: Happy path — check mode builds the report without a datastore
    Given a local release folder with no obtained.json
    When load runs in Check mode with no store
    Then it returns a report text whose first line is "ordin start-up report"
    And the text holds no "release:" line
    And no datastore call is made

  Scenario: Edge case — who may reach and the approvers are stated
    Given the Agent Configuration example
    When the report text is built
    Then it holds "who may reach: channels C04HELPDESK; users every member; direct messages U02ASHA; callers with the role claim ordin_role"
    And it holds "supplier-mail (webhook, external): email_supplier; 0 not exposed"

  Scenario: Edge case — traces switched on with no collector
    Given traces: true and trace status SwitchedOnNoEndpoint
    When the report text is built
    Then it holds "traces: switched on, not sent — ORDIN_OTLP_ENDPOINT is not set"

  Scenario: Edge case — the reviewer gets read-and-search only
    Given a manifest recording the reviewer "work-reviewer"
    When load runs
    Then the session's sub-agents are general-purpose with SameAsAgent and work-reviewer with ReadAndSearchOnly
    And the rules installed name work-reviewer as the reviewer

  Scenario: Edge case — a duty's STOP file is protected
    Given a declaration with the duty gate folder "/.agent-run/watch-queue/"
    When load runs
    Then the file tools are built with protected paths "/.agent-run/watch-queue/STOP" and "/.agent-run/watch-queue/escalation.md"
    And a call of the files server's delete_file with {"path": "/.agent-run/watch-queue/STOP"} returns ToolFailure with code FILE_AGENT_REFUSED and the work folder's refusal text

  Scenario: Error path — the content does not match the digest
    Given a registry stub that answers the manifest of D with one byte changed
    When the obtain sub-command runs
    Then it fails with REL_DIGEST_MISMATCH naming D and the digest found
    And the destination folder is empty

  Scenario: Error path — an unsafe archive entry
    Given a bundle part holding a symbolic link
    When the obtain sub-command runs
    Then it fails with REL_UNSAFE_ARCHIVE and nothing is left under the destination

  Scenario: Error path — a release prepared by an older minor version
    Given a release whose record states runtime_version "0.9.4" and ordin 1.0.2
    When load runs
    Then it fails with REL_RUNTIME_RANGE before any configuration is read

  Scenario: Error path — the schema is older than ordin needs
    Given a store whose schema_status is Older { found: "1.0.3", needed: "1.0.5" }
    When load runs in Serve mode
    Then it fails with LOAD_SCHEMA_OLD and the instance never reports ready

  Scenario: Every instance of one datastore gets the same workspace
    Given two instances loading the same release against one store whose hosted_workspace is W
    When both run load in Serve mode
    Then both LoadedAgent values hold workspace_id W

  Scenario: Error path — a missing knowledge tool
    Given a configuration exposing search on the source channel-history and a knowledge server that serves no tool
    When load runs
    Then it fails with LOAD_KNOWLEDGE_MISMATCH naming search_channel_history

  Scenario: Error path — the registry refuses the credentials
    Given a registry stub answering 403
    When the obtain sub-command runs
    Then it fails with REL_REGISTRY_REFUSED and the credentials appear in no output

  Scenario: Happy path — a command is expanded with its arguments
    Given a loaded agent whose manifest lists the command "run-go-no-go" and whose prompt file holds front matter and a body with ${input:arguments}
    When expand_command is called with "run-go-no-go" and "RC-12"
    Then it returns the body after the front matter with ${input:arguments} replaced by "RC-12"

  Scenario: Error path — an unknown command
    Given a loaded agent whose manifest lists "coordinate-rollback", "cut-release", "plan-release", "publish-release-notes" and "run-go-no-go"
    When expand_command is called with "run-go-nogo"
    Then it fails with CMD_UNKNOWN_COMMAND at location "run-go-nogo" listing the five commands in sorted order
    And no file under the bundle is read

  Scenario: Error path — a prompt file without front matter
    Given a loaded agent whose prompt file for "run-go-no-go" starts with no "---" line
    When expand_command is called with "run-go-no-go" and "RC-12"
    Then it fails with CMD_PROMPT_UNREADABLE at location ".apm/prompts/run-go-no-go.prompt.md"

  Scenario: Error path — a prompt body without the placeholder
    Given a loaded agent whose prompt body for "run-go-no-go" holds no ${input:arguments}
    When expand_command is called with "run-go-no-go" and "RC-12"
    Then it fails with CMD_PLACEHOLDER_MISSING at location ".apm/prompts/run-go-no-go.prompt.md"
```

## Performance / Security / Observability

- **Performance targets:** `read_release` and `read_manifest` under 20 ms p95; `load` without MCP servers under 2 s p95 for a 100 MB bundle (no bundle file is read but the manifest, the instructions file and the reviewer file); obtain bounded by the registry, with each request limited to `REQUEST_SECONDS` and unpacking of a 256 MiB part under 30 s p95 *(assumed here)*; memory during unpack under 64 MiB (streaming).
- **Security:** No inbound entry point. A release is loaded only when its digest matches what the deployment names (07 security row for hosted agents); every part is verified before anything is unpacked; archive entries are limited to regular files and folders inside the destination, written read-only; the bundle is the read-only working folder; the harness's own file-writing tools and shell are off and reading is limited to the bundle (C-79); the agent reaches the work folder only through the File Service's tools; a duty's `STOP` and `escalation.md` cannot be removed by the agent. The registry credentials, the model key and connection credentials are never logged or printed. The organisation key travels only inside `HarnessModelSettings` to the Claude Local Adapter (C-74).
- **Observability:** Log events (`tracing`, JSON): `release_obtained` (`info`; `release_id`, `digest`), `startup_report` (`info`; `release_id`, `digest`, `text`), logged once per instance start (`03:1569`); `release_load_failed` (`error`; `code`, `location`) before the process exits. Metrics: the program calls `Telemetry::set_release(release_id, digest)` after `load` returns, before the instance reports ready (`wrightrun_release_info`). No span.

## Gaps

None.
