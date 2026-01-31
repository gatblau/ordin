-- A fact's content as one subject–predicate–object triple, and the conflicts between facts (ADR-022).
--
-- The triple stays in `content`; the store derives these fields from it at `put_fact` so that a write
-- finds the live facts with the same subject and predicate by one indexed lookup. `fact_conflict` holds
-- one row per pair of live facts that disagree; it goes when either fact ends or is erased.
--
-- Additive: optional fields, one index on `fact`, one table with two indexes. No row is changed and no
-- backfill runs: facts written before this keep their content and have no triple fields (SA-TRIPLE-12).
DEFINE FIELD IF NOT EXISTS subject_entity ON fact TYPE option<string>;
DEFINE FIELD IF NOT EXISTS predicate      ON fact TYPE option<string>;
DEFINE FIELD IF NOT EXISTS object_kind    ON fact TYPE option<string>
    ASSERT $value IS NONE OR $value IN ['entity','text','number','money','date','boolean'];
DEFINE FIELD IF NOT EXISTS object_value   ON fact TYPE option<string>;
DEFINE FIELD IF NOT EXISTS object_unit    ON fact TYPE option<string>;
DEFINE FIELD IF NOT EXISTS object_entity  ON fact TYPE option<string>;
DEFINE FIELD IF NOT EXISTS single_valued  ON fact TYPE option<bool>;
DEFINE FIELD IF NOT EXISTS changed        ON fact TYPE option<bool>;
DEFINE FIELD IF NOT EXISTS author         ON fact TYPE option<string>;
DEFINE INDEX IF NOT EXISTS fact_triple ON fact FIELDS owner.team, subject_entity, predicate, valid_to;
DEFINE TABLE IF NOT EXISTS fact_conflict SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS fact_a       ON fact_conflict TYPE string;
DEFINE FIELD IF NOT EXISTS fact_b       ON fact_conflict TYPE string;
DEFINE FIELD IF NOT EXISTS tenant       ON fact_conflict TYPE string;
DEFINE FIELD IF NOT EXISTS owner        ON fact_conflict TYPE object;
DEFINE FIELD IF NOT EXISTS owner.tenant ON fact_conflict TYPE string;
DEFINE FIELD IF NOT EXISTS owner.team   ON fact_conflict TYPE option<string>;
DEFINE FIELD IF NOT EXISTS owner.user   ON fact_conflict TYPE string;
DEFINE FIELD IF NOT EXISTS recorded_at  ON fact_conflict TYPE datetime;
DEFINE INDEX IF NOT EXISTS fact_conflict_key ON fact_conflict FIELDS fact_a, fact_b UNIQUE;
DEFINE INDEX IF NOT EXISTS fact_conflict_b   ON fact_conflict FIELDS fact_b;
