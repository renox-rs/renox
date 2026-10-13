-- Orders (online and at the counter), their lines with the owner store of the goods sold
-- (consigned goods belong to another store), and payments for orders, rentals and work orders.

CREATE TABLE orders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    number TEXT NOT NULL UNIQUE,
    customer_id INTEGER REFERENCES customers(id),
    operating_store_id INTEGER NOT NULL REFERENCES stores(id),
    channel TEXT NOT NULL,
    fulfilment TEXT NOT NULL DEFAULT 'pickup',
    status TEXT NOT NULL DEFAULT 'pending',
    subtotal INTEGER NOT NULL DEFAULT 0,
    discount INTEGER NOT NULL DEFAULT 0,
    delivery_fee INTEGER NOT NULL DEFAULT 0,
    total INTEGER NOT NULL DEFAULT 0,
    delivery_address_id INTEGER REFERENCES addresses(id),
    placed_at TEXT,
    paid_at TEXT,
    served_by INTEGER REFERENCES staff(id),
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX orders_operating_store_id_status_index ON orders (operating_store_id, status);
CREATE INDEX orders_customer_id_index ON orders (customer_id);
CREATE INDEX orders_placed_at_index ON orders (placed_at);

CREATE TABLE order_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    order_id INTEGER NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
    variant_id INTEGER NOT NULL REFERENCES product_variants(id),
    owner_store_id INTEGER NOT NULL REFERENCES stores(id),
    quantity INTEGER NOT NULL,
    unit_price INTEGER NOT NULL,
    total INTEGER NOT NULL,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX order_items_order_id_index ON order_items (order_id);
CREATE INDEX order_items_variant_id_index ON order_items (variant_id);

CREATE TABLE payments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    customer_id INTEGER REFERENCES customers(id),
    payable_type TEXT NOT NULL,
    payable_id INTEGER NOT NULL,
    store_id INTEGER NOT NULL REFERENCES stores(id),
    amount INTEGER NOT NULL,
    method TEXT NOT NULL,
    gateway_reference TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    paid_at TEXT,
    received_by INTEGER REFERENCES staff(id),
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX payments_payable_type_payable_id_index ON payments (payable_type, payable_id);
CREATE INDEX payments_store_id_paid_at_index ON payments (store_id, paid_at);
CREATE INDEX payments_customer_id_index ON payments (customer_id);
