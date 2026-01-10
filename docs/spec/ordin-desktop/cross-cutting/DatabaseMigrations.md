# SPEC: Database Migrations

> **Spec set:** ordin-desktop · **Phase:** 4 — Cross-cutting · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-106; promoted to Approved by the user January 2026) · amended January 2026 (ADR-108; promoted to Approved by the user January 2026) · amended January 2026 (ADR-109; promoted to Approved by the user January 2026) · amended January 2026 (ADR-110; promoted to Approved by the user January 2026) · amended January 2026 (ADR-115) · **Mode:** greenfield · **derivedFromHld:** 0.63.0 · amended January 2026 (ADR-111; promoted to Approved by the user January 2026) · amended January 2026 (ADR-112, ADR-113; promoted to Approved by the user January 2026)
> **Implemented in:** `ordin-journal` (`MigrationRegistry`, `ModuleMigrations`, `MigrationStep`, `ApplyMode`, `MigrationReport`, table `schema_migrations`); each table-owning crate supplies its own steps; the two programs (`apps/desktop`, `apps/server`) register and apply them.

## Table of contents

- [Purpose](#purpose)
- [Approach](#approach)
- [Applicability by program](#applicability-by-program)
- [Shared Context](#shared-context)
- [Public Interface](#public-interface)
- [Internal Logic](#internal-logic)
- [Data Model](#data-model)
- [Error Table](#error-table)
- [Acceptance Criteria (Gherkin)](#acceptance-criteria-gherkin)
- [Performance, Security, Observability](#performance-security-observability)
- [Modules that register migrations](#modules-that-register-migrations)
- [Defects found while compiling this spec](#defects-found-while-compiling-this-spec)
- [Gaps](#gaps)

## Purpose

Fixes one rule for every schema change in ordin: how a module declares its tables' migrations, how they are numbered, when they run, what is copied before they run, what happens when a database is newer than the program, and how PostgreSQL is migrated in the agent profile's scaled shape. Every module owns its own tables (2B-1 rule 5; ADR-096 item 1) but no module migrates its own tables on its own; the Journal's registry applies all of them in one transaction (07 — *Migrations*; SA-XC-07).

## Approach

One registry in the datastore-owning crate (`ordin-journal`), fed by each table owner's `migrations() -> ModuleMigrations`: plain `(version, description, SQL)` tuples per engine, applied by ordin's own loop over `sqlx::raw_sql` inside one transaction, with a bookkeeping table `schema_migrations` keyed by (module, version) and a SHA-256 checksum per step. Forward-only; rollback is restoring the backup copy taken immediately before. Rejected: `sqlx::migrate::Migrator` per crate, because each would keep its own `_sqlx_migrations` table and could not be applied in one transaction across modules (Journal spec, reconciliation R3: "not `sqlx::migrate::Migrator`"); down migrations, because a desktop cannot be relied on to run them and the backup copy restores the exact earlier state.

## Applicability by program

| Program | Applies | How | Source |
|---|---|---|---|
| Desktop (D) | Yes | At every start, before any module that owns tables is constructed: `Datastore::open` (SQLite `<data root>/journal.db`) → `register` every module → `apply(ApplyMode::ApplyPending)` → `Journal::start`. Backup copy before any pending step. | SA-XC-07; OrdinDesktop.md wiring row 3.1 |
| Personal server (P) | Yes | As the desktop, at `ordin-server start`; a database newer than the program stops it with exit status 78. | SA-XC-07; OrdinServer.md |
| Agent profile (A), single shape | Yes | As the personal server (SQLite under `ORDIN_DATA_DIR`), at `agent start` and `agent db apply`. | SA-XC-07; OrdinServer.md |
| Agent profile (A), scaled shape | Yes | PostgreSQL. Only the schema command `ordin-server agent db apply` applies steps (`ApplyMode::ApplyPending`), run by the Helm chart as a Job annotated `helm.sh/hook: pre-install,pre-upgrade` before a release's instances start. Instances call `apply(ApplyMode::CheckOnly)` and stop, never reporting ready, while anything is pending. Within one minor line a change only adds. No backup copy is made by ordin: the operator's database backup applies, and running the schema command is the operator's action (sql-safety layer rule: migrations against shared databases are user actions). | SA-XC-07; ADR-098 item 5; OrdinServer.md Helm values |

## Shared Context

Copied from the Journal and Artefact Store spec (*Migration registry*), unchanged:

```rust
/// (version, description, SQL). Versions start at 1 and increase by exactly 1 within a module.
pub type MigrationStep = (i64, &'static str, &'static str);

pub struct ModuleMigrations {
    /// `[a-z][a-z0-9_]{1,40}`: the crate name without `ordin-`, hyphens as underscores
    /// (for example `journal`, `organisation_profile`, `identity`).
    pub module: &'static str,
    pub sqlite: &'static [MigrationStep],
    /// Empty for a module that has no tables in the scaled shape.
    pub postgres: &'static [MigrationStep],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyMode {
    /// Desktop, personal server, single shape, and the scaled shape's schema command.
    ApplyPending,
    /// Scaled-shape instances: refuse to start when anything is pending (ADR-098 item 5).
    CheckOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    pub applied: Vec<(String, i64)>,     // (module, version) applied by this call, in order
    pub backup_file: Option<std::path::PathBuf>,
}

pub struct MigrationRegistry { /* private */ }

impl MigrationRegistry {
    pub fn new() -> Self;
    /// Fails with JRNL_MIGRATION_DUPLICATE when the module is already registered or its
    /// versions are not 1, 2, 3, … without gaps.
    pub fn register(&mut self, migrations: ModuleMigrations) -> Result<(), JournalError>;
    pub async fn apply(&self, store: &Datastore, mode: ApplyMode) -> Result<MigrationReport, JournalError>;
}

/// This crate's own migrations; the program registers them first.
pub const MIGRATIONS: ModuleMigrations;

// JournalError variants used here
#[error("JRNL_SCHEMA_NEWER")] SchemaNewer { module: String, found: i64, known: i64 },
#[error("JRNL_SCHEMA_BEHIND")] SchemaBehind { pending: Vec<(String, i64)> },
#[error("JRNL_MIGRATION_DUPLICATE")] MigrationDuplicate { module: String },
#[error("JRNL_MIGRATION_CHANGED")] MigrationChanged { module: String, version: i64 },
#[error("JRNL_MIGRATION_FAILED")] MigrationFailed { module: String, version: i64 },
#[error("JRNL_BACKUP_FAILED")] BackupFailed,
```

Constants (Journal spec): backup copy `<application data root>/backups/journal-<YYYYMMDDTHHMMSSZ>.db`, newest 2 kept; PostgreSQL advisory lock key `7243100001`.

## Public Interface

Two forms of export, one per kind of table owner (Journal spec, *Called by*; OrdinDesktop.md SA-CLOSE-05):

1. **Every table owner that depends on the Journal** exports `pub fn migrations() -> ModuleMigrations` (crate `ordin-<name>`, module name `<name>` with hyphens as underscores).
2. **The two phase-1 table owners with no Journal edge** — Organisation Profile and Identity and Credentials (D4, R20) — export `pub const MODULE_NAME: &str` and `pub const SQLITE_MIGRATIONS: &[(i64, &str, &str)]`; the program builds `ModuleMigrations { module: MODULE_NAME, sqlite: SQLITE_MIGRATIONS, postgres: &[] }`.

The programs call, in order: `MigrationRegistry::new()`, `register(ordin_journal::MIGRATIONS)`, `register(..)` once for every other owner, then `apply(&datastore, mode)`, then `Journal::start`. The server program also registers its own `ModuleMigrations` under module name `server` (tables `server_profiles`, `server_settings`).

### Example

A desktop updated from a build whose `approval` module has steps 1 and 2 to one that adds step 3:

```
schema_migrations before: ("journal",1) ("approval",1) ("approval",2) …
apply(&store, ApplyPending)
  → backup: <data root>/backups/journal-20261020T091500Z.db  (older backups pruned to 2)
  → BEGIN IMMEDIATE; run ("approval",3) SQL; INSERT schema_migrations ("approval",3,"…","<sha256>","2026-01-10T09:15:00.412Z"); COMMIT
MigrationReport { applied: [("approval", 3)], backup_file: Some(".../journal-20261020T091500Z.db") }
```

## Internal Logic

Copied in substance from the Journal spec (*Internal Logic*, migration steps 1–9); this spec adds the numbering and destructive-change rules.

1. `register` keeps modules in registration order. It refuses a second registration of the same `module`, a version list that does not run 1, 2, 3 …, and a `module` name not matching `[a-z][a-z0-9_]{1,40}` → `JRNL_MIGRATION_DUPLICATE` (a build defect; a unit test registers every module of each program once).
2. Ensure `schema_migrations` exists (`CREATE TABLE IF NOT EXISTS`, outside the migration transaction).
3. Read every row of `schema_migrations`:
   - module not registered → `JRNL_SCHEMA_NEWER` (the database was written by a newer ordin that has a module this program lacks);
   - version greater than the highest registered version of that module → `JRNL_SCHEMA_NEWER`;
   - stored checksum (SHA-256 hex of the step's SQL for the current engine) differs from the registered step's → `JRNL_MIGRATION_CHANGED`.
   Each refusal is permanent, logged `journal.migrate.refused` with `module`, `version`; nothing is downgraded. The desktop shows "This copy of ordin is older than your saved work. Update ordin to open it." with **Send to IT**; the server stops with exit status 78.
4. Compute the pending list: for each registered module in registration order, every step above the module's highest applied version.
5. Pending empty → `MigrationReport { applied: [], backup_file: None }`.
6. `ApplyMode::CheckOnly` and pending non-empty → `JRNL_SCHEMA_BEHIND` with the list; the scaled-shape instance never reports ready.
7. **Backup (SQLite only).** `VACUUM INTO '<backup_dir>/journal-<YYYYMMDDTHHMMSSZ>.db'`; delete older backups so the newest two remain. Failure (disk full, folder not writable) → `JRNL_BACKUP_FAILED`; nothing is migrated. PostgreSQL: no copy; the operator's database backup applies.
8. **Apply in one transaction.** SQLite `BEGIN IMMEDIATE`; PostgreSQL `BEGIN` then `SELECT pg_advisory_xact_lock(7243100001)` so two schema commands never run together. For each pending step: `sqlx::raw_sql` (multi-statement), then insert its `schema_migrations` row. Any failure → `ROLLBACK`, `JRNL_MIGRATION_FAILED` with module and version, logged `journal.migrate.failed`; the SQLite file is unchanged and the backup copy stays. Commit.
9. Log `journal.migrate.applied` once per step with `module`, `version`; return the report.
10. **Numbering rule (this spec).** A step's version is the next integer in its module, never reused and never edited after release; a released step's SQL changing is detected by step 3 (`JRNL_MIGRATION_CHANGED`). File-style names that appear in component specs (for example `0001_server_profiles`) are the step's description, not its version.
11. **Engine rule (this spec).** A module that runs in the agent profile's scaled shape (2B-3 *Programs* column includes A) supplies a `postgres` slice with the same versions as its `sqlite` slice, so one version means one schema on both engines. A module that runs only on D or P, or whose data the scaled shape does not share (Catalogue Service, SA-RECON-06), passes an empty `postgres` slice.
12. **Destructive-change rule (this spec; sql-safety layer).** Within one minor line a step only adds (tables, nullable columns, columns with a default, indexes, triggers). A step that drops or narrows anything, or adds `NOT NULL` without a default to a populated table, is allowed only at a minor or major version change of ordin, and its component spec states the rollback consideration (restore the backup copy with the previous program; on PostgreSQL, the operator's backup). No step in version 1 of any module drops or narrows anything.
12a. **Widening rebuild (this spec; amended January 2026, ADR-106).** SQLite cannot change a `CHECK` list in place. A step that only adds values to a `CHECK` list (optionally with nullable columns) may build a copy of the table under a new name, copy every row, drop the old table and rename the copy, inside the transaction of step 8. Such a step is not a drop under rule 12, and may ship within a minor line, when all four hold: (a) every column of the old table is copied unchanged and no value is narrowed; (b) a table that refers to the rebuilt one with `ON DELETE CASCADE` is either rebuilt in the same step, copied and dropped before its parent, or copied to a temporary table and restored, so the drop deletes no child row; a reference without `ON DELETE` is checked at commit with `PRAGMA defer_foreign_keys = ON`; (c) indexes of the rebuilt tables are recreated in the step; (d) the component spec carries a migration test that starts from a database at the previous step, holding rows in the rebuilt table and in every table that refers to it, and proves after the step that every row count is unchanged and `PRAGMA foreign_key_check` returns nothing. Rollback stays the backup copy of step 7, restored with the previous program, which refuses the migrated file with `JRNL_SCHEMA_NEWER`. Steps of this form: Identity and Credentials step 2, Workspace Service step 3, Catalogue Service step 2, Handover Service step 3 (each in its component spec's ADR-106 amendment); Ordin Host step 2 (its ADR-108 amendment).
13. **Separate datastore exception.** In the agent profile only, the Catalogue Service's index is a rebuildable cache held in its own SQLite file `<ORDIN_DATA_DIR>/catalogue/index.sqlite` (backups in `<ORDIN_DATA_DIR>/catalogue/backups`) with its own `MigrationRegistry` and the same steps 1–9; a failure stops the server's start with `CAT_STORE_FAILED`.

## Data Model

```sql
-- SQLite (Journal spec, copied; created by MigrationRegistry::apply before any step)
CREATE TABLE IF NOT EXISTS schema_migrations (
    module       TEXT    NOT NULL CHECK (length(module) BETWEEN 2 AND 41),
    version      INTEGER NOT NULL CHECK (version >= 1),
    description  TEXT    NOT NULL,
    checksum     TEXT    NOT NULL CHECK (length(checksum) = 64),
    applied_at   TEXT    NOT NULL,
    PRIMARY KEY (module, version)
);

-- PostgreSQL (Journal spec, copied)
CREATE TABLE IF NOT EXISTS schema_migrations (
    module       text        NOT NULL CHECK (char_length(module) BETWEEN 2 AND 41),
    version      bigint      NOT NULL CHECK (version >= 1),
    description  text        NOT NULL,
    checksum     text        NOT NULL CHECK (checksum ~ '^[0-9a-f]{64}$'),
    applied_at   timestamptz NOT NULL,
    PRIMARY KEY (module, version)
);
```

Both forms are copied unchanged from the Journal spec (*Data Model*, SQLite and PostgreSQL). Rollback consideration: the table is append-only bookkeeping and is never dropped; restoring a backup copy restores it with the data it describes.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Database written by a newer ordin (unknown module or higher version) | `Err(permanent)`; server exit 78 | `JRNL_SCHEMA_NEWER` | `{"error":"This copy of ordin is older than your saved work. Update ordin to open it.","code":"JRNL_SCHEMA_NEWER","action":"send_to_it"}` |
| Scaled-shape instance started on a schema with pending steps | `Err(permanent)`; instance not ready | `JRNL_SCHEMA_BEHIND` | log only: `{"code":"JRNL_SCHEMA_BEHIND","pending":[["journal",2]]}` |
| A module registered twice, or versions not 1, 2, 3 … | `Err(permanent)` (build defect) | `JRNL_MIGRATION_DUPLICATE` | log only |
| An applied step's SQL differs from the registered step | `Err(permanent)` | `JRNL_MIGRATION_CHANGED` | log only; the desktop shows the `JRNL_SCHEMA_NEWER` sentence with **Send to IT** |
| A migration step fails; transaction rolled back | `Err(permanent)` | `JRNL_MIGRATION_FAILED` | `{"error":"ordin couldn't update its saved work. Your work is unchanged.","code":"JRNL_MIGRATION_FAILED","action":"send_to_it"}` |
| Backup copy before migrating cannot be written | `Err(permanent)` | `JRNL_BACKUP_FAILED` | `{"error":"ordin needs more free disk space to update.","code":"JRNL_BACKUP_FAILED","action":"try_again"}` |
| SQLite file fails `PRAGMA quick_check`, or cannot be created or opened | `Err(permanent)` | `JRNL_DATASTORE_OPEN_FAILED` | `{"error":"ordin can't open its saved work on this computer.","code":"JRNL_DATASTORE_OPEN_FAILED","action":"send_to_it"}` |
| Agent profile: the catalogue index file cannot be opened or migrated | start stops | `CAT_STORE_FAILED` | `error[CAT_STORE_FAILED]: …` |

The bodies above are the Journal spec's; the person-facing sentences are Localisation's at run time.

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Database migrations

  Scenario: Happy path — pending steps are applied after a backup, in one transaction
    Given schema_migrations holds ("journal", 1) and ("approval", 1)
    And the program registers approval steps 1 and 2
    When the desktop starts
    Then a file "<data root>/backups/journal-<UTC time>.db" exists
    And schema_migrations holds ("approval", 2)
    And the report lists ("approval", 2) as applied

  Scenario: Edge case — nothing pending writes no backup
    Given every registered step is already in schema_migrations
    When the personal server starts
    Then no backup file is written
    And the report lists no applied step

  Scenario: Edge case — the scaled shape never migrates from an instance
    Given PostgreSQL holds ("journal", 1) and the image registers journal steps 1 and 2
    When a run instance starts with ApplyMode CheckOnly
    Then the start fails with JRNL_SCHEMA_BEHIND
    And GET /readyz never answers ready
    And schema_migrations is unchanged

  Scenario: Error path — a newer database is refused, not downgraded
    Given schema_migrations holds ("journal", 7)
    And this program registers journal steps 1 to 3
    When migrations are applied
    Then the call fails with JRNL_SCHEMA_NEWER
    And no table is changed
    And the server exits with status 78

  Scenario: Error path — a failing step leaves the database unchanged
    Given approval step 2 holds SQL that fails
    When migrations are applied
    Then the call fails with JRNL_MIGRATION_FAILED for ("approval", 2)
    And schema_migrations does not hold ("approval", 2)
    And the backup file is kept

  Scenario: Edge case — a widening rebuild keeps every row (rule 12a; amended January 2026, ADR-106)
    Given a journal database at Workspace Service step 2 holding 3 source_links, 5 working_copy_items and 2 working_copy_unread rows
    When migrations are applied with Workspace Service step 3 registered
    Then source_links holds 3 rows, working_copy_items 5 and working_copy_unread 2
    And PRAGMA foreign_key_check returns no row
    And a source_links row of kind "jira_issue" can be inserted
```

## Performance, Security, Observability

- **Performance targets:** with nothing pending, step 2–5 cost one `CREATE TABLE IF NOT EXISTS` and one read of `schema_migrations` (under 20 ms on SQLite), inside A-03's 3 s start budget. A backup copy of a 1 GB journal (A: up to 100,000 runs before pruning) is bounded by disk speed; the desktop shows its starting state while it runs.
- **Security:** every step is fixed SQL compiled into the program — no step text comes from a profile, a package or the network; queries outside migrations are parameterised only (SA-DB-01). The backup copy lives under the application data root with the journal's own file permissions. In the scaled shape the database URL comes only from `ORDIN_DATABASE_URL_FILE` and is never printed, logged or placed in an error.
- **Observability:** `journal.migrate.applied` (`module`, `version`) per step, `journal.migrate.refused` and `journal.migrate.failed` with `module`, `version`; in the agent profile the start-up report names `JRNL_SCHEMA_BEHIND` and the pending list.

## Modules that register migrations

Enumerated from `components/*.md` (every spec holding `CREATE TABLE`, `migrations()`, `MIGRATIONS` or `SQLITE_MIGRATIONS`), checked against the Journal spec's *Called by* list. Every component spec states its module-name string (M-4 resolved); each follows the Journal's naming rule (crate name without `ordin-`, hyphens as underscores).

| Component | Crate | Module name | Export form | Tables | Programs (2B-3) |
|---|---|---|---|---|---|
| Journal and Artefact Store | `ordin-journal` | `journal` | `MIGRATIONS` (registered first) | `journal_meta`, `journal_entries`, `journal_events`, `journal_runs`, `run_steps`, `tool_requests`, `external_actions`, `artefacts`, `reports`, `dashboards`, `startup_reports` (+ `schema_migrations`) | D P A |
| Organisation Profile | `ordin-organisation-profile` | `organisation_profile` | `MODULE_NAME` + `SQLITE_MIGRATIONS` | `organisation_profiles`, `remote_agents` | D P (A: no tables) |
| Identity and Credentials | `ordin-identity` | `identity` | `MODULE_NAME` + `SQLITE_MIGRATIONS` | `credentials`, `credential_meta`; step 2 (amended January 2026, ADR-106): widening rebuild of both (rule 12a) to add kind `jira` | D P A |
| Approval Service | `ordin-approval` | `approval` | `migrations()` | `approvals`, `approval_buttons` | D P A |
| Localisation | `ordin-localisation` | `localisation` | `migrations()` | `locale_preferences`; step 2 (amended January 2026, ADR-109): `person_languages` and `person_messages`, created only (no existing row changes), SQLite only; rollback is the backup copy (dropping them loses the person's own wording) | D P A |
| Agent Folder Manager | `ordin-agent-folders` | `agent_folders` | `migrations()` | `agent_folders`, `session_folder_access`, `workspace_folder_grants` | D P A |
| Usage Planner | `ordin-usage-planner` | `usage_planner` | `migrations()` | `usage_settings`, `usage_daily`, `usage_work_history`, `reused_answers` | D P A |
| Knowledge Query Client | `ordin-knowledge-query-client` | `knowledge_query_client` | `migrations()` | `kq_connections`, `kq_connection_contents`; unique indexes `kq_connections_ws_url`, `kq_connections_ws_company` (*added January 2026, ADR-113*; version 1 on SQLite and PostgreSQL; the agent profile registers it and keeps no row); *amended January 2026, ADR-115:* version 2 adds `kq_connection_accounts` (connection, system, state, changed time; no account id, label or token), SQLite and PostgreSQL, additive | D P A |
| Run Queue | `ordin-run-queue` | `run_queue` | `migrations()` | `run_threads`, `run_records`, `run_events` | A |
| Settings Store | `ordin-settings-store` | `settings_store` | `migrations()` | `setting_values` | A |
| SharePoint Adapter | `ordin-sharepoint` | `sharepoint` | `migrations()` | `sharepoint_items`, `sharepoint_link_state`, `sharepoint_origin_changes` | D P |
| Server Connector | `ordin-server-connector` | `server_connector` | `MIGRATIONS` const (exempt, OrdinDesktop.md SA-CLOSE-05) | `server_connections`, `server_connection_backends`, `server_connection_pins` | D |
| Rules Engine | `ordin-rules` | `rules` | `migrations()` | `rules_approved_calls`, `rules_control_locks`, `rules_control_messages`, `rules_control_starts`, `rules_intake_places`, `rules_run_facts` | D P A |
| Brand | `ordin-brand` | `brand` | `migrations()` | `brand_packs` | D |
| Checker | `ordin-checker` | `checker` | `migrations()` | `check_runs`, `check_findings`, `executable_part_decisions`, `executable_part_subjects`, `install_candidate_parts` | D P A |
| Session Supervisor | `ordin-session-supervisor` | `session_supervisor` | `migrations()` | `runs`, `tasks` | D P A |
| Duty Timer | `ordin-duty-timer` | `duty_timer` | `migrations()` | `schedule_firings` | A |
| Remote API | `ordin-remote-api` | `remote_api` | `migrations()` | `desktop_pairings`, `desktop_pairing_codes`, `desktop_pairing_attempts`, `remote_api_listening` | P |
| File Service | `ordin-files` | `files` | `migrations()` | `places`, `file_versions`, `file_index`, `held_edits`, `published_copies`, `work_folder_files`, `file_index_fts` (FTS5) | D P A |
| Hook Relay | `ordin-hook-relay` | `hook_relay` | `migrations()` | `hook_late_decisions` | D P A |
| Crew Orchestrator | `ordin-crew-orchestrator` | `crew_orchestrator` | `migrations()` | `crew_runs`, `crew_step_states`, `rounds` | D P |
|  Memory Service | `ordin-memory-service` | `memory_service` | `migrations()` | `mem_*` tables (`mem_fact`, `mem_entity`, `mem_relationship`, `mem_source`, `mem_grant`, `mem_audit_log`, `mem_dead_letter`, `mem_fact_derived`, `mem_fact_entity`, `mem_idempotency_record`, `mem_maintenance_state`, `mem_quarantine`, `mem_work_job`, `mem_fact_fts` (FTS5)), `memory_fact_refs` *Amended January 2026 (ADR-103): a second, additive migration step of its team-knowledge amendment (see the component spec's amendment *Data Model*).* *Amended January 2026 (ADR-110): additive step adding `memory_fact_refs.read_folder_id` with its index, and `collection` on `mem_fact`, `mem_entity`, `mem_source` and `mem_relationship` (MemoryService.md amendment January 2026).* *Amended January 2026 (ADR-111): additive step "agentic memory": `memory_recalls`, `memory_recall_items`, `memory_made_from`, `memory_consolidations`, `memory_fact_refs.caller_kind`; and the memory component's M-9 SQL: `mem_relationship_fact`, `mem_entity_name`, `mem_relationship.replaces`, `mem_maintenance_state.names_backfilled_at`, three `mem_quarantine` columns, index `mem_rel_live`; entity names backfilled in Rust (MemoryService.md amendment January 2026 — agentic memory).* *Amended January 2026 (ADR-112, ADR-113): additive step "trusted pages and knowledge agents", SQLite and PostgreSQL: `memory_fact_refs.page_status` and `memory_fact_refs.page_tier` (nullable, CHECKed), `memory_recalls.wider` (TEXT / JSONB, nullable), table `memory_recall_wider`, index `memory_fact_refs_source_prefix`; the memory component gains nothing (MemoryService.md amendment January 2026 — knowledge agents).* *Amended January 2026 (ADR-115): additive step "versions of other sources", SQLite and PostgreSQL: `memory_recall_wider.commit_id` widened from 7–64 to 1–256 characters (a Confluence version number or a SharePoint `cTag`); SQLite by a table rebuild in one transaction that compares row counts before `DROP TABLE` and aborts on a difference, PostgreSQL by widening the column and swapping its check constraint in one transaction; widening only, no row lost; `memory_recalls.wider` JSON gains the key `unchecked` with no schema change (MemoryService.md amendment January 2026).* | D P A |
| Craft Toolkit | `ordin-craft-toolkit` | `craft` | `migrations()` | `craft_installations`, `file_type_map_entries`, `file_type_extensions` | D P |
| Confluence Adapter | `ordin-confluence` | `confluence` | `migrations()` | `confluence_items`, `confluence_link_state`, `confluence_origin_changes`, `confluence_folder_pages`, `confluence_publish_requests`, `confluence_ready_items` | D |
| Project Service | `ordin-projects` | `projects` | `migrations()` | `projects`, `coding_tasks`, `change_sets`, `change_set_entries`, `followed_originals`, `github_operation_settings`, `ide_choices`, `language_plugins`, `repository_fetch_state` | D P |
| Workspace Service | `ordin-workspaces` | `workspaces` | `migrations()` | `workspaces`, `workspace_places`, `source_links`, `folder_grants`, `email_links`, `email_grants`, `confluence_links`, `telegram_grants`, `slack_grants` (step 2, amended January 2026, ADR-102), `working_copy_items`, `working_copy_unread`; step 3 (amended January 2026, ADR-106): widening rebuild (rule 12a) of `source_links`, `working_copy_items` and `working_copy_unread` to add kind `jira_issue` and the nullable column `working_copy_items.origin_newer_at` | D P |
| Catalogue Service | `ordin-catalogue` | `catalogue` | `migrations()` (A: own SQLite file, Internal Logic 13) | `agent_repositories`, `repository_fetches`, `parts`, `part_index`, `part_dependencies`, `vocabulary_terms`, `withdrawn_list_entries`, `lifecycles`, `lifecycle_places`, `part_search` (FTS5); step 2 (amended January 2026, ADR-106): widening rebuild of `lifecycle_places` (rule 12a) to add place type `jira` | D P A |
| Push Service | `ordin-push` | `push` | `migrations()` | `pending_pushes`, `pending_push_items`, `pull_request_submissions`, `copy_operations`, `copy_audiences` *Amended January 2026 (ADR-103): a second, additive migration step of its team-knowledge amendment (see the component spec's amendment *Data Model*).* | D P |
| Knowledge Query Server | `ordin-knowledge-query-server` | `knowledge_query_server` | `migrations()` | `kq_cache`, `kq_cache_commits`, `kq_contents`, `kq_reader_status`, `kq_proxy_asks`, `kq_changes`, `kq_answer_sources`, `kq_answer_source_items`; indexes `kq_cache_used`, `kq_cache_commits_repo`, `kq_changes_at`, `kq_answer_sources_at`, unique `kq_contents_self` (*added January 2026, ADR-113*; version 1 on SQLite and PostgreSQL; registered only when the agent configuration has a `knowledge-agent` section); *amended January 2026, ADR-115:* version 2 adds `kq_linked_accounts` with index `kq_linked_accounts_used`, and purges `kq_cache` once (its rows change shape); additive, the purge loses only cached results | A |
| Source Access | `ordin-source-access` | `source_access` | `migrations()` | `src_notice_seen` (webhook replay guard, rows kept 24 hours) (*added January 2026, ADR-115*; version 1 on SQLite and PostgreSQL; registered only when the agent configuration has a `knowledge-agent` section) | A |
|  Knowledge Service | `ordin-knowledge` | `knowledge` | `migrations()` | `knowledge_pages`, `knowledge_page_updates`, `knowledge_items`, `knowledge_source_marks`, `knowledge_pages_fts`, `knowledge_items_fts_plain`, `knowledge_items_fts_porter` (FTS5) *Amended January 2026 (ADR-103): a second, additive migration step of its team-knowledge amendment (see the component spec's amendment *Data Model*).* *Amended January 2026 (ADR-104): step 4 adds `knowledge_page_names`, `knowledge_page_tags`, `knowledge_links`, `knowledge_suggestions`, `knowledge_suggestion_dismissals`, `knowledge_link_rewrites` and six columns of `knowledge_pages` (additive).* *Amended January 2026 (ADR-110): step 5 creates `knowledge_read_pages`, `knowledge_read_pages_fts`, `knowledge_read_page_names`, `knowledge_read_links` and `knowledge_read_pages_removed`.* *Amended January 2026 (ADR-112): step 6 "open knowledge format", SQLite only, additive: `knowledge_pages` gains `page_type`, `description`, `generated`, `verified`, `okf_tier` (NOT NULL DEFAULT 'unverified'), `okf_status`, `status_text`, `stale_after`; `knowledge_read_pages` gains the same eight and `concept_path`; indexes `knowledge_pages_stale_after`, `knowledge_read_pages_concept` (KnowledgeService.md amendment January 2026).* | D P A |
| Mail Service | `ordin-mail` | `mail` | `migrations()` | `email_drafts`, `sent_emails`, `mail_send_attempts`, `mail_settings`, `mail_link_check_state` | D P |
| Slack Service | `ordin-slack-service` | `slack_service` | `migrations()` | `slack_links`, `slack_connection`, `kept_slack_messages`, `kept_slack_messages_fts` (FTS5), `slack_member_names`, `slack_link_views`, `slack_drafts`, `slack_draft_runs`, `slack_posts`, `slack_rules`, `slack_rule_daily_counts`, `slack_reactions`, `slack_reaction_days` | D P |
| Team Knowledge Service | `ordin-team-knowledge` | `team_knowledge` | `migrations()` | `team_knowledge_bindings`, `team_knowledge_items`, `team_knowledge_problems`, `team_knowledge_shares` (*added January 2026, ADR-103*); step 2 (*added January 2026, ADR-110*, additive only): `team_knowledge_read_folders`, `team_knowledge_read_items`, `team_knowledge_read_problems`, `team_knowledge_extra_files`, `team_knowledge_adoptions`, `team_knowledge_descriptions`, `team_knowledge_stopped`, `team_knowledge_workspaces`, and five nullable columns on `team_knowledge_bindings` step 3 (*added January 2026, ADR-112*, additive only, SQLite): `team_knowledge_read_folders.bundle_kind` and table `team_knowledge_bundle_files`. | D P |
| Package Catalogue | `ordin-package-catalogue` | `package_catalogue` | `migrations()` | `agent_instances`, `agent_instance_choices`, `agent_instance_workspaces`, `agent_pending_updates` | D P |
| Ordin Host | `ordin-host` | `ordin_host` | `migrations()` | `ordin_conversations`, `ordin_messages`, `ordin_blocks`, `workflows`, `workflow_versions`, `workflow_drafts`, `crew_charters`, `voice_settings`; step 2 (amended January 2026, ADR-108): widening rebuild (rule 12a) of `ordin_conversations` (`ended_by` `put_away`, nullable `end_summary` and `continues_id`) and `ordin_messages` (`author` `system`, nullable `kind`), `ordin_blocks` copied and restored | D P |
| Home and Search | `ordin-home-search` | `home_search` | `migrations()` | `home_marks`, `changed_marks`, `remote_mark_poll`, `workspace_last_seen` | D P |
| Setup Assistant | `ordin-setup` | `setup` | `migrations()` | `setup_state`, `setup_checks`, `harness_attachments`, `setup_whats_new`, `setup_server_update` (*amended January 2026, ADR-114*) | D P |
| Retention and Erasure | `ordin-retention` | `retention` | `migrations()` | `scopes`, `erasure_records` | A |
| Knowledge Reader | `ordin-knowledge-reader` | `knowledge_reader` | `migrations()` | `kr_repositories`, `kr_documents`, `kr_shared_identities`, `kr_glossary_conflicts`, `kr_budget_days`, `kr_cycle_lease`; index `kr_documents_waiting` (*added January 2026, ADR-113*; version 1 on SQLite and PostgreSQL; registered only when the agent configuration has a `knowledge-agent` section); *amended January 2026, ADR-115:* step 2 "document sources" adds `kr_document_sources` and widens `kr_documents.outcome` by one value (`left_out`), SQLite by a counted table rebuild in one transaction, PostgreSQL by dropping and re-adding the check | A |
| Authoring Service | `ordin-authoring` | `authoring` | `migrations()` | `agent_drafts`, `part_drafts`, `draft_field_authors`, `rehearsals` | D P |
| Channel Gateway | `ordin-channel-gateway` | `channel_gateway` | `migrations()` | `channel_pairings`, `channel_outbox`, `card_references`, `card_deliveries`, `gateway_settings`, `chat_actions`, `chat_approval_deliveries`, `chat_events_seen`, `chat_pending_questions`, `chat_replies`, `chat_scope_notes` | P A |
| Handover Service | `ordin-handover` | `handover` | `migrations()` | `handovers`, `handover_places`, `sharepoint_place_reads`, `workspace_stages`; step 2 (amended January 2026, ADR-102): `handover_slack_posts`, `followed_handover_places`, and four columns added to `handovers` with constant defaults; step 3 (amended January 2026, ADR-106): widening rebuild of `handover_places` (rule 12a) to add place type `jira`, three columns added to `handovers` (nullable or defaulted), and `jira_pending_writes` | D P |
| Job Runner | `ordin-job-runner` | `job_runner` | `migrations()` | `jobs`, `job_timer_firings` | A |
| Telegram Adapter | `ordin-telegram-adapter` | `telegram_adapter` | `migrations()` | `telegram_pairing_codes`, `telegram_poll_state` | D P |
| Slack Channel Adapter | `ordin-slack-channel-adapter` | `slack_channel_adapter` | `migrations()` | `slack_pairing_codes` | P A |
| Trigger Service | `ordin-triggers` | `triggers` | `migrations()` | `triggers`, `trigger_events`, `trigger_daily_counts`, `trigger_notices`, `trigger_pending_deletes` | P |
| Telegram Service | `ordin-telegram-service` | `telegram_service` | `migrations()` | `telegram_links`, `telegram_link_views`, `telegram_bot_chats`, `telegram_bot_topics`, `telegram_drafts`, `telegram_draft_runs`, `telegram_post_requests`, `kept_telegram_messages`, `kept_telegram_messages_fts` (FTS5) | D P |
| ordin server program | `apps/server` | `server` | own `ModuleMigrations` | `server_profiles`, `server_settings` *Amended January 2026 (ADR-113): no schema change; the agent profile keeps the knowledge scope id as the `server_settings` row `knowledge_scope_id`.* | P A |

49 registrations in all (*amended January 2026, ADR-102: + `slack_service`; ADR-103: + `team_knowledge`; ADR-106: no new registration, the Jira Adapter owns no table; amended January 2026, ADR-113: + `knowledge_query_client`, `knowledge_query_server`, `knowledge_reader`; amended January 2026, ADR-115: + `source_access`*): 47 module crates (the Journal and the two phase-1 owners included) and the server program. Every step added on January 2026 (ADR-112, ADR-113) only adds: no table or column is dropped, narrowed or backfilled; rollback is the pre-migration backup copy (SA-XC-07). Tables marked (FTS5) are SQLite full-text virtual tables (`CREATE VIRTUAL TABLE … USING fts5`) created by the owning module's own migration steps, as its Data Model states. Every step added on January 2026 (ADR-115) only adds, except the Knowledge Reader's and the Memory Service's widening of one check each (no row lost, counted) and the Knowledge Query Server's one-time cache purge (cached results only); no table holds a reader list, a check outcome or a token; rollback is the pre-migration backup copy (SA-XC-07).

## Defects found while compiling this spec

| ID | Finding | Evidence | Recommended fix |
|---|---|---|---|
| M-1 *(resolved)* | The Journal spec's *Called by* list of modules whose migrations are registered omitted **Checker** and **Hook Relay**, both of which own tables and export `migrations()`. A program built from the Journal's list alone would never create `check_runs` or `hook_late_decisions`. | JournalAndArtefactStore.md:483; Checker.md:147; HookRelay.md (*Data Model*, `migrations()`) | Resolved: JournalAndArtefactStore.md:483 now lists Checker and Hook Relay |
| M-2 *(resolved)* | Of the modules in that list, the Journal named no `postgres` obligation per module; this spec's Internal Logic 11 makes the rule explicit (a module with A in its *Programs* column supplies both slices). | JournalAndArtefactStore.md *Migration registry* | Resolved: the `ModuleMigrations.postgres` field comment in JournalAndArtefactStore.md *Migration registry* now states the engine rule of Internal Logic 11 |
| M-3 *(resolved)* | Export form was inconsistent with the Journal's rule "every table-owning module that depends on the Journal exports `pub fn migrations()`": Agent Folder Manager exported `pub static MIGRATIONS`, Remote API `pub const MIGRATIONS`; OrdinDesktop.md SA-CLOSE-05 exempts only the Server Connector. | AgentFolderManager.md:141; RemoteApi.md:198; OrdinDesktop.md:44 | Resolved: both now export `pub fn migrations() -> ModuleMigrations` (AgentFolderManager.md:141, RemoteApi.md:198) |
| M-4 *(resolved)* | Table owners did not state their module-name string; the names were derived by the Journal's rule. | specs cited per row | Resolved: AgentFolderManager.md, UsagePlanner.md, RulesEngine.md, SessionSupervisor.md, DutyTimer.md, HookRelay.md, CrewOrchestrator.md, ConfluenceAdapter.md, CatalogueService.md, PackageCatalogue.md and TelegramService.md state the string beside `migrations()`; SettingsStore.md, KnowledgeService.md, RetentionAndErasure.md and JobRunner.md already did. |

## Gaps

None.
