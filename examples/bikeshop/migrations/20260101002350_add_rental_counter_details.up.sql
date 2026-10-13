-- Rentals at the counter (#235): what happens to the deposit, the condition checklists at
-- pick-up and return, damage notes and photos, reminders, cancellations, the hours ridden;
-- customers' ID documents (the photo is a private upload; the number itself is the
-- customer's Encrypted<String> id_number); a bike's hours at its last service.

ALTER TABLE rentals ADD COLUMN deposit_status TEXT NOT NULL DEFAULT 'unpaid';
ALTER TABLE rentals ADD COLUMN deposit_refunded INTEGER NOT NULL DEFAULT 0;
ALTER TABLE rentals ADD COLUMN pickup_checklist TEXT;
ALTER TABLE rentals ADD COLUMN return_checklist TEXT;
ALTER TABLE rentals ADD COLUMN damage_note TEXT;
ALTER TABLE rentals ADD COLUMN returned_by INTEGER;
ALTER TABLE rentals ADD COLUMN reminded_at TEXT;
ALTER TABLE rentals ADD COLUMN cancelled_at TEXT;
ALTER TABLE rentals ADD COLUMN ridden_minutes INTEGER NOT NULL DEFAULT 0;

ALTER TABLE rental_bikes ADD COLUMN serviced_at_hours INTEGER NOT NULL DEFAULT 0;

CREATE TABLE rental_photos (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    rental_id INTEGER NOT NULL REFERENCES rentals(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    path TEXT NOT NULL,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX rental_photos_rental_id_index ON rental_photos (rental_id);

CREATE TABLE identity_documents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    customer_id INTEGER NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    photo_path TEXT NOT NULL,
    store_id INTEGER NOT NULL REFERENCES stores(id),
    status TEXT NOT NULL DEFAULT 'pending',
    submitted_at TEXT NOT NULL,
    reviewed_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
    reviewed_at TEXT,
    note TEXT,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX identity_documents_customer_id_index ON identity_documents (customer_id);
CREATE INDEX identity_documents_status_index ON identity_documents (status);
