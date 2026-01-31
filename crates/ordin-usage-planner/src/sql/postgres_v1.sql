-- usage_planner v1, PostgreSQL (UsagePlanner.md, Data Model; scaled shape). Released text: never edit (DatabaseMigrations.md rule 10).
CREATE TABLE usage_daily (day date PRIMARY KEY, model_turns bigint NOT NULL DEFAULT 0, tool_calls bigint NOT NULL DEFAULT 0,
  turns_on_smaller bigint NOT NULL DEFAULT 0, answered_without_model bigint NOT NULL DEFAULT 0, tokens bigint, tokens_from_cache bigint);
CREATE TABLE usage_work_history (work_key text NOT NULL, run_id uuid NOT NULL, model_turns integer NOT NULL CHECK (model_turns >= 0),
  finished_at timestamptz NOT NULL, PRIMARY KEY (work_key, run_id));
