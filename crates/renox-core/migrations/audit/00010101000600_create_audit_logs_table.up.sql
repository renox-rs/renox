-- No foreign key on user_id: the trail outlives deleted accounts.
CREATE TABLE audit_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER,
    action TEXT NOT NULL,
    subject_type TEXT,
    subject_id INTEGER,
    data TEXT NOT NULL DEFAULT '{}',
    ip TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX audit_logs_user_id ON audit_logs (user_id);
CREATE INDEX audit_logs_subject ON audit_logs (subject_type, subject_id);
CREATE INDEX audit_logs_created_at ON audit_logs (created_at);
