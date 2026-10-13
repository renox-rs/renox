//! Dashboards, reports and exports (#242): the dashboard's numbers against
//! fixed data, counted by owner store and by operating store, adding up to
//! the company either way and moving with `TestApp::travel`; who sees which
//! stores; the cache and its invalidation; the grids, their exports and the
//! customers' lifetime value; the monthly report's batch, workbooks and
//! mails; the pages' query counts.

use bikeshop::app::access::catalogue::{CASHIER, MANAGER, OWNER};
use bikeshop::app::accounts::model::Customer;
use bikeshop::app::multistore::books;
use bikeshop::app::plans::factories::plan_subscriptions;
use bikeshop::app::plans::model::ServicePlan;
use bikeshop::app::rentals::RentalClosed;
use bikeshop::app::rentals::factories::{RentalStates, rentals};
use bikeshop::app::rentals::model::{Rental, RentalBike};
use bikeshop::app::reports::model::CustomerValue;
use bikeshop::app::reports::monthly;
use bikeshop::app::reports::numbers::{Numbers, changed};
use bikeshop::app::reports::scope::{By, Reach, StoreRef};
use bikeshop::app::sales::factories::{OrderStates, orders};
use bikeshop::app::sales::model::{Order, OrderItem};
use bikeshop::app::staff::model::Store;
use bikeshop::app::workshop::factories::{WorkOrderStates, customer_bikes_of, work_orders};
use bikeshop::app::workshop::model::WorkOrder;
use bikeshop::seed::{self, fixtures};
use renox::chart::Period;
use renox::db::capture_queries;
use renox::prelude::*;
use renox::testing::TestApp;
use std::time::Duration;

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

struct World {
    app: TestApp,
    north: Store,
    south: Store,
    west: Store,
    owner: User,
    manager_north: User,
    manager_south: User,
    cashier_north: User,
    customer: Customer,
    order: Order,
}

/// Three stores and their people, and one of each kind of income in the
/// last week:
///
/// | What | Owner store (books) | Operating store (work) | Amount (cents in the code) |
/// |---|---|---|---|
/// | A rental of North's bike, rented out at South, back late | North | South | $1,000 + $200 late fee |
/// | An order at South: a line of North's consigned goods and one of South's | North / South | South | $500 + $300 |
/// | A service at West | West | West | $400 |
/// | A plan visit at North | North | North | $250 |
async fn world() -> World {
    let app = TestApp::new(bikeshop::app()).await;
    let db = app.db().clone();
    fixtures::roles(&db).await.unwrap();
    let north = fixtures::store(&db, "North").await.unwrap();
    let south = fixtures::store(&db, "South").await.unwrap();
    let west = fixtures::store(&db, "West").await.unwrap();
    let owner = fixtures::person(&db, "owner@example.com", &[(OWNER, None)])
        .await
        .unwrap();
    let manager_north = fixtures::person(&db, "mn@example.com", &[(MANAGER, Some(north.id))])
        .await
        .unwrap();
    let manager_south = fixtures::person(&db, "ms@example.com", &[(MANAGER, Some(south.id))])
        .await
        .unwrap();
    let cashier_north = fixtures::person(&db, "cn@example.com", &[(CASHIER, Some(north.id))])
        .await
        .unwrap();
    let customer = Customer::create(
        &db,
        Customer {
            name: "Rider Report".into(),
            email: Some(format!("rider{}@example.com", seed::unique())),
            active: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // North's bike, placed at South and rented out there.
    let bike: RentalBike = fixtures::bike(&db, north.id, south.id).await.unwrap();
    let mut rental: Rental = rentals()
        .of_bike(&bike)
        .for_customer(customer.id)
        .returned_late()
        .create_one(&db)
        .await
        .unwrap();
    rental.price = 100_000;
    rental.late_fee = 20_000;
    rental.save(&db).await.unwrap();
    books::rental(&db, rental.id).await.unwrap();

    // An order at South with a line of North's consigned goods.
    let order = orders()
        .at(south.id)
        .for_customer(customer.id)
        .totalling(80_000)
        .paid()
        .create_one(&db)
        .await
        .unwrap();
    for (owner_store, total) in [(north.id, 50_000), (south.id, 30_000)] {
        OrderItem::create(
            &db,
            OrderItem {
                order_id: order.id,
                variant_id: bike.variant_id,
                owner_store_id: owner_store,
                quantity: 1,
                unit_price: total,
                total,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    // A service at West, and a plan visit at North.
    let customer_bike = customer_bikes_of(customer.id)
        .create_one(&db)
        .await
        .unwrap();
    work_orders()
        .at(west.id)
        .on_bike(customer_bike.id)
        .completed(40_000)
        .create_one(&db)
        .await
        .unwrap();
    let plan = ServicePlan::factory().create_one(&db).await.unwrap();
    let subscription = plan_subscriptions()
        .state(move |s| {
            s.customer_bike_id = customer_bike.id;
            s.service_plan_id = plan.id;
            s.store_id = north.id;
        })
        .create_one(&db)
        .await
        .unwrap();
    let mut visit: WorkOrder = work_orders()
        .at(north.id)
        .on_bike(customer_bike.id)
        .completed(25_000)
        .create_one(&db)
        .await
        .unwrap();
    visit.plan_subscription_id = Some(subscription.id);
    visit.save(&db).await.unwrap();

    World {
        app,
        north,
        south,
        west,
        owner,
        manager_north,
        manager_south,
        cashier_north,
        customer,
        order,
    }
}

fn stores(w: &World) -> Vec<StoreRef> {
    [&w.north, &w.south, &w.west]
        .into_iter()
        .map(|s| StoreRef {
            id: s.id,
            name: s.name.clone(),
        })
        .collect()
}

async fn numbers(w: &World, only: Option<i64>, by: By, period: Period) -> Numbers {
    let reach = Reach::from_stores(stores(w), only, true);
    w.app
        .at_travelled_time(Numbers::compute(w.app.state().clone(), reach, by, period))
        .await
        .unwrap()
}

fn stream(n: &Numbers, key: &str) -> i64 {
    n.streams.iter().find(|s| s.key == key).unwrap().amount
}

#[renox::test]
async fn the_numbers_match_the_data_by_work_and_by_books() {
    let w = world().await;
    let week = Period::days(7);

    // Where the work was done.
    let south = numbers(&w, Some(w.south.id), By::Work, week).await;
    assert_eq!(stream(&south, "sales"), 80_000);
    assert_eq!(stream(&south, "rentals"), 120_000);
    assert_eq!(south.orders, 1);
    assert_eq!(south.average_order, 80_000);
    assert_eq!(south.rentals, 1);
    let west = numbers(&w, Some(w.west.id), By::Work, week).await;
    assert_eq!(stream(&west, "workshop"), 40_000);
    assert_eq!(west.work_done, 1);
    let north = numbers(&w, Some(w.north.id), By::Work, week).await;
    assert_eq!(stream(&north, "plans"), 25_000);
    assert_eq!(north.revenue, 25_000);
    assert_eq!(north.active_plans, 1);
    assert_eq!(north.recurring_monthly, 6_000);

    // In whose books: North's bike and goods, wherever they earned.
    let north = numbers(&w, Some(w.north.id), By::Books, week).await;
    assert_eq!(stream(&north, "rentals"), 120_000);
    assert_eq!(stream(&north, "sales"), 50_000);
    assert_eq!(north.revenue, 120_000 + 50_000 + 25_000);
    let south = numbers(&w, Some(w.south.id), By::Books, week).await;
    assert_eq!(south.revenue, 30_000);

    // The stores add up to the company, and both ways give the same company.
    for by in [By::Work, By::Books] {
        let company = numbers(&w, None, by, week).await;
        assert_eq!(company.revenue, 265_000, "{by:?}");
        let mut sum = 0;
        for store in [&w.north, &w.south, &w.west] {
            sum += numbers(&w, Some(store.id), by, week).await.revenue;
        }
        assert_eq!(sum, company.revenue, "{by:?}");
        assert_eq!(company.comparison.len(), 3);
        assert_eq!(
            company.comparison.iter().map(|s| s.total).sum::<i64>(),
            265_000
        );
    }

    // The rental at South earned South its operating fee from North.
    let south = numbers(&w, Some(w.south.id), By::Work, week).await;
    let north = numbers(&w, Some(w.north.id), By::Books, week).await;
    assert!(south.fees_earned > 0);
    assert_eq!(south.fees_earned, north.fees_paid);
    assert_eq!(south.position, -north.position);

    // The best customer and the top product.
    let company = numbers(&w, None, By::Work, week).await;
    assert_eq!(company.top_customers[0].name, "Rider Report");
    assert_eq!(company.top_customers[0].amount, 265_000);
    assert_eq!(company.top_products[0].amount, 80_000);
    assert!(company.rental_hours.iter().map(|p| p.value).sum::<i64>() <= 1);

    // Forty days on, the last 30 days are empty and the last 90 still hold it all.
    w.app.travel(DAY * 40);
    let month = numbers(&w, None, By::Work, Period::days(30)).await;
    assert_eq!(month.revenue, 0);
    assert_eq!(month.revenue_previous, 265_000);
    assert_eq!(month.revenue_change, Some(-100.0));
    let quarter = numbers(&w, None, By::Work, Period::days(90)).await;
    assert_eq!(quarter.revenue, 265_000);
    let weeks = numbers(&w, None, By::Work, Period::weeks(12)).await;
    assert_eq!(weeks.labels.len(), weeks.revenue_trend.len());
    assert_eq!(weeks.revenue_trend.iter().sum::<f64>(), 265_000.0);
}

#[renox::test]
async fn a_manager_sees_only_the_stores_where_they_have_reports_view() {
    let w = world().await;
    // The owner: every store, compared.
    w.app.acting_as(&w.owner);
    let page = w.app.get("/staff/reports").await;
    page.assert_ok();
    page.assert_see("Store comparison");
    page.assert_see(&w.south.name);

    // North's manager: North only, whatever `?store=` asks.
    w.app.acting_as(&w.manager_north);
    let page = w
        .app
        .get(&format!("/staff/reports?store={}", w.south.id))
        .await;
    page.assert_ok();
    page.assert_dont_see("Store comparison");
    page.assert_dont_see(&format!("How {} is doing", w.south.name));
    // Its own store, picked, is named.
    w.app
        .get(&format!("/staff/reports?store={}", w.north.id))
        .await
        .assert_see(&format!("How {} is doing", w.north.name));
    // South's order isn't in North's orders report; North's rental at South is.
    let orders = w.app.get("/staff/reports/orders").await;
    orders.assert_ok();
    orders.assert_dont_see(&w.order.number);
    let rentals = w.app.get("/staff/reports/rentals").await;
    rentals.assert_see("Rider Report");
    // The customer did business with North (its bike and goods), so North sees them.
    w.app
        .get("/staff/reports/customers")
        .await
        .assert_see("Rider Report");

    // South's manager sees South's order.
    w.app.acting_as(&w.manager_south);
    w.app
        .get("/staff/reports/orders")
        .await
        .assert_see(&w.order.number);

    // A cashier has no reports.view: every report page is a 403.
    w.app.acting_as(&w.cashier_north);
    for path in [
        "/staff/reports",
        "/staff/reports/orders",
        "/staff/reports/rentals",
        "/staff/reports/work-orders",
        "/staff/reports/payments",
        "/staff/reports/customers",
        "/staff/reports/intercompany",
        "/staff/reports/monthly",
    ] {
        w.app.get(path).await.assert_forbidden();
    }
    // …and the staff home page shows no figures.
    let home = w.app.get("/staff").await;
    home.assert_ok();
    home.assert_dont_see(&format!("{}, last 7 days", w.north.name));
    // North's manager gets the store's week there.
    w.app.acting_as(&w.manager_north);
    w.app
        .get("/staff")
        .await
        .assert_see(&format!("{}, last 7 days", w.north.name));

    // A role in South for this week gives North's manager South's reports too.
    fixtures::dated_role(w.app.db(), &w.manager_north, MANAGER, w.south.id, -1, 7)
        .await
        .unwrap();
    let user = User::find(w.app.db(), w.manager_north.id)
        .await
        .unwrap()
        .unwrap();
    w.app.acting_as(&user);
    let page = w.app.get("/staff/reports").await;
    page.assert_ok();
    page.assert_see("Store comparison");
}

#[renox::test]
async fn the_dashboard_is_cached_until_income_changes() {
    let w = world().await;
    let state = w.app.state().clone();
    let reach = Reach::from_stores(stores(&w), None, true);
    let week = Period::days(7);
    let first = Numbers::for_page(&state, &reach, By::Work, week)
        .await
        .unwrap();
    assert_eq!(first.revenue, 265_000);

    // A rental returned without its event: the cached numbers stay.
    let rental = rentals()
        .of_bike(
            &fixtures::bike(w.app.db(), w.west.id, w.west.id)
                .await
                .unwrap(),
        )
        .for_customer(w.customer.id)
        .returned()
        .create_one(w.app.db())
        .await
        .unwrap();
    let cached = Numbers::for_page(&state, &reach, By::Work, week)
        .await
        .unwrap();
    assert_eq!(cached, first);

    // The event that closes a rental clears them.
    state
        .emit(RentalClosed {
            rental_id: rental.id,
        })
        .await
        .unwrap();
    let fresh = Numbers::for_page(&state, &reach, By::Work, week)
        .await
        .unwrap();
    assert_eq!(fresh.revenue, 265_000 + rental.price);

    // So does `changed`, which every listener calls.
    Order::query()
        .where_(Order::ID.eq(w.order.id))
        .update(w.app.db(), &[("status", &"refunded")])
        .await
        .unwrap();
    changed(&state).await.unwrap();
    let refunded = Numbers::for_page(&state, &reach, By::Work, week)
        .await
        .unwrap();
    assert_eq!(refunded.revenue, fresh.revenue - 80_000);
}

#[renox::test]
async fn the_grids_filter_group_sum_and_export() {
    let w = world().await;
    w.app.acting_as(&w.owner);
    // Grouped by store with a sum: the orders grid answers with the order.
    let page = w.app.get("/staff/reports/orders?group=store").await;
    page.assert_ok();
    page.assert_see(&w.order.number);
    // Filtered by a store that sold nothing.
    let none = w
        .app
        .get(&format!("/staff/reports/orders?in.store={}", w.west.name))
        .await;
    none.assert_ok();
    none.assert_dont_see(&w.order.number);
    // The advanced filter, in whole units: the order's $800.00 is over
    // $500, not over $1,000.
    w.app
        .get("/staff/reports/orders?r.0.c=total&r.0.o=gt&r.0.v=500")
        .await
        .assert_see(&w.order.number);
    w.app
        .get("/staff/reports/orders?r.0.c=total&r.0.o=gt&r.0.v=1000")
        .await
        .assert_dont_see(&w.order.number);

    // Exports: CSV, Excel, print.
    let csv = w.app.get("/staff/reports/orders?export=csv").await;
    csv.assert_ok();
    assert!(csv.text().contains(&w.order.number));
    let xlsx = w.app.get("/staff/reports/rentals?export=xlsx").await;
    xlsx.assert_ok();
    assert!(
        xlsx.header("content-type")
            .is_some_and(|t| t.contains("spreadsheetml"))
    );
    w.app
        .get("/staff/reports/work-orders?export=print")
        .await
        .assert_ok();
    for path in [
        "/staff/reports/payments",
        "/staff/reports/customers",
        "/staff/reports/intercompany",
        "/staff/reports/intercompany?group=kind",
        "/staff/reports/rentals?group=owner_store",
        "/staff/reports/work-orders?group=source",
    ] {
        w.app.get(path).await.assert_ok();
    }

    // The customer's lifetime value: every stream, added up.
    let value = CustomerValue::find(w.app.db(), w.customer.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(value.sales_value, 80_000);
    assert_eq!(value.rentals_value, 120_000);
    assert_eq!(value.workshop_value, 40_000);
    assert_eq!(value.plans_value, 25_000);
    assert_eq!(value.lifetime_value, 265_000);
    assert_eq!(value.visits, 4);
}

#[renox::test]
async fn the_monthly_report_mails_one_workbook_per_store() {
    let w = world().await;
    w.app.acting_as(&w.owner);
    let page = w.app.get("/staff/reports/monthly").await;
    page.assert_ok();
    page.assert_see("No runs yet");

    // A month in the future is refused.
    let next = (renox::db::now() + renox::chrono::Duration::days(40))
        .format("%Y-%m")
        .to_string();
    w.app
        .post("/staff/reports/monthly", &[("month", &next)])
        .await
        .assert_status(303);
    assert_eq!(w.app.run_all_jobs().await, 0);

    // This month, for every store.
    let month = renox::db::now().format("%Y-%m").to_string();
    w.app
        .post("/staff/reports/monthly", &[("month", &month)])
        .await
        .assert_status(303);
    let page = w.app.get("/staff/reports/monthly").await;
    page.assert_see("Running");
    // Three workbooks side by side, then the mail.
    assert!(w.app.run_all_jobs().await >= 4);
    let page = w.app.get("/staff/reports/monthly/runs").await;
    page.assert_ok();
    page.assert_see("Mailed");

    let mails = w.app.sent_mail();
    let to = |email: &str| {
        mails
            .iter()
            .find(|m| m.to.iter().any(|t| t.contains(email)))
            .unwrap_or_else(|| panic!("a mail to {email}"))
    };
    // The owner gets every store's workbook; each manager their store's.
    let owner = to("owner@example.com");
    assert_eq!(owner.attachments.len(), 3);
    assert!(owner.subject.starts_with("Monthly report"));
    for file in &owner.attachments {
        assert!(file.filename.ends_with(".xlsx"));
        assert_eq!(&file.data[..2], b"PK", "an xlsx is a zip file");
    }
    assert_eq!(to("mn@example.com").attachments.len(), 1);
    assert_eq!(to("ms@example.com").attachments.len(), 1);
    assert!(!mails.iter().any(|m| m.to.iter().any(|t| t.contains("cn@"))));

    // The workbooks download for those who may see the store, a 404 for others.
    let first = renox::db::now().date_naive().format("%Y-%m-01").to_string();
    let first = renox::chrono::NaiveDate::parse_from_str(&first, "%Y-%m-%d").unwrap();
    assert!(
        w.app
            .state()
            .storage
            .exists(&monthly::file_key(first, w.north.id))
            .await
            .unwrap()
    );
    w.app.acting_as(&w.manager_north);
    w.app
        .get(&format!("/staff/reports/monthly/{month}/{}", w.north.id))
        .await
        .assert_ok();
    w.app
        .get(&format!("/staff/reports/monthly/{month}/{}", w.south.id))
        .await
        .assert_not_found();
}

/// The dashboard (computed afresh each time) and every grid run the same
/// number of queries with the small seed and with more data.
#[renox::test]
async fn the_pages_use_a_fixed_number_of_queries() {
    let app = TestApp::new(bikeshop::app()).await;
    let db = app.db().clone();
    seed::shop::build(&db, seed::Volume::small()).await.unwrap();
    let owner = User::query()
        .where_eq("email", "owner@bikeshop.test")
        .first(&db)
        .await
        .unwrap()
        .unwrap();
    app.acting_as(&owner);
    // The first staff request picks the active store (one query, once).
    app.get("/staff").await.assert_ok();
    let pages = [
        "/staff/reports",
        "/staff/reports?by=books",
        "/staff/reports/orders",
        "/staff/reports/rentals",
        "/staff/reports/work-orders",
        "/staff/reports/payments",
        "/staff/reports/customers",
        "/staff/reports/intercompany",
        "/staff/reports/monthly",
        "/staff",
    ];
    let mut before = Vec::new();
    let mut logs = Vec::new();
    for page in pages {
        changed(app.state()).await.unwrap();
        let (response, queries) = capture_queries(app.get(page)).await;
        response.assert_ok();
        before.push(queries.len());
        logs.push(queries);
    }
    // More of everything: a store's worth of orders, rentals and services.
    let stores = Store::all_by_name(&db).await.unwrap();
    let customer = Customer::query().first(&db).await.unwrap().unwrap();
    for store in &stores {
        let bike = fixtures::bike(&db, store.id, store.id).await.unwrap();
        for _ in 0..3 {
            orders()
                .at(store.id)
                .for_customer(customer.id)
                .totalling(10_000)
                .paid()
                .create_one(&db)
                .await
                .unwrap();
            rentals()
                .of_bike(&bike)
                .for_customer(customer.id)
                .returned()
                .create_one(&db)
                .await
                .unwrap();
        }
    }
    for ((page, few), log) in pages.iter().zip(before).zip(logs) {
        changed(app.state()).await.unwrap();
        let (response, queries) = capture_queries(app.get(page)).await;
        response.assert_ok();
        assert_eq!(few, queries.len(), "{page}: {log:#?}\n{queries:#?}");
    }
    // A cached dashboard costs only the reach and the cache.
    app.get("/staff/reports").await.assert_ok();
    let (_, cached) = capture_queries(app.get("/staff/reports")).await;
    let (_, fresh) = {
        changed(app.state()).await.unwrap();
        capture_queries(app.get("/staff/reports")).await
    };
    assert!(
        cached.len() + 15 < fresh.len(),
        "{} vs {}",
        cached.len(),
        fresh.len()
    );
}
