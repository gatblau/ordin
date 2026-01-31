-- journal v1, SQLite (JournalAndArtefactStore.md, Data Model). Released text: never edit (DatabaseMigrations.md rule 10).
CREATE TABLE journal_meta (
    id                    INTEGER PRIMARY KEY CHECK (id = 1),
    backend_id            TEXT    NOT NULL CHECK (length(backend_id) = 36),
    last_seq              INTEGER NOT NULL DEFAULT 0 CHECK (last_seq >= 0),
    retention_days        INTEGER NOT NULL DEFAULT 90 CHECK (retention_days >= 1),
    company_max_days      INTEGER CHECK (company_max_days IS NULL OR company_max_days >= 1),
    last_pruned_at        TEXT,
    created_at            TEXT    NOT NULL
);

CREATE TABLE journal_events (
    seq     INTEGER PRIMARY KEY,            -- assigned by the Journal, never AUTOINCREMENT-reused
    at      TEXT    NOT NULL,
    kind    TEXT    NOT NULL CHECK (length(kind) BETWEEN 1 AND 64),
    run_id  TEXT    CHECK (run_id IS NULL OR length(run_id) = 36),
    data    TEXT    NOT NULL CHECK (json_valid(data) AND length(data) <= 262144)
);
CREATE INDEX journal_events_at  ON journal_events (at);
CREATE INDEX journal_events_run ON journal_events (run_id, seq) WHERE run_id IS NOT NULL;

CREATE TABLE journal_entries (
    id           TEXT PRIMARY KEY CHECK (length(id) = 36),
    kind         TEXT NOT NULL CHECK (length(kind) BETWEEN 1 AND 64),
    component    TEXT NOT NULL CHECK (length(component) BETWEEN 1 AND 40),
    run_id       TEXT,
    crew_run_id  TEXT,
    workspace_id TEXT,
    summary      TEXT NOT NULL CHECK (length(summary) BETWEEN 1 AND 300),
    data         TEXT NOT NULL CHECK (json_valid(data) AND length(data) <= 65536),
    recorded_at  TEXT NOT NULL
);
CREATE INDEX journal_entries_recorded ON journal_entries (recorded_at DESC, id DESC);
CREATE INDEX journal_entries_run      ON journal_entries (run_id, recorded_at) WHERE run_id IS NOT NULL;

CREATE TABLE journal_runs (
    run_id         TEXT PRIMARY KEY CHECK (length(run_id) = 36),
    first_seen_at  TEXT NOT NULL,
    state          TEXT NOT NULL CHECK (state IN ('queued','running','paused','completed','failed',
                        'stopped_at_limit','cancelled','interrupted','abandoned','no_reply')),
    ended_at       TEXT
);
CREATE INDEX journal_runs_first_seen ON journal_runs (first_seen_at DESC);
CREATE INDEX journal_runs_open       ON journal_runs (run_id) WHERE ended_at IS NULL;

CREATE TABLE artefacts (
    id                TEXT    PRIMARY KEY CHECK (length(id) = 36),
    location          TEXT    NOT NULL CHECK (json_valid(location)),
    location_key      TEXT    NOT NULL,      -- 'agent_folder:<uuid>' or 'place:<uuid>'; written by the Journal
    path              TEXT    NOT NULL CHECK (length(path) BETWEEN 1 AND 4096),
    version           INTEGER NOT NULL CHECK (version >= 1),
    hash              TEXT    NOT NULL CHECK (length(hash) = 64),
    producing_run_id  TEXT,
    media_type        TEXT    NOT NULL CHECK (length(media_type) BETWEEN 3 AND 255),
    created_at        TEXT    NOT NULL,
    UNIQUE (location_key, path, version)
);
CREATE INDEX artefacts_created ON artefacts (created_at DESC, id DESC);
CREATE INDEX artefacts_run     ON artefacts (producing_run_id) WHERE producing_run_id IS NOT NULL;

CREATE TABLE tool_requests (
    id                TEXT PRIMARY KEY CHECK (length(id) = 36),
    run_id            TEXT NOT NULL,
    tool              TEXT NOT NULL CHECK (length(tool) BETWEEN 1 AND 200),
    arguments_digest  TEXT NOT NULL CHECK (length(arguments_digest) = 64),
    decision          TEXT CHECK (decision IS NULL OR decision IN ('allow','deny','ask')),
    deciding_rule     TEXT CHECK (deciding_rule IS NULL OR length(deciding_rule) BETWEEN 1 AND 128),
    approval_id       TEXT,
    state             TEXT NOT NULL CHECK (state IN ('requested','decided','executed','refused')),
    requested_at      TEXT NOT NULL,
    CHECK ((state = 'requested') = (decision IS NULL)),
    CHECK (approval_id IS NULL OR decision = 'ask')
);
CREATE INDEX tool_requests_run ON tool_requests (run_id, requested_at);
CREATE INDEX tool_requests_at  ON tool_requests (requested_at);

CREATE TABLE external_actions (
    action_key    TEXT PRIMARY KEY CHECK (length(action_key) = 36),
    run_id        TEXT CHECK (run_id IS NULL OR length(run_id) = 36),   -- NULL: no run (2C C-7)
    kind          TEXT NOT NULL CHECK (kind IN ('slack_post','connection_write','wrightrun_run_start',
                       'email_send','origin_push','copy_publish','telegram_post',
                       'pull_request_opened','handover_written')),
    destination   TEXT NOT NULL CHECK (length(destination) BETWEEN 1 AND 2048),
    state         TEXT NOT NULL CHECK (state IN ('sent','confirmed','unknown_outcome')),
    sent_at       TEXT NOT NULL,
    confirmed_at  TEXT,
    CHECK ((state = 'confirmed') = (confirmed_at IS NOT NULL))
);
CREATE INDEX external_actions_run   ON external_actions (run_id);
CREATE INDEX external_actions_state ON external_actions (state, sent_at DESC);
CREATE INDEX external_actions_kind  ON external_actions (kind, state, sent_at);

CREATE TABLE startup_reports (
    id                         TEXT PRIMARY KEY CHECK (length(id) = 36),
    subject                    TEXT NOT NULL CHECK (json_valid(subject)),
    subject_key                TEXT NOT NULL UNIQUE,
    enforced_rules             TEXT NOT NULL CHECK (json_valid(enforced_rules)),
    rules_not_enforced         TEXT NOT NULL CHECK (json_valid(rules_not_enforced)),
    unavailable_parts          TEXT NOT NULL CHECK (json_valid(unavailable_parts)),
    available_tools            TEXT NOT NULL CHECK (json_valid(available_tools)),
    adapter_verbs_not_offered  TEXT NOT NULL CHECK (json_valid(adapter_verbs_not_offered)),
    generated_at               TEXT NOT NULL
);
CREATE INDEX startup_reports_at ON startup_reports (generated_at);

CREATE TABLE reports (
    id            TEXT PRIMARY KEY CHECK (length(id) = 36),
    workspace_id  TEXT NOT NULL,
    run_id        TEXT NOT NULL UNIQUE,
    title         TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
    as_of         TEXT NOT NULL,
    parts         TEXT NOT NULL CHECK (json_valid(parts) AND length(parts) <= 2000000),
    recorded_at   TEXT NOT NULL
);
CREATE INDEX reports_workspace ON reports (workspace_id, as_of DESC, id DESC);
CREATE INDEX reports_recorded  ON reports (recorded_at);

CREATE TABLE dashboards (
    id            TEXT PRIMARY KEY CHECK (length(id) = 36),
    workspace_id  TEXT NOT NULL,
    name          TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    made_by       TEXT NOT NULL CHECK (json_valid(made_by)),
    inputs        TEXT NOT NULL CHECK (json_valid(inputs)),
    refresh_runs  TEXT NOT NULL CHECK (json_valid(refresh_runs)),
    kept_at       TEXT NOT NULL,
    UNIQUE (workspace_id, name)
);

CREATE TABLE run_steps (
    id            TEXT    PRIMARY KEY CHECK (length(id) = 36),
    run_id        TEXT    NOT NULL,
    plan_version  INTEGER NOT NULL CHECK (plan_version BETWEEN 1 AND 20),
    position      INTEGER NOT NULL CHECK (position BETWEEN 1 AND 7),
    line          TEXT    NOT NULL CHECK (length(line) BETWEEN 1 AND 80),
    came_from     TEXT    NOT NULL CHECK (came_from IN ('workflow','crew_plan','agents_plan',
                       'harness_todo_list','added_by_ordin')),
    mark          TEXT    NOT NULL CHECK (mark IN ('next','now','done','skipped')),
    marks_set     TEXT    NOT NULL CHECK (json_valid(marks_set)),
    events_seen   TEXT    NOT NULL CHECK (json_valid(events_seen)),
    UNIQUE (run_id, plan_version, position)
);
CREATE INDEX run_steps_now ON run_steps (run_id, plan_version) WHERE mark = 'now';
