CREATE TABLE categories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL
);
CREATE TABLE products (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    sku TEXT NOT NULL UNIQUE,
    price INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'draft',
    active INTEGER NOT NULL DEFAULT 1,
    category_id INTEGER REFERENCES categories (id),
    released_on TEXT,
    created_at TEXT,
    updated_at TEXT,
    deleted_at TEXT
);
