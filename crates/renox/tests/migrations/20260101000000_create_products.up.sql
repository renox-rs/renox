CREATE TABLE products (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    price INTEGER NOT NULL,
    category TEXT,
    created_at TEXT,
    updated_at TEXT,
    deleted_at TEXT
);
