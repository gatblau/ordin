-- Home and Search, migration step 1 (HomeAndSearch.md *Data Model*). SQLite only (desktop and
-- personal server). Marks are ordin's own notes; deleting them changes no document.
-- spec: ordin-desktop/components/HomeAndSearch#SA-CLOSE-04
CREATE TABLE home_marks (
    id                   INTEGER PRIMARY KEY CHECK (id = 1),
    home_last_opened_at  TEXT
);
CREATE TABLE changed_marks (
    id            TEXT PRIMARY KEY,
    workspace_id  TEXT NOT NULL,
    place_id      TEXT NOT NULL,
    path          TEXT NOT NULL CHECK (length(path) BETWEEN 1 AND 4096),
    changed_at    TEXT NOT NULL,
    UNIQUE (workspace_id, place_id, path)
);
CREATE INDEX changed_marks_ws ON changed_marks (workspace_id, changed_at);
CREATE TABLE workspace_last_seen (
    workspace_id  TEXT PRIMARY KEY,
    last_seen_at  TEXT NOT NULL
);
CREATE TABLE remote_mark_poll (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    last_poll_at  TEXT NOT NULL
);
