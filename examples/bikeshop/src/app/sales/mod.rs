//! Cart, checkout, payments and counter sales (#234).
//!
//! | File | What |
//! |---|---|
//! | [`cart`] | the cart (session for guests, `carts` for customers), its page and the navbar's count |
//! | [`checkout`] | the wizard and placing the order: stock reserved in a transaction |
//! | [`payments`] | the payments contract every area uses (`start`, `record_counter`, `mark_paid`…) |
//! | [`gateway`] | Midtrans' hosted page and signed webhook (or the demo gateway), the `/pay/{payment}` page |
//! | [`orders`] | an order's life (paid, cancelled, expired), the customer's order page and invoice |
//! | [`ledger`] | what each step does to `stock_movements` and the books between stores |
//! | [`notify`] | the mails and in-app notifications |
//! | [`staff`] | the staff's order list and order page: ready, handed over, cancelled, returned |
//! | [`counter`] | the point of sale |
//! | [`mails`] | `/sales/mails`: every mail, previewed |
//!
//! Made with `rnx make:module sales`, `rnx make:migration create_carts_table`
//! and `rnx make:mail` for the mails; then the files by hand. The models
//! came with #232.

pub mod cart;
pub mod checkout;
pub mod counter;
pub mod explain;
pub mod factories;
pub mod gateway;
pub mod ledger;
pub mod mails;
pub mod model;
pub mod notify;
pub mod orders;
pub mod payments;
pub mod staff;

use renox::auth::events::EmailVerified;
use renox::prelude::*;

use crate::app::access::{self, catalogue};
use crate::app::accounts::model::Customer;

/// The sales area, registered in `src/lib.rs`.
pub struct Sales;

impl renox::Module for Sales {
    fn name(&self) -> &'static str {
        "sales"
    }

    fn routes(&self) -> Routes {
        Routes::new()
            .get("/cart", cart::show)
            .name("cart.show")
            .get("/cart/mini", cart::mini)
            .name("cart.mini")
            .post("/cart", cart::add)
            .name("cart.add")
            .patch("/cart/{variant}", cart::update)
            .name("cart.update")
            .delete("/cart/{variant}", cart::remove)
            .name("cart.remove")
            .post("/cart/store", cart::store)
            .name("cart.store")
            .get("/checkout", checkout::show)
            .name("checkout.show")
            .post("/checkout", checkout::place)
            .name("checkout.place")
            .get("/pay/{payment}", gateway::show)
            .name("pay.show")
            .get("/pay/demo/{payment}", gateway::demo)
            .name("pay.demo")
            .post("/pay/demo/{payment}", gateway::demo_complete)
            .name("pay.demo.complete")
            .get("/orders/{order}", orders::show)
            .name("orders.show")
            .get("/orders/{order}/invoice", orders::invoice)
            .name("orders.invoice")
            .get("/orders/{order}/view", orders::signed)
            .name("orders.signed")
            .get("/sales/mails", mails::index)
            .name("sales.mails")
            // Signature checked, stored once per event, handled by the queue.
            .webhook::<gateway::Midtrans>("/webhooks/midtrans")
            .merge(staff_routes())
    }

    fn register(&self, app: &mut Registry) {
        // `rnx db:check` and the schema test compare these models with the tables.
        app.model::<crate::app::sales::model::Order>()
            .model::<crate::app::sales::model::OrderItem>()
            .model::<crate::app::sales::model::Payment>()
            .model::<crate::app::sales::model::SavedCart>();
        app.webhook::<gateway::Midtrans>()
            .job::<gateway::DemoNotify>()
            // The payments contract's events, for orders.
            .listen(orders::on_payment)
            .listen(orders::on_payment_failed)
            // A guest who registers with the address they ordered with finds
            // those orders in their account, once the address is proven theirs
            // (the verification link): linking at registration would hand
            // anyone's orders to whoever signs up with their address. The
            // accounts area's claim merges the records (#238).
            .listen(|event: EmailVerified, state: AppState| async move {
                let Some(user) = User::find(&state.db, event.user_id).await? else {
                    return Ok(());
                };
                let guests = Customer::query()
                    .where_(Customer::EMAIL.eq(user.email.clone()))
                    .where_(Customer::USER_ID.is_null())
                    .get(&state.db)
                    .await?;
                for guest in guests {
                    crate::app::accounts::claim::link(&state.db, &user, guest.id).await?;
                }
                Ok(())
            });
        app.schedule()
            .every_minute("sales:expire-orders", |state| async move {
                orders::expire(state).await.map(|_| ())
            });
    }
}

/// The staff's routes: login, a store where they may work, and the
/// permission each needs there (`access::staff_routes`).
fn staff_routes() -> Routes {
    let orders = access::staff_routes(
        Routes::new()
            .get("/staff/orders", staff::index)
            .name("sales.orders.index")
            .get("/staff/orders/{order}", staff::show)
            .name("sales.orders.show")
            .post("/staff/orders/{order}/ready", staff::ready)
            .name("sales.orders.ready")
            .post("/staff/orders/{order}/complete", staff::complete)
            .name("sales.orders.complete")
            .post("/staff/orders/{order}/cancel", staff::cancel)
            .name("sales.orders.cancel")
            .post("/staff/orders/{order}/return", staff::take_back)
            .name("sales.orders.return")
            .require_permission(catalogue::ORDERS_VIEW),
    );
    let counter = access::staff_routes(
        Routes::new()
            .get("/staff/counter", counter::show)
            .name("sales.counter")
            .get("/staff/counter/variants", counter::variants)
            .name("sales.counter.variants")
            .get("/staff/counter/customers", counter::customers)
            .name("sales.counter.customers")
            .post("/staff/counter/lines", counter::add)
            .name("sales.counter.add")
            .patch("/staff/counter/lines/{variant}", counter::update)
            .name("sales.counter.update")
            .delete("/staff/counter/lines/{variant}", counter::remove)
            .name("sales.counter.remove")
            .post("/staff/counter/customer", counter::customer)
            .name("sales.counter.customer")
            .post("/staff/counter/clear", counter::clear)
            .name("sales.counter.clear")
            .post("/staff/counter/pay", counter::pay)
            .name("sales.counter.pay")
            .require_permission(catalogue::ORDERS_SELL),
    );
    orders.merge(counter)
}
