//! The data model (#232): migrations up and down, the search index files,
//! relations in a fixed number of queries, the sealed ID number, money and
//! enums as stored. Runs on SQLite, and on PostgreSQL with
//! `TEST_DATABASE_URL` set.

use bikeshop::app::accounts::model::Customer;
use bikeshop::app::catalog::model::{Product, ProductCard};
use bikeshop::app::rentals::model::{Rental, RentalRow};
use bikeshop::seed;
use renox::db::{Dialect, sql};
use renox::prelude::*;
use renox::testing::TestApp;
use std::path::PathBuf;

const SEARCH_MIGRATION: &str = "20260101000410_search_products";

fn migrations_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("migrations")
}

/// The search index's SQL files are what `renox::db::search::migration`
/// writes for `Product` (so they follow the model's `search = …`). Run with
/// `BIKESHOP_WRITE_SEARCH=1` to write them again after changing it.
#[test]
fn the_search_migration_files_match_the_model() {
    let migration = renox::db::search::migration::<Product>(SEARCH_MIGRATION);
    let files = [
        ("sqlite.up", migration.up_for(Dialect::Sqlite)),
        ("sqlite.down", migration.down_for(Dialect::Sqlite).unwrap()),
        ("postgres.up", migration.up_for(Dialect::Postgres)),
        (
            "postgres.down",
            migration.down_for(Dialect::Postgres).unwrap(),
        ),
    ];
    for (suffix, sql) in files {
        let path = migrations_dir().join(format!("{SEARCH_MIGRATION}.{suffix}.sql"));
        let expected = format!(
            "-- Written by renox::db::search::migration::<Product> (tests/data.rs checks it).\n{}\n",
            sql.trim()
        );
        if std::env::var("BIKESHOP_WRITE_SEARCH").is_ok() {
            std::fs::write(&path, &expected).unwrap();
        }
        let found = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(
            found.replace("\r\n", "\n"),
            expected,
            "{} is out of date: run `BIKESHOP_WRITE_SEARCH=1 cargo test -p bikeshop --test data`",
            path.display()
        );
    }
}

/// Every table the story lists, plus Renox's own.
const TABLES: &[&str] = &[
    "countries",
    "cities",
    "addresses",
    "stores",
    "staff",
    "staff_help_requests",
    "staff_help_hours",
    "customers",
    "categories",
    "brands",
    "products",
    "product_variants",
    "product_photos",
    "part_fits",
    "suppliers",
    "stock_levels",
    "stock_movements",
    "purchase_orders",
    "purchase_order_lines",
    "consignment_shipments",
    "consignment_shipment_lines",
    "rental_bikes",
    "bike_placements",
    "rentals",
    "orders",
    "order_items",
    "payments",
    "customer_bikes",
    "service_tasks",
    "work_orders",
    "work_order_tasks",
    "service_plans",
    "plan_tasks",
    "plan_subscriptions",
    "intercompany_entries",
    "settlements",
    "roles",
    "role_user",
    "field_samples",
];

async fn table_exists(db: &Db, table: &str) -> bool {
    sql(format!("SELECT COUNT(*) FROM {table}"))
        .scalar::<i64>(db)
        .await
        .is_ok()
}

#[renox::test]
async fn migrations_go_up_and_down() {
    let app = TestApp::new(bikeshop::app()).await;
    let db = app.db();
    for table in TABLES {
        assert!(table_exists(db, table).await, "{table} is missing");
    }
    // Every migration of the app rolls back cleanly…
    app.kernel().rollback(100).await.unwrap();
    for table in TABLES {
        assert!(!table_exists(db, table).await, "{table} is still there");
    }
    // …and comes back.
    app.kernel().migrate().await.unwrap();
    for table in TABLES {
        assert!(table_exists(db, table).await, "{table} is missing again");
    }
    seed::run(app.state().clone()).await.unwrap();
}

#[renox::test]
async fn the_id_number_is_unreadable_in_the_table() {
    let app = TestApp::new(bikeshop::app()).await;
    let db = app.db();
    let customer = Customer::create(
        db,
        Customer {
            name: "Ana Ruiz".into(),
            id_number: Some("X1234567L".to_owned().into()),
            active: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let raw: String = sql("SELECT id_number FROM customers WHERE id = ?")
        .bind(customer.id)
        .scalar(db)
        .await
        .unwrap();
    assert!(!raw.contains("X1234567L"), "stored in the clear: {raw}");
    assert!(!raw.contains("1234567"), "stored in the clear: {raw}");

    let back = Customer::find(db, customer.id).await.unwrap().unwrap();
    assert_eq!(
        back.id_number.as_deref().map(String::as_str),
        Some("X1234567L")
    );
    assert_eq!(back.masked_id_number().as_deref(), Some("••••••67L"));
    // Never serialized: a template or an API answer can't leak it.
    let json = renox::serde_json::to_string(&back).unwrap();
    assert!(
        !json.contains("X1234567L") && !json.contains("id_number"),
        "{json}"
    );
}

#[renox::test]
async fn money_and_enums_are_stored_as_integers_and_words() {
    let app = TestApp::new(bikeshop::app()).await;
    let db = app.db();
    seed::run(app.state().clone()).await.unwrap();
    let (price, status): (i64, String) =
        sql("SELECT price, status FROM rentals ORDER BY id LIMIT 1")
            .fetch_as::<(i64, String)>(db)
            .await
            .unwrap()
            .remove(0);
    assert!(price > 0);
    assert!(
        ["reserved", "active", "overdue", "returned", "cancelled"].contains(&status.as_str()),
        "{status}"
    );
    let code: String = sql("SELECT reservation_code FROM rentals ORDER BY id LIMIT 1")
        .scalar(db)
        .await
        .unwrap();
    assert_eq!(code.len(), 26, "a ULID: {code}");
}

#[renox::test]
async fn products_with_brand_category_and_variants_load_in_three_more_queries() {
    let app = TestApp::new(bikeshop::app()).await;
    let db = app.db().clone();
    seed::run(app.state().clone()).await.unwrap();

    let mut counts = Vec::new();
    for limit in [5, 40] {
        let products = Product::query()
            .order_by(Product::ID)
            .limit(limit)
            .get(&db)
            .await
            .unwrap();
        assert_eq!(products.len(), limit as usize);
        let (cards, queries) = renox::db::capture_queries(ProductCard::load(&db, products)).await;
        let cards = cards.unwrap();
        assert!(
            cards
                .iter()
                .all(|c| c.brand.is_some() && c.category.is_some())
        );
        assert!(
            cards
                .iter()
                .all(|c| !c.variants.is_empty() && c.price_from.is_some())
        );
        counts.push(queries.len());
    }
    assert_eq!(counts, [3, 3], "N+1?");
}

#[renox::test]
async fn rentals_with_bike_customer_and_stores_load_in_five_queries() {
    let app = TestApp::new(bikeshop::app()).await;
    let db = app.db().clone();
    seed::run(app.state().clone()).await.unwrap();

    let mut counts = Vec::new();
    for limit in [3, 30] {
        let rentals = Rental::query()
            .order_by(Rental::ID)
            .limit(limit)
            .get(&db)
            .await
            .unwrap();
        assert_eq!(rentals.len(), limit as usize);
        let (rows, queries) = renox::db::capture_queries(RentalRow::load(&db, rentals)).await;
        let rows = rows.unwrap();
        assert!(rows.iter().all(|r| r.bike.is_some()
            && r.model.is_some()
            && r.customer.is_some()
            && r.operating_store.is_some()
            && r.owner_store.is_some()));
        counts.push(queries.len());
    }
    assert_eq!(counts, [5, 5], "N+1?");
}

#[renox::test]
async fn search_finds_products_by_name_brand_and_sku() {
    let app = TestApp::new(bikeshop::app()).await;
    let db = app.db();
    seed::run(app.state().clone()).await.unwrap();
    let product = Product::query()
        .order_by(Product::ID)
        .first(db)
        .await
        .unwrap()
        .unwrap();
    let first_word = product.name.split_whitespace().next().unwrap().to_owned();
    let found = Product::search(&first_word).get(db).await.unwrap();
    assert!(found.iter().any(|p| p.id == product.id), "{first_word}");

    let sku: String = sql("SELECT sku FROM product_variants WHERE product_id = ? ORDER BY id")
        .bind(product.id)
        .scalar(db)
        .await
        .unwrap();
    let by_sku = Product::search(&sku).get(db).await.unwrap();
    assert!(by_sku.iter().any(|p| p.id == product.id), "{sku}");
}

#[renox::test]
async fn customers_work_orders_load_through_their_bikes_in_two_queries() {
    let app = TestApp::new(bikeshop::app()).await;
    let db = app.db().clone();
    seed::run(app.state().clone()).await.unwrap();
    let customers = Customer::query()
        .order_by(Customer::ID)
        .limit(30)
        .get(&db)
        .await
        .unwrap();
    let (found, queries) = renox::db::capture_queries(
        bikeshop::app::workshop::model::work_orders_of(&db, &customers),
    )
    .await;
    let found = found.unwrap();
    assert_eq!(queries.len(), 2, "{queries:#?}");
    assert!(found.values().any(|orders| !orders.is_empty()));
    let total: usize = found.values().map(Vec::len).sum();
    let expected: i64 = sql(
        "SELECT COUNT(*) FROM work_orders w JOIN customer_bikes b ON b.id = w.customer_bike_id \
         WHERE b.customer_id IN (SELECT id FROM customers WHERE deleted_at IS NULL ORDER BY id LIMIT 30)",
    )
    .scalar(&db)
    .await
    .unwrap();
    assert_eq!(total as i64, expected);
}

#[renox::test]
async fn the_data_page_explains_the_model_with_live_counts() {
    let app = TestApp::new(bikeshop::app()).await;
    seed::run(app.state().clone()).await.unwrap();
    let rentals: i64 = sql("SELECT COUNT(*) FROM rentals")
        .scalar(app.db())
        .await
        .unwrap();
    let page = app.get("/about/data").await;
    page.assert_ok()
        .assert_view("about/data.html")
        .assert_see("The data model")
        .assert_see("<code>part_fits</code>")
        .assert_see("<code>intercompany_entries</code>")
        .assert_see("Owner, location and operating store")
        .assert_see("waiting_parts")
        .assert_see("rentals.checkout")
        .assert_see(&renox::format_number(rentals as f64, 0, "en"))
        // The panel explains the page too.
        .assert_see("id=\"about-page\"");
    for area in bikeshop::app::about::data::AREAS {
        for table in area.tables {
            page.assert_see(&format!("<code>{}</code>", table.name));
        }
    }
}
