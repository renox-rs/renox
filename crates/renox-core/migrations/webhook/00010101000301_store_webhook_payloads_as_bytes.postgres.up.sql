-- Payloads are kept exactly as received (signatures are over the bytes).
ALTER TABLE webhook_calls ALTER COLUMN payload TYPE BYTEA USING convert_to(payload, 'UTF8');
