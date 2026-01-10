# SPEC: Authoring Service

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-102)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-authoring` · **Build phase:** 9 · **Programs:** D P
> **Dependencies:** Agent Model, Catalogue Service, Checker, Format Plug-ins, MCP Front, Ordin Host, Push Service, Journal

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

## Purpose

The Authoring Service holds everything a person writes for an agent repository before it is published: **agent drafts** made with the agent maker or imported, **part drafts** that create or change one part (including lifecycles, withdrawals and vocabulary changes), and the **rehearsal records** of agent drafts (02:462–466; ADR-083, ADR-084, ADR-088, ADR-090, ADR-091, ADR-094, ADR-095). It serves crucible's authoring tools to the agent maker's session only, through the MCP Front, returning structured fields and never files; runs the gap check of a role write-up against the library; runs the Foundry when the person approves *Make this agent*; writes every file in fixed code through the target repository's format plug-in; has the Checker check each draft and the whole repository with it; runs rehearsals and records them; lets the person try a passing draft alone; and prepares each pull request's contents for the Push Service, then follows the review on GitHub until the change is published or closed. It carries no approval role of its own (ADR-084).

## Approach

A command-driven draft store with one state machine per draft (2C `ReviewDraftState`), whose files live on a branch of the person's clone of the target agent repository (a worktree the Catalogue Service opens); every file, version, identifier, wording change and pull-request body is computed in fixed code (ADR-085), and a model is reached only through a session on the person's own harness via the Ordin Host, with source material passed as data, never inside the prompt template (ADR-088). The authoring tools are the 17 crucible tools left after removing `draft_propose` and `attestation_get` (`crucible/internal/registry/mcp/toolset/toolset.go:75-176`; ADR-090), registered with the MCP Front as the `ToolProvider` for server `crucible`. Error codes take the prefix `AUTHR_` (*chosen here*; not `AUTH_`, which is Caller Authentication's carried wrightrun prefix, Error Handling EH-D1) besides 2C's `ADRAFT_*`, `PDRAFT_*` and `REH_*`. Rejected: *keep drafts only in the datastore and write files at publish* — the HLD places drafts on a branch (04 Data Lifecycle), and the whole-repository check needs the tree with the change; *give the agent maker compile, Foundry or publish tools* — crucible keeps them off its tool surface on purpose (ADR-083 alternatives).

## Shared Context

### Shared types used (copied from Phase 2C, unchanged)

```rust
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>);
pub struct Sha256Hex(pub String);
// UUIDv7 newtypes: AgentDraftId, PartDraftId, RehearsalId, AgentRepositoryId, PartId, RunId,
// WorkspaceId, AgentInstanceId, ArtefactId, PlaceId

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentDraft {
    pub id: AgentDraftId,
    pub agent_repository_id: AgentRepositoryId,
    pub name: String,                         // 1–80 (ADRAFT_NAME_INVALID)
    pub purpose: String,                      // 1–500 (ADRAFT_PURPOSE_INVALID)
    pub jobs: Vec<DraftJob>,                  // lists ≤ 50 items of ≤ 500 chars (ADRAFT_LIST_TOO_LONG)
    pub tasks: Vec<String>,
    pub will: Vec<String>,
    pub wont: Vec<String>,
    pub needs: Vec<String>,
    pub reused_parts: Vec<PartId>,
    pub origin: DraftOrigin,
    pub branch: String,                       // valid git ref ≤ 255 bytes (ADRAFT_BRANCH_INVALID)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_results_ref: Option<String>,
    pub wording_changes: Vec<WordingChange>,
    pub field_authors: Vec<FieldAuthor>,      // stored in the side table draft_field_authors (2C, decision 80: C-21)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rehearsal_id: Option<RehearsalId>,
    pub trial: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<PullRequestRef>, // at most one open (ADRAFT_PR_ALREADY_OPEN)
    pub state: ReviewDraftState,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DraftJob { pub text: String, pub came_from: String }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WordingChange { pub field: String, pub before: String, pub after: String }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftOrigin { Made, Imported }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PullRequestRef { pub number: u64, pub url: String, pub state: PullRequestState }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PullRequestState { Open, Closed, Merged }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDraftState { Draft, WaitingForReview, ChangesAskedFor, ApprovedNotSignedYet, Published, Discarded }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Rehearsal {
    pub id: RehearsalId,
    pub agent_draft_id: AgentDraftId,
    pub draft_version: String,                // commit of the draft's branch rehearsed
    pub situations: Vec<RehearsalSituation>,  // 1–20 (REH_SITUATIONS_OUT_OF_RANGE)
    pub run_ids: Vec<RunId>,
    pub rehearsed_at: Timestamp,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RehearsalSituation {
    pub description: String,                  // 1–1000 (REH_DESCRIPTION_INVALID)
    pub documents_used: Vec<String>,
    pub expectations: Vec<RehearsalExpectation>, // 1–20, text 1–500 (REH_EXPECTATIONS_INVALID)
    pub reviewer_opinion: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RehearsalExpectation {
    pub text: String,
    /// The machine rule the Checker checks; absent for a model-judged expectation (an opinion only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<ExpectationRule>,
    pub outcome: ExpectationOutcome,
}
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpectationOutcome { Met, NotMet }   // 2C enum; the Checker's struct of the same name is written `ordin_checker::ExpectationOutcome` below

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PartDraft {
    pub id: PartDraftId,
    pub agent_repository_id: AgentRepositoryId,
    pub change_kind: PartChangeKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part_id: Option<PartId>,              // absent iff new_part not yet merged (PDRAFT_PART_MISMATCH)
    pub fields: serde_json::Value,            // ≤ 256 KiB (PDRAFT_FIELDS_TOO_LARGE)
    pub computed_version: String,             // semantic version (PDRAFT_VERSION_INVALID)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changes_what_part_does: Option<bool>, // required before waiting_for_review for changed_part (PDRAFT_ANSWER_MISSING)
    pub used_by_at_check: Vec<PartId>,
    pub branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_results_ref: Option<String>,
    pub wording_changes: Vec<WordingChange>,
    pub field_authors: Vec<FieldAuthor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<PullRequestRef>, // at most one open (PDRAFT_PR_ALREADY_OPEN)
    pub state: ReviewDraftState,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartChangeKind { NewPart, ChangedPart, Deprecation, Withdrawal, VocabularyTerm }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FieldAuthor { pub field: String, pub written_by: FieldWriter }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldWriter { Person, ModelThenPerson, Model }

// Part, PartKind, PartStatus, AgentRepository, AgentFormat, WithdrawnListEntry, VocabularyTerm,
// VocabularyAxis, Lifecycle, LifecycleStage, StageNext, StageChoice, SendBack: as CatalogueService.md copies them from 2C.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairAction { TryAgain, SignInAgain, SendToIt }
```

2C state machine (`AgentDraft` and `PartDraft`): — →`draft`; `draft`→`waiting_for_review` (pull request opened); `waiting_for_review`→`changes_asked_for` (reviewer asks); `changes_asked_for`→`waiting_for_review` (changes pushed); `waiting_for_review`→`approved_not_signed_yet` (approved, unsigned, signing required — only where the repository requires signed merges, `ADRAFT_SIGNING_NOT_REQUIRED`); `approved_not_signed_yet`→`published` (signed and merged); `waiting_for_review`→`published` (merged); `waiting_for_review`, `changes_asked_for`→`draft` (pull request closed); `draft`, `changes_asked_for`→`discarded`. Trial or publish refused when the draft changed after its rehearsal (`ADRAFT_REHEARSAL_STALE`). A part draft needs the person's right to open a review (`PDRAFT_NO_REVIEW_RIGHT`) and at most one open part draft per part (`PDRAFT_ALREADY_OPEN_FOR_PART`). Rehearsal rows are written once (`REH_IMMUTABLE`).

### Fixed rules carried from crucible and the HLD

| Rule | Value | Source |
|---|---|---|
| Kinds a part draft writes | Rule, Skill, PracticePack, Guideline, Memory, ControlLoop (crucible `forgeAuthorableKinds`, `pkg/types/kinds.go:152-155`), widened by ADR-094 item 5 to SystemRole, OperatingMode, Capability, Command, ContextResource, ToolBinding, StyleOverlay, LanguageOverlay, HandoffOverlay, SourceWorkflowOverlay, Evaluation, OutputContract, Journey; never WorkspaceProfile, Vocabulary (the **Words** tab), CrewProfile, AgentIdentity, BrandOverlay, JurisdictionOverlay | 02:463, ADR-091 item 1, ADR-092, ADR-094 item 5 |
| Code-workflow parts (SourceWorkflowOverlay) | offered only in a workspace that links code | ADR-094 item 5 |
| APM-format repository | only **Withdraw…** | ADR-091 *Which repositories*, revision 0.37.1 |
| Version cadence | new part 1.0.0; correction ("Does this change what the part does?" **No**) → patch; any other change → minor; vocabulary term added → minor; retired → major; deprecation → patch (*assumed here*: a status change is not a change in behaviour) | ADR-091 item 1, UI-D103 |
| Identifier of a new part | `urn:ordin:<publisher>:<kind-dir>[:<category>]:<slug>` (crucible `models/REGISTRY.md` "Canonical URN grammar"); slug = the name lower-cased, non `[a-z0-9]` runs replaced by `-`, trimmed, ≤ 64 chars; publisher = the repository's publisher directory (`@<publisher>`) | ADR-091 items 1 and 7 |
| Kind directories | crucible `KindDirectories` (`pkg/types/kinds.go:313-333`): `workspaces`, `journeys`, `crews`, `capabilities`, `commands`, `skills`, `rules`, `tool-bindings`, `context-resources`, `overlays/<sub>`, `practice-packs`, `evals`, `output-contracts`, `agent-identities`, `vocabulary`, `guidelines`, `memories` | ADR-091 item 7 |
| File placement | crucible Forge `pathFor` (`internal/registry/forge/render.go`), ported | ADR-091 item 1, ADR-094 |
| Borrowed parts and the APM format | a draft with borrowed parts is not published to an APM-format repository; the check says so | SA-APM-01, ADR-090 §2 |
| Authoring tool argument size | any string argument over 16,384 chars is refused as file content (*chosen here*, LW-10's text limit) | ADR-085 |
| Polling of open pull requests | on each Catalogue fetch notice and every 15 minutes (*assumed here*, 2D-4 interval) | ADR-084 |

### Reconciliation assumptions

- **SA-RECON-01** — The Authoring Service owns its command names (ruling R1): the Service API catalogue adopts the 21 `authoring.*` commands in *Public Interface*; `authoring.drafts`, `authoring.draft_get` and `authoring.send_for_review` are replaced by `authoring.list`, `authoring.agent_draft` and `authoring.publish`.
- **SA-RECON-02** — The `DraftModel` trait and its types belong to the Agent Model; the Authoring Service is the holder of agent drafts, part drafts and rehearsals and their only store (ruling R18). Trials reach the Resolver and Compiler through the Package Catalogue, never from this component.
- **SA-RECON-03** — The Authoring Service holds the job-to-task mapping `job_tasks` of each agent draft and supplies it to the Checker (reconciliation row 638); 2C `AgentDraft` is unchanged, so the mapping lives in this module's own column.
- **SA-RECON-04** — The Authoring Service implements the Journal's `PruneParticipant` for module `authoring` (Journal retention A-08).
- **SA-RECON-05** — Every provider's names win (ruling R6): this spec calls the Checker's inherent methods and `CheckError` (no `CheckerError`, `check_part_change` or `check_expectations`), the Format Plug-ins' concrete `FormatPlugins` struct (no `detect_any`, `read_source`, `starting_files()`), the Agent Model's `DraftModel` with `AmError`, and the Ordin Host's `AuthoringHost` with `OhError` (no `HostError`).
- **SA-RECON-06** — The Authoring Service implements the MCP Front's `ToolProvider` as the MCP Front defines it (ruling R2): provider id `authoring`, one server `crucible`, `offer` grants the 17 tools only to registered agent-maker runs. The program registers `tool_provider()`; this service does not register itself.
- **SA-RECON-07** — Migrations follow the Journal's tuple form (ruling R3): `pub fn migrations() -> ordin_journal::ModuleMigrations` with module `authoring`; journal records go through `Journal::append_entry` with `component = "authoring"`.
- **SA-RECON-08** — Agent-maker requirements go to `catalogue.match_requirements` in batches of at most 50 (reconciliation row 562); the results are concatenated in order, so a dossier with more than 50 requirement sections is classified in full.
- **SA-RECON-09** — The rehearsal checks only expectations that carry an `ExpectationRule` (2C `RehearsalExpectation.rule`, decision 80: C-32); an expectation without one is judged by the reviewer model and recorded as an opinion that never blocks publishing.

### Closing-round assumptions

- **SA-CLOSE-01** (*chosen here*, K5) — `lifecycle_of(&draft)` answers which lifecycle a draft changes, so the program can fill `PublishContext` from the Handover Service. A part draft of kind Journey whose part already exists in the catalogue (`changed_part`, `deprecation`, `withdrawal`) records, when it is opened, the `LifecycleId` of the Catalogue Service's `Lifecycle` with the same repository and identifier (`catalogue.lifecycles(Some(repository))`); every other draft has none. The answer is served from an in-memory map loaded by `run` at start and kept current on every part-draft write, because the method is synchronous.
- **SA-CLOSE-02** (*chosen here*, K5) — A review that asks for changes on the person's own draft is reported as `AuthoringNotice::ChangesAskedFor`; the program raises the Service API event `needs_you_changed` from it, and the Ordin Desktop program raises the draft-review notification from that event (no new event kind).
- **SA-CLOSE-03** (closes G-AUTH-2) — **Import…** of an agent archive reads a `.zip` file with the `zip` crate named in 05 (revision 0.49.0, *Archives*), with 05's limits: no absolute path, no `..` segment, at most 2,000 entries and 50 MB unpacked.
- **SA-CLOSE-04** (closes G-AUTH-3) — `StagedPart` and `SourceOverride` are Agent Model types (AgentModel.md SA-CLOSE-04; the Resolver and Compiler re-exports them); this crate imports them from `ordin_agent_model`, so no edge to `ordin-resolver` is needed.
- **SA-CLOSE-05** (closes G-AUTH-4, *chosen here*) — The handover places the person can reach are the Git repositories and SharePoint folders linked to any workspace on this computer. This crate owns the trait `HandoverReach`; the program implements it over the Workspace Service. A lifecycle stage's place is reachable when its target is one of those places or lies inside one (§E5).
- **SA-CLOSE-06** (closes G-AUTH-5) — The Ordin Host's `AuthoringHost::rehearse` waits for the rehearsal run to end and returns the Checker's `RunEvidence` (all six fields); this service no longer reads the run's evidence from the Journal.

### Boundary interfaces this spec requires (owned by other components)

```rust
// ordin-agent-model — Agent Model (AgentModel.md, "The draft model"; ruling R18: types owned there,
// drafts held here). Copied unchanged. There is no `ModelError`; every failure is an `AmError`.
pub struct GeneratedPart { pub kind: PartKind, pub name: String, pub fields: serde_json::Value }
pub struct ReusedPart { pub kind: PartKind, pub identifier: String }
pub trait DraftModel: Send + Sync {
    fn agent_parts_from_fields(&self, draft: &AgentDraft, publisher: &str,
        reused: &[ReusedPart], generated: &[GeneratedPart]) -> Result<Vec<ModelPart>, AmError>;
    fn part_from_fields(&self, kind: PartKind, fields: &serde_json::Value) -> Result<ModelPart, AmError>;
    fn fields_of(&self, part: &ModelPart) -> serde_json::Value;
    fn form(&self, kind: PartKind) -> serde_json::Value;            // JSON Schema of the kind's fields
    fn scaffold(&self, kind: PartKind, name: &str, identifier: &str) -> Result<ModelPart, AmError>;
    fn render_write_up(&self, draft: &AgentDraft) -> String;         // Markdown, role-dossier headings
}
pub struct FieldModel;                                              // the one implementation; the program passes it
// Also used: ModelPart { kind, manifest, manifest_path, files: PartFiles, content_digest }, PartFiles,
// content_digest(&PartFiles) -> Sha256Hex, part_file_path(kind, id) -> Result<String, AmError>,
// mint_identifier(kind, publisher, category, name) -> Result<String, AmError>,
// parse_withdrawn_list(bytes) -> Result<WithdrawnListFile, AmError>, render_withdrawn_list(&WithdrawnListFile) -> String,
// SourceScope { home: RepositoryRef, profile: Vec<RepositoryRef>, include_bundled: bool },
// StagedPart { part: ModelPart, repository: RepositoryRef, repository_label: String },
// SourceOverride(pub Arc<dyn PartSource>)   — owned by the Agent Model (SA-CLOSE-04),
// RepositoryRef { Repository(AgentRepositoryId), Bundled }, SourcedPart, Resolution { Found, NotFound, Ambiguous },
// PartSource::{resolve(scope, kind, id, version), files(&SourcedPart), repository_parts(&RepositoryRef)},
// PartSourceError, AmError (codes AM_*; AM_DRAFT_FIELDS_INVALID, AM_DRAFT_KIND_NOT_COMPOSABLE among them).

// ordin-catalogue — Catalogue Service (CatalogueService.md), called on `Arc<CatalogueService>`; every error is `CatalogueError { code, retryable, action, detail }`.
pub async fn list_repositories(&self) -> Result<Vec<RepositorySummary>, CatalogueError>;
pub async fn search(&self, query: LibraryQuery) -> Result<Page<LibraryEntry>, CatalogueError>;
pub async fn facets(&self, query: LibraryQuery) -> Result<LibraryFacets, CatalogueError>;
pub async fn match_requirements(&self, input: MatchRequirementsInput) -> Result<Vec<RequirementMatch>, CatalogueError>; // 1–50 requirements
pub async fn part(&self, id: PartId) -> Result<PartView, CatalogueError>;
pub async fn read_part(&self, id: PartId) -> Result<PartDocument, CatalogueError>;
pub async fn used_by(&self, id: PartId) -> Result<UsedBy, CatalogueError>;
pub async fn made_of(&self, agent: PartId) -> Result<MadeOf, CatalogueError>;
pub async fn part_history(&self, id: PartId) -> Result<PartHistory, CatalogueError>;
pub async fn withdrawn_list(&self, id: AgentRepositoryId) -> Result<Vec<WithdrawnListEntry>, CatalogueError>;
pub async fn vocabulary(&self, id: AgentRepositoryId) -> Result<Vec<VocabularyTerm>, CatalogueError>;
pub async fn reviewers_for(&self, id: AgentRepositoryId, paths: Vec<String>) -> Result<ReviewerInfo, CatalogueError>;
pub async fn pull_request_status(&self, id: AgentRepositoryId, number: u64) -> Result<PullRequestStatus, CatalogueError>;
pub async fn open_draft_worktree(&self, input: DraftWorktreeInput) -> Result<DraftWorktree, CatalogueError>;
pub async fn close_draft_worktree(&self, branch: &str, id: AgentRepositoryId) -> Result<(), CatalogueError>;
pub async fn draft_source(&self, id: AgentRepositoryId, worktree: std::path::PathBuf) -> Result<std::sync::Arc<dyn PartSource>, CatalogueError>;
pub async fn lifecycles(&self, id: Option<AgentRepositoryId>) -> Result<Vec<Lifecycle>, CatalogueError>;   // 2C `Lifecycle` (id, part_id, identifier, …)
pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<CatalogueNotice>;
// `CatalogueService` also implements `PartSource` (the profile and bundled repositories).
pub struct MatchRequirementsInput { pub requirements: Vec<String> }   // 1–50 items, each 1–2000 chars
pub enum Coverage { Covered, PartlyCovered, NotCovered }
pub struct RequirementMatch { pub requirement: String, pub coverage: Coverage, pub matched: Vec<PartView>, pub shortfall: Option<String> }
pub struct UsedBy { pub part_id: PartId, pub agents: Vec<PartView>, pub parts: Vec<PartView> }
pub struct ReviewerInfo { pub readable: bool, pub review_required: Option<bool>, pub reviewers: Vec<String> }
pub struct PullRequestStatus { pub state: PullRequestState, pub review: ReviewDecision, pub change_requests: Vec<ReviewComment>, pub merged_signed: Option<bool> }
pub struct DraftWorktreeInput { pub agent_repository_id: AgentRepositoryId, pub branch: String }
pub struct DraftWorktree { pub path: std::path::PathBuf, pub base_commit: String }
pub enum CatalogueNotice {
    RepositoryUpdated { agent_repository_id: AgentRepositoryId, commit: String, newly_withdrawn: Vec<WithdrawnListEntryId> },
    RepositoryDropped { agent_repository_id: AgentRepositoryId },
}
// LibraryQuery, Page<T>, LibraryEntry, LibraryFacets, PartView, PartDocument, MadeOf, PartHistory,
// RepositorySummary, ReviewDecision, ReviewComment: as CatalogueService.md defines them.

// ordin-format-plugins — Format Plug-ins (FormatPlugins.md). Held as `Arc<FormatPlugins>` (the concrete struct).
impl FormatPlugins {
    pub fn get(&self, id: FormatId) -> &dyn FormatPlugin;
    pub fn detect(&self, tree: &SourceTree) -> Result<Vec<Detection>, FormatError>;
    pub fn read_any(&self, tree: &SourceTree, request: &ReadRequest) -> Result<(Detection, ReadOutcome), FormatError>;
    pub fn propose_kind(&self, path: &str, bytes: &[u8]) -> Result<KindProposal, FormatError>;
}
pub trait FormatPlugin: Send + Sync {
    fn read(&self, tree: &SourceTree, request: &ReadRequest) -> Result<ReadOutcome, FormatError>;
    fn write(&self, request: &WriteRequest) -> Result<WriteOutcome, FormatError>;
    /* id, versions, detect */
}
impl From<AgentFormat> for FormatId { /* Ordin → Ordin, Apm → Apm */ }
pub struct SourceTree(pub BTreeMap<String, Vec<u8>>);
pub struct ReadRequest { pub publisher: String, pub vocabulary: VocabularySet, pub source_label: String }
pub struct ReadOutcome { pub parts: Vec<ModelPart>, pub mapping_report: MappingReport, pub package: Option<PackageIdentity> }
pub struct MappingReport { pub format: Option<FormatId>, pub items: Vec<MappingItem> }
pub struct MappingItem { pub source_path: String, pub construct: String, pub reason: UnmappedReason, pub part_identifier: Option<String> }
pub struct WriteRequest { pub parts: Vec<ModelPart>, pub borrowed: Vec<String>, pub bodies: BTreeMap<String, String>,
    pub package: Option<PackageIdentity>, pub existing: SourceTree, pub starting_vocabulary: Vec<ModelPart> }
pub struct WriteOutcome { pub files: BTreeMap<String, Vec<u8>>, pub starting_files: BTreeMap<String, Vec<u8>>, pub loss_report: LossReport }
pub struct LossReport { pub format: Option<FormatId>, pub items: Vec<LossItem> }
pub struct LossItem { pub part_identifier: String, pub kind: PartKind, pub what: String, pub reason: LossReason }
pub struct KindProposal { pub kind: PartKind, pub from_format: FormatId, pub rule: &'static str }
// UnmappedReason::code(), LossReason::code() give the snake_case reason strings; FormatError codes FMT_*;
// FormatId { Ordin, Apm, ClaudeCode, Copilot }, Detection, PackageIdentity as FormatPlugins.md defines them.

// ordin-checker — Checker (Checker.md). Held as `Arc<Checker>` (inherent methods; the Checker declares no
// trait). For test doubles this crate declares a private trait `DraftChecks` with exactly these
// signatures and implements it for `Checker` by delegation. Every error is `CheckError`.
impl Checker {
    pub async fn check_agent_draft(&self, request: &AgentDraftCheck) -> Result<CheckReport, CheckError>;
    pub async fn check_part_draft(&self, request: &RepositoryCheck, part_draft_id: PartDraftId,
        reachable_places: &BTreeMap<String, bool>) -> Result<CheckReport, CheckError>;
    pub async fn check_withdrawal(&self, request: &WithdrawalCheck, part_draft_id: PartDraftId) -> Result<CheckReport, CheckError>;
    pub async fn check_vocabulary(&self, request: &VocabularyCheck, part_draft_id: PartDraftId) -> Result<CheckReport, CheckError>;
    pub fn check_rehearsal(&self, rules: &[ExpectationRule], evidence: &RunEvidence) -> Vec<ordin_checker::ExpectationOutcome>;
    pub fn apply_wording_rules(&self, fields: &[(String, String)], language: &str)
        -> (Vec<(String, String)>, Vec<WordingChange>, Vec<Finding>);
    pub fn read_dossier(&self, write_up: &str, contract: &ModelPart) -> Result<DossierReading, CheckError>;
    pub async fn report(&self, id: CheckRunId) -> Result<CheckReport, CheckError>;
}
pub struct CheckRunId(pub uuid::Uuid);                               // `check_results_ref` = its string
pub enum Severity { Failure, Warning }
pub enum Fix { FixIt { action: FixAction }, AskOrdin, ChangeItYourself { field: String }, LeaveItOut { what: String },
    PublishToOrdinFormat, AskIt, AskAgentMaker, ChoosePacks { rule_index: u32, choices: Vec<String> }, LetAgentAsk { rule_index: u32 } }
pub enum FixAction { MintIdentifier { field: String, value: String }, PointToReplacement { field: String, from: String, to: String },
    AskMissingField { field: String }, DropUnknownField { field: String } }
pub struct Finding { pub code: String, pub severity: Severity, pub part_identifier: Option<String>, pub message_id: String,
    pub args: BTreeMap<String, String>, pub fixes: Vec<Fix>, pub detail: String }
pub struct CheckReport { pub check_run_id: CheckRunId, pub subject: CheckSubject, pub passed: bool, pub findings: Vec<Finding>,
    pub tidied: Vec<WordingChange>, pub checked_at: Timestamp, pub version_checked: String,
    pub agents_that_stop: Vec<String>, pub publish_gate_met: bool }
pub struct ReportItem { pub what: String, pub part_identifier: Option<String>, pub reason: String }
pub struct RepositoryCheck { pub repository: RepositoryRef, pub repository_display_name: String, pub scope: SourceScope,
    pub changes: Vec<StagedPart>, pub deletions: Vec<String>, pub source: Option<SourceOverride> }
pub struct AgentDraftCheck { pub draft: AgentDraft, pub parts: Vec<StagedPart>, pub target: RepositoryCheck,
    pub target_format: TargetFormat, pub job_tasks: BTreeMap<u32, Vec<String>>, pub mapping_report: Vec<ReportItem>,
    pub loss_report: Vec<ReportItem>, pub borrowed: Vec<String>, pub repository_agent_names: Vec<String>,
    pub rehearsal: Option<Vec<ordin_checker::ExpectationOutcome>>, pub agent_language: String,
    pub reachable_places: BTreeMap<String, bool> }
pub enum TargetFormat { Ordin, Apm }
pub struct WithdrawalCheck { pub repository: RepositoryCheck, pub entry: WithdrawnListEntry,
    pub existing_entries: Vec<WithdrawnListEntry>, pub proposed_list: Vec<WithdrawnListEntry> }
pub struct VocabularyCheck { pub repository: RepositoryCheck, pub axis: VocabularyAxis,
    pub added: Vec<(String, String, String)>, pub retired: Vec<String> }
pub struct RunEvidence { pub run_id: RunId, pub final_message: String, pub tools_used: Vec<String>,
    pub approvals_asked: u32, pub external_actions: u32, pub artefact_names: Vec<String> }
// The Checker's outcome struct (named `ExpectationOutcome` in ordin-checker; written qualified here
// because 2C's `ExpectationOutcome` is the Met/NotMet enum above):
pub struct /* ordin_checker:: */ ExpectationOutcome { pub rule: ExpectationRule, pub met: bool, pub evidence: String }
pub struct DossierReading { pub sections: Vec<DossierSection>, pub missing_sections: Vec<String>, pub unmatched_headings: Vec<String> }
pub struct DossierSection { pub name: String, pub text: String, pub evidence_count: u32, pub required: bool,
    pub low_confidence: bool, pub derived: bool }
pub enum CheckError { SourceUnavailable { retryable: bool }, ResolverFailed { code: String, retryable: bool },
    InputInvalid { field: &'static str }, TooManyFindings, RunNotFound { id: CheckRunId }, DatastoreFailed { retryable: bool },
    SourceNotBound, SourceAlreadyBound, ContractInvalid { reason: &'static str },
    InstanceUnknown { id: AgentInstanceId } }   // codes CHK_*; `retryable()`
// `StagedPart` and `SourceOverride`, which the Checker's inputs carry, are imported from `ordin_agent_model`
// (SA-CLOSE-04).

// ordin-mcp-front — MCP Front (McpFront.md "The ToolProvider trait", ruling R2). Implemented, not called.
#[async_trait]
pub trait ToolProvider: Send + Sync + 'static {
    fn provider_id(&self) -> &'static str;
    fn servers(&self) -> Vec<ServerSpec>;
    fn server_prefix(&self) -> Option<&'static str> { None }
    async fn offer(&self, grant: &SessionGrant) -> Result<Vec<ToolKey>, ToolFailure>;
    fn check_arguments(&self, key: &ToolKey, arguments: &serde_json::Value) -> Result<(), ArgumentProblem>;
    async fn call(&self, ctx: ToolCallContext, key: &ToolKey, arguments: serde_json::Value) -> Result<ToolOutput, ToolFailure>;
    async fn session_closed(&self, run_id: RunId);
}
pub struct ServerSpec { pub name: ServerName, pub tools: Vec<ToolSpec> }
pub struct ToolSpec { pub name: String, pub description: String, pub input_schema: serde_json::Value,
    pub argument_form: String, pub sandbox_callable: bool, pub plain: String }
pub struct ToolKey { pub server: ServerName, pub tool: String }
pub struct ToolOutput { pub text: String, pub structured: Option<serde_json::Value> }
pub struct ToolFailure { pub code: String, pub message: String, pub retryable: bool }
pub struct ArgumentProblem { pub at: String, pub reason: String }
// SessionGrant { run_id, workspace_id, agent_instance_id, harness, profile, caller: ToolCaller, scope_id, coding_task_id },
// ToolCallContext { run_id, workspace_id, agent_instance_id, caller, scope_id, coding_task_id, call_id, correlation_id,
//   deadline, cancel, progress, only_needed_parts, via }, ServerName: as McpFront.md defines them.

// ordin-host — Ordin Host (OrdinHost.md, in-process interface; types owned there). Copied unchanged.
pub struct AgentMakerStart { pub workspace_id: WorkspaceId, pub request: String, pub requirements: Vec<String>, pub mode: AgentMakerMode }
pub enum AgentMakerMode { WholeAgent { agent_draft_id: AgentDraftId }, OnePart { part_draft_id: PartDraftId },
                          Lifecycle { part_draft_id: PartDraftId }, Fix { finding: String } }
pub struct GenerationRequest { pub template_id: &'static str, pub data: serde_json::Value, pub workspace_id: WorkspaceId }
pub struct RehearsalRunRequest { pub agent_draft_id: AgentDraftId, pub agent_repository_id: AgentRepositoryId,
                                 pub worktree: std::path::PathBuf, pub situation: String, pub documents: Vec<String>,
                                 pub workspace_id: WorkspaceId }
pub struct TrialRequest { pub agent_draft_id: AgentDraftId, pub agent_repository_id: AgentRepositoryId,
                          pub worktree: std::path::PathBuf, pub agent: String, pub workspace_id: WorkspaceId }
#[async_trait::async_trait]
pub trait AuthoringHost: Send + Sync {
    async fn start_agent_maker(&self, req: AgentMakerStart) -> Result<RunId, OhError>;
    async fn generate(&self, req: GenerationRequest) -> Result<String, OhError>;
    /// Runs the rehearsal and waits for it to end; returns its evidence (SA-CLOSE-06).
    async fn rehearse(&self, req: RehearsalRunRequest) -> Result<RunEvidence, OhError>;
    async fn add_trial(&self, req: TrialRequest) -> Result<AgentInstanceId, OhError>;
}
// `ordin_host::OhError` with `code() -> String` and `retryable() -> bool`; variants used here:
// TemplateUnknown (OH_TEMPLATE_UNKNOWN), SessionStartFailed { cause, retryable },
// DependencyFailed { dependency, code, retryable } (OH_DEPENDENCY_FAILED; `add_trial` carries the Package Catalogue's code in `code`).

// ordin-push — Push Service (PushService.md)
pub async fn propose_pull_request(&self, req: PullRequestProposal) -> Result<SubmissionId, PushError>;
pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<PushNotice>;
pub struct PullRequestProposal { pub workspace_id: WorkspaceId, pub origin: SubmissionOrigin, pub host: String, pub repository: String,
    pub content: PullRequestContent, pub branch: String, pub base: String, pub title: String, pub body: String,
    pub labels: Vec<String>, pub reviewers: Vec<String>, pub existing: Option<PullRequestRef>, pub card: CardText }
pub enum SubmissionOrigin { AgentDraft { id: AgentDraftId }, PartDraft { id: PartDraftId }, Handover { id: HandoverId },
    WorkflowShare { workspace_id: WorkspaceId, file_name: String } }
pub enum PullRequestContent { Worktree { path: std::path::PathBuf, paths: Vec<String> }, Files { files: Vec<(String, Vec<u8>)> } }
pub struct CardText { pub question: String, pub verb: String, pub where_line: String, pub affects: Vec<AffectedItem>,
    pub changes: String, pub content: Option<ApprovalContent> }
pub enum PushNotice { /* … */ PullRequestOpened { submission: SubmissionId, origin: SubmissionOrigin, pull_request: PullRequestRef },
    PullRequestFailed { submission: SubmissionId, origin: SubmissionOrigin, code: &'static str },
    PullRequestRejected { submission: SubmissionId, origin: SubmissionOrigin } /* other variants ignored */ }
pub struct PushError { pub code: &'static str, pub retryable: bool, pub action: Option<RepairAction>, pub detail: String }

// ordin-journal — Journal (JournalAndArtefactStore.md; ruling R3, decision D4)
pub type MigrationStep = (i64, &'static str, &'static str);
pub struct ModuleMigrations { pub module: &'static str, pub sqlite: &'static [MigrationStep], pub postgres: &'static [MigrationStep] }
pub struct Datastore;                                                   // `sqlite()` gives the pool
impl Journal {
    pub async fn append_entry(&self, entry: NewJournalEntry) -> Result<JournalEntry, JournalError>;
    pub fn register_prune_participant(&self, participant: std::sync::Arc<dyn PruneParticipant>);
}
pub struct NewJournalEntry { pub kind: String, pub component: String, pub run_id: Option<RunId>, pub crew_run_id: Option<CrewRunId>,
    pub workspace_id: Option<WorkspaceId>, pub summary: String, pub data: serde_json::Value }
#[async_trait::async_trait]
pub trait PruneParticipant: Send + Sync {
    fn module(&self) -> &'static str;
    async fn prune(&self, store: &Datastore, request: &PruneRequest) -> Result<u64, PruneFailure>;
}
pub struct PruneRequest { pub cutoff: Timestamp, pub runs_in_progress: Vec<RunId> }
pub struct PruneFailure { pub code: &'static str }
```

## Public Interface

```rust
pub struct AuthoringService { /* private */ }

/// Owned here, implemented by the program over the Workspace Service (SA-CLOSE-05): the handover
/// places the person can reach — every Git repository and SharePoint folder linked to a workspace
/// on this computer.
#[async_trait::async_trait]
pub trait HandoverReach: Send + Sync {
    async fn reachable_places(&self) -> Result<Vec<ReachablePlace>, AuthoringError>;
}
pub struct ReachablePlace {
    pub place_type: HandoverPlaceType,          // 2C: Git | Sharepoint
    /// Git: `<host>/<owner>/<name>` lower-cased; SharePoint: the folder's web address.
    pub address: String,
}

pub struct AuthoringDeps {
    /// The Journal's `Datastore` (decision D4); this module's tables live in it.
    pub datastore: ordin_journal::Datastore,
    pub journal: std::sync::Arc<ordin_journal::Journal>,
    pub model: std::sync::Arc<dyn ordin_agent_model::DraftModel>,     // `FieldModel` in the programs
    pub catalogue: std::sync::Arc<ordin_catalogue::CatalogueService>,
    pub formats: std::sync::Arc<ordin_format_plugins::FormatPlugins>,
    pub checker: std::sync::Arc<ordin_checker::Checker>,
    pub host: std::sync::Arc<dyn ordin_host::AuthoringHost>,
    pub push: std::sync::Arc<ordin_push::PushService>,
    pub places: std::sync::Arc<dyn HandoverReach>,
}

impl AuthoringService {
    /// Module `authoring`, SQLite steps only (no agent-profile tables); registered by the program (SA-RECON-07).
    pub fn migrations() -> ordin_journal::ModuleMigrations;
    pub fn new(deps: AuthoringDeps, root: std::path::PathBuf /* <data root>/authoring */) -> Self;
    /// Follows pull requests (Catalogue notices, 15-minute poll) and Push notices until shutdown.
    pub async fn run(&self, shutdown: tokio_util::sync::CancellationToken) -> Result<(), AuthoringError>;
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<AuthoringNotice>;
    /// The Journal prune participant for module `authoring` (Internal Logic §N).
    pub fn prune_participant(&self) -> std::sync::Arc<dyn ordin_journal::PruneParticipant>;

    // Agent drafts
    pub async fn make_agent(&self, input: MakeAgentInput) -> Result<AgentDraftView, AuthoringError>;
    pub async fn make_this_agent(&self, id: AgentDraftId) -> Result<AgentDraftView, AuthoringError>;   // Foundry
    pub async fn update_agent_draft(&self, input: UpdateAgentDraftInput) -> Result<AgentDraftView, AuthoringError>;
    pub async fn rehearse(&self, input: RehearseInput) -> Result<Rehearsal, AuthoringError>;
    pub async fn try_it(&self, input: TryItInput) -> Result<AgentInstanceId, AuthoringError>;
    pub async fn set_publish_to(&self, id: AgentDraftId, repository: AgentRepositoryId) -> Result<AgentDraftView, AuthoringError>;

    // Part drafts (part, lifecycle, withdrawal, vocabulary, deprecation)
    pub async fn new_part(&self, input: NewPartInput) -> Result<PartDraftView, AuthoringError>;
    pub async fn change_part(&self, input: ChangePartInput) -> Result<PartDraftView, AuthoringError>;
    pub async fn stop_using_in_new_agents(&self, part: PartId) -> Result<PartDraftView, AuthoringError>;
    pub async fn withdraw(&self, input: WithdrawInput) -> Result<PartDraftView, AuthoringError>;
    pub async fn vocabulary_change(&self, input: VocabularyChangeInput) -> Result<PartDraftView, AuthoringError>;
    pub async fn update_part_draft(&self, input: UpdatePartDraftInput) -> Result<PartDraftView, AuthoringError>;
    pub async fn suggest_wording(&self, input: SuggestWordingInput) -> Result<PartDraftView, AuthoringError>;

    // Both kinds
    pub async fn import(&self, input: ImportInput) -> Result<DraftRef, AuthoringError>;
    pub async fn apply_fix(&self, input: ApplyFixInput) -> Result<DraftRef, AuthoringError>;
    pub async fn publish(&self, draft: DraftRef, context: Option<PublishContext>) -> Result<(), AuthoringError>; // raises the card via Push
    pub async fn send_change(&self, draft: DraftRef) -> Result<(), AuthoringError>;  // to the same pull request
    pub async fn discard(&self, draft: DraftRef) -> Result<(), AuthoringError>;
    pub async fn list(&self, filter: DraftFilter) -> Result<Vec<DraftSummary>, AuthoringError>;
    pub async fn agent_draft(&self, id: AgentDraftId) -> Result<AgentDraftView, AuthoringError>;
    pub async fn part_draft(&self, id: PartDraftId) -> Result<PartDraftView, AuthoringError>;
    /// The lifecycle a draft changes (SA-CLOSE-01); `None` for an agent draft and every other part draft.
    pub fn lifecycle_of(&self, draft: &DraftRef) -> Result<Option<LifecycleId>, AuthoringError>;
}

/// The `crucible` tool provider; the program registers it with the MCP Front before `start` (Internal Logic §H).
pub struct AuthoringToolProvider { /* private: Arc to the service */ }
impl AuthoringService { pub fn tool_provider(&self) -> std::sync::Arc<dyn ToolProvider>; }
#[async_trait::async_trait]
impl ToolProvider for AuthoringToolProvider { /* provider_id() = "authoring"; servers() = [crucible: 17 tools] */ }
```

Inputs and results (*chosen here*):

```rust
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DraftRef { Agent { id: AgentDraftId }, Part { id: PartDraftId } }
pub struct MakeAgentInput { pub workspace_id: WorkspaceId, pub request: String /* ≤ 8000 */, pub requirements: Vec<String> /* 0–50 */ }
pub struct UpdateAgentDraftInput {
    pub id: AgentDraftId,
    pub name: Option<String>, pub purpose: Option<String>, pub jobs: Option<Vec<DraftJob>>,
    pub tasks: Option<Vec<String>>, pub will: Option<Vec<String>>, pub wont: Option<Vec<String>>, pub needs: Option<Vec<String>>,
    pub reused_parts: Option<Vec<PartId>>, pub fixed_packs: Option<Vec<FixedPackChoice>>,
    /// Which tasks carry out each job: key = index into `jobs` (0-based), value = task texts from `tasks`.
    /// Held by the Authoring Service and handed to the Checker as `AgentDraftCheck.job_tasks` (§D, §E).
    pub job_tasks: Option<std::collections::BTreeMap<u32, Vec<String>>>,
}
/// The optional WorkspaceProfile field `fixedPracticePacks` (SA-NAME-07): fixed packs, or a rule left for the agent to ask.
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FixedPackChoice { Fixed { rule: String, packs: Vec<String> }, AgentAsks { rule: String } }
pub struct RehearseInput { pub id: AgentDraftId, pub workspace_id: WorkspaceId, pub situations: Vec<SituationInput> /* 1–20 */ }
pub struct SituationInput { pub description: String, pub documents: Vec<String>, pub expectations: Vec<String> }
pub struct TryItInput { pub id: AgentDraftId, pub workspace_id: WorkspaceId }
pub struct NewPartInput { pub agent_repository_id: AgentRepositoryId, pub kind: PartKind, pub name: String, pub workspace_links_code: bool, pub supersedes: Option<String> }
pub struct ChangePartInput { pub part: PartId }
pub struct WithdrawInput { pub part: PartId, pub reason: String /* 1–1000 */, pub replacement: Option<PartId> }
pub struct VocabularyChangeInput { pub agent_repository_id: AgentRepositoryId, pub axis: VocabularyAxis, pub change: VocabularyEdit }
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VocabularyEdit { Add { code: String, label: String, description: String }, Retire { code: String } }
pub struct UpdatePartDraftInput { pub id: PartDraftId, pub fields: serde_json::Value, pub changes_what_part_does: Option<bool> }
pub struct SuggestWordingInput { pub id: PartDraftId, pub field: String }
pub struct ImportInput { pub source: std::path::PathBuf, pub as_part: Option<PartKind>, pub agent_repository_id: AgentRepositoryId, pub workspace_id: WorkspaceId }
pub struct ApplyFixInput { pub draft: DraftRef, pub finding_code: String, pub fix: FixChoice }
#[serde(rename_all = "snake_case")]
pub enum FixChoice { FixIt, AskOrdin, LeaveItOut, AskAgentMaker, PublishToOrdinFormat }
pub struct DraftFilter { pub agent_repository_id: Option<AgentRepositoryId>, pub open_only: bool }
/// Lifecycle drafts only (ADR-095 item 2): facts the card names, filled by the program from the Handover Service.
pub struct PublishContext { pub following_workspaces: Vec<String>, pub open_handovers: u32 }

pub struct AgentDraftView { pub draft: AgentDraft /* carries field_authors */, pub job_tasks: std::collections::BTreeMap<u32, Vec<String>>, pub checks: Vec<Finding>, pub tidied: Vec<WordingChange>, pub publish_to: Vec<AgentRepositoryId>, pub can_try: bool, pub can_publish: bool, pub reviewer_words: Vec<String>, pub files: Option<Vec<String>> /* code workspaces only */ }
pub struct PartDraftView { pub draft: PartDraft, pub checks: Vec<Finding>, pub tidied: Vec<WordingChange>, pub agents_that_stop: Vec<String>, pub used_by: Vec<PartId>, pub form: serde_json::Value, pub writing_guide: Option<String>, pub can_publish: bool, pub reviewer_words: Vec<String> }
pub struct DraftSummary { pub draft: DraftRef, pub name: String, pub state: ReviewDraftState, pub repository: String, pub updated_at: Timestamp }

#[derive(Debug, Clone)]
pub enum AuthoringNotice {
    WaitingForReview { draft: DraftRef, pull_request: PullRequestRef },
    ChangesAskedFor { draft: DraftRef, reviewer_words: Vec<String> },   // Needs you + notice (11 §7)
    ApprovedNotSignedYet { draft: DraftRef },
    Published { draft: DraftRef },                                       // notice (11 §7)
    Closed { draft: DraftRef, reviewer_words: Vec<String> },
    SendFailed { draft: DraftRef, code: &'static str },
}

#[derive(Debug, thiserror::Error)]
#[error("{code}")]
pub struct AuthoringError { pub code: &'static str, pub retryable: bool, pub action: Option<RepairAction>, pub detail: String }
```

### Service API commands (definitive; the Service API catalogue adopts this list, ruling R1)

Every command is implemented by `ordin-authoring`, runs in the personal profile (programs D, P) and is called by the Service API on behalf of the screens. `Done` is the Service API's empty result. The names `authoring.drafts`, `authoring.draft_get` and `authoring.send_for_review` are not used; their operations are `authoring.list`, `authoring.agent_draft` and `authoring.publish` (SA-RECON-01).

| Command | Input | Result | Method | Class |
|---|---|---|---|---|
| `authoring.make_agent` | `MakeAgentInput` | `AgentDraftView` | `make_agent` | Change |
| `authoring.make_this_agent` | `AgentDraftId` | `AgentDraftView` | `make_this_agent` | Change |
| `authoring.update_agent_draft` | `UpdateAgentDraftInput` | `AgentDraftView` | `update_agent_draft` | Change |
| `authoring.rehearse` | `RehearseInput` | `Rehearsal` | `rehearse` | Change |
| `authoring.try_it` | `TryItInput` | `AgentInstanceId` | `try_it` | Change |
| `authoring.set_publish_to` | `SetPublishToInput { id: AgentDraftId, repository: AgentRepositoryId }` | `AgentDraftView` | `set_publish_to` | Change |
| `authoring.new_part` | `NewPartInput` | `PartDraftView` | `new_part` | Change |
| `authoring.change_part` | `ChangePartInput` | `PartDraftView` | `change_part` | Change |
| `authoring.stop_using_in_new_agents` | `PartId` | `PartDraftView` | `stop_using_in_new_agents` | Change |
| `authoring.withdraw` | `WithdrawInput` | `PartDraftView` | `withdraw` | Change |
| `authoring.vocabulary_change` | `VocabularyChangeInput` | `PartDraftView` | `vocabulary_change` | Change |
| `authoring.update_part_draft` | `UpdatePartDraftInput` | `PartDraftView` | `update_part_draft` | Change |
| `authoring.suggest_wording` | `SuggestWordingInput` | `PartDraftView` | `suggest_wording` | Change |
| `authoring.import` | `ImportInput` | `DraftRef` | `import` | Change |
| `authoring.apply_fix` | `ApplyFixInput` | `DraftRef` | `apply_fix` | Change |
| `authoring.publish` | `DraftRef` | `Done` | `publish` (the program adds `PublishContext`, §K1) | Change |
| `authoring.send_change` | `DraftRef` | `Done` | `send_change` | Change |
| `authoring.discard` | `DraftRef` | `Done` | `discard` | Change |
| `authoring.list` | `DraftFilter` | `Vec<DraftSummary>` | `list` | Read |
| `authoring.agent_draft` | `AgentDraftId` | `AgentDraftView` | `agent_draft` | Read |
| `authoring.part_draft` | `PartDraftId` | `PartDraftView` | `part_draft` | Read |

`SetPublishToInput` is the wire form of `set_publish_to(id, repository)`: `pub struct SetPublishToInput { pub id: AgentDraftId, pub repository: AgentRepositoryId }` (*chosen here*).

### Called by

| Public item | Called by |
|---|---|
| `migrations`, `new`, `run`, `tool_provider` | the desktop program and the server program in the personal profile at construction: `migrations()` goes to the Journal's `MigrationRegistry::register`; `tool_provider()` goes to `McpFront::register` before the MCP Front's `start` (McpFront.md registration order) |
| `prune_participant` | the same programs, which pass it to the Journal's `register_prune_participant`; the Journal then calls `PruneParticipant::prune` during its daily prune (§N) |
| `subscribe` | the program, which forwards `AuthoringNotice` to the Service API event stream; `ChangesAskedFor` and `ApprovedNotSignedYet` raise `needs_you_changed` (the screens' *Needs you* entry; the Ordin Desktop program raises the draft-review notification from that event, SA-CLOSE-02), `Published` and `SendFailed` raise a notice (11 §7) |
| `lifecycle_of` | the Ordin Desktop and Ordin Server programs, before forwarding `authoring.publish`: `Some(lifecycle_id)` → `PublishContext` from `HandoverService::following(lifecycle_id)`; `None` → no context |
| `HandoverReach` (implemented, not called by others) | implemented by the Ordin Desktop and Ordin Server programs over the Workspace Service's linked Git repositories and SharePoint folders; called by this service in §E5 |
| every method in the command table | the Service API, one command each, as listed above |
| `publish` | the Service API command `authoring.publish`; before forwarding a Journey (lifecycle) draft, the program fills `PublishContext` from the Handover Service (workspaces on this computer that follow the lifecycle, open handovers in it); for every other draft `context` is `None` |
| `AuthoringToolProvider` (`crucible` tools) | the MCP Front: `servers` at registration and session open, `offer`, `check_arguments`, `call`, `session_closed` (§H) |
| `AgentDraftView.job_tasks` / `UpdateAgentDraftInput.job_tasks` | the screens through `authoring.update_agent_draft` and `authoring.agent_draft`; the agent maker through tool `draft_update`; read by the Checker as `AgentDraftCheck.job_tasks` (§E) |

The Authoring Service does not call the Resolver and Compiler. A trial (`try_it`) reaches the Resolver only through the Ordin Host's `add_trial`, which calls the Package Catalogue; the Package Catalogue resolves the draft from the Catalogue Service's `draft_source(repository, worktree)` (SA-RECON-02).

### Authoring tools (MCP server `crucible`)

Names verified in `crucible/internal/registry/mcp/toolset/toolset.go:75-170`; one-line descriptions are crucible's summaries (C-38). Arguments are typed and `deny_unknown_fields`; results are structured JSON, never file contents.

| Tool | Arguments (*chosen here*) | ordin behaviour |
|---|---|---|
| `catalogue_search` | `text`, `kinds[]`, `repository_id?`, `cursor?` | `catalogue.search`, every kind, offered parts |
| `catalogue_facets` | as `catalogue_search` | `catalogue.facets` |
| `artefact_get` | `part_id` | `catalogue.part` + `part_history` |
| `artefact_composition` | `part_id` (a WorkspaceProfile) | `catalogue.made_of` + its open pack and overlay choices |
| `rule_constituents` | `part_id` (a Rule) | the rules its body imports, from `read_part` |
| `kind_guideline` | `kind`, `repository_id` | the repository's Guideline for that kind: `authorGuidance` and `rubric` |
| `memory_contract_get` | `part_id` (a Memory) | `read_part` fields `retains`, `excludes`, `retention`, `store` |
| `build_plan_create` | `write_up` (structured fields as `UpdateAgentDraftInput`) | gap check (§B) — stores nothing |
| `draftable_kinds` | — | the kinds of the table above the person may write in the session's target repository |
| `roles_get` | — | per repository the person reads: `can_publish`, `can_curate` (ADR-091 item 11) |
| `draft_get` | `draft` (`DraftRef`) | this session's draft fields, checks and authorship |
| `draft_list` | — | the drafts this session works on |
| `draft_create` | `kind`, `name`, `fields` | for a whole-agent session: refused (the draft exists); for a one-part session: `new_part` |
| `draft_update` | `draft`, `fields` | `update_agent_draft` / `update_part_draft`, every field written marked `model` |
| `draft_delete` | `draft` | discards a part draft the session created and never published |
| `draft_assist` | `draft`, `field`, `intent` | `suggest_wording` via the Ordin Host; the field marked `model` |
| `draft_assay` | `draft` | the Checker's report (`Finding`s with severity `failure` or `warning`); model-scored checks are added by this service with severity `opinion` (the Checker has no such severity) |

### Example

Values are illustrative; identifiers are UUIDv7 (SA-ID-01) and the publisher is `acme`.

1. **Make an agent** (Service API `authoring.make_agent`, §A). Input `MakeAgentInput`:

```json
{ "workspace_id": "0192f0a1-7c3e-7b2a-9d41-5e6f7a8b9c0d",
  "request": "Check supplier invoices",
  "requirements": ["Reads PDF invoices", "Compares each invoice line with the price list"] }
```

Result `AgentDraftView` (fields not shown are empty lists or absent):

```json
{ "draft": { "id": "0192f0a2-1d4e-7f00-8a11-22b3c4d5e6f7", "agent_repository_id": "0192e0aa-0000-7000-8000-000000000001",
             "origin": "made", "state": "draft", "trial": false,
             "branch": "ordin/agent/check-supplier-invoices-0192f0a2",
             "created_at": "2026-01-10T09:15:02.311Z", "updated_at": "2026-01-10T09:15:02.311Z" },
  "checks": [], "can_try": false, "can_publish": false }
```

Side effects: the draft worktree is opened on that branch, the agent-maker run is registered for the draft, and `authoring.draft_created` is logged with `draft_id`, `origin = made` and `repository_id`.

2. **Gap check** (tool `build_plan_create` on server `crucible`, called by that agent-maker run, §B). The write-up's two required dossier rows go to `catalogue.match_requirements`; the tool returns `ToolOutput { text: "2 requirements: 1 covered, 1 not covered", structured: Some(<the RequirementMatch list>) }`, with each `PartView` shown here by its identifier only:

```json
[ { "requirement": "Reads PDF invoices", "coverage": "covered",
    "matched": ["urn:ordin:acme:skills:pdf-reader"], "shortfall": null },
  { "requirement": "Compares each invoice line with the price list", "coverage": "not_covered",
    "matched": [], "shortfall": "No part compares invoice lines with a price list" } ]
```

Nothing is stored. On *Make this agent*, `urn:ordin:acme:skills:pdf-reader` becomes a `reused_parts` entry and the Foundry drafts one new part for the not-covered row (§C).

3. **Fix a finding** (Service API `authoring.apply_fix`, §F). The latest check holds a finding `{ "code": "id_grammar_invalid", "severity": "failure", "part_identifier": null, "fixes": [ { "fix_it": { "action": { "mint_identifier": { "field": "metadata.id", "value": "urn:ordin:acme:workspaces:check-supplier-invoices" } } } } ] }`. Input `ApplyFixInput`:

```json
{ "draft": { "kind": "agent", "id": "0192f0a2-1d4e-7f00-8a11-22b3c4d5e6f7" },
  "finding_code": "id_grammar_invalid", "fix": "fix_it" }
```

Result `DraftRef` `{ "kind": "agent", "id": "0192f0a2-1d4e-7f00-8a11-22b3c4d5e6f7" }`: `metadata.id` now holds the minted identifier and the draft is re-checked (§E). The same input with `"finding_code": "kind_unknown"`, a code absent from the latest report, fails with `AUTHR_FINDING_UNKNOWN` and changes nothing.

## Internal Logic

**A. Make an agent (ADR-083, ADR-088).**
1. `make_agent`: choose the target repository: the first `person_can_publish` repository in the order of `catalogue.list_repositories()` (profile repositories first, ADR-086). None → the draft is held locally: a git repository initialised with `git2` at `<root>/local/<draft id>` (*assumed here*: 2C requires a branch and a repository; the draft's `agent_repository_id` is then the bundled catalogue's row), and the screen says no repository can be published to (ADR-086).
2. Mint `AgentDraftId`; branch `ordin/agent/<slug-of-request ≤ 40>-<first 8 of id>` (*chosen here*); `catalogue.open_draft_worktree`; insert the draft (`origin = made`, `state = draft`).
3. `host.start_agent_maker(WholeAgent)`; record the run as this draft's agent-maker run (the tool provider serves only registered runs, §H1).
4. Log `authoring.draft_created` {draft_id, origin, repository_id}.

**B. Gap check on the write-up (ADR-088 step 3; crucible `buildplan.Plan`).** On `build_plan_create`, and when the person approves *Make this agent*: `model.render_write_up(draft)`; the `role-dossier` OutputContract is resolved through the Catalogue Service's `PartSource` implementation: `resolve(&SourceScope { home: RepositoryRef::Bundled, profile: vec![], include_bundled: true }, PartKind::OutputContract, "role-dossier", None)`; `Found(sourced)` → `files(&sourced)` → `ModelPart { kind, manifest: sourced.manifest, manifest_path: part_file_path(kind, id)?, files, content_digest: content_digest(&files) }`, refusing a digest different from the catalogue row's (`AM_SOURCE_CORRUPT` → `AUTHR_CHECK_UNAVAILABLE`); `NotFound`/`Ambiguous` → `AUTHR_CONTRACT_EMPTY`. `checker.read_dossier(write_up, contract)` → `DossierReading`; `CHK_CONTRACT_INVALID` → `AUTHR_CONTRACT_EMPTY` (crucible `ErrNoRequirements`). Each `DossierSection` with `required = true` and `derived = false` is one row, its text the requirement; `missing_sections` are rows with empty text, reported as not covered. The rows go to `catalogue.match_requirements(MatchRequirementsInput { requirements })` in batches of at most 50, results concatenated in order (SA-RECON-08); each `RequirementMatch.coverage` is `Covered`, `PartlyCovered` or `NotCovered` with `matched` parts. Covered and partly covered rows' first matched part becomes a `reused_parts` entry; not-covered rows become the gaps the Foundry drafts.

**C. Foundry (`make_this_agent`).** The person's approval of *Make this agent* is the trigger (02:462).
1. Run §B. For each gap row: `host.generate(template "foundry-part", data = {row, write_up})` → suggested fields for a new part of the kind the row names; record each generated field `model` (ADR-090 §7).
2. Reused parts: each `reused_parts` entry → `catalogue.part(id)` → `ReusedPart { kind: view.part.kind, identifier: <the part's identifier> }`. `model.agent_parts_from_fields(draft, publisher, &reused, &generated)` (publisher = the target repository's publisher directory without `@`) → parts. `AM_DRAFT_FIELDS_INVALID` or `AM_DRAFT_KIND_NOT_COMPOSABLE` → `AUTHR_FOUNDRY_FAILED` with the code in `detail` (permanent, the person's answers kept).
3. Write (§W).
4. Check (§E). A failed Foundry step keeps the person's answers and returns `AUTHR_FOUNDRY_FAILED` (retryable, 03 *Make an agent* error posture); there is no resume of a half-made set (ADR-090 *Loose ends*).

**D. Updating fields.** `update_agent_draft` / `update_part_draft` / tool `draft_update`: allowed in `draft` and `changes_asked_for` only (else `AUTHR_DRAFT_NOT_EDITABLE`); validate 2C rules; each changed field's author becomes `person` (by the person), `model` (by a tool call or **Suggest wording**), or `model_then_person` (the person changes a model-written field); regenerate files (§C2, §W) and re-check (§E). `fixed_packs` is written into the WorkspaceProfile's `fixedPracticePacks` (SA-NAME-07). `job_tasks` (SA-RECON-03): every key must be an index below `jobs.len()` and every task text must equal an entry of `tasks`, else `AUTHR_JOB_TASKS_INVALID` and nothing is written; at most 50 tasks per job (`ADRAFT_LIST_TOO_LONG`); the mapping is stored in `agent_drafts.job_tasks`; when `jobs` or `tasks` change without a new `job_tasks`, entries whose job index or task text no longer exists are dropped and the Checker then reports the job as not covered. The rehearsal becomes stale when the worktree's tree digest differs from the rehearsed commit's (`ADRAFT_REHEARSAL_STALE` on trial or publish).

**E. Checking.** Every check uses the draft's own source: `source = Some(SourceOverride(catalogue.draft_source(repository, worktree)?))` and `RepositoryCheck { repository: RepositoryRef::Repository(target), repository_display_name, scope: SourceScope { home: <target>, profile: <the profile repositories>, include_bundled: true }, changes, deletions, source }`, where `changes` are the draft's new and changed parts as `StagedPart { part, repository, repository_label }` and `deletions` the identifiers the draft removes.
1. Agent draft → `checker.check_agent_draft(&AgentDraftCheck { draft, parts: changes, target, target_format: TargetFormat::Ordin | Apm (from the repository's `AgentFormat`), job_tasks: <stored>, mapping_report: <the import's mapping items, empty for a made draft>, loss_report: <the last write's loss items>, borrowed: <identifiers resolved outside the target repository>, repository_agent_names: <names of the target repository's WorkspaceProfiles, from `catalogue.search` with `kinds` empty and `repositories = [target]`, every page>, rehearsal: <the current rehearsal's checked outcomes, None before one>, agent_language: <the WorkspaceProfile's first `metadata.audience.languages` entry, else "en-GB">, reachable_places: §E5 over the Journey parts among `changes` })`. Report items convert by the Format Plug-ins' fixed rule: a `MappingItem` → `ReportItem { what: construct, part_identifier, reason: reason.code() }`; a `LossItem` → `ReportItem { what, part_identifier: Some(part_identifier), reason: reason.code() }`.
2. Part draft, by `change_kind`: `new_part`, `changed_part`, `deprecation` → `checker.check_part_draft(&repository_check, id, &reachable_places)` (`reachable_places` from §E5 for a Journey draft, empty for every other kind); `withdrawal` → `checker.check_withdrawal(&WithdrawalCheck { repository, entry, existing_entries: catalogue.withdrawn_list(target), proposed_list: existing followed by the entry }, id)`; `vocabulary_term` → `checker.check_vocabulary(&VocabularyCheck { repository, axis, added, retired }, id)`. The Checker covers the whole repository with the change, borrowed repositories, every agent using the part rebuilt and conflicts (ADR-091 item 10).
3. Store `check_results_ref = report.check_run_id` as a string; the view's `checks`, `tidied` and `agents_that_stop` come from the report (`checker.report(id)` on a later read). `can_publish` = `passed` ∧ `publish_gate_met` ∧ (agent drafts) rehearsal current with every rule-carrying expectation met ∧ the target repository has `person_can_publish` ∧ the loss report is empty. Nothing can be waived (ADR-090).
5. **Reachable handover places** (SA-CLOSE-05). For a draft holding Journey parts: `places.reachable_places()` once per check (an error → `AUTHR_CHECK_UNAVAILABLE`, retryable). For each stage of each Journey part (`manifest.journey()`), key = the stage's `handover_place_target` exactly as written; value = true when a `ReachablePlace` of the same type matches: `git` — the target with any `https://` prefix, trailing `/` and trailing `.git` removed and its host lower-cased equals the address or starts with the address followed by `/`; `sharepoint` — the target, scheme and host compared case-insensitively and the path compared exactly, equals the address or starts with the address followed by `/`. No match → false (the Checker reports `lifecycle_place_unreachable`). The map is never sent anywhere else and no target is logged.
4. `CheckError` mapping: `SourceUnavailable`, `ResolverFailed`, `DatastoreFailed` with `retryable()` true → `AUTHR_CHECK_UNAVAILABLE` (retryable); `SourceNotBound` → `AUTHR_CHECK_UNAVAILABLE` (retryable; the program binds the source at start); `InputInvalid`, `TooManyFindings`, `RunNotFound`, `ContractInvalid`, `SourceAlreadyBound`, `InstanceUnknown` (never expected from a draft check) → `AUTHR_CHECK_FAILED` (permanent, detail = the code).

**F. Fixes (ADR-094 item 4).** `apply_fix`: the finding must be in the latest report (`AUTHR_FINDING_UNKNOWN`). The chosen `FixChoice` must match one of the finding's `fixes`, else `AUTHR_FINDING_UNKNOWN`. `FixIt` applies the `FixAction` in fixed code to the fields — `MintIdentifier { field, value }` writes `value`; `PointToReplacement { field, from, to }` replaces `from` by `to` in that field; `AskMissingField { field }` returns the draft with that field opened; `DropUnknownField { field }` removes the key after the person agreed on the card — and re-checks. `AskOrdin` → `host.generate(template "fix-finding", data = {finding, fields})`, written as model-drafted, re-checked. `LeaveItOut { what }` removes the item named `what`. `AskAgentMaker` → `host.start_agent_maker(AgentMakerStart { mode: AgentMakerMode::Fix { finding: code }, … })`. `PublishToOrdinFormat` → `set_publish_to` with the first `person_can_publish` ordin-format repository, else `AUTHR_NOT_READY_TO_PUBLISH`. `ChangeItYourself`, `AskIt`, `ChoosePacks` and `LetAgentAsk` are carried out by the screens through `update_agent_draft` (`fixed_packs`) or the request to IT, not through `apply_fix`. A fix on a library agent or part first opens a draft (§G1 / §A) and applies there; on an installed agent whose repository the person cannot write to, the draft targets the person's fork where GitHub allows it (ADR-094 item 4) — the Push Service opens that pull request.

**G. Part drafts (ADR-091, ADR-092, ADR-094, ADR-095).**
1. `change_part`: the repository must allow review (`PDRAFT_NO_REVIEW_RIGHT`); an open part draft for that part → `PDRAFT_ALREADY_OPEN_FOR_PART`; the kind must be one a part draft writes (table above) → `AUTHR_KIND_NOT_WRITABLE`; an APM-format repository → `AUTHR_APM_WITHDRAW_ONLY`. Open a worktree on branch `ordin/part/<slug>-<id8>`; `fields = model.fields_of(part)` (the part read through the Catalogue Service's `PartSource` as in §B); `used_by_at_check` = the `part.id` of each entry of `catalogue.used_by(part).agents` (shown first, ADR-091 item 8).
2. `new_part`: as 1 without a part; identifier minted (table above); unique in the repository → else `AUTHR_IDENTIFIER_TAKEN`; `model.scaffold(kind, name, identifier)?` (an `AmError` → `AUTHR_KIND_NOT_WRITABLE` for `AM_NO_FILE_CONVENTION`, else `AUTHR_STORE_FAILED` with the code in `detail`); `computed_version = 1.0.0`; `supersedes` when given must name an existing part of the same kind (the Checker's `sync_unknown_supersedes_target` also fails it). Code-workflow parts require `workspace_links_code`.
3. `stop_using_in_new_agents`: a `changed_part`-shaped draft with `status: deprecated`, offered on a part of any kind (ADR-091 item 2).
4. `withdraw`: `change_kind = withdrawal`; `fields` = the one new entry (`part`, `reason`, `withdrawnAt` = now in RFC 3339 ms UTC, `replacement` when given); the file written is `withdrawn.yaml`: the committed file read with `parse_withdrawn_list`, the entry appended, written with `render_withdrawn_list`, every existing entry byte-for-byte unchanged (append-only, ADR-091 item 3); `used_by` lists the agents that stop and those that move (the Checker reports which pass with the replacement, UI-D99). Allowed in APM-format repositories.
5. `vocabulary_change`: `change_kind = vocabulary_term`; edits the repository's Vocabulary part of that axis; retiring a status term (`draft`, `approved`, `deprecated` on `artefact-status`) → `VOCAB_STATUS_TERM_FIXED`; retiring a term cited in the same repository fails the check naming the parts (`VOCAB_TERM_CITED`); cited in another repository → a warning on the card and in the pull request.
6. Version (§ table): computed on each change; `changed_part` needs `changes_what_part_does` before publish (`PDRAFT_ANSWER_MISSING`).
7. Files: `model.part_from_fields(kind, &fields)` (`AM_DRAFT_FIELDS_INVALID` → `PDRAFT_FIELDS_TOO_LARGE` when over 256 KiB, else `AUTHR_FIELDS_INVALID`), then §W into the worktree; check (§E).
8. Lifecycle drafts (`Journey`, ADR-095 item 2): the form is the `stages` list; the Checker adds the lifecycle checks (every stage named with a role; agents and contracts resolve; places reachable; seam check on every link; reachability; no backward link; choices say *when*; send-backs to earlier stages, 1–10 rounds). The card names the workspaces on this computer that follow the lifecycle and any open handover in it; those facts arrive in `publish`'s `PublishContext` (*chosen here*), filled by the program from the Handover Service, because the Handover Service sits in the same build phase and is not a dependency. Each stage of the form has an optional **Slack channel** (ADR-102). The person picks it on the lifecycle draft screen from the channels their Slack sign-in belongs to (the screen calls `slack.offered_channels`; nothing is offered when the person is not signed in to Slack), or types a channel id and name. The draft stores it as the stage's `slackChannel { id, name }` and renders it through the Agent Model; clearing the field removes the key. The Checker's step 8 reports a malformed value as `lifecycle_slack_channel_invalid`. A private channel picked here is named in a file of the agent repository that people outside the channel may read; the field's help line says so: "Everyone who can read this lifecycle sees the channel's name." (*chosen here*). (amended January 2026, ADR-102)

**H. Authoring tools (`ToolProvider`, SA-RECON-06).**
1. `provider_id()` = `"authoring"`; `servers()` = one `ServerSpec { name: "crucible", tools }` with the 17 tools of the table above, each `ToolSpec { name, description: crucible's one-line summary (≤ 120 chars), input_schema: schemars schema of the argument type, argument_form, sandbox_callable: false, plain }`. The set is fixed, so this provider never calls `tools_changed`.
2. `offer(grant)`: `grant.run_id` registered as the agent-maker run of an open draft (§A3) → every `crucible` tool; any other run → an empty list (02:462: "to the agent maker's session only"). A datastore failure → `ToolFailure { code: "AUTHR_STORE_FAILED", retryable: true }`.
3. `check_arguments(key, arguments)`: typed deserialisation with unknown fields refused → `ArgumentProblem { at, reason }`; any string longer than 16,384 chars or any field named `content`, `file`, `files` or `body_text` → `ArgumentProblem` whose reason names `AUTHR_TOOL_FILE_CONTENT` (ADR-085: a call that carries file contents is refused).
4. `call(ctx, key, arguments)`: `ctx.run_id` must still be the registered agent-maker run of an open draft → else `ToolFailure { code: "AUTHR_TOOL_NOT_FOR_SESSION", retryable: false }`; the operation honours `ctx.cancel` and `ctx.deadline`; the result is `ToolOutput { text: <one plain line>, structured: Some(<the result JSON>) }`. Results never contain file text (`read_part`'s `file_text` is dropped). Each call is journalled through `append_entry` with kind `authoring_tool_call` and the arguments' SHA-256 digest only. An `AuthoringError` becomes `ToolFailure { code, message: one sentence, retryable }`.
5. `session_closed(run_id)`: the run is unregistered as an agent-maker run; the draft is kept.

**I. Rehearsal (ADR-083, ADR-090 §4).** `rehearse`: the draft must pass its checks (`AUTHR_CHECKS_FAILING`). Situations: the person's (from the agent maker's suggestion or their own) plus, for every deterministic expectation of the draft's `publishGate` evaluation part, a fixed situation (*reading taken*: model-judged expectations become opinions only). Each expectation is stored as 2C `RehearsalExpectation { text, rule, outcome }`: `rule` is the `ExpectationRule` the person chose on the form or the publish gate supplied, `None` for free text (SA-RECON-09). For each situation: `host.rehearse(RehearsalRunRequest { … })` → `RunEvidence` (SA-CLOSE-06: the Ordin Host waits for the run to end and fills `run_id`, `final_message`, `tools_used`, `approvals_asked`, `external_actions` and `artefact_names`); an `OhError` → `AUTHR_REHEARSAL_FAILED` with `retryable` copied from `OhError::retryable()`; `checker.check_rehearsal(&rules, &evidence)` → `ordin_checker::ExpectationOutcome`s, each mapped to `outcome = Met` when `met`, else `NotMet`; an expectation without a rule gets its outcome from the reviewer model's reading and is labelled an opinion; `host.generate(template "rehearsal-review", data = {situation, result})` → `reviewer_opinion`, labelled as a model's opinion, deciding nothing (ADR-085). The rule-carrying outcomes are passed to the Checker as `AgentDraftCheck.rehearsal` (§E). Before the first situation, the service commits the worktree locally with `git2` (message "ordin rehearsal snapshot") so the rehearsal names a commit; the commit is never pushed on its own (the Push Service pushes the branch at publish). Insert the `Rehearsal` once (`draft_version` = that commit); `journal.append_entry(NewJournalEntry { kind: "authoring_rehearsed", component: "authoring", run_id: None, workspace_id: Some(workspace_id), summary, data: {draft_id, situations, met, not_met} })`.

**J. Try it (ADR-083).** Every check and the whole rehearsal pass and the rehearsal is current → `host.add_trial`; `trial = true`; else `AUTHR_NOT_READY_TO_TRY`. The trust condition is checked by the Package Catalogue: `OhError::DependencyFailed { code: "PKG_TRIAL_NEEDS_TRUST", .. }` → `AUTHR_TRIAL_NEEDS_TRUST`; any other `OhError` → `AUTHR_TRIAL_FAILED` with `retryable` copied from `OhError::retryable()`.

**K. Publish and the review (ADR-084).**
1. `publish`: `context` is `Some` only for a Journey (lifecycle) part draft, filled by the program from the Handover Service before it forwards `authoring.publish` (*Called by*); `Some` for any other draft → `AUTHR_PUBLISH_CONTEXT_UNEXPECTED`; `None` for a Journey draft → the card states the facts as unknown ("ordin couldn't tell which workspaces follow this lifecycle"). `can_publish` must hold → else `AUTHR_NOT_READY_TO_PUBLISH`; an open pull request → `ADRAFT_PR_ALREADY_OPEN` / `PDRAFT_PR_ALREADY_OPEN`.
2. Body, in fixed code (*chosen here* layout): title "[name]: new agent" / "[name]: [change kind in words]"; sections **What this is**, **Checked by ordin** (each check passed), **Rehearsal** (situations, expectations met; opinions labelled "from a model"), **Who wrote each part** (field authorship, ADR-090 §7), **Used by** (part drafts), **Withdrawn list entry** (withdrawals). Labels: `ordin-agent` or `ordin-part` (*chosen here*). Reviewers: `catalogue.reviewers_for(repo, changed paths)` — when not readable (C-67 fallback) the card says reviewers are set on GitHub; when no review is required, the card says so (R-79).
3. Card text: question "Send "[name]" to [display name] for review?", verb "Send for review", where "Agent repository — [display name], on GitHub" (ADR-086), affects the agent or part and, for part drafts, the agents that use it; changes "The reviewers see [what the pull request holds]"; content `Files` with the changed files (business users see them in plain words on the card, 03 *Publish an agent*). Withdrawal cards never make **Send for review** the default button (11 checklist item 43) — the card's `reject_style = NotNow` and the screen draws no default (*reading taken*).
4. `push.propose_pull_request(PullRequestProposal { workspace_id, origin: SubmissionOrigin::AgentDraft { id } | PartDraft { id }, host, repository, content: PullRequestContent::Worktree { path, paths }, branch, base, title, body, labels, reviewers, existing: None, card })`; a `PushError` → `AUTHR_SEND_FAILED` with its `retryable` and `action`. On `PushNotice::PullRequestOpened` → `pull_request` set, `state = waiting_for_review`, notice. `PullRequestFailed` → stays `draft`, notice `SendFailed` (e.g. `PUSH_NO_REVIEW_RIGHT`: "the draft stays theirs"). `PullRequestRejected` → stays `draft`.
5. Follow: on every `CatalogueNotice::RepositoryUpdated` for the draft's repository and every 15 minutes, `catalogue.pull_request_status`: `ChangesRequested` → `changes_asked_for` with the reviewers' words (data, never instructions); `Merged` → if the repository requires signed merges and `merged_signed = Some(false)` → `approved_not_signed_yet` (shown "Approved on GitHub — not signed yet" with **Open the review on GitHub**), else `published`; `Closed` → `draft` with the reviewer's words. `published`: close the worktree with `catalogue.close_draft_worktree(branch, agent_repository_id)`; a new part's `part_id` is filled from the Catalogue once ingested.
6. `send_change` (`changes_asked_for` only, else `AUTHR_NO_CHANGE_REQUESTED`): card "Send the change to "[name]" for review?", affects names the review already open (11 §5.28); `propose_pull_request` with `existing: Some(pull_request)`; on success → `waiting_for_review`.
7. The protected branch moved while a part draft is open: re-check against the new head before **Publish…** is available (03 *Change a part* error posture).

**L. Import (ADR-094).** Read the folder (or the one file) into a `SourceTree`. An archive (`source` ending `.zip`, compared without case; SA-CLOSE-03) is read with `zip::ZipArchive` instead: each entry's name must be a relative path with no `..` segment, no leading `/` or drive letter, no `\` and valid UTF-8; directory entries are skipped; more than 2,000 entries, more than 50 MB unpacked in total (counted while reading, never trusting the header sizes), or a refused name → `AUTHR_ARCHIVE_INVALID` (permanent) and nothing is kept; the entries become the `SourceTree` in memory (nothing is unpacked to disk). A whole agent: `formats.read_any(&tree, &ReadRequest { publisher, vocabulary: <the target repository's vocabulary set from the Catalogue Service's `PartSource::vocabulary`>, source_label })`; `FMT_NOT_RECOGNISED` → `AUTHR_FORMAT_NOT_RECOGNISED` (one sentence with the formats ordin reads), other `FormatError`s → `AUTHR_IMPORT_FAILED` with the code in `detail`; the parts become an agent draft with `origin = imported`, *where it came from* naming the source. One file (**Import as a part…**): `formats.propose_kind(path, bytes)` → `KindProposal` shown for the person to confirm (`FMT_KIND_NOT_PROPOSABLE` → the person chooses the kind); with the confirmed `as_part`, a part draft of that kind in the chosen repository. Each mapping-report item (converted to `ReportItem`, §E1) becomes a failing check "[what] has no place in an ordin agent" with **Leave it out** and **Ask Ordin to fix it**; a tool the profile does not allow fails with **Leave it out** and **Ask IT**. Nothing in an imported file is followed as an instruction (R-84); the import, its fixes and who made each are recorded with `append_entry` (kind `authoring_import`, component `authoring`).

**O. Lifecycle of a draft (`lifecycle_of`, SA-CLOSE-01).**
1. When a part draft of kind Journey is opened by `change_part`, `stop_using_in_new_agents` or `withdraw`: `catalogue.lifecycles(Some(agent_repository_id))` → the `Lifecycle` whose `identifier` equals the part's identifier; its `id` is stored in `part_drafts.lifecycle_id` (none found, or a `CatalogueError` → `NULL`, logged `authoring.lifecycle_unknown` at `warn`). A `new_part` Journey draft stores `NULL`: no workspace follows a lifecycle before it is published.
2. `run` loads `(id, lifecycle_id)` of every part draft whose `lifecycle_id` is set and whose state is not `published` or `discarded` into an in-memory `std::sync::RwLock<HashMap<PartDraftId, LifecycleId>>` before it acts on its first `PushNotice` or `CatalogueNotice` (§K4, §K5), and marks the map loaded; every insert, state change and prune of a part draft updates the map in the same call.
3. `lifecycle_of(draft)`: map not loaded yet → `AUTHR_STARTING` (retryable); `DraftRef::Agent` → `Ok(None)`; `DraftRef::Part { id }` → `Ok(map.get(id).copied())`. A part draft unknown to the map is answered `Ok(None)` (the program then sends no `PublishContext`, and `publish` refuses an unknown draft with `AUTHR_DRAFT_UNKNOWN`). No I/O and no logging.

**M. Discard.** `draft` or `changes_asked_for` → `discarded`; worktree closed with `catalogue.close_draft_worktree(branch, agent_repository_id)`; branch deleted locally (the pull request, if any, is left to GitHub). Other states → `AUTHR_DRAFT_NOT_EDITABLE`.

**W. Writing files (ADR-085, ADR-088 decision 61).** `plugin = formats.get(FormatId::from(repository.format))`; `existing` = the worktree read into a `SourceTree`; `plugin.write(&WriteRequest { parts, borrowed: <identifiers of referenced parts that live in another repository>, bodies: <Markdown text of each new or changed part, keyed by `metadata.id`>, package: <for APM, the repository's package name and the draft's computed version>, existing, starting_vocabulary })`. `starting_vocabulary` for an ordin-format target = the bundled catalogue's Vocabulary parts: `PartSource::repository_parts(&RepositoryRef::Bundled)` on the Catalogue Service, kind Vocabulary, each converted to `ModelPart` as in §B; empty for APM. The service writes every `files` and `starting_files` entry to the worktree itself (mode `0600`, paths validated as repository-relative) and keeps `loss_report` for §E. Wording rules run before the write: `checker.apply_wording_rules(&<(field, text) pairs of name, purpose, jobs, tasks, will, wont, needs and each changed part's description>, language)` → the tidied pairs replace the fields and the `WordingChange`s are stored in `wording_changes` ("ordin tidied N things", ADR-085); its findings are shown with the next check. `FMT_WRITE_UNSUPPORTED` → `AUTHR_FORMAT_NOT_WRITABLE`; other `FormatError`s → `AUTHR_STORE_FAILED` with the code in `detail`.

**N. Pruning history (`PruneParticipant`, SA-RECON-04).** `module()` returns `"authoring"`. `prune(store, request)` runs one transaction on `store`:
1. Select agent drafts and part drafts in state `published` or `discarded` with `updated_at < request.cutoff`. Drafts in any other state are never removed: an open draft may have a publish card pending approval (A-08).
2. Exclude an agent draft whose `agent_maker_run_id`, or any `run_ids` entry of one of its rehearsals, is in `request.runs_in_progress`.
3. Delete the selected agent drafts' `rehearsals` rows (the immutability trigger guards updates only), then the agent drafts (`draft_field_authors` cascades), then the selected part drafts.
4. Delete `rehearsals` rows with `rehearsed_at < request.cutoff` that are not the `rehearsal_id` of a remaining agent draft and whose `run_ids` share nothing with `request.runs_in_progress`.
5. Return the number of rows deleted across the three tables. A datastore failure rolls back and returns `PruneFailure { code: "AUTHR_STORE_FAILED" }`. Worktrees of published and discarded drafts were closed at §K5 / §M, so no file is touched.
6. Log `authoring.pruned` {drafts, rehearsals, cutoff}.

## Data Model

Module `authoring`, journal database. Not in the agent profile, so no PostgreSQL variant. Draft files live in worktrees under `<data root>/catalogue/worktrees/` (Catalogue Service) or `<data root>/authoring/local/`.

```sql
-- migration authoring/0001
CREATE TABLE agent_drafts (
  id TEXT PRIMARY KEY,
  agent_repository_id TEXT NOT NULL,
  name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
  purpose TEXT NOT NULL CHECK (length(purpose) BETWEEN 1 AND 500),
  jobs TEXT NOT NULL, tasks TEXT NOT NULL, will TEXT NOT NULL, wont TEXT NOT NULL, needs TEXT NOT NULL,
  reused_parts TEXT NOT NULL,
  origin TEXT NOT NULL CHECK (origin IN ('made','imported')),
  branch TEXT NOT NULL,
  check_results_ref TEXT,
  wording_changes TEXT NOT NULL,
  rehearsal_id TEXT,
  trial INTEGER NOT NULL,
  pull_request TEXT,
  state TEXT NOT NULL CHECK (state IN ('draft','waiting_for_review','changes_asked_for','approved_not_signed_yet','published','discarded')),
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
  worktree_path TEXT NOT NULL,              -- chosen here
  workspace_id TEXT NOT NULL,               -- chosen here: where the draft was started
  agent_maker_run_id TEXT,                  -- chosen here
  imported_from TEXT,                       -- chosen here: the source line for imports
  job_tasks TEXT NOT NULL DEFAULT '{}'      -- chosen here (SA-RECON-03): JSON BTreeMap<u32, Vec<String>>
);
CREATE INDEX ix_agent_drafts_state ON agent_drafts(state);

CREATE TABLE draft_field_authors (          -- chosen here: ADR-090 §7 for agent drafts (2C has it only on PartDraft)
  agent_draft_id TEXT NOT NULL REFERENCES agent_drafts(id) ON DELETE CASCADE,
  field TEXT NOT NULL,
  written_by TEXT NOT NULL CHECK (written_by IN ('person','model_then_person','model')),
  PRIMARY KEY (agent_draft_id, field)
);

CREATE TABLE rehearsals (
  id TEXT PRIMARY KEY,
  agent_draft_id TEXT NOT NULL REFERENCES agent_drafts(id),
  draft_version TEXT NOT NULL,
  situations TEXT NOT NULL,                 -- JSON Vec<RehearsalSituation>
  run_ids TEXT NOT NULL,
  rehearsed_at TEXT NOT NULL
);
CREATE TRIGGER trg_rehearsals_immutable BEFORE UPDATE ON rehearsals
  BEGIN SELECT RAISE(ABORT, 'REH_IMMUTABLE'); END;

CREATE TABLE part_drafts (
  id TEXT PRIMARY KEY,
  agent_repository_id TEXT NOT NULL,
  change_kind TEXT NOT NULL CHECK (change_kind IN ('new_part','changed_part','deprecation','withdrawal','vocabulary_term')),
  part_id TEXT,
  fields TEXT NOT NULL CHECK (length(fields) <= 262144),
  computed_version TEXT NOT NULL,
  changes_what_part_does INTEGER,
  used_by_at_check TEXT NOT NULL,
  branch TEXT NOT NULL,
  check_results_ref TEXT,
  wording_changes TEXT NOT NULL,
  field_authors TEXT NOT NULL,
  pull_request TEXT,
  state TEXT NOT NULL CHECK (state IN ('draft','waiting_for_review','changes_asked_for','approved_not_signed_yet','published','discarded')),
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
  part_kind TEXT NOT NULL,                  -- chosen here
  worktree_path TEXT NOT NULL,              -- chosen here
  lifecycle_id TEXT                         -- SA-CLOSE-01: the Catalogue's Lifecycle id of a Journey draft's part
);  -- PDRAFT_PART_MISMATCH is enforced in code: part_id absent only for an unmerged new_part
CREATE UNIQUE INDEX ux_part_drafts_open_per_part ON part_drafts(part_id)
  WHERE part_id IS NOT NULL AND state IN ('draft','waiting_for_review','changes_asked_for','approved_not_signed_yet');
```

Retention: rehearsals with the journal's retention (A-08, 04 Data Lifecycle); drafts until published or discarded, then with the journal's retention; both removed by the `PruneParticipant` (§N). Rollback: forward-only; no destructive change.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Draft field out of its 2C range | `Err(permanent)` | `ADRAFT_NAME_INVALID` / `ADRAFT_PURPOSE_INVALID` / `ADRAFT_LIST_TOO_LONG` / `PDRAFT_FIELDS_TOO_LARGE` | `{"error":"That is longer than a draft can hold.","code":"ADRAFT_LIST_TOO_LONG","action":null}` |
| Draft not editable in its state | `Err(permanent)` | `AUTHR_DRAFT_NOT_EDITABLE` | `{"error":"This draft is waiting for review and can't be changed now.","code":"AUTHR_DRAFT_NOT_EDITABLE","action":null}` |
| Unknown draft | `Err(permanent)` | `AUTHR_DRAFT_UNKNOWN` | `{"error":"That draft is no longer here.","code":"AUTHR_DRAFT_UNKNOWN","action":null}` |
| Kind not written as a part draft | `Err(permanent)` | `AUTHR_KIND_NOT_WRITABLE` | in-process / screens defect |
| APM repository: only withdrawal | `Err(permanent)` | `AUTHR_APM_WITHDRAW_ONLY` | in-process / screens defect |
| No right to open a review | `Err(permanent)` | `PDRAFT_NO_REVIEW_RIGHT` | `{"error":"You can't send changes for review to this repository.","code":"PDRAFT_NO_REVIEW_RIGHT","action":null}` |
| Open part draft for the part exists | `Err(permanent)` | `PDRAFT_ALREADY_OPEN_FOR_PART` | `{"error":"You already have a draft of this part.","code":"PDRAFT_ALREADY_OPEN_FOR_PART","action":null}` |
| New part identifier taken | `Err(permanent)` | `AUTHR_IDENTIFIER_TAKEN` | `{"error":"A part with this name already exists here.","code":"AUTHR_IDENTIFIER_TAKEN","action":null}` |
| Correction question unanswered | `Err(permanent)` | `PDRAFT_ANSWER_MISSING` | `{"error":"Say whether this changes what the part does.","code":"PDRAFT_ANSWER_MISSING","action":null}` |
| Status term retired | `Err(permanent)` | `VOCAB_STATUS_TERM_FIXED` | `{"error":"This word can't be retired; ordin gives it meaning.","code":"VOCAB_STATUS_TERM_FIXED","action":null}` |
| Role-dossier contract has no requirement sections | `Err(permanent)` | `AUTHR_CONTRACT_EMPTY` | Diagnostics; draft shows a failed check |
| Foundry or generation failed | `Err(retryable)` | `AUTHR_FOUNDRY_FAILED` | `{"error":"The draft couldn't be made. Your answers are kept.","code":"AUTHR_FOUNDRY_FAILED","action":"try_again"}` |
| Rehearsal run failed | `Err(retryable)` | `AUTHR_REHEARSAL_FAILED` | `{"error":"The rehearsal couldn't run. Your answers are kept.","code":"AUTHR_REHEARSAL_FAILED","action":"try_again"}` |
| Rehearse with failing checks | `Err(permanent)` | `AUTHR_CHECKS_FAILING` | in-process / screens |
| Draft changed since rehearsal | `Err(permanent)` | `ADRAFT_REHEARSAL_STALE` | `{"error":"Rehearse the draft again; it changed since the last rehearsal.","code":"ADRAFT_REHEARSAL_STALE","action":null}` |
| Try before every check and rehearsal pass | `Err(permanent)` | `AUTHR_NOT_READY_TO_TRY` | in-process / screens |
| Trial needs IT approval | `Err(permanent)` | `AUTHR_TRIAL_NEEDS_TRUST` | `{"error":"This draft needs something your company hasn't allowed yet.","code":"AUTHR_TRIAL_NEEDS_TRUST","action":"send_to_it"}` |
| Publish before checks pass | `Err(permanent)` | `AUTHR_NOT_READY_TO_PUBLISH` | in-process / screens |
| A pull request is already open | `Err(permanent)` | `ADRAFT_PR_ALREADY_OPEN` / `PDRAFT_PR_ALREADY_OPEN` | in-process |
| Send change without a change request | `Err(permanent)` | `AUTHR_NO_CHANGE_REQUESTED` | in-process |
| `job_tasks` names a job index or task the draft does not hold | `Err(permanent)` | `AUTHR_JOB_TASKS_INVALID` | `{"error":"A job is linked to a task this draft doesn't have.","code":"AUTHR_JOB_TASKS_INVALID","action":null}` |
| `PublishContext` given for a draft that is not a lifecycle | `Err(permanent)` | `AUTHR_PUBLISH_CONTEXT_UNEXPECTED` | in-process / program defect |
| Prune of authoring history fails | `PruneFailure` | `AUTHR_STORE_FAILED` | recorded in the Journal's prune report; the next prune retries |
| Fix for a finding not in the report | `Err(permanent)` | `AUTHR_FINDING_UNKNOWN` | in-process |
| Import format not recognised | `Err(permanent)` | `AUTHR_FORMAT_NOT_RECOGNISED` | `{"error":"ordin reads the ordin, APM, Claude Code and Copilot formats; this isn't one of them.","code":"AUTHR_FORMAT_NOT_RECOGNISED","action":null}` |
| Tool called by another session | tool error | `AUTHR_TOOL_NOT_FOR_SESSION` | MCP error result |
| Tool call carrying file contents | tool error | `AUTHR_TOOL_FILE_CONTENT` | MCP error result |
| Checker unavailable | `Err(retryable)` | `AUTHR_CHECK_UNAVAILABLE` | `{"error":"The draft couldn't be checked right now.","code":"AUTHR_CHECK_UNAVAILABLE","action":"try_again"}` |
| Agent Model refuses the generated parts (`AM_DRAFT_FIELDS_INVALID`, `AM_DRAFT_KIND_NOT_COMPOSABLE`) | `Err(permanent)` | `AUTHR_FOUNDRY_FAILED` | `{"error":"The draft couldn't be made. Your answers are kept.","code":"AUTHR_FOUNDRY_FAILED","action":null}` |
| Part fields refused by the Agent Model (`AM_DRAFT_FIELDS_INVALID`, not size) | `Err(permanent)` | `AUTHR_FIELDS_INVALID` | `{"error":"One of these fields isn't in the form ordin expects.","code":"AUTHR_FIELDS_INVALID","action":null}` |
| Checker refuses the request (`CHK_INPUT_INVALID`, `CHK_TOO_MANY_FINDINGS`, `CHK_RUN_NOT_FOUND`, `CHK_CONTRACT_INVALID`, `CHK_SOURCE_ALREADY_BOUND`) | `Err(permanent)` | `AUTHR_CHECK_FAILED` | `{"error":"The draft couldn't be checked.","code":"AUTHR_CHECK_FAILED","action":null}` |
| Trial refused by the Ordin Host or the Package Catalogue for a reason other than trust | `Err(retryable or permanent, from OhError::retryable)` | `AUTHR_TRIAL_FAILED` | `{"error":"The draft couldn't be tried here.","code":"AUTHR_TRIAL_FAILED","action":"try_again"}` |
| Push Service refuses the pull-request proposal | `Err(from PushError)` | `AUTHR_SEND_FAILED` | `{"error":"The draft couldn't be sent for review.","code":"AUTHR_SEND_FAILED","action":"try_again"}` |
| Import read failed (any `FormatError` other than `FMT_NOT_RECOGNISED`) | `Err(permanent)` | `AUTHR_IMPORT_FAILED` | `{"error":"ordin couldn't read this import.","code":"AUTHR_IMPORT_FAILED","action":null}` |
| Target format cannot be written (`FMT_WRITE_UNSUPPORTED`) | `Err(permanent)` | `AUTHR_FORMAT_NOT_WRITABLE` | in-process / screens defect |
| Archive import with an unsafe name, over 2,000 entries or over 50 MB unpacked, or not a readable ZIP file | `Err(permanent)` | `AUTHR_ARCHIVE_INVALID` | `{"error":"ordin couldn't open this archive safely.","code":"AUTHR_ARCHIVE_INVALID","action":null}` |
| `lifecycle_of` called before `run` loaded the drafts | `Err(retryable)` | `AUTHR_STARTING` | in-process; the program retries after 1 s |
| The program cannot list the linked places for a lifecycle check | `Err(retryable)` | `AUTHR_CHECK_UNAVAILABLE` | as above |
| Datastore or worktree failure | `Err(retryable)` | `AUTHR_STORE_FAILED` | `{"error":"Something went wrong saving the draft.","code":"AUTHR_STORE_FAILED","action":"try_again"}` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Authoring Service

  Scenario: Happy path — make, check, rehearse and publish an agent
    Given the person can open a review in "Company agents"
    When the person starts Make an agent and the agent maker fills the write-up
    And the person approves Make this agent
    Then the Foundry writes the files through the ordin-format plug-in on a draft branch
    And Checked by ordin passes
    When the person rehearses three situations and every expectation is met
    And the person publishes and approves the card
    Then a pull request is opened and the draft is waiting_for_review

  Scenario: Gap check reuses library parts
    Given the write-up's "Responsibilities" section names the Skill "rate-compare"
    When the gap check runs
    Then rate-compare is in reused_parts and the row is covered

  Scenario: Loss report blocks publishing to an APM repository
    Given a draft whose ordin-only field the APM plug-in cannot write
    When the target is an APM-format repository
    Then a failing check names what cannot be written and can_publish is false

  Scenario: Tool call from a session that is not the agent maker
    Given run R is a crew member's session
    When R calls draft_update
    Then the call fails with AUTHR_TOOL_NOT_FOR_SESSION

  Scenario: Tool call carrying file contents
    When the agent maker calls draft_update with a 20,000-character string
    Then the call fails with AUTHR_TOOL_FILE_CONTENT and nothing changes

  Scenario: Stale rehearsal
    Given a rehearsed draft whose purpose was changed afterwards
    When the person presses Try it here — only you
    Then the call fails with ADRAFT_REHEARSAL_STALE

  Scenario: Reviewers ask for changes, then merge
    Given a draft waiting_for_review
    When GitHub reports changes requested with "Leave Slack out"
    Then the draft is changes_asked_for with that reviewer's words and ChangesAskedFor is sent
    When the person sends the change and approves the new card
    Then the same pull request receives the change and the draft is waiting_for_review
    When GitHub reports the pull request merged and signed
    Then the draft is published

  Scenario: Approved but not signed
    Given a repository that requires signed merges
    When the pull request is merged with an unverified commit
    Then the draft is approved_not_signed_yet

  Scenario: Correction keeps a patch version
    Given a part at 1.4.0 and a part draft changing its wording
    When the person answers No to "Does this change what the part does?"
    Then computed_version is 1.4.1

  Scenario: Retiring a status term
    When the person retires "approved" on artefact-status
    Then the call fails with VOCAB_STATUS_TERM_FIXED

  Scenario: Withdrawal is appended
    Given withdrawn.yaml holds two entries
    When the person withdraws "rate-compare" with a reason
    Then the written file holds the two entries unchanged followed by the new one

  Scenario: Job-to-task mapping reaches the Checker
    Given an agent draft with jobs "Compare quotes" and tasks "Read quotes", "Rank quotes"
    When the person links job 0 to "Read quotes" and "Rank quotes"
    Then agent_drafts.job_tasks holds {0: ["Read quotes", "Rank quotes"]}
    And the next check_agent_draft call receives that mapping as job_tasks

  Scenario: Job-to-task mapping names an unknown task
    Given an agent draft with tasks "Read quotes"
    When the person links job 0 to "Send quotes"
    Then the call fails with AUTHR_JOB_TASKS_INVALID and the draft is unchanged

  Scenario: Lifecycle publish carries the Handover Service's facts
    Given a Journey part draft followed by two workspaces on this computer with one open handover
    When the screens send authoring.publish and the program fills PublishContext from the Handover Service
    Then the card names the two workspaces and the open handover

  Scenario: Prune removes old finished drafts only
    Given a discarded agent draft last updated 400 days ago and an open draft last updated 400 days ago
    When the Journal prunes with a cutoff 365 days ago
    Then the discarded draft and its rehearsals are deleted
    And the open draft is kept

  Scenario: The lifecycle a draft changes
    Given the catalogue holds lifecycle L for the Journey part "contract-review" of repository R
    When the person opens a change to "contract-review" and the program asks lifecycle_of for that draft
    Then the answer is L
    And lifecycle_of for an agent draft answers none

  Scenario: Error — lifecycle_of before the service has loaded its drafts
    Given the service is built and run has not loaded its drafts
    When lifecycle_of is called
    Then it fails with AUTHR_STARTING and retryable is true

  Scenario: A stage's handover place outside every linked place
    Given workspace W links the Git repository "github.com/acme/contracts"
    And a lifecycle draft whose stage hands over to git target "github.com/acme/legal/reviews"
    When the draft is checked
    Then reachable_places maps "github.com/acme/legal/reviews" to false
    And the Checker reports lifecycle_place_unreachable

  Scenario: Rehearsal evidence comes from the Ordin Host
    Given a passing agent draft and a situation with the expectation ToolNotUsed "Bash"
    When the person rehearses it and the Ordin Host returns evidence with tools_used ["Read"]
    Then the expectation is met and the rehearsal is stored with that run's id

  Scenario: Error — an archive with a path leaving the folder
    Given a ZIP archive holding the entry "../outside.md"
    When the person imports it
    Then it fails with AUTHR_ARCHIVE_INVALID and no draft is created

  Scenario: Changes asked for reach Needs you
    Given the person's agent draft is waiting for review
    When the reviewer asks for changes on GitHub
    Then the draft is changes_asked_for and ChangesAskedFor is sent with the reviewers' words

  Scenario: Gap check sends requirements in batches of 50
    Given a role write-up whose contract yields 120 required sections
    When the gap check runs
    Then match_requirements is called three times with 50, 50 and 20 requirements
    And every section has a coverage row in write-up order

  Scenario: Part draft checks follow the change kind
    Given a withdrawal part draft in "Company agents"
    When the draft is checked
    Then check_withdrawal is called with the existing entries followed by the new one
    And check_part_draft and check_vocabulary are not called

  Scenario: Rule-carrying expectation is checked in fixed code
    Given a situation with the expectation "uses no external action" carrying the rule no_external_action
    And the rehearsal run recorded one external action
    When the rehearsal ends
    Then the expectation's outcome is not_met
    And an expectation without a rule is recorded as the reviewer model's opinion

  Scenario: Trial refused for trust keeps the trust code
    Given the Package Catalogue answers add_trial with PKG_TRIAL_NEEDS_TRUST
    When the Ordin Host returns OH_DEPENDENCY_FAILED carrying that code
    Then try_it fails with AUTHR_TRIAL_NEEDS_TRUST and action send_to_it

  Scenario: crucible tools are offered to the agent maker's run only
    Given run M is the registered agent-maker run of an open draft and run C is a crew member's run
    When the MCP Front asks the Authoring tool provider to offer tools to each
    Then run M is offered the 17 crucible tools
    And run C is offered none

  Scenario: Starting vocabulary travels with the first write to an empty ordin repository
    Given an ordin-format target repository with no Vocabulary parts
    When the Foundry writes the draft
    Then the WriteRequest carries the bundled catalogue's Vocabulary parts as starting_vocabulary
    And the returned starting files are written to the worktree

  Scenario: Import a Claude Code set-up
    Given a folder with CLAUDE.md and .claude/skills/quote-check/SKILL.md
    When the person imports it as an agent
    Then an agent draft with origin imported exists
    And any unmapped item is a failing check offering Leave it out
```

## Performance, Security, Observability

- **Performance targets:** a tool call that reads (catalogue tools, `draft_get`) ≤ 300 ms p95 (*chosen here*); `update_*_draft` including file regeneration and part checks ≤ 2 s p95 for an agent of ≤ 200 parts (*chosen here*); the whole-repository check is the Checker's budget; Foundry and rehearsal time is dominated by the harness and has no target here.
- **Security:** files, identifiers, versions, wording and pull-request bodies come from fixed code (ADR-085); a model's text is content only, recorded as model-written (ADR-090 §7); tool calls are accepted only from the registered agent-maker run and never carry file contents; imported files and reviewers' words are data, never instructions (R-84); publishing passes through an approval card the Approval Service builds and the review on GitHub (ADR-084); no check can be waived (ADR-090); worktrees sit under the application data root with mode `0700`.
- **Observability (D, P; logs only):** `component = "authoring"`; `authoring.draft_created` {draft_id, kind: agent|part, origin, repository_id}; `authoring.foundry` {draft_id, generated_parts, result, duration_ms}; `authoring.checked` {draft_id, passed, failures, warnings}; `authoring.rehearsed` {draft_id, situations, met, not_met}; `authoring.tool_call` {run_id, tool, arguments_digest, result_code}; `authoring.publish` {draft_id, submission_id}; `authoring.review_state` {draft_id, state}; `authoring.import` {draft_id, source_format, mapping_lines}; `authoring.pruned` {drafts, rehearsals, cutoff}. Never logged: field text, write-ups, reviewer words, file contents.

## Gaps

None.

---

## Amendment January 2026 — Slack for working together (ADR-102)

> Source: decision 82; HLD 0.50.0, ADR-102.

### Shared Context (addition)

`Lifecycle`, `LifecycleStage` as CatalogueService.md copies them from 2C, now with `slack_channel: Option<StageSlackChannel>` (`StageSlackChannel { channel_id, name }`); `JourneyStage.slack_channel: Option<JourneySlackChannel { id, name }>` from the Agent Model.

### Internal Logic (change to step 8, lifecycle drafts)

Step 8 (above) gives each stage of the lifecycle form an optional **Slack channel**, picked from `slack.offered_channels` or typed, stored as `slackChannel { id, name }`.

### Data Model

Unchanged: lifecycle drafts are stored as part bytes.

### Error Table

No new code; the Checker's `lifecycle_slack_channel_invalid` finding is shown on the draft as every other lifecycle finding.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: Authoring Service — stage Slack channel

  Scenario: The author sets a stage's Slack channel
    Given a lifecycle draft with stage "Design"
    When the author sets its Slack channel to id "C04ABCDEF12" and name "design-reviews"
    Then the rendered part's stage "Design" holds slackChannel { id: C04ABCDEF12, name: design-reviews }

  Scenario: Clearing the channel removes the key
    Given a lifecycle draft whose stage "Design" has a Slack channel
    When the author clears it
    Then the rendered stage has no slackChannel key

  Scenario: A malformed channel is shown as a finding
    When the author types the channel id "design-reviews"
    Then the draft shows lifecycle_slack_channel_invalid and Publish is unavailable until it is fixed
```

### Performance, Security, Observability

Unchanged. The channel name is part content and is not logged.

### Gaps

None.
