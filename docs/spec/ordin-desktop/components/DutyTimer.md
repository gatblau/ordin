# SPEC: Duty Timer

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-113; promoted to Approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-duty-timer` · **Build phase:** 3 · **Programs:** A
> **Dependencies:** Agent Configuration, Run Queue, Telemetry, Journal

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

The Duty Timer fires the schedule entries of the agent release an instance runs (02:805–807, ADR-097 item 6, ADR-098 item 5). It reads the entries from the agent configuration at start; every 10 seconds it works out which entries have come due; for each due instant it asks the Run Queue to queue the firing's run, which happens once across all instances, so one firing starts one run whatever the number of instances. The run is on a new thread with the entry's command and arguments. An instance that starts looks back 60 seconds; a firing older than one minute when an instance looks is not made up (2D-4 *Missed schedule firing*). The Duty Timer also records each firing it queued (the *Schedule firing* entity it owns, 02:806 amendment). No component reads the record back: the operator's `duty status` reads the Caller API's duty routes and the work folder, as wrightrun's does (SA-RECON-01).

## Approach

A loop with a fixed 10-second tick that asks a cron library for the due instants between the last tick and now, and calls `RunQueue::enqueue_firing`, whose unique index on (entry key, due instant) makes the one-run-per-firing guarantee a single statement on the Run Queue's own table (see Run Queue spec, `run_records_one_per_firing`). Every instance computes the same instants from the same expression, so no leader is needed. The firing record in this crate's `schedule_firings` table is written by the instance whose call queued the run; it is a report, not the guard. Cron expressions are evaluated with the **`croner`** crate (MIT, POSIX five-field syntax) (HLD 05, row *Cron expressions (agent profile)*). Error prefix: `TIMER_` (wrightrun's). Rejected: a lock that elects one timer instance — a firing would wait for the lease to pass when the holder stops, and the unique index already gives one winner. Rejected: a Kubernetes timed workload per entry — entries live in the agent release and must change with it, with no change to deployment files (wrightrun ADR-048).

### Reconciliation assumptions

- **SA-RECON-01** The Duty Timer offers no read of its firing records (reconciliation row 449, ruling R6): wrightrun's `duty status` reads `GET /operator/duties` and the work folder (wrightrun OperatorCommands.md step 14), so the Caller API keeps wrightrun's duty routes unchanged and needs no Duty Timer edge.
- **SA-RECON-02** The crate exports `migrations() -> ModuleMigrations` with `MigrationStep` tuples and counts through `Telemetry::record(TelemetryEvent::ScheduleFired { duty })` (rulings R3, R4); `ScheduleEntry` is imported from `ordin_types::domain::agent_profile` (reconciliation row 591, N126).

## Shared Context

### From Agent Configuration (types in `ordin_types::domain::agent_profile`, copied from AgentConfiguration.md)

wrightrun's `schedules[]` key; the configuration validator has already checked each entry (duty is a recurring duty of the package, command is a command of the package, five-field cron, interval not shorter than the run limit, 02:771).

```rust
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ScheduleEntry {
    pub duty: String,                       // slug of a recurring duty's skill
    pub command: String,                    // slug of a command of the package
    #[serde(default)] pub arguments: String,// ≤ 4,096 characters
    pub cron: String,                       // five fields, UTC
}
```

### From Run Queue (`ordin-run-queue`)

```rust
#[async_trait]
pub trait RunQueue: Send + Sync {
    /// Records the run of one firing on a new thread, started by ScheduleEntry { entry: entry_key },
    /// request Command { command, arguments: [TaskArgument { name: "arguments", value: arguments }] }.
    /// None, writing nothing, when a run for this entry_key and due exists.
    async fn enqueue_firing(&self, entry_key: &str, due: Timestamp, scope_id: ScopeId,
                            command: &str, arguments: &str) -> Result<Option<RunRecord>, RunQueueError>;
    // … other methods not used here
}
// RunQueueError::code(): "RUN_BAD_INPUT" | "DB_UNAVAILABLE" | …; retryable() true for DB_UNAVAILABLE only
```

`RunRecord` (2C) fields read here: `run_id: String`, `thread_id: String`.

### From Telemetry (`ordin-telemetry`, copied from Telemetry.md)

```rust
impl Telemetry {
    /// One increment for one event. Never fails and never awaits.
    pub fn record(&self, event: TelemetryEvent);
}
pub enum TelemetryEvent { ScheduleFired { duty: String }, /* … other variants not used here */ }
// family 4: wrightrun_schedule_firings_total{duty}, fed by the Duty Timer
```

### Shared types (2C)

`Timestamp` (RFC 3339 ms `Z`), `ScopeId` (UUIDv7). The firing's run has the agent-wide scope; the program passes that `ScopeId` (kept by Retention and Erasure, 02:826) at construction.

### From the Journal (`ordin-journal`, copied from JournalAndArtefactStore.md)

```rust
#[derive(Clone)] pub enum Datastore { Sqlite(sqlx::SqlitePool), Postgres(sqlx::PgPool) }
pub type MigrationStep = (i64, &'static str, &'static str);   // (version, description, SQL); versions 1, 2, 3, …
pub struct ModuleMigrations { pub module: &'static str, pub sqlite: &'static [MigrationStep], pub postgres: &'static [MigrationStep] }
// The program passes `migrations()` of every table-owning module to `MigrationRegistry::register` (ruling R3).
```

### Constants

| Constant | Value | Source |
|---|---|---|
| `TIMER_TICK_SECONDS` | 10 | wrightrun `DutyTimer.md` (SA-77) |
| `FIRING_LATE_SECONDS` | 60 | 2D-4; 02:805 ("older than one minute is not made up") |
| `MAX_ENTRY_KEY_CHARS` | 512 | wrightrun |

## Public Interface

```rust
// crate ordin-duty-timer
use ordin_types::domain::agent_profile::ScheduleEntry;

/// "<duty>/<command>/<cron>", the same text as the run's RunStarter::ScheduleEntry { entry }.
pub fn entry_key(entry: &ScheduleEntry) -> String;

/// The Schedule firing entity (wrightrun's fields; owned here, 02:806 amendment).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScheduleFiring {
    pub entry_key: String,
    pub due: Timestamp,
    pub recorded_at: Timestamp,
    pub run_id: String,
}

pub trait Clock: Send + Sync { fn now(&self) -> chrono::DateTime<chrono::Utc>; }

pub struct DutyTimer { /* entries with keys and parsed crons, queue, store, telemetry, scope, clock, tick */ }
impl DutyTimer {
    /// Fails with TIMER_BAD_ENTRY for an entry naming an unknown command or duty, an invalid
    /// five-field cron, or two entries with one key.
    pub fn new(entries: &[ScheduleEntry],
               commands: &std::collections::BTreeSet<String>,     // the package's command slugs
               duties: &std::collections::BTreeSet<String>,       // the package's recurring duty slugs
               queue: std::sync::Arc<dyn ordin_run_queue::RunQueue>,
               datastore: &ordin_journal::Datastore,
               telemetry: std::sync::Arc<ordin_telemetry::Telemetry>,
               agent_wide: ScopeId,
               clock: std::sync::Arc<dyn Clock>,
               tick: std::time::Duration) -> Result<Self, TimerError>;
    /// Returns when `stop` is cancelled; never returns an error.
    pub async fn run_forever(&self, stop: tokio_util::sync::CancellationToken);
}
/// Module "duty_timer": version 1 of the Data Model, for SQLite and PostgreSQL.
pub fn migrations() -> ordin_journal::ModuleMigrations;   // module "duty_timer" (ModuleMigrations.module)

#[derive(Debug, thiserror::Error)]
pub enum TimerError {
    #[error("Schedule entry {index} cannot be fired: {reason}.")] BadEntry { index: usize, reason: String },
}
impl TimerError { pub fn code(&self) -> &'static str; pub fn retryable(&self) -> bool; }
```

`tokio-util`'s `CancellationToken` is already a workspace dependency (`memory/Cargo.toml:45`).

**Called by.** `entry_key`, `DutyTimer::new`, `run_forever` and `migrations`: the server program (OrdinServer, agent profile, `runs` role and single shape). `ScheduleFiring` is the row shape of `schedule_firings`; no other component reads it. Removed in reconciliation: `firings(entry_key, limit)` and `TimerError::Unavailable` (`TIMER_UNAVAILABLE`) — no consumer called it (reconciliation row 449, SA-RECON-01).

### Example

Configuration of the running release:

```yaml
schedules:
  - duty: triage
    command: run-triage
    arguments: "queue=support"
    cron: "0 9 * * 1-5"
```

Three run instances run on Wednesday January 2026. Instance A ticks at 09:00:03.210 UTC, finds the due instant `2026-01-10T09:00:00.000Z` and calls `queue.enqueue_firing("triage/run-triage/0 9 * * 1-5", 2026-01-10T09:00:00.000Z, agent_wide, "run-triage", "queue=support")`, which returns a queued run; A writes the firing row `("triage/run-triage/0 9 * * 1-5", 2026-01-10T09:00:00.000Z, run_id)`. Instances B and C tick at 09:00:05 and 09:00:09, make the same call, and get `None`. One run exists. Instance D starts at 09:00:40, looks back to 08:59:40, finds the same instant, and gets `None`.

## Internal Logic

**`new`**

1. For each entry in order (`index` from 0): `command ∈ commands`, `duty ∈ duties`, the cron splits into exactly five white-space separated fields, and `croner::Cron::new(cron).parse()` succeeds; else `BadEntry { index, reason }` naming which of the four failed. `entry_key(entry)` longer than 512 chars → `BadEntry`. Two entries with the same key → `BadEntry` ("Schedule entries <m> and <n> have the same duty, command and cron expression"). The Agent Configuration refuses these first; this guard keeps the timer from firing what it cannot name.
2. Keep the entries with their keys and parsed expressions. No connection opened; nothing written. An agent with no entry gets a timer that ticks and does nothing.

**`run_forever(stop)`**

3. `last = clock.now() − 60 s`. Log `timer_started` (INFO; `entries` = count).
4. Until `stop` is cancelled: one tick (steps 5–9), then wait `tick` or until `stop`, whichever first. The first tick is at once. On `stop`, return without a further tick.
5. **Tick.** `now = clock.now()`. For each entry, collect every due instant `d` with `last < d ≤ now`, evaluated in UTC (`croner` `iter_after(last)` taking items while `d ≤ now`).
6. For each due instant of each entry, oldest first:
   1. `now − d > 60 s` → do not fire; log `firing_late` (WARN; `entry`, `due`, `late_seconds`); nothing counted.
   2. Else `queue.enqueue_firing(key, Timestamp(d), agent_wide, &entry.command, &entry.arguments)`.
   3. `Ok(None)`: recorded already by another instance (or by an earlier call whose answer was lost). Nothing more.
   4. `Ok(Some(run))`: `INSERT INTO schedule_firings (entry_key, due, recorded_at, run_id) VALUES (…) ON CONFLICT DO NOTHING`; a failure of that insert is logged `firing_record_failed` (WARN; `entry`, `due`) and does not undo the run. `telemetry.record(TelemetryEvent::ScheduleFired { duty })`. Log `firing_queued` (INFO; `entry`, `due`, `run_id`, `thread_id`). Arguments are never logged.
7. `Err(e)` with `e.retryable()` (`DB_UNAVAILABLE`): log `timer_tick_failed` (WARN; `entry`, `code`) and end this tick without moving `last`; the next tick retries the same instants, and step 6.1 drops any that are by then more than 60 s old. Because the Run Queue's insert is one statement on a unique index, a call that was carried out and whose answer was lost returns `None` on retry and no second run starts.
8. `Err(e)` not retryable (`RUN_BAD_INPUT`): nothing was written; log `firing_not_queued` (ERROR; `entry`, `due`, `code`); continue with the next instant; this instant is not asked for again.
9. `last = now`.
10. The timer does not check whether a cycle of the duty is still executing, whether `STOP` is present or how many cycles ran today: those are controls the Rules Engine decides at the start of the queued run (ADR-098 item 4).


**Where it runs.** The program builds one `DutyTimer` in each instance started in the `runs` role, and in the single shape, from the loaded release's configuration and manifest, and runs it beside the Session Supervisor with the same stop token (02:242 diagram). No other role runs it; the check and run-once commands do not build it. An entry a new release removes stops firing when the last instance of the earlier release stops; an entry both releases hold fires once, because its key is the same.

## Data Model

Module `duty_timer`. New table; nothing to roll back before use; after use, restore the migrator's backup (SA-XC-07).

### SQLite (`MigrationStep` 1, `"schedule firings"`)

```sql
CREATE TABLE schedule_firings (
    entry_key   TEXT NOT NULL CHECK (length(entry_key) BETWEEN 1 AND 512),
    due         TEXT NOT NULL,                           -- RFC 3339 ms Z
    recorded_at TEXT NOT NULL,
    run_id      TEXT NOT NULL,                           -- the run the firing queued (Run Queue's id; no foreign key)
    PRIMARY KEY (entry_key, due)
);
```

### PostgreSQL (`MigrationStep` 1, `"schedule firings"`)

```sql
CREATE TABLE schedule_firings (
    entry_key   text        NOT NULL CHECK (char_length(entry_key) BETWEEN 1 AND 512),
    due         timestamptz NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT now(),
    run_id      uuid        NOT NULL,
    PRIMARY KEY (entry_key, due)
);
```

A row holds no content of a person. Kept for: until the organisation deletes rows (wrightrun `DutyTimer.md` §Data Model; the retention job does not reach this table). An entry firing every minute writes about 0.5 million rows a year; a row older than 60 s is never read by the timer.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| An entry names an unknown command or duty, has a cron that is not five valid fields, or shares its key with another entry | exit 1 from the start command; the instance never reports ready | `TIMER_BAD_ENTRY` | `error[TIMER_BAD_ENTRY]: Schedule entry 2 cannot be fired: the command "run-triag" is not a command of the package.` |
| The Run Queue's datastore does not answer `enqueue_firing` | logged; retried at the next tick while the instant is ≤ 60 s old | `DB_UNAVAILABLE` (Run Queue) | log `timer_tick_failed` with `code="DB_UNAVAILABLE"` |
| `enqueue_firing` refuses its arguments | logged ERROR; nothing written; not asked again | `RUN_BAD_INPUT` (Run Queue) | log `firing_not_queued` with `code="RUN_BAD_INPUT"` |
| A due instant noticed more than 60 s late | logged WARN; not fired | — | log `firing_late` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Duty Timer

  Background:
    Given a package whose commands include "run-triage" and whose recurring duties include "triage"
    And a configuration with the entry duty "triage", command "run-triage", arguments "queue=support", cron "0 9 * * 1-5"
    And a Run Queue and the duty_timer migrations on SQLite, or on a PostgreSQL 18 container where marked [pg]

  Scenario: [pg] Happy path — three instances, one firing, one run
    Given three DutyTimers on their own pools, each with a test clock at 2026-01-10T08:59:55Z
    When the clocks move to 2026-01-10T09:00:03Z and each timer ticks at the same instant
    Then run_records holds exactly 1 run started by ScheduleEntry "triage/run-triage/0 9 * * 1-5", command "run-triage", argument "queue=support", state queued, on a thread no other run is on
    And schedule_firings holds exactly 1 row with due 2026-01-10T09:00:00.000Z
    And the sum of wrightrun_schedule_firings_total{duty="triage"} over the three timers rose by 1

  Scenario: Each firing starts a new thread and the earlier thread is kept
    Given one DutyTimer
    When the clock passes 09:00 on January 2026 and 09:00 on January 2026 with a tick after each
    Then run_records holds 2 runs on 2 different threads

  Scenario: An instance that starts within a minute of a due instant fires it
    Given no timer ran at 2026-01-10T09:00:00Z
    When a DutyTimer starts with its clock at 2026-01-10T09:00:40Z
    Then its first tick is made before any wait
    And run_records holds exactly 1 run for that entry

  Scenario: Edge case — an instance starting exactly 60 seconds after the instant does not fire it
    Given no timer ran at 2026-01-10T09:00:00Z
    When a DutyTimer starts with its clock at 2026-01-10T09:01:00Z and ticks once
    Then run_records holds no run

  Scenario: Edge case — an instant noticed 90 seconds late is not fired
    Given a DutyTimer whose last tick was at 08:59:55
    When its next tick is made with the clock at 09:01:30
    Then run_records holds no run and a firing_late log line holds late_seconds 90

  Scenario: Edge case — a firing missed by more than a minute while no instance ran is not made up
    Given no DutyTimer ran at 2026-01-10T09:00:00Z
    When a DutyTimer starts at 2026-01-10T09:01:20Z and ticks until 12:00:00Z
    Then run_records holds no run and no firing_late line is written

  Scenario: Error path — the datastore does not answer one tick, the next tick queues the run
    Given a Run Queue whose enqueue_firing fails with DB_UNAVAILABLE at 09:00:03 and works afterwards
    When the timer ticks at 09:00:03 and 09:00:13
    Then run_forever has not returned
    And exactly 1 run was queued, at the second tick, for 09:00:00
    And one WARN line timer_tick_failed with code DB_UNAVAILABLE was written

  Scenario: Error path — a call carried out whose answer was lost starts no second run
    Given a Run Queue that carries out enqueue_firing and then reports DB_UNAVAILABLE at 09:00:03
    When the timer ticks at 09:00:03 and 09:00:13
    Then the second call returned None and run_records holds exactly 1 run

  Scenario: Error path — the datastore is away for more than a minute
    Given enqueue_firing fails with DB_UNAVAILABLE at every tick from 09:00:03 to 09:00:53
    When the timer ticks every 10 seconds until 09:01:03
    Then enqueue_firing was called 6 times and not at 09:01:03
    And a firing_late line for 09:00:00 holds late_seconds 63

  Scenario: Error path — a refused firing is logged and not asked again
    Given enqueue_firing fails with RUN_BAD_INPUT
    When the timer ticks after 09:00 and 10 seconds later
    Then enqueue_firing was called exactly once and an ERROR line firing_not_queued holds code RUN_BAD_INPUT

  Scenario: Error path — an entry naming an unknown command stops the start
    Given an entry with command "run-triag" built without the configuration check
    When DutyTimer::new is called
    Then it fails with code TIMER_BAD_ENTRY naming entry 0

  Scenario: Error path — two entries with the same key
    Given two entries that differ only in their arguments
    When DutyTimer::new is called
    Then it fails with code TIMER_BAD_ENTRY

  Scenario: A timer told to stop fires nothing further
    Given a DutyTimer with its clock at 08:59:58
    When stop is cancelled at 08:59:59 and the clock then passes 09:00
    Then run_forever has returned and run_records holds no run

  Scenario: Edge case — an agent with no schedule entry
    Given a configuration with no schedules
    When a DutyTimer runs 3 ticks
    Then no statement reached the datastore and run_forever returns within 1 second of stop
```

## Performance / Security / Observability

- **Performance targets:** one tick every 10 s per run instance, the first at once. A tick with nothing due sends no statement and takes under 5 ms for 100 entries. A tick with `k` due instants sends `k` `enqueue_firing` calls (p95 under 20 ms each on PostgreSQL) and at most `k` firing inserts. A firing's run is queued 10–11 s after its due instant while one run instance runs and the datastore answers, and at most 60 s after it otherwise. No connection is held between ticks.
- **Security:** the timer starts runs inside the agent and needs no token. It can queue only the commands the running release's configuration lists, with the arguments that configuration states; nothing a caller, an event or the agent supplies reaches it, and no agent tool reaches it. It logs entry keys, instants and identifiers, never arguments. Statements are bound.
- **Observability:** agent profile only. Counter (name unchanged, SA-OBS-02): `wrightrun_schedule_firings_total{duty}`, added when `enqueue_firing` returned a run, so the sum over instances is the number of firings whose run was queued (the Run Queue's `wrightrun_runs_queued_total{started_by="schedule"}` counts the same runs). Log events (target `ordin_duty_timer`): `timer_started`, `firing_queued`, `firing_late` (WARN), `firing_not_queued` (ERROR), `timer_tick_failed` (WARN), `firing_record_failed` (WARN). No trace span.

## Gaps

None.

## Amendment January 2026 — Built-in duties for knowledge agents (ADR-113)

> Source: decision 92; HLD revision 0.61.0, ADR-113 (knowledge agents: ordin in the agent profile reads Git repositories into the one memory graph in the background, on the agent profile's recurring duties). Counterpart: `components/KnowledgeReader.md` (new, January 2026), which implements the one built-in duty, and `components/OrdinServer.md` (amendment January 2026), which registers it.
> Every rule below adds to the sections above. An agent whose configuration has no `knowledge-agent` section registers no built-in duty, and the timer behaves exactly as before.

### Purpose (addition)

Besides the schedule entries of the release, the timer fires **built-in duties**: work that is ordin's own fixed code rather than a harness run, registered by the program. The one built-in duty is `knowledge_fetch`, the Knowledge Reader's cycle (fetch the knowledge agent's repositories and read what changed). A built-in duty fires on a fixed interval with a fixed offset, once across all instances, as a schedule entry does, and runs in-process on the instance that recorded the firing, with no run queued.

### Approach (addition)

The once-across-instances guard for a built-in duty is this crate's own `schedule_firings` primary key (entry key, due), written with `INSERT … ON CONFLICT DO NOTHING`: the instance whose insert wrote a row fires the duty; the others see no row written and do nothing. For schedule entries the Run Queue's unique index stays the guard and the firing row stays a report. The duty's work runs on a task of its own so a long cycle never delays the tick. Rejected: *a schedule entry whose command starts a harness run that fetches* — reading must be ordin's fixed code, not left to the model (ADR-107); *a crate edge from the timer to the Knowledge Reader* — the reader sits in build phase 8, above the timer's 3; the timer owns the trait and the program registers the implementation (the D3 pattern, ADR-100).

Assumptions (*chosen here*):

- **SA-DUTY-KA-01** A built-in duty's due instants are the instants `t` with `(t − offset)` a whole multiple of `interval` counted from the Unix epoch, in UTC. `interval` is 5 minutes to 24 hours; `offset` is below `interval`. The implementation fixes both from its configuration, so every instance computes the same instants.
- **SA-DUTY-KA-02** The entry key of a built-in duty is `"built-in/<kind>/<interval in seconds>"`, so a change of interval starts a new series of firing rows and an old firing is never taken for a new one. `run_id` of its firing row is a fresh UUIDv7 naming the firing, not a run (no run exists).
- **SA-DUTY-KA-03** While a firing of a duty is still running on this instance, a later firing recorded by this instance is not started (logged `built_in_skipped`); its row stays as the record that it was due. Overlap across instances is the duty's own concern (the Knowledge Reader holds a lease, KnowledgeReader.md SA-KRD-10).
- **SA-DUTY-KA-04** The 60-second lateness rule of step 6.1 applies to built-in duties unchanged: a firing noticed more than 60 s late is not made up; with an interval of at least 5 minutes the next one follows within the interval.

### Shared Context (additions)

From the Knowledge Reader (`ordin-knowledge-reader`), the one implementation, copied: `kind()` `"knowledge_fetch"`, `interval()` `knowledge-agent.fetch-interval-minutes` (default 15), `offset()` derived from the agent's name.

### Public Interface (additions)

```rust
/// Implemented by a module above this crate and registered by the program (ADR-100).
#[async_trait::async_trait]
pub trait BuiltInDuty: Send + Sync {
    /// Lower-case letters and underscores, 1–64 characters; unique among registered duties.
    fn kind(&self) -> &'static str;
    /// 300 s to 86,400 s.
    fn interval(&self) -> std::time::Duration;
    /// Below `interval()`, whole seconds.
    fn offset(&self) -> std::time::Duration;
    /// Called on the one instance whose firing row was written, on a task of its own.
    async fn fire(&self, due: Timestamp, stop: tokio_util::sync::CancellationToken) -> Result<(), BuiltInDutyError>;
}

#[derive(Debug, thiserror::Error)]
#[error("{code}: {message}")]
pub struct BuiltInDutyError { pub code: &'static str, pub retryable: bool, pub message: String }

impl DutyTimer {
    /// Before `run_forever`. Refused with TIMER_BAD_ENTRY when the kind is malformed or already
    /// registered, or the interval or offset is out of bounds.
    pub fn register_built_in(&mut self, duty: std::sync::Arc<dyn BuiltInDuty>) -> Result<(), TimerError>;
}
```

`TelemetryEvent::ScheduleFired { duty }` is recorded with `duty` = the kind, so `wrightrun_schedule_firings_total{duty="knowledge_fetch"}` counts built-in firings beside schedule entries.

**Called by.** `register_built_in`: the server program, agent profile, in each instance of the `runs` role and of the single shape, after `DutyTimer::new` and before `run_forever`, with the Knowledge Reader's `duty()` when the configuration has a `knowledge-agent` section (OrdinServer.md amendment January 2026).

### Internal Logic (additions)

**`register_built_in(duty)`**

1. `kind()` matches `^[a-z_]{1,64}$` and no registered duty has it; `interval()` 300–86,400 s; `offset() < interval()`; else `BadEntry { index: <entries + position>, reason }`. Keep the duty with its entry key (SA-DUTY-KA-02).

**`run_forever` — step 5 (addition).** In each tick, after the schedule entries, for each built-in duty collect every due instant `d` with `last < d ≤ now` (SA-DUTY-KA-01).

**Step 6 (addition), for each due instant of a built-in duty, oldest first:**

1. `now − d > 60 s` → not fired; `firing_late` as for an entry.
2. `INSERT INTO schedule_firings (entry_key, due, recorded_at, run_id) VALUES (<key>, d, now, <new UUIDv7>) ON CONFLICT DO NOTHING`. Rows written 0 → another instance fired it; nothing more. A datastore error → step 7's rule (`timer_tick_failed`, `last` not moved; the next tick tries the same instant).
3. Rows written 1 → when this instance still runs an earlier firing of the same duty, log `built_in_skipped` (INFO; `duty`, `due`) and do nothing more (SA-DUTY-KA-03). Else spawn a task that calls `duty.fire(d, child of the stop token)`; record `ScheduleFired { duty: kind }`; log `built_in_fired` (INFO; `duty`, `due`). The task logs `built_in_completed` (INFO; `duty`, `due`, `elapsed_ms`) on `Ok`, and `built_in_failed` (WARN; `duty`, `due`, `code`, `retryable`) on `Err`. A failed firing is not retried: the next due instant is the retry.
4. On `stop`, `run_forever` cancels the child token and waits up to 30 s for running built-in tasks before it returns; a task still running then is left to the program's shutdown (GracefulShutdown).

### Data Model (no change)

No new table or column: built-in firings use `schedule_firings` as it is, with the entry key of SA-DUTY-KA-02. A built-in duty firing every 15 minutes adds about 35,000 rows a year.

### Error Table (additions)

| Condition | Status | Code | Response Body |
|---|---|---|---|
| A built-in duty with a malformed or repeated kind, or an interval or offset out of bounds | `Err(permanent)` from `register_built_in`; the start command exits 1 | `TIMER_BAD_ENTRY` | `error[TIMER_BAD_ENTRY]: Schedule entry 3 cannot be fired: the built-in duty knowledge_fetch is registered twice.` |
| The datastore does not answer the firing insert | logged; retried at the next tick while the instant is ≤ 60 s old | `DB_UNAVAILABLE` | log `timer_tick_failed` with `code="DB_UNAVAILABLE"` |
| A built-in duty's `fire` returns an error | logged WARN; not retried; the next due instant fires | the duty's code (e.g. `KRD_MEMORY_UNAVAILABLE`) | log `built_in_failed` |

### Acceptance Criteria (additions)

```gherkin
Feature: Duty Timer — built-in duties

  Background:
    Given a counting BuiltInDuty "knowledge_fetch" with interval 15 minutes and offset 7 minutes 30 seconds

  Scenario: [pg] Happy path — three instances, one firing, fired once
    Given three DutyTimers on their own pools with the duty registered, clocks at 2026-01-10T10:07:25Z
    When the clocks move to 2026-01-10T10:07:33Z and each timer ticks
    Then fire was called exactly once across the three timers, with due 2026-01-10T10:07:30.000Z
    And schedule_firings holds one row with entry key "built-in/knowledge_fetch/900"
    And no run was queued in run_records

  Scenario: Edge case — a long firing is not started twice on one instance
    Given a single timer whose duty's fire takes 20 minutes
    When the clock passes two due instants 15 minutes apart
    Then fire was called once and a built_in_skipped line names the second due instant

  Scenario: Edge case — a changed interval starts a new series
    Given firing rows with entry key "built-in/knowledge_fetch/900"
    When the duty is registered with interval 30 minutes and the clock passes its next due instant
    Then a row with entry key "built-in/knowledge_fetch/1800" is written and fire is called

  Scenario: Error path — the same kind registered twice
    When register_built_in is called twice with kind "knowledge_fetch"
    Then the second call fails with TIMER_BAD_ENTRY

  Scenario: Error path — a failed firing is not retried before the next due instant
    Given the duty's fire returns an error with code KRD_MEMORY_UNAVAILABLE
    When the timer ticks every 10 seconds for 14 minutes after a due instant
    Then fire was called once and a built_in_failed line holds code KRD_MEMORY_UNAVAILABLE

  Scenario: An agent with no knowledge-agent section fires nothing built in
    Given no built-in duty is registered
    When the timer ticks through 10:07:30
    Then no schedule_firings row starts with "built-in/"
```

### Performance / Security / Observability (additions)

- **Performance targets:** a tick with no built-in instant due adds no statement; a due instant adds one insert (p95 under 20 ms on PostgreSQL). `fire` never runs on the tick's own task.
- **Security:** a built-in duty is fixed code the program registers; no configuration key, caller, event or agent tool can register one or start one.
- **Observability:** log events added: `built_in_fired`, `built_in_skipped`, `built_in_completed`, `built_in_failed` (WARN). The counter `wrightrun_schedule_firings_total{duty}` gains the label value `knowledge_fetch`.

### Gaps

None.
