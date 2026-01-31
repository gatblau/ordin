CREATE TABLE places (
    id              TEXT PRIMARY KEY,
    friendly_name   TEXT NOT NULL CHECK (length(friendly_name) BETWEEN 1 AND 120),
    path            TEXT NOT NULL UNIQUE CHECK (length(path) BETWEEN 1 AND 4096),
    kind            TEXT NOT NULL CHECK (kind IN ('persons_folder','workspace_folder','agent_folder',
                                                  'working_copy','coding_task_worktree')),
    added_at        TEXT NOT NULL,
    watched         INTEGER NOT NULL DEFAULT 0 CHECK (watched IN (0,1)),
    index_state     TEXT NOT NULL DEFAULT 'none' CHECK (index_state IN ('none','building','ready')),
    index_indexed   INTEGER NOT NULL DEFAULT 0,
    index_total     INTEGER NOT NULL DEFAULT 0,
    index_skipped   INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE file_index (
    place_id    TEXT NOT NULL REFERENCES places(id) ON DELETE CASCADE,
    path        TEXT NOT NULL,
    parent      TEXT NOT NULL,
    name        TEXT NOT NULL,
    is_folder   INTEGER NOT NULL CHECK (is_folder IN (0,1)),
    kind        TEXT NOT NULL,
    size_bytes  INTEGER NOT NULL,
    mtime_ns    INTEGER NOT NULL,
    hash        TEXT,
    changed_at  TEXT NOT NULL,
    changed_by  TEXT,
    text_state  TEXT NOT NULL DEFAULT 'pending' CHECK (text_state IN ('pending','text','name_only')),
    PRIMARY KEY (place_id, path)
);
CREATE INDEX file_index_parent_name    ON file_index (place_id, parent, name);
CREATE INDEX file_index_parent_changed ON file_index (place_id, parent, changed_at);
CREATE INDEX file_index_name           ON file_index (name);

-- spec: ordin-desktop/components/FileService#SA-DB-01
CREATE VIRTUAL TABLE file_index_fts USING fts5 (
    place_id UNINDEXED, path UNINDEXED, body,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TABLE file_versions (
    id          TEXT PRIMARY KEY,
    place_id    TEXT NOT NULL,
    path        TEXT NOT NULL CHECK (length(path) BETWEEN 1 AND 4096),
    version     INTEGER NOT NULL CHECK (version >= 1),
    hash        TEXT NOT NULL CHECK (length(hash) = 64),
    origin      TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    UNIQUE (place_id, path, version)
);
CREATE INDEX file_versions_created ON file_versions (created_at);
CREATE INDEX file_versions_hash    ON file_versions (hash);

CREATE TABLE published_copies (
    id                          TEXT PRIMARY KEY,
    original                    TEXT NOT NULL,
    original_key                TEXT NOT NULL,
    destination                 TEXT NOT NULL,
    destination_item_id         TEXT,
    format                      TEXT NOT NULL CHECK (format IN ('page','word','pdf','slides','exact_file')),
    maker                       TEXT NOT NULL CHECK (maker IN ('ordin','word_craft','pdf_craft','deck_craft','none')),
    published_via               TEXT NOT NULL,
    confluence_link_id          TEXT,
    followed_branch             TEXT,
    keep_up_to_date             INTEGER NOT NULL CHECK (keep_up_to_date IN (0,1)),
    audience                    TEXT NOT NULL CHECK (length(audience) BETWEEN 1 AND 500),
    original_version_published  TEXT,
    destination_version_pushed  TEXT,
    last_updated_at             TEXT,
    state                       TEXT NOT NULL CHECK (state IN ('ready_to_publish','awaiting_approval','current',
                                   'out_of_date','updating','changed_at_destination','failed',
                                   'not_kept_up_to_date','removed')),
    failure_reason              TEXT CHECK (failure_reason IS NULL OR length(failure_reason) <= 500),
    CHECK ((state = 'failed') = (failure_reason IS NOT NULL))
);
CREATE INDEX published_copies_original ON published_copies (original_key);
CREATE INDEX published_copies_link     ON published_copies (confluence_link_id);

CREATE TABLE held_edits (
    place_id         TEXT NOT NULL,
    path             TEXT NOT NULL,
    file_version_id  TEXT NOT NULL REFERENCES file_versions(id),
    saved_at         TEXT NOT NULL,
    PRIMARY KEY (place_id, path)
);
