-- A customer's choices, on their login (#238); see the SQLite file for what each holds.
ALTER TABLE users ADD COLUMN locale TEXT;
ALTER TABLE users ADD COLUMN notification_preferences TEXT NOT NULL DEFAULT '{}';
