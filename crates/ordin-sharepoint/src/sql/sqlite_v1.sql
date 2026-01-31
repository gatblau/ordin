-- sharepoint v1, SQLite (SharePointAdapter.md, Data Model). Released text: never edit (DatabaseMigrations.md rule 10).
CREATE TABLE sharepoint_link_state (
    source_link_id   TEXT PRIMARY KEY,
    drive_id         TEXT NOT NULL,
    root_item_id     TEXT NOT NULL,
    path_prefix      TEXT NOT NULL,                  -- folder or file path from the library root
    delta_link       TEXT NOT NULL CHECK (length(delta_link) <= 4096),
    tracked_at       TEXT NOT NULL,
    last_checked_at  TEXT,
    last_error       TEXT                            -- error code only, no message text
);

CREATE TABLE sharepoint_items (
    source_link_id   TEXT NOT NULL REFERENCES sharepoint_link_state(source_link_id) ON DELETE CASCADE,
    item_id          TEXT NOT NULL,
    origin_path      TEXT NOT NULL CHECK (length(origin_path) BETWEEN 1 AND 2048),
    ctag             TEXT NOT NULL,
    etag             TEXT NOT NULL,
    version_label    TEXT,
    changed_at       TEXT NOT NULL,
    changed_by       TEXT,
    removed          INTEGER NOT NULL DEFAULT 0 CHECK (removed IN (0,1)),
    PRIMARY KEY (source_link_id, item_id)
);
CREATE INDEX sharepoint_items_path ON sharepoint_items (source_link_id, origin_path);

CREATE TABLE sharepoint_origin_changes (
    id               TEXT PRIMARY KEY,               -- UUIDv7
    source_link_id   TEXT NOT NULL REFERENCES sharepoint_link_state(source_link_id) ON DELETE CASCADE,
    origin_path      TEXT NOT NULL,
    new_version      TEXT NOT NULL,
    changed_by       TEXT,
    changed_at       TEXT NOT NULL,
    removed          INTEGER NOT NULL CHECK (removed IN (0,1)),
    found_at         TEXT NOT NULL
);
CREATE INDEX sharepoint_origin_changes_found ON sharepoint_origin_changes (source_link_id, found_at);
