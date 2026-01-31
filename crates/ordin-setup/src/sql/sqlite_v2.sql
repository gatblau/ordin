-- Setup Assistant, migration step 2: releases and updates (SetupAssistant.md amendment January
-- 2026, ADR-114, *Data Model (added)*).
-- spec: ordin-desktop/components/SetupAssistant#SET-UP-8
-- chosen here: release notes kept when a desktop download is verified; newest 5 kept
CREATE TABLE setup_whats_new (
    version   TEXT PRIMARY KEY CHECK (length(version) BETWEEN 1 AND 64),
    notes     TEXT NOT NULL CHECK (length(notes) <= 4000),
    kept_at   TEXT NOT NULL,
    seen_at   TEXT
);

-- spec: ordin-desktop/components/SetupAssistant#SET-UP-15
-- chosen here: the personal server's own update, one row
CREATE TABLE setup_server_update (
    singleton   INTEGER PRIMARY KEY CHECK (singleton = 1),
    version     TEXT NOT NULL CHECK (length(version) BETWEEN 1 AND 64),
    state       TEXT NOT NULL CHECK (state IN ('running','restarting','done','failed')),
    code        TEXT CHECK (code IS NULL OR length(code) <= 64),
    started_at  TEXT NOT NULL,
    ended_at    TEXT,
    CHECK (state IN ('running','restarting') OR ended_at IS NOT NULL)
);
