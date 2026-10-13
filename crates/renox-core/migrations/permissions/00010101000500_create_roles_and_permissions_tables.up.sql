CREATE TABLE roles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    created_at TEXT,
    updated_at TEXT
);
CREATE TABLE permissions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    created_at TEXT,
    updated_at TEXT
);
-- Which permissions each role grants.
CREATE TABLE permission_role (
    permission_id INTEGER NOT NULL REFERENCES permissions (id) ON DELETE CASCADE,
    role_id INTEGER NOT NULL REFERENCES roles (id) ON DELETE CASCADE,
    UNIQUE (permission_id, role_id)
);
CREATE INDEX permission_role_role_id ON permission_role (role_id);
-- Which roles each user has.
CREATE TABLE role_user (
    role_id INTEGER NOT NULL REFERENCES roles (id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    UNIQUE (role_id, user_id)
);
CREATE INDEX role_user_user_id ON role_user (user_id);
