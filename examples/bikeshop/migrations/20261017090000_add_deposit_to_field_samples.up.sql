-- /about/fields: a deposit kept as a `Money` pair (`#[model(money)]`), the
-- amount in the currency's smallest unit and its ISO 4217 code beside it.
-- Both are NULL when no deposit was asked for.
ALTER TABLE field_samples ADD COLUMN deposit BIGINT;
ALTER TABLE field_samples ADD COLUMN deposit_currency TEXT;
