-- JA-EXT-1: an external action the destination refused is `refused`. SQLite cannot change a CHECK, so the
-- table is rebuilt with every row and index kept; no other table refers to it.
CREATE TABLE external_actions_v2 (
    action_key    TEXT PRIMARY KEY CHECK (length(action_key) = 36),
    run_id        TEXT CHECK (run_id IS NULL OR length(run_id) = 36),   -- NULL: no run (2C C-7)
    kind          TEXT NOT NULL CHECK (kind IN ('slack_post','connection_write','wrightrun_run_start',
                       'email_send','origin_push','copy_publish','telegram_post',
                       'pull_request_opened','handover_written')),
    destination   TEXT NOT NULL CHECK (length(destination) BETWEEN 1 AND 2048),
    state         TEXT NOT NULL CHECK (state IN ('sent','confirmed','unknown_outcome','refused')),
    sent_at       TEXT NOT NULL,
    confirmed_at  TEXT,
    CHECK ((state = 'confirmed') = (confirmed_at IS NOT NULL))
);
INSERT INTO external_actions_v2 (action_key, run_id, kind, destination, state, sent_at, confirmed_at)
    SELECT action_key, run_id, kind, destination, state, sent_at, confirmed_at FROM external_actions;
DROP TABLE external_actions;
ALTER TABLE external_actions_v2 RENAME TO external_actions;
CREATE INDEX external_actions_run   ON external_actions (run_id);
CREATE INDEX external_actions_state ON external_actions (state, sent_at DESC);
CREATE INDEX external_actions_kind  ON external_actions (kind, state, sent_at);
