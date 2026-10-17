//! PostgreSQL-only checks. They run with `--features postgres` and
//! `TEST_DATABASE_URL` set, e.g. `postgres://postgres:postgres@localhost:5432/renox_test`
//! (which also runs every other test against PostgreSQL).

use renox::chrono;
use renox::db::{Db, Dialect, sql};

async fn db() -> Option<Db> {
    let url = std::env::var("TEST_DATABASE_URL").ok()?;
    let pool = renox::db::sqlx::PgPool::connect(&url)
        .await
        .expect("TEST_DATABASE_URL is reachable");
    Some(Db::from(pool))
}

#[tokio::test]
async fn raw_sql_runs_on_postgres() {
    let Some(db) = db().await else {
        eprintln!("skipped: TEST_DATABASE_URL is not set");
        return;
    };
    assert_eq!(db.dialect(), Dialect::Postgres);
    let mut tx = db.begin().await.unwrap();
    sql("CREATE TEMPORARY TABLE products (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL, price BIGINT NOT NULL)")
        .execute(&mut tx)
        .await
        .unwrap();
    let inserted = sql("INSERT INTO products (name, price) VALUES (?, ?), (?, ?)")
        .bind("Coffee")
        .bind(18_000_i64)
        .bind("Tea '?'")
        .bind(9_000_i64)
        .execute(&mut tx)
        .await
        .unwrap();
    assert_eq!(inserted, 2);
    let rows =
        sql("SELECT name, price FROM products WHERE price < ? AND name <> '?' ORDER BY name")
            .bind(20_000_i64)
            .fetch_all(&mut tx)
            .await
            .unwrap();
    assert_eq!(rows[0].columns(), ["name", "price"]);
    assert_eq!(rows[1].try_get::<String>("name").unwrap(), "Tea '?'");
    let total: i64 = sql("SELECT SUM(price)::BIGINT FROM products")
        .scalar(&mut tx)
        .await
        .unwrap();
    assert_eq!(total, 27_000);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn typed_values_bind_on_postgres() {
    let Some(db) = db().await else {
        eprintln!("skipped: TEST_DATABASE_URL is not set");
        return;
    };
    let mut tx = db.begin().await.unwrap();
    sql("CREATE TEMPORARY TABLE typed_values (active BOOLEAN, happened_at TIMESTAMPTZ, day DATE, body TEXT)")
        .execute(&mut tx)
        .await
        .unwrap();
    let at = renox::db::now();
    let day = at.date_naive();
    // NULL goes in untyped, so it fits a TIMESTAMPTZ as well as a TEXT column.
    sql("INSERT INTO typed_values (active, happened_at, day, body) VALUES (?, ?, ?, ?), (?, ?, ?, ?)")
        .bind(true)
        .bind(at)
        .bind(day)
        .bind("x")
        .bind(false)
        .bind(None::<renox::db::DateTime>)
        .bind(None::<chrono::NaiveDate>)
        .bind(None::<String>)
        .execute(&mut tx)
        .await
        .unwrap();
    let row = sql("SELECT active, happened_at, day FROM typed_values WHERE active = ?")
        .bind(true)
        .fetch_one(&mut tx)
        .await
        .unwrap();
    assert!(row.try_get::<bool>("active").unwrap());
    assert_eq!(
        row.try_get::<renox::db::DateTime>("happened_at").unwrap(),
        at
    );
    assert_eq!(row.try_get::<chrono::NaiveDate>("day").unwrap(), day);
    let empty: Option<renox::db::DateTime> =
        sql("SELECT happened_at FROM typed_values WHERE NOT active")
            .scalar(&mut tx)
            .await
            .unwrap();
    assert_eq!(empty, None);
}

/// `?` inside PostgreSQL's dollar-quoted and `E'…'` strings stays a
/// character, not a placeholder (#219).
#[tokio::test]
async fn question_marks_in_dollar_quoted_and_escaped_strings() {
    let Some(db) = db().await else {
        eprintln!("skipped: TEST_DATABASE_URL is not set");
        return;
    };
    let row = sql("SELECT $$why?$$ AS a, $t$ a ? b $t$ AS b, E'it\\'s ?' AS c, ?::BIGINT AS d")
        .bind(7_i64)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(row.try_get::<String>("a").unwrap(), "why?");
    assert_eq!(row.try_get::<String>("b").unwrap(), " a ? b ");
    assert_eq!(row.try_get::<String>("c").unwrap(), "it's ?");
    assert_eq!(row.try_get::<i64>("d").unwrap(), 7);
}

/// What docs/types.md says about `i8` and `serde_json::Value` (#219).
#[tokio::test]
async fn small_integers_and_json_values_on_postgres() {
    let Some(db) = db().await else {
        eprintln!("skipped: TEST_DATABASE_URL is not set");
        return;
    };
    let mut tx = db.begin().await.unwrap();
    sql("CREATE TEMPORARY TABLE probe (small SMALLINT, doc JSONB, text_doc TEXT)")
        .execute(&mut tx)
        .await
        .unwrap();
    let doc = serde_json::json!({"a": 1});
    sql("INSERT INTO probe (small, doc, text_doc) VALUES (?, ?, ?)")
        .bind(5_i8)
        .bind(doc.clone())
        .bind(doc.clone())
        .execute(&mut tx)
        .await
        .unwrap();
    let row = sql("SELECT small, doc, text_doc FROM probe")
        .fetch_one(&mut tx)
        .await
        .unwrap();
    // sqlx reads `i8` as PostgreSQL's one-byte `"CHAR"`, not SMALLINT:
    // models use `i16` there.
    assert_eq!(row.try_get::<i16>("small").unwrap(), 5);
    assert!(row.try_get::<i8>("small").is_err());
    // A bare `serde_json::Value` needs JSON or JSONB; `Json<T>` also reads TEXT.
    assert_eq!(row.try_get::<serde_json::Value>("doc").unwrap(), doc);
    assert!(row.try_get::<serde_json::Value>("text_doc").is_err());
    let text: renox::db::Json<serde_json::Value> = row.try_get("text_doc").unwrap();
    assert_eq!(text.0, doc);
    tx.rollback().await.unwrap();
}

/// The outbound webhook tables on PostgreSQL: the sealed secret, the delivery
/// log and the retention prune. `TestApp` gives the test its own schema, so
/// the webhook migrations run there too.
#[tokio::test]
async fn outbound_webhooks_store_and_prune_on_postgres() {
    use renox::http::FakeResponse;
    use renox::prelude::*;
    use renox::webhook::outbound::{Delivery, Endpoint};

    if std::env::var("TEST_DATABASE_URL").is_err() {
        eprintln!("skipped: TEST_DATABASE_URL is not set");
        return;
    }
    #[derive(Clone)]
    struct Shipped {
        id: i64,
    }
    impl Event for Shipped {}

    let app = renox::testing::TestApp::new(
        App::new().webhook_event::<Shipped>("order.shipped", |e| json!({ "id": e.id })),
    )
    .await;
    assert_eq!(app.db().dialect(), Dialect::Postgres);
    let http = app.fake_http();
    http.on("POST https://a.test/*", FakeResponse::status(500));
    http.on("POST https://a.test/*", FakeResponse::status(200));
    let (endpoint, secret) = Endpoint::create(app.state(), "https://a.test/h", "pg", &[])
        .await
        .unwrap();
    let stored: String = sql("SELECT secret FROM webhook_endpoints WHERE id = ?")
        .bind(endpoint.id)
        .scalar(app.db())
        .await
        .unwrap();
    assert_ne!(stored, secret);
    app.state().emit(Shipped { id: 3 }).await.unwrap();
    app.run_jobs().await;
    let rows = Delivery::list(app.db(), Some(endpoint.id), false, 10)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].response_status, Some(500));
    assert!(Delivery::redeliver(app.state(), rows[0].id).await.unwrap());
    app.run_jobs().await;
    http.assert_sent_count(2);
    app.travel(std::time::Duration::from_secs(2 * 86_400));
    let pruned = app
        .at_travelled_time(Delivery::prune(
            app.db(),
            std::time::Duration::from_secs(86_400),
        ))
        .await
        .unwrap();
    assert_eq!(pruned, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_state_transitions_allow_exactly_one_on_postgres() {
    if std::env::var("TEST_DATABASE_URL").is_err() {
        eprintln!("skipped: TEST_DATABASE_URL is not set");
        return;
    }
    let app = crate::state_machine::ticket_app().await;
    assert_eq!(app.db().dialect(), Dialect::Postgres);
    crate::state_machine::race_for_one_winner(&app, 8).await;
}

/// #45: a `Money` is a `BIGINT` amount and a `TEXT` code on PostgreSQL, and
/// the pair reads back (an empty pair is `None`).
#[tokio::test]
async fn money_pairs_round_trip_on_postgres() {
    use renox::money::Money;
    let Some(db) = db().await else {
        eprintln!("skipped: TEST_DATABASE_URL is not set");
        return;
    };
    let mut tx = db.begin().await.unwrap();
    sql("CREATE TEMPORARY TABLE money_pairs (id BIGINT PRIMARY KEY, total BIGINT, total_currency TEXT)")
        .execute(&mut tx)
        .await
        .unwrap();
    let cases = [
        (1_i64, Some(Money::kwd(12_345))),
        (2, Some(Money::jpy(-1_250))),
        (3, None),
    ];
    for (id, money) in cases {
        sql("INSERT INTO money_pairs (id, total, total_currency) VALUES (?, ?, ?)")
            .bind(id)
            .bind(money.map(|m| m.minor))
            .bind(money.map(|m| m.currency.code()))
            .execute(&mut tx)
            .await
            .unwrap();
    }
    for (id, want) in cases {
        let row = sql("SELECT total, total_currency FROM money_pairs WHERE id = ?")
            .bind(id)
            .fetch_one(&mut tx)
            .await
            .unwrap();
        assert_eq!(
            row.try_get_money_opt("total", "total_currency").unwrap(),
            want
        );
    }
    tx.rollback().await.unwrap();
}

/// #69: a `Quantity` is one `BIGINT` column on PostgreSQL, exact at the
/// `i64` limits, and a NULL reads back as `None`.
#[tokio::test]
async fn quantities_round_trip_on_postgres() {
    use renox::quantity::Quantity;
    let Some(db) = db().await else {
        eprintln!("skipped: TEST_DATABASE_URL is not set");
        return;
    };
    let mut tx = db.begin().await.unwrap();
    sql("CREATE TEMPORARY TABLE quantities (id BIGINT PRIMARY KEY, amount BIGINT NOT NULL, spare BIGINT)")
        .execute(&mut tx)
        .await
        .unwrap();
    let cases = [
        (
            1_i64,
            Quantity::<3>::from_raw(i64::MAX),
            Some(Quantity::<3>::from_raw(i64::MIN)),
        ),
        (2, Quantity::<3>::parse_exact("2.500").unwrap(), None),
        (3, Quantity::<3>::from_raw(-1), Some(Quantity::<3>::ZERO)),
    ];
    for (id, amount, spare) in cases {
        sql("INSERT INTO quantities (id, amount, spare) VALUES (?, ?, ?)")
            .bind(id)
            .bind(amount)
            .bind(spare)
            .execute(&mut tx)
            .await
            .unwrap();
    }
    for (id, amount, spare) in cases {
        let row = sql("SELECT amount, spare FROM quantities WHERE id = ?")
            .bind(id)
            .fetch_one(&mut tx)
            .await
            .unwrap();
        assert_eq!(row.try_get::<Quantity<3>>("amount").unwrap(), amount);
        assert_eq!(row.try_get::<Option<Quantity<3>>>("spare").unwrap(), spare);
    }
    tx.rollback().await.unwrap();
}
