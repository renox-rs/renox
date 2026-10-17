-- Run-time field definitions for models that opt in with `custom_fields` (#41). `model` is the
-- model's table name; `key` is stable (stored values are keyed by it) and matches
-- [a-z][a-z0-9_]*. `label` is a JSON map locale -> text; `rules` and `meta` are JSON text.
-- Deactivating (`active` = 0) hides a field but keeps the values stored for it.
CREATE TABLE custom_fields (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    model TEXT NOT NULL,
    key TEXT NOT NULL,
    kind TEXT NOT NULL,
    label TEXT NOT NULL DEFAULT '{}',
    rules TEXT NOT NULL DEFAULT '{}',
    meta TEXT NOT NULL DEFAULT 'null',
    position INTEGER NOT NULL DEFAULT 0,
    active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (model, key)
);
CREATE INDEX custom_fields_model ON custom_fields (model, active, position);
