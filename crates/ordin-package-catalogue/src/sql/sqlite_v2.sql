-- package_catalogue migration 2 (PackageCatalogue.md, amendment January 2026, PKG-DOC-2): additive.
-- No backfill: a workspace from an earlier version has no row and is offered the starter agents once.
-- spec: ordin-desktop/components/PackageCatalogue#PKG-DOC-2
CREATE TABLE starter_agent_offers (   -- each starter agent is offered to a workspace once
  workspace_id     TEXT NOT NULL,     -- UUID text, as workspace ids elsewhere in this database
  agent_identifier TEXT NOT NULL,     -- a STARTER_AGENTS identifier
  offered_at       TEXT NOT NULL,     -- RFC 3339 ms UTC
  PRIMARY KEY (workspace_id, agent_identifier)
);
