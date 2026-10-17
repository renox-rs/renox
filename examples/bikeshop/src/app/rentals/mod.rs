//! Bike rentals by the hour or the day, picked up and returned at a store
//! (#235): Pagila's `rental` in today's clothes.
//!
//! | Who | Pages | File |
//! |---|---|---|
//! | Customers | `/rent` (find a bike), `/rentals` (theirs), `/rentals/{code}` (a reservation), `/rentals/identity` (the ID check) | [`reserve`], [`identity`] |
//! | Cashiers | `/staff/rentals` (the counter), `/staff/rentals/{id}` (pick-up and return), the walk-in form, the receipt, `/staff/identities` | [`counter`], [`identity`] |
//! | Managers | `/staff/fleet` (the fleet board), `/staff/fleet/{bike}` | [`fleet`] |
//!
//! The rules (prices, late fees, the deposit, the time limits) are plain
//! functions in [`pricing`]; availability and the booking transaction are
//! in [`booking`]; the scheduled tasks (no-shows, reminders, overdue
//! rentals, bikes due for service) in [`tasks`]; the messages in [`notify`].
//!
//! **Three stores meet here** (#245): a bike is rented out where it stands
//! (its **location**), the rental's **operating store** is the store that
//! served the customer (it holds the deposit), and the bike's **owner
//! store** is copied onto the rental for the books. When the two differ,
//! the intercompany books (#245) book the revenue and fees to the owner and
//! the operating store's fee to the operating store: they listen to
//! [`RentalClosed`], emitted when a rental is settled at the return.
//!
//! Made with `rnx make:module rentals`, then the files by hand.

pub mod booking;
pub mod counter;
pub mod explain;
pub mod factories;
pub mod fleet;
pub mod identity;
pub mod machine;
pub mod model;
pub mod notify;
pub mod pricing;
pub mod reserve;
pub mod tasks;

use renox::prelude::*;

use crate::app::access::{self, catalogue};
use crate::app::accounts::model::Customer;
use crate::app::sales::payments::{Payable, PaymentFailed, PaymentSucceeded};
use crate::app::staff::model::Staff;

/// A rental was returned and settled: price, late fee, damage fee and the
/// deposit are final. The intercompany books (#245) listen to it to book
/// the revenue to the owner store and the fee to the operating store.
#[derive(Debug, Clone)]
pub struct RentalClosed {
    pub rental_id: i64,
}

impl Event for RentalClosed {}

/// A rental bike needs the workshop: damaged at a return, or due for its
/// service by ridden hours. The workshop area listens and opens a work
/// order at the bike's location, billed to its owner store.
#[derive(Debug, Clone)]
pub struct FleetRepairNeeded {
    pub bike_id: i64,
    /// The rental it came back damaged from.
    pub rental_id: Option<i64>,
    /// What to do: the damage note, or "Periodic service".
    pub note: String,
}

impl Event for FleetRepairNeeded {}

/// The rentals area, registered in `src/lib.rs`.
pub struct Rentals;

impl Module for Rentals {
    fn name(&self) -> &'static str {
        "rentals"
    }

    fn routes(&self) -> Routes {
        let public = Routes::new()
            .get("/rent", reserve::search)
            .name("rentals.create");
        let customers = Routes::new()
            .post("/rent", reserve::reserve)
            .name("rentals.reserve")
            .get("/rentals", reserve::mine)
            .name("rentals.mine")
            .get("/rentals/identity", identity::edit)
            .name("rentals.identity")
            .post("/rentals/identity", identity::store)
            .name("rentals.identity.store")
            .get("/rentals/{code}", reserve::show)
            .name("rentals.show")
            .post("/rentals/{code}/cancel", reserve::cancel)
            .name("rentals.cancel")
            .post("/rentals/{code}/pay", reserve::pay)
            .name("rentals.pay")
            .require_auth();

        let counter = Routes::new()
            .get("/staff/rentals", counter::index)
            .name("rentals.counter")
            .get("/staff/rentals/customers", counter::customer_options)
            .name("rentals.customers")
            .get("/staff/rentals/{rental}", counter::desk)
            .name("rentals.desk")
            .get("/staff/rentals/{rental}/receipt", counter::receipt)
            .name("rentals.receipt")
            .get("/staff/rentals/{rental}/photos/{photo}", counter::photo)
            .name("rentals.photo")
            .post("/staff/rentals/{rental}/pickup", counter::pickup)
            .name("rentals.pickup")
            .post("/staff/rentals/{rental}/return", counter::give_back)
            .name("rentals.return")
            .require_permission(catalogue::RENTALS_VIEW);
        let walk_in = Routes::new()
            .get("/staff/rentals/walk-in", counter::walk_in)
            .name("rentals.walkin")
            .post("/staff/rentals/walk-in", counter::walk_in_store)
            .name("rentals.walkin.store")
            .require_permission(catalogue::RENTALS_CHECKOUT);
        let identities = Routes::new()
            .get("/staff/identities", identity::index)
            .name("rentals.identities")
            .get("/staff/identities/{document}/photo", identity::photo)
            .name("rentals.identities.photo")
            .post("/staff/identities/{document}/approve", identity::approve)
            .name("rentals.identities.approve")
            .post("/staff/identities/{document}/refuse", identity::refuse)
            .name("rentals.identities.refuse")
            .require_permission(catalogue::RENTALS_VERIFY_ID);
        let fleet = Routes::new()
            .get("/staff/fleet", fleet::index)
            .name("rentals.fleet")
            .get("/staff/fleet/{bike}", fleet::show)
            .name("rentals.fleet.show")
            .require_permission(catalogue::FLEET_VIEW);

        public.merge(customers).merge(access::staff_routes(
            counter.merge(walk_in).merge(identities).merge(fleet),
        ))
    }

    fn register(&self, app: &mut Registry) {
        // `rnx db:check` and the schema test compare these models with the tables.
        app.model::<crate::app::rentals::model::RentalBike>()
            .model::<crate::app::rentals::model::BikePlacement>()
            .model::<crate::app::rentals::model::IdentityDocument>()
            .model::<crate::app::rentals::model::RentalPhoto>();
        // The deposit paid online (or not): the shared payments contract.
        app.listen(|event: PaymentSucceeded, state: AppState| async move {
            if let Payable::Rental(id) = event.payable {
                reserve::deposit_paid(&state, event.payment_id, id).await?;
            }
            Ok(())
        });
        app.listen(|event: PaymentFailed, state: AppState| async move {
            if let Payable::Rental(id) = event.payable {
                reserve::deposit_failed(&state, id).await?;
            }
            Ok(())
        });
        tasks::schedule(app.schedule());
    }
}

/// The customer record of a logged-in user, made from their account the
/// first time they rent or book (customers belong to the company, not to
/// a store; #238 owns the account pages).
pub async fn customer_of(db: &Db, user: &User) -> Result<Customer> {
    if let Some(customer) = Customer::of_user(db, user.id).await? {
        return Ok(customer);
    }
    Customer::create(
        db,
        Customer {
            user_id: Some(user.id),
            name: user.name.clone(),
            email: Some(user.email.clone()),
            active: true,
            ..Default::default()
        },
    )
    .await
}

/// The `staff` row of a user working at the counter (`None` for the owner
/// when they have no staff row).
pub async fn staff_id(db: &Db, user: &User) -> Result<Option<i64>> {
    Ok(Staff::of_user(db, user.id).await?.map(|s| s.id))
}

/// The store this staff request works in (the access area's active store),
/// or a 403 when none was picked.
pub fn active_store() -> Result<i64> {
    access::active_store::current().ok_or(Error::Forbidden)
}

/// An absolute link to the route `name` (with one parameter or none), for
/// mails and notifications. A plain function, so the `&dyn Display` list
/// `AppState::absolute_url` takes never lives across an `.await` in the
/// callers (that would make their futures non-`Send`, docs/contributing/conventions.md §4.2).
pub fn link(state: &AppState, name: &str, param: Option<impl std::fmt::Display>) -> Result<String> {
    match param {
        Some(param) => state.absolute_url(name, &[&param]),
        None => state.absolute_url(name, &[]),
    }
}
