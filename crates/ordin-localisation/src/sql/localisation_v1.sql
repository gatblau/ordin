CREATE TABLE locale_preferences (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    locale      TEXT    NOT NULL CHECK (length(locale) BETWEEN 2 AND 35),
    source      TEXT    NOT NULL CHECK (source IN ('operating_system', 'person')),
    updated_at  TEXT    NOT NULL
);
