# SPEC: Logging

> **Spec set:** ordin-desktop · **Phase:** 4 — Cross-cutting · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-107; promoted to Approved by the user January 2026) · amended January 2026 (ADR-113; promoted to Approved by the user January 2026) · amended January 2026 (ADR-115) · **Mode:** greenfield · **derivedFromHld:** 0.63.0
> **Implemented in:** `ordin-types`, module `log` *(chosen here)* — the line form, the field rules and the argument digest; installed by the two composition roots `apps/desktop` (Ordin Desktop spec, Internal Logic step 2) and `apps/server` (Ordin Server spec, Internal Logic step 3). Every crate writes lines through the `tracing` macros only.

## Purpose

Fixes, once for the whole workspace, what one log line holds, how a line is tied to the run, crew run, thread and command it belongs to, which levels exist and where they are set, and what a line may never hold. HLD 07 — *Logging* requires structured JSON lines with time, level, component, run identifier and crew-run identifier, and forbids tokens, message content, file content and tool arguments, which appear only as digests; support export is a person's action and passes through a redaction step. The agent profile keeps wrightrun's line form and run summary unchanged, so operators' log searches keep working (SA-OBS-01). This spec makes the three programs write one line form, so a component spec names its events and fields and nothing else.

## Approach

One formatter in `ordin-types::log`, an implementation of `tracing_subscriber::fmt::FormatEvent`, writes every line in wrightrun's key order (`time`, `level`, `logger`, `event`, then the binding identifiers, then the event's fields) on all three programs; only the destination differs (daily files on the desktop and the personal server, standard output in the agent profile). Binding identifiers are read from the enclosing `tracing` spans, so a component logs `run_id` once by entering the run's span rather than repeating it on every call. Redaction is enforced in two layers: the formatter refuses forbidden field names and bounds every value, and each component spec's Security section forbids content-bearing values. Rejected: `tracing-subscriber`'s stock JSON formatter, because its keys (`timestamp`, `target`, `fields`) differ from wrightrun's and would break the agent profile's log searches; a per-program formatter, because the desktop and server would drift.

## Applicability by program

| Program | Applies | How | Source |
|---|---|---|---|
| Desktop (D) | Yes | JSON lines to `<data root>/logs/ordin.<date>.log`, daily rotation, 14 files kept, level `info` fixed (the desktop reads no environment variable of its own, 2D precedence item 6) | SA-XC-03, SA-OBS-01, A-09; Ordin Desktop step 2 |
| Personal server (P) | Yes | JSON lines to `<ORDIN_DATA_DIR>/logs/ordin-server.log.<date>`, daily rotation, 14 kept, level from `ORDIN_LOG_LEVEL` | SA-XC-03, SA-OBS-01; Ordin Server step 3 |
| Agent profile (A) | Yes | JSON lines on standard output with wrightrun's field names and run summary; level from `ORDIN_LOG_LEVEL`; the cluster's log collector keeps them | SA-XC-03, SA-OBS-01; Ordin Server step 3 |

## Shared Context

**Libraries (1A SA-OBS-01).** `tracing` 0.1.44, `tracing-subscriber` 0.3.23 (feature `json` not used; the formatter is ordin's own), `tracing-appender` 0.2.5 — all MIT. `sha2` 0.10 for digests (05, *Utility and cryptography crates*).

**Environment (2D-1a, copied).**

| Variable | Type | Default | Required | Owner | Description |
|---|---|---|---|---|---|
| `ORDIN_LOG_LEVEL` | `error`, `warn`, `info`, `debug` | `info` | No | ordin server | Level of the JSON log lines. Any other value stops every command with `SERVER_ENV_INVALID`. Read once at start. Not read by the desktop. |

`RUST_LOG` is not read by any program (Ordin Server spec, *Shared Context*). The embedded sandbox receives the level, inside the program, as `PYS_LOG_LEVEL` and the embedded memory component as `log_level` (Sandbox spec; Memory Service spec; 2D-1e, 2D-1f).

**From 2C / `ordin-types` (copied).**

```rust
/// Exactly 64 characters of [0-9a-f] (2C). `from_digest` renders a caller's 32-byte digest.
pub struct Sha256Hex(pub String);
impl Sha256Hex {
    pub fn parse(raw: &str) -> Result<Self, TypesError>;
    pub fn from_digest(digest: &[u8; 32]) -> Self;
}

/// Every module error implements it; a line logs `code()`, never the error's text (rule R-6 below).
pub trait CodedError: std::error::Error + Send + Sync + 'static {
    fn code(&self) -> &'static str;
    fn retryable(&self) -> bool;
    fn repair(&self) -> Option<RepairAction>;
}
```

**Correlation values already fixed by component specs (copied).**

| Value | Set by | Form |
|---|---|---|
| `CallContext.correlation_id` | Service API `handle` | the command's `request_id` as text (Service API spec, Internal Logic step 3) |
| `ToolCallContext.correlation_id` | MCP Front | `<run_id>:<local call id>` (MCP Front spec, step 13) |
| `AdapterContext.correlation_id` | Session Supervisor | the `run_id` (Session Supervisor spec, step 1 of its adapter calls) |

**Spans that carry binding identifiers.** `ordin.run_stretch` (Session Supervisor; field `run_id`) is the Telemetry spec's span name; it is opened on every program, and only the agent profile exports it (see the Tracing spec). The Crew Orchestrator's supervisor task and every command handler enter a span carrying `crew_run_id` and `correlation_id` respectively (rule L-3).

## Public Interface

```rust
// crate ordin-types, module log (chosen here)

/// The one line formatter, used by apps/desktop and apps/server.
pub struct JsonLineFormat;
impl<S, N> tracing_subscriber::fmt::FormatEvent<S, N> for JsonLineFormat
where S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
      N: for<'a> tracing_subscriber::fmt::FormatFields<'a> + 'static;

/// Where lines go, chosen by the program.
pub enum LogSink {
    /// Daily files `<dir>/<prefix>.<YYYY-MM-DD>.<suffix>`, `keep` files kept.
    DailyFiles { dir: std::path::PathBuf, prefix: &'static str, suffix: &'static str, keep: usize },
    Stdout,
    /// Server commands that print a result (every command but `start`, `agent start`, `agent job run-one`).
    Stderr,
}

/// Parses ORDIN_LOG_LEVEL; `None` (unset or empty) → Info.
pub fn level_from_env(raw: Option<&str>) -> Result<tracing::Level, LogLevelInvalid>;
pub struct LogLevelInvalid; // mapped by the server program to SERVER_ENV_INVALID

/// Installs the global subscriber once: JsonLineFormat, the sink, the level filter
/// (ordin crates at `level`; every other crate at WARN). Returns the appender guard the program holds.
pub fn install(sink: LogSink, level: tracing::Level) -> Option<tracing_appender::non_blocking::WorkerGuard>;

/// SHA-256 of the canonical JSON of tool arguments (rule R-3). The only form in which
/// arguments reach a log line, a journal row or a Rules Engine record.
pub fn arguments_digest(arguments: &serde_json::Value) -> Sha256Hex;

pub const EVENT_NAME: &str = r"^[a-z][a-z0-9_.]{0,63}$";      // wrightrun Logging spec
pub const FIELD_NAME: &str = r"^[a-z][a-z0-9_]{0,39}$";        // wrightrun Logging spec
pub const RESERVED_KEYS: [&str; 7] = ["time", "level", "logger", "event", "run_id", "crew_run_id", "thread_id"];
pub const FORBIDDEN_FIELDS: [&str; 14] = ["token", "secret", "password", "authorization", "signature", "cookie",
    "url", "uri", "dsn", "address", "content", "body", "arguments", "prompt"]; // wrightrun Logging spec
pub const MAX_FIELD_CHARS: usize = 4_000;                      // wrightrun Logging spec
pub const MAX_REPORT_CHARS: usize = 262_144;                   // field `report` of event `startup_report` only
pub const MAX_LIST_ITEMS: usize = 200;                         // wrightrun Logging spec
pub const MAX_PANIC_MESSAGE_CHARS: usize = 200;                // Ordin Desktop spec, step 2
```

**The form of a line.** One JSON object, UTF-8, one line, keys in this order:

| Key | Type | Present | Content |
|---|---|---|---|
| `time` | string | always | UTC, RFC 3339, milliseconds, `Z` (SA-TIME-01; wrightrun's form is the same) |
| `level` | string | always | `DEBUG`, `INFO`, `WARNING`, `ERROR` (wrightrun's spelling; `tracing`'s `WARN` is written `WARNING`, `TRACE` is never enabled) |
| `logger` | string | always | the event's `tracing` target: the crate's module path (`ordin_mcp_front`) unless the call sets `target:`. This is HLD 07's *component* |
| `event` | string | always | the event's name, its `message` (`mcp_front.call`, `run_summary`); `library_record` for a line a library wrote |
| `run_id` | string | while a span with `run_id` encloses the event, or given as a field | UUIDv7 |
| `crew_run_id` | string | D and P, while a crew-run span encloses the event | UUIDv7 |
| `thread_id` | string | A, while a run is bound | thread identifier |
| `correlation_id` | string | while a command or tool-call span encloses the event | see *Shared Context* |
| each field | per rule R-4 | as given | in the order the call gave them |

### Example

Desktop, one MCP tool call inside a crew run:

```json
{"time":"2026-01-10T09:41:07.312Z","level":"INFO","logger":"ordin_mcp_front","event":"mcp_front.call","run_id":"0192a1c2-3344-7556-8677-8899aabbccdd","crew_run_id":"0192a1c2-0000-7a10-8b20-0c3d4e5f6a7b","correlation_id":"0192a1c2-3344-7556-8677-8899aabbccdd:0192a1c2-4455-7c66-9d77-aabbccddeeff","call_id":"0192a1c2-4455-7c66-9d77-aabbccddeeff","server":"knowledge","tool":"search","via":"mcp","outcome":"ok","code":null,"duration_ms":48,"arguments_digest":"9f2c1e7a40b3d85c6e1f0a2b4c6d8e0f11223344556677889900aabbccddeeff","output_chars":1812}
```

Agent profile, a refused field name in a component's call (a programming fault caught by rule R-2):

```json
{"time":"2026-01-10T09:41:08.004Z","level":"INFO","logger":"ordin_connection_binder","event":"connection_bound","connection":"tickets","kind":"http","dropped_fields":["bot_token"]}
```

## Internal Logic

**Installation (programs).**

1. **L-1 Desktop.** Ordin Desktop step 2: after `<data root>` and `logs/` exist, `install(LogSink::DailyFiles { dir: <data root>/logs, prefix: "ordin", suffix: "log", keep: 14 }, Level::INFO)`. Failure to create the folder is `DSK_DATA_ROOT_UNWRITABLE` and stops start before any line is written. A panic hook logs event `panic` (`ERROR`) with `location` and the first `MAX_PANIC_MESSAGE_CHARS` characters of the message, then calls the default hook.
2. **L-2 Server.** Ordin Server step 3: `level_from_env(ORDIN_LOG_LEVEL)`; `Err` → exit 1 with `SERVER_ENV_INVALID`, written to standard error in the `error[CODE]: sentence` form because no subscriber exists yet. Personal profile and single shape: `LogSink::DailyFiles { dir: <ORDIN_DATA_DIR>/logs, prefix: "ordin-server", suffix: "log", keep: 14 }`. Agent profile `agent start` and `agent job run-one`: `LogSink::Stdout`. Every other command: `LogSink::Stderr`, so standard output holds the command's result (wrightrun Logging spec, *Where a command logs*). An unwritable `logs/` is `SERVER_DATA_ROOT_UNWRITABLE`.
3. **L-3 Binding.** Session Supervisor enters `ordin.run_stretch` (field `run_id`; in A also `thread_id`) around every adapter call and event of a run; the Crew Orchestrator enters a span with `crew_run_id` around its supervisor task; the Service API's `handle` and the MCP Front's call path enter a span with `correlation_id`. A task spawned inside such a span is `.instrument`ed with the current span, so it carries the identifiers. A field given on the event that differs from the bound identifier is written as given and the line adds `binding_conflict: true` (the wrightrun rule raised an error; a log line must never fail in Rust).

**Formatting (`JsonLineFormat`).**

4. **R-1 Order.** Write `time`, `level`, `logger`, `event`; then `run_id`, `crew_run_id`, `thread_id`, `correlation_id` from the innermost enclosing span that recorded each, else from the event's fields; then every other field in call order.
5. **R-2 Names.** An event name not matching `EVENT_NAME` is written as `invalid_event_name` with field `name_chars` (its length). A field name not matching `FIELD_NAME`, equal to a `RESERVED_KEYS` entry other than the binding identifiers, equal to a `FORBIDDEN_FIELDS` word, or ending with `_` plus such a word (`bot_token`, `gateway_url`) is not written; its name is added to the list field `dropped_fields`. Each component spec's tests assert that its events produce no `dropped_fields`.
6. **R-3 Arguments.** Tool arguments are logged only as `arguments_digest`: `Sha256Hex::from_digest(sha256(ordin_types::encoding::canonical_json(arguments)))`, where canonical JSON has object keys sorted byte-wise at every level and no insignificant white space. `ordin-types` holds this one function (SharedTypes.md `encoding.rs`; the Agent Model re-exports it, and the Hook Relay's MAC uses it) so that the MCP Front (`mcp_front.call`), the Hook Relay (built-in tools), the Journal (`ToolRequest.arguments_digest`) and the Rules Engine (`rules_approved_calls`) compute the same value.
7. **R-4 Values.** Kept to a depth of four containers: `null`, booleans, integers; a non-finite float becomes `null`; a string longer than `MAX_FIELD_CHARS` (or `MAX_REPORT_CHARS` for `startup_report.report`) is cut and `…[cut]` appended; a list keeps at most `MAX_LIST_ITEMS` items, the last replaced by `…[cut]`; an object's keys are cut to 200 characters; a value recorded with `?` (`Debug`) is written as the text `<debug>`, so a type's `Debug` output — which may hold content — never reaches a line. Types holding secrets print `[redacted]` in `Debug` anyway (Identity and Credentials, Caller API, Journal `DatabaseUrl`, Model Policy `OrganisationKey`).
8. **R-5 Libraries.** Events from crates outside the workspace are written only at `WARNING` and above, as event `library_record` with field `template` (the message cut to 200 characters); their field values are not written, because a library's fields may hold an address or a header.
9. **R-6 Errors.** An error is logged as its `code()` in field `code` (or `error_code`), never as its `Display` text or source chain. A provider's own text is never logged, with the one exception the Model Policy spec fixes (`provider_text`, cut at `MAX_FIELD_CHARS`).
10. **R-7 Redaction rule (binding on every crate).** No line holds: a token, secret, password, `Authorization` header, signature, shared secret, cookie, key material; the value of an environment variable; a connection's or collector's address; a database connection string; message text (chat, mail, Slack, Telegram, harness transcript); document, file, page or attachment content; a knowledge item, note or memory fact text; a model's message; a tool argument's name-and-value or value; terminal content (07 — *Security posture*). A line may hold: identifiers ordin mints, a path inside a place, an agent folder or the data root, a slug or declared name (connection, intake, control, rule, tool, setting, model, command, server), a kind, a state, a registry code, a count, a size, a duration, a digest. A name taken from a request is logged only after it matched a declared name; otherwise the word `unknown` (wrightrun Logging spec, step 8).
11. **R-8 Failure to write.** A failed write (disk full, closed stream) loses that line; `tracing-appender`'s non-blocking writer drops lines when its buffer is full rather than blocking the caller. Nothing is raised.

**Levels.**

12. **V-1.** `ERROR`: a failure that stops a command, a run or a start. `WARNING`: a refused request, a dropped item, a retry. `INFO`: one line per state change, command, tool call and run stretch. `DEBUG`: per-step detail, off by default. The level filter applies to ordin crates (`ordin_*`, `memory`, `sandboxd` targets); other crates are filtered at `WARNING`.

**Run summary (agent profile).**

13. **S-1.** The Session Supervisor writes event `run_summary` once per stretch with wrightrun's fields in wrightrun's order (`run_id`, `thread_id`, `started_by`, `connection`, `scope`, `command`, `tools`, `tool_calls`, `tokens`, `used_fallback`, `context`, `context_cut`, `context_left_out`, `retried_without_context`, `controls`, `gates_set_aside`, `actions_left_out`, `argument_rules`, `events`, `state`, `ending`, `error_code`, `safe_reply`, `resumed`, `elapsed_seconds`, `limit_seconds`; Session Supervisor spec, step 2 of the run summary). The Agent Loader writes `startup_report` once per instance.

13a. **S-1a (amended January 2026, ADR-107).** Two fields are appended after wrightrun's, so every existing search keeps working: `checks` (content checks asked for in the stretch) and `checks_blocked` (of them, those that blocked). The field `context` is filled for a chat run: one object per configured context step, in step order, `{step, kind, outcome, items, bytes, observed_bytes}` — `step` the position from 1, `kind` the step kind (`transcript`, `images`, `channel-documents`, `knowledge-lookup`, `recent-messages`, `precedence-note`), `outcome` `piece`, `cut`, `empty`, `left-out` or `skipped` (a `recent-messages` step whose keywords were not in the question, a `precedence-note` whose two steps did not both give a piece), `items` the number of items the step supplied (messages, images, canvases or documents, knowledge hits; 0 for `empty`, `left-out` and `skipped`), `bytes` the UTF-8 bytes it supplied, `observed_bytes` the UTF-8 size observed before a cut or of an item left out for size (absent otherwise). A step that found nothing is listed with `items` 0, so a run that read no channel document shows it. Content is never a field: no text, no label beyond the step kind, no source item identifier.

**Support export (desktop and personal server).**

14. **E-1.** `diagnostics.support_export` (Journal `support_export(run_ids)`, 1–20 runs, else `JRNL_EXPORT_INVALID`) builds JSON Lines from journal rows only — entries, event `seq`/`at`/`kind` without `data`, tool requests with digests, external actions without `destination`, start-up reports, run steps without `line` — capped at 10,000,000 bytes. Log files are not included. The redaction step is that the journal stores no content (Journal spec, *Security*); the export never reads a log file or a place.

## Data Model

N/A — logs are files under the data root (D, P) or standard output (A); no table. Rotation deletes the oldest file beyond 14; nothing else deletes log files. The Journal's tables are the Journal spec's.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `ORDIN_LOG_LEVEL` holds a value other than `error`, `warn`, `info`, `debug` | exit 1, `Err(permanent)` | `SERVER_ENV_INVALID` | standard error: `error[SERVER_ENV_INVALID]: ORDIN_LOG_LEVEL must be error, warn, info or debug.` |
| Desktop `<data root>/logs` cannot be created or written | `Err(permanent)` — start stops | `DSK_DATA_ROOT_UNWRITABLE` | start-failure window with **Send to IT** (Ordin Desktop spec) |
| Server data dir not owned by the user or `logs/` not writable | exit 1, `Err(permanent)` | `SERVER_DATA_ROOT_UNWRITABLE` | standard error: `error[SERVER_DATA_ROOT_UNWRITABLE]: …` (Ordin Server spec) |
| Support export asked for no run or more than 20 runs | `Err(permanent)` | `JRNL_EXPORT_INVALID` | `{"request_id":…,"kind":"error","error":{"code":"JRNL_EXPORT_INVALID","message":"Choose between 1 and 20 runs to export."}}` |
| A component logs a forbidden or malformed field name | line written without it | — | `dropped_fields: ["<name>"]` on the line; the component's test fails |
| Writing a line fails | line lost; nothing raised | — | — |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Logging

  Scenario: Happy path — a tool call line carries the run and the digest only
    Given a desktop run "0192a1c2-3344-7556-8677-8899aabbccdd" inside crew run "0192a1c2-0000-7a10-8b20-0c3d4e5f6a7b"
    When the MCP Front answers tool "search" with arguments {"query":"Q3 invoices"}
    Then one line with event "mcp_front.call" is written with keys in the order time, level, logger, event, run_id, crew_run_id, correlation_id
    And its field arguments_digest equals the SHA-256 of the canonical JSON {"query":"Q3 invoices"}
    And no line in the file contains the text "Q3 invoices"

  Scenario: Edge case — a forbidden field name is dropped, not written
    Given a component logs event "connection_bound" with fields connection "tickets" and bot_token "xoxb-1"
    When the line is formatted
    Then the line holds connection "tickets" and dropped_fields ["bot_token"]
    And the line does not contain "xoxb-1"

  Scenario: Edge case — a long value is cut
    Given a field "reason" of 5,000 characters
    When the line is formatted
    Then the field holds 4,000 characters followed by "…[cut]"

  Scenario: Agent profile — lines go to standard output in wrightrun's form
    Given ORDIN_LOG_LEVEL is unset and the program runs "ordin-server agent start"
    When a run stretch ends
    Then standard output holds one line with event "run_summary" and level "INFO"
    And no log file is created under the data dir

  Scenario: Agent profile — the run summary lists each context step and the content checks (ADR-107)
    Given a chat run with a channel-documents step that found no canvas and one content check on each point, none blocking
    When the run stretch ends
    Then the run_summary line's context holds an entry with kind "channel-documents", outcome "empty", items 0 and bytes 0
    And its checks is 2 and checks_blocked is 0
    And the line holds no text of the question, the answer or a document

  Scenario: Error path — an unknown log level stops the server
    Given ORDIN_LOG_LEVEL is "verbose"
    When "ordin-server start" runs
    Then it exits with status 1
    And standard error holds "error[SERVER_ENV_INVALID]: ORDIN_LOG_LEVEL must be error, warn, info or debug."

  Scenario: Error path — a support export of 21 runs is refused
    When the person asks for a support export of 21 runs
    Then the reply has code JRNL_EXPORT_INVALID
```

## Performance, Security, Observability

- **Performance targets:** formatting one line of up to 20 fields in under 20 µs p95; the file writer is `tracing-appender`'s non-blocking writer, so a component never waits on disk; at `INFO` the desktop writes under 50 MB a day at A-04's eight concurrent sessions *(assumed here)*, so 14 files stay under 700 MB.
- **Security:** rules R-2 to R-7 are the redaction policy; log files are created with the data root's permissions (`0700` folder on macOS and Linux); a support export is the person's action and holds journal rows only; logs are never sent anywhere by ordin.
- **Observability:** this spec defines the log channel itself. No metrics or spans of its own; the agent profile's counters and spans are the Metrics and Tracing specs'.

## Gaps

None.

## Amendment January 2026 — Knowledge agents (ADR-113)

A knowledge agent (an agent-profile server whose agent configuration has a `knowledge-agent` section) adds three crates that log: the Knowledge Query Client (every program), the Knowledge Query Server and the Knowledge Reader (agent profile). Each follows R-1 to R-7 unchanged. The redaction rule gains these items:

- **R-7a (added).** No line holds: the text of a knowledge query or of a sub-query; a fact, statement, thing name, alias or link word returned by another knowledge agent or read from a repository; a document's or page's text; a table of contents' entity names; the subject or groups of an asker; a repository address with its path when it identifies a document (the reader logs a document as `path_hash`, the SHA-256 of the path, with the repository key). A line may hold: a request id, a connection id, an answer id, a repository key, a commit, a count, a duration, a registry code, the outcome of a document or a cycle, a proxy's configured name.
- **R-7b (added).** The queries a knowledge agent serves are counted and timed, never logged with their text (ADR-113): the Knowledge Query Server's `kq_served` carries `request_id`, `from_cache`, `hits` (a count), `unanswered` (a count) and `first_ms` only; the Memory Service's wider recall logs the number of facts asked for and returned, never the question.

Event names added (each spec's *Observability* is the record): Knowledge Query Client `kq_ask`, `kq_bad_event`, `kq_contents_trimmed`, `kq_hit_dropped`, `kq_service_token_refused`, `kq_store_failed`; Knowledge Query Server `kq_server_started`, `kq_served`, `kq_refused`, `kq_fact_skipped`, `kq_sources_record_failed`, `kq_hot_copy_failed`; Knowledge Reader `cycle_started`, `cycle_completed`, `cycle_skipped_leased`, `fetch_failed`, and the per-document events of KnowledgeReader.md; Ordin Host `ordin_host.recall_wider`; Telemetry `metric_label_unknown`.

```gherkin
Feature: Logging for knowledge agents (ADR-113)

  Scenario: A served knowledge query is logged without its text
    Given a knowledge agent that answers the query "who supplies packaging to Borealis Ltd"
    When the log lines of that request are read
    Then exactly one kq_served line names the request id, counts and first_ms
    And no line holds "packaging", "Borealis" or the asker's subject

  Scenario: A document the reader cannot read is logged by path hash
    Given the Knowledge Reader fails to read policies/suppliers.md in repository key "handbook"
    When the log is read
    Then the document_read_failed line holds the repository key and path_hash
    And it does not hold "policies/suppliers.md" or any text of the document
```

### Gaps

None.

## Amendment January 2026 — Per-document read access (ADR-115)

- **R-7c (added).** No line holds: a person's token presented for linking; a SharePoint delegation; an asker statement; a linked account id or label; a company subject next to a source system; the access unit, repository, page or file of a refused or unchecked check; a document title or body read from Confluence or SharePoint; a webhook secret, token or body; the agent's own Atlassian or Microsoft credentials. Checks are logged as counts only (`src_checks {units, allowed, refused, unchecked, remembered, ms}`), links as `{system, outcome}`, notices as `{system, event, units_forgotten}` (ADR-115).

Event names added: Source Access `src_started`, `src_link`, `src_checks`, `src_check_forbidden`, `src_delegation_refused`, `src_notice`, `src_delta_resync`, `src_read_sign_in_needed`, `src_forget_all`; Identity and Credentials `cred.resource_token`; the Knowledge Query Client `kq_accounts_linked`, `kq_delegation_unavailable`; the Knowledge Query Server `kq_link_refused`, `kq_accounts_linked`, `kq_notice_ignored`, and its `kq_served` gains `withheld` and `pending` (counts); the Knowledge Reader `source_fetch_failed`, `restrictions_unavailable`, `source_unreadable`, `document_refused`.

```gherkin
Feature: Logging for per-document read access (ADR-115)

  Scenario: A refused check names no document
    Given a question where one unit is refused for the asker
    When the knowledge agent answers
    Then no log line contains that unit, the asker's subject or any account id
    And src_checks carries refused 1 as a count
```
