# SPEC: Telemetry

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-107; promoted to Approved by the user January 2026) · amended January 2026 (ADR-113; promoted to Approved by the user January 2026) · amended January 2026 (ADR-115)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-telemetry` · **Build phase:** 1 · **Programs:** A
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
- [Amendment January 2026 — Knowledge agents (ADR-113)](#amendment-january-2026--knowledge-agents-adr-113)

## Purpose

Owns every metric and span name of a hosted agent and the two exports that carry them: Prometheus counters on every instance, rendered for the metrics route the Caller API serves, and OpenTelemetry traces sent to the organisation's collector only when the agent configuration switches traces on and `ORDIN_OTLP_ENDPOINT` is set (`02:829`, ADR-098 item 6, `07:15`–`16`). Metric names are wrightrun's, unchanged, so a deployment's dashboards and alerts keep working (SA-OBS-02); ordin's additions take the same prefix. A span holds no content — no message, argument, file content, address or identifier of a person — and sending spans never delays, repeats or fails a run: a span that cannot be sent is dropped and counted (`07:16`). This crate is linked into the agent profile only; on the desktop and the personal server A-10 holds and nothing of it runs (SA-XC-04, SA-XC-05).

## Approach

One process-wide `Telemetry` value, created once at start by the server program, holds a `prometheus-client` registry of its own with every metric family created up front, and — when traces are on — an OpenTelemetry tracer provider whose span processor is this crate's own bounded queue in front of the OTLP-over-HTTP exporter, so a full queue or a failed export drops and counts instead of waiting. Components increment through one typed call, `record(TelemetryEvent)`, whose enum fixes each metric's labels and closed label values at compile time (reconciliation R4: this form wins; there is no `add(name, labels, by)` on `Telemetry`). Components that do not depend on this crate (2B-3) declare a small `MetricsSink` of their own, which the server program implements with this crate's `event_from_named` and `record` — the *sink adapter*. No counter is derived from a Journal event: the Journal carries no event for these counters. Rejected: the library's default registry, because it would also expose whatever another crate registers; the SDK's own batch processor, because it drops a full queue without a count the operator can read.

**Error prefix:** `TEL_` *(chosen here)*.

## Shared Context

**From 2D (copied).**

| Variable | Type | Default | Required | Owner | Description |
|---|---|---|---|---|---|
| `ORDIN_OTLP_ENDPOINT` | URL (`https://`, or `http://` to loopback) | none | No | Telemetry | Address of the organisation's OpenTelemetry collector (OTLP over HTTP). Traces are sent only when the agent configuration switches tracing on **and** this is set; switched on with this unset, the start-up report says traces are not sent. ordin configures its exporter from this variable only and does not let the OpenTelemetry library read `OTEL_*` variables. |
| `ORDIN_LOG_LEVEL` | `error`, `warn`, `info`, `debug` | `info` | No | ordin server | Level of the JSON log lines (read by the program, not here). |

Read once at start; a change reaches a running instance only by a restart (2D-1a). A malformed value stops the start with a named error, and the instance never reports ready.

**From 2C (copied).**

```rust
/// UUIDv7, 36-character lowercase hyphenated string (2C conventions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RunId(pub uuid::Uuid);
```

**From the agent configuration (Agent Configuration spec; read by the program and passed in).** `AgentConfig.traces: bool` — `true` switches traces on (key *chosen* in the Agent Configuration spec). `Manifest.package.name: String` — the agent's package name, the value of the `agent` label.

**Libraries (1A).** `prometheus-client` 0.25.1 (SA-OBS-02); `opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp` 0.33.1 with the HTTP-protobuf transport, bridged from `tracing` by `tracing-opentelemetry` 0.34.0 (SA-OBS-03); `tracing` 0.1.44 (SA-OBS-01); `tokio`; `thiserror`; `uuid`; `url`. No other library.

**Constants** *(chosen here unless a source is cited)*:

```rust
pub const METRIC_PREFIX: &str = "wrightrun_";             // SA-OBS-02: wrightrun's names unchanged; ordin's additions take it too
pub const UNKNOWN: &str = "unknown";                      // a name no declaration holds (wrightrun MetricsEmitter)
pub const NONE: &str = "none";                            // an absent digest
pub const MAX_LABEL_CHARS: usize = 128;                   // wrightrun MetricsEmitter, Naming
pub const SPAN_QUEUE: usize = 2_048;                      // spans waiting to be sent
pub const SPAN_BATCH: usize = 512;                        // spans per export request
pub const SPAN_FLUSH_EVERY: std::time::Duration = std::time::Duration::from_secs(5);
pub const EXPORT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
pub const SHUTDOWN_FLUSH: std::time::Duration = std::time::Duration::from_secs(5);
pub const OPENMETRICS_CONTENT_TYPE: &str = "application/openmetrics-text; version=1.0.0; charset=utf-8";
```

### Reconciliation assumptions

- **SA-RECON-01** `record(TelemetryEvent)` and OpenMetrics output are the one contract; callers with a Telemetry edge call `record`, the others reach it through the server program's sink adapter over `event_from_named` (R4, row 58).
- **SA-RECON-02** The metric list is closed at 49 families; 48 and 49 are the MCP Front's (R4). *Amended January 2026 (ADR-107):* 50 families; 50 is the Rules Engine's content checks. The Journal, the Checker and the Resolver add no metric or exported span (rows 64, 65).
- **SA-RECON-03** Families 27 and 28 are fed by Retention and Erasure; family 29 by the server program after `AgentLoader::load` (rows 59, 60).
- **SA-CLOSE-01** *(K17)* `ErasedKind` gains `MemoryItems` (`memory-items`), which Retention and Erasure emits for `ErasureCounts.memory_items` (ordin keeps memory pages by scope, ADR-097 item 5). Families 36 and 37 are fed by the Knowledge Service's chat-history ingest step through the program's sink adapter (`event_from_named`), not by the Job Runner, which only schedules and leases the ingest job (ruling R14).
- **SA-RECON-04** The Agent Loader keeps its own `TraceStatusView`; the server program maps `TraceStatus` to it (row 61).

## Public Interface

```rust
// crate ordin-telemetry

/// Built once by the server program, before any instance role starts.
pub struct TelemetryConfig {
    pub ordin_version: String,            // e.g. "1.0.0"
    pub agent: String,                    // Manifest.package.name
    pub traces_switched_on: bool,         // AgentConfig.traces
    pub otlp_endpoint: Option<url::Url>,  // ORDIN_OTLP_ENDPOINT, checked by `from_env`
}
impl TelemetryConfig {
    /// Reads ORDIN_OTLP_ENDPOINT only. TEL_ENDPOINT_INVALID on a malformed value.
    pub fn from_env(ordin_version: String, agent: String, traces_switched_on: bool) -> Result<Self, TelemetryError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceStatus { Off, On, SwitchedOnNoEndpoint }

pub struct Telemetry { /* registry, metric families, optional tracer provider, span queue sender */ }

/// Creates the process-wide value. Must run inside a tokio runtime (the span exporter task).
/// TEL_ALREADY_INITIALISED on a second call; TEL_EXPORTER_BUILD when the exporter cannot be built.
pub fn init(config: TelemetryConfig) -> Result<&'static Telemetry, TelemetryError>;
/// The value `init` created; `None` before `init` (and always in a command that never calls it, such as check).
pub fn get() -> Option<&'static Telemetry>;

impl Telemetry {
    /// One increment (or one gauge set) for one event. Never fails and never awaits.
    pub fn record(&self, event: TelemetryEvent);
    /// The OpenMetrics exposition of every family, and its media type
    /// (`OPENMETRICS_CONTENT_TYPE`), for `GET /metrics`. The route answers with exactly these
    /// bytes and this media type; there is no `String` or `text/plain; version=0.0.4` form.
    pub fn render(&self) -> (Vec<u8>, &'static str);
    /// Sets wrightrun_release_info to exactly one series. TEL_RELEASE_INVALID on a bad argument.
    pub fn set_release(&self, release_id: &str, digest: Option<&str>) -> Result<(), TelemetryError>;
    pub fn trace_status(&self) -> TraceStatus;
    /// A `tracing` layer that turns the span names below into OpenTelemetry spans; `None` when traces are not on.
    /// The program adds it to its subscriber once.
    pub fn tracing_layer<S>(&self) -> Option<Box<dyn tracing_subscriber::Layer<S> + Send + Sync>>
    where S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>;
    /// W3C `traceparent` header value of the current span; `None` when traces are not on or no span is current.
    pub fn traceparent(&self) -> Option<String>;
    /// Sends what is queued, waiting at most SHUTDOWN_FLUSH; spans still queued then are dropped and counted.
    pub async fn shutdown(&self);
}

/// The sink adapter's one call: maps a family name (with or without `_total`) and its label
/// pairs to the `TelemetryEvent` that `record` takes (step M-6). The server program's
/// `MetricsSink` implementations call it and then `record`; on `Err` they log the code once
/// per name and drop the increment.
pub fn event_from_named(name: &str, labels: &[(&str, &str)], by: u64) -> Result<TelemetryEvent, TelemetryError>;

#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    #[error("error[TEL_ENDPOINT_INVALID]: {0} (ORDIN_OTLP_ENDPOINT)")] EndpointInvalid(String),
    #[error("error[TEL_ALREADY_INITIALISED]: telemetry is already set up for this process")] AlreadyInitialised,
    #[error("error[TEL_EXPORTER_BUILD]: the trace exporter could not be built ({0})")] ExporterBuild(&'static str),
    #[error("error[TEL_RELEASE_INVALID]: {0}")] ReleaseInvalid(&'static str),
    #[error("error[TEL_NAME_UNKNOWN]: no metric family has this name")] NameUnknown,
    #[error("error[TEL_LABEL_INVALID]: the labels do not match the family's label set")] LabelInvalid,
}

impl ordin_types::CodedError for TelemetryError {
    fn code(&self) -> &'static str;     // the TEL_ code in the message
    fn retryable(&self) -> bool;        // always false
    fn repair(&self) -> Option<ordin_types::RepairAction>; // always None
}
```

### Span names and their fields

Every span is opened by the component named, with `tracing::info_span!` and one of these names; the `tracing_layer` exports only spans whose name is in this table and only the fields listed for it (S-4 below). Names *chosen here* (`02:829`: "Owns: metric and span names").

```rust
pub const SPAN_RUN_STRETCH: &str = "ordin.run_stretch";     // Session Supervisor: one stretch of a run
pub const SPAN_MODEL_TURN: &str = "ordin.model_turn";       // Session Supervisor: one assistant turn of the harness stream
pub const SPAN_TOOL_CALL: &str = "ordin.tool_call";         // MCP Front (ordin's MCP servers) / Hook Relay (built-in tools): one tool call, decision to result
pub const SPAN_SUBAGENT_CALL: &str = "ordin.subagent_call"; // Session Supervisor: one call of a sub-agent
pub const SPAN_HTTP_OUT: &str = "ordin.http_out";           // Connection Binder: one outbound HTTP call of a connection
```

| Span | Fields (attribute keys) | Values |
|---|---|---|
| `ordin.run_stretch` | `run_id`, `started_by`, `command`, `outcome` | run UUID; `caller`, `schedule`, `intake`, `chat`; a command slug or `none`; `finished`, `paused`, `ended-by-control`, `ended-at-limit`, `failed`, `cancelled`, `interrupted` |
| `ordin.model_turn` | `model`, `input_tokens`, `output_tokens`, `outcome` | the `provider:model` text of the configuration; integers; `answered`, `failed`, `blocked` |
| `ordin.tool_call` | `tool`, `origin`, `outcome`, `control` | a declared tool name or `unknown`; `built_in`, `ordin_mcp_front`, `package_mcp_server`; `allowed`, `refused`, `paused`, `failed`; a control name or `none` |
| `ordin.subagent_call` | `subagent`, `outcome` | `general-purpose` or the reviewer's name; `finished`, `stopped` |
| `ordin.http_out` | `connection`, `kind`, `status` | a connection name; `http`, `webhook`, `mcp`; the HTTP status as text, or `timeout`, `failed` |

Callers open these spans with `tracing::info_span!(SPAN_…, field = …)` and need no Telemetry edge: the name string and the field keys are the contract. The MCP Front opens `ordin.tool_call` with `tool`, `origin = "ordin_mcp_front"`, `outcome`, `control` — not a span of its own name (reconciliation row 62). Any other span name — for example `journal.prune`, `checker.check`, `resolver.build`, `mcp_front.call` — is never exported, so those specs do not open one for export (row 65); a span used only for local `tracing` logs is unaffected.

The trace identifier of a stretch is the run's identifier: the 16 bytes of the `RunId` UUID (S-3). Every span also carries the duration and the span status the SDK records.

### Metrics

Every family below is registered on this crate's own registry at `init`. Counter names render with `_total`. Names 1–46 are wrightrun's `MetricsEmitter` names and label sets, unchanged (SA-OBS-02); 47–50 are ordin's additions (48 and 49 added for the MCP Front, R4; 50 for content checks, ADR-107). The list is closed: a family not in this table does not exist, and `event_from_named` refuses its name. "Fed by" is the ordin component that records the event (02 — *Replacing wrightrun* maps each wrightrun unit to it); "sink" means that component has no Telemetry edge and calls its own `MetricsSink::add(name, labels, by)`, which the server program's sink adapter turns into `record(event_from_named(…)?)`.

| # | Name | Labels (closed values) | Fed by |
|---|---|---|---|
| 1 | `wrightrun_runs_queued_total` | `started_by`: `caller`, `schedule`, `intake`, `chat` | Run Queue (sink) |
| 2 | `wrightrun_runs_started_total` | `started_by` | Run Queue (sink) |
| 3 | `wrightrun_runs_interrupted_total` | — | Run Queue (sink) |
| 4 | `wrightrun_schedule_firings_total` | `duty` (declared) | Duty Timer |
| 5 | `wrightrun_runs_ended_at_limit_total` | `started_by` | Session Supervisor (sink) |
| 6 | `wrightrun_tool_calls_paused_for_approval_total` | `tool` (declared) | Run Queue (sink) |
| 7 | `wrightrun_intake_events_accepted_total` | `connection` (intake name) | Caller API |
| 8 | `wrightrun_intake_events_refused_total` | `connection` (intake name or `unknown`), `reason` (error code) | Caller API |
| 9 | `wrightrun_outbound_messages_sent_total` | `connection` | Connection Binder |
| 10 | `wrightrun_outbound_messages_failed_total` | `connection` | Connection Binder |
| 11 | `wrightrun_runs_ended_by_control_total` | `agent`, `control`, `source`: `package`, `configuration`, `setting` | Rules Engine (sink) |
| 12 | `wrightrun_tool_calls_refused_total` | `agent`, `control`, `source` | Rules Engine (sink) |
| 13 | `wrightrun_tool_calls_paused_by_rule_total` | `agent`, `control`, `source` | Rules Engine (sink) |
| 14 | `wrightrun_run_endings_refused_total` | `agent`, `control`, `source` | Rules Engine (sink) |
| 15 | `wrightrun_controls_set_aside_total` | `agent`, `control`, `source` | Rules Engine (sink) |
| 16 | `wrightrun_safe_replies_total` | `reason`: `model-failed`, `content-filter` | Session Supervisor (sink) |
| 17 | `wrightrun_fallback_runs_total` | — | Session Supervisor (sink); only when the harness stream states the fallback model answered (C-77) |
| 18 | `wrightrun_chat_events_received_total` | `connection`, `kind`: `mention`, `thread-mention`, `command`, `action`, `form`, `membership`, `other` | Slack Channel Adapter (sink) |
| 19 | `wrightrun_chat_replies_delivered_total` | `connection`, `delivery`: `asker`, `thread`; `kind`: `reply`, `safe-reply`, `failure-text`, `approval-text` | Slack Channel Adapter (sink) |
| 20 | `wrightrun_chat_actions_total` | `connection`, `action`: `share`, `hand-off`, `link` | Slack Channel Adapter (sink) |
| 21 | `wrightrun_jobs_started_total` | `kind`: `ingest`, `retention` | Job Runner |
| 22 | `wrightrun_jobs_finished_total` | `kind` | Job Runner |
| 23 | `wrightrun_jobs_failed_total` | `kind` | Job Runner |
| 24 | `wrightrun_agent_events_recorded_total` | `kind`: `error`, `escalation`; `category`: `tool-failed`, `missing-input`, `could-not-finish`, `needs-person`, `needs-approval`, `out-of-scope`, `other` | Connection Binder |
| 25 | `wrightrun_notes_written_total` | `source`, `operation`: `write`, `replace` | Knowledge Service (sink) |
| 26 | `wrightrun_note_writes_refused_total` | `source`, `reason`: `limit`, `too-long` | Knowledge Service (sink) |
| 27 | `wrightrun_retention_items_deleted_total` | `kind`: `chat-history`, `entries`, `notes`, `chat-replies`, `runs`, `jobs` | Retention and Erasure (sink), inside the Job Runner's retention job |
| 28 | `wrightrun_erased_items_total` | `kind`: `knowledge-items`, `memory-items`, `chat-replies`, `pending-questions`, `runs`, `threads`, `settings`, `jobs`, `channels`; `request`: `person`, `scope` | Retention and Erasure (sink), on the Caller API's operator erase |
| 29 | `wrightrun_release_info` (gauge) | `release`, `digest`, `runtime_version` | `set_release`, by the server program after `AgentLoader::load` returns the release (the Agent Loader has no Telemetry edge) |
| 30 | `wrightrun_runs_ended_total` | `state`: `finished`, `ended-by-control`, `ended-at-limit`, `failed`, **`cancelled`** (ordin: ADR-098 item 2 adds cancel) | Run Queue (sink) |
| 31 | `wrightrun_run_leases_lost_total` | — | Session Supervisor (sink) |
| 32 | `wrightrun_chat_events_dropped_total` | `connection`, `reason`: `duplicate`, `not-enabled`, `not-permitted`, `no-trigger`, `empty` | Slack Channel Adapter (sink) |
| 33 | `wrightrun_chat_link_closed_total` | `connection` | Slack Channel Adapter (sink) |
| 34 | `wrightrun_chat_questions_abandoned_total` | `connection` | Slack Channel Adapter (sink) |
| 35 | `wrightrun_knowledge_queries_total` | `source`, `operation`: `search`, `recent`, `open` | Knowledge Service (sink) |
| 36 | `wrightrun_knowledge_items_written_total` | `source` | Knowledge Service (sink), in its chat-history ingest step (`KnowledgeIngest::ingest_step`, ruling R14) that the Job Runner's ingest job calls |
| 37 | `wrightrun_ingest_thread_failures_total` | `source` | Knowledge Service (sink), in the same ingest step; *amended January 2026 (ADR-107)*: also counts one thread whose replies could not be read during ingest (the page goes on) |
| 38 | `wrightrun_setting_writes_total` | `setting`, `outcome`: `saved`, `invalid`, `not-allowed`, `version-conflict` | Settings Store (sink) |
| 39 | `wrightrun_setting_values_ignored_total` | `setting` | Settings Store (sink) |
| 40 | `wrightrun_http_requests_total` | `route` (route name), `status` | Caller API |
| 41 | `wrightrun_http_refusals_total` | `code` | Caller API |
| 42 | `wrightrun_work_folder_changes_total` | `action`: `write`, `remove` | Caller API |
| 43 | `wrightrun_operator_requests_total` | `route`, `outcome`: `done`, `refused` | Caller API |
| 44 | `wrightrun_chat_replies_redelivered_total` | `connection` | Slack Channel Adapter (sink) |
| 45 | `wrightrun_chat_reconciliations_total` | `connection`, `outcome`: `done`, `incomplete`, `failed` | Slack Channel Adapter (sink) |
| 46 | `wrightrun_chat_channels_reconciled_total` | `connection`, `change`: `enabled`, `disabled` | Slack Channel Adapter (sink) |
| 47 | `wrightrun_spans_dropped_total` *(ordin, chosen here)* | `reason`: `queue-full`, `export-failed`, `shutdown` | Telemetry itself |
| 48 | `wrightrun_mcp_front_calls_total` *(ordin, R4)* | `server` (an MCP Front server name, or `unknown`), `outcome`: `answered`, `refused`, `failed`, `cancelled` | MCP Front (sink) |
| 49 | `wrightrun_mcp_front_call_seconds` (histogram) *(ordin, R4)* | `server`; buckets 0.01, 0.05, 0.1, 0.5, 1, 5, 30, 120, 300 seconds | MCP Front (sink) |
| 50 | `wrightrun_content_checks_total` *(ordin, ADR-107)* | `point`: `input`, `output`; `verdict`: `allow`, `block`, `unreachable` | Rules Engine (sink), once per content check called (RulesEngine.md L.7) |

The `HELP` text of families 1–46 is wrightrun's *Documentation texts* table, verbatim; family 47 reads `Spans not sent to the collector.`; 48 `Tool calls the MCP Front answered, by server and outcome.`; 49 `Time from a tool call reaching the MCP Front to its answer.`; 50 `Content checks ordin asked for, by point and verdict.`

```rust
/// One variant per family; label values that come from a declaration are passed as &str and
/// reduced to UNKNOWN by the caller when they did not match a declared name (M-3).
pub enum TelemetryEvent {
    RunQueued { started_by: StartedByKind },
    RunStarted { started_by: StartedByKind },
    RunsInterrupted { count: u64 },
    ScheduleFired { duty: String },
    RunEndedAtLimit { started_by: StartedByKind },
    ToolCallPausedForApproval { tool: String },
    IntakeAccepted { intake: String },
    IntakeRefused { intake: String, code: &'static str },
    OutboundSent { connection: String },
    OutboundFailed { connection: String },
    Control { what: ControlCounter, control: String, source: ControlSource },
    SafeReply { reason: SafeReplyReason },
    FallbackUsed,
    ChatEventReceived { connection: String, kind: ChatEventKind },
    ChatReplyDelivered { connection: String, delivery: DeliveryKind, kind: ReplyKind },
    ChatAction { connection: String, action: ChatActionKind },
    Job { what: JobCounter, kind: JobKind },
    AgentEventRecorded { kind: AgentEventKind, category: AgentEventCategory },
    NoteWritten { source: String, operation: NoteOperation },
    NoteWriteRefused { source: String, reason: NoteRefusal },
    RetentionDeleted { kind: RetentionKind, count: u64 },
    Erased { kind: ErasedKind, request: ErasureRequest, count: u64 },
    RunEnded { state: RunEndState },
    RunLeaseLost,
    ChatEventDropped { connection: String, reason: DropReason },
    ChatLinkClosed { connection: String },
    ChatQuestionsAbandoned { connection: String, count: u64 },
    KnowledgeQuery { source: String, operation: KnowledgeOp },
    KnowledgeItemsWritten { source: String, count: u64 },
    IngestThreadFailed { source: String },
    SettingWrite { setting: String, outcome: SettingWriteOutcome },
    SettingValueIgnored { setting: String },
    HttpRequest { route: &'static str, status: u16 },
    HttpRefusal { code: &'static str },
    WorkFolderChange { action: WorkFolderAction },
    OperatorRequest { route: &'static str, outcome: OperatorOutcome },
    ChatReplyRedelivered { connection: String },
    ChatReconciliation { connection: String, outcome: ReconcileOutcome },
    ChatChannelReconciled { connection: String, change: ChannelChange },
    McpFrontCall { server: String, outcome: McpCallOutcome, seconds: f64 },  // 48 and 49 together
    ContentCheck { point: CheckPointLabel, verdict: CheckVerdict },          // 50 (ADR-107)
}
```

Each label enum (`StartedByKind`, `ControlCounter`, `ControlSource`, `SafeReplyReason`, `ChatEventKind`, `DeliveryKind`, `ReplyKind`, `ChatActionKind`, `JobCounter`, `JobKind`, `AgentEventKind`, `AgentEventCategory`, `NoteOperation`, `NoteRefusal`, `RetentionKind`, `ErasedKind`, `ErasureRequest`, `RunEndState`, `DropReason`, `KnowledgeOp`, `SettingWriteOutcome`, `WorkFolderAction`, `OperatorOutcome`, `ReconcileOutcome`, `ChannelChange`, `McpCallOutcome`, `CheckPointLabel`, `CheckVerdict`) is `#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]`, has exactly the values the metrics table lists, and implements `prometheus_client::encoding::EncodeLabelValue` writing the kebab-case value shown in the table. `ControlCounter` is `EndedRun` (11), `RefusedCall` (12), `PausedCall` (13), `RefusedEnding` (14), `SetAside` (15); `JobCounter` is `Started` (21), `Finished` (22), `Failed` (23).

### Who calls each public item (reconciliation §3; R4, R6)

| Item | Called by |
|---|---|
| `TelemetryConfig::from_env`, `init`, `tracing_layer`, `shutdown` | server program (agent profile), at start and stop |
| `get` | server program; any component with a Telemetry edge that was not handed the value |
| `record` | Caller API (`HttpRequest`, `HttpRefusal`, `IntakeAccepted`, `IntakeRefused`, `OperatorRequest`, `WorkFolderChange`), Duty Timer (`ScheduleFired`), Job Runner (`Job`), Connection Binder (`OutboundSent`, `OutboundFailed`, `AgentEventRecorded`); the server program's `McpMetrics` implementation for the MCP Front (`McpFrontCall`, families 48 and 49); the server program's sink adapter for every other family marked "sink" |
| `event_from_named` | server program's sink adapter, implementing the `MetricsSink` (or `KnowledgeMetrics`) trait of Run Queue, Settings Store, Retention and Erasure, Channel Gateway, Slack Channel Adapter, Knowledge Service, Session Supervisor, Rules Engine (family 50 included, ADR-107) |
| `render` | Caller API (`GET /metrics`, answered with the returned bytes and media type) |
| `set_release` | server program, after `AgentLoader::load` |
| `trace_status` | server program, which maps it to the Agent Loader's own `TraceStatusView` for the start-up report (the Agent Loader has no Telemetry edge; the three values correspond one to one) |
| `traceparent` | Connection Binder (outbound HTTP header) |
| `SPAN_*` names | Session Supervisor (`ordin.run_stretch`, `ordin.model_turn`, `ordin.subagent_call`), MCP Front and Hook Relay (`ordin.tool_call`), Connection Binder (`ordin.http_out`) |

### Example

A run instance of release `2026.10.1`, after one run taken from a caller that paused on `email_supplier`, and one refusal by the configuration's argument rule `supplier-hosts`:

```
# HELP wrightrun_runs_started Runs a run pod took from the queue.
# TYPE wrightrun_runs_started counter
wrightrun_runs_started_total{started_by="caller"} 1
wrightrun_runs_started_total{started_by="schedule"} 0
wrightrun_runs_started_total{started_by="intake"} 0
wrightrun_runs_started_total{started_by="chat"} 0
# HELP wrightrun_tool_calls_paused_for_approval Tool calls paused because the tool is on the approval list.
# TYPE wrightrun_tool_calls_paused_for_approval counter
wrightrun_tool_calls_paused_for_approval_total{tool="email_supplier"} 1
# HELP wrightrun_tool_calls_refused Tool calls a gate or an argument rule refused.
# TYPE wrightrun_tool_calls_refused counter
wrightrun_tool_calls_refused_total{agent="support-agent",control="supplier-hosts",source="configuration"} 1
# HELP wrightrun_release_info The agent release this pod runs; the value is always 1.
# TYPE wrightrun_release_info gauge
wrightrun_release_info{release="2026.10.1",digest="sha256:9f2c1e7a40b3d85c6e1f0a2b4c6d8e0f11223344556677889900aabbccddeeff",runtime_version="1.0.0"} 1
# EOF
```

`render()` returns these bytes with `OPENMETRICS_CONTENT_TYPE`. A traced run `0192a1c2-3344-7556-8677-8899aabbccdd` produces a trace whose trace identifier is `0192a1c23344755686778899aabbccdd` (the UUID's 16 bytes in hexadecimal), holding one `ordin.run_stretch` span with `started_by="caller"`, `command="answer-ticket"`, `outcome="paused"`, and one `ordin.tool_call` span with `tool="email_supplier"`, `outcome="paused"`, `control="none"`.

## Internal Logic

`TelemetryConfig::from_env`:

1. **S-0.** Read `ORDIN_OTLP_ENDPOINT` with `std::env::var`. Absent or empty → `otlp_endpoint = None`. Otherwise parse with `url::Url::parse`; a parse failure, a scheme other than `https` or `http`, `http` with a host other than `localhost`, `127.0.0.1` or `::1`, or a URL holding a user name or password → `TelemetryError::EndpointInvalid` with a message that states the rule and never the value. No `OTEL_*` variable is read here or anywhere in this crate.

`init(config)`:

2. **S-1 Once.** A `std::sync::OnceLock<Telemetry>`; a second call → `AlreadyInitialised`.
3. **S-2 Metrics.** Create a fresh `prometheus_client::registry::Registry` (not a global one). Register families 1–50 in number order (1–28, 30–46, then 29, then 47–50), each a `Family<LabelSet, Counter>` (29: `Family<ReleaseLabels, Gauge>`; 49: `Family<ServerLabels, Histogram>` with the buckets of its table row), with its wrightrun `HELP` text and the name without `_total` (the library appends it on render). Then create, at value 0, every series whose labels are all closed: families 1, 2, 5 (4 each), 16 (2), 21–23 (2 each), 24 (8 valid kind/category pairs: `error` × {`tool-failed`, `missing-input`, `could-not-finish`, `other`}, `escalation` × {`needs-person`, `needs-approval`, `out-of-scope`, `other`}), 27 (6), 28 (16), 30 (5), 42 (2), 47 (3), 50 (6: `input` and `output` × `allow`, `block`, `unreachable`); families 48 and 49 take a declared server name and appear at their first event; families 3, 17 and 31 have no labels and render 0 from creation. A series whose label takes a declared name (connection, control, tool, duty, source, setting, route) appears at its first event.
4. **S-3 Traces.** `status = if !traces_switched_on { Off } else if otlp_endpoint.is_none() { SwitchedOnNoEndpoint } else { On }`. Only when `On`: build the OTLP span exporter over HTTP with protobuf, with the endpoint `config.otlp_endpoint` + path `/v1/traces` and the timeout `EXPORT_TIMEOUT`, using the pinned SDK's explicit builder settings so that no `OTEL_*` variable is consulted; the resource holds exactly `service.name = "ordin"`, `service.version = ordin_version` and `ordin.agent = agent`, built without the SDK's environment detectors. A build failure → `ExporterBuild` with the failing part named (`"exporter"` or `"provider"`), never the endpoint. Install this crate's `CountingSpanProcessor` (S-5) and an always-on sampler. The identifier generator gives each `ordin.run_stretch` span the trace identifier of its `run_id` field (the UUID's 16 bytes) and a random span identifier; spans opened inside it inherit the trace. If the pinned SDK refuses a chosen trace identifier, the run identifier remains the `run_id` attribute of the stretch span and the trace identifier is random (wrightrun ADR-076, its stated fallback).
5. **S-4 Allow list.** The `tracing_layer` exports a `tracing` span only when its name is one of the five `SPAN_*` names and records only the fields its table row lists; any other field recorded on such a span is dropped before export; any other span name is not exported. A field value longer than `MAX_LABEL_CHARS` is cut to that length. This is what keeps content out of a span even if a caller records more.
6. **S-5 CountingSpanProcessor.** `on_end(span)`: `try_send` into a `tokio::sync::mpsc` channel of capacity `SPAN_QUEUE`; a full channel → `spans_dropped{reason="queue-full"}` += 1 and the span is discarded; this never blocks the caller. A background task takes up to `SPAN_BATCH` spans, or what is there every `SPAN_FLUSH_EVERY`, and calls the exporter once per batch with `EXPORT_TIMEOUT`; an error or a timeout → `spans_dropped{reason="export-failed"}` += batch length, one `warn` event `spans_dropped` per minute at most with fields `reason`, `count`; a failed batch is not sent again (ADR-076: "never … repeats").
7. **S-6** Log one `info` event `telemetry_started` with fields `traces` (`off`, `on`, `switched_on_no_endpoint`), `families` (50). The endpoint address is never logged.

`record(event)`:

8. **M-1.** Match the variant to its family and labels; `get_or_create(&labels).inc()` (or `.inc_by(count)` for counted variants). `McpFrontCall` increments family 48 and observes `seconds` on family 49 (a negative or non-finite `seconds` observes 0). An atomic add; no lock beyond the family's map lookup; no await; it cannot fail.
9. **M-2.** A `String` label value longer than `MAX_LABEL_CHARS` is cut to `MAX_LABEL_CHARS` characters.
10. **M-3.** This crate does not check that a name was declared: the caller passes a declared name, or `UNKNOWN` when an outside value (an intake path, a tool name from a model) matched none (wrightrun MetricsEmitter, *Label values*). No label ever holds an identifier of a run, thread, job, channel, person, a path, an address, a text or an argument value.

`event_from_named(name, labels, by)`:

11a. **M-6.** Strip one trailing `_total` from `name`. Look the result up in a fixed table of families 1–46, 48 and 50 (one row per family: name, label keys in order, the variant to build); no row → `NameUnknown`. Family 29 (`set_release`), 47 (this crate's own) and 49 (observed with 48 through `McpFrontCall`) are not reachable by name. The label pairs must have exactly the row's keys, in any order → else `LabelInvalid`. A closed label's value must parse to its label enum from the kebab-case text of the metrics table → else `LabelInvalid`. A declared-name value is passed through (cut by M-2). Counted variants (`RunsInterrupted`, `RetentionDeleted`, `Erased`, `ChatQuestionsAbandoned`, `KnowledgeItemsWritten`) take `by`; every other variant is built once and the adapter calls `record` `by` times. Family 48's row keys are `server`, `outcome` and `seconds`; `seconds` is a decimal text read into `McpFrontCall.seconds` (unparsable → `LabelInvalid`). Pure function; no allocation beyond the returned strings.

`set_release(release_id, digest)`:

11. **M-4.** `release_id` 1–256 characters and `digest` (when given) at most 256 characters, else `ReleaseInvalid("release must have 1 to 256 characters and digest at most 256")`. Clear family 29, then set `{release, digest: digest.unwrap_or(NONE), runtime_version: ordin_version}` to 1, so one instance never shows two releases.

`render()`:

12. **M-5.** `prometheus_client::encoding::text::encode(&mut buf, &registry)` into a `String`; return its bytes and `OPENMETRICS_CONTENT_TYPE`. The Caller API's `GET /metrics` returns them with status 200 and no token (ADR-098 item 6). Sample names equal wrightrun's (`…_total`); the family lines follow the OpenMetrics text form (`# TYPE wrightrun_runs_started counter`, `# EOF`), which Prometheus reads *(assumed here: dashboards query sample names, which are unchanged)*.

`traceparent()`:

13. **T-1.** When status is `On` and a span is current: `00-<32 hex trace id>-<16 hex span id>-01`; otherwise `None`. No `tracestate` and no `baggage` is produced (wrightrun ADR-076).

`shutdown()`:

14. **T-2.** Close the channel; let the task send what is queued within `SHUTDOWN_FLUSH`; spans still queued at the deadline add to `spans_dropped{reason="shutdown"}`. Shut the provider down. Metrics stay readable until the process exits.

## Data Model

N/A — every metric lives in the memory of one instance and starts at 0 when it starts; Prometheus sums instances (wrightrun MetricsEmitter, *Where the numbers live*). Spans are held in memory until sent or dropped. No table, no file.

## Error Table

All errors are in-process `TelemetryError` values. None is retryable: the input is fixed for the life of the process.

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `ORDIN_OTLP_ENDPOINT` is not a URL, has another scheme, uses `http` to a host other than loopback, or holds a user name or password | `Err(permanent)`; the start stops, the instance never reports ready | `TEL_ENDPOINT_INVALID` | `error[TEL_ENDPOINT_INVALID]: must be an https address, or http to localhost, with no user name or password (ORDIN_OTLP_ENDPOINT)` |
| `init` called a second time in one process | `Err(permanent)` | `TEL_ALREADY_INITIALISED` | `error[TEL_ALREADY_INITIALISED]: telemetry is already set up for this process` |
| The OTLP exporter or the tracer provider cannot be built | `Err(permanent)`; the start stops | `TEL_EXPORTER_BUILD` | `error[TEL_EXPORTER_BUILD]: the trace exporter could not be built (exporter)` |
| `set_release` given an empty release, a release above 256 characters, or a digest above 256 characters | `Err(permanent)` | `TEL_RELEASE_INVALID` | `error[TEL_RELEASE_INVALID]: release must have 1 to 256 characters and digest at most 256` |
| The span queue is full | no error; span dropped | — | `wrightrun_spans_dropped_total{reason="queue-full"}` += 1 |
| An export request fails or times out | no error; batch dropped | — | `wrightrun_spans_dropped_total{reason="export-failed"}` += batch size; `warn` `spans_dropped` |
| `event_from_named` given a name outside the closed list | `Err(permanent)`; the adapter drops the increment and logs once per name | `TEL_NAME_UNKNOWN` | `error[TEL_NAME_UNKNOWN]: no metric family has this name` |
| `event_from_named` given missing, extra or out-of-set labels | `Err(permanent)`; increment dropped, logged once per name | `TEL_LABEL_INVALID` | `error[TEL_LABEL_INVALID]: the labels do not match the family's label set` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Telemetry

  Scenario: Happy path — every family exists under wrightrun's name
    Given init with traces switched off
    When render is called
    Then the text holds one "# TYPE" line for each of families 1 to 50
    And every sample line starts with "wrightrun_"
    And the content type is "application/openmetrics-text; version=1.0.0; charset=utf-8"

  Scenario: Happy path — closed series exist at zero before the first event
    Given a new process after init
    When render is called
    Then it holds wrightrun_runs_started_total{started_by="chat"} 0
    And wrightrun_safe_replies_total{reason="content-filter"} 0
    And wrightrun_content_checks_total{point="input",verdict="block"} 0
    And wrightrun_runs_ended_total{state="cancelled"} 0
    And wrightrun_fallback_runs_total 0
    And no sample line starts with "wrightrun_intake_events_refused_total{"

  Scenario: Happy path — erased memory items are counted
    When event_from_named("wrightrun_erased_items_total", [("kind","memory-items"),("request","scope")], 12) is recorded
    Then render shows wrightrun_erased_items_total{kind="memory-items",request="scope"} 12

  Scenario: Happy path — an event increases its counter
    Given render shows wrightrun_runs_queued_total{started_by="caller"} n
    When record(RunQueued{started_by: Caller}) is called twice and record(RunsInterrupted{count: 3}) once
    Then render shows wrightrun_runs_queued_total{started_by="caller"} n+2
    And wrightrun_runs_interrupted_total has increased by 3

  Scenario: Happy path — the release metric holds one series
    When set_release("2026.10.1", Some("sha256:9f2c…eeff")) and then set_release("2026.10.2", None) are called
    Then render holds exactly one wrightrun_release_info series, with release "2026.10.2" and digest "none"

  Scenario: Happy path — a traced stretch carries the run's identifier
    Given init with traces switched on and ORDIN_OTLP_ENDPOINT set to a collector stub
    When the Session Supervisor opens ordin.run_stretch with run_id 0192a1c2-3344-7556-8677-8899aabbccdd and a tool call span inside it
    Then the stub receives two spans with trace identifier 0192a1c23344755686778899aabbccdd
    And traceparent inside the tool call span returns "00-0192a1c23344755686778899aabbccdd-<16 hex>-01"

  Scenario: Edge case — content recorded on a span is not exported
    Given traces on
    When a caller records the field "arguments" with the text "card 4111 1111" on an ordin.tool_call span
    Then the exported span holds the fields tool, origin, outcome, control only
    And no exported attribute holds "4111"

  Scenario: Edge case — switched on with no endpoint
    Given traces switched on and ORDIN_OTLP_ENDPOINT unset
    When init is called
    Then trace_status returns SwitchedOnNoEndpoint
    And tracing_layer returns None
    And no network connection is opened

  Scenario: Edge case — no OTEL variable is read
    Given OTEL_EXPORTER_OTLP_ENDPOINT set to "https://elsewhere.example.org" and ORDIN_OTLP_ENDPOINT set to a collector stub
    When a traced stretch ends and the queue is flushed
    Then the stub receives the span and elsewhere.example.org receives nothing

  Scenario: Error path — a full queue drops and counts
    Given traces on and an exporter stub that never answers
    When 3000 spans end within one second
    Then wrightrun_spans_dropped_total{reason="queue-full"} is at least 952
    And no span end call waited for the exporter

  Scenario: Error path — a failed export is not repeated
    Given traces on and an exporter stub that answers 503
    When one batch of 10 spans is sent
    Then wrightrun_spans_dropped_total{reason="export-failed"} increases by 10
    And the stub receives that batch once

  Scenario: Error path — a malformed endpoint stops the start
    Given ORDIN_OTLP_ENDPOINT "http://collector.example.org:4318"
    When TelemetryConfig::from_env is called
    Then it returns TEL_ENDPOINT_INVALID
    And the error text does not contain "collector.example.org"

  Scenario: Happy path — the sink adapter maps a named counter to its family
    Given init with traces switched off
    When the Run Queue's sink calls add("wrightrun_runs_queued_total", [("started_by", "schedule")], 2)
    And the adapter records event_from_named of those values
    Then render shows wrightrun_runs_queued_total{started_by="schedule"} increased by 2

  Scenario: Happy path — an MCP Front call feeds families 48 and 49
    When record(McpFrontCall{server: "memory", outcome: Answered, seconds: 0.2}) is called
    Then render shows wrightrun_mcp_front_calls_total{server="memory",outcome="answered"} 1
    And wrightrun_mcp_front_call_seconds_bucket{server="memory",le="0.5"} 1

  Scenario: Error path — a name outside the closed list is refused
    When event_from_named("wrightrun_journal_events_appended_total", [], 1) is called
    Then it returns TEL_NAME_UNKNOWN

  Scenario: Error path — a label value outside the closed set is refused
    When event_from_named("wrightrun_runs_queued_total", [("started_by", "robot")], 1) is called
    Then it returns TEL_LABEL_INVALID

  Scenario: Error path — a second init
    Given init has succeeded
    When init is called again
    Then it returns TEL_ALREADY_INITIALISED
  Scenario: Happy path — a content check feeds family 50 by name (ADR-107)
    Given init has run
    When the sink adapter receives "wrightrun_content_checks_total" with point "output" and verdict "unreachable"
    Then event_from_named returns ContentCheck { point: output, verdict: unreachable }
    And render holds wrightrun_content_checks_total{point="output",verdict="unreachable"} 1
```

## Performance / Security / Observability

- **Performance targets:** `record` p95 under 2 µs and allocation-free for families with closed labels (an atomic add after a hashed lookup); `render` of up to 2,000 series under 10 ms p95 and under 1 MiB of output; span `on_end` p95 under 5 µs and never awaiting; memory for the span queue at most `SPAN_QUEUE` spans (≈ 2 MiB *assumed here* at about 1 KiB a span). Sending traces adds no latency to a run (`07:16`).
- **Security:** No entry point of its own: the metrics route belongs to the Caller API and carries no token (ADR-098 item 6), which is why no label holds content or an identifier of a person, run, thread or channel (M-3). Spans go only to `ORDIN_OTLP_ENDPOINT`, checked to be `https` or loopback; the allow list (S-4) keeps message text, arguments, file content, addresses and person identifiers out of every span; `traceparent` carries no baggage. The endpoint address and any collector credential are never logged. No `OTEL_*` variable is read (2D).
- **Observability:** Log events (`tracing`, JSON on standard output, SA-OBS-01): `telemetry_started` (`info`; fields `traces`, `families`); `spans_dropped` (`warn`, at most once a minute; fields `reason`, `count`). Metrics: family 47 counts every dropped span. Log `metric_name_refused` (`warn`; fields `code`, `name`) is written by the server program's sink adapter, once per name. Spans: this crate defines the five names and emits none of its own.

## Gaps

None.

## Amendment January 2026 — Knowledge agents (ADR-113)

A knowledge agent (an agent-profile server whose agent configuration has a `knowledge-agent` section) answers knowledge queries through the Knowledge Query Server and reads its Git repositories through the Knowledge Reader. Both crates have a Telemetry edge (Phase 2B) and call `record` directly; neither goes through the sink adapter. This amendment adds eight families, one span name and seven route names. Nothing else in this spec changes. The desktop and the personal server still record no metric (A-10).

### Purpose and approach (delta)

Families 51–58 are ordin's additions, numbered after 50, and keep the `wrightrun_` prefix of every family (SA-OBS-02, ADR-098 item 2), so an organisation's dashboards read one namespace. Labels stay closed and carry no content: no query text, no repository address, no path, no subject or group of an asker (M-3). The `proxy` label carries a proxy's configured `name` from the agent configuration's `knowledge-agent.proxies[]`, reduced to `unknown` by the caller when it is not a configured name, as declared names are for the other families.

### Public interface (delta)

Metrics table, rows added:

| # | Name | Labels (closed values) | Fed by |
|---|---|---|---|
| 51 | `wrightrun_knowledge_served_total` *(ordin, ADR-113)* | `via`: `http`, `mcp`; `outcome`: `answered`, `cached`, `partial`, `refused` | Knowledge Query Server |
| 52 | `wrightrun_knowledge_serve_seconds` (histogram) *(ordin, ADR-113)* | `via`; `cached`: `true`, `false`; buckets 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5 seconds | Knowledge Query Server (time to the first result) |
| 53 | `wrightrun_knowledge_proxy_asks_total` *(ordin, ADR-113)* | `proxy` (a configured proxy name, or `unknown`); `outcome`: `answered`, `not-in-time`, `refused`, `unreachable` | Knowledge Query Server |
| 54 | `wrightrun_knowledge_cache_dropped_total` *(ordin, ADR-113)* | `reason`: `moved`, `full`, `contents` | Knowledge Query Server |
| 55 | `wrightrun_knowledge_hot_copies_total` *(ordin, ADR-113)* | `action`: `requested`, `removed` | Knowledge Query Server |
| 56 | `wrightrun_knowledge_sources_record_deleted_total` *(ordin, ADR-113)* | — | Knowledge Query Server |
| 57 | `wrightrun_knowledge_reader_documents_total` *(ordin, ADR-113)* | `outcome`: `read`, `unread`, `failed`, `given-up`, `refused`, `not-text`, `too-large`, `not-utf8`, `too-long-for-model` (the `kr_documents.outcome` values, kebab-case) | Knowledge Reader |
| 58 | `wrightrun_knowledge_reader_cycles_total` *(ordin, ADR-113)* | `outcome`: `completed`, `stopped`, `failed` | Knowledge Reader |

`HELP` texts: 51 `Knowledge queries answered, by way in and outcome.`; 52 `Time from a knowledge query reaching this agent to its first result.`; 53 `Proxies asked for a knowledge query, by proxy and outcome.`; 54 `Cached knowledge answers dropped, by reason.`; 55 `Repositories a front door asked to read itself, or stopped reading.`; 56 `Sources record rows deleted after their retention period.`; 57 `Changed documents the knowledge reader tried to read, by outcome.`; 58 `Knowledge reading cycles, by outcome.`

`TelemetryEvent` gains, after `ContentCheck`:

```rust
    KnowledgeServed { via: KnowledgeVia, outcome: KnowledgeServeOutcome },              // 51
    KnowledgeServeSeconds { via: KnowledgeVia, cached: bool, seconds: f64 },           // 52
    KnowledgeProxyAsk { proxy: String, outcome: ProxyAskOutcome },                      // 53
    KnowledgeCacheDropped { reason: CacheDropReason, count: u64 },                      // 54
    KnowledgeHotCopy { action: HotCopyAction },                                         // 55
    KnowledgeSourcesRecordDeleted { count: u64 },                                       // 56
    KnowledgeReaderDocument { outcome: &'static str },                                  // 57; a kr_documents.outcome value
    KnowledgeReaderCycle { outcome: &'static str },                                     // 58; completed | stopped | failed
```

The label enums `KnowledgeVia` (`Http`, `Mcp`), `KnowledgeServeOutcome` (`Answered`, `Cached`, `Partial`, `Refused`), `ProxyAskOutcome` (`Answered`, `NotInTime`, `Refused`, `Unreachable`), `CacheDropReason` (`Moved`, `Full`, `Contents`) and `HotCopyAction` (`Requested`, `Removed`) follow the rule for every label enum above (derives, `EncodeLabelValue`, kebab-case values). Families 57 and 58 take the Knowledge Reader's `&'static str` values: `record` replaces `_` by `-` and counts a value outside the closed set of the table under `unknown`, logging `metric_label_unknown` (`warn`, once per value) — the reader's outcome strings are its own table's CHECK values, so the closed set and the table agree by construction. The existing `KnowledgeQuery { source, operation }` variant (wrightrun's knowledge sources, ADR-098 item 6) is unrelated and unchanged.

Span names, added:

```rust
pub const SPAN_KNOWLEDGE_QUERY: &str = "ordin.knowledge_query"; // Knowledge Query Server: one knowledge query, arrival to first result
```

| Span | Fields (attribute keys) | Values |
|---|---|---|
| `ordin.knowledge_query` | `from_cache`, `hits`, `unanswered`, `proxies_asked` | `true` or `false`; integers (counts only, never a fact, a query or an asker) |

Family 40 `wrightrun_http_requests_total` gains the Caller API's route names `knowledge.query`, `knowledge.contents`, `knowledge.contents.index`, `knowledge.about`, `knowledge.changes`, `knowledge.mcp` and `operator.knowledge` (CallerApi.md amendment January 2026, routes 34–40). Family 4 `wrightrun_schedule_firings_total` counts the built-in duty under `duty="knowledge_fetch"` (DutyTimer.md amendment January 2026); no new variant.

*Who calls each public item*, rows changed: `record` — also the Knowledge Query Server (families 51–56) and the Knowledge Reader (57, 58); `SPAN_*` names — also the Knowledge Query Server (`ordin.knowledge_query`).

### Internal logic (delta)

`init` registers families 51–58 with the others, so a render after `init` holds each closed series at zero before the first event (families 53, 57 and 58 hold no series until the first event, as `proxy` is declared and the reader's values arrive as text). The `tracing_layer` allow list gains `ordin.knowledge_query` with exactly its four fields (S-4). Nothing is stored.

### Error table (delta)

No new code. A family name outside 1–58 given to `event_from_named` still returns `TEL_NAME_UNKNOWN`; families 51–58 are not fed by name (their callers hold a Telemetry handle), so `event_from_named` refuses them with `TEL_NAME_UNKNOWN` as well.

### Acceptance criteria (Gherkin)

```gherkin
Feature: Telemetry for knowledge agents (ADR-113)

  Scenario: Happy path — the knowledge families exist under wrightrun's name
    Given init with traces switched off
    When render is called
    Then the text holds one "# TYPE" line for each of families 1 to 58
    And it holds wrightrun_knowledge_served_total{via="mcp",outcome="refused"} 0
    And it holds wrightrun_knowledge_cache_dropped_total{reason="moved"} 0

  Scenario: Happy path — a served query is counted and timed without content
    Given init has run
    When the Knowledge Query Server records KnowledgeServed { via: http, outcome: cached } and KnowledgeServeSeconds { via: http, cached: true, seconds: 0.012 }
    Then render holds wrightrun_knowledge_served_total{via="http",outcome="cached"} 1
    And wrightrun_knowledge_serve_seconds_bucket{via="http",cached="true",le="0.025"} 1
    And no label of either family holds query text, a repository address or an asker

  Scenario: Happy path — the reader's outcome strings map to kebab-case values
    Given init has run
    When the Knowledge Reader records KnowledgeReaderDocument { outcome: "too_long_for_model" }
    Then render holds wrightrun_knowledge_reader_documents_total{outcome="too-long-for-model"} 1

  Scenario: Error path — a reader outcome outside the closed set is counted as unknown
    Given init has run
    When KnowledgeReaderCycle { outcome: "paused" } is recorded
    Then render holds wrightrun_knowledge_reader_cycles_total{outcome="unknown"} 1
    And the log holds one metric_label_unknown line naming the family, not the run

  Scenario: Error path — the knowledge families are not fed by name
    When event_from_named("wrightrun_knowledge_served_total", [("via", "http"), ("outcome", "answered")], 1) is called
    Then it returns TEL_NAME_UNKNOWN

  Scenario: Happy path — the knowledge span exports only its four fields
    Given traces are switched on
    When the Knowledge Query Server closes an ordin.knowledge_query span carrying from_cache, hits, unanswered, proxies_asked and a field query
    Then the exported span holds from_cache, hits, unanswered and proxies_asked only
```

### Performance, security, observability (delta)

- **Performance:** the eight variants follow the `record` target (p95 under 2 µs; allocation-free except families 53, 57 and 58, whose label is looked up from text). Families 51–58 add at most 8 × 4 + 32 proxies × 4 + 9 + 3 series beside the 2,000 the render target already covers.
- **Security:** no family or span carries a query, a fact, a repository address, a path, an asker's subject or groups (ADR-113: queries are not logged with their text; counts and timings only).
- **Observability:** log `metric_label_unknown` (`warn`; fields `family`, `value`), once per value per process.

### Assumptions

- **SA-TEL-KA-01** Families 57 and 58 take the reader's text values and map them, rather than a typed enum in this crate, so the reader's `kr_documents.outcome` CHECK list stays the one place those values are listed; a value outside the table is counted as `unknown`. *Chosen here (January 2026, ADR-113).*
- **SA-TEL-KA-02** The `wrightrun_` prefix is kept for families 51–58, as for 47–50 (SA-OBS-02). *Chosen here (January 2026, ADR-113).*

### Gaps

None.

## Amendment January 2026 — Per-document read access (ADR-115)

> Source: decision 94 (the user's decisions of January 2026); HLD revision 0.63.0, ADR-115 (a knowledge agent checks each person against the system that holds a document at question time, remembers each answer for a short time in memory only, and accepts GitHub and GitLab sharing-change notices). Fed by: `components/SourceAccess.md` (new) and `components/KnowledgeQueryServer.md` (amendment January 2026).

### Purpose and approach (delta)

Families 59–64 are ordin's, numbered after 58, with the `wrightrun_` prefix (SA-OBS-02). Every label is closed: a system name or an outcome. **No label holds a person, an account, a repository, a page, a file or a unit**, so a scrape cannot tell who was refused what (ADR-115).

### Public interface (delta)

Metrics table, rows added:

| # | Name | Labels (closed values) | Fed by |
|---|---|---|---|
| 59 | `wrightrun_knowledge_access_checks_total` *(ordin, ADR-115)* | `system`: `github`, `gitlab`, `confluence`, `sharepoint`; `outcome`: `allowed`, `refused`, `unchecked` | Source Access (one per unit answered by a source system, not by the short memory) |
| 60 | `wrightrun_knowledge_access_check_seconds` (histogram) *(ordin, ADR-115)* | `system`; buckets 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1 seconds | Source Access (one per system batch of one question) |
| 61 | `wrightrun_knowledge_access_remembered_total` *(ordin, ADR-115)* | `system` | Source Access (one per unit answered from the short memory) |
| 62 | `wrightrun_knowledge_accounts_linked_total` *(ordin, ADR-115)* | `system`: `github`, `gitlab`, `confluence`; `outcome`: `linked`, `refused` | Source Access (one per account a person's ordin presented) |
| 63 | `wrightrun_knowledge_access_notices_total` *(ordin, ADR-115)* | `system`: `github`, `gitlab`; `outcome`: `applied`, `duplicate`, `ignored`, `refused` | Source Access |
| 64 | `wrightrun_knowledge_unchecked_answers_total` *(ordin, ADR-115)* | — | Knowledge Query Server (one per answer sent with `unchecked_sources = true`) |

`HELP` texts: 59 `Access checks answered by a document system, by system and outcome.`; 60 `Time a document system took to answer one question's access checks.`; 61 `Access checks answered from the short memory, by system.`; 62 `Accounts a person's ordin presented for linking, by system and outcome.`; 63 `Sharing-change notices received, by system and outcome.`; 64 `Knowledge answers sent with some sources that could not be checked.`

`TelemetryEvent` gains, after the families 51–58 variants:

```rust
    KnowledgeAccessCheck { system: SourceSystem, outcome: CheckOutcome },             // 59
    KnowledgeAccessCheckSeconds { system: SourceSystem, seconds: f64 },               // 60
    KnowledgeAccessRemembered { system: SourceSystem },                               // 61
    KnowledgeAccountLinked { system: SourceSystem, outcome: KnowledgeLinkOutcome },   // 62
    KnowledgeAccessNotice { system: SourceSystem, outcome: KnowledgeNoticeOutcome },  // 63
    KnowledgeUncheckedAnswer,                                                         // 64
```

with `SourceSystem` and `CheckOutcome` from `ordin-types` (2C wire §12, amendment January 2026) and two enums of this crate: `KnowledgeLinkOutcome { Linked, Refused }`, `KnowledgeNoticeOutcome { Applied, Duplicate, Ignored, Refused }` (kebab-case on render). `record` refuses family 62 with `system = sharepoint` and family 63 with a system other than `github` or `gitlab` as `LabelInvalid` (logged, not counted).

### Internal logic (delta)

- **S-2:** families 59–64 are registered after 58 in number order (59, 61–64 as `Family<LabelSet, Counter>`; 60 as `Family<KnowledgeSystemLabels, Histogram>` with the buckets above). Series created at value 0: 59 (12: four systems × three outcomes), 61 (4), 62 (6), 63 (8), 64 (no labels). Family 60 shows a series at its first observation.
- **M-6:** the by-name table gains rows for 59, 61, 62, 63 and 64, so a crate reaching Telemetry by name records them; family 60 is reachable by name with the keys `system` and `seconds` (decimal text, as family 48's `seconds`).
- **S-4:** the `tracing_layer` allow list for `ordin.knowledge_query` widens from four fields to seven: `from_cache`, `hits`, `unanswered`, `proxies_asked`, `checked`, `withheld`, `pending` (the last three counts, opened by the Knowledge Query Server; Tracing.md amendment January 2026).
- Nothing is stored; the desktop and the personal server still expose no metric (A-10).

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
Feature: Metrics for per-document read access (ADR-115)

  Scenario: Happy path — the scrape lists families 1 to 64
    Given a knowledge agent after start
    When GET /metrics is sent with no token
    Then the body holds one "# TYPE" line for each of families 1 to 64
    And wrightrun_knowledge_access_checks_total{system="confluence",outcome="refused"} is 0

  Scenario: Happy path — a check and a remembered check are counted apart
    When Source Access records KnowledgeAccessCheck { system: github, outcome: allowed } and KnowledgeAccessRemembered { system: github }
    Then render holds wrightrun_knowledge_access_checks_total{system="github",outcome="allowed"} 1
    And wrightrun_knowledge_access_remembered_total{system="github"} 1

  Scenario: Error — a SharePoint link outcome is refused
    When KnowledgeAccountLinked { system: sharepoint, outcome: linked } is recorded
    Then the record is refused with LabelInvalid and no series changes

  Scenario: Security — no label names a person or a document
    Given 100 checks recorded for 10 people over 30 units
    When GET /metrics is sent
    Then no series of families 59 to 64 has a label other than system and outcome
```
