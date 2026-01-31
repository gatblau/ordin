CREATE TABLE run_threads (
    thread_id        TEXT PRIMARY KEY,
    started_by_kind  TEXT NOT NULL CHECK (started_by_kind IN ('caller','schedule','intake','chat')),
    started_by_id    TEXT NOT NULL CHECK (length(started_by_id) BETWEEN 1 AND 512),
    started_by_asker TEXT,
    created_at       TEXT NOT NULL,
    CHECK ((started_by_kind = 'chat') = (started_by_asker IS NOT NULL))
);
CREATE INDEX run_threads_chat_asker_idx ON run_threads (started_by_id, started_by_asker)
    WHERE started_by_kind = 'chat';

CREATE TABLE run_records (
    id                  TEXT PRIMARY KEY,
    seq                 INTEGER NOT NULL UNIQUE,
    thread_id           TEXT NOT NULL REFERENCES run_threads (thread_id) ON DELETE CASCADE,
    state               TEXT NOT NULL CHECK (state IN ('queued','executing','paused','ended','cancelled','interrupted')),
    end_kind            TEXT CHECK (end_kind IN ('completed','control','failed','run_limit')),
    started_by_kind     TEXT NOT NULL CHECK (started_by_kind IN ('caller','schedule','intake','chat')),
    started_by_id       TEXT NOT NULL CHECK (length(started_by_id) BETWEEN 1 AND 512),
    started_by_asker    TEXT,
    scope_id            TEXT NOT NULL,
    command             TEXT,
    request             TEXT NOT NULL,
    gathered_context    TEXT NOT NULL DEFAULT '{}',
    firing_due          TEXT,
    result              TEXT,
    outcome             TEXT,
    decision            TEXT,
    session_record      BLOB,
    stretch_summaries   TEXT NOT NULL DEFAULT '[]',
    lease_holder        TEXT,
    lease_expires_at    TEXT,
    cancel_requested_at TEXT,
    queued_at           TEXT NOT NULL,
    created_at          TEXT NOT NULL,
    started_at          TEXT,
    ended_at            TEXT,
    CHECK ((started_by_kind = 'chat') = (started_by_asker IS NOT NULL)),
    CHECK ((state = 'executing') = (lease_expires_at IS NOT NULL)),
    CHECK ((state = 'ended') = (end_kind IS NOT NULL)),
    CHECK ((state IN ('ended','cancelled','interrupted')) = (ended_at IS NOT NULL)),
    CHECK ((firing_due IS NULL) OR (started_by_kind = 'schedule'))
);
CREATE INDEX run_records_queued_idx   ON run_records (queued_at, seq) WHERE state = 'queued';
CREATE INDEX run_records_lease_idx    ON run_records (lease_expires_at) WHERE state = 'executing';
CREATE INDEX run_records_open_idx     ON run_records (command) WHERE state IN ('queued','executing','paused');
CREATE UNIQUE INDEX run_records_one_open_per_thread ON run_records (thread_id)
    WHERE state IN ('queued','executing','paused');
CREATE UNIQUE INDEX run_records_one_per_firing ON run_records (started_by_id, firing_due)
    WHERE started_by_kind = 'schedule';
CREATE INDEX run_records_thread_idx   ON run_records (thread_id);
CREATE INDEX run_records_ended_idx    ON run_records (ended_at) WHERE ended_at IS NOT NULL;
CREATE INDEX run_records_scope_idx    ON run_records (scope_id);

CREATE TABLE run_events (
    run_id  TEXT NOT NULL REFERENCES run_records (id) ON DELETE CASCADE,
    seq     INTEGER NOT NULL CHECK (seq >= 1),
    kind    TEXT NOT NULL CHECK (kind IN ('run_state_changed','run_progress')),
    data    TEXT NOT NULL,
    at      TEXT NOT NULL,
    PRIMARY KEY (run_id, seq)
);
