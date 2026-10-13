//! Multi-store operations (#245): every booking rule between stores
//! (rental, late and damage fees, the deposit left out, a fleet repair, a
//! consigned sale and its return) with the company adding up to nothing;
//! staff lent to another store with a dated role that ends by itself (and
//! early), with rights in the active store only, audited; bikes placed,
//! called back and sent home, with who may do what (an ABAC matrix) and a
//! recall racing a booking; monthly settlements balanced per pair and
//! confirmed by both stores; fee rates changed by the owner only; 404s for
//! other stores; the pages and their queries.

use bikeshop::app::access::catalogue::{CASHIER, MANAGER, OWNER, STAFF};
use bikeshop::app::accounts::model::Customer;
use bikeshop::app::multistore::books;
use bikeshop::app::multistore::intercompany::positions;
use bikeshop::app::multistore::model::{
    EntryKind, IntercompanyEntry, Settlement, SettlementStatus,
};
use bikeshop::app::multistore::placements::{self, Recall};
use bikeshop::app::multistore::settlements;
use bikeshop::app::rentals::booking::{self, NewRental};
use bikeshop::app::rentals::factories::{PlacementStates, RentalStates, rentals};
use bikeshop::app::rentals::model::{
    BikePlacement, BikeStatus, DepositStatus, PlacementStatus, Rental, RentalBike,
};
use bikeshop::app::staff::model::{HelpStatus, Staff, StaffHelpRequest, Store, fee};
use bikeshop::app::workshop::factories::WorkOrderStates;
use bikeshop::app::workshop::model::{WorkOrder, WorkStatus};
use bikeshop::app::workshop::status;
use bikeshop::seed::fixtures;
use renox::chrono::{Datelike, Duration as Span};
use renox::db::capture_queries;
use renox::prelude::*;
use renox::testing::TestApp;
use std::time::Duration;

const HOUR: Duration = Duration::from_secs(60 * 60);
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

struct World {
    app: TestApp,
    north: Store,
    south: Store,
    west: Store,
    owner: User,
    manager_north: User,
    manager_south: User,
    manager_west: User,
    cashier_north: User,
    cashier_south: User,
    /// North's bike, standing at North.
    bike: RentalBike,
}

async fn world() -> World {
    let app = TestApp::new(bikeshop::app()).await;
    let db = app.db();
    fixtures::roles(db).await.unwrap();
    let north = fixtures::store(db, "North").await.unwrap();
    let south = fixtures::store(db, "South").await.unwrap();
    let west = fixtures::store(db, "West").await.unwrap();
    let owner = fixtures::person(db, "owner@example.com", &[(OWNER, None)])
        .await
        .unwrap();
    let manager_north = fixtures::person(db, "mn@example.com", &[(MANAGER, Some(north.id))])
        .await
        .unwrap();
    let manager_south = fixtures::person(db, "ms@example.com", &[(MANAGER, Some(south.id))])
        .await
        .unwrap();
    let manager_west = fixtures::person(db, "mw@example.com", &[(MANAGER, Some(west.id))])
        .await
        .unwrap();
    let cashier_north = fixtures::person(db, "cn@example.com", &[(CASHIER, Some(north.id))])
        .await
        .unwrap();
    let cashier_south = fixtures::person(db, "cs@example.com", &[(CASHIER, Some(south.id))])
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
        owner,
        manager_north,
        manager_south,
        manager_west,
        cashier_north,
        cashier_south,
        bike,
    }
}

/// North's bike moved to South through a placement, `days` ago.
async fn place_at_south(w: &World) -> (RentalBike, BikePlacement) {
    let db = w.app.db();
    let mut bike = w.bike.clone();
    bike.location_store_id = w.south.id;
    bike.save(db).await.unwrap();
    let placement = BikePlacement::factory()
        .of(&bike, w.south.id)
        .moved(3)
        .create_one(db)
        .await
        .unwrap();
    (bike, placement)
}

async fn customer(db: &Db) -> Customer {
    Customer::create(
        db,
        Customer {
            name: "Rider".into(),
            email: Some(format!("rider{}@example.com", bikeshop::seed::unique())),
            active: true,
            ..Default::default()
        },
    )
    .await
    .unwrap()
}

fn owed(entries: &[IntercompanyEntry], debtor: i64, creditor: i64, kind: EntryKind) -> i64 {
    entries
        .iter()
        .filter(|e| {
            e.debtor_store_id == debtor && e.creditor_store_id == creditor && e.kind == kind
        })
        .map(|e| e.amount)
        .sum()
}

fn company_adds_up(entries: &[IntercompanyEntry]) {
    let triples: Vec<(i64, i64, i64)> = entries
        .iter()
        .map(|e| (e.debtor_store_id, e.creditor_store_id, e.amount))
        .collect();
    assert_eq!(
        positions(&triples).values().sum::<i64>(),
        0,
        "the stores' positions add up to the company's: nothing"
    );
}

#[renox::test]
async fn a_bike_owned_by_north_rented_out_by_south_books_revenue_to_north_and_a_fee_to_south() {
    let w = world().await;
    let db = w.app.db();
    let (bike, _) = place_at_south(&w).await;
    let c = customer(db).await;
    let mut rental = rentals()
        .of_bike(&bike)
        .for_customer(c.id)
        .active()
        .create_one(db)
        .await
        .unwrap();
    rental.price = 4_500;
    rental.deposit_status = DepositStatus::Held;
    rental.due_at = renox::db::now() + HOUR;
    rental.save(db).await.unwrap();
    assert_eq!(
        (rental.owner_store_id, rental.operating_store_id),
        (w.north.id, w.south.id)
    );

    // Back at South two hours late, with a damage fee.
    w.app.travel(HOUR * 3);
    w.app.acting_as(&w.cashier_south);
    w.app
        .post_multipart(
            &format!("/staff/rentals/{}/return", rental.id),
            &[
                ("damaged", "on"),
                ("damage_fee", "80"),
                ("damage_note", "Bent wheel"),
                ("method", "card"),
            ],
            &[],
        )
        .await
        .assert_status(303);
    let back = Rental::find(db, rental.id).await.unwrap().unwrap();
    assert!(back.late_fee > 0);
    let entries = IntercompanyEntry::query()
        .where_(IntercompanyEntry::SOURCE_TYPE.eq("rentals"))
        .where_(IntercompanyEntry::SOURCE_ID.eq(rental.id))
        .get(db)
        .await
        .unwrap();
    // The price to the owner, the operating store's fee at its rate, late
    // and damage fees to the owner; the deposit isn't booked.
    assert_eq!(
        owed(&entries, w.south.id, w.north.id, EntryKind::RentalRevenue),
        4_500
    );
    assert_eq!(
        owed(&entries, w.north.id, w.south.id, EntryKind::OperatingFee),
        fee(4_500, w.south.fee_rate_bp)
    );
    assert_eq!(
        owed(&entries, w.south.id, w.north.id, EntryKind::LateFee),
        back.late_fee
    );
    assert_eq!(
        owed(&entries, w.south.id, w.north.id, EntryKind::DamageFee),
        8_000
    );
    assert_eq!(entries.len(), 4);
    let fee_entry = entries
        .iter()
        .find(|e| e.kind == EntryKind::OperatingFee)
        .unwrap();
    assert_eq!(fee_entry.fee_rate_bp, Some(w.south.fee_rate_bp));
    company_adds_up(&entries);
    // Booked once, however often the event comes.
    books::rental(db, rental.id).await.unwrap();
    assert_eq!(
        IntercompanyEntry::query()
            .where_(IntercompanyEntry::SOURCE_ID.eq(rental.id))
            .where_(IntercompanyEntry::SOURCE_TYPE.eq("rentals"))
            .count(db)
            .await
            .unwrap(),
        4
    );

    // A rental of North's bike served by North books nothing.
    let mut home_bike = fixtures::bike(db, w.north.id, w.north.id).await.unwrap();
    home_bike.daily_rate = 4_000;
    home_bike.save(db).await.unwrap();
    let own = rentals()
        .of_bike(&home_bike)
        .for_customer(c.id)
        .active()
        .create_one(db)
        .await
        .unwrap();
    assert!(books::rental(db, own.id).await.unwrap().is_empty());
}

#[renox::test]
async fn a_fleet_repair_by_another_store_is_charged_to_the_owner() {
    let w = world().await;
    let db = w.app.db();
    let mut order: WorkOrder = WorkOrder::factory()
        .fleet_repair(&w.bike, w.west.id)
        .create_one(db)
        .await
        .unwrap();
    order.billed_store_id = Some(w.north.id);
    order.total = 17_500;
    order.save(db).await.unwrap();
    status::set_status(w.app.state(), &mut order, WorkStatus::Completed)
        .await
        .unwrap();
    let entries = IntercompanyEntry::query()
        .where_(IntercompanyEntry::SOURCE_TYPE.eq("work_orders"))
        .get(db)
        .await
        .unwrap();
    assert_eq!(
        owed(&entries, w.north.id, w.west.id, EntryKind::Repair),
        17_500
    );
    assert_eq!(entries.len(), 1);
    company_adds_up(&entries);
}

#[renox::test]
async fn consigned_sales_and_returns_book_both_ways_and_the_rate_in_force_is_copied() {
    let w = world().await;
    let db = w.app.db();
    let mut tx = db.begin().await.unwrap();
    books::consigned_sale(&mut tx, 77, w.south.id, w.north.id, 40_000, false)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    // The owner changes South's rate; the sale's entry keeps 20 %.
    Store::query()
        .where_(Store::ID.eq(w.south.id))
        .update(db, &[("fee_rate_bp", &2_500)])
        .await
        .unwrap();
    let mut tx = db.begin().await.unwrap();
    books::consigned_sale(&mut tx, 77, w.south.id, w.north.id, 40_000, true)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let entries = IntercompanyEntry::query()
        .order_by(IntercompanyEntry::ID)
        .get(db)
        .await
        .unwrap();
    assert_eq!(entries.len(), 4);
    assert_eq!(
        (entries[1].kind, entries[1].amount, entries[1].fee_rate_bp),
        (EntryKind::SellingFee, 8_000, Some(2_000))
    );
    assert_eq!(
        (entries[3].kind, entries[3].amount, entries[3].fee_rate_bp),
        (EntryKind::SellingFee, 10_000, Some(2_500))
    );
    // The return reverses the sale.
    assert_eq!(
        owed(&entries, w.north.id, w.south.id, EntryKind::SaleRevenue),
        40_000
    );
    company_adds_up(&entries);
}

#[renox::test]
async fn a_help_role_works_in_the_helped_store_only_and_ends_by_itself() {
    let w = world().await;
    let db = w.app.db();
    let helper_staff = Staff::of_user(db, w.cashier_north.id)
        .await
        .unwrap()
        .unwrap();
    let today = bikeshop::seed::today();
    // South asks North for its cashier, as staff, today and tomorrow.
    w.app.acting_as(&w.manager_south);
    w.app
        .post(
            "/staff/help",
            &[
                ("store", &w.north.id.to_string()),
                ("staff", &helper_staff.id.to_string()),
                ("role", STAFF),
                ("starts_on", &today.to_string()),
                ("ends_on", &(today + Span::days(1)).to_string()),
                ("reason", "Market weekend"),
            ],
        )
        .await
        .assert_redirect("/staff/help");
    let request = StaffHelpRequest::query().first(db).await.unwrap().unwrap();
    assert_eq!(request.status, HelpStatus::Requested);
    // South can't approve its own request; West doesn't see it.
    w.app
        .post(&format!("/staff/help/{}/approve", request.id), &[])
        .await
        .assert_forbidden();
    w.app.acting_as(&w.manager_west);
    w.app
        .post(&format!("/staff/help/{}/approve", request.id), &[])
        .await
        .assert_not_found();
    w.app.acting_as(&w.manager_north);
    w.app
        .get("/staff/help")
        .await
        .assert_ok()
        .assert_see("Market weekend");
    w.app
        .post(&format!("/staff/help/{}/approve", request.id), &[])
        .await
        .assert_redirect("/staff/help");

    // The helper works at South now, with staff rights there only.
    w.app.acting_as(&w.cashier_north);
    w.app
        .post(&format!("/staff/store/{}", w.south.id), &[])
        .await
        .assert_status(303);
    w.app.get("/staff/rentals").await.assert_ok();
    w.app.get("/staff/identities").await.assert_forbidden();
    w.app
        .post(&format!("/staff/store/{}", w.north.id), &[])
        .await
        .assert_status(303);
    w.app.get("/staff/identities").await.assert_ok();
    // The audit trail names the store and the role used there.
    let data: String =
        renox::db::sql("SELECT data FROM audit_logs WHERE action = 'staff_help.approved'")
            .scalar(db)
            .await
            .unwrap();
    assert!(
        data.contains(&format!("\"store_id\":{}", w.north.id)),
        "{data}"
    );
    assert!(data.contains(MANAGER), "{data}");

    // Two days later the role has ended by itself: South is gone.
    w.app.travel(DAY * 2);
    w.app.acting_as(&w.cashier_north); // two days on, the session expired
    w.app
        .post(&format!("/staff/store/{}", w.south.id), &[])
        .await
        .assert_forbidden();
    let switcher = w.app.get("/staff").await;
    switcher.assert_ok().assert_dont_see("Switch store");
}

#[renox::test]
async fn either_store_ends_help_early_and_hours_are_counted_not_charged() {
    let w = world().await;
    let db = w.app.db();
    let helper_staff = Staff::of_user(db, w.cashier_north.id)
        .await
        .unwrap()
        .unwrap();
    let request = StaffHelpRequest::create(
        db,
        StaffHelpRequest {
            from_store_id: w.north.id,
            to_store_id: w.south.id,
            staff_id: helper_staff.id,
            role: STAFF.into(),
            starts_at: renox::db::now() - Span::hours(1),
            ends_at: renox::db::now() + Span::days(5),
            reason: "Short-handed".into(),
            status: HelpStatus::Requested,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    w.app.acting_as(&w.manager_north);
    w.app
        .post(&format!("/staff/help/{}/approve", request.id), &[])
        .await
        .assert_status(303);
    // South logs a day's hours.
    w.app.acting_as(&w.manager_south);
    w.app
        .post(
            &format!("/staff/help/{}/hours", request.id),
            &[
                ("worked_on", &bikeshop::seed::today().to_string()),
                ("hours", "7.5"),
            ],
        )
        .await
        .assert_status(303);
    w.app
        .get("/staff/help/hours")
        .await
        .assert_ok()
        .assert_see(r#"<td class="rx-num">7.5</td>"#);
    // North, who lent the cashier, sees their person's hours at South too;
    // West, in neither store, doesn't. (The table's cell: "7.5" alone is
    // also in an icon's path on every page.)
    w.app.acting_as(&w.manager_north);
    w.app
        .get("/staff/help/hours")
        .await
        .assert_ok()
        .assert_see(r#"<td class="rx-num">7.5</td>"#);
    w.app.acting_as(&w.manager_west);
    w.app
        .get("/staff/help/hours")
        .await
        .assert_ok()
        .assert_dont_see(r#"<td class="rx-num">7.5</td>"#);
    w.app.acting_as(&w.manager_south);
    // South ends it early: the helper can't work there any more.
    w.app
        .post(&format!("/staff/help/{}/end", request.id), &[])
        .await
        .assert_status(303);
    assert_eq!(
        StaffHelpRequest::find(db, request.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        HelpStatus::EndedEarly
    );
    w.app.acting_as(&w.cashier_north);
    w.app
        .post(&format!("/staff/store/{}", w.south.id), &[])
        .await
        .assert_forbidden();
    // Never charged.
    assert_eq!(IntercompanyEntry::query().count(db).await.unwrap(), 0);
}

#[renox::test]
async fn who_may_do_what_to_a_bike_of_north_standing_at_south() {
    let w = world().await;
    let db = w.app.db();
    let (bike, placement) = place_at_south(&w).await;
    let fleet = format!("/staff/fleet/{}", bike.id);
    let recall = format!("/staff/placements/{}/recall", placement.id);
    // West: doesn't exist.
    w.app.acting_as(&w.manager_west);
    w.app.get(&fleet).await.assert_not_found();
    w.app.post(&recall, &[]).await.assert_not_found();
    w.app
        .post(&format!("/staff/placements/send-back/{}", bike.id), &[])
        .await
        .assert_not_found();
    // South (location): sees it, may rent it out, may not call it back or retire it.
    w.app.acting_as(&w.manager_south);
    w.app.get(&fleet).await.assert_ok();
    w.app.post(&recall, &[]).await.assert_forbidden();
    w.app
        .post(&format!("/staff/stock/fleet/{}/retire", bike.id), &[])
        .await
        .assert_forbidden();
    w.app
        .get("/staff/placements")
        .await
        .assert_ok()
        .assert_see(&bike.frame_number);
    // North (owner): calls it back.
    w.app.acting_as(&w.manager_north);
    w.app.get(&fleet).await.assert_ok();
    w.app
        .post(&recall, &[])
        .await
        .assert_redirect("/staff/placements");
    let home = RentalBike::find(db, bike.id).await.unwrap().unwrap();
    assert_eq!(home.location_store_id, w.north.id);
    assert_eq!(
        BikePlacement::find(db, placement.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        PlacementStatus::Recalled
    );
    // The owner (a global role) may do all of it, anywhere.
    let (_, again) = place_at_south(&w).await;
    w.app.acting_as(&w.owner);
    w.app
        .post(&format!("/staff/placements/{}/recall", again.id), &[])
        .await
        .assert_redirect("/staff/placements");
}

#[renox::test]
async fn a_bike_is_asked_for_approved_moved_and_sent_home_from_a_third_store() {
    let w = world().await;
    let db = w.app.db();
    // South asks for North's bike; North approves and moves it.
    w.app.acting_as(&w.manager_south);
    w.app
        .get("/staff/placements/new?direction=ask")
        .await
        .assert_ok()
        .assert_see(&w.bike.frame_number);
    w.app
        .post(
            "/staff/placements",
            &[
                ("bike", &w.bike.id.to_string()),
                ("direction", "ask"),
                ("store", &w.north.id.to_string()),
            ],
        )
        .await
        .assert_redirect("/staff/placements");
    let placement = BikePlacement::query().first(db).await.unwrap().unwrap();
    assert_eq!(placement.status, PlacementStatus::Requested);
    w.app.acting_as(&w.manager_north);
    w.app
        .post(
            &format!("/staff/placements/{}/decide", placement.id),
            &[("decision", "approve")],
        )
        .await
        .assert_status(303);
    w.app
        .post(&format!("/staff/placements/{}/move", placement.id), &[])
        .await
        .assert_status(303);
    assert_eq!(
        RentalBike::find(db, w.bike.id)
            .await
            .unwrap()
            .unwrap()
            .location_store_id,
        w.south.id
    );
    // It comes back at West after a rental: West sees "send back to South".
    let mut bike = RentalBike::find(db, w.bike.id).await.unwrap().unwrap();
    bike.location_store_id = w.west.id;
    bike.save(db).await.unwrap();
    w.app.acting_as(&w.manager_west);
    w.app
        .get("/staff/placements")
        .await
        .assert_ok()
        .assert_see("Send back to South");
    w.app
        .post(&format!("/staff/placements/send-back/{}", bike.id), &[])
        .await
        .assert_redirect("/staff/placements");
    let home = RentalBike::find(db, bike.id).await.unwrap().unwrap();
    assert_eq!(
        (home.location_store_id, home.owner_store_id),
        (w.south.id, w.north.id)
    );
}

#[renox::test]
async fn a_recall_never_takes_a_bike_from_under_a_rental() {
    let w = world().await;
    let db = w.app.db();
    let (bike, mut placement) = place_at_south(&w).await;
    let c = customer(db).await;
    // Out with a customer at South: the recall is refused, nothing moves.
    let rental = rentals()
        .of_bike(&bike)
        .for_customer(c.id)
        .active()
        .create_one(db)
        .await
        .unwrap();
    assert_eq!(
        placements::recall_bike(db, &mut placement).await.unwrap(),
        Recall::Busy
    );
    w.app.acting_as(&w.manager_north);
    w.app
        .post(&format!("/staff/placements/{}/recall", placement.id), &[])
        .await
        .assert_status(409);
    assert_eq!(
        RentalBike::find(db, bike.id)
            .await
            .unwrap()
            .unwrap()
            .location_store_id,
        w.south.id
    );
    let mut done = rental.clone();
    done.status = bikeshop::app::rentals::model::RentalStatus::Returned;
    done.save(db).await.unwrap();

    // A recall and a booking at the same moment: exactly one wins.
    let start = renox::db::now() + Span::hours(2);
    let booking = {
        let db = db.clone();
        let new = NewRental {
            bike_id: bike.id,
            customer_id: c.id,
            operating_store_id: w.south.id,
            start,
            end: start + Span::hours(3),
            served_by: None,
        };
        renox::tokio::spawn(async move { booking::book(&db, new).await.unwrap() })
    };
    let recall = {
        let db = db.clone();
        let mut placement = placement.clone();
        renox::tokio::spawn(
            async move { placements::recall_bike(&db, &mut placement).await.unwrap() },
        )
    };
    let booked = booking.await.unwrap().is_ok();
    let recalled = recall.await.unwrap() == Recall::Done;
    assert!(booked != recalled, "booked {booked}, recalled {recalled}");
    let after = RentalBike::find(db, bike.id).await.unwrap().unwrap();
    if booked {
        assert_eq!(after.location_store_id, w.south.id);
    } else {
        assert_eq!(after.location_store_id, w.north.id);
        assert_eq!(after.status, BikeStatus::Available);
    }
}

/// An entry booked `days` into last month.
async fn entry(db: &Db, debtor: i64, creditor: i64, amount: i64, at: DateTime) {
    IntercompanyEntry::create(
        db,
        IntercompanyEntry {
            debtor_store_id: debtor,
            creditor_store_id: creditor,
            amount,
            kind: EntryKind::RentalRevenue,
            source_type: "rentals".into(),
            source_id: 1,
            booked_at: at,
            ..Default::default()
        },
    )
    .await
    .unwrap();
}

#[renox::test]
async fn monthly_settlements_balance_per_pair_and_both_stores_confirm() {
    let w = world().await;
    let db = w.app.db();
    let today = bikeshop::seed::today();
    let last_month = settlements::month_of(settlements::month_of(today) - Span::days(1));
    let at = last_month.and_hms_opt(12, 0, 0).unwrap().and_utc() + Span::days(3);
    let (n, s, x) = (w.north.id, w.south.id, w.west.id);
    entry(db, s, n, 90_000, at).await;
    entry(db, n, s, 18_000, at).await;
    entry(db, s, n, 5_000, at).await;
    entry(db, n, x, 12_000, at).await;
    entry(db, x, n, 3_000, at).await;
    // This month's entry waits for next month.
    entry(db, s, n, 1_000, renox::db::now()).await;

    let made = w
        .app
        .at_travelled_time(settlements::settle_month(w.app.state(), last_month))
        .await
        .unwrap();
    assert_eq!(made.len(), 2);
    let ns = made
        .iter()
        .find(|m| [m.debtor_store_id, m.creditor_store_id].contains(&s))
        .unwrap();
    assert_eq!(
        (ns.debtor_store_id, ns.creditor_store_id, ns.amount),
        (s, n, 77_000)
    );
    let nw = made
        .iter()
        .find(|m| [m.debtor_store_id, m.creditor_store_id].contains(&x))
        .unwrap();
    assert_eq!(
        (nw.debtor_store_id, nw.creditor_store_id, nw.amount),
        (n, x, 9_000)
    );
    // Each settlement holds exactly its pair's entries, and nets them.
    for settlement in &made {
        let held = IntercompanyEntry::query()
            .where_(IntercompanyEntry::SETTLEMENT_ID.eq(settlement.id))
            .get(db)
            .await
            .unwrap();
        let net: i64 = held
            .iter()
            .map(|e| {
                if e.debtor_store_id == settlement.debtor_store_id {
                    e.amount
                } else {
                    -e.amount
                }
            })
            .sum();
        assert_eq!(net, settlement.amount);
    }
    assert_eq!(
        IntercompanyEntry::query()
            .where_(IntercompanyEntry::SETTLEMENT_ID.is_null())
            .count(db)
            .await
            .unwrap(),
        1
    );
    let all = IntercompanyEntry::query().get(db).await.unwrap();
    company_adds_up(&all);
    // Twice is once.
    let again = settlements::settle_month(w.app.state(), last_month)
        .await
        .unwrap();
    assert!(again.is_empty());
    assert_eq!(Settlement::query().count(db).await.unwrap(), 2);

    // The statements go out as a batch: each manager of both stores and
    // the owner get it, with the entries attached.
    w.app.run_jobs().await;
    let mails = w.app.sent_mail();
    for to in ["ms@example.com", "mn@example.com", "owner@example.com"] {
        assert!(
            mails
                .iter()
                .any(|m| m.to.contains(&to.to_owned()) && m.subject.contains("Statement")),
            "{to} got the statement"
        );
    }
    assert!(
        !mails
            .iter()
            .any(|m| m.to.contains(&"cs@example.com".to_owned()))
    );

    // Confirmed by both stores: managers may see but not settle; the owner settles either side.
    let url = format!("/staff/books/settlements/{}", ns.id);
    w.app.acting_as(&w.manager_south);
    w.app.get(&url).await.assert_ok().assert_see("$770.00");
    w.app
        .post(&format!("{url}/confirm"), &[("side", "debtor")])
        .await
        .assert_forbidden();
    w.app.acting_as(&w.manager_west);
    w.app.get(&url).await.assert_not_found();
    w.app.acting_as(&w.owner);
    w.app
        .post(&format!("{url}/confirm"), &[("side", "debtor")])
        .await
        .assert_status(303);
    assert_eq!(
        Settlement::find(db, ns.id).await.unwrap().unwrap().status,
        SettlementStatus::Open
    );
    w.app
        .post(&format!("{url}/confirm"), &[("side", "creditor")])
        .await
        .assert_status(303);
    let settled = Settlement::find(db, ns.id).await.unwrap().unwrap();
    assert_eq!(settled.status, SettlementStatus::Settled);
    assert_eq!(settled.settled_by, Some(w.owner.id));
    w.app
        .post(&format!("{url}/confirm"), &[("side", "creditor")])
        .await
        .assert_status(409);
}

#[renox::test]
async fn the_monthly_task_settles_last_month_on_the_first() {
    let w = world().await;
    let db = w.app.db();
    let today = bikeshop::seed::today();
    let to_first = settlements::next_month(today)
        .signed_duration_since(today)
        .num_days();
    entry(db, w.south.id, w.north.id, 5_000, renox::db::now()).await;
    w.app.travel(DAY * (to_first as u32) + HOUR * 3);
    w.app.run_scheduled("books:settle").await.unwrap();
    let made = Settlement::query().get(db).await.unwrap();
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].period_start, settlements::month_of(today));
    assert_eq!(made[0].period_start.day(), 1);
}

#[renox::test]
async fn only_the_owner_changes_fee_rates_and_it_is_audited() {
    let w = world().await;
    let db = w.app.db();
    let url = format!("/staff/books/fees/{}", w.south.id);
    w.app.acting_as(&w.manager_south);
    w.app
        .get("/staff/books/fees")
        .await
        .assert_ok()
        .assert_see("Only the owner changes fee rates");
    w.app
        .post(&url, &[("percent", "25")])
        .await
        .assert_forbidden();
    w.app.acting_as(&w.owner);
    w.app
        .post(&url, &[("percent", "22.5")])
        .await
        .assert_redirect("/staff/books/fees");
    assert_eq!(
        Store::find(db, w.south.id)
            .await
            .unwrap()
            .unwrap()
            .fee_rate_bp,
        2_250
    );
    let data: String =
        renox::db::sql("SELECT data FROM audit_logs WHERE action = 'store.fee_rate_changed'")
            .scalar(db)
            .await
            .unwrap();
    assert!(
        data.contains("\"from_bp\":2000") && data.contains("\"to_bp\":2250"),
        "{data}"
    );
    w.app
        .get("/staff/books/fees")
        .await
        .assert_ok()
        .assert_see("20 % → 22.5 %");
}

#[renox::test]
async fn every_multistore_page_answers_and_lists_run_a_fixed_number_of_queries() {
    let w = world().await;
    let db = w.app.db();
    let today = bikeshop::seed::today();
    let last_month = settlements::month_of(settlements::month_of(today) - Span::days(1));
    let at = last_month.and_hms_opt(12, 0, 0).unwrap().and_utc();
    entry(db, w.south.id, w.north.id, 10_000, at).await;
    let made = settlements::settle_month(w.app.state(), last_month)
        .await
        .unwrap();
    w.app.acting_as(&w.manager_north);
    for page in [
        "/staff/help".to_owned(),
        "/staff/help/new".to_owned(),
        "/staff/help/hours".to_owned(),
        "/staff/placements".to_owned(),
        "/staff/placements/new".to_owned(),
        "/staff/placements/new?direction=ask".to_owned(),
        "/staff/books".to_owned(),
        "/staff/books?export=csv".to_owned(),
        "/staff/books/fees".to_owned(),
        "/staff/books/settlements".to_owned(),
        format!("/staff/books/settlements/{}", made[0].id),
    ] {
        w.app.get(&page).await.assert_ok();
    }
    // A cashier sees none of the books.
    w.app.acting_as(&w.cashier_north);
    w.app.get("/staff/books").await.assert_forbidden();

    let helper = Staff::of_user(db, w.cashier_south.id)
        .await
        .unwrap()
        .unwrap();
    let ask = |n: i64| StaffHelpRequest {
        from_store_id: w.south.id,
        to_store_id: w.north.id,
        staff_id: helper.id,
        role: STAFF.into(),
        starts_at: renox::db::now() + Span::days(n),
        ends_at: renox::db::now() + Span::days(n + 1),
        reason: format!("Day {n}"),
        ..Default::default()
    };
    StaffHelpRequest::create(db, ask(10)).await.unwrap();
    w.app.acting_as(&w.manager_north);
    let (_, few_books) = capture_queries(w.app.get("/staff/books")).await;
    let (_, few_help) = capture_queries(w.app.get("/staff/help")).await;
    let (_, few_places) = capture_queries(w.app.get("/staff/placements")).await;
    for n in 0..6 {
        entry(db, w.north.id, w.west.id, 1_000 + n, renox::db::now()).await;
        StaffHelpRequest::create(db, ask(n)).await.unwrap();
        let bike = fixtures::bike(db, w.north.id, w.north.id).await.unwrap();
        BikePlacement::factory()
            .of(&bike, w.south.id)
            .create_one(db)
            .await
            .unwrap();
    }
    let (_, many_books) = capture_queries(w.app.get("/staff/books")).await;
    let (_, many_help) = capture_queries(w.app.get("/staff/help")).await;
    let (_, many_places) = capture_queries(w.app.get("/staff/placements")).await;
    assert_eq!(few_books.len(), many_books.len(), "/staff/books");
    assert_eq!(few_help.len(), many_help.len(), "/staff/help");
    assert_eq!(few_places.len(), many_places.len(), "/staff/placements");
}
