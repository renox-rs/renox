CREATE TABLE items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    quantity INTEGER NOT NULL,
    weight REAL NOT NULL,
    active INTEGER NOT NULL,
    status TEXT NOT NULL,
    tags TEXT NOT NULL,
    extra TEXT,
    opens_at TEXT,
    starts_at TEXT,
    released_on TEXT,
    thumbnail BLOB,
    created_at TEXT,
    updated_at TEXT
);
