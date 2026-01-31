-- Reversal of 0005_audit_authority.
--
-- DESTRUCTIVE for the two columns: the audit rows survive, but the record of which service acted
-- and which fact class was refused does not. On a deployment that has relied on the class control,
-- this discards exactly the evidence the control was added to produce. Take a copy of `audit_log`
-- before running it, or leave the fields in place and roll back only the code — the fields are
-- optional, so a build predating them writes rows the schema still accepts.
REMOVE FIELD IF EXISTS fact_class ON audit_log;
REMOVE FIELD IF EXISTS client ON audit_log;
