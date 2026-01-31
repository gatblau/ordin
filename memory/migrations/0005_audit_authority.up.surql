-- Write authority by fact class (ADR-019, SA-AUTHZ-04). Two additive columns on the append-only
-- audit trail, so a refused class write leaves a record naming who was refused and what they tried
-- to assert. `audit_log` is SCHEMAFULL, so a write carrying an undeclared key is rejected outright:
-- without these two DEFINE FIELDs the refusal path could not be audited at all.
--
-- Additive and forward-only. Nothing is backfilled and no bi-temporal history is collapsed —
-- `audit_log` holds none. Rows written before this migration read back with both fields NULL, which
-- is indistinguishable from a call that carried neither.

-- The calling service / client identity, from the token's `azp` (or the configured claim).
-- Optional because a token that carries none is handled by a stated rule (SA-AUTHZ-08) rather than
-- refused: requiring it would invalidate every token minted before this change.
DEFINE FIELD IF NOT EXISTS client     ON audit_log TYPE option<string>;

-- The fact class the call attempted to assert, on `remember` only. The label the caller supplied
-- and nothing else — never any part of `content` (SA-AUDIT-01 is unchanged by this addition).
DEFINE FIELD IF NOT EXISTS fact_class ON audit_log TYPE option<string>;
