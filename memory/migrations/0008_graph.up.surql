-- Entities, links and graph recall (ADR-021, memory-store and write-pipeline amendments January 2026).
--
-- Additive: optional fields, two indexes and one table; no row is changed and nothing narrows.
-- `relationship_fact` records which facts asserted each relationship and which ended it, so a link
-- closes when no live fact asserts it any more (SA-GRAPH-04) and erasure removes it with its last fact.
-- `entity.names_norm` holds the normalised canonical name and aliases (SA-GRAPH-01), filled for rows
-- written before this migration by `backfill_entity_names` (SA-GRAPH-12).
DEFINE FIELD IF NOT EXISTS replaces ON relationship TYPE option<string>;
DEFINE INDEX IF NOT EXISTS relationship_live ON relationship FIELDS owner.team, from_ent, kind, to_ent;
DEFINE TABLE IF NOT EXISTS relationship_fact SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS relationship ON relationship_fact TYPE string;
DEFINE FIELD IF NOT EXISTS fact         ON relationship_fact TYPE string;
DEFINE FIELD IF NOT EXISTS role         ON relationship_fact TYPE string ASSERT $value IN ['asserted','ended'];
DEFINE FIELD IF NOT EXISTS at           ON relationship_fact TYPE datetime;
DEFINE INDEX IF NOT EXISTS relationship_fact_key  ON relationship_fact FIELDS relationship, fact, role UNIQUE;
DEFINE INDEX IF NOT EXISTS relationship_fact_fact ON relationship_fact FIELDS fact;
DEFINE FIELD IF NOT EXISTS names_norm ON entity TYPE option<array<string>>;
DEFINE INDEX IF NOT EXISTS entity_names ON entity FIELDS names_norm.*;
DEFINE FIELD IF NOT EXISTS names_backfilled_at ON maintenance_state TYPE option<datetime>;
-- C4's portion: a held write keeps the entities, links and summary origins it asserted, so a release
-- writes its links. `quarantine` is SCHEMAFULL, so the fields must exist before a held write carries
-- them. `any`, not `array<object>`: SCHEMAFULL refuses the keys of an object nested in a typed array, so
-- a held write stating one entity or link failed on every attempt.
DEFINE FIELD IF NOT EXISTS entities_asserted ON quarantine TYPE any DEFAULT [];
DEFINE FIELD IF NOT EXISTS links_asserted    ON quarantine TYPE any DEFAULT [];
DEFINE FIELD IF NOT EXISTS derived_from      ON quarantine TYPE array<string> DEFAULT [];
-- What stays with a private fact's owner: aliases from private facts, and a person's correction of a
-- link no teammate could read (read through the graph read filter).
DEFINE FIELD IF NOT EXISTS private_aliases    ON entity TYPE option<array<string>>;
DEFINE FIELD IF NOT EXISTS private_names_norm ON entity TYPE option<array<string>>;
DEFINE INDEX IF NOT EXISTS entity_private_names ON entity FIELDS private_names_norm.*;
DEFINE FIELD IF NOT EXISTS owner_only ON relationship TYPE option<bool>;
