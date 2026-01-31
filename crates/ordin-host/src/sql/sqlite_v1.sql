-- ordin_host/0001_init (OrdinHost.md Data Model). SQLite only: the agent profile builds no Ordin Host.
-- spec: ordin-desktop/components/OrdinHost#SA-RECON-07
CREATE TABLE ordin_conversations (
  id              TEXT PRIMARY KEY,
  workspace_id    TEXT NOT NULL,
  run_id          TEXT NOT NULL,
  state           TEXT NOT NULL CHECK (state IN ('current','earlier')),
  started_at      TEXT NOT NULL,
  ended_at        TEXT,
  ended_by        TEXT CHECK (ended_by IN ('summarised','started_fresh','after_break','context_limit')),
  seed_summary    TEXT CHECK (seed_summary IS NULL OR length(seed_summary) <= 16000),
  CHECK ((state = 'current') = (ended_at IS NULL))
);
CREATE UNIQUE INDEX ordin_conversations_one_current ON ordin_conversations (workspace_id) WHERE state = 'current';
CREATE INDEX ordin_conversations_ws ON ordin_conversations (workspace_id, started_at DESC);

CREATE TABLE ordin_messages (
  conversation_id TEXT NOT NULL REFERENCES ordin_conversations (id) ON DELETE CASCADE,
  seq             INTEGER NOT NULL CHECK (seq >= 1),
  author          TEXT NOT NULL CHECK (author IN ('person','ordin')),
  text            TEXT NOT NULL CHECK (length(text) BETWEEN 1 AND 8000),
  block_id        TEXT,
  at              TEXT NOT NULL,
  PRIMARY KEY (conversation_id, seq)
);

CREATE TABLE ordin_blocks (
  block_id        TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL REFERENCES ordin_conversations (id) ON DELETE CASCADE,
  workspace_id    TEXT NOT NULL,
  author          TEXT NOT NULL CHECK (author IN ('person','ordin')),
  body            TEXT NOT NULL,
  waiting         INTEGER NOT NULL CHECK (waiting IN (0,1)),
  closed          INTEGER NOT NULL DEFAULT 0 CHECK (closed IN (0,1)),
  shown_at        TEXT NOT NULL,
  CHECK (NOT (waiting = 1 AND closed = 1))
);
CREATE INDEX ordin_blocks_ws ON ordin_blocks (workspace_id, shown_at DESC);

CREATE TABLE workflows (
  id              TEXT PRIMARY KEY,
  workspace_id    TEXT NOT NULL,
  name            TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
  state           TEXT NOT NULL CHECK (state IN ('not_saved_yet','saved','needs_agents')),
  origin          TEXT NOT NULL,
  shared          TEXT,
  created_at      TEXT NOT NULL
);
CREATE UNIQUE INDEX workflows_name ON workflows (workspace_id, name);

CREATE TABLE workflow_versions (
  workflow_id            TEXT NOT NULL REFERENCES workflows (id) ON DELETE CASCADE,
  version                INTEGER NOT NULL CHECK (version >= 1),
  purpose                TEXT NOT NULL,
  change_note            TEXT NOT NULL CHECK (length(change_note) BETWEEN 1 AND 200),
  content                TEXT NOT NULL,
  content_sha256         TEXT NOT NULL CHECK (length(content_sha256) = 64),
  restored_from_version  INTEGER,
  saved_at               TEXT NOT NULL,
  PRIMARY KEY (workflow_id, version)
);

CREATE TABLE workflow_drafts (
  workflow_id     TEXT PRIMARY KEY REFERENCES workflows (id) ON DELETE CASCADE,
  base_version    INTEGER,
  content         TEXT NOT NULL,
  check_report    TEXT NOT NULL,
  drafted_by      TEXT NOT NULL CHECK (drafted_by IN ('ordin','import','repository','duplicate')),
  drafted_at      TEXT NOT NULL
);

CREATE TABLE crew_charters (
  id              TEXT PRIMARY KEY,
  workspace_id    TEXT NOT NULL,
  body            TEXT NOT NULL,
  state           TEXT NOT NULL CHECK (state IN ('written','valid','started','kept','discarded','archived')),
  workflow_id     TEXT,
  workflow_version INTEGER,
  started_by      TEXT,
  crew_run_id     TEXT,
  run_inputs      TEXT,
  written_at      TEXT NOT NULL
);
CREATE UNIQUE INDEX crew_charters_run ON crew_charters (crew_run_id) WHERE crew_run_id IS NOT NULL;
CREATE INDEX crew_charters_ws ON crew_charters (workspace_id, written_at DESC);

CREATE TABLE voice_settings (
  id              INTEGER PRIMARY KEY CHECK (id = 1),
  body            TEXT NOT NULL,
  updated_at      TEXT NOT NULL
);
