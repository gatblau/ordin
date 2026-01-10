# SPEC: Catalogue Service

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-102) · amended January 2026 (ADR-106; promoted to Approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-catalogue` · **Build phase:** 6 · **Programs:** D P A
> **Dependencies:** Organisation Profile, Project Service, Format Plug-ins, Checker, Agent Model, Journal

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
- [Amendment January 2026 — Jira issues and handovers (ADR-106)](#amendment-january-2026--jira-issues-and-handovers-adr-106)

## Purpose

The Catalogue Service keeps a local, checked copy of every agent repository the person reads — the ones the organisation profile names, the ones the person added, and the catalogue bundled with ordin — and answers every question asked of that library: which agents and parts exist, which are offered, what a part is made of and what uses it, which requirements of a request the library covers, what a repository's withdrawn list, vocabulary and lifecycles say, and how healthy the repository is (02:439–447; ADR-082, ADR-086, ADR-088, ADR-090, ADR-091, ADR-093). It also implements the Agent Model's `PartSource`, through which the Resolver and Compiler and the Checker fetch parts (ADR-100 item 1), and it owns the clones from which the Authoring Service's drafts branch.

## Approach

The service is an ingest pipeline plus a read model: each fetch of a protected branch is read through the repository's format plug-in, checked by the Checker part by part, and written to the index in **one datastore transaction per commit** (crucible's syncworker, rewritten: all or nothing per commit, 02:443), after which every query is a SQLite read of the journal database (04 Data Lifecycle, amended 0.48.0). Search is SQLite FTS5 with `bm25` ranking and named boundary constants set for it (ADR-088, R-76). Git clone and fetch use `git2` (SA-GIT-01, settled by OQ-67) with the person's GitHub token obtained through the Project Service; GitHub API reads (permissions, signature status, pull request state, `CODEOWNERS`) go through the Project Service's `gh`. Error codes take the prefix `CAT_` (*chosen here*), plus the 2C codes `AREPO_*`, `PART_*`, `WDL_*`, `VOCAB_*` and `LIFE_*` where a 2C rule is the failure. Rejected: *a separate index database file* — 04's 0.48.0 amendment puts the index tables in the journal database; *index each part as it is read, without a per-commit transaction* — a half-read commit would offer parts whose references point into the unread half.

### Reconciliation assumptions

- **SA-RECON-01** (R6, rows 537–545): the Agent Model's `PartSource` is implemented as that spec defines it — nine methods, `Resolution`, `PartSourceError`, `SourcedPart` — with no `PartReference`, `PartPin` or `fetch_pinned`; a pinned lookup is `resolve` with a version plus the caller's digest check. `ModelPart` is read through its fields; `search_text`, `facets` and vocabulary citations are defined here (§D0).
- **SA-RECON-02** (R6, rows 626–628): the Format Plug-ins are held as the concrete `FormatPlugins` struct and called with `SourceTree` and `ReadRequest`; unreadable files and not-carried constructs come from `MappingReport` items.
- **SA-RECON-03** (R6, row 619): ingest checks call the Checker's `check_repository` with subject `RepositoryRead` and map findings to parts by `part_identifier`; the error type is `CheckError`.
- **SA-RECON-04** (R6): the profile is read through `OrganisationProfileService::current()` and `subscribe()`, and GitHub through the Project Service's `GithubAccess`; the earlier `AgentCatalogueSettings` and this spec's own copy of `GithubAccess` are dropped.
- **SA-RECON-05** (R3, R18): migrations are exported by `migrations() -> ModuleMigrations`; the Checker and the Resolver and Compiler receive this service through their `install_part_source`, called once by the program after `new`; drafts reach them per request as `SourceOverride(draft_source(...))`.
- **SA-RECON-06** (closes G-CAT-4): on the desktop and the personal server the tables live in the Journal's SQLite datastore. In the agent profile, where the service holds only the bundled catalogue (a rebuildable cache that is the same in every instance), the program opens a separate SQLite datastore at `<ORDIN_DATA_DIR>/catalogue/index.sqlite` with `Datastore::open(DatastoreConfig::Sqlite { path, backup_dir })`, applies `migrations()` to it with its own `MigrationRegistry`, and passes it as `CatalogueDeps.datastore`; the `postgres` slice stays empty, because FTS5 search has no PostgreSQL form in this spec and the scaled shape needs no shared catalogue tables.
- **SA-CLOSE-01** (closes G-CAT-5, *chosen here*): the Ordin Server program, in the agent profile only, opens `<ORDIN_DATA_DIR>/catalogue/index.sqlite` exactly as SA-RECON-06 states (creating the `catalogue` folder with mode `0700` when absent, `backup_dir` = `<ORDIN_DATA_DIR>/catalogue/backups`), applies this module's `migrations()` to it, and passes it as `CatalogueDeps.datastore`; on the desktop and the personal server the program passes the Journal's `Datastore`. A failure to open or migrate that file stops the server's start with this module's `CAT_STORE_FAILED` (OrdinServer.md owns the start-up error mapping).
- **SA-RECON-07** (R1): the `catalogue.*` command table under *Public Interface* is the authoritative list the Service API catalogue merges.

## Shared Context

### Shared types used (copied from Phase 2C, unchanged)

```rust
// ordin-types — conventions (2C domain §Conventions)
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>); // RFC 3339, UTC, ms, `Z`
pub struct Sha256Hex(pub String);                         // 64 lowercase hex chars
// uuid_id! newtypes over uuid::Uuid (v7), serde transparent:
// AgentRepositoryId, PartId, WithdrawnListEntryId, VocabularyTermId, LifecycleId,
// HandoverPlaceId, WorkspaceStageId, WorkspaceId

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AgentRepository {
    pub id: AgentRepositoryId,
    pub display_name: String,
    pub host: String,
    pub repository: String,
    pub protected_branch: String,
    pub format: AgentFormat,
    pub format_version: String,
    pub source: RepositorySource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signed_merges_required: Option<bool>,
    pub person_can_publish: bool,
    pub person_can_curate: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_fetched_commit: Option<String>,
    pub clone_path: String,
    pub state: AgentRepositoryState,
    pub added_at: Timestamp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentFormat { Ordin, Apm }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositorySource { Profile, Person }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRepositoryState { Named, Cloned, Dropped, Removed }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Part {
    pub id: PartId,
    pub agent_repository_id: AgentRepositoryId,
    pub kind: PartKind,
    pub identifier: String,          // metadata.id
    pub name: String,
    pub version: String,
    pub status: PartStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,  // metadata.supersedes
    pub owner: String,
    pub file_path: String,
    pub commit: String,
    pub content_digest: Sha256Hex,
    pub vocabulary_terms_cited: Vec<VocabularyTermId>,
    pub uses: Vec<PartId>,           // child table part_dependencies
    pub withdrawn: bool,
}
/// UpperCamelCase on the wire (crucible spelling); the one enum without rename_all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PartKind {
    SystemRole, Rule, OperatingMode, Skill, Command, ContextResource, ToolBinding,
    AgentIdentity, Capability, WorkspaceProfile, CrewProfile, Journey, PracticePack,
    BrandOverlay, JurisdictionOverlay, LanguageOverlay, SourceWorkflowOverlay,
    StyleOverlay, HandoffOverlay, Evaluation, OutputContract, Vocabulary, Guideline,
    Memory, ControlLoop,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartStatus { Draft, Approved, Deprecated }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WithdrawnListEntry {
    pub id: WithdrawnListEntryId,
    pub agent_repository_id: AgentRepositoryId,
    pub part_kind: PartKind,
    pub part_identifier: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part_content_digest: Option<Sha256Hex>,
    pub reason: String,
    pub withdrawn_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replacement_identifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replacement_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed_by_hand_seen_at: Option<Timestamp>,
    pub read_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct VocabularyTerm {
    pub id: VocabularyTermId,
    pub agent_repository_id: AgentRepositoryId,
    pub vocabulary_part_id: PartId,
    pub axis: VocabularyAxis,
    pub code: String,
    pub label: String,
    pub description: String,
    pub state: VocabularyTermState,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VocabularyAxis {
    PackClass, Category, AudienceRole, RuleSubvariant,
    ArtefactStatus, MemoryContentClass, LoopStrategy, RuntimeCapability,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VocabularyTermState { Active, Retired }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Lifecycle {
    pub id: LifecycleId,
    pub part_id: PartId,
    pub agent_repository_id: AgentRepositoryId,
    pub identifier: String,
    pub name: String,
    pub stages: Vec<LifecycleStage>,
    pub status: PartStatus,
    pub version: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LifecycleStage {
    pub name: String,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,        // WorkspaceProfile identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hands_over: Option<String>,   // OutputContract identifier
    pub handover_place_id: HandoverPlaceId,
    pub days_to_answer: u32,
    pub next: StageNext,
    pub send_backs: Vec<SendBack>,
    /// Amended January 2026 (ADR-102): the stage's Slack channel; None when the lifecycle names none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slack_channel: Option<StageSlackChannel>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StageSlackChannel {
    pub channel_id: String,   // ^[CG][A-Z0-9]{8,20}$
    pub name: String,         // 1–80 characters, not starting with '#'
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StageNext {
    One { stage: String },
    OneOf { choices: Vec<StageChoice> },
    SideBySide { stages: Vec<String> },
    Nothing,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StageChoice { pub stage: String, pub when: String }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SendBack { pub to_stage: String, pub when: String, pub max_rounds: u32 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// `Jira`: a Jira Cloud project (amended January 2026, ADR-106).
pub enum HandoverPlaceType { Git, Sharepoint, Jira }

/// 2C wire §1, used in the screens' error reply (built by the program from CatalogueError).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairAction { TryAgain, SignInAgain, SendToIt }
```

2C validation rules this component enforces (copied): `AgentRepository.host` DNS name ≤ 253, `github.com` or a GitHub host the profile names → `AREPO_HOST_UNSUPPORTED`; `repository` `owner/name`, each 1–100 chars `[A-Za-z0-9._-]` → `AREPO_NAME_INVALID`; (host, repository) unique → `AREPO_DUPLICATE`; `protected_branch` valid git ref ≤ 255 bytes → `AREPO_BRANCH_INVALID`; `signed_merges_required` present iff `source = profile` → `AREPO_SIGNED_MERGES_MISMATCH`; `last_fetched_commit` 40 lowercase hex → `AREPO_COMMIT_INVALID`; removal only for `source = person` → `AREPO_REMOVE_NOT_OWN`; add refused when the profile turns adding off → `AREPO_ADD_DISABLED`. `Part.kind` one of 25 → `PART_KIND_UNKNOWN`; `identifier` 1–200, unique per (repository, kind, version) → `PART_DUPLICATE`; absent status reads `approved`; `version` semantic version → `PART_VERSION_INVALID`; `commit` 40 hex → `PART_COMMIT_INVALID`; `supersedes` names a part of the same repository and kind → `PART_SUPERSEDES_INVALID`; `withdrawn` never set back to false → `PART_WITHDRAWAL_FINAL`. `WithdrawnListEntry`: version and digest both or neither → `WDL_VERSION_DIGEST_MISMATCH`; replacement id and version both or neither → `WDL_REPLACEMENT_INCOMPLETE`; reason 1–1000 → `WDL_REASON_INVALID`; never updated except `removed_by_hand_seen_at`, never deleted while the repository is read → `WDL_APPEND_ONLY`. `VocabularyTerm.code` `[a-z0-9-]{1,64}` unique per (repository, axis) → `VOCAB_CODE_INVALID`; label 1–120 → `VOCAB_LABEL_INVALID`; description ≤ 1000 → `VOCAB_DESCRIPTION_TOO_LONG`. `Lifecycle.stages` 1–30, names unique 1–80 → `LIFE_STAGES_INVALID`; every stage reachable → `LIFE_STAGE_UNREACHABLE`; no stage leads earlier → `LIFE_STAGE_BACKWARD`; `one_of` 2–4 → `LIFE_CHOICES_INVALID`; send-back to an earlier stage → `LIFE_SEND_BACK_NOT_EARLIER`; `max_rounds` 1–10 → `LIFE_SEND_BACK_ROUNDS_OUT_OF_RANGE`; `days_to_answer` 1–365 → `LIFE_DAYS_OUT_OF_RANGE`; role 1–200 → `LIFE_ROLE_INVALID`. Pagination: SA-PAGE-01 page form `{"items": [...], "next_cursor": "<opaque>" | null}`, page size 100, cursor ≤ 1,024 chars, malformed → `SRV_BAD_CURSOR`.

### Configuration (2D)

| Key / constant | Value | Source |
|---|---|---|
| `agent_catalogue.repositories[]` | {`key`, `display_name`, `host`, `repository`, `protected_branch`, `format` (`ordin`, `apm`), `signed_merges_required` (default `false`)} | Organisation profile (2D-2) |
| `agent_catalogue.person_may_add` | boolean, default `true` | Organisation profile (2D-2) |
| Added agent repositories, each with display name | person's setting (2D-3), stored as `agent_repositories` rows with `source = person` | 2D-3 |
| Application data root | desktop: the platform data folder the program passes in; server: `ORDIN_DATA_DIR` (2D-1a) | 2D-1a |
| Fetch interval | at start, then every 15 minutes *(assumed here: the same interval 2D-4 gives the other repository fetches; the HLD names none for the catalogue)* | this spec |
| Probe lifetime | 10 minutes *(chosen here)* | this spec |
| Index limits | at most 2,000 parts per repository, at most 10 repositories per person besides the bundled catalogue (A-42) | 10 A-42 |

### Boundary interfaces this spec uses (owned by other components; copied from their specs)

```rust
// ordin-agent-model — owned by Agent Model (AgentModel.md; ADR-100: the hub owns PartSource; this crate implements it)
pub const BUNDLED_REPOSITORY_ID: AgentRepositoryId;   // 01900000-0000-7000-8000-000000000001
pub const WITHDRAWN_LIST_SCHEMA: &str = "ordin.withdrawn/v1";
pub const WITHDRAWN_LIST_FILE_NAME: &str = "withdrawn.yaml";

pub struct SourceScope { pub home: RepositoryRef, pub profile: Vec<RepositoryRef>, pub include_bundled: bool }
pub enum RepositoryRef { Repository(AgentRepositoryId), Bundled }
pub struct SourcedPart {
    pub part: Part,                        // 2C Part; agent_repository_id = BUNDLED_REPOSITORY_ID for the bundled catalogue
    pub repository: RepositoryRef,
    pub repository_label: String,          // `<host>/<owner>/<name>` or `bundled`
    pub repository_display_name: String,   // "ordin" for the bundled catalogue
    pub manifest: Manifest,
}
pub enum Resolution { Found(SourcedPart), NotFound, Ambiguous { repositories: Vec<String> } }

#[async_trait::async_trait]
pub trait PartSource: Send + Sync {
    async fn resolve(&self, scope: &SourceScope, kind: PartKind, id: &str, version: Option<&str>) -> Result<Resolution, PartSourceError>;
    async fn resolve_any_kind(&self, scope: &SourceScope, id: &str) -> Result<Resolution, PartSourceError>;
    async fn files(&self, part: &SourcedPart) -> Result<PartFiles, PartSourceError>;
    async fn ingredients(&self, scope: &SourceScope) -> Result<Vec<SourcedPart>, PartSourceError>;
    async fn withdrawn(&self, scope: &SourceScope) -> Result<Vec<WithdrawnListEntry>, PartSourceError>;
    async fn repository_parts(&self, repository: &RepositoryRef) -> Result<Vec<SourcedPart>, PartSourceError>;
    async fn vocabulary(&self, scope: &SourceScope) -> Result<VocabularySet, PartSourceError>;
    async fn used_by(&self, part: &SourcedPart) -> Result<Vec<SourcedPart>, PartSourceError>;
    async fn citing_parts(&self, repository: &RepositoryRef, axis: VocabularyAxis, code: &str) -> Result<Vec<SourcedPart>, PartSourceError>;
}
pub enum PartSourceError {
    Unavailable { retryable: bool },                 // AM_SOURCE_UNAVAILABLE
    WithdrawnListUnreadable { repository: String },  // AM_SOURCE_WITHDRAWN_LIST_UNREADABLE
    Corrupt { part_identifier: String },             // AM_SOURCE_CORRUPT
}

// Parsed parts (fields read here; ModelPart has no accessor methods).
pub struct ModelPart { pub kind: PartKind, pub manifest: Manifest, pub manifest_path: String, pub files: PartFiles, pub content_digest: Sha256Hex }
pub struct Manifest { pub api_version: String, pub authored_kind: String, pub metadata: Metadata, pub body: BTreeMap<String, YamlValue> }
impl Manifest {
    pub fn status_or_default(&self) -> Result<PartStatus, AmError>;
    pub fn references(&self) -> Vec<ManifestRef>;
    pub fn vocabulary(&self) -> Result<Option<VocabularyBody>, AmError>;
    pub fn journey(&self) -> Result<Option<JourneyBody>, AmError>;
}
pub struct Metadata { pub id: String, pub name: String, pub version: String, pub owner: String, pub loading: Option<String>,
                      pub description: String, pub category: String, pub audience: Audience, pub status: Option<String>,
                      pub supersedes: Option<String>, /* … */ }
pub struct Audience { pub domains: Vec<String>, pub roles: Vec<String>, pub languages: Vec<String> }
pub struct ManifestRef { pub id: String, pub relation: String }
pub fn relation_kind(relation: &str) -> Option<PartKind>;
pub struct PartFiles(pub BTreeMap<String, Vec<u8>>);
pub fn content_digest(files: &PartFiles) -> Sha256Hex;
pub fn parse_manifest(path: &str, bytes: &[u8]) -> Result<Option<Manifest>, AmError>;
pub fn markdown_body(path: &str, bytes: &[u8]) -> Option<String>;
pub struct VocabularyBody { pub axis: VocabularyAxis, pub terms: Vec<VocabularyTermBody> }
pub struct VocabularyTermBody { pub code: String, pub label: String, pub description: String }
pub struct JourneyBody { pub seams: Vec<String>, pub trigger: Option<String>, pub stages: Vec<JourneyStage> }
pub struct VocabularySet;
impl VocabularySet { pub fn from_parts(parts: &[&ModelPart]) -> Result<VocabularySet, AmError>; }
pub struct Urn { pub publisher: String, pub kind_dir: String, pub categories: Vec<String>, pub slug: String }
impl Urn { pub fn parse(id: &str) -> Option<Urn>; }

// withdrawn.yaml (AgentModel.md, step 13)
pub fn parse_withdrawn_list(bytes: &[u8]) -> Result<WithdrawnListFile, AmError>;
pub struct WithdrawnListFile { pub list_schema: String, pub entries: Vec<WithdrawnFileEntry> }
pub struct WithdrawnFileEntry { pub part: WithdrawnFilePart, pub reason: String, pub withdrawn_at: String, pub replacement: Option<WithdrawnFileReplacement> }
pub struct WithdrawnFilePart { pub kind: PartKind, pub id: String, pub version: Option<String>, pub content_hash: Option<String> }
pub struct WithdrawnFileReplacement { pub id: String, pub version: Option<String> }

// Contract test (behind the `testkit` Cargo feature)
pub mod testkit {
    pub struct ContractFixture { /* repositories, withdrawn, refused */ }
    impl ContractFixture { pub fn standard() -> ContractFixture; }
    pub async fn assert_part_source_contract(source: &dyn PartSource, fixture: &ContractFixture);
}

// ordin-format-plugins — owned by Format Plug-ins (FormatPlugins.md); held as the concrete struct
pub struct FormatPlugins;
impl FormatPlugins {
    pub fn get(&self, id: FormatId) -> &dyn FormatPlugin;
    pub fn detect(&self, tree: &SourceTree) -> Result<Vec<Detection>, FormatError>;
}
pub trait FormatPlugin: Send + Sync {
    fn read(&self, tree: &SourceTree, request: &ReadRequest) -> Result<ReadOutcome, FormatError>;
}
pub enum FormatId { Ordin, Apm, ClaudeCode, Copilot }
impl FormatId { pub fn as_repository_format(self) -> Option<AgentFormat>; }
impl From<AgentFormat> for FormatId { fn from(f: AgentFormat) -> FormatId; }
pub struct SourceTree(pub BTreeMap<String, Vec<u8>>);
pub struct Detection { pub format: FormatId, pub declared_version: String, pub version_tested: bool, pub agent_count: u32 }
pub struct ReadRequest { pub publisher: String, pub vocabulary: VocabularySet, pub source_label: String }
pub struct ReadOutcome { pub parts: Vec<ModelPart>, pub mapping_report: MappingReport, pub package: Option<PackageIdentity> }
pub struct MappingReport { pub format: Option<FormatId>, pub items: Vec<MappingItem> }
pub struct MappingItem { pub source_path: String, pub construct: String, pub reason: UnmappedReason, pub part_identifier: Option<String> }
pub enum UnmappedReason { NoEquivalentKind, UnknownField, Executable, UnrecognisedPath, Unreadable }
impl UnmappedReason { pub fn code(self) -> &'static str; }
pub enum FormatError { /* FMT_* codes, e.g. TreeTooLarge { files, bytes }, NotRecognised */ }

// ordin-checker — owned by Checker (Checker.md); held as Arc<Checker>, no trait
impl Checker {
    pub async fn check_repository(&self, request: &RepositoryCheck, subject: CheckSubject) -> Result<CheckReport, CheckError>;
}
pub struct RepositoryCheck {
    pub repository: RepositoryRef, pub repository_display_name: String, pub scope: SourceScope,
    pub changes: Vec<StagedPart>, pub deletions: Vec<String>, pub source: Option<SourceOverride>,
}
pub struct StagedPart { pub part: ModelPart, pub repository: RepositoryRef, pub repository_label: String }
pub enum CheckSubject { RepositoryRead { repository: String, commit: String }, /* … */ }
pub struct CheckReport { pub check_run_id: CheckRunId, pub passed: bool, pub findings: Vec<Finding>, /* … */ }
pub struct Finding { pub code: String, pub severity: Severity, pub part_identifier: Option<String>, /* … */ }
pub enum Severity { Failure, Warning }
pub enum CheckError { /* CHK_* codes */ }
impl CheckError { pub fn retryable(&self) -> bool; }

// ordin-projects — owned by Project Service (ProjectService.md, `GithubAccess`); methods used here
#[async_trait::async_trait]
pub trait GithubAccess: Send + Sync {
    async fn git_credentials(&self, host: &str) -> Result<GitCredential, GithubError>;
    async fn repository_permission(&self, host: &str, repository: &str) -> Result<RepoPermission, GithubError>;
    async fn default_branch(&self, host: &str, repository: &str) -> Result<String, GithubError>;
    async fn commit_verified(&self, host: &str, repository: &str, sha: &str) -> Result<bool, GithubError>;
    async fn branch_review_rules(&self, host: &str, repository: &str, branch: &str) -> Result<BranchReviewRules, GithubError>;
    async fn pull_request(&self, host: &str, repository: &str, number: u64) -> Result<PullRequestView, GithubError>;
    /* other methods not used here */
}
pub struct GitToken; impl GitToken { pub fn expose(&self) -> &str; }   // Debug prints GitToken(***)
pub struct GitCredential { pub username: String, pub token: GitToken }
pub enum RepoPermission { None, Read, Triage, Write, Maintain, Admin }
pub enum PullRequestState { Open, Closed, Merged }
pub enum ReviewDecision { Pending, ChangesRequested, Approved }
pub struct ReviewComment { pub reviewer: String, pub body: String, pub at: Timestamp }
pub struct PullRequestView { pub number: u64, pub state: PullRequestState, pub review: ReviewDecision,
    pub change_requests: Vec<ReviewComment>, pub merge_commit: Option<String>, pub head: String, pub base: String, pub url: String }
pub struct BranchReviewRules { pub review_required: bool, pub code_owner_review_required: bool, pub required_approvals: u32 }
pub enum GithubError { SignInNeeded, NotFound, RateLimited { retry_after_s: u32 }, Unreachable,
    Conflict { current_sha: String }, HostNotSupported, Other(String) }      // codes GH_*

// ordin-organisation-profile — owned by Organisation Profile (OrganisationProfile.md); no per-consumer trait
impl OrganisationProfileService {
    pub fn current(&self) -> std::sync::Arc<ProfileSnapshot>;
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<std::sync::Arc<ProfileSnapshot>>;
}
// read here: current().document.agent_catalogue
pub struct AgentCatalogueSection { pub repositories: Vec<AgentRepositoryEntry>, pub person_may_add: bool,
    pub trust_policy: serde_json::Value, pub it_approval_destination: Option<HelpDestination> }
pub struct AgentRepositoryEntry { pub key: String, pub display_name: String, pub host: String, pub repository: String,
    pub protected_branch: String, pub format: RepositoryFormat, pub signed_merges_required: bool }
pub enum RepositoryFormat { Ordin, Apm }   // mapped one-to-one to 2C AgentFormat

// ordin-journal — owned by Journal and Artefact Store (JournalAndArtefactStore.md; R3)
pub enum Datastore { Sqlite(sqlx::SqlitePool), Postgres(sqlx::PgPool) }
impl Datastore { pub fn sqlite(&self) -> Option<&sqlx::SqlitePool>; }
pub type MigrationStep = (i64, &'static str, &'static str);
pub struct ModuleMigrations { pub module: &'static str, pub sqlite: &'static [MigrationStep], pub postgres: &'static [MigrationStep] }

// ordin-types — paging (2C, C-15)
pub struct Cursor(String);
pub struct Page<T> { pub items: Vec<T>, pub next_cursor: Option<Cursor> }
```

## Public Interface

Crate `ordin-catalogue`, module root `ordin_catalogue`. All types derive `Debug, Clone, Serialize, Deserialize` with `#[serde(rename_all = "snake_case")]` unless stated; inputs arriving from the Service API also carry `deny_unknown_fields`.

```rust
pub struct CatalogueService { /* private */ }

pub struct CatalogueDeps {
    /// Desktop and personal server: the Journal's datastore (SQLite). Agent profile: see SA-RECON-06.
    pub datastore: ordin_journal::Datastore,
    pub profile: std::sync::Arc<ordin_organisation_profile::OrganisationProfileService>,
    pub github: std::sync::Arc<dyn ordin_projects::GithubAccess>,
    pub formats: std::sync::Arc<ordin_format_plugins::FormatPlugins>,
    pub checker: std::sync::Arc<ordin_checker::Checker>,
}

pub struct CatalogueConfig {
    /// `<app data root>/catalogue`; clones under `clones/`, draft worktrees under `worktrees/`, probes under `probes/`.
    pub root: std::path::PathBuf,
    /// Read-only folder of the bundled catalogue shipped with the program (frozen crucible `models/`, ADR-082).
    pub bundled_catalogue: std::path::PathBuf,
    /// The ordin version, recorded as the bundled catalogue's commit value.
    pub ordin_version: String,
}

/// The bundled catalogue's fixed repository id is the Agent Model's constant, re-exported unchanged.
pub use ordin_agent_model::BUNDLED_REPOSITORY_ID;

/// Module `catalogue`; `sqlite` holds the migrations of *Data Model*, `postgres` is empty (SA-RECON-06). Registered by the program (R3).
pub fn migrations() -> ordin_journal::ModuleMigrations;   // module "catalogue" (ModuleMigrations.module)

impl CatalogueService {
    pub fn new(deps: CatalogueDeps, config: CatalogueConfig) -> Self;
    /// Indexes the bundled catalogue, reconciles profile repositories, starts the fetch loop.
    pub async fn start(&self, shutdown: tokio_util::sync::CancellationToken) -> Result<(), CatalogueError>;
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<CatalogueNotice>;
    /// Reconcile with the profile, then clone missing, fetch present and re-clone damaged profile repositories,
    /// one result per profile repository (ADR-086; Setup Assistant's AgentRepositories step).
    pub async fn sync_profile_repositories(&self) -> Result<RepositorySyncReport, CatalogueError>;

    // Repositories (Settings › Agent repositories)
    pub async fn list_repositories(&self) -> Result<Vec<RepositorySummary>, CatalogueError>;
    pub async fn probe_repository(&self, input: ProbeRepositoryInput) -> Result<RepositoryProbe, CatalogueError>;
    pub async fn add_repository(&self, input: AddRepositoryInput) -> Result<AgentRepository, CatalogueError>;
    pub async fn rename_repository(&self, input: RenameRepositoryInput) -> Result<AgentRepository, CatalogueError>;
    pub async fn remove_repository(&self, id: AgentRepositoryId) -> Result<(), CatalogueError>;
    pub async fn fetch_now(&self, id: Option<AgentRepositoryId>) -> Result<Vec<FetchOutcome>, CatalogueError>;

    // Library
    pub async fn search(&self, query: LibraryQuery) -> Result<Page<LibraryEntry>, CatalogueError>;
    pub async fn facets(&self, query: LibraryQuery) -> Result<LibraryFacets, CatalogueError>;
    pub async fn match_requirements(&self, input: MatchRequirementsInput) -> Result<Vec<RequirementMatch>, CatalogueError>;
    pub async fn part(&self, id: PartId) -> Result<PartView, CatalogueError>;
    pub async fn read_part(&self, id: PartId) -> Result<PartDocument, CatalogueError>;
    pub async fn used_by(&self, id: PartId) -> Result<UsedBy, CatalogueError>;
    pub async fn made_of(&self, agent: PartId) -> Result<MadeOf, CatalogueError>;
    pub async fn part_history(&self, id: PartId) -> Result<PartHistory, CatalogueError>;
    pub async fn similar_parts(&self, kind: PartKind, text: &str, exclude: Option<PartId>) -> Result<Vec<PartView>, CatalogueError>;
    /// One head part of one repository by kind and identifier; without `version`, the highest semantic version.
    pub async fn find_part(&self, repository: AgentRepositoryId, kind: PartKind, identifier: &str, version: Option<&str>) -> Result<PartView, CatalogueError>;
    /// The repository a workflow file format 3 names (profile key or `https://` address); `None` when no read repository matches.
    pub async fn repository_by_ref(&self, reference: &AgentRepositoryRef) -> Result<Option<AgentRepository>, CatalogueError>;
    /// The reference a workflow file format 3 writes for a repository: its profile key when it has one, else its `https://` address.
    pub async fn repository_ref(&self, id: AgentRepositoryId) -> Result<AgentRepositoryRef, CatalogueError>;
    /// The resolution scope of an agent whose home is `home` (ADR-090 §2): the home, then the profile repositories
    /// in profile order (the home excluded), then the bundled catalogue.
    pub async fn source_scope(&self, home: RepositoryRef) -> Result<SourceScope, CatalogueError>;

    // Curation reads (Parts of [display name], ADR-091)
    pub async fn health(&self, id: AgentRepositoryId) -> Result<HealthReport, CatalogueError>;
    pub async fn withdrawn_list(&self, id: AgentRepositoryId) -> Result<Vec<WithdrawnListEntry>, CatalogueError>;
    pub async fn vocabulary(&self, id: AgentRepositoryId) -> Result<Vec<VocabularyTerm>, CatalogueError>;
    pub async fn layout(&self, id: AgentRepositoryId) -> Result<RepositoryLayout, CatalogueError>;
    pub async fn reviewers_for(&self, id: AgentRepositoryId, paths: Vec<String>) -> Result<ReviewerInfo, CatalogueError>;
    pub async fn pull_request_status(&self, id: AgentRepositoryId, number: u64) -> Result<PullRequestStatus, CatalogueError>;

    // Lifecycles (Handover Service)
    pub async fn lifecycles(&self, id: Option<AgentRepositoryId>) -> Result<Vec<Lifecycle>, CatalogueError>;
    pub async fn lifecycle(&self, id: LifecycleId) -> Result<Lifecycle, CatalogueError>;
    pub async fn lifecycle_place(&self, id: HandoverPlaceId) -> Result<LifecyclePlace, CatalogueError>;

    // Drafts (Authoring Service): a worktree of the clone on a new branch from the protected branch head
    pub async fn open_draft_worktree(&self, input: DraftWorktreeInput) -> Result<DraftWorktree, CatalogueError>;
    pub async fn close_draft_worktree(&self, branch: &str, id: AgentRepositoryId) -> Result<(), CatalogueError>;
    /// A `PartSource` that reads the draft worktree first, then resolves as `self` (trials and rehearsals of drafts).
    pub async fn draft_source(&self, id: AgentRepositoryId, worktree: std::path::PathBuf) -> Result<std::sync::Arc<dyn ordin_agent_model::PartSource>, CatalogueError>;
}

#[async_trait::async_trait]
impl ordin_agent_model::PartSource for CatalogueService { /* Internal Logic §L */ }
```

Inputs and results (*chosen here*):

```rust
pub struct ProbeRepositoryInput { pub web_address: String }               // ≤ 2048 bytes
pub struct RepositoryProbe {
    pub probe_id: uuid::Uuid,
    pub host: String, pub repository: String, pub suggested_name: String,
    pub format: AgentFormat, pub agent_count: u32,
    pub person_can_publish: bool,
    pub expires_at: Timestamp,
}
pub struct AddRepositoryInput { pub probe_id: uuid::Uuid, pub display_name: String }
pub struct RenameRepositoryInput { pub id: AgentRepositoryId, pub display_name: String }
pub struct RepositorySummary {
    pub repository: AgentRepository,
    pub agent_count: u32,
    pub set_by_company: bool,             // source = profile
    pub last_fetch: Option<FetchOutcome>,
}
pub struct FetchOutcome {
    pub agent_repository_id: AgentRepositoryId,
    pub result: FetchResult,
    pub commit: Option<String>,
    pub at: Timestamp,
}
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FetchResult {
    Ingested { offered: u32, not_offered: u32 },
    Unchanged,
    MergeUnsigned,                 // ADR-084: not published until signed
    WithdrawnListUnreadable,       // fail closed, R-78
    CommitRefused { reason_code: String },
    Unreachable,
    SignInNeeded,
}

pub struct LibraryQuery {
    pub text: Option<String>,                     // ≤ 500 chars
    pub kinds: Vec<PartKind>,                     // empty = WorkspaceProfile only (the agent library)
    pub repositories: Vec<AgentRepositoryId>,     // filter "From:"
    pub area: Option<String>, pub audience: Option<String>, pub publisher: Option<String>,
    pub include_not_for_new: bool,                // deprecated parts (curators)
    pub cursor: Option<Cursor>,                   // 2C Cursor (C-15)
}
// Page<T>: the 2C paging type (Shared Context).
pub struct LibraryEntry { pub part: PartView, pub from: String /* repository display name or "ordin" */ }
pub struct LibraryFacets { pub area: Vec<(String, u32)>, pub audience: Vec<(String, u32)>, pub publisher: Vec<(String, u32)>, pub repository: Vec<(AgentRepositoryId, u32)> }

pub struct MatchRequirementsInput { pub requirements: Vec<String> }   // 1–50 items, each 1–2000 chars
#[serde(rename_all = "snake_case")]
pub enum Coverage { Covered, PartlyCovered, NotCovered }
pub struct RequirementMatch {
    pub requirement: String,
    pub coverage: Coverage,
    pub matched: Vec<PartView>,       // ≤ 3, best first
    pub shortfall: Option<String>,    // present for partly_covered
}

pub struct PartView {
    pub part: Part,
    pub summary: String,              // one sentence "what it does"
    pub offered: bool,
    pub state_label: PartStateLabel,
    pub from: String,
}
#[serde(rename_all = "snake_case")]
pub enum PartStateLabel { Current, NotForNewAgents, Withdrawn, NotOffered }
pub struct PartDocument { pub view: PartView, pub body: serde_json::Value /* Agent Model fields by kind */, pub file_text: String /* shown only in code workspaces */ }
pub struct UsedBy { pub part_id: PartId, pub agents: Vec<PartView>, pub parts: Vec<PartView> }
pub struct MadeOf { pub agent: PartView, pub groups: Vec<(PartKind, Vec<PartView>)> }
pub struct PartHistory { pub versions: Vec<PartVersionLine>, pub replaces: Vec<PartView>, pub replaced_by: Vec<PartView> }
pub struct PartVersionLine { pub version: String, pub commit: String, pub merged_at: Timestamp }

pub struct HealthReport { pub agent_repository_id: AgentRepositoryId, pub computed_at: Timestamp, pub commit: String, pub lines: Vec<HealthLine> }
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HealthLine {
    UnusedPart { part_id: PartId },
    LookAlike { part_id: PartId, similar_to: PartId },
    SharedIdentifier { identifier: String, other_repository_id: AgentRepositoryId },
    BrokenBorrowedReference { part_id: PartId, reference: String },
    UsedWhileDeprecated { part_id: PartId, used_by: Vec<PartId> },
    UsedWhileWithdrawn { part_id: PartId, used_by: Vec<PartId> },
    UncitedTerm { vocabulary_term_id: VocabularyTermId },
    JourneyNotOffered { part_id: PartId },   // a crucible journey whose seams loop (ADR-099 item 7)
}
pub struct RepositoryLayout { pub kinds: Vec<KindLine> }
pub struct KindLine { pub kind: PartKind, pub count: u32, pub directory: String, pub readme: Option<String> }
pub struct ReviewerInfo { pub readable: bool, pub review_required: Option<bool>, pub reviewers: Vec<String> }
pub struct PullRequestStatus { pub state: PullRequestState, pub review: ReviewDecision, pub change_requests: Vec<ReviewComment>, pub merged_signed: Option<bool> }
pub struct LifecyclePlace { pub id: HandoverPlaceId, pub place_type: HandoverPlaceType, pub address: String }
pub struct DraftWorktreeInput { pub agent_repository_id: AgentRepositoryId, pub branch: String /* valid git ref ≤ 255 bytes */ }
pub struct DraftWorktree { pub path: std::path::PathBuf, pub base_commit: String }

pub struct RepositorySyncReport { pub results: Vec<RepositorySyncResult> }
pub struct RepositorySyncResult { pub display_name: String, pub outcome: RepositorySyncOutcome }
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RepositorySyncOutcome { Cloned, Fetched, Repaired, Failed { code: String /* a code from the Error Table */ } }

// AgentRepositoryRef: copied from 2C wire (`workflow_file`), unchanged.
pub enum AgentRepositoryRef { ProfileKey(String), Address(String) }

#[derive(Debug, Clone)]
pub enum CatalogueNotice {
    RepositoryUpdated { agent_repository_id: AgentRepositoryId, commit: String, newly_withdrawn: Vec<WithdrawnListEntryId> },
    RepositoryDropped { agent_repository_id: AgentRepositoryId },
}

#[derive(Debug, thiserror::Error)]
#[error("{code}")]
pub struct CatalogueError {
    pub code: &'static str,            // a code from the Error Table
    pub retryable: bool,
    pub action: Option<RepairAction>,
    pub detail: String,                // Diagnostics only; never shown to the person
}
```

Service API commands (`<module>.<verb>`, SA-API-01). Under the reconciliation ruling that each module owns its command names (SA-RECON, R1), this table is the authoritative list the Service API catalogue merges; each command maps one-to-one to the method of the same verb. The Service API's earlier `agents.library`, `agents.repositories`, `agents.repository_add` and `agents.repository_remove` rows are the same operations as `catalogue.search`, `catalogue.list_repositories`, `catalogue.probe_repository` + `catalogue.add_repository` and `catalogue.remove_repository`.

| Command | Input | Result | Called by |
|---|---|---|---|
| `catalogue.list_repositories` | `Done` (no input) | `Vec<RepositorySummary>` | Screens (Settings › Agent repositories) |
| `catalogue.probe_repository` | `ProbeRepositoryInput` | `RepositoryProbe` | Screens |
| `catalogue.add_repository` | `AddRepositoryInput` | `AgentRepository` | Screens |
| `catalogue.rename_repository` | `RenameRepositoryInput` | `AgentRepository` | Screens |
| `catalogue.remove_repository` | `{ "id": AgentRepositoryId }` | `Done` | Screens |
| `catalogue.fetch_now` | `{ "id": Option<AgentRepositoryId> }` | `Vec<FetchOutcome>` | Screens |
| `catalogue.search` | `LibraryQuery` | `Page<LibraryEntry>` | Screens (agent library) |
| `catalogue.facets` | `LibraryQuery` | `LibraryFacets` | Screens |
| `catalogue.part` | `{ "id": PartId }` | `PartView` | Screens |
| `catalogue.read_part` | `{ "id": PartId }` | `PartDocument` | Screens |
| `catalogue.used_by` | `{ "id": PartId }` | `UsedBy` | Screens |
| `catalogue.made_of` | `{ "agent": PartId }` | `MadeOf` | Screens |
| `catalogue.part_history` | `{ "id": PartId }` | `PartHistory` | Screens |
| `catalogue.health` | `{ "id": AgentRepositoryId }` | `HealthReport` | Screens (Parts of [display name]) |
| `catalogue.withdrawn_list` | `{ "id": AgentRepositoryId }` | `Vec<WithdrawnListEntry>` | Screens |
| `catalogue.vocabulary` | `{ "id": AgentRepositoryId }` | `Vec<VocabularyTerm>` | Screens |
| `catalogue.layout` | `{ "id": AgentRepositoryId }` | `RepositoryLayout` | Screens |
| `catalogue.lifecycles` | `{ "id": Option<AgentRepositoryId> }` | `Vec<Lifecycle>` | Screens |
| `catalogue.lifecycle` | `{ "id": LifecycleId }` | `Lifecycle` | Screens |

`LibraryQuery.text` is limited to 500 characters (the Service API copies this type unchanged). `LibraryEntry` carries no IT-approval flag: whether an agent needs IT approval is known only after the Package Catalogue's `agents.preview_add`.

In-process only, with their callers:

| Method | Called by |
|---|---|
| `start`, `subscribe` | the program (start); Package Catalogue (`subscribe`, §C–§D of its spec); Authoring Service (`subscribe`, its §K5 review follow-up) |
| `sync_profile_repositories` | Setup Assistant (AgentRepositories step) |
| `match_requirements` | Authoring Service (dossier sections). Ordin Host does not call the Catalogue Service; its agent searches go through the Package Catalogue's `find_agents` |
| `search` (in-process) | Package Catalogue (`find_agents`); Authoring Service (agent-maker tool `catalogue_search`) |
| `facets`, `part_history` | Authoring Service (agent-maker tools `catalogue_facets`, `artefact_get`) |
| `part`, `draft_source` | Package Catalogue; Authoring Service (`part`: Foundry and tool `artefact_get`; `draft_source`: every draft check, its §E) |
| `made_of` | Authoring Service (agent-maker tool `artefact_composition`) |
| `read_part` | Handover Service (a stage's contract part, after `find_part`); Authoring Service (agent-maker tools `rule_constituents`, `memory_contract_get`) |
| `used_by`, `withdrawn_list` | Authoring Service (`used_by`: a part draft's `used_by_at_check`, its §G1; `withdrawn_list`: a withdrawal's check, its §E2) |
| `find_part` | Package Catalogue (`match_workflow_agent`, `check_proposal`, uses labels and part states); Handover Service (a stage's contract and agent parts) |
| `repository_by_ref`, `repository_ref` | Package Catalogue (`match_workflow_agent`, `agent_identity`) |
| `source_scope` | Package Catalogue (every `ResolveRequest`, `InstallCheck` and `verify_and_repair`) |
| `migrations` | the program (registers it with the Journal's `MigrationRegistry`) |
| `similar_parts` | §H here (look-alikes in the health report); no other component calls it |
| `reviewers_for`, `pull_request_status`, `open_draft_worktree`, `close_draft_worktree` | Authoring Service |
| `lifecycles`, `lifecycle`, `lifecycle_place` | Handover Service; Authoring Service (`lifecycles(Some(repository))`, to record a lifecycle draft's `LifecycleId`, AuthoringService.md SA-CLOSE-01) |
| `PartSource` implementation | Resolver and Compiler and Checker (bound once by the program through their `install_part_source`, R18); Authoring Service (`repository_parts(&RepositoryRef::Bundled)` for starting vocabulary) |
| `list_repositories` (in-process) | Package Catalogue (repository display names for the Package Inspector); Authoring Service (target repository of a new agent, its §A1) |

### Example

`catalogue.probe_repository` input `{"web_address":"https://github.com/northwind/finance-agents"}` → result:

```json
{"probe_id":"0192a2b0-1111-7aaa-8bbb-ccccdddd0001","host":"github.com","repository":"northwind/finance-agents",
 "suggested_name":"finance-agents","format":"apm","agent_count":12,"person_can_publish":false,
 "expires_at":"2026-01-10T10:25:00.000Z"}
```

`match_requirements` input `{"requirements":["Compare supplier day rates across quotes","Draft the contract"]}` → `[{"requirement":"Compare supplier day rates across quotes","coverage":"covered","matched":[{…"name":"Rate comparison"…}]},{"requirement":"Draft the contract","coverage":"not_covered","matched":[]}]`.

## Internal Logic

**A. Start (`start`).**
1. Run the bundled-catalogue ingest (§D) against `config.bundled_catalogue` as the reserved repository row `BUNDLED_REPOSITORY_ID` (the Agent Model's constant, `01900000-0000-7000-8000-000000000001`), `host = "bundled"`, `repository = "ordin/catalogue"`, `source = profile`, `signed_merges_required = Some(false)`, `person_can_publish = false`, `person_can_curate = false`, `last_fetched_commit` = none, the commit column of its parts = `ordin-<ordin_version>`. Skip when the stored ordin version equals `config.ordin_version`. The 2C host rule is not applied to this row (see findings, disagreement D-B4-1). Log `catalogue.bundled_indexed` {parts, ordin_version}.
2. Reconcile with the profile (§B); spawn the fetch loop (§C); spawn a task that re-runs §B whenever `profile.subscribe()` yields a new snapshot. In the agent profile the Organisation Profile is constructed with no profile (2B-3), so §B adds no row and only the bundled catalogue is indexed; no person repository exists because no settings screen is served. (`CatalogueMode::Package` is dropped: no component builds it; the Release Builder reads packages through its own `PackageReader`.)

**B. Reconcile with the profile.**
1. Read `profile.current().document.agent_catalogue` (`AgentCatalogueSection`; `RepositoryFormat::Ordin`/`Apm` → `AgentFormat::Ordin`/`Apm`). For each entry, validate host and repository (2C rules); an invalid entry is skipped and logged `catalogue.profile_entry_invalid` {host, reason_code}; an entry whose `key` is `ordin` (reserved for the bundled catalogue, §J2 step 3) is skipped the same way with `reason_code = CAT_PROFILE_KEY_RESERVED`.
2. Insert a row (`state = named`, `source = profile`, `profile_key` = the entry's `key`) for each entry with no (host, repository) row; update `display_name`, `profile_key`, `protected_branch`, `format`, `signed_merges_required` of existing profile rows. A row the person added that the profile now names becomes `source = profile` (*assumed here: the company's naming wins; the person's display name is replaced*).
3. Each `source = profile` row the profile no longer names: `state = dropped`; delete its clone folder, its `parts`, `part_dependencies`, `part_index`, `vocabulary_terms`, `lifecycles` rows and its FTS rows in one transaction (retention "while the profile names the repository", 04 Data Lifecycle); keep its `withdrawn_list_entries` only while another read repository borrows from it — otherwise delete them too. Emit `CatalogueNotice::RepositoryDropped`. Log `catalogue.repository_dropped`.
4. `person_may_add = false` leaves existing person rows in place (*assumed here: the profile switch governs adding only, 04:300*).

**B2. Sync profile repositories (`sync_profile_repositories`).**
1. Run §B once and wait for it.
2. For each row with `source = profile` and `state` in (`named`, `cloned`), in profile order, one at a time, with the same lock the fetch loop holds so the two never run on one repository together:
   - `state = named` → clone and ingest (§C2, §D) → `Cloned`.
   - `state = cloned` and `git2::Repository::open(clone_path)` fails, or the clone's `HEAD` cannot be peeled to a commit → delete the clone folder, set `state = named`, clone and ingest → `Repaired`; log `catalogue.clone_repaired` {repository_id}.
   - `state = cloned` otherwise → fetch and ingest (§C2–§C5) → `Fetched` (an `Unchanged` fetch also reports `Fetched`).
   - Any failure → `Failed { code }` with the code §C2 or §D assigns (`CAT_SIGN_IN_NEEDED`, `CAT_UNREACHABLE`, `CAT_FETCH_FAILED`, `CAT_STORE_FAILED`), and the loop moves to the next repository.
3. `MergeUnsigned`, `WithdrawnListUnreadable` and `CommitRefused` fetch results report `Fetched`: the clone is current and the last ingested commit stays in force (ADR-084, R-78).
4. Return the report; the call itself fails only when §B1 cannot read the datastore → `CAT_STORE_FAILED`.

**C. Fetch loop.** At start and every 15 minutes, and on `fetch_now`, for each row in `named` or `cloned`, one at a time:
1. `state = named`: clone (§C2) then ingest. `state = cloned`: fetch then ingest.
2. Clone or fetch with `git2`: `RepoBuilder` / `Remote::fetch` of `refs/heads/<protected_branch>` only, HTTPS, credentials callback returning `github.git_credentials(host)` as user-pass plaintext. Errors: `GithubError::SignInNeeded` → `FetchResult::SignInNeeded`, row unchanged, `CAT_SIGN_IN_NEEDED`; network or 5xx → `FetchResult::Unreachable`, retried at the next tick, `CAT_UNREACHABLE`; any other git error → `CAT_FETCH_FAILED`. The clone stays usable offline (R-72).
3. If the fetched head equals `last_fetched_commit`, record `Unchanged` and stop.
4. If `signed_merges_required = Some(true)`: `github.commit_verified(host, repository, head)`; false → `FetchResult::MergeUnsigned`, the head is not ingested and `last_fetched_commit` does not move (ADR-084); log `catalogue.merge_unsigned` {repository_id, commit}.
5. Ingest the head commit (§D). Record the `FetchOutcome` in `repository_fetches`.
6. A failed fetch is retried at the next tick; three consecutive `Unreachable` results double the interval for that repository up to 60 minutes and reset on success (*assumed here: A-36-style growth*).

**D. Ingest one commit (all or nothing).**
1. Check out the commit into the clone's working tree (`git2` `checkout_tree` with force; the clone is ordin's own and never edited — drafts use worktrees, §M).
2. Read `withdrawn.yaml` (`WITHDRAWN_LIST_FILE_NAME`) at the tree root (§E). Unreadable → `FetchResult::WithdrawnListUnreadable`, keep the last list read, do not ingest, log `catalogue.withdrawn_unreadable` (R-78, ADR-091 item 3). An ordin-format repository without the file has an empty list.
3. Build a `SourceTree` from the commit's tree (every blob, '/'-separated repository paths). `formats.detect(&tree)`: the first `Detection` whose `format` is `FormatId::from(row.format)` gives `format_version = declared_version`; none, or a `FormatError` → `CommitRefused { reason_code: "CAT_FORMAT_CHANGED" }` (`FormatError::TreeTooLarge` → `CommitRefused { "CAT_REPOSITORY_TOO_LARGE" }`). Read in two passes (AgentModel.md step 6): `formats.get(FormatId::from(row.format)).read(&tree, &ReadRequest { publisher, vocabulary: VocabularySet::from_parts(&[])?, source_label: row.display_name })`, then again with `vocabulary = VocabularySet::from_parts(<the Vocabulary parts of the first pass>)`; `publisher` = the repository owner segment of `row.repository` (*assumed here*: identifiers minted for APM constructs carry the owner, so two companies' imports do not collide). An `AmError` from `from_parts` (two vocabulary parts for one axis) → `CommitRefused { "VOCAB_CODE_INVALID" }`. Each `MappingItem` with `reason = Unreadable` names an unreadable file; each part named by a `MappingItem.part_identifier` is kept as **not offered** with `not_offered_reason = reason.code()`; the items go to Diagnostics (ADR-087).
3a. Map each read `ModelPart` to an index row (§D0).
4. Count parts: more than 2,000 → `CommitRefused { "CAT_REPOSITORY_TOO_LARGE" }` (A-42).
5. Validate each part's 2C rules (`PART_*`). A duplicate (kind, identifier, version) in one commit → `CommitRefused { "PART_DUPLICATE" }`.
6. `checker.check_repository(&RepositoryCheck { repository: RepositoryRef::Repository(row.id), repository_display_name: row.display_name, scope: self.source_scope(home)?, changes: <every read part as StagedPart { part, repository, repository_label: "<host>/<repository>" }>, deletions: [], source: None }, CheckSubject::RepositoryRead { repository: "<host>/<repository>", commit })` — the Checker reads borrowed parts through its bound source, which is this service (R18). Each `Finding` with `severity = Failure` and `part_identifier = Some(id)` makes that part **not offered** (`not_offered_reason` = the finding's `code`); a Failure with `part_identifier = None` refuses the commit → `CommitRefused { "CAT_CHECK_FAILED" }`; any part whose `uses` closure contains a not-offered part is also not offered. A `CheckError` → `CommitRefused { "CAT_CHECK_UNAVAILABLE" }`, retried next tick.
7. Resolve references (§L order) into dependency edges. An identifier that resolves in two repositories of the same tier → both refused for that reference: the referencing part is not offered and a `SharedIdentifier` health line is written. An unresolved reference to another repository → `BrokenBorrowedReference` health line and the part not offered.
8. In one transaction: insert a `parts` row per (kind, identifier, version) not yet present (new `PartId`, UUIDv7); set `part_index.at_head = 1` for parts present in this commit and `0` for the rest of the repository; replace `part_dependencies` for head parts; upsert `part_index` and `part_search` (FTS5) text; append new withdrawn entries (§E); set `removed_by_hand_seen_at` on stored entries absent from the file; set `parts.withdrawn = 1` for every part an entry names (all versions when version is absent); upsert `vocabulary_terms` from Vocabulary parts (`manifest.vocabulary()`: `axis`, each term's `code`, `label`, `description`), marking terms no longer listed `retired`; upsert `lifecycles` from Journey parts (§F); set `last_fetched_commit`, `state = cloned`. Any SQL error rolls the whole commit back → `CAT_STORE_FAILED` (retryable).
9. Emit `CatalogueNotice::RepositoryUpdated` with the entries appended in step 8. Spawn the health report (§H) in the background.
10. Log `catalogue.ingested` {repository_id, commit, offered, not_offered, duration_ms}.

**D0. From a read part to an index row** (row 541: `ModelPart` has fields, no accessors). `identifier` = `manifest.metadata.id`; `name` = `metadata.name`; `version` = `metadata.version`; `status` = `manifest.status_or_default()` (an `AmError` → not offered, reason `PART_VERSION_INVALID`); `supersedes` = `metadata.supersedes`; `owner` = `metadata.owner`; `file_path` = `manifest_path`; `content_digest` = `ModelPart.content_digest`; `summary` = the first sentence of `metadata.description` (text up to the first `. `, `.\n` or end, at most 300 characters); `area` = `metadata.category` (empty → none); `audience` = `metadata.audience.roles`; `publisher` = `Urn::parse(identifier).publisher` (none → `owner`); references = `manifest.references()`, each `ManifestRef.id` resolved with kind `relation_kind(relation)` (none → resolved through `resolve_any_kind`); vocabulary citations (*defined here*, row 541) = (`Category`, `metadata.category`) when not empty and (`AudienceRole`, r) for each role, each mapped to a `VocabularyTermId` of the repository's own vocabulary; `part_search.body` = `markdown_body(manifest_path, <manifest file bytes>)`, else empty; `part_index.files` = the repository paths of the part's `PartFiles` (read back by §L4).

**E. Withdrawn list file** (ADR-091 item 3; path and schema confirmed here). `withdrawn.yaml` at the repository root, parsed with the Agent Model's `parse_withdrawn_list`, which reads through Shared types' restricted reader `ordin_types::yaml::read_restricted` (decision P4-1) and maps its `YAML_FEATURE_REFUSED`, `YAML_MALFORMED` and `YAML_TOO_LARGE` to `AmError` (AgentModel.md steps 3.4 and 13):

```yaml
listSchema: ordin.withdrawn/v1
entries:
  - part: { kind: Skill, id: "urn:ordin:default:skills:rate-compare", version: "1.2.0", contentHash: "<64 hex>" }
    reason: "Uses the retired rates feed"
    withdrawnAt: "2026-01-10T09:00:00.000Z"
    replacement: { id: "urn:ordin:default:skills:rate-compare-v2", version: "2.0.0" }
```

Keys keep ADR-091's camelCase (crucible's `RevocationEntry` names; SA-JSON-01 exception for files in crucible's format). Parsing rules: any `AmError` from `parse_withdrawn_list` (including a `listSchema` other than `WITHDRAWN_LIST_SCHEMA`) → unreadable; each `WithdrawnFileEntry` validated with the `WDL_*` rules (`withdrawn_at` RFC 3339, `content_hash` 64 lowercase hex) — one invalid entry makes the whole list unreadable (fail closed). An entry is identified by (kind, id, version or none); a stored entry is never updated except `removed_by_hand_seen_at`, which is set once and logged `catalogue.withdrawn_entry_removed_by_hand` (Diagnostics). Until C-66 is confirmed, the file's presence is assumed not to alter frozen crucible's ingest; if C-66 fails, the HLD's stated consequence applies (the parity test of R-69 is run on a tree without the file) and this component is unchanged.

**F. Lifecycles** (ADR-092, ADR-099). For each Journey part read: take `manifest.journey()` (`JourneyBody`; an `AmError` → the journey is not offered); a `stages` list maps field-by-field (08 *Lifecycle part*: `id`→`name`, `role`, `agent`, `produces`→`hands_over`, `handoverPlace`→place, `answerWithinDays`→`days_to_answer` (absent → `0`, read as "no answer time set"; see findings disagreement D-B4-4, because 2C bounds the field to 1–365 while 08 makes it optional), `next`/`together`→`StageNext`, `sendsBackTo`→`send_backs`, `slackChannel`→`slack_channel` (`id`→`channel_id`, `name`→`name`; absent → `None`; amended January 2026, ADR-102)). A journey without `stages` is read through its seams in producer-to-consumer order; a seam graph with a loop → `JourneyNotOffered` health line, not offered. Each distinct (place type, address) gets a `HandoverPlaceId` from `lifecycle_places` (minted once, UUIDv7). Validate with the `LIFE_*` rules (`days_to_answer = 0` accepted as "not set"; a `slack_channel` that fails `LIFE_SLACK_CHANNEL_INVALID` is set to `None` with the health line `LifecycleSlackChannelIgnored { lifecycle, stage }` and the lifecycle stays offered — the one `LIFE_*` rule that does not withhold it, amended January 2026, ADR-102); a failing lifecycle is not offered.

**G. Search and facets.** Build an FTS5 `MATCH` expression from `text` by quoting each whitespace-separated token (no operators pass through); rank with `bm25(part_search, 4.0, 2.0, 1.0)` over (name, summary, body). Filters: `kinds` (empty = `WorkspaceProfile`), repository, area, audience, publisher; only `at_head = 1` and `offered = 1`; deprecated excluded unless `include_not_for_new`; withdrawn always excluded from the library. Order by rank then name; keyset cursor over (rank, part_id). `facets` counts the same filtered set per facet.

**H. Health report** (ADR-091 item 9), in the background after each ingest, within 10 s for 2,000 parts (A-42): unused parts (no incoming edge and not a WorkspaceProfile); look-alikes per kind (for each part, `similar_parts(kind, summary or name)` limit 6, dropping itself and scores below `PARTLY_MIN`, at most 3 — crucible `forge/similarity.go:22-25`); shared identifiers across read repositories; broken borrowed references; parts used while deprecated or withdrawn; uncited active terms; looping journeys. Stored as JSON in `repository_fetches.health`. Blocks nothing.

**I. Requirement matching** (ADR-088; crucible `buildplan.go:151-200`, rewritten).
1. Validate 1–50 requirements of 1–2000 chars → else `CAT_REQUIREMENTS_INVALID`.
2. Per requirement: collapse whitespace; empty → `not_covered` with no search.
3. Search all kinds, offered head parts, limit 20.
4. Identity arm first: a `urn:ordin:` token in the text whose slug equals a result's identifier slug, or a bare token (trimmed of `.,;:()[]"'`) equal to a result's identifier slug → `covered` with that part.
5. Otherwise the best normalised score `s = r / (1 + r)` where `r = -bm25(…)` decides: `s ≥ COVERED_MIN` → `covered`; `PARTLY_MIN ≤ s < COVERED_MIN` → `partly_covered` with shortfall "[part name] matches part of this requirement; it does not cover: [requirement]"; below → `not_covered`.
6. `COVERED_MIN = 0.60`, `PARTLY_MIN = 0.15` are named constants **conditional on C-63**: the spike sets the final values so every row of frozen crucible's build-plan fixtures classifies as crucible's (`planMetRank 0.30`, `planPartialRank 0.02`); the HLD's fallback is that the use phase of R-69 is not done until they agree, with the constants kept in one place (R-76).
7. A search error fails the whole call (`CAT_STORE_FAILED`), never a silent `not_covered` (crucible step 10).
8. More than 50 requirements: the call is refused whole with `CAT_REQUIREMENTS_INVALID`; it never truncates. A caller with more sections sends them in consecutive calls of at most 50 and concatenates the results in order.

**J. Part reads.** `part`, `read_part` (Agent Model fields; `file_text` read from the clone at `parts.commit` via `git2` blob lookup, ≤ 100 KB rendered within 200 ms, A-45), `used_by` (reverse closure over `part_dependencies` across every read repository with a recursive CTE, depth ≤ 32), `made_of` (forward closure from a WorkspaceProfile, grouped by kind in the §5.27 order), `part_history` (versions of the same (repository, kind, identifier) from `parts`, merge times from `git2` commit times, and the `supersedes` chain both ways). Unknown id → `CAT_PART_UNKNOWN`.

**J2. Lookups for the Package Catalogue.**
1. `find_part(repository, kind, identifier, version)`: unknown repository → `CAT_REPOSITORY_UNKNOWN`; select from `parts` joined with `part_index` where `at_head = 1` and the repository, kind and identifier match (and the version, when given; otherwise the highest semantic version); none → `CAT_PART_UNKNOWN`. Returns the `PartView` with its `offered` flag and `state_label`; the caller decides what a not-offered, deprecated or withdrawn part means.
2. `repository_by_ref(reference)`: `ProfileKey(key)` → the row whose stored `profile_key` equals `key` (profile key from `agent_catalogue.repositories[].key`, stored by §B step 2); `Address(address)` → parse as §K1 step 1 and match (host, repository) case-insensitively; a malformed address → `AREPO_NAME_INVALID`. Rows in `dropped` or `removed` state are not returned. No match → `Ok(None)`.
3. `repository_ref(id)`: unknown → `CAT_REPOSITORY_UNKNOWN`; `profile_key` set → `ProfileKey(profile_key)`; otherwise `Address("https://<host>/<repository>")`. The bundled catalogue has neither and returns `ProfileKey("ordin")` (*chosen here*; `repository_by_ref(ProfileKey("ordin"))` returns the bundled row, and no profile key may equal `ordin` — a profile entry with that key is skipped by §B step 1 with reason `CAT_PROFILE_KEY_RESERVED`).

**K. Repositories.**
1. `probe_repository`: parse `https://<host>/<owner>/<name>[.git][/]` → else `AREPO_NAME_INVALID`; host must be `github.com` or a host a profile repository names → `AREPO_HOST_UNSUPPORTED` (GitLab waits on OQ-45); (host, repository) already a non-removed row → `AREPO_DUPLICATE`; `person_may_add = false` → `AREPO_ADD_DISABLED` (action `send_to_it`); `github.repository_permission` = `None` or `NotFound` → `CAT_NO_READ_ACCESS`; `SignInNeeded` → `CAT_SIGN_IN_NEEDED`. Clone the default branch into `probes/<probe_id>`; `formats.detect(&tree)` → the first `Detection` whose `format.as_repository_format()` is `Some` gives `format` and `agent_count`; none, or a `FormatError` → `CAT_FORMAT_UNKNOWN` (probe folder deleted). `person_can_publish = permission ≥ Write` (*assumed here: a branch and pull request in the same repository needs write*). Keep the probe 10 minutes; a timer deletes the folder after expiry.
2. `add_repository`: unknown or expired probe → `CAT_PROBE_EXPIRED`; display name 1–120 chars, trimmed, no control characters → `CAT_NAME_INVALID`; move the probe clone to `clones/<host>/<owner>/<name>`; insert the row (`source = person`, `signed_merges_required = None`, `person_can_curate = person_can_publish`, `state = cloned`); ingest (§D). Log `catalogue.repository_added`.
3. `rename_repository`: own or profile row; profile rows refuse rename → `AREPO_REMOVE_NOT_OWN`.
4. `remove_repository`: `source = profile` → `AREPO_REMOVE_NOT_OWN`; else delete as §B3 and set `state = removed`. The person's confirmation is the screens' (11 §Confirmations).

**L. `PartSource`** (the Agent Model trait, implemented over the index; AgentModel.md step 12 is the contract).
1. **Visible parts.** Only rows with `part_index.at_head = 1` and `offered = 1` are visible to every method below; a part the ingest refused is `NotFound`, never an error (row 543). A `SourcedPart` is built from the `parts` row (2C `Part`, `uses` from `part_dependencies`), `repository` = `RepositoryRef::Bundled` for `BUNDLED_REPOSITORY_ID` and `RepositoryRef::Repository(id)` otherwise, `repository_label` = `bundled` or `<host>/<repository>`, `repository_display_name` = `ordin` or the row's `display_name`, and `manifest` parsed from the file at `parts.commit:file_path` of the clone (`git2` blob; the bundled catalogue from `config.bundled_catalogue`) with `parse_manifest`.
2. **`resolve(scope, kind, id, version)`**: search in order `scope.home`, then each of `scope.profile` in listed order, then the bundled catalogue when `include_bundled`; at each step take the rows with that kind and identifier (and `version` when given; otherwise the highest semantic version whose status is not `draft`). The first step with any hit decides: one repository → `Found`; two repositories at the same step → `Ambiguous { repositories: <their labels> }`; no step → `NotFound`. A withdrawn hit is returned as `Found` with `part.withdrawn = true`. `scope.home` is where the earlier form read a `from` repository; a person-added repository is never in `scope.profile` (`source_scope` never puts it there).
3. **`resolve_any_kind(scope, id)`**: as step 2 over every kind; two kinds at the first step with a hit → `Ambiguous`.
4. **`files(part)`**: every blob under the directory of `part.part.file_path` at `part.part.commit` that the format plug-in assigned to the part (the paths recorded at ingest), as `PartFiles`; this service never compares digests — the caller recomputes `content_digest` and refuses a mismatch with `AM_SOURCE_CORRUPT` (row 542). A commit missing from the clone → fetch once; still missing → `PartSourceError::Corrupt { part_identifier }`.
5. **`ingredients(scope)`**: every visible PracticePack and overlay kind in the repositories of `scope`, latest non-draft version per identifier, in scope order.
6. **`withdrawn(scope)`**: every `withdrawn_list_entries` row of the repositories of `scope`, entries removed by hand included (R-78). A repository whose list was never read successfully → `PartSourceError::WithdrawnListUnreadable { repository: <label> }`.
7. **`repository_parts(repository)`**: every visible part of that repository, sorted by (kind, identifier, version).
8. **`vocabulary(scope)`**: `VocabularySet::from_parts` over the visible Vocabulary parts of `scope.home`, then of the profile repositories and the bundled catalogue for axes the home does not define (each axis from the first repository in scope order that has it).
9. **`used_by(part)`**: the WorkspaceProfiles whose forward closure over `part_dependencies` includes the part, across every read repository (recursive CTE, depth ≤ 32, as §J `used_by`). Vocabulary citations are not followed here.
10. **`citing_parts(repository, axis, code)`**: the `vocabulary_terms` row of `repository` with that `axis` and `code`; none → empty list; otherwise every visible head part in every read repository whose `vocabulary_terms_cited` contains its id, sorted by (repository_label, kind, identifier).
11. **Errors.** A datastore or clone read failure → `PartSourceError::Unavailable { retryable: true }`; a stored row whose file is gone at its commit → `Corrupt { part_identifier }`. Every error is logged `catalogue.part_source_error` {identifier, error} at `warn`.
12. **Contract test.** The test suite builds a catalogue over fixture repositories equivalent to `testkit::ContractFixture::standard()` and runs `testkit::assert_part_source_contract(&catalogue, &fixture)`; every named case passes (O131).

**L2. `source_scope(home)`.** `RepositoryRef::Repository(id)` with no row in `named` or `cloned` state → `CAT_REPOSITORY_UNKNOWN`. Otherwise `SourceScope { home, profile: <every row with source = profile and state in (named, cloned), in the order the profile lists them, excluding home>, include_bundled: true }`. `RepositoryRef::Bundled` as home gives the profile repositories and `include_bundled = true` (the bundled catalogue is then reached as the home and again as the last step; step 2 stops at the first hit). A datastore error → `CAT_STORE_FAILED`.

**M. Draft worktrees.** `open_draft_worktree`: repository must have `person_can_publish` → `CAT_NO_REVIEW_RIGHT`; branch must not exist locally → `CAT_BRANCH_EXISTS`; `git2::Repository::worktree` at `worktrees/<repository_id>/<branch-slug>` from the protected branch head. `close_draft_worktree` prunes it. The Authoring Service writes files there; the Push Service commits and pushes them.

**M2. Draft source.** `draft_source(repository, worktree)`: read the worktree into a `SourceTree` and read it as §D3 into memory (no index write, no check); the returned `PartSource` answers every method of §L with the worktree's parts in place of the indexed head parts of `repository` (a worktree part shadows an indexed part with the same kind and identifier; an indexed part absent from the worktree is treated as deleted), and falls through to §L for every other repository. Worktree parts are visible whatever their status, including to `resolve` without a version (a draft's parts are `draft` until merged). For a worktree part, `SourcedPart.part.commit` is forty `0`s and `files()` returns the bytes read from the worktree. The source lives as long as the returned `Arc`; it is passed per request as `SourceOverride` to the Resolver and the Checker (R18).

**N. Reviewers and pull requests.** `reviewers_for`: read `CODEOWNERS` from the clone head (`.github/CODEOWNERS`, `CODEOWNERS`, `docs/CODEOWNERS`, first found), match `paths` with the last matching pattern; `github.branch_review_rules`. **Conditional on C-67**: when the rules read fails with `NotFound` (no admin right), `readable = false` and the approval card says reviewers are set on GitHub (R-79's response). `pull_request_status`: `github.pull_request`; when merged and the repository requires signed merges, `merged_signed = commit_verified(merge_commit)`.

## Data Model

Module `catalogue`, migrations exported by `migrations()` as `MigrationStep` tuples and registered by the program (R3); tables in the journal database on the desktop and the personal server (04 Data Lifecycle, 0.48.0) and in the agent profile's own catalogue datastore (SA-RECON-06). No PostgreSQL variant.

```sql
-- catalogue migration 1 (MigrationStep (1, "catalogue tables", <this SQL>))
CREATE TABLE agent_repositories (
  id TEXT PRIMARY KEY,                       -- UUIDv7
  display_name TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 120),
  host TEXT NOT NULL,
  repository TEXT NOT NULL,
  protected_branch TEXT NOT NULL,
  format TEXT NOT NULL CHECK (format IN ('ordin','apm')),
  format_version TEXT NOT NULL,
  source TEXT NOT NULL CHECK (source IN ('profile','person')),
  signed_merges_required INTEGER,            -- NULL iff source = 'person'
  person_can_publish INTEGER NOT NULL,
  person_can_curate INTEGER NOT NULL,
  last_fetched_commit TEXT,
  profile_key TEXT,                          -- reconciliation: profile rows only; agent_catalogue.repositories[].key
  clone_path TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('named','cloned','dropped','removed')),
  added_at TEXT NOT NULL,
  ordin_version TEXT,                        -- bundled row only
  CHECK ((source = 'profile') = (signed_merges_required IS NOT NULL))
);
CREATE UNIQUE INDEX ux_agent_repositories_host_repo ON agent_repositories(host, repository)
  WHERE state IN ('named','cloned');

CREATE TABLE parts (
  id TEXT PRIMARY KEY,
  agent_repository_id TEXT NOT NULL REFERENCES agent_repositories(id) ON DELETE CASCADE,
  kind TEXT NOT NULL, identifier TEXT NOT NULL, name TEXT NOT NULL, version TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('draft','approved','deprecated')),
  supersedes TEXT, owner TEXT NOT NULL, file_path TEXT NOT NULL, "commit" TEXT NOT NULL,
  content_digest TEXT NOT NULL, vocabulary_terms_cited TEXT NOT NULL DEFAULT '[]',
  withdrawn INTEGER NOT NULL DEFAULT 0,
  UNIQUE (agent_repository_id, kind, identifier, version)
);
CREATE INDEX ix_parts_identifier ON parts(identifier);

CREATE TABLE part_dependencies (
  part_id TEXT NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
  uses_part_id TEXT NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
  PRIMARY KEY (part_id, uses_part_id)
);
CREATE INDEX ix_part_dependencies_uses ON part_dependencies(uses_part_id);

CREATE TABLE part_index (                    -- chosen here: index data beyond the 2C Part
  part_id TEXT PRIMARY KEY REFERENCES parts(id) ON DELETE CASCADE,
  at_head INTEGER NOT NULL, offered INTEGER NOT NULL, not_offered_reason TEXT,
  summary TEXT NOT NULL, area TEXT, audience TEXT NOT NULL DEFAULT '[]', publisher TEXT NOT NULL,
  files TEXT NOT NULL DEFAULT '[]'           -- reconciliation: JSON repository paths of the part's files (§L4)
);
CREATE VIRTUAL TABLE part_search USING fts5(name, summary, body, part_id UNINDEXED, tokenize = 'unicode61 remove_diacritics 2');

CREATE TABLE withdrawn_list_entries (
  id TEXT PRIMARY KEY,
  agent_repository_id TEXT NOT NULL REFERENCES agent_repositories(id) ON DELETE CASCADE,
  part_kind TEXT NOT NULL, part_identifier TEXT NOT NULL, part_version TEXT, part_content_digest TEXT,
  reason TEXT NOT NULL CHECK (length(reason) BETWEEN 1 AND 1000),
  withdrawn_at TEXT NOT NULL, replacement_identifier TEXT, replacement_version TEXT,
  removed_by_hand_seen_at TEXT, read_at TEXT NOT NULL,
  CHECK ((part_version IS NULL) = (part_content_digest IS NULL)),
  CHECK ((replacement_identifier IS NULL) = (replacement_version IS NULL))
);
CREATE UNIQUE INDEX ux_wdl_entry ON withdrawn_list_entries(agent_repository_id, part_kind, part_identifier, IFNULL(part_version, ''));
CREATE TRIGGER trg_wdl_append_only BEFORE UPDATE ON withdrawn_list_entries
  WHEN NEW.reason IS NOT OLD.reason OR NEW.part_identifier IS NOT OLD.part_identifier
    OR NEW.part_version IS NOT OLD.part_version OR NEW.replacement_identifier IS NOT OLD.replacement_identifier
    OR (OLD.removed_by_hand_seen_at IS NOT NULL AND NEW.removed_by_hand_seen_at IS NOT OLD.removed_by_hand_seen_at)
  BEGIN SELECT RAISE(ABORT, 'WDL_APPEND_ONLY'); END;

CREATE TABLE vocabulary_terms (
  id TEXT PRIMARY KEY,
  agent_repository_id TEXT NOT NULL REFERENCES agent_repositories(id) ON DELETE CASCADE,
  vocabulary_part_id TEXT NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
  axis TEXT NOT NULL, code TEXT NOT NULL, label TEXT NOT NULL, description TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('active','retired')),
  UNIQUE (agent_repository_id, axis, code)
);

CREATE TABLE lifecycles (
  id TEXT PRIMARY KEY,
  part_id TEXT NOT NULL UNIQUE REFERENCES parts(id) ON DELETE CASCADE,
  agent_repository_id TEXT NOT NULL REFERENCES agent_repositories(id) ON DELETE CASCADE,
  identifier TEXT NOT NULL, name TEXT NOT NULL, stages TEXT NOT NULL,   -- JSON Vec<LifecycleStage>
  status TEXT NOT NULL, version TEXT NOT NULL
);
CREATE TABLE lifecycle_places (              -- chosen here: places named by lifecycle stages
  id TEXT PRIMARY KEY, place_type TEXT NOT NULL CHECK (place_type IN ('git','sharepoint')),
  address TEXT NOT NULL UNIQUE
);

CREATE TABLE repository_fetches (            -- chosen here: last outcomes and health
  agent_repository_id TEXT NOT NULL REFERENCES agent_repositories(id) ON DELETE CASCADE,
  at TEXT NOT NULL, result TEXT NOT NULL, "commit" TEXT, health TEXT,
  PRIMARY KEY (agent_repository_id, at)
);
```

Rollback: forward-only migrations (SA-XC-07); every table here is a rebuildable cache of the repositories except `withdrawn_list_entries`, whose rows keep entries removed by hand — the backup copy taken before migration is the rollback. No destructive change.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Web address not a repository URL | `Err(permanent)` | `AREPO_NAME_INVALID` | `{"error":"That isn't the address of a GitHub repository.","code":"AREPO_NAME_INVALID","action":null}` |
| Host is not GitHub or a profile host | `Err(permanent)` | `AREPO_HOST_UNSUPPORTED` | `{"error":"Only GitHub repositories can be added.","code":"AREPO_HOST_UNSUPPORTED","action":null}` |
| Repository already read | `Err(permanent)` | `AREPO_DUPLICATE` | `{"error":"This agent repository is already in your list.","code":"AREPO_DUPLICATE","action":null}` |
| Profile turns adding off | `Err(permanent)` | `AREPO_ADD_DISABLED` | `{"error":"Your company sets the agent repositories. Ask IT to add one.","code":"AREPO_ADD_DISABLED","action":"send_to_it"}` |
| Remove or rename a company repository | `Err(permanent)` | `AREPO_REMOVE_NOT_OWN` | `{"error":"This repository is set by your company.","code":"AREPO_REMOVE_NOT_OWN","action":null}` |
| No read access | `Err(permanent)` | `CAT_NO_READ_ACCESS` | `{"error":"You can't read this repository with your GitHub sign-in.","code":"CAT_NO_READ_ACCESS","action":null}` |
| GitHub sign-in missing or expired | `Err(permanent)` | `CAT_SIGN_IN_NEEDED` | `{"error":"Sign in to GitHub again to read agent repositories.","code":"CAT_SIGN_IN_NEEDED","action":"sign_in_again"}` |
| No format plug-in recognises the tree | `Err(permanent)` | `CAT_FORMAT_UNKNOWN` | `{"error":"ordin doesn't recognise the agents in this repository.","code":"CAT_FORMAT_UNKNOWN","action":null}` |
| Probe unknown or expired | `Err(permanent)` | `CAT_PROBE_EXPIRED` | `{"error":"That check has expired. Paste the address again.","code":"CAT_PROBE_EXPIRED","action":null}` |
| Display name empty, too long or with control characters | `Err(permanent)` | `CAT_NAME_INVALID` | `{"error":"Give the repository a name of up to 120 characters.","code":"CAT_NAME_INVALID","action":null}` |
| Repository unreachable (network) | `Err(retryable)` | `CAT_UNREACHABLE` | `{"error":"Can't reach the company's agent library right now.","code":"CAT_UNREACHABLE","action":"try_again"}` |
| Git fetch or clone failed otherwise | `Err(retryable)` | `CAT_FETCH_FAILED` | `{"error":"The agent repository couldn't be updated.","code":"CAT_FETCH_FAILED","action":"try_again"}` |
| Merge not signed where required | outcome `MergeUnsigned` (no error to the screens) | `CAT_MERGE_UNSIGNED` | Diagnostics only |
| Withdrawn list unreadable | outcome `WithdrawnListUnreadable` | `CAT_WITHDRAWN_LIST_UNREADABLE` | Diagnostics only |
| Commit refused (format changed, too large, duplicate, checker down) | outcome `CommitRefused` | `CAT_FORMAT_CHANGED` / `CAT_REPOSITORY_TOO_LARGE` / `PART_DUPLICATE` / `CAT_CHECK_UNAVAILABLE` | Diagnostics only |
| Datastore failure | `Err(retryable)` | `CAT_STORE_FAILED` | `{"error":"Something went wrong reading the agent library.","code":"CAT_STORE_FAILED","action":"try_again"}` |
| Unknown part, repository or lifecycle | `Err(permanent)` | `CAT_PART_UNKNOWN` / `CAT_REPOSITORY_UNKNOWN` / `CAT_LIFECYCLE_UNKNOWN` | `{"error":"That item is no longer in the library.","code":"CAT_PART_UNKNOWN","action":null}` |
| Search text over 500 chars | `Err(permanent)` | `CAT_QUERY_INVALID` | `{"error":"Search text is too long.","code":"CAT_QUERY_INVALID","action":null}` |
| Malformed cursor | `Err(permanent)` | `SRV_BAD_CURSOR` | `{"error":"The list couldn't be continued.","code":"SRV_BAD_CURSOR","action":null}` |
| Requirements list empty, over 50, or an item over 2000 chars | `Err(permanent)` | `CAT_REQUIREMENTS_INVALID` | in-process only |
| A profile entry's `key` is `ordin`, reserved for the bundled catalogue (§B step 1) | entry skipped (no error to the caller) | `CAT_PROFILE_KEY_RESERVED` | Diagnostics only; logged `catalogue.profile_entry_invalid` |
| `sync_profile_repositories`: one repository's clone, fetch or ingest fails | result `Failed { code }` for that repository; the others continue | the failing step's code | Setup Assistant shows it |
| `repository_by_ref` with a malformed address | `Err(permanent)` | `AREPO_NAME_INVALID` | in-process only |
| `find_part` with no matching head part | `Err(permanent)` | `CAT_PART_UNKNOWN` | in-process only |
| Worktree requested without review right | `Err(permanent)` | `CAT_NO_REVIEW_RIGHT` | in-process only |
| Draft branch already exists | `Err(permanent)` | `CAT_BRANCH_EXISTS` | in-process only |
| `PartSource`: identifier not found, or refused at ingest | `Ok(Resolution::NotFound)` | — | in-process only |
| `PartSource`: identifier in two repositories at the same step | `Ok(Resolution::Ambiguous { repositories })` | — | in-process only; logged `catalogue.part_source_error` |
| `PartSource`: withdrawn part | `Ok(Resolution::Found)` with `part.withdrawn = true` | — | the caller decides |
| `PartSource`: index or clone unreadable | `Err(retryable)` | `AM_SOURCE_UNAVAILABLE` (`PartSourceError::Unavailable { retryable: true }`) | in-process only |
| `PartSource`: a repository's withdrawn list never read | `Err(permanent)` | `AM_SOURCE_WITHDRAWN_LIST_UNREADABLE` | in-process only |
| `PartSource`: a stored part's file is gone at its commit | `Err(permanent)` | `AM_SOURCE_CORRUPT` | in-process only |
| Ingest: a check failure that names no part | outcome `CommitRefused` | `CAT_CHECK_FAILED` | Diagnostics only |
| `source_scope` for an unknown repository | `Err(permanent)` | `CAT_REPOSITORY_UNKNOWN` | in-process only |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Catalogue Service

  Scenario: Happy path — add an agent repository the person can read
    Given the profile allows adding repositories
    And the person can read "https://github.com/northwind/finance-agents" holding 12 APM-format agents
    When the person probes that address
    Then the probe reports format "apm", agent_count 12 and person_can_publish false
    When the person adds it named "Finance agents"
    Then an agent_repositories row with source "person" and state "cloned" exists
    And the library lists its offered agents with from "Finance agents"

  Scenario: Edge — adding turned off by the company
    Given agent_catalogue.person_may_add is false
    When the person probes any GitHub address
    Then the call fails with code AREPO_ADD_DISABLED and action send_to_it

  Scenario: Error — no read access
    Given the person's GitHub sign-in cannot read "northwind/secret-agents"
    When the person probes it
    Then the call fails with code CAT_NO_READ_ACCESS and nothing is added

  Scenario: Ingest is all or nothing per commit
    Given a repository at commit A is indexed
    When commit B is fetched and the datastore write fails midway
    Then no part of commit B is visible and last_fetched_commit is still A
    And the fetch outcome records CAT_STORE_FAILED

  Scenario: A part that fails its checks is not offered
    Given commit B holds a Skill whose vocabulary term does not exist
    When commit B is ingested
    Then that Skill and every agent using it have offered = false
    And the other parts of commit B are offered

  Scenario: Unsigned merge where signing is required
    Given a profile repository with signed_merges_required true
    And its new head commit is not verified on GitHub
    When the repository is fetched
    Then the outcome is merge_unsigned and last_fetched_commit does not move

  Scenario: Withdrawn list that cannot be read fails closed
    Given withdrawn.yaml at the new head has listSchema "other/v9"
    When the repository is fetched
    Then the outcome is withdrawn_list_unreadable
    And the last list read stays in force and no update from that repository is ingested

  Scenario: An entry removed by hand stays in force
    Given an entry withdrawing "rate-compare" 1.2.0 was read earlier
    And the new withdrawn.yaml no longer lists it
    When the repository is ingested
    Then the entry is kept with removed_by_hand_seen_at set
    And rate-compare 1.2.0 is still withdrawn

  Scenario: Requirement matching by identity
    Given the library holds a Skill with identifier "urn:ordin:default:skills:rate-compare"
    When requirements ["Use urn:ordin:default:skills:rate-compare on the quotes"] are matched
    Then the coverage is covered naming that Skill

  Scenario: Empty requirement text
    When requirements ["   "] are matched
    Then the coverage is not_covered and no search runs

  Scenario: Borrowed reference resolution order
    Given an agent in repository P references identifier X
    And X exists in the bundled catalogue and in a person-added repository Q
    When the Resolver resolves X from P
    Then the bundled catalogue's X is returned and Q is not searched

  Scenario: Same identifier in two profile repositories
    Given identifier Y exists in profile repositories P1 and P2 and not in the agent's own
    When Y is resolved with the agent's source scope
    Then Resolution::Ambiguous is returned naming the labels of P1 and P2

  Scenario: A part refused at ingest is not found
    Given a Skill of repository P failed its checks at the last ingest
    When it is resolved with P as home
    Then Resolution::NotFound is returned and no error is raised

  Scenario: A withdrawn part is returned flagged
    Given "rate-compare" 1.2.0 is named by P's withdrawn list
    When it is resolved with version "1.2.0"
    Then Resolution::Found is returned with part.withdrawn true

  Scenario: Source scope of a person-added repository's agent
    Given profile repositories P1 then P2 and a person-added repository Q
    When source_scope is called with home Q
    Then the scope is home Q, profile [P1, P2] and include_bundled true

  Scenario: The PartSource contract test passes
    Given a catalogue built over the contract fixture repositories
    When assert_part_source_contract runs against it
    Then every named case passes

  Scenario: A check failure without a part refuses the commit
    Given the Checker reports a Failure finding with no part identifier for commit B
    When commit B is ingested
    Then the outcome is commit_refused with reason CAT_CHECK_FAILED and last_fetched_commit does not move

  Scenario: Company drops a repository
    Given profile repository P is cloned and indexed
    When a new profile no longer names P
    Then P's state is dropped, its clone folder is deleted and its parts leave the library

  Scenario: Sync profile repositories repairs a damaged clone
    Given profile repositories P1 (cloned, intact), P2 (named, never cloned) and P3 (cloned, clone folder corrupted)
    When sync_profile_repositories is called
    Then the report holds P1 fetched, P2 cloned and P3 repaired
    And P3's clone folder was deleted and cloned again

  Scenario: Sync profile repositories reports one unreachable repository
    Given profile repositories P1 reachable and P2 unreachable
    When sync_profile_repositories is called
    Then the report holds P1 fetched and P2 failed with code CAT_UNREACHABLE
    And the call itself succeeds

  Scenario: Requirement list over 50 is refused whole
    When 51 requirements are matched
    Then the call fails with code CAT_REQUIREMENTS_INVALID and no search runs

  Scenario: Repository named by profile key
    Given the profile names repository "acme/agents" with key "acme-agents"
    When repository_by_ref is called with ProfileKey "acme-agents"
    Then the "acme/agents" repository is returned

  Scenario: Reviewers not readable without admin rights (C-67 fallback)
    Given the branch rules read returns not found
    When reviewers_for is called
    Then readable is false and reviewers is empty
```

## Performance, Security, Observability

- **Performance targets:** `used_by` ≤ 200 ms p95 and the health report ≤ 10 s after a fetch at 2,000 parts and 10 repositories (A-42); `made_of` ≤ 300 ms p95 for a 200-part closure and `read_part` ≤ 200 ms for a 100 KB part (A-45); `search` and `facets` ≤ 150 ms p95 (A-03); `match_requirements` ≤ 500 ms p95 for 20 requirements (*chosen here*); ingest of one 2,000-part commit ≤ 30 s excluding network (*chosen here*); resident memory of the service ≤ 64 MiB outside SQLite's page cache (*chosen here*).
- **Security:** inputs from the Service API are typed and `deny_unknown_fields` (SA-API-02); the web address is parsed, never passed to a shell; git runs in-process (`git2`), never a `git` program; every repository's content is data — no file in a clone is executed, and part text reaches the person and agents only as data (ADR-093 item 2). The token from `git_credentials` lives only inside the credentials callback and is never stored, logged or placed in an error. Clone, probe and worktree folders are created with mode `0700` under the application data root. Adding a repository never widens what an agent may run: every part passes the Checker and the trust policy (ADR-086). Profile repositories cannot be removed by the person.
- **Observability (D, P; no metrics or spans, SA-XC-04/05):** `tracing` JSON events with `component = "catalogue"`: `catalogue.bundled_indexed` {parts, ordin_version}; `catalogue.fetch` {repository_id, result, commit, duration_ms}; `catalogue.ingested` {repository_id, commit, offered, not_offered, duration_ms}; `catalogue.merge_unsigned` {repository_id, commit}; `catalogue.withdrawn_unreadable` {repository_id, commit, reason_code}; `catalogue.withdrawn_entry_removed_by_hand` {repository_id, entry_id}; `catalogue.repository_added` / `repository_removed` / `repository_dropped` {repository_id, source}; `catalogue.clone_repaired` {repository_id}; `catalogue.profile_entry_invalid` {host, reason_code}; `catalogue.part_source_error` {identifier, error}; `catalogue.match` {requirements, covered, partly, not_covered, duration_ms}. Never logged: tokens, repository addresses beyond host and `owner/name`, part text, requirement text (only counts). In the agent-profile commands the same events go to standard output.

## Gaps

None.

---

## Amendment January 2026 — Slack for working together (ADR-102)

> Source: decision 82; HLD 0.50.0, ADR-102. The Catalogue Service only reads and checks the form of a stage's Slack channel; whether that channel may be posted in is the Slack Service's decision at post time (`slack.allowed_channels`), so a lifecycle never stops being offered because of Slack.

`LifecycleStage.slack_channel` and `StageSlackChannel` are in *Shared Context*, with the 2C rule `LIFE_SLACK_CHANNEL_INVALID` (`channel_id` matches `^[CG][A-Z0-9]{8,20}$`, `name` 1–80 characters not starting with `#`); §F maps `slackChannel` and drops a malformed one with a health line (*chosen here*): a stage's Slack channel only adds notices, so its absence changes nothing else. The Checker reports the same rule as a finding on drafts, so an author sees it before publishing.

### Data Model

Unchanged: `lifecycles.stages` holds `Vec<LifecycleStage>` as JSON (CatalogueService.md:853), and the new field is optional, so rows written before January 2026 read as `slack_channel = None`. No migration step.

### Error Table (row added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A stage's `slackChannel` has a malformed id or name | health line; field dropped, lifecycle offered | `LIFE_SLACK_CHANNEL_INVALID` | Diagnostics: "Stage [stage] of [lifecycle] names a Slack channel ordin can't read; its Slack messages are left out." |

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: Catalogue Service — stage Slack channel

  Scenario: A stage's Slack channel is mapped
    Given a journey part whose stage "Design" has slackChannel { id: C04ABCDEF12, name: design-reviews }
    When the catalogue reads lifecycles
    Then lifecycle "Supplier onboarding" stage "Design" has slack_channel channel_id "C04ABCDEF12" and name "design-reviews"

  Scenario: Lifecycles without a Slack channel keep their meaning
    Given a crucible journey read through its seams
    When the catalogue reads lifecycles
    Then every stage has slack_channel None and the lifecycle is offered as before

  Scenario: A malformed channel is dropped, not the lifecycle
    Given a stage whose slackChannel id is "design-reviews"
    When the catalogue reads lifecycles
    Then the lifecycle is offered with that stage's slack_channel None
    And the health line LifecycleSlackChannelIgnored names the lifecycle and the stage
```

### Performance, Security, Observability

Unchanged budgets. The channel id and name are not logged; the health line names the lifecycle and stage only.

### Gaps

None.

## Amendment January 2026 — Jira issues and handovers (ADR-106)

> Source: decision 86 (the user's decisions of January 2026: Jira as a handover place); HLD revision 0.54.0, ADR-106. Counterpart: `components/HandoverService.md` (amendment January 2026 — Jira issues and handovers), which owns every use of a Jira place.

A lifecycle stage may name a Jira project as its handover place: `handoverPlace.type: jira` with `handoverPlace.target` the project's address. This service reads and stores it; it has no Jira edge (Programs D P A, while the Jira Adapter is D only), so it does not call Jira and does not put the address in canonical form. The Handover Service does both when a draft uses the place (its §A.6 (Jira)), and refuses a Jira place where there is no Jira Adapter with `HAND_PLACE_TYPE_UNAVAILABLE`; the lifecycle stays offered everywhere.

### Shared Context (additions)

`HandoverPlaceType::Jira` (stored `jira`), in place above.

### Internal Logic (changes)

§F: `handoverPlace.type` maps `git` → `Git`, `sharepoint` → `Sharepoint` and, from this amendment, `jira` → `Jira`. For `Jira`, the target must be an `https` address ≤ 2,048 bytes whose host ends `.atlassian.net` (A-53: Jira Cloud only). A stage whose `handoverPlace.type` is none of the three, or whose Jira target fails that rule, fails the new rule `LIFE_PLACE_INVALID` (*chosen here*; the rules above named no place check), and the lifecycle is not offered, with a health line naming the stage. Each distinct (place type, address) still gets one `HandoverPlaceId` from `lifecycle_places`.

### Data Model (additive migration)

Migration step 2, `(2, "jira lifecycle places", MIGRATION_2_SQL)` in the SQLite slice (the `postgres` slice stays empty, SA-RECON-06). SQLite cannot change a `CHECK` in place and no table refers to `lifecycle_places`, so the step builds the new table, copies every row and replaces the old one:

```sql
-- catalogue migration 2 (MigrationStep (2, "jira lifecycle places", <this SQL>))
CREATE TABLE lifecycle_places_v2 (
  id TEXT PRIMARY KEY, place_type TEXT NOT NULL CHECK (place_type IN ('git','sharepoint','jira')),
  address TEXT NOT NULL UNIQUE
);
INSERT INTO lifecycle_places_v2 SELECT id, place_type, address FROM lifecycle_places;
DROP TABLE lifecycle_places;
ALTER TABLE lifecycle_places_v2 RENAME TO lifecycle_places;
```

Rollback: the step only widens a `CHECK` list and keeps every row and every `HandoverPlaceId`. It drops and recreates the table as a widening rebuild under DatabaseMigrations rule 12a (no table refers to it), and the scenario *the migration keeps every place id* is the rule's migration test; rollback is the backup copy of DatabaseMigrations step 7 (or, in the agent profile, the catalogue datastore's own backup), restored with the previous program, which refuses the migrated file with `JRNL_SCHEMA_NEWER`.

### Error Table (rows added)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A stage's handover place type is unknown, or its Jira target is not an `https` address on `*.atlassian.net` of at most 2,048 bytes | lifecycle not offered (health line) | `LIFE_PLACE_INVALID` | — (internal) |

### Acceptance Criteria (Gherkin, added)

```gherkin
  Scenario: Happy path — a stage names a Jira project as its place
    Given a journey part whose stage "Analysis" has handoverPlace {type: jira, target: "https://acme.atlassian.net/jira/software/projects/ABC/boards/3"}
    When the agent repository is read
    Then the lifecycle is offered and lifecycle_place for that stage returns place_type jira and that address

  Scenario: Error path — a Jira target outside Jira Cloud
    Given a stage with handoverPlace {type: jira, target: "https://jira.acme.internal/projects/ABC"}
    When the agent repository is read
    Then the lifecycle is not offered with LIFE_PLACE_INVALID and a health line names the stage

  Scenario: Edge case — the migration keeps every place id
    Given lifecycle_places holds a git place and a sharepoint place
    When catalogue migration 2 runs
    Then both rows keep their ids and a row of type jira can be inserted
```

### Gaps

None.
