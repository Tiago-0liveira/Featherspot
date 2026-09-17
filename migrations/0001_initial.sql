CREATE TABLE cache_entries (
    cache_key TEXT PRIMARY KEY NOT NULL,
    payload BLOB NOT NULL,
    fetched_at_ms INTEGER NOT NULL,
    expires_at_ms INTEGER NOT NULL,
    validator TEXT,
    accessed_at_ms INTEGER NOT NULL,
    size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0)
);

CREATE INDEX cache_entries_lru ON cache_entries(accessed_at_ms ASC);

CREATE TABLE capability_cache (
    capability TEXT PRIMARY KEY NOT NULL,
    available INTEGER NOT NULL,
    reason TEXT,
    checked_at_ms INTEGER NOT NULL,
    expires_at_ms INTEGER NOT NULL
);

