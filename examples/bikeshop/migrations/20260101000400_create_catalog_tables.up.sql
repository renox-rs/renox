-- The catalogue (Pagila's film, category and film_actor): bikes, gear and parts, their
-- variants (the SKUs that are sold and stocked), photos, and which parts fit which bikes.

CREATE TABLE categories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_id INTEGER REFERENCES categories(id),
    name TEXT NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL,
    position INTEGER NOT NULL DEFAULT 0,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX categories_parent_id_index ON categories (parent_id);

CREATE TABLE brands (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    website TEXT,
    created_at TEXT,
    updated_at TEXT
);

CREATE TABLE products (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    category_id INTEGER NOT NULL REFERENCES categories(id),
    brand_id INTEGER NOT NULL REFERENCES brands(id),
    name TEXT NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    specs TEXT NOT NULL DEFAULT '{}',
    keywords TEXT NOT NULL DEFAULT '',
    deleted_at TEXT,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX products_category_id_index ON products (category_id);
CREATE INDEX products_brand_id_index ON products (brand_id);

CREATE TABLE product_variants (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    product_id INTEGER NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    sku TEXT NOT NULL UNIQUE,
    size TEXT,
    colour TEXT,
    price INTEGER NOT NULL,
    cost INTEGER NOT NULL,
    reorder_level INTEGER NOT NULL DEFAULT 0,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX product_variants_product_id_index ON product_variants (product_id);

CREATE TABLE product_photos (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    product_id INTEGER NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    alt TEXT NOT NULL DEFAULT '',
    position INTEGER NOT NULL DEFAULT 0,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX product_photos_product_id_index ON product_photos (product_id);

CREATE TABLE part_fits (
    part_id INTEGER NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    bike_id INTEGER NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    note TEXT,
    created_at TEXT,
    updated_at TEXT,
    PRIMARY KEY (part_id, bike_id)
);
CREATE INDEX part_fits_bike_id_index ON part_fits (bike_id);
