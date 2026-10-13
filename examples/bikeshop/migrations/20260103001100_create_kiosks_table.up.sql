-- Store kiosks (#241): a self-service kiosk next to a store's bike racks, acting through
-- its own user (no password anyone knows) and that user's API token, limited to the
-- rental abilities a manager gave it. Revoked kiosks keep their row for the history.

CREATE TABLE kiosks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    store_id INTEGER NOT NULL REFERENCES stores(id),
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    token_id INTEGER,
    abilities TEXT NOT NULL DEFAULT '[]',
    created_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
    revoked_at TEXT,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX kiosks_store_id_index ON kiosks (store_id);
CREATE UNIQUE INDEX kiosks_user_id_unique ON kiosks (user_id);
