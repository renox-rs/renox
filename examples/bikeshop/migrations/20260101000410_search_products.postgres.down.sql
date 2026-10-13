-- Written by renox::db::search::migration::<Product> (tests/data.rs checks it).
DROP INDEX IF EXISTS "products_search_index";
ALTER TABLE "products" DROP COLUMN IF EXISTS "search_vector";
