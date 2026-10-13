-- The books between stores: an entry whenever the store that owns a bike or goods isn't the
-- one that did the work (who owes whom, why, for what), and the monthly settlements.

CREATE TABLE settlements (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    debtor_store_id INTEGER NOT NULL REFERENCES stores(id),
    creditor_store_id INTEGER NOT NULL REFERENCES stores(id),
    period_start TEXT NOT NULL,
    period_end TEXT NOT NULL,
    amount INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'open',
    settled_at TEXT,
    settled_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
    created_at TEXT,
    updated_at TEXT
);
CREATE UNIQUE INDEX settlements_debtor_store_id_creditor_store_id_period_start_unique ON settlements (debtor_store_id, creditor_store_id, period_start);

CREATE TABLE intercompany_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    debtor_store_id INTEGER NOT NULL REFERENCES stores(id),
    creditor_store_id INTEGER NOT NULL REFERENCES stores(id),
    amount INTEGER NOT NULL,
    kind TEXT NOT NULL,
    fee_rate_bp INTEGER,
    source_type TEXT NOT NULL,
    source_id INTEGER NOT NULL,
    settlement_id INTEGER REFERENCES settlements(id) ON DELETE SET NULL,
    booked_at TEXT NOT NULL,
    created_at TEXT,
    updated_at TEXT
);
CREATE INDEX intercompany_entries_debtor_store_id_creditor_store_id_index ON intercompany_entries (debtor_store_id, creditor_store_id);
CREATE INDEX intercompany_entries_source_type_source_id_index ON intercompany_entries (source_type, source_id);
CREATE INDEX intercompany_entries_settlement_id_index ON intercompany_entries (settlement_id);
CREATE INDEX intercompany_entries_booked_at_index ON intercompany_entries (booked_at);
