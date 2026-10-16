//! Cart, checkout, payments and counter sales (#234).

use bikeshop::app::catalog::model::{Brand, Category, CategoryKind, Product, ProductVariant};
use bikeshop::app::sales::cart::Cart;
use bikeshop::app::sales::model::SavedCart;
use bikeshop::app::staff::model::Store;
use bikeshop::app::stock::model::StockLevel;
use bikeshop::seed::fixtures;
use renox::prelude::*;
use renox::testing::TestApp;

/// Two stores and a few products with known stock.
struct World {
    north: Store,
    south: Store,
    /// A helmet: 3 at North, 1 at South.
    helmet: ProductVariant,
    /// A chain (a part): 10 at North.
    chain: ProductVariant,
    /// A bike: 1 at North.
    bike: ProductVariant,
}

async fn variant(db: &Db, name: &str, kind: CategoryKind, price: i64) -> ProductVariant {
    let n = bikeshop::seed::unique();
    let category = Category::create(
        db,
        Category {
            name: format!("{name} category {n}"),
            slug: format!("cat-{n}"),
            kind,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let brand = Brand::create(
        db,
        Brand {
            name: format!("Brand {n}"),
            slug: format!("brand-{n}"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let product = Product::create(
        db,
        Product {
            category_id: category.id,
            brand_id: brand.id,
            name: name.into(),
            slug: format!("{}-{n}", name.to_lowercase().replace(' ', "-")),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    ProductVariant::create(
        db,
        ProductVariant {
            product_id: product.id,
            sku: format!("SKU-{n}"),
            price,
            cost: price / 2,
            ..Default::default()
        },
    )
    .await
    .unwrap()
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

async fn world(app: &TestApp) -> World {
    let db = app.db();
    let north = fixtures::store(db, "North").await.unwrap();
    let south = fixtures::store(db, "South").await.unwrap();
    let helmet = variant(db, "Helmet", CategoryKind::Gear, 9_000).await;
    let chain = variant(db, "Chain", CategoryKind::Part, 3_000).await;
    let bike = variant(db, "Road bike", CategoryKind::Bike, 120_000).await;
    stock(db, helmet.id, north.id, 3).await;
    stock(db, helmet.id, south.id, 1).await;
    stock(db, chain.id, north.id, 10).await;
    stock(db, bike.id, north.id, 1).await;
    World {
        north,
        south,
        helmet,
        chain,
        bike,
    }
}

fn add(variant: &ProductVariant, quantity: i64) -> Vec<(&'static str, String)> {
    vec![
        ("variant_id", variant.id.to_string()),
        ("quantity", quantity.to_string()),
    ]
}

async fn post(
    app: &TestApp,
    uri: &str,
    form: Vec<(&'static str, String)>,
) -> renox::testing::TestResponse {
    let pairs: Vec<(&str, &str)> = form.iter().map(|(k, v)| (*k, v.as_str())).collect();
    app.post(uri, &pairs).await
}

#[renox::test]
async fn the_cart_holds_no_more_than_the_store_has() {
    let app = TestApp::new(bikeshop::app()).await;
    let w = world(&app).await;
    // The first store (North) by default: 3 helmets there.
    post(&app, "/cart", add(&w.helmet, 2))
        .await
        .assert_status(303);
    post(&app, "/cart", add(&w.helmet, 5))
        .await
        .assert_status(303);
    let cart: Cart = app.session_get("cart").unwrap();
    assert_eq!(cart.store_id, Some(w.north.id));
    assert_eq!(cart.lines[0].quantity, 3, "held to what North has");
    // Nothing left to add: a warning, the cart unchanged.
    let res = app
        .htmx()
        .post(
            "/cart",
            &[("variant_id", &w.helmet.id.to_string()), ("quantity", "1")],
        )
        .await;
    res.assert_ok();
    assert!(
        res.header("hx-trigger")
            .unwrap_or_default()
            .contains("There are no more")
    );
    // The quantity rules: 1 to 20.
    app.htmx()
        .post(
            "/cart",
            &[("variant_id", &w.helmet.id.to_string()), ("quantity", "0")],
        )
        .await
        .assert_invalid("quantity");

    // South has one: switching stores lowers the line and says so.
    app.post("/cart/store", &[("store_id", &w.south.id.to_string())])
        .await
        .assert_status(303);
    let page = app.get("/cart").await;
    page.assert_ok().assert_see("Helmet").assert_see("South");
    let cart: Cart = app.session_get("cart").unwrap();
    assert_eq!(cart.lines[0].quantity, 1);

    // Someone else buys it: the next visit lowers the line to zero and drops it.
    StockLevel::query()
        .where_(StockLevel::VARIANT_ID.eq(w.helmet.id))
        .where_(StockLevel::LOCATION_STORE_ID.eq(w.south.id))
        .update(app.db(), &[("reserved", &1)])
        .await
        .unwrap();
    app.get("/cart")
        .await
        .assert_see("Some quantities changed")
        .assert_see("Your cart is empty");
    let _ = (w.chain, w.bike);
}

#[renox::test]
async fn htmx_changes_answer_the_lines_and_the_navbar_count() {
    let app = TestApp::new(bikeshop::app()).await;
    let w = world(&app).await;
    // Adding from a product page: the navbar's count, out of band, and a toast.
    let added = app
        .htmx()
        .post(
            "/cart",
            &[("variant_id", &w.chain.id.to_string()), ("quantity", "2")],
        )
        .await;
    let text = added.assert_ok().text();
    assert!(
        text.contains("id=\"nav-cart\"") && text.contains("hx-swap-oob=\"true\""),
        "{text}"
    );
    assert!(text.contains(">2</span>"), "the count: {text}");
    assert!(
        added
            .header("hx-trigger")
            .unwrap_or_default()
            .contains("renox:toast")
    );
    // Changing a quantity on the cart page: the lines plus the count.
    let changed = app
        .htmx()
        .patch(&format!("/cart/{}", w.chain.id), &[("quantity", "4")])
        .await;
    let text = changed.assert_ok().text();
    assert!(
        text.contains("id=\"cart\"") && text.contains("id=\"nav-cart\""),
        "{text}"
    );
    assert!(text.contains(">4</span>"));
    // The navbar asks for its count when a page loads.
    app.htmx()
        .get("/cart/mini")
        .await
        .assert_ok()
        .assert_see(">4</span>");
    app.htmx()
        .delete(&format!("/cart/{}", w.chain.id))
        .await
        .assert_ok()
        .assert_see("Your cart is empty");
}

#[renox::test]
async fn a_guest_cart_joins_the_saved_cart_after_logging_in() {
    let app = TestApp::new(bikeshop::app()).await;
    let w = world(&app).await;
    let db = app.db();
    let user = User::register(db, "Ana", "ana@example.com", "password123")
        .await
        .unwrap();
    // Saved from an earlier visit on another device: one chain.
    let mut saved = Cart::default();
    saved.add(w.chain.id, 1);
    SavedCart::create(
        db,
        SavedCart {
            user_id: user.id,
            store_id: Some(w.north.id),
            lines: renox::db::Json(saved.lines.clone()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    // As a guest: two helmets and one more chain.
    post(&app, "/cart", add(&w.helmet, 2)).await;
    post(&app, "/cart", add(&w.chain, 1)).await;
    app.acting_as(&user);
    app.get("/cart")
        .await
        .assert_ok()
        .assert_see("Helmet")
        .assert_see("Chain");
    app.assert_session_missing("cart");
    let row = SavedCart::query()
        .where_(SavedCart::USER_ID.eq(user.id))
        .first(db)
        .await
        .unwrap()
        .unwrap();
    let mut lines = row.lines.0.clone();
    lines.sort_by_key(|l| l.variant_id);
    let mut want = vec![(w.helmet.id, 2), (w.chain.id, 2)];
    want.sort();
    assert_eq!(
        lines
            .iter()
            .map(|l| (l.variant_id, l.quantity))
            .collect::<Vec<_>>(),
        want
    );
    // From now on, the account's cart is used (another device sees it too).
    post(&app, "/cart", add(&w.helmet, 1)).await;
    let row = SavedCart::query()
        .where_(SavedCart::USER_ID.eq(user.id))
        .first(db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.lines
            .0
            .iter()
            .find(|l| l.variant_id == w.helmet.id)
            .unwrap()
            .quantity,
        3
    );
    let _ = w.bike;
}

// ---------------------------------------------------------------------------
// Checkout, payments, the order's life, the counter.

use bikeshop::app::access::catalogue::{CASHIER, MANAGER};
use bikeshop::app::accounts::model::Customer;
use bikeshop::app::plans::factories::{SubscriptionStates, plan_subscriptions};
use bikeshop::app::plans::model::ServicePlan;
use bikeshop::app::sales::gateway::{self, Notification};
use bikeshop::app::sales::ledger;
use bikeshop::app::sales::model::{
    Channel, Order, OrderItem, OrderStatus, Payment, PaymentMethod, PaymentStatus,
};
use bikeshop::app::stock::model::{MovementReason, StockMovement};
use bikeshop::app::workshop::model::CustomerBike;
use std::time::Duration;

/// The checkout form for a pickup at `store`.
fn pickup(store: &Store) -> Vec<(&'static str, String)> {
    vec![
        ("name", "Ana Ruiz".into()),
        ("email", "Ana@Example.com ".into()),
        ("phone", "+62 812 3456 7890".into()),
        ("fulfilment", "pickup".into()),
        ("store_id", store.id.to_string()),
    ]
}

/// The one level of `variant` at `store`: (on hand, reserved).
async fn level(app: &TestApp, variant: i64, store: i64) -> (i64, i64) {
    let l = StockLevel::query()
        .where_(StockLevel::VARIANT_ID.eq(variant))
        .where_(StockLevel::LOCATION_STORE_ID.eq(store))
        .first(app.db())
        .await
        .unwrap()
        .unwrap();
    (l.on_hand, l.reserved)
}

/// The order's ledger: (reason, quantity) in order.
async fn ledger_of(app: &TestApp, order: i64) -> Vec<(MovementReason, i64)> {
    StockMovement::query()
        .where_(StockMovement::REFERENCE_TYPE.eq("orders"))
        .where_(StockMovement::REFERENCE_ID.eq(order))
        .order_by(StockMovement::ID)
        .get(app.db())
        .await
        .unwrap()
        .into_iter()
        .map(|m| (m.reason, m.quantity))
        .collect()
}

/// Sends the gateway's signed notification for the order's pending payment.
async fn gateway_says(app: &TestApp, order: &Order, status: &str) -> renox::testing::TestResponse {
    let payment = Payment::query()
        .where_(Payment::PAYABLE_TYPE.eq("orders"))
        .where_(Payment::PAYABLE_ID.eq(order.id))
        .order_by_desc(Payment::ID)
        .first(app.db())
        .await
        .unwrap()
        .unwrap();
    let n = Notification::signed(&payment, status, &gateway::server_key(app.state()));
    app.post_body(
        "/webhooks/midtrans",
        "application/json",
        renox::serde_json::to_vec(&n).unwrap(),
    )
    .await
}

async fn last_order(app: &TestApp) -> Order {
    Order::query()
        .order_by_desc(Order::ID)
        .first(app.db())
        .await
        .unwrap()
        .unwrap()
}

#[renox::test]
async fn checkout_validates_and_keeps_the_old_input() {
    let app = TestApp::new(bikeshop::app()).await;
    let w = world(&app).await;
    // An empty cart goes back to the cart.
    app.get("/checkout").await.assert_redirect("/cart");
    post(&app, "/cart", add(&w.chain, 1)).await;
    app.get("/checkout")
        .await
        .assert_ok()
        .assert_see("data-rx-wizard")
        .assert_see("data-live-validate");
    // htmx: a 422 with each field's errors.
    let res = app
        .htmx()
        .post(
            "/checkout",
            &[
                ("fulfilment", "delivery"),
                ("email", "nope"),
                ("phone", "abc"),
            ],
        )
        .await;
    res.assert_invalid("name")
        .assert_invalid("email")
        .assert_invalid("phone")
        .assert_invalid("city_id")
        .assert_invalid("line1")
        .assert_invalid("store_id");
    // Live validation of one field: the same rules, the handler doesn't run.
    let live = app
        .request()
        .header("x-renox-validate", "email")
        .htmx()
        .post("/checkout", &[("email", "ana@example")])
        .await;
    let answer: renox::serde_json::Value = live.assert_ok().json();
    assert_eq!(answer["field"], "email");
    assert!(!answer["errors"].as_array().unwrap().is_empty());
    assert_eq!(Order::query().count(app.db()).await.unwrap(), 0);
    // A plain post goes back with the errors and the old input.
    app.post("/checkout", &[("name", "Ana"), ("fulfilment", "pickup")])
        .await
        .assert_status(303);
    app.get("/checkout").await.assert_see("value=\"Ana\"");
}

#[renox::test]
async fn a_guest_order_is_reserved_then_paid_by_the_signed_webhook() {
    let app = TestApp::new(bikeshop::app()).await;
    let w = world(&app).await;
    post(&app, "/cart", add(&w.bike, 1)).await;
    post(&app, "/cart", add(&w.chain, 2)).await;
    let res = post(&app, "/checkout", pickup(&w.north)).await;
    res.assert_status(303);
    let to = res.header("location").unwrap().to_owned();
    assert!(
        to.contains("/pay/demo/") && to.contains("signature="),
        "{to}"
    );

    // Placed: pending, the units put aside, a pending payment.
    let order = last_order(&app).await;
    assert_eq!(order.status, OrderStatus::Pending);
    assert_eq!(order.channel, Channel::Online);
    assert_eq!(order.total, 120_000 + 2 * 3_000);
    assert_eq!(order.locale.as_deref(), Some("en"));
    assert_eq!(level(&app, w.bike.id, w.north.id).await, (1, 1));
    assert_eq!(level(&app, w.chain.id, w.north.id).await, (10, 2));
    assert_eq!(
        ledger_of(&app, order.id).await,
        [(MovementReason::Reserved, 1), (MovementReason::Reserved, 2)]
    );
    let customer = Customer::find(app.db(), order.customer_id.unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        customer.email.as_deref(),
        Some("ana@example.com"),
        "tidied by `prepare`"
    );
    app.assert_session_missing("cart");
    // The demo gateway's page needs its signature.
    app.get(&to)
        .await
        .assert_ok()
        .assert_see("not a real payment");
    let unsigned = to.split('?').next().unwrap().to_owned();
    app.get(&unsigned).await.assert_status(403);
    // The return page waits.
    let payment_id: i64 = to
        .rsplit('/')
        .next()
        .unwrap()
        .split('?')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    app.get(&format!("/pay/{payment_id}"))
        .await
        .assert_see("Waiting for the payment");

    // A wrong signature is refused, nothing stored.
    let mut forged = Notification::signed(
        &Payment::find(app.db(), payment_id).await.unwrap().unwrap(),
        "settlement",
        "not-the-key",
    );
    forged.order_id = gateway::reference(payment_id);
    app.post_body(
        "/webhooks/midtrans",
        "application/json",
        renox::serde_json::to_vec(&forged).unwrap(),
    )
    .await
    .assert_status(401);
    app.run_jobs().await;
    assert_eq!(last_order(&app).await.status, OrderStatus::Pending);

    // The real one, sent twice: handled once.
    gateway_says(&app, &order, "settlement").await.assert_ok();
    let payment = Payment::find(app.db(), payment_id).await.unwrap().unwrap();
    let again = Notification {
        transaction_id: "same".into(),
        ..Notification::signed(&payment, "settlement", &gateway::server_key(app.state()))
    };
    for _ in 0..2 {
        app.post_body(
            "/webhooks/midtrans",
            "application/json",
            renox::serde_json::to_vec(&again).unwrap(),
        )
        .await
        .assert_ok();
    }
    app.run_jobs().await;
    let calls: i64 =
        renox::db::sql("SELECT COUNT(*) FROM webhook_calls WHERE event_id = 'same:settlement'")
            .scalar(app.db())
            .await
            .unwrap();
    assert_eq!(calls, 1, "the second delivery isn't stored");

    // Paid: the reservation became a sale, once.
    let order = last_order(&app).await;
    assert_eq!(order.status, OrderStatus::Paid);
    assert_eq!(level(&app, w.bike.id, w.north.id).await, (0, 0));
    assert_eq!(level(&app, w.chain.id, w.north.id).await, (8, 0));
    assert_eq!(
        ledger_of(&app, order.id).await,
        [
            (MovementReason::Reserved, 1),
            (MovementReason::Reserved, 2),
            (MovementReason::Released, 1),
            (MovementReason::Sale, -1),
            (MovementReason::Released, 2),
            (MovementReason::Sale, -2),
        ]
    );
    let payment = Payment::find(app.db(), payment_id).await.unwrap().unwrap();
    assert_eq!(payment.status, PaymentStatus::Paid);
    assert_eq!(payment.method, PaymentMethod::Gateway);
    // The bike is now one of the customer's bikes, waiting for its frame number.
    let bikes = CustomerBike::query()
        .where_(CustomerBike::ORDER_ID.eq(order.id))
        .get(app.db())
        .await
        .unwrap();
    assert_eq!(bikes.len(), 1);
    assert_eq!(bikes[0].customer_id, customer.id);
    // The confirmation, from the queue.
    let mails = app.sent_mail();
    assert!(
        mails.iter().any(|m| m.to == ["ana@example.com"]
            && m.subject.contains(&order.number)
            && m.subject.contains("paid")),
        "{:?}",
        mails.iter().map(|m| &m.subject).collect::<Vec<_>>()
    );
    // Back from the gateway: paid, the order, and an account offered.
    app.get(&format!("/pay/{payment_id}"))
        .await
        .assert_see("Thank you")
        .assert_see(&order.number)
        .assert_see("Create an account");
    app.get(&format!("/orders/{}", order.id))
        .await
        .assert_ok()
        .assert_see("Road bike");
    app.get(&format!("/orders/{}/invoice", order.id))
        .await
        .assert_ok()
        .assert_see("Print")
        // An online order stays an invoice on a sheet, not a till receipt.
        .assert_dont_see("rx-receipt");
    // Someone else's browser doesn't see it.
    app.logout();
    app.get(&format!("/orders/{}", order.id))
        .await
        .assert_not_found();
    app.get(&format!("/pay/{payment_id}"))
        .await
        .assert_not_found();
    // The mail's signed link opens it.
    let link = app
        .state()
        .signed_url("orders.signed", &[&order.id], Duration::from_secs(60))
        .unwrap();
    let path = link.trim_start_matches(&app.state().config.url).to_owned();
    app.get(&path)
        .await
        .assert_redirect(&format!("/orders/{}", order.id));
    app.get(&format!("/orders/{}", order.id)).await.assert_ok();
}

#[renox::test]
async fn the_last_unit_goes_to_one_of_two_concurrent_checkouts() {
    let app = TestApp::new(bikeshop::app()).await;
    let w = world(&app).await;
    let db = app.db().clone();
    let levels = ledger::levels_at(&db, w.north.id, &[w.bike.id])
        .await
        .unwrap();
    let attempt = |n: i64| {
        let (db, levels) = (db.clone(), levels.clone());
        let (store, bike) = (w.north.id, w.bike.id);
        async move {
            let mut tx = db.begin().await.unwrap();
            let mut order = Order {
                number: format!("RACE-{n}"),
                operating_store_id: store,
                ..Default::default()
            };
            order.insert(&mut tx).await.unwrap();
            let got = ledger::reserve(&mut tx, order.id, store, &[(bike, 1)], &levels, None)
                .await
                .unwrap();
            match got {
                Ok(_) => {
                    tx.commit().await.unwrap();
                    true
                }
                Err(short) => {
                    assert_eq!((short.wanted, short.left), (1, 0));
                    tx.rollback().await.unwrap();
                    false
                }
            }
        }
    };
    let (a, b) = renox::tokio::join!(attempt(1), attempt(2));
    assert!(a ^ b, "exactly one wins: {a} {b}");
    assert_eq!(level(&app, w.bike.id, w.north.id).await, (1, 1));
    assert_eq!(
        Order::query().count(&db).await.unwrap(),
        1,
        "the loser's order rolled back"
    );

    // Over HTTP: a cart with the bike, then it is gone: the cart says so.
    post(&app, "/cart", add(&w.chain, 1)).await;
    StockLevel::query()
        .where_(StockLevel::VARIANT_ID.eq(w.chain.id))
        .update(&db, &[("reserved", &10)])
        .await
        .unwrap();
    post(&app, "/checkout", pickup(&w.north))
        .await
        .assert_redirect("/cart");
    assert_eq!(Order::query().count(&db).await.unwrap(), 1);
}

#[renox::test]
async fn unpaid_orders_expire_after_thirty_minutes_and_release_the_stock() {
    let app = TestApp::new(bikeshop::app()).await;
    let w = world(&app).await;
    post(&app, "/cart", add(&w.helmet, 2)).await;
    post(&app, "/checkout", pickup(&w.north))
        .await
        .assert_status(303);
    let order = last_order(&app).await;
    assert_eq!(level(&app, w.helmet.id, w.north.id).await, (3, 2));

    // 29 minutes: still held.
    app.travel(Duration::from_secs(29 * 60));
    app.run_scheduled("sales:expire-orders").await.unwrap();
    assert_eq!(last_order(&app).await.status, OrderStatus::Pending);
    // 31 minutes: cancelled, released, the payment failed, the customer told.
    app.travel(Duration::from_secs(2 * 60));
    app.run_scheduled("sales:expire-orders").await.unwrap();
    app.run_jobs().await;
    let order = Order::find(app.db(), order.id).await.unwrap().unwrap();
    assert_eq!(order.status, OrderStatus::Cancelled);
    assert_eq!(level(&app, w.helmet.id, w.north.id).await, (3, 0));
    assert_eq!(
        ledger_of(&app, order.id).await,
        [(MovementReason::Reserved, 2), (MovementReason::Released, 2)]
    );
    let payment = Payment::query()
        .where_(Payment::PAYABLE_ID.eq(order.id))
        .first(app.db())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(payment.status, PaymentStatus::Failed);
    assert!(
        app.sent_mail()
            .iter()
            .any(|m| m.subject.contains("was cancelled"))
    );
    // A late payment changes nothing: the order stays cancelled.
    gateway_says(&app, &order, "settlement").await.assert_ok();
    app.run_jobs().await;
    assert_eq!(
        Order::find(app.db(), order.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        OrderStatus::Cancelled
    );
    assert_eq!(level(&app, w.helmet.id, w.north.id).await, (3, 0));
}

#[renox::test]
async fn customers_get_their_cart_mails_notifications_and_the_plan_discount() {
    let app = TestApp::new(bikeshop::app()).await;
    let w = world(&app).await;
    let db = app.db();
    let user = User::register(db, "Rider", "rider@example.com", "password123")
        .await
        .unwrap();
    // A customer with a bike on a service plan: 10 % off spare parts.
    let me = Customer::create(
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
    let mine = CustomerBike::create(
        db,
        CustomerBike {
            customer_id: me.id,
            name: "My bike".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let plan = ServicePlan::factory().create_one(db).await.unwrap();
    plan_subscriptions()
        .of(mine.id, plan.id, w.north.id)
        .create_one(db)
        .await
        .unwrap();
    app.acting_as(&user);
    post(&app, "/cart", add(&w.chain, 1)).await;
    post(&app, "/cart", add(&w.helmet, 1)).await;
    app.get("/checkout").await.assert_see("10% off parts");
    post(&app, "/checkout", pickup(&w.north))
        .await
        .assert_status(303);
    let order = last_order(&app).await;
    assert_eq!(order.customer_id, Some(me.id), "the account's customer");
    assert_eq!(order.discount, 300, "10 % of the chain, not of the helmet");
    assert_eq!(order.total, 3_000 + 9_000 - 300);
    gateway_says(&app, &order, "settlement").await.assert_ok();
    app.run_jobs().await;
    let notes: i64 = renox::db::sql("SELECT COUNT(*) FROM notifications WHERE user_id = ?")
        .bind(user.id)
        .scalar(db)
        .await
        .unwrap();
    assert_eq!(notes, 1, "an in-app notification for the account");
    app.get(&format!("/orders/{}", order.id)).await.assert_ok();
    let _ = w.south;
}

#[renox::test]
async fn staff_move_orders_on_and_take_returns_within_fourteen_days() {
    let app = TestApp::new(bikeshop::app()).await;
    let w = world(&app).await;
    let db = app.db();
    fixtures::roles(db).await.unwrap();
    let cashier = fixtures::person(db, "cashier@example.com", &[(CASHIER, Some(w.north.id))])
        .await
        .unwrap();
    let manager = fixtures::person(db, "manager@example.com", &[(MANAGER, Some(w.north.id))])
        .await
        .unwrap();
    let other = fixtures::person(db, "south@example.com", &[(MANAGER, Some(w.south.id))])
        .await
        .unwrap();

    post(&app, "/cart", add(&w.bike, 1)).await;
    post(&app, "/cart", add(&w.chain, 3)).await;
    post(&app, "/checkout", pickup(&w.north)).await;
    let order = last_order(&app).await;
    gateway_says(&app, &order, "settlement").await;
    app.run_jobs().await;
    let shown = app.sent_mail().len();

    // Another store's manager doesn't even see it.
    app.acting_as(&other);
    app.get(&format!("/staff/orders/{}", order.id))
        .await
        .assert_not_found();

    app.acting_as(&cashier);
    app.get("/staff/orders")
        .await
        .assert_ok()
        .assert_see(&order.number);
    app.get(&format!("/staff/orders/{}", order.id))
        .await
        .assert_ok()
        .assert_see("Ready for pickup");
    app.post(&format!("/staff/orders/{}/ready", order.id), &[])
        .await
        .assert_status(303);
    assert_eq!(
        Order::find(db, order.id).await.unwrap().unwrap().status,
        OrderStatus::Ready
    );
    app.run_jobs().await;
    assert!(
        app.sent_mail()[shown..]
            .iter()
            .any(|m| m.subject.contains("ready for pickup"))
    );
    // The handover, with the bike's frame number.
    let bike = CustomerBike::query()
        .where_(CustomerBike::ORDER_ID.eq(order.id))
        .first(db)
        .await
        .unwrap()
        .unwrap();
    app.htmx()
        .post(
            &format!("/staff/orders/{}/complete", order.id),
            &[
                ("frames[0][bike_id]", &bike.id.to_string()),
                ("frames[0][number]", " wtu123 "),
            ],
        )
        .await
        .assert_ok();
    let order = Order::find(db, order.id).await.unwrap().unwrap();
    assert_eq!(order.status, OrderStatus::Completed);
    assert!(order.completed_at.is_some());
    assert_eq!(
        CustomerBike::find(db, bike.id)
            .await
            .unwrap()
            .unwrap()
            .frame_number
            .as_deref(),
        Some("WTU123")
    );

    // A cashier may not refund.
    let chain_line = OrderItem::query()
        .where_(OrderItem::ORDER_ID.eq(order.id))
        .where_(OrderItem::VARIANT_ID.eq(w.chain.id))
        .first(db)
        .await
        .unwrap()
        .unwrap();
    let back = |q: &str| -> Vec<(String, String)> {
        vec![
            ("lines[0][item_id]".into(), chain_line.id.to_string()),
            ("lines[0][quantity]".into(), q.into()),
            ("reason".into(), "Wrong speed".into()),
        ]
    };
    let send = |form: Vec<(String, String)>| {
        let app = &app;
        let uri = format!("/staff/orders/{}/return", order.id);
        async move {
            let pairs: Vec<(&str, &str)> =
                form.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            app.post(&uri, &pairs).await
        }
    };
    send(back("2")).await.assert_forbidden();

    // The manager takes two chains back: stock, ledger, refund, mail.
    app.acting_as(&manager);
    let before = level(&app, w.chain.id, w.north.id).await;
    send(back("2")).await.assert_status(303);
    assert_eq!(
        level(&app, w.chain.id, w.north.id).await,
        (before.0 + 2, before.1)
    );
    let ledger = ledger_of(&app, order.id).await;
    assert_eq!(ledger.last(), Some(&(MovementReason::Return, 2)));
    let refund = Payment::query()
        .where_(Payment::PAYABLE_ID.eq(order.id))
        .where_(Payment::STATUS.eq(PaymentStatus::Refunded))
        .first(db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(refund.amount, 2 * 3_000);
    let order = Order::find(db, order.id).await.unwrap().unwrap();
    assert_eq!(
        order.status,
        OrderStatus::Completed,
        "the bike wasn't returned"
    );
    assert!(order.returned_at.is_some());
    app.run_jobs().await;
    assert!(app.sent_mail().iter().any(|m| m.subject.contains("refund")));
    // Can't return more than was sold.
    send(back("5")).await.assert_status(303);
    assert_eq!(
        level(&app, w.chain.id, w.north.id).await,
        (before.0 + 3, before.1)
    );
    // After 14 days, no returns.
    app.travel(Duration::from_secs(15 * 24 * 3600));
    app.acting_as(&manager); // the session ran out on the way
    send(back("1")).await.assert_status(409);
}

#[renox::test]
async fn a_counter_sale_follows_the_same_rules() {
    let app = TestApp::new(bikeshop::app()).await;
    let w = world(&app).await;
    let db = app.db();
    fixtures::roles(db).await.unwrap();
    let cashier = fixtures::person(db, "cashier@example.com", &[(CASHIER, Some(w.north.id))])
        .await
        .unwrap();
    // Customers can't reach it.
    app.get("/staff/counter").await.assert_redirect("/login");
    app.acting_as(&cashier);
    app.get("/staff/counter")
        .await
        .assert_ok()
        .assert_see("data-rx-key=\"p\"");
    // The product search finds the SKU first.
    let found = app
        .request()
        .json()
        .get(&format!("/staff/counter/variants?q={}", w.helmet.sku))
        .await;
    let options: renox::serde_json::Value = found.assert_ok().json();
    assert_eq!(options[0]["value"], w.helmet.id.to_string());
    assert!(options[0]["label"].as_str().unwrap().contains("3 here"));

    app.post(
        "/staff/counter/lines",
        &[("variant_id", &w.helmet.id.to_string()), ("quantity", "2")],
    )
    .await
    .assert_redirect("/staff/counter");
    // Not enough cash: refused, nothing sold.
    app.htmx()
        .post(
            "/staff/counter/pay",
            &[("method", "cash"), ("tendered", "100")],
        )
        .await
        .assert_invalid("tendered");
    let res = app
        .post(
            "/staff/counter/pay",
            &[("method", "cash"), ("tendered", "200.50")],
        )
        .await;
    res.assert_status(303);
    let order = last_order(&app).await;
    assert!(
        res.header("location")
            .unwrap()
            .starts_with(&format!("/orders/{}/invoice", order.id))
    );
    assert_eq!(order.channel, Channel::Counter);
    assert_eq!(order.status, OrderStatus::Completed);
    assert_eq!(level(&app, w.helmet.id, w.north.id).await, (1, 0));
    let payment = Payment::query()
        .where_(Payment::PAYABLE_ID.eq(order.id))
        .first(db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (payment.method, payment.status, payment.amount),
        (PaymentMethod::Cash, PaymentStatus::Paid, 18_000)
    );
    app.get(&format!("/orders/{}/invoice?receipt=1", order.id))
        .await
        .assert_ok()
        .assert_see("Receipt")
        .assert_see("Change to give")
        // $200.50 handed over for $180.00: the change is in cents too.
        .assert_see("$20.50")
        // The counter's receipt is the `receipt` block (#48): 80 mm, the
        // order's number in its code slot, and the total of $180.00.
        .assert_see(r#"class="rx-receipt rx-receipt--w80" id="counter-receipt""#)
        .assert_see(&format!(
            r#"<span class="bs-receipt__code">{}</span>"#,
            order.number
        ))
        .assert_see(r#"<th scope="row">Total</th><td>$180.00</td>"#);
}

#[renox::test]
async fn a_guest_who_registers_finds_their_orders() {
    let app = TestApp::new(bikeshop::app()).await;
    let w = world(&app).await;
    post(&app, "/cart", add(&w.chain, 1)).await;
    post(&app, "/checkout", pickup(&w.north)).await;
    let order = last_order(&app).await;
    app.logout();
    app.post(
        "/register",
        &[
            ("name", "Ana Ruiz"),
            ("email", "ana@example.com"),
            ("password", "a-long-password-1"),
            ("password_confirmation", "a-long-password-1"),
        ],
    )
    .await;
    let user = User::find_by_email(app.db(), "ana@example.com")
        .await
        .unwrap()
        .unwrap();
    // Not before the address is verified: anyone could register with it.
    let guest = Customer::find(app.db(), order.customer_id.unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(guest.user_id, None);
    let verify = app
        .state()
        .signed_url(
            "verification.verify",
            &[&user.id, &renox::webhook::sha256_hex(&user.email)],
            std::time::Duration::from_secs(3600),
        )
        .unwrap();
    app.acting_as(&user);
    app.get(&verify).await;
    let customer = Customer::find(app.db(), order.customer_id.unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(customer.user_id, Some(user.id));
    app.get(&format!("/orders/{}", order.id)).await.assert_ok();
}

#[renox::test]
async fn the_mails_page_previews_every_mail() {
    let app = TestApp::new(bikeshop::app()).await;
    app.get("/sales/mails")
        .await
        .assert_ok()
        .assert_see("N-DEMO0001 is paid")
        .assert_see("ready for pickup")
        .assert_see("srcdoc=");
}

#[renox::test]
async fn midtrans_is_asked_for_its_page_when_configured() {
    let app = TestApp::with_config(bikeshop::app(), |c| {
        c.vars
            .insert("MIDTRANS_SERVER_KEY".into(), "SB-test-key".into());
    })
    .await;
    let w = world(&app).await;
    app.fake_http().on(
        "POST https://app.sandbox.midtrans.com/snap/v1/transactions",
        renox::http::FakeResponse::json(
            201,
            renox::serde_json::json!({ "redirect_url": "https://pay.example/snap/abc" }),
        ),
    );
    post(&app, "/cart", add(&w.chain, 1)).await;
    post(&app, "/checkout", pickup(&w.north))
        .await
        .assert_redirect("https://pay.example/snap/abc");
    // The webhook checks Midtrans' own signature with the server key.
    let order = last_order(&app).await;
    gateway_says(&app, &order, "settlement").await.assert_ok();
    app.run_jobs().await;
    assert_eq!(last_order(&app).await.status, OrderStatus::Paid);
}

#[renox::test]
async fn the_cart_speaks_spanish_and_explains_itself() {
    let app = TestApp::new(bikeshop::app()).await;
    app.request()
        .header("accept-language", "es")
        .get("/cart")
        .await
        .assert_ok()
        .assert_see("Tu carrito está vacío")
        .assert_see("Lo que el comprador va a comprar");
}

#[renox::test]
async fn the_staff_order_page_names_everyone_who_hands_orders_over() {
    use bikeshop::app::access::catalogue::{ORDERS_SELL, STAFF};
    use bikeshop::explain::{self, Audience};

    // Floor staff hold `orders.sell`, so they hand orders over too.
    let staff = bikeshop::app::access::catalogue::roles()
        .into_iter()
        .find(|r| r.name == STAFF)
        .expect("the staff role");
    assert!(staff.permissions.contains(&ORDERS_SELL));
    let page = explain::for_route("sales.orders.show").unwrap();
    assert!(page.audience.contains(&Audience::Staff));
    assert!(page.who.contains("floor staff") && page.who.contains("`orders.sell`"));
    let es: renox::serde_json::Value =
        renox::serde_json::from_str(&std::fs::read_to_string("resources/lang/es.json").unwrap())
            .unwrap();
    let who = es["about_page"]["sales.orders.show"]["who"]
        .as_str()
        .unwrap();
    assert!(who.contains("personal de tienda") && who.contains("`orders.sell`"));
}
