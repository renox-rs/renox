//! The catalogue (Pagila's `film`, `category`, `film_category`,
//! `film_actor` and `film_text`).
//!
//! - A **product** is what a customer looks at: a bike model, a helmet, a
//!   chain. Its **variants** are what is sold and stocked: one SKU per
//!   size and colour, each with its own price, cost and reorder level.
//! - **Categories** form a tree, each of one kind ([`CategoryKind`]):
//!   bikes, gear or parts.
//! - **`part_fits`** says which parts fit which bike models (Pagila's
//!   `film_actor`), with a note: a many-to-many [`Pivot`] between products.
//! - **Search** (Pagila's `film_text`): `#[model(search = …)]` on
//!   [`Product`], over the name, `keywords` (the brand and every SKU,
//!   kept current by [`Product::refresh_keywords`]) and the description.
//!   The index migration is `migrations/20260101000410_search_products.*`
//!   (written by `renox::db::search::migration::<Product>`; `tests/data.rs`
//!   checks the files still match it).
//! - Discontinued products are soft deleted: hidden from the shop, kept
//!   for the orders that point at them.
//!
//! Migration: `migrations/20260101000400_create_catalog_tables.*`.

use renox::db::Json;
use renox::db::relations::{Pivot, belongs_to, has_many};
use renox::prelude::*;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

/// What a category holds.
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CategoryKind {
    /// Bike models: road, mountain, city, e-bikes, kids'.
    #[default]
    Bike,
    /// Helmets, lights, locks, clothing.
    Gear,
    /// Spare parts: chains, tyres, brake pads (they fit bikes: `part_fits`).
    Part,
}

/// A category of the catalogue, in a tree (`parent_id`).
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "categories", index(parent_id))]
pub struct Category {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub slug: String,
    pub kind: CategoryKind,
    /// The order among its siblings.
    pub position: i64,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

/// A brand (Trek, Shimano, Abus…).
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "brands", form, index(name))]
pub struct Brand {
    pub id: i64,
    #[form(validate(required, max = 100))]
    pub name: String,
    #[form(validate(required, max = 100, alpha_dash))]
    pub slug: String,
    #[form(validate(url, max = 255))]
    pub website: Option<String>,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

// [explain:catalog.search.model]
/// A product: a bike model, a piece of gear or a spare part.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(
    table = "products",
    index(category_id),
    index(brand_id),
    soft_deletes,
    search = "name, keywords, description"
)]
pub struct Product {
    pub id: i64,
    pub category_id: i64,
    pub brand_id: i64,
    pub name: String,
    pub slug: String,
    /// Markdown (rich text comes with the admin panel, #239).
    pub description: String,
    /// Specifications shown as a table: frame material, wheel size,
    /// weight, gears… (a map, so its keys sort the same on both databases).
    pub specs: Json<BTreeMap<String, String>>,
    /// The brand's name and every variant's SKU, so a search for "Trek" or
    /// "TRK-DOM-54" finds the product. Kept current by
    /// [`Product::refresh_keywords`].
    pub keywords: String,
    /// Set when the product is discontinued (soft delete).
    pub deleted_at: Option<DateTime>,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}
// [/explain:catalog.search.model]

/// A variant: what is sold, stocked and rented (one SKU).
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "product_variants")]
pub struct ProductVariant {
    pub id: i64,
    pub product_id: i64,
    pub sku: String,
    /// `S`, `M`, `L`, `54 cm`, `29"`… `None` when there's one size.
    pub size: Option<String>,
    pub colour: Option<String>,
    /// The selling price in the smallest unit of `APP_CURRENCY` (an
    /// integer, never a float; shown with the `money` filter).
    pub price: i64,
    /// What the shop paid, same unit.
    pub cost: i64,
    /// Below this many on hand at a store, it shows as low stock.
    pub reorder_level: i64,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

/// A photo of a product (a path under `public/` or the storage disk).
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "product_photos")]
pub struct ProductPhoto {
    pub id: i64,
    pub product_id: i64,
    pub path: String,
    pub alt: String,
    pub position: i64,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
}

/// Which parts fit which bike models (Pagila's `film_actor`): part
/// product → bike products, with a `note` ("needs a 12-speed hub").
pub const PART_FITS: Pivot = Pivot::new("part_fits", "part_id", "bike_id").with_timestamps();

/// The same pivot seen from the bike: bike → the parts that fit it.
pub const FITTING_PARTS: Pivot = PART_FITS.inverse();

/// The note on one `part_fits` link, read with `load_with_pivot`.
#[derive(FromRow, Serialize, Debug, Clone)]
pub struct Fit {
    pub note: Option<String>,
}

impl Product {
    /// Recomputes [`Product::keywords`] from the brand and the variants'
    /// SKUs and saves it (the search index follows by itself).
    pub async fn refresh_keywords(&mut self, db: &Db) -> Result {
        let brand: Option<String> = renox::db::sql("SELECT name FROM brands WHERE id = ?")
            .bind(self.brand_id)
            .scalar_optional(db)
            .await?;
        let skus: Vec<String> = ProductVariant::query()
            .where_(ProductVariant::PRODUCT_ID.eq(self.id))
            .order_by(ProductVariant::SKU)
            .pluck(db, ProductVariant::SKU)
            .await?;
        self.keywords = keywords(brand.as_deref().unwrap_or(""), &skus);
        self.save_only(db, &["keywords"]).await
    }
}

/// The `keywords` text: the brand, then each SKU as written and with its
/// parts apart (`GIR-JER-0001-1 GIR JER 0001 1`).
///
/// Why both: the parts let a search for `0001` find the variant on either
/// database. (A SKU typed whole is found on PostgreSQL too since Renox
/// #304, which reads `-1` in `0001-1` as a signed number.)
pub fn keywords(brand: &str, skus: &[String]) -> String {
    let mut words = vec![brand.to_owned()];
    for sku in skus {
        words.push(sku.clone());
        words.push(sku.replace(['-', '_', '/', '.'], " "));
    }
    words.join(" ").trim().to_owned()
}

/// A product as a list shows it: with its brand, category and variants.
#[derive(Serialize, Debug, Clone)]
pub struct ProductCard {
    #[serde(flatten)]
    pub product: Product,
    pub brand: Option<Brand>,
    pub category: Option<Category>,
    pub variants: Vec<ProductVariant>,
    /// The lowest variant price ("from …").
    pub price_from: Option<i64>,
}

impl ProductCard {
    /// Cards for a page of products: three queries however many products
    /// (brands, categories, variants), never one per product.
    pub async fn load(db: &Db, products: Vec<Product>) -> Result<Vec<ProductCard>> {
        let brands = belongs_to::<Brand, _, _>(db, &products, |p| p.brand_id).await?;
        let categories = belongs_to::<Category, _, _>(db, &products, |p| p.category_id).await?;
        let mut variants: HashMap<i64, Vec<ProductVariant>> = has_many(
            db,
            &products,
            ProductVariant::query().order_by(ProductVariant::PRICE),
            "product_id",
            |v| v.product_id,
        )
        .await?;
        Ok(products
            .into_iter()
            .map(|product| {
                let variants = variants.remove(&product.id).unwrap_or_default();
                ProductCard {
                    brand: brands.get(&product.brand_id).cloned(),
                    category: categories.get(&product.category_id).cloned(),
                    price_from: variants.iter().map(|v| v.price).min(),
                    variants,
                    product,
                }
            })
            .collect())
    }
}
