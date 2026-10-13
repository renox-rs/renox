-- Saved carts (#234): a logged-in customer's cart, kept between visits and devices.
-- A guest's cart lives in the session; at the first visit after logging in it is
-- merged into this row. `lines` is JSON: [{"variant_id": 7, "quantity": 2}, …].

CREATE TABLE carts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    store_id INTEGER REFERENCES stores(id),
    lines TEXT NOT NULL DEFAULT '[]',
    created_at TEXT,
    updated_at TEXT
);
