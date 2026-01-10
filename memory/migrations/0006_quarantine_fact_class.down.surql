-- Reversal of 0006_quarantine_fact_class.
--
-- DESTRUCTIVE for the column: the quarantine rows survive, but the record of what each held write
-- claimed to be does not. A reviewer is then left with what the write contained and no statement of
-- what it asserted itself to be. Copy the table before reversing, or roll back only the code — the
-- field is optional, so a build predating it writes rows the schema still accepts.
REMOVE FIELD IF EXISTS fact_class ON quarantine;
