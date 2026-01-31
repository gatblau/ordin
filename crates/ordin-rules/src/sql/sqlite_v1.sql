-- rules v1, SQLite (RulesEngine.md, Data Model). Released text: never edit (DatabaseMigrations.md rule 10).
CREATE TABLE rules_control_starts (
    kind TEXT NOT NULL CHECK (kind IN ('command', 'scope', 'asker')),
    key TEXT NOT NULL CHECK (length(key) BETWEEN 1 AND 512),
    period_seconds INTEGER NOT NULL CHECK (period_seconds BETWEEN 1 AND 2592000),
    window_start INTEGER NOT NULL,
    starts INTEGER NOT NULL CHECK (starts >= 1),
    PRIMARY KEY (kind, key, period_seconds, window_start)
);
CREATE TABLE rules_control_messages (
    scope TEXT NOT NULL CHECK (length(scope) BETWEEN 1 AND 512),
    asker TEXT NOT NULL CHECK (length(asker) BETWEEN 1 AND 512),
    digest TEXT NOT NULL CHECK (length(digest) = 64),
    seen_at INTEGER NOT NULL,
    repeated INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (scope, asker, digest)
);
CREATE INDEX rules_control_messages_seen ON rules_control_messages (scope, asker, seen_at);
CREATE TABLE rules_control_locks (
    key TEXT PRIMARY KEY CHECK (length(key) BETWEEN 1 AND 512),
    holder TEXT NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE TABLE rules_intake_places (
    intake TEXT NOT NULL, slot INTEGER NOT NULL CHECK (slot >= 1),
    run_id TEXT, taken_at INTEGER,
    PRIMARY KEY (intake, slot)
);
CREATE TABLE rules_run_facts (
    run_id TEXT PRIMARY KEY CHECK (length(run_id) = 36),
    agent_instance_id TEXT NOT NULL,
    facts TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE rules_approved_calls (
    run_id TEXT NOT NULL, tool TEXT NOT NULL, arguments_digest TEXT NOT NULL CHECK (length(arguments_digest) = 64),
    approval_id TEXT NOT NULL, used INTEGER NOT NULL DEFAULT 0 CHECK (used IN (0, 1)),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (run_id, tool, arguments_digest, approval_id)
);
CREATE INDEX rules_approved_calls_open ON rules_approved_calls (run_id, tool, arguments_digest) WHERE used = 0;
