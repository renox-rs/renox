-- Sessions logged out on one device. Sessions live in cookies, so a logout
-- remembers the session's id until it would have expired anyway.
CREATE TABLE revoked_sessions (
    id TEXT PRIMARY KEY,
    expires_at TEXT NOT NULL
);
