-- 0001_server_connections.sql
CREATE TABLE server_connections (
    id                      TEXT PRIMARY KEY CHECK (length(id) = 36),
    display_name            TEXT NOT NULL CHECK (length(display_name) BETWEEN 1 AND 120),
    address                 TEXT NOT NULL CHECK (length(address) BETWEEN 1 AND 253),
    port                    INTEGER NOT NULL CHECK (port BETWEEN 1025 AND 65535),
    service_api_version     TEXT,
    last_sequence_received  INTEGER NOT NULL DEFAULT 0 CHECK (last_sequence_received >= 0),
    keychain_ref            TEXT NOT NULL UNIQUE,
    state                   TEXT NOT NULL CHECK (state IN ('connected','not_connected','disconnected','refused_for_version')),
    paired_at               TEXT NOT NULL,
    UNIQUE (address, port)
);

-- Private to this crate: what 2C's ServerConnection does not carry.
CREATE TABLE server_connection_pins (
    server_connection_id    TEXT PRIMARY KEY REFERENCES server_connections(id) ON DELETE CASCADE,
    fingerprint_sha256      TEXT NOT NULL CHECK (length(fingerprint_sha256) = 64)
);

CREATE TABLE server_connection_backends (
    server_connection_id    TEXT PRIMARY KEY REFERENCES server_connections(id) ON DELETE CASCADE,
    backend_id              TEXT NOT NULL UNIQUE CHECK (length(backend_id) = 36),
    desktop_pairing_id      TEXT NOT NULL CHECK (length(desktop_pairing_id) = 36)
);
