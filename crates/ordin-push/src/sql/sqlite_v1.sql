-- push v1, SQLite (PushService.md, Data Model). Released text: never edit (DatabaseMigrations.md rule 10).
CREATE TABLE pending_pushes (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  run_id TEXT,
  request TEXT NOT NULL CHECK (length(request) BETWEEN 1 AND 4000),
  origins TEXT NOT NULL,
  approval_id TEXT,
  state TEXT NOT NULL CHECK (state IN ('proposed','held','approved','rejected','closed')),
  created_at TEXT NOT NULL,
  CHECK (state NOT IN ('approved') OR approval_id IS NOT NULL)
);
CREATE INDEX ix_pending_pushes_workspace ON pending_pushes(workspace_id, state);
CREATE UNIQUE INDEX ux_pending_pushes_approval ON pending_pushes(approval_id) WHERE approval_id IS NOT NULL;

CREATE TABLE pending_push_items (
  pending_push_id TEXT NOT NULL REFERENCES pending_pushes(id) ON DELETE CASCADE,
  working_copy_item_id TEXT NOT NULL,
  position INTEGER NOT NULL,
  summary TEXT NOT NULL CHECK (length(summary) BETWEEN 1 AND 120),
  change_view TEXT,
  ordins_local_version INTEGER NOT NULL,
  pending_action_key TEXT,
  external_action_key TEXT,
  result_code TEXT,
  PRIMARY KEY (pending_push_id, working_copy_item_id)
);
CREATE INDEX ix_push_items_item ON pending_push_items(working_copy_item_id);

CREATE TABLE pull_request_submissions (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL,
  origin TEXT NOT NULL,
  proposal TEXT NOT NULL,
  approval_id TEXT,
  action_key TEXT,
  pull_request TEXT,
  state TEXT NOT NULL CHECK (state IN ('proposed','approved','sent','rejected','failed')),
  failure_code TEXT,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);

CREATE TABLE copy_operations (
  id TEXT PRIMARY KEY,
  published_copy_id TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('first_publish','update','audience_hold','remove','update_copies')),
  approval_id TEXT,
  action_key TEXT,
  original_version TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('awaiting_approval','sending','done','failed','held')),
  created_at TEXT NOT NULL
);

CREATE TABLE copy_audiences (
  published_copy_id TEXT PRIMARY KEY,
  line TEXT NOT NULL CHECK (length(line) BETWEEN 1 AND 500),
  people INTEGER,
  everyone_who_can_open INTEGER NOT NULL CHECK (everyone_who_can_open IN (0,1)),
  principals_digest TEXT NOT NULL,
  recorded_at TEXT NOT NULL
);
