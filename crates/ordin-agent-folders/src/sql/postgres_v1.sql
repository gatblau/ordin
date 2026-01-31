CREATE TABLE agent_folders (
    agent_instance_id UUID PRIMARY KEY,
    folder_path       TEXT NOT NULL UNIQUE CHECK (char_length(folder_path) BETWEEN 1 AND 4096),
    meta_path         TEXT NOT NULL UNIQUE CHECK (char_length(meta_path) BETWEEN 1 AND 4096),
    state             TEXT NOT NULL CHECK (state IN ('created', 'removing', 'removed')),
    created_at        TIMESTAMPTZ NOT NULL,
    removed_at        TIMESTAMPTZ
);
CREATE TABLE session_folder_access (
    run_id            UUID NOT NULL,
    agent_instance_id UUID NOT NULL REFERENCES agent_folders(agent_instance_id),
    workspace_id      UUID NOT NULL,
    folder_grant_id   UUID NOT NULL,
    place_id          UUID NOT NULL,
    path              TEXT NOT NULL CHECK (char_length(path) BETWEEN 1 AND 4096),
    access            TEXT NOT NULL CHECK (access IN ('read', 'read_and_change')),
    applied_at        TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (run_id, place_id)
);
CREATE INDEX session_folder_access_instance ON session_folder_access (agent_instance_id);
CREATE INDEX session_folder_access_workspace_place ON session_folder_access (workspace_id, place_id);
CREATE TABLE workspace_folder_grants (
    folder_grant_id   UUID PRIMARY KEY,
    workspace_id      UUID NOT NULL,
    grantee_kind      TEXT NOT NULL CHECK (grantee_kind IN ('agent_instance', 'crew')),
    grantee_id        UUID NOT NULL,
    place_id          UUID NOT NULL,
    place_kind        TEXT NOT NULL CHECK (place_kind IN ('persons_folder', 'workspace_folder', 'working_copy', 'coding_task_worktree')),
    path              TEXT NOT NULL CHECK (char_length(path) BETWEEN 1 AND 4096),
    access            TEXT NOT NULL CHECK (access IN ('read', 'read_and_change')),
    granted_at        TIMESTAMPTZ NOT NULL
);
CREATE INDEX workspace_folder_grants_workspace ON workspace_folder_grants (workspace_id, grantee_kind, grantee_id);
