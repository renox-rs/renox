-- Back to global roles only: scoped assignments are dropped.
CREATE TABLE role_user_global (
    role_id INTEGER NOT NULL REFERENCES roles (id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    UNIQUE (role_id, user_id)
);
INSERT INTO role_user_global (role_id, user_id)
    SELECT role_id, user_id FROM role_user WHERE scope_type = '' AND scope_id = '';
DROP TABLE role_user;
ALTER TABLE role_user_global RENAME TO role_user;
CREATE INDEX role_user_user_id ON role_user (user_id);
