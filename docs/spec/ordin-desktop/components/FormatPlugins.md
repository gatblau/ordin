# SPEC: Format Plug-ins

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-format-plugins` · **Build phase:** 2 · **Programs:** D P A
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

The Format Plug-ins turn the files of an agent format into Agent Model parts and back (02:467–470, ADR-087, ADR-094). One Rust module per format, compiled into the build, each with four operations: *detect* (is a tree in this format), *read* (format files → Agent Model parts, with a mapping report of what has no equivalent), *write* (Agent Model parts → format files, with a loss report of what the format cannot hold) and *version* (the format versions it reads and writes). First release: the **ordin** format (lossless), the **APM** format (read and write, pinned to tested versions), and two read-only import plug-ins, **Claude Code** and **Copilot**. It also proposes the kind of a single imported file ("This reads like an ability", 11 §5.30). Anything that does not map is reported, never guessed; no plug-in is loaded at run time (ADR-087).

## Approach

A closed Rust enum of four plug-ins behind one trait, each a pure function over an in-memory tree (`BTreeMap<path, bytes>`); the caller (Catalogue Service, Authoring Service, Package Inspector) reads and writes disk and Git. The ordin plug-in delegates to the Agent Model's tree reading and file placement, so the ordin format has one implementation. Mapping and loss reports are returned values, not stored rows: the crate has no Journal dependency (2B-3), so the caller keeps each report with its own record (Diagnostics, the draft's check results). Rejected: (1) run-time loading of plug-ins (shared libraries, WebAssembly) — ADR-087 rejects it; (2) driving the `apm` program — ADR-082 removed it from the bundle.

## Shared Context

### Types from the Agent Model (`ordin-agent-model`), used unchanged

```rust
pub use ordin_types::{PartKind, Sha256Hex, AgentRepositoryId};
pub struct PartFiles(pub std::collections::BTreeMap<String, Vec<u8>>);
pub struct ModelPart {
    pub kind: PartKind,
    pub manifest: Manifest,
    pub manifest_path: String,     // repository-relative path of the manifest file
    pub files: PartFiles,          // keyed relative to the part's directory
    pub content_digest: Sha256Hex,
}
pub struct Manifest { pub api_version: String, pub authored_kind: String,
                      pub metadata: Metadata, pub body: BTreeMap<String, YamlValue> }
pub enum YamlValue { Null, Bool(bool), Int(i64), Decimal(String), Str(String),
                     Seq(Vec<YamlValue>), Map(BTreeMap<String, YamlValue>) }
pub enum LoadingMode { AlwaysOn, OnDemand, Event, ModeSwitch, RegisteredInvocation, Structural, None }
pub struct TreeReading { pub parts: Vec<ModelPart>, pub refused: Vec<(String, AmError)>,
                         pub unowned_files: Vec<String> }
pub struct VocabularySet { /* private */ }
// functions used:
pub fn read_tree(files: &BTreeMap<String, Vec<u8>>, vocabulary: &VocabularySet) -> TreeReading;
pub fn parse_manifest(path: &str, bytes: &[u8]) -> Result<Option<Manifest>, AmError>;
pub fn part_file_path(kind: PartKind, id: &str) -> Result<String, AmError>;
pub fn render_part(kind: PartKind, manifest: &Manifest, body: Option<&str>) -> Result<RenderedFile, AmError>;
pub fn content_digest(files: &PartFiles) -> Sha256Hex;
pub fn mint_identifier(kind: PartKind, publisher: &str, category: Option<&str>, name: &str) -> Result<String, AmError>;
pub fn kind_info(kind: PartKind) -> &'static KindInfo;     // default_loading, kind_dir, offered
pub const API_VERSION: &str = "ai.enterprise/v1";
// Used by TreeSource (step 12): the PartSource trait and its types, unchanged.
pub use ordin_agent_model::{PartSource, SourceScope, RepositoryRef, SourcedPart, Resolution,
    PartSourceError, PartRef, prefixed_digest, compare_versions};
// 2C types the Agent Model copies unchanged (phase-2c-shared-types-domain.md §Part, §WithdrawnListEntry, §VocabularyTerm).
pub use ordin_types::{Part, PartId, PartStatus, WithdrawnListEntry, VocabularyAxis};
```

Libraries: `async-trait` 0.1 (for `impl PartSource for TreeSource`), `uuid` 1 (`PartId`, SA-ID-01), `thiserror` 1, `serde` 1. No YAML library of its own: `apm.yml` and every front matter are read only through Shared types' restricted reader `ordin_types::yaml::read_restricted_node(text, max_bytes) -> Result<YamlNode, YamlError>` (over `saphyr`; SharedTypes.md module `yaml`; Phase 1 SA-YAML-01, decision P4-1; HLD 05 *YAML parsing*), with `max_bytes = MAX_FILE_BYTES`. The untyped form is used rather than `read_restricted`, because `version` and every other scalar must keep its exact source text (`1.10` stays `1.10`).

### Types from Phase 2C (`ordin-types`)

```rust
/// 2C §AgentRepository — the format of an agent repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentFormat { Ordin, Apm }
```

### HLD facts this spec relies on

| Fact | Source |
|---|---|
| APM package layout: `apm.yml`, `.apm/` instructions, prompts, skills, agents and hooks; mapping report on read, loss report on write; no `apm` program run | 08:58 |
| `apm.yml` carries the package name and version | 02:472 |
| Claude Code set-up, read only: `CLAUDE.md` (rules and reference material), `.claude/agents/*.md` (an agent: role, rules, tools), `.claude/skills/*/SKILL.md` and its files (an ability), `.claude/commands/*.md` (a task), the MCP servers the set-up lists (tools) | 08:59 |
| Copilot set-up, read only: `.github/copilot-instructions.md` and `.github/instructions/*.instructions.md` (rules), `.github/prompts/*.prompt.md` (tasks), its agent files (an agent: role, rules, tools) | 08:60 |
| crucible's APM converter maps loading modes to APM primitives: always-on → instructions, on-demand → skills, registered-invocation → commands, event → hooks, mode-switch → chatmodes, structural → agents, none → no primitive | `crucible/internal/cli/converters/apm/apm.go:30-50` |
| A part whose mapping report is not empty is not offered; the report goes to Diagnostics | ADR-087 *Reading* |
| A non-empty loss report on publish is a failing check with **Leave it out** and **Publish to an ordin-format repository** | ADR-087 *Writing*; 11:840 |
| A draft with borrowed parts is not published to an APM-format repository; the check says so | SA-APM-01; ADR-090 item 2 |
| The fixed packs go into the package where APM can hold them; otherwise the loss report names them | ADR-089 |
| A repository whose `apm.yml` declares a newer version than tested is read-only | R-70 |

### Confirmation items bound to this component

- **C-62** — the APM plug-in reads and writes the APM layout of the versions in use, and a package ordin writes installs through `apm install`. Until the C-62 lab reports, this spec fixes the layout in *Internal Logic* step 5 and the tested-version list is empty; the stated fallback (R-70) applies: a package whose declared version is not in the tested list is read, and write to its repository is refused with `FMT_VERSION_UNTESTED`.
- **C-70** — the Claude Code and Copilot layouts are stable enough to map. Until confirmed, the paths in steps 6 and 7 are pinned to the harness versions ordin pins (ADR-044); a set-up file outside those paths is reported in the mapping report, never guessed.

### Assumptions from the Phase 3 reconciliation

- **SA-RECON-01** — The provider's names win (ruling R6): consumers hold the concrete `Arc<FormatPlugins>` and call `get(id)`, `detect`, `read_any` and `propose_kind`; there is no `FormatPlugins` trait and no `detect_any`, `read_source`, `starting_files()`, `SourceFormat`, `ImportOutcome` or `ReadPart`.
- **SA-RECON-02** — The Release Builder's `PackageReader` (O147) is built by the server program over `read_any` and `TreeSource`; this crate defines no `PackageReader` of its own.
- **SA-RECON-03** — Mapping and loss items reach the Checker as its `ReportItem` through the caller's fixed conversion using `UnmappedReason::code` and `LossReason::code`; this crate does not depend on the Checker (2B-3 row: Agent Model only).

### Configuration

None. Plug-ins read no environment variable or profile key. Limits are constants (*assumed here*): `MAX_TREE_FILES = 20_000`, `MAX_TREE_BYTES = 256 MiB`, `MAX_FILE_BYTES = 1 MiB` (Agent Model's per-file limit).

## Public Interface

Crate `ordin_format_plugins`. Names are *chosen here* unless traced.

### Who calls what

The concrete struct `FormatPlugins` is the entry point; consumers hold `Arc<FormatPlugins>` (not a trait object) and reach one plug-in through `get(id)`. There are no `detect_any`, `read_source`, `starting_files()`, `SourceFormat`, `ImportOutcome` or `ReadPart` items: consumers use the names below (reconciliation rows 626–630).

| Public item | Called by | What the caller supplies or keeps |
|---|---|---|
| `FormatPlugins::builtin`, `get` | Catalogue Service, Authoring Service, Package Inspector, server program (Release Builder's `PackageReader`) | built once by the program |
| `FormatPlugins::detect` | Catalogue Service (repository probe and fetch), Authoring Service (import) | a `SourceTree` read from the clone or the imported folder |
| `FormatPlugin::read` (through `get(id)`) | Catalogue Service (ingest; `FormatId::from(AgentFormat)` gives the id), Package Inspector | `ReadRequest`; keeps `mapping_report` (Catalogue: Diagnostics, per part through `MappingItem.part_identifier`; unreadable files are items with `UnmappedReason::Unreadable`) |
| `FormatPlugins::read_any` | Authoring Service (import of an APM package, a Claude Code or Copilot set-up), server program (`PackageReader`) | `ReadRequest`; keeps the `MappingReport` with the draft's check results |
| `FormatPlugin::write` (through `get(id)`) | Authoring Service (publish) | `WriteRequest` with every field: `borrowed` (identifiers of borrowed parts), `bodies`, `package`, `existing`, and `starting_vocabulary` (see below); it writes the returned `files` and `starting_files` bytes to the worktree itself |
| `FormatPlugins::propose_kind` | Authoring Service (**Import as a part…**, 11 §5.30) | the one file's path and bytes |
| `UnmappedReason::code`, `LossReason::code` | Authoring Service, Catalogue Service, Package Inspector (each converts mapping and loss items to the Checker's `ReportItem`, below); the Checker does not call them | — |
| `TreeSource`, `TreeSource::root` | server program, inside the Release Builder's `PackageReader` (`read(package_dir) -> (Box<dyn PartSource>, PartRef)`): it reads the folder into a `SourceTree`, calls `read_any`, builds `TreeSource::new(&outcome, repository_id, label, commit)` and returns it with `root()`. The Release Builder then resolves and checks with `SourceScope { home: RepositoryRef::Repository(repository_id), profile: vec![], include_bundled: false }` | the `ReadOutcome` of one package, a repository id it mints, a label and the commit (40 zeros outside Git) |

**`starting_vocabulary` is supplied by the Authoring Service** (O149): for an ordin-format target it passes the bundled catalogue's Vocabulary parts, obtained from the Catalogue Service as `PartSource::repository_parts(&RepositoryRef::Bundled)` filtered to kind Vocabulary, and converted to `ModelPart` from the `SourcedPart`'s manifest and `PartSource::files`. For an APM target it passes an empty list. The Catalogue Service never writes and never supplies it.

**Report items as the Checker reads them** (N185). The caller converts each report item to the Checker's `ReportItem { what, part_identifier, reason }` (Checker.md) by this fixed rule: from a `MappingItem` — `what = construct`, `part_identifier = part_identifier`, `reason = reason.code()`; from a `LossItem` — `what = what`, `part_identifier = Some(part_identifier)`, `reason = reason.code()`. The Format Plug-ins do not depend on the Checker, so the conversion lives with the caller; the codes come from here so that the strings are defined once.

```rust
use std::collections::BTreeMap;

/// Every format ordin knows. `Ordin` and `Apm` are repository formats (2C `AgentFormat`);
/// `ClaudeCode` and `Copilot` are read-only import formats (ADR-094).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatId { Ordin, Apm, ClaudeCode, Copilot }

impl FormatId {
    pub fn as_repository_format(self) -> Option<AgentFormat>;   // Ordin→Ordin, Apm→Apm, others None
    pub fn writable(self) -> bool;                              // Ordin, Apm
}
/// AgentFormat::Ordin → FormatId::Ordin, AgentFormat::Apm → FormatId::Apm.
/// Called by: Catalogue Service (a repository's stored format to the plug-in that reads it).
impl From<AgentFormat> for FormatId { fn from(f: AgentFormat) -> FormatId; }

/// An in-memory tree: '/'-separated relative paths → bytes.
#[derive(Debug, Clone, Default)]
pub struct SourceTree(pub BTreeMap<String, Vec<u8>>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Detection {
    pub format: FormatId,
    /// The version the tree declares, or "unversioned".
    pub declared_version: String,
    /// True when `declared_version` is one this plug-in was tested with.
    pub version_tested: bool,
    /// Number of agents (WorkspaceProfiles, or the format's agent files) found, for
    /// "Found 12 agents in APM format" (ADR-086).
    pub agent_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct FormatVersions { pub reads: Vec<String>, pub writes: Vec<String> }

/// What a read needs besides the tree.
#[derive(Debug, Clone)]
pub struct ReadRequest {
    /// Publisher segment for identifiers minted on import (`urn:ordin:<publisher>:…`):
    /// the target repository's publisher, or "imported" when none is chosen yet.
    pub publisher: String,
    /// The vocabulary of the target repository (rule sub-variants, status terms).
    pub vocabulary: VocabularySet,
    /// Plain words naming the source for "Imported from [source]" (11 §5.30).
    pub source_label: String,
}

#[derive(Debug, Clone)]
pub struct ReadOutcome {
    pub parts: Vec<ModelPart>,
    pub mapping_report: MappingReport,
    /// For APM packages: `apm.yml` name and version (02:472); None for other formats.
    pub package: Option<PackageIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PackageIdentity { pub name: String, pub version: String }

/// Anything read that has no Agent Model equivalent (ADR-087).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct MappingReport { pub format: Option<FormatId>, pub items: Vec<MappingItem> }

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct MappingItem {
    /// Path in the source tree.
    pub source_path: String,
    /// The construct, in the format's own words ("hook", "apm.yml key scripts").
    pub construct: String,
    pub reason: UnmappedReason,
    /// The identifier of the part the item belonged to, when one was created.
    pub part_identifier: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnmappedReason {
    /// No Agent Model kind holds it ("[what] has no place in an ordin agent", 11 §5.30).
    NoEquivalentKind,
    /// A key or field the plug-in does not read.
    UnknownField,
    /// Something that would run code (hooks, scripts); never mapped (ADR-085, R-84).
    Executable,
    /// A file outside the paths the plug-in reads for this format.
    UnrecognisedPath,
    /// The file could not be parsed.
    Unreadable,
}
impl UnmappedReason {
    /// The snake_case spelling: "no_equivalent_kind", "unknown_field", "executable",
    /// "unrecognised_path", "unreadable".
    pub fn code(self) -> &'static str;
}

#[derive(Debug, Clone)]
pub struct WriteRequest {
    /// The parts to write: the draft's changed and new parts. Unchanged parts of an ordin-format
    /// repository are not passed; their files stay as committed.
    pub parts: Vec<ModelPart>,
    /// Parts the draft references but that live in another repository (ADR-090 §2).
    pub borrowed: Vec<String>,
    /// Text bodies of new or changed Markdown parts, keyed by `metadata.id`.
    pub bodies: BTreeMap<String, String>,
    /// For APM: the package name and version to write into `apm.yml`.
    pub package: Option<PackageIdentity>,
    /// The tree as it is now on the target branch (for "starting files", ADR-088).
    pub existing: SourceTree,
    /// Ordin format only: the bundled catalogue's Vocabulary parts, one per axis, used as
    /// starting files when the target lacks them (ADR-088 decision 61). Empty for APM.
    pub starting_vocabulary: Vec<ModelPart>,
}

#[derive(Debug, Clone)]
pub struct WriteOutcome {
    /// Files to create or replace, repository-relative.
    pub files: BTreeMap<String, Vec<u8>>,
    /// Starting files the target needs that `existing` lacks (ADR-088): generated here,
    /// listed separately so the card can say they travel with the first publish.
    pub starting_files: BTreeMap<String, Vec<u8>>,
    pub loss_report: LossReport,
}

/// Anything the target format cannot hold (ADR-087).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LossReport { pub format: Option<FormatId>, pub items: Vec<LossItem> }

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LossItem {
    pub part_identifier: String,
    pub kind: PartKind,
    /// The field or whole part that cannot be written ("fixedPracticePacks", "whole part").
    pub what: String,
    pub reason: LossReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LossReason { NoPrimitive, FieldNotHeld, BorrowedPart, VersionUntested }
impl LossReason {
    /// "no_primitive", "field_not_held", "borrowed_part", "version_untested".
    pub fn code(self) -> &'static str;
}

/// Proposed kind of one imported file (11 §5.30 "This reads like an ability").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct KindProposal { pub kind: PartKind, pub from_format: FormatId, pub rule: &'static str }

pub trait FormatPlugin: Send + Sync {
    fn id(&self) -> FormatId;
    fn versions(&self) -> FormatVersions;
    fn detect(&self, tree: &SourceTree) -> Option<Detection>;
    fn read(&self, tree: &SourceTree, request: &ReadRequest) -> Result<ReadOutcome, FormatError>;
    fn write(&self, request: &WriteRequest) -> Result<WriteOutcome, FormatError>;
}

/// The four plug-ins of the first release, in detection priority order.
pub struct FormatPlugins { /* private: [Box<dyn FormatPlugin>; 4] */ }
impl FormatPlugins {
    pub fn builtin() -> FormatPlugins;
    pub fn get(&self, id: FormatId) -> &dyn FormatPlugin;
    /// Every plug-in that recognises the tree, in priority order Ordin, Apm, ClaudeCode, Copilot.
    pub fn detect(&self, tree: &SourceTree) -> Result<Vec<Detection>, FormatError>;
    /// Detect, then read with the first match.
    pub fn read_any(&self, tree: &SourceTree, request: &ReadRequest)
        -> Result<(Detection, ReadOutcome), FormatError>;
    /// Proposes the kind of one file for "Import as a part…".
    pub fn propose_kind(&self, path: &str, bytes: &[u8]) -> Result<KindProposal, FormatError>;
}

/// A read-only `PartSource` (Agent Model trait) over the parts of one read: one repository, at one
/// commit, holding exactly the parts of `outcome`. Built by the server program inside the
/// Release Builder's `PackageReader` (O147), so the Resolver and Compiler and the Checker can
/// read a local package through the same trait as the catalogue.
pub struct TreeSource { /* private */ }
impl TreeSource {
    /// `commit`: 40 lowercase hex characters, or 40 zeros for a package not under Git.
    pub fn new(outcome: &ReadOutcome, repository_id: AgentRepositoryId,
        repository_label: &str, commit: &str) -> Result<TreeSource, FormatError>;
    /// The package's one WorkspaceProfile as a `PartRef`; FMT_ROOT_NOT_SINGLE when the read
    /// produced none or more than one.
    pub fn root(&self) -> Result<PartRef, FormatError>;
}
#[async_trait::async_trait]
impl PartSource for TreeSource { /* step 12 */ }

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FormatError {
    #[error("FMT_TREE_TOO_LARGE")] TreeTooLarge { files: u32, bytes: u64 },
    #[error("FMT_PATH_INVALID")] PathInvalid { path: String },
    #[error("FMT_NOT_RECOGNISED")] NotRecognised,
    #[error("FMT_PACKAGE_MANIFEST_INVALID")] PackageManifestInvalid { path: String, line: Option<u32> },
    #[error("FMT_WRITE_UNSUPPORTED")] WriteUnsupported { format: FormatId },
    #[error("FMT_VERSION_UNTESTED")] VersionUntested { declared: String },
    #[error("FMT_PART_INVALID")] PartInvalid { identifier: String, code: String },
    #[error("FMT_KIND_NOT_PROPOSABLE")] KindNotProposable { path: String },
    #[error("FMT_ROOT_NOT_SINGLE")] RootNotSingle { found: u32 },
    #[error("FMT_COMMIT_INVALID")] CommitInvalid { commit: String },
}
```

### Example

Input tree (APM package):

```text
apm.yml                                     name: supplier-analyst / version: 1.3.0
.apm/instructions/tone.instructions.md      "Write in plain British English."
.apm/prompts/compare-quotes.prompt.md       "Compare the supplier quotes the person names."
.apm/hooks/pre-commit.sh                    "#!/bin/sh …"
```

`FormatPlugins::builtin().read_any(&tree, &ReadRequest { publisher: "acme".into(), vocabulary, source_label: "an APM package".into() })` returns `Detection { format: Apm, declared_version: "unversioned", version_tested: false, agent_count: 1 }` and a `ReadOutcome` with three parts — Rule `urn:ordin:acme:rules:imported:tone`, Command `urn:ordin:acme:commands:imported:compare-quotes`, and a WorkspaceProfile `urn:ordin:acme:workspaces:supplier-analyst` that references both — `package: Some(PackageIdentity { name: "supplier-analyst", version: "1.3.0" })`, and

```json
{"format":"apm","items":[{"source_path":".apm/hooks/pre-commit.sh","construct":"hook","reason":"executable","part_identifier":null}]}
```

## Internal Logic

1. **Tree guard (every operation).** Count files and bytes of the tree; above `MAX_TREE_FILES` or `MAX_TREE_BYTES` → `FMT_TREE_TOO_LARGE`. Any path that is absolute, contains `..`, `\` or NUL → `FMT_PATH_INVALID`. Log nothing here.
2. **Detection** (`FormatPlugins::detect`): run each plug-in's `detect` in priority order and collect every `Some`. A tree with none → `FMT_NOT_RECOGNISED` (03:1471 "The format is not recognised: one sentence says so, with the formats ordin reads; nothing is created"). Per plug-in:
   - **Ordin:** at least one file under `models/@<publisher>/` (publisher `[a-z][a-z0-9-]*`) parses as a manifest with `apiVersion: ai.enterprise/v1`; `declared_version = "ai.enterprise/v1"`, `version_tested = true`; `agent_count` = WorkspaceProfile manifests.
   - **APM:** `apm.yml` at the root parses as a YAML mapping: `ordin_types::yaml::read_restricted_node(text, MAX_FILE_BYTES)` returns `Ok(YamlNode::Map(_))` (anchors, aliases, merge keys and tags outside the YAML 1.2 core schema refused, as in the Agent Model). Any `YamlError`, or a root that is not a mapping, means the APM plug-in does not recognise the tree. `declared_version` = the value of the key the C-62 lab names; until then `"unversioned"` (C-62 fallback). `version_tested` = `declared_version ∈ APM_TESTED_VERSIONS` (a `const &[&str]`, empty until C-62). `agent_count = 1` (one package is one agent).
   - **Claude Code:** `CLAUDE.md` at the root, or any of `.claude/agents/*.md`, `.claude/skills/*/SKILL.md`, `.claude/commands/*.md`. `declared_version = "unversioned"`, `version_tested = true` (the layout is pinned to the harness version, C-70). `agent_count` = number of `.claude/agents/*.md`, plus 1 when `CLAUDE.md` exists.
   - **Copilot:** `.github/copilot-instructions.md`, or any `.github/instructions/*.instructions.md`, `.github/prompts/*.prompt.md`, `.github/agents/*.agent.md` or `.github/chatmodes/*.chatmode.md` (the last two *assumed here* pending C-70). Counting as for Claude Code.
3. **Ordin read.** `read_tree(tree, vocabulary)` from the Agent Model. Each `refused` entry becomes a `MappingItem { reason: Unreadable, construct: <AmError code> }`; each `unowned_files` entry becomes `UnrecognisedPath`. Parts keep their original bytes, so writing an unchanged part back gives the same file (lossless, ADR-087). The mapping report of a valid ordin tree is empty. No identifier is minted.
4. **Ordin write.** For each part in `request.parts`: path = `part_file_path(kind, metadata.id)`; content = `render_part(kind, manifest, bodies.get(id))`; for a part whose `files` hold extra files (a skill's scripts or references), each extra file is written at `dirname(path)/<relative name>`. `borrowed` parts are written as references only (ADR-090 §2: publishing keeps a borrowed part as a reference) — nothing is added to `files`. **Starting files** (ADR-088): when `existing` holds no Vocabulary part for an axis that a written part cites, the plug-in adds the vocabulary part for that axis from `request.starting_vocabulary` (the bundled catalogue's vocabulary parts, supplied by the caller) to `starting_files`, at the path `part_file_path` gives it; an axis missing from `starting_vocabulary` too → `FMT_PART_INVALID { identifier: <citing part>, code: "starting_vocabulary_missing" }`. The loss report is always empty. A render error → `FMT_PART_INVALID { identifier, code }`.
5. **APM read** (new code; ADR-088 *APM read has no crucible source*). Layout pinned *(assumed here, pending C-62)*:

   | APM path | Agent Model kind | Identifier minted (`mint_identifier`) |
   |---|---|---|
   | `apm.yml` | WorkspaceProfile (name, version, `description` when present) | `urn:ordin:<publisher>:workspaces:<name>` |
   | `.apm/instructions/<slug>.instructions.md` | Rule (always-on) | `…:rules:imported:<slug>` |
   | `.apm/skills/<slug>/SKILL.md` and every file under `.apm/skills/<slug>/` | Skill | `…:skills:imported:<slug>` |
   | `.apm/prompts/<slug>.prompt.md` | Command | `…:commands:imported:<slug>` |
   | `.apm/agents/<slug>.agent.md` | SystemRole of the WorkspaceProfile (the first file by name; each further file → `MappingItem NoEquivalentKind`) | `…:workspaces:<name>:system-role` |
   | `.apm/context/<slug>.context.md` | ContextResource | `…:context-resources:imported:<slug>` |
   | `.apm/chatmodes/<slug>.chatmode.md` | OperatingMode | `…:rules:modes:imported:<slug>` |
   | `.apm/hooks/**` | none — `MappingItem { reason: Executable, construct: "hook" }` | — |
   | `apm.yml` keys other than `name`, `version`, `description` | none — `MappingItem { reason: UnknownField, construct: "apm.yml key <key>" }` | — |
   | any other path | none — `UnrecognisedPath` | — |

   Each created part gets a manifest with `apiVersion: ai.enterprise/v1`, the kind's first authored spelling, `metadata.id` as minted, `name` = the file's front-matter `name`/`description` title when present, else the slug with `-` replaced by spaces and the first letter capitalised, `version` = the package version, `owner` = `"imported"`, `status: draft`; the Markdown body is the file text after any front matter. The WorkspaceProfile references every created Rule (`defaultRules`), Skill (`skills`), Command (`commands`), ContextResource (`contextResources`), OperatingMode (`allowedOperatingModes`, the first by name as `defaultMode`) and the SystemRole (`systemRole`). Front-matter keys of APM files other than `name`, `description` and `applyTo` become `UnknownField` items attached to their part. `apm.yml` is read with `read_restricted_node(text, MAX_FILE_BYTES)`, its errors mapped by `YamlError::code()`: `YAML_FEATURE_REFUSED` (`FeatureRefused { line, .. }`) and `YAML_MALFORMED` (`Malformed { line }`) → `FMT_PACKAGE_MANIFEST_INVALID { path: "apm.yml", line: Some(line) }` (`None` when `line` is 0); `YAML_TOO_LARGE` → `FMT_PACKAGE_MANIFEST_INVALID { path: "apm.yml", line: None }`. A root that is not `YamlNode::Map`, or `name` or `version` missing or not a `YamlNode::Scalar` → the same code with `line: None`. Front matter (the text between a first line `---` and the next line `---`) of every APM file is read with `read_restricted_node(front_matter, MAX_FILE_BYTES)`; a `YamlError`, or front matter that is not a mapping, does not fail the read: the part is created as if the file had no front matter, and a `MappingItem { source_path, construct: <the error's code — "YAML_FEATURE_REFUSED", "YAML_MALFORMED" or "YAML_TOO_LARGE" — or "front matter not a mapping">, reason: Unreadable, part_identifier: Some(<the part's identifier>) }` is added (*chosen here*, as step 3 turns an unreadable ordin file into an `Unreadable` item). A part whose identifier would collide with another created in the same read gets `-2`, `-3` appended to its slug in path order.
6. **APM write** (starting point `crucible/internal/cli/converters/apm/apm.go`). Refuse with `FMT_VERSION_UNTESTED` when `existing` holds an `apm.yml` whose detection has `version_tested = false` and `existing` is non-empty (R-70, C-62 fallback). Then for each part, by its effective loading mode (`metadata.loading` when set, else the kind default):
   - always-on Rule, ControlLoop, SystemRole-of-agent, overlay → `.apm/instructions/<slug>.instructions.md` (overlays and control loops also add `LossItem { what: "kind semantics", reason: FieldNotHeld }`, because APM instructions carry no axis or strategy);
   - on-demand Skill → `.apm/skills/<slug>/` with every file; ContextResource → `.apm/context/<slug>.context.md`;
   - registered-invocation Command → `.apm/prompts/<slug>.prompt.md`;
   - mode-switch OperatingMode → `.apm/chatmodes/<slug>.chatmode.md`;
   - the WorkspaceProfile → `apm.yml` (`name`, `version`, `description`) and its system role → `.apm/agents/<slug>.agent.md`;
   - every other kind (PracticePack, Evaluation, OutputContract, Vocabulary, Guideline, Memory, Journey, ToolBinding, Capability, CrewProfile, AgentIdentity, BrandOverlay, JurisdictionOverlay) → `LossItem { what: "whole part", reason: NoPrimitive }`;
   - WorkspaceProfile fields with no APM key — `requiredPracticePacks`, `selectablePracticePacks`, `fixedPracticePacks`, `governanceMetadata`, `toolPermissions`, `requiredControlLoop`, `controlLoop` — each a `LossItem { what: <key>, reason: FieldNotHeld }` (ADR-089: the fixed packs go in where APM can hold them, otherwise the loss report names them; no APM key for them is known before C-62);
   - every identifier in `request.borrowed` → `LossItem { what: "borrowed part", reason: BorrowedPart }` (SA-APM-01).
   Starting files: when `existing` has no `apm.yml`, the written `apm.yml` is reported in `starting_files` as well as `files`.
7. **Claude Code import read** (new code; layouts from 08:59; pinned per C-70). `CLAUDE.md` → one Rule (`…:rules:imported:claude-md`), body = the file text with every line of the form `@<relative path>` kept as written; each `@`-imported file that exists in the tree → its own Rule (`…:rules:imported:<file stem>`), referenced from the WorkspaceProfile; an `@` path missing from the tree → `UnrecognisedPath`. `.claude/agents/<slug>.md` → a WorkspaceProfile plus a SystemRole from the file body; its front-matter `tools:` list → for each name, a reference to a ToolBinding part minted as `…:tool-bindings:imported:<name>` with `bindings: [{name, type: builtin}]` — the Checker later decides whether the organisation profile allows it. `.claude/skills/<slug>/SKILL.md` and its sibling files → Skill. `.claude/commands/<slug>.md` → Command. `.mcp.json` server entries (`mcpServers.<name>`) → a ToolBinding each with `bindings: [{name, type: mcp, serverType: <command or url kind>, transport}]`; an `env` or `headers` value is never copied (SA-CRED-01) and becomes `UnknownField`. `.claude/settings.json`, `.claude/settings.local.json` and any `hooks` key → `Executable` items. When there is no `.claude/agents/*.md`, one WorkspaceProfile named after `source_label`'s slug references every created part. `write` returns `FMT_WRITE_UNSUPPORTED`.
8. **Copilot import read.** `.github/copilot-instructions.md` and each `.github/instructions/<slug>.instructions.md` → Rule (front-matter `applyTo` kept in the manifest body under `appliesToFiles`, *chosen here*; it is not crucible's `appliesTo`); `.github/prompts/<slug>.prompt.md` → Command; `.github/agents/<slug>.agent.md` → WorkspaceProfile with SystemRole, front-matter `tools` mapped as in step 7; `.github/chatmodes/<slug>.chatmode.md` → OperatingMode. Everything else under `.github/` → `UnrecognisedPath`. `write` returns `FMT_WRITE_UNSUPPORTED`.
9. **Single-file kind proposal** (`propose_kind`). Fixed table, first match wins, `rule` names the matching row: base name `SKILL.md` → Skill; `*.instructions.md` or `CLAUDE.md` or `copilot-instructions.md` → Rule; `*.prompt.md` or a path under `.claude/commands/` → Command; `*.chatmode.md` → OperatingMode; `*.agent.md` or a path under `.claude/agents/` → SystemRole; a JSON file with a top-level `mcpServers` map → ToolBinding; a file that parses as an ordin manifest → its own kind. Otherwise `FMT_KIND_NOT_PROPOSABLE`; the person then chooses the kind (11 §5.30).
10. **Text is data** (R-84). No plug-in executes, evaluates or follows any text it reads; front matter is parsed only for the keys named above, always with `read_restricted_node` and the error rule of step 5 (Claude Code and Copilot files included); bodies are copied as bytes. Read plug-ins never write files.
11. **Logging.** `read_any` and `write` log one `tracing` event at `info` on success and `warn` on error (fields under *Observability*); mapping and loss report items are counted, not listed.
12. **Tree source** (`TreeSource`, O147). `new`: `commit` not matching `^[0-9a-f]{40}$` → `FMT_COMMIT_INVALID`. For each part of `outcome.parts` build a `SourcedPart`: `part` = 2C `Part` with a fresh `PartId` (UUIDv7), `agent_repository_id = repository_id`, `kind`, `identifier = metadata.id`, `name`, `version`, `status = manifest.status_or_default()` (an error → `FMT_PART_INVALID { identifier, code }`), `supersedes`, `owner`, `file_path = manifest_path`, `commit`, `content_digest`, `vocabulary_terms_cited` empty, `uses` empty, `withdrawn = false`; `repository = RepositoryRef::Repository(repository_id)`; `repository_label` and `repository_display_name` = `repository_label`; `manifest`. The `PartSource` methods answer only for this repository: `resolve` and `resolve_any_kind` look in it when `scope.home` is this repository and return `NotFound` otherwise (profile repositories and the bundled catalogue are not consulted: a package is self-contained, ADR-098); the highest semantic version wins (`compare_versions`), and draft parts are not excluded, because every part an import creates has status draft (step 5); `files` returns the part's `files`; `ingredients` = its PracticePack and overlay parts; `withdrawn` = empty; `repository_parts` = all parts for this repository, empty for another; `vocabulary` = `VocabularySet::from_parts` of its Vocabulary parts (an error → `PartSourceError::Corrupt { part_identifier }` of the second vocabulary part); `used_by` = the WorkspaceProfiles whose `Manifest::references()` closure within the tree reaches the part; `citing_parts` = parts whose `metadata.category`, `metadata.class` or `metadata.audience.roles` hold `code` for the axes `Category`, `PackClass` and `AudienceRole` respectively, empty for every other axis. No method returns `Unavailable` (everything is in memory). `root()` counts WorkspaceProfile parts: exactly one → its `PartRef` (`content_hash = prefixed_digest(content_digest)`, `repository = repository_label`, `commit`, `loading` = the kind's default); otherwise `FMT_ROOT_NOT_SINGLE { found }`.

## Data Model

N/A — the crate owns no table and writes no file. "Owns: mapping and loss reports" (02:469) is met by owning the report types and their content: each report is returned to the caller, which keeps it with its own record — the Catalogue Service in Diagnostics for a repository read (ADR-087), the Authoring Service in the draft's check results for an import or a publish (02:462). This keeps the crate free of a Journal dependency, as 2B-3 lists it.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Tree over 20,000 files or 256 MiB | `Err(permanent)` | `FMT_TREE_TOO_LARGE` | `FormatError::TreeTooLarge { files, bytes }` |
| A path is absolute, has `..`, `\` or NUL | `Err(permanent)` | `FMT_PATH_INVALID` | `FormatError::PathInvalid { path }` |
| No plug-in recognises the tree | `Err(permanent)` | `FMT_NOT_RECOGNISED` | `FormatError::NotRecognised` |
| `apm.yml` refused by the restricted YAML reader (`YAML_FEATURE_REFUSED`, `YAML_MALFORMED`, `YAML_TOO_LARGE`), not a mapping, or lacks `name` or `version` | `Err(permanent)` | `FMT_PACKAGE_MANIFEST_INVALID` | `FormatError::PackageManifestInvalid { path, line }` |
| `write` called on Claude Code or Copilot | `Err(permanent)` | `FMT_WRITE_UNSUPPORTED` | `FormatError::WriteUnsupported { format }` |
| APM write to a repository whose declared version is untested (R-70, C-62 fallback) | `Err(permanent)` | `FMT_VERSION_UNTESTED` | `FormatError::VersionUntested { declared }` |
| An Agent Model render or placement fails for a part | `Err(permanent)` | `FMT_PART_INVALID` | `FormatError::PartInvalid { identifier, code }` |
| `propose_kind` finds no rule for the file | `Err(permanent)` | `FMT_KIND_NOT_PROPOSABLE` | `FormatError::KindNotProposable { path }` |
| `TreeSource::root` finds no WorkspaceProfile or more than one | `Err(permanent)` | `FMT_ROOT_NOT_SINGLE` | `FormatError::RootNotSingle { found }` |
| `TreeSource::new` given a commit that is not 40 lowercase hex characters | `Err(permanent)` | `FMT_COMMIT_INVALID` | `FormatError::CommitInvalid { commit }` |

Words shown to the person for these outcomes belong to the calling screens (11 §5.30): "The format is not recognised" with the formats ordin reads; "[what] has no place in an ordin agent"; "APM format can't hold [what], so it would be left out" (11:840).

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Format Plug-ins

  Scenario: Happy path — ordin format round trip is lossless
    Given every fixture repository tree under "crucible/models/@default"
    When the ordin plug-in reads the tree and writes every part back unchanged
    Then each written file is byte-identical to the file read
    And the mapping report is empty

  Scenario: Happy path — APM package read with a mapping report
    Given an APM tree with apm.yml name "supplier-analyst" version "1.3.0", one instructions file, one prompt and one hook
    When read_any is called with publisher "acme"
    Then the detection format is apm
    And three parts are created: a Rule, a Command and a WorkspaceProfile referencing both
    And the mapping report holds one item with construct "hook" and reason "executable"

  Scenario: Edge — APM round trip through the ordin format
    Given an APM package from the C-62 fixture set whose loss report is empty
    When it is read by APM, written by ordin, read by ordin and written by APM
    Then the APM files are byte-identical to the original package

  Scenario: Edge — write to APM loses the fixed packs
    Given a draft WorkspaceProfile with fixedPracticePacks ["urn:ordin:default:practice-packs:stack:go"]
    When the APM plug-in writes it
    Then the loss report names "fixedPracticePacks" with reason "field_not_held"

  Scenario: Edge — borrowed part to an APM repository
    Given a draft that references a part borrowed from the bundled catalogue
    When the APM plug-in writes it
    Then the loss report holds an item "borrowed part" with reason "borrowed_part"

  Scenario: Edge — Claude Code set-up never copies MCP credentials
    Given a Claude Code set-up whose .mcp.json server "jira" has an env entry "JIRA_TOKEN"
    When the Claude Code plug-in reads it
    Then a ToolBinding part for "jira" is created without the env value
    And the mapping report holds an item for the env entry with reason "unknown_field"

  Scenario: Edge — a single SKILL.md is proposed as an ability
    When propose_kind is called for "notes/SKILL.md"
    Then the proposal kind is Skill

  Scenario: Error — unrecognised tree
    Given a tree holding only "README.txt"
    When detect is called
    Then it returns FMT_NOT_RECOGNISED

  Scenario: Error — write on an import-only format
    When write is called on the Copilot plug-in
    Then it returns FMT_WRITE_UNSUPPORTED

  Scenario: Error — APM write to an untested version
    Given an existing APM repository whose declared version is not in the tested list
    When the APM plug-in writes a draft to it
    Then it returns FMT_VERSION_UNTESTED

  Scenario: Error — apm.yml uses a YAML alias
    Given a tree whose apm.yml has an anchor on line 2 and an alias "*base" on line 3
    When the APM plug-in reads it
    Then it returns FMT_PACKAGE_MANIFEST_INVALID with path "apm.yml" and line 2
    And no part is created

  Scenario: Tree source — a local APM package resolves through PartSource
    Given the APM package "supplier-analyst" read with read_any
    And a TreeSource built from the outcome with repository label "local/supplier-analyst" and commit of 40 zeros
    When root is called
    Then it returns the PartRef of "urn:ordin:acme:workspaces:supplier-analyst"
    And resolve for kind Rule and "urn:ordin:acme:rules:imported:tone" with scope home set to that repository returns Found

  Scenario: Tree source — a tree with two agents has no single root
    Given an ordin tree holding two WorkspaceProfiles
    When a TreeSource is built from its read and root is called
    Then it returns FMT_ROOT_NOT_SINGLE with found 2

  Scenario: Report codes — a mapping item converts for the Checker
    Given a MappingItem with construct "hook" and reason Executable
    When the caller converts it with reason.code()
    Then the reason text is "executable"

  Scenario: Error — path traversal
    Given a tree containing the path "../etc/passwd"
    When read_any is called
    Then it returns FMT_PATH_INVALID
```

## Performance, Security, Observability

- **Performance targets:** detect over a 5,000-file tree ≤ 150 ms p95; read of the 516-file bundled catalogue ≤ 500 ms p95 (dominated by the Agent Model's parse); APM read of a 200-file package ≤ 100 ms p95; memory ≤ 3 × tree bytes. Pure CPU; no I/O.
- **Security:** every read is of untrusted text (R-84): nothing is executed or obeyed; hooks and scripts are reported as `executable` and never mapped; MCP credentials (`env`, `headers`) are never copied into parts (SA-CRED-01); paths are validated before use; tree size is bounded. Plug-ins are compiled in; nothing is loaded at run time (ADR-087).
- **Observability:** `tracing` events (SA-OBS-01), never file content or paths beyond the tree-relative path of a failing file:
  - `format_plugins.read` (info/warn): `format`, `declared_version`, `version_tested`, `parts`, `mapping_items`, `duration_ms`, `error_code`.
  - `format_plugins.write` (info/warn): `format`, `parts`, `starting_files`, `loss_items`, `duration_ms`, `error_code`.
  - `format_plugins.detect` (debug): `formats`.
  No metrics or spans (SA-XC-04, SA-XC-05); in the agent profile, no span either (no content-bearing operation runs there besides the Release Builder's, which owns its span).

## Gaps

None.
