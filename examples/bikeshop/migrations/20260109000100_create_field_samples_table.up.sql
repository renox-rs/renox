-- /about/fields (src/app/about/fields.rs): a sample bike whose every column
-- is one kind of form input, on SQLite. The PostgreSQL types are in
-- the .postgres.up.sql next to it.
CREATE TABLE field_samples (
    id BLOB PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT,
    stock INTEGER NOT NULL,
    weight_kg REAL NOT NULL,
    price INTEGER NOT NULL,
    available INTEGER NOT NULL,
    size TEXT NOT NULL,
    colors TEXT NOT NULL DEFAULT '[]',
    tags TEXT NOT NULL DEFAULT '[]',
    specs TEXT NOT NULL DEFAULT '[]',
    details TEXT,
    settings TEXT,
    brand TEXT,
    pickup_at TEXT,
    launch_at TEXT,
    released_on TEXT,
    photo TEXT,
    manual TEXT,
    manual_name TEXT,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX field_samples_user_id_index ON field_samples (user_id);
