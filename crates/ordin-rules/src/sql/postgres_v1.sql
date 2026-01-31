-- rules v1, PostgreSQL, scaled shape (RulesEngine.md, Data Model). Released text: never edit.
CREATE TABLE rules_control_starts (kind text NOT NULL CHECK (kind IN ('command','scope','asker')), key text NOT NULL,
  period_seconds integer NOT NULL, window_start timestamptz NOT NULL, starts integer NOT NULL CHECK (starts >= 1),
  PRIMARY KEY (kind, key, period_seconds, window_start));
CREATE TABLE rules_control_messages (scope text NOT NULL, asker text NOT NULL, digest text NOT NULL CHECK (digest ~ '^[0-9a-f]{64}$'),
  seen_at timestamptz NOT NULL, repeated boolean NOT NULL DEFAULT false, PRIMARY KEY (scope, asker, digest));
CREATE TABLE rules_control_locks (key text PRIMARY KEY, holder text NOT NULL, expires_at timestamptz NOT NULL);
CREATE TABLE rules_intake_places (intake text NOT NULL, slot integer NOT NULL CHECK (slot >= 1), run_id text, taken_at timestamptz,
  PRIMARY KEY (intake, slot));
CREATE TABLE rules_run_facts (run_id uuid PRIMARY KEY, agent_instance_id uuid NOT NULL, facts jsonb NOT NULL, updated_at timestamptz NOT NULL);
CREATE TABLE rules_approved_calls (run_id uuid NOT NULL, tool text NOT NULL, arguments_digest text NOT NULL, approval_id uuid NOT NULL,
  used boolean NOT NULL DEFAULT false, created_at timestamptz NOT NULL, PRIMARY KEY (run_id, tool, arguments_digest, approval_id));
