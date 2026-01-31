-- Setup Assistant, migration step 1 (SetupAssistant.md *Data Model*). SQLite only (desktop and
-- personal server), in the journal database.
-- spec: ordin-desktop/components/SetupAssistant#SA-RECON-02
CREATE TABLE harness_attachments (
    id               TEXT PRIMARY KEY CHECK (length(id) = 36),
    harness          TEXT NOT NULL UNIQUE CHECK (harness IN ('claude_code','copilot_cli')),
    location         TEXT NOT NULL CHECK (length(CAST(location AS BLOB)) BETWEEN 1 AND 4096),
    version          TEXT CHECK (version IS NULL OR length(version) <= 64),
    tested           INTEGER NOT NULL CHECK (tested IN (0,1)),
    signed_in        INTEGER NOT NULL CHECK (signed_in IN (0,1)),
    sign_in_method   TEXT CHECK (sign_in_method IS NULL OR length(sign_in_method) <= 120),
    state            TEXT NOT NULL CHECK (state IN ('found','checked','ready','needs_sign_in','needs_update')),
    last_checked_at  TEXT NOT NULL,
    CHECK (state <> 'ready' OR (tested = 1 AND signed_in = 1))
);

-- chosen here: one row
CREATE TABLE setup_state (
    singleton                 INTEGER PRIMARY KEY CHECK (singleton = 1),
    last_guided_completed_at  TEXT,
    requirements_digest       TEXT CHECK (requirements_digest IS NULL OR length(requirements_digest) = 64)
);

-- chosen here: latest result per check
CREATE TABLE setup_checks (
    check_kind   TEXT PRIMARY KEY CHECK (check_kind IN ('company_settings','harness','harness_sign_in','tokens',
                   'agent_repositories','agent_folders','programs','craft_apps')),
    outcome      TEXT NOT NULL CHECK (outcome IN ('passed','repaired','failed','skipped')),
    code         TEXT,
    result_json  TEXT NOT NULL,          -- the CheckResult as JSON (no secrets: codes, names, hashes only)
    checked_at   TEXT NOT NULL
);
