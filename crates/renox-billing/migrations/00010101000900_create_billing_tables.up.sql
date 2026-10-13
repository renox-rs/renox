-- renox-billing: who pays at which payment provider, and their subscriptions.
-- The owner is any "billable" record: (billable_type, billable_id) is
-- ('user', users.id) for a user, or what the app names (a team).
CREATE TABLE billing_customers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    billable_type TEXT NOT NULL,
    billable_id INTEGER NOT NULL,
    -- The gateway's name: `stripe`, `xendit`, …
    gateway TEXT NOT NULL,
    -- The customer's id there (Stripe's `cus_…`, Xendit's `cust-…`).
    gateway_id TEXT NOT NULL,
    created_at TEXT,
    updated_at TEXT,
    UNIQUE (billable_type, billable_id, gateway),
    UNIQUE (gateway, gateway_id)
);

CREATE TABLE subscriptions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    billable_type TEXT NOT NULL,
    billable_id INTEGER NOT NULL,
    -- Which of the owner's subscriptions: `default` unless the app has more.
    name TEXT NOT NULL,
    -- The plan's key, as declared in code (`Plan::new("pro", …)`).
    plan TEXT NOT NULL,
    -- The gateway's name; empty for a trial without a payment method.
    gateway TEXT NOT NULL,
    -- The subscription's id there (Stripe's `sub_…`, Xendit's `repl_…`).
    gateway_id TEXT,
    -- trialing, active, past_due, canceled or incomplete.
    status TEXT NOT NULL,
    trial_ends_at TEXT,
    -- When access ends after a cancellation (the grace period until then).
    ends_at TEXT,
    current_period_end TEXT,
    -- The time of the newest provider event applied (unix seconds), so an
    -- older event that arrives late changes nothing.
    synced_at INTEGER,
    created_at TEXT,
    updated_at TEXT,
    UNIQUE (gateway, gateway_id)
);

CREATE INDEX subscriptions_billable_index ON subscriptions (billable_type, billable_id, name);
