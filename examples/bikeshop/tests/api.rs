//! The JSON API (#241): kiosk tokens made by managers, shown once and
//! revoked; abilities (403), no token (401) and another store's data
//! (404); a kiosk's check-out and return matching the counter's; the
//! customer API's catalogue (pagination with links), reservations with the
//! website's rules (422) and cancellation; CORS; throttling (429); and
//! every `/api/v1` route explained on `/about/api`.

use std::collections::HashSet;
use std::time::Duration;

use bikeshop::app::access::catalogue::{CASHIER, MANAGER};
use bikeshop::app::accounts::model::Customer;
use bikeshop::app::api::endpoints::ENDPOINTS;
use bikeshop::app::api::kiosk::Kiosk;
use bikeshop::app::api::{APP_ORIGIN, PER_MINUTE};
use bikeshop::app::catalog::factories::{ProductStates, products, variants_of};
use bikeshop::app::catalog::model::{Brand, Category};
use bikeshop::app::rentals::booking::{self, NewRental};
use bikeshop::app::rentals::model::{DepositStatus, Rental, RentalBike, RentalStatus};
use bikeshop::app::sales::model::Payment;
use bikeshop::app::staff::model::Store;
use bikeshop::seed::{fixtures, unique};
use renox::db::Json as DbJson;
use renox::prelude::*;
use renox::serde_json::Value;
use renox::testing::{TestApp, TestResponse};

const HOUR: Duration = Duration::from_secs(60 * 60);

struct World {
    app: TestApp,
    north: Store,
    south: Store,
    manager: User,
    cashier: User,
    rider: User,
    customer: Customer,
}

async fn world() -> World {
    let app = TestApp::new(bikeshop::app()).await;
    let db = app.db();
    fixtures::roles(db).await.unwrap();
    let north = fixtures::store(db, "North").await.unwrap();
    let south = fixtures::store(db, "South").await.unwrap();
    let manager = fixtures::person(db, "manager@example.com", &[(MANAGER, Some(north.id))])
        .await
        .unwrap();
    let cashier = fixtures::person(db, "cashier@example.com", &[(CASHIER, Some(north.id))])
        .await
        .unwrap();
    let rider = User::register(db, "Rider", "rider@example.com", "a long password")
        .await
        .unwrap();
    let customer = Customer::create(
        db,
        Customer {
            user_id: Some(rider.id),
            name: "Rider".into(),
            email: Some("rider@example.com".into()),
            id_verified_at: Some(renox::db::now()),
            active: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    World {
        app,
        north,
        south,
        manager,
        cashier,
        rider,
        customer,
    }
}

/// A kiosk of `store` with `abilities` and its plain token, made the way
/// the staff page makes one.
async fn kiosk(w: &World, store: &Store, abilities: &[&str]) -> String {
    let db = w.app.db();
    let mut kiosk = Kiosk::create(
        db,
        Kiosk {
            store_id: store.id,
            name: "Kiosk".into(),
            abilities: DbJson(abilities.iter().map(|a| (*a).to_owned()).collect()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let token = renox::auth::DeviceToken::create(
        db,
        &bikeshop::app::api::kiosk::device_key(kiosk.id),
        "Kiosk",
        Some(abilities),
        None,
    )
    .await
    .unwrap();
    kiosk.token_id = Some(token.token.id);
    kiosk.save_only(db, &["token_id"]).await.unwrap();
    token.plain
}

/// The rider's personal token with `abilities`.
async fn personal(w: &World, abilities: &[&str]) -> String {
    w.rider
        .create_token_with(w.app.db(), "Phone", abilities, None)
        .await
        .unwrap()
        .plain
}

async fn get(w: &World, token: &str, uri: &str) -> TestResponse {
    w.app
        .request()
        .header("authorization", &format!("Bearer {token}"))
        .json()
        .get(uri)
        .await
}

async fn post(w: &World, token: &str, uri: &str, body: Value) -> TestResponse {
    w.app
        .request()
        .header("authorization", &format!("Bearer {token}"))
        .post_json(uri, &body)
        .await
}

/// A reservation of the rider at `store` for the next two hours.
async fn reserved(w: &World, store: &Store) -> Rental {
    let mut bike = fixtures::bike(w.app.db(), store.id, store.id)
        .await
        .unwrap();
    // Every bike at the same rates, so two rentals can be compared.
    bike.hourly_rate = 1_300;
    bike.daily_rate = 6_000;
    bike.deposit = 26_000;
    bike.save(w.app.db()).await.unwrap();
    let now = renox::db::now();
    booking::book(
        w.app.db(),
        NewRental {
            bike_id: bike.id,
            customer_id: w.customer.id,
            operating_store_id: store.id,
            start: now,
            end: now + renox::chrono::Duration::hours(2),
            served_by: None,
        },
    )
    .await
    .unwrap()
    .unwrap()
}

fn code(rental: &Rental) -> String {
    rental.reservation_code.to_string()
}

#[renox::test]
async fn managers_make_kiosk_tokens_shown_once_and_revoke_them() {
    let w = world().await;
    w.app.acting_as(&w.manager);
    w.app
        .post(
            "/staff/api-tokens",
            &[
                ("name", "Racks by the door"),
                ("abilities", "rentals:read"),
                ("abilities", "rentals:checkout"),
            ],
        )
        .await
        .assert_redirect("/staff/api-tokens");
    let page = w.app.get("/staff/api-tokens").await;
    page.assert_ok().assert_see("Racks by the door");
    let html = page.text();
    let start = html.find("id=\"fresh-token\"").expect("the token is shown");
    let value = &html[start..];
    let value = &value[value.find("value=\"").unwrap() + 7..];
    let token = value[..value.find('"').unwrap()].to_owned();
    assert!(token.contains('|'), "{token}");
    // Only once.
    w.app
        .get("/staff/api-tokens")
        .await
        .assert_dont_see("id=\"fresh-token\"");
    let kiosk = Kiosk::query()
        .where_(Kiosk::STORE_ID.eq(w.north.id))
        .first(w.app.db())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(kiosk.abilities.0, vec!["rentals:read", "rentals:checkout"]);

    w.app.logout();
    get(&w, &token, "/api/v1/kiosk/bikes").await.assert_ok();
    // A kiosk can't be given a customer's abilities.
    w.app.acting_as(&w.manager);
    w.app
        .request()
        .json()
        .post("/staff/api-tokens", &[("name", "X"), ("abilities", "read")])
        .await
        .assert_status(422);
    // [explain:api.kiosks.test]
    // Revoked: the kiosk is out at once.
    w.app
        .post(&format!("/staff/api-tokens/{}/revoke", kiosk.id), &[])
        .await
        .assert_redirect("/staff/api-tokens");
    w.app.logout();
    get(&w, &token, "/api/v1/kiosk/bikes")
        .await
        .assert_status(401);
    // Cashiers don't manage kiosks.
    w.app.acting_as(&w.cashier);
    w.app.get("/staff/api-tokens").await.assert_status(403);
    // [/explain:api.kiosks.test]
}

#[renox::test]
async fn each_endpoint_refuses_no_token_a_missing_ability_and_another_stores_data() {
    let w = world().await;
    let rental = reserved(&w, &w.north).await;
    let south_rental = reserved(&w, &w.south).await;
    let reader = kiosk(&w, &w.north, &["rentals:read"]).await;
    let full = kiosk(
        &w,
        &w.north,
        &["rentals:read", "rentals:checkout", "rentals:return"],
    )
    .await;
    let mine = personal(&w, &["read"]).await;
    let body = json!({ "checklist": ["frame"], "method": "card" });

    // No token: 401 everywhere.
    for e in ENDPOINTS {
        let uri = e
            .path
            .replace("{code}", &code(&rental))
            .replace("{slug}", "x");
        let answer = match e.method {
            "GET" => w.app.request().json().get(&uri).await,
            "DELETE" => w.app.request().json().delete(&uri).await,
            _ => w.app.request().json().post_json(&uri, &json!({})).await,
        };
        answer.assert_status(401);
    }
    // A token without the ability: 403.
    post(
        &w,
        &reader,
        &format!("/api/v1/kiosk/rentals/{}/checkout", code(&rental)),
        body.clone(),
    )
    .await
    .assert_status(403);
    post(
        &w,
        &reader,
        &format!("/api/v1/kiosk/rentals/{}/return", code(&rental)),
        json!({ "method": "card" }),
    )
    .await
    .assert_status(403);
    get(&w, &mine, "/api/v1/me/orders").await.assert_status(403);
    // The wrong kind of token is no login at all: a customer's token is no
    // device, and a kiosk's device token is no user.
    get(&w, &mine, "/api/v1/kiosk/bikes")
        .await
        .assert_status(401);
    get(&w, &reader, "/api/v1/me").await.assert_status(401);
    get(&w, &mine, "/api/v1/me/rentals")
        .await
        .assert_status(403);
    // Another store's reservation doesn't exist for this kiosk.
    get(
        &w,
        &reader,
        &format!("/api/v1/kiosk/rentals/{}", code(&south_rental)),
    )
    .await
    .assert_status(404);
    post(
        &w,
        &full,
        &format!("/api/v1/kiosk/rentals/{}/checkout", code(&south_rental)),
        body,
    )
    .await
    .assert_status(404);
    // Its own: fine.
    get(
        &w,
        &reader,
        &format!("/api/v1/kiosk/rentals/{}", code(&rental)),
    )
    .await
    .assert_ok()
    .assert_json_path("data.status", "reserved");
    // The bikes are its store's only.
    let bikes = get(&w, &reader, "/api/v1/kiosk/bikes").await;
    bikes.assert_ok().assert_json_path("store_id", w.north.id);
    let south_bikes: Vec<i64> = RentalBike::query()
        .where_(RentalBike::LOCATION_STORE_ID.eq(w.south.id))
        .pluck(w.app.db(), RentalBike::ID)
        .await
        .unwrap();
    let listed: Vec<i64> = bikes
        .json_path("data")
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["id"].as_i64().unwrap())
        .collect();
    assert!(listed.iter().all(|id| !south_bikes.contains(id)));
}

/// What a rental settles to, comparable between two rentals.
async fn outcome(w: &World, id: i64) -> (RentalStatus, DepositStatus, i64, i64, i64, Vec<i64>) {
    let r = Rental::find(w.app.db(), id).await.unwrap().unwrap();
    let mut paid: Vec<i64> = Payment::query()
        .where_(Payment::PAYABLE_TYPE.eq("rentals"))
        .where_(Payment::PAYABLE_ID.eq(id))
        .pluck(w.app.db(), Payment::AMOUNT)
        .await
        .unwrap();
    paid.sort();
    (
        r.status,
        r.deposit_status,
        r.late_fee,
        r.deposit_refunded,
        r.ridden_minutes,
        paid,
    )
}

#[renox::test]
async fn a_kiosk_checks_out_and_takes_back_as_the_counter_does() {
    let w = world().await;
    let at_counter = reserved(&w, &w.north).await;
    let at_kiosk = reserved(&w, &w.north).await;
    let token = kiosk(
        &w,
        &w.north,
        &["rentals:read", "rentals:checkout", "rentals:return"],
    )
    .await;

    // Out: the cashier at the counter, the customer at the kiosk.
    w.app.acting_as(&w.cashier);
    w.app
        .post(
            &format!("/staff/rentals/{}/pickup", at_counter.id),
            &[("checklist", "frame"), ("method", "card")],
        )
        .await
        .assert_status(303);
    w.app.logout();
    post(
        &w,
        &token,
        &format!("/api/v1/kiosk/rentals/{}/checkout", code(&at_kiosk)),
        json!({ "checklist": ["frame"], "method": "card" }),
    )
    .await
    .assert_ok()
    .assert_json_path("data.status", "active");
    // Checked out twice: a conflict.
    post(
        &w,
        &token,
        &format!("/api/v1/kiosk/rentals/{}/checkout", code(&at_kiosk)),
        json!({ "checklist": [], "method": "card" }),
    )
    .await
    .assert_status(409);

    // Back an hour late, both.
    w.app.travel(3 * HOUR);
    w.app.acting_as(&w.cashier);
    w.app
        .post(
            &format!("/staff/rentals/{}/return", at_counter.id),
            &[("checklist", "frame"), ("method", "card")],
        )
        .await
        .assert_status(303);
    w.app.logout();
    let back = post(
        &w,
        &token,
        &format!("/api/v1/kiosk/rentals/{}/return", code(&at_kiosk)),
        json!({ "checklist": ["frame"], "method": "card" }),
    )
    .await;
    back.assert_ok().assert_json_path("data.status", "returned");
    assert!(back.json_path("data.late_fee").as_i64().unwrap() > 0);
    assert!(back.json_path("settlement.refund").is_i64());

    let counter = outcome(&w, at_counter.id).await;
    let kiosk = outcome(&w, at_kiosk.id).await;
    assert_eq!(counter, kiosk, "the same rules, the same result");
    assert_eq!(counter.0, RentalStatus::Returned);
}

#[renox::test]
async fn an_unverified_customer_is_refused_at_the_kiosk_as_at_the_counter() {
    let w = world().await;
    let mut customer = w.customer.clone();
    customer.id_verified_at = None;
    customer.save(w.app.db()).await.unwrap();
    let rental = reserved(&w, &w.north).await;
    let token = kiosk(&w, &w.north, &["rentals:checkout"]).await;
    let answer = post(
        &w,
        &token,
        &format!("/api/v1/kiosk/rentals/{}/checkout", code(&rental)),
        json!({ "checklist": [], "method": "card" }),
    )
    .await;
    answer.assert_status(422);
    assert!(
        answer.json_path("errors.method").is_array(),
        "{}",
        answer.text()
    );
    // A wrong checklist item: the counter's own rule.
    post(
        &w,
        &token,
        &format!("/api/v1/kiosk/rentals/{}/checkout", code(&rental)),
        json!({ "checklist": ["wings"], "method": "card" }),
    )
    .await
    .assert_status(422);
}

#[renox::test]
async fn customers_reserve_and_cancel_with_the_websites_rules() {
    let w = world().await;
    let token = personal(&w, &["read", "rent"]).await;
    let bike = fixtures::bike(w.app.db(), w.north.id, w.north.id)
        .await
        .unwrap();
    let local = |hours: i64| {
        let at = renox::db::now() + renox::chrono::Duration::hours(hours);
        bikeshop::app::rentals::booking::to_local(&w.app.state().config, at)
            .format("%Y-%m-%dT%H:00:00")
            .to_string()
    };
    let body = json!({
        "store": w.north.id,
        "bike": bike.id,
        "starts_at": local(3),
        "ends_at": local(5),
    });
    let made = post(&w, &token, "/api/v1/me/rentals", body.clone()).await;
    made.assert_status(201);
    let code = made.json_path("data.code").as_str().unwrap().to_owned();
    assert_eq!(code.len(), 26, "a Ulid");
    // The same bike for the same hours: the overlap rule, as on /rent.
    let taken = post(&w, &token, "/api/v1/me/rentals", body).await;
    taken.assert_status(422);
    assert!(taken.json_path("message").is_string());
    assert!(
        taken.json_path("errors.bike").is_array(),
        "{}",
        taken.text()
    );
    // A period that ends before it starts.
    post(
        &w,
        &token,
        "/api/v1/me/rentals",
        json!({ "store": w.north.id, "bike": bike.id, "starts_at": local(6), "ends_at": local(4) }),
    )
    .await
    .assert_status(422);
    // Listed, then cancelled.
    get(&w, &token, "/api/v1/me/rentals")
        .await
        .assert_ok()
        .assert_json_path("data.0.code", code.as_str());
    w.app
        .request()
        .header("authorization", &format!("Bearer {token}"))
        .json()
        .delete(&format!("/api/v1/me/rentals/{code}"))
        .await
        .assert_ok()
        .assert_json_path("data.status", "cancelled");
    // Someone else's code: 404.
    let other = User::register(w.app.db(), "Other", "o@example.com", "a long password")
        .await
        .unwrap();
    let theirs = other
        .create_token_with(w.app.db(), "Phone", &["rent"], None)
        .await
        .unwrap()
        .plain;
    w.app
        .request()
        .header("authorization", &format!("Bearer {theirs}"))
        .json()
        .delete(&format!("/api/v1/me/rentals/{code}"))
        .await
        .assert_status(404);
}

#[renox::test]
async fn the_catalogue_comes_in_pages_with_links_and_stock_per_store() {
    let w = world().await;
    let db = w.app.db();
    let n = unique();
    let category = Category::create(
        db,
        Category {
            name: format!("Helmets {n}"),
            slug: format!("helmets-{n}"),
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
    let made = products()
        .count(25)
        .of(category.id, brand.id)
        .create(db)
        .await
        .unwrap();
    variants_of(made[0].id).create_one(db).await.unwrap();
    let token = personal(&w, &["read"]).await;
    let first = get(&w, &token, "/api/v1/products").await;
    first.assert_ok();
    let total = first.json_path("meta.total").as_u64().unwrap();
    assert!(total >= 25);
    assert_eq!(first.json_path("data").as_array().unwrap().len(), 20);
    assert!(first.json_path("links.prev").is_null());
    let next = first.json_path("links.next").as_str().unwrap().to_owned();
    assert!(next.ends_with("page=2"), "{next}");
    let path = &next[next.find("/api/").unwrap()..];
    let second = get(&w, &token, path).await;
    second.assert_ok().assert_json_path("meta.page", 2);
    // A product with its stock per store.
    get(&w, &token, &format!("/api/v1/products/{}", made[0].slug))
        .await
        .assert_ok()
        .assert_json_path("data.slug", made[0].slug.as_str())
        .assert_json_path("data.variants.0.stock.0.available", 0);
    get(&w, &token, "/api/v1/products/no-such-thing")
        .await
        .assert_status(404);
}

#[renox::test]
async fn the_app_origin_may_call_it_and_a_token_is_throttled() {
    let w = world().await;
    let token = personal(&w, &["read"]).await;
    let answer = w
        .app
        .request()
        .header("authorization", &format!("Bearer {token}"))
        .header("origin", APP_ORIGIN)
        .json()
        .get("/api/v1/me")
        .await;
    answer.assert_ok();
    assert_eq!(
        answer.header("access-control-allow-origin"),
        Some(APP_ORIGIN)
    );
    let other = w
        .app
        .request()
        .header("authorization", &format!("Bearer {token}"))
        .header("origin", "https://evil.example")
        .json()
        .get("/api/v1/me")
        .await;
    assert_eq!(other.header("access-control-allow-origin"), None);
    // PER_MINUTE a minute per token (two used above), then 429.
    for _ in 2..PER_MINUTE {
        get(&w, &token, "/api/v1/me").await.assert_ok();
    }
    let over = get(&w, &token, "/api/v1/me").await;
    over.assert_status(429);
    assert!(over.header("retry-after").is_some());
    // Another token has its own count.
    let fresh = personal(&w, &["read"]).await;
    get(&w, &fresh, "/api/v1/me").await.assert_ok();
}

#[renox::test]
async fn every_api_route_is_explained_on_the_about_page() {
    let w = world().await;
    let routes: HashSet<(String, String)> = w
        .app
        .kernel()
        .routes()
        .iter()
        .filter(|r| r.path.starts_with("/api/v1"))
        .map(|r| (r.method.clone(), r.path.clone()))
        .collect();
    let listed: HashSet<(String, String)> = ENDPOINTS
        .iter()
        .map(|e| (e.method.to_owned(), e.path.to_owned()))
        .collect();
    assert_eq!(
        routes, listed,
        "/about/api lists every /api/v1 route, and only those"
    );
    let page = w.app.get("/about/api").await;
    page.assert_ok();
    for e in ENDPOINTS {
        page.assert_see(e.path);
    }
    // The tokens pages answer.
    w.app.acting_as(&w.rider);
    w.app.get("/account/api-tokens").await.assert_ok();
    w.app
        .post(
            "/account/api-tokens",
            &[
                ("name", "Phone"),
                ("abilities", "read"),
                ("abilities", "rent"),
            ],
        )
        .await
        .assert_redirect("/account/api-tokens");
    w.app
        .get("/account/api-tokens")
        .await
        .assert_see("id=\"fresh-token\"")
        .assert_see("read, rent");
    w.app.acting_as(&w.manager);
    w.app.get("/staff/api-tokens").await.assert_ok();
}
