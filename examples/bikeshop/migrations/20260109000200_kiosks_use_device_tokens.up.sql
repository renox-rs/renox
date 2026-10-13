-- Kiosks act through a Renox device token (owner `kiosk:<id>`) instead of a
-- placeholder user. The old user-backed tokens stop working: kiosks made before
-- this migration are marked revoked, and a manager makes them again.

CREATE TABLE kiosks_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    store_id INTEGER NOT NULL REFERENCES stores(id),
    name TEXT NOT NULL,
    token_id INTEGER,
    abilities TEXT NOT NULL DEFAULT '[]',
    created_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
    revoked_at TEXT,
    created_at TEXT,
    updated_at TEXT
);
INSERT INTO kiosks_new (id, store_id, name, token_id, abilities, created_by, revoked_at, created_at, updated_at)
SELECT id, store_id, name, NULL, abilities, created_by, COALESCE(revoked_at, CURRENT_TIMESTAMP), created_at, updated_at
FROM kiosks;
DELETE FROM personal_access_tokens WHERE user_id IN (SELECT user_id FROM kiosks);
DELETE FROM users WHERE id IN (SELECT user_id FROM kiosks);
DROP TABLE kiosks;
ALTER TABLE kiosks_new RENAME TO kiosks;
CREATE INDEX kiosks_store_id_index ON kiosks (store_id);
