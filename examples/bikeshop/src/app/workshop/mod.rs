//! The workshop (#236): customers' bikes, service bookings with the
//! workshop's daily capacity, and work orders from the board to the
//! counter.
//!
//! | Who | Pages | File |
//! |---|---|---|
//! | Customers | `/bikes` (their bikes), `/bikes/{bike}` (a bike's service history), `/service/book` (book a service), `/service/{order}` (follow it, reschedule, cancel, pay) | [`bikes`], [`booking`] |
//! | Anyone with the link | `/service/approve/{extra}` (approve or refuse extra work, signed) | [`approval`] |
//! | Mechanics, managers | `/staff/workshop` (the board), `/staff/workshop/{order}` (the bench), `/staff/workshop/new` (a walk-in) | [`board`], [`order`] |
//!
//! The rules: the day's capacity in [`capacity`] (checked in the form's
//! hook and again in the booking transaction), the statuses and what
//! changing them does in [`status`], fleet repairs asked for by the rentals
//! area in [`fleet`], tomorrow's reminders in [`tasks`].
//!
//! **Stores:** a work order is done by one store's workshop (`store_id`,
//! its operating store); a fleet repair of another store's bike is billed
//! to that owner store (`billed_store_id`). The intercompany books (#245)
//! listen to [`status::WorkOrderClosed`] for those.
//!
//! Made with `rnx make:module workshop`, then the files by hand.

pub mod approval;
pub mod bikes;
pub mod board;
pub mod booking;
pub mod capacity;
pub mod explain;
pub mod factories;
pub mod fleet;
pub mod model;
pub mod order;
pub mod status;
pub mod tasks;

use renox::prelude::*;

use crate::app::access::{self, catalogue};
use crate::app::rentals::FleetRepairNeeded;
use crate::app::sales::payments::{Payable, PaymentSucceeded};

/// The workshop area, registered in `src/lib.rs`.
pub struct Workshop;

impl Module for Workshop {
    fn name(&self) -> &'static str {
        "workshop"
    }

    fn routes(&self) -> Routes {
        let customers = Routes::new()
            .get("/bikes", bikes::index)
            .name("workshop.bikes")
            .post("/bikes", bikes::store)
            .name("workshop.bikes.store")
            .get("/bikes/{bike}", bikes::show)
            .name("workshop.bikes.show")
            .get("/bikes/{bike}/photo", bikes::photo)
            .name("workshop.bikes.photo")
            .get("/service/book", booking::form)
            .name("workshop.book")
            .post("/service/book", booking::store)
            .name("workshop.book.store")
            .get("/service/{order}", booking::show)
            .name("workshop.service.show")
            .post("/service/{order}/cancel", booking::cancel)
            .name("workshop.service.cancel")
            .post("/service/{order}/reschedule", booking::reschedule)
            .name("workshop.service.reschedule")
            .post("/service/{order}/pay", booking::pay)
            .name("workshop.service.pay")
            .require_auth();
        // No login: the signed link is the proof (renox::signed).
        let approval = Routes::new()
            .get("/service/approve/{extra}", approval::show)
            .name("workshop.extra.show")
            .post("/service/approve/{extra}", approval::decide)
            .name("workshop.extra.decide");

        let look = Routes::new()
            .get("/staff/workshop", board::index)
            .name("workshop.board")
            .get("/staff/workshop/customers", board::customer_options)
            .name("workshop.customers")
            .get("/staff/workshop/{order}", order::show)
            .name("workshop.order")
            .get("/staff/workshop/{order}/parts", order::part_options)
            .name("workshop.parts")
            .get("/staff/workshop/{order}/notes/{note}/photo", order::photo)
            .name("workshop.order.photo")
            // The counter payment checks `orders.sell` in the order's store itself.
            .post("/staff/workshop/{order}/pay", order::pay)
            .name("workshop.order.pay")
            // Pay by link: the same check, then renox-billing's payment link.
            .post("/staff/workshop/{order}/pay-link", order::pay_link)
            .name("workshop.order.pay.link")
            .require_permission(catalogue::WORKORDERS_VIEW);
        // Taking a walk-in opens a work order: the same permission as working on one.
        let work = Routes::new()
            .get("/staff/workshop/new", board::walk_in)
            .name("workshop.walkin")
            .post("/staff/workshop/new", board::walk_in_store)
            .name("workshop.walkin.store")
            .post("/staff/workshop/move", board::move_card)
            .name("workshop.move")
            .post("/staff/workshop/{order}/tasks", order::tasks)
            .name("workshop.order.tasks")
            .post("/staff/workshop/{order}/notes", order::note)
            .name("workshop.order.notes")
            .post("/staff/workshop/{order}/signoff", order::signoff)
            .name("workshop.order.signoff")
            .post("/staff/workshop/{order}/parts", order::add_part)
            .name("workshop.order.parts")
            .post(
                "/staff/workshop/{order}/parts/{part}/take",
                order::take_waiting,
            )
            .name("workshop.order.parts.take")
            .post("/staff/workshop/{order}/extra", order::propose)
            .name("workshop.order.extra")
            .post("/staff/workshop/{order}/assign", order::assign)
            .name("workshop.order.assign")
            .post("/staff/workshop/{order}/status", order::change_status)
            .name("workshop.order.status")
            .require_permission(catalogue::WORKORDERS_UPDATE);

        customers
            .merge(approval)
            .merge(access::staff_routes(look.merge(work)))
    }

    fn register(&self, app: &mut Registry) {
        // `rnx db:check` and the schema test compare these models with the tables.
        app.model::<crate::app::workshop::model::CustomerBike>()
            .model::<crate::app::workshop::model::ServiceTask>()
            .model::<crate::app::workshop::model::WorkOrder>()
            .model::<crate::app::workshop::model::WorkOrderTask>()
            .model::<crate::app::workshop::model::WorkOrderPart>()
            .model::<crate::app::workshop::model::WorkOrderNote>()
            .model::<crate::app::workshop::model::ExtraWork>();
        // A damaged or worn rental bike: a fleet work order, billed to its owner store.
        app.listen(|event: FleetRepairNeeded, state: AppState| async move {
            fleet::open_repair(&state, &event).await?;
            Ok(())
        });
        // Paid online or at the counter (the shared payments contract).
        app.listen(|event: PaymentSucceeded, state: AppState| async move {
            if let Payable::WorkOrder(id) = event.payable {
                booking::paid(&state, id).await?;
            }
            Ok(())
        });
        // Paid by a payment link: renox-billing's event carries the reference
        // `work_order` / id the link was made with (workshop/order.rs, `pay_link`).
        app.listen(
            |event: renox_billing::PaymentSucceeded, state: AppState| async move {
                if event.reference_type.as_deref() == Some("work_order")
                    && let Some(id) = event.reference_id.as_deref().and_then(|id| id.parse().ok())
                {
                    booking::paid(&state, id).await?;
                }
                Ok(())
            },
        );
        tasks::schedule(app.schedule());
    }
}
