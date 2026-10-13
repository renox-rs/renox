//! Stock, consignment, suppliers and purchasing (#240): the ledger and the
//! levels agreeing under concurrent sales, a consignment from request to
//! recall with a sale at the other store booked to the owner, who may do
//! what to whose goods (and a 404 for everyone else), purchase orders with
//! partial receipts and the average cost, the price list import (good rows
//! saved, bad rows reported, large files queued), the daily reorder check,
//! the stock take, bikes between sale stock and the fleet, and the pages'
//! queries.

use bikeshop::app::access::catalogue::{CASHIER, MANAGER};
use bikeshop::app::catalog::model::{Brand, Category, CategoryKind, Product, ProductVariant};
use bikeshop::app::multistore::model::{EntryKind, IntercompanyEntry};
use bikeshop::app::rentals::model::{BikeStatus, RentalBike};
use bikeshop::app::staff::model::{Store, fee};
use bikeshop::app::stock::ledger;
use bikeshop::app::stock::model::{
    ConsignmentShipment, ConsignmentShipmentLine, MovementReason, PurchaseOrder, PurchaseStatus,
    ShipmentStatus, StockLevel, StockMovement, Supplier, SupplierItem,
};
use bikeshop::app::stock::purchasing::average_cost;
use bikeshop::app::stock::reorder;
use bikeshop::seed::fixtures;
use renox::db::capture_queries;
use renox::prelude::*;
use renox::testing::TestApp;
use std::time::Duration;

struct World {
    app: TestApp,
    north: Store,
    south: Store,
    west: Store,
    manager_north: User,
    manager_south: User,
    manager_west: User,
    cashier_south: User,
    /// A helmet: 10 at North, owned by North.
    helmet: ProductVariant,
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

/// `quantity` of `variant` owned by `owner` arriving at `location`, through the ledger.
async fn put(db: &Db, variant: i64, owner: i64, location: i64, quantity: i64) -> StockLevel {
    let mut tx = db.begin().await.unwrap();
    StockMovement::record(
        &mut tx,
        StockMovement {
            variant_id: variant,
            owner_store_id: owner,
            location_store_id: location,
            quantity,
            reason: if owner == location {
                MovementReason::Purchase
            } else {
                MovementReason::ConsignIn
            },
            ..Default::default()
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    level(db, variant, owner, location).await
}

async fn level(db: &Db, variant: i64, owner: i64, location: i64) -> StockLevel {
    StockLevel::query()
        .where_(StockLevel::VARIANT_ID.eq(variant))
        .where_(StockLevel::OWNER_STORE_ID.eq(owner))
        .where_(StockLevel::LOCATION_STORE_ID.eq(location))
        .first(db)
        .await
        .unwrap()
        .unwrap_or_default()
}

async fn world() -> World {
    let app = TestApp::new(bikeshop::app()).await;
    let db = app.db();
    fixtures::roles(db).await.unwrap();
    let north = fixtures::store(db, "North").await.unwrap();
    let south = fixtures::store(db, "South").await.unwrap();
    let west = fixtures::store(db, "West").await.unwrap();
    let manager_north = fixtures::person(db, "mn@example.com", &[(MANAGER, Some(north.id))])
        .await
        .unwrap();
    let manager_south = fixtures::person(db, "ms@example.com", &[(MANAGER, Some(south.id))])
        .await
        .unwrap();
    let manager_west = fixtures::person(db, "mw@example.com", &[(MANAGER, Some(west.id))])
        .await
        .unwrap();
    let cashier_south = fixtures::person(db, "cs@example.com", &[(CASHIER, Some(south.id))])
        .await
        .unwrap();
    let helmet = variant(db, "Helmet", CategoryKind::Gear, 4_500).await;
    put(db, helmet.id, north.id, north.id, 10).await;
    World {
        app,
        north,
        south,
        west,
        manager_north,
        manager_south,
        manager_west,
        cashier_south,
        helmet,
    }
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

#[renox::test]
async fn ledger_and_levels_agree_under_concurrent_sales() {
    let w = world().await;
    let db = w.app.db();
    // Twelve sales of two helmets at once, ten helmets on the shelf: five
    // can be served, the others find the units gone and write nothing.
    let sales: Vec<_> = (0..12)
        .map(|_| {
            let db = db.clone();
            let variant = w.helmet.id;
            let store = w.north.id;
            renox::tokio::spawn(async move {
                let mut tx = db.begin().await.unwrap();
                let taken = ledger::take(
                    &mut tx,
                    StockMovement {
                        variant_id: variant,
                        owner_store_id: store,
                        location_store_id: store,
                        quantity: -2,
                        reason: MovementReason::Sale,
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
                tx.commit().await.unwrap();
                taken.is_some()
            })
        })
        .collect();
    let mut served = Vec::new();
    for sale in sales {
        served.push(sale.await.unwrap());
    }
    assert_eq!(served.iter().filter(|s| **s).count(), 5);
    assert_eq!(
        level(db, w.helmet.id, w.north.id, w.north.id).await.on_hand,
        0
    );
    assert_eq!(ledger::mismatches(db).await.unwrap(), vec![]);
}

#[renox::test]
async fn a_consignment_goes_from_request_to_recall_and_sales_at_b_are_booked_to_a() {
    let w = world().await;
    let db = w.app.db();
    // South asks North for five helmets.
    w.app.acting_as(&w.manager_south);
    w.app
        .post(
            "/staff/consignments",
            &[
                ("direction", "ask"),
                ("store", &w.north.id.to_string()),
                ("lines[0][variant]", &w.helmet.id.to_string()),
                ("lines[0][quantity]", "5"),
            ],
        )
        .await
        .assert_status(303);
    let shipment = ConsignmentShipment::query()
        .first(db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(shipment.status, ShipmentStatus::Requested);
    assert_eq!(
        (shipment.owner_store_id, shipment.location_store_id),
        (w.north.id, w.south.id)
    );
    let url = format!("/staff/consignments/{}", shipment.id);
    // Only the owner store approves and ships.
    w.app
        .post(&format!("{url}/decide"), &[("decision", "approve")])
        .await
        .assert_forbidden();
    w.app.acting_as(&w.manager_north);
    w.app.get(&url).await.assert_ok().assert_see("Approve");
    w.app
        .post(&format!("{url}/decide"), &[("decision", "approve")])
        .await
        .assert_status(303);
    w.app
        .post(&format!("{url}/ship"), &[])
        .await
        .assert_status(303);
    // Shipping twice ships once.
    w.app
        .post(&format!("{url}/ship"), &[])
        .await
        .assert_status(409);
    assert_eq!(
        level(db, w.helmet.id, w.north.id, w.north.id).await.on_hand,
        5
    );
    w.app
        .get("/staff/consignments?view=transit")
        .await
        .assert_ok()
        .assert_see(&format!("CS-{}", shipment.id));

    // South receives two now, the other three later: still North's.
    let line = ConsignmentShipmentLine::query()
        .where_(ConsignmentShipmentLine::SHIPMENT_ID.eq(shipment.id))
        .first(db)
        .await
        .unwrap()
        .unwrap();
    w.app.acting_as(&w.manager_south);
    w.app
        .get(&url)
        .await
        .assert_ok()
        .assert_see("Nothing more will come")
        .assert_see("Goods are on their way");
    w.app
        .post(
            &format!("{url}/receive"),
            &[
                ("lines[0][line]", &line.id.to_string()),
                ("lines[0][received]", "2"),
            ],
        )
        .await
        .assert_status(303);
    let partly = ConsignmentShipment::find(db, shipment.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(partly.status, ShipmentStatus::PartlyReceived);
    assert_eq!(
        level(db, w.helmet.id, w.north.id, w.south.id).await.on_hand,
        2
    );
    w.app
        .post(
            &format!("{url}/receive"),
            &[
                ("lines[0][line]", &line.id.to_string()),
                ("lines[0][received]", "3"),
            ],
        )
        .await
        .assert_status(303);
    assert_eq!(
        ConsignmentShipment::find(db, shipment.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        ShipmentStatus::Received
    );
    // South's grid shows them as held for another store.
    w.app
        .get("/staff/stock?view=held")
        .await
        .assert_ok()
        .assert_see("Helmet");

    // Two sold at South's counter: the revenue is North's, South earns its fee.
    w.app.acting_as(&w.cashier_south);
    w.app
        .post(
            "/staff/counter/lines",
            &[("variant_id", &w.helmet.id.to_string()), ("quantity", "2")],
        )
        .await
        .assert_status(303);
    w.app
        .post(
            "/staff/counter/pay",
            &[("method", "cash"), ("tendered", "100")],
        )
        .await
        .assert_status(303);
    assert_eq!(
        level(db, w.helmet.id, w.north.id, w.south.id).await.on_hand,
        3
    );
    let entries = IntercompanyEntry::query().get(db).await.unwrap();
    assert_eq!(
        owed(&entries, w.south.id, w.north.id, EntryKind::SaleRevenue),
        9_000
    );
    assert_eq!(
        owed(&entries, w.north.id, w.south.id, EntryKind::SellingFee),
        fee(9_000, w.south.fee_rate_bp)
    );
    let selling_fee = entries
        .iter()
        .find(|e| e.kind == EntryKind::SellingFee)
        .unwrap();
    assert_eq!(selling_fee.fee_rate_bp, Some(w.south.fee_rate_bp));

    // North calls back what is left; South sends it; North has it again.
    w.app.acting_as(&w.manager_north);
    w.app
        .post(&format!("{url}/recall"), &[])
        .await
        .assert_status(303);
    w.app.acting_as(&w.manager_south);
    w.app
        .post(&format!("{url}/send-back"), &[])
        .await
        .assert_status(303);
    assert_eq!(
        level(db, w.helmet.id, w.north.id, w.south.id).await.on_hand,
        0
    );
    w.app.acting_as(&w.manager_north);
    w.app
        .post(&format!("{url}/receive-back"), &[])
        .await
        .assert_status(303);
    let back = ConsignmentShipment::find(db, shipment.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(back.status, ShipmentStatus::Recalled);
    assert_eq!(
        level(db, w.helmet.id, w.north.id, w.north.id).await.on_hand,
        8
    );
    assert_eq!(ledger::mismatches(db).await.unwrap(), vec![]);
    w.app.get(&url).await.assert_ok().assert_see("Back home");
}

#[renox::test]
async fn the_owner_decides_the_location_operates_and_other_stores_get_404() {
    let w = world().await;
    let db = w.app.db();
    let held = put(db, w.helmet.id, w.north.id, w.south.id, 4).await;
    let ledger_url = format!("/staff/stock/{}", held.id);
    // The location store sees the goods, but may not write them off.
    w.app.acting_as(&w.manager_south);
    w.app
        .get(&ledger_url)
        .await
        .assert_ok()
        .assert_see("Consigned")
        .assert_dont_see("Write off</");
    w.app
        .post(
            &format!("{ledger_url}/write-off"),
            &[("quantity", "1"), ("reason", "dropped")],
        )
        .await
        .assert_forbidden();
    // Another store doesn't know they exist.
    w.app.acting_as(&w.manager_west);
    w.app.get(&ledger_url).await.assert_not_found();
    w.app
        .post(
            &format!("{ledger_url}/write-off"),
            &[("quantity", "1"), ("reason", "dropped")],
        )
        .await
        .assert_not_found();
    // The owner may.
    w.app.acting_as(&w.manager_north);
    w.app
        .post(
            &format!("{ledger_url}/write-off"),
            &[("quantity", "1"), ("reason", "dropped")],
        )
        .await
        .assert_redirect(&ledger_url);
    assert_eq!(
        level(db, w.helmet.id, w.north.id, w.south.id).await.on_hand,
        3
    );
    let audit: i64 =
        renox::db::sql("SELECT COUNT(*) FROM audit_logs WHERE action = 'stock.written_off'")
            .scalar(db)
            .await
            .unwrap();
    assert_eq!(audit, 1);
    // A shipment between North and South is a 404 for West too.
    let shipment = ConsignmentShipment::create(
        db,
        ConsignmentShipment {
            owner_store_id: w.north.id,
            location_store_id: w.south.id,
            status: ShipmentStatus::Requested,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    w.app.acting_as(&w.manager_west);
    w.app
        .get(&format!("/staff/consignments/{}", shipment.id))
        .await
        .assert_not_found();
    w.app
        .post(
            &format!("/staff/consignments/{}/decide", shipment.id),
            &[("decision", "approve")],
        )
        .await
        .assert_not_found();
}

#[test]
fn the_average_cost_weighs_what_is_there_and_what_arrives() {
    assert_eq!(average_cost(10, 225_000, 4, 300_000), 246_429);
    assert_eq!(average_cost(0, 100, 5, 200), 200);
    assert_eq!(average_cost(-3, 100, 5, 200), 200);
    assert_eq!(average_cost(0, 100, 0, 200), 100);
}

#[renox::test]
async fn a_purchase_order_is_sent_received_in_part_and_costs_are_averaged() {
    let w = world().await;
    let db = w.app.db();
    w.app.acting_as(&w.manager_north);
    w.app
        .post(
            "/staff/suppliers",
            &[
                ("name", "Helmets Ltd"),
                ("email", "orders@helmets.example"),
                ("lead_days", "5"),
            ],
        )
        .await
        .assert_status(303);
    let supplier = Supplier::query().first(db).await.unwrap().unwrap();
    w.app
        .post(
            "/staff/purchase-orders",
            &[
                ("supplier", &supplier.id.to_string()),
                ("lines[0][variant]", &w.helmet.id.to_string()),
                ("lines[0][quantity]", "10"),
                ("lines[0][unit_cost]", "30"),
                ("lines[1][variant]", &w.helmet.id.to_string()),
                ("lines[1][quantity]", ""),
            ],
        )
        .await
        .assert_status(303);
    let order = PurchaseOrder::query().first(db).await.unwrap().unwrap();
    assert_eq!(
        (order.status, order.total, order.store_id),
        (PurchaseStatus::Draft, 30_000, w.north.id)
    );
    let url = format!("/staff/purchase-orders/{}", order.id);
    w.app
        .post(&format!("{url}/send"), &[])
        .await
        .assert_status(303);
    w.app.run_jobs().await;
    let mail = w
        .app
        .sent_mail()
        .into_iter()
        .find(|m| m.to.contains(&"orders@helmets.example".to_owned()))
        .expect("the supplier got the order");
    assert!(mail.subject.contains(&format!("PO-{}", order.id)));
    assert!(mail.html.unwrap_or_default().contains("signature="));
    // The printable page: only through the signed link.
    w.app
        .get(&format!("/purchase-orders/{}/print", order.id))
        .await
        .assert_forbidden();
    let signed = bikeshop::app::stock::purchasing::print_link(
        w.app.state(),
        &PurchaseOrder::find(db, order.id).await.unwrap().unwrap(),
    )
    .unwrap();
    let path = &signed[signed.find("/purchase-orders").unwrap()..];
    w.app.logout();
    w.app.get(path).await.assert_ok().assert_see("Helmets Ltd");

    // Four arrive, then six: the cost follows, North owns them.
    w.app.acting_as(&w.manager_north);
    w.app
        .get(&url)
        .await
        .assert_ok()
        .assert_see("Arrived")
        .assert_see("Partly received");
    let line_id: i64 =
        renox::db::sql("SELECT id FROM purchase_order_lines WHERE purchase_order_id = ?")
            .bind(order.id)
            .scalar(db)
            .await
            .unwrap();
    w.app
        .post(
            &format!("{url}/receive"),
            &[
                ("lines[0][line]", &line_id.to_string()),
                ("lines[0][received]", "4"),
            ],
        )
        .await
        .assert_status(303);
    assert_eq!(
        PurchaseOrder::find(db, order.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        PurchaseStatus::Partial
    );
    let cost = ProductVariant::find(db, w.helmet.id)
        .await
        .unwrap()
        .unwrap()
        .cost;
    assert_eq!(cost, average_cost(10, 2_250, 4, 3_000));
    w.app
        .post(
            &format!("{url}/receive"),
            &[
                ("lines[0][line]", &line_id.to_string()),
                ("lines[0][received]", "9"),
            ],
        )
        .await
        .assert_status(303);
    let received = PurchaseOrder::find(db, order.id).await.unwrap().unwrap();
    assert_eq!(received.status, PurchaseStatus::Received);
    assert!(received.received_at.is_some());
    // Nine were typed, six were still open: six taken.
    assert_eq!(
        level(db, w.helmet.id, w.north.id, w.north.id).await.on_hand,
        20
    );
    assert_eq!(
        ProductVariant::find(db, w.helmet.id)
            .await
            .unwrap()
            .unwrap()
            .cost,
        average_cost(14, cost, 6, 3_000)
    );
    assert_eq!(ledger::mismatches(db).await.unwrap(), vec![]);
}

#[renox::test]
async fn a_price_list_saves_good_rows_reports_bad_ones_and_queues_large_files() {
    let w = world().await;
    let db = w.app.db();
    let supplier = Supplier::create(
        db,
        Supplier {
            name: "Parts & Co".into(),
            lead_days: 3,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let product = Product::find(db, w.helmet.product_id)
        .await
        .unwrap()
        .unwrap();
    let csv = format!(
        "sku,product,size,colour,cost,price,barcode\n\
         {},,,,26.00,,8712345678901\n\
         NEW-1,{},M,red,10.00,18.00,\n\
         NEW-2,no-such-product,,,5,9,\n\
         BAD,,,,,,\n",
        w.helmet.sku, product.slug
    );
    w.app.acting_as(&w.manager_north);
    let url = format!("/staff/suppliers/{}/import", supplier.id);
    w.app
        .post_multipart(&url, &[], &[("file", "prices.csv", csv.as_bytes())])
        .await
        .assert_ok()
        .assert_see("2 imported, 2 rows left out")
        .assert_see("no-such-product");
    let helmet = ProductVariant::find(db, w.helmet.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((helmet.cost, helmet.price), (2_600, 4_500));
    let barcode: Option<String> =
        renox::db::sql("SELECT barcode FROM product_variants WHERE id = ?")
            .bind(helmet.id)
            .scalar(db)
            .await
            .unwrap();
    assert_eq!(barcode.as_deref(), Some("8712345678901"));
    let new = ProductVariant::query()
        .where_(ProductVariant::SKU.eq("NEW-1"))
        .first(db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (new.product_id, new.price, new.cost),
        (product.id, 1_800, 1_000)
    );
    // The refused row left nothing behind (its savepoint rolled back).
    assert!(
        ProductVariant::query()
            .where_(ProductVariant::SKU.eq("NEW-2"))
            .first(db)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        SupplierItem::query()
            .where_(SupplierItem::SUPPLIER_ID.eq(supplier.id))
            .count(db)
            .await
            .unwrap(),
        2
    );

    // A large file: queued, imported by the job, the report mailed.
    let mut large = String::from("sku,cost\n");
    for _ in 0..250 {
        large.push_str(&format!("{},27\n", w.helmet.sku));
    }
    let res = w
        .app
        .htmx()
        .post_multipart(&url, &[], &[("file", "big.csv", large.as_bytes())])
        .await;
    res.assert_ok();
    assert!(res.header("hx-refresh").is_some());
    assert_eq!(
        ProductVariant::find(db, w.helmet.id)
            .await
            .unwrap()
            .unwrap()
            .cost,
        2_600
    );
    w.app.run_jobs().await;
    assert_eq!(
        ProductVariant::find(db, w.helmet.id)
            .await
            .unwrap()
            .unwrap()
            .cost,
        2_700
    );
    assert!(
        w.app.sent_mail().iter().any(
            |m| m.to.contains(&"mn@example.com".to_owned()) && m.subject.contains("Parts & Co")
        )
    );
    w.app
        .get("/staff/suppliers/template.csv")
        .await
        .assert_ok()
        .assert_see("sku,product,size,colour,cost,price,barcode");
}

#[renox::test]
async fn the_daily_reorder_check_suggests_an_order_and_a_consignment() {
    let w = world().await;
    let db = w.app.db();
    let mut helmet = w.helmet.clone();
    helmet.reorder_level = 5;
    helmet.save(db).await.unwrap();
    put(db, helmet.id, w.south.id, w.south.id, 2).await;
    let supplier = Supplier::create(
        db,
        Supplier {
            name: "Helmets Ltd".into(),
            lead_days: 4,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    SupplierItem::create(
        db,
        SupplierItem {
            supplier_id: supplier.id,
            variant_id: helmet.id,
            cost: 2_800,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // The next morning.
    w.app.travel(Duration::from_secs(24 * 60 * 60));
    let reports = w
        .app
        .at_travelled_time(reorder::run(w.app.state()))
        .await
        .unwrap();
    let south = reports.iter().find(|r| r.store_id == w.south.id).unwrap();
    assert_eq!(south.low.len(), 1);
    assert_eq!(south.low[0].order, 8);
    assert_eq!(south.low[0].spare, vec![(w.north.id, 5)]);
    for store in [w.north.id, w.west.id] {
        assert!(
            reports
                .iter()
                .find(|r| r.store_id == store)
                .unwrap()
                .low
                .is_empty()
        );
    }
    let drafts = PurchaseOrder::query()
        .where_(PurchaseOrder::STORE_ID.eq(w.south.id))
        .where_(PurchaseOrder::SUGGESTED.eq(true))
        .get(db)
        .await
        .unwrap();
    assert_eq!(drafts.len(), 1);
    assert_eq!(drafts[0].total, 8 * 2_800);
    // South's manager hears of it, North's doesn't.
    let mails = w.app.sent_mail();
    let alert = mails
        .iter()
        .find(|m| m.to.contains(&"ms@example.com".to_owned()))
        .expect("South's manager got the alert");
    let html = alert.html.clone().unwrap_or_default();
    assert!(html.contains("Helmet"), "{html}");
    assert!(html.contains("North can spare 5"), "{html}");
    assert!(
        !mails
            .iter()
            .any(|m| m.to.contains(&"mn@example.com".to_owned()))
    );
    // Run again: today's suggestion replaces yesterday's, never two.
    w.app
        .at_travelled_time(reorder::run(w.app.state()))
        .await
        .unwrap();
    assert_eq!(
        PurchaseOrder::query()
            .where_(PurchaseOrder::STORE_ID.eq(w.south.id))
            .where_(PurchaseOrder::SUGGESTED.eq(true))
            .count(db)
            .await
            .unwrap(),
        1
    );
    // The scheduled task exists under its name.
    w.app.kernel().run_scheduled("stock:reorder").await.unwrap();
}

#[renox::test]
async fn a_stock_take_adjusts_and_charges_missing_consigned_goods_to_the_holder() {
    let w = world().await;
    let db = w.app.db();
    let held = put(db, w.helmet.id, w.north.id, w.south.id, 5).await;
    let own = put(db, w.helmet.id, w.south.id, w.south.id, 2).await;
    // A cashier may not count.
    w.app.acting_as(&w.cashier_south);
    w.app.get("/staff/stock/take").await.assert_forbidden();
    w.app.acting_as(&w.manager_south);
    w.app
        .get("/staff/stock/take")
        .await
        .assert_ok()
        .assert_see("Held for other stores (1)")
        .assert_see("owned by North");
    w.app
        .post(
            "/staff/stock/take",
            &[
                ("reason", "loss"),
                ("lines[0][level]", &own.id.to_string()),
                ("lines[0][counted]", "2"),
                ("lines[1][level]", &held.id.to_string()),
                ("lines[1][counted]", "3"),
            ],
        )
        .await
        .assert_status(303);
    assert_eq!(
        level(db, w.helmet.id, w.north.id, w.south.id).await.on_hand,
        3
    );
    let entries = IntercompanyEntry::query().get(db).await.unwrap();
    assert_eq!(
        owed(&entries, w.south.id, w.north.id, EntryKind::ConsignmentLoss),
        2 * w.helmet.cost
    );
    // North's manager is told.
    assert!(
        w.app
            .sent_mail()
            .iter()
            .any(|m| m.to.contains(&"mn@example.com".to_owned()) && m.subject.contains("missing"))
    );
    let roles: String = renox::db::sql("SELECT data FROM audit_logs WHERE action = 'stock.take'")
        .scalar(db)
        .await
        .unwrap();
    assert!(
        roles.contains(&format!("\"store_id\":{}", w.south.id)),
        "{roles}"
    );
    assert!(roles.contains(MANAGER), "{roles}");
    // A line of another store's shelf is refused whole.
    let elsewhere = level(db, w.helmet.id, w.north.id, w.north.id).await;
    w.app
        .post(
            "/staff/stock/take",
            &[
                ("reason", "count"),
                ("lines[0][level]", &elsewhere.id.to_string()),
                ("lines[0][counted]", "0"),
            ],
        )
        .await
        .assert_not_found();
    assert_eq!(ledger::mismatches(db).await.unwrap(), vec![]);
}

#[renox::test]
async fn bikes_move_between_sale_stock_and_the_fleet() {
    let w = world().await;
    let db = w.app.db();
    let bike = variant(db, "Trail 5", CategoryKind::Bike, 120_000).await;
    let shelf = put(db, bike.id, w.north.id, w.north.id, 2).await;
    w.app.acting_as(&w.manager_north);
    w.app
        .get("/staff/stock/fleet")
        .await
        .assert_ok()
        .assert_see("Trail 5");
    w.app
        .post(
            "/staff/stock/fleet",
            &[
                ("level", &shelf.id.to_string()),
                ("frame_number", "FR-NEW-1"),
                ("hourly_rate", "15"),
                ("daily_rate", "60"),
                ("deposit", "300"),
            ],
        )
        .await
        .assert_redirect("/staff/stock/fleet");
    let rental_bike = RentalBike::query()
        .where_(RentalBike::FRAME_NUMBER.eq("FR-NEW-1"))
        .first(db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            rental_bike.owner_store_id,
            rental_bike.status,
            rental_bike.asset_value
        ),
        (w.north.id, BikeStatus::Available, bike.cost)
    );
    // Rates typed in dollars, kept in cents.
    assert_eq!(
        (
            rental_bike.hourly_rate,
            rental_bike.daily_rate,
            rental_bike.deposit
        ),
        (1_500, 6_000, 30_000)
    );
    assert_eq!(level(db, bike.id, w.north.id, w.north.id).await.on_hand, 1);
    // The same frame number twice is refused.
    w.app
        .htmx()
        .post(
            "/staff/stock/fleet",
            &[
                ("level", &shelf.id.to_string()),
                ("frame_number", "FR-NEW-1"),
                ("hourly_rate", "1"),
                ("daily_rate", "1"),
                ("deposit", "1"),
            ],
        )
        .await
        .assert_invalid("frame_number");
    // South may not retire North's bike, even standing at South.
    let mut placed = rental_bike.clone();
    placed.location_store_id = w.south.id;
    placed.save(db).await.unwrap();
    w.app.acting_as(&w.manager_south);
    w.app
        .post(&format!("/staff/stock/fleet/{}/retire", placed.id), &[])
        .await
        .assert_forbidden();
    // North retires it: sold as used, from where it stands.
    w.app.acting_as(&w.manager_north);
    w.app
        .post(&format!("/staff/stock/fleet/{}/retire", placed.id), &[])
        .await
        .assert_redirect("/staff/stock/fleet");
    assert_eq!(
        RentalBike::find(db, placed.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        BikeStatus::Retired
    );
    let used = ProductVariant::query()
        .where_(ProductVariant::SKU.eq(format!("{}-USED-{}", bike.sku, placed.id)))
        .first(db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(level(db, used.id, w.north.id, w.south.id).await.on_hand, 1);
    assert_eq!(ledger::mismatches(db).await.unwrap(), vec![]);
}

#[renox::test]
async fn every_stock_page_answers_and_lists_run_a_fixed_number_of_queries() {
    let w = world().await;
    let db = w.app.db();
    let supplier = Supplier::create(
        db,
        Supplier {
            name: "Helmets Ltd".into(),
            lead_days: 4,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let held = put(db, w.helmet.id, w.north.id, w.south.id, 3).await;
    w.app.acting_as(&w.manager_north);
    for page in [
        "/staff/stock".to_owned(),
        "/staff/stock?view=away".to_owned(),
        "/staff/stock?view=low".to_owned(),
        "/staff/stock?export=csv".to_owned(),
        format!("/staff/stock/{}", held.id),
        "/staff/stock/take".to_owned(),
        "/staff/stock/fleet".to_owned(),
        "/staff/consignments".to_owned(),
        "/staff/consignments/new".to_owned(),
        "/staff/consignments/new?direction=ask".to_owned(),
        "/staff/suppliers".to_owned(),
        "/staff/suppliers/new".to_owned(),
        format!("/staff/suppliers/{}", supplier.id),
        format!("/staff/suppliers/{}/edit", supplier.id),
        "/staff/purchase-orders".to_owned(),
        "/staff/purchase-orders/new".to_owned(),
        format!("/staff/purchase-orders/new?supplier={}", supplier.id),
    ] {
        w.app.get(&page).await.assert_ok();
    }
    w.app
        .get("/staff/stock?export=csv")
        .await
        .assert_see("Helmet");

    bikeshop::app::stock::purchasing::draft(
        db,
        w.north.id,
        supplier.id,
        None,
        None,
        &[(w.helmet.id, 1, 100)],
        false,
    )
    .await
    .unwrap();
    let (_, few) = capture_queries(w.app.get("/staff/stock")).await;
    let (_, few_shipments) = capture_queries(w.app.get("/staff/consignments?view=all")).await;
    let (_, few_orders) = capture_queries(w.app.get("/staff/purchase-orders?status=all")).await;
    for n in 0..8 {
        let v = variant(db, &format!("Lamp {n}"), CategoryKind::Gear, 1_000).await;
        put(db, v.id, w.north.id, w.north.id, 3).await;
        ConsignmentShipment::create(
            db,
            ConsignmentShipment {
                owner_store_id: w.north.id,
                location_store_id: w.south.id,
                status: ShipmentStatus::Requested,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        bikeshop::app::stock::purchasing::draft(
            db,
            w.north.id,
            supplier.id,
            None,
            None,
            &[(v.id, 1, 100)],
            false,
        )
        .await
        .unwrap();
    }
    let (_, many) = capture_queries(w.app.get("/staff/stock")).await;
    let (_, many_shipments) = capture_queries(w.app.get("/staff/consignments?view=all")).await;
    let (_, many_orders) = capture_queries(w.app.get("/staff/purchase-orders?status=all")).await;
    assert_eq!(few.len(), many.len(), "/staff/stock");
    assert_eq!(
        few_shipments.len(),
        many_shipments.len(),
        "/staff/consignments"
    );
    assert_eq!(
        few_orders.len(),
        many_orders.len(),
        "/staff/purchase-orders"
    );
}
