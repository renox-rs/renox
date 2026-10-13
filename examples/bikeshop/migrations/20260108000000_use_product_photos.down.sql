-- Back to the categories' drawings: `images/products/{slug}-{n}.webp`
-- (n is one digit) becomes `images/categories/{slug}.svg`.
UPDATE product_photos
SET path = 'images/categories/' || substr(path, 17, length(path) - 23) || '.svg'
WHERE path LIKE 'images/products/%.webp';
