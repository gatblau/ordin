-- Collections (ADR-020, SA-COLL-01): a caller-chosen label partitioning one scope's facts into sets
-- searched side by side. "" is the default collection, which every row written before this migration
-- and every write that names none belongs to.
--
-- Additive: a field with a default on four tables, backfilled to '' so no existing row is lost or
-- narrowed, and one index per table. `quarantine` (C4's portion) is SCHEMAFULL, so without its field a
-- held write carrying a collection would be refused outright — the case 0006 addressed for
-- `fact_class`.
DEFINE FIELD IF NOT EXISTS collection ON fact TYPE string DEFAULT ''
    ASSERT string::len($value) <= 64 AND string::matches($value, /^[a-z0-9:_-]*$/);
UPDATE fact SET collection = '' WHERE collection IS NONE;
DEFINE INDEX IF NOT EXISTS fact_collection ON fact FIELDS owner.team, collection;
DEFINE FIELD IF NOT EXISTS collection ON entity TYPE string DEFAULT ''
    ASSERT string::len($value) <= 64 AND string::matches($value, /^[a-z0-9:_-]*$/);
UPDATE entity SET collection = '' WHERE collection IS NONE;
DEFINE INDEX IF NOT EXISTS entity_collection ON entity FIELDS owner.team, collection;
DEFINE FIELD IF NOT EXISTS collection ON relationship TYPE string DEFAULT ''
    ASSERT string::len($value) <= 64 AND string::matches($value, /^[a-z0-9:_-]*$/);
UPDATE relationship SET collection = '' WHERE collection IS NONE;
DEFINE INDEX IF NOT EXISTS relationship_collection ON relationship FIELDS owner.team, collection;
DEFINE FIELD IF NOT EXISTS collection ON source TYPE string DEFAULT ''
    ASSERT string::len($value) <= 64 AND string::matches($value, /^[a-z0-9:_-]*$/);
UPDATE source SET collection = '' WHERE collection IS NONE;
DEFINE INDEX IF NOT EXISTS source_collection ON source FIELDS owner.team, collection;
DEFINE FIELD IF NOT EXISTS collection ON quarantine TYPE string DEFAULT '';
