-- Service plans billed as subscriptions (#237): a plan's parts discount; a subscription's
-- billing owner and gateway, its visit counter, a plan change waiting for the next period,
-- a pause, a hold after a failed payment and the day a cancellation takes effect; each visit
-- (made ahead as a work order), and each payment the gateway reported.

ALTER TABLE service_plans ADD COLUMN parts_discount_bp INTEGER NOT NULL DEFAULT 0;

ALTER TABLE plan_subscriptions ADD COLUMN user_id INTEGER;
ALTER TABLE plan_subscriptions ADD COLUMN gateway TEXT NOT NULL DEFAULT '';
ALTER TABLE plan_subscriptions ADD COLUMN visit_seq INTEGER NOT NULL DEFAULT 0;
ALTER TABLE plan_subscriptions ADD COLUMN next_plan_id INTEGER;
ALTER TABLE plan_subscriptions ADD COLUMN swap_on TEXT;
ALTER TABLE plan_subscriptions ADD COLUMN paused_at TEXT;
ALTER TABLE plan_subscriptions ADD COLUMN held_at TEXT;
ALTER TABLE plan_subscriptions ADD COLUMN ends_on TEXT;
CREATE INDEX plan_subscriptions_user_id_index ON plan_subscriptions (user_id);

CREATE TABLE plan_visits (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    plan_subscription_id INTEGER NOT NULL REFERENCES plan_subscriptions(id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,
    due_on TEXT NOT NULL,
    work_order_id INTEGER REFERENCES work_orders(id) ON DELETE SET NULL,
    status TEXT NOT NULL DEFAULT 'scheduled',
    created_at TEXT,
    updated_at TEXT,
    UNIQUE (plan_subscription_id, seq)
);
CREATE INDEX plan_visits_work_order_id_index ON plan_visits (work_order_id);
CREATE INDEX plan_visits_status_index ON plan_visits (status);

CREATE TABLE plan_invoices (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    plan_subscription_id INTEGER NOT NULL REFERENCES plan_subscriptions(id) ON DELETE CASCADE,
    gateway TEXT NOT NULL,
    payment_id TEXT NOT NULL,
    amount INTEGER NOT NULL,
    currency TEXT NOT NULL,
    paid INTEGER NOT NULL,
    created_at TEXT,
    updated_at TEXT,
    UNIQUE (gateway, payment_id)
);
CREATE INDEX plan_invoices_plan_subscription_id_index ON plan_invoices (plan_subscription_id);
