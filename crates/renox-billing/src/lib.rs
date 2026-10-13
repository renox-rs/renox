//! Subscriptions for Renox apps (Laravel Cashier): plans declared in code,
//! free trials, upgrades and downgrades, cancellations with a grace period,
//! with Stripe and Xendit.
//!
//! ```
//! use renox::prelude::*;
//! use renox_billing::{Billing, Interval, Plan};
//!
//! # let _ =
//! App::new()
//!     .module(Auth::new().account()) // the account page shows the subscription
//!     .module(
//!         Billing::new()
//!             .plan(Plan::new("basic", "Basic").price(900, "USD", Interval::Month))
//!             .plan(Plan::new("pro", "Pro").price(1_900, "USD", Interval::Month).trial_days(14))
//!             .stripe(), // STRIPE_SECRET, STRIPE_WEBHOOK_SECRET, STRIPE_PRICE_BASIC, … in .env
//!     )
//! # ;
//! ```
//!
//! A plans page (`/billing`) sends the user to the gateway's payment page;
//! the subscription arrives by webhook (`/billing/webhooks/{gateway}`,
//! verified, stored once per event, applied by a queue worker) into the
//! `subscriptions` table, and the account page shows it with "Change
//! plan", "Cancel" and "Resume". In code, [`Billing::of`] answers
//! `subscribed()`, `on_trial()`, `on_grace_period()` and does `checkout`,
//! `swap`, `cancel`, `cancel_now`, `resume`, `pause` and `unpause`;
//! [`SubscriptionRoutes::require_subscription`] guards routes. The guide is
//! docs/billing.md in the Renox repository.

#![warn(missing_docs)]

use std::fmt;
use std::sync::Arc;

use renox::Config;
use renox::auth::events::AccountDeleted;
use renox::db::Migration;
use renox::prelude::*;

mod customer;
pub mod events;
pub mod gateway;
mod guard;
mod handlers;
mod model;
mod plan;
pub mod stripe;
mod sync;
mod webhook;
pub mod xendit;

pub use customer::{Billable, Customer, Owner};
pub use events::{
    PaymentFailed, PaymentSucceeded, SubscriptionCanceled, SubscriptionCreated, SubscriptionPaused,
    SubscriptionUnpaused, SubscriptionUpdated,
};
pub use gateway::{BoxFuture, Checkout, CheckoutRequest, Gateway, Notice, Payment, Remote};
pub use guard::SubscriptionRoutes;
pub use model::{BillingCustomer, Subscription, SubscriptionStatus};
pub use plan::{Interval, Plan};
pub use stripe::Stripe;
pub use xendit::Xendit;

const MIGRATIONS: &[Migration] = &[Migration::new(
    "00010101000900_create_billing_tables",
    "",
    Some(include_str!(
        "../migrations/00010101000900_create_billing_tables.down.sql"
    )),
)
.sqlite(
    include_str!("../migrations/00010101000900_create_billing_tables.up.sql"),
    None,
)
.postgres(
    include_str!("../migrations/00010101000900_create_billing_tables.postgres.up.sql"),
    None,
)];

/// The templates, compiled in. An app replaces one with a file of the same
/// name under its views directory (`resources/views/billing/plans.html`).
const VIEWS: &[(&str, &str)] = &[
    ("billing/plans.html", include_str!("../views/plans.html")),
    (
        "billing/section.html",
        include_str!("../views/section.html"),
    ),
];

/// The billing module: plans, gateways and how changes behave. Add it next
/// to `Auth` (with `.account()` for the subscription's card on the account
/// page).
#[derive(Clone)]
pub struct Billing {
    setup: Setup,
}

impl Default for Billing {
    fn default() -> Self {
        Self::new()
    }
}

/// The module's settings, provided to the whole app (`state.provided`).
#[derive(Clone, Default)]
pub(crate) struct Setup {
    pub(crate) plans: Vec<Plan>,
    pub(crate) gateways: Vec<Arc<dyn Gateway>>,
    pub(crate) generic_trials: bool,
    pub(crate) prorate: bool,
    pub(crate) redirect_to: Option<String>,
}

impl Billing {
    /// The module, with no plan and no gateway yet. Plan changes are
    /// prorated where the gateway does that.
    pub fn new() -> Self {
        Self {
            setup: Setup {
                prorate: true,
                ..Setup::default()
            },
        }
    }

    /// Sells `plan`. Plans show in the order they're added; one with the
    /// same key is replaced.
    pub fn plan(mut self, plan: Plan) -> Self {
        self.setup.plans.retain(|p| p.key != plan.key);
        self.setup.plans.push(plan);
        self
    }

    /// Adds Stripe, with `STRIPE_SECRET` and `STRIPE_WEBHOOK_SECRET` from
    /// the configuration (off while the secret is missing).
    pub fn stripe(self) -> Self {
        self.gateway(Stripe::from_config())
    }

    /// Adds Xendit, with `XENDIT_SECRET_KEY` and `XENDIT_CALLBACK_TOKEN`
    /// from the configuration (off while the key is missing).
    pub fn xendit(self) -> Self {
        self.gateway(Xendit::from_config())
    }

    /// Adds a gateway: [`Stripe::new`] with keys given in code, or one of
    /// your own ([`Gateway`]). One of the same name is replaced. Plans
    /// without [`Plan::via`] use the first gateway that is set up.
    pub fn gateway(mut self, gateway: impl Gateway) -> Self {
        self.setup.gateways.retain(|g| g.name() != gateway.name());
        self.setup.gateways.push(Arc::new(gateway));
        self
    }

    /// Offers plans' free trials without a payment method ("Start free
    /// trial" next to "Subscribe"); the days left carry over when the user
    /// subscribes.
    pub fn generic_trials(mut self) -> Self {
        self.setup.generic_trials = true;
        self
    }

    /// Plan changes take effect without charging or crediting the rest of
    /// the period.
    pub fn without_proration(mut self) -> Self {
        self.setup.prorate = false;
        self
    }

    /// Where users go after subscribing, changing plan or canceling (by
    /// default the account page, else `/`).
    pub fn redirect_to(mut self, path: impl Into<String>) -> Self {
        self.setup.redirect_to = Some(path.into());
        self
    }
}

impl fmt::Debug for Billing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Billing")
            .field(
                "plans",
                &self.setup.plans.iter().map(|p| &p.key).collect::<Vec<_>>(),
            )
            .field(
                "gateways",
                &self
                    .setup
                    .gateways
                    .iter()
                    .map(|g| g.name().to_owned())
                    .collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

impl Setup {
    /// The plan `key`; a refusal naming it otherwise.
    pub(crate) fn plan(&self, key: &str) -> Result<&Plan> {
        self.plans
            .iter()
            .find(|p| p.key == key)
            .ok_or_else(|| Error::BadRequest(format!("There's no plan `{key}`.")))
    }

    /// The gateway `name`, when its keys are set.
    pub(crate) fn gateway(&self, state: &AppState, name: &str) -> Option<Arc<dyn Gateway>> {
        self.gateways
            .iter()
            .find(|g| g.name() == name && g.configured(&state.config))
            .cloned()
    }

    /// The gateway that sells `plan`.
    pub(crate) fn gateway_for(&self, state: &AppState, plan: &Plan) -> Result<Arc<dyn Gateway>> {
        let found = match &plan.gateway {
            Some(name) => self.gateway(state, name),
            None => self
                .gateways
                .iter()
                .find(|g| g.configured(&state.config))
                .cloned(),
        };
        found.ok_or_else(|| {
            renox::anyhow::anyhow!(
                "no payment gateway is set up for the plan `{}`: set its keys in .env \
                 (STRIPE_SECRET, XENDIT_SECRET_KEY, …)",
                plan.key
            )
            .into()
        })
    }

    /// The plan whose price id at `gateway` is `price`.
    pub(crate) fn plan_by_price(
        &self,
        config: &Config,
        gateway: &str,
        price: &str,
    ) -> Option<&Plan> {
        self.plans
            .iter()
            .find(|p| p.price_id_for(gateway, config).as_deref() == Some(price))
    }
}

/// The module's settings, as the module provided them.
pub(crate) fn setup(state: &AppState) -> Result<Arc<Setup>> {
    state.provided::<Setup>().ok_or_else(|| {
        renox::anyhow::anyhow!("renox-billing: add the module with App::module(Billing::new()…)")
            .into()
    })
}

impl Module for Billing {
    fn name(&self) -> &'static str {
        "billing"
    }

    fn migrations(&self) -> &'static [Migration] {
        MIGRATIONS
    }

    fn routes(&self) -> Routes {
        handlers::routes()
    }

    fn register(&self, app: &mut Registry) {
        app.provide(self.setup.clone());
        app.webhook::<webhook::BillingWebhook>();
        app.templates(|env| {
            for (name, source) in VIEWS {
                // The app's own file of that name wins.
                if env.get_template(name).is_err() {
                    renox::view::add_template(env, *name, *source)
                        .expect("a built-in template compiles");
                }
            }
        });
        app.account_section("billing/section.html", 30, |user, state| async move {
            let setup = setup(&state)?;
            let billing = Billing::of(&state, &user);
            let current = match billing.subscription().await? {
                Some(s) => Some(handlers::Current::of(&state, &setup, &billing, s).await?),
                None => None,
            };
            Ok(renox::serde_json::json!({
                "current": current,
                "plans_url": state.url("billing.plans", &[])?,
            }))
        });
        // A deleted account's subscriptions end at the gateway too.
        app.listen(|e: AccountDeleted, state| async move {
            forget(&state, &Owner::new("user", e.user_id)).await
        });
        // Recorded in the activity log when the app has the `Audit` module.
        app.listen(|e: SubscriptionCreated, state| async move {
            let s = &e.subscription;
            audit(&state, s.owner(), "billing.subscribed", s, None).await
        })
        .listen(|e: SubscriptionCanceled, state| async move {
            let s = &e.subscription;
            audit(&state, s.owner(), "billing.canceled", s, None).await
        })
        .listen(|e: PaymentFailed, state| async move {
            match (&e.owner, &e.subscription) {
                (Some(owner), Some(s)) => {
                    audit(
                        &state,
                        owner.clone(),
                        "billing.payment_failed",
                        s,
                        Some(e.amount),
                    )
                    .await
                }
                _ => Ok(()),
            }
        });
    }
}

/// Ends `owner`'s running subscriptions at their gateways (a failure is
/// logged) and deletes their billing rows.
async fn forget(state: &AppState, owner: &Owner) -> Result {
    let setup = setup(state)?;
    for subscription in Subscription::of(&state.db, owner).await? {
        if subscription.ended() || subscription.gateway_id.is_none() {
            continue;
        }
        if let Some(gateway) = setup.gateway(state, &subscription.gateway)
            && let Err(err) = gateway.cancel(state, &subscription, false).await
        {
            tracing::warn!(error = ?err, subscription = subscription.id, "billing: couldn't cancel a deleted account's subscription");
        }
    }
    for table in ["subscriptions", "billing_customers"] {
        renox::db::sql(format!(
            "DELETE FROM {table} WHERE billable_type = ? AND billable_id = ?"
        ))
        .bind(owner.kind.as_str())
        .bind(owner.id)
        .execute(&state.db)
        .await?;
    }
    Ok(())
}

/// Records a user's billing change when the app has the `Audit` module.
async fn audit(
    state: &AppState,
    owner: Owner,
    action: &str,
    subscription: &Subscription,
    amount: Option<i64>,
) -> Result {
    if owner.kind != "user" {
        return Ok(());
    }
    let has_log = renox::db::sql("SELECT COUNT(*) FROM audit_logs WHERE 1 = 0")
        .scalar::<i64>(&state.db)
        .await
        .is_ok();
    if has_log {
        let entry =
            renox::audit::Entry::new(action)
                .user(owner.id)
                .data(renox::serde_json::json!({
                    "plan": subscription.plan,
                    "gateway": subscription.gateway,
                    "amount": amount,
                }));
        renox::audit::record(&state.db, entry).await?;
    }
    Ok(())
}

/// Compiles the Rust in docs/billing.md (the guide) as doctests.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/billing.md")]
pub struct Guide;
