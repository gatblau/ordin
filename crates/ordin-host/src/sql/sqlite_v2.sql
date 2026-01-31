-- ordin_host/0002_continue (OrdinHost.md Data Model step 2; ADR-108): a widening rebuild under
-- DatabaseMigrations rule 12a. Runs inside the migration transaction with foreign keys on.
-- spec: ordin-desktop/components/OrdinHost#OH-CC-10
--
-- OrdinHost.md Data Model step 2, as amended: the two new tables refer to `ordin_conversations_new`.
-- With foreign keys on (the Journal's datastore enables them, and a pragma cannot change inside the transaction), dropping
-- the old `ordin_conversations` would then cascade into `ordin_messages_new` and empty it, against the
-- scenario *Migration 0002 keeps every row*. The new tables refer to `ordin_conversations_new`; the
-- rename below rewrites both references to `ordin_conversations`, so the final schema is the spec's.
--
-- OrdinHost.md Data Model step 2, as amended: `kind` allows 'slack_turn' and 'consolidation_turn'
-- (the ADR-111 amendment stores consolidation turns in the column this step widened).
CREATE TEMP TABLE ordin_blocks_copy AS SELECT * FROM ordin_blocks;

CREATE TABLE ordin_conversations_new (
  id              TEXT PRIMARY KEY,
  workspace_id    TEXT NOT NULL,
  run_id          TEXT NOT NULL,
  state           TEXT NOT NULL CHECK (state IN ('current','earlier')),
  started_at      TEXT NOT NULL,
  ended_at        TEXT,
  ended_by        TEXT CHECK (ended_by IN ('summarised','started_fresh','after_break','context_limit','put_away')),
  seed_summary    TEXT CHECK (seed_summary IS NULL OR length(seed_summary) <= 16000),
  end_summary     TEXT CHECK (end_summary IS NULL OR length(end_summary) <= 16000),
  continues_id    TEXT REFERENCES ordin_conversations_new (id) ON DELETE SET NULL,
  CHECK ((state = 'current') = (ended_at IS NULL)),
  CHECK (continues_id IS NULL OR continues_id <> id)
);

CREATE TABLE ordin_messages_new (
  conversation_id TEXT NOT NULL REFERENCES ordin_conversations_new (id) ON DELETE CASCADE,
  seq             INTEGER NOT NULL CHECK (seq >= 1),
  author          TEXT NOT NULL CHECK (author IN ('person','ordin','system')),
  kind            TEXT CHECK (kind IS NULL OR kind IN ('slack_turn','consolidation_turn')),
  text            TEXT NOT NULL CHECK (length(text) BETWEEN 1 AND 8000),
  block_id        TEXT,
  at              TEXT NOT NULL,
  PRIMARY KEY (conversation_id, seq),
  CHECK ((author = 'system') = (kind IS NOT NULL))
);

INSERT INTO ordin_conversations_new (id, workspace_id, run_id, state, started_at, ended_at, ended_by, seed_summary, end_summary, continues_id)
  SELECT id, workspace_id, run_id, state, started_at, ended_at, ended_by, seed_summary, NULL, NULL FROM ordin_conversations;
INSERT INTO ordin_messages_new (conversation_id, seq, author, kind, text, block_id, at)
  SELECT conversation_id, seq, author, NULL, text, block_id, at FROM ordin_messages;

DROP TABLE ordin_messages;
DROP TABLE ordin_conversations;
ALTER TABLE ordin_conversations_new RENAME TO ordin_conversations;
ALTER TABLE ordin_messages_new RENAME TO ordin_messages;

INSERT INTO ordin_blocks SELECT * FROM ordin_blocks_copy;
DROP TABLE ordin_blocks_copy;

CREATE UNIQUE INDEX ordin_conversations_one_current ON ordin_conversations (workspace_id) WHERE state = 'current';
CREATE INDEX ordin_conversations_ws ON ordin_conversations (workspace_id, started_at DESC);
CREATE UNIQUE INDEX ordin_conversations_continued_once ON ordin_conversations (continues_id) WHERE continues_id IS NOT NULL;

-- `PRAGMA foreign_key_check` must return nothing before commit: any row fails the step here.
CREATE TEMP TABLE ordin_host_fk_guard (found INTEGER CHECK (found IS NULL));
INSERT INTO ordin_host_fk_guard SELECT 1 FROM pragma_foreign_key_check;
DROP TABLE ordin_host_fk_guard;
