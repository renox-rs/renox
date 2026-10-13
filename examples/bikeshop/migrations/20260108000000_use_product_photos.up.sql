-- The products' photos (#328): databases seeded before the shop had photos
-- point each product at its category's drawing; this points them at one of
-- the category's photos instead, as src/seed/content.rs `product_photo` does.
UPDATE product_photos SET path = CASE path
  WHEN 'images/categories/road-bikes.svg' THEN 'images/products/road-bikes-' || CAST((product_id % 5) + 1 AS TEXT) || '.webp'
  WHEN 'images/categories/mountain-bikes.svg' THEN 'images/products/mountain-bikes-' || CAST((product_id % 3) + 1 AS TEXT) || '.webp'
  WHEN 'images/categories/city-bikes.svg' THEN 'images/products/city-bikes-' || CAST((product_id % 5) + 1 AS TEXT) || '.webp'
  WHEN 'images/categories/folding-bikes.svg' THEN 'images/products/folding-bikes-' || CAST((product_id % 4) + 1 AS TEXT) || '.webp'
  WHEN 'images/categories/e-bikes.svg' THEN 'images/products/e-bikes-' || CAST((product_id % 4) + 1 AS TEXT) || '.webp'
  WHEN 'images/categories/kids-bikes.svg' THEN 'images/products/kids-bikes-' || CAST((product_id % 4) + 1 AS TEXT) || '.webp'
  WHEN 'images/categories/helmets.svg' THEN 'images/products/helmets-' || CAST((product_id % 5) + 1 AS TEXT) || '.webp'
  WHEN 'images/categories/lights.svg' THEN 'images/products/lights-' || CAST((product_id % 2) + 1 AS TEXT) || '.webp'
  WHEN 'images/categories/locks.svg' THEN 'images/products/locks-' || CAST((product_id % 2) + 1 AS TEXT) || '.webp'
  WHEN 'images/categories/clothing.svg' THEN 'images/products/clothing-' || CAST((product_id % 3) + 1 AS TEXT) || '.webp'
  WHEN 'images/categories/bags.svg' THEN 'images/products/bags-' || CAST((product_id % 3) + 1 AS TEXT) || '.webp'
  WHEN 'images/categories/chains.svg' THEN 'images/products/chains-' || CAST((product_id % 3) + 1 AS TEXT) || '.webp'
  WHEN 'images/categories/tyres.svg' THEN 'images/products/tyres-' || CAST((product_id % 3) + 1 AS TEXT) || '.webp'
  WHEN 'images/categories/brakes.svg' THEN 'images/products/brakes-' || CAST((product_id % 3) + 1 AS TEXT) || '.webp'
  WHEN 'images/categories/drivetrain.svg' THEN 'images/products/drivetrain-' || CAST((product_id % 4) + 1 AS TEXT) || '.webp'
  WHEN 'images/categories/saddles.svg' THEN 'images/products/saddles-' || CAST((product_id % 2) + 1 AS TEXT) || '.webp'
  ELSE path END
WHERE path LIKE 'images/categories/%.svg';
