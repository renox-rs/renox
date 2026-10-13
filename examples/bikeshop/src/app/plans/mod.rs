//! Service plans (#237): a customer subscribes one of their bikes to a
//! plan (a weekly check, a monthly tune-up…), pays for it monthly through
//! renox-billing, and the visits are booked for them ahead, within the
//! workshop's capacity.
//!
//! | Who | Pages | File |
//! |---|---|---|
//! | Anyone | `/plans` (the plans compared) | [`subscribe`] |
//! | Customers | `/plans/subscribe` (choose a bike, plan, store, weekday, way of paying), `/plans/mine` (their plans, next visit, visits done, invoices), `/plans/mine/{subscription}` (one plan: visits on a calendar, skip or move one, change plan, pause, cancel, resume) | [`subscribe`], [`mine`] |
//! | Readers of the example | `/plans/mails` (every plan mail, previewed) | [`mails`] |
//! | The demo gateway | `/plans/demo-pay/…` (its "hosted page", without real keys) | [`demo`] |
//!
//! How it fits together:
//!
//! - [`billing`](mod@billing): what renox-billing sells (each plan by card and through
//!   Xendit) and its gateways; registered in `src/lib.rs`.
//! - [`sync`]: renox-billing's events mirrored into the shop's plans (a
//!   payment starts or renews a plan, a failed one puts it on hold, a
//!   cancellation ends it at the period's end).
//! - [`visits`]: the visits, made ahead as work orders through the
//!   workshop's own booking code (`workshop::capacity`), skipped, moved,
//!   missed, held and released.
//! - [`tasks`]: `plans:visits`, every morning.
//! - [`parts_discount_bp`]: the parts discount the subscriber's plan gives,
//!   read by the checkout and the counter (#234) and by work orders.
//!
//! Made with `rnx make:module plans`, then the files by hand; the
//! migration with `rnx make:migration add_billing_to_plans`.

pub mod billing;
pub mod demo;
pub mod explain;
pub mod factories;
pub mod mails;
pub mod mine;
pub mod model;
pub mod subscribe;
pub mod sync;
pub mod tasks;
pub mod visits;

use renox::auth::events::AccountDeleted;
use renox::prelude::*;

use crate::app::staff::model::fee;
use crate::app::workshop::model::WorkOrder;
use crate::app::workshop::status::WorkOrderClosed;
use model::{PlanSubscription, ServicePlan, SubscriptionStatus};

pub use billing::billing;

/// The plans area, registered in `src/lib.rs`.
pub struct Plans;

impl Module for Plans {
    fn name(&self) -> &'static str {
        "plans"
    }

    fn routes(&self) -> Routes {
        let public = Routes::new()
            .get("/plans", subscribe::index)
            .name("plans.index")
            .get("/plans/mails", mails::index)
            .name("plans.mails");
        // The demo gateway's page: the signed link is the proof.
        let demo = Routes::new()
            .get("/plans/demo-pay/{user}/{name}/{plan}", demo::page)
            .name("plans.demo")
            .post("/plans/demo-pay/{user}/{name}/{plan}", demo::complete)
            .name("plans.demo.complete");
        let customers = Routes::new()
            .get("/plans/subscribe", subscribe::form)
            .name("plans.subscribe")
            .post("/plans/subscribe", subscribe::store)
            .name("plans.subscribe.store")
            .get("/plans/mine", mine::index)
            .name("plans.mine")
            .get("/plans/mine/{subscription}", mine::show)
            .name("plans.show")
            .post("/plans/mine/{subscription}/swap", mine::swap)
            .name("plans.swap")
            .post("/plans/mine/{subscription}/pause", mine::pause)
            .name("plans.pause")
            .post("/plans/mine/{subscription}/unpause", mine::unpause)
            .name("plans.unpause")
            .post("/plans/mine/{subscription}/cancel", mine::cancel)
            .name("plans.cancel")
            .post("/plans/mine/{subscription}/resume", mine::resume)
            .name("plans.resume")
            .post("/plans/mine/{subscription}/demo-renew", demo::renew)
            .name("plans.demo.renew")
            .post("/plans/visits/{visit}/skip", mine::skip)
            .name("plans.visits.skip")
            .post("/plans/visits/{visit}/move", mine::move_visit)
            .name("plans.visits.move")
            .require_auth();
        public.merge(demo).merge(customers)
    }

    fn register(&self, app: &mut Registry) {
        // `rnx db:check` and the schema test compare these models with the tables.
        app.model::<crate::app::plans::model::ServicePlan>()
            .model::<crate::app::plans::model::PlanSubscription>()
            .model::<crate::app::plans::model::PlanVisit>()
            .model::<crate::app::plans::model::PlanInvoice>();
        app.job::<demo::DemoBillingNotify>();
        // renox-billing's events, mirrored into the bike's plan (src/app/plans/sync.rs).
        app.listen(
            |e: renox_billing::SubscriptionCreated, state: AppState| async move {
                sync::mirror(&state, &e.subscription).await
            },
        );
        app.listen(
            |e: renox_billing::SubscriptionUpdated, state: AppState| async move {
                sync::mirror(&state, &e.subscription).await
            },
        );
        app.listen(
            |e: renox_billing::PaymentSucceeded, state: AppState| async move {
                sync::payment(
                    &state,
                    sync::Paid {
                        gateway: &e.gateway,
                        payment_id: &e.payment_id,
                        amount: e.amount,
                        currency: &e.currency,
                        subscription: e.subscription.as_ref(),
                        succeeded: true,
                    },
                )
                .await
            },
        );
        app.listen(
            |e: renox_billing::PaymentFailed, state: AppState| async move {
                sync::payment(
                    &state,
                    sync::Paid {
                        gateway: &e.gateway,
                        payment_id: &e.payment_id,
                        amount: e.amount,
                        currency: &e.currency,
                        subscription: e.subscription.as_ref(),
                        succeeded: false,
                    },
                )
                .await
            },
        );
        // renox-billing cancels the subscriptions at the gateway; the shop
        // ends the plans and frees their workshop slots.
        app.listen(|e: AccountDeleted, state: AppState| async move {
            sync::account_deleted(&state.db, e.user_id).await
        });
        // A collected work order of a plan: the visit is done.
        app.listen(|e: WorkOrderClosed, state: AppState| async move {
            visits::done(&state.db, e.work_order_id).await
        });
        tasks::schedule(app.schedule());
    }
}

/// The parts discount of a customer's service plan, in basis points (1000
/// = 10 %): the best of their bikes' running plans (active or paused, not
/// on hold after a failed payment); 0 without one. The checkout and the
/// counter (#234) take it off spare parts.
pub async fn parts_discount_bp(db: &Db, customer_id: Option<i64>) -> Result<i64> {
    let Some(customer) = customer_id else {
        return Ok(0);
    };
    Ok(renox::db::sql(
        "SELECT CAST(COALESCE(MAX(p.parts_discount_bp), 0) AS BIGINT) FROM plan_subscriptions s \
         JOIN customer_bikes b ON b.id = s.customer_bike_id \
         JOIN service_plans p ON p.id = s.service_plan_id \
         WHERE b.customer_id = ? AND s.status IN (?, ?) AND s.held_at IS NULL",
    )
    .bind(customer)
    .bind(SubscriptionStatus::Active)
    .bind(SubscriptionStatus::Paused)
    .scalar::<i64>(db)
    .await?)
}

/// A part's price on a work order: the catalogue price, less the plan's
/// parts discount when the work order is a visit of a running plan.
pub async fn part_price(db: &Db, order: &WorkOrder, price: i64) -> Result<i64> {
    let Some(id) = order.plan_subscription_id else {
        return Ok(price);
    };
    let Some(sub) = PlanSubscription::find(db, id).await? else {
        return Ok(price);
    };
    if !sub.live() || sub.held_at.is_some() {
        return Ok(price);
    }
    let bp = ServicePlan::find(db, sub.service_plan_id)
        .await?
        .map(|p| p.parts_discount_bp)
        .unwrap_or(0);
    Ok(price - fee(price, bp))
}
