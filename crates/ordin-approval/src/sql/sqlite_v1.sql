-- approval v1, SQLite (ApprovalService.md, Data Model). Released text: never edit (DatabaseMigrations.md rule 10).
CREATE TABLE approvals (
    id                    TEXT PRIMARY KEY CHECK (length(id) = 36),
    workspace_id          TEXT NOT NULL CHECK (length(workspace_id) = 36),
    asked_by_kind         TEXT NOT NULL CHECK (asked_by_kind IN ('run', 'crew_round', 'person')),
    asked_by_run_id       TEXT CHECK (asked_by_run_id IS NULL OR length(asked_by_run_id) = 36),
    asked_by_crew_run_id  TEXT CHECK (asked_by_crew_run_id IS NULL OR length(asked_by_crew_run_id) = 36),
    asked_by_round        INTEGER CHECK (asked_by_round IS NULL OR asked_by_round >= 1),
    action                TEXT NOT NULL CHECK (length(action) BETWEEN 1 AND 500),
    destination           TEXT NOT NULL CHECK (length(destination) BETWEEN 1 AND 2048),
    affected_records      TEXT NOT NULL,           -- JSON array of strings, at most 1000
    consequence           TEXT NOT NULL CHECK (length(consequence) BETWEEN 1 AND 2000),
    card                  TEXT NOT NULL,           -- JSON ApprovalCard
    asker                 TEXT,                    -- agent profile only
    leaves_company        INTEGER NOT NULL DEFAULT 0 CHECK (leaves_company IN (0, 1)),
    state                 TEXT NOT NULL CHECK (state IN ('pending', 'approved', 'rejected', 'expired')),
    decided_by            TEXT,
    decided_on            TEXT CHECK (decided_on IS NULL OR length(decided_on) BETWEEN 1 AND 80),
    decided_at            TEXT,
    created_at            TEXT NOT NULL,
    CHECK ((state IN ('approved', 'rejected')) = (decided_by IS NOT NULL AND decided_at IS NOT NULL)),
    CHECK ((asked_by_kind = 'run') = (asked_by_run_id IS NOT NULL)),
    CHECK ((asked_by_kind = 'crew_round') = (asked_by_crew_run_id IS NOT NULL AND asked_by_round IS NOT NULL))
);
CREATE INDEX approvals_pending ON approvals (state, workspace_id, created_at) WHERE state = 'pending';
CREATE INDEX approvals_run ON approvals (asked_by_run_id) WHERE asked_by_run_id IS NOT NULL;
CREATE INDEX approvals_crew ON approvals (asked_by_crew_run_id) WHERE asked_by_crew_run_id IS NOT NULL;

CREATE TABLE approval_buttons (
    card_ref     TEXT PRIMARY KEY CHECK (length(card_ref) = 22),
    approval_id  TEXT NOT NULL REFERENCES approvals (id) ON DELETE CASCADE,
    button       TEXT NOT NULL CHECK (button IN ('approve', 'reject', 'show_details')),
    state        TEXT NOT NULL CHECK (state IN ('open', 'used', 'closed')),
    used_at      TEXT,
    UNIQUE (approval_id, button)
);
