-- No foreign keys: the log outlives deleted records and accounts. `record_key` is the
-- record's key as text, so integer and string keys share one table.
CREATE TABLE state_transitions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    model TEXT NOT NULL,
    record_key TEXT NOT NULL,
    from_state TEXT,
    to_state TEXT NOT NULL,
    transition TEXT NOT NULL,
    actor_id INTEGER,
    reason TEXT,
    metadata TEXT NOT NULL DEFAULT 'null',
    at TEXT NOT NULL
);
CREATE INDEX state_transitions_record ON state_transitions (model, record_key);
