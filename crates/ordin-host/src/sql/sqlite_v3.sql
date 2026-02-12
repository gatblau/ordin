-- OrdinHost.md OH-MENU-1: the menu item a person's message came from (ServiceApi.md SVC-MENU-1).
-- spec: ordin-desktop/components/OrdinHost#OH-MENU-1
ALTER TABLE ordin_messages ADD COLUMN from_menu TEXT
  CHECK (from_menu IS NULL OR from_menu IN ('ask_about','add_to_memory','add_to_knowledge'));

-- OrdinHost.md OH-MENU-2: a document added to memory from the menu, by the hash of what was read.
-- spec: ordin-desktop/components/OrdinHost#OH-MENU-2
CREATE TABLE ordin_menu_reads (
  workspace_id  TEXT NOT NULL,
  action        TEXT NOT NULL CHECK (action IN ('add_to_memory')),
  place_id      TEXT NOT NULL,
  relative_path TEXT NOT NULL CHECK (length(relative_path) BETWEEN 1 AND 4096),
  content_hash  TEXT NOT NULL CHECK (length(content_hash) = 64),
  run_id        TEXT NOT NULL,
  read_at       TEXT NOT NULL,
  PRIMARY KEY (workspace_id, action, place_id, relative_path)
);
