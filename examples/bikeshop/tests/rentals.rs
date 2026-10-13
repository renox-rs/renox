//! Bike rentals (#235): reserving with the overlap rule checked twice (and
//! a race for the last bike), the deposit paid online, the ID check and who
//! may see an ID photo, the counter's pick-up and return (late and damage
//! fees, the deposit settled, a bike owned by one store rented out by
//! another), the scheduled no-show, reminder, overdue and service tasks,
//! the fleet board, and the pages' queries.

use bikeshop::app::access::catalogue::{CASHIER, MANAGER, MECHANIC};
use bikeshop::app::accounts::model::Customer;
use bikeshop::app::rentals::booking::{self, NewRental, Refusal};
use bikeshop::app::rentals::factories::{PlacementStates, RentalStates, rentals};
use bikeshop::app::rentals::model::{
    BikePlacement, BikeStatus, DepositStatus, IdentityDocument, IdentityStatus, Rental, RentalBike,
    RentalStatus,
};
use bikeshop::app::rentals::{RentalClosed, tasks};
use bikeshop::app::sales::model::{PAYABLE_RENTAL, Payment, PaymentStatus};
use bikeshop::app::sales::payments;
use bikeshop::app::staff::model::Store;
use bikeshop::app::workshop::model::{WorkOrder, WorkSource};
use bikeshop::seed::fixtures;
use renox::chrono::Duration as Span;
use renox::db::{Encrypted, capture_queries};
use renox::prelude::*;
use renox::testing::TestApp;
use std::time::Duration;

const HOUR: Duration = Duration::from_secs(60 * 60);
const MINUTE: Duration = Duration::from_secs(60);

/// A 1×1 PNG, for the ID photo and damage photos.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

/// The shop for a test: three stores, a cashier at North and at South, a
/// manager at West, and a bike owned by North standing at North.
struct World {
    app: TestApp,
    north: Store,
    south: Store,
    west: Store,
    cashier_north: User,
    cashier_south: User,
    manager_west: User,
    bike: RentalBike,
}

async fn world() -> World {
    let app = TestApp::new(bikeshop::app()).await;
    let db = app.db();
    fixtures::roles(db).await.unwrap();
    let north = fixtures::store(db, "North").await.unwrap();
    let south = fixtures::store(db, "South").await.unwrap();
    let west = fixtures::store(db, "West").await.unwrap();
    let cashier_north = fixtures::person(db, "cn@example.com", &[(CASHIER, Some(north.id))])
        .await
        .unwrap();
    let cashier_south = fixtures::person(db, "cs@example.com", &[(CASHIER, Some(south.id))])
        .await
        .unwrap();
    let manager_west = fixtures::person(db, "mw@example.com", &[(MANAGER, Some(west.id))])
        .await
        .unwrap();
    let mut bike = fixtures::bike(db, north.id, north.id).await.unwrap();
    bike.hourly_rate = 1_500;
    bike.daily_rate = 6_000;
    bike.deposit = 30_000;
    bike.save(db).await.unwrap();
    World {
        app,
        north,
        south,
        west,
        cashier_north,
        cashier_south,
        manager_west,
        bike,
    }
}

/// A customer with a login; `verified` sets their ID as checked.
async fn customer(app: &TestApp, email: &str, verified: bool) -> (User, Customer) {
    let db = app.db();
    let user = User::register(db, email, email, "password123")
        .await
        .unwrap();
    let customer = Customer::create(
        db,
        Customer {
            user_id: Some(user.id),
            name: email.into(),
            email: Some(email.into()),
            active: true,
            id_number: verified.then(|| Encrypted::new("X1234567".to_owned())),
            id_verified_at: verified.then(renox::db::now),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    (user, customer)
}

/// `YYYY-MM-DDTHH:MM` in UTC (the tests' `APP_TIMEZONE`), `hours` from now,
/// on the hour.
fn field_time(hours: i64) -> String {
    let at = renox::db::now() + Span::hours(hours);
    at.format("%Y-%m-%dT%H:00").to_string()
}

async fn book(
    app: &TestApp,
    bike: &RentalBike,
    customer: i64,
    from: i64,
    hours: i64,
) -> std::result::Result<Rental, Refusal> {
    let start = renox::db::now() + Span::hours(from);
    booking::book(
        app.db(),
        NewRental {
            bike_id: bike.id,
            customer_id: customer,
            operating_store_id: bike.location_store_id,
            start,
            end: start + Span::hours(hours),
            served_by: None,
        },
    )
    .await
    .unwrap()
}

#[renox::test]
async fn touching_periods_are_fine_overlapping_ones_are_refused() {
    let w = world().await;
    let (_, c) = customer(&w.app, "a@example.com", true).await;
    // 10:00–12:00 (from now +10 h), then 12:00–14:00 touches it: fine.
    book(&w.app, &w.bike, c.id, 10, 2).await.unwrap();
    book(&w.app, &w.bike, c.id, 12, 2).await.unwrap();
    // 11:00–13:00 overlaps both, 8:00–10:30 the first.
    assert_eq!(
        book(&w.app, &w.bike, c.id, 11, 2).await.unwrap_err(),
        Refusal::Taken
    );
    let start = renox::db::now() + Span::hours(8);
    let refused = booking::book(
        w.app.db(),
        NewRental {
            bike_id: w.bike.id,
            customer_id: c.id,
            operating_store_id: w.north.id,
            start,
            end: start + Span::minutes(150),
            served_by: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(refused.unwrap_err(), Refusal::Taken);
    // A bike at another store can't be booked here.
    let elsewhere = booking::book(
        w.app.db(),
        NewRental {
            bike_id: w.bike.id,
            customer_id: c.id,
            operating_store_id: w.south.id,
            start: start + Span::days(3),
            end: start + Span::days(3) + Span::hours(1),
            served_by: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(elsewhere.unwrap_err(), Refusal::Gone);
    // The rental copies the owner store and the operating store.
    let rentals = Rental::query()
        .where_(Rental::RENTAL_BIKE_ID.eq(w.bike.id))
        .get(w.app.db())
        .await
        .unwrap();
    assert_eq!(rentals.len(), 2);
    assert!(
        rentals
            .iter()
            .all(|r| r.owner_store_id == w.north.id && r.operating_store_id == w.north.id)
    );
}

#[renox::test]
async fn two_customers_racing_for_the_last_bike_one_wins() {
    let w = world().await;
    let (_, a) = customer(&w.app, "a@example.com", true).await;
    let (_, b) = customer(&w.app, "b@example.com", true).await;
    let (first, second) = renox::tokio::join!(
        book(&w.app, &w.bike, a.id, 5, 3),
        book(&w.app, &w.bike, b.id, 6, 3),
    );
    assert_eq!(
        [first.is_ok(), second.is_ok()]
            .iter()
            .filter(|ok| **ok)
            .count(),
        1,
        "exactly one booking wins: {first:?} / {second:?}"
    );
    w.app.assert_database_count("rentals", 1).await;
}

#[renox::test]
async fn reserving_online_shows_the_price_then_holds_the_bike_once_paid() {
    let w = world().await;
    let (user, c) = customer(&w.app, "rider@example.com", true).await;
    let (starts, ends) = (field_time(26), field_time(29));
    let search = format!(
        "/rent?store={}&starts_at={starts}&ends_at={ends}",
        w.north.id
    );
    // Anyone can look: the bike, its price for three hours and its deposit.
    w.app
        .get(&search)
        .await
        .assert_ok()
        .assert_view("rentals/search.html")
        .assert_see(&w.bike.frame_number)
        .assert_see("$45.00")
        .assert_see("$300.00")
        .assert_see("Log in to reserve");

    w.app.acting_as(&user);
    let form = [
        ("store", w.north.id.to_string()),
        ("starts_at", starts.clone()),
        ("ends_at", ends.clone()),
        ("bike", w.bike.id.to_string()),
    ];
    let fields: Vec<(&str, &str)> = form.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let res = w.app.post("/rent", &fields).await;
    res.assert_status(303);
    let rental = Rental::query()
        .where_(Rental::CUSTOMER_ID.eq(c.id))
        .first(w.app.db())
        .await
        .unwrap()
        .unwrap();
    res.assert_redirect(&format!("/rentals/{}", rental.reservation_code));
    assert_eq!((rental.price, rental.deposit), (4_500, 30_000));
    assert_eq!(rental.deposit_status, DepositStatus::Unpaid);
    w.app
        .get(&format!("/rentals/{}", rental.reservation_code))
        .await
        .assert_ok()
        .assert_see(&rental.reservation_code.to_string())
        .assert_see("Pay the deposit");

    // The same bike for an overlapping time: the after hook says so.
    let (other, _) = customer(&w.app, "late@example.com", true).await;
    w.app.acting_as(&other);
    w.app
        .htmx()
        .post("/rent", &fields)
        .await
        .assert_invalid("bike");

    // Paying online: the shared payments contract, then the gateway's webhook.
    w.app.acting_as(&user);
    let res = w
        .app
        .post(&format!("/rentals/{}/pay", rental.reservation_code), &[])
        .await;
    res.assert_status(303);
    let payment = Payment::query()
        .where_(Payment::PAYABLE_TYPE.eq(PAYABLE_RENTAL))
        .where_(Payment::PAYABLE_ID.eq(rental.id))
        .first(w.app.db())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(payment.amount, 30_000);
    payments::mark_paid(w.app.state(), payment.id, "gw-1")
        .await
        .unwrap();
    let rental = Rental::find(w.app.db(), rental.id).await.unwrap().unwrap();
    assert_eq!(rental.deposit_status, DepositStatus::Held);
    let mails = w.app.sent_mail();
    assert!(
        mails
            .iter()
            .any(|m| m.subject.contains(&rental.reservation_code.to_string())),
        "the confirmation mail carries the code"
    );

    // Someone else's reservation doesn't exist for them.
    w.app.acting_as(&other);
    w.app
        .get(&format!("/rentals/{}", rental.reservation_code))
        .await
        .assert_not_found();

    // My rentals lists it; cancelling more than an hour ahead gives the deposit back.
    w.app.acting_as(&user);
    w.app
        .get("/rentals")
        .await
        .assert_ok()
        .assert_see(&rental.reservation_code.to_string());
    w.app
        .post(&format!("/rentals/{}/cancel", rental.reservation_code), &[])
        .await
        .assert_status(303);
    let rental = Rental::find(w.app.db(), rental.id).await.unwrap().unwrap();
    assert_eq!(rental.status, RentalStatus::Cancelled);
    assert_eq!(rental.deposit_status, DepositStatus::Refunded);
    assert_eq!(rental.deposit_refunded, 30_000);
}

#[renox::test]
async fn customers_send_their_id_once_and_only_the_right_staff_see_the_photo() {
    let w = world().await;
    let (user, c) = customer(&w.app, "new@example.com", false).await;
    w.app.acting_as(&user);

    // Without an ID, reserving sends them to the ID page.
    let (starts, ends) = (field_time(26), field_time(28));
    let store = w.north.id.to_string();
    let bike = w.bike.id.to_string();
    let form = [
        ("store", store.as_str()),
        ("starts_at", &starts),
        ("ends_at", &ends),
        ("bike", &bike),
    ];
    w.app
        .post("/rent", &form)
        .await
        .assert_redirect("/rentals/identity");

    // The photo must be an image (by content); the number is sealed.
    w.app
        .post_multipart(
            "/rentals/identity",
            &[("id_number", "ab-123456"), ("store", &store)],
            &[("photo", "id.png", b"not an image at all")],
        )
        .await
        .assert_status(303);
    w.app.assert_database_count("identity_documents", 0).await;
    w.app
        .post_multipart(
            "/rentals/identity",
            &[("id_number", " ab-123456 "), ("store", &store)],
            &[("photo", "id.png", PNG)],
        )
        .await
        .assert_redirect("/rentals/identity");
    let document = IdentityDocument::query()
        .where_(IdentityDocument::CUSTOMER_ID.eq(c.id))
        .first(w.app.db())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(document.status, IdentityStatus::Pending);
    let sealed: String = renox::db::sql("SELECT id_number FROM customers WHERE id = ?")
        .bind(c.id)
        .scalar(w.app.db())
        .await
        .unwrap();
    assert!(
        !sealed.contains("AB-123456"),
        "the number is encrypted in the table"
    );
    let sealed_one = Customer::find(w.app.db(), c.id).await.unwrap().unwrap();
    assert_eq!(
        sealed_one.id_number.as_deref().map(String::as_str),
        Some("AB-123456")
    );

    // With the ID sent, the reservation goes through; pick-up still needs it checked.
    w.app.post("/rent", &form).await.assert_status(303);
    let rental = Rental::query()
        .where_(Rental::CUSTOMER_ID.eq(c.id))
        .first(w.app.db())
        .await
        .unwrap()
        .unwrap();

    let photo = format!("/staff/identities/{}/photo", document.id);
    // Another customer: not staff at all.
    let (other, _) = customer(&w.app, "nosy@example.com", true).await;
    w.app.acting_as(&other);
    w.app.get(&photo).await.assert_forbidden();
    // South's cashier: neither the chosen store nor serving a rental: 404.
    w.app.acting_as(&w.cashier_south);
    w.app.get(&photo).await.assert_not_found();
    w.app
        .get("/staff/identities")
        .await
        .assert_ok()
        .assert_dont_see("new@example.com");
    // A mechanic at North has no `rentals.verify_id`: 403 on the whole page.
    let mechanic = fixtures::person(
        w.app.db(),
        "mech@example.com",
        &[(MECHANIC, Some(w.north.id))],
    )
    .await
    .unwrap();
    w.app.acting_as(&mechanic);
    w.app.get(&photo).await.assert_forbidden();

    // North's cashier sees it, gets a signed link, and it serves the photo.
    w.app.acting_as(&w.cashier_north);
    w.app
        .get("/staff/identities")
        .await
        .assert_ok()
        .assert_see("new@example.com");
    let res = w.app.get(&photo).await;
    res.assert_status(303);
    let signed = res.header("location").unwrap().to_owned();
    assert!(signed.contains("signature="), "{signed}");
    let path = signed
        .split_once("://")
        .map(|(_, rest)| &rest[rest.find('/').unwrap()..])
        .unwrap_or(&signed);
    let file = w.app.get(path).await;
    file.assert_ok();
    // An altered link doesn't work.
    w.app
        .get(&path.replace("signature=", "signature=0"))
        .await
        .assert_forbidden();

    // Pick-up is refused until the ID is approved.
    let pickup = format!("/staff/rentals/{}/pickup", rental.id);
    w.app
        .htmx()
        .post(&pickup, &[("method", "card")])
        .await
        .assert_invalid("method");
    w.app
        .post(&format!("/staff/identities/{}/approve", document.id), &[])
        .await
        .assert_status(303);
    let checked = Customer::find(w.app.db(), c.id).await.unwrap().unwrap();
    assert!(checked.id_verified());
}

#[renox::test]
async fn a_bike_owned_by_north_rented_out_by_south_comes_back_late_and_damaged_at_west() {
    let w = world().await;
    let db = w.app.db();
    // North's bike, placed at South three days ago.
    let mut bike = w.bike.clone();
    bike.location_store_id = w.south.id;
    bike.save(db).await.unwrap();
    BikePlacement::factory()
        .of(&bike, w.south.id)
        .moved(3)
        .create_one(db)
        .await
        .unwrap();
    let (_, c) = customer(&w.app, "rider@example.com", true).await;
    let rental = book(&w.app, &bike, c.id, 0, 3).await.unwrap();
    assert_eq!(
        (rental.owner_store_id, rental.operating_store_id),
        (w.north.id, w.south.id)
    );

    // North's cashier can't hand it over: the counter is South's.
    w.app.acting_as(&w.cashier_north);
    w.app
        .post(
            &format!("/staff/rentals/{}/pickup", rental.id),
            &[("method", "card")],
        )
        .await
        .assert_forbidden();
    // South hands it over: price and deposit taken at the counter.
    w.app.acting_as(&w.cashier_south);
    w.app
        .get(&format!("/staff/rentals/{}", rental.id))
        .await
        .assert_ok()
        .assert_see("Hand the bike over");
    w.app
        .post(
            &format!("/staff/rentals/{}/pickup", rental.id),
            &[
                ("checklist", "frame"),
                ("checklist", "brakes"),
                ("method", "card"),
            ],
        )
        .await
        .assert_status(303);
    let out = Rental::find(db, rental.id).await.unwrap().unwrap();
    assert_eq!(out.status, RentalStatus::Active);
    assert_eq!(out.deposit_status, DepositStatus::Held);
    assert_eq!(
        RentalBike::find(db, bike.id).await.unwrap().unwrap().status,
        BikeStatus::Rented
    );
    let paid: i64 = Payment::query()
        .where_(Payment::PAYABLE_TYPE.eq(PAYABLE_RENTAL))
        .where_(Payment::PAYABLE_ID.eq(rental.id))
        .where_(Payment::STATUS.eq(PaymentStatus::Paid))
        .sum(db, "amount")
        .await
        .unwrap();
    assert_eq!(paid, 4_500 + 30_000);

    // Back at West 1 h 20 min late (two started hours), damaged.
    w.app.travel(HOUR * 3 + MINUTE * 80);
    w.app.acting_as(&w.manager_west);
    w.app
        .get(&format!("/staff/rentals/{}", rental.id))
        .await
        .assert_ok()
        .assert_see("Take the bike back")
        .assert_see("$30.00");
    w.app
        .get("/staff/rentals?q=rider")
        .await
        .assert_ok()
        .assert_see(&rental.reservation_code.to_string());
    let res = w
        .app
        .post_multipart(
            &format!("/staff/rentals/{}/return", rental.id),
            &[
                ("checklist", "frame"),
                ("damaged", "on"),
                ("damage_fee", "80"),
                ("damage_note", "Bent rear wheel"),
                ("method", "cash"),
            ],
            &[("photos", "wheel.png", PNG)],
        )
        .await;
    res.assert_redirect(&format!("/staff/rentals/{}/receipt", rental.id));
    let back = Rental::find(db, rental.id).await.unwrap().unwrap();
    assert_eq!(back.status, RentalStatus::Returned);
    assert_eq!(back.late_fee, 3_000);
    assert_eq!(back.damage_fee, 8_000);
    assert_eq!(back.deposit_status, DepositStatus::Settled);
    assert_eq!(back.deposit_refunded, 30_000 - 11_000);
    assert_eq!(back.return_store_id, Some(w.west.id));
    assert_eq!(
        (back.owner_store_id, back.operating_store_id),
        (w.north.id, w.south.id)
    );
    // The bike: now at West, still North's, in the workshop, hours added.
    let bike_after = RentalBike::find(db, bike.id).await.unwrap().unwrap();
    assert_eq!(bike_after.location_store_id, w.west.id);
    assert_eq!(bike_after.owner_store_id, w.north.id);
    assert_eq!(bike_after.status, BikeStatus::Maintenance);
    assert_eq!(bike_after.ridden_hours, bike.ridden_hours + 5);
    // A fleet work order at West, billed to North.
    let order = WorkOrder::query()
        .where_(WorkOrder::RENTAL_BIKE_ID.eq(bike.id))
        .first(db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (order.source, order.store_id, order.billed_store_id),
        (WorkSource::Fleet, w.west.id, Some(w.north.id))
    );
    // The receipt: booked to North, South's fee, send it back to South.
    w.app
        .get(&format!("/staff/rentals/{}/receipt", rental.id))
        .await
        .assert_ok()
        .assert_see("Send it back to South")
        .assert_see("fee (20 %)")
        .assert_see("$9.00");
    // A mechanic of a fourth store sees rentals, but not this one: it isn't
    // theirs, and it isn't out any more (a bike out may come back anywhere).
    let east = fixtures::store(db, "East").await.unwrap();
    let stranger = fixtures::person(db, "east@example.com", &[(MECHANIC, Some(east.id))])
        .await
        .unwrap();
    w.app.acting_as(&stranger);
    w.app
        .get(&format!("/staff/rentals/{}", rental.id))
        .await
        .assert_not_found();
}

#[renox::test]
async fn a_return_emits_rental_closed_for_the_books() {
    let w = world().await;
    let (_, c) = customer(&w.app, "rider@example.com", true).await;
    let rental = rentals()
        .of_bike(&w.bike)
        .for_customer(c.id)
        .active()
        .create_one(w.app.db())
        .await
        .unwrap();
    w.app.fake_events();
    w.app.acting_as(&w.cashier_north);
    w.app
        .post_multipart(
            &format!("/staff/rentals/{}/return", rental.id),
            &[("method", "card")],
            &[],
        )
        .await
        .assert_status(303);
    w.app
        .assert_emitted::<RentalClosed>(|e| e.rental_id == rental.id);
}

#[renox::test]
async fn no_shows_unpaid_reservations_reminders_and_overdue_rentals() {
    let w = world().await;
    let db = w.app.db();
    let (user, c) = customer(&w.app, "rider@example.com", true).await;
    w.app.fake_notifications();

    // A paid reservation starting in an hour, never picked up.
    let mut no_show = book(&w.app, &w.bike, c.id, 1, 2).await.unwrap();
    no_show.deposit_status = DepositStatus::Held;
    no_show.save(db).await.unwrap();
    // An unpaid one, made now, for tomorrow (another bike).
    let other_bike = fixtures::bike(db, w.north.id, w.north.id).await.unwrap();
    let unpaid = book(&w.app, &other_bike, c.id, 30, 2).await.unwrap();
    // One out, due in 45 minutes (a third bike).
    let third = fixtures::bike(db, w.north.id, w.north.id).await.unwrap();
    let mut out = rentals()
        .of_bike(&third)
        .for_customer(c.id)
        .active()
        .create_one(db)
        .await
        .unwrap();
    out.due_at = renox::db::now() + Span::minutes(45);
    out.save(db).await.unwrap();

    // Now: the reminder goes out once; the bike shows reserved on the board.
    w.app
        .at_travelled_time(tasks::watch(w.app.state()))
        .await
        .unwrap();
    w.app.assert_notified(&user, "rental-reminder");
    assert_eq!(
        RentalBike::find(db, w.bike.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        BikeStatus::Reserved
    );
    w.app
        .at_travelled_time(tasks::watch(w.app.state()))
        .await
        .unwrap();
    let reminders = w
        .app
        .notifications()
        .iter()
        .filter(|n| n.kind == "rental-reminder")
        .count();
    assert_eq!(reminders, 1, "reminded once");

    // 40 minutes on: the unpaid reservation lapsed.
    w.app.travel(MINUTE * 40);
    w.app
        .at_travelled_time(tasks::watch(w.app.state()))
        .await
        .unwrap();
    assert_eq!(
        Rental::find(db, unpaid.id).await.unwrap().unwrap().status,
        RentalStatus::Cancelled
    );
    w.app.assert_notified(&user, "rental-unpaid");

    // Two hours on: the no-show (an hour and a half after its start) and the overdue rental.
    w.app.travel(MINUTE * 120);
    w.app
        .at_travelled_time(tasks::watch(w.app.state()))
        .await
        .unwrap();
    let missed = Rental::find(db, no_show.id).await.unwrap().unwrap();
    assert_eq!(missed.status, RentalStatus::NoShow);
    assert_eq!(missed.deposit_status, DepositStatus::Forfeited);
    assert_eq!(missed.deposit_refunded, 30_000 - missed.price);
    assert_eq!(
        RentalBike::find(db, w.bike.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        BikeStatus::Available
    );
    w.app.assert_notified(&user, "rental-no-show");

    let late = Rental::find(db, out.id).await.unwrap().unwrap();
    assert_eq!(late.status, RentalStatus::Overdue);
    assert!(late.late_fee > 0);
    assert_eq!(
        RentalBike::find(db, third.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        BikeStatus::Overdue
    );
    w.app.assert_notified(&user, "rental-overdue");
    w.app
        .assert_notified(&w.cashier_north, "rental-overdue-staff");
    // The late fee grows; nobody is told twice.
    let fee = late.late_fee;
    w.app.travel(MINUTE * 120 + HOUR * 2);
    w.app
        .at_travelled_time(tasks::watch(w.app.state()))
        .await
        .unwrap();
    assert!(Rental::find(db, out.id).await.unwrap().unwrap().late_fee > fee);
    let overdue_notices = w
        .app
        .notifications()
        .iter()
        .filter(|n| n.kind == "rental-overdue")
        .count();
    assert_eq!(overdue_notices, 1);
}

#[renox::test]
async fn bikes_due_for_service_get_a_work_order() {
    let w = world().await;
    let db = w.app.db();
    // North's bike standing at South, 210 hours since its last service.
    let mut bike = w.bike.clone();
    bike.location_store_id = w.south.id;
    bike.ridden_hours = 610;
    bike.serviced_at_hours = 400;
    bike.save(db).await.unwrap();
    assert_eq!(tasks::service_due(w.app.state()).await.unwrap(), 1);
    let bike = RentalBike::find(db, bike.id).await.unwrap().unwrap();
    assert_eq!(bike.status, BikeStatus::Maintenance);
    let order = WorkOrder::query()
        .where_(WorkOrder::RENTAL_BIKE_ID.eq(bike.id))
        .first(db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (order.store_id, order.billed_store_id),
        (w.south.id, Some(w.north.id))
    );
    // Running it again opens nothing more.
    assert_eq!(tasks::service_due(w.app.state()).await.unwrap(), 0);
    w.app.assert_database_count("work_orders", 1).await;
}

#[renox::test]
async fn the_fleet_board_shows_owner_and_location_by_tab() {
    let w = world().await;
    let db = w.app.db();
    // South's bike placed at North, and North's bike placed at South.
    let theirs = fixtures::bike(db, w.south.id, w.north.id).await.unwrap();
    let mine_away = fixtures::bike(db, w.north.id, w.south.id).await.unwrap();
    w.app.acting_as(&w.cashier_north);
    w.app
        .get("/staff/fleet")
        .await
        .assert_ok()
        .assert_view("rentals/fleet.html")
        .assert_see(&w.bike.frame_number)
        .assert_see(&theirs.frame_number)
        .assert_see(&mine_away.frame_number);
    w.app
        .get("/staff/fleet?view=others_here")
        .await
        .assert_see(&theirs.frame_number)
        .assert_dont_see(&w.bike.frame_number);
    w.app
        .get("/staff/fleet?view=mine_elsewhere")
        .await
        .assert_see(&mine_away.frame_number)
        .assert_dont_see(&theirs.frame_number);
    // The bike's page says where each action is checked.
    w.app
        .get(&format!("/staff/fleet/{}", theirs.id))
        .await
        .assert_ok()
        .assert_see("Change its rates")
        .assert_see("Checked in South (owner)");
    // West's manager doesn't see North's bike at North.
    w.app.acting_as(&w.manager_west);
    w.app
        .get(&format!("/staff/fleet/{}", w.bike.id))
        .await
        .assert_not_found();
}

#[renox::test]
async fn lists_load_in_a_fixed_number_of_queries() {
    let w = world().await;
    let db = w.app.db();
    let (user, c) = customer(&w.app, "rider@example.com", true).await;
    for _ in 0..2 {
        let bike = fixtures::bike(db, w.north.id, w.north.id).await.unwrap();
        rentals()
            .of_bike(&bike)
            .for_customer(c.id)
            .returned()
            .create_one(db)
            .await
            .unwrap();
    }
    w.app.acting_as(&user);
    let (_, few) = capture_queries(w.app.get("/rentals")).await;
    for _ in 0..8 {
        let bike = fixtures::bike(db, w.north.id, w.north.id).await.unwrap();
        rentals()
            .of_bike(&bike)
            .for_customer(c.id)
            .active()
            .create_one(db)
            .await
            .unwrap();
    }
    let (_, many) = capture_queries(w.app.get("/rentals")).await;
    assert_eq!(
        few.len(),
        many.len(),
        "/rentals runs the same queries for 2 or 10 rentals"
    );

    w.app.acting_as(&w.cashier_north);
    // The first staff request picks the active store (one query, then the session has it).
    w.app.get("/staff/rentals").await.assert_ok();
    let (_, counter) = capture_queries(w.app.get("/staff/rentals")).await;
    for _ in 0..5 {
        let bike = fixtures::bike(db, w.north.id, w.north.id).await.unwrap();
        rentals()
            .of_bike(&bike)
            .for_customer(c.id)
            .overdue()
            .create_one(db)
            .await
            .unwrap();
    }
    let (_, more) = capture_queries(w.app.get("/staff/rentals")).await;
    assert_eq!(
        counter.len(),
        more.len(),
        "the counter's lists don't grow with rows"
    );
}

#[renox::test]
async fn every_rental_page_answers() {
    let w = world().await;
    let (user, c) = customer(&w.app, "rider@example.com", true).await;
    let rental = rentals()
        .of_bike(&w.bike)
        .for_customer(c.id)
        .returned_late()
        .create_one(w.app.db())
        .await
        .unwrap();
    w.app.get("/rent").await.assert_ok();
    w.app.acting_as(&user);
    for page in ["/rentals", "/rentals/identity"] {
        w.app.get(page).await.assert_ok();
    }
    w.app
        .get(&format!("/rentals/{}", rental.reservation_code))
        .await
        .assert_ok();
    w.app.acting_as(&w.cashier_north);
    for page in [
        "/staff/rentals".to_owned(),
        "/staff/rentals/walk-in".to_owned(),
        "/staff/identities".to_owned(),
        "/staff/fleet".to_owned(),
        format!("/staff/fleet/{}", w.bike.id),
        format!("/staff/rentals/{}", rental.id),
        format!("/staff/rentals/{}/receipt", rental.id),
    ] {
        w.app.get(&page).await.assert_ok();
    }
    // In Spanish too.
    w.app.post("/locale/es", &[]).await;
    w.app
        .get("/staff/rentals")
        .await
        .assert_ok()
        .assert_see("Mostrador de alquiler");
}

#[renox::test]
async fn a_walk_in_rental_is_booked_at_the_counter() {
    let w = world().await;
    let (_, c) = customer(&w.app, "walk@example.com", true).await;
    let (_, unverified) = customer(&w.app, "nobody@example.com", false).await;
    w.app.acting_as(&w.cashier_north);
    w.app
        .get("/staff/rentals/customers?q=walk")
        .await
        .assert_ok()
        .assert_see("walk@example.com")
        .assert_dont_see("nobody@example.com");
    let (starts, ends) = (field_time(1), field_time(3));
    let bike = w.bike.id.to_string();
    let who = unverified.id.to_string();
    w.app
        .htmx()
        .post(
            "/staff/rentals/walk-in",
            &[
                ("customer", &who),
                ("bike", &bike),
                ("starts_at", &starts),
                ("ends_at", &ends),
            ],
        )
        .await
        .assert_invalid("customer");
    let who = c.id.to_string();
    let res = w
        .app
        .post(
            "/staff/rentals/walk-in",
            &[
                ("customer", &who),
                ("bike", &bike),
                ("starts_at", &starts),
                ("ends_at", &ends),
            ],
        )
        .await;
    res.assert_status(303);
    let rental = Rental::query()
        .where_(Rental::CUSTOMER_ID.eq(c.id))
        .first(w.app.db())
        .await
        .unwrap()
        .unwrap();
    res.assert_redirect(&format!("/staff/rentals/{}", rental.id));
    assert_eq!(rental.operating_store_id, w.north.id);
    assert!(rental.served_by.is_some());
}
