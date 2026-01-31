-- spec: ordin-desktop/components/ProjectService#SA-RECON-01
CREATE TABLE projects (
    id                       TEXT PRIMARY KEY,
    place_id                 TEXT NOT NULL,
    workspace_id             TEXT NOT NULL,
    display_name             TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 120),
    is_git_repository        INTEGER NOT NULL CHECK (is_git_repository IN (0,1)),
    current_branch           TEXT,
    language_servers_in_use  TEXT NOT NULL DEFAULT '[]',
    check_commands           TEXT NOT NULL DEFAULT '[]',
    state                    TEXT NOT NULL CHECK (state IN ('opened','indexed','closed')),
    opened_at                TEXT NOT NULL,
    CHECK ((is_git_repository = 1) = (current_branch IS NOT NULL))
);
CREATE UNIQUE INDEX projects_open_place ON projects (place_id) WHERE state != 'closed';

CREATE TABLE github_operation_settings (
    id                 TEXT PRIMARY KEY,
    project_id         TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    operation_kind     TEXT NOT NULL,
    is_write           INTEGER NOT NULL,
    approval_required  INTEGER NOT NULL,
    set_by             TEXT NOT NULL CHECK (set_by IN ('organisation_profile','engineer')),
    set_at             TEXT NOT NULL,
    UNIQUE (project_id, operation_kind)
);

CREATE TABLE language_plugins (
    id                TEXT PRIMARY KEY,
    language          TEXT NOT NULL UNIQUE CHECK (length(language) BETWEEN 1 AND 40),
    approved_servers  TEXT NOT NULL,
    default_server    TEXT NOT NULL,
    chosen_server     TEXT,
    pinned_version    TEXT,
    source            TEXT NOT NULL CHECK (source IN ('agent_package','organisation_profile')),
    enabled           INTEGER NOT NULL,
    server_settings   TEXT NOT NULL DEFAULT '{}' CHECK (length(server_settings) <= 65536),
    status            TEXT NOT NULL CHECK (status IN ('available','added','installed','removed'))
);

CREATE TABLE coding_tasks (
    id                 TEXT PRIMARY KEY,
    origin             TEXT NOT NULL,                     -- JSON RunOrigin
    project_id         TEXT NOT NULL REFERENCES projects(id),
    agent_instance_id  TEXT NOT NULL,
    request            TEXT NOT NULL CHECK (length(request) BETWEEN 1 AND 4000),
    worktree_path      TEXT NOT NULL,
    worktree_place_id  TEXT NOT NULL,
    branch             TEXT NOT NULL,
    base_branch        TEXT NOT NULL,
    base_commit        TEXT NOT NULL CHECK (length(base_commit) = 40),
    attempt_group_id   TEXT,
    test_outcome       TEXT CHECK (test_outcome IN ('passed','failed')),
    new_problems       INTEGER NOT NULL DEFAULT 0,
    new_problem_list   TEXT NOT NULL DEFAULT '[]',        -- JSON Vec<Problem>
    state              TEXT NOT NULL CHECK (state IN ('working','ready_for_review','settled','set_aside'))
);
CREATE INDEX coding_tasks_group ON coding_tasks (attempt_group_id);

CREATE TABLE change_sets (
    id              TEXT PRIMARY KEY,
    coding_task_id  TEXT NOT NULL UNIQUE REFERENCES coding_tasks(id),
    run_id          TEXT NOT NULL,
    project_id      TEXT NOT NULL,
    state           TEXT NOT NULL CHECK (state IN ('open','in_review','settled')),
    created_at      TEXT NOT NULL
);
CREATE TABLE change_set_entries (
    id             TEXT PRIMARY KEY,
    change_set_id  TEXT NOT NULL REFERENCES change_sets(id) ON DELETE CASCADE,
    position       INTEGER NOT NULL,
    file           TEXT NOT NULL,
    hunk           TEXT NOT NULL CHECK (length(hunk) <= 1048576),
    state          TEXT NOT NULL CHECK (state IN ('pending','accepted','rejected'))
);

CREATE TABLE followed_originals (
    place_id     TEXT NOT NULL,
    path         TEXT NOT NULL,
    branch       TEXT NOT NULL,
    last_blob    TEXT,
    last_commit  TEXT,
    PRIMARY KEY (place_id, path, branch)
);
CREATE TABLE repository_fetch_state (
    place_id         TEXT NOT NULL,
    branch           TEXT NOT NULL,
    remote_commit    TEXT,
    last_checked_at  TEXT,
    last_error       TEXT,
    interval_s       INTEGER NOT NULL DEFAULT 900,
    PRIMARY KEY (place_id, branch)
);
CREATE TABLE ide_choices (
    ide_id   TEXT PRIMARY KEY,
    enabled  INTEGER NOT NULL
);
