-- 0002_items (agent profile, scaled shape)
CREATE TABLE knowledge_items (
    item_id      uuid PRIMARY KEY,
    source       text NOT NULL,
    kind         text NOT NULL CHECK (kind IN ('chat-history','entries','notes')),
    scope_id     uuid,
    connection   text,
    origin_id    text,
    origin_time  timestamptz NOT NULL,
    sender       text,
    sender_name  text,
    from_program boolean NOT NULL DEFAULT false,
    own          boolean NOT NULL DEFAULT false,
    thread_root  text,
    body         text NOT NULL CHECK (char_length(body) BETWEEN 1 AND 40000),
    body_hash    text NOT NULL,
    language     text NOT NULL,
    tsv          tsvector NOT NULL,
    has_link     boolean NOT NULL DEFAULT false,
    has_steps    boolean NOT NULL DEFAULT false,
    verified     boolean,
    added_by     text,
    verified_by  text,
    run_id       uuid,
    retired      boolean NOT NULL DEFAULT false,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    CHECK (kind <> 'chat-history' OR (origin_id IS NOT NULL AND sender IS NOT NULL AND scope_id IS NOT NULL AND connection IS NOT NULL)),
    CHECK (kind <> 'entries' OR (verified IS NOT NULL AND scope_id IS NOT NULL)),
    CHECK (kind <> 'notes' OR run_id IS NOT NULL)
);
CREATE INDEX knowledge_items_tsv ON knowledge_items USING GIN (tsv);
CREATE UNIQUE INDEX knowledge_items_origin ON knowledge_items (source, scope_id, origin_id) WHERE kind = 'chat-history';
CREATE INDEX knowledge_items_scope_time ON knowledge_items (source, scope_id, origin_time DESC);
CREATE INDEX knowledge_items_thread ON knowledge_items (source, scope_id, thread_root) WHERE thread_root IS NOT NULL;
CREATE INDEX knowledge_items_sender ON knowledge_items (connection, sender) WHERE kind = 'chat-history';
CREATE INDEX knowledge_items_kind_time ON knowledge_items (kind, origin_time);
