-- Reversal of 0007_fact_collection.
--
-- `fact`, `entity`, `relationship` and `source` are SCHEMALESS: the stored labels stay on the rows but
-- are no longer validated or indexed, and a build without step 9′ searches every collection together.
-- For `quarantine` (SCHEMAFULL) the stored labels of held records go with the field. Forward-only in
-- practice; this path exists for the test harness.
REMOVE INDEX IF EXISTS fact_collection ON fact;
REMOVE FIELD IF EXISTS collection ON fact;
REMOVE INDEX IF EXISTS entity_collection ON entity;
REMOVE FIELD IF EXISTS collection ON entity;
REMOVE INDEX IF EXISTS relationship_collection ON relationship;
REMOVE FIELD IF EXISTS collection ON relationship;
REMOVE INDEX IF EXISTS source_collection ON source;
REMOVE FIELD IF EXISTS collection ON source;
REMOVE FIELD IF EXISTS collection ON quarantine;
