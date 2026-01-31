-- checker v1, SQLite (Checker.md, Data Model). Released text: never edit (DatabaseMigrations.md rule 10).
CREATE TABLE check_runs (
    id               TEXT PRIMARY KEY CHECK (length(id) = 36),
    subject_kind     TEXT NOT NULL CHECK (subject_kind IN ('repository_read','agent_draft','part_draft',
                         'withdrawal','vocabulary_change','installed_agent','library_item','rehearsal',
                         'install_candidate')),
    subject          TEXT NOT NULL,             -- CheckSubject as JSON
    version_checked  TEXT NOT NULL CHECK (length(version_checked) BETWEEN 1 AND 512),
    passed           INTEGER NOT NULL CHECK (passed IN (0, 1)),
    failures         INTEGER NOT NULL CHECK (failures >= 0),
    warnings         INTEGER NOT NULL CHECK (warnings >= 0),
    tidied           TEXT NOT NULL,             -- Vec<WordingChange> as JSON
    agents_that_stop TEXT NOT NULL,             -- Vec<String> as JSON
    started_at       TEXT NOT NULL,
    finished_at      TEXT NOT NULL
) STRICT;
CREATE INDEX check_runs_subject ON check_runs (subject_kind, finished_at);

CREATE TABLE check_findings (
    check_run_id      TEXT NOT NULL REFERENCES check_runs(id) ON DELETE CASCADE,
    ordinal           INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 500),
    code              TEXT NOT NULL CHECK (length(code) BETWEEN 1 AND 64),
    severity          TEXT NOT NULL CHECK (severity IN ('failure', 'warning')),
    part_identifier   TEXT CHECK (part_identifier IS NULL OR length(part_identifier) <= 512),
    message_id        TEXT NOT NULL,
    args              TEXT NOT NULL,            -- JSON object of strings
    fixes             TEXT NOT NULL,            -- Vec<Fix> as JSON
    detail            TEXT NOT NULL CHECK (length(detail) <= 2000),
    PRIMARY KEY (check_run_id, ordinal)
) STRICT;

CREATE TABLE executable_part_decisions (
    id                TEXT PRIMARY KEY CHECK (length(id) = 36),
    agent_instance_id TEXT NOT NULL CHECK (length(agent_instance_id) = 36),
    part_name         TEXT NOT NULL CHECK (length(part_name) BETWEEN 1 AND 200),
    kind              TEXT NOT NULL CHECK (kind IN ('hook', 'mcp_server', 'bundled_program')),
    decision          TEXT NOT NULL CHECK (decision IN ('trusted', 'needs_it_approval')),
    source            TEXT NOT NULL CHECK (source = 'organisation_policy'),
    decided_at        TEXT NOT NULL,
    UNIQUE (agent_instance_id, kind, part_name)
) STRICT;

-- What each decision was made from, so it can be re-decided when the profile changes.
CREATE TABLE executable_part_subjects (
    decision_id  TEXT PRIMARY KEY REFERENCES executable_part_decisions(id) ON DELETE CASCADE,
    declared_by  TEXT NOT NULL CHECK (length(declared_by) BETWEEN 1 AND 512)
) STRICT;

-- SA-CLOSE-03: the complete executable-part list of an instance's latest install check.
CREATE TABLE install_candidate_parts (
    agent_instance_id TEXT PRIMARY KEY CHECK (length(agent_instance_id) = 36),
    plan_hash         TEXT NOT NULL CHECK (length(plan_hash) BETWEEN 1 AND 80),
    parts             TEXT NOT NULL,            -- Vec<ExecutablePart> as JSON
    check_run_id      TEXT NOT NULL CHECK (length(check_run_id) = 36),
    recorded_at       TEXT NOT NULL
) STRICT;
