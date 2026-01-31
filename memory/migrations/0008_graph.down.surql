-- Reversal of 0008_graph.
--
-- Destructive for `relationship_fact`: removing the table drops the record of which facts asserted each
-- link, after which links no longer close when their facts end (SA-GRAPH-04) and erasure no longer
-- removes them; restoring needs the pre-migration backup or a re-write of the facts. `names_norm`
-- values stay on the SCHEMALESS entity rows but are unindexed. The held records' assertions go with
-- the quarantine fields. Forward-only in practice; this path exists for the test harness.
REMOVE FIELD IF EXISTS owner_only ON relationship;
REMOVE INDEX IF EXISTS entity_private_names ON entity;
REMOVE FIELD IF EXISTS private_names_norm ON entity;
REMOVE FIELD IF EXISTS private_aliases ON entity;
REMOVE FIELD IF EXISTS derived_from ON quarantine;
REMOVE FIELD IF EXISTS links_asserted ON quarantine;
REMOVE FIELD IF EXISTS entities_asserted ON quarantine;
REMOVE FIELD IF EXISTS names_backfilled_at ON maintenance_state;
REMOVE INDEX IF EXISTS entity_names ON entity;
REMOVE FIELD IF EXISTS names_norm ON entity;
REMOVE TABLE IF EXISTS relationship_fact;
REMOVE INDEX IF EXISTS relationship_live ON relationship;
REMOVE FIELD IF EXISTS replaces ON relationship;
