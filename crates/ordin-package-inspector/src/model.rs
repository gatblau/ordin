//! The public value types of the Package Inspector (PackageInspector.md *Public Interface*).

use std::collections::BTreeMap;
use std::path::PathBuf;

use ordin_agent_model::{MemoryContract, PartKind, PartRef};
use ordin_types::domain::agents::Authority;
use ordin_types::time::Timestamp;
use ordin_types::AgentInstanceId;

/// The schema name written into every stored model.
pub const MODEL_SCHEMA: &str = "ordin.package-model/v1";

#[derive(Debug, Clone)]
pub struct InspectRequest {
    pub agent_instance_id: AgentInstanceId,
    /// 2C `AgentInstance.agent_folder_path`.
    pub folder_path: PathBuf,
    /// The bundle the lock file's plan hash names (`<data root>/bundles/<hex>/`), given by the
    /// Package Catalogue; None for an APM-era folder (R-73).
    pub bundle_dir: Option<PathBuf>,
    /// Display names of the repositories the parts come from, keyed by `PartRef.repository`
    /// (`bundled` is "ordin"), supplied by the Package Catalogue from the Catalogue Service.
    pub repository_names: BTreeMap<String, String>,
    /// Withdrawn and deprecated states of the parts, keyed by `PartRef.id`, from the Catalogue Service.
    pub part_states: BTreeMap<String, PartState>,
    /// Facts from the Installers' install record, copied by the Package Catalogue; None for an
    /// APM-era folder. The Inspector does not depend on the Installers.
    pub install: Option<InstallFacts>,
}

/// The install-record facts the model carries (Installers `InstallRecord`).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InstallFacts {
    /// `HarnessLaunch::ClaudeCode.plugin_name`; None for Copilot.
    pub plugin_name: Option<String>,
    /// `InstallRecord.commands`: command part identifier to the name the session accepts (R-94).
    pub command_names: BTreeMap<String, String>,
    /// `InstallRecord.reference_only_parts`.
    pub reference_only_parts: Vec<String>,
    /// `InstallRecord.needs_person_present`.
    pub needs_person_present: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartState {
    InUse,
    NotForNewAgents,
    Withdrawn,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSource {
    OrdinBundle {
        plan_hash: String,
        repository: String,
        commit: String,
    },
    ApmFolder {
        package_name: String,
        package_version: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TaskEntry {
    pub command_id: String,
    pub name: String,
    pub description: String,
    /// The name the harness session accepts (`<plugin>:<command>` for Claude Code), from
    /// `InstallFacts.command_names`; None for an APM-era folder.
    pub session_name: Option<String>,
    /// What the command expects as arguments, from its file's front matter `argument-hint`
    /// (at most 200 characters, control characters removed); None when absent (SA-CLOSE-04).
    #[serde(default)]
    pub argument_hint: Option<String>,
    /// The task's display name (`Manifest::title`, ADR-119 item 1); None when the command has none.
    // spec: ordin-desktop/components/PackageInspector#PKI-TF-1
    #[serde(default)]
    pub title: Option<String>,
    /// The task's inputs (`Manifest::inputs`); None when the command declares none, so its form is the
    /// one long-text field of ADR-119 item 2.
    #[serde(default)]
    pub inputs: Option<Vec<ordin_agent_model::TaskInput>>,
    /// True when `title` or `inputs` is present and does not read; the task is then listed but cannot
    /// be started from a form (OrdinHost.md OH-TF-1 step 2).
    #[serde(default)]
    pub form_unreadable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    BuiltIn,
    McpServer,
    Declared,
}

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
    #[serde(default)]
    pub arguments: Vec<String>,
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
pub enum AuthoritySource {
    /// SA-CLOSE-01: the declaration states no authority.
    Derived,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NotCarried {
    /// Part identifier, or the APM path of a construct that did not map.
    pub what: String,
    pub reason: NotCarriedReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotCarriedReason {
    KindNotOffered,
    ReferenceOnly,
    NoEquivalentKind,
    Executable,
    UnknownField,
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PartLine {
    pub part: PartRef,
    /// `metadata.name`, else the slug.
    pub name: String,
    /// `metadata.description`, first sentence, at most 300 characters.
    pub what_it_does: String,
    pub state: PartState,
    /// "From: ..." display name; None when the part is from the agent's own repository.
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
    #[serde(default)]
    pub memory: Option<MemoryContract>,
    pub parts: Vec<PartLine>,
    pub inspected_at: Timestamp,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct InspectOutcome {
    pub model: PackageModel,
    /// `pki:<agent instance id>:<sha256 hex of the stored model file>` for 2C `parsed_declaration_ref`.
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
pub enum MayDoChangeKind {
    ToolAdded,
    ToolRemoved,
    ReadAdded,
    ReadRemoved,
    ConnectionAdded,
    ConnectionRemoved,
    PolicyChanged,
    DeclarationChanged,
    AuthorityChanged,
}

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
pub enum Detail {
    BusinessUser,
    LinksCode,
}

/// Service API `agents.made_of` input.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MadeOfInput {
    pub agent_instance_id: AgentInstanceId,
    pub detail: Detail,
}

/// Service API `agents.installed_files` input.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct InstalledFilesInput {
    pub agent_instance_id: AgentInstanceId,
}

/// Service API `agents.part_page` input. `part_id` is a `PartLine` identifier.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PartPageInput {
    pub agent_instance_id: AgentInstanceId,
    pub part_id: String,
    pub detail: Detail,
}

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
    /// Command names, for "Its tasks: ...".
    pub tasks: Vec<String>,
    pub part_count: u32,
    pub groups: Vec<KindGroup>,
    /// LinksCode only: (repository, commit) for "Pinned by its lock file: [repository] at [commit]".
    pub pinned_by: Option<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct InstalledFileEntry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// One installed file's text, for the reader of *What [agent] is made of*; data, never obeyed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct InstalledFileText {
    /// The `/`-separated path inside the agent folder, as `installed_files` lists it.
    pub path: String,
    /// The file's text with control characters other than line breaks and tabs removed, cut at
    /// 100 KiB on a character boundary.
    pub text: String,
    /// True when the file was longer than the text shown.
    pub cut: bool,
}

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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RuleSetReloadFailure {
    pub agent_instance_id: AgentInstanceId,
    pub code: String,
}

/// The kinds a person can see as "Skill" or "Capability" in the directory.
pub(crate) fn is_ability(kind: PartKind) -> bool {
    matches!(kind, PartKind::Skill | PartKind::Capability)
}
