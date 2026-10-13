//! What the module announces. Listen with `App::listen`: send a welcome
//! mail, unlock features, warn about a failed payment. When the app has the
//! `Audit` module, a user's new subscriptions, cancellations and failed
//! payments are recorded in `audit_logs` too.
//!
//! ```
//! use renox::prelude::*;
//! use renox_billing::{PaymentFailed, SubscriptionCreated};
//!
//! # let _ =
//! App::new()
//!     .listen(|e: SubscriptionCreated, _state| async move {
//!         tracing::info!(plan = e.subscription.plan, "new subscription");
//!         Ok(())
//!     })
//!     .listen(|e: PaymentFailed, _state| async move {
//!         tracing::warn!(amount = e.amount, currency = e.currency, "a payment failed");
//!         Ok(())
//!     })
//! # ;
//! ```
//!
//! Each webhook is processed once (a provider's retry of the same event is
//! answered and skipped), and a change that changes nothing emits nothing.

use renox::prelude::*;

use crate::{Owner, Subscription, SubscriptionStatus};

/// A subscription was made: a trial without a payment method, or a
/// gateway's subscription arriving by webhook.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SubscriptionCreated {
    /// As stored.
    pub subscription: Subscription,
}

/// A subscription changed: its plan, status, trial, period or end (a
/// swap, a renewal, a cancellation, a resume, a failed payment…).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SubscriptionUpdated {
    /// As stored now.
    pub subscription: Subscription,
    /// The plan before.
    pub previous_plan: String,
    /// The status before.
    pub previous_status: SubscriptionStatus,
}

/// A subscription was canceled, now or at the end of its period
/// (`subscription.ends_at` says when access ends). Follows its
/// `SubscriptionUpdated`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SubscriptionCanceled {
    /// As stored now.
    pub subscription: Subscription,
}

/// Billing was paused ([`crate::Customer::pause`], or the gateway said so):
/// nothing is charged until it is taken up again. Follows its
/// `SubscriptionUpdated`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SubscriptionPaused {
    /// As stored now.
    pub subscription: Subscription,
}

/// A paused subscription is billed again ([`crate::Customer::unpause`], or
/// the gateway said so). Follows its `SubscriptionUpdated`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SubscriptionUnpaused {
    /// As stored now.
    pub subscription: Subscription,
}

/// A gateway took a payment.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PaymentSucceeded {
    /// The gateway's name.
    pub gateway: String,
    /// The payment's id there (Stripe's invoice, Xendit's cycle).
    pub payment_id: String,
    /// How much, in the currency's smallest unit.
    pub amount: i64,
    /// The ISO 4217 code.
    pub currency: String,
    /// Who paid, when known.
    pub owner: Option<Owner>,
    /// The subscription it paid for, when known.
    pub subscription: Option<Subscription>,
}

/// A payment failed; the gateway may retry it (the subscription is then
/// past due).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PaymentFailed {
    /// The gateway's name.
    pub gateway: String,
    /// The payment's id there.
    pub payment_id: String,
    /// How much was asked, in the currency's smallest unit.
    pub amount: i64,
    /// The ISO 4217 code.
    pub currency: String,
    /// Who should have paid, when known.
    pub owner: Option<Owner>,
    /// The subscription it was for, when known.
    pub subscription: Option<Subscription>,
}

impl Event for SubscriptionCreated {}
impl Event for SubscriptionUpdated {}
impl Event for SubscriptionCanceled {}
impl Event for SubscriptionPaused {}
impl Event for SubscriptionUnpaused {}
impl Event for PaymentSucceeded {}
impl Event for PaymentFailed {}
