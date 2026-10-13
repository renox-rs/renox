//! The product page: `/products/{slug}` (`catalog.show`).
//!
//! - `Found<Product>` finds the product by its `slug` column (route model
//!   binding); a discontinued product is soft deleted, so the model's
//!   query doesn't see it and the page answers 404.
//! - The variant picker (size, colour) is a plain `GET` form on the same
//!   address (`?size=M&colour=Blue`). With htmx, a change asks for the
//!   page's `buybox` block only (price, SKU, stock per store, the add-to-cart
//!   form) and keeps the address in step; without JavaScript the form
//!   reloads the page with the choice.
//! - Stock per store: the variant's `stock_levels` rows summed per
//!   location store (on hand minus reserved), in one query for every
//!   variant.
//! - What fits: a part lists the bikes it fits, a bike the parts that fit
//!   it, through the `part_fits` pivot (with its note).
//! - "Recently viewed" lives in the session: the last six product ids.

use std::collections::HashMap;

use renox::prelude::*;
use serde::Serialize;

use super::browse::{Card, Tree, photo_url};
use super::model::{
    Brand, CategoryKind, FITTING_PARTS, Fit, PART_FITS, Product, ProductPhoto, ProductVariant,
};
use crate::app::rentals::model::{BikeStatus, RentalBike};
use crate::app::staff::model::Store;
use crate::app::stock::model::StockLevel;

/// The session key of the recently viewed products (ids, newest first).
pub const RECENTLY_VIEWED: &str = "recently_viewed";
/// How many the session keeps.
pub const RECENT_KEEP: usize = 6;
/// How many related products (fits / fitting parts) the page shows.
pub const RELATED_SHOWN: usize = 8;

/// A variant as the page shows it: with its stock per store.
#[derive(Serialize, Debug, Clone)]
pub struct VariantView {
    #[serde(flatten)]
    pub variant: ProductVariant,
    /// Available (on hand − reserved) per store, in the stores' order.
    pub stock: Vec<StoreStock>,
    /// Available in all stores.
    pub available: i64,
}

/// What one store has of a variant.
#[derive(Serialize, Debug, Clone)]
pub struct StoreStock {
    pub store_id: i64,
    pub store: String,
    pub available: i64,
}

/// A related product (a bike a part fits, or a part that fits a bike).
#[derive(Serialize, Debug, Clone)]
pub struct Related {
    #[serde(flatten)]
    pub card: Card,
    /// The pivot's note: "Check the axle standard before fitting."
    pub note: Option<String>,
}

/// One option of the swatches block.
#[derive(Serialize, Debug, Clone)]
pub struct SwatchOption {
    pub value: String,
    pub label: String,
    pub note: Option<String>,
    pub color: Option<String>,
}

/// The variant asked for: the first with the size and colour asked for,
/// else the first with the size, else the first with the colour, else the
/// first.
pub fn pick<'a>(
    variants: &'a [ProductVariant],
    size: Option<&str>,
    colour: Option<&str>,
) -> Option<&'a ProductVariant> {
    let both = variants
        .iter()
        .find(|v| v.size.as_deref() == size && v.colour.as_deref() == colour);
    both.or_else(|| size.and_then(|s| variants.iter().find(|v| v.size.as_deref() == Some(s))))
        .or_else(|| colour.and_then(|c| variants.iter().find(|v| v.colour.as_deref() == Some(c))))
        .or_else(|| variants.first())
}

/// A colour's swatch: a few named colours, and names that end in one
/// ("Matte Black", "Hi-vis Yellow", "Signal Red") take its colour; others get
/// none (the label still names it).
pub fn colour_hex(name: &str) -> Option<&'static str> {
    let name = name.trim().to_ascii_lowercase();
    let named = |n: &str| -> Option<&'static str> {
        Some(match n {
            "black" => "#1d1d1f",
            "blue" => "#2563eb",
            "navy" => "#1e3a8a",
            "red" => "#dc2626",
            "green" => "#16a34a",
            "orange" => "#ea580c",
            "grey" | "gray" => "#8e8e93",
            "white" => "#f5f5f7",
            "teal" => "#0f766e",
            "silver" => "#c7c7cc",
            "purple" => "#7e22ce",
            "racing green" => "#14532d",
            "flame lacquer" => "#9a3412",
            "hi-vis yellow" => "#d4f000",
            "yellow" => "#eab308",
            _ => return None,
        })
    };
    named(&name).or_else(|| name.rsplit([' ', '-']).next().and_then(named))
}

/// The product's description as one line of plain text, for the page's
/// meta description (`seo()`): the first paragraph, at most 160 characters.
pub fn summary(description: &str) -> String {
    let first = description
        .split("\n\n")
        .find(|p| !p.trim().is_empty() && !p.trim_start().starts_with('#'))
        .unwrap_or("");
    let plain: String = first
        .chars()
        .filter(|c| !matches!(c, '*' | '_' | '`' | '#'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if plain.chars().count() <= 160 {
        return plain;
    }
    let cut: String = plain.chars().take(157).collect();
    format!("{}…", cut.trim_end())
}

/// The variant chosen with `?size=…&colour=…`.
#[derive(serde::Deserialize, Default)]
pub struct Choice {
    size: Option<String>,
    colour: Option<String>,
}

// [explain:catalog.show.handler]
/// `GET /products/{slug}` (`catalog.show`).
pub async fn show(
    State(state): State<AppState>,
    session: Session,
    lang: Lang,
    Found(product): Found<Product>,
    Query(choice): Query<Choice>,
) -> Result<View> {
    // [/explain:catalog.show.handler]
    let db = &state.db;
    let tree = Tree::load(db).await?;
    let category = tree
        .all
        .iter()
        .find(|c| c.id == product.category_id)
        .cloned();
    let breadcrumbs = tree.path_to(product.category_id);
    let brand = Brand::find(db, product.brand_id).await?;
    let variants = ProductVariant::query()
        .where_(ProductVariant::PRODUCT_ID.eq(product.id))
        .order_by(ProductVariant::ID)
        .get(db)
        .await?;
    let photos = ProductPhoto::query()
        .where_(ProductPhoto::PRODUCT_ID.eq(product.id))
        .order_by(ProductPhoto::POSITION)
        .order_by(ProductPhoto::ID)
        .get(db)
        .await?;
    let stores = Store::query().order_by(Store::ID).get(db).await?;
    let levels = StockLevel::query()
        .where_(StockLevel::VARIANT_ID.is_in(variants.iter().map(|v| v.id).collect::<Vec<_>>()))
        .get(db)
        .await?;
    let mut available: HashMap<(i64, i64), i64> = HashMap::new();
    for level in &levels {
        *available
            .entry((level.variant_id, level.location_store_id))
            .or_default() += level.available().max(0);
    }
    let views: Vec<VariantView> = variants
        .iter()
        .map(|v| {
            let stock: Vec<StoreStock> = stores
                .iter()
                .map(|s| StoreStock {
                    store_id: s.id,
                    store: s.name.clone(),
                    available: available.get(&(v.id, s.id)).copied().unwrap_or(0),
                })
                .collect();
            VariantView {
                available: stock.iter().map(|s| s.available).sum(),
                stock,
                variant: v.clone(),
            }
        })
        .collect();
    let chosen = pick(&variants, choice.size.as_deref(), choice.colour.as_deref())
        .map(|v| v.id)
        .and_then(|id| views.iter().find(|v| v.variant.id == id).cloned());

    // The size and colour chips: each value once, in the variants' order;
    // a size sold out everywhere says so.
    let mut sizes: Vec<SwatchOption> = Vec::new();
    let mut colours: Vec<SwatchOption> = Vec::new();
    for view in &views {
        if let Some(size) = &view.variant.size
            && !sizes.iter().any(|s| &s.value == size)
        {
            let left: i64 = views
                .iter()
                .filter(|v| v.variant.size.as_ref() == Some(size))
                .map(|v| v.available)
                .sum();
            sizes.push(SwatchOption {
                value: size.clone(),
                label: size.clone(),
                note: (left == 0).then(|| lang.t("catalog.product.sold_out", &[])),
                color: None,
            });
        }
        if let Some(colour) = &view.variant.colour
            && !colours.iter().any(|c| &c.value == colour)
        {
            colours.push(SwatchOption {
                value: colour.clone(),
                label: colour.clone(),
                note: None,
                color: colour_hex(colour).map(str::to_owned),
            });
        }
    }

    // [explain:catalog.show.fits]
    // What fits what, through the `part_fits` pivot (with its note).
    let kind = category.as_ref().map(|c| c.kind);
    let pivot = match kind {
        Some(CategoryKind::Part) => Some(PART_FITS),
        Some(CategoryKind::Bike) => Some(FITTING_PARTS),
        _ => None,
    };
    let mut related = Vec::new();
    if let Some(pivot) = pivot {
        let mut linked = pivot
            .load_with_pivot::<Product, Fit>(db, [product.id])
            .await?
            .remove(&product.id)
            .unwrap_or_default();
        linked.sort_by(|a, b| a.0.name.cmp(&b.0.name));
        linked.truncate(RELATED_SHOWN);
        let notes: HashMap<i64, Option<String>> = linked
            .iter()
            .map(|(p, fit)| (p.id, fit.note.clone()))
            .collect();
        let cards = Card::load(db, linked.into_iter().map(|(p, _)| p).collect()).await?;
        related = cards
            .into_iter()
            .map(|card| Related {
                note: notes.get(&card.card.product.id).cloned().flatten(),
                card,
            })
            .collect();
    }
    // [/explain:catalog.show.fits]

    // A bike of this model in the rental fleet: "rent this model".
    let rentable = kind == Some(CategoryKind::Bike)
        && RentalBike::query()
            .where_(RentalBike::VARIANT_ID.is_in(variants.iter().map(|v| v.id).collect::<Vec<_>>()))
            .where_not_in("status", [BikeStatus::Retired])
            .exists(db)
            .await?;
    let rent_url = rentable
        .then(|| rental_link(&state, &product.slug))
        .flatten();

    // Recently viewed: the others from the session, then this one first.
    let mut recent: Vec<i64> = session.get(RECENTLY_VIEWED).unwrap_or_default();
    let earlier: Vec<i64> = recent
        .iter()
        .copied()
        .filter(|id| *id != product.id)
        .collect();
    let mut recently = if earlier.is_empty() {
        Vec::new()
    } else {
        let found = Product::find_many(db, earlier.clone()).await?;
        let mut ordered: Vec<Product> = earlier
            .iter()
            .filter_map(|id| found.iter().find(|p| p.id == *id).cloned())
            .collect();
        ordered.truncate(4);
        Card::load(db, ordered).await?
    };
    recently.truncate(4);
    recent.retain(|id| *id != product.id);
    recent.insert(0, product.id);
    recent.truncate(RECENT_KEEP);
    session.put(RECENTLY_VIEWED, &recent)?;

    let gallery: Vec<renox::serde_json::Value> = photos
        .iter()
        .map(|p| json!({ "src": photo_url(&p.path), "alt": p.alt }))
        .collect();
    let og_image = photos.first().map(|p| photo_url(&p.path));
    let summary = summary(&product.description);
    let specs: Vec<(String, String)> = product
        .specs
        .0
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let price_low = variants.iter().map(|v| v.price).min();
    let price_high = variants.iter().map(|v| v.price).max();

    // [explain:catalog.show.handler]
    Ok(view(
        "catalog/show.html",
        context! {
        // [/explain:catalog.show.handler]
                product,
                brand,
                category,
                breadcrumbs,
                kind => kind.map(|k| k.as_str()),
                variants => views,
                chosen,
                sizes,
                colours,
                gallery,
                og_image,
                summary,
                specs,
                price_low,
                price_high,
                related,
                rent_url,
                recently,
                stores,
            },
        // [explain:catalog.show.handler]
    )
    .fragment("buybox"))
}
// [/explain:catalog.show.handler]

/// The rental story's page for this model, when the app has one (#235
/// names it). `None` until then, so the link only shows once it works.
pub fn rental_link(state: &AppState, slug: &str) -> Option<String> {
    super::first_route(state, &["rentals.create", "rentals.new", "rentals.index"])
        .map(|p| format!("{p}?model={}", super::filters::encode(slug)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variant(id: i64, size: &str, colour: &str) -> ProductVariant {
        ProductVariant {
            id,
            size: Some(size.into()),
            colour: Some(colour.into()),
            ..Default::default()
        }
    }

    #[test]
    fn picks_the_closest_variant() {
        let all = [
            variant(1, "S", "Black"),
            variant(2, "M", "Blue"),
            variant(3, "L", "Black"),
        ];
        assert_eq!(pick(&all, Some("L"), Some("Black")).unwrap().id, 3);
        assert_eq!(pick(&all, Some("M"), Some("Black")).unwrap().id, 2);
        assert_eq!(pick(&all, None, Some("Blue")).unwrap().id, 2);
        assert_eq!(pick(&all, Some("XL"), None).unwrap().id, 1);
    }

    #[test]
    fn summaries_are_one_short_line() {
        let text = "# Title\n\nThe **Domane** is a road bike.\nLight and fast.\n\nMore.";
        assert_eq!(summary(text), "The Domane is a road bike. Light and fast.");
        assert!(summary(&"word ".repeat(100)).chars().count() <= 160);
    }

    #[test]
    fn swatches_for_named_colours_and_their_shades() {
        assert_eq!(colour_hex("Black"), Some("#1d1d1f"));
        assert_eq!(colour_hex("Matte Black"), Some("#1d1d1f"));
        assert_eq!(colour_hex("Signal Red"), Some("#dc2626"));
        assert_eq!(colour_hex("Hi-vis Yellow"), Some("#d4f000"));
        assert_eq!(colour_hex("Purple"), Some("#7e22ce"));
        assert_eq!(colour_hex("Navy"), Some("#1e3a8a"));
        assert_eq!(colour_hex("Racing Green"), Some("#14532d"));
        assert_eq!(colour_hex("Chartreuse"), None);
    }
}
