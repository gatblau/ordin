-- 0002_items (agent profile; created everywhere, empty on the desktop and the personal server)
CREATE TABLE knowledge_items (
    item_id        TEXT PRIMARY KEY,
    source         TEXT NOT NULL,
    kind           TEXT NOT NULL CHECK (kind IN ('chat-history','entries','notes')),
    scope_id       TEXT,
    connection     TEXT,
    origin_id      TEXT,
    origin_time    TEXT NOT NULL,
    sender         TEXT,
    sender_name    TEXT,
    from_program   INTEGER NOT NULL DEFAULT 0,
    own            INTEGER NOT NULL DEFAULT 0,
    thread_root    TEXT,
    body           TEXT NOT NULL CHECK (length(body) BETWEEN 1 AND 40000),
    body_hash      TEXT NOT NULL,
    language       TEXT NOT NULL,
    has_link       INTEGER NOT NULL DEFAULT 0,
    has_steps      INTEGER NOT NULL DEFAULT 0,
    verified       INTEGER,
    added_by       TEXT,
    verified_by    TEXT,
    run_id         TEXT,
    retired        INTEGER NOT NULL DEFAULT 0,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    CHECK (kind <> 'chat-history' OR (origin_id IS NOT NULL AND sender IS NOT NULL AND scope_id IS NOT NULL AND connection IS NOT NULL)),
    CHECK (kind <> 'entries' OR (verified IS NOT NULL AND scope_id IS NOT NULL)),
    CHECK (kind <> 'notes' OR run_id IS NOT NULL)
);
CREATE UNIQUE INDEX knowledge_items_origin ON knowledge_items (source, scope_id, origin_id) WHERE kind = 'chat-history';
CREATE INDEX knowledge_items_scope_time ON knowledge_items (source, scope_id, origin_time DESC);
CREATE INDEX knowledge_items_thread ON knowledge_items (source, scope_id, thread_root) WHERE thread_root IS NOT NULL;
CREATE INDEX knowledge_items_sender ON knowledge_items (connection, sender) WHERE kind = 'chat-history';
CREATE INDEX knowledge_items_kind_time ON knowledge_items (kind, origin_time);
CREATE VIRTUAL TABLE knowledge_items_fts_porter USING fts5(body, content='knowledge_items', content_rowid='rowid',
    tokenize = 'porter unicode61 remove_diacritics 2');
CREATE VIRTUAL TABLE knowledge_items_fts_plain  USING fts5(body, content='knowledge_items', content_rowid='rowid',
    tokenize = 'unicode61 remove_diacritics 2');
CREATE TRIGGER knowledge_items_fts_insert AFTER INSERT ON knowledge_items BEGIN
    INSERT INTO knowledge_items_fts_porter (rowid, body) SELECT new.rowid, new.body WHERE new.language = 'english';
    INSERT INTO knowledge_items_fts_plain (rowid, body) SELECT new.rowid, new.body WHERE new.language <> 'english';
END;
CREATE TRIGGER knowledge_items_fts_update AFTER UPDATE OF body, language ON knowledge_items BEGIN
    INSERT INTO knowledge_items_fts_porter (knowledge_items_fts_porter, rowid, body)
        SELECT 'delete', old.rowid, old.body WHERE old.language = 'english';
    INSERT INTO knowledge_items_fts_plain (knowledge_items_fts_plain, rowid, body)
        SELECT 'delete', old.rowid, old.body WHERE old.language <> 'english';
    INSERT INTO knowledge_items_fts_porter (rowid, body) SELECT new.rowid, new.body WHERE new.language = 'english';
    INSERT INTO knowledge_items_fts_plain (rowid, body) SELECT new.rowid, new.body WHERE new.language <> 'english';
END;
CREATE TRIGGER knowledge_items_fts_delete AFTER DELETE ON knowledge_items BEGIN
    INSERT INTO knowledge_items_fts_porter (knowledge_items_fts_porter, rowid, body)
        SELECT 'delete', old.rowid, old.body WHERE old.language = 'english';
    INSERT INTO knowledge_items_fts_plain (knowledge_items_fts_plain, rowid, body)
        SELECT 'delete', old.rowid, old.body WHERE old.language <> 'english';
END;
