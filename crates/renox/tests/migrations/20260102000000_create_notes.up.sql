CREATE TABLE notes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    product_id INTEGER REFERENCES products (id) ON DELETE CASCADE,
    body TEXT NOT NULL
);
CREATE INDEX notes_product_id ON notes (product_id);
