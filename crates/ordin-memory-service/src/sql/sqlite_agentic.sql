ALTER TABLE memory_fact_refs ADD COLUMN caller_kind TEXT
    CHECK (caller_kind IS NULL OR caller_kind IN ('ordin','crew_member','agent','hosted_agent','knowledge_service','person'));
CREATE TABLE memory_recalls (
    id                TEXT PRIMARY KEY,
    run_id            TEXT,
    workspace_id      TEXT,
    scope_id          TEXT,
    caller_kind       TEXT NOT NULL CHECK (caller_kind IN ('ordin','crew_member','agent','hosted_agent','knowledge_service')),
    agent_instance_id TEXT,
    recalled_at       TEXT NOT NULL,
    CHECK ((workspace_id IS NULL) <> (scope_id IS NULL))
);
CREATE INDEX memory_recalls_age ON memory_recalls (recalled_at);
CREATE INDEX memory_recalls_scope ON memory_recalls (scope_id) WHERE scope_id IS NOT NULL;
CREATE TABLE memory_recall_items (
    recall_id TEXT NOT NULL REFERENCES memory_recalls(id) ON DELETE CASCADE,
    position  INTEGER NOT NULL CHECK (position BETWEEN 0 AND 39),
    item_kind TEXT NOT NULL CHECK (item_kind IN ('fact','link')),
    fact_ref  TEXT,
    link_id   TEXT,
    PRIMARY KEY (recall_id, position),
    CHECK (fact_ref IS NOT NULL AND (item_kind = 'link') = (link_id IS NOT NULL))
);
CREATE INDEX memory_recall_items_fact ON memory_recall_items (fact_ref) WHERE fact_ref IS NOT NULL;
CREATE INDEX memory_recall_items_link ON memory_recall_items (link_id) WHERE link_id IS NOT NULL;
CREATE TABLE memory_made_from (
    fact_ref   TEXT NOT NULL REFERENCES memory_fact_refs(id),
    origin_ref TEXT NOT NULL REFERENCES memory_fact_refs(id),
    PRIMARY KEY (fact_ref, origin_ref)
);
CREATE INDEX memory_made_from_origin ON memory_made_from (origin_ref);
CREATE TABLE memory_consolidations (
    workspace_id TEXT NOT NULL,
    thing_id     TEXT NOT NULL,
    fact_ref     TEXT NOT NULL REFERENCES memory_fact_refs(id),
    written_at   TEXT NOT NULL,
    PRIMARY KEY (workspace_id, thing_id, written_at)
);
