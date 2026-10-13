CREATE TABLE orders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    number TEXT NOT NULL,
    customer TEXT NOT NULL,
    email TEXT NOT NULL,
    region TEXT NOT NULL,
    city TEXT NOT NULL,
    status TEXT NOT NULL,
    tags TEXT NOT NULL DEFAULT '[]',
    items INTEGER NOT NULL DEFAULT 0,
    total INTEGER NOT NULL DEFAULT 0,
    discount REAL NOT NULL DEFAULT 0,
    ordered_on TEXT NOT NULL,
    paid INTEGER NOT NULL DEFAULT 0,
    trend TEXT NOT NULL DEFAULT '[]',
    fulfilled INTEGER NOT NULL DEFAULT 0,
    created_by TEXT NOT NULL DEFAULT '',
    updated_by TEXT NOT NULL DEFAULT '',
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX orders_ordered_on_index ON orders (ordered_on);
