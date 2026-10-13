CREATE TABLE password_reset_tokens (
    email TEXT PRIMARY KEY NOT NULL,
    token TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);
