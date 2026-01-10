# SPEC: Rate Limiting

> **Spec set:** ordin-desktop · **Phase:** 4 — Cross-cutting · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-115) · **Mode:** greenfield · **derivedFromHld:** 0.63.0
> **Implemented in:** no crate of its own; each limit lives in the component that owns the resource: `ordin-session-supervisor` (local sessions, run limits), `ordin-crew-orchestrator` (crew guardrails), `ordin-usage-planner` (person's usage frame), `ordin-triggers` (trigger limits), `ordin-remote-api` (command bucket, failed-key lockout), `ordin-rules` (declared start and repeat limits, agent profile), `ordin-caller-api` (intake runs in progress, load shedding), `ordin-run-queue` (one run per thread), `ordin-mcp-front` (calls in flight), `ordin-memory-service` (memory rate), and each adapter for its provider's limits.

## Purpose

States every limit ordin applies on how much work it starts or accepts, and how it behaves when a provider limits ordin. In the HLD's words, rate limiting "applies, as budgets" (07 — *Rate limiting*): request budgets per run and per crew, a limit on concurrent local sessions, one in-flight run per remote thread, minimum poll intervals, the person's usage settings, trigger limits on the personal server, the Remote API's failed-key lockout and command bucket, and, on a hosted agent, the limits a package or configuration declares, counted exactly in the datastore.

## Approach

Each limit is enforced by the single component that owns the resource it protects, with the count kept where that component keeps its state (in memory for limits held by one running program, in the datastore for limits that must hold across instances of a scaled hosted agent). There is no shared limiter crate: the limits use three different algorithms — a token bucket (Remote API commands), sliding windows over recorded events (failed keys, declared start counts, triggers per day), and concurrency permits (sessions, calls in flight, runs per thread) — and each is a few lines inside its owner. Rejected: a central rate-limit middleware, because most limits are on work started (runs, sessions, crew rounds), not on HTTP requests, and a middleware cannot see them. Rejected: counting in memory on the scaled shape, because several instances serve one agent and wrightrun's rule is an exact count in the datastore.

## Applicability by program

| Program | Applies | How | Source |
|---|---|---|---|
| Desktop (D) | Yes | Budgets and concurrency: 8 local sessions, run limits, crew guardrails, usage frame, poll intervals, MCP calls in flight, memory rate; provider `Retry-After` honoured. | SA-XC-09; SA-RATE-01; A-04 |
| Personal server (P) | Yes | As the desktop, plus trigger limits (one run at a time per trigger, up to 10 waiting, 20 triggered runs a day), the Remote API command bucket and the 10-failed-key lockout. | SA-XC-09; ADR-074; ADR-096 |
| Agent profile (A) | Yes | Declared limits (starts per command, scope or asker per period; repeated-message suppression; runs in progress per intake) counted in the datastore; one run at a time per thread; `ORDIN_RUN_CONCURRENCY` sessions per instance; 100 open requests per instance. | SA-XC-09; ADR-097; ADR-098 |

## Shared Context

**Limit register** (every figure copied from the cited spec; *chosen here* marks a value this spec set fixed):

| # | Limit | Value | Algorithm | Owner | Program | Source |
|---|---|---|---|---|---|---|
| L1 | Concurrent local sessions | 8 (`local_session_limit`); agent profile: `ORDIN_RUN_CONCURRENCY` | concurrency permit + FIFO queue | Session Supervisor | D P A | A-04; 2D-4 |
| L2 | Run wall clock, application maximum | 8 hours | timer | Session Supervisor | D P A | 2D-4 |
| L3 | Run idle timeout | 10 minutes | timer | Session Supervisor | D P A | 2D-4 |
| L4 | Run request budget | as the charter or run sets it; no default ceiling until OQ-3 | counter | Session Supervisor | D P A | 07 NFR; SA-RATE-01 |
| L5 | Crew rounds | default 10, maximum 30 | counter | Crew Orchestrator | D P | 07 NFR; 2D-4 |
| L6 | Crew wall clock | default 60 minutes, maximum 8 hours | timer | Crew Orchestrator | D P | 07 NFR; 2D-4 |
| L7 | Crew members running at once | default 4, maximum 8 | concurrency permit | Crew Orchestrator | D P | 07 NFR; 2D-4 |
| L8 | Same member failing in a row before escalation | 2 | counter | Crew Orchestrator | D P | 2D-4 |
| L9 | Person's agents at once | 1–3, within the profile's frame | setting | Usage Planner | D P | UsagePlanner.md B.3 |
| L10 | Person's daily pause | optional, within the profile's frame | daily counter | Usage Planner | D P | ADR-062; UsagePlanner.md F |
| L11 | Remote run state poll (Caller API) | at least 5 s; `Retry-After` honoured on 429 and 503 | interval | Hosted Agent Adapter | D P | SA-RATE-01 |
| L12 | Slack thread reply poll | at least 10 s; `Retry-After` honoured on 429 (default 30 s when absent) | interval | Slack Thread Adapter | D P | SA-RATE-01; 2D-4 |
| L13 | Remote thread in flight | one run per thread; the adapter queues the next and retries every 30 s | concurrency 1 | Hosted Agent Adapter; Run Queue | D P A | wrightrun ADR-064 |
| L14 | Trigger runs | one at a time per trigger; `limits.max_waiting` 0–10; at most 20 triggered runs a day per installation | permit + bounded queue + daily counter | Trigger Service | P | ADR-074 |
| L15 | Remote API commands | token bucket per pairing, `commands_per_second` 50, burst 100 | token bucket | Remote API | P | RemoteApi.md step 8; OrdinServer.md row 5.41 |
| L16 | Remote API failed key checks | 10 within 60 s from one address → address refused 10 minutes | sliding window | Remote API | P | 2D-4; ADR-096 |
| L17 | Desktop pairing codes | 5 wrong codes within 10 minutes stops pairing | sliding window | Remote API | P | 2D-4 |
| L18 | MCP calls in flight per session | 64; a permit not had within 30 s → refused | semaphore | MCP Front | D P A | McpFront.md step 12 |
| L19 | Memory calls | 600 reads and 600 writes a minute | bucket | Memory Service | D P A | 2D-1e; MemoryService.md |
| L20 | Messages waiting for Ordin | 5 | bounded queue | Ordin Host | D P | OrdinHost.md |
| L21 | Sandbox calls | 50 per run; 500 per session | counter | Sandbox | D P | 2D-4 |
| L22 | Declared start limits | `StartCount { of: Command \| Scope \| Asker, more_than: 0..100000, period_seconds: 1..2592000 }` | count of recorded starts in the window, in the datastore | Rules Engine | A | RulesEngine.md §Condition |
| L23 | Repeated-message suppression | `RepeatedMessage { period_seconds: 1..86400 }` over `rules_control_messages (scope, asker, digest)` | last-seen time per digest, in the datastore | Rules Engine | A | RulesEngine.md |
| L24 | Intake runs in progress | `max_runs_in_progress` per intake; full → 429 with `Retry-After: 30` | counted place, in the datastore | Caller API (`controls.take_intake_place`) | A | CallerApi.md step 27 |
| L25 | Open requests per instance | 100; beyond → 503 `SRV_REFUSED` (load shedding) | concurrency limit | Caller API | A | CallerApi.md step 3 |
| L26 | Provider retries | SharePoint and Confluence: 3 retries, 1 s, 2 s, 4 s ±20 % jitter, `Retry-After` wins; Slack Channel Adapter: 2 waits of `Retry-After` | backoff | each adapter | D P A | SharePointAdapter.md; ConfluenceAdapter.md; SlackChannelAdapter.md |
| L27 | Linked Slack channel reading | every 60 s, at least 10 s per conversation; at most 1,000 messages per channel at start; `Retry-After` honoured on 429 (30 s when absent) | interval | Slack Service | D P | SA-RATE-02; 2D-4 |
| L28 | Slack posts | one retry, only after 429; never after no answer; a rule at most its daily cap (1–20) | retry count; daily counter in the datastore | Slack Service | D P | SA-RATE-02; ADR-102 |
| L29 | Ordin asked about Slack messages | once per link per 5 minutes; 20 a day per installation | interval; daily counter | Slack Service | D P | SA-RATE-02; R-98 |
| L30 | Access checks per question | first 10 units, more only when some are refused, from a shortlist of 3 × the hits asked for; 300 ms deadline; at most 16 in flight per system | staged batch + concurrency permit + deadline | Knowledge Query Server, Source Access | A | SA-ACCESS-02; 2D-4 |
| L31 | Remembered access answers | one per (person, unit) for `access-checks.remember-seconds` (0–300, default 60); none for a source with `check-every-question`; capacity `REMEMBER_CAPACITY` (100,000), oldest evicted | bounded in-memory map with expiry | Source Access | A | SA-ACCESS-03 |
| L32 | Source-system retries for checks, links and reading | GitHub, GitLab, Confluence, Microsoft: `Retry-After` honoured on 429 and 503; reading retries per SourceAccess.md `BACKOFF` and `RETRIES`; a check is never retried past its deadline | backoff + deadline | Source Access | A | SourceAccess.md |
| L33 | Graph batch size | at most 20 requests per `$batch` | batch bound | Source Access | A | 2D-4; C-105 |

## Public Interface

Limits are not called through one interface; each is a check inside its owner. The surface each limit answers on:

| Surface | Signal | Header returned |
|---|---|---|
| Remote API | 429 `SVC_RATE_LIMITED` (L15), 429 `DESK_ADDRESS_LOCKED` (L16), 403 `DESK_PAIRING_STOPPED` (L17) | none *(Remote API spec states none)* |
| Caller API | 429 `INTAKE_BUSY` (L24); 503 `SRV_REFUSED` (L25); 409 `RUN_THREAD_BUSY` (L13) | `Retry-After: 30` on `INTAKE_BUSY` only |
| Service API reply (`kind: "error"`) | `USAGE_DAILY_PAUSE_REACHED`, `USAGE_AGENTS_OUT_OF_RANGE`, `USAGE_ABOVE_PROFILE_LIMIT` (L9–L10), `OH_TOO_MANY_WAITING` (L20), `RA_LIMIT_OUT_OF_RANGE` | — (IPC; status 200 on the Remote API) |
| Run outcome | `SUP_LIMIT_REACHED` (L2–L4), `CREW_LIMIT_REACHED` (L5–L6), `RA_ALLOWANCE_EXHAUSTED` | — |
| Trigger outcome | `refused_by_limit` with `TRIG_RUN_IN_PROGRESS` or `TRIG_DAILY_LIMIT_REACHED` (L14) | — |
| MCP tool result | `isError` `MCP_TOO_MANY_CALLS` (L18); `MEM_RATE_LIMITED` (L19) | — |
| Provider limits met by ordin | `STA_RATE_LIMITED`, `SCA_RATE_LIMITED`, `TGA_RATE_LIMITED`, `SP_THROTTLED`, `CONF_THROTTLED`, `MAIL_RATE_LIMITED`, `PROJ_RATE_LIMITED`, `GH_RATE_LIMITED`, `HAA_UNREACHABLE` | ordin reads `Retry-After` |

**Bypass rules** *(chosen here)*: no caller, role, origin or program is exempt from any limit above. The agent profile's health and metrics routes are not limited by L22–L24 because they start no run; they do count towards L25. An approval press is a command and counts towards L15.

### Example

```
POST /intakes/release-candidate HTTP/1.1
X-Wrightrun-Signature: sha256=9b1c…

HTTP/1.1 429 Too Many Requests
Retry-After: 30
Content-Type: application/json

{"error": "the intake has its limit of runs in progress; try again later", "code": "INTAKE_BUSY"}
```

## Internal Logic

1. **Sessions (L1).** The Session Supervisor's scheduler holds `local_session_limit` permits and a FIFO of queued runs; a run starts only with a permit; a finished, failed or interrupted run returns its permit and the oldest queued run starts.
2. **Run limits (L2–L4).** Each run's task holds a wall-clock timer, an idle timer reset by every harness event, and a request counter; the first to fire stops the run with `SUP_LIMIT_REACHED` and the reason. Limits outside their ranges at start → `RA_LIMIT_OUT_OF_RANGE`.
3. **Crew guardrails (L5–L8).** The Crew Orchestrator validates a charter's guardrails against the application maximum and the company maximum (a charter may lower, never raise; `CHRT_GUARDRAIL_ABOVE_MAX`); at run time a round, time or request limit stops the crew with `CREW_LIMIT_REACHED`; two failures in a row of one member escalate.
4. **Usage frame (L9–L10).** The Usage Planner refuses a setting the company fixed (`USAGE_FIXED_BY_COMPANY`), a value above the company's frame (`USAGE_ABOVE_PROFILE_LIMIT`), agents at once outside 1–3, and starts after the daily pause (`USAGE_DAILY_PAUSE_REACHED`).
5. **Poll intervals (L11–L12, L27).** Each adapter clamps its interval to the minimum; on HTTP 429 it waits the `Retry-After` value (Slack Thread Adapter: 30 s when absent) before the next call.
6. **One run per thread (L13).** The Run Queue refuses `enqueue` on a thread with a queued, executing or paused run (`RUN_THREAD_BUSY`); the Hosted Agent Adapter keeps its own FIFO per thread (`HAA_QUEUE_FULL` when full) and retries every 30 s.
7. **Triggers (L14).** One actor per trigger owns a run slot and a waiting queue of `max_waiting`; an event while the slot is busy joins the queue; a full queue → outcome `refused_by_limit`, `TRIG_RUN_IN_PROGRESS`; the twenty-first triggered run of the day → `TRIG_DAILY_LIMIT_REACHED` and the trigger enters `paused_by_daily_limit` until the next day.
8. **Remote API bucket (L15).** Per pairing, a token bucket refilled at `commands_per_second` up to twice that; an empty bucket → 429 `SVC_RATE_LIMITED`, action `try_again`.
9. **Failed keys and codes (L16–L17).** As `AuthenticationAndAuthorisation.md` steps 8–9; a locked address is refused before its key is read.
10. **Declared limits (L22–L23), agent profile.** The Rules Engine evaluates `StartCount` at `Point::Start` by counting recorded starts of the command, scope or asker within `period_seconds` in its control-state tables, in the same statement that records the new start, so concurrent instances cannot both pass the last place; `RepeatedMessage` upserts `rules_control_messages` and reads `repeated`. A statement failure → `RULE_STATE_UNAVAILABLE`, and inside a decision the call is denied.
11. **Intake places (L24).** `take_intake_place(name, run_id, max_runs_in_progress)` is one datastore statement; false → 429 `INTAKE_BUSY`, `Retry-After: 30`.
12. **Load shedding (L25).** `tower::limit::ConcurrencyLimitLayer` with 100 behind load shedding; beyond → 503 `SRV_REFUSED`.
13. **Provider limits (L26).** Adapters retry with the stated backoff, prefer the provider's `Retry-After`, and after the last retry return their `_RATE_LIMITED` / `_THROTTLED` code as retryable; the caller shows "Try again" or the run waits, never a tight loop.

## Data Model

N/A for this concern. Datastore-backed counts are owned by their components: Rules Engine control-state tables (`rules_control_messages`, the start records it counts), Caller API intake places through `controls`, Run Queue `run_threads` / `run_records`, Trigger Service daily counts. In-memory counts (L1, L15–L18) end with the program.

## Error Table

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| Remote API: command rate above the pairing's bucket | 429 | `SVC_RATE_LIMITED` | `{"request_id":…,"kind":"error","error":{"code":"SVC_RATE_LIMITED",…,"action":"try_again"}}` |
| Remote API: address locked after 10 failed key checks | 429 | `DESK_ADDRESS_LOCKED` | as above with `DESK_ADDRESS_LOCKED` |
| Caller API: intake at its runs-in-progress limit | 429, `Retry-After: 30` | `INTAKE_BUSY` | `{"error": "the intake has its limit of runs in progress; try again later", "code": "INTAKE_BUSY"}` |
| Caller API: more than 100 open requests | 503 | `SRV_REFUSED` | `{"error": "the request was refused", "code": "SRV_REFUSED"}` |
| Caller API: run on a thread whose run has not ended | 409 | `RUN_THREAD_BUSY` | `{"error": …, "code": "RUN_THREAD_BUSY"}` |
| Run wall clock, budget or idle limit reached | run stopped | `SUP_LIMIT_REACHED` | run event `ended` with the reason |
| Crew round, time or request limit reached | crew stopped | `CREW_LIMIT_REACHED` | crew event with the reason |
| Trigger slot busy and queue full | outcome `refused_by_limit` | `TRIG_RUN_IN_PROGRESS` | trigger event record |
| Daily trigger limit reached | outcome `refused_by_limit` | `TRIG_DAILY_LIMIT_REACHED` | trigger event record |
| Person's daily pause reached | `Err(permanent)` | `USAGE_DAILY_PAUSE_REACHED` | Service reply `error` |
| More than 64 MCP calls in flight for 30 s | `isError` | `MCP_TOO_MANY_CALLS` | text "Too many calls at once; wait for one to finish." |
| Memory rate bucket exhausted | `Err(retryable)` | `MEM_RATE_LIMITED` | tool: "Too many memory calls just now; try again in a minute." |
| A declared limit's state cannot be read | `Err(retryable)`; `deny` inside a decision | `RULE_STATE_UNAVAILABLE` | `RulesError::State` |
| Slack answers 429 while polling | wait `Retry-After`, retry | `STA_RATE_LIMITED` | — |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Rate Limiting

  Scenario: Happy path — a ninth local session waits for a place
    Given 8 local sessions are running on the desktop
    When the person starts a ninth run
    Then the run is queued
    And it starts when one of the 8 sessions ends

  Scenario: Edge case — a scaled hosted agent counts starts exactly
    Given a package control StartCount of asker, more_than 3, period_seconds 3600
    And two instances serve the agent
    And asker "u-17" started 3 runs in the last hour
    When "u-17" starts a run on each instance at the same moment
    Then exactly one start is refused by the control

  Scenario: Edge case — Slack asks the adapter to wait
    When conversations.replies answers HTTP 429 with Retry-After 30
    Then the Slack Thread Adapter makes no further call for 30 seconds

  Scenario: Error path — an intake at its limit
    Given the intake "release-candidate" has max_runs_in_progress 2 and 2 runs in progress
    When an event arrives with a valid signature
    Then the answer is 429 with Retry-After 30 and code INTAKE_BUSY

  Scenario: Error path — the twenty-first triggered run of the day
    Given 20 triggered runs started today on the personal server
    When a matching event arrives
    Then the outcome is refused_by_limit with code TRIG_DAILY_LIMIT_REACHED
```

## Performance, Security, Observability

- **Performance targets:** an in-memory limit check adds under 0.1 ms; a datastore-backed check is part of the statement that records the start (no extra round trip); a refusal answers within the surface's own p95 target (150 ms for the Remote API, A-46).
- **Security:** limits protect the person's plan allowance (R-64), the company's providers and the hosted agent's capacity; refusals carry no internal state; the failed-key lockout stores no key material; every limit is decided in fixed code.
- **Observability:** journal entry `remote_api_address_locked`; log lines `intake_refused` (WARN; `intake`, `reason`); trigger outcomes `refused_by_limit` in the trigger event records; run and crew end reasons in the journal; on the agent profile, the Telemetry counters for refused intakes and runs (`Metrics.md`).

## Gaps

None.
