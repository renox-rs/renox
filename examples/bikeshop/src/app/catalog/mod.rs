//! The public catalogue: bikes, gear and spare parts, product pages and
//! search (#233).
//!
//! | Route | Name | What |
//! |---|---|---|
//! | `GET /shop` | `catalog.index` | every product, filtered and sorted ([`browse`]) |
//! | `GET /shop/{slug}` | `catalog.category` | a category and the ones under it |
//! | `GET /search?q=` | `catalog.search` | full-text search, with the same filters |
//! | `GET /search/suggest?q=` | `catalog.suggest` | the navbar box's suggestions (an htmx fragment) |
//! | `GET /products/{slug}` | `catalog.show` | a product ([`product`]) |
//! | `GET /sitemap.xml` | `sitemap` | categories and products for search engines ([`sitemap`]) |
//!
//! Every page answers an `ETag` (`Routes::etag`): a browser or a crawler
//! asking again gets a `304 Not Modified` without the body when nothing
//! changed. The home page's storefront (featured bikes, the categories) is
//! shared with the home view by [`storefront`], only on `/`.
//!
//! Made with `rnx make:module catalog`, then the files by hand; the
//! models came with #232.

pub mod browse;
pub mod explain;
pub mod factories;
pub mod filters;
pub mod model;
pub mod product;

use renox::prelude::*;
use serde::Serialize;

use crate::app::staff::model::Store;
use browse::{Card, Tree};
use model::{Category, CategoryKind, Product};

/// The catalog area, registered in `src/lib.rs`.
pub struct Catalog;

impl renox::Module for Catalog {
    fn name(&self) -> &'static str {
        "catalog"
    }

    // [explain:catalog.routes]
    fn routes(&self) -> Routes {
        Routes::new()
            .get("/shop", browse::index)
            .name("catalog.index")
            .get("/shop/{slug}", browse::category)
            .name("catalog.category")
            .get("/search", browse::search)
            .name("catalog.search")
            .get("/search/suggest", browse::suggest)
            .name("catalog.suggest")
            .get("/products/{slug}", product::show)
            .name("catalog.show")
            // Named `sitemap`: Renox's robots.txt points search engines at it.
            .get("/sitemap.xml", sitemap)
            .name("sitemap")
            // A 304 without the body when the page didn't change (a layer
            // covers the routes added before it: all of the above).
            .etag()
    }
    // [/explain:catalog.routes]

    fn register(&self, app: &mut Registry) {
        // `rnx db:check` and the schema test compare these models with the tables.
        app.model::<model::Category>()
            .model::<model::Brand>()
            .model::<model::Product>()
            .model::<model::ProductVariant>()
            .model::<model::ProductPhoto>();
        // [explain:home.storefront]
        // The home page's featured bikes and categories, only for `/`, so
        // other pages run no query for it.
        app.share("storefront", |ctx: renox::view::ViewContext| async move {
            if ctx.path != "/" {
                return Ok(None);
            }
            let mut front = storefront(&ctx.state.db).await?;
            front.rent_url = first_route(
                &ctx.state,
                &["rentals.create", "rentals.new", "rentals.index"],
            );
            // Each store's own page, on its own host (Routes::domain).
            for store in &mut front.stores {
                store.2 = crate::app::home::stores::site_url(&ctx.state.config, &store.1);
            }
            front.service_url = first_route(
                &ctx.state,
                &[
                    "workshop.book",
                    "workshop.bookings.create",
                    "workshop.create",
                    "workshop.index",
                ],
            );
            Ok(Some(front))
        });
        // [/explain:home.storefront]
    }
}

/// How many bikes the home page features.
pub const FEATURED: u64 = 8;

/// A top category on the home page, with its sub-categories.
#[derive(Serialize, Debug, Clone)]
pub struct HomeCategory {
    pub name: String,
    pub href: String,
    pub kind: &'static str,
    pub children: Vec<(String, String)>,
}

/// What the home page shows from the catalogue.
#[derive(Serialize, Debug, Clone)]
pub struct Storefront {
    /// The best-selling bikes in stock.
    pub featured: Vec<Card>,
    /// The top categories and theirs.
    pub categories: Vec<HomeCategory>,
    /// The rental story's start page, once the app has it (#235).
    pub rent_url: Option<String>,
    /// The workshop's booking page, once the app has it (#236).
    pub service_url: Option<String>,
    /// How many bikes the shop sells (the hero's figure).
    pub bikes: u64,
    /// The stores, by name, with their slug (the stores strip's photos are
    /// `public/images/site/store-{slug}.webp`) and their own page's address,
    /// on the store's host (`src/app/home/stores.rs`).
    pub stores: Vec<(String, String, String)>,
}

/// The home page's storefront: the eight best-selling bikes (one query
/// for them, four for their cards) and the category tree (one query).
pub async fn storefront(db: &Db) -> Result<Storefront> {
    let tree = Tree::load(db).await?;
    let bike_categories: Vec<i64> = tree
        .all
        .iter()
        .filter(|c| c.kind == CategoryKind::Bike)
        .map(|c| c.id)
        .collect();
    let featured = Product::query()
        .where_(Product::CATEGORY_ID.is_in(bike_categories.clone()))
        .order_by_raw(
            "(SELECT COALESCE(SUM(oi.quantity), 0) FROM order_items oi \
             JOIN product_variants pv ON pv.id = oi.variant_id \
             WHERE pv.product_id = products.id) DESC",
        )
        .order_by(Product::ID)
        .limit(FEATURED)
        .get(db)
        .await?;
    let categories = tree
        .roots()
        .into_iter()
        .map(|root| HomeCategory {
            name: root.name.clone(),
            href: format!("/shop/{}", root.slug),
            kind: root.kind.as_str(),
            children: tree
                .children(root.id)
                .into_iter()
                .map(|c| (c.name.clone(), format!("/shop/{}", c.slug)))
                .collect(),
        })
        .collect();
    let bikes = Product::query()
        .where_(Product::CATEGORY_ID.is_in(bike_categories))
        .count(db)
        .await?;
    let stores = crate::app::staff::model::Store::query()
        .order_by(Store::ID)
        .select_as::<(String, String), _>(db, "name, slug")
        .await?
        .into_iter()
        .map(|(name, slug)| (name, slug, String::new()))
        .collect();
    Ok(Storefront {
        featured: Card::load(db, featured).await?,
        categories,
        rent_url: None,
        service_url: None,
        bikes,
        stores,
    })
}

/// The path of the first of these routes the app has (without parameters),
/// for links into areas built in parallel (rentals, the workshop): the link
/// shows once the page exists.
pub fn first_route(state: &AppState, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| state.url(name, &[]).ok())
}

// [explain:sitemap.handler]
/// `GET /sitemap.xml` (`sitemap`): the shop's pages for search engines:
/// the home page, the catalogue, every category and every product still
/// sold (discontinued ones are soft deleted, so not listed), with their
/// last change.
pub async fn sitemap(State(state): State<AppState>) -> Result<renox::seo::Sitemap> {
    let mut map = renox::seo::Sitemap::new(&state)
        .route("home", &[], None)?
        .route("catalog.index", &[], None)?;
    for category in Category::query()
        .order_by(Category::POSITION)
        .get(&state.db)
        .await?
    {
        map = map.route("catalog.category", &[&category.slug], category.updated_at)?;
    }
    for (slug, updated) in Product::query()
        .order_by(Product::ID)
        .select_as::<(String, Option<DateTime>), _>(&state.db, "slug, updated_at")
        .await?
    {
        map = map.route("catalog.show", &[&slug], updated)?;
    }
    Ok(map)
}
// [/explain:sitemap.handler]
