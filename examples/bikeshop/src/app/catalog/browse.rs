//! Browsing: the whole catalogue (`/shop`), a category (`/shop/{slug}`),
//! search results (`/search?q=`) and the navbar's suggestions.
//!
//! The three pages are one handler, [`listing`], with a different scope:
//! a category (and the categories under it), words to search for, or
//! everything. Each page:
//!
//! 1. reads the filters from the query string ([`Filters`]);
//! 2. counts and loads one page of products with them ([`Filters::apply`]
//!    on `Product::query()`, which hides discontinued products: they are
//!    soft deleted);
//! 3. loads what the cards show for the whole page at once
//!    ([`ProductCard::load`]: brands, categories, variants; then the
//!    first photos), never one query per product;
//! 4. works out the filter choices (brands, sizes, price range, wheel sizes,
//!    frame materials) from the products in scope, in a fixed number of
//!    queries.
//!
//! An htmx request (the filter form, a pagination link) gets only the
//! `results` block of the page (`View::fragment`); a plain request gets the
//! whole page, so everything works without JavaScript.

use std::collections::{BTreeSet, HashMap};

use renox::db::Json;
use renox::db::relations::has_many;
use renox::prelude::*;
use serde::Serialize;

use super::filters::{Chip, Filters, SPEC_FRAME, SPEC_MOTOR, SPEC_WHEELS, Sort};
use super::model::{
    Brand, Category, CategoryKind, Product, ProductCard, ProductPhoto, ProductVariant,
};
use crate::app::accounts::model::Customer;
use crate::app::staff::model::Store;
use crate::app::workshop::model::CustomerBike;

/// Products per page.
pub const PER_PAGE: u32 = 24;

/// A product as a card in a list: [`ProductCard`] plus its first photo.
#[derive(Serialize, Debug, Clone)]
pub struct Card {
    #[serde(flatten)]
    pub card: ProductCard,
    /// The first photo's address (`/images/…`), if it has one.
    pub photo: Option<String>,
    /// Its text alternative.
    pub photo_alt: String,
}

impl Card {
    /// Cards for these products, in their order: four queries in all
    /// (brands, categories, variants, photos), however many products.
    pub async fn load(db: &Db, products: Vec<Product>) -> Result<Vec<Card>> {
        let mut photos: HashMap<i64, Vec<ProductPhoto>> = has_many(
            db,
            &products,
            ProductPhoto::query()
                .order_by(ProductPhoto::POSITION)
                .order_by(ProductPhoto::ID),
            "product_id",
            |p| p.product_id,
        )
        .await?;
        Ok(ProductCard::load(db, products)
            .await?
            .into_iter()
            .map(|card| {
                let first = photos
                    .remove(&card.product.id)
                    .and_then(|mut p| (!p.is_empty()).then(|| p.swap_remove(0)));
                Card {
                    photo: first.as_ref().map(|p| photo_url(&p.path)),
                    photo_alt: first.map(|p| p.alt).unwrap_or_default(),
                    card,
                }
            })
            .collect())
    }
}

/// A photo's address: a path under `public/` (`images/…`) as an absolute
/// path, or an address as it is.
pub fn photo_url(path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") || path.starts_with('/') {
        path.to_owned()
    } else {
        format!("/{path}")
    }
}

/// The categories, in a tree.
#[derive(Debug, Clone, Default)]
pub struct Tree {
    /// Every category, by their `position`.
    pub all: Vec<Category>,
}

impl Tree {
    /// Every category, in one query.
    pub async fn load(db: &Db) -> Result<Tree> {
        Ok(Tree {
            all: Category::query()
                .order_by(Category::POSITION)
                .order_by(Category::ID)
                .get(db)
                .await?,
        })
    }

    /// The top-level categories.
    pub fn roots(&self) -> Vec<&Category> {
        self.all.iter().filter(|c| c.parent_id.is_none()).collect()
    }

    /// The categories directly under `id`.
    pub fn children(&self, id: i64) -> Vec<&Category> {
        self.all
            .iter()
            .filter(|c| c.parent_id == Some(id))
            .collect()
    }

    /// `id` and every category under it, at any depth.
    pub fn with_descendants(&self, id: i64) -> Vec<i64> {
        let mut ids = vec![id];
        let mut i = 0;
        while i < ids.len() {
            let parent = ids[i];
            ids.extend(
                self.all
                    .iter()
                    .filter(|c| c.parent_id == Some(parent))
                    .map(|c| c.id),
            );
            i += 1;
        }
        ids
    }

    /// The path from the top to `id` (for the breadcrumbs).
    pub fn path_to(&self, id: i64) -> Vec<Category> {
        let mut path = Vec::new();
        let mut current = self.all.iter().find(|c| c.id == id);
        while let Some(category) = current {
            path.insert(0, category.clone());
            current = category
                .parent_id
                .and_then(|p| self.all.iter().find(|c| c.id == p));
        }
        path
    }
}

/// A link in the category menu.
#[derive(Serialize, Debug, Clone)]
pub struct CategoryLink {
    pub name: String,
    pub href: String,
    pub current: bool,
    pub children: Vec<CategoryLink>,
}

/// A filter's options as the kit's fields take them: `[value, label]`.
pub type Options = Vec<(String, String)>;

/// What the filter form offers for the products in scope.
#[derive(Serialize, Debug, Clone, Default)]
pub struct Facets {
    pub brands: Options,
    pub sizes: Options,
    pub wheels: Options,
    pub frames: Options,
    /// Whether any product in scope is an e-bike (the toggle shows then).
    pub ebikes: bool,
    /// The cheapest and dearest variant in scope, rounded out to the
    /// slider's step.
    pub price_low: i64,
    pub price_high: i64,
    pub price_step: i64,
    pub stores: Options,
    /// "Fits my bike": any of the customer's registered bikes (`mine`) or
    /// one of them; empty for visitors, customers without bikes, and
    /// outside part categories and search.
    pub fits: Options,
}

/// The step of the price slider: 10,000 in the smallest unit ($100).
pub const PRICE_STEP: i64 = 10_000;

/// What the listing shows: a category, a search, or everything.
pub enum Scope {
    /// Every product.
    All,
    /// A category and the ones under it.
    Category(Category),
    /// Search results (the words are in the filters).
    Search,
}

/// The customer of a logged-in user and their bikes' models, for "fits my
/// bike" (two queries; none for visitors).
pub async fn my_bikes(db: &Db, user: Option<&User>) -> Result<Vec<CustomerBike>> {
    let Some(user) = user else {
        return Ok(Vec::new());
    };
    let Some(customer) = Customer::of_user(db, user.id).await? else {
        return Ok(Vec::new());
    };
    CustomerBike::query()
        .where_(CustomerBike::CUSTOMER_ID.eq(customer.id))
        .where_(CustomerBike::PRODUCT_ID.is_not_null())
        .order_by(CustomerBike::ID)
        .get(db)
        .await
}

/// The bike models (product ids) the `fits` filter stands for.
pub fn fits_models(fits: Option<&str>, bikes: &[CustomerBike]) -> Vec<i64> {
    let Some(fits) = fits else {
        return Vec::new();
    };
    let mut ids: Vec<i64> = bikes
        .iter()
        .filter(|b| fits == "mine" || fits == b.id.to_string())
        .filter_map(|b| b.product_id)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// Builds a listing page (see the module docs).
pub async fn listing(
    db: &Db,
    lang: &Lang,
    user: Option<&User>,
    scope: Scope,
    path: &str,
    filters: Filters,
) -> Result<View> {
    let tree = Tree::load(db).await?;
    let (category_ids, category, kind) = match &scope {
        Scope::Category(c) => (
            Some(tree.with_descendants(c.id)),
            Some(c.clone()),
            Some(c.kind),
        ),
        _ => (None, None, None),
    };
    // "Fits my bike" belongs to parts: their categories and search.
    let fits_on = matches!(scope, Scope::Search) || kind == Some(CategoryKind::Part);
    let bikes = if fits_on {
        my_bikes(db, user).await?
    } else {
        Vec::new()
    };
    let fits = fits_models(filters.fits.as_deref(), &bikes);

    // [explain:catalog.listing]
    // What's in scope, before the filters: the facets come from it.
    let in_scope = || {
        Product::query()
            .when(category_ids.is_some(), |q| {
                q.where_(Product::CATEGORY_ID.is_in(category_ids.clone().unwrap_or_default()))
            })
            .where_search(&filters.q)
    };
    let page = filters
        .apply(in_scope(), db.dialect(), &fits)
        .paginate(db, filters.page, PER_PAGE)
        .await?;
    let total = page.total;
    let cards = Card::load(db, page.items.clone()).await?;
    // [/explain:catalog.listing]
    let facets = facets(db, lang, in_scope(), bikes).await?;
    // The slider sends both ends every time: an end at the scale's edge is
    // no filter (no chip, and links without it).
    let mut filters = filters;
    if filters.price_min.is_some_and(|v| v <= facets.price_low) {
        filters.price_min = None;
    }
    if filters.price_max.is_some_and(|v| v >= facets.price_high) {
        filters.price_max = None;
    }

    let chips = chips(lang, &filters, path, &facets);
    let menu = menu(&tree, category.as_ref().map(|c| c.id));
    let breadcrumbs: Vec<Category> = category
        .as_ref()
        .map(|c| tree.path_to(c.id))
        .unwrap_or_default();
    let sorts: Vec<(String, String)> = Sort::ALL
        .into_iter()
        .filter(|s| *s != Sort::Relevance || !filters.q.is_empty())
        .map(|s| {
            (
                s.key().to_owned(),
                lang.t(&format!("catalog.sort.{}", s.key()), &[]),
            )
        })
        .collect();
    let title = match &scope {
        Scope::All => lang.t("catalog.all.title", &[]),
        Scope::Category(c) => c.name.clone(),
        Scope::Search if filters.q.is_empty() => lang.t("catalog.search.title", &[]),
        Scope::Search => lang.t("catalog.search.results", &[("q", &filters.q)]),
    };
    let description = match &scope {
        Scope::Category(c) => lang.t("catalog.category.description", &[("name", &c.name)]),
        _ => lang.t("catalog.all.description", &[]),
    };
    let reset = path.to_owned();
    let paginated = page.map(|_| ());
    // [explain:catalog.listing]
    Ok(view(
        "catalog/index.html",
        context! {
            title,
            description,
            path,
            reset,
            search => matches!(scope, Scope::Search),
            category,
            breadcrumbs,
            category_menu => menu,
            filters,
            facets,
            chips,
            sorts,
            fits_on,
            cards,
            total,
            pages => paginated,
        },
    )
    .fragment("results"))
    // [/explain:catalog.listing]
}

/// The filter choices for the products in `scope` (six queries).
async fn facets(
    db: &Db,
    lang: &Lang,
    scope: renox::db::Query<Product>,
    my_bikes: Vec<CustomerBike>,
) -> Result<Facets> {
    let brands = Brand::query()
        .where_in_query("id", scope.clone(), "brand_id")
        .order_by(Brand::NAME)
        .get(db)
        .await?;
    let variants_in_scope =
        || ProductVariant::query().where_in_query("product_id", scope.clone(), "id");
    let sizes: BTreeSet<String> = variants_in_scope()
        .where_(ProductVariant::SIZE.is_not_null())
        .pluck::<String, _>(db, ProductVariant::SIZE)
        .await?
        .into_iter()
        .collect();
    let mut sizes: Vec<String> = sizes.into_iter().collect();
    sizes.sort_by_key(|s| size_rank(s));
    let low = variants_in_scope()
        .min::<i64, _>(db, "price")
        .await?
        .unwrap_or(0);
    let high = variants_in_scope()
        .max::<i64, _>(db, "price")
        .await?
        .unwrap_or(0);
    let specs: Vec<Json<std::collections::BTreeMap<String, String>>> =
        scope.clone().pluck(db, Product::SPECS).await?;
    let mut wheels = BTreeSet::new();
    let mut frames = BTreeSet::new();
    let mut ebikes = false;
    for spec in &specs {
        if let Some(w) = spec.0.get(SPEC_WHEELS) {
            wheels.insert(w.clone());
        }
        if let Some(f) = spec.0.get(SPEC_FRAME) {
            frames.insert(f.clone());
        }
        ebikes |= spec.0.contains_key(SPEC_MOTOR);
    }
    let pairs = |values: BTreeSet<String>| -> Options {
        values.into_iter().map(|v| (v.clone(), v)).collect()
    };
    let mut fits: Options = Vec::new();
    if !my_bikes.is_empty() {
        fits.push(("mine".into(), lang.t("catalog.fits.any", &[])));
        fits.extend(my_bikes.iter().map(|b| (b.id.to_string(), b.name.clone())));
    }
    Ok(Facets {
        brands: brands.into_iter().map(|b| (b.slug, b.name)).collect(),
        sizes: sizes.into_iter().map(|s| (s.clone(), s)).collect(),
        wheels: pairs(wheels),
        frames: pairs(frames),
        ebikes,
        price_low: low / PRICE_STEP * PRICE_STEP,
        price_high: (high + PRICE_STEP - 1) / PRICE_STEP * PRICE_STEP,
        price_step: PRICE_STEP,
        stores: Store::all_by_name(db)
            .await?
            .into_iter()
            .map(|s| (s.id.to_string(), s.name))
            .collect(),
        fits,
    })
}

/// Sizes in a sensible order: letters by size, then numbers, then the rest.
fn size_rank(size: &str) -> (u8, i64, String) {
    const LETTERS: [&str; 7] = ["XXS", "XS", "S", "M", "L", "XL", "XXL"];
    if let Some(i) = LETTERS.iter().position(|l| *l == size) {
        return (0, i as i64, String::new());
    }
    let digits: String = size.chars().take_while(|c| c.is_ascii_digit()).collect();
    match digits.parse::<i64>() {
        Ok(n) => (1, n, size.to_owned()),
        Err(_) => (2, 0, size.to_owned()),
    }
}

/// The chips of the filters that are on, each with a link without it.
fn chips(lang: &Lang, f: &Filters, path: &str, facets: &Facets) -> Vec<Chip> {
    let mut chips = Vec::new();
    let mut add = |label: String, skip: (&str, &str)| {
        chips.push(Chip {
            label,
            href: f.url(path, Some(skip)),
        })
    };
    for brand in &f.brands {
        let name = facets
            .brands
            .iter()
            .find(|(slug, _)| slug == brand)
            .map_or(brand.as_str(), |(_, name)| name.as_str());
        add(
            lang.t("catalog.chip.brand", &[("value", &name)]),
            ("brand", brand),
        );
    }
    for size in &f.sizes {
        add(
            lang.t("catalog.chip.size", &[("value", size)]),
            ("size", size),
        );
    }
    if f.price_min.is_some() || f.price_max.is_some() {
        add(lang.t("catalog.chip.price", &[]), ("price", ""));
    }
    if let Some(w) = &f.wheels {
        add(
            lang.t("catalog.chip.wheels", &[("value", w)]),
            ("wheels", w),
        );
    }
    if let Some(fr) = &f.frame {
        add(
            lang.t("catalog.chip.frame", &[("value", fr)]),
            ("frame", fr),
        );
    }
    if f.ebike {
        add(lang.t("catalog.chip.ebike", &[]), ("ebike", "1"));
    }
    if let Some(store) = f.store {
        let name = facets
            .stores
            .iter()
            .find(|(id, _)| *id == store.to_string())
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| store.to_string());
        add(
            lang.t("catalog.chip.store", &[("value", &name)]),
            ("store", &store.to_string()),
        );
    }
    if let Some(fits) = &f.fits {
        add(lang.t("catalog.chip.fits", &[]), ("fits", fits));
    }
    chips
}

/// The category menu: the top categories and, under the open one, theirs.
fn menu(tree: &Tree, current: Option<i64>) -> Vec<CategoryLink> {
    let open: Vec<i64> = current
        .map(|c| tree.path_to(c).iter().map(|c| c.id).collect())
        .unwrap_or_default();
    tree.roots()
        .into_iter()
        .map(|root| CategoryLink {
            name: root.name.clone(),
            href: format!("/shop/{}", root.slug),
            current: current == Some(root.id),
            children: tree
                .children(root.id)
                .into_iter()
                .filter(|_| open.contains(&root.id) || current.is_none())
                .map(|c| CategoryLink {
                    name: c.name.clone(),
                    href: format!("/shop/{}", c.slug),
                    current: current == Some(c.id),
                    children: Vec::new(),
                })
                .collect(),
        })
        .collect()
}

// [explain:catalog.index.handler]
/// `GET /shop` (`catalog.index`): every product.
pub async fn index(
    State(db): State<Db>,
    lang: Lang,
    user: Option<AuthUser>,
    Query(pairs): Query<Vec<(String, String)>>,
) -> Result<View> {
    let mut filters = Filters::from_pairs(&pairs);
    filters.q.clear();
    listing(&db, &lang, user.as_deref(), Scope::All, "/shop", filters).await
}
// [/explain:catalog.index.handler]

// [explain:catalog.category.handler]
/// `GET /shop/{slug}` (`catalog.category`): a category and the ones under it.
pub async fn category(
    State(db): State<Db>,
    lang: Lang,
    user: Option<AuthUser>,
    Found(category): Found<Category>,
    Query(pairs): Query<Vec<(String, String)>>,
) -> Result<View> {
    let mut filters = Filters::from_pairs(&pairs);
    filters.q.clear();
    let path = format!("/shop/{}", category.slug);
    listing(
        &db,
        &lang,
        user.as_deref(),
        Scope::Category(category),
        &path,
        filters,
    )
    .await
}
// [/explain:catalog.category.handler]

// [explain:catalog.search.handler]
/// `GET /search?q=` (`catalog.search`): search results, with the same filters.
pub async fn search(
    State(db): State<Db>,
    lang: Lang,
    user: Option<AuthUser>,
    Query(pairs): Query<Vec<(String, String)>>,
) -> Result<View> {
    let filters = Filters::from_pairs(&pairs);
    listing(
        &db,
        &lang,
        user.as_deref(),
        Scope::Search,
        "/search",
        filters,
    )
    .await
}
// [/explain:catalog.search.handler]

/// One suggestion under the navbar's search box.
#[derive(Serialize, Debug, Clone)]
pub struct Suggestion {
    pub name: String,
    pub href: String,
    pub price_from: Option<i64>,
    pub brand: Option<String>,
    pub photo: Option<String>,
}

/// How many suggestions the box shows.
pub const SUGGESTIONS: u64 = 6;

/// `GET /search/suggest?q=` (`catalog.suggest`): the best matches for what
/// is typed, as a small list (an htmx fragment under the navbar's box).
pub async fn suggest(
    State(db): State<Db>,
    Query(pairs): Query<Vec<(String, String)>>,
) -> Result<View> {
    let q: String = pairs
        .iter()
        .find(|(k, _)| k == "q")
        .map(|(_, v)| v.trim().chars().take(100).collect())
        .unwrap_or_default();
    // Two letters at least, or nothing is shown (the panel stays closed).
    let q = if q.chars().filter(|c| c.is_alphanumeric()).count() < 2 {
        String::new()
    } else {
        q
    };
    // [explain:catalog.search.suggest]
    let suggestions = if q.is_empty() {
        Vec::new()
    } else {
        let products = Product::search(&q)
            .order_by(Product::ID)
            .limit(SUGGESTIONS)
            .get(&db)
            .await?;
        Card::load(&db, products)
            .await?
            .into_iter()
            .map(|c| Suggestion {
                href: format!("/products/{}", c.card.product.slug),
                name: c.card.product.name.clone(),
                price_from: c.card.price_from,
                brand: c.card.brand.as_ref().map(|b| b.name.clone()),
                photo: c.photo,
            })
            .collect()
    };
    Ok(view("catalog/_suggest.html", context! { q, suggestions }))
    // [/explain:catalog.search.suggest]
}
