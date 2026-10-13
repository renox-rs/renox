//! Multi-store operations (#245): staff helping other stores, bikes placed
//! at other stores, and the books between stores.
//!
//! | Who | Pages | File |
//! |---|---|---|
//! | Managers | `/staff/help…` (ask another store for someone, approve, end, hours) | [`help`] |
//! | Managers | `/staff/placements…` (place, ask for, move, call back, send back bikes) | [`placements`] |
//! | Managers, the owner | `/staff/books` (balances, entries), `/staff/books/settlements…` (monthly statements), `/staff/books/fees` (fee rates) | [`intercompany`], [`settlements`] |
//!
//! [`books`] is the one place that writes intercompany entries: the stock
//! area calls it for consigned sales and losses, and this module's
//! listeners call it when a rental closes (`rentals::RentalClosed`) and
//! when a fleet repair is collected (`workshop::status::WorkOrderClosed`).
//! [`audit`] records who did what, in which store, with which role there.
//!
//! Goods sent between stores on consignment live in the stock area
//! (`src/app/stock/consignment.rs`).
//!
//! Made with `rnx make:module multistore`, then the files by hand.

pub mod audit;
pub mod books;
pub mod explain;
pub mod factories;
pub mod help;
pub mod intercompany;
pub mod model;
pub mod placements;
pub mod settlements;

use renox::prelude::*;

use crate::app::access::{self, catalogue};
use crate::app::rentals::RentalClosed;
use crate::app::workshop::status::WorkOrderClosed;

/// The multistore area, registered in `src/lib.rs`.
pub struct Multistore;

impl Module for Multistore {
    fn name(&self) -> &'static str {
        "multistore"
    }

    fn routes(&self) -> Routes {
        let help = Routes::new()
            .get("/staff/help", help::index)
            .name("multistore.help")
            .get("/staff/help/new", help::create)
            .name("multistore.help.create")
            .get("/staff/help/hours", help::hours)
            .name("multistore.help.hours")
            .post("/staff/help", help::store)
            .name("multistore.help.store")
            .post("/staff/help/{request}/approve", help::approve)
            .name("multistore.help.approve")
            .post("/staff/help/{request}/refuse", help::refuse)
            .name("multistore.help.refuse")
            .post("/staff/help/{request}/withdraw", help::withdraw)
            .name("multistore.help.withdraw")
            .post("/staff/help/{request}/end", help::end)
            .name("multistore.help.end")
            .post("/staff/help/{request}/hours", help::log_hours)
            .name("multistore.help.log")
            .require_permission(catalogue::STAFF_HELP);
        let placements = Routes::new()
            .get("/staff/placements", placements::index)
            .name("multistore.placements")
            .post("/staff/placements/{placement}/decide", placements::decide)
            .name("multistore.placements.decide")
            .post("/staff/placements/{placement}/move", placements::move_bike)
            .name("multistore.placements.move")
            .post("/staff/placements/{placement}/recall", placements::recall)
            .name("multistore.placements.recall")
            .post("/staff/placements/send-back/{bike}", placements::send_back)
            .name("multistore.placements.send_back")
            .require_permission(catalogue::FLEET_VIEW);
        let placing = Routes::new()
            .get("/staff/placements/new", placements::create)
            .name("multistore.placements.create")
            .post("/staff/placements", placements::store)
            .name("multistore.placements.store")
            .require_permission(catalogue::FLEET_PLACE);
        let books = Routes::new()
            .get("/staff/books", intercompany::index)
            .name("multistore.books")
            .get("/staff/books/fees", intercompany::fees)
            .name("multistore.fees")
            .post("/staff/books/fees/{store}", intercompany::update_fee)
            .name("multistore.fees.update")
            .get("/staff/books/settlements", settlements::index)
            .name("multistore.settlements")
            .get("/staff/books/settlements/{settlement}", settlements::show)
            .name("multistore.settlements.show")
            .post(
                "/staff/books/settlements/{settlement}/confirm",
                settlements::confirm,
            )
            .name("multistore.settlements.confirm")
            .require_permission(catalogue::INTERCOMPANY_VIEW);
        access::staff_routes(help.merge(placements).merge(placing).merge(books))
    }

    fn register(&self, app: &mut Registry) {
        // `rnx db:check` and the schema test compare these models with the tables.
        app.model::<crate::app::multistore::model::IntercompanyEntry>()
            .model::<crate::app::multistore::model::Settlement>();
        // The books follow the business: a closed rental and a collected
        // fleet repair are booked between the stores involved.
        app.listen(|event: RentalClosed, state: AppState| async move {
            books::rental(&state.db, event.rental_id).await?;
            Ok(())
        });
        app.listen(|event: WorkOrderClosed, state: AppState| async move {
            books::repair(&state.db, event.work_order_id).await?;
            Ok(())
        });
        app.job::<settlements::SendStatement>();
        settlements::schedule(app.schedule());
    }
}
