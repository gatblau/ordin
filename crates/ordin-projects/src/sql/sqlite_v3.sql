-- Whether the person lets language servers run on a repository: they run the repository's own build
-- scripts and plug-ins. One row per place, written when the person answers; no row means unanswered.
-- `repository` is the SHA-256 of the origin address and root commit the answer was given for, so a
-- different repository at the same path is asked about again. A hash, because an address can hold
-- a credential.
CREATE TABLE language_server_answers (
    place_id     TEXT PRIMARY KEY,
    allowed      INTEGER NOT NULL CHECK (allowed IN (0, 1)),
    repository   TEXT NOT NULL CHECK (length(repository) = 64),
    answered_at  TEXT NOT NULL
);

-- A task's changed code that no language server looked at, so `new_problems = 0` does not read as
-- no new problems.
ALTER TABLE coding_tasks ADD COLUMN problems_unchecked INTEGER NOT NULL DEFAULT 0;
