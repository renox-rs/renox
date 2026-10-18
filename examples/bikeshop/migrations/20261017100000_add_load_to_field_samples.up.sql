-- /about/fields: a load limit kept as a `Quantity<3>` (`renox::quantity`), a
-- BIGINT holding thousandths (2.500 kg is 2500), with its unit in a column of
-- its own. The scale is in the Rust type, not in the schema. `load` is NULL
-- when none was given.
ALTER TABLE field_samples ADD COLUMN load BIGINT;
ALTER TABLE field_samples ADD COLUMN load_unit TEXT NOT NULL DEFAULT 'kg';
