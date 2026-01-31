-- spec: ordin-desktop/components/KnowledgeService#SA-KREAD-02
-- 5 read pages from other teams' folders (amendment January 2026, ADR-110)
CREATE TABLE knowledge_read_pages (
    workspace_id            TEXT NOT NULL,
    page_id                 TEXT NOT NULL,
    folder_id               TEXT NOT NULL,
    title                   TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
    body_markdown           TEXT NOT NULL CHECK (length(CAST(body_markdown AS BLOB)) <= 1048576),
    written_by              TEXT NOT NULL CHECK (written_by IN ('ordin','person')),
    sources                 TEXT NOT NULL DEFAULT '[]',
    tags                    TEXT NOT NULL DEFAULT '[]',
    aliases                 TEXT NOT NULL DEFAULT '[]',
    extra_header            TEXT NOT NULL DEFAULT '' CHECK (length(CAST(extra_header AS BLOB)) <= 65536),
    shared_by               TEXT NOT NULL CHECK (length(shared_by) BETWEEN 1 AND 200),
    shared_at               TEXT NOT NULL,
    last_changed_at         TEXT NOT NULL,
    bytes_sha256            TEXT NOT NULL CHECK (length(bytes_sha256) = 64),
    version                 INTEGER NOT NULL CHECK (version >= 1),
    memory_indexed_version  INTEGER,
    memory_index_attempts   INTEGER NOT NULL DEFAULT 0,
    memory_next_attempt_at  TEXT,
    got_at                  TEXT NOT NULL,
    PRIMARY KEY (workspace_id, page_id)
);
CREATE INDEX knowledge_read_pages_folder ON knowledge_read_pages (workspace_id, folder_id);
CREATE INDEX knowledge_read_pages_index  ON knowledge_read_pages (memory_indexed_version, memory_next_attempt_at);
CREATE VIRTUAL TABLE knowledge_read_pages_fts USING fts5(title, body, content='',
    tokenize='unicode61 remove_diacritics 2');
CREATE TABLE knowledge_read_page_names (
    workspace_id  TEXT NOT NULL,
    folder_id     TEXT NOT NULL,
    name_key      TEXT NOT NULL,
    page_id       TEXT NOT NULL,
    kind          TEXT NOT NULL CHECK (kind IN ('title','alias')),
    PRIMARY KEY (workspace_id, folder_id, name_key)
);
CREATE TABLE knowledge_read_links (
    workspace_id    TEXT NOT NULL,
    page_id         TEXT NOT NULL,
    ordinal         INTEGER NOT NULL CHECK (ordinal BETWEEN 1 AND 1000),
    target_key      TEXT NOT NULL,
    target_page_id  TEXT,
    PRIMARY KEY (workspace_id, page_id, ordinal)
);
CREATE INDEX knowledge_read_links_target ON knowledge_read_links (workspace_id, target_key);
CREATE TABLE knowledge_read_pages_removed (
    page_id         TEXT PRIMARY KEY,
    workspace_id    TEXT NOT NULL,
    folder_id       TEXT NOT NULL,
    memory_removed  INTEGER NOT NULL DEFAULT 0 CHECK (memory_removed IN (0,1)),
    removed_at      TEXT NOT NULL
);
