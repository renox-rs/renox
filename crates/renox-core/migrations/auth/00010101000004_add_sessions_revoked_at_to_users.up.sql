-- Unix milliseconds of the last logout: sessions logged in before it have ended.
ALTER TABLE users ADD COLUMN sessions_revoked_at INTEGER NOT NULL DEFAULT 0;
