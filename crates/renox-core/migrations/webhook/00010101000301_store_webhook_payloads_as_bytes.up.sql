-- Payloads are kept exactly as received (signatures are over the bytes).
ALTER TABLE webhook_calls ADD COLUMN body BLOB NOT NULL DEFAULT x'';
UPDATE webhook_calls SET body = CAST(payload AS BLOB);
ALTER TABLE webhook_calls DROP COLUMN payload;
ALTER TABLE webhook_calls RENAME COLUMN body TO payload;
