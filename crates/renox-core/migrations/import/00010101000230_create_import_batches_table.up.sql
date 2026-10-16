-- What an undoable import (renox::import) wrote, so it can be undone while its
-- window is open: one batch per import and, per row it inserted or updated, the
-- table, the key, the row as it was before (a JSON list of column/typed value
-- pairs, NULL for an insert) and the row's updated_at right after the write.
CREATE TABLE import_batches (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER,
    rows_written INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    undone_at INTEGER
);

CREATE TABLE import_changes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_id INTEGER NOT NULL REFERENCES import_batches (id) ON DELETE CASCADE,
    table_name TEXT NOT NULL,
    row_key TEXT NOT NULL,
    before_image TEXT,
    stamp TEXT
);
CREATE INDEX import_changes_batch_id ON import_changes (batch_id);
