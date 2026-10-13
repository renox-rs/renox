-- What the checkout and the order's life need on top of #232's orders (#234):
-- the customer's language (for the mails sent later, from the queue), when the
-- goods were handed over (the 14-day return window starts then), and when they
-- came back.
ALTER TABLE orders ADD COLUMN locale TEXT;
ALTER TABLE orders ADD COLUMN completed_at TIMESTAMPTZ;
ALTER TABLE orders ADD COLUMN returned_at TIMESTAMPTZ;
