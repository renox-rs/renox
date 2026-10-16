-- One-time codes and login links (`auth::OneTimeCode`, passwordless login).
-- `code` is a keyed hash of the secret (never the secret); `binding` is a
-- hash tying the code to the browser session that asked for it; `attempts`
-- counts the tries used. One live row per purpose and subject.
CREATE TABLE one_time_codes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    purpose TEXT NOT NULL,
    subject TEXT NOT NULL,
    code TEXT NOT NULL,
    binding TEXT,
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX one_time_codes_subject ON one_time_codes (purpose, subject);
CREATE INDEX one_time_codes_expires_at ON one_time_codes (expires_at);
