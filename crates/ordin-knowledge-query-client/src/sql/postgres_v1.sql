CREATE TABLE kq_connections (
    id                     UUID PRIMARY KEY,
    workspace_id           TEXT NOT NULL,
    url                    TEXT NOT NULL CHECK (length(url) BETWEEN 9 AND 2048 AND url LIKE 'https://%'),
    name                   TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    issuer                 TEXT NOT NULL CHECK (length(issuer) BETWEEN 9 AND 2048),
    token_audience         TEXT NOT NULL CHECK (length(token_audience) BETWEEN 1 AND 512),
    added_by               TEXT NOT NULL CHECK (added_by IN ('person', 'company')),
    company_entry_key      TEXT CHECK (company_entry_key IS NULL OR length(company_entry_key) BETWEEN 1 AND 64),
    removed_by_person      BOOLEAN NOT NULL DEFAULT FALSE,
    state                  TEXT NOT NULL CHECK (state IN ('ready', 'sign_in_needed', 'not_in_audience', 'unreachable')),
    sources_record_enabled BOOLEAN NOT NULL,
    sources_record_days    INTEGER CHECK (sources_record_days IS NULL OR sources_record_days BETWEEN 1 AND 3650),
    last_problem           TEXT CHECK (last_problem IS NULL OR length(last_problem) <= 300),
    added_at               TIMESTAMPTZ NOT NULL,
    last_answered_at       TIMESTAMPTZ,
    CHECK ((added_by = 'company') = (company_entry_key IS NOT NULL)),
    CHECK (added_by = 'company' OR removed_by_person = FALSE)
);
CREATE UNIQUE INDEX kq_connections_ws_url ON kq_connections (workspace_id, url);
CREATE UNIQUE INDEX kq_connections_ws_company ON kq_connections (workspace_id, company_entry_key) WHERE company_entry_key IS NOT NULL;

CREATE TABLE kq_connection_contents (
    connection_id UUID PRIMARY KEY REFERENCES kq_connections(id) ON DELETE CASCADE,
    contents_json JSONB NOT NULL CHECK (octet_length(contents_json::text) <= 2097152),
    fetched_at    TIMESTAMPTZ NOT NULL
);
