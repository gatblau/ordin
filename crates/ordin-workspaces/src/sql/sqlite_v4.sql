-- step (4, "team workspaces", …)
CREATE TABLE team_results_folders (
    workspace_id  TEXT PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
    place_id      TEXT NOT NULL,
    made_at       TEXT NOT NULL,
    ended_at      TEXT
);
CREATE TABLE team_result_files (
    workspace_id   TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    path           TEXT NOT NULL CHECK (length(CAST(path AS BLOB)) BETWEEN 1 AND 1024),
    run_id         TEXT,
    version        INTEGER NOT NULL CHECK (version >= 1),
    sha256         TEXT NOT NULL CHECK (length(sha256) = 64),
    size_bytes     INTEGER NOT NULL CHECK (size_bytes >= 0),
    sent_sha256    TEXT CHECK (sent_sha256 IS NULL OR length(sent_sha256) = 64),
    synced_sha256  TEXT CHECK (synced_sha256 IS NULL OR length(synced_sha256) = 64),
    changed_at     TEXT NOT NULL,
    PRIMARY KEY (workspace_id, path)
);
CREATE INDEX team_result_files_run ON team_result_files (run_id) WHERE run_id IS NOT NULL;
CREATE TABLE team_pending_grants (
    id                 TEXT PRIMARY KEY,                       -- TeamGrantId
    workspace_id       TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    agent_key          TEXT NOT NULL CHECK (length(agent_key) = 36),
    agent_name         TEXT NOT NULL CHECK (length(agent_name) BETWEEN 1 AND 120),
    repository         TEXT NOT NULL,                          -- JSON AgentRepositoryRef
    agent_id           TEXT NOT NULL CHECK (length(agent_id) BETWEEN 1 AND 300),
    agent_version      TEXT NOT NULL CHECK (length(agent_version) BETWEEN 1 AND 64),
    agent_instance_id  TEXT,
    access             TEXT NOT NULL,                          -- JSON Vec<PendingAccess>, 1–102
    added_by           TEXT NOT NULL CHECK (length(added_by) BETWEEN 1 AND 100),
    added_by_confirmed INTEGER NOT NULL CHECK (added_by_confirmed IN (0,1)),
    added_at           TEXT NOT NULL,
    state              TEXT NOT NULL CHECK (state IN ('waiting','allowed','declined','replaced')),
    decided_at         TEXT
);
CREATE UNIQUE INDEX team_pending_grants_waiting ON team_pending_grants (workspace_id, agent_key) WHERE state = 'waiting';
