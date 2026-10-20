-- Notes and authentication database.
--
-- The server stores opaque Loro updates in commit order and never merges
-- them. Login keys are already stretched on the device; `auth_hash` is a fast
-- SHA-256 of that key so the 10 ms CPU budget is not spent on Argon2.

CREATE TABLE account (
    id             INTEGER PRIMARY KEY CHECK (id = 1),
    username       TEXT    NOT NULL UNIQUE,
    auth_hash      TEXT    NOT NULL,
    created_at     INTEGER NOT NULL,
    failed_logins  INTEGER NOT NULL DEFAULT 0,
    blocked_until  INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE device (
    id          TEXT    PRIMARY KEY,
    name        TEXT    NOT NULL,
    token_hash  TEXT    NOT NULL UNIQUE,
    created_at  INTEGER NOT NULL,
    last_seen   INTEGER NOT NULL
);

CREATE TABLE update_log (
    seq        INTEGER PRIMARY KEY AUTOINCREMENT,
    note_id    TEXT    NOT NULL,
    blob       TEXT    NOT NULL,
    device_id  TEXT    NOT NULL,
    created_at INTEGER NOT NULL
);
