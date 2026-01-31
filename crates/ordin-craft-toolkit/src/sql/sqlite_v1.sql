-- spec: ordin-desktop/components/CraftToolkit#SA-RECON-06
CREATE TABLE craft_installations (
    id                   TEXT PRIMARY KEY,
    app_name             TEXT NOT NULL UNIQUE CHECK (length(app_name) BETWEEN 1 AND 40),
    pinned_version       TEXT NOT NULL,
    installed_version    TEXT,
    installed_sha256     TEXT,                                -- the hash of the archive installed_version came from
    verification_result  TEXT CHECK (verification_result IN ('verified','failed')),
    cli_modes            TEXT NOT NULL DEFAULT '[]',          -- JSON Vec<CraftCliMode>
    status               TEXT NOT NULL CHECK (status IN ('approved','downloaded','installed','verification_failed','removed')),
    signature_check      TEXT,                                -- 'valid', 'invalid', 'unsigned', 'not_checked' (C-47, Diagnostics)
    updated_at           TEXT NOT NULL,
    CHECK (status != 'installed' OR verification_result = 'verified')
);

CREATE TABLE file_type_map_entries (
    id               TEXT PRIMARY KEY,
    extensions       TEXT NOT NULL,                           -- JSON Vec<String>
    media_types      TEXT NOT NULL,                           -- JSON Vec<String>
    open_with_app    TEXT,
    craft_tools      TEXT NOT NULL,                           -- JSON Vec<String>
    profile_version  TEXT NOT NULL
);

CREATE TABLE file_type_extensions (                           -- lookup and the one-entry-per-extension rule
    extension  TEXT PRIMARY KEY CHECK (length(extension) BETWEEN 1 AND 16),
    entry_id   TEXT NOT NULL REFERENCES file_type_map_entries(id) ON DELETE CASCADE
);
