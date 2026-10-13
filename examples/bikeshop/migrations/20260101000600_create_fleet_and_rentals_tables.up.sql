-- The rental fleet (Pagila's inventory) with an owner store and a location store, bikes
-- placed at other stores, and rentals (Pagila's rental) with the store that served them.

CREATE TABLE rental_bikes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    variant_id INTEGER NOT NULL REFERENCES product_variants(id),
    owner_store_id INTEGER NOT NULL REFERENCES stores(id),
    location_store_id INTEGER NOT NULL REFERENCES stores(id),
    frame_number TEXT NOT NULL UNIQUE,
    condition TEXT NOT NULL DEFAULT 'good',
    status TEXT NOT NULL DEFAULT 'available',
    hourly_rate INTEGER NOT NULL,
    daily_rate INTEGER NOT NULL,
    deposit INTEGER NOT NULL,
    asset_value INTEGER NOT NULL DEFAULT 0,
    ridden_hours INTEGER NOT NULL DEFAULT 0,
    purchased_on TEXT,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX rental_bikes_location_store_id_status_index ON rental_bikes (location_store_id, status);
CREATE INDEX rental_bikes_owner_store_id_index ON rental_bikes (owner_store_id);

CREATE TABLE bike_placements (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    rental_bike_id INTEGER NOT NULL REFERENCES rental_bikes(id) ON DELETE CASCADE,
    from_store_id INTEGER NOT NULL REFERENCES stores(id),
    to_store_id INTEGER NOT NULL REFERENCES stores(id),
    status TEXT NOT NULL DEFAULT 'requested',
    requested_at TEXT NOT NULL,
    approved_at TEXT,
    moved_at TEXT,
    recalled_at TEXT,
    requested_by INTEGER REFERENCES staff(id),
    approved_by INTEGER REFERENCES staff(id),
    note TEXT,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX bike_placements_rental_bike_id_index ON bike_placements (rental_bike_id);
CREATE INDEX bike_placements_to_store_id_status_index ON bike_placements (to_store_id, status);

CREATE TABLE rentals (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    reservation_code TEXT NOT NULL UNIQUE,
    customer_id INTEGER NOT NULL REFERENCES customers(id),
    rental_bike_id INTEGER NOT NULL REFERENCES rental_bikes(id),
    owner_store_id INTEGER NOT NULL REFERENCES stores(id),
    operating_store_id INTEGER NOT NULL REFERENCES stores(id),
    return_store_id INTEGER REFERENCES stores(id),
    rate TEXT NOT NULL DEFAULT 'daily',
    starts_at TEXT NOT NULL,
    due_at TEXT NOT NULL,
    picked_up_at TEXT,
    returned_at TEXT,
    status TEXT NOT NULL DEFAULT 'reserved',
    price INTEGER NOT NULL DEFAULT 0,
    deposit INTEGER NOT NULL DEFAULT 0,
    late_fee INTEGER NOT NULL DEFAULT 0,
    damage_fee INTEGER NOT NULL DEFAULT 0,
    served_by INTEGER REFERENCES staff(id),
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX rentals_operating_store_id_status_index ON rentals (operating_store_id, status);
CREATE INDEX rentals_customer_id_index ON rentals (customer_id);
CREATE INDEX rentals_rental_bike_id_index ON rentals (rental_bike_id);
CREATE INDEX rentals_due_at_index ON rentals (due_at);
