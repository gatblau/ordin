-- team_knowledge step 1, SQLite (TeamKnowledgeService.md, Data Model, ADR-103): new tables only.
CREATE TABLE team_knowledge_bindings (
    workspace_id         TEXT PRIMARY KEY,
    source_link_id       TEXT NOT NULL,
    host                 TEXT NOT NULL CHECK (length(host) BETWEEN 1 AND 253),
    repository           TEXT NOT NULL CHECK (length(repository) BETWEEN 3 AND 200),
    branch               TEXT NOT NULL CHECK (length(CAST(branch AS BLOB)) BETWEEN 1 AND 255),
    folder               TEXT NOT NULL CHECK (length(CAST(folder AS BLOB)) <= 1024),
    access               TEXT NOT NULL CHECK (access IN ('read_write','read_only')),
    enabled_at           TEXT NOT NULL,
    last_checked_at      TEXT,
    last_checked_commit  TEXT,
    last_got_commit      TEXT,
    last_shared_at       TEXT,
    access_checked_at    TEXT,
    available            TEXT,                -- JSON TeamAvailable, NULL when nothing is new
    to_share             TEXT,                -- JSON TeamCounts
    held_for_check       INTEGER NOT NULL DEFAULT 0 CHECK (held_for_check >= 0),
    folder_large         INTEGER NOT NULL DEFAULT 0 CHECK (folder_large IN (0,1)),
    last_error_code      TEXT,
    last_get             TEXT,                -- JSON GetReport
    last_share           TEXT                 -- JSON ShareProposed
);
CREATE UNIQUE INDEX team_knowledge_bindings_link ON team_knowledge_bindings (source_link_id);

CREATE TABLE team_knowledge_items (
    workspace_id   TEXT NOT NULL REFERENCES team_knowledge_bindings(workspace_id) ON DELETE CASCADE,
    path           TEXT NOT NULL,             -- "facts/<id>.json" relative to the folder
    kind           TEXT NOT NULL CHECK (kind IN ('fact','entity','source','relationship','page')),
    item_id        TEXT NOT NULL,
    blob           TEXT NOT NULL CHECK (length(blob) = 40),   -- git blob id on the branch
    local_sha256   TEXT NOT NULL CHECK (length(local_sha256) = 64),
    synced_at      TEXT NOT NULL,
    PRIMARY KEY (workspace_id, path)
);
CREATE INDEX team_knowledge_items_item ON team_knowledge_items (workspace_id, kind, item_id);

CREATE TABLE team_knowledge_problems (
    workspace_id   TEXT NOT NULL REFERENCES team_knowledge_bindings(workspace_id) ON DELETE CASCADE,
    path           TEXT NOT NULL,
    code           TEXT NOT NULL,
    blob           TEXT NOT NULL,             -- a new blob at the same path clears the problem
    first_seen_at  TEXT NOT NULL,
    PRIMARY KEY (workspace_id, path)
);

CREATE TABLE team_knowledge_shares (
    id             TEXT PRIMARY KEY,          -- TeamShareId
    workspace_id   TEXT NOT NULL,
    state          TEXT NOT NULL CHECK (state IN ('proposed','approved','sending','waiting_review','done',
                                                  'rejected','failed','abandoned','superseded')),
    approval_id    TEXT,
    base_commit    TEXT NOT NULL,
    commit_id      TEXT NOT NULL,
    review_branch  TEXT,
    pull_request   TEXT,                      -- JSON 2C PullRequestRef
    counts         TEXT NOT NULL,             -- JSON TeamCounts
    planned        TEXT NOT NULL,             -- JSON [{path, blob | null}] (null = delete)
    moves          INTEGER NOT NULL DEFAULT 0 CHECK (moves BETWEEN 0 AND 3),
    result_code    TEXT,
    created_at     TEXT NOT NULL,
    finished_at    TEXT
);
CREATE INDEX team_knowledge_shares_open ON team_knowledge_shares (workspace_id, state)
    WHERE state IN ('proposed','approved','sending','waiting_review');
