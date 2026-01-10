# SPEC: Package Inspector

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-package-inspector` · **Build phase:** 4 · **Programs:** D P A
> **Dependencies:** Rules Engine, Format Plug-ins, Agent Model

## Table of contents

- [Purpose](#purpose)
- [Approach](#approach)
- [Shared Context](#shared-context)
- [Public Interface](#public-interface)
- [Internal Logic](#internal-logic)
- [Data Model](#data-model)
- [Error Table](#error-table)
- [Acceptance Criteria (Gherkin)](#acceptance-criteria-gherkin)
- [Performance, Security, Observability](#performance-security-observability)
- [Gaps](#gaps)

## Purpose

The Package Inspector reads an installed agent without executing it — its agent folder and its lock file (ADR-085, ADR-093 item 6) — and derives the agent's **parsed package model**: its task list (its commands), its declared tools and connections, its authority (document-only or system-changing, derived from what its tools and connections may do — SA-CLOSE-01) and the parts the application cannot carry across (02:472–480). It answers **What it's made of** for an installed agent (its parts grouped by kind in plain words, ADR-093 item 1, 11 §5.29) and **Show the installed files** (the agent folder as the Installers wrote it, the lock file first, ADR-093 item 3). When the folder holds `wrightrun-declaration.yaml`, it hands the declaration to the Rules Engine, which loads it (02:480). It also derives what an agent **may do** (its tools, reads, connections and policy) and compares two such summaries in plain words, so an update that changes what an agent may do is put to the person (A-16); and it serves the Ordin Host's **agent directory** for a workspace, with a change notification. APM-era sources (`apm.yml` and the files it deploys) are read only through the APM format plug-in (ADR-087, ADR-093 item 6). Text it reads is data; it obeys none of it.

## Approach

A read-only module over the files the Installers and the Resolver and Compiler wrote: the lock file in the agent folder and the bundle directory named by the lock file's plan hash, both parsed with the Agent Model, plus the APM plug-in for folders installed through `apm` before revision 0.25.0 (R-73). The model is computed once per install or update and kept as one JSON file per instance, whose digest is the `parsed_declaration_ref` the Package Catalogue stores on the agent instance (2C §AgentInstance). Rejected: (1) a table — the crate has no Journal dependency (2B-3), so it cannot register migrations; (2) parsing `wrightrun-declaration.yaml` here — 02:480 gives loading the declaration to the Rules Engine, so one parser exists.

### Reconciliation assumptions

- **SA-RECON-01** This component parses `wrightrun-declaration.yaml` into the Rules Engine's `Declaration`, builds the instance's `InstanceRuleSet` (including the tool policy mapped from the install's resolved tool policy) and calls `load_rule_set` / `unload_rule_set`; `load_declaration`, `DeclarationSummary` and `RuleError` are withdrawn (reconciliation N45, N178, O46; ruling R5).
- **SA-RECON-02** The Inspector holds no instance status: a refused rule set is returned as `PKI_DECLARATION_REFUSED` and the Package Catalogue marks the instance unavailable (reconciliation O143; ruling R6).
- **SA-RECON-03** Reference-only parts, plugin name, command names and `needs_person_present` are read from the install record handed in as `InstallFacts`, never re-derived (reconciliation row 108; ruling R6).
- **SA-RECON-04** The Inspector's made-of shape is `InstalledMadeOf`, so the schema registry holds one `MadeOf` (the Catalogue Service's) (reconciliation row 581).
- **SA-RECON-05** The `ordin-package-inspector → ordin-agent-model` edge is in 2B-3 (amended January 2026, decision 80).

### Closing-round assumptions

- **SA-CLOSE-01** (*chosen here*; closes the declaration-authority gap): the authority is always derived (step 6); the Rules Engine's `Declaration` carries no `authority`, so `AuthoritySource::Declaration` is withdrawn and `routing_only` is never produced by this component. An instance is `routing_only` only where another component sets it (none on the desktop today).
- **SA-CLOSE-02** (*chosen here*; closes the argument-places gap): each tool's argument places come from its ToolBinding binding's `argumentPaths` (Agent Model `ToolBindingSpec.argument_paths`, JSON Pointers, RFC 6901), converted to the Rules Engine's place form by step 6b.
- **SA-CLOSE-03** (*chosen here*; closes the approval-list gap): `InstanceRuleSet.approval_list` = the declaration's `approval-required` list of tool names (Rules Engine `Declaration.approval_required`); empty when the folder has no declaration.
- **SA-CLOSE-04** (*chosen here*; closes the command-arguments gap): each task carries the command file's `argument-hint` front-matter value (Agent Model `Manifest::argument_hint()`), so the agent directory tells the Ordin Host what each command expects.
- **SA-CLOSE-05** (*chosen here*; serves Ordin Host's `AgentMemoryRules`): the model keeps the agent's Memory part's contract (Agent Model `memory_contract()`), and `memory_contract(id)` returns it; the Ordin Host maps it to the Memory Service's `AgentMemoryRules` (`retains`, `excludes`, `max_age_days`, `store_location`).

## Shared Context

### Types from Phase 2C (`ordin-types`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)] pub struct AgentInstanceId(pub Uuid);   // UUIDv7
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)] pub struct WorkspaceId(pub Uuid);
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>);       // RFC 3339, ms, Z
pub struct Sha256Hex(pub String);                              // 64 lowercase hex

/// 2C §AgentInstance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Authority { DocumentOnly, SystemChanging, RoutingOnly }

pub enum PartKind { /* the 25 kinds, crucible spelling */ }
pub enum PartStatus { Draft, Approved, Deprecated }
```

From 2C §AgentInstance: `parsed_declaration_ref: String` — "Key of the Package Inspector's parsed package model for this instance (02:476)"; `authority: Authority`; `lock_file_path: String`; `agent_folder_path: String`.

### Types from the Agent Model (`ordin-agent-model`)

```rust
pub struct PartRef { pub kind: PartKind, pub id: String, pub name: String, pub version: String,
    pub content_hash: String, pub repository: String, pub commit: String, pub loading: Option<LoadingMode> }
pub struct LockFile { pub lockfile_schema: String, pub plan_hash: String, pub target_harness: String,
    pub composition: PartRef, pub artefacts: Vec<PartRef>, pub overlays_applied: Vec<PartRef>,
    pub selections: Selections }        // LockFile::from_yaml(bytes)
pub struct BundleManifest { /* …, pub resolved_tool_policy: ResolvedToolPolicy, … */ }
pub struct ResolvedToolPolicy { pub forbidden_flags: Vec<String>, pub coverage_floor: Option<DecimalString>,
    pub prohibited_verbs: Vec<String>, pub requires_human_approval: Vec<String>, pub mcp_servers: Vec<McpServerDecl> }
pub struct McpServerDecl { pub name: String, pub server_type: String, pub scope: Option<String>,
    pub env_ref: Option<String>, pub transport: Option<String>, pub declared_by: String }
pub struct ToolBindingSpec { pub name: String, pub binding_type: String, pub permission: String, /* …, */
    pub argument_paths: Vec<String> }                         // JSON Pointers (`argumentPaths`)
pub struct MemoryContract { pub retains: Vec<String>, pub excludes: Vec<String>, pub max_age_days: Option<u32>,
    pub unbounded: bool, pub store_location: String, pub memory_service: Option<String> }
impl Manifest {
    pub fn tool_bindings(&self) -> Result<Vec<ToolBindingSpec>, AmError>;
    pub fn argument_hint(&self) -> Result<Option<String>, AmError>;      // Command parts: front matter `argument-hint`
    pub fn memory_contract(&self) -> Result<Option<MemoryContract>, AmError>;
}
pub fn parse_manifest(path: &str, bytes: &[u8]) -> Result<Option<Manifest>, AmError>;
pub fn markdown_body(path: &str, bytes: &[u8]) -> Option<String>;
pub fn kind_info(kind: PartKind) -> &'static KindInfo;    // plain_message_id, offered
pub const LOCK_FILE_NAME: &str = "ordin.lock";
pub const BUNDLE_MANIFEST_NAME: &str = "bundle.manifest.json";
```

### Types from the Format Plug-ins (`ordin-format-plugins`)

```rust
pub enum FormatId { Ordin, Apm, ClaudeCode, Copilot }
pub struct SourceTree(pub BTreeMap<String, Vec<u8>>);
pub struct ReadRequest { pub publisher: String, pub vocabulary: VocabularySet, pub source_label: String }
pub struct ReadOutcome { pub parts: Vec<ModelPart>, pub mapping_report: MappingReport, pub package: Option<PackageIdentity> }
pub struct MappingReport { pub format: Option<FormatId>, pub items: Vec<MappingItem> }
pub struct MappingItem { pub source_path: String, pub construct: String, pub reason: UnmappedReason, pub part_identifier: Option<String> }
pub struct PackageIdentity { pub name: String, pub version: String }
impl FormatPlugins { pub fn builtin() -> Self; pub fn get(&self, id: FormatId) -> &dyn FormatPlugin; }
```

### Rules Engine (`ordin-rules`) calls this component makes

Copied from the Rules Engine spec (ruling R5: its names win). The engine's `Declaration` is parsed by this component; the engine never reads the agent folder.

```rust
/// The parts of `wrightrun-declaration.yaml` the engine reads (kebab-case YAML keys).
pub struct Declaration { pub format_version: u32, pub gates: Gates, pub message_rules: Vec<MessageRule>,
    /// YAML `approval-required`: tool names that always ask a person (SA-CLOSE-03).
    pub approval_required: Vec<String> }
pub struct AgentToolPolicy { pub prohibited_verbs: Vec<String>, pub forbidden_flags: Vec<String>,
    pub requires_approval: Vec<String>, pub mcp_servers: Vec<String> }
pub struct ToolDescription { pub name: String, pub arguments: Vec<String>, pub served_by: Option<String> }
pub struct InstanceRuleSet {
    pub agent_instance_id: AgentInstanceId, pub package_name: String, pub authority: Authority,
    pub agent_folder: std::path::PathBuf, pub agent_folder_read_only: bool,
    pub declaration: Option<Declaration>, pub configured: Option<ConfiguredControls>,
    pub tool_policy: Option<AgentToolPolicy>, pub approval_list: Vec<String>,
    pub package_read_only_tools: Vec<String>, pub tools: Vec<ToolDescription>,
    pub unmapped_operations: Vec<String>, pub external: Vec<String>,
    pub commands: Vec<String>, pub reviewer: Option<String>,
}
pub struct ControlLine { pub name: String, pub source: ControlSource, pub point: Point,
    pub command: Option<String>, pub enforced: bool, pub reason: String, pub detail: String }
pub enum ControlSource { Package, Configuration, Application, AgentPolicy }
impl RulesEngine {
    /// Replaces the instance's rule set; a load error leaves the previous set in place.
    pub fn load_rule_set(&self, set: InstanceRuleSet) -> Result<Vec<ControlLine>, RulesError>;
    pub fn unload_rule_set(&self, agent_instance_id: AgentInstanceId);
}
pub enum RulesError { /* Load { code, message, where } and others */ }
impl RulesError { pub fn code(&self) -> &'static str; pub fn retryable(&self) -> bool; }
```

### UX facts (words shown are the screens'; this component supplies data and message ids)

- **What it's made of** order of kinds (11:973): Role, Rules, Working modes, Abilities (a group of abilities is listed under Abilities), Tasks, Reference material, Tools, Memory rules, Way of working, Working practices, Language / writing style / hand-over (with Code workflow in a workspace that links code), Rehearsal checks, What a result must hold. A kind with no part is left out.
- A part from another agent repository adds "From: [display name]"; from the bundled catalogue "From: ordin"; withdrawn or not-for-new-agents parts carry their state label (11:973).
- Never shown to a business user: files, identifiers, versions, digests, commits, the lock file, kind names or folders (11 §5.29). In a workspace that links code: kind, identifier, version and digest per part, "Pinned by its lock file: [repository] at [commit]", and **Show the installed files**, the lock file first, then the files by folder.
- A-45: **What it's made of** for at most 200 parts within 300 ms; **Read it** renders a part of at most 100 KB within 200 ms.

### Configuration

| Name | Value | Source |
|---|---|---|
| Model store | `<application data root>/inspector/` (*chosen here*) | constructor argument from the program |
| Declaration file | `wrightrun-declaration.yaml` at the agent folder root | 02:478, 08:41 |
| Declaration size limit | 1 MiB (1,048,576 bytes; wrightrun `PrepareStage.md` *Name and place* fixes it, and the Release Builder's `MAX_DECLARATION_BYTES` and `cross-cutting/InputValidation.md` state the same single limit) | constant |

## Public Interface

Crate `ordin_package_inspector`. Names are *chosen here* unless traced.

```rust
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct InspectRequest {
    pub agent_instance_id: AgentInstanceId,
    /// 2C `AgentInstance.agent_folder_path`.
    pub folder_path: PathBuf,
    /// The bundle the lock file's plan hash names (`<data root>/bundles/<hex>/`), given by the
    /// Package Catalogue; None for an APM-era folder (R-73).
    pub bundle_dir: Option<PathBuf>,
    /// Display names of the repositories the parts come from, keyed by `PartRef.repository`
    /// (`bundled` → "ordin"), supplied by the Package Catalogue from the Catalogue Service.
    pub repository_names: std::collections::BTreeMap<String, String>,
    /// Withdrawn and deprecated states of the parts, keyed by `PartRef.id`, from the Catalogue Service.
    pub part_states: std::collections::BTreeMap<String, PartState>,
    /// Facts from the Installers' install record (`Installers::record(id)`), copied by the Package
    /// Catalogue; None for an APM-era folder. The Inspector does not depend on the Installers.
    pub install: Option<InstallFacts>,
}

/// The install-record facts the model carries (Installers `InstallRecord`).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InstallFacts {
    /// `HarnessLaunch::ClaudeCode.plugin_name`; None for Copilot.
    pub plugin_name: Option<String>,
    /// `InstallRecord.commands`: command part identifier → the name the session accepts (R-94).
    pub command_names: std::collections::BTreeMap<String, String>,
    /// `InstallRecord.reference_only_parts`.
    pub reference_only_parts: Vec<String>,
    /// `InstallRecord.needs_person_present`.
    pub needs_person_present: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartState { InUse, NotForNewAgents, Withdrawn }

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSource {
    OrdinBundle { plan_hash: String, repository: String, commit: String },
    ApmFolder { package_name: String, package_version: String },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TaskEntry { pub command_id: String, pub name: String, pub description: String,
    /// The name the harness session accepts (`<plugin>:<command>` for Claude Code), from
    /// `InstallFacts.command_names`; None for an APM-era folder.
    pub session_name: Option<String>,
    /// What the command expects as arguments, from its file's front matter `argument-hint`
    /// (at most 200 characters, control characters removed); None when absent (SA-CLOSE-04).
    #[serde(default)] pub argument_hint: Option<String> }

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind { BuiltIn, McpServer, Declared }

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolEntry {
    pub name: String,
    pub kind: ToolKind,
    /// `read-only`, `requires-approval`, `prohibited`, or the binding's own word.
    pub permission: String,
    /// Identifier of the ToolBinding part, or "declaration".
    pub declared_by: String,
    /// Argument places a rule may test, in the Rules Engine's place form (step 6b, SA-CLOSE-02).
    #[serde(default)] pub arguments: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ConnectionEntry {
    pub name: String,
    pub server_type: String,
    pub transport: Option<String>,
    /// True when the server needs a credential (`env_ref` set); the value is never read (SA-CRED-01).
    pub needs_credential: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthoritySource { Derived }   // SA-CLOSE-01: the declaration states no authority

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NotCarried {
    /// Part identifier, or the APM path of a construct that did not map.
    pub what: String,
    pub reason: NotCarriedReason,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotCarriedReason { KindNotOffered, ReferenceOnly, NoEquivalentKind, Executable, UnknownField, Unreadable }

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PartLine {
    pub part: PartRef,
    /// `metadata.name`, else the slug.
    pub name: String,
    /// `metadata.description`, first sentence, at most 300 characters.
    pub what_it_does: String,
    pub state: PartState,
    /// "From: …" display name; None when the part is from the agent's own repository.
    pub from: Option<String>,
}

/// The parsed package model per instance (02:479 *Owns*). Schema "ordin.package-model/v1".
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PackageModel {
    pub model_schema: String,
    pub agent_instance_id: AgentInstanceId,
    pub source: ModelSource,
    pub display_name: String,
    pub description: String,
    pub tasks: Vec<TaskEntry>,
    pub tools: Vec<ToolEntry>,
    pub connections: Vec<ConnectionEntry>,
    pub authority: Authority,
    pub authority_source: AuthoritySource,
    pub not_carried: Vec<NotCarried>,
    /// SHA-256 hex of `wrightrun-declaration.yaml` when present.
    pub declaration_sha256: Option<String>,
    /// `reason` of each `Package` control line `load_rule_set` returned for the instance.
    pub declared_rules: Vec<String>,
    /// SHA-256 hex of the canonical JSON of `bundle.manifest.resolved_tool_policy`; None for APM-era.
    pub tool_policy_sha256: Option<String>,
    /// From `InstallFacts`; None for APM-era.
    pub plugin_name: Option<String>,
    /// From `InstallFacts`; false for APM-era.
    pub needs_person_present: bool,
    /// The Memory part's contract, when the agent has a Memory part (SA-CLOSE-05).
    #[serde(default)] pub memory: Option<MemoryContract>,
    pub parts: Vec<PartLine>,
    pub inspected_at: Timestamp,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct InspectOutcome {
    pub model: PackageModel,
    /// `pki:<agent instance id>:<sha256 hex of the stored model file>` → 2C `parsed_declaration_ref`.
    pub parsed_declaration_ref: String,
    /// `may_do(&model)`, for the Package Catalogue to store on the instance.
    pub may_do: MayDo,
}

/// What an agent may do, compared between the installed agent and an update (A-16).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MayDo {
    /// Tool names whose permission is not `read-only`, sorted.
    pub tools: Vec<String>,
    /// Tool names whose permission is `read-only`, sorted.
    pub reads: Vec<String>,
    /// Connection (MCP server) names, sorted.
    pub connections: Vec<String>,
    /// `PackageModel.tool_policy_sha256`.
    pub tool_policy_digest: Option<String>,
    /// `PackageModel.declaration_sha256`.
    pub declaration_digest: Option<String>,
    pub authority: Authority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MayDoChangeKind { ToolAdded, ToolRemoved, ReadAdded, ReadRemoved, ConnectionAdded,
    ConnectionRemoved, PolicyChanged, DeclarationChanged, AuthorityChanged }

/// One difference in plain words: the screen renders `message_id` with `name` as its argument.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct MayDoChange {
    pub kind: MayDoChangeKind,
    /// The tool or connection name, or the new authority in snake_case; empty for the digests.
    pub name: String,
    /// Fluent message id, `may-do-<kind in kebab-case>`, e.g. "may-do-connection-added".
    pub message_id: &'static str,
}

/// One installed agent in a workspace's agent directory (Ordin Host X-5).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentDirectoryEntry {
    pub agent_instance_id: AgentInstanceId,
    pub display_name: String,
    pub description: String,
    pub plugin_name: Option<String>,
    /// Each with `session_name`, the name the session accepts.
    pub tasks: Vec<TaskEntry>,
    /// Names of the Skill and Capability parts, sorted.
    pub skills: Vec<String>,
    /// Name of the Journey part (a lifecycle), when the agent has one.
    pub lifecycle: Option<String>,
    pub authority: Authority,
    pub declared_rules: Vec<String>,
    pub needs_person_present: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Detail { BusinessUser, LinksCode }

/// Service API `agents.made_of` input.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MadeOfInput { pub agent_instance_id: AgentInstanceId, pub detail: Detail }

/// Service API `agents.installed_files` input.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct InstalledFilesInput { pub agent_instance_id: AgentInstanceId }

/// Service API `agents.part_page` input. `part_id` is a `PartLine` identifier.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PartPageInput { pub agent_instance_id: AgentInstanceId, pub part_id: String, pub detail: Detail }

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct KindGroup {
    /// Fluent message id of the kind in plain words (Agent Model kind table), e.g. "part-kind-ability".
    pub plain_kind_message_id: &'static str,
    pub count: u32,
    pub parts: Vec<PartLine>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct InstalledMadeOf {
    pub display_name: String,
    pub what_it_does: String,
    /// Command names, for "Its tasks: …".
    pub tasks: Vec<String>,
    pub part_count: u32,
    pub groups: Vec<KindGroup>,
    /// LinksCode only: (repository, commit) for "Pinned by its lock file: [repository] at [commit]".
    pub pinned_by: Option<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct InstalledFileEntry { pub path: String, pub size: u64, pub sha256: String }

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PartPage {
    pub line: PartLine,
    pub plain_kind_message_id: &'static str,
    /// The part's Markdown body, or its YAML manifest text for YAML-only kinds; data, never obeyed.
    pub text: String,
    /// LinksCode only: the part's primary file path inside the bundle, for **Show the file**.
    pub file: Option<String>,
}

pub struct PackageInspector { /* private: Arc<RulesEngine>, FormatPlugins, store dir, change sender */ }

impl PackageInspector {
    pub fn new(rules: std::sync::Arc<RulesEngine>, store_dir: PathBuf) -> Result<Self, InspectError>;
    pub async fn inspect(&self, request: &InspectRequest) -> Result<InspectOutcome, InspectError>;
    /// Inspects an update candidate in its staging folder: steps 1–6 without loading the rule set
    /// and without storing anything; the installed model is untouched.
    pub async fn inspect_candidate(&self, request: &InspectRequest) -> Result<PackageModel, InspectError>;
    pub async fn model(&self, id: AgentInstanceId) -> Result<PackageModel, InspectError>;
    pub async fn made_of(&self, id: AgentInstanceId, detail: Detail) -> Result<InstalledMadeOf, InspectError>;
    /// The agent folder's files, `ordin.lock` first, then by path; LinksCode only (ADR-093 item 3).
    pub async fn installed_files(&self, id: AgentInstanceId, folder_path: &std::path::Path)
        -> Result<Vec<InstalledFileEntry>, InspectError>;
    pub async fn part_page(&self, id: AgentInstanceId, part_id: &str, bundle_dir: &std::path::Path,
        detail: Detail) -> Result<PartPage, InspectError>;
    /// Deletes the stored model and rule set and unloads the rule set (instance removed).
    pub async fn forget(&self, id: AgentInstanceId) -> Result<(), InspectError>;
    /// Loads every stored rule set into the Rules Engine (program start); returns the failures.
    pub async fn reload_rule_sets(&self) -> Result<Vec<RuleSetReloadFailure>, InspectError>;
    /// Records the workspaces an instance belongs to, for `directory`; empty removes it from all.
    pub async fn set_workspaces(&self, id: AgentInstanceId, workspace_ids: Vec<WorkspaceId>)
        -> Result<(), InspectError>;
    /// The installed agents of one workspace, ordered by `display_name` then instance id.
    pub async fn directory(&self, workspace_id: WorkspaceId) -> Result<Vec<AgentDirectoryEntry>, InspectError>;
    /// One item per workspace whose directory changed (step 16).
    pub fn changes(&self) -> futures::stream::BoxStream<'static, WorkspaceId>;
    /// The installed agent's Memory part contract; None when it has none (step 17, SA-CLOSE-05).
    pub async fn memory_contract(&self, id: AgentInstanceId) -> Result<Option<MemoryContract>, InspectError>;
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RuleSetReloadFailure { pub agent_instance_id: AgentInstanceId, pub code: String }

/// Pure: the may-do summary of a model.
pub fn may_do(model: &PackageModel) -> MayDo;
/// Pure: the differences from `installed` to `candidate`, in `MayDoChangeKind` order, then by name.
/// Empty when the two are equal.
pub fn compare_may_do(installed: &MayDo, candidate: &MayDo) -> Vec<MayDoChange>;

#[derive(Debug, Clone, thiserror::Error)]
pub enum InspectError {
    #[error("PKI_FOLDER_UNREADABLE")] FolderUnreadable { retryable: bool },
    #[error("PKI_LOCK_FILE_INVALID")] LockFileInvalid,
    #[error("PKI_BUNDLE_UNREADABLE")] BundleUnreadable,
    #[error("PKI_FORMAT_UNRECOGNISED")] FormatUnrecognised,
    #[error("PKI_DECLARATION_TOO_LARGE")] DeclarationTooLarge { bytes: u64 },
    #[error("PKI_DECLARATION_INVALID")] DeclarationInvalid,
    #[error("PKI_DECLARATION_REFUSED")] DeclarationRefused { code: &'static str, retryable: bool },
    #[error("PKI_NOT_INSPECTED")] NotInspected,
    #[error("PKI_PART_NOT_FOUND")] PartNotFound { part_id: String },
    #[error("PKI_STORE_FAILED")] StoreFailed { retryable: bool },
}
impl InspectError { pub fn retryable(&self) -> bool; }
```

### Who calls each public item

| Item | Called by |
|---|---|
| `new` | Ordin Desktop and Ordin Server at start |
| `inspect` | Package Catalogue after `install` and after each `update` / `update_in_session`, with an `InspectRequest` whose `install` comes from `Installers::record(id)`; on `PKI_DECLARATION_REFUSED` the Package Catalogue sets the instance's status (the Inspector holds no instance status) |
| `inspect_candidate` | Package Catalogue, update check (A-16), on the candidate in its staging folder |
| `may_do`, `compare_may_do` | Package Catalogue, update check: `compare_may_do(&installed.may_do, &may_do(&candidate))` empty → silent update; otherwise the changes are shown to the person |
| `model` | Package Catalogue (re-reads; on `PKI_STORE_FAILED { retryable: false }` it re-runs `inspect`) |
| `made_of`, `installed_files`, `part_page` | Service API commands `agents.made_of`, `agents.installed_files` and `agents.part_page` (table below) for **What it's made of**, **Show the installed files** and **Read it** (ADR-093) |
| `forget` | Package Catalogue *Removal*, after `Installers::uninstall` |
| `reload_rule_sets` | Ordin Desktop and Ordin Server once at start, after the Rules Engine is built and before any session starts (the Rules Engine's `load_rule_set` "at start") |
| `set_workspaces` | Package Catalogue, whenever an instance's workspace memberships change (add, add to another workspace, remove from one) |
| `directory`, `changes` | Ordin Host (X-5): the agent directory of the seed, rebuilt on each change |
| `memory_contract` | Ordin Host, before a crew run starts, to build `AgentMemoryRules` for `MemoryService::set_agent_rules` (X-23) |

### Service API commands (owner: Package Inspector)

The Service API carries three commands for ADR-093's "what it's made of" views. The Package Inspector owns them; ServiceApi.md lists them with these names and types.

| Command | Input | Result | Calls | Errors |
|---|---|---|---|---|
| `agents.made_of` | `MadeOfInput` | `InstalledMadeOf` | `made_of(agent_instance_id, detail)` | `PKI_NOT_INSPECTED`, `PKI_STORE_FAILED` |
| `agents.installed_files` | `InstalledFilesInput` | `Vec<InstalledFileEntry>` | `installed_files(agent_instance_id, folder_path)` | `PKI_FOLDER_UNREADABLE` |
| `agents.part_page` | `PartPageInput` | `PartPage` | `part_page(agent_instance_id, part_id, bundle_dir, detail)` | `PKI_NOT_INSPECTED`, `PKI_PART_NOT_FOUND`, `PKI_BUNDLE_UNREADABLE`, `PKI_STORE_FAILED` |

The person's screen never sends a path: the command handler fills `folder_path` from the instance's 2C `AgentInstance.agent_folder_path`, and `bundle_dir` from the same instance record the Package Catalogue uses for `inspect`. `agents.installed_files` is offered only in the `LinksCode` view (ADR-093 item 3).

### Example

`inspect(InspectRequest { agent_instance_id: 0192a0f9-…, folder_path: ".../agents/0192a0f9-…/folder", bundle_dir: Some(".../bundles/4d1c…0c1d"), repository_names: {"bundled": "ordin", "github.com/acme/company-agents": "Company agents"}, part_states: {}, install: Some(InstallFacts { plugin_name: Some("cspa-431"), command_names: {"urn:ordin:default:commands:lifecycle:apply": "cspa-431:apply", …}, reference_only_parts: [], needs_person_present: true }) })` returns (with `may_do` beside `model`):

```json
{
  "model": {
    "model_schema": "ordin.package-model/v1",
    "agent_instance_id": "0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff",
    "source": {"ordin_bundle": {"plan_hash": "sha256:4d1c…0c1d", "repository": "bundled", "commit": "0123…4567"}},
    "display_name": "Software Engineer (CSPA-431)",
    "description": "The software-engineering workspace that runs the capture, structure, plan, and apply loop.",
    "tasks": [{"command_id": "urn:ordin:default:commands:lifecycle:apply", "name": "Apply", "description": "Apply one phase of a plan.", "session_name": "cspa-431:apply"}],
    "tools": [],
    "connections": [],
    "authority": "document_only",
    "authority_source": "derived",
    "not_carried": [],
    "declaration_sha256": null,
    "declared_rules": [],
    "tool_policy_sha256": "9b07…e1a2",
    "plugin_name": "cspa-431",
    "needs_person_present": true,
    "parts": [ … one line per part of the lock file … ],
    "inspected_at": "2026-01-10T09:12:44.120Z"
  },
  "parsed_declaration_ref": "pki:0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff:7c4e…19ab"
}
```

## Internal Logic

`inspect(request)`:

1. **Folder.** Read the agent folder's top level; unreadable → `PKI_FOLDER_UNREADABLE` (retryable on a transient I/O error). Paths are joined only from file names the directory listing returned; a name containing `/`, `\` or NUL is skipped and logged.
2. **Which source.** When `ordin.lock` exists at the root: parse it with `LockFile::from_yaml` (error → `PKI_LOCK_FILE_INVALID`) → ordin path (step 3). Else when `apm.yml` exists: APM-era path (step 4). Else → `PKI_FORMAT_UNRECOGNISED`.
3. **Ordin path.** `bundle_dir` must be given and hold `bundle.manifest.json` whose `plan_hash` equals the lock file's (else `PKI_BUNDLE_UNREADABLE`). For each `PartRef` in `lock.artefacts`: read the primary manifest file in `<bundle_dir>/artefacts/<hex>/` (the one file that `parse_manifest` accepts; none or two → the part goes to `not_carried { reason: Unreadable }`), and build a `PartLine`: `name` = `metadata.name` or the slug; `what_it_does` = first sentence of `metadata.description` (text up to the first `. `, `.\n` or end), cut at 300 characters; `state` from `part_states` (default `InUse`; a manifest status `deprecated` gives `NotForNewAgents`); `from` = `repository_names[part.repository]` when the part's repository differs from the composition's, else None. Then:
   - **tasks**: every Command part, in lock order → `TaskEntry { command_id, name, description: what_it_does, argument_hint }` with `argument_hint` = `manifest.argument_hint()` cut at 200 characters on a character boundary, control characters removed (an `AM_FIELD_SHAPE_INVALID` → `None`, logged `package_inspector.argument_hint_invalid` at `warn` with the command id);
   - **tools**: every binding of every ToolBinding part (`tool_bindings()`): `ToolEntry { name, kind: McpServer when type is mcp else BuiltIn, permission, declared_by, arguments: step 6b over the binding's argument_paths }`, sorted by name;
   - **memory**: the first Memory part in lock order → `memory_contract()` (an `AmError` → the part goes to `not_carried { reason: Unreadable }` and `memory = None`);
   - **connections**: `bundle.manifest.resolved_tool_policy.mcp_servers` → `ConnectionEntry { name, server_type, transport, needs_credential: env_ref.is_some() }`;
   - **not carried**: every part whose kind is not offered (`kind_info(kind).offered == false`) → `KindNotOffered`; every part named in `install.reference_only_parts` (the Installers' own list, never re-derived here) → `ReferenceOnly`;
   - each task's `session_name` = `install.command_names[command_id]`; `plugin_name` and `needs_person_present` from `install`;
   - `tool_policy_sha256` = SHA-256 hex of the canonical JSON of `bundle.manifest.resolved_tool_policy`;
   - `display_name` and `description` from the composition's manifest.
4. **APM-era path** (R-73, ADR-093 item 6). Build a `SourceTree` from the folder's files (at most 20,000 files and 256 MiB, else `PKI_FOLDER_UNREADABLE { retryable: false }`); call the APM plug-in's `read` with `publisher = "imported"` and `source_label = "an installed APM package"`; a `FormatError` → `PKI_FORMAT_UNRECOGNISED`. Parts become `PartLine`s (state `InUse`, `from` None); tasks from Command parts; tools and connections empty unless the read produced ToolBinding parts; every mapping item → `NotCarried` with its `UnmappedReason` mapped one to one. `source = ApmFolder { package_name, package_version }`.
5. **Declaration.** When `wrightrun-declaration.yaml` exists at the folder root: larger than 1 MiB (1,048,576 bytes) → `PKI_DECLARATION_TOO_LARGE`; else parse it with Shared types' restricted reader `ordin_types::yaml::read_restricted::<ordin_rules::Declaration>(text, 1_048_576)` (over `saphyr`; anchors, aliases, custom tags and over-size input refused, LW-46; 05 *YAML parsing*, decision P4-1); bytes that are not UTF-8, or any `YamlError` (`YAML_FEATURE_REFUSED`, `YAML_MALFORMED` — which covers a shape error — or `YAML_TOO_LARGE`), → `PKI_DECLARATION_INVALID` and the inspection stops. `declaration_sha256` = SHA-256 hex of the file bytes. When the file is absent, the declaration is `None`.
6. **Authority** (SA-CLOSE-01). Always derived, `authority_source = Derived` (*assumed here*; fail-safe, it never understates): `system_changing` when any tool entry has a permission other than `read-only`, or any connection exists, or the tool policy's `requires_human_approval` is non-empty, or the declaration's `approval_required` is non-empty; else `document_only`. `routing_only` is never produced here.
6b. **Argument places** (SA-CLOSE-02). Each JSON Pointer of a binding's `argument_paths` (already validated by the Agent Model) becomes one place: drop the leading `/`; split on `/`; unescape each token (`~1` → `/`, `~0` → `~`); join the tokens with `.`, except that a token `[]` is appended to the previous token without a `.` (list items) and a final token `*` becomes the suffix `.*` (every place below). Example: `/to/[]/address` → `to[].address`; `/payload/*` → `payload.*`. A token that, once unescaped, contains `.`, `[` or `]` (other than the whole token `[]`) cannot be expressed as a place: the place is left out and logged `package_inspector.argument_place_skipped` at `warn` with the tool name, so a rule naming it is refused by the Rules Engine with `RULE_UNKNOWN_ARGUMENT` (fail closed).
6a. **Rule set** (ordin path and APM-era path alike). Build `InstanceRuleSet { agent_instance_id, package_name: <composition PartRef.name, or the APM package name>, authority: <step 6>, agent_folder: request.folder_path, agent_folder_read_only: false, declaration: <step 5>, configured: None, tool_policy: <ordin path: Some(AgentToolPolicy { prohibited_verbs, forbidden_flags, requires_approval: requires_human_approval, mcp_servers: mcp_servers[].name }) from bundle.manifest.resolved_tool_policy; APM-era: None>, approval_list: <declaration.approval_required, de-duplicated in file order; empty with no declaration> (SA-CLOSE-03), package_read_only_tools: <names of tool entries of kind McpServer whose permission is read-only>, tools: <one ToolDescription { name, arguments: <the entry's arguments>, served_by: None } per tool entry> (SA-CLOSE-02), unmapped_operations: [], external: [], commands: <PartRef.name of each Command part, in lock order>, reviewer: None }`. Call `rules.load_rule_set(set)`: `Err(e)` → `PKI_DECLARATION_REFUSED { code: e.code(), retryable: e.retryable() }` and the inspection stops (the engine keeps the previous set; the Package Catalogue shows "This agent could not be added safely"). `Ok(lines)`: `declared_rules` = `reason` of every line whose `source` is `Package`, in the engine's order. Write the set as canonical JSON to `<store_dir>/<agent_instance_id>.rules.json` atomically (I/O failure → `PKI_STORE_FAILED { retryable: true }`), for `reload_rule_sets`.
7. **Store.** Serialise `PackageModel` as canonical JSON (sorted keys); write `<store_dir>/<agent_instance_id>.json` atomically (temporary file, `fsync`, rename); I/O failure → `PKI_STORE_FAILED { retryable: true }`. `parsed_declaration_ref = "pki:" + id + ":" + sha256_hex(file bytes)`. `may_do` = step 14. Send a change (step 16) for each workspace of the instance. Log `package_inspector.inspected`. Return.

Other operations:

8. **`model(id)`**: read and parse the stored file; absent → `PKI_NOT_INSPECTED`; unparseable or another schema → `PKI_STORE_FAILED { retryable: false }` (the Package Catalogue re-runs `inspect`).
9. **`made_of(id, detail)`**: from the stored model. Group `parts` by kind into the 11 §5.29 order: SystemRole; Rule; OperatingMode; Skill and Capability (one group, "Abilities"); Command; ContextResource; ToolBinding; Memory; ControlLoop; PracticePack; LanguageOverlay, StyleOverlay, HandoffOverlay and — for `LinksCode` only — SourceWorkflowOverlay; Evaluation; OutputContract. Kinds outside these (Vocabulary, Guideline, Journey, WorkspaceProfile itself, the not-offered kinds) are not listed. Empty groups are left out. Within a group, parts sorted by `name` (locale-independent byte order; the screen sorts for display). For `BusinessUser`, each `PartLine.part` is reduced to kind and name only (identifier, version, digest, repository and commit cleared to empty strings) so nothing technical reaches the screen (11 §5.29 *Never shown*); `pinned_by` is set only for `LinksCode`, from the composition's repository and commit.
10. **`installed_files(id, folder)`**: walk the folder without following symbolic links (a link is listed with its own size and the digest of its target path text), at most 20,000 entries; `ordin.lock` first, then every other file in byte order of its path; each with size and SHA-256. Unreadable → `PKI_FOLDER_UNREADABLE`.
11. **`part_page(id, part_id, bundle_dir, detail)`**: find the `PartLine` (absent → `PKI_PART_NOT_FOUND`); read its primary file from the bundle (missing or unreadable → `PKI_BUNDLE_UNREADABLE`); `text` = `markdown_body` for Markdown kinds, else the whole YAML text; files over 100 KiB are cut at 100 KiB on a character boundary with the line "…" appended (A-45 bound). Control characters other than `\n` and `\t` are removed. `file` set for `LinksCode` only.
12. **`forget(id)`**: delete the stored model and `<id>.rules.json` (absent is not an error), drop the instance from `memberships.json` (sending a change for each workspace it was in) and call `rules.unload_rule_set(id)` (infallible).
12a. **`reload_rule_sets()`** (program start): for each `<store_dir>/*.rules.json` in file-name order, parse it (unparseable → a failure `PKI_STORE_FAILED` for that instance) and call `rules.load_rule_set`; a `RulesError` is a failure with `e.code()`. Every failure is logged `package_inspector.rule_set_reload_failed` (warn) and returned in the list; the other instances still load. An instance whose set did not load has no rule set, so the Rules Engine's `begin_run` refuses its sessions with `RULE_RULESET_NOT_LOADED` (fail-safe).
13. **`inspect_candidate(request)`**: steps 1–4 as for `inspect`. Steps 5 and 6 run as for `inspect` (size check, parse, authority); step 6a is skipped — the Rules Engine is not called, so the loaded rules of the installed agent stay in force — and `declared_rules` is empty. Nothing is written and no change is sent. Return the model. A candidate whose declaration differs from the installed one therefore always shows `DeclarationChanged` and goes to the person (fail-safe).
14. **`may_do(model)`**: `tools` = names of `tools` entries whose `permission` ≠ `read-only`; `reads` = names whose `permission` = `read-only`; `connections` = names of `connections`; each list sorted and de-duplicated by byte order; `tool_policy_digest` = `tool_policy_sha256`; `declaration_digest` = `declaration_sha256`; `authority` = `authority`.
15. **`compare_may_do(installed, candidate)`**: for each list, names in `candidate` not in `installed` → `*Added`, the reverse → `*Removed`; digests that differ (including `None` against `Some`) → `PolicyChanged` / `DeclarationChanged` with empty `name`; authorities that differ → `AuthorityChanged` with the candidate's authority. `message_id` = `"may-do-"` + the kind in kebab-case. Order: by `MayDoChangeKind` declaration order, then by `name` byte order.
16. **Directory and changes.** `set_workspaces(id, ids)`: replace the instance's entry in `<store_dir>/memberships.json` (canonical JSON map of instance id → sorted workspace ids; written atomically; I/O failure → `PKI_STORE_FAILED { retryable: true }`); send a change for every workspace in the old or the new list. `directory(workspace_id)`: for every instance whose memberships include the workspace, read its stored model (an instance with no stored model is left out and logged `package_inspector.directory_missing_model`); build `AgentDirectoryEntry` from the model — `skills` = `name` of the Skill and Capability `parts`, `lifecycle` = `name` of the first Journey part; order by `display_name` then instance id. `changes()`: a subscription to an in-process `tokio::sync::broadcast` channel of capacity 256 carrying `WorkspaceId`; a subscriber that falls behind receives every workspace id in `memberships.json` once, so it rebuilds every directory.
17. **`memory_contract(id)`** (SA-CLOSE-05): `model(id)` with its errors unchanged (`PKI_NOT_INSPECTED`, `PKI_STORE_FAILED`) → `Ok(model.memory)`. An APM-era model has `memory = None`.

## Data Model

No table: the crate has no Journal dependency (2B-3). "Owns: the parsed package model per instance" (02:479) is one file per instance:

```text
<application data root>/inspector/
  <agent instance id>.json      PackageModel, canonical JSON, schema "ordin.package-model/v1"
  memberships.json              instance id → workspace ids, for directory (canonical JSON)
```

The Package Catalogue stores `parsed_declaration_ref` on `agent_instances` (2C); a mismatch between that reference's digest and the file's digest means the file changed outside ordin, and the Package Catalogue re-runs `inspect`. The file is recomputed on every install and update, so no migration is needed when the schema version changes: an unknown schema is treated as absent.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Agent folder cannot be listed or read (transient) | `Err(retryable)` | `PKI_FOLDER_UNREADABLE` | `InspectError::FolderUnreadable { retryable: true }` |
| Folder over 20,000 files or 256 MiB, or permission denied | `Err(permanent)` | `PKI_FOLDER_UNREADABLE` | `InspectError::FolderUnreadable { retryable: false }` |
| `ordin.lock` does not parse or has another schema | `Err(permanent)` | `PKI_LOCK_FILE_INVALID` | `InspectError::LockFileInvalid` |
| Bundle directory missing, or its plan hash differs from the lock file's, or a part's primary file cannot be read at `part_page` | `Err(permanent)` | `PKI_BUNDLE_UNREADABLE` | `InspectError::BundleUnreadable` |
| Neither `ordin.lock` nor `apm.yml`, or the APM plug-in refuses the folder | `Err(permanent)` | `PKI_FORMAT_UNRECOGNISED` | `InspectError::FormatUnrecognised` |
| Declaration over 1 MiB (1,048,576 bytes) | `Err(permanent)` | `PKI_DECLARATION_TOO_LARGE` | `InspectError::DeclarationTooLarge { bytes }` |
| `wrightrun-declaration.yaml` does not parse into the Rules Engine's `Declaration` | `Err(permanent)` | `PKI_DECLARATION_INVALID` | `InspectError::DeclarationInvalid` |
| The Rules Engine refuses the rule set (`load_rule_set` → `RulesError`) | `Err(retryable)` / `Err(permanent)` per `RulesError::retryable()` | `PKI_DECLARATION_REFUSED` | `InspectError::DeclarationRefused { code, retryable }` |
| `reload_rule_sets`: one stored set unparseable or refused | `Ok` with a `RuleSetReloadFailure` for that instance | `PKI_STORE_FAILED` or the `RULE_*` code | `RuleSetReloadFailure { agent_instance_id, code }` |
| `model`, `made_of` or `part_page` before any `inspect` | `Err(permanent)` | `PKI_NOT_INSPECTED` | `InspectError::NotInspected` |
| `part_page` for a part the model does not hold | `Err(permanent)` | `PKI_PART_NOT_FOUND` | `InspectError::PartNotFound { part_id }` |
| Model file write fails | `Err(retryable)` | `PKI_STORE_FAILED` | `InspectError::StoreFailed { retryable: true }` |
| Stored model unparseable | `Err(permanent)` | `PKI_STORE_FAILED` | `InspectError::StoreFailed { retryable: false }` |
| `memberships.json` write fails | `Err(retryable)` | `PKI_STORE_FAILED` | `InspectError::StoreFailed { retryable: true }` |
| `memberships.json` unparseable at `directory` | `Err(permanent)` | `PKI_STORE_FAILED` | `InspectError::StoreFailed { retryable: false }` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Package Inspector

  Scenario: Happy path — an installed ordin agent
    Given cspa-431 installed for Claude Code with its lock file and bundle
    When inspect is called
    Then the model lists the agent's 10 commands as tasks
    And its authority is "document_only" derived, because it has no tool binding and no connection
    And parsed_declaration_ref starts with "pki:0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff:"

  Scenario: Happy path — what it's made of for a business user
    Given an inspected agent with 1 role, 3 rules, 4 abilities and 3 tasks
    When made_of is called with BusinessUser
    Then the groups appear in the order role, rules, abilities, tasks
    And no part line carries an identifier, version, digest, repository or commit
    And pinned_by is absent

  Scenario: Edge — a borrowed part says where it comes from
    Given a part whose repository is "github.com/acme/company-agents" while the agent is from the bundled catalogue
    And repository_names maps it to "Company agents"
    When inspect is called
    Then that part line's from is "Company agents"

  Scenario: Edge — a declaration's approval list reaches the Rules Engine
    Given the folder holds wrightrun-declaration.yaml with approval-required ["email_send"]
    When inspect is called
    Then the rule set handed to load_rule_set has approval_list ["email_send"]
    And the model's authority is "system_changing" with source "derived"

  Scenario: Edge — argument places from a tool binding
    Given a ToolBinding binding "email_send" with argumentPaths ["/to/[]/address", "/attachments/*"]
    When inspect is called
    Then the rule set's tool "email_send" has arguments ["to[].address", "attachments.*"]

  Scenario: Edge — a command's argument hint reaches the agent directory
    Given a Command part "compare-quotes" whose front matter sets argument-hint "<suppliers> <quarter>"
    When inspect is called and directory is called for its workspace
    Then the task "compare-quotes" carries argument_hint "<suppliers> <quarter>"

  Scenario: Happy path — the memory contract of an installed agent
    Given an installed agent whose Memory part retains ["decision"] with maxAgeDays 90
    When memory_contract is called after inspect
    Then the result holds retains ["decision"] and max_age_days 90

  Scenario: Error — memory contract before inspection
    When memory_contract is called for an instance never inspected
    Then it fails with PKI_NOT_INSPECTED

  Scenario: Edge — an APM-era folder
    Given an agent folder installed through apm before revision 0.25.0, with apm.yml and a hook script
    When inspect is called without a bundle directory
    Then the model's source is an APM folder with the package name and version
    And not_carried holds the hook with reason "executable"

  Scenario: Edge — installed files list the lock file first
    Given an inspected agent
    When installed_files is called
    Then the first entry is "ordin.lock" and the rest follow in path order

  Scenario: Error — lock file damaged
    Given the agent folder's ordin.lock is not valid YAML
    When inspect is called
    Then it fails with PKI_LOCK_FILE_INVALID

  Scenario: Error — declaration refused
    Given the Rules Engine refuses the agent's rule set with RULE_UNBOUND_TOOL
    When inspect is called
    Then it fails with PKI_DECLARATION_REFUSED carrying code RULE_UNBOUND_TOOL and no model is stored

  Scenario: Error — declaration does not parse
    Given the agent folder's wrightrun-declaration.yaml uses a YAML alias
    When inspect is called
    Then it fails with PKI_DECLARATION_INVALID and the Rules Engine is not called

  Scenario: Happy path — rule sets reload at start
    Given two inspected instances whose rule sets are stored, one of which the Rules Engine now refuses
    When reload_rule_sets is called
    Then the other instance's rule set is loaded
    And the result holds one RuleSetReloadFailure with the refused instance's id and its RULE_ code

  Scenario: Error — unknown folder format
    Given an agent folder with neither ordin.lock nor apm.yml
    When inspect is called
    Then it fails with PKI_FORMAT_UNRECOGNISED

  Scenario: Happy path — an update that changes nothing the agent may do
    Given an installed agent whose may-do summary lists the tool "search" as a read
    And an update candidate with the same tools, reads, connections, policy and declaration
    When inspect_candidate is called on the candidate and compare_may_do compares the two summaries
    Then the result is empty
    And the installed agent's stored model is unchanged

  Scenario: Edge — an update that adds a connection
    Given an update candidate that adds the connection "mail"
    When compare_may_do compares the installed and candidate summaries
    Then the result holds one change of kind connection_added named "mail" with message id "may-do-connection-added"

  Scenario: Edge — a candidate with a changed declaration
    Given an update candidate whose wrightrun-declaration.yaml differs from the installed one
    When inspect_candidate is called
    Then the Rules Engine is not called
    And compare_may_do reports declaration_changed

  Scenario: Happy path — the agent directory of a workspace
    Given instances "Analyst" and "Builder" inspected and set_workspaces placing both in workspace W
    When directory is called for W
    Then two entries are returned, "Analyst" first
    And each task carries its session name such as "analyst:summarise"
    And the changes stream yielded W after set_workspaces

  Scenario: Error — page for an unknown part
    When part_page is called for "urn:ordin:default:skills:none"
    Then it fails with PKI_PART_NOT_FOUND
```

## Performance, Security, Observability

- **Performance targets:** `made_of` for 200 parts ≤ 300 ms p95 end to end (A-45) — served from the stored model, ≤ 20 ms inside this crate; `part_page` for a 100 KB part ≤ 200 ms p95 (A-45), ≤ 30 ms inside this crate; `inspect` of a 200-part bundle ≤ 500 ms p95; `installed_files` for 1,000 files ≤ 400 ms p95.
- **Security:** everything read is data (02:480, R-84): no file is executed, no text is followed; the declaration is parsed as data by this component's restricted YAML reader and evaluated only by the Rules Engine; MCP credentials are never read (`needs_credential` is a flag); symbolic links are not followed; text returned for a page has control characters removed and is size-bounded. A business-user view clears every technical field before it leaves the crate.
- **Observability:** `tracing` events (SA-OBS-01); never part text or the person's paths:
  - `package_inspector.inspected` (info): `agent_instance_id`, `source` (`ordin_bundle` or `apm_folder`), `parts`, `tasks`, `tools`, `connections`, `authority`, `authority_source`, `not_carried`, `duration_ms`.
  - `package_inspector.inspect_failed` (warn): `agent_instance_id`, `error_code`.
  - `package_inspector.declaration_loaded` (info): `agent_instance_id`, `declaration_sha256`.
  - `package_inspector.rule_set_reload_failed` (warn): `agent_instance_id`, `error_code`.
  - `package_inspector.candidate_inspected` (info): `agent_instance_id`, `duration_ms`.
  - `package_inspector.directory_missing_model` (warn): `agent_instance_id`, `workspace_id`.
  - `package_inspector.argument_hint_invalid` (warn): `agent_instance_id`, `command_id`.
  - `package_inspector.argument_place_skipped` (warn): `agent_instance_id`, `tool`.
  No metrics or spans (SA-XC-04, SA-XC-05).

## Gaps

None.
