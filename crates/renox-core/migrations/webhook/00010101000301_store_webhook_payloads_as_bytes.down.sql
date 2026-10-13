ALTER TABLE webhook_calls ADD COLUMN text_payload TEXT NOT NULL DEFAULT '';
UPDATE webhook_calls SET text_payload = CAST(payload AS TEXT);
ALTER TABLE webhook_calls DROP COLUMN payload;
ALTER TABLE webhook_calls RENAME COLUMN text_payload TO payload;
