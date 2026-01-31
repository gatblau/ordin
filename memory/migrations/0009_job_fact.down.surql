-- Reversal of 0009_job_fact.
--
-- Destructive for `job_fact`: without it a delete leaves the raw payload of a failed or retrying job.
-- `alias_folders` values stay on the SCHEMALESS entity rows, unread. Forward-only in practice; this path
-- exists for the test harness.
REMOVE FIELD IF EXISTS alias_folders ON entity;
REMOVE INDEX IF EXISTS dead_letter_job ON dead_letter;
REMOVE TABLE IF EXISTS job_fact;
