-- approval v1, PostgreSQL (ApprovalService.md, Data Model). Released text: never edit (DatabaseMigrations.md rule 10).
CREATE TABLE approvals (
    id uuid PRIMARY KEY, workspace_id uuid NOT NULL,
    asked_by_kind text NOT NULL CHECK (asked_by_kind IN ('run', 'crew_round', 'person')),
    asked_by_run_id uuid, asked_by_crew_run_id uuid, asked_by_round integer CHECK (asked_by_round >= 1),
    action text NOT NULL CHECK (char_length(action) BETWEEN 1 AND 500),
    destination text NOT NULL CHECK (char_length(destination) BETWEEN 1 AND 2048),
    affected_records jsonb NOT NULL, consequence text NOT NULL CHECK (char_length(consequence) BETWEEN 1 AND 2000),
    card jsonb NOT NULL, asker text, leaves_company boolean NOT NULL DEFAULT false,
    state text NOT NULL CHECK (state IN ('pending', 'approved', 'rejected', 'expired')),
    decided_by text, decided_on text CHECK (char_length(decided_on) BETWEEN 1 AND 80),
    decided_at timestamptz, created_at timestamptz NOT NULL
);
CREATE INDEX approvals_pending ON approvals (workspace_id, created_at) WHERE state = 'pending';
CREATE INDEX approvals_run ON approvals (asked_by_run_id) WHERE asked_by_run_id IS NOT NULL;
CREATE TABLE approval_buttons (
    card_ref text PRIMARY KEY CHECK (char_length(card_ref) = 22),
    approval_id uuid NOT NULL REFERENCES approvals (id) ON DELETE CASCADE,
    button text NOT NULL CHECK (button IN ('approve', 'reject', 'show_details')),
    state text NOT NULL CHECK (state IN ('open', 'used', 'closed')),
    used_at timestamptz, UNIQUE (approval_id, button)
);
