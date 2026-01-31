-- team_knowledge step 4, SQLite (TeamKnowledgeService.md amendment ADR-121, Data Model): one constant-default column and five new tables.
ALTER TABLE team_knowledge_shares ADD COLUMN scope TEXT NOT NULL DEFAULT 'all'
    CHECK (scope IN ('all','results','workspace'));

CREATE TABLE team_workspaces (
    workspace_id        TEXT PRIMARY KEY,
    state               TEXT NOT NULL CHECK (state IN ('proposed','team')),
    workspace_key       TEXT NOT NULL CHECK (length(workspace_key) = 36),
    turned_on_here      INTEGER NOT NULL CHECK (turned_on_here IN (0,1)),
    synced_description  TEXT,                                  -- JSON TeamWorkspaceFile; NULL until the first share or open
    results_kind        TEXT NOT NULL CHECK (results_kind IN ('team_folder','sharepoint','confluence')),
    results_place_key   TEXT,
    created_at          TEXT NOT NULL,
    CHECK ((results_kind = 'team_folder') = (results_place_key IS NULL))
);
CREATE TABLE team_workspace_places (
    workspace_id    TEXT NOT NULL REFERENCES team_workspaces(workspace_id) ON DELETE CASCADE,
    key             TEXT NOT NULL CHECK (length(key) = 36),
    source_link_id  TEXT,
    state           TEXT NOT NULL CHECK (state IN ('linked','no_access','waiting','removal_waits')),
    PRIMARY KEY (workspace_id, key)
);
CREATE UNIQUE INDEX team_workspace_places_link ON team_workspace_places (source_link_id) WHERE source_link_id IS NOT NULL;
CREATE TABLE team_workspace_agents (
    workspace_id         TEXT NOT NULL REFERENCES team_workspaces(workspace_id) ON DELETE CASCADE,
    key                  TEXT NOT NULL CHECK (length(key) = 36),
    agent_instance_id    TEXT,
    state                TEXT NOT NULL CHECK (state IN ('present','waiting','declined','not_found')),
    not_found_code       TEXT,
    declined_changed_at  TEXT,
    PRIMARY KEY (workspace_id, key)
);
CREATE TABLE team_activity (
    id           TEXT PRIMARY KEY,                             -- TeamActivityId
    workspace_id TEXT NOT NULL REFERENCES team_workspaces(workspace_id) ON DELETE CASCADE,
    mine         INTEGER NOT NULL CHECK (mine IN (0,1)),
    kind         TEXT NOT NULL CHECK (kind IN ('run','replaced')),
    by_login     TEXT CHECK (by_login IS NULL OR length(by_login) BETWEEN 1 AND 100),
    by_confirmed INTEGER NOT NULL CHECK (by_confirmed IN (0,1)),               -- TW-J
    at           TEXT NOT NULL,
    task         TEXT CHECK (task IS NULL OR length(task) BETWEEN 1 AND 200),
    agent        TEXT CHECK (agent IS NULL OR length(agent) BETWEEN 1 AND 120),
    outcome      TEXT CHECK (outcome IS NULL OR outcome IN ('completed','failed','stopped')),
    results      TEXT NOT NULL,                                -- JSON array of paths, 0–50
    change       TEXT,
    subject      TEXT CHECK (subject IS NULL OR length(subject) BETWEEN 1 AND 120),
    replaced_by  TEXT,
    replaced_by_confirmed INTEGER NOT NULL DEFAULT 0 CHECK (replaced_by_confirmed IN (0,1)),   -- TW-J
    run_id       TEXT,
    blob         TEXT CHECK (blob IS NULL OR length(blob) = 40),
    shared_at    TEXT
);
CREATE INDEX team_activity_newest ON team_activity (workspace_id, at DESC, id DESC);
CREATE UNIQUE INDEX team_activity_run ON team_activity (run_id) WHERE run_id IS NOT NULL;
CREATE TABLE team_workspace_files (
    workspace_id   TEXT NOT NULL REFERENCES team_workspaces(workspace_id) ON DELETE CASCADE,
    path           TEXT NOT NULL CHECK (length(CAST(path AS BLOB)) BETWEEN 1 AND 1100),
    kind           TEXT NOT NULL CHECK (kind IN ('description','activity','result')),
    blob           TEXT NOT NULL CHECK (length(blob) = 40),
    local_sha256   TEXT CHECK (local_sha256 IS NULL OR length(local_sha256) = 64),
    synced_at      TEXT NOT NULL,
    PRIMARY KEY (workspace_id, path)
);
