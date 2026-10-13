-- Places, as in Pagila: countries, their cities, and addresses in them.

CREATE TABLE countries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    code TEXT NOT NULL UNIQUE,
    created_at TEXT,
    updated_at TEXT
);

CREATE TABLE cities (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    country_id INTEGER NOT NULL REFERENCES countries(id),
    name TEXT NOT NULL,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX cities_country_id_index ON cities (country_id);

CREATE TABLE addresses (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    city_id INTEGER NOT NULL REFERENCES cities(id),
    line1 TEXT NOT NULL,
    line2 TEXT,
    district TEXT,
    postal_code TEXT,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX addresses_city_id_index ON addresses (city_id);
