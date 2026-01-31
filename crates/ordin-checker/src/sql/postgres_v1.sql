-- checker v1, PostgreSQL (Checker.md, Data Model). Released text: never edit (DatabaseMigrations.md rule 10).
CREATE TABLE check_runs (
    id UUID PRIMARY KEY,
    subject_kind TEXT NOT NULL CHECK (subject_kind IN ('repository_read','agent_draft','part_draft',
        'withdrawal','vocabulary_change','installed_agent','library_item','rehearsal',
        'install_candidate')),
    subject JSONB NOT NULL,
    version_checked TEXT NOT NULL CHECK (char_length(version_checked) BETWEEN 1 AND 512),
    passed BOOLEAN NOT NULL,
    failures INTEGER NOT NULL CHECK (failures >= 0),
    warnings INTEGER NOT NULL CHECK (warnings >= 0),
    tidied JSONB NOT NULL,
    agents_that_stop JSONB NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    finished_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX check_runs_subject ON check_runs (subject_kind, finished_at);
CREATE TABLE check_findings (
    check_run_id UUID NOT NULL REFERENCES check_runs(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 500),
    code TEXT NOT NULL CHECK (char_length(code) BETWEEN 1 AND 64),
    severity TEXT NOT NULL CHECK (severity IN ('failure', 'warning')),
    part_identifier TEXT CHECK (part_identifier IS NULL OR char_length(part_identifier) <= 512),
    message_id TEXT NOT NULL,
    args JSONB NOT NULL,
    fixes JSONB NOT NULL,
    detail TEXT NOT NULL CHECK (char_length(detail) <= 2000),
    PRIMARY KEY (check_run_id, ordinal)
);
CREATE TABLE executable_part_decisions (
    id UUID PRIMARY KEY,
    agent_instance_id UUID NOT NULL,
    part_name TEXT NOT NULL CHECK (char_length(part_name) BETWEEN 1 AND 200),
    kind TEXT NOT NULL CHECK (kind IN ('hook', 'mcp_server', 'bundled_program')),
    decision TEXT NOT NULL CHECK (decision IN ('trusted', 'needs_it_approval')),
    source TEXT NOT NULL CHECK (source = 'organisation_policy'),
    decided_at TIMESTAMPTZ NOT NULL,
    UNIQUE (agent_instance_id, kind, part_name)
);
CREATE TABLE executable_part_subjects (
    decision_id UUID PRIMARY KEY REFERENCES executable_part_decisions(id) ON DELETE CASCADE,
    declared_by TEXT NOT NULL CHECK (char_length(declared_by) BETWEEN 1 AND 512)
);
CREATE TABLE install_candidate_parts (
    agent_instance_id UUID PRIMARY KEY,
    plan_hash TEXT NOT NULL CHECK (char_length(plan_hash) BETWEEN 1 AND 80),
    parts JSONB NOT NULL,
    check_run_id UUID NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL
);
