-- spec: ordin-desktop/components/JournalAndArtefactStore#JA-MIN-1
-- Minutes kept by Ordin. Only adds a table and an index; the table stays empty in a build without
-- the minutes switch.
CREATE TABLE minutes (
    id            uuid        PRIMARY KEY,
    workspace_id  uuid        NOT NULL,
    title         text        NOT NULL CHECK (char_length(title) BETWEEN 1 AND 120),
    source        text        NOT NULL CHECK (source IN ('transcript_file','audio_file','recording')),
    source_name   text        NOT NULL CHECK (char_length(source_name) BETWEEN 1 AND 255),
    body          text        NOT NULL CHECK (octet_length(body) BETWEEN 1 AND 1048576),
    recording_id  uuid        NULL,
    run_id        uuid        NULL,
    revision      integer     NOT NULL CHECK (revision >= 1),
    kept_at       timestamptz NOT NULL,
    edited_at     timestamptz NULL
);
CREATE INDEX minutes_workspace ON minutes (workspace_id, kept_at DESC, id DESC);
