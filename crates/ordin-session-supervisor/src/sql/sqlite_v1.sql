CREATE TABLE tasks (
    id TEXT PRIMARY KEY CHECK (length(id) = 36),
    workspace_id TEXT NOT NULL,
    target TEXT NOT NULL,
    command TEXT CHECK (command IS NULL OR length(command) BETWEEN 1 AND 100),
    free_text TEXT CHECK (free_text IS NULL OR length(free_text) BETWEEN 1 AND 8000),
    arguments TEXT NOT NULL,
    attached_artefacts TEXT NOT NULL,
    run_again_of TEXT,
    requested_at TEXT NOT NULL,
    CHECK ((command IS NULL) <> (free_text IS NULL))
);
CREATE TABLE runs (
    id TEXT PRIMARY KEY CHECK (length(id) = 36),
    origin TEXT NOT NULL,
    task_id TEXT, crew_run_id TEXT, round_number INTEGER,
    workspace_id TEXT NOT NULL,
    agent TEXT NOT NULL,
    adapter TEXT NOT NULL CHECK (adapter IN ('claude_local', 'copilot_local', 'hosted_agent', 'slack_thread')),
    runtime_session_id TEXT, remote_run_id TEXT, remote_thread_id TEXT,
    state TEXT NOT NULL CHECK (state IN ('queued','running','paused','completed','failed','stopped_at_limit',
                                         'cancelled','interrupted','abandoned','no_reply')),
    pause_reason TEXT CHECK (pause_reason IN ('approval', 'sign_in', 'person')),
    pause_approval_id TEXT,
    wall_clock_secs INTEGER NOT NULL, request_budget INTEGER NOT NULL DEFAULT 0, idle_timeout_secs INTEGER NOT NULL,
    model_turns INTEGER NOT NULL DEFAULT 0, tool_calls INTEGER NOT NULL DEFAULT 0, model_level TEXT,
    tokens INTEGER, tokens_from_cache INTEGER,
    work_key TEXT NOT NULL,
    command TEXT CHECK (command IS NULL OR length(command) BETWEEN 1 AND 64),
    started_at TEXT, ended_at TEXT,
    end_reason TEXT CHECK (end_reason IS NULL OR length(end_reason) <= 500),
    inputs_used TEXT NOT NULL,
    queued_at TEXT NOT NULL,
    CHECK ((state = 'paused') = (pause_reason IS NOT NULL)),
    CHECK ((state IN ('completed','failed','stopped_at_limit','cancelled','interrupted','abandoned','no_reply')) = (ended_at IS NOT NULL)),
    CHECK (state <> 'no_reply' OR adapter = 'slack_thread'),
    CHECK (state <> 'abandoned' OR adapter IN ('hosted_agent', 'slack_thread')),
    CHECK (tokens_from_cache IS NULL OR tokens IS NULL OR tokens_from_cache <= tokens)
);
CREATE INDEX runs_in_progress ON runs (state) WHERE state IN ('queued', 'running', 'paused');
CREATE INDEX runs_workspace ON runs (workspace_id, queued_at);
CREATE INDEX runs_crew ON runs (crew_run_id) WHERE crew_run_id IS NOT NULL;
CREATE INDEX runs_command ON runs (command, state) WHERE command IS NOT NULL;
