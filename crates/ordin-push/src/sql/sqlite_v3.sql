-- push v3, SQLite (PushService.md Data Model, step 3). Additive: three tables, no existing table changes.
-- What a first publish or a pull request knew in memory only: the copy's workspace and chosen name, the
-- file a knowledge page's copy is made from, and the file bytes of a pull request until it is sent.
CREATE TABLE copy_requests (
  published_copy_id TEXT PRIMARY KEY,
  workspace_id      TEXT NOT NULL,
  original_name     TEXT NOT NULL,
  file_name         TEXT NOT NULL
);

CREATE TABLE knowledge_page_files (
  knowledge_page_id TEXT PRIMARY KEY,
  place_id          TEXT NOT NULL,
  path              TEXT NOT NULL
);
CREATE INDEX ix_knowledge_page_files_file ON knowledge_page_files(place_id, path);

CREATE TABLE pull_request_files (
  submission_id TEXT NOT NULL,
  position      INTEGER NOT NULL,
  path          TEXT NOT NULL,
  content       BLOB NOT NULL,
  PRIMARY KEY (submission_id, position)
);
