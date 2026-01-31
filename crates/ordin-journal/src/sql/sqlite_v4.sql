-- spec: ordin-desktop/components/JournalAndArtefactStore#JA-MIN-1
-- Minutes kept by Ordin. Only adds a table and an index; the table stays empty in a build without
-- the minutes switch.
CREATE TABLE minutes (
    id            TEXT    PRIMARY KEY CHECK (length(id) = 36),
    workspace_id  TEXT    NOT NULL CHECK (length(workspace_id) = 36),
    title         TEXT    NOT NULL CHECK (length(title) BETWEEN 1 AND 120),
    source        TEXT    NOT NULL CHECK (source IN ('transcript_file','audio_file','recording')),
    source_name   TEXT    NOT NULL CHECK (length(source_name) BETWEEN 1 AND 255),
    body          TEXT    NOT NULL CHECK (length(CAST(body AS BLOB)) BETWEEN 1 AND 1048576),
    recording_id  TEXT    NULL CHECK (recording_id IS NULL OR length(recording_id) = 36),
    run_id        TEXT    NULL,
    revision      INTEGER NOT NULL CHECK (revision >= 1),
    kept_at       TEXT    NOT NULL,
    edited_at     TEXT    NULL
);
CREATE INDEX minutes_workspace ON minutes (workspace_id, kept_at DESC, id DESC);
