-- renox-2fa: one row per user who started turning two-factor authentication on.
CREATE TABLE two_factor (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL UNIQUE REFERENCES users (id) ON DELETE CASCADE,
    -- The TOTP secret (base32), sealed with APP_KEY (db::Encrypted).
    secret TEXT NOT NULL,
    -- Set once the user typed a code from their app; until then it's off.
    confirmed_at TEXT,
    -- The last time step whose code was used, so a code works once.
    last_used_step INTEGER,
    -- Hashed one-time recovery codes, as a JSON array.
    recovery_codes TEXT NOT NULL DEFAULT '[]',
    created_at TEXT,
    updated_at TEXT
);
