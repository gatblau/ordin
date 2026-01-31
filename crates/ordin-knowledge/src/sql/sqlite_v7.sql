-- spec: ordin-desktop/components/KnowledgeService#KS-TR-3
-- 7 sections as passages (amendment January 2026, ADR-118). Sections indexed before this step carry
-- no class; clearing the indexed version makes the memory-index loop write every page's sections again,
-- now as passages, and the Memory Service retires the earlier ones. No page changes; no row is removed.
UPDATE knowledge_pages SET memory_indexed_version = NULL;
UPDATE knowledge_read_pages SET memory_indexed_version = NULL;
