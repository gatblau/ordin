-- team_knowledge step 2, SQLite (TeamKnowledgeService.md amendment ADR-110, Data Model): applied as written; its tables stay empty in the first build (ADR-121 item 9).
ALTER TABLE team_knowledge_bindings ADD COLUMN descriptions TEXT;          -- JSON resolved map (§M step 3)
ALTER TABLE team_knowledge_bindings ADD COLUMN descriptions_synced TEXT;   -- JSON last synced own-folder map (§M step 4)
ALTER TABLE team_knowledge_bindings ADD COLUMN checks TEXT;                -- JSON TeamChecks
ALTER TABLE team_knowledge_bindings ADD COLUMN folder_id TEXT;             -- NULL means the workspace_id value (SA-TEAMR-01)
ALTER TABLE team_knowledge_bindings ADD COLUMN last_extra_sync_at TEXT;

CREATE TABLE team_knowledge_read_folders (
    id                   TEXT PRIMARY KEY,               -- TeamFolderId
    workspace_id         TEXT NOT NULL,
    source_link_id       TEXT,
    host                 TEXT NOT NULL CHECK (length(host) BETWEEN 1 AND 253),
    repository           TEXT NOT NULL CHECK (length(repository) BETWEEN 3 AND 200),
    branch               TEXT NOT NULL CHECK (length(CAST(branch AS BLOB)) BETWEEN 1 AND 255),
    folder               TEXT NOT NULL CHECK (length(CAST(folder AS BLOB)) <= 1024),
    access               TEXT NOT NULL CHECK (access IN ('read_write','read_only')),
    added_by_person      INTEGER NOT NULL CHECK (added_by_person IN (0,1)),
    company_key          TEXT CHECK (company_key IS NULL OR length(company_key) BETWEEN 1 AND 64),
    display_name         TEXT CHECK (display_name IS NULL OR length(display_name) BETWEEN 1 AND 120),
    review_required      INTEGER NOT NULL DEFAULT 0 CHECK (review_required IN (0,1)),
    person_may_stop      INTEGER NOT NULL DEFAULT 1 CHECK (person_may_stop IN (0,1)),
    enabled_at           TEXT NOT NULL,
    last_checked_at      TEXT, last_checked_commit TEXT, last_got_commit TEXT, access_checked_at TEXT,
    available            TEXT,                            -- JSON TeamAvailable
    folder_large         INTEGER NOT NULL DEFAULT 0 CHECK (folder_large IN (0,1)),
    review_state         TEXT CHECK (review_state IS NULL OR review_state IN ('required','not_required','cannot_tell')),
    review_checked_at    TEXT,
    blocked_code         TEXT,
    last_error_code      TEXT,
    last_get             TEXT,                            -- JSON GetReport
    descriptions         TEXT,                            -- JSON resolved map
    checks               TEXT,                            -- JSON TeamChecks
    CHECK (added_by_person = 1 OR company_key IS NOT NULL)
);
CREATE UNIQUE INDEX team_knowledge_read_folders_place
    ON team_knowledge_read_folders (workspace_id, lower(host), lower(repository), branch, folder);
CREATE INDEX team_knowledge_read_folders_company ON team_knowledge_read_folders (company_key) WHERE company_key IS NOT NULL;

CREATE TABLE team_knowledge_read_items (
    folder_id      TEXT NOT NULL REFERENCES team_knowledge_read_folders(id) ON DELETE CASCADE,
    path           TEXT NOT NULL,
    kind           TEXT NOT NULL CHECK (kind IN ('fact','entity','source','relationship','page')),
    item_id        TEXT NOT NULL,
    blob           TEXT NOT NULL CHECK (length(blob) = 40),
    local_sha256   TEXT NOT NULL CHECK (length(local_sha256) = 64),
    synced_at      TEXT NOT NULL,
    PRIMARY KEY (folder_id, path)
);
CREATE INDEX team_knowledge_read_items_item ON team_knowledge_read_items (kind, item_id);

CREATE TABLE team_knowledge_read_problems (
    folder_id      TEXT NOT NULL REFERENCES team_knowledge_read_folders(id) ON DELETE CASCADE,
    path           TEXT NOT NULL,
    code           TEXT NOT NULL,
    blob           TEXT NOT NULL,
    first_seen_at  TEXT NOT NULL,
    PRIMARY KEY (folder_id, path)
);

CREATE TABLE team_knowledge_extra_files (           -- adoption, descriptions and checks files of any folder
    folder_id      TEXT NOT NULL,                     -- own (workspace id value) or read folder id
    path           TEXT NOT NULL,
    kind           TEXT NOT NULL CHECK (kind IN ('adoption','descriptions','checks')),
    blob           TEXT NOT NULL CHECK (length(blob) = 40),
    local_sha256   TEXT CHECK (local_sha256 IS NULL OR length(local_sha256) = 64),
    synced_at      TEXT NOT NULL,
    PRIMARY KEY (folder_id, path)
);

CREATE TABLE team_knowledge_adoptions (
    workspace_id    TEXT NOT NULL,
    item_kind       TEXT NOT NULL CHECK (item_kind IN ('fact','page')),
    item_id         TEXT NOT NULL,
    origin          TEXT NOT NULL CHECK (origin IN ('here','received')),
    from_folder     TEXT NOT NULL CHECK (length(from_folder) BETWEEN 1 AND 1500),
    from_host       TEXT NOT NULL, from_repository TEXT NOT NULL, from_branch TEXT NOT NULL, from_path TEXT NOT NULL,
    source_id       TEXT NOT NULL,
    source_blob     TEXT NOT NULL CHECK (length(source_blob) = 40),
    adopted_by      TEXT NOT NULL CHECK (length(adopted_by) BETWEEN 1 AND 200),
    adopted_at      TEXT NOT NULL,
    edited          INTEGER NOT NULL CHECK (edited IN (0,1)),
    version         INTEGER NOT NULL CHECK (version >= 1),
    mark            TEXT CHECK (mark IS NULL OR mark IN ('changed_at_source','removed_at_source')),
    mark_blob       TEXT,
    kept_blob       TEXT,
    PRIMARY KEY (workspace_id, item_kind, item_id)
);
CREATE INDEX team_knowledge_adoptions_source ON team_knowledge_adoptions (from_host, from_repository, from_branch);

CREATE TABLE team_knowledge_descriptions (
    workspace_id   TEXT NOT NULL,
    path           TEXT NOT NULL CHECK (length(CAST(path AS BLOB)) <= 1024),
    text           TEXT NOT NULL CHECK (length(text) BETWEEN 1 AND 500),
    changed_at     TEXT NOT NULL,
    PRIMARY KEY (workspace_id, path)
);

CREATE TABLE team_knowledge_stopped (
    workspace_id   TEXT NOT NULL,
    company_key    TEXT NOT NULL,
    stopped_at     TEXT NOT NULL,
    PRIMARY KEY (workspace_id, company_key)
);

CREATE TABLE team_knowledge_workspaces (
    workspace_id       TEXT PRIMARY KEY,
    workspace_facts    INTEGER NOT NULL DEFAULT 0 CHECK (workspace_facts >= 0),
    workspace_large    INTEGER NOT NULL DEFAULT 0 CHECK (workspace_large IN (0,1)),
    last_recall_check  TEXT                            -- JSON RecallCheckReport
);
