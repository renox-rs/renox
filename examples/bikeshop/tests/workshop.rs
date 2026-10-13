//! The workshop (#236): customers' bikes and their history, booking with
//! the workshop's daily capacity (a full day, a race for the last slot),
//! the board and its moves, parts taken from stock (or waited for), extra
//! work approved through a signed link (once, not tampered, not expired),
//! payment and collection, fleet repairs, reminders, who sees what, and
//! the pages' queries.

use bikeshop::app::access::catalogue::{CASHIER, MECHANIC};
use bikeshop::app::accounts::model::Customer;
use bikeshop::app::catalog::factories::{ProductStates, variants_of};
use bikeshop::app::catalog::model::{
    Brand, Category, CategoryKind, PART_FITS, Product, ProductVariant,
};
use bikeshop::app::rentals::model::{BikeStatus, RentalBike};
use bikeshop::app::staff::model::Store;
use bikeshop::app::stock::model::{MovementReason, StockLevel, StockMovement};
use bikeshop::app::workshop::approval;
use bikeshop::app::workshop::capacity::{self, NewBooking};
use bikeshop::app::workshop::factories::{WorkOrderStates, work_orders};
use bikeshop::app::workshop::model::{
    CustomerBike, ExtraStatus, ExtraWork, PartStatus, ServiceTask, WorkOrder, WorkOrderPart,
    WorkOrderTask, WorkSource, WorkStatus,
};
use bikeshop::app::workshop::tasks;
use bikeshop::seed::{fixtures, unique};
use renox::chrono::{Duration as Span, NaiveDate};
use renox::db::capture_queries;
use renox::prelude::*;
use renox::testing::TestApp;
use std::time::Duration;

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

struct World {
    app: TestApp,
    north: Store,
    south: Store,
    mechanic: User,
    mechanic_south: User,
    cashier: User,
    rider: User,
    customer: Customer,
    bike: CustomerBike,
    /// 40 minutes, $60.00.
    tune: ServiceTask,
    /// 20 minutes, $30.00.
    check: ServiceTask,
}

async fn world() -> World {
    let app = TestApp::new(bikeshop::app()).await;
    let db = app.db();
    fixtures::roles(db).await.unwrap();
    let mut north = fixtures::store(db, "North").await.unwrap();
    north.workshop_minutes_per_day = 60;
    north.save(db).await.unwrap();
    let south = fixtures::store(db, "South").await.unwrap();
    let mechanic = fixtures::person(db, "mech@example.com", &[(MECHANIC, Some(north.id))])
        .await
        .unwrap();
    let mechanic_south = fixtures::person(db, "mechs@example.com", &[(MECHANIC, Some(south.id))])
        .await
        .unwrap();
    let cashier = fixtures::person(db, "cash@example.com", &[(CASHIER, Some(north.id))])
        .await
        .unwrap();
    let rider = User::register(db, "Rider", "rider@example.com", "password123")
        .await
        .unwrap();
    let customer = Customer::create(
        db,
        Customer {
            user_id: Some(rider.id),
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
            name: "Blue roadie".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let task = |name: &str, minutes: i64, price: i64| {
        let n = unique();
        ServiceTask {
            name: name.into(),
            slug: format!("task-{n}"),
            minutes,
            price,
            ..Default::default()
        }
    };
    let tune = ServiceTask::create(db, task("Tune", 40, 6_000))
        .await
        .unwrap();
    let check = ServiceTask::create(db, task("Check", 20, 3_000))
        .await
        .unwrap();
    World {
        app,
        north,
        south,
        mechanic,
        mechanic_south,
        cashier,
        rider,
        customer,
        bike,
        tune,
        check,
    }
}

/// A day the store is open, `days` from today (fixture stores have no hours: open daily).
fn day(days: i64) -> NaiveDate {
    (renox::db::now() + Span::days(days)).date_naive()
}

async fn booking(
    w: &World,
    days: i64,
    tasks: Vec<ServiceTask>,
) -> Result<std::result::Result<WorkOrder, capacity::DayProblem>> {
    capacity::book(
        w.app.db(),
        &w.app.state().config,
        NewBooking {
            bike_id: w.bike.id,
            store_id: w.north.id,
            day: day(days),
            tasks,
            package: None,
            note: None,
            source: WorkSource::Booking,
            checked_in: false,
        },
    )
    .await
}

/// A spare part that fits a bike model, with `stock` at `store`.
async fn part(w: &World, fits: Option<i64>, stock: i64) -> ProductVariant {
    let db = w.app.db();
    let n = unique();
    let category = Category::create(
        db,
        Category {
            name: format!("Parts {n}"),
            slug: format!("parts-{n}"),
            kind: CategoryKind::Part,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let brand = Brand::create(
        db,
        Brand {
            name: format!("B{n}"),
            slug: format!("b-{n}"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let product: Product = Product::factory()
        .of(category.id, brand.id)
        .create_one(db)
        .await
        .unwrap();
    let mut variant = variants_of(product.id).create_one(db).await.unwrap();
    variant.price = 2_500;
    variant.save(db).await.unwrap();
    if let Some(bike_product) = fits {
        PART_FITS
            .attach(db, product.id, [bike_product])
            .await
            .unwrap();
    }
    if stock > 0 {
        let mut tx = db.begin().await.unwrap();
        StockMovement::record(
            &mut tx,
            StockMovement {
                variant_id: variant.id,
                owner_store_id: w.north.id,
                location_store_id: w.north.id,
                quantity: stock,
                reason: MovementReason::Purchase,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
    variant
}

#[renox::test]
async fn a_full_day_cannot_be_booked() {
    let w = world().await;
    // 40 of the day's 60 minutes.
    booking(&w, 3, vec![w.tune.clone()]).await.unwrap().unwrap();
    // 40 more doesn't fit; 20 does.
    assert_eq!(
        booking(&w, 3, vec![w.tune.clone()])
            .await
            .unwrap()
            .unwrap_err(),
        capacity::DayProblem::Full
    );
    // Through the form: the hook says so next to the day.
    w.app.acting_as(&w.rider);
    let (bike, store, task, d) = (
        w.bike.id.to_string(),
        w.north.id.to_string(),
        w.tune.id.to_string(),
        day(3).to_string(),
    );
    w.app
        .htmx()
        .post(
            "/service/book",
            &[
                ("bike", &bike),
                ("store", &store),
                ("tasks", &task),
                ("day", &d),
            ],
        )
        .await
        .assert_invalid("day");
    // The form greys the day out for 40 minutes, not for 20.
    w.app
        .get(&format!("/service/book?store={store}&tasks={task}"))
        .await
        .assert_ok()
        .assert_see(&d);
    let check = w.check.id.to_string();
    let small = w
        .app
        .get(&format!("/service/book?store={store}&tasks={check}"))
        .await;
    assert!(
        !small.text().contains(&format!("&quot;{d}&quot;"))
            && !small.text().contains(&format!("\"{d}\"")),
        "the day still takes 20 minutes"
    );
    booking(&w, 3, vec![w.check.clone()])
        .await
        .unwrap()
        .unwrap();
    // Past days and too far ahead are refused.
    assert_eq!(
        booking(&w, -1, vec![w.check.clone()])
            .await
            .unwrap()
            .unwrap_err(),
        capacity::DayProblem::Past
    );
    assert_eq!(
        booking(&w, 90, vec![w.check.clone()])
            .await
            .unwrap()
            .unwrap_err(),
        capacity::DayProblem::TooFar
    );
}

#[renox::test]
async fn two_bookings_for_the_last_slot_one_wins() {
    let w = world().await;
    let (a, b) = renox::tokio::join!(
        booking(&w, 5, vec![w.tune.clone()]),
        booking(&w, 5, vec![w.tune.clone()]),
    );
    let won = [a.unwrap().is_ok(), b.unwrap().is_ok()]
        .iter()
        .filter(|ok| **ok)
        .count();
    assert_eq!(won, 1);
    w.app.assert_database_count("work_orders", 1).await;
}

#[renox::test]
async fn customers_register_bikes_book_and_follow_the_work() {
    let w = world().await;
    let db = w.app.db();
    w.app.acting_as(&w.rider);
    w.app
        .get("/bikes")
        .await
        .assert_ok()
        .assert_see("Blue roadie");
    w.app
        .post_multipart(
            "/bikes",
            &[
                ("name", "Red commuter"),
                ("frame_number", "FR-1"),
                ("size", "M"),
            ],
            &[],
        )
        .await
        .assert_status(303);
    let added = CustomerBike::query()
        .where_(CustomerBike::NAME.eq("Red commuter"))
        .first(db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(added.customer_id, w.customer.id);

    // Book a service through the form.
    let (bike, store, d) = (
        w.bike.id.to_string(),
        w.north.id.to_string(),
        day(4).to_string(),
    );
    let (tune, check) = (w.tune.id.to_string(), w.check.id.to_string());
    let res = w
        .app
        .post(
            "/service/book",
            &[
                ("bike", &bike),
                ("store", &store),
                ("tasks", &tune),
                ("tasks", &check),
                ("day", &d),
                ("note", "Squeaky brakes"),
            ],
        )
        .await;
    res.assert_status(303);
    let order = WorkOrder::query()
        .where_(WorkOrder::CUSTOMER_BIKE_ID.eq(w.bike.id))
        .first(db)
        .await
        .unwrap()
        .unwrap();
    res.assert_redirect(&format!("/service/{}", order.id));
    assert_eq!(
        (order.minutes, order.total, order.status),
        (60, 9_000, WorkStatus::Booked)
    );
    assert_eq!(
        WorkOrderTask::query()
            .where_(WorkOrderTask::WORK_ORDER_ID.eq(order.id))
            .count(db)
            .await
            .unwrap(),
        2
    );
    assert!(
        w.app
            .sent_mail()
            .iter()
            .any(|m| m.subject.contains(&format!("#{}", order.id))),
        "a confirmation mail"
    );
    w.app
        .get(&format!("/service/{}", order.id))
        .await
        .assert_ok()
        .assert_see("Squeaky brakes")
        .assert_see("$90.00");
    w.app
        .get(&format!("/bikes/{}", w.bike.id))
        .await
        .assert_ok()
        .assert_see(&format!("Work order #{}", order.id));

    // Reschedule, then cancel (more than 24 hours ahead).
    let later = day(6).to_string();
    w.app
        .post(
            &format!("/service/{}/reschedule", order.id),
            &[("day", &later)],
        )
        .await
        .assert_status(303);
    let moved = WorkOrder::find(db, order.id).await.unwrap().unwrap();
    assert_eq!(moved.scheduled_for.date_naive(), day(6));
    w.app
        .post(&format!("/service/{}/cancel", order.id), &[])
        .await
        .assert_status(303);
    assert_eq!(
        WorkOrder::find(db, order.id).await.unwrap().unwrap().status,
        WorkStatus::Cancelled
    );

    // Someone else's bike and work order don't exist for another customer.
    let other = User::register(db, "Other", "other@example.com", "password123")
        .await
        .unwrap();
    w.app.acting_as(&other);
    w.app
        .get(&format!("/bikes/{}", w.bike.id))
        .await
        .assert_not_found();
    w.app
        .get(&format!("/service/{}", order.id))
        .await
        .assert_not_found();
}

#[renox::test]
async fn parts_move_stock_and_a_missing_one_makes_the_order_wait() {
    let w = world().await;
    let db = w.app.db();
    let mut order = booking(&w, 0, vec![w.check.clone()])
        .await
        .unwrap()
        .unwrap();
    order.status = WorkStatus::InProgress;
    order.save(db).await.unwrap();
    // A bike model the part fits.
    let n = unique();
    let category = Category::create(
        db,
        Category {
            name: format!("Bikes {n}"),
            slug: format!("bikes-{n}"),
            kind: CategoryKind::Bike,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let brand = Brand::create(
        db,
        Brand {
            name: format!("C{n}"),
            slug: format!("c-{n}"),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let model: Product = Product::factory()
        .of(category.id, brand.id)
        .create_one(db)
        .await
        .unwrap();
    let mut bike = w.bike.clone();
    bike.product_id = Some(model.id);
    bike.save(db).await.unwrap();
    let chain = part(&w, Some(model.id), 2).await;
    let other = part(&w, None, 5).await;

    w.app.acting_as(&w.mechanic);
    // The part select lists what fits the bike, with the stock.
    w.app
        .get(&format!("/staff/workshop/{}/parts?q=", order.id))
        .await
        .assert_ok()
        .assert_see(&format!("\"value\":\"{}\"", chain.id))
        .assert_dont_see(&format!("\"value\":\"{}\"", other.id));
    let (order_path, variant) = (
        format!("/staff/workshop/{}/parts", order.id),
        chain.id.to_string(),
    );
    w.app
        .post(&order_path, &[("variant", &variant), ("quantity", "1")])
        .await
        .assert_status(303);
    let level = StockLevel::query()
        .where_(StockLevel::VARIANT_ID.eq(chain.id))
        .first(db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(level.on_hand, 1);
    let movement = StockMovement::query()
        .where_(StockMovement::VARIANT_ID.eq(chain.id))
        .where_(StockMovement::REASON.eq(MovementReason::Service))
        .first(db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            movement.quantity,
            movement.reference_type.as_deref(),
            movement.reference_id
        ),
        (-1, Some("work_orders"), Some(order.id))
    );
    // Three more than there are: the line waits, the order waits for parts.
    w.app
        .post(&order_path, &[("variant", &variant), ("quantity", "3")])
        .await
        .assert_status(303);
    let waiting = WorkOrderPart::query()
        .where_(WorkOrderPart::WORK_ORDER_ID.eq(order.id))
        .where_(WorkOrderPart::STATUS.eq(PartStatus::Waiting))
        .first(db)
        .await
        .unwrap()
        .unwrap();
    let order = WorkOrder::find(db, order.id).await.unwrap().unwrap();
    assert_eq!(order.status, WorkStatus::WaitingParts);
    assert_eq!(order.parts, 2_500 * 4);
    assert_eq!(
        StockLevel::query()
            .where_(StockLevel::VARIANT_ID.eq(chain.id))
            .first(db)
            .await
            .unwrap()
            .unwrap()
            .on_hand,
        1
    );
    // The parts arrive: taken now.
    let mut tx = db.begin().await.unwrap();
    StockMovement::record(
        &mut tx,
        StockMovement {
            variant_id: chain.id,
            owner_store_id: w.north.id,
            location_store_id: w.north.id,
            quantity: 5,
            reason: MovementReason::Purchase,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    w.app
        .post(
            &format!("/staff/workshop/{}/parts/{}/take", order.id, waiting.id),
            &[],
        )
        .await
        .assert_status(303);
    assert_eq!(
        WorkOrderPart::find(db, waiting.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        PartStatus::Used
    );
    assert_eq!(
        StockLevel::query()
            .where_(StockLevel::VARIANT_ID.eq(chain.id))
            .first(db)
            .await
            .unwrap()
            .unwrap()
            .on_hand,
        3
    );
}

#[renox::test]
async fn extra_work_is_approved_once_through_a_signed_link() {
    let w = world().await;
    let db = w.app.db();
    let mut order = booking(&w, 0, vec![w.check.clone()])
        .await
        .unwrap()
        .unwrap();
    order.status = WorkStatus::InProgress;
    order.save(db).await.unwrap();
    w.app.acting_as(&w.mechanic);
    let tune = w.tune.id.to_string();
    w.app
        .post(
            &format!("/staff/workshop/{}/extra", order.id),
            &[("description", "The chain is worn."), ("tasks", &tune)],
        )
        .await
        .assert_status(303);
    let extra = ExtraWork::query()
        .where_(ExtraWork::WORK_ORDER_ID.eq(order.id))
        .first(db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(extra.total, 6_000);
    assert_eq!(
        WorkOrder::find(db, order.id).await.unwrap().unwrap().status,
        WorkStatus::WaitingApproval
    );
    let mail = w
        .app
        .sent_mail()
        .into_iter()
        .find(|m| m.subject.contains("Extra work"))
        .expect("the mail");
    let link = approval::signed_link(w.app.state(), &extra).unwrap();
    let path = link
        .split_once("://")
        .map(|(_, rest)| rest[rest.find('/').unwrap()..].to_owned())
        .unwrap();
    assert!(
        format!("{mail:?}").contains("signature="),
        "the mail carries the signed link"
    );

    // No login needed; a changed link is refused.
    w.app.logout();
    w.app
        .get(&path)
        .await
        .assert_ok()
        .assert_see("The chain is worn.")
        .assert_see("$60.00");
    w.app
        .get(&path.replace(&format!("/{}?", extra.id), &format!("/{}?", extra.id + 1)))
        .await
        .assert_forbidden();
    w.app
        .post(&path, &[("decision", "approve")])
        .await
        .assert_status(303);
    let extra = ExtraWork::find(db, extra.id).await.unwrap().unwrap();
    assert_eq!(extra.status, ExtraStatus::Approved);
    let order = WorkOrder::find(db, order.id).await.unwrap().unwrap();
    assert_eq!((order.status, order.total), (WorkStatus::InProgress, 9_000));
    // Once only.
    w.app
        .post(&path, &[("decision", "refuse")])
        .await
        .assert_status(409);
    w.app
        .get(&path)
        .await
        .assert_ok()
        .assert_see("You approved this work");

    // A link past its 72 hours doesn't work.
    w.app.acting_as(&w.mechanic);
    w.app
        .post(
            &format!("/staff/workshop/{}/extra", order.id),
            &[("description", "Tyres too."), ("tasks", &tune)],
        )
        .await
        .assert_status(303);
    let second = ExtraWork::query()
        .where_(ExtraWork::WORK_ORDER_ID.eq(order.id))
        .where_(ExtraWork::STATUS.eq(ExtraStatus::Pending))
        .first(db)
        .await
        .unwrap()
        .unwrap();
    let link = approval::signed_link(w.app.state(), &second).unwrap();
    let path = link
        .split_once("://")
        .map(|(_, rest)| rest[rest.find('/').unwrap()..].to_owned())
        .unwrap();
    w.app.logout();
    w.app.travel(DAY * 3 + Duration::from_secs(3600));
    w.app.get(&path).await.assert_forbidden();
}

#[renox::test]
async fn the_board_moves_cards_mechanics_see_their_store_only() {
    let w = world().await;
    let db = w.app.db();
    let order = booking(&w, 0, vec![w.check.clone()])
        .await
        .unwrap()
        .unwrap();
    w.app.fake_notifications();
    w.app.acting_as(&w.mechanic);
    w.app
        .get("/staff/workshop")
        .await
        .assert_ok()
        .assert_see("Blue roadie")
        .assert_see("Rider");
    let card = order.id.to_string();
    // Scheduled → checked in: allowed, the customer is told.
    w.app
        .htmx()
        .post(
            "/staff/workshop/move",
            &[("card", &card), ("column", "checked_in"), ("position", "0")],
        )
        .await
        .assert_status(204);
    assert_eq!(
        WorkOrder::find(db, order.id).await.unwrap().unwrap().status,
        WorkStatus::CheckedIn
    );
    w.app.assert_notified(&w.rider, "workshop-status");
    // Checked in → ready skips the work: refused.
    w.app
        .htmx()
        .post(
            "/staff/workshop/move",
            &[("card", &card), ("column", "ready"), ("position", "0")],
        )
        .await
        .assert_status(422);
    // Assign myself.
    w.app
        .post(&format!("/staff/workshop/{}/assign", order.id), &[])
        .await
        .assert_status(303);
    assert!(
        WorkOrder::find(db, order.id)
            .await
            .unwrap()
            .unwrap()
            .mechanic_id
            .is_some()
    );
    // South's mechanic: not their store.
    w.app.acting_as(&w.mechanic_south);
    w.app
        .get("/staff/workshop")
        .await
        .assert_ok()
        .assert_dont_see("Blue roadie");
    w.app
        .get(&format!("/staff/workshop/{}", order.id))
        .await
        .assert_not_found();
    w.app
        .htmx()
        .post(
            "/staff/workshop/move",
            &[
                ("card", &card),
                ("column", "in_progress"),
                ("position", "0"),
            ],
        )
        .await
        .assert_not_found();
    // North's cashier may look, not move.
    w.app.acting_as(&w.cashier);
    w.app
        .get(&format!("/staff/workshop/{}", order.id))
        .await
        .assert_ok();
    w.app
        .htmx()
        .post(
            "/staff/workshop/move",
            &[
                ("card", &card),
                ("column", "in_progress"),
                ("position", "0"),
            ],
        )
        .await
        .assert_forbidden();
    let _ = w.south;
}

#[renox::test]
async fn ready_work_is_paid_at_the_counter_then_collected() {
    let w = world().await;
    let db = w.app.db();
    let mut order = booking(&w, 0, vec![w.tune.clone()]).await.unwrap().unwrap();
    order.status = WorkStatus::Ready;
    order.save(db).await.unwrap();
    w.app.acting_as(&w.mechanic);
    // Not paid: it can't be collected.
    w.app
        .post(
            &format!("/staff/workshop/{}/status", order.id),
            &[("status", "completed")],
        )
        .await
        .assert_status(409);
    // The cashier takes the payment.
    w.app.acting_as(&w.cashier);
    w.app
        .post(
            &format!("/staff/workshop/{}/pay", order.id),
            &[("method", "cash")],
        )
        .await
        .assert_status(303);
    assert!(
        WorkOrder::find(db, order.id)
            .await
            .unwrap()
            .unwrap()
            .paid_at
            .is_some()
    );
    w.app.acting_as(&w.mechanic);
    w.app
        .post(
            &format!("/staff/workshop/{}/status", order.id),
            &[("status", "completed")],
        )
        .await
        .assert_status(303);
    let done = WorkOrder::find(db, order.id).await.unwrap().unwrap();
    assert_eq!(done.status, WorkStatus::Completed);
    assert!(done.completed_at.is_some());
}

#[renox::test]
async fn a_collected_fleet_repair_puts_the_bike_back_serviced() {
    let w = world().await;
    let db = w.app.db();
    let mut bike = fixtures::bike(db, w.south.id, w.north.id).await.unwrap();
    bike.status = BikeStatus::Maintenance;
    bike.ridden_hours = 420;
    bike.save(db).await.unwrap();
    let order = work_orders()
        .fleet_repair(&bike, w.north.id)
        .in_progress()
        .create_one(db)
        .await
        .unwrap();
    assert_eq!(order.billed_store_id, Some(w.south.id));
    w.app.acting_as(&w.mechanic);
    w.app
        .get("/staff/workshop")
        .await
        .assert_ok()
        .assert_see(&bike.frame_number);
    for to in ["ready", "completed"] {
        w.app
            .post(
                &format!("/staff/workshop/{}/status", order.id),
                &[("status", to)],
            )
            .await
            .assert_status(303);
    }
    let bike = RentalBike::find(db, bike.id).await.unwrap().unwrap();
    assert_eq!(
        (bike.status, bike.serviced_at_hours),
        (BikeStatus::Available, 420)
    );
}

#[renox::test]
async fn tomorrows_bookings_are_reminded_once() {
    let w = world().await;
    w.app.fake_notifications();
    booking(&w, 1, vec![w.check.clone()])
        .await
        .unwrap()
        .unwrap();
    booking(&w, 3, vec![w.check.clone()])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(tasks::reminders(w.app.state()).await.unwrap(), 1);
    assert_eq!(tasks::reminders(w.app.state()).await.unwrap(), 0);
    w.app.assert_notified(&w.rider, "workshop-reminder");
}

#[renox::test]
async fn the_board_loads_in_a_fixed_number_of_queries() {
    let w = world().await;
    let db = w.app.db();
    let mut north = w.north.clone();
    north.workshop_minutes_per_day = 10_000;
    north.save(db).await.unwrap();
    for _ in 0..2 {
        booking(&w, 0, vec![w.check.clone()])
            .await
            .unwrap()
            .unwrap();
    }
    let bike = fixtures::bike(db, w.north.id, w.north.id).await.unwrap();
    work_orders()
        .fleet_repair(&bike, w.north.id)
        .create_one(db)
        .await
        .unwrap();
    w.app.acting_as(&w.mechanic);
    w.app.get("/staff/workshop").await.assert_ok();
    let (_, few) = capture_queries(w.app.get("/staff/workshop")).await;
    for _ in 0..8 {
        booking(&w, 0, vec![w.check.clone()])
            .await
            .unwrap()
            .unwrap();
    }
    for _ in 0..3 {
        let bike = fixtures::bike(db, w.north.id, w.north.id).await.unwrap();
        work_orders()
            .fleet_repair(&bike, w.north.id)
            .create_one(db)
            .await
            .unwrap();
    }
    let (_, many) = capture_queries(w.app.get("/staff/workshop")).await;
    assert_eq!(
        few.len(),
        many.len(),
        "the board's queries don't grow with its cards"
    );
}

#[renox::test]
async fn only_who_works_on_work_orders_takes_a_walk_in() {
    let w = world().await;
    let check = w.check.id.to_string();
    let today = day(0).to_string();
    let form = [
        ("new_name", "Walk Inn"),
        ("bike", "Old cruiser"),
        ("tasks", check.as_str()),
        ("day", today.as_str()),
    ];
    // The cashier sees the board (`workorders.view`) but works on no bike.
    w.app.acting_as(&w.cashier);
    let board = w.app.get("/staff/workshop").await.assert_ok().text();
    assert!(
        !board.contains("href=\"/staff/workshop/new\""),
        "no button to a form they can't send"
    );
    w.app.get("/staff/workshop/new").await.assert_forbidden();
    w.app
        .post("/staff/workshop/new", &form)
        .await
        .assert_forbidden();
    assert!(
        WorkOrder::query()
            .where_(WorkOrder::SOURCE.eq(WorkSource::WalkIn))
            .first(w.app.db())
            .await
            .unwrap()
            .is_none()
    );
    // The mechanic does (`workorders.update` in their store).
    w.app.acting_as(&w.mechanic);
    w.app
        .post("/staff/workshop/new", &form)
        .await
        .assert_status(303);
    let walk = WorkOrder::query()
        .where_(WorkOrder::SOURCE.eq(WorkSource::WalkIn))
        .first(w.app.db())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(walk.store_id, w.north.id);
}

#[renox::test]
async fn every_workshop_page_answers() {
    let w = world().await;
    let order = booking(&w, 2, vec![w.check.clone()])
        .await
        .unwrap()
        .unwrap();
    w.app.acting_as(&w.rider);
    for page in [
        "/bikes".to_owned(),
        format!("/bikes/{}", w.bike.id),
        "/service/book".to_owned(),
        format!("/service/{}", order.id),
    ] {
        w.app.get(&page).await.assert_ok();
    }
    w.app.acting_as(&w.mechanic);
    for page in [
        "/staff/workshop".to_owned(),
        "/staff/workshop/new".to_owned(),
        format!("/staff/workshop/{}", order.id),
    ] {
        w.app.get(&page).await.assert_ok();
    }
    w.app
        .get("/staff/workshop/customers?q=Rid")
        .await
        .assert_ok()
        .assert_see("Rider");
    // A walk-in for today, with a new customer: checked in at once.
    let check = w.check.id.to_string();
    let today = day(0).to_string();
    w.app
        .post(
            "/staff/workshop/new",
            &[
                ("new_name", "Walk Inn"),
                ("bike", "Old cruiser"),
                ("tasks", &check),
                ("day", &today),
            ],
        )
        .await
        .assert_status(303);
    let walk = WorkOrder::query()
        .where_(WorkOrder::SOURCE.eq(WorkSource::WalkIn))
        .first(w.app.db())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(walk.status, WorkStatus::CheckedIn);
    w.app.post("/locale/es", &[]).await;
    w.app
        .get("/staff/workshop")
        .await
        .assert_ok()
        .assert_see("Panel del taller");
}
