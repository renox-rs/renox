ALTER TABLE webhook_calls ALTER COLUMN payload TYPE TEXT USING convert_from(payload, 'UTF8');
