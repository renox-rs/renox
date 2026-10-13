-- The workshop (#236): a customer bike's brand, size and photo; a work order's booked
-- minutes (the day's capacity), reminders, cancellation and payment; the parts used (taken
-- from stock, or waited for), the mechanic's notes and photos, and extra work proposed to
-- the customer and approved or refused through a signed link.

ALTER TABLE customer_bikes ADD COLUMN brand TEXT;
ALTER TABLE customer_bikes ADD COLUMN size TEXT;
ALTER TABLE customer_bikes ADD COLUMN photo_path TEXT;

ALTER TABLE work_orders ADD COLUMN minutes INTEGER NOT NULL DEFAULT 0;
ALTER TABLE work_orders ADD COLUMN package TEXT;
ALTER TABLE work_orders ADD COLUMN reminded_at TEXT;
ALTER TABLE work_orders ADD COLUMN cancelled_at TEXT;
ALTER TABLE work_orders ADD COLUMN paid_at TEXT;

CREATE TABLE work_order_parts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    work_order_id INTEGER NOT NULL REFERENCES work_orders(id) ON DELETE CASCADE,
    variant_id INTEGER NOT NULL REFERENCES product_variants(id),
    quantity INTEGER NOT NULL,
    unit_price INTEGER NOT NULL,
    total INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'used',
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX work_order_parts_work_order_id_index ON work_order_parts (work_order_id);

CREATE TABLE work_order_notes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    work_order_id INTEGER NOT NULL REFERENCES work_orders(id) ON DELETE CASCADE,
    staff_id INTEGER REFERENCES staff(id),
    kind TEXT NOT NULL DEFAULT 'note',
    body TEXT,
    photo_path TEXT,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX work_order_notes_work_order_id_index ON work_order_notes (work_order_id);

CREATE TABLE extra_work_requests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    work_order_id INTEGER NOT NULL REFERENCES work_orders(id) ON DELETE CASCADE,
    description TEXT NOT NULL,
    items TEXT NOT NULL DEFAULT '[]',
    total INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    expires_at TEXT NOT NULL,
    decided_at TEXT,
    created_by INTEGER REFERENCES staff(id),
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX extra_work_requests_work_order_id_index ON extra_work_requests (work_order_id);
