-- renox-billing: one-off payments (a charge or a payment link) and their
-- refunds. The reference is the app's own record the payment is for
-- (an order, a workshop job); the owner is optional, a link may have none.
CREATE TABLE billing_payments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    billable_type TEXT,
    billable_id INTEGER,
    reference_type TEXT,
    reference_id TEXT,
    -- The amount in the currency's smallest unit, and its ISO 4217 code.
    amount INTEGER NOT NULL,
    currency TEXT NOT NULL,
    -- The gateway's name, and the account of that gateway (NULL: the default one).
    gateway TEXT NOT NULL,
    account TEXT,
    -- The payment's id there (Stripe's `cs_…`, Xendit's invoice id); NULL until it made one.
    gateway_id TEXT,
    -- charge or link.
    kind TEXT NOT NULL,
    -- pending, succeeded, failed, expired, refunded, partially_refunded or disputed.
    status TEXT NOT NULL,
    -- The sum of the refunds that succeeded, in the same currency.
    refunded_amount INTEGER NOT NULL DEFAULT 0,
    -- The payment page of a link or a hosted charge, and when it stops working.
    url TEXT,
    expires_at TEXT,
    -- When the evidence for an open dispute is due.
    evidence_due_by TEXT,
    -- The time of the newest gateway event applied (unix seconds), so an
    -- older event that arrives late changes nothing.
    synced_at INTEGER,
    created_at TEXT,
    updated_at TEXT,
    UNIQUE (gateway, gateway_id)
);

CREATE INDEX billing_payments_billable_index ON billing_payments (billable_type, billable_id);
CREATE INDEX billing_payments_reference_index ON billing_payments (reference_type, reference_id);
CREATE INDEX billing_payments_status_index ON billing_payments (status, expires_at);

CREATE TABLE billing_refunds (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    payment_id INTEGER NOT NULL REFERENCES billing_payments (id) ON DELETE CASCADE,
    -- The refund's id at the gateway; NULL until it made one.
    gateway_id TEXT,
    amount INTEGER NOT NULL,
    -- pending, succeeded or failed.
    status TEXT NOT NULL,
    reason TEXT,
    created_at TEXT,
    updated_at TEXT
);

CREATE INDEX billing_refunds_payment_index ON billing_refunds (payment_id);
