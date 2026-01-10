# SPEC: Metrics

> **Spec set:** ordin-desktop · **Phase:** 4 — Cross-cutting · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-107; promoted to Approved by the user January 2026) · amended January 2026 (ADR-113; promoted to Approved by the user January 2026) · amended January 2026 (ADR-115) · **Mode:** greenfield · **derivedFromHld:** 0.63.0
> **Implemented in:** `ordin-telemetry` (Telemetry component spec) — the registry, the closed family list and the OpenMetrics rendering; served by `ordin-caller-api` route 30 `GET /metrics`; agent profile only.

## Purpose

States where metrics exist in ordin and the rules every metric obeys. The desktop and the personal server export no metric (A-10; HLD 07 — *Metrics*: "Does not apply in the first release"); run and crew counters are shown in the interface and the journal instead. A hosted agent (agent profile) exposes Prometheus counters on every instance with wrightrun's counter names carried across unchanged, so an organisation moving from wrightrun keeps its dashboards and alerts (07, SA-OBS-02). This spec copies the closed family list from the Telemetry component spec so that every other spec has one place to check a metric name against, and fixes label, cardinality and exposition rules.

## Approach

One registry of ordin's own in `ordin-telemetry`, with every family created at start from a closed list; components increment through `Telemetry::record(TelemetryEvent)` (typed labels) or, when they have no Telemetry edge, through a `MetricsSink` the server program adapts with `event_from_named`. A name outside the closed list does not exist. Rejected: the `prometheus-client` default registry (it would expose whatever another crate registers); letting each component declare its own families (names and label sets would drift from wrightrun's, which breaks deployments' dashboards).

## Applicability by program

| Program | Applies | How | Source |
|---|---|---|---|
| Desktop (D) | No | No metrics route and no export; `ordin-telemetry` is not linked (2B-3: Telemetry is A only). Counts the person sees come from the Journal and the screens. | SA-XC-04, A-10 |
| Personal server (P) | No | Same as the desktop: no route, no export; health is the systemd watchdog (Health Checks spec). | SA-XC-04, A-10 |
| Agent profile (A) | Yes | `GET /metrics` on `ORDIN_PORT` in every role (`build_router` for the `requests` role and the single shape, `health_router` for `runs`, `chat`, `jobs`), no token, OpenMetrics text. | SA-XC-04, SA-OBS-02; Caller API route 30; ADR-098 item 6 |

## Shared Context

**Libraries (1A SA-OBS-02).** `prometheus-client` 0.25.1 (`Apache-2.0 OR MIT`).

**Environment (2D-1a, copied).**

| Variable | Type | Default | Required | Owner | Description |
|---|---|---|---|---|---|
| `ORDIN_PORT` | integer 1024–65535 | `8080` | No | ordin server | The one port for caller, intake, operator, work-folder, health and metrics routes (agent profile only). |

**Constants (Telemetry spec, copied).**

```rust
pub const METRIC_PREFIX: &str = "wrightrun_";   // SA-OBS-02: wrightrun's names unchanged; ordin's additions take it too
pub const UNKNOWN: &str = "unknown";            // a name no declaration holds
pub const NONE: &str = "none";                  // an absent digest
pub const MAX_LABEL_CHARS: usize = 128;         // wrightrun MetricsEmitter, Naming
pub const OPENMETRICS_CONTENT_TYPE: &str = "application/openmetrics-text; version=1.0.0; charset=utf-8";
```

## Public Interface

```rust
// crate ordin-telemetry (Telemetry spec, copied)
impl Telemetry {
    pub fn record(&self, event: TelemetryEvent);               // never fails, never awaits
    pub fn render(&self) -> (Vec<u8>, &'static str);           // body and OPENMETRICS_CONTENT_TYPE
    pub fn set_release(&self, release_id: &str, digest: Option<&str>) -> Result<(), TelemetryError>;
}
pub fn event_from_named(name: &str, labels: &[(&str, &str)], by: u64) -> Result<TelemetryEvent, TelemetryError>;

// A component without a Telemetry edge declares its own sink and the server program implements it
// with event_from_named + record (Run Queue, Settings Store, Retention and Erasure, Channel Gateway,
// Slack Channel Adapter, Knowledge Service, Session Supervisor, Rules Engine, MCP Front).
pub trait MetricsSink: Send + Sync { fn add(&self, name: &str, labels: &[(&str, &str)], by: u64); }
```

**HTTP.** `GET /metrics` (Caller API route 30): no `Authorization`; 200 with the bytes of `render()` and `Content-Type: application/openmetrics-text; version=1.0.0; charset=utf-8`. Wrong method → 405 `SRV_METHOD_NOT_ALLOWED` (Caller API).

### The closed family list (copied from the Telemetry spec)

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
| 37 | `wrightrun_ingest_thread_failures_total` | `source` | Knowledge Service (sink), in the same ingest step |
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

The `HELP` text of families 1–46 is wrightrun's *Documentation texts* table, verbatim; family 47 reads `Spans not sent to the collector.`; 48 `Tool calls the MCP Front answered, by server and outcome.`; 49 `Time from a tool call reaching the MCP Front to its answer.`; 50 `Content checks ordin asked for, by point and verdict.` *(amended January 2026, ADR-107)*

### Example

```
# HELP wrightrun_runs_started Runs a run pod took from the queue.
# TYPE wrightrun_runs_started counter
wrightrun_runs_started_total{started_by="caller"} 1
wrightrun_runs_started_total{started_by="schedule"} 0
wrightrun_runs_started_total{started_by="intake"} 0
wrightrun_runs_started_total{started_by="chat"} 0
# HELP wrightrun_release_info The agent release this pod runs; the value is always 1.
# TYPE wrightrun_release_info gauge
wrightrun_release_info{release="2026.10.1",digest="sha256:9f2c1e7a40b3d85c6e1f0a2b4c6d8e0f11223344556677889900aabbccddeeff",runtime_version="1.0.0"} 1
# EOF
```

## Internal Logic

1. **N-1 Names.** Every family name is `wrightrun_<snake_case>`; counters render with `_total`; the one gauge is `wrightrun_release_info`; the one histogram is `wrightrun_mcp_front_call_seconds`. A new family is added only by amending the Telemetry spec's table and this copy together, with the `wrightrun_` prefix (SA-OBS-02). A family named in any other spec and absent from the table does not exist: the Telemetry `event_from_named` refuses it with `TEL_NAME_UNKNOWN`, and the server program's sink adapter logs `metric_name_refused` (`WARNING`; `code`, `name`) once per name and drops the increment.
2. **N-2 Labels.** Label keys are fixed per family (table). A closed label takes only the values listed; a declared-name label (`connection`, `control`, `tool`, `duty`, `source`, `setting`, `route`, `server`) takes a name the agent configuration or the release declares, or `unknown` when an outside value (an intake path, a model's tool name) matched none. A label value longer than `MAX_LABEL_CHARS` is cut to that length.
3. **N-3 Cardinality.** No label ever holds an identifier of a run, thread, job, channel, chat, person, a path, an address, a text, an argument value or an error message (Telemetry M-3); `reason` and `code` labels hold registry codes only. The series count of one instance is therefore bounded by the names its release declares, plus one `unknown` series per declared-name label; `render` is sized for 2,000 series (Telemetry spec, *Performance targets*).
4. **N-4 Zero series.** Families whose labels are all closed are created at value 0 at start (Telemetry S-2), so a rate query has a series before the first event; declared-name series appear at their first event.
5. **N-5 Exposition.** `render()` writes OpenMetrics text with `# EOF`; sample names equal wrightrun's (`…_total`). Prometheus sums instances; every value lives in one instance's memory and restarts at 0 (counter resets are normal Prometheus semantics).
6. **N-6 Histogram buckets.** Only family 49 is a histogram: 0.01, 0.05, 0.1, 0.5, 1, 5, 30, 120, 300 seconds (Telemetry table). A negative or non-finite observation observes 0.
7. **N-7 Desktop and personal server.** `ordin-telemetry` is not linked into the desktop program and is not constructed in the personal profile; components that accept a `MetricsSink` or `McpMetrics` receive `None` there and record nothing.

8. **N-8 Conformance of component specs (checked January 2026; resolved).** Every `wrightrun_` name in `components/*.md` is in the closed list. Ten families once named in component specs with the wrong prefix `ordin_` and absent from Telemetry's list (Approval Service `ordin_approvals_requested_total`, `ordin_approvals_decided_total{state}`; File Service `ordin_work_folder_writes_total{result}`, `ordin_work_folder_refusals_total{reason}`; Hook Relay `ordin_hook_calls_total{verdict}`; Claude Local Adapter `ordin_harness_sessions_started_total{adapter}`; Usage Planner `ordin_model_turns_total{level}`; Memory Service `ordin_memory_writes_total{outcome}`, `ordin_memory_recalls_total{abstained}`, `ordin_memory_recall_seconds`) were removed from those six specs in the correction round of January 2026; each now states that it records no metric of its own and names the closed-list family that covers its events, where one does. The closed list stayed at 49 families (50 from January 2026, ADR-107, the Rules Engine's content checks) (the cross-cutting rule wins: Sync mode, Component vs Cross-cutting).

## Data Model

N/A — metrics are held in one instance's memory; no table, no file.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A sink names a family outside the closed list | `Err(permanent)`; increment dropped, logged once per name | `TEL_NAME_UNKNOWN` | `error[TEL_NAME_UNKNOWN]: no metric family has this name` |
| A sink passes missing, extra or out-of-set labels | `Err(permanent)`; increment dropped, logged once per name | `TEL_LABEL_INVALID` | `error[TEL_LABEL_INVALID]: the labels do not match the family's label set` |
| `set_release` given an empty or over-long release or digest | `Err(permanent)` | `TEL_RELEASE_INVALID` | `error[TEL_RELEASE_INVALID]: release must have 1 to 256 characters and digest at most 256` |
| `POST /metrics` (any method but `GET`) | 405 | `SRV_METHOD_NOT_ALLOWED` | `{"error":"…","code":"SRV_METHOD_NOT_ALLOWED"}` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Metrics

  Scenario: Happy path — a scrape needs no token and returns OpenMetrics
    Given an agent-profile instance in the runs role has started one run from a caller
    When GET /metrics is sent with no Authorization header
    Then the status is 200 with Content-Type "application/openmetrics-text; version=1.0.0; charset=utf-8"
    And the body holds wrightrun_runs_started_total{started_by="caller"} 1 and ends with "# EOF"

  Scenario: Edge case — an undeclared tool name is recorded as unknown
    Given a model calls a tool "drop_tables" that the release does not declare
    When the call is paused for approval
    Then the series wrightrun_tool_calls_paused_for_approval_total{tool="unknown"} increases by 1
    And no series holds "drop_tables"

  Scenario: Edge case — the desktop has no metrics
    Given the desktop program is running
    Then no component holds a Telemetry value and no metrics route exists

  Scenario: Error path — a family outside the closed list is refused
    When a sink adds 1 to "wrightrun_journal_events_appended_total"
    Then event_from_named returns TEL_NAME_UNKNOWN
    And one WARNING line metric_name_refused is written for that name
```

## Performance, Security, Observability

- **Performance targets:** `record` under 2 µs p95; `render` of 2,000 series under 10 ms p95 and under 1 MiB (Telemetry spec).
- **Security:** the metrics route carries no token (ADR-098 item 6), so label rule N-3 is the confidentiality control: no content and no identifier of a person, run, thread or channel. The route is served on the instance port only; exposure beyond the cluster is the deployment's network policy.
- **Observability:** this spec defines the metrics channel. `metric_name_refused` (`WARNING`) is the one log line about metrics; Telemetry's own `spans_dropped` line belongs to the Tracing spec.

## Gaps

None.

## Amendment January 2026 — Knowledge agents (ADR-113)

The closed family list gains families 51–58, copied from the Telemetry spec's amendment of January 2026, with the same names, labels and `HELP` texts:

| # | Name | Labels (closed values) | Fed by |
|---|---|---|---|
| 51 | `wrightrun_knowledge_served_total` *(ordin, ADR-113)* | `via`: `http`, `mcp`; `outcome`: `answered`, `cached`, `partial`, `refused` | Knowledge Query Server |
| 52 | `wrightrun_knowledge_serve_seconds` (histogram) *(ordin, ADR-113)* | `via`; `cached`: `true`, `false`; buckets 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5 seconds | Knowledge Query Server |
| 53 | `wrightrun_knowledge_proxy_asks_total` *(ordin, ADR-113)* | `proxy` (a configured proxy name, or `unknown`); `outcome`: `answered`, `not-in-time`, `refused`, `unreachable` | Knowledge Query Server |
| 54 | `wrightrun_knowledge_cache_dropped_total` *(ordin, ADR-113)* | `reason`: `moved`, `full`, `contents` | Knowledge Query Server |
| 55 | `wrightrun_knowledge_hot_copies_total` *(ordin, ADR-113)* | `action`: `requested`, `removed` | Knowledge Query Server |
| 56 | `wrightrun_knowledge_sources_record_deleted_total` *(ordin, ADR-113)* | — | Knowledge Query Server |
| 57 | `wrightrun_knowledge_reader_documents_total` *(ordin, ADR-113)* | `outcome`: `read`, `unread`, `failed`, `given-up`, `refused`, `not-text`, `too-large`, `not-utf8`, `too-long-for-model` | Knowledge Reader |
| 58 | `wrightrun_knowledge_reader_cycles_total` *(ordin, ADR-113)* | `outcome`: `completed`, `stopped`, `failed` | Knowledge Reader |

Family 40 gains the route names `knowledge.query`, `knowledge.contents`, `knowledge.contents.index`, `knowledge.about`, `knowledge.changes`, `knowledge.mcp` and `operator.knowledge`; family 4 counts the built-in duty as `duty="knowledge_fetch"`. The desktop and the personal server still expose no metric: the Knowledge Query Client records none (A-10). No label holds a query, a fact, a repository address, a path or an asker.

```gherkin
Feature: Metrics for knowledge agents (ADR-113)

  Scenario: The scrape lists families 1 to 58
    Given a knowledge agent after start
    When GET /metrics is sent with no token
    Then the body holds one "# TYPE" line for each of families 1 to 58

  Scenario: A desktop connected to a front door exposes no metric
    Given a desktop workspace connected to one front door
    When the workspace asks the front door a question
    Then no metric family is registered or exposed on the desktop
```

### Gaps

None.

## Amendment January 2026 — Per-document read access (ADR-115)

The closed family list gains families 59–64, copied from the Telemetry spec's amendment of January 2026, with the same names, labels and `HELP` texts:

| # | Name | Labels (closed values) | Fed by |
|---|---|---|---|
| 59 | `wrightrun_knowledge_access_checks_total` *(ordin, ADR-115)* | `system`: `github`, `gitlab`, `confluence`, `sharepoint`; `outcome`: `allowed`, `refused`, `unchecked` | Source Access |
| 60 | `wrightrun_knowledge_access_check_seconds` (histogram) *(ordin, ADR-115)* | `system`; buckets 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1 seconds | Source Access |
| 61 | `wrightrun_knowledge_access_remembered_total` *(ordin, ADR-115)* | `system` | Source Access |
| 62 | `wrightrun_knowledge_accounts_linked_total` *(ordin, ADR-115)* | `system`: `github`, `gitlab`, `confluence`; `outcome`: `linked`, `refused` | Source Access |
| 63 | `wrightrun_knowledge_access_notices_total` *(ordin, ADR-115)* | `system`: `github`, `gitlab`; `outcome`: `applied`, `duplicate`, `ignored`, `refused` | Source Access |
| 64 | `wrightrun_knowledge_unchecked_answers_total` *(ordin, ADR-115)* | — | Knowledge Query Server |

Family 40 gains the route names `knowledge.link_accounts`, `knowledge.unlink_account`, `knowledge.notice_github`, `knowledge.notice_gitlab`, `knowledge.forget_checks`. No label holds a person, an account, a repository, a page, a file or an access unit.

```gherkin
Feature: Metrics for per-document read access (ADR-115)

  Scenario: The scrape lists families 1 to 64
    Given a knowledge agent after start
    When GET /metrics is sent with no token
    Then the body holds one "# TYPE" line for each of families 1 to 64
```

### Gaps

None.
