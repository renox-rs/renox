-- Back to global roles only: scoped assignments are dropped.
DELETE FROM role_user WHERE scope_type <> '' OR scope_id <> '';
ALTER TABLE role_user DROP CONSTRAINT role_user_role_id_user_id_scope_key;
ALTER TABLE role_user
    DROP COLUMN scope_type,
    DROP COLUMN scope_id,
    DROP COLUMN starts_at,
    DROP COLUMN ends_at;
ALTER TABLE role_user ADD CONSTRAINT role_user_role_id_user_id_key UNIQUE (role_id, user_id);
