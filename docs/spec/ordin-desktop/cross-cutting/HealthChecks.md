# SPEC: Health Checks

> **Spec set:** ordin-desktop · **Phase:** 4 — Cross-cutting · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-113; promoted to Approved by the user January 2026) · **Mode:** greenfield · **derivedFromHld:** 0.63.0
> **Implemented in:** desktop: `ordin-setup` (Setup Assistant check runner); personal server: `apps/server` (watchdog task, control socket, `ordin-server health`); agent profile: `ordin-caller-api` (`health_router`, routes 28–30) with the readiness check built by `apps/server`.

## Purpose

States how each program knows it is fit to work and tells its supervisor so. The desktop has no health endpoint: the Setup Assistant runs its checks at start, in the background and on demand, and repairs what it can before telling the person (07 — *Health checks*). The personal server has no network health endpoint: systemd's watchdog every 30 s, or the container's `HEALTHCHECK` running `ordin-server health` over the local control socket (SA-XC-08, ADR-076). A hosted agent serves liveness and readiness without a token, and an instance whose release fails to load never reports ready (ADR-098).

## Approach

Each program uses its supervisor's own mechanism instead of one shared HTTP route: a desktop is supervised by the person, a personal server by systemd or a rootless container, a hosted agent by Kubernetes. Readiness is computed from the same few facts in every server shape — the datastore answers a ping within 2 s, the Memory Service is ready where it was started, and every chat link is open in a chat role — so one rule serves the watchdog, the control socket and `/readyz`. Rejected: a `/healthz` on the personal server, because it would open a port the HLD forbids beyond the Remote API (07 — *Security posture*); rejected: readiness that probes external systems (identity provider, registry, model vendor), because an outage there would take every instance out of service at once and the per-request errors already report it.

## Applicability by program

| Program | Applies | How | Source |
|---|---|---|---|
| Desktop (D) | Yes, locally | No endpoint. Setup Assistant `run_checks` at start (`CheckTrigger::Start`), every 60 minutes (`Background`), on demand (`OnDemand`) and before a session (`BeforeSession`). | SA-XC-08; 07 — Health checks; SetupAssistant.md |
| Personal server (P) | Yes | No network endpoint. `sd_notify` `READY=1` and `WATCHDOG=1` every 30 s when `WATCHDOG_USEC` is set; `ordin-server health` asks over `<data dir>/run/control.sock`; container `HEALTHCHECK` runs it. Harness sign-in checked at start and every 6 hours. | SA-XC-08; ADR-076; OrdinServer.md steps 6–8 |
| Agent profile (A) | Yes | `GET /healthz`, `GET /readyz`, `GET /metrics` on `ORDIN_PORT`, no token. Single shape also serves the control socket and the watchdog. | SA-XC-08; ADR-098; CallerApi.md routes 28–30 |

## Shared Context

| Item | Value | Source |
|---|---|---|
| `NOTIFY_SOCKET`, `WATCHDOG_USEC` | set by systemd; present → `READY=1` and the watchdog signal every 30 s | 2D-1b; OrdinServer.md |
| `ORDIN_PORT` | 1024–65535, default `8080`; the one port for caller, intake, operator, work-folder, health and metrics routes (agent profile) | 2D-1a |
| Background check interval (desktop) | 60 minutes (SA-SETUP-01) | SetupAssistant.md |
| Harness sign-in check on the server | every 6 hours and before each harness session | 2D-4; SetupAssistant.md |
| Readiness check time limit | 2 s (`journal.ping(2 s)`); an unresolved check counts as not ready | CallerApi.md `ReadyCheck` |
| Health command time limit | 5 s to connect and answer | OrdinServer.md step 8 |
| Container health check | `HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=3 CMD ["ordin-server", "health"]` | OrdinServer.md |

```rust
// ordin-setup (copied)
pub enum CheckTrigger { Start, Background, OnDemand, BeforeSession }
pub enum CheckKind { CompanySettings, Harness, HarnessSignIn, Tokens, AgentRepositories, AgentFolders, Programs, CraftApps }
pub enum CheckOutcome { Passed, Repaired, Failed, Skipped }

// ordin-caller-api (copied)
pub type ReadyCheck = std::sync::Arc<
    dyn Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'static>> + Send + Sync>;
pub fn health_router(ready: ReadyCheck, telemetry: std::sync::Arc<ordin_telemetry::Telemetry>) -> axum::Router;

// apps/server (copied)
pub fn notify(socket: &std::ffi::OsStr, state: &str) -> std::io::Result<()>; // "READY=1", "WATCHDOG=1", "STOPPING=1"
```

## Public Interface

| Probe | Program | Form | Healthy answer | Unhealthy answer |
|---|---|---|---|---|
| Setup Assistant `run_checks(trigger)` | D (and P for harness sign-in) | function call in the core, one `CheckOutcome` per `CheckKind` | `Passed` or `Repaired` | `Failed` with a `SETUP_` code, words and one action |
| systemd notification | P, A single shape | datagram to `NOTIFY_SOCKET` | `READY=1` once, then `WATCHDOG=1` every 30 s | no datagram; systemd restarts after `WatchdogSec` |
| `ordin-server health` | P, A single shape | `{"op":"health"}` on `<data dir>/run/control.sock` | exit 0 | exit 1 with `SERVER_NOT_RUNNING` or `SERVER_HEALTH_TIMEOUT` |
| `GET /healthz` | A | HTTP, no token | 200 `{"status":"ok"}` while the program serves | connection refused |
| `GET /readyz` | A | HTTP, no token | 200 `{"status":"ready"}` | 503 `{"error":"the instance is not ready","code":"SRV_NOT_READY"}` |
| `GET /metrics` | A | HTTP, no token | 200 `application/openmetrics-text; version=1.0.0; charset=utf-8` | — |

Kubernetes probes in the Helm chart: liveness `GET /healthz`, readiness `GET /readyz` on `ORDIN_PORT` (OrdinServer.md, scaled shape).

### Example

```
$ ordin-server health; echo $?
0

$ curl -s -o /dev/null -w '%{http_code}\n' http://10.0.3.7:8080/readyz
503
```

## Internal Logic

**Desktop.**
1. At start, the program calls `setup_assistant.run_checks(Start)` before `start_background_checks()`. Checks run in this order: `CompanySettings` (status only, no wait), `Harness` and `HarnessSignIn` for each harness the profile lists (never starting a sign-in), `Tokens`, `AgentRepositories` (each clone can be fetched with the person's sign-in), `AgentFolders`, `Programs` (bundled programs present and unchanged), `CraftApps`.
2. Each check runs its probe; on failure it tries its one repair once; a repaired check reports `Repaired` and is journalled; a check still failing reports `Failed` with one plain-language sentence and at most one action (Try again, Sign in again, Send to IT).
3. `start_background_checks` repeats step 1 every 60 minutes and checks the update feed 60 s after start, then every 24 hours.
4. Before each harness session, the ready harness whose `last_checked_at` is within the background interval is used; an older one is checked first; none ready → `SETUP_NO_READY_HARNESS`.

**Personal server and agent single shape.**
5. After start completes, log `ready` and send `READY=1` when `NOTIFY_SOCKET` is set.
6. Watchdog: when `WATCHDOG_USEC` is set, every 30 s, if `journal.ping(5 s)` is true send `WATCHDOG=1`; otherwise send nothing, so systemd restarts a hung core. A send failure is logged `SERVER_WATCHDOG_SEND_FAILED` and retried at the next tick.
7. Control socket: the server answers `{"op":"health"}` with `Healthy` when `journal.ping(2 s)` is true.
8. `ordin-server health` connects within 5 s and exits 0 on `Healthy`; no socket or a refused connection → `SERVER_NOT_RUNNING`, exit 1; no answer within 5 s → `SERVER_HEALTH_TIMEOUT`, exit 1.
9. Personal profile: harness sign-in checked at start and every 6 hours; a change to signed-out is reported once on the paired chat channel.
10. Channel reconnection is retried without end; an outage over 10 minutes is reported on the other paired channel by the Channel Gateway (ADR-076).

**Agent profile.**
11. Every role but `requests` serves `health_router(ready, telemetry)` on `ORDIN_PORT`; `requests` serves the full router, which includes routes 28–30. No token is checked and Caller Authentication is not called on these routes.
12. `ready()` is false once stopping began; otherwise true when `journal.ping(2 s)` succeeds, `memory.ready()` succeeds in a role that started the Memory Service, and, in a chat role, every `HostedSlackLink::is_open()` is true. A check not resolved within 2 s counts as not ready.
13. An instance whose release fails to load stops before serving, so it never answers `/readyz` with 200.
14. `/healthz` answers 200 until the orderly stop ends.

## Data Model

N/A — no table. The desktop's check results are kept by the Setup Assistant in its own tables (`harness_attachments`) and in the journal; the server's restart facts are files under `<data dir>/run/` (`started`, `clean-stop`, `last-restart-notice`), owned by `apps/server`.

## Error Table

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| Agent instance not ready (datastore ping, memory or chat link fails, or check over 2 s) | 503 | `SRV_NOT_READY` | `{"error": "the instance is not ready", "code": "SRV_NOT_READY"}` |
| `ordin-server health` finds no server | exit 1, `Err(retryable)` | `SERVER_NOT_RUNNING` | `error[SERVER_NOT_RUNNING]: No ordin server answers on this data directory.` |
| `ordin-server health` gets no answer in 5 s | exit 1, `Err(retryable)` | `SERVER_HEALTH_TIMEOUT` | `error[SERVER_HEALTH_TIMEOUT]: The ordin server did not answer within 5 seconds.` |
| A watchdog datagram cannot be sent | logged; retried at the next tick | `SERVER_WATCHDOG_SEND_FAILED` | — |
| Desktop: no listed harness found | `Err(permanent)` in the result | `SETUP_HARNESS_MISSING` | `{"code":…,"action":"send_to_it"}` |
| Desktop: harness output unreadable or check timed out | `Err(retryable)` in the result | `SETUP_HARNESS_CHECK_FAILED` | `{"code":…,"action":"try_again"}` |
| Desktop: agent repository clone or fetch failed | `Err(retryable)` in the result | `SETUP_CATALOGUE_FAILED` | `{"code":…,"action":"try_again"}` |
| Desktop: company settings cannot be fetched or verified | `Err(retryable)` in the result | `SETUP_PROFILE_UNREACHABLE` | `{"code":…,"action":null}` (background retry) |
| No ready harness for a session | `Err(permanent)` | `SETUP_NO_READY_HARNESS` | `{"code":…,"action":"sign_in_again"}` or `send_to_it` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Health Checks

  Scenario: Happy path — a hosted agent instance becomes ready
    Given an agent-profile instance in the requests role whose datastore answers a ping in 10 ms
    When GET /readyz is sent with no Authorization header
    Then the answer is 200 with body {"status":"ready"}

  Scenario: Happy path — the personal server feeds the watchdog
    Given a personal server profile and NOTIFY_SOCKET and WATCHDOG_USEC set by systemd
    When start completes
    Then READY=1 is sent once
    And WATCHDOG=1 is sent every 30 seconds while the journal answers a ping within 5 seconds

  Scenario: Edge case — the desktop repairs before it reports
    Given an agent folder whose files no longer match its lock file
    When the background checks run
    Then the folder is rebuilt from the lock file
    And the AgentFolders check reports Repaired and nothing is shown to the person

  Scenario: Edge case — readiness drops when stopping begins
    Given a ready agent-profile instance
    When the instance is told to stop
    Then GET /readyz answers 503 with code SRV_NOT_READY at once
    And GET /healthz answers 200 until the orderly stop ends

  Scenario: Error path — the container health check finds no server
    Given no ordin server runs on the data directory
    When ordin-server health runs
    Then it exits 1 and prints error[SERVER_NOT_RUNNING]
```

## Performance, Security, Observability

- **Performance targets:** `/healthz` p95 under 5 ms; `/readyz` answers within 2 s in every case (the check is cut at 2 s); `ordin-server health` ends within 5 s; the desktop's start checks do not delay the interactive Start view beyond 3 s (A-03), because `CompanySettings` does not wait and the rest report into the Start view as they finish.
- **Security:** health and metrics routes carry no token and expose no content, identifier of a person, address or secret; the personal server opens no port for health (control socket under the data root, mode set by the program); the readiness check never calls an external system.
- **Observability:** log lines `ready` (roles, release digest), `SERVER_WATCHDOG_SEND_FAILED`, the Setup Assistant's journalled repairs; on the agent profile, readiness transitions are visible in Kubernetes probe events.

## Gaps

None.

## Amendment January 2026 — Knowledge agents (ADR-113)

On an agent-profile server whose agent configuration has a `knowledge-agent` section, `GET /readyz` in a role that started the Knowledge Query Server also requires `KnowledgeQueryServer::ready()` within the same 2 s check (OrdinServer.md amendment January 2026, step 6.6; KnowledgeQueryServer.md *Internal Logic* step 3). In a role that runs the Knowledge Reader, readiness also requires the reader's data directory to be writable (`KRD_DATA_DIR_UNWRITABLE` stops the instance before it serves). The knowledge routes answer `KQ_NOT_READY` (503, retryable, `Retry-After: 30`) while the agent has published no table of contents. `/healthz` is unchanged. The desktop's and the personal server's checks are unchanged: a front door that cannot be reached never makes them unhealthy; the workspace's recall answers from local memory and says the front door did not answer.

```gherkin
Feature: Health of a knowledge agent (ADR-113)

  Scenario: Readiness includes the knowledge store
    Given a requests-role instance of a knowledge agent whose knowledge tables cannot be read
    When GET /readyz is sent
    Then it answers 503 with code SRV_NOT_READY within 2 s

  Scenario: A desktop stays healthy when its front door is down
    Given a desktop workspace connected to one front door that does not answer
    When the Setup Assistant runs its checks
    Then no check fails because of the front door
```

### Gaps

None.
