//! Stock, consignment between stores, suppliers and purchasing (#240).
//!
//! | Who | Pages | File |
//! |---|---|---|
//! | Store staff | `/staff/stock` (the stock grid), `/staff/stock/{level}` (one level's ledger) | [`levels`] |
//! | Managers | `/staff/stock/take` (count a shelf) | [`take`] |
//! | Managers | `/staff/consignments…` (goods sent to and asked from other stores) | [`consignment`] |
//! | Buyers | `/staff/suppliers…`, `/staff/purchase-orders…`, the supplier's printable order | [`purchasing`], [`import`] |
//! | Managers | `/staff/stock/fleet` (new bikes into the rental fleet, old ones back to sale stock) | [`fleet`] |
//!
//! **One ledger.** Every change of stock is a `stock_movements` row with
//! its owner and location store, and the matching `stock_levels` row
//! changes with it in the same transaction ([`model::StockMovement::record`],
//! [`ledger::take`]); [`ledger::mismatches`] proves the two agree. Sales
//! (`sales::ledger`) and the workshop's parts write through the same two
//! functions.
//!
//! The daily reorder check is in [`reorder`]; the messages in [`notify`].
//!
//! Made with `rnx make:module stock`, then the files by hand.

pub mod consignment;
pub mod explain;
pub mod factories;
pub mod fleet;
pub mod import;
pub mod ledger;
pub mod levels;
pub mod model;
pub mod notify;
pub mod purchasing;
pub mod reorder;
pub mod take;

use renox::prelude::*;

use crate::app::access::{self, catalogue};

/// The stock area, registered in `src/lib.rs`.
pub struct Stock;

impl Module for Stock {
    fn name(&self) -> &'static str {
        "stock"
    }

    fn routes(&self) -> Routes {
        // Seeing stock (each record is checked again against its stores).
        let view = Routes::new()
            .get("/staff/stock", levels::index)
            .name("stock.index")
            .get("/staff/stock/{level}", levels::show)
            .name("stock.ledger")
            .post("/staff/stock/{level}/write-off", levels::write_off)
            .name("stock.write_off")
            .get("/staff/consignments", consignment::index)
            .name("stock.consignments")
            .get("/staff/consignments/{shipment}", consignment::show)
            .name("stock.consignments.show")
            .post("/staff/consignments/{shipment}/decide", consignment::decide)
            .name("stock.consignments.decide")
            .post("/staff/consignments/{shipment}/ship", consignment::ship)
            .name("stock.consignments.ship")
            .post(
                "/staff/consignments/{shipment}/receive",
                consignment::receive,
            )
            .name("stock.consignments.receive")
            .post("/staff/consignments/{shipment}/recall", consignment::recall)
            .name("stock.consignments.recall")
            .post(
                "/staff/consignments/{shipment}/send-back",
                consignment::send_back,
            )
            .name("stock.consignments.send_back")
            .post(
                "/staff/consignments/{shipment}/receive-back",
                consignment::receive_back,
            )
            .name("stock.consignments.receive_back")
            .get("/staff/purchase-orders/{order}", purchasing::show)
            .name("stock.purchasing.show")
            .post("/staff/purchase-orders/{order}/send", purchasing::send)
            .name("stock.purchasing.send")
            .post("/staff/purchase-orders/{order}/cancel", purchasing::cancel)
            .name("stock.purchasing.cancel")
            .post(
                "/staff/purchase-orders/{order}/receive",
                purchasing::receive,
            )
            .name("stock.purchasing.receive")
            .require_permission(catalogue::STOCK_VIEW);
        let take = Routes::new()
            .get("/staff/stock/take", take::sheet)
            .name("stock.take")
            .post("/staff/stock/take", take::store)
            .name("stock.take.store")
            .require_permission(catalogue::STOCK_ADJUST);
        let consign = Routes::new()
            .get("/staff/consignments/new", consignment::create)
            .name("stock.consignments.create")
            .post("/staff/consignments", consignment::store)
            .name("stock.consignments.store")
            .require_permission(catalogue::CONSIGNMENT_MANAGE);
        let buying = Routes::new()
            .get("/staff/suppliers", purchasing::suppliers)
            .name("stock.suppliers")
            .get("/staff/suppliers/new", purchasing::supplier_create)
            .name("stock.suppliers.create")
            .post("/staff/suppliers", purchasing::supplier_store)
            .name("stock.suppliers.store")
            .get("/staff/suppliers/template.csv", import::template)
            .name("stock.suppliers.template")
            .get("/staff/suppliers/{supplier}", purchasing::supplier_show)
            .name("stock.suppliers.show")
            .get(
                "/staff/suppliers/{supplier}/edit",
                purchasing::supplier_edit,
            )
            .name("stock.suppliers.edit")
            .post("/staff/suppliers/{supplier}", purchasing::supplier_update)
            .name("stock.suppliers.update")
            .post("/staff/suppliers/{supplier}/import", import::import)
            .name("stock.suppliers.import")
            .get("/staff/purchase-orders", purchasing::index)
            .name("stock.purchasing")
            .get("/staff/purchase-orders/new", purchasing::create)
            .name("stock.purchasing.create")
            .post("/staff/purchase-orders", purchasing::store)
            .name("stock.purchasing.store")
            .require_permission(catalogue::PURCHASING_MANAGE);
        let fleet = Routes::new()
            .get("/staff/stock/fleet", fleet::index)
            .name("stock.fleet")
            .post("/staff/stock/fleet", fleet::to_fleet)
            .name("stock.fleet.store")
            .post("/staff/stock/fleet/{bike}/retire", fleet::retire)
            .name("stock.fleet.retire")
            .require_permission(catalogue::FLEET_MANAGE);
        // The supplier's printable order: a signed link, no login.
        let public = Routes::new()
            .get("/purchase-orders/{order}/print", purchasing::print)
            .name("stock.purchasing.print");
        public.merge(access::staff_routes(
            view.merge(take).merge(consign).merge(buying).merge(fleet),
        ))
    }

    fn register(&self, app: &mut Registry) {
        // `rnx db:check` and the schema test compare these models with the tables.
        app.model::<crate::app::stock::model::StockLevel>()
            .model::<crate::app::stock::model::StockMovement>()
            .model::<crate::app::stock::model::Supplier>()
            .model::<crate::app::stock::model::PurchaseOrder>()
            .model::<crate::app::stock::model::PurchaseOrderLine>()
            .model::<crate::app::stock::model::ConsignmentShipment>()
            .model::<crate::app::stock::model::ConsignmentShipmentLine>()
            .model::<crate::app::stock::model::SupplierItem>();
        app.job::<import::ImportPriceList>();
        // `view:check` compares the template's variables with the struct's fields.
        app.view_data::<purchasing::SupplierList>();
        reorder::schedule(app.schedule());
    }
}
