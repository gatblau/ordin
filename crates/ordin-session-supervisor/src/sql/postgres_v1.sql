CREATE TABLE runs (
    id uuid PRIMARY KEY,
    origin jsonb NOT NULL,
    task_id uuid, crew_run_id uuid, round_number integer,
    workspace_id uuid NOT NULL,
    agent jsonb NOT NULL,
    adapter text NOT NULL CHECK (adapter IN ('claude_local', 'copilot_local', 'hosted_agent', 'slack_thread')),
    runtime_session_id text, remote_run_id text, remote_thread_id text,
    state text NOT NULL CHECK (state IN ('queued','running','paused','completed','failed','stopped_at_limit',
                                         'cancelled','interrupted','abandoned','no_reply')),
    pause_reason text CHECK (pause_reason IN ('approval', 'sign_in', 'person')),
    pause_approval_id uuid,
    wall_clock_secs integer NOT NULL, request_budget integer NOT NULL DEFAULT 0, idle_timeout_secs integer NOT NULL,
    model_turns integer NOT NULL DEFAULT 0, tool_calls integer NOT NULL DEFAULT 0, model_level text,
    tokens bigint, tokens_from_cache bigint,
    work_key text NOT NULL,
    command text CHECK (command IS NULL OR char_length(command) BETWEEN 1 AND 64),
    started_at timestamptz, ended_at timestamptz,
    end_reason text CHECK (end_reason IS NULL OR char_length(end_reason) <= 500),
    inputs_used jsonb NOT NULL,
    queued_at timestamptz NOT NULL,
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
