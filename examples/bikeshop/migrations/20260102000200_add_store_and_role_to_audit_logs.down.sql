DROP INDEX IF EXISTS audit_logs_store_id;
ALTER TABLE audit_logs DROP COLUMN role;
ALTER TABLE audit_logs DROP COLUMN store_id;
