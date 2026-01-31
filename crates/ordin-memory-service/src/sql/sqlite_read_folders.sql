ALTER TABLE memory_fact_refs ADD COLUMN read_folder_id TEXT;
CREATE INDEX memory_fact_refs_read_folder ON memory_fact_refs (workspace_id, read_folder_id) WHERE read_folder_id IS NOT NULL;
