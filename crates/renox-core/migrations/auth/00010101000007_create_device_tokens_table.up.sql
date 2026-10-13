-- API tokens for a device (a kiosk, a till, a sensor) instead of a user.
-- `device` is the app's own key for the owner, e.g. 'kiosk:3'.
CREATE TABLE device_tokens (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    device TEXT NOT NULL,
    name TEXT NOT NULL,
    token TEXT NOT NULL UNIQUE,
    abilities TEXT,
    last_used_at TEXT,
    expires_at TEXT,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX device_tokens_device ON device_tokens (device);
