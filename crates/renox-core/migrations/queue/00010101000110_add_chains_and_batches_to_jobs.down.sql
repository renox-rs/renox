DROP TABLE job_batches;
ALTER TABLE failed_jobs DROP COLUMN batch_id;
ALTER TABLE failed_jobs DROP COLUMN chain;
ALTER TABLE jobs DROP COLUMN batch_id;
ALTER TABLE jobs DROP COLUMN chain;
