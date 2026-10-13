CREATE TABLE password_reset_tokens (
    email TEXT PRIMARY KEY NOT NULL COLLATE NOCASE,
    token TEXT NOT NULL,
    created_at TEXT NOT NULL
);
