-- Dashboards, reports and exports (#242): read-only views the reports area reads. Nothing is
-- copied: each view is a query over the tables the other areas write, so the numbers can
-- never drift from the records. The same SQL runs on SQLite and PostgreSQL (the
-- `.postgres.up.sql` file is identical); sums are cast to BIGINT because PostgreSQL's SUM of a
-- BIGINT is a NUMERIC.
--
-- report_revenue: one row per line of income, whatever the stream, with BOTH store attributes:
--   owner_store_id     whose books it is in (the bike's or the goods' owner store),
--   operating_store_id the store that did the work (served the customer).
-- Revenue is counted when it is earned: an order once paid (not refunded or cancelled), a
-- rental once returned (price + late fee + damage fee), a work order once completed. Fleet
-- repairs (a rental bike in the workshop) are not income from customers: they are booked
-- between the stores (intercompany_entries), so they are left out. Plan visits (work orders
-- of a plan subscription) and plan payments (payable_type 'plan_subscriptions') are the
-- "plans" stream. `id` is unique across the four parts (source id × 4 + part).

CREATE VIEW report_revenue AS
SELECT oi.id * 4 AS id, 'sales' AS stream, 'orders' AS source_type, o.id AS source_id,
       oi.owner_store_id AS owner_store_id, o.operating_store_id AS operating_store_id,
       o.customer_id AS customer_id, p.id AS product_id, p.category_id AS category_id,
       oi.quantity AS quantity, oi.total AS amount, o.paid_at AS booked_at
FROM order_items oi
JOIN orders o ON o.id = oi.order_id
JOIN product_variants v ON v.id = oi.variant_id
JOIN products p ON p.id = v.product_id
WHERE o.status IN ('paid', 'ready', 'completed') AND o.paid_at IS NOT NULL
UNION ALL
SELECT r.id * 4 + 1, 'rentals', 'rentals', r.id,
       r.owner_store_id, r.operating_store_id,
       r.customer_id, p.id, p.category_id,
       1, r.price + r.late_fee + r.damage_fee, r.returned_at
FROM rentals r
JOIN rental_bikes b ON b.id = r.rental_bike_id
JOIN product_variants v ON v.id = b.variant_id
JOIN products p ON p.id = v.product_id
WHERE r.status = 'returned' AND r.returned_at IS NOT NULL
UNION ALL
SELECT w.id * 4 + 2, CASE WHEN w.plan_subscription_id IS NULL THEN 'workshop' ELSE 'plans' END,
       'work_orders', w.id,
       w.store_id, w.store_id,
       cb.customer_id, CAST(NULL AS BIGINT), CAST(NULL AS BIGINT),
       1, w.total, w.completed_at
FROM work_orders w
LEFT JOIN customer_bikes cb ON cb.id = w.customer_bike_id
WHERE w.status = 'completed' AND w.completed_at IS NOT NULL AND w.rental_bike_id IS NULL
UNION ALL
SELECT pay.id * 4 + 3, 'plans', 'plan_subscriptions', pay.payable_id,
       pay.store_id, pay.store_id,
       pay.customer_id, CAST(NULL AS BIGINT), CAST(NULL AS BIGINT),
       1, pay.amount, pay.paid_at
FROM payments pay
WHERE pay.payable_type = 'plan_subscriptions' AND pay.status = 'paid' AND pay.paid_at IS NOT NULL;

-- One row per customer with their lifetime value per stream (company-wide: customers belong
-- to the company, not to a store), how many purchases, rentals and visits, first and last.
CREATE VIEW report_customers AS
SELECT c.id, c.name, c.email, c.phone, ci.name AS city, c.created_at,
       COALESCE(v.sales_value, 0) AS sales_value,
       COALESCE(v.rentals_value, 0) AS rentals_value,
       COALESCE(v.workshop_value, 0) AS workshop_value,
       COALESCE(v.plans_value, 0) AS plans_value,
       COALESCE(v.lifetime_value, 0) AS lifetime_value,
       COALESCE(v.visits, 0) AS visits,
       v.first_at, v.last_at
FROM customers c
LEFT JOIN addresses a ON a.id = c.address_id
LEFT JOIN cities ci ON ci.id = a.city_id
LEFT JOIN (
    SELECT customer_id,
           CAST(SUM(CASE WHEN stream = 'sales' THEN amount ELSE 0 END) AS BIGINT) AS sales_value,
           CAST(SUM(CASE WHEN stream = 'rentals' THEN amount ELSE 0 END) AS BIGINT) AS rentals_value,
           CAST(SUM(CASE WHEN stream = 'workshop' THEN amount ELSE 0 END) AS BIGINT) AS workshop_value,
           CAST(SUM(CASE WHEN stream = 'plans' THEN amount ELSE 0 END) AS BIGINT) AS plans_value,
           CAST(SUM(amount) AS BIGINT) AS lifetime_value,
           COUNT(DISTINCT source_type || ':' || CAST(source_id AS TEXT)) AS visits,
           MIN(booked_at) AS first_at,
           MAX(booked_at) AS last_at
    FROM report_revenue
    WHERE customer_id IS NOT NULL
    GROUP BY customer_id
) v ON v.customer_id = c.id
WHERE c.deleted_at IS NULL;

-- Orders with their store's and customer's names (so a grid can filter, group and sort by
-- them as plain columns) and the units on them.
CREATE VIEW report_orders AS
SELECT o.id, o.number, o.operating_store_id, s.name AS store,
       o.customer_id, COALESCE(c.name, '') AS customer,
       o.channel, o.fulfilment, o.status,
       COALESCE((SELECT CAST(SUM(i.quantity) AS BIGINT) FROM order_items i WHERE i.order_id = o.id), 0) AS units,
       o.subtotal, o.discount, o.delivery_fee, o.total,
       o.placed_at, o.paid_at, o.completed_at
FROM orders o
JOIN stores s ON s.id = o.operating_store_id
LEFT JOIN customers c ON c.id = o.customer_id;

-- Rentals with both stores' names, the customer, the bike, its model and category, and
-- what the rental brought in (price + late fee + damage fee).
CREATE VIEW report_rentals AS
SELECT r.id, r.reservation_code AS code,
       r.operating_store_id, r.owner_store_id,
       so.name AS store, sw.name AS owner_store,
       c.name AS customer, b.frame_number AS bike, p.name AS model, cat.name AS category,
       r.rate, r.status, r.starts_at, r.due_at, r.picked_up_at, r.returned_at,
       r.price, r.late_fee, r.damage_fee, r.price + r.late_fee + r.damage_fee AS total, r.deposit
FROM rentals r
JOIN stores so ON so.id = r.operating_store_id
JOIN stores sw ON sw.id = r.owner_store_id
JOIN customers c ON c.id = r.customer_id
JOIN rental_bikes b ON b.id = r.rental_bike_id
JOIN product_variants v ON v.id = b.variant_id
JOIN products p ON p.id = v.product_id
JOIN categories cat ON cat.id = p.category_id;

-- Work orders with the workshop's name, the customer (none for a fleet repair), the bike and
-- the mechanic.
CREATE VIEW report_work_orders AS
SELECT w.id, w.store_id, s.name AS store, w.billed_store_id, w.source, w.status,
       COALESCE(c.name, '') AS customer,
       COALESCE(cb.name, rb.frame_number, '') AS bike,
       COALESCE(u.name, '') AS mechanic_name,
       w.scheduled_for, w.started_at, w.completed_at,
       w.labour, w.parts, w.total
FROM work_orders w
JOIN stores s ON s.id = w.store_id
LEFT JOIN customer_bikes cb ON cb.id = w.customer_bike_id
LEFT JOIN customers c ON c.id = cb.customer_id
LEFT JOIN rental_bikes rb ON rb.id = w.rental_bike_id
LEFT JOIN staff st ON st.id = w.mechanic_id
LEFT JOIN users u ON u.id = st.user_id;

-- Payments with the receiving store's and the customer's names.
CREATE VIEW report_payments AS
SELECT pay.id, pay.store_id, s.name AS store, pay.customer_id, COALESCE(c.name, '') AS customer,
       pay.payable_type AS kind, pay.payable_id, pay.amount, pay.method, pay.status,
       pay.paid_at, pay.created_at, COALESCE(pay.gateway_reference, '') AS reference
FROM payments pay
JOIN stores s ON s.id = pay.store_id
LEFT JOIN customers c ON c.id = pay.customer_id;

-- The books between stores with both stores' names and where the entry's settlement stands
-- ('unsettled' until the month is netted, then 'open' or 'settled').
CREATE VIEW report_entries AS
SELECT e.id, e.booked_at, e.kind, e.debtor_store_id, e.creditor_store_id,
       d.name AS debtor, cr.name AS creditor, e.amount, e.fee_rate_bp,
       e.source_type, e.source_id, e.settlement_id,
       COALESCE(st.status, 'unsettled') AS settlement
FROM intercompany_entries e
JOIN stores d ON d.id = e.debtor_store_id
JOIN stores cr ON cr.id = e.creditor_store_id
LEFT JOIN settlements st ON st.id = e.settlement_id;
