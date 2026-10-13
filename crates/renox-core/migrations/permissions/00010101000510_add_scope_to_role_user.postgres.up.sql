-- A role can be given in one record (a store, a team: `scope_type` is its
-- table, `scope_id` its key) and for a period. An empty scope is a global
-- role, as every row before this migration was. Empty strings rather than
-- NULLs, so (role, user, global) stays unique.
ALTER TABLE role_user
    ADD COLUMN scope_type TEXT NOT NULL DEFAULT '',
    ADD COLUMN scope_id TEXT NOT NULL DEFAULT '',
    ADD COLUMN starts_at TIMESTAMPTZ,
    ADD COLUMN ends_at TIMESTAMPTZ;
ALTER TABLE role_user DROP CONSTRAINT role_user_role_id_user_id_key;
ALTER TABLE role_user
    ADD CONSTRAINT role_user_role_id_user_id_scope_key
    UNIQUE (role_id, user_id, scope_type, scope_id);
