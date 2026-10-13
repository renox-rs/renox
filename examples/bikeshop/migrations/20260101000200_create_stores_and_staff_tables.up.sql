-- The three stores and the people who work in them. No role column on staff: roles are
-- given per store by the Permissions module (role_user.scope_type = 'stores').

CREATE TABLE stores (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    address_id INTEGER NOT NULL REFERENCES addresses(id),
    phone TEXT NOT NULL,
    email TEXT NOT NULL,
    opening_hours TEXT NOT NULL DEFAULT '[]',
    workshop_minutes_per_day INTEGER NOT NULL DEFAULT 960,
    fee_rate_bp INTEGER NOT NULL DEFAULT 2000,
    created_at TEXT,
    updated_at TEXT
);

CREATE TABLE staff (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    home_store_id INTEGER NOT NULL REFERENCES stores(id),
    phone TEXT,
    hired_on TEXT,
    active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX staff_home_store_id_index ON staff (home_store_id);

CREATE TABLE staff_help_requests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    from_store_id INTEGER NOT NULL REFERENCES stores(id),
    to_store_id INTEGER NOT NULL REFERENCES stores(id),
    staff_id INTEGER NOT NULL REFERENCES staff(id),
    role TEXT NOT NULL,
    starts_at TEXT NOT NULL,
    ends_at TEXT NOT NULL,
    reason TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'requested',
    requested_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
    approved_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
    decided_at TEXT,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX staff_help_requests_to_store_id_status_index ON staff_help_requests (to_store_id, status);
CREATE INDEX staff_help_requests_staff_id_index ON staff_help_requests (staff_id);

CREATE TABLE staff_help_hours (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    help_request_id INTEGER NOT NULL REFERENCES staff_help_requests(id) ON DELETE CASCADE,
    staff_id INTEGER NOT NULL REFERENCES staff(id),
    store_id INTEGER NOT NULL REFERENCES stores(id),
    worked_on TEXT NOT NULL,
    minutes INTEGER NOT NULL,
    note TEXT,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX staff_help_hours_help_request_id_index ON staff_help_hours (help_request_id);
CREATE INDEX staff_help_hours_store_id_worked_on_index ON staff_help_hours (store_id, worked_on);
