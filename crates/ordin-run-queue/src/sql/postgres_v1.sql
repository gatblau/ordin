CREATE TABLE run_threads (
    thread_id        uuid        PRIMARY KEY,
    started_by_kind  text        NOT NULL CHECK (started_by_kind IN ('caller','schedule','intake','chat')),
    started_by_id    text        NOT NULL CHECK (char_length(started_by_id) BETWEEN 1 AND 512),
    started_by_asker text,
    created_at       timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT run_threads_asker_with_chat CHECK ((started_by_kind = 'chat') = (started_by_asker IS NOT NULL))
);
CREATE INDEX run_threads_chat_asker_idx ON run_threads (started_by_id, started_by_asker)
    WHERE started_by_kind = 'chat';

CREATE TABLE run_records (
    id                  uuid        PRIMARY KEY,
    seq                 bigint      GENERATED ALWAYS AS IDENTITY UNIQUE,
    thread_id           uuid        NOT NULL REFERENCES run_threads (thread_id) ON DELETE CASCADE,
    state               text        NOT NULL CHECK (state IN ('queued','executing','paused','ended','cancelled','interrupted')),
    end_kind            text        CHECK (end_kind IN ('completed','control','failed','run_limit')),
    started_by_kind     text        NOT NULL CHECK (started_by_kind IN ('caller','schedule','intake','chat')),
    started_by_id       text        NOT NULL CHECK (char_length(started_by_id) BETWEEN 1 AND 512),
    started_by_asker    text,
    scope_id            uuid        NOT NULL,
    command             text,
    request             jsonb       NOT NULL,
    gathered_context    jsonb       NOT NULL DEFAULT '{}'::jsonb,
    firing_due          timestamptz,
    result              text,
    outcome             jsonb,
    decision            jsonb,
    session_record      bytea,
    stretch_summaries   jsonb       NOT NULL DEFAULT '[]'::jsonb,
    lease_holder        text,
    lease_expires_at    timestamptz,
    cancel_requested_at timestamptz,
    queued_at           timestamptz NOT NULL DEFAULT now(),
    created_at          timestamptz NOT NULL DEFAULT now(),
    started_at          timestamptz,
    ended_at            timestamptz,
    CONSTRAINT run_records_asker_with_chat CHECK ((started_by_kind = 'chat') = (started_by_asker IS NOT NULL)),
    CONSTRAINT run_records_lease_with_executing CHECK ((state = 'executing') = (lease_expires_at IS NOT NULL)),
    CONSTRAINT run_records_end_kind_with_ended CHECK ((state = 'ended') = (end_kind IS NOT NULL)),
    CONSTRAINT run_records_ended_at CHECK ((state IN ('ended','cancelled','interrupted')) = (ended_at IS NOT NULL)),
    CONSTRAINT run_records_firing_schedule CHECK (firing_due IS NULL OR started_by_kind = 'schedule')
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
    run_id  uuid        NOT NULL REFERENCES run_records (id) ON DELETE CASCADE,
    seq     bigint      NOT NULL CHECK (seq >= 1),
    kind    text        NOT NULL CHECK (kind IN ('run_state_changed','run_progress')),
    data    jsonb       NOT NULL,
    at      timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (run_id, seq)
);
