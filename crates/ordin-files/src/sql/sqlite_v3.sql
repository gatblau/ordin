CREATE TABLE agent_write_claims (
    id         TEXT PRIMARY KEY,
    claim      TEXT NOT NULL,
    run_id     TEXT NOT NULL,
    tool       TEXT NOT NULL,
    place_id   TEXT,
    path       TEXT,
    opened_at  TEXT NOT NULL,
    CHECK (path IS NULL OR place_id IS NOT NULL)
);
CREATE INDEX agent_write_claims_place ON agent_write_claims (place_id);
CREATE INDEX agent_write_claims_run ON agent_write_claims (run_id);
CREATE INDEX agent_write_claims_claim ON agent_write_claims (claim);
CREATE UNIQUE INDEX agent_write_claims_one_wide ON agent_write_claims (run_id) WHERE place_id IS NULL;
CREATE UNIQUE INDEX agent_write_claims_one_place ON agent_write_claims (run_id, place_id)
    WHERE place_id IS NOT NULL AND path IS NULL;
