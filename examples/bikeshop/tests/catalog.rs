//! The public catalogue (#233): filters, sort and pagination together,
//! search ranking, "fits my bike", 404s for unknown and discontinued
//! products, the product page (stock per store, the variant picker, recently
//! viewed), the sitemap, ETags, and a fixed number of queries on the large
//! seed.

use std::collections::BTreeMap;

use bikeshop::app::accounts::model::Customer;
use bikeshop::app::catalog::model::{
    Brand, Category, CategoryKind, PART_FITS, Product, ProductPhoto, ProductVariant,
};
use bikeshop::app::stock::model::StockLevel;
use bikeshop::app::workshop::model::CustomerBike;
use bikeshop::seed::{self, Volume, fixtures};
use renox::db::Json;
use renox::prelude::*;
use renox::testing::TestApp;

/// A tiny catalogue whose answers are known.
struct Shop {
    north: i64,
    south: i64,
    domane: Product,
    escape: Product,
    explore: Product,
    chain: Product,
    pedals: Product,
    old: Product,
    domane_m: ProductVariant,
}

async fn category(
    db: &Db,
    name: &str,
    slug: &str,
    kind: CategoryKind,
    parent: Option<i64>,
) -> Category {
    Category::create(
        db,
        Category {
            name: name.into(),
            slug: slug.into(),
            kind,
            parent_id: parent,
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

async fn brand(db: &Db, name: &str) -> Brand {
    Brand::create(
        db,
        Brand {
            name: name.into(),
            slug: name.to_lowercase(),
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

#[allow(clippy::too_many_arguments)]
async fn product(
    db: &Db,
    name: &str,
    category: i64,
    brand: i64,
    description: &str,
    specs: &[(&str, &str)],
    variants: &[(&str, i64)],
    days_old: i64,
) -> (Product, Vec<ProductVariant>) {
    let mut p = Product::create(
        db,
        Product {
            category_id: category,
            brand_id: brand,
            name: name.into(),
            slug: bikeshop::seed::shop::slugify(name),
            description: description.into(),
            specs: Json(
                specs
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                    .collect::<BTreeMap<_, _>>(),
            ),
            created_at: Some(renox::db::now() - renox::chrono::Duration::days(days_old)),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let mut made = Vec::new();
    for (i, (size, price)) in variants.iter().enumerate() {
        made.push(
            ProductVariant::create(
                db,
                ProductVariant {
                    product_id: p.id,
                    sku: format!("{}-{}", p.slug.to_uppercase(), i + 1),
                    size: Some((*size).into()),
                    colour: Some("Black".into()),
                    price: *price,
                    cost: price * 6 / 10,
                    ..Default::default()
                },
            )
            .await
            .unwrap(),
        );
    }
    ProductPhoto::create(
        db,
        ProductPhoto {
            product_id: p.id,
            path: "images/categories/road-bikes.svg".into(),
            alt: name.into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    p.refresh_keywords(db).await.unwrap();
    (p, made)
}

async fn stock(db: &Db, variant: i64, store: i64, on_hand: i64) {
    StockLevel::create(
        db,
        StockLevel {
            variant_id: variant,
            owner_store_id: store,
            location_store_id: store,
            on_hand,
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

async fn shop(app: &TestApp) -> Shop {
    let db = app.db();
    let north = fixtures::store(db, "North").await.unwrap().id;
    let south = fixtures::store(db, "South").await.unwrap().id;
    let bikes = category(db, "Bikes", "bikes", CategoryKind::Bike, None).await;
    let road = category(
        db,
        "Road bikes",
        "road-bikes",
        CategoryKind::Bike,
        Some(bikes.id),
    )
    .await;
    let parts = category(db, "Parts", "parts", CategoryKind::Part, None).await;
    let chains = category(db, "Chains", "chains", CategoryKind::Part, Some(parts.id)).await;
    let trek = brand(db, "Trek").await;
    let giant = brand(db, "Giant").await;
    let (domane, dv) = product(
        db,
        "Trek Domane",
        road.id,
        trek.id,
        "A light road bike for long days.",
        &[("Frame", "Carbon"), ("Wheels", "700c")],
        &[("S", 100_000), ("M", 120_000)],
        30,
    )
    .await;
    let (escape, ev) = product(
        db,
        "Giant Escape",
        road.id,
        giant.id,
        "An easy bike for the city.",
        &[("Frame", "Aluminium"), ("Wheels", "700c")],
        &[("M", 60_000)],
        5,
    )
    .await;
    let (explore, xv) = product(
        db,
        "Giant Explore E+",
        bikes.id,
        giant.id,
        "An e-bike with a strong motor.",
        &[("Frame", "Aluminium"), ("Motor", "Shimano EP8")],
        &[("M", 300_000)],
        1,
    )
    .await;
    let (chain, cv) = product(
        db,
        "Shimano Chain",
        chains.id,
        giant.id,
        "Twelve speeds.",
        &[("Speeds", "12")],
        &[("One size", 4_500)],
        10,
    )
    .await;
    let (pedals, _) = product(
        db,
        "Pedal kit",
        chains.id,
        trek.id,
        "Pedals, and a spare chain link for your chain.",
        &[],
        &[("One size", 3_000)],
        10,
    )
    .await;
    let (mut old, _) = product(
        db,
        "Trek Oldtimer",
        road.id,
        trek.id,
        "Not sold any more.",
        &[],
        &[("M", 50_000)],
        900,
    )
    .await;
    old.delete(db).await.unwrap();
    stock(db, dv[1].id, north, 2).await;
    stock(db, ev[0].id, south, 3).await;
    stock(db, xv[0].id, south, 1).await;
    stock(db, cv[0].id, north, 10).await;
    PART_FITS.attach(db, chain.id, [domane.id]).await.unwrap();
    PART_FITS.attach(db, pedals.id, [escape.id]).await.unwrap();
    Shop {
        north,
        south,
        domane_m: dv[1].clone(),
        domane,
        escape,
        explore,
        chain,
        pedals,
        old,
    }
}

/// The product names on a listing page, in order.
fn names(html: &str) -> Vec<String> {
    html.split("class=\"rx-media-card__title\">")
        .skip(1)
        .map(|rest| rest.split('<').next().unwrap_or("").to_owned())
        .collect()
}

#[renox::test]
async fn filters_and_sort_combine() {
    let app = TestApp::new(bikeshop::app()).await;
    let s = shop(&app).await;

    // The whole "Bikes" tree, most popular (none sold: by id).
    let all = app.get("/shop/bikes").await.assert_ok().text();
    assert_eq!(
        names(&all),
        ["Trek Domane", "Giant Escape", "Giant Explore E+"]
    );
    assert!(
        !all.contains("Trek Oldtimer"),
        "discontinued products are hidden"
    );

    let cheapest = names(&app.get("/shop/bikes?sort=price_asc").await.text());
    assert_eq!(
        cheapest,
        ["Giant Escape", "Trek Domane", "Giant Explore E+"]
    );
    let newest = names(&app.get("/shop/bikes?sort=newest").await.text());
    assert_eq!(newest, ["Giant Explore E+", "Giant Escape", "Trek Domane"]);

    let giant = names(
        &app.get("/shop/bikes?brand=giant&sort=price_desc")
            .await
            .text(),
    );
    assert_eq!(giant, ["Giant Explore E+", "Giant Escape"]);
    let two = names(
        &app.get("/shop/bikes?brand=giant&brand=trek&price_max=110000")
            .await
            .text(),
    );
    assert_eq!(
        two,
        ["Trek Domane", "Giant Escape"],
        "a variant in range is enough"
    );
    assert_eq!(
        names(&app.get("/shop/bikes?size=S").await.text()),
        ["Trek Domane"]
    );
    assert_eq!(
        names(&app.get("/shop/bikes?frame=Carbon").await.text()),
        ["Trek Domane"]
    );
    assert_eq!(
        names(&app.get("/shop/bikes?wheels=700c&brand=giant").await.text()),
        ["Giant Escape"]
    );
    assert_eq!(
        names(&app.get("/shop/bikes?ebike=1").await.text()),
        ["Giant Explore E+"]
    );
    let north = names(
        &app.get(&format!("/shop/bikes?store={}", s.north))
            .await
            .text(),
    );
    assert_eq!(north, ["Trek Domane"]);
    let south = names(
        &app.get(&format!("/shop/bikes?store={}&sort=price_asc", s.south))
            .await
            .text(),
    );
    assert_eq!(south, ["Giant Escape", "Giant Explore E+"]);
    // Nothing left: the empty state, with a way back.
    let none = app.get("/shop/bikes?brand=giant&size=S").await;
    none.assert_ok()
        .assert_see("Nothing matches")
        .assert_see("Clear filters");
    // A sub-category keeps its own products only.
    assert_eq!(
        names(&app.get("/shop/road-bikes?sort=price_asc").await.text()),
        ["Giant Escape", "Trek Domane"]
    );
    // The chips name what's on and link to the page without it.
    let chips = app.get("/shop/bikes?brand=giant&ebike=1").await.text();
    assert!(chips.contains("Brand: Giant"));
    assert!(
        chips.contains("href=\"/shop/bikes?brand=giant\""),
        "the e-bike chip drops only itself"
    );
}

#[renox::test]
async fn pages_keep_the_filters_and_never_overlap() {
    let app = TestApp::new(bikeshop::app()).await;
    let s = shop(&app).await;
    let db = app.db();
    let road = Category::query()
        .where_(Category::SLUG.eq("road-bikes"))
        .first(db)
        .await
        .unwrap()
        .unwrap();
    for n in 0..30 {
        product(
            db,
            &format!("Giant Road {n:02}"),
            road.id,
            s.escape.brand_id,
            "Another road bike.",
            &[("Frame", "Steel")],
            &[("M", 10_000 + n * 100)],
            40,
        )
        .await;
    }
    let first = app
        .get("/shop/bikes?brand=giant&sort=price_asc")
        .await
        .text();
    let second = app
        .get("/shop/bikes?brand=giant&sort=price_asc&page=2")
        .await
        .text();
    let (a, b) = (names(&first), names(&second));
    assert_eq!(a.len(), 24);
    assert_eq!(b.len(), 32 - 24, "30 + Escape + Explore");
    assert!(a.iter().all(|n| !b.contains(n)), "no product on both pages");
    assert_eq!(a[0], "Giant Road 00");
    assert_eq!(b.last().unwrap(), "Giant Explore E+");
    assert!(
        first.contains("brand=giant") && first.contains("page=2"),
        "the next page's link keeps the filters"
    );
}

#[renox::test]
async fn htmx_gets_only_the_results() {
    let app = TestApp::new(bikeshop::app()).await;
    shop(&app).await;
    let res = app.htmx().get("/shop/bikes?brand=trek").await;
    let html = res.assert_ok().text();
    assert!(
        html.trim_start().starts_with("<section id=\"results\""),
        "{html}"
    );
    assert!(!html.contains("<html"));
    assert_eq!(names(&html), ["Trek Domane"]);
}

#[renox::test]
async fn both_ends_of_the_price_range_apply_to_the_same_variant() {
    let app = TestApp::new(bikeshop::app()).await;
    shop(&app).await;
    // The Domane comes at 10,000,000 and 12,000,000: neither is between
    // 11,000,000 and 11,500,000, though one is above the low end and the
    // other below the high end.
    assert!(
        names(
            &app.get("/shop/bikes?price_min=110000&price_max=115000")
                .await
                .assert_ok()
                .text()
        )
        .is_empty()
    );
    assert_eq!(
        names(
            &app.get("/shop/bikes?price_min=110000&price_max=125000")
                .await
                .text()
        ),
        ["Trek Domane"]
    );
}

#[renox::test]
async fn a_search_filters_once() {
    let app = TestApp::new(bikeshop::app()).await;
    shop(&app).await;
    for sort in ["relevance", "newest", "price_asc"] {
        let (res, queries) =
            renox::db::capture_queries(app.get(&format!("/search?q=chain&sort={sort}"))).await;
        res.assert_ok();
        let count = queries
            .iter()
            .find(|q| q.contains("COUNT(") && q.contains("FROM \"products\""))
            .unwrap_or_else(|| panic!("the listing's count query in {queries:#?}"));
        let conditions = count.matches(" MATCH ").count() + count.matches(" @@ ").count();
        assert_eq!(conditions, 1, "{sort}: {count}");
    }
}

#[renox::test]
async fn search_ranks_name_matches_first_and_keeps_the_filters() {
    let app = TestApp::new(bikeshop::app()).await;
    let s = shop(&app).await;
    let found = names(&app.get("/search?q=chain").await.assert_ok().text());
    assert_eq!(
        found,
        ["Shimano Chain", "Pedal kit"],
        "the name weighs most"
    );
    // By brand and by SKU (the keywords).
    assert!(names(&app.get("/search?q=trek").await.text()).contains(&"Trek Domane".to_owned()));
    let sku = s.domane_m.sku.clone();
    assert_eq!(
        names(&app.get(&format!("/search?q={sku}")).await.text()),
        ["Trek Domane"]
    );
    // The same filters: Trek only.
    assert_eq!(
        names(&app.get("/search?q=chain&brand=trek").await.text()),
        ["Pedal kit"]
    );
    // Discontinued products aren't found.
    assert!(names(&app.get("/search?q=oldtimer").await.text()).is_empty());
    // The navbar's suggestions: links to the products.
    let suggest = app
        .htmx()
        .get("/search/suggest?q=giant")
        .await
        .assert_ok()
        .text();
    assert!(suggest.contains(&format!("/products/{}", s.escape.slug)));
    assert!(
        suggest.contains("/search?q=giant"),
        "and a link to all the results"
    );
    assert_eq!(
        app.htmx().get("/search/suggest?q=g").await.text().trim(),
        ""
    );
}

#[renox::test]
async fn fits_my_bike_shows_only_matching_parts() {
    let app = TestApp::new(bikeshop::app()).await;
    let s = shop(&app).await;
    let db = app.db();
    let user = User::register(db, "Rider", "rider@example.com", "password123")
        .await
        .unwrap();
    let customer = Customer::create(
        db,
        Customer {
            user_id: Some(user.id),
            name: "Rider".into(),
            email: Some("rider@example.com".into()),
            active: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let bike = CustomerBike::create(
        db,
        CustomerBike {
            customer_id: customer.id,
            product_id: Some(s.domane.id),
            name: "My Domane".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Visitors get no such filter, and `fits` alone keeps nothing.
    let guest = app.get("/shop/parts").await.text();
    assert!(!guest.contains("name=\"fits\""));
    assert!(names(&app.get("/shop/parts?fits=mine").await.text()).is_empty());

    app.acting_as(&user);
    let page = app.get("/shop/parts").await.text();
    assert!(page.contains("name=\"fits\"") && page.contains("My Domane"));
    assert_eq!(names(&page).len(), 2);
    assert_eq!(
        names(&app.get("/shop/parts?fits=mine").await.text()),
        ["Shimano Chain"]
    );
    let one = app
        .get(&format!("/shop/parts?fits={}", bike.id))
        .await
        .text();
    assert_eq!(names(&one), ["Shimano Chain"]);
    assert_eq!(
        names(&app.get("/search?q=chain&fits=mine").await.text()),
        ["Shimano Chain"]
    );
    // Bikes' categories don't offer it.
    assert!(
        !app.get("/shop/bikes")
            .await
            .text()
            .contains("name=\"fits\"")
    );
    let _ = s.pedals;
}

#[renox::test]
async fn unknown_and_discontinued_products_answer_404() {
    let app = TestApp::new(bikeshop::app()).await;
    let s = shop(&app).await;
    app.get(&format!("/products/{}", s.old.slug))
        .await
        .assert_not_found();
    app.get("/products/no-such-bike").await.assert_not_found();
    app.get("/shop/no-such-category").await.assert_not_found();
    app.get(&format!("/products/{}", s.domane.slug))
        .await
        .assert_ok();
}

#[renox::test]
async fn the_product_page_shows_stock_fits_and_remembers_the_visit() {
    let app = TestApp::new(bikeshop::app()).await;
    let s = shop(&app).await;
    let page = app
        .get(&format!("/products/{}?size=M", s.domane.slug))
        .await;
    page.assert_ok()
        .assert_see("$1,200.00")
        .assert_see("2 at North")
        .assert_see("Out of stock at South")
        .assert_see("Parts and accessories that fit")
        .assert_see("Shimano Chain")
        .assert_see("Add to cart")
        .assert_see("og:image");
    // The variant picker with htmx: only the buy box, for size S.
    let buybox = app
        .htmx()
        .get(&format!("/products/{}?size=S&colour=Black", s.domane.slug))
        .await
        .assert_ok()
        .text();
    assert!(
        buybox.trim_start().starts_with("<section id=\"buybox\""),
        "{buybox}"
    );
    assert!(buybox.contains("$1,000.00") && buybox.contains("Sold out in every store"));
    // A part lists the bikes it fits, with the pivot's note.
    app.get(&format!("/products/{}", s.chain.slug))
        .await
        .assert_see("Fits these bikes")
        .assert_see("Trek Domane");
    // Recently viewed: the session keeps the visits, newest first.
    let recent: Vec<i64> = app.session_get("recently_viewed").unwrap();
    assert_eq!(recent[..2], [s.chain.id, s.domane.id]);
    app.get(&format!("/products/{}", s.escape.slug))
        .await
        .assert_see("Recently viewed")
        .assert_see("Shimano Chain");
    let _ = (s.north, s.south, s.explore);
}

#[renox::test]
async fn the_sitemap_lists_categories_and_products_still_sold() {
    let app = TestApp::new(bikeshop::app()).await;
    let s = shop(&app).await;
    let xml = app.get("/sitemap.xml").await.assert_ok().text();
    assert!(xml.contains("/shop/road-bikes"));
    assert!(xml.contains(&format!("/products/{}", s.domane.slug)));
    assert!(
        !xml.contains(&s.old.slug),
        "discontinued products aren't listed"
    );
}

// [explain:sitemap.test]
#[renox::test]
async fn catalogue_routes_carry_etags_and_the_sitemap_answers_304() {
    let app = TestApp::new(bikeshop::app()).await;
    let s = shop(&app).await;
    for uri in ["/shop/bikes", &format!("/products/{}", s.escape.slug), "/"] {
        let res = app.get(uri).await;
        assert!(
            res.assert_ok().header("etag").is_some(),
            "{uri} has an ETag"
        );
    }
    // The sitemap is the same bytes every time: a 304 the second time.
    let first = app.get("/sitemap.xml").await;
    let etag = first
        .assert_ok()
        .header("etag")
        .expect("an ETag")
        .to_owned();
    app.request()
        .header("if-none-match", &etag)
        .get("/sitemap.xml")
        .await
        .assert_status(304);
}
// [/explain:sitemap.test]

#[renox::test]
async fn the_home_page_features_bikes_and_categories() {
    let app = TestApp::new(bikeshop::app()).await;
    shop(&app).await;
    app.get("/")
        .await
        .assert_ok()
        .assert_see("Popular <em>this week</em>")
        .assert_see("Trek Domane")
        .assert_see("Shop by <em>ride</em>")
        .assert_see("Road bikes")
        // The products and the category tiles show photos (#328).
        .assert_see("/images/products/road-bikes-");
}

/// The queries a GET runs.
async fn queries(app: &TestApp, uri: &str) -> usize {
    let (res, queries) = renox::db::capture_queries(app.get(uri)).await;
    res.assert_ok();
    queries.len()
}

#[renox::test]
async fn catalogue_pages_use_a_fixed_number_of_queries_on_the_large_seed() {
    let app = TestApp::new(bikeshop::app()).await;
    seed::shop::build(app.db(), Volume::large()).await.unwrap();
    let db = app.db();
    let bikes = queries(&app, "/shop/bikes").await;
    // Another page, other filters, another category: the same count.
    assert_eq!(
        queries(&app, "/shop/bikes?page=2&sort=price_asc").await,
        bikes
    );
    assert_eq!(queries(&app, "/shop/helmets?size=M").await, bikes);
    // Without a category there is no `Found<Category>` query.
    let all = queries(&app, "/shop").await;
    assert_eq!(all, bikes - 1);
    assert_eq!(
        queries(&app, "/shop?page=2&sort=newest&brand=trek").await,
        all
    );
    let found = app.get("/search?q=trek").await.text();
    assert!(
        names(&found).len() > 3,
        "{}",
        &found[found.find("results-count").unwrap_or(0)..][..200]
    );
    assert_eq!(queries(&app, "/search?q=trek").await, all);
    assert!(bikes <= 20, "{bikes} queries for a listing page");
    // Product pages: a bike with many fitting parts and a part, the same.
    let bike = Product::query()
        .where_in_query(
            "category_id",
            Category::query().where_(Category::KIND.eq(CategoryKind::Bike)),
            "id",
        )
        .order_by(Product::ID)
        .first(db)
        .await
        .unwrap()
        .unwrap();
    let part = Product::query()
        .where_in_query(
            "category_id",
            Category::query().where_(Category::KIND.eq(CategoryKind::Part)),
            "id",
        )
        .order_by(Product::ID)
        .first(db)
        .await
        .unwrap()
        .unwrap();
    let one = queries(&app, &format!("/products/{}", bike.slug)).await;
    let other = queries(&app, &format!("/products/{}", part.slug)).await;
    assert!(
        one <= 25 && other <= 25,
        "{one} and {other} queries for a product page"
    );
    // The home page.
    assert!(queries(&app, "/").await <= 15);
}

#[renox::test]
async fn each_store_has_its_own_page_on_its_own_host() {
    let app = TestApp::new(bikeshop::app()).await;
    let north = fixtures::store(app.db(), "North").await.unwrap();
    let host = format!("{}.localhost:3000", north.slug);
    let on = |host: String, path: &'static str| {
        let app = &app;
        async move { app.request().header("host", &host).get(path).await }
    };

    // Anyone: the store's name, address and hours, links back to the shop.
    let page = on(host.clone(), "/").await;
    page.assert_ok()
        .assert_view("home/store.html")
        .assert_see("The North store")
        .assert_see("Main Street")
        .assert_see("Opening hours")
        // Links to the shop are absolute: this host has only this page.
        .assert_see(r#"href="http://127.0.0.1:3000/""#)
        .assert_see(r#"href="http://127.0.0.1:3000/rent""#)
        // Its own "About this page", not the home page's.
        .assert_see("Store page on its own host")
        // whose link to every page goes to the shop's host.
        .assert_see(r#"href="http://127.0.0.1:3000/about/pages""#);

    // Any other path on a store's host goes to its page; an unknown store is a 404.
    on(host.clone(), "/shop").await.assert_redirect("/");
    on("nowhere.localhost".into(), "/").await.assert_not_found();

    // The shop itself stays on its own host, and its home page links to each store's.
    app.get("/")
        .await
        .assert_view("home/index.html")
        .assert_see(&format!(r#"href="http://{host}/""#));
}

#[renox::test]
async fn the_models_match_the_schema() {
    TestApp::new(bikeshop::app())
        .await
        .assert_models_match_schema()
        .await;
}
