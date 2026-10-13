//! Storing what gateways say: a [`Remote`] becomes a `subscriptions` row,
//! a [`Payment`] an event.

use renox::prelude::*;

use crate::events::{
    PaymentFailed, PaymentSucceeded, SubscriptionCanceled, SubscriptionCreated, SubscriptionPaused,
    SubscriptionUnpaused, SubscriptionUpdated,
};
use crate::model::{BillingCustomer, Subscription, SubscriptionStatus};
use crate::{Owner, Payment, Remote, Setup};

/// Applies `remote` (from `gateway`) to its row, making the row when the
/// gateway made the subscription (its metadata or customer names the
/// owner). `None` when no owner is known (a subscription made outside the
/// app) or the plan is unknown. What is older than the newest event
/// applied to the row is ignored; applying the same thing twice changes
/// nothing (and emits nothing).
pub(crate) async fn apply(
    state: &AppState,
    setup: &Setup,
    gateway: &str,
    remote: Remote,
) -> Result<Option<Subscription>> {
    let db = &state.db;
    let existing = Subscription::find_at(db, gateway, &remote.id).await?;
    if let Some(row) = &existing
        && let (Some(at), Some(synced)) = (remote.at, row.synced_at)
        && at < synced
    {
        tracing::info!(
            gateway,
            subscription = remote.id,
            "an older billing event arrived late; ignored"
        );
        return Ok(existing);
    }
    // The plan: by the gateway's price id first (a plan changed at the
    // gateway's dashboard), then the metadata.
    let plan = remote
        .price_id
        .as_deref()
        .and_then(|price| setup.plan_by_price(&state.config, gateway, price))
        .map(|plan| plan.key.clone())
        .or_else(|| remote.plan.clone());
    let (before, mut row) = match existing {
        Some(row) => (Some(row.clone()), row),
        None => {
            let Some(owner) = owner_of(state, gateway, &remote).await? else {
                tracing::warn!(
                    gateway,
                    subscription = remote.id,
                    "a subscription for no known owner; ignored"
                );
                return Ok(None);
            };
            let Some(plan) = plan.clone().filter(|key| setup.plan(key).is_ok()) else {
                tracing::warn!(
                    gateway,
                    subscription = remote.id,
                    "a subscription to no known plan; ignored"
                );
                return Ok(None);
            };
            let name = remote.name.clone().unwrap_or_else(|| "default".into());
            // A running trial without a payment method becomes this
            // subscription.
            let pending = Subscription::latest(db, &owner, &name)
                .await?
                .filter(|s| s.gateway_id.is_none() && !s.canceled());
            match pending {
                Some(row) => (Some(row.clone()), row),
                None => (
                    None,
                    Subscription {
                        billable_type: owner.kind,
                        billable_id: owner.id,
                        name,
                        plan,
                        ..Default::default()
                    },
                ),
            }
        }
    };
    row.gateway = gateway.to_owned();
    row.gateway_id = Some(remote.id.clone());
    if let Some(key) = plan.filter(|key| setup.plan(key).is_ok()) {
        row.plan = key;
    }
    if let Some(status) = remote.status {
        row.status = status;
    }
    if let Some(at) = remote.trial_ends_at {
        row.trial_ends_at = at;
    }
    if let Some(at) = remote.current_period_end {
        row.current_period_end = at;
    }
    if let Some(at) = remote.ends_at {
        row.ends_at = at;
    }
    if row.status == SubscriptionStatus::Canceled && row.ends_at.is_none() {
        row.ends_at = Some(renox::db::now());
    }
    if let Some(at) = remote.at {
        row.synced_at = Some(row.synced_at.map_or(at, |synced| synced.max(at)));
    }
    let changed = before.as_ref().is_none_or(|before| differs(before, &row));
    if !changed {
        return Ok(Some(row));
    }
    row.save(db).await?;
    match &before {
        None => {
            state
                .emit(SubscriptionCreated {
                    subscription: row.clone(),
                })
                .await?
        }
        Some(before) => emit_changes(state, before, &row).await?,
    }
    Ok(Some(row))
}

/// Whether what the module shows differs (not `synced_at`).
fn differs(a: &Subscription, b: &Subscription) -> bool {
    a.gateway != b.gateway
        || a.gateway_id != b.gateway_id
        || a.plan != b.plan
        || a.status != b.status
        || a.trial_ends_at != b.trial_ends_at
        || a.ends_at != b.ends_at
        || a.current_period_end != b.current_period_end
        || a.synced_at != b.synced_at
}

/// `SubscriptionUpdated` when the plan, status or end changed,
/// `SubscriptionCanceled` when it was just canceled, and
/// `SubscriptionPaused` / `SubscriptionUnpaused` when billing stopped or
/// went on.
pub(crate) async fn emit_changes(
    state: &AppState,
    before: &Subscription,
    after: &Subscription,
) -> Result {
    let visible = before.plan != after.plan
        || before.status != after.status
        || before.ends_at != after.ends_at
        || before.trial_ends_at != after.trial_ends_at
        || before.current_period_end != after.current_period_end
        || before.gateway_id != after.gateway_id;
    if !visible {
        return Ok(());
    }
    state
        .emit(SubscriptionUpdated {
            subscription: after.clone(),
            previous_plan: before.plan.clone(),
            previous_status: before.status,
        })
        .await?;
    if before.ends_at.is_none() && after.ends_at.is_some() {
        state
            .emit(SubscriptionCanceled {
                subscription: after.clone(),
            })
            .await?;
    }
    if !before.paused() && after.paused() {
        state
            .emit(SubscriptionPaused {
                subscription: after.clone(),
            })
            .await?;
    } else if before.paused() && !after.paused() {
        state
            .emit(SubscriptionUnpaused {
                subscription: after.clone(),
            })
            .await?;
    }
    Ok(())
}

/// The owner of a subscription the module hasn't stored yet: its
/// `renox_billable` metadata, else its customer.
async fn owner_of(state: &AppState, gateway: &str, remote: &Remote) -> Result<Option<Owner>> {
    if let Some(owner) = remote.owner.as_deref().and_then(Owner::from_key) {
        return Ok(Some(owner));
    }
    let Some(customer) = &remote.customer_id else {
        return Ok(None);
    };
    Ok(BillingCustomer::find_at(&state.db, gateway, customer)
        .await?
        .map(|c| Owner::new(c.billable_type, c.billable_id)))
}

/// Announces a payment: `PaymentSucceeded` or `PaymentFailed`, with the
/// subscription and owner when they're known.
pub(crate) async fn payment(state: &AppState, gateway: &str, payment: Payment) -> Result {
    let subscription = match &payment.subscription_id {
        Some(id) => Subscription::find_at(&state.db, gateway, id).await?,
        None => None,
    };
    let owner = match (&subscription, &payment.customer_id) {
        (Some(s), _) => Some(s.owner()),
        (None, Some(customer)) => BillingCustomer::find_at(&state.db, gateway, customer)
            .await?
            .map(|c| Owner::new(c.billable_type, c.billable_id)),
        (None, None) => None,
    };
    if payment.succeeded {
        state
            .emit(PaymentSucceeded {
                gateway: gateway.to_owned(),
                payment_id: payment.id,
                amount: payment.amount,
                currency: payment.currency,
                owner,
                subscription,
            })
            .await
    } else {
        state
            .emit(PaymentFailed {
                gateway: gateway.to_owned(),
                payment_id: payment.id,
                amount: payment.amount,
                currency: payment.currency,
                owner,
                subscription,
            })
            .await
    }
}
