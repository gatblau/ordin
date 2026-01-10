# Phase 2C — Shared Types Catalogue, part 2: wire and file-format types

> **Spec set:** `ordin-desktop` · **Mode:** greenfield · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-106; promoted to Approved by the user January 2026) · amended January 2026 (ADR-109; promoted to Approved by the user January 2026) · amended January 2026 (ADR-110; promoted to Approved by the user January 2026) · amended January 2026 (ADR-111; promoted to Approved by the user January 2026) · amended January 2026 (ADR-112, ADR-113; promoted to Approved by the user January 2026) · amended January 2026 (ADR-115)
> **derivedFromHld:** 0.63.0 · **Source HLD:** `docs/design/ordin-desktop/` · **Authored:** January 2026 · **Amended:** January 2026 (HLD-impact edits approved, decision 79; C-61 result); January 2026 (Phase 3 corrections, decision 80); January 2026 (ADR-103: team_knowledge commands noted); January 2026 (ADR-106: command count 354 in 38 groups, the `jira` group); January 2026 (ADR-108: command count 355; promoted to Approved by the user January 2026); January 2026 (ADR-109: command count 368, event kind `language_changed`, 19 kinds); January 2026 (ADR-110: command count 377, `TeamFolderStatus`, typed recall sub-queries and folder filters, the M-8 memory types, three team folder files); January 2026 (ADR-111: command count 390, `MessageBlock.memory_used`, the memory view types, the M-9 memory types); January 2026 (ADR-112, ADR-113: §12 the module `wire::knowledge_query` with `PageTier` and `PageStatus`, command count 395 in 39 groups, event kinds `knowledge_agents_changed` and `more_found`, the page and recall view additions); January 2026 (ADR-115: §12 per-document read access types)
> **Index:** `phase-2-architecture.md` §2C · **Part 1:** `phase-2c-shared-types-domain.md`

> **Status:** Approved (user, January 2026; Phase 6 passed)
> **Derived from HLD:** `docs/design/ordin-desktop/` revision 0.47.0 (January 2026)
> **Scope:** types that cross a module or process boundary — the Service API envelope, the Runtime Adapter interface, the Rules Engine's tool decision, the ordin hook protocol, the approval card, the start-up report items, the block catalogue, and workflow file format 3.
> **Out of scope (parallel part):** persisted domain entities of `04-domain.md`. Where a payload embeds one, it is referenced by its 04-domain name in UpperCamelCase (`Run`, `Approval`, `ToolRequest`, `Workflow`, `WorkflowStep`, `Report`, `RunStep`, `StartupReport`, `EmailDraft`, `CardReference`, `AgentInstance`, `RemoteAgent`, `AgentRepository`, `Workspace`, `Place`, `Artefact`, `CrewRun`, `CrewCharter`, `KnowledgePage`, `MemoryFact`, `DesktopPairing`, `ServerConnection`) and never redefined here. 04-domain has no entity named `ApprovalRequest`; the entity is **Approval** (`04-domain.md:194`).

## 0. Conventions binding every type below

- Timestamps use the `Timestamp` newtype defined in part 1 (domain entities): RFC 3339, UTC, three fractional digits, `Z` suffix.

| Convention | Rule |
|---|---|
| Identifiers | Newtypes over UUIDv7, serialised as the 36-character lowercase hyphenated form. An identifier of a 04-domain entity is written `<Entity>Id` (`RunId`, `ApprovalId`, `WorkspaceId`, …) and is defined by the domain-entity part of 2C; this part only uses them. Identifiers introduced here: `BackendId`, `CommandRequestId`, `HookCallId`, `BlockId`. |
| Timestamps | `chrono::Timestamp`, RFC 3339 with millisecond precision and `Z` (`2026-01-10T09:41:07.312Z`). |
| Case | `#[serde(rename_all = "snake_case")]` on every struct and enum. |
| Tagged enums | `#[serde(tag = "kind", rename_all = "snake_case")]`. Where a struct already has a field called `kind` the enum is flattened into it; no type below carries two `kind` fields. |
| Optional fields | `#[serde(skip_serializing_if = "Option::is_none")]` on every `Option<T>`; empty `Vec<T>` fields are serialised. |
| Unknown fields | Rejected (`#[serde(deny_unknown_fields)]`) on every type that enters the core from outside (Remote API bodies, hook requests, blocks, MCP tool arguments, workflow files). Types the core emits accept unknown fields on read so that a newer minor version can add fields. |
| Text | UTF-8, NFC-normalised at the boundary; control characters other than `\n` and `\t` are rejected. Text is always drawn as text (ADR-028, ADR-067). |
| Error codes | `SCREAMING_SNAKE_CASE` with an area prefix: `SVC_` Service API, `RA_` Runtime Adapter, `RULE_` Rules Engine, `HOOK_` hook protocol, `APPR_` approval card, `START_` start-up report, `BLOCK_` block catalogue, `WFF_` workflow file (renamed from `WF_` on January 2026, decision 80 / D8, because `WF_` is a carried wrightrun prefix). The person never sees a code (`07-cross-cutting.md:21`). |

```rust
// Shared id newtype shape used for every identifier in this part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BackendId(uuid::Uuid); // uuid's serde form is the lowercase hyphenated string
```

---

## 1. Service API (ADR-096)

### ServiceApiVersion
- **Crosses:** desktop ↔ server (Remote API, pairing and every response header); WebView ↔ core (Tauri IPC, at start-up).
- **Producer / Consumers:** Command Bridge and Remote API produce; Server Connector and the screens module consume.
- **HLD trace:** `09-decisions.md:2425` (ADR-096 item 2, *Versions*); `08-interfaces.md:38`; `02-architecture.md:591` (Server Connector refuses an unusable version); `04-domain.md:271` (Server connection keeps the version last reported).
- **Rust definition:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ServiceApiVersion {
    pub major: u16,
    pub minor: u16,
}

impl ServiceApiVersion {
    pub const CURRENT: ServiceApiVersion = ServiceApiVersion { major: 1, minor: 0 };

    /// ADR-096 item 2: a desktop works with a server of the same major version
    /// and the same or the previous minor version.
    pub fn desktop_accepts(desktop: Self, server: Self) -> bool {
        desktop.major == server.major
            && (server.minor == desktop.minor || server.minor + 1 == desktop.minor)
    }
}
```
- **JSON example:** `{"major":1,"minor":0}`. On the Remote API every response also carries the header `Ordin-Api-Version: 1.0`.
- **Validation:**

| Field | Rule | Error code |
|---|---|---|
| `major` | 1 ≤ major ≤ 999 (assumed) | `SVC_VERSION_MALFORMED` |
| `minor` | 0 ≤ minor ≤ 999 (assumed) | `SVC_VERSION_MALFORMED` |
| pair | `desktop_accepts(desktop, server)` is true, otherwise refused with one sentence naming which side to update | `SVC_VERSION_UNSUPPORTED` |

- **Versioning:** this is the version. A minor increment adds commands, events, optional fields or enum variants that an older reader may ignore; anything else is a major increment and a new `/v2/` prefix.

### BackendId
- **Crosses:** core → WebView (Tauri IPC, on every event); server → desktop (Remote API events).
- **Producer / Consumers:** the core of each installation (its identifier is minted once, at first start, and stored by the Journal); consumed by the screens module's backend handles and the Server Connector.
- **HLD trace:** `08-interfaces.md:38` ("Event: backend, sequence number, kind, typed data"); `09-decisions.md:2423` (ADR-096 item 2, backend handle); `02-architecture.md:217` (events "each with its backend and its sequence number").
- **Rust definition:** see §0 (`pub struct BackendId(uuid::Uuid)`).
- **JSON example:** `"0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b"`
- **Validation:**

| Field | Rule | Error code |
|---|---|---|
| value | UUID version 7, lowercase, 36 characters | `SVC_BACKEND_ID_MALFORMED` |
| value | names the local core or a `ServerConnection` this desktop holds | `SVC_BACKEND_UNKNOWN` |

### CommandEnvelope
- **Crosses:** WebView → core (Tauri IPC); desktop → server (Remote API request body).
- **Producer / Consumers:** the screens module produces; Command Bridge (desktop) and Remote API (server) consume, validate and route to the owning module.
- **HLD trace:** `08-interfaces.md:38` ("Command: a name, its typed input and one reply"); `09-decisions.md:2423`; `02-architecture.md:414-417`, `:579`.
- **Rust definition:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CommandEnvelope<I> {
    pub api_version: ServiceApiVersion,
    /// Chosen by the caller; repeated on the reply. Lets the screens match replies on a
    /// shared HTTP/2 connection and lets the server drop a resent command.
    pub request_id: CommandRequestId,
    /// `<module>.<verb>` in snake_case, for example `approval.decide`.
    pub command: CommandName,
    /// Which backend the command is for. The Command Bridge routes a command for a
    /// server's backend to the Server Connector (ADR-096 item 2).
    pub backend: BackendId,
    pub input: I,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CommandName(String);
```
- **JSON example:**
```json
{
  "api_version": {"major": 1, "minor": 0},
  "request_id": "0192a1c0-0d1e-7f20-8a31-425364758697",
  "command": "approval.decide",
  "backend": "0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b",
  "input": {"card_ref": "q8Zt3VxN0pLk2mRb7sYw1A", "decision": "approve"}
}
```
- **Validation:**

| Field | Rule | Error code |
|---|---|---|
| whole body | at most 1 MiB of JSON (assumed) | `SVC_BODY_TOO_LARGE` |
| `api_version` | accepted by `desktop_accepts` | `SVC_VERSION_UNSUPPORTED` |
| `request_id` | UUIDv7; a repeat within 10 minutes (assumed) returns the first reply, not a second effect | `SVC_REQUEST_ID_MALFORMED` |
| `command` | matches `^[a-z][a-z0-9_]{0,30}\.[a-z][a-z0-9_]{0,30}$`; is in the catalogue of this version | `SVC_COMMAND_UNKNOWN` |
| `backend` | see `BackendId` | `SVC_BACKEND_UNKNOWN` |
| `input` | matches the command's input schema exactly, unknown fields refused (`02-architecture.md:579`: "each checked against the command's schema before it reaches a module") | `SVC_INPUT_INVALID` |
| any text | no markup, script or style is interpreted (ADR-028) | — |

- **Versioning:** carries `api_version`. On the Remote API the command name maps to the resource path `POST /v1/<module>/<verb>` (for example `POST /v1/approval/decide`); every command is a `POST`, because each is one request with one reply and several change state (assumed). Pairing is the one route outside the catalogue (see *Names fixed by this spec*).

### CommandReply
- **Crosses:** core → WebView (Tauri IPC); server → desktop (Remote API response body).
- **Producer / Consumers:** Command Bridge and Remote API produce; the screens module consumes.
- **HLD trace:** `08-interfaces.md:38` ("one reply (a result, or an error with a plain-language sentence and a code)"); `07-cross-cutting.md:21`.
- **Rust definition:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CommandReply<O> {
    pub request_id: CommandRequestId,
    #[serde(flatten)]
    pub outcome: CommandOutcome<O>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommandOutcome<O> {
    Ok { result: O },
    Error { error: ServiceError },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ServiceError {
    /// Stable code, never shown to the person (07 — Error handling).
    pub code: String,
    /// One plain-language sentence in the person's locale, built by Localisation.
    pub message: String,
    /// At most one repair action (07 — Error handling).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<RepairAction>,
    /// The Diagnostics line identifier, shown as "Reference r-81" under an error that offers a
    /// repair (11 §6, UX-A70); never a path, token or internal state (amended January 2026, decision 80: C-30).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairAction {
    TryAgain,
    SignInAgain,
    SendToIt,
}
```
- **JSON example:**
```json
{
  "request_id": "0192a1c0-0d1e-7f20-8a31-425364758697",
  "kind": "error",
  "error": {
    "code": "APPR_CARD_ALREADY_DECIDED",
    "message": "This was already decided on Asha's laptop.",
    "action": null
  }
}
```
(`action` is omitted when `None`; shown here only for illustration of the field position.)
- **Validation:**

| Field | Rule | Error code |
|---|---|---|
| `error.code` | matches `^[A-Z]{2,6}_[A-Z0-9_]{2,60}$` and is listed in the error-code registry of the Phase 4 cross-cutting spec *Error Handling* (`cross-cutting/ErrorHandling.md`, *Error-code registry*; SA-ERR-01) | — (emitter defect, caught by test) |
| `error.message` | 1–300 characters (assumed), one sentence, no code, path, stack or tool output | — (emitter defect) |
| `error.reference` | 1–64 characters (assumed), the identifier of the Diagnostics line that recorded the error | — (emitter defect) |
| Remote API status | `ok` → 200; `SVC_INPUT_INVALID` and `SRV_BAD_CURSOR` (both refused by the input check before dispatch) → 422; `SVC_UNAUTHENTICATED` → 401; `SVC_VERSION_UNSUPPORTED` → 409; `SVC_RATE_LIMITED` → 429; any other error → 200 with `kind: "error"` (assumed: domain refusals are replies, not transport failures) | — |

### EventEnvelope
- **Crosses:** core → WebView (Tauri IPC events); server → desktop (Remote API SSE, `GET /v1/events`).
- **Producer / Consumers:** Journal assigns the sequence number; Command Bridge and Remote API emit; Server Connector and the screens module consume.
- **HLD trace:** `08-interfaces.md:38-39`; `09-decisions.md:2424` ("Every event a backend emits carries that backend's journal sequence number"), `:2427-2429` (SSE `id:`, `Last-Event-ID`, `reset`, heartbeat); `02-architecture.md:580`, `:589-590`; `04-domain.md:266-273` (last sequence number on Desktop pairing and Server connection).
- **Rust definition:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EventEnvelope {
    pub api_version: ServiceApiVersion,
    pub backend: BackendId,
    /// The backend's journal sequence number: strictly increasing per backend, starting at 1,
    /// never reused. On SSE it is also the `id:` line.
    pub seq: u64,
    pub at: Timestamp,
    #[serde(flatten)]
    pub event: ServiceEvent,
}
```
- **JSON example (SSE frame):**
```
id: 48213
event: approval_requested
data: {"api_version":{"major":1,"minor":0},"backend":"0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b","seq":48213,"at":"2026-01-10T09:41:07.312Z","kind":"approval_requested","card":{…ApprovalCard…}}

: heartbeat
```
- **Validation:**

| Field | Rule | Error code |
|---|---|---|
| `seq` | > last `seq` received from that backend; a gap triggers a reload of the affected screens through commands | `SVC_EVENT_GAP` (desktop-side diagnostic) |
| `Last-Event-ID` | an integer ≥ 1; older than the journal keeps → one `reset` event | `SVC_RESUME_TOO_OLD` (diagnostic only; the person sees nothing) |
| serialised `data` | at most 256 KiB (assumed); a larger payload is sent as a reference the screens fetch by command | `SVC_EVENT_TOO_LARGE` |
| heartbeat | an SSE comment line every 20 s (`08-interfaces.md:39`) | — |
| link loss | no event or heartbeat for 45 s → **Not connected**, reconnect with backoff 1 s to 60 s (`02-architecture.md:589-590`) | — |

- **Versioning:** `api_version` per event; the SSE `event:` line equals `kind`.

### ServiceEvent
- **Crosses:** as `EventEnvelope`.
- **Producer / Consumers:** Session Supervisor (run events), Approval Service (approval events), Crew Orchestrator (round events), Block Renderer's validating side (shown blocks), Remote API (`reset`); consumed by the screens module and, on the desktop, by Server Connector (notifications under UX-A4).
- **HLD trace:** `02-architecture.md:414` ("pushes events (session progress, approval requests, crew round changes)"); `09-decisions.md:2427` (`reset`); `03-sequences.md:1520-1530` (card event; decided elsewhere); `09-decisions.md:1497-1510` (ADR-068 plan and marks).
- **Rust definition:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServiceEvent {
    /// A `Run`'s state changed (04 — Run lifecycle).
    RunStateChanged { run_id: RunId, state: RunState },
    /// One observable progress line, or a tool count (ADR-068 "What happened").
    RunProgress { run_id: RunId, progress: RunProgress },
    /// A stated or changed plan (ADR-068); `steps` are `RunStep`s of the new plan version.
    PlanChanged { run_id: RunId, plan_version: u32, steps: Vec<RunStep> },
    /// One step's mark changed (ADR-068).
    StepMarked { run_id: RunId, plan_version: u32, position: u8, mark: StepMark },
    /// The start-up report of a run or crew run is ready (G9, P5).
    StartUpReportReady { subject: StartUpSubject, items: Vec<StartUpReportItem> },
    ApprovalRequested { card: ApprovalCard },
    ApprovalDecided { approval_id: ApprovalId, state: ApprovalState, decided_on: String },
    CrewRoundChanged { crew_run_id: CrewRunId, round: u32, state: CrewRunState },
    /// A block Ordin or an agent showed, after it passed the Block Renderer's check.
    BlockShown { workspace_id: WorkspaceId, block_id: BlockId, block: Block },
    /// Remote API only: `Last-Event-ID` is older than the journal keeps; reload by commands.
    Reset,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunProgress {
    /// One plain sentence, at most 160 characters (assumed).
    pub line: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<ProgressCount>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProgressCount {
    pub done: u64,
    /// Present only when the source carried a real total ("12 of 40"); absent → "12 so far".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepMark { Next, Now, Done, Skipped }
```
`RunState`, `ApprovalState` and `CrewRunState` are the lifecycle enumerations of the 04-domain entities `Run` (`04-domain.md:177`), `Approval` (`:196`) and `CrewRun` (`:220`), defined in the domain part of 2C. `StartUpSubject` is defined under *StartUpReportItem*.

*Amended January 2026 (Phase 3 corrections, decision 80: C-1, C-16, C-17).*

- **Placement.** `RunProgress`, `ProgressCount` and `StepMark` (above), `AdapterVerb` and `RunTarget` (§2), `RunLimits`, `ModelLevel`, `ThinkingLevel` and `ModelChoice` (§2, `StartRun`) and `RepairAction` (`CommandReply`) live in `ordin-types` (`wire::run`), not in the contract crates, because no crate may depend on a contract crate other than `ordin-types` (2B-1 rule 2); `ordin-service-api` and `ordin-runtime-adapter` re-export them. `StepMark` is defined once, here; the domain part's `RunStep.mark` re-exports it.
- **`EventKind`.** Every event kind, held in `ordin-types` so that emitters which cannot depend on `ordin-service-api` name the kind they pass to `Journal::append_event` with `EventKind::as_str()`, and a misspelt kind cannot compile. It lists the variants of `ServiceEvent` above and the eight kinds HLD 08 adds in revision 0.49.0 (H08-2): `server_state_changed` (desktop only), `workspace_changed`, `needs_you_changed`, `block_available`, `locale_changed`, `brand_changed` (desktop only), `setup_progress`, `sign_in_changed`. *Amended January 2026 (ADR-109):* one more, `language_changed` (a language's wording changed; payload `{ locale, change }` fixed by Localisation and restated by the Service API spec), 19 kinds in all.

```rust
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
- **JSON example:**
```json
{"kind":"run_progress","run_id":"0192a1c2-3344-7556-8677-8899aabbccdd","progress":{"line":"Read supplier quotes","count":{"done":12,"total":40}}}
```
- **Validation:**

| Field | Rule | Error code |
|---|---|---|
| `kind` | one of the variants of this API version; a reader of the same major ignores an unknown `kind` | — |
| `RunProgress.line` | 1–160 characters (assumed) | `SVC_EVENT_INVALID` |
| `ProgressCount` | `total` absent, or `done ≤ total` | `SVC_EVENT_INVALID` |
| `StepMarked.position` | 1–7 (ADR-068: three to seven steps) | `SVC_EVENT_INVALID` |
| `ApprovalDecided.decided_on` | 1–80 characters: the computer's name or the chat ("Asha's laptop", "Telegram") | `SVC_EVENT_INVALID` |

- **Versioning:** new variants are a minor increment, each with a matching `EventKind` variant. The payloads of the eight kinds added in revision 0.49.0 are fixed by the Service API component spec's event catalogue; `EventKind` is the full list of kinds, and the command list is that spec's catalogue of 395 commands in 39 groups (*amended January 2026, ADR-113 and ADR-112: the group `knowledge_agents` with `list`, `add`, `remove`, `details`, and `memory.more_found`; event kinds `knowledge_agents_changed` and `more_found` (21 kinds); `knowledge.save_edit` and `knowledge.write` gain `type` and `description`; amended January 2026, ADR-111: 390 commands; amended January 2026, ADR-110: seven `team_knowledge.*` commands — `add_read_folder`, `stop_reading`, `adopt`, `adopt_again`, `keep_as_is`, `set_description`, `check_recall` — and `knowledge.list_read_pages`; `team_knowledge.check_now` and `team_knowledge.get` gain an optional `folder_id`; no event kind added; amended January 2026, ADR-109: ten `localisation.*` language commands, three `ordin.*` drafting commands, the desktop's `files.save_export`, and the event kind `language_changed`; amended January 2026, ADR-108: `ordin.continue_conversation`; amended January 2026, ADR-103, ADR-104 and ADR-106*), version 1.0 (amended January 2026, ADR-106: `jira.connect_token` and `jira.find_issues` in a new group; amended January 2026, ADR-103: the seven `team_knowledge.*` commands in a new group, and the Memory Service's personal-data check command — the Service API spec's catalogue carries the new totals; amended January 2026, ADR-102: 20 Slack, grant, handover-link and sign-in commands; amended January 2026, decision 80: C-16; closes G-1).

---

## 2. Runtime Adapter interface (ADR-003)

### AdapterKind
- **Crosses:** core-internal module boundary (Session Supervisor and Crew Orchestrator ↔ each adapter crate); shown in the start-up report.
- **Producer / Consumers:** each adapter declares it; Session Supervisor, Crew Orchestrator, Journal (`Run.adapter`) consume.
- **HLD trace:** `02-architecture.md:494-502`, `:504`, `:539`, `:549-558` (wrightrun HTTP Adapter, becoming the Hosted Agent Adapter at 0.43.0), `:560`; `04-domain.md:174` (`Run` field *adapter*).
- **Rust definition:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterKind {
    ClaudeLocal,
    CopilotLocal,
    /// The wrightrun HTTP Adapter; the Hosted Agent Adapter from revision 0.43.0.
    HostedAgent,
    SlackThread,
}
```
- **JSON example:** `"claude_local"`
- **Validation:** closed set; an unknown value on read is `RA_ADAPTER_UNKNOWN`.
- **Single definition (amended January 2026, decision 80: C-1, C-3):** this is the only definition. It lives in `ordin-types` (`wire::run`), the domain part's `Run.adapter` re-exports it, and the stored value is `hosted_agent` (the domain part's earlier `WrightrunHttp` variant is withdrawn, ADR-098).

### AdapterVerb
- **Crosses:** adapter → Session Supervisor; core → WebView (start-up report "adapter verbs not offered").
- **Producer / Consumers:** each adapter's `unsupported_verbs()`; start-up report, Crew Orchestrator.
- **HLD trace:** `09-decisions.md:52-60` (ADR-003 verbs, "An adapter declares the verbs it cannot offer"); `02-architecture.md:494-502`; `08-interfaces.md:35`.
- **Rust definition:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterVerb {
    Start,
    Progress,
    Pause,
    Resume,
    Read,
    Cancel,
}
```
- **JSON example:** `["cancel"]`
- **Declared gaps per adapter (from the HLD):**

| Adapter | Verbs not offered | Trace |
|---|---|---|
| `claude_local` | none | `02-architecture.md:504-512` |
| `copilot_local` | none | `02-architecture.md:539-547` |
| `hosted_agent` | none from 0.43.0; before it, `cancel` and `progress` | `02-architecture.md:557-558`; `09-decisions.md:58` |
| `slack_thread` | `pause` and `resume` as structured verbs (the decision is made in Slack), `cancel`, `progress` | `02-architecture.md:560-568`; `09-decisions.md:58` |

### RuntimeAdapter (trait)
- **Crosses:** module boundary between the Session Supervisor / Crew Orchestrator crates and each adapter crate (in-process).
- **Producer / Consumers:** implemented by Claude Local Adapter, Copilot Local Adapter, Hosted Agent Adapter, Slack Thread Adapter; called by Session Supervisor (and through it by Crew Orchestrator, Ordin Host).
- **HLD trace:** `09-decisions.md:49-66` (ADR-003); `02-architecture.md:494-502`, `:483-492`.
- **Rust definition:**
```rust
#[async_trait::async_trait]
pub trait RuntimeAdapter: Send + Sync {
    fn kind(&self) -> AdapterKind;

    /// ADR-003: each adapter states the verbs it cannot offer; the start-up report shows them.
    fn unsupported_verbs(&self) -> &'static [AdapterVerb];

    /// start: a command with arguments, or free text.
    async fn start(&self, ctx: &AdapterContext, req: StartRun) -> Result<RunHandle, AdapterError>;

    /// progress events and pauses for a decision arrive on one ordered stream per run.
    /// The stream ends after exactly one `RunEvent::Ended`.
    fn events(
        &self,
        ctx: &AdapterContext,
        run: &RunHandle,
    ) -> Result<futures::stream::BoxStream<'static, RunEvent>, AdapterError>;

    /// resume with approve or reject.
    async fn resume(
        &self,
        ctx: &AdapterContext,
        run: &RunHandle,
        decision: ResumeDecision,
    ) -> Result<(), AdapterError>;

    /// read state and result.
    async fn read(&self, ctx: &AdapterContext, run: &RunHandle) -> Result<RunSnapshot, AdapterError>;

    /// cancel, where the route offers it; otherwise `AdapterError::VerbNotOffered`.
    async fn cancel(&self, ctx: &AdapterContext, run: &RunHandle) -> Result<(), AdapterError>;

    // `export_session`/`import_session` (C-12) removed January 2026 (decision 80, reconciliation E1):
    // a paused run's session record travels through the Run Queue (`save_session_record` /
    // `load_session_record` in the Session Supervisor), not through the adapter.
}

/// Per-call context: correlation identifier and a deadline for this call (not for the run).
#[derive(Debug, Clone)]
pub struct AdapterContext {
    pub correlation_id: String,
    pub deadline: std::time::Instant,
    pub cancel: tokio_util::sync::CancellationToken,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunHandle {
    pub run_id: RunId,
    pub adapter: AdapterKind,
    /// Harness session identifier (local) or remote run identifier (hosted, Slack).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_thread_id: Option<String>,
}
```
- **JSON example (RunHandle as journalled):**
```json
{"run_id":"0192a1c2-3344-7556-8677-8899aabbccdd","adapter":"hosted_agent","runtime_session_id":"run_7Hq2kd9","remote_thread_id":"th_41aa0c"}
```
- **Validation:**

| Field | Rule | Error code |
|---|---|---|
| call of an unsupported verb | refused without contacting the route | `RA_VERB_NOT_OFFERED` |
| `runtime_session_id` | 1–256 characters (assumed) | `RA_HANDLE_INVALID` |
| `remote_thread_id` | 1–256 characters (assumed); required for `hosted_agent` and `slack_thread` once started | `RA_HANDLE_INVALID` |
| busy thread (hosted) | a second run on a busy thread is queued by the adapter, never sent (`02-architecture.md:557`) | — |

### StartRun
- **Crosses:** Session Supervisor → adapter (in-process).
- **Producer / Consumers:** Session Supervisor (from a `Task` or a crew step); each adapter.
- **HLD trace:** `09-decisions.md:53` ("start (command and arguments, or free text)"); `04-domain.md:168-171` (`Task` fields), `:173-174` (`Run` limits); `02-architecture.md:486` (limits: wall-clock, request budget, idle timeout).
- **Rust definition:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct StartRun {
    pub run_id: RunId,
    pub workspace_id: WorkspaceId,
    pub target: RunTarget,
    pub input: StartInput,
    /// Artefacts attached to the task (04 — Task, *attached artefacts*).
    pub attachments: Vec<ArtefactId>,
    pub limits: RunLimits,
    /// An existing remote thread to continue (hosted agent); absent starts a new thread.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    /// The model level and model the Usage Planner chose (ADR-062, C-37); absent for remote
    /// adapters, whose model is the remote side's. amended January 2026, decision 80: C-11.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelChoice>,
    /// The thinking setting the Usage Planner chose; absent uses the harness's own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingLevel>,
    /// Who the session's ordin tools act for; the local adapters copy it into the MCP Front's
    /// `SessionGrant` (amended January 2026, decision 80, Phase 3 reconciliation).
    pub caller: ToolCaller,
    /// The hosted agent's scope (agent profile only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_id: Option<ScopeId>,
    /// The coding task whose worktree the session works in (engineering mode only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coding_task_id: Option<CodingTaskId>,
}

/// Who a session's ordin tools act for. Lives in `ordin-types` (module
/// `ordin_types::session`), not in this section's crate: both the Runtime Adapter interface
/// (`StartRun`) and the MCP Front (`SessionGrant`) use it, and the MCP Front has no edge to
/// `ordin-runtime-adapter`. Both crates re-export it (amended January 2026, decision 80).
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

/// A harness's map from level to model name; a level absent from the map uses the harness's
/// automatic selection.
pub type LevelModels = std::collections::BTreeMap<ModelLevel, String>;

/// Implemented by each local harness adapter and given to the Usage Planner by the program
/// (2B-2 D9). Remote adapters do not implement it.
pub trait HarnessModels: Send + Sync {
    fn harness(&self) -> HarnessKind;
    /// The map used when the organisation profile has no `usage.level_models.<harness>` entry.
    fn default_level_models(&self) -> LevelModels;
    /// True when the harness accepts a model per session (C-37); when false, `StartRun.model`
    /// is ignored by the adapter.
    fn per_session_model(&self) -> bool;
    /// True when `name` is a model this harness version accepts.
    fn accepts_model(&self, name: &str) -> bool;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunTarget {
    AgentInstance { agent_instance_id: AgentInstanceId },
    RemoteAgent { remote_agent_id: RemoteAgentId },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartInput {
    Command { name: String, arguments: String },
    FreeText { text: String },
}

/// The only definition of `RunLimits` (amended January 2026, decision 80: C-1, C-4); lives in `ordin-types`
/// (`wire::run`) and the domain part's `Run.limits` re-exports it. `request_budget` is optional
/// because `07-cross-cutting.md:49` sets no default ceiling until OQ-3; when present it is ≥ 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunLimits {
    pub wall_clock_secs: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_budget: Option<u32>,
    pub idle_timeout_secs: u32,
}
```
- **JSON example:**
```json
{
  "run_id": "0192a1c2-3344-7556-8677-8899aabbccdd",
  "workspace_id": "0192a0f1-1111-7222-8333-444455556666",
  "target": {"kind": "agent_instance", "agent_instance_id": "0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff"},
  "input": {"kind": "command", "name": "compare-quotes", "arguments": "Kestrel, Acme and Northwind Q3 quotes"},
  "attachments": ["0192a1c1-0001-7002-8003-000400050006"],
  "limits": {"wall_clock_secs": 3600, "idle_timeout_secs": 600}
}
```
- **Validation:**

| Field | Rule | Error code |
|---|---|---|
| `input.name` | 1–64 characters, the name of a task the instance declares | `RA_COMMAND_UNKNOWN` |
| `input.arguments` | 0–16,384 characters (assumed) | `RA_INPUT_TOO_LARGE` |
| `input.text` | 1–16,384 characters (assumed) | `RA_INPUT_TOO_LARGE` |
| `attachments` | at most 50 (assumed); each readable under the run's folder grants | `RA_ATTACHMENT_NOT_GRANTED` |
| `limits.wall_clock_secs` | 1–28,800 (8 hours, `07-cross-cutting.md:46` crew maximum; applied to single runs as assumed) | `RA_LIMIT_OUT_OF_RANGE` |
| `limits.idle_timeout_secs` | 30–3,600 (assumed) | `RA_LIMIT_OUT_OF_RANGE` |
| `target` | `remote_agent` only for `hosted_agent` and `slack_thread`; `agent_instance` only for local adapters | `RA_TARGET_MISMATCH` |
| `limits.request_budget` | absent, or ≥ 1 | `RA_LIMIT_OUT_OF_RANGE` |
| `model` | absent for `hosted_agent` and `slack_thread`; `model.model`, when present, accepted by the harness's `HarnessModels::accepts_model` | `RA_MODEL_UNKNOWN` |

### RunEvent
- **Crosses:** adapter → Session Supervisor (in-process stream); mapped by the Supervisor into `ServiceEvent`s and journal records.
- **Producer / Consumers:** each adapter ("Owns … the mapping of CLI events to run events", `02-architecture.md:510`, `:546`); Session Supervisor, Crew Orchestrator, Usage Planner (usage counts).
- **HLD trace:** `09-decisions.md:54-57`; `04-domain.md:173-177` (Run fields, states, end reasons); `09-decisions.md:1503` (ADR-068 item 4: harness to-do list, C-41); `02-architecture.md:365` (usage counters).
- **Rust definition:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunEvent {
    Started {
        #[serde(skip_serializing_if = "Option::is_none")]
        runtime_session_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        remote_thread_id: Option<String>,
    },
    Progress { progress: RunProgress },
    /// The harness's own to-do list, where the adapter can read it (ADR-068 item 4, C-41).
    HarnessPlan { steps: Vec<String> },
    Paused { reason: PauseReason },
    Resumed,
    Usage { usage: UsageDelta },
    Ended { end: RunEnd },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PauseReason {
    /// The Rules Engine decided *ask*; the Approval Service holds the card.
    Approval { approval_id: ApprovalId },
    /// The harness is signed out (ADR-044).
    SignIn,
    /// A question for the person (a member's question, a person-decided condition).
    Person { question: String },
    /// Slack route: the decision is made in Slack (ADR-009); the link opens the thread.
    DecisionElsewhere { link: url::Url },
    /// Hosted agent: waiting for the hosted agent's approvers, who decide on their own cards;
    /// no `Answer` is accepted. amended January 2026, decision 80: C-14; name *chosen here* from
    /// HostedAgentAdapter G-HAA-1.
    RemoteApprovers,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct UsageDelta {
    pub model_turns: u32,
    pub tool_calls: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_input_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunEnd {
    pub reason: RunEndReason,
    /// The final message, plain text, at most 32,768 characters (assumed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_message: Option<String>,
    pub artefacts: Vec<ArtefactId>,
}

/// The ended states of `Run` (04-domain.md:177). The domain part's `RunState` holds these
/// plus `queued`, `running`, `paused`; this enum is the subset an adapter may report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEndReason {
    Completed,
    Failed,
    StoppedAtLimit,
    Cancelled,
    Interrupted,
    Abandoned,
    NoReply,
}
```
- **JSON example:**
```json
{"kind":"paused","reason":{"kind":"approval","approval_id":"0192a1c3-9a8b-7c6d-8e5f-4a3b2c1d0e9f"}}
```
- **Validation:**

| Field | Rule | Error code |
|---|---|---|
| stream order | exactly one `started` first and one `ended` last | `RA_EVENT_ORDER` |
| `HarnessPlan.steps` | 3–7 items, each 1–80 characters (ADR-068); otherwise dropped, run continues with events only | `RA_PLAN_INVALID` (journal only) |
| `Person.question` | 1–500 characters (assumed) | `RA_EVENT_INVALID` |
| `NoReply` | only from `slack_thread`; `Abandoned` only from `hosted_agent` and `slack_thread` (`04-domain.md:177`) | `RA_EVENT_INVALID` |

### ResumeDecision
- **Crosses:** Session Supervisor → adapter.
- **Producer / Consumers:** Approval Service (via Supervisor); adapters.
- **HLD trace:** `09-decisions.md:56` ("resume with approve or reject"); `02-architecture.md:522` (hook relay resumes the session with the decision).
- **Rust definition:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResumeDecision {
    Approve { approval_id: ApprovalId },
    Reject {
        approval_id: ApprovalId,
        /// Told to the model so it can continue (03 — golden path, error posture).
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// Answer to `PauseReason::Person`.
    Answer { text: String },
}
```
- **JSON example:** `{"kind":"reject","approval_id":"0192a1c3-9a8b-7c6d-8e5f-4a3b2c1d0e9f","reason":"The person chose Not now."}`
- **Validation:** `reason` 0–500 characters (assumed) → `RA_INPUT_TOO_LARGE`; `approval_id` must be the one the run paused on → `RA_RESUME_MISMATCH`; `Answer.text` 1–4,000 characters (assumed).

### RunSnapshot
- **Crosses:** adapter → Session Supervisor (`read`), used by *Recovery after restart*.
- **Producer / Consumers:** adapters; Session Supervisor.
- **HLD trace:** `09-decisions.md:57` ("read state and result"); `02-architecture.md:490`; `03-sequences.md:409`.
- **Rust definition:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RunSnapshot {
    pub run_id: RunId,
    /// `Run`'s lifecycle state (domain part of 2C).
    pub state: RunState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<RunEnd>,
    /// When the route was last read; drives "last checked 20 seconds ago" (11 §5.4).
    pub checked_at: Timestamp,
}
```
- **JSON example:** `{"run_id":"0192a1c2-3344-7556-8677-8899aabbccdd","state":"running","checked_at":"2026-01-10T09:44:00.120Z"}`
- **Validation:** `end` present if and only if `state` is an ended state → `RA_SNAPSHOT_INCONSISTENT`.

### AdapterError
- **Crosses:** adapter → Session Supervisor; codes reach Diagnostics only.
- **HLD trace:** `03-sequences.md:170-176` (error posture); `07-cross-cutting.md:21`.
- **Rust definition:**
```rust
#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AdapterError {
    #[error("verb not offered")] VerbNotOffered { verb: AdapterVerb },
    #[error("harness signed out")] SignedOut,
    #[error("harness version not tested")] HarnessUntested { found: String },
    #[error("route unreachable")] Unreachable,
    #[error("plan allowance exhausted")] AllowanceExhausted { service_message: String },
    #[error("remote thread busy")] ThreadBusy,
    #[error("unknown run")] UnknownRun,
    #[error("deadline passed")] Deadline,
    /// The request failed the adapter's own validation; `code` is one of `RA_TARGET_MISMATCH`,
    /// `RA_COMMAND_UNKNOWN`, `RA_INPUT_TOO_LARGE`, `RA_HANDLE_INVALID`, `RA_RESUME_MISMATCH`.
    /// amended January 2026, decision 80: C-13; name *chosen here* from ClaudeLocalAdapter G-CLA-1 and
    /// CopilotLocalAdapter G-CPA-2.
    #[error("invalid request")] InvalidRequest { code: String },
    /// The remote side refused the request (no access, unknown agent, refused resume); `code` is
    /// the adapter's journalled code. amended January 2026, decision 80: C-13; name *chosen here* from
    /// HostedAgentAdapter G-HAA-3 (also answers SlackThreadAdapter G-STA-2).
    #[error("refused")] Refused { code: String },
}
```
- **Codes:** `RA_VERB_NOT_OFFERED`, `RA_SIGNED_OUT`, `RA_HARNESS_UNTESTED`, `RA_UNREACHABLE`, `RA_ALLOWANCE_EXHAUSTED`, `RA_THREAD_BUSY`, `RA_UNKNOWN_RUN`, `RA_DEADLINE`; `InvalidRequest` and `Refused` carry their own code.

---

## 3. Rules Engine tool decision

### ToolOrigin
- **Crosses:** ordin hook → Hook Relay (hook protocol); MCP Front → Rules Engine (in-process).
- **Producer / Consumers:** ordin hook (from the harness payload's `mcp_server` field), MCP Front; Rules Engine, Journal.
- **HLD trace:** `09-decisions.md:844` (ADR-045: "the tool's origin (built-in, ordin's MCP Front, or a package's MCP server)"); `08-interfaces.md:27`.
- **Rust definition:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolOrigin {
    BuiltIn,
    OrdinMcpFront { server: String },
    PackageMcpServer { server: String },
}
```
- **JSON example:** `{"kind":"package_mcp_server","server":"supplier-db"}`
- **Validation:** `server` 1–64 characters, `^[a-zA-Z0-9_.-]+$` (assumed) → `RULE_ORIGIN_INVALID`. An MCP server name that matches a server the MCP Front serves for the session is classified `ordin_mcp_front`; the hook's claim is not trusted for that classification.

### ToolCall
- **Crosses:** Hook Relay and MCP Front → Rules Engine (in-process); recorded in the Journal as the 04-domain entity `ToolRequest` (`04-domain.md:190-192`), which stores an arguments **digest**, not the arguments.
- **HLD trace:** `08-interfaces.md:27` ("Request: session, agent instance, tool, arguments, tool origin"); `02-architecture.md:521`; `09-decisions.md:209` (ADR-098 amendment: decisions at the start of a run, before a tool call and at the end of a run).
- **Rust definition:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ToolCall {
    pub run_id: RunId,
    pub agent_instance_id: AgentInstanceId,
    pub workspace_id: WorkspaceId,
    pub point: DecisionPoint,
    pub tool: String,
    pub origin: ToolOrigin,
    /// The arguments as the harness sent them; never journalled, only their digest.
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionPoint {
    RunStart,
    BeforeToolCall,
    RunEnd,
}
```
- **JSON example:**
```json
{"run_id":"0192a1c2-3344-7556-8677-8899aabbccdd","agent_instance_id":"0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff","workspace_id":"0192a0f1-1111-7222-8333-444455556666","point":"before_tool_call","tool":"Bash","origin":{"kind":"built_in"},"arguments":{"command":"git push origin main"}}
```
- **Validation:**

| Field | Rule | Error code |
|---|---|---|
| `tool` | 1–128 characters (assumed) | `RULE_TOOL_NAME_INVALID` |
| `arguments` | serialised at most 1 MiB (assumed); larger → deny | `RULE_ARGUMENTS_TOO_LARGE` |
| `run_id` / `agent_instance_id` | match the session the call came from (Hook Relay: the session secret's binding) | `RULE_SESSION_MISMATCH` |

### ToolDecision
- **Crosses:** Rules Engine → Hook Relay, MCP Front, Copilot Local Adapter, Crew Orchestrator (in-process); journalled with the rule that produced it; shown in Diagnostics.
- **Producer / Consumers:** Rules Engine; Hook Relay, MCP Front, Approval Service (on *ask*), Journal.
- **HLD trace:** `02-architecture.md:645-652` ("Each decision is one of allow, deny, or ask a person. Every decision is journalled with the rule that produced it"; sources: declaration gates, tool-call rules, argument rules; application policy: agent-folder and folder-grant limits, shell-command policy, approval policy; agent's own tool policy from ADR-090); `09-decisions.md:86-92` (ADR-005); `04-domain.md:190-192`.
- **Rust definition:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// The Rules Engine's full decision; keeps its name. The domain part's stored verdict of a
/// Tool request is `ToolRequestDecision` (amended January 2026, decision 80: C-2).
pub struct ToolDecision {
    #[serde(flatten)]
    pub verdict: Verdict,
    pub rule: RuleRef,
    pub decided_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum Verdict {
    Allow,
    Deny { reason: String },
    /// Ask a person: what the Approval Service builds the card from.
    Ask { proposal: ActionProposal },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RuleRef {
    pub source: RuleSource,
    /// Rule identifier within its source (a declaration rule name, or a fixed application
    /// policy identifier such as `app.folder_grant`).
    pub rule_id: String,
    /// For declaration and agent-tool-policy rules: the agent instance whose rule set it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_instance_id: Option<AgentInstanceId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleSource {
    DeclarationGate,
    DeclarationToolRule,
    DeclarationArgumentRule,
    AgentToolPolicy,
    AppFolderPolicy,
    AppShellPolicy,
    AppApprovalPolicy,
    /// No rule could be evaluated (parse error, engine error): deny, fail closed.
    FailClosed,
}

/// What an *ask* proposes; the Approval Service turns it into an `ApprovalCard`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ActionProposal {
    pub action: String,
    pub destination: String,
    pub affects: Vec<AffectedItem>,
    pub consequence: String,
}
```
`Verdict` uses the tag `decision` instead of `kind` because `ToolDecision` is flattened into journal records that carry their own `kind`.
- **JSON example:**
```json
{
  "decision": "ask",
  "proposal": {
    "action": "Push to GitHub",
    "destination": "acme/supplier-portal, branch main",
    "affects": [{"kind": "record", "label": "3 commits on main"}],
    "consequence": "The three commits become visible to everyone who can open the repository."
  },
  "rule": {"source": "app_approval_policy", "rule_id": "app.external_action"},
  "decided_at": "2026-01-10T09:41:07.305Z"
}
```
- **Validation:**

| Field | Rule | Error code |
|---|---|---|
| `Deny.reason` | 1–500 characters (assumed); plain words, shown to the model and in Diagnostics | `RULE_REASON_INVALID` |
| `rule.rule_id` | 1–128 characters (assumed) | `RULE_ID_INVALID` |
| `rule` with `fail_closed` | verdict is `deny` | `RULE_FAIL_CLOSED_NOT_DENY` (engine defect, test-only) |
| `proposal.*` | see `ApprovalCard` limits | `APPR_*` |
| combination | an agent's own tool policy can only narrow (ADR-090): it never turns a deny into allow or ask | `RULE_POLICY_WIDENS` (load-time refusal) |

---

## 4. ordin hook protocol (ADR-045)

### HookEnvironment (names fixed by this spec)
- **Crosses:** core → harness process environment → ordin hook process.
- **Producer / Consumers:** Claude Local Adapter and Copilot Local Adapter set them when starting a session; the ordin hook reads them.
- **HLD trace:** `09-decisions.md:844` ("an authenticated local channel (a per-session secret in its environment)"; "whose name is fixed in `/spec`"); `02-architecture.md:520-525`, `:831`.
- **Definition:**

| Variable | Value | Rule |
|---|---|---|
| `ORDIN_HOOK_ENDPOINT` | Path of the Hook Relay's Unix domain socket, `<app data root>/run/hook.sock`, mode `0600` (macOS, Linux); `\\.\pipe\ordin-hook-<installation id>` on Windows | absolute path, at most 104 bytes on macOS (socket path limit) |
| `ORDIN_HOOK_SECRET` | 32 random bytes, base64url without padding (43 characters), per session | regenerated for every session; never journalled or logged |
| `ORDIN_SESSION_ID` | the `RunId` the session belongs to | UUIDv7 |
| `ORDIN_HOOK_PROTOCOL` | `1` | integer |

The program is `ordin-hook` (`ordin-hook.exe` on Windows), registered for Claude Code through `--settings` as a `PreToolUse` command hook with `"onFailure": "block"` and for the Copilot CLI as a `preToolUse` command hook (`09-decisions.md:846-847`).

### HookRequest
- **Crosses:** harness ↔ core (hook channel): ordin hook → Hook Relay, one JSON line per connection.
- **Producer / Consumers:** ordin hook (after mapping the harness payload); Hook Relay, which builds a `ToolCall`.
- **HLD trace:** `08-interfaces.md:27`; `08-interfaces.md:94` (harness payloads: Claude Code `tool_name`, `tool_input`, `tool_use_id`, `mcp_server`; Copilot `toolName`, `toolArgs`); `09-decisions.md:844-850`.
- **Rust definition:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HookRequest {
    pub protocol: u16,
    pub call_id: HookCallId,
    /// From ORDIN_SESSION_ID.
    pub session: RunId,
    pub agent_instance_id: AgentInstanceId,
    /// HMAC-SHA256 of the canonical JSON of `message`, keyed with ORDIN_HOOK_SECRET,
    /// base64url without padding.
    pub mac: String,
    pub message: HookMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HookMessage {
    /// Before every tool call: decided by the Rules Engine.
    PreToolUse {
        harness: HarnessKind,
        tool: String,
        origin: ToolOrigin,
        arguments: serde_json::Value,
        /// The harness's own tool-use identifier, where it gives one (`tool_use_id`).
        #[serde(skip_serializing_if = "Option::is_none")]
        harness_tool_use_id: Option<String>,
        /// The sub-agent that asked, when the harness payload names it. Conditional on C-78:
        /// filled only once the spike confirms the payload carries it; absent until then
        /// (additive; amended January 2026, decision 80: C-20).
        #[serde(skip_serializing_if = "Option::is_none")]
        subagent: Option<String>,
    },
    /// After a tool call: feeds the journal (ADR-045 *Other events*).
    PostToolUse {
        harness: HarnessKind,
        tool: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        harness_tool_use_id: Option<String>,
        succeeded: bool,
    },
    /// The agent's turn is ending: lets Ordin check the result first (ADR-036).
    Stop { harness: HarnessKind },
}

// `HarnessKind` is defined once, in part 1 (`ordin_types::domain::identity`, entity
// `HarnessAttachment`); this module re-exports it (amended January 2026, decision 80: C-1).
pub use crate::domain::identity::HarnessKind;
```
- **JSON example:**
```json
{"protocol":1,"call_id":"0192a1c3-1200-7a00-8b00-c00d00e00f00","session":"0192a1c2-3344-7556-8677-8899aabbccdd","agent_instance_id":"0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff","mac":"x3mQ9Jt0b2pV8sYk1LwZr6NcA4fH7eDuIq5oGvRnTyE","message":{"kind":"pre_tool_use","harness":"claude_code","tool":"mcp__supplier-db__update_rate","origin":{"kind":"package_mcp_server","server":"supplier-db"},"arguments":{"supplier":"Kestrel","rate":412.5},"harness_tool_use_id":"toolu_01AbCdEf"}}
```
- **Validation:**

| Field | Rule | Error code |
|---|---|---|
| line | one JSON object, at most 1 MiB (assumed), terminated by `\n` | `HOOK_MALFORMED` → deny |
| `protocol` | equals `1`; a greater value → deny with "ordin needs updating" | `HOOK_PROTOCOL_UNSUPPORTED` |
| `mac` | constant-time match against the session's secret | `HOOK_UNAUTHENTICATED` → deny, journalled without the payload |
| `session` | a run with a live session and a secret | `HOOK_SESSION_UNKNOWN` → deny |
| `call_id` | UUIDv7, not seen before in this session | `HOOK_REPLAYED` → deny |
| peer | on Unix, the socket peer's user id equals the core's (`SO_PEERCRED` / `getpeereid`) (assumed) | `HOOK_PEER_REFUSED` → deny |

- **Versioning:** `protocol` field and `ORDIN_HOOK_PROTOCOL`, both `1`. The core accepts the current and the previous protocol number; the hook program is shipped with the core, so a mismatch arises only during an update.

### HookReply
- **Crosses:** core → ordin hook (hook channel), on the same connection.
- **Producer / Consumers:** Hook Relay; ordin hook, which maps a final verdict to the harness's own format (Claude Code `hookSpecificOutput.permissionDecision`, Copilot `permissionDecision`).
- **HLD trace:** `08-interfaces.md:27` ("Reply: allow, deny with reason, or ask (held until decided or the deadline)"); `02-architecture.md:521-524`; `09-decisions.md:848-849` (fail closed, waiting for a person).
- **Rust definition:**
```rust
/// The relay writes zero or one `Holding` line, then exactly one final line.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HookReply {
    /// The decision is *ask*: the card is open; the hook keeps waiting until `deadline`.
    Holding { call_id: HookCallId, approval_id: ApprovalId, deadline: Timestamp },
    Allow { call_id: HookCallId },
    /// To a `stop`, `Deny` means "do not end yet": the harness keeps the turn open and
    /// `reason` is given to the agent (amended January 2026, decision 80: C-20).
    Deny { call_id: HookCallId, reason: String },
    /// Reply to `post_tool_use` and `stop`.
    Ack { call_id: HookCallId },
}
```
- **JSON example (ask, then the person approves):**
```json
{"kind":"holding","call_id":"0192a1c3-1200-7a00-8b00-c00d00e00f00","approval_id":"0192a1c3-9a8b-7c6d-8e5f-4a3b2c1d0e9f","deadline":"2026-01-10T09:41:57.000Z"}
{"kind":"allow","call_id":"0192a1c3-1200-7a00-8b00-c00d00e00f00"}
```
- **Validation and timing:**

| Rule | Value | Error code / effect |
|---|---|---|
| allow or deny without *ask* | within 2 s of the request (assumed) | hook denies at its own deadline: `HOOK_CORE_TIMEOUT` |
| hook's own deadline | 50 s from the request (assumed), always shorter than the harness's hook timeout | deny with reason "waiting for the person's approval" (`09-decisions.md:849`) |
| harness hook timeout registered | 60 s (assumed) for both harnesses | the Claude Code spike observed a 5 s timeout blocking with `onFailure: block` (`09-decisions.md:846`) |
| `Deny.reason` | 1–500 characters (assumed) | — |
| any parse failure, closed connection, or unknown `kind` | deny | `HOOK_REPLY_MALFORMED` |
| decision after the deadline | the session is resumed with the decision by the Session Supervisor (`02-architecture.md:522`) | — |
| harness verdicts | the hook never returns `ask` or `defer` to the harness; sessions run with `--permission-prompts none` | — |

---

## 5. Approval card (Approval Service)

### ApprovalCard
- **Crosses:** core → WebView (Tauri IPC, in `ServiceEvent::ApprovalRequested`); server → desktop (Remote API); core → Channel Gateway (text form with buttons).
- **Producer / Consumers:** Approval Service only (never Ordin, never an agent, never the Block Renderer from Ordin's data); App Shell, Block Renderer's fixed approval screen, Server Connector, Channel Gateway.
- **HLD trace:** `02-architecture.md:654-657`; `02-architecture.md:332` ("Never renders an approval from Ordin's data"); `09-decisions.md:540` (ADR-028); `11-ux-guidelines.md:260-300` (§5.5 anatomy, fold rule, verbs); `04-domain.md:194-196` (entity `Approval`), `:276-278` (`CardReference`).
- **Rust definition:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApprovalCard {
    pub approval_id: ApprovalId,
    pub backend: BackendId,
    pub workspace_id: WorkspaceId,
    /// The run or crew round that asked (entity `Approval`, field *run or crew round*).
    pub asked_by: ApprovalAsker,
    /// "[Verb] [object]?": the question in plain words, e.g. "Publish the revised account brief?"
    pub question: String,
    /// The approve button's label; repeats the verb ("Publish", "Approve and post").
    pub verb: String,
    /// Where: the destination.
    pub where_line: String,
    /// Affects: records or files, by name and version.
    pub affects: Vec<AffectedItem>,
    /// Changes: what will be different afterwards.
    pub changes: String,
    /// What the first button shows; absent when nothing more can be shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<ApprovalContent>,
    /// `reject` → "Reject"; `not_now` → "Not now" where the person may decide later.
    pub reject_style: RejectStyle,
    /// Single-use references the buttons carry (entity `CardReference`).
    pub buttons: ApprovalButtons,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApprovalAsker {
    Run { run_id: RunId },
    CrewRound { crew_run_id: CrewRunId, round: u32 },
    /// The person's own request (Share…, Publish a copy…, Send…).
    Person,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AffectedItem {
    File { label: String, version: Option<String>, place_id: Option<PlaceId> },
    Record { label: String },
    Recipient { name: String, address: String, outside_company: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectStyle { Reject, NotNow }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApprovalButtons {
    pub approve: CardRef,
    pub reject: CardRef,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_details: Option<CardRef>,
}

/// Opaque single-use reference; the value of entity `CardReference`'s *reference* field.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CardRef(String);
```
The fixed lines ("From ordin — built from the action, not written by an agent" and "Nothing is sent or changed outside ordin until you choose.") are not in the payload: the screens draw them from the message catalogue, so no data can change them (`11-ux-guidelines.md:274`).
- **JSON example:**
```json
{
  "approval_id": "0192a1c3-9a8b-7c6d-8e5f-4a3b2c1d0e9f",
  "backend": "0192a1b4-5c6d-7e8f-9a0b-1c2d3e4f5a6b",
  "workspace_id": "0192a0f1-1111-7222-8333-444455556666",
  "asked_by": {"kind": "run", "run_id": "0192a1c2-3344-7556-8677-8899aabbccdd"},
  "question": "Publish the revised account brief?",
  "verb": "Publish",
  "where_line": "Sales workspace",
  "affects": [{"kind": "file", "label": "Account brief", "version": "4"}],
  "changes": "Updated revenue table and three action items",
  "content": {"kind": "changes", "label": "See the changes", "change_set": {"kind": "artefact", "artefact_id": "0192a1c4-0000-7000-8000-00000000a001"}},
  "reject_style": "reject",
  "buttons": {"approve": "q8Zt3VxN0pLk2mRb7sYw1A", "reject": "Lm4Qp0Rt7Vx2Zb5Nc8Df3g"},
  "created_at": "2026-01-10T09:41:07.312Z"
}
```
- **Validation:**

| Field | Rule | Error code |
|---|---|---|
| `question` | 1–120 characters (assumed), ends with "?" | `APPR_QUESTION_INVALID` |
| `verb` | 1–32 characters (assumed); not "OK", "Yes" or "Continue" (UX-A2) | `APPR_VERB_INVALID` |
| `where_line` | 1–200 characters (assumed) | `APPR_WHERE_INVALID` |
| `affects` | 1–50 items (assumed); each `label` 1–200 characters | `APPR_AFFECTS_INVALID` |
| `changes` | 1–1,000 characters (assumed) | `APPR_CHANGES_INVALID` |
| `CardRef` | 22 characters base64url (16 random bytes), fits Telegram's 64-byte button data (`04-domain.md:277`) | `APPR_CARD_REF_INVALID` |
| press | a reference used once; refused after use, after a decision elsewhere or after the run ended (`04-domain.md:278`) | `APPR_CARD_ALREADY_DECIDED`, `APPR_CARD_CLOSED` |
| press from a desktop | only from a paired desktop (`09-decisions.md:2441`) | `SVC_UNAUTHENTICATED` |
| timeout | none: a card is never approved by timeout and survives a restart (`11-ux-guidelines.md:292`) | — |

### ApprovalContent
- **Crosses:** inside `ApprovalCard`.
- **HLD trace:** `11-ux-guidelines.md:271-288` ("See the changes", "See the page", "See the files", "See the command", "See what will be sent"; a message or command shown open; fold rule); `02-architecture.md:654` and `09-decisions.md:1585-1592` (ADR-070 email card).
- **Rust definition:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApprovalContent {
    /// A message shown open on the card; longer than 12 lines folds to its first 6.
    Message { label: String, text: String, destination_note: Option<String> },
    /// A command shown in full in a code block, with where it runs.
    Command { label: String, command: String, runs_in: String },
    Changes { label: String, change_set: ContentRef },
    Page { label: String, page: ContentRef },
    Files { label: String, files: Vec<ContentRef> },
    /// ADR-070: built from the `EmailDraft`, never from an agent's words.
    Email { label: String, email: EmailApproval },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContentRef {
    Artefact { artefact_id: ArtefactId },
    PlaceFile { place_id: PlaceId, relative_path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EmailApproval {
    pub draft_id: EmailDraftId,
    pub from: String,
    pub to: Vec<EmailRecipient>,
    pub cc: Vec<EmailRecipient>,
    pub bcc: Vec<EmailRecipient>,
    pub subject: String,
    /// The whole message as it will be sent, with the person's signature.
    pub body: String,
    pub attachments: Vec<EmailAttachmentLine>,
    /// Agent instance display name, "Ordin" or absent when the person wrote it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drafted_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_reply_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EmailRecipient { pub name: Option<String>, pub address: String, pub outside_company: bool }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EmailAttachmentLine { pub file_name: String, pub comes_from: String, pub bytes: u64 }
```
- **JSON example:**
```json
{"kind":"email","label":"See what will be sent","email":{"draft_id":"0192a1c5-7777-7888-8999-aaaabbbbcccc","from":"asha.patel@acme.co.uk","to":[{"name":"Dan Moss","address":"dan.moss@kestrel.co.uk","outside_company":true}],"cc":[],"bcc":[],"subject":"Re: Q3 quote","body":"Hi Dan,\n\nThanks for the revised quote…\n\nAsha Patel\nProcurement, Acme","attachments":[{"file_name":"Q3 comparison.pdf","comes_from":"Suppliers › Q3","bytes":184320}],"drafted_by":"Supplier analyst","in_reply_to":"Dan Moss, 14 Oct, “Q3 quote”"}}
```
- **Validation:**

| Field | Rule | Error code |
|---|---|---|
| `label` | one of the fixed labels of §5.5, from the message catalogue | `APPR_CONTENT_LABEL_INVALID` |
| `Message.text` | 1–100,000 characters (assumed); fold at more than 12 lines, showing 6 (`11-ux-guidelines.md:288`) | `APPR_CONTENT_TOO_LARGE` |
| `Command.command` | 1–8,000 characters (assumed); shown in full | `APPR_CONTENT_TOO_LARGE` |
| `Files.files` | 1–100 (assumed) | `APPR_CONTENT_TOO_LARGE` |
| `to + cc + bcc` | 1 to the profile's maximum recipients (default 20, `09-decisions.md:1596`) | `APPR_TOO_MANY_RECIPIENTS` |
| recipients | inside the company only, when the profile says so (ADR-070) | `APPR_RECIPIENT_NOT_ALLOWED` |
| attachments | none from places the profile excludes (ADR-070) | `APPR_ATTACHMENT_NOT_ALLOWED` |

### ApprovalPress (command input)
- **Crosses:** WebView → core (`approval.decide`); desktop → server (`POST /v1/approval/decide`); Channel Gateway → Approval Service.
- **Producer / Consumers:** screens module, Server Connector, Channel Gateway; Approval Service.
- **HLD trace:** `03-sequences.md:1524-1528`; `09-decisions.md:2441` ("open card, single use, from a paired desktop"); `04-domain.md:277` (buttons: approve, not now, show details).
- **Rust definition:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ApprovalPress {
    pub card_ref: CardRef,
    pub decision: PressDecision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PressDecision { Approve, Reject }
```
- **JSON example:** `{"card_ref":"q8Zt3VxN0pLk2mRb7sYw1A","decision":"approve"}`
- **Validation:** `card_ref` must be the button for the chosen `decision` → `APPR_CARD_REF_MISMATCH`; plus the press rules under *ApprovalCard*.

---

## 6. Start-up report items

### StartUpReportItem
- **Crosses:** core → WebView (`ServiceEvent::StartUpReportReady`); server → desktop; stored with the run or crew run as the 04-domain entity `StartupReport` (`04-domain.md:253-255`), whose field lists hold these items.
- **Producer / Consumers:** the run's local adapter (`03-sequences.md:143`), Crew Orchestrator (`03-sequences.md:338`), Release Builder and Agent Loader in the agent profile (`03-sequences.md:1555`, `:1566`); App Shell ("What this agent will and won't do", `11-ux-guidelines.md:239`), Journal.
- **HLD trace:** `08-interfaces.md:35`; `04-domain.md:253-255`; `09-decisions.md:129` (ADR-007: rules not applicable to a local session reported by name); `09-decisions.md:1480` (ADR-067: "Shows its own screens, which ordin does not display"); `02-architecture.md:502`.
- **Rust definition:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartUpSubject {
    Run { run_id: RunId },
    CrewRun { crew_run_id: CrewRunId },
    /// The start of a hosted agent's release, whose report the Agent Loader builds
    /// (amended January 2026, decision 80: C-29).
    AgentRelease { agent_release_id: AgentReleaseId },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartUpReportItem {
    EnforcedRule { rule: RuleRef, plain: String },
    RuleNotEnforced { rule: RuleRef, plain: String, reason: NotEnforcedReason },
    UnavailablePart { part: String, plain: String, reason: UnavailableReason },
    AvailableTool { tool: String, origin: ToolOrigin, plain: String },
    VerbNotOffered { adapter: AdapterKind, verb: AdapterVerb, plain: String },
    /// ADR-067: a package's MCP server offers MCP Apps interfaces, which are not drawn.
    OwnScreensNotShown { server: String, plain: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotEnforcedReason {
    /// ADR-007: chat triggers, recurring-duty gates, settings only a chat system edits.
    NotApplicableToLocalSession,
    /// The route cannot carry the rule (for example structured approval over Slack).
    RouteCannotCarry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableReason {
    /// An executable part not trusted under the organisation's policy (04 — Agent instance).
    NotTrusted,
    MissingDependency,
    HarnessCannotRun,
}
```
`plain` is the sentence the person reads; identifiers (`rule`, `verb`, `tool`) are kept for Diagnostics only (`11-ux-guidelines.md:75`: never show rule identifiers or verb names).
- **JSON example:**
```json
[
  {"kind":"enforced_rule","rule":{"source":"app_approval_policy","rule_id":"app.external_action"},"plain":"Asks you before anything leaves your computer."},
  {"kind":"verb_not_offered","adapter":"slack_thread","verb":"cancel","plain":"Can't be stopped from ordin once asked; it answers in Slack."},
  {"kind":"own_screens_not_shown","server":"spend-explorer","plain":"Shows its own screens, which ordin does not display."}
]
```
- **Validation:**

| Field | Rule | Error code |
|---|---|---|
| list | at most 200 items (assumed) | `START_TOO_MANY_ITEMS` |
| `plain` | 1–200 characters (assumed), no identifier from the HLD's "never shown" column | `START_PLAIN_INVALID` |
| collapse | collapsed to one line when there is no `rule_not_enforced`, `unavailable_part`, `verb_not_offered` or `own_screens_not_shown` item (`11-ux-guidelines.md:239`) | — |

---

## 7. Block catalogue (ADR-028, ADR-067, ADR-068, ADR-070)

### Block
- **Crosses:** Ordin's harness session → core (MCP Front tools `show` and `report`, as JSON arguments); agent sessions → core (`report`); core → WebView and server → desktop (`ServiceEvent::BlockShown`); core → Channel Gateway (text form, `02-architecture.md:599`).
- **Producer / Consumers:** Ordin (through `show`), agents (through `report`, `plan`); MCP Front passes it to the Block Renderer's check; Block Renderer draws; Journal and Artefact Store keep reports; Channel Gateway renders text forms.
- **HLD trace:** `09-decisions.md:533-548` (ADR-028: catalogue, "data, never markup, script or style", "A block that fails validation is dropped and Ordin is told why"); `08-interfaces.md:22`; `11-ux-guidelines.md:561-602` (§5.15 kinds and rules); `09-decisions.md:1466-1492` (ADR-067); `09-decisions.md:1494-1523` (ADR-068); `09-decisions.md:1599` (ADR-070 Email list and Email); `02-architecture.md:330-337`; `10-risks.md:45` (A-34).
- **Rust definition:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Block {
    Message(MessageBlock),
    Question(QuestionBlock),
    Form(FormBlock),
    Documents(DocumentsBlock),
    Preview(PreviewBlock),
    Document(DocumentBlock),
    Changes(ChangesBlock),
    PlanProgress(PlanProgressBlock),
    Table(TableBlock),
    Chart(ChartBlock),
    KeyFigures(KeyFiguresBlock),
    Report(ReportBlock),
    EmailList(EmailListBlock),
    Email(EmailBlock),
    TelegramList(TelegramListBlock),
    SlackList(SlackListBlock),   // amended January 2026 (ADR-102)
    Result(ResultBlock),
    AgentProposal(AgentProposalBlock),
    AgentLibrary(AgentLibraryBlock),
    CrewPlan(CrewPlanBlock),
    Memory(MemoryBlock),
    Knowledge(KnowledgeBlock),
}
```
There is no approval or confirmation kind, and no kind carries a button that approves, sends, posts or publishes (`11-ux-guidelines.md:589`).
- **Validation common to every kind:**

| Rule | Value | Error code |
|---|---|---|
| serialised size of one block | at most 2 MiB (A-34's 2 MB for a report, applied to every block, assumed for non-report kinds) | `BLOCK_TOO_LARGE` |
| unknown `kind` or field | refused | `BLOCK_KIND_UNKNOWN`, `BLOCK_FIELD_UNKNOWN` |
| any text field | drawn as text; strings that look like markup are shown literally | — |
| links | only to a document or place in the workspace (`ContentRef`), never a URL (ADR-067 *Data only*) | `BLOCK_LINK_NOT_ALLOWED` |
| colours, styles, formulas, expressions | no field exists for them; a string starting with `=` in a table cell is text | — |
| result of refusal | the block is dropped and a `BlockRejection` is returned to the sender | — |

- **Versioning:** the catalogue is versioned with the Service API minor version; a new kind is a minor increment. The MCP tool schemas published to sessions carry the same version.

### MessageBlock
- **HLD trace:** `09-decisions.md:537`; `11-ux-guidelines.md:567`.
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MessageBlock {
    /// Plain text; paragraphs separated by a blank line.
    pub text: String,
    /// Attached results (§5.15: "Ordin's reply, with attached results").
    pub attachments: Vec<ContentRef>,
    /// Amended January 2026 (ADR-111): the recalls of the turn that produced this answer, 0–20; omitted when empty.
    /// The screens show **What Ordin used** from them (`memory.used`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memory_used: Vec<MemoryRecallId>,
}
```
- **JSON example:** `{"kind":"message","text":"Supplier analyst will compare the three quotes.","attachments":[]}`
- **Validation:** `text` 1–8,000 characters (assumed) → `BLOCK_TEXT_INVALID`; `attachments` at most 20 (assumed) → `BLOCK_TOO_MANY_ITEMS`; `memory_used` at most 20 distinct ids → `BLOCK_TOO_MANY_ITEMS` (*amended January 2026, ADR-111*).

### QuestionBlock
- **HLD trace:** `11-ux-guidelines.md:568` ("One question, two to four choices, an 'other' answer in words"); `09-decisions.md:1420` (Ask me: path names are the choices).
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct QuestionBlock {
    pub question: String,
    pub choices: Vec<String>,
    pub allow_other: bool,
}
```
- **JSON example:** `{"kind":"question","question":"Which quarter should I compare?","choices":["Q2","Q3"],"allow_other":true}`
- **Validation:** `question` 1–300 characters (assumed); `choices` 2–4 items (HLD), each 1–60 characters (assumed), unique → `BLOCK_CHOICES_INVALID`.

### FormBlock
- **HLD trace:** `11-ux-guidelines.md:569`, `:231-233` (§5.3: labelled fields, document pickers, choices, filled from the conversation; the task's own verb); `11-ux-guidelines.md:1158-1162` (§11.3: kinds text, document, place, choice).
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct FormBlock {
    pub title: String,
    /// The task's own verb on the submit button ("Compare quotes"); never an approval verb.
    pub verb: String,
    pub target: RunTarget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    pub fields: Vec<FormField>,
}

/// The Usage Planner's `UsageEstimate` (UsagePlanner.md), restated field for field for the block
/// catalogue; carried by `CrewPlanBlock.estimate` (amended January 2026, orchestrator decision F2).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct UsageEstimate {
    pub band: EstimateBand,
    pub min_turns: u32,
    pub max_turns: u32,
    /// Share of a typical day's use, in per-mille of 250 turns.
    pub typical_day_permille: u32,
    pub based_on: Vec<EstimateBasis>,
    /// True when the band reaches the person's "Ask before work estimated as" choice.
    pub ask_first: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstimateBand { Light, Medium, Heavy }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EstimateBasis {
    EarlierRuns { count: u32 },
    Limits { max_rounds: u32 },
    Members { count: u32 },
    Steps { count: u32 },
    Documents { count: u32, total_bytes: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct FormField {
    pub name: String,
    pub label: String,
    pub required: bool,
    #[serde(flatten)]
    pub input: FormInput,
    /// Set when Ordin filled the field from the conversation or the workspace.
    pub filled_by_ordin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "input", rename_all = "snake_case")]
pub enum FormInput {
    Text { value: Option<String>, placeholder: Option<String> },
    Document { value: Option<ContentRef> },
    Place { value: Option<PlaceId> },
    Choice { options: Vec<String>, value: Option<String> },
}
```
- **JSON example:** `{"kind":"form","title":"Compare quotes","verb":"Compare quotes","target":{"kind":"agent_instance","agent_instance_id":"0192a0f9-aaaa-7bbb-8ccc-ddddeeeeffff"},"command":"compare-quotes","fields":[{"name":"arguments","label":"Quotes to compare","required":true,"input":"text","value":"Kestrel, Acme and Northwind Q3","placeholder":null,"filled_by_ordin":true}]}`
- **Validation:** `fields` 1–20 (assumed); `name` `^[a-z][a-z0-9_]{0,31}$`, unique; `label` 1–80 characters; `Text.value` 0–16,384 characters; `Choice.options` 2–20 (assumed); `verb` not an approval verb from the Approval Service's list → `BLOCK_FORM_INVALID`.

### DocumentsBlock, PreviewBlock, DocumentBlock, ChangesBlock
- **HLD trace:** `09-decisions.md:537` (file list and folder tree; preview; change view); `11-ux-guidelines.md:570-573`.
- **Note:** these kinds draw application data (places, files, versions, change sets) that the core already holds; Ordin supplies references only, so no content enters through the block.
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct DocumentsBlock { pub workspace_id: WorkspaceId, pub items: Vec<ContentRef> }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PreviewBlock { pub item: ContentRef }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct DocumentBlock { pub item: ContentRef }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ChangesBlock {
    pub before: ContentRef,
    pub after: ContentRef,
    pub view: ChangeView,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeView { TextDifferences, BeforeAndAfterPages, BeforeAndAfterImages }
```
- **JSON example:** `{"kind":"preview","item":{"kind":"place_file","place_id":"0192a0f2-2222-7333-8444-555566667777","relative_path":"Q3/Kestrel quote.pdf"}}`
- **Validation:** `DocumentsBlock.items` 1–500 (assumed); every `ContentRef` resolvable and readable by the person → `BLOCK_REF_UNRESOLVED`; `relative_path` 1–1,024 bytes, no `..` segment, no leading `/` → `BLOCK_PATH_INVALID`.

### PlanProgressBlock and the `plan` tool arguments
- **Crosses:** agent session → MCP Front (`plan` tool); Ordin → `show`; drawn from the Journal's plan, marks and events.
- **HLD trace:** `09-decisions.md:1494-1523` (ADR-068: three to seven steps of at most 80 characters, stated before the first other tool call, each marked *now* or *done*); `04-domain.md:244-247` (`RunStep`); `11-ux-guidelines.md:244-252`.
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PlanProgressBlock {
    pub subject: StartUpSubject,
}

/// Arguments of the MCP Front's `plan` tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlanToolArgs {
    /// State (or restate) the plan; restating makes a new plan version.
    State { steps: Vec<String> },
    Mark { position: u8, mark: AgentStepMark },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStepMark { Now, Done, Skipped }
```
- **JSON example:** `{"action":"state","steps":["Read the supplier quotes","Compare the Q3 rates","Write the summary"]}`
- **Validation:** `steps` 3–7 items, each 1–80 characters (ADR-068) → `BLOCK_PLAN_INVALID`; `position` within the current plan → `BLOCK_PLAN_POSITION_INVALID`; at most one step marked *now* at a time; a step marked *done* with no recorded activity reads "Done — the agent reports no activity for this step" (ADR-068) — not an error. Plan versions per run: at most 20 (assumed) → `BLOCK_PLAN_TOO_MANY_VERSIONS`.

### TableBlock
- **Namespacing (amended January 2026, decision 80: C-1):** `TableColumn`, `ColumnType`, `ChartKind`, `KeyFigure`, `FigureChange` and `ReportPart` in this section are the block catalogue's types in `ordin-service-api`, with numbers as `f64`. The domain part's types of the same names (`ordin_types::domain::runs`, entity `Report`) are the stored family, with numbers as `DecimalString`; they are distinct types. The block check refuses a non-finite `f64` and converts each value into a `DecimalString` before the Journal stores it.
- **HLD trace:** `09-decisions.md:1471` (ADR-067 Table: column kinds text, number, money with its currency, percentage, date, document link); `10-risks.md:45` (A-34: 10,000 rows, 30 columns).
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TableBlock {
    pub title: String,
    pub columns: Vec<TableColumn>,
    /// Row-major; each row has exactly one cell per column, in column order.
    pub rows: Vec<Vec<Cell>>,
    pub source: ReportPartSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TableColumn {
    pub name: String,
    #[serde(flatten)]
    pub column_type: ColumnType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ColumnType {
    Text,
    Number { decimals: Option<u8> },
    /// ISO 4217 code, uppercase.
    Money { currency: String },
    Percentage,
    Date,
    DocumentLink,
}

/// A cell: `null`, a string (text, date as YYYY-MM-DD), a number (number, money, percentage
/// as a fraction 0.08 = 8 %), or a `ContentRef` object (document link).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Cell {
    Empty,
    Number(f64),
    Text(String),
    Link(ContentRef),
}
```
- **JSON example:**
```json
{"kind":"table","title":"Q3 rates by supplier","columns":[{"name":"Supplier","type":"text"},{"name":"Rate","type":"money","currency":"GBP"},{"name":"Change","type":"percentage"}],"rows":[["Kestrel",412.5,0.08],["Acme",398.0,-0.02]],"source":{"kind":"documents","documents":[{"item":{"kind":"place_file","place_id":"0192a0f2-2222-7333-8444-555566667777","relative_path":"Q3/rates.xlsx"},"version":"7"}]}}
```
- **Validation:**

| Field | Rule | Error code |
|---|---|---|
| `title` / `name` | 1–120 / 1–60 characters (assumed); column names unique | `BLOCK_TABLE_INVALID` |
| `columns` | 1–30 (A-34) | `BLOCK_TABLE_TOO_WIDE` |
| `rows` | 0–10,000 (A-34) | `BLOCK_TABLE_TOO_LONG` |
| cell type | matches its column's type; numbers finite (no NaN, no infinity) | `BLOCK_CELL_TYPE_MISMATCH` |
| `Money.currency` | `^[A-Z]{3}$` (ISO 4217) | `BLOCK_CURRENCY_INVALID` |
| `Text` cell | 0–2,000 characters (assumed) | `BLOCK_CELL_TOO_LONG` |
| `Date` cell | `YYYY-MM-DD` | `BLOCK_CELL_TYPE_MISMATCH` |
| `source` | required (ADR-067) | `BLOCK_SOURCE_MISSING` |

### ChartBlock
- **HLD trace:** `09-decisions.md:1472` (ADR-067 Chart: bar vertical or horizontal, grouped or stacked, line, area, scatter, pie with at most 6 slices; kind, data rows, column per axis, series, axis titles; **Show as table**); `10-risks.md:45` (A-34: 8 series, 5,000 points); `11-ux-guidelines.md:719` (colours from the chart palette only).
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ChartBlock {
    pub title: String,
    #[serde(flatten)]
    pub chart: ChartKind,
    /// The data rows, as a table; **Show as table** draws exactly these.
    pub columns: Vec<TableColumn>,
    pub rows: Vec<Vec<Cell>>,
    /// Column name on the x axis (category or value), or the slice label column for pie.
    pub x: String,
    /// One or more value columns; each is a series.
    pub series: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y_title: Option<String>,
    pub source: ReportPartSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "chart", rename_all = "snake_case")]
pub enum ChartKind {
    Bar { orientation: BarOrientation, arrangement: BarArrangement },
    Line,
    Area,
    Scatter,
    Pie,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BarOrientation { Vertical, Horizontal }

// `BarArrangement { Grouped, Stacked }` is defined once, in part 1 (`ordin_types::domain::runs`,
// entity `Report`), and re-exported here (amended January 2026, decision 80: C-23).
pub use ordin_types::domain::runs::BarArrangement;
```
- **JSON example:** `{"kind":"chart","title":"Spend by supplier","chart":"bar","orientation":"vertical","arrangement":"grouped","columns":[{"name":"Supplier","type":"text"},{"name":"Q2","type":"money","currency":"GBP"},{"name":"Q3","type":"money","currency":"GBP"}],"rows":[["Kestrel",382000,412000],["Acme",401000,398000]],"x":"Supplier","series":["Q2","Q3"],"y_title":"Spend","source":{"kind":"worked_out","by":"Supplier analyst","from":[{"kind":"place_file","place_id":"0192a0f2-2222-7333-8444-555566667777","relative_path":"Q3/rates.xlsx"}]}}`
- **Validation:**

| Field | Rule | Error code |
|---|---|---|
| `series` | 1–8 (A-34); each names a numeric column | `BLOCK_CHART_SERIES_INVALID` |
| points | `rows × series` ≤ 5,000 (A-34) | `BLOCK_CHART_TOO_MANY_POINTS` |
| `Pie` | exactly 1 series and at most 6 rows (ADR-067) | `BLOCK_CHART_PIE_INVALID` |
| `x` | names a column | `BLOCK_CHART_AXIS_INVALID` |
| `Scatter` | `x` is a numeric column | `BLOCK_CHART_AXIS_INVALID` |
| titles | 1–120 characters (assumed) | `BLOCK_CHART_INVALID` |
| `source` | required | `BLOCK_SOURCE_MISSING` |

### KeyFiguresBlock
- **HLD trace:** `09-decisions.md:1473` (one to eight figures: label, value, unit, optional change against a named earlier value); `11-ux-guidelines.md:720`.
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KeyFiguresBlock {
    pub figures: Vec<KeyFigure>,
    pub source: ReportPartSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KeyFigure {
    pub label: String,
    pub value: f64,
    /// A unit word ("tickets") or an ISO 4217 currency code; absent for a bare number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change: Option<FigureChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct FigureChange {
    /// Name of the earlier value ("Q2"); the change is shown only when this is present.
    pub against: String,
    pub earlier_value: f64,
}
```
- **JSON example:** `{"kind":"key_figures","figures":[{"label":"Spend","value":412000,"unit":"GBP","change":{"against":"Q2","earlier_value":381500}}],"source":{"kind":"company_system","system":"Finance ledger","read_at":"2026-01-10T15:20:00.000Z"}}`
- **Validation:** `figures` 1–8 (ADR-067, A-34) → `BLOCK_FIGURES_COUNT`; `label` 1–60 characters, `unit` 1–24 characters, `against` 1–60 characters (assumed) → `BLOCK_FIGURES_INVALID`; values finite; `source` required.

### ReportBlock
- **HLD trace:** `09-decisions.md:1474` (title, "as of", one to twelve parts each a Table, Chart or Key figures with a heading); `04-domain.md:239-242` (`Report`); `10-risks.md:45` (A-34: 12 parts, 2 MB).
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ReportBlock {
    pub title: String,
    pub as_of: Timestamp,
    pub parts: Vec<ReportPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ReportPart {
    pub heading: String,
    pub content: ReportPartContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReportPartContent {
    Table(TableBlock),
    Chart(ChartBlock),
    KeyFigures(KeyFiguresBlock),
}

/// Arguments of the MCP Front's `report` tool: a Report, or one Table, Chart or Key figures.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReportToolArgs {
    Report(ReportBlock),
    Table(TableBlock),
    Chart(ChartBlock),
    KeyFigures(KeyFiguresBlock),
}
```
- **JSON example:** `{"kind":"report","title":"Q3 supplier rates","as_of":"2026-01-10T15:20:00.000Z","parts":[{"heading":"Rates","content":{"kind":"table", "title":"Q3 rates by supplier", "columns":[…], "rows":[…], "source":{…}}}]}`
- **Validation:** `parts` 1–12 (A-34) → `BLOCK_REPORT_PARTS_COUNT`; total serialised size ≤ 2 MB (A-34) → `BLOCK_TOO_LARGE`; `title` and `heading` 1–120 characters (assumed); a part that fails its own check is left out and the report says so in one line (`11-ux-guidelines.md:727`) → per-part `BlockRejection`, the rest is shown; `as_of` not more than 5 minutes in the future (assumed) → `BLOCK_AS_OF_INVALID`.

### ReportPartSource
- **Renamed** from `PartSource` (amended January 2026, decision 80: C-5), because `PartSource` is the Agent Model's hub trait (ADR-100, 2B-1 rule 4), which keeps its name.
- **HLD trace:** `09-decisions.md:1475` ("one or more documents with the version read, a company system reached through the sandbox's broker with the time of the read, or an earlier result … When an agent computed the numbers, the source line says so"); `04-domain.md:240`.
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReportPartSource {
    Documents { documents: Vec<SourceDocument> },
    CompanySystem { system: String, read_at: Timestamp },
    EarlierResult { run_id: RunId },
    WorkedOut { by: String, from: Vec<ContentRef> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SourceDocument { pub item: ContentRef, pub version: String }
```
- **Validation:** `documents` 1–20 (assumed); `version` 1–64 characters; `system` 1–80 characters; `by` 1–80 characters and the name of the agent of the producing run; every reference resolvable → `BLOCK_SOURCE_INVALID`. Absent source → `BLOCK_SOURCE_MISSING`.

### EmailListBlock and EmailBlock
- **HLD trace:** `09-decisions.md:1599` (ADR-070: Email list with sender, subject, date, marked outside sender; Email: one message or one draft, from an agent always a draft); `11-ux-guidelines.md:578-579`; `02-architecture.md:335`; `04-domain.md:91-95` (`EmailDraft`).
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct EmailListBlock {
    pub workspace_id: WorkspaceId,
    /// The email link the list comes from (entity `EmailLink`).
    pub email_link_id: EmailLinkId,
    /// Mail-system message identifiers; the core fills sender, subject, date, attachments.
    pub messages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "of", rename_all = "snake_case", deny_unknown_fields)]
pub enum EmailBlock {
    Message { workspace_id: WorkspaceId, message_id: String },
    Draft { draft_id: EmailDraftId },
}
```
- **Note:** both kinds reference data the Mail Service holds; the block never carries message text, so a draft's text on screen is always the `EmailDraft` itself.
- **JSON example:** `{"kind":"email","of":"draft","draft_id":"0192a1c5-7777-7888-8999-aaaabbbbcccc"}`
- **Validation:** `messages` 1–200 (assumed); each `message_id` 1–998 characters (RFC 5322 line limit) and matching the workspace's email links → `BLOCK_EMAIL_NOT_LINKED`; `draft_id` exists and belongs to the workspace → `BLOCK_REF_UNRESOLVED`.

### TelegramListBlock, SlackListBlock, ResultBlock, AgentProposalBlock, AgentLibraryBlock, CrewPlanBlock, MemoryBlock, KnowledgeBlock
- **HLD trace:** `11-ux-guidelines.md:580-587` (§5.15 rows); `09-decisions.md:537` (result, agent proposal, crew plan, memory facts).
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct TelegramListBlock { pub telegram_link_id: TelegramLinkId }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SlackListBlock { pub slack_link_id: SlackLinkId }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "of", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResultBlock {
    /// What a task produced, with where it is.
    Produced { run_id: RunId, summary: String, items: Vec<ContentRef> },
    /// An offer that waits for an answer; the offers are a closed list.
    Offer { offer: ResultOffer, run_id: Option<RunId> },
    /// A list to tick (Ready to publish, §5.14).
    Checklist { title: String, items: Vec<ContentRef> },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultOffer { SaveAsWorkflow, UpdateTheWorkflow }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AgentProposalBlock { pub agent_repository_id: AgentRepositoryId, pub agent: String, pub reason: String }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AgentLibraryBlock { #[serde(skip_serializing_if = "Option::is_none")] pub search: Option<String> }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CrewPlanBlock {
    pub crew_charter_id: CrewCharterId,
    /// "Estimated use" of the crew before Start (ADR-062, UX-A32), from the Usage Planner's `estimate`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimate: Option<UsageEstimate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MemoryBlock { pub workspace_id: WorkspaceId, pub facts: Vec<MemoryFactId> }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeBlock { pub workspace_id: WorkspaceId, #[serde(skip_serializing_if = "Option::is_none")] pub page: Option<KnowledgePageId> }
```

`slack_link_id` names a linked Slack channel of the block's workspace → else `BLOCK_REF_UNRESOLVED` (the Block Renderer reaches the Slack Service's check through the program's `RefResolver`, `EntityRef::SlackLink`; amended January 2026, ADR-102).
- **Validation:** `summary` 1–2,000 characters (assumed); `reason` 1–300 characters (assumed); `agent` 1–256 characters (a `urn:ordin:` identifier, `00-overview.md:233`); `facts` 1–100 (assumed); `ResultOffer` is a closed list — Ordin cannot invent an offer → `BLOCK_OFFER_UNKNOWN`; any reference unresolved → `BLOCK_REF_UNRESOLVED`. A `CrewPlanBlock` is drawn from the charter the Ordin Host wrote, never from Ordin's words (UX-A35); its `estimate` is the Usage Planner's own result, filled by the Ordin Host and copied unchanged.

### BlockRejection
- **Crosses:** core → Ordin's or the agent's session (MCP tool result of `show` / `report` / `plan`); journalled.
- **Producer / Consumers:** Block Renderer's check via MCP Front; Ordin (explains to the person, `11-ux-guidelines.md:727`), agents.
- **HLD trace:** `09-decisions.md:539` ("A block that fails validation is dropped and Ordin is told why"); `02-architecture.md:331`; `02-architecture.md:514` (MCP Front: "answers a call with wrong arguments with a short error naming the correct form").
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BlockRejection {
    pub code: String,
    /// JSON Pointer to the failing field, e.g. `/parts/2/content/source`.
    pub at: String,
    /// One sentence naming the correct form, for the model.
    pub reason: String,
}
```
- **JSON example:** `{"code":"BLOCK_SOURCE_MISSING","at":"/parts/2/content/source","reason":"Every part needs a source: documents with versions, a company system with the time read, an earlier result, or who worked it out and from what."}`
- **Validation:** `reason` 1–300 characters (assumed); at most 20 rejections per call returned (assumed).

---

## 8. Workflow file, format 3 (`<name>.workflow.yaml`)

### WorkflowFile
- **Crosses:** file on disk (Export…, Import workflow…); Git repositories through `gh` and `glab` (Share…, Workflows in linked repositories…).
- **Producer / Consumers:** Ordin Host writes (export, share via Push Service) and reads (import, repository listing); the Catalogue Service matches agents.
- **HLD trace:** `08-interfaces.md:18-19`; `09-decisions.md:1385-1392` (ADR-064 export and import checks), `:1414-1436` (ADR-065 blocks, maxima, checks), `:1445-1457` (ADR-066); `10-risks.md:112` (OQ-47 settled: format 3); `10-risks.md:43` (A-32); `04-domain.md:222-237` (`Workflow`, `WorkflowStep`); `07-cross-cutting.md:44-49` (limit maxima).
- **Rust definition:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowFile {
    /// Format version; this release writes 3 and reads 1, 2 and 3.
    pub format: u8,
    pub name: String,
    pub purpose: String,
    pub inputs: Vec<WorkflowFileInput>,
    pub agents: Vec<WorkflowFileAgent>,
    pub steps: Vec<WorkflowFileStep>,
    pub limits: WorkflowFileLimits,
    /// One line of text ("Saved from the run of 16 Oct").
    pub origin: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowFileInput {
    pub label: String,
    pub kind: WorkflowInputKind,
    pub required: bool,
}

/// The only definition (amended January 2026, decision 80: C-1); the domain part's `WorkflowInput.kind` re-exports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowInputKind {
    Document,
    Text,
    /// A list a `for_each` block can go through (ADR-065); see HLD inconsistency I-1.
    List,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowFileAgent {
    /// Short local name the steps use, unique in the file ("analyst").
    pub key: String,
    /// Format 3 (OQ-47): the agent repository's profile key, or its address for one
    /// the person added.
    pub repository: AgentRepositoryRef,
    /// The agent's identifier, a `urn:ordin:` WorkspaceProfile identifier.
    pub id: String,
    pub version: String,
    /// The agent's display name.
    pub name: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentRepositoryRef {
    ProfileKey(String),
    Address(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkflowFileLimits {
    pub max_rounds: u16,
    pub wall_clock_minutes: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_budget: Option<u32>,
    pub agents_at_once: u8,
    pub failure_repeat_limit: u8,
    /// Agent keys allowed to take externally visible actions.
    pub external_action_agents: Vec<String>,
}
```
- **YAML example (format 3):**
```yaml
format: 3
name: Supplier rates refresh
purpose: Compare this quarter's supplier quotes and update the rates summary.
inputs:
  - {label: Quotes, kind: list, required: true}
  - {label: Rates summary, kind: document, required: true}
agents:
  - key: analyst
    repository: {profile_key: acme-agents}
    id: "urn:ordin:acme:workspaces:procurement:supplier-analyst"
    version: "^2.1"
    name: Supplier analyst
    role: Reads quotes and compares rates.
  - key: writer
    repository: {address: "https://github.com/acme/team-agents"}
    id: "urn:ordin:acme:workspaces:docs:summary-writer"
    version: "^1.0"
    name: Summary writer
    role: Updates the rates summary.
steps:
  - kind: for_each
    position: "1"
    list: {input: Quotes}
    max_items: 20
    steps:
      - {kind: agent, position: "1.1", agent: analyst, line: Read the quote and extract the rate, uses: [{item: true}], asks_first: false, ordin_checks: false, if_it_fails: skip_it}
  - kind: if
    position: "2"
    decided_by: {by: ordin, words: Any rate rose by more than 5 per cent}
    paths:
      - name: "Yes"
        steps:
          - {kind: agent, position: "2.1", agent: writer, line: Update the summary and flag the rises, uses: [{step: "1"}, {input: Rates summary}], asks_first: true, ordin_checks: true, if_it_fails: ordin_takes_over}
      - name: "No"
        steps: []
limits:
  max_rounds: 10
  wall_clock_minutes: 60
  agents_at_once: 4
  failure_repeat_limit: 2
  external_action_agents: [writer]
origin: Saved from the run of 16 Oct
```
- **Validation (the core's fixed-code check, before anything is shown):**

| Field | Rule | Error code |
|---|---|---|
| file | at most 256 KB (`08-interfaces.md:18`); UTF-8 YAML 1.2, no anchors, aliases or custom tags (assumed, to keep the reader free of expansion attacks) | `WFF_FILE_TOO_LARGE`, `WFF_FILE_MALFORMED` |
| `format` | 1, 2 or 3; greater → refused, "This workflow was made by a newer ordin." (`09-decisions.md:1388`) | `WFF_FORMAT_NEWER` |
| `name` | 1–80 characters (`04-domain.md:229`); a name used in the workspace gets " (2)" | `WFF_NAME_INVALID` |
| `purpose` | 1–200 characters (assumed: "a purpose in one line") | `WFF_PURPOSE_INVALID` |
| `inputs` | 0–20 (assumed); `label` 1–60 characters (assumed), unique | `WFF_INPUT_INVALID` |
| `agents` | 1–8 (A-32 original figure and `07-cross-cutting.md:42`, crews of up to 8 members) | `WFF_AGENTS_INVALID` |
| `agents[].key` | `^[a-z][a-z0-9_-]{0,31}$`, unique | `WFF_AGENT_KEY_INVALID` |
| `agents[].id` | 1–256 characters, starts `urn:ordin:` | `WFF_AGENT_ID_INVALID` |
| `agents[].version` | a SemVer requirement in Cargo syntax (assumed; see I-3) | `WFF_AGENT_VERSION_INVALID` |
| `agents[].repository` | a profile key known to the organisation profile, or an `https://` address; one the person has not added is listed, not an error (`99-changelog.md:219`) | `WFF_REPOSITORY_INVALID` |
| `agents[].role` | 1–120 characters (assumed: "role in one line") | `WFF_AGENT_ROLE_INVALID` |
| steps and blocks | at most 50 in all, nested at most 4 deep (A-32 revised; ADR-065) | `WFF_TOO_MANY_STEPS`, `WFF_TOO_DEEP` |
| `limits.max_rounds` | 1–30; above the company's maximum it is lowered and said so | `WFF_LIMIT_LOWERED` (notice, not refusal) |
| `limits.wall_clock_minutes` | 1–480 (`07-cross-cutting.md:46`); lowered as above | `WFF_LIMIT_LOWERED` |
| `limits.agents_at_once` | 1–8 (`07-cross-cutting.md:48`); lowered as above | `WFF_LIMIT_LOWERED` |
| `limits.failure_repeat_limit` | 1–5 (assumed; the default escalation is after 2, `07-cross-cutting.md:47`) | `WFF_LIMIT_INVALID` |
| `limits.external_action_agents` | each a declared agent key | `WFF_AGENT_KEY_UNKNOWN` |
| `origin` | 0–200 characters (assumed) | `WFF_ORIGIN_INVALID` |
| content | no field for documents, places, links, folder grants, memory, knowledge pages, run history, inputs used or a person's name (`08-interfaces.md:18`); `deny_unknown_fields` enforces it | `WFF_FIELD_UNKNOWN` |

- **Versioning:** the `format` field. Format 1 (revision 0.11.0): linear agent steps, agents by marketplace package name and version range. Format 2 (revision 0.12.0): adds control blocks and `if_it_fails`. Format 3 (revision 0.39.1): agents by repository, identifier, version range, name and role. A format 1 or 2 file is read by matching each agent by name against the workspace's agents (`08-interfaces.md:18`); writing is always format 3. A file is never signed in this release (OQ-8, settled by ADR-066 as repository review).

### WorkflowFileStep
- **HLD trace:** `08-interfaces.md:18` (format 2 block kinds `if`, `each`, `until`, `call`; per step: position, group, agent, line, uses, asks first, Ordin checks the result); `09-decisions.md:1417-1431` (ADR-065); `04-domain.md:235-237` (`WorkflowStep` rules).
- **Rust definition:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkflowFileStep {
    Agent {
        /// Display number: "3", "2.1", "3a"; steps sharing a number with letters run at the same time.
        position: String,
        agent: String,
        line: String,
        uses: Vec<StepUse>,
        asks_first: bool,
        ordin_checks: bool,
        #[serde(default)]
        if_it_fails: IfItFails,
    },
    If {
        position: String,
        decided_by: Decider,
        paths: Vec<NamedPath>,
    },
    #[serde(rename = "for_each")]
    Each {
        position: String,
        list: ListSource,
        max_items: u8,
        steps: Vec<WorkflowFileStep>,
    },
    #[serde(rename = "repeat_until")]
    Until {
        position: String,
        decided_by: Decider,
        max_times: u8,
        steps: Vec<WorkflowFileStep>,
    },
    #[serde(rename = "run_workflow")]
    Call {
        position: String,
        /// The name of a workflow saved in the same workspace.
        workflow: String,
    },
}

/// `wire::workflow_file::NamedPath` — distinct from the domain part's `domain::runs::NamedPath`
/// (the saved workflow's); the Ordin Host converts between them (amended January 2026, decision 80: C-1, namespaced).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct NamedPath {
    pub name: String,
    pub steps: Vec<WorkflowFileStep>,
}

/// `wire::workflow_file::StepUse` — distinct from `domain::runs::StepUse` (amended January 2026, decision 80: C-1, namespaced).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum StepUse {
    Input(String),
    Step(String),
    /// Inside a for_each: the current item.
    Item(bool),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ListSource {
    Input(String),
    Step(String),
}

/// The only definition (amended January 2026, decision 80: C-1); the domain part's `AgentStep.if_it_fails` re-exports it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IfItFails {
    #[default]
    OrdinTakesOver,
    AskMe,
    SkipIt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "by", rename_all = "snake_case", deny_unknown_fields)]
pub enum Decider {
    /// Fixed code, for what the application can know for certain (ADR-065).
    Application { check: ApplicationCheck, step: String },
    Ordin { words: String },
    /// The person's answer ("Ask me" is an `if` decided by the person; path names are the choices).
    Person { words: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationCheck {
    ListHasItems,
    StepFailed,
    CheckFoundShortfall,
    DocumentChanged,
}
```
The YAML tag values are `agent`, `if`, `for_each`, `repeat_until`, `run_workflow`. 08 names the format-2 kinds `if`, `each`, `until`, `call`; a reader accepts both spellings for formats 2 and 3 and the writer emits the spelled-out form (see I-6 and *Names fixed by this spec*).
- **Validation:**

| Field | Rule | Error code |
|---|---|---|
| `position` | `^[0-9]{1,2}[a-z]?(\.[0-9]{1,2}[a-z]?){0,3}$`, unique, in outline order (ADR-065 numbering) | `WFF_POSITION_INVALID` |
| `agent` | a key in `agents` | `WFF_AGENT_KEY_UNKNOWN` |
| `line` | 1–200 characters (A-32 original) | `WFF_LINE_INVALID` |
| `uses` | each input exists; each step comes before this one and its result exists on this path (ADR-065) | `WFF_USE_NOT_ON_PATH` |
| steps at the same time | a group (same number, letters) holds at most `agents_at_once` | `WFF_GROUP_TOO_LARGE` |
| `if.paths` | 2–4, names 1–40 characters (assumed) and unique; at least one path holds a step | `WFF_IF_PATHS_INVALID` |
| `decided_by.words` | 1–300 characters (assumed) | `WFF_CONDITION_INVALID` |
| `Application.step` | an earlier step's position | `WFF_CONDITION_INVALID` |
| `for_each.max_items` | 1–50 (ADR-065) | `WFF_LOOP_MAX_INVALID` |
| `for_each.list` | a `list` input or an earlier step | `WFF_LOOP_LIST_INVALID` |
| `repeat_until.max_times` | 1–10, required (ADR-065) | `WFF_LOOP_MAX_INVALID` |
| `run_workflow.workflow` | a workflow of the workspace, at most 3 levels deep, no cycle ("This would run … again inside itself") | `WFF_CALL_CYCLE`, `WFF_CALL_TOO_DEEP`, `WFF_CALL_UNKNOWN` |
| sending steps | a step whose agent sends email is always `asks_first: true` (`09-decisions.md:1592`) | `WFF_SEND_WITHOUT_ASK` |

A file whose blocks fail the check opens with the problems shown and cannot be saved until fixed (`09-decisions.md:1433`); a malformed file is refused with "This file isn't a workflow ordin can read." (`03-sequences.md:719`).

---

## 9. Types consumed from `memory/` and `sandbox/` (referenced, not redefined)

| Wrapper (02) | Crate type or function consumed | Location | Note |
|---|---|---|---|
| Memory Service | `memory::service::Service` (in-process service over the store) | `memory/src/service/mod.rs:45` | Entry points `capabilities` (`:163`), `recall` (`:203`, takes the JSON body as `&[u8]`), `remember` (`:243`), `get_fact` (`:400`), `retire` (`:415`), `delete` (`:464`). |
| Memory Service | `memory::service::CallContext` (`bearer`, `correlation_id`, `idempotency_key`) | `memory/src/service/mod.rs:58-66` | Every call carries a bearer token; see I-8. |
| Memory Service | `memory::service::CallResult<T>`, `CallError` | `memory/src/service/mod.rs:89`, `:107` | |
| Memory Service | `memory::types::api::RecallRequest` (`query` 1–4,096 chars, `filters`, `result_cap` 1–50 default 10, `cursor`, `include_provenance`) | `memory/src/types/api.rs:13-29` | |
| Memory Service | `memory::types::api::RecallFilters`, `RecallResponse`, `RankedFact`, `SourceProvenance` | `memory/src/types/api.rs:31`, `:42`, `:47`, `:60` | |
| Memory Service | `memory::types::api::RememberRequest` (`content`, `source`, `memory_class`, `fact_class`), `SourceInput` | `memory/src/types/api.rs:69`, `:85` | |
| Memory Service (*amended January 2026, ADR-110; memory change M-8*) | `Fact.collection` (`""` default, `^[a-z0-9:_-]{0,64}$`), `RememberRequest.collection`, `RecallFilters.collections` (1–32), `RecallRequest.searches` with `SubQuery { type: lex \| vec, query }` (1–4; exactly one of `query` and `searches`), `Candidate.fused_score` and `fused_norm`, `StoragePort::erase_collection`, `Service::check_content` → `ContentCheck { personal_data: None \| Review \| Redact, instruction_like }` | specified by the memory spec set (memory-store, retrieval-engine, service-layer amendments January 2026, memory ADR-020); file positions follow the build | Ordin's read folders are memory collections `"tk:<TeamFolderId>"`; own and local facts are the default collection `""`. |
| Memory Service (*amended January 2026, ADR-111; memory change M-9*) | `RememberRequest.entities` (`EntityAssertion { name, aliases }`, 0–16), `.links` (`LinkAssertion { from, kind, to, ended, confidence }`, 0–16), `.derived_from` (0 or 2–64, consolidated only); `Relationship.replaces`; `RankedFact.fading`; `RecallResponse.links` (`RecallLink`); trait `GraphStore` with `LinkRole`, `EntitySummary`, `EntityPage`, `EntityDetail`, `EntityCount`, `EntityErasureProof`; `Service::fact_history`, `fact_origins`, `list_entities`, `entity_detail`, `keep_fact`, `count_entity`, `forget_entity`, `merge_entities`, `correct_link`, `forget_link`; `memory::maintenance::is_fading`, `fading_since`; `memory::text::normalise_name` | specified by the memory spec set (memory-store, write-pipeline, retrieval-engine, maintenance-worker, service-layer amendments January 2026, memory ADR-021); file positions follow the build | A thing is an entity (`MemoryThingId`), a link a relationship (`MemoryLinkId`); the person-only operations are honoured only for the in-process client. |
| Memory Service | `memory::types::api::WriteAck`, `JobAckStatus`, `RetireAck`, `DeletionProof` | `memory/src/types/api.rs:94`, `:102`, `:110`, `:125` | |
| Memory Service | `memory::types::scope::ScopeRef` (`tenant`, `team`, `user`) | `memory/src/types/scope.rs:11` | Maps to 02's "one tenant per installation, one user, one team scope per workspace" (`02-architecture.md:350`). |
| MCP Front (memory tools) | memory MCP tool names `recall`, `remember`, `get`, `retire`, `delete`, `capabilities` | `memory/src/mcp/mod.rs:191-271` | |
| Sandbox | `sandboxd::controller::SandboxController::run(req, session) -> Result<RunOutcome, ControllerError>` | `sandbox/sandbox-host/src/controller.rs:152-156` | |
| Sandbox | `sandboxd::types::RunRequest` (`code`, `requested_capabilities`, `timeout_ms`, `dry_run`, `workspace_id`, `run_id`, `files`; **camelCase** on the wire) | `sandbox/sandbox-host/src/types.rs:166-183` | Its own serde convention, kept as is. |
| Sandbox | `sandboxd::types::RunResult` (`stdout`, `stderr`, `exit_code`, `api_calls`, `truncated`, `changes`), `DryRunResult`, `CallSummary`, `SessionHandle` | `sandbox/sandbox-host/src/types.rs:186-225` | |
| Sandbox | `sandboxd::controller::RunOutcome`, `ControllerError` (codes `CAP_UNKNOWN`, `INTERACTION_DENIED`, `RATE_LIMITED`, work-folder codes) | `sandbox/sandbox-host/src/controller.rs:85-113` | |
| Sandbox | `sandboxd::controller::Interactor` (sign-in, consent, step-up), `CapabilityMinter`, `IdTokenSink` traits | `sandbox/sandbox-host/src/controller.rs:32`, `:53`, `:79` | ordin implements `Interactor`; see I-9. |
| Sandbox | `sandboxd::types::InteractionRequired` (`SignIn`, `Consent`, `StepUp`) | `sandbox/sandbox-host/src/types.rs:134-150` | |
| Sandbox | `sandboxd::workdir::{WorkFile, WorkLimits, FileChange}` | `sandbox/sandbox-host/src/workdir.rs:47`, `:28`, `:64` | |
| MCP Front (sandbox tool) | tool name `python_sandbox` | `sandbox/sandbox-host/src/mcp.rs:125` | |

---

## 10. Team knowledge additions (ADR-110)

*Amended January 2026 (ADR-110).* Owner of every type here: **Team Knowledge Service** (TeamKnowledgeService.md amendment January 2026), except where named. Used by: Service API (`team_knowledge.*`), Screens module (team knowledge panel, Adopt dialog).

### TeamFolderStatus and the status additions

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct TeamFolderStatus {
    pub folder_id: TeamFolderId,
    pub name: String,                                  // shown name (2C TeamKnowledgeBinding)
    #[serde(skip_serializing_if = "Option::is_none")] pub description: Option<String>,
    pub set_by_company: bool,
    pub may_stop: bool,
    pub access: TeamAccess,
    pub available: Option<TeamAvailable>,
    pub held: TeamHeld,
    pub problems: Vec<TeamProblem>,
    pub last_checked_at: Option<Timestamp>,
    pub folder_large: bool,
    #[serde(skip_serializing_if = "Option::is_none")] pub review: Option<ReviewState>,
    /// TEAM_COMPANY_FOLDER_NO_ACCESS | TEAM_REVIEW_NOT_REQUIRED | TEAM_REVIEW_CANNOT_TELL; None = read normally.
    #[serde(skip_serializing_if = "Option::is_none")] pub blocked: Option<&'static str>,
    pub last_get: Option<GetReport>,
}
#[derive(Debug, Clone, Copy, Default, Serialize)] #[serde(rename_all = "snake_case")]
pub struct TeamHeld { pub facts: u32, pub pages: u32 }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)] #[serde(rename_all = "snake_case")]
pub enum ReviewState { Required, NotRequired, CannotTell }
#[derive(Debug, Clone, Serialize)] #[serde(rename_all = "snake_case")]
pub struct RecallCheckReport { pub ran_at: Timestamp, pub folders: Vec<RecallCheckFolder> }
#[derive(Debug, Clone, Serialize)] #[serde(rename_all = "snake_case")]
pub struct RecallCheckFolder { pub folder_id: Option<TeamFolderId> /* None = own */, pub name: String, pub checks: u32,
                               pub precision_at_5_permille: u16, pub recall_at_5_permille: u16, pub mrr_permille: u16 }
#[derive(Debug, Clone, Serialize, Deserialize)] #[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct FolderDescription { pub path: String /* relative to the own folder; "" = the folder */, pub text: String /* 1–500 */ }
```

`TeamKnowledgeStatus` keeps every field and adds `own_folder_id: Option<TeamFolderId>`, `folders: Vec<TeamFolderStatus>` (read folders, company-named first, then by name), `workspace_facts: u32`, `workspace_large: bool`, `last_recall_check: Option<RecallCheckReport>` and `descriptions: Vec<FolderDescription>`; `TeamBusy` gains `CheckingRecall`; `share_unavailable` may be `TEAM_NO_OWN_FOLDER`.

### Adopt command shapes

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AdoptItem { Fact { id: MemoryFactId }, Page { page_id: KnowledgePageId } }
#[derive(Debug, Clone, Default, Serialize, Deserialize)] #[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AdoptEdit { pub statement: Option<String>, pub title: Option<String>, pub body: Option<String> }
#[derive(Debug, Clone, Serialize)] #[serde(rename_all = "snake_case")]
pub struct AdoptResult { pub kind: String /* "fact" | "page" */, pub id: String, pub edited: bool, pub version: u32 }
```

`team_knowledge.adopt`, `adopt_again` and `keep_as_is` are interface commands only: no MCP tool, run, workflow step or Ordin reaches them.

### Recall additions (owner: Memory Service)

`RecallInput` of the `recall` tools on servers `memory` and `ordin` gains `searches: [{ "type": "lex" | "vec", "query": string 1–500 }]` (1–4; exactly one of `query` and `searches`) and `folders: [string]` (1–21 folder names as recall returns them, or `"own"`). `FactView` gains `folder`, `folder_description`, `set_by_company`, `adopted_from`, `adoption_mark` (domain part, *MemoryFact*).

### Team folder files (format 1 kept; owner: Team Knowledge Service)

| File | Shape | Bounds |
|---|---|---|
| `<folder>/ordin-team-descriptions.json` | `{"format":1,"paths":{"<relative path, \"\" = this folder>":"<description>"}}` | ≤ 64 KiB, ≤ 200 keys, descriptions 1–500 characters; read at the folder and each ancestor to the root, at most 8 files |
| `<folder>/ordin-team-checks.json` | `{"format":1,"checks":[{"query":"…","type":"lex"\|"vec"\|null,"expect":["<MemoryFactId>"]}]}` | ≤ 256 KiB, ≤ 100 checks, 1–10 expected ids each; never written by ordin |
| `<folder>/adoptions/<adopted item id>.json` | `TeamAdoption` (domain part) | ≤ 65,536 bytes |

An installation from before ADR-110 reads none of the three: they are not among the names format 1 reads (TeamKnowledgeService.md:182), so they cause no problem or warning there (A-59).

---

## 11. Agentic memory additions (ADR-111)

*Amended January 2026 (ADR-111).* Owner of every type here: **Memory Service** (MemoryService.md amendment January 2026). Used by: Service API (`memory.*`), Screens module (*What Ordin remembers*, **What Ordin used**, run progress), Ordin Host (`MessageBlock.memory_used`), Remote API and Channel Gateway (agent profile).

- **Identifiers:** `MemoryRecallId` (UUIDv7, `uuid_id!`); `MemoryThingId` (`^entity:<uuid>$`) and `MemoryLinkId` (`^relationship:<uuid>$`), any UUID version, refused with `ID_MEMORY_REF_INVALID` (domain part conventions block).
- **Tool inputs:** `remember` on servers `memory` and `ordin` gains `entities: [{ "name": string 1–200, "other_names": [string 1–200] (0–8) }]` (0–16) and `links: [{ "from", "kind": string 1–64, "to", "ended": boolean, "confidence_permille": 0–1000 }]` (0–16); the `ordin` server's `remember` also takes `made_from: [MemoryFactId]` (2–64). `RecallResult` gains `recall_id: MemoryRecallId` and `links: [LinkView]` (0–20).
- **Views:** `FactView` gains `kind` (`happened` \| `known` \| `summary`), `true_from`, `true_until`, `replaces`, `fading`, `not_used_since`, `things: [ThingRef]`, `made_from_count`; new `ThingRef`, `LinkView` (with `by`: `agent` \| `ordin` \| `person` \| `page` \| `team`), `ThingView` (with `explained_in: PageLabel`), `ThingPage`, `ThingDetail`, `FactHistoryView`, `OriginsView`, `MemoryUsedView`, `RememberedInRun`, `ThingCount`, `ThingForgetReceipt`, `LinkForgetReceipt`, `ForgetCount`; `ForgetReceipt` gains `summaries_removed`.
- **Example** (`LinkView`): `{"id":"relationship:5c1f0e2a-8b3d-5e4f-9a6b-7c8d9e0f1a2b","from":{"id":"entity:0192a2d0-1c3e-7a10-8b2f-5d6e7f809a1b","name":"Borealis"},"kind":"supplies","to":{"id":"entity:0192a2d0-1c3e-7a10-8b2f-5d6e7f809a1c","name":"packaging"},"since":"2026-01-10T09:12:44.318Z","by":{"kind":"agent","agent_instance_id":"0192a0f1-3333-7444-8555-666677778888"}}`.

## 12. Knowledge agents and the Open Knowledge Format (ADR-112, ADR-113)

*Amended January 2026 (ADR-112, ADR-113).* Module `ordin_types::wire::knowledge_query`, defined once here so that the phase-2 Knowledge Query Client can name every type; owner of the shapes: **Knowledge Query Client** (KnowledgeQueryClient.md *Shared Context*, *Wire types*), with `KnowledgeContents` and its parts from KnowledgeReader.md and `PageTier`/`PageStatus` defined in `ordin-types` (SharedTypes.md amendment January 2026). Used by: Knowledge Query Client, Knowledge Query Server, Knowledge Reader, Memory Service, Knowledge Service (`okf` module: `trust_tier` returns `PageTier`, `effective_status` returns `PageStatus`), Service API, Screens module.

- **Identifiers:** `KnowledgeConnectionId` (a workspace's connection to a front door) and `KnowledgeAnswerId` (minted by a knowledge agent that keeps a sources record), UUIDv7, `uuid_id!` (domain part, identifier block).
- **Trust tier:** `PageTier::trust_signal()` is 1.0 (`human_reviewed`), 0.9 (`machine_confirmed`) and 0.8 (`unverified`); a document-backed fact keeps trust 0.5 (ADR-111 item 8). Tier and the page-over-document order break ties only between otherwise equal fused results; a `deprecated` fact is returned only when no other fact matches.
- **Routes carrying these bodies:** Caller API routes 34–40 (`POST /v1/knowledge/query` → `KnowledgeQueryRequest` / `KnowledgeResult`, then Server-Sent Events `more_found` and `done`; `GET /v1/knowledge/contents` → `KnowledgeContents`; `GET /v1/knowledge/contents/index.md` → an OKF `index.md`; `GET /v1/knowledge/about` → `KnowledgeAbout`; `GET /v1/knowledge/changes` → events `RepositoryMoved`; `POST /mcp/knowledge`, tools `knowledge_search` and `knowledge_contents`; `GET /operator/knowledge`), CallerApi.md amendment January 2026.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum PageTier { HumanReviewed, MachineConfirmed, #[default] Unverified }     // OKF 0.2 derived trust (ADR-112)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum PageStatus { #[default] Stable, Draft, Deprecated }

#[derive(Debug, Clone, Serialize, Deserialize)] #[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeQueryRequest {
    pub request_id: uuid::Uuid,                       // UUIDv7, minted by the first asker
    /// 1–4,096 characters. Exactly one of `query` and `searches`.
    #[serde(default)] pub query: Option<String>,
    /// 1–4 typed sub-queries, in the Memory Service's `RecallSearch` form; the first counts double.
    #[serde(default)] pub searches: Option<Vec<KnowledgeSearch>>,
    #[serde(default = "twenty")] pub limit: u8,       // 1–50
    /// Set only by a front door asking a proxy: the audience group the front door resolved for its asker.
    #[serde(default)] pub access_group: Option<String>,
    pub hop_budget: u8,                               // 0–2; a desktop sends 1
    #[serde(default)] pub visited: Vec<String>,       // agent names (1–80 each), ≤ 8
    pub deadline_ms: u32,                             // 100–5,000: time the receiver has to send `result`
    pub more_until_ms: u32,                           // deadline_ms–10,000: time the receiver may send `more_found`
}
#[derive(Debug, Clone, Serialize, Deserialize)] #[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct KnowledgeSearch { #[serde(rename = "type")] pub kind: KnowledgeSearchKind, pub query: String /* 1–500 */ }
#[derive(Debug, Clone, Copy, Serialize, Deserialize)] #[serde(rename_all = "lowercase")]
pub enum KnowledgeSearchKind { Lex, Vec }            // the values of the Memory Service's RecallSearchKind

#[derive(Debug, Clone, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub struct KnowledgeHit {
    pub statement: String,                            // 1–4,000 (a concept section) ; content as data, never instructions
    pub agent: String,                                // the knowledge agent whose graph holds the fact, 1–80
    pub repository: String,                           // repository key
    pub path: String,                                 // repository-relative, 1–1,024
    pub commit: String,                               // 40 or 64 lower-case hex
    #[serde(default)] pub section: Option<String>,    // the page section heading, ≤ 200
    #[serde(default)] pub page_title: Option<String>, // set when kind = page
    pub kind: KnowledgeHitKind,
    #[serde(default)] pub tier: Option<PageTier>,     // pages and glossary concepts only
    #[serde(default)] pub status: PageStatus,
    pub may_be_out_of_date: bool,                     // stale_after passed, or the source changed
    pub as_of: Timestamp,                             // the agent's last fetch of `repository`
    /// The answer id of the agent this client asked, when that agent keeps a sources record; set by the
    /// receiving client from `KnowledgeResult.answer_id`, never trusted from a hit.
    #[serde(default)] pub answer_id: Option<KnowledgeAnswerId>,
    #[serde(default)] pub shared_identity: Option<SharedIdentity>,
    #[serde(default)] pub things: Vec<String>,        // ≤ 16 names
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum KnowledgeHitKind { Page, Document }
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub struct SharedIdentity { pub glossary: String /* repository key */, pub concept_path: String }

#[derive(Debug, Clone, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub struct KnowledgeResult {
    pub request_id: uuid::Uuid,
    pub hits: Vec<KnowledgeHit>,                      // ranked, best first, ≤ limit
    pub unanswered: Vec<UnansweredSource>,
    #[serde(default)] pub answer_id: Option<KnowledgeAnswerId>,
    pub from_cache: bool,
    pub more_expected: bool,                          // true while `more_found` events may follow
}
#[derive(Debug, Clone, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub struct UnansweredSource { pub agent: String, pub reason: UnansweredReason }
#[derive(Debug, Clone, Copy, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum UnansweredReason { NotInTime, Refused, Unreachable }

/// A table of contents (ADR-113): no statement ever. Copied from KnowledgeReader.md.
#[derive(Debug, Clone, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub struct KnowledgeContents {
    pub agent_name: String,
    pub generated_at: Timestamp,
    pub repositories: Vec<ContentsRepository>,        // ≤ 220 (repositories, glossaries and hot copies)
    pub things: Vec<ContentsThing>,                   // ≤ 500, most-connected first
}
pub struct ContentsRepository {
    pub repository: String,                           // repository key
    pub role: RepositoryRole,                         // reader | glossary | hot
    pub last_commit: Option<String>,
    pub last_fetched_at: Option<Timestamp>,
    pub readable: bool,
    pub fact_count: u32,
}
#[serde(rename_all = "snake_case")] pub enum RepositoryRole { Reader, Glossary, Hot }
pub struct ContentsThing { pub name: String, pub aliases: Vec<String> /* ≤ 8 */, pub link_count: u32,
                           pub shared_identity: Option<SharedIdentity> }

/// The description of a knowledge agent (route `GET /v1/knowledge/about`; no token — SA-KQS-08).
#[derive(Debug, Clone, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub struct KnowledgeAbout {
    pub agent: String,
    pub issuer: String,                               // the identity provider whose tokens it accepts
    pub token_audience: String,                       // the `aud` it requires
    pub sources_record: Option<SourcesRecordPolicy>,  // None when no record is kept
    pub repository_count: u32,
    pub as_of: Option<Timestamp>,                     // its oldest last fetch
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub struct SourcesRecordPolicy { pub enabled: bool, pub retention_days: u32 }

/// One change notice on `GET /v1/knowledge/changes` (Server-Sent Event `repository_moved`).
pub struct RepositoryMoved { pub repository: String /* repository key */, pub commit: String }
```

**Service API additions** (owners named; ServiceApi.md amendment January 2026): group `knowledge_agents` — `knowledge_agents.list { workspace_id }` → `{ connections: [KnowledgeConnectionView] }`; `knowledge_agents.add { workspace_id, url }` → `{ connection_id, about: KnowledgeAbout }` (person only); `knowledge_agents.remove { connection_id }` → `{}` (person only); `knowledge_agents.details { connection_id }` → `{ connection, about }`; `KnowledgeConnectionView` = `{ connection_id, name, url, added_by: SetBy, sources_record: { enabled, retention_days } | null, last_answered_at | null }`. Memory group: `memory.more_found { workspace_id, recall_id }` → `MoreFoundView { recall_id, wider: [WiderFact], unanswered: [UnansweredSource], done, expired }`. Changed: `knowledge.save_edit` and `knowledge.write` gain optional `type` and `description`; `MemoryUsedView` gains `wider: [WiderUsed]` (sources only, never a statement); `FactView` gains `page_tier` and `page_status`; `PageView` and `PageSummary` gain `type`, `description`, `trust`, `status` and `past_stale_after`, and `PageView` also `verified`. Events: `KnowledgeAgentsChanged { workspace_id }` (Knowledge Query Client) and `MoreFound { workspace_id, recall_id, added, unanswered }` (Memory Service).

- **Example** (`KnowledgeHit`): `{"statement":"Borealis Ltd supplies recycled packaging to the Northern plants.","agent":"procurement-knowledge","repository":"git.example.org/supply/handbook@main","path":"suppliers/borealis.md","commit":"9f3c2a1b7d4e5f60718293a4b5c6d7e8f9a0b1c2","section":"What they supply","page_title":"Supplier Borealis","kind":"page","tier":"human_reviewed","status":"stable","may_be_out_of_date":false,"as_of":"2026-01-10T08:15:02.114Z","shared_identity":{"glossary":"git.example.org/org/glossary@main","concept_path":"borealis-ltd.md"},"things":["Borealis Ltd","packaging"]}`.

### §12 amendment January 2026 — Per-document read access (ADR-115)

*Amended January 2026 (decision 94; HLD revision 0.63.0, ADR-115 — knowledge inherits its readers).* Module `ordin_types::wire::knowledge_query`, as before. Owner of the shapes: **Knowledge Query Server** (KnowledgeQueryServer.md amendment January 2026, *Shared Context*), defined once here so that the phase-2 Knowledge Query Client and Source Access can name them. Used by: Source Access, Knowledge Query Client, Knowledge Query Server, Knowledge Reader, Memory Service, Telemetry, Service API, Screens module.

`ordin-types` gains the workspace dependency `secrecy` (already in the build through Identity and Credentials) for `Delegation` only.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum SourceSystem { Github, Gitlab, Confluence, Sharepoint }

/// The thing an owner grants access to in a source system.
/// Git: `<host>/<owner path>` — the repository key of KnowledgeReader.md SA-KRD-12 with `@<branch>` and
/// `:<path>` removed; access is granted per repository, not per branch or sub-path.
/// Confluence: "<site host>/pages/<page id>". SharePoint: "<drive id>/<item id>".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccessUnit { pub system: SourceSystem, pub unit: String /* 1..=2,048 */ }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum CheckOutcome { Allowed, Refused, Unchecked }

/// The person's account in one source system, learned from the system itself (never from the desktop).
/// Kept by the Knowledge Query Server (`kq_linked_accounts`); never sent to a desktop.
#[derive(Debug, Clone, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub struct LinkedAccount { pub system: SourceSystem, pub account_id: String /* 1..=256 */, pub account_label: String /* 1..=256 */,
    pub linked_at: Timestamp, pub last_used_at: Timestamp }

/// A Microsoft token whose audience is a front door's own app, for one question's on-behalf-of exchange (R8).
/// Derives exactly `Clone, Deserialize`. `Serialize` is hand-written and writes the value only into the request
/// body; `Debug` is hand-written and prints "Delegation { sharepoint_assertion: [redacted] }"; consumers copy
/// this definition verbatim and derive nothing else;
/// the type is never put in a log, an error, the journal or a metric.
#[derive(Clone, Deserialize)] #[serde(deny_unknown_fields)]
pub struct Delegation { pub sharepoint_assertion: secrecy::SecretString /* 1..=16,384 bytes */ }

/// Who a front door asks for (R7: a plain object on the front door's authenticated request, not signed separately).
#[derive(Debug, Clone, Serialize, Deserialize)] #[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AskerStatement {
    pub subject: String,                                             // the person's company subject, 1..=255
    pub accounts: std::collections::BTreeMap<SourceSystem, String>,  // linked account ids, ≤ 3 (github, gitlab, confluence)
    pub issued_at: Timestamp,                                        // refused older than 30 s or > 5 s ahead
}

/// A hit a proxy could not check itself (SharePoint), for the front door to check before release.
/// Sent only towards a front door; never to a member or an MCP client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingHit { pub hit: KnowledgeHit, pub unit: AccessUnit }

/// A system a knowledge agent checks per person, and the host the person's account must be on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountNeed { pub system: SourceSystem, pub host: String /* github.com, a GitLab host, the Confluence site host; 1..=253 */ }
```

**Fields added to existing types** (each `#[serde(default)]`, so a peer from before the amendment keeps working):

| Type | Field | Rule |
|---|---|---|
| `KnowledgeQueryRequest` | `delegation: Option<Delegation>` | a member only; a front door or a service identity sending it → `KQ_DELEGATION_NOT_ALLOWED` |
| `KnowledgeQueryRequest` | `asker: Option<AskerStatement>` | a front door only; a member sending it → `KQ_ASKER_STATEMENT_INVALID` |
| `KnowledgeHit` | `person_checked: bool` | true when the agent holding the fact checked it for this asker; a front door never caches a result holding such a hit |
| `KnowledgeHit` | `commit: String` (rule widened) | Git: 40 or 64 lower-case hex, as before; Confluence: the page version number in decimal, 1..=10 digits; SharePoint: the item's `cTag`, 1..=256 characters |
| `KnowledgeResult` (and the `more_found` body) | `unchecked_sources: bool` | one flag, naming no system or source |
| `KnowledgeResult` (and the `more_found` body) | `pending: Vec<PendingHit>` | towards a front door only; `skip_serializing_if = "Vec::is_empty"` |
| `done` event body | `unchecked_sources: bool` | as above |
| `KnowledgeAbout` | `accounts: Vec<AccountNeed>` | systems this agent checks per person; ≤ 3, unique by system |
| `KnowledgeAbout` | `sharepoint_check_scope: Option<String>` | `api://<client-id>/knowledge.check` when `sharepoint` is configured; ≤ 512 |
| `ContentsRepository` | `system: SourceSystem` (default `github`) | the source's system; a Git row is `gitlab` when its `host-kind` is `gitlab`, else `github` (an audience-open Git row with no `host-kind` reads `github`); Confluence and SharePoint rows carry their own |
| `ContentsRepository` | `audience_open: bool` (default `true`) | a contents entry with `false` lists no thing (`things` come from audience-open facts only); rows with `false` are sent to front doors only, never to members, desktops or MCP clients |

**Service API additions** (owners named; ServiceApi.md amendment January 2026): `knowledge_agents.details` → `{ connection, about, accounts: [ConnectionAccount] }` with `ConnectionAccount { system: SourceSystem, state: AccountLinkState }` and `AccountLinkState` = `linked` | `not_signed_in` | `refused` (owner: Knowledge Query Client); `MemoryUsedView` gains `wider_unchecked: bool` (owner: Memory Service). No account id, account label or token crosses the Service API.

**Validation** (added rows; code, then the owner that raises it):

| Field | Rule | Code | Raised by |
|---|---|---|---|
| `AccessUnit.unit` | 1..=2,048 characters; no control character | `KQ_BAD_QUERY` | Knowledge Query Server (a `pending` unit from a proxy failing it is dropped and logged) |
| `Delegation.sharepoint_assertion` | 1..=16,384 bytes | `KQ_BAD_QUERY` | Knowledge Query Server |
| `AskerStatement.subject` | 1..=255 characters | `KQ_ASKER_STATEMENT_INVALID` | Knowledge Query Server |
| `AskerStatement.accounts` | ≤ 3 entries; keys `github`, `gitlab`, `confluence` only; values 1..=256 characters | `KQ_ASKER_STATEMENT_INVALID` | Knowledge Query Server |
| `AskerStatement.issued_at` | not older than 30 s, not more than 5 s ahead of the receiver's clock | `KQ_ASKER_STATEMENT_INVALID` | Knowledge Query Server |
| `AccountNeed.host` | 1..=253 characters, lower-case host name | `KQ_CONFIG_INVALID` | Knowledge Query Server (`about` built from configuration) |
| link body `accounts[]` (route 41) | 1..=3 entries; `system` ∈ {`github`, `gitlab`, `confluence`}, unique; `token` 1..=8,192 bytes | `KQ_BAD_ACCOUNTS` | Knowledge Query Server |

- **Example** (`KnowledgeAbout` of an agent that checks GitHub and SharePoint): `{"agent":"procurement-knowledge","issuer":"https://id.example.org/oauth2/default","token_audience":"ordin-knowledge","sources_record":null,"repository_count":4,"as_of":"2026-01-10T08:15:02.114Z","accounts":[{"system":"github","host":"github.com"}],"sharepoint_check_scope":"api://7c1e0b52-app/knowledge.check"}`.

---

## Names fixed by this spec

Bubble-up candidates for `08-interfaces.md`:

1. **ordin hook program:** `ordin-hook` (`ordin-hook.exe` on Windows) — ADR-045 "name fixed in `/spec`".
2. **Hook environment:** `ORDIN_HOOK_ENDPOINT`, `ORDIN_HOOK_SECRET`, `ORDIN_SESSION_ID`, `ORDIN_HOOK_PROTOCOL`.
3. **Hook channel:** Unix domain socket `<app data root>/run/hook.sock` (mode `0600`) on macOS and Linux; named pipe `\\.\pipe\ordin-hook-<installation id>` on Windows; newline-delimited JSON, one request per connection, interim `holding` then one final reply; HMAC-SHA256 message authentication with the per-session secret.
4. **Hook protocol version:** `1`.
5. **Service API version:** `1.0`; header `Ordin-Api-Version` on every Remote API response.
6. **Command naming:** `<module>.<verb>`; Remote API path `POST /v1/<module>/<verb>`; approval presses `approval.decide` / `POST /v1/approval/decide`.
7. **Event kinds:** `run_state_changed`, `run_progress`, `plan_changed`, `step_marked`, `start_up_report_ready`, `approval_requested`, `approval_decided`, `crew_round_changed`, `block_shown`, `reset`; the SSE `event:` line equals the kind. *Amended January 2026 (ADR-113):* also `knowledge_agents_changed` and `more_found`.
8. **Block kind identifiers:** `message`, `question`, `form`, `documents`, `preview`, `document`, `changes`, `plan_progress`, `table`, `chart`, `key_figures`, `report`, `email_list`, `email`, `telegram_list`, `slack_list`, `result`, `agent_proposal`, `agent_library`, `crew_plan`, `memory`, `knowledge` — ADR-028 / ADR-067 "block schemas fixed in `/spec`".
9. **MCP Front tool names:** `show`, `report`, `plan` — ADR-067 and ADR-068 "name fixed in `/spec`"; argument types `Block`, `ReportToolArgs`, `PlanToolArgs`.
10. **Chart kind identifiers:** `bar` (`vertical`/`horizontal`, `grouped`/`stacked`), `line`, `area`, `scatter`, `pie`; column types `text`, `number`, `money`, `percentage`, `date`, `document_link`.
11. **Workflow file format 3 keys:** `format`, `name`, `purpose`, `inputs` (`label`, `kind`, `required`), `agents` (`key`, `repository` {`profile_key` | `address`}, `id`, `version`, `name`, `role`), `steps`, `limits` (`max_rounds`, `wall_clock_minutes`, `request_budget`, `agents_at_once`, `failure_repeat_limit`, `external_action_agents`), `origin`; step kinds `agent`, `if`, `for_each`, `repeat_until`, `run_workflow` (with `each`, `until`, `call` accepted on read); step fields `position`, `agent`, `line`, `uses`, `asks_first`, `ordin_checks`, `if_it_fails` (`ordin_takes_over` | `ask_me` | `skip_it`), `decided_by` (`application` | `ordin` | `person`), `paths`, `list`, `max_items`, `max_times`, `workflow`; application checks `list_has_items`, `step_failed`, `check_found_shortfall`, `document_changed`.
12. **Rule sources:** `declaration_gate`, `declaration_tool_rule`, `declaration_argument_rule`, `agent_tool_policy`, `app_folder_policy`, `app_shell_policy`, `app_approval_policy`, `fail_closed`.
13. **Adapter kinds and verbs:** `claude_local`, `copilot_local`, `hosted_agent`, `slack_thread`; `start`, `progress`, `pause`, `resume`, `read`, `cancel`.
14. **Card reference form:** 16 random bytes, base64url, 22 characters.

## Assumed limits

Every value below is chosen here, not given by the HLD, and needs confirmation:

| # | Limit | Value |
|---|---|---|
| LW-1 | Service API major / minor range | 1–999 / 0–999 |
| LW-2 | Command body size | 1 MiB |
| LW-3 | Command `request_id` replay window | 10 minutes |
| LW-4 | Every command is `POST`; domain refusals reply 200 with `kind: error`; status mapping 401/409/422/429 | as listed |
| LW-5 | `ServiceError.message` length | 1–300 characters |
| LW-6 | Event `data` size | 256 KiB |
| LW-7 | `RunProgress.line` | 1–160 characters |
| LW-8 | `ApprovalDecided.decided_on` | 1–80 characters |
| LW-9 | Runtime/remote identifiers | 1–256 characters |
| LW-10 | Task arguments / free text | 16,384 characters |
| LW-11 | Attachments per task | 50 |
| LW-12 | Single-run wall-clock ceiling | 8 hours (crew maximum applied to single runs) |
| LW-13 | Idle timeout range | 30–3,600 s |
| LW-14 | Person question / answer | 500 / 4,000 characters |
| LW-15 | Final message | 32,768 characters |
| LW-16 | Deny / reject reason | 500 characters |
| LW-17 | Tool name | 128 characters |
| LW-18 | Tool arguments | 1 MiB |
| LW-19 | MCP server name | 1–64 characters, `[a-zA-Z0-9_.-]` |
| LW-20 | Rule identifier | 128 characters |
| LW-21 | Hook allow/deny reply time without *ask* | 2 s |
| LW-22 | Hook's own deadline | 50 s |
| LW-23 | Harness hook timeout registered | 60 s |
| LW-24 | Hook transport, peer-credential check, HMAC scheme | as fixed in *Names* item 3 |
| LW-25 | Approval question / verb / where / changes | 120 / 32 / 200 / 1,000 characters |
| LW-26 | Approval affects items | 1–50 |
| LW-27 | Approval message / command / files | 100,000 characters / 8,000 characters / 100 files |
| LW-28 | Start-up report items / plain sentence | 200 / 200 characters |
| LW-29 | Any block | 2 MiB (A-34's report figure extended to every kind) |
| LW-30 | Message text / attachments | 8,000 characters / 20 |
| LW-31 | Question text / choice | 300 / 60 characters |
| LW-32 | Form fields / label / choice options | 20 / 80 characters / 2–20 |
| LW-33 | Documents block items | 500 |
| LW-34 | Path length | 1,024 bytes |
| LW-35 | Plan versions per run | 20 |
| LW-36 | Table/chart title, column name, text cell | 120 / 60 / 2,000 characters |
| LW-37 | Key figure label / unit / against | 60 / 24 / 60 characters |
| LW-38 | Report `as_of` future tolerance | 5 minutes |
| LW-39 | Source documents per part; version / system / by | 20; 64 / 80 / 80 characters |
| LW-40 | Email list messages | 200 |
| LW-41 | Result summary / proposal reason / memory facts | 2,000 / 300 characters / 100 |
| LW-42 | Block rejection reason / count | 300 characters / 20 |
| LW-43 | Workflow purpose / input label / role / origin / path name / condition words | 200 / 60 / 120 / 200 / 40 / 300 characters |
| LW-44 | Workflow inputs | 20 |
| LW-45 | Workflow failure-repeat limit | 1–5 |
| LW-46 | YAML subset: no anchors, aliases or custom tags | — |
| LW-47 | Agent version range syntax | Cargo SemVer requirement |

## HLD inconsistencies

| # | Where | What disagrees | Suggested resolution |
|---|---|---|---|
| I-1 | `04-domain.md:225` vs `09-decisions.md:1421` | Workflow inputs are "document or text", yet a **For each** goes through "a list input". | Add `list` to the input kinds in 04 and 08 (this draft does). |
| I-2 | `08-interfaces.md:18` vs `04-domain.md:297-298` | Format 3 names an agent repository by "its profile key", but the Agent repository entity has no key field (display name, host and repository, branch, format, source…). | Add a profile key to the organisation profile's agent repository entries and to 04's Agent repository. |
| I-3 | `08-interfaces.md:18` vs `04-domain.md:36` | Format 3 carries "a version range", but from revision 0.25.0 "an agent version is a WorkspaceProfile at a commit of an agent repository"; what the range ranges over (a profile `version` field, tags, commits) is not stated. | State in 04 what an agent version number is; this draft assumes a SemVer version on the WorkspaceProfile. |
| I-4 | `03-sequences.md:700` | *Import a workflow* still matches agents "by package and version range" through the Package Catalogue, superseded in 0.25.0; format 3 matches by repository and identifier through the Catalogue Service. | Redraw the sequence for format 3. |
| I-5 | `09-decisions.md:1385` | ADR-064 *Exporting* still says the file names agents "by their marketplace package", with no amendment note for format 3. | Add an amendment note pointing to OQ-47 / format 3. |
| I-6 | `08-interfaces.md:18` vs `09-decisions.md:1418-1423`, `11-ux-guidelines.md:704` | 08 names the block kinds `if`, `each`, `until`, `call`; ADR-065 and §5.20 also name **Ask me** as a separate kind with its own shape. | This draft models Ask me as `if` decided by `person` (as ADR-065 defines it) and spells kinds out; 08 should state the file names once fixed. |
| I-7 | `09-decisions.md:1472` vs `:1486` and `10-risks.md:174` | ADR-067 lists five chart kinds (bar, line, area, scatter, pie) but its consequences and C-40 say "six chart kinds". | Decide whether horizontal bar counts as a kind, and make the count agree. |
| I-8 | `08-interfaces.md:108` vs `memory/src/types/ports.rs:338`, `memory/src/mcp/mod.rs:191-271` | 08 says the memory library offers "write, recall, forget and freshness check"; the crate removed its freshness checker (freshness is agent-side, its ADR-014) and offers `recall`, `remember`, `get`, `retire`, `delete`, `capabilities`. Also, 02 says the library is "linked in-process" with one tenant per installation (`02-architecture.md:348-350`), while every `Service` call authenticates a bearer token (`memory/src/service/mod.rs:58-66`) and derives its scope from token claims (`memory/src/types/scope.rs:21-48`). | Update 08's row; state how the Memory Service obtains the token it presents in-process (a locally minted token, or a trusted-caller path in the crate). |
| I-9 | `08-interfaces.md:109`, `02-architecture.md:377-381` vs `sandbox/sandbox-host/Cargo.toml:1-4`, `src/controller.rs:30-38` | The HLD links `sandboxd`'s library in-process; the crate is built as a daemon with a control socket and token file, and owns its own interaction surface (sign-in, consent, step-up via `ConsentUI`), whereas in ordin approvals and sign-in belong to the Approval Service and Identity and Credentials. | State in 02 that ordin supplies its own `Interactor` implementation and does not start the daemon binary. |
| I-10 | `02-architecture.md:341` vs `08-interfaces.md:24` | The Ordin Host offers Ordin's tools "through the Copilot SDK's custom tools"; 08 says from revision 0.9.0 they are served by the MCP Front. | Amend 02's Ordin Host bullet. |
| I-11 | `08-interfaces.md:22` vs `11-ux-guidelines.md:572`, `:580`, `:584`, `:587` | 08's block catalogue row lists ADR-028, ADR-067 and ADR-070 kinds; §5.15 also lists Document, Telegram list, Agent library and Knowledge as catalogue kinds. | Add those four kinds to 08's row. |
| I-12 | `08-interfaces.md:27`, `02-architecture.md:521` vs `09-decisions.md:844-849` | The hook protocol reply includes "ask", but ADR-045 has the hook wait and return only allow or deny to the harness. | Resolved here by an interim `holding` frame followed by a final allow or deny; 08 can adopt that wording. |
| I-13 | `10-risks.md:43` | A-32's revised text replaces the step and agent counts but does not say whether "a step's line is at most 200 characters" and "8 agents" still hold. | Restate both in the revised A-32 (this draft keeps 200 and 8). |

### Former gaps, now settled

Each item this file once listed as a gap is settled in the component spec named; the identifiers G-1 to G-5 are kept because other specs cite them.

| # | Former gap | Settled in |
|---|---|---|
| G-1 | The full Service API command and event list | The *Command catalogue, version 1.0* of `components/ServiceApi.md` (400 commands in 39 groups after ADR-114, *amended January 2026*; 395 in 39 after ADR-112 and ADR-113, 390 in 38 after ADR-111, 377 after ADR-110, 369 after ADR-109, 355 after ADR-108, `:854`; *amended January 2026, ADR-103, ADR-104 and ADR-106*; the catalogue test at `:1620`; *amended January 2026, ADR-102*); the event kinds are `EventKind` (§1, C-16), with payloads in that spec's *Event catalogue*. Closed January 2026 (decision 80). |
| G-2 | Pairing route shape (one-time code, pairing line, desktop key reply) | `components/RemoteApi.md`: `POST /v1/pairing` with `PairRequest { api_version, code, computer_name }` and reply `PairReply { desktop_pairing_id, desktop_key, backend, server_name, api_version }`, and `PairingLine` (routes table `:293`–`:300`; types `:86`). |
| G-3 | Idle timeout default | `components/SessionSupervisor.md`: 10 minutes (`idle_timeout_secs` 600; *Configuration* row `:154`, defaults `:246`), from 2D-4. |
| G-4 | Request budget default | `components/SessionSupervisor.md`: `RunLimits.request_budget` is `Option<u32>` and the default is no budget (`:66`, Internal Logic step 2 `:397`), until OQ-3 names a ceiling. |
| G-5 | Hook protocol on Windows | `components/HookRelay.md`: a named pipe `\\.\pipe\ordin-hook-<installation id>` created with `first_pipe_instance(true)` and `reject_remote_clients(true)` (Internal Logic `:217`). |

## Gaps

None.
