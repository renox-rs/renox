-- Sessions kept in the database (SESSION_DRIVER=database). The cookie holds
-- only the session id; `id` is its SHA-256, so the table alone can't be
-- used to take over a session.
CREATE TABLE sessions (
    id TEXT PRIMARY KEY NOT NULL,
    user_id BIGINT,
    payload TEXT NOT NULL,
    expires_at BIGINT NOT NULL,
    last_activity BIGINT NOT NULL
);
CREATE INDEX sessions_user_id_index ON sessions (user_id);
CREATE INDEX sessions_expires_at_index ON sessions (expires_at);
