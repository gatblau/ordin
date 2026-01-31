//! The DDL of the SQL storage (MemoryService spec, *Data Model* §B), one step per dialect.
//!
//! §B is the SurrealQL schema of `migrations/0001` to `0006` translated one for one, with a `mem_`
//! prefix so the tables sit apart from the host's own in a shared database file. The steps are handed
//! to the host's migrator through `SqlStorage::migrations`; the storage itself never runs DDL, so a
//! later change (M-6 onwards) appends a step here rather than editing this one.

/// The label of the one step §B defines. The host numbers the steps it applies; this names the step
/// after the SurrealDB migration it was translated from.
pub(super) const STEP_INIT: &str = "0001_init";

/// §B, SQLite — the statements as the spec writes them, its inline `--` comments aside.
pub(super) const SQLITE_INIT: &str = r#"
CREATE TABLE mem_fact (
    id TEXT PRIMARY KEY, tenant TEXT NOT NULL, owner_team TEXT, owner_user TEXT NOT NULL,
    content TEXT NOT NULL,
    content_text TEXT NOT NULL,
    entities TEXT NOT NULL,
    source_id TEXT, memory_class TEXT NOT NULL CHECK (memory_class IN ('episodic','semantic','consolidated')),
    fact_class TEXT, visibility TEXT NOT NULL CHECK (visibility IN ('user-private','team-shared','tenant-shared')),
    valid_from TEXT NOT NULL, valid_to TEXT, ingested_at TEXT NOT NULL,
    confidence REAL NOT NULL CHECK (confidence BETWEEN 0 AND 1),
    salience   REAL NOT NULL CHECK (salience BETWEEN 0 AND 1),
    stability  REAL NOT NULL CHECK (stability >= 0),
    pii_review INTEGER NOT NULL DEFAULT 0, supersedes TEXT, superseded_by TEXT,
    derived_from TEXT NOT NULL DEFAULT '[]', last_recalled_at TEXT,
    embedding BLOB, embedding_model TEXT
);
CREATE INDEX mem_fact_owner_user ON mem_fact (tenant, owner_user);
CREATE INDEX mem_fact_owner_team ON mem_fact (tenant, owner_team);
CREATE INDEX mem_fact_valid      ON mem_fact (tenant, valid_to);
CREATE TABLE mem_fact_entity (fact_id TEXT NOT NULL REFERENCES mem_fact(id) ON DELETE CASCADE,
    entity_id TEXT NOT NULL, PRIMARY KEY (fact_id, entity_id));
CREATE INDEX mem_fact_entity_entity ON mem_fact_entity (entity_id);
CREATE TABLE mem_fact_derived (fact_id TEXT NOT NULL REFERENCES mem_fact(id) ON DELETE CASCADE,
    derived_from TEXT NOT NULL, PRIMARY KEY (fact_id, derived_from));
CREATE INDEX mem_fact_derived_from ON mem_fact_derived (derived_from);
CREATE VIRTUAL TABLE mem_fact_fts USING fts5(content_text, content='mem_fact', content_rowid='rowid',
    tokenize='unicode61 remove_diacritics 2');
CREATE TRIGGER mem_fact_ai AFTER INSERT ON mem_fact BEGIN
  INSERT INTO mem_fact_fts(rowid, content_text) VALUES (new.rowid, new.content_text); END;
CREATE TRIGGER mem_fact_ad AFTER DELETE ON mem_fact BEGIN
  INSERT INTO mem_fact_fts(mem_fact_fts, rowid, content_text) VALUES ('delete', old.rowid, old.content_text); END;
CREATE TRIGGER mem_fact_au AFTER UPDATE OF content_text ON mem_fact BEGIN
  INSERT INTO mem_fact_fts(mem_fact_fts, rowid, content_text) VALUES ('delete', old.rowid, old.content_text);
  INSERT INTO mem_fact_fts(rowid, content_text) VALUES (new.rowid, new.content_text); END;

CREATE TABLE mem_entity (id TEXT PRIMARY KEY, tenant TEXT NOT NULL, owner_team TEXT, owner_user TEXT NOT NULL,
    canonical_name TEXT NOT NULL CHECK (length(canonical_name) BETWEEN 1 AND 512), aliases TEXT NOT NULL DEFAULT '[]');
CREATE INDEX mem_entity_name  ON mem_entity (tenant, canonical_name);
CREATE INDEX mem_entity_owner ON mem_entity (tenant, owner_user, owner_team);
CREATE TABLE mem_relationship (id TEXT PRIMARY KEY, tenant TEXT NOT NULL, owner_team TEXT, owner_user TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (length(kind) BETWEEN 1 AND 128), from_ent TEXT NOT NULL, to_ent TEXT NOT NULL,
    valid_from TEXT NOT NULL, valid_to TEXT, ingested_at TEXT NOT NULL,
    confidence REAL NOT NULL CHECK (confidence BETWEEN 0 AND 1), source_id TEXT);
CREATE INDEX mem_rel_from ON mem_relationship (tenant, from_ent);
CREATE INDEX mem_rel_to   ON mem_relationship (tenant, to_ent);
CREATE TABLE mem_source (id TEXT PRIMARY KEY, tenant TEXT NOT NULL, owner_team TEXT, owner_user TEXT NOT NULL,
    origin_ref TEXT NOT NULL, modification_marker TEXT,
    trust_signal REAL NOT NULL CHECK (trust_signal BETWEEN 0 AND 1));
CREATE TABLE mem_audit_log (id TEXT PRIMARY KEY, tenant TEXT NOT NULL, subject TEXT NOT NULL, operation TEXT NOT NULL,
    scope TEXT NOT NULL, outcome TEXT NOT NULL, token_jti TEXT NOT NULL, correlation_id TEXT NOT NULL,
    client TEXT, fact_class TEXT, at TEXT NOT NULL);
CREATE INDEX mem_audit_at ON mem_audit_log (tenant, at);
CREATE TRIGGER mem_audit_no_update BEFORE UPDATE ON mem_audit_log BEGIN SELECT RAISE(ABORT, 'append-only'); END;
CREATE TABLE mem_work_job (id TEXT PRIMARY KEY, tenant TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('extract_fact','re_embed_fact','re_read_source','consolidate','hard_delete')),
    payload TEXT NOT NULL, scope_team TEXT, scope_user TEXT NOT NULL, idempotency_key TEXT,
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    status TEXT NOT NULL CHECK (status IN ('pending','leased','done','dead_letter')),
    not_before TEXT NOT NULL, created_at TEXT NOT NULL, leased_until TEXT);
CREATE INDEX mem_work_job_claim ON mem_work_job (tenant, status, kind, not_before);
CREATE UNIQUE INDEX mem_work_job_idem ON mem_work_job (tenant, scope_team, scope_user, idempotency_key)
    WHERE idempotency_key IS NOT NULL;
CREATE TABLE mem_dead_letter (id TEXT PRIMARY KEY, tenant TEXT NOT NULL, kind TEXT NOT NULL, payload TEXT NOT NULL,
    scope TEXT NOT NULL, idempotency_key TEXT, attempts INTEGER NOT NULL, original_job_id TEXT NOT NULL,
    created_at TEXT NOT NULL, dead_lettered_at TEXT NOT NULL);
CREATE INDEX mem_dead_letter_scope ON mem_dead_letter (tenant, dead_lettered_at);
CREATE TABLE mem_quarantine (id TEXT PRIMARY KEY, tenant TEXT NOT NULL, owner_team TEXT, owner_user TEXT NOT NULL,
    content TEXT NOT NULL, entities TEXT NOT NULL, source_id TEXT,
    memory_class TEXT NOT NULL CHECK (memory_class IN ('episodic','semantic','consolidated')), fact_class TEXT,
    trust_score REAL NOT NULL CHECK (trust_score BETWEEN 0 AND 1),
    confidence REAL NOT NULL CHECK (confidence BETWEEN 0 AND 1), salience REAL NOT NULL CHECK (salience BETWEEN 0 AND 1),
    is_instruction_like INTEGER NOT NULL, pii_review INTEGER NOT NULL DEFAULT 0, idempotency_key TEXT,
    reason TEXT NOT NULL, quarantined_at TEXT NOT NULL);
CREATE INDEX mem_quarantine_owner ON mem_quarantine (tenant, owner_user, quarantined_at);
CREATE UNIQUE INDEX mem_quarantine_idem ON mem_quarantine (idempotency_key) WHERE idempotency_key IS NOT NULL;
CREATE TABLE mem_maintenance_state (tenant TEXT PRIMARY KEY CHECK (tenant <> ''),
    last_cycle_at TEXT, embed_model_version TEXT);
CREATE TABLE mem_idempotency_record (tenant TEXT NOT NULL, user TEXT NOT NULL, route TEXT NOT NULL,
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 255),
    response_body TEXT NOT NULL, status_code INTEGER NOT NULL, created_at TEXT NOT NULL, expires_at TEXT NOT NULL,
    PRIMARY KEY (tenant, user, route, idempotency_key));
CREATE INDEX mem_idem_expiry ON mem_idempotency_record (expires_at);
CREATE TABLE mem_grant (id TEXT PRIMARY KEY, tenant TEXT NOT NULL, fact TEXT NOT NULL,
    grantor_team TEXT, grantor_user TEXT NOT NULL, grantee TEXT NOT NULL,
    purpose TEXT NOT NULL CHECK (length(purpose) BETWEEN 1 AND 512), operation TEXT NOT NULL CHECK (operation = 'read'),
    valid_from TEXT NOT NULL, valid_to TEXT NOT NULL, revoked_at TEXT, evidence TEXT NOT NULL, created_at TEXT NOT NULL);
CREATE INDEX mem_grant_grantee ON mem_grant (tenant, grantee, revoked_at, valid_to);
CREATE INDEX mem_grant_fact    ON mem_grant (fact);
"#;

/// §B, PostgreSQL — the SQLite tables and columns with the type mapping §B states: JSON columns →
/// `JSONB`, timestamps → `TIMESTAMPTZ`, `REAL` → `DOUBLE PRECISION`, `BLOB` → `BYTEA`, `INTEGER`
/// booleans → `BOOLEAN`. The FTS5 table and its triggers become the generated `content_tsv` column and
/// its GIN index; the audit trail's append-only rule is a `BEFORE UPDATE` trigger plus a `REVOKE`.
///
/// The JSON columns are the ones holding a serialised value: `content`, `entities`, `derived_from`,
/// `aliases`, `payload`, the dead-letter and audit `scope` objects, and `response_body`.
// `mem_idempotency_record.user` is a reserved word in PostgreSQL, so the column is written `"user"`
// here (and in every statement); the identifier itself is unchanged. The trigger function carries the
// trigger's name, `mem_audit_no_update`.
// `erase_tenant` (ADR-018) must delete the tenant's audit rows. The revoke is made `FROM PUBLIC`, which
// binds every role but the table owner; the owner — the role that ran this step — keeps `DELETE`, so
// erasure works, and the trigger refuses an update from every role.
pub(super) const POSTGRES_INIT: &str = r#"
CREATE TABLE mem_fact (
    id TEXT PRIMARY KEY, tenant TEXT NOT NULL, owner_team TEXT, owner_user TEXT NOT NULL,
    content JSONB NOT NULL,
    content_text TEXT NOT NULL,
    entities JSONB NOT NULL,
    source_id TEXT, memory_class TEXT NOT NULL CHECK (memory_class IN ('episodic','semantic','consolidated')),
    fact_class TEXT, visibility TEXT NOT NULL CHECK (visibility IN ('user-private','team-shared','tenant-shared')),
    valid_from TIMESTAMPTZ NOT NULL, valid_to TIMESTAMPTZ, ingested_at TIMESTAMPTZ NOT NULL,
    confidence DOUBLE PRECISION NOT NULL CHECK (confidence BETWEEN 0 AND 1),
    salience   DOUBLE PRECISION NOT NULL CHECK (salience BETWEEN 0 AND 1),
    stability  DOUBLE PRECISION NOT NULL CHECK (stability >= 0),
    pii_review BOOLEAN NOT NULL DEFAULT FALSE, supersedes TEXT, superseded_by TEXT,
    derived_from JSONB NOT NULL DEFAULT '[]', last_recalled_at TIMESTAMPTZ,
    embedding BYTEA, embedding_model TEXT,
    -- A hyphen separates words here as in FTS5; `to_tsvector` would index `-7731` as a signed number.
    content_tsv TSVECTOR GENERATED ALWAYS AS (to_tsvector('simple', translate(content_text, '-', ' '))) STORED
);
CREATE INDEX mem_fact_owner_user ON mem_fact (tenant, owner_user);
CREATE INDEX mem_fact_owner_team ON mem_fact (tenant, owner_team);
CREATE INDEX mem_fact_valid      ON mem_fact (tenant, valid_to);
CREATE INDEX mem_fact_tsv ON mem_fact USING GIN (content_tsv);
CREATE TABLE mem_fact_entity (fact_id TEXT NOT NULL REFERENCES mem_fact(id) ON DELETE CASCADE,
    entity_id TEXT NOT NULL, PRIMARY KEY (fact_id, entity_id));
CREATE INDEX mem_fact_entity_entity ON mem_fact_entity (entity_id);
CREATE TABLE mem_fact_derived (fact_id TEXT NOT NULL REFERENCES mem_fact(id) ON DELETE CASCADE,
    derived_from TEXT NOT NULL, PRIMARY KEY (fact_id, derived_from));
CREATE INDEX mem_fact_derived_from ON mem_fact_derived (derived_from);

CREATE TABLE mem_entity (id TEXT PRIMARY KEY, tenant TEXT NOT NULL, owner_team TEXT, owner_user TEXT NOT NULL,
    canonical_name TEXT NOT NULL CHECK (length(canonical_name) BETWEEN 1 AND 512), aliases JSONB NOT NULL DEFAULT '[]');
CREATE INDEX mem_entity_name  ON mem_entity (tenant, canonical_name);
CREATE INDEX mem_entity_owner ON mem_entity (tenant, owner_user, owner_team);
CREATE TABLE mem_relationship (id TEXT PRIMARY KEY, tenant TEXT NOT NULL, owner_team TEXT, owner_user TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (length(kind) BETWEEN 1 AND 128), from_ent TEXT NOT NULL, to_ent TEXT NOT NULL,
    valid_from TIMESTAMPTZ NOT NULL, valid_to TIMESTAMPTZ, ingested_at TIMESTAMPTZ NOT NULL,
    confidence DOUBLE PRECISION NOT NULL CHECK (confidence BETWEEN 0 AND 1), source_id TEXT);
CREATE INDEX mem_rel_from ON mem_relationship (tenant, from_ent);
CREATE INDEX mem_rel_to   ON mem_relationship (tenant, to_ent);
CREATE TABLE mem_source (id TEXT PRIMARY KEY, tenant TEXT NOT NULL, owner_team TEXT, owner_user TEXT NOT NULL,
    origin_ref TEXT NOT NULL, modification_marker TEXT,
    trust_signal DOUBLE PRECISION NOT NULL CHECK (trust_signal BETWEEN 0 AND 1));
CREATE TABLE mem_audit_log (id TEXT PRIMARY KEY, tenant TEXT NOT NULL, subject TEXT NOT NULL, operation TEXT NOT NULL,
    scope JSONB NOT NULL, outcome TEXT NOT NULL, token_jti TEXT NOT NULL, correlation_id TEXT NOT NULL,
    client TEXT, fact_class TEXT, at TIMESTAMPTZ NOT NULL);
CREATE INDEX mem_audit_at ON mem_audit_log (tenant, at);
CREATE FUNCTION mem_audit_no_update() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'append-only'; END; $$;
CREATE TRIGGER mem_audit_no_update BEFORE UPDATE ON mem_audit_log
    FOR EACH ROW EXECUTE FUNCTION mem_audit_no_update();
REVOKE UPDATE, DELETE ON mem_audit_log FROM PUBLIC;
CREATE TABLE mem_work_job (id TEXT PRIMARY KEY, tenant TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('extract_fact','re_embed_fact','re_read_source','consolidate','hard_delete')),
    payload JSONB NOT NULL, scope_team TEXT, scope_user TEXT NOT NULL, idempotency_key TEXT,
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    status TEXT NOT NULL CHECK (status IN ('pending','leased','done','dead_letter')),
    not_before TIMESTAMPTZ NOT NULL, created_at TIMESTAMPTZ NOT NULL, leased_until TIMESTAMPTZ);
CREATE INDEX mem_work_job_claim ON mem_work_job (tenant, status, kind, not_before);
CREATE UNIQUE INDEX mem_work_job_idem ON mem_work_job (tenant, scope_team, scope_user, idempotency_key)
    WHERE idempotency_key IS NOT NULL;
CREATE TABLE mem_dead_letter (id TEXT PRIMARY KEY, tenant TEXT NOT NULL, kind TEXT NOT NULL, payload JSONB NOT NULL,
    scope JSONB NOT NULL, idempotency_key TEXT, attempts INTEGER NOT NULL, original_job_id TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL, dead_lettered_at TIMESTAMPTZ NOT NULL);
CREATE INDEX mem_dead_letter_scope ON mem_dead_letter (tenant, dead_lettered_at);
CREATE TABLE mem_quarantine (id TEXT PRIMARY KEY, tenant TEXT NOT NULL, owner_team TEXT, owner_user TEXT NOT NULL,
    content JSONB NOT NULL, entities JSONB NOT NULL, source_id TEXT,
    memory_class TEXT NOT NULL CHECK (memory_class IN ('episodic','semantic','consolidated')), fact_class TEXT,
    trust_score DOUBLE PRECISION NOT NULL CHECK (trust_score BETWEEN 0 AND 1),
    confidence DOUBLE PRECISION NOT NULL CHECK (confidence BETWEEN 0 AND 1), salience DOUBLE PRECISION NOT NULL CHECK (salience BETWEEN 0 AND 1),
    is_instruction_like BOOLEAN NOT NULL, pii_review BOOLEAN NOT NULL DEFAULT FALSE, idempotency_key TEXT,
    reason TEXT NOT NULL, quarantined_at TIMESTAMPTZ NOT NULL);
CREATE INDEX mem_quarantine_owner ON mem_quarantine (tenant, owner_user, quarantined_at);
CREATE UNIQUE INDEX mem_quarantine_idem ON mem_quarantine (idempotency_key) WHERE idempotency_key IS NOT NULL;
CREATE TABLE mem_maintenance_state (tenant TEXT PRIMARY KEY CHECK (tenant <> ''),
    last_cycle_at TIMESTAMPTZ, embed_model_version TEXT);
CREATE TABLE mem_idempotency_record (tenant TEXT NOT NULL, "user" TEXT NOT NULL, route TEXT NOT NULL,
    idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 255),
    response_body JSONB NOT NULL, status_code INTEGER NOT NULL, created_at TIMESTAMPTZ NOT NULL, expires_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (tenant, "user", route, idempotency_key));
CREATE INDEX mem_idem_expiry ON mem_idempotency_record (expires_at);
CREATE TABLE mem_grant (id TEXT PRIMARY KEY, tenant TEXT NOT NULL, fact TEXT NOT NULL,
    grantor_team TEXT, grantor_user TEXT NOT NULL, grantee TEXT NOT NULL,
    purpose TEXT NOT NULL CHECK (length(purpose) BETWEEN 1 AND 512), operation TEXT NOT NULL CHECK (operation = 'read'),
    valid_from TIMESTAMPTZ NOT NULL, valid_to TIMESTAMPTZ NOT NULL, revoked_at TIMESTAMPTZ, evidence TEXT NOT NULL, created_at TIMESTAMPTZ NOT NULL);
CREATE INDEX mem_grant_grantee ON mem_grant (tenant, grantee, revoked_at, valid_to);
CREATE INDEX mem_grant_fact    ON mem_grant (fact);
"#;

/// The label of the collections step (ordin change M-8 g), after the SurrealDB migration it translates.
pub(super) const STEP_COLLECTION: &str = "0007_fact_collection";

/// M-8 (g), SQLite: `collection` on the four tables that carry one (SA-COLL-05), defaulting to the
/// default collection `''`, each indexed `(tenant, owner_team, collection)`; and on `mem_quarantine`,
/// as SurrealDB migration `0007` declares it on `quarantine`. Additive only: columns with a constant
/// default and new indexes — no `DROP`, no narrowing, and no backfill (every existing row belongs to
/// the default collection, which is what the default gives it). Forward-only; the pre-migration
/// backup restores.
// The indexes are named after `mem_fact_collection`; `mem_quarantine.collection` is the one-for-one
// translation of SurrealDB 0007's `quarantine.collection`, without which a held write would lose its
// collection.
pub(super) const SQLITE_COLLECTION: &str = r#"
ALTER TABLE mem_fact ADD COLUMN collection TEXT NOT NULL DEFAULT '' CHECK (length(collection) <= 64);
CREATE INDEX mem_fact_collection ON mem_fact (tenant, owner_team, collection);
ALTER TABLE mem_entity ADD COLUMN collection TEXT NOT NULL DEFAULT '' CHECK (length(collection) <= 64);
CREATE INDEX mem_entity_collection ON mem_entity (tenant, owner_team, collection);
ALTER TABLE mem_relationship ADD COLUMN collection TEXT NOT NULL DEFAULT '' CHECK (length(collection) <= 64);
CREATE INDEX mem_relationship_collection ON mem_relationship (tenant, owner_team, collection);
ALTER TABLE mem_source ADD COLUMN collection TEXT NOT NULL DEFAULT '' CHECK (length(collection) <= 64);
CREATE INDEX mem_source_collection ON mem_source (tenant, owner_team, collection);
ALTER TABLE mem_quarantine ADD COLUMN collection TEXT NOT NULL DEFAULT '' CHECK (length(collection) <= 64);
"#;

/// M-8 (g), PostgreSQL — as [`SQLITE_COLLECTION`], with the `VARCHAR(64)` type the contract gives.
pub(super) const POSTGRES_COLLECTION: &str = r#"
ALTER TABLE mem_fact ADD COLUMN collection VARCHAR(64) NOT NULL DEFAULT '';
CREATE INDEX mem_fact_collection ON mem_fact (tenant, owner_team, collection);
ALTER TABLE mem_entity ADD COLUMN collection VARCHAR(64) NOT NULL DEFAULT '';
CREATE INDEX mem_entity_collection ON mem_entity (tenant, owner_team, collection);
ALTER TABLE mem_relationship ADD COLUMN collection VARCHAR(64) NOT NULL DEFAULT '';
CREATE INDEX mem_relationship_collection ON mem_relationship (tenant, owner_team, collection);
ALTER TABLE mem_source ADD COLUMN collection VARCHAR(64) NOT NULL DEFAULT '';
CREATE INDEX mem_source_collection ON mem_source (tenant, owner_team, collection);
ALTER TABLE mem_quarantine ADD COLUMN collection VARCHAR(64) NOT NULL DEFAULT '';
"#;

/// The label of the graph step (ordin change M-9 j), after the SurrealDB migration it translates.
pub(super) const STEP_GRAPH: &str = "0008_graph";

/// M-9 (j), SQLite — the memory component's SQL storage portion of the MemoryService spec's *Data Model*
/// (amendment January 2026): `mem_relationship.replaces`, the live-link and ranked-link indexes, the
/// fact-to-relationship rows, the normalised entity names, the backfill instant, and the held writes'
/// assertions, and what stays with a private fact's owner. Additive only: one
/// nullable column on `mem_relationship` and `mem_maintenance_state`, one constant-default column each
/// on `mem_relationship` and `mem_entity`, three on `mem_quarantine`, two new tables and six new indexes; no `DROP`,
/// no narrowing, no backfill in SQL (the name rows are written in Rust by `backfill_entity_names`,
/// idempotently). Forward-only; the pre-migration backup restores.
// spec: components/memory-store#SA-GRAPH-04
// The first step's *index* `mem_entity_name` (on `mem_entity (tenant, canonical_name)`) and this
// step's *table* of that name would clash: SQLite and PostgreSQL keep indexes and tables in one
// namespace. The table keeps the name (ordin's Memory Service reads it); the older index is dropped and
// recreated, same columns, as `mem_entity_canonical`, at the head of this step. No row changes; the
// index is absent only inside the step.
// `mem_rel_live` and `mem_entity_name_lookup` lead with `owner_team`, which a recall cannot bind (the
// read filter also admits the owner's rows of any team), so both planners would walk the tenant's
// rows. `mem_rel_from_rank` and `mem_rel_to_rank` give one end of a link in the order a hop follows it, the id in byte order as
// `pick_hop` compares it, so a hop stops at its fanout; they hold live links only, the links an as-of
// recall reads being found through `mem_rel_from` and `mem_rel_to`. `mem_entity_name_norm` finds a
// phrase without the team.
pub(super) const SQLITE_GRAPH: &str = r#"
DROP INDEX mem_entity_name;
CREATE INDEX mem_entity_canonical ON mem_entity (tenant, canonical_name);
ALTER TABLE mem_relationship ADD COLUMN replaces TEXT;
CREATE INDEX mem_rel_live ON mem_relationship (tenant, owner_team, from_ent, kind, to_ent) WHERE valid_to IS NULL;
CREATE INDEX mem_rel_from_rank ON mem_relationship (tenant, from_ent, confidence DESC, id) WHERE valid_to IS NULL;
CREATE INDEX mem_rel_to_rank ON mem_relationship (tenant, to_ent, confidence DESC, id) WHERE valid_to IS NULL;
CREATE TABLE mem_relationship_fact (
    relationship_id TEXT NOT NULL, fact_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('asserted','ended')), at TEXT NOT NULL,
    PRIMARY KEY (relationship_id, fact_id, role));
CREATE INDEX mem_rel_fact_fact ON mem_relationship_fact (fact_id);
ALTER TABLE mem_relationship ADD COLUMN owner_only INTEGER NOT NULL DEFAULT 0;
ALTER TABLE mem_entity ADD COLUMN private_aliases TEXT NOT NULL DEFAULT '[]';
CREATE TABLE mem_entity_name (
    entity_id TEXT NOT NULL REFERENCES mem_entity(id) ON DELETE CASCADE,
    tenant TEXT NOT NULL, owner_team TEXT, collection TEXT NOT NULL DEFAULT '',
    name_norm TEXT NOT NULL CHECK (length(name_norm) BETWEEN 1 AND 512),
    owner_only INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (entity_id, name_norm));
CREATE INDEX mem_entity_name_lookup ON mem_entity_name (tenant, owner_team, name_norm);
CREATE INDEX mem_entity_name_norm ON mem_entity_name (tenant, name_norm);
ALTER TABLE mem_maintenance_state ADD COLUMN names_backfilled_at TEXT;
ALTER TABLE mem_quarantine ADD COLUMN entities_asserted TEXT NOT NULL DEFAULT '[]';
ALTER TABLE mem_quarantine ADD COLUMN links_asserted    TEXT NOT NULL DEFAULT '[]';
ALTER TABLE mem_quarantine ADD COLUMN derived_from      TEXT NOT NULL DEFAULT '[]';
"#;

/// M-9 (j), PostgreSQL — as [`SQLITE_GRAPH`].
pub(super) const POSTGRES_GRAPH: &str = r#"
ALTER INDEX mem_entity_name RENAME TO mem_entity_canonical;
ALTER TABLE mem_relationship ADD COLUMN replaces TEXT;
CREATE INDEX mem_rel_live ON mem_relationship (tenant, owner_team, from_ent, kind, to_ent) WHERE valid_to IS NULL;
CREATE INDEX mem_rel_from_rank ON mem_relationship (tenant, from_ent, confidence DESC, id COLLATE "C") WHERE valid_to IS NULL;
CREATE INDEX mem_rel_to_rank ON mem_relationship (tenant, to_ent, confidence DESC, id COLLATE "C") WHERE valid_to IS NULL;
CREATE TABLE mem_relationship_fact (
    relationship_id TEXT NOT NULL, fact_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('asserted','ended')), at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (relationship_id, fact_id, role));
CREATE INDEX mem_rel_fact_fact ON mem_relationship_fact (fact_id);
ALTER TABLE mem_relationship ADD COLUMN owner_only BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE mem_entity ADD COLUMN private_aliases JSONB NOT NULL DEFAULT '[]'::jsonb;
CREATE TABLE mem_entity_name (
    entity_id TEXT NOT NULL REFERENCES mem_entity(id) ON DELETE CASCADE,
    tenant TEXT NOT NULL, owner_team TEXT, collection VARCHAR(64) NOT NULL DEFAULT '',
    name_norm VARCHAR(512) NOT NULL CHECK (length(name_norm) >= 1),
    owner_only BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (entity_id, name_norm));
CREATE INDEX mem_entity_name_lookup ON mem_entity_name (tenant, owner_team, name_norm);
CREATE INDEX mem_entity_name_norm ON mem_entity_name (tenant, name_norm);
ALTER TABLE mem_maintenance_state ADD COLUMN names_backfilled_at TIMESTAMPTZ;
ALTER TABLE mem_quarantine ADD COLUMN entities_asserted JSONB NOT NULL DEFAULT '[]'::jsonb;
ALTER TABLE mem_quarantine ADD COLUMN links_asserted    JSONB NOT NULL DEFAULT '[]'::jsonb;
ALTER TABLE mem_quarantine ADD COLUMN derived_from      JSONB NOT NULL DEFAULT '[]'::jsonb;
"#;

/// The label of the job-link step, after the SurrealDB migration it translates.
pub(super) const STEP_JOB_FACT: &str = "0009_job_fact";

/// SQLite: the link from a fact to the job that wrote it, and the folders behind each alias. Additive
/// only: one new table, two new indexes and one constant-default column; no `DROP`, no narrowing, no
/// backfill. Forward-only; the pre-migration backup restores.
// `mem_job_fact` lets `hard_delete` find a failed job's raw payload, and carries `tenant` and
// `scope_team` so both erasure sets cover it.
pub(super) const SQLITE_JOB_FACT: &str = r#"
CREATE TABLE mem_job_fact (job_id TEXT NOT NULL, fact_id TEXT NOT NULL, tenant TEXT NOT NULL, scope_team TEXT,
    PRIMARY KEY (job_id, fact_id));
CREATE INDEX mem_job_fact_fact ON mem_job_fact (tenant, fact_id);
CREATE INDEX mem_dead_letter_job ON mem_dead_letter (tenant, original_job_id);
ALTER TABLE mem_entity ADD COLUMN alias_folders TEXT NOT NULL DEFAULT '[]';
"#;

/// PostgreSQL — as [`SQLITE_JOB_FACT`].
pub(super) const POSTGRES_JOB_FACT: &str = r#"
CREATE TABLE mem_job_fact (job_id TEXT NOT NULL, fact_id TEXT NOT NULL, tenant TEXT NOT NULL, scope_team TEXT,
    PRIMARY KEY (job_id, fact_id));
CREATE INDEX mem_job_fact_fact ON mem_job_fact (tenant, fact_id);
CREATE INDEX mem_dead_letter_job ON mem_dead_letter (tenant, original_job_id);
ALTER TABLE mem_entity ADD COLUMN alias_folders JSONB NOT NULL DEFAULT '[]'::jsonb;
"#;

/// The label of the step that tells an owner's callers apart and lets a person review a held write.
pub(super) const STEP_LINK_AUTHOR: &str = "0010_link_author";

/// SQLite. `mem_relationship.author` says which of an owner's callers wrote a link; a link written
/// before it stays `NULL`, which no agent's write matches, so an agent ends none of them.
/// `mem_quarantine.fact_id` names the fact a held write would have been, and `mem_quarantine_team`
/// lists a place's held writes. `mem_work_job_depth` counts a team's open jobs. Additive: no backfill,
/// no `DROP`, no narrowing. Forward-only; the pre-migration backup restores.
// spec: phase-1-analysis#SA-GRAPH-14
pub(super) const SQLITE_LINK_AUTHOR: &str = r#"
ALTER TABLE mem_relationship ADD COLUMN author TEXT;
ALTER TABLE mem_quarantine ADD COLUMN fact_id TEXT;
CREATE INDEX mem_quarantine_team ON mem_quarantine (tenant, owner_team, quarantined_at);
CREATE INDEX mem_work_job_depth ON mem_work_job (tenant, scope_team, status);
"#;

/// PostgreSQL — as [`SQLITE_LINK_AUTHOR`].
pub(super) const POSTGRES_LINK_AUTHOR: &str = r#"
ALTER TABLE mem_relationship ADD COLUMN author TEXT;
ALTER TABLE mem_quarantine ADD COLUMN fact_id TEXT;
CREATE INDEX mem_quarantine_team ON mem_quarantine (tenant, owner_team, quarantined_at);
CREATE INDEX mem_work_job_depth ON mem_work_job (tenant, scope_team, status);
"#;

/// The label of the triples step (ordin change M-10 h), after the SurrealDB migration it translates.
pub(super) const STEP_TRIPLES: &str = "0011_triples";

/// SQLite: a fact's triple in columns of its own, the index the write's conflict lookup reads, and
/// the conflicts between facts (memory-store amendment ADR-022; MemoryService M-10 h). Additive: nullable
/// or defaulted columns, two indexes and one table; no row is changed, nothing narrows, no backfill
/// (SA-TRIPLE-12). Removing `mem_fact_conflict` loses the record of disagreeing facts. Forward-only;
/// the pre-migration backup restores.
// spec: components/memory-store#SA-TRIPLE-12
pub(super) const SQLITE_TRIPLES: &str = r#"
ALTER TABLE mem_fact ADD COLUMN subject_entity TEXT;
ALTER TABLE mem_fact ADD COLUMN predicate TEXT CHECK (predicate IS NULL OR length(predicate) BETWEEN 1 AND 128);
ALTER TABLE mem_fact ADD COLUMN object_kind TEXT CHECK (object_kind IS NULL OR object_kind IN ('entity','text','number','money','date','boolean'));
ALTER TABLE mem_fact ADD COLUMN object_value TEXT;
ALTER TABLE mem_fact ADD COLUMN object_unit TEXT;
ALTER TABLE mem_fact ADD COLUMN object_entity TEXT;
ALTER TABLE mem_fact ADD COLUMN single_valued INTEGER NOT NULL DEFAULT 0 CHECK (single_valued IN (0,1));
ALTER TABLE mem_fact ADD COLUMN changed INTEGER NOT NULL DEFAULT 0 CHECK (changed IN (0,1));
ALTER TABLE mem_fact ADD COLUMN author TEXT;
CREATE INDEX mem_fact_triple ON mem_fact (tenant, owner_team, subject_entity, predicate) WHERE valid_to IS NULL AND predicate IS NOT NULL;
CREATE TABLE mem_fact_conflict (
    fact_a TEXT NOT NULL REFERENCES mem_fact(id) ON DELETE CASCADE,
    fact_b TEXT NOT NULL REFERENCES mem_fact(id) ON DELETE CASCADE,
    tenant TEXT NOT NULL, owner_team TEXT, owner_user TEXT NOT NULL,
    recorded_at TEXT NOT NULL,
    PRIMARY KEY (fact_a, fact_b), CHECK (fact_a < fact_b));
CREATE INDEX mem_fact_conflict_b ON mem_fact_conflict (fact_b);
"#;

/// PostgreSQL — as [`SQLITE_TRIPLES`], with `BOOLEAN` flags, `TIMESTAMPTZ` for `recorded_at`, and the
/// pair's order checked in byte order (`COLLATE "C"`), as SQLite compares.
pub(super) const POSTGRES_TRIPLES: &str = r#"
ALTER TABLE mem_fact ADD COLUMN subject_entity TEXT;
ALTER TABLE mem_fact ADD COLUMN predicate TEXT CHECK (predicate IS NULL OR length(predicate) BETWEEN 1 AND 128);
ALTER TABLE mem_fact ADD COLUMN object_kind TEXT CHECK (object_kind IS NULL OR object_kind IN ('entity','text','number','money','date','boolean'));
ALTER TABLE mem_fact ADD COLUMN object_value TEXT;
ALTER TABLE mem_fact ADD COLUMN object_unit TEXT;
ALTER TABLE mem_fact ADD COLUMN object_entity TEXT;
ALTER TABLE mem_fact ADD COLUMN single_valued BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE mem_fact ADD COLUMN changed BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE mem_fact ADD COLUMN author TEXT;
CREATE INDEX mem_fact_triple ON mem_fact (tenant, owner_team, subject_entity, predicate) WHERE valid_to IS NULL AND predicate IS NOT NULL;
CREATE TABLE mem_fact_conflict (
    fact_a TEXT NOT NULL REFERENCES mem_fact(id) ON DELETE CASCADE,
    fact_b TEXT NOT NULL REFERENCES mem_fact(id) ON DELETE CASCADE,
    tenant TEXT NOT NULL, owner_team TEXT, owner_user TEXT NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (fact_a, fact_b), CHECK (fact_a COLLATE "C" < fact_b COLLATE "C"));
CREATE INDEX mem_fact_conflict_b ON mem_fact_conflict (fact_b);
"#;

/// The SQLite steps, in order.
pub(super) const SQLITE_STEPS: &[(&str, &str)] = &[
    (STEP_INIT, SQLITE_INIT),
    (STEP_COLLECTION, SQLITE_COLLECTION),
    (STEP_GRAPH, SQLITE_GRAPH),
    (STEP_JOB_FACT, SQLITE_JOB_FACT),
    (STEP_LINK_AUTHOR, SQLITE_LINK_AUTHOR),
    (STEP_TRIPLES, SQLITE_TRIPLES),
];

/// The PostgreSQL steps, in order.
pub(super) const POSTGRES_STEPS: &[(&str, &str)] = &[
    (STEP_INIT, POSTGRES_INIT),
    (STEP_COLLECTION, POSTGRES_COLLECTION),
    (STEP_GRAPH, POSTGRES_GRAPH),
    (STEP_JOB_FACT, POSTGRES_JOB_FACT),
    (STEP_LINK_AUTHOR, POSTGRES_LINK_AUTHOR),
    (STEP_TRIPLES, POSTGRES_TRIPLES),
];
