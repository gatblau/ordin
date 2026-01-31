-- Consent Grants (SA-GRANT-01..10). A directional, per-fact, expiring permission
-- from a fact's owner to one named principal. Lives inside the tenant namespace,
-- so a grant can never name a fact in another tenant (ADR-011).
DEFINE TABLE IF NOT EXISTS grant SCHEMAFULL;

-- The fact this grant names, held as the string form of a record id ("fact:<uuidv7>").
-- The read filter converts it with `type::record()` before comparing against `fact.id`,
-- which is a record link: comparing a record against a string never matches (FU-026).
DEFINE FIELD IF NOT EXISTS fact       ON grant TYPE string;
-- SCHEMAFULL requires every sub-field to be declared. Without these three, SurrealDB rejects
-- any write carrying `grantor.tenant` with "no such field exists", and only `grantor: {}`
-- inserts. C1's `fact` table gets away with a bare `owner` because it is SCHEMALESS (FU-027).
DEFINE FIELD IF NOT EXISTS grantor        ON grant TYPE object;
DEFINE FIELD IF NOT EXISTS grantor.tenant ON grant TYPE string;
DEFINE FIELD IF NOT EXISTS grantor.team   ON grant TYPE option<string>;
DEFINE FIELD IF NOT EXISTS grantor.user   ON grant TYPE string;
DEFINE FIELD IF NOT EXISTS grantee    ON grant TYPE string;
DEFINE FIELD IF NOT EXISTS purpose    ON grant TYPE string
    ASSERT string::len($value) > 0 AND string::len($value) <= 512;
-- Read is the only operation a grant may confer (SA-GRANT-05).
DEFINE FIELD IF NOT EXISTS operation  ON grant TYPE string ASSERT $value = 'read';
DEFINE FIELD IF NOT EXISTS valid_from ON grant TYPE datetime;
-- Required, with no default: a grant with no expiry is not expressible (SA-GRANT-04).
DEFINE FIELD IF NOT EXISTS valid_to   ON grant TYPE datetime;
-- Set on revocation. The row is retained so the audit trail can answer what was
-- readable when (SA-GRANT-07).
DEFINE FIELD IF NOT EXISTS revoked_at ON grant TYPE option<datetime>;
DEFINE FIELD IF NOT EXISTS evidence   ON grant TYPE string;
DEFINE FIELD IF NOT EXISTS created_at ON grant TYPE datetime;

-- Serves the read-filter sub-select, in the order the filter tests its columns.
DEFINE INDEX IF NOT EXISTS grant_grantee_active ON grant FIELDS grantee, revoked_at, valid_to;
-- Serves "which grants exist over this fact" on the grantor's review screen, and
-- the cascade check when a fact is hard-deleted.
DEFINE INDEX IF NOT EXISTS grant_fact ON grant FIELDS fact;
