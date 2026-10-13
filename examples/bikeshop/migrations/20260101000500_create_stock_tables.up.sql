-- Stock with two store attributes: who owns the goods (owner_store_id, their books) and
-- where they are (location_store_id). stock_movements is the ledger; stock_levels the sums.

CREATE TABLE suppliers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    email TEXT,
    phone TEXT,
    address_id INTEGER REFERENCES addresses(id),
    lead_days INTEGER NOT NULL DEFAULT 7,
    created_at TEXT,
    updated_at TEXT
);

CREATE TABLE stock_levels (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    variant_id INTEGER NOT NULL REFERENCES product_variants(id),
    owner_store_id INTEGER NOT NULL REFERENCES stores(id),
    location_store_id INTEGER NOT NULL REFERENCES stores(id),
    on_hand INTEGER NOT NULL DEFAULT 0,
    reserved INTEGER NOT NULL DEFAULT 0,
    created_at TEXT,
    updated_at TEXT
);
CREATE UNIQUE INDEX stock_levels_variant_id_owner_store_id_location_store_id_unique ON stock_levels (variant_id, owner_store_id, location_store_id);
CREATE INDEX stock_levels_location_store_id_index ON stock_levels (location_store_id);
CREATE INDEX stock_levels_owner_store_id_index ON stock_levels (owner_store_id);

CREATE TABLE purchase_orders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    supplier_id INTEGER NOT NULL REFERENCES suppliers(id),
    store_id INTEGER NOT NULL REFERENCES stores(id),
    status TEXT NOT NULL DEFAULT 'draft',
    ordered_at TEXT,
    expected_on TEXT,
    received_at TEXT,
    total INTEGER NOT NULL DEFAULT 0,
    created_by INTEGER REFERENCES staff(id),
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX purchase_orders_store_id_status_index ON purchase_orders (store_id, status);
CREATE INDEX purchase_orders_supplier_id_index ON purchase_orders (supplier_id);

CREATE TABLE purchase_order_lines (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    purchase_order_id INTEGER NOT NULL REFERENCES purchase_orders(id) ON DELETE CASCADE,
    variant_id INTEGER NOT NULL REFERENCES product_variants(id),
    quantity INTEGER NOT NULL,
    received_quantity INTEGER NOT NULL DEFAULT 0,
    unit_cost INTEGER NOT NULL,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX purchase_order_lines_purchase_order_id_index ON purchase_order_lines (purchase_order_id);

CREATE TABLE consignment_shipments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    owner_store_id INTEGER NOT NULL REFERENCES stores(id),
    location_store_id INTEGER NOT NULL REFERENCES stores(id),
    status TEXT NOT NULL DEFAULT 'draft',
    sent_at TEXT,
    received_at TEXT,
    recalled_at TEXT,
    created_by INTEGER REFERENCES staff(id),
    note TEXT,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX consignment_shipments_owner_store_id_index ON consignment_shipments (owner_store_id);
CREATE INDEX consignment_shipments_location_store_id_index ON consignment_shipments (location_store_id);

CREATE TABLE consignment_shipment_lines (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    shipment_id INTEGER NOT NULL REFERENCES consignment_shipments(id) ON DELETE CASCADE,
    variant_id INTEGER NOT NULL REFERENCES product_variants(id),
    quantity INTEGER NOT NULL,
    sold_quantity INTEGER NOT NULL DEFAULT 0,
    returned_quantity INTEGER NOT NULL DEFAULT 0,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX consignment_shipment_lines_shipment_id_index ON consignment_shipment_lines (shipment_id);

CREATE TABLE stock_movements (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    variant_id INTEGER NOT NULL REFERENCES product_variants(id),
    owner_store_id INTEGER NOT NULL REFERENCES stores(id),
    location_store_id INTEGER NOT NULL REFERENCES stores(id),
    quantity INTEGER NOT NULL,
    reason TEXT NOT NULL,
    reference_type TEXT,
    reference_id INTEGER,
    staff_id INTEGER REFERENCES staff(id),
    note TEXT,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX stock_movements_variant_id_index ON stock_movements (variant_id);
CREATE INDEX stock_movements_location_store_id_created_at_index ON stock_movements (location_store_id, created_at);
CREATE INDEX stock_movements_reference_type_reference_id_index ON stock_movements (reference_type, reference_id);
