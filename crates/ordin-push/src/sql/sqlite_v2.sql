-- push v2, SQLite (PushService.md, amendment January 2026, ADR-103). Additive: one table and two indexes.
CREATE TABLE team_share_submissions (
  share_id      TEXT PRIMARY KEY,
  workspace_id  TEXT NOT NULL,
  approval_id   TEXT,
  state         TEXT NOT NULL CHECK (state IN ('proposed','approved','sending','sent','failed','rejected','withdrawn')),
  clone_path    TEXT NOT NULL,
  commit_id     TEXT NOT NULL CHECK (length(commit_id) = 40),
  host          TEXT NOT NULL,
  repository    TEXT NOT NULL,
  branch        TEXT NOT NULL CHECK (length(CAST(branch AS BLOB)) BETWEEN 1 AND 255),
  review_branch TEXT CHECK (review_branch IS NULL OR length(CAST(review_branch AS BLOB)) BETWEEN 1 AND 255),
  title         TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 256),
  body          TEXT NOT NULL,
  pull_request  TEXT,
  action_key    TEXT,
  refused_keys  TEXT NOT NULL DEFAULT '[]',
  moves         INTEGER NOT NULL DEFAULT 0 CHECK (moves BETWEEN 0 AND 3),
  result_code   TEXT,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL,
  CHECK (state NOT IN ('approved','sending','sent') OR approval_id IS NOT NULL)
);
CREATE UNIQUE INDEX ux_team_share_approval ON team_share_submissions(approval_id) WHERE approval_id IS NOT NULL;
CREATE INDEX ix_team_share_open ON team_share_submissions(state) WHERE state IN ('proposed','approved','sending');
