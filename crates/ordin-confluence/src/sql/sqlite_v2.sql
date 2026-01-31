-- confluence migration step 2 (ConfluenceAdapter.md amendment 2026-01-24, Data Model): the Ready list
-- gains `changed` items. SQLite cannot widen a CHECK, so the table is rebuilt inside the migration's
-- transaction; every row is kept.
-- spec: ordin-desktop/components/ConfluenceAdapter#SA-CONF-AUTO-03
CREATE TABLE confluence_ready_items_new (
    confluence_link_id  TEXT NOT NULL,
    kind                TEXT NOT NULL CHECK (kind IN ('new','renamed','removed','changed')),
    path                TEXT NOT NULL,
    from_path           TEXT,
    published_copy_id   TEXT,
    found_at            TEXT NOT NULL,
    PRIMARY KEY (confluence_link_id, path)
);
INSERT INTO confluence_ready_items_new (confluence_link_id, kind, path, from_path, published_copy_id, found_at)
    SELECT confluence_link_id, kind, path, from_path, published_copy_id, found_at FROM confluence_ready_items;
DROP TABLE confluence_ready_items;
ALTER TABLE confluence_ready_items_new RENAME TO confluence_ready_items;
