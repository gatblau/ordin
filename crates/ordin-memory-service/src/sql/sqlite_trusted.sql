ALTER TABLE memory_fact_refs ADD COLUMN page_status TEXT
    CHECK (page_status IS NULL OR page_status IN ('stable','draft','deprecated'));
ALTER TABLE memory_fact_refs ADD COLUMN page_tier TEXT
    CHECK (page_tier IS NULL OR page_tier IN ('human_reviewed','machine_confirmed','unverified'));
ALTER TABLE memory_recalls ADD COLUMN wider TEXT;
CREATE TABLE memory_recall_wider (
    recall_id   TEXT NOT NULL REFERENCES memory_recalls(id) ON DELETE CASCADE,
    position    INTEGER NOT NULL CHECK (position BETWEEN 0 AND 49),
    agent       TEXT NOT NULL CHECK (length(agent) BETWEEN 1 AND 200),
    repository  TEXT NOT NULL CHECK (length(repository) BETWEEN 1 AND 2048),
    path        TEXT NOT NULL CHECK (length(CAST(path AS BLOB)) BETWEEN 1 AND 1024),
    commit_id   TEXT NOT NULL CHECK (length(commit_id) BETWEEN 7 AND 64),
    section     TEXT CHECK (section IS NULL OR length(section) <= 200),
    page_title  TEXT CHECK (page_title IS NULL OR length(page_title) <= 200),
    as_of       TEXT NOT NULL,
    answer_id   TEXT,
    more_found  INTEGER NOT NULL DEFAULT 0 CHECK (more_found IN (0,1)),
    PRIMARY KEY (recall_id, position)
);
CREATE INDEX memory_fact_refs_source_prefix ON memory_fact_refs (scope_key, origin_ref) WHERE origin_ref IS NOT NULL;
