-- The staff side's audit entries say which store the person was working in and which of
-- their roles there gave them the permission (#239), so the owner's audit page filters by
-- store. Renox's Audit module owns the table and leaves both NULL for its own entries
-- (logins, deleted accounts); src/app/staff/audit.rs fills them.
ALTER TABLE audit_logs ADD COLUMN store_id INTEGER;
ALTER TABLE audit_logs ADD COLUMN role TEXT;
CREATE INDEX audit_logs_store_id ON audit_logs (store_id);
