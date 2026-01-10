# SPEC: Settings Store

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-settings-store` · **Build phase:** 2 · **Programs:** A
> **Dependencies:** Agent Configuration, Journal

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

The Settings Store keeps the values of a hosted agent's declared settings, one value per setting per scope (02:817–819, ADR-098 item 6 *Settings*, wrightrun ADR-028). A setting is declared by the package's declaration or by the agent configuration with a name, a kind, limits, a default and who may change it. Its value belongs to one channel or direct-chat scope and is changed while the agent runs — by a chat user through the chat's settings form and entry commands, or by an operator through the operator routes. The store checks every write against the declaration, writes it in one statement (a list edit in one short transaction that keeps the entries the running image does not read), reads the datastore on every use so that no instance holds a copy, returns the stored rules of rule-list settings for the start-of-run checks, and ignores a stored value or entry the running image does not understand (wrightrun A-38, ADR-067).

## Approach

One table, `setting_values`, one row per scope and setting, with a JSON value and a version; every write is one `INSERT … ON CONFLICT … DO UPDATE … WHERE … RETURNING version` statement (wrightrun `SettingsStore.md` §Data Model), implemented twice behind the storage trait `SettingsBackend` (`SqliteSettings`, `PgSettings`, SA-DB-01). Validation is pure code over the declaration and runs before the statement. Two checks need modules this crate may not depend on (2B-3: the Rules Engine for a rule's vocabulary, the Knowledge Service for a selection's sender list); each is a trait this crate owns, injected by the program (2B-1 rule 3). Error prefix: `SET_` (wrightrun's, kept because the operator routes return it, SA-ERR-01). Rejected: one row per scope with every setting in one JSON document — two people changing two settings of one scope at the same instant would need a merge of the whole document. Rejected: reading settings once at start — a value saved through one instance must decide the next run on another (wrightrun RFC 03, criterion 19).

### Reconciliation assumptions

- **SA-RECON-01** `rules(scope)` returns `(setting name, rule)` pairs because the Rules Engine's start-up and decision lines name the setting a rule came from (reconciliation row 69, ruling R6).
- **SA-RECON-02** The Settings Store offers no `settings_with_value`, settings form or rule-check of its own: the program composes those adapters from `all`, `get`, `check`, `set_many` and the Rules Engine's `check_setting_rule` (reconciliation rows 68, 70; O17, O18, O47).
- **SA-RECON-03** The declaration family is copied unchanged from the Agent Configuration (`Who`, `SettingDefault`, `SettingLookup`, `RuleListOutcome`, path `ordin_types::domain::agent_profile`); migrations are `MigrationStep` tuples (rulings R3, R7).

## Shared Context

### Shared types (Phase 2C, `ordin-types`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScopeId(pub uuid::Uuid);    // UUIDv7; the scopes themselves are kept by Retention and Erasure (02:826)

/// RFC 3339, UTC, millisecond precision, `Z` suffix.
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>);
```

Paging (2C `ordin_types::paging`, decision 80: C-15): `Page<T> { items, next_cursor: Option<Cursor> }`, `PAGE_SIZE = 100`. This crate does not page; the operator route pages the declared names.

### From Agent Configuration (types in `ordin_types::domain::agent_profile`, copied unchanged from AgentConfiguration.md; reconciliation rows 72, 590, 591)

The declaration type of the configuration's `settings[]` key, which wrightrun's configuration file defines (YAML keys are the field names with a hyphen for each underscore). The package declaration's `settings` use the same type; the program passes both lists. Each struct carries `#[serde(rename_all = "kebab-case", deny_unknown_fields)]`; each enum `#[serde(rename_all = "kebab-case")]`.

```rust
pub struct SettingDeclaration {
    pub name: String,                                    // ^[a-z0-9]+(-[a-z0-9]+)*$, ≤ 64
    pub kind: SettingKind,
    pub label: String,                                   // 1..=75
    #[serde(default)] pub who: Who,                      // default members
    #[serde(default)] pub default: Option<SettingDefault>,
    #[serde(default)] pub minimum: Option<f64>,          // number
    #[serde(default)] pub maximum: Option<f64>,          // number
    #[serde(default)] pub max_chars: Option<u32>,        // text (required for text)
    #[serde(default)] pub options: Vec<String>,          // selection, fixed list
    #[serde(default)] pub lookup: Option<SettingLookup>, // selection from a lookup
    #[serde(default)] pub lookup_source: Option<String>,
    #[serde(default)] pub max_options: Option<u32>,
    #[serde(default)] pub always_listed: Vec<String>,
    #[serde(default)] pub max_items: Option<u32>,        // list, rule-list (required), 1–1,000
    #[serde(default)] pub fields: Vec<ListField>,        // list
    #[serde(default)] pub line_form: Option<String>,     // list, rule-list
    #[serde(default)] pub allowed_outcomes: Vec<RuleListOutcome>,   // rule-list
    #[serde(default)] pub keyword_share: Option<f64>,    // rule-list, 0.5–1.0
    #[serde(default = "one_hundred")] pub rule_priority: i32,       // 100
}
pub enum SettingKind { Flag, Number, Text, Selection, List, RuleList }
#[derive(Default)] pub enum Who { #[default] Members, OperatorsOnly }
pub enum SettingLookup { KnowledgeSenders }
pub enum RuleListOutcome { Reply, Command, SingleCall, AttachAction, Delivery }
/// A YAML boolean, number, string or list of strings; `#[serde(untagged)]`.
pub enum SettingDefault { Flag(bool), Number(f64), Text(String), Items(Vec<String>) }
pub struct ListField { pub name: String, pub max_chars: u32, #[serde(default = "yes")] pub required: bool }
```

`SettingDefault` is turned into the stored JSON form by kind: `Flag(b)` → `b`, `Number(x)` → `x`, `Text(t)` → `t`, `Items(v)` → the array `v`.

### From the Journal (`ordin-journal`, copied from JournalAndArtefactStore.md; ruling R3)

`#[derive(Clone)] pub enum Datastore { Sqlite(sqlx::SqlitePool), Postgres(sqlx::PgPool) }`; `pub type MigrationStep = (i64, &'static str, &'static str)`; `pub struct ModuleMigrations { pub module: &'static str, pub sqlite: &'static [MigrationStep], pub postgres: &'static [MigrationStep] }`. The program passes this crate's `migrations()` to `MigrationRegistry::register`.

### Constants

| Constant | Value | Source |
|---|---|---|
| `MAX_SELECTION_KEYS` | 1,000 | wrightrun `SettingsStore.md` |
| `MAX_OPTION_ID_CHARS` | 128 | wrightrun `SettingsStore.md` |
| `MAX_RULE_REPLY_CHARS` | 4,000 | wrightrun `ReplyText` |
| `MAX_KEYWORDS` | 50 | wrightrun `KeywordCondition` |

No environment variable is read by this crate.

## Public Interface

```rust
// crate ordin-settings-store
use async_trait::async_trait;
use ordin_types::domain::agent_profile::{SettingDeclaration, SettingKind};
use ordin_types::ScopeId;

/// A setting's value in one scope (wrightrun's SettingValue; owned here, 02:818 amendment).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SettingValue {
    pub name: String,
    pub kind: SettingKind,
    /// flag: bool; number: number; text: string; selection: array of selected ids, sorted;
    /// list: array of {field: text}; rule-list: array of rule objects (hyphen keys).
    pub value: serde_json::Value,
    pub version: i64,          // 0 for a value never written
    pub is_default: bool,
}

/// Who writes: a chat member (identifier never logged or stored) or an operator (token subject).
#[derive(Debug, Clone)]
pub enum SettingWriter {
    Member { chat_user: String },
    Operator { subject: String },
}

/// Checks one rule of a rule-list setting against the rule vocabulary allowed in a setting
/// and returns its normalised JSON. Implemented by the program from the Rules Engine's
/// setting-rule check (wrightrun ControlEvaluator `check_setting_rule`).
pub trait SettingRuleCheck: Send + Sync {
    fn check(&self, rule: &serde_json::Value, declaration: &SettingDeclaration) -> Result<serde_json::Value, RuleRefused>;
}
#[derive(Debug, Clone)]
pub struct RuleRefused { pub message: String, pub place: String }

/// The sender identifiers of a knowledge source in a scope, most recently active first.
/// Implemented by the program from the Knowledge Service.
#[async_trait]
pub trait SelectionLookup: Send + Sync {
    async fn senders(&self, source: &str, scope: ScopeId) -> Result<Vec<String>, LookupFailed>;
}
#[derive(Debug, Clone)]
pub struct LookupFailed;

pub trait MetricsSink: Send + Sync {
    fn add(&self, name: &'static str, labels: &[(&'static str, &str)], by: u64);
}

#[async_trait]
pub trait SettingsStore: Send + Sync {
    fn declarations(&self) -> &[SettingDeclaration];
    async fn get(&self, scope: ScopeId, name: &str) -> Result<SettingValue, SettingsError>;
    async fn all(&self, scope: ScopeId) -> Result<std::collections::BTreeMap<String, SettingValue>, SettingsError>;
    fn check(&self, name: &str, value: &serde_json::Value, operator: bool) -> Result<(), SettingsError>;
    async fn set(&self, scope: ScopeId, name: &str, value: serde_json::Value, by: &SettingWriter,
                 version: Option<i64>) -> Result<i64, SettingsError>;
    async fn set_many(&self, scope: ScopeId, changes: Vec<(String, serde_json::Value)>, by: &SettingWriter,
                      versions: &std::collections::BTreeMap<String, i64>) -> Result<std::collections::BTreeMap<String, i64>, SettingsError>;
    async fn selection_options(&self, scope: ScopeId, name: &str) -> Result<Vec<String>, SettingsError>;
    async fn count(&self, scope: ScopeId) -> Result<u64, SettingsError>;
    /// Each stored rule with the name of the rule-list setting that holds it (reconciliation row 69).
    async fn rules(&self, scope: ScopeId) -> Result<Vec<(String, serde_json::Value)>, SettingsError>;
    async fn erase(&self, scope: ScopeId) -> Result<u64, SettingsError>;
}

pub struct DatastoreSettingsStore { /* backend, declarations, rule_check, lookup, metrics */ }
impl DatastoreSettingsStore {
    /// Fails with SET_INVALID on two declarations of one name or a faulty line form.
    pub fn new(datastore: &ordin_journal::Datastore,
               declarations: Vec<SettingDeclaration>,              // package's, then configuration's
               rule_check: std::sync::Arc<dyn SettingRuleCheck>,
               lookup: Option<std::sync::Arc<dyn SelectionLookup>>,
               metrics: std::sync::Arc<dyn MetricsSink>) -> Result<Self, SettingsError>;
}
pub fn migrations() -> ordin_journal::ModuleMigrations;   // module "settings_store"; version 1 is the Data Model

/// The single-line form of a list entry or a rule (pure; wrightrun §G).
pub fn entry_from_line(declaration: &SettingDeclaration, line: &str) -> Result<serde_json::Value, SettingsError>;
pub fn line_from_entry(declaration: &SettingDeclaration, entry: &serde_json::Value) -> Result<String, SettingsError>;
/// Lower-case, split on every character that is not alphanumeric (char::is_alphanumeric), drop empties.
pub fn words_of(text: &str) -> Vec<String>;

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("no setting is named {name}; declared: {declared}")] Unknown { name: String, declared: String },
    #[error("{message}")]                                         Invalid { message: String, place: String },
    #[error("the setting {name} is changed by operators only")]   NotAllowed { name: String },
    #[error("the list was changed since it was read; read it again")] VersionConflict { name: String },
    #[error("the settings could not be read or written")]         Unavailable,
}
impl SettingsError { pub fn code(&self) -> &'static str; pub fn retryable(&self) -> bool; }
```

**Called by.**

| Public item | Called by |
|---|---|
| `DatastoreSettingsStore::new`, `migrations` | the server program (OrdinServer, agent profile, every role) |
| `declarations`, `get`, `all`, `set` | Caller API routes 13–15 |
| `get`, `rules` | the program's `RulesHost` for the Rules Engine: `setting_value(scope: Option<&str>, name)` = `get(<the ScopeId that Retention and Erasure's `ScopeRegistry::resolve(parse_scope_text(scope))` gives; `None` → `agent_wide()`>, name).value`; `setting_rules(scope: &str)` = `rules(<the same ScopeId>)` with each rule parsed into the Rules Engine's `MessageRule` |
| `get`, `all`, `check`, `set_many`, `selection_options`, `entry_from_line`, `line_from_entry` | the program's `HostedAgentCore::{setting_text, settings_form, submit_settings}` for the Channel Gateway (members read and change a scope's settings in chat) |
| `get` | the program's `KnowledgeScopeSettings` for the Knowledge Service (`SettingsError::Unknown` → `SettingsReadError::Unknown`, `Unavailable` → `SettingsReadError::Unavailable`) |
| `all` | the program's `ScopeSettings::settings_with_value` for the Connection Binder: the names whose `is_default` is false (reconciliation row 70, N14) |
| `count`, `erase` | Retention and Erasure (erasure by scope) |
| `SettingRuleCheck` | implemented by the program: parse `rule` into the Rules Engine's `MessageRule` (a parse failure → `RuleRefused` with the parser's message and place), call `RulesEngine::check_setting_rule(agent_instance_id, &declaration.name, &<declaration.allowed_outcomes as their kebab-case names>, &rule) -> Result<(), RulesError>` (a refusal → `RuleRefused { message, place }` from the error), and return the parsed rule serialised again as the normalised JSON; the program supplies the hosted agent's `agent_instance_id` (reconciliation row 68, O47) |
| `SelectionLookup` | implemented by the program over the Knowledge Service's `senders` (O17) |
| `MetricsSink` | implemented by the program, forwarding to Telemetry (families 38–39) |

`words_of` splits on non-alphanumeric characters *(assumed here: wrightrun's InputValidation names the function but this workspace does not hold its body; the rule is the plain reading of "the split every keyword condition uses")*.

**Value of each kind** (wrightrun `SettingsStore.md` table, unchanged):

| Kind | `SettingValue.value` | What `set` accepts | Stored JSON | Default when none declared |
|---|---|---|---|---|
| `flag` | bool | bool | `true` / `false` | `false` |
| `number` | number | number (not bool), within `minimum`..`maximum` | number | `0` |
| `text` | string | string ≤ `max_chars` | string | `""` |
| `selection` | sorted array of selected ids | an object id → bool (only the ids changed), or an array of ids (the whole selection) | object `{"<id>": true\|false}` | the declared `default` array, else `[]` |
| `list` | array of `{field: text}` | the whole new array | array of objects | `[]` |
| `rule-list` | array of rule objects | the whole new array of rule objects (hyphen keys) | array of rule objects | `[]` |

A selected option is one whose stored state is `true`, or a `default` option with no stored state. The stored selection object holds `true` for a selected id and `false` only for an unselected `default` option; a write that sets any other id to `false` removes its key.

### Example

```rust
let scope = scope_registry_entry.id;     // the ScopeId of #help-desk
let rule = entry_from_line(&decl("kept-answers"),
    "reset vpn password laptop => Open the self-service portal and choose Reset.")?;
// rule == {"name":"r-1917b157949f","priority":100,"command":null,
//          "conditions":[{"kind":"keyword-share","words":["reset","vpn","password","laptop"],"share":0.75}],
//          "decides":{"kind":"reply","text":"Open the self-service portal and choose Reset."},"adds":[]}
store.set(scope, "kept-answers", json!([rule]), &SettingWriter::Member { chat_user: "U024BE7LH".into() }, Some(0)).await?;   // 1
store.set(scope, "hidden-senders", json!({"U07BOT": true}), &SettingWriter::Member { chat_user: "U024BE7LH".into() }, None).await?; // 1
store.get(scope, "hidden-senders").await?.value;   // ["U07BOT"]
store.set(scope, "kept-answers", json!([]), &SettingWriter::Member { chat_user: "U999".into() }, Some(0)).await;
// Err(SettingsError::VersionConflict { name: "kept-answers" }) — code SET_VERSION_CONFLICT
```

## Internal Logic

A sqlx error becomes `SettingsError::Unavailable` (log `settings_unavailable` WARN with `operation`). Every statement uses bound parameters.

**A. Construction (`new`).**

1. Keep the declarations in the order given (package's first, then configuration's). Two with one name → `Invalid { place: <name> }`.
2. For each declaration with `line_form`: it must hold `{<field>}` exactly once for each field — the declared `fields` of a list; `trigger` and `value` of a rule-list — and no other brace; two placeholders need at least one character between them; a rule-list with a line form states exactly one `allowed_outcomes` entry. Otherwise → `Invalid { place: "<name>.line-form" }`.
3. No datastore access; nothing logged.

**B. `get` and `all`.**

4. `get`: unknown name → `Unknown` (message lists the declared names).
5. `SELECT name, kind, value, version FROM setting_values WHERE scope_id = ? [AND name = ?]`.
6. For each declaration decide the value: no row → default, `version 0`, `is_default true`. A row whose `kind` differs from the declaration's, or whose JSON is not the shape of that kind → ignored: default with the row's version, `is_default true`, log `setting_ignored` (WARN; `scope_id`, `setting`, `reason`), `metrics.add("wrightrun_setting_values_ignored_total", [("setting", name)], 1)`. A list or rule-list row: each element checked on its own; an element that is not a valid entry is left out, logged with reason `entry` and counted once per element; it stays stored. Otherwise the stored value, `is_default false`.
7. `all`: a row whose name no declaration holds is left out and logged with reason `not-declared`. Returns one value per declaration.

**C. `check`, `set`, `set_many`.**

8. `check`: unknown name → `Unknown`; `who = operators-only` and `operator == false` → `NotAllowed`. Validate the value; each failure → `Invalid` naming what was expected and the place (setting, and for a list the entry's position):
   - `flag`, `number`, `text`: type and limits of the table above; a text holds no NUL.
   - `selection`: an object with keys of 1–128 chars and bool values, or an array of such strings; with a fixed list every id is in `options` or `always_listed`; more than 1,000 ids → `Invalid`.
   - `list`: ≤ `max_items` entries; each an object with exactly the declared fields; each field trimmed, ≤ the field's `max_chars`, non-empty when `required`; two equal entries → duplicate.
   - `rule-list`: ≤ `max_items` rules; each element is passed to `rule_check.check(rule, declaration)`; a `RuleRefused` becomes `Invalid { message, place }`; the returned JSON has `priority` set to the declaration's `rule_priority`; two rules with one `name` → `Invalid`.
9. `set`: call `check` with `operator = matches!(by, SettingWriter::Operator { .. })`. Then:
   - **selection:** read the stored row (statement of step 5 for that name); `stored` is its value when it is a JSON object of kind `selection`, else `{}`. An array value becomes the object that holds `true` for each listed id and `false` for every other key of `stored` and every other `default` option. `drop` = ids whose state is `false` and that are no `default` option; `value` = the object without them. If `keys(stored) ∪ keys(value) − drop` exceeds 1,000 → `Invalid` ("the selection would hold <n> identifiers, and at most 1000 fit"). Run the write statement with `merge = true`.
   - **list / rule-list with `version = Some(v)` (an edit):** one transaction: the *read of an edit* (`… FOR UPDATE` on PostgreSQL; `BEGIN IMMEDIATE` on SQLite); `kept` = the stored elements step 6 would leave out, in stored order; for a rule-list, a kept element whose `name` equals a rule in `value` is dropped from `kept`. If `len(value) + len(kept) > max_items` → roll back, `Invalid` ("the list holds <k> entries this version of the runtime does not read; with them it would hold <n>, and at most <max> fit"). Else write `value ++ kept` with `expected = v` and commit; with `kept` non-empty log `setting_entries_kept` (INFO; `scope_id`, `setting`, `count`).
   - **every other case:** the write statement with `merge = false`, `expected = None` (a list written with no version replaces the whole list, unreadable entries included — how an operator removes them).
10. No row returned → `VersionConflict { name }`; stored value unchanged.
11. A row returned: log `setting_changed` (INFO; `scope_id`, `setting`, `kind`, `version`, `role` = `member` | `operator`, and `by` = the subject **only** for an operator; a chat user's identifier is never logged and never stored); `metrics.add("wrightrun_setting_writes_total", [("setting", name), ("outcome", "saved")], 1)`; return the version. A refusal with `NotAllowed`, `Invalid`, `VersionConflict` counts with `outcome` = `not-allowed`, `invalid`, `version-conflict`. `Unknown` and `Unavailable` are not counted.
12. `set_many`: call `check` for every change in order; the first refusal is returned and nothing is written. Then one transaction that runs step 9's statement per change (with `expected` from `versions` for a list or rule-list named there, and the edit read inside the same transaction). A statement returning no row rolls back everything and returns `VersionConflict` for that name. Otherwise commit, log and count each change as step 11, and return name → version. Empty `changes` → empty map, no statement.

**D. `rules(scope)`.**

13. No rule-list declaration → `[]`, no datastore call. Else `SELECT name, kind, value FROM setting_values WHERE scope_id = ? AND name IN (…)`. For each rule-list declaration in declared order, read the row as step 6; set each rule's `priority` to the declaration's `rule_priority`; `rule_check` is not called on a read (it ran when the rule was written). Return `(setting name, rule)` pairs in declaration order, then stored order. Nothing is cached.

**E. `count` and `erase`.**

14. `count`: `SELECT count(*) FROM setting_values WHERE scope_id = ?`. `erase`: `DELETE FROM setting_values WHERE scope_id = ?`; return the rows deleted; log `settings_erased` (INFO; `scope_id`, `count`).

**F. `selection_options(scope, name)`.**

15. Unknown → `Unknown`; kind other than selection → `Invalid`. Candidates: with `lookup = knowledge-senders`, `lookup.senders(lookup_source, scope)` (no lookup given → `Invalid` "no lookup was given"; `LookupFailed` → `Unavailable`); with a fixed list, `options` in declared order. Result: each `always_listed` id in order, then the candidates not yet listed, cut to `max_options` when stated.

**G. `entry_from_line` / `line_from_entry`** (pure).

16. Only for a declaration with `line_form`; otherwise `Invalid { place: "<name>.line-form" }` ("the setting has no line form"). The form is `L0 {f1} L1 … {fn} Ln`; the trimmed line must start with `L0` and end with `Ln`; the first occurrence of `L1` ends the first field, the first `L2` after it the second, and the last field is the rest; a missing literal → `Invalid` whose message repeats the form and never the line. Fields are trimmed.
17. For a list: the object of the fields, validated as step 8. For a rule-list: keywords = distinct `words_of(trigger)` in order of first appearance (1–50 else `Invalid`); condition = `{"kind":"keyword-share","words":…,"share":keyword_share}` when the declaration states a share, else `{"kind":"keywords","words":…,"match":"all"}`; the outcome of the one allowed kind built from `value` (`{"kind":"reply","text":…}`, `{"kind":"command","command":…}`, `{"kind":"single-call","command":…}`, `{"kind":"attach-action","action":…}`, `{"kind":"delivery","to":"asker"|"thread"}`); `name` = `"r-"` + first 12 hex chars of SHA-256 of the keywords joined by one space (`sha2` crate, `memory/Cargo.toml:44`); `priority` = `rule_priority`; `command` = null; `adds` = `[]` (the delivery and attach-action kinds go in `adds`, the others in `decides`).
18. `line_from_entry` writes the form back; a rule with more than one condition or a pattern condition is written as its `name` alone.

## Data Model

Module `settings_store`. A new table; nothing to roll back before first use; after use, rollback is restoring the migrator's backup (SA-XC-07).

### SQLite (`MigrationStep` 1, `"settings store"`)

```sql
CREATE TABLE setting_values (
    scope_id        TEXT    NOT NULL,                                     -- ScopeId (UUIDv7); no foreign key: scopes belong to Retention and Erasure
    name            TEXT    NOT NULL CHECK (length(name) BETWEEN 1 AND 64),
    kind            TEXT    NOT NULL,                                     -- no CHECK: a later image may add a kind (wrightrun A-38)
    value           TEXT    NOT NULL,                                     -- JSON
    version         INTEGER NOT NULL DEFAULT 1 CHECK (version >= 1),
    updated_at      TEXT    NOT NULL,                                     -- RFC 3339 ms Z
    updated_by_role TEXT    NOT NULL CHECK (updated_by_role IN ('member','operator')),
    PRIMARY KEY (scope_id, name)
);
```

### PostgreSQL (`MigrationStep` 1, `"settings store"`)

```sql
CREATE TABLE setting_values (
    scope_id        uuid        NOT NULL,
    name            text        NOT NULL CHECK (char_length(name) BETWEEN 1 AND 64),
    kind            text        NOT NULL,
    value           jsonb       NOT NULL,
    version         bigint      NOT NULL DEFAULT 1 CHECK (version >= 1),
    updated_at      timestamptz NOT NULL DEFAULT now(),
    updated_by_role text        NOT NULL CHECK (updated_by_role IN ('member','operator')),
    PRIMARY KEY (scope_id, name)
);
```

The write statement (PostgreSQL; wrightrun's, with `scope_id` for `(connection, channel)`):

```sql
INSERT INTO setting_values AS s (scope_id, name, kind, value, version, updated_at, updated_by_role)
SELECT $1, $2, $3, $4::jsonb, 1, now(), $5
WHERE $6::bigint IS NULL OR $6::bigint = 0
   OR EXISTS (SELECT 1 FROM setting_values e WHERE e.scope_id = $1 AND e.name = $2)
ON CONFLICT (scope_id, name) DO UPDATE
SET kind = EXCLUDED.kind,
    value = CASE WHEN $7 AND jsonb_typeof(s.value) = 'object'
                 THEN (s.value || EXCLUDED.value) - $8::text[] ELSE EXCLUDED.value END,
    version = s.version + ((CASE WHEN $7 AND jsonb_typeof(s.value) = 'object'
                 THEN (s.value || EXCLUDED.value) - $8::text[] ELSE EXCLUDED.value END) IS DISTINCT FROM s.value)::int,
    updated_at = now(),
    updated_by_role = EXCLUDED.updated_by_role
WHERE $6::bigint IS NULL OR s.version = $6::bigint OR s.value = EXCLUDED.value
RETURNING s.version;
-- $6 expected version (NULL for non-list kinds), $7 merge (selection), $8 keys to drop
```

SQLite runs the same logic with `json_patch` replaced by code: inside `BEGIN IMMEDIATE` the backend reads the row, computes the merged value and the new version in Rust with the same rules, and writes it with `INSERT … ON CONFLICT (scope_id, name) DO UPDATE SET … WHERE version = :expected OR :expected IS NULL OR value = :value RETURNING version` *(chosen here: SQLite's JSON functions do not remove a list of keys in one expression; the single writer makes the read-then-write atomic)*.

The table holds no identifier of a person (`by` is not stored), so an erasure by person has nothing to delete here.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| The name is not a declared setting | `Err(permanent)` | `SET_UNKNOWN` | `SettingsError::Unknown` — "no setting is named reply-tone; declared: hidden-senders, kept-answers" |
| The value breaks the declaration (type, limit, unknown option, duplicate entry, refused rule, line that does not fit the form, faulty line form, duplicate declaration, no line form, more than 1,000 selection keys, list too long with kept entries) | `Err(permanent)` | `SET_INVALID` | `SettingsError::Invalid` — "a pattern must have 1 to 200 characters" (place `kept-answers/r-1917b157949f`) |
| An `operators-only` setting written by a member | `Err(permanent)` | `SET_NOT_ALLOWED` | `SettingsError::NotAllowed` — "the setting escalation-contact is changed by operators only" |
| A list edit against an older version | `Err(permanent)` | `SET_VERSION_CONFLICT` | `SettingsError::VersionConflict` — "the list was changed since it was read; read it again" |
| A statement fails, no connection can be had, or the selection lookup fails | `Err(retryable)` | `SET_UNAVAILABLE` | `SettingsError::Unavailable` — "the settings could not be read or written" |
| A stored row or entry this image does not understand | not an error | — | log `setting_ignored`; the read returns the default or the other entries |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Settings Store

  Background:
    Given the settings_store migrations applied to SQLite and, where marked [pg], to a PostgreSQL 18 container
    And the declarations "hidden-senders" (selection, lookup knowledge-senders, max-options 10, always-listed [agent]),
        "kept-answers" (rule-list, max-items 50, line-form "{trigger} => {value}", allowed-outcomes [reply], keyword-share 0.75),
        "search-results" (number, 1 to 20, operators-only) and "reply-tone" absent
    And a SettingRuleCheck that accepts keyword, keyword-share and reply rules
    And scope S is a channel scope id

  Scenario: Happy path — a member saves a kept answer and reads it back
    When entry_from_line(kept-answers, "reset vpn password laptop => Open the self-service portal and choose Reset.") is called
    And set(S, "kept-answers", [that rule], Member "U024BE7LH", version 0) is called
    Then it returns 1
    And rules(S) returns one pair ("kept-answers", rule named "r-1917b157949f" with priority 100)
    And no log line holds "U024BE7LH"

  Scenario: Edge case — a value never written reads as the default
    When get(S, "search-results") is called
    Then it returns value 0, version 0 and is_default true

  Scenario: Selections merge and an option set to false leaves no key
    Given hidden-senders stored as {"a": true}
    When set(S, "hidden-senders", {"b": true}) and then set(S, "hidden-senders", {"a": false}) are called
    Then the stored object is {"b": true}
    And get returns ["b"]

  Scenario: Error path — a member writes an operators-only setting
    When set(S, "search-results", 8, Member "U1", None) is called
    Then it fails with code SET_NOT_ALLOWED
    And wrightrun_setting_writes_total{setting="search-results",outcome="not-allowed"} rose by 1

  Scenario: Error path — a number outside its limits
    When set(S, "search-results", 21, Operator "ops-1", None) is called
    Then it fails with code SET_INVALID and the stored value is unchanged

  Scenario: Error path — a list edit against an older version
    Given kept-answers at version 5
    When set with a different list and version 4 is called
    Then it fails with code SET_VERSION_CONFLICT
    And the same list with version 4 is accepted and the version stays 5

  Scenario: Error path — an unknown setting
    When get(S, "reply-tone") is called
    Then it fails with code SET_UNKNOWN and the message lists hidden-senders, kept-answers, search-results

  Scenario: Edge case — an entry this image does not read is kept by an edit
    Given kept-answers stored with two valid rules and one rule whose condition kind is "future-kind" at version 4
    When get(S, "kept-answers") is called
    Then it returns the two rules and logs setting_ignored with reason "entry"
    When set with the two rules plus a third and version 4 is called
    Then the stored array holds the three rules followed by the unreadable one, at version 5

  Scenario: [pg] Two list edits at the same instant: one wins, one conflicts
    Given kept-answers at version 3
    When two stores each call set with a different list and version 3 at the same instant
    Then one returns 4 and the other fails with code SET_VERSION_CONFLICT

  Scenario: set_many writes all or nothing
    When set_many(S, [("search-results", 5), ("kept-answers", [...])], Operator "ops-1", {"kept-answers": 9}) is called and kept-answers is at version 2
    Then it fails with code SET_VERSION_CONFLICT and search-results is still its earlier value

  Scenario: Selection options put always-listed first and cut to max-options
    Given the lookup returns 14 senders for S
    When selection_options(S, "hidden-senders") is called
    Then it returns "agent" followed by the 9 most recently active senders

  Scenario: Erase removes every value of one scope only
    Given 3 values in S and 2 in scope T
    When count(S) is called it returns 3
    When erase(S) is called it returns 3
    Then all(T) still returns the 2 stored values

  Scenario: Error path — the datastore does not answer
    Given the datastore is closed
    When get(S, "search-results") is called
    Then it fails with code SET_UNAVAILABLE and retryable() is true
```

## Performance / Security / Observability

- **Performance targets:** a read of one setting and a write each one statement on the primary key; a selection write two statements; a list edit two statements in one transaction. p95 below 5 ms on PostgreSQL and below 3 ms on SQLite (wrightrun target; E-61 measured 1–2 ms for single-row statements). `rules` is one statement per run start in a scope. Stored bounds: `max_items` ≤ 1,000 entries per list; selection ≤ 1,000 keys of ≤ 128 chars. Nothing is held in memory between calls.
- **Security:** the store checks who may change a setting from the declaration and the writer kind; it authenticates nobody (the Caller API verifies tokens; the Slack Channel Adapter takes the user from Slack). Every value is validated before it is stored; a rule's pattern is checked by the injected `SettingRuleCheck`, which bounds and compiles it with the `regex` crate (ADR-098 item 4), so a stored rule cannot slow every run of its scope. Values, list entries and rule texts are never put in a log line or a metric label; a chat user's identifier is never logged or stored. No agent tool reaches this crate.
- **Observability:** agent profile only. Counters (names unchanged, SA-OBS-02): `wrightrun_setting_writes_total{setting, outcome}` (`saved`, `invalid`, `not-allowed`, `version-conflict`), `wrightrun_setting_values_ignored_total{setting}`. Log events (target `ordin_settings_store`): `setting_changed`, `setting_ignored` (WARN), `setting_entries_kept`, `settings_erased`, `settings_unavailable` (WARN). No trace span.

## Gaps

None.
