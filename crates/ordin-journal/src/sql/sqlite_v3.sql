-- JA-DIAG-CLEAR-1: when Clear logs last emptied the Diagnostics event list. Entries are kept (support
-- export, run results, workspace tombstones); only lines recorded after this moment are listed.
ALTER TABLE journal_meta ADD COLUMN diagnostics_cleared_at TEXT;
