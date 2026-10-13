CREATE TABLE personal_access_tokens (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    token TEXT NOT NULL UNIQUE,
    last_used_at TEXT,
    expires_at TEXT,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX personal_access_tokens_user_id ON personal_access_tokens (user_id);
