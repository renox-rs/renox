-- Related tables for the grid's relationship columns.
CREATE TABLE customers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    tier TEXT NOT NULL DEFAULT 'bronze'
);
ALTER TABLE orders ADD COLUMN customer_id INTEGER REFERENCES customers (id);
CREATE TABLE order_notes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    order_id INTEGER NOT NULL REFERENCES orders (id) ON DELETE CASCADE,
    body TEXT NOT NULL
);
CREATE INDEX order_notes_order_id_index ON order_notes (order_id);
