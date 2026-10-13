-- The batch a then/catch/finally job was queued for (not counted in it).
ALTER TABLE jobs ADD COLUMN callback_of INTEGER;
ALTER TABLE failed_jobs ADD COLUMN callback_of INTEGER;
