-- Restore the 0001 definition of `fact_ent`. The graph leg still returns correct rows under this
-- definition — it simply cannot use the index and falls back to scanning `fact_valid`.
REMOVE INDEX IF EXISTS fact_ent ON fact;
DEFINE INDEX IF NOT EXISTS fact_ent ON fact FIELDS entities;
