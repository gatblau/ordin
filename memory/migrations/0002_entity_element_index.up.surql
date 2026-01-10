-- 0002 — Correct the graph-leg membership index so it can actually serve array-element matching.
--
-- `fact_ent` was defined as `FIELDS entities`, which indexes the array as a whole value. SurrealDB
-- 3.1.5 cannot use such an index for `$entity IN entities`: EXPLAIN showed the graph leg falling back
-- to `fact_valid` (an index on `valid_to` matching every live fact), and an explicit
-- `WITH INDEX fact_ent` hint degraded further to a full table scan — the index could not serve the
-- predicate at all. `FIELDS entities.*` indexes each element, which the planner does use.
--
-- Rollback: 0002.down restores the previous definition. Both directions only drop and recreate an
-- index; no fact data is read, written or lost, and the graph leg returns identical rows either way
-- (the index changes how they are found, not which ones qualify). Re-indexing cost is proportional to
-- the number of facts in the namespace.
REMOVE INDEX IF EXISTS fact_ent ON fact;
DEFINE INDEX IF NOT EXISTS fact_ent ON fact FIELDS entities.*;
