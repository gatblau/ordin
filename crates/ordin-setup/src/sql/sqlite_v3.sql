-- Setup Assistant, migration step 3: the highest application version started (SetupAssistant.md
-- amendment January 2026, SET-REC-2, *Data Model (added)*).
-- spec: ordin-desktop/components/SetupAssistant#SET-REC-2
-- chosen here: the highest application version started on this data root, one row
CREATE TABLE setup_started (
    singleton        INTEGER PRIMARY KEY CHECK (singleton = 1),
    highest_version  TEXT NOT NULL CHECK (length(highest_version) BETWEEN 1 AND 64),
    recorded_at      TEXT NOT NULL
);
