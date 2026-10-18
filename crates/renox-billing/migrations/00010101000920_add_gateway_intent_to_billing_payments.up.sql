-- renox-billing: the payment's second id at the gateway. Stripe's disputes
-- and dashboard-made refunds name only the PaymentIntent, never the
-- Checkout Session or Payment Link the row is keyed by.
ALTER TABLE billing_payments ADD COLUMN gateway_intent TEXT;

CREATE INDEX billing_payments_intent_index ON billing_payments (gateway, gateway_intent);
