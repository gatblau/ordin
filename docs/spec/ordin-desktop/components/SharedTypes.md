# SPEC: Shared types

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-107; promoted to Approved by the user January 2026) · amended January 2026 (ADR-109; promoted to Approved by the user January 2026) · amended January 2026 (ADR-111; promoted to Approved by the user January 2026) · amended January 2026 (ADR-113; promoted to Approved by the user January 2026) · amended January 2026 (Phase 1 build corrections: `EventKind` gains the two ADR-113 kinds, 21 in all; `PartKind` is ordered; Model Policy constructors; approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-types` · **Build phase:** 0 · **Programs:** D P A
> **Dependencies:** —

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

`ordin-types` is the one crate every other crate of the workspace depends on. It holds the Phase 2C shared types (`phase-2c-shared-types-domain.md`, and the parts of `phase-2c-shared-types-wire.md` that 2B-1 rule 2 does not give to `ordin-service-api` or `ordin-runtime-adapter`), and the small helpers every module needs to build and check them the same way: identifier minting and parsing, the millisecond UTC timestamp, text normalisation and length checks, the decimal-string and SHA-256 forms, base64url encoding, the keyset page form, the one restricted YAML reader (no anchors, aliases or custom tags), and the coded-error contract that lets a program turn any module's error into the Service API's error reply. It exists so that one identifier, one timestamp form and one validation rule set hold across the desktop, the personal server and the agent profile (SA-ID-01, SA-TIME-01, SA-JSON-01, SA-PAGE-01; ADR-096 item 1, ADR-100 item 3).

## Approach

The crate is data plus pure functions: no I/O, no async runtime, no logging, no global state. Types are grouped into modules that follow the 2C section headings, so a reader of 2C finds each type at a predictable path; the catalogue of types itself is 2C, which this spec cites rather than repeats, because this crate *is* 2C rendered as Rust. Where the two 2C parts define the same name twice, this spec settles each clash once (Internal Logic step 6) instead of letting two definitions compile under different paths unnoticed. Rejected: *one flat module with every type at the root* — 15 names clash between the two 2C parts and would not compile; *a separate crate per 2C section* — adds nine crates to the arch-test edge list for no ownership benefit, since no module owns a shared type.

## Shared Context

### Conventions copied from 2C (domain part, *Conventions binding every type below*; wire part §0)

- **Identifiers.** Every `…Id` is a newtype over a UUIDv7, serialised as the 36-character lowercase hyphenated string. A value that is not a version-7 UUID is refused with `ID_NOT_UUIDV7`. Minted with the `uuid` crate 1.28.0, feature `v7` (SA-ID-01).
- **Timestamps.** `Timestamp` wraps `chrono::DateTime<Utc>` and serialises as RFC 3339 with exactly three fractional digits and the `Z` suffix (`2026-01-10T08:15:02.120Z`). A value with an offset other than `Z` or without milliseconds is refused with `TS_NOT_RFC3339_MS_UTC` (SA-TIME-01). The Caller API's wrightrun form (microseconds with offset) is not produced here; it is the Caller API's own serialiser (SA-TIME-01 exception).
- **Case.** `#[serde(rename_all = "snake_case")]` on every struct and enum (SA-JSON-01).
- **Tagged enums.** `#[serde(tag = "kind", rename_all = "snake_case")]`, except where 2C names another tag (`decision`, `input`, `action`, `type`, `chart`, `of`, `by`).
- **Optional fields.** `#[serde(skip_serializing_if = "Option::is_none")]` on every `Option<T>`; empty `Vec<T>` fields are serialised.
- **Unknown fields.** `#[serde(deny_unknown_fields)]` on every type that enters the core from outside (Remote API bodies, hook requests, blocks, MCP tool arguments, workflow files). Types the core emits accept unknown fields on read.
- **Numbers.** Counters and sizes are `u32` or `u64`. Domain types carry no `f32`/`f64`; numbers in stored report data are `DecimalString`.
- **Text.** UTF-8, NFC-normalised at the boundary; control characters other than `\n` and `\t` are rejected. Text limits count Unicode scalar values unless stated as bytes.
- **Storage.** Field names are column names unchanged. A field whose type is a `Vec`, a struct or a data-carrying enum is stored as one JSON `TEXT` column of the same name unless the entity names a child table (2C LD-01).
- **Error codes.** `SCREAMING_SNAKE_CASE`, `<PREFIX>_<NAME>`, one registry (SA-ERR-01). Prefixes used by 2C types: `ID_`, `TS_`, `HASH_`, `NUM_`, `INPUT_`, `SVC_`, `RA_`, `RULE_`, `HOOK_`, `APPR_`, `START_`, `BLOCK_`, `WFF_` (renamed from `WF_`, decision 80 / D8), plus each entity's area prefix (`DESK_`, `SRV_`, `STEP_`, `USAGE_` and the others 2C lists per entity). The person never sees a code (`07-cross-cutting.md:21`).

### Shared rule table (2C domain part, copied)

| Shared rule | Rule | Error code |
|---|---|---|
| any `…Id` | UUID version 7, lowercase hyphenated | `ID_NOT_UUIDV7` |
| any `Timestamp` | RFC 3339, `Z`, three fractional digits | `TS_NOT_RFC3339_MS_UTC` |
| any `Sha256Hex` | exactly 64 chars `[0-9a-f]` | `HASH_NOT_SHA256_HEX` |
| any `DecimalString` | matches `^-?[0-9]{1,28}(\.[0-9]{1,10})?$` | `NUM_NOT_DECIMAL` |
| `MemoryThingId`, `MemoryLinkId` (*amended January 2026, ADR-111*) | `entity:` or `relationship:` followed by a lowercase hyphenated UUID of any version | `ID_MEMORY_REF_INVALID` |
| `InputUsed.name` | 1–120 chars | `INPUT_NAME_INVALID` |
| `InputUsed.location` | 1–2048 chars | `INPUT_LOCATION_INVALID` |
| `InputUsed.version_read` | 1–256 chars | `INPUT_VERSION_INVALID` |
| any display name not otherwise limited | 1–120 chars, trimmed, no control characters | `<AREA>_NAME_EMPTY`, `<AREA>_NAME_TOO_LONG` |
| any local file-system path | absolute, 1–4096 bytes UTF-8, no NUL | `<AREA>_PATH_INVALID` |
| any URL or remote address | absolute `https` URL, at most 2048 bytes | `<AREA>_URL_INVALID` |

### Types copied verbatim from 2C (domain part, conventions block)

```rust
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

/// RFC 3339, UTC, millisecond precision, `Z` suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub DateTime<Utc>);

/// SHA-256 as 64 lowercase hexadecimal characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Sha256Hex(pub String);

/// A base-10 decimal number written as text (`-12.50`), so no binary float enters the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DecimalString(pub String);

/// Who fixed a setting's current value (04:181, 04:185).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetBy { Person, Company }

/// A setting value with the mark of who set it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Setting<T> { pub value: T, pub set_by: SetBy }

/// An agent instance or a remote agent (crew members, run agents, workflow agents).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentRef {
    AgentInstance { agent_instance_id: AgentInstanceId },
    RemoteAgent { remote_agent_id: RemoteAgentId },
}

/// One input as a run or crew run used it: name, where it lives, version read (A-29, 10:41).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InputUsed {
    pub name: String,
    pub location: String,
    pub version_read: String,
}
```

Identifier names copied from 2C: the domain part's `uuid_id!` list (`BrandPackId` … `ScopeId`, then `TelegramPostRequestId`, `TelegramGrantId` added by decision 80: C-25; 73 names, `phase-2c-shared-types-domain.md` conventions block) plus the wire part's `BackendId`, `CommandRequestId`, `HookCallId`, `BlockId` (wire §0).

`RepairAction`, copied from 2C wire §1 (*CommandReply*) and placed in this crate (Internal Logic step 7):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairAction { TryAgain, SignInAgain, SendToIt }
```

`RunProgress`, `ProgressCount`, `StepMark`, `AdapterKind`, `AdapterVerb`, `RunTarget`, `RunLimits`, `ModelLevel`, `ThinkingLevel`, `ModelChoice`, copied from 2C wire §1 and §2 and placed in this crate by 2C wire §1 *Placement* (decision 80: C-11, C-17; Internal Logic step 5):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunProgress {
    /// One plain sentence, at most 160 characters.
    pub line: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<ProgressCount>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProgressCount {
    pub done: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepMark { Next, Now, Done, Skipped }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterKind { ClaudeLocal, CopilotLocal, HostedAgent, SlackThread }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterVerb { Start, Progress, Pause, Resume, Read, Cancel }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunTarget {
    AgentInstance { agent_instance_id: AgentInstanceId },
    RemoteAgent { remote_agent_id: RemoteAgentId },
}

/// Derives the union of both 2C parts' derive lists (the domain part adds PartialEq, Eq).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunLimits {
    pub wall_clock_secs: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_budget: Option<u32>,
    pub idle_timeout_secs: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelLevel { Automatic, Smaller, Standard, MostCapable }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingLevel { Low, Automatic, High }

/// The model a run starts on. `model` is the harness's own model name; `None` means the
/// harness's own automatic selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ModelChoice {
    pub level: ModelLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}
```

Event kinds: the 21 variants of 2C wire §1 `EventKind` (decision 80: C-16; amended January 2026, ADR-109: `language_changed`; amended January 2026, ADR-113: `knowledge_agents_changed`, `more_found`) — the ten of 2C wire *Names fixed by this spec* item 7 (`run_state_changed`, `run_progress`, `plan_changed`, `step_marked`, `start_up_report_ready`, `approval_requested`, `approval_decided`, `crew_round_changed`, `block_shown`, `reset`) and the eight of HLD 08 revision 0.49.0 (`server_state_changed`, `workspace_changed`, `needs_you_changed`, `block_available`, `locale_changed`, `brand_changed`, `setup_progress`, `sign_in_changed`), and `language_changed` (amended January 2026, ADR-109: a language's wording changed, emitted by the programs from Localisation), and the two of 2C wire *Names fixed by this spec* item 7 as amended for ADR-113 (amended January 2026): `knowledge_agents_changed` (a workspace's front doors changed, emitted by the Knowledge Query Client) and `more_found` (late results for one recall, emitted by the Memory Service); their payloads are the Service API spec's.

Pagination form (SA-PAGE-01): `{"items": [...], "next_cursor": "<opaque>" | null}`, fixed page size 100, cursor opaque base64 of the last row's sort key, at most 1,024 characters, a malformed cursor refused with `SRV_BAD_CURSOR`.

Libraries (all already in the build): `serde` 1, `serde_json` 1, `uuid` 1.28.0 (`v7`, `serde`), `chrono` 0.4 (`serde`), `thiserror`, `url` 2 (2C `PauseReason::DecisionElsewhere`), `icu` 2.3.1 (SA-I18N-02; its `normalizer` component), `base64` 0.22 (`memory/Cargo.toml:31`, `sandbox/sandbox-host/Cargo.toml` `[dependencies]`), `schemars` 1.2.3 behind feature `schema` (SA-API-02).

## Public Interface

### Crate layout

```
crates/ordin-types/
  Cargo.toml            features: default = [], schema = ["dep:schemars"]; dependencies include `saphyr` (YAML
                        parser, used only by `yaml.rs`) and `serde_json`
  src/lib.rs            pub mod declarations and `pub use` of the prelude below
  src/id.rs             uuid_id! and every identifier; IdError
  src/time.rs           Timestamp, parse_rfc3339_ms_utc, Timestamp::now
  src/text.rs           normalise_text, check_len, check_display_name, check_abs_path, check_https_url, AreaCodes
  src/num.rs            Sha256Hex, DecimalString and their checks
  src/encoding.rs       base64url helpers; canonical_json
  src/paging.rs         Page<T>, PageRequest, Cursor (module `ordin_types::paging`, 2C domain §0, C-15)
  src/error.rs          TypesError, CodedError, RepairAction
  src/yaml.rs           read_restricted, read_restricted_node, YamlNode, YamlError (module `ordin_types::yaml`, P4-1)
  src/common.rs         SetBy, Setting<T>, AgentRef, InputUsed
  src/domain/mod.rs     re-exports of the submodules below
  src/domain/identity.rs     2C part 1 § Identity and settings
  src/domain/agents.rs       2C part 1 § Agents and the catalogue
  src/domain/places.rs       2C part 1 § Workspaces, places and content
  src/domain/email.rs        2C part 1 § Email
  src/domain/engineering.rs  2C part 1 § Engineering mode and Craft
  src/domain/runs.rs         2C part 1 § Tasks, runs and crews
  src/domain/server.rs       2C part 1 § Server, channels and triggers
  src/domain/handover.rs     2C part 1 § Handover
  src/domain/agent_profile.rs 2C part 1 § Agent profile (ordin server, ADR-097, ADR-098), plus the
                        agent-configuration types the Agent Configuration, Model Policy and Agent
                        Loader specs add (see *Module paths*)
  src/wire/mod.rs       re-exports of the submodules below
  src/wire/run.rs       RunProgress, ProgressCount, StepMark, AdapterKind, AdapterVerb, RunTarget, RunLimits,
                        ModelLevel, ThinkingLevel, ModelChoice
  src/wire/events.rs    EventKind
  src/session.rs        ToolCaller (module `ordin_types::session`, 2C wire §2, decision 80)
  src/wire/rules.rs     2C part 2 §3 Rules Engine tool decision
  src/wire/hook.rs      2C part 2 §4 ordin hook protocol
  src/wire/approval.rs  2C part 2 §5 Approval card (and ApprovalPress)
  src/wire/startup.rs   2C part 2 §6 Start-up report items
  src/wire/workflow_file.rs 2C part 2 §8 Workflow file, format 3
```

Placement of 2C part 2 sections outside this crate (2B-1 rule 2):

| 2C wire section | Crate | Exception kept in `ordin-types` |
|---|---|---|
| §0 identifiers `BackendId`, `CommandRequestId`, `HookCallId`, `BlockId` | `ordin-types` (`id.rs`) | — |
| §1 Service API | `ordin-service-api` | `RunProgress`, `ProgressCount`, `StepMark`, `RepairAction` |
| §2 Runtime Adapter interface | `ordin-runtime-adapter` | `AdapterKind`, `AdapterVerb`, `RunTarget`, `RunLimits`, `ModelLevel`, `ThinkingLevel`, `ModelChoice` |
| §3, §4, §5, §6, §8 | `ordin-types` | — |
| §7 Block catalogue | `ordin-service-api` | — |
| §9 Types consumed from `memory/` and `sandbox/` | referenced only, never redefined | — |

Every type in the crate derives `Debug, Clone, Serialize, Deserialize`, and `schemars::JsonSchema` when feature `schema` is on (`#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]`). `ordin-service-api` turns the feature on, so every type a Service API command or event carries has a JSON Schema.

### `id.rs`

```rust
/// Declares one UUIDv7 identifier newtype. The serde form is the 36-character lowercase
/// hyphenated string. Deserialising refuses any UUID whose version is not 7.
#[macro_export]
macro_rules! uuid_id {
    ($($name:ident),* $(,)?) => {$(
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
        #[cfg_attr(feature = "schema", schemars(transparent))]
        pub struct $name(pub uuid::Uuid);

        impl $name {
            /// Mints a new identifier from the current time (UUIDv7).
            pub fn new() -> Self { Self(uuid::Uuid::now_v7()) }
            /// Parses the 36-character form; refuses anything but a lowercase hyphenated v7 UUID.
            pub fn parse(raw: &str) -> Result<Self, $crate::TypesError> {
                $crate::id::parse_v7(raw).map(Self)
            }
            pub fn as_uuid(&self) -> &uuid::Uuid { &self.0 }
        }
        impl Default for $name { fn default() -> Self { Self::new() } }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0.hyphenated())
            }
        }
        impl std::str::FromStr for $name {
            type Err = $crate::TypesError;
            fn from_str(s: &str) -> Result<Self, Self::Err> { Self::parse(s) }
        }
        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.collect_str(&self.0.hyphenated())
            }
        }
        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let raw = <std::borrow::Cow<'de, str>>::deserialize(d)?;
                Self::parse(&raw).map_err(serde::de::Error::custom)
            }
        }
    )*};
}

/// The domain part's identifiers (2C part 1 conventions block, in that order) and the wire part's four.
uuid_id!(
    BrandPackId, HarnessAttachmentId, OrganisationProfileId, AgentInstanceId,
    ExecutablePartDecisionId, PlaceId, FolderGrantId, WorkspaceId, ChangedMarkId,
    SourceLinkId, WorkingCopyItemId, EmailLinkId, EmailGrantId, EmailDraftId, SentEmailId,
    PendingPushId, MemoryFactId, KnowledgePageId, ConfluenceLinkId, PublishedCopyId,
    FileVersionId, ProjectId, GithubOperationSettingId, LanguagePluginId, CodingTaskId,
    AttemptGroupId, ChangeSetId, ChangeSetEntryId, CraftInstallationId, FileTypeMapEntryId,
    RemoteAgentId, TaskId, RunId, ReusedAnswerId, ToolRequestId, ApprovalId,
    ExternalActionKey, ArtefactId, CrewCharterId, CrewRunId, WorkflowId, ReportId,
    DashboardId, RunStepId, StartupReportId, CredentialId, ChannelPairingId,
    DesktopPairingId, ServerConnectionId, CardReferenceId, TriggerId, TriggerEventId,
    TelegramLinkId, KeptTelegramMessageId, TelegramDraftId, AgentRepositoryId,
    AgentDraftId, RehearsalId, PartId, PartDraftId, WithdrawnListEntryId, LifecycleId,
    WorkspaceStageId, HandoverId, HandoverPlaceId, VocabularyTermId, ServerProfileId,
    AgentReleaseId, RunRecordId, ScopeId,
    TelegramPostRequestId, TelegramGrantId,
    MemoryRecallId, // amended January 2026 (ADR-111)
    KnowledgeConnectionId, KnowledgeAnswerId, // amended January 2026 (ADR-113)
    BackendId, CommandRequestId, HookCallId, BlockId,
);

/// Shared parser behind every identifier.
pub fn parse_v7(raw: &str) -> Result<uuid::Uuid, TypesError>;
```

`Default` mints a fresh identifier *(chosen here: lets `#[derive(Default)]` on test fixtures compile; production code calls `new()` explicitly)*.

### `time.rs`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>);

impl Timestamp {
    /// The current time, truncated to whole milliseconds (so a value survives a round trip unchanged).
    pub fn now() -> Self;
    /// Builds from milliseconds since the Unix epoch; refuses values outside 1970-01-01..9999-12-31.
    pub fn from_unix_millis(ms: i64) -> Result<Self, TypesError>;
    pub fn unix_millis(&self) -> i64;
    /// `YYYY-MM-DDTHH:MM:SS.mmmZ`.
    pub fn to_wire(&self) -> String;
}
impl Serialize for Timestamp { /* writes to_wire() */ }
impl<'de> Deserialize<'de> for Timestamp { /* parse_rfc3339_ms_utc */ }
impl std::fmt::Display for Timestamp { /* to_wire() */ }

/// Refuses anything but `YYYY-MM-DDTHH:MM:SS.mmmZ` (`TS_NOT_RFC3339_MS_UTC`).
pub fn parse_rfc3339_ms_utc(raw: &str) -> Result<chrono::DateTime<chrono::Utc>, TypesError>;
```

### `text.rs`

```rust
/// The area-prefixed codes of one entity's name, path and URL rules (2C shared rule table).
#[derive(Debug, Clone, Copy)]
pub struct AreaCodes {
    pub name_empty: &'static str,     // e.g. "PLACE_NAME_INVALID"
    pub name_too_long: &'static str,  // e.g. "PLACE_NAME_INVALID"
    pub path_invalid: &'static str,   // e.g. "WS_PATH_INVALID"
    pub url_invalid: &'static str,    // e.g. "CONF_URL_INVALID"
}

/// NFC-normalises `raw` and refuses control characters other than `\n` and `\t`.
pub fn normalise_text(raw: &str) -> Result<String, TypesError>;

/// Counts Unicode scalar values; refuses fewer than `min` or more than `max` with `code`.
pub fn check_len(value: &str, min: usize, max: usize, code: &'static str) -> Result<(), TypesError>;

/// Trims, NFC-normalises, then applies 1..=max scalar values (max 120 unless the entity says
/// otherwise). Returns the normalised value to store.
pub fn check_display_name(raw: &str, max: usize, codes: AreaCodes) -> Result<String, TypesError>;

/// Absolute path, 1–4096 bytes of UTF-8, no NUL byte.
pub fn check_abs_path(raw: &str, codes: AreaCodes) -> Result<(), TypesError>;

/// Absolute `https` URL, at most 2048 bytes; `allow_loopback_http` admits `http://` to
/// `localhost` and `127.0.0.1` only (2D-1a `ORDIN_SERVER_URL`, `ORDIN_OTLP_ENDPOINT`).
pub fn check_https_url(raw: &str, allow_loopback_http: bool, codes: AreaCodes) -> Result<url::Url, TypesError>;
```

### `num.rs`

```rust
impl Sha256Hex {
    /// Exactly 64 characters of `[0-9a-f]`, else `HASH_NOT_SHA256_HEX`.
    pub fn parse(raw: &str) -> Result<Self, TypesError>;
    /// Lowercase hex of a 32-byte digest computed by the caller.
    pub fn from_digest(digest: &[u8; 32]) -> Self;
}
impl DecimalString {
    /// Matches `^-?[0-9]{1,28}(\.[0-9]{1,10})?$`, else `NUM_NOT_DECIMAL`.
    pub fn parse(raw: &str) -> Result<Self, TypesError>;
}
```

*Amended January 2026 (ADR-111).* Two string identifiers name the memory component's own records, whose ids the component derives (some by UUIDv5), so they are not `uuid_id!` types and are not version-checked:

```rust
/// The memory component's entity id, shown to the person as a "thing" (MemoryService.md, amendment January 2026).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MemoryThingId(pub String);   // ^entity:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$
/// The memory component's relationship id, shown as a "link".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MemoryLinkId(pub String);    // ^relationship:<the same UUID form>$
impl MemoryThingId { pub fn parse(raw: &str) -> Result<Self, TypesError>; }   // else Rule { code: "ID_MEMORY_REF_INVALID" }
impl MemoryLinkId  { pub fn parse(raw: &str) -> Result<Self, TypesError>; }
```

They live in `src/id.rs` beside `uuid_id!` and deserialise through `parse`.

`Sha256Hex` and `DecimalString` keep 2C's derived `Deserialize` shape but deserialise through `parse` (a custom `Deserialize` that calls it), so an invalid value never enters a type.

### `encoding.rs`

```rust
/// base64url without padding (RFC 4648 §5).
pub fn b64url_encode(bytes: &[u8]) -> String;
/// Refuses padding, characters outside the URL-safe alphabet, and a decoded length != `expected_len`.
pub fn b64url_decode_exact(raw: &str, expected_len: usize, code: &'static str) -> Result<Vec<u8>, TypesError>;
/// The one canonical JSON of the workspace (Phase 4 Logging defect, resolved): object keys sorted
/// by their UTF-8 bytes at every level, no insignificant white space, strings escaped and numbers
/// written as `serde_json` writes them. Used for the argument digest (Logging R-3), the hook MAC
/// (Hook Relay H, Ordin Hook E) and every hash the Agent Model takes (`ordin_agent_model::canonical_json`
/// re-exports this function).
pub fn canonical_json(value: &serde_json::Value) -> Vec<u8>;
```

Used for the card reference (16 bytes → 22 characters, 2C wire *Names* item 14), the desktop key (32 bytes → 43 characters) and the hook secret (43 characters, 2D-1c).

### `paging.rs` (`ordin_types::paging`)

```rust
pub const PAGE_SIZE: usize = 100;
pub const CURSOR_MAX_CHARS: usize = 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Page<T> {
    pub items: Vec<T>,
    /// Always serialised; `null` on the last page.
    pub next_cursor: Option<Cursor>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PageRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Cursor(String);

impl Cursor {
    /// base64url of the JSON form of the last row's sort key.
    pub fn encode<K: Serialize>(key: &K) -> Cursor;
    /// Refuses more than 1,024 characters, bad base64url, or JSON that is not a `K` (`SRV_BAD_CURSOR`).
    pub fn decode<K: serde::de::DeserializeOwned>(&self) -> Result<K, TypesError>;
}
```

`Page<T>.next_cursor` is the one `Option` the crate serialises as `null` rather than omitting, because SA-PAGE-01 fixes the key as always present.

### `error.rs`

```rust
/// Every validation failure this crate raises. `code()` is the registry code.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TypesError {
    #[error("identifier is not a version-7 UUID")]
    IdNotUuidV7,
    #[error("timestamp is not RFC 3339 UTC with milliseconds")]
    TimestampNotRfc3339MsUtc,
    #[error("hash is not 64 lowercase hexadecimal characters")]
    HashNotSha256Hex,
    #[error("number is not a decimal string")]
    NumNotDecimal,
    #[error("text contains a control character")]
    TextControlChar,
    #[error("cursor is not one this server issued")]
    BadCursor,
    #[error("value refused: {code}")]
    /// A rule whose code the caller supplied (lengths, names, paths, URLs, base64 lengths).
    Rule { code: &'static str },
}

impl TypesError {
    pub fn code(&self) -> &'static str; // ID_NOT_UUIDV7, TS_NOT_RFC3339_MS_UTC, HASH_NOT_SHA256_HEX,
                                        // NUM_NOT_DECIMAL, TEXT_CONTROL_CHAR, SRV_BAD_CURSOR, or `code`
}

/// The contract every module error type implements, so that a program can turn any module
/// error into a Service API error reply without knowing the module (SA-ERR-01).
pub trait CodedError: std::error::Error + Send + Sync + 'static {
    /// The registry code, `<PREFIX>_<NAME>`.
    fn code(&self) -> &'static str;
    /// True when the same call may succeed if repeated unchanged later.
    fn retryable(&self) -> bool;
    /// The one repair action shown to the person, if any (07 — Error handling).
    fn repair(&self) -> Option<RepairAction> { None }
}

impl CodedError for TypesError { /* code(); retryable() = false; repair() = None */ }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairAction { TryAgain, SignInAgain, SendToIt }
```

### `yaml.rs` (`ordin_types::yaml`; Phase 4 decision P4-1)

The one restricted YAML reader of the workspace. Every spec that reads a YAML file (Agent Model, Ordin Host, Catalogue Service, Handover Service and any other) calls it and maps its three codes to its own; no crate parses YAML another way.

```rust
/// Reads one YAML document into `T` under the restricted profile: no anchors, no aliases,
/// no tags other than the YAML 1.2 core-schema tags, at most `max_bytes` bytes of input.
pub fn read_restricted<T: serde::de::DeserializeOwned>(text: &str, max_bytes: usize) -> Result<T, YamlError>;

/// The same refusals and codes, returning an untyped tree whose scalars keep their exact source
/// text ("1.10" stays "1.10", "007" stays "007"); the Agent Model reads this form.
pub fn read_restricted_node(text: &str, max_bytes: usize) -> Result<YamlNode, YamlError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YamlNode {
    /// Keys in source order; each key is a scalar's source text.
    Map(Vec<(String, YamlNode)>),
    Seq(Vec<YamlNode>),
    /// `quoted` is true for single- and double-quoted scalars and block scalars; `text` is the
    /// scalar's value after YAML escape and folding rules, with no type resolution applied.
    Scalar { text: String, quoted: bool },
    /// An empty value, `~` or plain `null`.
    Null,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum YamlError {
    /// An anchor (`&a`), an alias (`*a`), a merge key (`<<`) or a tag outside the core schema.
    #[error("the YAML uses a feature that is refused: {feature}")]
    FeatureRefused { feature: YamlFeature, line: u32 },
    /// Not well-formed YAML, more than one document, a duplicate or non-string mapping key,
    /// nesting deeper than 64, or a document that does not have the shape of `T`.
    #[error("the YAML is malformed at line {line}")]
    Malformed { line: u32 },
    /// `text.len() > max_bytes`.
    #[error("the YAML is larger than {max_bytes} bytes")]
    TooLarge { max_bytes: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum YamlFeature { Anchor, Alias, MergeKey, CustomTag }

impl YamlError {
    /// `YAML_FEATURE_REFUSED` | `YAML_MALFORMED` | `YAML_TOO_LARGE`.
    pub fn code(&self) -> &'static str;
}
impl CodedError for YamlError { /* code(); retryable() = false; repair() = None */ }
```

`YamlError`'s `Display` never contains the input text, only the feature, the line (1-based) or the limit.

### `session.rs` (`ordin_types::session`)

```rust
/// Who a session's ordin tools act for (2C wire §2). Carried by `StartRun.caller`
/// (`ordin-runtime-adapter`) and by the MCP Front's `SessionGrant`; it lives here because the
/// MCP Front has no build edge to `ordin-runtime-adapter`. Both crates re-export it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolCaller {
    /// Ordin's own session.
    Ordin,
    /// An agent's session started from a task.
    Agent,
    /// A crew member's session in a crew round.
    CrewMember { crew_run_id: CrewRunId },
    /// The hosted agent's session (agent profile).
    HostedAgent,
}
```

### `wire/events.rs`

```rust
/// The kind of a Service API event, as the Journal records it (2C wire *Names* item 7).
/// Modules that cannot depend on `ordin-service-api` name the kind of the event they write
/// to the Journal with this enum; `ordin-service-api` decodes the payload (ServiceApi spec).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    RunStateChanged, RunProgress, PlanChanged, StepMarked, StartUpReportReady,
    ApprovalRequested, ApprovalDecided, CrewRoundChanged, BlockShown, Reset,
    ServerStateChanged, WorkspaceChanged, NeedsYouChanged, BlockAvailable,
    LocaleChanged, BrandChanged, SetupProgress, SignInChanged,
    LanguageChanged,   // amended January 2026, ADR-109
    KnowledgeAgentsChanged, MoreFound,   // amended January 2026, ADR-113
}

impl EventKind {
    /// The snake_case wire name, equal to the SSE `event:` line and to the Journal's `kind` column.
    pub fn as_str(&self) -> &'static str;
    /// The inverse of `as_str`; `None` for any other string.
    pub fn parse(raw: &str) -> Option<EventKind>;
}
```

The last eight variants are added by the Service API spec's event catalogue *(chosen there)*; this crate holds the enum because emitters that cannot depend on `ordin-service-api` (phase 1 and 2 modules) name the kind they pass to `Journal::append_event` with `EventKind::as_str()`, so a misspelt kind cannot compile.

### Prelude

`ordin_types::prelude` re-exports: every identifier, `Timestamp`, `Sha256Hex`, `DecimalString`, `SetBy`, `Setting`, `AgentRef`, `InputUsed`, `Page`, `PageRequest`, `Cursor`, `TypesError`, `CodedError`, `RepairAction`, `AdapterKind`, `AdapterVerb`, `RunTarget`, `RunLimits`, `RunProgress`, `ProgressCount`, `StepMark`, `ModelLevel`, `ThinkingLevel`, `ModelChoice`, `EventKind`. The crate root also re-exports `Page`, `PageRequest`, `Cursor` (`ordin_types::Page`) and `RepairAction`, `CodedError`, `TypesError` (`ordin_types::RepairAction`). Domain and wire types are reached by module path (`ordin_types::domain::runs::Run`, `ordin_types::wire::approval::ApprovalCard`), never through the prelude or the root, so a clash cannot be imported by accident (SA-RECON-01).

### Module paths of the types consumers name

The rule above, applied to every domain type another spec imports by a root path (`ordin_types::X`) or by a module path this crate does not have. Consumers write the path in the right-hand column.

| Type | Path |
|---|---|
| `Page<T>`, `PageRequest`, `Cursor` (`Cursor::encode`, `Cursor::decode`), `PAGE_SIZE` | `ordin_types::paging::…` (also `ordin_types::Page`, the prelude) |
| `Part`, `PartKind`, `PartStatus` | `ordin_types::domain::agents::…` |
| `CoordinatorKind` (renamed from `Coordinator`, decision 80: C-6) | `ordin_types::domain::runs::CoordinatorKind` |
| `ToolRequestDecision` (renamed from the domain `ToolDecision`, decision 80: C-2) | `ordin_types::domain::runs::ToolRequestDecision` |
| `NatsDelivery` | `ordin_types::domain::server::NatsDelivery` |
| `RunRecord`, `RunStarter`, `RunRequest`, `RunRecordState`, `RunEndKind`, `Scope`, `ScopeKind`, `ScopeState` | `ordin_types::domain::agent_profile::…` |
| `PlaceKind` (with `WorkingCopy` and `CodingTaskWorktree`, decision 80: C-9) | `ordin_types::domain::places::PlaceKind` |
| `CredentialKind` (with `Gitlab`, decision 80: C-10, D9), `HarnessKind` | `ordin_types::domain::identity::…` |
| The agent-configuration types the Agent Configuration spec places in `ordin-types` (`AgentConfig`, `HarnessChoice`, its own `ModelChoice`, `ModelPolicyConfig`, `ToolsConfig`, `McpBinding`, `IntakeEntry`, `ScheduleEntry`, `AuthConfig`, `ChatTrigger`, `ChatConnection`, `EntryCommand`, `JobCommand`, `ReachConfig`, `ApproversConfig`, `ApproverUser`, `SettingDeclaration`, `SettingKind`, `RetentionConfig`, `ControlsConfig`, `Declaration`, `Manifest` and every other type of its *Types* section) | `ordin_types::domain::agent_profile::…`; the configuration's `ModelChoice` is distinct from `wire::run::ModelChoice` and is never imported through the prelude |
| The types the Model Policy spec adds (`ModelProvider`, `OrganisationKey`, `HarnessModelSettings`, `HarnessEnvVar`, `ModelFailureCause`, `CallPlace`) | `ordin_types::domain::agent_profile::…`; *amended January 2026 (ADR-107)*: `ModelProvider` is a string newtype, `HarnessModelSettings.key` is optional and `HarnessEnvVar` is added; *amended January 2026 (Phase 1 build corrections, the user's decision of January 2026)*: `OrganisationKey::new` and `HarnessEnvVar::new` are the only constructors, so Model Policy can build both while the fields stay private; the constants `RESERVED_ENVIRONMENT_NAMES` and `RESERVED_ENVIRONMENT_PREFIX` sit with the Agent Configuration types |
| The types the Agent Loader spec adds (`HostedSessionSetup`, `HarnessToolPolicy`, `SubagentSetup`, `SubagentTools`, `SingleCallForm`; `LoadMode` stays in `ordin-agent-loader`, which owns it) | `ordin_types::domain::agent_profile::…` |

The field definitions of the last three rows are those of the named specs; this crate compiles them as written there (they are additions to 2C that 2C's *Entities not typed here* leaves to those specs).

### Who uses what

| Item | Implemented or used by |
|---|---|
| `Page<T>`, `Cursor` | every paged reader: Journal and Artefact Store, Approval Service, Catalogue Service, Confluence Adapter, File Service, Knowledge Service, SharePoint Adapter, Telegram Service, Run Queue, Caller API, Job Runner, Settings Store, Service API. Each imports this definition; no spec keeps a local `Page` or `PAGE_SIZE`. |
| `CodedError` | implemented by `TypesError` here and by every module's error type (`code()`, `retryable()`, `repair()` — the method is `retryable`, not `is_retryable`); read by the Ordin Desktop and Ordin Server programs' `ServiceApi` implementations. |
| `RepairAction` | defined here only; the Service API, Localisation, Identity and Credentials, Package Catalogue, Catalogue Service, Authoring Service, Push Service, Handover Service and Ordin Desktop import it. |
| identifiers (`uuid_id!`) | every crate; the field is `pub` (`BackendId(pub uuid::Uuid)`), and no spec redefines one. |
| `EventKind::as_str()` | every emitter that writes a Service API event to the Journal with `Journal::append_event(kind, data)` (R3): Session Supervisor, Approval Service, Crew Orchestrator, MCP Front, Ordin Host, Server Connector, Workspace Service, File Service, Package Catalogue, Handover Service, Push Service, Authoring Service, Knowledge Query Client (`knowledge_agents_changed`) and Memory Service (`more_found`) (amended January 2026, ADR-113), and the programs for `locale_changed`, `brand_changed`, `setup_progress`, `sign_in_changed`. |
| `ToolCaller` (`ordin_types::session`) | Runtime Adapter interface (`StartRun.caller`, re-exported), MCP Front (`SessionGrant`, re-exported), Session Supervisor, Crew Orchestrator, Ordin Host, local adapters. |
| `ordin_types::yaml::read_restricted`, `read_restricted_node`, `YamlNode`, `YamlError` | every crate (the Agent Model uses `read_restricted_node`) that reads YAML: Agent Model, Ordin Host, Catalogue Service, Handover Service, and any other spec that parses a YAML file; each maps `YAML_FEATURE_REFUSED`, `YAML_MALFORMED` and `YAML_TOO_LARGE` to a code of its own. |
| `ModelLevel`, `ThinkingLevel`, `ModelChoice` | Runtime Adapter interface (`StartRun`), Usage Planner, Session Supervisor, Crew Orchestrator, local adapters, Organisation Profile (`usage.level_models`). |

##### Example

```rust
use ordin_types::prelude::*;
use ordin_types::text::{check_display_name, AreaCodes};

// The codes of 2C domain `Place`; a place has no URL, so the unused URL rule carries the path code.
const PLACE: AreaCodes = AreaCodes {
    name_empty: "PLACE_NAME_INVALID", name_too_long: "PLACE_NAME_INVALID",
    path_invalid: "PLACE_PATH_INVALID", url_invalid: "PLACE_PATH_INVALID",
};

let run = RunId::new();                                   // 0192a1c2-3344-7556-8677-8899aabbccdd
let at: Timestamp = serde_json::from_str("\"2026-01-10T09:41:07.312Z\"")?;
assert_eq!(serde_json::to_string(&at)?, "\"2026-01-10T09:41:07.312Z\"");
assert!(serde_json::from_str::<Timestamp>("\"2026-01-10T09:41:07Z\"").is_err());          // no millis
assert!(serde_json::from_str::<RunId>("\"0192a1c2-3344-4556-8677-8899aabbccdd\"").is_err()); // v4
let name = check_display_name("  Quarterly reports ", 120, PLACE)?;                        // "Quarterly reports"
let page: Page<RunId> = Page { items: vec![run], next_cursor: None };
assert_eq!(serde_json::to_value(&page)?["next_cursor"], serde_json::Value::Null);
```

## Internal Logic

1. **Identifier parse (`parse_v7`).** (a) Refuse when `raw.len() != 36` → `IdNotUuidV7`. (b) Refuse when any character is an uppercase hex letter (lowercase is the only accepted form) → `IdNotUuidV7`. (c) Parse with `uuid::Uuid::try_parse`; a parse error → `IdNotUuidV7`. (d) Refuse when `get_version_num() != 7` → `IdNotUuidV7`. (e) Return the UUID. No logging; the caller decides whether the failure is journalled.
2. **Timestamp parse (`parse_rfc3339_ms_utc`).** (a) Require exactly 24 characters, the pattern `^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$` checked byte by byte without a regular-expression library. (b) Parse with `chrono::DateTime::parse_from_rfc3339`; failure (for example month 13) → `TimestampNotRfc3339MsUtc`. (c) Convert to `Utc`. `Timestamp::now()` takes `Utc::now()` and truncates to whole milliseconds with `DurationRound::trunc_subsecs(3)` so that serialise-then-parse returns an equal value.
3. **Text normalisation (`normalise_text`).** (a) Scan scalar values; any `char::is_control` other than `'\n'` and `'\t'` → `TextControlChar` (this includes `'\r'`: a caller holding CRLF text converts it to `\n` before calling). (b) If `icu::normalizer::ComposingNormalizerBorrowed::new_nfc().is_normalized(raw)` return `raw.to_owned()`; otherwise return `normalize(raw)`. Normalisation runs before length checks, because NFC can shorten a string.
4. **Length and form checks.** `check_len` counts `chars()`. `check_display_name` = trim (Unicode whitespace) → `normalise_text` → `check_len(1, max)` with `name_empty` for zero and `name_too_long` above `max`. `check_abs_path` refuses an empty value, a NUL byte, more than 4096 bytes, or a path for which `std::path::Path::is_absolute` is false → `path_invalid`. `check_https_url` parses with `url::Url::parse`, refuses more than 2048 bytes, a scheme other than `https` (or `http` to `localhost`/`127.0.0.1` when allowed), user information in the URL, or a missing host → `url_invalid`.
5. **Placement of types shared with the contract crates.** `ModelLevel`, `ThinkingLevel` and `ModelChoice` are fields of `StartRun` (`ordin-runtime-adapter`) and are read by the Usage Planner and the Organisation Profile, so 2C wire §1 *Placement* puts them here too. `AdapterKind` and `RunLimits` are fields of the domain entity `Run` (2C part 1, `Run.adapter`, `Run.limits`); `AdapterVerb` is a field of `StartUpReportItem::VerbNotOffered` (2C wire §6, in this crate); `RunTarget` is a field of the block catalogue's `FormBlock` (`ordin-service-api`) as well as of `StartRun` (`ordin-runtime-adapter`); and `RunProgress`, `ProgressCount`, `StepMark` are carried both by `RunEvent` (`ordin-runtime-adapter`) and `ServiceEvent` (`ordin-service-api`). Because no crate may depend on a contract crate other than this one (2B-1 rule 2), these ten live here and both contract crates re-export them (`pub use ordin_types::wire::run::*`). `RepairAction` lives here so module error types can implement `CodedError::repair` without depending on `ordin-service-api`.
6. **Names defined twice in 2C.** The two 2C parts define 15 type names twice. Each is settled once:

   | Name | Domain part | Wire part | Settled as |
   |---|---|---|---|
   | `AdapterKind` | `WrightrunHttp` variant | `HostedAgent` variant | One type, the wire definition (`wire::run`). `domain::runs` re-exports it. ADR-098 renamed the adapter; the domain variant predates revision 0.43.0 (SA-HLDCONFLICT-01, later revision wins). The `Run` validation row that names `wrightrun_http` reads `hosted_agent`. |
   | `RunLimits` | `request_budget: u32` | `request_budget: Option<u32>` | One type, the wire definition, with the union of both derive lists. `07-cross-cutting.md:49` leaves no default ceiling until OQ-3, which only the optional form can carry. The domain rule "each ≥ 1" applies to `request_budget` when present. |
   | `StepMark` | identical | identical | One type in `wire::run`; `domain::runs` re-exports it. |
   | `HarnessKind` | identical | identical | One type in `domain::identity`; `wire::hook` re-exports it. |
   | `IfItFails` | no default | `#[default] OrdinTakesOver` | One type, the wire definition, in `wire::workflow_file`; `domain::runs` re-exports it. The default only adds `Default`. |
   | `WorkflowInputKind` | `Document`, `Text` | adds `List` | One type, the wire definition; 2C I-1 adds `list`. |
   | `ToolDecision` | `enum { Allow, Deny, Ask }` (the stored verdict of a Tool request) | `struct { verdict, rule, decided_at }` (the Rules Engine's full decision) | Two types with two names since decision 80 (C-2): the domain enum is `domain::runs::ToolRequestDecision`; `wire::rules::ToolDecision` keeps its name. Neither is in the prelude. |
   | `NamedPath`, `StepUse` | the saved workflow's steps (`WorkflowStep`) | the file format's steps (`WorkflowFileStep`) | Two types each: `domain::runs::{NamedPath, StepUse}` and `wire::workflow_file::{NamedPath, StepUse}`. The Ordin Host converts between them on import and export. |
   | `ReportPart`, `TableColumn`, `ColumnType`, `ChartKind`, `KeyFigure`, `FigureChange` (stored: `ReportSection`, `ReportTableColumn`, `ReportKeyFigure`, `ReportFigureChange`, amended January 2026) | the stored `Report` entity, numbers as `DecimalString` | the block catalogue (`ordin-service-api`), numbers as `f64` | Two families: `domain::runs::{…}` here, and the block types in `ordin-service-api`. The Journal stores the domain form; the block check converts each `f64` into a `DecimalString` with `format!("{}", v)` after refusing non-finite values. Decision 80 (C-23) settles the mapping and adds `BarArrangement` and `decimals` to the stored family; `BarArrangement` is defined once in `domain::runs` and the block catalogue re-exports it. |

7. **Coded errors.** `CodedError` is implemented by `TypesError` and by every module's own error enum. The program's `ServiceApi` implementation reads `code()`, `retryable()` and `repair()` and asks Localisation for the sentence; this crate builds no sentence.
8. **Cursor.** `Cursor::encode` writes `serde_json::to_vec(key)` then `b64url_encode`. `Cursor::decode` refuses a value over 1,024 characters, decodes base64url, then `serde_json::from_slice::<K>`; any failure → `BadCursor` (`SRV_BAD_CURSOR`). The cursor carries no signature *(assumed here: it holds only a sort key the caller already saw, and every query applies the caller's own scope, so forging one reveals nothing)*.
9. **Tests the crate ships** (`crates/ordin-types/tests/`): a JSON round-trip test for every public type using 2C's JSON example where 2C gives one; a property test that `Timestamp::now()` round-trips; table-driven tests for every row of the Error Table; a compile-time test (`static_assertions`-free: a `const _: fn() = || { fn f<T: Send + Sync>() {} f::<Run>(); … }` block) that every type is `Send + Sync`.

10. **Restricted YAML (`read_restricted`, P4-1).** (a) `text.len() > max_bytes` → `TooLarge` before any parsing. (b) Run `saphyr`'s event parser (`saphyr_parser::Parser::new_from_str(text)`) over the text and build a `serde_json::Value` from the events: an `Alias` event → `FeatureRefused { Alias }`; any event carrying an anchor → `FeatureRefused { Anchor }`; a node tag other than `!!str`, `!!int`, `!!float`, `!!bool`, `!!null`, `!!map`, `!!seq` (in their `tag:yaml.org,2002:` form) → `FeatureRefused { CustomTag }`; a mapping key `<<` → `FeatureRefused { MergeKey }`; a scanner or parser error → `Malformed { line }` from the error's marker. (c) A second `DocumentStart` after the first document's end → `Malformed`; an empty stream is the document `null`. (d) Mapping keys must be scalars and are kept as strings; a key seen twice in one mapping → `Malformed`. Nesting deeper than 64 sequences and mappings → `Malformed`. (e) `read_restricted_node` stops after (d): it returns the tree as `YamlNode` with each scalar's text unchanged and no type resolution (a plain empty value, `~` or `null` → `Null`); `read_restricted` continues. Plain scalars resolve by the YAML 1.2 core schema (`null`, `~` and empty → null; `true`/`false` in its three spellings → bool; decimal, `0o` and `0x` integers that fit `i64`; floats including `.inf` and `.nan` → refused as `Malformed`, because JSON cannot carry them); quoted and block scalars are strings. (f) `serde_json::from_value::<T>(value)`; an error → `Malformed { line: 0 }` (the shape is wrong, not a line). No logging (the crate never logs).
11. **Agent-profile configuration types.** `domain/agent_profile.rs` holds 2C's `RunRecord` and `Scope` families and, below them, the types of the *Module paths* table's last three rows, compiled as their specs write them (including those specs' own `serde` attributes, which use `kebab-case` for the configuration file). They carry no validation function here; the Agent Configuration crate validates a whole configuration.

## Data Model

N/A — the crate owns no table. It fixes the storage convention every owning module applies (Shared Context, *Storage*): column names equal field names; `Timestamp` columns are `TEXT` in the wire form on SQLite and `TIMESTAMPTZ` on PostgreSQL (read back through `Timestamp::from_unix_millis` after `EXTRACT(EPOCH …) * 1000`); identifiers are `TEXT` (36 characters) on SQLite and `UUID` on PostgreSQL; `Vec`, struct and data-carrying enum fields are one JSON `TEXT` column (`JSONB` on PostgreSQL) *(assumed here: PostgreSQL's native types keep index locality and are what sqlx maps `Uuid` and `DateTime<Utc>` to)*.

## Error Table

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| Identifier not 36 characters, uppercase, unparseable or not version 7 | `Err(permanent)` | `ID_NOT_UUIDV7` | `TypesError::IdNotUuidV7` |
| Timestamp not `YYYY-MM-DDTHH:MM:SS.mmmZ`, or an impossible date | `Err(permanent)` | `TS_NOT_RFC3339_MS_UTC` | `TypesError::TimestampNotRfc3339MsUtc` |
| `from_unix_millis` outside 1970-01-01..9999-12-31 | `Err(permanent)` | `TS_NOT_RFC3339_MS_UTC` | `TypesError::TimestampNotRfc3339MsUtc` |
| Hash not exactly 64 lowercase hex characters | `Err(permanent)` | `HASH_NOT_SHA256_HEX` | `TypesError::HashNotSha256Hex` |
| Decimal string not matching the pattern | `Err(permanent)` | `NUM_NOT_DECIMAL` | `TypesError::NumNotDecimal` |
| `MemoryThingId` or `MemoryLinkId` without its prefix or with a malformed UUID (*amended January 2026, ADR-111*) | `Err(permanent)` | `ID_MEMORY_REF_INVALID` | `TypesError::Rule { code: "ID_MEMORY_REF_INVALID" }` |
| Text contains a control character other than `\n`, `\t` | `Err(permanent)` | `TEXT_CONTROL_CHAR` | `TypesError::TextControlChar` |
| Length, display-name, path, URL or base64 rule fails | `Err(permanent)` | the caller's code (for example `PLACE_NAME_INVALID`, `INPUT_NAME_INVALID`) | `TypesError::Rule { code }` |
| Cursor over 1,024 characters, bad base64url, or wrong key shape | `Err(permanent)` | `SRV_BAD_CURSOR` | `TypesError::BadCursor` |
| YAML uses an anchor, an alias, a merge key or a tag outside the core schema | `Err(permanent)` | `YAML_FEATURE_REFUSED` | `YamlError::FeatureRefused { feature, line }` |
| YAML not well-formed, several documents, duplicate or non-scalar key, nesting over 64, a non-finite float, or not the shape of `T` | `Err(permanent)` | `YAML_MALFORMED` | `YamlError::Malformed { line }` |
| YAML text longer than `max_bytes` | `Err(permanent)` | `YAML_TOO_LARGE` | `YamlError::TooLarge { max_bytes }` |

No error from this crate is retryable: each is a property of the value.

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Shared types

  Scenario: Happy path — an identifier and a timestamp round-trip
    Given a RunId minted with RunId::new()
    And the timestamp "2026-01-10T09:41:07.312Z"
    When both are serialised to JSON and parsed back
    Then the parsed values equal the originals
    And the RunId's UUID version is 7

  Scenario: Edge case — a version-4 UUID is refused
    Given the JSON string "0192a1c2-3344-4556-8677-8899aabbccdd"
    When it is deserialised as a WorkspaceId
    Then deserialisation fails with code ID_NOT_UUIDV7

  Scenario: Edge case — an uppercase identifier is refused
    Given the JSON string "0192A1C2-3344-7556-8677-8899AABBCCDD"
    When it is deserialised as a RunId
    Then deserialisation fails with code ID_NOT_UUIDV7

  Scenario: Error path — a timestamp without milliseconds is refused
    Given the JSON string "2026-01-10T09:41:07Z"
    When it is deserialised as a Timestamp
    Then deserialisation fails with code TS_NOT_RFC3339_MS_UTC

  Scenario: Error path — a timestamp with an offset is refused
    Given the JSON string "2026-01-10T10:41:07.312+01:00"
    When it is deserialised as a Timestamp
    Then deserialisation fails with code TS_NOT_RFC3339_MS_UTC

  Scenario: Happy path — text is NFC-normalised before its length is counted
    Given the text "Café" (5 scalar values, decomposed)
    When normalise_text is called and then check_len(1, 4, "X_TOO_LONG")
    Then normalise_text returns "Café" (4 scalar values)
    And check_len succeeds

  Scenario: Error path — a carriage return is refused
    Given the text "line one\r\nline two"
    When normalise_text is called
    Then it fails with code TEXT_CONTROL_CHAR

  Scenario: Error path — a malformed decimal string
    Given the JSON string "1e5"
    When it is deserialised as a DecimalString
    Then deserialisation fails with code NUM_NOT_DECIMAL

  Scenario: Error path — a forged cursor
    Given a cursor of 1,025 characters
    When Cursor::decode is called
    Then it fails with code SRV_BAD_CURSOR

  Scenario: Edge case — the last page carries next_cursor null
    Given a Page with one item and no next cursor
    When it is serialised
    Then the JSON object has the key "next_cursor" with value null

  Scenario: Happy path — a model choice serialises in the 2C form
    Given ModelChoice { level: Standard, model: Some("sonnet") }
    When it is serialised
    Then the JSON is {"level":"standard","model":"sonnet"}
    And ordin_runtime_adapter::ModelChoice is the same type as ordin_types::wire::run::ModelChoice

  Scenario: Error path — an unknown field in a model choice
    Given the JSON {"level":"standard","model":"sonnet","thinking":"high"}
    When it is deserialised as a ModelChoice
    Then deserialisation fails

  Scenario: Happy path — every event kind round-trips through its wire name
    Given each of the 21 EventKind variants (amended January 2026, ADR-109; amended January 2026, ADR-113)
    When as_str is called and the result is passed to EventKind::parse
    Then parse returns the same variant
    And EventKind::parse("job_finished") returns None

  Scenario: Happy path — a crew member's tool caller serialises in the 2C form
    Given ToolCaller::CrewMember with crew_run_id "0192a1c5-0000-7000-8000-000000000001"
    When it is serialised
    Then the JSON is {"kind":"crew_member","crew_run_id":"0192a1c5-0000-7000-8000-000000000001"}
    And ordin_runtime_adapter::ToolCaller is the same type as ordin_types::session::ToolCaller

  Scenario: Happy path — the page form lives at ordin_types::paging
    Given ordin_types::paging::Page<RunId> with one item and a next cursor encoded from the key [42]
    When the cursor is decoded as Vec<u32>
    Then it returns [42]

  Scenario: Happy path — a plain YAML document is read
    Given the YAML text "name: Rates\nsteps:\n  - one\n  - two\n"
    When read_restricted is called for a struct { name: String, steps: Vec<String> } with max_bytes 1024
    Then it returns name "Rates" and steps ["one", "two"]

  Scenario: Happy path — the node form keeps a scalar's source text
    Given the YAML text "version: 1.10\n"
    When read_restricted_node is called with max_bytes 1024
    Then it returns Map [("version", Scalar { text: "1.10", quoted: false })]

  Scenario: Error path — an alias is refused
    Given the YAML text "base: &b {a: 1}\nother: *b\n"
    When read_restricted is called with max_bytes 1024
    Then it fails with code YAML_FEATURE_REFUSED

  Scenario: Error path — a custom tag is refused
    Given the YAML text "value: !python/object foo\n"
    When read_restricted is called with max_bytes 1024
    Then it fails with code YAML_FEATURE_REFUSED

  Scenario: Error path — a duplicate key is malformed
    Given the YAML text "a: 1\na: 2\n"
    When read_restricted is called with max_bytes 1024
    Then it fails with code YAML_MALFORMED

  Scenario: Edge case — input one byte over the limit
    Given a YAML text of 1,025 bytes
    When read_restricted is called with max_bytes 1024
    Then it fails with code YAML_TOO_LARGE before any parsing

  Scenario: Happy path — one AdapterKind across domain and wire
    Given a Run whose adapter is AdapterKind::HostedAgent
    When the Run is serialised
    Then its "adapter" field is "hosted_agent"
    And ordin_runtime_adapter::AdapterKind is the same type as ordin_types::domain::runs::AdapterKind
```

## Performance, Security, Observability

- **Performance targets:** `parse_v7` ≤ 1 µs p95; `parse_rfc3339_ms_utc` ≤ 2 µs p95; `normalise_text` on 8,000 characters of already-NFC text ≤ 100 µs p95 and of decomposed text ≤ 400 µs p95, measured with `criterion` on the reference machine of A-03 (`cargo bench -p ordin-types`). No allocation on the already-normalised path except the returned `String`. The crate adds at most 1.5 MiB to a release binary (the `icu` normaliser data is compiled in).
- **Security:** pure functions with no I/O, no `unsafe`, `#![forbid(unsafe_code)]`. `read_restricted` refuses anchors and aliases, so a small file cannot expand into a large tree, and checks the size before parsing. `TypesError`'s `Display` never includes the refused value, so a caller logging the error cannot leak the input (a token pasted where an identifier was expected, a secret in a path). The cursor's content is a sort key only (step 8).
- **Observability:** none — the crate never logs, emits no metric and opens no span (SA-XC-04, SA-XC-05). Callers log `code()`.

## Reconciliation assumptions

- **SA-RECON-01** — Ruling R6/R7: this crate is the provider of every shared type; consumers import the 2C definition by the path in *Module paths* and keep no local copy of `Page`, `Cursor`, `PAGE_SIZE`, `RepairAction` or an identifier. Domain types are not re-exported at the root (the rule of *Prelude* stands over adding root re-exports, so two specs cannot import two different `ModelChoice` by accident).
- **SA-RECON-02** — Ruling R7: the page module is `ordin_types::paging`, as corrected 2C domain §0 names it (C-15), not `page`.
- **SA-RECON-03** — Ruling R3: emitters name an event's kind with `EventKind::as_str()` and pass `ServiceEvent::to_journal_data()` (or the same JSON) to `Journal::append_event`; no `Journal::emit(ServiceEvent)` exists, because the Journal cannot name `ServiceEvent`.
- **SA-RECON-04** — Row 47 of the reconciliation: the agent-configuration types other specs place in `ordin-types` live in `domain::agent_profile`, with the fields their owning specs give.

## Gaps

None.

---

## Amendment January 2026 — Knowledge agents (ADR-113)

> Source: decision 92 (the user's decisions of January 2026); HLD revision 0.61.0, ADR-113 (knowledge agents: a desktop's connections to front doors, answer ids for the optional sources record, a scope for a knowledge agent's graph). Owners of the types: phase 2C domain and wire (amended January 2026); consumers `components/KnowledgeQueryClient.md`, `components/KnowledgeQueryServer.md`, `components/KnowledgeReader.md`, `components/MemoryService.md`.
> Every rule below adds to the sections above. The crate stays data plus pure functions.

### Purpose (addition)

`ordin-types` carries the two new identifiers and the new scope kind that more than one crate reads, so the desktop, the personal server and the agent profile mint and parse them the same way.

### Approach (addition)

The identifiers join the existing `uuid_id!` list (*Public Interface*, the identifier block above, now marked *amended January 2026 (ADR-113)*), so they get the same UUIDv7 check, serde form and storage convention as every other identifier. The scope kind is a variant of 2C's `ScopeKind`, compiled in `domain/agent_profile.rs` as 2C writes it. The wire types of the knowledge query (`KnowledgeResult`, `KnowledgeHit`, `KnowledgeContents`, `KnowledgeAbout`, and the two page signals `PageTier` and `PageStatus`) are 2C wire types used by the Knowledge Query Client and Server, the Knowledge Reader, the Knowledge Service's `okf` module and the Memory Service; they sit at `ordin_types::wire::knowledge_query::…` and are compiled as 2C writes them. Rejected: *string ids chosen by each crate* — the client stores a connection id and the screens send it back through the Service API, so one checked form is needed.

Assumptions (*chosen here*):

- **SA-TYPES-KA-01** `KnowledgeAnswerId` is minted by the Knowledge Query Server only when its sources record is on; elsewhere it is only parsed. It is a UUIDv7 so the record's rows sort by time without a second column.
- **SA-TYPES-KA-03** `PageTier { HumanReviewed, MachineConfirmed, Unverified }` (serialised snake_case; default `unverified`) is the trust OKF 0.2 derives from a concept's `verified` list, with the trust signals 1.0, 0.9 and 0.8 (`PageTier::trust_signal`); `PageStatus { Stable, Draft, Deprecated }` (default `stable`) is a concept's `status`. They are defined here once, so no crate re-declares them (ADR-112).
- **SA-TYPES-KA-02** `ScopeKind::Knowledge` serialises as `"knowledge"`; at most one scope of this kind exists per agent (2C rule, error code `SCOPE_KNOWLEDGE_DUPLICATE` owned by 2C's scope rules), and it has no `chat_identifier`, as `agent_wide` has none.

### Shared Context (additions)

```rust
// domain/agent_profile.rs — 2C part 1, amended January 2026 (ADR-113)
pub enum ScopeKind { Channel, DirectChat, AgentWide, Knowledge }

// wire/knowledge_query.rs — 2C wire, amended January 2026 (ADR-112, ADR-113)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum PageTier { HumanReviewed, MachineConfirmed, #[default] Unverified }
impl PageTier { pub const fn trust_signal(self) -> f64 { match self { Self::HumanReviewed => 1.0, Self::MachineConfirmed => 0.9, Self::Unverified => 0.8 } } }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum PageStatus { #[default] Stable, Draft, Deprecated }
```

### Public Interface (additions)

- `ordin_types::KnowledgeConnectionId` and `ordin_types::KnowledgeAnswerId`: `uuid_id!` newtypes, re-exported by the prelude as every identifier is.
- *Module paths* gains the row: `KnowledgeResult`, `KnowledgeHit`, `KnowledgeContents`, `KnowledgeAbout`, `PageTier`, `PageStatus` and the types they hold → `ordin_types::wire::knowledge_query::…`.

### Internal Logic, Data Model, Error Table

N/A — no new function beyond the constant `PageTier::trust_signal`; the identifiers use the shared parser; 2C's scope rule rejects a second knowledge scope.

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Shared types — knowledge agents

  Scenario: Happy path — a connection id round-trips
    Given KnowledgeConnectionId::new()
    When it is serialised and parsed back
    Then the value is equal and its text is a 36-character lowercase version-7 UUID

  Scenario: Error — an answer id that is not version 7
    When KnowledgeAnswerId::parse("0192b7a0-5c1d-4e2f-8a3b-4c5d6e7f8091") is called
    Then it fails with "identifier is not a version-7 UUID"

  Scenario: Happy path — the knowledge scope kind
    When ScopeKind::Knowledge is serialised
    Then the JSON is "knowledge"

  Scenario: Happy path — page tiers carry their trust
    When PageTier::HumanReviewed, MachineConfirmed and Unverified are asked for trust_signal
    Then they return 1.0, 0.9 and 0.8
    And PageTier::default() is Unverified and PageStatus::default() is Stable
```

### Performance, Security, Observability (additions)

N/A — no change.

### Gaps

None.
