CREATE TABLE lifecycle_places_v2 (
  id TEXT PRIMARY KEY, place_type TEXT NOT NULL CHECK (place_type IN ('git','sharepoint','jira')),
  address TEXT NOT NULL UNIQUE
);
INSERT INTO lifecycle_places_v2 SELECT id, place_type, address FROM lifecycle_places;
DROP TABLE lifecycle_places;
ALTER TABLE lifecycle_places_v2 RENAME TO lifecycle_places;
