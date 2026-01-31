-- package_catalogue migration 1 (PackageCatalogue.md Data Model).
CREATE TABLE agent_instances (
  id TEXT PRIMARY KEY,
  display_name TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 120),
  package_name TEXT NOT NULL, package_version TEXT NOT NULL,
  agent_repository_id TEXT, "commit" TEXT,
  agent_folder_path TEXT NOT NULL UNIQUE,
  lock_file_path TEXT NOT NULL,
  audit_result TEXT CHECK (audit_result IN ('passed','failed')),
  authority TEXT NOT NULL CHECK (authority IN ('document_only','system_changing','routing_only')),
  parsed_declaration_ref TEXT NOT NULL,
  is_trial_of_own_draft INTEGER NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('installing','ready','refused','updating','update_awaiting_choice','removed')),
  updated_between_runs_at TEXT,
  installed_at TEXT NOT NULL,
  agent_identifier TEXT NOT NULL,
  agent_draft_id TEXT,
  stopped_reason TEXT,
  stopped_by TEXT CHECK (stopped_by IS NULL OR stopped_by IN ('withdrawal','trust')),
  may_do TEXT,
  plan_hash TEXT,
  pending_choices TEXT NOT NULL DEFAULT '[]',
  harness TEXT NOT NULL CHECK (harness IN ('claude_code','copilot_cli')),
  -- Every re-plan (change, answer, update, withdrawal) needs the languages and locale the
  -- instance was added with; they are kept with the instance.
  workspace_languages TEXT NOT NULL DEFAULT '[]',
  interface_locale TEXT NOT NULL DEFAULT 'en-GB',
  CHECK (("agent_repository_id" IS NULL) = ("commit" IS NULL)),
  CHECK (status <> 'refused' OR audit_result = 'failed' OR stopped_reason IS NOT NULL),
  CHECK ((stopped_by IS NULL) = (stopped_reason IS NULL))
);

CREATE TABLE agent_instance_workspaces (
  agent_instance_id TEXT NOT NULL REFERENCES agent_instances(id) ON DELETE CASCADE,
  workspace_id TEXT NOT NULL,
  added_at TEXT NOT NULL,
  PRIMARY KEY (agent_instance_id, workspace_id)
);
CREATE INDEX ix_aiw_workspace ON agent_instance_workspaces(workspace_id);

CREATE TABLE agent_instance_choices (
  agent_instance_id TEXT NOT NULL REFERENCES agent_instances(id) ON DELETE CASCADE,
  rule_index INTEGER NOT NULL CHECK (rule_index >= 0),
  packs TEXT NOT NULL,
  source TEXT NOT NULL CHECK (source IN ('person_change','agent_asked')),
  chosen_at TEXT NOT NULL,
  PRIMARY KEY (agent_instance_id, rule_index)
);

CREATE TABLE agent_pending_updates (
  agent_instance_id TEXT PRIMARY KEY REFERENCES agent_instances(id) ON DELETE CASCADE,
  to_commit TEXT NOT NULL, may_do_changes TEXT NOT NULL,
  candidate_plan_hash TEXT NOT NULL,
  found_at TEXT NOT NULL
);
