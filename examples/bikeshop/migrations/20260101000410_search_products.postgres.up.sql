-- Written by renox::db::search::migration::<Product> (tests/data.rs checks it).
DROP INDEX IF EXISTS "products_search_index";
ALTER TABLE "products" DROP COLUMN IF EXISTS "search_vector";
ALTER TABLE "products" ADD COLUMN "search_vector" tsvector GENERATED ALWAYS AS (setweight(to_tsvector('english'::regconfig, coalesce("name"::text, '')), 'A') || setweight(to_tsvector('english'::regconfig, coalesce("keywords"::text, '')), 'B') || setweight(to_tsvector('english'::regconfig, coalesce("description"::text, '')), 'C')) STORED;
CREATE INDEX "products_search_index" ON "products" USING GIN ("search_vector");
