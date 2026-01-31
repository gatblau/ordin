-- SQLite, journal database; module "brand", version 1
-- spec: ordin-desktop/components/Brand#SA-RECON-01
CREATE TABLE brand_packs (
    id                  TEXT PRIMARY KEY,                          -- UUIDv7
    brand_identifier    TEXT NOT NULL CHECK (length(brand_identifier) BETWEEN 1 AND 64),
    version             TEXT NOT NULL,
    source              TEXT NOT NULL CHECK (source IN ('build', 'organisation_profile')),
    strings             TEXT NOT NULL,                             -- JSON Vec<BrandStrings>
    logo_light_path     TEXT NOT NULL,
    logo_dark_path      TEXT NOT NULL,
    icon_set_light_path TEXT NOT NULL,
    icon_set_dark_path  TEXT NOT NULL,
    design_tokens       TEXT NOT NULL,                             -- JSON object
    font_file_paths     TEXT NOT NULL,                             -- JSON array
    help_url            TEXT NOT NULL,
    support_url         TEXT NOT NULL,
    privacy_url         TEXT NOT NULL,
    state               TEXT NOT NULL CHECK (state IN ('received', 'contrast_checked', 'active', 'refused')),
    refusal_reason      TEXT CHECK (refusal_reason IS NULL OR length(refusal_reason) <= 500),
    received_at         TEXT NOT NULL,
    activated_at        TEXT,
    CHECK ((state = 'refused') = (refusal_reason IS NOT NULL)),
    UNIQUE (brand_identifier, version, source)
);
CREATE UNIQUE INDEX brand_packs_one_active ON brand_packs (state) WHERE state = 'active';
