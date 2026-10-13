use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use renox::db::{DbValue, Dialect, Migration};
use renox::prelude::*;
use renox::{Kernel, fake::Fake};
use serde::Serialize;
use tower::ServiceExt;

#[derive(Model, Serialize, Default, Debug, Clone, PartialEq)]
#[model(table = "products", soft_deletes)]
struct Product {
    id: i64,
    name: String,
    price: i64,
    category: Option<String>,
    #[model(skip)]
    label: String,
    created_at: Option<DateTime>,
    updated_at: Option<DateTime>,
    deleted_at: Option<DateTime>,
}

impl Factory for Product {
    fn definition() -> Self {
        Product {
            name: (5..12).fake::<String>(),
            price: (1_000..50_000).fake(),
            ..Default::default()
        }
    }
}

#[derive(Model, Serialize, Default, Debug)]
#[model(table = "notes")]
struct Note {
    id: i64,
    product_id: Option<i64>,
    body: String,
}

fn product(name: &str, price: i64, category: Option<&str>) -> Product {
    Product {
        name: name.into(),
        price,
        category: category.map(Into::into),
        ..Default::default()
    }
}

fn config(views: &std::path::Path) -> Config {
    {
        let mut c = Config::default();
        c.env = Environment::Testing;
        c.key = Some(renox::generate_key());
        c.views_path = views.to_path_buf();
        c
    }
}

async fn kernel_with(app: impl FnOnce(App) -> App) -> (Kernel, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let kernel = app(
        App::with_config(config(dir.path())).migrations(renox::migrations!("tests/migrations"))
    )
    .boot()
    .await
    .unwrap();
    kernel.migrate().await.unwrap();
    (kernel, dir)
}

async fn kernel() -> (Kernel, tempfile::TempDir) {
    kernel_with(|app| app).await
}

#[test]
fn migrations_macro_embeds_files_in_order() {
    let migrations: &[Migration] = renox::migrations!("tests/migrations");
    let names: Vec<_> = migrations.iter().map(|m| m.name()).collect();
    assert_eq!(
        names,
        [
            "20260101000000_create_products",
            "20260102000000_create_notes"
        ]
    );
    assert!(
        migrations[0]
            .up_for(Dialect::Sqlite)
            .contains("CREATE TABLE products")
    );
    // `NAME.postgres.up.sql` replaces the plain file on PostgreSQL only; the
    // plain `.down.sql` still serves both.
    assert!(
        migrations[0]
            .up_for(Dialect::Sqlite)
            .contains("AUTOINCREMENT")
    );
    assert!(migrations[0].up_for(Dialect::Postgres).contains("IDENTITY"));
    assert_eq!(
        migrations[0].down_for(Dialect::Postgres),
        migrations[0].down_for(Dialect::Sqlite)
    );
    // Trimmed: Git may check files out with CRLF line endings on Windows.
    assert_eq!(
        migrations[0].down_for(Dialect::Sqlite).map(str::trim),
        Some("DROP TABLE products;")
    );
    assert!(
        renox::migrations!("tests/migrations_plain")[0]
            .down_for(Dialect::Sqlite)
            .is_none()
    );
    assert!(renox::migrations!("tests/does_not_exist").is_empty());
}

#[tokio::test]
async fn migrations_run_in_batches_and_roll_back() {
    let (kernel, _dir) = kernel().await;
    assert!(
        kernel.migrate().await.unwrap().is_empty(),
        "nothing left to run"
    );
    let status = kernel.migration_status().await.unwrap();
    assert!(status.iter().all(|m| m.batch == Some(1)));

    let rolled = kernel.rollback(1).await.unwrap();
    // Newest first; every app also gets the framework's jobs, cache,
    // sessions, grid_preferences and webhook_calls tables.
    assert_eq!(
        rolled,
        [
            "20260102000000_create_notes",
            "20260101000000_create_products",
            "00010101000301_store_webhook_payloads_as_bytes",
            "00010101000300_create_webhook_calls_table",
            "00010101000220_create_grid_preferences_table",
            "00010101000210_create_sessions_table",
            "00010101000200_create_cache_table",
            "00010101000120_add_callback_of_to_jobs",
            "00010101000110_add_chains_and_batches_to_jobs",
            "00010101000100_create_jobs_table"
        ]
    );
    assert!(Product::all(kernel.db()).await.is_err(), "table is gone");

    assert_eq!(kernel.migrate().await.unwrap().len(), 10);
    Product::create(kernel.db(), product("Coffee", 1, None))
        .await
        .unwrap();
    assert_eq!(kernel.fresh().await.unwrap().len(), 10);
    assert!(
        Product::all(kernel.db()).await.unwrap().is_empty(),
        "fresh drops data"
    );
}

#[tokio::test]
async fn migrations_without_down_cannot_roll_back() {
    let dir = tempfile::tempdir().unwrap();
    let kernel = App::with_config(config(dir.path()))
        .migrations(renox::migrations!("tests/migrations_plain"))
        .boot()
        .await
        .unwrap();
    kernel.migrate().await.unwrap();
    let err = kernel.rollback(1).await.unwrap_err();
    assert!(format!("{err:?}").contains("has no .down.sql"));
}

#[tokio::test]
async fn duplicate_migrations_fail_at_boot() {
    let dir = tempfile::tempdir().unwrap();
    let result = App::with_config(config(dir.path()))
        .migrations(renox::migrations!("tests/migrations"))
        .migrations(renox::migrations!("tests/migrations"))
        .boot()
        .await;
    assert!(format!("{:?}", result.err().unwrap()).contains("registered twice"));
}

#[tokio::test]
async fn file_databases_use_wal_and_create_their_directory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("storage/app.db");
    let config = {
        let mut c = config(dir.path());
        c.database_url = format!("sqlite://{}", path.display());
        c
    };
    let kernel = App::with_config(config).boot().await.unwrap();
    let mode: String = renox::db::sql("PRAGMA journal_mode")
        .scalar(kernel.db())
        .await
        .unwrap();
    assert_eq!(mode, "wal");
    assert!(path.exists());
}

#[tokio::test]
async fn save_inserts_then_updates_with_timestamps() {
    let (kernel, _dir) = kernel().await;
    let db = kernel.db();

    let mut coffee = product("Coffee", 18_000, Some("drinks"));
    coffee.save(db).await.unwrap();
    assert!(coffee.id > 0);
    let created = coffee.created_at.unwrap();
    assert_eq!(coffee.updated_at, Some(created));

    coffee.price = 20_000;
    coffee.label = "not stored".into();
    coffee.save(db).await.unwrap();
    assert_eq!(coffee.created_at, Some(created), "created_at is kept");

    let found = Product::find(db, coffee.id).await.unwrap().unwrap();
    assert_eq!(found.price, 20_000);
    assert_eq!(found.label, "", "skipped fields load as Default");
    assert_eq!(found.created_at, Some(created));
    assert!(Product::find(db, 999).await.unwrap().is_none());
}

#[tokio::test]
async fn soft_deletes_hide_rows_until_restored() {
    let (kernel, _dir) = kernel().await;
    let db = kernel.db();
    let mut coffee = Product::create(db, product("Coffee", 1, None))
        .await
        .unwrap();
    Product::create(db, product("Tea", 2, None)).await.unwrap();

    coffee.delete(db).await.unwrap();
    assert!(coffee.deleted_at.is_some());
    assert_eq!(Product::query().count(db).await.unwrap(), 1);
    assert_eq!(Product::query().with_trashed().count(db).await.unwrap(), 2);
    assert_eq!(
        Product::query().only_trashed().get(db).await.unwrap()[0].name,
        "Coffee"
    );
    assert!(Product::find(db, coffee.id).await.unwrap().is_none());

    coffee.restore(db).await.unwrap();
    assert_eq!(Product::query().count(db).await.unwrap(), 2);

    coffee.force_delete(db).await.unwrap();
    assert_eq!(Product::query().with_trashed().count(db).await.unwrap(), 1);
}

#[tokio::test]
async fn models_without_soft_deletes_delete_rows() {
    let (kernel, _dir) = kernel().await;
    let db = kernel.db();
    let mut notes = Note::create(
        db,
        Note {
            body: "hello".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(notes.restore(db).await.is_err());
    notes.delete(db).await.unwrap();
    assert_eq!(Note::query().count(db).await.unwrap(), 0);
}

#[tokio::test]
async fn queries_filter_order_and_limit() {
    let (kernel, _dir) = kernel().await;
    let db = kernel.db();
    for (name, price, category) in [
        ("Coffee", 18_000, Some("drinks")),
        ("Tea", 8_000, Some("drinks")),
        ("Toast", 12_000, Some("food")),
        ("Water", 5_000, None),
    ] {
        Product::create(db, product(name, price, category))
            .await
            .unwrap();
    }
    let names = |list: Vec<Product>| list.into_iter().map(|p| p.name).collect::<Vec<_>>();

    let drinks = Product::where_eq("category", "drinks")
        .order_by("name")
        .get(db)
        .await
        .unwrap();
    assert_eq!(names(drinks), ["Coffee", "Tea"]);

    let cheap = Product::query()
        .where_op("price", "<", 10_000)
        .order_by_desc("price")
        .get(db)
        .await
        .unwrap();
    assert_eq!(names(cheap), ["Tea", "Water"]);

    let chosen = Product::query()
        .where_in("name", ["Water", "Toast"])
        .order_by("id")
        .get(db)
        .await
        .unwrap();
    assert_eq!(names(chosen), ["Toast", "Water"]);
    assert!(
        Product::query()
            .where_in("name", Vec::<String>::new())
            .get(db)
            .await
            .unwrap()
            .is_empty()
    );

    assert_eq!(
        names(
            Product::query()
                .where_null("category")
                .get(db)
                .await
                .unwrap()
        ),
        ["Water"]
    );
    assert_eq!(
        Product::query()
            .where_not_null("category")
            .count(db)
            .await
            .unwrap(),
        3
    );
    assert_eq!(
        names(
            Product::query()
                .where_like("name", "%o%")
                .order_by("name")
                .get(db)
                .await
                .unwrap()
        ),
        ["Coffee", "Toast"]
    );
    assert_eq!(
        names(
            Product::query()
                .where_like("name", "COF%")
                .get(db)
                .await
                .unwrap()
        ),
        ["Coffee"],
        "like ignores case on every database"
    );
    let total = Product::query().count(db).await.unwrap() as usize;
    assert_eq!(
        Product::query()
            .order_by("price")
            .offset(1)
            .get(db)
            .await
            .unwrap()
            .len(),
        total - 1,
        "an offset works without a limit"
    );

    let slice = Product::query()
        .order_by("price")
        .limit(2)
        .offset(1)
        .get(db)
        .await
        .unwrap();
    assert_eq!(names(slice), ["Tea", "Toast"]);
    assert_eq!(
        Product::query()
            .latest()
            .first(db)
            .await
            .unwrap()
            .unwrap()
            .name,
        "Water"
    );
    assert!(
        Product::where_eq("name", "Coffee")
            .exists(db)
            .await
            .unwrap()
    );

    assert_eq!(
        Product::where_eq("category", "drinks")
            .delete(db)
            .await
            .unwrap(),
        2
    );
    assert_eq!(Product::query().count(db).await.unwrap(), 2);
}

#[tokio::test]
async fn invalid_columns_and_operators_are_errors_not_sql() {
    let (kernel, _dir) = kernel().await;
    let db = kernel.db();
    let err = Product::where_eq("name; DROP TABLE products", 1)
        .get(db)
        .await
        .unwrap_err();
    assert!(format!("{err:?}").contains("has no column"));
    let err = Product::query()
        .where_op("price", "OR 1=1 --", 1)
        .get(db)
        .await
        .unwrap_err();
    assert!(format!("{err:?}").contains("unsupported operator"));
    assert!(Product::all(db).await.is_ok(), "table still exists");
}

#[tokio::test]
async fn transactions_roll_back_on_drop() {
    let (kernel, _dir) = kernel().await;
    let db = kernel.db();
    {
        let mut tx = db.begin().await.unwrap();
        product("Coffee", 1, None).save(&mut tx).await.unwrap();
        assert_eq!(Product::query().count(&mut tx).await.unwrap(), 1);
    }
    assert_eq!(Product::query().count(db).await.unwrap(), 0);
}

#[tokio::test]
async fn foreign_keys_are_enforced() {
    let (kernel, _dir) = kernel().await;
    let db = kernel.db();
    let result = Note::create(
        db,
        Note {
            product_id: Some(42),
            body: "x".into(),
            ..Default::default()
        },
    )
    .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn paginate_counts_pages() {
    let (kernel, _dir) = kernel().await;
    let db = kernel.db();
    Product::factory().count(23).create(db).await.unwrap();
    let page = Product::query()
        .order_by("id")
        .paginate(db, 3, 10)
        .await
        .unwrap();
    assert_eq!(page.items.len(), 3);
    assert_eq!(
        (page.total, page.last_page, page.from, page.to),
        (23, 3, 21, 23)
    );
    assert!(page.has_prev && !page.has_next);
}

#[tokio::test]
async fn seeders_run_in_order() {
    let (kernel, _dir) = kernel_with(|app| {
        app.seeder(|state| async move {
            let db = state.db.clone();
            Product::factory().count(3).create(&db).await?;
            Ok(())
        })
        .seeder(|state| async move {
            let db = state.db.clone();
            let first = Product::query()
                .order_by("id")
                .first(&db)
                .await?
                .ok_or(Error::NotFound)?;
            Note::create(
                &db,
                Note {
                    product_id: Some(first.id),
                    body: "first".into(),
                    ..Default::default()
                },
            )
            .await?;
            Ok(())
        })
    })
    .await;
    kernel.seed().await.unwrap();
    assert_eq!(Product::query().count(kernel.db()).await.unwrap(), 3);
    assert_eq!(Note::query().count(kernel.db()).await.unwrap(), 1);
}

#[test]
fn values_skip_id_and_skipped_fields() {
    let p = product("Coffee", 5, None);
    assert_eq!(
        <Product as Model>::COLUMNS,
        &[
            "id",
            "name",
            "price",
            "category",
            "created_at",
            "updated_at",
            "deleted_at"
        ]
    );
    assert_eq!(
        p.values()[..3],
        [
            DbValue::Text("Coffee".into()),
            DbValue::Integer(5),
            DbValue::Null
        ]
    );
}

struct Shop;

impl Module for Shop {
    fn name(&self) -> &'static str {
        "shop"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/products", index)
            .get("/products/{id}", show)
    }
}

async fn index(State(db): State<Db>, Page(page): Page) -> Result<View> {
    let products = Product::query()
        .order_by("id")
        .paginate(&db, page, 2)
        .await?;
    Ok(view("index.html", context! { products }))
}

async fn show(State(db): State<Db>, Path(id): Path<i64>) -> Result<String> {
    Ok(Product::find_or_404(&db, id).await?.name)
}

#[tokio::test]
async fn handlers_use_the_pool_and_render_pagination() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("index.html"),
        r#"{% from "renox/pagination.html" import pagination %}{% for p in products.items %}[{{ p.name }}]{% endfor %}{{ pagination(products) }}"#,
    )
    .unwrap();
    let kernel = App::with_config(config(dir.path()))
        .migrations(renox::migrations!("tests/migrations"))
        .module(Shop)
        .boot()
        .await
        .unwrap();
    kernel.migrate().await.unwrap();
    for name in ["A", "B", "C", "D", "E"] {
        Product::create(kernel.db(), product(name, 1, None))
            .await
            .unwrap();
    }

    let get = |uri: &str| {
        let router = kernel.router();
        let uri = uri.to_owned();
        async move {
            let res = router
                .oneshot(Request::get(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            let status = res.status();
            let body = res.into_body().collect().await.unwrap().to_bytes();
            (status, String::from_utf8(body.to_vec()).unwrap())
        }
    };

    assert_eq!(get("/products/2").await, (StatusCode::OK, "B".into()));
    assert_eq!(get("/products/99").await.0, StatusCode::NOT_FOUND);

    let (status, body) = get("/products?page=2").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.starts_with("[C][D]"), "{body}");
    assert!(
        body.contains(r#"<span aria-current="page">2</span>"#),
        "{body}"
    );
    assert!(body.contains(r#"href="?page=1" rel="prev""#), "{body}");
    assert!(body.contains(r#"href="?page=3" rel="next""#), "{body}");
    assert!(get("/products?page=abc").await.1.starts_with("[A][B]"));

    // Page links keep the other query parameters.
    let (_, body) = get("/products?q=coffee+milk&page=2&sort=name").await;
    assert!(
        body.contains(r#"href="?q=coffee+milk&amp;sort=name&amp;page=3" rel="next""#),
        "{body}"
    );
}

#[tokio::test]
async fn raw_sql_binds_reads_and_commits() {
    let (kernel, _dir) = kernel().await;
    let db = kernel.db();

    let mut tx = db.begin().await.unwrap();
    let inserted = renox::db::sql("INSERT INTO products (name, price) VALUES (?, ?), (?, ?)")
        .bind("Coffee")
        .bind(18_000)
        .bind("Tea")
        .bind(9_000)
        .execute(&mut tx)
        .await
        .unwrap();
    assert_eq!(inserted, 2);
    tx.commit().await.unwrap();

    let rows = renox::db::sql("SELECT name, price FROM products WHERE price < ? ORDER BY name")
        .bind(20_000)
        .fetch_all(db)
        .await
        .unwrap();
    assert_eq!(rows[0].columns(), ["name", "price"]);
    assert_eq!(rows[0].try_get::<String>("name").unwrap(), "Coffee");
    assert_eq!(rows[1].try_get::<i64>(1).unwrap(), 9_000);

    // SUM of a BIGINT is NUMERIC on PostgreSQL; the cast keeps it an i64 on both.
    let total: i64 = renox::db::sql("SELECT CAST(SUM(price) AS BIGINT) FROM products")
        .scalar(db)
        .await
        .unwrap();
    assert_eq!(total, 27_000);
    let missing: Option<String> = renox::db::sql("SELECT name FROM products WHERE price > ?")
        .bind(1_000_000)
        .scalar_optional(db)
        .await
        .unwrap();
    assert_eq!(missing, None);
    let names: Vec<String> = renox::db::sql("SELECT name FROM products ORDER BY name")
        .scalars(db)
        .await
        .unwrap();
    assert_eq!(names, ["Coffee", "Tea"]);
}
