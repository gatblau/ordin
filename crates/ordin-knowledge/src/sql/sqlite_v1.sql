-- spec: ordin-desktop/components/KnowledgeService#SA-RECON-07
-- 0001_pages (desktop and personal server only; created in every program, empty in the agent profile)
CREATE TABLE knowledge_pages (
    id                      TEXT PRIMARY KEY,
    workspace_id            TEXT NOT NULL,
    title                   TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
    body_markdown           TEXT NOT NULL CHECK (length(CAST(body_markdown AS BLOB)) <= 1048576),
    written_by              TEXT NOT NULL CHECK (written_by IN ('ordin','person')),
    edited_by_person        INTEGER NOT NULL DEFAULT 0 CHECK (edited_by_person IN (0,1)),
    sources                 TEXT NOT NULL DEFAULT '[]',
    state                   TEXT NOT NULL CHECK (state IN ('current','may_be_out_of_date','removed')),
    last_changed_at         TEXT NOT NULL,
    place_id                TEXT NOT NULL,
    relative_path           TEXT NOT NULL,
    file_version            INTEGER NOT NULL CHECK (file_version >= 1),
    state_before_removal    TEXT CHECK (state_before_removal IN ('current','may_be_out_of_date')),
    bin_ticket              TEXT,
    memory_indexed_version  INTEGER,
    memory_index_attempts   INTEGER NOT NULL DEFAULT 0,
    memory_next_attempt_at  TEXT,
    memory_removed          INTEGER NOT NULL DEFAULT 0 CHECK (memory_removed IN (0,1)),
    created_at              TEXT NOT NULL,
    CHECK ((state = 'removed') = (state_before_removal IS NOT NULL)),
    CHECK ((state = 'removed') = (bin_ticket IS NOT NULL))
);
CREATE UNIQUE INDEX knowledge_pages_title ON knowledge_pages (workspace_id, lower(title)) WHERE state <> 'removed';
CREATE UNIQUE INDEX knowledge_pages_file  ON knowledge_pages (place_id, relative_path);
CREATE INDEX knowledge_pages_state        ON knowledge_pages (workspace_id, state);
CREATE INDEX knowledge_pages_memory       ON knowledge_pages (memory_next_attempt_at) WHERE memory_indexed_version IS NULL OR memory_indexed_version < file_version;

CREATE VIRTUAL TABLE knowledge_pages_fts USING fts5(
    title, body, content='', tokenize = 'unicode61 remove_diacritics 2');

CREATE TABLE knowledge_source_marks (
    page_id        TEXT NOT NULL REFERENCES knowledge_pages(id) ON DELETE CASCADE,
    origin         TEXT NOT NULL,
    location       TEXT NOT NULL,
    newer_version  TEXT NOT NULL,
    changed_by     TEXT,
    changed_at     TEXT,
    marked_at      TEXT NOT NULL,
    left_at        TEXT,
    PRIMARY KEY (page_id, origin, location)
);
CREATE INDEX knowledge_source_marks_location ON knowledge_source_marks (origin, location);

CREATE TABLE knowledge_page_updates (
    page_id          TEXT PRIMARY KEY REFERENCES knowledge_pages(id) ON DELETE CASCADE,
    body_markdown    TEXT NOT NULL CHECK (length(CAST(body_markdown AS BLOB)) <= 1048576),
    sources          TEXT NOT NULL,
    run_id           TEXT NOT NULL,
    proposed_at      TEXT NOT NULL
);
