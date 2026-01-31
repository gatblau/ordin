-- Who among one owner's callers wrote a link, and what a person needs to review a held write.
--
-- `relationship.author` tells an owner's callers apart: every agent of a workspace shares its
-- `owner.user`. A link written before this has none, which no agent's write matches. `quarantine.fact_id`
-- names the fact a held write would have been. The indexes list a team's held writes and count its
-- open jobs.
--
-- Additive: two optional fields and two indexes; no backfill.
DEFINE FIELD IF NOT EXISTS author ON relationship TYPE option<string>;
DEFINE FIELD IF NOT EXISTS fact_id ON quarantine TYPE option<string>;
DEFINE INDEX IF NOT EXISTS quarantine_team ON quarantine FIELDS owner.team, quarantined_at;
DEFINE INDEX IF NOT EXISTS work_job_depth ON work_job FIELDS scope.team, status;
