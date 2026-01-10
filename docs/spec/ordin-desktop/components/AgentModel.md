# SPEC: Agent Model

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-102) · amended January 2026 (identifier prefix `urn:ordin:`, ADR-082 as amended; approved by the user January 2026) · amended January 2026 (Phase 1 build corrections: `LoadingMode` is ordered, so `PartRef`'s derived `Ord` and `required_overlays`' `BTreeMap` compile with 2C's ordered `PartKind`; approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-agent-model` · **Build phase:** 1 · **Programs:** D P A
> **Dependencies:** Shared types

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

The Agent Model is the ordin format in Rust: crucible's component model (`apiVersion: ai.enterprise/v1`, 25 kinds, overlays, vocabularies) ported unchanged, plus the one optional WorkspaceProfile field of ordin's own, `fixedPracticePacks` (02:435–438, ADR-082, ADR-089, SA-NAME-07). It reads and writes part manifests, groups the files of a repository tree into parts, renders the file of a part from its fields, places a part's file where crucible places it, and computes the canonical form whose SHA-256 is a part's content digest (02:435, ADR-091 item 3). It also defines the plan, bundle-manifest, lock-file and resolved-tool-policy types that the Resolver and Compiler produces, and it owns the `PartSource` trait through which the Resolver and Compiler and the Checker read parts that the Catalogue Service holds (2B-1 item 4, ADR-100 item 1, 2B-2 D8). It is the only form the Checker, the Resolver and Compiler, the Installers and the Authoring Service see (ADR-087 *Consequences*).

## Approach

A pure library with no I/O, no datastore and no model call (ADR-085): every function is deterministic over its inputs, ported from `crucible/pkg/types` and `crucible/pkg/canonical` at commit `e270904` (ADR-088 port map) and tested table-driven against frozen crucible's fixtures (`crucible/models/@default/`, 516 files). A manifest is held as crucible holds it — typed `metadata`, and a generic `body` map read through typed accessors — because crucible itself keeps kind-specific keys generic (`crucible/pkg/types/artefact.go:171-208`, `ArtefactManifest.Body`), so the 25 kinds need no 25 structs. The canonical form is an **uncompressed** deterministic POSIX tar (crucible's normalisation of `CanonicalTar`, `crucible/internal/registry/artefactbuild/artefactbuild.go:166-198`) rather than crucible's gzip of it. Rejected: (1) the gzip form — deflate output is not byte-stable across implementations, so a Rust port cannot promise crucible's byte digests; (2) typed per-kind structs — they would drop unknown keys and break the ordin format's lossless round trip (ADR-087).

## Shared Context

### Types copied from Phase 2C (`ordin-types`), used unchanged

```rust
// phase-2c-shared-types-domain.md §Conventions, §Part, §WithdrawnListEntry, §VocabularyTerm
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PartId(pub Uuid);                       // UUIDv7
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentRepositoryId(pub Uuid);            // UUIDv7
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VocabularyTermId(pub Uuid);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WithdrawnListEntryId(pub Uuid);

/// SHA-256 as 64 lowercase hexadecimal characters (HASH_NOT_SHA256_HEX otherwise).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Sha256Hex(pub String);

/// RFC 3339, UTC, millisecond precision, `Z` suffix (TS_NOT_RFC3339_MS_UTC otherwise).
/// Serialised by 2C's own `Serialize` impl (`to_rfc3339_opts(SecondsFormat::Millis, true)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>);

/// A base-10 decimal number written as text; no binary float enters ordin's types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DecimalString(pub String);

/// The 25 kinds, spelt as crucible declares them (`crucible/pkg/types/kinds.go:17-86`).
/// The one enum exempt from the snake_case rule: no `rename_all`.
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
pub struct Part {
    pub id: PartId,
    pub agent_repository_id: AgentRepositoryId,
    pub kind: PartKind,
    /// `metadata.id`.
    pub identifier: String,
    pub name: String,
    pub version: String,
    pub status: PartStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
    pub owner: String,
    pub file_path: String,
    pub commit: String,
    pub content_digest: Sha256Hex,
    pub vocabulary_terms_cited: Vec<VocabularyTermId>,
    pub uses: Vec<PartId>,
    pub withdrawn: bool,
}

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

/// Serialises kebab-case to match crucible's axis values (`pack-class`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VocabularyAxis {
    PackClass, Category, AudienceRole, RuleSubvariant,
    ArtefactStatus, MemoryContentClass, LoopStrategy, RuntimeCapability,
}
```

Shared rules carried: `metadata.status` absent reads as `approved` (04:313; `crucible/pkg/types/status.go`); `withdrawn` is a flag, never a status (2C §Part).

### Facts of the ordin format (from crucible at `e270904`)

| Fact | Value | Source |
|---|---|---|
| API version | `ai.enterprise/v1` | `crucible/internal/registry/authoringcheck/stageone.go:35` |
| Identifier grammar | `urn:ordin:<publisher>:<kind-dir>[:<category>]*:<slug>` (amended January 2026: ordin's prefix in place of the reference format's; the segments are unchanged); legacy `@<publisher>/<kind-dir>/…` also recognised | `crucible/pkg/types/kinds.go:279-289`, `artefact.go:226` |
| Identifier pattern (references) | `^(@[a-z][a-z0-9-]*(/[A-Za-z0-9][A-Za-z0-9-]*)+\|urn:ordin:[a-z][a-z0-9-]*(:[A-Za-z0-9][A-Za-z0-9._-]*)+)$` | `crucible/pkg/types/artefact.go:226` |
| Name (slug) pattern | `^[A-Za-z][A-Za-z0-9-]{0,127}$` | `crucible/pkg/types/artefact.go` `nameRegex` |
| Version pattern | `^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$` | `crucible/pkg/types/artefact.go` `versionRegex` |
| Status vocabulary | `draft`, `approved`, `deprecated`; absent = `approved` | `crucible/pkg/types/status.go` |
| Loading modes | `always-on`, `on-demand`, `event`, `mode-switch`, `registered-invocation`, `structural`, `none` | `crucible/pkg/types/loadingmode.go` |
| Forge-authorable kinds | Rule, Skill, PracticePack, Guideline, Memory, ControlLoop (+ Journey in ordin, ADR-092) | `crucible/pkg/types/kinds.go:146-149`; ADR-091 item 1 |
| Composition roots | WorkspaceProfile (CrewProfile is not ported, ADR-081) | `crucible/pkg/types/kinds.go:100-103` |
| Never installable as a root | Journey, CrewProfile, Memory | `crucible/internal/registry/resolver/resolver.go` `nonInstallableRootKinds` |
| Read and checked, never offered | Journey (read as a lifecycle, ADR-092), AgentIdentity, CrewProfile, BrandOverlay, JurisdictionOverlay | ADR-090 items 8–9; 11 §5.27 table |

### Configuration

None. The Agent Model reads no environment variable, organisation profile key or setting (2D).

### Libraries

`serde` 1, `serde_json` 1, `sha2` 0.10, `thiserror` 1, `async-trait` 0.1 (each already a dependency of the repository's `memory` crate, `memory/Cargo.toml:31-44`), `regex` 1 (05 — Pattern matching), `uuid` 1.28.0 (SA-ID-01), `chrono` (2C `Timestamp`). YAML is **parsed** only through Shared types' restricted node reader `ordin_types::yaml::read_restricted_node(text, max_bytes) -> Result<YamlNode, YamlError>` (over `saphyr`; `YamlNode { Map(Vec<(String, YamlNode)>), Seq(Vec<YamlNode>), Scalar { text: String, quoted: bool }, Null }`, scalars keeping their exact source text; it refuses anchors, aliases, custom tags and over-size input; `YamlError::code()` is `YAML_FEATURE_REFUSED`, `YAML_MALFORMED` or `YAML_TOO_LARGE`; decision P4-1, HLD 05 *YAML parsing*). This crate does not depend on `saphyr` directly. YAML is **emitted** by this crate's own fixed code (step 9), never by a library.

### Assumptions from the Phase 3 reconciliation

- **SA-RECON-01** — The Checker and the Resolver and Compiler hold their `PartSource` in a slot bound once after the Catalogue Service is built (ruling R18); the Agent Model keeps the trait object-safe and defines no binding mechanism of its own.
- **SA-RECON-02** — `ResolutionPlan.target_harness` and `LockFile.target_harness` stay `String` holding the serde spelling of 2C `HarnessKind` (`claude_code`, `copilot_cli`; ruling R7), because both are hashed and written to `ordin.lock`; callers convert with `serde`.
- **SA-RECON-03** — The `DraftModel` trait and its types are owned here; the Authoring Service holds the drafts and calls the model (ruling R18). There is no `ModelError`: failures are `AmError`.
- **SA-RECON-04** — The provider's names win (ruling R6): there is no `PartReference`, `PartPin`, `fetch_pinned` or accessor method on `ModelPart`; consumers use the trait and fields as written here.
- **SA-RECON-05** — `withdrawn.yaml` and `ordin.lock` keep camelCase keys (decision D7); every other type this crate serialises follows 2C's snake_case convention except `PartKind` and `VocabularyAxis`.

### Assumptions from the closing round

- **SA-CLOSE-01** (*chosen here*; closes the command-arguments gap) — A Command part's file may carry the front-matter key `argument-hint` (Claude Code's command front-matter key, a plain string such as `<suppliers> <quarter>`); `Manifest::argument_hint()` reads it so the Package Inspector can tell the Ordin Host what each command expects. Absent → `None`.
- **SA-CLOSE-02** (*chosen here*) — A ToolBinding binding may carry `argumentPaths`, a list of JSON Pointers (RFC 6901) to the arguments a rule may test; two wildcard tokens are allowed: a whole token `[]` (every item of a list) and a final token `*` (every place below). This is ordin's addition to crucible's binding shape; crucible ignores the key. Read into `ToolBindingSpec.argument_paths`.
- **SA-CLOSE-03** (*chosen here*) — `OutputContract::read(manifest)` is the one typed reader of an OutputContract's `sections`; the Checker uses it for the dossier reader and the handover contract check.
- **SA-CLOSE-04** (*chosen here*, closing round K22) — `StagedPart` and `SourceOverride` are owned here (moved from the Resolver and Compiler, which re-exports them): the Authoring Service builds both for the Checker and has no edge to `ordin-resolver`.

## Public Interface

All items are in crate `ordin_agent_model`. Names not found in 2C or the HLD are *chosen here*.

### Who calls what

Every public item below, with the components that call it (Phase 3 reconciliation, R6: the consumer uses these names and signatures exactly).

| Public item | Called by |
|---|---|
| Kind table (`kind_info`, `all_kinds`, `overlay_order`, `canonical_kind`) | Checker, Resolver and Compiler, Format Plug-ins, Package Inspector, Catalogue Service, Authoring Service |
| Identifiers (`Urn`, `is_part_identifier`, `slug_from_id`, `strip_kind_dir_prefix`, `mint_identifier`) | Checker, Format Plug-ins, Installers, Authoring Service |
| `parse_manifest`, `read_tree`, `Manifest` and its typed readers, `relation_kind` | Format Plug-ins (the ordin format's read), Catalogue Service (through Format Plug-ins), Checker, Resolver and Compiler, Installers, Package Inspector |
| `content_digest`, `canonical_archive`, `prefixed_digest`, `parse_prefixed_digest` | Resolver and Compiler, Checker, Format Plug-ins, Catalogue Service, Installers |
| `part_file_path`, `render_part`, `render_manifest_yaml`, `markdown_body` | Format Plug-ins (the ordin format's write), Authoring Service (through `DraftModel`), Installers, Package Inspector, Resolver and Compiler |
| `VocabularySet` | Checker, Resolver and Compiler, Format Plug-ins, Package Inspector, Catalogue Service |
| `PartRef`, `ResolutionPlan`, `Selections`, `BundleManifest`, `LockFile`, `canonical_json`, `compare_versions` | Resolver and Compiler (produces), Installers, Package Inspector, Checker, Package Catalogue and Release Builder (both through `ordin_resolver::model`), Format Plug-ins (`PartRef`, returned by `TreeSource::root`) |
| `ResolvedToolPolicy`, `McpServerDecl` | Resolver and Compiler (produces), Installers (`InstallRecord.tool_policy`), Package Catalogue, Rules Engine (converted to its `AgentToolPolicy` by the caller named in RulesEngine.md) |
| `PartSource` trait, `SourceScope`, `RepositoryRef`, `SourcedPart`, `Resolution`, `PartSourceError` | implemented by the Catalogue Service (its index, and `draft_source` for a draft worktree) and by the Format Plug-ins' `TreeSource` (one package read); called by the Resolver and Compiler and the Checker (both late-bound, R18, plus a per-request override), the Authoring Service (`repository_parts` of the bundled catalogue for `starting_vocabulary`), the Release Builder (a `TreeSource` returned by the server program's `PackageReader`). The Handover Service reaches parts only through the Checker |
| `BUNDLED_REPOSITORY_ID` | Catalogue Service (rows of the bundled catalogue), Resolver and Compiler, Checker |
| `parse_withdrawn_list`, `render_withdrawn_list`, `WithdrawnListFile` | Catalogue Service (reads `withdrawn.yaml` at ingest), Authoring Service (writes a withdrawal draft), Checker (reads the proposed list) |
| `DraftModel` trait, `FieldModel`, `GeneratedPart`, `ReusedPart` | Authoring Service (holds drafts and calls the model, R18) |
| `Manifest::argument_hint` | Package Inspector (each task's `argument_hint`) |
| `ToolBindingSpec.argument_paths` | Package Inspector (the Rules Engine's argument places, converted) |
| `OutputContract`, `ContractSection` | Checker (`read_dossier`, `check_handover`) |
| `StagedPart`, `SourceOverride` | Authoring Service (builds them), Checker and Resolver and Compiler (consume them; the Resolver re-exports both) |
| `testkit::ContractFixture`, `testkit::assert_part_source_contract` | Catalogue Service's test suite (every `PartSource` implementation passes it) |

### Kinds and the kind table

```rust
pub use ordin_types::{PartKind, PartStatus, Part, PartId, AgentRepositoryId, Sha256Hex,
    Timestamp, VocabularyAxis, WithdrawnListEntry, DecimalString};

pub const API_VERSION: &str = "ai.enterprise/v1";
/// Names of the agent folder's lock file and the bundle manifest (chosen here).
pub const LOCK_FILE_NAME: &str = "ordin.lock";
pub const LOCK_FILE_SCHEMA: &str = "ordin.lock/v1";
pub const BUNDLE_MANIFEST_NAME: &str = "bundle.manifest.json";
pub const BUNDLE_SCHEMA: &str = "ordin.bundle/v1";
/// `agent_repository_id` of every row of the bundled catalogue (chosen here; a fixed UUIDv7-shaped
/// value so the Catalogue Service, the Resolver and the Checker agree on it without a lookup).
pub const BUNDLED_REPOSITORY_ID: AgentRepositoryId =
    AgentRepositoryId(uuid::uuid!("01900000-0000-7000-8000-000000000001"));
/// `listSchema` of `withdrawn.yaml` (08 *Withdrawn list*).
pub const WITHDRAWN_LIST_SCHEMA: &str = "ordin.withdrawn/v1";
pub const WITHDRAWN_LIST_FILE_NAME: &str = "withdrawn.yaml";

/// crucible's loading modes; serialised as crucible spells them. Ordered by declaration order, so
/// `PartRef` can derive `Ord` (amended January 2026, Phase 1 build corrections).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LoadingMode { AlwaysOn, OnDemand, Event, ModeSwitch, RegisteredInvocation, Structural, None }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindFamily { Constituent, Root, Ingredient, Overlay, ControlLoop, MetadataOnly }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileConvention {
    /// `<base>/<slug>.md`, YAML front matter then Markdown body.
    MarkdownFile,
    /// `<base>/<slug>/SKILL.md`.
    SkillDirectory,
    /// `<base>/<slug>.yaml`, whole manifest as YAML, no body.
    YamlFile,
    /// `<base>/<slug>/<fixed file name>` (practice-pack.yaml, workspace-profile.yaml).
    YamlInDirectory { file_name: &'static str },
    /// `<base>/control-loop-<slug>.md`.
    ControlLoopFile,
}

#[derive(Debug, Clone, Copy)]
pub struct KindInfo {
    pub kind: PartKind,
    /// Authored `kind:` spellings that map to this kind (rule sub-variants come from the vocabulary).
    pub authored_spellings: &'static [&'static str],
    /// Directory under `models/@<publisher>/`; None for overlays (resolved through `overlays/<axis>/`).
    pub kind_dir: Option<&'static str>,
    pub family: KindFamily,
    pub default_loading: LoadingMode,
    /// At most one selection per agent (overlay axes and the control-loop family).
    pub exactly_one: bool,
    pub forge_authorable: bool,
    /// Where a part draft writes this kind; None = not written as a part.
    pub file_convention: Option<FileConvention>,
    /// Offered to people (false for Journey-as-agent, AgentIdentity, CrewProfile, BrandOverlay, JurisdictionOverlay).
    pub offered: bool,
    /// Fluent message id of the kind in plain words (11 §5.27 table), e.g. "part-kind-ability".
    pub plain_message_id: &'static str,
}

pub fn kind_info(kind: PartKind) -> &'static KindInfo;
/// The 25 kinds in crucible's `AllKinds` order (non-overlay, then overlay registry, then control loop).
pub fn all_kinds() -> &'static [PartKind; 25];
/// Overlay axes in composition order: Brand, Jurisdiction, SourceWorkflow, Style, Language, Handoff.
pub fn overlay_order() -> &'static [PartKind; 6];
/// Maps an authored `kind:` value; rule sub-variants (`rule-layer`, `rule-orchestrator`) resolve
/// through the `rule-subvariant` vocabulary to `Rule`.
pub fn canonical_kind(authored: &str, vocabulary: &VocabularySet) -> Option<PartKind>;
```

The table (*values ported from `crucible/pkg/types/kinds.go`, `overlaykinds.go`, `loadingmode.go`, `artefact.go:476-510`, and `crucible/internal/registry/forge/render.go:137-176`; message ids chosen here*):

| Kind | Authored spellings | Kind dir | Default loading | Exactly one | File convention | Plain message id |
|---|---|---|---|---|---|---|
| SystemRole | `system-role` | — (beside its workspace) | always-on | no | MarkdownFile | `part-kind-role` |
| Rule | `rule` (+ sub-variants) | `rules` | always-on | no | MarkdownFile | `part-kind-rule` |
| OperatingMode | `operating-mode` | `rules` | mode-switch | no | — | `part-kind-working-mode` |
| Skill | `skill` | `skills` | on-demand | no | SkillDirectory | `part-kind-ability` |
| Command | `command` | `commands` | registered-invocation | no | MarkdownFile | `part-kind-task` |
| ContextResource | `context-resource` | `context-resources` | on-demand | no | MarkdownFile | `part-kind-reference-material` |
| ToolBinding | `tool-binding` | `tool-bindings` | structural | no | — | `part-kind-tool` |
| AgentIdentity | `agent-identity` | `agent-identities` | structural | no | — | `part-kind-not-offered` |
| Capability | `capability` | `capabilities` | structural | no | YamlFile | `part-kind-group-of-abilities` |
| WorkspaceProfile | `workspace-profile` | `workspaces` | structural | no | YamlInDirectory `workspace-profile.yaml` | `part-kind-agent` |
| CrewProfile | `crew-profile` | `crews` | structural | no | — | `part-kind-not-offered` |
| Journey | `journey` | `journeys` | none | no | YamlFile | `part-kind-lifecycle` |
| PracticePack | `practice-pack` | `practice-packs` | structural | no | YamlInDirectory `practice-pack.yaml` | `part-kind-working-practice` |
| BrandOverlay | — | `overlays/brand` | always-on | no | — | `part-kind-not-offered` |
| JurisdictionOverlay | — | `overlays/jurisdiction` | always-on | no | — | `part-kind-not-offered` |
| SourceWorkflowOverlay | `source-workflow-overlay` | `overlays/source-workflow` | always-on | yes | MarkdownFile | `part-kind-code-workflow` |
| StyleOverlay | `style-overlay` | `overlays/style` | always-on | yes | MarkdownFile | `part-kind-writing-style` |
| LanguageOverlay | `language-overlay` | `overlays/language` | always-on | yes | MarkdownFile | `part-kind-language` |
| HandoffOverlay | `handoff-overlay` | `overlays/handoff` | always-on | yes | MarkdownFile | `part-kind-hand-over` |
| Evaluation | `evaluation`, `ci-gate` | `evals` | none | no | YamlFile | `part-kind-rehearsal-checks` |
| OutputContract | `output-contract` | `output-contracts` | always-on | no | YamlFile | `part-kind-result-must-hold` |
| Vocabulary | `vocabulary` | `vocabulary` | none | no | YamlFile | `part-kind-words` |
| Guideline | `guideline` | `guidelines` | none | no | YamlFile | `part-kind-writing-guide` |
| Memory | `memory` | `memories` | always-on | no | YamlFile | `part-kind-memory-rules` |
| ControlLoop | `control-loop` | `rules` (`control-loops/` category) | always-on | yes | ControlLoopFile | `part-kind-way-of-working` |

A `—` in *File convention* means crucible's Forge has no convention for that kind; ordin writes such a kind only through an agent draft that renders it with `YamlFile` (*assumed here*: crucible's `render` writes every non-Markdown kind as the whole manifest in YAML, `forge/render.go:22-26`). A `—` in *Authored spellings* means no manifest in frozen crucible's catalogue authors the kind (`grep '^kind:'` over `crucible/models/@default`); a manifest naming it is refused with `AM_KIND_UNKNOWN`.

### Identifiers

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Urn { pub publisher: String, pub kind_dir: String, pub categories: Vec<String>, pub slug: String }
impl Urn {
    /// `urn:ordin:` prefix, at least three non-empty segments (`crucible/pkg/types/urn.go`).
    pub fn parse(id: &str) -> Option<Urn>;
    pub fn to_id(&self) -> String;
}
/// True when `s` matches the identifier pattern above (either spelling).
pub fn is_part_identifier(s: &str) -> bool;
/// Port of `StripKindDirPrefix` (`crucible/pkg/types/kinds.go:316-357`).
pub fn strip_kind_dir_prefix(id: &str) -> String;
/// Port of `SlugFromID`: strip the prefix, collapse each run of `[^A-Za-z0-9-]` to `-`, trim `-`.
pub fn slug_from_id(id: &str) -> String;
/// Mints `urn:ordin:<publisher>:<kind-dir>[:<category>]:<slug>` from a plain name:
/// lowercase, each run of characters outside `[a-z0-9]` becomes `-`, trimmed, at most 64 characters.
pub fn mint_identifier(kind: PartKind, publisher: &str, category: Option<&str>, name: &str)
    -> Result<String, AmError>;
```

### Values, manifests and parts

```rust
/// A YAML value as the ordin format holds it. Non-integer numbers keep their source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YamlValue {
    Null, Bool(bool), Int(i64), Decimal(String), Str(String),
    Seq(Vec<YamlValue>), Map(std::collections::BTreeMap<String, YamlValue>),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Audience { pub domains: Vec<String>, pub roles: Vec<String>, pub languages: Vec<String> }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UseCase { pub title: String, pub summary: String }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    pub id: String,
    pub name: String,
    pub version: String,
    pub owner: String,
    pub loading: Option<String>,
    pub description: String,
    pub category: String,
    pub class: String,
    pub audience: Audience,
    pub use_cases: Vec<UseCase>,
    /// None = absent in the file (reads as Approved through `status_or_default`).
    pub status: Option<String>,
    pub supersedes: Option<String>,
    /// Metadata keys crucible does not type; kept so a lossless write can re-emit them.
    pub extra: std::collections::BTreeMap<String, YamlValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub api_version: String,
    /// The authored `kind:` value, unchanged (`workspace-profile`, `rule-layer`).
    pub authored_kind: String,
    pub metadata: Metadata,
    /// Every top-level key other than apiVersion, kind and metadata.
    pub body: std::collections::BTreeMap<String, YamlValue>,
}

impl Manifest {
    pub fn kind(&self, vocabulary: &VocabularySet) -> Option<PartKind>;
    /// AM_API_VERSION_UNKNOWN unless `api_version == API_VERSION`.
    pub fn require_api_version(&self) -> Result<(), AmError>;
    pub fn status_or_default(&self) -> Result<PartStatus, AmError>;
    pub fn slug(&self) -> String;                         // slug_from_id(metadata.id)
    /// Port of `References()`: every identifier under a relation key, sorted by key, deepest
    /// relation key wins (`crucible/pkg/types/artefact.go:426-470`).
    pub fn references(&self) -> Vec<ManifestRef>;
    // Typed readers over `body` (step 7). Each returns AM_FIELD_SHAPE_INVALID when the key
    // is present with the wrong shape, and the empty value when the key is absent.
    pub fn required_practice_packs(&self) -> Result<Vec<RequiredPackRule>, AmError>;
    pub fn selectable_practice_packs(&self) -> Result<Vec<SelectablePack>, AmError>;
    pub fn fixed_practice_packs(&self) -> Result<Vec<FixedPackEntry>, AmError>;
    pub fn overlay_offer(&self, axis: PartKind) -> Result<Option<OverlayOffer>, AmError>;
    pub fn required_overlays(&self) -> Result<std::collections::BTreeMap<PartKind, Vec<String>>, AmError>;
    pub fn required_control_loop(&self) -> Result<Option<RequiredLoopRule>, AmError>;
    pub fn allowed_operating_modes(&self) -> Vec<String>;
    pub fn default_mode(&self) -> Option<String>;
    pub fn control_loop(&self) -> Result<Option<ControlLoopSpec>, AmError>;
    pub fn prohibited_verbs(&self) -> Result<Vec<String>, AmError>;
    pub fn tool_bindings(&self) -> Result<Vec<ToolBindingSpec>, AmError>;
    pub fn requires_human_approval(&self) -> Result<Vec<String>, AmError>;
    pub fn pack_toolchain(&self) -> Result<Option<PackToolchain>, AmError>;
    pub fn memory_contract(&self) -> Result<Option<MemoryContract>, AmError>;
    /// Command parts: the front-matter `argument-hint` string (SA-CLOSE-01).
    pub fn argument_hint(&self) -> Result<Option<String>, AmError>;
    pub fn publish_gate(&self) -> Option<String>;
    pub fn applies_to(&self) -> Result<Vec<AppliesTo>, AmError>;
    pub fn vocabulary(&self) -> Result<Option<VocabularyBody>, AmError>;
    pub fn journey(&self) -> Result<Option<JourneyBody>, AmError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestRef { pub id: String, pub relation: String }

/// The files of one part, keyed by '/'-separated path relative to the part's directory.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PartFiles(pub std::collections::BTreeMap<String, Vec<u8>>);

/// One part as the ordin format holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelPart {
    pub kind: PartKind,
    pub manifest: Manifest,
    /// Repository-relative path of the manifest file (`models/@default/skills/x/SKILL.md`).
    pub manifest_path: String,
    pub files: PartFiles,
    pub content_digest: Sha256Hex,
}

/// Result of reading a whole tree.
#[derive(Debug, Clone, Default)]
pub struct TreeReading {
    pub parts: Vec<ModelPart>,
    /// A manifest that failed to parse: (repository path, error). The part is not offered.
    pub refused: Vec<(String, AmError)>,
    /// Files in a directory holding two manifests and no closer one: owned by no part.
    pub unowned_files: Vec<String>,
}

/// The kind a relation key points at (step 6), None for keys that are not relations.
pub fn relation_kind(relation: &str) -> Option<PartKind>;
pub fn parse_manifest(path: &str, bytes: &[u8]) -> Result<Option<Manifest>, AmError>;
pub fn read_tree(files: &std::collections::BTreeMap<String, Vec<u8>>, vocabulary: &VocabularySet)
    -> TreeReading;
pub fn canonical_archive(files: &PartFiles) -> Vec<u8>;
pub fn content_digest(files: &PartFiles) -> Sha256Hex;
/// `sha256:<hex>`, the spelling used inside crucible-shaped files (lock file, withdrawn list).
pub fn prefixed_digest(d: &Sha256Hex) -> String;
pub fn parse_prefixed_digest(s: &str) -> Result<Sha256Hex, AmError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedFile { pub path: String, pub content: Vec<u8> }
/// Port of the Forge's file convention (`pathFor`, `crucible/internal/registry/forge/render.go:137-176`).
pub fn part_file_path(kind: PartKind, id: &str) -> Result<String, AmError>;
/// Port of `render`: front matter + body for Markdown conventions, whole manifest for YAML ones.
pub fn render_part(kind: PartKind, manifest: &Manifest, body: Option<&str>) -> Result<RenderedFile, AmError>;
pub fn render_manifest_yaml(manifest: &Manifest) -> String;
/// Body text of a Markdown part (after the closing `---`), or None for YAML conventions.
pub fn markdown_body(path: &str, bytes: &[u8]) -> Option<String>;
```

### Body shapes read by the typed readers

```rust
/// One conjunctive rule (`crucible/pkg/types/artefact.go:131-158`). Exactly one of class / ids.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RequiredPackRule { pub at_least: u32, pub class: Option<String>, pub ids: Vec<String> }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectablePack { pub id: String, pub description: Option<String> }

/// One entry of `fixedPracticePacks` (SA-NAME-07): a pack identifier, or a rule left for the
/// agent to ask (decision 64). Authored as a string, or as `{ask: {class: <c>}}` /
/// `{ask: {ids: [<id>, …]}}` (the ask form is chosen here, see HLD disagreement B3-D1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixedPackEntry { Pack(String), Ask { class: Option<String>, ids: Vec<String> } }

/// `governanceMetadata.<axis>Overlays` (`cspa-431/workspace-profile.yaml`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayOffer { pub available: Vec<String>, pub default: Option<String> }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequiredLoopRule { pub required: bool, pub ids: Vec<String> }

/// `crucible/pkg/types/controlloop.go:19-24`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ControlLoopSpec { pub strategy: String, pub trigger: String, pub requires: Vec<String>, pub modes: Vec<String> }

/// One entry of a ToolBinding's `bindings:` (`crucible/internal/registry/compiler/toolpolicy.go:120-132`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolBindingSpec {
    pub name: String,
    #[serde(rename = "type")] pub binding_type: String,
    pub permission: String,
    pub forbidden_flags: Vec<String>,
    pub coverage_floor: Option<DecimalString>,
    pub server_type: Option<String>,
    pub scope: Option<String>,
    pub env_ref: Option<String>,
    pub transport: Option<String>,
    /// `argumentPaths`: JSON Pointers to the arguments a rule may test (SA-CLOSE-02); empty when absent.
    #[serde(default)] pub argument_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackToolchain { pub forbidden_flags: Vec<String>, pub coverage_floor: Option<DecimalString> }

/// `crucible/pkg/types/memory.go:28-60` (retains, excludes, retention, store).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MemoryContract {
    pub retains: Vec<String>,
    pub excludes: Vec<String>,
    pub max_age_days: Option<u32>,
    pub unbounded: bool,
    /// `local` or `remote`.
    pub store_location: String,
    pub memory_service: Option<String>,
}

/// One section of an OutputContract (`crucible/pkg/types/outputcontract.go:14-34`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractSection {
    pub name: String,
    pub meaning: String,
    pub required: bool,
    /// `evidenceThreshold`; 0 when absent.
    pub evidence_threshold: u32,
    pub derived: bool,
}
/// The typed view of an OutputContract part's body (SA-CLOSE-03).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputContract { /* private: sections: Vec<ContractSection> */ }
impl OutputContract {
    /// Reads `sections` (step 7); absent → no sections. Does not check the manifest's kind.
    pub fn read(manifest: &Manifest) -> Result<OutputContract, AmError>;
    /// The sections in the contract's order.
    pub fn sections(&self) -> &[ContractSection];
}

/// One `appliesTo.workspaces` pin: an identifier and an exact (`1.0.0`) or caret (`^1.0.0`) range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliesTo { pub id: String, pub range: String }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocabularyBody { pub axis: VocabularyAxis, pub terms: Vec<VocabularyTermBody> }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocabularyTermBody { pub code: String, pub label: String, pub description: String }

/// 08 *Lifecycle part*: crucible's JourneySpec plus `stages`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JourneyBody {
    pub seams: Vec<String>,
    pub trigger: Option<String>,
    pub stages: Vec<JourneyStage>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JourneyStage {
    pub id: String, pub name: String, pub role: String,
    pub agent: Option<String>, pub produces: Option<String>,
    pub handover_place_type: String, pub handover_place_target: String,
    pub answer_within_days: Option<u32>,
    /// None = the next stage in the list; Some(empty) = the lifecycle ends here.
    pub next: Option<Vec<StageNextEntry>>,
    pub together: bool,
    pub sends_back_to: Vec<SendsBackEntry>,
    /// `slackChannel` of an ordin-format stage; None when the key is absent (crucible journeys never carry it). Amended January 2026 (ADR-102).
    pub slack_channel: Option<JourneySlackChannel>,
}
/// The Agent Model's own shape (it reads YAML only and does not validate Slack's identifier form;
/// the Catalogue Service and the Checker do, as `StageSlackChannel` in 2C). Amended January 2026 (ADR-102).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JourneySlackChannel { pub id: String, pub name: String }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageNextEntry { pub stage: String, pub when: Option<String> }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendsBackEntry { pub stage: String, pub when: String, pub max_rounds: u32 }
```

### Vocabulary set

```rust
/// The terms of every `vocabulary` part read in one repository (and the repositories it borrows
/// from), one per axis. Port of `crucible/pkg/types/vocabulary_set.go`.
#[derive(Debug, Clone, Default)]
pub struct VocabularySet { /* private: BTreeMap<VocabularyAxis, BTreeMap<code, VocabularyTermBody>> */ }
impl VocabularySet {
    /// Refuses a second vocabulary part for one axis (AM_VOCABULARY_AXIS_DUPLICATE).
    pub fn from_parts(parts: &[&ModelPart]) -> Result<VocabularySet, AmError>;
    pub fn has(&self, axis: VocabularyAxis, code: &str) -> bool;
    pub fn codes(&self, axis: VocabularyAxis) -> Vec<String>;
    pub fn label(&self, axis: VocabularyAxis, code: &str) -> Option<String>;
    /// `rule-subvariant` terms resolve to `Rule`.
    pub fn rule_subvariant(&self, authored: &str) -> Option<PartKind>;
}
```

### Plan, bundle and lock-file types (produced by the Resolver and Compiler)

```rust
/// A part as one plan, bundle or lock file names it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartRef {
    pub kind: PartKind,
    /// `metadata.id`.
    pub id: String,
    /// slug_from_id(id).
    pub name: String,
    pub version: String,
    /// `sha256:<hex>`.
    pub content_hash: String,
    /// `<host>/<owner>/<name>`, or `bundled` for the catalogue shipped with ordin.
    pub repository: String,
    /// 40 lowercase hex characters; for `bundled`, the ordin release's catalogue commit.
    pub commit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loading: Option<LoadingMode>,
}

/// Port of `crucible/pkg/types/plan.go` `ResolvedToolPolicy`, widened by ADR-090 item 1.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedToolPolicy {
    pub forbidden_flags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage_floor: Option<DecimalString>,
    pub prohibited_verbs: Vec<String>,
    /// ADR-090 item 1: `defaultToolPolicy.requiresHumanApproval` of the agent's capabilities.
    pub requires_human_approval: Vec<String>,
    pub mcp_servers: Vec<McpServerDecl>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerDecl {
    pub name: String,
    #[serde(rename = "type")] pub server_type: String,
    #[serde(skip_serializing_if = "Option::is_none")] pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub env_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub transport: Option<String>,
    /// Identifier of the ToolBinding part that declared it.
    pub declared_by: String,
}

/// The pack and overlay choices one plan was built with.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selections {
    pub practice_packs: Vec<SelectedPart>,
    pub overlays: Vec<SelectedPart>,
    /// Required pack rules the maker left for the agent to ask and nobody has answered yet
    /// (decision 64): the bundle is built without them until the person answers.
    pub pending_choices: Vec<PendingPackChoice>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingPackChoice {
    /// 0-based index of the rule in `requiredPracticePacks`.
    pub rule_index: u32,
    pub at_least: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    /// Identifiers of the packs the rule allows, sorted; the question's choices (ADR-089).
    pub choices: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedPart { pub id: String, pub chosen_by: ChosenBy }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChosenBy { WorkspaceCode, MakerFixed, PersonAnswer, PersonChange, AgentDefault, InterfaceLanguage }

/// Port of `ResolutionPlan`, without tenant, inline root and crew fields (not ported, ADR-081).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolutionPlan {
    /// `sha256:<hex>` of the canonical JSON of this plan with planHash = "" and toolPolicy = None.
    pub plan_hash: String,
    pub root: PartRef,
    /// The serde spelling of 2C `HarnessKind` (`claude_code`, `copilot_cli`), kept as text
    /// because the plan is hashed and written to files (SA-RECON-02).
    pub target_harness: String,
    pub overlay_order: Vec<PartRef>,
    pub practice_packs: Vec<PartRef>,
    /// Sorted by (kind, name, version).
    pub resolved: Vec<PartRef>,
    pub selections: Selections,
    /// One plain warning per deprecated constituent, in `resolved` order.
    pub warnings: Vec<String>,
    /// Digest (`sha256:<hex>`) of the withdrawn entries consulted, sorted by entry id.
    pub withdrawn_snapshot: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_policy: Option<ResolvedToolPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleArtefact { pub part: PartRef, pub loading: LoadingMode }

/// `bundle.manifest.json`. Port of `BundleManifestV2` without the signing and time fields
/// (ADR-084, R-71: no registry signature).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleManifest {
    pub bundle_schema: String,            // BUNDLE_SCHEMA
    pub plan_hash: String,
    pub composition: PartRef,
    pub artefacts: Vec<BundleArtefact>,   // sorted by (kind, name, version)
    pub overlays_applied: Vec<PartRef>,   // application order
    pub resolved_tool_policy: ResolvedToolPolicy,
    /// SHA-256 hex of the canonical lock-file bytes.
    pub lockfile_sha256: String,
    pub compiler_version: String,         // ordin's release version
}

/// `ordin.lock`. Port of crucible's `Lockfile` without signing fields; one entry per part with
/// repository, commit and digest (02:453, 04:42).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockFile {
    pub lockfile_schema: String,          // LOCK_FILE_SCHEMA
    pub plan_hash: String,
    pub target_harness: String,           // 2C `HarnessKind` spelling (SA-RECON-02)
    pub composition: PartRef,
    pub artefacts: Vec<PartRef>,          // sorted by (kind, name, version)
    pub overlays_applied: Vec<PartRef>,   // application order
    pub selections: Selections,
}
impl LockFile {
    /// Deterministic YAML (step 9): fixed key order, two-space indent, LF, one trailing newline.
    pub fn to_canonical_yaml(&self) -> String;
    pub fn from_yaml(bytes: &[u8]) -> Result<LockFile, AmError>;
}
/// Sorted-key, no-whitespace JSON: a re-export of `ordin_types::encoding::canonical_json`, the one
/// definition in the workspace (Logging R-3); same output as `crucible/pkg/canonical/json.go` for ASCII keys.
pub use ordin_types::encoding::canonical_json;
/// Semantic-version precedence (SemVer 2.0.0 §11), for "latest" choices.
pub fn compare_versions(a: &str, b: &str) -> Result<std::cmp::Ordering, AmError>;
```

### The `PartSource` trait (owned here, ADR-100; implemented by the Catalogue Service)

This trait is the only shape of a part source. There is no `PartReference`, `PartPin` or `fetch_pinned`: a pinned lookup is `resolve(scope, kind, id, Some(version))` followed by the caller's digest comparison against the lock file (the Resolver and Compiler's `rebuild_from_lock`). Callers adapt the catalogue's own rows to `SourcedPart`; `ModelPart` gains no accessor methods for them (the Catalogue Service reads `ModelPart`'s fields, `Metadata` and the `Manifest` readers).

**Late binding (R18).** The Checker and the Resolver and Compiler are built before the Catalogue Service, which implements this trait and itself depends on the Checker. Those two components therefore hold their `Arc<dyn PartSource>` in a slot set once, after the Catalogue Service is built; a call made before the slot is set fails with a permanent error of the holder's own type (`CHK_SOURCE_NOT_BOUND` in Checker.md, `RES_SOURCE_NOT_BOUND` in ResolverAndCompiler.md). The Agent Model defines no binding mechanism; the trait is object-safe so it can sit behind such a slot.

```rust
/// Where one agent's references resolve, in order (ADR-090 §2): `home`, then `profile`
/// in the order the organisation profile lists them, then the bundled catalogue.
/// A repository the person added appears only as `home` of its own agents, never in `profile`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceScope {
    pub home: RepositoryRef,
    pub profile: Vec<RepositoryRef>,
    pub include_bundled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RepositoryRef { Repository(AgentRepositoryId), Bundled }

/// One part as the source holds it at its followed commit.
#[derive(Debug, Clone)]
pub struct SourcedPart {
    /// The catalogue row (2C `Part`); for the bundled catalogue, `agent_repository_id` is
    /// `BUNDLED_REPOSITORY_ID`.
    pub part: Part,
    pub repository: RepositoryRef,
    /// `<host>/<owner>/<name>` or `bundled` — the value written into `PartRef.repository`.
    pub repository_label: String,
    /// Display name shown as "From: [display name]"; "ordin" for the bundled catalogue.
    pub repository_display_name: String,
    pub manifest: Manifest,
}

#[derive(Debug, Clone)]
pub enum Resolution {
    Found(SourcedPart),
    NotFound,
    /// Two repositories at the same step of the order carry the identifier (ADR-090 §2):
    /// refused for that part; the source reports it to Diagnostics.
    Ambiguous { repositories: Vec<String> },
}

#[async_trait::async_trait]
pub trait PartSource: Send + Sync {
    /// The part with `kind` and `metadata.id` = `id`, at `version` when given, otherwise the
    /// highest semantic version whose status is not draft on the followed commit, resolved
    /// through `scope` in order. Withdrawn parts are returned with `part.withdrawn = true`.
    /// Only parts the source offers are visible: a part its ingest refused (a failed check, or
    /// a construct the format's mapping report marks as not offered) is `NotFound`, never an
    /// error (reconciliation row 543). The Catalogue Service reads `scope.home` where it
    /// earlier read a `from` repository.
    async fn resolve(&self, scope: &SourceScope, kind: PartKind, id: &str, version: Option<&str>)
        -> Result<Resolution, PartSourceError>;

    /// The same lookup when the reference carries no kind (a bare identifier under a relation
    /// key the Agent Model cannot map): every kind is tried; two kinds matching is Ambiguous.
    async fn resolve_any_kind(&self, scope: &SourceScope, id: &str)
        -> Result<Resolution, PartSourceError>;

    /// The files of one part, exactly as committed. The caller recomputes `content_digest`
    /// and refuses a mismatch (AM_SOURCE_CORRUPT).
    async fn files(&self, part: &SourcedPart) -> Result<PartFiles, PartSourceError>;

    /// Every PracticePack and every overlay visible through `scope`, latest non-draft version of
    /// each identifier, for pack rules by class and overlay offers.
    async fn ingredients(&self, scope: &SourceScope) -> Result<Vec<SourcedPart>, PartSourceError>;

    /// Every withdrawn-list entry in force in the repositories of `scope` (entries removed by
    /// hand included, R-78).
    async fn withdrawn(&self, scope: &SourceScope) -> Result<Vec<WithdrawnListEntry>, PartSourceError>;

    /// Every part of one repository at its followed commit (the staged snapshot of crucible's
    /// `authoringcheck.Catalogue.StagedSnapshot`), for whole-repository checks.
    async fn repository_parts(&self, repository: &RepositoryRef)
        -> Result<Vec<SourcedPart>, PartSourceError>;

    /// The vocabulary set of `scope` (home repository's vocabulary parts, then borrowed ones).
    async fn vocabulary(&self, scope: &SourceScope) -> Result<VocabularySet, PartSourceError>;

    /// Agents (WorkspaceProfiles) whose forward closure includes the part (reverse closure,
    /// 02:441 *Used by*), across every repository the source reads. Vocabulary citations are
    /// not references and are not followed here; `citing_parts` answers them.
    async fn used_by(&self, part: &SourcedPart) -> Result<Vec<SourcedPart>, PartSourceError>;

    /// Every head part, in every repository the source reads, whose `vocabulary_terms_cited`
    /// includes the term `code` on `axis` of `repository`'s vocabulary (ADR-091 item 4).
    /// Sorted by (repository_label, kind, identifier). Called by: Checker (retiring a term).
    async fn citing_parts(&self, repository: &RepositoryRef, axis: VocabularyAxis, code: &str)
        -> Result<Vec<SourcedPart>, PartSourceError>;
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum PartSourceError {
    /// The clone or index cannot be read now (disk, lock). Retryable.
    #[error("AM_SOURCE_UNAVAILABLE")]
    Unavailable { retryable: bool },
    /// The repository's withdrawn list could not be read and no earlier copy exists (R-78).
    #[error("AM_SOURCE_WITHDRAWN_LIST_UNREADABLE")]
    WithdrawnListUnreadable { repository: String },
    /// The source holds a row whose files no longer exist at the recorded commit.
    #[error("AM_SOURCE_CORRUPT")]
    Corrupt { part_identifier: String },
}
```

### Staged parts and per-request sources (SA-CLOSE-04)

```rust
/// A part that is not (yet) in the source: a draft's new or changed part. It shadows a source
/// part with the same kind and identifier. Built by the Authoring Service; read by the Checker
/// and the Resolver and Compiler (which re-exports it as `ordin_resolver::StagedPart`).
#[derive(Debug, Clone)]
pub struct StagedPart {
    pub part: ModelPart,
    pub repository: RepositoryRef,
    /// `<host>/<owner>/<name>`.
    pub repository_label: String,
}

/// A source used for one request in place of the bound one: the Catalogue Service's
/// `draft_source(repository, worktree)` for a trial or a draft check (R18).
#[derive(Clone)]
pub struct SourceOverride(pub std::sync::Arc<dyn PartSource>);
impl std::fmt::Debug for SourceOverride {
    /// Prints `SourceOverride(..)`; the source has no printable state.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result;
}
```

### The withdrawn-list file (`withdrawn.yaml`, 08 *Withdrawn list*, ADR-091 item 3)

The Agent Model reads and writes the file's shape; value rules (`WDL_*`) belong to the callers (Catalogue Service at ingest, Checker on a withdrawal draft). Keys keep crucible's camelCase (D7, SA-JSON-01 exception).

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawnListFile {
    /// Always WITHDRAWN_LIST_SCHEMA when parsed successfully.
    pub list_schema: String,
    /// Oldest first, as in the file.
    pub entries: Vec<WithdrawnFileEntry>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawnFileEntry {
    pub part: WithdrawnFilePart,
    pub reason: String,
    /// RFC 3339 text as written; the caller validates it (`WDL_*`).
    pub withdrawn_at: String,
    pub replacement: Option<WithdrawnFileReplacement>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawnFilePart {
    pub kind: PartKind,
    pub id: String,
    pub version: Option<String>,
    /// 64 lowercase hex characters, written without the `sha256:` prefix (08).
    pub content_hash: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithdrawnFileReplacement { pub id: String, pub version: Option<String> }

/// Parses `withdrawn.yaml` with the manifest YAML rules of step 3 (anchors, aliases and custom
/// tags refused; 1 MiB limit). Called by: Catalogue Service, Checker.
pub fn parse_withdrawn_list(bytes: &[u8]) -> Result<WithdrawnListFile, AmError>;
/// Deterministic YAML through the step-9 emitter. Called by: Authoring Service (withdrawal draft).
pub fn render_withdrawn_list(list: &WithdrawnListFile) -> String;
```

### The draft model (R18: types owned here; drafts held by the Authoring Service)

Turns the fields a person or the agent maker writes into parts, and parts back into fields (02:435 "generates every file of a draft from its fields", ADR-083). The trait exists so the Authoring Service can hold `Arc<dyn DraftModel>` and substitute a test double; `FieldModel` is the one implementation. All methods are pure.

```rust
/// A part the Foundry proposed for an agent draft (ADR-088 step 3, ADR-090 §7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GeneratedPart {
    pub kind: PartKind,
    /// Plain name; the identifier is minted from it with `mint_identifier`.
    pub name: String,
    /// The kind's fields in the `fields_of` shape; `metadata.id` may be absent (it is minted).
    pub fields: serde_json::Value,
}

/// A part the agent draft reuses from the library, as the Authoring Service resolved it from
/// `AgentDraft.reused_parts` through the Catalogue Service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReusedPart { pub kind: PartKind, pub identifier: String }

pub trait DraftModel: Send + Sync {
    /// Every part of an agent draft: one WorkspaceProfile, one SystemRole, and one part per
    /// `generated` entry, with the WorkspaceProfile referencing the generated and reused parts.
    /// `publisher` is the target repository's publisher directory without `@`.
    fn agent_parts_from_fields(&self, draft: &AgentDraft, publisher: &str,
        reused: &[ReusedPart], generated: &[GeneratedPart]) -> Result<Vec<ModelPart>, AmError>;
    /// One part from its fields (the `fields_of` shape). `metadata.id` is required.
    fn part_from_fields(&self, kind: PartKind, fields: &serde_json::Value) -> Result<ModelPart, AmError>;
    /// The fields of one part, lossless: `part_from_fields(kind, &fields_of(p))` renders the
    /// same bytes as `p`'s manifest file.
    fn fields_of(&self, part: &ModelPart) -> serde_json::Value;
    /// JSON Schema (draft 2020-12) of the kind's fields, for the part-draft form.
    fn form(&self, kind: PartKind) -> serde_json::Value;
    /// A new part of `kind` (port of `crucible init`): version 1.0.0, status draft, empty body.
    fn scaffold(&self, kind: PartKind, name: &str, identifier: &str) -> Result<ModelPart, AmError>;
    /// The agent draft as a Markdown write-up with the `role-dossier` section headings.
    fn render_write_up(&self, draft: &AgentDraft) -> String;
}

/// The implementation of `DraftModel`. Stateless.
#[derive(Debug, Clone, Copy, Default)]
pub struct FieldModel;
impl DraftModel for FieldModel { /* steps 14–18 */ }
```

`AgentDraft` is 2C's (`phase-2c-shared-types-domain.md` §AgentDraft), re-exported from `ordin_types`. There is no `ModelError`; every failure is an `AmError`.

### Test kit (feature `testkit`)

```rust
pub mod testkit {
    /// An in-memory PartSource over fixed repositories, keeping every contract rule of step 12.
    pub struct InMemorySource { /* private */ }
    impl InMemorySource { pub fn new(fixture: &ContractFixture) -> InMemorySource; }
    /// The fixed repositories the contract test uses: a home repository, two profile
    /// repositories sharing one identifier, the bundled catalogue, a withdrawn part, a
    /// draft-only part, a vocabulary with one cited term, and one part the ingest refused.
    pub struct ContractFixture { pub repositories: Vec<(RepositoryRef, String, Vec<ModelPart>)>,
        pub withdrawn: Vec<WithdrawnListEntry>, pub refused: Vec<(RepositoryRef, String)> }
    impl ContractFixture { pub fn standard() -> ContractFixture; }
    /// Runs every contract assertion of step 12 against `source`, which the implementer has
    /// loaded with `ContractFixture::standard()`. Panics with the failing rule's name.
    /// Called by: Catalogue Service's test suite; Agent Model's own tests over InMemorySource.
    pub async fn assert_part_source_contract(source: &dyn PartSource, fixture: &ContractFixture);
}
```

### Errors

```rust
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AmError {
    #[error("AM_MANIFEST_MALFORMED")] ManifestMalformed { path: String, line: Option<u32> },
    #[error("AM_MANIFEST_FIELD_MISSING")] ManifestFieldMissing { path: String, fields: Vec<&'static str> },
    #[error("AM_API_VERSION_UNKNOWN")] ApiVersionUnknown { path: String, found: String },
    #[error("AM_KIND_UNKNOWN")] KindUnknown { path: String, authored: String },
    #[error("AM_YAML_FEATURE_REFUSED")] YamlFeatureRefused { path: String, feature: &'static str },
    #[error("AM_FILE_TOO_LARGE")] FileTooLarge { path: String, bytes: u64 },
    #[error("AM_PATH_INVALID")] PathInvalid { path: String },
    #[error("AM_ID_INVALID")] IdInvalid { id: String },
    #[error("AM_VERSION_INVALID")] VersionInvalid { version: String },
    #[error("AM_STATUS_UNKNOWN")] StatusUnknown { status: String },
    #[error("AM_FIELD_SHAPE_INVALID")] FieldShapeInvalid { key: String, expected: &'static str },
    #[error("AM_NO_FILE_CONVENTION")] NoFileConvention { kind: PartKind },
    #[error("AM_VOCABULARY_AXIS_DUPLICATE")] VocabularyAxisDuplicate { axis: VocabularyAxis },
    #[error("AM_DIGEST_INVALID")] DigestInvalid { value: String },
    #[error("AM_LOCK_FILE_INVALID")] LockFileInvalid { reason: &'static str },
    #[error("AM_WITHDRAWN_LIST_INVALID")] WithdrawnListInvalid { reason: &'static str, entry: Option<u32> },
    #[error("AM_DRAFT_FIELDS_INVALID")] DraftFieldsInvalid { field: String, expected: &'static str },
    #[error("AM_DRAFT_KIND_NOT_COMPOSABLE")] DraftKindNotComposable { kind: PartKind },
}
```

### Example

Input file `models/@default/skills/quality/codegen/SKILL.md`:

```text
---
apiVersion: ai.enterprise/v1
kind: skill
metadata:
  id: "urn:ordin:default:skills:quality:codegen"
  name: Code generation
  version: 1.4.0
  owner: engineering-enablement
  description: "Generates a component from its specification."
---
# Code generation
1. Read the component spec.
```

`parse_manifest(path, bytes)` returns `Ok(Some(Manifest { api_version: "ai.enterprise/v1", authored_kind: "skill", metadata: Metadata { id: "urn:ordin:default:skills:quality:codegen", name: "Code generation", version: "1.4.0", owner: "engineering-enablement", status: None, .. }, body: {} }))`. `slug_from_id` gives `quality-codegen`; `part_file_path(Skill, id)` gives `models/@default/skills/quality/codegen/SKILL.md`; `content_digest` over `PartFiles {"SKILL.md": <bytes>}` gives a 64-hex `Sha256Hex`, and `prefixed_digest` gives `sha256:<hex>` for the lock file.

A lock-file entry written by `LockFile::to_canonical_yaml`:

```yaml
lockfileSchema: ordin.lock/v1
planHash: sha256:4d1c9f0b7e2a5c3d1f6e8a9b0c7d2e4f5a6b8c9d0e1f2a3b4c5d6e7f8a9b0c1d
targetHarness: claude_code
composition:
  kind: WorkspaceProfile
  id: urn:ordin:default:workspaces:cspa-431
  name: cspa-431
  version: 1.0.0
  contentHash: sha256:9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b
  repository: bundled
  commit: 0123456789abcdef0123456789abcdef01234567
artefacts:
  - kind: Command
    id: urn:ordin:default:commands:lifecycle:apply
    name: lifecycle-apply
    version: 1.2.0
    contentHash: sha256:1f2e3d4c5b6a79881726354a5b6c7d8e9f0a1b2c3d4e5f60718293a4b5c6d7e8
    repository: github.com/acme/company-agents
    commit: 89abcdef0123456789abcdef0123456789abcdef
    loading: registered-invocation
overlaysApplied: []
selections:
  practicePacks:
    - id: urn:ordin:default:practice-packs:stack:go
      chosenBy: workspace-code
  overlays: []
  pendingChoices: []
```

## Internal Logic

1. **Kind table.** `kind_info` is one `static` array of 25 `KindInfo` rows in `all_kinds()` order, filled from the table in *Public Interface*. A unit test asserts: exactly 25 rows; every `PartKind` variant present once; the authored-spelling sets are disjoint; the Forge-authorable set equals {Rule, Skill, PracticePack, Guideline, Memory, ControlLoop, Journey}; the exactly-one set equals {SourceWorkflowOverlay, StyleOverlay, LanguageOverlay, HandoffOverlay, ControlLoop}. No error path.
2. **Canonical kind.** `canonical_kind(authored, vocab)` trims `authored`; looks it up in the authored-spelling map built from the table; on a miss, returns `vocab.rule_subvariant(authored)` (`crucible/internal/registry/authoringcheck/stageone.go:199-204`). No error: `None` means unknown.
3. **Manifest parse** (port of `ParseManifestStrict`, `artefactbuild.go:69-128`).
   1. Refuse `bytes.len() > 1,048,576` with `AM_FILE_TOO_LARGE` (*assumed here*: 1 MiB per file; A-45 renders parts up to 100 KB).
   2. By lower-cased extension of `path`: `.md`/`.markdown` — when the text does not start with `---`, return `Ok(None)`; take the text after the first `---` line up to the next line that is exactly `---`; if no closing line exists and the front matter holds a line starting `apiVersion:` or `kind:`, return `AM_MANIFEST_MALFORMED`, else `Ok(None)`. `.yaml`/`.yml` — the whole text. Any other extension — `Ok(None)`.
   3. When no line of the document starts with `apiVersion:` or `kind:`, return `Ok(None)` (not a manifest).
   4. Bytes that are not UTF-8 → `AM_MANIFEST_MALFORMED { line: None }`. Parse the document with `ordin_types::yaml::read_restricted_node(text, 1_048_576)` and map its error by `code()`: `YAML_FEATURE_REFUSED` (anchor, alias or custom tag, LW-46) → `AM_YAML_FEATURE_REFUSED` with `feature = "anchor, alias or custom tag"`; `YAML_TOO_LARGE` → `AM_FILE_TOO_LARGE`; `YAML_MALFORMED` (syntax error) → `AM_MANIFEST_MALFORMED { line: None }`. Refuse a document whose top-level node is not `YamlNode::Map` with `AM_MANIFEST_MALFORMED { line: None }`.
   5. Convert the `YamlNode` tree to `YamlValue`: `Null` → `Null`; `Scalar { quoted: true, text }` → `Str(text)`; `Scalar { quoted: false, text }`: `null`/`~`/empty → `Null`; `true`/`false` → `Bool`; matching `^[-+]?[0-9]+$` and within i64 → `Int`; matching `^[-+]?([0-9]+\.[0-9]*|\.[0-9]+)([eE][-+]?[0-9]+)?$` → `Decimal(text)` (source text kept, "1.10" stays "1.10"); every other plain scalar → `Str(text)`; `Seq` → `Seq`; `Map` → `Map`. A key already present in the same mapping → `AM_MANIFEST_MALFORMED { line: None }`.
   6. Extract `apiVersion`, `kind`, `metadata` into `Manifest`; typed metadata keys (`id`, `name`, `version`, `owner`, `loading`, `description`, `category`, `class`, `audience`, `useCases`, `status`, `supersedes`) into `Metadata`, every other metadata key into `extra`; every other top-level key into `body`. A typed key holding the wrong shape (for example `audience.roles` not a sequence of strings) → `AM_FIELD_SHAPE_INVALID`.
   7. Missing `apiVersion`, `kind` or `metadata.id` → `AM_MANIFEST_FIELD_MISSING` naming each (`artefactbuild.go:141-163`). The function does not check `apiVersion`'s value; the Checker's stage one does (`apiversion_unknown`).
   8. Log nothing (pure function). Return `Ok(Some(manifest))`.
4. **Tree reading** (port of `GroupFileSets`, `artefactbuild.go:200-249`). `read_tree(files, vocab)`:
   1. Refuse each path that is absolute, contains a `..` segment, a `\`, a NUL or is not valid UTF-8, by adding it to `refused` with `AM_PATH_INVALID`; it is not read further.
   2. Parse every file with step 3. A parse error adds `(path, error)` to `refused`. Each `Ok(Some)` is a manifest file; group manifest files by their directory.
   3. For each non-manifest file, walk its ancestors from the nearest directory upwards: the first directory holding exactly one manifest owns the file; a directory holding two or more manifests stops the walk and the file goes to `unowned_files`; reaching the root without a manifest drops the file silently (not a part file). The file's name inside the part is its path relative to the owning manifest's directory.
   4. A manifest file's own entry in its part is its base name.
   5. For each manifest: `kind = canonical_kind(authored_kind, vocab)`; `None` adds `AM_KIND_UNKNOWN` to `refused`. Otherwise build `ModelPart` with `content_digest = content_digest(files)`.
   6. Sort `parts` by `manifest_path`. Vocabulary parts themselves are read before any other kind so `vocab` can be built by the caller in two passes: the caller first calls `read_tree` with an empty set, builds `VocabularySet::from_parts` from the Vocabulary parts, then calls `read_tree` again; parts refused in the first pass only for `AM_KIND_UNKNOWN` may then resolve.
5. **Canonical archive and digest.** `canonical_archive(files)` writes a POSIX ustar archive: entries in byte-wise ascending order of name; each a regular file (`typeflag '0'`) with mode `0000644`, uid and gid `0`, mtime `0`, empty `uname` and `gname`, `magic "ustar\0"`, `version "00"`, the checksum computed per POSIX; names longer than 100 bytes split into `prefix`/`name` per ustar, and a name that cannot be split (a component over 100 bytes, or a total over 255 bytes) refused with `AM_PATH_INVALID` by `content_digest`'s caller; content padded to 512 bytes; two zero blocks at the end; no gzip. `content_digest(files)` = SHA-256 of that archive as `Sha256Hex`. Empty `PartFiles` is refused by callers (a part always has its manifest file). Unit tests pin the digest of three fixture parts so a change to the archive bytes fails the build.
6. **References** (port of `References()` and `collectArtefactRefs`). Walk `body` keys in sorted order; a key in the relation map starts relation tracking; in nested maps the deepest relation key wins; a string value under a tracked relation that matches the identifier pattern yields `ManifestRef { id: trimmed, relation }`. Relation keys and their kinds (`crucible/pkg/types/artefact.go:254-330`): `systemRole`→SystemRole, `defaultRules`/`rules`/`packMembers`/`addRules`→Rule, `controlLoop`→ControlLoop, `allowedOperatingModes`/`defaultMode`/`modes`/`entry`→OperatingMode, `capabilities`→Capability, `commands`→Command, `skills`→Skill, `toolBindings`/`memoryService`→ToolBinding, `contextResources`→ContextResource, `outputContracts`/`outputContract`→OutputContract, `workspace`→WorkspaceProfile, `identity`→AgentIdentity, `memory`→Memory. `selectablePracticePacks` and `requiredPracticePacks` are not relations. `relation_kind(relation) -> Option<PartKind>` is public for the Resolver and Checker.
7. **Typed readers.** Each reads one documented key and returns `AM_FIELD_SHAPE_INVALID { key, expected }` on a present key of the wrong shape:
   - `required_practice_packs`: `requiredPracticePacks` as a sequence of maps, each `atLeast` (integer ≥ 1) and either `where: {class: <code>}` or `class: <code>` or `ids: [<id>…]` or `anyOf: [<id>…]` (port of `rulesFromSeq`, `rulesFromAnyOf`, `ruleFromMap`, `crucible/internal/registry/authoringcheck/stageone.go:283-375`); a rule with both or neither selector → `AM_FIELD_SHAPE_INVALID`.
   - `selectable_practice_packs`: `selectablePracticePacks` as a sequence of strings or of maps with `id` and optional `description`.
   - `fixed_practice_packs`: `fixedPracticePacks` (SA-NAME-07) as a sequence of strings (pack identifiers) or maps `{ask: {class: <code>}}` / `{ask: {ids: [<id>…]}}`; absent = empty, and an absent field leaves the file and its digest unchanged (ADR-089).
   - `overlay_offer(axis)`: `governanceMetadata.<axisKey>` where axisKey is `brandOverlays`, `jurisdictionOverlays`, `languageOverlays`, `styleOverlays`, `sourceWorkflowOverlays` or `handoffOverlays`; a sequence (the empty `[]` form) means `available` only; a map gives `available` and `default`, where an empty-string default reads as `None`.
   - `required_overlays`: keys `required<Axis>Overlays` with `anyOf: [<id>…]` (`artefact.go:39-48`).
   - `required_control_loop`: `requiredControlLoop: {required: bool, ids: [..]}`.
   - `control_loop`: the body of a ControlLoop part: `strategy` (required; absent → `Ok(None)`), `trigger`, `requires`, `modes`.
   - `prohibited_verbs`: `toolPermissions.workspaceOverrides.prohibited`.
   - `tool_bindings`: `bindings` of a ToolBinding part, keys `name`, `type`, `permission`, `forbiddenFlags`, `coverageFloor` (read as `DecimalString` from `Int` or `Decimal`), `serverType`, `scope`, `envRef`, `transport`, `argumentPaths` (SA-CLOSE-02: a sequence of at most 50 strings, each 2–512 characters, starting with `/`, every `~` followed by `0` or `1`, no empty token, a token `*` only in last position; anything else → `AM_FIELD_SHAPE_INVALID { key: "argumentPaths", expected: "JSON Pointer list" }`).
   - `argument_hint`: the top-level key `argument-hint` of a Command part's front matter (in `body`) as a string of at most 200 characters; another shape or a longer string → `AM_FIELD_SHAPE_INVALID { key: "argument-hint", expected: "string up to 200 characters" }` (SA-CLOSE-01).
   - `OutputContract::read`: `sections` as a sequence of maps with `name` (non-empty string, required), `meaning` (string, default empty), `required` (bool, default false), `evidenceThreshold` (integer 0–1,000, default 0), `derived` (bool, default false); any item of another shape → `AM_FIELD_SHAPE_INVALID { key: "sections", expected: "list of sections with a name" }` (SA-CLOSE-03).
   - `requires_human_approval`: `defaultToolPolicy.requiresHumanApproval` of a Capability (ADR-090 item 1).
   - `pack_toolchain`: `toolchain.forbiddenFlags`, `toolchain.coverageFloor` of a PracticePack.
   - `memory_contract`: `retains`, `excludes`, `retention.maxAgeDays` or `retention.unbounded`, `store.location`, `store.memoryService` of a Memory part.
   - `publish_gate`: `governanceMetadata.publishGate` as a string.
   - `applies_to`: `appliesTo.workspaces` as a sequence of strings `<id>` or `<id>@<range>` (port of `ParseAppliesTo`, `crucible/internal/registry/compositioncheck/compositioncheck.go:81-96`).
   - `vocabulary`: `axis` (one of the eight `VocabularyAxis` values, else `AM_FIELD_SHAPE_INVALID`) and `terms[]` with `code`, `label`, `description`.
   - `journey`: crucible's `seams`, `trigger` and ordin's `stages[]` with `id`, `name`, `role`, `agent`, `produces`, `handoverPlace.type`, `handoverPlace.target`, `answerWithinDays`, `next[]` (`stage`, `when`), `together`, `sendsBackTo[]` (`stage`, `when`, `maxRounds`), and `slackChannel` (a map with `id` and `name`, both non-empty strings; optional) (08 *Lifecycle part*; amended January 2026, ADR-102). A `slackChannel` of any other shape → `AM_FIELD_SHAPE_INVALID { key: "slackChannel", expected: "a map with id and name" }`, which makes `journey()` fail for the whole part.
8. **File placement and render** (port of `pathFor` and `render`, `crucible/internal/registry/forge/render.go:28-176`). `part_file_path(kind, id)`: parse `id` with `Urn::parse` (failure → `AM_ID_INVALID`); `base = "models/@" + publisher + "/" + kind_dir + ("/" + categories.join("/") when non-empty)`; then by the kind's `FileConvention`: MarkdownFile → `base/<slug>.md`; SkillDirectory → `base/<slug>/SKILL.md`; YamlFile → `base/<slug>.yaml`; YamlInDirectory → `base/<slug>/<file_name>`; ControlLoopFile → `base/control-loop-<slug>.md`; no convention → `AM_NO_FILE_CONVENTION`. `render_part(kind, manifest, body)`: Markdown conventions give `"---\n" + render_manifest_yaml(manifest) + "---\n" + body`, with a trailing `\n` appended when `body` is non-empty and lacks one; YAML conventions give `render_manifest_yaml(manifest)` and refuse a non-empty `body` with `AM_FIELD_SHAPE_INVALID { key: "body" }`.
9. **YAML emission** (fixed code, no library). Key order: `apiVersion`, `kind`, `metadata` (typed keys in the order of `Metadata`'s fields, then `extra` sorted), then `body` keys sorted. Indent two spaces; sequences as `- ` items under their key at the same indent plus two; empty sequence `[]`, empty map `{}`. A string is written plain only when it matches `^[A-Za-z0-9_./@][A-Za-z0-9 _./@:+-]*$`, does not end with a space or `:`, contains no `: ` or ` #`, and is not one of `true false yes no on off null ~` (any case) and does not parse as `Int` or `Decimal` under step 3.5; otherwise it is written double-quoted with JSON escaping. `Decimal` is written as its source text. LF line endings and exactly one trailing newline. `LockFile::to_canonical_yaml` uses the same emitter with keys in the struct's declared order and `artefacts` sorted by (kind, name, version) (port of `crucible/pkg/canonical/yaml.go:19-90`). `LockFile::from_yaml(bytes)` reads through the same restricted reader: bytes not UTF-8, or any `YamlError` from `ordin_types::yaml::read_restricted_node(text, 1_048_576)` (the tree converted by step 3.5) (`YAML_FEATURE_REFUSED`, `YAML_MALFORMED`, `YAML_TOO_LARGE`) → `AM_LOCK_FILE_INVALID { reason: "yaml" }`; `lockfileSchema` other than `ordin.lock/v1` (`LOCK_FILE_SCHEMA`) → `{ reason: "schema" }`; a required key missing or of the wrong shape → `{ reason: "key" }`. A stage with `slack_channel = Some` is written with the key `slackChannel` after `sendsBackTo`, as `slackChannel: { id: <id>, name: <name> }`; `None` writes no key, so a lifecycle without Slack renders byte for byte as before (amended January 2026, ADR-102).
10. **Canonical JSON and versions.** `canonical_json` is `ordin_types::encoding::canonical_json` re-exported (object keys sorted byte-wise at every level, no insignificant whitespace); this crate does not define its own; numbers are never present in the values ordin hashes (`DecimalString` is a string). `compare_versions` parses both with the version pattern (failure → `AM_VERSION_INVALID`) and applies SemVer 2.0.0 precedence: numeric major, minor, patch; a version with a pre-release ranks below the same version without; pre-release identifiers compared dot by dot, numeric identifiers numerically and below alphanumeric ones; build metadata ignored.
11. **Vocabulary set.** `from_parts` takes each part of kind Vocabulary, reads `vocabulary()`; a second part for one axis → `AM_VOCABULARY_AXIS_DUPLICATE` (crucible's `sync_duplicate_vocabulary_axis`). Status terms `draft`, `approved`, `deprecated` are always treated as present on `artefact-status` even when the repository omits them, because ordin's code gives them meaning (ADR-091 item 4).
12. **PartSource contract.** The trait carries the rules every implementation must keep and that the Agent Model's test kit (`ordin_agent_model::testkit::InMemorySource`, *chosen here*, behind the `testkit` Cargo feature) enforces: resolution order `home` → `profile` (listed order) → bundled; the first step that finds the identifier wins; two repositories at the same step → `Ambiguous`; a repository the person added is never searched for another repository's agent; `files()` returns committed bytes only. Callers recompute the digest of `files()` and refuse a mismatch with `AM_SOURCE_CORRUPT` (logged by the caller with the part identifier, never file content); an implementation does not compare digests itself (reconciliation row 542). A part the source's ingest refused is `NotFound` to `resolve`, `resolve_any_kind`, `ingredients` and `used_by`, and absent from `repository_parts`. `citing_parts` returns only head parts. `testkit::assert_part_source_contract` asserts each of these rules as one named case, in this order: `order_home_first`, `order_profile_listed`, `order_bundled_last`, `ambiguous_same_step`, `added_repository_isolated`, `latest_non_draft`, `explicit_version`, `withdrawn_flagged`, `refused_not_found`, `files_committed_bytes`, `ingredients_latest`, `withdrawn_entries`, `used_by_reverse_closure`, `citing_parts_by_code`, `bundled_repository_id`.
13. **Withdrawn-list file.** `parse_withdrawn_list(bytes)`: apply steps 3.1 (size), 3.4 (the restricted reader with its code mapping; a feature refusal is `AM_YAML_FEATURE_REFUSED`) and 3.5 (scalar conversion) to the whole text; top level not a mapping → `AM_WITHDRAWN_LIST_INVALID { reason: "not_a_mapping" }`; `listSchema` absent or not `ordin.withdrawn/v1` → `{ reason: "schema" }`; `entries` absent reads as empty, present and not a sequence → `{ reason: "entries_not_a_list" }`. For each entry (1-based index `n`): `part` must be a map with `kind` (a `PartKind` spelling exactly as the enum, e.g. `Skill`) and `id` (string), optional `version` and `contentHash` (strings); `reason` and `withdrawnAt` strings; optional `replacement` map with `id` and optional `version`. Any missing or wrongly shaped key → `{ reason: "entry_shape", entry: Some(n) }`; an unknown key in an entry → `{ reason: "entry_unknown_key", entry: Some(n) }`. Values are not otherwise validated. `render_withdrawn_list` writes `listSchema`, then `entries` in list order, each entry's keys in the order `part` (`kind`, `id`, `version`, `contentHash`), `reason`, `withdrawnAt`, `replacement` (`id`, `version`), omitting absent optional keys, through the step-9 emitter. `parse_withdrawn_list(render_withdrawn_list(l).as_bytes()) == Ok(l)` for every valid `l` (property test).
14. **Draft model: fields shape.** `fields_of(part)` returns the JSON object `{"metadata": {...}, "spec": {...}, "text": <string or null>}`: `metadata` holds the typed `Metadata` keys under their manifest spellings (`id`, `name`, `version`, `owner`, `loading`, `description`, `category`, `class`, `audience` {`domains`, `roles`, `languages`}, `useCases` [{`title`, `summary`}], `status`, `supersedes`) with absent options omitted, then every `extra` key; `spec` is `body` converted value by value (`Null`→null, `Bool`→bool, `Int`→number, `Decimal(t)`→`{"$decimal": t}`, `Str`→string, `Seq`→array, `Map`→object); `text` is `markdown_body` of the manifest file for Markdown conventions, else null. `part_from_fields(kind, fields)` inverts it: `fields` not an object, or `metadata.id` absent or not a string → `AM_DRAFT_FIELDS_INVALID { field, expected }`; a typed key of the wrong JSON type → the same error naming it; a JSON number that is not an integer within i64 → `{ field, expected: "integer or {\"$decimal\": text}" }`; `$decimal` text not matching step 3.5's decimal pattern → the same error. The manifest gets `api_version = API_VERSION` and `authored_kind` = the kind's first authored spelling (or the kebab-case kind name when the table lists none). Then `path = part_file_path(kind, id)?`, `file = render_part(kind, &manifest, text)?`, `files = {basename(path): file.content}`, `content_digest(files)`; the result's `manifest_path = path`.
15. **Draft model: form.** `form(kind)` is a fixed JSON Schema per kind, generated from one table in code: the common `metadata` object (`id` string matching the identifier pattern, `name` 1–200 characters, `version` matching the version pattern, `owner`, `description` up to 2,000 characters, `category`, `class`, `audience`, `useCases`, `status` enum `draft|approved|deprecated`, `supersedes`), `spec` with one property per body key the kind's typed reader of step 7 reads (for example `requiredPracticePacks`, `fixedPracticePacks` for WorkspaceProfile; `stages` for Journey; `bindings` for ToolBinding) and `additionalProperties: true` so unknown keys pass through, and `text` (string) only for Markdown conventions. A kind with no file convention returns the schema with `"readOnly": true` at the top level. A unit test validates `fields_of` of every frozen crucible fixture part against `form(kind)`.
16. **Draft model: scaffold.** `scaffold(kind, name, identifier)`: `is_part_identifier(identifier)` false → `AM_ID_INVALID`; build `Metadata { id: identifier, name, version: "1.0.0", owner: "", status: Some("draft"), .. }` with an empty `body`; for Markdown conventions `text = "# " + name + "\n"`; render through step 14's tail. `owner` is left empty: the Authoring Service fills it from the repository before the Checker runs (the Checker's stage one fails an empty owner).
17. **Draft model: agent parts.** `agent_parts_from_fields(draft, publisher, reused, generated)`:
    1. `draft.name` empty after trim → `AM_DRAFT_FIELDS_INVALID { field: "name" }`; `publisher` not matching `^[a-z][a-z0-9-]*$` → `AM_ID_INVALID`.
    2. Workspace identifier `w = mint_identifier(WorkspaceProfile, publisher, None, draft.name)?`; its slug `s`. System role identifier `urn:ordin:<publisher>:workspaces:<s>:system-role` (crucible's placement, `forge/render.go:169-172`).
    3. Each `generated` entry: `kind` with no file convention, or WorkspaceProfile, or SystemRole → `AM_DRAFT_KIND_NOT_COMPOSABLE { kind }`. Its identifier is `fields.metadata.id` when present, else `mint_identifier(kind, publisher, None, name)?`; its fields get `version` `1.0.0` and `status` `draft` when absent; `part_from_fields`.
    4. The SystemRole part: name `<draft.name> role`, description = `draft.purpose`, text = `render_write_up(draft)` without its `## evidence-index` section.
    5. The WorkspaceProfile part: name = `draft.name`, description = `draft.purpose`, version `1.0.0`, status `draft`, `systemRole` = the system role identifier, and, for every generated and reused part in input order (reused first), its identifier appended under the relation key for its kind: Rule → `defaultRules`, Skill → `skills`, Command → `commands`, Capability → `capabilities`, ContextResource → `contextResources`, ToolBinding → `toolBindings`, OutputContract → `outputContracts`, Memory → `memory` (a second Memory → `AM_DRAFT_KIND_NOT_COMPOSABLE`), ControlLoop → `controlLoop` (a second → the same error), PracticePack → `fixedPracticePacks` as a `Pack` entry, OperatingMode → `allowedOperatingModes`. Any other reused kind → `AM_DRAFT_KIND_NOT_COMPOSABLE`. Identifiers are de-duplicated, first occurrence kept.
    6. Return `[workspace, system role, generated parts in input order]`.
18. **Draft model: write-up.** `render_write_up(draft)` emits Markdown with the `role-dossier` OutputContract's section names as level-2 headings (`crucible/models/@default/output-contracts/role-dossier.yaml`), in the contract's order, writing only sections the draft has content for: `# <name>`; `## role-identity` = `purpose`; `## responsibilities` = one bullet per `jobs[].text`; `## outputs-artefacts` = one bullet per `tasks` entry; `## decisions-owned` = bullets "Will: …" per `will`, then "Won't: …" per `wont`; `## tools-systems` = one bullet per `needs` entry. Sections without a draft field (`triggers-inputs`, `collaborators-handoffs`, `cadence-rhythm`, `success-measures`, `failure-modes`, `vocabulary`, `evidence-index`) are not written, so the gap check reports them. Line endings LF; one trailing newline.

## Data Model

N/A — the Agent Model is a library with no tables, no files of its own and no journal records. It defines the shapes of four files other components write: `ordin.lock` (written by the Resolver and Compiler into each bundle and copied by the Installers into the agent folder), `bundle.manifest.json` (Resolver and Compiler), `withdrawn.yaml` (read by the Catalogue Service, written through a withdrawal draft of the Authoring Service) and the part files of the ordin format (Format Plug-ins, Authoring Service).

## Error Table

All errors are returned values of an in-process API; none reaches the person directly. The caller maps them to its own surface.

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A file is larger than 1 MiB | `Err(permanent)` | `AM_FILE_TOO_LARGE` | `AmError::FileTooLarge { path, bytes }` |
| Front matter opened with `---` but never closed, YAML syntax error, top level not a mapping, non-string or duplicate key | `Err(permanent)` | `AM_MANIFEST_MALFORMED` | `AmError::ManifestMalformed { path, line }` |
| `apiVersion`, `kind` or `metadata.id` missing | `Err(permanent)` | `AM_MANIFEST_FIELD_MISSING` | `AmError::ManifestFieldMissing { path, fields }` |
| Anchor, alias or custom tag in a manifest | `Err(permanent)` | `AM_YAML_FEATURE_REFUSED` | `AmError::YamlFeatureRefused { path, feature }` |
| Authored kind maps to no kind | `Err(permanent)` | `AM_KIND_UNKNOWN` | `AmError::KindUnknown { path, authored }` |
| `apiVersion` is not `ai.enterprise/v1` when `Manifest::require_api_version` is called | `Err(permanent)` | `AM_API_VERSION_UNKNOWN` | `AmError::ApiVersionUnknown { path, found }` |
| Path absolute, with `..`, `\`, NUL, non-UTF-8, or not representable in ustar | `Err(permanent)` | `AM_PATH_INVALID` | `AmError::PathInvalid { path }` |
| Identifier does not parse as a crucible URN where one is required | `Err(permanent)` | `AM_ID_INVALID` | `AmError::IdInvalid { id }` |
| Version does not match the version pattern | `Err(permanent)` | `AM_VERSION_INVALID` | `AmError::VersionInvalid { version }` |
| `metadata.status` is not a known status term | `Err(permanent)` | `AM_STATUS_UNKNOWN` | `AmError::StatusUnknown { status }` |
| A typed key is present with the wrong shape | `Err(permanent)` | `AM_FIELD_SHAPE_INVALID` | `AmError::FieldShapeInvalid { key, expected }` |
| Render or placement asked for a kind with no file convention | `Err(permanent)` | `AM_NO_FILE_CONVENTION` | `AmError::NoFileConvention { kind }` |
| Two vocabulary parts for one axis | `Err(permanent)` | `AM_VOCABULARY_AXIS_DUPLICATE` | `AmError::VocabularyAxisDuplicate { axis }` |
| `sha256:` digest text malformed | `Err(permanent)` | `AM_DIGEST_INVALID` | `AmError::DigestInvalid { value }` |
| Lock file YAML does not parse, schema is not `ordin.lock/v1`, or a required key is missing | `Err(permanent)` | `AM_LOCK_FILE_INVALID` | `AmError::LockFileInvalid { reason }` |
| `PartSource` cannot read its clone or index | `Err(retryable)` | `AM_SOURCE_UNAVAILABLE` | `PartSourceError::Unavailable { retryable: true }` |
| `PartSource` has no readable withdrawn list and no earlier copy | `Err(permanent)` | `AM_SOURCE_WITHDRAWN_LIST_UNREADABLE` | `PartSourceError::WithdrawnListUnreadable { repository }` |
| `PartSource` files do not hash to the part's digest | `Err(permanent)` | `AM_SOURCE_CORRUPT` | `PartSourceError::Corrupt { part_identifier }` |
| `withdrawn.yaml` not a mapping, wrong `listSchema`, `entries` not a list, an entry of the wrong shape or with an unknown key | `Err(permanent)` | `AM_WITHDRAWN_LIST_INVALID` | `AmError::WithdrawnListInvalid { reason, entry }` |
| Draft fields not an object, `metadata.id` missing, a typed field of the wrong type, a non-integer number without `$decimal`, an empty draft name | `Err(permanent)` | `AM_DRAFT_FIELDS_INVALID` | `AmError::DraftFieldsInvalid { field, expected }` |
| A generated or reused part of a kind a WorkspaceProfile cannot reference, or a second Memory or ControlLoop | `Err(permanent)` | `AM_DRAFT_KIND_NOT_COMPOSABLE` | `AmError::DraftKindNotComposable { kind }` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Agent Model

  Scenario: Happy path — a skill manifest is read and placed
    Given the file "models/@default/skills/quality/codegen/SKILL.md" with front matter apiVersion "ai.enterprise/v1", kind "skill" and metadata.id "urn:ordin:default:skills:quality:codegen"
    When parse_manifest is called with that path and its bytes
    Then it returns a manifest whose authored kind is "skill" and whose slug is "quality-codegen"
    And part_file_path(Skill, the id) returns "models/@default/skills/quality/codegen/SKILL.md"

  Scenario: Parity — every frozen crucible fixture reads
    Given the 516 files of "crucible/models/@default" at commit e270904
    When read_tree is called twice, the second time with the vocabulary set built from the first
    Then no manifest is refused
    And every part's kind equals the kind frozen crucible's ingest assigns it

  Scenario: Edge — a file without a manifest marker is not a part
    Given the file "models/@default/workspaces/README.md" with no front matter
    When parse_manifest is called
    Then it returns Ok(None)

  Scenario: Edge — the digest ignores file order and timestamps
    Given two PartFiles maps with the same names and bytes inserted in different orders
    When content_digest is computed for both
    Then both digests are equal

  Scenario: Edge — a file shared by two manifests in one directory belongs to no part
    Given a directory holding "a.md" and "b.md", both manifests, and a file "notes.txt"
    When read_tree is called
    Then "notes.txt" is listed in unowned_files

  Scenario: Edge — absent fixedPracticePacks leaves the file untouched
    Given the bundled cspa-431 workspace profile without fixedPracticePacks
    When it is parsed and fixed_practice_packs is read
    Then the result is empty
    And the part's content digest is unchanged from the file's bytes

  Scenario: Edge — an ask entry in fixedPracticePacks
    Given fixedPracticePacks holds "urn:ordin:default:practice-packs:stack:go" and an entry ask with class "testing"
    When fixed_practice_packs is read
    Then it returns one Pack entry and one Ask entry with class "testing"

  Scenario: Error — unclosed front matter
    Given a Markdown file starting "---" with a line "kind: rule" and no closing "---"
    When parse_manifest is called
    Then it returns AM_MANIFEST_MALFORMED

  Scenario: Error — YAML alias refused
    Given a manifest whose body uses the anchor "&base" and the alias "*base"
    When parse_manifest is called
    Then it returns AM_YAML_FEATURE_REFUSED

  Scenario: Error — required pack rule with two selectors
    Given requiredPracticePacks holds one rule with both class "stack" and ids ["urn:ordin:default:practice-packs:stack:go"]
    When required_practice_packs is read
    Then it returns AM_FIELD_SHAPE_INVALID with key "requiredPracticePacks"

  Scenario: Error — no file convention
    When part_file_path is called for kind CrewProfile
    Then it returns AM_NO_FILE_CONVENTION

  Scenario: Error — path traversal in a tree
    Given a tree containing the path "models/../secrets.yaml"
    When read_tree is called
    Then that path is listed in refused with AM_PATH_INVALID and is not read

  Scenario: Edge — a command's argument hint is read
    Given the file "models/@default/commands/compare-quotes.md" whose front matter holds argument-hint "<suppliers> <quarter>"
    When the manifest is parsed and argument_hint is called
    Then the result is "<suppliers> <quarter>"

  Scenario: Error — argument hint of the wrong shape
    Given a Command part whose front matter holds argument-hint as a list
    When argument_hint is called
    Then it fails with AM_FIELD_SHAPE_INVALID naming "argument-hint"

  Scenario: Edge — a tool binding's argument paths
    Given a ToolBinding binding "email_send" with argumentPaths ["/to/[]/address", "/attachments/*"]
    When tool_bindings is called
    Then the binding's argument_paths are "/to/[]/address" and "/attachments/*"

  Scenario: Error — an argument path that is not a JSON Pointer
    Given a ToolBinding binding with argumentPaths ["to.address"]
    When tool_bindings is called
    Then it fails with AM_FIELD_SHAPE_INVALID naming "argumentPaths"

  Scenario: Happy path — OutputContract sections
    Given the role-dossier OutputContract
    When OutputContract::read is called and sections are listed
    Then the first section is "role-identity" and each section carries required, evidence_threshold and derived

  Scenario: Error — an OutputContract section without a name
    Given an OutputContract whose second section has no name
    When OutputContract::read is called
    Then it fails with AM_FIELD_SHAPE_INVALID naming "sections"

  Scenario: PartSource — ambiguous borrowed identifier
    Given the in-memory test source holds the identifier "urn:ordin:acme:rules:tone" in two profile repositories
    And the agent's home repository does not hold it
    When resolve is called for kind Rule and that identifier
    Then it returns Resolution::Ambiguous naming both repositories

  Scenario: PartSource — a part refused at ingest is not found
    Given the in-memory test source holds the Skill "urn:ordin:acme:skills:broken" marked refused by its ingest
    When resolve is called for kind Skill and that identifier
    Then it returns Resolution::NotFound

  Scenario: PartSource — parts citing a vocabulary term
    Given the home repository's vocabulary defines the code "finance" on the category axis
    And the Rule "urn:ordin:acme:rules:tone" cites it
    When citing_parts is called for the home repository, axis Category and code "finance"
    Then it returns that Rule only

  Scenario: Withdrawn list — round trip
    Given a withdrawn.yaml with listSchema "ordin.withdrawn/v1" and one entry for Skill "urn:ordin:default:skills:rate-compare" version "1.2.0" with a replacement
    When parse_withdrawn_list is called and the result is rendered with render_withdrawn_list
    Then parsing the rendered text gives the same WithdrawnListFile

  Scenario: Withdrawn list — wrong schema
    Given a withdrawn.yaml whose listSchema is "other/v9"
    When parse_withdrawn_list is called
    Then it returns AM_WITHDRAWN_LIST_INVALID with reason "schema"

  Scenario: Draft model — fields round trip
    Given the bundled Skill "urn:ordin:default:skills:quality:codegen"
    When fields_of is called and part_from_fields is called on the result
    Then the rendered manifest file bytes equal the original's

  Scenario: Draft model — an agent draft becomes parts
    Given an agent draft named "Contract reviewer" in publisher "acme" with two jobs and the reused Rule "urn:ordin:default:rules:layer:output-language"
    And one generated Skill named "Flag clauses"
    When agent_parts_from_fields is called
    Then it returns a WorkspaceProfile "urn:ordin:acme:workspaces:contract-reviewer", the SystemRole "urn:ordin:acme:workspaces:contract-reviewer:system-role" and the Skill
    And the WorkspaceProfile lists the Rule under defaultRules and the Skill under skills

  Scenario: Draft model — a generated overlay is refused
    Given one generated part of kind BrandOverlay
    When agent_parts_from_fields is called
    Then it returns AM_DRAFT_KIND_NOT_COMPOSABLE

  Scenario: Draft model — write-up headings
    Given an agent draft with purpose, jobs and needs but no will or won't
    When render_write_up is called
    Then the text has the headings "role-identity", "responsibilities" and "tools-systems" in that order
    And it has no "decisions-owned" heading
```

## Performance, Security, Observability

- **Performance targets:** `parse_manifest` p95 ≤ 2 ms for a 100 KB file; `read_tree` over the 516-file bundled catalogue ≤ 400 ms p95 on the reference laptop (A-21 hardware class); `content_digest` ≥ 200 MB/s; resident memory of a `TreeReading` ≤ 3 × the tree's bytes. Pure CPU; no I/O inside the crate.
- **Security:** every manifest and file is data: no field is executed, interpolated into a command or used as a path without `AM_PATH_INVALID` checks (R-84). YAML anchors, aliases and custom tags are refused, which removes alias-expansion attacks. File and path limits bound memory. The crate holds no secret and reads no environment variable.
- **Observability:** the crate does not log; it returns typed errors. Callers log the error code with `path` or `part_identifier`, never file content. No metrics or spans (SA-XC-04, SA-XC-05); in the agent profile the calling component owns any span.

## Gaps

None.

---

## Amendment January 2026 — Slack for working together (ADR-102)

> Source: decision 82 (a lifecycle stage can name a Slack channel); HLD 0.50.0, ADR-102; 08 *Lifecycle part* has `slackChannel`.

`JourneyStage.slack_channel` and `JourneySlackChannel` are in *Public Interface*; step 7 reads `slackChannel` and step 9 writes it (both marked in place). A lifecycle without the key reads and renders as before.

### Error Table

No new code: the existing `AM_FIELD_SHAPE_INVALID` row covers the new key.

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: Agent Model — stage Slack channel

  Scenario: A stage names a Slack channel
    Given a journey part whose stage "Design" has slackChannel { id: C04ABCDEF12, name: design-reviews }
    When journey() is read
    Then the stage's slack_channel is Some with id "C04ABCDEF12" and name "design-reviews"

  Scenario: A stage without the key
    Given a crucible journey with seams and no stages, and an ordin journey whose stages carry no slackChannel
    When journey() is read
    Then every stage's slack_channel is None

  Scenario: A malformed slackChannel
    Given a stage with slackChannel "design-reviews" (a string, not a map)
    When journey() is read
    Then it fails with AM_FIELD_SHAPE_INVALID naming key "slackChannel"

  Scenario: Rendering keeps lifecycles without Slack unchanged
    Given a lifecycle part whose stages have no slack_channel
    When it is rendered and read back
    Then the bytes equal the rendering before January 2026 and no slackChannel key appears
```

### Performance, Security, Observability

Unchanged: one more optional key per stage; the value is never logged.

### Gaps

None.
