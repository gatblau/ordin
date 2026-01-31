-- spec: ordin-desktop/components/MemoryService#memory_fact_refs
CREATE TABLE memory_fact_refs (
    id                 UUID PRIMARY KEY,
    memory_fact_id     TEXT NOT NULL UNIQUE,
    scope_key          TEXT NOT NULL,
    workspace_id       UUID,
    scope_id           UUID,
    everywhere         BOOLEAN NOT NULL DEFAULT FALSE,
    origin_kind        TEXT NOT NULL CHECK (origin_kind IN ('conversation','agent','document','knowledge_page','person')),
    agent_instance_id  UUID,
    run_id             UUID,
    origin_ref         VARCHAR(2048),
    origin_version     VARCHAR(256),
    fact_class         VARCHAR(128),
    job_id             TEXT NOT NULL,
    state              TEXT NOT NULL CHECK (state IN ('pending','kept','not_kept','retired','forgotten','replaced')),
    may_be_out_of_date BOOLEAN NOT NULL DEFAULT FALSE,
    replaces_id        UUID REFERENCES memory_fact_refs(id),
    retain_until       TIMESTAMPTZ,
    written_at         TIMESTAMPTZ NOT NULL,
    settled_at         TIMESTAMPTZ,
    CHECK ((workspace_id IS NULL) <> (scope_id IS NULL))
);
CREATE INDEX memory_fact_refs_scope   ON memory_fact_refs (scope_key, state, written_at);
CREATE INDEX memory_fact_refs_origin  ON memory_fact_refs (origin_ref) WHERE origin_ref IS NOT NULL;
CREATE INDEX memory_fact_refs_pending ON memory_fact_refs (state, written_at) WHERE state = 'pending';
CREATE INDEX memory_fact_refs_retain  ON memory_fact_refs (retain_until) WHERE retain_until IS NOT NULL;
