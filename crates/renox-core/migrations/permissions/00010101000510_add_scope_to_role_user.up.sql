-- A role can be given in one record (a store, a team: `scope_type` is its
-- table, `scope_id` its key) and for a period. An empty scope is a global
-- role, as every row before this migration was. SQLite can't drop the old
-- UNIQUE (role_id, user_id), so the table is rebuilt.
CREATE TABLE role_user_scoped (
    role_id INTEGER NOT NULL REFERENCES roles (id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    scope_type TEXT NOT NULL DEFAULT '',
    scope_id TEXT NOT NULL DEFAULT '',
    starts_at TEXT,
    ends_at TEXT,
    UNIQUE (role_id, user_id, scope_type, scope_id)
);
INSERT INTO role_user_scoped (role_id, user_id) SELECT role_id, user_id FROM role_user;
DROP TABLE role_user;
ALTER TABLE role_user_scoped RENAME TO role_user;
CREATE INDEX role_user_user_id ON role_user (user_id);
