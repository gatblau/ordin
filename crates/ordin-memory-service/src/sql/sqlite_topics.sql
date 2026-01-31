-- spec: ordin-desktop/components/MemoryService#MEM-TOP-1
-- Additive: new tables only; no existing table or column changes.
CREATE TABLE memory_thing_topics (
    place_key   TEXT NOT NULL,
    thing_id    TEXT NOT NULL CHECK (length(thing_id) BETWEEN 1 AND 128),
    topic       TEXT NOT NULL CHECK (length(topic) BETWEEN 1 AND 40),
    topic_norm  TEXT NOT NULL,
    set_by      TEXT NOT NULL CHECK (set_by IN ('ordin','agent','hosted_agent','person')),
    set_at      TEXT NOT NULL,
    PRIMARY KEY (place_key, thing_id)
);
CREATE INDEX memory_thing_topics_topic ON memory_thing_topics (place_key, topic_norm);
-- spec: ordin-desktop/components/MemoryService#MEM-TOP-3
-- A topic given with a remembered fact waits here, by the subject's normalised name, until the memory
-- component has written the thing; a read of the place's topics then moves it to memory_thing_topics.
CREATE TABLE memory_topic_hints (
    place_key    TEXT NOT NULL,
    subject_norm TEXT NOT NULL CHECK (length(subject_norm) BETWEEN 1 AND 512),
    topic        TEXT NOT NULL CHECK (length(topic) BETWEEN 1 AND 40),
    set_by       TEXT NOT NULL CHECK (set_by IN ('ordin','agent','hosted_agent','person')),
    set_at       TEXT NOT NULL,
    PRIMARY KEY (place_key, subject_norm)
);
