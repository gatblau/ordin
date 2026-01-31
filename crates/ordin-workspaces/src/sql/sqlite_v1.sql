CREATE TABLE workspaces (
    id                   TEXT PRIMARY KEY,
    name                 TEXT NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 80),
    name_key             TEXT NOT NULL,                  -- lower(trim(name)) for the uniqueness rule
    own_folder_place_id  TEXT NOT NULL,
    state                TEXT NOT NULL CHECK (state IN ('active','archived','in_bin')),
    moved_to_bin_at      TEXT,
    created_at           TEXT NOT NULL,
    CHECK ((state = 'in_bin') = (moved_to_bin_at IS NOT NULL))
);
CREATE UNIQUE INDEX workspaces_name_not_in_bin ON workspaces (name_key) WHERE state != 'in_bin';

CREATE TABLE workspace_places (
    workspace_id  TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    place_id      TEXT NOT NULL,
    added_at      TEXT NOT NULL,
    PRIMARY KEY (workspace_id, place_id)
);
CREATE INDEX workspace_places_place ON workspace_places (place_id);

CREATE TABLE folder_grants (
    id            TEXT PRIMARY KEY,
    workspace_id  TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    grantee       TEXT NOT NULL,                         -- JSON Grantee
    grantee_key   TEXT NOT NULL,                         -- 'agent:<id>' or 'crew:<id>'
    place_id      TEXT NOT NULL,
    access        TEXT NOT NULL CHECK (access IN ('read','read_and_change')),
    granted_at    TEXT NOT NULL,
    UNIQUE (workspace_id, grantee_key, place_id),
    FOREIGN KEY (workspace_id, place_id) REFERENCES workspace_places(workspace_id, place_id) ON DELETE CASCADE
);
CREATE INDEX folder_grants_grantee ON folder_grants (grantee_key);

CREATE TABLE source_links (
    id               TEXT PRIMARY KEY,
    workspace_id     TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    kind             TEXT NOT NULL CHECK (kind IN ('local_folder','git_repository_folder','sharepoint_location','confluence_location','git_remote')),
    origin_location  TEXT NOT NULL CHECK (length(origin_location) BETWEEN 1 AND 2048),
    display_name     TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 120),
    place_id         TEXT,
    state            TEXT NOT NULL CHECK (state IN ('proposed','active')),
    added_at         TEXT NOT NULL,
    last_read_at     TEXT,
    UNIQUE (workspace_id, kind, origin_location)
);

CREATE TABLE working_copy_items (
    id                   TEXT PRIMARY KEY,
    source_link_id       TEXT NOT NULL REFERENCES source_links(id) ON DELETE CASCADE,
    origin_path          TEXT NOT NULL CHECK (length(origin_path) BETWEEN 1 AND 2048),
    origin_version_read  TEXT NOT NULL CHECK (length(origin_version_read) BETWEEN 1 AND 256),
    local_version        INTEGER NOT NULL CHECK (local_version >= 1),
    state                TEXT NOT NULL CHECK (state IN ('read','changed','awaiting_push','pushed','origin_changed')),
    read_at              TEXT NOT NULL,
    origin_changed_by    TEXT,                            -- from the adapter's OriginChange
    git_branch           TEXT,                            -- git folder links only (Internal Logic 6.7)
    origin_changed_at    TEXT,
    size_bytes           INTEGER NOT NULL DEFAULT 0 CHECK (size_bytes >= 0), -- bytes read; the retry's budget sum
    UNIQUE (source_link_id, origin_path)
);
CREATE INDEX working_copy_items_origin_changed ON working_copy_items (origin_changed_at);

CREATE TABLE working_copy_unread (
    source_link_id  TEXT NOT NULL REFERENCES source_links(id) ON DELETE CASCADE,
    origin_path     TEXT NOT NULL,
    item_ref        TEXT NOT NULL,                        -- JSON SharePointItemRef or RemoteItemRef of the adapter
    size_bytes      INTEGER NOT NULL,
    PRIMARY KEY (source_link_id, origin_path)
);

CREATE TABLE confluence_links (
    id                TEXT PRIMARY KEY,
    workspace_id      TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    place_id          TEXT NOT NULL,
    folder            TEXT NOT NULL,
    confluence_space  TEXT NOT NULL CHECK (length(confluence_space) BETWEEN 1 AND 255),
    parent_page_id    TEXT NOT NULL CHECK (length(parent_page_id) BETWEEN 1 AND 64 AND parent_page_id GLOB '[0-9]*'),
    audience          TEXT NOT NULL CHECK (length(audience) BETWEEN 1 AND 500),
    approval_id       TEXT,
    approved_at       TEXT,
    state             TEXT NOT NULL CHECK (state IN ('proposed','active','paused')),
    CHECK (state = 'proposed' OR (approval_id IS NOT NULL AND approved_at IS NOT NULL))
);
CREATE INDEX confluence_links_approval ON confluence_links (approval_id);

CREATE TABLE email_links (
    id                     TEXT PRIMARY KEY,
    workspace_id           TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    kind                   TEXT NOT NULL CHECK (kind IN ('folder_or_label','people_and_domains')),
    folder_or_label        TEXT,
    addresses              TEXT NOT NULL,                -- JSON array
    domains                TEXT NOT NULL,                -- JSON array
    display_name           TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 120),
    state                  TEXT NOT NULL CHECK (state IN ('proposed','active')),
    added_at               TEXT NOT NULL
    -- no check columns: the Mail Service keeps them in mail_link_check_state (2C C-26)
);

CREATE TABLE email_grants (
    id                 TEXT PRIMARY KEY,
    workspace_id       TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    agent_instance_id  TEXT NOT NULL,
    access             TEXT NOT NULL CHECK (access IN ('read_email','read_and_draft_email')),
    granted_at         TEXT NOT NULL,
    UNIQUE (workspace_id, agent_instance_id)
);

CREATE TABLE telegram_grants (
    id                 TEXT PRIMARY KEY,
    workspace_id       TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    agent_instance_id  TEXT NOT NULL,
    access             TEXT NOT NULL CHECK (access IN ('read_telegram','read_and_draft_telegram')),
    granted_at         TEXT NOT NULL,
    UNIQUE (workspace_id, agent_instance_id)
);
