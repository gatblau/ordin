# SPEC: Checker

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-102) · amended 2026-01-10 (as-built readings) · amended 2026-01-10 (publish gate names a set of evaluations; Approved — user, 2026-01-10, "proceed with recommendations") · amended 2026-01-10 (the publish gate is read in the agent's own repository; bundled agents are outside the company trust policy; Approved — user, 2026-01-10, "proceed with recommendations") · amended January 2026 (a task's title and inputs are checked, CHK-TF-1 and CHK-TF-2, ADR-119; Approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.66.0
> **Crate:** `ordin-checker` · **Build phase:** 3 · **Programs:** D P A
> **Dependencies:** Agent Model, Resolver and Compiler, Organisation Profile, Journal

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
- [Amendment January 2026 — Slack for working together (ADR-102)](#amendment-january-2026--slack-for-working-together-adr-102)
- [Amendment 2026-01-10 — the publish gate names a set of evaluations](#amendment-2026-01-10--the-publish-gate-names-a-set-of-evaluations)

## Purpose

The Checker is ordin's fixed code that decides whether a part or an agent is fit to be offered, tried, published or kept installed — "Checked by ordin" (02:448–452, ADR-082, ADR-085). It runs the earlier implementation's checks on every part read and every draft — vocabulary first, then each part, then cross-checks between parts — and adds ordin's own: the organisation's trust policy over tool bindings and hooks (owning the resulting **executable-part decisions**, 02:449 revision 0.48.0), name collisions, coverage of each job by a task, the wording rules ("ordin tidied N things"), the format loss report, conflicts between parts that can be installed together (ADR-088), the fixed-packs rule (ADR-089), the publish gate met by the rehearsal (ADR-090 §4), and each rehearsal expectation against the run's evidence. Before any part draft, withdrawal or vocabulary change is published it checks the whole repository with the change, all or nothing, and rebuilds and checks every agent that uses the changed part (ADR-091 item 10). It runs on demand for **Check it** against an installed agent's lock file (ADR-094 item 3). It also gives the Package Catalogue an install verdict before an agent is added or updated, the Handover Service its three handover checks (ADR-092 item 2, ADR-095), the Authoring Service the dossier reader, and the Release Builder a check of a package that names the file and entry of each finding. Like the Resolver and Compiler, it is built without a part source and the program binds the Catalogue Service later (reconciliation ruling R18). No check can be waived (ADR-090 *Loose ends*). Every finding names its fix where fixed code knows one (ADR-094 item 4).

## Approach

A port of the earlier implementation's `authoringcheck` (stage one, cross-checks, partial cross-checks, reference checks, memory, control loop, crew, assay policy), `compositioncheck` and `artefactbuild` (ADR-088 port map), keeping the earlier implementation's finding codes byte for byte so the parity tests compare the frozen reference outcomes with ordin's (R-69); ordin's own checks are new code with codes of the same form. Each check is a pure function `(&CheckContext) -> Vec<Finding>` over in-memory parts; the `Checker` service gathers the context (through `PartSource`, the Resolver and Compiler, and the organisation profile), runs the table of checks in a fixed order, and records the run and its findings in its own tables. Rejected: (1) a rules language or plug-in checks — every gate must be a Rust function with table-driven tests (ADR-085 *Consequences*); (2) stopping at the first failure as the earlier implementation's ingest does — the draft screen lists every failing check with its fix (11:820), so ordin collects all findings and the caller decides (all or nothing is kept: one failure fails the subject).

## Shared Context

### Types from Phase 2C (`ordin-types`)

```rust
pub struct AgentInstanceId(pub Uuid); pub struct AgentDraftId(pub Uuid); pub struct PartDraftId(pub Uuid);
pub struct RehearsalId(pub Uuid); pub struct PartId(pub Uuid); pub struct AgentRepositoryId(pub Uuid);
pub struct ExecutablePartDecisionId(pub Uuid); pub struct RunId(pub Uuid); pub struct WorkspaceId(pub Uuid);
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>);
pub struct Sha256Hex(pub String);

/// 2C §ExecutablePartDecision — owner amended to the Checker (02:449, revision 0.48.0).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExecutablePartDecision {
    pub id: ExecutablePartDecisionId,
    pub agent_instance_id: AgentInstanceId,
    pub part_name: String,
    pub kind: ExecutablePartKind,
    pub decision: ExecutablePartTrust,
    pub source: DecisionSource,
    pub decided_at: Timestamp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutablePartKind { Hook, McpServer, BundledProgram }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutablePartTrust { Trusted, NeedsItApproval }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionSource { OrganisationPolicy }

/// 2C §AgentDraft (fields the Checker reads).
pub struct AgentDraft { pub id: AgentDraftId, pub agent_repository_id: AgentRepositoryId, pub name: String,
    pub purpose: String, pub jobs: Vec<DraftJob>, pub tasks: Vec<String>, pub will: Vec<String>,
    pub wont: Vec<String>, pub needs: Vec<String>, pub reused_parts: Vec<PartId>, pub origin: DraftOrigin,
    /* … */ }
pub struct DraftJob { pub text: String, pub came_from: String }
/// 2C — "ordin tidied N things" entries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WordingChange { pub field: String, pub before: String, pub after: String }
pub enum DraftOrigin { Made, Imported }
pub struct Lifecycle { /* 2C §Lifecycle */ }
pub enum PartKind { /* 25 kinds */ }  pub enum PartStatus { Draft, Approved, Deprecated }
pub struct WithdrawnListEntry { /* 2C §WithdrawnListEntry */ }
pub enum VocabularyAxis { PackClass, Category, AudienceRole, RuleSubvariant, ArtefactStatus,
    MemoryContentClass, LoopStrategy, RuntimeCapability }
```

2C rules this component enforces: `EXP_PART_DUPLICATE` (part_name unique per instance and kind); `EXP_SOURCE_NOT_POLICY` (source is always `organisation_policy`); the AgentDraft limits LD-21 (name ≤ 80, purpose ≤ 500, ≤ 50 items of ≤ 500 characters); the Lifecycle rules `LIFE_*` (LD-20); `VOCAB_TERM_CITED`, `VOCAB_STATUS_TERM_FIXED`; `WDL_APPEND_ONLY`.

### Types from the Agent Model and the Resolver and Compiler

```rust
// ordin-agent-model
pub struct ModelPart { pub kind: PartKind, pub manifest: Manifest, pub manifest_path: String,
                       pub files: PartFiles, pub content_digest: Sha256Hex }
pub struct Manifest { /* api_version, authored_kind, metadata, body; typed readers */ }
pub struct VocabularySet; pub struct ControlLoopSpec { pub strategy: String, pub trigger: String,
    pub requires: Vec<String>, pub modes: Vec<String> }
pub struct MemoryContract; pub struct RequiredPackRule; pub enum FixedPackEntry; pub struct JourneyBody;
pub struct PartRef; pub struct LockFile; pub struct ResolvedToolPolicy; pub struct McpServerDecl;
pub enum LoadingMode;
pub fn kind_info(kind: PartKind) -> &'static KindInfo;  pub fn relation_kind(r: &str) -> Option<PartKind>;
pub fn slug_from_id(id: &str) -> String;                pub fn is_part_identifier(s: &str) -> bool;
/// An OutputContract's `sections` (AgentModel.md, typed reader; SA-CLOSE-02 here).
pub struct ContractSection { pub name: String, pub meaning: String, pub required: bool,
                             pub evidence_threshold: u32, pub derived: bool }
pub struct OutputContract { /* private: sections */ }
impl OutputContract {
    pub fn read(manifest: &Manifest) -> Result<OutputContract, AmError>;   // AM_FIELD_SHAPE_INVALID on a malformed list
    pub fn sections(&self) -> &[ContractSection];
}
/// Owned by the Agent Model (moved there in the closing round, K22); `ordin_resolver` re-exports both.
pub struct StagedPart { pub part: ModelPart, pub repository: RepositoryRef, pub repository_label: String }
#[derive(Clone)] pub struct SourceOverride(pub Arc<dyn PartSource>);   // per-request source (R18)
pub struct SourceScope; pub enum RepositoryRef; pub struct SourcedPart; pub enum Resolution;
#[async_trait] pub trait PartSource { /* resolve, resolve_any_kind, files, ingredients, withdrawn,
    repository_parts, vocabulary, used_by — see Agent Model spec */
    async fn citing_parts(&self, repository: &RepositoryRef, axis: VocabularyAxis, code: &str)
        -> Result<Vec<SourcedPart>, PartSourceError>;   // retiring a vocabulary term (step 28)
}
pub enum PartSourceError { Unavailable { retryable: bool }, WithdrawnListUnreadable { repository: String },
    Corrupt { part_identifier: String } }   // AM_SOURCE_*

// ordin-resolver (re-exports `StagedPart` and `SourceOverride` from the Agent Model)
pub struct ResolveRequest { /* scope, root_id, root_version, target_harness, workspace_languages,
    interface_language, pack_answers, pack_changes, overlay_changes, replacements, staged, allow_draft_root,
    source: Option<SourceOverride> */ }
pub struct PackSpace { pub packs: Vec<SourcedPart>, pub overlays: Vec<SourcedPart> }
pub struct Conflict { pub slot: String, pub candidates: Vec<ConflictCandidate> }
impl ResolverAndCompiler {
    pub async fn resolve(&self, r: &ResolveRequest) -> Result<ResolutionPlan, ResolveError>;
    pub async fn pack_space(&self, r: &ResolveRequest) -> Result<PackSpace, ResolveError>;
    pub async fn conflicts(&self, r: &ResolveRequest, s: &PackSpace) -> Result<Vec<Conflict>, ResolveError>;
    /// Read-only: the plan a lock file names; writes nothing (used by `check_installed`).
    pub async fn resolve_lock(&self, lock: &LockFile, scope: &SourceScope) -> Result<ResolutionPlan, ResolveError>;
}
/// Free function, used by `check_parts` (step 39) with the Release Builder's source.
pub struct CompiledInstructions { pub agents_md: String, pub lock: LockFile }
pub async fn compile_instructions(source: &dyn PartSource, request: &ResolveRequest)
    -> Result<CompiledInstructions, ResolveError>;
// ordin-format-plugins types arrive as plain data from the caller (no dependency edge):
// the caller converts each `MappingItem` and `LossItem` to `ReportItem` by the Format
// Plug-ins' fixed rule (FormatPlugins.md, N185): MappingItem → { what: construct,
// part_identifier, reason: reason.code() }; LossItem → { what, part_identifier:
// Some(part_identifier), reason: reason.code() }.
```

### Organisation Profile (`ordin-organisation-profile`)

`OrganisationProfileService::current() -> Arc<ProfileSnapshot>` and `subscribe() -> watch::Receiver<Arc<ProfileSnapshot>>` (B2 contract §3). This component reads `snapshot.document.agent_catalogue.trust_policy: serde_json::Value` (`AgentCatalogueSection`; 2D-2: "object: tool bindings and hooks the company trusts"; default empty — every executable part needs IT approval). The Organisation Profile carries the value as untyped JSON; its type, `TrustPolicy`, is owned by this component and defined in the Public Interface (reconciliation row 624).

### OutputContract sections (dossier reader, handover contract check)

An OutputContract part's body holds `sections`, a list of `{ name, meaning, required, evidenceThreshold, derived }` (verified against the `role-dossier` OutputContract of the reference catalogue). The Checker reads it through the Agent Model's typed reader `OutputContract::read(manifest)?.sections()` (step 38).

### Journal (`ordin-journal`)

The B2 form (ruling R3, decision D4): this crate depends on the Journal, receives `Arc<Journal>` and reaches its tables through `Journal::datastore() -> &Datastore`. It exports `pub fn migrations() -> ModuleMigrations` with `module = "checker"` and `MigrationStep = (i64, &'static str, &'static str)` tuples (version 1 in each of `sqlite` and `postgres`); the program registers it. The Checker writes no journal entry or event: its runs are its own tables (*Data Model*).

### Reconciliation assumptions

- **SA-RECON-01** — The source is bound late (ruling R18): the program builds the Checker with `new(resolver, profile, journal)` and calls `install_part_source` once, right after the Catalogue Service is built; a part-reading call before then fails with the permanent `CHK_SOURCE_NOT_BOUND`. A caller that must read a draft passes `RepositoryCheck.source` for that request only.
- **SA-RECON-02** — The Checker owns its names (ruling R6): every error is `CheckError` and the crate declares no consumer-facing trait; consumers rename `CheckerError` and their narrow traits.
- **SA-RECON-03** — Vocabulary citations in other repositories come from the Agent Model's `citing_parts` (reconciliation row 549); `used_by` is used only for agents whose closure includes a changed part (step 13).
- **SA-RECON-04** — Format reports reach the Checker as `ReportItem`s converted by the caller with the Format Plug-ins' codes (N185); the Checker has no edge to `ordin-format-plugins` (2B-3).
- **SA-RECON-05** — The Checker uses the Journal's B2 form (ruling R3): `Arc<Journal>`, `datastore()`, and `migrations() -> ModuleMigrations`.
- **SA-RECON-06** — `TrustPolicy` is typed only here; the Organisation Profile carries `agent_catalogue.trust_policy` as untyped JSON (reconciliation row 624).

### Closing-round assumptions

- **SA-CLOSE-01** (*chosen here*; closes the installed-scope gap): the program fills `checker.check_installed`'s `scope` with `PackageCatalogue::source_scope(instance)` (PackageCatalogue.md §M) and its `lock` from the install record's bundle.
- **SA-CLOSE-02** (*chosen here*; closes the OutputContract gap): contract sections are read with the Agent Model's `OutputContract::read(manifest)?.sections()`; the Checker no longer walks `manifest.body["sections"]` itself.
- **SA-CLOSE-03** (*chosen here*; serves Package Catalogue G-PKG-1): `check_for_install` with `agent_instance_id = Some(id)` stores the complete executable-part list it derived (step 31's derivation) for that instance, replacing the previous list; `executable_parts(id)` returns it. `InstallVerdict::NeedsItApproval` still lists only the untrusted parts.

### Confirmation and HLD facts

- The earlier implementation's stage-one order: apiVersion, kind, `metadata.{id,version,owner}`, descriptive metadata of WorkspaceProfile and PracticePack (category and ≥ 1 audience role, each in the vocabulary or the built-in term `imported`; class in the vocabulary when set), pack toolchain, id slug `^[A-Za-z][A-Za-z0-9-]{0,127}$`, loading mode, status in the vocabulary, body policies (required packs, assay policy).
- The earlier implementation's cross-checks and their partial-safety: control loops (partial-safe), journey seams, appliesTo, profile pack offers, profile overlay offers, publish gate, crew references, crew, memory, control-loop terms (partial-safe), crew loop override (partial-safe).
- The earlier implementation's candidate validation reports only findings that the candidate introduces, not those already present in the snapshot (`ValidateCandidate`).
- R-94: package text that names a bare command is reported by the Checker.
- ADR-091 item 3 as amended by UI-D99: an agent that does not rebuild and pass with a withdrawal's replacement is listed under *Stop*; the withdrawal is never blocked by it.

### Configuration

No environment variable. Profile key read: `agent_catalogue.trust_policy` (2D-2). Constants (*chosen here*): `MAX_FINDINGS_PER_RUN = 500`; British-spelling table and leading-verb list are compiled-in data (step 14).

## Public Interface

Crate `ordin_checker`. Names are *chosen here* unless traced.

```rust
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CheckRunId(pub uuid::Uuid);                       // UUIDv7; 2C `check_results_ref` = its string

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity { Failure, Warning }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "fix", rename_all = "snake_case")]
pub enum Fix {
    /// "Fix it" — fixed code knows the one right answer (ADR-094 item 4).
    FixIt { action: FixAction },
    /// "Ask Ordin to fix it" — judgement needed; the agent maker suggests text.
    AskOrdin,
    /// "Change it yourself" — opens the field.
    ChangeItYourself { field: String },
    /// "Leave it out" / "Leave [tool] out".
    LeaveItOut { what: String },
    /// "Publish to an ordin-format repository" (ADR-087).
    PublishToOrdinFormat,
    /// "Ask IT" / "Ask IT about [tool]" (ADR-012).
    AskIt,
    /// "Ask the agent maker to fix this" (ADR-088 *Conflicts*).
    AskAgentMaker,
    /// "Choose which [class] this agent uses when the workspace doesn't say" (ADR-089).
    ChoosePacks { rule_index: u32, choices: Vec<String> },
    /// "Let the agent ask" (decision 64).
    LetAgentAsk { rule_index: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum FixAction {
    MintIdentifier { field: String, value: String },
    PointToReplacement { field: String, from: String, to: String },
    AskMissingField { field: String },
    DropUnknownField { field: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Finding {
    /// Stable machine code: the earlier implementation's verbatim (`sync_pack_not_applicable`, `kind_unknown`) or
    /// ordin's (`trust_needs_it_approval`). Lower snake case, ≤ 64 characters.
    pub code: String,
    pub severity: Severity,
    /// `metadata.id` of the part the finding concerns, or None for the agent as a whole.
    pub part_identifier: Option<String>,
    /// Fluent message id of the one plain sentence (`check-…`); the screens render it.
    pub message_id: String,
    /// Plain values the sentence needs (part names, display names, counts); never file content.
    pub args: BTreeMap<String, String>,
    pub fixes: Vec<Fix>,
    /// Technical detail for Diagnostics only (the earlier implementation's message text where ported).
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckSubject {
    RepositoryRead { repository: String, commit: String },
    AgentDraft { agent_draft_id: AgentDraftId },
    PartDraft { part_draft_id: PartDraftId },
    Withdrawal { part_draft_id: PartDraftId },
    VocabularyChange { part_draft_id: PartDraftId },
    InstalledAgent { agent_instance_id: AgentInstanceId },
    LibraryItem { part_identifier: String, version: String },
    Rehearsal { rehearsal_id: RehearsalId },
    /// `check_for_install`: an agent about to be added, changed, updated or tried.
    InstallCandidate { root_id: String, plan_hash: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CheckReport {
    pub check_run_id: CheckRunId,
    pub subject: CheckSubject,
    /// True when no finding has severity Failure.
    pub passed: bool,
    pub findings: Vec<Finding>,
    /// Applied wording changes ("ordin tidied N things"); empty for non-draft subjects.
    pub tidied: Vec<WordingChange>,
    /// "Checked [time], against the version in use" (11 §5.30).
    pub checked_at: Timestamp,
    pub version_checked: String,
    /// Agents that stop when a withdrawal is published (UI-D99), by identifier.
    pub agents_that_stop: Vec<String>,
    /// False only when step 25 reported `publish_gate_not_met`; true for every subject with no
    /// publish gate. Derived from the findings, so `report(id)` returns the same value.
    pub publish_gate_met: bool,
}

/// Mapping or loss report items, as plain data from the Format Plug-ins (via the caller).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReportItem { pub what: String, pub part_identifier: Option<String>, pub reason: String }

#[derive(Debug, Clone)]
pub struct RepositoryCheck {
    pub repository: RepositoryRef,
    pub repository_display_name: String,
    /// The repositories this one borrows from (ADR-090 §2), for reference resolution.
    pub scope: SourceScope,
    /// New and changed parts; an empty list checks the repository as it is (a read).
    pub changes: Vec<StagedPart>,
    /// Identifiers of parts the change deletes from the tree.
    pub deletions: Vec<String>,
    /// None = the bound source; Some = this source for this check only — the Catalogue
    /// Service's `draft_source(repository, worktree)` for a draft check (R18).
    pub source: Option<SourceOverride>,
}

#[derive(Debug, Clone)]
pub struct AgentDraftCheck {
    pub draft: AgentDraft,
    /// The draft's WorkspaceProfile and every new or changed part, generated by the Agent Model.
    pub parts: Vec<StagedPart>,
    pub target: RepositoryCheck,
    /// Target repository format, for the loss report and SA-APM-01.
    pub target_format: TargetFormat,
    /// Which tasks serve each job (index into `draft.jobs`), from the Authoring Service.
    pub job_tasks: BTreeMap<u32, Vec<String>>,
    /// Format Plug-ins outputs passed through by the caller.
    pub mapping_report: Vec<ReportItem>,
    pub loss_report: Vec<ReportItem>,
    /// Identifiers the draft references from other repositories.
    pub borrowed: Vec<String>,
    /// Other agents' names in the target repository, for the collision check.
    pub repository_agent_names: Vec<String>,
    /// The latest rehearsal's expectation outcomes, None before the first rehearsal.
    pub rehearsal: Option<Vec<ExpectationOutcome>>,
    /// The agent's language for the wording rules (BCP 47); "en-GB" when the agent has none.
    pub agent_language: String,
    /// For a lifecycle part: which handover places the person can reach (ADR-095 item 2).
    pub reachable_places: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetFormat { Ordin, Apm }

#[derive(Debug, Clone)]
pub struct WithdrawalCheck {
    pub repository: RepositoryCheck,
    pub entry: WithdrawnListEntry,
    /// The repository's withdrawn list as it is now (entries already merged).
    pub existing_entries: Vec<WithdrawnListEntry>,
    /// The file's proposed full text, to verify the change only appends.
    pub proposed_list: Vec<WithdrawnListEntry>,
}

#[derive(Debug, Clone)]
pub struct VocabularyCheck {
    pub repository: RepositoryCheck,
    pub axis: VocabularyAxis,
    pub added: Vec<(String, String, String)>,      // (code, label, description)
    pub retired: Vec<String>,                       // codes
}

/// A deterministic rehearsal expectation (ADR-085, ADR-090 §4). Closed set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExpectationRule {
    ResultContains { text: String },
    ResultHasSection { heading: String },
    ToolUsed { tool: String },
    ToolNotUsed { tool: String },
    ApprovalAsked,
    NoExternalAction,
    ArtefactProduced { name_suffix: String },
}

/// What the run left. Built by the Ordin Host when a rehearsal run ends (`AuthoringHost::rehearse`,
/// OrdinHost.md SA-CLOSE-06, X-28) and passed by the Authoring Service to `check_rehearsal`.
#[derive(Debug, Clone, Default)]
pub struct RunEvidence {
    pub run_id: RunId,
    pub final_message: String,
    pub tools_used: Vec<String>,
    pub approvals_asked: u32,
    pub external_actions: u32,
    pub artefact_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExpectationOutcome { pub rule: ExpectationRule, pub met: bool, pub evidence: String }

/// One executable part of an installed agent, as the trust decision sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExecutablePart {
    pub part_name: String,          // MCP server name, program name, or hook part identifier
    pub kind: ExecutablePartKind,
    pub declared_by: String,        // ToolBinding or hook part identifier
}

/// The organisation's trust policy (2D-2 `agent_catalogue.trust_policy`); owned here (row 624).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
/// Missing keys read as empty lists, so the profile's default `{}` is the empty policy; unknown
/// keys are refused.
#[serde(rename_all = "snake_case", deny_unknown_fields, default)]
pub struct TrustPolicy {
    /// ToolBinding parts the company trusts, by `metadata.id`, with the programs or MCP servers
    /// each may start (ADR-082: "by identifier and the program each starts").
    pub tool_bindings: Vec<TrustedToolBinding>,
    /// Hook parts (event-loaded parts) the company trusts, by `metadata.id`.
    pub hooks: Vec<String>,
    /// Built-in harness tools a tool part may name (ADR-094 item 5: "a tool part must name a
    /// tool the organisation profile allows"); empty = every built-in tool allowed.
    pub allowed_builtin_tools: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TrustedToolBinding { pub id: String, pub programs: Vec<String> }
impl TrustPolicy {
    /// Parses the profile's JSON value. Absent → empty policy. A value that does not parse →
    /// empty policy (fail closed: every executable part needs IT approval), logged
    /// `checker.trust_policy_invalid` at `error`.
    pub fn from_profile_value(value: Option<&serde_json::Value>) -> TrustPolicy;
}

/// `check_for_install` input: a plan from `ResolverAndCompiler::resolve`.
#[derive(Debug, Clone)]
pub struct InstallCheck {
    pub plan: ResolutionPlan,
    pub scope: SourceScope,
    /// None for a new agent; Some for a change, update or trial of an existing instance.
    pub agent_instance_id: Option<AgentInstanceId>,
    /// Some(draft source) for a trial of the person's own draft; None otherwise.
    pub source: Option<SourceOverride>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum InstallVerdict {
    Clean,
    /// At least one Failure finding; the findings are in the report.
    Failed,
    /// No Failure, but these executable parts are not trusted by the profile.
    NeedsItApproval { parts: Vec<ExecutablePart> },
}
#[derive(Debug, Clone, Serialize)]
pub struct InstallCheckReport { pub verdict: InstallVerdict, pub report: CheckReport }

/// `check_handover` input, built by the Handover Service from the Workspace Service's facts.
#[derive(Debug, Clone)]
pub struct HandoverCheckInput {
    pub results: Vec<HandoverResult>,
    /// The stage's OutputContract; None for a handover outside any lifecycle.
    pub contract: Option<HandoverContract>,
    pub receiver: ReceiverFacts,
}
#[derive(Debug, Clone)]
pub struct HandoverResult {
    pub link: String,
    pub label: String,
    /// True when the result exists only on this computer (Workspace Service `ThisComputerOnly`).
    pub local_only: bool,
    /// None = unknown (no check possible), Some(false) = the receiver cannot open it.
    pub readable_by_receiver: Option<bool>,
    /// The result's Markdown headings, None when the result is not a readable document.
    pub headings: Option<Vec<String>>,
}
#[derive(Debug, Clone)]
pub struct HandoverContract { pub scope: SourceScope, pub identifier: String }
#[derive(Debug, Clone)]
pub struct ReceiverFacts { pub is_person: bool, pub reachable: bool }
#[derive(Debug, Clone, Serialize)]
pub struct HandoverCheckReport { pub passed: bool, pub findings: Vec<HandoverFinding> }
#[derive(Debug, Clone, Serialize)]
pub struct HandoverFinding {
    /// `results_readable`, `contract_sections` or `receiver_reachable`.
    pub code: String,
    pub passed: bool,
    /// The result link or contract identifier the finding concerns.
    pub item: Option<String>,
    pub missing_sections: Vec<String>,
}

/// `read_dossier` output (port of the earlier implementation's dossier `Reader::Parse`).
#[derive(Debug, Clone, Serialize)]
pub struct DossierReading {
    /// Contract sections found, in the contract's order.
    pub sections: Vec<DossierSection>,
    /// Contract section names no heading matched.
    pub missing_sections: Vec<String>,
    /// Normalised headings matching no contract section, first-seen order.
    pub unmatched_headings: Vec<String>,
}
#[derive(Debug, Clone, Serialize)]
pub struct DossierSection { pub name: String, pub text: String, pub evidence_count: u32,
    pub required: bool, pub low_confidence: bool, pub derived: bool }

/// `check_parts` output for the Release Builder: one entry per finding, located by file.
#[derive(Debug, Clone, Serialize)]
pub struct PackageCheckReport { pub findings: Vec<PackageFinding> }
#[derive(Debug, Clone, Serialize)]
pub struct PackageFinding {
    /// True for a Failure finding.
    pub stop: bool,
    /// Path of the part's file within the package, or "" for a finding about the agent as a whole.
    pub file: String,
    /// The part's `metadata.id`, or the finding code when there is no part.
    pub entry: String,
    /// The finding's `detail`.
    pub message: String,
}

/// Check a package's parts from `source` for the Release Builder (no datastore, no record, no
/// trust policy, no bound source). Sections C and D steps 9–12 over the root's closure.
pub async fn check_parts(source: &dyn PartSource, request: &ResolveRequest)
    -> Result<PackageCheckReport, CheckError>;

pub struct Checker { /* private: OnceLock<Arc<dyn PartSource>>, resolver, profile, journal */ }

impl Checker {
    /// Built without a source (R18): every operation that reads parts fails with
    /// `CHK_SOURCE_NOT_BOUND` until `install_part_source` has run.
    pub fn new(resolver: Arc<ResolverAndCompiler>, profile: Arc<OrganisationProfileService>,
               journal: Arc<Journal>) -> Self;
    /// Bind the source once, after the Catalogue Service is built; a second call fails with
    /// `CHK_SOURCE_ALREADY_BOUND` and leaves the first source in place.
    pub fn install_part_source(&self, source: Arc<dyn PartSource>) -> Result<(), CheckError>;
    /// Part checks of a plan's closure plus the trust policy; records the run, never the
    /// executable-part decisions (those are `decide_executable_parts`, after install).
    pub async fn check_for_install(&self, request: &InstallCheck) -> Result<InstallCheckReport, CheckError>;
    /// The three handover checks (ADR-092 item 2, ADR-095); not recorded.
    pub async fn check_handover(&self, input: &HandoverCheckInput) -> Result<HandoverCheckReport, CheckError>;
    /// Pure: the earlier implementation's dossier reader over an OutputContract's sections.
    pub fn read_dossier(&self, write_up: &str, contract: &ModelPart) -> Result<DossierReading, CheckError>;

    /// Pure: stage one of one part (no I/O, no record).
    pub fn check_part(&self, part: &ModelPart, vocabulary: &VocabularySet) -> Vec<Finding>;
    /// Every part of a repository at a commit, all or nothing (Catalogue Service ingest, ADR-082).
    pub async fn check_repository(&self, request: &RepositoryCheck, subject: CheckSubject)
        -> Result<CheckReport, CheckError>;
    pub async fn check_agent_draft(&self, request: &AgentDraftCheck) -> Result<CheckReport, CheckError>;
    pub async fn check_part_draft(&self, request: &RepositoryCheck, part_draft_id: PartDraftId,
        reachable_places: &BTreeMap<String, bool>) -> Result<CheckReport, CheckError>;
    pub async fn check_withdrawal(&self, request: &WithdrawalCheck, part_draft_id: PartDraftId)
        -> Result<CheckReport, CheckError>;
    pub async fn check_vocabulary(&self, request: &VocabularyCheck, part_draft_id: PartDraftId)
        -> Result<CheckReport, CheckError>;
    /// **Check it** on an installed agent, against its lock file (ADR-094 item 3).
    pub async fn check_installed(&self, id: AgentInstanceId, lock: &LockFile, scope: &SourceScope)
        -> Result<CheckReport, CheckError>;
    /// **Check it** on a library agent or part, against the protected branch.
    pub async fn check_library_item(&self, scope: &SourceScope, kind: PartKind, identifier: &str)
        -> Result<CheckReport, CheckError>;
    /// Pure: each expectation against the run's evidence.
    pub fn check_rehearsal(&self, rules: &[ExpectationRule], evidence: &RunEvidence) -> Vec<ExpectationOutcome>;
    /// Pure: the wording rules over named text fields; returns the tidied text and the changes.
    pub fn apply_wording_rules(&self, fields: &[(String, String)], language: &str)
        -> (Vec<(String, String)>, Vec<WordingChange>, Vec<Finding>);
    /// Decides and stores the trust of each executable part of an instance (install, update).
    pub async fn decide_executable_parts(&self, id: AgentInstanceId, parts: &[ExecutablePart])
        -> Result<Vec<ExecutablePartDecision>, CheckError>;
    /// Every executable part (trusted or not) derived at the instance's latest `check_for_install`
    /// with `agent_instance_id = Some(instance)` (SA-CLOSE-03); none stored → `CHK_INSTANCE_UNKNOWN`.
    pub async fn executable_parts(&self, instance: AgentInstanceId) -> Result<Vec<ExecutablePart>, CheckError>;
    /// Re-decides every stored instance against the current profile; returns the instances whose
    /// trust was lost (each to be disabled until IT approves) and those whose trust was gained.
    pub async fn reevaluate_trust(&self) -> Result<TrustChanges, CheckError>;
    pub async fn executable_part_decisions(&self, id: AgentInstanceId)
        -> Result<Vec<ExecutablePartDecision>, CheckError>;
    pub async fn forget_instance(&self, id: AgentInstanceId) -> Result<(), CheckError>;
    pub async fn report(&self, id: CheckRunId) -> Result<CheckReport, CheckError>;
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TrustChanges { pub lost: Vec<AgentInstanceId>, pub gained: Vec<AgentInstanceId> }

#[derive(Debug, Clone, thiserror::Error)]
pub enum CheckError {
    #[error("CHK_SOURCE_UNAVAILABLE")] SourceUnavailable { retryable: bool },
    #[error("CHK_RESOLVER_FAILED")] ResolverFailed { code: String, retryable: bool },
    #[error("CHK_INPUT_INVALID")] InputInvalid { field: &'static str },
    #[error("CHK_TOO_MANY_FINDINGS")] TooManyFindings,
    #[error("CHK_RUN_NOT_FOUND")] RunNotFound { id: CheckRunId },
    #[error("CHK_DATASTORE_FAILED")] DatastoreFailed { retryable: bool },
    #[error("CHK_SOURCE_NOT_BOUND")] SourceNotBound,
    #[error("CHK_SOURCE_ALREADY_BOUND")] SourceAlreadyBound,
    #[error("CHK_CONTRACT_INVALID")] ContractInvalid { reason: &'static str },
    #[error("CHK_INSTANCE_UNKNOWN")] InstanceUnknown { id: AgentInstanceId },
}
impl CheckError { pub fn retryable(&self) -> bool; }
```

Consumers hold `Arc<Checker>` and call the inherent methods; the Checker declares no trait. A consumer that needs a test double declares a private trait in its own crate with exactly these signatures and implements it for `Checker` by delegation. Every error is `CheckError` (there is no `CheckerError`).

### Who calls each public item

| Item | Called by |
|---|---|
| `new`, `install_part_source` | Ordin Desktop and Ordin Server programs (`install_part_source(catalogue)` right after the Catalogue Service is built) |
| `check_part` | this crate's own operations (section C); no other component calls it today |
| `check_repository` | Catalogue Service (ingest of a repository at a commit, subject `RepositoryRead`), mapping findings to parts by `part_identifier` |
| `check_agent_draft`, `check_part_draft`, `check_withdrawal`, `check_vocabulary` | Authoring Service (by the draft's change kind) |
| `check_for_install` | Package Catalogue (preview, add, change, answer, update candidate, withdrawal re-plan, trial with `source = Some(draft source)`) |
| `decide_executable_parts` | Package Catalogue, after the Package Inspector's `inspect` succeeds on add and update |
| `executable_parts` | Package Catalogue, right before `decide_executable_parts` (add, accepted update, fixed version) |
| `check_installed`, `check_library_item`, `report` | Service API, through the **Check it** commands below (ruling R1, reconciliation row 625) |
| `report` | Authoring Service (later reads of a draft's check run, its §E3) |
| `check_rehearsal`, `apply_wording_rules`, `read_dossier` | Authoring Service |
| `check_handover` | Handover Service |
| `reevaluate_trust` | Ordin Desktop and Ordin Server programs, on each new verified profile from `OrganisationProfileService::subscribe()`; the Package Catalogue disables the returned `lost` instances |
| `executable_part_decisions` | no caller today; kept for the agent page's trust view (reconciliation row 625) |
| `forget_instance` | Package Catalogue, when an instance is removed (§E) |
| `check_parts` | Release Builder (P-3 Compile) |
| `TrustPolicy` | Organisation Profile stores it as JSON; only this crate types it |

### Service API commands (ruling R1)

The Checker owns these command names; the Service API catalogue adopts them. Each runs in the personal profile (programs D, P) and is called on behalf of the screens. `CheckRunId` and `CheckReport` are defined above.

```rust
pub struct CheckInstalledInput { pub agent_instance_id: AgentInstanceId }
pub struct CheckLibraryItemInput { pub agent_repository_id: Option<AgentRepositoryId>, // None = bundled catalogue
                                   pub kind: PartKind, pub identifier: String }
pub struct CheckRunRef { pub check_run_id: CheckRunId }
```

| Command | Input | Result | Method | Class |
|---|---|---|---|---|
| `checker.check_installed` | `CheckInstalledInput` | `CheckReport` | `check_installed(id, lock, scope)` | Change (records a run) |
| `checker.check_library_item` | `CheckLibraryItemInput` | `CheckReport` | `check_library_item(scope, kind, identifier)` | Change (records a run) |
| `checker.report` | `CheckRunRef` | `CheckReport` | `report(id)` | Read |

The program fills the arguments the screens do not send: for `checker.check_installed`, `lock` = `resolver.open_bundle(installers.record(id).plan_hash).lock_file` and `scope` = `package_catalogue.source_scope(id)` (SA-CLOSE-01; its `PKG_TRIAL_NO_SCOPE` and `PKG_INSTANCE_UNKNOWN` reach the screen as permanent errors); for `checker.check_library_item`, `scope` = `SourceScope { home: the repository (or Bundled), profile: the profile repositories in profile order, include_bundled: true }`. Errors map as the Service API maps any module error: `CHK_INPUT_INVALID` and `CHK_RUN_NOT_FOUND` are permanent; `CHK_SOURCE_UNAVAILABLE` and `CHK_DATASTORE_FAILED` carry their `retryable` flag.

### Example

`check_agent_draft` for a draft "Contract clause checker" whose tool part names the MCP server `jira-reader`, not in the profile's trust policy, and whose job 2 has no task:

```json
{
  "check_run_id": "0192a1d0-1111-7222-8333-444455556666",
  "subject": {"kind": "agent_draft", "agent_draft_id": "0192a1c9-aaaa-7bbb-8ccc-ddddeeee0001"},
  "passed": false,
  "findings": [
    {"code": "trust_needs_it_approval", "severity": "failure",
     "part_identifier": "urn:ordin:acme:tool-bindings:jira", "message_id": "check-trust-needs-it-approval",
     "args": {"tool": "jira-reader"}, "fixes": [{"fix": "leave_it_out", "what": "jira-reader"}, {"fix": "ask_it"}],
     "detail": "mcp server jira-reader declared by urn:ordin:acme:tool-bindings:jira is not in agent_catalogue.trust_policy"},
    {"code": "job_not_covered", "severity": "failure", "part_identifier": null,
     "message_id": "check-job-not-covered", "args": {"job": "Flag clauses that differ from the template"},
     "fixes": [{"fix": "ask_ordin"}, {"fix": "change_it_yourself", "field": "tasks"}], "detail": "job index 2 has no task"}
  ],
  "tidied": [{"field": "purpose", "before": "Analyzes contract clauses", "after": "Analyses contract clauses"}],
  "checked_at": "2026-01-10T10:02:11.540Z",
  "version_checked": "draft branch ordin/draft/contract-clause-checker at 5e1f…",
  "agents_that_stop": [],
  "publish_gate_met": true
}
```

## Internal Logic

### A. Running a check (every async operation)

0. **Source** (R18): `install_part_source` stores the source in a `OnceLock`; a second call → `CHK_SOURCE_ALREADY_BOUND`. Every operation that reads parts uses the request's `source` when it is `Some` (passing it on as `ResolveRequest.source` to every Resolver call), else the bound source; with neither → `CHK_SOURCE_NOT_BOUND` (permanent), logged `checker.source_not_bound` at `error`, nothing recorded. `check_rehearsal`, `apply_wording_rules` and `read_dossier` read no part and never fail this way.
1. Allocate `CheckRunId` (UUIDv7) and `started_at`. Gather the context (section B); a `PartSourceError::Unavailable` → `CHK_SOURCE_UNAVAILABLE` (retryable as given); a `ResolveError` that is not a finding (`RES_SOURCE_UNAVAILABLE`, `RES_STORE_WRITE_FAILED`) → `CHK_RESOLVER_FAILED { code, retryable }`. Resolve errors that are findings (conflict, unsettled pack, withdrawn part, cycle, unresolved reference) become findings (section D).
2. Run the checks of the operation in the order listed in its section; collect every finding (no early stop). More than `MAX_FINDINGS_PER_RUN` → keep the first 500, add one finding `findings_truncated` (failure) and continue (*assumed here*; `CHK_TOO_MANY_FINDINGS` is returned only when the subject itself is malformed beyond reading).
3. `passed = no Failure`. Insert the run and its findings in one transaction (section *Data Model*); a datastore error → `CHK_DATASTORE_FAILED` (the report is not returned, because an unrecorded result cannot be referenced by `check_results_ref`). Append journal record of kind `check_run_completed`, component `checker`, `{check_run_id, subject, passed, failures, warnings, tidied}`; a journal error is logged at `error` and the report is still returned (the run row is the record). The Journal's kind grammar (`^[a-z][a-z0-9_]{0,63}$`) refuses a dot, so a record kind carries no `checker.` prefix; the component names the writer.
4. Log `checker.check_run` (fields below). Return the report.

### B. Context

5. **Vocabulary first.** With no Vocabulary part in the change, `source.vocabulary(scope)` unchanged. With one, a `VocabularySet` cannot be edited axis by axis, so the set is rebuilt from the repository's own Vocabulary parts with the change applied, and vocabularies of borrowed repositories are not part of it. A duplicate axis → finding `sync_duplicate_vocabulary_axis` (failure) and an empty set for that axis; a Vocabulary part whose body is malformed is dropped from the set. Status terms `draft`, `approved`, `deprecated` always present (ADR-091 item 4).
6. **Snapshot.** `source.repository_parts(repository)` → the staged snapshot; apply `changes` (shadowing by kind and identifier) and `deletions` → the union. Keep the pre-change snapshot for "new findings only" (step 10).

**Resolutions.** Every Resolver request the Checker makes names `target_harness` `ClaudeCode`, `interface_language` `en-GB` and empty `workspace_languages`; neither the harness nor the language changes which constituents a closure holds.

### C. Part checks (stage one, port of `StageOne`) — `check_part`

7. For one part, the first failing rule produces one finding and stops that part (the earlier implementation's order): `apiversion_unknown`; `kind_unknown`; `metadata_required` (id, version, owner); for WorkspaceProfile and PracticePack, `descriptive_metadata_missing` (category and ≥ 1 audience role) then `vocabulary_unknown_term` (category, each role, class; the built-in term `imported` is accepted as category and as audience role in every vocabulary set, per the Agent Model's vocabulary rule); `pack_toolchain_invalid`; `id_grammar_invalid` (slug pattern); `loading_invalid`; `vocabulary_unknown_term` (status); `requiredpacks_invalid`; `assay_policy_invalid` (port of `validateAssayPolicy`, read from the body key `assayPolicy`, a list of checks or a map with `checks`; each check is a map with `type` (`deterministic`, `narrative` or `scored`), `blocking`, `threshold` and `direction`: a deterministic check must block; a narrative check must not; a scored check needs a threshold and a direction). Then the per-kind body checks that do not stop the part: memory (`memory_content_class_invalid`, `memory_retains_empty`, `memory_class_conflict`, `memory_retention_invalid`, `memory_store_unresolved`; `memory.go`), control loop (`sync_control_loop_missing_strategy`, `sync_control_loop_retired_field` (the retired field is the body key `phases`, replaced by `modes`), `sync_unknown_loop_strategy`, `sync_unknown_runtime_capability`, `sync_invalid_loop_trigger`; `controlloop.go`). Each finding's `detail` is the earlier implementation's message with ordin's identifiers; `message_id` is `check-part-invalid` with `args.part` = the part's name, and `fixes` per section F.
8. A part whose kind is Journey and whose body has a `stages` key is additionally checked as a lifecycle (ADR-092, ADR-095 item 2, 2C §Lifecycle); a Journey with `seams` and no `stages` is the reference format's journey and is not. Every stage has a name and a role (`lifecycle_stage_unnamed`); stage ids unique, 1–30 stages (`lifecycle_stages_invalid`); every stage reachable from the first (`lifecycle_stage_unreachable`); no `next` to an earlier stage (`lifecycle_stage_backward`); choices 2–4 (`lifecycle_choices_invalid`); `sendsBackTo` names an earlier stage, `maxRounds` 1–10 (`lifecycle_send_back_invalid`); `answerWithinDays` 1–365 (`lifecycle_days_invalid`); a stage's `slackChannel`, when present, has an `id` matching `^[CG][A-Z0-9]{8,20}$` and a `name` of 1–80 characters not starting with `#` (`lifecycle_slack_channel_invalid`, 2C `LIFE_SLACK_CHANNEL_INVALID`; fix `ChangeItYourself { field: "slackChannel" }`; amended January 2026, ADR-102); each `agent` resolves to a WorkspaceProfile and each `produces` to an OutputContract in scope (`lifecycle_reference_unresolved`); each handover place reachable per `reachable_places`, keyed by the stage's `handoverPlace.target` (its `type` when the target is empty), a place the map does not name being not reported (`lifecycle_place_unreachable`, fix `ChangeItYourself { field: "handoverPlace" }`); where the next stage's agent declares the contract it consumes (the `consumes` value of the HandoffOverlay it offers by default, else of each overlay it offers), the stage hands on that contract (`sync_journey_seam_unmatched`, the earlier implementation's seam check applied to stages).

### D. Cross-checks and repository checks — `check_repository`, and as part of every draft check

9. **Stage one** on every part of the union (step 7); each refused part is a failure.
10. **Cross-checks** (port of `CrossCheck` over the union; `CrossCheckPartial` when `RepositoryCheck` is a partial tree — never in ordin's whole-repository check, ADR-091 item 10): control loops, journey seams, appliesTo (`sync_applies_to_unbound`, `sync_applies_to_malformed`, `sync_applies_to_unsatisfiable`), profile pack offers (`sync_pack_not_applicable`), profile overlay offers (`sync_overlay_not_applicable`), publish gate present (`sync_publish_gate_missing`), crew references and crew (`sync_crew_empty`: a crew with no `members`; `sync_crew_member_unresolved`: a member that is not an agent of the repository; `sync_crew_loop_override_invalid`: a crew `controlLoop` that is not a control loop of the repository; CrewProfile and AgentIdentity are read and checked, never offered, ADR-090 items 8–9), memory, control-loop terms, crew loop override. For a change, report only findings not present in the pre-change snapshot (port of `ValidateCandidate`'s `preexisting` filter); pre-existing ones are logged `checker.preexisting_finding`.
11. **References** (port of `CrossCheckReferences`): every reference of every changed part (or every part, for a read) resolves within the union first, then through `PartSource` with `scope`; unresolved → `reference_unresolved`; `Resolution::Ambiguous` → `reference_ambiguous` (ADR-090 §2: refused for that part, reported to Diagnostics).
12. **Ingest rules** (port of `syncworker` steps ADR-091 cites): `metadata.supersedes` names no part in the union → `sync_unknown_supersedes_target`; two Guideline parts for one target kind (body key `targetKind`) → `sync_duplicate_guideline_target_kind`; a part citing a vocabulary term not in the set → `sync_unknown_vocabulary_term`.
13. **Agents that use the change** (ADR-091 item 10, R-80): for each changed part, `source.used_by(part)`; for each agent, `resolver.resolve` with the change staged and `allow_draft_root = false`, then `resolver.conflicts` over its `pack_space`. A `ResolveError` that is a finding → `agent_rebuild_failed { agent, reason code }`, failure, `message_id = check-agent-would-not-pass`, `args.agent` = the agent's name; a conflict → `parts_conflict` (step 18). One finding `repository_checked` (warning-free marker, severity Warning only when other repositories' warnings exist) carries "Checked with the whole of [display name]" (11:890) as `message_id = check-whole-repository` with `args.repository`.

### E. Draft checks — `check_agent_draft`, `check_part_draft`

14. **Wording rules** (ADR-085), applied before every other draft check to `name`, `purpose`, each job's text, each task, `will`, `wont`, `needs`, and each changed part's `metadata.description`, when `agent_language` is `en-GB` or starts with `en-GB` (other languages: only the whitespace and length rules):
    - **W1 whitespace**: trim; collapse runs of spaces and tabs to one space → a `WordingChange` when changed.
    - **W2 British spelling**: whole-word, case-preserving replacement from a compiled-in table — for the stems `organiz`, `analyz`, `recogniz`, `prioritiz`, `summariz`, `optimiz`, `minimiz`, `maximiz`, `customiz`, `authoriz`, `finaliz`, `utiliz`, `standardiz`, `realiz`, `categoriz`, `characteriz`, `emphasiz`, `normaliz`, `synchroniz`, `visualiz` the forms `-e`, `-es`, `-ed`, `-ing`, `-ation`, `-ations`, `-er`, `-ers` become `-is…`; and the words `color/colour`, `behavior/behaviour`, `favorite/favourite`, `honor/honour`, `labor/labour`, `center/centre`, `catalog/catalogue`, `defense/defence`, `gray/grey`, `modeling/modelling`, `modeled/modelled`, `canceled/cancelled`, `canceling/cancelling`, `traveling/travelling`, `labeled/labelled`, `labeling/labelling`, `fulfill/fulfil`, `enrollment/enrolment`, `license` (noun position not detectable) excluded (*table chosen here*). Words inside back-ticks, URLs (`https?://\S+`) and identifiers (`urn:ordin:\S+`) are never changed.
    - **W3 dates**: `\b(\d{4})-(\d{2})-(\d{2})\b` with a valid calendar date → `D Month YYYY` in English month names (`January 2026` → `19 January 2026`); `\b\d{1,2}/\d{1,2}/\d{2,4}\b` → finding `date_ambiguous` (failure, fix `ChangeItYourself`), because day and month order cannot be known.
    - **W4 numbers**: an integer of five or more digits in prose, not part of an identifier, URL, code span or a longer token → comma thousands groups (`12500` → `12,500`).
    - **W5 task names start with a verb**: the first word of each task, lower-cased, must be in the compiled-in list of imperative verbs (`add, analyse, answer, apply, approve, archive, ask, assess, build, calculate, change, check, choose, clean, collect, compare, compile, confirm, count, create, decide, define, delete, describe, design, draft, estimate, explain, export, extract, file, find, fix, flag, format, gather, generate, group, import, improve, inspect, list, map, mark, match, measure, merge, monitor, move, note, open, order, plan, prepare, price, publish, rank, rate, read, record, reconcile, refresh, remove, rename, report, request, review, revise, rewrite, run, schedule, score, search, send, sort, split, summarise, tag, test, track, translate, update, validate, verify, write`, *list chosen here*); a lower-case first letter of a listed verb is capitalised (a `WordingChange`); a first word ending in `ing` whose stem or stem + `e` is listed becomes that verb (`Comparing` → `Compare`); otherwise finding `task_name_not_verb` (failure; fixes `AskOrdin`, `ChangeItYourself { field: "tasks" }`).
    - **W6 length limits** (LD-21): name ≤ 80, purpose ≤ 500, ≤ 50 items per list, each ≤ 500 characters → finding `text_too_long` (failure; fix `ChangeItYourself`). Not truncated.
    - **W7 vocabulary**: a cited term whose state is `retired` → finding `vocabulary_term_retired` (failure; fix `ChangeItYourself`; no automatic replacement — see HLD disagreement B3-D4).
    The tidied text replaces the input for every later check; the changes are returned in `tidied`.
15. **Mapping report** (imports, ADR-094): each item → `no_place_in_agent` (failure; `message_id = check-no-place-in-agent`, `args.what`; fixes `LeaveItOut { what }`, `AskOrdin`) (11 §5.30).
16. **Parts and repository**: sections C and D over the draft's staged parts in the target repository.
17. **Trust policy** (ADR-082, ADR-086, ADR-094 item 5): from the staged parts derive the executable parts (step 31); each not trusted by `TrustPolicy::from_profile_value` over the current profile's `agent_catalogue.trust_policy` → `trust_needs_it_approval` (failure; `args.tool`; fixes `LeaveItOut { what: name }`, `AskIt`). A ToolBinding binding of `type: builtin` whose `name` is not in a non-empty `allowed_builtin_tools` → `tool_not_allowed` (failure; fixes `LeaveItOut`, `AskIt`).
18. **Conflicts** (ADR-088): `resolver.pack_space(req)` then `resolver.conflicts(req, space)` with the draft staged and `allow_draft_root = true`; each conflict → `parts_conflict` (failure; `message_id = check-parts-disagree`, `args.what` = the slot's part name, `args.part_a`, `args.part_b` = the requesting parts' names; fix `AskAgentMaker`) — "Two parts of this agent say different things about [what]: [part] and [part]" (11:821).
19. **Fixed packs** (ADR-089): for each required rule, count fixed `Pack` entries that are candidates of the rule (as the Resolver defines candidates); the rule passes when the count ≥ `at_least`, or a fixed `Ask` entry matches the rule; else `fixed_packs_unsatisfied` (failure; `args.class` = the rule's class in plain words from the vocabulary label; fixes `ChoosePacks { rule_index, choices }`, `LetAgentAsk { rule_index }`) — "Choose which [class] this agent uses when the workspace doesn't say" (11:822).
20. **Jobs covered**: each index of `draft.jobs` must have a non-empty entry in `job_tasks`, each named task present in `draft.tasks` → else `job_not_covered` (failure; fixes `AskOrdin`, `ChangeItYourself { field: "tasks" }`).
21. **Name collision**: `draft.name` equal, ignoring case and surrounding spaces, to a name in `repository_agent_names` → `name_collision` (failure; fix `ChangeItYourself { field: "name" }`).
22. **Format** (ADR-087, SA-APM-01): each `loss_report` item → `format_loss` (failure; `message_id = check-format-cannot-hold`, `args.format = "APM"`, `args.what`; fixes `LeaveItOut`, `PublishToOrdinFormat`) — "APM format can't hold [what], so it would be left out" (11:840); `target_format = Apm` with non-empty `borrowed` → `borrowed_apm_unsupported` (failure; fix `PublishToOrdinFormat`).
23. **Borrowed parts reachable** (ADR-090 §2): each borrowed identifier must resolve in the target repository's own scope (its profile repositories or the bundled catalogue) — a part only in a person-added repository fails `borrowed_unreachable` (failure; fix `AskAgentMaker`). A borrowed part with status `deprecated` → `deprecated_part_reused` (warning; ADR-090 §6 "the agent maker does not reuse it").
24. **Bare commands** (R-94): in each Markdown body of the draft's parts, a token `/<slug>` (preceded by start of line or whitespace, followed by whitespace, punctuation or end) where `<slug>` is one of the draft's command slugs and is not written `/<agent>:<slug>` → `bare_command_name` (warning; `args.command`; fix `FixIt { action: PointToReplacement { field: <part id>, from: "/<slug>", to: "/<agent>:<slug>" } }`).
25. **Publish gate** (ADR-090 §4): when the root's `governanceMetadata.publishGate` is set — `rehearsal` None → `publish_gate_not_met` (failure; `args.reason = "not rehearsed"`); any outcome with `met = false` → `publish_gate_not_met` with the failed rule in `detail`. A part draft has no publish gate.
26. **Identifier fixes** (ADR-094 item 4, "a name turned into a valid identifier"): an `id_grammar_invalid` finding carries `FixIt { MintIdentifier { field: "metadata.id", value: mint_identifier(kind, publisher, None, metadata.name) } }`; `metadata_required` carries `FixIt { AskMissingField { field } }`; an unknown top-level key reported by stage one carries `FixIt { DropUnknownField { field } }` (applied after the person agrees, by the Authoring Service).

### F. Withdrawal and vocabulary — `check_withdrawal`, `check_vocabulary`

27. **Withdrawal** (ADR-091 item 3, amended by UI-D99): `proposed_list` must equal `existing_entries` followed by exactly one new entry equal to `entry`, comparing the fields the withdrawn-list file holds (kind, identifier, version, digest, reason, time withdrawn, replacement) and not the row identifier, repository or read times the source adds → else `withdrawn_list_not_append` (failure; `WDL_APPEND_ONLY`); a second entry for an already withdrawn (kind, identifier, version) or for every version when one exists without version → `withdrawn_entry_duplicate`; the target part must exist in the repository → `withdrawn_target_missing`; `part_version` and `part_content_digest` both present or both absent (`WDL_VERSION_DIGEST_MISMATCH`) and the digest must equal the part's → `withdrawn_digest_mismatch`; `reason` 1–1,000 characters → `withdrawn_reason_invalid`; a replacement must resolve to a part of the same kind, neither deprecated nor withdrawn, reachable from every agent that uses the withdrawn part under ADR-090 §2 → else `withdrawn_replacement_invalid`. Then for each agent in `used_by`: resolve with a `Replacement` (when one is named) and run step 18; an agent that does not pass is added to `agents_that_stop` and to a `agent_stops_on_withdrawal` finding of severity **Warning** — it never fails the withdrawal (UI-D99). With no replacement, every using agent is in `agents_that_stop`.
28. **Vocabulary** (ADR-091 item 4): an added code must match `[a-z0-9-]{1,64}`, not exist on the axis (`VOCAB_CODE_INVALID` → `vocabulary_code_invalid`), label 1–120, description ≤ 1,000; retiring `draft`, `approved` or `deprecated` on `artefact-status` → `vocabulary_status_term_fixed` (failure); retiring a code cited by a part of the same repository → `vocabulary_term_cited` (failure; `args.parts` = the citing parts' names, comma-joined, at most 10 then "and N more"); cited by a part in another repository ordin reads (`source.citing_parts(repository, axis, code)`, keeping the parts whose repository is not this one) → `vocabulary_term_cited_elsewhere` (warning). A `PartSourceError` from `citing_parts` → `CHK_SOURCE_UNAVAILABLE` (retryable as the source says). Then section D for the whole repository with the changed vocabulary part.

### G. On-demand, trust and rehearsal

29. **`check_installed`** (ADR-094 item 3): `resolver.resolve_lock(lock, scope)` gives the parts in use without writing a bundle (a `RES_LOCK_PART_UNAVAILABLE` becomes the finding `installed_part_gone`, failure, fix `AskOrdin`); run section C on each part, step 11 (references) within the lock's parts, step 17 (trust, against the current profile), and step 18 (conflicts among the installed parts). `version_checked` = the lock's plan hash. Withdrawn parts → `installed_part_withdrawn` (failure; with a replacement, fix `FixIt { PointToReplacement }` — applied by making **your own fixed copy** through the Authoring Service, ADR-094 item 4, never in place). With no finding the screen shows "Every check passed." (11 §5.30).
30. **`check_library_item`**: resolve the item through `PartSource`; for a WorkspaceProfile, resolve it with the Resolver (`allow_draft_root = false`, the person's current workspace languages are not needed: `workspace_languages` empty and every pending choice tolerated) and run sections C, D (references and cross-checks for its closure), step 17 and step 18; for another kind, section C and step 11.
31. **Executable parts** (`decide_executable_parts`, 04:52): derive from an installed bundle's tool policy and parts — each `mcp_servers` entry → `McpServer { part_name: name, declared_by }`; each ToolBinding binding whose `type` is neither `mcp` nor `builtin` → `BundledProgram { part_name: binding name }`; each part loaded with mode `event` → `Hook { part_name: part identifier }`. Duplicate (part_name, kind) → keep the first (`EXP_PART_DUPLICATE` is never stored). Decision: `Trusted` when the profile's `TrustPolicy` lists the declaring ToolBinding id with the program or server name in `programs`, or lists the hook id; otherwise `NeedsItApproval`. Replace the instance's rows in one transaction, `source = organisation_policy`, `decided_at = now`. Append a journal record of kind `executable_parts_decided` `{agent_instance_id, trusted, needs_it_approval}`.
32. **`reevaluate_trust`**: run on every new verified profile (the program subscribes to `OrganisationProfileService::subscribe()` and calls this). For each instance in `executable_part_subjects`, recompute step 31's decision; instances where any part moved `trusted → needs_it_approval` go to `lost` (the Package Catalogue disables them until IT approves, 04:52), the reverse to `gained`; rows updated; one journal record of kind `trust_reevaluated` with the two lists.
33. **`check_rehearsal`** (pure): `ResultContains` — case-insensitive substring of `final_message`; `ResultHasSection` — a Markdown heading line (`#` to `######`) whose text equals `heading` ignoring case and surrounding spaces; `ToolUsed`/`ToolNotUsed` — membership in `tools_used`; `ApprovalAsked` — `approvals_asked > 0`; `NoExternalAction` — `external_actions == 0`; `ArtefactProduced` — an artefact name ending with `name_suffix`. `evidence` is one plain line ("tool Bash used 3 times"), never message content beyond 120 characters. A model-judged expectation is not an `ExpectationRule` and is never checked here (ADR-085: opinions).

### H. Install, handover, dossier and package checks

34. **`check_for_install`**: subject `InstallCandidate { root_id: plan.root.id, plan_hash }`. For each `PartRef` in `plan.resolved`, resolve it through the selected source (`resolve(scope, kind, id, Some(version))`; `NotFound` → finding `installed_part_gone`) and run section C; run step 17 with the executable parts derived as step 31 from `plan.tool_policy` and the parts. `verdict` = `Failed` when any finding is a Failure other than `trust_needs_it_approval`; else `NeedsItApproval { parts }` listing the untrusted executable parts when any; else `Clean`. Record the run (step 3). No `executable_part_decisions` row is written (O137: decisions are stored by `decide_executable_parts` once the folder exists). With `agent_instance_id = Some(id)`, in the same transaction as the run, upsert `install_candidate_parts (agent_instance_id = id, plan_hash, parts = JSON of every derived executable part in derivation order, check_run_id, recorded_at = now)` (SA-CLOSE-03).
34a. **`executable_parts(id)`**: select the `install_candidate_parts` row of `id`; none → `CHK_INSTANCE_UNKNOWN` (permanent); a value that does not parse → `CHK_DATASTORE_FAILED { retryable: false }`; else the stored list. Reads no part and needs no bound source.
35. **`check_handover`** (ADR-092 item 2, ADR-095; codes as HandoverService.md:145): `results_readable` — one finding per result, failing when `local_only` or `readable_by_receiver == Some(false)`, `item` = the link; `contract_sections` — with a contract, resolve it as kind OutputContract (`NotFound` or `Ambiguous` → one failing finding with `item` = the identifier); read its sections (step 38); for each result with `headings`, normalise each heading as step 37 and list the required, non-derived sections absent in `missing_sections`; a result with `headings = None` fails with `item` = the link; with no contract, no `contract_sections` finding; `receiver_reachable` — one finding, passing when `receiver.reachable`. `passed` = every finding passed. Not recorded (the Handover Service keeps the passed codes on its handover record). `PartSourceError::Unavailable` → `CHK_SOURCE_UNAVAILABLE`.
36. **`read_dossier`** (port of the earlier implementation's dossier `Reader::Parse`): `contract.kind` not OutputContract → `CHK_CONTRACT_INVALID { reason: "not_output_contract" }`; read the sections (step 38); none → `CHK_CONTRACT_INVALID { reason: "no_sections" }`.
37. Walk `write_up` line by line: a line matching `^#{1,6}[ \t]*(.*)$` is a heading; normalise its text (lower-case, trim, each run of `[^a-z0-9]+` → `-`, trim `-`); a name that is a contract section starts (or continues) that section's block; any other name is added once to `unmatched_headings` and stops the current block. Other lines append to the current block's text; a line matching `^[ \t]*([-*]|[0-9]+\.)` adds one to its `evidence_count`. Text before the first heading is dropped. Walking the contract's sections in order: a section with no block → `missing_sections`; otherwise a `DossierSection` with `evidence_count` raised to 1 when it is 0 and the text is not blank, `low_confidence = required && evidence_count < evidenceThreshold`. The text is data: no link is followed and nothing is executed.
38. **Contract sections**: `OutputContract::read(&contract.manifest)` (Agent Model; `name` required, `meaning` default empty, `required` default false, `evidenceThreshold` default 0, `derived` default false); `AM_FIELD_SHAPE_INVALID` → `CHK_CONTRACT_INVALID { reason: "section_malformed" }`; then `.sections()`.
39. **`check_parts`** (Release Builder): `ordin_resolver::compile_instructions(source, request)` gives the plan's lock file (a `ResolveError` becomes one stopping finding with `file = ""`, `entry` = its code, and the check returns at once; `RES_SOURCE_UNAVAILABLE` → `CHK_SOURCE_UNAVAILABLE`); resolve each lock artefact through `source` (`resolve(scope, kind, id, Some(version))`); run section C on every resolved part and section D steps 9–12 over them, with the vocabulary from `source.vocabulary(scope)`. Each finding → `PackageFinding { stop: severity == Failure, file: <the part's Markdown or YAML file holding its manifest, from `source.files(part)`>, entry: part_identifier or code, message: detail }`, in the order produced. No datastore, journal or profile access.

### I. Words shown

40. The Checker owns these Fluent message ids (*chosen here*; text from 11): `check-part-invalid`, `check-parts-disagree` ("Two parts of this agent say different things about { $what }: { $part_a } and { $part_b }"), `check-choose-packs` ("Choose which { $class } this agent uses when the workspace doesn't say"), `check-format-cannot-hold` ("{ $format } format can't hold { $what }, so it would be left out"), `check-no-place-in-agent` ("{ $what } has no place in an ordin agent"), `check-whole-repository` ("Checked with the whole of { $repository }"), `check-agent-would-not-pass`, `check-trust-needs-it-approval`, `check-tool-not-allowed`, `check-job-not-covered`, `check-name-collision`, `check-task-name-not-verb`, `check-text-too-long`, `check-date-ambiguous`, `check-borrowed-unreachable`, `check-borrowed-apm-unsupported`, `check-bare-command-name`, `check-publish-gate-not-met`, `check-installed-part-gone`, `check-installed-part-withdrawn`, `check-withdrawal-invalid`, `check-agent-stops`, `check-vocabulary-term-cited`, `check-vocabulary-status-fixed`, `check-lifecycle-invalid`, `check-every-check-passed` ("Every check passed."), `check-tidied` ("ordin tidied { $count } things in the wording"). The cross-checks, the reference and ingest checks, `vocabulary_code_invalid`, `vocabulary_term_retired`, `deprecated_part_reused` and `findings_truncated` have no message of their own and use `check-part-invalid` with `args.part`. The heading "Checked by ordin" and its line "Fixed rules, the same answer every time. No agent decides these." belong to the screens (11:820).

## Data Model

Tables in the journal database, written only by the Checker (2B-1 item 5); `migrations()` returns `ModuleMigrations { module: "checker", .. }` with migration version 1 (SA-RECON-05). The Checker reads no other module's table.

SQLite:

```sql
CREATE TABLE check_runs (
    id               TEXT PRIMARY KEY CHECK (length(id) = 36),
    subject_kind     TEXT NOT NULL CHECK (subject_kind IN ('repository_read','agent_draft','part_draft',
                         'withdrawal','vocabulary_change','installed_agent','library_item','rehearsal',
                         'install_candidate')),
    subject          TEXT NOT NULL,             -- CheckSubject as JSON
    version_checked  TEXT NOT NULL CHECK (length(version_checked) BETWEEN 1 AND 512),
    passed           INTEGER NOT NULL CHECK (passed IN (0, 1)),
    failures         INTEGER NOT NULL CHECK (failures >= 0),
    warnings         INTEGER NOT NULL CHECK (warnings >= 0),
    tidied           TEXT NOT NULL,             -- Vec<WordingChange> as JSON
    agents_that_stop TEXT NOT NULL,             -- Vec<String> as JSON
    started_at       TEXT NOT NULL,
    finished_at      TEXT NOT NULL
) STRICT;
CREATE INDEX check_runs_subject ON check_runs (subject_kind, finished_at);

CREATE TABLE check_findings (
    check_run_id      TEXT NOT NULL REFERENCES check_runs(id) ON DELETE CASCADE,
    ordinal           INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 500),
    code              TEXT NOT NULL CHECK (length(code) BETWEEN 1 AND 64),
    severity          TEXT NOT NULL CHECK (severity IN ('failure', 'warning')),
    part_identifier   TEXT CHECK (part_identifier IS NULL OR length(part_identifier) <= 512),
    message_id        TEXT NOT NULL,
    args              TEXT NOT NULL,            -- JSON object of strings
    fixes             TEXT NOT NULL,            -- Vec<Fix> as JSON
    detail            TEXT NOT NULL CHECK (length(detail) <= 2000),
    PRIMARY KEY (check_run_id, ordinal)
) STRICT;

CREATE TABLE executable_part_decisions (
    id                TEXT PRIMARY KEY CHECK (length(id) = 36),
    agent_instance_id TEXT NOT NULL CHECK (length(agent_instance_id) = 36),
    part_name         TEXT NOT NULL CHECK (length(part_name) BETWEEN 1 AND 200),
    kind              TEXT NOT NULL CHECK (kind IN ('hook', 'mcp_server', 'bundled_program')),
    decision          TEXT NOT NULL CHECK (decision IN ('trusted', 'needs_it_approval')),
    source            TEXT NOT NULL CHECK (source = 'organisation_policy'),
    decided_at        TEXT NOT NULL,
    UNIQUE (agent_instance_id, kind, part_name)
) STRICT;

-- What each decision was made from, so it can be re-decided when the profile changes.
CREATE TABLE executable_part_subjects (
    decision_id  TEXT PRIMARY KEY REFERENCES executable_part_decisions(id) ON DELETE CASCADE,
    declared_by  TEXT NOT NULL CHECK (length(declared_by) BETWEEN 1 AND 512)
) STRICT;

-- SA-CLOSE-03: the complete executable-part list of an instance's latest install check.
CREATE TABLE install_candidate_parts (
    agent_instance_id TEXT PRIMARY KEY CHECK (length(agent_instance_id) = 36),
    plan_hash         TEXT NOT NULL CHECK (length(plan_hash) BETWEEN 1 AND 80),
    parts             TEXT NOT NULL,            -- Vec<ExecutablePart> as JSON
    check_run_id      TEXT NOT NULL CHECK (length(check_run_id) = 36),
    recorded_at       TEXT NOT NULL
) STRICT;
```

PostgreSQL (agent profile scaled shape; the Release Builder checks there). The statements are written once, for SQLite: on a PostgreSQL datastore every recorded operation fails with the permanent `CHK_DATASTORE_FAILED`; `check_parts` uses no datastore.

```sql
CREATE TABLE check_runs (
    id UUID PRIMARY KEY,
    subject_kind TEXT NOT NULL CHECK (subject_kind IN ('repository_read','agent_draft','part_draft',
        'withdrawal','vocabulary_change','installed_agent','library_item','rehearsal',
        'install_candidate')),
    subject JSONB NOT NULL,
    version_checked TEXT NOT NULL CHECK (char_length(version_checked) BETWEEN 1 AND 512),
    passed BOOLEAN NOT NULL,
    failures INTEGER NOT NULL CHECK (failures >= 0),
    warnings INTEGER NOT NULL CHECK (warnings >= 0),
    tidied JSONB NOT NULL,
    agents_that_stop JSONB NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    finished_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX check_runs_subject ON check_runs (subject_kind, finished_at);
CREATE TABLE check_findings (
    check_run_id UUID NOT NULL REFERENCES check_runs(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 500),
    code TEXT NOT NULL CHECK (char_length(code) BETWEEN 1 AND 64),
    severity TEXT NOT NULL CHECK (severity IN ('failure', 'warning')),
    part_identifier TEXT CHECK (part_identifier IS NULL OR char_length(part_identifier) <= 512),
    message_id TEXT NOT NULL,
    args JSONB NOT NULL,
    fixes JSONB NOT NULL,
    detail TEXT NOT NULL CHECK (char_length(detail) <= 2000),
    PRIMARY KEY (check_run_id, ordinal)
);
CREATE TABLE executable_part_decisions (
    id UUID PRIMARY KEY,
    agent_instance_id UUID NOT NULL,
    part_name TEXT NOT NULL CHECK (char_length(part_name) BETWEEN 1 AND 200),
    kind TEXT NOT NULL CHECK (kind IN ('hook', 'mcp_server', 'bundled_program')),
    decision TEXT NOT NULL CHECK (decision IN ('trusted', 'needs_it_approval')),
    source TEXT NOT NULL CHECK (source = 'organisation_policy'),
    decided_at TIMESTAMPTZ NOT NULL,
    UNIQUE (agent_instance_id, kind, part_name)
);
CREATE TABLE executable_part_subjects (
    decision_id UUID PRIMARY KEY REFERENCES executable_part_decisions(id) ON DELETE CASCADE,
    declared_by TEXT NOT NULL CHECK (char_length(declared_by) BETWEEN 1 AND 512)
);
CREATE TABLE install_candidate_parts (
    agent_instance_id UUID PRIMARY KEY,
    plan_hash TEXT NOT NULL CHECK (char_length(plan_hash) BETWEEN 1 AND 80),
    parts JSONB NOT NULL,
    check_run_id UUID NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL
);
```

Retention: check runs of a subject older than its latest 20 runs are deleted after each insert for that subject (*assumed here*; the draft and Diagnostics show only the latest); runs referenced by a draft's `check_results_ref` are the latest. `forget_instance` deletes the instance's decisions and its `install_candidate_parts` row. Journal record kinds (*chosen here*): `checker.check_run_completed`, `checker.executable_parts_decided`, `checker.trust_reevaluated`. The 2C storage line for `executable_part_decisions` names the Package Catalogue as owner; this spec follows 02:449 (revision 0.48.0, the later statement, SA-HLDCONFLICT-01) — recorded as HLD disagreement B3-D3. Rollback: migration 1 only creates tables.

## Error Table

Findings are results, not errors: a failing check returns `Ok(CheckReport { passed: false, … })`. The errors below are failures to run a check.

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `PartSource` cannot read the clone or index | `Err(retryable)` | `CHK_SOURCE_UNAVAILABLE` | `CheckError::SourceUnavailable { retryable: true }` |
| `PartSource` reports an unreadable withdrawn list with no earlier copy | `Err(permanent)` | `CHK_SOURCE_UNAVAILABLE` | `CheckError::SourceUnavailable { retryable: false }` |
| Resolver and Compiler fails for a reason that is not a finding (source, store) | `Err(retryable)` / `Err(permanent)` | `CHK_RESOLVER_FAILED` | `CheckError::ResolverFailed { code, retryable }` |
| Request malformed (job index outside `draft.jobs`, empty `entry.reason` with no list, a draft whose parts hold no WorkspaceProfile, `field` = `parts`). The axis is the closed `VocabularyAxis`, so an unknown axis cannot be written | `Err(permanent)` | `CHK_INPUT_INVALID` | `CheckError::InputInvalid { field }` |
| Subject unreadable even for stage one (no part at all in a draft) | `Err(permanent)` | `CHK_TOO_MANY_FINDINGS` | `CheckError::TooManyFindings` |
| `report` for an unknown or pruned run | `Err(permanent)` | `CHK_RUN_NOT_FOUND` | `CheckError::RunNotFound { id }` |
| Datastore busy or locked | `Err(retryable)` | `CHK_DATASTORE_FAILED` | `CheckError::DatastoreFailed { retryable: true }` |
| Other datastore failure | `Err(permanent)` | `CHK_DATASTORE_FAILED` | `CheckError::DatastoreFailed { retryable: false }` |
| A part-reading call before `install_part_source` and without a per-request source | `Err(permanent)` | `CHK_SOURCE_NOT_BOUND` | `CheckError::SourceNotBound` |
| `install_part_source` called a second time | `Err(permanent)` | `CHK_SOURCE_ALREADY_BOUND` | `CheckError::SourceAlreadyBound` |
| `executable_parts` for an instance with no install check stored | `Err(permanent)` | `CHK_INSTANCE_UNKNOWN` | `CheckError::InstanceUnknown { id }` |
| `read_dossier` given a part that is not an OutputContract, has no sections, or has a section without a name | `Err(permanent)` | `CHK_CONTRACT_INVALID` | `CheckError::ContractInvalid { reason }` (`not_output_contract`, `no_sections`, `section_malformed`) |

Finding codes the Checker emits (results, Severity in brackets): the earlier implementation's verbatim — `apiversion_unknown`, `kind_unknown`, `metadata_required`, `descriptive_metadata_missing`, `vocabulary_unknown_term`, `pack_toolchain_invalid`, `id_grammar_invalid`, `loading_invalid`, `requiredpacks_invalid`, `assay_policy_invalid`, `memory_content_class_invalid`, `memory_retains_empty`, `memory_class_conflict`, `memory_retention_invalid`, `memory_store_unresolved`, `memory_cardinality`, `sync_control_loop_missing_strategy`, `sync_control_loop_retired_field`, `sync_unknown_loop_strategy`, `sync_unknown_runtime_capability`, `sync_invalid_loop_trigger`, `sync_control_loop_mode_escape`, `sync_journey_seam_unmatched`, `sync_applies_to_unbound`, `sync_applies_to_malformed`, `sync_applies_to_unsatisfiable`, `sync_pack_not_applicable`, `sync_overlay_not_applicable`, `sync_publish_gate_missing`, `sync_crew_empty`, `sync_crew_member_unresolved`, `sync_crew_loop_override_invalid`, `reference_unresolved`, `sync_unknown_supersedes_target`, `sync_duplicate_guideline_target_kind`, `sync_duplicate_vocabulary_axis`, `sync_unknown_vocabulary_term` [failure]; ordin's — `reference_ambiguous`, `lifecycle_*`, `agent_rebuild_failed`, `parts_conflict`, `fixed_packs_unsatisfied`, `job_not_covered`, `name_collision`, `trust_needs_it_approval`, `tool_not_allowed`, `format_loss`, `borrowed_apm_unsupported`, `borrowed_unreachable`, `no_place_in_agent`, `publish_gate_not_met`, `task_name_not_verb`, `text_too_long`, `date_ambiguous`, `vocabulary_term_retired`, `withdrawn_list_not_append`, `withdrawn_entry_duplicate`, `withdrawn_target_missing`, `withdrawn_digest_mismatch`, `withdrawn_reason_invalid`, `withdrawn_replacement_invalid`, `vocabulary_code_invalid`, `vocabulary_status_term_fixed`, `vocabulary_term_cited`, `installed_part_gone`, `installed_part_withdrawn`, `findings_truncated` [failure]; `deprecated_part_reused`, `bare_command_name`, `agent_stops_on_withdrawal`, `vocabulary_term_cited_elsewhere`, `repository_checked` [warning].

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Checker

  Scenario: Parity — the frozen reference fixtures give the same findings
    Given each fixture of the earlier implementation's authoringcheck, compositioncheck and artefactbuild tests
    When the ported check runs on the same input
    Then the set of finding codes equals the frozen reference's

  Scenario: Happy path — a clean draft passes and the wording is tidied
    Given a draft whose purpose reads "Analyzes contract clauses" and whose every job has a task
    And every part passes and every tool is trusted
    When check_agent_draft is called
    Then the report passes
    And tidied holds "Analyzes" → "Analyses"

  Scenario: Error — tool outside the trust policy
    Given a draft whose ToolBinding declares the MCP server "jira-reader" and a trust policy that does not list it
    When check_agent_draft is called
    Then a finding "trust_needs_it_approval" fails with fixes "leave_it_out" and "ask_it"

  Scenario: Error — two parts disagree
    Given a draft whose two allowed testing packs bring different versions of one skill under a rule needing two packs
    When check_agent_draft is called
    Then a finding "parts_conflict" fails with the fix "ask_agent_maker"

  Scenario: Error — the maker's fixed packs do not settle a required rule
    Given a draft requiring at least 1 stack pack, with no fixed stack pack and no ask entry
    When check_agent_draft is called
    Then a finding "fixed_packs_unsatisfied" fails with fixes "choose_packs" and "let_agent_ask"

  Scenario: Error — APM cannot hold part of the draft
    Given a loss report naming "fixedPracticePacks" for an APM target
    When check_agent_draft is called
    Then a finding "format_loss" fails with fixes "leave_it_out" and "publish_to_ordin_format"

  Scenario: Error — publish gate not met
    Given an agent whose publishGate is set and a rehearsal with one expectation not met
    When check_agent_draft is called
    Then a finding "publish_gate_not_met" fails

  Scenario: Edge — bare command name is only a warning
    Given a skill body that says "run /apply next" and the agent "cspa-431" has a command "apply"
    When check_agent_draft is called
    Then a warning "bare_command_name" offers to point "/apply" to "/cspa-431:apply"
    And the report still passes when nothing else fails

  Scenario: Edge — a change rebuilds every agent that uses the part
    Given a part draft changing a rule used by agents A and B, and B would then hold two control loops
    When check_part_draft is called
    Then a finding "agent_rebuild_failed" names agent B
    And the report fails

  Scenario: Edge — a withdrawal is never blocked by agents that would stop
    Given a withdrawal with a replacement, where agent C does not pass with the replacement
    When check_withdrawal is called
    Then the report passes
    And agents_that_stop lists agent C with a warning "agent_stops_on_withdrawal"

  Scenario: Error — a withdrawal that edits an earlier entry
    Given a proposed withdrawn list whose second entry's reason differs from the existing one
    When check_withdrawal is called
    Then a finding "withdrawn_list_not_append" fails

  Scenario: Error — retiring a status term
    When check_vocabulary is called retiring "deprecated" on axis artefact-status
    Then a finding "vocabulary_status_term_fixed" fails

  Scenario: Happy path — Check it on an installed agent
    Given an installed agent whose lock-file parts are all present, approved and trusted
    When check_installed is called
    Then the report passes and version_checked is the lock file's plan hash

  Scenario: Edge — trust lost when the profile changes
    Given an instance whose MCP server "files" was trusted
    When a new profile without "files" becomes active and reevaluate_trust is called
    Then the instance is listed in lost and its decision row reads "needs_it_approval"

  Scenario: Edge — rehearsal expectations
    Given the expectation ToolNotUsed "Bash" and evidence with tools_used ["Read"]
    When check_rehearsal is called
    Then the outcome is met

  Scenario: Error — a check before the source is bound
    Given a Checker built with new and no install_part_source call
    When check_repository is called with no per-request source
    Then it fails with CHK_SOURCE_NOT_BOUND, retryable() is false and nothing is recorded

  Scenario: Happy path — a trial is checked through the draft source
    Given a plan for the person's draft agent and the Catalogue Service's draft source for its worktree
    And every tool of the draft is trusted
    When check_for_install is called with source Some(draft source)
    Then the verdict is clean and a run with subject install_candidate is recorded
    And no executable-part decision is stored

  Scenario: Edge — install needs IT approval
    Given a plan whose tool policy names the MCP server "jira-reader" and a trust policy that does not list it
    When check_for_install is called
    Then the verdict is needs_it_approval listing "jira-reader"

  Scenario: Happy path — the complete executable-part list of an instance
    Given a plan whose tool policy names the MCP servers "files" (trusted) and "jira-reader" (not trusted)
    When check_for_install is called with agent_instance_id Some("I1")
    Then the verdict is needs_it_approval listing only "jira-reader"
    And executable_parts("I1") returns both "files" and "jira-reader"

  Scenario: Error — executable parts of an instance never checked
    When executable_parts is called for an instance with no stored install check
    Then it fails with CHK_INSTANCE_UNKNOWN and retryable() is false

  Scenario: Error — a contract section without a name
    Given an OutputContract whose second section has no name
    When read_dossier is called
    Then it fails with CHK_CONTRACT_INVALID reason "section_malformed"

  Scenario: Error — a handover result left on this computer
    Given a handover whose only result is on this computer only and a reachable receiver
    When check_handover is called
    Then results_readable fails for that result and receiver_reachable passes
    And the report does not pass

  Scenario: Edge — handover result missing a contract section
    Given the stage's contract requires the sections "summary" and "risks"
    And the result's headings are "Summary" and "Next steps"
    When check_handover is called
    Then contract_sections fails with missing_sections ["risks"]

  Scenario: Happy path — reading a role dossier
    Given the role-dossier contract and a write-up with the headings "Role identity" and "Triggers & inputs" and one list item under each
    When read_dossier is called
    Then sections hold "role-identity" and "triggers-inputs", each with evidence_count 1
    And missing_sections holds the other required sections

  Scenario: Error — package finding names its file
    Given a package whose rule file ".apm/instructions/tone.instructions.md" has no metadata id
    When check_parts is called
    Then one finding has stop true and file ".apm/instructions/tone.instructions.md"

  Scenario: Error — clone unavailable
    Given the PartSource reports Unavailable with retryable true
    When check_repository is called
    Then it fails with CHK_SOURCE_UNAVAILABLE and nothing is recorded
```

## Performance, Security, Observability

- **Performance targets:** `check_part` p95 ≤ 2 ms; `check_agent_draft` for a 200-part agent with a 30-pack space ≤ 3 s p95; `check_installed` (**Check it**) ≤ 2 s p95 for 200 parts; whole-repository `check_part_draft` over the 516-part bundled catalogue with 20 agents rebuilt ≤ 10 s p95; `reevaluate_trust` over 200 instances ≤ 500 ms. Memory ≤ 256 MB for the largest repository check.
- **Security:** every part and imported file is data (R-84): no check executes or follows text; regular expressions are the `regex` crate's linear-time engine (05 — Pattern matching); no model call decides anything (ADR-085) — model opinions are never inputs; no check can be waived (ADR-090): there is no API to suppress a finding; trust comes only from the signed organisation profile (`DecisionSource::OrganisationPolicy`, never the business user, 04:52); credential values never enter a finding (`env_ref` names only).
- **Observability:** `tracing` events (SA-OBS-01), never part text, file content or the person's words; findings logged by code and part identifier only:
  - `checker.check_run` (info): `check_run_id`, `subject_kind`, `passed`, `failures`, `warnings`, `tidied`, `parts`, `duration_ms`.
  - `checker.check_failed` (warn): `subject_kind`, `error_code`, `retryable`.
  - `checker.preexisting_finding` (debug): `code`, `part_identifier`.
  - `checker.executable_parts_decided` (info): `agent_instance_id`, `trusted`, `needs_it_approval`.
  - `checker.trust_reevaluated` (info): `instances`, `lost`, `gained`.
  - `checker.source_not_bound` (error): `operation`.
  - `checker.trust_policy_invalid` (error): no fields beyond the profile version.
  - `checker.handover_checked` (info): `passed`, `failed_codes`.
  Agent profile only: `checker.check` with fields `subject_kind`, `passed`, `parts` (no identifiers of people, SA-XC-05) is a local `tracing` span only, never exported (Tracing.md T-9: the exported spans are the five of the Telemetry allow list). No metrics.

## Gaps

None.

---

## Amendment January 2026 — Slack for working together (ADR-102)

> Source: decision 82; HLD 0.50.0, ADR-102.

### Internal Logic (change to the lifecycle check, step 8)

Step 8 (above) checks a stage's `slackChannel` form after `lifecycle_days_invalid` (`lifecycle_slack_channel_invalid`).

Nothing else is added: whether the company allows the channel, and whether the author can post in it, are not checked here. Posting falls back to email when Slack refuses (ADR-102), so a check that the Checker cannot settle in fixed code would only stop a publish for no effect on handovers. The handover checks (`check_handover`) are unchanged.

### Error Table / findings (row added)

| Condition | Finding code | Fix |
|---|---|---|
| A stage's `slackChannel` id or name malformed | `lifecycle_slack_channel_invalid` | `ChangeItYourself { field: "slackChannel" }` — "Pick the stage's Slack channel again, or remove it." |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: Checker — stage Slack channel

  Scenario: A valid stage channel passes
    Given a lifecycle draft whose stage "Design" has slackChannel { id: C04ABCDEF12, name: design-reviews }
    When the draft is checked
    Then no lifecycle_slack_channel_invalid finding is reported

  Scenario: A channel name typed with a hash
    Given a lifecycle draft whose stage has slackChannel { id: C04ABCDEF12, name: "#design-reviews" }
    When the draft is checked
    Then lifecycle_slack_channel_invalid is reported with the fix ChangeItYourself on field "slackChannel"

  Scenario: A stage without a channel is not checked for one
    Given a lifecycle draft whose stages carry no slackChannel
    When the draft is checked
    Then the lifecycle findings are those of step 8 before January 2026
```

### Performance, Security, Observability

Unchanged.

### Gaps

None.

---

## Amendment 2026-01-10 — the publish gate names a set of evaluations

> Status: Approved (user, 2026-01-10, "proceed with recommendations"). Source: ADR-090 §4; step 10's cross-check is a port of the earlier implementation's `crossCheckPublishGate`, whose meaning this amendment states, since the step above named the check without its rule.

### Internal Logic (change to the cross-checks, step 10)

Publish gate present (`sync_publish_gate_missing`) reads a WorkspaceProfile's `governanceMetadata.publishGate` as the name of a **set of evaluations**, not of one part. The rule, applied when the gate is set and is a part identifier:

1. An Evaluation of the repository (authored kind `evaluation` or `ci-gate`) is a member of the set when its `metadata.id` equals the gate, or begins with the gate followed by the separator of the gate's spelling — `:` for the `urn:ordin:` spelling, `/` for the `@` spelling. `urn:ordin:default:evals` therefore has `urn:ordin:default:evals:ci-gate` and `urn:ordin:default:evals:mode-boundary:observe-refuses-edit` as members; `urn:ordin:default:evalsuite` is not a member.
2. The check passes when the set has at least one member. It does not run the evaluations; whether they pass is step 25.
3. An empty set gives one `sync_publish_gate_missing` finding on the workspace.

A gate that is not a part identifier is not checked here, as before.

### Error Table / findings (row added)

| Condition | Finding code | Fix |
|---|---|---|
| A workspace's `publishGate` names a set with no Evaluation of the repository in it | `sync_publish_gate_missing` | `ChangeItYourself { field: "governanceMetadata.publishGate" }`; shown with `check-part-invalid` like every cross-check (step 40) |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: Checker — publish gate names a set of evaluations

  Scenario: A gate naming a set is met by an evaluation under that name
    Given a workspace whose publishGate is "urn:ordin:default:evals"
    And an evaluation "urn:ordin:default:evals:ci-gate" in the same repository
    When the repository is checked
    Then no sync_publish_gate_missing finding is reported

  Scenario: A gate naming one evaluation exactly is met
    Given a workspace whose publishGate is "urn:ordin:default:evals:ci-gate"
    And an evaluation "urn:ordin:default:evals:ci-gate" in the same repository
    When the repository is checked
    Then no sync_publish_gate_missing finding is reported

  Scenario: Error — a gate naming an empty set
    Given a workspace whose publishGate is "urn:ordin:default:evals:ghost"
    And no evaluation whose id is that name or begins with "urn:ordin:default:evals:ghost:"
    When the repository is checked
    Then sync_publish_gate_missing is reported on the workspace with the fix ChangeItYourself on field "governanceMetadata.publishGate"

  Scenario: A shared name prefix is not membership
    Given a workspace whose publishGate is "urn:ordin:default:evals"
    And the only evaluation is "urn:ordin:default:evalsuite:x"
    When the repository is checked
    Then sync_publish_gate_missing is reported

  Scenario: The bundled advisor and coordinator pass the publish-gate check
    Given the advisor and coordinator workspaces with their ids in the urn:ordin: spelling
    And the bundled evaluations under "urn:ordin:default:evals"
    When the repository is checked
    Then no sync_publish_gate_missing finding is reported for either workspace
```

### Performance, Security, Observability

Unchanged.

### Gaps

None.

---

## Amendment 2026-01-10 — the publish gate is read in the agent's own repository; bundled agents are outside the company trust policy

> Status: Approved (user, 2026-01-10, "proceed with recommendations"). Every section above stays in force except where a rule below names what it replaces. Source: ADR-090 §4 (publish gate), ADR-082 and ADR-086 (trust); the amendment 2026-01-10 above (the publish gate names a set of evaluations); OrdinDesktop.md DSK-REC-39a (the bundled catalogue is a pinned commit of the bundled agents repository).
>
> Evidence read: this file's steps 10, 17, 29, 30, 31 and 32, `ExecutablePart`, `TrustPolicy`; `PartSource::repository_parts(repository) -> Vec<SourcedPart>` and `SourcedPart { part, repository, repository_label, repository_display_name, manifest }` (Agent Model); the `urn:ordin:default:workspaces:coordinator` and `…:advisor` profiles, both with a `publishGate`; `GAPS.md` item G10 of the bundled agents repository (the gate passes in the whole-repository check and fails in the two checks that read one agent).

### Internal Logic (changes)

**CHK-PG-1 The publish gate is read in the agent's own repository (replaces rule 2 of the amendment above for the checks of one agent).** A publish gate is a rule about publishing the repository, not a part the agent needs, so the evaluations a gate names are never in the agent's closure. Step 10's publish-gate cross-check therefore reads its members from two places:
1. the parts of the tree being checked (a whole-repository check: the whole repository, as before); and
2. for a WorkspaceProfile read from a repository, the Evaluations of that repository, listed with `PartSource::repository_parts(repository)` (the repository the WorkspaceProfile's `SourcedPart.repository` names), by `metadata.id`, the latest version of each.
The set has a member when either place holds one (the membership rule of the amendment above is unchanged). This applies to step 30 (`check_library_item` of a WorkspaceProfile) and to `check_parts` (the Release Builder's package check). A WorkspaceProfile with no source repository (a draft) is checked against the tree only, as before. An error of `repository_parts` is the check's `CHK_SOURCE_UNAVAILABLE`, as for every other source read of these checks. No evaluation is added to the closure, the plan or the installed bundle; only the cross-check reads the repository.

**CHK-TR-1 Bundled agents are outside the company trust policy (amends steps 17, 31 and 32).** The company's trust policy (`agent_catalogue.trust_policy`) governs what the company has not shipped. A part read from the bundled catalogue (`SourcedPart.repository` is `RepositoryRef::Bundled`, whose label is `bundled`: the bundled agents repository of OrdinDesktop.md DSK-REC-39a) is frozen into the release and is not the company's to approve. Therefore:
1. `ExecutablePart` gains `bundled: bool`, omitted when false and read as false when absent (`#[serde(default, skip_serializing_if = "std::ops::Not::not")]`), so an `ExecutablePart` stored before this amendment reads as not bundled. Step 31 sets it to true when the part that declares the executable part (`declared_by`) is read from the bundled catalogue; a part with the same identifier in a repository the person added is read from that repository and is not bundled.
2. Step 31's decision for an `ExecutablePart` with `bundled` true is `Trusted`, whatever the policy lists. Step 17 raises no `trust_needs_it_approval` and no `tool_not_allowed` for a part read from the bundled catalogue.
3. Step 32 (`reevaluate_trust`) keeps every part of the instance that the instance's stored candidate (the `parts` JSON of the latest `check_for_install` with the instance id) marks `bundled` as `Trusted`, whatever the new profile lists; the other parts are re-decided as before. An instance with no stored candidate has no part marked.
4. No other check is relaxed: the vocabulary, reference, cross-check and conflict checks apply to a bundled agent as to any other.

### Error Table / findings (rows added)

None. A bundled agent's check no longer reports `sync_publish_gate_missing` for a gate whose evaluations are in its repository, nor `trust_needs_it_approval` or `tool_not_allowed`.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: Checker — the publish gate in the agent's repository, and bundled agents outside the trust policy

  Scenario: Happy path — a library item's gate is met by its repository's evaluation (CHK-PG-1)
    Given a repository holding the workspace "urn:ordin:default:workspaces:coordinator" with publishGate "urn:ordin:default:evals"
    And the repository holds the evaluation "urn:ordin:default:evals:ci-gate", which the workspace does not reference
    When check_library_item is run for the workspace
    Then no sync_publish_gate_missing finding is reported

  Scenario: Error path — the repository has no evaluation in the set (CHK-PG-1)
    Given the same workspace and a repository whose only evaluation is "urn:ordin:default:other:x"
    When check_library_item is run for the workspace
    Then sync_publish_gate_missing is reported with the fix ChangeItYourself on field "governanceMetadata.publishGate"

  Scenario: Happy path — the package check reads the same repository (CHK-PG-1)
    Given the first scenario's repository
    When check_parts is run for the workspace
    Then no stop finding names the publish gate

  Scenario: Edge case — a draft has no repository to read (CHK-PG-1)
    Given a draft workspace with publishGate "urn:ordin:default:evals" and no evaluation in the draft
    When its cross-checks run
    Then sync_publish_gate_missing is reported

  Scenario: Error path — the repository cannot be listed (CHK-PG-1)
    Given PartSource::repository_parts fails with a retryable error
    When check_library_item is run for the workspace
    Then the check fails with CHK_SOURCE_UNAVAILABLE

  Scenario: Happy path — a bundled agent's program is trusted without the policy listing it (CHK-TR-1)
    Given an agent read from the bundled catalogue whose tool part starts the program "ordin-hook"
    And a trust policy that lists nothing
    When check_for_install and then decide_executable_parts are run
    Then no trust_needs_it_approval finding is reported
    And the stored decision for the program is Trusted

  Scenario: Security — the same program in an added repository is not exempt (CHK-TR-1)
    Given the same tool part and program in a repository the person added
    When check_for_install is run with the empty trust policy
    Then trust_needs_it_approval is reported for the program

  Scenario: Edge case — a new profile does not take the trust back (CHK-TR-1)
    Given an installed bundled agent whose program is stored Trusted and marked bundled
    When reevaluate_trust runs against a profile whose policy lists nothing
    Then the instance is not in lost

  Scenario: Edge case — a part stored before this amendment (CHK-TR-1)
    Given an ExecutablePart stored without the field bundled
    When it is read
    Then bundled is false
```

### Performance, Security, Observability

Unchanged, except: the library item and package checks of a WorkspaceProfile that has a `publishGate` make one `repository_parts` call. Security: the exemption follows the origin the source reports for each read, never a name in the part's own text.

### Gaps

None.

## Amendment 2026-01-31 — A profile's prohibited list is checked in the Rules Engine's form

> **Status:** Approved 2026-01-31 (user decision: "fix the agents repo", with the publish gate applying the Rules Engine's rule so an agent that cannot be installed cannot be published). Source: ordin's own bundled agent passed the publish gate but could not be installed. Its WorkspaceProfile listed `github.pullrequest.create` and five other three-part entries under `toolPermissions.workspaceOverrides.prohibited`. The Rules Engine accepts a prohibited verb only in the form `^[a-z0-9_-]+(\.[a-z0-9_-]+)?$` (RulesEngine.md, Internal Logic A.4: "each verb … otherwise `RULE_POLICY_INVALID`"), so it refused the agent's whole rule set, the Package Inspector refused the declaration (`PKI_DECLARATION_REFUSED`), and the install failed on every start. Every section above stays in force.
>
> Evidence read: RulesEngine.md Internal Logic A.4; `crates/ordin-rules/src/load.rs` (`verb_shape`, `compile_set` A.4); `crates/ordin-rules/src/policy.rs` (`verb_of`: a verb is `program.subcommand`, or the program alone); `crates/ordin-agent-model/src/readers.rs` (`prohibited_verbs`); this spec's Internal Logic step 7.

- **CHK-TP-1 Prohibited entries.** In step 7, after `assay_policy_invalid` and before the per-kind body checks, a WorkspaceProfile whose `toolPermissions.workspaceOverrides.prohibited` cannot be read as a list of text, or holds an entry that does not match `^[a-z0-9_-]+(\.[a-z0-9_-]+)?$`, gets the finding `tool_policy_invalid` [failure] and the part stops, as for every rule of step 7. `detail`: `prohibited entry "<entry>" is not a command (program or program.subcommand) or a tool name`, or `toolPermissions.workspaceOverrides.prohibited is malformed (<code>)`. `message_id` is `check-part-invalid` with `args.part`; the fix is `ChangeItYourself { field: "toolPermissions.workspaceOverrides.prohibited" }`. The pattern is the Rules Engine's, so a profile that passes this check is not refused by A.4 for its prohibited list.

### Error Table (rows added)

| Condition | Finding | Severity |
|---|---|---|
| A prohibited entry is not in the Rules Engine's verb form, or the list is not a list of text | `tool_policy_invalid` | failure |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario: Error path — a three-part prohibited entry (CHK-TP-1)
    Given a WorkspaceProfile whose prohibited list holds "git.commit" and "github.pullrequest.create"
    When the part is checked
    Then the finding tool_policy_invalid names "github.pullrequest.create"
    And the check run does not pass

  Scenario: Happy path — commands and tool names pass (CHK-TP-1)
    Given a WorkspaceProfile whose prohibited list holds "git.commit", "gh.pr" and "secrets.read"
    When the part is checked
    Then no tool_policy_invalid finding is given
```

### Gaps

None.

## Amendment January 2026 — Check it on an installed agent: offered choices, one finding per reference, and words for every finding

> **Status:** Approved by the user, January 2026 ("approve, go ahead with phase 3A code"). Source: the person's own use of the first release — **Check it** on the bundled document agent showed eight failures, each the bare id `check-part-invalid`, although the agent works.
>
> Evidence read: the person's check run (installed agent, 8 failures, every one `reference_unresolved` for `urn:ordin:default:overlays:language:en-US`, `…:fr`, `…:es` and `urn:ordin:default:overlays:style:concise`, each twice); the agent's lock file (`ordin.lock/v1`: the profile is the `composition` and again an entry of `artefacts`; `overlaysApplied` and `selections.overlays` hold only `language:en-GB` and `style:plain-language`); `crates/ordin-checker/src/checker.rs` `installed` (step 11 with no lookup, over `[composition] + artefacts`); `crates/ordin-checker/src/repo.rs` `references_of` (a profile's `available` and `default` overlays, and its offered packs, are counted as references); `crates/ordin-checker/src/finding.rs` `msg` (25 message ids); the Fluent catalogues `crates/ordin-localisation/catalogues/{en-GB,de-DE,fr-FR}/` (none of the 25 ids present).

- **CHK-INS-1 An installed agent's offers are choices, not references** *(amended January 2026)*. In step 29, step 11 runs over the lock's parts with one change: for a WorkspaceProfile, the overlays listed under an axis's `available` and the packs it offers are references only when the lock chose them (`selections.overlays`, `selections.practicePacks`, `overlaysApplied`); an offer the lock did not choose is not looked for. A chosen overlay or pack missing from the lock's parts still fails `reference_unresolved`. Every other reference is unchanged. Steps 11 for drafts, library items and whole repositories (sections C and D) are unchanged: there every offer must exist.
- **CHK-INS-2 Each part is read once** *(amended January 2026)*. In step 29, the parts read are the lock's `composition` followed by its `artefacts`, keeping the first of entries with the same kind, identifier and version, so a part the lock lists twice gives each finding once.
- **CHK-MSG-1 Every message id has words** *(amended January 2026)*. Step 40's message ids carry these en-GB strings in `crates/ordin-localisation/catalogues/en-GB/screens-fixed.ftl`. The other shipped languages (de-DE, fr-FR) carry no screen strings and show the screens in English through the fallback, so en-GB is where every id must be (corrected while implementing, January 2026: the approved text named de-DE and fr-FR too). `$part` is the part's name (`args.part`); a Fluent number argument selects the plural.

| Id | en-GB |
|---|---|
| `check-part-invalid` | { $part } doesn't pass one of ordin's checks. |
| `check-parts-disagree` | Two parts of this agent say different things about { $what }: { $part_a } and { $part_b }. |
| `check-choose-packs` | Choose which { $class } this agent uses when the workspace doesn't say. |
| `check-format-cannot-hold` | { $format } format can't hold { $what }, so it would be left out. |
| `check-no-place-in-agent` | { $what } has no place in an ordin agent. |
| `check-whole-repository` | Checked with the whole of { $repository }. |
| `check-agent-would-not-pass` | { $agent } would no longer pass its checks: { $reason } |
| `check-trust-needs-it-approval` | { $tool } runs a program on this computer that your organisation's policy hasn't approved. |
| `check-tool-not-allowed` | { $tool } isn't one of the tools your organisation allows. |
| `check-job-not-covered` | No task does the job "{ $job }". |
| `check-name-collision` | Another agent here is already called "{ $name }". |
| `check-task-name-not-verb` | The task "{ $task }" should be named for what it does, starting with a verb such as "Summarise". |
| `check-text-too-long` | { $field } is too long. |
| `check-date-ambiguous` | { $field } has a date that can be read two ways. Write it as 2026-01-31. |
| `check-borrowed-unreachable` | { $part } comes from a repository the people using this agent can't reach. |
| `check-borrowed-apm-unsupported` | { $count -> [one] One borrowed part *[other] { $count } borrowed parts } can't be kept in the APM format. |
| `check-bare-command-name` | Write the command "{ $command }" with its agent's name in front, so it can't be mistaken for another agent's. |
| `check-publish-gate-not-met` | This agent can't be published yet: { $reason } |
| `check-installed-part-gone` | { $part } is no longer in its repository. |
| `check-installed-part-withdrawn` | { $part } has been withdrawn. |
| `check-withdrawal-invalid` | The withdrawal of { $part } can't be made as written. |
| `check-agent-stops` | { $agent } would stop working. |
| `check-vocabulary-term-cited` | "{ $term }" is still used by { $parts }. |
| `check-vocabulary-status-fixed` | "{ $term }" is one of ordin's fixed states and can't be retired. |
| `check-lifecycle-invalid` | { $part } has a status ordin doesn't recognise. |

A test in `crates/ordin-checker` reads the en-GB catalogue and fails when any id of `msg` has no string there.

```gherkin
Feature: Check it on an installed agent (CHK-INS-1, CHK-INS-2, CHK-MSG-1)

  Scenario: Happy path — an agent that offers languages it was not installed with
    Given an installed agent whose profile offers the languages en-GB, en-US, fr and es
    And its lock chose en-GB only
    When check_installed is called
    Then there is no finding

  Scenario: Error path — a chosen overlay missing from the lock
    Given an installed agent whose lock chose the style "concise"
    And the lock's parts do not hold "concise"
    When check_installed is called
    Then there is exactly one finding reference_unresolved naming "concise"

  Scenario: Edge case — a lock that lists the profile twice
    Given a lock whose artefacts repeat its composition
    When check_installed is called on an agent with one real problem
    Then that problem is reported once

  Scenario: Edge case — every finding reads as a sentence
    Given any finding the Checker makes
    Then its message_id has an en-GB string
```

### Gaps

None.

## Amendment January 2026 — A task's title and inputs are checked (ADR-119)

> **Status:** Approved by the user January 2026. Source: HLD 09 ADR-119 items 1 and 8 (decision 101). The readers are AgentModel.md AM-TF-2 to AM-TF-4. Every section above stays in force.
>
> Evidence read: step 7 (per-kind body checks that do not stop the part, each finding with `message_id` `check-part-invalid` and `args.part`); step 14's W5 (`task_name_not_verb` over each task's name, with its compiled-in verb list); CatalogueService.md SA-RECON-03 (ingest maps findings to parts by `part_identifier`; a part with a Failure is not offered); `ChangeItYourself { field }`.

- **CHK-TF-1 A Command's title and inputs** (step 7, a per-kind body check that does not stop the part). For a part of kind Command: `Manifest::title()` → an `AmError` gives finding `task_title_invalid` (failure); `Manifest::inputs()` → an `AmError` gives `task_inputs_invalid` (failure, `detail` naming the input's index and the rule); when both read, `check_task_body(inputs, markdown_body(manifest_path, bytes))` → an `AmError` gives `task_body_invalid` (failure, `detail` naming the rule and the name). Each has `message_id` `check-task-form-invalid` with `args.task` = the title when it reads, else `metadata.name`, and fix `ChangeItYourself { field: "inputs" }` (`"title"` for `task_title_invalid`). At ingest a part with any of them is not offered (SA-RECON-03), so no installed agent holds a task whose form cannot be built. *Amended January 2026 (user decision):* `task_inputs_invalid` also names the rule `whole` (a `whole` key that is not a boolean, or on an input that is not a number) and the rule `default` for a whole number's default holding a fraction (AgentModel.md AM-TF-3); the check is otherwise unchanged.
- **CHK-TF-2 The wording rule reads the title.** Step 14's W5 (*task names start with a verb*) reads a task's `title` when it has one, else its name as now; `task_name_not_verb` names the title then.

Finding codes added (Severity): `task_title_invalid` (failure), `task_inputs_invalid` (failure), `task_body_invalid` (failure). Message id added to step 40 and to the table of CHK-MSG-1:

| Id | en-GB |
|---|---|
| `check-task-form-invalid` | The task "{ $task }" can't show its form: its fields or the text that uses them don't match. |

```gherkin
Feature: Checker — a task's title and inputs (ADR-119)

  Scenario: Happy path — the bundled tasks pass
    Given the bundled catalogue of CatalogueService.md CAT-TF-1 to CAT-TF-3
    When check_repository runs over it
    Then no finding task_title_invalid, task_inputs_invalid or task_body_invalid is reported

  Scenario: Error — a body that names an undeclared input
    Given a Command whose inputs are "plan" and "phase" and whose body names ${input:step}
    When check_part runs
    Then one failure task_body_invalid is reported with message id check-task-form-invalid
    And at ingest the part is not offered

  Scenario: Edge — the title is read by the wording rule
    Given a draft Command with title "Summary of a document"
    When the draft check runs
    Then task_name_not_verb is reported naming "Summary of a document"
```

### Gaps

None.
