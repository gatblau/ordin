-- spec: ordin-desktop/components/KnowledgeService#SA-OKF-06
-- 6 open knowledge format (amendment January 2026, ADR-112)
ALTER TABLE knowledge_pages ADD COLUMN page_type TEXT CHECK (page_type IS NULL OR length(page_type) BETWEEN 1 AND 64);
ALTER TABLE knowledge_pages ADD COLUMN description TEXT CHECK (description IS NULL OR length(description) BETWEEN 1 AND 2000);
ALTER TABLE knowledge_pages ADD COLUMN okf_tier TEXT NOT NULL DEFAULT 'unverified'
    CHECK (okf_tier IN ('human_reviewed','machine_confirmed','unverified'));
ALTER TABLE knowledge_pages ADD COLUMN okf_status TEXT NOT NULL DEFAULT 'stable' CHECK (okf_status IN ('draft','stable','deprecated'));
ALTER TABLE knowledge_pages ADD COLUMN status_text TEXT;
ALTER TABLE knowledge_pages ADD COLUMN stale_after TEXT;
ALTER TABLE knowledge_pages ADD COLUMN verified TEXT NOT NULL DEFAULT '[]';
ALTER TABLE knowledge_pages ADD COLUMN generated TEXT;

ALTER TABLE knowledge_read_pages ADD COLUMN page_type TEXT CHECK (page_type IS NULL OR length(page_type) BETWEEN 1 AND 64);
ALTER TABLE knowledge_read_pages ADD COLUMN description TEXT CHECK (description IS NULL OR length(description) BETWEEN 1 AND 2000);
ALTER TABLE knowledge_read_pages ADD COLUMN okf_tier TEXT NOT NULL DEFAULT 'unverified'
    CHECK (okf_tier IN ('human_reviewed','machine_confirmed','unverified'));
ALTER TABLE knowledge_read_pages ADD COLUMN okf_status TEXT NOT NULL DEFAULT 'stable' CHECK (okf_status IN ('draft','stable','deprecated'));
ALTER TABLE knowledge_read_pages ADD COLUMN status_text TEXT;
ALTER TABLE knowledge_read_pages ADD COLUMN stale_after TEXT;
ALTER TABLE knowledge_read_pages ADD COLUMN verified TEXT NOT NULL DEFAULT '[]';
ALTER TABLE knowledge_read_pages ADD COLUMN generated TEXT;
ALTER TABLE knowledge_read_pages ADD COLUMN concept_path TEXT CHECK (concept_path IS NULL OR length(CAST(concept_path AS BLOB)) <= 1024);
CREATE INDEX knowledge_read_pages_concept ON knowledge_read_pages (workspace_id, folder_id, concept_path)
    WHERE concept_path IS NOT NULL;
CREATE INDEX knowledge_pages_stale_after ON knowledge_pages (stale_after) WHERE stale_after IS NOT NULL;
