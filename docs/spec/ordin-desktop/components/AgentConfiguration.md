# SPEC: Agent Configuration

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-107; promoted to Approved by the user January 2026) · amended January 2026 (ADR-113; promoted to Approved by the user January 2026) · amended January 2026 (ADR-115)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-agent-configuration` · **Build phase:** 1 · **Programs:** A
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
- [Performance / Security / Observability](#performance--security--observability)
- [Gaps](#gaps)

## Purpose

Reads the agent configuration file of one hosted agent, checks it against its schema, and checks every name the file takes from the package against the package's manifest. It is the only place where a configuration key is interpreted, so the Release Builder (prepare, check, publish), the Agent Loader (every instance at start) and every operator sub-command see one set of rules (`02:780`, ADR-098 item 2). The file format is wrightrun's configuration file, kept key for key (`08:40`, ADR-098 item 2, as `agent-wright-runtime/docs/spec/components/ConfigurationValidator.md` fixes it), plus the optional keys ADR-097 adds — who may reach the agent, its approvers, whether the asker may approve actions that stay inside the company, connections marked external (`04:352`–`355`, `04:382`–`385`) — and three keys this spec adds for what ordin reports or refuses (`harness`, `traces`, and the refusal of `tools.connectors`). Every fault is a `ConfigError` that names the key. It refuses a configuration that names Copilot until C-75 is settled, and a configuration that names a Gemini model until C-83 is settled (OQ-65, decision 74).

*Amended January 2026 (ADR-107; HLD 0.55.0):* what must happen on every run is configured here and done by ordin, never left to the model. Three additions: `model.environment`, the settings the harness's own provider mode needs, so that any model provider is a deployment's configuration and ordin names none (the closed list of offered providers is removed); the control kind `content-check` under `controls.content-checks`, which has ordin send each question, each answer or both to a checker the organisation supplies behind a declared HTTP operation; and a `chat[].context` written as an ordered list of **context steps** that ordin runs before the model, of which the former five keys are the first three.

## Approach

The configuration's data types are plain `serde` structs with `deny_unknown_fields` and kebab-case keys; the file is read with Shared types' restricted YAML reader (`ordin_types::yaml::read_restricted`, SA-YAML-01, 05 *YAML parsing*) into a `serde_json::Value`, which is then deserialised into those structs through `serde_path_to_error`, so an unknown key, a missing key and a wrong value each carry the full key path. Rules that the types cannot express run after the parse as ordered table-driven checks, and the checks that need the package run in a second call, `check_against_manifest`, because the manifest exists only after the Release Builder's prepare step. The configuration types live in the shared types crate (`ordin-types`, module `domain::agent_profile`) so that the Rules Engine, the Approval Service, the Slack Channel Adapter and the Session Supervisor — none of which depends on this crate (2B-3) — read the same structs; this crate holds the behaviour. Rejected: a JSON Schema document with a validator library, because every rule would be stated twice; a hand-written parser over a generic YAML value, because unknown-key detection at every depth is what `deny_unknown_fields` gives.

**Error prefix:** `CFG_` (carried from wrightrun, SA-ERR-01). The shared rule helpers also raise the same codes with the prefix `DEC_` on the Release Builder's behalf for the package declaration; the `DEC_` prefix is the Release Builder's.

### Reconciliation assumptions

- **SA-RECON-01** Two type names change so that `ordin-types` holds one meaning per name: `ModelChoice` → `ModelSetting` (2C wire keeps `ModelChoice` for the run-level choice) and `KnowledgeSource` → `KnowledgeSourceConfig` (2C domain keeps `KnowledgeSource`) (reconciliation rows 278, 322; ruling R7). YAML keys are unchanged.
- **SA-RECON-02** `run-limit-minutes` is capped at 480 so that every hosted run's limit fits the 2C run limit (`wall_clock_secs` 1–28,800) the Session Supervisor applies; wrightrun's 1,440 is not carried (reconciliation row 597).
- **SA-RECON-03** The `f64` exception to 2C's no-float convention stands for this module; the Rules Engine reads the same `f64` fields rather than a decimal copy (reconciliation row 592).
- **SA-RECON-04** `FILE_TOOLS` names the File Service's tools exactly, `delete_file` in place of wrightrun's `delete` (reconciliation rows 323, 585; HLD 0.49.0). A configuration or declaration that names `delete` is refused with `CFG_UNKNOWN_TOOL` / `DEC_UNKNOWN_TOOL`.

## Shared Context

**From Phase 2B / 2D.**

- This crate is built into the ordin server only (Programs: A). It reads no environment variable, no secret and no organisation profile (2D *Configuration sources*: "The agent profile reads no organisation profile", *chosen* in 2D).
- The configuration names a variable, never a value (`08:40`). One exception, *amended January 2026 (ADR-107)*: an entry of `model.environment` may hold a `literal` value, for a provider setting that is not a secret (a region, a project name); a secret is always written `{secret: NAME}`, and the value of `NAME` is resolved as below by Model Policy. Every variable name matches `^[A-Z_][A-Z0-9_]*$` (2D-1d, wrightrun's `ENV_NAME`). The **value** of each named variable is resolved by the server program (the Caller API's intake secrets and the Slack Channel Adapter's tokens, which both receive values the program has already resolved) and by the Connection Binder's `SecretSource`, from the server's secret store or from `ORDIN_SECRETS_DIR/<NAME>` (2D-1d); this crate never reads one.
- Keys that hold a variable name (2D-1d): each item of `tools.mcp[].env`; each value of `tools.mcp[].headers`, `tools.http[].headers`, `tools.publish[].headers`; `tools.publish[].url-env`; `intakes[].secret-env`; `chat[].bot-token-env`, `chat[].app-token-env`. 2D-1d leaves `tools.connectors[].env` to this spec: the whole `tools.connectors` key is refused (step V-21 below).
- Pattern matching uses the Rust `regex` crate for every pattern of a message rule, an argument rule and a question type (`05:44`, ADR-098 item 4).
- Error-code form: `SCREAMING_SNAKE_CASE`, area prefix, codes carried from wrightrun keep their names (SA-ERR-01).

**From 2C (copied).** The identifier conventions (`Timestamp`, `…Id` newtypes) are not used by this crate. Enums serialise `snake_case` in ordin's own wire formats (SA-JSON-01); this file is in another owner's format, so its YAML keys and literal values keep wrightrun's kebab-case (SA-JSON-01 *Exceptions*).

**Libraries (1A and 05).** `serde` and `serde_json` (workspace), `regex` (`05:44`), `thiserror` (repository). YAML is read only through Shared types' restricted reader `ordin_types::yaml::read_restricted::<T>(text, max_bytes) -> Result<T, YamlError>` (over `saphyr`; SharedTypes.md module `yaml`; Phase 1 SA-YAML-01, decision P4-1; HLD 05 *YAML parsing*), which checks the size before parsing and refuses anchors, aliases, merge keys and tags outside the YAML 1.2 core schema; this crate takes no YAML library of its own. Two further crates, both named in HLD 05: **`serde_path_to_error`** (`MIT OR Apache-2.0`; key path of a `serde` error; 05 row *Utility and cryptography crates*) and **`croner`** (`MIT`; five-field cron expressions and their firings; 05 row *Cron expressions (agent profile)*). Versions are pinned at lock time and pass SA-LIC-01's `cargo-deny`.

**Fixed constants of this crate** *(chosen here unless a wrightrun source is cited)*:

```rust
pub const MAX_CONFIG_BYTES: u64 = 1_048_576;                 // wrightrun ConfigurationValidator, load_config step 1
pub const CONFIG_FILE_NAME_IN_RELEASE: &str = "wrightrun.yaml"; // the name a release stores the file under (ADR-098 item 2: same artefact)
pub const SLUG: &str = r"^[a-z0-9]+(-[a-z0-9]+)*$";          // at most 64 characters
pub const TOOL_NAME: &str = r"^[A-Za-z][A-Za-z0-9_-]{0,63}$";
pub const ENV_NAME: &str = r"^[A-Z_][A-Z0-9_]*$";
pub const NAMESPACE: &str = r"^[A-Za-z0-9_@+:~-]{1,64}$";
pub const MODEL_ID: &str = r"^[a-z0-9_-]+:\S+$";             // `provider:model`
pub const CHAT_ID: &str = r"^[A-Z0-9]{2,64}$";               // a Slack channel or user identifier (assumed here: Slack identifiers are upper-case letters and digits)
pub const MAX_PATTERN_CHARS: usize = 200;
pub const MAX_MATCH_CHARS: usize = 4_000;
pub const FORM_MAX_FIELDS: usize = 100;
pub const FIELD_MAX_CHARS: u32 = 3_000;
pub const SELECTION_MAX_OPTIONS: u32 = 100;
pub const MAX_REPLY_ACTIONS: usize = 5;
pub const CRON_REFERENCE: &str = "2024-01-01T00:00:00Z";      // wrightrun SA-10
pub const CRON_FIRINGS: usize = 1_000;
pub const DEFAULT_SAFE_REPLY: &str = "I could not answer that."; // wrightrun SA-68
pub const GENERAL_PURPOSE: &str = "general-purpose";
pub const SUBAGENT_TOOL: &str = "task";
/// The File Service's agent file tools on server `files` (FileService.md *Agent file tools*; HLD 0.49.0 H08-6: `delete_file`).
pub const FILE_TOOLS: [&str; 7] = ["ls", "read_file", "write_file", "edit_file", "delete_file", "glob", "grep"];
pub const CONTRACT_TOOLS: [(&str, &[&str]); 3] = [
    ("notes", &["note_write", "note_replace", "note_retire"]),
    ("events", &["event_record"]),
    ("run-facts", &["run_facts"]),
];
pub const RUNTIME_TOOLS: [&str; 2] = ["reply_action", "channel_documents"];
/// FILE_TOOLS, "execute", "task", every CONTRACT_TOOLS name and every RUNTIME_TOOLS name.
pub fn reserved_tool_names() -> &'static std::collections::BTreeSet<&'static str>;
// Amended January 2026 (ADR-107): the closed list OFFERED_PROVIDERS is removed. Any provider text that
// matches MODEL_ID is accepted except those of REFUSED_PROVIDERS; what a provider needs is declared
// under `model.environment`, and ordin holds no provider-specific code.
/// Providers wrightrun offered that ordin refuses with a stated reason (OQ-65, decision 74).
pub const REFUSED_PROVIDERS: [(&str, &str); 1] = [(
    "google_genai",
    "Gemini models are not offered for hosted agents until the Gemini command-line harness is confirmed (C-83)",
)];
pub const BUNDLE_DIR_PREFIXES: [&str; 4] = ["/.apm/", "/knowledge/", "/acceptance/", "/brand/"];
pub const BUNDLE_FILES: [&str; 3] = ["/AGENTS.md", "/wrightrun-manifest.json", "/wrightrun-declaration.yaml"];

// ---- amended January 2026 (ADR-107) ----
/// A name under `model.environment` (ADR-107; chosen here, SA-HOST-01).
pub const MODEL_ENVIRONMENT_NAME: &str = r"^[A-Z][A-Z0-9_]{2,63}$";
pub const MAX_MODEL_ENVIRONMENT: usize = 32;                  // chosen here, SA-HOST-01
pub const MAX_LITERAL_CHARS: usize = 4_096;                   // chosen here, SA-HOST-01
/// Names `model.environment` may never declare: every name the Claude Local Adapter copies into, sets in
/// or keeps out of the Claude Code child (ClaudeLocalAdapter.md step 8), compared ignoring case.
/// Every name beginning `ORDIN_` is reserved as well (ordin's own variables, the hook pairs among them).
/// These two constants are placed in `ordin_types::agent_profile` with the types, so that the Claude Local
/// Adapter, which has no edge to this crate, checks the same list (ClaudeLocalAdapter.md `new`).
pub const RESERVED_ENVIRONMENT_NAMES: [&str; 20] = [
    "PATH", "HOME", "USER", "LOGNAME", "SHELL", "LANG", "LC_ALL", "TMPDIR",
    "HTTPS_PROXY", "HTTP_PROXY", "NO_PROXY",
    "USERPROFILE", "APPDATA", "LOCALAPPDATA", "SYSTEMROOT", "TEMP",
    "ANTHROPIC_API_KEY", "ANTHROPIC_BASE_URL", "CLAUDE_CODE_OAUTH_TOKEN", "CLAUDE_CONFIG_DIR",
];
pub const RESERVED_ENVIRONMENT_PREFIX: &str = "ORDIN_";
pub const MAX_CONTENT_CHECKS: usize = 10;                      // chosen here, SA-HOST-05
pub const MAX_CONTEXT_STEPS: usize = 10;                       // chosen here, SA-HOST-09
pub const IMAGES_TOTAL_BYTES: u32 = 20_971_520;               // the former fixed 20 MiB of Channel Gateway K.3.7
```

## Public Interface

### Types (crate `ordin-types`, module `domain::agent_profile`)

These types are an addition to Phase 2C, *placed here* because 2C lists the agent configuration as "typed in the wire half" (`phase-2c-shared-types-domain.md` *Entities not typed here*) and the wire half does not type it. Every struct below carries `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]` and `#[serde(rename_all = "kebab-case", deny_unknown_fields)]` unless shown otherwise; every enum of literal values carries `#[serde(rename_all = "kebab-case")]`. A `Vec` field defaults to empty and a `bool` to `false` unless a default function is shown. Floating-point fields are `f64`: the format is wrightrun's and holds numbers written as YAML decimals (sampling temperature, keyword share, setting bounds); this is an exception to 2C's "no `f64`" convention, which binds ordin's persisted entities *(assumed here: the values are compared, never summed or stored as money)*.

```rust
// ---------- the file ----------
pub struct AgentConfig {
    pub package: String,
    pub model: ModelSetting,
    #[serde(default)] pub work_namespace: Option<String>,
    pub run_limit_minutes: u32,
    #[serde(default)] pub writable_paths: Option<Vec<String>>, // None: whole work folder; Some([]): none (ADR-055 of wrightrun)
    #[serde(default)] pub tools: ToolsConfig,
    #[serde(default)] pub schedules: Vec<ScheduleEntry>,
    #[serde(default)] pub intakes: Vec<IntakeEntry>,
    #[serde(default)] pub chat: Vec<ChatConnection>,
    #[serde(default)] pub knowledge: Vec<KnowledgeSourceConfig>,
    #[serde(default)] pub retention: RetentionConfig,
    #[serde(default)] pub controls: ControlsConfig,
    #[serde(default)] pub settings: Vec<SettingDeclaration>,
    #[serde(default)] pub contract: ContractConfig,
    #[serde(default)] pub mappings: Vec<OperationMapping>,
    #[serde(default)] pub auth: Option<AuthConfig>,
    // ---- keys added by ordin (all optional; absent = wrightrun's behaviour, ADR-098 item 2) ----
    #[serde(default)] pub harness: Option<HarnessChoice>,      // chosen here; default Claude
    #[serde(default)] pub traces: bool,                        // chosen here; ADR-098 item 6, `07:16`
    #[serde(default)] pub reach: Option<ReachConfig>,          // ADR-097 item 3
    #[serde(default)] pub approvers: Option<ApproversConfig>,  // ADR-097 item 4
}

#[serde(rename_all = "kebab-case")]
pub enum HarnessChoice { Claude, Copilot }

/// `model: anthropic:claude-sonnet-4-6`, or the mapping form. Named `ModelSetting` so as not to clash with
/// 2C wire `ModelChoice` (the run-level level and model of `StartRun`; reconciliation row 278). Deserialised by a hand-written
/// `Deserialize` that accepts a YAML string or a mapping and reports the mapping's own key errors.
pub enum ModelSetting { Short(String), Policy(ModelPolicyConfig) }

pub struct ModelPolicyConfig {
    pub primary: String,                         // MODEL_ID
    #[serde(default)] pub fallback: Option<String>,
    pub temperature: f64,                        // 0.0..=2.0; accepted, reported as not carried (C-77)
    #[serde(default)] pub gateway_url: Option<String>,
    pub safe_reply: String,                      // 1..=2000 characters
    /// Amended January 2026 (ADR-107): settings the harness's own provider mode needs, set in the harness
    /// child's environment only (ClaudeLocalAdapter.md step 8). Name -> value; at most MAX_MODEL_ENVIRONMENT.
    #[serde(default)] pub environment: std::collections::BTreeMap<String, EnvValue>,
}

/// One `model.environment` value: exactly one of the two keys (V-4 row `model.environment`).
/// `{literal: "eu-1"}` for a setting that is not a secret; `{secret: MODEL_TOKEN}` names a variable whose
/// value Model Policy resolves from `ORDIN_SECRETS_DIR` or the server's secret store (2D-1d).
pub struct EnvValue {
    #[serde(default)] pub literal: Option<String>,   // 1..=MAX_LITERAL_CHARS, no control character
    #[serde(default)] pub secret: Option<String>,    // the variable-name rule (ENV_NAME)
}

// ---------- connections ----------
pub struct ToolsConfig {
    #[serde(default)] pub mcp: Vec<McpBinding>,
    #[serde(default)] pub http: Vec<HttpConnection>,
    #[serde(default)] pub publish: Vec<PublishConnection>,
    /// wrightrun's Python plug-in contract is not kept (ADR-098 item 2); any item is refused (V-21).
    #[serde(default)] pub connectors: Vec<serde_json::Value>,
    #[serde(default)] pub approve_before: Vec<String>,
}

pub struct McpBinding {
    pub server: String,                                        // 1..=128 characters
    pub expose: Vec<String>,
    #[serde(default)] pub env: Vec<String>,                    // ENV_NAME each
    #[serde(default)] pub headers: std::collections::BTreeMap<String, String>, // header -> ENV_NAME
    #[serde(default)] pub transport: Option<McpTransport>,
    #[serde(default)] pub command: Option<String>,
    #[serde(default)] pub url: Option<String>,
    #[serde(default)] pub serves: Vec<String>,
    #[serde(default)] pub external: bool,                      // ADR-097 item 4
}
#[serde(rename_all = "kebab-case")]
pub enum McpTransport { Stdio, Sse, Http, StreamableHttp }

pub struct HttpOperation {
    pub name: String,                                  // TOOL_NAME
    pub method: HttpMethod,
    pub path: String,                                  // starts with "/"
    pub description: String,                           // 1..=1024
    #[serde(default)] pub query: Vec<String>,
    #[serde(default)] pub body: bool,
}
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod { Get, Post, Put, Patch, Delete }

pub struct HttpConnection {
    pub name: String,                                  // SLUG
    pub base_url: String,
    #[serde(default)] pub openapi: Option<String>,     // relative path beside the configuration
    #[serde(default)] pub operations: Vec<HttpOperation>,
    pub expose: Vec<String>,
    #[serde(default)] pub headers: std::collections::BTreeMap<String, String>,
    #[serde(default = "thirty")] pub timeout_seconds: u32, // 1..=300
    #[serde(default)] pub serves: Vec<String>,
    #[serde(default)] pub external: bool,
}

pub struct PublishConnection {
    pub name: String,
    pub tool: String,                                  // TOOL_NAME
    pub description: String,                           // 1..=1024
    #[serde(default)] pub url: Option<String>,
    #[serde(default)] pub url_env: Option<String>,
    #[serde(default)] pub headers: std::collections::BTreeMap<String, String>,
    #[serde(default = "message_field")] pub field: String, // default "message"
    #[serde(default = "thirty")] pub timeout_seconds: u32,
    #[serde(default)] pub serves: Vec<String>,
    #[serde(default)] pub external: bool,
}

// ---------- intakes, schedules, auth ----------
pub struct IntakeEntry {
    pub name: String,
    pub command: String,
    #[serde(default = "yes")] pub accept_token: bool,
    #[serde(default)] pub secret_env: Option<String>,
    #[serde(default = "signature_header")] pub signature_header: String, // "X-Wrightrun-Signature"
    #[serde(default)] pub timestamp_header: Option<String>,
    #[serde(default = "body_64k")] pub max_body_bytes: u32,           // 65_536; 1..=1_048_576
    #[serde(default = "ten")] pub max_runs_in_progress: u32,          // 1..=1000
}

pub struct ScheduleEntry {
    pub duty: String,
    pub command: String,
    #[serde(default)] pub arguments: String,            // at most 4096 characters
    pub cron: String,                                   // five fields, UTC
}

pub struct AuthConfig {
    pub issuer: String,
    pub audience: String,
    pub jwks_url: String,
    pub role_claim: String,
    pub operator_value: String,
    pub caller_value: String,
    #[serde(default = "rs256")] pub algorithms: Vec<String>, // default ["RS256"]
}

// ---------- chat ----------
pub struct ChatTrigger {
    pub kind: TriggerKind,
    #[serde(default)] pub name: Option<String>,
    #[serde(default)] pub starts: Option<String>,
    #[serde(default)] pub delivery: Delivery,           // default Asker
}
#[serde(rename_all = "kebab-case")] pub enum TriggerKind { Mention, Command, ThreadMention }
#[serde(rename_all = "kebab-case")] #[derive(Default)] pub enum Delivery { #[default] Asker, Thread }

/// Amended January 2026 (ADR-107): `chat[].context` is either the former mapping of five keys (`Fixed`) or a
/// YAML sequence of context steps (`Steps`). Deserialised by a hand-written `Deserialize` that takes a mapping
/// as `Fixed` and a sequence as `Steps` and reports each form's own key errors. `context_steps` widens `Fixed`
/// to the first three steps, so a configuration written before ADR-107 loads and behaves unchanged.
pub enum ChatContext { Fixed(ChatContextFixed), Steps(Vec<ContextStep>) }
impl Default for ChatContext { /* Fixed(ChatContextFixed::default()) */ }
impl ChatContext {
    /// The ordered steps, `Fixed` widened (V-18). In `ordin-types`, so the Channel Gateway reads the same rule.
    pub fn steps(&self) -> Vec<ContextStep>;
}

/// The former keys, unchanged.
pub struct ChatContextFixed {
    #[serde(default = "two_hundred")] pub transcript_messages: u32,       // 0..=200
    #[serde(default = "yes")] pub images: bool,
    #[serde(default = "ten_mib")] pub image_max_bytes: u32,               // 1..=10_485_760
    #[serde(default)] pub channel_documents: bool,
    #[serde(default)] pub channel_documents_max_bytes: Option<u32>,       // 1..=10_485_760
}

/// One step ordin runs before the model, in list order (Channel Gateway K.3.7). Each step's result is one
/// context piece, or a `LeftOut` item when it fails or finds nothing; a failing step never stops the others.
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ContextStep {
    /// The thread so far, oldest messages dropped beyond `max_messages`.
    Transcript { #[serde(default = "two_hundred")] max_messages: u32 },                 // 0..=200
    /// Images of the question and the thread, newest first, at most 10.
    Images { #[serde(default = "ten_mib")] max_bytes: u32,                              // each, 1..=10_485_760
             #[serde(default = "twenty_mib")] max_total_bytes: u32 },                   // 1..=IMAGES_TOTAL_BYTES
    /// The documents attached to the channel.
    ChannelDocuments { max_bytes: u32 },                                                // 1..=10_485_760
    /// Search of one knowledge source for the asker's words (Knowledge Service `lookup`).
    KnowledgeLookup { source: String, #[serde(default = "five")] limit: u32,            // 1..=20
                      max_bytes: u32 },                                                 // 1..=1_048_576; required, no default (HLD OQ-71)
    /// The newest items of one knowledge source, run only when the question holds one of `when`.
    RecentMessages { source: String, #[serde(default = "ten")] limit: u32,              // 1..=50
                     max_bytes: u32,                                                    // 1..=1_048_576
                     #[serde(default)] when: Vec<String> },                             // keywords; empty = always
    /// A fixed note, added only when a `channel-documents` and a `knowledge-lookup` step both gave a piece.
    PrecedenceNote { text: String },                                                    // 1..=1_000 characters
}

pub struct StatusSignals {
    #[serde(default = "eyes")] pub received: String,
    /// Set on the question while its run is queued or running, removed at delivery (ADR-107).
    #[serde(default = "hourglass_flowing_sand")] pub working: String,
    #[serde(default = "white_check_mark")] pub done: String,
    #[serde(default = "warning")] pub failed: String,
}

pub struct ReplyAction {
    pub id: String,
    pub kind: ReplyActionKind,
    pub label: String,                                  // 1..=75
    #[serde(default)] pub url_setting: Option<String>,
    #[serde(default)] pub notify_setting: Option<String>,
    // hand-off only (ADR-107): where the escalation goes, whom it mentions, and a switch per scope
    #[serde(default)] pub target_channel_setting: Option<String>,   // a text setting: one channel id
    #[serde(default)] pub mention_users_setting: Option<String>,    // a text setting: user ids
    #[serde(default)] pub mention_groups_setting: Option<String>,   // a text setting: group ids, `here`, `channel`
    #[serde(default)] pub enabled_setting: Option<String>,          // a flag setting declared with default true
    // link only (ADR-107): a list-backed link — one button per list entry whose keywords match the question
    #[serde(default)] pub list_setting: Option<String>,             // a list setting
    #[serde(default)] pub label_field: Option<String>,
    #[serde(default)] pub url_field: Option<String>,
    #[serde(default)] pub keywords_field: Option<String>,
    #[serde(default)] pub style: LinkStyle,                         // default Url
}
#[serde(rename_all = "kebab-case")] pub enum ReplyActionKind { Share, HandOff, Link }
#[serde(rename_all = "kebab-case")] #[derive(Default)] pub enum LinkStyle { #[default] Url, WorkflowLink }

pub struct EntryCommand {
    pub command: String,
    pub setting: String,
    pub action: EntryAction,
    #[serde(default)] pub fill: std::collections::BTreeMap<String, FillSource>,
}
#[serde(rename_all = "kebab-case")] pub enum EntryAction { Add, Remove, Show }
#[serde(rename_all = "kebab-case")] pub enum FillSource { LatestQuestion, LatestReply }

pub struct JobCommand {
    pub command: String,
    pub does: JobCommandKind,
    pub source: String,
    #[serde(default)] pub who: Who,                     // default Members
}
#[serde(rename_all = "kebab-case")] pub enum JobCommandKind { Ingest, Progress }
#[serde(rename_all = "kebab-case")] #[derive(Default)] pub enum Who { #[default] Members, OperatorsOnly }

pub struct ChatConnection {
    pub name: String,
    pub kind: ChatKind,
    pub bot_token_env: String,
    pub app_token_env: String,
    #[serde(default)] pub triggers: Vec<ChatTrigger>,
    #[serde(default)] pub context: ChatContext,
    #[serde(default)] pub status: StatusSignals,
    #[serde(default)] pub reply_actions: Vec<ReplyAction>,
    #[serde(default)] pub expose_tools: Vec<RuntimeTool>,
    pub failure_text: String,                           // 1..=2000
    pub approval_text: String,                          // 1..=2000
    #[serde(default)] pub entry_commands: Vec<EntryCommand>,
    #[serde(default)] pub job_commands: Vec<JobCommand>,
    #[serde(default)] pub version_command: Option<String>,
    #[serde(default)] pub settings_command: Option<String>,
    #[serde(default)] pub serves: Vec<String>,
}
#[serde(rename_all = "kebab-case")] pub enum ChatKind { Slack }
#[serde(rename_all = "snake_case")] pub enum RuntimeTool { ReplyAction, ChannelDocuments }

// ---------- knowledge, retention ----------
pub struct QuestionType {
    pub name: String,
    #[serde(default)] pub keywords: Vec<String>,
    #[serde(default)] pub patterns: Vec<String>,
    #[serde(default)] pub r#match: MatchMode,          // YAML key `match`; default Any
}
#[serde(rename_all = "kebab-case")] #[derive(Default)] pub enum MatchMode { #[default] Any, All }

pub struct Ranking {
    #[serde(default)] pub thread_replies: bool,
    #[serde(default)] pub one_per_text: bool,
    #[serde(default)] pub favour_links_and_steps: Vec<QuestionType>,
}

/// One `knowledge[]` source of the configuration. Named `KnowledgeSourceConfig` so as not to clash with
/// 2C domain `KnowledgeSource { origin, location, version_read }` (reconciliation row 322).
pub struct KnowledgeSourceConfig {
    pub name: String,
    pub kind: KnowledgeKind,
    #[serde(default)] pub connection: Option<String>,
    #[serde(default = "english")] pub language: String,  // ^[a-z_]{1,32}$
    #[serde(default)] pub ingest_cron: Option<String>,
    #[serde(default)] pub ranking: Ranking,
    #[serde(default = "five")] pub default_results: u32, // 1..=50
    #[serde(default)] pub results_setting: Option<String>,
    #[serde(default)] pub own_messages_setting: Option<String>,
    #[serde(default)] pub hidden_senders_setting: Option<String>,
    #[serde(default)] pub expose: Vec<KnowledgeOperation>,
    #[serde(default)] pub max_notes_per_scope: Option<u32>, // 1..=10_000, kind Notes only
    #[serde(default)] pub max_note_chars: Option<u32>,      // 1..=20_000, kind Notes only
}
#[serde(rename_all = "kebab-case")] pub enum KnowledgeKind { ChatHistory, Entries, Notes }
#[serde(rename_all = "kebab-case")] pub enum KnowledgeOperation { Search, Recent, Open }

pub struct RetentionConfig {
    #[serde(default)] pub chat_history_days: Option<u32>,   // each 1..=36_500
    #[serde(default)] pub entries_days: Option<u32>,
    #[serde(default)] pub notes_days: Option<u32>,
    #[serde(default)] pub chat_replies_days: Option<u32>,
    #[serde(default)] pub runs_days: Option<u32>,
    #[serde(default)] pub jobs_days: Option<u32>,
    #[serde(default)] pub cron: Option<String>,
}

// ---------- settings, contract, mappings ----------
pub struct ListField { pub name: String, pub max_chars: u32, #[serde(default = "yes")] pub required: bool }

pub struct SettingDeclaration {
    pub name: String,
    pub kind: SettingKind,
    pub label: String,                                   // 1..=75
    #[serde(default)] pub who: Who,
    #[serde(default)] pub default: Option<SettingDefault>,
    #[serde(default)] pub minimum: Option<f64>,
    #[serde(default)] pub maximum: Option<f64>,
    #[serde(default)] pub max_chars: Option<u32>,
    #[serde(default)] pub options: Vec<String>,
    #[serde(default)] pub lookup: Option<SettingLookup>,
    #[serde(default)] pub lookup_source: Option<String>,
    #[serde(default)] pub max_options: Option<u32>,
    #[serde(default)] pub always_listed: Vec<String>,
    #[serde(default)] pub max_items: Option<u32>,
    #[serde(default)] pub fields: Vec<ListField>,
    #[serde(default)] pub line_form: Option<String>,
    #[serde(default)] pub allowed_outcomes: Vec<RuleListOutcome>,
    #[serde(default)] pub keyword_share: Option<f64>,
    #[serde(default = "one_hundred")] pub rule_priority: i32,
}
#[serde(rename_all = "kebab-case")] pub enum SettingKind { Flag, Number, Text, Selection, List, RuleList }
#[serde(rename_all = "kebab-case")] pub enum SettingLookup { KnowledgeSenders }
#[serde(rename_all = "kebab-case")] pub enum RuleListOutcome { Reply, Command, SingleCall, AttachAction, Delivery }
/// A YAML boolean, number, string or list of strings; deserialised by `#[serde(untagged)]`.
#[serde(untagged)] pub enum SettingDefault { Flag(bool), Number(f64), Text(String), Items(Vec<String>) }

#[serde(rename_all = "kebab-case")] pub enum Capability { Notes, Events, RunFacts }
pub struct ContractConfig {
    #[serde(default)] pub capabilities: Vec<Capability>,
    #[serde(default)] pub requires: Vec<Capability>,
}

pub struct OperationMapping {
    pub operation: String,
    pub connection: String,
    pub exposed: String,
    #[serde(default)] pub arguments: std::collections::BTreeMap<String, String>,
}

// ---------- ADR-097 keys (chosen here) ----------
pub struct ReachConfig {
    /// Chat identifiers of the channels the agent answers in; absent = every enabled channel (wrightrun).
    #[serde(default)] pub channels: Option<Vec<String>>,
    /// Chat identifiers of the people the agent answers in those channels; absent = every member.
    #[serde(default)] pub users: Option<Vec<String>>,
    /// Chat identifiers of the people whose direct messages are answered; absent or empty = none (ADR-097 item 3).
    #[serde(default)] pub direct_message_users: Vec<String>,
}

pub struct ApproversConfig {
    #[serde(default)] pub users: Vec<ApproverUser>,
    /// Identity-provider group names; checked only where a token is presented (a desktop, the resume route).
    #[serde(default)] pub groups: Vec<String>,
    #[serde(default = "groups_claim")] pub group_claim: String,  // default "groups"
    #[serde(default)] pub asker_may_approve_internal: bool,      // default false
}
pub struct ApproverUser {
    #[serde(default)] pub chat_user: Option<String>,   // CHAT_ID
    #[serde(default)] pub subject: Option<String>,     // the identity provider's `sub`, 1..=256
}
// Who reads which approver field (reconciliation rows 589, 598):
// - `users[].subject`, `groups`, `group_claim`: Caller Authentication sets `Principal.approver` from a token's
//   `sub` and from the values of the claim `group_claim` names; the Approval Service matches a desktop
//   approver (ADR-097 item 9) by the same `subject` and groups.
// - `users[].chat_user`: the Channel Gateway, which sends approval cards in an approver's direct chat and
//   passes the chat user to the Approval Service as the deciding approver.
// - `asker_may_approve_internal`: the Approval Service's asker rule.
```

The control vocabulary and the package's declaration and manifest are part of the same module, because a configuration's `controls` and a declaration's `gates` share the vocabulary and the Rules Engine evaluates both (ADR-098 item 4). Field names and literal values are wrightrun's, unchanged:

```rust
#[serde(rename_all = "kebab-case")] pub enum Point { Start, BeforeTool, End }

#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Condition {
    PathExists { path: String },
    PathAbsent { path: String },
    PathContains { path: String, text: String },                      // text 1..=200
    RunInProgress,
    StartCount { of: CountOf, more_than: u32, period_seconds: u32 },   // 0..=100_000; 1..=2_592_000
    RepeatedMessage { period_seconds: u32 },                           // 1..=86_400
    Keywords { words: Vec<String>, #[serde(default)] r#match: MatchMode },
    KeywordShare { words: Vec<String>, share: f64 },                   // 0.5..=1.0
    Pattern { patterns: Vec<String>, #[serde(default)] r#match: MatchMode },
    Setting { name: String, equals: ScalarValue },
    Called { target: CallTarget, name: String, #[serde(default = "one")] count: u32 },
    NotCalled { target: CallTarget, name: String, #[serde(default = "one")] count: u32 },
    Elapsed { at_least_seconds: u32 },                                 // 1..=86_400
    ChangedWorkFolder,
}
#[serde(rename_all = "kebab-case")] pub enum CountOf { Command, Scope, Asker }
#[serde(rename_all = "kebab-case")] pub enum CallTarget { Tool, Subagent }
#[serde(untagged)] pub enum ScalarValue { Flag(bool), Number(f64), Text(String) }

#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DecidingOutcome {
    End { ending: String },
    Reply { text: String },               // 1..=4000
    Command { command: String },
    Tools { tools: Vec<String> },
    SingleCall { command: String },
}
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum AdditiveOutcome { AttachAction { action: String }, Delivery { to: Delivery } }

pub struct MessageRule {
    pub name: String,
    #[serde(default)] pub priority: i32,                 // -1000..=1000
    #[serde(default)] pub command: Option<String>,
    pub conditions: Vec<Condition>,
    #[serde(default)] pub decides: Option<DecidingOutcome>,
    #[serde(default)] pub adds: Vec<AdditiveOutcome>,
}

#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum StepOutcome { Refuse, RefuseEnding { #[serde(default = "three")] pause_after: u32 } }

pub struct StepGate {
    pub name: String,
    pub point: Point,                                    // BeforeTool or End only
    #[serde(default)] pub command: Option<String>,
    #[serde(default)] pub tool: Option<String>,
    pub conditions: Vec<Condition>,
    pub outcome: StepOutcome,
}

pub struct AddressCondition {
    #[serde(default)] pub schemes: Vec<String>,
    #[serde(default)] pub hosts: Vec<String>,
    #[serde(default)] pub with_subdomains: bool,
    #[serde(default)] pub host_is_ip: Option<bool>,
    #[serde(default)] pub host_is_single_label: Option<bool>,
    #[serde(default)] pub ports: Vec<u16>,
    #[serde(default)] pub path_prefixes: Vec<String>,
}

pub struct ArgumentRule {
    pub name: String,
    pub tool: String,
    pub argument: String,
    pub form: ArgumentForm,
    #[serde(default)] pub outcome: ArgumentOutcome,      // default Refuse
    #[serde(default)] pub present: Option<bool>,
    #[serde(default)] pub equals_any: Vec<String>,
    #[serde(default)] pub patterns: Vec<String>,
    #[serde(default)] pub max_length: Option<u32>,
    #[serde(default)] pub address: Option<AddressCondition>,
}
#[serde(rename_all = "kebab-case")] pub enum ArgumentForm { RefuseWhen, RefuseUnless }
#[serde(rename_all = "kebab-case")] #[derive(Default)] pub enum ArgumentOutcome { #[default] Refuse, Pause }

pub struct ControlsConfig {
    #[serde(default)] pub message_rules: Vec<MessageRule>,
    #[serde(default)] pub steps: Vec<StepGate>,
    #[serde(default)] pub argument_rules: Vec<ArgumentRule>,   // at most 200
    /// Amended January 2026 (ADR-107): configuration only; a package declaration has no content checks,
    /// because the checker is a connection of the deployment. At most MAX_CONTENT_CHECKS.
    #[serde(default)] pub content_checks: Vec<ContentCheck>,
}

/// A `content-check` control (ADR-107; chosen here, SA-HOST-05). ordin sends the text to `operation` of the
/// HTTP connection `connection` and acts on the verdict; the operation is never offered to the model.
pub struct ContentCheck {
    pub name: String,                                        // SLUG; unique across all controls of the file
    pub connection: String,                                  // a `tools.http[].name`
    pub operation: String,                                   // an item of that connection's `expose`
    pub points: Vec<CheckPoint>,                             // `input`, `output`, or both; no duplicate
    #[serde(default)] pub on_block: OnBlock,                 // default End
    #[serde(default)] pub on_unreachable: OnUnreachable,     // default Block
    #[serde(default)] pub categories: Vec<String>,           // empty = any block counts; each 1..=64 characters
    #[serde(default = "five")] pub timeout_seconds: u32,     // 1..=30
    #[serde(default = "twelve_thousand")] pub max_chars: u32,// 1..=100_000; the text is cut to this length
}
#[serde(rename_all = "kebab-case")] pub enum CheckPoint { Input, Output }
#[serde(rename_all = "kebab-case")] #[derive(Default)] pub enum OnBlock { #[default] End, RetryWithoutContext }
#[serde(rename_all = "kebab-case")] #[derive(Default)] pub enum OnUnreachable { #[default] Block, Allow }

// ---------- the package declaration (written by the package; read by the Release Builder) ----------
pub struct Check { pub kind: CheckKind, #[serde(default)] pub condition: Option<Condition>, #[serde(default)] pub command: Option<String> }
#[serde(rename_all = "kebab-case")] pub enum CheckKind { File, Command }
pub struct HeartbeatForm { pub entry: String, pub endings: Vec<String> }
pub struct DutyGate {
    pub duty: String, pub command: String, pub folder: String,
    #[serde(default)] pub work_check: Option<Check>, #[serde(default)] pub done_check: Option<Check>,
    #[serde(default)] pub max_cycles: Option<u32>, #[serde(default)] pub period_seconds: Option<u32>,
    #[serde(default)] pub heartbeat: Option<HeartbeatForm>, #[serde(default)] pub max_failed_cycles: Option<u32>,
}
pub struct JudgedLoopGate { pub commands: Vec<String>, pub reviewer: String, #[serde(default = "three")] pub pause_after: u32 }
pub struct CommandBound {
    pub command: String,
    #[serde(default)] pub wall_clock_minutes: Option<u32>,
    #[serde(default)] pub max_rounds: Option<u32>,
    #[serde(default)] pub max_repeated_failures: Option<u32>,
}
pub struct Gates {
    #[serde(default)] pub duties: Vec<DutyGate>,
    #[serde(default)] pub judged_loops: Vec<JudgedLoopGate>,
    #[serde(default)] pub bounds: Vec<CommandBound>,
    #[serde(default)] pub steps: Vec<StepGate>,
    #[serde(default)] pub argument_rules: Vec<ArgumentRule>,
}
pub struct ContractStatement { pub version: u32, #[serde(default)] pub requires: Vec<Capability>, #[serde(default)] pub can_use: Vec<Capability> }
#[serde(rename_all = "kebab-case")] pub enum ArgumentType { String, Integer, Number, Boolean, Array, Object }
pub struct OperationArgument { pub name: String, pub r#type: ArgumentType, #[serde(default = "yes")] pub required: bool, #[serde(default)] pub description: String }
pub struct DeclaredOperation {
    pub section: String, pub name: String, pub description: String,
    #[serde(default)] pub arguments: Vec<OperationArgument>, #[serde(default)] pub required: bool,
}
pub struct Declaration {
    pub format_version: u32,
    #[serde(default)] pub gates: Gates,
    #[serde(default)] pub message_rules: Vec<MessageRule>,
    #[serde(default)] pub settings: Vec<SettingDeclaration>,
    #[serde(default)] pub chat_triggers: Vec<ChatTrigger>,
    #[serde(default)] pub contract: Option<ContractStatement>,
    #[serde(default)] pub operations: Vec<DeclaredOperation>,
}

// ---------- the manifest (written by the Release Builder; JSON, snake_case keys) ----------
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,                    // MANIFEST_SCHEMA_VERSION = 3 (Release Builder)
    pub ordin_version: String,
    pub compiler: String,                       // "ordin <version>"; replaces wrightrun's apm_version
    pub package: PackageFacts,
    pub authority: Authority,
    pub instruction_count: u32,
    pub skills: Vec<SkillFacts>,
    pub tools: Vec<ToolSection>,
    pub mcp_servers: Vec<McpEntry>,
    pub commands: Vec<CommandFacts>,
    pub reviewer: Option<ReviewerFacts>,
    pub recurring_duties: Vec<RecurringDuty>,
    pub memory_skill: bool,
    pub declaration: Option<Declaration>,
}
// Each of these is #[serde(rename_all = "snake_case", deny_unknown_fields)]:
pub struct PackageFacts { pub name: String, pub version: String }
pub struct Authority { pub outcome: String, pub modes: Vec<String> }
pub struct SkillFacts { pub slug: String, pub description: String }
pub struct ToolSection { pub name: String, pub use_for: String, pub never: String, pub connection: String }
pub struct McpEntry { pub name: String, pub registry: bool, pub transport: Option<McpTransport>, pub command: Option<String>, pub url: Option<String> }
pub struct CommandFacts { pub slug: String, pub argument_hint: String }
pub struct ReviewerFacts { pub name: String, pub description: String }
pub struct RecurringDuty { pub skill_slug: String, pub loop_file: String }
```

### Functions (crate `ordin-agent-configuration`)

```rust
/// Step 1 of the check: the file against the schema and the rules that need the file alone.
pub fn load_config(path: &std::path::Path) -> Result<AgentConfig, ConfigError>;
/// The same, for text already read (tests, and any caller that holds the text; reconciliation row 599).
pub fn load_config_str(text: &str, label: &str) -> Result<AgentConfig, ConfigError>;
/// Step 2: every name the configuration takes from the package.
pub fn check_against_manifest(config: &AgentConfig, manifest: &Manifest) -> Result<(), ConfigError>;
/// The mapping form of `model`; a string is widened (V-13).
pub fn model_policy(config: &AgentConfig) -> ModelPolicyConfig;
/// The files the configuration names (`tools.http[].openapi`), resolved and checked to exist.
pub fn named_files(config: &AgentConfig, config_path: &std::path::Path) -> Result<Vec<std::path::PathBuf>, ConfigError>;
/// One row for each connection with an address, for `ordin-server agent hosts --config FILE` (reconciliation row 596).
pub fn connection_hosts(config: &AgentConfig) -> Vec<ConnectionHost>;
/// The offered names of a configuration (rule "Offered names").
pub fn offered_names(config: &AgentConfig) -> Vec<String>;
/// What the configuration states and ordin does not carry (start-up report and check output).
pub fn not_carried(config: &AgentConfig) -> Vec<NotCarried>;
/// The harness the configuration names; Claude when `harness` is absent.
pub fn harness(config: &AgentConfig) -> HarnessChoice;
/// Amended January 2026 (ADR-107): the context steps of a chat connection, `Fixed` widened (V-18).
pub fn context_steps(connection: &ChatConnection) -> Vec<ContextStep>;
/// Amended January 2026 (ADR-107): the (connection, operation) pairs `content-check` controls name. These
/// operations are not offered names (rule "Offered names") and are never offered to the model.
pub fn checker_operations(config: &AgentConfig) -> Vec<(String, String)>;

/// The rule helpers the Release Builder calls for a package declaration with `Prefix::Dec`.
pub fn check_control_entries(c: &ControlsView<'_>, location: &str, prefix: Prefix) -> Result<(), ConfigError>;
pub fn check_setting_entries(s: &[SettingDeclaration], location: &str, prefix: Prefix) -> Result<(), ConfigError>;
pub fn check_trigger_entries(t: &[ChatTrigger], location: &str, prefix: Prefix) -> Result<(), ConfigError>;

pub struct ControlsView<'a> {
    pub message_rules: &'a [MessageRule],
    pub steps: &'a [StepGate],
    pub argument_rules: &'a [ArgumentRule],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Prefix { Cfg, Dec }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionHost { pub connection: String, pub kind: HostKind, pub host: String } // host or "env:<NAME>"
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum HostKind { Mcp, Http, Webhook }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotCarried { pub key: String, pub reason: String }

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("error[{code}]: {message}{}", .location.as_ref().map(|l| format!(" ({l})")).unwrap_or_default())]
pub struct ConfigError {
    pub code: &'static str,      // one of the CFG_* (or DEC_*) constants of module `codes`
    pub message: String,         // never holds a value read from the environment or the secret store
    pub location: Option<String>,// key path, e.g. "tools.http[0].headers.Authorization"
}
pub mod codes { /* pub const CFG_FILE_NOT_FOUND: &str = "CFG_FILE_NOT_FOUND"; … one per Error Table row, and the DEC_ twin of each shared code */ }
```

Every function returns `ConfigError` and no other error type. None performs a network call, reads an environment variable or writes a log line.

**Called by.** Types are imported from `ordin_types::domain::agent_profile`; this crate re-exports none of them (reconciliation row 591, N126).

| Public item | Called by |
|---|---|
| `load_config`, `check_against_manifest` | Release Builder (prepare, check, publish); Agent Loader (L-12, every instance start) |
| `named_files` | Release Builder (C-1) |
| `not_carried` | Agent Loader (L-22, the start-up report's *not carried across* lines) |
| `harness`, `offered_names` | tests; no component calls them today |
| `context_steps`, `ChatContext::steps` | Channel Gateway (K.3.7, `ChatContext::steps`); Connection Binder (the `channel_documents` tool's size, B-19) |
| `checker_operations` | Connection Binder (B-5a, the operations it binds without offering) |
| `ContentCheck` and its enums | Rules Engine (it imports them; Agent Loader passes them in `ConfiguredControls.content_checks`) |
| `load_config_str` | tests; no component calls it today |
| `model_policy` | Model Policy (`build`, MP-1) |
| `connection_hosts` | the server program's `ordin-server agent hosts --config FILE` |
| `check_control_entries`, `check_setting_entries`, `check_trigger_entries`, `Prefix` | Release Builder (the package declaration, `Prefix::Dec`) |
| `AgentConfig` and its section types | Agent Loader, Connection Binder, Model Policy, Release Builder; the program, which fills the settings of Caller API, Duty Timer, Job Runner, Retention and Erasure, Knowledge Service and Memory Service from it |
| `AuthConfig`, `ApproversConfig` | Caller Authentication (`Verifier::new`), Approval Service (approver policy) |
| `ScheduleEntry` | Duty Timer |
| `SettingDeclaration` and its family (`Who`, `SettingDefault`, `SettingLookup`, `RuleListOutcome`) | Settings Store, unchanged (reconciliation rows 72, 590) |
| `ControlsConfig`, the control vocabulary, `Declaration`, `Manifest` | Rules Engine (it imports the vocabulary and keeps no copy; it reads `Declaration` from `Manifest.declaration`, which the Release Builder wrote; reconciliation rows 592–594); Agent Loader passes `ControlsConfig` to it |
| `KnowledgeSourceConfig` | Knowledge Service (taken directly; `default_results: u32`, reconciliation row 595); Connection Binder (tool names) |
| `AgentConfig.run_limit_minutes` | Agent Loader (L-19, converted into `HostedSessionSetup.run_limit_secs`, the hosted run's wall-clock limit) |

### Rules of the keys

The rules are wrightrun's (`ConfigurationValidator.md`, tables *Rules of the keys of the first Draft*, *Rules of the keys added since the first Draft*, *Rules of a control*, *Rules of a setting declaration*), carried unchanged, with the rows marked **ordin** added. "The variable-name rule": the value matches `ENV_NAME`; a breach reads "must be the name of an environment variable (capital letters, digits and underscores), not a value". "The address rule": scheme `https`, or `http` with host `localhost` or `127.0.0.1`; no user name, no password, no fragment, no `@` before the first `/` after `://`.

| Key | Rule | Code on breach |
|---|---|---|
| `package` | non-empty string | `CFG_INVALID_VALUE` |
| `model` | a string matching `MODEL_ID`, or the mapping `ModelPolicyConfig` | `CFG_INVALID_VALUE` |
| `model.primary`, `model.fallback` | each matches `MODEL_ID`; `fallback` differs from `primary` | `CFG_INVALID_VALUE` |
| **ordin** `model.primary`, `model.fallback` provider (text before `:`) | a provider of `REFUSED_PROVIDERS` → refused with its reason; every other provider is accepted (*amended January 2026, ADR-107*: no closed list of offered providers) | `CFG_MODEL_NOT_OFFERED` |
| **ordin** `model.fallback` | same provider as `model.primary` *(assumed here: one harness process has one provider route)* | `CFG_INVALID_VALUE` |
| `model.temperature` | required in the mapping form; 0.0 to 2.0 | `CFG_MISSING_KEY` / `CFG_INVALID_VALUE` |
| `model.gateway-url` | the address rule | `CFG_INVALID_VALUE` |
| **ordin** `model.environment` (ADR-107) | at most `MAX_MODEL_ENVIRONMENT` entries; each name matches `MODEL_ENVIRONMENT_NAME`, is not in `RESERVED_ENVIRONMENT_NAMES` (compared ignoring case) and does not begin `ORDIN_`; each value holds exactly one of `literal` and `secret`; `literal` 1 to `MAX_LITERAL_CHARS` characters with no control character; `secret` the variable-name rule. The message names the key, never a value | `CFG_MODEL_ENVIRONMENT_INVALID` |
| `model.safe-reply` | required in the mapping form; 1 to 2,000 characters | `CFG_MISSING_KEY` / `CFG_INVALID_VALUE` |
| `work-namespace` | matches `NAMESPACE` | `CFG_INVALID_VALUE` |
| `run-limit-minutes` | integer 1 to 480 (the 2C run limit, `wall_clock_secs` ≤ 28,800, and 2D `limits.run.wall_clock_minutes` 1–480; SA-RECON-02) | `CFG_INVALID_VALUE` |
| `writable-paths[]` | starts with `/`; no `..`, no `~`, no empty segment other than a trailing one; neither a bundle path nor a prefix of one (`/`, `/.apm`, `/AGENTS.md`, `/knowledge/loops/`, `/wrightrun-declaration.yaml` are refused) | `CFG_INVALID_VALUE` / `CFG_BUNDLE_PATH_WRITABLE` |
| `tools.mcp[].server` | 1 to 128 characters | `CFG_INVALID_VALUE` |
| `tools.mcp[].transport`, `.command`, `.url` | no transport: no command and no url; `stdio`: non-empty command, no url; other transports: url by the address rule, no command | `CFG_INVALID_VALUE` |
| connection names (`tools.http[].name`, `tools.publish[].name`, `chat[].name`; `tools.mcp[].server` counts) | `SLUG`; unique across all lists | `CFG_INVALID_VALUE` / `CFG_DUPLICATE` |
| `expose` of `tools.mcp[]`, `tools.http[]` | at least one item; each 1 to 128 characters; no duplicate within a connection | `CFG_INVALID_VALUE` / `CFG_DUPLICATE` |
| Offered names | no offered name twice across the file; none is a reserved tool name, whether or not its capability is on. *Amended January 2026 (ADR-107):* an operation a `content-check` names (`checker_operations`) is not an offered name, so `approve-before`, a mapping or a control that names it is refused by its own row | `CFG_DUPLICATE` / `CFG_RESERVED_TOOL_NAME` |
| `env` of `tools.mcp[]` | the variable-name rule | `CFG_INVALID_VALUE` |
| `headers` of `tools.mcp[]`, `tools.http[]`, `tools.publish[]` | each key `^[A-Za-z0-9-]{1,64}$`; each value the variable-name rule | `CFG_INVALID_VALUE` |
| `serves` of every connection, `chat[]` included | non-empty strings, no duplicate within a connection | `CFG_INVALID_VALUE` / `CFG_DUPLICATE` |
| `tools.http[].base-url` | the address rule | `CFG_INVALID_VALUE` |
| `tools.http[].openapi`, `.operations` | exactly one is set; `openapi` is a non-empty relative path with no `://`; `operations` holds at least one item | `CFG_INVALID_VALUE` |
| `tools.http[].operations[]` | `name` `TOOL_NAME`, unique within the connection; `path` starts with `/`; `description` 1 to 1,024; each `query` item `^[A-Za-z0-9_-]{1,64}$`, unique, holding no `.`, `[` or `]` | `CFG_INVALID_VALUE` / `CFG_DUPLICATE` |
| `timeout-seconds` of `tools.http[]`, `tools.publish[]` | 1 to 300 | `CFG_INVALID_VALUE` |
| `tools.publish[].tool`, `.description` | `TOOL_NAME`; 1 to 1,024 characters | `CFG_INVALID_VALUE` |
| `tools.publish[].url`, `.url-env` | exactly one; `url` by the address rule; `url-env` by the variable-name rule | `CFG_INVALID_VALUE` |
| `tools.publish[].field` | `^[A-Za-z_][A-Za-z0-9_]{0,63}$` | `CFG_INVALID_VALUE` |
| **ordin** `tools.connectors` | must be absent or empty | `CFG_CONNECTOR_PLUGIN_NOT_CARRIED` |
| `tools.approve-before[]` | no duplicate; each an offered name; an item that is the `exposed` name of a mapping reads "is offered to the model as <operation>; list that name" | `CFG_DUPLICATE` / `CFG_APPROVAL_NOT_EXPOSED` |
| `intakes[].name`, `.command` | `SLUG`; names unique | `CFG_INVALID_VALUE` / `CFG_DUPLICATE` |
| `intakes[].accept-token`, `.secret-env` | `secret-env` the variable-name rule; `accept-token: false` without `secret-env` reads "an intake must accept a token, a signature, or both" | `CFG_INVALID_VALUE` |
| `intakes[].signature-header`, `.timestamp-header` | each `^[A-Za-z0-9-]{1,64}$`, not `authorization` in any case, the two differ; `timestamp-header` needs `secret-env` | `CFG_INVALID_VALUE` |
| `intakes[].max-body-bytes`, `.max-runs-in-progress` | 1 to 1,048,576; 1 to 1,000 | `CFG_INVALID_VALUE` |
| `schedules[].duty`, `.command` | `SLUG`; no two entries share `duty`, `command` and `cron` as written | `CFG_INVALID_VALUE` / `CFG_DUPLICATE` |
| `schedules[].arguments` | at most 4,096 characters | `CFG_INVALID_VALUE` |
| `schedules[].cron` | five fields, valid; shortest gap between two consecutive firings of the first `CRON_FIRINGS` firings after `CRON_REFERENCE` ≥ `run-limit-minutes` minutes | `CFG_INVALID_VALUE` / `CFG_SCHEDULE_TOO_FREQUENT` |
| `auth.issuer`, `auth.jwks-url` | the address rule | `CFG_INVALID_VALUE` |
| `auth.audience`, `.role-claim`, `.operator-value`, `.caller-value` | 1 to 256 characters; the two values differ | `CFG_INVALID_VALUE` |
| `auth.algorithms` | at least one; each one of `RS256`, `RS384`, `RS512`, `ES256`, `ES384`, `ES512`, `PS256`, `PS384`, `PS512` | `CFG_INVALID_VALUE` |
| `chat[].kind` | `slack` | `CFG_UNKNOWN_KIND` |
| `chat[].bot-token-env`, `.app-token-env` | the variable-name rule; the two differ | `CFG_INVALID_VALUE` |
| `chat[].triggers[]` | `command` kind: `name` is a chat command text; other kinds: no `name`; `starts` is a `SLUG`; at most one `mention` and one `thread-mention` per connection | `CFG_INVALID_VALUE` / `CFG_DUPLICATE` |
| `chat[].context.*` (mapping form) | `transcript-messages` 0 to 200; `image-max-bytes` 1 to 10,485,760; `channel-documents-max-bytes` 1 to 10,485,760 and required when `channel-documents` is true or `expose-tools` holds `channel_documents` | `CFG_INVALID_VALUE` / `CFG_DOCUMENTS_SIZE_MISSING` |
| **ordin** `chat[].context[]` (step form, ADR-107) | 1 to `MAX_CONTEXT_STEPS` steps; `transcript`, `images`, `channel-documents` and `precedence-note` each at most once; at most one `knowledge-lookup` and one `recent-messages` per `source`; a `precedence-note` needs a `channel-documents` and a `knowledge-lookup` step in the same list | `CFG_CONTEXT_STEP_INVALID` |
| **ordin** `chat[].context[]` step keys | `max-messages` 0 to 200; `max-bytes` of `images` and `channel-documents` 1 to 10,485,760; `max-total-bytes` 1 to 20,971,520 and not below `max-bytes`; `knowledge-lookup.limit` 1 to 20; `recent-messages.limit` 1 to 50; `max-bytes` of `knowledge-lookup` and `recent-messages` 1 to 1,048,576; `when` ≤ 50 keywords of one word of 1 to 50 characters; `precedence-note.text` 1 to 1,000 characters | `CFG_INVALID_VALUE` |
| **ordin** `chat[].context[].source` | names a `knowledge[]` source | `CFG_UNKNOWN_SOURCE` |
| **ordin** `chat[].expose-tools` holding `channel_documents` with the step form | the list holds a `channel-documents` step, whose `max-bytes` the tool uses | `CFG_DOCUMENTS_SIZE_MISSING` |
| `chat[].status.*` | `^[a-z0-9_+-]{1,64}$` (`received`, `working`, `done`, `failed`) | `CFG_INVALID_VALUE` |
| `chat[].reply-actions[]` | `id` `SLUG`, unique per connection; `label` 1 to 75; `link` needs exactly one of `url-setting` and `list-setting`, and no `notify-setting`; `hand-off` needs at least one of `notify-setting`, `url-setting`, `target-channel-setting`, `mention-users-setting`, `mention-groups-setting`; `share` has none of the setting keys; setting names `SLUG` | `CFG_INVALID_VALUE` / `CFG_DUPLICATE` |
| `chat[].reply-actions[]` route and list keys (*added January 2026, ADR-107*) | `target-channel-setting`, `mention-users-setting`, `mention-groups-setting` and `enabled-setting` only on `hand-off`; `list-setting`, `label-field`, `url-field`, `keywords-field` and `style` only on `link`; with `list-setting`, all three field keys present, each `^[a-z][a-z0-9_]{0,31}$`, the three different; without it, none of them; `style` `url` or `workflow-link` | `CFG_INVALID_VALUE` / `CFG_ACTION_LIST_INVALID` |
| `chat[].expose-tools` | no duplicate; across all chat connections each tool listed at most once | `CFG_DUPLICATE` |
| `chat[].failure-text`, `.approval-text` | required; 1 to 2,000 characters | `CFG_MISSING_KEY` / `CFG_INVALID_VALUE` |
| `chat[].entry-commands[]` | `command` a chat command text; `setting` `SLUG`; `fill` only with `add`, keys `^[a-z][a-z0-9_]{0,31}$` | `CFG_INVALID_VALUE` |
| `chat[].job-commands[]` | `command` a chat command text; `source` names a `chat-history` source whose `connection` is this connection | `CFG_INVALID_VALUE` / `CFG_UNKNOWN_SOURCE` |
| `chat[].version-command`, `.settings-command` | a chat command text | `CFG_INVALID_VALUE` |
| Chat command texts | within one connection no text twice among trigger names, entry commands, job commands, version and settings commands (compared ignoring case) | `CFG_DUPLICATE` |
| `knowledge[].name` | `SLUG`; unique | `CFG_INVALID_VALUE` / `CFG_DUPLICATE` |
| `knowledge[].kind`, `.connection` | `chat-history` needs `connection` naming a `chat[]` entry; others have none; at most one `notes` source | `CFG_INVALID_VALUE` / `CFG_UNKNOWN_CONNECTION` / `CFG_DUPLICATE` |
| `knowledge[].language` | `^[a-z_]{1,32}$` | `CFG_INVALID_VALUE` |
| `knowledge[].ingest-cron` | `chat-history` only; a cron expression | `CFG_INVALID_VALUE` |
| `knowledge[].ranking` | `chat-history` only; question types: `name` `SLUG` unique, at least one keyword or pattern, ≤ 50 keywords of one word of 1 to 50 characters, ≤ 10 patterns of ≤ 200 characters each, each compiled by `regex` | `CFG_INVALID_VALUE` / `CFG_RULE_BOUNDS` / `CFG_INVALID_PATTERN` |
| `knowledge[].default-results` | 1 to 50 | `CFG_INVALID_VALUE` |
| `knowledge[].*-setting` | `SLUG`; `own-messages-setting` and `hidden-senders-setting` with `chat-history` only | `CFG_INVALID_VALUE` |
| `knowledge[].expose` | no duplicate | `CFG_DUPLICATE` |
| `knowledge[].max-notes-per-scope`, `.max-note-chars` | both required with `notes`, absent otherwise; 1 to 10,000; 1 to 20,000 | `CFG_MISSING_KEY` / `CFG_INVALID_VALUE` |
| `retention.*-days` | 1 to 36,500 | `CFG_INVALID_VALUE` |
| `retention.cron` | a cron expression; refused when no period is set | `CFG_INVALID_VALUE` |
| `contract.capabilities`, `.requires` | no duplicate; every `requires` item is in `capabilities`; with `notes` on, exactly one `notes` source | `CFG_DUPLICATE` / `CFG_CAPABILITY_REQUIRED_OFF` / `CFG_INVALID_VALUE` |
| `mappings[].operation` | `TOOL_NAME`; unique | `CFG_INVALID_VALUE` / `CFG_DUPLICATE` |
| `mappings[].connection`, `.exposed` | `connection` names a connection of `tools.mcp`, `tools.http` or `tools.publish`; `exposed` is an `expose` item of it or the outbound `tool`; the pair is unique | `CFG_UNKNOWN_CONNECTION` / `CFG_MAPPING_NOT_EXPOSED` / `CFG_DUPLICATE` |
| `mappings[].arguments` | keys `TOOL_NAME`; values 1 to 128 characters; no value twice | `CFG_INVALID_VALUE` / `CFG_DUPLICATE` |
| **ordin** `harness` | `claude`; `copilot` is refused while C-75 is open | `CFG_HARNESS_NOT_OFFERED` |
| **ordin** `reach` | only with at least one `chat[]` connection; every identifier matches `CHAT_ID`; no duplicate within a list | `CFG_INVALID_VALUE` / `CFG_DUPLICATE` |
| **ordin** `approvers.users[]` | each holds `chat-user`, `subject` or both; `chat-user` matches `CHAT_ID`; `subject` 1 to 256 characters; no duplicate | `CFG_INVALID_VALUE` / `CFG_DUPLICATE` |
| **ordin** `approvers.groups[]`, `.group-claim` | each 1 to 256 characters, unique; with any group, `auth` is required ("a group approver is checked against the identity provider's token; add auth") | `CFG_INVALID_VALUE` / `CFG_MISSING_KEY` |
| **ordin** `approvers` | at least one user or one group | `CFG_INVALID_VALUE` |
| **ordin** `controls.content-checks[]` (ADR-107) | at most `MAX_CONTENT_CHECKS`; `name` `SLUG`, unique across every control name of the file; `points` 1 or 2 items, no duplicate; `categories` each 1 to 64 characters, no duplicate; `timeout-seconds` 1 to 30; `max-chars` 1 to 100,000 | `CFG_INVALID_VALUE` / `CFG_DUPLICATE` |
| **ordin** `controls.content-checks[].connection`, `.operation` | `connection` names a `tools.http[]` connection; `operation` is an item of its `expose`; with declared `operations`, that operation has `body: true`; no two content checks name the same point and operation pair | `CFG_CHECKER_UNKNOWN` / `CFG_DUPLICATE` |

The rules of a control and of a setting declaration (`check_control_entries`, `check_setting_entries`) are wrightrun's tables, unchanged: names `SLUG` and unique across the three lists of one source; conditions only of the kinds their point offers (message rule: `path-exists`, `path-absent`, `path-contains`, `run-in-progress`, `start-count`, `repeated-message`, `keywords`, `keyword-share`, `pattern`, `setting`; step gate `before-tool`: `path-exists`, `path-absent`, `path-contains`, `called`, `not-called`, `elapsed`; step gate `end`: `called`, `not-called`, `changed-work-folder`) — another kind reads "the condition <kind> is not available at the point <point>" (`CFG_UNKNOWN_KIND`); at least one condition; paths normalised; numeric bounds as the type comments state; at least one of `decides` and `adds`, at most one `delivery`, no action twice; `before-tool` gates take `refuse`, `end` gates take `refuse-ending` with `pause-after` 1 to 10, `tool` with `before-tool` only; at most 200 argument rules per source, `equals-any` ≤ 100 items of ≤ 1,000 characters, ≤ 10 patterns, address lists ≤ 100; argument place matches `^[A-Za-z_][A-Za-z0-9_-]*(\[\])?(\.[A-Za-z_][A-Za-z0-9_-]*(\[\])?)*$` and ≤ 200 characters; at least one argument condition; address hosts are lower-case ASCII host names, not dotted quads (`CFG_INVALID_HOST`); `with-subdomains` needs a host; ports 1 to 65,535; path prefixes start with `/` and hold no `..`. Settings: kinds as the type comments state; a key of another kind reads "applies to the kind <kind> only"; `text` needs `max-chars` 1 to 3,000; `selection` exactly one of `options` (1 to 100, each 1 to 75, unique) and `lookup` (which needs `lookup-source`); `list` needs 1 to 10 fields, `max-items` 1 to 1,000 and a `line-form` with each `{<field>}` once; `rule-list` needs `allowed-outcomes` (exactly one when `line-form` is set), `max-items`, a `line-form` with `{trigger}` and `{value}` once each, `keyword-share` 0.5 to 1.0 (`CFG_KEYWORD_SHARE`), `rule-priority` -1,000 to 1,000.

### Example

`support-agent.yaml`:

```yaml
package: ./support-agent
model:
  primary: anthropic:claude-sonnet-4-6
  fallback: anthropic:claude-haiku-4-5
  temperature: 0.2
  safe-reply: I could not answer that. Please ask the service desk.
run-limit-minutes: 30
writable-paths: [/.agent-run/, /memory/]
tools:
  http:
    - name: tickets
      base-url: https://tickets.example.org/api
      openapi: tickets.openapi.yaml
      expose: [getTicket, createTicket]
      headers: {Authorization: TICKETS_AUTHORIZATION}
  publish:
    - name: supplier-mail
      tool: email_supplier
      description: Sends one message to the supplier desk.
      url-env: SUPPLIER_WEBHOOK_URL
      external: true
  approve-before: [email_supplier]
chat:
  - name: workspace
    kind: slack
    bot-token-env: SLACK_BOT_TOKEN
    app-token-env: SLACK_APP_TOKEN
    triggers: [{kind: mention, delivery: thread}]
    failure-text: Something went wrong on our side.
    approval-text: This request is waiting for approval.
contract: {capabilities: [events]}
harness: claude
traces: true
reach:
  channels: [C04HELPDESK]
  direct-message-users: [U02ASHA]
approvers:
  users: [{chat-user: U07LEAD, subject: 00u1lead}]
  groups: [service-desk-leads]
  asker-may-approve-internal: true
auth:
  issuer: https://example.okta.com/oauth2/default
  audience: support-agent
  jwks-url: https://example.okta.com/oauth2/default/v1/keys
  role-claim: ordin_role
  operator-value: support-operator
  caller-value: support-caller
```

- `load_config(Path::new("support-agent.yaml"))` returns `Ok(AgentConfig { run_limit_minutes: 30, traces: true, .. })`.
- `offered_names(&config)` returns `["getTicket", "createTicket", "email_supplier", "event_record"]` (no chat connection lists a runtime tool, so neither `reply_action` nor `channel_documents` is offered).
- `not_carried(&config)` returns `[NotCarried { key: "model.temperature", reason: "Claude Code does not expose a sampling temperature; the value 0.2 is not applied (C-77)" }]`.
- `connection_hosts(&config)` returns `[ConnectionHost { connection: "tickets", kind: Http, host: "tickets.example.org" }, ConnectionHost { connection: "supplier-mail", kind: Webhook, host: "env:SUPPLIER_WEBHOOK_URL" }]`.
- The same file with `model.primary: google_genai:gemini-2.5-pro` returns `Err(ConfigError { code: "CFG_MODEL_NOT_OFFERED", message: "Gemini models are not offered for hosted agents until the Gemini command-line harness is confirmed (C-83)", location: Some("model.primary") })`, printed as `error[CFG_MODEL_NOT_OFFERED]: Gemini models are not offered for hosted agents until the Gemini command-line harness is confirmed (C-83) (model.primary)`.
- With `headers: {Authorization: "Bearer abc123"}` it returns `error[CFG_INVALID_VALUE]: must be the name of an environment variable (capital letters, digits and underscores), not a value (tools.http[0].headers.Authorization)`.

## Internal Logic

`load_config(path)` / `load_config_str(text, label)`:

1. **V-1 Read.** `std::fs::metadata(path)`; a missing or unreadable file, or a size above `MAX_CONFIG_BYTES`, returns `CFG_FILE_NOT_FOUND` with `location = path` and a message stating which. Read the bytes; bytes that are not UTF-8 return `CFG_FILE_NOT_FOUND` ("the file is not UTF-8").
2. **V-2 Parse.** `value = ordin_types::yaml::read_restricted::<serde_json::Value>(text, MAX_CONFIG_BYTES as usize)`, then `serde_path_to_error::deserialize::<_, AgentConfig>(value)` (the typed step runs over the `serde_json::Value`, not inside `read_restricted::<AgentConfig>`, because the reader reports a shape error without its key path). The reader's errors map by `code()`: `YAML_TOO_LARGE` → `CFG_FILE_NOT_FOUND` ("the file is larger than 1,048,576 bytes"; V-1 has already refused such a file, so this is a guard); `YAML_MALFORMED` → `CFG_NOT_YAML` ("the file must hold one YAML mapping", with the reader's line); `YAML_FEATURE_REFUSED` → `CFG_NOT_YAML` ("the file uses a YAML <anchor | alias | merge key | custom tag>, which ordin does not accept", with the reader's line). A document that is not one mapping (`value` not a JSON object) → `CFG_NOT_YAML` ("the file must hold one YAML mapping").
3. **V-3 Schema errors.** On a deserialisation error, build `location` from `serde_path_to_error::Path` (a sequence index written `[i]`, a key written with a `.` separator). Classify by the inner error's text, which `serde` forms stably: `unknown field` → `CFG_UNKNOWN_KEY` ("unknown key; the allowed keys here are: <the expected list serde gives, in schema order>"); `missing field` → `CFG_MISSING_KEY` with the missing key appended to `location` ("required key is missing"); `unknown variant` → `CFG_UNKNOWN_KIND` ("unknown <key>; the values are: <expected list>"); every other (wrong type, a number written as a string or boolean, an integer out of range of its Rust type) → `CFG_INVALID_VALUE` ("must be <type and bounds of the key from the rules table>"). Keys written with an underscore are unknown keys, because every struct renames to kebab-case.
4. **V-4 Field rules.** Apply the rules table, row by row in the table's order, each over the list's items in file order. The first breach returns the code its row names. Slugs, tool names, variable names and addresses are checked with private helpers `check_slug`, `check_tool_name`, `check_env_name`, `check_url` that return `ConfigError` with the row's code.
5. **V-5 Cron.** For each cron expression (`schedules[].cron`, `knowledge[].ingest-cron`, `retention.cron`): split on ASCII white space; not exactly five fields → `CFG_INVALID_VALUE` ("a cron expression has five fields, in UTC"). Parse with `croner::Cron::new(expr).parse()`; a failure → `CFG_INVALID_VALUE`. For `schedules[]` compute the first `CRON_FIRINGS` firings after `CRON_REFERENCE` in UTC; the smallest gap between consecutive firings below `run_limit_minutes × 60` seconds → `CFG_SCHEDULE_TOO_FREQUENT` ("the shortest interval is <n> minutes and the run limit is <m> minutes").
6. **V-6 Controls, settings, triggers.** `check_control_entries(&ControlsView{…config.controls…}, "controls", Prefix::Cfg)`, `check_setting_entries(&config.settings, "settings", Prefix::Cfg)`, and for each chat connection `check_trigger_entries(&chat[i].triggers, "chat[i].triggers", Prefix::Cfg)`. Patterns are compiled with `regex::RegexBuilder::new(p).size_limit(1 << 20).build()` (*assumed here: a 1 MiB compiled-size ceiling keeps a hostile pattern from exhausting memory*); a failure → `CFG_INVALID_PATTERN` ("the pattern is not valid; back-references and look-around are not supported").
7. **V-7 References inside the file.** In this order: the offered names (duplicates, reserved names); `tools.approve-before`; `mappings`; every tool reference of `controls` — a step gate's `tool`, the `name` of `called`/`not-called` with `target: tool`, each item of a `tools` outcome, an argument rule's `tool` — must be an offered name, a `FILE_TOOLS` name, `task` or `task:<sub-agent>` (`CFG_UNKNOWN_TOOL`, message lists the offered names; a `tools` outcome item that is a file tool or `task` reads "<name> cannot be listed in a tools outcome, which lists offered tools only; the offered names are: <names>"); every `attach-action` names a reply action of some chat connection (`CFG_UNKNOWN_ACTION`).
8. **V-8 ordin keys.** `harness: copilot` → `CFG_HARNESS_NOT_OFFERED` ("Copilot is not offered for hosted agents until a GitHub account that is not a person's is confirmed to hold a Copilot seat (C-75)", `location = "harness"`). Model providers per the rules table (`CFG_MODEL_NOT_OFFERED`). `model.environment` (ADR-107), each entry in key order: a name failing `MODEL_ENVIRONMENT_NAME` → `CFG_MODEL_ENVIRONMENT_INVALID` ("a provider setting name has 3 to 64 capital letters, digits and underscores and starts with a letter"); a reserved name → `CFG_MODEL_ENVIRONMENT_INVALID` ("<NAME> is set by ordin itself and cannot be declared"); neither or both of `literal` and `secret` → `CFG_MODEL_ENVIRONMENT_INVALID` ("give exactly one of literal and secret"); a `literal` out of bounds or holding a control character → `CFG_MODEL_ENVIRONMENT_INVALID` ("a literal value has 1 to 4096 characters and no control character"); a `secret` failing the variable-name rule → `CFG_MODEL_ENVIRONMENT_INVALID`. `location = "model.environment.<NAME>"`; no message holds a literal value. Content checks and context steps per the rules table; a `content-check` whose connection is not a `tools.http[]` name, or whose operation is not in its `expose`, or (declared operations) takes no body → `CFG_CHECKER_UNKNOWN` ("<connection>/<operation> is not an HTTP operation that takes a JSON body; a content check calls one", `location = "controls.content-checks[i].operation"`). A non-empty `tools.connectors` → `CFG_CONNECTOR_PLUGIN_NOT_CARRIED` ("connector plug-ins are not carried; install the system's MCP server in the image and declare it under tools.mcp", `location = "tools.connectors[0]"`). `reach` and `approvers` per the rules table.
9. **V-9** Return the `AgentConfig`. Nothing is logged; no value of a key is written anywhere.

`check_against_manifest(config, manifest)` — wrightrun's steps 1–11 unchanged, with `declaration = manifest.declaration`:

10. **V-10 MCP servers.** For each binding find the `McpEntry` with `name == server`. No `transport`: no entry → `CFG_UNKNOWN_SERVER` (message lists the package's server names or "the package configures no MCP server", ending "; to connect a server the package does not declare, add transport and url or command to the binding"); an entry with `registry: true` → `CFG_REGISTRY_SERVER`. With a `transport`: an entry with `registry: false` → `CFG_DUPLICATE` ("the package already declares the MCP server <name>; remove transport, command and url from the binding"). Effective transport `stdio` with non-empty `headers`, or another transport with non-empty `env` → `CFG_INVALID_VALUE`.
11. **V-11 Sections, commands, duties.** Each `serves` item names a `ToolSection` (`CFG_UNKNOWN_SECTION`). Each command reference — `intakes[].command`, `schedules[].command`, `chat[].triggers[].starts`, `controls.message-rules[].command`, `command`/`single-call` outcomes, `controls.steps[].command` — names a `CommandFacts.slug` (`CFG_UNKNOWN_COMMAND`). Each `schedules[].duty` names a `RecurringDuty.skill_slug` (`CFG_UNKNOWN_DUTY`); the over-cycles test per duty against a `DutyGate`'s `max_cycles` and `period_seconds` (`CFG_SCHEDULE_OVER_CYCLES`); a command mapped to two guarded duties (`CFG_INVALID_VALUE`, "the command <command> already belongs to the duty <other duty>; a command runs under the gates of one duty").
12. **V-12 Namespace, settings, gates, contract, operations, sub-agents, package chat commands.** `work-namespace` absent and the package name not matching `NAMESPACE` → `CFG_INVALID_VALUE`. A setting declared in both sources → `CFG_DUPLICATE`; more than `FORM_MAX_FIELDS` across both → `CFG_INVALID_VALUE`; every setting reference resolves with a fitting kind (`CFG_UNKNOWN_SETTING`) — for a reply action (*amended January 2026, ADR-107*): `url-setting`, `notify-setting`, `target-channel-setting`, `mention-users-setting` and `mention-groups-setting` name `text` settings, `enabled-setting` a `flag` setting, `list-setting` a `list` setting; an `enabled-setting` whose declaration has no `default: true` → `CFG_INVALID_VALUE` ("the enabled setting of a hand-off must default to true, so the hand-off is offered until someone turns it off"); a `label-field`, `url-field` or `keywords-field` that is not one of the list setting's declared `fields` → `CFG_ACTION_LIST_INVALID` (message lists the fields); every `lookup-source` names a `chat-history` source (`CFG_UNKNOWN_SOURCE`). A configuration control with the name of a declaration control → `CFG_GATE_OVERRIDE` ("the package declares a control of this name; a configuration adds controls and replaces none"). The required capabilities (`declaration.contract.requires`, else `config.contract.requires`) are on (`CFG_CAPABILITY_REQUIRED_OFF`). Each `mappings[].operation` names a `DeclaredOperation` (`CFG_UNKNOWN_OPERATION`); each mapping argument key is an argument of it (`CFG_MAPPING_ARGUMENT`); an offered name equal to a declared operation not reached through a mapping → `CFG_DUPLICATE`. A sub-agent named by `target: subagent` or `task:<name>` is `general-purpose` or `manifest.reviewer.name` (`CFG_UNKNOWN_TOOL`). A configuration chat command whose text equals a package `command` trigger's `name` → `CFG_DUPLICATE` (a configuration trigger of kind `command` with the same name replaces the package's and is not a duplicate).

`model_policy(config)` (V-13): `ModelSetting::Policy(p)` → `p.clone()`; `ModelSetting::Short(s)` → `ModelPolicyConfig { primary: s, fallback: None, temperature: 0.0, gateway_url: None, safe_reply: DEFAULT_SAFE_REPLY.into(), environment: BTreeMap::new() }`.

`context_steps(connection)` (V-18, ADR-107) returns `connection.context.steps()`: `ChatContext::Steps(s)` → `s.clone()`. `ChatContext::Fixed(f)` → `[Transcript { max_messages: f.transcript_messages }]`, then `Images { max_bytes: f.image_max_bytes, max_total_bytes: IMAGES_TOTAL_BYTES }` when `f.images`, then `ChannelDocuments { max_bytes: f.channel_documents_max_bytes.unwrap() }` when `f.channel_documents` (V-4 has required the size). A `transcript-messages` of 0 still gives the transcript step with `max_messages: 0`, which reads no message, as before.

`checker_operations(config)` (V-19, ADR-107): one `(connection, operation)` pair per `controls.content-checks[]` entry, in file order, without duplicates.

`named_files(config, config_path)` (V-14): `base = config_path.parent().canonicalize()`. For each set `tools.http[i].openapi`, in file order: `p = base.join(value).canonicalize()`; not under `base` → `CFG_NAMED_FILE_OUTSIDE`; not a regular file → `CFG_NAMED_FILE_MISSING`; `location = "tools.http[i].openapi"`. Return without duplicates, in file order. The content is not read.

`connection_hosts(config)` (V-15): one row per `tools.mcp[]` binding with `url` (`Mcp`), per `tools.http[]` (`Http`), per `tools.publish[]` (`Webhook`; host, or `env:<NAME>` for `url-env`), in that order; host lower-cased, without port, user information or path.

`not_carried(config)` (V-16): one `NotCarried` for `model.temperature` when the mapping form is used ("Claude Code does not expose a sampling temperature; the value <t> is not applied (C-77)"). The list is empty for the string form. *(assumed here: a refused key is an error, not a `NotCarried` entry; only accepted-and-ignored keys appear.)*

`harness(config)` (V-17): `config.harness.unwrap_or(HarnessChoice::Claude)`.

The three helpers take `prefix`: with `Prefix::Dec` every code they return is the `DEC_` twin of the `CFG_` code (`DEC_UNKNOWN_KIND`, `DEC_UNKNOWN_KEY`, `DEC_MISSING_KEY`, `DEC_INVALID_VALUE`, `DEC_DUPLICATE`, `DEC_RULE_BOUNDS`, `DEC_INVALID_PATTERN`, `DEC_INVALID_HOST`, `DEC_KEYWORD_SHARE`).

## Data Model

N/A — the crate owns no table and stores nothing. It owns the agent configuration file format (the YAML of *Public Interface*) and the types of the package declaration and the manifest, which the Release Builder writes. No change is destructive at this stage; a later change that removes or narrows a key is a breaking change of a public format (ADR-098 item 2) and needs an HLD decision.

## Error Table

Every row is a `ConfigError` returned by a function of this crate (in-process). Each is permanent: the same input gives the same error, so no caller retries. A command that receives one prints its `Display` form and exits with status 1 (Release Builder, operator sub-commands); an instance that receives one at start never reports ready (Agent Loader).

| Condition | Status | Code | Response Body |
|---|---|---|---|
| File missing, unreadable, above 1,048,576 bytes, or not UTF-8 | `Err(permanent)` | `CFG_FILE_NOT_FOUND` | `error[CFG_FILE_NOT_FOUND]: the configuration file cannot be read (support-agent.yaml)` |
| Not YAML, or not one mapping (the restricted reader's `YAML_MALFORMED`) | `Err(permanent)` | `CFG_NOT_YAML` | `error[CFG_NOT_YAML]: the file must hold one YAML mapping (support-agent.yaml)` |
| The file uses a YAML anchor, alias, merge key or a tag outside the YAML 1.2 core schema (the restricted reader's `YAML_FEATURE_REFUSED`) | `Err(permanent)` | `CFG_NOT_YAML` | `error[CFG_NOT_YAML]: the file uses a YAML alias, which ordin does not accept (support-agent.yaml: line 14)` |
| Unknown key at any depth, a key with an underscore included | `Err(permanent)` | `CFG_UNKNOWN_KEY` | `error[CFG_UNKNOWN_KEY]: unknown key; the allowed keys here are: server, expose, env, headers, transport, command, url, serves, external (tools.mcp[0].tool)` |
| Required key absent | `Err(permanent)` | `CFG_MISSING_KEY` | `error[CFG_MISSING_KEY]: required key is missing (model)` |
| A value breaks its rule; a credential value where a variable name is expected; an address with a user name or password | `Err(permanent)` | `CFG_INVALID_VALUE` | `error[CFG_INVALID_VALUE]: must be an integer from 1 to 480 (run-limit-minutes)` |
| A value outside a closed vocabulary; a condition at a point that does not offer it | `Err(permanent)` | `CFG_UNKNOWN_KIND` | `error[CFG_UNKNOWN_KIND]: unknown form; the forms are: refuse-when, refuse-unless (controls.argument-rules[0].form)` |
| A duplicate name, offered name, entry, source, setting, control, chat command text or mapping | `Err(permanent)` | `CFG_DUPLICATE` | `error[CFG_DUPLICATE]: the tool name getTicket is offered twice (tools.http[1].expose[0])` |
| `approve-before` names a tool that is not offered | `Err(permanent)` | `CFG_APPROVAL_NOT_EXPOSED` | `error[CFG_APPROVAL_NOT_EXPOSED]: createTicket is offered to the model as create_ticket; list that name (tools.approve-before[0])` |
| A writable path is or holds a bundle path | `Err(permanent)` | `CFG_BUNDLE_PATH_WRITABLE` | `error[CFG_BUNDLE_PATH_WRITABLE]: /knowledge/ is served from the bundle and is read-only (writable-paths[0])` |
| A schedule fires more often than the run limit | `Err(permanent)` | `CFG_SCHEDULE_TOO_FREQUENT` | `error[CFG_SCHEDULE_TOO_FREQUENT]: the shortest interval is 15 minutes and the run limit is 30 minutes (schedules[0].cron)` |
| A schedule fires more often than its duty's declared cycles | `Err(permanent)` | `CFG_SCHEDULE_OVER_CYCLES` | `error[CFG_SCHEDULE_OVER_CYCLES]: the schedule fires 24 times in 86400 seconds and the duty watch-queue declares at most 2 (schedules[0].cron)` |
| A binding without transport names a server the package lacks | `Err(permanent)` | `CFG_UNKNOWN_SERVER` | `error[CFG_UNKNOWN_SERVER]: the package configures no MCP server; to connect a server the package does not declare, add transport and url or command to the binding (tools.mcp[0].server)` |
| A binding without transport names a registry-form entry | `Err(permanent)` | `CFG_REGISTRY_SERVER` | `error[CFG_REGISTRY_SERVER]: io.github.github/github-mcp-server is a registry identifier with no transport; add transport and url or command to the binding (tools.mcp[0].server)` |
| `serves` names a section the package lacks | `Err(permanent)` | `CFG_UNKNOWN_SECTION` | `error[CFG_UNKNOWN_SECTION]: Tickts is not a tool section of the package; the sections are: Tickets, Supplier desk (tools.http[0].serves[0])` |
| A schedule names a skill that is not a recurring duty | `Err(permanent)` | `CFG_UNKNOWN_DUTY` | `error[CFG_UNKNOWN_DUTY]: watch-queue is not a recurring duty of the package; the package has none (schedules[0].duty)` |
| An unknown command | `Err(permanent)` | `CFG_UNKNOWN_COMMAND` | `error[CFG_UNKNOWN_COMMAND]: unknown command; the commands are: answer-ticket, triage (intakes[0].command)` |
| A mapping or source names an undeclared connection | `Err(permanent)` | `CFG_UNKNOWN_CONNECTION` | `error[CFG_UNKNOWN_CONNECTION]: ticketz is not a connection; the connections are: tickets, supplier-mail (mappings[0].connection)` |
| A job command or lookup names an unfit source | `Err(permanent)` | `CFG_UNKNOWN_SOURCE` | `error[CFG_UNKNOWN_SOURCE]: history is not a chat-history source of the connection workspace (chat[0].job-commands[0].source)` |
| A setting reference neither source declares, or of an unfit kind | `Err(permanent)` | `CFG_UNKNOWN_SETTING` | `error[CFG_UNKNOWN_SETTING]: runbook-link is not a declared setting of the kind text (chat[0].reply-actions[2].url-setting)` |
| A control names a tool not offered, or an unknown sub-agent | `Err(permanent)` | `CFG_UNKNOWN_TOOL` | `error[CFG_UNKNOWN_TOOL]: createTicket is not offered to the model; the offered names are: getTicket, create_ticket, email_supplier (controls.argument-rules[0].tool)` |
| A list-backed link's field keys missing, repeated, or not fields of its list setting (ADR-107) | `Err(permanent)` | `CFG_ACTION_LIST_INVALID` | `error[CFG_ACTION_LIST_INVALID]: url-field names no field of the list setting workflows; its fields are: name, link, words (chat[0].reply-actions[1].url-field)` |
| A rule attaches an undeclared reply action | `Err(permanent)` | `CFG_UNKNOWN_ACTION` | `error[CFG_UNKNOWN_ACTION]: run-book is not a reply action; the reply actions are: share, hand-off (controls.message-rules[0].adds[0].action)` |
| A mapping names an undeclared operation | `Err(permanent)` | `CFG_UNKNOWN_OPERATION` | `error[CFG_UNKNOWN_OPERATION]: make_ticket is not an operation the package declares; the operations are: create_ticket (mappings[0].operation)` |
| A mapping names an operation its connection does not expose | `Err(permanent)` | `CFG_MAPPING_NOT_EXPOSED` | `error[CFG_MAPPING_NOT_EXPOSED]: the connection tickets does not expose newTicket; it exposes: getTicket, createTicket (mappings[0].exposed)` |
| A mapping names an argument the operation lacks | `Err(permanent)` | `CFG_MAPPING_ARGUMENT` | `error[CFG_MAPPING_ARGUMENT]: create_ticket declares no argument priority; its arguments are: project, summary (mappings[0].arguments.priority)` |
| A connection exposes a reserved name | `Err(permanent)` | `CFG_RESERVED_TOOL_NAME` | `error[CFG_RESERVED_TOOL_NAME]: note_write is a tool name of the runtime contract and cannot be exposed by a connection (tools.mcp[0].expose[2])` |
| A required capability is off | `Err(permanent)` | `CFG_CAPABILITY_REQUIRED_OFF` | `error[CFG_CAPABILITY_REQUIRED_OFF]: the package requires the capability notes and the configuration does not switch it on (contract.capabilities)` |
| A configuration control reuses a package control's name | `Err(permanent)` | `CFG_GATE_OVERRIDE` | `error[CFG_GATE_OVERRIDE]: the package declares a control of this name; a configuration adds controls and replaces none (controls.steps[0].name)` |
| A pattern, list or rule count beyond its bound | `Err(permanent)` | `CFG_RULE_BOUNDS` | `error[CFG_RULE_BOUNDS]: a pattern has at most 200 characters; this one has 240 (controls.argument-rules[0].patterns[0])` |
| A pattern the `regex` crate refuses | `Err(permanent)` | `CFG_INVALID_PATTERN` | `error[CFG_INVALID_PATTERN]: the pattern is not valid; back-references and look-around are not supported (controls.message-rules[1].conditions[0].patterns[0])` |
| A host that is not a valid host name | `Err(permanent)` | `CFG_INVALID_HOST` | `error[CFG_INVALID_HOST]: Example.org/ is not a host name; write lower-case labels separated by full stops (controls.argument-rules[1].address.hosts[0])` |
| Channel documents read with no total size | `Err(permanent)` | `CFG_DOCUMENTS_SIZE_MISSING` | `error[CFG_DOCUMENTS_SIZE_MISSING]: state the greatest total size of a channel's documents in bytes (chat[0].context.channel-documents-max-bytes)` |
| A keyword share outside 0.5 to 1.0 | `Err(permanent)` | `CFG_KEYWORD_SHARE` | `error[CFG_KEYWORD_SHARE]: the share must be a number from 0.5 to 1.0 (settings[5].keyword-share)` |
| A named file does not exist | `Err(permanent)` | `CFG_NAMED_FILE_MISSING` | `error[CFG_NAMED_FILE_MISSING]: the file tickets.openapi.yaml does not exist beside the configuration (tools.http[0].openapi)` |
| A named file lies above the configuration's folder | `Err(permanent)` | `CFG_NAMED_FILE_OUTSIDE` | `error[CFG_NAMED_FILE_OUTSIDE]: a named file must lie in the folder of the configuration file or below it (tools.http[0].openapi)` |
| **ordin** `harness: copilot` (C-75 open) | `Err(permanent)` | `CFG_HARNESS_NOT_OFFERED` | `error[CFG_HARNESS_NOT_OFFERED]: Copilot is not offered for hosted agents until a GitHub account that is not a person's is confirmed to hold a Copilot seat (C-75) (harness)` |
| **ordin** a provider of `REFUSED_PROVIDERS` (Gemini) | `Err(permanent)` | `CFG_MODEL_NOT_OFFERED` | `error[CFG_MODEL_NOT_OFFERED]: Gemini models are not offered for hosted agents until the Gemini command-line harness is confirmed (C-83) (model.primary)` |
| **ordin** a `model.environment` entry with a bad or reserved name, neither or both of `literal` and `secret`, or a bad value (ADR-107) | `Err(permanent)` | `CFG_MODEL_ENVIRONMENT_INVALID` | `error[CFG_MODEL_ENVIRONMENT_INVALID]: ANTHROPIC_API_KEY is set by ordin itself and cannot be declared (model.environment.ANTHROPIC_API_KEY)` |
| **ordin** a content check names no HTTP operation that takes a body (ADR-107) | `Err(permanent)` | `CFG_CHECKER_UNKNOWN` | `error[CFG_CHECKER_UNKNOWN]: moderation/moderate is not an HTTP operation that takes a JSON body; a content check calls one (controls.content-checks[0].operation)` |
| **ordin** context steps repeat a single step, exceed 10, or hold a precedence note without both of its steps (ADR-107) | `Err(permanent)` | `CFG_CONTEXT_STEP_INVALID` | `error[CFG_CONTEXT_STEP_INVALID]: a precedence-note needs a channel-documents step and a knowledge-lookup step in the same list (chat[0].context[3])` |
| **ordin** `tools.connectors` holds an item | `Err(permanent)` | `CFG_CONNECTOR_PLUGIN_NOT_CARRIED` | `error[CFG_CONNECTOR_PLUGIN_NOT_CARRIED]: connector plug-ins are not carried; install the system's MCP server in the image and declare it under tools.mcp (tools.connectors[0])` |

The person never sees these codes: they are read by the team that writes the configuration, in the check command's output and an instance's log (`07`, Error handling).

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Agent Configuration

  Scenario: Happy path — the example file loads and its derived values are stated
    Given the file "support-agent.yaml" of the Example
    When load_config is called
    Then it returns a configuration with run-limit-minutes 30 and traces on
    And offered_names returns getTicket, createTicket, email_supplier, event_record in that order
    And harness returns Claude
    And not_carried returns one entry for model.temperature naming C-77

  Scenario: Happy path — a first-Draft wrightrun file keeps its meaning
    Given a file holding only "package: ./p", "model: anthropic:claude-sonnet-4-6" and "run-limit-minutes: 60"
    When load_config is called
    Then it succeeds
    And model_policy returns primary "anthropic:claude-sonnet-4-6", no fallback, temperature 0.0, no gateway and safe reply "I could not answer that."
    And not_carried returns an empty list

  Scenario: Edge case — ADR-097 keys absent behave as wrightrun
    Given the example file without the keys reach, approvers, harness and traces
    When load_config is called
    Then it succeeds with reach None, approvers None, harness None and traces false

  Scenario: Error path — unknown key at depth names the allowed keys
    Given the example file with "tool: x" added to tools.http[0]
    When load_config is called
    Then it returns CFG_UNKNOWN_KEY with location "tools.http[0].tool"
    And the message lists the allowed keys of an HTTP connection including "external"

  Scenario: Error path — a key written with an underscore is unknown
    Given a file with "run_limit_minutes: 60" in place of "run-limit-minutes: 60"
    When load_config is called
    Then it returns CFG_UNKNOWN_KEY with location "run_limit_minutes"

  Scenario: Error path — a credential value where a variable name is expected
    Given the example file with headers {Authorization: "Bearer abc123"} under tools.http[0]
    When load_config is called
    Then it returns CFG_INVALID_VALUE with location "tools.http[0].headers.Authorization"
    And the error text does not contain "abc123"

  Scenario: Error path — a number written as a string
    Given the example file with run-limit-minutes: "30"
    When load_config is called
    Then it returns CFG_INVALID_VALUE with location "run-limit-minutes"

  Scenario: Error path — a YAML anchor and alias are refused by the restricted reader
    Given the example file with "headers: &h {X-Team: TEAM_HEADER}" under tools.http[0] and "headers: *h" under tools.mcp[0]
    When load_config is called
    Then it returns CFG_NOT_YAML whose message names a YAML anchor
    And no AgentConfig is returned

  Scenario: Error path — Copilot is refused until C-75 is settled
    Given the example file with harness: copilot
    When load_config is called
    Then it returns CFG_HARNESS_NOT_OFFERED with location "harness" and a message naming C-75

  Scenario: Error path — Gemini is refused with the reason
    Given the example file with model.primary "google_genai:gemini-2.5-pro"
    When load_config is called
    Then it returns CFG_MODEL_NOT_OFFERED whose message names C-83

  Scenario: Error path — a fallback on another provider
    Given the example file with model.fallback "other-provider:claude-haiku-4-5"
    When load_config is called
    Then it returns CFG_INVALID_VALUE with location "model.fallback"

  Scenario: Happy path — any provider is accepted with its declared settings (ADR-107)
    Given the example file with model.primary "other-provider:claude-sonnet-4-6", fallback removed,
      and model.environment {SERVICE_REGION: {literal: "eu-1"}, TOKEN: {secret: MODEL_TOKEN}}
    When load_config is called
    Then it succeeds
    And model_policy returns an environment of two entries in the order SERVICE_REGION, TOKEN

  Scenario: Error path — a reserved name is refused (ADR-107)
    Given model.environment names ANTHROPIC_API_KEY as {literal: "x"}
    When load_config is called
    Then it returns CFG_MODEL_ENVIRONMENT_INVALID with location "model.environment.ANTHROPIC_API_KEY"

  Scenario: Error path — a name of ordin's own is refused, whatever its case (ADR-107)
    Given model.environment names ORDIN_HOOK_SECRET, and in a second file Path
    When load_config is called on each
    Then each returns CFG_MODEL_ENVIRONMENT_INVALID

  Scenario: Error path — both literal and secret (ADR-107)
    Given model.environment names TOKEN as {literal: "abc", secret: MODEL_TOKEN}
    When load_config is called
    Then it returns CFG_MODEL_ENVIRONMENT_INVALID whose message reads "give exactly one of literal and secret"
    And the error text does not contain "abc"

  Scenario: Happy path — a content check is loaded and its operation is not offered (ADR-107)
    Given the example file with a tools.http connection "moderation" exposing "moderate" (declared, POST, body true)
    And controls.content-checks [{name: screen, connection: moderation, operation: moderate, points: [input, output]}]
    When load_config is called
    Then it succeeds with on-block End, on-unreachable Block, timeout-seconds 5 and max-chars 12000
    And offered_names does not contain "moderate"
    And checker_operations returns [("moderation", "moderate")]

  Scenario: Error path — a content check names an operation the connection does not expose (ADR-107)
    Given controls.content-checks [{name: screen, connection: tickets, operation: moderate, points: [input]}]
    When load_config is called
    Then it returns CFG_CHECKER_UNKNOWN with location "controls.content-checks[0].operation"

  Scenario: Happy path — old context keys still load (ADR-107)
    Given chat[0].context {transcript-messages: 50, channel-documents: true, channel-documents-max-bytes: 200000}
    When load_config is called and context_steps is called for chat[0]
    Then it returns a transcript step of 50, an images step of 10485760 and 20971520 bytes and a channel-documents step of 200000 bytes, in that order

  Scenario: Happy path — context written as steps (ADR-107)
    Given knowledge [{name: channel-history, kind: chat-history, connection: workspace}]
    And chat[0].context [{kind: transcript, max-messages: 50}, {kind: knowledge-lookup, source: channel-history, limit: 5, max-bytes: 20000}]
    When load_config is called
    Then context_steps for chat[0] returns the two steps in that order

  Scenario: Error path — a precedence note without its two steps (ADR-107)
    Given chat[0].context [{kind: transcript}, {kind: precedence-note, text: "The channel's documents win over history."}]
    When load_config is called
    Then it returns CFG_CONTEXT_STEP_INVALID with location "chat[0].context[1]"

  Scenario: Error path — a lookup step names an unknown source (ADR-107)
    Given chat[0].context [{kind: knowledge-lookup, source: history, max-bytes: 20000}] and no knowledge source "history"
    When load_config is called
    Then it returns CFG_UNKNOWN_SOURCE with location "chat[0].context[0].source"

  Scenario: Error path — a connector plug-in is not carried
    Given the example file with tools.connectors holding one item of kind kafka
    When load_config is called
    Then it returns CFG_CONNECTOR_PLUGIN_NOT_CARRIED with location "tools.connectors[0]"

  Scenario: Error path — a schedule shorter than the run limit
    Given the example file with schedules [{duty: watch-queue, command: watch-queue, cron: "*/15 * * * *"}]
    When load_config is called
    Then it returns CFG_SCHEDULE_TOO_FREQUENT whose message states 15 minutes and 30 minutes

  Scenario: Error path — a writable bundle path
    Given the example file with writable-paths [/knowledge/]
    When load_config is called
    Then it returns CFG_BUNDLE_PATH_WRITABLE with location "writable-paths[0]"

  Scenario: Error path — a group approver with no identity provider
    Given the example file without the auth key
    When load_config is called
    Then it returns CFG_MISSING_KEY with location "auth" and a message naming the group approver

  Scenario: Error path — reach without a chat connection
    Given the example file without the chat key
    When load_config is called
    Then it returns CFG_INVALID_VALUE with location "reach"

  Scenario: Error path — a condition at the wrong point
    Given the example file with a step gate at point end holding the condition path-exists
    When load_config is called
    Then it returns CFG_UNKNOWN_KIND whose message reads "the condition path-exists is not available at the point end"

  Scenario: Error path — a pattern the regex crate refuses
    Given a message rule pattern "(?<=a)b"
    When load_config is called
    Then it returns CFG_INVALID_PATTERN

  Scenario: Happy path — check against the manifest
    Given the example configuration and a manifest whose commands are answer-ticket and triage and whose tool sections are Tickets and Supplier desk
    When check_against_manifest is called
    Then it returns Ok

  Scenario: Error path — a configuration control replaces a package gate
    Given a manifest whose declaration holds the step gate "ticket-before-mail"
    And a configuration whose controls.steps holds a gate named "ticket-before-mail"
    When check_against_manifest is called
    Then it returns CFG_GATE_OVERRIDE

  Scenario: Error path — a named file lies outside the configuration folder
    Given tools.http[0].openapi "../shared/tickets.yaml"
    When named_files is called with the configuration path "/srv/agents/support/support-agent.yaml"
    Then it returns CFG_NAMED_FILE_OUTSIDE with location "tools.http[0].openapi"

  Scenario: Edge case — the declaration helpers raise DEC codes
    Given a list of settings with a selection that has both options and lookup
    When check_setting_entries is called with Prefix::Dec
    Then it returns the code DEC_INVALID_VALUE

  Scenario: A list-backed link action loads (ADR-107)
    Given a list setting "workflows" with fields name, link and words
    And a reply action of kind link with list-setting workflows, label-field name, url-field link, keywords-field words and style workflow-link
    When check_against_manifest runs
    Then the configuration loads with that action's style WorkflowLink

  Scenario: A list-backed link names a field the list lacks (ADR-107)
    Given the same action with url-field "url"
    Then the check fails with CFG_ACTION_LIST_INVALID naming chat[0].reply-actions[0].url-field and listing name, link, words

  Scenario: A link with both a url-setting and a list-setting is refused (ADR-107)
    Given a reply action of kind link with url-setting runbook-link and list-setting workflows
    Then the check fails with CFG_INVALID_VALUE

  Scenario: A hand-off with only a target channel loads (ADR-107)
    Given a reply action of kind hand-off with target-channel-setting escalation-channel, a declared text setting
    Then the configuration loads

  Scenario: A hand-off enabled setting must default to true (ADR-107)
    Given a hand-off with enabled-setting hand-off-on, a flag setting declared with no default
    Then the check fails with CFG_INVALID_VALUE

  Scenario: The working signal has a default (ADR-107)
    Given a chat connection with no status key
    Then its status working is "hourglass_flowing_sand"
```

## Performance / Security / Observability

- **Performance targets:** `load_config` of a 20 KB file (a typical configuration) completes in under 20 ms p95 on one core; of a 1 MiB file in under 300 ms p95. The cron test computes at most `CRON_FIRINGS` (1,000) firings per expression. Peak memory of one call under 32 MiB. The functions are synchronous and CPU-bound; callers on the tokio runtime call them through `tokio::task::spawn_blocking` when the file exceeds 64 KiB *(assumed here: below that the call ends within one scheduler tick)*.
- **Security:** No entry point is reachable over a network; the callers are the ordin server's sub-commands and start sequence, run by the deploying organisation. Input validation is the whole purpose: unknown keys refused at every depth; every credential is a variable name, never a value, and a value that looks like a credential is refused; addresses with user information are refused; patterns are compiled with a size ceiling and matched by the `regex` crate, whose time grows with the text length only (`05:44`); named files must lie inside the configuration's folder; YAML is read only through `ordin_types::yaml::read_restricted`, which refuses anchors, aliases, merge keys and tags outside YAML 1.2's core schema (so a small file cannot expand into a large tree) and checks the size before parsing. No error message ever holds a value read from the environment or a secret, and no value of a key is logged. *Amended January 2026 (ADR-107):* a `model.environment` name that ordin sets, copies into or keeps out of the harness child is refused, so a configuration cannot replace the key route, the hook pairs or the copied system variables; a `literal` value never appears in an error, and a secret is named, never written.
- **Observability:** This crate writes no log line, metric or span (SA-XC-04/05 apply to the agent profile only through Telemetry; this crate is pure). Its callers log the outcome: the Release Builder as `tracing` event `config_checked` with fields `outcome` (`ok` or the code), `location`; the Agent Loader as part of `release_loaded`.

## Gaps

None.

## Amendment January 2026 — The knowledge agent section (ADR-113)

> Source: decision 92; HLD revision 0.61.0, ADR-113 (knowledge agents: ordin in the agent profile reads Git repositories into the one memory graph and answers retrieval for an audience; a team talks to one front door that asks proxies; amends ADR-097 and ADR-098 item 2: the configuration gains an optional key). Consumers: `components/KnowledgeReader.md`, `components/KnowledgeQueryServer.md`, `components/KnowledgeQueryClient.md` (new, January 2026), `components/CallerApi.md`, `components/CallerAuthentication.md` and `components/OrdinServer.md` (amendments January 2026).
> Every rule below adds to the sections above. A configuration without the key loads and behaves exactly as before; wrightrun's `knowledge[]` sources are unchanged and unrelated.

### Purpose (addition)

The configuration gains one optional top-level key, **`knowledge-agent`**, which makes the hosted agent a knowledge agent: the Git repositories and glossaries it reads, the audience it serves, the front doors allowed to ask it, the proxies it may ask, its reading budget and fetch interval, its hot-copy threshold, its read-only MCP endpoint and its optional sources record. This crate checks the section's shape, bounds and references, and refuses a repository the operator has not stated the whole audience may read, so that a knowledge agent never starts on a repository its audience might not be allowed to see (ADR-113: access by audience).

### Approach (addition)

The key is named `knowledge-agent`, not `knowledge`: wrightrun's `knowledge[]` (`KnowledgeSourceConfig`, the chat-history, entries and notes sources) already holds that key and stays compatible (ADR-098 item 2). The section is a plain `serde` struct with `deny_unknown_fields` and kebab-case keys, as every other section; its rules join the rules table and run in V-4 and a new step V-8a. Whether a repository can be read is not checked here: this crate makes no network call (*Public Interface*), so the Release Builder's check states what the file says and the Knowledge Reader reports each repository it cannot read at start and on each fetch. Rejected: *the check command asking each Git host which groups may read each repository* — each host answers differently, the agent's credential often may not see group grants, and the check would need network access this crate never has; the operator's statement per repository is checked here and the reader's own access is checked on every fetch.

Assumptions (*chosen here*):

- **SA-ACFG-KA-01** `audience-may-read: true` on every `repositories[]` and `glossaries[]` entry is the operator's statement that every member of every group in `audiences` may read that repository in Git. It is required, not defaulted, so that it is written on purpose; an entry without it, or with `false`, is refused.
- **SA-ACFG-KA-02** A knowledge agent needs `auth` (the identity provider's issuer, audience and keys), because every knowledge route checks the asker's token and group claim. `service-identity` is required only when `proxies` is non-empty (the front door then signs in to each proxy as itself).
- **SA-ACFG-KA-03** Bounds chosen here: `name` 1–80 characters; at most 200 repositories, 20 glossaries, 16 audiences, 32 front doors and 32 proxies; `max-proxies` 1–32 (default 8); `reading.daily-documents` 0–100,000 (default 1,000; 0 turns model reading off, pages are still read); `fetch-interval-minutes` 5–1,440 (default 15); `hot-copy.asks-per-day` 1–100,000 (default 200); `sources-record.retention-days` 1–3,650 (default 365); a sub-path 1–512 characters; a branch 1–255 characters.
- **SA-ACFG-KA-04** The same repository may not appear twice: two entries across `repositories` and `glossaries` with the same URL (compared lower-case without a trailing `/` or `.git`), branch and path are refused; the same URL with different paths is allowed (two parts of one repository).

### Shared Context (additions)

A repository address follows **the https rule**: scheme `https` only (a Git host over plain `http` is refused even on `localhost`), with no user name, password, query or fragment. Group names, front-door identities and proxy audiences are compared as exact strings, as the identity provider asserts them.

### Public Interface (additions)

Types in `ordin_types::domain::agent_profile` (rules of *Types* above: kebab-case keys, `deny_unknown_fields`, `Vec` default empty):

```rust
pub struct AgentConfig { /* … every existing field … */
    #[serde(default)] pub knowledge_agent: Option<KnowledgeAgentConfig>,   // YAML key `knowledge-agent` (ADR-113)
}

pub struct KnowledgeAgentConfig {
    pub name: String,                                              // 1..=80; shown to askers
    pub repositories: Vec<KnowledgeRepository>,                    // 1..=200
    #[serde(default)] pub glossaries: Vec<KnowledgeRepository>,    // 0..=20; listed order is precedence
    pub audiences: Vec<String>,                                    // 1..=16; each 1..=256; unique
    pub group_claim: String,                                       // 1..=128; a top-level claim of the token
    #[serde(default)] pub front_doors: Vec<String>,                // 0..=32 service-identity subjects allowed to ask as front doors; each 1..=256
    #[serde(default)] pub proxies: Vec<KnowledgeProxy>,            // 0..=32
    #[serde(default = "eight")] pub max_proxies: u32,              // 1..=32
    #[serde(default)] pub service_identity: Option<ServiceIdentity>,
    #[serde(default)] pub reading: ReadingConfig,
    #[serde(default = "fifteen")] pub fetch_interval_minutes: u32, // 5..=1_440
    #[serde(default)] pub hot_copy: HotCopyConfig,
    #[serde(default)] pub mcp: McpEndpointConfig,
    #[serde(default)] pub sources_record: SourcesRecordConfig,
}
pub struct KnowledgeRepository {
    pub url: String,                                   // the https rule
    #[serde(default)] pub path: Option<String>,        // 1..=512; no leading "/", no "..", no empty segment
    #[serde(default)] pub branch: Option<String>,      // 1..=255; a valid Git branch name; None = the remote's default branch
    pub credential_env: String,                        // the variable-name rule; a read token
    #[serde(default)] pub audience_may_read: bool,     // must be true (SA-ACFG-KA-01); absent reads false, refused in V-8a
}
pub struct KnowledgeProxy {
    pub name: String,                                  // SLUG; unique
    pub url: String,                                   // the https rule; the proxy's base address
    pub audience: String,                              // 1..=256; the token audience the proxy's `auth` expects
}
pub struct ServiceIdentity {
    pub token_url: String,                             // the https rule; the identity provider's token endpoint
    pub client_id: String,                             // 1..=256
    pub client_secret_env: String,                     // the variable-name rule
    #[serde(default)] pub scope: Option<String>,       // 1..=512
}
pub struct ReadingConfig { #[serde(default = "thousand")] pub daily_documents: u32 }            // 0..=100_000
pub struct HotCopyConfig {
    #[serde(default = "two_hundred")] pub asks_per_day: u32,                                    // 1..=100_000
    #[serde(default)] pub credential_env: Option<String>,                                       // None = hot copies off
}
pub struct McpEndpointConfig { #[serde(default = "yes")] pub enabled: bool }                    // default true
pub struct SourcesRecordConfig {
    #[serde(default)] pub enabled: bool,                                                        // default false
    #[serde(default = "three_hundred_sixty_five")] pub retention_days: u32,                     // 1..=3_650
}
```

Keys that hold a variable name (2D-1d, added): `knowledge-agent.repositories[].credential-env`, `.glossaries[].credential-env`, `.service-identity.client-secret-env`, `.hot-copy.credential-env`. `connection_hosts` (V-15) gains one row per repository, glossary and proxy, `HostKind::Git` for repositories and glossaries and `HostKind::Knowledge` for proxies, after the existing rows (`HostKind` gains the two variants).

```rust
/// The knowledge-agent section, when present (V-20). The program fills the Knowledge Reader, Knowledge Query
/// Server and Knowledge Query Client from it.
pub fn knowledge_agent(config: &AgentConfig) -> Option<&KnowledgeAgentConfig>;
```

**Called by** (additions):

| Public item | Called by |
|---|---|
| `KnowledgeAgentConfig` and its section types | the server program, which builds the Knowledge Reader, Knowledge Query Server and Knowledge Query Client from it (OrdinServer.md amendment January 2026); Knowledge Reader, Knowledge Query Server and Knowledge Query Client (they import the types) |
| `knowledge_agent` | the server program; Agent Loader (start-up report: repositories, glossaries, audiences, MCP endpoint and sources record lines) |

**Rules of the keys (rows added, marked ordin):**

| Key | Rule | Code on breach |
|---|---|---|
| **ordin** `knowledge-agent` | needs `auth` (SA-ACFG-KA-02) | `CFG_MISSING_KEY` |
| **ordin** `knowledge-agent.name` | 1 to 80 characters, no control character | `CFG_INVALID_VALUE` |
| **ordin** `knowledge-agent.repositories[]` | 1 to 200 items | `CFG_MISSING_KEY` (none) / `CFG_INVALID_VALUE` |
| **ordin** `knowledge-agent.glossaries[]` | 0 to 20 items | `CFG_INVALID_VALUE` |
| **ordin** `…repositories[].url`, `…glossaries[].url` | the https rule | `CFG_INVALID_VALUE` |
| **ordin** `…repositories[].path`, `…glossaries[].path` | 1 to 512 characters; no leading `/`; no `..` or empty segment | `CFG_INVALID_VALUE` |
| **ordin** `…repositories[].branch`, `…glossaries[].branch` | 1 to 255 characters; no white space, `~`, `^`, `:`, `?`, `*`, `[`, `\`, `..`, `@{`; not ending `.lock` or `/` | `CFG_INVALID_VALUE` |
| **ordin** `…repositories[].credential-env`, `…glossaries[].credential-env` | the variable-name rule | `CFG_INVALID_VALUE` |
| **ordin** `…repositories[].audience-may-read`, `…glossaries[].audience-may-read` | present and `true` (SA-ACFG-KA-01) | `CFG_KNOWLEDGE_AUDIENCE_UNATTESTED` |
| **ordin** repositories and glossaries together | no two with the same URL, branch and path (SA-ACFG-KA-04) | `CFG_DUPLICATE` |
| **ordin** `knowledge-agent.audiences[]` | 1 to 16 items; each 1 to 256 characters; unique | `CFG_MISSING_KEY` / `CFG_INVALID_VALUE` / `CFG_DUPLICATE` |
| **ordin** `knowledge-agent.group-claim` | 1 to 128 characters | `CFG_INVALID_VALUE` |
| **ordin** `knowledge-agent.front-doors[]` | 0 to 32 items; each 1 to 256 characters; unique | `CFG_INVALID_VALUE` / `CFG_DUPLICATE` |
| **ordin** `knowledge-agent.proxies[]` | 0 to 32 items; `name` `SLUG`, unique; `url` the https rule, unique; `audience` 1 to 256 characters | `CFG_INVALID_VALUE` / `CFG_DUPLICATE` |
| **ordin** `knowledge-agent.service-identity` | required when `proxies` is non-empty; `token-url` the https rule; `client-id` 1 to 256; `client-secret-env` the variable-name rule; `scope` 1 to 512 | `CFG_MISSING_KEY` / `CFG_INVALID_VALUE` |
| **ordin** `knowledge-agent.max-proxies` | 1 to 32 | `CFG_INVALID_VALUE` |
| **ordin** `knowledge-agent.reading.daily-documents` | 0 to 100,000 | `CFG_INVALID_VALUE` |
| **ordin** `knowledge-agent.fetch-interval-minutes` | 5 to 1,440 | `CFG_INVALID_VALUE` |
| **ordin** `knowledge-agent.hot-copy.asks-per-day` | 1 to 100,000 | `CFG_INVALID_VALUE` |
| **ordin** `knowledge-agent.hot-copy.credential-env` | the variable-name rule | `CFG_INVALID_VALUE` |
| **ordin** `knowledge-agent.sources-record.retention-days` | 1 to 3,650 | `CFG_INVALID_VALUE` |

#### Example (addition)

```yaml
knowledge-agent:
  name: Supply knowledge
  audiences: [supply-staff]
  group-claim: groups
  front-doors: [svc-knowledge-frontdoor-ops]
  glossaries:
    - {url: https://git.example.org/org/glossary, credential-env: GLOSSARY_TOKEN, audience-may-read: true}
  repositories:
    - {url: https://git.example.org/supply/handbook, path: policies, credential-env: HANDBOOK_TOKEN, audience-may-read: true}
    - {url: https://git.example.org/supply/contracts, branch: release, credential-env: CONTRACTS_TOKEN, audience-may-read: true}
  proxies:
    - {name: finance, url: https://finance-knowledge.example.org, audience: finance-knowledge}
  service-identity:
    token-url: https://idp.example.com/oauth2/token
    client-id: supply-knowledge
    client-secret-env: KNOWLEDGE_CLIENT_SECRET
  sources-record: {enabled: true, retention-days: 180}
```

- With the example file's `auth` beside it, `load_config` returns `Ok` with `knowledge_agent.max_proxies` 8, `reading.daily_documents` 1,000, `fetch_interval_minutes` 15, `hot_copy.asks_per_day` 200 with no credential (hot copies off), `mcp.enabled` true and `sources_record` `{enabled: true, retention_days: 180}`.
- Without `audience-may-read` on the contracts entry: `error[CFG_KNOWLEDGE_AUDIENCE_UNATTESTED]: state that every member of the audience may read this repository in Git with audience-may-read: true; a knowledge agent serves its whole audience (knowledge-agent.repositories[1].audience-may-read)`.

### Internal Logic (additions)

**V-4 (addition).** The rows above run in the rules table's order, after the existing rows.

**V-8a Knowledge agent** (after V-8; only when `knowledge_agent` is `Some`):

1. `auth` absent → `CFG_MISSING_KEY` ("a knowledge agent checks each asker's token and groups; add auth", `location = "auth"`).
2. For each entry of `glossaries` then `repositories`, in file order: `audience_may_read` false → `CFG_KNOWLEDGE_AUDIENCE_UNATTESTED` (message as the example, `location = "knowledge-agent.<list>[i].audience-may-read"`). A missing `audience-may-read` key is reported here with the same code, not as `CFG_MISSING_KEY`: the field is read with `#[serde(default)]` for this purpose only, so the operator reads the reason rather than a bare missing key.
3. Duplicates per SA-ACFG-KA-04, the later entry named (`CFG_DUPLICATE`, "the repository <url> is listed twice with the same branch and path").
4. `proxies` non-empty and `service_identity` absent → `CFG_MISSING_KEY` ("a front door signs in to its proxies as itself; add service-identity", `location = "knowledge-agent.service-identity"`).
5. Nothing else: whether a repository, a proxy or the token endpoint answers is not checked here (*Approach*).

**V-15 (addition).** After the existing rows: one `ConnectionHost { connection: "knowledge-agent.glossaries[i]" | "knowledge-agent.repositories[i]", kind: Git, host }` per entry, then `{ connection: <proxy name>, kind: Knowledge, host }` per proxy.

**V-20 `knowledge_agent(config)`.** Returns `config.knowledge_agent.as_ref()`.

### Data Model (no change)

N/A — the crate still owns no table. Adding an optional key is not a breaking change of the format (ADR-098 item 2); an installation before revision 0.61.0 refuses a file that holds the key (`CFG_UNKNOWN_KEY`), which is the intended signal that the release is too old for a knowledge agent.

### Error Table (additions)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| **ordin** a knowledge repository or glossary without `audience-may-read: true` (ADR-113) | `Err(permanent)` | `CFG_KNOWLEDGE_AUDIENCE_UNATTESTED` | `error[CFG_KNOWLEDGE_AUDIENCE_UNATTESTED]: state that every member of the audience may read this repository in Git with audience-may-read: true; a knowledge agent serves its whole audience (knowledge-agent.repositories[1].audience-may-read)` |
| **ordin** a knowledge agent without `auth`, or with proxies and no `service-identity` (ADR-113) | `Err(permanent)` | `CFG_MISSING_KEY` | `error[CFG_MISSING_KEY]: a knowledge agent checks each asker's token and groups; add auth (auth)` |
| **ordin** a knowledge repository over `http`, or with a bad path, branch, bound or variable name (ADR-113) | `Err(permanent)` | `CFG_INVALID_VALUE` | `error[CFG_INVALID_VALUE]: a knowledge repository is read over https only (knowledge-agent.repositories[0].url)` |
| **ordin** a repository listed twice, or a repeated audience, front door or proxy (ADR-113) | `Err(permanent)` | `CFG_DUPLICATE` | `error[CFG_DUPLICATE]: the repository https://git.example.org/supply/handbook is listed twice with the same branch and path (knowledge-agent.repositories[2])` |

### Acceptance Criteria (additions)

```gherkin
Feature: Agent Configuration — knowledge agent section

  Scenario: Happy path — the knowledge example loads with its defaults
    Given the example file with the knowledge-agent example added
    When load_config is called
    Then it returns Ok with max_proxies 8, daily_documents 1000, fetch_interval_minutes 15, mcp enabled true
    And hot_copy.credential_env is None
    And connection_hosts lists 3 rows of kind Git and 1 row of kind Knowledge after the existing rows

  Scenario: A configuration without the key behaves as before
    Given the example file without knowledge-agent
    When load_config is called
    Then knowledge_agent(config) is None and every existing derived value is unchanged

  Scenario: Error path — a repository the operator has not attested
    Given knowledge-agent.repositories[1] without audience-may-read
    When load_config is called
    Then it fails with CFG_KNOWLEDGE_AUDIENCE_UNATTESTED at knowledge-agent.repositories[1].audience-may-read

  Scenario: Error path — audience-may-read false
    Given knowledge-agent.glossaries[0] with audience-may-read false
    When load_config is called
    Then it fails with CFG_KNOWLEDGE_AUDIENCE_UNATTESTED at knowledge-agent.glossaries[0].audience-may-read

  Scenario: Error path — a knowledge agent without auth
    Given the knowledge-agent example and no auth key
    When load_config is called
    Then it fails with CFG_MISSING_KEY at auth

  Scenario: Error path — proxies without a service identity
    Given the knowledge-agent example without service-identity
    When load_config is called
    Then it fails with CFG_MISSING_KEY at knowledge-agent.service-identity

  Scenario: Error path — a repository over http
    Given knowledge-agent.repositories[0].url "http://git.example.org/supply/handbook"
    When load_config is called
    Then it fails with CFG_INVALID_VALUE at knowledge-agent.repositories[0].url

  Scenario: Error path — the same repository listed as a glossary and a repository
    Given a glossary and a repository both "https://git.example.org/org/glossary.git/" with no branch and no path
    When load_config is called
    Then it fails with CFG_DUPLICATE naming the later entry

  Scenario: Edge case — the same repository with two paths is allowed
    Given two repositories with URL "https://git.example.org/supply/handbook" and paths "policies" and "procedures"
    When load_config is called
    Then it returns Ok

  Scenario: Edge case — a token written as a value is refused
    Given knowledge-agent.repositories[0].credential-env "ghp_abc123"
    When load_config is called
    Then it fails with CFG_INVALID_VALUE saying it must be the name of an environment variable
```

### Performance / Security / Observability (additions)

- **Performance targets:** unchanged; the section adds at most 300 entries to check, within the existing 20 ms p95 for a 20 KB file.
- **Security:** every credential of the section is a variable name, never a value; repository and token addresses must be `https`; a repository is accepted only with the operator's statement that the whole audience may read it, so a knowledge agent cannot be configured, by omission, to serve content to people who may not read its source; front doors and audiences are exact identities, with no pattern.
- **Observability:** unchanged; this crate still writes nothing. The Agent Loader's start-up report lists the knowledge agent's repositories and glossaries with their hosts, its audiences, whether its MCP endpoint is on, and whether it keeps a sources record and for how long.

### Gaps

None.

## Amendment January 2026 — Sources read per person (ADR-115)

> Source: decision 94 (the user's decisions of January 2026); HLD revision 0.63.0, ADR-115 (knowledge inherits its readers: a knowledge agent may read a document only part of its audience may open, and serves what it derived only to the people the document's own system lets open it, checked when they ask; amends ADR-113's access by audience). Consumers: `components/SourceAccess.md` (new, January 2026), `components/KnowledgeReader.md`, `components/KnowledgeQueryServer.md`, `components/OrdinServer.md` (amendments January 2026).
> Every rule below adds to or replaces, where it says so, the ADR-113 amendment above. A configuration in which every repository and glossary carries `audience-may-read: true` and which has no `confluence`, `sharepoint`, `access-checks` or `notices` key loads with the same values as before this amendment, and the agent makes no per-person check.

### Purpose (addition)

The `knowledge-agent` section may now name sources that only part of the audience may open: a Git repository marked `audience-may-read: false`, a Confluence space and a SharePoint library. For each such source the knowledge agent checks, when a person asks, whether the source's own system lets that person open the document behind each fact, and shows the fact only when it does (ADR-115). This crate checks the shape, bounds and references of the new keys; it still makes no network call and never decides who may read anything.

### Approach (addition)

`audience-may-read` stays required on every entry, so that every source carries a written statement; what changes is that `false` is now a valid statement for a Git repository — it means "check each person" — while a glossary must still be readable by the whole audience, because glossary names become the names every answer uses. A per-person Git repository must say which host kind it is on, because GitHub and GitLab answer "may this person read this?" with different calls (SourceAccess.md). Confluence and SharePoint get their own optional sub-sections because their credentials and units differ from Git's: a Confluence space is read with the agent's own Atlassian account, and its pages can carry view restrictions that only an account with the Confluence Administrator permission may check for someone else, so `person-checks` is off by default and a space the whole audience may not view is refused while it is off; a SharePoint library is read with the agent's own Microsoft account and checked through the agent's own app registration, every file of a library per person. Rejected: *defaulting `audience-may-read` to `false` now that `false` is valid* — an entry written without thought would silently become a per-person source, and the operator would never have stated anything; *one `sources[]` list for every system* — each system's keys differ in kind and in rule, and a tagged union would hide which keys belong to which system from the operator reading the file.

Assumptions (*chosen here*; each listed in Phase 1 1A):

- **SA-ACFG-PD-01** *(replaces SA-ACFG-KA-01)* `audience-may-read` is required on every `repositories[]`, `glossaries[]` and `confluence.spaces[]` entry. `true` is the operator's statement that every member of every group in `audiences` may open everything under that entry in its system. `false` (repositories and spaces only) states that only some of them may, and makes the entry a **per-person source**: the knowledge agent checks each asker against the system before showing anything derived from it. An entry without the key is refused; a glossary with `false` is refused. A SharePoint library has no `audience-may-read` key (SA-ACFG-PD-05).
- **SA-ACFG-PD-02** A repository with `audience-may-read: false` must carry `host-kind` (`github` or `gitlab`); the key is accepted, and ignored, on a repository the whole audience may read.
- **SA-ACFG-PD-03** `check-every-question: true` asks Source Access to keep no remembered answer for the entry's documents. It is accepted only on a per-person entry; on an entry with `audience-may-read: true` it is refused, because no check happens there.
- **SA-ACFG-PD-04** Bounds chosen here: `confluence.spaces` 1–100, `sharepoint.libraries` 1–100, a space key 1–255 characters, a library name 1–255 characters, a library path 1–512 characters with the repository path rule, `access-checks.remember-seconds` 0–300 (default 60; 0 keeps nothing). `tenant-id` and `client-id` are GUIDs in the 8-4-4-4-12 hexadecimal form, compared lower-case.
- **SA-ACFG-PD-05** A SharePoint library is always a per-person source and carries no `audience-may-read` key: a SharePoint owner can narrow the sharing of one file inside a library the whole audience may open, and nothing at the library level shows it cheaply (SourceAccess.md SA-SRC-12). The key written on a library is refused as an unknown key.
- **SA-ACFG-PD-07** Every per-person Git repository of one host kind is on one host: all `host-kind: github` per-person repositories share one host, and all `host-kind: gitlab` per-person repositories share one host, because a person links one account per system (ADR-115) and an account on one GitLab or GitHub host means nothing on another. Whole-audience repositories may be on any host.
- **SA-ACFG-PD-06** The same Confluence space key may appear once; the same SharePoint library (site URL compared lower-case without a trailing `/`, library name compared case-insensitively, and path) may appear once.

### Shared Context (additions)

A Confluence site and a SharePoint site URL follow **the https rule** above. A GUID matches `^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$`.

### Public Interface (additions and changes)

```rust
pub struct KnowledgeAgentConfig { /* … every field of the ADR-113 amendment … */
    #[serde(default)] pub confluence: Option<KnowledgeConfluenceConfig>,
    #[serde(default)] pub sharepoint: Option<KnowledgeSharePointConfig>,
    #[serde(default)] pub access_checks: AccessChecksConfig,
    #[serde(default)] pub notices: NoticesConfig,
}

pub struct KnowledgeRepository {
    pub url: String,
    #[serde(default)] pub path: Option<String>,
    #[serde(default)] pub branch: Option<String>,
    pub credential_env: String,
    /// Changed from `bool` (ADR-113): `None` = the key was not written, refused in V-8a (SA-ACFG-PD-01).
    #[serde(default)] pub audience_may_read: Option<bool>,
    #[serde(default)] pub host_kind: Option<GitHostKind>,          // required when audience_may_read == Some(false)
    #[serde(default)] pub check_every_question: bool,              // only with audience_may_read == Some(false)
}
impl KnowledgeRepository {
    /// True when the entry is a per-person source (`audience-may-read: false`). Valid only after `load_config` returned Ok.
    pub fn per_person(&self) -> bool;
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "kebab-case")]
pub enum GitHostKind { Github, Gitlab }

pub struct KnowledgeConfluenceConfig {
    pub site: String,                                  // the https rule; e.g. https://acme.atlassian.net
    pub account_email_env: String,                     // the variable-name rule; the agent's own Atlassian account
    pub token_env: String,                             // the variable-name rule; that account's API token
    #[serde(default)] pub person_checks: bool,         // default false; true only with the Confluence Administrator permission
    pub spaces: Vec<KnowledgeConfluenceSpace>,         // 1..=100
}
pub struct KnowledgeConfluenceSpace {
    pub key: String,                                   // 1..=255; unique
    #[serde(default)] pub audience_may_read: Option<bool>,
    #[serde(default)] pub check_every_question: bool,
}
pub struct KnowledgeSharePointConfig {
    pub tenant_id: String,                             // GUID
    pub client_id: String,                             // GUID; the agent's own Entra app registration
    pub client_secret_env: String,                     // the variable-name rule; used for the on-behalf-of exchange
    pub read_token_env: String,                        // the variable-name rule; refresh token of the agent's own Microsoft account
    pub libraries: Vec<KnowledgeSharePointLibrary>,    // 1..=100
}
pub struct KnowledgeSharePointLibrary {
    pub site_url: String,                              // the https rule
    pub library: String,                               // 1..=255; the document library's display name
    #[serde(default)] pub path: Option<String>,        // 1..=512; the repository path rule
    #[serde(default)] pub check_every_question: bool,  // always a per-person source (SA-ACFG-PD-05)
}
pub struct AccessChecksConfig { #[serde(default = "sixty")] pub remember_seconds: u32 }   // 0..=300
pub struct NoticesConfig {
    #[serde(default)] pub github_secret_env: Option<String>,   // the variable-name rule
    #[serde(default)] pub gitlab_token_env: Option<String>,    // the variable-name rule
}
```

`HostKind` is unchanged: the Confluence and Microsoft hosts are listed with the existing `Http` kind, because a connection host's kind only says how the host is reached, and Source Access reaches both over HTTPS. Keys that hold a variable name (2D-1d, added): `knowledge-agent.confluence.account-email-env`, `.confluence.token-env`, `.sharepoint.client-secret-env`, `.sharepoint.read-token-env`, `.notices.github-secret-env`, `.notices.gitlab-token-env`. `connection_hosts` (V-15) gains, after the ADR-113 rows: `{connection: "knowledge-agent.confluence", kind: Http, host: <site host>}` when `confluence` is present; `{connection: "knowledge-agent.sharepoint", kind: Http, host: "graph.microsoft.com"}` and `{…, host: "login.microsoftonline.com"}` when `sharepoint` is present, then one `{connection: "knowledge-agent.sharepoint.libraries[i]", kind: Http, host: <site host>}` per library; per-person Git repositories need no new row (their host is already listed as `Git`).

**Called by** (additions):

| Public item | Called by |
|---|---|
| `KnowledgeConfluenceConfig`, `KnowledgeSharePointConfig`, `AccessChecksConfig`, `NoticesConfig`, `GitHostKind` | the server program, which builds Source Access from them (OrdinServer.md amendment January 2026); Source Access, Knowledge Reader and Knowledge Query Server (they import the types) |
| `KnowledgeRepository::per_person` | Knowledge Reader, Knowledge Query Server, Source Access |

**Rules of the keys (rows added or replaced, marked ordin):**

| Key | Rule | Code on breach |
|---|---|---|
| **ordin** `…repositories[].audience-may-read` *(replaces the ADR-113 row for repositories)* | present, `true` or `false` (SA-ACFG-PD-01) | `CFG_KNOWLEDGE_AUDIENCE_UNATTESTED` |
| **ordin** `…glossaries[].audience-may-read` *(replaces the ADR-113 row for glossaries)* | present (`CFG_KNOWLEDGE_AUDIENCE_UNATTESTED`) and `true` | `CFG_KNOWLEDGE_GLOSSARY_RESTRICTED` |
| **ordin** `…repositories[].host-kind` | `github` or `gitlab`; required when `audience-may-read` is `false` | `CFG_INVALID_VALUE` / `CFG_KNOWLEDGE_HOST_KIND_MISSING` |
| **ordin** per-person repositories (`audience-may-read: false`) | of one `host-kind`, all on one host (SA-ACFG-PD-07) | `CFG_KNOWLEDGE_HOST_MIXED` |
| **ordin** `…repositories[].check-every-question`, `…spaces[].check-every-question` | `true` only on an entry with `audience-may-read: false` (SA-ACFG-PD-03); on a library always allowed | `CFG_INVALID_VALUE` |
| **ordin** `…glossaries[].host-kind`, `…glossaries[].check-every-question` | absent | `CFG_UNKNOWN_KEY` |
| **ordin** `knowledge-agent.confluence.site` | the https rule | `CFG_INVALID_VALUE` |
| **ordin** `knowledge-agent.confluence.account-email-env`, `.token-env` | present; the variable-name rule | `CFG_MISSING_KEY` / `CFG_INVALID_VALUE` |
| **ordin** `knowledge-agent.confluence.spaces[]` | 1 to 100 items; `key` 1 to 255 characters, no white space, unique | `CFG_MISSING_KEY` (none) / `CFG_INVALID_VALUE` / `CFG_DUPLICATE` |
| **ordin** `…confluence.spaces[].audience-may-read` | present (`CFG_KNOWLEDGE_AUDIENCE_UNATTESTED`); `false` only when `confluence.person-checks` is `true` | `CFG_KNOWLEDGE_CONFLUENCE_RESTRICTED_SPACE` |
| **ordin** `knowledge-agent.sharepoint` | `tenant-id`, `client-id`, `client-secret-env`, `read-token-env` and `libraries` all present | `CFG_KNOWLEDGE_SHAREPOINT_INCOMPLETE` |
| **ordin** `…sharepoint.tenant-id`, `…sharepoint.client-id` | a GUID | `CFG_INVALID_VALUE` |
| **ordin** `…sharepoint.client-secret-env`, `…sharepoint.read-token-env` | the variable-name rule | `CFG_INVALID_VALUE` |
| **ordin** `…sharepoint.libraries[]` | 1 to 100 items; `site-url` the https rule; `library` 1 to 255 characters; `path` the repository path rule; no two the same (SA-ACFG-PD-06) | `CFG_INVALID_VALUE` / `CFG_DUPLICATE` |
| **ordin** `…sharepoint.libraries[].audience-may-read` | absent (SA-ACFG-PD-05) | `CFG_UNKNOWN_KEY` |
| **ordin** `knowledge-agent.access-checks.remember-seconds` | 0 to 300 | `CFG_INVALID_VALUE` |
| **ordin** `knowledge-agent.notices.github-secret-env`, `.gitlab-token-env` | the variable-name rule | `CFG_INVALID_VALUE` |

#### Example (addition)

```yaml
knowledge-agent:
  name: Supply knowledge
  audiences: [supply-staff]
  group-claim: groups
  glossaries:
    - {url: https://git.example.org/org/glossary, credential-env: GLOSSARY_TOKEN, audience-may-read: true}
  repositories:
    - {url: https://git.example.org/supply/handbook, path: policies, credential-env: HANDBOOK_TOKEN, audience-may-read: true}
    - {url: https://github.com/acme-supply/contracts, credential-env: CONTRACTS_TOKEN, audience-may-read: false, host-kind: github}
    - {url: https://gitlab.example.org/supply/pricing, credential-env: PRICING_TOKEN, audience-may-read: false, host-kind: gitlab, check-every-question: true}
  confluence:
    site: https://acme.atlassian.net
    account-email-env: CONFLUENCE_AGENT_EMAIL
    token-env: CONFLUENCE_AGENT_TOKEN
    spaces:
      - {key: SUPPLY, audience-may-read: true}
  sharepoint:
    tenant-id: 3f2c8a10-6b1e-4d2a-9c33-0a7b5e4d1f20
    client-id: 9b1d7e42-2c5a-4f8e-b6d1-4e0c3a2f7b19
    client-secret-env: SHAREPOINT_APP_SECRET
    read-token-env: SHAREPOINT_AGENT_REFRESH
    libraries:
      - {site-url: https://acme.sharepoint.com/sites/supply, library: Documents, path: Suppliers}
  access-checks: {remember-seconds: 60}
  notices: {github-secret-env: GITHUB_NOTICE_SECRET}
```

- With `auth` beside it, `load_config` returns `Ok`; `repositories[1].per_person()` and `repositories[2].per_person()` are true, `repositories[0].per_person()` is false; `confluence.person_checks` is false; `access_checks.remember_seconds` is 60; `connection_hosts` ends with one `Http` row for `acme.atlassian.net`, two for `graph.microsoft.com` and `login.microsoftonline.com`, and one for `acme.sharepoint.com`.
- Without `host-kind` on `repositories[1]`: `error[CFG_KNOWLEDGE_HOST_KIND_MISSING]: a repository only part of the audience may read is checked per person; say whether it is on github or gitlab with host-kind (knowledge-agent.repositories[1].host-kind)`.
- With `audience-may-read: false` on the `SUPPLY` space: `error[CFG_KNOWLEDGE_CONFLUENCE_RESTRICTED_SPACE]: a Confluence space only part of the audience may view needs person-checks: true, which needs the Confluence Administrator permission for the agent's account (knowledge-agent.confluence.spaces[0].audience-may-read)`.

### Internal Logic (additions and changes)

**V-8a (changed).** Step 2 is replaced by:

2. For each entry of `glossaries`, then `repositories`, then `confluence.spaces`, in file order:
   1. `audience_may_read` is `None` → `CFG_KNOWLEDGE_AUDIENCE_UNATTESTED` (message: "state with audience-may-read: true or false whether every member of the audience may open this source", `location = "knowledge-agent.<list>[i].audience-may-read"`). The ADR-113 rule that a missing key is reported here, not as `CFG_MISSING_KEY`, still holds.
   2. A glossary with `Some(false)` → `CFG_KNOWLEDGE_GLOSSARY_RESTRICTED` ("a glossary names things for every answer, so the whole audience must be able to read it").
   3. A repository with `Some(false)` and no `host_kind` → `CFG_KNOWLEDGE_HOST_KIND_MISSING`.
   4. `check_every_question` true with `audience_may_read == Some(true)` → `CFG_INVALID_VALUE` ("check-every-question applies only to a source checked per person").
   5. A space with `Some(false)` while `confluence.person_checks` is false → `CFG_KNOWLEDGE_CONFLUENCE_RESTRICTED_SPACE`.
   6. After the loop: for each host kind, the hosts (URL host, lower-case, without port) of the per-person repositories of that kind; more than one → `CFG_KNOWLEDGE_HOST_MIXED` naming the first repository on a second host ("per-person repositories of one host kind must be on one host, because a person links one account per system; github.com is already used", `location = "knowledge-agent.repositories[i].url"`).

Steps 3 to 5 of V-8a are unchanged; then:

6. `sharepoint` present with any of `tenant_id`, `client_id`, `client_secret_env`, `read_token_env` empty, or `libraries` empty → `CFG_KNOWLEDGE_SHAREPOINT_INCOMPLETE` ("a SharePoint source needs tenant-id, client-id, client-secret-env, read-token-env and at least one library", `location = "knowledge-agent.sharepoint"`). Serde's own missing-field error for these keys is mapped to this code.
7. Duplicates per SA-ACFG-PD-06, the later entry named (`CFG_DUPLICATE`).
8. Nothing else: whether the agent's accounts may read a space or a library, and whether the Confluence account holds the Confluence Administrator permission, are not checked here; Source Access reports what it cannot read at start and on each cycle (*Approach* of the ADR-113 amendment).

**V-15 (addition).** The `Http` rows above, after the ADR-113 rows, in the order: Confluence, SharePoint token and Graph hosts, then libraries in file order.

**`per_person`.** Returns `self.audience_may_read == Some(false)`.

### Data Model (no change)

N/A — the crate still owns no table. The new keys are optional; an installation before revision 0.63.0 refuses a file holding any of them (`CFG_UNKNOWN_KEY`), and refuses `audience-may-read: false` (`CFG_KNOWLEDGE_AUDIENCE_UNATTESTED`), which is the intended signal that the release is too old for per-person sources.

### Error Table (additions)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| **ordin** a repository with `audience-may-read: false` and no `host-kind` (ADR-115) | `Err(permanent)` | `CFG_KNOWLEDGE_HOST_KIND_MISSING` | `error[CFG_KNOWLEDGE_HOST_KIND_MISSING]: a repository only part of the audience may read is checked per person; say whether it is on github or gitlab with host-kind (knowledge-agent.repositories[1].host-kind)` |
| **ordin** a glossary with `audience-may-read: false` (ADR-115) | `Err(permanent)` | `CFG_KNOWLEDGE_GLOSSARY_RESTRICTED` | `error[CFG_KNOWLEDGE_GLOSSARY_RESTRICTED]: a glossary names things for every answer, so the whole audience must be able to read it (knowledge-agent.glossaries[0].audience-may-read)` |
| **ordin** a Confluence space with `audience-may-read: false` while `person-checks` is off (ADR-115) | `Err(permanent)` | `CFG_KNOWLEDGE_CONFLUENCE_RESTRICTED_SPACE` | `error[CFG_KNOWLEDGE_CONFLUENCE_RESTRICTED_SPACE]: a Confluence space only part of the audience may view needs person-checks: true, which needs the Confluence Administrator permission for the agent's account (knowledge-agent.confluence.spaces[0].audience-may-read)` |
| **ordin** a `sharepoint` section missing a key or with no library (ADR-115) | `Err(permanent)` | `CFG_KNOWLEDGE_SHAREPOINT_INCOMPLETE` | `error[CFG_KNOWLEDGE_SHAREPOINT_INCOMPLETE]: a SharePoint source needs tenant-id, client-id, client-secret-env, read-token-env and at least one library (knowledge-agent.sharepoint)` |
| **ordin** an entry without `audience-may-read`, now including Confluence spaces (ADR-115; message changed) | `Err(permanent)` | `CFG_KNOWLEDGE_AUDIENCE_UNATTESTED` | `error[CFG_KNOWLEDGE_AUDIENCE_UNATTESTED]: state with audience-may-read: true or false whether every member of the audience may open this source (knowledge-agent.confluence.spaces[0].audience-may-read)` |
| **ordin** per-person repositories of one host kind on two hosts (ADR-115) | `Err(permanent)` | `CFG_KNOWLEDGE_HOST_MIXED` | `error[CFG_KNOWLEDGE_HOST_MIXED]: per-person repositories of one host kind must be on one host, because a person links one account per system; github.com is already used (knowledge-agent.repositories[3].url)` |
| **ordin** `check-every-question` on a whole-audience entry, a bad GUID, `remember-seconds` over 300 (ADR-115) | `Err(permanent)` | `CFG_INVALID_VALUE` | `error[CFG_INVALID_VALUE]: check-every-question applies only to a source checked per person (knowledge-agent.repositories[0].check-every-question)` |
| **ordin** a Confluence space or SharePoint library listed twice (ADR-115) | `Err(permanent)` | `CFG_DUPLICATE` | `error[CFG_DUPLICATE]: the Confluence space SUPPLY is listed twice (knowledge-agent.confluence.spaces[1])` |

### Acceptance Criteria (additions)

```gherkin
Feature: Agent Configuration — sources read per person

  Scenario: Happy path — the per-person example loads
    Given the example file with the per-person knowledge-agent example added
    When load_config is called
    Then it returns Ok
    And repositories[1].per_person() and repositories[2].per_person() are true and repositories[0].per_person() is false
    And access_checks.remember_seconds is 60 and confluence.person_checks is false
    And connection_hosts ends with 4 rows of kind Http

  Scenario: Backward compatibility — an ADR-113 configuration loads unchanged
    Given the ADR-113 knowledge-agent example, every entry with audience-may-read true and no new key
    When load_config is called
    Then it returns Ok with confluence None, sharepoint None, access_checks.remember_seconds 60, notices with no variable
    And no repository is per_person

  Scenario Outline: Error paths — per-person rules
    Given the per-person example changed so that <change>
    When load_config is called
    Then it fails with <code> at <location>

    Examples:
      | change | code | location |
      | repositories[1] has no host-kind | CFG_KNOWLEDGE_HOST_KIND_MISSING | knowledge-agent.repositories[1].host-kind |
      | glossaries[0] has audience-may-read false | CFG_KNOWLEDGE_GLOSSARY_RESTRICTED | knowledge-agent.glossaries[0].audience-may-read |
      | confluence.spaces[0] has audience-may-read false | CFG_KNOWLEDGE_CONFLUENCE_RESTRICTED_SPACE | knowledge-agent.confluence.spaces[0].audience-may-read |
      | sharepoint has no read-token-env | CFG_KNOWLEDGE_SHAREPOINT_INCOMPLETE | knowledge-agent.sharepoint |
      | sharepoint.libraries[0] has audience-may-read true | CFG_UNKNOWN_KEY | knowledge-agent.sharepoint.libraries[0].audience-may-read |
      | confluence.spaces[0] has no audience-may-read | CFG_KNOWLEDGE_AUDIENCE_UNATTESTED | knowledge-agent.confluence.spaces[0].audience-may-read |
      | repositories[3] is per person with host-kind github on github.example.org | CFG_KNOWLEDGE_HOST_MIXED | knowledge-agent.repositories[3].url |
      | repositories[0] has check-every-question true | CFG_INVALID_VALUE | knowledge-agent.repositories[0].check-every-question |
      | sharepoint.tenant-id is "acme" | CFG_INVALID_VALUE | knowledge-agent.sharepoint.tenant-id |
      | access-checks.remember-seconds is 301 | CFG_INVALID_VALUE | knowledge-agent.access-checks.remember-seconds |
      | confluence.spaces lists SUPPLY twice | CFG_DUPLICATE | knowledge-agent.confluence.spaces[1] |
      | glossaries[0] has host-kind gitlab | CFG_UNKNOWN_KEY | knowledge-agent.glossaries[0].host-kind |

  Scenario: Edge case — a Confluence space only part of the audience may view, with person checks on
    Given the per-person example with confluence.person-checks true and spaces[0] audience-may-read false
    When load_config is called
    Then it returns Ok

  Scenario: Edge case — remembering nothing
    Given access-checks.remember-seconds 0
    When load_config is called
    Then it returns Ok with remember_seconds 0

  Scenario: Edge case — host-kind on a whole-audience repository is accepted and ignored
    Given repositories[0] with audience-may-read true and host-kind github
    When load_config is called
    Then it returns Ok and repositories[0].per_person() is false

  Scenario: Edge case — a token written as a value is refused
    Given confluence.token-env "ATATT3xFfGF0abc"
    When load_config is called
    Then it fails with CFG_INVALID_VALUE saying it must be the name of an environment variable
```

### Performance / Security / Observability (additions)

- **Performance targets:** unchanged; the section adds at most 200 further entries, within the existing 20 ms p95 for a 20 KB file.
- **Security:** every new credential is a variable name, never a value; every new address must be `https`; every Git repository and Confluence space still carries a written statement of who may open it, and every SharePoint file is checked per person, and the only way to make a source per person is to write `false`, so no source becomes per person, or whole-audience, by omission; a glossary can never be per person; Confluence pages that only part of the audience may view are refused unless the operator states the agent holds the permission to check others.
- **Observability:** unchanged; this crate still writes nothing. The Agent Loader's start-up report gains three lines: per-person sources (count of repositories and spaces with `audience-may-read: false`, and of libraries), Confluence person checks (`on` or `off — pages with a view restriction are not read`), and the remembered-answer time (`remember-seconds`, or `none` for 0). The count of Confluence pages left out is the Knowledge Reader's (`ReaderStatus.pages_left_out`, KnowledgeReader.md amendment January 2026), shown by the server program's knowledge status, not by this crate.

### Gaps

None.
