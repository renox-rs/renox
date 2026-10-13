CREATE TABLE webhook_calls (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider TEXT NOT NULL,
    event_id TEXT NOT NULL,
    payload TEXT NOT NULL,
    status TEXT NOT NULL,
    error TEXT,
    received_at INTEGER NOT NULL,
    processed_at INTEGER,
    UNIQUE (provider, event_id)
);
CREATE INDEX webhook_calls_status ON webhook_calls (status);
