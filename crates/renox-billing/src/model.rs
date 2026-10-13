//! The tables: `subscriptions` and `billing_customers`.

use renox::prelude::*;
use serde::Serialize;

use crate::Owner;

/// Where a subscription is, as the gateway last said (`subscriptions.status`).
#[derive(DbEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum SubscriptionStatus {
    /// In its free trial.
    Trialing,
    /// Paid for.
    Active,
    /// A payment failed; the gateway is retrying it. Not valid until paid.
    PastDue,
    /// Canceled: see [`Subscription::ends_at`] for when access ends.
    Canceled,
    /// Billing is paused ([`crate::Customer::pause`]): nothing is charged
    /// and access is not valid until it is taken up again.
    Paused,
    /// Started, not paid for yet (the customer left the payment page).
    #[default]
    Incomplete,
}

/// A subscription of an owner (a user, or what the app bills, such as a
/// team) to a plan. An owner keeps one row per subscription they took;
/// [`crate::Customer::subscription`] is the newest of a name.
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "subscriptions")]
pub struct Subscription {
    /// The row's id.
    pub id: i64,
    /// What kind of owner: `user`, or the app's name for it (`team`).
    pub billable_type: String,
    /// The owner's id.
    pub billable_id: i64,
    /// Which of the owner's subscriptions: `default` unless the app sells
    /// more than one thing.
    pub name: String,
    /// The plan's key.
    pub plan: String,
    /// The gateway's name; empty for a trial without a payment method.
    pub gateway: String,
    /// The subscription's id at the gateway; `None` until it made one.
    pub gateway_id: Option<String>,
    /// As the gateway last said.
    pub status: SubscriptionStatus,
    /// When the free trial ends.
    pub trial_ends_at: Option<DateTime>,
    /// When access ends after a cancellation; `None` while not canceled.
    pub ends_at: Option<DateTime>,
    /// When the period paid for ends: the next charge.
    pub current_period_end: Option<DateTime>,
    /// The newest gateway event applied (unix seconds).
    #[serde(skip)]
    pub synced_at: Option<i64>,
    /// When it was made.
    pub created_at: Option<DateTime>,
    /// When it last changed.
    pub updated_at: Option<DateTime>,
}

impl Subscription {
    /// Whether the owner has what they pay for: on a trial, paid for and
    /// not canceled, or canceled with time left (the grace period). Past
    /// due, paused and incomplete subscriptions aren't valid.
    pub fn valid(&self) -> bool {
        !self.paused()
            && (self.on_trial()
                || self.on_grace_period()
                || (self.status == SubscriptionStatus::Active && self.ends_at.is_none()))
    }

    /// Whether it's in its free trial (with a gateway or without).
    pub fn on_trial(&self) -> bool {
        !self.ended()
            && self.status != SubscriptionStatus::Incomplete
            && self.trial_ends_at.is_some_and(|at| at > renox::db::now())
    }

    /// Whether it was canceled (it may still run until [`Subscription::ends_at`]).
    pub fn canceled(&self) -> bool {
        self.ends_at.is_some()
    }

    /// Whether it was canceled and runs until `ends_at`.
    pub fn on_grace_period(&self) -> bool {
        self.ends_at.is_some_and(|at| at > renox::db::now())
            && self.status != SubscriptionStatus::Incomplete
    }

    /// Whether it was canceled and has run out.
    pub fn ended(&self) -> bool {
        self.ends_at.is_some_and(|at| at <= renox::db::now())
    }

    /// Whether billing is paused ([`crate::Customer::pause`]).
    pub fn paused(&self) -> bool {
        self.status == SubscriptionStatus::Paused
    }

    /// Whether a payment failed and the gateway is retrying it.
    pub fn past_due(&self) -> bool {
        self.status == SubscriptionStatus::PastDue
    }

    /// Whether it's on the plan `key`.
    pub fn has_plan(&self, key: &str) -> bool {
        self.plan == key
    }

    /// Whether it's a trial without a payment method (no gateway yet).
    pub fn is_generic_trial(&self) -> bool {
        self.gateway.is_empty()
    }

    /// Who it belongs to, as an [`Owner`] (without email and name).
    pub fn owner(&self) -> Owner {
        Owner::new(self.billable_type.clone(), self.billable_id)
    }

    /// The subscription with this id at `gateway`.
    pub async fn find_at(db: &Db, gateway: &str, gateway_id: &str) -> Result<Option<Self>> {
        Self::where_eq("gateway", gateway)
            .where_eq("gateway_id", gateway_id)
            .first(db)
            .await
    }

    /// Every subscription of `owner`, newest first.
    pub async fn of(db: &Db, owner: &Owner) -> Result<Vec<Self>> {
        Self::where_eq("billable_type", owner.kind.as_str())
            .where_eq("billable_id", owner.id)
            .order_by_desc("id")
            .get(db)
            .await
    }

    /// `owner`'s newest subscription named `name`.
    pub async fn latest(db: &Db, owner: &Owner, name: &str) -> Result<Option<Self>> {
        Self::where_eq("billable_type", owner.kind.as_str())
            .where_eq("billable_id", owner.id)
            .where_eq("name", name)
            .order_by_desc("id")
            .first(db)
            .await
    }
}

/// An owner's customer id at a gateway (one per gateway).
#[derive(Model, Serialize, Default, Debug, Clone)]
#[model(table = "billing_customers")]
pub struct BillingCustomer {
    /// The row's id.
    pub id: i64,
    /// What kind of owner (`user`).
    pub billable_type: String,
    /// The owner's id.
    pub billable_id: i64,
    /// The gateway's name.
    pub gateway: String,
    /// The customer's id there.
    pub gateway_id: String,
    /// When it was made.
    pub created_at: Option<DateTime>,
    /// When it last changed.
    pub updated_at: Option<DateTime>,
}

impl BillingCustomer {
    /// `owner`'s customer at `gateway`, if one was made.
    pub async fn of(db: &Db, owner: &Owner, gateway: &str) -> Result<Option<Self>> {
        Self::where_eq("billable_type", owner.kind.as_str())
            .where_eq("billable_id", owner.id)
            .where_eq("gateway", gateway)
            .first(db)
            .await
    }

    /// The customer `gateway_id` at `gateway`.
    pub async fn find_at(db: &Db, gateway: &str, gateway_id: &str) -> Result<Option<Self>> {
        Self::where_eq("gateway", gateway)
            .where_eq("gateway_id", gateway_id)
            .first(db)
            .await
    }
}
