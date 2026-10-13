-- See the SQLite file: the store and role of the staff side's audit entries (#239).
ALTER TABLE audit_logs ADD COLUMN store_id BIGINT;
ALTER TABLE audit_logs ADD COLUMN role TEXT;
CREATE INDEX audit_logs_store_id ON audit_logs (store_id);
