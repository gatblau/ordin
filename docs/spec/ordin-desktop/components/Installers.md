# SPEC: Installers

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-installers` · **Build phase:** 3 · **Programs:** D P A
> **Dependencies:** Agent Folder Manager, Resolver and Compiler, Agent Model

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

The Installers write a bundle into an agent folder for the session's harness — Claude Code first, then Copilot — and keep the **install record** (02:457–460, 04:50 revision 0.48.0). Updating and removing an agent restore the folder exactly as it was. Under Claude Code's `--restricted` mode nothing in the agent folder is read (C-61 result, 1B OQ-C61 settled (b)), so the Claude Code Installer writes, besides crucible's layout, `instructions.md` — the bootstrap `CLAUDE.md` with its `@` imports inlined, passed with `--append-system-prompt-file` — and `plugin/` — the agent's skills and commands as a plugin named after the agent, passed with `--plugin-dir`; commands are then named `<agent>:<command>` (R-94). The Installers verify an installed folder against its lock file before each session and when the person opens the agent's page, rebuilding it without asking when it no longer matches (SA-INST-01, ADR-088 *Offline verify*). The tool policy is not written into the folder's settings, which `--restricted` ignores; it is carried in the install record for the Rules Engine (ADR-090 item 1).

## Approach

A port of crucible's converters `claudecode`, `vscode` and `agentsmd` (`crucible/internal/cli/converters`), the offline lock-file check of `crucible/internal/cli/verify`, the restore on removal of `crucible/internal/cli/commands/activate.go` and `uninstall.go`, and the control-loop check of `crucible/internal/cli/converters/controlloop.go` (02:459, ADR-090 *Loose ends*); the `cursor` converter is not carried. Each installer renders the bundle into an in-memory file map (pure, golden-tree tested against frozen crucible's converter output), and one shared writer applies that map to the folder under a `Write` lease, recording every file's before and after digest so a later update or removal can restore byte for byte. The install record is a JSON file in the instance's `meta/` folder, outside the agent folder: the crate has no Journal dependency (2B-3), and a session may write inside its own agent folder but not into `meta/`. Rejected: (1) crucible's signature-based verify — ordin has no bundle signature (ADR-084, R-71), so verify compares SHA-256 digests of every written file; (2) writing `settings.local.json` and `.mcp.json` — ignored under `--restricted --strict-mcp-config` (C-26, C-61) and superseded by the Rules Engine and the MCP Front.

### Reconciliation assumptions

- **SA-RECON-01** The harness is the 2C `HarnessKind { ClaudeCode, CopilotCli }` everywhere in this crate, including `InstallRequest.harness`; no harness string is accepted (reconciliation row 105; ruling R7).
- **SA-RECON-02** The install record is the single source of the launch paths (`HarnessLaunch`), the tool policy, `reference_only_parts` and `needs_person_present`; consumers read `record(id)` and do not re-derive them (reconciliation rows 106–108, O15, O140; ruling R6).
- **SA-RECON-03** The Package Catalogue is the only caller of `install`, `update`, `update_in_session`, `uninstall`, `verify` and `verify_and_repair`, with the signatures in *Public Interface* (reconciliation rows 101–104; ruling R6).
- **SA-RECON-04** The Release Builder's `HarnessLayoutWriter` is served by `render_claude_session_form` and `write_tree` (reconciliation N19, O14).
- **SA-RECON-05** The `ordin-installers → ordin-agent-model` edge is in 2B-3 (amended January 2026, decision 80); the Agent Model types below are imported, not copied into another crate.

## Shared Context

### Types from the Resolver and Compiler (`ordin-resolver`)

```rust
pub struct Bundle { pub plan: ResolutionPlan, pub manifest: BundleManifest, pub lock_file: LockFile,
                    pub lock_file_bytes: Vec<u8>, pub dir: PathBuf }
impl Bundle { pub fn part_dir(&self, part: &PartRef) -> PathBuf;
              pub fn read_part_files(&self, part: &PartRef) -> Result<PartFiles, ResolveError>; }
impl ResolverAndCompiler {
    pub fn open_bundle(&self, plan_hash: &str) -> Result<Bundle, ResolveError>;
    pub async fn rebuild_from_lock(&self, lock: &LockFile, scope: &SourceScope) -> Result<Bundle, ResolveError>;
}
pub enum ResolveError { /* RES_* codes; retryable() */ }
```

### Types from the Agent Model (`ordin-agent-model`)

```rust
pub enum PartKind { /* 25 kinds, crucible spelling */ }
pub enum LoadingMode { AlwaysOn, OnDemand, Event, ModeSwitch, RegisteredInvocation, Structural, None }
pub struct PartFiles(pub BTreeMap<String, Vec<u8>>);
pub struct PartRef { pub kind: PartKind, pub id: String, pub name: String, pub version: String,
    pub content_hash: String, pub repository: String, pub commit: String, pub loading: Option<LoadingMode> }
pub struct BundleArtefact { pub part: PartRef, pub loading: LoadingMode }
pub struct BundleManifest { pub bundle_schema: String, pub plan_hash: String, pub composition: PartRef,
    pub artefacts: Vec<BundleArtefact>, pub overlays_applied: Vec<PartRef>,
    pub resolved_tool_policy: ResolvedToolPolicy, pub lockfile_sha256: String, pub compiler_version: String }
pub struct ResolvedToolPolicy { pub forbidden_flags: Vec<String>, pub coverage_floor: Option<DecimalString>,
    pub prohibited_verbs: Vec<String>, pub requires_human_approval: Vec<String>, pub mcp_servers: Vec<McpServerDecl> }
pub struct LockFile { /* lockfile_schema, plan_hash, target_harness, composition, artefacts, overlays_applied, selections */ }
pub struct ControlLoopSpec { pub strategy: String, pub trigger: String, pub requires: Vec<String>, pub modes: Vec<String> }
pub struct MemoryContract { pub retains: Vec<String>, pub excludes: Vec<String>, pub max_age_days: Option<u32>,
    pub unbounded: bool, pub store_location: String, pub memory_service: Option<String> }
pub struct SourceScope { /* home, profile, include_bundled */ }
pub struct Urn { pub publisher: String, pub kind_dir: String, pub categories: Vec<String>, pub slug: String }
pub fn parse_manifest(path: &str, bytes: &[u8]) -> Result<Option<Manifest>, AmError>;  // control_loop(), memory_contract()
pub fn markdown_body(path: &str, bytes: &[u8]) -> Option<String>;
pub const LOCK_FILE_NAME: &str = "ordin.lock";
pub const BUNDLE_MANIFEST_NAME: &str = "bundle.manifest.json";
```

### Agent Folder Manager (`ordin-agent-folders`)

```rust
pub enum LockMode { Session, Write, SessionUpdate }
pub struct FolderLease;   // folder(), meta(); drop releases
impl AgentFolderManager {
    pub async fn lock(&self, id: AgentInstanceId, mode: LockMode) -> Result<FolderLease, AfmError>;
}
pub enum AfmError { /* AFM_FOLDER_NOT_FOUND, AFM_FOLDER_BUSY (retryable), AFM_IO_FAILED … */ }
```

### Phase 2C and HLD facts

- `AgentInstanceId` — UUIDv7 newtype (2C). `Timestamp` — RFC 3339, ms, `Z` (2C).
- `HarnessKind` — 2C `ordin_types::domain::identity`, defined once there (amended January 2026, decision 80: C-1):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessKind { ClaudeCode, CopilotCli }
```
- Claude Code under `--restricted --strict-mcp-config` loads nothing from the agent folder; `--append-system-prompt-file <file>` with `CLAUDE.md`'s `@` imports inlined loads the instructions; `--plugin-dir <dir>` with skills and commands laid out as a plugin loads them, named `<plugin>:<name>`; an invoked skill's body loads; sub-agents not covered (10 C-61 result, January 2026, Claude Code 2.1.304).
- crucible control-loop support per target: `claude-code` strategies `goal-evaluated`, `plan-then-execute`, `reactive`, `tool-iterative`, capabilities `bounded-iteration`, `completion-evaluator` (`crucible/internal/cli/converters/claudecode/convert.go:51-56`); `vscode` and `agents-md` strategies `reactive`, `tool-iterative`, no capabilities (`vscode/convert.go:42-44`, `agentsmd/convert.go:42-44`). Loop triggers `human-pulled`, `autonomous` (`crucible/pkg/types/controlloop.go:102-105`).

### Confirmation items bound to this component

- **C-60** — Copilot loads what the vscode or agents-md Installer writes. This spec maps `HarnessKind::CopilotCli` to the **vscode** installer. If C-60 finds Copilot does not load the vscode layout but loads the agents-md one, the map entry changes to agents-md (one constant); if it loads neither, `COPILOT_SUPPORTED` becomes false and `install` for `CopilotCli` returns `INST_HARNESS_UNSUPPORTED` (the HLD's fallback: Claude Code first).
- **C-01** — the Copilot CLI half (loads files from the session's folder); same fallback as C-60.
- **C-64** — a pack changed mid-session takes effect from the next turn, else the session is resumed. Because Claude Code reads `instructions.md` and `plugin/` only at start (C-61), `update_in_session` reports `restart_required = true` for Claude Code whenever either changed, and `false` otherwise; for Copilot it reports `true` until C-64 confirms otherwise.

### Configuration

No environment variable or profile key. Constants (*chosen here*): `COPILOT_SUPPORTED = true` until C-60/C-01 settle otherwise; install record file `meta/install-record.json`; backups `meta/backups/<UTC yyyymmddThhmmssZ>/`; bundle copy `meta/bundle/` (manifest and lock file only); `@`-import inlining depth 5 (*assumed here*: Claude Code's documented import depth); maximum installed files 20,000.

## Public Interface

Crate `ordin_installers`. Names are *chosen here* unless traced.

```rust
use std::{path::PathBuf, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstallerKind { ClaudeCode, Vscode, AgentsMd }

/// `ClaudeCode` → ClaudeCode; `CopilotCli` → Vscode (C-60), or INST_HARNESS_UNSUPPORTED while
/// the constant `COPILOT_SUPPORTED` is false (C-60/C-01 rule Copilot out).
pub fn installer_for(harness: HarnessKind) -> Result<InstallerKind, InstallError>;

/// Called by: Package Catalogue (*Add an agent*, *Change*, update acceptance, trial), which builds it
/// from the agent instance, the harness the person's profile selects, and `Bundle.plan.plan_hash`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InstallRequest {
    pub agent_instance_id: AgentInstanceId,
    pub harness: HarnessKind,
    /// `sha256:<hex>` of a bundle the Resolver and Compiler built.
    pub plan_hash: String,
}

/// The launch paths of an installed agent. Read from `record(id).launch` by the program's
/// `SessionSetupSource` and handed to the Claude Local Adapter (`--append-system-prompt-file`,
/// `--plugin-dir`, the plugin name for `<plugin>:<command>`, R-94) and the Copilot Local Adapter
/// (working folder). The paths are always `<folder>/instructions.md` and `<folder>/plugin`; the
/// adapters take them from here rather than composing them.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "harness", rename_all = "snake_case")]
pub enum HarnessLaunch {
    ClaudeCode {
        working_dir: PathBuf,
        /// `<folder>/instructions.md`, for `--append-system-prompt-file`.
        append_system_prompt_file: PathBuf,
        /// `<folder>/plugin`, for `--plugin-dir`.
        plugin_dir: PathBuf,
        plugin_name: String,
    },
    Copilot { working_dir: PathBuf },
}

/// How each of the agent's commands is named in a session (R-94).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CommandName { pub command_id: String, pub session_name: String }

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileAction { Created, Replaced, Unchanged }

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InstalledFile {
    /// '/'-separated path relative to the agent folder.
    pub path: String,
    pub sha256: String,              // 64 hex
    pub size: u64,
    pub action: FileAction,
    /// Digest of the file that was there before this install wrote it, when one was.
    pub pre_sha256: Option<String>,
    /// Path relative to `meta/` of the backup of that file.
    pub backup: Option<String>,
}

/// The install record (04:50 revision 0.48.0), `meta/install-record.json`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InstallRecord {
    /// "ordin.install/v1".
    pub record_schema: String,
    pub agent_instance_id: AgentInstanceId,
    pub installer: InstallerKind,
    pub harness: HarnessKind,
    pub plan_hash: String,
    pub previous_plan_hash: Option<String>,
    /// SHA-256 hex of the lock-file bytes written into the folder.
    pub lock_file_sha256: String,
    /// The resolved tool policy, for the Rules Engine (ADR-090 item 1). Read by the Package
    /// Catalogue through `record(id)` and converted to the Rules Engine's `AgentToolPolicy`.
    pub tool_policy: ResolvedToolPolicy,
    pub launch: HarnessLaunch,
    pub commands: Vec<CommandName>,
    /// True when the agent's control loop is `human-pulled` (ADR-090 *Loose ends*: never given
    /// an unattended step). Read by the Package Catalogue through `record(id)` and passed to the
    /// Package Inspector (`InspectRequest.install.needs_person_present`, an `InstallFacts` field), which carries it into the parsed
    /// package model and the agent directory.
    pub needs_person_present: bool,
    /// Identifiers of the parts the installer could place only as reference files (crucible's
    /// fallback placement). Read the same way into `InspectRequest.install.reference_only_parts`; consumers
    /// read this list rather than re-deriving it from part kinds.
    pub reference_only_parts: Vec<String>,
    pub files: Vec<InstalledFile>,
    pub installed_at: Timestamp,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct InstallOutcome {
    pub record: InstallRecord,
    /// For `update_in_session`: the running session must be resumed to see the change (C-64).
    pub restart_required: bool,
    /// True when `verify_and_repair` rebuilt the folder.
    pub repaired: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum VerifyOutcome {
    Verified,
    /// Files whose digest differs from the record, files missing, or the lock file changed.
    Mismatch { changed: Vec<String>, missing: Vec<String> },
}

pub struct Installers { /* private: Arc<AgentFolderManager>, Arc<ResolverAndCompiler> */ }

impl Installers {
    pub fn new(folders: Arc<AgentFolderManager>, resolver: Arc<ResolverAndCompiler>) -> Self;
    /// First install into an existing, empty agent folder (created by the caller, 03 *Add an agent*).
    pub async fn install(&self, request: &InstallRequest) -> Result<InstallOutcome, InstallError>;
    /// Replace the installed bundle with another between runs (Write lease).
    pub async fn update(&self, request: &InstallRequest) -> Result<InstallOutcome, InstallError>;
    /// Pack-choice rebuild while a session runs (SessionUpdate lease, ADR-089 decision 64, C-64).
    pub async fn update_in_session(&self, request: &InstallRequest) -> Result<InstallOutcome, InstallError>;
    /// Compare the folder with the install record and the lock file (SA-INST-01).
    pub async fn verify(&self, id: AgentInstanceId) -> Result<VerifyOutcome, InstallError>;
    /// Verify; on a mismatch rebuild from the lock file without asking (SA-INST-01).
    pub async fn verify_and_repair(&self, id: AgentInstanceId, scope: &SourceScope)
        -> Result<InstallOutcome, InstallError>;
    /// Restore the folder to its state before the first install (Write lease).
    pub async fn uninstall(&self, id: AgentInstanceId) -> Result<(), InstallError>;
    pub async fn record(&self, id: AgentInstanceId) -> Result<InstallRecord, InstallError>;
    /// Pure render, for golden-tree tests (no folder touched).
    pub fn render(&self, kind: InstallerKind, bundle: &Bundle) -> Result<RenderedTree, InstallError>;
}

/// Input of the Claude Code session form for a release bundle (Release Builder P-9, C-61 (b)).
#[derive(Debug, Clone)]
pub struct ClaudeSessionFormInput {
    /// The package name; the plugin name is derived from it as in step 3.
    pub agent_name: String,
    /// The package version, for `plugin.json`.
    pub version: String,
    /// The package description, for `plugin.json`; empty when the package has none.
    pub description: String,
    /// The compiled always-on instructions (`AGENTS.md` text); `@` imports are inlined against
    /// `bundle_dir` as in step 3.
    pub instructions: String,
    /// Skill slug → the skill folder's files ('/'-separated relative paths → bytes).
    pub skills: std::collections::BTreeMap<String, std::collections::BTreeMap<String, Vec<u8>>>,
    /// Command slug → the command file's Markdown bytes.
    pub commands: std::collections::BTreeMap<String, Vec<u8>>,
}

/// Pure: renders `instructions.md` and `plugin/` for a release bundle, with paths relative to the
/// target folder (`instructions.md`, `plugin/.claude-plugin/plugin.json`, `plugin/skills/<slug>/…`,
/// `plugin/commands/<slug>.md`). The same code as step 3's `instructions.md` and `plugin/` rules.
/// Called by: the server program's `HarnessLayoutWriter` for the Release Builder (P-9).
pub fn render_claude_session_form(input: &ClaudeSessionFormInput, bundle_dir: &std::path::Path)
    -> Result<RenderedTree, InstallError>;

/// Writes a rendered tree into `target_dir` with step 11's atomic file writes; refuses a target
/// that already holds any of the tree's paths (`INST_FOLDER_NOT_EMPTY`). Takes no lease: the caller
/// owns `target_dir`. Called by: the server program's `HarnessLayoutWriter`, with
/// `target_dir = <bundle>/.ordin/claude`.
pub async fn write_tree(target_dir: &std::path::Path, tree: &RenderedTree) -> Result<(), InstallError>;

#[derive(Debug, Clone, Default)]
pub struct RenderedTree {
    pub files: std::collections::BTreeMap<String, Vec<u8>>,
    pub launch_paths: Option<(String, String, String)>,   // (instructions.md, plugin dir, plugin name)
    pub commands: Vec<CommandName>,
    pub reference_only_parts: Vec<String>,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum InstallError {
    #[error("INST_HARNESS_UNSUPPORTED")] HarnessUnsupported { harness: HarnessKind },
    #[error("INST_FOLDER_UNAVAILABLE")] FolderUnavailable { code: &'static str, retryable: bool },
    #[error("INST_FOLDER_NOT_EMPTY")] FolderNotEmpty,
    #[error("INST_NOT_INSTALLED")] NotInstalled,
    #[error("INST_BUNDLE_UNAVAILABLE")] BundleUnavailable { code: String, retryable: bool },
    #[error("INST_LOOP_UNSUPPORTED")] LoopUnsupported { strategy: String, missing_capability: Option<String> },
    #[error("INST_IMPORT_CYCLE")] ImportCycle { file: String },
    #[error("INST_PATH_INVALID")] PathInvalid { path: String },
    #[error("INST_WRITE_FAILED")] WriteFailed { retryable: bool },
    #[error("INST_RECORD_INVALID")] RecordInvalid,
    #[error("INST_REPAIR_FAILED")] RepairFailed { code: String },
}
impl InstallError { pub fn retryable(&self) -> bool; }
```

### Who calls each public item

| Item | Called by |
|---|---|
| `Installers::new` | Ordin Desktop and Ordin Server at start (OrdinDesktop row 3.18, OrdinServer row 5.13) |
| `install` | Package Catalogue *Add an agent*, after `AgentFolderManager::create`, with an `InstallRequest`; keeps the returned `InstallOutcome.record` |
| `update` | Package Catalogue (*Change* and accepted updates) when the instance has no running session |
| `update_in_session` | Package Catalogue *Change* (ADR-089 decision 64, C-64) when the instance has a running session; `restart_required` decides whether the session is resumed |
| `verify` | Package Catalogue, when the person opens the agent's page (SA-INST-01) |
| `verify_and_repair` | Package Catalogue `ensure_ready` before each session (SA-INST-01); success with `repaired = true` is journalled by the caller as a repair; `Err(INST_REPAIR_FAILED)` makes the agent unavailable |
| `uninstall` | Package Catalogue *Removal*, before `AgentFolderManager::remove` |
| `record` | Package Catalogue (tool policy for the Rules Engine; `reference_only_parts` and `needs_person_present` for the Package Inspector); the program's `SessionSetupSource` (`launch`, for the Claude and Copilot Local Adapters) |
| `render` | Golden-tree tests of this crate |
| `render_claude_session_form`, `write_tree` | The server program's `HarnessLayoutWriter` (Release Builder P-9) |

### Example

Request `InstallRequest { agent_instance_id: 0192a0f9-…, harness: HarnessKind::ClaudeCode, plan_hash: "sha256:4d1c…0c1d" }` for cspa-431. The agent folder after `install`:

```text
ordin.lock
CLAUDE.md                                    crucible bootstrap (header "Installed by ordin.")
instructions.md                              CLAUDE.md with @ imports inlined
.claude/rules/system-role.md
.claude/rules/orchestrators/working-modes.md
.claude/rules/modes/observe-mode.md          … one per allowed operating mode
.claude/rules/control-loops/cspa.md
.claude/rules/overlays/language/en-GB/…
.claude/skills/quality-codegen/SKILL.md      … one directory per skill
.claude/commands/apply.md                    … one file per command
.claude/context/cspa-glossary.md
plugin/.claude-plugin/plugin.json            {"name":"cspa-431","version":"1.0.0","description":"…"}
plugin/skills/quality-codegen/SKILL.md
plugin/commands/apply.md
```

`record.launch = ClaudeCode { working_dir: ".../folder", append_system_prompt_file: ".../folder/instructions.md", plugin_dir: ".../folder/plugin", plugin_name: "cspa-431" }`; `record.commands` holds `{command_id: "urn:ordin:default:commands:lifecycle:apply", session_name: "cspa-431:apply"}`; `needs_person_present = true` (cspa's loop is `human-pulled`).

## Internal Logic

**Render (pure; `render(kind, bundle)`).**

1. **Read the bundle's parts.** For each `BundleArtefact` in `bundle.manifest.artefacts`: `bundle.read_part_files(part)` (errors → `INST_BUNDLE_UNAVAILABLE { code }`); parse the part's primary manifest file with `parse_manifest` to read `control_loop()` and `memory_contract()` where the kind needs them. Every file name is checked: absolute, `..`, `\` or NUL → `INST_PATH_INVALID`.
2. **Control-loop check** (port of `CheckLoopSupport`, `controlloop.go:67-84`): when a ControlLoop part is present, its `strategy` must be in the installer's strategy list and each `requires` entry in its capability list; otherwise `INST_LOOP_UNSUPPORTED { strategy, missing_capability }`. `needs_person_present = trigger == "human-pulled"`.
3. **Claude Code layout** (port of `crucible/internal/cli/converters/claudecode/native.go`, `claudecode.go`), paths relative to the agent folder (crucible's `client/claude-code/` prefix dropped):
   - Skill → `.claude/skills/<slug>/` (every file); Command → `.claude/commands/<slug>.md`; Rule and OperatingMode → `.claude/rules/<categories…>/<slug>.md`; ControlLoop → `.claude/rules/<categories…>/<slug>.md`; ContextResource → `.claude/context/<categories…>/<slug>.md`; overlays → `.claude/rules/overlays/<categories…>/<slug>/`; SystemRole → `.claude/rules/system-role.md`; Memory → no file, a policy block in `CLAUDE.md` under `## Memory policy`; every other kind → `.claude/ordin/<Kind>-<name>/` (crucible's fallback directory, renamed from `crucible`), listed in `reference_only_parts`. `<slug>` and `<categories…>` come from `Urn::parse(part.id)`; a part whose identifier does not parse takes the fallback placement.
   - `CLAUDE.md` bootstrap: crucible's `bootstrapData.render()` order and text — header line `# ordin agent — <name>@<version>`, then "Installed by ordin." with crucible's sentence on auto-discovery, then sections `## System role`, `## Operating modes & rules`, `## Control loop` (the derived graph, port of `DeriveLoopGraph`), `## Overlays`, `## Context resources`, `## Memory policy`, `## Bundled artefacts (reference; not auto-loaded)`, each with `@<path>` import lines sorted as crucible sorts them. The two word changes (header prefix and "ordin" for "Crucible") are the only intended differences from frozen crucible's golden tree.
   - Not written: `.claude/settings.local.json`, `.mcp.json`, `.claude/crucible.policy.md` (ADR-090 item 1; C-26; C-61: ignored under `--restricted --strict-mcp-config`).
   - `instructions.md` (C-61): start from the rendered `CLAUDE.md` text; scan line by line, skipping lines inside fenced code blocks (``` or ~~~); a line whose trimmed text matches `^@([^\s]+)$` is replaced by the referenced file's text from the rendered map, path resolved relative to the importing file's directory, with a leading front-matter block (`---` … `---`) removed and the same scan applied recursively; depth above 5 or a file met again on the current import path → `INST_IMPORT_CYCLE { file }`; a path absent from the map leaves the line unchanged and logs `installers.import_missing`. Output LF, one trailing newline.
   - `plugin/` (C-61): `plugin/.claude-plugin/plugin.json` = canonical JSON `{"description": <root metadata.description, or "">, "name": <plugin name>, "version": <root version>}` (*assumed here*: Claude Code's plugin manifest location and keys, the layout the C-61 spike loaded); a copy of each `.claude/skills/<slug>/` under `plugin/skills/<slug>/`; a copy of each `.claude/commands/<slug>.md` under `plugin/commands/<slug>.md`. No sub-agents are written: no ordin kind installs as a Claude Code sub-agent in the first release, and C-61 did not cover them (*assumed here*). Plugin name = the root's slug, lowercased, each run outside `[a-z0-9-]` replaced by `-`, trimmed, at most 64 characters. `commands` = for each Command part `{command_id: part.id, session_name: "<plugin name>:<slug>"}` (R-94).
4. **Copilot (vscode) layout** (port of `crucible/internal/cli/converters/vscode`): always-on behavioural parts → `.github/instructions/<slug>.instructions.md`; Command → `.github/prompts/<slug>.prompt.md`; ContextResource → `.github/context/<slug>.md`; remaining kinds → `agents/<Kind>-<name>/` (listed in `reference_only_parts`); `.github/copilot-instructions.md` index; `AGENTS.md`; `.vscode/settings.json` (crucible's AGENTS.md toggle). Not written: `.vscode/mcp.json` (*assumed here*: the agent's MCP servers start only through the trust policy, ADR-090 item 1). `commands` = `{command_id, session_name: <slug>}`. Launch = `Copilot { working_dir }`.
5. **agents-md layout** (port of `crucible/internal/cli/converters/agentsmd`): `AGENTS.md` embedding behavioural parts, `agents/context/<slug>.md`, `agents/<Kind>-<name>/` fallback; `mcp.json` not written (as step 4). Used only if C-60 selects it.
6. **Lock file.** Every layout adds `ordin.lock` = `bundle.lock_file_bytes` at the folder root (shown first under **Show the installed files**, ADR-093 item 3).
7. The rendered map over 20,000 files → `INST_WRITE_FAILED { retryable: false }`.

**Write (`install`, `update`, `update_in_session`).**

8. **Prepare.** `installer_for(request.harness)`; `resolver.open_bundle(plan_hash)` (not found or corrupt → `INST_BUNDLE_UNAVAILABLE { code, retryable: false }`); `render`. Take the lease: `Write` for `install` and `update`, `SessionUpdate` for `update_in_session` (`AFM_FOLDER_BUSY` → `INST_FOLDER_UNAVAILABLE { code, retryable: true }`; not found → `INST_FOLDER_UNAVAILABLE { retryable: false }`).
9. **Install.** The agent folder must be empty (`INST_FOLDER_NOT_EMPTY` otherwise; the record file must not exist). Write every file of the map (step 11). Record each as `Created`.
10. **Update.** Read the current record (missing → `INST_NOT_INSTALLED`; unparseable → `INST_RECORD_INVALID`). Three-way per path, port of `activatePlainFile`:
    - in new map and old record, disk digest = old `sha256`: write when the bytes differ (`Replaced`), else `Unchanged`;
    - in new map and old record, disk digest ≠ old `sha256` (the session or the person changed it): back up the disk file to `meta/backups/<stamp>/<path>`, then write (`Replaced`, `pre_sha256` = disk digest, `backup` set);
    - in new map, not in old record: when a file exists on disk (written by a session), back it up and write (`Replaced`); else `Created`;
    - in old record, not in new map: when the disk digest equals the recorded one, delete it; when it differs, move it to `meta/backups/<stamp>/<path>` and log `installers.kept_changed_file`; carry forward the earliest `pre_sha256`/`backup` of a path so uninstall can restore the state before the first install.
    Files the record never named (artefacts and memory pages a session wrote, 04:394–395) are never touched.
11. **Atomic file writes.** Each file: create parent directories (`0700`), write to `<dir>/.ordin-tmp-<UUIDv7>`, `fsync`, rename over the target; never follow a symbolic link at the target (an existing link is removed first, its target untouched). I/O failure → `INST_WRITE_FAILED { retryable: true }`; on failure of a first install, every file written so far is deleted and the folder is left empty, so the Package Catalogue can remove it and retry once (03 *Add an agent*, error posture).
12. **Record.** Copy `bundle.manifest.json` and `ordin.lock` to `meta/bundle/`; write `meta/install-record.json` atomically (canonical JSON of `InstallRecord`, `record_schema = "ordin.install/v1"`, `previous_plan_hash` = the old record's plan hash on update, `tool_policy` = `bundle.manifest.resolved_tool_policy`, `lock_file_sha256` = SHA-256 of the lock bytes).
13. **Restart flag** (`update_in_session` only): `restart_required` = for ClaudeCode, whether `instructions.md` or any file under `plugin/` changed; for Copilot, `true` while C-64 is unconfirmed.
14. Log `installers.installed` / `installers.updated`. Return the outcome. The caller journals the install (04:50; the Package Catalogue records install, lock file, check results and trust decisions, 03 *Add an agent*).

**Release-bundle session form (`render_claude_session_form`, `write_tree`; Release Builder P-9).**

14a. **Render.** Every skill and command path is checked as in step 1 (`INST_PATH_INVALID`). Plugin name = `agent_name` normalised as in step 3. `instructions.md` = `input.instructions` with step 3's `@`-import inlining, imports resolved against the files under `bundle_dir` (read-only; a missing target leaves the line unchanged and logs `installers.import_missing`; depth above 5 or a cycle → `INST_IMPORT_CYCLE`). `plugin/.claude-plugin/plugin.json` = canonical JSON `{"description", "name", "version"}` from the input; `plugin/skills/<slug>/<path>` for each skill file; `plugin/commands/<slug>.md` for each command. `commands` = `{command_id: <slug>, session_name: "<plugin name>:<slug>"}`; `reference_only_parts` empty. A tree over 20,000 files → `INST_WRITE_FAILED { retryable: false }`.
14b. **Write.** Create `target_dir` (`0700`) when missing; any tree path that already exists under it → `INST_FOLDER_NOT_EMPTY`; write each file with step 11's atomic write (I/O failure → `INST_WRITE_FAILED { retryable: true }`, files written so far deleted). No record, no lease, no backup: the release bundle is built in the Release Builder's temporary folder.

**Verify and repair (SA-INST-01).**

15. **Verify.** Take a `Session` lease for the duration of the check (busy → `INST_FOLDER_UNAVAILABLE { retryable: true }`). Read the record (missing → `INST_NOT_INSTALLED`). For each recorded file: missing → `missing`; digest differs → `changed`. `ordin.lock`'s digest must equal `lock_file_sha256` (else `changed`). Return `Verified` or `Mismatch`. Read-only.
16. **Repair.** On `Mismatch`: take a `Write` lease; parse `meta/bundle/ordin.lock` (the copy outside the folder; unparseable → `INST_REPAIR_FAILED { code: "AM_LOCK_FILE_INVALID" }`); open the bundle by its plan hash, and when it is gone or corrupt call `resolver.rebuild_from_lock(lock, scope)`; failure → `INST_REPAIR_FAILED { code: <RES_* code> }` — the caller shows the agent as unavailable with **Send to IT** (SA-INST-01). Re-render and rewrite every recorded file whose digest differs or is missing (`Replaced`), leaving unrecorded files alone; write the record; `repaired = true`. Log `installers.repaired` with the counts. The caller journals the repair (SA-INST-01 "records it in the journal").

**Uninstall.**

17. Take a `Write` lease; read the record (missing → `INST_NOT_INSTALLED`). For each recorded file: when a `backup` exists, restore it byte for byte to its path (`pre_sha256` must match the backup's digest, else log `installers.backup_mismatch` and restore anyway); otherwise delete the file. Remove directories the record's files created that are now empty. Delete `meta/install-record.json`, `meta/bundle/` and `meta/backups/`. Port of `crucible/internal/cli/commands/uninstall.go`'s byte restore. The Agent Folder Manager then removes the folder (Package Catalogue *Removal*).

## Data Model

No table: the crate has no Journal dependency (2B-3). The install record is a file in the instance's `meta/` folder (outside the agent folder, created by the Agent Folder Manager):

```text
<data root>/agents/<agent instance id>/meta/
  install-record.json        InstallRecord, canonical JSON, schema "ordin.install/v1"
  bundle/bundle.manifest.json
  bundle/ordin.lock          the copy repair trusts (the folder's copy may be altered by a session)
  backups/<stamp>/<path>     files replaced or kept by an update, for byte restore on uninstall
```

The Package Catalogue's `agent_instances` row keeps `lock_file_path` and `agent_folder_path` (2C §AgentInstance); the install record is read through `record()`. Rollback: an update keeps the previous record's plan hash; re-running `update` with that plan hash restores the earlier install while its bundle exists.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| C-60/C-01 rule out Copilot (`COPILOT_SUPPORTED = false`) and the harness is `CopilotCli` | `Err(permanent)` | `INST_HARNESS_UNSUPPORTED` | `InstallError::HarnessUnsupported { harness }` |
| `write_tree` target already holds a path of the tree | `Err(permanent)` | `INST_FOLDER_NOT_EMPTY` | `InstallError::FolderNotEmpty` |
| Agent folder missing | `Err(permanent)` | `INST_FOLDER_UNAVAILABLE` | `InstallError::FolderUnavailable { code: "AFM_FOLDER_NOT_FOUND", retryable: false }` |
| Agent folder leased by a session or another write | `Err(retryable)` | `INST_FOLDER_UNAVAILABLE` | `InstallError::FolderUnavailable { code: "AFM_FOLDER_BUSY", retryable: true }` |
| First install into a non-empty folder | `Err(permanent)` | `INST_FOLDER_NOT_EMPTY` | `InstallError::FolderNotEmpty` |
| Update, verify, repair or uninstall with no install record | `Err(permanent)` | `INST_NOT_INSTALLED` | `InstallError::NotInstalled` |
| Bundle missing or corrupt | `Err(permanent)` | `INST_BUNDLE_UNAVAILABLE` | `InstallError::BundleUnavailable { code, retryable: false }` |
| Loop strategy or capability not supported by the installer | `Err(permanent)` | `INST_LOOP_UNSUPPORTED` | `InstallError::LoopUnsupported { strategy, missing_capability }` |
| `@` import deeper than 5 or circular | `Err(permanent)` | `INST_IMPORT_CYCLE` | `InstallError::ImportCycle { file }` |
| A part file name is absolute, has `..`, `\` or NUL | `Err(permanent)` | `INST_PATH_INVALID` | `InstallError::PathInvalid { path }` |
| File write, rename or delete fails | `Err(retryable)` | `INST_WRITE_FAILED` | `InstallError::WriteFailed { retryable: true }` |
| Rendered tree over 20,000 files | `Err(permanent)` | `INST_WRITE_FAILED` | `InstallError::WriteFailed { retryable: false }` |
| `install-record.json` does not parse or has another schema | `Err(permanent)` | `INST_RECORD_INVALID` | `InstallError::RecordInvalid` |
| Repair cannot rebuild from the lock file | `Err(permanent)` | `INST_REPAIR_FAILED` | `InstallError::RepairFailed { code }` |

The person never sees these codes. The words the caller shows: a failed repair makes the agent unavailable with **Send to IT** (SA-INST-01); an interrupted install is retried once and then "This agent could not be added safely" with **Send to IT** (11:209, 11:1046).

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Installers

  Scenario: Happy path — Claude Code install with the C-61 additions
    Given a bundle for cspa-431 and an empty agent folder
    When install is called for harness ClaudeCode
    Then the folder holds ordin.lock, CLAUDE.md, instructions.md and plugin/.claude-plugin/plugin.json with name "cspa-431"
    And instructions.md contains the text of ".claude/rules/system-role.md" in place of its "@" line
    And the record's launch names instructions.md and the plugin directory
    And the command "urn:ordin:default:commands:lifecycle:apply" is named "cspa-431:apply"
    And no ".claude/settings.local.json" or ".mcp.json" is written

  Scenario: Parity — layout equals frozen crucible's converter apart from the named differences
    Given each WorkspaceProfile of the bundled catalogue built by ordin and by frozen crucible
    When the Claude Code tree is rendered
    Then every file equals frozen crucible's "client/claude-code/" tree except the header line, the word "ordin", the fallback directory name, the three files not written, and the added instructions.md and plugin/

  Scenario: Edge — update keeps what the session wrote
    Given an installed agent whose session wrote "results/q3.xlsx" and changed ".claude/commands/apply.md"
    When update is called with a newer bundle
    Then "results/q3.xlsx" is unchanged
    And the changed "apply.md" is backed up under meta/backups before it is replaced

  Scenario: Edge — uninstall restores byte for byte
    Given an agent installed into a folder that held "notes.md" before the install, overwritten by an update
    When uninstall is called
    Then "notes.md" holds its original bytes and every other installed file is gone

  Scenario: Edge — pack chosen mid-session
    Given a running Claude Code session on the agent
    When update_in_session adds the testing pack's files
    Then the outcome's restart_required is true because instructions.md changed

  Scenario: Edge — verify finds a changed file and repairs it
    Given an installed agent whose ".claude/rules/system-role.md" was edited on disk
    When verify_and_repair is called
    Then the file is rewritten from the bundle and repaired is true

  Scenario: Error — loop the harness cannot run
    Given an agent whose control loop has strategy "plan-then-execute"
    When install is called for harness CopilotCli
    Then it fails with INST_LOOP_UNSUPPORTED naming "plan-then-execute"

  Scenario: Error — install while a session runs
    Given a session holds the agent folder
    When update is called
    Then it fails with INST_FOLDER_UNAVAILABLE and retryable is true

  Scenario: Error — repair impossible
    Given an installed agent with a changed file whose bundle is gone and whose lock-file parts are no longer in the clone
    When verify_and_repair is called
    Then it fails with INST_REPAIR_FAILED with code "RES_LOCK_PART_UNAVAILABLE"

  Scenario: Error — circular import
    Given a rule file that imports itself with "@.claude/rules/a.md"
    When render is called for Claude Code
    Then it fails with INST_IMPORT_CYCLE

  Scenario: Error — Copilot ruled out
    Given COPILOT_SUPPORTED is false
    When installer_for is called with CopilotCli
    Then it fails with INST_HARNESS_UNSUPPORTED

  Scenario: Happy path — session form for a release bundle
    Given a ClaudeSessionFormInput for "support-agent" version "2026.10.1" with skill "triage" and command "summarise"
    When render_claude_session_form is called and write_tree writes the tree to "<bundle>/.ordin/claude"
    Then "<bundle>/.ordin/claude/instructions.md" holds the instructions with "@" imports inlined
    And "<bundle>/.ordin/claude/plugin/.claude-plugin/plugin.json" names "support-agent"
    And the command "summarise" is named "support-agent:summarise"

  Scenario: Edge — the program reads launch paths from the record
    Given an installed Claude Code agent "cspa-431"
    When record is called for the instance
    Then launch.append_system_prompt_file is "<folder>/instructions.md" and launch.plugin_name is "cspa-431"
```

## Performance, Security, Observability

- **Performance targets:** `render` of a 200-part bundle ≤ 300 ms p95; `install` of 1,000 files (10 MB) ≤ 3 s p95 on SSD; `verify` of 1,000 files ≤ 400 ms p95 (one SHA-256 pass, SA-INST-01's per-session cost); `update` with 10% of files changed ≤ 1.5 s p95.
- **Security:** part files are written as data, never run; every path is checked and confined to the agent folder; links are never followed when writing or deleting; the install record and the trusted lock-file copy live in `meta/`, which sessions cannot reach (Agent Folder Manager); no credential is written into the folder (SA-CRED-01; no `.mcp.json` or `env` values); the tool policy is never written for the harness to read (ADR-090 item 1).
- **Observability:** `tracing` events (SA-OBS-01), no file content; paths are agent-folder-relative only:
  - `installers.installed`, `installers.updated` (info): `agent_instance_id`, `installer`, `plan_hash`, `created`, `replaced`, `unchanged`, `removed`, `backed_up`, `duration_ms`.
  - `installers.verified` (debug) / `installers.mismatch` (warn): `agent_instance_id`, `changed`, `missing`.
  - `installers.repaired` (warn) / `installers.repair_failed` (error): `agent_instance_id`, `error_code`.
  - `installers.import_missing`, `installers.kept_changed_file`, `installers.backup_mismatch` (warn): `agent_instance_id`, `path`.
  No metrics or spans (SA-XC-04, SA-XC-05).

## Gaps

None.
