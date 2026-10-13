//! renox-billing's events, mirrored into the shop's plans.
//!
//! The gateway is the truth about money: renox-billing receives its
//! webhooks, keeps the `subscriptions` row and emits `SubscriptionCreated`,
//! `SubscriptionUpdated`, `SubscriptionCanceled`, `PaymentSucceeded` and
//! `PaymentFailed` (docs/billing.md, "Events and the activity log"). The
//! listeners registered in [`super::Plans`] turn them into what the
//! workshop needs:
//!
//! | renox-billing says | The shop does |
//! |---|---|
//! | the subscription is valid (paid or in its grace period) | a pending plan starts and its first visits are made; a hold ends and held visits are booked again |
//! | the plan changed (a swap) | the new plan's visits start on the next period's first day (`next_plan_id`, `swap_on`) |
//! | cancelled at the period's end | visits stop after that day (`ends_on`); resumed: they go on |
//! | past due | the plan goes on hold: its upcoming visits leave the workshop's days |
//! | ended | the plan is cancelled, its visits too |
//! | a payment succeeded / failed | an invoice row; the "plan started" or "renewed" mail, or the "payment failed" mail |
//!
//! A subscription belongs to a bike through its name (`bike-{id}`, see
//! [`PlanSubscription::billing_name`]) and to its user (the owner
//! `user:{id}`); one whose user isn't the plan's is ignored.

use renox::prelude::*;
use renox_billing::Subscription;

use super::billing::slug_of;
use super::model::{
    PlanInvoice, PlanSubscription, ServicePlan, SubscriptionStatus, bike_of_billing_name,
};
use super::visits;
use crate::app::accounts::preferences::Kind;
use crate::app::rentals::booking::to_local;
use crate::app::rentals::notify::{self, Notice, Tone};
use crate::app::workshop::model::CustomerBike;

/// The shop's plan a billing subscription is about: the bike's newest
/// plan that isn't cancelled, paid by the subscription's owner.
pub async fn local_of(db: &Db, subscription: &Subscription) -> Result<Option<PlanSubscription>> {
    if subscription.billable_type != "user" {
        return Ok(None);
    }
    let Some(bike) = bike_of_billing_name(&subscription.name) else {
        return Ok(None);
    };
    PlanSubscription::query()
        .where_(PlanSubscription::CUSTOMER_BIKE_ID.eq(bike))
        .where_(PlanSubscription::STATUS.ne(SubscriptionStatus::Cancelled))
        .where_(PlanSubscription::USER_ID.eq(subscription.billable_id))
        .order_by_desc(PlanSubscription::ID)
        .first(db)
        .await
}

/// Mirrors `subscription` (as renox-billing stored it after a webhook or a
/// change) into the bike's plan (see the module docs).
pub async fn mirror(state: &AppState, subscription: &Subscription) -> Result {
    let db = &state.db;
    let Some(mut sub) = local_of(db, subscription).await? else {
        return Ok(());
    };
    let Some(plan) = ServicePlan::query()
        .where_(ServicePlan::SLUG.eq(slug_of(&subscription.plan)))
        .first(db)
        .await?
    else {
        return Ok(());
    };
    if sub.gateway != subscription.gateway {
        sub.gateway = subscription.gateway.clone();
        sub.save_only(db, &["gateway"]).await?;
    }
    let today = visits::today(&state.config);
    if !subscription.valid() {
        if subscription.ended()
            || subscription.status == renox_billing::SubscriptionStatus::Canceled
        {
            if sub.status != SubscriptionStatus::Pending {
                visits::end(db, &mut sub).await?;
            }
        } else if subscription.past_due() && sub.status != SubscriptionStatus::Pending {
            visits::hold(db, &mut sub).await?;
        }
        return Ok(());
    }
    if sub.status == SubscriptionStatus::Pending {
        sub.status = SubscriptionStatus::Active;
        sub.service_plan_id = plan.id;
        sub.save_only(db, &["status", "service_plan_id"]).await?;
    } else if plan.id != sub.service_plan_id && sub.next_plan_id != Some(plan.id) {
        // A swap: from the next period (renox-billing without proration).
        sub.next_plan_id = Some(plan.id);
        sub.swap_on = Some(
            subscription
                .current_period_end
                .map(|at| to_local(&state.config, at).date())
                .unwrap_or(today),
        );
        sub.save_only(db, &["next_plan_id", "swap_on"]).await?;
    } else if plan.id == sub.service_plan_id && sub.next_plan_id.is_some() {
        // Swapped back before the change took effect.
        sub.next_plan_id = None;
        sub.swap_on = None;
        sub.save_only(db, &["next_plan_id", "swap_on"]).await?;
    }
    let ends_on = subscription
        .on_grace_period()
        .then(|| {
            subscription
                .ends_at
                .map(|at| to_local(&state.config, at).date())
        })
        .flatten();
    if ends_on != sub.ends_on {
        sub.ends_on = ends_on;
        sub.save_only(db, &["ends_on"]).await?;
        if let Some(last) = ends_on {
            visits::cancel_upcoming(db, &sub, Some(last)).await?;
        }
    }
    if sub.held_at.is_some() {
        visits::release(state, &mut sub).await?;
    } else {
        visits::plan_ahead(state, &mut sub).await?;
    }
    Ok(())
}

/// A payment renox-billing announced, as the listeners pass it on.
#[derive(Debug, Clone)]
pub struct Paid<'a> {
    pub gateway: &'a str,
    pub payment_id: &'a str,
    pub amount: i64,
    pub currency: &'a str,
    pub subscription: Option<&'a Subscription>,
    pub succeeded: bool,
}

/// Records a payment as the plan's invoice, and tells the customer: the
/// plan started (its first payment), renewed, or the payment failed (the
/// plan goes on hold until it is paid).
pub async fn payment(state: &AppState, paid: Paid<'_>) -> Result {
    let db = &state.db;
    let Some(subscription) = paid.subscription else {
        return Ok(());
    };
    let Some(mut sub) = local_of(db, subscription).await? else {
        return Ok(());
    };
    // `USD` for card plans, `IDR` for Xendit's (Stripe writes `usd`).
    let currency = paid.currency.to_ascii_uppercase();
    let earlier_paid = PlanInvoice::query()
        .where_(PlanInvoice::PLAN_SUBSCRIPTION_ID.eq(sub.id))
        .where_(PlanInvoice::PAID.eq(true))
        .where_(PlanInvoice::PAYMENT_ID.ne(paid.payment_id))
        .exists(db)
        .await?;
    match PlanInvoice::query()
        .where_(PlanInvoice::GATEWAY.eq(paid.gateway))
        .where_(PlanInvoice::PAYMENT_ID.eq(paid.payment_id))
        .first(db)
        .await?
    {
        Some(mut invoice) => {
            if invoice.paid == paid.succeeded {
                return Ok(()); // told already
            }
            invoice.paid = paid.succeeded;
            invoice.amount = paid.amount;
            invoice.save_only(db, &["paid", "amount"]).await?;
        }
        None => {
            PlanInvoice::create(
                db,
                PlanInvoice {
                    plan_subscription_id: sub.id,
                    gateway: paid.gateway.to_owned(),
                    payment_id: paid.payment_id.to_owned(),
                    amount: paid.amount,
                    currency: currency.clone(),
                    paid: paid.succeeded,
                    ..Default::default()
                },
            )
            .await?;
        }
    }
    let (kind, title, body, tone) = if !paid.succeeded {
        if sub.status == SubscriptionStatus::Active {
            visits::hold(db, &mut sub).await?;
        }
        (
            "plans-payment-failed",
            "plans.mail.failed.title",
            "plans.mail.failed.body",
            Tone::Warning,
        )
    } else {
        if sub.held_at.is_some() {
            visits::release(state, &mut sub).await?;
        }
        if earlier_paid {
            (
                "plans-renewed",
                "plans.mail.renewed.title",
                "plans.mail.renewed.body",
                Tone::Success,
            )
        } else {
            (
                "plans-started",
                "plans.mail.started.title",
                "plans.mail.started.body",
                Tone::Success,
            )
        }
    };
    tell(
        state,
        &sub,
        kind,
        title,
        body,
        tone,
        (paid.amount, &currency),
    )
    .await
}

/// A billing message to the plan's customer, with the amount and the link
/// to their plan.
async fn tell(
    state: &AppState,
    sub: &PlanSubscription,
    kind: &'static str,
    title: &'static str,
    body: &'static str,
    tone: Tone,
    (amount, currency): (i64, &str),
) -> Result {
    let Some(bike) = CustomerBike::find(&state.db, sub.customer_bike_id).await? else {
        return Ok(());
    };
    let Some(customer) =
        crate::app::accounts::model::Customer::find(&state.db, bike.customer_id).await?
    else {
        return Ok(());
    };
    let plan = ServicePlan::find(&state.db, sub.service_plan_id).await?;
    let url = crate::app::rentals::link(state, "plans.show", Some(sub.id))?;
    let amount = crate::money::format(amount, currency, &state.current_lang().locale);
    let next = sub
        .next_visit_on
        .map(|d| d.to_string())
        .unwrap_or_else(|| "—".into());
    notify::customer(
        state,
        &customer,
        Kind::Plan,
        &Notice::new(kind, title, body)
            .param("plan", plan.map(|p| p.name).unwrap_or_default())
            .param("bike", &bike.name)
            .param("amount", &amount)
            .row("plans.fields.amount", &amount)
            .row("plans.fields.next_visit", next)
            .tone(tone)
            .view("mail/plans/notice")
            .url(url),
    )
    .await
}

/// A user deleted their account: renox-billing cancels their subscriptions
/// at the gateway and deletes its rows; the shop ends their plans and
/// takes the upcoming visits off the workshop's days.
pub async fn account_deleted(db: &Db, user_id: i64) -> Result {
    let subs = PlanSubscription::query()
        .where_(PlanSubscription::USER_ID.eq(user_id))
        .where_(PlanSubscription::STATUS.ne(SubscriptionStatus::Cancelled))
        .get(db)
        .await?;
    for mut sub in subs {
        visits::end(db, &mut sub).await?;
    }
    Ok(())
}
