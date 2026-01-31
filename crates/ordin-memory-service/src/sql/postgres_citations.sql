-- spec: ordin-desktop/components/MemoryService#SA-CITE-02
CREATE TABLE memory_citations_confirmed (
    fact_ref     UUID PRIMARY KEY REFERENCES memory_fact_refs(id) ON DELETE CASCADE,
    confirmed_at TIMESTAMPTZ NOT NULL
);
