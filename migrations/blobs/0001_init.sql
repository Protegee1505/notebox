-- Attachment chunks, content-addressed by SHA-256.
--
-- Chunks stay under 1 MiB so a row is well below D1's 2 MB limit. A separate
-- database keeps images from ever filling the notes database.

CREATE TABLE blob_meta (
    hash       TEXT    PRIMARY KEY,
    size       INTEGER NOT NULL,
    mime       TEXT    NOT NULL,
    chunks     INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE blob_chunk (
    hash TEXT    NOT NULL,
    idx  INTEGER NOT NULL,
    data BLOB    NOT NULL,
    PRIMARY KEY (hash, idx)
);
