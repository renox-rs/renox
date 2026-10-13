-- Written by renox::db::search::migration::<Product> (tests/data.rs checks it).
DROP TRIGGER IF EXISTS "products_search_insert";
DROP TRIGGER IF EXISTS "products_search_update";
DROP TRIGGER IF EXISTS "products_search_delete";
DROP TABLE IF EXISTS "products_search";
