CREATE TABLE agent_folders (
    agent_instance_id TEXT PRIMARY KEY CHECK (length(agent_instance_id) = 36),
    folder_path       TEXT NOT NULL UNIQUE CHECK (length(folder_path) BETWEEN 1 AND 4096),
    meta_path         TEXT NOT NULL UNIQUE CHECK (length(meta_path) BETWEEN 1 AND 4096),
    state             TEXT NOT NULL CHECK (state IN ('created', 'removing', 'removed')),
    created_at        TEXT NOT NULL,
    removed_at        TEXT
) STRICT;

CREATE TABLE session_folder_access (
    run_id            TEXT NOT NULL CHECK (length(run_id) = 36),
    agent_instance_id TEXT NOT NULL REFERENCES agent_folders(agent_instance_id),
    workspace_id      TEXT NOT NULL CHECK (length(workspace_id) = 36),
    folder_grant_id   TEXT NOT NULL CHECK (length(folder_grant_id) = 36),
    place_id          TEXT NOT NULL CHECK (length(place_id) = 36),
    path              TEXT NOT NULL CHECK (length(path) BETWEEN 1 AND 4096),
    access            TEXT NOT NULL CHECK (access IN ('read', 'read_and_change')),
    applied_at        TEXT NOT NULL,
    PRIMARY KEY (run_id, place_id)
) STRICT;
CREATE INDEX session_folder_access_instance ON session_folder_access (agent_instance_id);
CREATE INDEX session_folder_access_workspace_place ON session_folder_access (workspace_id, place_id);

CREATE TABLE workspace_folder_grants (
    folder_grant_id   TEXT PRIMARY KEY CHECK (length(folder_grant_id) = 36),
    workspace_id      TEXT NOT NULL CHECK (length(workspace_id) = 36),
    grantee_kind      TEXT NOT NULL CHECK (grantee_kind IN ('agent_instance', 'crew')),
    grantee_id        TEXT NOT NULL CHECK (length(grantee_id) = 36),
    place_id          TEXT NOT NULL CHECK (length(place_id) = 36),
    place_kind        TEXT NOT NULL CHECK (place_kind IN ('persons_folder', 'workspace_folder', 'working_copy', 'coding_task_worktree')),
    path              TEXT NOT NULL CHECK (length(path) BETWEEN 1 AND 4096),
    access            TEXT NOT NULL CHECK (access IN ('read', 'read_and_change')),
    granted_at        TEXT NOT NULL
) STRICT;
CREATE INDEX workspace_folder_grants_workspace ON workspace_folder_grants (workspace_id, grantee_kind, grantee_id);
