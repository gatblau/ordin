-- Reverses 0011_triples. Destructive for `fact_conflict`: the record of which facts disagree is lost and
-- both facts stay live with nothing telling the caller that they disagree; restoring needs the
-- pre-migration backup. The triple fields stay on the SCHEMALESS fact rows, unindexed; the content still
-- holds the triple. Forward-only in practice; this path exists for the test harness.
REMOVE INDEX IF EXISTS fact_conflict_b ON fact_conflict;
REMOVE INDEX IF EXISTS fact_conflict_key ON fact_conflict;
REMOVE TABLE IF EXISTS fact_conflict;
REMOVE INDEX IF EXISTS fact_triple ON fact;
REMOVE FIELD IF EXISTS author ON fact;
REMOVE FIELD IF EXISTS changed ON fact;
REMOVE FIELD IF EXISTS single_valued ON fact;
REMOVE FIELD IF EXISTS object_entity ON fact;
REMOVE FIELD IF EXISTS object_unit ON fact;
REMOVE FIELD IF EXISTS object_value ON fact;
REMOVE FIELD IF EXISTS object_kind ON fact;
REMOVE FIELD IF EXISTS predicate ON fact;
REMOVE FIELD IF EXISTS subject_entity ON fact;
