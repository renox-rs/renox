-- Outbound webhook subscriptions. `secret` is sealed with APP_KEY (never the plain
-- `whsec_` value). `events` is a JSON array of event names, '[]' meaning every event.
-- Dates are Unix seconds, like `webhook_calls`.
CREATE TABLE webhook_endpoints (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    url TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    secret TEXT NOT NULL,
    events TEXT NOT NULL DEFAULT '[]',
    enabled INTEGER NOT NULL DEFAULT 1,
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    disabled_at INTEGER,
    disabled_reason TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX webhook_endpoints_enabled ON webhook_endpoints (enabled);
