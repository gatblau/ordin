-- journal v1, PostgreSQL (JournalAndArtefactStore.md, Data Model). Released text: never edit (DatabaseMigrations.md rule 10).
CREATE TABLE journal_events (
    seq     bigint      PRIMARY KEY,
    at      timestamptz NOT NULL,
    kind    text        NOT NULL CHECK (char_length(kind) BETWEEN 1 AND 64),
    run_id  uuid,
    data    jsonb       NOT NULL CHECK (jsonb_typeof(data) = 'object' AND octet_length(data::text) <= 262144)
);
CREATE INDEX journal_events_at  ON journal_events (at);
CREATE INDEX journal_events_run ON journal_events (run_id, seq) WHERE run_id IS NOT NULL;

CREATE TABLE journal_meta (
    id                integer     PRIMARY KEY CHECK (id = 1),
    backend_id        uuid        NOT NULL,
    last_seq          bigint      NOT NULL DEFAULT 0 CHECK (last_seq >= 0),
    retention_days    integer     NOT NULL DEFAULT 90 CHECK (retention_days >= 1),
    company_max_days  integer     CHECK (company_max_days IS NULL OR company_max_days >= 1),
    last_pruned_at    timestamptz,
    created_at        timestamptz NOT NULL
);

CREATE TABLE journal_entries (
    id            uuid        PRIMARY KEY,
    kind          text        NOT NULL CHECK (char_length(kind) BETWEEN 1 AND 64),
    component     text        NOT NULL CHECK (char_length(component) BETWEEN 1 AND 40),
    run_id        uuid,
    crew_run_id   uuid,
    workspace_id  uuid,
    summary       text        NOT NULL CHECK (char_length(summary) BETWEEN 1 AND 300),
    data          jsonb       NOT NULL CHECK (octet_length(data::text) <= 65536),
    recorded_at   timestamptz NOT NULL
);
CREATE INDEX journal_entries_recorded ON journal_entries (recorded_at DESC, id DESC);
CREATE INDEX journal_entries_run      ON journal_entries (run_id, recorded_at) WHERE run_id IS NOT NULL;

CREATE TABLE journal_runs (
    run_id         uuid        PRIMARY KEY,
    first_seen_at  timestamptz NOT NULL,
    state          text        NOT NULL CHECK (state IN ('queued','running','paused','completed','failed',
                        'stopped_at_limit','cancelled','interrupted','abandoned','no_reply')),
    ended_at       timestamptz
);
CREATE INDEX journal_runs_first_seen ON journal_runs (first_seen_at DESC);
CREATE INDEX journal_runs_open       ON journal_runs (run_id) WHERE ended_at IS NULL;

CREATE TABLE artefacts (
    id                uuid        PRIMARY KEY,
    location          jsonb       NOT NULL,
    location_key      text        NOT NULL,
    path              text        NOT NULL CHECK (octet_length(path) BETWEEN 1 AND 4096),
    version           integer     NOT NULL CHECK (version >= 1),
    hash              text        NOT NULL CHECK (hash ~ '^[0-9a-f]{64}$'),
    producing_run_id  uuid,
    media_type        text        NOT NULL CHECK (char_length(media_type) BETWEEN 3 AND 255),
    created_at        timestamptz NOT NULL,
    UNIQUE (location_key, path, version)
);
CREATE INDEX artefacts_created ON artefacts (created_at DESC, id DESC);
CREATE INDEX artefacts_run     ON artefacts (producing_run_id) WHERE producing_run_id IS NOT NULL;

CREATE TABLE tool_requests (
    id                uuid        PRIMARY KEY,
    run_id            uuid        NOT NULL,
    tool              text        NOT NULL CHECK (char_length(tool) BETWEEN 1 AND 200),
    arguments_digest  text        NOT NULL CHECK (arguments_digest ~ '^[0-9a-f]{64}$'),
    decision          text        CHECK (decision IS NULL OR decision IN ('allow','deny','ask')),
    deciding_rule     text        CHECK (deciding_rule IS NULL OR char_length(deciding_rule) BETWEEN 1 AND 128),
    approval_id       uuid,
    state             text        NOT NULL CHECK (state IN ('requested','decided','executed','refused')),
    requested_at      timestamptz NOT NULL,
    CHECK ((state = 'requested') = (decision IS NULL)),
    CHECK (approval_id IS NULL OR decision = 'ask')
);
CREATE INDEX tool_requests_run ON tool_requests (run_id, requested_at);
CREATE INDEX tool_requests_at  ON tool_requests (requested_at);

CREATE TABLE external_actions (
    action_key    uuid        PRIMARY KEY,
    run_id        uuid,
    kind          text        NOT NULL CHECK (kind IN ('slack_post','connection_write','wrightrun_run_start',
                       'email_send','origin_push','copy_publish','telegram_post',
                       'pull_request_opened','handover_written')),
    destination   text        NOT NULL CHECK (char_length(destination) BETWEEN 1 AND 2048),
    state         text        NOT NULL CHECK (state IN ('sent','confirmed','unknown_outcome')),
    sent_at       timestamptz NOT NULL,
    confirmed_at  timestamptz,
    CHECK ((state = 'confirmed') = (confirmed_at IS NOT NULL))
);
CREATE INDEX external_actions_run   ON external_actions (run_id);
CREATE INDEX external_actions_state ON external_actions (state, sent_at DESC);
CREATE INDEX external_actions_kind  ON external_actions (kind, state, sent_at);

CREATE TABLE startup_reports (
    id                         uuid        PRIMARY KEY,
    subject                    jsonb       NOT NULL,
    subject_key                text        NOT NULL UNIQUE,
    enforced_rules             jsonb       NOT NULL,
    rules_not_enforced         jsonb       NOT NULL,
    unavailable_parts          jsonb       NOT NULL,
    available_tools            jsonb       NOT NULL,
    adapter_verbs_not_offered  jsonb       NOT NULL,
    generated_at               timestamptz NOT NULL
);
CREATE INDEX startup_reports_at ON startup_reports (generated_at);

CREATE TABLE reports (
    id            uuid        PRIMARY KEY,
    workspace_id  uuid        NOT NULL,
    run_id        uuid        NOT NULL UNIQUE,
    title         text        NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
    as_of         timestamptz NOT NULL,
    parts         jsonb       NOT NULL CHECK (octet_length(parts::text) <= 2000000),
    recorded_at   timestamptz NOT NULL
);
CREATE INDEX reports_workspace ON reports (workspace_id, as_of DESC, id DESC);
CREATE INDEX reports_recorded  ON reports (recorded_at);

CREATE TABLE dashboards (
    id            uuid        PRIMARY KEY,
    workspace_id  uuid        NOT NULL,
    name          text        NOT NULL CHECK (char_length(name) BETWEEN 1 AND 80),
    made_by       jsonb       NOT NULL,
    inputs        jsonb       NOT NULL,
    refresh_runs  jsonb       NOT NULL,
    kept_at       timestamptz NOT NULL,
    UNIQUE (workspace_id, name)
);

CREATE TABLE run_steps (
    id            uuid    PRIMARY KEY,
    run_id        uuid    NOT NULL,
    plan_version  integer NOT NULL CHECK (plan_version BETWEEN 1 AND 20),
    position      integer NOT NULL CHECK (position BETWEEN 1 AND 7),
    line          text    NOT NULL CHECK (char_length(line) BETWEEN 1 AND 80),
    came_from     text    NOT NULL CHECK (came_from IN ('workflow','crew_plan','agents_plan',
                       'harness_todo_list','added_by_ordin')),
    mark          text    NOT NULL CHECK (mark IN ('next','now','done','skipped')),
    marks_set     jsonb   NOT NULL,
    events_seen   jsonb   NOT NULL,
    UNIQUE (run_id, plan_version, position)
);
CREATE INDEX run_steps_now ON run_steps (run_id, plan_version) WHERE mark = 'now';
