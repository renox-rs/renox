-- One row per outbound delivery. `message_id` is the `webhook-id` sent with every attempt;
-- `payload` is the JSON body, stored once so a redelivery replays it. `status` is
-- pending, succeeded, retrying or failed; the last attempt's outcome is kept on the row.
CREATE TABLE webhook_deliveries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    endpoint_id INTEGER NOT NULL REFERENCES webhook_endpoints (id) ON DELETE CASCADE,
    message_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    payload TEXT NOT NULL,
    status TEXT NOT NULL,
    attempt INTEGER NOT NULL DEFAULT 0,
    response_status INTEGER,
    error TEXT,
    duration_ms INTEGER,
    created_at INTEGER NOT NULL,
    attempted_at INTEGER,
    next_attempt_at INTEGER
);
CREATE INDEX webhook_deliveries_endpoint ON webhook_deliveries (endpoint_id, created_at);
CREATE INDEX webhook_deliveries_status ON webhook_deliveries (status);
CREATE INDEX webhook_deliveries_created_at ON webhook_deliveries (created_at);
