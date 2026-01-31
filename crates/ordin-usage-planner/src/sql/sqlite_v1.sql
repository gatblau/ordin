-- usage_planner v1, SQLite (UsagePlanner.md, Data Model). Released text: never edit (DatabaseMigrations.md rule 10).
CREATE TABLE usage_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    settings TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE reused_answers (
    id TEXT PRIMARY KEY CHECK (length(id) = 36),
    request TEXT NOT NULL CHECK (length(request) BETWEEN 1 AND 8000),
    request_digest TEXT NOT NULL CHECK (length(request_digest) = 64),
    agent TEXT NOT NULL,
    agent_key TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    input_versions TEXT NOT NULL,
    source_run_id TEXT NOT NULL,
    text TEXT NOT NULL,
    answered_at TEXT NOT NULL,
    shown_again_at TEXT NOT NULL,
    expires_at TEXT NOT NULL
);
CREATE INDEX reused_answers_lookup ON reused_answers (request_digest, agent_key, workspace_id, expires_at);
CREATE TABLE usage_daily (
    day TEXT PRIMARY KEY,
    model_turns INTEGER NOT NULL DEFAULT 0,
    tool_calls INTEGER NOT NULL DEFAULT 0,
    turns_on_smaller INTEGER NOT NULL DEFAULT 0,
    answered_without_model INTEGER NOT NULL DEFAULT 0,
    tokens INTEGER,
    tokens_from_cache INTEGER
);
CREATE TABLE usage_work_history (
    work_key TEXT NOT NULL CHECK (length(work_key) BETWEEN 1 AND 200),
    run_id TEXT NOT NULL,
    model_turns INTEGER NOT NULL CHECK (model_turns >= 0),
    finished_at TEXT NOT NULL,
    PRIMARY KEY (work_key, run_id)
);
