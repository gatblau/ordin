-- memory_recall_wider.commit_id widened from 7–64 to 1–256 characters (a CHECK cannot be altered).
-- The guard table's CHECK aborts the step, and with it the transaction, if the copy lost a row.
CREATE TABLE memory_recall_wider_v2 (
    recall_id   TEXT NOT NULL REFERENCES memory_recalls(id) ON DELETE CASCADE,
    position    INTEGER NOT NULL CHECK (position BETWEEN 0 AND 49),
    agent       TEXT NOT NULL CHECK (length(agent) BETWEEN 1 AND 200),
    repository  TEXT NOT NULL CHECK (length(repository) BETWEEN 1 AND 2048),
    path        TEXT NOT NULL CHECK (length(CAST(path AS BLOB)) BETWEEN 1 AND 1024),
    commit_id   TEXT NOT NULL CHECK (length(commit_id) BETWEEN 1 AND 256),
    section     TEXT CHECK (section IS NULL OR length(section) <= 200),
    page_title  TEXT CHECK (page_title IS NULL OR length(page_title) <= 200),
    as_of       TEXT NOT NULL,
    answer_id   TEXT,
    more_found  INTEGER NOT NULL DEFAULT 0 CHECK (more_found IN (0,1)),
    PRIMARY KEY (recall_id, position)
);
INSERT INTO memory_recall_wider_v2 SELECT * FROM memory_recall_wider;
CREATE TEMP TABLE memory_recall_wider_guard (same INTEGER NOT NULL CHECK (same = 1));
INSERT INTO memory_recall_wider_guard
    SELECT (SELECT COUNT(*) FROM memory_recall_wider) = (SELECT COUNT(*) FROM memory_recall_wider_v2);
DROP TABLE memory_recall_wider_guard;
DROP TABLE memory_recall_wider;
ALTER TABLE memory_recall_wider_v2 RENAME TO memory_recall_wider;
