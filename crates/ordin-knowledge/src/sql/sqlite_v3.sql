-- spec: ordin-desktop/components/KnowledgeService#SA-KTEAM-01
-- 3 team knowledge columns (amendment January 2026, ADR-103)
ALTER TABLE knowledge_pages ADD COLUMN shared_by TEXT CHECK (shared_by IS NULL OR length(shared_by) BETWEEN 1 AND 200);
ALTER TABLE knowledge_pages ADD COLUMN shared_at TEXT;
ALTER TABLE knowledge_pages ADD COLUMN first_exported_at TEXT;
