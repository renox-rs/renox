//! Storing what gateways say: a [`Remote`] becomes a `subscriptions` row,
//! a [`Payment`] an event.

use renox::prelude::*;

use crate::events::{
    DisputeLost, DisputeOpened, DisputeWon, PaymentFailed, PaymentLinkExpired, PaymentRefunded,
    PaymentSucceeded, RefundFailed, RefundSucceeded, SubscriptionCanceled, SubscriptionCreated,
    SubscriptionPaused, SubscriptionUnpaused, SubscriptionUpdated,
};
use crate::model::{BillingCustomer, Subscription, SubscriptionStatus};
use crate::payment::{
    BillingPayment, BillingRefund, DisputeState, DisputeUpdate, PaymentKind, PaymentState,
    PaymentStatus, PaymentUpdate, RefundStatus, RefundUpdate,
};
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
                reference_type: None,
                reference_id: None,
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
                reference_type: None,
                reference_id: None,
            })
            .await
    }
}

/// Applies a one-off payment notice from a webhook to its row (see
/// [`apply_status`]). A payment the module doesn't know is ignored.
pub(crate) async fn one_off(state: &AppState, gateway: &str, update: PaymentUpdate) -> Result {
    let Some(row) = BillingPayment::find_at(&state.db, gateway, &update.gateway_id).await? else {
        tracing::warn!(
            gateway,
            payment = update.gateway_id,
            "a notice for an unknown payment; ignored"
        );
        return Ok(());
    };
    let row = remember_intent(state, row, update.intent.as_deref()).await?;
    apply_status(state, row, update.status, update.at).await?;
    Ok(())
}

/// Stores the payment's second id (Stripe's PaymentIntent) on its row when
/// the gateway names one, so notices that carry only that id find the row.
pub(crate) async fn remember_intent(
    state: &AppState,
    mut row: BillingPayment,
    intent: Option<&str>,
) -> Result<BillingPayment> {
    if let Some(intent) = intent
        && row.gateway_intent.as_deref() != Some(intent)
    {
        row.gateway_intent = Some(intent.to_owned());
        row.save(&state.db).await?;
    }
    Ok(row)
}

/// Moves a payment to `status` and announces it, by the late-success rule:
/// a success always moves a pending, failed or expired payment to
/// succeeded; a failure or expiry never undoes a success (or anything after
/// it); between failed and expired the newest gateway time wins. Applying
/// what the row already says changes nothing and announces nothing.
pub(crate) async fn apply_status(
    state: &AppState,
    mut row: BillingPayment,
    status: PaymentStatus,
    at: Option<i64>,
) -> Result<BillingPayment> {
    use PaymentStatus::{Expired, Failed, Pending, Succeeded};
    let apply = match (row.status, status) {
        (Pending | Failed | Expired, Succeeded) | (Pending, Failed | Expired) => true,
        (Failed, Expired) | (Expired, Failed) => match (at, row.synced_at) {
            (Some(at), Some(synced)) => at >= synced,
            _ => true,
        },
        _ => false,
    };
    if !apply {
        return Ok(row);
    }
    row.status = status;
    if let Some(at) = at {
        row.synced_at = Some(row.synced_at.map_or(at, |synced| synced.max(at)));
    }
    row.save(&state.db).await?;
    let (gateway, owner) = (row.gateway.clone(), row.owner());
    let currency = row.amount.currency.code().to_owned();
    let gateway_id = row.gateway_id.clone().unwrap_or_default();
    let (reference_type, reference_id) = (row.reference_type.clone(), row.reference_id.clone());
    match status {
        Succeeded => {
            state
                .emit(PaymentSucceeded {
                    gateway,
                    payment_id: gateway_id,
                    amount: row.amount.minor,
                    currency,
                    owner,
                    subscription: None,
                    reference_type,
                    reference_id,
                })
                .await?
        }
        Expired if row.kind == PaymentKind::Link => {
            state
                .emit(PaymentLinkExpired {
                    payment: row.clone(),
                })
                .await?
        }
        _ => {
            state
                .emit(PaymentFailed {
                    gateway,
                    payment_id: gateway_id,
                    amount: row.amount.minor,
                    currency,
                    owner,
                    subscription: None,
                    reference_type,
                    reference_id,
                })
                .await?
        }
    }
    Ok(row)
}

/// Applies what a gateway says about a payment when asked: its status, then
/// its refunds, the way webhooks would.
pub(crate) async fn apply_state(
    state: &AppState,
    row: BillingPayment,
    reported: &PaymentState,
) -> Result {
    let gateway = row.gateway.clone();
    let id = row.gateway_id.clone();
    let row = remember_intent(state, row, reported.intent.as_deref()).await?;
    apply_status(state, row, reported.status, reported.at).await?;
    for refund in &reported.refunds {
        let mut refund = refund.clone();
        if refund.payment_gateway_id.is_none() {
            refund.payment_gateway_id = id.clone();
        }
        refund_update(state, &gateway, refund).await?;
    }
    Ok(())
}

/// Counts a succeeded refund of `amount` on `payment`: the payment becomes
/// refunded or partially refunded, and `RefundSucceeded` and
/// `PaymentRefunded` are announced.
pub(crate) async fn refund_done(
    state: &AppState,
    payment: &mut BillingPayment,
    amount: i64,
) -> Result {
    payment.refunded_amount += amount;
    payment.status = if payment.refunded_amount >= payment.amount.minor {
        PaymentStatus::Refunded
    } else {
        PaymentStatus::PartiallyRefunded
    };
    payment.save(&state.db).await?;
    state
        .emit(RefundSucceeded {
            payment: payment.clone(),
            amount,
        })
        .await?;
    state
        .emit(PaymentRefunded {
            payment: payment.clone(),
        })
        .await
}

/// Applies a refund notice. A pending refund becomes succeeded or failed
/// (a failed one may still turn out succeeded, a succeeded one never
/// changes); a refund the app didn't ask for (made at the gateway's
/// dashboard) is stored when the notice names its payment and amount.
pub(crate) async fn refund_update(state: &AppState, gateway: &str, update: RefundUpdate) -> Result {
    let db = &state.db;
    let mut payment = match &update.payment_gateway_id {
        Some(id) => BillingPayment::find_named(db, gateway, id).await?,
        None => None,
    };
    let existing = BillingRefund::where_eq("gateway_id", update.gateway_id.as_str())
        .first(db)
        .await?;
    let mut row = match existing {
        Some(row) => {
            if payment.as_ref().is_none_or(|p| p.id != row.payment_id) {
                payment = BillingPayment::find(db, row.payment_id)
                    .await?
                    .filter(|p| p.gateway == gateway);
            }
            row
        }
        None => {
            let (Some(found), Some(amount)) = (&payment, update.amount) else {
                tracing::warn!(
                    gateway,
                    refund = update.gateway_id,
                    "a notice for an unknown refund; ignored"
                );
                return Ok(());
            };
            BillingRefund::create(
                db,
                BillingRefund {
                    payment_id: found.id,
                    gateway_id: Some(update.gateway_id.clone()),
                    amount,
                    reason: update.reason.clone(),
                    ..Default::default()
                },
            )
            .await?
        }
    };
    let Some(mut payment) = payment else {
        return Ok(());
    };
    match (row.status, update.status) {
        (RefundStatus::Pending | RefundStatus::Failed, RefundStatus::Succeeded) => {
            if row.amount == 0 {
                row.amount = update.amount.unwrap_or(0);
            }
            row.status = RefundStatus::Succeeded;
            row.save(db).await?;
            refund_done(state, &mut payment, row.amount).await
        }
        (RefundStatus::Pending, RefundStatus::Failed) => {
            row.status = RefundStatus::Failed;
            if update.reason.is_some() {
                row.reason = update.reason.clone();
            }
            row.save(db).await?;
            state
                .emit(RefundFailed {
                    payment,
                    amount: row.amount,
                    reason: update.reason,
                })
                .await
        }
        _ => Ok(()),
    }
}

/// Applies a dispute notice to the disputed payment: opened (paid payments
/// only) marks it disputed with the evidence deadline; won puts it back to
/// what it was; lost counts the payment as given back. A notice that
/// repeats the state, or doesn't fit it, changes nothing.
pub(crate) async fn dispute(state: &AppState, gateway: &str, update: DisputeUpdate) -> Result {
    use PaymentStatus::{Disputed, PartiallyRefunded, Refunded, Succeeded};
    let db = &state.db;
    let Some(mut row) = BillingPayment::find_named(db, gateway, &update.payment_gateway_id).await?
    else {
        tracing::warn!(
            gateway,
            payment = update.payment_gateway_id,
            "a dispute for an unknown payment; ignored"
        );
        return Ok(());
    };
    match update.state {
        DisputeState::Opened if matches!(row.status, Succeeded | PartiallyRefunded | Refunded) => {
            row.status = Disputed;
            row.evidence_due_by = update.evidence_due_by;
            row.save(db).await?;
            state
                .emit(DisputeOpened {
                    evidence_due_by: row.evidence_due_by,
                    payment: row,
                })
                .await
        }
        DisputeState::Won if row.status == Disputed => {
            row.status = match row.refunded_amount {
                0 => Succeeded,
                n if n >= row.amount.minor => Refunded,
                _ => PartiallyRefunded,
            };
            row.evidence_due_by = None;
            row.save(db).await?;
            state.emit(DisputeWon { payment: row }).await
        }
        DisputeState::Lost if row.status == Disputed => {
            row.status = Refunded;
            row.refunded_amount = row.amount.minor;
            row.evidence_due_by = None;
            row.save(db).await?;
            state.emit(DisputeLost { payment: row }).await
        }
        _ => Ok(()),
    }
}
