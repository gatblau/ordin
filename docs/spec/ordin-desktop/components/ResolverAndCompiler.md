# SPEC: Resolver and Compiler

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-resolver` · **Build phase:** 2 · **Programs:** D P A
> **Dependencies:** Agent Model

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

The Resolver and Compiler turns one agent — a WorkspaceProfile with its practice packs and overlays — into a deterministic **bundle** and its **lock file**: repository, commit and digest per part (02:453–456, ADR-082). It picks open practice packs from the languages of the workspace's linked code, otherwise from the packs the agent's maker fixed, and leaves a required choice open when the maker left it for the agent to ask, rebuilding once the person answers (ADR-089, decision 64). It chooses overlays the same way, the language overlay following the person's interface language (ADR-090 §5). It detects conflicting parts, which the Checker runs on every draft (ADR-088, decision 61), rebuilds an agent with the replacement of a withdrawn part (ADR-090 §6), rebuilds a bundle from a lock file for the Installers' repair (SA-INST-01), and composes the agent's resolved tool policy, which travels in the bundle to the install record for the Rules Engine (ADR-090 item 1). It reads parts only through the Agent Model's `PartSource`, implemented by the Catalogue Service and bound late by the program with `install_part_source` once the Catalogue Service exists, because the Catalogue Service is built after the Checker that needs this crate (ADR-100, 2B-2 D8, reconciliation ruling R18). A caller that must read a draft (a trial of the person's own draft, a draft check) passes a per-request source instead. For the Release Builder it also compiles a package's always-on instructions into `AGENTS.md` text without writing anything (`compile_instructions`).

## Approach

A port of `crucible/internal/registry/resolver` (`Resolve`, closure walk, conflict detection, lifecycle and cardinality gates, plan hash) and `crucible/internal/registry/compiler` (tool policy, neutral bundle assembly) at commit `e270904` (ADR-088), with crucible's registry-specific steps removed: no tenant, no signature verification or signing (ADR-084, R-71), no pins (ADR-088 *Conflicts*: "ordin has no pin"), no crew plans (ADR-081). The closure is walked over manifest references through `PartSource` (crucible read precomputed edges from PostgreSQL). A bundle is a directory, not crucible's zip, so the Installers read part files without unpacking and the lock file can be verified by plain digests. Rejected: (1) crucible's deterministic zip — no consumer in ordin needs one archive, and it adds a dependency; (2) storing bundle rows in a table — the crate has no Journal dependency (2B-3); the install record (Installers) and the agent instance (Package Catalogue) name the bundle by its plan hash.

## Shared Context

### Types from the Agent Model (`ordin-agent-model`), used unchanged

```rust
pub use ordin_types::{PartKind, PartStatus, Part, AgentRepositoryId, Sha256Hex, DecimalString, WithdrawnListEntry,
    VocabularyAxis, HarnessKind};

pub enum LoadingMode { AlwaysOn, OnDemand, Event, ModeSwitch, RegisteredInvocation, Structural, None }
pub struct PartFiles(pub BTreeMap<String, Vec<u8>>);
pub struct ModelPart { pub kind: PartKind, pub manifest: Manifest, pub manifest_path: String,
                       pub files: PartFiles, pub content_digest: Sha256Hex }
pub struct Manifest { /* api_version, authored_kind, metadata, body */ }
//   used readers: references(), required_practice_packs(), selectable_practice_packs(),
//   fixed_practice_packs(), overlay_offer(axis), required_overlays(), required_control_loop(),
//   allowed_operating_modes(), control_loop(), prohibited_verbs(), tool_bindings(),
//   requires_human_approval(), pack_toolchain(), memory_contract(), applies_to(),
//   status_or_default(), slug(); metadata.class, metadata.audience.languages, metadata.loading
pub struct RequiredPackRule { pub at_least: u32, pub class: Option<String>, pub ids: Vec<String> }
pub enum FixedPackEntry { Pack(String), Ask { class: Option<String>, ids: Vec<String> } }
pub struct OverlayOffer { pub available: Vec<String>, pub default: Option<String> }
pub struct ManifestRef { pub id: String, pub relation: String }
pub fn relation_kind(relation: &str) -> Option<PartKind>;
pub fn kind_info(kind: PartKind) -> &'static KindInfo;   // default_loading, exactly_one, family
pub fn overlay_order() -> &'static [PartKind; 6];
pub fn content_digest(files: &PartFiles) -> Sha256Hex;
pub fn markdown_body(path: &str, bytes: &[u8]) -> Option<String>;   // Markdown text of a part file
pub fn prefixed_digest(d: &Sha256Hex) -> String;
pub use ordin_types::encoding::canonical_json;   // re-exported by the Agent Model (Logging R-3)
pub fn compare_versions(a: &str, b: &str) -> Result<std::cmp::Ordering, AmError>;
pub const LOCK_FILE_NAME: &str = "ordin.lock";          pub const LOCK_FILE_SCHEMA: &str = "ordin.lock/v1";
pub const BUNDLE_MANIFEST_NAME: &str = "bundle.manifest.json"; pub const BUNDLE_SCHEMA: &str = "ordin.bundle/v1";

pub struct PartRef { pub kind: PartKind, pub id: String, pub name: String, pub version: String,
    pub content_hash: String, pub repository: String, pub commit: String, pub loading: Option<LoadingMode> }
pub struct ResolvedToolPolicy { pub forbidden_flags: Vec<String>, pub coverage_floor: Option<DecimalString>,
    pub prohibited_verbs: Vec<String>, pub requires_human_approval: Vec<String>, pub mcp_servers: Vec<McpServerDecl> }
pub struct McpServerDecl { pub name: String, pub server_type: String, pub scope: Option<String>,
    pub env_ref: Option<String>, pub transport: Option<String>, pub declared_by: String }
pub struct Selections { pub practice_packs: Vec<SelectedPart>, pub overlays: Vec<SelectedPart>,
    pub pending_choices: Vec<PendingPackChoice> }
pub struct SelectedPart { pub id: String, pub chosen_by: ChosenBy }
pub enum ChosenBy { WorkspaceCode, MakerFixed, PersonAnswer, PersonChange, AgentDefault, InterfaceLanguage }
pub struct PendingPackChoice { pub rule_index: u32, pub at_least: u32, pub class: Option<String>, pub choices: Vec<String> }
pub struct ResolutionPlan { pub plan_hash: String, pub root: PartRef, pub target_harness: String,
    pub overlay_order: Vec<PartRef>, pub practice_packs: Vec<PartRef>, pub resolved: Vec<PartRef>,
    pub selections: Selections, pub warnings: Vec<String>, pub withdrawn_snapshot: String,
    pub tool_policy: Option<ResolvedToolPolicy> }
pub struct BundleArtefact { pub part: PartRef, pub loading: LoadingMode }
pub struct BundleManifest { pub bundle_schema: String, pub plan_hash: String, pub composition: PartRef,
    pub artefacts: Vec<BundleArtefact>, pub overlays_applied: Vec<PartRef>,
    pub resolved_tool_policy: ResolvedToolPolicy, pub lockfile_sha256: String, pub compiler_version: String }
pub struct LockFile { pub lockfile_schema: String, pub plan_hash: String, pub target_harness: String,
    pub composition: PartRef, pub artefacts: Vec<PartRef>, pub overlays_applied: Vec<PartRef>,
    pub selections: Selections }   // to_canonical_yaml(), from_yaml()

// PartSource (owned by the Agent Model, ADR-100; implemented by the Catalogue Service)
pub struct SourceScope { pub home: RepositoryRef, pub profile: Vec<RepositoryRef>, pub include_bundled: bool }
pub enum RepositoryRef { Repository(AgentRepositoryId), Bundled }
pub struct SourcedPart { pub part: Part, pub repository: RepositoryRef, pub repository_label: String,
    pub repository_display_name: String, pub manifest: Manifest }
pub enum Resolution { Found(SourcedPart), NotFound, Ambiguous { repositories: Vec<String> } }
#[async_trait] pub trait PartSource: Send + Sync {
    async fn resolve(&self, scope: &SourceScope, kind: PartKind, id: &str, version: Option<&str>) -> Result<Resolution, PartSourceError>;
    async fn resolve_any_kind(&self, scope: &SourceScope, id: &str) -> Result<Resolution, PartSourceError>;
    async fn files(&self, part: &SourcedPart) -> Result<PartFiles, PartSourceError>;
    async fn ingredients(&self, scope: &SourceScope) -> Result<Vec<SourcedPart>, PartSourceError>;
    async fn withdrawn(&self, scope: &SourceScope) -> Result<Vec<WithdrawnListEntry>, PartSourceError>;
    async fn repository_parts(&self, repository: &RepositoryRef) -> Result<Vec<SourcedPart>, PartSourceError>;
    async fn vocabulary(&self, scope: &SourceScope) -> Result<VocabularySet, PartSourceError>;
    async fn used_by(&self, part: &SourcedPart) -> Result<Vec<SourcedPart>, PartSourceError>;
    async fn citing_parts(&self, repository: &RepositoryRef, axis: VocabularyAxis, code: &str)
        -> Result<Vec<SourcedPart>, PartSourceError>;   // not called by this crate
}
pub enum PartSourceError { Unavailable { retryable: bool }, WithdrawnListUnreadable { repository: String },
    Corrupt { part_identifier: String } }
```

### Reconciliation assumptions

- **SA-RECON-01** — The source is bound late (ruling R18): the program builds this component with `new(config)`, before the Checker, and calls `install_part_source` once, right after the Catalogue Service is built; a part-reading call before then fails with the permanent `RES_SOURCE_NOT_BOUND`.
- **SA-RECON-02** — The Agent Model owns `PartSource` and its types (ruling R6); the copy above is the Agent Model's trait verbatim, including `citing_parts`, which this crate never calls.
- **SA-RECON-03** — `ResolveRequest.target_harness` is the 2C `HarnessKind` (ruling R7); the plan and lock file keep the Agent Model's `String` field and carry the enum's `snake_case` value.
- **SA-RECON-04** — Callers own the digest check of `PartSource::files` (Agent Model, reconciliation row 542); this crate re-checks every part it reads (steps 17, 22, 22c).

### HLD rules this component implements

| Rule | Source |
|---|---|
| Open practice packs come from the languages of the workspace's linked projects matched against each pack's `metadata.audience.languages`; for a required rule as many as it needs; an optional pack whenever it matches; no model call | ADR-089 (b) |
| Otherwise the maker's fixed packs; optional packs the workspace does not match are left out unless fixed | ADR-089 (a) |
| A required rule may be left for the agent to ask; the bundle is rebuilt when the person answers | ADR-089 decision 64 |
| An agent whose required rules the fixed packs do not satisfy is offered only where the workspace's code settles every rule; elsewhere it is unavailable, saying which kind of pack is missing | ADR-089 last bullet |
| Language overlay follows the person's interface language where offered, otherwise the agent's default; other overlays take the agent's default; handoff overlays bound through `appliesTo` are read and checked, not applied | ADR-090 §5 |
| Resolution order of a reference: own repository, profile repositories, bundled catalogue; same identifier in two → refused | ADR-090 §2 (enforced by `PartSource`) |
| Withdrawn part with a named replacement: rebuild with it; otherwise the agent stops | ADR-090 §6 |
| A deprecated part resolves inside an agent with a warning; refused as an agent of its own | ADR-091 item 2; 04:315 |
| No pin; a conflict is a failing check on drafts and a refusal on read | ADR-088 *Conflicts* |
| Tool policy is not written into the agent folder's settings; it goes to the install record for the Rules Engine | ADR-090 item 1; 02:456 |

### Configuration

| Name | Value | Source |
|---|---|---|
| `bundle_root` | `<application data root>/bundles` (*chosen here*) | constructor argument from the program |
| `compiler_version` | ordin's release version (`CARGO_PKG_VERSION` of the program) | constructor argument |
| Closure limit | 1,000 parts (*assumed here*: A-45's 200 parts is the shown case; five times that bounds memory) | constant `MAX_CLOSURE_PARTS` |
| Reference depth limit | 64 (*assumed here*) | constant `MAX_DEPTH` |

No environment variable or organisation-profile key (2D).

### Closing-round assumptions

- **SA-CLOSE-01** (*chosen here*, closing round K22): `StagedPart` and `SourceOverride` move to the Agent Model (AgentModel.md, *Staged parts and per-request sources*) because the Authoring Service builds them and has no edge to this crate; this crate re-exports both under the same names, so no caller of `ordin_resolver` changes.

## Public Interface

Crate `ordin_resolver`. Names are *chosen here* unless traced.

```rust
use std::{collections::BTreeSet, path::PathBuf, sync::Arc};

/// The Agent Model types, re-exported so the Release Builder reaches them through this crate
/// (2B-3 gives the Release Builder no direct edge to `ordin-agent-model`).
pub use ordin_agent_model as model;

pub struct ResolverConfig { pub bundle_root: PathBuf, pub compiler_version: String }

/// `SourceOverride` (a source used for one request in place of the bound one) and `StagedPart`
/// (a draft's new or changed part that shadows a source part of the same kind and identifier)
/// are owned by the Agent Model and re-exported here unchanged (SA-CLOSE-01), so existing
/// callers keep `ordin_resolver::{SourceOverride, StagedPart}` and the Authoring Service, which
/// has no edge to this crate, reaches them through `ordin_agent_model`. A staged part's commit is
/// written as forty `0`s in plan and lock (*chosen here*: a staged part has no commit until merged).
pub use ordin_agent_model::{SourceOverride, StagedPart};

#[derive(Debug, Clone)]
pub struct PackAnswer { pub rule_index: u32, pub pack_ids: Vec<String> }

#[derive(Debug, Clone)]
pub struct Replacement { pub withdrawn_id: String, pub withdrawn_kind: PartKind,
                         pub replacement_id: String, pub replacement_version: Option<String> }

#[derive(Debug, Clone)]
pub struct ResolveRequest {
    pub scope: SourceScope,
    /// `metadata.id` of the WorkspaceProfile.
    pub root_id: String,
    /// None = highest non-draft version on the followed commit.
    pub root_version: Option<String>,
    /// The harness the bundle is built for (2C `HarnessKind`); written into the plan and the
    /// lock file as its `snake_case` serialisation (`claude_code`, `copilot_cli`).
    pub target_harness: HarnessKind,
    /// Lower-case language names of the workspace's linked projects (Project Service, ADR-024);
    /// empty for a workspace with no code.
    pub workspace_languages: Vec<String>,
    /// BCP 47 tag of the person's interface language (ADR-042), e.g. "en-GB".
    pub interface_language: String,
    pub pack_answers: Vec<PackAnswer>,
    /// Packs the person chose on **Change** (ADR-089), identifiers.
    pub pack_changes: Vec<String>,
    /// Overlays the person chose on **Change** (ADR-090 §5), identifiers.
    pub overlay_changes: Vec<String>,
    pub replacements: Vec<Replacement>,
    pub staged: Vec<StagedPart>,
    /// True only for a draft root (Checker, trial): the root may be `draft` status.
    pub allow_draft_root: bool,
    /// None = the source bound by `install_part_source`; Some = this source for this request only.
    pub source: Option<SourceOverride>,
}

/// `compile_instructions` output: the always-on instructions as one Markdown text, and the
/// lock file of the plan they came from. Nothing is written to disk.
#[derive(Debug, Clone)]
pub struct CompiledInstructions { pub agents_md: String, pub lock: LockFile }

/// Resolve `request` against `source` (ignoring `request.source`) and return the always-on
/// instructions as `AGENTS.md` text. No `ResolverAndCompiler` and no bound source is needed.
pub async fn compile_instructions(source: &dyn PartSource, request: &ResolveRequest)
    -> Result<CompiledInstructions, ResolveError>;

/// A bundle on disk: `<bundle_root>/<plan hash hex>/`.
#[derive(Debug, Clone)]
pub struct Bundle {
    pub plan: ResolutionPlan,
    pub manifest: BundleManifest,
    pub lock_file: LockFile,
    /// The exact bytes of `ordin.lock` (canonical YAML).
    pub lock_file_bytes: Vec<u8>,
    pub dir: PathBuf,
}
impl Bundle {
    /// `<dir>/artefacts/<content hash hex>/`.
    pub fn part_dir(&self, part: &PartRef) -> PathBuf;
    pub fn read_part_files(&self, part: &PartRef) -> Result<PartFiles, ResolveError>;
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Conflict {
    /// `<Kind>/<name>`: the slot two answers compete for.
    pub slot: String,
    pub candidates: Vec<ConflictCandidate>,
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ConflictCandidate { pub part: PartRef, pub requested_by: Vec<PartRef> }

/// Every pack and overlay the agent allows (fixed packs, packs a workspace match could add,
/// every pack and overlay **Change** offers) — the space the Checker's conflict check covers.
#[derive(Debug, Clone, Default)]
pub struct PackSpace { pub packs: Vec<SourcedPart>, pub overlays: Vec<SourcedPart> }

#[derive(Debug, Clone, Default)]
pub struct PruneReport { pub removed: u32, pub kept: u32, pub bytes_freed: u64 }

pub struct ResolverAndCompiler { /* private: OnceLock<Arc<dyn PartSource>>, ResolverConfig */ }

impl ResolverAndCompiler {
    /// Built without a source (R18); every call that reads parts fails with
    /// `RES_SOURCE_NOT_BOUND` until `install_part_source` has run.
    pub fn new(config: ResolverConfig) -> Result<Self, ResolveError>;
    /// Bind the source once, after the Catalogue Service is built. A second call fails with
    /// `RES_SOURCE_ALREADY_BOUND` and leaves the first source in place.
    pub fn install_part_source(&self, source: Arc<dyn PartSource>) -> Result<(), ResolveError>;
    /// Resolve only: the plan, with `tool_policy` filled.
    pub async fn resolve(&self, request: &ResolveRequest) -> Result<ResolutionPlan, ResolveError>;
    /// Compile a plan into a bundle on disk (reuses an existing bundle with the same plan hash).
    pub async fn compile(&self, plan: &ResolutionPlan, staged: &[StagedPart]) -> Result<Bundle, ResolveError>;
    /// `resolve` then `compile`.
    pub async fn build(&self, request: &ResolveRequest) -> Result<Bundle, ResolveError>;
    /// Read-only: the plan a lock file names, each part checked against the source; writes nothing.
    pub async fn resolve_lock(&self, lock: &LockFile, scope: &SourceScope) -> Result<ResolutionPlan, ResolveError>;
    /// `resolve_lock` then `compile`: rebuilds the bundle directory on disk (Installers' repair, SA-INST-01).
    pub async fn rebuild_from_lock(&self, lock: &LockFile, scope: &SourceScope) -> Result<Bundle, ResolveError>;
    /// The packs and overlays the root allows, for the Checker.
    pub async fn pack_space(&self, request: &ResolveRequest) -> Result<PackSpace, ResolveError>;
    /// Conflicts over every pair of parts that can be installed together (ADR-088).
    pub async fn conflicts(&self, request: &ResolveRequest, space: &PackSpace) -> Result<Vec<Conflict>, ResolveError>;
    /// An existing bundle by plan hash (`sha256:<hex>`).
    pub fn open_bundle(&self, plan_hash: &str) -> Result<Bundle, ResolveError>;
    /// Remove every bundle directory whose plan hash is not in `keep`.
    pub fn prune(&self, keep: &BTreeSet<String>) -> Result<PruneReport, ResolveError>;
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ResolveError {
    #[error("RES_ROOT_NOT_FOUND")] RootNotFound { id: String },
    #[error("RES_NOT_INSTALLABLE")] NotInstallable { kind: PartKind },
    #[error("RES_ROOT_NOT_APPROVED")] RootNotApproved { id: String, status: PartStatus },
    #[error("RES_DRAFT_CONSTITUENT")] DraftConstituent { part: String },
    #[error("RES_REFERENCE_UNRESOLVED")] ReferenceUnresolved { from: String, id: String, relation: String },
    #[error("RES_SOURCE_AMBIGUOUS")] SourceAmbiguous { id: String, repositories: Vec<String> },
    #[error("RES_DEPENDENCY_CYCLE")] DependencyCycle { path: Vec<String> },
    #[error("RES_CLOSURE_TOO_LARGE")] ClosureTooLarge { parts: u32 },
    #[error("RES_CONFLICT")] Conflict { conflicts: Vec<Conflict> },
    #[error("RES_PART_WITHDRAWN")] PartWithdrawn { part: String, replacement: Option<String> },
    #[error("RES_REQUIRED_PACK_UNSETTLED")] RequiredPackUnsettled { rule_index: u32, class: Option<String> },
    #[error("RES_PACK_NOT_ALLOWED")] PackNotAllowed { id: String },
    #[error("RES_REQUIRED_OVERLAY_MISSING")] RequiredOverlayMissing { axis: PartKind },
    #[error("RES_OVERLAY_NOT_ALLOWED")] OverlayNotAllowed { id: String },
    #[error("RES_OVERLAY_CARDINALITY")] OverlayCardinality { axis: PartKind },
    #[error("RES_OVERLAY_NOT_APPLICABLE")] OverlayNotApplicable { overlay: String },
    #[error("RES_MEMORY_CARDINALITY")] MemoryCardinality { parts: Vec<String> },
    #[error("RES_LOOP_MISSING")] LoopMissing,
    #[error("RES_LOOP_MULTIPLE")] LoopMultiple { parts: Vec<String> },
    #[error("RES_LOOP_NOT_PERMITTED")] LoopNotPermitted { loop_id: String },
    #[error("RES_LOOP_MODE_ESCAPE")] LoopModeEscape { loop_id: String, mode: String },
    #[error("RES_TOOL_POLICY_INVALID")] ToolPolicyInvalid { binding: String, reason: &'static str },
    #[error("RES_MANIFEST_INVALID")] ManifestInvalid { part: String, code: String },
    #[error("RES_DIGEST_MISMATCH")] DigestMismatch { part: String },
    #[error("RES_LOCK_PART_UNAVAILABLE")] LockPartUnavailable { part: String },
    #[error("RES_LOCK_MISMATCH")] LockMismatch { expected: String, found: String },
    #[error("RES_BUNDLE_NOT_FOUND")] BundleNotFound { plan_hash: String },
    #[error("RES_BUNDLE_CORRUPT")] BundleCorrupt { plan_hash: String },
    #[error("RES_SOURCE_UNAVAILABLE")] SourceUnavailable { retryable: bool },
    #[error("RES_STORE_WRITE_FAILED")] StoreWriteFailed { retryable: bool },
    #[error("RES_SOURCE_NOT_BOUND")] SourceNotBound,
    #[error("RES_SOURCE_ALREADY_BOUND")] SourceAlreadyBound,
}

impl ResolveError {
    /// RES_SOURCE_UNAVAILABLE (when the source says so) and RES_STORE_WRITE_FAILED are retryable.
    pub fn retryable(&self) -> bool;
}
```

### Who calls each public item

| Item | Called by |
|---|---|
| `new`, `install_part_source` | Ordin Desktop and Ordin Server programs (start-up: `new` before the Checker; `install_part_source(catalogue)` right after the Catalogue Service is built) |
| `resolve` | Package Catalogue (preview, add, change, update, withdrawal re-plan), Checker (agent rebuilds, library item) |
| `build` | Package Catalogue (add, change, update, trial with `source = Some(draft source)`) |
| `compile` | this crate's `build` and `rebuild_from_lock`; no other caller |
| `resolve_lock` | Checker (`check_installed`, **Check it**) |
| `rebuild_from_lock` | Installers (repair when the bundle is gone or corrupt) |
| `open_bundle`, `Bundle::read_part_files`, `Bundle::part_dir` | Installers (install, update, repair); Ordin Desktop and Ordin Server programs (`open_bundle` only: the `lock` argument of the Checker's `checker.check_installed`, their SA-CLOSE-06); the Package Inspector reads the same bundle directories through the layout in *Data Model* |
| `pack_space`, `conflicts` | Checker (draft conflicts, agent rebuilds, withdrawal) |
| `prune` | Package Catalogue, with the set of live plan hashes, after each install, update and removal |
| `compile_instructions`, `model` | Release Builder (P-3 Compile) |
| `compile_instructions` | Checker (`check_parts`, its step 39) |

### Example

Request: `root_id = "urn:ordin:default:workspaces:cspa-431"`, `target_harness = HarnessKind::ClaudeCode`, `workspace_languages = ["go"]`, `interface_language = "en-GB"`, no answers, no changes; the bundled cspa-431 carries `fixedPracticePacks: [{ask: {class: testing}}]` (ADR-089 *Consequences*).

`build` returns a `Bundle` at `<data root>/bundles/4d1c…0c1d/` whose plan has:

```json
{
  "selections": {
    "practicePacks": [{"id": "urn:ordin:default:practice-packs:stack:go", "chosenBy": "workspace-code"}],
    "overlays": [{"id": "urn:ordin:default:overlays:language:en-GB", "chosenBy": "interface-language"}],
    "pendingChoices": [{"ruleIndex": 1, "atLeast": 1, "class": "testing",
      "choices": ["urn:ordin:default:practice-packs:testing:bdd",
                  "urn:ordin:default:practice-packs:testing:test-pyramid",
                  "urn:ordin:default:practice-packs:testing:test-trophy",
                  "urn:ordin:default:practice-packs:testing:testing-honeycomb"]}]
  },
  "warnings": []
}
```

When the person later answers "Test pyramid", the caller repeats `build` with `pack_answers = [PackAnswer { rule_index: 1, pack_ids: ["urn:ordin:default:practice-packs:testing:test-pyramid"] }]`; the new plan has an empty `pendingChoices` and a different plan hash.

The same request with `workspace_languages = []` and no fixed stack pack fails with `RES_REQUIRED_PACK_UNSETTLED { rule_index: 0, class: Some("stack") }`.

## Internal Logic

`install_part_source(source)` and source selection (R18):

0. `install_part_source` stores the source in a `OnceLock`; when one is already stored → `RES_SOURCE_ALREADY_BOUND` (permanent; the first source stays). Every operation below that reads parts uses `request.source` when it is `Some`, else the bound source; with neither → `RES_SOURCE_NOT_BOUND` (permanent), logged `resolver.source_not_bound` at `error` because it means the program's start-up order is wrong. `rebuild_from_lock`, `resolve_lock` and `open_bundle` with re-verification use the bound source only.

`resolve(request)`:

1. **Root.** Look up the root: a staged part with kind WorkspaceProfile and `metadata.id = root_id` wins; otherwise `source.resolve(scope, WorkspaceProfile, root_id, root_version)`. `NotFound` → `RES_ROOT_NOT_FOUND`; `Ambiguous` → `RES_SOURCE_AMBIGUOUS`; `PartSourceError::Unavailable` → `RES_SOURCE_UNAVAILABLE` (retryable as the source says); `PartSourceError::Corrupt { part_identifier }` → `RES_DIGEST_MISMATCH { part: part_identifier }`; the same mapping applies to every `PartSource` call below. A root of any other kind is refused with `RES_NOT_INSTALLABLE` (ordin installs only WorkspaceProfiles; Journey, CrewProfile and Memory are never roots, `resolver.go` `nonInstallableRootKinds`). Root status: `approved` passes; `draft` passes only when `allow_draft_root`; otherwise `RES_ROOT_NOT_APPROVED` (a deprecated part is refused as an agent of its own).
2. **Withdrawn list.** `source.withdrawn(scope)` → entries; `WithdrawnListUnreadable` → `RES_SOURCE_UNAVAILABLE { retryable: false }` (a list that cannot be read fails closed, R-78). `withdrawn_snapshot` = `prefixed_digest(SHA-256(canonical_json(entries sorted by id, each as {kind, identifier, version, contentHash})))`.
3. **Ingredients.** `source.ingredients(scope)` gives every visible PracticePack and overlay; staged ones shadow by (kind, id).
4. **Practice packs** (ADR-089). Read `required_practice_packs`, `selectable_practice_packs`, `fixed_practice_packs` of the root (a reader error → `RES_MANIFEST_INVALID`). Then:
   1. Candidates of rule *r*: when `r.ids` is non-empty, those identifiers; otherwise the selectable packs whose `metadata.class == r.class`, or, when the root lists no selectable pack, every ingredient pack of that class. A candidate identifier that resolves to nothing is skipped and logged (`resolver.pack_candidate_missing`).
   2. A pack *matches the workspace* when one of its `metadata.audience.languages` equals (ASCII case-insensitive) one of `workspace_languages`.
   3. For each rule in authored order: (a) a `PackAnswer` for the rule's index — every identifier must be a candidate (else `RES_PACK_NOT_ALLOWED`) — gives the selection, `chosen_by = person-answer`; (b) else `pack_changes` restricted to the rule's candidates, when non-empty, `chosen_by = person-change`; (c) else candidates matching the workspace, sorted by identifier, the first `at_least` of them, `chosen_by = workspace-code`; (d) when (c) gives fewer than `at_least`, add fixed `Pack` entries that are candidates, in authored order, until `at_least` is reached, `chosen_by = maker-fixed`; (e) still short: when a fixed `Ask` entry has the rule's class (or the same `ids` set), record a `PendingPackChoice { rule_index, at_least, class, choices: candidates sorted }` and continue; otherwise fail with `RES_REQUIRED_PACK_UNSETTLED { rule_index, class }`.
   4. Optional packs (selectable, not a candidate of any rule): included when in `pack_changes` (`person-change`), else when matching the workspace (`workspace-code`), else when a fixed `Pack` names it (`maker-fixed`). An identifier in `pack_changes` that is neither a candidate nor selectable → `RES_PACK_NOT_ALLOWED`.
   5. `practice_packs` = the union, de-duplicated, sorted by identifier, each resolved to a `SourcedPart` (latest non-draft version).
5. **Overlays** (ADR-090 §5). For each axis in `overlay_order()` except HandoffOverlay (bound from outside through `appliesTo`, not applied in the first release), BrandOverlay and JurisdictionOverlay (read, not offered): read `overlay_offer(axis)`. Selection: an `overlay_changes` identifier in `available` (`person-change`; an identifier of that axis not in `available` → `RES_OVERLAY_NOT_ALLOWED`); else, for LanguageOverlay, the first available overlay whose slug's last segment equals `interface_language` ignoring ASCII case, then one whose last segment equals its primary language subtag (`en-GB` → `en`), `interface-language`; else `default` when set (`agent-default`); else none. More than one overlay selected on an exactly-one axis → `RES_OVERLAY_CARDINALITY`. Required overlays (`required_overlays`): an axis with a non-empty `anyOf` and no selected overlay whose slug matches one entry's slug → `RES_REQUIRED_OVERLAY_MISSING`. Applicability (port of `validateOverlayApplicability`): an overlay with `appliesTo` pins applies only when one pin's identifier slug equals the root's slug and the root's version satisfies the pin's range (exact, or caret: same major and ≥); otherwise `RES_OVERLAY_NOT_APPLICABLE`.
6. **Seeds and order.** `seeds = [root] ++ practice packs (identifier order) ++ overlays (overlay_order tier, then identifier)` (port of `orderedOverlays`).
7. **Closure walk.** For each seed, depth-first over `manifest.references()`: the reference kind is `relation_kind(relation)`; a known kind uses `source.resolve(scope, kind, id, None)`, an unknown one `resolve_any_kind`; staged parts shadow. `NotFound` → `RES_REFERENCE_UNRESOLVED { from, id, relation }`; `Ambiguous` → `RES_SOURCE_AMBIGUOUS`. A node met again on the current path → `RES_DEPENDENCY_CYCLE { path }`; depth > 64 → the same error. Record every (seed, resolved part) contribution keyed by slot `"<Kind>/<name>"` (port of `accumulateClosure`). More than 1,000 distinct parts → `RES_CLOSURE_TOO_LARGE`.
8. **Replacements and withdrawal** (ADR-090 §6). A resolved part is withdrawn when `part.withdrawn` or an entry matches its kind and identifier and either has no version (every version withdrawn) or matches its version and digest. For each withdrawn part: a `Replacement` naming it substitutes the replacement (resolved through `source.resolve`) and steps 7–8 repeat for its closure; else `RES_PART_WITHDRAWN { part, replacement: entry.replacement_identifier }`.
9. **Conflicts** (port of `findConflicts`): a slot holding two or more distinct identities (kind, name, version, digest) → `RES_CONFLICT { conflicts }`, sorted by slot; candidates sorted by identity, each with its requesting seeds sorted. No pin resolves a conflict.
10. **Closure gates** (port of `validateClosure` and the control-loop gates): more than one Memory part → `RES_MEMORY_CARDINALITY`; more than one ControlLoop → `RES_LOOP_MULTIPLE`; `required_control_loop` with `required: true` and no loop → `RES_LOOP_MISSING`; a loop outside a non-empty `ids` → `RES_LOOP_NOT_PERMITTED`; a loop mode (by slug) absent from the root's `allowed_operating_modes` slugs → `RES_LOOP_MODE_ESCAPE`.
11. **Lifecycle gate** (port of `gateStatus`): each non-root part — `approved` passes; `deprecated` adds the warning `"constituent <id> is deprecated; do not add new uses"` and logs `resolver.constituent_deprecated`; `draft` → `RES_DRAFT_CONSTITUENT` unless the part is staged.
12. **Part references.** For each resolved part build `PartRef { kind, id, name: slug, version, content_hash: prefixed_digest(part.content_digest), repository: repository_label, commit, loading: metadata.loading parsed or None }`; staged parts use their computed digest and the all-zero commit. `resolved` sorted by (kind, name, version); `overlay_order` and `practice_packs` in seed order.
13. **Tool policy** (port of `ComposeToolPolicy`, `crucible/internal/registry/compiler/toolpolicy.go:52-375`, widened by ADR-090 item 1): `forbidden_flags` = sorted, de-duplicated union of every resolved ToolBinding's bindings' `forbiddenFlags` and every pack's `toolchain.forbiddenFlags`; `coverage_floor` = the greatest of all `coverageFloor` values compared as decimals; `prohibited_verbs` = sorted, de-duplicated root `toolPermissions.workspaceOverrides.prohibited`; `requires_human_approval` = sorted, de-duplicated union over resolved Capabilities; `mcp_servers` = every binding with `type: mcp`, requiring non-empty `name` and `serverType` and `transport` ∈ {`stdio`, `http`, `sse`} (else `RES_TOOL_POLICY_INVALID { binding, reason }`), de-duplicated by name (first by ToolBinding identifier order), sorted by name, `declared_by` = the ToolBinding identifier.
14. **Plan hash.** Build the plan (`target_harness` = the `snake_case` serialisation of `request.target_harness`) with `plan_hash = ""` and `tool_policy = None`; `plan_hash = "sha256:" + hex(SHA-256(canonical_json(serde_json::to_value(plan))))`; then set `tool_policy = Some(policy)` (crucible excludes the policy from the hash, `resolver.go:1415-1441`). The same request against the same source state gives the same plan hash.
15. Log `resolver.resolved` (fields below). Return the plan.

`compile(plan, staged)`:

16. **Reuse.** When `<bundle_root>/<hex of plan_hash>/bundle.manifest.json` exists and parses and its `plan_hash` equals the plan's, return it through `open_bundle` (step 21). Otherwise continue.
17. **Stage.** Create `<bundle_root>/.tmp-<UUIDv7>/` (`RES_STORE_WRITE_FAILED { retryable: true }` on an I/O error). For each `PartRef` in `plan.resolved` (and each overlay and pack, which are also in `resolved`): files = the staged part's files, or `source.files(sourced part)`; recompute `content_digest(files)` and compare with the reference's hash (mismatch → `RES_DIGEST_MISMATCH { part }`, logged at `error`, staging directory removed); write each file to `artefacts/<hex>/<relative name>` creating directories, refusing a relative name with `..` or a leading `/` (`RES_DIGEST_MISMATCH` cannot arise there because the digest covers names; such a name is `RES_MANIFEST_INVALID { code: "AM_PATH_INVALID" }`).
18. **Lock file.** `LockFile { lockfile_schema: LOCK_FILE_SCHEMA, plan_hash, target_harness, composition: plan.root, artefacts: plan.resolved, overlays_applied: plan.overlay_order, selections: plan.selections }`; bytes = `to_canonical_yaml()`; write `ordin.lock`.
19. **Bundle manifest.** `BundleManifest { bundle_schema: BUNDLE_SCHEMA, plan_hash, composition: root, artefacts: resolved with loading = part's override else kind default, overlays_applied, resolved_tool_policy: plan.tool_policy, lockfile_sha256: hex(SHA-256(lock bytes)), compiler_version }`; write `bundle.manifest.json` as `canonical_json`. No time field, so the same plan gives byte-identical bundle files.
20. **Publish.** `fsync` each file, rename the staging directory to `<hex of plan_hash>`; when the target already exists (a concurrent compile of the same plan won), remove the staging directory and use the existing one. Log `resolver.compiled`.
21. **`open_bundle(plan_hash)`**: read the manifest and lock file; missing → `RES_BUNDLE_NOT_FOUND`; the manifest's `lockfile_sha256` not equal to the lock bytes' digest, or an artefact directory whose recomputed digest differs → `RES_BUNDLE_CORRUPT`. The plan is reconstructed from the manifest and lock file (`warnings` empty).

`resolve_lock(lock, scope)` and `rebuild_from_lock(lock, scope)`:

22. `resolve_lock`: for each entry of `lock.artefacts`: `source.resolve(scope, kind, id, Some(version))`; `NotFound`, a withdrawn part, or a digest different from the entry's → `RES_LOCK_PART_UNAVAILABLE { part }`. Build the plan from the lock's fields and recompute the plan hash with step 14; not equal to `lock.plan_hash` → `RES_LOCK_MISMATCH`. Recompute the tool policy (step 13). Return the plan; nothing is written under `bundle_root`.
22a. `rebuild_from_lock`: `resolve_lock`, then `compile` (steps 16–20), which writes the bundle directory.

`compile_instructions(source, request)` (Release Builder, ADR-082 replacing `apm compile -t agents`):

22b. Run steps 1–14 against `source` (ignoring `request.source` and any bound source) — every resolution error is returned unchanged. No step of `compile` runs and nothing is written.
22c. `agents_md`: for `plan.root` first, then each other `PartRef` of `plan.resolved` in its order, whose loading (the part's `metadata.loading`, else `kind_info(kind).default_loading`) is `AlwaysOn`: `source.files(part)` (re-checking `content_digest` as step 17; mismatch → `RES_DIGEST_MISMATCH`); for each file in name order where `markdown_body(name, bytes)` returns `Some(text)`, append `text` with surrounding blank lines trimmed. Join the pieces with one blank line, LF line endings, one trailing newline. A part with no Markdown file (an always-on YAML part such as OutputContract or Memory) adds nothing (*chosen here*: those parts carry no instruction text).
22d. `lock` = the `LockFile` of step 18 built from the plan. Log `resolver.instructions_compiled` (`plan_hash`, `parts`, `bytes`).

`pack_space(request)` and `conflicts(request, space)` (ADR-088, ADR-089 for the Checker):

23. `pack_space`: every candidate of every required rule, every selectable pack, every fixed pack, and, per applied overlay axis, every `available` overlay — each resolved; unresolvable ones are skipped and logged.
24. `conflicts`: seeds = root plus every part of the space; walk as step 7 and collect contributions; run step 9's slot test, then drop a conflict whose competing candidates are contributed only by overlays of one exactly-one axis, or only by candidates of one rule whose `at_least` is 1 — those never install together. Return the remaining conflicts (empty = pass). Source errors as in step 1.

`prune(keep)`:

25. List `<bundle_root>`; remove every directory whose name is a 64-hex plan hash not in `keep` (compared after stripping `sha256:`) and every `.tmp-*` directory older than 1 hour; return counts. An I/O error on one directory is logged and the next is tried; the report counts only what was removed.

## Data Model

No table: the crate has no Journal dependency (2B-3). "Owns: bundles and lock files" (02:454) is met on disk:

```text
<application data root>/bundles/
  <plan hash hex>/                    one bundle, immutable once renamed into place
    bundle.manifest.json             canonical JSON (BundleManifest)
    ordin.lock                       canonical YAML (LockFile)
    artefacts/<content hash hex>/    the part's files, as committed
  .tmp-<UUIDv7>/                     staging; removed on success, failure or by prune
```

Who references a bundle: the install record (Installers) and the agent instance (Package Catalogue) keep its plan hash; the Package Catalogue passes the set of live plan hashes to `prune` after each install, update and removal (*assumed here*; interaction recorded). Rollback: a bundle directory is never modified after the rename; removing one is safe once no install record names it.

## Error Table

All errors are values of an in-process API; callers map them to their own surface. The person never sees a code (07).

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Root identifier not in scope | `Err(permanent)` | `RES_ROOT_NOT_FOUND` | `ResolveError::RootNotFound { id }` |
| Root is not a WorkspaceProfile | `Err(permanent)` | `RES_NOT_INSTALLABLE` | `ResolveError::NotInstallable { kind }` |
| Root is draft (without `allow_draft_root`) or deprecated | `Err(permanent)` | `RES_ROOT_NOT_APPROVED` | `ResolveError::RootNotApproved { id, status }` |
| A non-staged constituent is draft | `Err(permanent)` | `RES_DRAFT_CONSTITUENT` | `ResolveError::DraftConstituent { part }` |
| A reference resolves to nothing | `Err(permanent)` | `RES_REFERENCE_UNRESOLVED` | `ResolveError::ReferenceUnresolved { from, id, relation }` |
| Same identifier in two repositories at one resolution step | `Err(permanent)` | `RES_SOURCE_AMBIGUOUS` | `ResolveError::SourceAmbiguous { id, repositories }` |
| Reference cycle, or depth over 64 | `Err(permanent)` | `RES_DEPENDENCY_CYCLE` | `ResolveError::DependencyCycle { path }` |
| Closure over 1,000 parts | `Err(permanent)` | `RES_CLOSURE_TOO_LARGE` | `ResolveError::ClosureTooLarge { parts }` |
| Two distinct parts fill one slot | `Err(permanent)` | `RES_CONFLICT` | `ResolveError::Conflict { conflicts }` |
| A resolved part is withdrawn and no replacement was given | `Err(permanent)` | `RES_PART_WITHDRAWN` | `ResolveError::PartWithdrawn { part, replacement }` |
| A required pack rule cannot be satisfied and is not left to ask | `Err(permanent)` | `RES_REQUIRED_PACK_UNSETTLED` | `ResolveError::RequiredPackUnsettled { rule_index, class }` |
| An answered or changed pack is not allowed by the agent | `Err(permanent)` | `RES_PACK_NOT_ALLOWED` | `ResolveError::PackNotAllowed { id }` |
| A required overlay axis is unsatisfied | `Err(permanent)` | `RES_REQUIRED_OVERLAY_MISSING` | `ResolveError::RequiredOverlayMissing { axis }` |
| A changed overlay is not in the axis's `available` list | `Err(permanent)` | `RES_OVERLAY_NOT_ALLOWED` | `ResolveError::OverlayNotAllowed { id }` |
| Two overlays on an exactly-one axis | `Err(permanent)` | `RES_OVERLAY_CARDINALITY` | `ResolveError::OverlayCardinality { axis }` |
| An overlay's `appliesTo` excludes this agent | `Err(permanent)` | `RES_OVERLAY_NOT_APPLICABLE` | `ResolveError::OverlayNotApplicable { overlay }` |
| More than one memory contract | `Err(permanent)` | `RES_MEMORY_CARDINALITY` | `ResolveError::MemoryCardinality { parts }` |
| Required control loop absent | `Err(permanent)` | `RES_LOOP_MISSING` | `ResolveError::LoopMissing` |
| More than one control loop | `Err(permanent)` | `RES_LOOP_MULTIPLE` | `ResolveError::LoopMultiple { parts }` |
| Loop outside the root's allow-list | `Err(permanent)` | `RES_LOOP_NOT_PERMITTED` | `ResolveError::LoopNotPermitted { loop_id }` |
| Loop names a mode the root does not allow | `Err(permanent)` | `RES_LOOP_MODE_ESCAPE` | `ResolveError::LoopModeEscape { loop_id, mode }` |
| An MCP binding lacks name, server type or a valid transport | `Err(permanent)` | `RES_TOOL_POLICY_INVALID` | `ResolveError::ToolPolicyInvalid { binding, reason }` |
| A manifest field the resolver reads has the wrong shape | `Err(permanent)` | `RES_MANIFEST_INVALID` | `ResolveError::ManifestInvalid { part, code }` |
| A part's files do not hash to its recorded digest | `Err(permanent)` | `RES_DIGEST_MISMATCH` | `ResolveError::DigestMismatch { part }` |
| A lock-file entry is gone, withdrawn or changed in the source | `Err(permanent)` | `RES_LOCK_PART_UNAVAILABLE` | `ResolveError::LockPartUnavailable { part }` |
| Recomputed plan hash differs from the lock file's | `Err(permanent)` | `RES_LOCK_MISMATCH` | `ResolveError::LockMismatch { expected, found }` |
| No bundle directory for a plan hash | `Err(permanent)` | `RES_BUNDLE_NOT_FOUND` | `ResolveError::BundleNotFound { plan_hash }` |
| Bundle manifest, lock file or part files fail their digests | `Err(permanent)` | `RES_BUNDLE_CORRUPT` | `ResolveError::BundleCorrupt { plan_hash }` |
| `PartSource` unavailable, or the withdrawn list unreadable | `Err(retryable)` / `Err(permanent)` | `RES_SOURCE_UNAVAILABLE` | `ResolveError::SourceUnavailable { retryable }` |
| Disk write or rename fails | `Err(retryable)` | `RES_STORE_WRITE_FAILED` | `ResolveError::StoreWriteFailed { retryable: true }` |
| A part-reading call before `install_part_source` and without `request.source` | `Err(permanent)` | `RES_SOURCE_NOT_BOUND` | `ResolveError::SourceNotBound` |
| `install_part_source` called a second time | `Err(permanent)` | `RES_SOURCE_ALREADY_BOUND` | `ResolveError::SourceAlreadyBound` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Resolver and Compiler

  Scenario: Happy path — parity with frozen crucible
    Given every WorkspaceProfile in "crucible/models/@default/workspaces/" from the bundled catalogue
    And the same pack and overlay choices given to frozen crucible
    When build is called for each
    Then each bundle holds the same parts, versions and file sets as frozen crucible's bundle
    And each tool policy's forbidden flags, prohibited verbs and MCP servers equal frozen crucible's

  Scenario: Happy path — packs from the workspace's code
    Given cspa-431 with requiredPracticePacks "at least 1 stack" and "at least 1 testing", and a fixed ask entry for class "testing"
    And the workspace's linked projects are written in Go
    When build is called
    Then the stack pack "urn:ordin:default:practice-packs:stack:go" is selected with chosenBy "workspace-code"
    And one pending choice is recorded for the testing rule with four choices

  Scenario: Edge — the person answers the agent's question
    Given the pending testing choice above
    When build is called with a pack answer "urn:ordin:default:practice-packs:testing:test-pyramid" for rule 1
    Then the plan has no pending choices
    And the plan hash differs from the first build's

  Scenario: Edge — language overlay follows the interface language
    Given cspa-431 offers language overlays en-GB, en-US, fr and es with default en-GB
    When build is called with interface language "fr"
    Then the selected language overlay is "urn:ordin:default:overlays:language:fr" with chosenBy "interface-language"

  Scenario: Edge — determinism
    Given the same request and the same source state
    When build is called twice
    Then both plan hashes are equal and the second call reuses the first bundle directory

  Scenario: Edge — a deprecated constituent warns
    Given a skill used by the agent has status "deprecated"
    When resolve is called
    Then the plan holds the warning "constituent urn:… is deprecated; do not add new uses"

  Scenario: Edge — withdrawn part with a replacement
    Given a rule of the agent is withdrawn with replacement "urn:ordin:acme:rules:tone-v2"
    When resolve is called with that replacement
    Then the plan holds "urn:ordin:acme:rules:tone-v2" and not the withdrawn rule

  Scenario: Error — no stack pack can be chosen
    Given a workspace with no linked code and an agent whose maker fixed no stack pack and left nothing to ask
    When resolve is called
    Then it fails with RES_REQUIRED_PACK_UNSETTLED naming class "stack"

  Scenario: Error — conflicting parts
    Given two packs the agent uses bring different versions of the skill "quality-codegen"
    When resolve is called
    Then it fails with RES_CONFLICT for slot "Skill/quality-codegen" naming both packs

  Scenario: Error — withdrawn part without replacement
    Given a skill of the agent is on its repository's withdrawn list with no replacement
    When resolve is called
    Then it fails with RES_PART_WITHDRAWN

  Scenario: Error — tampered source files
    Given the source returns files for a part that do not hash to the part's digest
    When compile is called
    Then it fails with RES_DIGEST_MISMATCH and no bundle directory is created

  Scenario: Error — lock file names a part the source no longer has
    Given a lock file whose skill version is gone from the clone
    When rebuild_from_lock is called
    Then it fails with RES_LOCK_PART_UNAVAILABLE

  Scenario: Error — the clone is locked
    Given the PartSource returns Unavailable with retryable true
    When resolve is called
    Then it fails with RES_SOURCE_UNAVAILABLE and retryable() is true

  Scenario: Error — a call before the source is bound
    Given a ResolverAndCompiler built with new and no install_part_source call
    When resolve is called with a request whose source is None
    Then it fails with RES_SOURCE_NOT_BOUND and retryable() is false

  Scenario: Edge — the source is bound only once
    Given install_part_source has bound the Catalogue Service
    When install_part_source is called again with another source
    Then it fails with RES_SOURCE_ALREADY_BOUND and resolve still reads the first source

  Scenario: Happy path — a trial reads the person's draft through a per-request source
    Given the bound source does not hold the draft agent "contract-clause-checker"
    And a draft source that holds it
    When build is called with allow_draft_root true and source Some(draft source)
    Then the bundle's root is the draft agent

  Scenario: Happy path — Check it reads the lock without writing
    Given an installed agent's lock file whose parts are all present in the source
    When resolve_lock is called
    Then it returns the plan with the lock file's plan hash
    And no directory under the bundle root is created or changed

  Scenario: Happy path — always-on instructions for the Release Builder
    Given a package source whose root has a system role and two rules loaded always-on and one skill loaded on demand
    When compile_instructions is called
    Then agents_md holds the system role text followed by the two rules' texts, separated by blank lines
    And agents_md holds no text of the skill
    And no bundle directory is written

  Scenario: Checker support — conflicts across the pack space ignore exclusive choices
    Given an agent offering two testing packs under a rule with at_least 1, each bringing a different version of one skill
    When conflicts is called with the agent's pack space
    Then no conflict is reported for that skill
```

## Performance, Security, Observability

- **Performance targets:** `resolve` for a 200-part closure from a warm local index ≤ 250 ms p95 (A-45 renders the same closure in 300 ms); `compile` of 200 parts totalling 5 MB ≤ 600 ms p95 on SSD; `rebuild_from_lock` ≤ `resolve` + `compile`; memory ≤ 64 MB for a 1,000-part closure. `conflicts` over a 30-pack space ≤ 1 s p95.
- **Security:** part files are data; nothing is executed. Relative file names are checked before any write under `bundle_root`; writes happen only beneath `bundle_root`, staged then renamed, so a half-written bundle is never visible. Digests are recomputed on every compile and every `open_bundle`. The tool policy only narrows (the Rules Engine refuses a widening policy, 2C `RULE_POLICY_WIDENS`). `env_ref` names a credential variable, never a value (SA-CRED-01).
- **Observability:** `tracing` events (SA-OBS-01); never part content, file paths outside the bundle root, or credential names' values:
  - `resolver.resolved` (info): `root_id`, `root_version`, `plan_hash`, `parts`, `packs`, `overlays`, `pending_choices`, `warnings`, `duration_ms`.
  - `resolver.resolve_failed` (warn): `root_id`, `error_code`, `retryable`.
  - `resolver.compiled` (info): `plan_hash`, `parts`, `bytes`, `reused`, `duration_ms`.
  - `resolver.digest_mismatch` (error): `part_id`, `repository_label`.
  - `resolver.constituent_deprecated` (info), `resolver.pack_candidate_missing` (warn): `part_id`.
  - `resolver.pruned` (info): `removed`, `kept`, `bytes_freed`.
  - `resolver.source_not_bound` (error): `operation`.
  - `resolver.instructions_compiled` (info): `plan_hash`, `parts`, `bytes`.
  Agent profile only (Release Builder use): `resolver.build` with fields `plan_hash`, `parts` (no identifiers of people, no content; SA-XC-05) is a local `tracing` span only, never exported (Tracing.md T-9: the exported spans are the five of the Telemetry allow list).

## Gaps

None.
