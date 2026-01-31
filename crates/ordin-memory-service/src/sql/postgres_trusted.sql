ALTER TABLE memory_fact_refs ADD COLUMN page_status TEXT
    CHECK (page_status IS NULL OR page_status IN ('stable','draft','deprecated'));
ALTER TABLE memory_fact_refs ADD COLUMN page_tier TEXT
    CHECK (page_tier IS NULL OR page_tier IN ('human_reviewed','machine_confirmed','unverified'));
ALTER TABLE memory_recalls ADD COLUMN wider JSONB;
CREATE TABLE memory_recall_wider (
    recall_id UUID NOT NULL REFERENCES memory_recalls(id) ON DELETE CASCADE,
    position SMALLINT NOT NULL CHECK (position BETWEEN 0 AND 49),
    agent VARCHAR(200) NOT NULL CHECK (length(agent) >= 1), repository VARCHAR(2048) NOT NULL CHECK (length(repository) >= 1),
    path VARCHAR(1024) NOT NULL CHECK (length(path) >= 1), commit_id VARCHAR(64) NOT NULL CHECK (length(commit_id) >= 7),
    section VARCHAR(200), page_title VARCHAR(200), as_of TIMESTAMPTZ NOT NULL, answer_id UUID,
    more_found BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (recall_id, position));
CREATE INDEX memory_fact_refs_source_prefix ON memory_fact_refs (scope_key, origin_ref text_pattern_ops) WHERE origin_ref IS NOT NULL;
