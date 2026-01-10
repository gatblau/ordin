# SPEC: Package Catalogue

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-package-catalogue` · **Build phase:** 7 · **Programs:** D P
> **Dependencies:** Catalogue Service, Resolver and Compiler, Installers, Checker, Organisation Profile, Agent Folder Manager, Package Inspector, Journal

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

The Package Catalogue makes adding an agent one action for the person — **Add it** — and keeps the list of agent instances on this computer with their install records (02:422–432, amended by ADR-082). For each add it asks the Checker whether the agent's parts pass and fall inside the company's trust policy, has the Resolver and Compiler build the bundle with the packs and overlays picked for the workspace (ADR-089, ADR-090 §5), has the Agent Folder Manager create the folder and the session harness's Installer write it, and has the Package Inspector describe what the agent can do. It applies updates between runs when what the agent may do or read is unchanged and asks the person otherwise (A-16), stops agents whose parts are withdrawn or moves them to the named replacement (ADR-090 §6), records the person's answer when an agent asks which pack to use (decision 64), installs the person's own draft as a trial, and removes an agent after the person confirms. It never edits a part's files.

## Approach

An orchestrator over the agent-catalogue modules with one persisted state machine per instance (2C `AgentInstanceStatus`): every step is a call to the module that owns the decision, and this component records the outcome. "Between runs" is read through a trait this component owns, `InstanceActivity`, which the program implements over the Session Supervisor (ADR-100 pattern: the Session Supervisor sits in a lower build phase and cannot be called directly). Whether an update changes what the agent may do or read is decided by comparing the Package Inspector's summaries of the installed and the candidate folder, in fixed code (ADR-085). Error codes take the prefix `PKG_` (*chosen here*); 2C's `AGI_*` codes are used for record validation. Rejected: *version-number rule for updates* — superseded in 0.36.0 (A-16); *a separate instance per workspace for every agent regardless of choices* — rejected in favour of one instance per (agent, workspace), because pack choices are recorded per workspace (decision 64) and change the bundle.

### Reconciliation assumptions

- **SA-RECON-01** (R6): every provider's own names and types win. This component holds the Catalogue Service, Resolver and Compiler, Checker, Installers, Agent Folder Manager and Package Inspector as their concrete types and calls their inherent methods with their error types (`CatalogueError`, `ResolveError`, `CheckError`, `InstallError`, `AfmError`, `InspectError`); the earlier port traits `Resolve`, `InstallChecks`, `Install`, `AgentFolders` and `Inspect` are dropped.
- **SA-RECON-02** (R1): the `agents.*` command table under *Public Interface* is the authoritative list the Service API catalogue merges.
- **SA-RECON-03** (R7): `AgentInstance` (with `stopped_reason`, C-22) and `HarnessKind` (C-1) are copied from the corrected 2C; a withdrawn part or lost trust stops an instance as `refused` with `stopped_reason`.
- **SA-RECON-04** (R3): migrations are exported by `migrations() -> ModuleMigrations`; journal entries are written with `Journal::append_entry(NewJournalEntry)`.
- **SA-RECON-05** (R18): the Resolver and the Checker read the Catalogue Service through their late-bound source; a trial of an own draft passes `catalogue.draft_source(...)` per request as `SourceOverride`.
- **SA-RECON-06** (pass 1b): `agents_in_workspace` is owned here because the Package Catalogue owns which instances serve a workspace (`agent_instance_workspaces`).
- **SA-RECON-07**: bundles live under the Resolver's `bundle_root` and are pruned by plan hash after each install, update and removal (ResolverAndCompiler.md `prune`); the earlier `<data root>/agents-staging` folder is dropped.

### Closing-round assumptions

- **SA-CLOSE-01** (*chosen here*; closes G-PKG-1): an instance's complete executable-part list comes from the Checker's `executable_parts(id)`, which returns the list the Checker derived and stored at the instance's latest `check_for_install` with `agent_instance_id = Some(id)`. **Add it** therefore mints the instance id (UUIDv7) before its install check and passes `Some(id)` (§A4); `preview_add` still passes `None`, so a preview stores nothing.
- **SA-CLOSE-02** (*chosen here*; closes G-PKG-2): 2C gains the transition `refused → ready`, taken in two cases only: (a) an instance stopped because its trust was lost (`stopped_by = trust`) returns to `ready` when the Checker's `reevaluate_trust` lists it in `gained`, because its folder is unchanged and the signed profile now allows every program it starts (§L3); (b) an instance stopped by a withdrawal (`stopped_by = withdrawal`) returns to `ready` when the person accepts a fixed version — a later repository commit whose plan no longer names the withdrawn part (§C5). Every other `refused` instance stays refused until it is removed.
- **SA-CLOSE-03** (*chosen here*): `source_scope(instance)` is the scope the Checker's **Check it** uses for an installed agent: the Catalogue Service's scope of the instance's home repository, or of the bundled catalogue for a bundled agent. A trial of an own draft has no merged repository and returns `PKG_TRIAL_NO_SCOPE`; the draft itself is checked by the Authoring Service.
- **SA-CLOSE-04** (*chosen here*; serves the Ordin Host's pack question, decision 64): an installed instance's open pack questions are read with `pending_choices(instance)`, which returns one `PendingChoice` per `PendingPackChoice` its installed plan left for the agent to ask. `PendingChoice` carries exactly what `record_answer` needs to answer it (`agent_instance_id`, `rule_index`, the candidate pack identifiers) plus the words the question shows (`class_label`, each candidate's name). The installed plan's `selections.pending_choices` are stored on the instance (`agent_instances.pending_choices`, migration 1) whenever `plan_hash` is stored, so the read needs no re-plan.

## Shared Context

### Shared types used (copied from Phase 2C, unchanged)

```rust
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>);
pub struct Sha256Hex(pub String);
// UUIDv7 newtypes: AgentInstanceId, AgentRepositoryId, PartId, WorkspaceId, AgentDraftId, WithdrawnListEntryId

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentInstance {
    pub id: AgentInstanceId,
    pub display_name: String,                 // 1–120 (AGI_NAME_INVALID)
    pub package_name: String,                 // [a-z0-9._-]{1,214} (AGI_PACKAGE_NAME_INVALID)
    pub package_version: String,              // 1–64 (AGI_PACKAGE_VERSION_INVALID)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_repository_id: Option<AgentRepositoryId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,               // 40 hex (AGI_COMMIT_INVALID); with repository (AGI_REPOSITORY_MISMATCH)
    pub agent_folder_path: String,            // absolute, under the data root, unique (AGI_FOLDER_INVALID)
    pub lock_file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audit_result: Option<AuditResult>,
    pub authority: Authority,
    pub parsed_declaration_ref: String,
    pub is_trial_of_own_draft: bool,
    pub status: AgentInstanceStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped_reason: Option<String>,       // plain words; with status refused (2C C-22)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_between_runs_at: Option<Timestamp>,
    pub installed_at: Timestamp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditResult { Passed, Failed }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Authority { DocumentOnly, SystemChanging, RoutingOnly }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentInstanceStatus { Installing, Ready, Refused, Updating, UpdateAwaitingChoice, Removed }

// Part, PartKind, PartStatus, WithdrawnListEntry: as the Catalogue Service spec copies them from 2C.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairAction { TryAgain, SignInAgain, SendToIt }

// ordin_types::domain::identity (2C, decision 80: C-1)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessKind { ClaudeCode, CopilotCli }

// ordin_types::workflow_file (2C wire)
pub enum AgentRepositoryRef { ProfileKey(String), Address(String) }
```

2C transitions this component enforces: — →`installing` on **Add it**; `installing`→`ready` when install and checks pass; `installing`→`refused` when a check fails (folder removed); `ready`→`updating` (update that changes nothing it may do or read, between runs); `ready`→`update_awaiting_choice` (update that changes what it may do or read); `update_awaiting_choice`→`updating` (accepted) or →`ready` (declined); `updating`→`ready`; `ready`→`refused` when a part it uses is withdrawn or its trust is lost, with `stopped_reason` set (2C C-22); `refused`→`ready` when trust is regained or the person accepts a fixed version (SA-CLOSE-02); `ready`→`removed` (person confirms). `refused` only when `audit_result = failed`, the Checker refused it, or `stopped_reason` is present (`AGI_STATE_INCONSISTENT`). Storage: `agent_instances`, journal database, owned by the Package Catalogue (2C).

### Words this component owns (from 11; rendered by the screens through Fluent, message ids *chosen here*)

| Message id | en-GB text | Source |
|---|---|---|
| `agent-add-unsafe` | This agent could not be added safely | 03 *Add an agent*; 02:425 |
| `agent-add-needs-it` | This agent needs IT approval | 03 *Add an agent*; ADR-013 |
| `agent-updated-between-runs` | Updated between runs today; what it may do hasn't changed, so nothing was asked. | A-16; 02:428 |
| `agent-unavailable-pack-missing` | Not available in this workspace: it needs a [kind of pack] this workspace doesn't settle. | ADR-089 |
| `agent-stopped-withdrawn` | [agent] stopped: [ability] was withdrawn by [repository]. | ADR-082, ADR-090 §6 |
| `agent-stopped-trust-lost` | [agent] stopped: your company's rules no longer allow one of the programs it uses. IT can approve it. | 04:52 (trust lost) |

### Boundary interfaces this spec uses (owned by other components; copied from their specs)

Every provider is held as its concrete type; the crates export no traits for these calls (Checker, Agent Folder Manager), and a test double is a private port trait in this crate with exactly these signatures.

```rust
// ordin-agent-model — Agent Model (AgentModel.md), imported through the Resolver's re-export
// `ordin_resolver::model` (2B-3 gives this crate no Agent Model edge), as the Release Builder does.
pub struct SourceScope { pub home: RepositoryRef, pub profile: Vec<RepositoryRef>, pub include_bundled: bool }
pub enum RepositoryRef { Repository(AgentRepositoryId), Bundled }
pub struct PartRef { pub kind: PartKind, pub id: String, pub name: String, pub version: String,
                     pub content_hash: String, pub repository: String, pub commit: String, pub loading: Option<LoadingMode> }
pub struct ResolutionPlan { pub plan_hash: String, pub root: PartRef, pub target_harness: String,
    pub overlay_order: Vec<PartRef>, pub practice_packs: Vec<PartRef>, pub resolved: Vec<PartRef>,
    pub selections: Selections, pub warnings: Vec<String>, pub withdrawn_snapshot: String,
    pub tool_policy: Option<ResolvedToolPolicy> }
pub struct Selections { pub practice_packs: Vec<SelectedPart>, pub overlays: Vec<SelectedPart>, pub pending_choices: Vec<PendingPackChoice> }
pub struct SelectedPart { pub id: String, pub chosen_by: ChosenBy }
pub enum ChosenBy { WorkspaceCode, MakerFixed, PersonAnswer, PersonChange, AgentDefault, InterfaceLanguage }
pub struct PendingPackChoice { pub rule_index: u32, pub at_least: u32, pub class: Option<String>, pub choices: Vec<String> }
pub struct LockFile { /* lockfile_schema, plan_hash, target_harness, composition, artefacts, overlays_applied, selections */ }
pub trait PartSource;   // implemented by the Catalogue Service
// Types named below and not copied here (ExecutablePartDecision, ExecutablePartKind, CheckReport, BundleManifest,
// HarnessLaunch, CommandName, PackageModel, MayDoChangeKind, AgentFolderState) are as their owners' specs define them.

// ordin-catalogue — Catalogue Service (CatalogueService.md)
impl CatalogueService {
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<CatalogueNotice>;
    pub async fn part(&self, id: PartId) -> Result<PartView, CatalogueError>;
    pub async fn find_part(&self, repository: AgentRepositoryId, kind: PartKind, identifier: &str, version: Option<&str>) -> Result<PartView, CatalogueError>;
    pub async fn search(&self, query: LibraryQuery) -> Result<Page<LibraryEntry>, CatalogueError>;
    pub async fn list_repositories(&self) -> Result<Vec<RepositorySummary>, CatalogueError>;
    pub async fn source_scope(&self, home: RepositoryRef) -> Result<SourceScope, CatalogueError>;
    pub async fn repository_by_ref(&self, reference: &AgentRepositoryRef) -> Result<Option<AgentRepository>, CatalogueError>;
    pub async fn repository_ref(&self, id: AgentRepositoryId) -> Result<AgentRepositoryRef, CatalogueError>;
    pub async fn draft_source(&self, id: AgentRepositoryId, worktree: std::path::PathBuf) -> Result<std::sync::Arc<dyn PartSource>, CatalogueError>;
}
pub enum CatalogueNotice {
    RepositoryUpdated { agent_repository_id: AgentRepositoryId, commit: String, newly_withdrawn: Vec<WithdrawnListEntryId> },
    RepositoryDropped { agent_repository_id: AgentRepositoryId },
}
// PartView, LibraryQuery, LibraryEntry, RepositorySummary, Page<T>, CatalogueError { code, retryable, action, detail }: as CatalogueService.md defines them.

// ordin-resolver — Resolver and Compiler (ResolverAndCompiler.md)
impl ResolverAndCompiler {
    pub async fn resolve(&self, request: &ResolveRequest) -> Result<ResolutionPlan, ResolveError>;
    pub async fn build(&self, request: &ResolveRequest) -> Result<Bundle, ResolveError>;
    pub fn prune(&self, keep: &std::collections::BTreeSet<String>) -> Result<PruneReport, ResolveError>;
}
pub struct ResolveRequest {
    pub scope: SourceScope, pub root_id: String, pub root_version: Option<String>,
    pub target_harness: HarnessKind,         // 2C `HarnessKind`; plan and lock carry its snake_case spelling
    pub workspace_languages: Vec<String>, pub interface_language: String,
    pub pack_answers: Vec<PackAnswer>, pub pack_changes: Vec<String>, pub overlay_changes: Vec<String>,
    pub replacements: Vec<Replacement>, pub staged: Vec<StagedPart>, pub allow_draft_root: bool,
    pub source: Option<SourceOverride>,
}
pub struct PackAnswer { pub rule_index: u32, pub pack_ids: Vec<String> }
pub struct Replacement { pub withdrawn_id: String, pub withdrawn_kind: PartKind, pub replacement_id: String, pub replacement_version: Option<String> }
pub struct SourceOverride(pub std::sync::Arc<dyn PartSource>);
pub struct Bundle { pub plan: ResolutionPlan, pub manifest: BundleManifest, pub lock_file: LockFile, pub lock_file_bytes: Vec<u8>, pub dir: std::path::PathBuf }
pub struct PruneReport { pub removed: u32, pub kept: u32, pub bytes_freed: u64 }
pub enum ResolveError { /* RES_* codes, e.g. RequiredPackUnsettled { rule_index, class }, PartWithdrawn { part, replacement }, SourceNotBound */ }
impl ResolveError { pub fn retryable(&self) -> bool; }

// ordin-checker — Checker (Checker.md)
impl Checker {
    pub async fn check_for_install(&self, request: &InstallCheck) -> Result<InstallCheckReport, CheckError>;
    pub async fn executable_parts(&self, instance: AgentInstanceId) -> Result<Vec<ExecutablePart>, CheckError>;
    pub async fn decide_executable_parts(&self, id: AgentInstanceId, parts: &[ExecutablePart]) -> Result<Vec<ExecutablePartDecision>, CheckError>;
    pub async fn forget_instance(&self, id: AgentInstanceId) -> Result<(), CheckError>;
}
pub struct InstallCheck { pub plan: ResolutionPlan, pub scope: SourceScope, pub agent_instance_id: Option<AgentInstanceId>, pub source: Option<SourceOverride> }
pub enum InstallVerdict { Clean, Failed, NeedsItApproval { parts: Vec<ExecutablePart> } }
pub struct InstallCheckReport { pub verdict: InstallVerdict, pub report: CheckReport }
pub struct ExecutablePart { pub part_name: String, pub kind: ExecutablePartKind, pub declared_by: String }
pub struct TrustChanges { pub lost: Vec<AgentInstanceId>, pub gained: Vec<AgentInstanceId> }
pub enum CheckError { /* CHK_* codes */ }
impl CheckError { pub fn retryable(&self) -> bool; }

// ordin-installers — Installers (Installers.md)
impl Installers {
    pub async fn install(&self, request: &InstallRequest) -> Result<InstallOutcome, InstallError>;
    pub async fn update(&self, request: &InstallRequest) -> Result<InstallOutcome, InstallError>;
    pub async fn update_in_session(&self, request: &InstallRequest) -> Result<InstallOutcome, InstallError>;
    pub async fn verify(&self, id: AgentInstanceId) -> Result<VerifyOutcome, InstallError>;
    pub async fn verify_and_repair(&self, id: AgentInstanceId, scope: &SourceScope) -> Result<InstallOutcome, InstallError>;
    pub async fn uninstall(&self, id: AgentInstanceId) -> Result<(), InstallError>;
    pub async fn record(&self, id: AgentInstanceId) -> Result<InstallRecord, InstallError>;
}
pub struct InstallRequest { pub agent_instance_id: AgentInstanceId, pub harness: HarnessKind, pub plan_hash: String }
pub struct InstallOutcome { pub record: InstallRecord, pub restart_required: bool, pub repaired: bool }
pub struct InstallRecord { /* …, harness, plan_hash, lock_file_sha256, tool_policy, launch: HarnessLaunch,
    commands: Vec<CommandName>, needs_person_present: bool, reference_only_parts: Vec<String>, … */ }
pub enum VerifyOutcome { Verified, Mismatch { changed: Vec<String>, missing: Vec<String> } }
pub enum InstallError { /* INST_* codes, e.g. RepairFailed { code } */ }
impl InstallError { pub fn retryable(&self) -> bool; }

// ordin-agent-folders — Agent Folder Manager (AgentFolderManager.md)
impl AgentFolderManager {
    pub async fn create(&self, id: AgentInstanceId) -> Result<AgentFolder, AfmError>;
    pub async fn remove(&self, id: AgentInstanceId) -> Result<(), AfmError>;
}
pub struct AgentFolder { pub agent_instance_id: AgentInstanceId, pub folder_path: std::path::PathBuf, pub meta_path: std::path::PathBuf, pub state: AgentFolderState, pub created_at: Timestamp }
pub enum AfmError { /* AFM_* codes */ }

// ordin-package-inspector — Package Inspector (PackageInspector.md)
impl PackageInspector {
    pub async fn inspect(&self, request: &InspectRequest) -> Result<InspectOutcome, InspectError>;
    pub async fn inspect_candidate(&self, request: &InspectRequest) -> Result<PackageModel, InspectError>;
    pub async fn forget(&self, id: AgentInstanceId) -> Result<(), InspectError>;
    pub async fn set_workspaces(&self, id: AgentInstanceId, workspace_ids: Vec<WorkspaceId>) -> Result<(), InspectError>;
}
pub fn may_do(model: &PackageModel) -> MayDo;
pub fn compare_may_do(installed: &MayDo, candidate: &MayDo) -> Vec<MayDoChange>;
pub struct InspectRequest {
    pub agent_instance_id: AgentInstanceId, pub folder_path: std::path::PathBuf, pub bundle_dir: Option<std::path::PathBuf>,
    pub repository_names: std::collections::BTreeMap<String, String>,
    pub part_states: std::collections::BTreeMap<String, PartState>,
    pub install: Option<InstallFacts>,
}
pub struct InstallFacts { pub plugin_name: Option<String>, pub command_names: std::collections::BTreeMap<String, String>,
                          pub reference_only_parts: Vec<String>, pub needs_person_present: bool }
pub enum PartState { InUse, NotForNewAgents, Withdrawn }
pub struct InspectOutcome { pub model: PackageModel, pub parsed_declaration_ref: String, pub may_do: MayDo }
pub struct MayDo { pub tools: Vec<String>, pub reads: Vec<String>, pub connections: Vec<String>,
                   pub tool_policy_digest: Option<String>, pub declaration_digest: Option<String>, pub authority: Authority }
pub struct MayDoChange { pub kind: MayDoChangeKind, pub name: String, pub message_id: &'static str }
pub enum InspectError { /* PKI_* codes, e.g. DeclarationRefused { code, retryable } */ }
impl InspectError { pub fn retryable(&self) -> bool; }

// ordin-journal — Journal and Artefact Store (JournalAndArtefactStore.md; R3)
pub enum Datastore { Sqlite(sqlx::SqlitePool), Postgres(sqlx::PgPool) }
impl Datastore { pub fn sqlite(&self) -> Option<&sqlx::SqlitePool>; }
pub type MigrationStep = (i64, &'static str, &'static str);
pub struct ModuleMigrations { pub module: &'static str, pub sqlite: &'static [MigrationStep], pub postgres: &'static [MigrationStep] }
impl Journal { pub async fn append_entry(&self, entry: NewJournalEntry) -> Result<JournalEntry, JournalError>; }
pub struct NewJournalEntry { pub kind: String, pub component: String, pub run_id: Option<RunId>, pub crew_run_id: Option<CrewRunId>,
                             pub workspace_id: Option<WorkspaceId>, pub summary: String, pub data: serde_json::Value }
```

## Public Interface

```rust
pub struct PackageCatalogue { /* private */ }

/// Owned here; implemented by the program over the Session Supervisor (ADR-100 pattern).
pub trait InstanceActivity: Send + Sync {
    fn running(&self, id: AgentInstanceId) -> bool;
    fn idle(&self) -> tokio::sync::broadcast::Receiver<AgentInstanceId>;   // fired when an instance's last run ends
}

/// Owned here; implemented by the program over the Organisation Profile: `ProfileSnapshot::default_harness()`
/// mapped `HarnessName::ClaudeCode` → `HarnessKind::ClaudeCode`, `HarnessName::Copilot` → `HarnessKind::CopilotCli`
/// (OrganisationProfile.md, consumer table).
pub trait HarnessSettings: Send + Sync { fn default_harness(&self) -> HarnessKind; }

pub struct PackageCatalogueDeps {
    pub datastore: ordin_journal::Datastore,
    pub journal: std::sync::Arc<ordin_journal::Journal>,
    pub catalogue: std::sync::Arc<ordin_catalogue::CatalogueService>,
    pub resolver: std::sync::Arc<ordin_resolver::ResolverAndCompiler>,
    pub checker: std::sync::Arc<ordin_checker::Checker>,
    pub installers: std::sync::Arc<ordin_installers::Installers>,
    pub folders: std::sync::Arc<ordin_agent_folders::AgentFolderManager>,
    pub inspector: std::sync::Arc<ordin_package_inspector::PackageInspector>,
    pub harness: std::sync::Arc<dyn HarnessSettings>,
    pub activity: std::sync::Arc<dyn InstanceActivity>,
}

/// Module `package_catalogue`; empty `postgres` (not in the agent profile). Registered by the program (R3).
pub fn migrations() -> ordin_journal::ModuleMigrations;   // module "package_catalogue" (ModuleMigrations.module)

impl PackageCatalogue {
    /// Bundles are written by the Resolver and Compiler under its own `bundle_root`; this component keeps no staging folder.
    pub fn new(deps: PackageCatalogueDeps) -> Self;
    pub async fn run(&self, shutdown: tokio_util::sync::CancellationToken) -> Result<(), PackageError>;
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<AgentNotice>;

    pub async fn preview_add(&self, input: AddAgentInput) -> Result<AddPreview, PackageError>;
    pub async fn add(&self, input: AddAgentInput) -> Result<AddOutcome, PackageError>;
    pub async fn list(&self, workspace_id: Option<WorkspaceId>) -> Result<Vec<InstalledAgent>, PackageError>;
    pub async fn change_choice(&self, input: ChangeChoiceInput) -> Result<InstalledAgent, PackageError>;
    pub async fn record_answer(&self, input: ChangeChoiceInput) -> Result<InstalledAgent, PackageError>;
    pub async fn accept_update(&self, id: AgentInstanceId) -> Result<InstalledAgent, PackageError>;
    pub async fn decline_update(&self, id: AgentInstanceId) -> Result<InstalledAgent, PackageError>;
    pub async fn remove(&self, input: RemoveAgentInput) -> Result<(), PackageError>;
    pub async fn ensure_ready(&self, id: AgentInstanceId) -> Result<InstalledAgent, PackageError>;
    pub async fn add_trial(&self, input: TrialInput) -> Result<InstalledAgent, PackageError>;
    /// One instance by id, removed instances included (status `removed`).
    pub async fn get(&self, id: AgentInstanceId) -> Result<InstalledAgent, PackageError>;
    /// Verify every `ready` or `updating` instance's folder against its lock file; with `repair`, rebuild a damaged one (SA-INST-01).
    pub async fn verify_agent_folders(&self, repair: bool) -> Result<AgentFolderReport, PackageError>;

    // Ordin Host (interaction X-9 of OrdinHost.md)
    /// Workflow file format 3: is the agent (repository, identifier, version) in this workspace?
    pub async fn match_workflow_agent(&self, workspace_id: WorkspaceId, repository: &AgentRepositoryRef, id: &str, version: &str) -> Result<AgentMatch, PackageError>;
    /// Workflow file formats 1–2: is an agent of this display name in this workspace?
    pub async fn match_by_name(&self, workspace_id: WorkspaceId, name: &str) -> Result<AgentMatch, PackageError>;
    /// (repository, identifier, version, display name) of an installed instance, for writing a workflow file format 3.
    pub async fn agent_identity(&self, agent_instance_id: AgentInstanceId) -> Result<(AgentRepositoryRef, String, String, String), PackageError>;
    /// Library agents that fit a need in words, at most 10, best first, marked when already in this workspace.
    pub async fn find_agents(&self, workspace_id: WorkspaceId, need: &str) -> Result<Vec<AgentFound>, PackageError>;
    /// Confirms an agent may be proposed: an offered, current WorkspaceProfile of that repository.
    pub async fn check_proposal(&self, agent_repository_id: AgentRepositoryId, agent: &str) -> Result<(), PackageError>;
    /// True when the installed agent may use the Mail Service's tools (`mail` server); false for an unknown instance.
    pub async fn can_send_email(&self, agent_instance_id: AgentInstanceId) -> bool;
    /// The pack questions an installed instance's plan left open, lowest `rule_index` first (§N, SA-CLOSE-04).
    pub async fn pending_choices(&self, instance: AgentInstanceId) -> Result<Vec<PendingChoice>, PackageError>;

    // Knowledge Service (agent knowledge of a workspace's agents)
    /// The non-removed instances serving one workspace, ordered by `display_name` then instance id.
    pub async fn agents_in_workspace(&self, id: WorkspaceId) -> Result<Vec<WorkspaceAgent>, PackageError>;

    // The program, after the Checker's `reevaluate_trust` (Checker.md step 32)
    /// Stops each `ready` instance in `changes.lost` (status `refused`, `stopped_by = trust`, `stopped_reason` set);
    /// returns each instance in `changes.gained` that trust loss had stopped to `ready` (SA-CLOSE-02).
    pub async fn apply_trust_changes(&self, changes: &TrustChanges) -> Result<(), PackageError>;

    // The program, filling `checker.check_installed` (Checker.md, Service API commands)
    /// The source scope an installed instance's parts resolve in (SA-CLOSE-03).
    pub async fn source_scope(&self, instance: AgentInstanceId) -> Result<SourceScope, PackageError>;
}
```

Inputs and results (*chosen here*; Service API inputs carry `deny_unknown_fields`):

```rust
pub struct AddAgentInput {
    pub workspace_id: WorkspaceId,
    pub agent: PartId,                        // a WorkspaceProfile part offered by the Catalogue Service
    /// Filled by the program from the Project Service before the call (languages of the workspace's linked code).
    pub workspace_languages: Vec<String>,
    pub interface_locale: String,             // BCP 47, e.g. "en-GB"
    pub choices: Vec<PackChoice>,             // from **Change**; empty on a first add
}
/// A pack choice for one required pack rule of the agent (ADR-089 *Change*, decision 64).
pub struct PackChoice {
    pub rule_index: u32,                      // 0-based index in the agent's requiredPracticePacks (Resolver `PackAnswer`)
    pub packs: Vec<String>,                   // pack part identifiers
}
/// One pack or overlay a plan picked, for "Uses: …" (derived from `ResolutionPlan.selections`).
pub struct UsesItem {
    pub kind: PartKind,
    pub identifier: String,
    pub label: String,                        // the part's name from the Catalogue Service
    pub chosen_by: ChosenBy,
    pub changeable: bool,                     // false for `MakerFixed`; true otherwise
}
pub struct AddPreview { pub agent_name: String, pub from: String, pub uses: Vec<UsesItem>, pub asks: Vec<PendingPackChoice>, pub available: Availability }
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Availability { Available, PackMissing { pack_class_label: String }, NeedsItApproval { parts: Vec<String> }, Unsafe }
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AddOutcome {
    Ready { agent: InstalledAgent },
    Unsafe,                                   // message agent-add-unsafe, action send_to_it
    NeedsItApproval { parts: Vec<String> },   // message agent-add-needs-it, action Ask IT (send_to_it)
    PackMissing { pack_class_label: String },
}
pub struct InstalledAgent {
    pub instance: AgentInstance,
    pub workspaces: Vec<WorkspaceId>,
    pub tasks: Vec<String>,
    pub uses: Vec<UsesItem>,
    pub update: Option<PendingUpdate>,        // the stopped reason is `instance.stopped_reason`
}
pub struct PendingUpdate { pub to_commit: String, pub may_do_changes: Vec<MayDoChange> }   // the Package Inspector's plain-words differences
pub struct ChangeChoiceInput { pub agent_instance_id: AgentInstanceId, pub choice: PackChoice }
/// One open pack question of an installed instance (decision 64; SA-CLOSE-04). Answered with `record_answer`
/// as `ChangeChoiceInput { agent_instance_id, choice: PackChoice { rule_index, packs } }`, `packs` taken from `choices`.
pub struct PendingChoice {
    pub agent_instance_id: AgentInstanceId,   // → ChangeChoiceInput.agent_instance_id
    pub rule_index: u32,                      // → PackChoice.rule_index
    pub at_least: u32,                        // how many packs PackChoice.packs must hold
    pub class_label: String,                  // the rule's class; "pack" when the rule names none
    pub choices: Vec<PendingChoiceOption>,    // the rule's candidates, sorted by identifier
}
pub struct PendingChoiceOption {
    pub pack: String,                         // pack part identifier → an element of PackChoice.packs
    pub label: String,                        // the pack's name from the Catalogue Service; the identifier when the lookup fails
}
pub struct RemoveAgentInput { pub agent_instance_id: AgentInstanceId, pub workspace_id: Option<WorkspaceId> }
pub enum AgentMatch {
    Present { agent_instance_id: AgentInstanceId, display_name: String },
    OtherVersion { agent_instance_id: AgentInstanceId, display_name: String, installed_version: String },
    Missing,
}
pub struct AgentFound {
    pub agent: PartId,
    pub agent_repository_id: AgentRepositoryId,
    pub identifier: String,
    pub name: String,
    pub summary: String,                      // one sentence, from the Catalogue Service's PartView
    pub from: String,                         // repository display name or "ordin"
    pub in_workspace: Option<AgentInstanceId>,
}
pub struct AgentFolderReport { pub checked: u32, pub repaired: u32, pub failed: Vec<AgentFolderFailure> }
pub struct AgentFolderFailure { pub agent_name: String, pub code: String /* PKG_FOLDER_UNUSABLE or PKG_FOLDER_MISMATCH */ }
/// Knowledge Service's view of one agent of a workspace (KnowledgeService.md).
pub struct WorkspaceAgent { pub agent_instance_id: AgentInstanceId, pub display_name: String }

pub struct TrialInput { pub agent_draft_id: AgentDraftId, pub agent_repository_id: AgentRepositoryId, pub worktree: std::path::PathBuf, pub agent: String /* identifier */, pub workspace_id: WorkspaceId, pub workspace_languages: Vec<String>, pub interface_locale: String }

#[derive(Debug, Clone)]
pub enum AgentNotice {
    Added { agent_instance_id: AgentInstanceId, workspace_id: WorkspaceId },
    UpdatedBetweenRuns { agent_instance_id: AgentInstanceId },
    UpdateNeedsChoice { agent_instance_id: AgentInstanceId },         // an entry in Needs you
    Stopped { agent_instance_id: AgentInstanceId, reason: String },   // withdrawn or trust lost
    /// A refused instance is ready again (trust regained, or a fixed version accepted; SA-CLOSE-02).
    Resumed { agent_instance_id: AgentInstanceId },
    Removed { agent_instance_id: AgentInstanceId },
}

#[derive(Debug, thiserror::Error)]
#[error("{code}")]
pub struct PackageError { pub code: &'static str, pub retryable: bool, pub action: Option<RepairAction>, pub detail: String }
```

Service API commands. Under the reconciliation ruling that each module owns its command names (SA-RECON, R1), this list is authoritative for the Service API catalogue:

| Command | Input | Result | Called by |
|---|---|---|---|
| `agents.preview_add` | `AddAgentInput` | `AddPreview` | Screens (agent library, **Add it**) |
| `agents.add` | `AddAgentInput` | `AddOutcome` | Screens |
| `agents.list` | `{ "workspace_id": Option<WorkspaceId> }` | `Vec<InstalledAgent>` | Screens |
| `agents.get` | `{ "agent_instance_id": AgentInstanceId }` | `InstalledAgent` | Screens (agent page) |
| `agents.change_choice` | `ChangeChoiceInput` | `InstalledAgent` | Screens (**Change**) |
| `agents.accept_update` | `{ "agent_instance_id": AgentInstanceId }` | `InstalledAgent` | Screens (Needs you) |
| `agents.decline_update` | `{ "agent_instance_id": AgentInstanceId }` | `InstalledAgent` | Screens (Needs you) |
| `agents.remove` | `RemoveAgentInput` | `Done` | Screens (after the confirmation) |

The Service API fills `AddAgentInput.workspace_languages` from the Project Service before the call (the screens send the other fields); `agents.add` returns `AddOutcome`, never a bare instance.

In-process methods and their callers:

| Method | Called by |
|---|---|
| `run`, `migrations`, `new` | the program |
| `subscribe` | the program (maps `AgentNotice` to Service API events, below) |
| `record_answer` | Ordin Host (the agent's pack question, decision 64; OrdinHost.md step 8.3 (a0) through step 38, X-9) |
| `pending_choices` | Ordin Host (before a delegated task starts on an instance; OrdinHost.md step 8.3 (a0), X-9) |
| `ensure_ready` | Ordin Host, before a session starts on an instance |
| `add_trial` | Ordin Host (`AuthoringHost::add_trial` for the Authoring Service); it fills `workspace_languages` and `interface_locale` and maps a `PackageError` to `OhError` `OH_DEPENDENCY_FAILED` carrying the `PKG_*` code (OrdinHost.md) |
| `verify_agent_folders` | Setup Assistant (AgentFolders step) |
| `match_workflow_agent`, `match_by_name`, `agent_identity`, `find_agents`, `check_proposal`, `can_send_email` | Ordin Host (X-9) |
| `list` (in-process) | the program's `SearchSource` for Home and Search, which offers installed agents by display name |
| `agents_in_workspace` | Knowledge Service (agent knowledge of a workspace's agents) |
| `apply_trust_changes` | the program, with the `TrustChanges` the Checker's `reevaluate_trust` returns on each new verified profile |
| `source_scope` | the program, filling the `scope` argument of the Checker's `checker.check_installed` command (Checker.md) |

**Events the program raises from `AgentNotice`** (the Package Catalogue owns no Service API event of its own): `Added`, `UpdatedBetweenRuns`, `Stopped`, `Resumed` and `Removed` → `workspace_changed` for each workspace the instance serves; `UpdateNeedsChoice` → `needs_you_changed` (the pending update is an entry in Needs you until accepted or declined); `Removed` → the program also calls the Workspace Service's `revoke_grantee(Grantee::AgentInstance { agent_instance_id })` so the removed instance keeps no folder or email grant.

### Example

`agents.add` input `{"workspace_id":"0192a0f1-1111-7222-8333-444455556666","agent":"0192a2c0-0000-7000-8000-0000000000aa","workspace_languages":["go"],"interface_locale":"en-GB","choices":[]}` → `{"kind":"ready","agent":{"instance":{…"display_name":"Release planner","status":"ready"…},"workspaces":["0192a0f1-…"],"tasks":["Plan the release"],"uses":[{"kind":"PracticePack","label":"Go",…},{"kind":"PracticePack","label":"Test pyramid",…}],"update":null,"stopped_reason":null}}`.

## Internal Logic

**A0. Building the requests (used by every section below).**
1. **Scope:** `catalogue.source_scope(RepositoryRef::Repository(agent_repository_id))` (CatalogueService.md); a bundled agent uses `RepositoryRef::Bundled`.
2. **`ResolveRequest`:** `scope`; `root_id` = the agent part's `identifier`; `root_version` = `Some(part.version)` of the `PartView` read in §A1 (for an update, `None`: the head version); `target_harness` = the instance's `HarnessKind`, passed unchanged (the harness is `harness.default_harness()` at first add and the install record's `harness` afterwards); `workspace_languages` and `interface_language` from the input; `pack_answers` = the stored choices with source `agent_asked` as `PackAnswer { rule_index, pack_ids: packs }`; `pack_changes` = the union of the packs of the stored choices with source `person_change`; `overlay_changes` = empty; `replacements` = empty except in §D; `staged` = empty; `allow_draft_root = false` (true only in §G); `source = None` (the bound catalogue; `Some(SourceOverride(draft source))` only in §G).
3. **`ResolveError` mapping:** `RequiredPackUnsettled { class, .. }` → `PackMissing { pack_class_label: class or "pack" }`; `PartWithdrawn` → `PKG_AGENT_NOT_OFFERED`; any other error with `retryable()` → `PKG_CHECK_UNAVAILABLE`; any other permanent error → outcome `Unsafe` (journal entry `agent_add_refused` with the error code).
4. **Uses and asks:** `uses` = one `UsesItem` per `plan.selections.practice_packs` and `plan.selections.overlays` entry, kind from the matching `PartRef` of `plan.resolved`, `label` = `catalogue.find_part(...)`'s `part.name` (the identifier when the lookup fails), `changeable = chosen_by != MakerFixed`; `asks` = `plan.selections.pending_choices` (decision 64: left for the agent to ask).
5. **`InspectRequest`:** `folder_path` = the agent folder; `bundle_dir = Some(bundle.dir)`; `repository_names` = `"bundled"` → `"ordin"` plus, for every `RepositorySummary` of `catalogue.list_repositories()`, `"<host>/<repository>"` → `display_name`; `part_states` = for each `PartRef` of `plan.resolved`, the `state_label` of its `catalogue.find_part` (`Current`/`NotOffered` → `InUse`, `NotForNewAgents` → `NotForNewAgents`, `Withdrawn` → `Withdrawn`); `install = Some(InstallFacts)` from `installers.record(id)`: `plugin_name` from `HarnessLaunch::ClaudeCode.plugin_name` (None for Copilot), `command_names` from `commands` (`command_id` → `session_name`), `reference_only_parts`, `needs_person_present`.
6. **Journal entries:** `journal.append_entry(NewJournalEntry { kind, component: "package_catalogue", run_id: None, crew_run_id: None, workspace_id, summary, data })`, kinds `agent_added`, `agent_add_refused`, `agent_update_refused`, `agent_repaired`, `agent_stopped`, `agent_resumed`, `agent_removed` (*chosen here*). A journal failure is logged `agent.journal_failed` at `warn` and does not undo the step.

**A. Preview and add.**
1. `catalogue.part(agent)`: `CAT_PART_UNKNOWN` → `PKG_AGENT_UNKNOWN`; not a WorkspaceProfile → `PKG_NOT_AN_AGENT`; `offered = false` or state `Withdrawn` → `PKG_AGENT_NOT_OFFERED`; state `NotForNewAgents` → `PKG_DEPRECATED_AS_AGENT` (2C rule; the code is this component's, EH-D6); another `CatalogueError` → `PKG_LIBRARY_UNAVAILABLE`.
2. An instance of the same agent identifier already in this workspace, not `removed` → `PKG_ALREADY_ADDED`.
3. `resolver.resolve(&request)` (§A0.2), errors per §A0.3.
4. `checker.check_for_install(&InstallCheck { plan, scope, agent_instance_id, source: None })` — `agent_instance_id` is `None` for `preview_add`; for `add` it is `Some(id)` with `id` a UUIDv7 minted here and used for the row inserted in step 6 (SA-CLOSE-01): `Failed` → `Unsafe` (journal `agent_add_refused` with the report's `check_run_id`; nothing created); `NeedsItApproval { parts }` → `NeedsItApproval { parts: part_name of each }` (nothing created); `CheckError` → `PKG_CHECK_UNAVAILABLE` (retryable).
5. `preview_add` stops here and returns `AddPreview` with `uses` and `asks` (§A0.4). `add` continues.
6. Insert `agent_instances` (`installing`, `display_name` from the part's name, `package_name` = the identifier's slug, `package_version` = the part's version, `agent_repository_id`, `commit`, `is_trial_of_own_draft = false`, `authority = document_only` placeholder, `harness` = `harness.default_harness()`) and the choices.
7. `folders.create(id)` → `AgentFolder` (its `folder_path` is `agent_folder_path`); `resolver.build(&request)` → `Bundle`; `installers.install(&InstallRequest { agent_instance_id: id, harness, plan_hash: bundle.plan.plan_hash })` → `InstallOutcome` (its `record` is kept by the Installers and read back through `record(id)`).
8. `inspector.inspect(&InspectRequest)` (§A0.5) → `InspectOutcome`: set `authority` = `model.authority`, `parsed_declaration_ref`, `lock_file_path` = `<folder_path>/ordin.lock`, `may_do` = JSON of `outcome.may_do`, `audit_result = passed`, `plan_hash` = the bundle's; `pending_choices` = JSON of the bundle plan's `selections.pending_choices` (SA-CLOSE-04; every step that repeats §A8, §B and §C included, writes both together); `parts = checker.executable_parts(id)` (the complete list stored by step 4's check, SA-CLOSE-01) then `checker.decide_executable_parts(id, &parts)` — a `CheckError` from either counts as a step-8 failure (§A10); `status = ready`; insert the workspace membership; `inspector.set_workspaces(id, memberships)`; journal `agent_added` with the lock file digest, the check run id and the trust outcome (03 *Add an agent*); `resolver.prune(live plan hashes)` (§A9); notice `Added`.
9. **Prune:** after every install, update and removal, `resolver.prune(keep)` with `keep` = the `plan_hash` of every instance not `removed` plus every pending update's candidate plan hash; a `ResolveError` is logged `agent.prune_failed` at `warn` and never fails the step.
10. A failure in step 7 or 8: `installers.uninstall(id)` when step 7 reached the installer, `folders.remove(id)`, and retry steps 7–8 once (03 error posture); a second failure → `status = refused`, `audit_result = failed`, folder removed, `PKG_INSTALL_FAILED` (retryable by a new **Add it**). `InspectError::DeclarationRefused` counts as such a failure (the Package Inspector holds no instance status; the Package Catalogue sets it).

**B. Change and answer (ADR-089 *Change*, decision 64).** `change_choice` / `record_answer`: `choice.rule_index` must name a required pack rule of the agent and every pack must be one of its candidates → a `ResolveError::PackNotAllowed` from the re-plan maps to `PKG_CHOICE_NOT_ALLOWED`; store the choice (`person_change` for `change_choice`, `agent_asked` for `record_answer`); re-plan (§A0.2), re-check (§A4 with `agent_instance_id = Some(id)`), `resolver.build`; then:
- `record_answer`, and `change_choice` while `activity.running(id)` is true: `installers.update_in_session(&request)` (C-64: the change takes effect at the next turn); `restart_required = true` in the outcome → the instance's notice `UpdatedBetweenRuns` is not sent and the Ordin Host resumes the session (C-64 fallback, owned by the Session Supervisor).
- `change_choice` with no running session: `installers.update(&request)`.
- After either: re-inspect (§A8, without the membership insert), `ready`, prune (§A9).

**C. Updates (A-16).** On `CatalogueNotice::RepositoryUpdated` for a repository with `ready` instances:
1. For each instance, `resolver.resolve` at the head (§A0.2 with `root_version = None`); if `plan.plan_hash` equals the instance's stored `plan_hash`, nothing to do.
2. `resolver.build(&request)` → candidate `Bundle`; `checker.check_for_install` with `agent_instance_id = Some(id)`: `Failed` → keep the installed version and journal `agent_update_refused`; `NeedsItApproval` → `update_awaiting_choice` with the parts listed.
3. `inspector.inspect_candidate(&InspectRequest)` with `bundle_dir = Some(candidate.dir)` and the installed folder's `install` facts → candidate `PackageModel`; `changes = compare_may_do(&installed_may_do, &may_do(&candidate_model))`, where `installed_may_do` is the stored `may_do`. Empty → silent update: wait until `activity.running(id)` is false (subscribe to `activity.idle()`), `status = updating`, `installers.update(&InstallRequest { plan_hash: candidate.plan.plan_hash, .. })`, re-inspect (§A8), `ready`, `updated_between_runs_at = now`, notice `UpdatedBetweenRuns`. Not empty → `update_awaiting_choice`, store `PendingUpdate { to_commit, may_do_changes: changes }` and the candidate plan hash, notice `UpdateNeedsChoice`.
4. `accept_update` (`update_awaiting_choice`, or `refused` with a pending update per step 5; else `PKG_NO_UPDATE`) → between runs as step 3's silent path, then `parts = checker.executable_parts(id)` and `checker.decide_executable_parts(id, &parts)` for the new version; `decline_update` → `ready` (or stays `refused` for step 5's instance), the pending row deleted, prune (§A9).
5. **Fixed version for a withdrawn agent** (SA-CLOSE-02 b). On `RepositoryUpdated` for the repository of a `refused` instance with `stopped_by = withdrawal`: re-plan at the head (§A0.2, `root_version = None`); a `ResolveError` (the part is still withdrawn, or no replacement resolves) → nothing changes. Otherwise build, `check_for_install` with `Some(id)`: `Failed` or `NeedsItApproval` → nothing changes; `Clean` → store `PendingUpdate { to_commit, may_do_changes: compare_may_do(installed, candidate) }` and the candidate plan hash, notice `UpdateNeedsChoice` — the status stays `refused`. `accept_update` then installs the candidate (`installers.update`), re-inspects (§A8 without the membership insert), decides its executable parts, and sets `status = ready`, `stopped_reason = NULL`, `stopped_by = NULL` (2C `refused → ready`), notice `Resumed`, journal `agent_resumed`.

**D. Withdrawal (ADR-082, ADR-090 §6).** For each newly withdrawn entry, the instances whose lock file names the part (and version, when the entry names one):
1. A replacement is named: re-plan with `replacements = [Replacement { withdrawn_id, withdrawn_kind, replacement_id, replacement_version }]`; check; passes → treat as an update (§C3, A-16 rules); fails → step 2.
2. No replacement, or it fails: `status = refused`, `stopped_by = withdrawal`, `stopped_reason` = "[agent] stopped: [ability] was withdrawn by [repository]." (2C C-22); notice `Stopped`; journal `agent_stopped`. The instance's results stay where they are; its folder is kept until the person removes it.
3. `CatalogueNotice::RepositoryDropped`: instances from that repository keep running from their folders (R-72: clones and folders work offline); no update is offered from it again.

**E. Remove.** `remove` with `workspace_id`: drop that membership and `inspector.set_workspaces(id, remaining)`; when no membership remains, or with no `workspace_id`: `installers.uninstall(id)` (restores the folder as before, 02:455), `inspector.forget(id)`, `checker.forget_instance(id)`, `folders.remove(id)`, `status = removed`, prune (§A9), journal `agent_removed`, notice `Removed`. An error from `forget` or `forget_instance` is logged `agent.forget_failed` at `warn` and does not stop the removal. The confirmation is the screen's (11 §Confirmations, **Remove [agent]…**). Not found or already removed → `PKG_INSTANCE_UNKNOWN`.

**F. Before a session (SA-INST-01).** `ensure_ready(id)`: status must be `ready` or `updating` → else `PKG_INSTANCE_NOT_READY`; `installers.verify_and_repair(id, &scope)` (§A0.1) — `Ok` with `repaired = true` → journal `agent_repaired`; `Err(InstallError::RepairFailed { .. })` or any other permanent `InstallError` → `PKG_FOLDER_UNUSABLE` with action `send_to_it` (the agent is shown as unavailable); a retryable `InstallError` → `PKG_INSTALL_FAILED`.

**G. Trial of the person's own draft (ADR-083 *Trying it*).** `add_trial`: `source = catalogue.draft_source(repository, worktree)`; plan and check as §A3–A4 with `scope = catalogue.source_scope(RepositoryRef::Repository(agent_repository_id))`, `root_id = input.agent`, `root_version = None`, `ResolveRequest.source = Some(SourceOverride(source))`, `allow_draft_root = true` and `InstallCheck.source` the same — the draft's needs must already be inside the trust policy (`NeedsItApproval` → `PKG_TRIAL_NEEDS_TRUST`); build, install into a new folder as §A7–A8; `is_trial_of_own_draft = true`, `agent_repository_id` and `commit` absent (the draft is not merged); membership only in `workspace_id`. A second trial of the same draft replaces the first (§E, then add).

**H. Read one instance (`get`).** Select the `agent_instances` row with its memberships, choices and pending update; unknown → `PKG_INSTANCE_UNKNOWN`. A `removed` instance is returned with its status (the agent page of a removed agent shows it as removed).

**I. Verify every folder (`verify_agent_folders`).**
1. Select instances with status `ready` or `updating`; `checked` = their count.
2. For each, one at a time: `repair = false` → the Installers' `verify(id)` (Installers.md): `Verified` → next; `Mismatch` → a failure with code `PKG_FOLDER_MISMATCH`, folder untouched. `repair = true` → as §F (`ensure_ready`): rebuilt → `repaired += 1`, journal `agent.repaired`; not rebuildable → a failure with code `PKG_FOLDER_UNUSABLE`.
3. An installer error on one instance is that instance's failure; the loop moves to the next instance. The call fails only on a datastore read error → `PKG_STORE_FAILED`.
4. `agent_name` in a failure is the instance's `display_name`.

**J. Ordin Host lookups (X-9).**
1. `match_workflow_agent(workspace_id, repository, id, version)`: `catalogue.repository_by_ref(repository)`; `None` → `Missing`. Select the non-removed instance serving `workspace_id` whose `agent_repository_id` is that repository and whose `agent_identifier` equals `id`; none → `Missing`; its `package_version` equals `version` → `Present`; otherwise `OtherVersion` with the installed version.
2. `match_by_name(workspace_id, name)`: non-removed instances serving `workspace_id` whose `display_name` equals `name` compared case-insensitively after trimming; none → `Missing`; one → `Present`; more than one → `PKG_AGENT_NAME_AMBIGUOUS`.
3. `agent_identity(agent_instance_id)`: unknown or removed → `PKG_INSTANCE_UNKNOWN`; a trial of an own draft (no repository) → `PKG_TRIAL_NOT_NAMEABLE`; else `(catalogue.repository_ref(agent_repository_id), agent_identifier, package_version, display_name)`.
4. `find_agents(workspace_id, need)`: `need` trimmed, 1–500 characters → else `PKG_NEED_INVALID`; `catalogue.search(LibraryQuery { text: Some(need), kinds: [], repositories: [], area: None, audience: None, publisher: None, include_not_for_new: false, cursor: None })`; take the first 10 entries; for each, `in_workspace` = the non-removed instance serving `workspace_id` with that repository and identifier. A `CatalogueError` → `PKG_LIBRARY_UNAVAILABLE` (retryable).
5. `check_proposal(agent_repository_id, agent)`: `catalogue.find_part(agent_repository_id, WorkspaceProfile, agent, None)`; `CAT_PART_UNKNOWN` or `CAT_REPOSITORY_UNKNOWN` → `PKG_AGENT_UNKNOWN`; `offered = false` or state `withdrawn` → `PKG_AGENT_NOT_OFFERED`; state `not_for_new_agents` → `PKG_DEPRECATED_AS_AGENT`; otherwise `Ok(())`.
6. `can_send_email(agent_instance_id)`: read the instance's stored `may_do` (written at §A8 and §C3 from the Package Inspector's summary); true when its `connections` contain `mail` (the Mail Service's server name, MailService.md); unknown instance, removed instance or unreadable value → false, logged `agent.may_do_unreadable` at `warn`.

**M. Source scope of an instance (`source_scope`, SA-CLOSE-03).**
1. Read the `agent_instances` row; unknown or `removed` → `PKG_INSTANCE_UNKNOWN`; `is_trial_of_own_draft = true` → `PKG_TRIAL_NO_SCOPE`.
2. `home` = `RepositoryRef::Repository(agent_repository_id)` when the row has a repository, else `RepositoryRef::Bundled`; return `catalogue.source_scope(home)`. A `CatalogueError` → `PKG_LIBRARY_UNAVAILABLE` with the error's `retryable` flag; a datastore error → `PKG_STORE_FAILED`.

**N. Open pack questions of an instance (`pending_choices`, SA-CLOSE-04).** Called by the Ordin Host before a delegated task starts (OrdinHost.md step 8.3 (a0)).
1. Read the `agent_instances` row; unknown or `removed` → `PKG_INSTANCE_UNKNOWN`; status not `ready` or `updating` → `PKG_INSTANCE_NOT_READY`.
2. Parse `pending_choices` as `Vec<PendingPackChoice>`; a value that does not parse → `PKG_STORE_FAILED` (permanent; logged `agent.pending_choices_unreadable` at `warn`). Drop each entry whose `rule_index` has an `agent_instance_choices` row for this instance (answered or changed since the column was written).
3. For each remaining entry, ordered by `rule_index`: `PendingChoice { agent_instance_id: instance, rule_index, at_least, class_label: class or "pack", choices }`, one `PendingChoiceOption` per identifier of `choices` in its stored (sorted) order, `label` per §A0.4 (`catalogue.find_part(...)`'s `part.name`, the identifier when the lookup fails). None remaining → empty list.
4. A datastore error → `PKG_STORE_FAILED` (retryable).

**K. Agents of a workspace (`agents_in_workspace`).** Called by the Knowledge Service (agent knowledge of the workspace's agents, KnowledgeService.md).
1. Select instances joined with `agent_instance_workspaces` where `workspace_id = id` and `status <> 'removed'`, ordered by `display_name`, then `id`.
2. Return one `WorkspaceAgent { agent_instance_id, display_name }` per row; a workspace with none → empty list (an unknown workspace id is not an error: this component does not own workspaces).
3. A datastore error → `PKG_STORE_FAILED` (retryable).

**L. Trust changes (`apply_trust_changes`).** Called by the program right after `Checker::reevaluate_trust` returns (Checker.md step 32; 04:52).
1. For each id in `changes.lost` whose status is `ready`, `updating` or `update_awaiting_choice`: in one transaction set `status = refused`, `stopped_by = trust`, `stopped_reason` = the en-GB text of `agent-stopped-trust-lost` with the display name, delete any pending update; notice `Stopped { reason }`; journal `agent_stopped`. Instances in another status, or unknown, are skipped and logged `agent.trust_change_skipped`.
2. A running session is not ended here: the program raises `workspace_changed` from `Stopped`, and the next `ensure_ready` fails with `PKG_INSTANCE_NOT_READY`.
3. Each id in `changes.gained` whose status is `refused` with `stopped_by = trust` (SA-CLOSE-02 a): `installers.verify(id)` — `Verified` → in one transaction set `status = ready`, `stopped_reason = NULL`, `stopped_by = NULL` (2C `refused → ready`); notice `Resumed`; journal `agent_resumed`; logged `agent.trust_gained` at `info`. `Mismatch` or an `InstallError` → the instance stays `refused` and is logged `agent.trust_gained_folder_unusable` at `warn` (the person removes and adds it again). An instance in any other status, or stopped by a withdrawal, is logged `agent.trust_gained` and not changed.
4. A datastore error → `PKG_STORE_FAILED` (retryable); instances already updated stay updated.

## Data Model

Module `package_catalogue`, journal database, migrations exported by `migrations()` as `MigrationStep` tuples (R3). Not in the agent profile, so the `postgres` slice is empty.

```sql
-- package_catalogue migration 1
CREATE TABLE agent_instances (
  id TEXT PRIMARY KEY,
  display_name TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 120),
  package_name TEXT NOT NULL, package_version TEXT NOT NULL,
  agent_repository_id TEXT, "commit" TEXT,
  agent_folder_path TEXT NOT NULL UNIQUE,
  lock_file_path TEXT NOT NULL,
  audit_result TEXT CHECK (audit_result IN ('passed','failed')),
  authority TEXT NOT NULL CHECK (authority IN ('document_only','system_changing','routing_only')),
  parsed_declaration_ref TEXT NOT NULL,
  is_trial_of_own_draft INTEGER NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('installing','ready','refused','updating','update_awaiting_choice','removed')),
  updated_between_runs_at TEXT,
  installed_at TEXT NOT NULL,
  agent_identifier TEXT NOT NULL,           -- chosen here: the WorkspaceProfile identifier
  agent_draft_id TEXT,                      -- chosen here: trials only
  stopped_reason TEXT,                      -- chosen here: withdrawal reason in plain words
  stopped_by TEXT CHECK (stopped_by IS NULL OR stopped_by IN ('withdrawal','trust')),   -- SA-CLOSE-02
  may_do TEXT,                              -- reconciliation: JSON of the installed MayDo; read by can_send_email and §C3
  plan_hash TEXT,                           -- reconciliation: sha256:<hex> of the installed bundle (§A9, §C1)
  pending_choices TEXT NOT NULL DEFAULT '[]',   -- SA-CLOSE-04: JSON Vec<PendingPackChoice> of the installed plan
  harness TEXT NOT NULL CHECK (harness IN ('claude_code','copilot_cli')),   -- reconciliation: 2C HarnessKind
  CHECK ((agent_repository_id IS NULL) = ("commit" IS NULL)),
  CHECK (status <> 'refused' OR audit_result = 'failed' OR stopped_reason IS NOT NULL),  -- 2C C-22
  CHECK ((stopped_by IS NULL) = (stopped_reason IS NULL))
);

CREATE TABLE agent_instance_workspaces (    -- chosen here: which workspaces an instance serves
  agent_instance_id TEXT NOT NULL REFERENCES agent_instances(id) ON DELETE CASCADE,
  workspace_id TEXT NOT NULL,
  added_at TEXT NOT NULL,
  PRIMARY KEY (agent_instance_id, workspace_id)
);
CREATE INDEX ix_aiw_workspace ON agent_instance_workspaces(workspace_id);

CREATE TABLE agent_instance_choices (       -- chosen here: pack choices and answers (decision 64)
  agent_instance_id TEXT NOT NULL REFERENCES agent_instances(id) ON DELETE CASCADE,
  rule_index INTEGER NOT NULL CHECK (rule_index >= 0),   -- reconciliation: Resolver PackAnswer.rule_index
  packs TEXT NOT NULL,                      -- JSON Vec<String>
  source TEXT NOT NULL CHECK (source IN ('person_change','agent_asked')),
  chosen_at TEXT NOT NULL,
  PRIMARY KEY (agent_instance_id, rule_index)
);

CREATE TABLE agent_pending_updates (        -- chosen here
  agent_instance_id TEXT PRIMARY KEY REFERENCES agent_instances(id) ON DELETE CASCADE,
  to_commit TEXT NOT NULL, may_do_changes TEXT NOT NULL,   -- JSON Vec<MayDoChange>
  candidate_plan_hash TEXT NOT NULL,        -- reconciliation: the candidate bundle under the Resolver's bundle_root
  found_at TEXT NOT NULL
);
```

Rollback: forward-only; no destructive change. `pending_choices` is part of migration 1 (nothing is installed before the first release, so no later `ALTER` is needed).

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Agent part unknown | `Err(permanent)` | `PKG_AGENT_UNKNOWN` | `{"error":"That agent is no longer in the library.","code":"PKG_AGENT_UNKNOWN","action":null}` |
| Part is not an agent | `Err(permanent)` | `PKG_NOT_AN_AGENT` | in-process / screens defect |
| Agent not offered (failed checks, withdrawn) | `Err(permanent)` | `PKG_AGENT_NOT_OFFERED` | `{"error":"This agent can't be added.","code":"PKG_AGENT_NOT_OFFERED","action":"send_to_it"}` |
| Deprecated part added as an agent | `Err(permanent)` | `PKG_DEPRECATED_AS_AGENT` | `{"error":"This agent is no longer offered for new work.","code":"PKG_DEPRECATED_AS_AGENT","action":null}` |
| Already added to this workspace | `Err(permanent)` | `PKG_ALREADY_ADDED` | `{"error":"This agent is already in this workspace.","code":"PKG_ALREADY_ADDED","action":null}` |
| Checks fail | outcome `Unsafe` | `PKG_UNSAFE` | message `agent-add-unsafe`, action `send_to_it` |
| Outside the trust policy | outcome `NeedsItApproval` | `PKG_NEEDS_IT_APPROVAL` | message `agent-add-needs-it`, action `send_to_it` |
| Workspace cannot settle a required pack | outcome `PackMissing` | `PKG_PACK_MISSING` | message `agent-unavailable-pack-missing` |
| Checker unavailable | `Err(retryable)` | `PKG_CHECK_UNAVAILABLE` | `{"error":"The agent couldn't be checked right now.","code":"PKG_CHECK_UNAVAILABLE","action":"try_again"}` |
| Build, install or inspection failed twice | `Err(retryable)` | `PKG_INSTALL_FAILED` | `{"error":"The agent couldn't be added. Try again.","code":"PKG_INSTALL_FAILED","action":"try_again"}` |
| Choice outside what the agent allows | `Err(permanent)` | `PKG_CHOICE_NOT_ALLOWED` | in-process / screens |
| No update waiting | `Err(permanent)` | `PKG_NO_UPDATE` | `{"error":"There's no update waiting for this agent.","code":"PKG_NO_UPDATE","action":null}` |
| Instance unknown or removed | `Err(permanent)` | `PKG_INSTANCE_UNKNOWN` | `{"error":"That agent is no longer on this computer.","code":"PKG_INSTANCE_UNKNOWN","action":null}` |
| Instance not ready for a session, or `pending_choices` asked for an instance not `ready`/`updating` | `Err(permanent)` | `PKG_INSTANCE_NOT_READY` | in-process |
| Folder cannot be verified or rebuilt | `Err(permanent)` | `PKG_FOLDER_UNUSABLE` | `{"error":"This agent can't be used right now.","code":"PKG_FOLDER_UNUSABLE","action":"send_to_it"}` |
| Trial needs IT approval | `Err(permanent)` | `PKG_TRIAL_NEEDS_TRUST` | in-process (draft screen shows the check) |
| Folder differs from its lock file (`verify_agent_folders` without repair) | failure line in `AgentFolderReport` | `PKG_FOLDER_MISMATCH` | Setup Assistant shows it |
| Two instances in the workspace share the name a format 1–2 workflow cites | `Err(permanent)` | `PKG_AGENT_NAME_AMBIGUOUS` | in-process (Ordin Host reports a workflow check problem) |
| Identity asked for a trial of an own draft | `Err(permanent)` | `PKG_TRIAL_NOT_NAMEABLE` | in-process |
| `source_scope` asked for a trial of an own draft | `Err(permanent)` | `PKG_TRIAL_NO_SCOPE` | `{"error":"Check your draft from the draft page.","code":"PKG_TRIAL_NO_SCOPE","action":null}` |
| Trust regained but the folder no longer matches its lock file | instance stays `refused`, logged `agent.trust_gained_folder_unusable` | — | none |
| `find_agents` need empty or over 500 characters | `Err(permanent)` | `PKG_NEED_INVALID` | in-process |
| Catalogue Service search failed | `Err(retryable)` | `PKG_LIBRARY_UNAVAILABLE` | in-process (Ordin Host answers "That did not work; try again.") |
| `apply_trust_changes` names an instance that is not running or unknown | skipped, logged `agent.trust_change_skipped` | — | none |
| Install record unreadable after install (`installers.record`) | `Err(retryable)` per §A10 | `PKG_INSTALL_FAILED` | as above |
| Stored `pending_choices` value unreadable (§N.2) | `Err(permanent)` | `PKG_STORE_FAILED` | in-process (the Ordin Host reports the code; no task starts) |
| Datastore failure | `Err(retryable)` | `PKG_STORE_FAILED` | `{"error":"Something went wrong saving the agent.","code":"PKG_STORE_FAILED","action":"try_again"}` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Package Catalogue

  Scenario: Happy path — add a clean, trusted agent
    Given an offered agent whose parts pass and whose tool bindings are inside the trust policy
    When the person presses Add it in a workspace that links Go code
    Then an instance moves from installing to ready
    And its uses list Go picked from the workspace
    And the journal records the install with its lock file and check results

  Scenario: Agent needs IT approval
    Given an agent whose hook is outside the trust policy
    When the person presses Add it
    Then the outcome is needs_it_approval and no instance or folder is created

  Scenario: Agent fails its checks
    Given an agent with a part that fails the Checker
    When the person presses Add it
    Then the outcome is unsafe and nothing is created

  Scenario: Required pack the workspace cannot settle
    Given an agent whose testing pack rule has no maker's fixed pack and is not left to ask
    And the workspace links no code
    When the person previews adding it
    Then availability is pack_missing naming the kind of pack

  Scenario: Interrupted install retried once
    Given the Installer fails on the first attempt and succeeds on the second
    When the person adds the agent
    Then the first folder is removed and the agent ends ready

  Scenario: Silent update between runs
    Given a ready instance and a new commit whose candidate has the same tools, connections and reads
    And the instance is running a task
    When the repository update arrives
    Then nothing changes until the run ends
    And then the instance is updated and updated_between_runs_at is set

  Scenario: Update that changes what the agent may do
    Given a candidate that adds a connection
    When the repository update arrives
    Then the instance is update_awaiting_choice and UpdateNeedsChoice is sent

  Scenario: Withdrawn part with no replacement
    Given a ready instance whose lock file names a part that is now withdrawn without replacement
    When the withdrawal is read
    Then the instance is refused with a stopped reason and Stopped is sent

  Scenario: The agent asks which pack to use
    Given an instance with a testing-pack rule left for the agent to ask
    When the person answers with "Test pyramid"
    Then the choice is stored with source agent_asked
    And the bundle is rebuilt and the folder updated before the agent goes on

  Scenario: Verify agent folders without repair
    Given two ready instances, one whose folder matches its lock file and one with a changed file
    When verify_agent_folders is called with repair false
    Then checked is 2, repaired is 0 and one failure carries code PKG_FOLDER_MISMATCH
    And no folder is changed

  Scenario: Verify agent folders with repair
    Given a ready instance whose folder has a missing file
    When verify_agent_folders is called with repair true
    Then the folder is rebuilt from the lock file and repaired is 1

  Scenario: Workflow agent at another version
    Given the workspace serves agent "urn:ordin:acme:agents:analyst" version "1.2.0" from the repository with profile key "acme-agents"
    When match_workflow_agent is called with that key, identifier and version "1.3.0"
    Then the result is OtherVersion with installed_version "1.2.0"

  Scenario: Find agents for a need
    Given the library offers an agent "Rate comparison" and the workspace has none
    When find_agents is called with need "compare supplier day rates"
    Then the result lists "Rate comparison" first with in_workspace empty

  Scenario: Proposal of a withdrawn agent
    Given repository R's agent "planner" is withdrawn
    When check_proposal is called for R and "planner"
    Then the call fails with code PKG_AGENT_NOT_OFFERED

  Scenario: Agent that may send email
    Given an installed instance whose stored summary lists the connection "mail"
    When can_send_email is called for it
    Then the answer is true

  Scenario: Remove after confirmation
    Given a ready instance in one workspace
    When the person confirms Remove
    Then the Installers uninstall it, the Package Inspector forgets it, the Checker forgets its decisions, the folder is deleted and the status is removed
    And the Resolver prunes bundles no longer named by any instance

  Scenario: Agents of a workspace for the Knowledge Service
    Given workspace W serves "Release planner" and "Analyst", and a removed instance "Old helper"
    When agents_in_workspace is called for W
    Then the result lists "Analyst" then "Release planner" and not "Old helper"

  Scenario: Trust lost when the company's profile changes
    Given a ready instance whose MCP server is no longer trusted by the new profile
    When apply_trust_changes is called with that instance in lost
    Then the instance is refused with a stopped reason and Stopped is sent
    And the next ensure_ready for it fails with PKG_INSTANCE_NOT_READY

  Scenario: Add stores the complete executable-part decisions
    Given an offered agent whose plan names the MCP servers "files" and "jira-reader", both trusted
    When the person presses Add it
    Then the install check runs with the new instance's id
    And decide_executable_parts receives both servers from checker.executable_parts

  Scenario: Trust regained after IT approves
    Given an instance refused because its MCP server "files" lost trust, whose folder still matches its lock file
    When apply_trust_changes is called with that instance in gained
    Then the instance is ready, its stopped reason is cleared and Resumed is sent

  Scenario: A fixed version brings a withdrawn agent back
    Given an instance refused because a part it used was withdrawn without replacement
    And a new commit of its repository whose agent no longer uses that part and passes its checks
    When the repository update arrives
    Then the instance stays refused with an update waiting and UpdateNeedsChoice is sent
    When the person accepts the update
    Then the instance is ready and Resumed is sent

  Scenario: Source scope of a trial
    Given an instance that is a trial of the person's own draft
    When source_scope is called for it
    Then the call fails with code PKG_TRIAL_NO_SCOPE

  Scenario: Open pack question of an installed agent
    Given a ready instance whose plan left rule 0 "testing" open with candidates "pytest" and "unittest"
    When pending_choices is called for it
    Then the result is one PendingChoice with rule_index 0, at_least 1, class_label "testing" and both candidates with their names
    When record_answer is called with rule_index 0 and packs ["pytest"]
    Then the choice is stored with source agent_asked
    And pending_choices for it returns an empty list

  Scenario: Pack change while the agent is working
    Given a ready instance with a running session
    When the person changes its testing pack
    Then the bundle is rebuilt and installed with update_in_session
    And the installed instance ends ready with the new choice stored
```

## Performance, Security, Observability

- **Performance targets:** `preview_add` ≤ 1 s p95 for a 200-part closure (*chosen here*; plan and check only); `add` ≤ 10 s p95 excluding first-time network (*chosen here*); `list` ≤ 150 ms p95 (A-03); `ensure_ready` ≤ 500 ms p95 when the folder matches (one digest pass over ≤ 200 parts, SA-INST-01).
- **Security:** an agent is installed only after the Checker's verdict and inside the trust policy; adding a repository never widens what may run (ADR-086); executable parts are never put to the business user (02:426); folders are created by the Agent Folder Manager under the application data root; no part file is executed by this component; trials are limited to needs the trust policy already covers (ADR-083). Service API inputs are typed with unknown fields refused.
- **Observability (D, P; logs only):** `component = "package_catalogue"`; `agent.add` {agent_instance_id, agent_identifier, outcome, duration_ms}; `agent.add_refused` {agent_identifier, verdict}; `agent.update` {agent_instance_id, kind: silent|needs_choice|refused, to_commit}; `agent.stopped` {agent_instance_id, entry_id}; `agent.removed` {agent_instance_id}; `agent.repaired` {agent_instance_id}; `agent.trust_gained` {agent_instance_id, resumed}; `agent.trust_gained_folder_unusable` {agent_instance_id}; `agent.pending_choices_unreadable` {agent_instance_id} (§N.2). Never logged: part text, check finding text beyond codes, folder contents.

## Gaps

None.
