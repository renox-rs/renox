-- Written by renox::db::search::migration::<Product> (tests/data.rs checks it).
DROP TRIGGER IF EXISTS "products_search_insert";
DROP TRIGGER IF EXISTS "products_search_update";
DROP TRIGGER IF EXISTS "products_search_delete";
DROP TABLE IF EXISTS "products_search";
CREATE VIRTUAL TABLE "products_search" USING fts5("name", "keywords", "description", content='products', tokenize='porter unicode61 remove_diacritics 2');
CREATE TRIGGER "products_search_insert" AFTER INSERT ON "products" BEGIN
    INSERT INTO "products_search"(rowid, "name", "keywords", "description") VALUES (new.rowid, new."name", new."keywords", new."description");
END;
CREATE TRIGGER "products_search_delete" AFTER DELETE ON "products" BEGIN
    INSERT INTO "products_search"("products_search", rowid, "name", "keywords", "description") VALUES ('delete', old.rowid, old."name", old."keywords", old."description");
END;
CREATE TRIGGER "products_search_update" AFTER UPDATE OF "id", "name", "keywords", "description" ON "products" BEGIN
    INSERT INTO "products_search"("products_search", rowid, "name", "keywords", "description") VALUES ('delete', old.rowid, old."name", old."keywords", old."description");
    INSERT INTO "products_search"(rowid, "name", "keywords", "description") VALUES (new.rowid, new."name", new."keywords", new."description");
END;
INSERT INTO "products_search"("products_search") VALUES ('rebuild');
