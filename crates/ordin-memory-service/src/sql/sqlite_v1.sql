-- spec: ordin-desktop/components/MemoryService#memory_fact_refs
CREATE TABLE memory_fact_refs (
    id                 TEXT PRIMARY KEY,
    memory_fact_id     TEXT NOT NULL UNIQUE,
    scope_key          TEXT NOT NULL,
    workspace_id       TEXT,
    scope_id           TEXT,
    everywhere         INTEGER NOT NULL DEFAULT 0 CHECK (everywhere IN (0,1)),
    origin_kind        TEXT NOT NULL CHECK (origin_kind IN ('conversation','agent','document','knowledge_page','person')),
    agent_instance_id  TEXT,
    run_id             TEXT,
    origin_ref         TEXT CHECK (origin_ref IS NULL OR length(origin_ref) BETWEEN 1 AND 2048),
    origin_version     TEXT CHECK (origin_version IS NULL OR length(origin_version) BETWEEN 1 AND 256),
    fact_class         TEXT CHECK (fact_class IS NULL OR length(fact_class) BETWEEN 1 AND 128),
    job_id             TEXT NOT NULL,
    state              TEXT NOT NULL CHECK (state IN ('pending','kept','not_kept','retired','forgotten','replaced')),
    may_be_out_of_date INTEGER NOT NULL DEFAULT 0 CHECK (may_be_out_of_date IN (0,1)),
    replaces_id        TEXT REFERENCES memory_fact_refs(id),
    retain_until       TEXT,
    written_at         TEXT NOT NULL,
    settled_at         TEXT,
    CHECK ((workspace_id IS NULL) <> (scope_id IS NULL))
);
CREATE INDEX memory_fact_refs_scope   ON memory_fact_refs (scope_key, state, written_at);
CREATE INDEX memory_fact_refs_origin  ON memory_fact_refs (origin_ref) WHERE origin_ref IS NOT NULL;
CREATE INDEX memory_fact_refs_pending ON memory_fact_refs (state, written_at) WHERE state = 'pending';
CREATE INDEX memory_fact_refs_retain  ON memory_fact_refs (retain_until) WHERE retain_until IS NOT NULL;
