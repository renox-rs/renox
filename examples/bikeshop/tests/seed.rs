//! The seeders (#232): `db:seed` builds a whole shop in seconds, never
//! twice; `demo:seed` refuses a seeded database; the data is alive today
//! (rentals out and overdue, work on the bench, low stock, bikes and goods
//! at other stores, staff helping another store this week, balances not
//! settled yet); the stock ledger adds up to the levels; the books between
//! stores balance; and the demo users can work.

use bikeshop::app::access::catalogue::STAFF_ACCESS;
use bikeshop::app::staff::model::Store;
use bikeshop::seed;
use renox::db::sql;
use renox::prelude::*;
use renox::testing::TestApp;

async fn seeded() -> TestApp {
    let app = TestApp::new(bikeshop::app()).await;
    let started = std::time::Instant::now();
    seed::run(app.state().clone()).await.unwrap();
    let took = started.elapsed();
    assert!(took.as_secs() < 30, "the small seed took {took:?}");
    app
}

async fn count(app: &TestApp, query: &str) -> i64 {
    sql(query).scalar::<i64>(app.db()).await.unwrap()
}

#[renox::test]
async fn db_seed_makes_a_small_shop_once() {
    let app = seeded().await;
    let volume = seed::Volume::small();
    assert_eq!(count(&app, "SELECT COUNT(*) FROM stores").await, 3);
    assert!(count(&app, "SELECT COUNT(*) FROM products").await >= volume.products as i64);
    assert!(count(&app, "SELECT COUNT(*) FROM customers").await > volume.customers as i64);
    assert!(count(&app, "SELECT COUNT(*) FROM rentals").await > 200);
    assert!(count(&app, "SELECT COUNT(*) FROM orders").await >= volume.orders as i64);
    assert!(count(&app, "SELECT COUNT(*) FROM work_orders").await > volume.work_orders as i64);
    for table in [
        "countries",
        "cities",
        "addresses",
        "staff",
        "staff_help_requests",
        "staff_help_hours",
        "categories",
        "brands",
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
        "order_items",
        "payments",
        "customer_bikes",
        "service_tasks",
        "work_order_tasks",
        "service_plans",
        "plan_tasks",
        "plan_subscriptions",
        "intercompany_entries",
        "settlements",
    ] {
        assert!(
            count(&app, &format!("SELECT COUNT(*) FROM {table}")).await > 0,
            "{table} is empty"
        );
    }

    // Again: nothing is added.
    let before = count(&app, "SELECT COUNT(*) FROM rentals").await;
    seed::run(app.state().clone()).await.unwrap();
    assert_eq!(count(&app, "SELECT COUNT(*) FROM stores").await, 3);
    assert_eq!(count(&app, "SELECT COUNT(*) FROM rentals").await, before);

    // `demo:seed` refuses, and says how to start again.
    let refused = app
        .kernel()
        .call("demo:seed", ["--size", "small"])
        .await
        .unwrap_err();
    let refused = format!("{refused:?}");
    assert!(refused.contains("migrate:fresh"), "{refused}");
}

#[renox::test]
async fn demo_seed_fills_an_empty_database() {
    let app = TestApp::new(bikeshop::app()).await;
    app.kernel()
        .call("demo:seed", ["--size", "small"])
        .await
        .unwrap();
    assert_eq!(count(&app, "SELECT COUNT(*) FROM stores").await, 3);
}

#[renox::test]
async fn the_shop_is_alive_today() {
    let app = seeded().await;
    let db = app.db();
    let now = renox::db::now();
    let day_end = (now.date_naive() + renox::chrono::Duration::days(1))
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc();
    let at = |query: &str| sql(query.to_owned()).bind(now);

    // Bikes out right now, some overdue, some due back today.
    let out: i64 =
        at("SELECT COUNT(*) FROM rentals WHERE status IN ('active', 'overdue') AND starts_at <= ?")
            .scalar(db)
            .await
            .unwrap();
    assert!(out > 0, "no bike is out");
    let overdue: i64 = at("SELECT COUNT(*) FROM rentals WHERE status = 'overdue' AND due_at < ?")
        .scalar(db)
        .await
        .unwrap();
    assert!(overdue > 0, "nothing is overdue");
    let due_today: i64 =
        sql("SELECT COUNT(*) FROM rentals WHERE status = 'active' AND due_at >= ? AND due_at < ?")
            .bind(now)
            .bind(day_end)
            .scalar(db)
            .await
            .unwrap();
    assert!(due_today > 0, "no rental is due back today");
    // The history ends now and starts months ago.
    let oldest: DateTime = sql("SELECT MIN(starts_at) FROM rentals")
        .scalar(db)
        .await
        .unwrap();
    assert!(now - oldest > renox::chrono::Duration::days(60), "{oldest}");

    // Work on the bench, low stock, bikes placed elsewhere, consigned goods.
    assert!(
        count(
            &app,
            "SELECT COUNT(*) FROM work_orders WHERE status IN ('in_progress', 'waiting_parts')"
        )
        .await
            > 0
    );
    assert!(
        count(
            &app,
            "SELECT COUNT(*) FROM work_orders WHERE status = 'booked'"
        )
        .await
            > 0
    );
    assert!(
        count(
            &app,
            "SELECT COUNT(*) FROM stock_levels s JOIN product_variants v ON v.id = s.variant_id \
             WHERE s.owner_store_id = s.location_store_id AND s.on_hand < v.reorder_level"
        )
        .await
            > 0,
        "no low stock"
    );
    assert!(
        count(
            &app,
            "SELECT COUNT(*) FROM rental_bikes WHERE owner_store_id <> location_store_id"
        )
        .await
            > 0
    );
    assert!(
        count(&app, "SELECT COUNT(*) FROM stock_levels WHERE owner_store_id <> location_store_id AND on_hand > 0").await
            > 0,
        "no goods on consignment"
    );
    // Someone helps another store this week (a dated role in force).
    let helping: i64 = at("SELECT COUNT(*) FROM role_user WHERE scope_type = 'stores' AND ends_at > ? AND starts_at <= ?")
        .bind(now)
        .scalar(db)
        .await
        .unwrap();
    assert_eq!(helping, 1);
    // Balances between stores: some settled, the last month and this one not yet.
    assert!(
        count(
            &app,
            "SELECT COUNT(*) FROM settlements WHERE status = 'settled'"
        )
        .await
            > 0
    );
    assert!(
        count(
            &app,
            "SELECT COUNT(*) FROM settlements WHERE status = 'open'"
        )
        .await
            > 0
    );
    assert!(
        count(
            &app,
            "SELECT COUNT(*) FROM intercompany_entries WHERE settlement_id IS NULL"
        )
        .await
            > 0
    );
}

#[renox::test]
async fn the_stock_ledger_adds_up_to_the_levels() {
    let app = seeded().await;
    let rows: Vec<(i64, i64, i64)> = sql("SELECT s.on_hand, s.reserved, \
         COALESCE((SELECT CAST(SUM(m.quantity) AS BIGINT) FROM stock_movements m WHERE m.variant_id = s.variant_id \
           AND m.owner_store_id = s.owner_store_id AND m.location_store_id = s.location_store_id \
           AND m.reason NOT IN ('reserved', 'released')), 0) \
         FROM stock_levels s")
    .fetch_as(app.db())
    .await
    .unwrap();
    assert!(!rows.is_empty());
    for (on_hand, reserved, ledger) in rows {
        assert_eq!(on_hand, ledger, "a level doesn't match its ledger");
        assert!(on_hand >= 0 && reserved >= 0 && reserved <= on_hand);
    }
}

#[renox::test]
async fn the_books_between_stores_balance() {
    let app = seeded().await;
    let db = app.db();
    // (PostgreSQL sums BIGINTs as NUMERIC: the CASTs read them as i64.)
    // Each store's balance (owed to it minus owed by it) adds up to zero for
    // the company: the stores' numbers add up to the company's.
    let stores = Store::all_by_name(db).await.unwrap();
    let mut total = 0;
    for store in &stores {
        let owed_to: i64 = sql(
            "SELECT CAST(COALESCE(SUM(amount), 0) AS BIGINT) FROM intercompany_entries WHERE creditor_store_id = ?",
        )
        .bind(store.id)
        .scalar(db)
        .await
        .unwrap();
        let owed_by: i64 = sql(
            "SELECT CAST(COALESCE(SUM(amount), 0) AS BIGINT) FROM intercompany_entries WHERE debtor_store_id = ?",
        )
        .bind(store.id)
        .scalar(db)
        .await
        .unwrap();
        total += owed_to - owed_by;
    }
    assert_eq!(total, 0);
    // A rental of another store's bike books the revenue to the owner and
    // the fee (at the operating store's rate) to the store that served it.
    let rows: Vec<(i64, i64, i64, i64)> = sql(
        "SELECT r.price, e.amount, e.fee_rate_bp, s.fee_rate_bp FROM rentals r \
         JOIN intercompany_entries e ON e.source_type = 'rentals' AND e.source_id = r.id \
           AND e.kind = 'operating_fee' AND e.creditor_store_id = r.operating_store_id \
         JOIN stores s ON s.id = r.operating_store_id",
    )
    .fetch_as(db)
    .await
    .unwrap();
    assert!(!rows.is_empty(), "no rental of another store's bike");
    for (price, fee, rate, store_rate) in rows {
        assert_eq!(rate, store_rate);
        assert_eq!(fee, bikeshop::app::staff::model::fee(price, rate));
    }
    // Every settled month is netted: its amount is what its entries net to.
    let settled: Vec<(i64, i64, i64, i64)> = sql(
        "SELECT st.id, st.debtor_store_id, st.amount, \
         CAST(COALESCE(SUM(CASE WHEN e.debtor_store_id = st.debtor_store_id THEN e.amount ELSE -e.amount END), 0) AS BIGINT) \
         FROM settlements st JOIN intercompany_entries e ON e.settlement_id = st.id \
         GROUP BY st.id, st.debtor_store_id, st.amount",
    )
    .fetch_as(db)
    .await
    .unwrap();
    for (id, _, amount, net) in settled {
        assert_eq!(amount, net, "settlement {id}");
    }
}

#[renox::test]
async fn the_demo_users_can_work() {
    let app = seeded().await;
    let db = app.db();
    let user = |email: &'static str| async move {
        User::query()
            .where_eq("email", email)
            .first(db)
            .await
            .unwrap()
            .unwrap()
    };

    // The owner: every store in the switcher.
    app.acting_as(&user("owner@bikeshop.test").await);
    let page = app.get("/staff").await;
    page.assert_ok();
    for store in Store::all_by_name(db).await.unwrap() {
        page.assert_see(&format!("/staff/store/{}", store.id));
    }

    // A store's manager works in their store only.
    app.acting_as(&user("manager.north@bikeshop.test").await);
    app.get("/staff")
        .await
        .assert_ok()
        .assert_see("Working in North")
        .assert_dont_see("id=\"store-menu\"");

    // Roles in two stores: both in the switcher.
    app.acting_as(&user("floater@bikeshop.test").await);
    app.get("/staff")
        .await
        .assert_see("id=\"store-menu\"")
        .assert_see(">North<")
        .assert_see(">South<");

    // West's mechanic helps South this week.
    app.acting_as(&user("mechanic.west@bikeshop.test").await);
    app.get("/staff")
        .await
        .assert_see(">South<")
        .assert_see(">West<");

    // The customer isn't staff, and has their bikes, rentals and a plan.
    let customer = user("customer@bikeshop.test").await;
    app.acting_as(&customer);
    app.get("/staff").await.assert_forbidden();
    let id: i64 = sql("SELECT id FROM customers WHERE user_id = ?")
        .bind(customer.id)
        .scalar(db)
        .await
        .unwrap();
    for (what, query) in [
        (
            "bikes",
            "SELECT COUNT(*) FROM customer_bikes WHERE customer_id = ?",
        ),
        (
            "rentals",
            "SELECT COUNT(*) FROM rentals WHERE customer_id = ?",
        ),
        (
            "a plan",
            "SELECT COUNT(*) FROM plan_subscriptions p JOIN customer_bikes b ON b.id = p.customer_bike_id \
             WHERE b.customer_id = ? AND p.status = 'active'",
        ),
    ] {
        let n: i64 = sql(query).bind(id).scalar(db).await.unwrap();
        assert!(n > 0, "the demo customer has no {what}");
    }
    let _ = STAFF_ACCESS;
}
