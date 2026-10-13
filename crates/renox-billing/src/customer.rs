//! Who pays ([`Owner`], [`Billable`]) and what they can do
//! ([`Customer`]): Laravel Cashier's billable methods.

use std::sync::Arc;

use renox::auth::User;
use renox::prelude::*;
use serde::Serialize;

use crate::events::SubscriptionCreated;
use crate::model::{BillingCustomer, Subscription, SubscriptionStatus};
use crate::sync::{apply, emit_changes};
use crate::{Billing, CheckoutRequest, Remote, Setup};

/// Who pays: a kind (`user`, or the app's name for what it bills, such
/// as `team`) and an id, with an email address and a name for the
/// gateway's customer record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Owner {
    /// What kind of owner: `user`, `team`, …
    pub kind: String,
    /// Its id.
    pub id: i64,
    /// The address the gateway sends receipts to.
    pub email: Option<String>,
    /// The name on the gateway's customer record.
    pub name: Option<String>,
}

impl Owner {
    /// The owner `id` of kind `kind`.
    pub fn new(kind: impl Into<String>, id: i64) -> Self {
        Self {
            kind: kind.into(),
            id,
            email: None,
            name: None,
        }
    }

    /// A user, with their address and name.
    pub fn user(user: &User) -> Self {
        Self::new("user", user.id)
            .email(user.email.clone())
            .name(user.name.clone())
    }

    /// With the address the gateway sends receipts to.
    pub fn email(mut self, email: impl Into<String>) -> Self {
        self.email = Some(email.into()).filter(|e| !e.is_empty());
        self
    }

    /// With the name on the gateway's customer record.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into()).filter(|n| !n.trim().is_empty());
        self
    }

    /// `kind:id`, e.g. `user:5`: what the gateways store as `renox_billable`.
    pub fn key(&self) -> String {
        format!("{}:{}", self.kind, self.id)
    }

    /// The owner of a [`Owner::key`].
    pub fn from_key(key: &str) -> Option<Self> {
        let (kind, id) = key.rsplit_once(':')?;
        let id = id.parse().ok()?;
        (!kind.is_empty()).then(|| Self::new(kind, id))
    }

    /// Whether this is the user `user_id`.
    pub fn is_user(&self, user_id: i64) -> bool {
        self.kind == "user" && self.id == user_id
    }
}

/// What can subscribe. Users are billable as they are; for a team (or an
/// organization…), say which [`Owner`] it is:
///
/// ```
/// use renox_billing::{Billable, Owner};
///
/// struct Team {
///     id: i64,
///     name: String,
///     billing_email: String,
/// }
///
/// impl Billable for Team {
///     fn owner(&self) -> Owner {
///         Owner::new("team", self.id)
///             .name(self.name.clone())
///             .email(self.billing_email.clone())
///     }
/// }
/// ```
pub trait Billable {
    /// The owner the subscriptions belong to.
    fn owner(&self) -> Owner;
}

impl Billable for User {
    fn owner(&self) -> Owner {
        Owner::user(self)
    }
}

impl Billable for AuthUser {
    fn owner(&self) -> Owner {
        Owner::user(self)
    }
}

impl Billable for Owner {
    fn owner(&self) -> Owner {
        self.clone()
    }
}

/// An owner's subscriptions, and what they can do with them: from
/// [`Billing::of`]. Every method is about one subscription name,
/// `default` unless [`Customer::named`] says another.
///
/// ```
/// # use renox::prelude::*;
/// use renox_billing::Billing;
///
/// async fn dashboard(State(state): State<AppState>, user: AuthUser) -> Result<View> {
///     let billing = Billing::of(&state, &*user);
///     let subscribed = billing.subscribed().await?;
///     let on_trial = billing.on_trial().await?;
///     let pro = billing.subscribed_to("pro").await?;
///     Ok(view("dashboard.html", context! { subscribed, on_trial, pro }))
/// }
/// ```
pub struct Customer<'a> {
    state: &'a AppState,
    owner: Owner,
    name: String,
}

impl Billing {
    /// `billable`'s subscriptions (a [`User`], or any [`Billable`]).
    pub fn of<'a>(state: &'a AppState, billable: &impl Billable) -> Customer<'a> {
        Customer {
            state,
            owner: billable.owner(),
            name: "default".into(),
        }
    }
}

impl Customer<'_> {
    /// About the subscription named `name` rather than `default`, for an
    /// app that sells more than one thing.
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Who this is.
    pub fn owner(&self) -> &Owner {
        &self.owner
    }

    fn setup(&self) -> Result<Arc<Setup>> {
        crate::setup(self.state)
    }

    /// The newest subscription of this name, whatever its state.
    pub async fn subscription(&self) -> Result<Option<Subscription>> {
        Subscription::latest(&self.state.db, &self.owner, &self.name).await
    }

    /// Every subscription the owner has had, of any name, newest first.
    pub async fn subscriptions(&self) -> Result<Vec<Subscription>> {
        Subscription::of(&self.state.db, &self.owner).await
    }

    /// Whether the owner has a [valid](Subscription::valid) subscription:
    /// a trial, paid for, or canceled with time left.
    pub async fn subscribed(&self) -> Result<bool> {
        Ok(self.subscription().await?.is_some_and(|s| s.valid()))
    }

    /// Whether they're subscribed to the plan `key`.
    pub async fn subscribed_to(&self, key: &str) -> Result<bool> {
        Ok(self
            .subscription()
            .await?
            .is_some_and(|s| s.valid() && s.has_plan(key)))
    }

    /// Whether the subscription is in its free trial.
    pub async fn on_trial(&self) -> Result<bool> {
        Ok(self.subscription().await?.is_some_and(|s| s.on_trial()))
    }

    /// Whether it was canceled and still runs until its end.
    pub async fn on_grace_period(&self) -> Result<bool> {
        Ok(self
            .subscription()
            .await?
            .is_some_and(|s| s.on_grace_period()))
    }

    /// Whether the owner may still get a plan's free trial: they've never
    /// had a subscription of this name (a trial without a payment method
    /// that is still running carries over to the checkout).
    pub async fn trial_available(&self) -> Result<bool> {
        Ok(self.subscription().await?.is_none())
    }

    /// The owner's customer id at `gateway`, made there on first use.
    pub async fn customer_id(&self, gateway: &str) -> Result<String> {
        if let Some(customer) = BillingCustomer::of(&self.state.db, &self.owner, gateway).await? {
            return Ok(customer.gateway_id);
        }
        let setup = self.setup()?;
        let gateway = setup.gateway(self.state, gateway).ok_or_else(|| {
            renox::anyhow::anyhow!("the payment gateway `{gateway}` isn't set up")
        })?;
        let id = gateway.create_customer(self.state, &self.owner).await?;
        BillingCustomer::create(
            &self.state.db,
            BillingCustomer {
                billable_type: self.owner.kind.clone(),
                billable_id: self.owner.id,
                gateway: gateway.name().to_owned(),
                gateway_id: id.clone(),
                ..Default::default()
            },
        )
        .await?;
        Ok(id)
    }

    /// Starts subscribing to the plan `key`: answers the gateway's page
    /// where the customer pays (redirect them there). They come back to the
    /// `billing.return` route; the subscription arrives by webhook.
    ///
    /// The plan's free trial applies to a first subscription; a running
    /// trial without a payment method carries its days over, and a
    /// canceled subscription's remaining time is not charged again.
    pub async fn checkout(&self, key: &str) -> Result<String> {
        let setup = self.setup()?;
        let plan = setup.plan(key)?.clone();
        let gateway = setup.gateway_for(self.state, &plan)?;
        let current = self.subscription().await?;
        if let Some(current) = &current
            && (current.valid() || current.paused())
            && !current.is_generic_trial()
            && !current.canceled()
        {
            return Err(Error::BadRequest(
                "You're subscribed already: change your plan instead.".into(),
            ));
        }
        let now = renox::db::now();
        let trial_ends_at = match &current {
            None if plan.trial_days > 0 => {
                Some(now + renox::chrono::Duration::days(i64::from(plan.trial_days)))
            }
            None => None,
            Some(s) if s.is_generic_trial() && s.on_trial() => s.trial_ends_at,
            Some(s) if s.on_grace_period() => s.ends_at,
            Some(_) => None,
        };
        let customer_id = self.customer_id(gateway.name()).await?;
        let request = CheckoutRequest {
            owner: self.owner.clone(),
            customer_id,
            name: self.name.clone(),
            price_id: plan.price_id_for(gateway.name(), &self.state.config),
            plan,
            trial_ends_at,
            success_url: self.state.absolute_url("billing.return", &[])?,
            cancel_url: self.state.absolute_url("billing.plans", &[])?,
        };
        let checkout = gateway.checkout(self.state, &request).await?;
        Ok(checkout.url)
    }

    /// Starts the plan `key`'s free trial without a payment method (a
    /// "generic trial"). The owner subscribes with a payment method before
    /// it ends ([`Customer::checkout`] keeps the days left).
    pub async fn start_trial(&self, key: &str) -> Result<Subscription> {
        let setup = self.setup()?;
        let plan = setup.plan(key)?;
        if plan.trial_days == 0 {
            return Err(Error::BadRequest(format!(
                "{} has no free trial.",
                plan.label
            )));
        }
        if !self.trial_available().await? {
            return Err(Error::BadRequest(
                "You've had a subscription already, so there's no free trial.".into(),
            ));
        }
        let ends = renox::db::now() + renox::chrono::Duration::days(i64::from(plan.trial_days));
        let subscription = Subscription::create(
            &self.state.db,
            Subscription {
                billable_type: self.owner.kind.clone(),
                billable_id: self.owner.id,
                name: self.name.clone(),
                plan: plan.key.clone(),
                status: SubscriptionStatus::Trialing,
                trial_ends_at: Some(ends),
                ..Default::default()
            },
        )
        .await?;
        self.state
            .emit(SubscriptionCreated {
                subscription: subscription.clone(),
            })
            .await?;
        Ok(subscription)
    }

    /// The subscription to change: valid and not ended.
    async fn current(&self) -> Result<Subscription> {
        self.subscription()
            .await?
            .filter(|s| s.valid() || (s.paused() && !s.ended()))
            .ok_or_else(|| Error::BadRequest("You're not subscribed.".into()))
    }

    /// Moves the subscription to the plan `key` (an upgrade or a
    /// downgrade), prorated where the gateway does that unless the module
    /// says [`Billing::without_proration`].
    pub async fn swap(&self, key: &str) -> Result<Subscription> {
        let setup = self.setup()?;
        let plan = setup.plan(key)?.clone();
        let current = self.current().await?;
        if current.plan == plan.key {
            return Ok(current);
        }
        if current.canceled() {
            return Err(Error::BadRequest(
                "Resume the subscription before changing its plan.".into(),
            ));
        }
        if current.is_generic_trial() {
            let mut changed = current.clone();
            changed.plan = plan.key.clone();
            changed.save(&self.state.db).await?;
            emit_changes(self.state, &current, &changed).await?;
            return Ok(changed);
        }
        let gateway = setup.gateway_for(self.state, &plan)?;
        if gateway.name() != current.gateway {
            return Err(Error::BadRequest(format!(
                "{} is sold through another payment provider: cancel, then subscribe to it.",
                plan.label
            )));
        }
        let remote = gateway
            .swap(self.state, &current, &plan, setup.prorate)
            .await?;
        let remote = Remote {
            plan: Some(plan.key.clone()),
            price_id: None,
            ..remote
        };
        self.applied(&setup, &current, remote).await
    }

    /// Cancels at the end of the period paid for (or of the trial): the
    /// owner keeps access until then (the grace period).
    pub async fn cancel(&self) -> Result<Subscription> {
        self.cancel_with(true).await
    }

    /// Cancels now: access ends at once.
    pub async fn cancel_now(&self) -> Result<Subscription> {
        self.cancel_with(false).await
    }

    async fn cancel_with(&self, at_period_end: bool) -> Result<Subscription> {
        let setup = self.setup()?;
        let current = self.current().await?;
        if at_period_end && current.canceled() {
            return Ok(current);
        }
        if current.is_generic_trial() {
            let mut changed = current.clone();
            changed.status = SubscriptionStatus::Canceled;
            changed.ends_at = Some(match (at_period_end, changed.trial_ends_at) {
                (true, Some(at)) => at,
                _ => renox::db::now(),
            });
            changed.save(&self.state.db).await?;
            emit_changes(self.state, &current, &changed).await?;
            return Ok(changed);
        }
        let gateway = setup.gateway(self.state, &current.gateway).ok_or_else(|| {
            renox::anyhow::anyhow!("the payment gateway `{}` isn't set up", current.gateway)
        })?;
        let remote = gateway.cancel(self.state, &current, at_period_end).await?;
        self.applied(&setup, &current, remote).await
    }

    /// Whether the subscription can be [resumed](Customer::resume): in its
    /// grace period, at a gateway that resumes.
    pub async fn can_resume(&self) -> Result<bool> {
        let Some(current) = self.subscription().await? else {
            return Ok(false);
        };
        if !current.on_grace_period() {
            return Ok(false);
        }
        if current.is_generic_trial() {
            return Ok(true);
        }
        let setup = self.setup()?;
        Ok(setup
            .gateway(self.state, &current.gateway)
            .is_some_and(|g| g.resumes()))
    }

    /// Takes back a cancellation during the grace period: charging goes on
    /// as before.
    pub async fn resume(&self) -> Result<Subscription> {
        let setup = self.setup()?;
        let current = self.current().await?;
        if !current.on_grace_period() {
            return Err(Error::BadRequest(
                "Only a canceled subscription that hasn't ended can be resumed.".into(),
            ));
        }
        if current.is_generic_trial() {
            let mut changed = current.clone();
            changed.status = SubscriptionStatus::Trialing;
            changed.ends_at = None;
            changed.save(&self.state.db).await?;
            emit_changes(self.state, &current, &changed).await?;
            return Ok(changed);
        }
        let gateway = setup.gateway(self.state, &current.gateway).ok_or_else(|| {
            renox::anyhow::anyhow!("the payment gateway `{}` isn't set up", current.gateway)
        })?;
        let remote = gateway.resume(self.state, &current).await?;
        self.applied(&setup, &current, remote).await
    }

    /// Whether billing can be [paused](Customer::pause): an active
    /// subscription with a payment method, not canceled, at a gateway that
    /// pauses.
    pub async fn can_pause(&self) -> Result<bool> {
        let Some(current) = self.subscription().await? else {
            return Ok(false);
        };
        if current.is_generic_trial()
            || current.canceled()
            || current.status != SubscriptionStatus::Active
        {
            return Ok(false);
        }
        let setup = self.setup()?;
        Ok(setup
            .gateway(self.state, &current.gateway)
            .is_some_and(|g| g.pauses()))
    }

    /// Stops billing for a while: nothing is charged and the subscription
    /// is not [valid](Subscription::valid) (the guards turn the owner
    /// away) until [`Customer::unpause`], or, with `resumes_at`, until the
    /// gateway takes it up again then. A gateway that can't pause
    /// answers a refusal ([`Error::BadRequest`]); so does a subscription
    /// that isn't active.
    pub async fn pause(&self, resumes_at: Option<DateTime>) -> Result<Subscription> {
        let setup = self.setup()?;
        let current = self.current().await?;
        if current.paused() {
            return Ok(current);
        }
        if current.is_generic_trial() || current.status != SubscriptionStatus::Active {
            return Err(Error::BadRequest(
                "Only a subscription that is paid for can be paused.".into(),
            ));
        }
        if current.canceled() {
            return Err(Error::BadRequest(
                "A canceled subscription can't be paused: resume it first.".into(),
            ));
        }
        let gateway = setup.gateway(self.state, &current.gateway).ok_or_else(|| {
            renox::anyhow::anyhow!("the payment gateway `{}` isn't set up", current.gateway)
        })?;
        let remote = gateway.pause(self.state, &current, resumes_at).await?;
        let remote = Remote {
            status: Some(SubscriptionStatus::Paused),
            ..remote
        };
        self.applied(&setup, &current, remote).await
    }

    /// Takes billing up again after [`Customer::pause`].
    pub async fn unpause(&self) -> Result<Subscription> {
        let setup = self.setup()?;
        let current = self.current().await?;
        if !current.paused() {
            return Err(Error::BadRequest(
                "Only a paused subscription can be taken up again.".into(),
            ));
        }
        let gateway = setup.gateway(self.state, &current.gateway).ok_or_else(|| {
            renox::anyhow::anyhow!("the payment gateway `{}` isn't set up", current.gateway)
        })?;
        let remote = gateway.unpause(self.state, &current).await?;
        let remote = Remote {
            status: remote
                .status
                .filter(|s| *s != SubscriptionStatus::Paused)
                .or(Some(SubscriptionStatus::Active)),
            ..remote
        };
        self.applied(&setup, &current, remote).await
    }

    /// Applies what the gateway answered to `current`, and reads it back.
    async fn applied(
        &self,
        setup: &Setup,
        current: &Subscription,
        remote: Remote,
    ) -> Result<Subscription> {
        // Only webhooks move `synced_at`: the gateway's clock orders them.
        let remote = Remote { at: None, ..remote };
        let gateway = current.gateway.clone();
        Ok(apply(self.state, setup, &gateway, remote)
            .await?
            .unwrap_or_else(|| current.clone()))
    }
}
