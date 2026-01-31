CREATE TABLE source_links_v3 (
    id               TEXT PRIMARY KEY,
    workspace_id     TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    kind             TEXT NOT NULL CHECK (kind IN ('local_folder','git_repository_folder','sharepoint_location','confluence_location','git_remote','jira_issue')),
    origin_location  TEXT NOT NULL CHECK (length(origin_location) BETWEEN 1 AND 2048),
    display_name     TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 120),
    place_id         TEXT,
    state            TEXT NOT NULL CHECK (state IN ('proposed','active')),
    added_at         TEXT NOT NULL,
    last_read_at     TEXT,
    UNIQUE (workspace_id, kind, origin_location)
);
CREATE TABLE working_copy_items_v3 (
    id                   TEXT PRIMARY KEY,
    source_link_id       TEXT NOT NULL REFERENCES source_links_v3(id) ON DELETE CASCADE,
    origin_path          TEXT NOT NULL CHECK (length(origin_path) BETWEEN 1 AND 2048),
    origin_version_read  TEXT NOT NULL CHECK (length(origin_version_read) BETWEEN 1 AND 256),
    local_version        INTEGER NOT NULL CHECK (local_version >= 1),
    state                TEXT NOT NULL CHECK (state IN ('read','changed','awaiting_push','pushed','origin_changed')),
    read_at              TEXT NOT NULL,
    origin_changed_by    TEXT,
    git_branch           TEXT,
    origin_changed_at    TEXT,
    origin_newer_at      TEXT,                            -- Jira issue items only (§J.4)
    size_bytes           INTEGER NOT NULL DEFAULT 0 CHECK (size_bytes >= 0),
    UNIQUE (source_link_id, origin_path)
);
CREATE TABLE working_copy_unread_v3 (
    source_link_id  TEXT NOT NULL REFERENCES source_links_v3(id) ON DELETE CASCADE,
    origin_path     TEXT NOT NULL,
    item_ref        TEXT NOT NULL,
    size_bytes      INTEGER NOT NULL,
    PRIMARY KEY (source_link_id, origin_path)
);
INSERT INTO source_links_v3 SELECT id, workspace_id, kind, origin_location, display_name, place_id, state, added_at, last_read_at FROM source_links;
INSERT INTO working_copy_items_v3 (id, source_link_id, origin_path, origin_version_read, local_version, state, read_at,
                                   origin_changed_by, git_branch, origin_changed_at, size_bytes)
    SELECT id, source_link_id, origin_path, origin_version_read, local_version, state, read_at,
           origin_changed_by, git_branch, origin_changed_at, size_bytes FROM working_copy_items;
INSERT INTO working_copy_unread_v3 SELECT source_link_id, origin_path, item_ref, size_bytes FROM working_copy_unread;
DROP TABLE working_copy_unread;
DROP TABLE working_copy_items;
DROP TABLE source_links;
ALTER TABLE source_links_v3 RENAME TO source_links;
ALTER TABLE working_copy_items_v3 RENAME TO working_copy_items;
ALTER TABLE working_copy_unread_v3 RENAME TO working_copy_unread;
CREATE INDEX working_copy_items_origin_changed ON working_copy_items (origin_changed_at);
