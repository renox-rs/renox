DROP VIEW IF EXISTS stock_overview;
ALTER TABLE purchase_orders DROP COLUMN suggested;
ALTER TABLE purchase_orders DROP COLUMN note;
ALTER TABLE consignment_shipments DROP COLUMN approved_at;
ALTER TABLE consignment_shipments DROP COLUMN approved_by;
ALTER TABLE consignment_shipments DROP COLUMN requested_by;
ALTER TABLE consignment_shipment_lines DROP COLUMN received_quantity;
DROP TABLE IF EXISTS supplier_items;
ALTER TABLE product_variants DROP COLUMN barcode;
