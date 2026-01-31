CREATE TABLE person_languages (
  locale            TEXT PRIMARY KEY CHECK (length(locale) BETWEEN 2 AND 35),
  kind              TEXT NOT NULL CHECK (kind IN ('shipped','made','imported')),
  state             TEXT NOT NULL CHECK (state IN ('drafting','needs_review','in_use')),
  name              TEXT CHECK (name IS NULL OR length(name) BETWEEN 1 AND 60),
  made_for_version  TEXT NOT NULL,
  checked_against   TEXT NOT NULL,
  draft_total       INTEGER CHECK (draft_total IS NULL OR draft_total >= 0),
  created_at        TEXT NOT NULL,
  updated_at        TEXT NOT NULL,
  confirmed_at      TEXT,
  CHECK (kind <> 'shipped' OR state = 'in_use')
);
CREATE TABLE person_messages (
  locale      TEXT NOT NULL REFERENCES person_languages(locale) ON DELETE CASCADE,
  message_id  TEXT NOT NULL CHECK (length(message_id) BETWEEN 1 AND 200),
  ftl         TEXT NOT NULL CHECK (length(ftl) BETWEEN 1 AND 4096),
  origin      TEXT NOT NULL CHECK (origin IN ('edited','drafted','imported')),
  reviewed    INTEGER NOT NULL CHECK (reviewed IN (0,1)),
  usable      INTEGER NOT NULL CHECK (usable IN (0,1)),
  updated_at  TEXT NOT NULL,
  PRIMARY KEY (locale, message_id)
);
