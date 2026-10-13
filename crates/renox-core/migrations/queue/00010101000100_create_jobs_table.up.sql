CREATE TABLE jobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    queue TEXT NOT NULL,
    job TEXT NOT NULL,
    payload TEXT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL,
    available_at INTEGER NOT NULL,
    reserved_at INTEGER,
    created_at INTEGER NOT NULL
);
CREATE INDEX jobs_queue_available_at ON jobs (queue, available_at);

CREATE TABLE failed_jobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    queue TEXT NOT NULL,
    job TEXT NOT NULL,
    payload TEXT NOT NULL,
    max_attempts INTEGER NOT NULL,
    error TEXT NOT NULL,
    failed_at INTEGER NOT NULL
);
