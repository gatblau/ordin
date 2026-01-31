-- spec: ordin-desktop/components/KnowledgeService#SA-KLINK-02
-- 4 page links, names and tags (amendment January 2026, ADR-104)
ALTER TABLE knowledge_pages ADD COLUMN extra_header TEXT NOT NULL DEFAULT ''
    CHECK (length(CAST(extra_header AS BLOB)) <= 65536);
ALTER TABLE knowledge_pages ADD COLUMN tags TEXT NOT NULL DEFAULT '[]';
ALTER TABLE knowledge_pages ADD COLUMN aliases TEXT NOT NULL DEFAULT '[]';
ALTER TABLE knowledge_pages ADD COLUMN title_refused TEXT;
ALTER TABLE knowledge_pages ADD COLUMN links_built_version INTEGER;
ALTER TABLE knowledge_pages ADD COLUMN links_truncated INTEGER NOT NULL DEFAULT 0;

CREATE TABLE knowledge_page_names (
    workspace_id  TEXT NOT NULL,
    name_key      TEXT NOT NULL,
    page_id       TEXT NOT NULL REFERENCES knowledge_pages(id) ON DELETE CASCADE,
    kind          TEXT NOT NULL CHECK (kind IN ('title','alias')),
    PRIMARY KEY (workspace_id, name_key)
);
CREATE INDEX knowledge_page_names_page ON knowledge_page_names (page_id);

CREATE TABLE knowledge_page_tags (
    page_id  TEXT NOT NULL REFERENCES knowledge_pages(id) ON DELETE CASCADE,
    tag_key  TEXT NOT NULL,
    PRIMARY KEY (page_id, tag_key)
);
CREATE INDEX knowledge_page_tags_key ON knowledge_page_tags (tag_key);

CREATE TABLE knowledge_links (
    page_id         TEXT NOT NULL REFERENCES knowledge_pages(id) ON DELETE CASCADE,
    ordinal         INTEGER NOT NULL CHECK (ordinal BETWEEN 1 AND 1000),
    workspace_id    TEXT NOT NULL,
    target_page_id  TEXT REFERENCES knowledge_pages(id) ON DELETE SET NULL,
    target_key      TEXT NOT NULL,
    target_text     TEXT NOT NULL,
    section         TEXT,
    form            TEXT NOT NULL CHECK (form IN ('wiki','embed','markdown')),
    start_byte      INTEGER NOT NULL,
    end_byte        INTEGER NOT NULL CHECK (end_byte > start_byte),
    PRIMARY KEY (page_id, ordinal)
);
CREATE INDEX knowledge_links_target ON knowledge_links (target_page_id);
CREATE INDEX knowledge_links_unresolved ON knowledge_links (workspace_id, target_key) WHERE target_page_id IS NULL;

CREATE TABLE knowledge_suggestions (
    id              TEXT PRIMARY KEY,
    page_id         TEXT NOT NULL REFERENCES knowledge_pages(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL CHECK (kind IN ('link','duplicate')),
    other_page_id   TEXT NOT NULL REFERENCES knowledge_pages(id) ON DELETE CASCADE,
    matched_key     TEXT NOT NULL DEFAULT '',
    matched_text    TEXT,
    context         TEXT,
    start_byte      INTEGER,
    end_byte        INTEGER,
    body_hash       TEXT,
    reason          TEXT CHECK (reason IN ('names','text')),
    created_at      TEXT NOT NULL,
    UNIQUE (page_id, kind, other_page_id, matched_key),
    CHECK ((kind = 'link') = (start_byte IS NOT NULL AND body_hash IS NOT NULL AND reason IS NULL))
);

CREATE TABLE knowledge_suggestion_dismissals (
    page_id         TEXT NOT NULL REFERENCES knowledge_pages(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL CHECK (kind IN ('link','duplicate')),
    other_page_id   TEXT NOT NULL,
    matched_key     TEXT NOT NULL DEFAULT '',
    dismissed_at    TEXT NOT NULL,
    PRIMARY KEY (page_id, kind, other_page_id, matched_key)
);

CREATE TABLE knowledge_link_rewrites (
    target_page_id  TEXT NOT NULL REFERENCES knowledge_pages(id) ON DELETE CASCADE,
    old_key         TEXT NOT NULL,
    due_at          TEXT NOT NULL,
    PRIMARY KEY (target_page_id, old_key)
);

INSERT INTO knowledge_page_names (workspace_id, name_key, page_id, kind)
    SELECT workspace_id, lower(trim(title)), id, 'title' FROM knowledge_pages WHERE state <> 'removed';
