-- Kiosks act through a Renox device token (owner `kiosk:<id>`) instead of a
-- placeholder user. The old user-backed tokens stop working: kiosks made before
-- this migration are marked revoked, and a manager makes them again.

CREATE TEMPORARY TABLE old_kiosk_users AS SELECT user_id FROM kiosks;
UPDATE kiosks SET token_id = NULL, revoked_at = COALESCE(revoked_at, NOW());
ALTER TABLE kiosks DROP COLUMN user_id;
DELETE FROM users WHERE id IN (SELECT user_id FROM old_kiosk_users);
DROP TABLE old_kiosk_users;
