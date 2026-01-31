CREATE TABLE agent_repositories (
  id TEXT PRIMARY KEY,                       -- UUIDv7
  display_name TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 120),
  host TEXT NOT NULL,
  repository TEXT NOT NULL,
  protected_branch TEXT NOT NULL,
  format TEXT NOT NULL CHECK (format IN ('ordin','apm')),
  format_version TEXT NOT NULL,
  source TEXT NOT NULL CHECK (source IN ('profile','person')),
  signed_merges_required INTEGER,            -- NULL iff source = 'person'
  person_can_publish INTEGER NOT NULL,
  person_can_curate INTEGER NOT NULL,
  last_fetched_commit TEXT,
  profile_key TEXT,                          -- reconciliation: profile rows only; agent_catalogue.repositories[].key
  clone_path TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('named','cloned','dropped','removed')),
  added_at TEXT NOT NULL,
  ordin_version TEXT,                        -- bundled row only
  CHECK ((source = 'profile') = (signed_merges_required IS NOT NULL))
);
CREATE UNIQUE INDEX ux_agent_repositories_host_repo ON agent_repositories(host, repository)
  WHERE state IN ('named','cloned');

CREATE TABLE parts (
  id TEXT PRIMARY KEY,
  agent_repository_id TEXT NOT NULL REFERENCES agent_repositories(id) ON DELETE CASCADE,
  kind TEXT NOT NULL, identifier TEXT NOT NULL, name TEXT NOT NULL, version TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('draft','approved','deprecated')),
  supersedes TEXT, owner TEXT NOT NULL, file_path TEXT NOT NULL, "commit" TEXT NOT NULL,
  content_digest TEXT NOT NULL, vocabulary_terms_cited TEXT NOT NULL DEFAULT '[]',
  withdrawn INTEGER NOT NULL DEFAULT 0,
  UNIQUE (agent_repository_id, kind, identifier, version)
);
CREATE INDEX ix_parts_identifier ON parts(identifier);

CREATE TABLE part_dependencies (
  part_id TEXT NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
  uses_part_id TEXT NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
  PRIMARY KEY (part_id, uses_part_id)
);
CREATE INDEX ix_part_dependencies_uses ON part_dependencies(uses_part_id);

CREATE TABLE part_index (                    -- chosen here: index data beyond the 2C Part
  part_id TEXT PRIMARY KEY REFERENCES parts(id) ON DELETE CASCADE,
  at_head INTEGER NOT NULL, offered INTEGER NOT NULL, not_offered_reason TEXT,
  summary TEXT NOT NULL, area TEXT, audience TEXT NOT NULL DEFAULT '[]', publisher TEXT NOT NULL,
  files TEXT NOT NULL DEFAULT '[]'           -- reconciliation: JSON repository paths of the part's files (§L4)
);
CREATE VIRTUAL TABLE part_search USING fts5(name, summary, body, part_id UNINDEXED, tokenize = 'unicode61 remove_diacritics 2');

CREATE TABLE withdrawn_list_entries (
  id TEXT PRIMARY KEY,
  agent_repository_id TEXT NOT NULL REFERENCES agent_repositories(id) ON DELETE CASCADE,
  part_kind TEXT NOT NULL, part_identifier TEXT NOT NULL, part_version TEXT, part_content_digest TEXT,
  reason TEXT NOT NULL CHECK (length(reason) BETWEEN 1 AND 1000),
  withdrawn_at TEXT NOT NULL, replacement_identifier TEXT, replacement_version TEXT,
  removed_by_hand_seen_at TEXT, read_at TEXT NOT NULL,
  CHECK ((part_version IS NULL) = (part_content_digest IS NULL)),
  CHECK ((replacement_identifier IS NULL) = (replacement_version IS NULL))
);
CREATE UNIQUE INDEX ux_wdl_entry ON withdrawn_list_entries(agent_repository_id, part_kind, part_identifier, IFNULL(part_version, ''));
CREATE TRIGGER trg_wdl_append_only BEFORE UPDATE ON withdrawn_list_entries
  WHEN NEW.reason IS NOT OLD.reason OR NEW.part_identifier IS NOT OLD.part_identifier
    OR NEW.part_version IS NOT OLD.part_version OR NEW.replacement_identifier IS NOT OLD.replacement_identifier
    OR (OLD.removed_by_hand_seen_at IS NOT NULL AND NEW.removed_by_hand_seen_at IS NOT OLD.removed_by_hand_seen_at)
  BEGIN SELECT RAISE(ABORT, 'WDL_APPEND_ONLY'); END;

CREATE TABLE vocabulary_terms (
  id TEXT PRIMARY KEY,
  agent_repository_id TEXT NOT NULL REFERENCES agent_repositories(id) ON DELETE CASCADE,
  vocabulary_part_id TEXT NOT NULL REFERENCES parts(id) ON DELETE CASCADE,
  axis TEXT NOT NULL, code TEXT NOT NULL, label TEXT NOT NULL, description TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('active','retired')),
  UNIQUE (agent_repository_id, axis, code)
);

CREATE TABLE lifecycles (
  id TEXT PRIMARY KEY,
  part_id TEXT NOT NULL UNIQUE REFERENCES parts(id) ON DELETE CASCADE,
  agent_repository_id TEXT NOT NULL REFERENCES agent_repositories(id) ON DELETE CASCADE,
  identifier TEXT NOT NULL, name TEXT NOT NULL, stages TEXT NOT NULL,   -- JSON Vec<LifecycleStage>
  status TEXT NOT NULL, version TEXT NOT NULL
);
CREATE TABLE lifecycle_places (              -- chosen here: places named by lifecycle stages
  id TEXT PRIMARY KEY, place_type TEXT NOT NULL CHECK (place_type IN ('git','sharepoint')),
  address TEXT NOT NULL UNIQUE
);

CREATE TABLE repository_fetches (            -- chosen here: last outcomes and health
  agent_repository_id TEXT NOT NULL REFERENCES agent_repositories(id) ON DELETE CASCADE,
  at TEXT NOT NULL, result TEXT NOT NULL, "commit" TEXT, health TEXT,
  PRIMARY KEY (agent_repository_id, at)
);
