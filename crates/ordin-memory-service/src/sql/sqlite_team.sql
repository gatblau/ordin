ALTER TABLE memory_fact_refs ADD COLUMN shared_by TEXT CHECK (shared_by IS NULL OR length(shared_by) BETWEEN 1 AND 200);
ALTER TABLE memory_fact_refs ADD COLUMN shared_at TEXT;
ALTER TABLE memory_fact_refs ADD COLUMN first_exported_at TEXT;
ALTER TABLE memory_fact_refs ADD COLUMN team_imported INTEGER NOT NULL DEFAULT 0 CHECK (team_imported IN (0,1));
