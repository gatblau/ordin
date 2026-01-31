-- confluence v1, SQLite (ConfluenceAdapter.md, Data Model). Released text: never edit (DatabaseMigrations.md rule 10).
-- spec: ordin-desktop/components/ConfluenceAdapter#confluence_link_state
CREATE TABLE confluence_link_state (
    source_link_id   TEXT PRIMARY KEY,
    space_key        TEXT NOT NULL,
    root_page_id     TEXT,                              -- NULL for a whole-space link
    tracked_at       TEXT NOT NULL,
    last_checked_at  TEXT NOT NULL,
    last_error       TEXT
);

-- spec: ordin-desktop/components/ConfluenceAdapter#confluence_items
CREATE TABLE confluence_items (
    source_link_id   TEXT NOT NULL REFERENCES confluence_link_state(source_link_id) ON DELETE CASCADE,
    item_key         TEXT NOT NULL,                     -- 'page:<id>' or 'attachment:<page id>:<id>'
    origin_path      TEXT NOT NULL CHECK (length(origin_path) BETWEEN 1 AND 2048),
    version_number   INTEGER NOT NULL,
    changed_at       TEXT NOT NULL,
    changed_by       TEXT,
    removed          INTEGER NOT NULL DEFAULT 0 CHECK (removed IN (0,1)),
    PRIMARY KEY (source_link_id, item_key)
);

-- spec: ordin-desktop/components/ConfluenceAdapter#confluence_origin_changes
CREATE TABLE confluence_origin_changes (
    id               TEXT PRIMARY KEY,
    source_link_id   TEXT NOT NULL REFERENCES confluence_link_state(source_link_id) ON DELETE CASCADE,
    origin_path      TEXT NOT NULL,
    new_version      TEXT NOT NULL,
    changed_by       TEXT,
    changed_at       TEXT NOT NULL,
    removed          INTEGER NOT NULL CHECK (removed IN (0,1)),
    found_at         TEXT NOT NULL
);
CREATE INDEX confluence_origin_changes_found ON confluence_origin_changes (source_link_id, found_at);

-- spec: ordin-desktop/components/ConfluenceAdapter#confluence_folder_pages
CREATE TABLE confluence_folder_pages (
    confluence_link_id  TEXT NOT NULL,
    folder_path         TEXT NOT NULL,                  -- relative to the link's folder; '' = the link root
    page_id             TEXT NOT NULL,
    PRIMARY KEY (confluence_link_id, folder_path)
);

-- spec: ordin-desktop/components/ConfluenceAdapter#confluence_ready_items
CREATE TABLE confluence_ready_items (
    confluence_link_id  TEXT NOT NULL,
    kind                TEXT NOT NULL CHECK (kind IN ('new','renamed','removed')),
    path                TEXT NOT NULL,
    from_path           TEXT,
    published_copy_id   TEXT,
    found_at            TEXT NOT NULL,
    PRIMARY KEY (confluence_link_id, path)
);

-- spec: ordin-desktop/components/ConfluenceAdapter#confluence_publish_requests
CREATE TABLE confluence_publish_requests (
    id                  TEXT PRIMARY KEY,
    confluence_link_id  TEXT NOT NULL,
    approval_id         TEXT NOT NULL UNIQUE,
    items               TEXT NOT NULL,                  -- JSON Vec<ApprovedItem>: each ReadyItem with its content hash
    created_at          TEXT NOT NULL
);
