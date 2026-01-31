-- Reversal of 0010_link_author.
--
-- Not destructive of stored rows: `relationship` is SCHEMALESS, so `author` values stay, unread. A held
-- write's `fact_id` is dropped with the field on the SCHEMAFULL `quarantine`. While reversed an agent's
-- `ended` link can close another agent's link again.
REMOVE INDEX IF EXISTS work_job_depth ON work_job;
REMOVE INDEX IF EXISTS quarantine_team ON quarantine;
REMOVE FIELD IF EXISTS fact_id ON quarantine;
REMOVE FIELD IF EXISTS author ON relationship;
