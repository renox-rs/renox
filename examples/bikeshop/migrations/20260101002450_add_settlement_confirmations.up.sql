-- Multi-store operations (#245): a monthly settlement between two stores is marked settled by
-- both of them (the store that pays and the store that is paid each confirm), and remembers
-- when its statement was mailed.

ALTER TABLE settlements ADD COLUMN debtor_confirmed_at TEXT;
ALTER TABLE settlements ADD COLUMN debtor_confirmed_by INTEGER REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE settlements ADD COLUMN creditor_confirmed_at TEXT;
ALTER TABLE settlements ADD COLUMN creditor_confirmed_by INTEGER REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE settlements ADD COLUMN mailed_at TEXT;
UPDATE settlements SET debtor_confirmed_at = settled_at, creditor_confirmed_at = settled_at,
    debtor_confirmed_by = settled_by, creditor_confirmed_by = settled_by
    WHERE status = 'settled';
