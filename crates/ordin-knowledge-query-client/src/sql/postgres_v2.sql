CREATE TABLE kq_connection_accounts (
    connection_id  UUID NOT NULL REFERENCES kq_connections(id) ON DELETE CASCADE,
    system         TEXT NOT NULL CHECK (system IN ('github', 'gitlab', 'confluence')),
    state          TEXT NOT NULL CHECK (state IN ('linked', 'not_signed_in', 'refused')),
    changed_at     TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (connection_id, system)
);
