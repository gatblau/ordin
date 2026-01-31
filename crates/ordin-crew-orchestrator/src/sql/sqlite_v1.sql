-- crew orchestrator v1, SQLite (CrewOrchestrator.md, Data Model). Released text: never edit (DatabaseMigrations.md rule 10).
CREATE TABLE crew_runs (
    id TEXT PRIMARY KEY CHECK (length(id) = 36),
    workspace_id TEXT NOT NULL,
    crew_charter_id TEXT NOT NULL,
    charter_version INTEGER NOT NULL,
    charter TEXT NOT NULL,
    mode TEXT NOT NULL CHECK (mode IN ('coordinated', 'saved_steps')),
    workflow_id TEXT, workflow_version INTEGER,
    taken_over_from_step TEXT,
    inputs TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('starting','running','paused','finished','escalated','stopped_at_limit','killed','failed')),
    pause_reason TEXT CHECK (pause_reason IN ('approval', 'restart', 'escalation')),
    round_counter INTEGER NOT NULL DEFAULT 0,
    requests_used INTEGER NOT NULL DEFAULT 0,
    wall_clock_used_secs INTEGER NOT NULL DEFAULT 0,
    repeated_failures INTEGER NOT NULL DEFAULT 0,
    end_reason TEXT CHECK (end_reason IS NULL OR length(end_reason) <= 500),
    started_at TEXT NOT NULL, ended_at TEXT,
    CHECK ((workflow_id IS NULL) = (workflow_version IS NULL)),
    CHECK ((state = 'paused') = (pause_reason IS NOT NULL))
);
CREATE INDEX crew_runs_in_progress ON crew_runs (state) WHERE state IN ('starting', 'running', 'paused');
CREATE INDEX crew_runs_workspace ON crew_runs (workspace_id, state);
CREATE TABLE rounds (
    crew_run_id TEXT NOT NULL REFERENCES crew_runs (id),
    number INTEGER NOT NULL CHECK (number >= 1),
    coordinator_decision TEXT NOT NULL CHECK (length(coordinator_decision) BETWEEN 1 AND 4000),
    guardrail_decisions TEXT NOT NULL,
    member_runs TEXT NOT NULL,
    opened_at TEXT NOT NULL, closed_at TEXT,
    PRIMARY KEY (crew_run_id, number)
);
CREATE TABLE crew_step_states (
    crew_run_id TEXT NOT NULL REFERENCES crew_runs (id),
    position TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('waiting','running','done','failed','skipped','changed_by_ordin')),
    attempts INTEGER NOT NULL DEFAULT 0,
    run_id TEXT,
    outcome TEXT,
    decided_path TEXT,
    passes INTEGER,
    PRIMARY KEY (crew_run_id, position)
);
