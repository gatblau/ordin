# SPEC: Release Builder

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-release-builder` · **Build phase:** 7 · **Programs:** A
> **Dependencies:** Agent Configuration, Checker, Resolver and Compiler, Agent Loader

## Table of contents

- [Purpose](#purpose)
- [Approach](#approach)
- [Shared Context](#shared-context)
- [Public Interface](#public-interface)
- [Internal Logic](#internal-logic)
- [Data Model](#data-model)
- [Error Table](#error-table)
- [Acceptance Criteria (Gherkin)](#acceptance-criteria-gherkin)
- [Performance / Security / Observability](#performance--security--observability)
- [Gaps](#gaps)

## Purpose

Turns a package and an agent configuration into a bundle and its manifest with the Checker and the Resolver and Compiler (ADR-082) in place of wrightrun's `apm compile`; runs the check — load with no model call, start-up report; and, for publish, writes one immutable agent release to the organisation's container registry, refusing an existing release identifier with different content (`02:784`, `03:1543`–`1574`). It runs in three sub-commands of the ordin server, on an operator's machine or in CI: `ordin-server agent prepare`, `ordin-server agent check` and `ordin-server agent publish` (`08:47`; wrightrun's command names in kebab-case). It reads the package declaration, `wrightrun-declaration.yaml`, whose format ordin owns from ADR-098 (`08:41`), and it owns the bundle layout, the manifest and the release record (`02:786`). A failed check writes nothing and names the file and entry (`03:1572`). The agent profile ships only once C-81 confirms Anthropic's terms allow an organisation-installed Claude Code with its own key; C-81 has no fallback in this component (Phase 1, *Confirmation items*).

## Approach

A fixed sequence that works in a temporary folder beside the output folder and renames it into place as its last step, so a failed run leaves no partial bundle: inputs, copy, compile, links, skills, package facts, declaration, manifest, harness layout, write. It ports wrightrun's prepare stage (`agent-wright-runtime/docs/spec/components/PrepareStage.md`) unchanged where it reads the AgentWright package layout and the declaration, and replaces its `apm compile -t agents` step by three calls: the program-supplied `PackageReader` turns the package folder into Agent Model parts (the APM format plug-in, ADR-087), the Checker checks them, and the Resolver and Compiler compiles the always-on instructions into `AGENTS.md`; the Claude Code session form (`instructions.md` and a plugin folder, C-61 result (b)) is written by a program-supplied `HarnessLayoutWriter` over the Claude Code Installer. Check and publish reuse the Agent Loader's load and its release module, so a release is checked by exactly the code that later loads it. Rejected: running `apm`, which ADR-082 removed; building the Claude Code plugin form here, because the Installers own that form and a second writer would drift.

**Error prefixes:** `PRE_` (wrightrun's prepare codes, kept) and `DEC_` (the declaration file, kept). Release folder and registry codes `REL_*` are raised by the Agent Loader's `release` module and pass through.

### Reconciliation assumptions

- **SA-RECON-01** `check` takes no release identifier and writes `local` in the temporary record, as wrightrun's `check` (reconciliation row 97, ruling R6).
- **SA-RECON-02** `publish` keeps one `reference` input; the server program keeps wrightrun's `--release-id ID --to REPOSITORY` arguments and composes `<REPOSITORY>:<ID>` (reconciliation row 98).
- **SA-RECON-03** The Release Builder calls the Resolver's async `compile_instructions` and the Checker's async `check_parts` with one `ResolveRequest` built from the `PackageReader`'s `TreeSource`, root and scope; no bound source is needed (ruling R18, FormatPlugins SA-RECON-02). The harness layout is written by the program over the Installers' `render_claude_session_form` and `write_tree` (Installers SA-RECON-04).

## Shared Context

**From 2D (copied).**

| Variable | Type | Required | Owner | Description |
|---|---|---|---|---|
| `ORDIN_REGISTRY_AUTH_FILE` | path | No; without it the registry is read with no credential | Agent Loader, Release Builder | Credentials file for the container registry. Never printed or logged. |

Publish takes the reference as a command argument, not a variable; `ORDIN_RELEASE` is the obtain step's (2D-1a).

**From 2C (copied).** `StartupReport` (the persisted start-up report record) is not written here: the check prints the text and the release record carries it (wrightrun A-39). The release record's 2C entity `AgentRelease` (`reference`, `digest`, …, `state: Published | Obtained | Running`) is published here and recorded by the Agent Loader on first load.

**From the Agent Configuration spec (copied; `ordin_types::domain::agent_profile` and `ordin-agent-configuration`).** `AgentConfig` (field `package: String`, a path relative to the configuration file's folder); `Declaration { format_version, gates: Gates { duties: Vec<DutyGate>, judged_loops: Vec<JudgedLoopGate>, bounds: Vec<CommandBound>, steps: Vec<StepGate>, argument_rules: Vec<ArgumentRule> }, message_rules, settings, chat_triggers, contract: Option<ContractStatement { version, requires, can_use }>, operations: Vec<DeclaredOperation { section, name, description, arguments: Vec<OperationArgument>, required }> }` with every nested type as that spec defines it; `Manifest { schema_version, ordin_version, compiler, package: PackageFacts { name, version }, authority: Authority { outcome, modes }, instruction_count, skills: Vec<SkillFacts { slug, description }>, tools: Vec<ToolSection { name, use_for, never, connection }>, mcp_servers: Vec<McpEntry { name, registry, transport, command, url }>, commands: Vec<CommandFacts { slug, argument_hint }>, reviewer: Option<ReviewerFacts { name, description }>, recurring_duties: Vec<RecurringDuty { skill_slug, loop_file }>, memory_skill, declaration: Option<Declaration> }`; functions `load_config`, `check_against_manifest`, `named_files`, `check_control_entries`, `check_setting_entries`, `check_trigger_entries` (all with `Prefix::Dec` for a declaration), constants `SLUG`, `TOOL_NAME`, `FILE_TOOLS`, `CONTRACT_TOOLS`, `RUNTIME_TOOLS`, `reserved_tool_names()`, `BUNDLE_DIR_PREFIXES`, `BUNDLE_FILES`, `GENERAL_PURPOSE`.

**From the Agent Loader spec (copied; `ordin-agent-loader`).** Module `release`: `ReleaseRecord { package_name, package_version, release_id, runtime_version, contract_version, manifest_schema_version, startup_report }`, `CONTRACT_VERSION = 1`, `write_release_dir(out_dir, bundle_dir, config_path, named, record)`, async `publish_dir(dir, reference, auth) -> "sha256:<hex>"`, `read_auth_file(Option<&Path>) -> RegistryAuth`, `parse_reference(&str) -> Result<ReleaseReference, ReleaseError>`, `ReleaseError` (`REL_*`). Module `manifest`: `MANIFEST_FILE_NAME = "wrightrun-manifest.json"`, `MANIFEST_SCHEMA_VERSION = 3`. The load: `load(LoadInput { mode: LoadMode::Check, .. }) -> LoadedAgent { report_text, .. }`.

**From the Checker and the Resolver and Compiler (copied from ResolverAndCompiler.md and Checker.md).** The Agent Model types are reached through the Resolver's re-export `pub use ordin_agent_model as model;` (2B-3 gives this crate no direct edge to `ordin-agent-model`). Neither call needs a bound source (ruling R18): each takes the source as an argument.

```rust
// Resolver and Compiler (ordin_resolver)
pub struct ResolveRequest {
    pub scope: model::SourceScope, pub root_id: String, pub root_version: Option<String>,
    pub target_harness: ordin_types::HarnessKind, pub workspace_languages: Vec<String>,
    pub interface_language: String, pub pack_answers: Vec<PackAnswer>, pub pack_changes: Vec<String>,
    pub overlay_changes: Vec<String>, pub replacements: Vec<Replacement>, pub staged: Vec<StagedPart>,
    pub allow_draft_root: bool, pub source: Option<SourceOverride>,
}
pub struct CompiledInstructions { pub agents_md: String, pub lock: model::LockFile }
/// Resolves `request` against `source` (ignoring `request.source`); no bound source needed.
pub async fn compile_instructions(source: &dyn model::PartSource, request: &ResolveRequest)
    -> Result<CompiledInstructions, ResolveError>;
// Checker (ordin_checker)
pub async fn check_parts(source: &dyn model::PartSource, request: &ResolveRequest)
    -> Result<PackageCheckReport, CheckError>;
pub struct PackageCheckReport { pub findings: Vec<PackageFinding> }
pub struct PackageFinding { pub stop: bool, pub file: String, pub entry: String, pub message: String }
```

**From the Installers (copied from Installers.md; used by the program's `HarnessLayoutWriter`).** `ClaudeSessionFormInput { agent_name, version, description, instructions, skills: BTreeMap<slug, BTreeMap<path, bytes>>, commands: BTreeMap<slug, bytes> }`; `render_claude_session_form(&input, bundle_dir) -> Result<RenderedTree, InstallError>`; `async fn write_tree(target_dir, &tree) -> Result<(), InstallError>` (`INST_FOLDER_NOT_EMPTY`, `INST_WRITE_FAILED`).

**From the Format Plug-ins (copied from FormatPlugins.md; used by the program's `PackageReader`).** `TreeSource::new(&outcome, repository_id, repository_label, commit) -> Result<TreeSource, FormatError>`; `TreeSource::root() -> Result<PartRef, FormatError>` (`FMT_ROOT_NOT_SINGLE`); `impl PartSource for TreeSource`.

**Library.** `tokio`, `serde_json`, `serde_path_to_error` (the Agent Configuration spec's), `regex`, `tracing`, `thiserror`. No other. YAML is read only through Shared types' restricted reader `ordin_types::yaml::read_restricted::<T>(text, max_bytes) -> Result<T, YamlError>` (over `saphyr`; SharedTypes.md module `yaml`; Phase 1 SA-YAML-01, decision P4-1; HLD 05 *YAML parsing*), which checks the size before parsing and refuses anchors, aliases, merge keys and tags outside the YAML 1.2 core schema; its codes `YAML_FEATURE_REFUSED`, `YAML_MALFORMED` and `YAML_TOO_LARGE` map to this crate's codes as *Internal Logic* states for each file.

**Constants** (wrightrun's unless marked):

```rust
pub const DECLARATION_FILE_NAME: &str = "wrightrun-declaration.yaml";
pub const DECLARATION_FORMAT_MIN: u32 = 1;
pub const DECLARATION_FORMAT_MAX: u32 = 1;
pub const MAX_DECLARATION_BYTES: u64 = 1_048_576;   // wrightrun PrepareStage.md, read_declaration step 1; the one declaration limit (PackageInspector.md, InputValidation.md)
pub const MAX_FRONT_MATTER_BYTES: usize = 1_048_576; // chosen here: apm.yml and each front matter, the declaration's bound
pub const DUTY_ENDINGS: [&str; 5] = ["stop-present", "escalation-present", "nothing-to-do", "overlap", "cycle-limit"];
pub const AUTHORITY_OUTCOMES: [&str; 3] = ["routing-only", "document-only", "system-changing"]; // wrightrun SA-29 (PrepareStage.md:48)
pub const EXCLUDED_TOP_LEVEL: [&str; 3] = [".git", ".agent-run", "memory"];
pub const HARNESS_LAYOUT_DIR: &str = ".ordin/claude";            // chosen here
pub const TEMP_PREFIX: &str = ".ordin-prepare-";                  // chosen here (wrightrun: ".wrightrun-")
```

## Public Interface

```rust
// crate ordin-release-builder

/// Supplied by the server program over the Format Plug-ins (APM format plug-in, ADR-087): it reads the
/// folder, calls `read_any`, builds a `TreeSource` (40 zeros as the commit outside Git) and returns it with
/// its `root()` and the scope `SourceScope { home: RepositoryRef::Repository(<minted id>), profile: vec![],
/// include_bundled: false }` (FormatPlugins.md, O147).
#[async_trait::async_trait]
pub trait PackageReader: Send + Sync {
    async fn read(&self, package_dir: &std::path::Path) -> Result<PackageRead, PackageReadError>;
}
pub struct PackageRead {
    pub source: Box<dyn ordin_resolver::model::PartSource>,
    pub root: ordin_resolver::model::PartRef,
    pub scope: ordin_resolver::model::SourceScope,
}
/// Supplied by the server program over the Installers (C-61 result (b)): builds `ClaudeSessionFormInput`
/// from the manifest (`package.name`, `package.version`, description "" — the manifest holds none — the
/// skills' folders `<bundle>/.apm/skills/<slug>/` and the commands' files `<bundle>/.apm/prompts/<slug>.prompt.md`),
/// calls `render_claude_session_form(&input, bundle_dir)` and `write_tree(<bundle>/.ordin/claude, &tree)`.
/// Writes `<bundle>/.ordin/claude/instructions.md` and `<bundle>/.ordin/claude/plugin/` (reconciliation row 100, N19).
#[async_trait::async_trait]
pub trait HarnessLayoutWriter: Send + Sync {
    async fn write(&self, bundle_dir: &std::path::Path, manifest: &Manifest, agents_md: &str) -> Result<(), LayoutError>;
}
pub struct PackageReadError { pub file: String, pub message: String }
pub struct LayoutError { pub message: String }

pub struct Builder {
    pub reader: std::sync::Arc<dyn PackageReader>,
    pub layout: std::sync::Arc<dyn HarnessLayoutWriter>,
    pub ordin_version: String,
    pub harness_version: String,
}

pub struct PrepareInput<'a> {
    pub config_path: &'a std::path::Path,   // the agent configuration file
    pub out_dir: &'a std::path::Path,       // the bundle folder to write
}
/// `ordin-server agent check --config FILE`: no release identifier; the check's record uses "local", as wrightrun's (reconciliation row 97).
pub struct CheckInput<'a> { pub config_path: &'a std::path::Path }
pub const CHECK_RELEASE_ID: &str = "local";
pub struct PublishInput<'a> {
    pub config_path: &'a std::path::Path,
    /// "<registry>/<repository>:<release_id>", no digest. The server program composes it from
    /// `publish --to REPOSITORY --release-id ID` as "<REPOSITORY>:<ID>" (reconciliation row 98).
    pub reference: &'a str,
    pub auth_file: Option<&'a std::path::Path>, // ORDIN_REGISTRY_AUTH_FILE
}

impl Builder {
    /// `ordin-server agent prepare`: writes the bundle; returns its manifest.
    pub async fn prepare(&self, input: PrepareInput<'_>) -> Result<Manifest, BuildError>;
    /// `ordin-server agent check`: prepares into a temporary folder, lays out a local release,
    /// loads it in Check mode and returns the start-up report text. Writes nothing that lasts.
    pub async fn check(&self, input: CheckInput<'_>, load: &LoadDeps<'_>) -> Result<String, BuildError>;
    /// `ordin-server agent publish`: check, then write one release; returns "<registry>/<repository>@sha256:<hex>".
    pub async fn publish(&self, input: PublishInput<'_>, load: &LoadDeps<'_>) -> Result<String, BuildError>;
}
/// What the check's load needs (secrets for connections, an MCP Front and a Rules Engine instance for the
/// check, the model key source); built by the server program as for an instance, with no datastore.
pub struct LoadDeps<'a> { /* the LoadInput fields of the Agent Loader other than mode, release_dir and store */ }

/// The declaration file (wrightrun PrepareStage `read_declaration`, kept).
pub fn read_declaration(package_dir: &std::path::Path) -> Result<Option<Declaration>, BuildError>;
pub fn write_manifest(bundle_dir: &std::path::Path, manifest: &Manifest) -> Result<(), BuildError>;

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("{0}")] Prepare(PrepareFailure),     // PRE_*, DEC_*
    #[error("{0}")] Config(ConfigError),         // CFG_*
    #[error("{0}")] Release(ReleaseError),       // REL_*
    #[error("{0}")] Load(LoadError),             // LOAD_*, MPOL_*, TOOL_* (the check's load)
}
#[derive(Debug, thiserror::Error)]
#[error("error[{code}]: {message}{}", .location.as_ref().map(|l| format!(" ({l})")).unwrap_or_default())]
pub struct PrepareFailure { pub code: &'static str, pub message: String, pub location: Option<String> }
```

**Called by.**

| Public item | Called by |
|---|---|
| `Builder`, `prepare`, `check`, `publish`, `LoadDeps` | the server program's `ordin-server agent prepare`, `check` and `publish` (OrdinServer, agent operator commands of group L) |
| `PackageReader` | implemented by the server program over the Format Plug-ins (APM read), giving a `PartSource` (O147) |
| `HarnessLayoutWriter` | implemented by the server program over the Installers' Claude Code layout (reconciliation row 100) |
| `read_declaration`, `write_manifest` | this crate's `prepare` and `check` only |

### The declaration file

`wrightrun-declaration.yaml` at the package root: one UTF-8 YAML mapping of at most 1,048,576 bytes (1 MiB, `MAX_DECLARATION_BYTES`; wrightrun `PrepareStage.md` *Name and place* fixes it, and the Package Inspector and `cross-cutting/InputValidation.md` state the same single limit), read with `ordin_types::yaml::read_restricted`, so anchors, aliases, merge keys and custom tags are refused; keys are the `Declaration` field names in kebab-case; an unknown key at any depth is an error; every list defaults to empty; the smallest valid file is `format-version: 1`. `format-version` is read before every other key; `contract.version` is independent of it. The keys and their rules are wrightrun's table *Keys* (`PrepareStage.md`, *The declaration file*), unchanged; in particular: `operations[].name` matches `TOOL_NAME`, unique, not reserved, at most 50 arguments each with a `TOOL_NAME` name holding no `.` or `[`; `gates.duties[].folder` is exactly `/.agent-run/<duty>/`; `gates.duties[].command` unique across duties; `max-cycles` and `period-seconds` both or neither (1 to 100,000; 1 to 2,592,000); `heartbeat.entry` one line of 1 to 200 characters holding `{time}` and `{ending}` once each and `{cycle}` at most once; `heartbeat.endings` 1 to 20 slugs holding every ending ordin writes for the duty (`stop-present`, `escalation-present` and `overlap` always; `nothing-to-do` when `work-check` is a file check; `cycle-limit` when `max-cycles` is set), else `DEC_HEARTBEAT_ENDINGS`; `judged-loops[]` `pause-after` 1 to 10; `bounds[]` at least one bound, `wall-clock-minutes` 1 to 1,440, the two others 1 to 1,000; controls, settings and chat triggers by the Agent Configuration's helpers with `Prefix::Dec`.

**What a declaration may name** (P-7), each over the file's entries in file order; the first name the package does not hold returns the row's error with `location` = `wrightrun-declaration.yaml: <key path>` and a message listing what the package holds:

| The declaration names | Where | It must be | Code |
|---|---|---|---|
| A command | `gates.duties[].command`, `gates.judged-loops[].commands[]`, `gates.bounds[].command`, a step gate's or message rule's `command`, a `command` or `single-call` outcome, `chat-triggers[].starts` | the slug of `.apm/prompts/<slug>.prompt.md` | `DEC_UNKNOWN_COMMAND` |
| A duty | `gates.duties[].duty` | a recurring duty (a skill with `knowledge/loops/recurring-<slug>.md`) | `DEC_UNKNOWN_DUTY` |
| A sub-agent | `gates.judged-loops[].reviewer`; `called`/`not-called` with `target: subagent`; `task:<name>` | the reviewer's name; for the other two also `general-purpose` | `DEC_UNKNOWN_SUBAGENT` |
| A tool | a step gate's or argument rule's `tool`; `called`/`not-called` with `target: tool`; a `tools` outcome item | an `operations[]` name; a contract tool whose capability is in `requires` or `can-use`; a `FILE_TOOLS` name; `reply_action`, `channel_documents`; `task`; `task:<sub-agent>` (a `tools` outcome item: the first, second and fourth only) | `DEC_UNKNOWN_TOOL` |
| A path | every path condition's `path` | when a bundle path: a file or folder of the bundle after P-5, or `/wrightrun-manifest.json` | `DEC_UNKNOWN_PATH` |
| A setting | a `setting` condition's `name` | a declared setting of kind `flag`, `number`, `text` or `selection` | `DEC_UNKNOWN_SETTING` |
| A tool section | `operations[].section` | the heading of a tool section | `DEC_UNKNOWN_SECTION` |

### The bundle layout

```
<bundle>/                         the package's files at their relative paths (no write permission on any entry)
  AGENTS.md                       the compiled always-on instructions, links rewritten
  wrightrun-manifest.json         the manifest (schema 3)
  wrightrun-declaration.yaml      when the package holds one, unchanged
  .ordin/claude/instructions.md   written by the HarnessLayoutWriter (C-61)
  .ordin/claude/plugin/           written by the HarnessLayoutWriter (C-61)
```

The manifest file is UTF-8 JSON with the keys in `Manifest` field order, two-space indentation, characters outside ASCII written as themselves, one trailing newline; `declaration` is `null` or the declaration with snake_case keys. Two writes of equal manifests give equal bytes.

### Example

`ordin-server agent publish --config ./support-agent.yaml --release-id 2026.10.1 --to registry.example.org/agents/support-agent` (the program passes `reference = "registry.example.org/agents/support-agent:2026.10.1"`), with `support-agent.yaml` naming `package: ./support-agent` and `tools.http[0].openapi: tickets.openapi.yaml`:

1. `prepare` writes the bundle into `<tmp>/bundle` and returns a manifest with `package: {name: "support-agent", version: "1.4.0"}`, `schema_version: 3`, `compiler: "ordin 1.0.0"`.
2. `write_release_dir(<tmp>/release, <tmp>/bundle, ./support-agent.yaml, [./tickets.openapi.yaml], record)` with `release_id: "2026.10.1"`, `runtime_version: "1.0.0"`, `contract_version: 1`, `startup_report: ""`.
3. `load(Check)` returns the report text; the record is written again with it.
4. `publish_dir` returns `sha256:9f2c61d0c8a4b7e35a1d0f6b2e9c4a7d8b3f5e1c0a6d9b2e4f7c1a3d5b8e0f24`; the command prints `registry.example.org/agents/support-agent@sha256:9f2c61d0c8a4b7e35a1d0f6b2e9c4a7d8b3f5e1c0a6d9b2e4f7c1a3d5b8e0f24` and the report, and exits 0.

The same command with one changed byte in the package and the same release identifier exits 1 with `error[REL_EXISTS_DIFFERENT]: the release 2026.10.1 exists with other content, as sha256:9f2c…0f24; choose a new release identifier (registry.example.org/agents/support-agent:2026.10.1)`.

## Internal Logic

`prepare(input)`:

1. **P-0 Configuration.** `config = load_config(config_path)`; `package_dir = config_path.parent().join(config.package).canonicalize()`. `CFG_*` passes through.
2. **P-1 Inputs.** `package_dir/apm.yml` a file → else `PRE_NO_APM_YML`; `package_dir/.apm` a folder → else `PRE_NO_APM_DIR`; `out_dir` (resolved) neither `package_dir` nor inside it → else `PRE_OUTPUT_INSIDE_PACKAGE`; `out_dir` absent or empty → else `PRE_OUTPUT_EXISTS`.
3. **P-2 Copy.** Create `<out_dir parent>/<TEMP_PREFIX><uuid v7>/` and copy `package_dir` into its `bundle/`, not following symbolic links, leaving out the top-level entries of `EXCLUDED_TOP_LEVEL`. A top-level `AGENTS.md`, `wrightrun-manifest.json` or `.ordin` in the package → `PRE_UNPARSED` ("a package ships no AGENTS.md, no manifest and no .ordin folder"). From here only `<tmp>/bundle` is written; on any error the temporary folder is removed before the error is returned.
4. **P-3 Compile (ADR-082).** `read = reader.read(<tmp>/bundle).await` → `PackageReadError` → `PRE_PACKAGE_UNREADABLE` (location its `file`). `request = ResolveRequest { scope: read.scope, root_id: read.root.id, root_version: Some(read.root.version), target_harness: HarnessKind::ClaudeCode, workspace_languages: vec![], interface_language: "en-GB", pack_answers: vec![], pack_changes: vec![], overlay_changes: vec![], replacements: vec![], staged: vec![], allow_draft_root: true, source: None }` (*chosen here*: a package is built for Claude Code, the one harness the agent profile runs; its own draft status does not stop a release). `report = ordin_checker::check_parts(&*read.source, &request).await`; a `CheckError` → `PRE_CHECK_FAILED` with its message; any finding with `stop: true` → `PRE_CHECK_FAILED`, whose message lists every stopping finding as `<file>: <entry> — <message>` in report order (`03:1558`: "named error with file and entry; nothing written"). `compiled = ordin_resolver::compile_instructions(&*read.source, &request).await` → `ResolveError` → `PRE_COMPILE_FAILED` (the Resolver's message). Write `compiled.agents_md` to `<tmp>/bundle/AGENTS.md`; an empty text → `PRE_NO_AGENTS_MD`. Non-stopping findings are kept for the command's output.
5. **P-4 Links** (wrightrun step 4). In `AGENTS.md` replace `](../../` with `](/` and `](../skills/` with `](/.apm/skills/`; find every target with `\]\(([^)\s]+)(?:\s+"[^"]*")?\)`; skip `http://`, `https://`, `mailto:`, `#`; drop a `#fragment`; resolve against `<tmp>/bundle`; a target outside the bundle or absent is broken; any broken → `PRE_BROKEN_LINK` once, listing every broken target in file order.
6. **P-5 Skills** (wrightrun step 5). For each `.apm/skills/<slug>/` in sorted order: no `SKILL.md` → `PRE_UNPARSED`; the front matter (`^---\s*\n(.*?)\n---\s*\n`) is read with `read_restricted::<serde_json::Value>(front_matter, MAX_FRONT_MATTER_BYTES)`; front matter absent, any `YamlError` (`YAML_FEATURE_REFUSED`, `YAML_MALFORMED`, `YAML_TOO_LARGE`), not a mapping, or without string `name` and `description` → `PRE_SKILL_FRONT_MATTER`; `name ≠ slug` → `PRE_SKILL_NAME`; `description` above 1,024 characters → `PRE_SKILL_DESCRIPTION`.
7. **P-6 Package facts** (wrightrun step 6; each breach `PRE_UNPARSED` with the file and line and the expected form). `apm.yml` and the front matter of each prompt and reviewer file are read with `read_restricted::<serde_json::Value>(text, MAX_FRONT_MATTER_BYTES)`; any `YamlError` (`YAML_FEATURE_REFUSED`, `YAML_MALFORMED`, `YAML_TOO_LARGE`) → `PRE_UNPARSED` with the file and the reader's line:
   1. `apm.yml`: `name` non-empty; `version` matching `^\d+\.\d+\.\d+`; `dependencies.mcp` a list of non-empty strings (registry entries) or mappings `{name, registry: false, transport, command | url}`; names unique.
   2. `instruction_count` = files matching `.apm/instructions/*.instructions.md`, at least one.
   3. `.apm/instructions/index.instructions.md` has a line `## Authority` followed by `^(?P<outcome>[a-z]+(?:-[a-z]+)*) — modes: (?P<modes>[A-Z][a-z]+(?:, [A-Z][a-z]+)*)\.$` (U+2014); `outcome` in `AUTHORITY_OUTCOMES` (wrightrun SA-29); the Agent Loader prints the outcome and nothing enforces it.
   4. `.apm/instructions/tools.instructions.md` absent → no sections; else each `## ` section other than `Verification commands` holds exactly `- Use for: `, `- Never: `, `- Connection: ` lines in that order, non-empty; names unique.
   5. Commands: each `.apm/prompts/<slug>.prompt.md` (sorted): slug matches `SLUG`, front matter with a string `argument-hint`, body holds `${input:arguments}`.
   6. Reviewer: `.apm/agents/work-reviewer.agent.md`, when present, has front matter `name: work-reviewer`, a non-empty `description` and a non-blank body.
   7. Recurring duties: each `knowledge/loops/recurring-<x>.md` (sorted) holds a line `## Each cycle` and `<x>` is a skill slug.
   8. `memory_skill`: `.apm/skills/memory/SKILL.md` exists and its first non-blank body line matches `^<!-- bundle: memory \d+ -->$`.
8. **P-7 Declaration.** `declaration = read_declaration(<tmp>/bundle)`; when present, apply *What a declaration may name*.
9. **P-8 Manifest.** Build `Manifest { schema_version: 3, ordin_version, compiler: format!("ordin {ordin_version}"), … }` from P-6 and P-7; `check_against_manifest(&config, &manifest)` (`CFG_*` passes through).
10. **P-9 Harness layout.** `layout.write(<tmp>/bundle, &manifest, &compiled.agents_md).await` → `LayoutError` → `PRE_LAYOUT_FAILED`. Afterwards `<tmp>/bundle/.ordin/claude/instructions.md` and the folder `<tmp>/bundle/.ordin/claude/plugin/` must exist → else `PRE_LAYOUT_FAILED`.
11. **P-10 Write.** `write_manifest(<tmp>/bundle, &manifest)`; remove write permission from every entry under `<tmp>/bundle`; remove `out_dir` when it is an empty folder; rename `<tmp>/bundle` to `out_dir`; remove the temporary folder; return the manifest. Log `info` `bundle_prepared` with `package`, `version`, `skills`, `commands` (counts), `declaration` (`yes`/`no`).

`read_declaration(package_dir)` (D-1 to D-7; wrightrun's, kept): absent file → `Ok(None)`; not a regular file, above `MAX_DECLARATION_BYTES`, or not UTF-8 → `DEC_NOT_YAML`; `value = ordin_types::yaml::read_restricted::<serde_json::Value>(text, MAX_DECLARATION_BYTES as usize)`, its errors mapped by `code()`: `YAML_TOO_LARGE` → `DEC_NOT_YAML` (a guard; the size is refused above), `YAML_MALFORMED` → `DEC_NOT_YAML` ("the declaration must hold one YAML mapping", with the reader's line), `YAML_FEATURE_REFUSED` → `DEC_NOT_YAML` ("the declaration uses a YAML <anchor | alias | merge key | custom tag>, which ordin does not accept", with the reader's line); `value` not a JSON object → `DEC_NOT_YAML`; `format-version` absent → `DEC_MISSING_KEY`, not an integer → `DEC_INVALID_VALUE`, outside 1..=1 → `DEC_FORMAT_VERSION` (no other key is read first); `contract.version` outside `CONTRACT_MIN..=CONTRACT_MAX` → `DEC_CONTRACT_RANGE`; typed parse of `value` through `serde_path_to_error::deserialize::<_, Declaration>` (over the `serde_json::Value`, so the key path is kept), classifying unknown field → `DEC_UNKNOWN_KEY`, missing field → `DEC_MISSING_KEY`, unknown variant → `DEC_UNKNOWN_KIND`, other → `DEC_INVALID_VALUE`; then `check_control_entries` (message rules; gates' steps and argument rules), `check_setting_entries`, `check_trigger_entries` with `Prefix::Dec`, control names unique across the two calls (`DEC_DUPLICATE`); then the *Keys* rules the types do not express (`DEC_DUPLICATE`, `DEC_RESERVED_TOOL_NAME`, `DEC_HEARTBEAT_ENDINGS`, `DEC_INVALID_VALUE`). Every location is `wrightrun-declaration.yaml: <key path>`.

`check(input, load)`:

12. **C-1.** `prepare` into `<tmp>/bundle` (a fresh temporary folder); `named = named_files(&config, config_path)`; `write_release_dir(<tmp>/release, <tmp>/bundle, config_path, &named, &ReleaseRecord { release_id: <CHECK_RELEASE_ID for check; the reference's release identifier for publish>, runtime_version: ordin_version, contract_version: CONTRACT_VERSION, manifest_schema_version: 3, startup_report: "", package_name, package_version })`.
13. **C-2.** `ordin_agent_loader::load(LoadInput { mode: LoadMode::Check, release_dir: <tmp>/release, store: None, .. })` — the load builds every connection (MCP servers are reached, HTTP and outbound send nothing), the model settings (the key is read; no model call is made) and the rules; any error passes through. Return `loaded.report_text`. Remove the temporary folders in every outcome.

`publish(input, load)`:

14. **U-1.** `parse_reference(reference)`: a digest present, or no release identifier → `REL_REFERENCE_INVALID`. Run C-1 and C-2 with the reference's release identifier.
15. **U-2.** Write the record again with `startup_report = report_text` (the text the check built, which holds no digest, wrightrun A-39).
16. **U-3.** `digest = publish_dir(<tmp>/release, reference, &read_auth_file(auth_file)?).await` (`REL_*` pass through: `REL_EXISTS_DIFFERENT` when the identifier holds other content). Return `"<registry>/<repository>@<digest>"`. Log `info` `release_published` with `release_id`, `digest`. Remove the temporary folders.

## Data Model

N/A — the crate owns no table and runs where no datastore exists (an operator's machine or CI). It owns three file formats: the bundle layout and the manifest (*Public Interface*), and the declaration file (wrightrun's format, owned by ordin from ADR-098). The release record (`release.json`) form is the Agent Loader's `ReleaseRecord`, written by this crate's calls. A published release is never changed; a changed package or configuration is a new release identifier. No change here is destructive.

## Error Table

Each is an in-process error returned to the sub-command, which prints its `Display` form and exits with status 1; each is permanent for the same input. The person never sees a code; the team that publishes reads them.

| Condition | Status | Code | Response Body |
|---|---|---|---|
| No `apm.yml` in the package folder | `Err(permanent)` | `PRE_NO_APM_YML` | `error[PRE_NO_APM_YML]: the package folder holds no apm.yml (support-agent)` |
| No `.apm/` folder | `Err(permanent)` | `PRE_NO_APM_DIR` | `error[PRE_NO_APM_DIR]: the package folder holds no .apm folder (support-agent)` |
| Output folder is the package or inside it | `Err(permanent)` | `PRE_OUTPUT_INSIDE_PACKAGE` | `error[PRE_OUTPUT_INSIDE_PACKAGE]: the bundle must be written outside the package folder (support-agent/bundle)` |
| Output folder exists and is not empty | `Err(permanent)` | `PRE_OUTPUT_EXISTS` | `error[PRE_OUTPUT_EXISTS]: the output folder exists and is not empty (build/bundle)` |
| The package cannot be read into parts | `Err(permanent)` | `PRE_PACKAGE_UNREADABLE` | `error[PRE_PACKAGE_UNREADABLE]: the instructions file has no front matter (.apm/instructions/index.instructions.md)` |
| The Checker finds one or more stops | `Err(permanent)` | `PRE_CHECK_FAILED` | `error[PRE_CHECK_FAILED]: .apm/skills/triage/SKILL.md: tools — the tool binding names a server the package does not declare` |
| The Resolver and Compiler cannot compile the instructions | `Err(permanent)` | `PRE_COMPILE_FAILED` | `error[PRE_COMPILE_FAILED]: the part support-agent references an overlay that does not resolve` |
| The compiled `AGENTS.md` is empty | `Err(permanent)` | `PRE_NO_AGENTS_MD` | `error[PRE_NO_AGENTS_MD]: the compile wrote no always-on instructions` |
| Link targets missing from the bundle | `Err(permanent)` | `PRE_BROKEN_LINK` | `error[PRE_BROKEN_LINK]: link targets that do not exist in the bundle: /knowledge/glossary.md (AGENTS.md)` |
| Skill front matter absent or not readable (including any `YamlError` of the restricted reader) | `Err(permanent)` | `PRE_SKILL_FRONT_MATTER` | `error[PRE_SKILL_FRONT_MATTER]: expected front matter with the string keys name and description (.apm/skills/triage/SKILL.md)` |
| Skill name differs from its folder | `Err(permanent)` | `PRE_SKILL_NAME` | `error[PRE_SKILL_NAME]: name is "triag" and the folder is "triage"; they must be equal (.apm/skills/triage/SKILL.md)` |
| Skill description above 1,024 characters | `Err(permanent)` | `PRE_SKILL_DESCRIPTION` | `error[PRE_SKILL_DESCRIPTION]: description has 1100 characters; the limit is 1024 (.apm/skills/triage/SKILL.md)` |
| A package fact missing or of another form (including any `YamlError` of the restricted reader on `apm.yml` or a prompt or reviewer front matter); a package shipping `AGENTS.md`, a manifest or `.ordin` | `Err(permanent)` | `PRE_UNPARSED` | `error[PRE_UNPARSED]: expected the line '## Authority' followed by '<outcome> — modes: <Mode>, <Mode>.' (.apm/instructions/index.instructions.md)` |
| The Claude Code session form cannot be written | `Err(permanent)` | `PRE_LAYOUT_FAILED` | `error[PRE_LAYOUT_FAILED]: the Claude Code plugin folder was not written (.ordin/claude/plugin)` |
| Declaration not a UTF-8 YAML mapping of ≤ 1 MiB (1,048,576 bytes; the restricted reader's `YAML_MALFORMED` or `YAML_TOO_LARGE`) | `Err(permanent)` | `DEC_NOT_YAML` | `error[DEC_NOT_YAML]: the declaration must hold one YAML mapping (wrightrun-declaration.yaml)` |
| Declaration uses a YAML anchor, alias, merge key or a tag outside the YAML 1.2 core schema (the restricted reader's `YAML_FEATURE_REFUSED`) | `Err(permanent)` | `DEC_NOT_YAML` | `error[DEC_NOT_YAML]: the declaration uses a YAML alias, which ordin does not accept (wrightrun-declaration.yaml: line 7)` |
| Format version outside 1 to 1 | `Err(permanent)` | `DEC_FORMAT_VERSION` | `error[DEC_FORMAT_VERSION]: the declaration has format version 2; this ordin supports 1 to 1 (wrightrun-declaration.yaml: format-version)` |
| Contract version outside 1 to 1 | `Err(permanent)` | `DEC_CONTRACT_RANGE` | `error[DEC_CONTRACT_RANGE]: the package was built against contract version 3; this ordin offers version 1 and supports 1 to 1 (wrightrun-declaration.yaml: contract.version)` |
| Unknown key | `Err(permanent)` | `DEC_UNKNOWN_KEY` | `error[DEC_UNKNOWN_KEY]: unknown key; the allowed keys here are: duties, judged-loops, bounds, steps, argument-rules (wrightrun-declaration.yaml: gates.rounds)` |
| Required key absent | `Err(permanent)` | `DEC_MISSING_KEY` | `error[DEC_MISSING_KEY]: required key is missing (wrightrun-declaration.yaml: format-version)` |
| Value outside the closed vocabulary; condition at a point that lacks it | `Err(permanent)` | `DEC_UNKNOWN_KIND` | `error[DEC_UNKNOWN_KIND]: unknown point; the points are: before-tool, end (wrightrun-declaration.yaml: gates.steps[0].point)` |
| A value breaks its rule | `Err(permanent)` | `DEC_INVALID_VALUE` | `error[DEC_INVALID_VALUE]: the folder of the duty watch-queue must be /.agent-run/watch-queue/ (wrightrun-declaration.yaml: gates.duties[0].folder)` |
| A duplicate | `Err(permanent)` | `DEC_DUPLICATE` | `error[DEC_DUPLICATE]: the control name reviewed-before-end is used twice (wrightrun-declaration.yaml: message-rules[1].name)` |
| A pattern, list or rule count beyond its bound | `Err(permanent)` | `DEC_RULE_BOUNDS` | `error[DEC_RULE_BOUNDS]: a declaration holds at most 200 argument rules; this one holds 201 (wrightrun-declaration.yaml: gates.argument-rules)` |
| A pattern the `regex` crate refuses | `Err(permanent)` | `DEC_INVALID_PATTERN` | `error[DEC_INVALID_PATTERN]: the pattern is not valid; back-references and look-around are not supported (wrightrun-declaration.yaml: gates.argument-rules[1].patterns[0])` |
| A host that is not a host name | `Err(permanent)` | `DEC_INVALID_HOST` | `error[DEC_INVALID_HOST]: Example.org/ is not a host name; write lower-case labels separated by full stops (wrightrun-declaration.yaml: gates.argument-rules[0].address.hosts[0])` |
| A keyword share outside 0.5 to 1.0 | `Err(permanent)` | `DEC_KEYWORD_SHARE` | `error[DEC_KEYWORD_SHARE]: the share must be a number from 0.5 to 1.0 (wrightrun-declaration.yaml: settings[0].keyword-share)` |
| Heartbeat endings lack one ordin writes | `Err(permanent)` | `DEC_HEARTBEAT_ENDINGS` | `error[DEC_HEARTBEAT_ENDINGS]: the endings overlap, cycle-limit are written by ordin for this duty and are not in the list (wrightrun-declaration.yaml: gates.duties[0].heartbeat.endings)` |
| An operation named with a reserved tool name | `Err(permanent)` | `DEC_RESERVED_TOOL_NAME` | `error[DEC_RESERVED_TOOL_NAME]: note_write is a tool name of the runtime contract and cannot name a declared operation (wrightrun-declaration.yaml: operations[0].name)` |
| A command the package lacks | `Err(permanent)` | `DEC_UNKNOWN_COMMAND` | `error[DEC_UNKNOWN_COMMAND]: unknown command triag; the commands are: answer-ticket, triage (wrightrun-declaration.yaml: gates.bounds[0].command)` |
| A duty the package lacks | `Err(permanent)` | `DEC_UNKNOWN_DUTY` | `error[DEC_UNKNOWN_DUTY]: watch-queue is not a recurring duty of the package; the package has none (wrightrun-declaration.yaml: gates.duties[0].duty)` |
| A sub-agent the package lacks | `Err(permanent)` | `DEC_UNKNOWN_SUBAGENT` | `error[DEC_UNKNOWN_SUBAGENT]: work-reviewer is named as the reviewer and the package holds no reviewer file (wrightrun-declaration.yaml: gates.judged-loops[0].reviewer)` |
| A tool the package cannot name | `Err(permanent)` | `DEC_UNKNOWN_TOOL` | `error[DEC_UNKNOWN_TOOL]: createTicket is not a tool this package can name; it can name: create_ticket, ls, read_file, … (wrightrun-declaration.yaml: gates.steps[0].tool)` |
| A bundle path the bundle lacks | `Err(permanent)` | `DEC_UNKNOWN_PATH` | `error[DEC_UNKNOWN_PATH]: /knowledge/freeze.md is a path of the bundle and the package holds no such file (wrightrun-declaration.yaml: message-rules[0].conditions[0].path)` |
| A setting the declaration lacks, or of an unfit kind | `Err(permanent)` | `DEC_UNKNOWN_SETTING` | `error[DEC_UNKNOWN_SETTING]: quiet-hours is not a setting of this declaration of the kind flag, number, text or selection (wrightrun-declaration.yaml: message-rules[0].conditions[0].name)` |
| A tool section the package lacks | `Err(permanent)` | `DEC_UNKNOWN_SECTION` | `error[DEC_UNKNOWN_SECTION]: Tickts is not a tool section of the package; the sections are: Tickets, Supplier desk (wrightrun-declaration.yaml: operations[0].section)` |
| A configuration fault | `Err(permanent)` | `CFG_*` | as in the Agent Configuration spec |
| A release folder, reference or registry fault, including an identifier that exists with other content | `Err(permanent)` / `Err(retryable)` for `REL_REGISTRY_UNREACHABLE` | `REL_*` | as in the Agent Loader spec |
| A fault of the check's load | `Err(permanent)` | `LOAD_*`, `MPOL_*`, `TOOL_*` | as in the Agent Loader, Model Policy and Connection Binder specs |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Release Builder

  Scenario: Happy path — prepare writes a read-only bundle
    Given the support-agent package fixture and its configuration
    When prepare runs with an output folder that does not exist
    Then the folder holds AGENTS.md, wrightrun-manifest.json with schema_version 3, .ordin/claude/instructions.md and .ordin/claude/plugin/
    And no entry of the folder has write permission
    And the package folder is unchanged

  Scenario: Happy path — check prints the start-up report and writes nothing that lasts
    Given the fixture and a configuration with no MCP binding
    When check runs with only the configuration path
    Then it returns a text whose first line is "ordin start-up report"
    And no temporary folder remains
    And no request reaches the registry
    And the temporary release record held the release identifier "local" while the check ran

  Scenario: Happy path — publish returns the reference with its digest
    Given the fixture and a registry container
    When publish runs with "<registry>/agents/support-agent:2026.10.1"
    Then it returns "<registry>/agents/support-agent@sha256:" followed by 64 hexadecimal characters
    And the release record pulled by that digest holds the check's report and no "sha256:" text

  Scenario: Edge case — the same content published again changes nothing
    Given the fixture published once with digest D
    When publish runs again an hour later with the same package, configuration and reference
    Then it returns D and the registry receives no write

  Scenario: Edge case — a package with no declaration loads
    Given the fixture without wrightrun-declaration.yaml
    When prepare runs
    Then the manifest's declaration is null

  Scenario: Error path — an identifier that exists with other content
    Given the fixture published once
    When publish runs with one changed byte in the package and the same reference
    Then it fails with REL_EXISTS_DIFFERENT
    And the first digest still pulls the first content

  Scenario: Error path — a Checker stop names the file and entry; nothing is written
    Given a package whose skill triage binds a tool to an undeclared server
    When publish runs
    Then it fails with PRE_CHECK_FAILED whose message begins ".apm/skills/triage/SKILL.md: tools"
    And the output and temporary folders do not exist
    And the registry receives no request

  Scenario: Error path — a broken link in the compiled instructions
    Given an instruction that links to /knowledge/glossary.md, which the package lacks
    When prepare runs
    Then it fails with PRE_BROKEN_LINK listing /knowledge/glossary.md

  Scenario: Error path — a declaration of a later format
    Given wrightrun-declaration.yaml holding "format-version: 2"
    When prepare runs
    Then it fails with DEC_FORMAT_VERSION and no other key is reported

  Scenario: Error path — a declaration using a YAML anchor and alias is refused
    Given wrightrun-declaration.yaml holding "format-version: 1" and two gates.bounds entries where the second is "*b" for an anchor "&b" on the first
    When prepare runs
    Then it fails with DEC_NOT_YAML whose message names a YAML anchor
    And the output folder does not exist

  Scenario: Error path — a declared tool the package cannot name
    Given a step gate whose tool is createTicket and an operation named create_ticket
    When prepare runs
    Then it fails with DEC_UNKNOWN_TOOL listing create_ticket

  Scenario: Error path — heartbeat endings without the ones ordin writes
    Given a duty with max-cycles 2 and heartbeat endings [done]
    When prepare runs
    Then it fails with DEC_HEARTBEAT_ENDINGS naming stop-present, escalation-present, overlap and cycle-limit

  Scenario: Error path — a configuration naming Copilot is refused with the reason
    Given the configuration holds "harness: copilot"
    When check runs
    Then it fails with CFG_HARNESS_NOT_OFFERED naming C-75

  Scenario: Error path — a package that ships its own AGENTS.md
    Given a package with AGENTS.md at its root
    When prepare runs
    Then it fails with PRE_UNPARSED
```

## Performance / Security / Observability

- **Performance targets:** `prepare` of a 50 MB package with 200 files under 10 s p95, of which the copy and the compile take the most; `check` adds the Agent Loader's load (under 2 s p95 without MCP servers, plus up to 30 s per MCP server); `publish` adds the upload, bounded by the registry and `MAX_PART_BYTES`. Memory under 256 MiB *(assumed here)*.
- **Security:** Runs only on the publishing team's machine or CI, as an operator sub-command; no network entry point. It never writes to the package folder; the bundle is written read-only; links that leave the bundle are refused; no shell command of the package is run (a `command` check is recorded and never executed); every YAML file (the declaration, `apm.yml`, each front matter) is read only through `ordin_types::yaml::read_restricted`, which refuses anchors, aliases, merge keys and custom tags and checks the size before parsing; the declaration is parsed with unknown keys refused at every depth and patterns compiled by the `regex` crate. A release holds no secret (`08:42`): the configuration names variables only (checked by the Agent Configuration). The registry credentials file is read by path and never printed or logged. Publish refuses to overwrite a release identifier with other content.
- **Observability:** Log events (`tracing`, JSON): `bundle_prepared` (`info`; `package`, `version`, `skills`, `commands`, `declaration`), `config_checked` (`info`; `outcome`, `location`), `release_published` (`info`; `release_id`, `digest`). No metric and no span: Telemetry runs in instances, not in these sub-commands.

## Gaps

None.
