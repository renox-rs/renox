-- renox-oauth: the provider accounts (Google, GitHub, …) a user logs in with.
CREATE TABLE oauth_accounts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    -- The provider's name: `google`, `github`, …
    provider TEXT NOT NULL,
    -- The user's id at the provider (Google's `sub`, GitHub's numeric id).
    provider_user_id TEXT NOT NULL,
    -- What the provider said last time, for the account page.
    email TEXT,
    name TEXT,
    avatar TEXT,
    created_at TEXT,
    updated_at TEXT,
    UNIQUE (provider, provider_user_id),
    -- One account per provider for each user.
    UNIQUE (user_id, provider)
);
