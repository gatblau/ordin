-- team_knowledge step 3, SQLite (TeamKnowledgeService.md amendment ADR-112, Data Model): applied as written; its table stays empty in the first build (ADR-121 item 9).
ALTER TABLE team_knowledge_read_folders ADD COLUMN bundle_kind TEXT NOT NULL DEFAULT 'team'
    CHECK (bundle_kind IN ('team','okf'));
CREATE TABLE team_knowledge_bundle_files (           -- index.md and log.md of the own folder
    folder_id      TEXT NOT NULL,                     -- the own folder (workspace id value, SA-TEAMR-01)
    path           TEXT NOT NULL CHECK (path IN ('index.md','log.md')),
    blob           TEXT NOT NULL CHECK (length(blob) = 40),
    synced_at      TEXT NOT NULL,
    PRIMARY KEY (folder_id, path)
);
