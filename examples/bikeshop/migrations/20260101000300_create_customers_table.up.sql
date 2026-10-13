-- Customers belong to the company, not to a store. Walk-ins have no user account.
-- id_number is sealed with APP_KEY (Encrypted<String>): unreadable in the table.

CREATE TABLE customers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER REFERENCES users(id) ON DELETE SET NULL,
    name TEXT NOT NULL,
    email TEXT,
    phone TEXT,
    address_id INTEGER REFERENCES addresses(id),
    id_number TEXT,
    id_verified_at TEXT,
    active INTEGER NOT NULL DEFAULT 1,
    deleted_at TEXT,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX customers_user_id_index ON customers (user_id);
CREATE INDEX customers_email_index ON customers (email);
