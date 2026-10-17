-- First responses of `.offline()` routes, kept so a replayed request (same Idempotency-Key) gets
-- the same answer without running the handler again (#52). `scope` is the user or session the key
-- belongs to ('' when anonymous), so one caller cannot replay another's key; (scope, route, key)
-- is unique, and inserting that row first is what serialises concurrent requests. `body_hash` is
-- the SHA-256 (hex) of the request body. `status`, `headers` (JSON text) and `response` stay NULL
-- until the handler has answered. Old rows are removed by `idempotency:prune`.
CREATE TABLE idempotency_keys (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scope TEXT NOT NULL DEFAULT '',
    route TEXT NOT NULL,
    key TEXT NOT NULL,
    body_hash TEXT NOT NULL,
    status INTEGER,
    headers TEXT,
    response BLOB,
    created_at TEXT NOT NULL,
    UNIQUE (scope, route, key)
);
CREATE INDEX idempotency_keys_created_at ON idempotency_keys (created_at);
