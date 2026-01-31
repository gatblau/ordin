ALTER TABLE memory_fact_refs ADD COLUMN shared_by VARCHAR(200);
ALTER TABLE memory_fact_refs ADD COLUMN shared_at TIMESTAMPTZ;
ALTER TABLE memory_fact_refs ADD COLUMN first_exported_at TIMESTAMPTZ;
ALTER TABLE memory_fact_refs ADD COLUMN team_imported BOOLEAN NOT NULL DEFAULT FALSE;
