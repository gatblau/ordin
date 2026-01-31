CREATE TABLE slack_grants (
    id                 TEXT PRIMARY KEY,
    workspace_id       TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    agent_instance_id  TEXT NOT NULL,
    access             TEXT NOT NULL CHECK (access IN ('read_slack','read_and_draft_slack')),
    granted_at         TEXT NOT NULL,
    UNIQUE (workspace_id, agent_instance_id)
);
