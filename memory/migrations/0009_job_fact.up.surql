-- The link from a fact to the job that wrote it, and the folders behind each alias.
--
-- `job_fact` lets `hard_delete` find a failed or retrying job of the fact, whose payload holds the raw
-- content and whose retry would write the fact back. A row is written just before the fact and goes
-- when its job completes. `dead_letter_job` finds that job's parked copy. `entity.alias_folders`
-- records which team's folder stated each alias, so erasing a folder removes what only it added.
--
-- Additive: one new table, one field and one index on existing tables; no backfill. An entity row
-- without the field keeps its aliases through a folder erasure.
DEFINE TABLE IF NOT EXISTS job_fact SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS job          ON job_fact TYPE string;
DEFINE FIELD IF NOT EXISTS fact         ON job_fact TYPE string;
DEFINE FIELD IF NOT EXISTS scope        ON job_fact TYPE object;
DEFINE FIELD IF NOT EXISTS scope.tenant ON job_fact TYPE string;
DEFINE FIELD IF NOT EXISTS scope.team   ON job_fact TYPE option<string>;
DEFINE FIELD IF NOT EXISTS scope.user   ON job_fact TYPE string;
DEFINE INDEX IF NOT EXISTS job_fact_fact ON job_fact FIELDS fact;
DEFINE INDEX IF NOT EXISTS job_fact_job  ON job_fact FIELDS job;
DEFINE INDEX IF NOT EXISTS dead_letter_job ON dead_letter FIELDS original_job_id;
DEFINE FIELD IF NOT EXISTS alias_folders ON entity TYPE option<array<object>>;
