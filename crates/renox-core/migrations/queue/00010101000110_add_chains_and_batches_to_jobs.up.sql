-- The jobs to run after this one (a JSON array), and the batch it belongs to.
ALTER TABLE jobs ADD COLUMN chain TEXT;
ALTER TABLE jobs ADD COLUMN batch_id INTEGER;
ALTER TABLE failed_jobs ADD COLUMN chain TEXT;
ALTER TABLE failed_jobs ADD COLUMN batch_id INTEGER;

CREATE TABLE job_batches (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    total INTEGER NOT NULL,
    pending INTEGER NOT NULL,
    failed INTEGER NOT NULL DEFAULT 0,
    allow_failures INTEGER NOT NULL DEFAULT 0,
    then_job TEXT,
    catch_job TEXT,
    finally_job TEXT,
    created_at INTEGER NOT NULL,
    cancelled_at INTEGER,
    finished_at INTEGER
);
