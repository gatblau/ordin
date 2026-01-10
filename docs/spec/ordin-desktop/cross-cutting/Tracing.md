# SPEC: Tracing

> **Spec set:** ordin-desktop · **Phase:** 4 — Cross-cutting · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-113; promoted to Approved by the user January 2026) · amended January 2026 (ADR-115) · **Mode:** greenfield · **derivedFromHld:** 0.63.0
> **Implemented in:** `ordin-telemetry` (Telemetry component spec) — span allow list, OTLP exporter, counting span queue, `traceparent`; spans are opened with `tracing::info_span!` by the Session Supervisor, MCP Front, Hook Relay and Connection Binder; installed by `apps/server` (agent profile only).

## Purpose

States where distributed traces exist and the rules every exported span obeys. The desktop and the personal server export no trace (A-10; HLD 07 — *Tracing*: the journal links runs, rounds and tool requests by identifier). A hosted agent sends OpenTelemetry spans per stretch of a run to the organisation's collector only when switched on; a span holds no content, address or identifier of a person; a span that cannot be sent is dropped and counted, and sending never delays, repeats or fails a run (07, ADR-098 item 6). This spec enumerates every span name the component specs use, fixes which ones are exported, and states the exporter configuration, sampling and context propagation.

## Approach

Spans are ordinary `tracing` spans; the Telemetry crate's `tracing_layer` turns the five allow-listed names into OpenTelemetry spans and drops everything else, so components need no OpenTelemetry dependency and a span opened for local logging can never leak to the collector. Export is OTLP over HTTP with protobuf through a bounded queue of ordin's own that counts drops. Rejected: the OpenTelemetry SDK's batch processor (drops a full queue without a count the operator can read); OTLP over gRPC (HTTP passes company proxies, SA-OBS-03); reading `OTEL_*` variables (two configuration sources for one setting, which 2D forbids).

## Applicability by program

| Program | Applies | How | Source |
|---|---|---|---|
| Desktop (D) | No | No exporter; `ordin-telemetry` not linked. Spans the components open (for example `ordin.run_stretch`) bind log identifiers only (Logging spec, L-3). | SA-XC-05, A-10 |
| Personal server (P) | No | Same as the desktop. | SA-XC-05, A-10 |
| Agent profile (A) | Yes, off by default | Exported only when the agent configuration sets `traces: true` **and** `ORDIN_OTLP_ENDPOINT` is set; otherwise the start-up report says traces are off or not sent. | SA-XC-05, SA-OBS-03; Telemetry S-3 |

## Shared Context

**Libraries (1A SA-OBS-03).** `opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp` 0.33.1 (Apache-2.0, HTTP-protobuf transport), `tracing-opentelemetry` 0.34.0 (MIT); upgraded together.

**Environment (2D-1a, copied).**

| Variable | Type | Default | Required | Owner | Description |
|---|---|---|---|---|---|
| `ORDIN_OTLP_ENDPOINT` | URL (`https://`, or `http://` to `localhost`, `127.0.0.1`, `::1`) | none | No | Telemetry | The organisation's OpenTelemetry collector. No user name or password in the URL. ordin sends to `<endpoint>/v1/traces`. |

**Agent configuration (Agent Configuration spec, copied).** `AgentConfig.traces: bool`, default `false`, file key `traces`.

**Constants (Telemetry spec, copied).**

```rust
pub const SPAN_QUEUE: usize = 2_048;                                  // spans waiting to be sent
pub const SPAN_BATCH: usize = 512;                                    // spans per export request
pub const SPAN_FLUSH_EVERY: Duration = Duration::from_secs(5);
pub const EXPORT_TIMEOUT: Duration = Duration::from_secs(10);
pub const SHUTDOWN_FLUSH: Duration = Duration::from_secs(5);
pub const MAX_LABEL_CHARS: usize = 128;                               // also the cut length of a span field
```

## Public Interface

```rust
// crate ordin-telemetry (Telemetry spec, copied)
pub enum TraceStatus { Off, On, SwitchedOnNoEndpoint }
impl Telemetry {
    pub fn trace_status(&self) -> TraceStatus;
    /// A `tracing` layer exporting the allow-listed span names; None unless TraceStatus::On.
    pub fn tracing_layer<S>(&self) -> Option<Box<dyn tracing_subscriber::Layer<S> + Send + Sync>>
    where S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>;
    /// W3C traceparent of the current span; None when traces are not on or no span is current.
    pub fn traceparent(&self) -> Option<String>;
    pub async fn shutdown(&self);
}
pub const SPAN_RUN_STRETCH: &str = "ordin.run_stretch";
pub const SPAN_MODEL_TURN: &str = "ordin.model_turn";
pub const SPAN_TOOL_CALL: &str = "ordin.tool_call";
pub const SPAN_SUBAGENT_CALL: &str = "ordin.subagent_call";
pub const SPAN_HTTP_OUT: &str = "ordin.http_out";
```

### Exported spans (the allow list, copied from the Telemetry spec)

| Span | Opened by | Fields (attribute keys) | Values |
|---|---|---|---|
| `ordin.run_stretch` | Session Supervisor: one stretch of a run | `run_id`, `started_by`, `command`, `outcome` | run UUID; `caller`, `schedule`, `intake`, `chat`; a command slug or `none`; `finished`, `paused`, `ended-by-control`, `ended-at-limit`, `failed`, `cancelled`, `interrupted` |
| `ordin.model_turn` | Session Supervisor: one assistant turn of the harness stream | `model`, `input_tokens`, `output_tokens`, `outcome` | the `provider:model` text of the configuration; integers; `answered`, `failed`, `blocked` |
| `ordin.tool_call` | MCP Front (ordin's MCP servers, `origin = "ordin_mcp_front"`) and Hook Relay (built-in tools, `origin = "built_in"`); package MCP servers `origin = "package_mcp_server"` | `tool`, `origin`, `outcome`, `control` | a declared tool name or `unknown`; `built_in`, `ordin_mcp_front`, `package_mcp_server`; `allowed`, `refused`, `paused`, `failed`; a control name or `none` |
| `ordin.subagent_call` | Session Supervisor: one call of a sub-agent | `subagent`, `outcome` | `general-purpose` or the reviewer's name; `finished`, `stopped` |
| `ordin.http_out` | Connection Binder: one outbound HTTP call of a connection | `connection`, `kind`, `status` | a connection name; `http`, `webhook`, `mcp`; the HTTP status as text, or `timeout`, `failed` |

Every exported span also carries its duration and the SDK's span status. The resource holds exactly `service.name = "ordin"`, `service.version = <ordin version>`, `ordin.agent = <package name>`.

### Span names used across the component specs (enumerated January 2026)

| Name | Spec that names it | Exported | Note |
|---|---|---|---|
| `ordin.run_stretch`, `ordin.model_turn`, `ordin.subagent_call` | Telemetry, Session Supervisor (*Observability*) | Yes | T-D1 resolved: SessionSupervisor.md states that it opens them |
| `ordin.tool_call` | Telemetry, MCP Front (step 21), Hook Relay (*Observability*, built-in tools) | Yes | T-D1 resolved: HookRelay.md states that it opens it for built-in tools |
| `ordin.http_out` | Telemetry, Connection Binder | Yes | — |
| `checker.check` | Checker (*Observability*, agent profile) | No | local `tracing` span only, as Checker.md now states (T-D2 resolved) |
| `resolver.build` | Resolver and Compiler (*Observability*, agent profile) | No | local `tracing` span only, as ResolverAndCompiler.md now states (T-D2 resolved) |
| `memory.recall` | Memory Service (*Observability*, agent profile) | No | local `tracing` span only, as MemoryService.md now states (T-D2 resolved) |
| `claude.session` | Claude Local Adapter (*Observability*, agent profile) | No | local `tracing` span only, as ClaudeLocalAdapter.md now states (T-D2 resolved) |
| `work_folder.<operation>` | File Service (*Observability*, agent profile) | No | local `tracing` span only, as FileService.md states |

### Example

Trace of run `0192a1c2-3344-7556-8677-8899aabbccdd` (trace identifier `0192a1c23344755686778899aabbccdd`, the UUID's 16 bytes): one `ordin.run_stretch` {`started_by`="caller", `command`="answer-ticket", `outcome`="paused"} holding one `ordin.model_turn` {`model`="anthropic:claude-sonnet-4-6", `input_tokens`=8120, `output_tokens`=412, `outcome`="answered"} and one `ordin.tool_call` {`tool`="email_supplier", `origin`="built_in", `outcome`="paused", `control`="none"}. An outbound call from that run carries `traceparent: 00-0192a1c23344755686778899aabbccdd-<16 hex span id>-01`.

## Internal Logic

1. **T-1 Switch.** At start the server program builds `TelemetryConfig::from_env(version, package_name, config.traces)`. Status: `Off` when `traces` is false; `SwitchedOnNoEndpoint` when true and `ORDIN_OTLP_ENDPOINT` is unset; `On` otherwise. A malformed endpoint stops the start with `TEL_ENDPOINT_INVALID` and the instance never reports ready. The Agent Loader's start-up report prints `traces: off`, `traces: on` or `traces: switched on, not sent — ORDIN_OTLP_ENDPOINT is not set`.
2. **T-2 Exporter.** Only when `On`: OTLP span exporter over HTTP with protobuf to `<endpoint>/v1/traces`, timeout `EXPORT_TIMEOUT`, built with explicit builder settings so no `OTEL_*` variable is read and no environment resource detector runs. Outbound TLS uses the platform verifier and the system proxy (SA-HTTP-02, A-39). A build failure stops the start with `TEL_EXPORTER_BUILD`.
3. **T-3 Sampling.** Always on: every span of an allow-listed name is recorded. The volume bound is the queue (T-5), not a sampler: at most `SPAN_QUEUE` spans wait, and an overflow is dropped and counted.
4. **T-4 Allow list.** The layer exports a span only when its name is one of the five; it records only the fields its row lists, cutting each value at `MAX_LABEL_CHARS`; any other field is dropped before export. This keeps message text, arguments, file content, addresses and person identifiers out of every span even if a caller records them.
5. **T-5 Queue and drops.** `on_end` puts the span in a channel of `SPAN_QUEUE`; full → `wrightrun_spans_dropped_total{reason="queue-full"}` += 1, never blocking. A background task exports up to `SPAN_BATCH` spans, or what is waiting every `SPAN_FLUSH_EVERY`; an error or timeout → `{reason="export-failed"}` += batch size and a `WARNING` line `spans_dropped` (`reason`, `count`) at most once a minute. A failed batch is not sent again.
6. **T-6 Trace identity.** The identifier generator gives each `ordin.run_stretch` the trace identifier of its `run_id` (the UUID's 16 bytes); spans opened inside it inherit the trace; a resumed run's next stretch therefore joins the same trace. If the pinned SDK refuses a chosen trace identifier, the trace identifier is random and `run_id` stays an attribute of the stretch span (Telemetry S-3).
7. **T-7 Propagation.** Outbound: the Connection Binder adds `traceparent` (`00-<trace>-<span>-01`, from `Telemetry::traceparent()`) to each outbound HTTP request of a connection when traces are on; no `tracestate` and no `baggage` is sent. Inbound: the Caller API does not read `traceparent` from callers; a run's trace starts at its stretch span. Inside the program: a task spawned for a run is instrumented with the current span (Logging spec, L-3).
8. **T-8 Shutdown.** `shutdown()` closes the queue, sends what waits within `SHUTDOWN_FLUSH`, counts the rest as `{reason="shutdown"}` and shuts the provider down (Graceful Shutdown spec, agent profile).
9. **T-9 Conformance.** A component spec that names a span outside the allow list states a local `tracing` span at most; it is not exported. Defects T-D1 and T-D2 (table above) were resolved in the correction round of January 2026: the openers of the exported spans state them, and every other named span is stated as local only; the cross-cutting allow list wins.

## Data Model

N/A — spans are held in memory until sent or dropped; no table, no file.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `ORDIN_OTLP_ENDPOINT` malformed, another scheme, `http` to a non-loopback host, or holding a user name or password | `Err(permanent)`; start stops, never ready | `TEL_ENDPOINT_INVALID` | `error[TEL_ENDPOINT_INVALID]: must be an https address, or http to localhost, with no user name or password (ORDIN_OTLP_ENDPOINT)` |
| Exporter or tracer provider cannot be built | `Err(permanent)`; start stops | `TEL_EXPORTER_BUILD` | `error[TEL_EXPORTER_BUILD]: the trace exporter could not be built (exporter)` |
| `init` called twice in one program | `Err(permanent)` | `TEL_ALREADY_INITIALISED` | `error[TEL_ALREADY_INITIALISED]: telemetry is already set up for this process` |
| Span queue full | no error; span dropped | — | `wrightrun_spans_dropped_total{reason="queue-full"}` += 1 |
| Export fails or times out | no error; batch dropped | — | `wrightrun_spans_dropped_total{reason="export-failed"}` += batch size; `WARNING` `spans_dropped` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Tracing

  Scenario: Happy path — a traced run's stretch is exported with its run as trace identifier
    Given an agent configuration with traces: true and ORDIN_OTLP_ENDPOINT "https://otel.example.org"
    When run "0192a1c2-3344-7556-8677-8899aabbccdd" pauses on tool "email_supplier"
    Then the collector receives a span "ordin.run_stretch" with trace identifier "0192a1c23344755686778899aabbccdd" and outcome "paused"
    And a span "ordin.tool_call" with tool "email_supplier" and outcome "paused" in the same trace

  Scenario: Edge case — switched on without a collector
    Given traces: true and ORDIN_OTLP_ENDPOINT unset
    When the instance starts
    Then the start-up report holds "traces: switched on, not sent — ORDIN_OTLP_ENDPOINT is not set"
    And no export request is made

  Scenario: Edge case — a span outside the allow list is not exported
    Given traces are on
    When the Checker opens a span "checker.check"
    Then the collector receives no span named "checker.check"

  Scenario: Edge case — a recorded content field is dropped
    Given traces are on
    When a caller records field "query" with "Q3 invoices" on "ordin.tool_call"
    Then the exported span has no attribute "query"

  Scenario: Error path — the collector is down
    Given traces are on and the collector refuses every request
    When 600 spans end
    Then wrightrun_spans_dropped_total{reason="export-failed"} increases by 600
    And every run finishes as it would with traces off

  Scenario: Error path — a plain-http endpoint to a remote host stops the start
    Given ORDIN_OTLP_ENDPOINT "http://otel.example.org"
    When the instance starts
    Then the start stops with TEL_ENDPOINT_INVALID and the instance never reports ready
```

## Performance, Security, Observability

- **Performance targets:** span `on_end` under 5 µs p95 and never awaiting; queue memory at most `SPAN_QUEUE` spans; tracing adds no latency to a run (07).
- **Security:** export only to `https` or loopback; allow-listed names and fields only; no `tracestate`, no `baggage`; the endpoint is never logged; no `OTEL_*` variable is read, so no exporter header can be set outside 2D.
- **Observability:** `wrightrun_spans_dropped_total{reason}` (Metrics spec, family 47); log `telemetry_started` (`INFO`; `traces`, `families`) and `spans_dropped` (`WARNING`).

## Gaps

None.

## Amendment January 2026 — Knowledge agents (ADR-113)

The exported allow list gains one span, copied from the Telemetry spec's amendment of January 2026:

| Span | Opened by | Fields (attribute keys) | Values |
|---|---|---|---|
| `ordin.knowledge_query` | Knowledge Query Server: one knowledge query, arrival to first result | `from_cache`, `hits`, `unanswered`, `proxies_asked`, `checked`, `withheld`, `pending` *(the last three added January 2026, ADR-115)* | `true` or `false`; counts only — never a unit, a system, an account or the asker |

*Span names used across the component specs*, row added: `ordin.knowledge_query` — Telemetry, Knowledge Query Server — exported — no field holds the query, a fact or the asker.

```gherkin
Feature: Tracing for knowledge agents (ADR-113)

  Scenario: A knowledge query exports one span with counts only
    Given traces are switched on at a knowledge agent
    When a query is answered from two proxies with one not answering in time
    Then one ordin.knowledge_query span is exported with proxies_asked 2 and unanswered 1
    And the span holds no field carrying the query or a fact

  Scenario: A desktop exports no knowledge span
    Given a desktop workspace connected to one front door
    When the workspace asks the front door a question
    Then the desktop exports no span (A-10)
```

### Gaps

None.
