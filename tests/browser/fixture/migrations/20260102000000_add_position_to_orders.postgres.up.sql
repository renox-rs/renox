-- The order rows can be dragged into (the grid's `reorder`).
ALTER TABLE orders ADD COLUMN position BIGINT NOT NULL DEFAULT 0;
