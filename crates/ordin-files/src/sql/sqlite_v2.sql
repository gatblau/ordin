CREATE TABLE work_folder_files (
    namespace    TEXT NOT NULL,
    path         TEXT NOT NULL CHECK (length(path) BETWEEN 1 AND 1024),
    content      TEXT NOT NULL,
    encoding     TEXT NOT NULL CHECK (encoding IN ('utf-8','base64')),
    size_bytes   INTEGER NOT NULL CHECK (size_bytes <= 10485760),
    created_at   TEXT NOT NULL,
    modified_at  TEXT NOT NULL,
    PRIMARY KEY (namespace, path)
);
