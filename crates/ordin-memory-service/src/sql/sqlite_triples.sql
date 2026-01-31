-- spec: ordin-desktop/components/MemoryService#MEM-TR-3
-- SPEC-GAP-3: the Data Model writes the agent CHECK before the agent_instance_id column, which
-- SQLite refuses (a table constraint must follow the columns); it is written last here.
ALTER TABLE memory_fact_refs ADD COLUMN subject_norm TEXT CHECK (subject_norm IS NULL OR length(subject_norm) BETWEEN 1 AND 512);
ALTER TABLE memory_fact_refs ADD COLUMN term_key TEXT CHECK (term_key IS NULL OR length(term_key) BETWEEN 1 AND 64);
CREATE INDEX memory_fact_refs_triple ON memory_fact_refs (scope_key, subject_norm, term_key, origin_ref) WHERE term_key IS NOT NULL;
CREATE TABLE memory_terms (
    place_key         TEXT NOT NULL,
    term_key          TEXT NOT NULL CHECK (length(term_key) BETWEEN 1 AND 64),
    label             TEXT NOT NULL CHECK (length(label) BETWEEN 1 AND 64),
    label_norm        TEXT NOT NULL,
    meaning           TEXT NOT NULL CHECK (length(meaning) BETWEEN 1 AND 300),
    object_kind       TEXT NOT NULL CHECK (object_kind IN ('thing','text','number','money','date','yes_no')),
    single_valued     INTEGER NOT NULL CHECK (single_valued IN (0,1)),
    origin            TEXT NOT NULL CHECK (origin IN ('seed','ordin','agent','hosted_agent','team','person')),
    agent_instance_id TEXT,
    merged_into       TEXT,
    created_at        TEXT NOT NULL,
    renamed_at        TEXT,
    PRIMARY KEY (place_key, term_key),
    CHECK ((origin = 'agent') = (agent_instance_id IS NOT NULL))
);
CREATE UNIQUE INDEX memory_terms_label ON memory_terms (place_key, label_norm) WHERE merged_into IS NULL;
