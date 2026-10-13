-- Service plans (a set of tasks every week, month…), and customers' bikes subscribed to them.

CREATE TABLE service_plans (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    slug TEXT NOT NULL UNIQUE,
    frequency TEXT NOT NULL,
    price INTEGER NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT,
    updated_at TEXT
);

CREATE TABLE plan_tasks (
    service_plan_id INTEGER NOT NULL REFERENCES service_plans(id) ON DELETE CASCADE,
    service_task_id INTEGER NOT NULL REFERENCES service_tasks(id) ON DELETE CASCADE,
    created_at TEXT,
    updated_at TEXT,
    PRIMARY KEY (service_plan_id, service_task_id)
);

CREATE TABLE plan_subscriptions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    customer_bike_id INTEGER NOT NULL REFERENCES customer_bikes(id) ON DELETE CASCADE,
    service_plan_id INTEGER NOT NULL REFERENCES service_plans(id),
    store_id INTEGER NOT NULL REFERENCES stores(id),
    preferred_weekday INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL DEFAULT 'active',
    starts_on TEXT NOT NULL,
    next_visit_on TEXT,
    cancelled_at TEXT,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX plan_subscriptions_store_id_status_index ON plan_subscriptions (store_id, status);
CREATE INDEX plan_subscriptions_customer_bike_id_index ON plan_subscriptions (customer_bike_id);
CREATE INDEX plan_subscriptions_next_visit_on_index ON plan_subscriptions (next_visit_on);
