CREATE TABLE work_folder_files (
    namespace    TEXT NOT NULL,
    path         TEXT NOT NULL CHECK (char_length(path) BETWEEN 1 AND 1024),
    content      TEXT NOT NULL,
    encoding     TEXT NOT NULL CHECK (encoding IN ('utf-8','base64')),
    size_bytes   BIGINT NOT NULL CHECK (size_bytes <= 10485760),
    created_at   TIMESTAMPTZ NOT NULL,
    modified_at  TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (namespace, path)
);
CREATE INDEX work_folder_files_prefix ON work_folder_files (namespace, path text_pattern_ops);
