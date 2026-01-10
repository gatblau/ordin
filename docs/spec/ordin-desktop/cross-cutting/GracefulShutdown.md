# SPEC: GracefulShutdown

> **Spec set:** ordin-desktop · **Phase:** 4 — Cross-cutting · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-115) · **Mode:** greenfield · **derivedFromHld:** 0.63.0
> **Implemented in:** `apps/desktop` (`Core::shutdown`, OrdinDesktop.md *Internal Logic* steps 10–11), `apps/server` (*orderly stop*, OrdinServer.md *Internal Logic* step 11, decision P4-2), `ordin-session-supervisor` (`shutdown`, `runs_going`, `stop_taking_runs`, `wait_for_runs`; SessionSupervisor.md H.5, I, SA-CLOSE-03), `ordin-caller-api` (`serve`, `SHUTDOWN_SECONDS`), `ordin-job-runner` (`JOB_STOP_GRACE_SECONDS`). Each module's own `shutdown()` or cancellation token is its spec's; this concern fixes the order and the bounds. The cancellation type is `tokio_util::sync::CancellationToken` (05 — *Utility and cryptography crates*).

## Purpose

Fixes, for each program, what ends the program (a person's quit, the operating system ending the session, sleep, `SIGTERM`/`SIGINT`, a cluster's stop), the order in which modules stop, the bound on each step, what happens to work in flight, and what the next start finds. The invariant from HLD 07 holds throughout: every completed step and every external action is journalled before the next begins, so no stop loses a completed step; a run cut short is recorded *interrupted* and never silently restarted.

## Approach

One ordered, bounded stop sequence per program, written in the composition root (2B-1 rule 3), since only the program holds every module. Each step has a time bound; an overrun is logged with the program's step-timeout code and the next step runs, so one stuck module cannot keep the journal from closing. Producers stop first (entry points refuse new work), then the Session Supervisor ends or drains runs, then providers and hubs, then the Hook Relay, then the Journal and the datastore last. The personal server stops the same modules as the desktop, in the desktop's order, preceded by its own inputs (Channel Gateway, Remote API, Trigger Service), and lets running runs finish until 30 s after the signal before cancelling them; the agent profile never cuts a run short (decision P4-2). Rejected: each module subscribing to the signal on its own, because the order (journal last) could not be guaranteed; an unbounded wait for runs on the desktop and personal server, because a hung harness would block quit (SA-STOP-01 sets 10 s on the desktop); cutting agent-profile runs short at `SIGTERM`, because a run instance's stop period is sized to the run limit (wrightrun *GracefulShutdown*).

## Applicability by program

| Program | Applies | How | Source |
|---|---|---|---|
| Desktop (D) | Yes | Ask before ending running sessions; then the bounded sequence of *Internal Logic* A; sleep marks sessions that cannot continue *interrupted* on wake. | SA-XC-13, SA-STOP-01; OrdinDesktop.md steps 10–11 |
| Personal server (P) | Yes | `SIGTERM`/`SIGINT` runs sequence B without asking: the desktop's modules in the desktop's order after the server's own inputs; runs may finish until 30 s after the signal; the stop ends at most 54 s after the signal, inside systemd `TimeoutStopSec=60s` and the container's 60 s stop period. | SA-XC-13, SA-STOP-01; OrdinServer.md step 11.1 (P4-2) |
| Agent profile (A) | Yes | wrightrun's *GracefulShutdown* order per role (sequence C), single and scaled shapes alike: readiness 503 at once, no new run, runs finish within the deployment's stop period and are never cut short by ordin, workers checkpoint jobs. The single shape also sends `STOPPING=1` first and writes `run/clean-stop` last. | SA-XC-13, SA-STOP-01; OrdinServer.md step 11.2 (P4-2); SessionSupervisor.md H.5, SA-CLOSE-03 |

## Shared Context

| Constant / setting | Value | Owner | Source |
|---|---|---|---|
| Stop deadline per harness child program on desktop quit or sleep | 10 s (`SupervisorConfig.stop_deadline`), then killed and the run marked interrupted | Session Supervisor | SA-STOP-01; OrdinDesktop.md row 3.19; SessionSupervisor.md |
| Desktop: Session Supervisor shutdown step bound (including kills) | 15 s | program | OrdinDesktop.md step 10.3 |
| Desktop: every other stop step bound | 2 s | program | OrdinDesktop.md step 10.3 |
| Personal server: run window at `SIGTERM` | running runs may finish until 30 s after the signal (checked once a second with `runs_going()`); then `shutdown(Signal)` cancels each run still going and records it `interrupted` | program, Session Supervisor | P4-2; OrdinServer.md step 11.1(d); SessionSupervisor.md I, SA-CLOSE-03 |
| Personal server: Channel Gateway outbox drain | bound 10 s | program | P4-2; OrdinServer.md step 11.1(a) |
| Personal server: Remote API in-flight commands | bound 5 s | program | P4-2; OrdinServer.md step 11.1(b); RemoteApi.md *Internal Logic* step 1.6 |
| Personal server: every other stop step bound (including `shutdown(Signal)`) | 2 s | program | P4-2; OrdinServer.md step 11.1 |
| Personal server: worst-case stop | 54 s after the signal (30 s window + 24 s for steps (d)–(i)) | program | OrdinServer.md step 11.1 |
| systemd unit stop timeout (personal) | `TimeoutStopSec=60s` | OrdinServer | OrdinServer.md *Constants* and unit file |
| Container stop period (personal) | 60 s (`--stop-timeout 60`) | OrdinServer | OrdinServer.md *Constants* and run command |
| Container stop signal | `STOPSIGNAL SIGTERM` | OrdinServer | OrdinServer.md Containerfile |
| Agent profile: runs at `SIGTERM` | never cut short by ordin; a run still executing when the process ends keeps its Run Queue record `executing` until its lease lapses and is then recorded `interrupted` by another instance's `mark_interrupted` | Session Supervisor, Run Queue | P4-2; SessionSupervisor.md H.5, SA-CLOSE-03; OrdinServer.md step 11.2 |
| Agent profile stop period (run instances, scaled) | Helm `roles.runs.stopPeriodSeconds`, default `3660`; requests 40, chat and jobs 60 | OrdinServer (Helm chart) | OrdinServer.md *Helm values*; SessionSupervisor.md (2D-3) |
| Agent profile stop period (single shape) | `ordin-server-agent.service` `TimeoutStopSec=` the run limit plus 60 s, default `3660s` | OrdinServer | OrdinServer.md unit file |
| Caller API drain | `SHUTDOWN_SECONDS` = 25 | Caller API | CallerApi.md *Constants* (wrightrun) |
| Job stop grace | `JOB_STOP_GRACE_SECONDS` = 20 | Job Runner | JobRunner.md (wrightrun) |

```rust
// apps/desktop (OrdinDesktop.md)
pub async fn shutdown(&self, reason: QuitReason) -> ShutdownSummary;
pub enum QuitReason { PersonQuit, OperatingSystemEnding }
pub struct ShutdownSummary { pub sessions_stopped: u32, pub sessions_interrupted: u32, pub journal_shut_down: bool }

// ordin-session-supervisor (as cited by OrdinDesktop.md and OrdinServer.md)
pub async fn SessionSupervisor::shutdown(&self, reason: ShutdownReason) -> ShutdownReport; // Quit | Signal | Sleep → { stopped, interrupted }
pub fn SessionSupervisor::runs_going(&self) -> u32;   // personal server: checked once a second in the 30 s window
pub async fn SessionSupervisor::stop_taking_runs(&self);
pub async fn SessionSupervisor::wait_for_runs(&self, stop: CancellationToken);

// ordin-journal
pub async fn Journal::shutdown(&self);
pub async fn Datastore::close(&self);
pub async fn HookRelay::shutdown(&self);
```

## Public Interface

| Trigger | Program | Entry |
|---|---|---|
| Window `CloseRequested`, menu Quit, or command `app.quit` → `AppQuitResult { stopping_sessions: u32 }` | D | `Core::shutdown(QuitReason::PersonQuit)` after the confirmation |
| Event `ordin://quit-requested { "running_sessions": <u32> }` | D | emitted when a quit is asked while local sessions run; the screens answer with `app.quit` or nothing |
| `RunEvent::ExitRequested` without a person's quit | D | `Core::shutdown(QuitReason::OperatingSystemEnding)`, no question |
| Wake detected by `WakeWatcher` | D | `session_supervisor.on_wake(slept_for)`, `server_connector.on_wake()` |
| `SIGTERM` or `SIGINT` (first) | P, A | orderly stop |
| `SIGTERM` or `SIGINT` (second, during the stop) | P, A | exit at once, status 143 (`SIGTERM`) or 130 (`SIGINT`) |

### Example

Personal server, two harness sessions running, `systemctl --user stop ordin-server`:

```
STOPPING=1 sent to systemd
Channel Gateway token cancelled; an update received now → CHAN_STOPPING (retryable)
Remote API: streams closed, Serving::Stopped
Trigger Service paused (no new run, no acknowledgement)
runs_going() checked once a second: 1 run ends at 12 s; 1 still going at 30 s
SessionSupervisor::shutdown(Signal): the remaining run cancelled → interrupted
setup_assistant.close; shutdown() on Telegram Service, Workspace Service, Project Service, Craft Toolkit, Sandbox, File Service, Organisation Profile
core stop token cancelled (Mail Service, Catalogue Service, Memory Service, Telegram Adapter, Slack Channel Adapter, NATS Adapter, MCP Front, Ordin Host, Authoring Service)
HookRelay::shutdown; Journal::shutdown; Datastore::close; run/clean-stop written; exit 0 (about 31 s after the signal)
```

## Internal Logic

**A. Desktop** (OrdinDesktop.md step 10, copied).

1. If a shutdown is already running, do nothing more.
2. When the trigger is not `app.quit` and `session_supervisor.running_local_sessions() > 0`, emit `ordin://quit-requested { running_sessions }` and stop; the screens ask and answer with `app.quit` or nothing.
3. `Core::shutdown(QuitReason::PersonQuit)`, in this order, each step bounded (15 s for the Session Supervisor, 2 s for every other; an overrun logs `DSK_SHUTDOWN_STEP_TIMEOUT` and the next step runs):
   1. `command_bridge.close()` — later commands are refused with `BRG_CLOSING`.
   2. `dictation.cancel(session_id)` for the session the program forwards, if any.
   3. `session_supervisor.shutdown(ShutdownReason::Quit)` — each local run: cancel token, `adapter.cancel`, wait up to 10 s for `Ended`, then mark *interrupted*; crews pause and wait for the person after the next start; a new run meanwhile → `SUP_STOPPING`.
   4. Cancel the Server Connector's token — remote runs continue remotely and are reconciled on the next start.
   5. `setup_assistant.close()`.
   6. `shutdown()` on the Telegram Service, Workspace Service, Project Service, Craft Toolkit, Sandbox, File Service and Organisation Profile.
   7. Cancel the core's shutdown token (Mail Service, Catalogue Service, Memory Service, Telegram Adapter, MCP Front — later tool calls answer `MCP_SHUTTING_DOWN` — and Ordin Host).
   8. `hook_relay.shutdown()`.
   9. `journal.shutdown()`, then `datastore.close()`.
4. Log `shutdown` with the `ShutdownSummary`; `app.exit(0)`.
5. Operating system ending the session: step 3 without asking (`QuitReason::OperatingSystemEnding`).
6. Sleep (OrdinDesktop.md step 11): `WakeWatcher` ticks every 5 s; a tick more than 30 s late by `Instant`, or a wall clock 30 s ahead of `Instant`, means the computer slept; `session_supervisor.on_wake(slept_for)` marks local sessions that cannot continue *interrupted*; the Server Connector reconnects at once; a forwarded dictation session is cancelled. Logged `wake` with `slept_for_s`.

**B. Personal server** (OrdinServer.md step 11.1, decision P4-2, copied). The same modules as the desktop stops, in the desktop's order (A.3), preceded by the server's own inputs; nothing is asked (SA-XC-13). Each step is bounded: (1) 10 s, (2) 5 s, every other step 2 s; an overrun logs `SERVER_SHUTDOWN_STEP_TIMEOUT` and the next step runs.

1. Send `STOPPING=1` to systemd; cancel the Channel Gateway's shutdown token — `receive` refuses with `CHAN_STOPPING`; outboxes drain (bound 10 s).
2. Cancel the Remote API's `serve` token — open streams closed, in-flight commands answered (bound 5 s), `Serving::Stopped`.
3. Cancel the Trigger Service's shutdown token — it pauses: no new run, no acknowledgement (`TRIG_STOPPING` for a start meanwhile).
4. Runs: while `session_supervisor.runs_going() > 0`, check once a second until 30 s after the signal, so running runs may finish (the 60 s stop period less the 24 s steps 4–9 may take, less 6 s margin); then `session_supervisor.shutdown(ShutdownReason::Signal)` takes no new run (`SUP_STOPPING`) and cancels every run still going, recording it `interrupted`. The Session Supervisor never cancels a run inside the 30 s window; the program calls `shutdown` earlier only when no run is going.
5. `setup_assistant.close()`.
6. `shutdown()` on the Telegram Service, Workspace Service, Project Service, Craft Toolkit, Sandbox, File Service and Organisation Profile, one step each.
7. Cancel the core's stop token: Mail Service, Catalogue Service, Memory Service, Telegram Adapter, Slack Channel Adapter, NATS Adapter, MCP Front (later tool calls answer `MCP_SHUTTING_DOWN`), Ordin Host, Authoring Service.
8. `hook_relay.shutdown()`.
9. `journal.shutdown()`, then `datastore.close()`; write `run/clean-stop`; exit 0. The stop ends at most 54 s after the signal, inside `TimeoutStopSec=60s` and the container's 60 s stop period. At the next start, `run/started` present and `run/clean-stop` absent means the last run did not stop in order, and the server sends its restart message from the recovery report; it then writes `run/started` and deletes `run/clean-stop` (OrdinServer.md, *Restart message*).

**C. Agent profile, single and scaled shapes** (OrdinServer.md step 11.2; SessionSupervisor.md H.5; wrightrun *GracefulShutdown* order; P4-2: ordin never cuts a run short). The single shape also sends `STOPPING=1` first and writes `run/clean-stop` last.

1. Readiness answers 503 at once (`SRV_NOT_READY`); liveness keeps answering.
2. `requests` role: stop accepting new connections; finish requests in flight within `SHUTDOWN_SECONDS` (25 s, inside the role's 40 s stop period).
3. `runs` role: `session_supervisor.stop_taking_runs()`, then `wait_for_runs()`, which returns when no run is executing or the program cancels `stop` — running runs finish within the stop period (`roles.runs.stopPeriodSeconds`, default 3,660 s; the single shape's `TimeoutStopSec`, default 3,660 s) and are never cut short by ordin. A run still executing when the cluster or systemd ends the process keeps its Run Queue record `executing` until its lease lapses; the server program's `run_queue.mark_interrupted()` (every 10 s, on another instance) then records it *interrupted*, and it is not started again. The Session Supervisor never calls `mark_interrupted`.
4. `chat` role: close the chat links.
5. `jobs` role: each running job records its checkpoint within `JOB_STOP_GRACE_SECONDS` (20 s); the next taker resumes from it.
6. Stop the health server; exit 0.

**D. Common rules.**

1. A second signal during the stop exits at once (130 or 143) — the operator's escape.
2. Nothing is flushed at stop time that was not already durable: the Journal writes every completed step and every external action before the next begins (HLD 07 — Availability), so `Journal::shutdown` only closes.
3. Remote runs (hosted agents, Slack threads) are never stopped by a local quit; they are reconciled by `recover` on the next start.
4. An *interrupted* run is shown as such and never restarted automatically (HLD 07 — Graceful shutdown, agent profile; SessionSupervisor.md).

## Data Model

N/A — no table of its own. The stop writes through the owners' tables: run state *interrupted* (Session Supervisor, Run Queue), job checkpoints (Job Runner); and one file, `run/clean-stop`, under the server's data root (OrdinServer.md).

## Error Table

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| A command arrives after quitting began (desktop) | reply, `Err(permanent)` | `BRG_CLOSING` | `{"request_id":…,"kind":"error","error":{"code":"BRG_CLOSING",…}}` |
| A desktop shutdown step overruns its bound | logged; the next step runs | `DSK_SHUTDOWN_STEP_TIMEOUT` | — |
| A server stop step overruns its bound | logged; next step runs | `SERVER_SHUTDOWN_STEP_TIMEOUT` | — |
| New run requested while the Session Supervisor shuts down | `Err(permanent)` | `SUP_STOPPING` | module error |
| Chat item received after the gateway's token was cancelled | `Err(retryable)` | `CHAN_STOPPING` | module error |
| Trigger start or proposal after the token was cancelled | `Err(retryable)` | `TRIG_STOPPING` | module error |
| Tool call while the program is shutting down | `isError` | `MCP_SHUTTING_DOWN` | MCP tool result |
| Harness child did not exit within the stop deadline | logged; child killed | `CLA_STOP_DEADLINE` / `CPA_STOP_DEADLINE` | — (run marked interrupted) |
| Agent-profile run still executing when its process ended | run interrupted (by another instance at lease expiry) | `SUP_LEASE_LOST` | state `interrupted`, never started again |
| Agent instance not ready (stopping) | 503 | `SRV_NOT_READY` | `{"error": "…", "code": "SRV_NOT_READY"}` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: GracefulShutdown

  Scenario: Happy path — desktop quit with nothing running
    Given no local session is running
    When the person chooses Quit
    Then no confirmation is asked
    And Journal.shutdown and Datastore.close run before the program exits with status 0

  Scenario: Edge case — desktop quit with running sessions asks first
    Given two local sessions are running
    When the person closes the window
    Then "ordin://quit-requested" is emitted with running_sessions 2
    And nothing stops until the screens send app.quit

  Scenario: Error path — a personal-server run still going 30 s after SIGTERM is interrupted
    Given a personal server with runs "A" and "B" executing
    And run "A" ends 12 seconds after the signal
    When the service receives SIGTERM
    Then the program waits until 30 seconds after the signal
    And run "B" is cancelled and recorded interrupted
    And the journal is shut down, the datastore closed and run/clean-stop is written
    And the program exits 0 within 54 seconds of the signal

  Scenario: Error path — a stop step overruns its bound
    Given the Sandbox's shutdown does not return
    When the desktop shuts down
    Then DSK_SHUTDOWN_STEP_TIMEOUT is logged after 2 seconds
    And the File Service's shutdown runs next

  Scenario: Edge case — an agent run instance drains
    Given an agent run instance executing one run
    When it receives SIGTERM
    Then /readyz answers 503 at once
    And no new run is taken
    And the run is not cancelled by ordin
    And the program exits 0 after the run ends
```

## Performance, Security, Observability

- **Performance targets:** desktop quit with no running session completes in ≤ 2 s p95; with sessions, at most 15 s for the Session Supervisor plus 2 s per remaining step. Personal server stop completes at most 54 s after the signal, inside systemd's `TimeoutStopSec=60s` and the container's 60 s stop period, so `run/clean-stop` is written even in the worst case.
- **Security:** a stop never leaves a half-written external action: an action begun before the stop is either confirmed in the journal or recovered by `journal.recover_external_actions()` at the next start; the Hook Relay stops after the sessions, so a harness tool call during the stop is denied rather than unchecked (fail closed, ADR-045).
- **Observability:** `shutdown` log line with `ShutdownSummary` (desktop); `wake` with `slept_for_s`; step-timeout codes; `STOPPING=1` to systemd; on A, readiness 503 is visible to the cluster at once.

*Closed gaps and defects.* The four earlier gaps are closed by decision P4-2 as applied in OrdinServer.md step 11 and SessionSupervisor.md H.5, I and SA-CLOSE-03: the personal server stops the same modules as the desktop in the desktop's order (B.5–B.7); the Channel Gateway drain is bounded at 10 s and the Remote API wait at 5 s, every other step at 2 s; the worst case is 54 s, inside `TimeoutStopSec=60s` and the container's 60 s stop period; personal runs may finish until 30 s after the signal and are then cancelled and recorded `interrupted`, while the agent profile never cuts a run short.

## Gaps

None.

## Amendment January 2026 — Per-document read access (ADR-115)

Sequence C (agent profile), a knowledge agent only: after the Caller API stops accepting requests, the program calls `SourceAccess::forget_all()`, dropping every remembered check answer; access checks in flight end at their own deadline (300 ms) and their hits are withheld; delegated SharePoint tokens die with the requests that hold them. Nothing of Source Access is written at stop (OrdinServer.md amendment January 2026, step 11.2).
