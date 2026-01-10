-- Remove the lineage index. `hard_delete`'s derived-collection query still returns correct rows; it
-- reverts to scanning the `fact` table to find them.
REMOVE INDEX IF EXISTS fact_derived ON fact;
