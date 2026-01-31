-- The SHA-256 of a whole-file entry's bytes when the change set was captured (Internal Logic 5.6).
-- NULL for a hunk entry, and for a whole-file entry whose file could not be read at capture, which
-- can then only be rejected.
ALTER TABLE change_set_entries ADD COLUMN content_hash TEXT CHECK (length(content_hash) = 64);
