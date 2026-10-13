-- The workshop: customers' bikes, the tasks a mechanic does, and work orders for a
-- customer's bike or a rental bike of the fleet (billed to its owner store).

CREATE TABLE customer_bikes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    customer_id INTEGER NOT NULL REFERENCES customers(id) ON DELETE CASCADE,
    product_id INTEGER REFERENCES products(id),
    name TEXT NOT NULL,
    frame_number TEXT,
    order_id INTEGER REFERENCES orders(id),
    bought_on TEXT,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX customer_bikes_customer_id_index ON customer_bikes (customer_id);

CREATE TABLE service_tasks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    minutes INTEGER NOT NULL,
    price INTEGER NOT NULL,
    created_at TEXT,
    updated_at TEXT
);

CREATE TABLE work_orders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    customer_bike_id INTEGER REFERENCES customer_bikes(id),
    rental_bike_id INTEGER REFERENCES rental_bikes(id),
    store_id INTEGER NOT NULL REFERENCES stores(id),
    mechanic_id INTEGER REFERENCES staff(id),
    source TEXT NOT NULL,
    plan_subscription_id INTEGER,
    billed_store_id INTEGER REFERENCES stores(id),
    scheduled_for TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'booked',
    labour INTEGER NOT NULL DEFAULT 0,
    parts INTEGER NOT NULL DEFAULT 0,
    total INTEGER NOT NULL DEFAULT 0,
    customer_note TEXT,
    started_at TEXT,
    completed_at TEXT,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX work_orders_store_id_status_index ON work_orders (store_id, status);
CREATE INDEX work_orders_customer_bike_id_index ON work_orders (customer_bike_id);
CREATE INDEX work_orders_rental_bike_id_index ON work_orders (rental_bike_id);
CREATE INDEX work_orders_scheduled_for_index ON work_orders (scheduled_for);

CREATE TABLE work_order_tasks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    work_order_id INTEGER NOT NULL REFERENCES work_orders(id) ON DELETE CASCADE,
    service_task_id INTEGER NOT NULL REFERENCES service_tasks(id),
    minutes INTEGER NOT NULL,
    price INTEGER NOT NULL,
    done INTEGER NOT NULL DEFAULT 0,
    note TEXT,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX work_order_tasks_work_order_id_index ON work_order_tasks (work_order_id);
