-- A customer's choices, on their login (#238):
-- - locale: the language of their pages and mails ('en', 'es'); Renox's notifications read a
--   `locale` column on `users` by themselves (`Recipient::locale`).
-- - notification_preferences: how each kind of notification reaches them, as JSON
--   ({"order": "both", "marketing": "none", …}); read by accounts::channels_for.
ALTER TABLE users ADD COLUMN locale TEXT;
ALTER TABLE users ADD COLUMN notification_preferences TEXT NOT NULL DEFAULT '{}';
