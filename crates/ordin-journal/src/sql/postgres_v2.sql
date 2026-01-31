-- JA-EXT-1: an external action the destination refused is `refused`.
ALTER TABLE external_actions DROP CONSTRAINT external_actions_state_check;
ALTER TABLE external_actions ADD CONSTRAINT external_actions_state_check
    CHECK (state IN ('sent','confirmed','unknown_outcome','refused'));
